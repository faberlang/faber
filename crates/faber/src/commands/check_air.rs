//! Advisory AIR-lane compatibility report for `faber check --air`.
//!
//! Re-runs the live gate (`air_unroll` + `air_purity`) over a synthetic lane
//! map — every unannotated top-level function overlaid with
//! `RadixLane::Air`, real annotations preserved — on cloned artifacts so the
//! mutating unroll pass never touches the real pipeline result. Report-only:
//! verdicts never change exit status or diagnostics.

use radix::module::hir::visit::{HirVisitor, walk_expr};
use radix::module::hir::{
    DefId, HirBlock, HirExpression, HirExpressionKind, HirFunction, HirItemKind, HirIteraMode,
};
use radix::module::lexer::Span;
use radix::module::semantic::passes::{air_purity, air_unroll};
use radix::module::semantic::tensor_type_directed::is_tensor_type;
use radix::module::semantic::{
    PassConfig, Primitive, RadixLane, SemanticError, Type, TypeId, TypeTable,
};
use std::collections::{HashMap, HashSet};

/// Verdict for one top-level function under a synthetic AIR-lane mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AirVerdict {
    Admissible,
    WouldReject,
    NotATarget,
}

impl AirVerdict {
    /// Census tier label for one row: the verdict plus the row's annotation
    /// state split the admissible tier into kernel entry vs one annotation
    /// away; the other verdicts are annotation-independent.
    pub(crate) fn census_label(self, annotated: bool) -> &'static str {
        match (self, annotated) {
            (Self::Admissible, true) => "ADMISSIBLE-KERNEL",
            (Self::Admissible, false) => "ADMISSIBLE-ONE-AWAY",
            (Self::WouldReject, _) => "WOULD-REJECT",
            (Self::NotATarget, _) => "NOT-A-TARGET",
        }
    }
}

/// One rejection reason with its rule id, source location, and attribution.
///
/// `line` is the 1-based human-table location; `start`/`end` are the exact
/// byte span of the offending construct for machine rows. `via_callee`
/// marks reasons attributed through a callee closure rather than the row's
/// own body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AirReason {
    pub rule: String,
    pub line: usize,
    pub start: u32,
    pub end: u32,
    pub via_callee: bool,
}

/// One report row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AirRow {
    pub name: String,
    pub verdict: AirVerdict,
    pub annotated: bool,
    pub reasons: Vec<AirReason>,
    pub note: Option<String>,
}

