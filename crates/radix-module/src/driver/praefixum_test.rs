//! Build-time evaluation of `praefixum` statics (DECISIONS D8.4–D8.6).
//!
//! Contract: analysis runs each `praefixum { … }` body on the MIR runner in
//! dependency order and replaces the initializer with a literal of the
//! declared type, so no backend sees the body. `mori` and a propagated `⇥`
//! are build errors carrying the message; an endless loop hits the step
//! budget; a closure result has no literal form. Faber re-emission keeps the
//! authored body.
#![cfg(feature = "runner")]

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, AnalyzedModule, Config, Session, analyze_source};
use crate::hir::{HirArrayElement, HirExpression, HirExpressionKind, HirItemKind, HirLiteral, HirObjectKey, HirUnOp};
use radix_diagnostics::Diagnostic;

fn analyze(source: &str) -> Result<AnalyzedModule, Vec<Diagnostic>> {
    let session = Session::new(Config::default().with_latin_locale().with_dev_locale_root());
    analyze_source(&session, "praefixum.fab", source, AnalyzeRequest::default())
}

fn analyze_ok(source: &str) -> AnalyzedModule {
    analyze(source).unwrap_or_else(|diagnostics| panic!("analysis failed: {diagnostics:?}"))
}

pub(super) fn static_value(unit: &AnalyzedModule, name: &str) -> HirExpression {
    unit.hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if unit.interner.resolve(konst.name) == name => Some(konst.value.clone()),
            _ => None,
        })
        .expect("static item")
}

pub(super) fn int(expr: &HirExpression) -> i128 {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(v)) => i128::from(*v),
        HirExpressionKind::Unary(HirUnOp::Neg, inner) => -int(inner),
        other => panic!("expected int literal, got {other:?}"),
    }
}

fn rejection(source: &str, issue: &str) -> Diagnostic {
    let diagnostics = match analyze(source) {
        Ok(unit) => panic!("expected {issue}, analysis succeeded: {:?}", unit.diagnostics),
        Err(diagnostics) => diagnostics,
    };
    diagnostics
        .iter()
        .find(|d| d.code == Some("SEM064") && d.issue() == Some(issue))
        .cloned()
        .unwrap_or_else(|| panic!("expected SEM064 {issue}, got {diagnostics:?}"))
}

pub(super) fn arg<'a>(diagnostic: &'a Diagnostic, name: &str) -> Option<&'a str> {
    diagnostic
        .args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

#[test]
fn loop_with_callee_folds_to_int_literal() {
    let unit = analyze_ok(
        "functio quadratum(numerus n) → numerus {\n    redde n * n\n}\n\
         fixum numerus SUMMA = praefixum {\n    varia numerus s ← 0\n    \
         itera ab 1‥4 sit i {\n        s ← s + quadratum(i)\n    }\n    redde s\n}\n\
         incipit {\n    nota SUMMA\n}\n",
    );
    assert_eq!(int(&static_value(&unit, "SUMMA")), 14);
}

#[test]
fn dependent_statics_fold_after_evaluation() {
    let unit = analyze_ok(
        "fixum numerus B = praefixum {\n    redde A * 2\n}\n\
         fixum numerus C = B + 1\n\
         fixum numerus A = praefixum {\n    redde 0 - 3\n}\n",
    );
    assert_eq!(int(&static_value(&unit, "A")), -3);
    assert_eq!(int(&static_value(&unit, "B")), -6);
    assert_eq!(int(&static_value(&unit, "C")), -5);
}

#[test]
fn list_result_is_an_array_literal() {
    let unit = analyze_ok(
        "fixum lista<numerus> L = praefixum {\n    varia lista<numerus> xs ← vacua\n    \
         itera ab 0‥3 sit i {\n        xs.appende(i * 10)\n    }\n    redde xs\n}\n",
    );
    let HirExpressionKind::Array(elements) = static_value(&unit, "L").kind else {
        panic!("expected array literal");
    };
    let values: Vec<i128> = elements
        .iter()
        .map(|element| match element {
            HirArrayElement::Expr(expr) => int(expr),
            HirArrayElement::Spread(_) => panic!("unexpected spread"),
            HirArrayElement::Infer(_) => panic!("unexpected shape hole"),
        })
        .collect();
    assert_eq!(values, [0, 10, 20]);
}

