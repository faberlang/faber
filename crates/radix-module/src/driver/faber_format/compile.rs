//! Canonical Faber format entry: analyze + HIR re-emit.
//!
//! The reader-locale system has no single canonical language surface, only
//! locale surfaces. This HIR-backed Faber re-emitter is shared by `faber
//! format` and `faber convert` with source and output packs kept separate.
//!
//! Owns the format-compilation workflow. Delegates lexing, parsing, and
//! analysis to their respective subsystems; this module integrates them and
//! produces Faber output. Does not own emit internals.

use crate::codegen::{CodegenError, Target};
use crate::driver::source_inventory::{LostConstruct, lost as lost_constructs, parse_inventory};
use crate::driver::{
    AnalyzeRequest, AnalyzedModule, Session, adjust_diagnostic_lines_for_body, analyze_source, peel_raw_source,
};
use crate::locale::{KeywordSurface, LocalePack, latin_locale_pack};
use crate::program::compile::prepare_program_module_for_name;
use crate::{Diagnostic, FaberOutput};
use handroll::FxHashMap;
use radix_hir::visit::{HirVisitor, walk_block, walk_expr, walk_function, walk_item};
use radix_hir::{
    HirExpression, HirExpressionKind, HirItem, HirItemKind, HirOwnerRef, HirTypeForm, HirTypeFormArg, HirTypeFormHead,
    HirTypeSlot,
};
use radix_syntax::TypeFormLevel;
use radix_types::{AppliedArg, BitsWidth, Primitive, Type, TypeId};
use std::collections::{BTreeMap, BTreeSet};

/// Output and diagnostics from one Faber HIR re-emission invocation.
#[derive(Debug)]
pub struct FaberReemitResult {
    /// Faber source when emission succeeded.
    pub output: Option<FaberOutput>,
    /// Ordered diagnostics accumulated across compiler phases.
    pub diagnostics: Vec<Diagnostic>,
}

impl FaberReemitResult {
    /// Return true when emission produced output and no error diagnostics remain.
    pub fn success(&self) -> bool {
        self.output.is_some()
            && !self
                .diagnostics
                .iter()
                .any(radix_diagnostics::Diagnostic::is_error)
    }
}

/// Analyze source under `source_session` and re-emit it through `emit_pack`.
///
/// The source session owns peeling, lexing, parsing, imports, semantic
/// analysis, and HIR construction. The emit pack is used only for the Faber
/// keyword surface and for revalidation of the emitted body.
#[must_use]
pub fn reemit_faber(source_session: &Session, emit_pack: &LocalePack, name: &str, source: &str) -> FaberReemitResult {
    // Path-shaped names enter the program/discovery helper; in-memory names
    // keep empty contracts. Module analysis itself never walks the filesystem.
    let prepared = match prepare_program_module_for_name(name, source, source_session) {
        Ok(prepared) => prepared,
        Err(diagnostic) => return FaberReemitResult { output: None, diagnostics: vec![*diagnostic] },
    };
    let mut session = prepared.session;
    // Re-emit prints the author's text: constant expressions stay unfolded
    // (F9 ruling 31), here and in the revalidation session cloned from this one.
    session.config.keep_authored_constants = true;
    // Re-emission is the HirFaber path whichever session the caller passed:
    // the driver evaluates `comptime` bodies only for other targets, and a
    // block-level body keeps no authored copy, so evaluating here would print
    // its literal value in place of the author's body.
    session.config.target = Target::HirFaber;
    let mut analysis = match analyze_source(
        &session,
        name,
        source,
        AnalyzeRequest { cli_program: None, contract: prepared.contract },
    ) {
        Ok(analysis) => analysis,
        Err(diagnostics) => return FaberReemitResult { output: None, diagnostics },
    };

    // Reconcile finalized HIR witnesses before the emitter indexes authored
    // forms. This keeps direct re-emission on the same artifact contract as
    // the FHIR writer without changing the semantic analysis pass.
    crate::hir::presentation_witness::normalize_presentation_witnesses(
        &analysis.hir,
        &mut analysis.presentation,
        &analysis.types,
    );

    // An explicit reader pack drives a reader-locale emit: keywords localize
    // but glyph-shape sugar (e.g. template application `"…"(args)`) is kept.
    let surface = KeywordSurface::locale(emit_pack);
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
    by_def.extend(imported_contract_spellings(
        &analysis.hir,
        &analysis.resolver,
        &analysis.interner,
    ));
    match crate::codegen::faber::FaberCodegen::new(&analysis.types, &analysis.interner)
        .with_imported_spellings(&imported_spellings.by_type, &by_def, &imported_spellings.by_def_pattern)
        .generate(
            &analysis.hir,
            &analysis.presentation,
            &crate::codegen::faber::LocaleKeywordBridge(surface),
        ) {
        Ok(output) => {
            let output = FaberOutput { code: output.code };
            // Structure comes after revalidation so the emitted text is known to
            // parse; it stays out of `revalidate_emitted_output`, which compares
            // analyzed facts and not statement structure.
            let checked = revalidate_emitted_output(&session, name, &output.code, &analysis, emit_pack)
                .and_then(|()| check_structure(&session, emit_pack, name, source, &output.code));
            match checked {
                Ok(()) => FaberReemitResult { output: Some(output), diagnostics: analysis.diagnostics },
                Err(diagnostic) => {
                    let mut diagnostics = analysis.diagnostics;
                    diagnostics.push(*diagnostic);
                    FaberReemitResult { output: None, diagnostics }
                }
            }
        }
        Err(err) => {
            analysis
                .diagnostics
                .push(Diagnostic::codegen_error(&err.message).with_args(err.args));
            FaberReemitResult { output: None, diagnostics: analysis.diagnostics }
        }
    }
}

