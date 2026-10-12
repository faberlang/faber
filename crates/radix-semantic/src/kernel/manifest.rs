//! Compile-time kernel manifest (v1 module set).
//!
//! **Parity contract:** each `verbs` list is a **subset** of the matching
//! `$FABER_LIBRARY_HOME/norma/src/<module>.fab` public file-scope surface. Spellings must match
//! `norma:*` exactly. Verified by
//! `kernel_manifest_verbs_are_subset_of_norma_public_surface` in
//! `kernel_namespace_test.rs`. Application packages use `norma:*`; script mode
//! uses `faber:*` against this manifest.

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

const V1_SINCE: &str = env!("CARGO_PKG_VERSION");

const MANIFEST: &[KernelManifestEntry] = &[
    KernelManifestEntry {
        module: KernelModule::Json,
        import_path: "faber:json",
        provider_module: "faber/kernel/json",
        // English canonicals (Pass B): solve→parse, pange→stringify,
        // tempta→try_parse. The runner dispatch speaks the same names.
        verbs: &["parse", "stringify", "try_parse"],
        since: V1_SINCE,
    },
    KernelManifestEntry {
        module: KernelModule::Processus,
        import_path: "faber:processus",
        provider_module: "faber/kernel/processus",
        // English canonicals (Pass B): argumenta→argv, lege→read_env,
        // sedes→cwd, muta→chdir, identitas→pid, exi→exit_process,
        // exsequi→exec, genera→spawn. The runner dispatch speaks the same
        // names. scribe→write_env is not advertised: hosts dropped the
        // processus:scribe route (7098458) and norma removed its wrapper
        // (b62153e).
        verbs: &[
            "argv",
            "read_env",
            "cwd",
            "chdir",
            "pid",
            "exit_process",
            "exec",
            "spawn",
        ],
        since: V1_SINCE,
    },
    KernelManifestEntry {
        module: KernelModule::Aleator,
        import_path: "faber:aleator",
        provider_module: "faber/kernel/aleator",
        // English canonicals (Pass B): fractum→random_float, sortire→random_int,
        // octetos→random_bytes, uuid→uuid, semina→seed. The runner dispatch
        // speaks the same names.
        verbs: &["random_float", "random_int", "random_bytes", "uuid", "seed"],
        since: V1_SINCE,
    },
    KernelManifestEntry {
        module: KernelModule::Consolum,
        import_path: "faber:consolum",
        provider_module: "faber/kernel/consolum",
        // English canonicals (Pass B): dic→write_partial, scribe→write_line,
        // mone→warn_line, funde→write_bytes, vide→debug_line, lege→read_line,
        // hauri→read_bytes, audit→stdin_is_tty, loquitur→stdout_is_tty,
        // admonet→stderr_is_tty. Raw/partial stdout (`write_partial`) unblocks
        // package-MIR coreutils such as `echo -n` and GNU `printf` without an
        // implicit newline. The runner dispatch speaks the same names.
        verbs: &[
            "write_partial",
            "write_bytes",
            "write_line",
            "warn_line",
            "debug_line",
            "read_line",
            "read_bytes",
            "stdin_is_tty",
            "stdout_is_tty",
            "stderr_is_tty",
        ],
        since: V1_SINCE,
    },
    KernelManifestEntry {
        module: KernelModule::Toml,
        import_path: "faber:toml",
        provider_module: "faber/kernel/toml",
        // English canonicals (Pass B): solve→parse, pange→stringify,
        // tempta→try_parse. The runner dispatch speaks the same names.
        // Mirrors the `norma:toml` wire-floor facade; TOML datetimes lower to
        // `Valor::Instans` and TOML trees never contain `Valor::Nihil`.
        verbs: &["parse", "stringify", "try_parse"],
        since: V1_SINCE,
    },
];

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
