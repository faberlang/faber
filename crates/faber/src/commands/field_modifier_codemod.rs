//! `faber field-modifier-codemod` — insert `varia` (en `var`) before every
//! unmarked `genus` field (DECISIONS D16.1, D16.4(1)).
//!
//! The rewrite is span-based (`radix::services::insert_field_modifiers`): each
//! file gets its own reader pack's spelling and every other byte stays
//! identical. `faber format` is not used because HIR re-emission is lossy
//! for a migration diff. Usage:
//!
//! ```text
//! faber field-modifier-codemod --check <PATH>...   # dry run: report counts, exit 1 on drift
//! faber field-modifier-codemod <PATH>...           # rewrite in place
//! faber field-modifier-codemod --stdout <FILE>     # print one rewritten file
//! ```

use std::fs;
use std::path::{Path, PathBuf};

/// Arguments for `faber field-modifier-codemod`.
#[derive(Debug, Clone)]
pub struct FieldModifierCodemodCommand {
    pub paths: Vec<PathBuf>,
    pub check: bool,
    pub stdout: bool,
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
/// check/stdout/in-place contract. Prints one line per changed file and a
/// total on stderr.
pub fn cmd_field_modifier_codemod(command: &FieldModifierCodemodCommand) {
    let files = match super::format::resolve_convert_paths(&command.paths) {
        Ok(files) => files,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    };
    if files.is_empty() {
        eprintln!("error: no .fab files found for field-modifier codemod");
        std::process::exit(1);
    }
    if command.stdout && files.len() > 1 {
        eprintln!(
            "error: --stdout accepts exactly one input file, found {}",
            files.len()
        );
        std::process::exit(1);
    }

    let mut changed_files = 0usize;
    let mut inserted = 0usize;
    let mut error_count = 0usize;
    for path in &files {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("error: failed to read '{}': {err}", path.display());
                error_count += 1;
                continue;
            }
        };
        let result = match codemod_field_modifiers(path, &source) {
            Ok(result) => result,
            Err(message) => {
                eprintln!("error: {}: {message}", path.display());
                error_count += 1;
                continue;
            }
        };
        if command.stdout {
            print!("{}", result.source);
            continue;
        }
        if result.inserted == 0 {
            continue;
        }
        changed_files += 1;
        inserted += result.inserted;
        let verb = if command.check {
            "would codemod"
        } else {
            "codemodded"
        };
        eprintln!("{verb} {} ({} field(s))", path.display(), result.inserted);
        if !command.check && fs::write(path, &result.source).is_err() {
            eprintln!("error: failed to write '{}'", path.display());
            error_count += 1;
        }
    }

    if !command.stdout {
        eprintln!(
            "{inserted} field(s) in {changed_files} of {} file(s); {error_count} error(s)",
            files.len()
        );
    }
    if error_count > 0 || (command.check && changed_files > 0) {
        std::process::exit(1);
    }
}

#[cfg(test)]
#[path = "field_modifier_codemod_test.rs"]
mod tests;
