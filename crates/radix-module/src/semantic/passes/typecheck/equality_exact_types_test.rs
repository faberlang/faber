//! Typecheck probes for exact-type equality (`≡`, `≠`, `est`, `non est`).

use crate::diagnostics::semantic_message;
use crate::semantic::SemanticErrorKind;
use crate::{Compiler, Config, Target};

#[cfg(feature = "hir-rust")]
const EXACT_EQ_ISSUE: &str = "equality_exact_types_required";

fn compile(source: &str) -> crate::CompileResult {
    Compiler::new(Config::default().with_target(Target::HirRust)).compile_str("test.fab", source)
}

#[cfg(feature = "hir-rust")]
enum EqualityExpect {
    Accepts,
    RejectsIssue(&'static str),
}

#[cfg(feature = "hir-rust")]
struct EqualityCase {
    name: &'static str,
    source: &'static str,
    expect: EqualityExpect,
}

#[cfg(feature = "hir-rust")]
fn equality_exact_types_cases() -> Vec<EqualityCase> {
    vec![
        EqualityCase {
            name: "ascii_textus_eq_rejects",
            source: "incipit { si \"a\" ≡ 'a' { nota \"bad\" } }",
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        EqualityCase {
            name: "ascii_textus_neq_rejects",
            source: "incipit { si 'a' ≠ \"a\" { nota \"bad\" } }",
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
    redde fine < coarse
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
    fixum numerus<u8> narrow ← 1 ∷ numerus<u8>
    fixum numerus wide ← 1
    si narrow ≡ wide { nota "bad" }
}
"#,
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        EqualityCase {
            name: "octeti_lista_eq_rejects",
            source: r#"
incipit {
    fixum octeti raw ← |41|
    fixum lista<numerus<u8>> bytes ← [0x41 ∷ numerus<u8>]
    si raw ≡ bytes { nota "bad" }
}
"#,
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
        EqualityCase {
            name: "ascii_index_numerus_eq_rejects",
            source: r#"
incipit {
    fixum textus s ← "ab"
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
            source: "incipit { si nihil ≡ nihil { nota \"ok\" } }",
            expect: EqualityExpect::Accepts,
        },
        EqualityCase {
            name: "est_same_type",
            source: "incipit { si 1 est 1 { nota \"ok\" } }",
            expect: EqualityExpect::Accepts,
        },
        EqualityCase {
            name: "unrelated_types_reject",
            source: "incipit { si \"x\" ≡ 1 { nota \"bad\" } }",
            expect: EqualityExpect::RejectsIssue(EXACT_EQ_ISSUE),
        },
    ]
}

#[cfg(feature = "hir-rust")]
#[test]
fn equality_exact_types_matrix() {
    for case in equality_exact_types_cases() {
        let result = compile(case.source);
        match case.expect {
            EqualityExpect::Accepts => {
                assert!(result.success(), "case {}: {:?}", case.name, result.diagnostics);
            }
            EqualityExpect::RejectsIssue(issue) => {
                assert!(!result.success(), "case {} should fail: {:?}", case.name, result.diagnostics);
                assert!(
                    result.diagnostics.iter().any(|diag| {
                        // Live SEM010 naming: the exact-types equality error is
                        // SemanticErrorKind::TypeMismatch (catalog_semantic.rs),
                        // with the issue tag as its sub-variant.
                        diag.semantic_kind() == Some(SemanticErrorKind::TypeMismatch)
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
        }
    }
}

/// The §3b diagnostic pointer: `ascii ≡ numerus` (e.g. `s[i] ≡ 97`) stays a
/// strict SEM010 exact-types error whose message names the existing `↦ numerus`
/// conversio instead of suggesting an implicit comparison rule.
#[test]
fn ascii_numerus_equality_message_names_numerus_conversio() {
    let result = compile(
        r#"
incipit {
    fixum textus s ← "ab"
    si s[0] ≡ 97 { nota "bad" }
}
"#,
    );
    assert!(!result.success(), "expected failure, got: {:?}", result.diagnostics);
    let diag = result
        .diagnostics
        .iter()
        .find(|diag| diag.semantic_kind() == Some(SemanticErrorKind::TypeMismatch))
        .expect("s[i] ≡ 97 must produce a SEM010 TypeMismatch diagnostic");
    let message =
        semantic_message(diag.semantic_kind().unwrap(), &diag.args).expect("SEM010 TypeMismatch must render a message");
    assert!(
        message.contains("↦ numerus"),
        "SEM010 message should point at the `↦ numerus` conversio, got: {message:?}"
    );
    assert!(
        message.contains("↦"),
        "SEM010 message should name the `↦` conversio operator, got: {message:?}"
    );
}
