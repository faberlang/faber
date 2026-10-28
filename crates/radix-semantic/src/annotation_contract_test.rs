//! Tests for annotation contract collection, registration, and validation.
//!
//! These tests prove that local and imported annotation contracts are
//! parsed, registered, applied, and that violations produce expected
//! diagnostics.

use crate::annotation_contract::{
    AnnotationAttachmentTarget, ContractFieldType, ContractPayloadValue, analyze,
};
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceAnnotationContract,
    InterfaceAnnotationContractField, InterfaceStructExport, InterfaceStructField,
    InterfaceTypeSnapshot,
};
use crate::test_support::args_have_no_legacy_message;
use crate::{PassConfig, Primitive, SemanticErrorKind, analyze as semantic_analyze};
use radix_hir::{HirAnnotationValue, HirItemKind};
use radix_lexer::lex;
use radix_parser::parse;
use radix_target::Target;
use std::collections::BTreeMap;

fn expect_issue(source: &str, issue: &str) {
    let lex_result = lex(source);
    let mut parsed = parse(lex_result);
    let program = parsed.program.expect("parse");
    let config = PassConfig::for_target(Target::HirRust);
    let result = semantic_analyze(&program, &config, &mut parsed.interner);
    let err = result
        .diagnostics
        .iter()
        .find(|err| {
            err.kind == SemanticErrorKind::AnnotationInvalid
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == issue)
        })
        .unwrap_or_else(|| panic!("expected issue {issue}, got {:?}", result.diagnostics));
    assert!(args_have_no_legacy_message(&err.args));
}

fn expect_definition_issues(source: &str, expected: &[&str]) {
    let lex_result = lex(source);
    let mut parsed = parse(lex_result);
    let program = parsed.program.expect("parse");
    let config = PassConfig::for_target(Target::HirRust);
    let result = semantic_analyze(&program, &config, &mut parsed.interner);
    let actual: Vec<_> = result
        .diagnostics
        .iter()
        .filter_map(|error| {
            error
                .args
                .iter()
                .find(|arg| arg.name == "issue")
                .map(|arg| arg.value.as_str())
        })
        .collect();
    assert_eq!(actual, expected, "diagnostics={:?}", result.diagnostics);
}

fn analyze_ok(source: &str) -> crate::SemanticResult {
    let lex_result = lex(source);
    let mut parsed = parse(lex_result);
    let program = parsed.program.expect("parse");
    let config = PassConfig::for_target(Target::HirRust);
    let result = semantic_analyze(&program, &config, &mut parsed.interner);
    assert!(
        result.is_ok(),
        "expected success, errors={:?}",
        result.diagnostics
    );
    result
}

#[test]
fn local_annotatio_marks_genus_as_contract() {
    let source = r#"
@ annotatio { target = functio }
genus Meta {
    varia textus selector
}

@ Meta { selector = "main" }
functio apply() → vacuum {
}
"#;
    let result = analyze_ok(source);
    let contracts: Vec<_> = result.annotation_contracts.registry.iter().collect();
    assert_eq!(contracts.len(), 1);
    assert_eq!(contracts[0].target, AnnotationAttachmentTarget::Functio);
    assert_eq!(contracts[0].fields.len(), 1);
    assert_eq!(contracts[0].fields[0].ty, ContractFieldType::Textus);
    assert!(!contracts[0].fields[0].optional);

    let apps: Vec<_> = result.annotation_contracts.applications().collect();
    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].contract_id, contracts[0].def_id);
    match &apps[0].fields[0].value {
        ContractPayloadValue::String(_) => {}
        other => panic!("expected string payload value, got {other:?}"),
    }

    let hir = result.hir.expect("hir");
    let ann = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Function(_) => item
                .annotations
                .iter()
                .find(|ann| ann.contract_id.is_some()),
            _ => None,
        })
        .expect("resolved contract annotation on function");
    assert_eq!(ann.contract_id, Some(contracts[0].def_id));
    assert_eq!(ann.fields.len(), 1);
    assert!(matches!(ann.fields[0].value, HirAnnotationValue::String(_)));
}

#[test]
fn bare_annotatio_defaults_to_functio_target() {
    let source = r#"
@ annotatio
genus Tag {
    varia textus name
}

@ Tag { name = "x" }
functio f() → vacuum {
}
"#;
    let result = analyze_ok(source);
    let contract = result
        .annotation_contracts
        .registry
        .iter()
        .next()
        .expect("contract");
    assert_eq!(contract.target, AnnotationAttachmentTarget::Functio);
}

#[test]
fn rejects_annotatio_on_function() {
    expect_issue(
        r"
@ annotatio
functio bad() → vacuum {
}
",
        "annotatio_misplaced",
    );
}

#[test]
fn rejects_duplicate_annotatio_marker() {
    expect_issue(
        r"
@ annotatio
@ annotatio
genus Meta {
    varia textus selector
}
",
        "annotatio_duplicate_marker",
    );
}

#[test]
fn rejects_unknown_contract_application() {
    expect_issue(
        r#"
@ MissingContract { selector = "x" }
functio f() → vacuum {
}
"#,
        "annotatio_unknown_contract",
    );
}

#[test]
fn rejects_non_contract_genus_as_annotation() {
    expect_issue(
        r#"
genus Plain {
    varia textus name
}

@ Plain { name = "x" }
functio f() → vacuum {
}
"#,
        "annotatio_not_a_contract",
    );
}

#[test]
fn rejects_unknown_payload_field() {
    expect_issue(
        r#"
@ annotatio
genus Meta {
    varia textus selector
}

@ Meta { selector = "x", extra = "y" }
functio f() → vacuum {
}
"#,
        "annotatio_unknown_field",
    );
}

