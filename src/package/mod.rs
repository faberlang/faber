//! Package build orchestration for the user-facing `faber` CLI.
//!
//! This module is the boundary between Faber source packages and the generated
//! Rust crate that Cargo builds. It owns package discovery, manifest policy,
//! import graph loading, built-in library binding, mounted CLI command analysis,
//! generated-crate layout, and Cargo invocation. Compiler parsing, semantic
//! analysis, and backend code generation remain in `radix`; this file decides
//! how many source files and package-level policies become one compiler input
//! and one generated build artifact.
//!
//! INVARIANTS
//! ==========
//! - Package mode is filesystem-backed; stdin cannot represent imports,
//!   manifests, or generated layouts.
//! - Generated Rust crates live under `<package>/target/faber/`.
//! - Cargo artifacts live under sibling `<package>/target/{debug,release}/`.
//! - Manifest, import, mount, and package-policy errors are diagnostics, not
//!   silent fallbacks to single-file compilation.
//! - Built-in library interfaces are parsed as Faber source so package builds
//!   do not need compiler-only special cases for stdlib APIs.
//!
//! COMPATIBILITY
//! =============
//! Legacy direct-file and directory inputs are still accepted where possible.
//! Those paths deliberately share layout discovery with manifest-backed
//! packages so old examples keep deterministic binary names and target paths
//! while `faber.toml` remains the preferred package surface.

// Diagnostic is a large CLI error type by design; returning it by value is deliberate (faber is a CLI, not a hot library path).
#![allow(clippy::result_large_err)]

pub mod artifact_plan;
pub mod binding;
#[cfg(feature = "hir-rust")]
mod binding_probe;
mod cargo;
mod cmd;
#[cfg(feature = "hir-rust")]
mod codegen;
mod compile;
mod device;
mod discovery;
mod dispatch;
#[cfg(feature = "hir-fhir")]
mod fhir;
mod file_interface;
mod frontmatter;
#[cfg(feature = "hir-go")]
mod go_build;
mod host_factory;
mod import_graph;
mod library;
#[cfg_attr(not(feature = "hir-rust"), path = "library_link_unavailable.rs")]
mod library_link;
mod llvm;
mod llvm_host;
mod locale;
mod lockfile;
mod manifest;
mod member_path;
mod mir;
mod modules;
mod paths;
mod product;
mod runtime_dependency;
#[cfg(feature = "hir-rust")]
mod rust_runtime_dependency;
#[cfg(feature = "hir-rust")]
pub(crate) mod rust_target;
mod source_files;
mod test_source_filter;

