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
use radix::module::diagnostics::Diagnostic;
#[cfg(feature = "runner")]
use radix::module::driver::Session;
#[cfg(feature = "runner")]
use radix::module::mir::run_source as radix_run_source;
#[cfg(feature = "runner")]
use radix::program::mir::DeviceSelection;
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
    run_with_session(&session, name, source, host, DeviceSelection::Auto)
}

/// Interpret source with a caller-owned [`Session`] (target and policy reuse).
#[cfg(feature = "runner")]
pub fn run_with_session(
    session: &Session,
    name: &str,
    source: &str,
    host: &mut dyn Host,
    selection: DeviceSelection,
) -> Result<ExitCode, RunSourceError> {
    let mut trap_host = trap::TrapHost { inner: host };
    let mut run_error = None::<RunSourceError>;

    let exit_result = trap::run_trapped(AssertUnwindSafe(|| {
        // Library-importing units need provider bodies, which only the
        // package route links; the single-source runner refuses them.
        let result = if source_declares_non_kernel_import(name, source)
            || source_declares_cli_entry(name, source)
        {
            run_as_package_source(session, name, source, &mut trap_host, selection)
        } else {
            radix_run_source(session, name, source, &mut trap_host)
        };
        if let Err(error) = result {
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

/// Removes a scratch package directory when its run ends, including on the
/// `HostTrap` unwind raised by an interpreted `process.exit_process`.
#[cfg(feature = "runner")]
struct ScratchPackageDir(std::path::PathBuf);

#[cfg(feature = "runner")]
impl Drop for ScratchPackageDir {
    fn drop(&mut self) {
        // Best effort: a leftover scratch directory must not mask the run result.
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Run `source` through the package route so imported library bodies link.
///
/// An on-disk `name` is used as the entry directly; an in-memory name such as
/// [`EMBED_SOURCE_NAME`] gets a fresh scratch directory holding `main.fab`,
/// removed when the run ends.
#[cfg(feature = "runner")]
fn run_as_package_source(
    session: &Session,
    name: &str,
    source: &str,
    host: &mut dyn Host,
    selection: DeviceSelection,
) -> Result<(), RunSourceError> {
    let path = std::path::Path::new(name);
    if path.is_file() {
        return radix::program::mir::run_package_mir_with_selection(
            &session.config,
            path,
            selection,
            &faber_cli::package::mir::faber_mir_seams(),
            host,
        )
        .map_err(RunSourceError::Frontend);
    }

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let dir = std::env::temp_dir().join(format!("faber-script-{}-{}", std::process::id(), nanos));
    std::fs::create_dir(&dir)
        .map_err(|err| RunSourceError::Frontend(vec![Diagnostic::io_error(&dir, &err)]))?;
    let scratch = ScratchPackageDir(dir);
    let entry = scratch.0.join("main.fab");
    std::fs::write(&entry, source)
        .map_err(|err| RunSourceError::Frontend(vec![Diagnostic::io_error(&entry, &err)]))?;
    radix::program::mir::run_package_mir_with_selection(
        &session.config,
        &entry,
        selection,
        &faber_cli::package::mir::faber_mir_seams(),
        host,
    )
    .map_err(RunSourceError::Frontend)
    // `scratch` drops here (or while unwinding a `HostTrap`), removing `dir`.
}

/// Whether `source` declares an import outside the kernel namespace: such a
/// unit needs provider library bodies, which only the package route links.
#[cfg_attr(not(test), allow(dead_code))]
#[cfg(feature = "runner")]
pub(crate) fn source_declares_non_kernel_import(name: &str, source: &str) -> bool {
    let Ok(peeled) = radix::module::driver::peel_raw_source(name, source) else {
        return false;
    };
    // TEMPORARY STAGING: the pack-less fallback reads the
    // no-locale default while `RADIX_DEFAULT_CODE_LOCALE=en`; unset keeps the
    // historical pack-less (Latin) reading.
    let code_pack =
        faber_cli::package::default_config_with_locale(radix::module::Target::HirTypeScript)
            .ok()
            .and_then(|config| config.locale_pack)
            .or_else(|| {
                radix::module::driver::default_code_pack(&radix::module::driver::Config::default())
            });
    let lex = match code_pack.as_ref() {
        Some(pack) => radix::module::lexer::lex_with_locale_pack(peeled.body, pack),
        None => radix::module::lexer::lex(peeled.body),
    };
    let parse = radix::module::parser::parse(lex);
    let Some(program) = parse.program else {
        return false;
    };
    program.statements.iter().any(|statement| {
        let radix::module::syntax::StmtKind::Import(import) = &statement.kind else {
            return false;
        };
        !radix::module::kernel::is_kernel_import_path(parse.interner.resolve(import.path))
    })
}

/// Whether `source` declares an `@ cli` entry annotation: such a unit is a CLI
/// program whose operands and options only the package route plans.
///
/// Ruling (Fable CTO batch 2026-10-15): single-file mode must bind `@ cli`
/// operands through the same CLI analysis package mode uses, rather than run
/// them with unbound operands and silently print placeholder values.
///
/// The predicate deliberately gates only on the `@ cli` root: semantic CLI
/// analysis (`radix_semantic::cli::find_cli_stmt`) requires that annotation to
/// enter any `CliMode`, so a file carrying stray `@ option`/`@ operand`
/// annotations without `@ cli` stays a plain unit on both routes.
#[cfg_attr(not(test), allow(dead_code))]
#[cfg(feature = "runner")]
pub(crate) fn source_declares_cli_entry(name: &str, source: &str) -> bool {
    let Ok(peeled) = radix::module::driver::peel_raw_source(name, source) else {
        return false;
    };
    // TEMPORARY STAGING: the pack-less fallback reads the
    // no-locale default while `RADIX_DEFAULT_CODE_LOCALE=en`; unset keeps the
    // historical pack-less (Latin) reading.
    let code_pack =
        faber_cli::package::default_config_with_locale(radix::module::Target::HirTypeScript)
            .ok()
            .and_then(|config| config.locale_pack)
            .or_else(|| {
                radix::module::driver::default_code_pack(&radix::module::driver::Config::default())
            });
    let lex = match code_pack.as_ref() {
        Some(pack) => radix::module::lexer::lex_with_locale_pack(peeled.body, pack),
        None => radix::module::lexer::lex(peeled.body),
    };
    let parse = radix::module::parser::parse(lex);
    let Some(program) = parse.program else {
        return false;
    };
    program.statements.iter().any(|statement| {
        statement.annotations.iter().any(|annotation| {
            match &annotation.kind {
                radix::module::syntax::AnnotationKind::Cli(_) => true,
                // Annotation names are canonicalized surface→Latin by the
                // parser and `cli` is spelled `cli` on every reader surface, so
                // braced-form detection does not depend on the pack.
                radix::module::syntax::AnnotationKind::Braced(braced) => {
                    parse.interner.resolve(braced.name.name) == "cli"
                }
                _ => false,
            }
        })
    })
}

/// Print [`RunSourceError`] diagnostics to stderr.
#[cfg(feature = "runner")]
pub fn print_run_source_error(error: &RunSourceError) {
    match error {
        RunSourceError::Frontend(diagnostics) => {
            // Same renderer as `faber check`, so a coded advisory such as
            // LOCALE002 shows its code and issue instead of bare message text.
            // One LOCALE002 per file, ahead of the errors it explains.
            let ordered = radix::services::normalize_locale002_notes(diagnostics.clone());
            for diag in &ordered {
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
