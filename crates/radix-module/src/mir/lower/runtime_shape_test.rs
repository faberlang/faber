//! Caller-owned size witnesses must survive nested-call lowering without
//! becoming callee argument ordinals. Malformed callee sources still fail closed.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, AnalyzedModule, Config, Session, analyze_source};
use crate::hir::visit::{HirVisitor, walk_expr};
use crate::hir::{DefId, HirExpression, HirExpressionKind, HirId, HirItemKind};
use crate::mir::dump_analyzed_unit;
use crate::semantic::{IndexExpr, RuntimeExtentId};

const SOURCE: &str = r#"+++
locale = "en"
+++
fn zero_block(int width) → list<f32> {
    return [0.0, 0.0]
}
fn zero_bias<size N>(list<f32> z) → tf32[N] ⇥ string {
    return z ↦ tf32[N]
}
fn bias_or_zero<size N>(bool has, tf32[N] real, int width) → tf32[N] ⇥ string {
    if has then return real
    return zero_bias(zero_block(width))
}
main {}
"#;

fn session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root(),
    )
}

fn analyze() -> AnalyzedModule {
    analyze_source(&session(), "caller_size_witness.fab", SOURCE, AnalyzeRequest::default())
        .expect("the nested generic call should analyze")
}

#[test]
fn caller_size_witness_keeps_its_binding_not_the_callee_argument_ordinal() {
    let unit = analyze();
    let caller = unit
        .hir
        .items
        .iter()
        .find(|item| {
            matches!(&item.kind, HirItemKind::Function(function)
            if unit.interner.resolve(function.name) == "bias_or_zero")
        })
        .expect("caller function");
    let HirItemKind::Function(function) = &caller.kind else {
        unreachable!("selected function");
    };
    let real = function.params[1].def_id;
    let function_binding = &unit.shape_obligations.function_size_bindings[&caller.def_id][0];
    assert_eq!(function_binding.parameter, 0, "generic descriptor space");
    assert!(
        matches!(
            unit.types.get_index(function_binding.witness),
            IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis { parameter: 1, axis: 0 })
        ),
        "entry witness remains in caller formal-parameter space"
    );

    struct CallSite {
        callee: DefId,
        site: Option<HirId>,
    }
    impl HirVisitor for CallSite {
        fn visit_expr(&mut self, expression: &HirExpression) {
            if let HirExpressionKind::Call(callee, _, arguments, _) = &expression.kind
                && matches!(callee.kind, HirExpressionKind::Path(def) if def == self.callee)
            {
                assert_eq!(arguments.len(), 1, "callee argument space contains only ordinal 0");
                self.site = Some(expression.id);
            }
            walk_expr(self, expression);
        }
    }
    let callee = unit
        .hir
        .items
        .iter()
        .find(|item| {
            matches!(&item.kind, HirItemKind::Function(function)
            if unit.interner.resolve(function.name) == "zero_bias")
        })
        .expect("callee function");
    let mut site = CallSite { callee: callee.def_id, site: None };
    site.visit_program(&unit.hir);
    let call = site.site.expect("zero_bias call");
    assert_eq!(unit.shape_obligations.call_size_bindings.len(), 1);
    let call_binding = unit.shape_obligations.call_size_bindings[&call][0];
    assert_eq!(call_binding.parameter, 0, "callee generic descriptor space");
    assert!(
        matches!(unit.types.get_index(call_binding.witness),
        IndexExpr::Runtime(RuntimeExtentId::TensorBindingAxis { binding, axis: 0 }) if *binding == real),
        "the witness node addresses the caller binding, not callee argument 1"
    );
    dump_analyzed_unit(unit.application_view())
        .expect("one-argument zero_bias call should lower with the caller witness");
}

#[test]
fn return_only_runtime_size_still_requires_a_formal_carrier_witness() {
    // This template has no N-bearing callee input. Fixing caller provenance
    // must not bypass the separate specialization admission for such a call.
    let source = SOURCE.replace(
        "main {}",
        r"main {
    let tf32[2] real ← [1.0, 2.0] ↦ tf32[2]
    do {
        print bias_or_zero(false, real, 2).length()
    } catch error {
        print error
    }
}",
    );
    let unit = analyze_source(&session(), "return_only_size.fab", &source, AnalyzeRequest::default())
        .expect("the return-only size template should analyze");
    let errors =
        dump_analyzed_unit(unit.application_view()).expect_err("runtime specialization needs a formal carrier witness");
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "invalid_mir_runtime_generic_shape_has_no_stable_formal_carrier_witness"),
        "missing-witness specialization must remain fail-closed: {errors:?}"
    );
}

#[test]
fn callee_argument_out_of_range_remains_rejected() {
    let mut unit = analyze();
    let invalid = unit
        .types
        .intern_index(IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis {
            parameter: 1,
            axis: 0,
        }));
    unit.shape_obligations
        .call_size_bindings
        .values_mut()
        .next()
        .expect("call witness")[0]
        .witness = invalid;
    let errors = dump_analyzed_unit(unit.application_view()).expect_err("zero_bias has only callee argument 0");
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "unsupported_mir_lowering_runtime_shape_call_argument_is_out_of_range"),
        "the call-argument admission guard must remain fail-closed: {errors:?}"
    );
}
