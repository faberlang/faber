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
#[cfg(feature = "hir-rust")]
mod binding_probe;
mod cargo;
mod cmd;
pub(crate) mod composite_plan;
mod dense_full_model_device;
#[cfg(feature = "device-runtime")]
mod device;
mod dispatch;
#[cfg(feature = "hir-fhir")]
mod fhir;
#[cfg(feature = "hir-go")]
mod go_build;
#[cfg(all(feature = "device-runtime", feature = "host-macos-arm64"))]
mod host_factory;
pub(crate) mod host_spawn;
#[cfg_attr(not(feature = "hir-rust"), path = "library_link_unavailable.rs")]
mod library_link;
#[cfg(feature = "mir-llvm")]
mod llvm_host;
mod locale;
mod manifest;
pub(crate) mod mir;
mod packed_projection;
mod product;
mod runtime_dependency;
mod runtime_sources;
#[cfg(feature = "hir-rust")]
mod rust_runtime_dependency;

#[cfg(feature = "hir-rust")]
#[allow(unused_imports)]
pub(crate) use binding::verify_library_binding_shapes;
#[allow(unused_imports)]
pub use binding::{
    verify_library_bindings, verify_library_bindings_with_probe_mode, BindingProbeMode,
};
#[allow(unused_imports)]
pub(crate) use cargo::emit_generated_crate_with_runtime_plan;
#[allow(unused_imports)]
pub(crate) use cargo::invoke_cargo_build;
pub use cargo::{emit_generated_crate, invoke_cargo_test};
pub(crate) use cargo::{
    package_host_selection_diagnostic, rust_runtime_plan_for_package, RustRuntimePlan,
};
pub use cmd::{
    cmd_build, cmd_check_package, cmd_emit_package, should_treat_as_package_from_args,
    use_package_compiler_from_args,
};
#[cfg(all(feature = "device-runtime", feature = "host-macos-arm64"))]
#[allow(unused_imports)]
pub(crate) use device::execute_device_route;
#[cfg(feature = "device-runtime")]
#[allow(unused_imports)]
pub(crate) use device::host_partition_for_lowered;
#[cfg(feature = "device-runtime")]
#[allow(unused_imports)]
pub(crate) use device::{
    admit_device_program_section, dense_input_initialization, dense_input_lifetime,
    device_program_for_lowered, device_section_for_program, run_dense_full_model_metal_closeout,
    run_dense_full_model_spawn_timing,
};
pub(crate) use dispatch::{
    load_provider_manifests, selected_providers_for_routes, ProviderManifest,
};
#[cfg(feature = "hir-go")]
#[allow(unused_imports)]
pub(crate) use go_build::{emit_go_module, invoke_go_build, GoBuildLayout};
#[cfg(all(feature = "device-runtime", feature = "host-macos-arm64"))]
#[allow(unused_imports)]
pub(crate) use host_factory::{
    admitted_backends, construct_composite_host, create_program_session, discovery_receipt,
    effective_backend_selection, execute_device_descriptor, host_error_diagnostic,
    missing_backend_artifact, missing_device_descriptor, resolve_backend_selection,
    BackendDiscoveryReceipt, E_BACKEND_UNAVAILABLE, E_DEVICE_ABI_MISMATCH, E_DEVICE_DESCRIPTOR,
    E_DEVICE_DTYPE_MISMATCH, E_DEVICE_ENTRY_MISMATCH, E_DEVICE_SHAPE_MISMATCH, E_NO_DEVICE_PROGRAM,
};
#[allow(unused_imports)]
pub(crate) use library_link::emit_linked_library_crates;
#[cfg(feature = "mir-llvm")]
#[allow(unused_imports)]
pub use llvm_host::{
    build_host_program, discover_llvm_host_toolchain, ensure_llvm_runtime_archive,
    host_llvm_target_triple, host_triple_for, LlvmHostBuild, LlvmHostProfile,
};
pub(crate) use manifest::validate_manifest;
#[allow(unused_imports)]
pub(crate) use manifest::{
    manifest_backend_selection, manifest_build_target_for_input, manifest_device_inputs,
};
pub use manifest::{
    read_manifest, FaberManifest, ManifestProduct, ManifestProductEmit, ManifestProductKind,
    ManifestProductShaders,
};
#[allow(unused_imports)]
pub use mir::run_fmir_image_bytes_with_stdio;
#[allow(unused_imports)]
pub use mir::with_lowered_package_mir;
pub(crate) use product::build_browser_product_with_postprocess;
#[cfg(test)]
pub(crate) use product::{build_browser_product, build_browser_product_static_assets};
use radix_module::codegen::Target;
use radix_module::diagnostics::Diagnostic;
use radix_module::driver::Config;
use radix_program::discovery::PackageSpec;
use radix_program::library::{LibraryInterfaceCache, LibraryResolver, LockedLibraryPackage};
use std::path::Path;

