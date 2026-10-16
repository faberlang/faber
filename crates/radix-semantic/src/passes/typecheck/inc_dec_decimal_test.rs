use crate::test_support::{assert_accepts_la, assert_rejects_with_issue_la};

const ISSUE: &str = "postfix_inc_dec_numerus_operand";

#[test]
fn d64_increment_is_rejected() {
    assert_rejects_with_issue_la(
        "functio g() → vacuum {\n    varia d64 d ← 1.5\n    d ↑\n}\n",
        ISSUE,
    );
}

#[test]
fn d64_decrement_is_rejected() {
    assert_rejects_with_issue_la(
        "functio g() → vacuum {\n    varia d64 d ← 1.5\n    d ↓\n}\n",
        ISSUE,
    );
}

#[test]
fn integer_increment_remains_accepted() {
    assert_accepts_la("functio g() → vacuum {\n    varia numerus n ← 1\n    n ↑\n}\n");
}

#[test]
fn fractus_increment_remains_rejected() {
    assert_rejects_with_issue_la(
        "functio g() → vacuum {\n    varia fractus f ← 1.5\n    f ↑\n}\n",
        ISSUE,
    );
}
