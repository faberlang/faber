//! Non-capturing closures: a synthesized top-level function per closure, whose
//! value is a C function pointer.

use crate::test_support::{emitted, rejected};

#[test]
fn a_non_capturing_closure_is_a_top_level_function() {
    let code = emitted("main {\n    let (int) → int dupla ← int x ∴ x * 2\n    print dupla(5)\n}");
    assert!(
        code.contains("static int64_t f_closure_0(int64_t p_x);"),
        "{code}"
    );
    assert!(
        code.contains("int64_t (*l_dupla)(int64_t) = f_closure_0;"),
        "{code}"
    );
}

#[test]
fn a_block_closure_body_returns_from_its_function() {
    let code = emitted(
        "main {\n    let (int) → int f ← int x → int ∴ do {\n        let int y ← x + 1\n        return y * 2\n    }\n    print f(5)\n}",
    );
    assert!(
        code.contains("static int64_t f_closure_0(int64_t p_x)"),
        "{code}"
    );
    assert!(code.contains("const int64_t l_y ="), "{code}");
}

#[test]
fn a_closure_with_a_mut_list_parameter_takes_the_arena() {
    let code = emitted(
        "main {\n    mut list<int> xs ← [1, 2, 3]\n    let (mut list<int>) → void grow ← mut list<int> ys ∴ ys.append(9)\n    grow(mut xs)\n    print xs\n}",
    );
    assert!(
        code.contains("static void f_closure_0(faber_arena *faber_a, faber_list_i64 *p_ys)"),
        "{code}"
    );
}

#[test]
fn a_capturing_closure_is_rejected() {
    assert_eq!(
        rejected(
            "main {\n    let int n ← 3\n    let (int) → int add ← int x ∴ x + n\n    print add(1)\n}"
        ),
        "capturing closure"
    );
}

#[test]
fn a_bool_closure_call_in_a_condition_is_parenthesized() {
    let code = emitted(
        "main {\n    let (int) → bool par ← int x → bool ∴ x % 2 ≡ 0\n    if par(4) {\n        print \"even\"\n    }\n}",
    );
    assert!(code.contains("if (l_par(INT64_C(4)))"), "{code}");
}
