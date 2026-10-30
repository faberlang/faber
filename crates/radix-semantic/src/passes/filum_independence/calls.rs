//! Calls inside a `filum` body (or a reduction term), seen through.
//!
//! gpu-reset rule 12.7 inlines a kernel called from a kernel, so the proof
//! analyses such a callee as if its body stood at the call site. The callee's
//! tensor parameters take the storage of the call's arguments, its coordinate
//! parameters take the arguments' coordinates, and its body meets the rules of
//! the enclosing body unchanged. A call whose callee is itself a foreign value
//! (a selectively imported target binding, or a method of one) is a host
//! effect and declines as `foreign_call_effect`; a call to anything else keeps
//! the `unproved_call_effect` decline.
//!
//! The inlinable callees are the module's `@ nucleum` functions and instance
//! methods (`fragment`s and entry kernels, public or private), synchronous and
//! infallible. Their host-freedom is the kernel legality pass's verdict, which
//! runs after this pass and is not consulted: this pass needs no more than its
//! own rules, because every unproved effect in a callee body declines exactly
//! as it would in the caller.
//!
//! A method call resolves statically: the receiver's type names one class
//! (genus have no inheritance), and the class and method name name one method.
//! A receiver typed by a contract or a type parameter is dispatched at run
//! time, so it declines by name (`dynamic_dispatch`). Inside a method body
//! `ego` is the class's own definition, and a tensor-typed instance field is
//! storage with a root of its own (the field's definition); a call maps `ego`
//! to the receiver's storage and each such field to the storage the receiver
//! holds in that field. Overlap is then judged on the mapped roots like any
//! other access. The call-site borrow rule checks the arguments of a
//! user-method call by their places, so it catches two arguments spelled as
//! the same tensor. It cannot see that an argument and the receiver's field
//! name one storage when the receiver holds that argument, so a method call
//! also declines when two roots it receives, one of them writable, name the
//! same storage (`overlapping_call_arguments`). This pass is the sole owner of
//! the receiver-field spelling.
//!
//! This file also owns the scan that bounds what a call outside a `filum`
//! body can disturb in the alias facts.

use super::{
    Aliases, BodyProof, Coordinate, Facts, LoopCtx, Owned, ParamRoots, Storage, coordinate,
    is_access_intrinsic, is_aggregate, param_root,
};
use crate::passes::fragment_definition::struct_def_from_type;
use crate::{Primitive, SemanticError, SemanticErrorKind, Type, TypeId, TypeTable};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{HirVisitor, walk_expr, walk_stmt};
use radix_hir::{
    DefId, HirBlock, HirCallArg, HirExpression, HirExpressionKind, HirFunction, HirItemKind,
    HirModule, HirParamMode, HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, Symbol};

/// Deepest chain of helper calls analysed as inlined. Rule 12.7 inlines
/// unconditionally for now; a chain past this bound is declined by name
/// (`inline_depth_exceeded`) rather than analysed without limit.
pub(super) const MAX_INLINE_DEPTH: usize = 8;

/// The issue a reduction term reports: the filum issue is about loops.
const REDUCTION_EFFECT_ISSUE: &str = "reducta_body_effect";

/// What an instance-method callee takes from its receiver.
pub(super) struct Receiver {
    /// `ego` in the body is the path to the class definition.
    pub(super) ego: DefId,
    /// Tensor-typed instance fields, by name; the field definition is the
    /// root a body access through `ego.<field>` carries.
    pub(super) fields: Vec<(Symbol, DefId)>,
}

/// One inlinable callee and the facts its analysis reuses at every call.
pub(super) struct Helper<'a> {
    function: &'a HirFunction,
    /// Set for an instance method.
    receiver: Option<Receiver>,
    aliases: Aliases,
    /// Defs one call owns: body locals and by-value scalar parameters.
    owned: FxHashSet<DefId>,
    /// Whether a call can leave the caller's alias facts as they were: the
    /// body rebinds no parameter, stores into no field of parameter storage,
    /// and calls only inert helpers.
    inert: bool,
}

/// The inlinable callees of one module, keyed by definition.
pub(super) struct Module<'a> {
    helpers: FxHashMap<DefId, Helper<'a>>,
    /// Instance methods by (class, name): the static resolution of a method
    /// call, whether or not the method is a kernel.
    methods: FxHashMap<(DefId, Symbol), DefId>,
}

