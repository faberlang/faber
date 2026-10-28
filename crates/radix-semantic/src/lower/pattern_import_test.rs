//! Imported-class match classification (BUG-82852379).
//!
//! A `discerne` arm naming a class member of the scrutinee union lowers as a
//! type pattern whether the class is file-local, selectively imported (D22.5),
//! or qualified through its namespace — the same `casu Circle fixum c` over
//! `Circle ∪ nihil` checks for all three forms. Genuinely unknown names keep
//! failing closed: an unknown namespace member and a non-qualifier leading
//! path both report `unknown_variant`.

use std::collections::BTreeMap;

use crate::Primitive;
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceStructExport, InterfaceStructField,
    InterfaceTypeSnapshot,
};
use crate::test_support::{
    analyze_source_with_import_contract, assert_accepts, assert_rejects_with_issue,
};

/// Producer-shaped file interface: a public genus with one field exports as a
/// `Struct`, exactly as the CLI export assembly records `genus Circle` today.
fn circle_interface() -> FileInterface {
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Circle".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            name: "Circle".to_owned(),
            params: Vec::new(),
            fields: vec![InterfaceStructField {
                name: "radius".to_owned(),
                ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
                optional: false,
                required: true,
                default: None,
                modifier: None,
                visibility: radix_syntax::MemberVisibility::Publica,
            }],
            methods: Vec::new(),
            annotation_contract: None,
        }),
    });
    interface
}

fn analyze_geo_consumer(source: &str) -> Vec<crate::SemanticError> {
    // The namespace form keys the interface by the binding name; a selective
    // import keys it by the import path itself (EOT-2M). Install both, as the
    // package driver does when a unit carries both import forms.
    let public_circle = |key: &str| {
        (
            key.to_owned(),
            BTreeMap::from([(
                "Circle".to_owned(),
                crate::passes::visibility_annotation::VisibilityTier::Publica,
            )]),
        )
    };
    let fixture = analyze_source_with_import_contract(
        source,
        BTreeMap::from([public_circle("geo"), public_circle("./geo")]),
        BTreeMap::from([
            ("geo".to_owned(), circle_interface()),
            ("./geo".to_owned(), circle_interface()),
        ]),
    );
    fixture.diagnostics.clone()
}

fn assert_consumer_accepts(source: &str) {
    let diagnostics = analyze_geo_consumer(source);
    let errors: Vec<_> = diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.is_empty(),
        "expected imported-class discerne to check green, got: {diagnostics:?}"
    );
}

#[test]
fn local_class_arm_lowers_as_type_pattern() {
    // Control: the file-local genus keeps checking green (the parser's
    // prescan already routed this arm to a type pattern).
    assert_accepts(
        r"genus Circle { varia numerus radius }

functio area(Circle ∪ nihil u) → numerus {
    discerne u {
        casu Circle sit c { redde c.radius }
        casu nihil { redde 0 }
    }
}",
    );
}

#[test]
fn selectively_imported_class_arm_lowers_as_type_pattern() {
    assert_consumer_accepts(
        r#"importa ex "./geo" fixum Circle

functio area(Circle ∪ nihil u) → numerus {
    discerne u {
        casu Circle sit c { redde c.radius }
        casu nihil { redde 0 }
    }
}"#,
    );
}

#[test]
fn qualified_imported_class_arm_lowers_as_type_pattern() {
    assert_consumer_accepts(
        r#"importa ex "./geo" geo

functio area(geo.Circle ∪ nihil u) → numerus {
    discerne u {
        casu geo.Circle sit c { redde c.radius }
        casu nihil { redde 0 }
    }
}"#,
    );
}

#[test]
fn unknown_namespace_member_still_fails_closed() {
    // `geo.Missing` names no type export: the classification must not mask
    // the unknown name (bare-variant resolution reports it).
    let diagnostics = analyze_geo_consumer(
        r#"importa ex "./geo" geo

functio area(geo.Circle ∪ nihil u) → numerus {
    discerne u {
        casu geo.Missing sit c { redde c.radius }
        casu nihil { redde 0 }
    }
}"#,
    );
    assert!(
        diagnostics
            .iter()
            .any(|err| err.is_error() && crate::test_support::issue(err) == Some("unknown_variant")),
        "expected unknown_variant, got: {diagnostics:?}"
    );
}

#[test]
fn non_qualifier_leading_path_still_fails_closed() {
    // A leading path that is not a real qualifier must never bind the local
    // `Eventum.Ictus` (CG2-R07 fail-closed arm).
    assert_rejects_with_issue(
        r"discretio Eventum { Ictus, Sonitus }

functio describe(Eventum e) → numerus {
    discerne e {
        casu missing.Eventum.Ictus { redde 1 }
        casu Eventum.Sonitus { redde 2 }
    }
}",
        "unknown_variant",
    );
}
