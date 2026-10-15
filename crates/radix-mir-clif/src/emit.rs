//! `emit_object`: validated MIR to a native object file.

use radix_lexer::Interner;
use radix_mir::names::MirNames;
use radix_mir::{MirProgram, ValidatedMir};
use radix_types::TypeTable;
use radix_types::vocabulary::{CapabilityGap, Lowerability};

use crate::capability::{SLUG_CLIF_SUBSET, classify_clif_reachable};
use crate::lower::{ProgramInput, lower_program};

/// A relocatable native object for the host triple.
#[derive(Debug, Clone)]
pub struct ClifObject {
    /// Object file bytes (Mach-O or ELF, per host).
    pub bytes: Vec<u8>,
}

/// A decline or failure of the CLIF backend. `category` is the first gap's
/// slug, `shape` its shape text; `detail` joins every gap in storage order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClifError {
    pub message: String,
    pub category: String,
    pub shape: String,
    pub detail: String,
}

impl ClifError {
    fn from_gaps(gaps: &[CapabilityGap]) -> Self {
        let first = gaps.first().expect("rejected carries a gap");
        let mut shapes: Vec<&str> = Vec::new();
        for gap in gaps {
            if !shapes.contains(&gap.shape()) {
                shapes.push(gap.shape());
            }
        }
        let detail = shapes.join(", ");
        Self {
            message: format!("clif target declines: {}; gaps: {detail}", first.shape()),
            category: first.slug().to_owned(),
            shape: first.shape().to_owned(),
            detail,
        }
    }

    fn late_decline(shape: &str) -> Self {
        Self {
            message: format!("clif target declines: {shape}; gaps: {shape}"),
            category: SLUG_CLIF_SUBSET.to_owned(),
            shape: shape.to_owned(),
            detail: shape.to_owned(),
        }
    }
}

/// Emit a native object for `validated`.
///
/// # Errors
///
/// Returns [`ClifError`] naming the first gap when the program is outside the
/// CLIF subset, or the failing shape when the lowering declines late.
pub fn emit_object(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
) -> Result<ClifObject, ClifError> {
    let program = validated.program();
    let types = validated.validation().types.as_table();
    let entry = MirNames::new(program, types, interner)
        .explicit_entry_function()
        .map(|function| function.id);
    if let Lowerability::Rejected(gaps) = classify_clif_reachable(validated, entry) {
        return Err(ClifError::from_gaps(&gaps));
    }
    let tokens = match entry {
        Some(id) => display_tokens(validated, id),
        None => ["true", "false", "none", "tuple"].map(str::to_owned),
    };
    emit_program(program, types, interner, entry, tokens)
}

fn display_tokens(validated: &ValidatedMir<'_>, entry: radix_mir::MirFunctionId) -> [String; 4] {
    let table = &validated.validation().display_tokens;
    match table.for_function(entry) {
        Some(tokens) => [
            tokens.true_.clone(),
            tokens.false_.clone(),
            tokens.none.clone(),
            tokens.tuple.clone(),
        ],
        None => ["true", "false", "none", "tuple"].map(str::to_owned),
    }
}

/// Lower a program that already passed the capability pre-pass. Exposed for
/// fixture tests that have no validation context.
///
/// # Errors
///
/// Returns [`ClifError`] naming the shape the lowering declined.
pub fn emit_program(
    program: &MirProgram,
    types: &TypeTable,
    interner: &Interner,
    entry: Option<radix_mir::MirFunctionId>,
    tokens: [String; 4],
) -> Result<ClifObject, ClifError> {
    let input = ProgramInput {
        program,
        types,
        interner,
        entry,
        tokens,
    };
    lower_program(&input)
        .map(|bytes| ClifObject { bytes })
        .map_err(|decline| ClifError::late_decline(&decline.shape))
}
