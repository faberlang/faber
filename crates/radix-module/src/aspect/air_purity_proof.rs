//! Lean proof source and recognizer for AIR gate/purity correspondence.
//!
//! The proof is deliberately structural (Line 3, v1 subset: scalar leaves,
//! immutable `let` bindings, calls).  For each admitted translated
//! declaration it establishes two things inside the Lean model: the term
//! inhabits the pure type fragment (`Except Effect PureValue` landing in
//! `ok`), and it carries no `IO` or monadic effect.  It claims nothing
//! about semantic preservation, numerics, or execution.
//!
//! The live gate (`radix-semantic` `air_purity.rs`) is strictly read-only
//! here.  The recognizer mirrors its v1-relevant admission decisions and
//! runs an independent model judgment; a shape the gate mirror would still
//! admit while the model finds an effect surfaces as a **named gate-hole
//! finding** — never as a downgraded, ignorable failed proof.
//!
//! Two such holes are expressible in v1 checked HIR and both are surfaced:
//! the gate never inspects `HirLocal::is_await` (a monadic suspension bound
//! through an otherwise-admissible `let`), and it silently accepts
//! `HirExpressionKind::Error` nodes inside an otherwise-admissible body.

use crate::hir::{HirExpression, HirExpressionKind, HirFunction, HirLiteral, HirLocal, HirStatementKind};

/// Structural term for one v1-admitted declaration body.
///
/// Scalars keep their literal magnitude (paths carry `0`); calls are curried
/// left so multi-argument calls nest `call (call f a) b`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurityTerm {
    /// Scalar leaf: literal magnitude or a bound scalar path.
    Scalar(i64),
    /// Immutable `let` binding: initializer term, then the body term.
    Let(Box<PurityTerm>, Box<PurityTerm>),
    /// Call: callee term and argument terms (rendered curried left).
    Call(Box<PurityTerm>, Vec<PurityTerm>),
}

impl PurityTerm {
    /// Render the term and its gate-admission derivation as Lean source.
    /// Composite subterms are parenthesized so nesting stays unambiguous.
    fn render(&self) -> (String, String) {
        match self {
            Self::Scalar(value) => (format!("Term.scalar {value}"), format!("GateAdmits.scalar {value}")),
            Self::Let(value, body) => {
                let (value_term, value_proof) = value.render();
                let (body_term, body_proof) = body.render();
                (
                    format!("Term.letIn ({value_term}) ({body_term})"),
                    format!("GateAdmits.letIn ({value_proof}) ({body_proof})"),
                )
            }
            Self::Call(callee, args) => {
                let (mut term, mut proof) = callee.render();
                for arg in args {
                    let (arg_term, arg_proof) = arg.render();
                    term = format!("Term.call ({term}) ({arg_term})");
                    proof = format!("GateAdmits.call ({proof}) ({arg_proof})");
                }
                (term, proof)
            }
        }
    }
}

/// Named gate-hole issues surfaced by the Line-3 recognizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateHoleIssue {
    /// A local bound with `is_await` — a monadic suspension the purity gate
    /// never inspects inside an otherwise-admissible `let`.
    AwaitingLocalInit,
    /// An `HirExpressionKind::Error` node the purity gate accepts silently
    /// inside an otherwise-admissible body.
    UntranslatableErrorNode,
}

impl GateHoleIssue {
    /// Stable finding label used in record details and checker evidence.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::AwaitingLocalInit => "gate_hole.awaiting_local_init",
            Self::UntranslatableErrorNode => "gate_hole.untranslatable_error_node",
        }
    }
}

/// Classification of one function declaration against the Line-3 recognizer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatePurityFinding {
    /// Inside the v1 scalar/let/call subset with a complete pure term:
    /// carries a Line-3 proof obligation.
    Admitted(PurityTerm),
    /// Outside the v1 subset, or carrying a form the gate itself rejects:
    /// no Line-3 obligation.
    NoObligation,
    /// A shape the mirrored gate policy still admits while the model finds
    /// an IO/monadic effect.  A named gate-hole finding; it is never
    /// downgraded to an ignorable failed proof.
    GateHole { issue: GateHoleIssue },
}

