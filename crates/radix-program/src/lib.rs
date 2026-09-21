//! Compiler-owned multi-file package compilation surface.
//!
//! `radix-program` is the whole-program layer consumed by the `radix` façade.
//! It composes the existing single-unit compiler into package analysis and
//! artifact assembly through module-level typed seams. The crate owns package
//! orchestration — never a second lexer/parser/typechecker, never global or
//! thread-local state, never a hidden output root.

use std::path::PathBuf;

#[cfg(any(feature = "mir-llvm", feature = "package-mir"))]
mod mir_canonicalize;

pub mod analyze;
pub mod artifact_plan;
#[cfg(feature = "hir-rust")]
pub mod codegen;
pub mod discovery;
pub mod emit;
#[cfg(feature = "hir-fhir")]
pub mod fhir;
pub mod file_interface;
pub mod frontmatter;
pub mod import_graph;
pub mod library;
#[cfg(feature = "mir-llvm")]
pub mod llvm;
pub mod lockfile;
pub mod manifest;
pub mod member_path;
#[cfg(feature = "package-mir")]
pub mod mir;
pub mod modules;
#[cfg(feature = "hir-fhir")]
pub mod nucleum;
pub mod paths;
#[cfg(feature = "hir-rust")]
pub mod rust_target;
pub mod source_files;
pub mod test_source_filter;
#[cfg(feature = "mir-wasm")]
pub mod wasm;

#[cfg(all(test, feature = "package-mir"))]
pub(crate) mod test_support;

pub(crate) const MANIFEST_FILE: &str = "faber.toml";

/// One loaded package source file.
///
/// `RP2c`: moved from `crates/faber/src/package/mod.rs` with the import-graph
/// and library surfaces that operate on it. Faber keeps constructing these
/// through its loader (via re-export) until RP3 moves multi-unit analysis.
pub struct PackageFile {
    pub path: PathBuf,
    pub module_segments: Vec<String>,
    /// Original on-disk source, including optional `+++` frontmatter.
    #[allow(dead_code)] // retained for future package inspect/diagnostic surfaces
    pub raw_source: String,
    /// Peeled Faber body used for parse spans and semantic analysis.
    pub source: String,
    /// Byte offset of `source` within [`PackageFile::raw_source`].
    pub body_byte_offset: u32,
    pub frontmatter: Option<radix_module::driver::FileFrontmatter>,
    /// Reader pack selected for this file before lexing. `None` is the
    /// compiler's Latin fallback when the product seam supplies no pack.
    pub locale_pack: Option<radix_module::locale::LocalePack>,
    pub program: radix_module::syntax::Program,
    pub interner: radix_module::lexer::Interner,
    /// Direct `norma:` (and future provider) imports declared in this package file.
    pub library_imports: Vec<LibraryImportBinding>,
    /// Transitive closure of [`PackageFile::library_imports`], dependencies
    /// first, deduped by identity.
    pub expanded_library_imports: Vec<LibraryImportBinding>,
}

/// One expanded library-import binding carried by a package file or analyzed
/// unit.
///
/// `RP2c`: moved from `crates/faber/src/package/mod.rs`; the import graph and
/// library expansion surfaces construct these, and faber's analysis/emission
/// halves consume them through the re-export.
#[derive(Clone)]
pub struct LibraryImportBinding {
    pub binding: String,
    /// Preserved for file-namespace re-export wiring.
    #[allow(dead_code)]
    pub visibility: radix_module::syntax::Visibility,
    pub import_span: radix_module::lexer::Span,
    pub module: crate::library::ResolvedLibraryModule,
}

/// Package diagnostics carry the same code as faber's package surface
/// (`PKG001`) so moved compiler diagnostics keep their identity.
pub(crate) fn package_diagnostic_error(
    message: impl Into<String>,
) -> radix_module::diagnostics::Diagnostic {
    radix_module::diagnostics::Diagnostic::error(message).with_code("PKG001")
}

#[cfg(test)]
#[path = "bounded_param_pkg001_test.rs"]
mod bounded_param_pkg001_test;

#[cfg(test)]
#[path = "analysis_entry_cross_pin_test.rs"]
mod analysis_entry_cross_pin_test;
