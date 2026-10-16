//! d11-6 U5: the typecheck constant fold (`const_int_value`) and the runner
//! compute the same exact value for constant integer expressions, so a store
//! the fold says fits is never a store the runner disagrees with.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::hir::HirStatementKind;
use crate::mir::{BufferHost, RunSourceError, run_source};
use crate::semantic::passes::const_int_value;

fn session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    )
}

fn program(expr: &str) -> String {
    format!("incipit {{\n    sit _ d ← {expr}\n    nota d\n}}\n")
}

fn folded(expr: &str) -> Option<i128> {
    let module = analyze_source(&session(), "fold.fab", &program(expr), AnalyzeRequest::default())
        .unwrap_or_else(|d| panic!("{expr}: {d:?}"));
    let entry = module.hir.entry.as_ref().expect("incipit");
    entry.statements.iter().find_map(|stmt| match &stmt.kind {
        HirStatementKind::Local(local) => local.init.as_ref().and_then(const_int_value),
        _ => None,
    })
}

/// The runner prints the expression itself: an expression with no slot never
/// traps for size (ruling 2), so only the 64-bit range can stop it.
fn ran(expr: &str) -> Result<String, ()> {
    let mut host = BufferHost::default();
    let source = format!("incipit {{\n    nota 0 + ({expr})\n}}\n");
    match run_source(&session(), "run.fab", &source, &mut host) {
        Ok(()) => Ok(host.stdout_lines.join(",")),
        Err(RunSourceError::Stepper(_)) => Err(()),
        Err(other) => panic!("{expr}: {other:?}"),
    }
}

const SPREAD: &[&str] = &[
    "1 ⇐ 63",
    "(-7) / 2 + 5",
    "-7 % 2",
    "-7 / 2",
    "7 / -2",
    "7 % -2",
    "-7 % -2",
    "3 + 4 * 5 - 6",
    "9223372036854775807 + 1",
    "0 - 9223372036854775807 - 1",
    "1 ⇐ 62",
    "255 ⇐ 8",
    "-5 ⇒ 1",
    "5 ⇒ 1",
    "-1 ⇒ 100",
    "¬5",
    "¬(-6)",
    "¬0",
    "-1 ∧ 255",
    "-256 ∨ 255",
    "-1 ⊻ 5",
    "12 ∧ 10",
    "12 ∨ 10",
    "12 ⊻ 10",
    "(-3) * (-4)",
    "100 / 7 * 7 + 100 % 7",
    "(-100) / 7 * 7 + (-100) % 7",
];

#[test]
fn fold_and_runner_agree_on_constant_expressions() {
    for expr in SPREAD {
        if let Some(value) = folded(expr) {
            assert_eq!(ran(expr), Ok(value.to_string()), "`{expr}` folds to {value}");
        }
    }
}

#[test]
fn the_audited_expressions_fold() {
    // Reproduced by the G1 audit: the fold and the runner used to disagree.
    for (expr, expected) in [("1 ⇐ 63", 1_i128 << 63), ("(-7) / 2 + 5", 1), ("-7 % 2", 1)] {
        assert_eq!(folded(expr), Some(expected), "{expr}");
        assert_eq!(ran(expr), Ok(expected.to_string()), "{expr}");
    }
}
