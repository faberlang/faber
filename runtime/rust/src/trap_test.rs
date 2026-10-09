use std::hint::black_box;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::Trap;
use crate::trap::{fail, ok, ok_static, require, some, unreachable};

/// Run `f`, require a panic, and return its message text.
fn panic_text<R>(f: impl FnOnce() -> R) -> String {
    let Err(payload) = catch_unwind(AssertUnwindSafe(f)) else {
        panic!("expected a panic, none occurred")
    };
    if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else if let Some(text) = payload.downcast_ref::<&'static str>() {
        (*text).to_owned()
    } else {
        panic!("panic payload is neither String nor &str")
    }
}

const FIXED: &[(Trap, &str)] = &[
    (Trap::ListaIndex, "lista index trap"),
    (Trap::TextusIndex, "textus index trap"),
    (Trap::AsciiIndex, "ascii index trap"),
    (Trap::OctetiIndex, "octeti index trap"),
    (Trap::TabulaKey, "tabula missing key"),
    (Trap::Nonnull, "nonnull assertion failed"),
    (Trap::RangeStep, "range step must be positive"),
    (
        Trap::Lockstep,
        "itera lockstep sources must have equal length",
    ),
    (Trap::ListaLength, "lista elementwise length mismatch"),
    (Trap::Capacity, "bounded lista literal exceeds capacity"),
    (Trap::FailableCall, "failable call failed"),
    (
        Trap::Conversion("valor", "numerus"),
        "valor to numerus conversion failed",
    ),
    (
        Trap::OutOfRange("decimal", "numerus"),
        "decimal to numerus conversion out of range",
    ),
    (
        Trap::Other("sermo materialization failed"),
        "sermo materialization failed",
    ),
];

#[test]
fn every_trap_displays_its_exact_text() {
    for (trap, text) in FIXED {
        assert_eq!(trap.to_string(), *text);
    }
}

#[test]
fn fail_panics_with_the_message() {
    assert_eq!(panic_text(|| fail("boom")), "boom");
    assert_eq!(panic_text(|| fail(Trap::ListaIndex)), "lista index trap");
}

#[test]
fn some_returns_the_value_and_traps_on_none_like_option_expect() {
    assert_eq!(some(Some(7), Trap::ListaIndex), 7);
    for (trap, text) in FIXED {
        let ours = panic_text(|| some(None::<i32>, *trap));
        let std = panic_text(|| black_box(None::<i32>).expect(text));
        assert_eq!(ours, *text);
        assert_eq!(ours, std, "{trap:?} differs from Option::expect");
    }
}

#[test]
fn ok_returns_the_value_and_traps_on_err_like_result_expect() {
    assert_eq!(ok(Ok::<_, String>(7), Trap::FailableCall), 7);
    for (trap, text) in FIXED {
        let ours = panic_text(|| ok(Err::<i32, _>("bad input"), *trap));
        let std = panic_text(|| black_box(Err::<i32, _>("bad input")).expect(text));
        assert_eq!(ours, format!("{text}: \"bad input\""));
        assert_eq!(ours, std, "{trap:?} differs from Result::expect");
    }
}

#[test]
fn ok_uses_the_debug_form_of_the_error() {
    // Any `Debug` error: the text carries its `{:?}` form, not its `Display`.
    let ours = panic_text(|| {
        ok(
            Err::<(), _>((3_usize, "x")),
            Trap::Conversion("textus", "numerus"),
        );
    });
    assert_eq!(ours, "textus to numerus conversion failed: (3, \"x\")");
}

#[test]
fn ok_static_returns_the_value_and_traps_with_the_err_text() {
    assert_eq!(ok_static(Ok::<_, &'static str>(7)), 7);
    assert_eq!(
        panic_text(|| ok_static(Err::<i32, _>(
            "tensor structa element count does not match shape"
        ))),
        "tensor structa element count does not match shape"
    );
}

#[test]
fn require_passes_on_true_and_traps_on_false() {
    require(true, Trap::RangeStep);
    for (trap, text) in FIXED {
        assert_eq!(panic_text(|| require(false, *trap)), *text);
    }
}

#[test]
fn unreachable_names_the_broken_invariant() {
    assert_eq!(
        panic_text(|| unreachable("emitter lowered a hole")),
        "internal error: entered unreachable code: emitter lowered a hole"
    );
}

#[test]
fn the_oracle_substrings_survive_through_other() {
    for text in [
        "sermo materialization failed",
        "does not fit in `i64`",
        "negative input",
        "tensor structa element count does not match shape",
    ] {
        assert_eq!(panic_text(|| fail(Trap::Other(text))), text);
    }
}