/// Classify one checked-HIR function declaration for the Line-3 subset.
///
/// The gate mirror applies the v1-relevant `air_purity` decisions (no
/// async/generator/cli-args/defaulted params; immutable, initialized,
/// non-runtime-provided locals; no named/spread call arguments).  The model
/// judgment then admits only scalar leaves, immutable `let` bindings, and
/// positional calls.  Anything else declines with no obligation — except a
/// shape the gate mirror still admits while the model finds an effect,
/// which is returned as a named gate hole.
#[must_use]
pub fn gate_purity_finding(function: &HirFunction) -> GatePurityFinding {
    if function.is_async || function.is_generator || function.cli_args.is_some() {
        return GatePurityFinding::NoObligation;
    }
    if function.params.iter().any(|param| param.default.is_some()) {
        return GatePurityFinding::NoObligation;
    }
    // Device kernel/fragment admission depends on lane metadata the aspect
    // report cannot see; those shapes carry no v1 obligation either way.
    if function.is_kernel || function.nucleum_fragment {
        return GatePurityFinding::NoObligation;
    }
    let Some(body) = &function.body else {
        return GatePurityFinding::NoObligation;
    };

    let mut walk = Walk::default();
    walk_block(body, &mut walk);
    walk.finish()
}

/// Model judgment for one initializer/argument/value expression.
enum ValueJudgment {
    Pure(PurityTerm),
    OutsideSubset,
    GateRejects,
    Hole(GateHoleIssue),
}

/// Accumulated judgments for one body walk.
#[derive(Default)]
struct Walk {
    /// Initializer terms of the immutable `let` chain, in source order.
    lets: Vec<PurityTerm>,
    /// The single value form ending the chain (`redde` or tail expression).
    body: Option<PurityTerm>,
    /// A form outside the v1 scalar/let/call subset was seen.
    outside_subset: bool,
    /// A form the gate itself rejects was seen.
    gate_rejects: bool,
    /// A gate-admitted impure form was seen (model effect, gate silent).
    hole: Option<GateHoleIssue>,
}

impl Walk {
    fn record_local(&mut self, local: &HirLocal) {
        let Some(init) = local.init.as_ref() else {
            self.gate_rejects = true;
            return;
        };
        if local.mutable || local.runtime_provided {
            self.gate_rejects = true;
            return;
        }
        if self.body.is_some() {
            // A binding after the value form is not a v1 binding chain.
            self.outside_subset = true;
        }
        if local.is_await {
            // The gate walks only the initializer expression; the await flag
            // on the binding itself is never inspected.  An awaited local
            // inside an otherwise-admissible body is a real gate hole.
            self.hole = Some(GateHoleIssue::AwaitingLocalInit);
        }
        match walk_expr(init) {
            // The initializer extends the immutable `let` chain; the chain
            // wraps around the value term in `finish`.
            ValueJudgment::Pure(term) => self.lets.push(term),
            ValueJudgment::OutsideSubset => self.outside_subset = true,
            ValueJudgment::GateRejects => self.gate_rejects = true,
            ValueJudgment::Hole(issue) => self.hole = Some(issue),
        }
    }

    fn record_value(&mut self, judgment: ValueJudgment) {
        match judgment {
            ValueJudgment::Pure(term) => {
                if self.body.is_some() {
                    self.outside_subset = true;
                } else {
                    self.body = Some(term);
                }
            }
            ValueJudgment::OutsideSubset => self.outside_subset = true,
            ValueJudgment::GateRejects => self.gate_rejects = true,
            ValueJudgment::Hole(issue) => self.hole = Some(issue),
        }
    }

    /// Wrap the `let` chain around the value term, right-nested.
    fn finish(self) -> GatePurityFinding {
        if self.gate_rejects {
            // The gate never admits this declaration: no hole, no obligation.
            return GatePurityFinding::NoObligation;
        }
        if let Some(issue) = self.hole {
            return GatePurityFinding::GateHole { issue };
        }
        if self.outside_subset {
            return GatePurityFinding::NoObligation;
        }
        let Some(body) = self.body else {
            return GatePurityFinding::NoObligation;
        };
        let term = self
            .lets
            .into_iter()
            .rev()
            .fold(body, |body, init| PurityTerm::Let(Box::new(init), Box::new(body)));
        GatePurityFinding::Admitted(term)
    }
}

