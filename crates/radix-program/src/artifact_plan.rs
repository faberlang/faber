//! Target-neutral package artifact planning (HIR v1 G4).
//!
//! Package discovery and analysis run once into [`crate::analyze::AnalyzedProgram`].
//! Target planners select artifacts and bindings from that graph without reloading
//! source or scanning generated text for dependency policy.
//!
//! TARGET: Inspectable debug/test surface for artifact DAGs — not a public ABI.
//! WHY: Product deliveries (Go CLI, browser, `SQLite`) share one analysis graph.
//!
//! RP4: moved from `crates/faber/src/package/artifact_plan.rs`; target-neutral
//! artifact planning is compiler-owned. The radix-side duplicate planner
//! (`crates/radix/src/semantic/artifact_plan.rs`) was deleted in the same unit
//! — never bridged.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use radix_module::codegen::Target;
use radix_module::diagnostics::Diagnostic;
use serde::Serialize;

use crate::analyze::AnalyzedProgram;
use crate::discovery::{PackageSpec, sanitize_crate_name};
use crate::lockfile::{LockedPackage, lock_index, read_lock};
use crate::manifest::{FaberManifest, read_manifest};

/// Qualified package identity (provider/package name + version when known).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct PackageId {
    pub name: String,
    pub version: Option<String>,
}

/// Package-relative module identity (path segments under the package source root).
///
/// This is the one `ModuleId` in the program crate: the [`crate::analyze::AnalyzedProgram`]
/// map key and the artifact-plan identity. Do not mint a second `ModuleId`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ModuleId {
    pub package: PackageId,
    pub segments: Vec<String>,
}

impl ModuleId {
    /// Identity for `segments` inside the package named by `spec`.
    #[must_use]
    pub fn for_spec(spec: &PackageSpec, segments: Vec<String>) -> Self {
        Self {
            package: package_id_from_spec(spec),
            segments,
        }
    }
}

/// Export within a module (function, type, etc.).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub(crate) struct ExportId {
    pub module: ModuleId,
    pub name: String,
}

/// Binding identity for a native-target declaration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub(crate) struct BindingId {
    pub package: PackageId,
    pub key: String,
}

/// Package dependency vs source-import edge kinds stay distinct (G4 invariant).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum DependencyKind {
    PackageDependency,
    SourceImport,
    NativeBinding,
    RuntimeCrate,
}

/// Edge connecting package or module identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub kind: DependencyKind,
}

/// Kind of node in the artifact DAG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum ArtifactKind {
    SourceUnit,
    GeneratedModule,
    GeneratedEntry,
    GeneratedLibrary,
    NativeShim,
    RuntimeDependency,
    TargetDependency,
    PlanOnly,
}

/// One deterministic artifact node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactNode {
    pub id: String,
    pub kind: ArtifactKind,
    pub target: Option<&'static str>,
    pub path: PathBuf,
    pub depends_on: Vec<String>,
}

/// Inspectable target-specific artifact plan (debug/test surface, not public ABI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactPlan {
    pub package: PackageId,
    pub target: &'static str,
    pub supported: bool,
    pub rejection: Option<String>,
    pub nodes: Vec<ArtifactNode>,
    pub edges: Vec<DependencyEdge>,
    pub entry_artifact: Option<String>,
    /// Provider → generated Cargo crate name for native-binding library deps.
    pub linked_library_crates: BTreeMap<String, String>,
}

impl ArtifactPlan {
    /// Stable pretty JSON for tests and tooling.
    /// # Errors
    /// Returns an error when the input or persisted artifact is invalid.
    pub fn to_debug_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|err| err.to_string())
    }

    /// Whether the plan includes a named runtime dependency node (e.g. `rust:runtime:tokio`).
    #[must_use]
    pub fn has_runtime_dependency(&self, id: &str) -> bool {
        self.nodes
            .iter()
            .any(|node| node.kind == ArtifactKind::RuntimeDependency && node.id == id)
    }
}

