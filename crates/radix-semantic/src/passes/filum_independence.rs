//! Typed-HIR admission for iteration-independent `filum` bodies.
//!
//! The proof is private to this pass. Storage roots survive aliases, and view
//! coordinates are mapped before comparing accesses. Unknown provenance or
//! effects decline admission; ordinary iteration is not checked.
//!
//! A call to a kernel (`calls`: any `@ nucleum` function, or instance method
//! bound statically by its receiver's type, public or private) is analysed as
//! if inlined (gpu-reset rule 12.7): the callee's parameters, and `ego` with
//! its tensor fields for a method, map to the caller's argument and receiver
//! storage and coordinates, and its reads and writes meet the same rules.

mod calls;
mod ends;

use crate::{SemanticError, SemanticErrorKind, Type, TypeId, TypeTable};
pub(crate) use calls::is_inlinable_kernel;
use calls::{EffectScan, Frame, Module, Receiver};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{HirVisitor, walk_expr, walk_local, walk_stmt};
use radix_hir::{
    DefId, HirArrayElement, HirBinOp, HirBlock, HirCallArg, HirConversioTarget, HirExpression,
    HirExpressionKind, HirFunction, HirLocal, HirModule, HirObjectKey, HirParam, HirParamMode,
    HirRangeKind, HirStatement, HirStatementKind, HirUnOp,
};
use radix_intrinsics::{IntrinsicReceiver, IntrinsicReturnKind};
use radix_lexer::{Interner, Span, Symbol};

