//! A1: numeric `+`, `-`, `*`, and `/` on lists are elementwise.
//!
//! Dynamic list lengths typecheck here; A2 owns the runtime equal-length check.

use crate::Type;
use crate::locale::LocalePack;
use crate::test_support::{analyze_ok, assert_accepts, assert_rejects_with_issue};
use radix_diagnostics::Diagnostic;
use radix_target::Target;

#[test]
fn numeric_lista_add_and_sub_pair_elementwise() {
    assert_accepts(
        r"functio addita(lista<numerus> a, lista<numerus> b) → lista<numerus> {
    redde a + b
}
functio subtrahe(lista<numerus> a, lista<numerus> b) → lista<numerus> {
    redde a - b
}",
    );
}

// 2026-10-04 operator ruling: `*` and `/` between two lists are errors that
// name the glyphs, as for a vector or a tensor (this reverses the earlier A1
// list-list `*` and `/`).
#[test]
fn numeric_lista_multiply_and_divide_between_lists_name_the_glyphs() {
    assert_rejects_with_issue(
        r"functio multiplica(lista<numerus> a, lista<numerus> b) → lista<numerus> {
    redde a * b
}",
        "lista_multiply_requires_glyph",
    );
    assert_rejects_with_issue(
        r"functio divide(lista<numerus> a, lista<numerus> b) → lista<numerus> {
    redde a / b
}",
        "lista_divide_requires_glyph",
    );
}

#[test]
fn numeric_lista_hadamard_glyphs_pair_elementwise() {
    assert_accepts(
        r"functio multiplica(lista<numerus> a, lista<numerus> b) → lista<numerus> {
    redde a ⊙ b
}
functio divide(lista<numerus> a, lista<numerus> b) → lista<numerus> {
    redde a ⊘ b
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
        "lista_capacity_mismatch",
    );
}

#[test]
fn numeric_lista_width_mismatch_points_to_conversion() {
    assert_rejects_with_issue(
        r"functio mismatch(lista<i32> a, lista<i64> b) → lista<i32> {
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
        let path = crate::locale::workspace_locale_pack_path(locale);
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

// ---- 2026-10-04 aggregate operator rulings --------------------------------

#[test]
fn numeric_lista_untyped_literal_adopts_the_element_type() {
    // A sized float element takes a float literal on either side (the law
    // used to reject `xs * 2.0` for every element but `fractus`).
    assert_accepts(
        r"functio left(lista<f32> xs) → lista<f32> {
    redde xs * 2.0
}
functio right(lista<f32> xs) → lista<f32> {
    redde 0.5 - xs
}
functio ints(lista<i32> xs) → lista<i32> {
    redde 2 * xs + 1
}
functio bytes(lista<u8> xs) → lista<u8> {
    redde xs + 1
}",
    );
}

#[test]
fn numeric_lista_constant_joins_by_value_like_a_tensor() {
    // `xs + 300` cannot stay `u8`; the element joins to `u16` (F9).
    assert_accepts(
        r"functio wide(lista<u8> xs) → lista<u16> {
    redde xs + 300
}",
    );
    assert_rejects_with_issue(
        r"functio narrow(lista<u8> xs) → lista<u8> {
    redde xs + 300
}",
        "return_type_mismatch",
    );
}

#[test]
fn numeric_lista_remainder_takes_a_scalar_on_either_side() {
    assert_accepts(
        r"functio left(lista<numerus> xs, numerus n) → lista<numerus> {
    redde xs % n
}
functio right(lista<numerus> xs, numerus n) → lista<numerus> {
    redde n % xs
}",
    );
    assert_rejects_with_issue(
        r"functio pairs(lista<numerus> a, lista<numerus> b) → lista<numerus> {
    redde a % b
}",
        "lista_scalar_required",
    );
}

