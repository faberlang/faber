//! Target-neutral structurizer: a reducible MIR CFG becomes a region tree.
//!
//! Faber has no `goto`, so every lowered CFG is reducible. Emitters that need
//! structured control flow (Metal, WGSL) consume the [`Region`] tree instead of
//! pattern-matching block shapes.
//!
//! # Algorithm
//!
//! Blocks reachable from the entry (`blocks[0]`) are collected by a depth-first
//! walk that records back edges and rejects unsupported terminators. With the
//! back edges removed the graph is a DAG: the walk yields a reverse topological
//! order, and the set of blocks reachable from each block is computed along it.
//!
//! # Loops
//!
//! A back-edge target is a loop header. Its natural loop (the header plus every
//! block that reaches a back-edge source without passing the header) must be
//! single-entry, else [`StructurizeError::IrreducibleLoop`]. The loop becomes
//! [`Region::Loop`]; inside it an edge to the header is [`Region::Continue`] and
//! an edge to the loop's exit block (the earliest block every exit edge reaches)
//! is [`Region::Break`]. An edge that would leave or restart an OUTER loop from
//! an inner one needs a labelled jump, which the targets lack, and fails closed
//! as [`StructurizeError::MultiLevelExit`]. A `Branch` becomes an
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
//! A `ReturnError` terminator is a dead end like `Return`: it becomes
//! [`Region::Fault`] carrying its THROW ORDINAL (its index among the
//! function's `ReturnError` blocks in block order, the numbering of
//! [`crate::throw_sites`]). `Switch`, `TryCall` and `Unreachable` terminators
//! fail closed as [`StructurizeError::UnsupportedTerminator`].

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
    /// A constant throw (`ReturnError`): a dead end like `Return`. The payload
    /// is the throw ordinal among the function's `ReturnError` blocks in
    /// block order; the ABI adds the index-site count to get the fault slot.
    Fault(u32),
    /// A natural loop: the body repeats until it executes a `Break` or `Return`.
    /// The body starts at the loop header block.
    Loop { body: Box<Region> },
    /// Leave the innermost enclosing `Loop`; control resumes after it.
    Break,
    /// Restart the innermost enclosing `Loop` at its header.
    Continue,
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
    /// A loop with more than one entry: `block` inside the natural loop of
    /// `header` is reachable without passing `header`.
    IrreducibleLoop {
        header: MirBlockId,
        block: MirBlockId,
    },
    /// An edge to the header or exit of an outer loop from inside an inner
    /// loop; structured `break` / `continue` reach only the innermost loop.
    MultiLevelExit { target: MirBlockId },
    /// A terminator with no structured form here.
    UnsupportedTerminator {
        block: MirBlockId,
        kind: &'static str,
    },
    /// A block reachable from both arms of a branch that has no common join.
    SharedTail { block: MirBlockId },
}

const NOT_FOUND: usize = usize::MAX;

/// Build the region tree for a reducible function CFG.
///
/// # Errors
///
/// See [`StructurizeError`].
pub fn structurize(function: &MirFunction) -> Result<Region, StructurizeError> {
    structurize_with(function, false)
}

/// [`structurize`] that places a shared tail by duplicating it into every arm
/// that reaches it (tail duplication), for emitters whose targets have no
/// `goto`. A branch never uses one of its own arm starts as the join (an arm
/// that merely falls into the other arm's start emits that start itself), and a
/// block may be emitted once per exclusive path. Emitting a block twice on ONE
/// path, or growing the tree past a fixed multiple of the block count, still
/// fails closed as [`StructurizeError::SharedTail`].
///
/// # Errors
///
/// See [`StructurizeError`].
pub fn structurize_duplicating_tails(function: &MirFunction) -> Result<Region, StructurizeError> {
    structurize_with(function, true)
}

fn structurize_with(
    function: &MirFunction,
    duplicate_tails: bool,
) -> Result<Region, StructurizeError> {
    let entry = function
        .blocks
        .first()
        .ok_or(StructurizeError::EmptyFunction)?;
    let graph = Graph::build(function, entry.id)?;
    let start = graph.position(entry.id);
    let loops = graph.natural_loops(start)?;
    let mut walker = Walker {
        function,
        reach: graph.reachability(),
        must: if duplicate_tails {
            graph.must_pass()
        } else {
            Vec::new()
        },
        graph: &graph,
        emitted: vec![false; function.blocks.len()],
        loops,
        active: Vec::new(),
        entering: None,
        duplicate_tails,
        emitted_total: 0,
    };
    Ok(Region::Seq(walker.emit(start, None)?))
}

