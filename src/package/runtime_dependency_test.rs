use super::{
    dependency_path, parse_dependency_requirement, runtime_path_from_cargo_manifest,
    runtime_path_from_target_dependencies,
};
use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn temp_root(label: &str) -> std::path::PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "faber-runtime-dependency-{label}-{}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create temp root");
    root
}

#[test]
fn detects_runtime_path_from_target_path_dependency_manifest() {
    let root = temp_root("path-dep");
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
    let parsed = parse_dependency_requirement(dependencies["faber_http_transport"].as_str());
    let table = parsed.as_table().expect("inline dependency table");
    assert_eq!(dependency_path(&package, table), Some(transport.clone()));
    assert_eq!(
        runtime_path_from_cargo_manifest(&transport),
        Some(runtime.clone())
    );

    assert_eq!(
        runtime_path_from_target_dependencies(&package, &dependencies),
        Some(runtime)
    );
}

#[test]
fn detects_runtime_path_from_direct_target_dependency() {
    let root = temp_root("direct-dep");
    let package = root.join("packages/app");
    let runtime = root.join("faber-runtime");
    fs::create_dir_all(&package).expect("create package");
    fs::create_dir_all(&runtime).expect("create runtime");
    let dependencies = BTreeMap::from([(
        "faber".to_owned(),
        r#"{ package = "faber-runtime", path = "../../faber-runtime" }"#.to_owned(),
    )]);

    assert_eq!(
        runtime_path_from_target_dependencies(&package, &dependencies),
        Some(runtime)
    );
}