const ISSUE: &str = "filum_independence_unproven";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Transform {
    Transpose,
    Slice(i128, i128),
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Storage {
    root: DefId,
    transforms: Vec<Transform>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Facts {
    storage: Vec<Storage>,
    unknown: bool,
}

impl Facts {
    fn fresh(root: DefId) -> Self {
        Self {
            storage: vec![Storage {
                root,
                transforms: Vec::new(),
            }],
            unknown: false,
        }
    }

    fn unknown() -> Self {
        Self {
            storage: Vec::new(),
            unknown: true,
        }
    }

    fn merge(&mut self, other: Self) {
        self.unknown |= other.unknown;
        for storage in other.storage {
            if !self.storage.contains(&storage) {
                self.storage.push(storage);
            }
        }
    }
}

/// How the parameters of the analysed function get storage roots.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ParamRoots {
    /// The caller-side rule of [`parameter_roots`]: `ref` and unmarked tensor
    /// parameters share one class root.
    Classes,
    /// A helper being analysed as inlined: every tensor parameter keeps its
    /// own root, which the call maps to the argument's storage. The caller's
    /// own roots carry any overlap between arguments.
    PerParam,
}

/// Flow-insensitive unions intentionally include stores after reads and every
/// branch. Rebinding cannot make an earlier alias disappear from the proof.
///
/// A call outside every `filum` body is an effect the facts must survive.
/// A call to a named function (not a local closure) can only reach what its
/// arguments reach, so it poisons the argument variables and the field
/// facts of the roots reachable from them. Every other effect poisons the
/// whole function (`uncertain`). Effects inside a `filum` body need no
/// poison: the proof inlines or declines each one.
#[derive(Default)]
struct Aliases {
    bindings: Vec<HirLocal>,
    stores: Vec<(HirExpression, HirExpression)>,
    values: FxHashMap<DefId, Facts>,
    fields: FxHashMap<(DefId, Symbol), Facts>,
    /// Initializers of the immutable locals of the analysed body: an affine
    /// address or a constant bound may resolve through them, because an
    /// immutable local keeps its initializer's value for its whole life.
    /// Mutable, awaited and runtime-provided bindings are excluded: their
    /// value is not their initializer.
    inits: FxHashMap<DefId, HirExpression>,
    /// The affine coordinate each immutable local's initializer resolves to,
    /// computed once per body and memoized here. A local resolves only when
    /// every binding its initializer names is value-stable (see
    /// [`InitCoords`]); anything else keeps the local an opaque variable.
    init_coords: FxHashMap<DefId, Coordinate>,
    /// Variables a call may rebind (the call's argument variables).
    poisoned_vars: FxHashSet<DefId>,
    /// Roots whose field facts a call may rewrite.
    poisoned_roots: FxHashSet<DefId>,
    uncertain: bool,
}

impl Aliases {
    /// `scan` is `None` for a helper body: every effect in it runs inside the
    /// caller's `filum` body, where the proof inlines or declines it.
    ///
    /// `receiver` is set for an instance-method body: `ego` and each tensor
    /// field of it get roots of their own, which a call maps to the storage of
    /// the receiver expression.
    fn build(
        params: &[HirParam],
        body: &HirBlock,
        types: &TypeTable,
        interner: &Interner,
        mode: ParamRoots,
        scan: Option<&Module<'_>>,
        receiver: Option<&Receiver>,
    ) -> Self {
        let mut result = Self::build_aliases(params, body, types, interner, mode, scan, receiver);
        result.init_coords =
            InitCoords::resolve(&result.inits, &result.bindings, &result.poisoned_vars);
        result
    }

    /// The alias equations of the body, without the initializer-coordinate
    /// memo (which needs the settled poison sets of [`Aliases::build`]).
    fn build_aliases(
        params: &[HirParam],
        body: &HirBlock,
        types: &TypeTable,
        interner: &Interner,
        mode: ParamRoots,
        scan: Option<&Module<'_>>,
        receiver: Option<&Receiver>,
    ) -> Self {
        let mut declarations = Declarations::default();
        declarations.visit_block(body);
        let locals: FxHashSet<DefId> = params
            .iter()
            .map(|param| param.def_id)
            .chain(declarations.bindings.iter().map(|local| local.def_id))
            .collect();
        let mut effects = EffectScan::new(scan, types, interner, &locals);
        if scan.is_some() {
            effects.visit_block(body);
        }
        let inits = declarations
            .bindings
            .iter()
            .filter(|local| !local.mutable && !local.is_await && !local.runtime_provided)
            .filter_map(|local| local.init.clone().map(|init| (local.def_id, init)))
            .collect();
        let mut result = Self {
            bindings: declarations.bindings,
            stores: declarations.stores,
            values: parameter_roots(params, types, mode),
            inits,
            ..Self::default()
        };
        if let Some(receiver) = receiver {
            result
                .values
                .insert(receiver.ego, Facts::fresh(receiver.ego));
            for (name, field) in &receiver.fields {
                result
                    .fields
                    .insert((receiver.ego, *name), Facts::fresh(*field));
            }
        }
        // Each poison round only grows the poisoned sets, so a few rounds per
        // call and binding settle it; anything longer is declined as unknown.
        let rounds = result.bindings.len() + result.stores.len() + effects.calls.len() + 2;
        for _ in 0..rounds {
            if !result.propagate(types, interner) {
                result.uncertain = true;
                return result;
            }
            if !result.poison(&effects.calls, types, interner) {
                result.uncertain |= effects.opaque;
                return result;
            }
        }
        result.uncertain = true;
        result
    }

    /// Acyclic propagation needs at most one round per equation. A cycle that
    /// keeps transforming views has no finite address certificate. Returns
    /// whether the facts settled.
    fn propagate(&mut self, types: &TypeTable, interner: &Interner) -> bool {
        let rounds = 2 * (self.bindings.len() + self.stores.len() + 1);
        for _ in 0..rounds {
            let before = (self.values.clone(), self.fields.clone());
            for local in self.bindings.clone() {
                let facts = if local.ty.is_some_and(|ty| types.is_copy_view(ty)) {
                    Facts::fresh(local.def_id)
                } else if let Some(init) = &local.init {
                    self.initializer(init, local.def_id, types, interner)
                } else {
                    Facts::unknown()
                };
                self.values.entry(local.def_id).or_default().merge(facts);
            }
            for (target, source) in self.stores.clone() {
                let facts = self.facts(&source, types, interner);
                match &target.kind {
                    HirExpressionKind::Path(def) => {
                        let copied = self.bindings.iter().any(|local| {
                            local.def_id == *def
                                && local.ty.is_some_and(|ty| types.is_copy_view(ty))
                        });
                        self.values.entry(*def).or_default().merge(if copied {
                            Facts::fresh(*def)
                        } else {
                            facts
                        });
                    }
                    HirExpressionKind::Field(object, field) => {
                        for storage in self.facts(object, types, interner).storage {
                            self.fields
                                .entry((storage.root, *field))
                                .or_default()
                                .merge(facts.clone());
                        }
                    }
                    _ => {}
                }
            }
            if before == (self.values.clone(), self.fields.clone()) {
                return true;
            }
        }
        false
    }

    /// Grow the poisoned sets from the arguments of the named calls. Returns
    /// whether anything new was poisoned.
    fn poison(&mut self, calls: &[HirExpression], types: &TypeTable, interner: &Interner) -> bool {
        let before = (self.poisoned_vars.len(), self.poisoned_roots.len());
        for arg in calls {
            self.poison_argument(arg, types, interner);
        }
        before != (self.poisoned_vars.len(), self.poisoned_roots.len())
    }

    fn poison_argument(&mut self, arg: &HirExpression, types: &TypeTable, interner: &Interner) {
        let facts = self.facts(arg, types, interner);
        for storage in facts.storage {
            self.poison_root(storage.root);
        }
        match &arg.kind {
            HirExpressionKind::Path(def) => {
                self.poisoned_vars.insert(*def);
            }
            HirExpressionKind::Field(object, _) => self.poison_argument(object, types, interner),
            HirExpressionKind::NonNull(inner, _)
            | HirExpressionKind::Deref(inner)
            | HirExpressionKind::Ref(_, inner)
            | HirExpressionKind::Verte {
                source: inner,
                entries: None,
                ..
            } => self.poison_argument(inner, types, interner),
            _ => {}
        }
    }

    /// A root and every root reachable through its recorded fields.
    fn poison_root(&mut self, root: DefId) {
        if !self.poisoned_roots.insert(root) {
            return;
        }
        let reachable: Vec<DefId> = self
            .fields
            .iter()
            .filter(|((owner, _), _)| *owner == root)
            .flat_map(|(_, facts)| facts.storage.iter().map(|storage| storage.root))
            .collect();
        for next in reachable {
            self.poison_root(next);
        }
    }

    fn initializer(
        &mut self,
        expr: &HirExpression,
        root: DefId,
        types: &TypeTable,
        interner: &Interner,
    ) -> Facts {
        match &expr.kind {
            HirExpressionKind::Vacua | HirExpressionKind::Array(_) => Facts::fresh(root),
            // `crea` and `strue` are the tensor factories: their contract
            // (`docs/design/tensor-intrinsics.md`) and every MIR lowering
            // allocate a new buffer, never a view of the receiver. The
            // `tf32[…]` constructor spelling reaches this arm as the
            // synthesized vacua-receiver `crea` call of `lower_construct`;
            // the retired receiver row no longer backs the registry probe,
            // so the constructor shape is acknowledged directly.
            HirExpressionKind::MethodCall(receiver, name, _, args, _)
                if matches!(interner.resolve(*name), "crea" | "strue")
                    && (is_access_intrinsic(receiver, *name, args.len(), types, interner)
                        || is_construct_call(receiver, *name, args.len(), interner)) =>
            {
                Facts::fresh(root)
            }
            // The `↦` tensor conversion mints the same fresh storage: its
            // lowering is the `TensorFromFlat` collection op shared with
            // explicit tensor construction (`lower_lista_to_tensor_conversio`).
            // Fresh storage at creation is a property of the lowering, not
            // the spelling, so the fact shape mirrors the factories' above.
            // Only the flat-lista sources prove: the three `lower_conversio`
            // arms that lower through a fresh tensor aggregate. A tensor
            // source converts by sharing its buffer, and a routed row or `⊥`
            // recovery can yield an aliased value: unknown decline.
            HirExpressionKind::Conversio {
                source,
                target: HirConversioTarget::Type(target),
                route: None,
                recovery: None,
                implicit: None,
                ..
            } if source.ty.is_some_and(|source_ty| {
                lowers_lista_to_fresh_tensor(source, source_ty, *target, types)
            }) =>
            {
                Facts::fresh(root)
            }
            HirExpressionKind::Struct(_, fields) => {
                for (name, value) in fields {
                    let facts = self.facts(value, types, interner);
                    self.fields.entry((root, *name)).or_default().merge(facts);
                }
                Facts::fresh(root)
            }
            HirExpressionKind::Verte {
                entries: Some(entries),
                ..
            } => {
                for entry in entries {
                    let HirObjectKey::Ident(name) = entry.key else {
                        return Facts::unknown();
                    };
                    let Some(value) = &entry.value else {
                        return Facts::unknown();
                    };
                    let facts = self.facts(value, types, interner);
                    self.fields.entry((root, name)).or_default().merge(facts);
                }
                Facts::fresh(root)
            }
            _ => self.facts(expr, types, interner),
        }
    }

    fn facts(&self, expr: &HirExpression, types: &TypeTable, interner: &Interner) -> Facts {
        if self.uncertain {
            return Facts::unknown();
        }
        match &expr.kind {
            HirExpressionKind::Path(def) if self.poisoned_vars.contains(def) => Facts::unknown(),
            HirExpressionKind::Path(def) => self.values.get(def).cloned().unwrap_or_default(),
            HirExpressionKind::Field(object, field) => {
                let parent = self.facts(object, types, interner);
                let mut result = Facts {
                    unknown: parent.unknown,
                    ..Facts::default()
                };
                for storage in parent.storage {
                    if self.poisoned_roots.contains(&storage.root) {
                        result.unknown = true;
                        continue;
                    }
                    result.merge(
                        self.fields
                            .get(&(storage.root, *field))
                            .cloned()
                            .unwrap_or_else(Facts::unknown),
                    );
                }
                result
            }
            HirExpressionKind::NonNull(inner, _)
            | HirExpressionKind::Deref(inner)
            | HirExpressionKind::Ref(_, inner)
            | HirExpressionKind::Verte {
                source: inner,
                entries: None,
                ..
            } => self.facts(inner, types, interner),
            HirExpressionKind::MethodCall(receiver, name, type_args, args, _) => {
                if !is_access_intrinsic(receiver, *name, args.len(), types, interner) {
                    return Facts::unknown();
                }
                let transform = match interner.resolve(*name) {
                    // A shifted, reshaped or edge-policy view reads another
                    // coordinate of its parent, so its map is unknown: it
                    // certifies a read of storage the body never writes, and
                    // nothing else.
                    "disloco" | "forma" | "expande" => Transform::Unknown,
                    "limes" if builtin_edge_policy(type_args, types) => Transform::Unknown,
                    "transpone" | "__faber_glyph_transpose" => Transform::Transpose,
                    "sectio" if args.len() >= 2 => {
                        let start = super::const_int_value(&args[0].expr);
                        let step = args
                            .get(2)
                            .map_or(Some(1), |arg| super::const_int_value(&arg.expr));
                        match (start, step) {
                            (Some(start), Some(step)) if step != 0 => Transform::Slice(start, step),
                            _ => Transform::Unknown,
                        }
                    }
                    _ => return Facts::unknown(),
                };
                let mut result = self.facts(receiver, types, interner);
                for storage in &mut result.storage {
                    storage.transforms.push(transform.clone());
                }
                result
            }
            _ => Facts::unknown(),
        }
    }
}

/// Storage roots of the tensor parameters of the function that owns the body.
///
/// The call-site borrow check (`borrow.rs`, `borrow_arg_storage`) judges
/// overlap on storage: a `mut` argument cannot name the same tensor as any
/// other argument, by name, `←` alias or view. So within the body:
/// - a `mut` parameter, and a `copy` parameter (duplicated at the call), each
///   own a root distinct from every other parameter;
/// - `ref` and unmarked parameters may name one another's tensor, so they share
///   one root class, represented by its first member. An unmarked parameter is
///   writable, which keeps a write through it checked against reads through
///   its siblings;
/// - an `own` parameter gets no root: the call site registers no borrow for a
///   non-exclusive argument passed to it, so `k(own a, mut b)` may overlap;
/// - a parameter that is not a bare tensor (list, genus, map, alias-typed
///   tensor, optional) gets no root, because the call-site rule is tensor-typed
///   only. Its accesses keep the unknown-identity decline.
fn parameter_roots(
    params: &[HirParam],
    types: &TypeTable,
    mode: ParamRoots,
) -> FxHashMap<DefId, Facts> {
    let mut roots = FxHashMap::default();
    let mut shared: Option<DefId> = None;
    for param in params {
        let Some(root) = param_root(param, types, mode, &mut shared) else {
            continue;
        };
        roots.insert(param.def_id, Facts::fresh(root));
    }
    roots
}

/// The root a tensor parameter reads and writes through, if it has one. In
/// [`ParamRoots::PerParam`] mode `ref` and unmarked parameters keep a root of
/// their own instead of the shared class root.
fn param_root(
    param: &HirParam,
    types: &TypeTable,
    mode: ParamRoots,
    shared: &mut Option<DefId>,
) -> Option<DefId> {
    let (_, inner) = types.peel_access_marker(param.ty);
    if !matches!(types.get(inner), Type::Tensor(_, _)) {
        return None;
    }
    let own = matches!(param.mode, HirParamMode::Ex) || types.is_own_view(param.ty);
    let mutable = matches!(param.mode, HirParamMode::In) || types.is_mut_view(param.ty);
    if types.is_copy_view(param.ty) || mutable || mode == ParamRoots::PerParam && !own {
        Some(param.def_id)
    } else if own {
        None
    } else {
        Some(*shared.get_or_insert(param.def_id))
    }
}

/// Whether this conversion yields storage no other root names: one of the
/// `lower_conversio` arms that constructs a fresh tensor aggregate and fills
/// it through `TensorFromFlat` (the padded bounded-lista snapshot, the flat
/// lista, or the rectangular lista literal; the same predicates the lowering
/// arm dispatches on), or a list literal whose untyped constants adopt the
/// tensor element.
///
/// The adopted literal is admitted by the typechecker's `literal_padded_shape`
/// although its element type is not the tensor's, so the strict padded
/// predicate no longer names it. Its source is a list built in place,
/// which no existing tensor can alias, so the conversion result is fresh
/// whichever lowering arm builds it.
fn lowers_lista_to_fresh_tensor(
    source: &HirExpression,
    source_ty: TypeId,
    target: TypeId,
    types: &TypeTable,
) -> bool {
    crate::lista_tensor_conversio::resolve_bounded_lista_padded_shape(source_ty, target, types)
        .is_some()
        || crate::lista_tensor_conversio::validate_lista_to_tensor_source(source_ty, target, types)
            .is_ok()
        || crate::lista_tensor_conversio::analyze_rectangular_lista_literal(source, target, types)
            .is_ok()
        || (matches!(source.kind, HirExpressionKind::Array(_))
            && crate::lista_tensor_conversio::padded_literal_facts(source_ty, target, types)
                .is_some())
}

fn is_access_intrinsic(
    receiver: &HirExpression,
    name: Symbol,
    argc: usize,
    types: &TypeTable,
    interner: &Interner,
) -> bool {
    receiver.ty.is_some_and(|ty| {
        radix_intrinsics::lookup_method_for_type_id(ty, types, interner.resolve(name), argc)
            .is_some()
    })
}

/// The type-expression constructor synthesizes a `crea` call on a vacua
/// receiver pre-typed with the claimed type (`lower_construct`; the check
/// lane rewrites its symbol to the canonical spelling). The retired receiver
/// row no longer backs the registry probe, so the shape is matched directly.
pub(super) fn is_construct_call(
    receiver: &HirExpression,
    name: Symbol,
    argc: usize,
    interner: &Interner,
) -> bool {
    matches!(receiver.kind, HirExpressionKind::Vacua)
        && interner.resolve(name) == "crea"
        && argc == 2
}

/// A value-returning method of a scalar or register-class receiver. It reads
/// its receiver and arguments and writes nothing: the in-place numeric twins
/// were desugared to stores before this pass, and none of these families
/// returns `vacuum` for a value method.
fn is_pure_value_method(
    receiver: &HirExpression,
    name: Symbol,
    argc: usize,
    types: &TypeTable,
    interner: &Interner,
) -> bool {
    receiver
        .ty
        .and_then(|ty| {
            radix_intrinsics::lookup_method_for_type_id(ty, types, interner.resolve(name), argc)
        })
        .is_some_and(|entry| {
            matches!(
                entry.receiver,
                IntrinsicReceiver::Numeric
                    | IntrinsicReceiver::Fractus
                    | IntrinsicReceiver::Vector
                    | IntrinsicReceiver::Matrix
                    | IntrinsicReceiver::Intervallum
            ) && entry.return_kind != IntrinsicReturnKind::Vacuum
        })
}

/// `limes<P>` with a compiler-owned policy. A custom policy calls user
/// methods, so it keeps the unproved-call decline.
fn builtin_edge_policy(type_args: &[TypeId], types: &TypeTable) -> bool {
    type_args.len() == 1 && matches!(types.get(type_args[0]), Type::TensorEdgePolicy(_))
}

/// A type whose values can alias other storage or hide a handle to it.
fn is_aggregate(types: &TypeTable, ty: TypeId) -> bool {
    let (_, ty) = types.peel_access_marker(ty);
    types.is_aliasing_container(ty)
        || matches!(
            types.get(ty),
            Type::Struct(_)
                | Type::Interface(_)
                | Type::Applied(..)
                | Type::Param(_)
                | Type::Record(_)
                | Type::Tuple(_)
                | Type::Option(_)
                | Type::Union(_)
                | Type::Primitive(
                    crate::Primitive::Ignotum | crate::Primitive::Valor | crate::Primitive::Json
                )
        )
}

/// `vector` and `matrix` are register-class values, not tensor aliases
/// (`docs/design/systems-shaped-values.md`): extracting a component reads a
/// value that an earlier recorded access already produced.
fn is_register_value(ty: Option<TypeId>, types: &TypeTable) -> bool {
    ty.is_some_and(|ty| {
        let (_, ty) = types.peel_access_marker(ty);
        matches!(types.get(ty), Type::Vector(..) | Type::Matrix(..))
    })
}

/// A fixed-array (`list<T, size N>`) field of a genus value: `record.field`.
/// The array lives inline in the record, so indexing it reads the record's own
/// bytes (the record is the storage the enclosing access already recorded).
fn is_record_array_field(expr: &HirExpression, types: &TypeTable) -> bool {
    let HirExpressionKind::Field(parent, _) = &expr.kind else {
        return false;
    };
    let peeled = |ty: Option<TypeId>| ty.map(|ty| types.peel_access_marker(ty).1);
    matches!(
        peeled(expr.ty).map(|ty| types.get(ty)),
        Some(Type::BoundedArray { .. })
    ) && matches!(
        peeled(parent.ty).map(|ty| types.get(ty)),
        Some(Type::Struct(_))
    )
}

#[derive(Default)]
struct Declarations {
    bindings: Vec<HirLocal>,
    stores: Vec<(HirExpression, HirExpression)>,
}

impl HirVisitor for Declarations {
    fn visit_local(&mut self, local: &HirLocal) {
        self.bindings.push(local.clone());
        walk_local(self, local);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Assign(target, source)
        | HirExpressionKind::ConversioAssign { target, source, .. } = &expr.kind
        {
            self.stores.push(((**target).clone(), (**source).clone()));
        }
        walk_expr(self, expr);
    }
}

/// One storage coordinate as an affine address `Σ coefficient · variable +
/// offset`, each variable named at most once and never with a zero
/// coefficient. A constant address names no variable.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Coordinate {
    terms: Vec<(DefId, i128)>,
    offset: i128,
}