#[allow(unused_imports)]
// public package API for library callers; binary crate does not use it.
pub use artifact_plan::ArtifactPlan;
#[cfg(feature = "hir-rust")]
#[allow(unused_imports)]
// public package API for library callers/tests; binary may not call it.
pub use binding::verify_library_binding_shapes;
#[allow(unused_imports)]
// public package API for library callers/tests; binary crate only uses full verification.
pub use binding::{
    verify_library_bindings, verify_library_bindings_with_probe_mode, BindingProbeMode,
};
pub use test_source_filter::TestSourceFilter;
// used by `commands/run.rs` / tests for G4 library path-deps on package emit
#[allow(unused_imports)]
// library crate tests use this re-export; binary crate imports cargo directly.
pub(crate) use cargo::emit_generated_crate_with_runtime_plan;
#[allow(unused_imports)]
// used by `commands/run.rs` and `commands/test.rs` in the binary crate
pub(crate) use cargo::invoke_cargo_build;
#[allow(unused_imports)]
// public package API for tests/library callers; binary crate uses narrower seams.
pub use cargo::{emit_generated_crate, invoke_cargo_test};
pub(crate) use cargo::{package_host_selection_diagnostic, RustRuntimePlan};
#[cfg(test)]
pub use cmd::use_package_compiler;
pub use cmd::{
    cmd_build, cmd_check_package, cmd_emit_package, should_treat_as_package_from_args,
    use_package_compiler_from_args,
};
#[cfg(feature = "hir-go")]
#[allow(unused_imports)] // binary build/run paths consume this crate-visible Go entry.
pub(crate) use compile::compile_package_go;
pub(crate) use compile::package_rust_runtime_plan;
#[allow(unused_imports)] // package MIR stages consume this crate-visible analysis API.
pub(crate) use compile::{
    analyze_package, analyze_package_for_tests, AnalyzedPackage, AnalyzedPackageUnit,
};
#[cfg(feature = "hir-fhir")]
#[allow(unused_imports)]
// binary commands/run consumes load/run + PackageFhir; lib uses build_package_fhir.
pub(crate) use fhir::{
    build_package_fhir, load_package_fhir, run_loaded_package_fhir, PackageFhir,
};
// `compile_package_with_test_*` remain public for lib tests / tooling; the
// binary `faber test` path no longer emits Rust tests (stepper-only).
#[allow(unused_imports)] // binary crate root does not call test compile helpers
pub use compile::{
    check_package, compile_package, compile_package_with_test_options,
    compile_package_with_test_selection,
};
#[allow(unused_imports)] // public package API; used by integration tests and external callers
pub use discovery::{discover_build_layout, sanitize_crate_name, BuildLayout};
pub(crate) use dispatch::{
    load_provider_manifests, selected_providers_for_routes, ProviderManifest,
};
#[cfg(feature = "hir-go")]
#[allow(unused_imports)] // binary `commands/run` consumes run_go_binary
pub(crate) use go_build::{emit_go_module, invoke_go_build, run_go_binary, GoBuildLayout};
#[allow(unused_imports)] // binary `commands/run` + `commands/test` G4 linkage
pub(crate) use library_link::emit_linked_library_crates;
pub(crate) use manifest::validate_manifest;
#[allow(unused_imports)]
// binary commands/run resolves run targets through this crate-visible mapping.
pub(crate) use manifest::{
    manifest_backend_selection, manifest_build_target, manifest_device_inputs,
};
#[allow(unused_imports)] // public package API; used by integration tests and external callers
pub use manifest::{
    read_manifest, FaberManifest, ManifestBuild, ManifestDispatch, ManifestLibrary,
    ManifestPackage, ManifestPaths, ManifestProduct, ManifestProductEmit, ManifestProductKind,
    ManifestProductShaders, ManifestRustFieldNames, ManifestRustHost,
};
pub(crate) use product::build_browser_product_with_postprocess;
#[cfg(test)]
pub(crate) use product::{build_browser_product, build_browser_product_static_assets};
// binary-only package interpretation route consumes this through `commands`.
#[allow(unused_imports)] // the S1-6 device-route seam (constructor + execution).
pub(crate) use device::{
    admit_device_program_section, device_program_for_lowered, device_section_for_program,
    execute_device_route,
};
#[allow(unused_imports)] // the one host-construction policy for the run routes.
pub(crate) use host_factory::{
    admitted_backends, construct_composite_host, create_program_session, discovery_receipt,
    effective_backend_selection, execute_device_descriptor, host_error_diagnostic,
    missing_backend_artifact, missing_device_descriptor, resolve_backend_selection,
    BackendDiscoveryReceipt, E_BACKEND_UNAVAILABLE, E_DEVICE_ABI_MISMATCH, E_DEVICE_DESCRIPTOR,
    E_DEVICE_DTYPE_MISMATCH, E_DEVICE_ENTRY_MISMATCH, E_DEVICE_SHAPE_MISMATCH, E_NO_DEVICE_PROGRAM,
};
#[allow(unused_imports)]
// graph-rooted build is package-crate internal (graph type is pub(crate)).
pub(crate) use llvm::build_package_llvm_from_graph;
#[allow(unused_imports)]
// LLVM host harnesses consume the reusable package-to-LLVM builder (S8.3).
pub use llvm::{
    build_package_llvm, PackageLlvmBuild, PackageLlvmLinkManifest, PackageLlvmModule,
    PackageLlvmOptions,
};
#[allow(unused_imports)]
// `faber build/run --target llvm-host` + product tests consume the Stage 9 lane.
pub use llvm_host::{
    build_host_program, discover_llvm_host_toolchain, ensure_llvm_runtime_archive,
    host_llvm_target_triple, host_triple_for, LlvmHostBuild, LlvmHostProfile,
};
#[allow(unused_imports)] // generated fmir-bin runner crates consume this public API.
pub use mir::run_fmir_image_bytes_with_stdio;
#[cfg(test)]
pub(super) use mir::test_support::{fmir_image_test_summary, fmir_text_image_test_summary};
#[allow(unused_imports)] // External backend harnesses consume the public package-MIR callback.
pub use mir::with_lowered_package_mir;
#[allow(unused_imports)] // FMIR stages consume this crate-visible image API.
pub(crate) use mir::{
    build_package_fmir_binary_bundle, build_package_fmir_image, build_package_fmir_text_image,
    fmir_image_route_decision, run_fmir_image_path, run_fmir_image_path_with_selection,
    run_package_fmir_image, run_package_fmir_image_with_selection, run_package_fmir_text_image,
    run_package_fmir_text_image_with_selection,
};
#[allow(unused_imports)]
pub(crate) use mir::{build_package_mir_artifact, run_package_mir, run_package_mir_artifact};

