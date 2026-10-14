//! The single home of the failable-closure fixed point over analyzed HIR.
//!
//! Backends consume [`derive_failable_defs`] instead of re-deriving the
//! transitive failable set from HIR walks at emit time (S08-2). This is the
//! standalone derivation for leaves that must stay `radix-semantic`-free;
//! `radix-semantic::function_facts` publishes the richer analyzed-application
//! table over the same law.
//!
//! The law:
//! - Direct unsuppressed `iace` marks the enclosing function failable.
//! - A declared `err_ty` (`⇥ E`) marks the function failable even without
//!   throw evidence.
//! - Direct `ad` marks the defining function failable for codegen (host frame
//!   errors are catchable) but does not propagate through the call graph.
//! - Calls — including receiver-resolved method calls (F-034) — to failable
//!   functions make callers failable, repeated to a fixed point.
//! - `Handled` bodies, `capta` trap bodies, and the then-arm of an
//!   arm-attached `cape` are contained: their throws do not escape.
//! - A closure body is not a boundary: a throw or failable call inside a
//!   closure marks the enclosing function failable, as in `function_facts`;
//!   only `Handled`, `capta` and `cape` containment above suppress it.

use crate::visit::{HirVisitor, walk_expr};
use crate::{DefId, HirExpression, HirExpressionKind, HirFunction, HirItemKind, HirModule};
use radix_lexer::Symbol;
use radix_types::{Type, TypeId, TypeTable};
use rustc_hash::{FxHashMap, FxHashSet};

/// Per-function dependency evidence collected in one HIR walk.
#[derive(Default)]
struct FnDeps {
    direct_throw: bool,
    direct_ad: bool,
    calls: FxHashSet<DefId>,
}

/// Derive the transitive set of failable definitions for one HIR module.
///
/// `types` enables receiver-typed method-call resolution; pass `None` only
/// before a type table exists (the set then carries no method-call edges).
#[must_use]
pub fn derive_failable_defs(hir: &HirModule, types: Option<&TypeTable>) -> FxHashSet<DefId> {
    let method_index = collect_struct_method_defs(hir);
    let (fn_deps, declared_err) = collect_function_dependencies(hir, &method_index, types);
    propagate_failable(&fn_deps, &declared_err)
}

fn collect_function_dependencies(
    hir: &HirModule,
    method_index: &MethodDefIndex,
    types: Option<&TypeTable>,
) -> (FxHashMap<DefId, FnDeps>, FxHashSet<DefId>) {
    let mut fn_deps = FxHashMap::default();
    let mut declared_err = FxHashSet::default();

    for_each_hir_function(hir, |def_id, func| {
        if func.err_ty.is_some() {
            declared_err.insert(def_id);
        }

        let mut visitor = DependencyVisitor {
            deps: FnDeps::default(),
            suppressed: false,
            method_index,
            types,
        };
        if let Some(body) = &func.body {
            visitor.visit_block(body);
        }
        fn_deps.insert(def_id, visitor.deps);
    });

    (fn_deps, declared_err)
}

fn propagate_failable(
    fn_deps: &FxHashMap<DefId, FnDeps>,
    declared_err: &FxHashSet<DefId>,
) -> FxHashSet<DefId> {
    // Reverse edges: callee → callers. Worklist visits each edge once (O(N+E))
    // instead of rescanning the full map each fixed-point iteration (O(D×N)).
    let mut callers: FxHashMap<DefId, Vec<DefId>> = FxHashMap::default();
    for (def_id, deps) in fn_deps {
        for &callee in &deps.calls {
            callers.entry(callee).or_default().push(*def_id);
        }
    }

    let mut failable = FxHashSet::default();
    let mut worklist: Vec<DefId> = Vec::new();
    for (def_id, deps) in fn_deps {
        if (deps.direct_throw || declared_err.contains(def_id)) && failable.insert(*def_id) {
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
    for (def_id, deps) in fn_deps {
        if deps.direct_ad {
            failable.insert(*def_id);
        }
    }
    failable
}

/// Maps `(struct_def_id, method_symbol)` to the method's [`DefId`].
type MethodDefIndex = FxHashMap<(DefId, Symbol), DefId>;

fn collect_struct_method_defs(hir: &HirModule) -> MethodDefIndex {
    let mut index = MethodDefIndex::default();
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

/// Resolve a method-call's receiver type + method name to a [`DefId`].
fn resolve_method_call_def(
    method_index: &MethodDefIndex,
    receiver: &HirExpression,
    method: Symbol,
    types: Option<&TypeTable>,
) -> Option<DefId> {
    let receiver_ty = receiver.ty?;
    let struct_def = struct_def_from_type(receiver_ty, types)?;
    method_index.get(&(struct_def, method)).copied()
}

/// Walk through type wrappers to find the underlying struct definition.
fn struct_def_from_type(ty: TypeId, types: Option<&TypeTable>) -> Option<DefId> {
    match types?.get(ty) {
        Type::Struct(def) => Some(*def),
        Type::Ref(_, inner) | Type::Applied(inner, _) | Type::Alias(_, inner) => {
            struct_def_from_type(*inner, types)
        }
        _ => None,
    }
}

struct DependencyVisitor<'a> {
    deps: FnDeps,
    suppressed: bool,
    method_index: &'a MethodDefIndex,
    types: Option<&'a TypeTable>,
}

impl DependencyVisitor<'_> {
    fn with_suppressed(&mut self, suppressed: bool, f: impl FnOnce(&mut Self)) {
        let previous = self.suppressed;
        self.suppressed = suppressed;
        f(self);
        self.suppressed = previous;
    }

    /// Record a call edge unless the call sits inside a containing boundary.
    fn note_call(&mut self, callee: DefId) {
        if !self.suppressed {
            self.deps.calls.insert(callee);
        }
    }
}

impl HirVisitor for DependencyVisitor<'_> {
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
                if let HirExpressionKind::Path(def_id) = &callee.kind {
                    self.note_call(*def_id);
                }
                walk_expr(self, expr);
            }
            // F-034: track method calls to failable methods so callers are
            // also marked failable.
            HirExpressionKind::MethodCall(receiver, method, _, _, _) => {
                if let Some(method_def) =
                    resolve_method_call_def(self.method_index, receiver, *method, self.types)
                {
                    self.note_call(method_def);
                }
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
            // default itself is evaluated outside the boundary.
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
                    // The attached `cape` owns throw sites in its then arm;
                    // only the catch and the else arm remain enclosing code.
                    self.with_suppressed(true, |visitor| visitor.visit_block(then_block));
                    self.visit_block(&catch.body);
                } else {
                    self.visit_block(then_block);
                }
                if let Some(else_block) = else_block {
                    self.visit_block(else_block);
                }
            }
            _ => walk_expr(self, expr),
        }
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

#[cfg(test)]
#[path = "failable_test.rs"]
mod tests;