#[test]
fn genus_result_is_a_typed_construction_literal() {
    let unit = analyze_ok(
        "genus Punctum {\n    sit numerus x\n    sit numerus y\n}\n\
         fixum Punctum P = praefixum {\n    redde Punctum { x = 2 * 3, y = 0 - 1 }\n}\n",
    );
    let HirExpressionKind::Verte { entries: Some(entries), .. } = static_value(&unit, "P").kind else {
        panic!("expected typed construction");
    };
    let fields: Vec<(String, i128)> = entries
        .iter()
        .map(|field| match (&field.key, &field.value) {
            (HirObjectKey::Ident(name), Some(value)) => (unit.interner.resolve(*name).to_owned(), int(value)),
            other => panic!("unexpected entry {other:?}"),
        })
        .collect();
    assert_eq!(fields, [("x".to_owned(), 6), ("y".to_owned(), -1)]);
}

#[test]
fn mori_is_a_build_error_carrying_its_message() {
    let diagnostic = rejection(
        "fixum numerus N = praefixum {\n    si 1 ≡ 1 {\n        mori \"limes excessus\"\n    }\n    redde 1\n}\n",
        "praefixum_failed",
    );
    assert_eq!(arg(&diagnostic, "message"), Some("limes excessus"));
    assert_eq!(arg(&diagnostic, "name"), Some("N"));
}

#[test]
fn endless_loop_hits_the_step_limit() {
    let diagnostic = rejection(
        "fixum numerus N = praefixum {\n    varia numerus i ← 0\n    dum verum {\n        i ← i + 1\n    }\n    redde i\n}\n",
        "praefixum_step_limit",
    );
    assert_eq!(arg(&diagnostic, "name"), Some("N"));
}

#[test]
fn faber_reemission_keeps_the_authored_body() {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_target(Target::HirFaber),
    );
    let unit = analyze_source(
        &session,
        "praefixum.fab",
        "fixum numerus N = praefixum {\n    redde 2 + 2\n}\n",
        AnalyzeRequest::default(),
    )
    .expect("analysis");
    assert!(matches!(static_value(&unit, "N").kind, HirExpressionKind::Praefixum(_)));
}

pub(super) mod imported {
    //! A body that reaches an import never runs in analysis: it is refused
    //! by name, or the whole module is left for the package layer.

