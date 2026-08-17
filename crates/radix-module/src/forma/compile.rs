//! Forma format entry point: parse/analyze + forma emit (author and canonical).
//!
//! LEGACY(canonical): the "canonical" name predates reader locales — with the
//! reader-locale system there is no single canonical language surface, only
//! locale surfaces. The HIR-backed Faber re-emit below is the engine behind
//! `faber format --locale <X>`; rename `compile_canonical*` / `canonical::` in
//! a later sweep.
//!
//! Owns the format-compilation workflow. Delegates lexing, parsing, and
//! analysis to their respective subsystems; this module integrates them
//! and produces Faber output. Does not own the canonical or author emit
//! internals.

#[cfg(feature = "hir-faber")]
use crate::codegen::CodegenError;
use crate::diagnostics::{DiagnosticArg, DiagnosticPhase};
#[cfg(feature = "hir-faber")]
use crate::driver::analyze_source_with_module_import_contract;
use crate::driver::{peel_raw_source, source_load_diagnostic, Config, Session};
use crate::lexer;
// `KeywordSurface` and `latin_locale_pack` are ungated `crate::locale` items:
// the author path (`compile_author`) resolves the reader pack's keyword
// surface unconditionally, so the import must not ride the hir-faber gate.
use crate::locale::{latin_locale_pack, KeywordSurface};
use crate::parser;
#[cfg(feature = "hir-faber")]
use crate::tool::commands::compile::prepare_program_module_for_name;
use crate::{Diagnostic, FaberOutput};

use super::author;
#[cfg(feature = "hir-faber")]
use super::canonical;
use super::options::{FormatOptions, FormatPolicy, PolicyError};
use super::pretty;
use crate::diagnostics::DiagnosticConvert;

const MISSING_PARSE_PROGRAM_MESSAGE: &str = "successful parse result missing program";

/// Output and diagnostics from one canonical Faber format invocation.
#[derive(Debug)]
pub struct FormatCompileResult {
    /// Canonical Faber source when emission succeeded.
    pub output: Option<FaberOutput>,
    /// Ordered diagnostics accumulated across compiler phases.
    pub diagnostics: Vec<Diagnostic>,
}

impl FormatCompileResult {
    /// Return true when emission produced output and no error diagnostics remain.
    pub fn success(&self) -> bool {
        self.output.is_some()
            && !self
                .diagnostics
                .iter()
                .any(radix_diagnostics::Diagnostic::is_error)
    }
}