impl<'a> Module<'a> {
    pub(super) fn build(hir: &'a HirModule, types: &TypeTable, interner: &Interner) -> Self {
        let mut helpers = FxHashMap::default();
        let mut methods = FxHashMap::default();
        for item in &hir.items {
            match &item.kind {
                HirItemKind::Function(function) => {
                    add_helper(&mut helpers, item.def_id, function, None, types, interner);
                }
                HirItemKind::Struct(strukt) => {
                    for method in &strukt.methods {
                        // HIR carries no receiver mode (lowering leaves it `None`
                        // for every method): every genus method is an instance
                        // method, reached through its receiver.
                        methods.insert((item.def_id, method.func.name), method.def_id);
                        let receiver = Receiver {
                            ego: item.def_id,
                            fields: strukt
                                .fields
                                .iter()
                                .filter(|field| {
                                    !field.is_static() && is_tensor_typed(types, field.ty)
                                })
                                .map(|field| (field.name, field.def_id))
                                .collect(),
                        };
                        add_helper(
                            &mut helpers,
                            method.def_id,
                            &method.func,
                            Some(receiver),
                            types,
                            interner,
                        );
                    }
                }
                _ => {}
            }
        }
        let calls = mark_inert(&mut helpers, types, interner);
        settle_inert(&mut helpers, &calls);
        Self { helpers, methods }
    }

    /// The method a call `receiver.name(..)` is statically bound to, or why
    /// it is not.
    fn resolve_method(
        &self,
        receiver: &HirExpression,
        name: Symbol,
        types: &TypeTable,
    ) -> Resolved<'_, 'a> {
        let Some(ty) = receiver.ty else {
            return Resolved::Opaque;
        };
        if is_dispatched_at_run_time(types, ty) {
            return Resolved::Dynamic;
        }
        let Some(def) = struct_def_from_type(ty, types)
            .and_then(|class| self.methods.get(&(class, name)))
            .copied()
        else {
            return Resolved::Opaque;
        };
        self.helpers
            .get(&def)
            .map_or(Resolved::Opaque, |helper| Resolved::Helper(def, helper))
    }

    /// A call to `def` leaves the alias facts of its caller as they were.
    pub(super) fn is_inert(&self, def: DefId) -> bool {
        self.helpers.get(&def).is_some_and(|helper| helper.inert)
    }
}

/// How a method call resolves.
enum Resolved<'m, 'a> {
    Helper(DefId, &'m Helper<'a>),
    /// Dispatched on a contract or a type parameter: no one body to look through.
    Dynamic,
    /// Not a kernel method of this module (a host method, a function-valued
    /// field, an imported class).
    Opaque,
}

fn add_helper<'a>(
    helpers: &mut FxHashMap<DefId, Helper<'a>>,
    def: DefId,
    function: &'a HirFunction,
    receiver: Option<Receiver>,
    types: &TypeTable,
    interner: &Interner,
) {
    let Some(body) = &function.body else {
        return;
    };
    if !is_inlinable_kernel(function) {
        return;
    }
    let mut owned = Owned::default();
    owned.visit_block(body);
    owned.defs.extend(
        function
            .params
            .iter()
            .filter(|param| !is_aggregate(types, param.ty))
            .map(|param| param.def_id),
    );
    let aliases = Aliases::build(
        &function.params,
        body,
        types,
        interner,
        ParamRoots::PerParam,
        None,
        receiver.as_ref(),
    );
    helpers.insert(
        def,
        Helper {
            function,
            receiver,
            aliases,
            owned: owned.defs,
            inert: false,
        },
    );
}

/// Mark every helper whose own body cannot disturb its caller's alias facts
/// and return the helpers each one calls.
fn mark_inert(
    helpers: &mut FxHashMap<DefId, Helper<'_>>,
    types: &TypeTable,
    interner: &Interner,
) -> FxHashMap<DefId, Vec<DefId>> {
    let defs: FxHashSet<DefId> = helpers.keys().copied().collect();
    let mut calls = FxHashMap::default();
    for (def, helper) in helpers.iter_mut() {
        let Some(body) = &helper.function.body else {
            continue;
        };
        let mut reach = Reach {
            aliases: &helper.aliases,
            params: helper
                .function
                .params
                .iter()
                .map(|p| p.def_id)
                .chain(helper.receiver.iter().map(|receiver| receiver.ego))
                .collect(),
            helpers: &defs,
            types,
            interner,
            disturbs: false,
            calls: Vec::new(),
        };
        reach.visit_block(body);
        helper.inert = !reach.disturbs;
        calls.insert(*def, reach.calls);
    }
    calls
}

