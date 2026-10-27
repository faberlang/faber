//! Process exit statuses of the `faber` tool.
//!
//! The tool says which class of failure ended it. The classes are the
//! `sysexits.h` ones the operator chose (ruling 2026-10-27, "mechanical and
//! explicit"):
//!
//! | Status | Name | Meaning |
//! | --- | --- | --- |
//! | 0 | success | the command did what was asked (warnings alone are success) |
//! | 1 | [`FAILURE`] | a failure with no known class |
//! | 64 | [`USAGE`] | bad command-line arguments |
//! | 65 | [`DATA_ERROR`] | the source has error diagnostics |
//! | 78 | [`CONFIG`] | a bad `faber.toml` manifest key, or a locale pack that fails to load |
//!
//! These are the tool's own outcome statuses. What a compiled or interpreted
//! Faber program exits with when it traps or calls `exitus` is the program's
//! status and passes through unchanged; it is never one of these.

use radix::module::diagnostics::{Diagnostic, DiagnosticPhase};

/// A failure with no known class.
pub const FAILURE: i32 = 1;

/// Bad command-line arguments (`EX_USAGE`).
pub const USAGE: i32 = 64;

/// The source has error diagnostics (`EX_DATAERR`).
pub const DATA_ERROR: i32 = 65;

/// A bad `faber.toml` manifest key or a locale pack that fails to load
/// (`EX_CONFIG`).
pub const CONFIG: i32 = 78;

/// Fact name that classes a package-layer diagnostic. The value is
/// [`CLASS_CONFIG`] or [`CLASS_USAGE`].
pub const CLASS_ARG: &str = "exit_class";
/// Diagnostic arg the runner sets to a trap's sysexits class (see
/// `radix_runtime_contract::trap`).
const TRAP_STATUS_ARG: &str = "exit_status";

/// [`CLASS_ARG`] value for a manifest or locale-pack failure.
pub const CLASS_CONFIG: &str = "config";

/// [`CLASS_ARG`] value for a bad-argument failure.
pub const CLASS_USAGE: &str = "usage";

/// Build a `PKG001` diagnostic for a bad manifest key or a locale pack that
/// fails to load.
pub fn config_diagnostic_error(message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message).with_arg(CLASS_ARG, CLASS_CONFIG)
}

/// Build a `PKG001` diagnostic for a bad command-line argument.
pub fn usage_diagnostic_error(message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message).with_arg(CLASS_ARG, CLASS_USAGE)
}

/// The exit status a failure diagnostic maps to.
///
/// - A diagnostic tagged [`CLASS_ARG`] takes that class.
/// - A diagnostic whose file is a `faber.toml` manifest is [`CONFIG`]; the
///   compiler-side manifest reader tags nothing, it names the file.
/// - A diagnostic from a compiler phase (lex through codegen) is
///   [`DATA_ERROR`]: the source is the input that failed.
/// - Anything else (filesystem failures, tool failures) is [`FAILURE`].
#[must_use]
pub fn status_for_diagnostic(diag: &Diagnostic) -> i32 {
    classify(diag, true)
}

/// [`status_for_diagnostic`] for a failure that may come from running the
/// program: a MIR or codegen phase diagnostic can be a run-time fault the
/// runner reported (a program's own trap keeps the program's status), so only
/// the front-end phases count as [`DATA_ERROR`] here.
#[must_use]
pub fn status_for_run_diagnostic(diag: &Diagnostic) -> i32 {
    classify(diag, false)
}

fn classify(diag: &Diagnostic, back_end_is_source: bool) -> i32 {
    // A run-time trap carries the class the shared trap table chose for it
    // (radix-runtime-contract); that wins over the tool's own phase rules.
    if let Some(status) = diag
        .args
        .iter()
        .find(|arg| arg.name == TRAP_STATUS_ARG)
        .and_then(|arg| arg.value.parse::<i32>().ok())
    {
        return status;
    }
    let tagged = diag.args.iter().find(|arg| arg.name == CLASS_ARG);
    match tagged.map(|arg| arg.value.as_str()) {
        Some(CLASS_CONFIG) => return CONFIG,
        Some(CLASS_USAGE) => return USAGE,
        _ => {}
    }
    if std::path::Path::new(&diag.file)
        .file_name()
        .is_some_and(|name| name == "faber.toml")
    {
        return CONFIG;
    }
    match diag.phase {
        DiagnosticPhase::Io | DiagnosticPhase::Tool => FAILURE,
        DiagnosticPhase::Mir | DiagnosticPhase::Codegen if !back_end_is_source => FAILURE,
        DiagnosticPhase::Lex
        | DiagnosticPhase::Parse
        | DiagnosticPhase::Resolve
        | DiagnosticPhase::Lower
        | DiagnosticPhase::Typecheck
        | DiagnosticPhase::Analysis
        | DiagnosticPhase::Mir
        | DiagnosticPhase::Codegen => DATA_ERROR,
    }
}

