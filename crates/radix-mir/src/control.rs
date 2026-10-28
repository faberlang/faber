//! Structured CFG shape recognition for device kernel lowering.
//!
//! GPU text probes (`metal_text`, `wgsl_text`) and the device-safe gate depend
//! on these predicates to recognize scalar loops, indexed writes, joins, and
//! returns without duplicating terminator-walk logic per target.

use super::{
    MirBlock, MirConstant, MirFunction, MirLocalId, MirOperand, MirPlace, MirPlaceBase,
    MirProjection, MirStatementKind, MirSwitchCase, MirTerminatorKind, MirValueKind,
};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuredScalarLoop {
    pub local: MirLocalId,
    pub condition_block: super::MirBlockId,
    pub body_block: super::MirBlockId,
    pub exit_block: super::MirBlockId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuredReturnBranch {
    pub then_block: super::MirBlockId,
    pub else_block: super::MirBlockId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuredIndexedWriteBranch {
    pub then_block: super::MirBlockId,
    pub else_block: super::MirBlockId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuredIndexedValueJoinBranch {
    pub then_block: super::MirBlockId,
    pub else_block: super::MirBlockId,
    pub join_block: super::MirBlockId,
    pub join_local: MirLocalId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuredValueJoinBranch {
    pub then_block: super::MirBlockId,
    pub else_block: super::MirBlockId,
    pub join_block: super::MirBlockId,
    pub join_local: MirLocalId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructuredScalarSwitch {
    pub cases: Vec<MirSwitchCase>,
    pub default: super::MirBlockId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructuredIndexedWriteSwitch {
    pub cases: Vec<MirSwitchCase>,
    pub default: super::MirBlockId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuredLoopControl {
    pub local: MirLocalId,
    pub condition_block: super::MirBlockId,
    pub body_block: super::MirBlockId,
    pub exit_block: super::MirBlockId,
    pub control_block: super::MirBlockId,
    pub continuation_block: super::MirBlockId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuredNestedLoopControl {
    pub local: MirLocalId,
    pub condition_block: super::MirBlockId,
    pub body_block: super::MirBlockId,
    pub exit_block: super::MirBlockId,
    pub nested_branch_block: super::MirBlockId,
    pub control_block: super::MirBlockId,
    pub nested_update_block: super::MirBlockId,
    pub continuation_block: super::MirBlockId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuredNestedScalarLoop {
    pub local: MirLocalId,
    pub condition_block: super::MirBlockId,
    pub body_block: super::MirBlockId,
    pub exit_block: super::MirBlockId,
    pub then_block: super::MirBlockId,
    pub continuation_block: super::MirBlockId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopControlKind {
    Break,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IndexedKernelTerminals {
    output: MirLocalId,
    index: MirLocalId,
}

fn indexed_kernel_terminals(function: &MirFunction) -> Option<IndexedKernelTerminals> {
    let params = function.params.as_slice();
    if params.len() < 3 {
        return None;
    }
    let [.., output, index] = params else {
        return None;
    };
    Some(IndexedKernelTerminals {
        output: output.local,
        index: index.local,
    })
}

#[must_use]
pub fn is_structured_return_branch(function: &MirFunction) -> bool {
    structured_return_branch(function).is_some()
}

#[must_use]
pub fn structured_return_branch(function: &MirFunction) -> Option<StructuredReturnBranch> {
    let entry = function.blocks.first()?;
    let MirTerminatorKind::Branch {
        then_block,
        else_block,
        ..
    } = &entry.terminator.kind
    else {
        return None;
    };
    let mut branch_blocks = BTreeSet::new();
    branch_blocks.insert(entry.id);
    let then_block = scalar_return_target(function, *then_block, &mut branch_blocks)?.id;
    let else_block = scalar_return_target(function, *else_block, &mut branch_blocks)?.id;
    if function
        .blocks
        .iter()
        .any(|block| !branch_blocks.contains(&block.id))
    {
        return None;
    }
    Some(StructuredReturnBranch {
        then_block,
        else_block,
    })
}

#[must_use]
pub fn structured_scalar_return_goto(function: &MirFunction) -> Option<super::MirBlockId> {
    let target = straight_through_goto_target(function)?;
    if !target.statements.is_empty()
        || !matches!(target.terminator.kind, MirTerminatorKind::Return(Some(_)))
    {
        return None;
    }
    Some(target.id)
}

#[must_use]
pub fn is_structured_scalar_return_goto(function: &MirFunction) -> bool {
    structured_scalar_return_goto(function).is_some()
}

#[must_use]
pub fn structured_indexed_write_goto(function: &MirFunction) -> Option<super::MirBlockId> {
    let terminals = indexed_kernel_terminals(function)?;
    let target = straight_through_goto_target(function)?;
    if !matches!(target.terminator.kind, MirTerminatorKind::Return(None)) {
        return None;
    }
    if !block_writes_output(target, terminals.output, terminals.index) {
        return None;
    }
    Some(target.id)
}

#[must_use]
pub fn is_structured_indexed_write_goto(function: &MirFunction) -> bool {
    structured_indexed_write_goto(function).is_some()
}

fn straight_through_goto_target(function: &MirFunction) -> Option<&MirBlock> {
    let mut block = function.blocks.first()?;
    if !matches!(block.terminator.kind, MirTerminatorKind::Goto(_)) {
        return None;
    }
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(block.id) {
            return None;
        }
        let MirTerminatorKind::Goto(target_id) = block.terminator.kind else {
            if visited.len() != function.blocks.len() {
                return None;
            }
            return Some(block);
        };
        if !block.statements.is_empty() {
            return None;
        }
        block = function
            .blocks
            .iter()
            .find(|candidate| candidate.id == target_id)?;
    }
}

fn scalar_return_target<'a>(
    function: &'a MirFunction,
    block_id: super::MirBlockId,
    branch_blocks: &mut BTreeSet<super::MirBlockId>,
) -> Option<&'a MirBlock> {
    let mut block = function.blocks.iter().find(|block| block.id == block_id)?;
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(block.id) {
            return None;
        }
        branch_blocks.insert(block.id);
        match block.terminator.kind {
            MirTerminatorKind::Goto(target) => {
                if !block.statements.is_empty() {
                    return None;
                }
                block = function
                    .blocks
                    .iter()
                    .find(|candidate| candidate.id == target)?;
            }
            MirTerminatorKind::Return(Some(_)) => {
                if block.statements.iter().all(is_temp_assignment) {
                    return Some(block);
                }
                return None;
            }
            _ => return None,
        }
    }
}

fn value_join_assignment_target<'a>(
    function: &'a MirFunction,
    block_id: super::MirBlockId,
    branch_blocks: &mut BTreeSet<super::MirBlockId>,
) -> Option<&'a MirBlock> {
    let mut block = function.blocks.iter().find(|block| block.id == block_id)?;
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(block.id) {
            return None;
        }
        branch_blocks.insert(block.id);
        match block.terminator.kind {
            MirTerminatorKind::Goto(target) if block.statements.is_empty() => {
                block = function
                    .blocks
                    .iter()
                    .find(|candidate| candidate.id == target)?;
            }
            MirTerminatorKind::Goto(_) => return Some(block),
            _ => return None,
        }
    }
}

#[must_use]
pub fn is_structured_indexed_write_branch(function: &MirFunction) -> bool {
    structured_indexed_write_branch(function).is_some()
}

#[must_use]
pub fn structured_indexed_write_branch(
    function: &MirFunction,
) -> Option<StructuredIndexedWriteBranch> {
    let entry = function.blocks.first()?;
    let terminals = indexed_kernel_terminals(function)?;
    let MirTerminatorKind::Branch {
        then_block,
        else_block,
        ..
    } = &entry.terminator.kind
    else {
        return None;
    };

    let mut branch_blocks = BTreeSet::new();
    branch_blocks.insert(entry.id);
    let then_block = indexed_output_write_target(
        function,
        *then_block,
        terminals.output,
        terminals.index,
        &mut branch_blocks,
    )?
    .id;
    let else_block = indexed_output_write_target(
        function,
        *else_block,
        terminals.output,
        terminals.index,
        &mut branch_blocks,
    )?
    .id;
    let join = shared_output_join(function, &[then_block, else_block])?;
    if !matches!(join.terminator.kind, MirTerminatorKind::Return(None)) {
        return None;
    }
    if !function.blocks.iter().all(|block| {
        branch_blocks.contains(&block.id)
            || block.id == join.id
            || (block.statements.is_empty()
                && matches!(block.terminator.kind, MirTerminatorKind::Unreachable))
    }) {
        return None;
    }

    Some(StructuredIndexedWriteBranch {
        then_block,
        else_block,
    })
}

#[must_use]
pub fn is_structured_indexed_write_switch(function: &MirFunction) -> bool {
    structured_indexed_write_switch(function).is_some()
}

#[must_use]
pub fn structured_indexed_write_switch(
    function: &MirFunction,
) -> Option<StructuredIndexedWriteSwitch> {
    let entry = function.blocks.first()?;
    let terminals = indexed_kernel_terminals(function)?;
    let MirTerminatorKind::Switch { cases, default, .. } = &entry.terminator.kind else {
        return None;
    };
    if cases.is_empty()
        || !cases
            .iter()
            .all(|case| is_switch_case_constant(&case.value))
    {
        return None;
    }

    let mut switch_blocks = BTreeSet::new();
    switch_blocks.insert(entry.id);
    let mut targets = Vec::with_capacity(cases.len() + 1);
    let mut resolved_cases = Vec::with_capacity(cases.len());
    for case in cases {
        let target = indexed_output_write_target(
            function,
            case.target,
            terminals.output,
            terminals.index,
            &mut switch_blocks,
        )?
        .id;
        targets.push(target);
        resolved_cases.push(MirSwitchCase {
            value: case.value.clone(),
            target,
        });
    }
    let default = indexed_output_write_target(
        function,
        *default,
        terminals.output,
        terminals.index,
        &mut switch_blocks,
    )?
    .id;
    targets.push(default);
    let join = shared_output_join(function, &targets)?;
    if !matches!(join.terminator.kind, MirTerminatorKind::Return(None)) {
        return None;
    }

    if !function.blocks.iter().all(|block| {
        switch_blocks.contains(&block.id)
            || block.id == join.id
            || (block.statements.is_empty()
                && matches!(block.terminator.kind, MirTerminatorKind::Unreachable))
    }) {
        return None;
    }

    Some(StructuredIndexedWriteSwitch {
        cases: resolved_cases,
        default,
    })
}

#[must_use]
pub fn is_structured_indexed_value_join_branch(function: &MirFunction) -> bool {
    structured_indexed_value_join_branch(function).is_some()
}

#[must_use]
pub fn structured_indexed_value_join_branch(
    function: &MirFunction,
) -> Option<StructuredIndexedValueJoinBranch> {
    let entry = function.blocks.first()?;
    let terminals = indexed_kernel_terminals(function)?;
    let MirTerminatorKind::Branch {
        then_block,
        else_block,
        ..
    } = &entry.terminator.kind
    else {
        return None;
    };

    let mut branch_blocks = BTreeSet::new();
    branch_blocks.insert(entry.id);
    let then_block = value_join_assignment_target(function, *then_block, &mut branch_blocks)?;
    let else_block = value_join_assignment_target(function, *else_block, &mut branch_blocks)?;
    let join = shared_output_join(function, &[then_block.id, else_block.id])?;
    let join_local = join_writes_output_from_local(join, terminals.output, terminals.index)?;
    if !branch_arm_assigns_local(then_block, join_local, join.id)
        || !branch_arm_assigns_local(else_block, join_local, join.id)
    {
        return None;
    }
    if !function
        .blocks
        .iter()
        .all(|block| branch_blocks.contains(&block.id) || block.id == join.id)
    {
        return None;
    }

    Some(StructuredIndexedValueJoinBranch {
        then_block: then_block.id,
        else_block: else_block.id,
        join_block: join.id,
        join_local,
    })
}

#[must_use]
pub fn is_structured_value_join_branch(function: &MirFunction) -> bool {
    structured_value_join_branch(function).is_some()
}

#[must_use]
pub fn structured_value_join_branch(function: &MirFunction) -> Option<StructuredValueJoinBranch> {
    let entry = function.blocks.first()?;
    let [_] = function.params.as_slice() else {
        return None;
    };
    let MirTerminatorKind::Branch {
        then_block,
        else_block,
        ..
    } = &entry.terminator.kind
    else {
        return None;
    };

    let mut branch_blocks = BTreeSet::new();
    branch_blocks.insert(entry.id);
    let then_block = value_join_assignment_target(function, *then_block, &mut branch_blocks)?;
    let else_block = value_join_assignment_target(function, *else_block, &mut branch_blocks)?;
    let join = shared_output_join(function, &[then_block.id, else_block.id])?;
    let MirTerminatorKind::Return(Some(MirOperand::Place(join_place))) = &join.terminator.kind
    else {
        return None;
    };
    let join_local = local_place_id(join_place)?;
    if !branch_arm_assigns_local(then_block, join_local, join.id)
        || !branch_arm_assigns_local(else_block, join_local, join.id)
    {
        return None;
    }
    if !function
        .blocks
        .iter()
        .all(|block| branch_blocks.contains(&block.id) || block.id == join.id)
    {
        return None;
    }

    Some(StructuredValueJoinBranch {
        then_block: then_block.id,
        else_block: else_block.id,
        join_block: join.id,
        join_local,
    })
}

#[must_use]
pub fn is_structured_scalar_switch(function: &MirFunction) -> bool {
    structured_scalar_switch(function).is_some()
}

#[must_use]
pub fn structured_scalar_switch(function: &MirFunction) -> Option<StructuredScalarSwitch> {
    let entry = function.blocks.first()?;
    let [_] = function.params.as_slice() else {
        return None;
    };
    let MirTerminatorKind::Switch { cases, default, .. } = &entry.terminator.kind else {
        return None;
    };
    if cases.is_empty()
        || !cases
            .iter()
            .all(|case| is_switch_case_constant(&case.value))
    {
        return None;
    }

    let mut switch_blocks = BTreeSet::new();
    switch_blocks.insert(entry.id);
    let mut resolved_cases = Vec::with_capacity(cases.len());
    for case in cases {
        let target = scalar_return_target(function, case.target, &mut switch_blocks)?.id;
        resolved_cases.push(MirSwitchCase {
            value: case.value.clone(),
            target,
        });
    }
    let default = scalar_return_target(function, *default, &mut switch_blocks)?.id;

    if !function.blocks.iter().all(|block| {
        switch_blocks.contains(&block.id)
            || (block.statements.is_empty()
                && matches!(block.terminator.kind, MirTerminatorKind::Unreachable))
    }) {
        return None;
    }

    Some(StructuredScalarSwitch {
        cases: resolved_cases,
        default,
    })
}

#[must_use]
pub fn is_structured_scalar_loop(function: &MirFunction) -> bool {
    structured_scalar_loop(function).is_some()
}

#[must_use]
pub fn is_structured_scalar_loop_break(function: &MirFunction) -> bool {
    structured_scalar_loop_break(function).is_some()
}

#[must_use]
pub fn is_structured_scalar_loop_continue(function: &MirFunction) -> bool {
    structured_scalar_loop_continue(function).is_some()
}

#[must_use]
pub fn is_structured_nested_scalar_loop_branch(function: &MirFunction) -> bool {
    structured_nested_scalar_loop_branch(function).is_some()
}

#[must_use]
pub fn is_structured_nested_indexed_write_loop_branch(function: &MirFunction) -> bool {
    structured_nested_indexed_write_loop_branch(function).is_some()
}

#[must_use]
pub fn is_structured_nested_indexed_write_loop_break(function: &MirFunction) -> bool {
    structured_nested_indexed_write_loop_break(function).is_some()
}

#[must_use]
pub fn is_structured_nested_indexed_write_loop_continue(function: &MirFunction) -> bool {
    structured_nested_indexed_write_loop_continue(function).is_some()
}

#[must_use]
pub fn is_structured_indexed_write_loop(function: &MirFunction) -> bool {
    structured_indexed_write_loop(function).is_some()
}

#[must_use]
pub fn is_structured_indexed_write_loop_break(function: &MirFunction) -> bool {
    structured_indexed_write_loop_break(function).is_some()
}

#[must_use]
pub fn is_structured_indexed_write_loop_continue(function: &MirFunction) -> bool {
    structured_indexed_write_loop_continue(function).is_some()
}

#[must_use]
pub fn structured_scalar_loop_break(function: &MirFunction) -> Option<StructuredLoopControl> {
    structured_scalar_loop_control(function, LoopControlKind::Break)
}

#[must_use]
pub fn structured_scalar_loop_continue(function: &MirFunction) -> Option<StructuredLoopControl> {
    structured_scalar_loop_control(function, LoopControlKind::Continue)
}

#[must_use]
pub fn structured_indexed_write_loop_break(
    function: &MirFunction,
) -> Option<StructuredLoopControl> {
    structured_indexed_write_loop_control(function, LoopControlKind::Break)
}

#[must_use]
pub fn structured_indexed_write_loop_continue(
    function: &MirFunction,
) -> Option<StructuredLoopControl> {
    structured_indexed_write_loop_control(function, LoopControlKind::Continue)
}

#[must_use]
pub fn structured_nested_indexed_write_loop_break(
    function: &MirFunction,
) -> Option<StructuredNestedLoopControl> {
    structured_nested_indexed_write_loop_control(function, LoopControlKind::Break)
}

#[must_use]
pub fn structured_nested_indexed_write_loop_continue(
    function: &MirFunction,
) -> Option<StructuredNestedLoopControl> {
    structured_nested_indexed_write_loop_control(function, LoopControlKind::Continue)
}

pub fn structured_nested_scalar_loop_branch(
    function: &MirFunction,
) -> Option<StructuredNestedScalarLoop> {
    let [entry, condition, body, exit, then_block, continuation] = function.blocks.as_slice()
    else {
        return None;
    };
    let [param] = function.params.as_slice() else {
        return None;
    };
    let loop_local = entry_loop_local(entry, param.local)?;
    if !function
        .locals
        .iter()
        .any(|local| local.id == loop_local && local.mutable)
    {
        return None;
    }
    let MirTerminatorKind::Goto(condition_target) = entry.terminator.kind else {
        return None;
    };
    if condition_target != condition.id || !condition.statements.iter().all(is_temp_assignment) {
        return None;
    }
    let MirTerminatorKind::Branch {
        then_block: loop_body,
        else_block,
        ..
    } = condition.terminator.kind
    else {
        return None;
    };
    if loop_body != body.id || else_block != exit.id {
        return None;
    }
    if !body_branches_to_loop_control(body, then_block.id, continuation.id) {
        return None;
    }
    if !body_updates_loop_local(then_block, loop_local)
        || !matches!(then_block.terminator.kind, MirTerminatorKind::Goto(target) if target == continuation.id)
    {
        return None;
    }
    if !body_updates_loop_local(continuation, loop_local)
        || !matches!(continuation.terminator.kind, MirTerminatorKind::Goto(target) if target == condition.id)
    {
        return None;
    }
    if !exit.statements.is_empty()
        || !matches!(
            &exit.terminator.kind,
            MirTerminatorKind::Return(Some(MirOperand::Place(place)))
                if is_local_place(place, loop_local)
        )
    {
        return None;
    }

    Some(StructuredNestedScalarLoop {
        local: loop_local,
        condition_block: condition.id,
        body_block: body.id,
        exit_block: exit.id,
        then_block: then_block.id,
        continuation_block: continuation.id,
    })
}

pub fn structured_nested_indexed_write_loop_branch(
    function: &MirFunction,
) -> Option<StructuredNestedScalarLoop> {
    let [entry, condition, body, exit, then_block, continuation] = function.blocks.as_slice()
    else {
        return None;
    };
    let terminals = indexed_kernel_terminals(function)?;
    let loop_local = entry_loop_local(entry, terminals.index)?;
    if !function
        .locals
        .iter()
        .any(|local| local.id == loop_local && local.mutable)
    {
        return None;
    }
    let MirTerminatorKind::Goto(condition_target) = entry.terminator.kind else {
        return None;
    };
    if condition_target != condition.id || !condition.statements.iter().all(is_temp_assignment) {
        return None;
    }
    let MirTerminatorKind::Branch {
        then_block: loop_body,
        else_block,
        ..
    } = condition.terminator.kind
    else {
        return None;
    };
    if loop_body != body.id || else_block != exit.id {
        return None;
    }
    if !body_branches_to_loop_control(body, then_block.id, continuation.id) {
        return None;
    }
    if !body_updates_loop_local(then_block, loop_local)
        || !matches!(then_block.terminator.kind, MirTerminatorKind::Goto(target) if target == continuation.id)
    {
        return None;
    }
    if !body_writes_output_then_updates_loop_local(
        continuation,
        terminals.output,
        terminals.index,
        loop_local,
    ) || !matches!(continuation.terminator.kind, MirTerminatorKind::Goto(target) if target == condition.id)
    {
        return None;
    }
    if !exit.statements.is_empty()
        || !matches!(exit.terminator.kind, MirTerminatorKind::Return(None))
    {
        return None;
    }

    Some(StructuredNestedScalarLoop {
        local: loop_local,
        condition_block: condition.id,
        body_block: body.id,
        exit_block: exit.id,
        then_block: then_block.id,
        continuation_block: continuation.id,
    })
}

fn structured_nested_indexed_write_loop_control(
    function: &MirFunction,
    control_kind: LoopControlKind,
) -> Option<StructuredNestedLoopControl> {
    let [
        entry,
        condition,
        body,
        exit,
        nested_branch,
        continuation,
        control_block,
        nested_update,
    ] = function.blocks.as_slice()
    else {
        return None;
    };
    let terminals = indexed_kernel_terminals(function)?;
    let loop_local = entry_loop_local(entry, terminals.index)?;
    if !function
        .locals
        .iter()
        .any(|local| local.id == loop_local && local.mutable)
    {
        return None;
    }
    let MirTerminatorKind::Goto(condition_target) = entry.terminator.kind else {
        return None;
    };
    if condition_target != condition.id || !condition.statements.iter().all(is_temp_assignment) {
        return None;
    }
    let MirTerminatorKind::Branch {
        then_block: loop_body,
        else_block,
        ..
    } = condition.terminator.kind
    else {
        return None;
    };
    if loop_body != body.id || else_block != exit.id {
        return None;
    }
    if !body_branches_to_loop_control(body, nested_branch.id, continuation.id) {
        return None;
    }
    if !body_branches_to_loop_control(nested_branch, control_block.id, nested_update.id) {
        return None;
    }
    let control_target = match control_kind {
        LoopControlKind::Break => exit.id,
        LoopControlKind::Continue => condition.id,
    };
    if !control_block.statements.is_empty()
        || !matches!(control_block.terminator.kind, MirTerminatorKind::Goto(target) if target == control_target)
    {
        return None;
    }
    if !body_updates_loop_local(nested_update, loop_local)
        || !matches!(nested_update.terminator.kind, MirTerminatorKind::Goto(target) if target == continuation.id)
    {
        return None;
    }
    if !body_writes_output_then_updates_loop_local(
        continuation,
        terminals.output,
        terminals.index,
        loop_local,
    ) || !matches!(continuation.terminator.kind, MirTerminatorKind::Goto(target) if target == condition.id)
    {
        return None;
    }
    if !exit.statements.is_empty()
        || !matches!(exit.terminator.kind, MirTerminatorKind::Return(None))
    {
        return None;
    }

    Some(StructuredNestedLoopControl {
        local: loop_local,
        condition_block: condition.id,
        body_block: body.id,
        exit_block: exit.id,
        nested_branch_block: nested_branch.id,
        control_block: control_block.id,
        nested_update_block: nested_update.id,
        continuation_block: continuation.id,
    })
}

fn structured_scalar_loop_control(
    function: &MirFunction,
    control_kind: LoopControlKind,
) -> Option<StructuredLoopControl> {
    let entry = function.blocks.first()?;
    let [param] = function.params.as_slice() else {
        return None;
    };
    let mut loop_blocks = BTreeSet::new();
    let loop_local = entry_loop_local(entry, param.local)?;
    if !function
        .locals
        .iter()
        .any(|local| local.id == loop_local && local.mutable)
    {
        return None;
    }
    let condition = loop_entry_condition_target(function, entry, &mut loop_blocks)?;
    if !condition.statements.iter().all(is_temp_assignment) {
        return None;
    }
    let MirTerminatorKind::Branch {
        then_block,
        else_block,
        ..
    } = condition.terminator.kind
    else {
        return None;
    };
    let body = function.block_by_id(then_block)?;
    let exit = function.block_by_id(else_block)?;
    loop_blocks.insert(body.id);
    loop_blocks.insert(exit.id);
    let MirTerminatorKind::Branch {
        then_block: control_block_id,
        else_block: continuation_id,
        ..
    } = body.terminator.kind
    else {
        return None;
    };
    let control_block = function.block_by_id(control_block_id)?;
    let continuation = function.block_by_id(continuation_id)?;
    loop_blocks.insert(control_block.id);
    loop_blocks.insert(continuation.id);
    if !body_branches_to_loop_control(body, control_block.id, continuation.id) {
        return None;
    }
    let control_target = match control_kind {
        LoopControlKind::Break => exit.id,
        LoopControlKind::Continue => condition.id,
    };
    if !control_block.statements.is_empty()
        || !matches!(control_block.terminator.kind, MirTerminatorKind::Goto(target) if target == control_target)
    {
        return None;
    }
    if !body_updates_loop_local(continuation, loop_local)
        || !matches!(continuation.terminator.kind, MirTerminatorKind::Goto(target) if target == condition.id)
    {
        return None;
    }
    if !exit.statements.is_empty()
        || !matches!(
            &exit.terminator.kind,
            MirTerminatorKind::Return(Some(MirOperand::Place(place)))
                if is_local_place(place, loop_local)
        )
    {
        return None;
    }
    if !function
        .blocks
        .iter()
        .all(|block| loop_blocks.contains(&block.id) || is_empty_unreachable_block(block))
    {
        return None;
    }

    Some(StructuredLoopControl {
        local: loop_local,
        condition_block: condition.id,
        body_block: body.id,
        exit_block: exit.id,
        control_block: control_block.id,
        continuation_block: continuation.id,
    })
}

fn structured_indexed_write_loop_control(
    function: &MirFunction,
    control_kind: LoopControlKind,
) -> Option<StructuredLoopControl> {
    let entry = function.blocks.first()?;
    let terminals = indexed_kernel_terminals(function)?;
    let mut loop_blocks = BTreeSet::new();
    let loop_local = entry_loop_local(entry, terminals.index)?;
    if !function
        .locals
        .iter()
        .any(|local| local.id == loop_local && local.mutable)
    {
        return None;
    }
    let condition = loop_entry_condition_target(function, entry, &mut loop_blocks)?;
    if !condition.statements.iter().all(is_temp_assignment) {
        return None;
    }
    let MirTerminatorKind::Branch {
        then_block,
        else_block,
        ..
    } = condition.terminator.kind
    else {
        return None;
    };
    let body = function.block_by_id(then_block)?;
    let exit = function.block_by_id(else_block)?;
    loop_blocks.insert(body.id);
    loop_blocks.insert(exit.id);
    let MirTerminatorKind::Branch {
        then_block: control_block_id,
        else_block: continuation_id,
        ..
    } = body.terminator.kind
    else {
        return None;
    };
    let control_block = function.block_by_id(control_block_id)?;
    let continuation = function.block_by_id(continuation_id)?;
    loop_blocks.insert(control_block.id);
    loop_blocks.insert(continuation.id);
    if !body_branches_to_loop_control(body, control_block.id, continuation.id) {
        return None;
    }
    let control_target = match control_kind {
        LoopControlKind::Break => exit.id,
        LoopControlKind::Continue => condition.id,
    };
    if !control_block.statements.is_empty()
        || !matches!(control_block.terminator.kind, MirTerminatorKind::Goto(target) if target == control_target)
    {
        return None;
    }
    if !body_writes_output_and_updates_loop_local(
        continuation,
        terminals.output,
        terminals.index,
        loop_local,
    ) || !matches!(continuation.terminator.kind, MirTerminatorKind::Goto(target) if target == condition.id)
    {
        return None;
    }
    if !exit.statements.is_empty()
        || !matches!(exit.terminator.kind, MirTerminatorKind::Return(None))
    {
        return None;
    }
    if !function
        .blocks
        .iter()
        .all(|block| loop_blocks.contains(&block.id) || is_empty_unreachable_block(block))
    {
        return None;
    }

    Some(StructuredLoopControl {
        local: loop_local,
        condition_block: condition.id,
        body_block: body.id,
        exit_block: exit.id,
        control_block: control_block.id,
        continuation_block: continuation.id,
    })
}

pub fn structured_indexed_write_loop(function: &MirFunction) -> Option<StructuredScalarLoop> {
    let entry = function.blocks.first()?;
    let terminals = indexed_kernel_terminals(function)?;
    let mut loop_blocks = BTreeSet::new();
    let loop_local = entry_loop_local(entry, terminals.index)?;
    if !function
        .locals
        .iter()
        .any(|local| local.id == loop_local && local.mutable)
    {
        return None;
    }
    let condition = loop_entry_condition_target(function, entry, &mut loop_blocks)?;
    if !condition.statements.iter().all(is_temp_assignment) {
        return None;
    }
    let MirTerminatorKind::Branch {
        then_block,
        else_block,
        ..
    } = condition.terminator.kind
    else {
        return None;
    };
    let body = function.block_by_id(then_block)?;
    let exit = function.block_by_id(else_block)?;
    loop_blocks.insert(body.id);
    loop_blocks.insert(exit.id);
    if !body_writes_output_and_updates_loop_local(
        body,
        terminals.output,
        terminals.index,
        loop_local,
    ) || !matches!(body.terminator.kind, MirTerminatorKind::Goto(target) if target == condition.id)
    {
        return None;
    }
    if !exit.statements.is_empty()
        || !matches!(exit.terminator.kind, MirTerminatorKind::Return(None))
    {
        return None;
    }
    if !function
        .blocks
        .iter()
        .all(|block| loop_blocks.contains(&block.id) || is_empty_unreachable_block(block))
    {
        return None;
    }

    Some(StructuredScalarLoop {
        local: loop_local,
        condition_block: condition.id,
        body_block: body.id,
        exit_block: exit.id,
    })
}

pub fn structured_scalar_loop(function: &MirFunction) -> Option<StructuredScalarLoop> {
    let entry = function.blocks.first()?;
    let [param] = function.params.as_slice() else {
        return None;
    };
    let mut loop_blocks = BTreeSet::new();
    let loop_local = entry_loop_local(entry, param.local)?;
    if !function
        .locals
        .iter()
        .any(|local| local.id == loop_local && local.mutable)
    {
        return None;
    }
    let condition = loop_entry_condition_target(function, entry, &mut loop_blocks)?;
    if !condition.statements.iter().all(is_temp_assignment) {
        return None;
    }
    let MirTerminatorKind::Branch {
        then_block,
        else_block,
        ..
    } = condition.terminator.kind
    else {
        return None;
    };
    let body = function.block_by_id(then_block)?;
    let exit = function.block_by_id(else_block)?;
    loop_blocks.insert(body.id);
    loop_blocks.insert(exit.id);
    if !body_updates_loop_local(body, loop_local)
        || !matches!(body.terminator.kind, MirTerminatorKind::Goto(target) if target == condition.id)
    {
        return None;
    }
    if !exit.statements.is_empty()
        || !matches!(
            &exit.terminator.kind,
            MirTerminatorKind::Return(Some(MirOperand::Place(place)))
                if is_local_place(place, loop_local)
        )
    {
        return None;
    }
    if !function
        .blocks
        .iter()
        .all(|block| loop_blocks.contains(&block.id) || is_empty_unreachable_block(block))
    {
        return None;
    }

    Some(StructuredScalarLoop {
        local: loop_local,
        condition_block: condition.id,
        body_block: body.id,
        exit_block: exit.id,
    })
}

fn loop_entry_condition_target<'a>(
    function: &'a MirFunction,
    entry: &MirBlock,
    loop_blocks: &mut BTreeSet<super::MirBlockId>,
) -> Option<&'a MirBlock> {
    loop_blocks.insert(entry.id);
    let MirTerminatorKind::Goto(mut target_id) = entry.terminator.kind else {
        return None;
    };
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(target_id) {
            return None;
        }
        let target = function.block_by_id(target_id)?;
        loop_blocks.insert(target.id);
        match target.terminator.kind {
            MirTerminatorKind::Goto(next) if target.statements.is_empty() => target_id = next,
            _ => return Some(target),
        }
    }
}

fn is_empty_unreachable_block(block: &MirBlock) -> bool {
    block.statements.is_empty() && matches!(block.terminator.kind, MirTerminatorKind::Unreachable)
}

fn indexed_output_write_target<'a>(
    function: &'a MirFunction,
    block_id: super::MirBlockId,
    output: MirLocalId,
    index: MirLocalId,
    switch_blocks: &mut BTreeSet<super::MirBlockId>,
) -> Option<&'a MirBlock> {
    let mut block = function.block_by_id(block_id)?;
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(block.id) {
            return None;
        }
        switch_blocks.insert(block.id);
        match block.terminator.kind {
            MirTerminatorKind::Goto(target) if block.statements.is_empty() => {
                block = function.block_by_id(target)?;
            }
            MirTerminatorKind::Goto(_) if block_writes_output(block, output, index) => {
                return Some(block);
            }
            _ => return None,
        }
    }
}