impl Coordinate {
    fn constant(offset: i128) -> Self {
        Self {
            terms: Vec::new(),
            offset,
        }
    }

    fn variable(def: DefId) -> Self {
        Self {
            terms: vec![(def, 1)],
            offset: 0,
        }
    }

    /// The variable the coordinate names exactly: coefficient one, no
    /// offset, no other term. The lane coordinates of a loop are exactly
    /// this shape.
    fn plain_variable(&self) -> Option<DefId> {
        match self.terms.as_slice() {
            [(def, 1)] if self.offset == 0 => Some(*def),
            _ => None,
        }
    }

    /// Whether the coordinate names `def` with any coefficient.
    fn names(&self, def: DefId) -> bool {
        self.terms.iter().any(|(variable, _)| *variable == def)
    }

    /// The coefficient of `def`, or `None` when the coordinate does not
    /// name it.
    fn coefficient(&self, def: DefId) -> Option<i128> {
        self.terms
            .iter()
            .find(|(variable, _)| *variable == def)
            .map(|(_, coefficient)| *coefficient)
    }

    /// The coordinate without the term of `def`: the residual beside a
    /// lane's own term.
    fn without(mut self, def: DefId) -> Self {
        self.terms.retain(|(variable, _)| *variable != def);
        self
    }

