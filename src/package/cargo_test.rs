use super::{render_generated_cargo_toml, RustRuntimePlan};
use crate::core_support::materialize::materialize;
use crate::package::paths::paths_equivalent;
use crate::package::ManifestRustHost;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    emit_generated_crate_with_runtime_plan, inject_crate_snapshot_failure_at,
    lock_generated_crate_build, BuildLayout,
};

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
    assert!(!rendered.contains("hosts/crates/solum"));
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

    let runtime_path = manifest["dependencies"]["faber"]["path"]
        .as_str()
        .expect("runtime path");
    assert!(paths_equivalent(Path::new(runtime_path), &runtime));
    Ok(())
}

// ---------------------------------------------------------------------------
// FBR-P2-004: atomic generated-crate publication (Stage 3)
// ---------------------------------------------------------------------------

fn assert_no_staging_temps(layout: &BuildLayout) {
    let target_dir = &layout.cargo_target_dir;
    for entry in fs::read_dir(target_dir).expect("read target dir").flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !(name.starts_with(".crate.tmp-") || name.starts_with(".old.tmp-")),
            "leftover staging directory `{name}` under {}",
            target_dir.display()
        );
    }
}

#[test]
fn interleaved_emit_sequences_never_publish_a_mixed_crate() {
    let root = temp_root("interleaved-emit");
    let pkg = root.join("pkg");
    let layout = BuildLayout::from_package_root(&pkg, "mix");
    let plans: Vec<(RustRuntimePlan, String)> = (0..6)
        .map(|i| {
            let mut plan = RustRuntimePlan {
                needs_faber: true,
                ..RustRuntimePlan::default()
            };
            plan.library_path_deps
                .push((format!("lib-{i}"), root.join(format!("lib-{i}"))));
            (
                plan,
                format!("// marker-plan-{i}\nfn main() {{ println!(\"plan-{i}\"); }}\n"),
            )
        })
        .collect();

    // 12 interleaved emit sequences for the same package, each holding the
    // per-package lock. Without serialization, files from different plans
    // would interleave inside `target/faber/`.
    std::thread::scope(|scope| {
        for round in 0..12 {
            let layout = &layout;
            let plans = &plans;
            scope.spawn(move || {
                let (plan, code) = &plans[round % plans.len()];
                let _lock = lock_generated_crate_build(layout).expect("lock");
                emit_generated_crate_with_runtime_plan(layout, code, None, plan).expect("emit");
            });
        }
    });

    // Whatever plan won, the published crate is one complete snapshot: the
    // manifest's dependency marker and the source's marker always agree, the
    // completion marker is present, and no staging directory is left behind.
    let cargo = fs::read_to_string(&layout.generated_cargo_manifest).expect("cargo");
    let main = fs::read_to_string(&layout.generated_rust_entry).expect("main");
    let cargo_plan = (0..6usize).find(|i| cargo.contains(&format!("lib-{i} = {{ path")));
    let source_plan = (0..6usize).find(|i| main.contains(&format!("marker-plan-{i}")));
    assert!(
        cargo_plan.is_some() && cargo_plan == source_plan,
        "mixed snapshot published:\ncargo:\n{cargo}\nmain:\n{main}"
    );
    assert!(layout
        .generated_crate_root
        .join(".faber-crate-complete")
        .is_file());
    assert_no_staging_temps(&layout);
}

#[test]
fn failed_generation_preserves_last_known_good_crate() {
    let root = temp_root("failed-emit");
    let pkg = root.join("pkg");
    let layout = BuildLayout::from_package_root(&pkg, "last-good");
    let plan_a = RustRuntimePlan {
        needs_faber: true,
        ..RustRuntimePlan::default()
    };
    let mut plan_b = RustRuntimePlan {
        needs_faber: true,
        ..RustRuntimePlan::default()
    };
    plan_b
        .library_path_deps
        .push(("lib-b".to_owned(), root.join("lib-b")));

    emit_generated_crate_with_runtime_plan(
        &layout,
        "// marker-good\nfn main() {}\n",
        None,
        &plan_a,
    )
    .expect("first emit");
    assert!(layout
        .generated_crate_root
        .join(".faber-crate-complete")
        .is_file());

    // Inject a failure after each major snapshot stage; the published crate
    // must remain exactly the last-known-good one with no partial new files.
    for stage in 1..=4 {
        inject_crate_snapshot_failure_at(stage);
        let err = emit_generated_crate_with_runtime_plan(
            &layout,
            "// marker-bad\nfn main() {}\n",
            None,
            &plan_b,
        )
        .expect_err("injected failure must abort generation");
        assert!(
            err.message.contains("injected"),
            "stage {stage}: unexpected error: {}",
            err.message
        );
        let main = fs::read_to_string(&layout.generated_rust_entry).expect("main");
        assert!(
            main.contains("marker-good"),
            "last good source lost at stage {stage}"
        );
        assert!(
            !main.contains("marker-bad"),
            "partial new source leaked at stage {stage}"
        );
        let cargo = fs::read_to_string(&layout.generated_cargo_manifest).expect("cargo");
        assert!(
            !cargo.contains("lib-b"),
            "partial new manifest leaked at stage {stage}"
        );
        assert!(layout
            .generated_crate_root
            .join(".faber-crate-complete")
            .is_file());
        assert_no_staging_temps(&layout);
    }

    // A clean generation after the injected failures publishes the new plan.
    inject_crate_snapshot_failure_at(0);
    emit_generated_crate_with_runtime_plan(&layout, "// marker-bad\nfn main() {}\n", None, &plan_b)
        .expect("emit succeeds after reset");
    let main = fs::read_to_string(&layout.generated_rust_entry).expect("main");
    assert!(main.contains("marker-bad"));
    assert!(!main.contains("marker-good"));
    let cargo = fs::read_to_string(&layout.generated_cargo_manifest).expect("cargo");
    assert!(cargo.contains("lib-b"));
    assert!(layout
        .generated_crate_root
        .join(".faber-crate-complete")
        .is_file());
    assert_no_staging_temps(&layout);
}

