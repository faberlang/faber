//! Rejection tests: each construct outside the subset fails by name, and the
//! test asserts the `issue`, `target` and `construct` facts, never message text.

use crate::test_support::rejected;

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
fn policy_float_type() {
    assert_eq!(
        rejected("main {\n    let saturating<f64> x ← 1.0\n    print x\n}"),
        "wrapping, saturating, or policy numeric type"
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