/// Build an artifact plan from a shared analyzed package for one target.
#[must_use]
pub fn plan_package(package: &AnalyzedProgram, target: Target) -> ArtifactPlan {
    let package_id = package_id_from_spec(&package.spec);
    let target_label = target_name(target);
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut seen_ids = BTreeSet::new();

    push_source_units(package, &package_id, &mut nodes, &mut edges, &mut seen_ids);
    push_package_dependency_edges(package, &package_id, &mut edges);

    let linked = linked_library_crate_map(&package.spec.package_root);
    let (supported, rejection, entry_artifact) = match target {
        Target::HirRust => {
            plan_rust_artifacts(
                package,
                &package_id,
                &linked,
                &mut nodes,
                &mut edges,
                &mut seen_ids,
            );
            let entry = nodes
                .iter()
                .find(|n| n.kind == ArtifactKind::GeneratedEntry)
                .map(|n| n.id.clone());
            (true, None, entry)
        }
        Target::HirGo => {
            plan_go_artifacts(package, &package_id, &mut nodes, &mut edges, &mut seen_ids);
            let entry = nodes
                .iter()
                .find(|n| n.kind == ArtifactKind::GeneratedEntry)
                .map(|n| n.id.clone());
            (true, None, entry)
        }
        Target::HirTypeScript => {
            plan_ts_artifacts(package, &package_id, &mut nodes, &mut edges, &mut seen_ids);
            let entry = nodes
                .iter()
                .find(|n| n.kind == ArtifactKind::GeneratedEntry)
                .map(|n| n.id.clone());
            (true, None, entry)
        }
        Target::HirPython => {
            plan_python_artifacts(package, &package_id, &mut nodes, &mut edges, &mut seen_ids);
            let entry = nodes
                .iter()
                .find(|n| n.kind == ArtifactKind::GeneratedEntry)
                .map(|n| n.id.clone());
            (true, None, entry)
        }
        Target::HirHaskell => {
            plan_haskell_artifacts(package, &package_id, &mut nodes, &mut edges, &mut seen_ids);
            let entry = nodes
                .iter()
                .find(|n| n.kind == ArtifactKind::GeneratedEntry)
                .map(|n| n.id.clone());
            (true, None, entry)
        }
        Target::HirFaber => (
            false,
            Some("package artifact planning does not emit Faber re-source packages yet".to_owned()),
            None,
        ),
        Target::MirWasm => {
            plan_wasm_artifacts(package, &package_id, &mut nodes, &mut edges, &mut seen_ids);
            let entry = nodes
                .iter()
                .find(|n| n.kind == ArtifactKind::GeneratedEntry)
                .map(|n| n.id.clone());
            (true, None, entry)
        }
        other => (
            false,
            Some(format!(
                "package artifact planning does not support target `{}`",
                target_name(other)
            )),
            None,
        ),
    };

    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| (&a.from, &a.to, a.kind as u8).cmp(&(&b.from, &b.to, b.kind as u8)));

    ArtifactPlan {
        package: package_id,
        target: target_label,
        supported,
        rejection,
        nodes,
        edges,
        entry_artifact,
        linked_library_crates: linked,
    }
}

/// Export names that remain in target symbol tables after composition.
///
/// Nucleum fragment bodies are omitted; `Entry` kernels and ordinary exports stay.
#[cfg(feature = "hir-fhir")]
#[must_use]
pub fn target_symbol_exports(package: &AnalyzedProgram) -> Vec<String> {
    crate::nucleum::target_symbol_exports(package)
}

/// Fail closed when the plan is unsupported for the selected target.
pub(crate) fn plan_or_reject(
    package: &AnalyzedProgram,
    target: Target,
) -> Result<ArtifactPlan, Diagnostic> {
    let plan = plan_package(package, target);
    if !plan.supported {
        return Err(crate::package_diagnostic_error(
            plan.rejection
                .clone()
                .unwrap_or_else(|| "package target is unsupported".to_owned()),
        )
        .with_file(package.spec.package_root.display().to_string())
        .with_arg("issue", "package_target_unsupported")
        .with_arg("target", plan.target));
    }
    Ok(plan)
}

/// Map provider names to Cargo crate names for native-binding library deps.
#[must_use]
pub fn linked_library_crate_map(app_root: &Path) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Some(lock) = read_lock(app_root).ok().flatten() else {
        return map;
    };
    let Ok(lock_index) = lock_index(&app_root.join(crate::lockfile::LOCK_FILE), &lock) else {
        return map;
    };
    let Some(manifest) = package_manifest(app_root) else {
        return map;
    };
    for name in manifest.dependencies.keys() {
        let Some(locked) = lock_index.get(name) else {
            continue;
        };
        if library_needs_native_crate(app_root, locked) {
            map.insert(name.clone(), sanitize_crate_name(name));
        }
    }
    map
}

