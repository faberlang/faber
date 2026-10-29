//! inf track U2: the unbounded integer `inf` as a declared value type.
//!
//! `inf` is `numerus<inf>`: accepted as a local, parameter, return, field and
//! list element, one type under all four spellings, and rejected by name where
//! it cannot live (tensor-family elements, kernels, the AIR lane). Its
//! arithmetic, literals and conversions are typed by U3 (`inf_typing_test.rs`).
//! Every rejection asserts the structured `kind` and `issue`, never prose.

use crate::test_support::{analyze_source_la, assert_accepts_la, assert_rejects_with_issue_la};
use crate::{SemanticError, SemanticErrorKind};

fn errors(source: &str) -> Vec<SemanticError> {
    analyze_source_la(source)
        .diagnostics
        .iter()
        .filter(|err| err.is_error())
        .cloned()
        .collect()
}

fn expect_issue(source: &str, kind: SemanticErrorKind, issue: &str) -> SemanticError {
    let errors = errors(source);
    errors
        .iter()
        .find(|err| err.kind == kind && err.issue() == Some(issue))
        .cloned()
        .unwrap_or_else(|| panic!("missing {kind:?} {issue} in {errors:?}"))
}

fn arg<'a>(err: &'a SemanticError, name: &str) -> Option<&'a str> {
    err.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

// ---------------------------------------------------------------------------
// Value flow
// ---------------------------------------------------------------------------

#[test]
fn inf_is_a_type_for_locals_params_returns_fields_and_list_elements() {
    assert_accepts_la(
        r"
genus Capsa {
    sit inf a
    varia inf b
}

functio transmitte(inf x) → inf {
    redde x
}

incipit {
    sit inf x ← 0
    sit inf y ← transmitte(x)
    sit Capsa c ← Capsa { a = y, b = x }
    sit lista<inf> xs ← [x, y]
    sit inf z ← xs[0]
    nota c.a
    nota z
}
",
    );
}

#[test]
fn the_four_spellings_are_one_type() {
    assert_accepts_la(
        r"
functio tere(inf a, exactus<inf> b, modulus<inf> c, saturatus<inf> d) → inf {
    sit exactus<inf> e ← a
    sit modulus<inf> f ← b
    sit saturatus<inf> g ← c
    sit inf h ← d
    redde h
}
",
    );
}

#[test]
fn numerus_inf_is_the_same_type_as_bare_inf() {
    assert_accepts_la(
        r"
functio tere(inf a) → inf {
    redde a
}
",
    );
}

#[test]
fn a_constant_lands_in_an_inf_slot() {
    assert_accepts_la(
        r"
incipit {
    sit inf x ← 0
    sit inf y ← -5
    nota x
    nota y
}
",
    );
}

#[test]
fn a_u64_sized_literal_lands_in_an_inf_slot_but_not_a_signed_one() {
    assert_accepts_la(
        r"
incipit {
    sit inf x ← 18446744073709551615
    nota x
}
",
    );
    assert_rejects_with_issue_la(
        "incipit { sit i64 x ← 18446744073709551615\n nota x }\n",
        "store_constant_out_of_range",
    );
}

#[test]
fn inf_values_join_in_lists_and_branches() {
    assert_accepts_la(
        r"
functio elige(bivalens b, inf x, inf y) → inf {
    redde b ✓ x ✗ y
}

incipit {
    sit inf x ← 1
    sit lista<inf> xs ← [x, 2, -3]
    nota elige(verum, x, 4)
    nota xs
}
",
    );
}

#[test]
fn every_bounded_integer_width_widens_into_inf() {
    for width in ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"] {
        assert_accepts_la(&format!(
            r"
functio amplia({width} x) → inf {{
    sit inf y ← x
    redde y
}}
"
        ));
    }
}

/// Ruling 38 (inf track U3): `inf` widens into no bounded integer, but a
/// store of an `inf` into a bounded trapping slot is the implicit checked
/// narrowing (a trap, never `⇥`), so the slot accepts it.
#[test]
fn an_inf_into_a_bounded_slot_is_the_implicit_checked_narrowing() {
    assert_accepts_la(
        r"
functio contrahe(inf x) → i64 {
    sit i64 y ← x
    redde y
}
",
    );
}