fn block_writes_output(block: &MirBlock, output: MirLocalId, index: MirLocalId) -> bool {
    let mut wrote_output = false;
    for statement in &block.statements {
        match &statement.kind {
            MirStatementKind::Assign { place, .. } if !wrote_output && is_temp_place(place) => {}
            MirStatementKind::Assign { place, .. }
                if !wrote_output && is_output_index_place(place, output, index) =>
            {
                wrote_output = true;
            }
            _ => return false,
        }
    }
    wrote_output
}

fn shared_output_join<'a>(
    function: &'a MirFunction,
    targets: &[super::MirBlockId],
) -> Option<&'a MirBlock> {
    let mut join = None;
    for target in targets {
        let block = function.block_by_id(*target)?;
        let MirTerminatorKind::Goto(target_join) = block.terminator.kind else {
            return None;
        };
        match join {
            Some(existing) if existing != target_join => return None,
            Some(_) => {}
            None => join = Some(target_join),
        }
    }
    let join = join?;
    function.block_by_id(join)
}

fn branch_arm_assigns_local(block: &MirBlock, local: MirLocalId, join: super::MirBlockId) -> bool {
    if !matches!(&block.terminator.kind, MirTerminatorKind::Goto(target) if *target == join) {
        return false;
    }

    let mut assigned_join_local = false;
    for statement in &block.statements {
        match &statement.kind {
            MirStatementKind::Assign { place, .. } if local_place_id(place) == Some(local) => {
                if assigned_join_local {
                    return false;
                }
                assigned_join_local = true;
            }
            MirStatementKind::Assign { place, .. }
                if !assigned_join_local
                    && matches!(place.base, MirPlaceBase::Temp(_))
                    && place.projections.is_empty() => {}
            _ => return false,
        }
    }

    assigned_join_local
}

