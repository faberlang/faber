//! Build-recipe machinery shared by every target that builds a program with an
//! external toolchain (`go`, `tsc`/`node`, and the native `clif` link).
//!
//! A backend ships its default recipe as data beside its runtime sources; a
//! project's `faber.toml` `[target.<t>]` states only its changes. This module
//! holds what no target decides for itself: finding a tool on `PATH` and
//! failing by name when it is missing, expanding `{placeholder}` words of a
//! command line, merging the project's `build`/`run` command replacements,
//! running the built program with the caller's streams, and reporting the
//! child's exit status back unchanged.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use radix::module::diagnostics::Diagnostic;
use radix::program::manifest::ManifestTarget;

/// The toolchain a recipe's missing-tool diagnostic speaks for.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Toolchain {
    /// Product name in the message: "Go programs require `go` on PATH...".
    pub programs: &'static str,
    /// The `issue` fact of the diagnostic.
    pub issue: &'static str,
}

/// The `build` and `run` command lines of a recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Commands {
    pub build: Vec<String>,
    pub run: Vec<String>,
}

impl Commands {
    /// Apply a project's `[target.<t>]` command replacements: only the
    /// commands it states move.
    pub(crate) fn with_project(mut self, project: Option<&ManifestTarget>) -> Self {
        if let Some(project) = project {
            if let Some(build) = &project.build {
                self.build.clone_from(build);
            }
            if let Some(run) = &project.run {
                self.run.clone_from(run);
            }
        }
        self
    }
}

/// Find `tool` on `path` (the value of a `PATH` variable).
pub(crate) fn find_tool(tool: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    if tool.contains(std::path::MAIN_SEPARATOR) {
        let direct = PathBuf::from(tool);
        return direct.is_file().then_some(direct);
    }
    std::env::split_paths(path?)
        .map(|directory| directory.join(tool))
        .find(|candidate| candidate.is_file())
}

/// Resolve the program of a command line from `PATH`, or fail naming it.
pub(crate) fn require_tool(
    argv: &[String],
    toolchain: &Toolchain,
) -> Result<PathBuf, Box<Diagnostic>> {
    require_tool_on(argv, std::env::var_os("PATH").as_deref(), toolchain)
}

/// [`require_tool`] against an explicit `PATH` value.
pub(crate) fn require_tool_on(
    argv: &[String],
    path: Option<&OsStr>,
    toolchain: &Toolchain,
) -> Result<PathBuf, Box<Diagnostic>> {
    let Some(tool) = argv.first() else {
        return Err(Box::new(
            crate::package_diagnostic_error(format!(
                "the {} recipe has an empty command",
                toolchain.programs
            ))
            .with_arg("issue", toolchain.issue),
        ));
    };
    find_tool(tool, path).ok_or_else(|| {
        Box::new(
            crate::package_diagnostic_error(format!(
                "{} programs require `{tool}` on PATH, and it was not found",
                toolchain.programs
            ))
            .with_arg("issue", toolchain.issue)
            .with_arg("tool", tool.clone()),
        )
    })
}

/// Expand the placeholders of a command line. A word that is exactly
/// `{name}` and names a list becomes that list's words (none when empty); a
/// word that contains `{name}` and names a value has it replaced in place.
pub(crate) fn expand(
    argv: &[String],
    values: &[(&str, &str)],
    lists: &[(&str, &[String])],
) -> Vec<String> {
    let mut out = Vec::with_capacity(argv.len());
    for word in argv {
        if let Some((_, items)) = lists
            .iter()
            .find(|(name, _)| word.as_str() == format!("{{{name}}}"))
        {
            out.extend(items.iter().cloned());
            continue;
        }
        let mut word = word.clone();
        for (name, value) in values {
            word = word.replace(&format!("{{{name}}}"), value);
        }
        out.push(word);
    }
    out
}

/// Run a command line with the caller's standard streams and return how it
/// ended. `extra` is appended to the command line; `env` is set for the child
/// only.
pub(crate) fn run_inherited(
    argv: &[String],
    extra: &[String],
    cwd: &Path,
    env: &[(String, String)],
    toolchain: &Toolchain,
) -> Result<ExitStatus, Box<Diagnostic>> {
    let tool = require_tool(argv, toolchain)?;
    Command::new(&tool)
        .args(&argv[1..])
        .args(extra)
        .envs(env.iter().map(|(key, value)| (key, value)))
        .current_dir(cwd)
        .status()
        .map_err(|err| {
            Box::new(
                crate::package_diagnostic_error(format!("failed to execute `{}`: {err}", argv[0]))
                    .with_arg("issue", toolchain.issue),
            )
        })
}

/// The status a shell would report: the exit code, or 128 plus the signal.
pub(crate) fn exit_code_of(status: ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    status.code().unwrap_or(1)
}

/// Whether `pack` has a reader-pack row for the package refusal `diag`.
fn pack_has_row(diag: &Diagnostic, pack: Option<&radix::module::locale::LocalePack>) -> bool {
    let (Some(pack), Some(issue)) = (pack, diag.issue()) else {
        return false;
    };
    pack.diagnostics
        .get(crate::PACKAGE_DIAGNOSTIC_CODE)
        .is_some_and(|package| package.issues.contains_key(issue))
}

pub(crate) fn report_and_exit_mode(
    diagnostics: &[Diagnostic],
    locale_pack: Option<&radix::module::locale::LocalePack>,
    mode: radix::services::DiagnosticMode,
) -> ! {
    if mode == radix::services::DiagnosticMode::Diagnostics {
        crate::postprocess::print_diagnostics(diagnostics, mode, locale_pack);
        crate::exit_status::exit_for_diagnostics(diagnostics);
    }
    for diag in diagnostics {
        if diag.code == Some(crate::PACKAGE_DIAGNOSTIC_CODE) && !pack_has_row(diag, locale_pack) {
            eprintln!("error: {}", diag.message);
        } else {
            crate::postprocess::print_diagnostics(
                std::slice::from_ref(diag),
                radix::services::DiagnosticMode::Normal,
                locale_pack,
            );
        }
    }
    crate::exit_status::exit_for_diagnostics(diagnostics);
}

pub(crate) fn print_run_explain_footer(diagnostics: &[Diagnostic]) {
    if let Some(first_error) = diagnostics.iter().find(|diagnostic| diagnostic.is_error())
        && let (Some(code), Some(issue)) = (first_error.code, first_error.issue())
    {
        eprintln!("explain: faber explain {code}.{issue}");
    }
}

pub(crate) fn report_run_and_exit_mode(
    diagnostics: &[Diagnostic],
    locale_pack: Option<&radix::module::locale::LocalePack>,
    mode: radix::services::DiagnosticMode,
) -> ! {
    crate::postprocess::print_diagnostics(diagnostics, mode, locale_pack);
    print_run_explain_footer(diagnostics);
    crate::exit_status::exit_for_diagnostics(diagnostics);
}

/// A file name safe for a built artifact.
pub(crate) fn sanitize_file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "program".to_owned()
    } else {
        cleaned
    }
}

#[cfg(test)]
#[path = "build_recipe_test.rs"]
mod tests;