    /// `self + other`, one term per variable; cancelled terms fold away.
    fn plus(self, other: &Self) -> Option<Self> {
        let mut terms = self.terms;
        for (variable, coefficient) in &other.terms {
            if let Some(slot) = terms.iter_mut().find(|(v, _)| v == variable) {
                slot.1 = slot.1.checked_add(*coefficient)?;
            } else {
                terms.push((*variable, *coefficient));
            }
        }
        terms.retain(|(_, coefficient)| *coefficient != 0);
        Some(Self {
            terms,
            offset: self.offset.checked_add(other.offset)?,
        })
    }

    /// `self - other`.
    fn minus(self, other: &Self) -> Option<Self> {
        self.plus(&other.clone().scaled(-1)?)
    }

    /// `self * scale`; a zero scale is the constant zero.
    fn scaled(mut self, scale: i128) -> Option<Self> {
        for (_, coefficient) in &mut self.terms {
            *coefficient = coefficient.checked_mul(scale)?;
        }
        self.terms.retain(|(_, coefficient)| *coefficient != 0);
        self.offset = self.offset.checked_mul(scale)?;
        Some(self)
    }

    /// The value interval of the coordinate over the loop binders with
    /// proven constant ranges; `None` when a term names a variable without
    /// one. WHY: the terms name independent variables, so the interval sums
    /// per term, the smaller product at the minimum and the larger at the
    /// maximum whatever the coefficient's sign.
    fn interval(&self, ranges: &[(DefId, (i128, i128))]) -> Option<(i128, i128)> {
        let mut lo = self.offset;
        let mut hi = self.offset;
        for (variable, coefficient) in &self.terms {
            let (bound_lo, bound_hi) = ranges
                .iter()
                .find(|(def, _)| def == variable)
                .map(|(_, range)| *range)?;
            let (term_lo, term_hi) = if *coefficient >= 0 {
                (
                    coefficient.checked_mul(bound_lo)?,
                    coefficient.checked_mul(bound_hi)?,
                )
            } else {
                (
                    coefficient.checked_mul(bound_hi)?,
                    coefficient.checked_mul(bound_lo)?,
                )
            };
            lo = lo.checked_add(term_lo)?;
            hi = hi.checked_add(term_hi)?;
        }
        Some((lo, hi))
    }
}

/// Resolution of an immutable local's initializer to the affine coordinate
/// the local keeps for its whole life. A local resolves only when every
/// binding its initializer names is value-stable across that life: a def no
/// store, call or rebind of the body can change — a parameter, a loop
/// binder, a module constant, or another immutable local whose own
/// initializer is value-stable. An initializer that names a mutable or
/// call-poisoned binding reads a value later stores may change, so the local
/// keeps its own opaque variable instead of the source's live identity.
///
/// Each initializer is walked once and memoized, so a chain that names a
/// local twice (`a₁ ← a₀ + a₀`, …) costs one walk per local instead of
/// doubling with the chain. A total node budget bounds the whole map:
/// when it runs out, the locals still unresolved keep the opaque-variable
/// decline, which loses proofs but proves nothing false.
struct InitCoords;

/// Total initializer nodes one body may expand across all its locals.
const INIT_COORDINATE_BUDGET: u64 = 8192;

impl InitCoords {
    fn resolve(
        inits: &FxHashMap<DefId, HirExpression>,
        bindings: &[HirLocal],
        poisoned: &FxHashSet<DefId>,
    ) -> FxHashMap<DefId, Coordinate> {
        let mut unstable: FxHashSet<DefId> = poisoned.iter().copied().collect();
        unstable.extend(
            bindings
                .iter()
                .filter(|local| local.mutable)
                .map(|local| local.def_id),
        );
        let mut resolver = Resolver {
            inits,
            unstable,
            memo: FxHashMap::default(),
            visiting: Vec::new(),
            budget: INIT_COORDINATE_BUDGET,
        };
        // Sorted for a deterministic budget cutoff: hash-map order would
        // decide which locals keep their proofs when the budget runs out.
        let mut defs: Vec<DefId> = inits.keys().copied().collect();
        defs.sort_unstable_by_key(|def| def.0);
        for def in defs {
            resolver.of(def);
        }
        resolver.memo
    }
}

struct Resolver<'a> {
    inits: &'a FxHashMap<DefId, HirExpression>,
    unstable: FxHashSet<DefId>,
    memo: FxHashMap<DefId, Coordinate>,
    visiting: Vec<DefId>,
    budget: u64,
}

impl Resolver<'_> {
    /// The coordinate accesses through `def` name: its initializer's
    /// coordinate when that is value-stable, `def` as an opaque variable
    /// otherwise.
    fn of(&mut self, def: DefId) -> Coordinate {
        if let Some(resolved) = self.memo.get(&def) {
            return resolved.clone();
        }
        if self.visiting.contains(&def) {
            // A cycle of initializers is a malformed tree; the local keeps
            // its own opaque variable.
            return Coordinate::variable(def);
        }
        let Some(init) = self.inits.get(&def) else {
            return Coordinate::variable(def);
        };
        self.visiting.push(def);
        let resolved = self.expr(init);
        self.visiting.pop();
        let resolved = resolved.unwrap_or_else(|| Coordinate::variable(def));
        self.memo.insert(def, resolved.clone());
        resolved
    }

    fn expr(&mut self, expr: &HirExpression) -> Option<Coordinate> {
        work::count(1);
        if self.budget == 0 {
            return None;
        }
        self.budget -= 1;
        if let Some(offset) = super::const_int_value(expr) {
            return Some(Coordinate::constant(offset));
        }
        match &expr.kind {
            HirExpressionKind::Path(def) => {
                if self.unstable.contains(def) {
                    // A mutable or call-poisoned source changes after the
                    // snapshot; the local is not its live alias.
                    return None;
                }
                Some(self.of(*def))
            }
            HirExpressionKind::Widen { operand, .. } => self.expr(operand),
            HirExpressionKind::Conversio {
                source,
                implicit: Some(_),
                ..
            } => self.expr(source),
            HirExpressionKind::Binary(HirBinOp::Add, lhs, rhs) => {
                self.expr(lhs)?.plus(&self.expr(rhs)?)
            }
            HirExpressionKind::Binary(HirBinOp::Sub, lhs, rhs) => {
                self.expr(lhs)?.minus(&self.expr(rhs)?)
            }
            HirExpressionKind::Binary(HirBinOp::Mul, lhs, rhs) => {
                let lhs = self.expr(lhs)?;
                let rhs = self.expr(rhs)?;
                // A product of two variables is not affine: no coefficient.
                if lhs.terms.is_empty() {
                    rhs.scaled(lhs.offset)
                } else if rhs.terms.is_empty() {
                    lhs.scaled(rhs.offset)
                } else {
                    None
                }
            }
            HirExpressionKind::Unary(HirUnOp::Neg, operand) => self.expr(operand)?.scaled(-1),
            _ => None,
        }
    }
}

/// Operation counter of the coordinate walk, so tests assert expansion work
/// as a count of visited nodes instead of a timing.
pub(crate) mod work {
    use std::cell::Cell;

    thread_local! {
        static NODES: Cell<u64> = const { Cell::new(0) };
    }

    /// Record `nodes` visited expression nodes.
    pub(crate) fn count(nodes: u64) {
        NODES.with(|counter| counter.set(counter.get() + nodes));
    }

    /// Take the recorded node count, resetting the counter.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn take() -> u64 {
        NODES.with(|counter| counter.replace(0))
    }
}

fn coordinates(aliases: &Aliases, expr: &HirExpression) -> Option<Vec<Coordinate>> {
    match &expr.kind {
        HirExpressionKind::Array(elements) => elements
            .iter()
            .map(|element| match element {
                HirArrayElement::Expr(expr) => coordinate(aliases, expr),
                _ => None,
            })
            .collect(),
        HirExpressionKind::Tuple(elements, _) => elements
            .iter()
            .map(|expr| coordinate(aliases, expr))
            .collect(),
        _ => Some(vec![coordinate(aliases, expr)?]),
    }
}

