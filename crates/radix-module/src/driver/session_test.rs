//! Tests for `Config` defaults, builder methods, and `Session` wrapping.
//!
//! Contract: `Session` is a thin wrapper over `Config`; config fields are stable across default construction.

use super::{Config, Session, WarnPolicy};
use crate::codegen::Target;
use crate::diagnostics::Severity;
use std::path::PathBuf;

#[test]
fn config_default_values_are_stable() {
    let config = Config::default();

    assert_eq!(config.target, Target::HirRust);
    assert!(config.emit_comments);
    assert!(!config.warn_policy.deny_all_warnings);
    assert!(config.warn_policy.deny_codes.is_empty());
    // The default config discovers the workspace stdlib tree so
    // frontmatter-locale sources can load their reader packs without every
    // caller reaching for `with_dev_stdlib()`.
    let expected = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib");
    assert_eq!(
        config.stdlib_path,
        expected.is_dir().then_some(expected),
        "default config should discover the workspace stdlib when present"
    );
    assert!(config.locale_pack.is_none());
    assert!(!config.emit_nvvm_device_module);
}

#[test]
fn config_builders_apply_expected_fields() {
    let stdlib = PathBuf::from("/tmp/norma");
    let config = Config::new()
        .with_target(Target::HirTypeScript)
        .with_stdlib(stdlib.clone())
        .with_warn_policy(WarnPolicy { deny_all_warnings: true, deny_codes: vec!["WARN006".into()] });

    assert_eq!(config.target, Target::HirTypeScript);
    assert_eq!(config.stdlib_path, Some(stdlib));
    assert!(config.warn_policy.deny_all_warnings);
    assert_eq!(config.warn_policy.deny_codes, vec!["WARN006"]);
}

#[test]
fn config_builder_accepts_locale_pack() {
    let pack = crate::locale::LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");
    let config = Config::new().with_locale_pack(pack);

    assert_eq!(
        config
            .locale_pack
            .as_ref()
            .map(|pack| pack.metadata.id.as_str()),
        Some("zh-Hans")
    );
}

#[test]
fn session_wraps_config() {
    let config = Config::default().with_target(Target::HirTypeScript);
    let session = Session::new(config);

    assert_eq!(session.config.target, Target::HirTypeScript);
}

#[test]
fn config_builder_enables_nvvm_device_module() {
    let config = Config::new().with_nvvm_device_module();
    assert!(config.emit_nvvm_device_module);
}

#[test]
fn warn_policy_default_is_permissive() {
    let policy = WarnPolicy::default();
    assert!(!policy.promotes(Severity::Warning, Some("WARN001")));
    assert!(!policy.promotes(Severity::Error, Some("SEM001")));
    assert!(!policy.promotes(Severity::Info, Some("WARN001")));
}

#[test]
fn warn_policy_deny_all_promotes_warnings_only() {
    let policy = WarnPolicy { deny_all_warnings: true, deny_codes: vec![] };
    assert!(policy.promotes(Severity::Warning, Some("WARN001")));
    assert!(policy.promotes(Severity::Warning, None));
    assert!(!policy.promotes(Severity::Error, Some("SEM001")));
    assert!(!policy.promotes(Severity::Info, Some("WARN001")));
}

#[test]
fn warn_policy_deny_code_promotes_matching_warning_only() {
    let policy = WarnPolicy { deny_all_warnings: false, deny_codes: vec!["WARN006".into()] };
    assert!(policy.promotes(Severity::Warning, Some("WARN006")));
    assert!(!policy.promotes(Severity::Warning, Some("WARN001")));
    assert!(!policy.promotes(Severity::Warning, None));
    assert!(!policy.promotes(Severity::Error, Some("WARN006")));
}
