use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

const RUNTIME_TARGET: &str = r#"+++
locale = "en"
+++
main {
    let list<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    mut int n ← 3
    let tf32[2, size n] a ← flat ↦ tf32[2, size n]
    print a.shape().get(0)
    print a.shape().get(1)
}
"#;

/// C2 equivalence fixture: the derived `↤` spelling. The explicit spelling
/// swaps only the declaration line ([`conversio_assign_explicit_source`]), so
/// outcome equality between the two is a real derivation check, not a fixture
/// artifact.
const CONVERSIO_ASSIGN_DERIVED: &str = r#"+++
locale = "en"
+++
main {
    let list<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    mut int n ← 3
    mut tf32[2, size n] a ↤ flat
    print a.shape().get(0)
    print a.shape().get(1)
}
"#;

fn conversio_assign_explicit_source(derived: &str) -> String {
    derived.replace("mut tf32[2, size n] a ↤ flat", "let tf32[2, size n] a ← flat ↦ tf32[2, size n]")
}

#[test]
fn conversion_check_runtime_target_axes_run() {
    let mut host = BufferHost::default();
    run_source(&session(), "conversion-runtime-target.fab", RUNTIME_TARGET, &mut host)
        .expect("runtime conversion target axes execute");
    assert_eq!(host.stdout_lines, ["2", "3"]);
}

#[test]
fn conversion_check_negative_lift_is_named() {
    let source = RUNTIME_TARGET.replace("n ← 3", "n ← -3");
    let mut host = BufferHost::default();
    let error = run_source(&session(), "conversion-negative-lift.fab", &source, &mut host)
        .expect_err("negative size lift fails");
    let RunSourceError::Stepper(errors) = error else {
        panic!("expected runner failure: {error:?}")
    };
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "size_lift_negative"),
        "{errors:?}"
    );
}

#[test]
fn conversion_check_element_mismatch_is_rejected_at_check() {
    let source = r#"+++
locale = "en"
+++
main {
    let list<i32> flat ← [1, 2, 3, 4, 5, 6]
    let tf32[2, 3] tensor ← flat ↦ tf32[2, 3]
}
"#;
    let mut host = BufferHost::default();
    let error = run_source(&session(), "conversion-element-mismatch.fab", source, &mut host)
        .expect_err("integer list cannot convert to float tensor");
    let RunSourceError::Frontend(errors) = error else {
        panic!("expected check failure: {error:?}")
    };
    assert!(format!("{errors:?}").contains("lista_to_tensor_element_mismatch"), "{errors:?}");
}

#[test]
fn conversion_check_length_mismatch_is_named_with_counts() {
    let source = RUNTIME_TARGET.replace("[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]", "[1.0, 2.0, 3.0]");
    let mut host = BufferHost::default();
    let error = run_source(&session(), "conversion-length-mismatch.fab", &source, &mut host)
        .expect_err("wrong element count fails");
    let RunSourceError::Stepper(errors) = error else {
        panic!("expected runner failure: {error:?}")
    };
    let error = errors
        .iter()
        .find(|error| error.issue == "conversion_length_mismatch")
        .expect("named issue");
    assert_eq!(error.arg("expected"), Some("6"));
    assert_eq!(error.arg("actual"), Some("3"));
}

#[test]
fn conversio_assign_tensor_row_runs() {
    let mut host = BufferHost::default();
    run_source(
        &session(),
        "conversio-assign-tensor-row.fab",
        CONVERSIO_ASSIGN_DERIVED,
        &mut host,
    )
    .expect("derived conversion assignment runs");
    assert_eq!(host.stdout_lines, ["2", "3"]);
}

#[test]
fn conversio_assign_equivalent_to_explicit() {
    let explicit = conversio_assign_explicit_source(CONVERSIO_ASSIGN_DERIVED);
    let mut derived_host = BufferHost::default();
    run_source(
        &session(),
        "conversio-assign-derived.fab",
        CONVERSIO_ASSIGN_DERIVED,
        &mut derived_host,
    )
    .expect("derived conversion assignment runs");
    let mut explicit_host = BufferHost::default();
    run_source(&session(), "conversio-assign-explicit.fab", &explicit, &mut explicit_host)
        .expect("explicit conversion runs");
    assert_eq!(derived_host.stdout_lines, explicit_host.stdout_lines);
    // `pending_stdout` is the host's unclosed raw-stdout fragment; with
    // `stdout_lines` it covers every stdout byte either run captured.
    assert_eq!(derived_host.pending_stdout, explicit_host.pending_stdout);
}

#[test]
fn conversio_assign_failure_matches_explicit() {
    let derived = CONVERSIO_ASSIGN_DERIVED.replace("[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]", "[1.0, 2.0, 3.0]");
    let explicit = conversio_assign_explicit_source(&derived);
    let mut derived_host = BufferHost::default();
    let derived_error = run_source(&session(), "conversio-assign-mismatch.fab", &derived, &mut derived_host)
        .expect_err("length mismatch fails in the derived spelling");
    let mut explicit_host = BufferHost::default();
    let explicit_error = run_source(
        &session(),
        "conversio-assign-explicit-mismatch.fab",
        &explicit,
        &mut explicit_host,
    )
    .expect_err("length mismatch fails in the explicit spelling");
    let RunSourceError::Stepper(derived_errors) = derived_error else {
        panic!("expected runner failure: {derived_error:?}")
    };
    let RunSourceError::Stepper(explicit_errors) = explicit_error else {
        panic!("expected runner failure: {explicit_error:?}")
    };
    let derived_error = derived_errors
        .iter()
        .find(|error| error.issue == "conversion_length_mismatch")
        .expect("derived spelling names the mismatch");
    let explicit_error = explicit_errors
        .iter()
        .find(|error| error.issue == "conversion_length_mismatch")
        .expect("explicit spelling names the mismatch");
    assert_eq!(derived_error.arg("expected"), Some("6"));
    assert_eq!(derived_error.arg("actual"), Some("3"));
    // Spans legitimately differ (the declaration lines have different
    // columns); everything else (rendered text, issue, code, structured
    // facts) must not.
    let mut derived_without_span = derived_error.clone();
    derived_without_span.span = explicit_error.span;
    assert_eq!(&derived_without_span, explicit_error);
    assert_eq!(derived_error.args, explicit_error.args);
}
