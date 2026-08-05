//! Shared collection-op statement classification and whole-function scans.
//!
//! The nested block/statement scan for `MirIntrinsic::Collection` runtime
//! calls is shared by the ABI contract resolver, the plan-mapping pass, the
//! static-shape fold, and the kernel decomposition. The statement-level
//! predicate is the single destructure; the whole-function scans build the
//! deduplicated op set (optionally dead-filtered against a read context).

use crate::{
    MirCollectionOp, MirFunction, MirIntrinsic, MirRuntimeCall, MirStatement, MirStatementKind,
};

/// The collection op a statement carries, when it is a collection runtime
/// call.
pub(crate) fn statement_collection_op(statement: &MirStatement) -> Option<MirCollectionOp> {
    let MirStatementKind::RuntimeCall {
        call:
            MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(op),
                ..
            },
        ..
    } = &statement.kind
    else {
        return None;
    };
    Some(*op)
}

/// The distinct collection ops in a kernel body's statement scan, in scan
/// order (the same scan `abi::collection_op_contract` runs; the plan pass
/// needs the FULL op set — never the first match only (N3.2) — to decide
/// recipe presence and to reject unplannable ops with a typed diagnostic).
///
/// A collection op whose result is never read is DEAD (S3-A5: the companion
/// recomputes the forward's mean without using it) — it is not part of the
/// kernel's semantics and must not drive the plan (a dead reduction must not
/// force a reduction contract on an elementwise kernel).
///
/// The dead filter resolves reads against `read_context` — the function the
/// op set is scanned from when that is the live context, or the SOURCE
/// function when the scan runs over a decomposition subchain (the subchain's
/// produced local is read by a LATER subchain, invisible inside the subchain
/// slice itself).
pub(crate) fn collection_ops_with_read_context(
    function: &MirFunction,
    read_context: &MirFunction,
) -> Vec<MirCollectionOp> {
    let mut ops: Vec<MirCollectionOp> = Vec::new();
    for block in &function.blocks {
        for statement in &block.statements {
            let Some(op) = statement_collection_op(statement) else {
                continue;
            };
            let MirStatementKind::RuntimeCall { destination, .. } = &statement.kind else {
                continue;
            };
            if destination
                .as_ref()
                .is_some_and(|dest| !crate::read_analysis::place_is_read(read_context, dest))
            {
                continue;
            }
            if !ops.contains(&op) {
                ops.push(op);
            }
        }
    }
    ops
}

/// The distinct collection ops in a kernel body's statement scan (plain
/// scan; the dead filter resolves reads against the function itself).
pub(crate) fn collection_ops_for_function(function: &MirFunction) -> Vec<MirCollectionOp> {
    collection_ops_with_read_context(function, function)
}
