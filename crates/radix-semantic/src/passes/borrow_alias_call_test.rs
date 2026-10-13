//! A call's `mut`/`ref` arguments are checked for overlap on storage, not on
//! the argument's own binding: `var alias ← g` names `g`'s tensor, so
//! `k(g, alias)` overlaps exactly like `k(g, g)`. Alias-spelled overlaps are
//! rejected with the same issues the direct spelling produces. Read/read
//! aliasing stays legal, `copy`/`own` binds do not alias, and call-scoped
//! borrows release at call end.

use crate::SemanticErrorKind;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};

const PRELUDE: &str = r"fn mut_read(mut tf32[2, 2] a, tf32[2, 2] b) → void {
    a ⇇ b
}

fn mut_ref(mut tf32[2, 2] a, ref tf32[2, 2] b) → void {
    a ⇇ b
}

fn mut_mut(mut tf32[2, 2] a, mut tf32[2, 2] b) → void {
    a ⇇ b
}

fn read_read(tf32[2, 2] a, tf32[2, 2] b) → void {
    print a.flatten()
    print b.flatten()
}
";

/// The fixtures are written in the English surface.
fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

/// `body` runs in `main` after `g` (a tensor) and `fresh` (an unrelated one).
fn program(body: &str) -> String {
    format!(
        r"{PRELUDE}
main {{
    const list<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    var tf32[2, 2] g ← flat ↦ tf32[2, 2]
    var tf32[2, 2] fresh ← flat ↦ tf32[2, 2]
{body}
}}
"
    )
}

fn errors(body: &str) -> Vec<(SemanticErrorKind, String)> {
    analyze_en(&program(body))
        .result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| (d.kind.clone(), issue(d).unwrap_or_default().to_string()))
        .collect()
}

fn assert_accepts(body: &str) {
    let found = errors(body);
    assert!(found.is_empty(), "expected no error, got: {found:?}");
}

fn assert_rejects(body: &str, expected: &str) {
    let found = errors(body);
    assert!(
        found.iter().any(|(_, issue)| issue == expected),
        "expected issue {expected:?}, got: {found:?}"
    );
}

// =============================================================================
// Rejected: an alias spelling overlaps like the direct spelling
// =============================================================================

#[test]
fn direct_spelling_of_the_same_overlap_is_rejected() {
    assert_rejects("    mut_read(mut g, g)", "use_while_mutably_borrowed");
    assert_rejects("    mut_mut(mut g, mut g)", "mutable_borrow_conflict");
}

#[test]
fn mut_then_aliased_read_argument_is_rejected() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_read(mut g, alias)",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn aliased_mut_then_read_argument_is_rejected() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_read(mut alias, g)",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn mut_then_aliased_ref_argument_is_a_shared_borrow_conflict() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_ref(mut g, alias)",
        "shared_borrow_conflict",
    );
}

#[test]
fn aliased_mut_then_ref_argument_is_a_shared_borrow_conflict() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_ref(mut alias, g)",
        "shared_borrow_conflict",
    );
}

#[test]
fn two_mut_arguments_through_an_alias_are_rejected() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_mut(mut g, mut alias)",
        "mutable_borrow_conflict",
    );
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_mut(mut alias, mut g)",
        "mutable_borrow_conflict",
    );
}

#[test]
fn two_aliases_of_one_tensor_overlap() {
    assert_rejects(
        "    var tf32[2, 2] left ← g\n    var tf32[2, 2] right ← g\n    mut_mut(mut left, mut right)",
        "mutable_borrow_conflict",
    );
}

#[test]
fn alias_of_an_alias_overlaps_transitively() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    var tf32[2, 2] again ← alias\n    mut_mut(mut g, mut again)",
        "mutable_borrow_conflict",
    );
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    var tf32[2, 2] again ← alias\n    mut_read(mut again, g)",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn view_of_an_alias_passed_with_the_original_mut_is_rejected() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_read(mut g, alias.transpose())",
        "tensor_view_read_only",
    );
}

