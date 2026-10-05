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
    const list<f32> flat ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    var int n ← 3
    const tf32[2, size n] a ← flat ↦ tf32[2, size n]
    print a.shape().get(0)
    print a.shape().get(1)
}
"#;

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
    const list<i32> flat ← [1, 2, 3, 4, 5, 6]
    const tf32[2, 3] tensor ← flat ↦ tf32[2, 3]
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