/// A helper that calls a non-inert helper is not inert; a cycle of inert
/// helpers stays inert (recursion is declined where it is analysed).
fn settle_inert(helpers: &mut FxHashMap<DefId, Helper<'_>>, calls: &FxHashMap<DefId, Vec<DefId>>) {
    loop {
        let stale: Vec<DefId> = calls
            .iter()
            .filter(|(def, called)| {
                helpers[*def].inert && called.iter().any(|callee| !helpers[callee].inert)
            })
            .map(|(def, _)| *def)
            .collect();
        if stale.is_empty() {
            return;
        }
        for def in stale {
            if let Some(helper) = helpers.get_mut(&def) {
                helper.inert = false;
            }
        }
    }
}

/// What a helper body can do to alias facts outside it.
struct Reach<'a> {
    aliases: &'a Aliases,
    params: FxHashSet<DefId>,
    helpers: &'a FxHashSet<DefId>,
    types: &'a TypeTable,
    interner: &'a Interner,
    disturbs: bool,
    calls: Vec<DefId>,
}

impl Reach<'_> {
    /// Element stores change no alias fact. Rebinding a parameter writes
    /// through to the caller's variable; a field store changes the field
    /// facts of its object's roots, which only a helper-local root keeps
    /// out of the caller's reach.
    fn store(&mut self, target: &HirExpression) {
        match &target.kind {
            HirExpressionKind::Path(def) => self.disturbs |= self.params.contains(def),
            HirExpressionKind::Field(object, _) => {
                let facts = self.aliases.facts(object, self.types, self.interner);
                self.disturbs |= facts.unknown
                    || facts.storage.is_empty()
                    || facts
                        .storage
                        .iter()
                        .any(|storage| self.params.contains(&storage.root));
            }
            _ => {}
        }
    }
}

impl HirVisitor for Reach<'_> {
    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if let HirStatementKind::IncDec(inc) = &stmt.kind {
            self.store(&inc.target);
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Assign(target, _)
            | HirExpressionKind::ConversioAssign { target, .. } => self.store(target),
            HirExpressionKind::Call(callee, ..) => match &callee.kind {
                HirExpressionKind::Path(def) if self.helpers.contains(def) => self.calls.push(*def),
                _ => self.disturbs = true,
            },
            HirExpressionKind::Ad { .. }
            | HirExpressionKind::CopyInto { .. }
            | HirExpressionKind::Reddet(_)
            | HirExpressionKind::Tacebit(_)
            | HirExpressionKind::Clausura(..)
            | HirExpressionKind::Conversio { route: Some(_), .. } => self.disturbs = true,
            HirExpressionKind::MethodCall(receiver, name, _, args, _)
                if !is_access_intrinsic(receiver, *name, args.len(), self.types, self.interner)
                    && !super::is_construct_call(receiver, *name, args.len(), self.interner) =>
            {
                self.disturbs = true;
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

/// Whether a callable is a kernel that rule 12.7 inlines: `@ nucleum`
/// (fragment or entry, public or private) and synchronous. A kernel may
/// declare `⇥ text` (KTHROW, ruling 2026-10-08): its failure follows the
/// lane body's own recovery rules, and kernel legality admits no handler.
pub(crate) fn is_inlinable_kernel(function: &HirFunction) -> bool {
    function.nucleum_role().is_some()
        && !function.is_async
        && !function.posture.is_async()
        && !function.is_generator
        && !function.posture.is_stream()
}

/// A bare tensor (through an access marker): the one field type with a
/// storage root of its own.
fn is_tensor_typed(types: &TypeTable, ty: TypeId) -> bool {
    let (_, inner) = types.peel_access_marker(ty);
    matches!(types.get(inner), Type::Tensor(_, _))
}

/// Whether the receiver's type is a foreign value (`ignotum`): a method called
/// on it is a host effect with no Faber body to look through.
fn is_foreign_callee(ty: Option<TypeId>, types: &TypeTable) -> bool {
    ty.is_some_and(|ty| matches!(types.get(ty), Type::Primitive(Primitive::Ignotum)))
}

/// A receiver typed by a contract or a type parameter: which body a call
/// reaches is decided at run time (or per instantiation), not here.
fn is_dispatched_at_run_time(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::Interface(_) | Type::Param(_) => true,
        Type::Ref(_, inner) | Type::Alias(_, inner) => is_dispatched_at_run_time(types, *inner),
        Type::Applied(base, _) => is_dispatched_at_run_time(types, *base),
        _ => false,
    }
}

/// A helper being analysed as inlined at one call site.
pub(super) struct Frame<'a> {
    callee: DefId,
    aliases: &'a Aliases,
    /// Callee tensor parameter -> storage of the argument, already in the
    /// terms of the body that owns the loop.
    roots: FxHashMap<DefId, Facts>,
    /// Callee parameter -> coordinate of its argument in the same terms;
    /// `None` when the argument is not a coordinate. Parameters are immutable,
    /// so a body never changes the value a parameter names.
    coords: FxHashMap<DefId, Option<Coordinate>>,
}