/// ONE-EFFECTIVE-TARGET SEAM — executed lane (TARGETLANE001 executed-lane
/// routing; authoritative decision record
/// `radix/docs/factory/executed-lane-target-decision.md`).
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
///   3. **Executed-lane default `fmir`** (`Target::MirFmirBinary`) when no
///      target is named — the source-independent package-MIR binary image the
///      in-process runner consumes.
///
/// The seam is single-valued and never ambient: analysis and execution resolve
/// from the same source, so the two targets cannot diverge (ABI source-of-
/// truth invariant). The radix-side TARGETLANE001 guard is unchanged — this
/// only fixes the executed route's target selection.
pub(crate) fn executed_lane_analysis_target(config: &Config, input: &Path) -> Target {
    if is_mir_backed_target(config.target) {
        return config.target;
    }
    manifest::manifest_build_target_for_input(input)
        .ok()
        .flatten()
        .unwrap_or(Target::MirFmirBinary)
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
            | Target::MirMetal
            | Target::MirWgsl
            | Target::MirSexp
            | Target::MirScena
            | Target::MirFmir
            | Target::MirFmirBinary
            | Target::MirFmirBundle
    )
}

#[allow(unused_imports)]
pub use locale::available_locales;
/// Installed reader-pack path resolution shared by lib surfaces
/// (`diagnostic_explain`) and package code-locale lookup. Used by the lib
/// crate only; the bin's copy is legitimately unused.
#[allow(unused_imports)]
pub(crate) use locale::installed_locale_pack_path;
#[allow(unused_imports)]
pub use locale::locale_pack_for_emit;
#[allow(unused_imports)]
pub(crate) use locale::{
    config_with_locale, default_config_with_locale, load_locale_pack_for_input,
    locale_pack_for_emit_with_diagnostic,
};

pub(super) const MANIFEST_FILE: &str = "faber.toml";

pub(crate) fn missing_package_path_diagnostic(path: &Path) -> Box<Diagnostic> {
    Box::new(
        crate::package_diagnostic_error(format!(
            "package input path '{}' does not exist",
            path.display()
        ))
        .with_file(path.display().to_string())
        .with_arg("issue", "package_path_missing")
        .with_arg("path", path.display().to_string()),
    )
}

/// Resolve a product input against the caller's working directory before
/// entering the explicit-input compiler API. Ambient CWD lookup is a Faber
/// command concern, not a radix-program concern.
///
/// `faber test` shares this seam because package discovery requires absolute
/// inputs: a raw `.` lexically normalizes to an empty path and fails as
/// `cannot read '.'` (rdx-s05-1).
pub(crate) fn absolutize_path(path: &Path) -> std::path::PathBuf {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(path)
    };
    radix_program::paths::normalize_path(&path)
}

pub(crate) fn discover_package(input: &Path) -> Result<PackageSpec, Box<Diagnostic>> {
    let spec = radix_program::discovery::discover_package(&absolutize_path(input))?;
    if spec.manifest_backed {
        let manifest_path = spec.package_root.join(MANIFEST_FILE);
        if manifest_path.exists() {
            let manifest = read_manifest(&manifest_path)?;
            validate_manifest(&manifest, &manifest_path)?;
        }
    }
    Ok(spec)
}

pub(crate) fn discover_build_layout(
    input: &Path,
) -> Result<radix_program::discovery::BuildLayout, Box<Diagnostic>> {
    radix_program::discovery::discover_build_layout(&absolutize_path(input))
}

#[allow(dead_code)]
pub(crate) fn is_manifest_backed_or_directory_package_input(input: &Path) -> bool {
    radix_program::discovery::is_manifest_backed_or_directory_package_input(&absolutize_path(input))
}

