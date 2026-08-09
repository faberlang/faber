//! `faber format` — author-mode formatter (default) with locale/check/stdout,
//! the `--write`/`--stdin`/`--policy` steady-state flag surface (FORMAT-PRETTY
//! S4).

use radix::codegen::Target;
use radix::driver::{peel_raw_source, split_frontmatter, Config, Session};
use radix::forma::{compile_author_with_policy, FormatCompileResult, FormatPolicy};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Arguments for `faber format`.
#[derive(Debug, Clone)]
pub struct FormatCommand {
    pub paths: Vec<PathBuf>,
    pub locale: Option<String>,
    pub check: bool,
    pub stdout: bool,
    pub write: bool,
    pub stdin: bool,
    pub policy: Option<String>,
    pub config: Option<PathBuf>,
}

pub fn cmd_format(command: &FormatCommand) {
    if command.config.is_some() {
        eprintln!("warning: --config is not implemented yet (forma.toml deferred)");
    }

    // Policy resolution (rule-slug registry). The built-in default is
    // `normalise-v1`; the CLI `--policy` override wins over it. An unknown slug
    // fails clearly before any file is touched, with a message distinct from
    // formatting-difference output.
    let policy = match resolve_policy(command.policy.as_deref()) {
        Ok(policy) => policy,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    };

    // The locale-interplay contract: reader-locale re-emission is canonical
    // HIR output and cannot honor a format policy. Reject explicitly rather
    // than silently switching to canonical HIR re-emission.
    if command.policy.is_some() && command.locale.is_some() {
        eprintln!(
            "error: --policy cannot be combined with --locale: reader-locale re-emission emits the canonical HIR surface and does not honor a format policy; drop one of the two flags"
        );
        std::process::exit(1);
    }

    // --stdin: read exactly one source document from stdin and print the
    // formatted result to stdout (implies stdout; no path arguments — enforced
    // at the CLI parse boundary). The diagnostic source name is `<stdin>`.
    if command.stdin {
        let mut source = String::new();
        if let Err(err) = std::io::stdin().read_to_string(&mut source) {
            eprintln!("error: failed to read <stdin>: {err}");
            std::process::exit(1);
        }
        let path = Path::new("<stdin>");
        match format_single_doc(path, command.locale.as_deref(), &source, policy) {
            Ok(formatted) => {
                print!("{formatted}");
                return;
            }
            Err(()) => std::process::exit(1),
        }
    }

    // --write is the explicit spelling of the in-place default; the default
    // branch below already writes in place (the `--write`/`--stdout` conflict
    // is enforced at the CLI parse boundary).
    let _ = command.write;

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

    // FORMAT-PRETTY S4: `--stdout` is tightened to EXACTLY ONE input file.
    // The old multi-file `=== path ===` separator output is a deliberate
    // behavior change (delivery §S4); multiple files with `--stdout` fail
    // clearly instead of concatenating.
    if command.stdout && files.len() > 1 {
        eprintln!(
            "error: --stdout accepts exactly one input file, found {}; format one file at a time (or use the default in-place write / --check for multiple files)",
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

        let formatted = match format_single_doc(path, command.locale.as_deref(), &source, policy) {
            Ok(formatted) => formatted,
            Err(()) => {
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
            // Exactly one file here (multi-file `--stdout` rejected above).
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

/// Resolve the effective format policy: built-in default (`normalise-v1`) <
/// CLI `--policy` slug (validated against the radix rule-slug registry).
fn resolve_policy(slug: Option<&str>) -> Result<FormatPolicy, String> {
    match slug {
        None => Ok(FormatPolicy::default()),
        Some(slug) => FormatPolicy::from_slug(slug).map_err(|err| err.to_string()),
    }
}

/// Format ONE source document through the selected pipeline (author policy or
/// reader-locale re-emit), printing diagnostics and returning the formatted
/// output ready for write/stdout (frontmatter preserved). `Err(())` means a
/// diagnostic was already printed.
fn format_single_doc(
    path: &Path,
    locale: Option<&str>,
    source: &str,
    policy: FormatPolicy,
) -> Result<String, ()> {
    let name = path.display().to_string();
    let result = if locale.is_some() {
        #[cfg(not(feature = "hir-faber"))]
        {
            eprintln!(
                "error: localized Faber re-emission requires a faber build with feature `hir-faber`"
            );
            return Err(());
        }
        #[cfg(feature = "hir-faber")]
        {
            let session = match format_session(path, locale, source) {
                Ok(session) => session,
                Err(message) => {
                    eprintln!("error: {message}");
                    return Err(());
                }
            };
            radix::forma::compile_canonical(&session, &name, source)
        }
    } else {
        let session = match format_session(path, None, source) {
            Ok(session) => session,
            Err(message) => {
                eprintln!("error: {message}");
                return Err(());
            }
        };
        match compile_author_with_policy(&session, &name, source, policy) {
            Ok(result) => result,
            Err(err) => {
                eprintln!("error: {err}");
                return Err(());
            }
        }
    };

    let mut had_error = false;
    for diag in &result.diagnostics {
        if diag.is_error() {
            eprintln!("error: {}: {}", path.display(), diag.message);
            had_error = true;
        } else {
            eprintln!("warning: {}: {}", path.display(), diag.message);
        }
    }

    let Some(output) = result.output else {
        if !had_error {
            eprintln!("error: {}: format failed", path.display());
        }
        return Err(());
    };

    match formatted_source_for_write(path, source, &output.code) {
        Ok(formatted) => Ok(formatted),
        Err(message) => {
            eprintln!("error: {message}");
            Err(())
        }
    }
}

pub(super) fn format_session(path: &Path, locale: Option<&str>, source: &str) -> Result<Session, String> {
    // An explicit CLI locale wins. Without one, preserve an existing source
    // locale while making an untagged source use the product default (`en`).
    let frontmatter_locale = if locale.is_none() {
        let peeled = peel_raw_source(&path.display().to_string(), source)
            .map_err(|error| error.to_string())?;
        match peeled.frontmatter {
            Some(frontmatter) => match frontmatter.locale_result() {
                Some(Ok(locale)) => Some(locale.to_owned()),
                Some(Err(())) => {
                    return Err("frontmatter 'locale' key must be a string".to_owned());
                }
                None => None,
            },
            None => None,
        }
    } else {
        None
    };
    let selected_locale = locale.or(frontmatter_locale.as_deref());

    // The package locale probe requires a real file path. A virtual source
    // (e.g. `--stdin`, source name `<stdin>`) has no file to probe; use the
    // dev-stdlib session directly (the author pipeline's default config for a
    // plain manifestless file).
    if !path.is_file() {
        return Ok(Session::new(
            Config::default()
                .with_target(Target::HirFaber)
                .with_dev_stdlib(),
        ));
    }

    // Locale-mode sessions need the dev stdlib path: files with
    // `+++ locale = "…" +++` frontmatter resolve their reader pack through
    // `Config::stdlib_path` (READER003 otherwise).
    crate::package::config_with_locale(Target::HirFaber, path, selected_locale, None)
        .map(|(config, _)| Session::new(config.with_dev_stdlib()))
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
