//! A static (`generis`) genus field reads through the type name; a static read
//! through an instance and an instance field read through the type name are
//! named compile errors. Inside the genus's own members the path `Tab` is also
//! the receiver (`ego` lowers to the same path), so the receiver keeps reading.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use crate::test_support;

const TAB: &str = "genus Tab {\n    generis numerus Z = 3\n    varia numerus w = 1\n    functio f() → numerus { redde ego.w + ego.Z }\n}\n";

#[test]
fn static_field_read_through_type_name_is_accepted() {
    test_support::assert_accepts(&format!("{TAB}functio g() → numerus {{ redde Tab.Z + 1 }}"));
}

#[test]
fn static_field_read_through_instance_is_named() {
    test_support::assert_rejects_with_issue(
        &format!("{TAB}functio g(Tab t) → numerus {{ redde t.Z }}"),
        "static_field_read_through_instance",
    );
}

#[test]
fn static_field_read_of_instance_field_through_type_is_named() {
    test_support::assert_rejects_with_issue(
        &format!("{TAB}functio g() → numerus {{ redde Tab.w }}"),
        "instance_field_read_through_type",
    );
}

#[test]
fn static_field_read_of_unknown_field_through_type_is_unknown_struct_field() {
    test_support::assert_rejects_with_issue(
        &format!("{TAB}functio g() → numerus {{ redde Tab.nope }}"),
        "unknown_struct_field",
    );
}

#[test]
fn static_field_read_call_through_type_name_keeps_name_not_value() {
    test_support::assert_rejects_with_issue(
        &format!("{TAB}functio g() → numerus {{ redde Tab.f() }}"),
        "name_not_value",
    );
}

// ST3-2: a static field of an imported genus reads through `module.Tab.Z`.

mod imported {
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceStructExport, InterfaceStructField,
        InterfaceTypeSnapshot,
    };
    use crate::passes::visibility_annotation::VisibilityTier::Publica;
    use crate::test_support::{self, AnalyzedFixture};
    use radix_hir::HirItemKind;
    use radix_syntax::{FieldModifier, MemberVisibility};
    use std::collections::BTreeMap;

    fn field(name: &str, modifier: FieldModifier) -> InterfaceStructField {
        InterfaceStructField {
            name: name.to_owned(),
            ty: InterfaceTypeSnapshot::Primitive(crate::Primitive::Numerus),
            optional: false,
            required: false,
            default: None,
            modifier: Some(modifier),
            visibility: MemberVisibility::Publica,
        }
    }

    fn fons_interface() -> FileInterface {
        let mut interface = FileInterface::new();
        interface.insert(FileExport {
            name: "Tab".to_owned(),
            kind: FileExportKind::Struct(InterfaceStructExport {
                is_json: false,
                implements: Vec::new(),
                name: "Tab".to_owned(),
                params: Vec::new(),
                fields: vec![
                    field("Z", FieldModifier::Generis),
                    field("w", FieldModifier::Varia),
                ],
                methods: Vec::new(),
                annotation_contract: None,
            }),
        });
        interface
    }

    fn analyze(body: &str) -> AnalyzedFixture {
        test_support::analyze_source_with_import_contract(
            &format!("importa ex \"./fons\" fons\n{body}"),
            BTreeMap::from([(
                "fons".to_owned(),
                BTreeMap::from([("Tab".to_owned(), Publica)]),
            )]),
            BTreeMap::from([("fons".to_owned(), fons_interface())]),
        )
    }

    fn assert_issue(body: &str, expected: &str) {
        let fixture = analyze(body);
        assert!(
            fixture
                .diagnostics
                .iter()
                .any(|err| err.is_error() && test_support::issue(err) == Some(expected)),
            "expected {expected:?}, got: {:?}",
            fixture.diagnostics
        );
    }

    #[test]
    fn static_field_read_through_imported_type_name_is_accepted() {
        let fixture = analyze("functio g() → numerus { redde fons.Tab.Z + 1 }");
        assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
        let hir = fixture.result.hir.as_ref().expect("hir");
        let body = hir
            .items
            .iter()
            .find_map(|item| match &item.kind {
                HirItemKind::Function(func) => func.body.as_ref(),
                _ => None,
            })
            .expect("g body");
        let rendered = format!("{body:?}");
        assert!(
            rendered.contains("Field(") && rendered.contains("Path("),
            "the read lowers to a field of the imported genus path: {rendered}"
        );
    }

    #[test]
    fn static_field_read_through_imported_type_name_uses_the_import() {
        let fixture = analyze("functio g() → numerus { redde fons.Tab.Z }");
        assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
        assert!(
            !fixture
                .diagnostics
                .iter()
                .any(|err| test_support::issue(err) == Some("unused_import")),
            "diagnostics: {:?}",
            fixture.diagnostics
        );
    }

    #[test]
    fn static_field_read_of_imported_instance_field_through_type_is_named() {
        assert_issue(
            "functio g() → numerus { redde fons.Tab.w }",
            "instance_field_read_through_type",
        );
    }

    #[test]
    fn static_field_read_of_imported_unknown_field_is_unknown_struct_field() {
        assert_issue(
            "functio g() → numerus { redde fons.Tab.nope }",
            "unknown_struct_field",
        );
    }

    #[test]
    fn static_field_read_of_imported_static_through_instance_is_named() {
        assert_issue(
            "functio g(fons.Tab t) → numerus { redde t.Z }",
            "static_field_read_through_instance",
        );
    }
}
