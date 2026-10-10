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
pub(super) fn eval_collection(
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

/// `s[lo‥hi]` through the collection index op, exclusive range.
fn text_range(text: &str, lo: i64, hi: i64) -> Result<Value, RunnerError> {
    eval_collection(
        MirCollectionOp::Index,
        vec![
            Value::Str(text.to_owned()),
            Value::Tuple(vec![Value::Int(lo), Value::Int(hi), Value::Bool(false)]),
        ],
        textus,
    )
}

#[test]
fn text_range_past_the_end_dies_naming_the_range() {
    let error = text_range("abcdef", 1, 99).expect_err("a range past the end must die");
    assert_eq!(error.issue, "runner_text_range_out_of_bounds");
    assert_eq!(error.message, "text range 1‥99 outside 0‥6");
}

#[test]
fn text_range_wholly_outside_dies() {
    let error = text_range("abcdef", 10, 12).expect_err("a range outside must die");
    assert_eq!(error.message, "text range 10‥12 outside 0‥6");
}

#[test]
fn text_range_reversed_dies() {
    let error = text_range("abcdef", 4, 2).expect_err("a reversed range must die");
    assert_eq!(error.message, "text range 4‥2 outside 0‥6");
}

#[test]
fn text_range_inside_is_the_substring() {
    assert_eq!(
        text_range("abcdef", 0, 3).expect("in-range text window"),
        Value::Str("abc".to_owned())
    );
}

#[test]
fn text_range_empty_at_the_end_is_the_empty_string() {
    assert_eq!(
        text_range("abcdef", 6, 6).expect("empty range at the end"),
        Value::Str(String::new())
    );
}

#[test]
fn textus_get_non_ascii_is_the_ith_character_or_nil() {
    assert_eq!(
        index("Grüße", 2, optional_textus),
        Value::Str("ü".to_owned())
    );
    assert_eq!(index("Grüße", 9, optional_textus), Value::Nil);
}

#[test]
fn text_get_negative_index_is_nil() {
    assert_eq!(index("hé", 1, optional_textus), Value::Str("é".to_owned()));
    assert_eq!(index("hé", -1, optional_textus), Value::Nil);
}
