//! Clean-break contract for the removed ternary spellings (DECISIONS D4.7).
//!
//! `c ? a : b` and `c sic a secus b` (en `yields … else`) were removed in
//! favour of `c ✓ a ✗ b`. Each old spelling reports a named migration
//! diagnostic pointing at `✓ ✗`, not a generic parse error. The retired `sic`
//! stays a reserved control-flow word that carries this diagnostic; `?` keeps
//! its optional-chain forms.

use crate::test_support::{ParseExpectation, assert_parse_error_matches, parse_ok};

#[test]
fn question_colon_reports_migration_to_check_ballot() {
    assert_parse_error_matches(
        "fixum _ v ← c ? 1 : 2",
        ParseExpectation::Issue("conditional_question_colon_removed"),
    );
}

#[test]
fn sic_secus_reports_migration_to_check_ballot() {
    assert_parse_error_matches(
        "fixum _ v ← c sic 1 secus 2",
        ParseExpectation::Issue("conditional_sic_removed"),
    );
}

#[test]
fn optional_chain_forms_survive_the_removal() {
    let _ = parse_ok("fixum _ v ← a?.b\nfixum _ w ← a?[0]\nfixum _ x ← a?(1)");
}
