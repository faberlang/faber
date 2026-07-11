use super::probe_manifest;
use std::collections::BTreeMap;
use std::path::Path;

#[test]
fn probe_manifest_preserves_inline_dependency_tables() {
    let dependencies = BTreeMap::from([(
        "rusqlite".to_owned(),
        r#"{ version = "0.32", features = ["bundled"] }"#.to_owned(),
    )]);

    let manifest = probe_manifest(Path::new("/tmp/package"), &dependencies).expect("probe manifest");
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