#[test]
fn rejects_missing_required_field() {
    expect_issue(
        r"
@ annotatio
genus Meta {
    varia textus selector
}

@ Meta { }
functio f() → vacuum {
}
",
        "annotatio_missing_field",
    );
}

#[test]
fn rejects_type_mismatched_payload() {
    expect_issue(
        r"
@ annotatio
genus Meta {
    varia textus selector
}

@ Meta { selector = 12 }
functio f() → vacuum {
}
",
        "annotatio_invalid_payload",
    );
}

#[test]
fn rejects_duplicate_payload_field() {
    expect_issue(
        r#"
@ annotatio
genus Meta {
    varia textus selector
}

@ Meta { selector = "a", selector = "b" }
functio f() → vacuum {
}
"#,
        "annotatio_duplicate_field",
    );
}

#[test]
fn rejects_contract_on_genus_attachment() {
    expect_issue(
        r#"
@ annotatio
genus Meta {
    varia textus selector
}

@ Meta { selector = "x" }
genus Other {
}
"#,
        "annotatio_invalid_attachment",
    );
}

#[test]
fn optional_field_allows_omission() {
    let source = r"
@ annotatio
genus Meta {
    varia textus selector sponte
}

@ Meta { }
functio f() → vacuum {
}
";
    let _ = analyze_ok(source);
}

#[test]
fn contract_with_no_fields_registers_and_applies() {
    let source = r"
@ annotatio
genus EmptyTag {
}

@ EmptyTag { }
functio f() → vacuum {
}
";
    let result = analyze_ok(source);
    let contract = result
        .annotation_contracts
        .registry
        .iter()
        .next()
        .expect("contract");
    assert!(
        contract.fields.is_empty(),
        "no-fields contract should have zero fields"
    );
}

#[test]
fn imported_contract_resolves_application() {
    let source = r#"
importa ex "./lib" lib

@ Meta { selector = "[data-x]" }
functio apply() → vacuum {
}
"#;
    let lex_result = lex(source);
    let mut parsed = parse(lex_result);
    let program = parsed.program.expect("parse");
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Meta".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            params: Vec::new(),
            name: "Meta".to_owned(),
            fields: vec![InterfaceStructField {
                name: "selector".to_owned(),
                ty: InterfaceTypeSnapshot::Primitive(Primitive::Textus),
                optional: false,
                required: true,
                default: None,
                modifier: None,
                visibility: radix_syntax::MemberVisibility::Publica,
            }],
            methods: Vec::new(),
            annotation_contract: Some(InterfaceAnnotationContract {
                target: "functio".to_owned(),
                fields: vec![InterfaceAnnotationContractField {
                    name: "selector".to_owned(),
                    ty: "textus".to_owned(),
                    optional: false,
                }],
                qualified_identity: Some(crate::file_interface::InterfaceQualifiedIdentity {
                    provider: "package".to_owned(),
                    package: Some("demo".to_owned()),
                    module_path: vec!["lib".to_owned()],
                    export_name: "Meta".to_owned(),
                }),
            }),
        }),
    });
    let mut config = PassConfig::for_target(Target::HirRust);
    config.namespace_exports.insert(
        "lib".to_owned(),
        BTreeMap::from([(
            "Meta".to_owned(),
            crate::passes::visibility_annotation::VisibilityTier::Publica,
        )]),
    );
    config.file_interfaces.insert("lib".to_owned(), interface);

    let result = semantic_analyze(&program, &config, &mut parsed.interner);
    assert!(
        result.is_ok(),
        "imported contract should resolve, errors={:?}",
        result.diagnostics
    );
    let meta_name = parsed.interner.intern("Meta");
    assert!(
        result
            .annotation_contracts
            .registry
            .lookup_name(meta_name)
            .is_some()
    );
    let apps: Vec<_> = result.annotation_contracts.applications().collect();
    assert_eq!(apps.len(), 1);
}

#[test]
fn commune_field_uniqueness_diagnostics_keep_issue_args() {
    expect_definition_issues(
        r"discretio Eventum {
    @ commune
    textus message
    @ commune
    textus message
}
",
        &["duplicate_commune_field"],
    );
    let parsed = parse(lex(r"discretio Eventum {
    @ commune
    textus message

    Variant { textus message }
}
"));
    assert!(parsed.errors.iter().any(|error| {
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "union_variant_redeclares_shared_field")
    }));
}

#[test]
fn local_analyze_helper_collects_contract() {
    let source = r"
@ annotatio
genus Meta {
    varia numerus n
}
";
    let lex_result = lex(source);
    let parsed = parse(lex_result);
    let program = parsed.program.expect("parse");
    let mut types = crate::TypeTable::new();
    let mut resolver = crate::Resolver::new();
    crate::passes::collect::collect(&program, &mut resolver, &mut types).expect("collect");
    let meta = analyze(&program, &resolver, &parsed.interner).expect("contracts");
    assert_eq!(meta.registry.iter().count(), 1);
}

#[test]
fn rejects_retired_doc_family_bare() {
    // D7.5: `doc` was a dead compiler-owned family (no parser, lowering or
    // consumer); it now fails closed like any unregistered contract.
    expect_issue(
        r"
@ doc
functio documented() → vacuum {
}
",
        "annotatio_unknown_contract",
    );
}

#[test]
fn rejects_retired_doc_family_braced() {
    expect_issue(
        r#"
@ doc { textus = "prose" }
functio documented() → vacuum {
}
"#,
        "annotatio_unknown_contract",
    );
}
