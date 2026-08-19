//! NF-9 behavioral corpus for `@ nucleum fragment`.
//!
//! Every fixture routes the fragment through a wrapper `@ nucleum` entry.
//! Fragments have no standalone runtime identity, so no fixture here emits a
//! fragment alone.

use super::{surviving_fragment_calls, ExpansionBudget, ISSUE_CYCLE, ISSUE_EXPANSION};
use crate::diagnostics::Diagnostic;
use crate::driver::{self, Config, Session};
use crate::lexer::Span;
use crate::mir::{
    device_roles_from_hir, MirBinOp, MirDeviceRole, MirFunction, MirOperand, MirPlaceBase, MirProgram,
    MirStatementKind, MirValue, MirValueKind,
};
use crate::semantic::SemanticErrorKind;

const ISSUE_CONTEXT: &str = "NUCLEUM_FRAGMENT_CONTEXT";
const ISSUE_TYPE: &str = "NUCLEUM_FRAGMENT_TYPE";
const ISSUE_EFFECT: &str = "NUCLEUM_FRAGMENT_EFFECT";

fn with_budget_override<R>(budget: ExpansionBudget, f: impl FnOnce() -> R) -> R {
    struct Reset(Option<ExpansionBudget>);
    impl Drop for Reset {
        fn drop(&mut self) {
            super::BUDGET_OVERRIDE.with(|slot| slot.set(self.0));
        }
    }
    let prev = super::BUDGET_OVERRIDE.with(|slot| slot.replace(Some(budget)));
    let _reset = Reset(prev);
    f()
}

fn assert_wrapper_route(source: &str) {
    assert!(
        source.contains("@ nucleum fragment"),
        "corpus fixture must declare a fragment:\n{source}"
    );
    let has_entry = source.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == "@ nucleum" || trimmed.starts_with("@ nucleum {")
    });
    assert!(
        has_entry,
        "corpus fixture must wrap the fragment in an @ nucleum entry:\n{source}"
    );
}

fn session() -> Session {
    Session::new(Config::default())
}

fn analyze(source: &str) -> Result<crate::driver::AnalyzedModule, Vec<Diagnostic>> {
    assert_wrapper_route(source);
    driver::analyze_source(&session(), "device.fab", source)
}

fn lower(source: &str) -> Result<(crate::driver::AnalyzedModule, MirProgram), Vec<crate::mir::MirError>> {
    let mut analysis = analyze(source).unwrap_or_else(|errors| panic!("analyze must succeed: {errors:?}"));
    let lowered = crate::mir::lower_analyzed_unit(&mut analysis)?;
    Ok((analysis, lowered))
}

fn analyze_err(source: &str) -> Vec<Diagnostic> {
    match analyze(source) {
        Ok(_) => panic!("expected analyze to fail:\n{source}"),
        Err(errors) => errors,
    }
}

fn arg<'a>(args: &'a [crate::diagnostics::DiagnosticArg], name: &str) -> Option<&'a str> {
    args.iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

fn spanned<'a>(source: &'a str, span: Span) -> &'a str {
    let start = span.start as usize;
    let end = span.end as usize;
    source
        .get(start..end)
        .unwrap_or_else(|| panic!("span {span:?} out of range for:\n{source}"))
}

fn find_range(source: &str, needle: &str) -> (u32, u32) {
    let start = source
        .find(needle)
        .unwrap_or_else(|| panic!("needle {needle:?} missing in:\n{source}"));
    (start as u32, (start + needle.len()) as u32)
}

fn span_in(span: Span, source: &str, needle: &str) -> bool {
    let (start, end) = find_range(source, needle);
    span.start < end && span.end > start
}

fn expect_analyze_issue<'a>(errors: &'a [Diagnostic], issue: &str, reason: &str) -> &'a Diagnostic {
    errors
        .iter()
        .find(|err| err.issue() == Some(issue) && arg(&err.args, "reason") == Some(reason))
        .unwrap_or_else(|| panic!("expected {issue} reason={reason}, got {errors:?}"))
}

fn expect_lower_issue<'a>(errors: &'a [crate::mir::MirError], issue: &str) -> &'a crate::mir::MirError {
    errors
        .iter()
        .find(|err| err.issue == issue)
        .unwrap_or_else(|| panic!("expected {issue}, got {errors:?}"))
}

fn kernel_function<'a>(functions: &'a [MirFunction], analysis: &crate::driver::AnalyzedModule) -> &'a MirFunction {
    let roles = device_roles_from_hir(&analysis.hir);
    functions
        .iter()
        .find(|function| {
            function
                .source
                .is_some_and(|source| roles.get(&source) == Some(&MirDeviceRole::Kernel))
        })
        .unwrap_or_else(|| panic!("wrapper @ nucleum entry missing from {functions:#?}"))
}

