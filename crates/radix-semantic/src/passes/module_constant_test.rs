//! A top-level `fixum T X = e` is the module constant (st1 R1/R3).
//!
//! Contract:
//! - it lowers, types, and folds exactly as the `generis` static does: the
//!   twin sources below keep every byte offset equal (the `fixum` spelling is
//!   padded to the width of `generis`), so the analyzed HIR is equal whole;
//! - both spellings coexist in one module and read each other;
//! - `praefixum` and `insere` initializers are admitted exactly as on a static;
//! - `_` infers the constant's type, as it does for locals;
//! - a runtime store (`←`) at top level stays SEM062 `top_level_binding`;
//! - an `@ publica` constant analyzes like the static (the export itself is
//!   pinned in `radix-program`'s `file_interface_const_test`);
//! - a block-level `fixum … =` stays a local, never a module constant.

use crate::PassConfig;
use crate::insere::{InsereFiles, InsereSource};
use crate::test_support::{analyze_source, analyze_source_with_config, assert_rejects_with_issue};
use radix_hir::{HirConst, HirExpressionKind, HirItemKind, HirLiteral};
use radix_target::Target;

/// Analyze `source`, assert success, and return the named module constant.
fn constant(source: &str, name: &str) -> HirConst {
    let fixture = analyze_source(source);
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    hir.items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if fixture.interner.resolve(konst.name) == name => {
                Some(konst.clone())
            }
            _ => None,
        })
        .expect("constant item")
}

/// Analyze `source` and render every analyzed item, spans included.
fn items_debug(source: &str) -> String {
    let fixture = analyze_source(source);
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    format!("{:?}", hir.items)
}

/// The `fixum` twin of a `generis` source with every byte offset preserved.
fn fixum_twin(source: &str) -> String {
    source.replace("generis ", "fixum   ")
}

fn assert_twin_equal(source: &str) {
    assert_eq!(
        items_debug(source),
        items_debug(&fixum_twin(source)),
        "{source}"
    );
}

fn int_value(konst: &HirConst) -> u64 {
    match &konst.value.kind {
        HirExpressionKind::Literal(HirLiteral::Int(v)) => *v,
        other => panic!("expected folded int literal, got {other:?}"),
    }
}

#[test]
fn fixum_constant_carries_the_statica_marker() {
    let konst = constant("fixum numerus LIMES = 4 * 1024\n", "LIMES");
    let statica = konst
        .statica
        .as_ref()
        .expect("a top-level `=` constant is a module constant");
    assert!(
        statica.authored.is_some(),
        "the authored initializer is kept for re-emission"
    );
    assert_eq!(int_value(&konst), 4096);
}

#[test]
fn fixum_constant_equals_the_generis_static_in_hir() {
    assert_twin_equal("generis numerus LIMES = 4 * 1024\n");
    assert_twin_equal("generis numerus N = 7\n");
    assert_twin_equal("generis textus NOMEN = \"ab\" + \"cd\"\n");
    assert_twin_equal("generis bivalens F = 4 > 3\n");
    assert_twin_equal("generis lista<numerus> XS = [1, 2, 3]\n");
    assert_twin_equal("generis numerus A = 3\ngeneris numerus B = A * 2 + 1\n");
    assert_twin_equal("generis numerus<u8> A = 200\ngeneris numerus<u8> B = A + 50\n");
}

#[test]
fn both_spellings_share_one_module_and_read_each_other() {
    let source = "generis numerus A = 3\nfixum numerus B = A + 1\ngeneris numerus C = B * 2\n";
    assert_eq!(int_value(&constant(source, "B")), 4);
    assert_eq!(int_value(&constant(source, "C")), 8);
}

#[test]
fn fixum_constant_initializer_faults_keep_their_identity() {
    assert_rejects_with_issue(
        "fixum numerus<u8> A = 200\nfixum numerus<u8> B = A + 100\n",
        "static_arithmetic_overflow",
    );
    assert_rejects_with_issue("fixum numerus D = 1 / 0\n", "static_division_by_zero");
    assert_rejects_with_issue(
        "functio f() → numerus { redde 1 }\nfixum numerus W = f()\n",
        "static_initializer_not_constant",
    );
    assert_rejects_with_issue(
        "fixum numerus A = B + 1\nfixum numerus B = A + 1\n",
        "static_cycle",
    );
}

