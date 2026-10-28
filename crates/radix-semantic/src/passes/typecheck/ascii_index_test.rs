//! Typecheck probes for scalar textus/ascii index returning `ascii<1>` (BB-3R).
//!
//! `s[i]` / `ascii[i]` scalar index infers `BoundedAscii { 1 }` (the type of
//! `'x'` literals); range index `s[i‥j]` stays `textus`.
//! Comparisons `s[i] ≡ 'a'` and `s[i] ≡ '\n'` typecheck naturally; the
//! chars-as-ints comparison `s[i] ≡ 97` remains a strict SEM010 exact-types
//! error pointing at the `↦ numerus` conversio (see also
//! `equality_exact_types_test.rs` for the message-pointer proof). Bare ascii
//! literals of any length typecheck; `ascii_literal_too_long` is reserved for
//! bounded `ascii<N>` carriers.

use super::TypeChecker;
use crate::{IndexExpr, Primitive, Resolver, SemanticErrorKind, Type, TypeTable};
use radix_lexer::{Interner, Span};
use radix_target::Target;

const EXACT_EQ_ISSUE: &str = "equality_exact_types_required";

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_la(source)
}

fn accepts(source: &str) {
    let result = compile(source);
    assert!(
        result.is_ok(),
        "expected success, got: {:?}",
        result.diagnostics
    );
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = compile(source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.is_error() && diag.issue() == Some(issue)),
        "expected diagnostic issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

/// A scalar index on `textus` is `ascii<1>`: binding the result to an
/// `ascii<1>`-annotated local only typechecks once the scalar return is
/// `BoundedAscii { 1 }` (the previous `numerus` / bare-`ascii` return
/// rejected this annotation).
#[test]
fn scalar_textus_index_infers_ascii() {
    accepts(
        r#"incipit {
    sit textus s ← "ab"
    sit ascii<magnitudo 1> c ← s[0]
    nota c
}"#,
    );
}

/// A scalar index on an `ascii` value is one scalar: `littera` (D10.3).
#[test]
fn scalar_ascii_index_infers_littera() {
    accepts(
        r"incipit {
    sit ascii s ← 'ab'
    sit littera c ← s[1]
    nota c
}",
    );
}

/// Direct type pin: `s[i]` on bare `textus` is `BoundedAscii { 1 }` (F5
/// held); on bare `ascii` it is `littera` (D10.3).
#[test]
fn scalar_index_type_is_ascii_one_for_textus_littera_for_ascii() {
    let mut types = TypeTable::new();
    let resolver = Resolver::new();
    let interner = Interner::new();
    let mut checker = TypeChecker::new(&resolver, &mut types, Some(&interner), Target::HirRust);
    let textus = checker.textus_type();
    let ascii = checker.ascii_type();
    let numerus = checker.numerus_type();
    let span = Span::default();
    let from_textus = checker.check_index_from_type(textus, numerus, span, span);
    let from_ascii = checker.check_index_from_type(ascii, numerus, span, span);
    let n1 = checker.types.intern_index(IndexExpr::Literal(1));
    assert!(
        matches!(
            checker.types.get(from_textus),
            Type::BoundedAscii { capacity } if checker.types.index_equals(*capacity, n1)
        ),
        "textus[i] is BoundedAscii {{ 1 }}, got {:?}",
        checker.types.get(from_textus)
    );
    assert!(
        matches!(
            checker.types.get(from_ascii),
            Type::Primitive(Primitive::Littera)
        ),
        "ascii[i] is littera, got {:?}",
        checker.types.get(from_ascii)
    );
    assert!(
        !matches!(
            checker.types.get(from_textus),
            Type::Primitive(Primitive::Ascii)
        ),
        "scalar index must not stay bare Primitive::Ascii"
    );
}

/// The natural scalar-index comparisons compile: `s[i] ≡ 'a'` and `s[i] ≡ '\n'`
/// (single-quoted ascii literals), no magic-number comparisons needed.
#[test]
fn scalar_index_eq_ascii_literals_compile() {
    accepts(
        r#"incipit {
    sit textus s ← "hello\nworld"
    si s[0] ≡ 'a' { nota "first" }
    si s[5] ≡ '\n' { nota "newline" }
    nota s
}"#,
    );
}