/// Collect the reader-visible spellings of imported contracts appearing in
/// declaration bounds. Contract refs carry only definition identities, while
/// the ordinary HIR name table contains definitions owned by this module; an
/// imported contract would otherwise fall through to the synthetic `def_N`
/// spelling.
fn imported_contract_spellings(
    hir: &radix_hir::HirModule,
    resolver: &crate::semantic::Resolver,
    interner: &crate::lexer::Interner,
) -> FxHashMap<radix_hir::DefId, String> {
    struct Collector<'a> {
        resolver: &'a crate::semantic::Resolver,
        interner: &'a crate::lexer::Interner,
        spellings: FxHashMap<radix_hir::DefId, String>,
    }

    impl Collector<'_> {
        fn add_contracts(&mut self, contracts: &[radix_types::ContractRef]) {
            for contract in contracts {
                if let Some(symbol) = self.resolver.get_symbol(contract.contract) {
                    self.spellings
                        .insert(contract.contract, self.interner.resolve(symbol.name).to_owned());
                }
            }
        }
    }

    impl HirVisitor for Collector<'_> {
        fn visit_item(&mut self, item: &HirItem) {
            if let HirItemKind::Struct(strukt) = &item.kind {
                self.add_contracts(&strukt.implements);
            }
            walk_item(self, item);
        }

        fn visit_type_param(&mut self, type_param: &radix_hir::HirTypeParam) {
            if let radix_hir::HirTypeParamConstraint::Implet(contracts) = &type_param.constraint {
                self.add_contracts(contracts);
            }
            self.visit_def(type_param.def_id, type_param.name);
        }
    }

    let mut collector = Collector { resolver, interner, spellings: FxHashMap::default() };
    collector.visit_program(hir);
    collector.spellings
}

/// Refuse output that drops a source construct HIR cannot represent.
///
/// Counts top-level statements and test-suite members in the source and in the
/// emitted text and fails when any count shrinks. Both sides are parsed, never
/// analyzed, so the check sees surface structure that HIR lowering discards.
/// The source side uses the prepared session (source locale); the output side
/// uses the emit pack, as revalidation does.
///
/// # Errors
///
/// Returns `CODEGEN006` / `ast_construct_dropped` naming the first lost
/// construct and its first source line, or `CODEGEN006` / `structure_check_failed`
/// when either side cannot be parsed (the check fails closed).
fn check_structure(
    session: &Session,
    emit_pack: &LocalePack,
    name: &str,
    source: &str,
    emitted: &str,
) -> Result<(), Box<Diagnostic>> {
    let unparsed = |side: &str, diagnostics: &[Diagnostic]| {
        Box::new(
            Diagnostic::error(format!(
                "canonical Faber output failed structure check: {side} could not be parsed: {}",
                diagnostics_summary(diagnostics)
            ))
            .with_phase(crate::diagnostics::DiagnosticPhase::Codegen)
            .with_code("CODEGEN006")
            .with_file(name.to_owned())
            .with_arg("issue", "structure_check_failed"),
        )
    };
    let expected = parse_inventory(session, name, source).map_err(|diagnostics| unparsed("source", &diagnostics))?;
    let output_session = Session::new(
        session
            .config
            .clone()
            .with_resolved_locale_pack(emit_pack.clone()),
    );
    let actual =
        parse_inventory(&output_session, name, emitted).map_err(|diagnostics| unparsed("output", &diagnostics))?;
    let lost = lost_constructs(&expected, &actual);
    let Some(first) = lost.first() else {
        return Ok(());
    };
    Err(Box::new(structure_loss(name, source, first, &lost)))
}

