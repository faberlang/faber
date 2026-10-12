//! Script kernel manifest and `faber:*` import resolution.
//!
//! Kernel modules are compiler-known host builtins for script mode (`faber run`).
//! Package builds reject `faber:` imports; script mode resolves paths against
//! [`KERNEL_MANIFEST`] and lowers them with `MirProviderKind::Kernel`.

mod manifest;

pub use manifest::{
    KERNEL_GLOB_IMPORT, KERNEL_MANIFEST_VERSION, KERNEL_PREFIX, KernelManifestEntry, KernelModule,
    kernel_manifest_entries, kernel_module_count, kernel_module_supports_verb,
};

/// Outcome of resolving a `faber:<module>` import path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelResolveError {
    NotKernelPath,
    InvalidPath { path: String, reason: String },
    UnknownModule { name: String },
}

/// Return whether `path` is a `faber:` kernel import specifier.
#[must_use]
pub fn is_kernel_import_path(path: &str) -> bool {
    path.starts_with(KERNEL_PREFIX)
}

/// Return whether `path` is the manifest glob import specifier.
#[must_use]
pub fn is_kernel_glob_import_path(path: &str) -> bool {
    path == KERNEL_GLOB_IMPORT
}

/// Resolve a manifest module name (`solum`, `json`, …), accepting the English
/// norma module coordinates (`fs`, `console`, `random`, `process`) the
/// interpreted-package bridge uses after the norma English rename.
#[must_use]
pub fn resolve_kernel_module_name(name: &str) -> Option<KernelModule> {
    match name {
        // N2: I/O Norma modules run through `ad` / the shared leaf. Do not
        // alias them onto retired kernel modules.
        "fs" | "solum" | "console" | "consolum" | "random" | "aleator" | "process"
        | "processus" => None,
        _ => manifest::lookup_module(name),
    }
}

/// Resolve a kernel import path to its manifest module.
///
/// # Errors
///
/// Returns [`KernelResolveError`] if the path is not a kernel path, is
/// malformed (empty, glob, or contains `/` or `:`), or names an unknown module.
pub fn resolve_kernel_module(path: &str) -> Result<KernelModule, KernelResolveError> {
    if !is_kernel_import_path(path) {
        return Err(KernelResolveError::NotKernelPath);
    }
    if is_kernel_glob_import_path(path) {
        return Err(KernelResolveError::InvalidPath {
            path: path.to_owned(),
            reason: "`faber:*` must be imported as a named glob alias".to_owned(),
        });
    }

    let module_name = &path[KERNEL_PREFIX.len()..];
    if module_name.is_empty() {
        return Err(KernelResolveError::InvalidPath {
            path: path.to_owned(),
            reason: "kernel module name must not be empty".to_owned(),
        });
    }
    if module_name.contains('/') || module_name.contains(':') {
        return Err(KernelResolveError::InvalidPath {
            path: path.to_owned(),
            reason: "kernel import paths are flat (`faber:<module>` only)".to_owned(),
        });
    }

    manifest::lookup_module(module_name).ok_or_else(|| KernelResolveError::UnknownModule {
        name: module_name.to_owned(),
    })
}

/// Diagnostic text for an unknown `faber:*` module.
#[must_use]
pub fn unknown_kernel_module_message(name: &str) -> String {
    format!("unknown faber kernel module '{name}'; see faber host manifest")
}

/// Diagnostic text for `faber:*` imports in package mode.
#[must_use]
pub fn kernel_script_mode_only_message() -> &'static str {
    "`faber:` kernel imports are script-mode only; use `faber run` for kernel scripts"
}
