//! Live `tuus<T>.cursor()` provenance and escape analysis helpers.
//!
//! Phase 1 tracks locals initialized directly from `tuus.cursor()` so Rust
//! codegen can lower them to `faber::frame::TuusCursor<T>` instead of `Vec<_>`.
//! Phase 2 uses the same map to reject unsupported escapes before codegen.

use crate::{Type, TypeId, TypeTable};
use handroll::FxHashMap;
use radix_hir::{DefId, HirExpression, HirExpressionKind, HirLocal, HirModule};
use radix_lexer::Symbol;

/// Payload `T` from `tuus<T>.cursor()` when `expr` is that zero-arg method call.
pub fn tuus_cursor_method_payload<'a>(
    expr: &HirExpression,
    tuus_def_id: Option<DefId>,
    resolve_symbol: impl Fn(Symbol) -> &'a str,
    types: &TypeTable,
) -> Option<TypeId> {
    let HirExpressionKind::MethodCall(receiver, method, type_args, args, _) = &expr.kind else {
        return None;
    };
    if !type_args.is_empty() || !args.is_empty() {
        return None;
    }
    if resolve_symbol(*method) != "cursor" {
        return None;
    }
    let receiver_ty = receiver.ty?;
    let Type::Applied(base, type_args) = types.get(receiver_ty) else {
        return None;
    };
    if type_args.len() != 1 {
        return None;
    }
    let Type::Struct(def_id) = types.get(*base) else {
        return None;
    };
    if tuus_def_id != Some(*def_id) {
        return None;
    }
    match type_args[0] {
        radix_types::AppliedArg::Type(id) => Some(id),
        radix_types::AppliedArg::Size(_) => None,
    }
}

/// `DefId` of locals whose initializer is a direct `tuus<T>.cursor()` call.
pub fn collect_live_tuus_cursor_locals<'a>(
    hir: &HirModule,
    tuus_def_id: Option<DefId>,
    resolve_symbol: &impl Fn(Symbol) -> &'a str,
    types: &TypeTable,
) -> FxHashMap<DefId, TypeId> {
    struct Collector<'b, 'a> {
        tuus_def_id: Option<DefId>,
        resolve_symbol: &'b (dyn Fn(Symbol) -> &'a str + 'b),
        types: &'b TypeTable,
        locals: FxHashMap<DefId, TypeId>,
    }

    impl radix_hir::visit::HirVisitor for Collector<'_, '_> {
        fn visit_local(&mut self, local: &HirLocal) {
            if let Some(init) = &local.init
                && let Some(payload) = tuus_cursor_method_payload(
                    init,
                    self.tuus_def_id,
                    self.resolve_symbol,
                    self.types,
                )
            {
                self.locals.insert(local.def_id, payload);
            }
            radix_hir::visit::walk_local(self, local);
        }
    }

    let mut collector = Collector {
        tuus_def_id,
        resolve_symbol,
        types,
        locals: FxHashMap::default(),
    };
    radix_hir::visit::HirVisitor::visit_program(&mut collector, hir);
    collector.locals
}

/// True when `expr` references a live tuus cursor local or is a direct `tuus.cursor()` call.
pub fn expr_is_live_tuus_cursor_value<'a>(
    expr: &HirExpression,
    live_locals: &FxHashMap<DefId, TypeId>,
    tuus_def_id: Option<DefId>,
    resolve_symbol: impl Fn(Symbol) -> &'a str,
    types: &TypeTable,
) -> bool {
    if tuus_cursor_method_payload(expr, tuus_def_id, &resolve_symbol, types).is_some() {
        return true;
    }
    let HirExpressionKind::Path(def_id) = &expr.kind else {
        return false;
    };
    live_locals.contains_key(def_id)
}
