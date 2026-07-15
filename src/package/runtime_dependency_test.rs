use super::{
    dependency_path, parse_dependency_requirement, runtime_path_for_target_dependencies,
    runtime_path_from_cargo_manifest, runtime_path_from_target_dependencies,
};
use crate::package::paths::paths_equivalent;
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
    assert!(dependency_path(&package, table)
        .as_deref()
        .is_some_and(|path| paths_equivalent(path, &transport)));
    assert!(runtime_path_from_cargo_manifest(&transport)
        .as_deref()
        .is_some_and(|path| paths_equivalent(path, &runtime)));

    assert!(
        runtime_path_from_target_dependencies(&package, &dependencies)
            .as_deref()
            .is_some_and(|path| paths_equivalent(path, &runtime))
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

    assert!(
        runtime_path_from_target_dependencies(&package, &dependencies)
            .as_deref()
            .is_some_and(|path| paths_equivalent(path, &runtime))
    );
}

#[test]
fn missing_direct_runtime_dependency_falls_back_to_materialized_runtime() {
    let root = temp_root("missing-direct-dep");
    let package = root.join("packages/app");
    fs::create_dir_all(&package).expect("create package");
    let dependencies = BTreeMap::from([(
        "faber".to_owned(),
        r#"{ package = "faber-runtime", path = "missing-runtime" }"#.to_owned(),
    )]);
    let expected = crate::core_support::materialize::materialize()
        .expect("embedded core support materializes")
        .faber_runtime()
        .expect("materialized runtime path");

    let actual = runtime_path_for_target_dependencies(&package, &dependencies)
        .expect("missing direct path should fall back to materialized runtime");
    assert!(
        paths_equivalent(&actual, &expected),
        "expected materialized runtime {}, got {}",
        expected.display(),
        actual.display()
    );
}
