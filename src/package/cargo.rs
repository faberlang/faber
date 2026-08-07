use fs2::FileExt;
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use radix::codegen::Target;
use radix::diagnostics::Diagnostic;

use crate::core_support::materialize::{materialize, MaterializedCoreSupport};

use super::runtime_dependency::runtime_path_from_crate_roots;
use super::{BuildLayout, FaberManifest, ManifestRustHost, ProviderManifest};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RustRuntimePlan {
    /// Whether the generated crate path-links `faber-runtime` (HIR/plan fact).
    pub(crate) needs_faber: bool,
    /// Whether the generated crate depends on `tokio` (async/cede HIR fact).
    pub(crate) needs_tokio: bool,
    pub(crate) host: Option<ManifestRustHost>,
    pub(crate) non_runtime_routes: BTreeSet<String>,
    pub(crate) selected_providers: BTreeSet<String>,
    pub(crate) provider_manifests: Vec<ProviderManifest>,
    pub(crate) provider_error: Option<String>,
    /// Faber library path dependencies: (`crate_name`, absolute crate root).
    pub(crate) library_path_deps: Vec<(String, PathBuf)>,
}

impl RustRuntimePlan {
    /// Plan for emit paths that only have generated source and no analysis
    /// context (tests / fallback). Always links faber-runtime; never sniffs
    /// emitted text for policy.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn default_generated_crate_plan() -> Self {
        Self {
            needs_faber: true,
            needs_tokio: false,
            host: None,
            non_runtime_routes: BTreeSet::new(),
            selected_providers: BTreeSet::new(),
            provider_manifests: Vec::new(),
            provider_error: None,
            library_path_deps: Vec::new(),
        }
    }

    /// True when the build must emit a Cargo crate (not a bare `.rs` file).
    pub(crate) fn requires_generated_crate(&self) -> bool {
        self.needs_faber
            || self.needs_tokio
            || self.host.is_some()
            || !self.library_path_deps.is_empty()
            || !self.selected_providers.is_empty()
    }
}

pub(crate) fn package_host_selection_diagnostic(
    plan: &RustRuntimePlan,
    manifest_path: &Path,
) -> Option<Diagnostic> {
    if let Some(error) = &plan.provider_error {
        return Some(
            crate::package_diagnostic_error(error.clone())
                .with_file(manifest_path.display().to_string())
                .with_arg("issue", "host_provider_selection_invalid"),
        );
    }
    if plan.host.is_some() {
        return None;
    }
    // Dual-backend: builtin-covered ad routes do not require host selection.
    // Only host-only routes (and explicit `[dispatch].providers`) gate host.
    let host_routes = super::dispatch::host_required_routes(&plan.non_runtime_routes);
    if host_routes.is_empty() && plan.selected_providers.is_empty() {
        return None;
    }
    let routes = host_routes.iter().cloned().collect::<Vec<_>>().join(", ");
    let providers = plan
        .selected_providers
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let detail = if routes.is_empty() {
        format!("providers [{providers}]")
    } else {
        format!("routes [{routes}]")
    };
    Some(
        crate::package_diagnostic_error(format!(
            "package uses host providers without [target.rust] host selection: {detail}"
        ))
        .with_file(manifest_path.display().to_string())
        .with_arg("issue", "package_host_selection_required")
        .with_arg("routes", routes)
        .with_arg("providers", providers),
    )
}

/// Generate a minimal, deterministic `Cargo.toml` for the emitted Rust crate.
///
/// The Rust edition is fixed at 2021 for backend output; Faber source edition
/// is manifest metadata for the language frontend and does not imply a Rust
/// edition. `binary_name` must already be sanitized for Cargo.
fn generate_cargo_toml(
    meta: &FaberManifest,
    binary_name: &str,
    plan: &RustRuntimePlan,
    support: &MaterializedCoreSupport,
) -> Result<String, Box<Diagnostic>> {
    let version = if meta.package.version.trim().is_empty() {
        "0.1.0"
    } else {
        meta.package.version.trim()
    };
    render_generated_cargo_toml_with_support(binary_name, version, plan, support)
}

