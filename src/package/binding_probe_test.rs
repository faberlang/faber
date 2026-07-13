use super::probe_manifest;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn temp_root(label: &str) -> std::path::PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "faber-binding-probe-{label}-{}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create temp root");
    root
}

#[test]
fn probe_manifest_preserves_inline_dependency_tables() {
    let dependencies = BTreeMap::from([(
        "rusqlite".to_owned(),
        r#"{ version = "0.32", features = ["bundled"] }"#.to_owned(),
    )]);

    let manifest =
        probe_manifest(Path::new("/tmp/package"), &dependencies).expect("probe manifest");
    let parsed = toml::from_str::<toml::Value>(&manifest).expect("valid TOML");
    let rusqlite = &parsed["dependencies"]["rusqlite"];
    assert_eq!(rusqlite["version"].as_str(), Some("0.32"));
    assert_eq!(rusqlite["features"][0].as_str(), Some("bundled"));
    assert_eq!(
        parsed["dependencies"]["faber"]["package"].as_str(),
        Some("faber-runtime")
    );
}

#[test]
fn probe_manifest_rebases_dependency_path_tables_to_package_root() {
    let dependencies = BTreeMap::from([(
        "faber_http_transport".to_owned(),
        r#"{ package = "faber-http-transport", path = "../../crates/http-transport" }"#.to_owned(),
    )]);

    let manifest =
        probe_manifest(Path::new("/repo/packages/http"), &dependencies).expect("probe manifest");
    let parsed = toml::from_str::<toml::Value>(&manifest).expect("valid TOML");
    assert_eq!(
        parsed["dependencies"]["faber_http_transport"]["path"].as_str(),
        Some("/repo/packages/http/../../crates/http-transport")
    );
}

#[test]
fn probe_manifest_keeps_plain_versions_as_strings() {
    let dependencies = BTreeMap::from([("bytes".to_owned(), "1".to_owned())]);

    let manifest =
        probe_manifest(Path::new("/repo/packages/http"), &dependencies).expect("probe manifest");
    let parsed = toml::from_str::<toml::Value>(&manifest).expect("valid TOML");
    assert_eq!(parsed["dependencies"]["bytes"].as_str(), Some("1"));
}

#[test]
fn probe_manifest_uses_verified_materialized_runtime() {
    let package_root = Path::new("/tmp/faber-binding-probe");
    let manifest = probe_manifest(package_root, &BTreeMap::new()).expect("probe manifest");
    let parsed = toml::from_str::<toml::Value>(&manifest).expect("valid TOML");
    let runtime_path = parsed["dependencies"]["faber"]["path"]
        .as_str()
        .expect("runtime path");
    let support = crate::core_support::materialize::materialize().expect("materialized support");
    assert_eq!(
        runtime_path,
        support.faber_runtime().unwrap().to_string_lossy()
    );
    assert!(!runtime_path.contains("worktrees"));
}

#[test]
fn probe_manifest_reuses_runtime_path_from_target_path_dependencies() {
    let root = temp_root("runtime-path-dep");
    let package = root.join("packages/http");
    let transport = root.join("crates/http-transport");
    let runtime = root.join("faber-runtime");
    fs::create_dir_all(&package).expect("create package");
    fs::create_dir_all(&transport).expect("create transport");
    fs::create_dir_all(&runtime).expect("create runtime");
    fs::write(
        transport.join("Cargo.toml"),
        r#"[package]
name = "faber-http-transport"
version = "0.1.0"
edition = "2021"

[dependencies]
faber = { package = "faber-runtime", path = "../../faber-runtime" }
"#,
    )
    .expect("write transport manifest");
    let dependencies = BTreeMap::from([(
        "faber_http_transport".to_owned(),
        r#"{ package = "faber-http-transport", path = "../../crates/http-transport" }"#.to_owned(),
    )]);

    let manifest = probe_manifest(&package, &dependencies).expect("probe manifest");
    let parsed = toml::from_str::<toml::Value>(&manifest).expect("valid TOML");

    assert_eq!(
        parsed["dependencies"]["faber"]["path"].as_str(),
        Some(runtime.to_string_lossy().as_ref())
    );
}