impl<'a> BodyProof<'a> {
    pub(super) fn new(
        aliases: &'a Aliases,
        module: &'a Module<'a>,
        types: &'a TypeTable,
        interner: &'a Interner,
        issue: &'static str,
    ) -> Self {
        Self {
            aliases,
            module,
            types,
            interner,
            loops: Vec::new(),
            stack: Vec::new(),
            range_stack: Vec::new(),
            frames: Vec::new(),
            accesses: Vec::new(),
            errors: Vec::new(),
            suppress_reads: false,
            bound_depth: 0,
            issue,
            skip_arguments: false,
        }
    }

    /// The alias facts of the body the cursor is in.
    pub(super) fn current(&self) -> &'a Aliases {
        self.frames
            .last()
            .map_or(self.aliases, |frame| frame.aliases)
    }

    /// Storage facts of `expr`, in the terms of the body that owns the loop:
    /// a helper's parameter roots become the storage of the arguments.
    pub(super) fn facts_of(&self, expr: &HirExpression) -> Facts {
        let Some(frame) = self.frames.last() else {
            return self.aliases.facts(expr, self.types, self.interner);
        };
        let facts = frame.aliases.facts(expr, self.types, self.interner);
        let mut resolved = Facts {
            unknown: facts.unknown,
            storage: Vec::new(),
        };
        for storage in facts.storage {
            let Some(argument) = frame.roots.get(&storage.root) else {
                // A helper-local root: iteration-local, owned by the loop.
                resolved.merge(Facts {
                    storage: vec![storage],
                    unknown: false,
                });
                continue;
            };
            resolved.unknown |= argument.unknown;
            for base in &argument.storage {
                let mut transforms = base.transforms.clone();
                transforms.extend(storage.transforms.iter().cloned());
                resolved.merge(Facts {
                    storage: vec![Storage {
                        root: base.root,
                        transforms,
                    }],
                    unknown: false,
                });
            }
        }
        resolved
    }

    /// A helper's coordinate in the terms of the loop; `None` when any
    /// component is not provably the caller's coordinate.
    pub(super) fn map_coords(&self, coords: Vec<Coordinate>) -> Option<Vec<Coordinate>> {
        let Some(frame) = self.frames.last() else {
            return Some(coords);
        };
        coords
            .into_iter()
            .map(|coord| substitute(frame, coord))
            .collect()
    }

    /// A call in a body under proof. A helper is analysed as inlined; a
    /// foreign callee is a host effect; any other callee is an unproved
    /// effect.
    pub(super) fn call(
        &mut self,
        expr: &HirExpression,
        callee: &HirExpression,
        args: &[HirCallArg],
    ) {
        let module = self.module;
        if is_foreign_callee(callee.ty, self.types) {
            return self.decline_call(expr, "foreign_call_effect");
        }
        let helper = match &callee.kind {
            HirExpressionKind::Path(def) => module.helpers.get(def).map(|helper| (*def, helper)),
            _ => None,
        };
        let Some((def, helper)) = helper else {
            return self.decline_call(expr, "unproved_call_effect");
        };
        // A method is called through its receiver, never by path.
        if helper.receiver.is_some() {
            return self.decline_call(expr, "unproved_call_effect");
        }
        self.inline(expr, def, helper, None, args);
    }

    /// A method call in a body under proof. The receiver's type binds it to
    /// one method; that method is analysed as inlined with `ego` mapped to the
    /// receiver. A call that binds to no kernel method is an unproved effect.
    pub(super) fn method_call(
        &mut self,
        expr: &HirExpression,
        receiver: &HirExpression,
        name: Symbol,
        args: &[HirCallArg],
    ) {
        let module = self.module;
        if is_foreign_callee(receiver.ty, self.types) {
            return self.decline_call(expr, "foreign_call_effect");
        }
        match module.resolve_method(receiver, name, self.types) {
            Resolved::Helper(def, helper) => self.inline(expr, def, helper, Some(receiver), args),
            Resolved::Dynamic => self.decline_call(expr, "dynamic_dispatch"),
            Resolved::Opaque => self.decline_call(expr, "unproved_call_effect"),
        }
    }

    /// Whether the call `receiver.name(..)` resolves to a kernel method this
    /// pass looks through.
    pub(super) fn is_method_helper(&self, receiver: &HirExpression, name: Symbol) -> bool {
        matches!(
            self.module.resolve_method(receiver, name, self.types),
            Resolved::Helper(..)
        )
    }

    fn inline(
        &mut self,
        expr: &HirExpression,
        def: DefId,
        helper: &'a Helper<'a>,
        receiver: Option<&HirExpression>,
        args: &[HirCallArg],
    ) {
        let function = helper.function;
        if args.len() != function.params.len()
            || args.iter().any(|arg| arg.spread || arg.name.is_some())
        {
            return self.decline_call(expr, "unproved_call_effect");
        }
        if self.frames.iter().any(|frame| frame.callee == def) {
            return self.decline_call(expr, "recursive_call");
        }
        if self.frames.len() >= MAX_INLINE_DEPTH {
            return self.decline_call(expr, "inline_depth_exceeded");
        }
        let frame = self.bind_arguments(def, helper, receiver, args);
        if receiver.is_some() && self.roots_overlap(helper, &frame) {
            return self.decline_call(expr, "overlapping_call_arguments");
        }
        if let Some(receiver) = receiver
            && !self.skip_arguments
        {
            self.receiver(receiver);
        }
        self.visit_arguments(args);
        let owner = *self.stack.last().expect("inside a filum loop");
        self.loops[owner].owned.extend(helper.owned.iter().copied());
        self.frames.push(frame);
        let before = std::mem::replace(&mut self.suppress_reads, false);
        if let Some(body) = &function.body {
            self.visit_block(body);
        }
        self.suppress_reads = before;
        self.frames.pop();
    }

    fn decline_call(&mut self, expr: &HirExpression, reason: &str) {
        self.decline(expr.span, reason);
        walk_expr(self, expr);
    }

    fn bind_arguments(
        &self,
        callee: DefId,
        helper: &'a Helper<'a>,
        receiver: Option<&HirExpression>,
        args: &[HirCallArg],
    ) -> Frame<'a> {
        let mut roots = FxHashMap::default();
        let mut coords = FxHashMap::default();
        let mut shared = None;
        for (param, arg) in helper.function.params.iter().zip(args) {
            if param_root(param, self.types, ParamRoots::PerParam, &mut shared).is_some() {
                roots.insert(param.def_id, self.facts_of(&arg.expr));
            }
            let coord = coordinate(self.current(), &arg.expr)
                .and_then(|coord| self.map_coords(vec![coord])?.pop());
            coords.insert(param.def_id, coord);
        }
        if let (Some(model), Some(receiver)) = (&helper.receiver, receiver) {
            roots.insert(model.ego, self.facts_of(receiver));
            for (name, field) in &model.fields {
                let held = HirExpression {
                    id: receiver.id,
                    kind: HirExpressionKind::Field(Box::new(receiver.clone()), *name),
                    ty: None,
                    span: receiver.span,
                };
                roots.insert(*field, self.facts_of(&held));
            }
        }
        Frame {
            callee,
            aliases: &helper.aliases,
            roots,
            coords,
        }
    }

    /// Two roots a method call receives name the same storage and one of them
    /// is writable: the receiver's tensor fields, and the tensor parameters
    /// that are not shared (`de`, `ref`). Unknown or empty facts do not count
    /// here; an access through them declines where it is analysed.
    fn roots_overlap(&self, helper: &Helper<'_>, frame: &Frame<'_>) -> bool {
        let mut roots: Vec<(&Facts, bool)> = Vec::new();
        if let Some(model) = &helper.receiver {
            for (_, field) in &model.fields {
                roots.extend(frame.roots.get(field).map(|facts| (facts, true)));
            }
        }
        for param in &helper.function.params {
            let shared = matches!(param.mode, HirParamMode::De) || self.types.is_ref_view(param.ty);
            roots.extend(frame.roots.get(&param.def_id).map(|facts| (facts, !shared)));
        }
        roots
            .iter()
            .enumerate()
            .any(|(index, (left, left_writable))| {
                roots[index + 1..].iter().any(|(right, right_writable)| {
                    (*left_writable || *right_writable)
                        && left
                            .storage
                            .iter()
                            .any(|a| right.storage.iter().any(|b| a.root == b.root))
                })
            })
    }

    /// The argument expressions run in the caller before the body does. A
    /// storage-typed argument is a binding, not a read: the helper's own
    /// accesses through the parameter are the reads.
    fn visit_arguments(&mut self, args: &[HirCallArg]) {
        if self.skip_arguments {
            return;
        }
        let before = self.suppress_reads;
        for arg in args {
            self.suppress_reads = self.is_storage(arg.expr.ty);
            self.visit_expr(&arg.expr);
        }
        self.suppress_reads = before;
    }
}

