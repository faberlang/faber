//! Sequential-counter bounds of a kernel body (the device side of the
//! checker's `apud` proof).
//!
//! A sequential tensor iteration (`for from t at [i] let v`, no `thread lane`)
//! lowers to a counter loop: `c = 0`, a header `if c < e { body; c = c + 1 }`
//! with `e` the iterated tensor's extent. The checker proves `t[i]` in range
//! there, so the device must not guard it. A local `c` is such a COUNTER when
//! every write of it is a non-negative integer literal or `c + 1`, so it never
//! goes below zero. A must-dataflow over the CFG then records, at each point,
//! the bounds `b` for which `c < b` is known: the true edge of a branch on a
//! `c < b` compare (computed in the same block, `b` a loop-invariant symbolic
//! value) adds the bound, and any write of `c` drops its bounds. A counter is
//! BOUNDED by `b` when `c < b` holds at every read of `c` except the header
//! compare and the increment itself, so every value read from it (directly or
//! through a single-definition copy) lies in `[0, b)`.

use std::collections::{BTreeMap, BTreeSet};

use crate::visit::MirVisitor;
use crate::{
    MirBinOp, MirConstant, MirFunction, MirOperand, MirPlace, MirPlaceBase, MirStatement,
    MirStatementKind, MirTerminatorKind, MirValueKind,
};

use super::{BaseKey, LaneIndexProof, Sym, operand_key};

/// The known `(counter, bound)` facts at a point; `None` is "not reached yet"
/// (the top of the must lattice).
type Facts = Option<BTreeSet<(BaseKey, Sym)>>;

/// A statement position `(block index, statement index)`.
type Position = (usize, usize);

/// Every key a statement reads (operands, place bases and index operands; the
/// projection-free destination of an assignment is not a read).
#[derive(Default)]
struct Reads(BTreeSet<BaseKey>);

impl MirVisitor for Reads {
    fn visit_stmt(&mut self, stmt: &MirStatement) {
        if let MirStatementKind::Assign { place, value } = &stmt.kind {
            if !place.projections.is_empty() {
                self.visit_place(place);
            }
            self.visit_value(value);
        } else {
            crate::visit::walk_stmt(self, stmt);
        }
    }

    fn visit_operand(&mut self, operand: &MirOperand) {
        if let Some(key) = operand_key(operand) {
            self.0.insert(key);
        }
        crate::visit::walk_operand(self, operand);
    }

    fn visit_place(&mut self, place: &MirPlace) {
        self.0.insert(BaseKey::of(&place.base));
        crate::visit::walk_place(self, place);
    }
}

fn statement_reads(statement: &MirStatement) -> BTreeSet<BaseKey> {
    let mut reads = Reads::default();
    reads.visit_stmt(statement);
    reads.0
}

/// The key a statement writes, if any.
fn statement_write(statement: &MirStatement) -> Option<BaseKey> {
    match &statement.kind {
        MirStatementKind::Assign { place, .. } => Some(BaseKey::of(&place.base)),
        MirStatementKind::Call { destination, .. }
        | MirStatementKind::RuntimeCall { destination, .. } => {
            destination.as_ref().map(|place| BaseKey::of(&place.base))
        }
        MirStatementKind::Construct { destination, .. } => Some(BaseKey::of(&destination.base)),
    }
}

/// Whether `operand` names `counter` with no projection.
fn names(operand: &MirOperand, counter: BaseKey) -> bool {
    operand_key(operand) == Some(counter)
}

/// Whether `kind` is `counter + 1` (either operand order).
fn is_step(kind: &MirValueKind, counter: BaseKey) -> bool {
    let one = |operand: &MirOperand| matches!(operand, MirOperand::Constant(MirConstant::Int(1)));
    matches!(kind, MirValueKind::Binary { op: MirBinOp::Add, lhs, rhs }
        if (names(lhs, counter) && one(rhs)) || (one(lhs) && names(rhs, counter)))
}

