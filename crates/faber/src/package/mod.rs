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

pub mod binding;
mod binding_manifest;
#[cfg(feature = "hir-rust")]
mod binding_probe;
mod build_recipe;
#[cfg(feature = "hir-rust")]
mod cargo;
#[cfg(feature = "mir-clif")]
pub(crate) mod clif_build;
#[cfg(all(feature = "mir-clif", feature = "runner"))]
mod clif_jit;
mod cmd;
pub(crate) mod composite_plan;
#[cfg(feature = "device-runtime")]
mod device;
mod dispatch;
mod embedded_locale;
pub(crate) mod embedded_runtime;
#[cfg(feature = "hir-fhir")]
mod fhir;
#[cfg(feature = "hir-go")]
mod go_build;
#[cfg(feature = "hir-go")]
mod go_recipe;
#[cfg(feature = "device-runtime")]
mod host_factory;
#[cfg(feature = "hir-rust")]
mod library_link;
#[cfg(feature = "mir-llvm")]
mod llvm_host;
mod locale;
mod manifest;
pub mod mir;
#[cfg(test)]
// WHY: packed-projection seam has no production caller yet; exercised by packed_projection_test.rs
mod packed_projection;
#[cfg(feature = "hir-ts")]
mod product;
#[cfg(feature = "hir-rust")]
mod runtime_dependency;
#[cfg(any(feature = "hir-rust", feature = "hir-go", feature = "hir-ts"))]
mod runtime_sources;
#[cfg(feature = "hir-rust")]
mod rust_runtime_dependency;
pub mod target_gate;
mod temp_dir;
mod test_route;
// The public export removes the TypeScript backend recipe and runtime, so its
// implementation (and its unit tests) must follow the optional target feature.
#[cfg(feature = "hir-ts")]
mod ts_build;
#[cfg(feature = "hir-ts")]
mod ts_package;
#[cfg(feature = "hir-ts")]
mod ts_project;
#[cfg(feature = "hir-ts")]
mod ts_recipe;
#[cfg(feature = "hir-ts")]
mod ts_runtime;

#[cfg(feature = "hir-rust")]
#[cfg(test)]
pub(crate) use binding::verify_library_binding_shapes;
pub use binding::{load_target_bindings, verify_library_bindings};
#[cfg(feature = "hir-rust")]
pub(crate) use cargo::{
    RustRuntimePlan, package_host_selection_diagnostic, rust_runtime_plan_for_package,
};
#[cfg(feature = "hir-rust")]
#[cfg(test)]
pub(crate) use cargo::{
    emit_generated_crate_with_runtime_plan, invoke_cargo_build, invoke_cargo_build_library,
};
// S09-4 (register row, docs/audits/radix-defect-audit-2026-09-20): the
// retired Cargo test executor stays exported for its deletion unit only —
// `invoke_cargo_test` has no consumer in bin, lib, or tests (body retained,
// deletion is S09-4's fix); `emit_generated_crate` is consumed only by
// `package_test.rs`, hence the cfg(test) gate.
#[cfg(feature = "hir-rust")]
#[cfg(test)]
pub use cargo::emit_generated_crate;
#[cfg(all(feature = "mir-clif", feature = "runner"))]
pub use clif_jit::{cmd_run_clif_jit, run_default_engine_or_decline};
#[cfg(feature = "mir-clif")]
pub use cmd::compile_clif_package_input;
pub use cmd::{
    CHECKS_BUT_DOES_NOT_LOWER, DeviceAdviceRequest, cmd_build, cmd_check_package, cmd_emit_package,
    should_treat_as_package_from_args, use_package_compiler, use_package_compiler_from_args,
};
#[cfg(feature = "hir-go")]
pub use go_build::go_program::cmd_run_go;
#[cfg(feature = "hir-go")]
#[cfg(test)]
pub(crate) use go_build::{GoBuildLayout, emit_go_module, invoke_go_build_default};
#[cfg(feature = "mir-llvm")]
pub use llvm_host::{
    LlvmHostBuild, LlvmHostProfile, build_host_program, build_host_program_in, compile_nvvm_to_ptx,
    discover_llvm_host_toolchain, ensure_llvm_runtime_archive, ensure_llvm_runtime_archive_in,
    host_llvm_target_triple, host_triple_for, llvm_program::cmd_run_llvm_host,
};
pub(crate) use manifest::manifest_device_inputs;
pub(crate) use manifest::manifest_locale;
pub(crate) use manifest::validate_manifest;
pub use manifest::{
    DeviceChoice, FaberManifest, ManifestProduct, ManifestProductEmit, ManifestProductKind,
    ManifestProductShaders, manifest_package_build_target as manifest_build_target_for_input,
    read_manifest, resolve_format_width, resolve_safe_kernels,
};
#[cfg(feature = "runner")]
pub use manifest::{
    manifest_device_choice, resolve_cpu_threads, resolve_cuda_libdevice, resolve_device_selection,
};
#[cfg(feature = "runner")]
#[cfg(feature = "runner")]
pub use mir::with_lowered_package_mir;
#[cfg(feature = "hir-ts")]
pub(crate) use product::build_browser_product_with_postprocess;
#[cfg(test)]
#[cfg(feature = "hir-ts")]
pub(crate) use product::{build_browser_product, build_browser_product_static_assets};
use radix::module::codegen::Target;
use radix::module::diagnostics::Diagnostic;
use radix::module::driver::Config;
use radix::program::discovery::PackageSpec;
use radix::program::library::{LibraryInterfaceCache, LibraryResolver, LockedLibraryPackage};
use std::path::{Path, PathBuf};
pub(crate) use temp_dir::unique_temp_sibling;
pub use test_route::{analyze_package_for_tests, test_source_filter_for_input};
#[cfg(feature = "hir-ts")]
pub use ts_build::cmd_run_ts;

