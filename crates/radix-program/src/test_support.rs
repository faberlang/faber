//! Shared test seams for radix-program unit tests (RP4).
//!
//! Mirrors the faber product seams the analysis/emission surfaces receive in
//! production: no product manifest validation, no library home, no linked
//! crates — the compiler-owned paths exercised standalone. The locale seam
//! mirrors the faber product default: the product-default English pack is
//! loaded when the caller selects no locale (pack-less registry-direct lexing
//! is not the package analysis route).

use std::collections::BTreeMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use radix_module::diagnostics::Diagnostic;
use radix_module::driver::Config;

use crate::analyze::AnalysisSeams;
use crate::discovery::PackageSpec;
use crate::library::{LibraryResolver, LockedLibraryPackage};
use crate::mir::{DeviceManifestConfig, MirSeams, PreparedPackageMir};

/// The reader pack selected by the CLI locale or the package's `[locale]`
/// (`[reader]` legacy alias) manifest section, loaded from the workspace
/// language-pack tree. The default (no selection) is the product-default
/// English pack. Mirrors the faber product seam; the moved mir tests use
/// Latin-locale fixture packages (`[reader] locale = "la"`).
pub fn test_locale_pack(
    input: &Path,
    cli_locale: Option<&str>,
) -> Result<Option<radix_module::locale::LocalePack>, Box<Diagnostic>> {
    let locale = if let Some(locale) = cli_locale {
        locale.trim().to_owned()
    } else {
        crate::discovery::discover_build_layout(input)
            .ok()
            .map(|layout| layout.package_root.join(crate::MANIFEST_FILE))
            .filter(|path| path.is_file())
            .and_then(|path| crate::manifest::read_manifest(&path).ok())
            .and_then(|manifest| manifest.locale.locale.clone())
            .unwrap_or_else(|| "en".to_owned())
    };
    let path = radix_module::locale::workspace_locale_pack_path(&locale);
    let pack = radix_module::locale::LocalePack::from_toml_path(path).map_err(|err| {
        Box::new(
            Diagnostic::error(format!(
                "failed to load reader locale '{locale}' pack: {err}"
            ))
            .with_phase(radix_module::diagnostics::DiagnosticPhase::Lex),
        )
    })?;
    Ok(Some(pack))
}

pub fn accept_manifest(_spec: &PackageSpec) -> Result<(), Box<Diagnostic>> {
    Ok(())
}