    use super::*;
    use crate::driver::ModuleImportContract;
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceCallable, InterfaceConstValue, InterfaceTypeSnapshot,
    };
    use crate::semantic::Primitive;
    use crate::semantic::passes::visibility_annotation::VisibilityTier;
    use std::collections::BTreeMap;

    pub(in crate::driver::praefixum) const IMPORT: &str = "importa ex \"./consts\" consts\n";

    fn contract() -> ModuleImportContract {
        let mut interface = FileInterface::new();
        interface.insert(FileExport {
            name: "BASE".to_owned(),
            kind: FileExportKind::Const(
                InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
                Some(InterfaceConstValue::Int(5)),
            ),
        });
        interface.insert(FileExport {
            name: "seven".to_owned(),
            kind: FileExportKind::Function(InterfaceCallable {
                type_params: Vec::new(),
                params: Vec::new(),
                ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Numerus)),
                err: None,
                is_async: false,
                is_generator: false,
                tensor_edge_policy_requirements: Vec::new(),
                device: None,
            }),
        });
        let exports = ["BASE", "seven"]
            .into_iter()
            .map(|name| (name.to_owned(), VisibilityTier::Publica))
            .collect();
        ModuleImportContract {
            namespace_exports: BTreeMap::from([("consts".to_owned(), exports)]),
            file_interfaces: BTreeMap::from([("consts".to_owned(), interface)]),
            ..ModuleImportContract::default()
        }
    }

    pub(in crate::driver::praefixum) fn analyze_with(
        source: &str,
        defer: bool,
    ) -> Result<AnalyzedModule, Vec<Diagnostic>> {
        let mut config = Config::default().with_latin_locale().with_dev_locale_root();
        if defer {
            config = config.with_defer_imported_praefixum();
        }
        let session = Session::new(config);
        analyze_source(
            &session,
            "praefixum.fab",
            source,
            AnalyzeRequest { cli_program: None, contract: contract() },
        )
    }

    fn refusal(source: &str) -> Vec<Diagnostic> {
        match analyze_with(source, false) {
            Ok(_) => panic!("a body reaching an import must be refused"),
            Err(diagnostics) => diagnostics,
        }
    }

    fn refused(source: &str, name: &str, callee: &str) {
        let diagnostics = refusal(source);
        let found: Vec<&Diagnostic> = diagnostics
            .iter()
            .filter(|d| d.code == Some("SEM064") && d.issue() == Some("praefixum_import_unavailable"))
            .collect();
        assert_eq!(found.len(), 1, "diagnostics: {diagnostics:?}");
        assert_eq!(arg(found[0], "name"), Some(name));
        assert_eq!(arg(found[0], "callee"), Some(callee));
    }

    pub(in crate::driver::praefixum) fn is_unevaluated(unit: &AnalyzedModule, name: &str) -> bool {
        matches!(static_value(unit, name).kind, HirExpressionKind::Praefixum(_))
    }

    #[test]
    fn flag_off_refuses_an_imported_call_by_name() {
        refused(
            &format!("{IMPORT}fixum numerus G = praefixum {{\n    redde consts.seven()\n}}\n"),
            "G",
            "consts.seven",
        );
    }

    #[test]
    fn flag_off_refuses_an_imported_value_by_name() {
        refused(
            &format!("{IMPORT}fixum numerus E = praefixum {{\n    redde consts.BASE + 1\n}}\n"),
            "E",
            "consts.BASE",
        );
    }

    #[test]
    fn flag_off_refuses_a_reference_reached_through_a_local_helper() {
        refused(
            &format!(
                "{IMPORT}functio auxilium() → numerus {{\n    redde consts.seven()\n}}\n\
                 fixum numerus G = praefixum {{\n    redde auxilium()\n}}\n"
            ),
            "G",
            "consts.seven",
        );
    }

    #[test]
    fn flag_off_evaluates_the_sites_that_reach_nothing() {
        let diagnostics = refusal(&format!(
            "{IMPORT}fixum numerus L = praefixum {{\n    redde 2 + 3\n}}\n\
             fixum numerus G = praefixum {{\n    redde consts.seven()\n}}\n"
        ));
        let issues: Vec<_> = diagnostics.iter().filter_map(|d| d.issue()).collect();
        assert_eq!(issues, ["praefixum_import_unavailable"], "diagnostics: {diagnostics:?}");
    }

    #[test]
    fn flag_on_leaves_every_initializer_unevaluated() {
        let unit = analyze_with(
            &format!(
                "{IMPORT}fixum numerus L = praefixum {{\n    redde 2 + 3\n}}\n\
                 fixum numerus G = praefixum {{\n    redde consts.seven()\n}}\n"
            ),
            true,
        )
        .unwrap_or_else(|diagnostics| panic!("deferral is not an error: {diagnostics:?}"));
        assert!(is_unevaluated(&unit, "G"));
        assert!(is_unevaluated(&unit, "L"), "a module defers all its sites or none");
    }

    #[test]
    fn local_only_comptime_evaluates_in_both_modes() {
        let source = format!("{IMPORT}fixum numerus L = praefixum {{\n    redde 2 + 3\n}}\n");
        for defer in [false, true] {
            let unit = analyze_with(&source, defer).unwrap_or_else(|diagnostics| panic!("{diagnostics:?}"));
            assert_eq!(int(&static_value(&unit, "L")), 5, "defer = {defer}");
        }
    }
}
