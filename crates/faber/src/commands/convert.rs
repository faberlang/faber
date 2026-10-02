//! `faber convert` — explicit reader-locale source conversion.
//!
//! `faber convert <path> --to <LOCALE>` re-emits a Faber source file in the
//! target reader-locale surface (HIR-backed round-trip — the machinery the
//! former `faber format --locale` mode used). Conversion is never implicit:
//! `--to` is required, unknown locales fail closed listing embedded packs.
//! Report-only unless `--write` is given.
//! See `docs/operator/convert-command.md`.

use super::format::{RewriteTally, RewriteVerb};
use faber_cli::package::available_locales;
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
    pub write: bool,
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
            "error: --stdout accepts exactly one input file, found {}; use --check or --write for multiple files",
            files.len()
        );
        std::process::exit(1);
    }

    // Modes (mutually exclusive at the CLI parse boundary): `--stdout` prints
    // one file; `--write` rewrites changed files; everything else (the
    // default, spelled `--check`) reports drift and writes nothing.
    let report_only = command.check || !command.write;
    let mut tally = RewriteTally::new(RewriteVerb::CONVERT);

    for path in &files {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("error: failed to read '{}': {err}", path.display());
                tally.refused += 1;
                continue;
            }
        };

        let converted =
            match convert_single_doc(path, &command.to, command.from.as_deref(), &source) {
                Ok(converted) => converted,
                Err(()) => {
                    tally.refused += 1;
                    continue;
                }
            };

        if command.stdout {
            print!("{converted}");
            continue;
        }

        // One predicate for report and write: the file's bytes would change.
        if converted == source {
            continue;
        }

        if report_only {
            println!("{}", tally.would_line(path));
            tally.changed += 1;
        } else if fs::write(path, &converted).is_ok() {
            // Transactional in place: the write happens only after the target
            // surface was emitted successfully (same contract as the corpus
            // tool).
            println!("{}", tally.applied_line(path));
            tally.changed += 1;
        } else {
            eprintln!("error: failed to write '{}'", path.display());
            tally.write_failed += 1;
        }
    }

    if command.stdout {
        if tally.refused > 0 {
            std::process::exit(1);
        }
        return;
    }

    let total = files.len();
    if report_only {
        println!("{}", tally.report_summary(total));
        if tally.changed + tally.refused > 0 {
            std::process::exit(1);
        }
        return;
    }
    println!("{}", tally.write_summary(total));
    if tally.refused + tally.write_failed > 0 {
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

#[cfg(test)]
#[path = "convert_inf_test.rs"]
mod inf_tests;