/// Materialized repository-shaped root for the runtime embedded in this binary.
///
/// The `faber` binary links this crate as `faber_cli`, so its command handlers
/// cannot see `pub(crate)` items; this is the cross-crate seam for corpus
/// suites that build against the embedded runtime.
#[must_use]
pub fn embedded_runtime_root() -> Option<PathBuf> {
    embedded_runtime::embedded_runtime_root()
}

/// ONE-EFFECTIVE-TARGET SEAM — executed lane (TARGETLANE001 routing).
/// BUG-F585D081: all manifest-backed routes share the manifest's target,
/// including its implicit default; caller defaults apply only without a manifest.
///
/// Resolve the effective analysis target for an executed package:
///   1. **Explicit MIR-backed config target** — authoritative and never
///      rerouted. `faber run` no longer carries a CLI `-t` (interpreted
///      execution, always), so the config target arrives from callers that
///      build their own config; a non-MIR-backed config target here means
///      "no executed target chosen".
///   2. **Manifest `[build] target`** — propagated to the analysis target when
///      the caller did not name an executed target. The package's declared
///      target is the ABI source of truth for analysis and execution alike;
///      this propagation is the routing fix that stops `TARGETLANE001`
///      firing at analysis on the executed path.
///   3. **Executed-lane default `fmir`** (`Target::MirFmirBinary`) only when no
///      manifest exists. An existing manifest owns its implicit portable
///      default (`fhir`) as well as any explicitly declared target.
///
/// The seam is single-valued and never ambient: analysis and execution resolve
/// from the same source, so the two targets cannot diverge (ABI source-of-
/// truth invariant). The radix-side TARGETLANE001 guard is unchanged — this
/// only fixes the executed route's target selection.
///
/// # Errors
/// A manifest naming an unreadable or unsupported target is an error, never a
/// fallback to the default: `faber check`, `build` and `test` agree on it.
pub fn executed_lane_analysis_target(
    config: &Config,
    input: &Path,
) -> Result<Target, Box<Diagnostic>> {
    let manifest_target = manifest_build_target_for_input(input)?;
    if is_mir_backed_target(config.target) {
        return Ok(config.target);
    }
    match manifest_target {
        Some(target) => Ok(target),
        None => Ok(Target::MirFmirBinary),
    }
}

