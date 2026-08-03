//! `faber run` — compile and run a package, or interpret a single `.fab` file.
//!
//! POLICY: single `.fab` file → interpret; package directory → compile. Override
//! with `--interpret` / `--compile`. Shebang: `#!/usr/bin/env faber run`.
//!
//! The canonical interpreted-source command is `faber script`; this command
//! delegates to [`crate::commands::script::interpret_path`] on the interpret
//! branch. `--interpret` / `--compile` are retained until the Stage 6 clean
//! break (see `docs/factory/faber-script-runtime/stage0-baseline.md`).

use crate::cli::{FmirRunArgs, RunArgs};
use crate::input_shape::reader_locale_without_package_error;
use crate::package;
use fs2::FileExt;
use radix::codegen::Target;
use radix::diagnostics::Diagnostic;
use radix::mir::StdioHost;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;

fn should_interpret(args: &RunArgs, path: &Path) -> bool {
    if args.reader_locale.is_some() {
        return false;
    }
    if resolve_run_target(args, path) != Target::Rust {
        return false;
    }
    if args.compile {
        return false;
    }
    if args.interpret {
        return true;
    }
    super::script::is_single_fab_file(path)
}

/// Builds a package as Rust or interprets a single `.fab` file.
pub(super) fn cmd_run(args: RunArgs) {
    crate::commands::validate_deny_codes(&args.deny);
    let input_path = PathBuf::from(&args.path);
    if let Some(message) = reader_locale_without_package_error(
        args.reader_locale.as_deref(),
        &[args.path.display().to_string()],
        false,
    ) {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
    if args.interpret && args.reader_locale.is_some() {
        eprintln!("error: --reader-locale is not supported with `faber run --interpret`");
        std::process::exit(1);
    }
    let target = resolve_run_target(&args, &input_path);
    match target {
        Target::Rust => {}
        Target::Go => {
            cmd_run_go(&args);
            return;
        }
        Target::Scena => {
            cmd_run_scena(args);
            return;
        }
        Target::FmirText => {
            cmd_run_fmir_text(args);
            return;
        }
        Target::Fmir => {
            cmd_run_fmir(args);
            return;
        }
        Target::FmirBin => {
            cmd_run_fmir_bin(&args);
            return;
        }
        Target::Fhir => {
            cmd_run_fhir(args);
            return;
        }
        target => {
            eprintln!(
                "error: faber run does not support target `{}`; use `rust`, `go`, `scena`, `fhir`, `fmir-text`, `fmir`, or `fmir-bin`",
                run_target_name(target)
            );
            std::process::exit(1);
        }
    }

    if should_interpret(&args, &input_path) {
        super::script::interpret_path(&input_path, &args.args);
        return;
    }

    cmd_run_compiled(&args);
}

fn run_target_name(target: Target) -> &'static str {
    match target {
        Target::Rust => "rust",
        Target::TypeScript => "ts",
        Target::Go => "go",
        Target::Faber => "faber",
        Target::WasmText => "wasm-text",
        Target::Wasm => "wasm",
        Target::LlvmText => "llvm-text",
        Target::MetalText => "metal-text",
        Target::WgslText => "wgsl-text",
        Target::Sexp => "sexp",
        Target::Scena => "scena",
        Target::FmirText => "fmir-text",
        Target::Fmir => "fmir",
        Target::FmirBin => "fmir-bin",
        Target::Swift => "swift",
        Target::Fhir => "fhir",
    }
}

/// Resolve the run target for `faber run`: an explicit `--target` wins; else
/// the manifest `[build] target`; else the implicit portable default (FHIR →
/// FMIR). Never probes Cargo for an un-targeted package (portable default).
fn resolve_run_target(args: &RunArgs, input_path: &Path) -> Target {
    if let Some(target) = args.target {
        return Target::from(target);
    }
    let Ok(layout) = crate::package::discover_build_layout(input_path) else {
        return Target::Fhir;
    };
    if !layout.manifest_path.exists() {
        return Target::Fhir;
    }
    let Ok(manifest) = crate::package::read_manifest(&layout.manifest_path) else {
        return Target::Fhir;
    };
    crate::package::manifest_build_target(
        manifest.build.target.as_deref(),
        &layout.manifest_path,
    )
    .unwrap_or(Target::Fhir)
}

