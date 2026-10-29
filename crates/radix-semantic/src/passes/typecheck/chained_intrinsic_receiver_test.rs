//! need: an intrinsic call whose receiver is itself an intrinsic
//! call result checks under a non-la reader pack.
//!
//! The numeric-twin probe checks the receiver of a twin-named method
//! (`added`, `subtracted`, `multiplied`, ...) to learn its type. A checked
//! intrinsic call writes its canonical method symbol back to the HIR, so a
//! second check of the same receiver saw the canonical Latin spelling under
//! the en pack and rejected it as `unknown_method`. The probe now hands its
//! result to the one later check instead of checking the receiver twice.

use crate::locale::LocalePack;
use radix_target::Target;

fn en_pack() -> LocalePack {
    let path = crate::locale::workspace_locale_pack_path("en");
    LocalePack::from_toml_path(&path).expect("installed en pack should load")
}

fn accepted_in_en(source: &str) {
    let mut config = crate::PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(en_pack());
    let result = crate::test_support::analyze_source_with_config(source, config);
    assert!(
        result.is_ok(),
        "chained receiver must check clean: {:?}\n{source}",
        result.diagnostics
    );
}

#[test]
fn vector_added_then_multiply_chain_checks() {
    accepted_in_en(
        "fn t(vector<f32, size 4> a, vector<f32, size 4> b, vector<f32, size 4> c) → vector<f32, size 4> {
    return a.added(b).multiplied(c)
}",
    );
}

#[test]
fn vector_subtract_then_multiply_chain_checks() {
    accepted_in_en(
        "fn t(vector<f32, size 4> a, vector<f32, size 4> b) → vector<f32, size 4> {
    return a.subtracted(b).multiplied(b)
}",
    );
}

#[test]
fn vector_chain_through_alias_spelling_checks() {
    accepted_in_en(
        "fn t(vf32[4] a, vf32[4] b, vf32[4] c) → vf32[4] {
    return a.multiplied(b).added(c).subtracted(a)
}",
    );
}

#[test]
fn vector_chain_ending_in_a_scalar_result_checks() {
    accepted_in_en(
        "fn t(vector<f32, size 4> a, vector<f32, size 4> b) → f32 {
    return a.added(b).multiplied(b).dot(a)
}",
    );
}

#[test]
fn list_intrinsic_chain_into_a_numeric_twin_checks() {
    accepted_in_en(
        "fn t(list<int> xs) → int {
    return xs.sorted().length().added(1)
}",
    );
}

#[test]
fn list_length_into_a_numeric_twin_checks() {
    accepted_in_en(
        "fn t(list<int> xs) → int {
    return xs.length().added(1)
}",
    );
}

#[test]
fn text_intrinsic_chain_into_a_numeric_twin_checks() {
    accepted_in_en(
        "fn t(string s) → int {
    return s.trim().length().added(1)
}",
    );
}

#[test]
fn unknown_method_after_a_chained_receiver_is_still_rejected() {
    let mut config = crate::PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(en_pack());
    let result = crate::test_support::analyze_source_with_config(
        "fn t(vector<f32, size 4> a, vector<f32, size 4> b) → vector<f32, size 4> {
    return a.added(b).nonexistent(b)
}",
        config,
    );
    assert!(!result.is_ok(), "an unknown method must still be rejected");
}
