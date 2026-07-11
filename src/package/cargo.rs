use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use radix::codegen::Target;
use radix::diagnostics::Diagnostic;

use super::{provider_crate_path, BuildLayout, FaberManifest, ManifestRustHost, ProviderManifest};

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
    /// Faber library path dependencies: (crate_name, absolute crate root).
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
            Diagnostic::error(error.clone())
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
        Diagnostic::error(format!(
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
fn generate_cargo_toml(meta: &FaberManifest, binary_name: &str, plan: &RustRuntimePlan) -> String {
    let version = if meta.package.version.trim().is_empty() {
        "0.1.0"
    } else {
        meta.package.version.trim()
    };
    render_generated_cargo_toml(binary_name, version, plan)
}

fn render_generated_cargo_toml(name: &str, version: &str, plan: &RustRuntimePlan) -> String {
    let faber_path = if matches!(plan.host, Some(ManifestRustHost::Native))
        || !plan.selected_providers.is_empty()
    {
        faber_runtime_path()
    } else {
        local_repo_path("faber-runtime")
    };
    let mut deps = String::new();
    if plan.needs_faber {
        deps.push_str(&format!(
            "faber = {{ package = \"faber-runtime\", path = \"{}\" }}\n",
            faber_path.display(),
        ));
    }
    if matches!(plan.host, Some(ManifestRustHost::Native)) {
        deps.push_str(&format!(
            "host_kernel = {{ package = \"host-kernel\", path = \"{}\" }}\n",
            host_kernel_path().display()
        ));
        deps.push_str(&format!(
            "host_native = {{ package = \"host-native\", path = \"{}\" }}\n",
            host_native_path().display()
        ));
        for provider in &plan.selected_providers {
            deps.push_str(&format!(
                "{provider} = {{ package = \"{provider}\", path = \"{}\" }}\n",
                provider_crate_path(provider).display()
            ));
        }
    }
    if plan.needs_tokio {
        deps.push_str("tokio = { version = \"1\", features = [\"rt\", \"net\", \"time\"] }\n");
    }
    for (crate_name, crate_path) in &plan.library_path_deps {
        deps.push_str(&format!(
            "{crate_name} = {{ path = \"{}\" }}\n",
            crate_path.display()
        ));
    }

    format!(
        r#"[package]
name = "{name}"
version = "{version}"
edition = "2021"

# This crate was generated by `faber build` from the package's faber.toml.
# Source of truth: faber.toml at the package root.
# Do not edit this file by hand.

[workspace]
# Empty workspace table keeps this generated crate independent when the
# package lives inside the faber repository workspace tree (e.g. examples/).
# Prevents "current package believes it's in a workspace" errors for
# `cargo build/test --manifest-path target/faber/Cargo.toml`.

[dependencies]
{deps}"#,
        name = name,
        version = version,
        deps = deps
    )
}

fn sibling_repo_path_from(manifest_dir: &Path, name: &str) -> PathBuf {
    for candidate in manifest_dir.ancestors() {
        let sibling = candidate.join(name);
        if !sibling.is_dir() {
            continue;
        }
        let has_core_runtime_repos = ["faber-runtime", "host-kernel-rs", "host-native-rs"]
            .iter()
            .all(|repo| candidate.join(repo).is_dir());
        if has_core_runtime_repos {
            return fs::canonicalize(&sibling).unwrap_or(sibling);
        }
    }
    let fallback = manifest_dir.join("..").join(name);
    fs::canonicalize(&fallback).unwrap_or(fallback)
}

fn local_repo_path_from(manifest_dir: &Path, name: &str) -> PathBuf {
    for candidate in manifest_dir.ancestors() {
        let sibling = candidate.join(name);
        if sibling.is_dir() {
            return sibling;
        }
    }
    manifest_dir.join("..").join(name)
}

pub(crate) fn sibling_repo_path(name: &str) -> PathBuf {
    sibling_repo_path_from(Path::new(env!("CARGO_MANIFEST_DIR")), name)
}

pub(crate) fn local_repo_path(name: &str) -> PathBuf {
    local_repo_path_from(Path::new(env!("CARGO_MANIFEST_DIR")), name)
}

pub(crate) fn faber_runtime_path() -> PathBuf {
    // Public `faber` repo lives beside private `radix` under faberlang/.
    sibling_repo_path("faber-runtime")
}

pub(crate) fn host_kernel_path() -> PathBuf {
    sibling_repo_path("host-kernel-rs")
}

pub(crate) fn host_native_path() -> PathBuf {
    sibling_repo_path("host-native-rs")
}

#[cfg(test)]
mod tests {
    use super::{local_repo_path_from, sibling_repo_path_from};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("faber-{label}-{nonce}"));
        fs::create_dir_all(&path).expect("temp dir");
        path
    }

    #[test]
    fn local_repo_path_prefers_nearest_worktree_sibling() {
        let root = temp_dir("cargo-local-repo-path");
        let worktree = root.join("worktrees").join("slice").join("faber-build");
        fs::create_dir_all(&worktree).expect("worktree");
        fs::create_dir_all(
            worktree
                .parent()
                .expect("worktree parent")
                .join("faber-runtime"),
        )
        .expect("worktree faber-runtime");
        fs::create_dir_all(root.join("faber-runtime")).expect("repo faber-runtime");

        assert_eq!(
            local_repo_path_from(&worktree, "faber-runtime"),
            worktree
                .parent()
                .expect("worktree parent")
                .join("faber-runtime")
        );
    }

    #[test]
    fn sibling_repo_path_prefers_canonical_repo_cluster() {
        let root = temp_dir("cargo-sibling-repo-path");
        let worktree = root.join("worktrees").join("slice").join("faber-build");
        fs::create_dir_all(&worktree).expect("worktree");
        let cluster = worktree.parent().expect("worktree parent");
        for repo in ["faber-runtime", "host-kernel-rs", "host-native-rs"] {
            fs::create_dir_all(cluster.join(repo)).expect("cluster repo");
        }
        let direct = cluster.join("faber-runtime");
        let expected = fs::canonicalize(&direct).unwrap_or_else(|_| direct.clone());

        assert_eq!(sibling_repo_path_from(&worktree, "faber-runtime"), expected);
    }
}

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
    use std::fs;

    let src_dir = layout.generated_crate_root.join("src");
    if let Err(err) = fs::create_dir_all(&src_dir) {
        return Err(Box::new(Diagnostic::io_error(&src_dir, err)));
    }

    let cargo_src = if let Some(m) = meta {
        generate_cargo_toml(m, layout.binary_name(), plan)
    } else {
        render_generated_cargo_toml(layout.binary_name(), "0.1.0", plan)
    };
    if let Err(err) = fs::write(&layout.generated_cargo_manifest, &cargo_src) {
        return Err(Box::new(Diagnostic::io_error(
            &layout.generated_cargo_manifest,
            err,
        )));
    }

    // Policy: keep an outer generated marker even when backend codegen already
    // writes its own header, because this file belongs to the package builder's
    // generated crate contract.
    let rust_code = rust_code.to_owned();
    if matches!(plan.host, Some(ManifestRustHost::Native)) {
        if let Err(err) = write_host_registration(&src_dir, plan) {
            return Err(Box::new(Diagnostic::io_error(&src_dir, err)));
        }
        if let Err(err) = write_host_manifest(&layout.generated_crate_root, plan) {
            return Err(Box::new(Diagnostic::io_error(
                &layout.generated_crate_root,
                err,
            )));
        }
    }
    let final_code = format!(
        "// Generated by faber build — do not edit by hand.\n\
         // Crate layout: target/faber/  (see plan.md)\n\
         // Run with: cargo build --manifest-path target/faber/Cargo.toml --target-dir target\n\n{}",
        rust_code
    );
    let final_code = format_package_rust_source(&final_code);
    if let Err(err) = fs::write(&layout.generated_rust_entry, final_code) {
        return Err(Box::new(Diagnostic::io_error(
            &layout.generated_rust_entry,
            err,
        )));
    }

    Ok(layout.generated_crate_root.clone())
}