/// The affine address of one index expression: a constant, a variable, or a
/// sum, difference or constant product of those. An immutable local
/// resolves to the coordinate its initializer proved at alias-build time
/// ([`InitCoords`]): its own opaque variable when the initializer is not an
/// address or its dependencies are not value-stable. Anything else is not
/// an address the proof can name.
fn coordinate(aliases: &Aliases, expr: &HirExpression) -> Option<Coordinate> {
    coordinate_at(aliases, expr, 0)
}

/// `depth` bounds the syntactic walk. Initializer chains resolve through the
/// memoized map, so the bound only guards the walk of one expression tree
/// against a malformed nest.
fn coordinate_at(aliases: &Aliases, expr: &HirExpression, depth: usize) -> Option<Coordinate> {
    if depth > 64 {
        return None;
    }
    work::count(1);
    if let Some(offset) = super::const_int_value(expr) {
        return Some(Coordinate::constant(offset));
    }
    match &expr.kind {
        HirExpressionKind::Path(def) => Some(
            aliases
                .init_coords
                .get(def)
                .cloned()
                .unwrap_or_else(|| Coordinate::variable(*def)),
        ),
        HirExpressionKind::Widen { operand, .. } => coordinate_at(aliases, operand, depth),
        HirExpressionKind::Conversio {
            source,
            implicit: Some(_),
            ..
        } => coordinate_at(aliases, source, depth),
        HirExpressionKind::Binary(HirBinOp::Add, lhs, rhs) => {
            coordinate_at(aliases, lhs, depth)?.plus(&coordinate_at(aliases, rhs, depth)?)
        }
        HirExpressionKind::Binary(HirBinOp::Sub, lhs, rhs) => {
            coordinate_at(aliases, lhs, depth)?.minus(&coordinate_at(aliases, rhs, depth)?)
        }
        HirExpressionKind::Binary(HirBinOp::Mul, lhs, rhs) => {
            let lhs = coordinate_at(aliases, lhs, depth)?;
            let rhs = coordinate_at(aliases, rhs, depth)?;
            // A product of two variables is not affine: no coefficient.
            if lhs.terms.is_empty() {
                rhs.scaled(lhs.offset)
            } else if rhs.terms.is_empty() {
                lhs.scaled(rhs.offset)
            } else {
                None
            }
        }
        HirExpressionKind::Unary(HirUnOp::Neg, operand) => {
            coordinate_at(aliases, operand, depth)?.scaled(-1)
        }
        _ => None,
    }
}

/// A constant integer through immutable locals: the shared folding of
/// [`super::const_int_value`] plus resolution of a path to an immutable
/// local's constant initializer.
fn const_int_through(aliases: &Aliases, expr: &HirExpression) -> Option<i128> {
    if let Some(value) = super::const_int_value(expr) {
        return Some(value);
    }
    match &expr.kind {
        HirExpressionKind::Path(def) => aliases
            .inits
            .get(def)
            .and_then(|init| const_int_through(aliases, init)),
        HirExpressionKind::Widen { operand, .. } => const_int_through(aliases, operand),
        HirExpressionKind::Conversio {
            source,
            implicit: Some(_),
            ..
        } => const_int_through(aliases, source),
        _ => None,
    }
}

/// The proven value range `[lo, hi]` (inclusive) of an immutable counted-loop
/// binder, when both bounds and the step are constants through immutable
/// locals and the step is positive. The loop yields `lo, lo + step, ..` up to
/// its end (excluded or included), so every value lies in that interval. A
/// mutable binder may be reassigned inside the body, and a negative step or a
/// non-constant bound proves nothing.
fn counted_range(
    aliases: &Aliases,
    binding: DefId,
    mutable: bool,
    start: &HirExpression,
    end: &HirExpression,
    step: Option<&HirExpression>,
    kind: HirRangeKind,
) -> Option<(DefId, (i128, i128))> {
    if mutable {
        return None;
    }
    let lo = const_int_through(aliases, start)?;
    let end = const_int_through(aliases, end)?;
    let step = step.map_or(Some(1), |step| const_int_through(aliases, step))?;
    if step < 1 {
        return None;
    }
    let hi = match kind {
        // The last yielded value is below an excluded end.
        HirRangeKind::Exclusive => end.checked_sub(1)?,
        HirRangeKind::Inclusive => end,
    };
    if lo > hi {
        return None;
    }
    Some((binding, (lo, hi)))
}

fn mapped(storage: &Storage, mut coords: Vec<Coordinate>) -> Option<Vec<Coordinate>> {
    // A view maps local coordinates back through each parent, in reverse
    // construction order. Transpose swaps only the trailing pair of axes.
    for transform in storage.transforms.iter().rev() {
        match transform {
            Transform::Transpose if coords.len() >= 2 => {
                let n = coords.len();
                coords.swap(n - 2, n - 1);
            }
            Transform::Slice(start, step) if !coords.is_empty() => {
                coords[0] = coords[0]
                    .clone()
                    .scaled(*step)?
                    .plus(&Coordinate::constant(*start))?;
            }
            _ => return None,
        }
    }
    Some(coords)
}

/// One `filum` loop of a nest: its own coordinates, the defs its body owns,
/// and the storage its element fetch reads.
struct LoopCtx {
    coords: Vec<Coordinate>,
    owned: FxHashSet<DefId>,
    source: Facts,
}

struct Access {
    facts: Facts,
    coords: Option<Vec<Coordinate>>,
    write: bool,
    span: Span,
    /// Innermost enclosing `filum` loop (index into `BodyProof::loops`).
    owner: usize,
    /// Enclosing `filum` loop count; the coordinates of the whole chain,
    /// outermost first, form the one coordinate an iteration of the innermost
    /// loop owns.
    depth: usize,
    composed: Vec<Coordinate>,
    every_loop_has_coordinates: bool,
    /// Read inside a range-loop bound: a bound is evaluated once per lane
    /// and must read nothing the lanes write, at any coordinate.
    bound: bool,
    /// The counted-loop binder ranges in scope at the access: they bound the
    /// residual of a flattened lane row.
    ranges: Vec<(DefId, (i128, i128))>,
}

struct BodyProof<'a> {
    aliases: &'a Aliases,
    module: &'a Module<'a>,
    types: &'a TypeTable,
    interner: &'a Interner,
    /// Every `filum` loop met so far; `stack` is the chain around the cursor.
    loops: Vec<LoopCtx>,
    stack: Vec<usize>,
    /// Proven constant value ranges of the immutable binders of counted
    /// loops around the cursor, innermost last.
    range_stack: Vec<(DefId, (i128, i128))>,
    /// Helper calls being analysed as inlined, innermost last. Empty while the
    /// cursor is in the body that owns the loop.
    frames: Vec<Frame<'a>>,
    accesses: Vec<Access>,
    errors: Vec<SemanticError>,
    suppress_reads: bool,
    /// Depth of range-loop bounds around the cursor.
    bound_depth: usize,
    /// Issue named by a decline: this pass's, or a reduction body's.
    issue: &'static str,
    /// Arguments of an inlined call are not visited (a reduction body's
    /// own checks already own them).
    skip_arguments: bool,
}

fn coordinates_of(defs: &[(DefId, Symbol)]) -> Vec<Coordinate> {
    defs.iter()
        .map(|(def, _)| Coordinate::variable(*def))
        .collect()
}

