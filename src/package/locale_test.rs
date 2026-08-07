use super::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP_PACKAGE: AtomicU64 = AtomicU64::new(0);

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

/// Unique scratch dir under the platform temp root (never inside a workspace).
fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let serial = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("{label}-{nonce}-{serial}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

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

#[test]
fn missing_installed_locale_pack_fails_closed_with_next_action() {
    // D6: package reads on a missing locale exit nonzero naming the missing
    // pack and one next action (installed-binary path, no CARGO_MANIFEST_DIR).
    let err = locale_pack_for_emit(&[], Some("zz")).expect_err("missing locale pack");
    assert!(
        err.contains("failed to load reader locale 'zz'"),
        "unexpected message: {err}"
    );
    assert!(err.contains("next action"), "missing next action: {err}");
    assert!(
        err.contains("share/faber/locale/zz/pack.toml"),
        "missing pack location: {err}"
    );
}

#[test]
fn dev_installed_locale_pack_resolves_english_from_sibling_radix() {
    // Development builds resolve the installed pack from the sibling radix
    // tree without any CARGO_MANIFEST_DIR bake.
    let pack = installed_locale_pack_path("en");
    assert!(pack.is_file(), "en pack must resolve in the dev workspace");
    let display = pack.display().to_string();
    assert!(
        display.contains("radix/stdlib/locale/en/pack.toml"),
        "unexpected dev resolution: {display}"
    );
}

#[test]
fn installed_locale_pack_resolves_install_prefix() {
    let prefix = temp_dir("faber-locale-prefix");
    let pack = prefix.join("share/faber/locale/la/pack.toml");
    fs::create_dir_all(pack.parent().expect("pack dir")).expect("pack dir");
    fs::write(&pack, "ignored content").expect("pack");
    let exe = prefix.join("bin/faber");
    fs::create_dir_all(exe.parent().expect("bin dir")).expect("bin dir");

    let resolved = installed_locale_pack_path_in(Some(&exe), Some(&prefix), "la");
    assert_eq!(
        resolved,
        pack.canonicalize().unwrap_or(pack.clone()),
        "resolved must match the canonicalized pack path"
    );
    let _ = fs::remove_dir_all(&prefix);
}

#[test]
fn installed_locale_pack_dev_fallback_resolves_sibling_radix() {
    let work = temp_dir("faber-locale-dev");
    let faber_dir = work.join("faber");
    fs::create_dir_all(&faber_dir).expect("faber dir");
    let pack = work.join("radix/stdlib/locale/en/pack.toml");
    fs::create_dir_all(pack.parent().expect("pack dir")).expect("pack dir");
    fs::write(&pack, "ignored content").expect("pack");

    let resolved = installed_locale_pack_path_in(None, Some(&faber_dir), "en");
    assert_eq!(
        resolved,
        pack.canonicalize().unwrap_or(pack.clone()),
        "resolved must match the canonicalized pack path"
    );
    let _ = fs::remove_dir_all(&work);
}

#[test]
fn installed_locale_pack_fails_closed_with_nonexistent_path() {
    let hermetic = temp_dir("faber-locale-missing");
    let exe = hermetic.join("bin/faber");
    fs::create_dir_all(exe.parent().expect("bin dir")).expect("bin dir");

    // Installed binary with no pack and no sibling checkout: the resolved path
    // must not exist and must name the install layout (fail closed, E5).
    let resolved = installed_locale_pack_path_in(Some(&exe), Some(&hermetic), "zz");
    assert!(!resolved.is_file(), "resolved path must not exist");
    let display = resolved.display().to_string();
    assert!(
        display.contains("share/faber/locale/zz/pack.toml"),
        "missing pack must name the installed location: {display}"
    );
    let _ = fs::remove_dir_all(&hermetic);
}

#[test]
fn installed_locale_pack_resolves_both_layout_suffixes() {
    // The install prefix may be `share/faber/locale` or `lib/faber/locale`.
    let prefix = temp_dir("faber-locale-lib");
    let pack = prefix.join("lib/faber/locale/th-TH/pack.toml");
    fs::create_dir_all(pack.parent().expect("pack dir")).expect("pack dir");
    fs::write(&pack, "ignored content").expect("pack");
    let exe = prefix.join("bin/faber");
    fs::create_dir_all(exe.parent().expect("bin dir")).expect("bin dir");

    let resolved = installed_locale_pack_path_in(Some(&exe), Some(&prefix), "th-TH");
    assert_eq!(
        resolved,
        pack.canonicalize().unwrap_or(pack.clone()),
        "resolved must match the canonicalized pack path"
    );
    let _ = fs::remove_dir_all(&prefix);
}

#[test]
fn dev_walkup_is_never_an_installed_binary_fallback() {
    // A release-shaped binary beside a stray sibling radix checkout must not
    // resolve the locale pack (E5 false-green eliminated for installed
    // binaries); development builds still may.
    let work = temp_dir("faber-locale-stray");
    let faber_dir = work.join("faber");
    fs::create_dir_all(&faber_dir).expect("faber dir");
    let pack = work.join("radix/stdlib/locale/en/pack.toml");
    fs::create_dir_all(pack.parent().expect("pack dir")).expect("pack dir");
    fs::write(&pack, "ignored content").expect("pack");
    let exe = work.join("faber/bin/faber");

    let installed = installed_locale_pack_path_in(Some(&exe), Some(&faber_dir), "en");
    assert!(!installed.is_file(), "installed binary must not resolve a stray checkout");

    let dev = installed_locale_pack_path_in(None, Some(&faber_dir), "en");
    assert_eq!(
        dev,
        pack.canonicalize().unwrap_or(pack.clone()),
        "development builds still resolve the sibling radix pack"
    );
    let _ = fs::remove_dir_all(&work);
}

