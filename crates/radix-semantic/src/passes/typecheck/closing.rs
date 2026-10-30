//! The closing contract and conversation `close()` (DECISIONS D8.3).
//!
//! Norma declares `implendum Closable { functio close() → bivalens ⇥ textus }`
//! under `@ radix contract "closing"`. The compiler adds:
//!
//! - **Recognition**: the contract is found by its mark and its shape is
//!   checked once, like the ordering contract.
//! - **Built-in conformance**: `sermo` (bare or `sermo<O, R>`) satisfies
//!   `T implet Closable` without user code; it is the first implementer.
//! - **`s.close()`**: closes a conversation early. It returns whether this
//!   call closed the handle (`falsum` when it was already closed) and reports
//!   the conversation's error terminal on `⇥ textus`.
//! - **Cycle warning**: a reference cycle never reaches its last reference,
//!   so a closable value held inside one never auto-closes.
//!   `closable_in_cycle` names the field that can hold it.

use handroll::{FxHashMap, FxHashSet};
use radix_hir::{DefId, HirCallArg, HirInterface, HirItemKind, HirModule};
use radix_lexer::Span;
use radix_types::{AppliedArg, ContractRef, FuncSig, Primitive, Type, TypeId};

use super::TypeChecker;
use crate::radix_contract::{CLOSING_METHOD, RadixContractRole, contract_error};
use crate::{SemanticError, SemanticErrorKind, WarningKind};

/// `@ radix contract "closing"` on an implendum whose shape is not
/// `Name { close() → bivalens ⇥ textus }`.
const ISSUE_CLOSING_CONTRACT_SHAPE: &str = "radix_contract_closing_shape";

/// A second implendum claims the closing role in one unit.
const ISSUE_CLOSING_CONTRACT_DUPLICATE: &str = "radix_contract_duplicate";

/// A field of a type on a reference cycle can hold a closable value.
pub(crate) const ISSUE_CLOSABLE_IN_CYCLE: &str = "closable_in_cycle";

