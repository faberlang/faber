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
use crate::driver::{analyze_source, AnalyzeRequest, AnalyzedModule, Config, Session};
use crate::locale::{latin_locale_pack, KeywordSurface, LocalePack};
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
            let output = FaberOutput { code: output.code };
            match revalidate_emitted_output(&session, name, &output.code, &analysis, &emit_pack) {
                Ok(()) => FormatCompileResult { output: Some(output), diagnostics: analysis.diagnostics },
                Err(diagnostic) => {
                    let mut diagnostics = analysis.diagnostics;
                    diagnostics.push(diagnostic);
                    FormatCompileResult { output: None, diagnostics }
                }
            }
        }
        Err(err) => {
            analysis
                .diagnostics
                .push(Diagnostic::codegen_error(&err.message).with_args(err.args));
            FormatCompileResult { output: None, diagnostics: analysis.diagnostics }
        }
    }
}

/// Re-analyze emitted Faber and compare the source facts that the emitter must
/// preserve. The emitted body is prepared again so path-shaped names rebuild
/// their import contract instead of reusing the input analysis.
///
/// The incoming reader pack is made explicit for this pass. The first analysis
/// may have selected a source frontmatter pack, while the output is written in
/// the caller-selected pack captured by `emit_pack`.
///
/// # Errors
///
/// Returns `CODEGEN006` when the emitted body cannot be analyzed or when its
/// authored variant-path spelling multiset differs from the input analysis.
pub(crate) fn revalidate_emitted_output(
    session: &Session,
    name: &str,
    emitted: &str,
    input: &AnalyzedModule,
    emit_pack: &LocalePack,
) -> Result<(), Diagnostic> {
    let revalidation_session = Session::new(
        session
            .config
            .clone()
            .with_locale_pack(emit_pack.clone())
            .with_locale_pack_explicit(),
    );
    let expected = variant_path_spelling_multiset(&input.presentation, &input.interner);
    let prepared = prepare_program_module_for_name(name, emitted, &revalidation_session).map_err(|diagnostic| {
        revalidation_failure(
            name,
            "variant_path_spelling_reanalysis_failed",
            format!("preparing emitted output failed: {}", diagnostic.message),
            expected.len(),
            None,
        )
    })?;
    let reanalysis = analyze_source(
        &prepared.session,
        name,
        emitted,
        AnalyzeRequest { cli_program: None, contract: prepared.contract },
    );
    let reanalysis = match reanalysis {
        Ok(reanalysis) if reanalysis.diagnostics.iter().any(Diagnostic::is_error) => {
            return Err(revalidation_failure(
                name,
                "variant_path_spelling_reanalysis_failed",
                diagnostics_summary(&reanalysis.diagnostics),
                expected.len(),
                None,
            ));
        }
        Ok(reanalysis) => reanalysis,
        Err(diagnostics) => {
            return Err(revalidation_failure(
                name,
                "variant_path_spelling_reanalysis_failed",
                diagnostics_summary(&diagnostics),
                expected.len(),
                None,
            ));
        }
    };
    let actual = variant_path_spelling_multiset(&reanalysis.presentation, &reanalysis.interner);
    if actual != expected {
        return Err(revalidation_failure(
            name,
            "variant_path_spelling_multiset_diverged",
            "emitted variant-path spellings differ from the input",
            expected.len(),
            Some(actual.len()),
        ));
    }
    Ok(())
}

/// Return the authored variant paths as a sorted spelling multiset.
///
/// The `DefId` is deliberately omitted: each analysis allocates identities in
/// its own namespace, while the authored spelling is the stable comparison
/// fact for this gate. Sorting preserves duplicate references as a multiset.
pub(crate) fn variant_path_spelling_multiset(
    presentation: &crate::hir::HirPresentation,
    interner: &crate::lexer::Interner,
) -> Vec<String> {
    let mut paths = presentation
        .variant_paths
        .iter()
        .map(|variant| {
            variant
                .path
                .iter()
                .map(|segment| interner.resolve(*segment))
                .collect::<Vec<_>>()
                .join(".")
        })
        .collect::<Vec<_>>();
    paths.sort_unstable();
    paths
}

fn revalidation_failure(
    name: &str,
    issue: &str,
    detail: impl Into<String>,
    expected_count: usize,
    actual_count: Option<usize>,
) -> Diagnostic {
    let mut diagnostic = Diagnostic::error(format!("canonical Faber output failed re-validation: {}", detail.into()))
        .with_phase(crate::diagnostics::DiagnosticPhase::Codegen)
        .with_code("CODEGEN006")
        .with_file(name.to_owned())
        .with_arg("issue", issue)
        .with_arg("expected_count", expected_count.to_string());
    if let Some(actual_count) = actual_count {
        diagnostic = diagnostic.with_arg("actual_count", actual_count.to_string());
    }
    diagnostic
}

fn diagnostics_summary(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>()
        .join("; ")
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
