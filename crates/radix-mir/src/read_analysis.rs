//! Shared read-analysis helpers for device-program lowering.
//!
//! The read/write analysis of MIR bodies (which locals/temps a statement,
//! terminator, value, or operand reads; whether a place is read anywhere in
//! a function; the pure `Local(l) ← Temp(t)` forward scan) was hand-rolled
//! independently in [`crate::device`], [`crate::static_shape_fold`],
//! [`crate::kernel_decomposition`], and [`crate::device_program_plans`] —
//! four copies of the same traversal with per-file collection mechanics and
//! slightly different coverage. This module is the single implementation.
//!
//! **Coverage policy (superset).** The shared walkers count every read the
//! four consumers originally covered, unioned:
//! - `Call` arguments count as reads (the older `device_program_plans`
//!   predicate returned `false` for `Call` statements — the S3-A5 dead-op
//!   filter only ever ran on device-shaped bodies, but a plain `Call` in a
//!   lane body reads its operands);
//! - any `Local` base counts, whether or not the place carries projections
//!   (the older `device_program_plans` predicate required empty projections);
//! - `Option`/`Closure` values and `Keyed` aggregate keys count
//!   (`kernel_decomposition`'s narrow walker did not descend into them);
//! - terminators cover `Return(Some)`, `ReturnError`, `Branch`, `Switch`,
//!   and `TryCall`.
//! Broader reads are the conservative direction: a destination that reads
//! more is less likely to be skipped as dead, so no value that was kept is
//! ever dropped. The consumers' test suites and the exempla corpus prove the
//! unification is inert for every reachable body.

use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirCallee, MirCollectionOp, MirFunction,
    MirIntrinsic, MirLocalId, MirOperand, MirOptionChainLink, MirOptionOp, MirPlace, MirPlaceBase,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTempId, MirTerminatorKind, MirValue,
    MirValueKind,
};
use rustc_hash::FxHashSet;

/// Every local read anywhere in the function (statements and terminators,
/// across all blocks). Destinations are writes, never reads; an operand's
/// base slot is read whether or not the place carries projections.
pub(crate) fn function_read_locals(function: &MirFunction) -> FxHashSet<MirLocalId> {
    let mut read = FxHashSet::default();
    for block in &function.blocks {
        for statement in &block.statements {
            read.extend(statement_read_locals(statement));
        }
        read.extend(terminator_read_locals(&block.terminator.kind));
    }
    read
}

/// The locals a statement reads (the statement's own destination is a
/// write, never a read).
pub(crate) fn statement_read_locals(statement: &MirStatement) -> Vec<MirLocalId> {
    let mut read = Vec::new();
    match &statement.kind {
        MirStatementKind::Assign { value, .. } => {
            value_read_locals(value, &mut read);
        }
        MirStatementKind::RuntimeCall { call, .. } => {
            read.extend(live_call_args(call).iter().filter_map(operand_local));
        }
        MirStatementKind::Call { args, .. } => {
            read.extend(args.iter().filter_map(operand_local));
        }
        MirStatementKind::Construct { aggregate, .. } => {
            aggregate_read_locals(aggregate, &mut read);
        }
    }
    read
}

