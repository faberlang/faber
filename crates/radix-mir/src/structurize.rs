//! Target-neutral structurizer: an acyclic MIR CFG becomes a region tree.
//!
//! Faber has no `goto`, so every lowered CFG is reducible. Emitters that need
//! structured control flow (Metal, WGSL) consume the [`Region`] tree instead of
//! pattern-matching block shapes.
//!
//! # Algorithm
//!
//! Blocks reachable from the entry (`blocks[0]`) are collected by a depth-first
//! walk that rejects back edges ([`StructurizeError::Loop`]) and unsupported
//! terminators. The walk yields a reverse topological order, and the set of
//! blocks reachable from each block is computed along it. A `Branch` becomes an
//! [`Region::If`] whose join is its immediate post-dominator modulo returns: the
//! earliest block (topologically) reachable from both arms. Both arms are
//! emitted up to the join and the walk continues there. With no join (the arms
//! never meet: each ends in a `Return`, or reaches the enclosing region's join)
//! each arm is emitted to its own end and the `If` closes the region. A path
//! that returns early inside an arm therefore needs no special case.
//!
//! # Canonical form
//!
//! * `Goto` chains and straight-line code become consecutive [`Region::Block`]
//!   entries of the enclosing [`Region::Seq`]; one `Block` per MIR block.
//! * `Block(b)` stands for the statements of `b`; the terminator is expressed by
//!   the following region (`If`, `Return`, or the next `Block`).
//! * Both arms of an `If` are always a `Seq` (possibly empty when an arm goes
//!   straight to the join).
//! * An early-return guard (`branch c bb1 bb2`, `bb1: return`) is the no-join
//!   case: `If { then: Seq[Block(bb1), Return], else: Seq[Block(bb2), ..] }`.
//! * Every reachable block appears exactly once. A block that two arms would
//!   both have to emit (a shared tail with no common join) cannot be placed
//!   without duplication and fails closed as [`StructurizeError::SharedTail`].
//!
//! `Switch`, `TryCall`, `ReturnError` and `Unreachable` terminators fail closed
//! as [`StructurizeError::UnsupportedTerminator`].

use crate::{MirBlockId, MirFunction, MirOperand, MirTerminatorKind};

/// One node of the structured control-flow tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Region {
    /// Ordered regions executed one after another.
    Seq(Vec<Region>),
    /// The straight-line statements of one MIR block (not its terminator).
    Block(MirBlockId),
    /// Two-way conditional; control rejoins after it unless both arms return.
    If {
        cond: MirOperand,
        then_region: Box<Region>,
        else_region: Box<Region>,
    },
    /// Function return.
    Return(Option<MirOperand>),
}

/// Why a CFG cannot be structurized (yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructurizeError {
    /// The function has no blocks.
    EmptyFunction,
    /// A terminator names a block the function does not contain.
    UnknownBlock {
        from: MirBlockId,
        target: MirBlockId,
    },
    /// A back edge `from -> header` closes a loop (structurized by C4-4b).
    Loop {
        from: MirBlockId,
        header: MirBlockId,
    },
    /// A terminator with no structured form here.
    UnsupportedTerminator {
        block: MirBlockId,
        kind: &'static str,
    },
    /// A block reachable from both arms of a branch that has no common join.
    SharedTail { block: MirBlockId },
}

const NOT_FOUND: usize = usize::MAX;

/// Build the region tree for an acyclic function CFG.
///
/// # Errors
///
/// See [`StructurizeError`].
pub fn structurize(function: &MirFunction) -> Result<Region, StructurizeError> {
    let entry = function
        .blocks
        .first()
        .ok_or(StructurizeError::EmptyFunction)?;
    let graph = Graph::build(function, entry.id)?;
    let mut walker = Walker {
        function,
        reach: graph.reachability(),
        graph: &graph,
        emitted: vec![false; function.blocks.len()],
    };
    let start = graph.position(entry.id);
    Ok(Region::Seq(walker.emit(start, None)?))
}

/// Reachable subgraph over block positions.
struct Graph {
    successors: Vec<Vec<usize>>,
    /// Reachable positions, successors before predecessors.
    postorder: Vec<usize>,
    ids: Vec<MirBlockId>,
}

impl Graph {
    fn position(&self, id: MirBlockId) -> usize {
        self.ids
            .iter()
            .position(|candidate| *candidate == id)
            .unwrap_or(NOT_FOUND)
    }