fn walk_block(block: &crate::hir::HirBlock, walk: &mut Walk) {
    for statement in &block.statements {
        match &statement.kind {
            HirStatementKind::Local(local) => walk.record_local(local),
            HirStatementKind::Redde(Some(expr)) => walk.record_value(walk_expr(expr)),
            HirStatementKind::Redde(None)
            | HirStatementKind::ReddeFilum(_)
            | HirStatementKind::Rumpe
            | HirStatementKind::Perge
            | HirStatementKind::Tacet
            | HirStatementKind::IncDec(_)
            | HirStatementKind::Custodi(_) => walk.gate_rejects = true,
            HirStatementKind::Expr(_) => {
                // The gate admits only control expressions as statement
                // forms; every control form is outside the v1 subset and
                // anything else is a gate reject.  Both decline with no
                // obligation, so one bucket suffices.
                walk.outside_subset = true;
            }
        }
    }
    if let Some(expr) = &block.expr {
        walk.record_value(walk_expr(expr));
    }
}

fn walk_expr(expr: &HirExpression) -> ValueJudgment {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(value)) => match i64::try_from(*value) {
            Ok(value) => ValueJudgment::Pure(PurityTerm::Scalar(value)),
            Err(_) => ValueJudgment::OutsideSubset,
        },
        HirExpressionKind::Literal(HirLiteral::Float(_)) => {
            // Numerics honesty: no float leaf enters the v1 subset.
            ValueJudgment::OutsideSubset
        }
        HirExpressionKind::Path(_) => ValueJudgment::Pure(PurityTerm::Scalar(0)),
        HirExpressionKind::Call(callee, _, args, _) => {
            if args.iter().any(|arg| arg.name.is_some() || arg.spread) {
                return ValueJudgment::GateRejects;
            }
            let callee_term = match walk_expr(callee) {
                ValueJudgment::Pure(term) => term,
                other => return other,
            };
            let mut arg_terms = Vec::new();
            for arg in args {
                match walk_expr(&arg.expr) {
                    ValueJudgment::Pure(term) => arg_terms.push(term),
                    other => return other,
                }
            }
            ValueJudgment::Pure(PurityTerm::Call(Box::new(callee_term), arg_terms))
        }
        HirExpressionKind::Error => {
            // The purity gate accepts Error nodes silently; the model has no
            // pure translation for them.  Gate hole inside an otherwise
            // admissible body.
            ValueJudgment::Hole(GateHoleIssue::UntranslatableErrorNode)
        }
        // Every other form is either gate-admitted but outside the v1
        // scalar/let/call subset (binary/unary arithmetic, blocks, si,
        // discerne, arrays, structs, vacua, ...) or a gate reject
        // (assignment, IO, loops, throw, await, closures, ...).  Both
        // decline with no obligation, so a single bucket suffices here.
        _ => ValueJudgment::OutsideSubset,
    }
}

/// Shared declarations used by every generated Line-3 theorem.
///
/// `Term` deliberately includes the effect forms (`ioOp`, `monadicBind`)
/// that the gate must never admit: the correspondence theorem is therefore
/// non-vacuous — it excludes a nonempty class of terms.  Translation lands
/// in `Except Effect PureValue`: `ok` is the pure type fragment, `error` is an effect.
pub const GATE_PURITY_PRELUDE: &str = r"-- Machine-checked structural evidence for the AIR gate/purity
-- correspondence (Line 3, v1 scalar/let/call subset).  Structural only: no
-- semantic-preservation claim, no numeric claim, no execution claim.

/-- Effects the proof line excludes: IO and monadic suspension. -/
inductive Effect where
  | io : Effect
  | monadic : Effect

/-- Values of the pure type fragment. -/
inductive PureValue where
  | scalar : Int → PureValue
  | computed : PureValue

/-- Translated terms: the v1 pure forms plus the effect forms the gate must
never admit (calls translate their subterms only — a call target is a pure
function value; no interprocedural claim). -/
inductive Term where
  | scalar : Int → Term
  | letIn : Term → Term → Term
  | call : Term → Term → Term
  | ioOp : Term → Term
  | monadicBind : Term → Term → Term

