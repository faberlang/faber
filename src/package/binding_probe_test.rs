use super::*;
use crate::package::paths::paths_equivalent;
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

// ── truncate_output ───────────────────────────────────────────────────────

#[test]
fn truncate_output_passes_through_short_string() {
    let short = "hello probe";
    assert_eq!(truncate_output(short), short);
}

#[test]
fn truncate_output_truncates_at_8000_chars() {
    let long = "a".repeat(10_000);
    let truncated = truncate_output(&long);
    assert_eq!(truncated.len(), 8_000);
    assert!(truncated.chars().all(|c| c == 'a'));
}

#[test]
fn truncate_output_handles_unicode_character_boundary() {
    // 4-byte unicode characters at the boundary
    let long: String = std::iter::repeat("🔥").take(5_000).collect();
    let truncated = truncate_output(&long);
    // Should not panic and should be <= 8000 chars
    assert!(truncated.chars().count() <= 8_000);
}

// ── probe_root ────────────────────────────────────────────────────────────

#[test]
fn probe_root_returns_unique_path_per_call() {
    let root1 = probe_root();
    let root2 = probe_root();
    assert_ne!(
        root1, root2,
        "each probe_root call must produce a unique path"
    );
}

#[test]
fn probe_root_lives_in_temp_dir() {
    let root = probe_root();
    assert!(
        root.starts_with(std::env::temp_dir()),
        "probe_root must be under temp dir: {:?}",
        root
    );
}

#[test]
fn probe_root_contains_faber_and_binding_in_name() {
    let name = probe_root().to_string_lossy().to_string();
    assert!(
        name.contains("faber-binding-probe"),
        "probe_root name must contain 'faber-binding-probe': {name}"
    );
}

#[test]
fn probe_root_includes_process_id() {
    let root = probe_root().to_string_lossy().to_string();
    assert!(
        root.contains(&std::process::id().to_string()),
        "probe_root should include pid: {root}"
    );
}

// ── probe_key ─────────────────────────────────────────────────────────────

#[test]
fn probe_key_produces_deterministic_output() {
    let package_root = Path::new("/repo/pkg");
    let anchor = Path::new("/repo/pkg/Cargo.toml");
    let deps = BTreeMap::from([("serde".to_owned(), "1".to_owned())]);
    let probes = vec!["fn test() {}".to_owned()];

    let key1 = probe_key(package_root, anchor, &deps, None, &probes);
    let key2 = probe_key(package_root, anchor, &deps, None, &probes);
    assert_eq!(key1, key2, "probe_key must be deterministic");
}

#[test]
fn probe_key_changes_with_different_dependencies() {
    let package_root = Path::new("/repo/pkg");
    let anchor = Path::new("/repo/pkg/Cargo.toml");

    let deps_a = BTreeMap::from([("serde".to_owned(), "1".to_owned())]);
    let deps_b = BTreeMap::from([("tokio".to_owned(), "1".to_owned())]);
    let probes = vec!["fn test() {}".to_owned()];

    let key_a = probe_key(package_root, anchor, &deps_a, None, &probes);
    let key_b = probe_key(package_root, anchor, &deps_b, None, &probes);
    assert_ne!(key_a, key_b, "different deps should produce different keys");
}

#[test]
fn probe_key_changes_with_different_probes() {
    let package_root = Path::new("/repo/pkg");
    let anchor = Path::new("/repo/pkg/Cargo.toml");
    let deps = BTreeMap::new();

    let key_a = probe_key(package_root, anchor, &deps, None, &["fn a() {}".to_owned()]);
    let key_b = probe_key(package_root, anchor, &deps, None, &["fn b() {}".to_owned()]);
    assert_ne!(
        key_a, key_b,
        "different probes should produce different keys"
    );
}

#[test]
fn probe_key_includes_shim_path_when_present() {
    let package_root = Path::new("/repo/pkg");
    let anchor = Path::new("/repo/pkg/Cargo.toml");
    let deps = BTreeMap::new();
    let probes = vec!["fn test() {}".to_owned()];

    let key_without = probe_key(package_root, anchor, &deps, None, &probes);
    let key_with = probe_key(
        package_root,
        anchor,
        &deps,
        Some(Path::new("/shim.rs")),
        &probes,
    );
    assert_ne!(
        key_without, key_with,
        "shim presence should change probe key"
    );
}

// ── canonical_probe_path ──────────────────────────────────────────────────

#[test]
fn canonical_probe_path_returns_path_as_is_when_not_found() {
    let path = Path::new("/tmp/does-not-exist-42-for-sure");
    let result = canonical_probe_path(path);
    assert_eq!(result, path.display().to_string());
}

// ── probe_source ──────────────────────────────────────────────────────────

#[test]
fn probe_source_without_shim_includes_probes_and_main() {
    let source = probe_source(None, &["fn test() {}".to_owned()]);
    assert!(source.contains("fn test() {}"));
    assert!(source.contains("fn main() {}"));
}

#[test]
fn probe_source_with_shim_includes_path_attribute() {
    let dir = temp_root("probe-source-shim");
    let shim_path = dir.join("shim.rs");
    fs::write(&shim_path, "fn shared() {}").expect("write shim");
    let source = probe_source(Some(&shim_path), &["fn test() {}".to_owned()]);
    assert!(source.contains("#[path ="));
    assert!(source.contains("mod shim;"));
    assert!(source.contains("fn test() {}"));
    assert!(source.contains("fn main() {}"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn probe_source_handles_multiple_probes() {
    let source = probe_source(
        None,
        &["fn test_a() {}".to_owned(), "fn test_b() {}".to_owned()],
    );
    assert!(source.contains("fn test_a() {}"));
    assert!(source.contains("fn test_b() {}"));
    assert!(source.contains("fn main() {}"));
}

#[test]
fn probe_source_handles_empty_probes() {
    let source = probe_source(None, &[]);
    assert!(source.contains("fn main() {}"));
}

// ── probe_manifest tests (existing) ────────────────────────────────────────

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

    let runtime_path = parsed["dependencies"]["faber"]["path"]
        .as_str()
        .expect("runtime path");
    assert!(paths_equivalent(Path::new(runtime_path), &runtime));
}