impl TypeChecker<'_> {
    /// Record a local implendum carrying `@ radix contract "closing"`.
    pub(super) fn register_local_closing_contract(
        &mut self,
        def_id: DefId,
        interface: &HirInterface,
        span: Span,
    ) {
        if !self.closing_contract_shape_ok(interface) {
            self.errors
                .push(contract_error(span, ISSUE_CLOSING_CONTRACT_SHAPE));
            return;
        }
        self.claim_closing_contract(def_id, span);
    }

    /// Record an imported implendum whose file interface names the role.
    pub(super) fn register_imported_closing_contract(&mut self, def_id: DefId) {
        if self
            .resolver
            .imported_contract_role(def_id)
            .and_then(RadixContractRole::parse)
            == Some(RadixContractRole::Closing)
        {
            self.claim_closing_contract(def_id, Span::default());
        }
    }

    fn claim_closing_contract(&mut self, def_id: DefId, span: Span) {
        match self.types.closing_contract {
            Some(existing) if existing != def_id => {
                self.errors
                    .push(contract_error(span, ISSUE_CLOSING_CONTRACT_DUPLICATE));
            }
            _ => self.types.closing_contract = Some(def_id),
        }
    }

    /// `Name { close() → bivalens ⇥ textus }`, nothing else.
    fn closing_contract_shape_ok(&self, interface: &HirInterface) -> bool {
        let [method] = interface.methods.as_slice() else {
            return false;
        };
        let names_close = self
            .interner
            .is_some_and(|interner| interner.resolve(method.name) == CLOSING_METHOD);
        let returns_bool = method
            .ret_ty
            .is_some_and(|ret| matches!(self.types.get(ret), Type::Primitive(Primitive::Bivalens)));
        let fails_textus = method
            .err_ty
            .is_some_and(|err| matches!(self.types.get(err), Type::Primitive(Primitive::Textus)));
        interface.type_params.is_empty()
            && method.params.is_empty()
            && names_close
            && returns_bool
            && fails_textus
    }

    /// `sermo` satisfies the closing contract without an `implet` clause.
    pub(super) fn builtin_satisfies_closing(
        &self,
        witness: TypeId,
        required: &ContractRef,
    ) -> bool {
        self.types.closing_contract == Some(required.contract)
            && required.args.is_empty()
            && self.sermo_shape(witness).is_some()
    }

    /// `s.close()` on a conversation: `bivalens`, failing with `textus`.
    /// `None` when the receiver is not a `sermo` or the method is not `close`.
    pub(super) fn check_sermo_close(
        &mut self,
        receiver_ty: TypeId,
        method_name: &str,
        explicit_type_args: &[TypeId],
        args: &mut [HirCallArg],
        span: Span,
    ) -> Option<TypeId> {
        if method_name != CLOSING_METHOD || self.sermo_shape(receiver_ty).is_none() {
            return None;
        }
        if !explicit_type_args.is_empty() {
            self.typecheck_call_arg_exprs(args);
            self.invalid_operand_types(super::call::ISSUE_DIRECTIONAL_VIEW_METHOD_TYPE_ARGS, span);
            return Some(self.error_type);
        }
        if !args.is_empty() {
            self.typecheck_call_arg_exprs(args);
            self.wrong_arity_with_arg("method_accepts_no_arguments", span, "method", "sermo.close");
            return Some(self.error_type);
        }
        let ret = self.bool_type();
        let sig = FuncSig {
            type_params: Vec::new(),
            params: Vec::new(),
            ret,
            err: Some(self.types.primitive(Primitive::Textus)),
            is_async: false,
            is_generator: false,
        };
        self.reject_failable_call(&sig, span);
        Some(ret)
    }

    /// Warn on every field that can hold a closable value while its owner
    /// sits on a reference cycle (D8.3 limit: cycles never auto-close).
    pub(super) fn warn_closable_cycles(&mut self, hir: &HirModule) {
        let mut fields: Vec<(DefId, Span, radix_lexer::Symbol, TypeId)> = Vec::new();
        let mut nodes = FxHashSet::default();
        for item in &hir.items {
            match &item.kind {
                HirItemKind::Struct(genus) => {
                    nodes.insert(item.def_id);
                    for field in &genus.fields {
                        if field.modifier != Some(radix_syntax::FieldModifier::Const) {
                            fields.push((item.def_id, field.span, field.name, field.ty));
                        }
                    }
                }
                HirItemKind::Enum(discretio) => {
                    nodes.insert(item.def_id);
                    let variant_fields = discretio.variants.iter().flat_map(|v| &v.fields);
                    for field in discretio.commune_fields.iter().chain(variant_fields) {
                        fields.push((item.def_id, field.span, field.name, field.ty));
                    }
                }
                _ => {}
            }
        }

        let mut edges: FxHashMap<DefId, Vec<DefId>> = FxHashMap::default();
        let mut self_edges = FxHashSet::default();
        for (owner, _, _, ty) in &fields {
            let mut targets = Vec::new();
            referenced_nominals(self.types, *ty, &mut targets, 0);
            targets.retain(|target| nodes.contains(target));
            if targets.contains(owner) {
                self_edges.insert(*owner);
            }
            edges.entry(*owner).or_default().extend(targets);
        }
        let component = radix_types::type_graph::strongly_connected_components(&nodes, &edges);
        let mut sizes: FxHashMap<usize, usize> = FxHashMap::default();
        for index in component.values() {
            *sizes.entry(*index).or_default() += 1;
        }
        let cyclic = |def: DefId| {
            self_edges.contains(&def)
                || component
                    .get(&def)
                    .is_some_and(|index| sizes.get(index).copied().unwrap_or(0) > 1)
        };

        for (owner, span, name, ty) in fields {
            let owner_closable = self.def_is_closable(owner);
            if !cyclic(owner)
                || !(owner_closable
                    || self.type_holds_closable(ty, &cyclic, &mut FxHashSet::default(), 0))
            {
                continue;
            }
            let (owner_name, field_name) = self.interner.map_or_else(
                || (String::new(), String::new()),
                |interner| {
                    let owner_name = hir
                        .items
                        .iter()
                        .find(|item| item.def_id == owner)
                        .and_then(|item| match &item.kind {
                            HirItemKind::Struct(genus) => Some(genus.name),
                            HirItemKind::Enum(discretio) => Some(discretio.name),
                            _ => None,
                        })
                        .map_or_else(String::new, |sym| interner.resolve(sym).to_owned());
                    (owner_name, interner.resolve(name).to_owned())
                },
            );
            self.errors.push(
                SemanticError::structured(
                    SemanticErrorKind::Warning(WarningKind::ClosableInCycle),
                    span,
                )
                .with_arg("issue", ISSUE_CLOSABLE_IN_CYCLE)
                .with_arg("type", owner_name)
                .with_arg("field", field_name),
            );
        }
    }

    /// A genus that declares `implet Closable`.
    pub(super) fn def_is_closable(&self, def: DefId) -> bool {
        let Some(contract) = self.types.closing_contract else {
            return false;
        };
        self.implemented_contracts
            .get(&def)
            .is_some_and(|declared| declared.iter().any(|c| c.contract == contract))
    }

    /// Whether a value of `ty` can be, or contain, a closable value.
    ///
    /// A genus off every cycle is looked through: its own fields never warn,
    /// so the field that holds it must. A genus on a cycle is not, because its
    /// fields warn for themselves and the field that merely points at it would
    /// repeat them.
    fn type_holds_closable(
        &self,
        ty: TypeId,
        cyclic: &dyn Fn(DefId) -> bool,
        seen: &mut FxHashSet<DefId>,
        depth: usize,
    ) -> bool {
        if depth > 64 {
            return false;
        }
        if self.sermo_shape(ty).is_some() {
            return true;
        }
        let next = depth + 1;
        match self.types.get(ty) {
            Type::Struct(def) => {
                self.def_is_closable(*def)
                    || (!cyclic(*def)
                        && seen.insert(*def)
                        && self.struct_fields_hold_closable(*def, cyclic, seen, next))
            }
            Type::Interface(def) => self.types.closing_contract == Some(*def),
            _ => {
                let mut children = Vec::new();
                child_types(self.types.get(ty), &mut children);
                children
                    .into_iter()
                    .any(|child| self.type_holds_closable(child, cyclic, seen, next))
            }
        }
    }

    /// Whether an instance field of `def` can hold a closable value.
    fn struct_fields_hold_closable(
        &self,
        def: DefId,
        cyclic: &dyn Fn(DefId) -> bool,
        seen: &mut FxHashSet<DefId>,
        depth: usize,
    ) -> bool {
        self.structs.get(&def).is_some_and(|info| {
            info.fields
                .values()
                .filter(|field| field.modifier != Some(radix_syntax::FieldModifier::Const))
                .any(|field| self.type_holds_closable(field.ty, cyclic, seen, depth))
        })
    }
}

