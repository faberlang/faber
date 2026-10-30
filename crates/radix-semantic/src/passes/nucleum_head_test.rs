//! Kernel declaration head (operator ruling 2026-10-30) lowers to the same
//! function facts as the `@ nucleum` annotation form: `is_kernel`, the
//! fragment bit, the tier annotation field, and the requirement-report row.
//!
//! Identity assertions target the structured `issue`, never rendered prose.

use crate::PassConfig;
use crate::locale::LocalePack;
use crate::test_support::{
    AnalyzedFixture, analyze_source_with_config, analyze_source_with_config_la,
};
use radix_hir::{HirItemKind, NucleumRole};
use radix_syntax::KernelTier;
use radix_target::Target;

fn en_config() -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    config
}

fn analyze_en(source: &str) -> AnalyzedFixture {
    analyze_source_with_config(source, en_config())
}

fn analyze_la(source: &str) -> AnalyzedFixture {
    analyze_source_with_config_la(source, PassConfig::for_target(Target::HirRust))
}

/// One `(name, kernel facts)` row per function in the lowered module.
fn kernel_fact_rows(
    fixture: &AnalyzedFixture,
) -> Vec<(String, bool, bool, KernelTier, Option<NucleumRole>)> {
    let hir = fixture.hir.as_ref().expect("analyzed module carries HIR");
    let mut rows = Vec::new();
    for item in &hir.items {
        let HirItemKind::Function(function) = &item.kind else {
            continue;
        };
        let tier = item
            .annotations
            .iter()
            .find(|annotation| fixture.interner.resolve(annotation.family) == "nucleum")
            .map(|annotation| annotation.kernel_tier(&fixture.interner))
            .unwrap_or(KernelTier::Default);
        let role = function.nucleum_role();
        rows.push((
            fixture.interner.resolve(function.name).to_owned(),
            function.is_kernel,
            function.nucleum_fragment,
            tier,
            role,
        ));
    }
    rows.sort_by(|left, right| left.0.cmp(&right.0));
    rows
}

/// The kernel-requirement rows the report publishes, keyed by name and tier.
fn requirement_tiers(fixture: &AnalyzedFixture) -> Vec<(String, KernelTier)> {
    let mut rows: Vec<(String, KernelTier)> = fixture
        .kernel_requirements
        .kernels
        .iter()
        .map(|kernel| (kernel.name.clone(), kernel.tier))
        .collect();
    rows.sort();
    rows
}

fn assert_head_matches_annotation_form(
    annotation_form: &str,
    head_form: &str,
    analyze: fn(&str) -> AnalyzedFixture,
) {
    let annotation = analyze(annotation_form);
    assert!(
        annotation.is_ok(),
        "annotation form: {:?}",
        annotation.diagnostics
    );
    let head = analyze(head_form);
    assert!(head.is_ok(), "head form: {:?}", head.diagnostics);
    assert_eq!(
        kernel_fact_rows(&annotation),
        kernel_fact_rows(&head),
        "the two heads must lower to the same function facts"
    );
    assert_eq!(
        requirement_tiers(&annotation),
        requirement_tiers(&head),
        "the two heads must publish the same requirement rows"
    );
}

#[test]
fn head_form_lowers_to_the_annotation_form_facts_in_english() {
    for (annotation_form, head_form) in [
        (
            "@ kernel\n@ public\nfn plain(f32 x) → f32 { return x + 1.0 }\n",
            "@ public\nkernel plain(f32 x) → f32 { return x + 1.0 }\n",
        ),
        (
            "@ kernel relaxed\n@ public\nfn loose(f32 x) → f32 { return x + 1.0 }\n",
            "@ public\nkernel loose(f32 x) relaxed → f32 { return x + 1.0 }\n",
        ),
        (
            "@ kernel unsafe\n@ public\nfn raw(f32 x) → f32 { return x + 1.0 }\n",
            "@ public\nkernel raw(f32 x) unsafe → f32 { return x + 1.0 }\n",
        ),
        (
            "@ kernel fragment\nfn shade() → void { }\n",
            "kernel shade() fragment → void { }\n",
        ),
    ] {
        assert_head_matches_annotation_form(annotation_form, head_form, analyze_en);
    }
}

#[test]
fn head_form_lowers_to_the_annotation_form_facts_in_latin() {
    for (annotation_form, head_form) in [
        (
            "@ nucleum\n@ publica\nfunctio plain(f32 x) → f32 { redde x + 1.0 }\n",
            "@ publica\nnucleum plain(f32 x) → f32 { redde x + 1.0 }\n",
        ),
        (
            "@ nucleum laxus\n@ publica\nfunctio loose(f32 x) → f32 { redde x + 1.0 }\n",
            "@ publica\nnucleum loose(f32 x) laxus → f32 { redde x + 1.0 }\n",
        ),
        (
            "@ nucleum fragment\nfunctio umbra() → vacuum { }\n",
            "nucleum umbra() fragment → vacuum { }\n",
        ),
    ] {
        assert_head_matches_annotation_form(annotation_form, head_form, analyze_la);
    }
}

#[test]
fn a_fragment_head_row_reads_as_a_fragment_role() {
    let fixture = analyze_la("nucleum umbra() fragment → vacuum { }\n");
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    let rows = kernel_fact_rows(&fixture);
    let (name, is_kernel, nucleum_fragment, tier, role) = &rows[0];
    assert_eq!(name, "umbra");
    // A fragment rides the fragment bit, not the entry bit — the same facts
    // the `@ nucleum fragment` annotation form lowers to.
    assert!(!is_kernel, "a fragment is not an entry kernel");
    assert!(*nucleum_fragment);
    assert_eq!(*tier, KernelTier::Default);
    assert_eq!(*role, Some(NucleumRole::Fragment));
}

#[test]
fn both_heads_on_one_declaration_never_reach_analysis() {
    let fixture = analyze_la("@ nucleum\nnucleum k(f32 x) → f32 { redde x }\n");
    assert!(fixture.is_err(), "the double head is a parse refusal");
}

#[test]
fn a_head_form_kernel_keeps_the_runner_refusal() {
    let fixture = analyze_la("nucleum k(f32 x) → f32 { redde x }\n");
    assert!(fixture.is_ok(), "{:?}", fixture.diagnostics);
    let rows = kernel_fact_rows(&fixture);
    assert!(rows[0].1, "the head form marks is_kernel");
}
