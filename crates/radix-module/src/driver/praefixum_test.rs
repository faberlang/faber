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
    let session = Session::new(Config::default().with_dev_locale_root());
    analyze_source(&session, "praefixum.fab", source, AnalyzeRequest::default())
}

fn analyze_ok(source: &str) -> AnalyzedModule {
    analyze(source).unwrap_or_else(|diagnostics| panic!("analysis failed: {diagnostics:?}"))
}

fn static_value(unit: &AnalyzedModule, name: &str) -> HirExpression {
    unit.hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if unit.interner.resolve(konst.name) == name => Some(konst.value.clone()),
            _ => None,
        })
        .expect("static item")
}

fn int(expr: &HirExpression) -> i128 {
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

fn arg<'a>(diagnostic: &'a Diagnostic, name: &str) -> Option<&'a str> {
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
         generis numerus SUMMA = praefixum {\n    varia numerus s ← 0\n    \
         itera ab 1‥4 fixum i {\n        s ← s + quadratum(i)\n    }\n    redde s\n}\n\
         incipit {\n    nota SUMMA\n}\n",
    );
    assert_eq!(int(&static_value(&unit, "SUMMA")), 14);
}

#[test]
fn dependent_statics_fold_after_evaluation() {
    let unit = analyze_ok(
        "generis numerus B = praefixum {\n    redde A * 2\n}\n\
         generis numerus C = B + 1\n\
         generis numerus A = praefixum {\n    redde 0 - 3\n}\n",
    );
    assert_eq!(int(&static_value(&unit, "A")), -3);
    assert_eq!(int(&static_value(&unit, "B")), -6);
    assert_eq!(int(&static_value(&unit, "C")), -5);
}

#[test]
fn list_result_is_an_array_literal() {
    let unit = analyze_ok(
        "generis lista<numerus> L = praefixum {\n    fixum lista<numerus> xs ← vacua\n    \
         itera ab 0‥3 fixum i {\n        xs.appende(i * 10)\n    }\n    redde xs\n}\n",
    );
    let HirExpressionKind::Array(elements) = static_value(&unit, "L").kind else {
        panic!("expected array literal");
    };
    let values: Vec<i128> = elements
        .iter()
        .map(|element| match element {
            HirArrayElement::Expr(expr) => int(expr),
            HirArrayElement::Spread(_) => panic!("unexpected spread"),
        })
        .collect();
    assert_eq!(values, [0, 10, 20]);
}

#[test]
fn genus_result_is_a_typed_construction_literal() {
    let unit = analyze_ok(
        "genus Punctum {\n    fixum numerus x\n    fixum numerus y\n}\n\
         generis Punctum P = praefixum {\n    redde Punctum { x = 2 * 3, y = 0 - 1 }\n}\n",
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
        "generis numerus N = praefixum {\n    si 1 ≡ 1 {\n        mori \"limes excessus\"\n    }\n    redde 1\n}\n",
        "praefixum_failed",
    );
    assert_eq!(arg(&diagnostic, "message"), Some("limes excessus"));
    assert_eq!(arg(&diagnostic, "name"), Some("N"));
}

#[test]
fn endless_loop_hits_the_step_limit() {
    let diagnostic = rejection(
        "generis numerus N = praefixum {\n    varia numerus i ← 0\n    dum verum {\n        i ← i + 1\n    }\n    redde i\n}\n",
        "praefixum_step_limit",
    );
    assert_eq!(arg(&diagnostic, "name"), Some("N"));
}

#[test]
fn faber_reemission_keeps_the_authored_body() {
    let session = Session::new(
        Config::default()
            .with_dev_locale_root()
            .with_target(Target::HirFaber),
    );
    let unit = analyze_source(
        &session,
        "praefixum.fab",
        "generis numerus N = praefixum {\n    redde 2 + 2\n}\n",
        AnalyzeRequest::default(),
    )
    .expect("analysis");
    assert!(matches!(static_value(&unit, "N").kind, HirExpressionKind::Praefixum(_)));
}
