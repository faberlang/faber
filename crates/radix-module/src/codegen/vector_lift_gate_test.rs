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

const PRELUDE: &str = "sit vector<f32, magnitudo 3> a ← [1.0, 2.0, 3.0] ↦ vector<f32, magnitudo 3>
sit vector<f32, magnitudo 3> b ← [3.0, 2.0, 1.0] ↦ vector<f32, magnitudo 3>
sit matrix<f32, [2, 2]> m ← [[1.0, 2.0], [3.0, 4.0]] ↦ matrix<f32, [2, 2]>
sit matrix<f32, [2, 2]> n ← [[4.0, 3.0], [2.0, 1.0]] ↦ matrix<f32, [2, 2]>
sit f32 s ← 2.0
";

const VECTOR_ISSUE: &str = "vector_lift_unsupported_on_target";
const MATRIX_ISSUE: &str = "matrix_lift_unsupported_on_target";

fn issues(target: Target, body: &str) -> Vec<String> {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(target)
            .with_dev_locale_root(),
    );
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
    "sit vector<f32, magnitudo 3> r ← a * s",
    "sit vector<f32, magnitudo 3> r ← s * a",
    "sit vector<f32, magnitudo 3> r ← a / s",
    "sit vector<f32, magnitudo 3> r ← s - a",
    "sit vector<f32, magnitudo 3> r ← a + 1.0",
    "sit vector<f32, magnitudo 3> r ← -a",
    "sit vector<f32, magnitudo 3> r ← a ÷ s",
    "sit vector<f32, magnitudo 3> r ← a ÷ b",
    "sit vector<f32, magnitudo 3> r ← a ⤒ s",
    "sit vector<f32, magnitudo 3> r ← a ⤓ b",
    "sit vector<bivalens, magnitudo 3> r ← a ≺ b",
    "sit vector<bivalens, magnitudo 3> r ← a ≥ s",
    "sit vector<bivalens, magnitudo 3> r ← s ≻ a",
    "sit vector<bivalens, magnitudo 3> r ← a ≅ b",
    "sit vector<bivalens, magnitudo 3> r ← a ≇ s",
];

const LIFTED_MATRIX: &[&str] = &[
    "sit matrix<f32, [2, 2]> r ← m * s",
    "sit matrix<f32, [2, 2]> r ← s * m",
    "sit matrix<f32, [2, 2]> r ← m / s",
    "sit matrix<f32, [2, 2]> r ← s - m",
    "sit matrix<f32, [2, 2]> r ← m + 1.0",
    "sit matrix<f32, [2, 2]> r ← -m",
    "sit matrix<f32, [2, 2]> r ← m ÷ s",
    "sit matrix<f32, [2, 2]> r ← m ÷ n",
    "sit matrix<f32, [2, 2]> r ← m ⤒ s",
    "sit matrix<f32, [2, 2]> r ← m ⤓ n",
    "sit matrix<bivalens, [2, 2]> r ← m ≺ n",
    "sit matrix<bivalens, [2, 2]> r ← m ≥ s",
    "sit matrix<bivalens, [2, 2]> r ← s ≻ m",
    "sit matrix<bivalens, [2, 2]> r ← m ≅ n",
    "sit matrix<bivalens, [2, 2]> r ← m ≇ s",
];

/// The logic and select forms over a bool register mask, with the masks
/// taken as parameters so each form is the first one the gate meets.
const MASKED: &[(&str, &str)] = &[
    ("sit vector<bivalens, magnitudo 3> r ← k et k", VECTOR_ISSUE),
    ("sit vector<bivalens, magnitudo 3> r ← k aut verum", VECTOR_ISSUE),
    ("sit vector<bivalens, magnitudo 3> r ← non k", VECTOR_ISSUE),
    ("sit vector<f32, magnitudo 3> r ← k ✓ a ✗ b", VECTOR_ISSUE),
    ("sit matrix<bivalens, [2, 2]> r ← j et j", MATRIX_ISSUE),
    ("sit matrix<bivalens, [2, 2]> r ← j aut verum", MATRIX_ISSUE),
    ("sit matrix<bivalens, [2, 2]> r ← non j", MATRIX_ISSUE),
    ("sit matrix<f32, [2, 2]> r ← j ✓ m ✗ n", MATRIX_ISSUE),
];

