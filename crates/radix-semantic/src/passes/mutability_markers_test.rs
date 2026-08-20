//! MU-1 marker parse, type identity, mixed-spelling, and scalar rejection.

use crate::test_support::{
    analyze_ok, analyze_source_with_config, assert_rejects_with_issue, entry_local_tys,
};
use crate::{locale::LocalePack, PassConfig, SemanticMutability, Type, ISSUE_SCALAR_ACCESS_MARKER};
use radix_target::Target;

fn english_pack() -> LocalePack {
    LocalePack::from_toml_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/en/pack.toml"),
    )
    .expect("load English locale pack")
}

#[test]
fn own_and_copy_markers_carry_type_table_facts() {
    let result = analyze_ok(
        r"
incipit {
    fixum own lista<numerus> owned
    fixum copy lista<numerus> copied
    fixum de lista<numerus> shared
    varia in lista<numerus> exclusive
}
",
    );
    let tys = entry_local_tys(&result);
    assert_eq!(tys.len(), 4);
    assert!(matches!(
        result.types.get(tys[0]),
        Type::Ref(SemanticMutability::Own, _)
    ));
    assert!(matches!(
        result.types.get(tys[1]),
        Type::Ref(SemanticMutability::Copy, _)
    ));
    assert!(matches!(
        result.types.get(tys[2]),
        Type::Ref(SemanticMutability::Immutable, _)
    ));
    assert!(matches!(
        result.types.get(tys[3]),
        Type::Ref(SemanticMutability::Mutable, _)
    ));
}

#[test]
fn own_and_copy_remain_identifier_bindings() {
    analyze_ok(
        r"
incipit {
    fixum numerus own ← 1
    fixum numerus copy ← 2
    nota own
    nota copy
}
",
    );
}

#[test]
fn mut_list_locals_still_parse() {
    // OQ-3: grammar allows mut locals. MU-4's WARN019 is non-fatal.
    analyze_ok(
        r"
incipit {
    varia in lista<numerus> xs
}
",
    );
}

#[test]
fn scalar_access_marker_rejects_at_the_marker() {
    assert_rejects_with_issue(
        "incipit { fixum de numerus x ← 5 }",
        ISSUE_SCALAR_ACCESS_MARKER,
    );
    assert_rejects_with_issue(
        "incipit { varia in numerus x ← 5 }",
        ISSUE_SCALAR_ACCESS_MARKER,
    );
    assert_rejects_with_issue(
        "incipit { fixum own numerus x ← 5 }",
        ISSUE_SCALAR_ACCESS_MARKER,
    );
    assert_rejects_with_issue(
        "incipit { fixum copy bivalens flag ← verum }",
        ISSUE_SCALAR_ACCESS_MARKER,
    );
}

#[test]
fn mixed_spelling_fixum_ref_numerus_must_reject() {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(english_pack());
    let result = analyze_source_with_config("fixum ref numerus a", config);
    assert!(
        !result.is_ok(),
        "mixed-spelling pack discipline must reject, got {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|diag| {
            crate::semantic_spec(diag.kind).code == "SEM008"
                || diag.issue() == Some("undefined_name")
        }),
        "expected emergent SEM008, got {:?}",
        result.diagnostics
    );
}
