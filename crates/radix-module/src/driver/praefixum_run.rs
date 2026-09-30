//! Run one `praefixum` body on the MIR runner (DECISIONS D8.5).

use super::evaluation::{PRAEFIXUM_STEP_BUDGET, initializer_mut, invalid};
use crate::driver::AnalyzedModule;
use crate::mir::{MirLoweringOptions, lower_analyzed_unit};
use crate::semantic::{self, SemanticError};
use radix_hir::{DefId, HirExpressionKind, HirFunction, HirItem, HirItemKind};
use radix_lexer::Span;
use radix_mir_runner::{BufferHost, EvaluationFailure, RunnerError, Value, evaluate_function};
use radix_syntax::CallablePosture;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Definition id of the synthetic evaluation function. It exists only while
/// one body is lowered and run, and sits between the HIR-synthetic band and
/// the closure-lift band, so it never meets a real definition.
const EVALUATION_DEF_ID: DefId = DefId(0x3FFF_FF00);

/// Evaluate the `praefixum` body of the static `def_id`.
pub(super) fn run_site(unit: &mut AnalyzedModule, def_id: DefId, span: Span) -> Result<Value, SemanticError> {
    let function = evaluation_function(unit, def_id, span)?;
    let reached = semantic::passes::praefixum::reached_functions(&unit.hir, &unit.types, def_id);
    // Lower only what the body needs: the synthetic function, the functions
    // it reaches, and every declaration. Statics still awaiting evaluation
    // are unread by this body (dependency order) and would not lower.
    let items: Vec<HirItem> = unit
        .hir
        .items
        .iter()
        .filter(|item| match &item.kind {
            HirItemKind::Function(_) => reached.contains(&item.def_id),
            HirItemKind::Constant(konst) => !matches!(konst.value.kind, HirExpressionKind::Praefixum(_)),
            _ => true,
        })
        .cloned()
        .chain(std::iter::once(HirItem {
            id: function.1,
            def_id: EVALUATION_DEF_ID,
            annotations: Vec::new(),
            kind: HirItemKind::Function(Box::new(function.0)),
            span,
        }))
        .collect();
    let saved_items = std::mem::replace(&mut unit.hir.items, items);
    let saved_entry = unit.hir.entry.take();
    let result = lower_and_run(unit, span);
    unit.hir.items = saved_items;
    unit.hir.entry = saved_entry;
    result
}

/// The body as a zero-argument synchronous function returning the declared
/// type, with its HIR id.
fn evaluation_function(
    unit: &mut AnalyzedModule,
    def_id: DefId,
    span: Span,
) -> Result<(HirFunction, radix_hir::HirId), SemanticError> {
    let (body, ret_ty) = initializer_mut(unit, def_id)
        .and_then(|init| match &init.kind {
            HirExpressionKind::Praefixum(inner) => match &inner.kind {
                HirExpressionKind::Block(block) => Some((block.clone(), init.ty)),
                _ => None,
            },
            _ => None,
        })
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
    let lowered = lower_analyzed_unit(unit, MirLoweringOptions::default()).map_err(|errors| {
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

    // `mori` reaches `BufferHost::abort`, which panics; trap it quietly.
    let mut host = BufferHost::default();
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let trapped = catch_unwind(AssertUnwindSafe(|| {
        evaluate_function(&lowered.validated, function_id, &mut host, PRAEFIXUM_STEP_BUDGET)
    }));
    std::panic::set_hook(previous_hook);

    match trapped {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(EvaluationFailure::Errors(errors))) => Err(runner_failure(&errors, span)),
        Ok(Err(EvaluationFailure::Raised(value))) => {
            let message = value
                .display(
                    &lowered.interner,
                    radix_mir_runner::FieldOrder::new(
                        &lowered.validated.validation().struct_field_order,
                        &lowered.validated.validation().variant_field_order,
                    )
                    .with_names(&lowered.validated.validation().display_names),
                    span,
                )
                .unwrap_or_else(|_| "error".to_owned());
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
