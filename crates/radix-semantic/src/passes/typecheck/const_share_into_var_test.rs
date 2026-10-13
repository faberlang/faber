//! `const` is deep (need, rule 2): a plain `var T y ← x` that shares
//! a `const` or `ref` container into a writable name is SEM065
//! `const_shared_into_var`; `← copy x`, `← own x`, scalars and `var` sources
//! stay legal.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use crate::SemanticErrorKind;
use crate::test_support;

const ISSUE: &str = "const_shared_into_var";

#[test]
fn const_list_shared_into_var_is_rejected() {
    let source = "incipit {\n    fixum lista<numerus> xs ← [1]\n    varia lista<numerus> ys ← xs\n    ys.appende(2)\n    nota xs\n}";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn const_scalar_into_var_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    fixum numerus a ← 1\n    varia numerus b ← a\n    b ← b + 1\n    nota b\n}",
    );
}

#[test]
fn const_list_copy_into_var_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    fixum lista<numerus> xs ← [1]\n    varia lista<numerus> ys ← copy xs\n    ys.appende(2)\n    nota xs\n    nota ys\n}",
    );
}

#[test]
fn var_list_shared_into_var_is_accepted() {
    test_support::assert_accepts(
        "incipit {\n    varia lista<numerus> xs ← [1]\n    varia lista<numerus> ys ← xs\n    ys.appende(2)\n    nota ys\n}",
    );
}

#[test]
fn module_constant_list_shared_into_var_is_rejected() {
    let source = "fixum lista<numerus> XS = [1, 2]\nincipit {\n    varia lista<numerus> ys ← XS\n    ys.appende(3)\n    nota XS\n}";
    let result = test_support::analyze_source(source);
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()
            && matches!(err.kind, SemanticErrorKind::ConstSharedIntoVar)
            && test_support::issue(err) == Some(ISSUE)),
        "expected SEM065 {ISSUE}, got {:?}",
        result.diagnostics
    );
}

#[test]
fn module_constant_list_copy_into_var_is_accepted() {
    test_support::assert_accepts(
        "fixum lista<numerus> XS = [1, 2]\nincipit {\n    varia lista<numerus> ys ← copy XS\n    ys.appende(3)\n    nota XS\n    nota ys\n}",
    );
}

#[test]
fn module_constant_scalar_into_var_is_accepted() {
    test_support::assert_accepts(
        "fixum numerus N = 2\nincipit {\n    varia numerus b ← N\n    b ← b + 1\n    nota b\n}",
    );
}
