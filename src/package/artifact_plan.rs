//! Target-neutral package artifact planning (HIR v1 G4).
//!
//! Package discovery and analysis run once into [`super::compile::AnalyzedPackage`].
//! Target planners select artifacts and bindings from that graph without reloading
//! source or scanning generated text for dependency policy.
//!
//! TARGET: Inspectable debug/test surface for artifact DAGs — not a public ABI.
//! WHY: Product deliveries (Go CLI, browser, SQLite) share one analysis graph.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use radix::codegen::Target;
use radix::diagnostics::Diagnostic;
use serde::Serialize;

use super::compile::AnalyzedPackage;
use super::discovery::sanitize_crate_name;
use super::lockfile::{read_lock, LockedPackage};
use super::manifest::{read_manifest, FaberManifest};
use super::PackageSpec;

/// Qualified package identity (provider/package name + version when known).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct PackageId {
    pub name: String,
    pub version: Option<String>,
}

/// Package-relative module identity (path segments under the package source root).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ModuleId {
    pub package: PackageId,
    pub segments: Vec<String>,
}

/// Export within a module (function, type, etc.).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ExportId {
    pub module: ModuleId,
    pub name: String,
}

/// Binding identity for a native-target declaration.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct BindingId {
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
    pub fn to_debug_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|err| err.to_string())
    }

    /// Whether the plan includes a named runtime dependency node (e.g. `rust:runtime:tokio`).
    pub fn has_runtime_dependency(&self, id: &str) -> bool {
        self.nodes
            .iter()
            .any(|node| node.kind == ArtifactKind::RuntimeDependency && node.id == id)
    }
}

