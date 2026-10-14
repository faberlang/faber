//! Genus member privacy (DECISIONS D5.2, D5.3, D5.4, D15.2, D9.2).
//!
//! Members are public by default (D5.2). `@ privata` on a member is private to
//! the type: only the type's own methods may read, write, or call it (D5.3).
//! `@ interna` on a member admits same-package code only. A type-private field
//! may still be set in any construction literal, and `Genus { … } ex p`
//! copies it (D5.4, D15.2). A write through a union overlap needs the same
//! visibility on every member (D9.2).
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use std::collections::BTreeMap;

use radix_diagnostics::DiagnosticArg;

use crate::test_support;

const ARCA: &str = "\
genus Arca {
    @ privata
    varia numerus secretum
    varia numerus apertum

    functio lege() → numerus {
        redde ego.secretum
    }

    functio muta(numerus n) → vacuum {
        ego.secretum ← n
        ego.occulta()
    }

    functio compara(Arca alia) → bivalens {
        redde ego.secretum ≡ alia.secretum
    }

    @ privata
    functio occulta() → vacuum {
    }
}

";

fn arca(body: &str) -> String {
    format!("{ARCA}incipit {{\n    varia Arca a ← Arca {{ secretum = 1, apertum = 2 }}\n{body}\n}}")
}

fn private_args(issue: &'static str, member: &'static str) -> [DiagnosticArg; 3] {
    [
        DiagnosticArg::new("issue", issue),
        DiagnosticArg::new("member", member),
        DiagnosticArg::new("owner", "Arca"),
    ]
}

#[test]
fn own_methods_reach_private_members_and_construction_sets_them() {
    test_support::assert_accepts(&arca(
        "    nota a.lege()\n    a.muta(3)\n    nota a.compara(a)",
    ));
}

#[test]
fn unmarked_members_are_public() {
    test_support::assert_accepts(&arca("    a.apertum ← 5\n    nota a.apertum"));
}

#[test]
fn private_field_read_from_outside_is_rejected() {
    test_support::assert_rejects_with_args(
        &arca("    nota a.secretum"),
        &private_args("member_private_read", "secretum"),
    );
}

#[test]
fn private_field_write_from_outside_is_rejected() {
    test_support::assert_rejects_with_args(
        &arca("    a.secretum ← 4"),
        &private_args("member_private_write", "secretum"),
    );
}

#[test]
fn private_method_call_from_outside_is_rejected() {
    test_support::assert_rejects_with_args(
        &arca("    a.occulta()"),
        &private_args("member_private_call", "occulta"),
    );
}

#[test]
fn private_field_read_from_a_free_function_is_rejected() {
    test_support::assert_rejects_with_args(
        &format!("{ARCA}functio f(Arca a) → numerus {{\n    redde a.secretum\n}}\n"),
        &private_args("member_private_read", "secretum"),
    );
}

#[test]
fn private_field_is_copied_by_construction_source() {
    test_support::assert_accepts(&arca(
        "    fixum Arca b ← Arca { apertum = 7 } ex a\n    nota b.lege()",
    ));
}

#[test]
fn private_field_set_in_construction_outside_the_type_is_accepted() {
    test_support::assert_accepts(&format!(
        "{ARCA}functio fac() → Arca {{\n    redde Arca {{ secretum = 9, apertum = 0 }}\n}}\n"
    ));
}

#[test]
fn interna_members_are_reachable_in_the_same_file() {
    test_support::assert_accepts(
        "genus Arca {\n    @ interna\n    varia numerus n\n}\n\nincipit {\n    varia Arca a ← Arca { n = 1 }\n    a.n ← 2\n    nota a.n\n}",
    );
}

fn figura(circulus: &str, quadratum: &str) -> String {
    format!(
        "genus Circulus {{\n    {circulus}varia textus nomen\n}}\n\n\
         genus Quadratum {{\n    {quadratum}varia textus nomen\n}}\n\n\
         functio f(Circulus ∪ Quadratum x) {{\n    varia Circulus ∪ Quadratum y ← copy x\n    y.nomen ← \"z\"\n}}"
    )
}

#[test]
fn union_write_with_differing_member_visibility_is_rejected() {
    test_support::assert_rejects_with_args(
        &figura("@ interna\n    ", ""),
        &[
            DiagnosticArg::new("issue", "union_member_visibility_differs"),
            DiagnosticArg::new("member", "nomen"),
            DiagnosticArg::new(
                "detail",
                "`@ interna` in Circulus, `@ publica` in Quadratum",
            ),
        ],
    );
}