fn assert_composed(source: &str) {
    let (analysis, lowered) = lower(source).unwrap_or_else(|errors| panic!("legal wrapper must compose: {errors:?}"));
    let roles = device_roles_from_hir(&analysis.hir);
    assert!(
        surviving_fragment_calls(&lowered.functions, &[], &roles).is_empty(),
        "no fragment call may survive the wrapper route: {:#?}",
        lowered.functions
    );
}

#[test]
fn definition_host_callable_emphasizes_fragment_body() {
    let source = r"
functio host_add(fractus<f32> x) → fractus<f32> {
    redde x
}

@ nucleum fragment
functio helper(fractus<f32> x) → fractus<f32> {
    redde host_add(x)
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde helper(x)
}
";
    let errors = analyze_err(source);
    let err = expect_analyze_issue(&errors, ISSUE_CONTEXT, "host_callable");
    assert_eq!(arg(&err.args, "site"), Some("definition"));
    assert!(
        crate::test_support::args_have_no_legacy_message(&err.args),
        "definition diagnostic must keep catalog identity: {:?}",
        err.args
    );
    let span = err.span.expect("definition diagnostic needs a span");
    assert!(
        span_in(span, source, "host_add(x)"),
        "definition-site CONTEXT must emphasize the fragment body, span={span:?}"
    );
    assert!(
        !span_in(span, source, "redde helper(x)"),
        "definition-site CONTEXT must not point at the wrapper call, span={span:?}"
    );
}

#[test]
fn instantiation_host_callable_emphasizes_call_site() {
    let source = r"
@ nucleum fragment
functio helper(fractus<f32> x) → fractus<f32> {
    redde x
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde helper(x)
}

functio host(fractus<f32> x) → fractus<f32> {
    redde helper(x)
}
";
    let mut analysis = analyze(source).unwrap_or_else(|errors| panic!("host→fragment analyzes: {errors:?}"));
    let errors = crate::mir::lower_analyzed_unit(&mut analysis).expect_err("host→fragment must fail at instantiation");
    let err = expect_lower_issue(&errors, ISSUE_CONTEXT);
    assert!(
        err.message
            .contains("host code cannot call an @ nucleum fragment"),
        "instantiation CONTEXT must name the host caller: {}",
        err.message
    );
    assert_eq!(
        spanned(source, err.span),
        "helper(x)",
        "instantiation CONTEXT must emphasize the host call, span={:?}",
        err.span
    );
    assert!(
        !span_in(err.span, source, "redde x"),
        "instantiation CONTEXT must not emphasize the fragment definition body, span={:?}",
        err.span
    );
    let host_fn = source.find("functio host(").expect("host function");
    assert!(
        err.span.start as usize >= host_fn,
        "call-site primary span must sit in the host function, span={:?}",
        err.span
    );
}

#[test]
fn definition_effect_through_wrapper_keeps_identifier() {
    let source = r"
@ nucleum fragment
functio helper(fractus<f32> x) → fractus<f32> {
    scribe x
    redde x
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde helper(x)
}
";
    let errors = analyze_err(source);
    let err = expect_analyze_issue(&errors, ISSUE_EFFECT, "host_io");
    let span = err.span.expect("effect diagnostic needs a span");
    assert!(
        span_in(span, source, "scribe x"),
        "effect span must sit on the forbidden form: {span:?}"
    );
}

#[test]
fn instantiation_type_emphasizes_call_site() {
    let source = r"
@ nucleum fragment
functio helper(textus x) → textus {
    redde x
}

@ nucleum
functio k(textus x) → textus {
    redde helper(x)
}
";
    let mut analysis = analyze(source).unwrap_or_else(|errors| panic!("textus fragment must typecheck: {errors:?}"));
    let errors = crate::mir::lower_analyzed_unit(&mut analysis).expect_err("target-illegal fragment type must fail");
    let err = expect_lower_issue(&errors, ISSUE_TYPE);
    assert!(
        err.message
            .contains("specialized types are not legal for the current device target"),
        "TYPE identifier must keep catalog wording: {}",
        err.message
    );
    assert!(
        span_in(err.span, source, "helper(x)"),
        "instantiation TYPE must emphasize the wrapper call, span={:?}",
        err.span
    );
}

#[test]
fn matrix_entry_to_fragment_composes_through_wrapper() {
    assert_composed(
        r"
@ nucleum fragment
functio helper(fractus<f32> x) → fractus<f32> {
    redde x
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde helper(x)
}
",
    );
}

