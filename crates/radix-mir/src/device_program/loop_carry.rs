//! Loop-carry facts of the host call order.
//!
//! WHY: a device program lists each kernel once, and its producer/consumer
//! graph joins buffers across kernels. A host loop that updates a buffer in
//! place -- `fwd(w, mut y)` then `upd(y, mut w)` -- makes that graph a cycle,
//! though no launch waits on a later one: `upd`'s write of `w` is read by
//! `fwd` only in the NEXT iteration. The host call order tells the two apart.
//! This module is the pure analysis of one host function's control-flow graph;
//! it decides nothing about buffers.
//!
//! THE RULE. For kernels `a` and `b` called by the same host function:
//!
//! - `a` is *before* `b` when some call site of `a` precedes some call site of
//!   `b` in a reverse post-order of the CFG (one iteration of any loop);
//! - `b` is *reachable after* `a` when some call site of `b` can execute after
//!   some call site of `a` (through a loop's back edge or in straight code).
//!
//! `a`'s output reaches `b` only through the loop carry when `b` is reachable
//! after `a` and `a` is never before `b`. Any other pair keeps its edge:
//! unrelated branches and straight-line anti-dependences are not loop carry.

use std::collections::{BTreeMap, BTreeSet};

use crate::{MirBlockId, MirFunction, MirFunctionId, MirTerminatorKind};

/// One call site of a kernel inside a host function: block position and
/// statement position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CallSite {
    pub(super) kernel: MirFunctionId,
    pub(super) block: usize,
    pub(super) statement: usize,
}

/// The ordered kernel pairs of one host function.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CallOrder {
    /// `(a, b)`: a call site of `a` precedes a call site of `b` in one
    /// iteration.
    pub(super) before: BTreeSet<(MirFunctionId, MirFunctionId)>,
    /// `(a, b)`: a call site of `b` can execute after a call site of `a`.
    pub(super) reaches: BTreeSet<(MirFunctionId, MirFunctionId)>,
}

fn successors(kind: &MirTerminatorKind) -> Vec<MirBlockId> {
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

/// The call order of `sites` in `function`. Sites in blocks the entry block
/// cannot reach are ignored.
pub(super) fn call_order(function: &MirFunction, sites: &[CallSite]) -> CallOrder {
    let index_of: BTreeMap<MirBlockId, usize> = function
        .blocks
        .iter()
        .enumerate()
        .map(|(index, block)| (block.id, index))
        .collect();
    let edges: Vec<Vec<usize>> = function
        .blocks
        .iter()
        .map(|block| {
            successors(&block.terminator.kind)
                .into_iter()
                .filter_map(|target| index_of.get(&target).copied())
                .collect()
        })
        .collect();

    // Reverse post-order from the entry block (an iterative depth-first walk).
    let mut rank: Vec<Option<usize>> = vec![None; edges.len()];
    let mut post: Vec<usize> = Vec::new();
    if !edges.is_empty() {
        let mut seen = vec![false; edges.len()];
        let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
        seen[0] = true;
        while let Some((block, next)) = stack.pop() {
            if let Some(&target) = edges[block].get(next) {
                stack.push((block, next + 1));
                if !seen[target] {
                    seen[target] = true;
                    stack.push((target, 0));
                }
            } else {
                post.push(block);
            }
        }
    }
    for (position, block) in post.iter().rev().enumerate() {
        rank[*block] = Some(position);
    }

    // `after[b]`: the blocks reachable from `b` through at least one edge.
    let mut after: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); edges.len()];
    for (start, reached) in after.iter_mut().enumerate() {
        let mut frontier: Vec<usize> = edges[start].clone();
        while let Some(block) = frontier.pop() {
            if reached.insert(block) {
                frontier.extend(edges[block].iter().copied());
            }
        }
    }

    let mut order = CallOrder::default();
    for first in sites {
        for second in sites {
            if first.kernel == second.kernel {
                continue;
            }
            let (Some(first_rank), Some(second_rank)) = (rank[first.block], rank[second.block])
            else {
                continue;
            };
            let pair = (first.kernel, second.kernel);
            if (first_rank, first.statement) < (second_rank, second.statement) {
                order.before.insert(pair);
            }
            let same_block_later =
                first.block == second.block && second.statement > first.statement;
            if same_block_later || after[first.block].contains(&second.block) {
                order.reaches.insert(pair);
            }
        }
    }
    order
}
