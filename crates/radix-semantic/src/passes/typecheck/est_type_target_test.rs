//! `est` / `non est` type-target resolution (DECISIONS D23.1, superseding D18.1).
//!
//! The right-hand side is always a type (D23.1/D23.5), so an imported type —
//! qualified (`aux.Felis`) or selectively imported (`Canis`) — is a legal
//! target. A right-hand side that resolves to a value is rejected by name:
//! value comparison belongs to the equality family.

use std::collections::BTreeMap;

use crate::Primitive;
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceLibraryIdentity, InterfaceStructExport,
    InterfaceStructField, InterfaceTypeSnapshot,
};
use crate::passes::visibility_annotation::VisibilityTier;
use crate::test_support;

const ISSUE: &str = "est_value_rhs";

fn genus(name: &str) -> FileExport {
    FileExport {
        name: name.to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            params: Vec::new(),
            name: name.to_owned(),
            fields: vec![InterfaceStructField {
                name: "nomen".to_owned(),
                ty: InterfaceTypeSnapshot::Primitive(Primitive::Textus),
                optional: false,
                required: true,
                default: None,
                modifier: None,
                visibility: radix_syntax::MemberVisibility::Publica,
            }],
            methods: Vec::new(),
            annotation_contract: None,
        }),
    }
}

fn aux_interface() -> FileInterface {
    let mut interface = FileInterface::new();
    // One module identity, so the namespace and path installs unify.
    interface.identity = Some(InterfaceLibraryIdentity {
        provider: "faber".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["aux".to_owned()],
    });
    interface.insert(genus("Felis"));
    interface.insert(genus("Canis"));
    interface.insert(FileExport {
        name: "NOMEN".to_owned(),
        kind: FileExportKind::Const(InterfaceTypeSnapshot::Primitive(Primitive::Textus), None),
    });
    interface
}

/// Analyze `source` with `./aux` installed both as the `aux` namespace and
/// under its path (the key selective imports resolve through).
fn analyze(source: &str) -> test_support::AnalyzedFixture {
    let tiers = BTreeMap::from([
        ("Felis".to_owned(), VisibilityTier::Publica),
        ("Canis".to_owned(), VisibilityTier::Publica),
        ("NOMEN".to_owned(), VisibilityTier::Publica),
    ]);
    test_support::analyze_source_with_import_contract(
        source,
        BTreeMap::from([
            ("aux".to_owned(), tiers.clone()),
            ("./aux".to_owned(), tiers),
        ]),
        BTreeMap::from([
            ("aux".to_owned(), aux_interface()),
            ("./aux".to_owned(), aux_interface()),
        ]),
    )
}

fn has_issue(result: &test_support::AnalyzedFixture, issue: &str) -> bool {
    result
        .diagnostics
        .iter()
        .any(|err| err.is_error() && test_support::issue(err) == Some(issue))
}

fn assert_est_value_rhs(source: &str) {
    let result = analyze(source);
    assert!(
        has_issue(&result, ISSUE),
        "expected {ISSUE}, got: {:?}",
        result.diagnostics
    );
}

// ---------------------------------------------------------------------------
// D23.1/D23.5: imported types are legal `est` targets
// ---------------------------------------------------------------------------

#[test]
fn est_qualified_imported_type_is_legal() {
    let result = analyze(
        r#"importa ex "./aux" aux

functio f(aux.Felis ∪ aux.Canis x) → bivalens {
    redde x est aux.Felis
}

incipit { tacet }"#,
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(crate::SemanticError::is_error),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn non_est_qualified_imported_type_is_legal() {
    let result = analyze(
        r#"importa ex "./aux" aux

functio f(aux.Felis ∪ aux.Canis x) → bivalens {
    redde x non est aux.Canis
}

incipit { tacet }"#,
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(crate::SemanticError::is_error),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn est_selective_imported_type_is_legal() {
    let result = analyze(
        r#"importa ex "./aux" fixum Felis, Canis

functio f(Felis ∪ Canis x) → bivalens {
    redde x est Canis
}

incipit { tacet }"#,
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(crate::SemanticError::is_error),
        "{:?}",
        result.diagnostics
    );
}

// ---------------------------------------------------------------------------
// D23.1: a value right-hand side names the equality operator
// ---------------------------------------------------------------------------

#[test]
fn est_imported_const_value_is_rejected() {
    // A const export is a value: `est` against it is not a type test.
    assert_est_value_rhs(
        r#"importa ex "./aux" aux

functio f(textus x) → bivalens {
    redde x est aux.NOMEN
}

incipit { tacet }"#,
    );
}

#[test]
fn est_local_value_name_is_rejected() {
    assert_est_value_rhs("functio f(numerus x, numerus aliud) → bivalens { redde x est aliud }");
}

#[test]
fn est_boolean_literal_is_rejected() {
    assert_est_value_rhs("functio f(ignotum x) → bivalens { redde x est verum }");
    assert_est_value_rhs("functio f(ignotum x) → bivalens { redde x non est falsum }");
}

// ---------------------------------------------------------------------------
// D22.5: selective imports bind types as well as values
// ---------------------------------------------------------------------------

#[test]
fn selective_type_import_binds_a_type_name() {
    let result = analyze(
        r#"importa ex "./aux" fixum Canis

functio nomen(Canis c) → textus {
    redde c.nomen
}

incipit { tacet }"#,
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(crate::SemanticError::is_error),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn selective_type_import_alias_binds_the_local_name() {
    let result = analyze(
        r#"importa ex "./aux" fixum Canis ut Hund

functio nomen(Hund c) → textus {
    redde c.nomen
}

incipit { tacet }"#,
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(crate::SemanticError::is_error),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn selective_type_import_is_the_imported_type() {
    // The bare selective name and the qualified path are one type: a value of
    // one flows into a slot of the other with no conversion.
    let result = analyze(
        r#"importa ex "./aux" aux
importa ex "./aux" fixum Canis

functio nomen(Canis c) → textus {
    redde c.nomen
}

functio f(aux.Canis c) → textus {
    redde nomen(c)
}

incipit { tacet }"#,
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(crate::SemanticError::is_error),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn selective_value_import_still_binds_a_value() {
    let result = analyze(
        r#"importa ex "./aux" fixum NOMEN

functio f() → textus {
    redde NOMEN
}

incipit { tacet }"#,
    );
    assert!(
        !result
            .diagnostics
            .iter()
            .any(crate::SemanticError::is_error),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn selective_type_import_is_not_a_value() {
    let result = analyze(
        r#"importa ex "./aux" fixum Canis

incipit {
    nota Canis
}"#,
    );
    assert!(
        has_issue(&result, "name_not_value"),
        "{:?}",
        result.diagnostics
    );
}