#[test]
fn republished_crate_preserves_library_dependency_tree() {
    let root = temp_root("deps-preserved");
    let pkg = root.join("pkg");
    let layout = BuildLayout::from_package_root(&pkg, "deps-pkg");
    let plan = RustRuntimePlan {
        needs_faber: true,
        ..RustRuntimePlan::default()
    };
    // Simulate the library crates that `emit_linked_library_crates` writes
    // into `target/faber/deps/` before the application snapshot is published.
    let lib_root = layout.generated_crate_root.join("deps").join("native-lib");
    fs::create_dir_all(lib_root.join("src")).expect("lib src");
    fs::write(
        lib_root.join("Cargo.toml"),
        "[package]\nname = \"native-lib\"\nversion = \"0.1.0\"\n",
    )
    .expect("lib manifest");
    fs::write(lib_root.join("src/lib.rs"), "// library body\n").expect("lib body");

    emit_generated_crate_with_runtime_plan(&layout, "fn main() {}", None, &plan)
        .expect("first emit");
    assert!(
        lib_root.join("Cargo.toml").is_file(),
        "deps lost on first publish"
    );

    emit_generated_crate_with_runtime_plan(&layout, "fn main() {}", None, &plan)
        .expect("second emit");
    assert!(
        lib_root.join("src/lib.rs").is_file(),
        "deps lost on republish"
    );
    assert_no_staging_temps(&layout);
}

// ---------------------------------------------------------------------------
// FBR-P2-004 Stage 3 residuals (audit a02d2e78): quarantine portability (R1)
// and Go subtree preservation across the crate swap (R2)
// ---------------------------------------------------------------------------

#[test]
fn quarantine_path_is_never_pre_created() {
    let parent = temp_root("quarantine");
    let quarantine = super::unique_quarantine_sibling(&parent).expect("quarantine path");
    assert!(
        fs::symlink_metadata(&quarantine).is_err(),
        "quarantine path `{}` must not be pre-created: a pre-created path \
         makes the old-crate rename onto it fail on Windows",
        quarantine.display()
    );
}

#[test]
fn republished_crate_preserves_go_module_tree() {
    let root = temp_root("go-preserved");
    let pkg = root.join("pkg");
    let layout = BuildLayout::from_package_root(&pkg, "go-pkg");
    let plan = RustRuntimePlan {
        needs_faber: true,
        ..RustRuntimePlan::default()
    };
    // Simulate Go module output that `go_build.rs` emits into
    // `target/faber/go/` before a later Rust publish (GO3/GO4 layout).
    let go_root = layout.generated_crate_root.join("go");
    fs::create_dir_all(go_root.join("bin")).expect("go bin dir");
    fs::write(go_root.join("main.go"), "package main\n\nfunc main() {}\n").expect("go entry");
    fs::write(go_root.join("go.mod"), "module faber/go-pkg\n\ngo 1.21\n").expect("go module file");
    fs::write(go_root.join("bin/go-pkg"), "binary-stub").expect("go binary");

    emit_generated_crate_with_runtime_plan(&layout, "fn main() {}", None, &plan)
        .expect("first emit");
    assert!(
        go_root.join("go.mod").is_file(),
        "go output lost on first publish"
    );

    // Republish swaps the whole `target/faber/` directory; a previously
    // emitted go/ subtree must survive the swap (residual R2).
    emit_generated_crate_with_runtime_plan(&layout, "fn main() {}", None, &plan)
        .expect("second emit");
    assert!(
        go_root.join("main.go").is_file(),
        "go output lost on republish"
    );
    assert!(
        go_root.join("bin/go-pkg").is_file(),
        "go binary lost on republish"
    );
    assert_no_staging_temps(&layout);
}
