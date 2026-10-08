//! `TensorExtent` on the runner: the value is the carrier's extent on the
//! requested axis, read from the tensor's runtime shape.
// Test fixtures: exact float expectations and small, known-in-range numeric casts.
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use super::*;

use radix_mir::{MirType, MirValue};
use radix_types::TypeId;

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

/// Evaluate `TensorExtent { value 0, axis }` with `tensor` in frame slot 0.
fn extent(tensor: TensorValue, axis: u32) -> Result<Value, RunnerError> {
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
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), Value::Tensor(Rc::new(RefCell::new(tensor))));
    runner.eval_value(
        &MirValue {
            id: MirValueId(1),
            kind: MirValueKind::TensorExtent {
                tensor: MirOperand::Value(MirValueId(0)),
                axis,
            },
            ty: MirType::semantic(TypeId(0)),
            span: Span::default(),
        },
        Span::default(),
    )
}

fn zeros(shape: &[i64]) -> TensorValue {
    let count: i64 = shape.iter().product();
    TensorValue::contiguous(vec![Value::Float(0.0); count as usize], shape.to_vec())
}

#[test]
fn tensor_extent_reads_each_axis_of_a_runtime_shaped_tensor() {
    assert!(matches!(extent(zeros(&[3, 5]), 0), Ok(Value::Int(3))));
    assert!(matches!(extent(zeros(&[3, 5]), 1), Ok(Value::Int(5))));
    assert!(matches!(extent(zeros(&[7]), 0), Ok(Value::Int(7))));
}

#[test]
fn tensor_extent_of_a_view_is_the_view_extent() {
    let base = zeros(&[4, 6]);
    let view = base
        .view(vec![2, 3], vec![6, 1], 0)
        .expect("a 2x3 view of a 4x6 tensor");
    assert!(matches!(extent(view, 0), Ok(Value::Int(2))));
    assert!(matches!(extent(view_again(&base), 1), Ok(Value::Int(3))));
}

fn view_again(base: &TensorValue) -> TensorValue {
    base.view(vec![2, 3], vec![6, 1], 0).expect("view")
}

#[test]
fn tensor_extent_axis_past_the_carrier_rank_errors() {
    assert!(extent(zeros(&[3, 5]), 2).is_err());
}