/// Each coordinate with its frame parameters replaced by the argument's
/// coordinate: `c · p + rest` becomes `c · (argument) + rest`, folding
/// constants into the offset. A parameter whose argument is not a provable
/// address declines.
fn substitute(frame: &Frame<'_>, coord: Coordinate) -> Option<Coordinate> {
    let mut result = Coordinate::constant(coord.offset);
    for (variable, coefficient) in coord.terms {
        let argument = frame.coords.get(&variable)?.as_ref()?;
        result = result.plus(&argument.clone().scaled(coefficient)?)?;
    }
    Some(result)
}

// ---------------------------------------------------------------------------
// Reduction terms
// ---------------------------------------------------------------------------

/// Helper calls inside the term of a reduction outside any `filum` body. The
/// term is typechecked as a pure fold; a private kernel helper it calls must
/// be proved to write nothing the reduction does not own.
pub(super) fn reduction_term_errors(
    body: &HirBlock,
    aliases: &Aliases,
    module: &Module<'_>,
    types: &TypeTable,
    interner: &Interner,
) -> Vec<SemanticError> {
    let mut scan = ReductionTerms {
        aliases,
        module,
        types,
        interner,
        errors: Vec::new(),
    };
    scan.visit_block(body);
    scan.errors
}

struct ReductionTerms<'a> {
    aliases: &'a Aliases,
    module: &'a Module<'a>,
    types: &'a TypeTable,
    interner: &'a Interner,
    errors: Vec<SemanticError>,
}

