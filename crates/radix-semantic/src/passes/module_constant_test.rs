//! A top-level `fixum T X = e` is the module constant (st1 R1/R3).
//!
//! Contract:
//! - it lowers, types, and folds to its compile-time value and carries the
//!   `statica` marker;
//! - constants read each other in dependency order;
//! - `praefixum` and `insere` initializers are admitted;
//! - `_` infers the constant's type, as it does for locals;
//! - a runtime store (`←`) at top level stays SEM062 `top_level_binding`;
//! - an `@ publica` constant keeps its annotation (the export itself is
//!   pinned in `radix-program`'s `file_interface_const_test`);
//! - a block-level `fixum … =` stays a local, never a module constant.

use crate::PassConfig;
use crate::insere::{InsereFiles, InsereSource};
use crate::test_support::{
    analyze_source_la, analyze_source_with_config_la, assert_rejects_with_issue_la,
};
use radix_hir::{HirConst, HirExpressionKind, HirItemKind, HirLiteral};
use radix_target::Target;

/// Analyze `source`, assert success, and return the named module constant.
fn constant(source: &str, name: &str) -> HirConst {
    constant_with(source, name, PassConfig::for_target(Target::HirRust))
}

fn constant_with(source: &str, name: &str, config: PassConfig) -> HirConst {
    let fixture = analyze_source_with_config_la(source, config);
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
fn fixum_constants_fold_to_their_values() {
    assert_eq!(int_value(&constant("fixum numerus N = 7\n", "N")), 7);
    let text = constant("fixum textus NOMEN = \"ab\" + \"cd\"\n", "NOMEN");
    assert!(
        matches!(
            text.value.kind,
            HirExpressionKind::Literal(HirLiteral::String(_))
        ),
        "{:?}",
        text.value.kind
    );
    let flag = constant("fixum bivalens F = 4 ≻ 3\n", "F");
    assert!(
        matches!(
            flag.value.kind,
            HirExpressionKind::Literal(HirLiteral::Bool(true))
        ),
        "{:?}",
        flag.value.kind
    );
    let list = constant("fixum lista<numerus> XS = [1, 2, 3]\n", "XS");
    assert!(list.statica.is_some());
    let narrow = constant("fixum u8 A = 200\nfixum u8 B = A + 50\n", "B");
    assert_eq!(int_value(&narrow), 250);
}

#[test]
fn constants_read_each_other_in_dependency_order() {
    let source = "fixum numerus C = B * 2\nfixum numerus B = A + 1\nfixum numerus A = 3\n";
    assert_eq!(int_value(&constant(source, "B")), 4);
    assert_eq!(int_value(&constant(source, "C")), 8);
}

#[test]
fn fixum_constant_initializer_faults_keep_their_identity() {
    assert_rejects_with_issue_la(
        "fixum u8 A = 200\nfixum u8 B = A + 100\n",
        "constant_arithmetic_overflow",
    );
    assert_rejects_with_issue_la("fixum numerus D = 1 / 0\n", "constant_division_by_zero");
    assert_rejects_with_issue_la(
        "functio f() → numerus { redde 1 }\nfixum numerus W = f()\n",
        "constant_initializer_not_constant",
    );
    assert_rejects_with_issue_la(
        "fixum numerus A = B + 1\nfixum numerus B = A + 1\n",
        "constant_cycle",
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
    let source = "fixum _ NOMEN = \"ab\" + \"cd\"\nfixum _ F = 3 ≻ 2\n";
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
    assert_rejects_with_issue_la("fixum numerus N ← 4\n", "top_level_binding");
    assert_rejects_with_issue_la("fixum _ N ← 4\n", "top_level_binding");
    assert_rejects_with_issue_la("varia numerus N ← 4\n", "top_level_binding");
}

#[test]
fn varia_with_equals_stays_a_named_parse_error() {
    assert_rejects_with_issue_la("varia numerus N = 4\n", "varia_compile_time_initializer");
}

#[test]
fn publica_annotated_fixum_constant_keeps_its_annotation() {
    let fixture = analyze_source_la("@ publica\nfixum numerus N = 4\n");
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    let item = hir
        .items
        .iter()
        .find(|item| matches!(&item.kind, HirItemKind::Constant(_)))
        .expect("constant item");
    assert_eq!(item.annotations.len(), 1, "{:?}", item.annotations);
}

#[test]
fn block_level_fixum_equals_stays_a_local() {
    let fixture = analyze_source_la("incipit {\n    fixum numerus N = 3\n    nota N\n}\n");
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
    // The driver evaluates the body after analysis; analysis keeps it.
    let konst = constant("fixum numerus Z = praefixum {\n    redde 3 + 4\n}\n", "Z");
    assert!(konst.statica.is_some());
    assert!(
        matches!(konst.value.kind, HirExpressionKind::Praefixum(_)),
        "{:?}",
        konst.value.kind
    );
}

fn embed_config() -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    let mut files = InsereFiles::default();
    files.insert("nuntius.txt", InsereSource::Bytes(b"salve".to_vec()));
    files.insert("signum.bin", InsereSource::Bytes(vec![1, 2, 3]));
    config.insere_files = files;
    config
}

#[test]
fn insere_initializer_is_admitted_on_a_fixum_constant() {
    let text = constant_with(
        "fixum textus S = insere \"nuntius.txt\"\n",
        "S",
        embed_config(),
    );
    assert!(
        matches!(
            text.value.kind,
            HirExpressionKind::Literal(HirLiteral::String(_))
        ),
        "{:?}",
        text.value.kind
    );
    let bytes = constant_with(
        "fixum octeti B = insere \"signum.bin\"\n",
        "B",
        embed_config(),
    );
    assert!(
        matches!(
            bytes.value.kind,
            HirExpressionKind::Literal(HirLiteral::Octeti(_))
        ),
        "{:?}",
        bytes.value.kind
    );
}

#[test]
fn insere_on_a_fixum_constant_keeps_the_declared_type_rule() {
    let fixture =
        analyze_source_with_config_la("fixum numerus N = insere \"nuntius.txt\"\n", embed_config());
    assert!(
        fixture.diagnostics.iter().any(|err| {
            err.kind == crate::SemanticErrorKind::InsereInvalid
                && crate::test_support::issue(err) == Some("insere_type_invalid")
        }),
        "{:?}",
        fixture.diagnostics
    );
}
