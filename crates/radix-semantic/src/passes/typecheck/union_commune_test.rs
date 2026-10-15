//! TEU2 commune-field semantic contract.
//!
//! Proves the union declaration region is checked: `@ commune` / `@ shared`
//! resolve to one marker, construction requires commune fields, matching binds
//! them, `e.message` is member access (same-module and imported), assignment
//! through the projection is rejected, and `@ commune` is illegal off unions.

use std::collections::BTreeMap;

use crate::Primitive;
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceEnumVariant, InterfaceNominalExport,
    InterfaceStructField, InterfaceTypeSnapshot,
};
use crate::test_support;

const SHAPE_ERROR: &str = r"discretio ShapeError {
    @ commune
    textus message

    Incompatible,
    ElementMismatch { numerus expected numerus actual }
}
";

fn assert_checks_green(source: &str, label: &str) {
    let fixture = test_support::analyze_source_la(source);
    let errors: Vec<_> = fixture
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .collect();
    assert!(
        errors.is_empty(),
        "expected {label} to check green, got: {:?}",
        fixture.diagnostics
    );
}

fn shape_error_interface() -> FileInterface {
    let mut interface = FileInterface::new();
    let message = InterfaceStructField {
        name: "message".to_owned(),
        ty: InterfaceTypeSnapshot::Primitive(Primitive::Textus),
        optional: false,
        required: true,
        default: None,
        modifier: None,
        visibility: radix_syntax::MemberVisibility::Publica,
    };
    interface.insert(FileExport {
        name: "ShapeError".to_owned(),
        kind: FileExportKind::Enum(InterfaceNominalExport {
            params: Vec::new(),
            name: "ShapeError".to_owned(),
            methods: Vec::new(),
            shared_fields: vec![message.clone()],
            radix_contract: None,
            variants: vec![
                InterfaceEnumVariant {
                    name: "Incompatible".to_owned(),
                    fields: Vec::new(),
                },
                InterfaceEnumVariant {
                    name: "ElementMismatch".to_owned(),
                    fields: vec![
                        InterfaceStructField {
                            name: "expected".to_owned(),
                            ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
                            optional: false,
                            required: true,
                            default: None,
                            modifier: None,
                            visibility: radix_syntax::MemberVisibility::Publica,
                        },
                        InterfaceStructField {
                            name: "actual".to_owned(),
                            ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
                            optional: false,
                            required: true,
                            default: None,
                            modifier: None,
                            visibility: radix_syntax::MemberVisibility::Publica,
                        },
                    ],
                },
            ],
        }),
    });
    interface
}

fn analyze_consumer(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_with_import_contract_la(
        source,
        BTreeMap::from([(
            "shape".to_owned(),
            BTreeMap::from([(
                "ShapeError".to_owned(),
                crate::passes::visibility_annotation::VisibilityTier::Publica,
            )]),
        )]),
        BTreeMap::from([("shape".to_owned(), shape_error_interface())]),
    )
}

#[test]
fn commune_union_construct_match_and_member_access_checks_green() {
    assert_checks_green(
        &format!(
            r#"{SHAPE_ERROR}
functio describe(ShapeError e) → textus {{
    discerne e {{
        casu Incompatible fixum message {{ redde message }}
        casu ElementMismatch fixum message, expected, actual {{ redde message }}
    }}
}}

functio render(ShapeError e) → textus {{
    redde e.message
}}

functio boom() → vacuum ⇥ ShapeError {{
    iace finge Incompatible {{ message = "no" }} ∷ ShapeError
}}

incipit {{
    sit ShapeError a ← finge Incompatible {{ message = "incompatible" }} ∷ ShapeError
    sit ShapeError b ← finge ElementMismatch {{ message = "mismatch", expected = 1, actual = 2 }} ∷ ShapeError
    nota describe(a) + describe(b) + render(a)
}}
"#
        ),
        "commune declare/construct/match/member-access",
    );
}