/// `CODEGEN006` / `ast_construct_dropped`: the emitted text carries fewer of a
/// construct than the source, so nothing is written.
fn structure_loss(name: &str, source: &str, first: &LostConstruct, all: &[LostConstruct]) -> Diagnostic {
    let location = if first.suite.is_empty() {
        String::new()
    } else {
        format!(" in suite `{}`", first.suite)
    };
    let mut diagnostic = Diagnostic::error(String::new())
        .with_phase(crate::diagnostics::DiagnosticPhase::Codegen)
        .with_code("CODEGEN006")
        .with_arg("issue", "ast_construct_dropped")
        .with_arg("construct", first.construct.clone())
        .with_arg("suite", first.suite.clone())
        .with_arg("source_count", first.source_count.to_string())
        .with_arg("output_count", first.output_count.to_string())
        .with_arg(
            "lost",
            all.iter()
                .map(|lost| {
                    format!("{} {}", lost.suite, lost.construct)
                        .trim()
                        .to_owned()
                })
                .collect::<Vec<_>>()
                .join(", "),
        );
    // Spans are relative to the peeled body; the line is shifted back onto the
    // physical document.
    if let Ok(peeled) = peel_raw_source(name, source) {
        diagnostic = diagnostic.with_source_context(name, peeled.body, first.first_span);
        adjust_diagnostic_lines_for_body(std::slice::from_mut(&mut diagnostic), source, peeled.body_byte_offset);
    } else {
        diagnostic = diagnostic.with_file(name.to_owned());
    }
    let line = diagnostic
        .line
        .map_or_else(String::new, |line| format!(" (first at line {line})"));
    diagnostic.message = format!(
        "canonical Faber output would drop source construct `{}`{location}{line}: the source has {}, the output {}; nothing was written",
        first.construct, first.source_count, first.output_count
    );
    diagnostic
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
/// Returns `CODEGEN006` when the emitted body cannot be analyzed or when an
/// authored source fact differs from the input analysis.
pub(crate) fn revalidate_emitted_output(
    session: &Session,
    name: &str,
    emitted: &str,
    input: &AnalyzedModule,
    emit_pack: &LocalePack,
) -> Result<(), Box<Diagnostic>> {
    let revalidation_session = Session::new(
        session
            .config
            .clone()
            .with_resolved_locale_pack(emit_pack.clone()),
    );
    let expected_variants = variant_path_spelling_multiset(&input.presentation, &input.interner);
    let expected_types = authored_type_form_fingerprint(input);
    let prepared = prepare_program_module_for_name(name, emitted, &revalidation_session).map_err(|diagnostic| {
        Box::new(revalidation_failure(
            name,
            "variant_path_spelling_reanalysis_failed",
            format!("preparing emitted output failed: {}", diagnostic.message),
            expected_variants.len(),
            None,
        ))
    })?;
    let reanalysis = analyze_source(
        &prepared.session,
        name,
        emitted,
        AnalyzeRequest { cli_program: None, contract: prepared.contract },
    );
    let mut reanalysis = match reanalysis {
        Ok(reanalysis) if reanalysis.diagnostics.iter().any(Diagnostic::is_error) => {
            return Err(Box::new(revalidation_failure(
                name,
                "variant_path_spelling_reanalysis_failed",
                diagnostics_summary(&reanalysis.diagnostics),
                expected_variants.len(),
                None,
            )));
        }
        Ok(reanalysis) => reanalysis,
        Err(diagnostics) => {
            return Err(Box::new(revalidation_failure(
                name,
                "variant_path_spelling_reanalysis_failed",
                diagnostics_summary(&diagnostics),
                expected_variants.len(),
                None,
            )));
        }
    };
    // Apply the same artifact-boundary witness reconciliation to the emitted
    // reanalysis before comparing authored forms. Without this, record fields
    // can have equivalent source forms with different canonical map order.
    crate::hir::presentation_witness::normalize_presentation_witnesses(
        &reanalysis.hir,
        &mut reanalysis.presentation,
        &reanalysis.types,
    );
    let actual_variants = variant_path_spelling_multiset(&reanalysis.presentation, &reanalysis.interner);
    if actual_variants != expected_variants {
        return Err(Box::new(revalidation_failure(
            name,
            "variant_path_spelling_multiset_diverged",
            "emitted variant-path spellings differ from the input",
            expected_variants.len(),
            Some(actual_variants.len()),
        )));
    }

    let actual_types = authored_type_form_fingerprint(&reanalysis);
    // Re-emission may materialize an inferred signature that had no authored
    // source site. Compare every input key and ignore only such extra keys;
    // dropping or changing an expected occurrence still fails closed.
    let keyed_diverged = expected_types
        .keyed
        .iter()
        .any(|(key, expected)| actual_types.keyed.get(key) != Some(expected));
    if keyed_diverged || actual_types.unkeyed != expected_types.unkeyed {
        let issue = if keyed_diverged {
            "authored_type_form_map_diverged"
        } else {
            "authored_type_form_count_diverged"
        };
        return Err(Box::new(revalidation_failure(
            name,
            issue,
            "emitted authored type forms differ from the input",
            expected_types.total(),
            Some(actual_types.total()),
        )));
    }

    // D8.9: the formatter never changes meaning. The narrower checks above
    // name the known emitter-debt families; this one proves the whole
    // analyzed program is unchanged.
    let expected_meaning = super::meaning::meaning_fingerprint(input);
    let actual_meaning = super::meaning::meaning_fingerprint(&reanalysis);
    if let Some(difference) = super::meaning::first_meaning_difference(&expected_meaning, &actual_meaning) {
        return Err(Box::new(meaning_divergence(name, &difference)));
    }
    Ok(())
}

/// `CODEGEN006` / `hir_meaning_diverged`: the emitted program analyzes to a
/// different HIR than the input, so the formatter refuses to write it.
fn meaning_divergence(name: &str, difference: &super::meaning::MeaningDifference) -> Diagnostic {
    Diagnostic::error(format!(
        "canonical Faber output failed re-validation: the emitted program means something else; first difference in {}: expected `{}`, emitted `{}`",
        difference.segment, difference.expected, difference.actual
    ))
    .with_phase(crate::diagnostics::DiagnosticPhase::Codegen)
    .with_code("CODEGEN006")
    .with_file(name.to_owned())
    .with_arg("issue", "hir_meaning_diverged")
    .with_arg("segment", difference.segment.clone())
    .with_arg("expected", difference.expected.clone())
    .with_arg("actual", difference.actual.clone())
}

/// Occurrence-aware authored type facts used by output revalidation.
///
/// Named and local `Def` owners use structural keys. `Hir` owners identify
/// expression occurrences only by slot, so those sites are checked by count
/// per slot rather than by a false cross-analysis HIR id.
#[derive(Debug, Default, PartialEq, Eq)]
struct AuthoredTypeFormFingerprint {
    keyed: BTreeMap<String, String>,
    unkeyed: BTreeMap<String, usize>,
}

impl AuthoredTypeFormFingerprint {
    fn total(&self) -> usize {
        self.keyed.len() + self.unkeyed.values().sum::<usize>()
    }
}

fn authored_type_form_fingerprint(analysis: &AnalyzedModule) -> AuthoredTypeFormFingerprint {
    let owners = StructuralOwnerIndex::build(&analysis.hir, &analysis.interner);
    let mut fingerprint = AuthoredTypeFormFingerprint::default();
    for authored in &analysis.presentation.authored_types {
        // Field emission remains canonical until its owner path is migrated.
        if matches!(&authored.site.slot, HirTypeSlot::Field(_)) {
            continue;
        }
        let slot = type_slot_key(&authored.site.slot);
        let signature = type_form_signature(analysis, &authored.form);
        match &authored.site.owner {
            HirOwnerRef::Def(def_id) => {
                if let Some(owner) = owners.owners.get(def_id) {
                    fingerprint
                        .keyed
                        .insert(format!("{owner}/{slot}"), signature);
                } else {
                    // An unknown Def owner is not structurally nameable. Keep
                    // it conservative and compare its slot count instead of
                    // comparing an analysis-local DefId.
                    *fingerprint
                        .unkeyed
                        .entry(format!("unresolved-def/{slot}"))
                        .or_default() += 1;
                }
            }
            HirOwnerRef::Hir(_) => {
                // HIR ids are analysis-local. Expression-owned sites are
                // therefore count-only per slot, as required by §10.
                *fingerprint.unkeyed.entry(slot).or_default() += 1;
            }
        }
    }
    fingerprint
}

#[derive(Debug, Clone)]
enum StructuralOwner {
    Named(String),
    Local {
        function: String,
        name: String,
        ordinal: usize,
    },
}

impl std::fmt::Display for StructuralOwner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named(path) => write!(formatter, "named:{path}"),
            Self::Local { function, name, ordinal } => write!(formatter, "local:{function}:{name}:{ordinal}"),
        }
    }
}