fn render_generated_cargo_toml_with_support(
    name: &str,
    version: &str,
    plan: &RustRuntimePlan,
    support: &MaterializedCoreSupport,
) -> Result<String, Box<Diagnostic>> {
    let materialized_faber_path = support.faber_runtime().map_err(core_support_diagnostic)?;
    let linked_runtime_path = runtime_path_from_crate_roots(
        plan.library_path_deps
            .iter()
            .map(|(_, crate_path)| crate_path.as_path()),
    );
    let faber_path = linked_runtime_path.unwrap_or(materialized_faber_path);
    let mut deps = String::new();
    if plan.needs_faber {
        deps.push_str(&format!(
            "faber = {{ package = {}, path = {} }}\n",
            toml_string("faber-runtime"),
            toml_path(&faber_path),
        ));
    }
    if matches!(plan.host, Some(ManifestRustHost::Native)) {
        deps.push_str(&format!(
            "host_kernel = {{ package = {}, path = {} }}\n",
            toml_string("host-kernel"),
            toml_path(&support.host_kernel().map_err(core_support_diagnostic)?),
        ));
        deps.push_str(&format!(
            "host_native = {{ package = {}, path = {} }}\n",
            toml_string("host-native"),
            toml_path(&support.host_native().map_err(core_support_diagnostic)?),
        ));
        for provider in &plan.selected_providers {
            deps.push_str(&format!(
                "{} = {{ package = {}, path = {} }}\n",
                toml_key(provider),
                toml_string(provider),
                toml_path(
                    &support
                        .provider(provider)
                        .map_err(core_support_diagnostic)?
                ),
            ));
        }
    }
    if plan.needs_tokio {
        deps.push_str("tokio = { version = \"1\", features = [\"rt\", \"net\", \"time\"] }\n");
    }
    for (crate_name, crate_path) in &plan.library_path_deps {
        deps.push_str(&format!(
            "{} = {{ path = {} }}\n",
            toml_key(crate_name),
            toml_path(crate_path),
        ));
    }

    Ok(format!(
        r#"[package]
name = {name}
version = {version}
edition = "2021"

# This crate was generated by `faber build` from the package's faber.toml.
# Source of truth: faber.toml at the package root.
# Do not edit this file by hand.
#
# Core-support archive sha256: {sha}

[workspace]
# Empty workspace table keeps this generated crate independent when the
# package lives inside the faber repository workspace tree (e.g. examples/).
# Prevents "current package believes it's in a workspace" errors for
# `cargo build/test --manifest-path target/faber/Cargo.toml`.

[dependencies]
{deps}"#,
        name = toml_string(name),
        version = toml_string(version),
        sha = crate::core_support::SHA256,
        deps = deps
    ))
}

#[cfg(test)]
fn render_generated_cargo_toml(
    name: &str,
    version: &str,
    plan: &RustRuntimePlan,
    _: &Path,
) -> String {
    let support = match materialize() {
        Ok(support) => support,
        Err(error) => return format!("core support materialization failed: {error}"),
    };
    match render_generated_cargo_toml_with_support(name, version, plan, &support) {
        Ok(rendered) => rendered,
        Err(error) => format!("generated Cargo.toml rendering failed: {}", error.message),
    }
}

fn core_support_diagnostic(
    error: crate::core_support::materialize::MaterializeError,
) -> Box<Diagnostic> {
    Box::new(
        crate::package_diagnostic_error(format!("verified core support is unavailable: {error}"))
            .with_arg("issue", "core_support_materialization_failed"),
    )
}

pub(super) fn toml_key(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        value.to_owned()
    } else {
        toml_string(value)
    }
}

pub(super) fn toml_path(path: &Path) -> String {
    toml_string(&path.display().to_string())
}

