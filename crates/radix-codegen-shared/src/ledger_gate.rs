//! Fail-closed gate for the ledger type `ledger<K, V>` (la `tabulae<K, V>`).
//!
//! A ledger is a built-in insertion-ordered map (operator ruling 2026-10-10,
//! need). Its carrier lands per target with the ledger operations; until
//! then no emitter implements the type, and reading it as a plain map would
//! silently drop the order. This gate rejects any module whose type table names
//! a ledger before an emitter runs, naming the type and the target. A target
//! that gains a ledger carrier is added to [`target_admits_ledger`].

use crate::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_target::Target;
use radix_types::{Type, TypeTable};

const ISSUE: &str = "ledger_type_unsupported_on_target";

/// Targets that carry a ledger faithfully. The Faber re-emitter keeps the
/// authored spelling; the MIR runner does not reach codegen.
fn target_admits_ledger(target: Target) -> bool {
    matches!(target, Target::HirFaber | Target::MirStepper)
}

/// Whether the type table names a ledger anywhere.
fn names_ledger(types: &TypeTable) -> bool {
    (0..types.type_count()).any(|index| {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        let ty = radix_types::TypeId(index as u32);
        matches!(types.get(ty), Type::Ledger(_, _))
    })
}

/// # Errors
///
/// Returns a [`CodegenError`] (`ledger_type_unsupported_on_target`) when the
/// module's type table names a ledger type and `target` has no carrier for it.
pub fn reject_ledger_type(types: &TypeTable, target: Target) -> Result<(), CodegenError> {
    if target_admits_ledger(target) || !names_ledger(types) {
        return Ok(());
    }
    Err(CodegenError {
        message: format!(
            "the ledger type `ledger<K, V>` is not supported on the {target:?} target yet: \
             only the Faber source emitter and the MIR runner accept it"
        ),
        args: vec![
            DiagnosticArg::new("issue", ISSUE),
            DiagnosticArg::new("type", "ledger"),
            DiagnosticArg::new("target", format!("{target:?}")),
        ],
    })
}
