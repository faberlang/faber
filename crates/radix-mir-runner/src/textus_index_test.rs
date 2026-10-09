//! F-40: a scalar text index whose declared result is plain `textus` yields a
//! heap `Value::Str`, so `textus` consumers (`contains`) accept it. A declared
//! `littera` / `ascii<1>` result keeps the compact `Value::Ascii` carrier.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirOperand, MirProgram,
    MirRuntimeCall, MirType, MirValidationContext, MirValueId,
};
use radix_types::{IndexExpr, Primitive, TypeId, TypeTable};

/// Run one collection intrinsic over `args` (bound to `%0..`) in a fresh
/// runner. `return_ty` builds the declared result type from the table.
fn eval_collection(
    op: MirCollectionOp,
    args: Vec<Value>,
    return_ty: impl FnOnce(&mut TypeTable) -> TypeId,
) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let return_ty = return_ty(&mut type_table);
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
    let mut operands = Vec::new();
    for (slot, arg) in args.into_iter().enumerate() {
        let id = MirValueId(slot as u32);
        runner.frames.last_mut().unwrap().values.insert(id, arg);
        operands.push(MirOperand::Value(id));
    }
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(op),
        args: operands,
        return_ty: MirType::semantic(return_ty),
    };
    runner.eval_runtime_call(&call, Span::default())
}

fn index(text: &str, at: i64, return_ty: impl FnOnce(&mut TypeTable) -> TypeId) -> Value {
    eval_collection(
        MirCollectionOp::Index,
        vec![Value::Str(text.to_owned()), Value::Int(at)],
        return_ty,
    )
    .expect("text index")
}

fn textus(types: &mut TypeTable) -> TypeId {
    types.primitive(Primitive::Textus)
}

fn optional_textus(types: &mut TypeTable) -> TypeId {
    let inner = types.primitive(Primitive::Textus);
    types.option(inner)
}

fn contains(haystack: &str, needle: Value) -> Result<Value, RunnerError> {
    eval_collection(
        MirCollectionOp::Contains,
        vec![Value::Str(haystack.to_owned()), needle],
        textus,
    )
}

#[test]
fn textus_index_declared_textus_is_a_str() {
    assert_eq!(index("12", 0, textus), Value::Str("1".to_owned()));
}

#[test]
fn textus_get_declared_optional_textus_is_a_str_or_nil() {
    assert_eq!(index("12", 1, optional_textus), Value::Str("2".to_owned()));
    assert_eq!(index("12", 5, optional_textus), Value::Nil);
}

/// F-40 repro: `"0123456789".contains(s.get(0) coalesce "?")`.
#[test]
fn contains_accepts_the_text_index_result() {
    let needle = index("12", 0, optional_textus);
    assert_eq!(
        contains("0123456789", needle).expect("contains"),
        Value::Bool(true)
    );
    let needle = index("zz", 0, optional_textus);
    assert_eq!(
        contains("0123456789", needle).expect("contains"),
        Value::Bool(false)
    );
}

#[test]
fn contains_with_a_str_needle_is_unchanged() {
    assert_eq!(
        contains("0123456789", Value::Str("1".to_owned())).expect("contains"),
        Value::Bool(true)
    );
}

#[test]
fn textus_index_declared_littera_keeps_the_compact_carrier() {
    assert_eq!(
        index("ab", 0, |types| types.primitive(Primitive::Littera)),
        Value::Ascii(b'a')
    );
}

#[test]
fn textus_index_declared_ascii1_keeps_the_compact_carrier() {
    let value = index("ab", 1, |types| {
        let one = types.intern_index(IndexExpr::Literal(1));
        types.bounded_ascii(one)
    });
    assert_eq!(value, Value::Ascii(b'b'));
}