/// Range index `s[i‥j]` still returns `textus`: the result binds to a `textus`
/// local, and comparing a range slice to an ascii literal is still an
/// exact-types error (a slice is not a single character).
#[test]
fn range_index_still_textus() {
    accepts(
        r#"incipit {
    sit textus s ← "hello"
    sit textus slice ← s[0‥2]
    nota slice
}"#,
    );
    rejects_with_issue(
        r#"incipit {
    sit textus s ← "hello"
    si s[0‥2] ≡ 'h' { nota "bad" }
}"#,
        EXACT_EQ_ISSUE,
    );
}

/// `s[i] ≡ 97` stays a strict SEM010 exact-types error (live naming
/// `SemanticErrorKind::TypeMismatch`).
#[test]
fn scalar_index_eq_numerus_rejects_with_sem010() {
    let result = compile(
        r#"incipit {
    sit textus s ← "ab"
    si s[0] ≡ 97 { nota "bad" }
}"#,
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

/// Bare ascii is unbounded: 64-byte and 65-byte literals both typecheck.
/// `ascii_literal_too_long` no longer fires on the bare form.
#[test]
fn ascii_literal_capacity_boundary() {
    accepts(&format!(
        "incipit {{ sit ascii ok ← '{}' nota ok }}",
        "a".repeat(64)
    ));
    accepts(&format!(
        "incipit {{ sit ascii big ← '{}' nota big }}",
        "a".repeat(65)
    ));
}

/// A bounded literal is checked against the declared carrier capacity, while
/// the same payload remains legal for bare unbounded `ascii`.
#[test]
fn bounded_ascii_literal_rejects_payload_over_capacity() {
    rejects_with_issue(
        "incipit { sit ascii<magnitudo 8> too_long ← 'abcdefghi' nota too_long }",
        "ascii_literal_too_long",
    );
    accepts(&format!(
        "incipit {{ sit ascii<magnitudo 8> fits ← '{}' nota fits }}",
        "a".repeat(8)
    ));
}

#[test]
fn bounded_ascii_scalar_index_returns_littera() {
    accepts(
        r"incipit {
    sit ascii<magnitudo 8> source ← 'abc'
    sit littera c ← source[1]
    nota c
}",
    );
}

#[test]
fn bounded_ascii_capacity_widens_only_directionally() {
    accepts(
        r"incipit {
    sit ascii<magnitudo 8> narrow ← 'abc'
    sit ascii<magnitudo 16> wide ← narrow
    nota wide
}",
    );
    rejects_with_issue(
        r"incipit {
    sit ascii<magnitudo 16> wide ← 'abc'
    sit ascii<magnitudo 8> narrow ← wide
    nota narrow
}",
        "incompatible_tensor_index",
    );
}

#[test]
fn bounded_ascii_d7_conversions_are_explicit() {
    accepts(
        r"incipit {
    sit ascii<magnitudo 8> bounded ← 'abc'
    sit ascii unbounded ← bounded ↦ ascii
    sit textus text ← bounded ↦ textus
    sit ascii<magnitudo 8> roundtrip ← unbounded ↦ ascii<magnitudo 8>
    nota roundtrip
}",
    );
}

#[test]
fn bounded_ascii_equivalence_stays_same_type_only() {
    rejects_with_issue(
        r"incipit {
    sit ascii<magnitudo 8> narrow ← 'abc'
    sit ascii<magnitudo 16> wide ← 'abc'
    si narrow ≡ wide { nota 'same' }
}",
        EXACT_EQ_ISSUE,
    );
}

/// `'\u{41}'` uses the shared unicode escape table in ascii literals (unit 74d26a84).
#[test]
fn ascii_unicode_escape_in_ascii_literal_checks() {
    accepts(
        r"incipit {
    sit ascii a ← '\u{41}'
    nota a
}",
    );
}

/// `'\u{E9}'` is above 7F and fails closed as the existing invalid-escape issue.
#[test]
fn ascii_unicode_escape_above_7f_is_invalid_escape() {
    rejects_with_issue(
        r"incipit {
    sit ascii a ← '\u{E9}'
    nota a
}",
        "ascii_literal_invalid_escape",
    );
}