// binary-only `faber run --interpret` route consumes this through `commands`.
#[allow(unused_imports)]
pub(crate) use discovery::{discover_package, is_manifest_backed_or_directory_package_input};

use crate::library::LibraryResolver;
use radix::diagnostics::Diagnostic;
use radix::driver::FileFrontmatter;
use radix::lexer::Interner;
use radix::syntax::{Program, StmtKind, Visibility};
use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

pub(crate) use discovery::PackageSpec;
use import_graph::{
    detect_import_cycles, import_unsupported_diagnostic, library_import_binding,
    library_import_kind_diagnostic, resolve_import, ImportResolution,
};
use library::expand_library_imports;
pub(crate) use member_path::resolve_package_member;
use modules::module_segments_for_file;
use paths::normalize_path;
use source_files::{is_proba_source_path, load_package_source, package_source_files};

pub(crate) use library::{
    analysis_source_for_file, library_cached_analysis, library_cached_file_interface,
    library_interface_export_names, library_interface_has_module, program_export_names,
    LibraryInterfaceCache,
};
#[cfg(feature = "hir-rust")]
pub(crate) use library::{
    library_cached_expanded_imports, library_generates_rust_module, library_module_segments,
    with_library_cached_analysis_mut,
};
pub use locale::locale_pack_for_emit;
#[allow(unused_imports)]
pub(crate) use locale::{
    config_with_locale, default_config_with_locale, load_locale_pack_for_input,
};

pub(super) const MANIFEST_FILE: &str = "faber.toml";

pub(super) struct PackageFile {
    path: PathBuf,
    module_segments: Vec<String>,
    /// Original on-disk source, including optional `+++` frontmatter.
    #[allow(dead_code)] // retained for future package inspect/diagnostic surfaces
    raw_source: String,
    /// Peeled Faber body used for parse spans and semantic analysis.
    source: String,
    frontmatter: Option<FileFrontmatter>,
    program: Program,
    interner: Interner,
    /// Direct `norma:` (and future provider) imports declared in this package file.
    library_imports: Vec<LibraryImportBinding>,
    /// Transitive closure of [`library_imports`], dependencies first, deduped by identity.
    expanded_library_imports: Vec<LibraryImportBinding>,
}

#[derive(Clone)]
pub(super) struct LibraryImportBinding {
    binding: String,
    /// Preserved for Milestone C file-namespace re-export wiring.
    #[allow(dead_code)]
    visibility: Visibility,
    import_span: radix::lexer::Span,
    module: crate::library::ResolvedLibraryModule,
}

pub(crate) fn library_resolver_from_config(config: &radix::driver::Config) -> LibraryResolver {
    // The library home (provider repos such as `norma/` and `triga/`) is
    // independent of the reader-pack stdlib path (`stdlib/locale/<id>/pack.toml`).
    // Precedence: an explicit custom stdlib roots the library home; otherwise
    // `FABER_LIBRARY_HOME` and the workspace probe locate provider repos. The
    // auto-discovered dev stdlib is never a library home.
    if config.stdlib_explicit {
        return config
            .stdlib_path
            .as_ref()
            .map(|path| LibraryResolver::new(path.clone()))
            .unwrap_or_else(LibraryResolver::default);
    }
    LibraryResolver::default()
}

