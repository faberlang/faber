//! Fail-closed gates for ranged store policies, `saturating|trapping|wrapping<W, lo‥hi>`,
//! and list length ranges, `list<T, lo‥hi>`.
//!
//! A range is a refinement a store applies inside the explicit width (need
//! d5d00916). It is built on the MIR runner and the Rust target; every other
//! emitter reads the cell as its bare width, which would silently drop the
//! range and change what a store does. This gate rejects any module whose type
//! table names a ranged integer before an emitter runs, naming the type and the
//! target. A target that gains the store lowering is added to
//! [`Target::lowers_ranged_int`].

use crate::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_target::Target;
use radix_types::{Type, TypeTable};

const ISSUE: &str = "ranged_policy_target_unsupported";

/// The first ranged integer the type table names, as `(policy word, width marker)`.
fn first_ranged_int(types: &TypeTable) -> Option<(&'static str, &'static str)> {
    (0..types.type_count()).find_map(|index| {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        let ty = radix_types::TypeId(index as u32);
        match types.get(ty) {
            Type::RangedInt { policy, width, .. } => Some((policy.word(), width.faber_marker())),
            _ => None,
        }
    })
}

/// # Errors
///
/// Returns a [`CodegenError`] (`ranged_policy_target_unsupported`) when the
/// module's type table names a ranged integer and `target` has no store
/// lowering for it.
pub fn reject_ranged_int_type(types: &TypeTable, target: Target) -> Result<(), CodegenError> {
    if target.lowers_ranged_int() {
        return Ok(());
    }
    let Some((policy, width)) = first_ranged_int(types) else {
        return Ok(());
    };
    Err(CodegenError {
        message: format!(
            "the ranged store policy `{policy}<{width}, lo‥hi>` is not supported on the \
             {target:?} target yet: only the Faber source emitter, the MIR runner and the \
             Rust target accept it"
        ),
        args: vec![
            DiagnosticArg::new("issue", ISSUE),
            DiagnosticArg::new("type", format!("{policy}<{width}, lo‥hi>")),
            DiagnosticArg::new("target", format!("{target:?}")),
        ],
    })
}

const LIST_ISSUE: &str = "ranged_list_target_unsupported";

/// Whether the type table names a list length range.
fn names_ranged_list(types: &TypeTable) -> bool {
    (0..types.type_count()).any(|index| {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        let ty = radix_types::TypeId(index as u32);
        matches!(types.get(ty), Type::RangedArray { .. })
    })
}

/// A list length range is applied by the Faber source emitter, the MIR runner
/// and the Rust target; every other emitter would read the
/// cell as a plain list and silently drop the range.
///
/// # Errors
///
/// Returns a [`CodegenError`] (`ranged_list_target_unsupported`) when the
/// module's type table names a list length range and `target` has no lowering
/// for it.
pub fn reject_ranged_list_type(types: &TypeTable, target: Target) -> Result<(), CodegenError> {
    if target.lowers_ranged_list() || !names_ranged_list(types) {
        return Ok(());
    }
    Err(CodegenError {
        message: format!(
            "the list length range `list<T, lo‥hi>` is not supported on the {target:?} target \
             yet: only the Faber source emitter, the MIR runner and the Rust target accept it"
        ),
        args: vec![
            DiagnosticArg::new("issue", LIST_ISSUE),
            DiagnosticArg::new("type", "list<T, lo‥hi>"),
            DiagnosticArg::new("target", format!("{target:?}")),
        ],
    })
}

/// The value-type gates a CLI product path runs together before any emitter:
/// raw bits, ledgers and ranged store policies have no carrier on those
/// targets, so each fails closed by name instead of reaching the emitter's
/// anonymous unsupported-type error.
///
/// # Errors
///
/// Returns the first [`CodegenError`] of [`crate::reject_bits_type`],
/// [`crate::reject_ledger_type`], [`reject_ranged_int_type`] and
/// [`reject_ranged_list_type`].
pub fn reject_unlowered_value_types(types: &TypeTable, target: Target) -> Result<(), CodegenError> {
    crate::reject_bits_type(types, target)?;
    crate::reject_ledger_type(types, target)?;
    reject_ranged_int_type(types, target)?;
    reject_ranged_list_type(types, target)
}
