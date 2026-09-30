//! K4: the elementwise tensor lift has only the MIR runner as its executor.
//! Every HIR-backed source target rejects the lifted forms with the named
//! `tensor_lift_unsupported_on_target` diagnostic instead of printing the
//! scalar operator between tensor values.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const PRELUDE: &str = "fixum tensor<f32, []> seed ← vacua
fixum tensor<f32, [2]> a ← seed.strue([1.0, 2.0], [2])
fixum tensor<f32, [2]> b ← seed.strue([2.0, 1.0], [2])
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
    "fixum tensor<bivalens, [2]> r ← a > b",
    "fixum tensor<f32, [2]> r ← a ⤒ b",
    "fixum tensor<f32, [2]> r ← (a > b) ✓ a ✗ b",
    "fixum tensor<f32, [2]> r ← a ÷ b",
];

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_rejects_the_lifted_forms() {
    for body in LIFTED {
        assert_rejected(Target::HirTypeScript, body);
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