#[test]
fn underscore_infers_the_constant_type() {
    let konst = constant("fixum _ N = 10\nfixum _ M = N * 2\n", "M");
    assert!(konst.statica.is_some());
    assert_eq!(int_value(&konst), 20);
}

#[test]
fn underscore_infers_text_and_bool() {
    let source = "fixum _ NOMEN = \"ab\" + \"cd\"\nfixum _ F = 3 > 2\n";
    assert!(matches!(
        constant(source, "NOMEN").value.kind,
        HirExpressionKind::Literal(HirLiteral::String(_))
    ));
    assert!(matches!(
        constant(source, "F").value.kind,
        HirExpressionKind::Literal(HirLiteral::Bool(true))
    ));
}

#[test]
fn a_runtime_store_at_top_level_stays_top_level_binding() {
    assert_rejects_with_issue("fixum numerus N ← 4\n", "top_level_binding");
    assert_rejects_with_issue("fixum _ N ← 4\n", "top_level_binding");
    assert_rejects_with_issue("varia numerus N ← 4\n", "top_level_binding");
}

#[test]
fn varia_with_equals_stays_a_named_parse_error() {
    assert_rejects_with_issue("varia numerus N = 4\n", "varia_compile_time_initializer");
}

#[test]
fn publica_annotated_fixum_constant_analyzes_like_the_static() {
    let source = "@ publica\ngeneris numerus N = 4\n";
    let fixture_static = analyze_source(source);
    let fixture_const = analyze_source(&fixum_twin(source));
    assert!(
        fixture_const.is_ok(),
        "diagnostics: {:?}",
        fixture_const.diagnostics
    );
    let annotations = |fixture: &crate::test_support::AnalyzedFixture| {
        let hir = fixture.result.hir.as_ref().expect("hir");
        format!(
            "{:?}",
            hir.items
                .iter()
                .map(|item| &item.annotations)
                .collect::<Vec<_>>()
        )
    };
    assert_eq!(annotations(&fixture_static), annotations(&fixture_const));
}

#[test]
fn block_level_fixum_equals_stays_a_local() {
    let fixture = analyze_source("incipit {\n    fixum numerus N = 3\n    nota N\n}\n");
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    assert!(
        hir.items
            .iter()
            .all(|item| !matches!(&item.kind, HirItemKind::Constant(_))),
        "a block-level `=` binding is not a module constant"
    );
}

#[test]
fn praefixum_initializer_is_admitted_on_a_fixum_constant() {
    assert_twin_equal("generis numerus Z = praefixum {\n    redde 3 + 4\n}\n");
}

fn embed_config() -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    let mut files = InsereFiles::default();
    files.insert("nuntius.txt", InsereSource::Bytes(b"salve".to_vec()));
    files.insert("signum.bin", InsereSource::Bytes(vec![1, 2, 3]));
    config.insere_files = files;
    config
}

fn items_debug_with(source: &str, config: PassConfig) -> String {
    let fixture = analyze_source_with_config(source, config);
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    format!("{:?}", fixture.result.hir.as_ref().expect("hir").items)
}

#[test]
fn insere_initializer_is_admitted_on_a_fixum_constant() {
    for source in [
        "generis textus S = insere \"nuntius.txt\"\n",
        "generis octeti B = insere \"signum.bin\"\n",
    ] {
        assert_eq!(
            items_debug_with(source, embed_config()),
            items_debug_with(&fixum_twin(source), embed_config()),
            "{source}"
        );
    }
}

#[test]
fn insere_on_a_fixum_constant_keeps_the_declared_type_rule() {
    let fixture =
        analyze_source_with_config("fixum numerus N = insere \"nuntius.txt\"\n", embed_config());
    assert!(
        fixture.diagnostics.iter().any(|err| {
            err.kind == crate::SemanticErrorKind::InsereInvalid
                && crate::test_support::issue(err) == Some("insere_type_invalid")
        }),
        "{:?}",
        fixture.diagnostics
    );
}
