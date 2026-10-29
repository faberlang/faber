//! Kernel safety tiers (operator ruling 2026-10-28): the tier word parses in
//! the English and Latin surfaces, reaches the requirement report, is refused
//! by name when misused, and the safe switch lowers it to the default tier.
//!
//! Every refusal asserts the structured `issue`, never rendered prose.

use crate::PassConfig;
use crate::locale::LocalePack;
use crate::test_support::{
    AnalyzedFixture, analyze_source_la, analyze_source_with_config, analyze_source_with_config_la,
};
use radix_hir::KernelTier;
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

fn tier_of(fixture: &AnalyzedFixture, name: &str) -> KernelTier {
    fixture
        .kernel_requirements
        .kernels
        .iter()
        .find(|kernel| kernel.name == name)
        .unwrap_or_else(|| panic!("missing kernel row for {name}"))
        .tier
}

fn issues(fixture: &AnalyzedFixture) -> Vec<String> {
    fixture
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.issue().map(str::to_owned))
        .collect()
}

const EN_KERNELS: &str = r"
@ kernel
@ public
fn plain(f32 x) → f32 { return x + 1.0 }

@ kernel relaxed
@ public
fn loose(f32 x) → f32 { return x + 1.0 }

@ kernel unsafe
@ public
fn raw(f32 x) → f32 { return x + 1.0 }
";

const LA_KERNELS: &str = "
@ nucleum
@ publica
functio plain(f32 x) → f32 { redde x + 1.0 }

@ nucleum laxus
@ publica
functio loose(f32 x) → f32 { redde x + 1.0 }

@ nucleum intutus
@ publica
functio raw(f32 x) → f32 { redde x + 1.0 }
";

#[test]
fn english_tier_words_reach_the_requirement_report() {
    let unit = analyze_en(EN_KERNELS);
    assert!(unit.is_ok(), "diagnostics: {:?}", unit.diagnostics);
    assert_eq!(tier_of(&unit, "plain"), KernelTier::Default);
    assert_eq!(tier_of(&unit, "loose"), KernelTier::Relaxed);
    assert_eq!(tier_of(&unit, "raw"), KernelTier::Unsafe);
}

#[test]
fn latin_tier_words_reach_the_requirement_report() {
    let unit = analyze_source_la(LA_KERNELS);
    assert!(unit.is_ok(), "diagnostics: {:?}", unit.diagnostics);
    assert_eq!(tier_of(&unit, "plain"), KernelTier::Default);
    assert_eq!(tier_of(&unit, "loose"), KernelTier::Relaxed);
    assert_eq!(tier_of(&unit, "raw"), KernelTier::Unsafe);
}

#[test]
fn the_tiers_are_ordered_and_unsafe_includes_relaxed() {
    assert!(KernelTier::Default < KernelTier::Relaxed);
    assert!(KernelTier::Relaxed < KernelTier::Unsafe);
    assert!(!KernelTier::Default.allows_relaxed_math());
    assert!(KernelTier::Relaxed.allows_relaxed_math());
    assert!(KernelTier::Unsafe.allows_relaxed_math());
    assert!(!KernelTier::Relaxed.allows_unchecked_indexing());
    assert!(KernelTier::Unsafe.allows_unchecked_indexing());
}

#[test]
fn the_safe_switch_lowers_every_tier_to_the_default() {
    let mut config = en_config();
    config.deny_kernel_tiers = true;
    let unit = analyze_source_with_config(EN_KERNELS, config);
    assert!(unit.is_ok(), "diagnostics: {:?}", unit.diagnostics);
    for name in ["plain", "loose", "raw"] {
        assert_eq!(tier_of(&unit, name), KernelTier::Default, "{name}");
    }

    let mut config = PassConfig::for_target(Target::HirRust);
    config.deny_kernel_tiers = true;
    let unit = analyze_source_with_config_la(LA_KERNELS, config);
    assert!(unit.is_ok(), "diagnostics: {:?}", unit.diagnostics);
    for name in ["plain", "loose", "raw"] {
        assert_eq!(tier_of(&unit, name), KernelTier::Default, "{name}");
    }
}

#[test]
fn a_second_tier_word_is_refused_by_name() {
    for source in [
        "@ kernel relaxed unsafe\nfn k(f32 x) → f32 { return x }\n",
        "@ kernel relaxed relaxed\nfn k(f32 x) → f32 { return x }\n",
        "@ kernel unsafe relaxed\nfn k(f32 x) → f32 { return x }\n",
    ] {
        let unit = analyze_en(source);
        assert!(
            issues(&unit).contains(&"duplicate_nucleum_tier".to_owned()),
            "{source}: {:?}",
            unit.diagnostics
        );
    }
}

#[test]
fn an_unknown_tier_is_refused_by_name() {
    let unit = analyze_en("@ kernel fast\nfn k(f32 x) → f32 { return x }\n");
    assert!(issues(&unit).contains(&"unknown_nucleum_tier".to_owned()));
    let name = unit
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.issue() == Some("unknown_nucleum_tier"))
        .and_then(|diagnostic| diagnostic.args.iter().find(|arg| arg.name == "name"))
        .map(|arg| arg.value.clone());
    assert_eq!(name.as_deref(), Some("fast"));
}

#[test]
fn a_tier_on_a_fragment_is_refused_by_name() {
    for source in [
        "@ kernel fragment relaxed\nfn k(f32 x) → f32 { return x }\n",
        "@ kernel relaxed fragment\nfn k(f32 x) → f32 { return x }\n",
    ] {
        let unit = analyze_en(source);
        assert!(
            issues(&unit).contains(&"nucleum_tier_on_fragment".to_owned()),
            "{source}: {:?}",
            unit.diagnostics
        );
    }
}

#[test]
fn a_tier_on_a_non_kernel_declaration_is_refused_by_name() {
    for source in [
        "@ kernel relaxed\nclass P { let int a }\n",
        "class P {\n    @ kernel unsafe\n    let int a\n}\n",
    ] {
        let unit = analyze_en(source);
        assert!(
            issues(&unit).contains(&"nucleum_tier_non_kernel".to_owned()),
            "{source}: {:?}",
            unit.diagnostics
        );
    }
}

#[test]
fn a_tier_on_a_kernel_method_is_accepted() {
    let unit = analyze_en(
        "class P {\n    let int a\n    @ kernel relaxed\n    fn k(f32 x) → f32 { return x }\n}\n",
    );
    assert!(
        !issues(&unit).contains(&"nucleum_tier_non_kernel".to_owned()),
        "diagnostics: {:?}",
        unit.diagnostics
    );
}

#[test]
fn tier_words_are_ordinary_identifiers_outside_the_annotation() {
    let unit = analyze_en("fn f(int relaxed, int unsafe) → int { return relaxed + unsafe }\n");
    assert!(unit.is_ok(), "diagnostics: {:?}", unit.diagnostics);
}

#[test]
fn the_other_locale_word_is_not_a_tier() {
    // Locale surfaces are sealed: the Latin word in an English file and the
    // English word in a Latin file are plain unknown tiers.
    let unit = analyze_en("@ kernel laxus\nfn k(f32 x) → f32 { return x }\n");
    assert!(issues(&unit).contains(&"unknown_nucleum_tier".to_owned()));
    let unit = analyze_source_la("@ nucleum relaxed\nfunctio k(f32 x) → f32 { redde x }\n");
    assert!(issues(&unit).contains(&"unknown_nucleum_tier".to_owned()));
}
