//! The ordering contract (DECISIONS D1.4; names D1.5; defaults F1, F2).
//!
//! Norma declares `implendum Orderable<T> { functio compare(T other) → numerus }`
//! under `@ radix contract "ordering"`. This module owns what the compiler
//! adds on top of an ordinary contract:
//!
//! - **Recognition**: the contract is found by its mark, locally or through an
//!   imported file interface, and its shape is checked once.
//! - **Built-in conformance**: `numerus`, `fractus`, `textus`, `instans` and
//!   tuples of conforming types satisfy `T implet Orderable<T>` without user
//!   code ([`radix_types::ordering`]).
//! - **Glyph dispatch**: on a genus that `implet Orderable<Self>`, and on a
//!   type parameter bounded by `Orderable<T>`, `a ≺ b` is rewritten to
//!   `a.compare(b) ≺ 0` (likewise `≻ ≤ ≥`), so the glyphs and `compare`
//!   cannot disagree. No other operator is overloadable (D1.8).
//! - **Tuples** order lexicographically when every element conforms; the
//!   glyph stays a binary node and each backend expands it.

use radix_hir::{
    DefId, HirAnnotation, HirBinOp, HirCallArg, HirExpression, HirExpressionKind, HirId,
    HirInterface, HirLiteral,
};
use radix_lexer::Symbol;
use radix_types::ordering::{OrderingKind, ordering_kind};
use radix_types::{ContractRef, Type, TypeId};

use super::TypeChecker;
use crate::radix_contract::{ORDERING_METHOD, RadixContractRole, contract_error, role_from_hir};

/// `@ radix contract "ordering"` on an implendum whose shape is not
/// `Name<T> { compare(T other) → numerus }`.
const ISSUE_ORDERING_CONTRACT_SHAPE: &str = "radix_contract_ordering_shape";

/// A second implendum claims the ordering role in one unit.
const ISSUE_ORDERING_CONTRACT_DUPLICATE: &str = "radix_contract_duplicate";

/// Contract-ordered glyph operands differ in type.
const ISSUE_ORDERING_OPERAND_MISMATCH: &str = "ordering_operands_differ";