/// The locals a terminator reads.
pub(crate) fn terminator_read_locals(terminator: &MirTerminatorKind) -> Vec<MirLocalId> {
    let mut read = Vec::new();
    match terminator {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            read.extend(operand_local(operand));
        }
        MirTerminatorKind::Branch { condition, .. }
        | MirTerminatorKind::Switch {
            value: condition, ..
        } => {
            read.extend(operand_local(condition));
        }
        MirTerminatorKind::TryCall { args, .. } => {
            read.extend(args.iter().filter_map(operand_local));
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
    read
}

/// Whether a statement reads the given local in any operand or value
/// position (the statement's own destination is a write, never a read).
pub(crate) fn statement_reads_local(statement: &MirStatement, local: MirLocalId) -> bool {
    statement_read_locals(statement).contains(&local)
}

/// Whether a terminator reads the given local.
pub(crate) fn terminator_reads_local(terminator: &MirTerminatorKind, local: MirLocalId) -> bool {
    terminator_read_locals(terminator).contains(&local)
}

/// Whether a place's base local is read anywhere in the function (the
/// S3-A5 dead-op filter's read set). A destination that IS a function param
/// (e.g. a matmul writing its output-buffer param) is a side effect — always
/// live.
pub(crate) fn place_is_read(function: &MirFunction, place: &MirPlace) -> bool {
    let MirPlaceBase::Local(local) = place.base else {
        return true;
    };
    if function.params.iter().any(|param| param.local == local) {
        return true;
    }
    function_read_locals(function).contains(&local)
}

/// The local a plain place operand references, if any.
pub(crate) fn operand_local(operand: &MirOperand) -> Option<MirLocalId> {
    let MirOperand::Place(place) = operand else {
        return None;
    };
    match place.base {
        MirPlaceBase::Local(local) => Some(local),
        MirPlaceBase::Temp(_) => None,
    }
}

/// The destination local of a statement, when it writes a local place
/// (temps are not plain local destinations — the decomposition's read/write
/// analysis tracks [`MirPlaceBase::Local`] only).
pub(crate) fn statement_destination(statement: &MirStatement) -> Option<MirLocalId> {
    statement_destination_place(statement).and_then(place_local_base)
}

/// The destination place of a statement, when it writes one — any base
/// slot (the elementwise planner's local/temp value space enumerates temps
/// too). Projection barriers stay the caller's check.
pub(crate) fn statement_destination_place(statement: &MirStatement) -> Option<&MirPlace> {
    match &statement.kind {
        MirStatementKind::Assign { place, .. }
        | MirStatementKind::Construct {
            destination: place, ..
        } => Some(place),
        MirStatementKind::RuntimeCall { destination, .. }
        | MirStatementKind::Call { destination, .. } => destination.as_ref(),
    }
}

/// The unprojected base slot of a place — `None` when the place carries
/// projections (slot enumeration is unprojected-only).
pub(crate) fn place_slot_base(place: &MirPlace) -> Option<MirPlaceBase> {
    if !place.projections.is_empty() {
        return None;
    }
    Some(place.base.clone())
}

/// The base slot an operand reads, when it reads one — a bare temp or a
/// place base, projected or not (the base is read either way, matching
/// [`operand_local`]).
pub(crate) fn operand_slot(operand: &MirOperand) -> Option<MirPlaceBase> {
    match operand {
        MirOperand::Temp(temp) => Some(MirPlaceBase::Temp(*temp)),
        MirOperand::Place(place) => Some(place.base.clone()),
        _ => None,
    }
}

/// The base slots a statement reads, locals and temps (the statement's own
/// destination is a write, never a read). Slot-level mirror of
/// [`statement_read_locals`], with the same exclusion policy: the
/// `TensorFill` shape-receiver first operand is not a live value read.
pub(crate) fn statement_read_slots(statement: &MirStatement) -> Vec<MirPlaceBase> {
    let mut read = Vec::new();
    match &statement.kind {
        MirStatementKind::Assign { value, .. } => {
            value_read_slots(value, &mut read);
        }
        MirStatementKind::RuntimeCall { call, .. } => {
            read.extend(live_call_args(call).iter().filter_map(operand_slot));
        }
        MirStatementKind::Call { args, .. } => {
            read.extend(args.iter().filter_map(operand_slot));
        }
        MirStatementKind::Construct { aggregate, .. } => {
            aggregate_read_slots(aggregate, &mut read);
        }
    }
    read
}

/// The base slots a terminator reads (locals and temps).
pub(crate) fn terminator_read_slots(terminator: &MirTerminatorKind) -> Vec<MirPlaceBase> {
    let mut read = Vec::new();
    match terminator {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            read.extend(operand_slot(operand));
        }
        MirTerminatorKind::Branch { condition, .. }
        | MirTerminatorKind::Switch {
            value: condition, ..
        } => {
            read.extend(operand_slot(condition));
        }
        MirTerminatorKind::TryCall { args, .. } => {
            read.extend(args.iter().filter_map(operand_slot));
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
    read
}

/// The destination local of a place, when it is a local place.
pub(crate) fn place_local_base(place: &MirPlace) -> Option<MirLocalId> {
    match place.base {
        MirPlaceBase::Local(local) => Some(local),
        MirPlaceBase::Temp(_) => None,
    }
}

/// The plain-local operand a function return terminator returns, when it
/// returns one (the D-2a last-subchain output set). Temps and constants are
/// not subchain outputs.
pub(crate) fn terminator_return_local(terminator: &MirTerminatorKind) -> Option<MirLocalId> {
    match terminator {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            operand_local(operand)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Collection helpers (private to the module)
// ---------------------------------------------------------------------------

/// The call arguments that carry live value reads. TensorFill's first
/// operand is a shape receiver only: the device emitters derive the output
/// extent from the destination type and consume only the scalar fill
/// operand, so a folded vacua seed used solely in this position is not a
/// live value. Every read walker (locals and slots) shares this exclusion.
fn live_call_args(call: &MirRuntimeCall) -> &[MirOperand] {
    if matches!(
        call.intrinsic,
        MirIntrinsic::Collection(MirCollectionOp::TensorFill)
    ) {
        call.args.get(1..).unwrap_or_default()
    } else {
        &call.args
    }
}

/// Collect the locals a value reads into `out`.
fn value_read_locals(value: &MirValue, out: &mut Vec<MirLocalId>) {
    match &value.kind {
        MirValueKind::Operand(operand)
        | MirValueKind::Unary { operand, .. }
        | MirValueKind::TensorExtent {
            tensor: operand, ..
        } => {
            out.extend(operand_local(operand));
        }
        MirValueKind::Binary { lhs, rhs, .. } => {
            out.extend(operand_local(lhs));
            out.extend(operand_local(rhs));
        }
        MirValueKind::Option(op) => option_read_locals(op, out),
        MirValueKind::Closure(closure) => out.extend(operand_local(&closure.environment)),
    }
}

/// Collect the base slots a value reads into `out` (slot-level mirror of
/// [`value_read_locals`]).
fn value_read_slots(value: &MirValue, out: &mut Vec<MirPlaceBase>) {
    match &value.kind {
        MirValueKind::Operand(operand)
        | MirValueKind::Unary { operand, .. }
        | MirValueKind::TensorExtent {
            tensor: operand, ..
        } => {
            out.extend(operand_slot(operand));
        }
        MirValueKind::Binary { lhs, rhs, .. } => {
            out.extend(operand_slot(lhs));
            out.extend(operand_slot(rhs));
        }
        MirValueKind::Option(op) => option_read_slots(op, out),
        MirValueKind::Closure(closure) => out.extend(operand_slot(&closure.environment)),
    }
}

/// Collect the locals an option operation reads into `out`.
fn option_read_locals(op: &MirOptionOp, out: &mut Vec<MirLocalId>) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand) => out.extend(operand_local(operand)),
        MirOptionOp::Unwrap { value, .. } => out.extend(operand_local(value)),
        MirOptionOp::Coalesce { value, fallback } => {
            out.extend(operand_local(value));
            out.extend(operand_local(fallback));
        }
        MirOptionOp::Chain { base, .. } => out.extend(operand_local(base)),
        MirOptionOp::None => {}
    }
}

/// Collect the base slots an option operation reads into `out`
/// (slot-level mirror of [`option_read_locals`]).
fn option_read_slots(op: &MirOptionOp, out: &mut Vec<MirPlaceBase>) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand) => out.extend(operand_slot(operand)),
        MirOptionOp::Unwrap { value, .. } => out.extend(operand_slot(value)),
        MirOptionOp::Coalesce { value, fallback } => {
            out.extend(operand_slot(value));
            out.extend(operand_slot(fallback));
        }
        MirOptionOp::Chain { base, .. } => out.extend(operand_slot(base)),
        MirOptionOp::None => {}
    }
}

