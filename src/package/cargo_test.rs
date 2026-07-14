use super::{render_generated_cargo_toml, RustRuntimePlan};
use crate::core_support::materialize::materialize;
use crate::package::ManifestRustHost;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn temp_root(label: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "faber-cargo-test-{label}-{}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create temp root");
    root
}

#[test]
fn minimal_generated_cargo_manifest_links_only_materialized_runtime() -> Result<(), Box<dyn Error>>
{
    let support = materialize()?;
    let package_root = PathBuf::from("/tmp/faber-node-c-minimal");
    let rendered = render_generated_cargo_toml(
        "demo",
        "0.1.0",
        &RustRuntimePlan {
            needs_faber: true,
            ..RustRuntimePlan::default()
        },
        &package_root,
    );

    let manifest = toml::from_str::<toml::Value>(&rendered)?;
    let dependencies = manifest["dependencies"]
        .as_table()
        .ok_or("missing dependencies")?;
    assert_eq!(dependencies.len(), 1);
    assert!(dependencies.contains_key("faber"));
    assert_eq!(
        dependencies["faber"]["path"].as_str(),
        Some(support.faber_runtime()?.to_string_lossy().as_ref())
    );
    assert!(!rendered.contains(package_root.to_string_lossy().as_ref()));
    assert!(!rendered.contains("host-providers-rs"));
    Ok(())
}

#[test]
fn native_manifest_links_only_selected_explicit_provider_from_materialized_root(
) -> Result<(), Box<dyn Error>> {
    let support = materialize()?;
    let package_root = PathBuf::from("/tmp/faber-node-c-native");
    let mut plan = RustRuntimePlan {
        needs_faber: true,
        host: Some(ManifestRustHost::Native),
        ..RustRuntimePlan::default()
    };
    plan.selected_providers.insert("solum".to_owned());

    let rendered = render_generated_cargo_toml("demo", "0.1.0", &plan, &package_root);
    let dependencies = toml::from_str::<toml::Value>(&rendered)?["dependencies"]
        .as_table()
        .ok_or("missing dependencies")?
        .clone();
    for (name, path) in [
        ("faber", support.faber_runtime()?),
        ("host_kernel", support.host_kernel()?),
        ("host_native", support.host_native()?),
        ("solum", support.provider("solum")?),
    ] {
        assert_eq!(
            dependencies[name]["path"].as_str(),
            Some(path.to_string_lossy().as_ref()),
            "missing materialized path for {name}"
        );
    }
    for forbidden in ["aleator", "consolum", "processus", "tempus", "sqlite"] {
        assert!(
            !dependencies.contains_key(forbidden),
            "unexpected provider {forbidden}"
        );
    }
    assert!(!rendered.contains(package_root.to_string_lossy().as_ref()));
    Ok(())
}

#[test]
fn unknown_provider_is_a_structured_materialized_root_failure() {
    let plan = RustRuntimePlan {
        needs_faber: true,
        host: Some(ManifestRustHost::Native),
        selected_providers: ["sqlite".to_owned()].into_iter().collect(),
        ..RustRuntimePlan::default()
    };
    let support = materialize().expect("embedded core support materializes");
    let error = super::render_generated_cargo_toml_with_support("demo", "0.1.0", &plan, &support)
        .expect_err("unknown provider must fail closed");
    assert_eq!(error.code, Some(crate::PACKAGE_DIAGNOSTIC_CODE));
    assert_eq!(
        error
            .args
            .iter()
            .find(|arg| arg.name == "issue")
            .map(|arg| arg.value.as_str()),
        Some("core_support_materialization_failed")
    );
}

#[test]
fn generated_cargo_manifest_escapes_metadata_paths_and_dependency_keys(
) -> Result<(), Box<dyn Error>> {
    let version = "0.1.0\"\n# injected";
    let library_path = PathBuf::from("/tmp/library-\"-\\-path");
    let mut plan = RustRuntimePlan::default();
    plan.library_path_deps
        .push(("library\"key".to_owned(), library_path.clone()));

    let rendered = render_generated_cargo_toml(
        "demo",
        version,
        &plan,
        PathBuf::from("/tmp/faber-node-c-escape").as_path(),
    );
    let manifest = toml::from_str::<toml::Value>(&rendered)?;
    assert_eq!(manifest["package"]["version"].as_str(), Some(version));
    assert_eq!(
        manifest["dependencies"]["library\"key"]["path"].as_str(),
        Some(library_path.to_string_lossy().as_ref())
    );
    Ok(())
}

#[test]
fn generated_cargo_manifest_reuses_runtime_path_from_linked_library_deps(
) -> Result<(), Box<dyn Error>> {
    let root = temp_root("linked-runtime");
    let library = root.join("target/faber/deps/http");
    let runtime = root.join("faber-runtime");
    fs::create_dir_all(&library)?;
    fs::create_dir_all(&runtime)?;
    fs::write(
        library.join("Cargo.toml"),
        format!(
            r#"[package]
name = "http"
version = "0.1.0"
edition = "2021"

[dependencies]
faber = {{ package = "faber-runtime", path = "{}" }}
"#,
            runtime.display()
        ),
    )?;
    let mut plan = RustRuntimePlan {
        needs_faber: true,
        ..RustRuntimePlan::default()
    };
    plan.library_path_deps
        .push(("http".to_owned(), library.clone()));

    let rendered = render_generated_cargo_toml("demo", "0.1.0", &plan, &root);
    let manifest = toml::from_str::<toml::Value>(&rendered)?;

    assert_eq!(
        manifest["dependencies"]["faber"]["path"].as_str(),
        Some(runtime.to_string_lossy().as_ref())
    );
    Ok(())
}