impl BodyProof<'_> {
    /// Index of the innermost `filum` loop around the cursor; the proof only
    /// records accesses with the loop being proved on the stack.
    fn current_loop(&self) -> usize {
        *self.stack.last().expect("inside a filum loop")
    }

    fn owner(&self) -> &LoopCtx {
        &self.loops[self.current_loop()]
    }

    fn push_access(
        &mut self,
        facts: Facts,
        coords: Option<Vec<Coordinate>>,
        write: bool,
        span: Span,
    ) {
        let composed = self
            .stack
            .iter()
            .flat_map(|&index| self.loops[index].coords.clone())
            .collect();
        let coords = coords.and_then(|coords| self.map_coords(coords));
        self.accesses.push(Access {
            facts,
            coords,
            write,
            span,
            owner: self.current_loop(),
            depth: self.stack.len(),
            composed,
            every_loop_has_coordinates: self
                .stack
                .iter()
                .all(|&index| !self.loops[index].coords.is_empty()),
            bound: self.bound_depth > 0,
            ranges: self.range_stack.clone(),
        });
    }

    /// An iteration's element fetch reads its source at the loop's own
    /// coordinates, even when the binder is unused.
    fn fetch(&mut self, source: &HirExpression, coords: Vec<Coordinate>) {
        let facts = self.facts_of(source);
        self.push_access(facts, Some(coords), false, source.span);
    }

    /// A nested `filum` loop composes its coordinates after the enclosing
    /// chain and owns the locals its body declares; a plain nested loop is
    /// sequential inside the iteration and owns nothing new.
    fn nested_loop(
        &mut self,
        filum: bool,
        coordinates: &[(DefId, Symbol)],
        binding: DefId,
        mutable: bool,
        source: &HirExpression,
        body: &HirBlock,
    ) {
        let coords = coordinates_of(coordinates);
        if let (
            false,
            HirExpressionKind::Intervallum {
                start,
                end,
                step,
                kind,
            },
        ) = (filum, &source.kind)
        {
            // A counted range yields values, not storage elements: only its
            // bounds read, once, before the first iteration.
            self.bounds(
                std::iter::once(&**start)
                    .chain(std::iter::once(&**end))
                    .chain(step.as_deref()),
            );
            // An immutable binder with constant bounds keeps its proven
            // value range for the whole body: a flattened lane row's
            // residual is bounded by it.
            if let Some(range) = counted_range(
                self.current(),
                binding,
                mutable,
                start,
                end,
                step.as_deref(),
                *kind,
            ) {
                self.range_stack.push(range);
                self.visit_block(body);
                self.range_stack.pop();
            } else {
                self.visit_block(body);
            }
            return;
        }
        self.fetch(source, coords.clone());
        self.receiver(source);
        if !filum {
            self.visit_block(body);
            return;
        }
        self.enter_loop(coords, binding, source, body);
        self.visit_block(body);
        self.stack.pop();
    }

    /// The bounds of a counted loop are evaluated once per lane before the
    /// loop runs, so they may read lane-invariant data and the lane's own
    /// locals, never storage a lane writes.
    fn bounds<'e>(&mut self, bounds: impl Iterator<Item = &'e HirExpression>) {
        self.bound_depth += 1;
        for bound in bounds {
            self.visit_expr(bound);
        }
        self.bound_depth -= 1;
    }

    fn enter_loop(
        &mut self,
        coords: Vec<Coordinate>,
        binding: DefId,
        source: &HirExpression,
        body: &HirBlock,
    ) {
        let mut owned = Owned::default();
        owned.visit_block(body);
        owned.defs.insert(binding);
        self.loops.push(LoopCtx {
            coords,
            owned: owned.defs,
            source: self.facts_of(source),
        });
        self.stack.push(self.loops.len() - 1);
    }

    fn decline(&mut self, span: Span, reason: &str) {
        self.errors.push(issue(self.issue, span, reason));
    }

    fn is_storage(&self, ty: Option<TypeId>) -> bool {
        ty.is_some_and(|ty| {
            !is_register_value(Some(ty), self.types) && is_aggregate(self.types, ty)
        })
    }

    fn access(
        &mut self,
        object: &HirExpression,
        index: Option<&HirExpression>,
        write: bool,
        span: Span,
    ) {
        let facts = self.facts_of(object);
        let coords = index.and_then(|index| coordinates(self.current(), index));
        self.push_access(facts, coords, write, span);
    }

    fn receiver(&mut self, object: &HirExpression) {
        let before = self.suppress_reads;
        self.suppress_reads = true;
        self.visit_expr(object);
        self.suppress_reads = before;
    }

    fn write(&mut self, target: &HirExpression) {
        match &target.kind {
            // A lane store on a register-class value (`s[1] ← x` on a vector)
            // rewrites the value it indexes: the same rules as storing that
            // value whole, so a lane's own local is free and an outer one is
            // `outer_local_mutation`.
            HirExpressionKind::Index(object, index) if is_register_value(object.ty, self.types) => {
                self.write(object);
                self.visit_expr(index);
            }
            HirExpressionKind::Index(object, index) => {
                self.access(object, Some(index), true, target.span);
                self.receiver(object);
                self.visit_expr(index);
            }
            HirExpressionKind::Path(def) => {
                let borrowed = self.current().bindings.iter().any(|local| {
                    local.def_id == *def
                        && local.ty.is_some_and(|ty| {
                            self.types.is_mut_view(ty) || self.types.is_ref_view(ty)
                        })
                });
                let facts = self.facts_of(target);
                let owned = &self.owner().owned;
                if !owned.contains(def)
                    || borrowed
                        && (facts.unknown
                            || facts.storage.is_empty()
                            || facts.storage.iter().any(|s| !owned.contains(&s.root)))
                {
                    self.decline(target.span, "outer_local_mutation");
                }
            }
            HirExpressionKind::Field(object, _) => {
                let facts = self.facts_of(object);
                let owned = &self.owner().owned;
                if facts.unknown
                    || facts.storage.is_empty()
                    || facts.storage.iter().any(|s| !owned.contains(&s.root))
                {
                    self.decline(target.span, "outer_storage_mutation");
                }
                self.receiver(object);
            }
            _ => self.decline(target.span, "unknown_write_address"),
        }
    }

    /// An access-intrinsic or pure value-method call: record the storage
    /// access it names, then read the receiver and the arguments.
    fn intrinsic_method(
        &mut self,
        expr: &HirExpression,
        receiver: &HirExpression,
        name: Symbol,
        type_args: &[TypeId],
        args: &[HirCallArg],
    ) {
        match self.interner.resolve(name) {
            "accipe" if args.len() == 1 => {
                self.access(receiver, Some(&args[0].expr), false, expr.span);
            }
            "ponde" if args.len() == 2 => {
                self.access(receiver, Some(&args[0].expr), true, expr.span);
            }
            "transpone"
            | "__faber_glyph_transpose"
            | "sectio"
            | "forma"
            | "expande"
            | "disloco" => {}
            "limes" if builtin_edge_policy(type_args, self.types) => {}
            _ if is_pure_value_method(receiver, name, args.len(), self.types, self.interner) => {}
            _ => self.decline(expr.span, "unproved_call_effect"),
        }
        self.receiver(receiver);
        let before = self.suppress_reads;
        self.suppress_reads = false;
        for arg in args {
            self.visit_expr(&arg.expr);
        }
        self.suppress_reads = before;
    }
}