impl HirVisitor for ReductionTerms<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            // A filum body is the loop proof's: its reductions fold inside
            // the iteration and see their helper calls there.
            HirExpressionKind::Itera {
                filum: Some(_),
                iter,
                ..
            } => self.visit_expr(iter),
            HirExpressionKind::Reduction {
                coordinates,
                filum,
                binding,
                body: Some(body),
                ..
            } => {
                walk_expr(self, expr);
                let mut owned = Owned::default();
                owned.visit_block(body);
                owned.defs.extend(coordinates.iter().map(|(def, _)| *def));
                owned.defs.extend(filum.iter().map(|(def, _)| *def));
                owned.defs.extend(binding.iter().map(|(def, _)| *def));
                self.term(body, owned.defs);
            }
            HirExpressionKind::Scan {
                coordinates,
                filum,
                binding,
                body,
                ..
            } => {
                walk_expr(self, expr);
                let mut owned = Owned::default();
                owned.visit_block(body);
                owned.defs.extend(coordinates.iter().map(|(def, _)| *def));
                owned.defs.extend([filum.0, binding.0]);
                self.term(body, owned.defs);
            }
            _ => walk_expr(self, expr),
        }
    }
}

impl ReductionTerms<'_> {
    fn term(&mut self, body: &HirBlock, owned: FxHashSet<DefId>) {
        let mut proof = BodyProof::new(
            self.aliases,
            self.module,
            self.types,
            self.interner,
            REDUCTION_EFFECT_ISSUE,
        );
        proof.skip_arguments = true;
        proof.loops.push(LoopCtx {
            coords: Vec::new(),
            owned,
            source: Facts::default(),
        });
        proof.stack.push(0);
        let mut calls = TermCalls { proof };
        calls.visit_block(body);
        let mut proof = calls.proof;
        for access in proof.accesses.iter().filter(|access| access.write) {
            let owned = &proof.loops[access.owner].owned;
            if access.facts.unknown
                || access.facts.storage.is_empty()
                || access
                    .facts
                    .storage
                    .iter()
                    .any(|s| !owned.contains(&s.root))
            {
                proof.errors.push(
                    SemanticError::structured(SemanticErrorKind::TypeMismatch, access.span)
                        .with_arg("issue", REDUCTION_EFFECT_ISSUE)
                        .with_arg("reason", "captured_mut_host"),
                );
            }
        }
        self.errors.extend(proof.errors);
    }
}