#[test]
fn shared_spelling_resolves_to_the_commune_marker() {
    assert_checks_green(
        r#"discretio ShapeError {
    @ shared
    textus message

    Incompatible
}

functio render(ShapeError e) → textus {
    redde e.message
}

incipit {
    sit ShapeError e ← finge Incompatible { message = "x" } ∷ ShapeError
    nota render(e)
}
"#,
        "@ shared member access",
    );
}

#[test]
fn existing_union_without_commune_region_checks_identically() {
    test_support::assert_accepts_la(
        r"discretio Eventum {
    Ictus { numerus vis },
    Sonitus
}

incipit {
    sit Eventum a ← finge Ictus { vis = 3 } ∷ Eventum
    sit Eventum b ← finge Eventum.Sonitus { } ∷ Eventum
    nota a
    nota b
}",
    );
}

#[test]
fn construction_requires_commune_fields() {
    test_support::assert_rejects_with_issue_la(
        &format!(
            r"{SHAPE_ERROR}
incipit {{
    sit ShapeError e ← finge Incompatible {{ }} ∷ ShapeError
    nota e
}}
"
        ),
        "wrong_number_of_arguments",
    );
}

#[test]
fn commune_field_on_genus_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        r"genus Box {
    @ commune
    varia textus message
}

incipit {
    nota 1
}
",
        "commune_on_non_union",
    );
}

#[test]
fn commune_annotation_on_functio_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        r"@ commune
functio f() → vacuum {
}
",
        "commune_on_non_union",
    );
}

#[test]
fn duplicate_commune_field_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        r"discretio ShapeError {
    @ commune
    textus message
    @ commune
    textus message

    Incompatible
}
",
        "duplicate_commune_field",
    );
}

#[test]
fn variant_redeclaring_commune_field_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        r"discretio ShapeError {
    @ commune
    textus message

    Incompatible { textus message }
}
",
        "union_variant_redeclares_shared_field",
    );
}

#[test]
fn variant_redeclaring_unannotated_region_field_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        r"discretio ShapeError {
    @ commune
    textus message
    numerus code

    Incompatible { numerus code }
}
",
        "union_variant_redeclares_shared_field",
    );
}

#[test]
fn commune_field_assignment_is_rejected() {
    test_support::assert_rejects_with_issue_la(
        &format!(
            r#"{SHAPE_ERROR}
incipit {{
    varia ShapeError e ← finge Incompatible {{ message = "a" }} ∷ ShapeError
    e.message ← "b"
}}
"#
        ),
        "assignment_to_commune_field",
    );
}

#[test]
fn imported_union_member_access_checks_green() {
    let source = r#"importa ex "./shape" shape

functio render(shape.ShapeError e) → textus {
    redde e.message
}
"#;
    let diagnostics = analyze_consumer(source).diagnostics.clone();
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected imported-union e.message to check green, got: {diagnostics:?}"
    );
}

#[test]
fn imported_union_construction_requires_commune_field() {
    let source = r#"importa ex "./shape" shape

functio boom() → shape.ShapeError {
    redde finge shape.Incompatible { } ∷ shape.ShapeError
}
"#;
    let diagnostics = analyze_consumer(source).diagnostics.clone();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.is_error()
                && crate::test_support::issue(d) == Some("wrong_number_of_arguments")),
        "expected missing commune field to reject imported construction, got: {diagnostics:?}"
    );
}

#[test]
fn imported_union_match_binds_commune_field() {
    let source = r#"importa ex "./shape" shape

functio describe(shape.ShapeError e) → textus {
    discerne e {
        casu shape.ShapeError.Incompatible fixum message { redde message }
        casu shape.ShapeError.ElementMismatch fixum message, expected, actual {
            redde message
        }
    }
}
"#;
    let diagnostics = analyze_consumer(source).diagnostics.clone();
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected imported-union commune match bindings to check green, got: {diagnostics:?}"
    );
}

#[test]
fn publica_stacking_on_commune_field_checks_green() {
    assert_checks_green(
        r"discretio ShapeError {
    @ publica
    @ commune
    textus message

    Incompatible
}

functio render(ShapeError e) → textus {
    redde e.message
}
",
        "@ publica stacking on commune field",
    );
}