/// Build an artifact plan from a shared analyzed package for one target.
pub(crate) fn plan_package(package: &AnalyzedPackage, target: Target) -> ArtifactPlan {
    let package_id = package_id_from_spec(&package.spec);
    let target_name = target_name(target);
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut seen_ids = BTreeSet::new();

    push_source_units(package, &package_id, &mut nodes, &mut edges, &mut seen_ids);
    push_package_dependency_edges(package, &package_id, &mut edges);

    let linked = linked_library_crate_map(package);
    let (supported, rejection, entry_artifact) = match target {
        Target::Rust => {
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
        Target::Go => {
            plan_go_artifacts(package, &package_id, &mut nodes, &mut edges, &mut seen_ids);
            let entry = nodes
                .iter()
                .find(|n| n.kind == ArtifactKind::GeneratedEntry)
                .map(|n| n.id.clone());
            (true, None, entry)
        }
        Target::TypeScript => {
            plan_ts_artifacts(package, &package_id, &mut nodes, &mut edges, &mut seen_ids);
            let entry = nodes
                .iter()
                .find(|n| n.kind == ArtifactKind::GeneratedEntry)
                .map(|n| n.id.clone());
            (true, None, entry)
        }
        Target::Faber => (
            false,
            Some("package artifact planning does not emit Faber re-source packages yet".to_owned()),
            None,
        ),
        other => (
            false,
            Some(format!(
                "package artifact planning does not support target `{}`",
                super::artifact_plan::target_name(other)
            )),
            None,
        ),
    };

    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    edges.sort_by(|a, b| (&a.from, &a.to, a.kind as u8).cmp(&(&b.from, &b.to, b.kind as u8)));

    ArtifactPlan {
        package: package_id,
        target: target_name,
        supported,
        rejection,
        nodes,
        edges,
        entry_artifact,
        linked_library_crates: linked,
    }
}

/// Fail closed when the plan is unsupported for the selected target.
pub(crate) fn plan_or_reject(
    package: &AnalyzedPackage,
    target: Target,
) -> Result<ArtifactPlan, Diagnostic> {
    let plan = plan_package(package, target);
    if !plan.supported {
        return Err(Diagnostic::error(
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
pub(crate) fn linked_library_crate_map(package: &AnalyzedPackage) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let app_root = &package.spec.package_root;
    let Some(lock) = read_lock(app_root).ok().flatten() else {
        return map;
    };
    let Some(manifest) = package_manifest(package) else {
        return map;
    };
    for name in manifest.dependencies.keys() {
        let Some(locked) = lock.packages.iter().find(|p| &p.name == name) else {
            continue;
        };
        if library_needs_native_crate(app_root, locked) {
            map.insert(name.clone(), sanitize_crate_name(name));
        }
    }
    map
}

/// Locked packages that must be emitted as generated library crates for Rust.
pub(crate) fn native_library_deps(
    package_root: &Path,
) -> Result<Vec<(String, LockedPackage, FaberManifest)>, Vec<Diagnostic>> {
    let manifest_path = package_root.join(super::MANIFEST_FILE);
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
            return Err(vec![Diagnostic::error(format!(
                "faber.toml declares dependencies but {} is missing",
                package_root.join(super::lockfile::LOCK_FILE).display()
            ))
            .with_arg("issue", "missing_faber_lock")]);
        }
        Err(diag) => return Err(vec![*diag]),
    };
    let mut out = Vec::new();
    let mut diagnostics = Vec::new();
    for (name, version) in &manifest.dependencies {
        let Some(locked) = lock.packages.iter().find(|p| &p.name == name) else {
            diagnostics.push(
                Diagnostic::error(format!(
                    "dependency `{name}` is missing from {}",
                    super::lockfile::LOCK_FILE
                ))
                .with_arg("issue", "dependency_missing_from_lock")
                .with_arg("package", name.clone()),
            );
            continue;
        };
        if &locked.version != version {
            diagnostics.push(
                Diagnostic::error(format!(
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
        let lib_manifest_path = lib_root.join(super::MANIFEST_FILE);
        match read_manifest(&lib_manifest_path) {
            Ok(lib_manifest) => {
                if lib_manifest.build.kind != "lib" {
                    diagnostics.push(
                        Diagnostic::error(format!(
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
                        Diagnostic::error(format!(
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
    let Ok(manifest) = read_manifest(&root.join(super::MANIFEST_FILE)) else {
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

fn package_id_from_spec(spec: &PackageSpec) -> PackageId {
    let name = read_manifest(&spec.package_root.join(super::MANIFEST_FILE))
        .ok()
        .map(|m| m.package.name)
        .unwrap_or_else(|| {
            spec.package_root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("package")
                .to_owned()
        });
    let version = read_manifest(&spec.package_root.join(super::MANIFEST_FILE))
        .ok()
        .map(|m| m.package.version);
    PackageId { name, version }
}

fn package_manifest(package: &AnalyzedPackage) -> Option<FaberManifest> {
    read_manifest(&package.spec.package_root.join(super::MANIFEST_FILE)).ok()
}

fn module_key(module: &ModuleId) -> String {
    if module.segments.is_empty() {
        module.package.name.clone()
    } else {
        format!("{}/{}", module.package.name, module.segments.join("/"))
    }
}

pub(crate) fn target_name(target: Target) -> &'static str {
    match target {
        Target::Rust => "rust",
        Target::TypeScript => "ts",
        Target::Go => "go",
        Target::Faber => "faber",
        Target::Wasm => "wasm",
        Target::WasmText => "wasm-text",
        Target::LlvmText => "llvm-text",
        Target::MetalText => "metal-text",
        Target::WgslText => "wgsl-text",
        Target::Sexp => "sexp",
        Target::Scena => "scena",
        Target::FmirText => "fmir-text",
        Target::Fmir => "fmir",
        Target::FmirBin => "fmir-bin",
    }
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
    package: &AnalyzedPackage,
    package_id: &PackageId,
    nodes: &mut Vec<ArtifactNode>,
    edges: &mut Vec<DependencyEdge>,
    seen: &mut BTreeSet<String>,
) {
    for unit in &package.units {
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

        for export in &unit.export_names {
            let _export = ExportId {
                module: module.clone(),
                name: export.clone(),
            };
        }

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
    package: &AnalyzedPackage,
    package_id: &PackageId,
    edges: &mut Vec<DependencyEdge>,
) {
    let Some(manifest) = package_manifest(package) else {
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
    package: &AnalyzedPackage,
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
    for unit in &package.units {
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

    push_unique_node(
        nodes,
        seen,
        ArtifactNode {
            id: "rust:runtime:faber".to_owned(),
            kind: ArtifactKind::RuntimeDependency,
            target: Some("rust"),
            path: PathBuf::from("faber-runtime"),
            depends_on: Vec::new(),
        },
    );

    // Tokio is selected from HIR async facts (same rule as RustRuntimePlan), not
    // from scanning generated text for `tokio::` / `__faber_block_on`.
    let needs_tokio = package.units.iter().any(|unit| {
        unit.analysis.hir.entry_is_async
            || unit.analysis.hir.items.iter().any(|item| {
                matches!(
                    &item.kind,
                    radix::hir::HirItemKind::Function(function) if function.is_async
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

fn plan_go_artifacts(
    package: &AnalyzedPackage,
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
    for unit in &package.units {
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
    package: &AnalyzedPackage,
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
    for unit in &package.units {
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

#[cfg(test)]
#[path = "artifact_plan_test.rs"]
mod tests;
