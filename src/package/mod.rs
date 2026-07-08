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

mod cargo;
mod cmd;
mod codegen;
mod compile;
mod discovery;
mod file_interface;
mod frontmatter;
mod import_graph;
mod library;
mod manifest;
mod mir;
mod modules;
mod paths;
mod reader;

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
#[allow(unused_imports)] // public package API; used by integration tests and external callers
pub use manifest::{
    read_manifest, FaberManifest, ManifestBuild, ManifestPackage, ManifestPaths,
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
use radix::driver::{peel_raw_source, source_load_diagnostic, FileFrontmatter};
use radix::lexer::Interner;
use radix::parser;
use radix::syntax::{Program, StmtKind, Visibility};
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::PathBuf;

pub(crate) use discovery::PackageSpec;
use frontmatter::{manifest_path_for_spec, validate_frontmatter_against_manifest};
use import_graph::{
    detect_import_cycles, import_unsupported_diagnostic, library_import_binding,
    library_import_kind_diagnostic, resolve_import, ImportResolution,
};
use library::expand_library_imports;
use modules::module_segments_for_file;
use paths::normalize_path;

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

struct LoadedPackageSource {
    raw_source: String,
    body: String,
    frontmatter: Option<FileFrontmatter>,
    program: Program,
    interner: Interner,
}

pub(crate) fn library_resolver_from_config(config: &radix::driver::Config) -> LibraryResolver {
    config
        .stdlib_path
        .as_ref()
        .map(|path| LibraryResolver::new(path.clone()))
        .unwrap_or_else(LibraryResolver::default)
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
    let mut queue = VecDeque::from([spec.entry.clone()]);
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

fn load_package_source(
    canonical: &PathBuf,
    manifest: Option<&FaberManifest>,
    reader_pack: Option<&radix::reader_locale::ReaderLocalePack>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<LoadedPackageSource> {
    let raw_source = match fs::read_to_string(canonical) {
        Ok(source) => source,
        Err(err) => {
            diagnostics.push(Diagnostic::io_error(canonical, err));
            return None;
        }
    };

    let display_name = canonical.display().to_string();
    let peeled = match peel_raw_source(&display_name, &raw_source) {
        Ok(peeled) => peeled,
        Err(error) => {
            diagnostics.push(source_load_diagnostic(&display_name, error));
            return None;
        }
    };

    if let Some(manifest) = manifest {
        if let Some(diag) =
            validate_frontmatter_against_manifest(canonical, peeled.frontmatter.as_ref(), manifest)
        {
            diagnostics.push(diag);
            return None;
        }
    }

    let radix::driver::PeeledSource {
        body, frontmatter, ..
    } = peeled;
    let body = body.to_owned();

    let lex_result = match reader_pack {
        Some(pack) => radix::lexer::lex_with_reader_pack(&body, pack),
        None => radix::lexer::lex(&body),
    };
    diagnostics.extend(
        lex_result.reader_fallbacks.iter().map(|fallback| {
            Diagnostic::from_reader_locale_fallback(&display_name, &body, fallback)
        }),
    );
    diagnostics.extend(lex_result.reader_suggestions.iter().map(|suggestion| {
        Diagnostic::from_reader_locale_suggestion(&display_name, &body, suggestion)
    }));
    if !lex_result.success() {
        diagnostics.extend(
            lex_result
                .errors
                .iter()
                .map(|err| Diagnostic::from_lex_error(&display_name, &body, err)),
        );
        return None;
    }

    let parse = parser::parse(lex_result);
    if !parse.success() {
        diagnostics.extend(
            parse
                .errors
                .iter()
                .map(|err| Diagnostic::from_parse_error(&display_name, &body, err)),
        );
        return None;
    }

    let radix::parser::ParseResult {
        program, interner, ..
    } = parse;
    let Some(program) = program else {
        diagnostics.push(
            Diagnostic::error("successful package parse result missing program")
                .with_file(canonical.display().to_string()),
        );
        return None;
    };

    Some(LoadedPackageSource {
        raw_source,
        body,
        frontmatter,
        program,
        interner,
    })
}

#[cfg(test)]
#[path = "../package_test.rs"]
mod tests;

#[cfg(test)]
#[path = "../package_text_contract_test.rs"]
mod text_contract_tests;