/// Reachable subgraph over block positions.
struct Graph {
    /// Forward edges without back edges (a DAG), for reachability.
    successors: Vec<Vec<usize>>,
    /// Every edge in terminator order, back edges included, for emission.
    targets: Vec<Vec<usize>>,
    /// `(source, header)` for every back edge.
    back_edges: Vec<(usize, usize)>,
    /// Reachable positions, successors before predecessors.
    postorder: Vec<usize>,
    ids: Vec<MirBlockId>,
}

/// A natural loop: its header and member blocks.
struct NaturalLoop {
    body: Vec<bool>,
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
            targets: vec![Vec::new(); count],
            back_edges: Vec::new(),
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
            MirTerminatorKind::Return(_) | MirTerminatorKind::ReturnError(_) => Vec::new(),
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
            self.targets[at].push(next);
            match state[next] {
                1 => {
                    self.back_edges.push((at, next));
                    continue;
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

    /// The natural loop of every header, indexed by header position. Rejects a
    /// loop that can be entered other than through its header.
    fn natural_loops(&self, entry: usize) -> Result<Vec<Option<NaturalLoop>>, StructurizeError> {
        let count = self.ids.len();
        let mut preds = vec![Vec::new(); count];
        for &block in &self.postorder {
            for &target in &self.targets[block] {
                preds[target].push(block);
            }
        }
        let mut loops: Vec<Option<NaturalLoop>> = (0..count).map(|_| None).collect();
        for &(_, header) in &self.back_edges {
            if loops[header].is_some() {
                continue;
            }
            let mut body = vec![false; count];
            body[header] = true;
            let mut work: Vec<usize> = self
                .back_edges
                .iter()
                .filter(|(_, to)| *to == header)
                .map(|(from, _)| *from)
                .collect();
            while let Some(block) = work.pop() {
                if std::mem::replace(&mut body[block], true) {
                    continue;
                }
                work.extend(preds[block].iter().copied());
            }
            let entered_elsewhere = (0..count)
                .filter(|&member| body[member] && member != header)
                .find(|&member| member == entry || preds[member].iter().any(|&pred| !body[pred]));
            if let Some(member) = entered_elsewhere {
                return Err(StructurizeError::IrreducibleLoop {
                    header: self.ids[header],
                    block: self.ids[member],
                });
            }
            loops[header] = Some(NaturalLoop { body });
        }
        Ok(loops)
    }

    /// `must[b][x]`: every path from `b` to a function exit (forward edges
    /// only) passes through `x`, `b` and `x == b` included.
    fn must_pass(&self) -> Vec<Vec<bool>> {
        let count = self.ids.len();
        let mut must = vec![vec![false; count]; count];
        for &block in &self.postorder {
            let mut meet: Option<Vec<bool>> = None;
            for &succ in &self.successors[block] {
                meet = Some(match meet {
                    None => must[succ].clone(),
                    Some(acc) => acc.iter().zip(&must[succ]).map(|(a, b)| *a && *b).collect(),
                });
            }
            let mut row = meet.unwrap_or_else(|| vec![false; count]);
            row[block] = true;
            must[block] = row;
        }
        must
    }

    /// `reach[b][x]`: `x` is reachable from `b` (including `b` itself).
    fn reachability(&self) -> Vec<Vec<bool>> {
        let count = self.ids.len();
        let mut reach = vec![vec![false; count]; count];
        for &block in &self.postorder {
            reach[block][block] = true;
            for &succ in &self.successors[block] {
                let from_succ = reach[succ].clone();
                for (reachable, &via_succ) in reach[block].iter_mut().zip(&from_succ) {
                    *reachable |= via_succ;
                }
            }
        }
        reach
    }
}

fn terminator_name(kind: &MirTerminatorKind) -> &'static str {
    match kind {
        MirTerminatorKind::Return(_) => "return",
        MirTerminatorKind::ReturnThread => "return_thread",
        MirTerminatorKind::ReturnError(_) => "return_error",
        MirTerminatorKind::TryCall { .. } => "try_call",
        MirTerminatorKind::Goto(_) => "goto",
        MirTerminatorKind::Branch { .. } => "branch",
        MirTerminatorKind::Switch { .. } => "switch",
        MirTerminatorKind::Unreachable => "unreachable",
    }
}

/// A loop being emitted: where `Continue` and `Break` go.
struct ActiveLoop {
    header: usize,
    exit: Option<usize>,
}

struct Walker<'a> {
    function: &'a MirFunction,
    graph: &'a Graph,
    reach: Vec<Vec<bool>>,
    /// Must-pass table (tail-duplication mode only).
    must: Vec<Vec<bool>>,
    emitted: Vec<bool>,
    loops: Vec<Option<NaturalLoop>>,
    active: Vec<ActiveLoop>,
    /// The header whose first emission is the loop body itself, not a new loop.
    entering: Option<usize>,
    /// Tail-duplication mode (see [`structurize_duplicating_tails`]).
    duplicate_tails: bool,
    /// Blocks emitted so far, duplicates included (the duplication budget).
    emitted_total: usize,
}