#[derive(Debug)]
struct FunctionOwnerFrame {
    path: String,
    occurrences: BTreeMap<String, usize>,
    closures: usize,
}

struct StructuralOwnerIndex<'a> {
    interner: &'a crate::lexer::Interner,
    owners: FxHashMap<crate::hir::DefId, StructuralOwner>,
    functions: Vec<FunctionOwnerFrame>,
}

impl<'a> StructuralOwnerIndex<'a> {
    fn build(hir: &crate::hir::HirModule, interner: &'a crate::lexer::Interner) -> Self {
        let mut index = Self { interner, owners: FxHashMap::default(), functions: Vec::new() };
        index.visit_program(hir);
        index
    }

    fn visit_function_in_context(&mut self, path: String, function: &crate::hir::HirFunction) {
        self.functions
            .push(FunctionOwnerFrame { path, occurrences: BTreeMap::new(), closures: 0 });
        walk_function(self, function);
        self.functions.pop();
    }

    fn current_function_path(&self) -> String {
        self.functions
            .last()
            .map_or_else(|| "<module>".to_owned(), |frame| frame.path.clone())
    }

    fn visit_struct_method(&mut self, parent: &str, method: &crate::hir::HirMethod) {
        let path = format!("{parent}.{}", self.interner.resolve(method.func.name));
        self.owners
            .insert(method.def_id, StructuralOwner::Named(path.clone()));
        for annotation in &method.annotations {
            self.visit_annotation(annotation);
        }
        self.visit_function_in_context(path, &method.func);
    }
}