fn fold(statuses: &[i32]) -> i32 {
    [USAGE, CONFIG, DATA_ERROR]
        .into_iter()
        .find(|class| statuses.contains(class))
        .unwrap_or(FAILURE)
}

/// The exit status for a set of failure diagnostics: the first class found
/// among the error diagnostics, in the order usage, config, source data;
/// [`FAILURE`] when none carries a class.
#[must_use]
pub fn status_for_diagnostics(diagnostics: &[Diagnostic]) -> i32 {
    let statuses: Vec<i32> = diagnostics
        .iter()
        .filter(|diag| diag.is_error())
        .map(status_for_diagnostic)
        .collect();
    fold(&statuses)
}

/// [`status_for_diagnostics`] over [`status_for_run_diagnostic`].
#[must_use]
pub fn status_for_run_diagnostics(diagnostics: &[Diagnostic]) -> i32 {
    let statuses: Vec<i32> = diagnostics
        .iter()
        .filter(|diag| diag.is_error())
        .map(status_for_run_diagnostic)
        .collect();
    fold(&statuses)
}

/// The exit status for a compile that produced no output: the class the error
/// diagnostics carry, else [`DATA_ERROR`]. The compile refused, so the source
/// is the input that failed even when no diagnostic names a phase.
#[must_use]
pub fn status_for_compile_failure(diagnostics: &[Diagnostic]) -> i32 {
    match status_for_diagnostics(diagnostics) {
        FAILURE => DATA_ERROR,
        class => class,
    }
}

/// Exit with [`status_for_compile_failure`].
pub fn exit_for_compile_failure(diagnostics: &[Diagnostic]) -> ! {
    std::process::exit(status_for_compile_failure(diagnostics));
}

/// Exit with [`USAGE`].
pub fn exit_usage() -> ! {
    std::process::exit(USAGE);
}

/// Exit with [`DATA_ERROR`].
pub fn exit_data_error() -> ! {
    std::process::exit(DATA_ERROR);
}

/// Exit with [`CONFIG`].
pub fn exit_config() -> ! {
    std::process::exit(CONFIG);
}

/// Exit with the status [`status_for_diagnostic`] gives `diag`.
pub fn exit_for_diagnostic(diag: &Diagnostic) -> ! {
    std::process::exit(status_for_diagnostic(diag));
}

/// Exit with the status [`status_for_diagnostics`] gives `diagnostics`.
pub fn exit_for_diagnostics(diagnostics: &[Diagnostic]) -> ! {
    std::process::exit(status_for_diagnostics(diagnostics));
}

/// Exit with the status [`status_for_run_diagnostic`] gives `diag`.
pub fn exit_for_run_diagnostic(diag: &Diagnostic) -> ! {
    std::process::exit(status_for_run_diagnostic(diag));
}

/// Exit with the status [`status_for_run_diagnostics`] gives `diagnostics`.
pub fn exit_for_run_diagnostics(diagnostics: &[Diagnostic]) -> ! {
    std::process::exit(status_for_run_diagnostics(diagnostics));
}

/// Parse the tool's command line, exiting [`USAGE`] on a bad argument.
///
/// `--help` and `--version` keep clap's success exit. clap's own default for a
/// usage error is 2; the tool's class is 64.
pub fn parse_cli<T: clap::Parser>() -> T {
    T::try_parse().unwrap_or_else(|err| {
        let usage_error = err.use_stderr();
        err.print().ok();
        std::process::exit(if usage_error { USAGE } else { 0 });
    })
}

#[cfg(test)]
#[path = "exit_status_test.rs"]
mod tests;
