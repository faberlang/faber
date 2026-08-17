//! Typecheck probes for scalar textus/ascii index returning `ascii` (A2).
//!
//! `s[i]` / `ascii[i]` scalar index infers the single-character `ascii` type
//! (the type of `'x'` literals); range index `s[i‥j]` stays `textus`.
//! Comparisons `s[i] ≡ 'a'` and `s[i] ≡ '\n'` typecheck naturally; the
//! chars-as-ints comparison `s[i] ≡ 97` remains a strict SEM010 exact-types
//! error pointing at the `↦ numerus` conversio (see also
//! `equality_exact_types_test.rs` for the message-pointer proof). Ascii
//! literals beyond the inline-buffer capacity reject with
//! `ascii_literal_too_long`.

use crate::diagnostics::semantic_message;
use crate::semantic::SemanticErrorKind;
use crate::{Compiler, Config, Target};

const EXACT_EQ_ISSUE: &str = "equality_exact_types_required";

fn compile(source: &str) -> crate::CompileResult {
    Compiler::new(Config::default().with_target(Target::HirRust)).compile_str("test.fab", source)
}

#[cfg(feature = "hir-rust")]
fn accepts(source: &str) {
    let result = compile(source);
    assert!(result.success(), "expected success, got: {:?}", result.diagnostics);
}

#[cfg(feature = "hir-rust")]
fn rejects_with_issue(source: &str, issue: &str) {
    let result = compile(source);
    assert!(!result.success(), "expected failure, got: {:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.is_error() && diag.issue() == Some(issue)),
        "expected diagnostic issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

/// A scalar index on `textus` is the `ascii` type: binding the result to an
/// `ascii`-annotated local only typechecks once the scalar return is `ascii`
/// (the previous `numerus` return rejected this annotation).
#[cfg(feature = "hir-rust")]
#[test]
fn scalar_textus_index_infers_ascii() {
    accepts(
        r#"incipit {
    fixum textus s ← "ab"
    fixum ascii c ← s[0]
    nota c
}"#,
    );
}

/// A scalar index on an `ascii` value is also the `ascii` type.
#[cfg(feature = "hir-rust")]
#[test]
fn scalar_ascii_index_infers_ascii() {
    accepts(
        r"incipit {
    fixum ascii s ← 'ab'
    fixum ascii c ← s[1]
    nota c
}",
    );
}

/// The natural scalar-index comparisons compile: `s[i] ≡ 'a'` and `s[i] ≡ '\n'`
/// (single-quoted ascii literals), no magic-number comparisons needed.
#[cfg(feature = "hir-rust")]
#[test]
fn scalar_index_eq_ascii_literals_compile() {
    accepts(
        r#"incipit {
    fixum textus s ← "hello\nworld"
    si s[0] ≡ 'a' { nota "first" }
    si s[5] ≡ '\n' { nota "newline" }
    nota s
}"#,
    );
}

/// Range index `s[i‥j]` still returns `textus`: the result binds to a `textus`
/// local, and comparing a range slice to an ascii literal is still an
/// exact-types error (a slice is not a single character).
#[cfg(feature = "hir-rust")]
#[test]
fn range_index_still_textus() {
    accepts(
        r#"incipit {
    fixum textus s ← "hello"
    fixum textus slice ← s[0‥2]
    nota slice
}"#,
    );
    rejects_with_issue(
        r#"incipit {
    fixum textus s ← "hello"
    si s[0‥2] ≡ 'h' { nota "bad" }
}"#,
        EXACT_EQ_ISSUE,
    );
}

/// `s[i] ≡ 97` stays a strict SEM010 exact-types error (live naming
/// `SemanticErrorKind::TypeMismatch`) whose message points at `↦ numerus`.
#[test]
fn scalar_index_eq_numerus_rejects_with_sem010_pointer() {
    let result = compile(
        r#"incipit {
    fixum textus s ← "ab"
    si s[0] ≡ 97 { nota "bad" }
}"#,
    );
    assert!(!result.success(), "expected failure, got: {:?}", result.diagnostics);
    let diag = result
        .diagnostics
        .iter()
        .find(|diag| diag.semantic_kind() == Some(SemanticErrorKind::TypeMismatch))
        .expect("s[i] ≡ 97 must produce a SEM010 TypeMismatch diagnostic");
    assert_eq!(diag.issue(), Some(EXACT_EQ_ISSUE));
    let message =
        semantic_message(diag.semantic_kind().unwrap(), &diag.args).expect("SEM010 TypeMismatch must render a message");
    assert!(
        message.contains("↦ numerus"),
        "SEM010 message should point at the `↦ numerus` conversio, got: {message:?}"
    );
}

/// Ascii literals at the inline-buffer capacity boundary: exactly 64 bytes
/// typechecks; one byte over rejects with `ascii_literal_too_long`.
#[cfg(feature = "hir-rust")]
#[test]
fn ascii_literal_capacity_boundary() {
    accepts(&format!("incipit {{ fixum ascii ok ← '{}' nota ok }}", "a".repeat(64)));
    rejects_with_issue(
        &format!("incipit {{ fixum ascii big ← '{}' }}", "a".repeat(65)),
        "ascii_literal_too_long",
    );
}
