//! `TensorCopyInto(dest, src)` on the runner: the source's logical values are
//! written through the destination's strides into its existing storage.

use super::*;

use radix_mir::{MirCollectionOp, MirIntrinsic, MirRuntimeCall, MirType};
use radix_types::TypeId;
use std::sync::Arc;

use crate::value::TensorValue;

fn function() -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn floats(values: &[f64]) -> Vec<Value> {
    values.iter().copied().map(Value::Float).collect()
}

fn contiguous(values: &[f64], shape: &[i64]) -> Rc<RefCell<TensorValue>> {
    Rc::new(RefCell::new(TensorValue::contiguous(
        floats(values),
        shape.to_vec(),
    )))
}

fn logical(tensor: &Rc<RefCell<TensorValue>>) -> Vec<f64> {
    tensor
        .borrow()
        .logical_values()
        .expect("valid tensor layout")
        .into_iter()
        .map(|value| match value {
            Value::Float(value) => value,
            other => panic!("expected a float element, got {other:?}"),
        })
        .collect()
}

/// Run `TensorCopyInto(dest, src)` and return its result.
fn copy_into(
    dest: &Rc<RefCell<TensorValue>>,
    src: &Rc<RefCell<TensorValue>>,
) -> Result<Value, RunnerError> {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let mut host = BufferHost::default();
    let types = TypeTable::default();
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(function());
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    let frame = runner.frames.last_mut().unwrap();
    frame
        .values
        .insert(MirValueId(0), Value::Tensor(Rc::clone(dest)));
    frame
        .values
        .insert(MirValueId(1), Value::Tensor(Rc::clone(src)));
    runner.eval_runtime_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorCopyInto),
            args: vec![
                MirOperand::Value(MirValueId(0)),
                MirOperand::Value(MirValueId(1)),
            ],
            return_ty: MirType::semantic(TypeId(0)),
        },
        Span::default(),
    )
}

#[test]
fn copies_every_value_and_returns_unit() {
    let dest = contiguous(&[0.0; 4], &[2, 2]);
    let src = contiguous(&[1.0, 2.0, 3.0, 4.0], &[2, 2]);
    let result = copy_into(&dest, &src).expect("copy-into runs");
    assert_eq!(result, Value::Unit);
    assert_eq!(logical(&dest), [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn the_destination_keeps_its_storage_identity() {
    let dest = contiguous(&[0.0; 4], &[2, 2]);
    let alias = Rc::new(RefCell::new(dest.borrow().clone()));
    let storage = Arc::clone(&dest.borrow().data);
    let src = contiguous(&[1.0, 2.0, 3.0, 4.0], &[2, 2]);
    copy_into(&dest, &src).expect("copy-into runs");
    assert!(
        Arc::ptr_eq(&storage, &dest.borrow().data),
        "the destination allocation is the same one"
    );
    assert_eq!(
        logical(&alias),
        [1.0, 2.0, 3.0, 4.0],
        "a second alias of the destination sees the new values"
    );
}

#[test]
fn a_strided_view_source_is_copied_by_logical_position() {
    // `src` is the transpose of a contiguous 2x3 buffer: shape [3, 2],
    // strides [1, 3]. Logical order is 1, 4, 2, 5, 3, 6.
    let base = TensorValue::contiguous(floats(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), vec![2, 3]);
    let transposed = base
        .view(vec![3, 2], vec![1, 3], 0)
        .expect("transpose view");
    let src = Rc::new(RefCell::new(transposed));
    let dest = contiguous(&[0.0; 6], &[3, 2]);
    copy_into(&dest, &src).expect("copy-into runs");
    assert_eq!(logical(&dest), [1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
}

#[test]
fn a_strided_destination_is_written_through_its_strides() {
    // The destination is the transposed view of a contiguous 2x3 buffer; the
    // copy writes logical position (i, j) at physical offset i + 3 * j.
    let base = Rc::new(RefCell::new(TensorValue::contiguous(
        floats(&[0.0; 6]),
        vec![2, 3],
    )));
    let transposed = base
        .borrow()
        .view(vec![3, 2], vec![1, 3], 0)
        .expect("transpose view");
    let dest = Rc::new(RefCell::new(transposed));
    let src = contiguous(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[3, 2]);
    copy_into(&dest, &src).expect("copy-into runs");
    assert_eq!(logical(&dest), [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    assert_eq!(
        logical(&base),
        [1.0, 3.0, 5.0, 2.0, 4.0, 6.0],
        "the contiguous owner sees the strided write"
    );
}

#[test]
fn an_overlapping_source_is_read_fully_before_the_write() {
    // The source is the transpose of the destination's own storage: copying
    // it back must use the values it had when the statement began, not a
    // half-overwritten buffer.
    let dest = contiguous(&[1.0, 2.0, 3.0, 4.0], &[2, 2]);
    let transposed = dest
        .borrow()
        .view(vec![2, 2], vec![1, 2], 0)
        .expect("transpose view");
    let src = Rc::new(RefCell::new(transposed));
    copy_into(&dest, &src).expect("copy-into runs");
    assert_eq!(logical(&dest), [1.0, 3.0, 2.0, 4.0]);
}

#[test]
fn a_shape_mismatch_is_a_recoverable_error_naming_the_issue() {
    let dest = contiguous(&[0.0; 4], &[2, 2]);
    let src = contiguous(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], &[2, 3]);
    let error = copy_into(&dest, &src).expect_err("a shape mismatch is an error value");
    assert_eq!(error.issue, "runner_tensor_copy_into_shape_mismatch");
    assert_eq!(
        logical(&dest),
        [0.0; 4],
        "a rejected copy writes nothing into the destination"
    );
}
