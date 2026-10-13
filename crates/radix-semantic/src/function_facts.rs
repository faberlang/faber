//! Upstream function effect, capture, and call-graph facts for HIR v1.
//!
//! These facts are computed once during analyzed-application construction.
//! Application backends consume them instead of re-deriving failable closure,
//! async/generator posture, or capture sets from HIR walks at emit time.

use crate::{Type, TypeId, TypeTable};
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{
    DefId, HirExpression, HirExpressionKind, HirFunction, HirItemKind, HirModule, HirParam,
    HirStatementKind,
};
use radix_lexer::Symbol;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

/// Capture mode demanded by a free variable use inside a closure body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CaptureMode {
    /// Read-only / shared use (`de` posture).
    Read,
    /// Mutating use observed in the body (`in` posture).
    Mutate,
    /// Consume/move demand (`ex` posture).
    Consume,
}

/// One captured local or parameter referenced from a closure body.
///
/// F9: included in wire shape because codegen backends read captured-variable
/// facts for closure layout (closure struct fields, move detection, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureFact {
    pub def_id: DefId,
    pub mode: CaptureMode,
}

/// Effect and call facts for one function, method, or entry body.
///
/// F9: included in wire shape because codegen backends read per-function
/// effect facts to decide async/await handling, error propagation, and
/// call-graph reachability without re-walking HIR.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionFact {
    /// Direct unsuppressed `iace` in the body.
    pub direct_failure: bool,
    /// Declared alternate-exit channel (`⇥ E`) or transitive failure.
    pub may_fail: bool,
    /// Declared `async` / analyzed await requirement.
    pub is_async: bool,
    /// Declared generator / yield requirement.
    pub is_generator: bool,
    /// Body requires async suspension through an await operation.
    pub requires_await: bool,
    /// Direct callees by definition id (functions and resolved methods).
    pub call_edges: FxHashSet<DefId>,
    /// Captures for closure-shaped bodies (empty for ordinary functions).
    pub captures: Vec<CaptureFact>,
    /// Declared parameter modes keyed by parameter def id.
    pub param_modes: FxHashMap<DefId, crate::SemanticParamMode>,
    /// Declared error type when present.
    pub err_ty: Option<TypeId>,
}

/// Complete function-fact table for one analyzed unit.
///
/// F9: included in wire shape because backends require the full function-fact
/// table at codegen time. Deserializing it from snapshot avoids re-running
/// the `analyze_function_facts` analysis pass.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionFactTable {
    pub by_def: FxHashMap<DefId, FunctionFact>,
    /// Synthetic entry facts when the unit has an `incipit` body.
    pub entry: Option<FunctionFact>,
}

impl FunctionFactTable {
    /// Construct a fact table from already-analyzed data (artifact loader).
    #[must_use]
    pub fn from_raw(by_def: FxHashMap<DefId, FunctionFact>, entry: Option<FunctionFact>) -> Self {
        Self { by_def, entry }
    }