/// The config-rooted library resolver for a package root, attaching
/// `faber.toml` dependencies and `faber.lock` interface roots when present
/// (mirrors the faber product seam `library_resolver_for_package`; the moved
/// mir tests write lock-backed fixture libraries under the config library
/// home).
fn test_resolver_for_package(
    config: &Config,
    package_root: &Path,
) -> Result<LibraryResolver, Vec<Diagnostic>> {
    let resolver = test_library_resolver(config);
    let manifest_path = package_root.join(crate::MANIFEST_FILE);
    if !manifest_path.is_file() {
        return Ok(resolver);
    }
    let manifest = crate::manifest::read_manifest(&manifest_path).map_err(|diag| vec![*diag])?;
    let lock = crate::lockfile::read_lock(package_root).map_err(|diag| vec![*diag])?;
    let mut locked = BTreeMap::new();
    if let Some(lock) = lock.as_ref() {
        let lock_index =
            crate::lockfile::lock_index(&package_root.join(crate::lockfile::LOCK_FILE), lock)?;
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
    // A package that declares `[library] provider = "X"` resolves its own
    // source root for `X:*` self-imports (a lock entry always wins).
    if let Some(library) = manifest.library.as_ref() {
        locked
            .entry(library.provider.clone())
            .or_insert_with(|| LockedLibraryPackage {
                name: manifest.package.name.clone(),
                version: manifest.package.version.clone(),
                interface_root: package_root.join(&manifest.paths.source),
            });
    }
    Ok(resolver.with_package_lock(manifest.dependencies, locked))
}

pub fn test_analysis_seams() -> AnalysisSeams {
    AnalysisSeams {
        locale_loader: test_locale_pack,
        manifest_validator: accept_manifest,
        resolver_for_package: test_resolver_for_package,
    }
}

// ── Package-MIR test seams (RP5) ─────────────────────────────────────────────

/// The executed-lane target resolution used by the moved mir tests: an
/// explicit MIR-backed config target wins, otherwise the caller's target is
/// kept (the tests never read a product manifest `[build] target`).
fn test_executed_lane_target(
    config: &Config,
    _input: &Path,
) -> Result<radix_module::codegen::Target, Box<radix_module::Diagnostic>> {
    use radix_module::codegen::Target;
    if matches!(
        config.target,
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
    ) {
        return Ok(config.target);
    }
    // The mir tests analyze through the executed-lane default (`fmir`), the
    // same default faber's product resolution produces for manifestless
    // inputs and manifests without `[build] target`.
    Ok(radix_module::codegen::Target::MirFmirBinary)
}

/// A config-rooted library resolver: an explicit `library_home` becomes the
/// home (the moved tests use `Config::with_library_home` fixtures); without
/// one the resolver has no home (no `FABER_LIBRARY_HOME` in tests).
fn test_library_resolver(config: &Config) -> LibraryResolver {
    if let Some(path) = config.library_home.as_ref() {
        return LibraryResolver::new(path.clone());
    }
    LibraryResolver::with_optional_home(None)
}

/// A config-rooted library cache wired to the test locale loader.
fn test_library_cache_for_config(config: &Config) -> crate::library::LibraryInterfaceCache {
    crate::library::LibraryInterfaceCache::with_config(config).with_locale_loader(test_locale_pack)
}

/// The moved mir tests never declare a `[device]` surface; the device-config
/// seam returns the empty default.
fn test_device_config_for_input(_input: &Path) -> Result<DeviceManifestConfig, Vec<Diagnostic>> {
    Ok(DeviceManifestConfig::default())
}

/// The moved mir tests never construct a device section (no `device-runtime`
/// in radix-program); the seam returns `None`.
fn test_device_section_builder(
    _prepared: &PreparedPackageMir<'_>,
    _lowered: &radix_mir_lower::lower::LoweredMirUnit<'_>,
    _diagnostic_path: &Path,
) -> Result<Option<radix_mir_fmir::FmirDeviceSection>, Vec<Diagnostic>> {
    Ok(None)
}

/// The moved mir tests never write an fmir-bin runner crate.
fn test_bin_runner_writer(
    _artifact_root: &Path,
    _entrypoint_path: &Path,
    _diagnostic_path: &Path,
    _image: &[u8],
    _release: bool,
) -> Result<(), Vec<Diagnostic>> {
    Ok(())
}

/// Test package-MIR seams for the moved mir tests (RP5).
pub fn test_mir_seams() -> MirSeams {
    MirSeams {
        analysis: test_analysis_seams(),
        executed_lane_target: test_executed_lane_target,
        library_cache_for_config: test_library_cache_for_config,
        device_config_for_input: test_device_config_for_input,
        device_section_builder: test_device_section_builder,
        bin_runner_writer: test_bin_runner_writer,
        toolchain_version: "test-toolchain",
    }
}

/// Temp directory that `Deref`s to [`Path`] so call sites can keep using
/// `dir.join(...)` after tempfile 3.26+ dropped `TempDir: Deref<Path>`.
pub struct TestDir {
    inner: tempfile::TempDir,
}

impl TestDir {
    pub fn path(&self) -> &Path {
        self.inner.path()
    }
}

impl Deref for TestDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        self.inner.path()
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        self.inner.path()
    }
}

impl From<&TestDir> for PathBuf {
    fn from(dir: &TestDir) -> PathBuf {
        dir.path().to_path_buf()
    }
}

impl std::fmt::Debug for TestDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("TestDir").field(&self.path()).finish()
    }
}

pub fn test_temp_dir(label: &str) -> TestDir {
    let inner = tempfile::Builder::new()
        .prefix(&format!("radix-program-{label}-"))
        .tempdir()
        .expect("create temp dir");
    TestDir { inner }
}
