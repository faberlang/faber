//! `faber format` — canonical HIR re-emit at the source's own locale
//! (frontmatter → package `[locale]` → product default `en`). Keyword and
//! type vocabulary stay on that surface. Reader-locale conversion lives in
//! `faber convert` (`commands/convert.rs`).

use faber_cli::exit_status;
use radix::module::codegen::Target;
use radix::module::driver::{Config, Session, peel_raw_source, split_frontmatter};
use std::fs;
use std::io::Read;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

/// Arguments for `faber format`.
#[derive(Debug, Clone)]
pub struct FormatCommand {
    pub diagnostic_mode: radix::services::DiagnosticMode,
    pub paths: Vec<PathBuf>,
    pub check: bool,
    pub stdout: bool,
    pub write: bool,
    pub stdin: bool,
    /// `--width N`; wins over the package manifest `[format] width`. `None`
    /// from both sources means no wrapping.
    pub width: Option<NonZeroU32>,
}

/// Effective wrap width for one file: `--width` > manifest `[format] width` >
/// none. `Err(status)` means a diagnostic was already printed.
pub(super) fn effective_width(
    command: &FormatCommand,
    path: &Path,
) -> Result<Option<NonZeroU32>, i32> {
    faber_cli::package::resolve_format_width(command.width, path).map_err(|diagnostic| {
        faber_cli::postprocess::print_diagnostics(
            &[(*diagnostic).clone()],
            command.diagnostic_mode,
            None,
        );
        exit_status::status_for_diagnostic(&diagnostic)
    })
}

pub fn cmd_format(command: &FormatCommand) {
    // --stdin: read exactly one source document from stdin and print the
    // formatted result to stdout (implies stdout; no path arguments — enforced
    // at the CLI parse boundary). The diagnostic source name is `<stdin>`.
    if command.stdin {
        let mut source = String::new();
        if let Err(err) = std::io::stdin().read_to_string(&mut source) {
            eprintln!("error: failed to read <stdin>: {err}");
            exit_status::exit_no_input();
        }
        let path = Path::new("<stdin>");
        // No package context on stdin: only the flag applies.
        match format_single_doc(path, &source, command.width, command.diagnostic_mode) {
            Ok(formatted) => {
                print!("{formatted}");
                return;
            }
            Err(status) => std::process::exit(status),
        }
    }

    let files = match resolve_format_paths(&command.paths) {
        Ok(files) => files,
        Err((status, message)) => {
            eprintln!("error: {message}");
            std::process::exit(status);
        }
    };

    if files.is_empty() {
        eprintln!("error: no .fab files found to format");
        exit_status::exit_no_input();
    }

    // `--stdout` is tightened to EXACTLY ONE input file. Multiple files with
    // `--stdout` fail clearly instead of concatenating.
    if command.stdout && files.len() > 1 {
        eprintln!(
            "error: --stdout accepts exactly one input file, found {}; use --check or --write for multiple files",
            files.len()
        );
        exit_status::exit_usage();
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
                tally.refuse(exit_status::NO_INPUT);
                continue;
            }
        };

        let width = match effective_width(command, path) {
            Ok(width) => width,
            Err(status) => {
                tally.refuse(status);
                continue;
            }
        };
        let formatted = match format_single_doc(path, &source, width, command.diagnostic_mode) {
            Ok(formatted) => formatted,
            Err(status) => {
                tally.refuse(status);
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
            std::process::exit(exit_status::FAILURE);
        }
        return;
    }
    println!("{}", tally.write_summary(total));
    if tally.refused > 0 {
        std::process::exit(tally.refusal_status);
    }
    if tally.write_failed > 0 {
        exit_status::exit_cant_create();
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
    /// Exit status for the refusals: the first classed refusal's status, else
    /// [`exit_status::FAILURE`].
    pub(super) refusal_status: i32,
    /// Files that rewrote but could not be written.
    pub(super) write_failed: usize,
}

impl RewriteTally {
    pub(super) fn new(verb: RewriteVerb) -> Self {
        Self {
            verb,
            changed: 0,
            refused: 0,
            refusal_status: exit_status::FAILURE,
            write_failed: 0,
        }
    }