fn format_package_rust_source(source: &str) -> String {
    radix::tool::format_generated_code(Target::Rust, source).unwrap_or_else(|_| source.to_owned())
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
        "    let host = host_native::NativeHost::new(kernel);\n    if let Err(error) = faber::install_host_dispatch(std::sync::Arc::new(host)) {\n        eprintln!(\"host dispatch initialization failed: {error}\");\n        std::process::exit(70);\n    }\n}\n",
    );
    std::fs::write(path, source)
}

fn write_host_manifest(root: &Path, plan: &RustRuntimePlan) -> std::io::Result<()> {
    let value = serde_json::json!({
        "manifest_version": 1,
        "providers": plan.provider_manifests,
        "required_routes": plan.non_runtime_routes,
    });
    std::fs::write(
        root.join("host-manifest.json"),
        serde_json::to_vec_pretty(&value).expect("host manifest serialization is infallible"),
    )
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
        Box::new(Diagnostic::error(format!(
            "failed to spawn cargo (ensure cargo is installed and on PATH): {e}"
        )))
    })?;

    if !status.success() {
        return Err(Box::new(Diagnostic::error(format!(
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
        Box::new(Diagnostic::error(format!(
            "failed to spawn cargo (ensure cargo is installed and on PATH): {e}"
        )))
    })?;

    Ok(status)
}