fn join_writes_output_from_local(
    block: &MirBlock,
    output: MirLocalId,
    index: MirLocalId,
) -> Option<MirLocalId> {
    if !matches!(block.terminator.kind, MirTerminatorKind::Return(None)) {
        return None;
    }
    let [statement] = block.statements.as_slice() else {
        return None;
    };
    let MirStatementKind::Assign { place, value } = &statement.kind else {
        return None;
    };
    if !is_output_index_place(place, output, index) {
        return None;
    }
    let MirValueKind::Operand(MirOperand::Place(value_place)) = &value.kind else {
        return None;
    };
    local_place_id(value_place)
}

fn entry_loop_local(entry: &MirBlock, param: MirLocalId) -> Option<MirLocalId> {
    let mut assigned_local = None;
    for statement in &entry.statements {
        match &statement.kind {
            MirStatementKind::Assign { place, .. } if is_temp_place(place) => {}
            MirStatementKind::Assign { place, .. } => {
                let MirPlaceBase::Local(local) = place.base else {
                    return None;
                };
                if local == param || !place.projections.is_empty() {
                    return None;
                }
                if assigned_local.is_some() {
                    return None;
                }
                assigned_local = Some(local);
            }
            _ => return None,
        }
    }
    assigned_local
}

fn body_updates_loop_local(body: &MirBlock, loop_local: MirLocalId) -> bool {
    let mut updates_loop_local = false;
    for statement in &body.statements {
        match &statement.kind {
            MirStatementKind::Assign { place, .. } if is_temp_place(place) => {}
            MirStatementKind::Assign { place, .. }
                if !updates_loop_local && is_local_place(place, loop_local) =>
            {
                updates_loop_local = true;
            }
            _ => return false,
        }
    }
    updates_loop_local
}

