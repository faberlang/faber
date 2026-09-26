//! CC-1 / D10.1: text cursors walk into one `Value::Char` (`littera`) per scalar.

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
fn ascii_cursor_two_bytes_are_littera_not_heap_strings() {
    let value = eval_cord_cursor(Value::Str("ab".to_owned())).expect("ascii cursor");
    let Value::Array(cell) = value else {
        panic!("cursor must materialize as an array, got {value:?}");
    };
    let elems = cell.borrow().clone();
    assert_eq!(elems, vec![Value::Char('a'), Value::Char('b')]);
}

#[test]
fn ascii_cursor_one_byte_ascii_value_is_singleton() {
    let value = eval_cord_cursor(Value::Ascii(b'x')).expect("one-byte ascii cursor");
    let Value::Array(cell) = value else {
        panic!("cursor must materialize as an array, got {value:?}");
    };
    assert_eq!(cell.borrow().as_slice(), &[Value::Char('x')]);
}

/// D10.1: text cursors yield one `littera` per Unicode scalar, with no
/// non-ASCII trap.
#[test]
fn textus_cursor_non_ascii_scalars_are_littera() {
    let value = eval_cord_cursor(Value::Str("é👍".to_owned())).expect("non-ASCII cursor");
    let Value::Array(cell) = value else {
        panic!("cursor must materialize as an array, got {value:?}");
    };
    assert_eq!(
        cell.borrow().as_slice(),
        &[Value::Char('é'), Value::Char('👍')]
    );
}
