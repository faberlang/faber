use super::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP_PACKAGE: AtomicU64 = AtomicU64::new(0);

/// Minimal package so pack resolution can fall through to installed stdlib packs.
fn temp_package_entry() -> (PathBuf, PathBuf) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let serial = NEXT_TEMP_PACKAGE.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("faber-diag-locale-{nonce}-{serial}"));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).expect("temp package root");
    fs::write(
        root.join("faber.toml"),
        r#"[package]
name = "diag-locale-test"
version = "0.0.0"

[paths]
source = "src"
entry = "main.fab"

[build]
target = "rust"
kind = "bin"
"#,
    )
    .expect("manifest");
    let entry = root.join("src").join("main.fab");
    fs::write(&entry, "incipit {}\n").expect("entry");
    (root, entry)
}

#[test]
fn code_locale_defaults_to_english_pack_without_manifest_setting() {
    let (_root, entry) = temp_package_entry();
    let (config, diagnostic_pack) =
        config_with_locale(Target::HirRust, &entry, None, None).expect("default locale config");
    assert_eq!(
        config
            .locale_pack
            .as_ref()
            .map(|pack| pack.metadata.id.as_str()),
        Some("en")
    );
    assert_eq!(
        diagnostic_pack
            .as_ref()
            .map(|pack| pack.metadata.id.as_str()),
        Some("en")
    );
    let _ = fs::remove_dir_all(_root);
}

#[test]
fn direct_default_config_uses_english_pack() {
    let config =
        default_config_with_locale(Target::HirTypeScript).expect("default code locale config");
    assert_eq!(
        config
            .locale_pack
            .as_ref()
            .map(|pack| pack.metadata.id.as_str()),
        Some("en")
    );
}

#[test]
fn diagnostic_locale_defaults_to_code_pack() {
    let (_root, entry) = temp_package_entry();
    let (config, diagnostic_pack) =
        config_with_locale(Target::HirRust, &entry, Some("zh-Hans"), None).expect("locale config");
    let code_pack = config.locale_pack.expect("code pack");
    let diagnostic_pack = diagnostic_pack.expect("diagnostic pack");
    assert_eq!(code_pack.metadata.id, "zh-Hans");
    assert_eq!(diagnostic_pack.metadata.id, "zh-Hans");
    let _ = fs::remove_dir_all(_root);
}

#[test]
fn diagnostic_locale_can_differ_from_code_locale() {
    let (_root, entry) = temp_package_entry();
    let (config, diagnostic_pack) =
        config_with_locale(Target::HirRust, &entry, Some("zh-Hans"), Some("th-TH"))
            .expect("locale config");
    let code_pack = config.locale_pack.expect("code pack");
    let diagnostic_pack = diagnostic_pack.expect("diagnostic pack");
    assert_eq!(code_pack.metadata.id, "zh-Hans");
    assert_eq!(diagnostic_pack.metadata.id, "th-TH");
    let _ = fs::remove_dir_all(_root);
}

#[test]
fn empty_diagnostic_locale_is_rejected() {
    let (_root, entry) = temp_package_entry();
    let err = config_with_locale(Target::HirRust, &entry, Some("zh-Hans"), Some("   "))
        .expect_err("empty diagnostic locale");
    assert!(
        err.message
            .contains("--diagnostic-locale must not be empty"),
        "unexpected message: {}",
        err.message
    );
    let _ = fs::remove_dir_all(_root);
}
