//! Typecheck probes for numeric value equality (`≅`, `≇`).

const EXACT_EQ_ISSUE: &str = "equality_exact_types_required";
const NUMERIC_VALUE_EQ_ISSUE: &str = "numeric_value_equality_operands_required";

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_la(source)
}

enum ValueEqualityExpect {
    Accepts,
    RejectsIssue(&'static str),
}

struct ValueEqualityCase {
    name: &'static str,
    source: &'static str,
    expect: ValueEqualityExpect,
}

fn numeric_value_equality_cases() -> Vec<ValueEqualityCase> {
    vec![
        ValueEqualityCase {
            name: "u8_default_numerus_approx_accepts",
            source: r"
incipit {
    fixum u8 port ← 42 ∷ u8
    fixum numerus count ← 42
    adfirma port ≅ count
}
",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "u8_default_numerus_exact_rejects",
            source: r"
incipit {
    fixum u8 port ← 42 ∷ u8
    fixum numerus count ← 42
    adfirma port ≡ count
}
",
            expect: ValueEqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        ValueEqualityCase {
            name: "numerus_fractus_approx_accepts",
            source: "incipit { adfirma 1 ≅ 1.0 }",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "numerus_fractus_exact_rejects",
            source: "incipit { si 1 ≡ 1.0 { nota \"bad\" } }",
            expect: ValueEqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        ValueEqualityCase {
            name: "fractus_approx_typechecks",
            source: "incipit { adfirma 42.145 ∷ f64 ≅ 42.0 ∷ f64 }",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "instans_cross_approx_accepts",
            source: r"
functio same(instans<ns> fine, instans<ms> coarse) → bivalens {
    redde fine ≅ coarse
}
",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "instans_cross_approx_not_eq_accepts",
            source: r"
functio same(instans<ns> fine, instans<ms> coarse) → bivalens {
    redde fine ≇ coarse
}
",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "instans_cross_fuzzy_rejects",
            source: r"
functio same(instans<ns> fine, instans<ms> coarse) → bivalens {
    redde fine ≈ coarse
}
",
            expect: ValueEqualityExpect::RejectsIssue(NUMERIC_VALUE_EQ_ISSUE),
        },
        ValueEqualityCase {
            name: "instans_cross_exact_rejects",
            source: r"
functio same(instans<ns> fine, instans<ms> coarse) → bivalens {
    redde fine ≡ coarse
}
",
            expect: ValueEqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        ValueEqualityCase {
            name: "ascii_textus_approx_rejects",
            source: "incipit { si 'a' ≅ \"a\" { nota \"bad\" } }",
            expect: ValueEqualityExpect::RejectsIssue(NUMERIC_VALUE_EQ_ISSUE),
        },
        ValueEqualityCase {
            name: "signed_unsigned_approx_accepts",
            source: r"
incipit {
    fixum i32 signed ← -1
    fixum u8 unsigned ← 255 ∷ u8
    adfirma signed ≅ unsigned
}
",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "cross_width_params_approx_accepts",
            source: r"
functio promoted(numerus left, u8 right) → bivalens {
    redde left ≅ right
}
",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "approx_not_eq_accepts",
            source: "incipit { adfirma 1 ≇ 2 }",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "fuzzy_near_values_typecheck",
            source: "incipit { adfirma 42.0 ≈ 42.00000001 }",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "fuzzy_not_eq_typecheck",
            source: "incipit { adfirma 42.0 ≉ 42.145 }",
            expect: ValueEqualityExpect::Accepts,
        },
        // E1 contract rows (equality-tier-respelling): the widened-join
        // admission law across the i64/u64/f64 carrier pairs.
        ValueEqualityCase {
            name: "i64_u64_promoted_exact_accepts",
            source: "incipit { adfirma 1 ∷ i64 ≅ 1 ∷ u64 }",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "i64_f64_promoted_exact_accepts",
            source: "incipit { adfirma 1 ∷ i64 ≅ 1.0 ∷ f64 }",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "u64_f64_promoted_exact_accepts",
            source: "incipit { adfirma 1 ∷ u64 ≅ 1.0 ∷ f64 }",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "i64_u64_fuzzy_accepts",
            source: "incipit { adfirma 1 ∷ i64 ≈ 1 ∷ u64 }",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "i64_f64_fuzzy_accepts",
            source: "incipit { adfirma 1 ∷ i64 ≈ 1.0 ∷ f64 }",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "u64_f64_fuzzy_accepts",
            source: "incipit { adfirma 1 ∷ u64 ≈ 1.0 ∷ f64 }",
            expect: ValueEqualityExpect::Accepts,
        },
        // The same carrier pairs stay red under strict `≡` (fail-closed).
        ValueEqualityCase {
            name: "i64_u64_exact_rejects",
            source: "incipit { adfirma 1 ∷ i64 ≡ 1 ∷ u64 }",
            expect: ValueEqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        ValueEqualityCase {
            name: "i64_f64_exact_rejects",
            source: "incipit { adfirma 1 ∷ i64 ≡ 1.0 ∷ f64 }",
            expect: ValueEqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        ValueEqualityCase {
            name: "u64_f64_exact_rejects",
            source: "incipit { adfirma 1 ∷ u64 ≡ 1.0 ∷ f64 }",
            expect: ValueEqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
    ]
}

#[test]
fn numeric_value_equality_matrix() {
    for case in numeric_value_equality_cases() {
        let result = compile(case.source);
        match case.expect {
            ValueEqualityExpect::Accepts => {
                assert!(
                    result.is_ok(),
                    "case {}: {:?}",
                    case.name,
                    result.diagnostics
                );
            }
            ValueEqualityExpect::RejectsIssue(issue) => {
                assert!(
                    !result.is_ok(),
                    "case {} should fail: {:?}",
                    case.name,
                    result.diagnostics
                );
                assert!(
                    result.diagnostics.iter().any(|diag| diag
                        .args
                        .iter()
                        .any(|arg| arg.name == "issue" && arg.value == issue)),
                    "case {}: expected diagnostic issue {issue:?}, got: {:?}",
                    case.name,
                    result.diagnostics
                );
            }
        }
    }
}

// The two named acceptance rows from the E1 contract record: the exact probe
// sources must typecheck (admission is this gate's contract; the `falsum`
// verdict is the runner's sign-aware law that downstream lowerings must
// reproduce — see the law block on `numeric_value_equality_types`).
#[test]
fn sign_boundary_row_admits() {
    let result = compile(
        r"
incipit {
    fixum i64 n ← -1
    fixum u64 u ← 18446744073709551615
    adfirma n ≅ u
}
",
    );
    assert!(
        result.is_ok(),
        "sign-boundary row (−1 ≅ u64::MAX) must admit, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn cross_width_row_admits() {
    let result = compile(
        r"
incipit {
    fixum i64 n ← 9007199254740993 ∷ i64
    fixum u64 u ← 9007199254740992 ∷ u64
    adfirma n ≅ u
}
",
    );
    assert!(
        result.is_ok(),
        "cross-width row (2^53+1 ≅ 2^53) must admit, got: {:?}",
        result.diagnostics
    );
}