#[test]
fn view_of_the_original_passed_with_an_aliased_mut_is_rejected() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    mut_read(mut alias, g.transpose())",
        "tensor_view_read_only",
    );
}

#[test]
fn stored_view_passed_with_an_alias_of_its_origin_mut_is_rejected() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    const tf32[2, 2] v ← g.transpose()\n    mut_read(mut alias, v)",
        "tensor_view_read_only",
    );
}

// =============================================================================
// Accepted: no overlap, or overlap that is only read/read
// =============================================================================

#[test]
fn aliased_read_arguments_stay_legal() {
    assert_accepts("    var tf32[2, 2] alias ← g\n    read_read(g, alias)");
    assert_accepts("    var tf32[2, 2] alias ← g\n    read_read(alias, g)");
}

#[test]
fn aliased_ref_arguments_stay_legal() {
    let found = errors(
        "    var tf32[2, 2] alias ← g\n    const ref tf32[2, 2] r ← g\n    read_read(alias, r)",
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn copy_bind_does_not_alias() {
    assert_accepts("    var tf32[2, 2] dup ← copy g\n    mut_read(mut g, dup)");
    assert_accepts("    var tf32[2, 2] dup ← copy g\n    mut_read(mut dup, g)");
    assert_accepts("    var tf32[2, 2] dup ← copy g\n    mut_mut(mut g, mut dup)");
}

#[test]
fn own_bind_does_not_alias() {
    let found = errors("    const tf32[2, 2] taken ← own g\n    mut_mut(mut g, mut taken)");
    assert!(
        !found.iter().any(|(_, issue)| matches!(
            issue.as_str(),
            "mutable_borrow_conflict" | "shared_borrow_conflict" | "use_while_mutably_borrowed"
        )),
        "{found:?}"
    );
}

#[test]
fn unrelated_tensor_is_not_an_overlap() {
    assert_accepts("    mut_read(mut g, fresh)");
    assert_accepts("    mut_mut(mut g, mut fresh)");
    assert_accepts("    var tf32[2, 2] alias ← g\n    mut_mut(mut alias, mut fresh)");
    assert_accepts("    var tf32[2, 2] alias ← g\n    mut_mut(mut fresh, mut alias)");
}

#[test]
fn alias_of_another_tensor_does_not_overlap_this_one() {
    assert_accepts("    var tf32[2, 2] alias ← fresh\n    mut_read(mut g, alias)");
    assert_accepts("    var tf32[2, 2] alias ← fresh\n    mut_mut(mut alias, mut g)");
}

#[test]
fn sequential_calls_release_call_scoped_borrows() {
    assert_accepts(
        "    var tf32[2, 2] alias ← g\n    mut_read(mut g, fresh)\n    mut_read(mut alias, fresh)\n    mut_mut(mut fresh, mut g)\n    mut_read(mut fresh, alias)",
    );
}

// =============================================================================
// Flow: an unconditional rebind detaches; a conditional rebind may-aliases
// =============================================================================

#[test]
fn source_rebind_detaches_the_stale_alias() {
    assert_accepts(
        "    var tf32[2, 2] alias ← g\n    g ← fresh\n    mut_read(mut g, alias)\n    mut_mut(mut alias, mut g)",
    );
}

#[test]
fn source_rebind_keeps_the_other_aliases_of_the_old_storage() {
    assert_rejects(
        "    var tf32[2, 2] left ← g\n    var tf32[2, 2] right ← g\n    g ← fresh\n    mut_mut(mut left, mut right)",
        "mutable_borrow_conflict",
    );
}

#[test]
fn conditional_alias_rebind_keeps_the_may_alias() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    if true {\n        alias ← fresh\n    }\n    mut_read(mut g, alias)",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn conditional_source_rebind_keeps_the_may_alias() {
    assert_rejects(
        "    var tf32[2, 2] alias ← g\n    if true {\n        g ← fresh\n    }\n    mut_read(mut g, alias)",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn unconditional_alias_rebind_clears_the_alias() {
    assert_accepts("    var tf32[2, 2] alias ← g\n    alias ← fresh\n    mut_read(mut g, alias)");
}

// =============================================================================
// User method calls: the receiver's methods obey the same overlap rule
// =============================================================================

const METHOD_PRELUDE: &str = r"class Kern {
    fn mut_read(mut tf32[2, 2] a, tf32[2, 2] b) → void {
        a ⇇ b
    }

    fn mut_ref(mut tf32[2, 2] a, ref tf32[2, 2] b) → void {
        a ⇇ b
    }

    fn mut_mut(mut tf32[2, 2] a, mut tf32[2, 2] b) → void {
        a ⇇ b
    }

    fn read_read(tf32[2, 2] a, tf32[2, 2] b) → void {
        print a.flatten()
        print b.flatten()
    }
}
";

/// `body` runs in `main` after `g`, `fresh` and a `Kern` receiver `k`.
fn method_program(body: &str) -> String {
    format!(
        r"{METHOD_PRELUDE}
main {{
    const list<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    var tf32[2, 2] g ← flat ↦ tf32[2, 2]
    var tf32[2, 2] fresh ← flat ↦ tf32[2, 2]
    const Kern k ← Kern {{}}
{body}
}}
"
    )
}

fn method_errors(body: &str) -> Vec<(SemanticErrorKind, String)> {
    analyze_en(&method_program(body))
        .result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| (d.kind.clone(), issue(d).unwrap_or_default().to_string()))
        .collect()
}

fn assert_method_accepts(body: &str) {
    let found = method_errors(body);
    assert!(found.is_empty(), "expected no error, got: {found:?}");
}

fn assert_method_rejects(body: &str, expected: &str) {
    let found = method_errors(body);
    assert!(
        found.iter().any(|(_, issue)| issue == expected),
        "expected issue {expected:?}, got: {found:?}"
    );
}

#[test]
fn method_call_with_the_same_tensor_mut_and_read_is_rejected() {
    assert_method_rejects("    k.mut_read(mut g, g)", "use_while_mutably_borrowed");
}

#[test]
fn method_call_with_the_same_tensor_in_two_mut_arguments_is_rejected() {
    assert_method_rejects("    k.mut_mut(mut g, mut g)", "mutable_borrow_conflict");
}

#[test]
fn method_call_with_the_same_tensor_mut_and_ref_is_rejected() {
    assert_method_rejects("    k.mut_ref(mut g, g)", "shared_borrow_conflict");
}

#[test]
fn method_call_overlap_through_an_alias_is_rejected() {
    assert_method_rejects(
        "    var tf32[2, 2] alias ← g\n    k.mut_read(mut g, alias)",
        "use_while_mutably_borrowed",
    );
    assert_method_rejects(
        "    var tf32[2, 2] alias ← g\n    k.mut_read(mut alias, g)",
        "use_while_mutably_borrowed",
    );
    assert_method_rejects(
        "    var tf32[2, 2] alias ← g\n    k.mut_mut(mut g, mut alias)",
        "mutable_borrow_conflict",
    );
}

#[test]
fn method_call_view_of_the_mut_argument_is_rejected() {
    assert_method_rejects(
        "    k.mut_read(mut g, g.transpose())",
        "tensor_view_read_only",
    );
}

#[test]
fn method_call_without_overlap_stays_legal() {
    assert_method_accepts("    k.mut_read(mut g, fresh)");
    assert_method_accepts("    k.mut_mut(mut g, mut fresh)");
    assert_method_accepts("    k.read_read(g, g)");
    assert_method_accepts("    var tf32[2, 2] dup ← copy g\n    k.mut_read(mut g, dup)");
}

#[test]
fn sequential_method_calls_release_call_scoped_borrows() {
    assert_method_accepts("    k.mut_read(mut g, fresh)\n    k.mut_read(mut g, fresh)");
    assert_method_accepts(
        "    var tf32[2, 2] alias ← g\n    k.mut_read(mut g, fresh)\n    k.mut_read(mut alias, fresh)",
    );
}