/// The bounded counters of the proof's function: each counter with the set of
/// bounds `b` for which every read of it lies in `[0, b)`.
pub(super) fn bounded_counters(proof: &LaneIndexProof<'_>) -> BTreeMap<BaseKey, BTreeSet<Sym>> {
    let function = proof.function;
    let Some(counters) = counters(proof) else {
        return BTreeMap::new();
    };
    if counters.is_empty() {
        return BTreeMap::new();
    }
    let compares = compares(proof, &counters);
    let mut excluded: BTreeSet<Position> = counters.values().flatten().copied().collect();
    excluded.extend(compares.values().map(|(_, _, position)| *position));
    let entries = fixpoint(function, &counters, &compares);

    let mut bounds: BTreeMap<BaseKey, Option<BTreeSet<Sym>>> =
        counters.keys().map(|key| (*key, None)).collect();
    let mut note = |key: &BaseKey, facts: &BTreeSet<(BaseKey, Sym)>| {
        if let Some(slot) = bounds.get_mut(key) {
            let here: BTreeSet<Sym> = facts
                .iter()
                .filter(|(counter, _)| counter == key)
                .map(|(_, bound)| bound.clone())
                .collect();
            *slot = Some(match slot.take() {
                None => here,
                Some(known) => known.intersection(&here).cloned().collect(),
            });
        }
    };
    for (index, block) in function.blocks.iter().enumerate() {
        let Some(mut facts) = entries[index].clone() else {
            continue;
        };
        let mut pending = BTreeMap::new();
        for (position, statement) in block.statements.iter().enumerate() {
            if !excluded.contains(&(index, position)) {
                for key in statement_reads(statement) {
                    note(&key, &facts);
                }
            }
            step(
                statement,
                (index, position),
                &counters,
                &compares,
                &mut facts,
                &mut pending,
            );
        }
        let mut reads = Reads::default();
        reads.visit_terminator(&block.terminator);
        for key in reads.0 {
            note(&key, &facts);
        }
    }
    bounds
        .into_iter()
        .filter_map(|(key, set)| set.filter(|set| !set.is_empty()).map(|set| (key, set)))
        .collect()
}

/// The counter candidates: projection-free non-parameter locals whose every
/// write is a non-negative literal or a `+ 1` step, with at least one literal.
/// Each maps to the positions of its step statements (reads of the counter
/// there are the step itself, not a use). `None` when the CFG has a block id
/// that does not resolve.
fn counters(proof: &LaneIndexProof<'_>) -> Option<BTreeMap<BaseKey, Vec<Position>>> {
    let function = proof.function;
    let params: BTreeSet<u32> = function.params.iter().map(|param| param.local.0).collect();
    let mut steps: BTreeMap<BaseKey, Vec<Position>> = BTreeMap::new();
    let mut inits: BTreeSet<BaseKey> = BTreeSet::new();
    let mut disqualified: BTreeSet<BaseKey> = BTreeSet::new();
    // The single-definition temps holding `c + 1`, by temp key.
    let mut step_temps: BTreeMap<BaseKey, (BaseKey, Position)> = BTreeMap::new();
    for (index, block) in function.blocks.iter().enumerate() {
        for (position, statement) in block.statements.iter().enumerate() {
            if let MirStatementKind::Assign { place, value } = &statement.kind
                && place.projections.is_empty()
                && let MirPlaceBase::Temp(_) = place.base
                && let MirValueKind::Binary { lhs, rhs, .. } = &value.kind
            {
                for operand in [lhs, rhs] {
                    if let Some(counter) = operand_key(operand)
                        && is_step(&value.kind, counter)
                    {
                        step_temps.insert(BaseKey::of(&place.base), (counter, (index, position)));
                    }
                }
            }
        }
    }
    for (index, block) in function.blocks.iter().enumerate() {
        for (position, statement) in block.statements.iter().enumerate() {
            let Some(key) = statement_write(statement) else {
                continue;
            };
            let BaseKey::Local(local) = key else {
                continue;
            };
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                disqualified.insert(key);
                continue;
            };
            if params.contains(&local) || !place.projections.is_empty() {
                disqualified.insert(key);
                continue;
            }
            match &value.kind {
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(n))) if *n >= 0 => {
                    inits.insert(key);
                }
                kind if is_step(kind, key) => {
                    steps.entry(key).or_default().push((index, position));
                }
                MirValueKind::Operand(operand) => match operand_key(operand)
                    .filter(|temp| !proof.defs.multi.contains(temp))
                    .and_then(|temp| step_temps.get(&temp))
                {
                    Some((counter, step)) if *counter == key => {
                        steps.entry(key).or_default().push(*step);
                    }
                    _ => {
                        disqualified.insert(key);
                    }
                },
                _ => {
                    disqualified.insert(key);
                }
            }
        }
        if let MirTerminatorKind::TryCall {
            destination,
            error_place,
            ..
        } = &block.terminator.kind
        {
            disqualified.extend(destination.iter().map(|place| BaseKey::of(&place.base)));
            disqualified.insert(BaseKey::of(&error_place.base));
        }
    }
    for block in &function.blocks {
        for target in successors(&block.terminator.kind) {
            function.block_by_id(target)?;
        }
    }
    Some(
        inits
            .into_iter()
            .filter(|key| !disqualified.contains(key))
            .map(|key| (key, steps.remove(&key).unwrap_or_default()))
            .collect(),
    )
}