/// Duplicated emission may grow the tree to this multiple of the block count.
const DUPLICATION_FACTOR: usize = 24;

impl Walker<'_> {
    /// Whether an edge to `at` is a `Break` / `Continue` of the innermost loop.
    fn is_jump(&self, at: usize) -> bool {
        self.active
            .last()
            .is_some_and(|ctx| ctx.header == at || ctx.exit == Some(at))
    }

    /// Earliest block reachable from both arms, if any. An arm that is itself a
    /// jump out of (or back into) the loop reaches nothing; a join that is a
    /// jump target is no join (each arm emits its own jump).
    fn join(&self, then_at: usize, else_at: usize) -> Option<usize> {
        if self.is_jump(then_at) || self.is_jump(else_at) {
            return None;
        }
        // Postorder lists successors first, so the latest entry is the earliest block.
        let earliest = |accept: &dyn Fn(usize) -> bool| {
            self.graph
                .postorder
                .iter()
                .rev()
                .copied()
                .find(|&x| self.reach[then_at][x] && self.reach[else_at][x] && accept(x))
                .filter(|&x| !self.is_jump(x))
        };
        if self.duplicate_tails {
            // The immediate post-dominator of the branch: every path of both
            // arms passes it. Without one the arms are emitted whole, tails
            // duplicated; the weaker reachability join serves only the shapes
            // (loop exits) where a path ends in a jump instead of a block.
            return earliest(&|x| self.must[then_at][x] && self.must[else_at][x])
                .or_else(|| earliest(&|x| self.joins_a_whole_arm(then_at, else_at, x)));
        }
        earliest(&|_| true)
    }

    /// Tail-duplication mode: `x` is a sound join only when one whole arm
    /// cannot bypass it. An arm start is a join only when the other arm must
    /// pass through it; any other block must be passed by at least one arm.
    /// Otherwise `x` lies inside a tail that both arms enter at different
    /// points, which is duplicated into each arm instead.
    fn joins_a_whole_arm(&self, then_at: usize, else_at: usize, x: usize) -> bool {
        if x == then_at {
            return self.must[else_at][x];
        }
        if x == else_at {
            return self.must[then_at][x];
        }
        self.must[then_at][x] || self.must[else_at][x]
    }

    /// The `Break` / `Continue` an edge to `at` becomes, if it is one.
    fn jump(&self, at: usize) -> Result<Option<Region>, StructurizeError> {
        if let Some(ctx) = self.active.last() {
            if ctx.header == at {
                return Ok(Some(Region::Continue));
            }
            if ctx.exit == Some(at) {
                return Ok(Some(Region::Break));
            }
        }
        let outer = self.active.iter().rev().skip(1);
        for ctx in outer {
            if ctx.header == at || ctx.exit == Some(at) {
                return Err(StructurizeError::MultiLevelExit {
                    target: self.graph.ids[at],
                });
            }
        }
        Ok(None)
    }

    /// The block control resumes at after the loop headed by `header`: the
    /// single exit target, else the earliest block every exit target reaches.
    fn loop_exit(&self, header: usize) -> Option<usize> {
        let body = &self.loops[header].as_ref()?.body;
        let mut exits: Vec<usize> = Vec::new();
        for (block, _) in body.iter().enumerate().filter(|(_, member)| **member) {
            for &target in &self.graph.targets[block] {
                if !body[target] && !exits.contains(&target) {
                    exits.push(target);
                }
            }
        }
        match exits[..] {
            [] => None,
            [only] => Some(only),
            _ => self
                .graph
                .postorder
                .iter()
                .rev()
                .copied()
                .find(|&x| exits.iter().all(|&exit| self.reach[exit][x]))
                .or_else(|| self.main_exit(header, &exits)),
        }
    }

    /// With no block every exit reaches (the others return on their own), the
    /// exit with the longest continuation; ties go to the header's own exit.
    fn main_exit(&self, header: usize, exits: &[usize]) -> Option<usize> {
        let size = |exit: usize| self.reach[exit].iter().filter(|r| **r).count();
        let longest = exits.iter().map(|&exit| size(exit)).max()?;
        let ties: Vec<usize> = exits
            .iter()
            .copied()
            .filter(|&exit| size(exit) == longest)
            .collect();
        ties.iter()
            .copied()
            .find(|exit| self.graph.targets[header].contains(exit))
            .or_else(|| ties.first().copied())
    }

    /// Emit the loop headed by `header` into `out`; the block control resumes at.
    fn emit_loop(
        &mut self,
        header: usize,
        out: &mut Vec<Region>,
    ) -> Result<Option<usize>, StructurizeError> {
        let exit = self.loop_exit(header);
        self.active.push(ActiveLoop { header, exit });
        self.entering = Some(header);
        let body = self.emit(header, None);
        self.active.pop();
        out.push(Region::Loop {
            body: Box::new(Region::Seq(body?)),
        });
        Ok(exit)
    }

    /// Emit from `start` up to (not including) `stop`, or to a `Return`.
    fn emit(&mut self, start: usize, stop: Option<usize>) -> Result<Vec<Region>, StructurizeError> {
        let mut out = Vec::new();
        let mut at = start;
        loop {
            if Some(at) == stop {
                break;
            }
            if self.entering == Some(at) {
                self.entering = None;
            } else {
                if let Some(jump) = self.jump(at)? {
                    out.push(jump);
                    break;
                }
                if self.loops[at].is_some() {
                    match self.emit_loop(at, &mut out)? {
                        Some(next) => {
                            at = next;
                            continue;
                        }
                        None => break,
                    }
                }
            }
            if std::mem::replace(&mut self.emitted[at], true) {
                return Err(StructurizeError::SharedTail {
                    block: self.graph.ids[at],
                });
            }
            self.emitted_total += 1;
            if self.duplicate_tails
                && self.emitted_total > DUPLICATION_FACTOR * self.emitted.len() + 64
            {
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
                MirTerminatorKind::ReturnError(_) => {
                    let ordinal = self.function.blocks[..at]
                        .iter()
                        .filter(|earlier| {
                            matches!(earlier.terminator.kind, MirTerminatorKind::ReturnError(_))
                        })
                        .count();
                    out.push(Region::Fault(u32::try_from(ordinal).unwrap_or(u32::MAX)));
                    break;
                }
                MirTerminatorKind::Goto(_) => at = self.graph.targets[at][0],
                MirTerminatorKind::Branch { condition, .. } => {
                    let [then_at, else_at] = self.graph.targets[at][..] else {
                        return Err(StructurizeError::UnsupportedTerminator {
                            block: block.id,
                            kind: terminator_name(&block.terminator.kind),
                        });
                    };
                    let join = self.join(then_at, else_at);
                    let arm_stop = join.or(stop);
                    // Exclusive arms may each emit a shared tail once; the
                    // path after the `If` has emitted the union of both.
                    let before = self.duplicate_tails.then(|| self.emitted.clone());
                    let then_region = Region::Seq(self.emit(then_at, arm_stop)?);
                    let after_then = before
                        .as_ref()
                        .map(|before| std::mem::replace(&mut self.emitted, before.clone()));
                    let else_region = Region::Seq(self.emit(else_at, arm_stop)?);
                    if let Some(after_then) = after_then {
                        for (mark, then_mark) in self.emitted.iter_mut().zip(after_then) {
                            *mark |= then_mark;
                        }
                    }
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
                other => {
                    return Err(StructurizeError::UnsupportedTerminator {
                        block: block.id,
                        kind: terminator_name(other),
                    });
                }
            }
        }
        Ok(out)
    }
}
