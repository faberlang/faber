//! Genus field writes against the declared modifier (DECISIONS D16.1, D16.3, D9.2).
//!
//! A `fixum` field is set only in a construction literal: any later write
//! (from outside or from the type's own methods) is `assignment_to_fixum_field`.
//! It behaves like the `fixum` binding form (mutability-engine D-1): an
//! assignment whose place passes through the field (`p.inner.y ← …`,
//! `p.xs[0] ← …`) is rejected, while a mutating method on it
//! (`p.xs.appende(3)`) is allowed. `varia` fields stay writable (an unmarked
//! genus field is a parse error since D5c).
//!
//! Writes through a union overlap need `varia` on the member fields: a member
//! whose field declares any other modifier is named in `union_member_not_varia`.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use radix_diagnostics::DiagnosticArg;

use crate::test_support;

const PUNCTUM: &str = "\
genus Punctum {
    sit numerus x
    varia numerus y
    varia numerus w
    sit lista<numerus> xs
}

";

fn punctum(body: &str) -> String {
    format!("{PUNCTUM}{body}")
}

const NEW: &str = "varia Punctum p ← Punctum { x = 1, y = 2, w = 3, xs = [] }";

#[test]
fn fixum_field_is_set_by_construction() {
    test_support::assert_accepts(&punctum(&format!(
        "incipit {{\n    {NEW}\n    nota p.x\n}}"
    )));
}

#[test]
fn fixum_field_reassignment_is_rejected() {
    test_support::assert_rejects_with_args(
        &punctum(&format!("incipit {{\n    {NEW}\n    p.x ← 5\n}}")),
        &[
            DiagnosticArg::new("issue", "assignment_to_fixum_field"),
            DiagnosticArg::new("field", "x"),
        ],
    );
}

#[test]
fn fixum_field_write_from_own_method_is_rejected() {
    test_support::assert_rejects_with_issue(
        "genus Punctum {\n    sit numerus x\n\n    functio move() → vacuum { ego.x ← 2 }\n}\n",
        "assignment_to_fixum_field",
    );
}

#[test]
fn place_write_through_fixum_field_is_rejected_like_the_binding_form() {
    test_support::assert_rejects_with_args(
        &punctum(&format!("incipit {{\n    {NEW}\n    p.xs[0] ← 5\n}}")),
        &[
            DiagnosticArg::new("issue", "assignment_to_fixum_field"),
            DiagnosticArg::new("field", "xs"),
        ],
    );
    test_support::assert_rejects_with_args(
        concat!(
            "genus Intus {\n    varia numerus y\n}\n\n",
            "genus Foris {\n    sit Intus intus\n}\n\n",
            "incipit {\n    varia Foris f ← Foris { intus = Intus { y = 1 } }\n    f.intus.y ← 2\n}",
        ),
        &[
            DiagnosticArg::new("issue", "assignment_to_fixum_field"),
            DiagnosticArg::new("field", "intus"),
        ],
    );
}

#[test]
fn varia_fields_stay_writable() {
    test_support::assert_accepts(&punctum(&format!(
        "incipit {{\n    {NEW}\n    p.y ← 5\n    p.w ← 6\n    nota p.y\n}}"
    )));
}

#[test]
fn fixum_list_field_contents_may_change() {
    // Shallow like the binding form (mutability-engine D-1).
    test_support::assert_accepts(&punctum(&format!(
        "incipit {{\n    {NEW}\n    p.xs.appende(3)\n    nota p.xs\n}}"
    )));
}

fn figura(circulus: &str, quadratum: &str) -> String {
    format!(
        "genus Circulus {{\n    {circulus}textus nomen\n}}\n\n\
         genus Quadratum {{\n    {quadratum}textus nomen\n}}\n\n\
         functio f(Circulus ∪ Quadratum x) {{\n    varia Circulus ∪ Quadratum y ← copy x\n    y.nomen ← \"z\"\n}}"
    )
}

#[test]
fn union_write_with_every_member_varia_is_accepted() {
    test_support::assert_accepts(&figura("varia ", "varia "));
}

#[test]
fn union_write_names_the_member_that_is_not_varia() {
    test_support::assert_rejects_with_args(
        &figura("varia ", "sit "),
        &[
            DiagnosticArg::new("issue", "union_member_not_varia"),
            DiagnosticArg::new("member", "nomen"),
            DiagnosticArg::new("members", "Quadratum"),
        ],
    );
}

/// An imported genus keeps its field modifiers across the file interface.
fn imported_punctum_interface() -> crate::file_interface::FileInterface {
    use crate::Primitive;
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceStructExport, InterfaceStructField,
        InterfaceTypeSnapshot,
    };
    let field = |name: &str, modifier| InterfaceStructField {
        name: name.to_owned(),
        ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
        optional: false,
        required: true,
        default: None,
        modifier,
        visibility: radix_syntax::MemberVisibility::Publica,
    };
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Punctum".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            params: Vec::new(),
            name: "Punctum".to_owned(),
            fields: vec![
                field("x", Some(radix_syntax::FieldModifier::Fixum)),
                field("y", Some(radix_syntax::FieldModifier::Varia)),
            ],
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });
    interface
}

fn analyze_importer(body: &str) -> crate::test_support::AnalyzedFixture {
    use crate::passes::visibility_annotation::VisibilityTier;
    use std::collections::BTreeMap;
    test_support::analyze_source_with_import_contract(
        &format!(
            "importa ex \"./forma\" forma\n\nincipit {{\n    varia forma.Punctum p ← forma.Punctum {{ x = 1, y = 2 }}\n{body}\n}}"
        ),
        BTreeMap::from([(
            "forma".to_owned(),
            BTreeMap::from([("Punctum".to_owned(), VisibilityTier::Publica)]),
        )]),
        BTreeMap::from([("forma".to_owned(), imported_punctum_interface())]),
    )
}

#[test]
fn imported_fixum_field_reassignment_is_rejected() {
    let result = analyze_importer("    p.y ← 3\n    p.x ← 3");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error()
                && test_support::issue(err) == Some("assignment_to_fixum_field")),
        "{:?}",
        result.diagnostics
    );
    let varia_only = analyze_importer("    p.y ← 3");
    assert!(varia_only.is_ok(), "{:?}", varia_only.diagnostics);
}