    #[must_use]
    pub fn get(&self, def_id: DefId) -> Option<&FunctionFact> {
        self.by_def.get(&def_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (DefId, &FunctionFact)> {
        self.by_def.iter().map(|(id, fact)| (*id, fact))
    }

    #[must_use]
    pub fn failable_defs(&self) -> FxHashSet<DefId> {
        self.by_def
            .iter()
            .filter(|(_, fact)| fact.may_fail)
            .map(|(id, _)| *id)
            .collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_def.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_def.is_empty() && self.entry.is_none()
    }

    #[must_use]
    pub fn async_defs(&self) -> FxHashSet<DefId> {
        self.by_def
            .iter()
            .filter_map(|(def_id, fact)| fact.is_async.then_some(*def_id))
            .collect()
    }

    /// Fail-closed: every analyzed function must have a fact row.
    #[must_use]
    pub fn has_fact(&self, def_id: DefId) -> bool {
        self.by_def.contains_key(&def_id)
    }
}

/// Analyze function/method/entry facts from typed HIR.
#[must_use]
pub fn analyze_function_facts(hir: &HirModule, types: &TypeTable) -> FunctionFactTable {
    let method_index = collect_struct_method_defs(hir);
    let mut raw: FxHashMap<DefId, RawFnDeps> = FxHashMap::default();
    let mut declared_async = FxHashSet::default();
    let mut declared_generator = FxHashSet::default();
    let mut declared_err: FxHashMap<DefId, TypeId> = FxHashMap::default();
    let mut param_modes: FxHashMap<DefId, FxHashMap<DefId, crate::SemanticParamMode>> =
        FxHashMap::default();

    for_each_hir_function(hir, |def_id, func| {
        if func.is_async {
            declared_async.insert(def_id);
        }
        if func.is_generator {
            declared_generator.insert(def_id);
        }
        if let Some(err) = func.err_ty {
            declared_err.insert(def_id, err);
        }
        param_modes.insert(def_id, collect_param_modes(&func.params));

        let mut visitor = BodyVisitor {
            types,
            method_index: &method_index,
            deps: RawFnDeps::default(),
            suppressed: false,
            bound_locals: func.params.iter().map(|p| p.def_id).collect(),
        };
        if let Some(body) = &func.body {
            visitor.visit_block(body);
        }
        raw.insert(def_id, visitor.deps);
    });

    let failable = propagate_failable(&raw, &declared_err);

    let mut table = FunctionFactTable::default();
    for (def_id, deps) in raw {
        let may_fail = failable.contains(&def_id) || declared_err.contains_key(&def_id);
        // `cede` is a yield inside cursor functions, not an await requirement.
        let requires_await = deps.requires_await && !declared_generator.contains(&def_id);
        table.by_def.insert(
            def_id,
            FunctionFact {
                direct_failure: deps.direct_throw,
                may_fail,
                is_async: declared_async.contains(&def_id) || requires_await,
                is_generator: declared_generator.contains(&def_id),
                requires_await,
                call_edges: deps.calls,
                captures: Vec::new(),
                param_modes: param_modes.remove(&def_id).unwrap_or_default(),
                err_ty: declared_err.get(&def_id).copied(),
            },
        );
    }

    // The entry body gets its own row (failure, await and call edges only; its
    // `captures` stay empty). Closure captures are collected per function in
    // the second pass below.
    if let Some(entry) = &hir.entry {
        let mut visitor = BodyVisitor {
            types,
            method_index: &method_index,
            deps: RawFnDeps::default(),
            suppressed: false,
            bound_locals: FxHashSet::default(),
        };
        visitor.visit_block(entry);
        let entry_may_fail = visitor.deps.direct_throw
            || visitor
                .deps
                .calls
                .iter()
                .any(|c| failable.contains(c) || declared_err.contains_key(c));
        table.entry = Some(FunctionFact {
            direct_failure: visitor.deps.direct_throw,
            may_fail: entry_may_fail,
            is_async: hir.entry_is_async || visitor.deps.requires_await,
            is_generator: false,
            requires_await: visitor.deps.requires_await,
            call_edges: visitor.deps.calls,
            captures: Vec::new(),
            param_modes: FxHashMap::default(),
            err_ty: None,
        });
    }

    // Second pass: closure captures inside function bodies.
    for_each_hir_function(hir, |def_id, func| {
        let Some(body) = &func.body else {
            return;
        };
        let mut collector = ClosureCaptureCollector {
            bound: func.params.iter().map(|p| p.def_id).collect(),
            captures: Vec::new(),
        };
        collector.visit_block(body);
        if let Some(fact) = table.by_def.get_mut(&def_id) {
            fact.captures = collector.captures;
        }
    });

    table
}

#[derive(Default)]
pub(crate) struct RawFnDeps {
    pub direct_throw: bool,
    /// Direct `ad` in body — codegen-failable only; does not propagate via calls.
    pub direct_ad: bool,
    pub calls: FxHashSet<DefId>,
    pub requires_await: bool,
}

#[must_use]
pub(crate) fn propagate_failable(
    raw: &FxHashMap<DefId, RawFnDeps>,
    declared_err: &FxHashMap<DefId, TypeId>,
) -> FxHashSet<DefId> {
    // Reverse edges: callee → callers. Worklist visits each edge once (O(N+E))
    // instead of rescanning the full map each fixed-point iteration (O(D×N)).
    let mut callers: FxHashMap<DefId, Vec<DefId>> = FxHashMap::default();
    for (def_id, deps) in raw {
        for &callee in &deps.calls {
            callers.entry(callee).or_default().push(*def_id);
        }
    }

    let mut failable = FxHashSet::default();
    let mut worklist: Vec<DefId> = Vec::new();
    for (def_id, deps) in raw {
        // `iace` / declared ⇥ seed the worklist; propagation walks callers only.
        if (deps.direct_throw || declared_err.contains_key(def_id)) && failable.insert(*def_id) {
            worklist.push(*def_id);
        }
    }
    while let Some(callee) = worklist.pop() {
        let Some(cs) = callers.get(&callee) else {
            continue;
        };
        for &caller in cs {
            if failable.insert(caller) {
                worklist.push(caller);
            }
        }
    }
    // Host `ad` materialize is fallible at the defining function only so
    // fac/cape can catch FrameError without forcing every caller to declare ⇥.
    for (def_id, deps) in raw {
        if deps.direct_ad {
            failable.insert(*def_id);
        }
    }
    failable
}

fn collect_param_modes(params: &[HirParam]) -> FxHashMap<DefId, crate::SemanticParamMode> {
    params
        .iter()
        .map(|param| {
            let mode = match param.mode {
                radix_hir::HirParamMode::Owned | radix_hir::HirParamMode::Axis => {
                    crate::SemanticParamMode::Owned
                }
                radix_hir::HirParamMode::De => crate::SemanticParamMode::Ref,
                radix_hir::HirParamMode::In => crate::SemanticParamMode::MutRef,
                radix_hir::HirParamMode::Ex => crate::SemanticParamMode::Move,
            };
            (param.def_id, mode)
        })
        .collect()
}

type MethodDefIndex = FxHashMap<(DefId, Symbol), DefId>;

fn collect_struct_method_defs(hir: &HirModule) -> MethodDefIndex {
    let mut index = FxHashMap::default();
    for item in &hir.items {
        let HirItemKind::Struct(strukt) = &item.kind else {
            continue;
        };
        for method in &strukt.methods {
            index.insert((item.def_id, method.func.name), method.def_id);
        }
    }
    index
}

fn struct_def_from_type(ty: TypeId, types: &TypeTable) -> Option<DefId> {
    match types.get(ty) {
        Type::Struct(def) => Some(*def),
        Type::Ref(_, inner) | Type::Applied(inner, _) | Type::Alias(_, inner) => {
            struct_def_from_type(*inner, types)
        }
        _ => None,
    }
}

fn resolve_method_call_def(
    method_index: &MethodDefIndex,
    receiver: &HirExpression,
    method: Symbol,
    types: &TypeTable,
) -> Option<DefId> {
    let receiver_ty = receiver.ty?;
    let struct_def = struct_def_from_type(receiver_ty, types)?;
    method_index.get(&(struct_def, method)).copied()
}

struct BodyVisitor<'a> {
    types: &'a TypeTable,
    method_index: &'a MethodDefIndex,
    deps: RawFnDeps,
    suppressed: bool,
    bound_locals: FxHashSet<DefId>,
}

impl BodyVisitor<'_> {
    fn with_suppressed(&mut self, suppressed: bool, f: impl FnOnce(&mut Self)) {
        let previous = self.suppressed;
        self.suppressed = suppressed;
        f(self);
        self.suppressed = previous;
    }
}