#[test]
fn numeric_lista_true_division_takes_its_width_from_ruling_23() {
    assert_accepts(
        r"functio ints(lista<numerus> a, lista<numerus> b) → lista<fractus> {
    redde a ÷ b
}
functio scalar(lista<numerus> a) → lista<fractus> {
    redde a ÷ 2
}
functio left(lista<numerus> a) → lista<fractus> {
    redde 7 ÷ a
}
functio narrow(lista<i16> a) → lista<f32> {
    redde a ÷ a
}
functio floats(lista<f32> a) → lista<f32> {
    redde a ÷ 2.0
}",
    );
    assert_rejects_with_issue(
        r"functio kept(lista<numerus> a) → lista<numerus> {
    redde a ÷ 2
}",
        "return_type_mismatch",
    );
}

#[test]
fn numeric_lista_unary_minus_keeps_the_list() {
    assert_accepts(
        r"functio negated(lista<numerus> xs) → lista<numerus> {
    redde -xs
}
functio bounded(lista<f32, 4> xs) → lista<f32, 4> {
    redde -xs
}",
    );
    assert_rejects_with_issue(
        r"functio nested(lista<lista<numerus>> xs) → lista<lista<numerus>> {
    redde -xs
}",
        "numeric_operand_required",
    );
}

#[test]
fn numeric_lista_glyphs_reject_scalars_and_mixed_elements() {
    assert_rejects_with_issue(
        r"functio scalar(lista<numerus> xs) → lista<numerus> {
    redde xs ⊙ 2
}",
        "glyph_scalar_operand",
    );
    assert_rejects_with_issue(
        r"functio mixed(lista<i32> a, lista<i64> b) → lista<i32> {
    redde a ⊙ b
}",
        "glyph_element_type_mismatch",
    );
}

#[test]
fn numeric_lista_capacity_is_part_of_the_type_for_every_pairing_form() {
    assert_rejects_with_issue(
        r"functio hadamard(lista<numerus, 4> a, lista<numerus, 8> b) → lista<numerus, 4> {
    redde a ⊙ b
}",
        "lista_capacity_mismatch",
    );
    assert_rejects_with_issue(
        r"functio quotient(lista<f32, 4> a, lista<f32, 8> b) → lista<f64, 4> {
    redde a ÷ b
}",
        "lista_capacity_mismatch",
    );
    assert_accepts(
        r"functio same(lista<numerus, 4> a, lista<numerus, 4> b) → lista<numerus, 4> {
    redde a ⊙ b
}
functio scalar(lista<numerus, 4> a) → lista<numerus, 4> {
    redde a % 3
}",
    );
}

#[test]
fn lista_arithmetic_diagnostics_are_localized_in_en_and_la() {
    let cases: [(&str, Vec<(&str, &str)>, &str, &str); 4] = [
        (
            "lista_multiply_requires_glyph",
            vec![],
            "list `*` list is unsupported; use ⊙ (elementwise)",
            "lista `*` lista non licet; utere ⊙ (elementwise)",
        ),
        (
            "lista_divide_requires_glyph",
            vec![],
            "list `/` list is unsupported; use ⊘ (elementwise, like `/`) or ÷ (elementwise true division)",
            "lista `/` lista non licet; utere ⊘ (elementwise, ut `/`) aut ÷ (divisio vera per elementa)",
        ),
        (
            "lista_scalar_required",
            vec![("operator", "%")],
            "list `%` list is unsupported; `%` takes a scalar on one side",
            "lista `%` lista non licet; `%` scalare ex una parte requirit",
        ),
        (
            "lista_capacity_mismatch",
            vec![("operator", "+"), ("lhs", "4"), ("rhs", "8")],
            "list `+` needs equal capacities; found capacity 4 and capacity 8",
            "lista `+` capacitates pares requirit; inventae sunt capacitas 4 et capacitas 8",
        ),
    ];
    for (issue, args, en, la) in cases {
        for (locale, expected) in [("en", en), ("la", la)] {
            let mut diagnostic = Diagnostic::error("SEM011")
                .with_code("SEM011")
                .with_arg("issue", issue);
            for (name, value) in &args {
                diagnostic = diagnostic.with_arg(name, *value);
            }
            let path = crate::locale::workspace_locale_pack_path(locale);
            let pack = LocalePack::from_toml_path(path).expect("locale pack");
            let rendered = pack
                .render_diagnostic_text(&diagnostic)
                .expect("diagnostic template")
                .expect("SEM011 row");
            assert_eq!(rendered.message, expected, "{issue} in {locale}");
        }
    }
}