pub(super) fn toml_string(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\u{08}' => quoted.push_str("\\b"),
            '\u{0C}' => quoted.push_str("\\f"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character if character.is_control() => {
                use std::fmt::Write;
                write!(&mut quoted, "\\u{:04X}", character as u32)
                    .expect("writing to a string cannot fail");
            }
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
#[path = "cargo_test.rs"]
mod tests;

/// Write the generated Rust crate tree under the layout's `target/faber/` directory.
///
/// The generated crate owns exactly `target/faber/Cargo.toml` and
/// `target/faber/src/main.rs`. Other files under `target/` are Cargo artifacts
/// or user-adjacent build output and are intentionally left alone.
#[cfg_attr(not(test), allow(dead_code))]
pub fn emit_generated_crate(
    layout: &BuildLayout,
    rust_code: &str,
    meta: Option<&FaberManifest>,
) -> Result<PathBuf, Box<Diagnostic>> {
    emit_generated_crate_with_runtime_plan(
        layout,
        rust_code,
        meta,
        &RustRuntimePlan::default_generated_crate_plan(),
    )
}

pub(crate) fn emit_generated_crate_with_runtime_plan(
    layout: &BuildLayout,
    rust_code: &str,
    meta: Option<&FaberManifest>,
    plan: &RustRuntimePlan,
) -> Result<PathBuf, Box<Diagnostic>> {
    // FBR-P2-004: the generated crate is written as a complete snapshot into a
    // unique temporary sibling of `target/faber/` and atomically published
    // into the shared contract path. A failure at any point before the final
    // rename keeps the last-known-good generated crate in place. Concurrent
    // emit + cargo sequences for the same package are serialized by
    // [`lock_generated_crate_build`] at the command layer.
    let crate_root = layout.generated_crate_root.clone();
    let staging = crate_root.parent().ok_or_else(|| {
        Box::new(crate::package_diagnostic_error(
            "generated crate root has no parent directory",
        ))
    })?;
    fs::create_dir_all(staging).map_err(|err| Box::new(Diagnostic::io_error(staging, &err)))?;

    let support = materialize().map_err(core_support_diagnostic)?;
    let cargo_src = if let Some(m) = meta {
        generate_cargo_toml(m, layout.binary_name(), plan, &support)?
    } else {
        render_generated_cargo_toml_with_support(layout.binary_name(), "0.1.0", plan, &support)?
    };

    let temp = unique_temp_sibling(staging, "crate")?;
    let snapshot = (|| {
        write_crate_snapshot(&temp, &cargo_src, rust_code, plan)?;
        // Preserve library path-dependency crates emitted earlier in this
        // sequence (`target/faber/deps/...`) inside the new snapshot; the
        // generated crate's Cargo.toml path-links them.
        let deps = crate_root.join("deps");
        if fs::symlink_metadata(&deps).is_ok() {
            copy_tree(&deps, &temp.join("deps"))?;
        }
        // Preserve Go package output written by go_build.rs
        // (`target/faber/go/...`). It is not part of the crate snapshot, but
        // the publish swap replaces all of `target/faber/`, so without this
        // copy every Rust publish would silently delete the Go module tree.
        let go_output = crate_root.join("go");
        if fs::symlink_metadata(&go_output).is_ok() {
            copy_tree(&go_output, &temp.join("go"))?;
        }
        write_snapshot_completion(&temp)?;
        fsync_tree(&temp)?;
        #[cfg(test)]
        maybe_inject_crate_failure(4)?;
        publish_directory(&temp, &crate_root)?;
        fsync_dir(staging)?;
        Ok(())
    })();
    match snapshot {
        Ok(()) => Ok(crate_root),
        Err(error) => {
            if fs::symlink_metadata(&temp).is_ok() {
                remove_temp_tree(&temp)?;
            }
            Err(error)
        }
    }
}

/// Write the complete generated crate files into `temp` with the same relative
/// layout as the published `target/faber/` crate: `Cargo.toml`, `src/main.rs`,
/// and optional native host registration + host manifest.
fn write_crate_snapshot(
    temp: &Path,
    cargo_src: &str,
    rust_code: &str,
    plan: &RustRuntimePlan,
) -> Result<(), Box<Diagnostic>> {
    let src_dir = temp.join("src");
    fs::create_dir_all(&src_dir).map_err(|err| Box::new(Diagnostic::io_error(&src_dir, &err)))?;
    let cargo_path = temp.join("Cargo.toml");
    fs::write(&cargo_path, cargo_src)
        .map_err(|err| Box::new(Diagnostic::io_error(&cargo_path, &err)))?;
    #[cfg(test)]
    maybe_inject_crate_failure(1)?;

    // Policy: keep an outer generated marker even when backend codegen already
    // writes its own header, because this file belongs to the package builder's
    // generated crate contract.
    let rust_code = rust_code.to_owned();
    if matches!(plan.host, Some(ManifestRustHost::Native)) {
        write_host_registration(&src_dir, plan)
            .map_err(|err| Box::new(Diagnostic::io_error(&src_dir, &err)))?;
        write_host_manifest(temp, plan)
            .map_err(|err| Box::new(Diagnostic::io_error(temp, &err)))?;
    }
    #[cfg(test)]
    maybe_inject_crate_failure(2)?;
    let final_code = format!(
        "// Generated by faber build — do not edit by hand.\n\
         // Crate layout: target/faber/  (see plan.md)\n\
         // Run with: cargo build --manifest-path target/faber/Cargo.toml --target-dir target\n\n{}",
        rust_code
    );
    let final_code = format_package_rust_source(&final_code);
    let main_path = src_dir.join("main.rs");
    fs::write(&main_path, final_code)
        .map_err(|err| Box::new(Diagnostic::io_error(&main_path, &err)))?;
    #[cfg(test)]
    maybe_inject_crate_failure(3)?;
    Ok(())
}

fn format_package_rust_source(source: &str) -> String {
    radix::tool::format_generated_code(Target::HirRust, source)
        .unwrap_or_else(|_| source.to_owned())
}

/// Lock file for the per-package generated-crate sequence, stored beside the
/// published crate (`target/faber/`) so atomic publication never replaces it.
const GENERATED_CRATE_LOCK_FILE: &str = ".faber-build.lock";

/// Completion marker written into a fully-written temporary crate snapshot
/// before atomic publication (same convention as the core-support materializer).
const GENERATED_CRATE_COMPLETE_FILE: &str = ".faber-crate-complete";

/// Acquire the exclusive per-package advisory lock that serializes the
/// generated-crate emit + cargo sequence (FBR-P2-004).
///
/// The command layer holds the returned guard across emission AND the Cargo
/// invocation, so concurrent `faber build`/`faber run` processes for the same
/// package cannot interleave files from different runtime plans. The lock
/// lives in `target/` (a sibling of the published `target/faber/` crate), so
/// publication never replaces the lock file.
pub(crate) fn lock_generated_crate_build(
    layout: &BuildLayout,
) -> Result<GeneratedCrateLock, Box<Diagnostic>> {
    let target_dir = layout.cargo_target_dir.clone();
    fs::create_dir_all(&target_dir)
        .map_err(|err| Box::new(Diagnostic::io_error(&target_dir, &err)))?;
    let lock_path = target_dir.join(GENERATED_CRATE_LOCK_FILE);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|err| Box::new(Diagnostic::io_error(&lock_path, &err)))?;
    file.lock_exclusive()
        .map_err(|err| Box::new(Diagnostic::io_error(&lock_path, &err)))?;
    Ok(GeneratedCrateLock { _file: file })
}

/// RAII guard for the per-package generated-crate lock. Dropping the guard
/// closes the file, which releases the OS-level advisory lock.
pub(crate) struct GeneratedCrateLock {
    _file: File,
}

/// Create a unique temporary sibling directory for staged writes (same style
/// as the core-support materializer).
fn unique_temp_sibling(parent: &Path, label: &str) -> Result<PathBuf, Box<Diagnostic>> {
    for attempt in 0..128_u32 {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| {
                Box::new(crate::package_diagnostic_error(
                    "system clock precedes epoch",
                ))
            })?
            .as_nanos();
        let path = parent.join(format!(
            ".{label}.tmp-{}-{nonce}-{attempt}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(Box::new(Diagnostic::io_error(&path, &error))),
        }
    }
    Err(Box::new(crate::package_diagnostic_error(
        "could not create a unique temporary directory",
    )))
}

/// Pick an unused sibling path for quarantining the previously published
/// generated crate during a swap — WITHOUT creating it.
///
/// The returned path must not exist before the rename that moves the old
/// crate onto it: `fs::rename` cannot replace an existing directory on
/// Windows, so a pre-created empty quarantine (as [`unique_temp_sibling`]
/// returns) would make every republish fail there. Same non-created-path
/// convention as `unique_product_quarantine` in product.rs and
/// `quarantine_incomplete_entry` in core_support/materialize.rs. Stale paths
/// (e.g. an orphaned quarantine from a previous crash) are skipped.
fn unique_quarantine_sibling(parent: &Path) -> Result<PathBuf, Box<Diagnostic>> {
    for attempt in 0..128_u32 {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| {
                Box::new(crate::package_diagnostic_error(
                    "system clock precedes epoch",
                ))
            })?
            .as_nanos();
        let path = parent.join(format!(".old.tmp-{}-{nonce}-{attempt}", std::process::id()));
        if fs::symlink_metadata(&path).is_err() {
            return Ok(path);
        }
    }
    Err(Box::new(crate::package_diagnostic_error(
        "could not allocate a quarantine path for the previous generated crate",
    )))
}