impl TypeChecker<'_> {
    /// Record a local implendum carrying `@ radix contract "ordering"`.
    pub(super) fn register_local_contract_role(
        &mut self,
        def_id: DefId,
        annotations: &[HirAnnotation],
        interface: &HirInterface,
        span: radix_lexer::Span,
    ) {
        let Some(interner) = self.interner else {
            return;
        };
        let role = role_from_hir(annotations, interner);
        if role == Some(RadixContractRole::Closing) {
            self.register_local_closing_contract(def_id, interface, span);
            return;
        }
        if role == Some(RadixContractRole::Sinking) {
            self.register_local_sinking_contract(def_id, interface, span);
            return;
        }
        if role != Some(RadixContractRole::Ordering) {
            return;
        }
        if !self.ordering_contract_shape_ok(interface) {
            self.errors
                .push(contract_error(span, ISSUE_ORDERING_CONTRACT_SHAPE));
            return;
        }
        self.claim_ordering_contract(def_id, span);
    }

    /// Record an imported implendum whose file interface names the role.
    pub(super) fn register_imported_contract_role(&mut self, def_id: DefId) {
        self.register_imported_closing_contract(def_id);
        self.register_imported_sinking_contract(def_id);
        let role = self
            .resolver
            .imported_contract_role(def_id)
            .and_then(RadixContractRole::parse);
        if role == Some(RadixContractRole::Ordering) {
            self.claim_ordering_contract(def_id, radix_lexer::Span::default());
        }
    }

    /// Record selective import bindings that name a compiler-supplied
    /// contract (ordering, sinking); backends emit no import for them.
    pub(super) fn record_ordering_contract_bindings(&mut self, import: &radix_hir::HirImport) {
        for item in &import.items {
            let Some(ty) = self.resolver.selective_import_type(item.def_id) else {
                continue;
            };
            let Type::Interface(def) = self.types.get(ty) else {
                continue;
            };
            let def = *def;
            if Some(def) == self.types.ordering_contract {
                self.types.ordering_contract_bindings.insert(item.def_id);
            } else if Some(def) == self.types.sinking_contract {
                self.types.sinking_contract_bindings.insert(item.def_id);
            }
        }
    }

    fn claim_ordering_contract(&mut self, def_id: DefId, span: radix_lexer::Span) {
        match self.types.ordering_contract {
            Some(existing) if existing != def_id => {
                self.errors
                    .push(contract_error(span, ISSUE_ORDERING_CONTRACT_DUPLICATE));
            }
            _ => self.types.ordering_contract = Some(def_id),
        }
    }

    /// `Name<T> { compare(T other) → numerus }`, nothing else.
    fn ordering_contract_shape_ok(&self, interface: &HirInterface) -> bool {
        let ([param], [method]) = (
            interface.type_params.as_slice(),
            interface.methods.as_slice(),
        ) else {
            return false;
        };
        let names_compare = self
            .interner
            .is_some_and(|interner| interner.resolve(method.name) == ORDERING_METHOD);
        let takes_t = matches!(
            method.params.as_slice(),
            [other] if matches!(self.types.get(other.ty), Type::Param(sym) if *sym == param.name)
        );
        let returns_numerus = method.ret_ty.is_some_and(|ret| {
            matches!(
                self.types.get(ret),
                Type::Primitive(radix_types::Primitive::Numerus)
            )
        });
        names_compare && takes_t && returns_numerus && method.err_ty.is_none()
    }

    /// How `ty` conforms to the ordering contract, if it does.
    pub(super) fn ordering_kind_of(&self, ty: TypeId) -> Option<OrderingKind> {
        let ty = self.resolve_inferred_type(ty);
        let contract = self.types.ordering_contract;
        let genus_conforms = |def: DefId| {
            let Some(contract) = contract else {
                return false;
            };
            self.implemented_contracts
                .get(&def)
                .is_some_and(|declared| {
                    declared.iter().any(|declared| {
                        declared.contract == contract
                            && matches!(declared.args.as_slice(),
                            [arg] if matches!(self.types.get(self.resolve_inferred_type(*arg)),
                                Type::Struct(inner) if *inner == def))
                    })
                })
        };
        let param_conforms = |sym: Symbol| {
            let Some(contract) = contract else {
                return false;
            };
            self.current_generic_bounds.get(&sym).is_some_and(|bounds| {
                bounds.iter().any(|bound| {
                    bound.contract == contract
                        && matches!(bound.args.as_slice(),
                            [arg] if matches!(self.types.get(*arg), Type::Param(inner) if *inner == sym))
                })
            })
        };
        ordering_kind(self.types, ty, &genus_conforms, &param_conforms)
    }

    /// Built-in conformance (D1.4): a built-in or tuple witness satisfies
    /// `Orderable<W>` without an `implet` clause. Genus and parameter
    /// witnesses keep the nominal rule (D1.3).
    pub(super) fn builtin_satisfies_contract(
        &self,
        witness: TypeId,
        required: &ContractRef,
    ) -> bool {
        if self.builtin_satisfies_closing(witness, required) {
            return true;
        }
        if self.types.ordering_contract != Some(required.contract) {
            return false;
        }
        let [arg] = required.args.as_slice() else {
            return false;
        };
        let witness = self.resolve_inferred_type(witness);
        if !self.types.equals(witness, self.resolve_inferred_type(*arg)) {
            return false;
        }
        matches!(
            self.ordering_kind_of(witness),
            Some(
                OrderingKind::Integer
                    | OrderingKind::Float
                    | OrderingKind::Text
                    | OrderingKind::Instans
                    | OrderingKind::Tuple(_)
            )
        )
    }

    /// Check `≺ ≻ ≤ ≥` whose operands order through the contract: a
    /// conforming genus, a bounded type parameter, or a tuple. Returns `None`
    /// for every other operand pair (built-in scalars keep their own rules,
    /// so bare `fractus` glyphs stay IEEE, operator default F2).
    pub(super) fn check_contract_ordering(
        &mut self,
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
    ) -> Option<TypeId> {
        let kind = self.ordering_kind_of(lhs_ty)?;
        if !matches!(
            kind,
            OrderingKind::Genus(_) | OrderingKind::Param | OrderingKind::Tuple(_)
        ) {
            return None;
        }
        let lhs_resolved = self.resolve_inferred_type(lhs_ty);
        let rhs_resolved = self.resolve_inferred_type(rhs_ty);
        let lhs_peeled = self.types.peel_access_marker(lhs_resolved).1;
        let rhs_peeled = self.types.peel_access_marker(rhs_resolved).1;
        if !self.types.equals(lhs_peeled, rhs_peeled) {
            self.invalid_operand_types(ISSUE_ORDERING_OPERAND_MISMATCH, rhs.span);
            return Some(self.bool_type());
        }
        if matches!(kind, OrderingKind::Genus(_) | OrderingKind::Param) {
            self.rewrite_glyph_to_compare(op, lhs, rhs);
        }
        Some(self.bool_type())
    }

    /// `a OP b` → `a.compare(b) OP 0`, with the method call typed `numerus`.
    fn rewrite_glyph_to_compare(
        &mut self,
        op: HirBinOp,
        lhs: &mut HirExpression,
        rhs: &mut HirExpression,
    ) {
        let Some(compare) = self.ordering_method_symbol() else {
            return;
        };
        let numerus = self.numerus_type();
        let span = lhs.span;
        let placeholder = |id: HirId| HirExpression {
            id,
            kind: HirExpressionKind::Literal(HirLiteral::Int(0)),
            ty: Some(numerus),
            span,
        };
        let call_id = self.fresh_hir_id();
        let receiver = std::mem::replace(lhs, placeholder(call_id));
        let zero_id = self.fresh_hir_id();
        let other = std::mem::replace(rhs, placeholder(zero_id));
        let arg_span = other.span;
        lhs.kind = HirExpressionKind::MethodCall(
            Box::new(receiver),
            compare,
            Vec::new(),
            vec![HirCallArg {
                name: None,
                spread: false,
                marker: None,
                expr: other,
                span: arg_span,
            }],
            None,
        );
        debug_assert!(matches!(
            op,
            HirBinOp::Lt | HirBinOp::Gt | HirBinOp::LtEq | HirBinOp::GtEq
        ));
    }

    /// The ordering contract's method name as the unit interned it.
    fn ordering_method_symbol(&self) -> Option<Symbol> {
        let contract = self.types.ordering_contract?;
        let methods = self.interfaces.get(&contract)?;
        methods.keys().copied().next()
    }

    /// A fresh expression id above every id in the module being checked.
    pub(super) fn fresh_hir_id(&mut self) -> HirId {
        let id = HirId(self.next_hir_id);
        self.next_hir_id += 1;
        id
    }
}