fn warn_policy_from_args(args: &RunArgs) -> radix::driver::WarnPolicy {
    radix::driver::WarnPolicy {
        deny_all_warnings: args.deny_warnings,
        deny_codes: args.deny.clone(),
    }
}

fn run_config(
    target: Target,
    input_path: &Path,
    reader_locale: Option<&str>,
    warn_policy: radix::driver::WarnPolicy,
) -> Result<radix::driver::Config, Box<Diagnostic>> {
    package::config_with_reader_locale(target, input_path, reader_locale)
        .map(|(config, _reader_pack)| config.with_warn_policy(warn_policy))
}

fn run_config_or_exit(
    target: Target,
    input_path: &Path,
    reader_locale: Option<&str>,
    warn_policy: radix::driver::WarnPolicy,
) -> radix::driver::Config {
    match run_config(target, input_path, reader_locale, warn_policy) {
        Ok(config) => config,
        Err(diag) => {
            eprintln!("error: {}", diag.message);
            std::process::exit(1);
        }
    }
}

/// G6 GO3 — package compile → go build → exec with forwarded argv.
fn cmd_run_go(args: &RunArgs) {
    let input_path = PathBuf::from(&args.path);
    let config = run_config_or_exit(
        Target::Go,
        &input_path,
        args.reader_locale.as_deref(),
        warn_policy_from_args(args),
    );
    let result = package::compile_package_go(&config, &input_path);
    super::eprint_compile_diagnostics(&result.compile_result.diagnostics);
    let Some(output) = result.compile_result.output else {
        eprintln!("compilation failed");
        std::process::exit(1);
    };
    let code = if let radix::Output::Go(go) = output {
        go.code
    } else {
        eprintln!("error: go run expected Go package output");
        std::process::exit(1);
    };
    let layout = match package::discover_build_layout(&input_path) {
        Ok(l) => l,
        Err(d) => {
            eprintln!("error: {}", d.message);
            std::process::exit(1);
        }
    };
    let go_layout = package::GoBuildLayout::from_package(&layout);
    if let Err(d) = package::emit_go_module(&go_layout, &code, &result.go_modules) {
        eprintln!("error: {}", d.message);
        std::process::exit(1);
    }
    let binary = match package::invoke_go_build(&go_layout) {
        Ok(path) => path,
        Err(d) => {
            eprintln!("error: {}", d.message);
            std::process::exit(1);
        }
    };
    match package::run_go_binary(&binary, &args.args) {
        Ok(code) => std::process::exit(code),
        Err(d) => {
            eprintln!("error: {}", d.message);
            std::process::exit(1);
        }
    }
}

fn cmd_run_scena(args: RunArgs) {
    let input_path = PathBuf::from(&args.path);
    let warn_policy = warn_policy_from_args(&args);
    let argumenta = args.args.clone();
    let mut host = StdioHost::with_argumenta(args.args);
    let config = run_config_or_exit(
        Target::Scena,
        &input_path,
        args.reader_locale.as_deref(),
        warn_policy,
    );
    let artifact = match package::build_package_mir_artifact(&config, &input_path, &argumenta) {
        Ok(artifact) => artifact,
        Err(diagnostics) => {
            super::eprint_compile_diagnostics(&diagnostics);
            eprintln!("scena artifact build failed");
            std::process::exit(1);
        }
    };
    if let Err(diagnostics) = package::run_package_mir_artifact(&config, &artifact, &mut host) {
        super::eprint_compile_diagnostics(&diagnostics);
        eprintln!("scena artifact execution failed");
        std::process::exit(1);
    }
}

fn cmd_run_fmir_text(args: RunArgs) {
    let input_path = PathBuf::from(&args.path);
    let warn_policy = warn_policy_from_args(&args);
    let mut host = StdioHost::with_argumenta(args.args);
    let config = run_config_or_exit(
        Target::FmirText,
        &input_path,
        args.reader_locale.as_deref(),
        warn_policy,
    );
    let image = match package::build_package_fmir_text_image(&config, &input_path, &[]) {
        Ok(image) => image,
        Err(diagnostics) => {
            super::eprint_compile_diagnostics(&diagnostics);
            eprintln!("fmir-text image build failed");
            std::process::exit(1);
        }
    };
    if let Err(diagnostics) = package::run_package_fmir_text_image(&image, &mut host) {
        super::eprint_compile_diagnostics(&diagnostics);
        eprintln!("fmir-text image execution failed");
        std::process::exit(1);
    }
}

