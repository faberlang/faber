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
    MirFunction, MirOperand, MirPlace, MirPlaceBase, MirStatement, MirStatementKind, MirTerminator,
    MirTerminatorKind, MirValueKind,
};

/// Whether nothing but plain forwards and the kernel's `Return` reads a
/// recipe's result.
///
/// A mapping that writes a whole row from one thread means a same-kernel
/// consumer that reads `output[index]` (the per-element binding) could read an
/// element its owner has not written yet. Following plain forwards
/// (`temp → local → return`), the result may be read by nothing else. The
/// result need not be returned at all: a recipe that writes the kernel's output
/// parameter in a `void` kernel has no `Return` of it.
///
/// Every place on the chain must also be written at most once; see
/// [`result_returned_as`].
#[must_use]
pub fn result_only_returned(function: &MirFunction, destination: &MirPlace) -> bool {
    follow_result(function, destination).is_some()
}

/// The place the kernel finally returns a recipe's result as, when nothing
/// else reads it: the destination itself for `return x.softmax()`, the local
/// at the end of a plain forward chain for `let y ← x.softmax(); return y`.
///
/// A backend that lets one thread write a whole row of the output must also
/// stop the return from re-storing the result element by element, which would
/// race the thread that owns the row. A backend whose return recognizes the
/// recipe's own destination can record the place returned here instead, so the
/// forwarded forms stay on the fast path and stay correct.
///
/// `None` unless every step is provably the same value:
///
/// - the result is read by nothing but plain forwards and the return;
/// - every place along the chain is written **at most once**, so a forward
///   target reassigned before the return (`mut y ← x.softmax(); y ← other;
///   return y`) is refused: it would return `other` while the caller believed
///   it was returning the recipe's result;
/// - the chain ends in a `Return` of the last place (unlike
///   [`result_only_returned`], a result nobody returns is not enough).
#[must_use]
pub fn result_returned_as(function: &MirFunction, destination: &MirPlace) -> Option<MirPlace> {
    let (base, returned) = follow_result(function, destination)?;
    returned.then(|| MirPlace {
        base,
        projections: Vec::new(),
    })
}

/// Whether a kernel that returns nothing leaves a recipe's result unread: the
/// `output ⇇ x.recipe()` shape, where the recipe writes a temp, a plain forward
/// copies it into the output parameter, and the kernel ends in `Return(None)`.
///
/// Unlike [`result_returned_as`] there is no return to teach to skip a
/// re-store, so no returned place is needed; the requirement is that nothing
/// reads the result (same-kernel consumers would see elements their owner has
/// not written yet) and that the kernel returns nothing. A kernel that
/// returns a *different* value does not qualify: its return writes
/// `output[index]` from every thread, which would race a row's owner.
#[must_use]
pub fn result_unread_in_a_void_kernel(function: &MirFunction, destination: &MirPlace) -> bool {
    let returns_a_value = function
        .blocks
        .iter()
        .any(|block| matches!(block.terminator.kind, MirTerminatorKind::Return(Some(_))));
    !returns_a_value && result_only_returned(function, destination)
}

/// Whether `place` is a plain place written at most once in the function: its
/// single write is the one a forward gives it, so it still holds that value
/// wherever it is read.
///
/// A backend that moves a "this result was already written to the output" mark
/// along a forward (`s ← temp`) needs this much of [`result_returned_as`]'s proof
/// when the chain ends somewhere other than a return (a write into the output
/// parameter): a target written again (`s ← s / n`, `s ← 1.0`) holds another
/// value, and marking it would skip the store of that value.
#[must_use]
pub fn written_at_most_once(function: &MirFunction, place: &MirPlace) -> bool {
    place.projections.is_empty() && write_count(function, &place.base) <= 1
}

/// Follow a recipe result along plain forwards. `Some((end, returned))` when no
/// step is read by anything else and every place is written at most once;
/// `returned` says whether the chain ends in a `Return` of `end`.
fn follow_result(function: &MirFunction, destination: &MirPlace) -> Option<(MirPlaceBase, bool)> {
    if !destination.projections.is_empty() {
        return None;
    }
    let mut base = destination.base.clone();
    // A forward chain is short (`temp → local → return`); a longer one is
    // not the shape this is meant to prove.
    for _ in 0..4 {
        if write_count(function, &base) > 1 {
            return None;
        }
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
                other_read |= stmt_reads(&base, statement);
            }
            match &block.terminator.kind {
                MirTerminatorKind::Return(Some(operand)) if operand_is_plain(operand, &base) => {
                    returned = true;
                }
                _ => other_read |= terminator_reads(&base, &block.terminator),
            }
        }
        if other_read {
            return None;
        }
        match (forwards.as_slice(), returned) {
            ([], returned) => return Some((base, returned)),
            ([next], false) => base = next.clone(),
            _ => return None,
        }
    }
    None
}

/// Whether any read inside `statement` touches `base`.
fn stmt_reads(base: &MirPlaceBase, statement: &MirStatement) -> bool {
    let mut reads = BaseReads { base, found: false };
    walk_stmt(&mut reads, statement);
    reads.found
}

/// Whether any read inside `terminator` touches `base`.
fn terminator_reads(base: &MirPlaceBase, terminator: &MirTerminator) -> bool {
    let mut reads = BaseReads { base, found: false };
    walk_terminator(&mut reads, terminator);
    reads.found
}

/// How many places in the function write `base`, counting any write through a
/// projection (`base[i] ← …`) as disqualifying.
fn write_count(function: &MirFunction, base: &MirPlaceBase) -> usize {
    let mut count = 0;
    let mut note = |place: &MirPlace| {
        if place.base == *base {
            count += if place.projections.is_empty() {
                1
            } else {
                usize::MAX / 2
            };
        }
    };
    for block in &function.blocks {
        for statement in &block.statements {
            match &statement.kind {
                MirStatementKind::Assign { place, .. } => note(place),
                MirStatementKind::Call { destination, .. }
                | MirStatementKind::RuntimeCall { destination, .. } => {
                    if let Some(place) = destination {
                        note(place);
                    }
                }
                MirStatementKind::Construct { destination, .. } => note(destination),
            }
        }
        if let MirTerminatorKind::TryCall {
            destination,
            error_place,
            ..
        } = &block.terminator.kind
        {
            if let Some(place) = destination {
                note(place);
            }
            note(error_place);
        }
    }
    count
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
