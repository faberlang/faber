//! A1: numeric `+`, `-`, `*`, and `/` on lists are elementwise.
//!
//! Dynamic list lengths typecheck here; A2 owns the runtime equal-length check.

use crate::locale::LocalePack;
use crate::test_support::{analyze_ok, assert_accepts, assert_rejects_with_issue};
use crate::Type;
use radix_diagnostics::Diagnostic;
use radix_target::Target;

#[test]
fn numeric_lista_operators_are_elementwise_for_all_four_ops() {
    assert_accepts(
        r"functio addita(lista<numerus> a, lista<numerus> b) → lista<numerus> {
    redde a + b
}
functio subtrahe(lista<numerus> a, lista<numerus> b) → lista<numerus> {
    redde a - b
}
functio multiplica(lista<numerus> a, lista<numerus> b) → lista<numerus> {
    redde a * b
}
functio divide(lista<numerus> a, lista<numerus> b) → lista<numerus> {
    redde a / b
}",
    );
}

#[test]
fn numeric_lista_scalar_broadcasts_on_either_side() {
    assert_accepts(
        r"functio left(lista<numerus> xs, numerus scalar) → lista<numerus> {
    redde xs * scalar
}
functio right(lista<numerus> xs, numerus scalar) → lista<numerus> {
    redde scalar / xs
}",
    );
}

#[test]
fn numeric_lista_dynamic_lengths_are_deferred_to_a2() {
    // A1 does not reject unbounded list pairs: A2 owns the runtime check.
    assert_accepts(
        r"functio dynamic(lista<numerus> a, lista<numerus> b) → lista<numerus> {
    redde a + b
}",
    );
}

#[test]
fn numeric_lista_static_bounds_must_match() {
    assert_accepts(
        r"functio same(lista<numerus, 3> a, lista<numerus, 3> b) → lista<numerus, 3> {
    redde a + b
}",
    );
    assert_rejects_with_issue(
        r"functio mismatch(lista<numerus, 2> a, lista<numerus, 3> b) → lista<numerus, 2> {
    redde a + b
}",
        "numeric_lista_operator_law",
    );
}

#[test]
fn numeric_lista_width_mismatch_points_to_conversion() {
    assert_rejects_with_issue(
        r"functio mismatch(lista<numerus<i32>> a, lista<numerus<i64>> b) → lista<numerus<i32>> {
    redde a + b
}",
        "numeric_lista_operator_law",
    );
}

#[test]
fn numeric_lista_nested_elements_are_declined() {
    assert_rejects_with_issue(
        r"functio nested(lista<lista<numerus>> a, numerus scalar) → lista<lista<numerus>> {
    redde a + scalar
}",
        "numeric_lista_operator_law",
    );
}

#[test]
fn numeric_lista_multiplication_is_broadcast_not_repetition() {
    // The result remains a fresh list expression, not an integer repetition
    // count. A2 supplies the elementwise lowering; A1 only admits its type.
    let fixture = analyze_ok(
        r"incipit {
    fixum lista<numerus> xs ← [1, 2]
    fixum lista<numerus> scaled ← xs * 3
    nota scaled
}",
    );
    let entry = fixture.hir.as_ref().expect("entry HIR");
    let result_ty = entry
        .entry
        .as_ref()
        .expect("entry block")
        .statements
        .iter()
        .find_map(|statement| match &statement.kind {
            radix_hir::HirStatementKind::Local(local)
                if fixture.interner.resolve(local.name) == "scaled" =>
            {
                local.ty
            }
            _ => None,
        })
        .expect("scaled type");
    assert!(matches!(fixture.types.get(result_ty), Type::Array(_)));
}

#[test]
fn numeric_lista_operator_law_is_localized_in_en_and_la() {
    let diagnostic = Diagnostic::error("SEM011")
        .with_code("SEM011")
        .with_arg("issue", "numeric_lista_operator_law")
        .with_arg("operator", "+");
    for (locale, expected) in [
        (
            "en",
            "numeric list operator + is elementwise; scalars broadcast on either side, lists must have equal length; use ↦ for explicit width conversion",
        ),
        (
            "la",
            "operator listae numericae + per elementa agit; scalaria utrimque propagantur, listae parem longitudinem requirunt; utere ↦ ad latitudinem convertendam",
        ),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../stdlib/locale")
            .join(locale)
            .join("pack.toml");
        let pack = LocalePack::from_toml_path(path).expect("locale pack");
        let rendered = pack
            .render_diagnostic_text(&diagnostic)
            .expect("diagnostic template")
            .expect("SEM011 row");
        assert_eq!(rendered.message, expected, "locale {locale}");
    }
}

#[test]
fn numeric_lista_operator_law_uses_rust_semantic_pipeline() {
    let result = crate::test_support::analyze_source_for_target(
        "functio addita(lista<numerus> a, lista<numerus> b) → lista<numerus> { redde a + b }",
        Target::HirRust,
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}