#[test]
fn inf_compares_and_is_still_an_ordinary_identifier() {
    assert_accepts_la(
        r"
functio minor(inf a, inf b) → bivalens {
    redde a ≺ b
}

incipit {
    sit numerus inf ← 3
    nota inf
}
",
    );
}

// ---------------------------------------------------------------------------
// Spelling: no prefixed sugar, no float `inf`
// ---------------------------------------------------------------------------

#[test]
fn prefixed_inf_spellings_are_not_types() {
    for sugar in ["linf", "tinf", "vinf", "minf", "sinf"] {
        let err = expect_issue(
            &format!("functio f({sugar} x) → inf {{ redde 0 }}\n"),
            SemanticErrorKind::TypeUndefined,
            "unknown_type",
        );
        assert!(err.args.iter().any(|arg| arg.name == "issue"));
    }
}

#[test]
fn a_prefixed_inf_name_is_an_ordinary_identifier() {
    assert_accepts_la(
        r"
incipit {
    sit numerus linf ← 1
    sit numerus sinf ← 2
    nota linf + sinf
}
",
    );
}

#[test]
fn fractus_inf_is_a_retired_wrapper() {
    // `inf` is an integer-family marker; the wrapped `fractus<inf>` is gone
    // with the wrapped numeric spelling (N7d).
    expect_issue(
        "functio f(fractus<inf> x) → inf { redde 0 }\n",
        SemanticErrorKind::LoweringError,
        "numeric_wrapper_retired",
    );
}

// ---------------------------------------------------------------------------
// Rejections by name (ruling 39)
// ---------------------------------------------------------------------------

#[test]
fn a_tensor_family_element_cannot_be_inf() {
    for (ty, what) in [
        ("tensor<inf, [2]>", "tensor"),
        ("vector<inf, magnitudo 4>", "vector"),
        ("matrix<inf, [2, 2]>", "matrix"),
        ("sparsa<inf, [2]>", "sparsa"),
    ] {
        let err = expect_issue(
            &format!("functio f({ty} t) → numerus {{ redde 0 }}\n"),
            SemanticErrorKind::LoweringError,
            "tensor_element_unbounded",
        );
        assert!(
            err.args.iter().any(|arg| arg.name == "issue"),
            "{what} must carry the structured issue"
        );
    }
}

#[test]
fn a_kernel_inf_parameter_is_host_only() {
    let err = expect_issue(
        r"
@ nucleum
@ publica
functio k(inf x) → u32 {
    redde 1
}
",
        SemanticErrorKind::LoweringError,
        "nucleum_host_type",
    );
    assert_eq!(arg(&err, "family"), Some("inf"));
    assert_eq!(arg(&err, "site"), Some("parameter"));
}

#[test]
fn a_kernel_inf_local_is_host_only() {
    let err = expect_issue(
        r"
@ nucleum
@ publica
functio k(u32 x) → u32 {
    sit inf y ← 1
    redde x
}
",
        SemanticErrorKind::LoweringError,
        "nucleum_host_type",
    );
    assert_eq!(arg(&err, "family"), Some("inf"));
}

#[test]
fn an_air_lane_inf_local_is_rejected() {
    let err = expect_issue(
        r#"
@ radix lane "air"
functio pura(i32 n) → i32 {
    sit inf x ← 1
    redde n
}
"#,
        SemanticErrorKind::AirPurityViolation,
        "air_unbounded_integer",
    );
    assert_eq!(arg(&err, "lane"), Some("air"));
}

#[test]
fn an_air_lane_inf_parameter_and_return_are_rejected() {
    let source = r#"
@ radix lane "air"
functio pura(inf n) → inf {
    redde n
}
"#;
    let rejected = errors(source)
        .iter()
        .filter(|err| {
            err.kind == SemanticErrorKind::AirPurityViolation
                && err.issue() == Some("air_unbounded_integer")
        })
        .count();
    assert!(rejected >= 2, "parameter and return sites: {rejected}");
}