/// Build the explicit library cache seam used by product MIR route plumbing.
pub(crate) fn library_cache_with_config(config: &Config) -> LibraryInterfaceCache {
    LibraryInterfaceCache::with_config(config).with_locale_loader(load_locale_pack_for_input)
}

pub(crate) fn library_cache_default() -> LibraryInterfaceCache {
    LibraryInterfaceCache::default().with_locale_loader(load_locale_pack_for_input)
}

pub(crate) fn library_resolver_from_config(
    config: &radix_module::driver::Config,
) -> LibraryResolver {
    // The library home (provider repos such as `norma/` and `triga/`) is
    // independent of the reader-pack stdlib path (`stdlib/locale/<id>/pack.toml`).
    // Precedence: an explicit custom stdlib roots the library home; otherwise
    // `FABER_LIBRARY_HOME` and the workspace probe locate provider repos. The
    // auto-discovered dev stdlib is never a library home. The ambient probe is
    // faber-owned (CTO memo); `radix-program` receives the resolved
    // home as an explicit input.
    if config.stdlib_explicit {
        return config
            .stdlib_path
            .as_ref()
            .map(|path| LibraryResolver::new(path.clone()))
            .unwrap_or_else(crate::library::default_library_resolver);
    }
    crate::library::default_library_resolver()
}

/// Build a library resolver for a package root, attaching `faber.toml`
/// dependencies and `faber.lock` interface roots when present.
pub(crate) fn library_resolver_for_package(
    config: &radix_module::driver::Config,
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
    let lock = match radix_program::lockfile::read_lock(package_root) {
        Ok(lock) => lock,
        Err(diag) => return Err(vec![*diag]),
    };
    let lock_diags = radix_program::lockfile::validate_dependencies_against_lock(
        package_root,
        &manifest.dependencies,
        lock.as_ref(),
    );
    if lock_diags.iter().any(Diagnostic::is_error) {
        return Err(lock_diags);
    }

    let mut locked = std::collections::BTreeMap::new();
    if let Some(lock) = lock.as_ref() {
        let lock_index = radix_program::lockfile::lock_index(
            &package_root.join(radix_program::lockfile::LOCK_FILE),
            lock,
        )?;
        for package in lock_index.values() {
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
// `FABER_LIBRARY_HOME` / workspace probing — while keeping the faber-internal
// entry signatures.
pub fn faber_analysis_seams() -> radix_program::analyze::AnalysisSeams {
    radix_program::analyze::AnalysisSeams {
        locale_loader: load_locale_pack_for_input,
        manifest_validator: validate_product_manifest_for_spec,
        resolver_for_package: library_resolver_for_package,
    }
}

/// Faber-supplied package-emission seams. Go source assembly remains a product
/// helper because it owns the generated multi-file package shape; the shared
/// dispatch and Rust assembly live in `radix-program`.
pub fn faber_emission_seams() -> radix_program::emit::EmissionSeams {
    radix_program::emit::EmissionSeams {
        analysis: faber_analysis_seams(),
        #[cfg(feature = "hir-go")]
        emit_go: go_build::generate_package_go_result,
    }
}

fn validate_product_manifest_for_spec(spec: &PackageSpec) -> Result<(), Box<Diagnostic>> {
    manifest::manifest_for_spec(spec).map(|_| ())
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
#[cfg(feature = "device-runtime")]
#[path = "ngab1_host_partition_test.rs"]
mod ngab1_host_partition_test;

#[cfg(test)]
mod frontmatter_integration_test;

#[cfg(test)]
mod proba_integration_test;

#[cfg(test)]
mod proba_runner_test;

#[cfg(test)]
mod composite_plan_synthetic_test;

#[cfg(test)]
#[path = "../package_test.rs"]
mod tests;

#[cfg(test)]
#[path = "../package_text_contract_test.rs"]
mod text_contract_tests;

// DDPP1-U2 (C2 feature isolation): the small-build surface proof — these tests
// compile and run only without `device-runtime`, exercising the CPU-only
// route-selection plumbing in the build that excludes the device runtime.
#[cfg(all(test, not(feature = "device-runtime")))]
#[path = "feature_isolation_test.rs"]
mod feature_isolation_test;