/// Analyze source and re-emit Faber from HIR.
///
/// LEGACY(canonical): see the module note. This is the engine behind
/// `faber format --locale <X>`; the name predates reader locales.
#[must_use]
#[cfg(feature = "hir-faber")]
pub fn compile_canonical(session: &Session, name: &str, source: &str) -> FormatCompileResult {
    // The legacy no-pack path parses with the Latin pack so the round-trip
    // re-parse sees the same type spellings the emit writes (`nihil_t`, …).
    // Frontmatter locales still win on the first pass (`locale_pack_explicit`
    // is not set), so `en` sources keep their reader surface there.
    let latin_pack;
    let owned_session;
    let session: &Session = if session.config.locale_pack.is_none() {
        latin_pack = latin_locale_pack();
        owned_session = Session::new(session.config.clone().with_locale_pack(latin_pack.clone()));
        &owned_session
    } else {
        session
    };

    // Session pack is the emit surface (`faber convert --to`, requested reader
    // locale). `prepare_program_module` overwrites that pack with source
    // frontmatter so lex/analysis match the input locale; emit must keep the
    // incoming pack. In-memory names skip the overwrite (locale_fanout);
    // real file paths do not (`faber convert`, format corpus).
    let emit_pack = session
        .config
        .locale_pack
        .clone()
        .expect("compile_canonical installs the Latin pack on the no-pack path before reaching here");

    // Path-shaped names enter the program/discovery helper; in-memory names
    // keep empty contracts. Module analysis itself never walks the filesystem.
    let prepared = match prepare_program_module_for_name(name, source, session) {
        Ok(prepared) => prepared,
        Err(diagnostic) => return FormatCompileResult { output: None, diagnostics: vec![*diagnostic] },
    };
    let session = prepared.session;
    let mut analysis = match analyze_source_with_module_import_contract(&session, name, source, None, prepared.contract)
    {
        Ok(analysis) => analysis,
        Err(diagnostics) => return FormatCompileResult { output: None, diagnostics },
    };

    // An explicit reader pack drives a reader-locale emit: keywords localize
    // but glyph-shape sugar (e.g. template application `"…"(args)`) is kept.
    let surface = KeywordSurface::locale(&emit_pack);
    // Imported nominal types have no local definition name; carry their
    // qualified source spelling (`base.Vector3`) so the re-emit prints the
    // namespace instead of the `def_N` fallback. Compiler-generated backward
    // companions ride the same DefId→spelling map: they have no HIR
    // definition for the name collector to find.
    let imported_spellings = analysis.resolver.imported_spellings(&analysis.interner);
    let mut by_def = imported_spellings.by_def;
    by_def.extend(
        analysis
            .resolver
            .generated_companion_spellings(&analysis.interner),
    );
    match canonical::emit(
        &analysis.hir,
        &analysis.presentation,
        &analysis.types,
        &analysis.interner,
        &surface,
        &imported_spellings.by_type,
        &by_def,
    ) {
        Ok(output) => {
            FormatCompileResult { output: Some(FaberOutput { code: output.code }), diagnostics: analysis.diagnostics }
        }
        Err(err) => {
            analysis
                .diagnostics
                .push(Diagnostic::codegen_error(&err.message).with_args(err.args));
            FormatCompileResult { output: None, diagnostics: analysis.diagnostics }
        }
    }
}

/// Convenience wrapper with a default Faber-format session config.
///
/// LEGACY(canonical): see the module note.
#[must_use]
#[cfg(feature = "hir-faber")]
pub fn compile_canonical_str(name: &str, source: &str) -> FormatCompileResult {
    let session = Session::new(Config::default());
    compile_canonical(&session, name, source)
}

/// Parse source and emit author-mode Faber (comment-preserving).
#[must_use]
pub fn compile_author(session: &Session, name: &str, source: &str) -> FormatCompileResult {
    let mut diagnostics = Vec::new();

    let peeled = match peel_raw_source(name, source) {
        Ok(peeled) => peeled,
        Err(error) => {
            return FormatCompileResult { output: None, diagnostics: vec![source_load_diagnostic(name, &error)] };
        }
    };
    let body = peeled.body;

    // Resolve reader pack: frontmatter locale > session config > Latin default
    // (same resolution the analysis front end uses, so author formatting
    // honors `+++ locale = "en" +++` corpus sources).
    let Ok(lex_pack) = crate::driver::resolve_peeled_lex_pack(session, &peeled, &mut diagnostics) else {
        return FormatCompileResult { output: None, diagnostics };
    };

    let lex_result = match lex_pack.as_ref() {
        Some(pack) => lexer::lex_with_locale_pack(body, pack),
        None => lexer::lex(body),
    };
    if !lex_result.success() {
        for err in &lex_result.errors {
            diagnostics.push(Diagnostic::from_lex_error(name, body, err));
        }
        return FormatCompileResult { output: None, diagnostics };
    }

    let parse_result = parser::parse(lex_result);
    for err in &parse_result.errors {
        diagnostics.push(Diagnostic::from_parse_error(name, body, err));
    }
    if !parse_result.success() {
        return FormatCompileResult { output: None, diagnostics };
    }

    let parser::ParseResult { program, interner, .. } = parse_result;
    let Some(program) = program else {
        diagnostics.push(
            Diagnostic::error(MISSING_PARSE_PROGRAM_MESSAGE)
                .with_phase(DiagnosticPhase::Parse)
                .with_code("INTERNAL001")
                .with_args(vec![DiagnosticArg::new("issue", "parse_result_missing_program")])
                .with_file(name.to_owned()),
        );
        return FormatCompileResult { output: None, diagnostics };
    };

    // Author emit renders the reader pack's keyword surface — the same pack
    // that lexed the input — so the output re-parses under that pack and a
    // second format pass is stable (Latin default when no pack was resolved).
    let latin_pack;
    let surface = if let Some(pack) = lex_pack.as_ref() {
        KeywordSurface::new(pack)
    } else {
        latin_pack = latin_locale_pack();
        KeywordSurface::new(&latin_pack)
    };
    let output = author::emit(&program, &surface, &interner, &FormatOptions::author());
    FormatCompileResult { output: Some(output), diagnostics }
}

