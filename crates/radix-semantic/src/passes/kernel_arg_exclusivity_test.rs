//! Kernel argument exclusivity (ruling 7b25889f R6 K1): a kernel's arguments
//! are device buffers live simultaneously for a whole launch, so a writable
//! argument (`mut`, `own`) must not overlap any other argument of the same
//! call — whole tensor, `←` alias, or view of one storage. Where overlap
//! cannot be proven absent the call is refused with
//! `kernel_argument_overlap`. Read-only duplicates, read-only overlapping
//! views, disjoint tensors and `copy`/`materialize()` independence stay legal.
//! View-shaped overlaps stay with `tensor_view_read_only`; ordinary (non
//! kernel) calls keep their call-scoped borrow behavior.

use crate::SemanticErrorKind;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};

const PRELUDE: &str = r"@ kernel
@ public
fn k_read_write(mut tf32[2, 2] a, tf32[2, 2] b) → void {
    a ⇇ b
}

@ kernel
@ public
fn k_write_read(tf32[2, 2] a, mut tf32[2, 2] b) → void {
    b ⇇ a
}

@ kernel
@ public
fn k_read_read(tf32[2, 2] a, tf32[2, 2] b) → void {
    let f32 v ← a[[0, 0]] + b[[0, 0]]
}

@ kernel
@ public
fn k_two_writable(mut tf32[2, 2] a, mut tf32[2, 2] b) → void {
    a ⇇ b
}

@ kernel
@ public
fn k_own_read(own tf32[2, 2] a, tf32[2, 2] b) → void {
    let f32 v ← a[[0, 0]] + b[[0, 0]]
}

@ kernel
@ public
fn k_two_own(own tf32[2, 2] a, own tf32[2, 2] b) → void {
    let f32 v ← a[[0, 0]] + b[[0, 0]]
}

@ kernel
@ public
fn k_writable_scalar(mut tf32[2, 2] a, int n) → void {
    let f32 v ← (n ↦ f32) + a[[0, 0]]
}

fn host_read_write(mut tf32[2, 2] a, tf32[2, 2] b) → void {
    a ⇇ b
}

fn host_write_read(tf32[2, 2] a, mut tf32[2, 2] b) → void {
    b ⇇ a
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
    let list<f32> flat ← [1.0, 2.0, 3.0, 4.0]
    mut tf32[2, 2] g ← flat ↦ tf32[2, 2]
    mut tf32[2, 2] fresh ← flat ↦ tf32[2, 2]
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

fn assert_lacks(body: &str, unwanted: &str) {
    let found = errors(body);
    assert!(
        !found.iter().any(|(_, issue)| issue == unwanted),
        "did not expect issue {unwanted:?}, got: {found:?}"
    );
}

// =============================================================================
// Rejected: a writable kernel argument overlaps another argument
// =============================================================================

#[test]
fn writable_after_the_same_tensor_is_rejected() {
    assert_rejects("    k_read_write(mut g, g)", "kernel_argument_overlap");
}

#[test]
fn writable_before_the_same_tensor_is_rejected() {
    assert_rejects("    k_write_read(g, mut g)", "kernel_argument_overlap");
}

#[test]
fn writable_after_an_alias_is_rejected() {
    assert_rejects(
        "    mut tf32[2, 2] alias ← g\n    k_read_write(mut g, alias)",
        "kernel_argument_overlap",
    );
}

#[test]
fn writable_before_an_alias_is_rejected() {
    assert_rejects(
        "    mut tf32[2, 2] alias ← g\n    k_write_read(alias, mut g)",
        "kernel_argument_overlap",
    );
}

#[test]
fn two_writable_arguments_through_an_alias_are_rejected_once() {
    let found = errors("    mut tf32[2, 2] alias ← g\n    k_two_writable(mut g, mut alias)");
    assert_eq!(
        found
            .iter()
            .filter(|(_, issue)| issue == "kernel_argument_overlap")
            .count(),
        1,
        "one diagnostic for one overlapping pair, got: {found:?}"
    );
}

#[test]
fn own_argument_overlapping_an_alias_is_rejected() {
    assert_rejects(
        "    mut tf32[2, 2] alias ← g\n    k_own_read(own g, alias)",
        "kernel_argument_overlap",
    );
}

#[test]
fn two_own_arguments_sharing_storage_are_rejected() {
    assert_rejects(
        "    mut tf32[2, 2] alias ← g\n    k_two_own(own g, own alias)",
        "kernel_argument_overlap",
    );
}

#[test]
fn a_view_of_the_writable_argument_keeps_its_own_diagnostic() {
    // The view overlap is `tensor_view_read_only`'s to name; the exclusivity
    // rule must not stack a second diagnostic on the same pair.
    let found = errors("    k_read_write(mut g, g.transpose())");
    assert!(
        found
            .iter()
            .any(|(_, issue)| issue == "tensor_view_read_only"),
        "{found:?}"
    );
    assert_lacks(
        "    k_read_write(mut g, g.transpose())",
        "kernel_argument_overlap",
    );
}

#[test]
fn a_view_in_the_writable_position_keeps_its_own_diagnostic() {
    let found = errors("    let tf32[2, 2] v ← g.transpose()\n    k_read_write(mut v, fresh)");
    assert!(
        found
            .iter()
            .any(|(_, issue)| issue == "tensor_view_read_only"),
        "{found:?}"
    );
}

// =============================================================================
// Accepted: no writable overlap, or overlap that is only read/read
// =============================================================================

#[test]
fn read_only_duplicates_stay_legal() {
    assert_accepts("    k_read_read(g, g)");
}

#[test]
fn read_only_overlapping_views_stay_legal() {
    assert_accepts("    k_read_read(g.transpose(), g.transpose())");
    assert_accepts("    let tf32[2, 2] v ← g.transpose()\n    k_read_read(v, g.transpose())");
}

#[test]
fn writable_with_disjoint_tensors_stays_legal() {
    assert_accepts("    k_read_write(mut g, fresh)");
    assert_accepts("    k_write_read(fresh, mut g)");
    assert_accepts("    k_two_writable(mut g, mut fresh)");
}

#[test]
fn an_independent_copy_beside_the_writable_original_stays_legal() {
    assert_accepts("    mut tf32[2, 2] dup ← copy g\n    k_read_write(mut g, dup)");
    assert_accepts("    mut tf32[2, 2] dup ← copy g\n    k_write_read(dup, mut g)");
    assert_accepts(
        "    let tf32[2, 2] solid ← g.transpose().materialize()\n    k_write_read(solid, mut g)",
    );
}

#[test]
fn a_scalar_argument_never_overlaps() {
    assert_accepts("    k_writable_scalar(mut g, 3)");
}

// =============================================================================
// Scope: the rule is the kernel call's, not the ordinary call's
// =============================================================================

#[test]
fn ordinary_calls_keep_their_call_scoped_borrow_behavior() {
    // Read-before-writable stays legal on an ordinary call: only the kernel
    // contract refuses it.
    assert_accepts("    host_write_read(g, mut g)");
    assert_accepts("    mut tf32[2, 2] alias ← g\n    host_write_read(alias, mut g)");
    // Writable-first keeps its borrow conflict on both kinds of call.
    assert_rejects(
        "    host_read_write(mut g, g)",
        "use_while_mutably_borrowed",
    );
}