#[test]
fn matrix_fragment_to_fragment_to_entry_composes() {
    assert_composed(
        r"
@ nucleum fragment
functio inner(fractus<f32> x) → fractus<f32> {
    redde x
}

@ nucleum fragment
functio outer(fractus<f32> x) → fractus<f32> {
    redde inner(x)
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde outer(x)
}
",
    );
}

#[test]
fn matrix_entry_to_entry_is_nested_dispatch() {
    let source = r"
@ nucleum fragment
functio helper(fractus<f32> x) → fractus<f32> {
    redde x
}

@ nucleum
functio inner(fractus<f32> x) → fractus<f32> {
    redde helper(x)
}

@ nucleum
functio outer(fractus<f32> x) → fractus<f32> {
    redde inner(x)
}
";
    let mut analysis = analyze(source).unwrap_or_else(|errors| panic!("nested dispatch analyzes: {errors:?}"));
    let errors = crate::mir::lower_analyzed_unit(&mut analysis).expect_err("Entry→Entry must fail");
    let err = expect_lower_issue(&errors, ISSUE_CONTEXT);
    assert!(
        err.message
            .contains("@ nucleum cannot call another @ nucleum entry"),
        "nested dispatch must keep the caller/callee catalog text: {}",
        err.message
    );
    assert!(
        span_in(err.span, source, "inner(x)"),
        "nested dispatch span must be the wrapper call: {:?}",
        err.span
    );
}

#[test]
fn matrix_fragment_to_entry_is_rejected() {
    let source = r"
@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde x
}

@ nucleum fragment
functio helper(fractus<f32> x) → fractus<f32> {
    redde k(x)
}

@ nucleum
functio wrap(fractus<f32> x) → fractus<f32> {
    redde helper(x)
}
";
    let errors = analyze_err(source);
    let err = expect_analyze_issue(&errors, ISSUE_CONTEXT, "entry_callee");
    assert_eq!(arg(&err.args, "site"), Some("definition"));
    let span = err.span.expect("entry_callee needs a span");
    assert!(
        span_in(span, source, "k(x)"),
        "Fragment→Entry must point at the fragment call: {span:?}"
    );
}

#[test]
fn matrix_fragment_to_graphics_is_rejected() {
    let source = r"
@ fragment
functio shade() → vacuum {
    redde
}

@ nucleum fragment
functio helper() → vacuum {
    shade()
    redde
}

@ nucleum
functio k() → vacuum {
    redde helper()
}
";
    let errors = analyze_err(source);
    let err = expect_analyze_issue(&errors, ISSUE_CONTEXT, "graphics_stage");
    assert_eq!(arg(&err.args, "site"), Some("definition"));
    let span = err.span.expect("graphics_stage needs a span");
    assert_eq!(
        spanned(source, span),
        "shade()",
        "graphics rejection must point at the call: {span:?}"
    );
}

#[test]
fn matrix_entry_to_graphics_is_rejected() {
    let source = r"
@ fragment
functio shade() → vacuum {
    redde
}

@ nucleum fragment
functio helper() → vacuum {
    redde
}

@ nucleum
functio k() → vacuum {
    shade()
    redde helper()
}
";
    let mut analysis = analyze(source).unwrap_or_else(|errors| panic!("entry→graphics analyzes: {errors:?}"));
    let errors = crate::mir::lower_analyzed_unit(&mut analysis).expect_err("Entry→graphics must fail");
    let err = expect_lower_issue(&errors, ISSUE_CONTEXT);
    assert!(
        err.message.contains("graphics @ fragment"),
        "entry→graphics must name the callee role: {}",
        err.message
    );
    assert_eq!(
        spanned(source, err.span),
        "shade()",
        "entry→graphics span must be the call: {:?}",
        err.span
    );
}

#[test]
fn matrix_entry_to_ordinary_is_rejected() {
    let source = r"
functio host_add(fractus<f32> x) → fractus<f32> {
    redde x
}

@ nucleum fragment
functio helper(fractus<f32> x) → fractus<f32> {
    redde x
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde host_add(helper(x))
}
";
    let mut analysis = analyze(source).unwrap_or_else(|errors| panic!("entry→host analyzes: {errors:?}"));
    let errors = crate::mir::lower_analyzed_unit(&mut analysis).expect_err("Entry→ordinary must fail");
    let err = expect_lower_issue(&errors, ISSUE_CONTEXT);
    assert!(
        err.message.contains("cannot call a host function"),
        "nucleus→ordinary must name the callee class: {}",
        err.message
    );
    assert_eq!(
        spanned(source, err.span),
        "host_add(helper(x))",
        "nucleus→ordinary span must be the host call: {:?}",
        err.span
    );
}

