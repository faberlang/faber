//! Ranged store policies: `saturating|trapping|wrapping<W, lo‥hi>`.
//!
//! A range is the second argument of the existing store policies. The width
//! stays explicit, the range must fit it, the policy word is mandatory, and
//! the existing spellings are sugar for the whole-width range (one type
//! identity, not a new family).

use crate::driver::AnalyzeRequest;
use crate::{Config, Target};

fn pack(id: &str) -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(id))
        .unwrap_or_else(|err| panic!("installed {id} pack should load: {err:?}"))
}

fn diagnostics_with(config: Config, source: &str) -> Vec<crate::Diagnostic> {
    let session = crate::driver::Session::new(config);
    match crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()) {
        Ok(unit) => unit.diagnostics,
        Err(diagnostics) => diagnostics,
    }
}

fn english(source: &str) -> Vec<crate::Diagnostic> {
    diagnostics_with(
        Config::new()
            .with_locale_pack(pack("en"))
            .with_target(Target::HirRust),
        source,
    )
}

fn latin(source: &str) -> Vec<crate::Diagnostic> {
    diagnostics_with(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirRust),
        source,
    )
}

fn assert_accepts(diagnostics: &[crate::Diagnostic]) {
    assert!(
        !diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error),
        "diagnostics: {diagnostics:?}"
    );
}

fn has_issue(diagnostics: &[crate::Diagnostic], issue: &str) -> bool {
    diagnostics.iter().any(|diagnostic| {
        diagnostic.is_error()
            && diagnostic
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == issue)
    })
}

fn assert_issue(diagnostics: &[crate::Diagnostic], issue: &str) {
    assert!(
        has_issue(diagnostics, issue),
        "expected error issue {issue:?}, got: {diagnostics:?}",
    );
}

fn refused(source: &str, issue: &str) {
    assert_issue(&english(source), issue);
}

#[test]
fn each_policy_takes_a_range_as_its_second_argument() {
    assert_accepts(&english(
        r"
main {
    let saturating<u32, 0‥8> cx ← 20
    let trapping<i32, -2‥3> dy ← 2
    let wrapping<u32, 0‥5> k ← 12
    print cx
    print dy
    print k
}
",
    ));
}

#[test]
fn the_inclusive_glyph_holds_the_high_end_too() {
    assert_accepts(&english(
        r"
main {
    let trapping<u8, 1…5> a ← 5
    print a
}
",
    ));
}

#[test]
fn infinity_is_no_bound_on_that_side() {
    assert_accepts(&english(
        r"
main {
    let saturating<u32, 4‥∞> floor ← 1
    let saturating<i32, -∞‥8> ceiling ← 99
    print floor
    print ceiling
}
",
    ));
}

#[test]
fn the_latin_reader_spells_the_same_policies() {
    assert_accepts(&latin(
        r"
functio pone(saturatus<u32, 0‥8> a, exactus<i32, -2‥3> b, modulus<u32, 0‥5> c) → exactus<i32, -2‥3> {
    redde b
}
",
    ));
}

#[test]
fn a_ranged_cell_reads_as_its_width_in_arithmetic() {
    assert_accepts(&english(
        r"
fn widen(saturating<u32, 0‥8> a, saturating<u32, 0‥5> b) → saturating<u32> {
    let saturating<u32> sum ← a + b
    return sum
}
",
    ));
}

#[test]
fn a_ranged_cell_flows_between_ranges_of_one_policy_as_a_store() {
    // Widening is a plain read; narrowing is the same-policy store that the
    // existing width law already allows (`saturating<u32>` into
    // `saturating<u8>`), so the destination range is applied there.
    assert_accepts(&english(
        r"
fn grow(trapping<u8, 0‥10> small) → trapping<u8, 0‥20> {
    let trapping<u8, 0‥20> large ← small
    return large
}
fn shrink(saturating<u32, 0‥20> large) → saturating<u32, 0‥10> {
    let saturating<u32, 0‥10> small ← large
    return small
}
",
    ));
}

#[test]
fn a_ranged_cell_does_not_cross_into_another_policy_family_without_a_conversion() {
    refused(
        r"
fn cross(saturating<u32, 0‥8> a) → wrapping<u32, 0‥8> {
    let wrapping<u32, 0‥8> w ← a
    return w
}
",
        "expression_type_mismatch",
    );
}

#[test]
fn converting_between_ranges_is_a_store() {
    assert_accepts(&english(
        r"
fn narrow(saturating<u32, 0‥20> large) → saturating<u32, 0‥10> {
    return large ↦ saturating<u32, 0‥10>
}
",
    ));
}