/// Runs the inlined analysis on each helper call of a term, and nothing else:
/// the typechecker owns the rest of the term's body.
struct TermCalls<'a> {
    proof: BodyProof<'a>,
}

impl HirVisitor for TermCalls<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Call(callee, _, args, _) = &expr.kind
            && let HirExpressionKind::Path(def) = &callee.kind
            && self.proof.module.helpers.contains_key(def)
        {
            self.proof.call(expr, callee, args);
            for arg in args {
                self.visit_expr(&arg.expr);
            }
            return;
        }
        if let HirExpressionKind::MethodCall(receiver, name, _, args, _) = &expr.kind
            && self.proof.is_method_helper(receiver, *name)
        {
            self.proof.method_call(expr, receiver, *name, args);
            self.visit_expr(receiver);
            for arg in args {
                self.visit_expr(&arg.expr);
            }
            return;
        }
        walk_expr(self, expr);
    }
}

// ---------------------------------------------------------------------------
// Effects outside a filum body
// ---------------------------------------------------------------------------

/// Collects what the effects outside every `filum` body can disturb. A call
/// to a named function can reach only its arguments (and what they reach);
/// any other effect is opaque and poisons the whole function.
pub(super) struct EffectScan<'a> {
    module: Option<&'a Module<'a>>,
    types: &'a TypeTable,
    interner: &'a Interner,
    /// Params and locals of the analysed function: a call through one is a
    /// call through a closure value.
    locals: &'a FxHashSet<DefId>,
    pub(super) opaque: bool,
    /// Argument expressions of the named calls.
    pub(super) calls: Vec<HirExpression>,
}

impl<'a> EffectScan<'a> {
    pub(super) fn new(
        module: Option<&'a Module<'a>>,
        types: &'a TypeTable,
        interner: &'a Interner,
        locals: &'a FxHashSet<DefId>,
    ) -> Self {
        Self {
            module,
            types,
            interner,
            locals,
            opaque: false,
            calls: Vec::new(),
        }
    }

    fn carries_function(&self, arg: &HirCallArg) -> bool {
        matches!(arg.expr.kind, HirExpressionKind::Clausura(..))
            || arg.expr.ty.is_some_and(|ty| {
                let (_, ty) = self.types.peel_access_marker(ty);
                matches!(self.types.get(ty), Type::Func(_))
            })
    }
}

impl HirVisitor for EffectScan<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            // The loop proof inlines or declines every effect in its body.
            HirExpressionKind::Itera {
                filum: Some(_),
                iter,
                ..
            } => {
                self.visit_expr(iter);
                return;
            }
            HirExpressionKind::Call(callee, _, args, _) => match &callee.kind {
                // An inert helper leaves every alias fact as it was.
                HirExpressionKind::Path(def)
                    if self.module.is_some_and(|module| module.is_inert(*def)) => {}
                HirExpressionKind::Path(def)
                    if !self.locals.contains(def)
                        && !args.iter().any(|arg| self.carries_function(arg)) =>
                {
                    self.calls.extend(args.iter().map(|arg| arg.expr.clone()));
                }
                _ => self.opaque = true,
            },
            HirExpressionKind::Ad { .. }
            | HirExpressionKind::CopyInto { .. }
            | HirExpressionKind::Reddet(_)
            | HirExpressionKind::Tacebit(_)
            | HirExpressionKind::Conversio { route: Some(_), .. } => self.opaque = true,
            HirExpressionKind::MethodCall(receiver, name, _, args, _)
                if !is_access_intrinsic(receiver, *name, args.len(), self.types, self.interner)
                    && !super::is_construct_call(receiver, *name, args.len(), self.interner) =>
            {
                self.opaque = true;
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}