/// Nominal types a field of `ty` can reference, through any container.
fn referenced_nominals(
    types: &radix_types::TypeTable,
    ty: TypeId,
    out: &mut Vec<DefId>,
    depth: usize,
) {
    if depth > 64 {
        return;
    }
    match types.get(ty) {
        Type::Struct(def) | Type::Enum(def) => out.push(*def),
        other => {
            let mut children = Vec::new();
            child_types(other, &mut children);
            for child in children {
                referenced_nominals(types, child, out, depth + 1);
            }
        }
    }
}

/// Type arguments a composite type stores values of.
fn child_types(ty: &Type, out: &mut Vec<TypeId>) {
    match ty {
        Type::Array(inner)
        | Type::Queue(inner)
        | Type::Stack(inner)
        | Type::Set(inner)
        | Type::Option(inner)
        | Type::Ref(_, inner)
        | Type::Alias(_, inner)
        | Type::Atomic(inner)
        | Type::Cursor(inner)
        | Type::Promissum(inner) => out.push(*inner),
        Type::BoundedArray { element, .. }
        | Type::RangedArray { element, .. }
        | Type::BoundedQueue { element, .. }
        | Type::BoundedStack { element, .. } => out.push(*element),
        Type::Map(key, value)
        | Type::Ledger(key, value)
        | Type::PromissumFailable(key, value)
        | Type::AsyncCursor(key, value) => out.extend([*key, *value]),
        Type::Record(fields) => out.extend(fields.values().copied()),
        Type::Tuple(members) | Type::Union(members) | Type::Intersection(members) => {
            out.extend(members.iter().copied());
        }
        Type::Applied(base, args) => {
            out.push(*base);
            out.extend(args.iter().filter_map(|arg| match arg {
                AppliedArg::Type(arg) => Some(*arg),
                AppliedArg::Size(_) => None,
            }));
        }
        _ => {}
    }
}
