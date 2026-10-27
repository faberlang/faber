//! Typecheck probes for exact-type equality (`≡`, `≢`).
//!
//! `est` / `non est` is a type test, not an equality operator (DECISIONS
//! D23.1); its value-target rejection is pinned here too.

use crate::SemanticErrorKind;

const EXACT_EQ_ISSUE: &str = "equality_exact_types_required";

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_la(source)
}

enum EqualityExpect {
    Accepts,
    RejectsIssue(&'static str),
    /// A rejection outside the equality family: the issue under its own kind.
    RejectsIssueKind(SemanticErrorKind, &'static str),
}

struct EqualityCase {
    name: &'static str,
    source: &'static str,
    expect: EqualityExpect,
}

fn equality_exact_types_cases() -> Vec<EqualityCase> {
    vec![
        EqualityCase {
            name: "ascii_textus_eq_rejects",
            source: "incipit { si \"a\" ≡ 'a' { nota \"bad\" } }",
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        EqualityCase {
            name: "ascii_textus_neq_rejects",
            source: "incipit { si 'a' ≢ \"a\" { nota \"bad\" } }",
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        EqualityCase {
            name: "ascii_textus_explicit_widen_accepts",
            source: "incipit { si 'a' ↦ textus ≡ \"a\" { nota \"ok\" } }",
            expect: EqualityExpect::Accepts,
        },
        EqualityCase {
            name: "instans_cross_precision_eq_rejects",
            source: r"
functio same(instans<ns> fine, instans<ms> coarse) → bivalens {
    redde fine ≡ coarse
}
",
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        EqualityCase {
            name: "instans_cross_precision_order_accepts",
            source: r"
functio before(instans<ns> fine, instans<ms> coarse) → bivalens {
    redde fine ≺ coarse
}
",
            expect: EqualityExpect::Accepts,
        },
        EqualityCase {
            name: "instans_explicit_precision_widen_accepts",
            source: r"
functio same(instans<ns> fine, instans<ms> coarse) → bivalens {
    redde fine ↦ instans<ms> ≡ coarse
}
",
            expect: EqualityExpect::Accepts,
        },
        EqualityCase {
            name: "numerus_fractus_eq_rejects",
            source: "incipit { si 1 ≡ 1.0 { nota \"bad\" } }",
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        EqualityCase {
            name: "sized_numerus_cross_width_eq_rejects",
            source: r#"
incipit {
    sit u8 narrow ← 1 ∷ u8
    sit numerus wide ← 1
    si narrow ≡ wide { nota "bad" }
}
"#,
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        // decimal-widths D3: within-width decimal equality is exact and
        // accepted.
        EqualityCase {
            name: "sized_numerus_d64_within_width_eq_accepts",
            source: r#"
incipit {
    sit d64 a ← 4.2
    si a ≡ a { nota "ok" }
}
"#,
            expect: EqualityExpect::Accepts,
        },
        EqualityCase {
            name: "decimal_integer_cross_family_eq_rejects",
            source: r#"
incipit {
    sit d64 a ← 4.2
    si a ≡ 1 { nota "bad" }
}
"#,
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        EqualityCase {
            name: "decimal_fractus_cross_family_eq_rejects",
            source: r#"
incipit {
    sit d64 a ← 4.2
    si a ≡ 4.2 { nota "bad" }
}
"#,
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        EqualityCase {
            // `octeti` is exactly `lista<b8>` — one type.
            name: "octeti_lista_eq_accepts",
            source: r#"
incipit {
    sit lista<b8> raw ← |41|
    sit lista<b8> bytes ← |41|
    si raw ≡ bytes { nota "same" }
}
"#,
            expect: EqualityExpect::Accepts,
        },
        EqualityCase {
            // Bits are not numbers (ruling B1): `lista<b8>` and `lista<u8>`
            // are different types, so exact equality refuses the pair.
            name: "octeti_lista_u8_eq_rejects",
            source: r#"
incipit {
    sit lista<b8> raw ← |41|
    sit lista<u8> bytes ← [0x41 ∷ u8]
    si raw ≡ bytes { nota "bad" }
}
"#,
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        EqualityCase {
            name: "ascii_index_numerus_eq_rejects",
            source: r#"
incipit {
    sit textus s ← "ab"
    si s[0] ≡ 97 { nota "bad" }
}
"#,
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        EqualityCase {
            name: "same_type_baseline",
            source: "incipit { si 1 ≡ 1 { nota \"ok\" } }",
            expect: EqualityExpect::Accepts,
        },
        EqualityCase {
            name: "nihil_bypass",
            source: "incipit { si nulla ≡ nulla { nota \"ok\" } }",
            expect: EqualityExpect::Accepts,
        },
        EqualityCase {
            name: "est_value_rhs_rejects",
            // `est` tests a type (D23.1); a value right-hand side is rejected
            // by name rather than compared.
            source: "incipit {\n    sit numerus unus ← 1\n    si unus est unus { nota \"bad\" }\n}",
            expect: EqualityExpect::RejectsIssueKind(
                SemanticErrorKind::OperandTypesInvalid,
                "est_value_rhs",
            ),
        },
        EqualityCase {
            name: "unrelated_types_reject",
            source: "incipit { si \"x\" ≡ 1 { nota \"bad\" } }",
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
    ]
}

#[test]
fn equality_exact_types_matrix() {
    for case in equality_exact_types_cases() {
        let result = compile(case.source);
        match case.expect {
            EqualityExpect::Accepts => {
                assert!(
                    result.is_ok(),
                    "case {}: {:?}",
                    case.name,
                    result.diagnostics
                );
            }
            EqualityExpect::RejectsIssue(issue) => {
                assert!(
                    !result.is_ok(),
                    "case {} should fail: {:?}",
                    case.name,
                    result.diagnostics
                );
                assert!(
                    result.diagnostics.iter().any(|diag| {
                        // Live SEM010 naming: the exact-types equality error is
                        // SemanticErrorKind::TypeMismatch (catalog_semantic.rs),
                        // with the issue tag as its sub-variant.
                        diag.kind == SemanticErrorKind::TypeMismatch
                            && diag
                                .args
                                .iter()
                                .any(|arg| arg.name == "issue" && arg.value == issue)
                    }),
                    "case {}: expected diagnostic issue {issue:?} as SEM010 TypeMismatch, got: {:?}",
                    case.name,
                    result.diagnostics
                );
            }
            EqualityExpect::RejectsIssueKind(kind, issue) => {
                assert!(
                    !result.is_ok(),
                    "case {} should fail: {:?}",
                    case.name,
                    result.diagnostics
                );
                assert!(
                    result.diagnostics.iter().any(|diag| {
                        diag.kind == kind
                            && diag
                                .args
                                .iter()
                                .any(|arg| arg.name == "issue" && arg.value == issue)
                    }),
                    "case {}: expected diagnostic issue {issue:?} as {kind:?}, got: {:?}",
                    case.name,
                    result.diagnostics
                );
            }
        }
    }
}

#[test]
fn ref_view_field_read_exact_equality_uses_carrier_type() {
    let result = compile(
        r#"
genus Euler {
    varia textus order
}

functio same(de Euler value) → bivalens {
    redde value.order ≢ "XYZ"
}
"#,
    );
    assert!(
        result.is_ok(),
        "ref field read equality should compare its carrier: {:?}",
        result.diagnostics
    );
}

#[test]
fn non_view_field_read_keeps_exact_equality_rejection() {
    let result = compile(
        r"
genus Euler {
    varia textus order
}

functio same(Euler value) → bivalens {
    redde value.order ≢ 'X'
}
",
    );
    assert!(
        result.diagnostics.iter().any(|diag| {
            diag.kind == SemanticErrorKind::TypeMismatch
                && diag
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == EXACT_EQ_ISSUE)
        }),
        "non-view field equality should retain exact-type rejection: {:?}",
        result.diagnostics
    );
}

#[test]
fn direct_ref_value_exact_equality_keeps_view_contract() {
    let result = compile(
        r#"
functio same(de textus value) → bivalens {
    redde value ≢ "XYZ"
}
"#,
    );
    assert!(
        result.diagnostics.iter().any(|diag| {
            diag.kind == SemanticErrorKind::TypeMismatch
                && diag
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == EXACT_EQ_ISSUE)
        }),
        "direct ref equality should retain the view: {:?}",
        result.diagnostics
    );
}

#[test]
fn ascii_numerus_equality_rejects_with_sem010() {
    let result = compile(
        r#"
incipit {
    sit textus s ← "ab"
    si s[0] ≡ 97 { nota "bad" }
}
"#,
    );
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    let diag = result
        .diagnostics
        .iter()
        .find(|diag| diag.kind == SemanticErrorKind::TypeMismatch)
        .expect("s[i] ≡ 97 must produce a SEM010 TypeMismatch diagnostic");
    assert_eq!(diag.issue(), Some(EXACT_EQ_ISSUE));
}

/// A `copy` / `own` binding marker is a binding fact, not part of the value's
/// type: `≡` / `≢` compare the carrier.
#[test]
fn value_marker_bindings_keep_exact_equality_accepting() {
    let cases: [(&str, &str); 6] = [
        (
            "copy_eq",
            "genus Point { varia numerus x }\nfunctio f(Point a) → bivalens { varia Point d ← copy a\nredde a ≡ d }",
        ),
        (
            "copy_neq_reversed",
            "genus Point { varia numerus x }\nfunctio f(Point a) → bivalens { varia Point d ← copy a\nredde d ≢ a }",
        ),
        (
            "own_eq",
            "genus Point { varia numerus x }\nfunctio f(Point a, Point b) → bivalens { varia Point d ← own b\nredde a ≡ d }",
        ),
        (
            "own_neq_reversed",
            "genus Point { varia numerus x }\nfunctio f(Point a, Point b) → bivalens { varia Point d ← own b\nredde d ≢ a }",
        ),
        (
            "copy_both_sides",
            "genus Point { varia numerus x }\nfunctio f(Point a) → bivalens { varia Point d ← copy a\nvaria Point e ← copy a\nredde d ≡ e }",
        ),
        (
            "optional_copy_binding",
            "genus Point { varia numerus x }\nfunctio f(Point ∪ nihil a) → bivalens { varia Point ∪ nihil d ← copy a\nredde a ≡ d }",
        ),
    ];
    for (name, source) in cases {
        let result = compile(source);
        assert!(
            result.is_ok(),
            "case {name}: marker binding must not reject ≡/≢: {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn value_marker_binding_keeps_distinct_type_rejection() {
    let result = compile(
        "genus Point { varia numerus x }\ngenus Rect { varia numerus w }\nfunctio f(Point a, Rect r) → bivalens { varia Point d ← copy a\nredde d ≡ r }",
    );
    assert!(
        result.diagnostics.iter().any(|diag| {
            diag.kind == SemanticErrorKind::TypeMismatch
                && diag
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == EXACT_EQ_ISSUE)
        }),
        "Point ≡ Rect must still reject: {:?}",
        result.diagnostics
    );
}

/// A sized integer operand beside an untyped integer constant takes part by
/// value inside a kernel exactly as on the host (gpu-reset K3, need):
/// the host exemption is not device-dependent. A float constant is not
/// exempt, so exact-type discipline still binds in a kernel.
#[test]
fn kernel_sized_int_constant_equality_takes_part_by_value() {
    let eq = compile("@ nucleum\nfunctio k(i32 kind) → bivalens {\n    redde kind ≡ 1\n}\n");
    assert!(
        eq.is_ok(),
        "kernel `kind ≡ 1` must take the constant by value: {:?}",
        eq.diagnostics
    );
    let neq = compile("@ nucleum\nfunctio k(i32 kind) → bivalens {\n    redde kind ≢ 0\n}\n");
    assert!(
        neq.is_ok(),
        "kernel `kind ≢ 0` must take the constant by value: {:?}",
        neq.diagnostics
    );
    let float = compile("@ nucleum\nfunctio k(i32 kind) → bivalens {\n    redde kind ≡ 1.5\n}\n");
    assert!(
        !float.is_ok(),
        "kernel `kind ≡ 1.5` must stay an exact-type error: {:?}",
        float.diagnostics
    );
    assert_eq!(
        float
            .diagnostics
            .iter()
            .find(|diag| diag.kind == SemanticErrorKind::TypeMismatch)
            .and_then(|diag| diag.issue()),
        Some(EXACT_EQ_ISSUE)
    );
}
