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

fn eval_declared_lista_index(
    receiver: Value,
    index: Value,
    optional: bool,
) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let element = type_table.primitive(radix_types::Primitive::Numerus);
    let capacity = type_table.intern_index(radix_types::IndexExpr::Literal(8));
    let _declared = type_table.bounded_array(element, capacity);
    let return_ty = if optional {
        type_table.option(element)
    } else {
        element
    };
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
        return_ty: MirType::semantic(return_ty),
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
        return_ty: MirType::semantic(return_ty),
    };
    runner.eval_runtime_call(&call, Span::default())
}

#[test]
fn bounded_lista_dynamic_oob_is_a_hard_runner_error() {
    let error = eval_declared_lista_index(
        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
            Value::Int(1),
            Value::Int(2),
        ]))),
        Value::Int(8),
        false,
    )
    .expect_err("bounded lista dynamic OOB must fail hard");
    assert_eq!(error.issue, "runner_array_index_out_of_bounds");
}

#[test]
fn unbounded_lista_dynamic_oob_stays_nil_in_runner() {
    let value = eval_declared_lista_index(
        Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
            Value::Int(1),
            Value::Int(2),
        ]))),
        Value::Int(8),
        true,
    )
    .expect("unbounded lista lookup must not hard-fail");
    assert_eq!(value, Value::Nil);
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
    assert_eq!(
        err.issue, "runner_octeti_index_out_of_bounds",
        "expected live octeti OOB fail, got: {err:?}"
    );
}

#[test]
fn octeti_index_range_accepts_byte_list_carrier() {
    let receiver = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
        [0xde, 0xad, 0xbe, 0xef]
            .into_iter()
            .map(|byte| Value::Int(i64::from(byte)))
            .collect(),
    )));
    let result = eval_octeti_index(receiver, exclusive_range(0, 4))
        .expect("byte-list carrier must support octeti window reads");
    assert_eq!(result, Value::Octeti(vec![0xde, 0xad, 0xbe, 0xef]));
}

#[test]
fn octeti_index_scalar_still_returns_byte() {
    let result = eval_octeti_index(Value::Octeti(vec![0xde, 0xad, 0xbe, 0xef]), Value::Int(1))
        .expect("in-bounds octeti scalar");
    assert_eq!(result, Value::Int(0xad));
}

fn list_of(items: Vec<Value>) -> Value {
    Value::Array(std::rc::Rc::new(std::cell::RefCell::new(items)))
}

fn eval_list_window(
    receiver: Value,
    index: Value,
    element: impl FnOnce(&mut TypeTable) -> TypeId,
) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let element = element(&mut type_table);
    let list = type_table.array(element);
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
        return_ty: MirType::semantic(list),
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
    let frame = runner.frames.last_mut().unwrap();
    frame.values.insert(MirValueId(0), receiver);
    frame.values.insert(MirValueId(1), index);
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Index),
        args: vec![
            MirOperand::Value(MirValueId(0)),
            MirOperand::Value(MirValueId(1)),
        ],
        return_ty: MirType::semantic(list),
    };
    runner.eval_runtime_call(&call, Span::default())
}

fn numerus_element(types: &mut TypeTable) -> TypeId {
    types.primitive(radix_types::Primitive::Numerus)
}

#[test]
fn list_window_over_numerus_copies_the_range() {
    let receiver = list_of((10..15).map(Value::Int).collect());
    let result = eval_list_window(receiver, exclusive_range(1, 4), numerus_element)
        .expect("in-bounds list window");
    assert_eq!(
        result,
        list_of(vec![Value::Int(11), Value::Int(12), Value::Int(13)])
    );
}

#[test]
fn list_window_over_textus_copies_the_range() {
    let receiver = list_of(vec![
        Value::Str("a".into()),
        Value::Str("b".into()),
        Value::Str("c".into()),
    ]);
    let result = eval_list_window(receiver, exclusive_range(1, 3), |types| {
        types.primitive(radix_types::Primitive::Textus)
    })
    .expect("in-bounds textus list window");
    assert_eq!(
        result,
        list_of(vec![Value::Str("b".into()), Value::Str("c".into())])
    );
}

#[test]
fn list_window_over_bits_b8_copies_the_range() {
    let receiver = list_of(vec![Value::Int(0xde), Value::Int(0xad), Value::Int(0xbe)]);
    let result = eval_list_window(receiver, exclusive_range(0, 2), |types| {
        types.bits(radix_types::BitsWidth::B8)
    })
    .expect("in-bounds b8 list window");
    assert_eq!(result, list_of(vec![Value::Int(0xde), Value::Int(0xad)]));
}

#[test]
fn list_window_past_the_end_fails_not_short() {
    let receiver = list_of((0..3).map(Value::Int).collect());
    let error = eval_list_window(receiver, exclusive_range(1, 4), numerus_element)
        .expect_err("a window past the end must fail");
    assert_eq!(error.issue, "runner_list_window_out_of_bounds");
}

#[test]
fn list_window_empty_range_is_an_empty_list() {
    let receiver = list_of((0..3).map(Value::Int).collect());
    let result = eval_list_window(receiver, exclusive_range(2, 2), numerus_element)
        .expect("empty list window");
    assert_eq!(result, list_of(vec![]));
}

#[test]
fn list_window_inclusive_range_keeps_the_end() {
    let receiver = list_of((0..4).map(Value::Int).collect());
    let inclusive = Value::Tuple(vec![Value::Int(1), Value::Int(2), Value::Bool(true)]);
    let result =
        eval_list_window(receiver, inclusive, numerus_element).expect("inclusive list window");
    assert_eq!(result, list_of(vec![Value::Int(1), Value::Int(2)]));
}
