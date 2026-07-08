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
use crate::package;
use radix::codegen::Target;
use radix::mir::StdioHost;
use std::path::{Path, PathBuf};
use std::process::Command;

fn should_interpret(args: &RunArgs, path: &Path) -> bool {
    if Target::from(args.target) != Target::Rust {
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
    let input_path = PathBuf::from(&args.path);
    match Target::from(args.target) {
        Target::Rust => {}
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
            cmd_run_fmir_bin(args);
            return;
        }
        target => {
            eprintln!(
                "error: faber run does not support target `{}`; use `rust`, `scena`, `fmir-text`, `fmir`, or `fmir-bin`",
                run_target_name(target)
            );
            std::process::exit(1);
        }
    }

    if should_interpret(&args, &input_path) {
        super::script::interpret_path(&input_path, &args.args);
        return;
    }

    cmd_run_compiled(args);
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
    }
}

fn cmd_run_scena(args: RunArgs) {
    let input_path = PathBuf::from(&args.path);
    let argumenta = args.args.clone();
    let mut host = StdioHost::with_argumenta(args.args);
    let config = radix::driver::Config::default().with_target(Target::Scena);
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
    let mut host = StdioHost::with_argumenta(args.args);
    let config = radix::driver::Config::default().with_target(Target::FmirText);
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
    let mut host = StdioHost::with_argumenta(args.args);
    let config = radix::driver::Config::default().with_target(Target::Fmir);
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

fn cmd_run_fmir_bin(args: RunArgs) {
    let input_path = PathBuf::from(&args.path);
    let config = radix::driver::Config::default().with_target(Target::FmirBin);
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

fn cmd_run_compiled(args: RunArgs) {
    let input_path = PathBuf::from(&args.path);

    // POLICY: `run` is package-scoped, so stale generated crates are never
    // trusted over the current Faber sources.
    let config = radix::driver::Config::default().with_target(radix::codegen::Target::Rust);
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

    let meta = if layout.manifest_path.exists() {
        package::read_manifest(&layout.manifest_path).ok()
    } else {
        None
    };

    let code_string = match output {
        radix::Output::Rust(r) => r.code,
        _ => {
            eprintln!("error: run only supports Rust backend packages");
            std::process::exit(1);
        }
    };

    if let Err(d) = package::emit_generated_crate(&layout, &code_string, meta.as_ref()) {
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