impl HirVisitor for BodyVisitor<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Throw(_) => {
                if !self.suppressed {
                    self.deps.direct_throw = true;
                }
                walk_expr(self, expr);
            }
            HirExpressionKind::Ad { .. } => {
                if !self.suppressed {
                    self.deps.direct_ad = true;
                }
                walk_expr(self, expr);
            }
            HirExpressionKind::Call(callee, _, _, _) => {
                if !self.suppressed
                    && let HirExpressionKind::Path(def_id) = &callee.kind
                {
                    self.deps.calls.insert(*def_id);
                }
                walk_expr(self, expr);
            }
            HirExpressionKind::MethodCall(receiver, method, _, _, _) => {
                if !self.suppressed
                    && let Some(method_def) =
                        resolve_method_call_def(self.method_index, receiver, *method, self.types)
                {
                    self.deps.calls.insert(method_def);
                }
                walk_expr(self, expr);
            }
            HirExpressionKind::Cede(_) => {
                // Legacy generic carrier; new source lowers to Yield or a
                // distinct await operation before function facts run.
                walk_expr(self, expr);
            }
            HirExpressionKind::Reddet(_) | HirExpressionKind::Tacebit(_) => {
                // Distinct await operations — always require async context.
                self.deps.requires_await = true;
                walk_expr(self, expr);
            }
            HirExpressionKind::Yield(_) => {
                // Yield-only — does not require async context.
                walk_expr(self, expr);
            }
            HirExpressionKind::Handled { body, catch } => {
                self.with_suppressed(true, |visitor| visitor.visit_block(body));
                self.visit_block(&catch.body);
            }
            // UT: a `capta` trap captures every alternate exit inside its
            // body into the trap value — the enclosing function stays
            // non-failable.
            HirExpressionKind::Trap(body) => {
                self.with_suppressed(true, |visitor| visitor.visit_block(body));
            }
            // D17.7: `call ⊥ default` contains the call's failure; the
            // default is evaluated outside the boundary.
            HirExpressionKind::CallDefault { body, default, .. } => {
                self.with_suppressed(true, |visitor| visitor.visit_block(body));
                self.visit_expr(default);
            }
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => {
                self.visit_expr(cond);
                if let Some(catch) = then_catch {
                    // Same containment as `fac`/`cape`: a then-arm throw is
                    // the catch's local channel, not the enclosing function.
                    self.with_suppressed(true, |visitor| visitor.visit_block(then_block));
                    self.visit_block(&catch.body);
                } else {
                    self.visit_block(then_block);
                }
                if let Some(else_block) = else_block {
                    self.visit_block(else_block);
                }
            }
            HirExpressionKind::Clausura(params, _, _, body) => {
                let mut nested_bound = self.bound_locals.clone();
                for param in params {
                    nested_bound.insert(param.def_id);
                }
                let previous = std::mem::replace(&mut self.bound_locals, nested_bound);
                self.visit_expr(body);
                self.bound_locals = previous;
            }
            _ => walk_expr(self, expr),
        }
    }

    fn visit_stmt(&mut self, stmt: &radix_hir::HirStatement) {
        if let HirStatementKind::Local(local) = &stmt.kind {
            self.bound_locals.insert(local.def_id);
        }
        radix_hir::visit::walk_stmt(self, stmt);
    }
}

