//! A2-LIFT G02 and the operator-consistency lift: a scalar operator against a
//! register vector or matrix (`v * s`, `s - m`, `-v`), `÷` and `⤒ ⤓` between
//! two registers unroll per lane in MIR, so only the MIR-backed targets
//! execute them. Every HIR-backed source target rejects the form with the
//! named `vector_lift_unsupported_on_target` (or
//! `matrix_lift_unsupported_on_target`) diagnostic instead of printing the
//! scalar operator between an aggregate and a number. Register with register
//! `+ -` and the `⊙ ⊘` products keep their own emitters and are never gated
//! here.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const PRELUDE: &str = "fixum vector<f32, 3> a ← [1.0, 2.0, 3.0] ↦ vector<f32, 3>
fixum vector<f32, 3> b ← [3.0, 2.0, 1.0] ↦ vector<f32, 3>
fixum matrix<f32, [2, 2]> m ← [[1.0, 2.0], [3.0, 4.0]] ↦ matrix<f32, [2, 2]>
fixum matrix<f32, [2, 2]> n ← [[4.0, 3.0], [2.0, 1.0]] ↦ matrix<f32, [2, 2]>
fixum f32 s ← 2.0
";

const VECTOR_ISSUE: &str = "vector_lift_unsupported_on_target";
const MATRIX_ISSUE: &str = "matrix_lift_unsupported_on_target";

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

fn rejected_with(target: Target, body: &str, issue: &str) -> bool {
    issues(target, body).iter().any(|found| found == issue)
}

const LIFTED: &[&str] = &[
    "fixum vector<f32, 3> r ← a * s",
    "fixum vector<f32, 3> r ← s * a",
    "fixum vector<f32, 3> r ← a / s",
    "fixum vector<f32, 3> r ← s - a",
    "fixum vector<f32, 3> r ← a + 1.0",
    "fixum vector<f32, 3> r ← -a",
    "fixum vector<f32, 3> r ← a ÷ s",
    "fixum vector<f32, 3> r ← a ÷ b",
    "fixum vector<f32, 3> r ← a ⤒ s",
    "fixum vector<f32, 3> r ← a ⤓ b",
];

const LIFTED_MATRIX: &[&str] = &[
    "fixum matrix<f32, [2, 2]> r ← m * s",
    "fixum matrix<f32, [2, 2]> r ← s * m",
    "fixum matrix<f32, [2, 2]> r ← m / s",
    "fixum matrix<f32, [2, 2]> r ← s - m",
    "fixum matrix<f32, [2, 2]> r ← m + 1.0",
    "fixum matrix<f32, [2, 2]> r ← -m",
    "fixum matrix<f32, [2, 2]> r ← m ÷ s",
    "fixum matrix<f32, [2, 2]> r ← m ÷ n",
    "fixum matrix<f32, [2, 2]> r ← m ⤒ s",
    "fixum matrix<f32, [2, 2]> r ← m ⤓ n",
];

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_rejects_the_lifted_register_forms() {
    for body in LIFTED {
        assert!(rejected_with(Target::HirTypeScript, body, VECTOR_ISSUE), "`{body}`");
    }
    for body in LIFTED_MATRIX {
        assert!(rejected_with(Target::HirTypeScript, body, MATRIX_ISSUE), "`{body}`");
    }
}

#[cfg(feature = "hir-go")]
#[test]
fn go_rejects_the_lifted_register_forms() {
    for body in LIFTED {
        assert!(rejected_with(Target::HirGo, body, VECTOR_ISSUE), "`{body}`");
    }
    for body in LIFTED_MATRIX {
        assert!(rejected_with(Target::HirGo, body, MATRIX_ISSUE), "`{body}`");
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_rejects_the_lifted_register_forms() {
    for body in LIFTED {
        assert!(rejected_with(Target::HirRust, body, VECTOR_ISSUE), "`{body}`");
    }
    for body in LIFTED_MATRIX {
        assert!(rejected_with(Target::HirRust, body, MATRIX_ISSUE), "`{body}`");
    }
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_python_and_haskell_never_emit_the_lifted_register_forms() {
    for target in [Target::HirSwift, Target::HirPython, Target::HirHaskell] {
        for body in LIFTED.iter().chain(LIFTED_MATRIX) {
            let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
            let source = format!("incipit {{\n{PRELUDE}{body}\n}}\n");
            let result = crate::driver::compile(&session, "<test>", &source, ModuleCompileRequest::default());
            // Swift and Python may stop earlier on the vector type itself;
            // they must still fail, never emit.
            assert!(result.output.is_none(), "{target:?} `{body}` emitted code");
        }
    }
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_reemission_keeps_the_lifted_register_forms() {
    for body in LIFTED.iter().chain(LIFTED_MATRIX) {
        let found = issues(Target::HirFaber, body);
        assert!(
            !found
                .iter()
                .any(|issue| issue == VECTOR_ISSUE || issue == MATRIX_ISSUE),
            "`{body}`: {found:?}"
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn register_with_register_plus_keeps_its_rust_emitter() {
    for body in [
        "fixum vector<f32, 3> r ← a + b",
        "fixum matrix<f32, [2, 2]> r ← m + n",
        "fixum matrix<f32, [2, 2]> r ← m ⊙ n",
    ] {
        let found = issues(Target::HirRust, body);
        assert!(
            !found
                .iter()
                .any(|issue| issue == VECTOR_ISSUE || issue == MATRIX_ISSUE),
            "`{body}`: {found:?}"
        );
    }
}