fn body_branches_to_loop_control(
    body: &MirBlock,
    control_block: super::MirBlockId,
    continuation: super::MirBlockId,
) -> bool {
    body.statements.iter().all(is_temp_assignment)
        && matches!(
            body.terminator.kind,
            MirTerminatorKind::Branch { then_block, else_block, .. }
                if then_block == control_block && else_block == continuation
        )
}

fn body_writes_output_and_updates_loop_local(
    body: &MirBlock,
    output: MirLocalId,
    index: MirLocalId,
    loop_local: MirLocalId,
) -> bool {
    let mut wrote_output = false;
    let mut updated_loop_local = false;
    for statement in &body.statements {
        match &statement.kind {
            MirStatementKind::Assign { place, .. } if is_temp_place(place) => {}
            MirStatementKind::Assign { place, .. }
                if !wrote_output && is_output_index_place(place, output, index) =>
            {
                wrote_output = true;
            }
            MirStatementKind::Assign { place, .. }
                if !updated_loop_local && is_local_place(place, loop_local) =>
            {
                updated_loop_local = true;
            }
            _ => return false,
        }
    }
    wrote_output && updated_loop_local
}

fn body_writes_output_then_updates_loop_local(
    body: &MirBlock,
    output: MirLocalId,
    index: MirLocalId,
    loop_local: MirLocalId,
) -> bool {
    let mut wrote_output = false;
    let mut updated_loop_local = false;
    for statement in &body.statements {
        match &statement.kind {
            MirStatementKind::Assign { place, .. } if is_temp_place(place) => {}
            MirStatementKind::Assign { place, .. }
                if !wrote_output && is_output_index_place(place, output, index) =>
            {
                wrote_output = true;
            }
            MirStatementKind::Assign { place, .. }
                if wrote_output && !updated_loop_local && is_local_place(place, loop_local) =>
            {
                updated_loop_local = true;
            }
            _ => return false,
        }
    }
    wrote_output && updated_loop_local
}