/// Convenience wrapper with a default Faber-format session config.
///
/// Uses dev-stdlib discovery so `+++ locale = "en" +++` sources (the exempla
/// corpus) can load their reader pack from a workspace checkout.
#[must_use]
pub fn compile_author_str(name: &str, source: &str) -> FormatCompileResult {
    let session = Session::new(Config::default().with_dev_stdlib());
    compile_author(&session, name, source)
}

/// Compile with an explicit format policy (rule-slug registry selection).
///
/// `normalise-v1` selects the exact current author emit path — output is
/// byte-identical to [`compile_author`]. `pretty-v1` selects the pretty layout
/// engine ([`compile_pretty_author`]). A registered policy the engine cannot
/// honor fails clearly here; there is never a silent fallback to another
/// policy.
///
/// # Errors
///
/// Returns [`PolicyError::NotImplemented`] for a registered policy the engine
/// does not support yet.
pub fn compile_author_with_policy(
    session: &Session,
    name: &str,
    source: &str,
    policy: FormatPolicy,
) -> Result<FormatCompileResult, PolicyError> {
    match policy {
        FormatPolicy::NormaliseV1 => Ok(compile_author(session, name, source)),
        FormatPolicy::PrettyV1 => Ok(compile_pretty_author(session, name, source)),
    }
}

/// Convenience wrapper with a default Faber-format session config.
///
/// Uses dev-stdlib discovery, matching [`compile_author_str`].
///
/// # Errors
///
/// Returns [`PolicyError::NotImplemented`] for a registered policy the engine
/// does not support yet.
pub fn compile_author_with_policy_str(
    name: &str,
    source: &str,
    policy: FormatPolicy,
) -> Result<FormatCompileResult, PolicyError> {
    let session = Session::new(Config::default().with_dev_stdlib());
    compile_author_with_policy(&session, name, source, policy)
}