    fn build(function: &MirFunction, entry: MirBlockId) -> Result<Self, StructurizeError> {
        let count = function.blocks.len();
        let mut graph = Self {
            successors: vec![Vec::new(); count],
            postorder: Vec::new(),
            ids: function.blocks.iter().map(|block| block.id).collect(),
        };
        let mut state = vec![0u8; count]; // 0 new, 1 on stack, 2 done
        let start = graph.position(entry);
        graph.visit(function, start, &mut state)?;
        Ok(graph)
    }

    fn visit(
        &mut self,
        function: &MirFunction,
        at: usize,
        state: &mut [u8],
    ) -> Result<(), StructurizeError> {
        state[at] = 1;
        let block = &function.blocks[at];
        let targets: Vec<MirBlockId> = match &block.terminator.kind {
            MirTerminatorKind::Return(_) => Vec::new(),
            MirTerminatorKind::Goto(target) => vec![*target],
            MirTerminatorKind::Branch {
                then_block,
                else_block,
                ..
            } => vec![*then_block, *else_block],
            other => {
                return Err(StructurizeError::UnsupportedTerminator {
                    block: block.id,
                    kind: terminator_name(other),
                });
            }
        };
        for target in targets {
            let next = self.position(target);
            if next == NOT_FOUND {
                return Err(StructurizeError::UnknownBlock {
                    from: block.id,
                    target,
                });
            }
            match state[next] {
                1 => {
                    return Err(StructurizeError::Loop {
                        from: block.id,
                        header: target,
                    });
                }
                0 => self.visit(function, next, state)?,
                _ => {}
            }
            self.successors[at].push(next);
        }
        state[at] = 2;
        self.postorder.push(at);
        Ok(())
    }

    /// `reach[b][x]`: `x` is reachable from `b` (including `b` itself).
    fn reachability(&self) -> Vec<Vec<bool>> {
        let count = self.ids.len();
        let mut reach = vec![vec![false; count]; count];
        for &block in &self.postorder {
            reach[block][block] = true;
            for &succ in &self.successors[block] {
                for x in 0..count {
                    if reach[succ][x] {
                        reach[block][x] = true;
                    }
                }
            }
        }
        reach
    }
}

fn terminator_name(kind: &MirTerminatorKind) -> &'static str {
    match kind {
        MirTerminatorKind::Return(_) => "return",
        MirTerminatorKind::ReturnError(_) => "return_error",
        MirTerminatorKind::TryCall { .. } => "try_call",
        MirTerminatorKind::Goto(_) => "goto",
        MirTerminatorKind::Branch { .. } => "branch",
        MirTerminatorKind::Switch { .. } => "switch",
        MirTerminatorKind::Unreachable => "unreachable",
    }
}

struct Walker<'a> {
    function: &'a MirFunction,
    graph: &'a Graph,
    reach: Vec<Vec<bool>>,
    emitted: Vec<bool>,
}

impl Walker<'_> {
    /// Earliest block reachable from both arms, if any.
    fn join(&self, then_at: usize, else_at: usize) -> Option<usize> {
        // Postorder lists successors first, so the latest entry is the earliest block.
        self.graph
            .postorder
            .iter()
            .rev()
            .copied()
            .find(|&x| self.reach[then_at][x] && self.reach[else_at][x])
    }

    /// Emit from `start` up to (not including) `stop`, or to a `Return`.
    fn emit(&mut self, start: usize, stop: Option<usize>) -> Result<Vec<Region>, StructurizeError> {
        let mut out = Vec::new();
        let mut at = start;
        while Some(at) != stop {
            if std::mem::replace(&mut self.emitted[at], true) {
                return Err(StructurizeError::SharedTail {
                    block: self.graph.ids[at],
                });
            }
            let block = &self.function.blocks[at];
            out.push(Region::Block(block.id));
            match &block.terminator.kind {
                MirTerminatorKind::Return(value) => {
                    out.push(Region::Return(value.clone()));
                    break;
                }
                MirTerminatorKind::Goto(_) => at = self.graph.successors[at][0],
                MirTerminatorKind::Branch { condition, .. } => {
                    let [then_at, else_at] = self.graph.successors[at][..] else {
                        unreachable!("branch has two successors");
                    };
                    let join = self.join(then_at, else_at);
                    let arm_stop = join.or(stop);
                    let then_region = Region::Seq(self.emit(then_at, arm_stop)?);
                    let else_region = Region::Seq(self.emit(else_at, arm_stop)?);
                    out.push(Region::If {
                        cond: condition.clone(),
                        then_region: Box::new(then_region),
                        else_region: Box::new(else_region),
                    });
                    match join {
                        Some(join) => at = join,
                        None => break,
                    }
                }
                _ => unreachable!("graph build rejects other terminators"),
            }
        }
        Ok(out)
    }
}
