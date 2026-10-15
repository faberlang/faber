//! Rejection tests: each construct outside the subset fails by name, and the
//! test asserts the `issue`, `target` and `construct` facts, never message text.

use crate::test_support::{rejected, rejected_construct};

const F32_TYPE: &str = include_str!("../tests/fixtures/reject/f32_type.fab");

#[test]
fn integer_beyond_the_representation_cap() {
    assert_eq!(
        rejected("main {\n    let int a ← 9223372036854775807\n    print a * a\n}"),
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
        rejected("main {\n    let int z ← 0\n    print 7 / z\n}"),
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
        rejected("main {\n    let u8 n ← 5\n    print n\n}"),
        "sized integer type"
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
        rejected("main {\n    for range 0‥3 let i {\n        print i\n    }\n}"),
        "iteration loop"
    );
}

/// `inf` needs an arena-backed bignum (goal unit C6); until then it is named.
#[test]
fn unbounded_integer_type() {
    assert_eq!(
        rejected("main {\n    mut inf n ← 5\n    print n\n}"),
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

#[test]
fn nested_list_type() {
    assert_eq!(
        rejected("main {\n    let list<list<int>> grid ← [[1], [2]]\n    print grid\n}"),
        "nested list type"
    );
}

/// `mut int` would need a pointer to the caller's cell; only lists, which
/// already alias, take an access marker in this subset.
#[test]
fn access_marker_on_a_scalar_parameter() {
    assert_eq!(
        rejected(
            "fn bump(mut int x) {\n    x ← x + 1\n}\nmain {\n    mut int n ← 1\n    bump(mut n)\n    print n\n}"
        ),
        "ref or mut access to a scalar"
    );
}

#[test]
fn list_of_genus() {
    assert_eq!(
        rejected(
            "class P {\n    mut int x = 0\n}\nmain {\n    let list<P> ps ← [{ x = 1 }]\n    print ps.length()\n}"
        ),
        "list of genus"
    );
}

#[test]
fn static_field() {
    assert_eq!(
        rejected(
            "class P {\n    static int count = 0\n    mut int x = 0\n}\nmain {\n    let P p ← { x = 1 }\n    print p.x\n}"
        ),
        "static (generis) field"
    );
}

/// `f32` math runs at full precision and narrows at the store (goal unit C7,
/// step 4); until then the width is rejected by name.
#[test]
fn f32_fixture() {
    assert_eq!(
        rejected_construct("f32_type.fab", F32_TYPE),
        "float width other than f64"
    );
}