/// Whether a compiler target is MIR-backed (the executed-lane positive set;
/// the mirror of the radix `TARGETLANE001` guard's HIR-direct reject list in
/// `crates/radix/src/driver/mod.rs`). `HirFhir` is deliberately excluded: it
/// is a portable HIR artifact surface, not a MIR-backed executed target, and
/// the executed lane resolves to `fmir`, not `fhir`.
fn is_mir_backed_target(target: Target) -> bool {
    matches!(
        target,
        Target::MirStepper
            | Target::MirWasm
            | Target::MirLlvm
            | Target::MirLlvmHost
            | Target::MirClif
            | Target::MirMetal
            | Target::MirWgsl
            | Target::MirSexp
            | Target::MirBasic
            | Target::MirScena
            | Target::MirFmir
            | Target::MirFmirBinary
    )
}

pub(crate) use embedded_locale::{EMBEDDED_LOCALE_IDS, load_embedded_locale_pack};
pub use locale::available_locales;
pub use locale::embedded_locale_pack_loader;
pub use locale::locale_pack_for_emit;
pub use locale::{
    DEFAULT_CODE_LOCALE, config_with_locale, default_config_with_locale,
    embedded_conversion_locale_pack, load_locale_pack_for_input,
    locale_pack_for_emit_with_diagnostic,
};

pub(super) const MANIFEST_FILE: &str = "faber.toml";

fn resolution_base() -> std::path::PathBuf {
    radix::program::paths::normalize_path(
        &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    )
}

pub fn missing_package_path_diagnostic(path: &Path) -> Box<Diagnostic> {
    let path = absolutize_path(path);
    let base = resolution_base();
    let error = std::io::Error::from_raw_os_error(2);
    Box::new(
        crate::package_diagnostic_error(format!(
            "cannot read manifest at '{}' resolved against '{}': {error}",
            path.display(),
            base.display()
        ))
        .with_phase(radix::module::diagnostics::DiagnosticPhase::Io)
        .with_file(path.display().to_string())
        .with_arg("issue", "package_path_missing")
        .with_arg("path", path.display().to_string())
        .with_arg("base", base.display().to_string())
        .with_arg("artifact", "manifest"),
    )
}

/// Attach path facts to a package I/O diagnostic whose lower-level loader
/// could not carry them across the product seam (currently locale-pack reads).
///
/// The package code and issue remain owned by the originating diagnostic. This
/// adapter only fills the stable path/base/artifact facts at the CLI boundary.
pub fn attach_path_diagnostic_context(mut diagnostic: Box<Diagnostic>) -> Box<Diagnostic> {
    if ["path", "base", "artifact"]
        .iter()
        .all(|name| diagnostic.args.iter().any(|arg| arg.name == *name))
    {
        return diagnostic;
    }

    let Some(pack_path) = locale_pack_path_from_diagnostic(&diagnostic.message) else {
        return diagnostic;
    };
    let base = pack_path.parent().unwrap_or_else(|| Path::new("."));
    let read_marker = ": cannot read reader pack '";
    if let Some(read_start) = diagnostic.message.find(read_marker) {
        let read_start = read_start + read_marker.len();
        if let Some(error_start) = diagnostic.message[read_start..].find("': ") {
            let error_start = read_start + error_start + 3;
            diagnostic.message = format!(
                "cannot read locale pack '{}' resolved against '{}': {}",
                pack_path.display(),
                base.display(),
                &diagnostic.message[error_start..]
            );
        }
    }
    diagnostic.file = pack_path.display().to_string();
    diagnostic
        .args
        .push(radix::module::diagnostics::DiagnosticArg::new(
            "issue",
            "locale_pack_missing",
        ));
    diagnostic
        .args
        .push(radix::module::diagnostics::DiagnosticArg::new(
            "path",
            pack_path.display().to_string(),
        ));
    diagnostic
        .args
        .push(radix::module::diagnostics::DiagnosticArg::new(
            "base",
            base.display().to_string(),
        ));
    diagnostic
        .args
        .push(radix::module::diagnostics::DiagnosticArg::new(
            "artifact",
            "locale pack",
        ));
    diagnostic
}