impl HirVisitor for StructuralOwnerIndex<'_> {
    fn visit_program(&mut self, program: &crate::hir::HirModule) {
        for item in &program.items {
            self.visit_item(item);
        }
        for annotation in &program.entry_annotations {
            self.visit_annotation(annotation);
        }
        if let Some(entry) = &program.entry {
            self.functions.push(FunctionOwnerFrame {
                path: "<entry>".to_owned(),
                occurrences: BTreeMap::new(),
                closures: 0,
            });
            walk_block(self, entry);
            self.functions.pop();
        }
    }

    fn visit_item(&mut self, item: &HirItem) {
        for annotation in &item.annotations {
            self.visit_annotation(annotation);
        }
        match &item.kind {
            HirItemKind::Function(function) => {
                let path = self.interner.resolve(function.name).to_owned();
                self.owners
                    .insert(item.def_id, StructuralOwner::Named(path.clone()));
                self.visit_function_in_context(path, function);
            }
            HirItemKind::Struct(strukt) => {
                let parent = self.interner.resolve(strukt.name).to_owned();
                self.owners
                    .insert(item.def_id, StructuralOwner::Named(parent.clone()));
                for field in &strukt.fields {
                    self.visit_field(field);
                }
                for method in &strukt.methods {
                    self.visit_struct_method(&parent, method);
                }
            }
            HirItemKind::Enum(enumeration) => {
                let path = self.interner.resolve(enumeration.name).to_owned();
                self.owners
                    .insert(item.def_id, StructuralOwner::Named(path));
            }
            HirItemKind::Interface(interface) => {
                let parent = self.interner.resolve(interface.name).to_owned();
                self.owners
                    .insert(item.def_id, StructuralOwner::Named(parent.clone()));
                for method in &interface.methods {
                    let path = format!("{parent}.{}", self.interner.resolve(method.name));
                    self.functions
                        .push(FunctionOwnerFrame { path, occurrences: BTreeMap::new(), closures: 0 });
                    for param in &method.params {
                        self.visit_param(param);
                    }
                    self.functions.pop();
                }
            }
            HirItemKind::TypeAlias(alias) => {
                let path = self.interner.resolve(alias.name).to_owned();
                self.owners
                    .insert(item.def_id, StructuralOwner::Named(path));
            }
            HirItemKind::Schema(schema) => {
                let path = self.interner.resolve(schema.name).to_owned();
                self.owners
                    .insert(item.def_id, StructuralOwner::Named(path));
            }
            HirItemKind::Constant(constant) => {
                let path = self.interner.resolve(constant.name).to_owned();
                self.owners
                    .insert(item.def_id, StructuralOwner::Named(path));
                self.visit_expr(&constant.value);
            }
            HirItemKind::Import(import) => {
                for imported in &import.items {
                    self.owners.insert(
                        imported.def_id,
                        StructuralOwner::Named(
                            self.interner
                                .resolve(imported.alias.unwrap_or(imported.name))
                                .to_owned(),
                        ),
                    );
                }
            }
        }
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Clausura(..) = &expr.kind {
            let parent = self.current_function_path();
            let ordinal = self.functions.last_mut().map_or(0, |frame| {
                let ordinal = frame.closures;
                frame.closures += 1;
                ordinal
            });
            let path = format!("{parent}::<closure:{ordinal}>");
            self.functions
                .push(FunctionOwnerFrame { path, occurrences: BTreeMap::new(), closures: 0 });
            walk_expr(self, expr);
            self.functions.pop();
        } else {
            walk_expr(self, expr);
        }
    }

    fn visit_def(&mut self, def_id: crate::hir::DefId, name: crate::lexer::Symbol) {
        let name = self.interner.resolve(name).to_owned();
        let Some(frame) = self.functions.last_mut() else {
            return;
        };
        let occurrences = frame.occurrences.entry(name.clone()).or_default();
        let ordinal = *occurrences;
        *occurrences += 1;
        self.owners
            .insert(def_id, StructuralOwner::Local { function: frame.path.clone(), name, ordinal });
    }

    // WHY: the default `visit_type_param` calls `visit_def`, which would index a
    // type parameter as a local binding; only value bindings get owner paths here.
    fn visit_type_param(&mut self, _type_param: &crate::hir::HirTypeParam) {}
}

