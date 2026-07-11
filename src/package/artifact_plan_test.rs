use super::*;
use radix::codegen::Target;
use std::path::PathBuf;

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
