//! In-memory `clif-jit` execution (`faber run --target clif-jit`, and the
//! default engine of a plain `faber run`).
//!
//! The same compile route as `faber build --target clif`: a package directory
//! or manifest goes through the package-MIR emitter, a lone `.fab` file through
//! the single-file compiler, and both lower to Cranelift IR behind the same
//! capability pre-pass. The configured [`radix::module::ClifEngine`] is the
//! only difference: the functions are defined in a JIT module instead of an
//! object file, so nothing is written, no linker runs and no `runtime/clif`
//! archive is built. The runtime is this binary's own `faber_rt_clif`
//! dependency, and the image resolves its imports from the explicit table
//! [`faber_rt_clif::runtime_symbols`].
//!
//! The program owns the process the way a linked one does. `run_program`
//! records the arguments, calls the JIT-compiled `faber_clif_incipit`, flushes
//! stdout and returns the exit status the program hands back; this module
//! exits with it. A trap or an uncaught failure inside the runtime flushes,
//! prints the one `error:` line and exits the process with its own sysexits
//! status, never returning here. Both exit paths are the runtime's, so the
//! stderr text and the status match a built `--target clif` executable.
//!
//! Two routes share the compile: the explicit `--target clif-jit` reports a
//! decline itself and never falls back, while the no-`--target` default
//! (operator ruling 2026-10-29) treats any compile-time decline as "run it on
//! the runner instead" — the fallback is decided only from that compile-time
//! result, before any program code has run.

use std::convert::Infallible;
use std::path::{Path, PathBuf};

use radix::module::codegen::Target;
use radix::module::{ClifEngine, ClifRuntimeTable, CompileResult, Diagnostic, Output};

use super::cmd::{package_compile_result, use_package_compiler};

/// One clif-jit lane compile, before any printing or process exit.
enum JitCompile {
    /// The lowering accepted the program: the image is ready and the
    /// compile's diagnostics (warnings) belong to the engine that runs it.
    Accepted {
        diagnostics: Vec<Diagnostic>,
        image: radix::module::ClifJitOutput,
        locale_pack: Option<radix::module::locale::LocalePack>,
    },
    /// The compile declined the program (a named capability refusal, or any
    /// other compile-time failure); its diagnostics are carried, not printed.
    Declined {
        diagnostics: Vec<Diagnostic>,
        locale_pack: Option<radix::module::locale::LocalePack>,
    },
    /// The lane config itself failed (for example an unknown locale).
    ConfigFailed(Box<Diagnostic>),
}

/// Compile `input` through the clif-jit lane. Neither prints nor exits: the
/// explicit target and the default engine disagree about what a decline
/// means, so the decision and the reporting stay with the caller.
fn compile_jit(input: &Path, locale: Option<&str>, diagnostics_locale: Option<&str>) -> JitCompile {
    let target = Target::MirClif;
    let is_package = use_package_compiler(target, input, false);
    let input_path: PathBuf = if is_package {
        crate::package::absolutize_path(input)
    } else {
        input.to_path_buf()
    };
    let (config, locale_pack) =
        match crate::package::config_with_locale(target, &input_path, locale, diagnostics_locale) {
            Ok(pair) => pair,
            Err(diagnostic) => return JitCompile::ConfigFailed(diagnostic),
        };
    let config = config.with_clif_engine(ClifEngine::Jit(ClifRuntimeTable::new(
        faber_rt_clif::runtime_symbols(),
    )));
    let result: CompileResult = if is_package {
        package_compile_result(&config, &input_path, target)
    } else {
        radix::module::Compiler::new(config).compile(&input_path)
    };
    match result.output {
        Some(Output::ClifJit(image)) => JitCompile::Accepted {
            diagnostics: result.diagnostics,
            image,
            locale_pack,
        },
        _ => JitCompile::Declined {
            diagnostics: result.diagnostics,
            locale_pack,
        },
    }
}

/// `faber run --target clif-jit`: compile `input` in memory, run it with
/// `program_args` and exit with the program's own status.
///
/// A compile failure, including a shape the CLIF subset declines, prints the
/// diagnostics and exits exactly as `faber build --target clif` does. The
/// explicit target never falls back.
pub fn cmd_run_clif_jit(
    input: &Path,
    locale: Option<&str>,
    diagnostics_locale: Option<&str>,
    program_args: &[String],
    diagnostic_mode: radix::services::DiagnosticMode,
) -> ! {
    match compile_jit(input, locale, diagnostics_locale) {
        JitCompile::ConfigFailed(diagnostic) => crate::package::build_recipe::report_and_exit_mode(
            std::slice::from_ref(&*diagnostic),
            None,
            diagnostic_mode,
        ),
        JitCompile::Declined {
            diagnostics,
            locale_pack,
        } => {
            crate::postprocess::print_diagnostics(
                &diagnostics,
                diagnostic_mode,
                locale_pack.as_ref(),
            );
            eprintln!("compilation failed");
            crate::exit_status::exit_for_compile_failure(&diagnostics);
        }
        JitCompile::Accepted {
            diagnostics,
            image,
            locale_pack,
        } => {
            crate::postprocess::print_diagnostics(
                &diagnostics,
                diagnostic_mode,
                locale_pack.as_ref(),
            );
            run_image(&image, program_args);
        }
    }
}

/// The clif-jit half of `faber run` without `--target`.
///
/// Compiles through the same lane as [`cmd_run_clif_jit`] and, when the
/// lowering accepts the program, runs it exactly as the explicit target does.
/// `None` means the lane declined at compile time — nothing has been printed
/// and no program code has run — and the caller must then interpret the input
/// exactly as `--target runner` does. The fallback is decided only from that
/// compile-time result: a program that accepted and then trapped or exited
/// nonzero on the JIT owns its process inside [`run_image`] and is never
/// re-run, so there is no double output and no second side effect.
///
/// `note` (the run `--debug` switch) names the engine on stderr; stdout is
/// untouched either way.
pub fn run_default_engine_or_decline(
    input: &Path,
    locale: Option<&str>,
    diagnostics_locale: Option<&str>,
    program_args: &[String],
    note: bool,
) -> Option<Infallible> {
    let JitCompile::Accepted { image, .. } = compile_jit(input, locale, diagnostics_locale) else {
        return None;
    };
    if note {
        eprintln!("debug: engine: clif-jit (default)");
    }
    // The compile's diagnostics (warnings) stay behind the explicit target:
    // the plain run's diagnostics surface has always been the run's own, and
    // the lanes warn differently (an accepted program's stderr must stay the
    // program's, so a trap still matches the runner byte for byte).
    run_image(&image, program_args);
}

/// Hand the process to the JIT image and exit with the program's own status.
///
/// # Panics (never returns)
/// `run_program` exits the process on every path, so this never returns.
fn run_image(image: &radix::module::ClifJitOutput, program_args: &[String]) -> ! {
    // SAFETY: `incipit` is this program's own `faber_clif_incipit`, a
    // `() -> i32` C function, and `image` stays alive across the call (the
    // call exits the process or returns the status).
    let status =
        unsafe { faber_rt_clif::run_program(program_args.to_vec(), image.program.incipit()) };
    std::process::exit(status);
}
