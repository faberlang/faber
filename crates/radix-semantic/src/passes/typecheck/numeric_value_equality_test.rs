//! Typecheck probes for numeric value equality (`≅`, `≇`).

use crate::test_support;

const EXACT_EQ_ISSUE: &str = "equality_exact_types_required";
const NUMERIC_VALUE_EQ_ISSUE: &str = "numeric_value_equality_operands_required";

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source(source)
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
    fixum numerus<u8> port ← 42 ∷ numerus<u8>
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
    fixum numerus<u8> port ← 42 ∷ numerus<u8>
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
            source: "incipit { adfirma 42.145 ∷ fractus<f64> ≅ 42.0 ∷ fractus<f64> }",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "instans_cross_approx_rejects",
            source: r"
functio same(instans<ns> fine, instans<ms> coarse) → bivalens {
    redde fine ≅ coarse
}
",
            expect: ValueEqualityExpect::RejectsIssue(NUMERIC_VALUE_EQ_ISSUE),
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
    fixum numerus<i32> signed ← -1 ∷ numerus<i32>
    fixum numerus<u8> unsigned ← 255 ∷ numerus<u8>
    adfirma signed ≅ unsigned
}
",
            expect: ValueEqualityExpect::Accepts,
        },
        ValueEqualityCase {
            name: "cross_width_params_approx_accepts",
            source: r"
functio promoted(numerus left, numerus<u8> right) → bivalens {
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