/// Atomically move a complete `temp` snapshot into `target`, quarantining any
/// previously published directory and removing it only after the swap
/// succeeds. A failed swap restores the previous directory, so the
/// last-known-good generated crate survives any single publish failure.
///
/// The quarantine name is allocated WITHOUT creating it (see
/// [`unique_quarantine_sibling`]): `fs::rename` cannot replace an existing
/// destination on Windows, so a pre-created empty quarantine would make every
/// republish fail there. A non-created path keeps the swap portable.
fn publish_directory(temp: &Path, target: &Path) -> Result<(), Box<Diagnostic>> {
    if fs::symlink_metadata(target).is_err() {
        return fs::rename(temp, target)
            .map_err(|err| Box::new(Diagnostic::io_error(target, &err)));
    }
    let parent = target.parent().ok_or_else(|| {
        Box::new(crate::package_diagnostic_error(
            "generated crate root has no parent directory",
        ))
    })?;
    let quarantine = unique_quarantine_sibling(parent)?;
    fs::rename(target, &quarantine).map_err(|err| Box::new(Diagnostic::io_error(target, &err)))?;
    match fs::rename(temp, target) {
        Ok(()) => {
            remove_quarantine(&quarantine);
            Ok(())
        }
        Err(error) => {
            let restored = fs::rename(&quarantine, target);
            remove_quarantine(temp);
            match restored {
                Ok(()) => Err(Box::new(Diagnostic::io_error(target, &error))),
                Err(restore_error) => Err(Box::new(Diagnostic::io_error(
                    target,
                    &io::Error::other(format!(
                        "publish rename failed ({error}); restoring previous crate failed ({restore_error})"
                    )),
                ))),
            }
        }
    }
}

