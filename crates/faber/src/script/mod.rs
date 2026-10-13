//! In-process Faber scripting via the runner (`faber run`, `-c`, REPL).
//!
//! Absorbs the former `scena` crate: analyze, lower, and interpret Faber source
//! through `radix::module::mir`. Custom hosts implement [`Host`] for diagnostics,
//! stdin, argv, and capability expansion.

#[cfg(feature = "runner")]
mod trap;

#[cfg(feature = "runner")]
use radix::module::codegen::Target;
#[cfg(feature = "runner")]
use radix::module::driver::Session;
#[cfg(feature = "runner")]
use radix::module::mir::run_source as radix_run_source;
#[cfg(feature = "runner")]
use std::panic::AssertUnwindSafe;
#[cfg(feature = "runner")]
use std::process::ExitCode;

#[cfg(all(test, feature = "runner"))]
pub use radix::module::mir::BufferHost;
#[cfg(feature = "runner")]
pub use radix::module::mir::{Host, RunSourceError, StdioHost};
#[cfg(feature = "runner")]
pub use trap::raw_exit_code;

/// Default diagnostic identity for [`run_source`].
///
/// Used by unit tests and as the default name when callers omit a path identity.
#[cfg_attr(not(test), allow(dead_code))]
#[cfg(feature = "runner")]
pub const EMBED_SOURCE_NAME: &str = "<source>";

/// Analyze, lower, and interpret Faber source; return a process-style exit code.
///
/// Normal completion yields [`ExitCode::SUCCESS`]. Explicit
/// `process.exit_process(code)` (or the `exi` builtin) yields `code` masked
/// to eight bits, matching common
/// Unix status encoding. Abort paths yield [`ExitCode::FAILURE`].
///
/// Frontend, MIR, and runner failures return [`RunSourceError`] without running
/// `exit` on the embedder process.
#[cfg_attr(not(test), allow(dead_code))]
#[cfg(feature = "runner")]
pub fn run_source(source: &str, host: &mut dyn Host) -> Result<ExitCode, RunSourceError> {
    run_named(EMBED_SOURCE_NAME, source, host)
}

/// Like [`run_source`], but uses `name` as the unit identity in diagnostics.
#[cfg(feature = "runner")]
pub fn run_named(
    name: &str,
    source: &str,
    host: &mut dyn Host,
) -> Result<ExitCode, RunSourceError> {
    let config = faber_cli::package::default_config_with_locale(Target::HirTypeScript)
        .map_err(|diagnostic| RunSourceError::Frontend(vec![*diagnostic]))?
        .with_dev_locale_root();
    let session = Session::new(config);
    run_with_session(&session, name, source, host)
}

/// Interpret source with a caller-owned [`Session`] (target and policy reuse).
#[cfg(feature = "runner")]
pub fn run_with_session(
    session: &Session,
    name: &str,
    source: &str,
    host: &mut dyn Host,
) -> Result<ExitCode, RunSourceError> {
    let mut trap_host = trap::TrapHost { inner: host };
    let mut run_error = None::<RunSourceError>;

    let exit_result = trap::run_trapped(AssertUnwindSafe(|| {
        if let Err(error) = radix_run_source(session, name, source, &mut trap_host) {
            run_error = Some(error);
        }
    }));

    if let Some(error) = run_error {
        return Err(error);
    }

    match exit_result {
        Ok(code) => Ok(code),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// Print [`RunSourceError`] diagnostics to stderr.
#[cfg(feature = "runner")]
pub fn print_run_source_error(error: &RunSourceError) {
    match error {
        RunSourceError::Frontend(diagnostics) => {
            // Same renderer as `faber check`, so a coded advisory such as
            // LOCALE002 shows its code and issue instead of bare message text.
            for diag in diagnostics {
                eprintln!("{}", radix::module::diagnostics::render_plain(diag));
            }
        }
        RunSourceError::Mir(errors) => {
            for error in errors {
                eprintln!("error: {}", error.message);
            }
        }
        RunSourceError::Stepper(errors) => {
            for error in errors {
                eprintln!("error: {}", error.message);
            }
        }
    }
}

/// Analyze, lower, and interpret Faber source on the runner.
#[cfg(feature = "runner")]
pub fn interpret_source<H: Host>(
    name: &str,
    source: &str,
    host: &mut H,
) -> Result<(), RunSourceError> {
    run_named(name, source, host).map(|_| ())
}

/// Run source via the runner, printing errors and exiting the process on failure.
#[cfg(feature = "runner")]
pub fn interpret_source_or_exit(name: &str, source: &str, host: &mut StdioHost) {
    match run_named(name, source, host) {
        Ok(status) => {
            if status != ExitCode::SUCCESS {
                std::process::exit(raw_exit_code(status));
            }
        }
        Err(error) => {
            print_run_source_error(&error);
            std::process::exit(1);
        }
    }
}