/// Locked packages that must be emitted as generated library crates for Rust.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn native_library_deps(
    package_root: &Path,
) -> Result<Vec<(String, LockedPackage, FaberManifest)>, Vec<Diagnostic>> {
    let manifest_path = package_root.join(crate::MANIFEST_FILE);
    if !manifest_path.is_file() {
        return Ok(Vec::new());
    }
    let manifest = match read_manifest(&manifest_path) {
        Ok(manifest) => manifest,
        Err(diag) => return Err(vec![*diag]),
    };
    if manifest.dependencies.is_empty() {
        return Ok(Vec::new());
    }
    let lock = match read_lock(package_root) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            return Err(vec![
                crate::package_diagnostic_error(format!(
                    "faber.toml declares dependencies but {} is missing",
                    package_root.join(crate::lockfile::LOCK_FILE).display()
                ))
                .with_arg("issue", "missing_faber_lock"),
            ]);
        }
        Err(diag) => return Err(vec![*diag]),
    };
    let lock_index = lock_index(&package_root.join(crate::lockfile::LOCK_FILE), &lock)?;
    let mut out = Vec::new();
    let mut diagnostics = Vec::new();
    for (name, version) in &manifest.dependencies {
        let Some(locked) = lock_index.get(name) else {
            diagnostics.push(
                crate::package_diagnostic_error(format!(
                    "dependency `{name}` is missing from {}",
                    crate::lockfile::LOCK_FILE
                ))
                .with_arg("issue", "dependency_missing_from_lock")
                .with_arg("package", name.clone()),
            );
            continue;
        };
        if &locked.version != version {
            diagnostics.push(
                crate::package_diagnostic_error(format!(
                    "dependency `{name}` version mismatch: faber.toml has `{version}`, lock has `{}`",
                    locked.version
                ))
                .with_arg("issue", "dependency_version_mismatch")
                .with_arg("package", name.clone()),
            );
            continue;
        }
        if !library_needs_native_crate(package_root, locked) {
            continue;
        }
        let lib_root = locked.package_root_path(package_root);
        let lib_manifest_path = lib_root.join(crate::MANIFEST_FILE);
        match read_manifest(&lib_manifest_path) {
            Ok(lib_manifest) => {
                if lib_manifest.build.kind != "lib" {
                    diagnostics.push(
                        crate::package_diagnostic_error(format!(
                            "dependency `{name}` package_root is not a library package"
                        ))
                        .with_file(lib_manifest_path.display().to_string())
                        .with_arg("issue", "dependency_not_library")
                        .with_arg("package", name.clone()),
                    );
                    continue;
                }
                if !lib_manifest.build.targets.iter().any(|t| t == "rust") {
                    diagnostics.push(
                        crate::package_diagnostic_error(format!(
                            "dependency `{name}` does not support target `rust`"
                        ))
                        .with_file(lib_manifest_path.display().to_string())
                        .with_arg("issue", "dependency_target_unsupported")
                        .with_arg("package", name.clone())
                        .with_arg("target", "rust"),
                    );
                    continue;
                }
                // Rewrite lock paths to absolute so emit/link consumers do not depend on CWD.
                let mut locked = locked.clone();
                locked.package_root = lib_root.display().to_string();
                locked.interface_root = locked
                    .interface_root_path_for(package_root)
                    .display()
                    .to_string();
                out.push((name.clone(), locked, lib_manifest));
            }
            Err(diag) => diagnostics.push(*diag),
        }
    }
    if diagnostics.is_empty() {
        Ok(out)
    } else {
        Err(diagnostics)
    }
}

fn library_needs_native_crate(app_package_root: &Path, locked: &LockedPackage) -> bool {
    let root = locked.package_root_path(app_package_root);
    if !root.is_dir() {
        return false;
    }
    let Ok(manifest) = read_manifest(&root.join(crate::MANIFEST_FILE)) else {
        return false;
    };
    if manifest.build.kind != "lib" {
        return false;
    }
    manifest
        .target
        .get("rust")
        .and_then(|t| t.bindings.as_ref())
        .is_some()
}