/// FORMAT-PRETTY S3 engine entry for the `pretty-v1` layout policy (author
/// path). Selected by the policy registry ([`compile_author_with_policy`]).
///
/// Runs the same peel/lex/parse pipeline as [`compile_author`], then applies
/// the line-sensitive grammar guard and the pretty layout engine. The layout
/// rules (blocks & indentation, wrapping, blanks/comments, spacing) live in
/// `forma::pretty` and share no emit code with the normalise-v1 path — the
/// normaliser stays byte-identical by construction.
///
/// Guard behavior: when a construct cannot be preserved confidently (compact
/// `ergo` bodies over block-owning statements, lossy annotation tokens), the
/// body is returned **unchanged** with an error diagnostic naming the
/// path/location. `--check` mode fails on the error; write mode leaves the
/// file untouched. A partial rewrite is worse than an unformatted file.
#[doc(hidden)]
#[must_use]
pub fn compile_pretty_author(session: &Session, name: &str, source: &str) -> FormatCompileResult {
    let mut diagnostics = Vec::new();

    let peeled = match peel_raw_source(name, source) {
        Ok(peeled) => peeled,
        Err(error) => {
            return FormatCompileResult { output: None, diagnostics: vec![source_load_diagnostic(name, &error)] };
        }
    };
    let body = peeled.body;

    let Ok(lex_pack) = crate::driver::resolve_peeled_lex_pack(session, &peeled, &mut diagnostics) else {
        return FormatCompileResult { output: None, diagnostics };
    };

    let lex_result = match lex_pack.as_ref() {
        Some(pack) => lexer::lex_with_locale_pack(body, pack),
        None => lexer::lex(body),
    };
    if !lex_result.success() {
        for err in &lex_result.errors {
            diagnostics.push(Diagnostic::from_lex_error(name, body, err));
        }
        return FormatCompileResult { output: None, diagnostics };
    }

    let parse_result = parser::parse(lex_result);
    for err in &parse_result.errors {
        diagnostics.push(Diagnostic::from_parse_error(name, body, err));
    }
    if !parse_result.success() {
        return FormatCompileResult { output: None, diagnostics };
    }

    let parser::ParseResult { program, interner, .. } = parse_result;
    let Some(program) = program else {
        diagnostics.push(
            Diagnostic::error(MISSING_PARSE_PROGRAM_MESSAGE)
                .with_phase(DiagnosticPhase::Parse)
                .with_code("INTERNAL001")
                .with_args(vec![DiagnosticArg::new("issue", "parse_result_missing_program")])
                .with_file(name.to_owned()),
        );
        return FormatCompileResult { output: None, diagnostics };
    };

    // Line-sensitive grammar guard: leave the file unchanged and report when a
    // construct cannot be preserved confidently.
    let guard_hits = pretty::find_guard_hits(&program);
    if !guard_hits.is_empty() {
        let line_index = pretty::LineIndex::new(body);
        for hit in &guard_hits {
            let (issue, location) = match hit.kind {
                pretty::GuardKind::ErgoBlockNesting => {
                    ("ergo_body_nested_block", format!("line {}", line_index.line_of(hit.span.start)))
                }
                pretty::GuardKind::AnnotationTokenLossy => (
                    "annotation_token_not_preservable",
                    format!("line {}", line_index.line_of(hit.span.start)),
                ),
            };
            diagnostics.push(
                Diagnostic::error("pretty-v1 cannot preserve this construct confidently; file left unchanged")
                    .with_phase(DiagnosticPhase::Tool)
                    .with_code("FORMAT001")
                    .with_args(vec![
                        DiagnosticArg::new("issue", issue),
                        DiagnosticArg::new("location", location),
                    ])
                    .with_file(name.to_owned()),
            );
        }
        return FormatCompileResult { output: Some(FaberOutput { code: body.to_owned() }), diagnostics };
    }

    let output = pretty::emit(&program, &interner, body);
    FormatCompileResult { output: Some(output), diagnostics }
}

/// Convenience wrapper with a default Faber-format session config.
///
/// Uses dev-stdlib discovery, matching [`compile_pretty_author`].
#[doc(hidden)]
#[must_use]
pub fn compile_pretty_author_str(name: &str, source: &str) -> FormatCompileResult {
    let session = Session::new(Config::default().with_dev_stdlib());
    compile_pretty_author(&session, name, source)
}

/// Map codegen errors for callers that already hold analyzed state.
///
/// # Errors
///
/// Returns [`CodegenError`] if canonical Faber emission fails.
#[cfg(feature = "hir-faber")]
pub fn emit_from_analyzed(
    hir: &crate::hir::HirModule,
    types: &crate::semantic::TypeTable,
    interner: &crate::lexer::Interner,
) -> Result<FaberOutput, CodegenError> {
    let latin = latin_locale_pack();
    let surface = KeywordSurface::new(&latin);
    // No presentation sidecar on this loose entry: render trivia-free.
    let output = canonical::emit(
        hir,
        &canonical::empty_presentation(),
        types,
        interner,
        &surface,
        &rustc_hash::FxHashMap::default(),
        &rustc_hash::FxHashMap::default(),
    )?;
    Ok(FaberOutput { code: output.code })
}