/// Best-effort removal of a quarantined or failed-snapshot directory. The
/// publish outcome is never masked by cleanup failures.
fn remove_quarantine(path: &Path) {
    if let Ok(()) = fs::remove_dir_all(path) {}
}

fn remove_temp_tree(temp: &Path) -> Result<(), Box<Diagnostic>> {
    fs::remove_dir_all(temp).map_err(|err| Box::new(Diagnostic::io_error(temp, &err)))
}

/// Write the completion marker that distinguishes a fully-written snapshot
/// from a partial one (same convention as the core-support materializer).
fn write_snapshot_completion(temp: &Path) -> Result<(), Box<Diagnostic>> {
    use std::io::Write;

    let marker = temp.join(GENERATED_CRATE_COMPLETE_FILE);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
        .map_err(|err| Box::new(Diagnostic::io_error(&marker, &err)))?;
    file.write_all(b"generated-crate-v1\n")
        .map_err(|err| Box::new(Diagnostic::io_error(&marker, &err)))?;
    file.sync_all()
        .map_err(|err| Box::new(Diagnostic::io_error(&marker, &err)))?;
    Ok(())
}

/// Recursively copy `source` into `destination`. Symlinks and non-regular
/// entries are rejected so a snapshot never carries indirection.
fn copy_tree(source: &Path, destination: &Path) -> Result<(), Box<Diagnostic>> {
    fs::create_dir_all(destination)
        .map_err(|err| Box::new(Diagnostic::io_error(destination, &err)))?;
    for entry in fs::read_dir(source).map_err(|err| Box::new(Diagnostic::io_error(source, &err)))? {
        let entry = entry.map_err(|err| Box::new(Diagnostic::io_error(source, &err)))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|err| Box::new(Diagnostic::io_error(&path, &err)))?;
        if metadata.file_type().is_symlink() {
            return Err(Box::new(crate::package_diagnostic_error(
                "generated crate dependency tree contains a symlink",
            )));
        }
        let destination_child = destination.join(entry.file_name());
        if metadata.is_dir() {
            copy_tree(&path, &destination_child)?;
        } else if metadata.is_file() {
            fs::copy(&path, &destination_child)
                .map_err(|err| Box::new(Diagnostic::io_error(&destination_child, &err)))?;
        } else {
            return Err(Box::new(crate::package_diagnostic_error(
                "generated crate dependency tree contains a non-regular entry",
            )));
        }
    }
    Ok(())
}