/// The unit-local facts needed to attribute AIR blockers without crossing a
/// module boundary. Calls are already resolved in HIR as definition identities;
/// an identity absent from this table is deliberately not treated as a callee
/// with an inferred blocker.
#[derive(Debug, Clone)]
struct FunctionRecord {
    def_id: DefId,
    name: String,
    numeric: bool,
    annotated: bool,
    has_itera_ab: bool,
    own_reasons: Vec<AirReason>,
    unresolved_reasons: Vec<AirReason>,
    calls: Vec<DefId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RootBlocker {
    name: String,
    rule: String,
    line: usize,
    start: u32,
    end: u32,
}

/// Advisory AIR report is single-file only for now (pattern of the package
/// check message).
pub const PACKAGE_AIR_MESSAGE: &str = "error: `faber check --air` package mode is not supported yet; use `faber check --air <single-file.fab>`";

/// Header shown above the table. Snapshot honesty: the report reflects the
/// live gate, so verdicts legitimately change when gate rulings change.
pub const AIR_REPORT_HEADER: &str =
    "air lane advisory report — reflects the live gate; verdicts track gate changes";

/// Compute the advisory report for one source file.
///
/// Returns `None` when the file does not reach semantic analysis cleanly
/// (lex/parse/CLI errors) — the plain check pipeline already reported those,
/// and a report over a broken module would be noise.
#[must_use]
pub fn advisory_rows(_name: &str, source: &str) -> Option<Vec<AirRow>> {
    // TEMPORARY STAGING: a pack-less source reads the
    // no-locale default while `RADIX_DEFAULT_CODE_LOCALE=en`; unset keeps the
    // historical pack-less (Latin) reading.
    let code_pack =
        radix::module::driver::default_code_pack(&radix::module::driver::Config::default());
    let lex_result = match code_pack.as_ref() {
        Some(pack) => radix::module::lexer::lex_with_locale_pack(source, pack),
        None => radix::module::lexer::lex(source),
    };
    if !lex_result.success() {
        return None;
    }
    let mut parsed = radix::module::parser::parse(lex_result);
    if !parsed.errors.is_empty() {
        return None;
    }
    let program = parsed.program.take()?;
    let mut interner = parsed.interner;
    let cli_analysis = radix::cli::analyze(&program, &interner);
    if !cli_analysis.errors.is_empty() {
        return None;
    }
    let config = PassConfig::for_target(radix::module::codegen::Target::HirRust);
    let result = radix::module::semantic::analyze_with_cli(
        &program,
        &config,
        &mut interner,
        cli_analysis.program.as_ref(),
    );
    let hir = result.hir?;

    // Synthetic lane map: real annotations preserved, every unannotated
    // top-level function overlaid with the AIR lane.
    let mut lanes = result.radix_lanes.clone();
    let mut functions = Vec::new();
    for item in &hir.items {
        let HirItemKind::Function(function) = &item.kind else {
            continue;
        };
        if lanes.lane_for(item.def_id).is_none() {
            lanes.insert(item.def_id, RadixLane::Air);
        }
        functions.push((item.def_id, item.span, function));
    }

    // Gate re-run on cloned artifacts: `air_unroll` mutates in place, so it
    // must never see the real pipeline's `hir`/`interner`.
    let mut hir_clone = hir.clone();
    let mut presentation_clone = result.presentation.clone();
    let mut interner_clone = interner.clone();
    let unroll_errors = air_unroll::unroll(
        &mut hir_clone,
        &mut presentation_clone,
        &lanes,
        &mut interner_clone,
        &result.types,
    )
    .err()
    .unwrap_or_default();
    let purity_errors = air_purity::check(
        &mut hir_clone,
        &mut lanes,
        &mut interner_clone,
        &result.types,
    )
    .err()
    .unwrap_or_default();
    let mut gate_errors = unroll_errors;
    gate_errors.extend(purity_errors);

    let mut records = Vec::new();
    for (def_id, span, function) in functions {
        let annotated = result.radix_lanes.lane_for(def_id).is_some();
        let name = interner.resolve(function.name).to_owned();
        let numeric = signature_is_numeric_or_tensor(function, &result.types);
        let mut calls = CallFinder::default();
        if let Some(body) = function.body.as_ref() {
            calls.visit_block(body);
        }

        let mut own_reasons = Vec::new();
        let mut unresolved_reasons = Vec::new();
        for err in gate_errors
            .iter()
            .filter(|err| span_contains(span, err.span))
        {
            let reason = AirReason {
                rule: error_rule(err),
                line: line_of(source, err.span),
                start: err.span.start,
                end: err.span.end,
                via_callee: false,
            };
            if reason.rule == "non_air_call" {
                // A non-AIR call is a boundary fact, not evidence about the
                // callee's body. Keep it as an unresolved note rather than
                // inventing a blocker from an unavailable module.
                unresolved_reasons.push(AirReason {
                    rule: "callee-unresolved".to_owned(),
                    line: reason.line,
                    start: reason.start,
                    end: reason.end,
                    via_callee: false,
                });
            } else {
                own_reasons.push(reason);
            }
        }
        rank_reasons(&mut own_reasons);
        records.push(FunctionRecord {
            def_id,
            name,
            numeric,
            annotated,
            has_itera_ab: contains_itera_ab(function.body.as_ref()),
            own_reasons,
            unresolved_reasons,
            calls: calls.0,
        });
    }

    let records_by_def: HashMap<DefId, FunctionRecord> = records
        .iter()
        .cloned()
        .map(|record| (record.def_id, record))
        .collect();
    let mut rows = Vec::new();
    for record in records {
        let mut reasons = record.own_reasons.clone();
        let mut closure_reasons = Vec::new();
        for callee in &record.calls {
            let mut visiting = HashSet::default();
            if let Some(blocker) = first_blocker_for(*callee, &records_by_def, &mut visiting) {
                closure_reasons.push(AirReason {
                    rule: format!("blocked by callee `{}`: {}", blocker.name, blocker.rule),
                    line: blocker.line,
                    start: blocker.start,
                    end: blocker.end,
                    via_callee: true,
                });
            }
        }
        dedupe_reasons(&mut closure_reasons);
        rank_reasons(&mut closure_reasons);
        reasons.extend(closure_reasons);
        // Boundary notes are intentionally last: they carry no claim about a
        // callee's body and must not outrank an actionable local blocker.
        reasons.extend(record.unresolved_reasons.clone());

        let (verdict, note) = if !record.numeric {
            (
                AirVerdict::NotATarget,
                Some("no numeric/tensor input or output — nothing to differentiate".to_owned()),
            )
        } else if !reasons.is_empty() {
            (AirVerdict::WouldReject, None)
        } else {
            let note = record
                .has_itera_ab
                .then(|| format!("itera ab unrolls (budget {})", air_unroll::UNROLL_BUDGET));
            (AirVerdict::Admissible, note)
        };
        rows.push(AirRow {
            name: record.name,
            verdict,
            annotated: record.annotated,
            reasons,
            note,
        });
    }
    Some(rows)
}

#[derive(Default)]
struct CallFinder(Vec<DefId>);

impl HirVisitor for CallFinder {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Call(callee, _, _, _) = &expr.kind
            && let HirExpressionKind::Path(def_id) = &callee.kind
        {
            self.0.push(*def_id);
        }
        walk_expr(self, expr);
    }
}

