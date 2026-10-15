//! F9 ruling 28: a negative `potentia` exponent on an integer receiver is a
//! compile error when the exponent is a constant, and never an error on a
//! `fractus` receiver (IEEE `pow`).

use super::test_support::{assert_accepts_la, assert_rejects_with_issue_la};

const ISSUE: &str = "potentia_exponent_negative";

#[test]
fn constant_negative_exponent_on_an_integer_receiver_is_a_compile_error() {
    for source in [
        "incipit {\nnota 2.potentia(-1)\n}\n",
        "incipit {\nsit numerus two ← 2\nnota two.potentia(-1)\n}\n",
        "incipit {\nsit u8 two ← 2\nnota two.potentia(-3)\n}\n",
        "incipit {\nsit i16 two ← 2\nnota two.potentia(1 - 2)\n}\n",
    ] {
        assert_rejects_with_issue_la(source, ISSUE);
    }
}

#[test]
fn non_negative_and_non_constant_exponents_are_accepted() {
    assert_accepts_la(
        "incipit {\nsit numerus two ← 2\nsit numerus k ← -1\nnota two.potentia(0)\nnota two.potentia(3)\nnota two.potentia(k)\n}\n",
    );
}

#[test]
fn fractus_receiver_keeps_a_negative_exponent() {
    assert_accepts_la(
        "incipit {\nsit fractus half ← 2.0\nnota half.potentia(-1.0)\nnota half.potentia(-1)\n}\n",
    );
}