/// Collect the locals an aggregate construct reads into `out` (its field
/// operands; the destination is a write).
fn aggregate_read_locals(aggregate: &MirAggregate, out: &mut Vec<MirLocalId>) {
    match &aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                let (MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand)) = item;
                out.extend(operand_local(operand));
            }
        }
        MirAggregateFields::Named(fields) => {
            for field in fields {
                out.extend(operand_local(&field.value));
            }
        }
        MirAggregateFields::Keyed(fields) => {
            for field in fields {
                out.extend(operand_local(&field.key));
                out.extend(operand_local(&field.value));
            }
        }
    }
}

/// Collect the base slots an aggregate construct reads into `out`
/// (slot-level mirror of [`aggregate_read_locals`]).
fn aggregate_read_slots(aggregate: &MirAggregate, out: &mut Vec<MirPlaceBase>) {
    match &aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                let (MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand)) = item;
                out.extend(operand_slot(operand));
            }
        }
        MirAggregateFields::Named(fields) => {
            for field in fields {
                out.extend(operand_slot(&field.value));
            }
        }
        MirAggregateFields::Keyed(fields) => {
            for field in fields {
                out.extend(operand_slot(&field.key));
                out.extend(operand_slot(&field.value));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Temp liveness (the S5-U2c dead-artifact forward trace)
// ---------------------------------------------------------------------------

/// Whether a statement is the lowered pure forward copy `Local(l) ←
/// Temp(t)` of the given temp — the only reader [`temp_forward_local`]
/// traces.
pub(crate) fn is_pure_temp_forward(statement: &MirStatement, temp: MirTempId) -> bool {
    let MirStatementKind::Assign { place, value } = &statement.kind else {
        return false;
    };
    if !matches!(place.base, MirPlaceBase::Local(_)) || !place.projections.is_empty() {
        return false;
    }
    matches!(&value.kind, MirValueKind::Operand(MirOperand::Temp(t)) if *t == temp)
}

/// The local a temp's value forwards into through the lowered body's copy
/// (`Local(l) ← Temp(t)`), when exactly one such forward exists.
pub(crate) fn temp_forward_local(function: &MirFunction, temp: MirTempId) -> Option<MirLocalId> {
    let mut found: Option<MirLocalId> = None;
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            let MirPlaceBase::Local(local) = place.base else {
                continue;
            };
            if !place.projections.is_empty() {
                continue;
            }
            let MirValueKind::Operand(MirOperand::Temp(forwarded)) = &value.kind else {
                continue;
            };
            if *forwarded != temp {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = Some(local);
        }
    }
    found
}

/// Whether any statement or terminator reads the temp OUTSIDE the lowered
/// forward copy — a direct operand consumer that is not the pure
/// `Local(l) ← Temp(t)` forward (which [`temp_forward_local`] traces).
pub(crate) fn temp_directly_read(function: &MirFunction, temp: MirTempId) -> bool {
    for block in &function.blocks {
        for statement in &block.statements {
            if is_pure_temp_forward(statement, temp) {
                continue;
            }
            if statement_reads_temp(statement, temp) {
                return true;
            }
        }
        if terminator_reads_temp(&block.terminator.kind, temp) {
            return true;
        }
    }
    false
}

/// Whether a statement reads the given temp in any operand or value position
/// (the statement's own destination is a write, never a read).
fn statement_reads_temp(statement: &MirStatement, temp: MirTempId) -> bool {
    match &statement.kind {
        MirStatementKind::Assign { value, .. } => value_reads_temp(value, temp),
        MirStatementKind::Call { callee, args, .. } => {
            callee_reads_temp(callee, temp) || operand_list_reads_temp(args, temp)
        }
        MirStatementKind::RuntimeCall { call, .. } => operand_list_reads_temp(&call.args, temp),
        MirStatementKind::Construct { aggregate, .. } => aggregate_reads_temp(aggregate, temp),
    }
}

/// Whether an aggregate construct reads the given temp (its field operands;
/// the destination is a write). Every field shape counts, including ordered
/// spreads and keyed keys.
fn aggregate_reads_temp(aggregate: &MirAggregate, temp: MirTempId) -> bool {
    match &aggregate.fields {
        MirAggregateFields::Ordered(items) => items
            .iter()
            .any(|item| aggregate_item_reads_temp(item, temp)),
        MirAggregateFields::Named(fields) => fields
            .iter()
            .any(|field| operand_reads_temp(&field.value, temp)),
        MirAggregateFields::Keyed(fields) => fields.iter().any(|field| {
            operand_reads_temp(&field.key, temp) || operand_reads_temp(&field.value, temp)
        }),
    }
}

/// Whether one ordered aggregate item reads the given temp.
fn aggregate_item_reads_temp(item: &MirAggregateItem, temp: MirTempId) -> bool {
    match item {
        MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
            operand_reads_temp(operand, temp)
        }
    }
}

