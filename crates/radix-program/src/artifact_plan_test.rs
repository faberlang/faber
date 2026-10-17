use super::*;
use crate::analyze::{AnalyzedProgram, analyze_package};
use crate::discovery::PackageSpec;
use crate::test_support::test_analysis_seams;
use crate::{MANIFEST_FILE, lockfile};
use radix_module::codegen::Target;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn empty_package(root: &str) -> AnalyzedProgram {
    AnalyzedProgram::from_spec(PackageSpec {
        package_root: PathBuf::from(root),
        source_root: PathBuf::from(root).join("src"),
        entry: PathBuf::from(root).join("src/main.fab"),
        templates: BTreeMap::new(),
        manifest_backed: false,
    })
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
fn plan_package_rust_is_supported() {
    let package = empty_package("g4-plan-support");
    let a = plan_package(&package, Target::HirRust);
    assert!(a.supported);
    assert_eq!(a.target, "rust");
}

#[test]
fn artifact_plan_supports_clif() {
    let plan = plan_package(&empty_package("g4-plan-clif"), Target::MirClif);
    assert!(
        plan.supported,
        "clif plan must be supported: {:?}",
        plan.rejection
    );
    assert_eq!(plan.target, "clif");
}

#[test]
fn plan_package_rust_is_deterministic() {
    let package = empty_package("g4-plan-det");
    let a = plan_package(&package, Target::HirRust);
    let b = plan_package(&package, Target::HirRust);
    let a_json = a.to_debug_json();
    let b_json = b.to_debug_json();
    assert_eq!(a_json, b_json);
}

#[test]
fn plan_package_rust_includes_runtime_dependency() {
    let package = empty_package("g4-plan-runtime-dep");
    let a = plan_package(&package, Target::HirRust);
    assert!(
        a.nodes
            .iter()
            .any(|n| n.kind == ArtifactKind::RuntimeDependency)
    );
}

#[test]
fn plan_haskell_entry_and_module_paths_follow_hs_mod() {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut seen = BTreeSet::new();
    let package_id = PackageId {
        name: "hello".to_owned(),
        version: None,
    };
    push_haskell_unit(
        Path::new("/tmp/hello"),
        &package_id,
        &[],
        true,
        &mut nodes,
        &mut edges,
        &mut seen,
    );
    push_haskell_unit(
        Path::new("/tmp/hello"),
        &package_id,
        &["salve-munde".to_owned()],
        false,
        &mut nodes,
        &mut edges,
        &mut seen,
    );
    let entry = nodes
        .iter()
        .find(|node| node.id == "haskell:entry:hello")
        .expect("entry node");
    assert_eq!(entry.kind, ArtifactKind::GeneratedEntry);
    assert_eq!(
        entry.path,
        PathBuf::from("/tmp/hello/target/faber/haskell/app/Main.hs")
    );
    let module = nodes
        .iter()
        .find(|node| node.id == "haskell:module:hello:salve-munde")
        .expect("module node");
    assert_eq!(module.kind, ArtifactKind::GeneratedModule);
    assert_eq!(
        module.path,
        PathBuf::from("/tmp/hello/target/faber/haskell/src/Faber/Romanus/Hello/SalveMunde.hs")
    );
    assert!(edges.iter().any(|edge| {
        edge.from == "haskell:entry:hello"
            && edge.to == "source:hello"
            && edge.kind == DependencyKind::SourceImport
    }));
    assert!(edges.iter().any(|edge| {
        edge.from == "haskell:module:hello:salve-munde"
            && edge.to == "source:hello/salve-munde"
            && edge.kind == DependencyKind::SourceImport
    }));
}

#[test]
fn plan_haskell_empty_module_segments_use_package_file() {
    assert_eq!(
        haskell_module_rel_path("hello", &[]),
        PathBuf::from("src/Faber/Romanus/Hello.hs")
    );
}

#[test]
fn plan_package_go_and_ts_are_supported_seams() {
    let package = empty_package("/tmp/g4-plan-go");
    let go = plan_package(&package, Target::HirGo);
    let ts = plan_package(&package, Target::HirTypeScript);
    let python = plan_package(&package, Target::HirPython);
    let haskell = plan_package(&package, Target::HirHaskell);
    assert!(go.supported);
    assert_eq!(go.target, "go");
    assert!(ts.supported);
    assert_eq!(ts.target, "ts");
    assert!(python.supported);
    assert_eq!(python.target, "python");
    assert!(haskell.supported);
    assert_eq!(haskell.target, "haskell");
}

#[test]
fn plan_package_python_uses_analyzed_module_identities() {
    let root = temp_dir("python-modules");
    fs::create_dir_all(root.join("src/extra")).expect("src");
    fs::write(
        root.join(MANIFEST_FILE),
        r#"
[package]
name = "py-plan"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("manifest");
    fs::write(
        root.join("src/main.fab"),
        "import from \"./extra/util\" util\nprint \"hi\"\n",
    )
    .expect("entry");
    fs::write(
        root.join("src/extra/util.fab"),
        "@ public\nfn greet() → string {\n    return \"x\"\n}\n",
    )
    .expect("module");
    let package = analyze_package(
        &radix_module::driver::Config::default(),
        &root,
        &test_analysis_seams(),
    )
    .expect("analyze two-unit package");
    let plan = plan_package(&package, Target::HirPython);
    assert!(plan.supported);
    assert_eq!(plan.target, "python");
    assert!(
        plan.nodes
            .iter()
            .any(|node| node.id.starts_with("python:entry:") && node.path.ends_with("main.py")),
        "entry artifact missing: {:#?}",
        plan.nodes
    );
    assert!(
        plan.nodes.iter().any(|node| {
            node.id.contains("python:module:") && node.path.ends_with("extra/util.py")
        }),
        "module artifact missing: {:#?}",
        plan.nodes
    );
    assert!(
        plan.nodes
            .iter()
            .any(|node| node.path.ends_with("extra/__init__.py")),
        "package init missing: {:#?}",
        plan.nodes
    );
    assert!(
        plan.edges
            .iter()
            .all(|edge| edge.kind != DependencyKind::SourceImport || edge.to.starts_with("source:")),
        "generated edges must point at analyzed source ids: {:#?}",
        plan.edges
    );
}

#[test]
fn plan_or_reject_fails_closed_for_unsupported_targets() {
    let package = empty_package("/tmp/g4-plan-reject");
    let err = plan_or_reject(&package, Target::MirWgsl).expect_err("wgsl package unsupported");
    assert_eq!(err.issue(), Some("package_target_unsupported"));
}

#[test]
fn plan_package_wasm_is_supported_and_no_longer_rejected() {
    let package = empty_package("g4-plan-wasm");
    let plan = plan_package(&package, Target::MirWasm);
    assert!(plan.supported);
    assert_eq!(plan.target, "wasm-text");
    assert!(plan.rejection.is_none());
    let result = plan_or_reject(&package, Target::MirWasm);
    assert!(
        result.is_ok(),
        "wasm package must no longer be rejected as package_target_unsupported"
    );
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