fn first_blocker_for(
    def_id: DefId,
    records: &HashMap<DefId, FunctionRecord>,
    visiting: &mut HashSet<DefId>,
) -> Option<RootBlocker> {
    let record = records.get(&def_id)?;
    // A non-target callee is not evidence that its caller is blocked. The
    // report only closes over functions that themselves claim an AIR-relevant
    // numeric/tensor signature.
    if !record.numeric || !visiting.insert(def_id) {
        return None;
    }

    let mut candidates = record
        .own_reasons
        .iter()
        .filter(|reason| reason.rule != "callee-unresolved")
        .map(|reason| RootBlocker {
            name: record.name.clone(),
            rule: reason.rule.clone(),
            line: reason.line,
            start: reason.start,
            end: reason.end,
        })
        .collect::<Vec<_>>();
    for callee in &record.calls {
        if let Some(blocker) = first_blocker_for(*callee, records, visiting) {
            candidates.push(blocker);
        }
    }
    visiting.remove(&def_id);

    candidates.sort_by_key(|candidate| reason_bucket(&candidate.rule));
    candidates.into_iter().next()
}

fn dedupe_reasons(reasons: &mut Vec<AirReason>) {
    let mut unique = Vec::with_capacity(reasons.len());
    for reason in reasons.drain(..) {
        if !unique.contains(&reason) {
            unique.push(reason);
        }
    }
    *reasons = unique;
}

/// Order the live gate's reasons by the settled ruling tiers. The final
/// fallback is deliberate: a newly named reason must remain visible, but it
/// must not outrank a reason whose bucket is known.
pub(crate) fn rank_reasons(reasons: &mut [AirReason]) {
    reasons.sort_by_key(|reason| reason_bucket(&reason.rule));
}