fn is_temp_assignment(statement: &super::MirStatement) -> bool {
    matches!(&statement.kind, MirStatementKind::Assign { place, .. } if is_temp_place(place))
}

fn is_temp_place(place: &MirPlace) -> bool {
    matches!(place.base, MirPlaceBase::Temp(_)) && place.projections.is_empty()
}

fn is_local_place(place: &MirPlace, local: MirLocalId) -> bool {
    place.base == MirPlaceBase::Local(local) && place.projections.is_empty()
}

fn is_output_index_place(place: &MirPlace, output: MirLocalId, index: MirLocalId) -> bool {
    place.base == MirPlaceBase::Local(output)
        && matches!(
            place.projections.as_slice(),
            [MirProjection::Index(MirOperand::Place(index_place)) | MirProjection::ProvenIndex(MirOperand::Place(index_place))]
                if index_place.base == MirPlaceBase::Local(index) && index_place.projections.is_empty()
        )
}

fn local_place_id(place: &MirPlace) -> Option<MirLocalId> {
    match place.base {
        MirPlaceBase::Local(local) if place.projections.is_empty() => Some(local),
        _ => None,
    }
}

// Deliberate: `MirConstant::Magnus` (an `inf` beyond the 64-bit carriers) is
// not a structured-kernel switch case: `inf` is host-only (ruling 39).
fn is_switch_case_constant(constant: &MirConstant) -> bool {
    matches!(
        constant,
        MirConstant::Bool(_) | MirConstant::Int(_) | MirConstant::UInt(_)
    )
}
