//! An imported generic enum substitutes its type parameters into variant
//! payload fields at construction, exactly like a local `discretio Cista<T>`
//! (polish finding A-201): the consumer needs the exporter's declared
//! parameter names to key the witness substitution.

use std::collections::BTreeMap;

use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceEnumVariant, InterfaceGenericParam,
    InterfaceNominalExport, InterfaceStructField, InterfaceTypeSnapshot,
};

/// Producer-shaped interface: `lib` exports `discretio Cista<T> { Plena { T res } }`.
fn generic_union_interface() -> FileInterface {
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Cista".to_owned(),
        kind: FileExportKind::Enum(InterfaceNominalExport {
            params: vec![InterfaceGenericParam {
                name: "T".to_owned(),
                kind: radix_syntax::GenericParamKind::Typus,
            }],
            name: "Cista".to_owned(),
            methods: Vec::new(),
            shared_fields: Vec::new(),
            radix_contract: None,
            variants: vec![InterfaceEnumVariant {
                name: "Plena".to_owned(),
                fields: vec![InterfaceStructField {
                    name: "res".to_owned(),
                    ty: InterfaceTypeSnapshot::Param("T".to_owned()),
                    optional: false,
                    required: true,
                    default: None,
                    modifier: None,
                    visibility: radix_syntax::MemberVisibility::Publica,
                }],
            }],
        }),
    });
    interface
}

fn analyze_consumer(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_with_import_contract(
        source,
        BTreeMap::from([(
            "lib".to_owned(),
            BTreeMap::from([(
                "Cista".to_owned(),
                crate::passes::visibility_annotation::VisibilityTier::Publica,
            )]),
        )]),
        BTreeMap::from([("lib".to_owned(), generic_union_interface())]),
    )
}

fn error_issues(unit: &crate::test_support::AnalyzedFixture) -> Vec<Option<&str>> {
    unit.diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(crate::test_support::issue)
        .collect()
}

#[test]
fn imported_generic_enum_variant_construction_substitutes_applied_witness() {
    // `T ↦ numerus` must reach the `res` payload: the `1` literal checks
    // against the substituted field type, not the rigid parameter `T`.
    let unit = analyze_consumer(
        r#"importa ex "./lib" lib

functio facere() → lib.Cista<numerus> {
    redde finge lib.Plena { res = 1 } ∷ lib.Cista<numerus>
}"#,
    );
    let issues = error_issues(&unit);
    assert!(
        issues.is_empty(),
        "an applied imported generic variant must construct, got: {issues:?}"
    );
}

#[test]
fn imported_generic_enum_variant_construction_rejects_mismatched_witness() {
    // The substitution is real: a `textus` payload cannot flow into `T ↦ numerus`.
    let unit = analyze_consumer(
        r#"importa ex "./lib" lib

functio facere() → lib.Cista<numerus> {
    redde finge lib.Plena { res = "x" } ∷ lib.Cista<numerus>
}"#,
    );
    let issues = error_issues(&unit);
    assert!(
        issues.contains(&Some("argument_type_mismatch")),
        "a mismatched payload witness must be refused at the field, got: {issues:?}"
    );
}
