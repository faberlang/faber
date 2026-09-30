//! Fail-closed gate for the unbounded integer `inf` on HIR-backed targets.
//!
//! `inf` (`numerus<inf>`, inf-delivery ruling 32) has no value bound and no
//! native carrier: every target needs a bignum (ruling 46). Until a target's
//! carrier lands, its emitter would read the width as "not an integer" or
//! print a bounded primitive for it, which is a wrong program, not a missing
//! one. This gate rejects any module whose type table names `inf` before a
//! HIR emitter runs, naming the target, so no backend can miscompile it
//! silently. A target that gains a carrier is added to [`target_admits_inf`].

use crate::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_target::Target;
use radix_types::{Primitive, Type, TypeId, TypeTable};

const ISSUE: &str = "inf_target_unsupported";

/// Targets whose emitters carry `inf` faithfully. The Faber re-emitter keeps
/// the authored spelling; no bignum-carrying target exists yet (units U6r,
/// U6t, U6g, U6p of the `inf` track add Rust, TypeScript, Go and Python).
fn target_admits_inf(target: Target) -> bool {
    matches!(target, Target::HirFaber | Target::HirFhir)
}

/// The error for a big integer literal (beyond `u64`, ruling 34) reaching a
/// backend that has no bignum carrier. [`reject_unbounded_integer`] refuses
/// such a module first (a big literal can only type as `inf`), so this arm is
/// the named fail-closed backstop, never a silent wrong emit.
#[must_use]
pub fn big_literal_unsupported(target: &str) -> CodegenError {
    CodegenError {
        message: format!(
            "an integer literal beyond u64 needs the unbounded integer `inf`, which has no bignum \
             carrier on the {target} target yet"
        ),
        args: vec![
            DiagnosticArg::new("issue", ISSUE),
            DiagnosticArg::new("type", "inf"),
            DiagnosticArg::new("target", target),
        ],
    }
}

/// # Errors
///
/// Returns a [`CodegenError`] (`inf_target_unsupported`) when the module's
/// type table names the unbounded integer and `target` has no carrier for it.
pub fn reject_unbounded_integer(types: &TypeTable, target: Target) -> Result<(), CodegenError> {
    if target_admits_inf(target) {
        return Ok(());
    }
    let names_inf = (0..types.type_count()).any(|index| {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        let ty = TypeId(index as u32);
        matches!(
            types.get(ty),
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_unbounded()
        )
    });
    if !names_inf {
        return Ok(());
    }
    Err(CodegenError {
        message: format!(
            "the unbounded integer `inf` is not supported on the {target:?} target: \
             it has no bignum carrier there yet"
        ),
        args: vec![
            DiagnosticArg::new("issue", ISSUE),
            DiagnosticArg::new("type", "inf"),
            DiagnosticArg::new("target", format!("{target:?}")),
        ],
    })
}
