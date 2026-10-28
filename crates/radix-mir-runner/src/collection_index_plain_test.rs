//! Runner pins for plain `xs[i]` element reads versus interval windows.
//!
//! A plain integer index must reach the element read without building (and
//! dropping) a `RunnerError`; an interval index must still slice; and every
//! failure keeps its message, issue and code.

use super::*;
use crate::value::TensorValue;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirOperand, MirProgram,
    MirRuntimeCall, MirType, MirValidationContext, MirValueId,
};
use radix_types::{TypeId, TypeTable};

fn eval_index(receiver: Value, index: Value) -> Result<Value, RunnerError> {
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
    let frame = runner.frames.last_mut().unwrap();
    frame.values.insert(MirValueId(0), receiver);
    frame.values.insert(MirValueId(1), index);
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

fn list(values: &[i64]) -> Value {
    Value::Array(Rc::new(RefCell::new(
        values.iter().copied().map(Value::Int).collect(),
    )))
}

fn tensor(values: &[i64]) -> Value {
    Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        values.iter().copied().map(Value::Int).collect(),
        vec![values.len() as i64],
    ))))
}

fn range(start: i64, end: i64, inclusive: bool) -> Value {
    Value::Tuple(vec![
        Value::Int(start),
        Value::Int(end),
        Value::Bool(inclusive),
    ])
}

fn assert_same_error(actual: &RunnerError, expected: &RunnerError) {
    assert_eq!(actual.issue, expected.issue);
    assert_eq!(actual.code, expected.code);
    assert_eq!(actual.args, expected.args);
}

/// The expected error: `error` plus the structured out-of-bounds facts.
fn with_facts(mut error: RunnerError, facts: &[(&str, usize)]) -> RunnerError {
    error.args.extend(
        facts
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.to_string())),
    );
    error
}

fn errors_built() -> usize {
    crate::INTERNAL_ERRORS_BUILT.with(std::cell::Cell::get)
}

#[test]
fn plain_index_returns_the_element_for_list_bytes_text_and_tensor() {
    assert_eq!(
        eval_index(list(&[10, 20, 30]), Value::Int(1)).unwrap(),
        Value::Int(20)
    );
    assert_eq!(
        eval_index(Value::Octeti(vec![7, 8, 9]), Value::Int(2)).unwrap(),
        Value::Int(9)
    );
    assert_eq!(
        eval_index(Value::Str("abc".into()), Value::Int(1)).unwrap(),
        Value::Str("b".into())
    );
    assert_eq!(
        eval_index(tensor(&[4, 5, 6]), Value::Int(0)).unwrap(),
        Value::Int(4)
    );
}

#[test]
fn plain_index_builds_no_runner_error() {
    let before = errors_built();
    eval_index(list(&[10, 20, 30]), Value::Int(1)).unwrap();
    eval_index(Value::Octeti(vec![7, 8, 9]), Value::Int(2)).unwrap();
    eval_index(Value::Str("abc".into()), Value::Int(1)).unwrap();
    eval_index(tensor(&[4, 5, 6]), Value::Int(0)).unwrap();
    assert_eq!(
        errors_built(),
        before,
        "a plain index must not build an error"
    );
}

#[test]
fn out_of_range_plain_index_keeps_its_error_text_and_code() {
    let span = Span::default();
    let error = eval_index(list(&[1]), Value::Int(5)).unwrap_err();
    assert_same_error(
        &error,
        &with_facts(
            RunnerError::internal(span, "array index out of bounds"),
            &[("index", 5), ("len", 1)],
        ),
    );
    assert_eq!(error.code, "runner_internal");

    let error = eval_index(Value::Octeti(vec![1]), Value::Int(5)).unwrap_err();
    assert_same_error(
        &error,
        &with_facts(
            RunnerError::internal(span, "list<b8> index out of bounds"),
            &[("index", 5), ("len", 1)],
        ),
    );

    let error = eval_index(Value::Str("a".into()), Value::Int(5)).unwrap_err();
    assert_eq!(error.issue, "runner_text_index_out_of_bounds");
    assert_eq!(error.arg("index"), Some("5"));
    assert_eq!(error.arg("len"), Some("1"));
    assert_eq!(error.code, "runner_internal");

    let error = eval_index(tensor(&[1]), Value::Int(5)).unwrap_err();
    assert_same_error(
        &error,
        &RunnerError::internal(span, "tensor index out of bounds"),
    );
}

#[test]
fn wrong_type_plain_index_keeps_the_as_index_error() {
    let span = Span::default();
    let expected = Value::Str("x".into()).as_index(span).unwrap_err();
    for receiver in [list(&[1]), Value::Octeti(vec![1]), Value::Str("a".into())] {
        let error = eval_index(receiver, Value::Str("x".into())).unwrap_err();
        assert_same_error(&error, &expected);
    }
    let expected = Value::Int(-1).as_index(span).unwrap_err();
    let error = eval_index(list(&[1]), Value::Int(-1)).unwrap_err();
    assert_same_error(&error, &expected);
}

#[test]
fn interval_index_still_slices() {
    assert_eq!(
        eval_index(Value::Octeti(vec![1, 2, 3, 4]), range(1, 3, false)).unwrap(),
        Value::Octeti(vec![2, 3])
    );
    assert_eq!(
        eval_index(Value::Octeti(vec![1, 2, 3, 4]), range(1, 3, true)).unwrap(),
        Value::Octeti(vec![2, 3, 4])
    );
    assert_eq!(
        eval_index(Value::Str("abcdef".into()), range(1, 4, false)).unwrap(),
        Value::Str("bcd".into())
    );
    // A lista<u8> carrier windows back to octeti.
    assert_eq!(
        eval_index(list(&[1, 2, 3, 4]), range(0, 2, false)).unwrap(),
        Value::Octeti(vec![1, 2])
    );
    let error = eval_index(Value::Octeti(vec![1, 2]), range(1, 9, false)).unwrap_err();
    assert_same_error(
        &error,
        &with_facts(
            RunnerError::internal(Span::default(), "list<b8> index out of bounds"),
            &[("start", 1), ("end", 9), ("len", 2)],
        ),
    );
}

#[test]
fn malformed_tuple_index_still_falls_through_to_the_as_index_error() {
    let span = Span::default();
    let malformed = Value::Tuple(vec![Value::Int(1), Value::Int(2)]);
    let expected = malformed.as_index(span).unwrap_err();
    let error = eval_index(Value::Octeti(vec![1, 2, 3]), malformed).unwrap_err();
    assert_same_error(&error, &expected);
}
