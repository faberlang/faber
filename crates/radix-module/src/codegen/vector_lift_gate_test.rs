//! A2-LIFT G02: a scalar operator against a register vector (`v * s`, `s - v`,
//! `-v`) unrolls per lane in MIR, so only the MIR-backed targets execute it.
//! Every HIR-backed source target rejects the form with the named
//! `vector_lift_unsupported_on_target` diagnostic instead of printing the
//! scalar operator between a vector and a number. Vector with vector `+ -`
//! keeps its own emitters and is never gated here.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const PRELUDE: &str = "fixum vector<f32, 3> a ← [1.0, 2.0, 3.0] ↦ vector<f32, 3>
fixum vector<f32, 3> b ← [3.0, 2.0, 1.0] ↦ vector<f32, 3>
fixum f32 s ← 2.0
";

fn issues(target: Target, body: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
    let source = format!("incipit {{\n{PRELUDE}{body}\n}}\n");
    let result =
        crate::driver::compile(&session, "<test>", &source, ModuleCompileRequest::default());
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
        .any(|issue| issue == "vector_lift_unsupported_on_target")
}

const LIFTED: &[&str] = &[
    "fixum vector<f32, 3> r ← a * s",
    "fixum vector<f32, 3> r ← s * a",
    "fixum vector<f32, 3> r ← a / s",
    "fixum vector<f32, 3> r ← s - a",
    "fixum vector<f32, 3> r ← a + 1.0",
    "fixum vector<f32, 3> r ← -a",
];

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_rejects_the_lifted_vector_forms() {
    for body in LIFTED {
        assert!(rejected_with(Target::HirTypeScript, body), "`{body}`");
    }
}

#[cfg(feature = "hir-go")]
#[test]
fn go_rejects_the_lifted_vector_forms() {
    for body in LIFTED {
        assert!(rejected_with(Target::HirGo, body), "`{body}`");
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_rejects_the_lifted_vector_forms() {
    for body in LIFTED {
        assert!(rejected_with(Target::HirRust, body), "`{body}`");
    }
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_python_and_haskell_never_emit_the_lifted_vector_forms() {
    for target in [Target::HirSwift, Target::HirPython, Target::HirHaskell] {
        for body in LIFTED {
            let session =
                Session::new(Config::default().with_target(target).with_dev_locale_root());
            let source = format!("incipit {{\n{PRELUDE}{body}\n}}\n");
            let result = crate::driver::compile(
                &session,
                "<test>",
                &source,
                ModuleCompileRequest::default(),
            );
            // Swift and Python may stop earlier on the vector type itself;
            // they must still fail, never emit.
            assert!(result.output.is_none(), "{target:?} `{body}` emitted code");
        }
    }
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_reemission_keeps_the_lifted_vector_forms() {
    for body in LIFTED {
        let found = issues(Target::HirFaber, body);
        assert!(
            !found
                .iter()
                .any(|issue| issue == "vector_lift_unsupported_on_target"),
            "`{body}`: {found:?}"
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn vector_with_vector_plus_keeps_its_rust_emitter() {
    let found = issues(Target::HirRust, "fixum vector<f32, 3> r ← a + b");
    assert!(
        !found
            .iter()
            .any(|issue| issue == "vector_lift_unsupported_on_target"),
        "{found:?}"
    );
}