fn cmd_run_fmir(args: RunArgs) {
    let input_path = PathBuf::from(&args.path);
    let warn_policy = warn_policy_from_args(&args);
    let mut host = StdioHost::with_argumenta(args.args);
    let config = run_config_or_exit(
        Target::Fmir,
        &input_path,
        args.reader_locale.as_deref(),
        warn_policy,
    );
    let image = match package::build_package_fmir_image(&config, &input_path, &[]) {
        Ok(image) => image,
        Err(diagnostics) => {
            super::eprint_compile_diagnostics(&diagnostics);
            eprintln!("fmir image build failed");
            std::process::exit(1);
        }
    };
    if let Err(diagnostics) = package::run_package_fmir_image(&image, &mut host) {
        super::eprint_compile_diagnostics(&diagnostics);
        eprintln!("fmir image execution failed");
        std::process::exit(1);
    }
}

/// Build the FHIR package envelope, load it source-free, lower to FMIR, and
/// run in-process — no Rust, no Cargo (portable default route).
fn cmd_run_fhir(args: RunArgs) {
    let input_path = PathBuf::from(&args.path);
    let warn_policy = warn_policy_from_args(&args);
    let mut host = StdioHost::with_argumenta(args.args);
    let config = run_config_or_exit(
        Target::Fhir,
        &input_path,
        args.reader_locale.as_deref(),
        warn_policy,
    );
    let artifact = match package::build_package_fhir(&config, &input_path) {
        Ok(artifact) => artifact,
        Err(diagnostics) => {
            super::eprint_compile_diagnostics(&diagnostics);
            eprintln!("fhir package build failed");
            std::process::exit(1);
        }
    };
    let loaded = match package::load_package_fhir(&artifact.package_path) {
        Ok(loaded) => loaded,
        Err(diagnostics) => {
            super::eprint_compile_diagnostics(&diagnostics);
            eprintln!("fhir package load failed");
            std::process::exit(1);
        }
    };
    if let Err(diagnostics) =
        package::run_loaded_package_fhir(&config, loaded, &artifact.root, &mut host)
    {
        super::eprint_compile_diagnostics(&diagnostics);
        eprintln!("fhir package execution failed");
        std::process::exit(1);
    }
}

fn cmd_run_fmir_bin(args: &RunArgs) {
    let input_path = PathBuf::from(&args.path);
    let config = run_config_or_exit(
        Target::FmirBin,
        &input_path,
        args.reader_locale.as_deref(),
        warn_policy_from_args(&args),
    );
    let bundle =
        match package::build_package_fmir_binary_bundle(&config, &input_path, &[], args.release) {
            Ok(bundle) => bundle,
            Err(diagnostics) => {
                super::eprint_compile_diagnostics(&diagnostics);
                eprintln!("fmir-bin bundle build failed");
                std::process::exit(1);
            }
        };
    run_executable(&bundle.entrypoint_path, &args.args);
}

pub(super) fn cmd_fmir_run_image(args: FmirRunArgs) {
    let mut host = StdioHost::with_argumenta(args.args);
    if let Err(diagnostics) = package::run_fmir_image_path(&args.image, &mut host) {
        super::eprint_compile_diagnostics(&diagnostics);
        eprintln!("fmir image execution failed");
        std::process::exit(1);
    }
}

#[cfg(test)]
fn run_scena_package_with_host<H: radix::mir::Host + ?Sized>(
    input_path: &Path,
    argumenta: &[String],
    host: &mut H,
) -> Result<(), Vec<radix::diagnostics::Diagnostic>> {
    let config = radix::driver::Config::default().with_target(Target::Scena);
    let artifact = package::build_package_mir_artifact(&config, input_path, argumenta)?;
    package::run_package_mir_artifact(&config, &artifact, host)
}

/// Lock file for the per-package generated-crate sequence, stored beside the
/// published crate (`target/faber/`) so atomic publication never replaces it.
const GENERATED_CRATE_LOCK_FILE: &str = ".faber-build.lock";