/// Whether a value reads the given temp, over every value shape (plain and
/// unary/binary operands, tensor extents, option operations, and closure
/// environments).
fn value_reads_temp(value: &MirValue, temp: MirTempId) -> bool {
    match &value.kind {
        MirValueKind::Operand(operand) => operand_reads_temp(operand, temp),
        MirValueKind::Closure(closure) => operand_reads_temp(&closure.environment, temp),
        MirValueKind::Unary { operand, .. } => operand_reads_temp(operand, temp),
        MirValueKind::Binary { lhs, rhs, .. } => {
            operand_reads_temp(lhs, temp) || operand_reads_temp(rhs, temp)
        }
        MirValueKind::Option(op) => option_reads_temp(op, temp),
        MirValueKind::TensorExtent { tensor, .. } => operand_reads_temp(tensor, temp),
    }
}

/// Whether an option operation reads the given temp, including the operands
/// carried by an optional-chain link.
fn option_reads_temp(op: &MirOptionOp, temp: MirTempId) -> bool {
    match op {
        MirOptionOp::None => false,
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand) => operand_reads_temp(operand, temp),
        MirOptionOp::Unwrap { value, .. } => operand_reads_temp(value, temp),
        MirOptionOp::Coalesce { value, fallback } => {
            operand_reads_temp(value, temp) || operand_reads_temp(fallback, temp)
        }
        MirOptionOp::Chain { base, link } => {
            operand_reads_temp(base, temp) || option_chain_link_reads_temp(link, temp)
        }
    }
}