fn type_slot_key(slot: &HirTypeSlot) -> String {
    match slot {
        HirTypeSlot::Declared => "declared".to_owned(),
        HirTypeSlot::AliasBody => "alias-body".to_owned(),
        HirTypeSlot::Parameter(ordinal) => format!("parameter:{ordinal}"),
        HirTypeSlot::Return => "return".to_owned(),
        HirTypeSlot::Error => "error".to_owned(),
        HirTypeSlot::Field(ordinal) => format!("field:{ordinal}"),
        HirTypeSlot::AnnotationField { annotation, field } => format!("annotation-field:{annotation}:{field}"),
        HirTypeSlot::ClosureParameter(ordinal) => format!("closure-parameter:{ordinal}"),
        HirTypeSlot::TupleTypeArgument(ordinal) => format!("tuple-type-argument:{ordinal}"),
        HirTypeSlot::AscriptionTarget => "ascription-target".to_owned(),
        HirTypeSlot::ConversionTarget => "conversion-target".to_owned(),
        HirTypeSlot::TypedZero => "typed-zero".to_owned(),
        HirTypeSlot::TypeCheckTarget => "type-check-target".to_owned(),
        HirTypeSlot::PatternType => "pattern-type".to_owned(),
        HirTypeSlot::DiscardBinding => "discard-binding".to_owned(),
    }
}

/// The level the canonical emitter writes for an authored form. The retired
/// long numeric forms (`numerus<u8>` / `fractus<f32>`, LONG) re-emit as the
/// bare width marker (SHORT), so both spellings share one fingerprint level
/// (N7c); every other form keeps its authored level.
fn emitted_level(analysis: &AnalyzedModule, form: &HirTypeForm, level: TypeFormLevel) -> TypeFormLevel {
    let sized_numeric = matches!(
        analysis.types.get(form.ty),
        Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
    );
    if sized_numeric && level == TypeFormLevel::LONG {
        TypeFormLevel::SHORT
    } else {
        level
    }
}