def Term.translate : Term → Except Effect PureValue
  | .scalar n => .ok (.scalar n)
  | .letIn value body =>
      match value.translate with
      | .ok _ => body.translate
      | .error e => .error e
  | .call callee arg =>
      match callee.translate, arg.translate with
      | .ok _, .ok _ => .ok .computed
      | .error e, _ => .error e
      | _, .error e => .error e
  | .ioOp _ => .error .io
  | .monadicBind _ _ => .error .monadic

/-- Every IO/monadic effect form present in a term. -/
def Term.effects : Term → List Effect
  | .scalar _ => []
  | .letIn value body => value.effects ++ body.effects
  | .call callee arg => callee.effects ++ arg.effects
  | .ioOp inner => .io :: inner.effects
  | .monadicBind bound body => .monadic :: (bound.effects ++ body.effects)

/-- The gate's v1 admission policy, mirrored as an inductive predicate:
exactly the scalar/let/call shapes the purity gate admits inside the v1
subset.  Effect forms have no constructor and are therefore never admitted. -/
inductive GateAdmits : Term → Prop where
  | scalar (n : Int) : GateAdmits (.scalar n)
  | letIn : GateAdmits value → GateAdmits body → GateAdmits (.letIn value body)
  | call : GateAdmits callee → GateAdmits arg → GateAdmits (.call callee arg)

/-- Non-vacuity pins: the effect forms exist, are unadmissible, and
translate to effects — the correspondence below excludes a nonempty class. -/
theorem ioOpIsNeverAdmitted (inner : Term) : ¬ GateAdmits (Term.ioOp inner) := by
  intro h
  cases h

theorem monadicBindIsNeverAdmitted (bound body : Term) : ¬ GateAdmits (Term.monadicBind bound body) := by
  intro h
  cases h

theorem effectFormsTranslateToErrors :
    (Term.ioOp (Term.scalar 1)).translate = Except.error Effect.io ∧
    (Term.monadicBind (Term.scalar 1) (Term.scalar 2)).translate = Except.error Effect.monadic := by
  refine ⟨rfl, rfl⟩

/-- Inhabitation: every gate-admitted term translates to a value of the
pure type fragment. -/
theorem gateAdmittedInhabitsPureFragment (t : Term) (h : GateAdmits t) :
    ∃ v, t.translate = Except.ok v := by
  induction h with
  | scalar n => exact ⟨.scalar n, rfl⟩
  | letIn _ _ ihv ihb =>
      obtain ⟨v, hv'⟩ := ihv
      simp only [Term.translate, hv']
      exact ihb
  | call _ _ ihf iha =>
      obtain ⟨f, hf'⟩ := ihf
      obtain ⟨a, ha'⟩ := iha
      simp only [Term.translate, hf', ha']
      exact ⟨.computed, rfl⟩

/-- Effect exclusion: a gate-admitted term carries no IO and no monadic
effect anywhere in its structure. -/
theorem gateAdmittedCarriesNoEffects (t : Term) (h : GateAdmits t) :
    t.effects = [] := by
  induction h with
  | scalar _ => rfl
  | letIn _ _ ihv ihb => simp [Term.effects, ihv, ihb]
  | call _ _ ihf iha =>
      rw [Term.effects, ihf, iha]
      rfl

/-- Gate/purity correspondence (machine-checked-structural): every v1
gate-admitted term inhabits the pure type fragment and carries no IO or
monadic effect. -/
theorem gatePurityCorrespondence (t : Term) (h : GateAdmits t) :
    (∃ v, t.translate = Except.ok v) ∧ t.effects = [] :=
  ⟨gateAdmittedInhabitsPureFragment t h, gateAdmittedCarriesNoEffects t h⟩
";

/// Emit one real theorem for a recognized gate-purity obligation.
///
/// The theorem instantiates the master correspondence at the declaration's
/// structural term and supplies the gate-admission derivation directly.
#[must_use]
pub fn emit_gate_purity_theorem(name: &str, term: &PurityTerm) -> String {
    let (rendered, admission) = term.render();
    format!(
        "theorem {name} :\n\
    (∃ v, ({rendered}).translate = Except.ok v) ∧\n\
    ({rendered}).effects = [] :=\n\
  gatePurityCorrespondence ({rendered}) ({admission})\n"
    )
}

#[cfg(test)]
#[path = "air_purity_proof_test.rs"]
mod tests;