// 2026-10-04 operator ruling, kept 2026-10-10: the ordering
// glyphs and `≅ ≇` lift per element over a numeric list, like `+`.
#[test]
fn numeric_lista_comparisons_lift_to_a_list_of_bivalens() {
    assert_accepts(
        r"functio ordered(lista<numerus> a, lista<numerus> b) → lista<bivalens> {
    redde a ≺ b
}
functio above(lista<f64> a) → lista<bivalens> {
    redde a ≻ 2.0
}
functio scalar_left(lista<numerus> a) → lista<bivalens> {
    redde 3 ≤ a
}
functio equal(lista<numerus> a, lista<numerus> b) → lista<bivalens> {
    redde a ≅ b
}
functio apart(lista<numerus> a, lista<numerus> b) → lista<bivalens> {
    redde a ≇ b
}
functio bounded(lista<numerus, 4> a, lista<numerus, 4> b) → lista<bivalens, 4> {
    redde a ≥ b
}",
    );
}

#[test]
fn numeric_lista_comparison_result_is_not_a_single_bivalens() {
    assert_rejects_with_issue(
        r"functio single(lista<numerus> a, lista<numerus> b) → bivalens {
    redde a ≺ b
}",
        "return_type_mismatch",
    );
}

#[test]
fn numeric_lista_whole_value_equality_stays_one_bivalens() {
    assert_accepts(
        r"functio same(lista<numerus> a, lista<numerus> b) → bivalens {
    redde a ≡ b
}
functio text_equal(lista<textus> a, lista<textus> b) → bivalens {
    redde a ≅ b
}",
    );
}

#[test]
fn numeric_lista_comparison_keeps_the_capacity_law() {
    assert_rejects_with_issue(
        r"functio mismatched(lista<numerus, 4> a, lista<numerus, 8> b) → lista<bivalens, 4> {
    redde a ≺ b
}",
        "lista_capacity_mismatch",
    );
}

#[test]
fn lista_of_bits_orders_with_whole_value_equality_only() {
    assert_rejects_with_issue(
        r"functio ordered(lista<b8> a, lista<b8> b) → lista<bivalens> {
    redde a ≺ b
}",
        "lista_bits_whole_equality_only",
    );
    assert_accepts(
        r"functio same(lista<b8> a, lista<b8> b) → bivalens {
    redde a ≡ b
}",
    );
}

#[test]
fn non_numeric_lista_ordering_is_still_refused() {
    assert_rejects_with_issue(
        r"functio ordered(lista<textus> a, lista<textus> b) → lista<bivalens> {
    redde a ≺ b
}",
        "numeric_operands_required",
    );
}

#[test]
fn lista_bits_whole_equality_only_is_localized_in_en_and_la() {
    let diagnostic = Diagnostic::error("SEM011")
        .with_code("SEM011")
        .with_arg("issue", "lista_bits_whole_equality_only");
    for locale in ["en", "la"] {
        let path = crate::locale::workspace_locale_pack_path(locale);
        let pack = LocalePack::from_toml_path(path).expect("locale pack");
        let rendered = pack
            .render_diagnostic_text(&diagnostic)
            .expect("diagnostic template")
            .expect("SEM011 row");
        assert!(
            rendered.message.contains(match locale {
                "en" => "whole-value equality only",
                _ => "aequalitatem totius valoris tantum",
            }),
            "{locale}: {}",
            rendered.message
        );
    }
}

// Need A5 (`approx-aggregate-named-reject`): `≈ ≉` over a list, set,
// map or other aggregate that is not a numeric tensor is a named check error,
// never an `invalid MIR` failure at run time.
#[test]
fn approx_over_non_numeric_lists_is_a_named_check_error() {
    for (element, glyph) in [("textus", "≈"), ("bivalens", "≉"), ("lista<numerus>", "≈")] {
        assert_rejects_with_issue(
            &format!(
                r"functio near(lista<{element}> a, lista<{element}> b) → bivalens {{
    redde a {glyph} b
}}"
            ),
            "approx_requires_numeric_operands",
        );
    }
}

