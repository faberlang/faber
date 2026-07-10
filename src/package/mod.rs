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

pub mod binding;
mod binding_probe;
mod cargo;
mod cmd;
mod codegen;
mod compile;
mod discovery;
mod file_interface;
mod frontmatter;
mod import_graph;
mod library;
mod lockfile;
mod manifest;
mod member_path;
mod mir;
mod modules;
mod paths;
mod reader;
mod source_files;

pub use binding::verify_library_bindings;
#[allow(unused_imports)]
// used by `commands/run.rs` and `commands/test.rs` in the binary crate
pub(crate) use cargo::invoke_cargo_build;
pub use cargo::{emit_generated_crate, invoke_cargo_test};
#[cfg(test)]
pub use cmd::use_package_compiler;
pub use cmd::{
    cmd_build, cmd_check_package, cmd_emit_package, should_treat_as_package_from_args,
    use_package_compiler_from_args,
};
#[allow(unused_imports)] // package MIR stages consume this crate-visible analysis API.
pub(crate) use compile::{analyze_package, AnalyzedPackage, AnalyzedPackageUnit};
pub use compile::{check_package, compile_package, compile_package_with_test_selection};
#[allow(unused_imports)] // public package API; used by integration tests and external callers
pub use discovery::{discover_build_layout, sanitize_crate_name, BuildLayout};
pub(crate) use manifest::validate_manifest;
#[allow(unused_imports)] // public package API; used by integration tests and external callers
pub use manifest::{
    read_manifest, FaberManifest, ManifestBuild, ManifestLibrary, ManifestPackage, ManifestPaths,
    ManifestRustFieldNames,
};
// binary-only package interpretation route consumes this through `commands`.
#[allow(unused_imports)] // generated fmir-bin runner crates consume this public API.
pub use mir::run_fmir_image_bytes_with_stdio;
#[cfg(test)]
pub(super) use mir::test_support::{fmir_image_test_summary, fmir_text_image_test_summary};
#[allow(unused_imports)] // FMIR stages consume this crate-visible image API.
pub(crate) use mir::{
    build_package_fmir_binary_bundle, build_package_fmir_image, build_package_fmir_text_image,
    run_fmir_image_path, run_package_fmir_image, run_package_fmir_text_image,
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
use std::path::PathBuf;

pub(crate) use discovery::PackageSpec;
use frontmatter::manifest_path_for_spec;
use import_graph::{
    detect_import_cycles, import_unsupported_diagnostic, library_import_binding,
    library_import_kind_diagnostic, resolve_import, ImportResolution,
};
use library::expand_library_imports;
pub(crate) use member_path::resolve_package_member;
use modules::module_segments_for_file;
use paths::normalize_path;
use source_files::{load_package_source, package_source_files};

pub(crate) use library::{
    analysis_source_for_file, attach_library_provenance, library_cached_analysis,
    library_cached_file_interface, library_generates_rust_module, library_imported_function_params,
    library_interface_export_names, library_interface_has_module, library_module_segments,
    program_export_names, LibraryInterfaceCache,
};
pub(crate) use reader::{config_with_reader_locale, load_reader_pack_for_input};

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
    config
        .stdlib_path
        .as_ref()
        .map(|path| LibraryResolver::new(path.clone()))
        .unwrap_or_else(LibraryResolver::default)
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
        for package in &lock.packages {
            locked.insert(
                package.name.clone(),
                crate::library::LockedLibraryPackage {
                    name: package.name.clone(),
                    version: package.version.clone(),
                    interface_root: package.interface_root_path(),
                },
            );
        }
    }
    resolver = resolver.with_package_lock(manifest.dependencies, locked);
    Ok(resolver)
}

pub(crate) fn load_package(
    spec: &PackageSpec,
    library_resolver: &LibraryResolver,
) -> Result<Vec<PackageFile>, Vec<Diagnostic>> {
    load_package_with_reader_pack(spec, library_resolver, None)
}

pub(crate) fn load_package_with_reader_pack(
    spec: &PackageSpec,
    library_resolver: &LibraryResolver,
    reader_pack: Option<&radix::reader_locale::ReaderLocalePack>,
) -> Result<Vec<PackageFile>, Vec<Diagnostic>> {
    let manifest = manifest_path_for_spec(spec).and_then(|path| read_manifest(&path).ok());
    let initial_files = if spec.entry.is_dir() {
        package_source_files(&spec.entry)?
    } else {
        vec![spec.entry.clone()]
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
            load_package_source(&canonical, manifest.as_ref(), reader_pack, &mut diagnostics)
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
                ImportResolution::Local(target) => queue.push_back(target),
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
#[path = "../package_test.rs"]
mod tests;

#[cfg(test)]
#[path = "../package_text_contract_test.rs"]
mod text_contract_tests;
