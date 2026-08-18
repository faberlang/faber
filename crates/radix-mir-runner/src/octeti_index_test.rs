//! Runner pins for `octeti[i]` and `octeti[lo‥hi]`.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirOperand, MirProgram,
    MirRuntimeCall, MirType, MirValidationContext, MirValueId,
};
use radix_types::{TypeId, TypeTable};

fn eval_octeti_index(receiver: Value, index: Value) -> Result<Value, RunnerError> {
    let type_table = TypeTable::new();
    let interner = Interner::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
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
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), receiver);
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(1), index);
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Index),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(TypeId(0)),
    };
    runner.eval_runtime_call(&call, Span::default())
}

fn exclusive_range(start: i64, end: i64) -> Value {
    Value::Tuple(vec![Value::Int(start), Value::Int(end), Value::Bool(false)])
}

#[test]
fn octeti_index_range_in_bounds_copies_window() {
    let result = eval_octeti_index(
        Value::Octeti(vec![0xde, 0xad, 0xbe, 0xef]),
        exclusive_range(0, 4),
    )
    .expect("in-bounds octeti range");
    assert_eq!(result, Value::Octeti(vec![0xde, 0xad, 0xbe, 0xef]));
}

#[test]
fn octeti_index_range_out_of_bounds_fails() {
    let err = eval_octeti_index(
        Value::Octeti(vec![0xde, 0xad, 0xbe, 0xef]),
        exclusive_range(0, 5),
    )
    .expect_err("out-of-range octeti window must fail");
    assert!(
        err.message.contains("octeti index out of bounds"),
        "expected live octeti OOB fail, got: {}",
        err.message
    );
}

#[test]
fn octeti_index_scalar_still_returns_byte() {
    let result = eval_octeti_index(Value::Octeti(vec![0xde, 0xad, 0xbe, 0xef]), Value::Int(1))
        .expect("in-bounds octeti scalar");
    assert_eq!(result, Value::Int(0xad));
}