#[test]
fn approx_over_map_and_set_is_a_named_check_error() {
    assert_rejects_with_issue(
        r"functio near(tabula<textus, numerus> a, tabula<textus, numerus> b) → bivalens {
    redde a ≈ b
}",
        "approx_requires_numeric_operands",
    );
    assert_rejects_with_issue(
        r"functio near(copia<numerus> a, copia<numerus> b) → bivalens {
    redde a ≈ b
}",
        "approx_requires_numeric_operands",
    );
}

#[test]
fn approx_over_numeric_lists_keeps_the_deferral_name() {
    assert_rejects_with_issue(
        r"functio near(lista<fractus> a, lista<fractus> b) → bivalens {
    redde a ≈ b
}",
        "tensor_approx_comparison_deferred",
    );
}

#[test]
fn approx_over_scalars_still_checks() {
    assert_accepts(
        r"functio near(fractus a, fractus b) → bivalens {
    redde a ≈ b
}",
    );
}

#[test]
fn approx_requires_numeric_operands_is_localized_in_every_pack() {
    let diagnostic = Diagnostic::error("SEM011")
        .with_code("SEM011")
        .with_arg("issue", "approx_requires_numeric_operands")
        .with_arg("op", "≈")
        .with_arg("kind", "list");
    for locale in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let path = crate::locale::workspace_locale_pack_path(locale);
        let pack = LocalePack::from_toml_path(path).expect("locale pack");
        let rendered = pack
            .render_diagnostic_text(&diagnostic)
            .expect("diagnostic template")
            .expect("SEM011 row");
        assert!(
            rendered.message.contains('≈') && !rendered.message.contains("{op}"),
            "{locale}: {}",
            rendered.message
        );
    }
}

// Q-C of the same ruling: `et` / `aut` / `non` lift per element over
// `lista<bivalens>` and keep the list shape.
#[test]
fn bool_list_logic_lifts_per_element_and_keeps_the_list_type() {
    assert_accepts(
        r"functio both(lista<bivalens> a, lista<bivalens> b) → lista<bivalens> {
    redde a et b
}
functio either(lista<bivalens> a, lista<bivalens> b) → lista<bivalens> {
    redde a aut b
}
functio flip(lista<bivalens> a) → lista<bivalens> {
    redde non a
}
functio mixed(lista<bivalens> a, bivalens flag) → lista<bivalens> {
    redde flag et a
}",
    );
}

#[test]
fn bool_list_logic_refuses_non_bool_elements_by_name() {
    assert_rejects_with_issue(
        r"functio both(lista<numerus> a, lista<bivalens> b) → lista<bivalens> {
    redde a et b
}",
        "boolean_operands_required",
    );
    assert_rejects_with_issue(
        r"functio flip(lista<numerus> a) → lista<bivalens> {
    redde non a
}",
        "boolean_operand_required",
    );
}

#[test]
fn bool_list_logic_result_is_not_a_scalar_bool() {
    assert_rejects_with_issue(
        r"functio both(lista<bivalens> a, lista<bivalens> b) → bivalens {
    redde a et b
}",
        "return_type_mismatch",
    );
}

#[test]
fn numeric_lista_comparison_capacity_mismatch_adds_no_scalar_operand_diagnostic() {
    for glyph in ["≺", "≅"] {
        let source = format!(
            "functio mismatched(lista<numerus, 4> a, lista<numerus, 8> b) → lista<bivalens, 4> {{
    redde a {glyph} b
}}"
        );
        let issues: Vec<String> = crate::test_support::analyze_and_get_diagnostics(&source)
            .iter()
            .filter(|err| err.is_error())
            .filter_map(|err| crate::test_support::issue(err).map(str::to_owned))
            .collect();
        assert_eq!(
            issues
                .iter()
                .filter(|issue| *issue == "lista_capacity_mismatch")
                .count(),
            1,
            "`{glyph}` over mismatched capacities: {issues:?}"
        );
        assert!(
            !issues
                .iter()
                .any(|issue| issue == "numeric_operands_required"),
            "`{glyph}` over mismatched capacities fell through to the scalar path: {issues:?}"
        );
    }
}
