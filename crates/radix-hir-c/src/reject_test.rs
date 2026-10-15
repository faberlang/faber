//! Rejection tests: each construct outside the subset fails by name, and the
//! test asserts the `issue`, `target` and `construct` facts, never message text.

use crate::test_support::{rejected, rejected_construct};

const FLOAT_PRINT: &str = include_str!("../tests/fixtures/reject/float_print.fab");

#[test]
fn float_literal() {
    assert_eq!(rejected("main {\n    print 1.5\n}"), "float literal");
}

#[test]
fn float_fixture() {
    assert_eq!(
        rejected_construct("float_print.fab", FLOAT_PRINT),
        "float literal"
    );
}

#[test]
fn integer_beyond_the_representation_cap() {
    assert_eq!(
        rejected("main {\n    const int a ← 9223372036854775807\n    print a * a\n}"),
        "integer constant beyond the 64-bit representation cap"
    );
}

#[test]
fn integer_division_by_zero() {
    assert_eq!(
        rejected("main {\n    print 7 / 0\n}"),
        "integer division by zero"
    );
}

#[test]
fn integer_remainder_by_zero() {
    assert_eq!(
        rejected("main {\n    print 7 % 0\n}"),
        "integer remainder by zero"
    );
}

#[test]
fn integer_division_by_zero_constant() {
    assert_eq!(
        rejected("main {\n    const int z ← 0\n    print 7 / z\n}"),
        "integer division by zero"
    );
}

#[test]
fn bitwise_operator() {
    assert_eq!(rejected("main {\n    print 1 ∧ 3\n}"), "bitwise operator");
}

#[test]
fn sized_integer_type() {
    assert_eq!(
        rejected("main {\n    const u8 n ← 5\n    print n\n}"),
        "sized integer type"
    );
}

#[test]
fn list_type() {
    assert_eq!(
        rejected("main {\n    const list<int> xs ← [1, 2]\n    print 1\n}"),
        "list type"
    );
}

#[test]
fn write_output_is_not_print() {
    assert_eq!(
        rejected("main {\n    warn \"w\"\n}"),
        "write, debug, or warn output (only print is supported)"
    );
}

#[test]
fn text_comparison() {
    assert_eq!(
        rejected("main {\n    if \"a\" ≡ \"b\" {\n        print 1\n    }\n}"),
        "comparison of text or booleans"
    );
}

#[test]
fn iteration_loop() {
    assert_eq!(
        rejected("main {\n    for range 0‥3 const i {\n        print i\n    }\n}"),
        "iteration loop"
    );
}

/// `inf` needs an arena-backed bignum (goal unit C6); until then it is named.
#[test]
fn unbounded_integer_type() {
    assert_eq!(
        rejected("main {\n    var inf n ← 5\n    print n\n}"),
        "unbounded integer type (inf)"
    );
}

#[test]
fn generic_function() {
    assert_eq!(
        rejected("fn id<T>(T x) → T {\n    return x\n}\nmain {\n    print id(1)\n}"),
        "generic function"
    );
}