#[test]
fn eval_once_through_wrapper_binds_argument_once() {
    let source = r"
@ nucleum fragment
functio twice(fractus<f32> x) → fractus<f32> {
    redde x + x
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde twice(x)
}
";
    let (analysis, lowered) =
        lower(source).unwrap_or_else(|errors| panic!("eval-once fixture must compose: {errors:?}"));
    let roles = device_roles_from_hir(&analysis.hir);
    assert!(surviving_fragment_calls(&lowered.functions, &[], &roles).is_empty());
    let entry = kernel_function(&lowered.functions, &analysis);
    let bind_locals: Vec<_> = entry
        .blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(|stmt| match &stmt.kind {
            MirStatementKind::Assign {
                place,
                value: MirValue { kind: MirValueKind::Operand(MirOperand::Place(src)), .. },
            } => match (&place.base, &src.base) {
                (MirPlaceBase::Local(dest), MirPlaceBase::Local(src_id)) if dest != src_id => Some(*dest),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert!(
        !bind_locals.is_empty(),
        "composed wrapper must bind the argument once: {entry:#?}"
    );
    let add_uses = entry
        .blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(|stmt| match &stmt.kind {
            MirStatementKind::Assign {
                value: MirValue { kind: MirValueKind::Binary { op: MirBinOp::Add, lhs, rhs }, .. },
                ..
            } => Some((lhs.clone(), rhs.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(add_uses.len(), 1, "composed twice() must keep one add: {entry:#?}");
    assert_eq!(
        add_uses[0].0, add_uses[0].1,
        "both add operands must be the eval-once bind: {entry:#?}"
    );
}

#[test]
fn cycle_rejection_reports_caller_callee_chain() {
    let source = r"
@ nucleum fragment
functio ping(fractus<f32> x) → fractus<f32> {
    redde pong(x)
}

@ nucleum fragment
functio pong(fractus<f32> x) → fractus<f32> {
    redde ping(x)
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde ping(x)
}
";
    let mut analysis = analyze(source).unwrap_or_else(|errors| panic!("cycle analyzes: {errors:?}"));
    let errors = crate::mir::lower_analyzed_unit(&mut analysis).expect_err("cycle must fail closed");
    let err = expect_lower_issue(&errors, ISSUE_CYCLE);
    assert!(
        err.message.contains("ping → pong → ping") || err.message.contains("pong → ping → pong"),
        "cycle diagnostic must carry the caller/callee chain: {}",
        err.message
    );
    assert!(span_in(err.span, source, "pong(x)") || span_in(err.span, source, "ping(x)"));
}

#[test]
fn budget_override_rejects_wrapper_expansion() {
    let source = r"
@ nucleum fragment
functio helper(fractus<f32> x) → fractus<f32> {
    redde x
}

@ nucleum
functio k(fractus<f32> x) → fractus<f32> {
    redde helper(x)
}
";
    let budget = ExpansionBudget { max_concrete_instances: 0, ..ExpansionBudget::DOCUMENTED_DEFAULTS };
    with_budget_override(budget, || {
        let mut analysis = analyze(source).unwrap_or_else(|errors| panic!("budget fixture analyzes: {errors:?}"));
        let errors = crate::mir::lower_analyzed_unit(&mut analysis).expect_err("budget override must fail closed");
        let err = expect_lower_issue(&errors, ISSUE_EXPANSION);
        assert!(
            err.message.contains("instances") && err.message.contains("limit 0"),
            "budget diagnostic must report the observed/limit pair: {}",
            err.message
        );
        assert!(
            err.message.contains("k") && err.message.contains("helper"),
            "budget diagnostic must report the caller/callee chain: {}",
            err.message
        );
        assert!(
            span_in(err.span, source, "helper(x)"),
            "budget span must be the wrapper call: {:?}",
            err.span
        );
    });
}

#[test]
fn catalog_identities_stay_stable() {
    use crate::diagnostics::{semantic_message, DiagnosticArg};
    let definition = vec![
        DiagnosticArg::new("issue", ISSUE_CONTEXT),
        DiagnosticArg::new("reason", "host_callable"),
        DiagnosticArg::new("site", "definition"),
    ];
    assert_eq!(
        semantic_message(SemanticErrorKind::InvalidOperandTypes, &definition).as_deref(),
        Some("@ nucleum fragment cannot call a host function or other non-device-safe callable")
    );
    let instantiation = vec![
        DiagnosticArg::new("issue", ISSUE_CONTEXT),
        DiagnosticArg::new("reason", "host_callable"),
        DiagnosticArg::new("caller", "host"),
        DiagnosticArg::new("site", "call"),
    ];
    assert_eq!(
        semantic_message(SemanticErrorKind::InvalidOperandTypes, &instantiation).as_deref(),
        Some("host code cannot call an @ nucleum fragment")
    );
}
