//! `faber format` — author-mode formatter (default) with canonical/check/stdout.

use radix::codegen::Target;
use radix::driver::{split_frontmatter, Config, Session};
use radix::forma::{compile_author, compile_canonical, FormatCompileResult};
use std::fs;
use std::path::{Path, PathBuf};

/// Arguments for `faber format`.
#[derive(Debug, Clone)]
pub struct FormatCommand {
    pub paths: Vec<PathBuf>,
    pub canonical: bool,
    pub reader_locale: Option<String>,
    pub check: bool,
    pub stdout: bool,
    pub config: Option<PathBuf>,
}

pub fn cmd_format(command: FormatCommand) {
    if command.config.is_some() {
        eprintln!("warning: --config is not implemented yet (forma.toml deferred)");
    }

    // --canonical is the la alias; --reader-locale=<X> drives the emitter
    // surface. Either selects the canonical re-emit path (localizing via the
    // reader pack); no flags keeps author mode.
    let use_canonical = command.canonical || command.reader_locale.is_some();

    let files = match resolve_format_paths(&command.paths) {
        Ok(files) => files,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    };

    if files.is_empty() {
        eprintln!("error: no .fab files found to format");
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

        let name = path.display().to_string();
        let result = if use_canonical {
            let session = match format_session(path, command.reader_locale.as_deref()) {
                Ok(session) => session,
                Err(message) => {
                    eprintln!("error: {message}");
                    error_count += 1;
                    continue;
                }
            };
            compile_canonical(&session, &name, &source)
        } else {
            let session = Session::new(Config::default());
            compile_author(&session, &name, &source)
        };

        if !result.diagnostics.is_empty() {
            for diag in &result.diagnostics {
                if diag.is_error() {
                    eprintln!("error: {}: {}", path.display(), diag.message);
                    error_count += 1;
                } else {
                    eprintln!("warning: {}: {}", path.display(), diag.message);
                }
            }
        }

        let Some(output) = result.output else {
            if error_count == 0 {
                eprintln!("error: {}: format failed", path.display());
                error_count += 1;
            }
            continue;
        };

        let formatted = match formatted_source_for_write(path, &source, &output.code) {
            Ok(formatted) => formatted,
            Err(message) => {
                eprintln!("error: {message}");
                error_count += 1;
                continue;
            }
        };

        if command.check {
            let original = normalize_trailing_newline(&source_for_compare(path, &source));
            if formatted != original {
                eprintln!("would reformat {}", path.display());
                drift_count += 1;
            }
            continue;
        }

        if command.stdout {
            if files.len() > 1 {
                eprintln!("=== {} ===", path.display());
            }
            print!("{formatted}");
            continue;
        }

        if fs::write(path, &formatted).is_err() {
            eprintln!("error: failed to write '{}'", path.display());
            error_count += 1;
        }
    }

    if command.check {
        if drift_count > 0 {
            eprintln!("{drift_count} file(s) would be reformatted");
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

fn format_session(path: &Path, reader_locale: Option<&str>) -> Result<Session, String> {
    if reader_locale.is_none() {
        return Ok(Session::new(Config::default()));
    }

    crate::package::config_with_reader_locale(Target::Faber, path, reader_locale)
        .map(|(config, _)| Session::new(config))
        .map_err(|diag| diag.message)
}

pub(super) fn formatted_source_for_write(
    path: &Path,
    raw: &str,
    formatted_body: &str,
) -> Result<String, String> {
    let name = path.display().to_string();
    let split = split_frontmatter(raw).map_err(|error| format!("{name}: {error}"))?;
    let body = normalize_trailing_newline(formatted_body);

    if split.frontmatter_text.is_none() {
        return Ok(body);
    }

    let body_start = split.body_byte_offset as usize;
    Ok(format!("{}{}", &raw[..body_start], body))
}

pub(super) fn source_for_compare(_path: &Path, raw: &str) -> String {
    raw.to_owned()
}

pub(super) fn normalize_trailing_newline(text: &str) -> String {
    let trimmed = text.trim_end_matches('\n');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

fn resolve_format_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let roots: Vec<PathBuf> = if paths.is_empty() {
        vec![std::env::current_dir().map_err(|err| err.to_string())?]
    } else {
        paths.to_vec()
    };

    let mut files = Vec::new();
    for root in roots {
        if root.is_file() {
            if is_fab_file(&root) {
                files.push(root);
            } else {
                return Err(format!("'{}' is not a .fab file", root.display()));
            }
        } else if root.is_dir() {
            collect_fab_files(&root, &mut files);
        } else {
            return Err(format!("'{}' does not exist", root.display()));
        }
    }

    files.sort();
    files.dedup();
    Ok(files)
}

fn collect_fab_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if should_skip_dir(&path) {
                continue;
            }
            collect_fab_files(&path, out);
        } else if is_fab_file(&path) {
            out.push(path);
        }
    }
}

fn should_skip_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "target" | ".git" | "node_modules"))
}

fn is_fab_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("fab")
}

#[allow(dead_code)]
fn format_result_success(result: &FormatCompileResult) -> bool {
    result.success()
}
