//! `faber const-codemod` — swap every module-level `static` (en) / `generis`
//! (la) declaration for `const` / `fixum` (st1 R18).
//!
//! The rewrite is span-based (`radix::services::convert_static_to_const`): each
//! file gets its own reader pack's spelling and every other byte stays
//! identical. Unlike `faber format` the command also reads `.proba` files.
//! It is deleted with the lock unit (st1 U3). Usage:
//!
//! ```text
//! faber const-codemod --check <PATH>...   # dry run: report counts, exit 1 on drift
//! faber const-codemod <PATH>...           # rewrite in place
//! faber const-codemod --stdout <FILE>     # print one rewritten file
//! ```

use std::fs;
use std::path::{Path, PathBuf};

/// Arguments for `faber const-codemod`.
#[derive(Debug, Clone)]
pub struct ConstCodemodCommand {
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
pub fn codemod_constants(
    path: &Path,
    source: &str,
) -> Result<radix::services::ConstCodemod, String> {
    let session = super::format::format_session(path, None, source)
        .map_err(|diagnostic| diagnostic.message.clone())?;
    let pack = session
        .config
        .locale_pack
        .as_ref()
        .ok_or_else(|| "reader locale did not resolve".to_owned())?;
    radix::services::convert_static_to_const(pack, source)
}

/// Run the codemod over files and directories, matching the field-modifier
/// codemod's check/stdout/in-place contract. Prints one line per changed file
/// and a total on stderr.
pub fn cmd_const_codemod(command: &ConstCodemodCommand) {
    let files = match resolve_paths(&command.paths) {
        Ok(files) => files,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    };
    if files.is_empty() {
        eprintln!("error: no .fab or .proba files found for const codemod");
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
    let mut converted = 0usize;
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
        let result = match codemod_constants(path, &source) {
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
        if result.converted == 0 {
            continue;
        }
        changed_files += 1;
        converted += result.converted;
        let verb = if command.check {
            "would codemod"
        } else {
            "codemodded"
        };
        eprintln!(
            "{verb} {} ({} constant(s))",
            path.display(),
            result.converted
        );
        if !command.check && fs::write(path, &result.source).is_err() {
            eprintln!("error: failed to write '{}'", path.display());
            error_count += 1;
        }
    }

    if !command.stdout {
        eprintln!(
            "{converted} constant(s) in {changed_files} of {} file(s); {error_count} error(s)",
            files.len()
        );
    }
    if error_count > 0 || (command.check && changed_files > 0) {
        std::process::exit(1);
    }
}

/// Expand files and directories to sorted `.fab` and `.proba` files.
fn resolve_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let roots: Vec<PathBuf> = if paths.is_empty() {
        vec![std::env::current_dir().map_err(|err| err.to_string())?]
    } else {
        paths.to_vec()
    };
    let mut files = Vec::new();
    for root in roots {
        if root.is_file() {
            if !is_source_file(&root) {
                return Err(format!("'{}' is not a .fab or .proba file", root.display()));
            }
            files.push(root);
        } else if root.is_dir() {
            collect_source_files(&root, &mut files);
        } else {
            return Err(format!("'{}' does not exist", root.display()));
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !is_skipped_dir(&path) {
                collect_source_files(&path, out);
            }
        } else if is_source_file(&path) {
            out.push(path);
        }
    }
}

fn is_skipped_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "target" | ".git" | "node_modules"))
}

fn is_source_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("fab" | "proba")
    )
}

#[cfg(test)]
#[path = "const_codemod_test.rs"]
mod tests;
