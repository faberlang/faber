use super::probe_manifest;
use std::collections::BTreeMap;

#[test]
fn probe_manifest_preserves_inline_dependency_tables() {
    let dependencies = BTreeMap::from([(
        "rusqlite".to_owned(),
        r#"{ version = "0.32", features = ["bundled"] }"#.to_owned(),
    )]);

    let manifest = probe_manifest(&dependencies).expect("probe manifest");
    let parsed = toml::from_str::<toml::Value>(&manifest).expect("valid TOML");
    let rusqlite = &parsed["dependencies"]["rusqlite"];
    assert_eq!(rusqlite["version"].as_str(), Some("0.32"));
    assert_eq!(rusqlite["features"][0].as_str(), Some("bundled"));
    assert_eq!(
        parsed["dependencies"]["faber"]["package"].as_str(),
        Some("faber-runtime")
    );
}