/// Build a library resolver for a package root, attaching `faber.toml`
/// dependencies and `faber.lock` interface roots when present.
pub(crate) fn library_resolver_for_package(
    config: &radix::driver::Config,
    package_root: &std::path::Path,
) -> Result<LibraryResolver, Vec<Diagnostic>> {
    let mut resolver = library_resolver_from_config(config);
    let manifest_path = package_root.join(MANIFEST_FILE);
    if !manifest_path.is_file() {
        return Ok(resolver);
    }
    let manifest = match read_manifest(&manifest_path) {
        Ok(manifest) => manifest,
        Err(diag) => return Err(vec![*diag]),
    };
    let lock = match lockfile::read_lock(package_root) {
        Ok(lock) => lock,
        Err(diag) => return Err(vec![*diag]),
    };
    let mut lock_diags = lockfile::validate_dependencies_against_lock(
        package_root,
        &manifest.dependencies,
        lock.as_ref(),
    );
    if lock_diags.iter().any(Diagnostic::is_error) {
        return Err(lock_diags);
    }
    // Non-error path notes are unused for now; keep only errors.
    lock_diags.clear();

    let mut locked = std::collections::BTreeMap::new();
    if let Some(lock) = lock.as_ref() {
        let lock_index = lockfile::lock_index(&package_root.join(lockfile::LOCK_FILE), lock)?;
        for package in lock_index.values() {
            locked.insert(
                package.name.clone(),
                crate::library::LockedLibraryPackage {
                    name: package.name.clone(),
                    version: package.version.clone(),
                    interface_root: package.interface_root_path_for(package_root),
                },
            );
        }
    }
    resolver = resolver.with_package_lock(manifest.dependencies, locked);
    Ok(resolver)
}

/// Test-facing convenience wrapper around [`load_package_with_locale_pack`]
/// (no reader pack, no proba filtering).
///
/// Dead-code allowance (lib/bin non-test builds): every production path calls
/// [`load_package_with_locale_pack`] or the higher-level compile/run routes
/// directly, while the package test suites (`package_test`,
/// `frontmatter_integration_test`, `proba_integration_test`,
/// `proba_stepper_test`) exercise this wrapper. The deferred FHIR package
/// Stages 5-6 (Cista installed-library path) reconstruct packages through
/// radix's `radix::hir::package::load_package` + faber `load_package_fhir`
/// (see `radix/docs/factory/hir-artifact-fhir/`), not this source-tree
/// wrapper — retained as the test-facing convenience surface.
#[allow(dead_code)]
pub(crate) fn load_package(
    spec: &PackageSpec,
    library_resolver: &LibraryResolver,
) -> Result<Vec<PackageFile>, Vec<Diagnostic>> {
    load_package_with_locale_pack(spec, library_resolver, None, false, None)
}