/// The single-definition `c < b` compares on a counter, by the compare's
/// key: the counter, the bound and the compare's position. `b` must evaluate
/// to a symbolic value (literals, tensor extents and their products).
fn compares(
    proof: &LaneIndexProof<'_>,
    counters: &BTreeMap<BaseKey, Vec<Position>>,
) -> BTreeMap<BaseKey, (BaseKey, Sym, Position)> {
    let mut found = BTreeMap::new();
    for (index, block) in proof.function.blocks.iter().enumerate() {
        for (position, statement) in block.statements.iter().enumerate() {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            let MirValueKind::Binary {
                op: MirBinOp::Lt,
                lhs,
                rhs,
            } = &value.kind
            else {
                continue;
            };
            let key = BaseKey::of(&place.base);
            let Some(counter) = operand_key(lhs).filter(|counter| counters.contains_key(counter))
            else {
                continue;
            };
            if !place.projections.is_empty() || proof.defs.multi.contains(&key) {
                continue;
            }
            let bound = proof.eval_operand(rhs, 0);
            if bound != Sym::Opaque {
                found.insert(key, (counter, bound, (index, position)));
            }
        }
    }
    found
}

/// One statement's effect on the facts: a write of a counter drops its bounds
/// (and its pending compares); a compare becomes pending for the block's
/// branch.
fn step(
    statement: &MirStatement,
    position: Position,
    counters: &BTreeMap<BaseKey, Vec<Position>>,
    compares: &BTreeMap<BaseKey, (BaseKey, Sym, Position)>,
    facts: &mut BTreeSet<(BaseKey, Sym)>,
    pending: &mut BTreeMap<BaseKey, (BaseKey, Sym)>,
) {
    let Some(written) = statement_write(statement) else {
        return;
    };
    if counters.contains_key(&written) {
        facts.retain(|(counter, _)| *counter != written);
        pending.retain(|_, (counter, _)| *counter != written);
    }
    if let Some((counter, bound, at)) = compares.get(&written)
        && *at == position
    {
        pending.insert(written, (*counter, bound.clone()));
    }
}

fn successors(kind: &MirTerminatorKind) -> Vec<crate::MirBlockId> {
    match kind {
        MirTerminatorKind::Goto(target) => vec![*target],
        MirTerminatorKind::Branch {
            then_block,
            else_block,
            ..
        } => vec![*then_block, *else_block],
        MirTerminatorKind::TryCall {
            ok_block,
            error_block,
            ..
        } => vec![*ok_block, *error_block],
        MirTerminatorKind::Switch { cases, default, .. } => cases
            .iter()
            .map(|case| case.target)
            .chain(std::iter::once(*default))
            .collect(),
        MirTerminatorKind::Return(_)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::ReturnError(_)
        | MirTerminatorKind::Unreachable => Vec::new(),
    }
}

/// The facts at each block's entry, by block position.
fn fixpoint(
    function: &MirFunction,
    counters: &BTreeMap<BaseKey, Vec<Position>>,
    compares: &BTreeMap<BaseKey, (BaseKey, Sym, Position)>,
) -> Vec<Facts> {
    let index_of: BTreeMap<u32, usize> = function
        .blocks
        .iter()
        .enumerate()
        .map(|(index, block)| (block.id.0, index))
        .collect();
    let mut entries: Vec<Facts> = vec![None; function.blocks.len()];
    if entries.is_empty() {
        return entries;
    }
    entries[0] = Some(BTreeSet::new());
    let mut changed = true;
    while changed {
        changed = false;
        for (index, block) in function.blocks.iter().enumerate() {
            let Some(mut facts) = entries[index].clone() else {
                continue;
            };
            let mut pending = BTreeMap::new();
            for (position, statement) in block.statements.iter().enumerate() {
                step(
                    statement,
                    (index, position),
                    counters,
                    compares,
                    &mut facts,
                    &mut pending,
                );
            }
            let mut edges = Vec::new();
            if let MirTerminatorKind::Branch {
                condition,
                then_block,
                else_block,
            } = &block.terminator.kind
            {
                let mut then_facts = facts.clone();
                if let Some(fact) = operand_key(condition).and_then(|key| pending.get(&key)) {
                    then_facts.insert(fact.clone());
                }
                edges.push((*then_block, then_facts));
                edges.push((*else_block, facts));
            } else {
                for target in successors(&block.terminator.kind) {
                    edges.push((target, facts.clone()));
                }
            }
            for (target, incoming) in edges {
                let Some(&target) = index_of.get(&target.0) else {
                    continue;
                };
                let merged = match &entries[target] {
                    None => incoming,
                    Some(known) => known.intersection(&incoming).cloned().collect(),
                };
                if entries[target].as_ref() != Some(&merged) {
                    entries[target] = Some(merged);
                    changed = true;
                }
            }
        }
    }
    entries
}