struct ClosureCaptureCollector {
    bound: FxHashSet<DefId>,
    captures: Vec<CaptureFact>,
}

impl HirVisitor for ClosureCaptureCollector {
    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                if !self.bound.contains(def_id) && self.captures.iter().all(|c| c.def_id != *def_id)
                {
                    self.captures.push(CaptureFact {
                        def_id: *def_id,
                        mode: CaptureMode::Read,
                    });
                }
                walk_expr(self, expr);
            }
            HirExpressionKind::Clausura(params, _, _, body) => {
                let mut nested = self.bound.clone();
                for param in params {
                    nested.insert(param.def_id);
                }
                let previous = std::mem::replace(&mut self.bound, nested);
                self.visit_expr(body);
                self.bound = previous;
            }
            _ => walk_expr(self, expr),
        }
    }

    fn visit_stmt(&mut self, stmt: &radix_hir::HirStatement) {
        if let HirStatementKind::Local(local) = &stmt.kind {
            self.bound.insert(local.def_id);
        }
        radix_hir::visit::walk_stmt(self, stmt);
    }
}

fn for_each_hir_function<'a, F>(hir: &'a HirModule, mut visit: F)
where
    F: FnMut(DefId, &'a HirFunction),
{
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(func) => visit(item.def_id, func),
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    visit(method.def_id, &method.func);
                }
            }
            _ => {}
        }
    }
}