/// Exclusive per-package advisory lock covering the generated-crate emit +
/// cargo sequence (FBR-P2-004).
///
/// Mirrors `package::cargo::lock_generated_crate_build` on the same lock file
/// (`<pkg>/target/.faber-build.lock`) so `faber build` and `faber run` for the
/// same package serialize against each other. The private `package::cargo`
/// module is not reachable from `commands`, so the guard is reproduced here.
fn lock_generated_crate(
    layout: &package::BuildLayout,
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
struct GeneratedCrateLock {
    _file: std::fs::File,
}

fn cmd_run_compiled(args: &RunArgs) {
    let input_path = PathBuf::from(&args.path);

    // POLICY: `run` is package-scoped, so stale generated crates are never
    // trusted over the current Faber sources.
    let config = run_config_or_exit(
        Target::Rust,
        &input_path,
        args.reader_locale.as_deref(),
        warn_policy_from_args(args),
    );
    let result = package::compile_package(&config, &input_path);

    super::eprint_compile_diagnostics(&result.diagnostics);

    let Some(output) = result.output else {
        eprintln!("compilation failed");
        std::process::exit(1);
    };

    // EDGE: legacy entry paths still need a build layout so existing examples
    // remain runnable while package manifests become the preferred surface.
    let layout = match package::discover_build_layout(&input_path) {
        Ok(l) => l,
        Err(d) => {
            eprintln!("error: {}", d.message);
            std::process::exit(1);
        }
    };

    // FBR-P2-004: the per-package advisory lock spans the emit + cargo
    // sequence (library crates, generated crate snapshot, Cargo invocation) so
    // a concurrent `faber build` for this package cannot interleave files from
    // a different runtime plan. Released before the binary executes.
    let _build_lock = match lock_generated_crate(&layout) {
        Ok(lock) => lock,
        Err(d) => {
            eprintln!("error: {}", d.message);
            std::process::exit(1);
        }
    };

    let meta = if layout.manifest_path.exists() {
        package::read_manifest(&layout.manifest_path).ok()
    } else {
        None
    };

    let code_string = if let radix::Output::Rust(r) = output {
        r.code
    } else {
        eprintln!("error: run only supports Rust backend packages");
        std::process::exit(1);
    };

    // Match `faber build`: runtime plan + G4 native library path deps (no text-sniff).
    // Stage 3 SQLite apps failed here because run used the default plan without
    // emit_linked_library_crates, so Cargo never saw the generated path dep.
    let mut runtime_plan = match package::package_rust_runtime_plan(&config, &input_path) {
        Ok(plan) => plan,
        Err(diagnostics) => {
            super::eprint_compile_diagnostics(&diagnostics);
            eprintln!("runtime plan failed");
            std::process::exit(1);
        }
    };
    if let Some(diagnostic) =
        package::package_host_selection_diagnostic(&runtime_plan, &layout.manifest_path)
    {
        super::eprint_compile_diagnostics(&[diagnostic]);
        eprintln!("runtime plan failed");
        std::process::exit(1);
    }
    match package::emit_linked_library_crates(&layout.package_root, &layout) {
        Ok(linked) => {
            runtime_plan.library_path_deps = linked
                .into_iter()
                .map(|lib| (lib.crate_name, lib.crate_root))
                .collect();
        }
        Err(diagnostics) => {
            super::eprint_compile_diagnostics(&diagnostics);
            eprintln!("library dependency graph failed");
            std::process::exit(1);
        }
    }

    if let Err(d) = package::emit_generated_crate_with_runtime_plan(
        &layout,
        &code_string,
        meta.as_ref(),
        &runtime_plan,
    ) {
        eprintln!("error emitting: {}", d.message);
        std::process::exit(1);
    }

    let binary = match package::invoke_cargo_build(&layout, args.release) {
        Ok(b) => b,
        Err(d) => {
            eprintln!("error: {}", d.message);
            std::process::exit(1);
        }
    };

    // Release the emit/cargo lock before executing the compiled program so a
    // long-running binary never blocks a concurrent rebuild of the package.
    drop(_build_lock);

    // CONTRACT: `faber run` behaves like the compiled program for callers that
    // depend on argv forwarding and process status.
    run_executable(&binary, &args.args);
}

fn run_executable(binary: &Path, args: &[String]) {
    let status = Command::new(binary)
        .args(args)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("error: failed to execute {}: {}", binary.display(), e);
            std::process::exit(1);
        });

    if let Some(code) = status.code() {
        std::process::exit(code);
    } else {
        std::process::exit(1);
    }
}

#[cfg(test)]
#[path = "run_test.rs"]
mod tests;