fn reason_bucket(rule: &str) -> u8 {
    let base = rule.split(':').next().unwrap_or(rule);
    match base {
        // Effects: these cannot become AIR dataflow by a local spelling tweak.
        "mori" | "diagnostic_output" | "input_read" | "runtime_assertion" => 0,
        // Structural: the AIR companion or representational boundary is
        // missing, including unknown-trip-count control flow.
        "async_function"
        | "generator_function"
        | "cede/await/yield"
        | "cli_mounted_function"
        | "runtime_provided_binding"
        | "missing_body"
        | "non_air_call"
        | "method_call"
        | "tensor_method_receiver"
        | "kernel_device_function"
        | "loop"
        | "dum_loop"
        | "itera_loop"
        | "breakable_block"
        | "recoverable_handler"
        | "iace"
        | "expression_ad" => 1,
        // Syntax/transform tier: the source form can be made explicit or
        // moved to one of the admitted dataflow surfaces.
        "defaulted_parameter"
        | "expression_statement"
        | "named_call_arg"
        | "spread_call_arg"
        | "runtime_conversion"
        | "air_store_not_proven"
        | "air_integer_range_unproven"
        | "reference_place_op"
        | "runtime_variant_check"
        | "mutable_varia"
        | "assignment_mutation"
        | "inc_dec_mutation"
        | "optional_null_chain"
        | "closure"
        | "gradient_expression_unsupported"
        | "tacet"
        | "deferred_local_init" => 2,
        _ => 3,
    }
}

/// Render the report table to `out` (one line per row, `name VERDICT [annotated] reasons/note`).
pub fn render_report(out: &mut dyn std::io::Write, rows: &[AirRow]) -> std::io::Result<()> {
    writeln!(out, "{AIR_REPORT_HEADER}")?;
    let width = rows.iter().map(|row| row.name.len()).max().unwrap_or(0);
    for row in rows {
        write!(
            out,
            "{:<width$}  {:<20}",
            row.name,
            row.verdict.census_label(row.annotated)
        )?;
        if row.annotated {
            write!(out, "annotated ")?;
        }
        let parts: Vec<String> = row
            .reasons
            .iter()
            .map(|reason| format!("{} (l.{})", reason.rule, reason.line))
            .chain(row.note.clone())
            .collect();
        writeln!(out, "{}", parts.join("; "))?;
    }
    Ok(())
}

/// Whether any parameter or the return type is numeric or tensor typed.
fn signature_is_numeric_or_tensor(function: &HirFunction, types: &TypeTable) -> bool {
    function
        .params
        .iter()
        .map(|param| param.ty)
        .chain(function.ret_ty)
        .any(|ty| is_numeric_or_tensor(ty, types))
}

fn is_numeric_or_tensor(ty: TypeId, types: &TypeTable) -> bool {
    if is_tensor_type(ty, types) {
        return true;
    }
    match types.get(ty) {
        Type::Alias(_, inner) => is_numeric_or_tensor(*inner, types),
        Type::Primitive(Primitive::Numerus | Primitive::Fractus) => true,
        Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _) => true,
        Type::ModularWord(_) | Type::SaturatingWord(_) => true,
        _ => false,
    }
}

/// Rule id for one gate error: the `issue` arg, with the decline `causa`
/// folded in for `itera_loop` declines.
fn error_rule(err: &SemanticError) -> String {
    let issue = err
        .args
        .iter()
        .find(|arg| arg.name == "issue")
        .map(|arg| arg.value.clone())
        .unwrap_or_else(|| "air".to_owned());
    if issue == "itera_loop"
        && let Some(causa) = err.args.iter().find(|arg| arg.name == "causa")
    {
        return format!("{issue}:{}", causa.value);
    }
    issue
}

fn line_of(source: &str, span: Span) -> usize {
    1 + source
        .as_bytes()
        .get(..span.start as usize)
        .map(|prefix| prefix.iter().filter(|&&byte| byte == b'\n').count())
        .unwrap_or(0)
}

fn span_contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

fn contains_itera_ab(body: Option<&HirBlock>) -> bool {
    let Some(body) = body else {
        return false;
    };
    let mut finder = IteraAbFinder(false);
    finder.visit_block(body);
    finder.0
}

struct IteraAbFinder(bool);

impl HirVisitor for IteraAbFinder {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Itera {
            mode: HirIteraMode::Ab,
            ..
        } = &expr.kind
        {
            self.0 = true;
        }
        walk_expr(self, expr);
    }
}
