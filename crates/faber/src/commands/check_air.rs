//! Advisory AIR-lane compatibility report for `faber check --air`.
//!
//! Re-runs the live gate (`air_unroll` + `air_purity`) over a synthetic lane
//! map — every unannotated top-level function overlaid with
//! `RadixLane::Air`, real annotations preserved — on cloned artifacts so the
//! mutating unroll pass never touches the real pipeline result. Report-only:
//! verdicts never change exit status or diagnostics.

use radix::module::hir::visit::{walk_expr, walk_stmt, HirVisitor};
use radix::module::hir::{
    HirBlock, HirExpression, HirExpressionKind, HirFunction, HirItemKind, HirIteraMode,
    HirStatement,
};
use radix::module::lexer::Span;
use radix::module::semantic::passes::{air_purity, air_unroll};
use radix::module::semantic::tensor_type_directed::is_tensor_type;
use radix::module::semantic::{
    PassConfig, Primitive, RadixLane, SemanticError, Type, TypeTable, TypeId,
};

/// Verdict for one top-level function under a synthetic AIR-lane mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AirVerdict {
    Admissible,
    WouldReject,
    NotATarget,
}

impl AirVerdict {
    fn label(self) -> &'static str {
        match self {
            Self::Admissible => "ADMISSIBLE",
            Self::WouldReject => "WOULD-REJECT",
            Self::NotATarget => "NOT-A-TARGET",
        }
    }
}

/// One rejection reason with its rule id and 1-based source line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AirReason {
    pub rule: String,
    pub line: usize,
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

/// Advisory AIR report is single-file only for now (pattern of the package
/// check message).
pub const PACKAGE_AIR_MESSAGE: &str =
    "error: `faber check --air` package mode is not supported yet; use `faber check --air <single-file.fab>`";

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
    let lex_result = radix::module::lexer::lex(source);
    if !lex_result.success() {
        return None;
    }
    let mut parsed = radix::module::parser::parse(lex_result);
    if !parsed.errors.is_empty() {
        return None;
    }
    let Some(program) = parsed.program.take() else {
        return None;
    };
    let mut interner = parsed.interner;
    let cli_analysis = radix::cli::analyze(&program, &interner);
    if !cli_analysis.errors.is_empty() {
        return None;
    }
    let mut config =
        PassConfig::for_target(radix::module::codegen::Target::HirRust);
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
    let mut interner_clone = interner.clone();
    let unroll_errors = air_unroll::unroll(&mut hir_clone, &lanes, &mut interner_clone, &result.types)
        .err()
        .unwrap_or_default();
    let purity_errors = air_purity::check(&hir_clone, &lanes, &interner_clone, &result.types)
        .err()
        .unwrap_or_default();
    let mut gate_errors = unroll_errors;
    gate_errors.extend(purity_errors);

    let mut rows = Vec::new();
    for (def_id, span, function) in functions {
        let annotated = result.radix_lanes.lane_for(def_id).is_some();
        let name = interner.resolve(function.name).to_owned();
        let numeric = signature_is_numeric_or_tensor(function, &result.types);
        let own_errors: Vec<&SemanticError> = gate_errors
            .iter()
            .filter(|err| span_contains(span, err.span))
            .collect();
        let reasons: Vec<AirReason> = own_errors
            .iter()
            .map(|err| AirReason {
                rule: error_rule(err),
                line: line_of(source, err.span),
            })
            .collect();
        let (verdict, note) = if !numeric {
            (AirVerdict::NotATarget, Some("no numeric/tensor input or output — nothing to differentiate".to_owned()))
        } else if !reasons.is_empty() {
            (AirVerdict::WouldReject, None)
        } else {
            let note = contains_itera_ab(function.body.as_ref())
                .then(|| format!("itera ab unrolls (budget {})", air_unroll::UNROLL_BUDGET));
            (AirVerdict::Admissible, note)
        };
        rows.push(AirRow {
            name,
            verdict,
            annotated,
            reasons,
            note,
        });
    }
    Some(rows)
}

/// Render the report table to `out` (one line per row, `name VERDICT [annotated] reasons/note`).
pub fn render_report(out: &mut dyn std::io::Write, rows: &[AirRow]) -> std::io::Result<()> {
    writeln!(out, "{AIR_REPORT_HEADER}")?;
    let width = rows.iter().map(|row| row.name.len()).max().unwrap_or(0);
    for row in rows {
        write!(out, "{:<width$}  {:<13}", row.name, row.verdict.label())?;
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
        Type::ModularWord(_) => true,
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
    if issue == "itera_loop" {
        if let Some(causa) = err.args.iter().find(|arg| arg.name == "causa") {
            return format!("{issue}:{}", causa.value);
        }
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
    fn visit_block(&mut self, block: &HirBlock) {
        for stmt in &block.statements {
            self.visit_stmt(stmt);
        }
        if let Some(expr) = &block.expr {
            self.visit_expr(expr);
        }
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Itera(HirIteraMode::Ab, ..) = &expr.kind {
            self.0 = true;
        }
        walk_expr(self, expr);
    }
}
