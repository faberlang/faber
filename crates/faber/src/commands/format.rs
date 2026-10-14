//! `faber format` — canonical HIR re-emit at the source's own locale
//! (frontmatter → package `[locale]` → product default `en`). Keyword and
//! type vocabulary stay on that surface. Reader-locale conversion lives in
//! `faber convert` (`commands/convert.rs`).

use radix::module::codegen::Target;
use radix::module::driver::{Config, Session, peel_raw_source, split_frontmatter};
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
            "error: --stdout accepts exactly one input file, found {}; use --check or --write for multiple files",
            files.len()
        );
        std::process::exit(1);
    }

    // Modes (mutually exclusive at the CLI parse boundary): `--stdout` prints
    // one file; `--write` rewrites changed files; everything else (the
    // default, spelled `--check`) reports drift and writes nothing.
    let report_only = command.check || !command.write;
    let mut tally = RewriteTally::new(RewriteVerb::FORMAT);

    for path in &files {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("error: failed to read '{}': {err}", path.display());
                tally.refused += 1;
                continue;
            }
        };

        let formatted = match format_single_doc(path, &source) {
            Ok(formatted) => formatted,
            Err(()) => {
                tally.refused += 1;
                continue;
            }
        };

        if command.stdout {
            // Exactly one file here (multi-file `--stdout` rejected above).
            print!("{formatted}");
            continue;
        }

        // One predicate for report and write: the file's bytes would change.
        if formatted == source_for_compare(path, &source) {
            continue;
        }

        if report_only {
            println!("{}", tally.would_line(path));
            tally.changed += 1;
        } else if fs::write(path, &formatted).is_ok() {
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

/// The words one in-place rewriter uses in its report and `--write` lines.
#[derive(Debug, Clone, Copy)]
pub(super) struct RewriteVerb {
    /// Per-file report line: `would <imperative> <path>`.
    pub(super) imperative: &'static str,
    /// Report summary: `N of M files would be <past>` (and the nothing-to-do
    /// line `M files already <applied>`).
    pub(super) past: &'static str,
    /// `--write` lines: `<applied> <path>` and `<applied> N of M files`.
    pub(super) applied: &'static str,
}

impl RewriteVerb {
    pub(super) const FORMAT: Self = Self {
        imperative: "reformat",
        past: "reformatted",
        applied: "formatted",
    };
    pub(super) const CONVERT: Self = Self {
        imperative: "convert",
        past: "converted",
        applied: "converted",
    };
    pub(super) const CODEMOD: Self = Self {
        imperative: "codemod",
        past: "codemodded",
        applied: "codemodded",
    };
}

/// Per-run counters and summary lines shared by the three in-place rewriters
/// (`format`, `convert`, `field-modifier-codemod`):
/// report by default, `--write` applies.
#[derive(Debug)]
pub(super) struct RewriteTally {
    verb: RewriteVerb,
    /// Files whose bytes would change (report) or were written (`--write`).
    pub(super) changed: usize,
    /// Files refused (unreadable, or a diagnostic was printed); never written.
    pub(super) refused: usize,
    /// Files that rewrote but could not be written.
    pub(super) write_failed: usize,
}

impl RewriteTally {
    pub(super) fn new(verb: RewriteVerb) -> Self {
        Self {
            verb,
            changed: 0,
            refused: 0,
            write_failed: 0,
        }
    }

    /// Per-file stdout line for a file that would change (report mode).
    pub(super) fn would_line(&self, path: &Path) -> String {
        format!("would {} {}", self.verb.imperative, path.display())
    }

    /// Per-file stdout line for a file that was written (`--write`).
    pub(super) fn applied_line(&self, path: &Path) -> String {
        format!("{} {}", self.verb.applied, path.display())
    }

    fn refused_clause(&self) -> String {
        if self.refused == 0 {
            String::new()
        } else {
            format!(", {} refused (nothing written)", self.refused)
        }
    }

    /// Last stdout line of report mode.
    pub(super) fn report_summary(&self, total: usize) -> String {
        if self.changed > 0 {
            format!(
                "{} of {total} files would be {}; rerun with --write to apply{}",
                self.changed,
                self.verb.past,
                self.refused_clause()
            )
        } else if self.refused > 0 {
            format!("{} refused (nothing written)", self.refused)
        } else {
            format!("{total} files already {}", self.verb.applied)
        }
    }

    /// Last stdout line of `--write` mode.
    pub(super) fn write_summary(&self, total: usize) -> String {
        let failed = if self.write_failed == 0 {
            String::new()
        } else {
            format!(", {} failed to write", self.write_failed)
        };
        format!(
            "{} {} of {total} files{}{failed}",
            self.verb.applied,
            self.changed,
            self.refused_clause()
        )
    }
}

/// Convert ONE source document to an explicit target locale surface
/// (`faber convert --to <LOCALE>`). Same HIR round-trip as the former
/// reader-locale conversion mode (canonical HIR surface).
/// `Err(())` means a diagnostic was already printed.
#[cfg(feature = "hir-faber")]
pub(super) fn convert_single_doc(
    path: &Path,
    to: &str,
    from: Option<&str>,
    source: &str,
) -> Result<String, ()> {
    let name = path.display().to_string();
    let session = match format_session(path, from, source) {
        Ok(session) => session,
        Err(diagnostic) => {
            faber_cli::postprocess::print_diagnostics(
                &[(*diagnostic).clone()],
                radix::services::DiagnosticMode::Normal,
                None,
            );
            return Err(());
        }
    };
    let emit_pack = match faber_cli::package::embedded_conversion_locale_pack(to) {
        Ok(pack) => pack,
        Err(diagnostic) => {
            faber_cli::postprocess::print_diagnostics(
                &[(*diagnostic).clone()],
                radix::services::DiagnosticMode::Normal,
                None,
            );
            return Err(());
        }
    };
    let result = radix::module::driver::reemit_faber(&session, &emit_pack, &name, source);
    let had_error = result
        .diagnostics
        .iter()
        .any(radix::module::diagnostics::Diagnostic::is_error);
    faber_cli::postprocess::print_diagnostics(
        &result.diagnostics,
        radix::services::DiagnosticMode::Normal,
        None,
    );
    if had_error {
        eprintln!(
            "conversion failed while reading {} as locale '{}' and emitting locale '{}'",
            path.display(),
            source_locale_id(&session),
            emit_pack.metadata.id,
        );
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
        Err(diagnostic) => {
            faber_cli::postprocess::print_diagnostics(
                &[(*diagnostic).clone()],
                radix::services::DiagnosticMode::Normal,
                None,
            );
            return Err(());
        }
    };
    let Some(emit_pack) = session.config.locale_pack.as_ref() else {
        // format_session always installs a resolved reader pack; reaching this
        // means the session invariant broke, so fail closed as a diagnostic.
        faber_cli::postprocess::print_diagnostics(
            &[radix::module::diagnostics::Diagnostic::error(
                "format session did not resolve a source locale pack",
            )
            .with_code("LOCALE003")
            .with_arg("issue", "unresolved")
            .with_file(name)],
            radix::services::DiagnosticMode::Normal,
            None,
        );
        return Err(());
    };
    let result = radix::module::driver::reemit_faber(&session, emit_pack, &name, source);

    let had_error = result
        .diagnostics
        .iter()
        .any(radix::module::diagnostics::Diagnostic::is_error);
    faber_cli::postprocess::print_diagnostics(
        &result.diagnostics,
        radix::services::DiagnosticMode::Normal,
        None,
    );

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
    source_override: Option<&str>,
    source: &str,
) -> Result<Session, Box<radix::module::diagnostics::Diagnostic>> {
    let name = path.display().to_string();
    let peeled = peel_raw_source(&name, source)
        .map_err(|error| Box::new(radix::module::driver::source_load_diagnostic(&name, &error)))?;
    let frontmatter_locale = match peeled
        .frontmatter
        .as_ref()
        .and_then(|frontmatter| frontmatter.locale_result())
    {
        Some(Ok(locale)) => Some(locale.to_owned()),
        Some(Err(())) if source_override.is_none() => {
            return Err(Box::new(
                radix::module::diagnostics::Diagnostic::error(
                    "frontmatter 'locale' key must be a string",
                )
                .with_phase(radix::module::diagnostics::DiagnosticPhase::Lex)
                .with_code("LOCALE003")
                .with_arg("issue", "not_a_string")
                .with_file(name),
            ));
        }
        Some(Err(())) => None,
        None => None,
    };
    let selected_locale = source_override.or(frontmatter_locale.as_deref());
    let pack = if path.is_file() {
        faber_cli::package::load_locale_pack_for_input(path, selected_locale)?
    } else {
        faber_cli::package::locale_pack_for_emit_with_diagnostic(
            &[],
            Some(selected_locale.unwrap_or(faber_cli::package::DEFAULT_CODE_LOCALE)),
        )?
    }
    .ok_or_else(|| {
        Box::new(
            radix::module::diagnostics::Diagnostic::error("reader locale did not resolve")
                .with_code("LOCALE003")
                .with_arg("issue", "unresolved")
                .with_file(name),
        )
    })?;

    Ok(Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_resolved_locale_pack(pack)
            .with_dev_locale_root(),
    ))
}

fn source_locale_id(session: &Session) -> &str {
    session
        .config
        .locale_pack
        .as_ref()
        .map_or("<unresolved>", |pack| pack.metadata.id.as_str())
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
    rewrite_frontmatter_locale(&formatted, target_locale)
}

/// Replace or insert the locale key in a source frontmatter block. The
/// conversion output always declares the target surface.
fn rewrite_frontmatter_locale(source: &str, locale: &str) -> Result<String, String> {
    let split = split_frontmatter(source).map_err(|error| error.to_string())?;
    let Some(frontmatter) = split.frontmatter_text else {
        return Ok(format!("+++\nlocale = \"{locale}\"\n+++\n{source}"));
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
            updated.push('"');
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

    if !found {
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str("locale = \"");
        updated.push_str(locale);
        updated.push_str("\"\n");
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