#[test]
fn the_whole_width_range_is_the_existing_spelling() {
    // `saturating<u8>` is `saturating<u8, 0…255>`: one type identity.
    assert_accepts(&english(
        r"
fn same(saturating<u8> a, saturating<u8, 0…255> b) → saturating<u8, 0‥256> {
    let saturating<u8, 0…255> x ← a
    let saturating<u8> y ← b
    let trapping<u8> z ← 1
    let trapping<u8, 0…255> w ← z
    let wrapping<u8> p ← 1 ↦ wrapping<u8>
    let wrapping<u8, 0…255> q ← p
    return x + y
}
",
    ));
    // `inf` is `trapping<inf, -∞…∞>`: no bound on either side.
    assert_accepts(&english(
        r"
fn same(inf a, trapping<inf, -∞…∞> b) → inf {
    let inf x ← b
    let trapping<inf, -∞…∞> y ← a
    return x + y
}
",
    ));
}

#[test]
fn the_policy_word_is_mandatory_with_a_range() {
    refused(
        r"
main {
    let u8<0‥8> x ← 1
    print x
}
",
        "ranged_policy_word_required",
    );
}

#[test]
fn an_open_ended_range_must_write_both_ends() {
    let diagnostics = english(
        r"
main {
    let saturating<u32, 0…> x ← 1
    print x
}
",
    );
    assert!(
        diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error),
        "a one-sided range is refused: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(|d| d
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "range_bound_required")),
        "refused by name: {diagnostics:?}"
    );
}

#[test]
fn an_empty_range_is_refused() {
    refused(
        r"
main {
    let saturating<u32, 5‥5> x ← 1
    print x
}
",
        "ranged_policy_range_empty",
    );
    refused(
        r"
main {
    let saturating<u32, 6‥2> x ← 1
    print x
}
",
        "ranged_policy_range_empty",
    );
}

#[test]
fn a_range_outside_the_width_is_refused() {
    refused(
        r"
main {
    let trapping<u8, 0‥300> x ← 1
    print x
}
",
        "ranged_policy_range_outside_width",
    );
    refused(
        r"
main {
    let trapping<i8, -300‥5> x ← 1
    print x
}
",
        "ranged_policy_range_outside_width",
    );
}

#[test]
fn an_unsigned_width_refuses_a_negative_low_end() {
    refused(
        r"
main {
    let trapping<u8, -1‥5> x ← 1
    print x
}
",
        "ranged_policy_unsigned_negative_bound",
    );
}

#[test]
fn a_signed_width_takes_negative_bounds() {
    assert_accepts(&english(
        r"
main {
    let wrapping<i16, -100‥100> x ← 101
    print x
}
",
    ));
}

#[test]
fn float_and_inf_widths_take_no_range() {
    refused(
        r"
main {
    let saturating<f32, 0‥8> x ← 1
    print x
}
",
        "ranged_policy_width_unsupported",
    );
    refused(
        r"
main {
    let trapping<inf, 0‥8> x ← 1
    print x
}
",
        "ranged_policy_width_unsupported",
    );
}

#[test]
fn a_run_time_bound_is_refused_by_name() {
    refused(
        r"
fn bound(int n) → void {
    let saturating<u32, 0‥n> x ← 1
    print x
}
",
        "ranged_policy_bound_not_constant",
    );
}

#[test]
fn policies_do_not_mix_across_families() {
    assert_issue(
        &english(
            r"
fn mix(saturating<u32, 0‥8> a, wrapping<u32, 0‥8> b) → u32 {
    return a + b
}
",
        ),
        "saturating_word_family_mismatch",
    );
}

#[test]
fn a_kernel_refuses_a_ranged_cell_by_name() {
    let diagnostics = english(
        r"
@ kernel
@ public
fn launch(saturating<u32, 0‥8> lane) → void {
}
",
    );
    assert_issue(&diagnostics, "nucleum_ranged_policy_not_built");
}

#[cfg(feature = "hir-rust")]
fn rust_code(source: &str) -> String {
    let result = crate::Compiler::new(
        Config::new()
            .with_locale_pack(pack("en"))
            .with_target(Target::HirRust),
    )
    .compile_str("test.fab", source);
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected Rust output")
    };
    output.code
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_codegen_applies_each_range_once_at_the_store() {
    let code = rust_code(
        r"
fn pin(int n) → int {
    let saturating<u32, 0‥8> cx ← n ↦ saturating<u32, 0‥8>
    let wrapping<u32, 0‥5> k ← n ↦ wrapping<u32, 0‥5>
    let trapping<i32, -2‥3> dy ← 1
    let _ sum ← cx + cx
    print sum
    print k
    print dy
    return n
}
main {
    print pin(9)
}
",
    );
    assert!(code.contains(".clamp(0_i128, 7_i128)"), "no clamp:\n{code}");
    assert!(code.contains(".rem_euclid(5_i128)"), "no wrap:\n{code}");
    for per_step in ["saturating_add", "wrapping_add", "checked_add"] {
        assert!(!code.contains(per_step), "per-operation {per_step} must not appear:\n{code}");
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_codegen_names_the_range_in_a_trapping_store() {
    let code = rust_code(
        r"
main {
    let i32 d ← 3
    let trapping<i32, -2‥3> dy ← d
    print dy
}
",
    );
    assert!(code.contains("trapping<i32, -2‥3>"), "range not named:\n{code}");
}
