//! Lean proof source and recognizer for AIR additive mask neutrality.
//!
//! The proof is deliberately structural.  It treats the caller-supplied dead
//! lane count as data, folds `Nat` values with an explicitly supplied neutral
//! element, and proves only the right-neutral suffix law used by the
//! zero-fill route.  No floating-point type, arithmetic, identity inference,
//! or execution target belongs here.

use crate::hir::visit::HirVisitor;
use crate::hir::{HirBinOp, HirExpression, HirExpressionKind, HirLiteral};

/// Reduction family that consumes the additive mask-neutrality lemma.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskNeutralityFamily {
    /// Fold a caller-built zero-filled suffix to one value.
    Reduce,
    /// Prefix-combine family whose mask proof uses the same fold projection.
    Scan,
}

impl MaskNeutralityFamily {
    /// Stable proof-source label for this family.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Reduce => "reduce",
            Self::Scan => "scan",
        }
    }
}

/// Return the proof family for an AIR higher-order reduction method.
///
/// The semantic AIR gate owns eligibility.  This helper only recognizes the
/// two already-admitted additive-combine consumers named by ARC: `reducta`
/// and `cumulata`.
#[must_use]
pub fn reduction_family(method: &str) -> Option<MaskNeutralityFamily> {
    match method {
        "reducta" => Some(MaskNeutralityFamily::Reduce),
        "cumulata" => Some(MaskNeutralityFamily::Scan),
        _ => None,
    }
}

/// Recognize the narrow additive-combine shape used by the proof line.
///
/// The recognizer accepts an inline closure whose body is an addition.  Named
/// paths are resolved by the report builder against the complete HIR module;
/// this low-level helper does not admit an uninspected path.  Float literals
/// are rejected even when nested inside the closure body.
#[must_use]
pub fn is_additive_combine(expr: &HirExpression) -> bool {
    match &expr.kind {
        HirExpressionKind::Clausura(_, _, _, body) => is_additive_body(body),
        _ => false,
    }
}

/// The proof line admits only the explicit integer zero seed.  This keeps a
/// neutral element from being inferred and makes float reductions impossible
/// to admit accidentally.
#[must_use]
pub fn is_integer_zero_seed(expr: &HirExpression) -> bool {
    matches!(expr.kind, HirExpressionKind::Literal(HirLiteral::Int(0)))
}

pub(crate) fn is_additive_body(expr: &HirExpression) -> bool {
    matches!(expr.kind, HirExpressionKind::Binary(HirBinOp::Add, _, _)) && !contains_float(expr)
}

fn contains_float(expr: &HirExpression) -> bool {
    struct FloatFinder(bool);

    impl crate::hir::visit::HirVisitor for FloatFinder {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if matches!(expr.kind, HirExpressionKind::Literal(HirLiteral::Float(_))) {
                self.0 = true;
            }
            crate::hir::visit::walk_expr(self, expr);
        }
    }

    let mut finder = FloatFinder(false);
    finder.visit_expr(expr);
    finder.0
}

/// Shared declarations used by every generated Line-2 theorem.
///
/// `dead` is caller-provided.  The generated proof never constructs a mask in
/// the AIR region; `zeroFilledSuffix` models only the already-materialized
/// boundary value supplied to the reduction.
pub const MASK_NEUTRALITY_PRELUDE: &str = r"-- Machine-checked structural evidence for the AIR maskless zero-fill route.
-- The caller supplies the dead-lane count; this module performs no mask construction.
def foldWith (combine : Nat → Nat → Nat) (seed : Nat) (values : List Nat) : Nat :=
  values.foldl combine seed

def zeroFilledSuffix (live : List Nat) (dead : Nat) : List Nat :=
  live ++ List.replicate dead 0

theorem foldNeutralSuffix
    (combine : Nat → Nat → Nat)
    (neutral : Nat)
    (hneutral : ∀ acc : Nat, combine acc neutral = acc)
    (live : List Nat)
    (dead : Nat) :
    foldWith combine neutral (zeroFilledSuffix live dead) =
      foldWith combine neutral live := by
  have replicateNeutral :
      ∀ (acc : Nat) (count : Nat),
        (List.replicate count neutral).foldl combine acc = acc := by
    intro acc count
    induction count generalizing acc with
    | zero => rfl
    | succ count ih =>
        simp [List.replicate_succ, ih, hneutral]
  simp [foldWith, zeroFilledSuffix, List.foldl_append, replicateNeutral]

theorem additiveReduceMaskNeutrality (live : List Nat) (dead : Nat) :
    foldWith (fun acc value => acc + value) 0 (zeroFilledSuffix live dead) =
      foldWith (fun acc value => acc + value) 0 live := by
  exact foldNeutralSuffix (fun acc value => acc + value) 0 (by intro acc; simp) live dead

-- The scan route consumes the same fold projection.  This theorem does not
-- claim a scan implementation or execution semantics; it records the
-- structural lemma that its additive combine must satisfy.
theorem additiveScanMaskNeutrality (live : List Nat) (dead : Nat) :
    foldWith (fun acc value => acc + value) 0 (zeroFilledSuffix live dead) =
      foldWith (fun acc value => acc + value) 0 live := by
  exact additiveReduceMaskNeutrality live dead
";

/// Emit one real theorem for a recognized mask-neutrality obligation.
#[must_use]
pub fn emit_mask_neutrality_theorem(name: &str, family: MaskNeutralityFamily) -> String {
    let theorem = match family {
        MaskNeutralityFamily::Reduce => "additiveReduceMaskNeutrality",
        MaskNeutralityFamily::Scan => "additiveScanMaskNeutrality",
    };
    format!(
        "theorem {name} (live : List Nat) (dead : Nat) :\n\
    foldWith (fun acc value => acc + value) 0 (zeroFilledSuffix live dead) =\n\
      foldWith (fun acc value => acc + value) 0 live := by\n\
  exact {theorem} live dead\n"
    )
}

#[cfg(test)]
#[path = "air_mask_proof_test.rs"]
mod tests;
