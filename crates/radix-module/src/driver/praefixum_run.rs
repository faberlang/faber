//! Run one `praefixum` body on the MIR runner (DECISIONS D8.5).

use super::evaluation::{PRAEFIXUM_STEP_BUDGET, invalid, with_initializer_mut};
use crate::driver::AnalyzedModule;
use crate::mir::{MirFunctionId, MirLoweringOptions, ValidatedMir, lower_analyzed_unit};
use crate::semantic::{self, SemanticError};
use handroll::FxHashSet;
use radix_hir::visit::{HirVisitor, walk_local};
use radix_hir::{DefId, HirExpressionKind, HirFunction, HirItem, HirItemKind, HirLocal};
use radix_lexer::Span;
use radix_mir_runner::{BufferHost, EvaluationFailure, RunnerError, Value, evaluate_function};
use radix_syntax::CallablePosture;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Definition id of the synthetic evaluation function. It exists only while
/// one body is lowered and run, and sits between the HIR-synthetic band and
/// the closure-lift band, so it never meets a real definition.
pub(super) const EVALUATION_DEF_ID: DefId = DefId(0x3FFF_FF00);

/// Evaluate the `praefixum` body of the static `def_id`.
pub(super) fn run_site(unit: &mut AnalyzedModule, def_id: DefId, span: Span) -> Result<Value, SemanticError> {
    let items = evaluation_items(unit, def_id, span)?;
    let saved_items = std::mem::replace(&mut unit.hir.items, items);
    let saved_entry = unit.hir.entry.take();
    let result = lower_and_run(unit, span);
    unit.hir.items = saved_items;
    unit.hir.entry = saved_entry;
    result
}

/// The items a module holds while the body of `def_id` is evaluated: the
/// synthetic function (`EVALUATION_DEF_ID`), the functions it reaches, and
/// every declaration. The caller installs them in place of `unit.hir.items`
/// and drops the entry; `unit` itself is not changed except for interning the
/// synthetic function's name.
pub(super) fn evaluation_items(
    unit: &mut AnalyzedModule,
    def_id: DefId,
    span: Span,
) -> Result<Vec<HirItem>, SemanticError> {
    let (function, id) = evaluation_function(unit, def_id, span)?;
    let reached = semantic::passes::praefixum::reached_functions(&unit.hir, &unit.types, def_id);
    // Lower only what the body needs: the synthetic function, the functions
    // it reaches, and every declaration. Statics still awaiting evaluation
    // are unread by this body (dependency order) and would not lower.
    Ok(unit
        .hir
        .items
        .iter()
        .filter(|item| match &item.kind {
            HirItemKind::Function(_) => reached.contains(&item.def_id),
            HirItemKind::Constant(konst) => !matches!(konst.value.kind, HirExpressionKind::Praefixum(_)),
            _ => true,
        })
        .map(|item| runnable_item(item, &reached))
        .chain(std::iter::once(HirItem {
            id,
            def_id: EVALUATION_DEF_ID,
            annotations: Vec::new(),
            kind: HirItemKind::Function(Box::new(function)),
            span,
        }))
        .collect())
}

/// An item with the methods the evaluation cannot lower removed: a method
/// that still holds an unevaluated local `praefixum` and is not reached by the
/// body (a reached method's sites were evaluated first, in dependency order).
fn runnable_item(item: &HirItem, reached: &FxHashSet<DefId>) -> HirItem {
    let mut item = item.clone();
    if let HirItemKind::Struct(strukt) = &mut item.kind {
        strukt
            .methods
            .retain(|method| reached.contains(&method.def_id) || !holds_praefixum(&method.func));
    }
    item
}

/// Whether a function body still contains a `praefixum` constant.
fn holds_praefixum(function: &HirFunction) -> bool {
    struct Finder(bool);
    impl HirVisitor for Finder {
        fn visit_local(&mut self, local: &HirLocal) {
            self.0 |= local
                .init
                .as_ref()
                .is_some_and(|init| matches!(init.kind, HirExpressionKind::Praefixum(_)));
            walk_local(self, local);
        }
    }
    let mut finder = Finder(false);
    finder.visit_function(function);
    finder.0
}

