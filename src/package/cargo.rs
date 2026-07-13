use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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

[workspace]
# Empty workspace table keeps this generated crate independent when the
# package lives inside the faber repository workspace tree (e.g. examples/).
# Prevents "current package believes it's in a workspace" errors for
# `cargo build/test --manifest-path target/faber/Cargo.toml`.

[dependencies]
{deps}"#,
        name = toml_string(name),
        version = toml_string(version),
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
        Diagnostic::error(format!("verified core support is unavailable: {error}"))
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
    use std::fs;

    let src_dir = layout.generated_crate_root.join("src");
    if let Err(err) = fs::create_dir_all(&src_dir) {
        return Err(Box::new(Diagnostic::io_error(&src_dir, err)));
    }

    let support = materialize().map_err(core_support_diagnostic)?;
    let cargo_src = if let Some(m) = meta {
        generate_cargo_toml(m, layout.binary_name(), plan, &support)?
    } else {
        render_generated_cargo_toml_with_support(layout.binary_name(), "0.1.0", plan, &support)?
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
