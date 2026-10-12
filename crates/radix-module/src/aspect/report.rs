//! HIR aspect recognition and report builder.
//!
//! Walks checked HIR to recognize aspect obligations (`discerne omnia`
//! exhaustiveness, etc.) and assembles a deterministic [`HirAspectReport`].
//! Recognition relies on checked HIR facts and the semantic [`TypeTable`]
//! rather than re-resolving source names. Enum-variant facts are borrowed from
//! the exhaustive-match pass so aspect recognition and pattern checking agree.

use crate::hir::visit::{HirVisitor, walk_expr, walk_item};
use crate::hir::{DefId, HirCasuArm, HirExpression, HirExpressionKind, HirId, HirItem, HirItemKind, HirModule};
use crate::lexer::{Interner, Span};

use super::air_mask_proof::{
    MaskNeutralityFamily, is_additive_body, is_additive_combine, is_integer_zero_seed, reduction_family,
};
use super::air_purity_proof::{GateHoleIssue, GatePurityFinding, PurityTerm, gate_purity_finding};
use super::air_unroll_proof::{UnrollRoute, unroll_route_facts};
use crate::semantic::TypeTable;
use crate::semantic::passes::exhaustive::{collect_enum_variants, enum_def_from_type, pattern_variant_id};
use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(test)]
#[path = "report_test.rs"]
mod tests;

#[cfg(test)]
#[path = "report_corpus_test.rs"]
mod corpus_tests;

/// Build a deterministic aspect report from checked HIR.
///
/// The caller provides the semantic type table so aspect recognition can rely on
/// checked HIR facts instead of re-resolving source names.
#[must_use]
pub fn report_hir_aspects(hir: &HirModule, types: &TypeTable) -> HirAspectReport {
    report_hir_aspects_with_optional_interner(hir, types, None)
}

/// Build an aspect report with source-name resolution available to concrete
/// AIR recognizers.  The legacy [`report_hir_aspects`] entry point stays
/// interner-free for callers that only need the backend-neutral Discerne
/// report; Line-2 callers use this entry point so method names are resolved
/// from the checked source table rather than from unstable `Symbol` numbers.
#[must_use]
pub fn report_hir_aspects_with_interner(hir: &HirModule, types: &TypeTable, interner: &Interner) -> HirAspectReport {
    report_hir_aspects_with_optional_interner(hir, types, Some(interner))
}