/// Qualified package identity from a live [`PackageSpec`] (manifest name/version
/// when present, otherwise the package-root directory name).
#[must_use]
pub fn package_id_from_spec(spec: &PackageSpec) -> PackageId {
    let manifest = read_manifest(&spec.package_root.join(crate::MANIFEST_FILE)).ok();
    let name = manifest.as_ref().map_or_else(
        || {
            spec.package_root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("package")
                .to_owned()
        },
        |manifest| manifest.package.name.clone(),
    );
    let version = manifest.map(|manifest| manifest.package.version);
    PackageId { name, version }
}

fn package_manifest(package_root: &Path) -> Option<FaberManifest> {
    read_manifest(&package_root.join(crate::MANIFEST_FILE)).ok()
}

fn module_key(module: &ModuleId) -> String {
    if module.segments.is_empty() {
        module.package.name.clone()
    } else {
        format!("{}/{}", module.package.name, module.segments.join("/"))
    }
}

pub(crate) fn target_name(target: Target) -> &'static str {
    target.as_str()
}

fn push_unique_node(
    nodes: &mut Vec<ArtifactNode>,
    seen: &mut BTreeSet<String>,
    node: ArtifactNode,
) {
    if seen.insert(node.id.clone()) {
        nodes.push(node);
    }
}

fn push_source_units(
    package: &AnalyzedProgram,
    package_id: &PackageId,
    nodes: &mut Vec<ArtifactNode>,
    edges: &mut Vec<DependencyEdge>,
    seen: &mut BTreeSet<String>,
) {
    for unit in package.nodes.values() {
        let module = ModuleId {
            package: package_id.clone(),
            segments: unit.module_segments.clone(),
        };
        let id = format!("source:{}", module_key(&module));
        push_unique_node(
            nodes,
            seen,
            ArtifactNode {
                id: id.clone(),
                kind: ArtifactKind::SourceUnit,
                target: None,
                path: unit.path.clone(),
                depends_on: Vec::new(),
            },
        );

        for import in &unit.expanded_library_imports {
            edges.push(DependencyEdge {
                from: id.clone(),
                to: format!("package:{}", import.module.package),
                kind: DependencyKind::SourceImport,
            });
        }
    }
}

fn push_package_dependency_edges(
    package: &AnalyzedProgram,
    package_id: &PackageId,
    edges: &mut Vec<DependencyEdge>,
) {
    let Some(manifest) = package_manifest(&package.spec.package_root) else {
        return;
    };
    for dep_name in manifest.dependencies.keys() {
        edges.push(DependencyEdge {
            from: format!("package:{}", package_id.name),
            to: format!("package:{dep_name}"),
            kind: DependencyKind::PackageDependency,
        });
    }
}

