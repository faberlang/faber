//! Post-composition nucleum-fragment survivor invariant.
//!
//! Validated MIR must not contain a fragment call or a standalone fragment
//! symbol. Emitters consume the same scan at `MirDeviceRole` sites so a
//! bypassed composition pass fails closed (internal-error class) instead of
//! emitting a helper symbol. Ordinary kernels with no `Fragment` role are
//! untouched.

use crate::validate::MirValidationError;
use crate::{
    MirCallee, MirDeviceContext, MirDeviceRole, MirFunction, MirFunctionId, MirProgram,
    MirStatementKind, MirTerminatorKind,
};
use radix_lexer::Span;

/// Stable diagnostic identifier for a fragment that survived composition.
pub const NUCLEUM_FRAGMENT_SURVIVED: &str = "NUCLEUM_FRAGMENT_SURVIVED";

/// How a nucleum fragment leaked past composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentSurvivorKind {
    Call,
    Symbol,
}

/// One fragment call or leftover fragment function in a MIR program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentSurvivor {
    pub kind: FragmentSurvivorKind,
    pub span: Span,
    pub function: MirFunctionId,
}

impl FragmentSurvivor {
    #[must_use]
    pub fn describe(&self) -> String {
        match self.kind {
            FragmentSurvivorKind::Call => {
                format!("fragment call in f{}", self.function.0)
            }
            FragmentSurvivorKind::Symbol => {
                format!("standalone fragment symbol f{}", self.function.0)
            }
        }
    }
}

/// Fragment calls and leftover fragment functions still present in `program`.
///
/// Empty `device` roles yield an empty result — ordinary, fragment-free
/// kernels must not false-positive.
#[must_use]
pub fn surviving_nucleum_fragments(
    program: &MirProgram,
    device: &MirDeviceContext,
) -> Vec<FragmentSurvivor> {
    if !device.has_roles() {
        return Vec::new();
    }
    let mut survivors = Vec::new();
    for function in &program.functions {
        if device.role_for_function(function) == Some(MirDeviceRole::Fragment) {
            survivors.push(FragmentSurvivor {
                kind: FragmentSurvivorKind::Symbol,
                span: function.span,
                function: function.id,
            });
        }
        collect_fragment_calls(function, program, device, &mut survivors);
    }
    survivors
}

/// Validation errors for every surviving fragment call or symbol.
#[must_use]
pub fn reject_nucleum_fragment_survivors(
    program: &MirProgram,
    device: &MirDeviceContext,
) -> Vec<MirValidationError> {
    surviving_nucleum_fragments(program, device)
        .into_iter()
        .map(|survivor| {
            let message = match survivor.kind {
                FragmentSurvivorKind::Call => "nucleum fragment call survived composition",
                FragmentSurvivorKind::Symbol => "nucleum fragment symbol survived composition",
            };
            MirValidationError::with_issue(survivor.span, NUCLEUM_FRAGMENT_SURVIVED, message)
        })
        .collect()
}

/// First leftover fragment *call*, if any. Emitters omit leftover symbols
/// rather than fail — the call is the composition-bypass proof.
#[must_use]
pub fn first_surviving_fragment_call(
    program: &MirProgram,
    device: &MirDeviceContext,
) -> Option<FragmentSurvivor> {
    surviving_nucleum_fragments(program, device)
        .into_iter()
        .find(|survivor| survivor.kind == FragmentSurvivorKind::Call)
}

fn collect_fragment_calls(
    function: &MirFunction,
    program: &MirProgram,
    device: &MirDeviceContext,
    survivors: &mut Vec<FragmentSurvivor>,
) {
    for block in &function.blocks {
        for statement in &block.statements {
            if let MirStatementKind::Call { callee, .. } = &statement.kind {
                if is_fragment_callee(callee, program, device) {
                    survivors.push(FragmentSurvivor {
                        kind: FragmentSurvivorKind::Call,
                        span: statement.span,
                        function: function.id,
                    });
                }
            }
        }
        if let MirTerminatorKind::TryCall { callee, .. } = &block.terminator.kind {
            if is_fragment_callee(callee, program, device) {
                survivors.push(FragmentSurvivor {
                    kind: FragmentSurvivorKind::Call,
                    span: block.terminator.span,
                    function: function.id,
                });
            }
        }
    }
}

fn is_fragment_callee(callee: &MirCallee, program: &MirProgram, device: &MirDeviceContext) -> bool {
    match callee {
        MirCallee::Function(id) => program
            .functions
            .iter()
            .find(|function| function.id == *id)
            .is_some_and(|function| {
                device.role_for_function(function) == Some(MirDeviceRole::Fragment)
            }),
        MirCallee::Definition { source, .. } => {
            if device.role_for_def(*source) == Some(MirDeviceRole::Fragment) {
                return true;
            }
            program.functions.iter().any(|function| {
                function.source == Some(*source)
                    && device.role_for_function(function) == Some(MirDeviceRole::Fragment)
            })
        }
        MirCallee::Closure(_) | MirCallee::Value(_) => false,
    }
}

#[cfg(test)]
#[path = "fragments_test.rs"]
mod tests;
