//! In-memory `clif-jit` execution (`faber run --target clif-jit`).
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

use std::path::{Path, PathBuf};

use radix::module::codegen::Target;
use radix::module::{ClifEngine, ClifRuntimeTable, Output};

use super::cmd::{package_compile_result, use_package_compiler};

/// `faber run --target clif-jit`: compile `input` in memory, run it with
/// `program_args` and exit with the program's own status.
///
/// A compile failure, including a shape the CLIF subset declines, prints the
/// diagnostics and exits exactly as `faber build --target clif` does.
pub fn cmd_run_clif_jit(
    input: &Path,
    locale: Option<&str>,
    diagnostics_locale: Option<&str>,
    program_args: &[String],
    diagnostic_mode: radix::services::DiagnosticMode,
) -> ! {
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
            Err(diagnostic) => crate::package::build_recipe::report_and_exit_mode(
                std::slice::from_ref(&*diagnostic),
                None,
                diagnostic_mode,
            ),
        };
    let config = config.with_clif_engine(ClifEngine::Jit(ClifRuntimeTable::new(
        faber_rt_clif::runtime_symbols(),
    )));
    let result = if is_package {
        package_compile_result(&config, &input_path, target)
    } else {
        radix::module::Compiler::new(config).compile(&input_path)
    };
    crate::postprocess::print_diagnostics(
        &result.diagnostics,
        diagnostic_mode,
        locale_pack.as_ref(),
    );
    let Some(output) = result.output else {
        eprintln!("compilation failed");
        crate::exit_status::exit_for_compile_failure(&result.diagnostics);
    };
    let Output::ClifJit(image) = output else {
        eprintln!("clif-jit compilation produced no image");
        crate::exit_status::exit_software();
    };
    // SAFETY: `incipit` is this program's own `faber_clif_incipit`, a
    // `() -> i32` C function, and `image` stays alive across the call (the
    // call exits the process or returns the status).
    let status =
        unsafe { faber_rt_clif::run_program(program_args.to_vec(), image.program.incipit()) };
    std::process::exit(status);
}
