//! The elementwise list forms added with the 2026-10-04 aggregate operator
//! rulings (`%` and `÷` over a list, unary `-`, the glyph forms `⊙ ⊘`, and a
//! list against a scalar that joins to a wider element) have only the MIR
//! runner as their executor — except on the Python target, which lowers them
//! as comprehensions with the runner's equal-length trap. Every other
//! HIR-backed source target rejects them with the named
//! `lista_lift_unsupported_on_target` diagnostic instead of printing the
//! scalar operator between a list and a number. `+ -` between two lists and
//! `+ - * /` against a scalar of the element type keep their own emitters.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const PRELUDE: &str = "sit lista<numerus> a ← [7, 8, 9]
sit lista<numerus> b ← [2, 3, 4]
sit lista<u8> bytes ← [1, 2, 3]
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

fn rejected_with(target: Target, body: &str) -> bool {
    issues(target, body)
        .iter()
        .any(|issue| issue == "lista_lift_unsupported_on_target")
}

const RUNNER_ONLY: &[&str] = &[
    "sit lista<numerus> r ← a % 2",
    "sit lista<numerus> r ← 9 % a",
    "sit lista<fractus> r ← a ÷ 2",
    "sit lista<fractus> r ← a ÷ b",
    "sit lista<numerus> r ← a ⊙ b",
    "sit lista<numerus> r ← a ⊘ b",
    "sit lista<numerus> r ← -a",
    "sit lista<u16> r ← bytes + 300",
    "sit lista<bivalens> r ← a ≺ b",
    "sit lista<bivalens> r ← a ≥ 8",
    "sit lista<bivalens> r ← 8 ≅ a",
    "sit lista<bivalens> r ← a ≇ b",
];

const KEPT: &[&str] = &[
    "sit lista<numerus> r ← a + b",
    "sit lista<numerus> r ← a - b",
    "sit lista<numerus> r ← a * 2",
    "sit lista<numerus> r ← 2 - a",
    "sit lista<u8> r ← bytes + 1",
];

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_rejects_the_runner_only_list_forms() {
    for body in RUNNER_ONLY {
        assert!(rejected_with(Target::HirTypeScript, body), "`{body}`");
    }
}

#[cfg(feature = "hir-go")]
#[test]
fn go_rejects_the_runner_only_list_forms() {
    for body in RUNNER_ONLY {
        assert!(rejected_with(Target::HirGo, body), "`{body}`");
    }
}

/// The Rust emitter lifts these forms itself (iterator `zip`/`map` over the
/// lanes), so none of them reaches the lift issue.
#[cfg(feature = "hir-rust")]
#[test]
fn rust_emits_the_runner_only_list_forms() {
    for body in RUNNER_ONLY {
        assert!(!rejected_with(Target::HirRust, body), "`{body}`");
    }
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_and_haskell_never_emit_the_runner_only_list_forms() {
    for target in [Target::HirSwift, Target::HirHaskell] {
        for body in RUNNER_ONLY {
            let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
            let source = format!("incipit {{\n{PRELUDE}{body}\n}}\n");
            let result = crate::driver::compile(&session, "<test>", &source, ModuleCompileRequest::default());
            assert!(result.output.is_none(), "{target:?} `{body}` emitted code");
        }
    }
}

#[cfg(feature = "hir-python")]
#[test]
fn python_lowers_the_runner_only_list_forms() {
    // The Python emitter owns the elementwise lowering (comprehensions with
    // the runner's equal-length trap), so every gated form emits instead of
    // failing closed; the corpus files lista/lift-scalar and
    // lista/elementwise-comparison pin the outputs end to end.
    for body in RUNNER_ONLY {
        let session = Session::new(
            Config::default()
                .with_target(Target::HirPython)
                .with_dev_locale_root(),
        );
        let source = format!("incipit {{\n{PRELUDE}{body}\n}}\n");
        let result = crate::driver::compile(&session, "<test>", &source, ModuleCompileRequest::default());
        assert!(result.output.is_some(), "{body} refused: {:?}", result.diagnostics);
    }
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_reemission_keeps_the_runner_only_list_forms() {
    for body in RUNNER_ONLY {
        let found = issues(Target::HirFaber, body);
        assert!(
            !found
                .iter()
                .any(|issue| issue == "lista_lift_unsupported_on_target"),
            "`{body}`: {found:?}"
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn list_forms_with_their_own_emitters_are_never_gated() {
    for body in KEPT {
        let found = issues(Target::HirRust, body);
        assert!(
            !found
                .iter()
                .any(|issue| issue == "lista_lift_unsupported_on_target"),
            "`{body}`: {found:?}"
        );
    }
}