fn report_hir_aspects_with_optional_interner(
    hir: &HirModule,
    types: &TypeTable,
    interner: Option<&Interner>,
) -> HirAspectReport {
    let enum_variants = collect_enum_variants(hir);
    let mut collector = HirAspectCollector::new(hir, types, enum_variants, interner);
    collector.visit_program(hir);
    collector.finish()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirAspectPrecondition {
    /// The caller must provide HIR after the semantic pipeline has completed.
    CheckedHir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirAspectReport {
    pub precondition: HirAspectPrecondition,
    records: Vec<HirAspectRecord>,
}

impl HirAspectReport {
    // constructor: moves precondition into struct
    fn new(precondition: HirAspectPrecondition) -> Self {
        Self { precondition, records: Vec::new() }
    }

    #[cfg(test)]
    pub(crate) fn from_records(precondition: HirAspectPrecondition, records: Vec<HirAspectRecord>) -> Self {
        let mut report = Self { precondition, records };
        report.sort_records();
        report
    }

    #[must_use]
    pub fn records(&self) -> &[HirAspectRecord] {
        &self.records
    }

    fn push(&mut self, record: HirAspectRecord) {
        self.records.push(record);
    }

    fn sort_records(&mut self) {
        self.records.sort_by_key(HirAspectRecord::sort_key);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirAspectRecord {
    pub aspect: HirAspectKind,
    pub record_kind: HirAspectRecordKind,
    pub span: HirAspectSpan,
    pub node_id: Option<HirId>,
    pub reason: HirAspectReason,
    pub detail: HirAspectDetail,
}

impl HirAspectRecord {
    #[must_use]
    pub fn recognized(aspect: HirAspectKind, span: HirAspectSpan, node_id: Option<HirId>) -> Self {
        Self {
            aspect,
            record_kind: HirAspectRecordKind::Recognized,
            span,
            node_id,
            reason: HirAspectReason::Recognized,
            detail: HirAspectDetail::None,
        }
    }

    #[must_use]
    pub fn skipped_candidate(
        aspect: HirAspectKind,
        span: HirAspectSpan,
        node_id: Option<HirId>,
        reason: HirAspectReason,
    ) -> Self {
        Self {
            aspect,
            record_kind: HirAspectRecordKind::SkippedCandidate,
            span,
            node_id,
            reason,
            detail: HirAspectDetail::None,
        }
    }

    #[must_use]
    pub fn not_proven(
        aspect: HirAspectKind,
        span: HirAspectSpan,
        node_id: Option<HirId>,
        reason: HirAspectReason,
    ) -> Self {
        Self {
            aspect,
            record_kind: HirAspectRecordKind::NotProven,
            span,
            node_id,
            reason,
            detail: HirAspectDetail::None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: HirAspectDetail) -> Self {
        self.detail = detail;
        self
    }

    fn sort_key(&self) -> HirAspectRecordSortKey {
        HirAspectRecordSortKey {
            span_start: self.span.sort_start(),
            span_end: self.span.sort_end(),
            aspect: self.aspect,
            record_kind: self.record_kind,
            node_id: self.node_id.map_or(u32::MAX, |id| id.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirAspectDetail {
    None,
    DiscerneOmnia {
        variants: Vec<DefId>,
        covered: Vec<DefId>,
    },
    /// Structural contract consumed by the Line-2 Lean proof emitter.
    MaskNeutrality {
        family: MaskNeutralityFamily,
    },
    /// Structural contract consumed by the Line-1 Lean proof emitter: the
    /// settled AEU route and its lane sequence in source order.
    UnrollEquivalence {
        route: UnrollRoute,
        lanes: Vec<i64>,
    },
    /// Structural contract consumed by the Line-3 Lean proof emitter: the
    /// v1 scalar/let/call term of one admitted translated declaration.
    GatePurity {
        term: PurityTerm,
    },
    /// A named gate-hole finding (Line 3): a shape the mirrored gate policy
    /// still admits while the model finds an IO/monadic effect.  Never
    /// downgraded to an ignorable failed proof.
    GatePurityHole {
        issue: GateHoleIssue,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HirAspectKind {
    /// `discerne omnia` finite coverage over an enum-like scrutinee.
    DiscerneOmnia,

    /// AIR line 1: the emitted straight-line unroll preserves the source
    /// loop's lane sequence, source order, and failure behavior. Proof
    /// content is owned by APA-L1; registration here is identity only.
    UnrollEquivalence,

    /// AIR line 2: caller-built masks are neutral — zero-fill combines keep
    /// the live prefix and dead suffix unchanged. Proof content is owned by
    /// APA-L2; registration here is identity only.
    MaskNeutrality,

    /// AIR line 3: the admitted gate form inhabits the pure type fragment
    /// with no IO/monadic effects. Proof content is owned by APA-L3;
    /// registration here is identity only.
    GatePurity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HirAspectRecordKind {
    /// A concrete recognizer accepted this HIR candidate.
    Recognized,

    /// A concrete recognizer found the candidate but could not prove it.
    NotProven,

    /// HIR shape suggests a future aspect candidate, but this stage cannot
    /// prove it yet.
    SkippedCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirAspectSpan {
    /// Source span carried by the HIR node that produced the record.
    Source(Span),

    /// Explicit reason a record has no source span.
    NoSpan(HirAspectNoSpanReason),
}

impl HirAspectSpan {
    pub(crate) fn sort_start(&self) -> u32 {
        match self {
            HirAspectSpan::Source(span) => span.start,
            HirAspectSpan::NoSpan(_) => u32::MAX,
        }
    }

    pub(crate) fn sort_end(&self) -> u32 {
        match self {
            HirAspectSpan::Source(span) => span.end,
            HirAspectSpan::NoSpan(_) => u32::MAX,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirAspectNoSpanReason {
    /// The record summarizes a whole HIR program rather than a specific node.
    ProgramSummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirAspectReason {
    /// Placeholder used by future concrete recognizers.
    Recognized,

    /// The dispatch is not marked `omnia`.
    NonOmniaDiscerne,

    /// `omnia` was used with anything other than one scrutinee.
    MultipleScrutinees,

    /// The scrutinee is missing checked type information.
    MissingScrutineeType,

    /// The checked scrutinee type does not resolve to an enum definition.
    NonEnumScrutinee,

    /// The enum definition could not be found in the current HIR item table.
    EnumVariantsUnavailable,

    /// At least one reachable `omnia` arm used a non-variant pattern.
    NonVariantPattern,

    /// At least one enum variant is not covered by an unguarded explicit
    /// variant pattern.
    MissingExplicitVariants,

    /// A reduction/scan candidate did not carry its two required arguments.
    MissingReductionArguments,

    /// A reduction/scan candidate used a non-additive combine shape.
    NonAdditiveCombine,

    /// A reduction/scan candidate did not use the explicit integer-zero seed.
    NonNeutralSeed,

    /// A candidate supplied an in-region or otherwise unsupported mask shape.
    UnsupportedMaskShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct HirAspectRecordSortKey {
    span_start: u32,
    span_end: u32,
    aspect: HirAspectKind,
    record_kind: HirAspectRecordKind,
    node_id: u32,
}

struct HirAspectCollector<'a> {
    hir: &'a HirModule,
    types: &'a TypeTable,
    enum_variants: FxHashMap<DefId, Vec<DefId>>,
    interner: Option<&'a Interner>,
    report: HirAspectReport,
}

impl<'a> HirAspectCollector<'a> {
    fn new(
        hir: &'a HirModule,
        types: &'a TypeTable,
        enum_variants: FxHashMap<DefId, Vec<DefId>>,
        interner: Option<&'a Interner>,
    ) -> Self {
        Self { hir, types, enum_variants, interner, report: HirAspectReport::new(HirAspectPrecondition::CheckedHir) }
    }

    fn finish(mut self) -> HirAspectReport {
        self.report.sort_records();
        self.report
    }

    fn record_discerne(
        &mut self,
        expr: &HirExpression,
        scrutinees: &[HirExpression],
        arms: &[HirCasuArm],
        exhaustive: bool,
    ) {
        if !exhaustive {
            self.push_skipped(expr, HirAspectReason::NonOmniaDiscerne);
            return;
        }

        if scrutinees.len() != 1 {
            self.push_skipped(expr, HirAspectReason::MultipleScrutinees);
            return;
        }

        let scrutinee = &scrutinees[0];
        let Some(scrutinee_ty) = scrutinee.ty else {
            self.push_skipped(expr, HirAspectReason::MissingScrutineeType);
            return;
        };
        let Some(enum_def) = enum_def_from_type(scrutinee_ty, self.types) else {
            self.push_skipped(expr, HirAspectReason::NonEnumScrutinee);
            return;
        };
        let Some(expected_variants) = self.enum_variants.get(&enum_def) else {
            self.push_skipped(expr, HirAspectReason::EnumVariantsUnavailable);
            return;
        };

        let coverage = explicit_variant_coverage(arms);
        let covered_variants = expected_variants
            .iter()
            .copied()
            .filter(|variant| coverage.covered_variants.contains(variant))
            .collect::<Vec<_>>();
        let detail = HirAspectDetail::DiscerneOmnia { variants: expected_variants.clone(), covered: covered_variants };
        let reason = if coverage.has_non_variant_pattern {
            Some(HirAspectReason::NonVariantPattern)
        } else if expected_variants
            .iter()
            .any(|variant| !coverage.covered_variants.contains(variant))
        {
            Some(HirAspectReason::MissingExplicitVariants)
        } else {
            None
        };

        let record = match reason {
            Some(reason) => HirAspectRecord::not_proven(
                HirAspectKind::DiscerneOmnia,
                HirAspectSpan::Source(expr.span),
                Some(expr.id),
                reason,
            )
            .with_detail(detail),
            None => HirAspectRecord::recognized(
                HirAspectKind::DiscerneOmnia,
                HirAspectSpan::Source(expr.span),
                Some(expr.id),
            )
            .with_detail(detail),
        };
        self.report.push(record);
    }

    fn record_mask_neutrality(
        &mut self,
        expr: &HirExpression,
        method: crate::lexer::Symbol,
        args: &[crate::hir::HirCallArg],
    ) {
        let Some(interner) = self.interner else {
            return;
        };
        let method_name = interner.resolve(method);
        let Some(family) = reduction_family(method_name) else {
            return;
        };

        let detail = HirAspectDetail::MaskNeutrality { family };
        let reason = if args.len() > 2 {
            Some(HirAspectReason::UnsupportedMaskShape)
        } else if args.len() != 2 {
            Some(HirAspectReason::MissingReductionArguments)
        } else if !is_additive_combine_in_program(&args[0].expr, self.hir) {
            Some(HirAspectReason::NonAdditiveCombine)
        } else if !is_integer_zero_seed(&args[1].expr) {
            Some(HirAspectReason::NonNeutralSeed)
        } else {
            None
        };

        let record = match reason {
            Some(reason) => HirAspectRecord::not_proven(
                HirAspectKind::MaskNeutrality,
                HirAspectSpan::Source(expr.span),
                Some(expr.id),
                reason,
            )
            .with_detail(detail),
            None => HirAspectRecord::recognized(
                HirAspectKind::MaskNeutrality,
                HirAspectSpan::Source(expr.span),
                Some(expr.id),
            )
            .with_detail(detail),
        };
        self.report.push(record);
    }

    fn record_unroll_equivalence(
        &mut self,
        expr: &HirExpression,
        mode: crate::hir::HirIteraMode,
        coordinates: &[(DefId, crate::lexer::Symbol)],
        source: &HirExpression,
    ) {
        // Decline shapes carry no proof obligation (spec: decline paths emit
        // none), so only settled route facts produce a record.
        let Some(facts) = unroll_route_facts(mode, coordinates.len(), source, self.types) else {
            return;
        };
        self.report.push(
            HirAspectRecord::recognized(
                HirAspectKind::UnrollEquivalence,
                HirAspectSpan::Source(expr.span),
                Some(expr.id),
            )
            .with_detail(HirAspectDetail::UnrollEquivalence { route: facts.route, lanes: facts.lanes }),
        );
    }

    fn record_gate_purity(&mut self, item: &HirItem, function: &crate::hir::HirFunction) {
        // Decline shapes (outside the v1 subset, or forms the gate itself
        // rejects) carry no Line-3 obligation.  A gate hole is a named
        // finding in the report — never a suppressed failure.
        let detail = match gate_purity_finding(function) {
            GatePurityFinding::Admitted(term) => HirAspectDetail::GatePurity { term },
            GatePurityFinding::GateHole { issue } => HirAspectDetail::GatePurityHole { issue },
            GatePurityFinding::NoObligation => return,
        };
        self.report.push(
            HirAspectRecord::recognized(HirAspectKind::GatePurity, HirAspectSpan::Source(item.span), Some(item.id))
                .with_detail(detail),
        );
    }

    fn push_skipped(&mut self, expr: &HirExpression, reason: HirAspectReason) {
        self.report.push(HirAspectRecord::skipped_candidate(
            HirAspectKind::DiscerneOmnia,
            HirAspectSpan::Source(expr.span),
            Some(expr.id),
            reason,
        ));
    }
}

fn is_additive_combine_in_program(expr: &HirExpression, hir: &HirModule) -> bool {
    let HirExpressionKind::Path(def_id) = expr.kind else {
        return is_additive_combine(expr);
    };

    let Some(item) = hir.items.iter().find(|item| item.def_id == def_id) else {
        return false;
    };
    let HirItemKind::Function(function) = &item.kind else {
        return false;
    };
    let Some(body) = &function.body else {
        return false;
    };

    if let Some(expr) = body.expr.as_deref() {
        return is_additive_body(expr);
    }

    body.statements
        .iter()
        .rev()
        .find_map(|statement| match &statement.kind {
            crate::hir::HirStatementKind::Redde(Some(expr)) => Some(is_additive_body(expr)),
            _ => None,
        })
        == Some(true)
}

impl HirVisitor for HirAspectCollector<'_> {
    fn visit_item(&mut self, item: &HirItem) {
        if let HirItemKind::Function(function) = &item.kind {
            self.record_gate_purity(item, function);
        }
        walk_item(self, item);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Discerne { scrutinees, arms, exhaustive } = &expr.kind {
            self.record_discerne(expr, scrutinees, arms, *exhaustive);
        }
        if let HirExpressionKind::MethodCall(_, method, _, args, _) = &expr.kind {
            self.record_mask_neutrality(expr, *method, args);
        }
        if let HirExpressionKind::Itera { mode, coordinates, iter: source, .. } = &expr.kind {
            self.record_unroll_equivalence(expr, *mode, coordinates, source);
        }

        walk_expr(self, expr);
    }
}

struct ExplicitVariantCoverage {
    covered_variants: FxHashSet<DefId>,
    has_non_variant_pattern: bool,
}

fn explicit_variant_coverage(arms: &[HirCasuArm]) -> ExplicitVariantCoverage {
    let mut covered_variants = FxHashSet::default();
    let mut has_non_variant_pattern = false;

    for arm in arms {
        for pattern in &arm.patterns {
            if let Some(variant) = pattern_variant_id(pattern) {
                covered_variants.insert(variant);
            } else {
                has_non_variant_pattern = true;
            }
        }
    }

    ExplicitVariantCoverage { covered_variants, has_non_variant_pattern }
}
