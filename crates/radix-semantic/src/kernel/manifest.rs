//! Residual `faber:*` kernel import prefix.
//!
//! N2 deleted the kernel modules. The prefix is still recognized so a
//! `faber:*` import fails closed as an unknown module. Application packages
//! use `norma:*`.

/// Kernel import prefix for author paths.
pub const KERNEL_PREFIX: &str = "faber:";

/// Kernel glob import path for manifest expansion.
pub const KERNEL_GLOB_IMPORT: &str = "faber:*";

/// Manifest schema version (matches introducing `faber` release).
pub const KERNEL_MANIFEST_VERSION: &str = env!("CARGO_PKG_VERSION");

pub use radix_types::KernelModule;

/// One row in the kernel manifest table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KernelManifestEntry {
    pub module: KernelModule,
    pub import_path: &'static str,
    pub provider_module: &'static str,
    pub verbs: &'static [&'static str],
    pub since: &'static str,
}

/// N2 deleted the `faber:*` kernel import surface. The prefix is still
/// recognized so `importa ex "faber:json"` fails as an unknown module rather
/// than a silent no-op. There are no remaining kernel modules.
const MANIFEST: &[KernelManifestEntry] = &[];

/// Return the locked v1 kernel manifest rows.
#[must_use]
pub fn kernel_manifest_entries() -> &'static [KernelManifestEntry] {
    MANIFEST
}

/// Return the number of v1 kernel modules.
#[must_use]
pub fn kernel_module_count() -> usize {
    MANIFEST.len()
}

/// Look up a kernel module by its short name (`solum`, `json`, …).
pub fn lookup_module(name: &str) -> Option<KernelModule> {
    MANIFEST
        .iter()
        .find(|entry| entry.module.name() == name)
        .map(|entry| entry.module)
}

/// Whether `verb` is in the manifest-supported subset for `module`.
///
/// Drives the interpreted-package host bridge (see
/// `docs/factory/faber-script-runtime/stage1b-package-host-bridge.md`): a
/// `norma:<module>` call routes to the in-process kernel only when the verb is
/// in this subset. The subset itself is the script/package parity contract,
/// verified by `kernel_manifest_verbs_are_subset_of_norma_public_surface`.
#[must_use]
pub fn kernel_module_supports_verb(module: KernelModule, verb: &str) -> bool {
    MANIFEST
        .iter()
        .any(|entry| entry.module == module && entry.verbs.contains(&verb))
}