fn plan_rust_artifacts(
    package: &AnalyzedProgram,
    package_id: &PackageId,
    linked: &BTreeMap<String, String>,
    nodes: &mut Vec<ArtifactNode>,
    edges: &mut Vec<DependencyEdge>,
    seen: &mut BTreeSet<String>,
) {
    let gen_root = package
        .spec
        .package_root
        .join("target")
        .join("faber")
        .join("src");
    for unit in package.nodes.values() {
        let module = ModuleId {
            package: package_id.clone(),
            segments: unit.module_segments.clone(),
        };
        let source_id = format!("source:{}", module_key(&module));
        let gen_id = if unit.is_entry {
            format!("rust:entry:{}", package_id.name)
        } else {
            format!("rust:module:{}", module_key(&module))
        };
        let rel = if unit.is_entry {
            PathBuf::from("main.rs")
        } else if unit.module_segments.is_empty() {
            PathBuf::from("lib_unit.rs")
        } else {
            PathBuf::from(format!("{}.rs", unit.module_segments.join("/")))
        };
        push_unique_node(
            nodes,
            seen,
            ArtifactNode {
                id: gen_id.clone(),
                kind: if unit.is_entry {
                    ArtifactKind::GeneratedEntry
                } else {
                    ArtifactKind::GeneratedModule
                },
                target: Some("rust"),
                path: gen_root.join(rel),
                depends_on: vec![source_id.clone()],
            },
        );
        edges.push(DependencyEdge {
            from: gen_id,
            to: source_id,
            kind: DependencyKind::SourceImport,
        });
    }

    push_rust_runtime_node(nodes, seen);

    // Tokio is selected from HIR async facts (same rule as RustRuntimePlan), not
    // from scanning generated text for `tokio::` or the `block_on` spelling.
    let needs_tokio = package.nodes.values().any(|unit| {
        unit.analysis.hir.entry_is_async
            || unit.analysis.hir.items.iter().any(|item| {
                matches!(
                    &item.kind,
                    radix_module::hir::HirItemKind::Function(function) if function.is_async
                )
            })
    });
    if needs_tokio {
        push_unique_node(
            nodes,
            seen,
            ArtifactNode {
                id: "rust:runtime:tokio".to_owned(),
                kind: ArtifactKind::RuntimeDependency,
                target: Some("rust"),
                path: PathBuf::from("tokio"),
                depends_on: Vec::new(),
            },
        );
    }

    for (provider, crate_name) in linked {
        let crate_path = package
            .spec
            .package_root
            .join("target")
            .join("faber")
            .join("deps")
            .join(crate_name);
        let lib_id = format!("rust:library:{provider}");
        push_unique_node(
            nodes,
            seen,
            ArtifactNode {
                id: lib_id.clone(),
                kind: ArtifactKind::GeneratedLibrary,
                target: Some("rust"),
                path: crate_path,
                depends_on: vec![format!("package:{provider}")],
            },
        );
        edges.push(DependencyEdge {
            from: format!("rust:entry:{}", package_id.name),
            to: lib_id,
            kind: DependencyKind::PackageDependency,
        });
    }
}

fn push_rust_runtime_node(nodes: &mut Vec<ArtifactNode>, seen: &mut BTreeSet<String>) {
    push_unique_node(
        nodes,
        seen,
        ArtifactNode {
            id: "rust:runtime:faber".to_owned(),
            kind: ArtifactKind::RuntimeDependency,
            target: Some("rust"),
            path: PathBuf::from("faber"),
            depends_on: Vec::new(),
        },
    );
}

fn plan_go_artifacts(
    package: &AnalyzedProgram,
    package_id: &PackageId,
    nodes: &mut Vec<ArtifactNode>,
    edges: &mut Vec<DependencyEdge>,
    seen: &mut BTreeSet<String>,
) {
    let gen_root = package
        .spec
        .package_root
        .join("target")
        .join("faber")
        .join("go");
    for unit in package.nodes.values() {
        let module = ModuleId {
            package: package_id.clone(),
            segments: unit.module_segments.clone(),
        };
        let source_id = format!("source:{}", module_key(&module));
        let gen_id = if unit.is_entry {
            format!("go:entry:{}", package_id.name)
        } else {
            format!("go:module:{}", module_key(&module))
        };
        let file = if unit.is_entry {
            PathBuf::from("main.go")
        } else if unit.module_segments.is_empty() {
            PathBuf::from("lib.go")
        } else {
            PathBuf::from(format!("{}.go", unit.module_segments.join("_")))
        };
        push_unique_node(
            nodes,
            seen,
            ArtifactNode {
                id: gen_id.clone(),
                kind: if unit.is_entry {
                    ArtifactKind::GeneratedEntry
                } else {
                    ArtifactKind::GeneratedModule
                },
                target: Some("go"),
                path: gen_root.join(file),
                depends_on: vec![source_id.clone()],
            },
        );
        edges.push(DependencyEdge {
            from: gen_id,
            to: source_id,
            kind: DependencyKind::SourceImport,
        });
    }
}