pub(crate) fn load_package_with_locale_pack(
    spec: &PackageSpec,
    library_resolver: &LibraryResolver,
    locale_pack: Option<&radix::locale::LocalePack>,
    include_proba: bool,
    proba_filter: Option<&TestSourceFilter>,
) -> Result<Vec<PackageFile>, Vec<Diagnostic>> {
    let manifest = match manifest::manifest_for_spec(spec) {
        Ok(manifest) => manifest,
        Err(diag) => return Err(vec![*diag]),
    };
    let source_root_for_filter = if spec.source_root.is_dir() {
        spec.source_root.clone()
    } else {
        spec.entry
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };
    let proba_allowed = |path: &Path| -> bool {
        if !is_proba_source_path(path) {
            return true;
        }
        match proba_filter {
            Some(filter) if !filter.is_empty() => filter.allows_path(&source_root_for_filter, path),
            _ => true,
        }
    };
    let initial_files = if spec.entry.is_dir() {
        let all = package_source_files(&spec.entry, include_proba)?;
        // When `faber test --include/--exclude` selects proba files, seed only
        // those tests. Product modules enter via local import edges, so a
        // focused math.proba run does not compile the whole library graph.
        let proba_only_seed =
            include_proba && proba_filter.is_some_and(|filter| !filter.is_empty());
        if proba_only_seed {
            all.into_iter()
                .filter(|path| is_proba_source_path(path) && proba_allowed(path))
                .collect::<Vec<_>>()
        } else {
            all.into_iter()
                .filter(|path| proba_allowed(path))
                .collect::<Vec<_>>()
        }
    } else {
        // Single-file entry: allow an explicit `.proba` path on the test path only.
        if is_proba_source_path(&spec.entry) && !include_proba {
            return Err(vec![crate::package_diagnostic_error(
                ".proba files are test sources; use `faber test` to load them",
            )
            .with_file(spec.entry.display().to_string())
            .with_arg("issue", "proba_source_build_forbidden")]);
        }
        // Product graph starts at the entry file. For `faber test`, also walk
        // `source_root` for `*.proba` so colocated test files are not orphaned
        // when nothing imports them (they must not be importable).
        let mut files = vec![spec.entry.clone()];
        if include_proba {
            for path in package_source_files(&source_root_for_filter, true)? {
                if is_proba_source_path(&path) && proba_allowed(&path) {
                    files.push(path);
                }
            }
        }
        files
    };
    let mut queue = VecDeque::from(initial_files);
    let mut seen = BTreeSet::new();
    let mut files = Vec::new();
    let mut diagnostics = Vec::new();

    while let Some(path) = queue.pop_front() {
        let canonical = normalize_path(&path);
        if !seen.insert(canonical.clone()) {
            continue;
        }

        let Some(loaded) =
            load_package_source(&canonical, manifest.as_ref(), locale_pack, &mut diagnostics)
        else {
            continue;
        };

        let mut library_imports = Vec::new();
        for stmt in &loaded.program.statements {
            let StmtKind::Import(decl) = &stmt.kind else {
                continue;
            };
            let import_path = loaded.interner.resolve(decl.path);
            match resolve_import(spec, library_resolver, &canonical, import_path) {
                ImportResolution::Local(target) => {
                    // Test sources may import product `.fab` modules; never the reverse.
                    if is_proba_source_path(&target) {
                        diagnostics.push(
                            crate::package_diagnostic_error(
                                ".proba files are test sources and cannot be imported; move shared helpers to a .fab module",
                            )
                            .with_file(canonical.display().to_string())
                            .with_span(decl.span)
                            .with_arg("issue", "proba_import_forbidden")
                            .with_arg("import", import_path.to_owned()),
                        );
                    } else {
                        queue.push_back(target);
                    }
                }
                ImportResolution::Library(module) => {
                    if let Some(binding) = library_import_binding(&loaded.interner, decl, module) {
                        library_imports.push(binding);
                    } else {
                        diagnostics.push(library_import_kind_diagnostic(
                            &canonical,
                            decl,
                            import_path,
                        ));
                    }
                }
                ImportResolution::Unsupported => {
                    diagnostics.push(import_unsupported_diagnostic(&canonical, decl, import_path));
                }
                ImportResolution::Error(diag) => {
                    diagnostics.push(diag.with_span(decl.span));
                }
            }
        }

        files.push(PackageFile {
            module_segments: module_segments_for_file(
                &spec.source_root,
                &canonical,
                loaded.frontmatter.as_ref(),
            ),
            path: canonical,
            raw_source: loaded.raw_source,
            source: loaded.body,
            frontmatter: loaded.frontmatter,
            program: loaded.program,
            interner: loaded.interner,
            library_imports,
            expanded_library_imports: Vec::new(),
        });
    }

    if diagnostics.iter().any(|diag| diag.is_error()) {
        Err(diagnostics)
    } else {
        let mut library_cache = LibraryInterfaceCache::default();
        for file in &mut files {
            file.expanded_library_imports = expand_library_imports(
                &file.library_imports,
                library_resolver,
                &mut library_cache,
                &mut diagnostics,
            );
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        diagnostics.extend(detect_import_cycles(spec, &files));
        if diagnostics.iter().any(|diag| diag.is_error()) {
            return Err(diagnostics);
        }
        Ok(files)
    }
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod frontmatter_integration_test;

#[cfg(test)]
mod proba_integration_test;

#[cfg(test)]
mod proba_stepper_test;

#[cfg(test)]
#[path = "../package_test.rs"]
mod tests;

#[cfg(test)]
#[path = "../package_text_contract_test.rs"]
mod text_contract_tests;
