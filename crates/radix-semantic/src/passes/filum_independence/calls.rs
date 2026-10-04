//! Calls inside a `filum` body (or a reduction term), seen through.
//!
//! gpu-reset rule 12.7 inlines a private kernel called from a kernel, so the
//! proof analyses such a callee as if its body stood at the call site. The
//! callee's tensor parameters take the storage of the call's arguments, its
//! coordinate parameters take the arguments' coordinates, and its body meets
//! the rules of the enclosing body unchanged. A call to anything else keeps
//! the `unproved_call_effect` decline.
//!
//! The inlinable callees are the module's `@ nucleum` functions that are
//! `fragment`s or private (non-`publica`) entry kernels, synchronous and
//! infallible. Their host-freedom is the kernel legality pass's verdict; this
//! pass needs no more than its own rules, because every unproved effect in a
//! callee body declines exactly as it would in the caller.
//!
//! This file also owns the scan that bounds what a call outside a `filum`
//! body can disturb in the alias facts.

use super::{
    Aliases, BodyProof, Coordinate, Facts, LoopCtx, Owned, ParamRoots, Storage, coordinate,
    is_access_intrinsic, is_aggregate, param_root,
};
use crate::{SemanticError, SemanticErrorKind, Type, TypeTable};
use radix_hir::visit::{HirVisitor, walk_expr, walk_stmt};
use radix_hir::{
    DefId, HirAnnotation, HirBlock, HirCallArg, HirExpression, HirExpressionKind, HirFunction,
    HirItemKind, HirModule, HirStatement, HirStatementKind, NucleumRole,
};
use radix_lexer::Interner;
use rustc_hash::{FxHashMap, FxHashSet};

/// Deepest chain of helper calls analysed as inlined. Rule 12.7 inlines
/// unconditionally for now; a chain past this bound is declined by name
/// (`inline_depth_exceeded`) rather than analysed without limit.
pub(super) const MAX_INLINE_DEPTH: usize = 8;

/// The issue a reduction term reports: the filum issue is about loops.
const REDUCTION_EFFECT_ISSUE: &str = "reducta_body_effect";

/// One inlinable callee and the facts its analysis reuses at every call.
pub(super) struct Helper<'a> {
    function: &'a HirFunction,
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
}

impl<'a> Module<'a> {
    pub(super) fn build(hir: &'a HirModule, types: &TypeTable, interner: &Interner) -> Self {
        let mut helpers = FxHashMap::default();
        for item in &hir.items {
            let HirItemKind::Function(function) = &item.kind else {
                continue;
            };
            let Some(body) = &function.body else {
                continue;
            };
            if !is_inlinable(function, &item.annotations, interner) {
                continue;
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
            helpers.insert(
                item.def_id,
                Helper {
                    function,
                    aliases: Aliases::build(
                        &function.params,
                        body,
                        types,
                        interner,
                        ParamRoots::PerParam,
                        None,
                    ),
                    owned: owned.defs,
                    inert: false,
                },
            );
        }
        let calls = mark_inert(&mut helpers, types, interner);
        settle_inert(&mut helpers, &calls);
        Self { helpers }
    }

    /// A call to `def` leaves the alias facts of its caller as they were.
    pub(super) fn is_inert(&self, def: DefId) -> bool {
        self.helpers.get(&def).is_some_and(|helper| helper.inert)
    }
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
            params: helper.function.params.iter().map(|p| p.def_id).collect(),
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
                if !is_access_intrinsic(receiver, *name, args.len(), self.types, self.interner) =>
            {
                self.disturbs = true;
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

fn is_inlinable(
    function: &HirFunction,
    annotations: &[HirAnnotation],
    interner: &Interner,
) -> bool {
    let public = annotations
        .iter()
        .any(|annotation| interner.resolve(annotation.family) == "publica");
    let device_helper = match function.nucleum_role() {
        Some(NucleumRole::Fragment) => true,
        Some(NucleumRole::Entry) => !public,
        None => false,
    };
    device_helper
        && !function.is_async
        && !function.posture.is_async()
        && !function.is_generator
        && !function.posture.is_stream()
        && function.err_ty.is_none()
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
            frames: Vec::new(),
            accesses: Vec::new(),
            errors: Vec::new(),
            suppress_reads: false,
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

    /// A call in a body under proof. A helper is analysed as inlined; any
    /// other callee is an unproved effect.
    pub(super) fn call(
        &mut self,
        expr: &HirExpression,
        callee: &HirExpression,
        args: &[HirCallArg],
    ) {
        let module = self.module;
        let helper = match &callee.kind {
            HirExpressionKind::Path(def) => module.helpers.get(def).map(|helper| (*def, helper)),
            _ => None,
        };
        let Some((def, helper)) = helper else {
            return self.decline_call(expr, "unproved_call_effect");
        };
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
        let frame = self.bind_arguments(def, helper, args);
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
        args: &[HirCallArg],
    ) -> Frame<'a> {
        let mut roots = FxHashMap::default();
        let mut coords = FxHashMap::default();
        let mut shared = None;
        for (param, arg) in helper.function.params.iter().zip(args) {
            if param_root(param, self.types, ParamRoots::PerParam, &mut shared).is_some() {
                roots.insert(param.def_id, self.facts_of(&arg.expr));
            }
            let coord = coordinate(&arg.expr).and_then(|coord| self.map_coords(vec![coord])?.pop());
            coords.insert(param.def_id, coord);
        }
        Frame {
            callee,
            aliases: &helper.aliases,
            roots,
            coords,
        }
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

/// `scale * parameter + offset` with the parameter replaced by its argument.
fn substitute(frame: &Frame<'_>, coord: Coordinate) -> Option<Coordinate> {
    let Some(parameter) = coord.variable else {
        return Some(coord);
    };
    let argument = frame.coords.get(&parameter)?.as_ref()?;
    Some(Coordinate {
        variable: argument.variable,
        scale: coord.scale.checked_mul(argument.scale)?,
        offset: coord
            .scale
            .checked_mul(argument.offset)?
            .checked_add(coord.offset)?,
    })
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
                if !is_access_intrinsic(receiver, *name, args.len(), self.types, self.interner) =>
            {
                self.opaque = true;
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}