fn type_form_signature(analysis: &AnalyzedModule, form: &HirTypeForm) -> String {
    // WHY: ruling d477c76d retires `octeti` / `octeti<N>`; `lista<b8>` is the
    // only spelling (bounded: `lista<b8, magnitudo N>`). The authored retired word and
    // its canonical `list<b8>` spelling lower to the same type, so both share
    // one fingerprint regardless of head level or authored arguments.
    match analysis.types.get(form.ty) {
        Type::BoundedOcteti { capacity } => {
            return format!(
                "byte-buffer-bounded({})[]",
                crate::semantic::format_figura(*capacity, &analysis.types, &analysis.interner)
            );
        }
        Type::Array(element) if matches!(analysis.types.get(*element), Type::Bits(BitsWidth::B8)) => {
            return "byte-buffer[]".to_owned();
        }
        _ => {}
    }
    let head = match &form.head {
        HirTypeFormHead::Semantic(level) => format!(
            "semantic(level={},shape={})",
            emitted_level(analysis, form, *level).0,
            canonical_type_shape(analysis, form.ty, &mut BTreeSet::new())
        ),
        HirTypeFormHead::Named { path, .. } => format!(
            "named(path={},shape={})",
            path.iter()
                .map(|symbol| analysis.interner.resolve(*symbol))
                .collect::<Vec<_>>()
                .join("."),
            canonical_type_shape(analysis, form.ty, &mut BTreeSet::new())
        ),
        HirTypeFormHead::ImportedAlias { path } => format!(
            "imported-alias(path={},shape={})",
            path.iter()
                .map(|symbol| analysis.interner.resolve(*symbol))
                .collect::<Vec<_>>()
                .join("."),
            canonical_type_shape(analysis, form.ty, &mut BTreeSet::new())
        ),
        HirTypeFormHead::Infer => {
            format!("infer(shape={})", canonical_type_shape(analysis, form.ty, &mut BTreeSet::new()))
        }
        HirTypeFormHead::UnionHole => format!(
            "union-hole(shape={})",
            canonical_type_shape(analysis, form.ty, &mut BTreeSet::new())
        ),
    };
    let args = form
        .args
        .iter()
        .map(|arg| match arg {
            HirTypeFormArg::Type(child) => format!("type({})", type_form_signature(analysis, child)),
            HirTypeFormArg::Size(index) => format!(
                "size({})",
                crate::semantic::format_figura(*index, &analysis.types, &analysis.interner)
            ),
            HirTypeFormArg::Name(symbol) => {
                format!("name({})", analysis.interner.resolve(*symbol))
            }
            HirTypeFormArg::Range(range) => {
                let bound = |bound: &radix_hir::HirRangeBound| match bound {
                    radix_hir::HirRangeBound::Literal(value) => value.clone(),
                    radix_hir::HirRangeBound::Unbounded { negative: true } => "-∞".to_owned(),
                    radix_hir::HirRangeBound::Unbounded { negative: false } => "∞".to_owned(),
                    radix_hir::HirRangeBound::Name { name, lifted } => {
                        format!("{}{}", if *lifted { "size " } else { "" }, analysis.interner.resolve(*name))
                    }
                };
                format!(
                    "range({}{}{})",
                    bound(&range.lo),
                    if range.inclusive { "…" } else { "‥" },
                    bound(&range.hi)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{head}[{args}]")
}

fn canonical_type_shape(analysis: &AnalyzedModule, type_id: TypeId, active: &mut BTreeSet<u32>) -> String {
    if !active.insert(type_id.0) {
        return "<recursive>".to_owned();
    }
    let shape = match analysis.types.get(type_id) {
        Type::Primitive(primitive) => format!("primitive:{primitive:?}"),
        Type::TensorEdgePolicy(policy) => format!("tensor-edge-policy:{}", policy.latin()),
        Type::Array(element) => format!("array<{}>", canonical_type_shape(analysis, *element, active)),
        Type::BoundedArray { element, capacity } => format!(
            "bounded-array<{},{}>",
            canonical_type_shape(analysis, *element, active),
            crate::semantic::format_figura(*capacity, &analysis.types, &analysis.interner)
        ),
        Type::RangedArray { element, min, max } => format!(
            "ranged-array<{},{min},{}>",
            canonical_type_shape(analysis, *element, active),
            max.map_or_else(|| "inf".to_owned(), |max| max.to_string())
        ),
        Type::Queue(element) => format!("queue<{}>", canonical_type_shape(analysis, *element, active)),
        Type::BoundedQueue { element, capacity } => format!(
            "bounded-queue<{},{}>",
            canonical_type_shape(analysis, *element, active),
            crate::semantic::format_figura(*capacity, &analysis.types, &analysis.interner)
        ),
        Type::Stack(element) => format!("stack<{}>", canonical_type_shape(analysis, *element, active)),
        Type::BoundedStack { element, capacity } => format!(
            "bounded-stack<{},{}>",
            canonical_type_shape(analysis, *element, active),
            crate::semantic::format_figura(*capacity, &analysis.types, &analysis.interner)
        ),
        Type::BoundedTextus { capacity } => format!(
            "bounded-textus<{}>",
            crate::semantic::format_figura(*capacity, &analysis.types, &analysis.interner)
        ),
        Type::BoundedAscii { capacity } => format!(
            "bounded-ascii<{}>",
            crate::semantic::format_figura(*capacity, &analysis.types, &analysis.interner)
        ),
        Type::BoundedOcteti { capacity } => format!(
            "bounded-octeti<{}>",
            crate::semantic::format_figura(*capacity, &analysis.types, &analysis.interner)
        ),
        Type::Map(key, value) => format!(
            "map<{},{}>",
            canonical_type_shape(analysis, *key, active),
            canonical_type_shape(analysis, *value, active)
        ),
        Type::Ledger(key, value) => format!(
            "ledger<{},{}>",
            canonical_type_shape(analysis, *key, active),
            canonical_type_shape(analysis, *value, active)
        ),
        Type::Record(fields) => {
            let mut fields = fields
                .iter()
                .map(|(name, ty)| {
                    format!(
                        "{}:{}",
                        analysis.interner.resolve(*name),
                        canonical_type_shape(analysis, *ty, active)
                    )
                })
                .collect::<Vec<_>>();
            fields.sort_unstable();
            format!("record{{{}}}", fields.join(","))
        }
        Type::Set(element) => format!("set<{}>", canonical_type_shape(analysis, *element, active)),
        Type::Promissum(element) => format!("promissum<{}>", canonical_type_shape(analysis, *element, active)),
        Type::PromissumFailable(success, alternate) => format!(
            "promissum-failable<{},{}>",
            canonical_type_shape(analysis, *success, active),
            canonical_type_shape(analysis, *alternate, active)
        ),
        Type::Cursor(element) => format!("cursor<{}>", canonical_type_shape(analysis, *element, active)),
        Type::AsyncCursor(item, alternate) => format!(
            "async-cursor<{},{}>",
            canonical_type_shape(analysis, *item, active),
            canonical_type_shape(analysis, *alternate, active)
        ),
        Type::Tensor(element, shape) => format!(
            "tensor<{},{}>",
            canonical_type_shape(analysis, *element, active),
            crate::semantic::format_figura(*shape, &analysis.types, &analysis.interner)
        ),
        Type::Vector(element, width) => format!(
            "vector<{},{}>",
            canonical_type_shape(analysis, *element, active),
            crate::semantic::format_figura(*width, &analysis.types, &analysis.interner)
        ),
        Type::Matrix(element, shape) => format!(
            "matrix<{},{}>",
            canonical_type_shape(analysis, *element, active),
            crate::semantic::format_figura(*shape, &analysis.types, &analysis.interner)
        ),
        Type::Sparsa(element, shape) => format!(
            "sparsa<{},{}>",
            canonical_type_shape(analysis, *element, active),
            crate::semantic::format_figura(*shape, &analysis.types, &analysis.interner)
        ),
        Type::Atomic(element) => format!("atomic<{}>", canonical_type_shape(analysis, *element, active)),
        Type::Intervallum(bound, _) => format!("intervallum<{}>", canonical_type_shape(analysis, *bound, active)),
        Type::SizedNumeric(primitive, width) => format!("sized-numeric:{primitive:?}:{}", width.faber_marker()),
        Type::ModularWord(width) => format!("modular-word:{}", width.faber_marker()),
        Type::RangedInt { policy, width, lo, hi } => {
            format!("ranged-int:{}:{}:{lo}:{hi}", policy.word(), width.faber_marker())
        }
        Type::Bits(width) => format!("bits:{}", width.faber_marker()),
        Type::SaturatingWord(width) => format!("saturating-word:{}", width.faber_marker()),
        Type::PolicyFloat(policy, width) => format!("policy-float:{}:{}", policy.word(), width.faber_marker()),
        Type::SizedInstans(precision) => format!("sized-instans:{precision:?}"),
        Type::Option(inner) => format!("option<{}>", canonical_type_shape(analysis, *inner, active)),
        Type::Ref(mutability, inner) => {
            format!("ref:{mutability:?}<{}>", canonical_type_shape(analysis, *inner, active))
        }
        Type::Struct(def_id) => format!("struct:{}", declaration_name(analysis, *def_id)),
        Type::Enum(def_id) => format!("enum:{}", declaration_name(analysis, *def_id)),
        Type::Interface(def_id) => format!("interface:{}", declaration_name(analysis, *def_id)),
        Type::Alias(def_id, resolved) => format!(
            "alias:{}={}",
            declaration_name(analysis, *def_id),
            canonical_type_shape(analysis, *resolved, active)
        ),
        Type::Func(signature) => {
            let params = signature
                .params
                .iter()
                .map(|param| format!("{:?}:{}", param.mode, canonical_type_shape(analysis, param.ty, active)))
                .collect::<Vec<_>>()
                .join(",");
            let error = signature
                .err
                .map(|error| canonical_type_shape(analysis, error, active))
                .unwrap_or_default();
            format!(
                "func<{};{};{};{};{}>",
                params,
                canonical_type_shape(analysis, signature.ret, active),
                error,
                signature.is_async,
                signature.is_generator
            )
        }
        Type::Param(symbol) => format!("param:{}", analysis.interner.resolve(*symbol)),
        Type::Applied(base, args) => {
            let args = args
                .iter()
                .map(|arg| match arg {
                    AppliedArg::Type(ty) => canonical_type_shape(analysis, *ty, active),
                    AppliedArg::Size(index) => {
                        crate::semantic::format_figura(*index, &analysis.types, &analysis.interner)
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("applied<{};{}>", canonical_type_shape(analysis, *base, active), args)
        }
        Type::Infer(_) => "infer".to_owned(),
        Type::InferUnion(_) => "infer-union".to_owned(),
        Type::MarkerHole { family, .. } => format!("marker-hole:{family:?}"),
        Type::Union(members) => {
            let mut members = members
                .iter()
                .map(|member| canonical_type_shape(analysis, *member, active))
                .collect::<Vec<_>>();
            members.sort_unstable();
            format!("union<{}>", members.join(","))
        }
        Type::Intersection(members) => {
            let mut members = members
                .iter()
                .map(|member| canonical_type_shape(analysis, *member, active))
                .collect::<Vec<_>>();
            members.sort_unstable();
            format!("intersection<{}>", members.join(","))
        }
        Type::Tuple(members) => format!(
            "tuple<{}>",
            members
                .iter()
                .map(|member| canonical_type_shape(analysis, *member, active))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::Error => "error".to_owned(),
    };
    active.remove(&type_id.0);
    shape
}

fn declaration_name(analysis: &AnalyzedModule, def_id: crate::hir::DefId) -> String {
    analysis
        .resolver
        .get_symbol(def_id)
        .map(|symbol| analysis.interner.resolve(symbol.name).to_owned())
        .or_else(|| {
            analysis
                .resolver
                .imported_spellings(&analysis.interner)
                .by_def
                .get(&def_id)
                .cloned()
        })
        .unwrap_or_else(|| "<external>".to_owned())
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
    let output = crate::codegen::faber::FaberCodegen::new(types, interner).generate(
        hir,
        &crate::codegen::faber::empty_presentation(),
        &crate::codegen::faber::LocaleKeywordBridge(surface),
    )?;
    Ok(FaberOutput { code: output.code })
}