    /// Count one refused file whose refusal has exit `status`.
    pub(super) fn refuse(&mut self, status: i32) {
        self.refused += 1;
        if self.refusal_status == exit_status::FAILURE {
            self.refusal_status = status;
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
/// `Err(status)` means a diagnostic was already printed; `status` is the exit
/// status its class gives.
#[cfg(feature = "hir-faber")]
pub(super) fn convert_single_doc(
    path: &Path,
    to: &str,
    from: Option<&str>,
    source: &str,
    diagnostic_mode: radix::services::DiagnosticMode,
) -> Result<String, i32> {
    let name = path.display().to_string();
    let mut session = match format_session(path, from, source) {
        Ok(session) => session,
        Err(diagnostic) => {
            faber_cli::postprocess::print_diagnostics(
                &[(*diagnostic).clone()],
                diagnostic_mode,
                None,
            );
            return Err(exit_status::status_for_diagnostic(&diagnostic));
        }
    };
    // Conversion remains a file-at-a-time operation: it does not claim the
    // whole-package import contract that `check` has. Keep its established
    // refusal for imported kernel calls whose mutability is only known from
    // the sibling declaration. Formatter re-emission opts into those access
    // markers separately, because it must validate and preserve source calls.
    session.config.preserve_imported_param_access_markers = false;
    let emit_pack = match faber_cli::package::embedded_conversion_locale_pack(to) {
        Ok(pack) => pack,
        Err(diagnostic) => {
            faber_cli::postprocess::print_diagnostics(
                &[(*diagnostic).clone()],
                diagnostic_mode,
                None,
            );
            return Err(exit_status::status_for_diagnostic(&diagnostic));
        }
    };
    let result = radix::module::driver::reemit_faber(&session, &emit_pack, &name, source);
    let had_error = result
        .diagnostics
        .iter()
        .any(radix::module::diagnostics::Diagnostic::is_error);
    faber_cli::postprocess::print_diagnostics(
        &result.diagnostics,
        diagnostic_mode,
        session.config.locale_pack.as_ref(),
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
        return Err(exit_status::DATA_ERROR);
    };
    match converted_source_for_write(path, source, &output.code, to) {
        Ok(converted) => Ok(converted),
        Err(message) => {
            eprintln!("error: {message}");
            Err(exit_status::DATA_ERROR)
        }
    }
}

/// Format ONE source document through canonical HIR emit at the source's own
/// locale, printing diagnostics and returning the formatted output ready for
/// write/stdout (frontmatter preserved). `Err(())` means a diagnostic was
/// already printed.
#[cfg(not(feature = "hir-faber"))]
pub(super) fn format_single_doc(
    _path: &Path,
    _source: &str,
    _width: Option<NonZeroU32>,
    _diagnostic_mode: radix::services::DiagnosticMode,
) -> Result<String, i32> {
    eprintln!("error: formatting requires a faber build with feature `hir-faber`");
    Err(exit_status::UNAVAILABLE)
}

#[cfg(not(feature = "hir-faber"))]
pub(super) fn convert_single_doc(
    _path: &Path,
    _to: &str,
    _from: Option<&str>,
    _source: &str,
    _diagnostic_mode: radix::services::DiagnosticMode,
) -> Result<String, i32> {
    eprintln!("error: reader-locale conversion requires a faber build with feature `hir-faber`");
    Err(exit_status::UNAVAILABLE)
}

///
/// With a `width`, the canonical body is then wrapped at legal token breaks
/// ([`radix_hir_faber::wrap_faber_source`]). The wrapped text is re-emitted
/// and must equal the unwrapped canonical body; a wrap that would change the
/// program is refused, never written. Without a width the output is exactly
/// the canonical re-emit.
#[cfg(feature = "hir-faber")]
pub(super) fn format_single_doc(
    path: &Path,
    source: &str,
    width: Option<NonZeroU32>,
    diagnostic_mode: radix::services::DiagnosticMode,
) -> Result<String, i32> {
    let name = path.display().to_string();
    let context_path = format_context_path(path);
    let context_name = context_path.display().to_string();
    let mut session = match format_session(&context_path, None, source) {
        Ok(session) => session,
        Err(diagnostic) => {
            faber_cli::postprocess::print_diagnostics(
                &[(*diagnostic).clone()],
                diagnostic_mode,
                None,
            );
            return Err(exit_status::status_for_diagnostic(&diagnostic));
        }
    };
    // Formatter re-emission checks imported calls against their producer's
    // parameter access markers; omitting them turns valid `mut` calls into
    // read-only refusals.
    session.config.preserve_imported_param_access_markers = true;
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
            diagnostic_mode,
            None,
        );
        return Err(exit_status::SOFTWARE);
    };
    let result = radix::module::driver::reemit_faber(&session, emit_pack, &context_name, source);

    let had_error = result
        .diagnostics
        .iter()
        .any(radix::module::diagnostics::Diagnostic::is_error);
    faber_cli::postprocess::print_diagnostics(
        &result.diagnostics,
        diagnostic_mode,
        Some(emit_pack),
    );

    let Some(output) = result.output else {
        if !had_error {
            eprintln!("error: {}: format failed", path.display());
        }
        return Err(exit_status::DATA_ERROR);
    };

    let body = match width {
        None => output.code,
        Some(width) => {
            let wrapped = radix_hir_faber::wrap_faber_source(
                &output.code,
                width.get() as usize,
                Some(emit_pack),
            );
            if wrapped != output.code
                && !wrap_preserves_program(
                    &context_path,
                    source,
                    &session,
                    emit_pack,
                    &output.code,
                    &wrapped,
                )
            {
                eprintln!(
                    "error: {}: wrapping at width {width} would change the program; nothing formatted",
                    path.display()
                );
                return Err(exit_status::DATA_ERROR);
            }
            wrapped
        }
    };

    match formatted_source_for_write(path, source, &body) {
        Ok(formatted) => Ok(formatted),
        Err(message) => {
            eprintln!("error: {message}");
            Err(exit_status::DATA_ERROR)
        }
    }
}

/// Whether `wrapped` re-emits to exactly the `canonical` body: the wrap moved
/// only whitespace between tokens and the program reads the same.
#[cfg(feature = "hir-faber")]
fn wrap_preserves_program(
    path: &Path,
    source: &str,
    session: &Session,
    emit_pack: &radix::module::locale::LocalePack,
    canonical: &str,
    wrapped: &str,
) -> bool {
    let context_path = format_context_path(path);
    let Ok(wrapped_source) = formatted_source_for_write(path, source, wrapped) else {
        return false;
    };
    let name = context_path.display().to_string();
    let again = radix::module::driver::reemit_faber(session, emit_pack, &name, &wrapped_source);
    again.output.is_some_and(|output| output.code == canonical)
}

/// Resolve an existing relative input against the CLI's working directory
/// before package/import discovery. Discovery needs an absolute path to locate
/// the package root; passing `src/main.fab` directly loses that context and
/// silently treats relative imports as unresolved.
#[cfg(feature = "hir-faber")]
fn format_context_path(path: &Path) -> PathBuf {
    if path.is_absolute() || !path.is_file() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
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
            // Import targets resolve their own frontmatter/manifest locales
            // through this session; the embedded reader packs keep that
            // resolution working on an installed binary with no workspace
            // `locale/` tree.
            .with_locale_pack_loader(faber_cli::package::embedded_locale_pack_loader())
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

/// A path argument that cannot be resolved to `.fab` files: the exit status
/// that names the class (a path that does not exist is `NO_INPUT`; a file that
/// is not a `.fab` file is a `USAGE` error) and the message.
pub(super) type PathError = (i32, String);

pub(super) fn resolve_convert_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>, PathError> {
    resolve_format_paths(paths)
}

fn resolve_format_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>, PathError> {
    let roots: Vec<PathBuf> = if paths.is_empty() {
        vec![std::env::current_dir().map_err(|err| (exit_status::NO_INPUT, err.to_string()))?]
    } else {
        paths.to_vec()
    };

    let mut files = Vec::new();
    for root in roots {
        if root.is_file() {
            if is_fab_file(&root) {
                files.push(root);
            } else {
                return Err((
                    exit_status::USAGE,
                    format!("'{}' is not a .fab file", root.display()),
                ));
            }
        } else if root.is_dir() {
            collect_fab_files(&root, &mut files);
        } else {
            return Err((
                exit_status::NO_INPUT,
                format!("'{}' does not exist", root.display()),
            ));
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