impl HirVisitor for BodyProof<'_> {
    fn visit_local(&mut self, local: &HirLocal) {
        walk_local(self, local);
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if let HirStatementKind::IncDec(inc) = &stmt.kind {
            self.write(&inc.target);
            self.visit_expr(&inc.target);
        } else {
            walk_stmt(self, stmt);
        }
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Assign(target, source) => {
                self.write(target);
                self.visit_expr(source);
            }
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
                ..
            } => {
                self.write(target);
                self.visit_expr(source);
                if let Some(recovery) = recovery {
                    self.visit_expr(recovery);
                }
                self.decline(expr.span, "unproved_conversion_effect");
            }
            HirExpressionKind::Index(object, index) => {
                // A component of a register-class value is no storage access,
                // nor is an element of a fixed-array field of a record: the
                // array is part of the record's own bytes, read as a value.
                if !is_register_value(object.ty, self.types)
                    && !is_record_array_field(object, self.types)
                {
                    self.access(object, Some(index), false, expr.span);
                }
                self.receiver(object);
                self.visit_expr(index);
            }
            HirExpressionKind::Path(_) | HirExpressionKind::Field(_, _)
                if !self.suppress_reads && self.is_storage(expr.ty) =>
            {
                self.access(expr, None, false, expr.span);
                if let HirExpressionKind::Field(object, _) = &expr.kind {
                    self.receiver(object);
                }
            }
            // A scalar or register-class field is a value: reading it does not
            // read the storage of the object that holds it.
            HirExpressionKind::Field(object, _) => self.receiver(object),
            // The typed zero `Type {}` (and the `empty` the checker rewrites
            // into it) allocates fresh storage, as the bare `vacua` did: it
            // writes nothing, so only its fill and extent arguments are read.
            HirExpressionKind::MethodCall(receiver, name, _, args, _)
                if is_construct_call(receiver, *name, args.len(), self.interner) =>
            {
                for arg in args {
                    self.visit_expr(&arg.expr);
                }
            }
            HirExpressionKind::MethodCall(receiver, name, type_args, args, _) => {
                if is_access_intrinsic(receiver, *name, args.len(), self.types, self.interner) {
                    self.intrinsic_method(expr, receiver, *name, type_args, args);
                } else {
                    self.method_call(expr, receiver, *name, args);
                }
            }
            HirExpressionKind::Call(callee, _, args, _) => self.call(expr, callee, args),
            HirExpressionKind::Ad { .. }
            | HirExpressionKind::CopyInto { .. }
            | HirExpressionKind::Gradient { .. }
            | HirExpressionKind::Reddet(_)
            | HirExpressionKind::Tacebit(_)
            | HirExpressionKind::Cede(_)
            | HirExpressionKind::Yield(_)
            | HirExpressionKind::Conversio { route: Some(_), .. } => {
                self.decline(expr.span, "unproved_call_effect");
                walk_expr(self, expr);
            }
            HirExpressionKind::Itera {
                filum,
                coordinates,
                binding,
                mutable,
                iter,
                body,
                ..
            } if filum.is_some() && !self.frames.is_empty() => {
                // A parallel loop inside a helper would nest under the call.
                self.decline(expr.span, "unproved_loop_form");
            }
            HirExpressionKind::Itera {
                filum,
                coordinates,
                binding,
                mutable,
                iter,
                body,
                ..
            } => self.nested_loop(filum.is_some(), coordinates, *binding, *mutable, iter, body),
            // A reduction folds inside the iteration: its accumulator is not
            // loop-carried state, and its pure term reads the source at the
            // reduction's own coordinates, so a written source still fails.
            HirExpressionKind::Reduction {
                source,
                coordinates,
                group,
                body,
                identity,
                ..
            } => {
                self.fetch(source, coordinates_of(coordinates));
                self.receiver(source);
                for part in group.iter().chain(identity.iter()) {
                    self.visit_expr(part);
                }
                if let Some(body) = body {
                    self.visit_block(body);
                }
            }
            HirExpressionKind::Scan {
                source,
                coordinates,
                group,
                body,
                ..
            } => {
                self.fetch(source, coordinates_of(coordinates));
                self.receiver(source);
                if let Some(group) = group {
                    self.visit_expr(group);
                }
                self.visit_block(body);
            }
            // The range product walks counted ranges sequentially inside the
            // iteration, like nested `while` loops: bounds are read once, the
            // binders are iteration-local, and the body meets the same rules.
            HirExpressionKind::IteraRange { ranges, body, .. } => {
                self.bounds(ranges.iter());
                self.visit_block(body);
            }
            HirExpressionKind::IteraLockstep { .. } => {
                self.decline(expr.span, "unproved_loop_form");
            }
            HirExpressionKind::Clausura(..)
            | HirExpressionKind::Handled { .. }
            | HirExpressionKind::Trap(_)
            | HirExpressionKind::OptionalChain(..) => {
                self.decline(expr.span, "unproved_body_form");
            }
            _ => walk_expr(self, expr),
        }
    }
}

#[derive(Default)]
struct Owned {
    defs: FxHashSet<DefId>,
}
impl HirVisitor for Owned {
    fn visit_def(&mut self, def: DefId, _: Symbol) {
        self.defs.insert(def);
    }
}

/// A coordinate component that is the same for every lane: a constant, or a
/// def declared outside the proved loop. Lane coordinates and the locals a lane
/// body declares differ per lane; rule 10 refuses a body write to any outer
/// local, so an outer def keeps one value for the whole loop.
fn lane_invariant(loops: &[LoopCtx], coord: &Coordinate) -> bool {
    coord.terms.iter().all(|(variable, _)| {
        !loops[0].owned.contains(variable)
            && !loops
                .iter()
                .any(|ctx| ctx.coords.iter().any(|c| c.names(*variable)))
    })
}

/// Whether `leading`, the leading storage coordinate of a write of a
/// one-coordinate lane loop, is that lane's own flattened row: `lane * pitch`
/// plus a residual whose value interval, over the counted-loop binder ranges
/// in scope at the write, stays inside `[0, pitch)`. The lane variable must
/// not be named again by the rest of the address. A stride that does not
/// cover the residual (`h[[k + j]]` with a wide `j`), an unbounded residual
/// and a reused lane coordinate all keep the decline.
fn flattened_row_leads(
    lane_coord: &Coordinate,
    leading: &Coordinate,
    rest: &[Coordinate],
    ranges: &[(DefId, (i128, i128))],
) -> bool {
    let Some(lane) = lane_coord.plain_variable() else {
        return false;
    };
    if !rest.iter().all(|coord| !coord.names(lane)) {
        return false;
    }
    let Some(pitch) = leading.coefficient(lane) else {
        return false;
    };
    if pitch < 1 {
        return false;
    }
    leading
        .clone()
        .without(lane)
        .interval(ranges)
        .is_some_and(|(lo, hi)| lo >= 0 && hi < pitch)
}

fn prove(
    body: &HirBlock,
    source: &HirExpression,
    coords: &[(DefId, Symbol)],
    binding: DefId,
    aliases: &Aliases,
    module: &Module,
    types: &TypeTable,
    interner: &Interner,
) -> Vec<SemanticError> {
    let mut proof = BodyProof::new(aliases, module, types, interner, ISSUE);
    let own = coordinates_of(coords);
    proof.enter_loop(own.clone(), binding, source, body);
    proof.fetch(source, own);
    proof.visit_block(body);
    let mut writes: FxHashMap<DefId, Vec<Coordinate>> = FxHashMap::default();
    for access in proof.accesses.iter().filter(|a| a.write) {
        if access.facts.unknown || access.facts.storage.is_empty() {
            proof
                .errors
                .push(issue(ISSUE, access.span, "unknown_storage_identity"));
        }
        let owner = &proof.loops[access.owner];
        for storage in &access.facts.storage {
            if owner.owned.contains(&storage.root) {
                continue;
            }
            let physical = access
                .coords
                .clone()
                .and_then(|coords| mapped(storage, coords));
            // Components every lane shares do not tell lanes apart: the
            // remainder must be exactly the lanes' own coordinates.
            let lane_part = physical.as_ref().map(|coords| {
                coords
                    .iter()
                    .filter(|c| !lane_invariant(&proof.loops, c))
                    .cloned()
                    .collect::<Vec<_>>()
            });
            // A single-coordinate lane loop may keep a whole private row in
            // scratch: once the leading storage coordinate is exactly the
            // lane coordinate — unchanged, and named by no other coordinate
            // of the address — inner coordinates only select cells within
            // that lane's disjoint row. A lane coordinate reused inside the
            // address (`scratch[[lane, lane]]`) is not row ownership, and
            // keeps the composed-coordinate proof, as do nests and
            // multidimensional lane domains, where the leading coordinate
            // alone may not distinguish every concurrent lane.
            let lane_variable = access.composed.first().and_then(Coordinate::plain_variable);
            let leading_lane_coordinate = access.depth == 1
                && access.composed.len() == 1
                && physical.as_ref().and_then(|coords| coords.first()) == access.composed.first()
                && physical.as_ref().is_some_and(|coords| {
                    coords
                        .iter()
                        .skip(1)
                        .all(|coord| lane_variable.is_none_or(|lane| !coord.names(lane)))
                });
            // The same one-coordinate lane loop may keep that row flattened
            // into one dimension: the leading storage coordinate is then
            // `lane * pitch + residual`, and once the counted-loop bounds in
            // scope at the write prove the residual inside `[0, pitch)`, the
            // lanes address disjoint index runs (row `k` is
            // `[k * pitch, (k + 1) * pitch)`), exactly as the unchanged
            // leading coordinate of a 2-D row does. Like the unchanged
            // leading coordinate, this is a one-coordinate lane loop's rule:
            // a multidimensional lane domain has concurrent lanes that share
            // the leading lane coordinate, so its writes must name every
            // lane coordinate and keep the composed-coordinate proof.
            let flattened_lane_row = access.depth == 1
                && access.composed.len() == 1
                && access
                    .composed
                    .first()
                    .zip(physical.as_ref().and_then(|coords| coords.split_first()))
                    .is_some_and(|(lane_coord, (leading, rest))| {
                        flattened_row_leads(lane_coord, leading, rest, &access.ranges)
                    });
            // One loop keeps its element fetch as the expected address (a write
            // into the storage the loop reads takes no invariant components).
            // In a nest the address must be the whole chain's composed
            // coordinate, so no inner iteration repeats a cell an outer one
            // also writes.
            let valid = if access.depth == 1 {
                let source_facts = &owner.source;
                let expected: Vec<_> = source_facts
                    .storage
                    .iter()
                    .filter(|s| s.root == storage.root)
                    .map(|s| mapped(s, access.composed.clone()))
                    .collect();
                !access.composed.is_empty()
                    && lane_part.is_some()
                    && (leading_lane_coordinate
                        || flattened_lane_row
                        || if expected.is_empty() {
                            lane_part.as_ref() == Some(&access.composed)
                        } else {
                            !source_facts.unknown
                                && expected.iter().all(|e| e.is_some() && *e == physical)
                        })
            } else {
                access.every_loop_has_coordinates && lane_part.as_ref() == Some(&access.composed)
            };
            if !valid {
                proof
                    .errors
                    .push(issue(ISSUE, access.span, "write_not_own_coordinate"));
            }
            if let Some(physical) = physical {
                if writes
                    .get(&storage.root)
                    .is_some_and(|old| *old != physical)
                {
                    proof
                        .errors
                        .push(issue(ISSUE, access.span, "inconsistent_written_coordinate"));
                }
                writes.insert(storage.root, physical);
            }
        }
    }
    for access in proof.accesses.iter().filter(|a| !a.write) {
        if writes.is_empty() {
            continue;
        }
        if access.facts.unknown || access.facts.storage.is_empty() {
            proof
                .errors
                .push(issue(ISSUE, access.span, "unknown_read_storage_identity"));
        }
        for storage in &access.facts.storage {
            if access.bound && writes.contains_key(&storage.root) {
                proof
                    .errors
                    .push(issue(ISSUE, access.span, "written_storage_neighbor_read"));
                continue;
            }
            if let Some(written) = writes.get(&storage.root) {
                let physical = access
                    .coords
                    .clone()
                    .and_then(|coords| mapped(storage, coords));
                if physical.as_ref() != Some(written) {
                    proof
                        .errors
                        .push(issue(ISSUE, access.span, "written_storage_neighbor_read"));
                }
            }
        }
    }
    proof.errors
}

