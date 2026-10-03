//! Device lowering of the `⇇` copy-into statement.
//!
//! On the host, `target ⇇ value` is one [`MirCollectionOp::TensorCopyInto`]:
//! the runner copies the value through the target's strides into its existing
//! storage. A device kernel's `mut` tensor parameter has no separate host
//! storage to preserve: it IS the output buffer the recipe bodies already
//! write their result into. So on a device function the copy lowers exactly
//! as the retired whole-tensor `output ← value` did: one forwarding
//! [`MirStatementKind::Assign`] of the source operand into the target place.
//! A recipe result (matmul, rope, softmax, ...) then rebinds to the output
//! buffer (fusion, no extra copy), a view source (`input.transpose()`) is
//! written through its recipe directly, and a plain parameter source becomes
//! an elementwise copy body.
//!
//! INVARIANTS
//! ==========
//! - Static size equality is guaranteed by the semantic copy-into rules; no
//!   device runtime check is introduced here (entry checks stay out of
//!   kernels).
//! - A copy-into whose destination is not an unprojected local place is left
//!   untouched, so the device gate still rejects it by name.

use crate::local_ty::local_ty;
use crate::{
    MirCollectionOp, MirFunction, MirIntrinsic, MirOperand, MirPlaceBase, MirRuntimeCall,
    MirStatement, MirStatementKind, MirValue, MirValueId, MirValueKind,
};

/// Rewrite every device-lowerable `TensorCopyInto(dest, src)` in `function`
/// into the forwarding assign `dest = src`. Returns the number of rewritten
/// statements.
///
/// Call only on functions with a device role; host functions keep the
/// storage-preserving copy.
pub fn lower_device_copy_into(function: &mut MirFunction) -> usize {
    let mut next_value = next_value_id(function);
    let mut rewrites = Vec::new();
    for (block_index, block) in function.blocks.iter().enumerate() {
        for (statement_index, statement) in block.statements.iter().enumerate() {
            if let Some(assign) = forwarding_assign(function, statement, &mut next_value) {
                rewrites.push((block_index, statement_index, assign));
            }
        }
    }
    let count = rewrites.len();
    for (block_index, statement_index, assign) in rewrites {
        function.blocks[block_index].statements[statement_index] = assign;
    }
    count
}

fn forwarding_assign(
    function: &MirFunction,
    statement: &MirStatement,
    next_value: &mut u32,
) -> Option<MirStatement> {
    let MirStatementKind::RuntimeCall {
        call:
            MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorCopyInto),
                args,
                ..
            },
        ..
    } = &statement.kind
    else {
        return None;
    };
    let [MirOperand::Place(dest), source] = args.as_slice() else {
        return None;
    };
    let MirPlaceBase::Local(local) = dest.base else {
        return None;
    };
    if !dest.projections.is_empty() {
        return None;
    }
    let ty = local_ty(function, local)?;
    let id = MirValueId(*next_value);
    *next_value += 1;
    Some(MirStatement {
        kind: MirStatementKind::Assign {
            place: dest.clone(),
            value: MirValue {
                id,
                kind: MirValueKind::Operand(source.clone()),
                ty,
                span: statement.span,
            },
        },
        span: statement.span,
    })
}

/// One past the largest value id the function defines.
fn next_value_id(function: &MirFunction) -> u32 {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::Assign { value, .. } => Some(value.id.0 + 1),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}
