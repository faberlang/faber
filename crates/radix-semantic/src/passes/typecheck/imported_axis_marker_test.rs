//! Imported axis-marked parameters keep the call-site echo rule (polish
//! A-202): a local `functio rows(axis numerus a)` requires every call to
//! write `axis` again (`SEM057:axis_marker_required`). The file interface
//! must carry the flag on the exported parameter record so an importer's
//! call is refused exactly like the producer's own call.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use std::collections::BTreeMap;

use crate::Primitive;
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceCallable, InterfaceMethodExport,
    InterfaceParam, InterfaceStructExport, InterfaceTypeSnapshot,
};
use crate::passes::visibility_annotation::VisibilityTier;
use crate::test_support::{self, AnalyzedFixture};

const ISSUE: &str = "axis_marker_required";

fn error_issues(unit: &AnalyzedFixture) -> Vec<Option<&str>> {
    unit.diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(test_support::issue)
        .collect()
}

fn has_issue(unit: &AnalyzedFixture, expected: &str) -> bool {
    unit.diagnostics
        .iter()
        .any(|diagnostic| diagnostic.is_error() && test_support::issue(diagnostic) == Some(expected))
}

/// One `axis numerus` parameter.
fn axis_param() -> InterfaceParam {
    InterfaceParam {
        ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
        mode: crate::SemanticParamMode::Owned,
        optional: false,
        sponte: false,
        axis: true,
    }
}

fn rows_callable() -> InterfaceCallable {
    InterfaceCallable {
        type_params: Vec::new(),
        params: vec![axis_param()],
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Numerus)),
        err: None,
        is_async: false,
        is_generator: false,
        tensor_edge_policy_requirements: Vec::new(),
        device: None,
    }
}

fn axis_rows_method() -> InterfaceMethodExport {
    InterfaceMethodExport {
        name: "rows".to_owned(),
        callable: rows_callable(),
        visibility: radix_syntax::MemberVisibility::Publica,
        unavailable: Vec::new(),
    }
}

fn axis_rows_interface() -> FileInterface {
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "rows".to_owned(),
        kind: FileExportKind::Function(rows_callable()),
    });
    interface.insert(FileExport {
        name: "Row".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            name: "Row".to_owned(),
            params: Vec::new(),
            fields: Vec::new(),
            methods: vec![axis_rows_method()],
            annotation_contract: None,
        }),
    });
    interface
}

fn analyze_imported(source: &str) -> AnalyzedFixture {
    let rows = BTreeMap::from([
        ("rows".to_owned(), VisibilityTier::Publica),
        ("Row".to_owned(), VisibilityTier::Publica),
    ]);
    test_support::analyze_source_with_import_contract(
        source,
        BTreeMap::from([("lib".to_owned(), rows)]),
        BTreeMap::from([("lib".to_owned(), axis_rows_interface())]),
    )
}

// LOCAL FORM

#[test]
fn local_axis_parameter_requires_the_marker() {
    let unit = test_support::analyze_source(
        "functio rows(axis numerus a) → numerus { redde a + 1 }\n\
incipit {\n    fixum numerus x ← rows(1)\n}",
    );
    assert!(has_issue(&unit, ISSUE), "diagnostics: {:?}", unit.diagnostics);
}

#[test]
fn local_instance_method_axis_parameter_requires_the_marker() {
    let unit = test_support::analyze_source(
        "genus Row {\n    functio rows(axis numerus a) → numerus { redde a + 1 }\n}\n\
functio use(Row r) → numerus { redde r.rows(1) }",
    );
    assert!(has_issue(&unit, ISSUE), "diagnostics: {:?}", unit.diagnostics);
}

// IMPORTED FORM

#[test]
fn imported_axis_parameter_requires_the_marker() {
    let unit = analyze_imported(
        r#"importa ex "./lib" lib

incipit {
    fixum numerus x ← lib.rows(1)
}"#,
    );
    assert!(
        has_issue(&unit, ISSUE),
        "an imported axis parameter must echo `axis` like the local form, got: {:?}",
        error_issues(&unit)
    );
}

#[test]
fn imported_axis_parameter_accepts_the_marker() {
    let unit = analyze_imported(
        r#"importa ex "./lib" lib

incipit {
    fixum numerus x ← lib.rows(axis 1)
}"#,
    );
    assert!(
        unit.is_ok(),
        "an echoed imported axis argument must check, got: {:?}",
        unit.diagnostics
    );
}

#[test]
fn imported_instance_method_axis_parameter_requires_the_marker() {
    let unit = analyze_imported(
        r#"importa ex "./lib" lib

functio use(lib.Row r) → numerus {
    redde r.rows(1)
}"#,
    );
    assert!(
        has_issue(&unit, ISSUE),
        "an imported method axis parameter must echo `axis` like the local form, got: {:?}",
        error_issues(&unit)
    );
}
