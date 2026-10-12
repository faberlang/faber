//! Compiler-owned multi-file package compilation surface.
//!
//! `radix-program` is the whole-program layer consumed by the `radix` façade.
//! It composes the existing single-unit compiler into package analysis and
//! artifact assembly through module-level typed seams. The crate owns package
//! orchestration — never a second lexer/parser/typechecker, never a hidden
//! output root — and its own modules own no global or thread-local state
//! (ARCH-2 Ruling 5 / B6-A5).
//!
//! # Session-memo register
//!
//! `radix-program` depends on the whole `radix-module` monolith, so while that
//! edge exists this charter promises no absence of transitive thread-locals.
//! What it records is the memo set of the program-phase helpers — the
//! compilation surfaces physically homed in `radix-module` (cycle law:
//! `radix-module` cannot depend on `radix-program`) that package analysis
//! reaches through. Every entry carries its key and its invalidation rule.
//!
//! Driver-run memos — cells on `radix_module::driver::Session`'s
//! `SessionMemos` (ARCH-U3 / S01-9), shared across `Session` re-wraps;
//! invalidation is dropping the last `Rc` handle, so nothing outlives the
//! driver run that filled it:
//!
//! - `import_targets` (cell declared in `radix-module/src/driver/session.rs`,
//!   key type from `radix-module/src/program/compile.rs`): parse-once memo for
//!   import targets, keyed by `(reader-pack id, target path)`.
//! - `package_roots` (cell declared in `radix-module/src/driver/session.rs`,
//!   filled by `find_package_root_with_session` in
//!   `radix-module/src/import_resolve.rs`): package-root ancestor-walk memo,
//!   keyed by the directory that started the walk; every intermediate
//!   directory visited is filled, so later imports from the same package pay
//!   O(1). The session-less `find_package_root` is the unmemoized discovery
//!   form.
//!
//! Process-wide memos — `OnceLock`s in `radix-module/src/import_resolve.rs`,
//! environment-keyed with honest process scope: neither the environment nor
//! the installed library layout changes during a compilation session, so
//! production never invalidates them (override/clearing is test-only):
//!
//! - `LIBRARY_HOME_CACHE`: `FABER_LIBRARY_HOME` env var plus the workspace
//!   ancestor walk → the library home path.
//! - `PROVIDER_NAME_MEMO`: library home path → provider segment → provider
//!   repo directory.
//!
//! History — ARCH-2 Ruling 5 originally recorded five memos. Three
//! thread-locals were retired by defect-audit batch 6:
//! `IMPORTED_DEVICE_INTERFACES` (device interfaces now ride the per-unit
//! identity carrier, so the map's lifetime is the unit's — cross-package
//! service is impossible by construction; B6-U2, `6203c62ca`) and
//! `IMPORT_TARGET_CACHE` / `PACKAGE_ROOT_CACHE` (absorbed into `SessionMemos`;
//! B6-U3, `2918d57f4`). The two entries in each group above are the surviving
//! set this register is responsible for.

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
#[cfg(any(feature = "mir-wasm", feature = "mir-llvm"))]
pub(crate) mod leaf_gate;
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
pub mod praefixum_imports;
pub mod route_table;
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
    /// Import path → target `regio` name for this file's default-named imports
    /// (D7.7); already applied to [`PackageFile::program`] and handed to the
    /// driver's import contract so the unit's own parse binds the same names.
    pub default_import_names: std::collections::BTreeMap<String, String>,
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