/// The body as a zero-argument synchronous function returning the declared
/// type, with its HIR id.
fn evaluation_function(
    unit: &mut AnalyzedModule,
    def_id: DefId,
    span: Span,
) -> Result<(HirFunction, radix_hir::HirId), SemanticError> {
    let (body, ret_ty) = with_initializer_mut(unit, def_id, |init| match &init.kind {
        HirExpressionKind::Praefixum(inner) => match &inner.kind {
            HirExpressionKind::Block(block) => Some((block.clone(), init.ty)),
            _ => None,
        },
        _ => None,
    })
    .flatten()
    .ok_or_else(|| invalid("praefixum_unsupported", span))?;
    let symbol = unit.interner.intern("praefixum_evaluatio");
    let id = radix_hir::HirId(semantic::passes::statica::max_hir_id(&unit.hir) + 1);
    Ok((
        HirFunction {
            name: symbol,
            type_params: Vec::new(),
            params: Vec::new(),
            cli_args: None,
            ret_ty,
            err_ty: None,
            body: Some(body),
            posture: CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            test: None,
        },
        id,
    ))
}

fn lower_and_run(unit: &AnalyzedModule, span: Span) -> Result<Value, SemanticError> {
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).map_err(|errors| {
        let detail = errors
            .first()
            .map_or_else(|| "MIR lowering failed".to_owned(), |error| error.message.clone());
        invalid("praefixum_unsupported", span).with_arg("detail", detail)
    })?;
    let Some(function_id) = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| function.source == Some(EVALUATION_DEF_ID))
        .map(|function| function.id)
    else {
        return Err(invalid("praefixum_unsupported", span).with_arg("detail", "no lowered body"));
    };

    run_linked_function(&lowered.validated, function_id, span)
}

/// Run the lowered function `function_id` of `validated` under the step
/// budget and map every way it can fail to a `praefixum` issue. A `mori`
/// panic is trapped quietly. The shared tail of the local path and of the
/// package layer's linked evaluation.
///
/// # Errors
///
/// `praefixum_failed`, `praefixum_step_limit`, `praefixum_no_result`, or
/// `praefixum_unsupported`, each on `span`.
pub fn run_linked_function(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    span: Span,
) -> Result<Value, SemanticError> {
    // `mori` reaches `BufferHost::abort`, which panics; trap it quietly.
    let mut host = BufferHost::default();
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let trapped = catch_unwind(AssertUnwindSafe(|| {
        evaluate_function(validated, function_id, &mut host, PRAEFIXUM_STEP_BUDGET)
    }));
    std::panic::set_hook(previous_hook);

    match trapped {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(EvaluationFailure::Errors(errors))) => Err(runner_failure(&errors, span)),
        Ok(Err(EvaluationFailure::Raised(value))) => {
            let context = validated.validation();
            let message = context
                .interner
                .as_interner()
                .and_then(|interner| {
                    value
                        .display(
                            interner,
                            radix_mir_runner::FieldOrder::new(
                                &context.struct_field_order,
                                &context.variant_field_order,
                            )
                            .with_names(&context.display_names),
                            span,
                        )
                        .ok()
                })
                .unwrap_or_else(|| "error".to_owned());
            Err(invalid("praefixum_failed", span).with_arg("message", message))
        }
        Ok(Err(EvaluationFailure::NoValue)) => Err(invalid("praefixum_no_result", span)),
        Ok(Err(EvaluationFailure::StepLimit)) => {
            Err(invalid("praefixum_step_limit", span).with_arg("limit", PRAEFIXUM_STEP_BUDGET.to_string()))
        }
        Err(_) => {
            let message = host.last_abort.unwrap_or_default();
            Err(invalid("praefixum_failed", span).with_arg("message", message))
        }
    }
}

fn runner_failure(errors: &[RunnerError], span: Span) -> SemanticError {
    let Some(error) = errors.first() else {
        return invalid("praefixum_unsupported", span);
    };
    invalid("praefixum_unsupported", span).with_arg("detail", error.message.clone())
}