/// The one diagnostic shape of this pass: a structured type mismatch naming
/// the issue and the reason. The issue is this pass's, or a reduction body's.
fn issue(issue: &'static str, span: Span, reason: &str) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
        .with_arg("issue", issue)
        .with_arg("reason", reason)
}

struct Loops<'a> {
    aliases: &'a Aliases,
    module: &'a Module<'a>,
    types: &'a TypeTable,
    interner: &'a Interner,
    errors: Vec<SemanticError>,
}
impl HirVisitor for Loops<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Itera {
            filum: Some(_),
            coordinates,
            binding,
            iter,
            body,
            ..
        } = &expr.kind
        {
            self.errors.extend(prove(
                body,
                iter,
                coordinates,
                *binding,
                self.aliases,
                self.module,
                self.types,
                self.interner,
            ));
            let mut ends = ends::Ends::new();
            ends.visit_block(body);
            self.errors.extend(ends.errors);
        } else {
            walk_expr(self, expr);
        }
    }
}

/// Whether a body holds a `filum` loop or a reduction with a term body.
#[derive(Default)]
struct NeedsProof(bool);
impl HirVisitor for NeedsProof {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(
            expr.kind,
            HirExpressionKind::Itera { filum: Some(_), .. }
                | HirExpressionKind::Reduction { body: Some(_), .. }
                | HirExpressionKind::Scan { .. }
        ) {
            self.0 = true;
        }
        if !self.0 {
            walk_expr(self, expr);
        }
    }
}

struct Scopes<'a> {
    module: &'a Module<'a>,
    types: &'a TypeTable,
    interner: &'a Interner,
    errors: Vec<SemanticError>,
}
impl Scopes<'_> {
    fn check_body(&mut self, params: &[HirParam], body: &HirBlock) {
        let mut finder = NeedsProof::default();
        finder.visit_block(body);
        if !finder.0 {
            return;
        }
        let aliases = Aliases::build(
            params,
            body,
            self.types,
            self.interner,
            ParamRoots::Classes,
            Some(self.module),
            None,
        );
        let mut loops = Loops {
            aliases: &aliases,
            module: self.module,
            types: self.types,
            interner: self.interner,
            errors: Vec::new(),
        };
        loops.visit_block(body);
        self.errors.extend(loops.errors);
        self.errors.extend(calls::reduction_term_errors(
            body,
            &aliases,
            self.module,
            self.types,
            self.interner,
        ));
    }
}
impl HirVisitor for Scopes<'_> {
    fn visit_block(&mut self, body: &HirBlock) {
        self.check_body(&[], body);
    }

    fn visit_function(&mut self, function: &HirFunction) {
        for param in &function.params {
            if let Some(default) = &param.default {
                self.visit_expr(default);
            }
        }
        if let Some(body) = &function.body {
            self.check_body(&function.params, body);
        }
    }
}

/// Validates `redde filum` placement independently of the lane-independence proof.
/// A lane records the nested-body depth at its entry: closures and reduction
/// bodies nested inside that lane cannot target it, but a lane nested inside
/// such a body is itself a valid target.
#[derive(Default)]
struct ThreadReturns {
    lane_nested_depths: Vec<u32>,
    nested_body_depth: u32,
    errors: Vec<SemanticError>,
}

impl ThreadReturns {
    fn refuse(&mut self, issue: &'static str, stmt: &HirStatement) {
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::TypeMismatch, stmt.span)
                .with_arg("issue", issue),
        );
    }
}

impl HirVisitor for ThreadReturns {
    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Itera {
                filum: Some(_),
                iter,
                body,
                ..
            } => {
                self.visit_expr(iter);
                self.lane_nested_depths.push(self.nested_body_depth);
                self.visit_block(body);
                self.lane_nested_depths.pop();
            }
            HirExpressionKind::Clausura(params, _, _, body) => {
                for param in params {
                    if let Some(default) = &param.default {
                        self.visit_expr(default);
                    }
                }
                self.nested_body_depth += 1;
                self.visit_expr(body);
                self.nested_body_depth -= 1;
            }
            HirExpressionKind::Reduction {
                source,
                group,
                body,
                identity,
                ..
            } => {
                self.visit_expr(source);
                if let Some(group) = group {
                    self.visit_expr(group);
                }
                if let Some(body) = body {
                    self.nested_body_depth += 1;
                    self.visit_block(body);
                    self.nested_body_depth -= 1;
                }
                if let Some(identity) = identity {
                    self.visit_expr(identity);
                }
            }
            HirExpressionKind::Scan {
                source,
                group,
                body,
                ..
            } => {
                self.visit_expr(source);
                if let Some(group) = group {
                    self.visit_expr(group);
                }
                self.nested_body_depth += 1;
                self.visit_block(body);
                self.nested_body_depth -= 1;
            }
            HirExpressionKind::Sort { source, body, .. } => {
                self.visit_expr(source);
                if let Some(body) = body {
                    self.nested_body_depth += 1;
                    self.visit_block(body);
                    self.nested_body_depth -= 1;
                }
            }
            _ => walk_expr(self, expr),
        }
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if let HirStatementKind::ReddeFilum(value) = &stmt.kind {
            if value.is_some() {
                self.refuse("return_thread_with_value", stmt);
            } else {
                match self.lane_nested_depths.last() {
                    Some(depth) if *depth == self.nested_body_depth => {}
                    Some(_) => self.refuse("return_thread_in_nested_body", stmt),
                    None => self.refuse("return_thread_outside_lane", stmt),
                }
            }
        }
        walk_stmt(self, stmt);
    }
}

/// All targets share this admission before target-sensitive HIR rewrites.
pub(crate) fn check(hir: &HirModule, types: &TypeTable, interner: &Interner) -> Vec<SemanticError> {
    let module = Module::build(hir, types, interner);
    let mut scopes = Scopes {
        module: &module,
        types,
        interner,
        errors: Vec::new(),
    };
    for item in &hir.items {
        scopes.visit_item(item);
    }
    if let Some(entry) = &hir.entry {
        scopes.check_body(&[], entry);
    }
    let mut thread_returns = ThreadReturns::default();
    thread_returns.visit_program(hir);
    scopes.errors.extend(thread_returns.errors);
    scopes.errors
}
