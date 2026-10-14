//! `evaluate_deferred`: the package layer's entry to the evaluation of a
//! module whose sites reach an import. The tests inject a stub
//! `ImportedEffects` and a stub runner; assertions are on issues, args and
//! tree shape only.

use super::evaluate_deferred;
use super::tests::{
    arg,
    imported::{IMPORT, analyze_with, is_unevaluated},
    int, static_value,
};
use crate::driver::{AnalyzeRequest, AnalyzedModule, Config, ModuleImportContract, Session, analyze_source};
use crate::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceCallable, InterfaceMethodExport, InterfaceStructExport,
    InterfaceStructField, InterfaceTypeSnapshot,
};
use crate::hir::{DefId, HirItemKind};
use crate::semantic::Primitive;
use crate::semantic::passes::praefixum::{ImportedCallee, ImportedEffects, ImportedReference};
use crate::semantic::passes::visibility_annotation::VisibilityTier;
use crate::semantic::{SemanticError, SemanticErrorKind};
use radix_diagnostics::Diagnostic;
use radix_lexer::Span;
use radix_mir_runner::Value;
use std::collections::BTreeMap;

/// Answers every import reference with one fixed answer, counting asks.
struct Stub {
    answer: ImportedCallee,
    asked: usize,
}

impl Stub {
    fn new(answer: ImportedCallee) -> Self {
        Self { answer, asked: 0 }
    }
}

impl ImportedEffects for Stub {
    fn callee(&mut self, _reference: &ImportedReference) -> ImportedCallee {
        self.asked += 1;
        self.answer.clone()
    }
}

const SOURCE_TAIL: &str = "fixum numerus G = praefixum {\n    redde consts.seven()\n}\n\
                           fixum numerus H = G + 1\n";

fn deferred_unit() -> AnalyzedModule {
    let source = format!("{IMPORT}{SOURCE_TAIL}");
    let unit =
        analyze_with(&source, true).unwrap_or_else(|diagnostics| panic!("deferral is not an error: {diagnostics:?}"));
    assert!(is_unevaluated(&unit, "G"));
    unit
}

fn evaluate_with(
    unit: &mut AnalyzedModule,
    imports: &mut Stub,
    run: &mut dyn FnMut(AnalyzedModule, DefId, Span) -> Result<Value, SemanticError>,
) -> Result<(), Vec<Diagnostic>> {
    evaluate_deferred(unit, "praefixum.fab", "", imports, run)
}

fn issue_of(diagnostics: &[Diagnostic]) -> &Diagnostic {
    assert_eq!(diagnostics.len(), 1, "diagnostics: {diagnostics:?}");
    assert_eq!(diagnostics[0].code, Some("SEM064"));
    &diagnostics[0]
}

/// A contract whose `consts` namespace exports one genus, `Box`, carrying a
/// single public method `get`. `contract()` in `praefixum_test.rs` is private
/// to `mod imported`, so this file builds its own.
fn box_contract() -> ModuleImportContract {
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "Box".to_owned(),
        kind: FileExportKind::Struct(InterfaceStructExport {
            is_json: false,
            implements: Vec::new(),
            name: "Box".to_owned(),
            params: Vec::new(),
            fields: vec![InterfaceStructField {
                name: "n".to_owned(),
                ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
                optional: false,
                required: true,
                default: None,
                modifier: None,
                visibility: crate::syntax::MemberVisibility::Publica,
            }],
            methods: vec![InterfaceMethodExport {
                name: "get".to_owned(),
                callable: InterfaceCallable {
                    type_params: Vec::new(),
                    params: Vec::new(),
                    ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Numerus)),
                    err: None,
                    is_async: false,
                    is_generator: false,
                    tensor_edge_policy_requirements: Vec::new(),
                    device: None,
                },
                visibility: crate::syntax::MemberVisibility::Publica,
                unavailable: Vec::new(),
            }],
            annotation_contract: None,
        }),
    });
    let exports = BTreeMap::from([("Box".to_owned(), VisibilityTier::Publica)]);
    ModuleImportContract {
        namespace_exports: BTreeMap::from([("consts".to_owned(), exports)]),
        file_interfaces: BTreeMap::from([("consts".to_owned(), interface)]),
        ..ModuleImportContract::default()
    }
}

fn analyze_box(source: &str) -> Result<AnalyzedModule, Vec<Diagnostic>> {
    let session = Session::new(
        Config::default()
            .with_dev_locale_root()
            .with_defer_imported_praefixum(),
    );
    analyze_source(
        &session,
        "praefixum.fab",
        source,
        AnalyzeRequest { cli_program: None, contract: box_contract() },
    )
}

/// The body reaches an import only through a method on a genus this module
/// does not declare; the genus literal itself is not an import reference.
fn deferred_method_unit() -> AnalyzedModule {
    let source = format!(
        "{IMPORT}fixum numerus M = praefixum {{\n    fixum consts.Box b ← consts.Box {{ n = 7 }}\n    redde b.get()\n}}\n"
    );
    let unit = analyze_box(&source)
        .unwrap_or_else(|diagnostics| panic!("method-only deferral is not an error: {diagnostics:?}"));
    assert!(is_unevaluated(&unit, "M"));
    unit
}