/// Durably flush every file and directory under `root`, deepest directories
/// first (same convention as the core-support materializer).
fn fsync_tree(root: &Path) -> Result<(), Box<Diagnostic>> {
    let mut directories = vec![root.to_path_buf()];
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|err| Box::new(Diagnostic::io_error(&directory, &err)))?
        {
            let entry = entry.map_err(|err| Box::new(Diagnostic::io_error(&directory, &err)))?;
            let path = entry.path();
            if fs::symlink_metadata(&path)
                .map_err(|err| Box::new(Diagnostic::io_error(&path, &err)))?
                .is_dir()
            {
                directories.push(path.clone());
                pending.push(path);
                continue;
            }
            File::open(&path)
                .and_then(|file| file.sync_all())
                .map_err(|err| Box::new(Diagnostic::io_error(&path, &err)))?;
        }
    }
    directories.sort_by_key(|directory| std::cmp::Reverse(directory.components().count()));
    for directory in directories {
        fsync_dir(&directory)?;
    }
    Ok(())
}

#[cfg(unix)]
fn fsync_dir(path: &Path) -> Result<(), Box<Diagnostic>> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|err| Box::new(Diagnostic::io_error(path, &err)))?;
    Ok(())
}

#[cfg(not(unix))]
fn fsync_dir(_: &Path) -> Result<(), Box<Diagnostic>> {
    Ok(())
}

