//! Whether a recipe's result is read by anything other than the kernel's
//! `Return`.
//!
//! A backend may restructure a recipe so one thread writes a whole row (or any
//! region) of the output, instead of every thread writing its own element. That
//! is only sound when no same-kernel consumer reads the result by per-element
//! index, because such a read could observe an element its owner has not
//! written yet. This is the target-neutral proof of that condition; the
//! backends decide separately whether the restructuring pays.

use crate::visit::{MirVisitor, walk_operand, walk_stmt, walk_terminator};
use crate::{
    MirFunction, MirOperand, MirPlace, MirPlaceBase, MirStatementKind, MirTerminatorKind,
    MirValueKind,
};

/// Whether a recipe's result is consumed only by the kernel's `Return`.
///
/// A mapping that writes a whole row from one thread means a same-kernel
/// consumer that reads `output[index]` (the per-element binding) could read an
/// element its owner has not written yet. Following plain forwards
/// (`temp → local → return`), the result may be read by nothing else.
#[must_use]
pub fn result_only_returned(function: &MirFunction, destination: &MirPlace) -> bool {
    if !destination.projections.is_empty() {
        return false;
    }
    let mut base = destination.base.clone();
    // A forward chain is short (`temp → local → return`); a longer one is
    // not the shape this is meant to prove.
    for _ in 0..4 {
        let mut forwards = Vec::new();
        let mut returned = false;
        let mut other_read = false;
        for block in &function.blocks {
            for statement in &block.statements {
                if let MirStatementKind::Assign { place, value } = &statement.kind
                    && place.projections.is_empty()
                    && let MirValueKind::Operand(operand) = &value.kind
                    && operand_is_plain(operand, &base)
                {
                    forwards.push(place.base.clone());
                    continue;
                }
                let mut reads = BaseReads {
                    base: &base,
                    found: false,
                };
                walk_stmt(&mut reads, statement);
                other_read |= reads.found;
            }
            match &block.terminator.kind {
                MirTerminatorKind::Return(Some(operand)) if operand_is_plain(operand, &base) => {
                    returned = true;
                }
                _ => {
                    let mut reads = BaseReads {
                        base: &base,
                        found: false,
                    };
                    walk_terminator(&mut reads, &block.terminator);
                    other_read |= reads.found;
                }
            }
        }
        if other_read {
            return false;
        }
        match (forwards.as_slice(), returned) {
            ([], _) => return true,
            ([next], false) => base = next.clone(),
            _ => return false,
        }
    }
    false
}

/// Whether `operand` reads exactly `base`, with no projection.
fn operand_is_plain(operand: &MirOperand, base: &MirPlaceBase) -> bool {
    match operand {
        MirOperand::Temp(temp) => matches!(base, MirPlaceBase::Temp(id) if id == temp),
        MirOperand::Place(place) => place.projections.is_empty() && place.base == *base,
        MirOperand::Value(_) | MirOperand::Constant(_) => false,
    }
}

/// Finds any read of one place base.
struct BaseReads<'a> {
    base: &'a MirPlaceBase,
    found: bool,
}

impl MirVisitor for BaseReads<'_> {
    fn visit_operand(&mut self, operand: &MirOperand) {
        let reads = match operand {
            MirOperand::Temp(temp) => matches!(self.base, MirPlaceBase::Temp(id) if id == temp),
            MirOperand::Place(place) => place.base == *self.base,
            MirOperand::Value(_) | MirOperand::Constant(_) => false,
        };
        self.found |= reads;
        walk_operand(self, operand);
    }
}

#[cfg(test)]
#[path = "result_use_test.rs"]
mod tests;
