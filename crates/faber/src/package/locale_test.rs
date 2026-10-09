use super::*;
use radix::module::diagnostics::DiagnosticArg;
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

/// Minimal package so pack resolution can fall through to embedded packs.
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
fn diagnostics_locale_defaults_to_code_pack() {
    let (_root, entry) = temp_package_entry();
    let (config, diagnostic_pack) =
        config_with_locale(Target::HirRust, &entry, Some("zh-Hans"), None).expect("locale config");
    let code_pack = config.locale_pack.expect("code pack");
    let diagnostic_pack = diagnostic_pack.expect("diagnostic pack");
    assert_eq!(code_pack.metadata.id, "zh-Hans");
    assert!(config.locale_pack_explicit);
    assert_eq!(diagnostic_pack.metadata.id, "zh-Hans");
    let _ = fs::remove_dir_all(_root);
}

#[test]
fn diagnostics_locale_can_differ_from_code_locale() {
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
fn empty_diagnostics_locale_is_rejected() {
    let (_root, entry) = temp_package_entry();
    let err = config_with_locale(Target::HirRust, &entry, Some("zh-Hans"), Some("   "))
        .expect_err("empty diagnostics locale");
    assert!(
        err.args
            .contains(&DiagnosticArg::new("flag", "--diagnostics-locale")),
        "unexpected diagnostic: {err:?}"
    );
    let _ = fs::remove_dir_all(_root);
}

#[test]
fn missing_embedded_locale_pack_fails_closed() {
    let err = locale_pack_for_emit(&[], Some("zz")).expect_err("unknown locale");
    assert!(
        err.contains("failed to load reader locale 'zz'"),
        "unexpected message: {err}"
    );
    assert!(
        err.contains("no embedded reader pack"),
        "missing embedded source: {err}"
    );
    assert!(err.contains("next action"), "missing next action: {err}");
    assert!(
        !err.contains("reinstall"),
        "reinstall-the-dev-kit path is deleted: {err}"
    );
    assert!(
        !err.contains("share/faber"),
        "installed pack path is deleted: {err}"
    );
}

#[test]
fn missing_emit_package_path_is_a_structured_diagnostic() {
    let root = temp_dir("faber-emit-missing-package");
    let missing = root.join("missing-package");
    let input = vec![missing.display().to_string()];

    let diagnostic = locale_pack_for_emit_with_diagnostic(&input, Some("th-TH"))
        .expect_err("missing package path");
    assert_eq!(diagnostic.code, Some("PKG001"));
    assert_eq!(diagnostic.issue(), Some("package_path_missing"));
    assert_eq!(diagnostic.file, missing.display().to_string());
    assert!(
        diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "path" && arg.value == missing.display().to_string())
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn available_locales_are_the_static_eight() {
    let expected: Vec<String> = ["ar", "en", "hi", "la", "th-TH", "vi", "zh-Hans", "zh-Hant"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(available_locales(), expected);
}

#[test]
fn stdin_emit_loads_embedded_english_pack() {
    let pack = locale_pack_for_emit(&[], Some("en")).expect("embedded en");
    assert_eq!(
        pack.as_ref().map(|pack| pack.metadata.id.as_str()),
        Some("en")
    );
}

#[test]
fn package_local_override_is_selected_before_embedded() {
    let (root, entry) = temp_package_entry();
    let pack_path = root.join("locale").join("en.toml");
    fs::create_dir_all(pack_path.parent().expect("locale dir")).expect("locale dir");
    fs::write(&pack_path, "not a pack").expect("override");

    let err = load_locale_pack_for_input(&entry, Some("en")).expect_err("override must load");
    let display = pack_path.display().to_string();
    assert!(
        err.args.contains(&DiagnosticArg::new("pack", display)),
        "package-local path must be named, got: {err:?}"
    );
    assert!(
        !err.args.iter().any(|arg| arg.name == "embedded_locales"),
        "must not fall through to embedded: {err:?}"
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn manifest_pack_path_is_selected_before_embedded() {
    let (root, entry) = temp_package_entry();
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

[locale]
locale = "en"
pack = "./locale/custom.toml"
"#,
    )
    .expect("manifest with pack path");

    let err = load_locale_pack_for_input(&entry, None).expect_err("manifest pack must load");
    assert!(
        err.args.iter().any(|arg| arg.name == "pack"
            && Path::new(&arg.value)
                .file_name()
                .is_some_and(|name| name == "custom.toml")),
        "manifest pack path must be named, got: {err:?}"
    );
    assert!(
        !err.args.iter().any(|arg| arg.name == "embedded_locales"),
        "must not fall through to embedded: {err:?}"
    );

    let _ = fs::remove_dir_all(root);
}