fn locale_pack_path_from_diagnostic(message: &str) -> Option<PathBuf> {
    if !message.contains("cannot read reader pack") {
        return None;
    }
    let marker = " pack '";
    let start = message.find(marker)? + marker.len();
    let end = message[start..].find("':")? + start;
    Some(PathBuf::from(&message[start..end]))
}

/// Resolve a product input against the caller's working directory before
/// entering the explicit-input compiler API. Ambient CWD lookup is a Faber
/// command concern, not a radix-program concern.
///
/// `faber test` shares this seam because package discovery requires absolute
/// inputs: a raw `.` lexically normalizes to an empty path and fails as
/// `cannot read '.'` (rdx-s05-1).
pub fn absolutize_path(path: &Path) -> std::path::PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        resolution_base().join(path)
    };
    radix::program::paths::normalize_path(&path)
}

/// Discover the package containing `input`, reading and validating its manifest
/// when it is manifest-backed.
///
/// # Errors
///
/// Returns the discovery diagnostic when the package cannot be located, or the
/// manifest diagnostic when a present manifest cannot be read or validated.
pub fn discover_package(input: &Path) -> Result<PackageSpec, Box<Diagnostic>> {
    let path = absolutize_path(input);
    let spec = radix::program::discovery::discover_package_with_base(&path, &resolution_base())?;
    if spec.manifest_backed {
        let manifest_path = spec.package_root.join(MANIFEST_FILE);
        if manifest_path.exists() {
            let manifest = read_manifest(&manifest_path)?;
            validate_manifest(&manifest, &manifest_path)?;
        }
    }
    Ok(spec)
}

/// `faber test` execution route selected from the input path.
///
/// Used by the `faber` binary `cmd_test` through the `faber_cli` lib seam and
/// by package-route tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestInputRoute {
    /// Bare single-file MIR runner (no package graph).
    SingleFile,
    /// Package analysis + linking, including a single file under a package root.
    Package,
    /// Directory with no `faber.toml` — guiding diagnostic, not `main.fab` fallback.
    ManifestLessDirectory,
}

/// Guiding diagnostic for E1: `faber test .` in a library tree without a manifest.
pub const MANIFEST_LESS_TEST_MESSAGE: &str =
    "no faber.toml found; testing a library requires a package root";

fn is_test_source_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "fab" || ext == "proba")
}

/// Select the `faber test` route for `path`.
///
/// A single `.fab`/`.proba` under a discoverable package root uses package
/// analysis so in-package imports resolve. `--no-package` keeps the bare
/// single-file route. A directory without `faber.toml` is rejected with
/// [`MANIFEST_LESS_TEST_MESSAGE`] instead of the legacy `main.fab` fallback.
pub fn test_input_route(path: &Path, no_package: bool) -> TestInputRoute {
    let path = absolutize_path(path);
    if is_test_source_file(&path) && no_package {
        return TestInputRoute::SingleFile;
    }
    match discover_package(&path) {
        Ok(spec) if spec.manifest_backed => TestInputRoute::Package,
        Ok(_) if path.is_dir() => TestInputRoute::ManifestLessDirectory,
        _ if is_test_source_file(&path) => TestInputRoute::SingleFile,
        _ => TestInputRoute::Package,
    }
}

/// Discover the build layout for `input`.
///
/// # Errors
///
/// Returns the discovery diagnostic when the layout cannot be determined.
pub fn discover_build_layout(
    input: &Path,
) -> Result<radix::program::discovery::BuildLayout, Box<Diagnostic>> {
    radix::program::discovery::discover_build_layout(&absolutize_path(input))
}

pub fn is_manifest_backed_or_directory_package_input(input: &Path) -> bool {
    radix::program::discovery::is_manifest_backed_or_directory_package_input(&absolutize_path(
        input,
    ))
}

