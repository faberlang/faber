//! A top-level `fixum T X = praefixum { … }` is evaluated at build time exactly
//! as the `generis` static is (st1 R1/R2).
//!
//! Contract: the body runs on the MIR runner, the literal replaces the
//! initializer, statics of either spelling that read it fold after it, and a
//! `_` type slot takes its type from the body's `redde`.
#![cfg(feature = "runner")]

use crate::driver::{AnalyzeRequest, AnalyzedModule, Config, Session, analyze_source};
use crate::hir::{HirExpressionKind, HirItemKind, HirLiteral};

fn analyze_ok(source: &str) -> AnalyzedModule {
    let session = Session::new(Config::default().with_dev_locale_root());
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
fn fixum_praefixum_evaluates_like_the_static() {
    for spelling in ["generis", "fixum"] {
        let unit = analyze_ok(&format!(
            "{HELPER}{spelling} numerus Z = praefixum {{\n    redde septem() + 1\n}}\nincipit {{\n    nota Z\n}}\n"
        ));
        assert_eq!(int_of(&unit, "Z"), 8, "{spelling}");
    }
}

#[test]
fn underscore_takes_its_type_from_the_body() {
    let unit = analyze_ok(&format!(
        "{HELPER}fixum _ Z = praefixum {{\n    redde septem() + 1\n}}\nincipit {{\n    nota Z\n}}\n"
    ));
    assert_eq!(int_of(&unit, "Z"), 8);
}

#[test]
fn mixed_spellings_fold_after_the_evaluated_constant() {
    let unit = analyze_ok(&format!(
        "{HELPER}fixum numerus Z = praefixum {{\n    redde septem()\n}}\ngeneris numerus D = Z * 2\nfixum numerus E = D + Z\nincipit {{\n    nota E\n}}\n"
    ));
    assert_eq!(int_of(&unit, "D"), 14);
    assert_eq!(int_of(&unit, "E"), 21);
}
