//! Canonical Faber format entry: analyze + HIR re-emit.
//!
//! LEGACY(canonical): the "canonical" name predates reader locales — with the
//! reader-locale system there is no single canonical language surface, only
//! locale surfaces. The HIR-backed Faber re-emit below is the engine behind
//! `faber format --locale <X>`; rename `compile_canonical*` in a later sweep.
//!
//! Owns the format-compilation workflow. Delegates lexing, parsing, and
//! analysis to their respective subsystems; this module integrates them and
//! produces Faber output. Does not own emit internals.

use crate::codegen::CodegenError;
use crate::driver::{analyze_source, AnalyzeRequest, Config, Session};
use crate::locale::{latin_locale_pack, KeywordSurface};
use crate::program::compile::prepare_program_module_for_name;
use crate::{Diagnostic, FaberOutput};

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
    let mut analysis = match analyze_source(
        &session,
        name,
        source,
        AnalyzeRequest { cli_program: None, contract: prepared.contract },
    ) {
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
    match super::emit(
        &analysis.hir,
        &analysis.presentation,
        &analysis.types,
        &analysis.interner,
        &super::LocaleKeywordBridge(surface),
        &imported_spellings.by_type,
        &by_def,
        &imported_spellings.by_def_pattern,
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
pub fn compile_canonical_str(name: &str, source: &str) -> FormatCompileResult {
    let session = Session::new(Config::default());
    compile_canonical(&session, name, source)
}

/// Map codegen errors for callers that already hold analyzed state.
///
/// # Errors
///
/// Returns [`CodegenError`] if canonical Faber emission fails.
pub fn emit_from_analyzed(
    hir: &crate::hir::HirModule,
    types: &crate::semantic::TypeTable,
    interner: &crate::lexer::Interner,
) -> Result<FaberOutput, CodegenError> {
    let latin = latin_locale_pack();
    let surface = KeywordSurface::new(&latin);
    // No presentation sidecar on this loose entry: render trivia-free.
    let output = super::emit(
        hir,
        &super::empty_presentation(),
        types,
        interner,
        &super::LocaleKeywordBridge(surface),
        &rustc_hash::FxHashMap::default(),
        &rustc_hash::FxHashMap::default(),
        &rustc_hash::FxHashMap::default(),
    )?;
    Ok(FaberOutput { code: output.code })
}
