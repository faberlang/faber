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

// ── size-type Z2 untested oracles (goal.md done-when 3, 4, 6) ─────────────
//
// The run-level companions of this module's check-level slot/pin oracles.
// They live here rather than in `runtime_shape_test.rs` because that module
// asserts analyze/dump facts only; these need the BufferHost runner.

const TN5_CREATOR_RETURN: &str = r#"+++
locale = "en"
+++
fn make<size H, size W>() → tf32[H, W] {
    var tf32[H, W] out ← tf32[H, W]
    out[[0, 0]] ← 7.5
    return out
}
main {
    const tf32[2, 3] a ← make<2, 3>()
    print a.shape().get(0)
    print a.shape().get(1)
    print a.flatten()
}
"#;

// Done-when 3: the callee returns an `[H, W]`-shaped local against the
// declared `→ tf32[H, W]`. The run succeeding is the assertion — the old
// failure mode was `invalid MIR: return type mismatch`.
#[test]
fn size_type_tn5_creator_return_shape_runs() {
    let mut host = BufferHost::default();
    run_source(&session(), "tn5-return.fab", TN5_CREATOR_RETURN, &mut host)
        .expect("the [H, W]-shaped local return must run — invalid MIR: return type mismatch is gone");
    assert_eq!(host.stdout_lines, ["2", "3", "[7.5, 0.0, 0.0, 0.0, 0.0, 0.0]"]);
}

const AFFINE_RETURN: &str = r#"+++
locale = "en"
+++
fn make<size H, size W>() → tf32[H, W] {
    const tf32[] seed ← empty
    const tf32[H + 1, W] out ← seed.create(0.0, [H + 1, W])
    return out
}
main {
    do {
        const tf32[2, 3] a ← make<2, 3>()
        print "returned"
    }
    catch err {
        print "caught"
    }
    print "after"
}
"#;

// Known gap (Z2-MIR residual, done-when 4): the `[H + 1, W]`-against-`[H, W]`
// return does not fail recoverably yet. The callee's FunctionReturn guard
// emits inside the callee directly before `Return`, and the runner routes a
// `TensorShapeCheck` failure into a `TryCall` error block only when the guard
// is the last statement of the caller's TryCall block
// (`shape_guard_precedes_try_call`), so the mismatch escapes `fac`/`cape` as
// a hard error. Probed at `b81752184`:
// - this fixture (affine annotation `tf32[H + 1, W]`): check passes, but the
//   literal-slot instance folds `H + 1` to 3 and `H` to 2, so MIR validation
//   rejects the instance up front — `invalid MIR: return type mismatch`;
// - hole-annotation body (`const tf32[_, _] out ← seed.from_flat(...,
//   [H + 1, W])`): the runtime relation fires — issue
//   `runner_tensor_shape_mismatch`, message `tensor runtime shape mismatch:
//   3 != 2` — but escapes the catch;
// - a concrete destination at a hole-formal callee crashes lowering
//   (`unsupported MIR lowering: path that does not resolve to a local value`).
// The successor unit needs the return-boundary guard routable into the
// caller's fac/cape (sibling of the pin-store relation-coverage residual)
// plus no-fold retention of the affine annotation.
#[ignore = "callee-side return-boundary shape guard is not catchable by fac/cape yet"]
#[test]
fn size_type_affine_return_mismatch_is_recoverable_at_the_return() {
    let mut host = BufferHost::default();
    run_source(&session(), "affine-return.fab", AFFINE_RETURN, &mut host)
        .expect("the [H + 1, W]-against-[H, W] return must fail recoverably into the cape handler");
    assert_eq!(host.stdout_lines, ["caught", "after"]);
}

const ANONYMOUS_LIFT_ITERA: &str = r#"+++
locale = "en"
+++
fn make<size H, size W>() → tf32[H, W] {
    var tf32[H, W] out ← tf32[H, W]
    out[[0, 0]] ← 7.5
    return out
}
main {
    const tf32[_, _] a ← make<2, 3>()
    var f32 sum ← 0.0
    var int coords ← 0
    var int visits ← 0
    for from a at [i, j] const c {
        sum ← sum + c
        coords ← coords + i * 10 + j
        visits ← visits + 1
    }
    print sum
    print coords
    print visits
}
"#;

// Done-when 6: the call's result binds at a `tf32[_, _]` annotation
// (anonymous lift, opaque extent axes) and iterates under R1 — the loop
// reads its bounds and cells from the carrier, so the visited count and
// values are the proof the opaque axes are usable at run time.
#[test]
fn size_type_anonymous_lift_result_iterates_under_r1() {
    let mut host = BufferHost::default();
    run_source(&session(), "anonymous-lift-itera.fab", ANONYMOUS_LIFT_ITERA, &mut host)
        .expect("the anonymous-lift result must iterate via its carrier");
    assert_eq!(host.stdout_lines, ["7.5", "36", "6"]);
}