/// Whether an optional-chain link reads the given temp — an index operand or
/// a link call's callee/arguments.
fn option_chain_link_reads_temp(link: &MirOptionChainLink, temp: MirTempId) -> bool {
    match link {
        MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => false,
        MirOptionChainLink::Index(index) => operand_reads_temp(index, temp),
        MirOptionChainLink::Call { callee, args } => {
            callee_reads_temp(callee, temp) || operand_list_reads_temp(args, temp)
        }
    }
}

/// Whether a terminator reads the given temp.
fn terminator_reads_temp(terminator: &MirTerminatorKind, temp: MirTempId) -> bool {
    match terminator {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            operand_reads_temp(operand, temp)
        }
        MirTerminatorKind::TryCall { callee, args, .. } => {
            callee_reads_temp(callee, temp) || operand_list_reads_temp(args, temp)
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => false,
        MirTerminatorKind::Branch { condition, .. } => operand_reads_temp(condition, temp),
        MirTerminatorKind::Switch { value, .. } => operand_reads_temp(value, temp),
    }
}

/// Whether a call target reads the given temp: a closure carries its
/// environment operand and a first-class callee is itself an operand.
/// Symbolic function/definition callees read nothing.
fn callee_reads_temp(callee: &MirCallee, temp: MirTempId) -> bool {
    match callee {
        MirCallee::Function(_) | MirCallee::Definition { .. } => false,
        MirCallee::Closure(closure) => operand_reads_temp(&closure.environment, temp),
        MirCallee::Value(operand) => operand_reads_temp(operand, temp),
    }
}

/// Whether any operand in a call/projection argument list reads the given
/// temp.
fn operand_list_reads_temp(operands: &[MirOperand], temp: MirTempId) -> bool {
    operands.iter().any(|operand| operand_reads_temp(operand, temp))
}

/// Whether an operand reads the given temp as a plain base slot. A value id
/// was already scanned at its defining statement; a constant reads nothing.
fn operand_reads_temp(operand: &MirOperand, temp: MirTempId) -> bool {
    match operand {
        MirOperand::Temp(t) => *t == temp,
        MirOperand::Place(place) => matches!(place.base, MirPlaceBase::Temp(t) if t == temp),
        MirOperand::Value(_) | MirOperand::Constant(_) => false,
    }
}

#[cfg(test)]
#[path = "read_analysis_test.rs"]
mod tests;