/// Whether a manifestless `.fab` file declares an import outside the kernel
/// (`faber:`) and outside `norma:`. Such a file is package-shaped: a lone-file
/// compile keeps the imported definitions external (the `clif` object emitter
/// declines them as CODEGEN001 `external definition call`; the text targets
/// emit unlinked namespaces), so the routes that must produce runnable output
/// take the package machinery instead — `faber run` interprets it through the
/// package-MIR runner, and the `clif` product lanes compile it through the
/// package-MIR emitter.
pub fn manifestless_file_declares_non_kernel_import(path: &Path) -> bool {
    if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("fab") {
        return false;
    }
    let Ok(raw_source) = std::fs::read_to_string(path) else {
        return false;
    };
    let display_name = path.display().to_string();
    let Ok(peeled) = radix::module::driver::peel_raw_source(&display_name, &raw_source) else {
        return false;
    };
    // The no-locale default is `en`; a default pack that fails to load is a
    // failed read, not a Latin fallback.
    let Some(code_pack) = default_config_with_locale(Target::HirTypeScript)
        .ok()
        .and_then(|config| config.locale_pack)
    else {
        return false;
    };
    let lex = radix::module::lexer::lex_with_locale_pack(peeled.body, &code_pack);
    let parse = radix::module::parser::parse(lex);
    let Some(program) = parse.program else {
        return false;
    };
    program.statements.iter().any(|statement| {
        let radix::module::syntax::StmtKind::Import(import) = &statement.kind else {
            return false;
        };
        let path = parse.interner.resolve(import.path);
        !radix::module::kernel::is_kernel_import_path(path) && !path.starts_with("norma:")
    })
}