#[test]
fn union_write_with_matching_member_visibility_is_accepted() {
    test_support::assert_accepts(&figura("@ interna\n    ", "@ interna\n    "));
}

#[test]
fn union_read_of_a_private_member_is_rejected() {
    let source = "genus Circulus {\n    @ privata\n    varia textus nomen\n}\n\n\
         genus Quadratum {\n    @ privata\n    varia textus nomen\n}\n\n\
         functio f(Circulus ∪ Quadratum x) → textus {\n    redde x.nomen\n}";
    test_support::assert_rejects_with_issue(source, "member_private_read");
}

// IMPORTED TYPES

fn imported_interface() -> crate::file_interface::FileInterface {
    use crate::Primitive;
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceCallable, InterfaceMethodExport,
        InterfaceStructExport, InterfaceStructField, InterfaceTypeSnapshot,
    };
    use radix_syntax::MemberVisibility;
    let field = |name: &str, visibility| InterfaceStructField {
        name: name.to_owned(),
        ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
        optional: false,
        required: true,
        default: None,
        modifier: Some(radix_syntax::FieldModifier::Varia),
        visibility,
    };
    let method = |name: &str, visibility| InterfaceMethodExport {
        name: name.to_owned(),
        callable: InterfaceCallable {
            type_params: Vec::new(),
            params: Vec::new(),
            ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Vacuum)),
            err: None,
            is_async: false,
            is_generator: false,
            tensor_edge_policy_requirements: Vec::new(),
            device: None,
        },
        visibility,
        unavailable: Vec::new(),
    };
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Arca".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            params: Vec::new(),
            name: "Arca".to_owned(),
            fields: vec![
                field("secretum", MemberVisibility::Privata),
                field("domesticum", MemberVisibility::Interna),
                field("apertum", MemberVisibility::Publica),
            ],
            methods: vec![
                method("occulta", MemberVisibility::Privata),
                method("domestica", MemberVisibility::Interna),
            ],
            annotation_contract: None,
        }),
    });
    interface
}

fn analyze_importer(body: &str, same_package: bool) -> test_support::AnalyzedFixture {
    use crate::passes::visibility_annotation::VisibilityTier;
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.namespace_exports = BTreeMap::from([(
        "arcae".to_owned(),
        BTreeMap::from([("Arca".to_owned(), VisibilityTier::Publica)]),
    )]);
    config.file_interfaces = BTreeMap::from([("arcae".to_owned(), imported_interface())]);
    config.namespace_seams = BTreeMap::from([("arcae".to_owned(), same_package)]);
    test_support::analyze_source_with_config(
        &format!(
            "importa ex \"./arcae\" arcae\n\nincipit {{\n    varia arcae.Arca a ← arcae.Arca {{ secretum = 1, domesticum = 2, apertum = 3 }}\n{body}\n}}"
        ),
        config,
    )
}

fn has_error_issue(result: &test_support::AnalyzedFixture, issue: &str) -> bool {
    result
        .diagnostics
        .iter()
        .any(|err| err.is_error() && test_support::issue(err) == Some(issue))
}

#[test]
fn imported_private_field_may_be_set_in_construction() {
    for same_package in [true, false] {
        let result = analyze_importer("    nota a.apertum", same_package);
        assert!(result.is_ok(), "{:?}", result.diagnostics);
    }
}

#[test]
fn imported_private_members_are_rejected_even_in_the_same_package() {
    let read = analyze_importer("    nota a.secretum", true);
    assert!(
        has_error_issue(&read, "member_private_read"),
        "{:?}",
        read.diagnostics
    );
    let call = analyze_importer("    a.occulta()", true);
    assert!(
        has_error_issue(&call, "member_private_call"),
        "{:?}",
        call.diagnostics
    );
}

#[test]
fn imported_interna_members_are_reachable_in_the_same_package() {
    let result = analyze_importer(
        "    a.domesticum ← 4\n    nota a.domesticum\n    a.domestica()",
        true,
    );
    assert!(result.is_ok(), "{:?}", result.diagnostics);
}

#[test]
fn imported_interna_members_are_rejected_across_packages() {
    let read = analyze_importer("    nota a.domesticum", false);
    assert!(
        has_error_issue(&read, "member_interna_read"),
        "{:?}",
        read.diagnostics
    );
    let write = analyze_importer("    a.domesticum ← 4", false);
    assert!(
        has_error_issue(&write, "member_interna_write"),
        "{:?}",
        write.diagnostics
    );
    let call = analyze_importer("    a.domestica()", false);
    assert!(
        has_error_issue(&call, "member_interna_call"),
        "{:?}",
        call.diagnostics
    );
}
