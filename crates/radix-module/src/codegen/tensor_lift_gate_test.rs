//! K4: the elementwise tensor lift has only the MIR runner as its executor.
//! Every HIR-backed source target rejects the lifted forms with the named
//! `tensor_lift_unsupported_on_target` diagnostic instead of printing the
//! scalar operator between tensor values.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const PRELUDE: &str = "fixum tensor<f32, [2]> a ← [1.0, 2.0] ↦ tensor<f32, [2]>
fixum tensor<f32, [2]> b ← [2.0, 1.0] ↦ tensor<f32, [2]>
";

fn issues(target: Target, body: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
    let source = format!("incipit {{\n{PRELUDE}{body}\n}}\n");
    let result = crate::driver::compile(&session, "<test>", &source, ModuleCompileRequest::default());
    result
        .diagnostics
        .iter()
        .flat_map(|diagnostic| diagnostic.args.iter())
        .filter(|arg| arg.name == "issue")
        .map(|arg| arg.value.clone())
        .collect()
}

fn assert_rejected(target: Target, body: &str) {
    let found = issues(target, body);
    assert!(
        found
            .iter()
            .any(|issue| issue == "tensor_lift_unsupported_on_target"),
        "{target:?} `{body}` was not rejected by the lift gate: {found:?}"
    );
}

const LIFTED: &[&str] = &[
    "fixum tensor<f32, [2]> r ← a * 0.5",
    "fixum tensor<f32, [2]> r ← -a",
    "fixum tensor<bivalens, [2]> r ← a ≻ b",
    "fixum tensor<f32, [2]> r ← a ⤒ b",
    "fixum tensor<f32, [2]> r ← (a ≻ b) ✓ a ✗ b",
    "fixum tensor<f32, [2]> r ← a ÷ b",
];

/// The forms the TypeScript carrier lifts itself (`Tensor.lift`, `map`,
/// `select`): every entry of `LIFTED` plus the remaining scalar, logic and
/// `vel` forms.
#[cfg(feature = "hir-ts")]
const TS_LIFTED: &[&str] = &[
    "fixum tensor<f32, [2]> r ← a * 0.5",
    "fixum tensor<f32, [2]> r ← 10.0 - a",
    "fixum tensor<f32, [2]> r ← a + 1.0",
    "fixum tensor<f32, [2]> r ← a / 4.0",
    "fixum tensor<f32, [2]> r ← a % 3.0",
    "fixum tensor<f32, [2]> r ← -a",
    "fixum tensor<bivalens, [2]> r ← a ≻ b",
    "fixum tensor<f32, [2]> r ← a ⤒ b",
    "fixum tensor<f32, [2]> r ← a ⤓ b",
    "fixum tensor<f32, [2]> r ← (a ≻ b) ✓ a ✗ b",
    "fixum tensor<f32, [2]> r ← (a ≻ b) ✓ 1.0 ✗ 0.0",
    "fixum tensor<bivalens, [2]> r ← non (a ≻ b)",
    "fixum tensor<bivalens, [2]> r ← (a ≺ 2.0) aut (a ≻ 3.0)",
    "fixum tensor<bivalens, [2]> r ← (a ≻ 1.0) et (a ≤ b)",
    "fixum tensor<f32, [2]> r ← a ÷ b",
    "fixum tensor<bivalens, [2]> r ← a ≅ b",
    "fixum tensor<bivalens, [2]> r ← a ≇ 2.0",
    "fixum tensor<bivalens, [2]> r ← a ≤ b",
];

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_admits_the_whole_tensor_lift() {
    for body in LIFTED.iter().chain(TS_LIFTED) {
        let found = issues(Target::HirTypeScript, body);
        assert!(
            !found
                .iter()
                .any(|issue| issue == "tensor_lift_unsupported_on_target"),
            "TypeScript `{body}` is gated: {found:?}"
        );
    }
}

