//! A top-level `fixum T X = praefixum { … }` is evaluated at build time (st1
//! R1/R2).
//!
//! Contract: the body runs on the MIR runner, the literal replaces the
//! initializer, constants that read it fold after it, and a `_` type slot
//! takes its type from the body's `redde`.
#![cfg(feature = "runner")]

use crate::driver::{AnalyzeRequest, AnalyzedModule, Config, Session, analyze_source};
use crate::hir::{HirExpressionKind, HirItemKind, HirLiteral};

fn analyze_ok(source: &str) -> AnalyzedModule {
    let session = Session::new(Config::default().with_latin_locale().with_dev_locale_root());
    analyze_source(&session, "module_constant.fab", source, AnalyzeRequest::default())
        .unwrap_or_else(|diagnostics| panic!("analysis failed: {diagnostics:?}"))
}

fn int_of(unit: &AnalyzedModule, name: &str) -> u64 {
    unit.hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if unit.interner.resolve(konst.name) == name => match &konst.value.kind {
                HirExpressionKind::Literal(HirLiteral::Int(v)) => Some(*v),
                other => panic!("expected int literal for {name}, got {other:?}"),
            },
            _ => None,
        })
        .unwrap_or_else(|| panic!("constant {name} not found"))
}

const HELPER: &str = "functio septem() → numerus {\n    redde 7\n}\n";

#[test]
fn fixum_praefixum_evaluates_at_build_time() {
    let unit = analyze_ok(&format!(
        "{HELPER}fixum numerus Z = praefixum {{\n    redde septem() + 1\n}}\nincipit {{\n    nota Z\n}}\n"
    ));
    assert_eq!(int_of(&unit, "Z"), 8);
}

#[test]
fn underscore_takes_its_type_from_the_body() {
    let unit = analyze_ok(&format!(
        "{HELPER}fixum _ Z = praefixum {{\n    redde septem() + 1\n}}\nincipit {{\n    nota Z\n}}\n"
    ));
    assert_eq!(int_of(&unit, "Z"), 8);
}

#[test]
fn dependent_constants_fold_after_the_evaluated_constant() {
    let unit = analyze_ok(&format!(
        "{HELPER}fixum numerus Z = praefixum {{\n    redde septem()\n}}\nfixum numerus D = Z * 2\nfixum numerus E = D + Z\nincipit {{\n    nota E\n}}\n"
    ));
    assert_eq!(int_of(&unit, "D"), 14);
    assert_eq!(int_of(&unit, "E"), 21);
}
