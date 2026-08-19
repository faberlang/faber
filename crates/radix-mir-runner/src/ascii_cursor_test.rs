//! CC-1: `ascii.cursor()` walks bytes into one-byte `Value::Ascii` cells.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirOperand, MirProgram,
    MirRuntimeCall, MirType, MirValidationContext, MirValueId,
};
use radix_types::{TypeId, TypeTable};

fn eval_cord_cursor(receiver: Value) -> Result<Value, RunnerError> {
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
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::CordCursor),
        args: vec![MirOperand::Value(MirValueId(0))],
        return_ty: MirType::semantic(TypeId(0)),
    };
    runner.eval_runtime_call(&call, Span::default())
}

#[test]
fn ascii_cursor_two_bytes_are_ascii1_not_heap_strings() {
    let value = eval_cord_cursor(Value::Str("ab".to_owned())).expect("ascii cursor");
    let Value::Array(cell) = value else {
        panic!("cursor must materialize as an array, got {value:?}");
    };
    let elems = cell.borrow().clone();
    assert_eq!(elems, vec![Value::Ascii(b'a'), Value::Ascii(b'b')]);
    assert!(
        elems.iter().all(|elem| !matches!(elem, Value::Str(_))),
        "cord cursor must not allocate a heap String per step, got {elems:?}"
    );
}

#[test]
fn ascii_cursor_one_byte_ascii_value_is_singleton() {
    let value = eval_cord_cursor(Value::Ascii(b'x')).expect("one-byte ascii cursor");
    let Value::Array(cell) = value else {
        panic!("cursor must materialize as an array, got {value:?}");
    };
    assert_eq!(cell.borrow().as_slice(), &[Value::Ascii(b'x')]);
}

#[test]
fn textus_cursor_two_scalars_are_ascii1_not_heap_strings() {
    let value = eval_cord_cursor(Value::Str("ab".to_owned())).expect("textus cursor");
    let Value::Array(cell) = value else {
        panic!("cursor must materialize as an array, got {value:?}");
    };
    let elems = cell.borrow().clone();
    assert_eq!(elems, vec![Value::Ascii(b'a'), Value::Ascii(b'b')]);
    assert!(
        elems.iter().all(|elem| !matches!(elem, Value::Str(_))),
        "cord cursor must not allocate a heap String per step, got {elems:?}"
    );
}

#[test]
fn textus_cursor_non_ascii_scalar_traps() {
    let err = eval_cord_cursor(Value::Str("é".to_owned())).expect_err("non-ASCII");
    assert!(
        err.message.contains("textus index trap"),
        "non-ASCII cursor must use the live [i] trap, got {err:?}"
    );
    let err = eval_cord_cursor(Value::Str("ξ".to_owned())).expect_err("non-ASCII greek");
    assert!(
        err.message.contains("textus index trap"),
        "non-ASCII cursor must use the live [i] trap, got {err:?}"
    );
}