// Test probe: how many times this thread built the package-preparation
// library cache. Every package preparation builds it once per MIR link, so the
// count measures "how many times was the package prepared".
#[cfg(test)]
thread_local! {
    pub(crate) static LIBRARY_CACHE_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Build the explicit library cache seam used by product MIR route plumbing.
pub(crate) fn library_cache_with_config(config: &Config) -> LibraryInterfaceCache {
    #[cfg(test)]
    LIBRARY_CACHE_BUILDS.with(|builds| builds.set(builds.get() + 1));
    LibraryInterfaceCache::with_config(config).with_locale_loader(load_locale_pack_for_input)
}

#[cfg(feature = "hir-rust")]
pub(crate) fn library_cache_default() -> LibraryInterfaceCache {
    LibraryInterfaceCache::default().with_locale_loader(load_locale_pack_for_input)
}

pub(crate) fn library_resolver_from_config(
    config: &radix::module::driver::Config,
) -> LibraryResolver {
    // The library home (provider repos such as `norma/` and `triga/`) is
    // independent of the language-pack root (`locale/<id>/pack.toml`).
    // Precedence: an explicit `library_home` on Config (`--library-home` or
    // `[paths] library_home`); otherwise the workspace probe locates provider
    // repos.
    // The ambient probe is faber-owned (CTO memo); `radix-program`
    // receives the resolved home as an explicit input.
    if let Some(path) = config.library_home.as_ref() {
        return LibraryResolver::new(path.clone());
    }
    crate::library::default_library_resolver()
}

/// S-D3: lock rows the manifest does not declare must not enter the
/// resolver — an undeclared row would make its absolute interface root
/// importable. Platform-default providers are the producer-sanctioned
/// exception (`cista/src/commands/install.rs` `PLATFORM_DEFAULT_PACKAGES`
/// rewrites their lock rows without a `[dependencies]` entry). Absolute
/// store paths are lock contract (`faber-lock.md` fields 4/8/9/10), never
/// a rejection case.
const PLATFORM_DEFAULT_PROVIDERS: &[&str] = &["norma"];

/// Build a library resolver for a package root, attaching `faber.toml`
/// dependencies and `faber.lock` interface roots when present.
///
/// # Errors
///
/// Returns the diagnostics when the manifest or lockfile cannot be read, when
/// the lock fails dependency validation, or when the lock index cannot be built.
pub fn library_resolver_for_package(
    config: &radix::module::driver::Config,
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
    let lock = match radix::program::lockfile::read_lock(package_root) {
        Ok(lock) => lock,
        Err(diag) => return Err(vec![*diag]),
    };
    let lock_diags = radix::program::lockfile::validate_dependencies_against_lock(
        package_root,
        &manifest.dependencies,
        lock.as_ref(),
    );
    if lock_diags.iter().any(Diagnostic::is_error) {
        return Err(lock_diags);
    }

    let mut locked = std::collections::BTreeMap::new();
    if let Some(lock) = lock.as_ref() {
        let lock_index = radix::program::lockfile::lock_index(
            &package_root.join(radix::program::lockfile::LOCK_FILE),
            lock,
        )?;
        for package in lock_index.values() {
            let declared = manifest.dependencies.contains_key(&package.name)
                || PLATFORM_DEFAULT_PROVIDERS.contains(&package.name.as_str());
            if !declared {
                continue;
            }
            locked.insert(
                package.name.clone(),
                LockedLibraryPackage {
                    name: package.name.clone(),
                    version: package.version.clone(),
                    interface_root: package.interface_root_path_for(package_root),
                },
            );
        }
    }
    // A package that declares `[library] provider = "X"` is itself the
    // provider repo for `X:*` self-imports (faber-web imports `web:dom` from
    // its own source). Resolve them against the package's own source root so
    // a provider repo whose checkout name differs from its provider name
    // (`faber-web` vs `web`) still analyzes; a lock entry always wins.
    if let Some(library) = manifest.library.as_ref() {
        locked
            .entry(library.provider.clone())
            .or_insert_with(|| LockedLibraryPackage {
                name: manifest.package.name.clone(),
                version: manifest.package.version.clone(),
                interface_root: package_root.join(&manifest.paths.source),
            });
    }
    resolver = resolver.with_package_lock(manifest.dependencies, locked);
    Ok(resolver)
}

// RP3: multi-unit analysis lives in `radix-program` (`analyze.rs`). These
// adapters supply the faber product-orchestration seams — reader-locale pack
// loading, strict product-view manifest validation (FBR-P1-001), and
// workspace probing — while keeping the faber-internal
// entry signatures.
pub fn faber_analysis_seams() -> radix::program::analyze::AnalysisSeams {
    radix::program::analyze::AnalysisSeams {
        locale_loader: load_locale_pack_for_input,
        manifest_validator: validate_product_manifest_for_spec,
        resolver_for_package: library_resolver_for_package,
    }
}

/// Faber-supplied package-emission seams. Go source assembly remains a product
/// helper because it owns the generated multi-file package shape; the shared
/// dispatch and Rust assembly live in `radix-program`.
pub fn faber_emission_seams() -> radix::program::emit::EmissionSeams {
    radix::program::emit::EmissionSeams {
        analysis: faber_analysis_seams(),
        #[cfg(feature = "hir-go")]
        emit_go: go_build::generate_package_go_result,
        #[cfg(feature = "hir-ts")]
        emit_ts: ts_package::generate_package_ts_result,
    }
}

fn validate_product_manifest_for_spec(spec: &PackageSpec) -> Result<(), Box<Diagnostic>> {
    manifest::manifest_for_spec(spec).map(|_| ())
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod library_resolver_lock_test;

#[cfg(test)]
mod frontmatter_integration_test;

#[cfg(test)]
mod proba_integration_test;

#[cfg(test)]
mod proba_runner_test;

#[cfg(test)]
#[path = "../package_test.rs"]
mod tests;

#[cfg(test)]
#[path = "../package_text_contract_test.rs"]
mod text_contract_tests;

// DDPP1-U2 (C2 feature isolation): the small-build surface proof — these tests
// compile and run only without `device-runtime` (the small build; the default
// build includes it), exercising the CPU-only route-selection plumbing in the
// build that excludes the device runtime.
#[cfg(all(test, not(feature = "device-runtime")))]
#[path = "feature_isolation_test.rs"]
mod feature_isolation_test;