// Test-only failure injection for the generated-crate snapshot writer
// (FBR-P2-004). When the injected stage is non-zero, snapshot writing fails
// once the current stage reaches that value. Never compiled into production
// binaries.
#[cfg(test)]
thread_local! {
    static CRATE_SNAPSHOT_FAILURE_STAGE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn inject_crate_snapshot_failure_at(stage: u8) {
    CRATE_SNAPSHOT_FAILURE_STAGE.with(|cell| cell.set(stage));
}

#[cfg(test)]
fn maybe_inject_crate_failure(stage: u8) -> Result<(), Box<Diagnostic>> {
    CRATE_SNAPSHOT_FAILURE_STAGE.with(|target| {
        if target.get() != 0 && stage >= target.get() {
            Err(Box::new(crate::package_diagnostic_error(
                "injected generated-crate snapshot failure",
            )))
        } else {
            Ok(())
        }
    })
}

fn write_host_registration(src_dir: &Path, plan: &RustRuntimePlan) -> std::io::Result<()> {
    let path = src_dir.join("host_register.rs");
    let mut source = String::from(
        "pub fn install_or_exit() {\n    let mut kernel = host_kernel::Kernel::new();\n",
    );
    for provider in &plan.selected_providers {
        source.push_str(&format!(
            "    if let Err(error) = {provider}::register(&mut kernel) {{\n        eprintln!(\"host provider {provider} initialization failed: {{error}}\");\n        std::process::exit(70);\n    }}\n"
        ));
    }
    source.push_str(
        "    let host = match host_native::NativeHost::try_new(kernel) {\n        Ok(host) => host,\n        Err(error) => {\n            eprintln!(\"host native initialization failed: {error}\");\n            std::process::exit(70);\n        }\n    };\n    if let Err(error) = faber::install_host_dispatch(std::sync::Arc::new(host)) {\n        eprintln!(\"host dispatch initialization failed: {error}\");\n        std::process::exit(70);\n    }\n}\n",
    );
    std::fs::write(path, source)
}

fn write_host_manifest(root: &Path, plan: &RustRuntimePlan) -> std::io::Result<()> {
    let value = serde_json::json!({
        "manifest_version": 1,
        "providers": plan.provider_manifests,
        "required_routes": plan.non_runtime_routes,
    });
    let bytes = serde_json::to_vec_pretty(&value)
        .map_err(|err| std::io::Error::other(format!("serialize host manifest: {err}")))?;
    std::fs::write(root.join("host-manifest.json"), bytes)
}

/// Invoke Cargo to build the generated crate and return the expected binary path.
///
/// Uses the layout's paths so that artifacts land in `<pkg>/target/debug/<name>`
/// (sibling to `target/faber/`, never nested).
///
/// Cargo's stdout/stderr are inherited to preserve native compiler progress and
/// diagnostics.
#[allow(dead_code)]
pub(crate) fn invoke_cargo_build(
    layout: &BuildLayout,
    release: bool,
) -> Result<PathBuf, Box<Diagnostic>> {
    use std::process::Command;

    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("--manifest-path")
        .arg(&layout.generated_cargo_manifest)
        .arg("--target-dir")
        .arg(&layout.cargo_target_dir);

    if release {
        cmd.arg("--release");
    }

    let status = cmd.status().map_err(|e| {
        Box::new(crate::package_diagnostic_error(format!(
            "failed to spawn cargo (ensure cargo is installed and on PATH): {e}"
        )))
    })?;

    if !status.success() {
        return Err(Box::new(crate::package_diagnostic_error(format!(
            "cargo build exited with status {status}"
        ))));
    }

    let bin = if release {
        &layout.release_binary
    } else {
        &layout.debug_binary
    };
    Ok(bin.clone())
}

/// Invoke `cargo test` against the generated Rust crate.
///
/// Uses the package build directory contract:
///   --manifest-path <pkg>/target/faber/Cargo.toml
///   --target-dir <pkg>/target
///
/// The optional `filter` is passed before `--` as Cargo's Rust test name
/// filter. `harness_args` are forwarded after `--`. Test failures are not
/// converted into diagnostics; the harness exit status is returned verbatim so
/// the CLI can preserve Cargo's semantics.
#[allow(dead_code)]
pub fn invoke_cargo_test(
    layout: &BuildLayout,
    filter: Option<&str>,
    harness_args: &[String],
) -> Result<std::process::ExitStatus, Box<Diagnostic>> {
    use std::process::Command;

    let mut cmd = Command::new("cargo");
    cmd.arg("test")
        .arg("--manifest-path")
        .arg(&layout.generated_cargo_manifest)
        .arg("--target-dir")
        .arg(&layout.cargo_target_dir);

    if let Some(f) = filter {
        cmd.arg(f);
    }

    if !harness_args.is_empty() {
        cmd.arg("--");
        for arg in harness_args {
            cmd.arg(arg);
        }
    }

    let status = cmd.status().map_err(|e| {
        Box::new(crate::package_diagnostic_error(format!(
            "failed to spawn cargo (ensure cargo is installed and on PATH): {e}"
        )))
    })?;

    Ok(status)
}
