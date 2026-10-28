//! `faber field-modifier-codemod` — insert `varia` (en `var`) before every
//! unmarked `genus` field (DECISIONS D16.1, D16.4(1)).
//!
//! The rewrite is span-based (`radix::services::insert_field_modifiers`): each
//! file gets its own reader pack's spelling and every other byte stays
//! identical. `faber format` is not used because HIR re-emission is lossy
//! for a migration diff. Usage:
//!
//! ```text
//! faber field-modifier-codemod <PATH>...           # report only (default), exit 1 on drift
//! faber field-modifier-codemod --check <PATH>...   # explicit spelling of the default
//! faber field-modifier-codemod --write <PATH>...   # rewrite changed files in place
//! faber field-modifier-codemod --stdout <FILE>     # print one rewritten file
//! ```

use super::format::{RewriteTally, RewriteVerb};
use std::fs;
use std::path::{Path, PathBuf};

/// Arguments for `faber field-modifier-codemod`.
#[derive(Debug, Clone)]
pub struct FieldModifierCodemodCommand {
    pub paths: Vec<PathBuf>,
    pub check: bool,
    pub stdout: bool,
    pub write: bool,
}

/// Rewrite one document with its own reader pack.
///
/// # Errors
///
/// Returns a message when the reader locale does not resolve or the source is
/// refused by the span rewriter (it does not lex or parse cleanly).
pub fn codemod_field_modifiers(
    path: &Path,
    source: &str,
) -> Result<radix::services::FieldModifierCodemod, String> {
    let session = super::format::format_session(path, None, source)
        .map_err(|diagnostic| diagnostic.message.clone())?;
    let pack = session
        .config
        .locale_pack
        .as_ref()
        .ok_or_else(|| "reader locale did not resolve".to_owned())?;
    radix::services::insert_field_modifiers(pack, source)
}

/// Run the codemod over files and directories, matching `faber format`'s
/// report/`--write`/stdout contract: report-only by default; report and
/// summary lines go to stdout, diagnostics to stderr.
pub fn cmd_field_modifier_codemod(command: &FieldModifierCodemodCommand) {
    let files = match super::format::resolve_convert_paths(&command.paths) {
        Ok(files) => files,
        Err((status, message)) => {
            eprintln!("error: {message}");
            std::process::exit(status);
        }
    };
    if files.is_empty() {
        eprintln!("error: no .fab files found for field-modifier codemod");
        faber_cli::exit_status::exit_no_input();
    }
    if command.stdout && files.len() > 1 {
        eprintln!(
            "error: --stdout accepts exactly one input file, found {}; use --check or --write for multiple files",
            files.len()
        );
        faber_cli::exit_status::exit_usage();
    }

    let report_only = command.check || !command.write;
    let mut tally = RewriteTally::new(RewriteVerb::CODEMOD);
    for path in &files {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("error: failed to read '{}': {err}", path.display());
                tally.refuse(faber_cli::exit_status::NO_INPUT);
                continue;
            }
        };
        let result = match codemod_field_modifiers(path, &source) {
            Ok(result) => result,
            Err(message) => {
                eprintln!("error: {}: {message}", path.display());
                tally.refuse(faber_cli::exit_status::DATA_ERROR);
                continue;
            }
        };
        if command.stdout {
            print!("{}", result.source);
            continue;
        }
        // One predicate for report and write: the file's bytes would change.
        if result.source == source {
            continue;
        }
        if report_only {
            println!("{}", tally.would_line(path));
            tally.changed += 1;
        } else if fs::write(path, &result.source).is_ok() {
            println!("{}", tally.applied_line(path));
            tally.changed += 1;
        } else {
            eprintln!("error: failed to write '{}'", path.display());
            tally.write_failed += 1;
        }
    }

    if command.stdout {
        if tally.refused > 0 {
            std::process::exit(tally.refusal_status);
        }
        return;
    }
    let total = files.len();
    if report_only {
        println!("{}", tally.report_summary(total));
        if tally.refused > 0 {
            std::process::exit(tally.refusal_status);
        }
        if tally.changed > 0 {
            // Drift is a report, not a classed failure.
            std::process::exit(faber_cli::exit_status::FAILURE);
        }
        return;
    }
    println!("{}", tally.write_summary(total));
    if tally.refused > 0 {
        std::process::exit(tally.refusal_status);
    }
    if tally.write_failed > 0 {
        faber_cli::exit_status::exit_cant_create();
    }
}

#[cfg(test)]
#[path = "field_modifier_codemod_test.rs"]
mod tests;
