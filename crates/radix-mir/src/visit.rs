//! Read-only MIR visitor traits plus CFG edge helpers.
//!
//! WHY: MIR is stored as functions containing ordered blocks, statements, and
//! terminators. Structural walks should follow that storage order; CFG walks
//! should use explicit successor helpers so loops and shared targets do not
//! cause accidental recursive revisits.
//!
//! TRAVERSAL CONTRACT
//! ==================
//! Visitors walk the tree-shaped storage view, not the control-flow graph.
//! `visit_function` visits params, locals, temps, then blocks in vector order;
//! `visit_block` visits statements before the terminator. Branch targets are
//! not recursively followed by terminator visitors. CFG consumers should call
//! `terminator_successors` and manage reachability, deduplication, and worklists
//! themselves.

use super::nodes::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirBlock, MirBlockId, MirCallee,
    MirFunction, MirIntrinsic, MirLocal, MirOperand, MirOptionChainLink, MirOptionOp, MirParam,
    MirPlace, MirProgram, MirProjection, MirRuntimeCall, MirStatement, MirStatementKind, MirTemp,
    MirTerminator, MirTerminatorKind, MirValue, MirValueKind,
};

/// Infallible read-only visitor over MIR storage order.
///
/// Override the methods where a pass needs to observe a node. The default
/// methods walk children but do not maintain CFG state or revisit target blocks.
pub trait MirVisitor: Sized {
    fn visit_program(&mut self, program: &MirProgram) {
        walk_program(self, program);
    }

    fn visit_function(&mut self, function: &MirFunction) {
        walk_function(self, function);
    }

    fn visit_param(&mut self, _param: &MirParam) {}

    fn visit_local(&mut self, _local: &MirLocal) {}

    fn visit_temp(&mut self, _temp: &MirTemp) {}

    fn visit_block(&mut self, block: &MirBlock) {
        walk_block(self, block);
    }

    fn visit_stmt(&mut self, stmt: &MirStatement) {
        walk_stmt(self, stmt);
    }

    fn visit_terminator(&mut self, terminator: &MirTerminator) {
        walk_terminator(self, terminator);
    }

    fn visit_value(&mut self, value: &MirValue) {
        walk_value(self, value);
    }

    fn visit_operand(&mut self, operand: &MirOperand) {
        walk_operand(self, operand);
    }

    fn visit_place(&mut self, place: &MirPlace) {
        walk_place(self, place);
    }

    fn visit_callee(&mut self, callee: &MirCallee) {
        walk_callee(self, callee);
    }

    fn visit_runtime_call(&mut self, call: &MirRuntimeCall) {
        walk_runtime_call(self, call);
    }

    fn visit_aggregate(&mut self, aggregate: &MirAggregate) {
        walk_aggregate(self, aggregate);
    }

    fn visit_option_op(&mut self, op: &MirOptionOp) {
        walk_option_op(self, op);
    }

    fn visit_option_chain_link(&mut self, link: &MirOptionChainLink) {
        walk_option_chain_link(self, link);
    }
}

