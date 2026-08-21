//! `faber convert` — explicit reader-locale source conversion.
//!
//! `faber convert <path> --to <LOCALE>` re-emits a Faber source file in the
//! target reader-locale surface (HIR-backed round-trip — the machinery the
//! former `faber format --locale` mode used). Conversion is never implicit:
//! `--to` is required, unknown locales fail closed listing embedded packs.
//! See `docs/operator/convert-command.md`.

use crate::package::available_locales;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Arguments for `faber convert`.
#[derive(Debug, Clone)]
pub struct ConvertCommand {
    pub paths: Vec<PathBuf>,
    pub to: String,
    pub from: Option<String>,
    pub check: bool,
    pub stdout: bool,
    pub stdin: bool,
}

pub fn cmd_convert(command: &ConvertCommand) {
    validate_target_locale(&command.to);

    if command.stdin {
        let mut source = String::new();
        if let Err(err) = std::io::stdin().read_to_string(&mut source) {
            eprintln!("error: failed to read <stdin>: {err}");
            std::process::exit(1);
        }
        let path = Path::new("<stdin>");
        match convert_single_doc(path, &command.to, command.from.as_deref(), &source) {
            Ok(converted) => {
                print!("{converted}");
                return;
            }
            Err(()) => std::process::exit(1),
        }
    }

    let files = match super::format::resolve_convert_paths(&command.paths) {
        Ok(files) => files,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    };

    if files.is_empty() {
        eprintln!("error: no .fab files found to convert");
        std::process::exit(1);
    }

    if command.stdout && files.len() > 1 {
        eprintln!(
            "error: --stdout accepts exactly one input file, found {}; convert one file at a time (or use the default in-place write / --check for multiple files)",
            files.len()
        );
        std::process::exit(1);
    }

    let mut drift_count = 0usize;
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

        let converted =
            match convert_single_doc(path, &command.to, command.from.as_deref(), &source) {
                Ok(converted) => converted,
                Err(()) => {
                    error_count += 1;
                    continue;
                }
            };

        if command.check {
            if converted != source {
                eprintln!("would convert {} to '{}'", path.display(), command.to);
                drift_count += 1;
            }
            continue;
        }

        if command.stdout {
            print!("{converted}");
            continue;
        }

        // Transactional in place: the write happens only after the target
        // surface was emitted successfully (same contract as the corpus tool).
        if fs::write(path, &converted).is_err() {
            eprintln!("error: failed to write '{}'", path.display());
            error_count += 1;
        }
    }

    if command.check {
        if drift_count > 0 {
            eprintln!(
                "{drift_count} file(s) would be converted to '{}'",
                command.to
            );
            std::process::exit(1);
        }
        if error_count > 0 {
            std::process::exit(1);
        }
        return;
    }

    if error_count > 0 {
        std::process::exit(1);
    }
}

fn validate_target_locale(locale: &str) {
    let available = available_locales();
    if available.iter().any(|existing| existing == locale) {
        return;
    }
    eprintln!(
        "error: unknown or unsupported reader locale '{locale}'; embedded pack locales: {}",
        available.join(" ")
    );
    std::process::exit(1);
}

/// Convert ONE source document to the target locale surface. `Err(())` means
/// a diagnostic was already printed.
fn convert_single_doc(
    path: &Path,
    to: &str,
    from: Option<&str>,
    source: &str,
) -> Result<String, ()> {
    #[cfg(not(feature = "hir-faber"))]
    {
        drop((path, to, from, source));
        eprintln!(
            "error: reader-locale conversion requires a faber build with feature `hir-faber`"
        );
        Err(())
    }
    #[cfg(feature = "hir-faber")]
    {
        super::format::convert_single_doc(path, to, from, source)
    }
}

#[cfg(test)]
#[path = "convert_test.rs"]
mod tests;
