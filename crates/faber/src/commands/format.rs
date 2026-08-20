//! `faber format` — canonical HIR re-emit at the source's own locale
//! (frontmatter → package `[locale]` → product default `en`). Keyword and
//! type vocabulary stay on that surface. Reader-locale conversion lives in
//! `faber convert` (`commands/convert.rs`).

use radix_module::codegen::Target;
use radix_module::driver::{peel_raw_source, split_frontmatter, Config, Session};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Arguments for `faber format`.
#[derive(Debug, Clone)]
pub struct FormatCommand {
    pub paths: Vec<PathBuf>,
    pub check: bool,
    pub stdout: bool,
    pub write: bool,
    pub stdin: bool,
}

pub fn cmd_format(command: &FormatCommand) {
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
        match format_single_doc(path, &source) {
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
    let _write = command.write;

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

    // `--stdout` is tightened to EXACTLY ONE input file. Multiple files with
    // `--stdout` fail clearly instead of concatenating.
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

        let formatted = match format_single_doc(path, &source) {
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

/// Convert ONE source document to an explicit target locale surface
/// (`faber convert --to <LOCALE>`). Same HIR round-trip as the former
/// `format --locale` mode (canonical HIR surface).
/// `Err(())` means a diagnostic was already printed.
#[cfg(feature = "hir-faber")]
pub(super) fn convert_single_doc(
    path: &Path,
    to: &str,
    from: Option<&str>,
    source: &str,
) -> Result<String, ()> {
    let name = path.display().to_string();
    let source_for_compile = match source_with_locale_override(source, from) {
        Ok(source) => source,
        Err(message) => {
            eprintln!("error: {message}");
            return Err(());
        }
    };
    let session = match format_session(path, Some(to), &source_for_compile) {
        Ok(session) => session,
        Err(message) => {
            eprintln!("error: {message}");
            return Err(());
        }
    };
    let result =
        radix_module::codegen::faber::compile_canonical(&session, &name, &source_for_compile);
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
            eprintln!("error: {}: conversion failed", path.display());
        }
        return Err(());
    };
    match converted_source_for_write(path, source, &output.code, to) {
        Ok(converted) => Ok(converted),
        Err(message) => {
            eprintln!("error: {message}");
            Err(())
        }
    }
}

/// Format ONE source document through canonical HIR emit at the source's own
/// locale, printing diagnostics and returning the formatted output ready for
/// write/stdout (frontmatter preserved). `Err(())` means a diagnostic was
/// already printed.
#[cfg(not(feature = "hir-faber"))]
fn format_single_doc(_path: &Path, _source: &str) -> Result<String, ()> {
    eprintln!("error: formatting requires a faber build with feature `hir-faber`");
    Err(())
}

#[cfg(not(feature = "hir-faber"))]
pub(super) fn convert_single_doc(
    _path: &Path,
    _to: &str,
    _from: Option<&str>,
    _source: &str,
) -> Result<String, ()> {
    eprintln!("error: reader-locale conversion requires a faber build with feature `hir-faber`");
    Err(())
}

#[cfg(feature = "hir-faber")]
fn format_single_doc(path: &Path, source: &str) -> Result<String, ()> {
    let name = path.display().to_string();
    let session = match format_session(path, None, source) {
        Ok(session) => session,
        Err(message) => {
            eprintln!("error: {message}");
            return Err(());
        }
    };
    let result = radix_module::codegen::faber::compile_canonical(&session, &name, source);

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

pub(super) fn format_session(
    path: &Path,
    locale: Option<&str>,
    source: &str,
) -> Result<Session, String> {
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
    // (e.g. `--stdin`, source name `<stdin>`) has no package context, so load
    // the selected installed pack directly. The source locale still wins for
    // parsing through frontmatter; this pack is also the target emit surface.
    if !path.is_file() {
        let virtual_locale = selected_locale.unwrap_or("en");
        let pack = crate::package::locale_pack_for_emit_with_diagnostic(&[], Some(virtual_locale))
            .map_err(|diagnostic| diagnostic.message.clone())?
            .ok_or_else(|| format!("reader locale '{virtual_locale}' did not resolve to a pack"))?;
        return Ok(Session::new(
            Config::default()
                .with_target(Target::HirFaber)
                .with_locale_pack(pack)
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

/// Write converted output with a frontmatter locale that matches its target
/// body surface. Ordinary `format` deliberately preserves the source prefix;
/// conversion is the one path that changes the code locale.
pub(super) fn converted_source_for_write(
    path: &Path,
    raw: &str,
    formatted_body: &str,
    target_locale: &str,
) -> Result<String, String> {
    let formatted = formatted_source_for_write(path, raw, formatted_body)?;
    rewrite_frontmatter_locale(&formatted, target_locale, false)
}

/// Replace or insert the locale key in a source frontmatter block. The
/// `insert_if_missing` mode is used only for the temporary source passed to
/// canonical compilation when `--from` overrides a missing or stale key.
fn rewrite_frontmatter_locale(
    source: &str,
    locale: &str,
    insert_if_missing: bool,
) -> Result<String, String> {
    let split = split_frontmatter(source).map_err(|error| error.to_string())?;
    let Some(frontmatter) = split.frontmatter_text else {
        if insert_if_missing {
            return Ok(format!("+++\nlocale = \"{locale}\"\n+++\n{source}"));
        }
        return Ok(source.to_owned());
    };

    let mut updated = String::with_capacity(frontmatter.len());
    let mut found = false;
    for line in frontmatter.split_inclusive('\n') {
        let line_without_newline = line.strip_suffix('\n').unwrap_or(line);
        let content = line_without_newline
            .strip_suffix('\r')
            .unwrap_or(line_without_newline);
        let trimmed = content.trim_start();
        let is_locale_key = trimmed
            .strip_prefix("locale")
            .is_some_and(|rest| rest.trim_start().starts_with('='));
        if is_locale_key {
            let indent_len = content.len() - trimmed.len();
            updated.push_str(&content[..indent_len]);
            updated.push_str("locale = \"");
            updated.push_str(locale);
            updated.push_str("\"");
            if line_without_newline.ends_with('\r') {
                updated.push('\r');
            }
            if line.ends_with('\n') {
                updated.push('\n');
            }
            found = true;
        } else {
            updated.push_str(line);
        }
    }

    if !found && insert_if_missing {
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str("locale = \"");
        updated.push_str(locale);
        updated.push_str("\"\n");
    }
    if !found && !insert_if_missing {
        return Ok(source.to_owned());
    }

    let frontmatter_start = source
        .find('\n')
        .map(|index| index + 1)
        .ok_or_else(|| "frontmatter opening delimiter is unterminated".to_owned())?;
    let frontmatter_end = frontmatter_start + frontmatter.len();
    let body_start = split.body_byte_offset as usize;
    Ok(format!(
        "{}{}{}{}",
        &source[..frontmatter_start],
        updated,
        &source[frontmatter_end..body_start],
        &source[body_start..]
    ))
}

fn source_with_locale_override(source: &str, from: Option<&str>) -> Result<String, String> {
    let Some(from) = from else {
        return Ok(source.to_owned());
    };
    rewrite_frontmatter_locale(source, from, true)
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

pub(super) fn resolve_convert_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    resolve_format_paths(paths)
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
