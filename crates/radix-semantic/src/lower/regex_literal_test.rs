//! Inline regex literals lower to `HirLiteral::Regex` (ruling d381ae56):
//! `regex /pattern/` (the locale's regex type word) and `_ /pattern/` carry
//! the raw pattern body with no suffix flags.

use crate::test_support;
use radix_hir::{HirExpression, HirExpressionKind, HirLiteral, HirStatementKind};

fn entry_init(unit: &test_support::AnalyzedFixture) -> &HirExpression {
    let hir = unit.hir.as_ref().expect("hir");
    let entry = hir.entry.as_ref().expect("entry");
    entry
        .statements
        .iter()
        .find_map(|stmt| match &stmt.kind {
            HirStatementKind::Local(local) => local.init.as_ref(),
            _ => None,
        })
        .expect("entry local initializer")
}

fn assert_regex_init(unit: &test_support::AnalyzedFixture, expected: &str) {
    let init = entry_init(unit);
    let HirExpressionKind::Literal(HirLiteral::Regex(pattern, flags)) = &init.kind else {
        panic!("expected HIR regex literal, got {:?}", init.kind);
    };
    assert_eq!(unit.interner.resolve(*pattern), expected);
    assert_eq!(*flags, None, "no suffix flags: inline (?i) only");
}

#[test]
fn regex_type_word_literal_lowers_to_hir_regex() {
    let unit = test_support::analyze_ok(r"incipit { fixum regex r ← regex /\d+/ }");
    assert_regex_init(&unit, r"\d+");
}

#[test]
fn inferred_regex_literal_lowers_to_hir_regex() {
    let unit = test_support::analyze_ok(r"incipit { fixum regex r ← _ /a/ }");
    assert_regex_init(&unit, "a");
}

#[test]
fn repro_nota_hole_accepts_today() {
    let unit = test_support::analyze_source(r"incipit { nota _ /a/ }");
    panic!(
        "repro: success={:?} errs={:?}",
        unit.is_ok(),
        unit.err().map(|e| e.len())
    );
}