#[cfg(feature = "hir-go")]
#[test]
fn go_rejects_the_lifted_forms() {
    for body in LIFTED {
        assert_rejected(Target::HirGo, body);
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_rejects_the_lifted_forms() {
    // Tensor `*` against a scalar keeps the older Rust addita-family message.
    for body in &LIFTED[1..] {
        assert_rejected(Target::HirRust, body);
    }
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_python_and_haskell_reject_the_lifted_forms() {
    for target in [Target::HirSwift, Target::HirPython, Target::HirHaskell] {
        for body in &LIFTED[1..] {
            // Swift and Python may stop earlier on the tensor type itself;
            // they must still fail, never emit.
            let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
            let source = format!("incipit {{\n{PRELUDE}{body}\n}}\n");
            let result = crate::driver::compile(&session, "<test>", &source, ModuleCompileRequest::default());
            assert!(result.output.is_none(), "{target:?} `{body}` emitted code");
        }
    }
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_reemission_keeps_the_lifted_forms() {
    let found = issues(Target::HirFaber, "fixum tensor<bivalens, [2]> r ← a > b");
    assert!(
        !found
            .iter()
            .any(|issue| issue == "tensor_lift_unsupported_on_target"),
        "{found:?}"
    );
}

#[cfg(feature = "hir-ts")]
#[test]
fn same_element_tensor_plus_keeps_its_emitter() {
    let found = issues(Target::HirTypeScript, "fixum tensor<f32, [2]> r ← a + b");
    assert!(
        !found
            .iter()
            .any(|issue| issue == "tensor_lift_unsupported_on_target"),
        "{found:?}"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_admits_scalar_coalesce_over_a_shifted_view() {
    // The runtime carries `vel` over a nullable-element tensor as `coalesce`.
    let found = issues(Target::HirRust, "fixum tensor<f32, [2]> r ← a.disloco([1]) vel 0.0");
    // Only the unused-binding warnings remain: the form type-checks and emits.
    assert!(found.iter().all(|issue| issue == "unused_variable"), "{found:?}");
}

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_still_rejects_coalesce_over_a_shifted_view() {
    assert_rejected(Target::HirTypeScript, "fixum tensor<f32, [2]> r ← a.disloco([1]) vel 0.0");
}

/// `≡ ≢` between two tensors compare whole values; the runner, the
/// TypeScript emitter and the Rust runtime are their executors, so every other
/// HIR-backed target names `tensor_equality_unsupported_on_target`.
const WHOLE_EQUALITY: &[&str] = &[
    "fixum bivalens r ← a ≡ b",
    "fixum bivalens r ← a ≢ b",
    "fixum bivalens r ← a ≡ a",
];

fn assert_equality_rejected(target: Target, body: &str) {
    let found = issues(target, body);
    assert!(
        found
            .iter()
            .any(|issue| issue == "tensor_equality_unsupported_on_target"),
        "{target:?} `{body}` was not rejected by the equality gate: {found:?}"
    );
    assert!(
        !found
            .iter()
            .any(|issue| issue == "tensor_lift_unsupported_on_target"),
        "{target:?} `{body}` is not an elementwise lift: {found:?}"
    );
}

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_emits_whole_tensor_equality() {
    for body in WHOLE_EQUALITY {
        let found = issues(Target::HirTypeScript, body);
        assert!(
            !found
                .iter()
                .any(|issue| issue == "tensor_equality_unsupported_on_target"),
            "TypeScript `{body}` is gated: {found:?}"
        );
    }
}

#[cfg(feature = "hir-go")]
#[test]
fn go_rejects_whole_tensor_equality_by_name() {
    for body in WHOLE_EQUALITY {
        assert_equality_rejected(Target::HirGo, body);
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_emits_whole_tensor_equality() {
    for body in WHOLE_EQUALITY {
        let found = issues(Target::HirRust, body);
        assert!(
            !found
                .iter()
                .any(|issue| issue == "tensor_equality_unsupported_on_target"),
            "Rust `{body}` is gated: {found:?}"
        );
    }
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_python_and_haskell_never_emit_whole_tensor_equality() {
    for target in [Target::HirSwift, Target::HirPython, Target::HirHaskell] {
        for body in WHOLE_EQUALITY {
            let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
            let source = format!("incipit {{\n{PRELUDE}{body}\n}}\n");
            let result = crate::driver::compile(&session, "<test>", &source, ModuleCompileRequest::default());
            assert!(result.output.is_none(), "{target:?} `{body}` emitted code");
        }
    }
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_reemission_keeps_whole_tensor_equality() {
    for body in WHOLE_EQUALITY {
        let found = issues(Target::HirFaber, body);
        assert!(
            !found
                .iter()
                .any(|issue| issue == "tensor_equality_unsupported_on_target"),
            "`{body}`: {found:?}"
        );
    }
}
