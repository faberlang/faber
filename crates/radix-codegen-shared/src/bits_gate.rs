//! Fail-closed gate for the raw-bits types `b8 b16 b32 b64`.
//!
//! A `bN` is exactly N bits: no arithmetic, no width above its own, bit
//! operations at the type's width (operator ruling 2026-10-10, need).
//! Only the Faber source emitter (which prints the marker) and the MIR runner
//! (which executes it) implement the type. Every other emitter would read the
//! width as "not an integer" or print a plain integer carrier, which is a wrong
//! program, not a missing one. This gate rejects any module whose type table
//! names a `bN` before an emitter runs, naming the type and the target, so no
//! backend can miscompile it silently. A target that gains a `bN` carrier is
//! added to [`target_admits_bits`].

use crate::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_target::Target;
use radix_types::{Type, TypeId, TypeTable};

const ISSUE: &str = "bits_type_unsupported_on_target";

/// Targets that carry `bN` faithfully. The Faber re-emitter keeps the authored
/// marker; the MIR runner executes the type and does not reach codegen. The
/// Phase 3 emitters carry it as an unsigned carrier: Rust and Go as u8..u64,
/// Swift as UInt8..UInt64, TypeScript as `number` (b8, b16, b32) and `bigint`
/// (b64), and the MIR sexp target as a non-negative exact Racket integer kept
/// inside its N bits.
fn target_admits_bits(target: Target) -> bool {
    matches!(
        target,
        Target::HirFaber
            | Target::MirStepper
            | Target::HirRust
            | Target::HirTypeScript
            | Target::HirGo
            | Target::HirSwift
            | Target::MirSexp
            | Target::MirWasm
    )
}

/// The first raw-bits marker (`b8`) the type table names, if any.
fn first_bits_marker(types: &TypeTable) -> Option<&'static str> {
    (0..types.type_count()).find_map(|index| {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        let ty = TypeId(index as u32);
        match types.get(ty) {
            Type::Bits(width) => Some(width.faber_marker()),
            _ => None,
        }
    })
}

/// # Errors
///
/// Returns a [`CodegenError`] (`bits_type_unsupported_on_target`) when the
/// module's type table names a raw-bits type and `target` has no carrier for it.
pub fn reject_bits_type(types: &TypeTable, target: Target) -> Result<(), CodegenError> {
    if target_admits_bits(target) {
        return Ok(());
    }
    let Some(marker) = first_bits_marker(types) else {
        return Ok(());
    };
    Err(CodegenError {
        message: format!(
            "the raw-bits type `{marker}` is not supported on the {target:?} target: \
             only the Faber source emitter and the MIR runner implement it"
        ),
        args: vec![
            DiagnosticArg::new("issue", ISSUE),
            DiagnosticArg::new("type", marker),
            DiagnosticArg::new("target", format!("{target:?}")),
        ],
    })
}
