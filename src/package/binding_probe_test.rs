use super::probe_manifest;
use std::collections::BTreeMap;
use std::path::Path;

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
fn probe_manifest_prefers_packet_local_runtime_when_packet_marks_exist() {
    let root = std::env::temp_dir().join(format!(
        "faber-probe-runtime-pin-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let packet = root.join("worktrees").join("slice");
    let package_root = packet.join("faber-build").join("packages").join("http");
    std::fs::create_dir_all(&package_root).expect("package root");
    std::fs::write(packet.join("PACKET.md"), "# packet\n").expect("packet marker");
    std::fs::create_dir_all(packet.join("faber-runtime")).expect("packet runtime");
    std::fs::create_dir_all(root.join("canonical").join("faber-runtime")).expect("main runtime");

    let manifest = probe_manifest(&package_root, &BTreeMap::new()).expect("probe manifest");
    let parsed = toml::from_str::<toml::Value>(&manifest).expect("valid TOML");
    let runtime_path = parsed["dependencies"]["faber"]["path"]
        .as_str()
        .expect("runtime path");
    let packet_runtime = std::fs::canonicalize(packet.join("faber-runtime"))
        .expect("canonical packet runtime")
        .display()
        .to_string();
    assert_eq!(runtime_path, packet_runtime);
    assert!(
        !runtime_path.contains("/canonical/"),
        "probe runtime path should not fall back to canonical main: {runtime_path}"
    );
}