fn masked_issues(target: Target, body: &str) -> (Vec<String>, bool) {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(target)
            .with_dev_locale_root(),
    );
    let source = format!(
        "functio masked(vector<bivalens, magnitudo 3> k, vector<f32, magnitudo 3> a, vector<f32, magnitudo 3> b, matrix<bivalens, [2, 2]> j, matrix<f32, [2, 2]> m, matrix<f32, [2, 2]> n) {{\n{body}\n}}\nincipit {{\n}}\n"
    );
    let result = crate::driver::compile(&session, "<test>", &source, ModuleCompileRequest::default());
    let issues = result
        .diagnostics
        .iter()
        .flat_map(|diagnostic| diagnostic.args.iter())
        .filter(|arg| arg.name == "issue")
        .map(|arg| arg.value.clone())
        .collect();
    (issues, result.output.is_some())
}

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_rejects_register_logic_and_select_by_name() {
    for (body, issue) in MASKED {
        let (found, _) = masked_issues(Target::HirTypeScript, body);
        assert!(found.iter().any(|found| found == issue), "`{body}`: {found:?}");
    }
}

#[cfg(feature = "hir-go")]
#[test]
fn go_rejects_register_logic_and_select_by_name() {
    for (body, issue) in MASKED {
        let (found, _) = masked_issues(Target::HirGo, body);
        assert!(found.iter().any(|found| found == issue), "`{body}`: {found:?}");
    }
}

/// The Rust emitter lifts the register forms itself, so a float or boolean
/// lane form reaches output with no lift issue.
#[cfg(feature = "hir-rust")]
#[test]
fn rust_emits_register_logic_and_select() {
    for (body, _) in MASKED {
        let (found, emitted) = masked_issues(Target::HirRust, body);
        assert!(emitted, "`{body}`: {found:?}");
        assert!(
            !found
                .iter()
                .any(|issue| issue == VECTOR_ISSUE || issue == MATRIX_ISSUE),
            "`{body}`: {found:?}"
        );
    }
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_python_and_haskell_never_emit_register_logic_and_select() {
    for target in [Target::HirSwift, Target::HirPython, Target::HirHaskell] {
        for (body, _) in MASKED {
            let (_, emitted) = masked_issues(target, body);
            assert!(!emitted, "{target:?} `{body}` emitted code");
        }
    }
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_reemission_keeps_register_logic_and_select() {
    for (body, _) in MASKED {
        let (found, _) = masked_issues(Target::HirFaber, body);
        assert!(
            !found
                .iter()
                .any(|issue| issue == VECTOR_ISSUE || issue == MATRIX_ISSUE),
            "`{body}`: {found:?}"
        );
    }
}

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
fn rust_emits_the_lifted_register_forms() {
    for body in LIFTED.iter().chain(LIFTED_MATRIX) {
        let found = issues(Target::HirRust, body);
        assert!(
            !found
                .iter()
                .any(|issue| issue == VECTOR_ISSUE || issue == MATRIX_ISSUE),
            "`{body}`: {found:?}"
        );
    }
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_python_and_haskell_never_emit_the_lifted_register_forms() {
    for target in [Target::HirSwift, Target::HirPython, Target::HirHaskell] {
        for body in LIFTED.iter().chain(LIFTED_MATRIX) {
            let session = Session::new(
                Config::default()
                    .with_latin_locale()
                    .with_target(target)
                    .with_dev_locale_root(),
            );
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
        "sit vector<f32, magnitudo 3> r ← a + b",
        "sit matrix<f32, [2, 2]> r ← m + n",
        "sit matrix<f32, [2, 2]> r ← m ⊙ n",
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