#[test]
fn praefixum_method_deferral_method_only_body_defers() {
    let mut unit = deferred_method_unit();
    let mut imports = Stub::new(ImportedCallee::Pure);
    let result = evaluate_with(&mut unit, &mut imports, &mut |_scratch, _function, _span| Ok(Value::Int(7)));
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(imports.asked, 1, "only the method reference is answered");
    assert_eq!(int(&static_value(&unit, "M")), 7);
}

#[test]
fn praefixum_method_deferral_unavailable_is_refused() {
    let mut unit = deferred_method_unit();
    let mut imports = Stub::new(ImportedCallee::Unavailable);
    let diagnostics = evaluate_with(&mut unit, &mut imports, &mut |_scratch, _function, _span| {
        panic!("an unavailable site must not run")
    })
    .expect_err("an unavailable import is a build error");
    let diagnostic = issue_of(&diagnostics);
    assert_eq!(diagnostic.issue(), Some("praefixum_import_unavailable"));
    assert_eq!(arg(diagnostic, "name"), Some("M"));
    assert!(is_unevaluated(&unit, "M"), "a refused site keeps its initializer");
}

#[test]
fn a_returned_value_is_installed_and_dependents_refold() {
    let mut unit = deferred_unit();
    let mut imports = Stub::new(ImportedCallee::Pure);
    let mut seen_scratch = false;
    let result = evaluate_with(&mut unit, &mut imports, &mut |scratch, function, _span| {
        // The scratch holds the synthetic function the runner is told about.
        seen_scratch = scratch
            .hir
            .items
            .iter()
            .any(|item| item.def_id == function && matches!(item.kind, HirItemKind::Function(_)));
        Ok(Value::Int(7))
    });
    assert!(result.is_ok(), "{result:?}");
    assert!(seen_scratch);
    assert_eq!(imports.asked, 1);
    assert_eq!(int(&static_value(&unit, "G")), 7);
    assert_eq!(int(&static_value(&unit, "H")), 8);
}

#[test]
fn a_runner_failure_surfaces_its_issue() {
    let mut unit = deferred_unit();
    let mut imports = Stub::new(ImportedCallee::Pure);
    let diagnostics = evaluate_with(&mut unit, &mut imports, &mut |_scratch, _function, span| {
        Err(SemanticError::structured(SemanticErrorKind::PraefixumInvalid, span)
            .with_arg("issue", "praefixum_failed")
            .with_arg("message", "boom"))
    })
    .expect_err("a failing runner is a build error");
    let diagnostic = issue_of(&diagnostics);
    assert_eq!(diagnostic.issue(), Some("praefixum_failed"));
    assert_eq!(arg(diagnostic, "name"), Some("G"));
    assert_eq!(arg(diagnostic, "message"), Some("boom"));
    assert!(is_unevaluated(&unit, "G"), "a failed site keeps its initializer");
}

#[test]
fn an_effect_is_forbidden_with_the_imported_hop_in_the_chain() {
    let mut unit = deferred_unit();
    let mut imports =
        Stub::new(ImportedCallee::Effect { effect: "scribe", chain: vec!["seven".to_owned(), "imprimo".to_owned()] });
    let diagnostics = evaluate_with(&mut unit, &mut imports, &mut |_scratch, _function, _span| {
        panic!("a forbidden site must not run")
    })
    .expect_err("an effect is a build error");
    let diagnostic = issue_of(&diagnostics);
    assert_eq!(diagnostic.issue(), Some("praefixum_forbidden"));
    assert_eq!(arg(diagnostic, "name"), Some("G"));
    assert_eq!(arg(diagnostic, "effect"), Some("scribe"));
    assert_eq!(arg(diagnostic, "chain"), Some("G → consts.seven → imprimo"));
}

#[test]
fn praefixum_method_deferral_builtin_collection_body_is_unchanged() {
    let unit = analyze_with(
        "fixum numerus K = praefixum {\n    varia lista<numerus> xs ← vacua\n    \
         xs.appende(10)\n    xs.appende(20)\n    redde xs.longitudo()\n}\n",
        true,
    )
    .unwrap_or_else(|diagnostics| panic!("a builtin collection method is not an import: {diagnostics:?}"));
    assert!(!is_unevaluated(&unit, "K"));
    assert_eq!(int(&static_value(&unit, "K")), 2);
}

#[test]
fn an_unavailable_answer_is_refused_by_name_and_never_runs() {
    let mut unit = deferred_unit();
    let mut imports = Stub::new(ImportedCallee::Unavailable);
    let diagnostics = evaluate_with(&mut unit, &mut imports, &mut |_scratch, _function, _span| {
        panic!("an unavailable site must not run")
    })
    .expect_err("an unavailable import is a build error");
    let diagnostic = issue_of(&diagnostics);
    assert_eq!(diagnostic.issue(), Some("praefixum_import_unavailable"));
    assert_eq!(arg(diagnostic, "name"), Some("G"));
    assert_eq!(arg(diagnostic, "callee"), Some("consts.seven"));
}
