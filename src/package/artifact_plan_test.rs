use super::*;
use crate::package::{lockfile, MANIFEST_FILE};
use radix::codegen::Target;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn empty_package(root: &str) -> AnalyzedPackage {
    AnalyzedPackage {
        spec: PackageSpec {
            package_root: PathBuf::from(root),
            source_root: PathBuf::from(root).join("src"),
            entry: PathBuf::from(root).join("src/main.fab"),
        },
        units: Vec::new(),
        entry_frontmatter: None,
        diagnostics: Vec::new(),
        linked_library_crates: BTreeMap::new(),
    }
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("faber-artifact-plan-{label}-{nonce}"));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

#[test]
fn plan_package_rust_is_supported_and_deterministic() {
    let package = empty_package("/tmp/g4-plan-a");
    let a = plan_package(&package, Target::Rust);
    let b = plan_package(&package, Target::Rust);
    assert!(a.supported);
    assert_eq!(a.target, "rust");
    let a_json = a.to_debug_json();
    let b_json = b.to_debug_json();
    assert_eq!(a_json, b_json);
    assert!(a
        .nodes
        .iter()
        .any(|n| n.kind == ArtifactKind::RuntimeDependency));
}

#[test]
fn plan_package_go_and_ts_are_supported_seams() {
    let package = empty_package("/tmp/g4-plan-go");
    let go = plan_package(&package, Target::Go);
    let ts = plan_package(&package, Target::TypeScript);
    assert!(go.supported);
    assert_eq!(go.target, "go");
    assert!(ts.supported);
    assert_eq!(ts.target, "ts");
}

#[test]
fn plan_or_reject_fails_closed_for_unsupported_targets() {
    let package = empty_package("/tmp/g4-plan-reject");
    let err = plan_or_reject(&package, Target::Wasm).expect_err("wasm package unsupported");
    assert_eq!(err.issue(), Some("package_target_unsupported"));
}

#[test]
fn package_dependency_edges_are_distinct_from_source_imports() {
    assert_ne!(
        DependencyKind::PackageDependency,
        DependencyKind::SourceImport
    );
    assert_ne!(DependencyKind::NativeBinding, DependencyKind::RuntimeCrate);
}

#[test]
fn native_library_deps_rejects_duplicate_lock_package_names() {
    let root = temp_dir("duplicate-lock");
    fs::create_dir_all(root.join("src")).expect("create app src");
    fs::write(
        root.join(MANIFEST_FILE),
        r#"
[package]
name = "app"

[paths]
source = "src"
entry = "main.fab"

[dependencies]
liba = "2.0.0"
"#,
    )
    .expect("write manifest");
    fs::write(
        root.join(lockfile::LOCK_FILE),
        r#"
[[package]]
name = "liba"
version = "1.0.0"
source = "path"
package_root = "lib-a"
kind = "lib"
target_language = "rust"
target_triple = "host"
target_manifest = ""
interface_root = "lib-a/src"
artifact = ""
crate = "liba"
rustc = ""

[[package]]
name = "liba"
version = "2.0.0"
source = "path"
package_root = "lib-b"
kind = "lib"
target_language = "rust"
target_triple = "host"
target_manifest = ""
interface_root = "lib-b/src"
artifact = ""
crate = "liba"
rustc = ""
"#,
    )
    .expect("write lock");

    let diagnostics = native_library_deps(&root).expect_err("duplicate lock should fail closed");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("duplicate_locked_package")),
        "expected duplicate lock diagnostic, got {diagnostics:?}"
    );
}
