use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

const SLOT_LIFT: &str = r#"+++
locale = "en"
+++
fn make<size H, size W>() → tf32[H, W] {
    const tf32[] seed ← empty
    const tf32[H, W] out ← seed.from_flat([0.0, 0.0, 0.0, 0.0, 0.0, 0.0], [H, W])
    return out
}
main {
    var int w ← 4
    const tf32[3, 4] a ← make<3, size w>()
    print a.shape().get(0)
    print a.shape().get(1)
}
"#;

const PIN: &str = r#"+++
locale = "en"
+++
fn load(textus path) → tf32[_, _] {
    const tf32[] seed ← empty
    const tf32[2, 3] t ← seed.from_flat([1.0, 2.0, 3.0, 4.0, 5.0, 6.0], [2, 3])
    return t
}
main {
    const size H ← 2
    const size W ← 3
    const tf32[H, W] elev ← load("x")
    print elev.shape().get(0)
    print elev.shape().get(1)
}
"#;

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

// Known gap (Z2-MIR residual): a spelled runtime lift in the call slot
// (`make<3, size w>()`) has no caller->callee channel for its value — the
// callee's size param needs a synthetic scalar argument (or equivalent
// operand-backed passing) for pure creators with no tensor argument to
// carry the witness. Check-level binding, the bare-name did-you-mean, the
// pin store, and literal creators all run; these two oracles persist for
// the successor unit.
#[ignore = "runtime size-param passing for pure creators is unimplemented"]
#[test]
fn slot_pin_call_slot_runtime_lift_runs() {
    let mut host = BufferHost::default();
    run_source(&session(), "slot-lift.fab", SLOT_LIFT, &mut host).expect("runtime call-slot lift runs");
    assert_eq!(host.stdout_lines, ["3", "4"]);
}

// Known gap (Z2-MIR residual, sibling of the call-slot pair above): the pin
// store's run-level acceptance needs destination-validation relaxation that
// consults the emitted runtime relation — type-level identity cannot
// discriminate (the destination's ScalarBinding term and the callee's
// FunctionParameterAxis term name the same extent through the call boundary;
// the matching-derived-witness contract correctly forbids accepting on
// runtime-shape alone). The check-level pin, the Store obligation record,
// and the emitted runtime check all land; this run oracle persists for the
// successor unit that gives validate_optional_destination
// relation-coverage semantics.
#[ignore = "pin store run-level blocked on relation-coverage destination validation"]
#[test]
fn slot_pin_opaque_return_store_runs() {
    let mut host = BufferHost::default();
    run_source(&session(), "pin.fab", PIN, &mut host).expect("opaque return pins at declaration store");
    assert_eq!(host.stdout_lines, ["2", "3"]);
}

// Known gap (Z2-MIR residual): a spelled runtime lift in the call slot
// (`make<3, size w>()`) has no caller->callee channel for its value — the
// callee's size param needs a synthetic scalar argument (or equivalent
// operand-backed passing) for pure creators with no tensor argument to
// carry the witness. Check-level binding, the bare-name did-you-mean, the
// pin store, and literal creators all run; these two oracles persist for
// the successor unit.
#[ignore = "runtime size-param passing for pure creators is unimplemented"]
#[test]
fn slot_pin_negative_call_slot_lift_is_recoverable() {
    let source = SLOT_LIFT.replace("w ← 4", "w ← -1");
    let mut host = BufferHost::default();
    let error = run_source(&session(), "negative-slot-lift.fab", &source, &mut host).expect_err("negative lift fails");
    let RunSourceError::Stepper(errors) = error else {
        panic!("expected runner error: {error:?}")
    };
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "size_lift_negative"),
        "{errors:?}"
    );
}