fn plan_ts_artifacts(
    package: &AnalyzedProgram,
    package_id: &PackageId,
    nodes: &mut Vec<ArtifactNode>,
    edges: &mut Vec<DependencyEdge>,
    seen: &mut BTreeSet<String>,
) {
    let gen_root = package
        .spec
        .package_root
        .join("target")
        .join("faber")
        .join("ts");
    for unit in package.nodes.values() {
        let module = ModuleId {
            package: package_id.clone(),
            segments: unit.module_segments.clone(),
        };
        let source_id = format!("source:{}", module_key(&module));
        let gen_id = if unit.is_entry {
            format!("ts:entry:{}", package_id.name)
        } else {
            format!("ts:module:{}", module_key(&module))
        };
        let file = if unit.is_entry {
            PathBuf::from("main.ts")
        } else if unit.module_segments.is_empty() {
            PathBuf::from("index.ts")
        } else {
            PathBuf::from(format!("{}.ts", unit.module_segments.join("/")))
        };
        push_unique_node(
            nodes,
            seen,
            ArtifactNode {
                id: gen_id.clone(),
                kind: if unit.is_entry {
                    ArtifactKind::GeneratedEntry
                } else {
                    ArtifactKind::GeneratedModule
                },
                target: Some("ts"),
                path: gen_root.join(file),
                depends_on: vec![source_id.clone()],
            },
        );
        edges.push(DependencyEdge {
            from: gen_id,
            to: source_id,
            kind: DependencyKind::SourceImport,
        });
    }
}

fn plan_python_artifacts(
    package: &AnalyzedProgram,
    package_id: &PackageId,
    nodes: &mut Vec<ArtifactNode>,
    edges: &mut Vec<DependencyEdge>,
    seen: &mut BTreeSet<String>,
) {
    let gen_root = package
        .spec
        .package_root
        .join("target")
        .join("faber")
        .join("python");
    for unit in package.nodes.values() {
        let module = ModuleId {
            package: package_id.clone(),
            segments: unit.module_segments.clone(),
        };
        let source_id = format!("source:{}", module_key(&module));
        let gen_id = if unit.is_entry {
            format!("python:entry:{}", package_id.name)
        } else {
            format!("python:module:{}", module_key(&module))
        };
        let file = if unit.is_entry {
            PathBuf::from("main.py")
        } else if unit.module_segments.is_empty() {
            PathBuf::from("__init__.py")
        } else {
            let mut path = PathBuf::new();
            let last = unit.module_segments.len() - 1;
            for (idx, segment) in unit.module_segments.iter().enumerate() {
                if idx == last {
                    path.push(format!("{segment}.py"));
                } else {
                    path.push(segment);
                }
            }
            path
        };
        push_unique_node(
            nodes,
            seen,
            ArtifactNode {
                id: gen_id.clone(),
                kind: if unit.is_entry {
                    ArtifactKind::GeneratedEntry
                } else {
                    ArtifactKind::GeneratedModule
                },
                target: Some("python"),
                path: gen_root.join(&file),
                depends_on: vec![source_id.clone()],
            },
        );
        edges.push(DependencyEdge {
            from: gen_id,
            to: source_id,
            kind: DependencyKind::SourceImport,
        });
        if !unit.is_entry && unit.module_segments.len() > 1 {
            let mut package_dir = gen_root.clone();
            for segment in &unit.module_segments[..unit.module_segments.len() - 1] {
                package_dir.push(segment);
                let init_id = format!("python:init:{}", package_dir.display());
                push_unique_node(
                    nodes,
                    seen,
                    ArtifactNode {
                        id: init_id,
                        kind: ArtifactKind::GeneratedModule,
                        target: Some("python"),
                        path: package_dir.join("__init__.py"),
                        depends_on: Vec::new(),
                    },
                );
            }
        }
    }
}

fn plan_haskell_artifacts(
    package: &AnalyzedProgram,
    package_id: &PackageId,
    nodes: &mut Vec<ArtifactNode>,
    edges: &mut Vec<DependencyEdge>,
    seen: &mut BTreeSet<String>,
) {
    for unit in package.nodes.values() {
        push_haskell_unit(
            &package.spec.package_root,
            package_id,
            &unit.module_segments,
            unit.is_entry,
            nodes,
            edges,
            seen,
        );
    }
}

