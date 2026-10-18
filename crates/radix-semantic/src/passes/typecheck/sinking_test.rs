//! Need: the `@ radix contract "sinking"` mark and its local
//! recognition — the `Sink<T>` shape check and the one-claim rule.
//!
//! Fixtures declare the marked contract locally (the shape norma ships) with
//! latin spellings, because the radix unit-test harness compiles bare source
//! without the en-locale lex pack — the same surface as `ordering_test.rs`.

use crate::test_support;

const SINK: &str = "\
@ radix contract \"sinking\"
implendum Sink<T> {
    functio sink(T res) → vacuum ⇥ textus
    functio flush() → vacuum ⇥ textus
}

functio effundo<T implet Sink<T>>(T v) → vacuum {
}

";

#[test]
fn sinking_contract_local_shape_ok() {
    let result = test_support::analyze_ok(SINK);
    assert!(
        result.result.types.sinking_contract.is_some(),
        "the marked implendum must claim types.sinking_contract"
    );
}

#[test]
fn sinking_contract_shape_rejected() {
    test_support::assert_rejects_with_issue(
        "@ radix contract \"sinking\"\nimplendum Sink<T> {\n    functio sink(T res) → vacuum\n    functio flush() → vacuum ⇥ textus\n}\n",
        "radix_contract_sinking_shape",
    );
}

#[test]
fn sinking_contract_duplicate_rejected() {
    let source = format!(
        "{SINK}\
@ radix contract \"sinking\"
implendum Altera<T> {{
    functio sink(T res) → vacuum ⇥ textus
    functio flush() → vacuum ⇥ textus
}}
"
    );
    test_support::assert_rejects_with_issue(&source, "radix_contract_duplicate");
}

#[test]
fn sinking_contract_placement_rejected() {
    test_support::assert_analysis_rejects_with_issue(
        "@ radix contract \"sinking\"\ngenus Lapis {\n    varia numerus massa\n}\n",
        "radix_contract_placement",
    );
}