/// Fallible read-only visitor over MIR storage order.
///
/// The first returned error aborts traversal. This is useful for probes or
/// emitters that stop at the first unsupported MIR shape, while validators that
/// collect many diagnostics should usually keep their own error vector instead.
pub trait FallibleMirVisitor: Sized {
    type Error;

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_program(&mut self, program: &MirProgram) -> Result<(), Self::Error> {
        try_walk_program(self, program)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_function(&mut self, function: &MirFunction) -> Result<(), Self::Error> {
        try_walk_function(self, function)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_param(&mut self, _param: &MirParam) -> Result<(), Self::Error> {
        Ok(())
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_local(&mut self, _local: &MirLocal) -> Result<(), Self::Error> {
        Ok(())
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_temp(&mut self, _temp: &MirTemp) -> Result<(), Self::Error> {
        Ok(())
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_block(&mut self, block: &MirBlock) -> Result<(), Self::Error> {
        try_walk_block(self, block)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_stmt(&mut self, stmt: &MirStatement) -> Result<(), Self::Error> {
        try_walk_stmt(self, stmt)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_terminator(&mut self, terminator: &MirTerminator) -> Result<(), Self::Error> {
        try_walk_terminator(self, terminator)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_value(&mut self, value: &MirValue) -> Result<(), Self::Error> {
        try_walk_value(self, value)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_operand(&mut self, operand: &MirOperand) -> Result<(), Self::Error> {
        try_walk_operand(self, operand)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_place(&mut self, place: &MirPlace) -> Result<(), Self::Error> {
        try_walk_place(self, place)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_callee(&mut self, callee: &MirCallee) -> Result<(), Self::Error> {
        try_walk_callee(self, callee)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_runtime_call(&mut self, call: &MirRuntimeCall) -> Result<(), Self::Error> {
        try_walk_runtime_call(self, call)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_aggregate(&mut self, aggregate: &MirAggregate) -> Result<(), Self::Error> {
        try_walk_aggregate(self, aggregate)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_option_op(&mut self, op: &MirOptionOp) -> Result<(), Self::Error> {
        try_walk_option_op(self, op)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`Self::Error`].
    fn visit_option_chain_link(&mut self, link: &MirOptionChainLink) -> Result<(), Self::Error> {
        try_walk_option_chain_link(self, link)
    }
}

/// Walk all functions in program storage order.
pub fn walk_program<V: MirVisitor>(visitor: &mut V, program: &MirProgram) {
    for function in &program.functions {
        visitor.visit_function(function);
    }
}

/// Fallibly walk all functions in program storage order.
/// # Errors
/// Returns an error when validation or type resolution fails. See [`V::Error`].
pub fn try_walk_program<V: FallibleMirVisitor>(
    visitor: &mut V,
    program: &MirProgram,
) -> Result<(), V::Error> {
    for function in &program.functions {
        visitor.visit_function(function)?;
    }
    Ok(())
}

/// Walk a function's declarations and blocks in deterministic storage order.
pub fn walk_function<V: MirVisitor>(visitor: &mut V, function: &MirFunction) {
    for param in &function.params {
        visitor.visit_param(param);
    }
    for local in &function.locals {
        visitor.visit_local(local);
    }
    for temp in &function.temps {
        visitor.visit_temp(temp);
    }
    for block in &function.blocks {
        visitor.visit_block(block);
    }
}

/// Fallibly walk a function's declarations and blocks in deterministic storage order.
/// # Errors
/// Returns an error when validation or type resolution fails. See [`V::Error`].
pub fn try_walk_function<V: FallibleMirVisitor>(
    visitor: &mut V,
    function: &MirFunction,
) -> Result<(), V::Error> {
    for param in &function.params {
        visitor.visit_param(param)?;
    }
    for local in &function.locals {
        visitor.visit_local(local)?;
    }
    for temp in &function.temps {
        visitor.visit_temp(temp)?;
    }
    for block in &function.blocks {
        visitor.visit_block(block)?;
    }
    Ok(())
}

/// Walk straight-line statements before the block terminator.
pub fn walk_block<V: MirVisitor>(visitor: &mut V, block: &MirBlock) {
    for stmt in &block.statements {
        visitor.visit_stmt(stmt);
    }
    visitor.visit_terminator(&block.terminator);
}

/// Fallibly walk straight-line statements before the block terminator.
/// # Errors
/// Returns an error when validation or type resolution fails. See [`V::Error`].
pub fn try_walk_block<V: FallibleMirVisitor>(
    visitor: &mut V,
    block: &MirBlock,
) -> Result<(), V::Error> {
    for stmt in &block.statements {
        visitor.visit_stmt(stmt)?;
    }
    visitor.visit_terminator(&block.terminator)
}

/// Walk the children of one statement without crossing block boundaries.
pub fn walk_stmt<V: MirVisitor>(visitor: &mut V, stmt: &MirStatement) {
    match &stmt.kind {
        MirStatementKind::Assign { place, value } => {
            visitor.visit_place(place);
            visitor.visit_value(value);
        }
        MirStatementKind::Call {
            destination,
            callee,
            args,
        } => {
            if let Some(destination) = destination {
                visitor.visit_place(destination);
            }
            visitor.visit_callee(callee);
            for arg in args {
                visitor.visit_operand(arg);
            }
        }
        MirStatementKind::RuntimeCall { destination, call } => {
            if let Some(destination) = destination {
                visitor.visit_place(destination);
            }
            visitor.visit_runtime_call(call);
        }
        MirStatementKind::Construct {
            destination,
            aggregate,
        } => {
            visitor.visit_place(destination);
            visitor.visit_aggregate(aggregate);
        }
    }
}

/// Fallibly walk the children of one statement without crossing block boundaries.
/// # Errors
/// Returns an error when validation or type resolution fails. See [`V::Error`].
pub fn try_walk_stmt<V: FallibleMirVisitor>(
    visitor: &mut V,
    stmt: &MirStatement,
) -> Result<(), V::Error> {
    match &stmt.kind {
        MirStatementKind::Assign { place, value } => {
            visitor.visit_place(place)?;
            visitor.visit_value(value)
        }
        MirStatementKind::Call {
            destination,
            callee,
            args,
        } => {
            if let Some(destination) = destination {
                visitor.visit_place(destination)?;
            }
            visitor.visit_callee(callee)?;
            for arg in args {
                visitor.visit_operand(arg)?;
            }
            Ok(())
        }
        MirStatementKind::RuntimeCall { destination, call } => {
            if let Some(destination) = destination {
                visitor.visit_place(destination)?;
            }
            visitor.visit_runtime_call(call)
        }
        MirStatementKind::Construct {
            destination,
            aggregate,
        } => {
            visitor.visit_place(destination)?;
            visitor.visit_aggregate(aggregate)
        }
    }
}

/// Walk operands and places owned by a terminator, but not successor blocks.
pub fn walk_terminator<V: MirVisitor>(visitor: &mut V, terminator: &MirTerminator) {
    match &terminator.kind {
        MirTerminatorKind::Return(Some(value))
        | MirTerminatorKind::ReturnError(value)
        | MirTerminatorKind::Switch { value, .. } => {
            visitor.visit_operand(value);
        }
        MirTerminatorKind::TryCall {
            destination,
            callee,
            args,
            error_place,
            ..
        } => {
            if let Some(destination) = destination {
                visitor.visit_place(destination);
            }
            visitor.visit_callee(callee);
            for arg in args {
                visitor.visit_operand(arg);
            }
            visitor.visit_place(error_place);
        }
        MirTerminatorKind::Branch { condition, .. } => {
            visitor.visit_operand(condition);
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

/// Fallibly walk operands and places owned by a terminator, but not successor blocks.
/// # Errors
/// Returns an error when validation or type resolution fails. See [`V::Error`].
pub fn try_walk_terminator<V: FallibleMirVisitor>(
    visitor: &mut V,
    terminator: &MirTerminator,
) -> Result<(), V::Error> {
    match &terminator.kind {
        MirTerminatorKind::Return(Some(value)) | MirTerminatorKind::ReturnError(value) => {
            visitor.visit_operand(value)
        }
        MirTerminatorKind::TryCall {
            destination,
            callee,
            args,
            error_place,
            ..
        } => {
            if let Some(destination) = destination {
                visitor.visit_place(destination)?;
            }
            visitor.visit_callee(callee)?;
            for arg in args {
                visitor.visit_operand(arg)?;
            }
            visitor.visit_place(error_place)
        }
        MirTerminatorKind::Branch { condition, .. } => visitor.visit_operand(condition),
        MirTerminatorKind::Switch { value, .. } => visitor.visit_operand(value),
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => Ok(()),
    }
}

pub fn walk_value<V: MirVisitor>(visitor: &mut V, value: &MirValue) {
    match &value.kind {
        MirValueKind::Operand(operand) | MirValueKind::Unary { operand, .. } => {
            visitor.visit_operand(operand);
        }
        MirValueKind::Closure(closure) => visitor.visit_operand(&closure.environment),
        MirValueKind::TensorExtent { tensor, .. } => visitor.visit_operand(tensor),
        MirValueKind::Binary { lhs, rhs, .. } => {
            visitor.visit_operand(lhs);
            visitor.visit_operand(rhs);
        }
        MirValueKind::Option(op) => visitor.visit_option_op(op),
    }
}

/// # Errors
/// Returns an error when validation or type resolution fails. See [`V::Error`].
pub fn try_walk_value<V: FallibleMirVisitor>(
    visitor: &mut V,
    value: &MirValue,
) -> Result<(), V::Error> {
    match &value.kind {
        MirValueKind::Operand(operand) | MirValueKind::Unary { operand, .. } => {
            visitor.visit_operand(operand)
        }
        MirValueKind::Closure(closure) => visitor.visit_operand(&closure.environment),
        MirValueKind::TensorExtent { tensor, .. } => visitor.visit_operand(tensor),
        MirValueKind::Binary { lhs, rhs, .. } => {
            visitor.visit_operand(lhs)?;
            visitor.visit_operand(rhs)
        }
        MirValueKind::Option(op) => visitor.visit_option_op(op),
    }
}

pub fn walk_operand<V: MirVisitor>(visitor: &mut V, operand: &MirOperand) {
    if let MirOperand::Place(place) = operand {
        visitor.visit_place(place);
    }
}

/// # Errors
/// Returns an error when validation or type resolution fails. See [`V::Error`].
pub fn try_walk_operand<V: FallibleMirVisitor>(
    visitor: &mut V,
    operand: &MirOperand,
) -> Result<(), V::Error> {
    if let MirOperand::Place(place) = operand {
        visitor.visit_place(place)?;
    }
    Ok(())
}

pub fn walk_place<V: MirVisitor>(visitor: &mut V, place: &MirPlace) {
    for projection in &place.projections {
        if let MirProjection::Index(index) | MirProjection::ProvenIndex(index) = projection {
            visitor.visit_operand(index);
        }
    }
}

/// # Errors
/// Returns an error when validation or type resolution fails. See [`V::Error`].
pub fn try_walk_place<V: FallibleMirVisitor>(
    visitor: &mut V,
    place: &MirPlace,
) -> Result<(), V::Error> {
    for projection in &place.projections {
        if let MirProjection::Index(index) | MirProjection::ProvenIndex(index) = projection {
            visitor.visit_operand(index)?;
        }
    }
    Ok(())
}

pub fn walk_callee<V: MirVisitor>(visitor: &mut V, callee: &MirCallee) {
    match callee {
        MirCallee::Closure(closure) => visitor.visit_operand(&closure.environment),
        MirCallee::Value(value) => visitor.visit_operand(value),
        MirCallee::Function(_) | MirCallee::Definition { .. } => {}
    }
}

/// # Errors
/// Returns an error when validation or type resolution fails. See [`V::Error`].
pub fn try_walk_callee<V: FallibleMirVisitor>(
    visitor: &mut V,
    callee: &MirCallee,
) -> Result<(), V::Error> {
    match callee {
        MirCallee::Closure(closure) => visitor.visit_operand(&closure.environment)?,
        MirCallee::Value(value) => visitor.visit_operand(value)?,
        MirCallee::Function(_) | MirCallee::Definition { .. } => {}
    }
    Ok(())
}

pub fn walk_runtime_call<V: MirVisitor>(visitor: &mut V, call: &MirRuntimeCall) {
    for arg in &call.args {
        visitor.visit_operand(arg);
    }
    if let MirIntrinsic::Convert(conversion) = &call.intrinsic {
        if let Some(recovery) = &conversion.recovery {
            visitor.visit_operand(recovery);
        }
        for defaults in &conversion.struct_defaults {
            for field in &defaults.fields {
                visitor.visit_operand(&field.value);
            }
        }
    }
}

/// # Errors
/// Returns an error when validation or type resolution fails. See [`V::Error`].
pub fn try_walk_runtime_call<V: FallibleMirVisitor>(
    visitor: &mut V,
    call: &MirRuntimeCall,
) -> Result<(), V::Error> {
    for arg in &call.args {
        visitor.visit_operand(arg)?;
    }
    if let MirIntrinsic::Convert(conversion) = &call.intrinsic {
        if let Some(recovery) = &conversion.recovery {
            visitor.visit_operand(recovery)?;
        }
        for defaults in &conversion.struct_defaults {
            for field in &defaults.fields {
                visitor.visit_operand(&field.value)?;
            }
        }
    }
    Ok(())
}

pub fn walk_aggregate<V: MirVisitor>(visitor: &mut V, aggregate: &MirAggregate) {
    match &aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(value) | MirAggregateItem::Spread(value) => {
                        visitor.visit_operand(value);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                visitor.visit_operand(&item.value);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                visitor.visit_operand(&item.key);
                visitor.visit_operand(&item.value);
            }
        }
    }
}

/// # Errors
/// Returns an error when validation or type resolution fails. See [`V::Error`].
pub fn try_walk_aggregate<V: FallibleMirVisitor>(
    visitor: &mut V,
    aggregate: &MirAggregate,
) -> Result<(), V::Error> {
    match &aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(value) | MirAggregateItem::Spread(value) => {
                        visitor.visit_operand(value)?;
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                visitor.visit_operand(&item.value)?;
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                visitor.visit_operand(&item.key)?;
                visitor.visit_operand(&item.value)?;
            }
        }
    }
    Ok(())
}

pub fn walk_option_op<V: MirVisitor>(visitor: &mut V, op: &MirOptionOp) {
    match op {
        MirOptionOp::None => {}
        MirOptionOp::Some(value)
        | MirOptionOp::IsNil(value)
        | MirOptionOp::IsNotNil(value)
        | MirOptionOp::Unwrap { value, .. } => visitor.visit_operand(value),
        MirOptionOp::Coalesce { value, fallback } => {
            visitor.visit_operand(value);
            visitor.visit_operand(fallback);
        }
        MirOptionOp::Chain { base, link } => {
            visitor.visit_operand(base);
            visitor.visit_option_chain_link(link);
        }
    }
}

/// # Errors
/// Returns an error when validation or type resolution fails. See [`V::Error`].
pub fn try_walk_option_op<V: FallibleMirVisitor>(
    visitor: &mut V,
    op: &MirOptionOp,
) -> Result<(), V::Error> {
    match op {
        MirOptionOp::None => Ok(()),
        MirOptionOp::Some(value)
        | MirOptionOp::IsNil(value)
        | MirOptionOp::IsNotNil(value)
        | MirOptionOp::Unwrap { value, .. } => visitor.visit_operand(value),
        MirOptionOp::Coalesce { value, fallback } => {
            visitor.visit_operand(value)?;
            visitor.visit_operand(fallback)
        }
        MirOptionOp::Chain { base, link } => {
            visitor.visit_operand(base)?;
            visitor.visit_option_chain_link(link)
        }
    }
}

pub fn walk_option_chain_link<V: MirVisitor>(visitor: &mut V, link: &MirOptionChainLink) {
    match link {
        MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => {}
        MirOptionChainLink::Index(index) => visitor.visit_operand(index),
        MirOptionChainLink::Call { callee, args } => {
            visitor.visit_callee(callee);
            for arg in args {
                visitor.visit_operand(arg);
            }
        }
    }
}

/// # Errors
/// Returns an error when validation or type resolution fails. See [`V::Error`].
pub fn try_walk_option_chain_link<V: FallibleMirVisitor>(
    visitor: &mut V,
    link: &MirOptionChainLink,
) -> Result<(), V::Error> {
    match link {
        MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => Ok(()),
        MirOptionChainLink::Index(index) => visitor.visit_operand(index),
        MirOptionChainLink::Call { callee, args } => {
            visitor.visit_callee(callee)?;
            for arg in args {
                visitor.visit_operand(arg)?;
            }
            Ok(())
        }
    }
}

/// Return explicit CFG successors for a terminator.
///
/// The order matches the terminator's source order: success then error for
/// `TryCall`, then/else for branches, cases then default for switches.
#[must_use]
pub fn terminator_successors(kind: &MirTerminatorKind) -> Vec<MirBlockId> {
    match kind {
        MirTerminatorKind::TryCall {
            ok_block,
            error_block,
            ..
        } => vec![*ok_block, *error_block],
        MirTerminatorKind::Goto(target) => vec![*target],
        MirTerminatorKind::Branch {
            then_block,
            else_block,
            ..
        } => vec![*then_block, *else_block],
        MirTerminatorKind::Switch { cases, default, .. } => {
            let mut successors = cases.iter().map(|case| case.target).collect::<Vec<_>>();
            successors.push(*default);
            successors
        }
        MirTerminatorKind::Return(_)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::ReturnError(_)
        | MirTerminatorKind::Unreachable => Vec::new(),
    }
}