/// HS-MOD path and identity for one analyzed unit.
///
/// Segment capitalization stays aligned with
/// `radix_hir_haskell::names::sanitize_module_segment`.
fn push_haskell_unit(
    package_root: &Path,
    package_id: &PackageId,
    segments: &[String],
    is_entry: bool,
    nodes: &mut Vec<ArtifactNode>,
    edges: &mut Vec<DependencyEdge>,
    seen: &mut BTreeSet<String>,
) {
    let module = ModuleId {
        package: package_id.clone(),
        segments: segments.to_vec(),
    };
    let source_id = format!("source:{}", module_key(&module));
    let gen_id = if is_entry {
        format!("haskell:entry:{}", package_id.name)
    } else if segments.is_empty() {
        format!("haskell:module:{}", package_id.name)
    } else {
        format!("haskell:module:{}:{}", package_id.name, segments.join("/"))
    };
    let gen_root = package_root.join("target").join("faber").join("haskell");
    let file = if is_entry {
        PathBuf::from("app/Main.hs")
    } else {
        haskell_module_rel_path(&package_id.name, segments)
    };
    push_unique_node(
        nodes,
        seen,
        ArtifactNode {
            id: gen_id.clone(),
            kind: if is_entry {
                ArtifactKind::GeneratedEntry
            } else {
                ArtifactKind::GeneratedModule
            },
            target: Some("haskell"),
            path: gen_root.join(file),
            depends_on: vec![source_id.clone()],
        },
    );
    edges.push(DependencyEdge {
        from: gen_id,
        to: source_id,
        kind: DependencyKind::SourceImport,
    });
}

fn haskell_module_rel_path(package_name: &str, segments: &[String]) -> PathBuf {
    let mut parts = vec![
        "src".to_owned(),
        "Faber".to_owned(),
        "Romanus".to_owned(),
        haskell_pascal_segment(package_name),
    ];
    if segments.is_empty() {
        let file = format!("{}.hs", parts.pop().unwrap_or_else(|| "Module".to_owned()));
        return PathBuf::from_iter(parts).join(file);
    }
    for (index, segment) in segments.iter().enumerate() {
        let piece = haskell_pascal_segment(segment);
        let piece = if piece == "Main" {
            "Main_".to_owned()
        } else {
            piece
        };
        if index + 1 == segments.len() {
            parts.push(format!("{piece}.hs"));
        } else {
            parts.push(piece);
        }
    }
    PathBuf::from_iter(parts)
}

fn haskell_pascal_segment(raw: &str) -> String {
    let pieces: Vec<String> = raw
        .split(['-', '_', '.', '/'])
        .filter(|piece| !piece.is_empty())
        .map(|piece| {
            let mut chars = piece.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = String::new();
                    out.extend(first.to_uppercase());
                    out.extend(chars);
                    out
                }
                None => "Module".to_owned(),
            }
        })
        .collect();
    if pieces.is_empty() {
        "Module".to_owned()
    } else {
        pieces.concat()
    }
}

fn plan_wasm_artifacts(
    package: &AnalyzedProgram,
    package_id: &PackageId,
    nodes: &mut Vec<ArtifactNode>,
    edges: &mut Vec<DependencyEdge>,
    seen: &mut BTreeSet<String>,
) {
    // One package-aware `.wat` module per unit under target/faber/wasm, named
    // deterministically like the package-to-Wasm builder writes them
    // (zero-padded index + product + canonical module segments).
    let product = package
        .spec
        .package_root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| package_id.name.clone());
    let gen_root = package
        .spec
        .package_root
        .join("target")
        .join("faber")
        .join("wasm");
    for (index, unit) in package.nodes.values().enumerate() {
        let module = ModuleId {
            package: package_id.clone(),
            segments: unit.module_segments.clone(),
        };
        let source_id = format!("source:{}", module_key(&module));
        let gen_id = if unit.is_entry {
            format!("wasm:entry:{}", package_id.name)
        } else {
            format!("wasm:module:{}", module_key(&module))
        };
        let segments = if unit.module_segments.is_empty() {
            "root".to_owned()
        } else {
            unit.module_segments.join("-")
        };
        push_unique_node(
            nodes,
            seen,
            ArtifactNode {
                id: gen_id.clone(),
                kind: if unit.is_entry {
                    ArtifactKind::GeneratedEntry
                } else {
                    ArtifactKind::GeneratedModule
                },
                target: Some("wasm"),
                path: gen_root.join(format!("{index:03}-{product}-{segments}.wat")),
                depends_on: vec![source_id.clone()],
            },
        );
        edges.push(DependencyEdge {
            from: gen_id,
            to: source_id,
            kind: DependencyKind::SourceImport,
        });
    }
}

#[cfg(test)]
#[path = "artifact_plan_test.rs"]
mod tests;
