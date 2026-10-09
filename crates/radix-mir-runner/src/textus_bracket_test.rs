//! F-44: the bracket projection `s[i]` on a `textus` yields the compact
//! `Value::Ascii` (a MIR place carries no static type, so the runner cannot
//! widen it the way the `get` collection op does). Every `Str`-only consumer
//! reads text through `as_text_str`, so the projection result behaves exactly
//! like the `get` result: `contains`, equality, and map keys.

use super::textus_index_test::eval_collection;
use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirCollectionOp, MirConstant, MirFunction, MirFunctionId, MirOperand, MirProgram,
    MirProjection, MirType, MirValidationContext,
};
use radix_types::{Primitive, TypeId, TypeTable};

/// `text[at]` through the runner's place projection (the bracket path).
fn bracket(text: &str, at: i64) -> Value {
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
    runner.push_frame(program.functions[0].id, MirBlockId(0));
    let projection = MirProjection::Index(MirOperand::Constant(MirConstant::Int(at)));
    runner
        .project(Value::Str(text.to_owned()), &projection, Span::default())
        .expect("bracket projection")
}

fn get(text: &str, at: i64) -> Value {
    eval_collection(
        MirCollectionOp::Index,
        vec![Value::Str(text.to_owned()), Value::Int(at)],
        |types| types.primitive(Primitive::Textus),
    )
    .expect("get")
}

fn contains(haystack: &str, needle: Value) -> Value {
    eval_collection(
        MirCollectionOp::Contains,
        vec![Value::Str(haystack.to_owned()), needle],
        |types| types.primitive(Primitive::Bivalens),
    )
    .expect("contains accepts a bracket projection")
}

#[test]
fn bracket_projection_is_the_compact_ascii_carrier() {
    assert_eq!(bracket("12", 0), Value::Ascii(b'1'));
}

#[test]
fn contains_accepts_the_bracket_projection_like_get() {
    for (text, hit) in [("12", true), ("zz", false)] {
        assert_eq!(contains("0123456789", bracket(text, 0)), Value::Bool(hit));
        assert_eq!(
            contains("0123456789", bracket(text, 0)),
            contains("0123456789", get(text, 0))
        );
    }
}

#[test]
fn contains_accepts_a_bracket_projection_receiver() {
    assert_eq!(
        eval_collection(
            MirCollectionOp::Contains,
            vec![bracket("12", 0), Value::Str("1".to_owned())],
            |types| types.primitive(Primitive::Bivalens),
        )
        .expect("contains on an ascii receiver"),
        Value::Bool(true)
    );
}

#[test]
fn bracket_projection_equals_the_get_result() {
    assert!(values_equal(&bracket("12", 1), &get("12", 1)));
    assert!(values_equal(&bracket("12", 1), &Value::Str("2".to_owned())));
    assert!(!values_equal(
        &bracket("12", 1),
        &Value::Str("1".to_owned())
    ));
}

#[test]
fn bracket_projection_keys_a_map_like_get() {
    let span = Span::default();
    assert_eq!(
        bracket("12", 0).to_key(span).expect("bracket key"),
        get("12", 0).to_key(span).expect("get key")
    );
}

#[test]
fn other_text_consumers_accept_the_bracket_projection() {
    // split delimiter and replace pattern are `Str`-only operands too.
    let split = eval_collection(
        MirCollectionOp::Split,
        vec![Value::Str("a-b".to_owned()), bracket("-", 0)],
        |types| types.primitive(Primitive::Textus),
    )
    .expect("split by a bracket projection");
    let Value::Array(parts) = split else {
        panic!("split result is not an array");
    };
    assert_eq!(
        *parts.borrow(),
        vec![Value::Str("a".to_owned()), Value::Str("b".to_owned())]
    );
    let replaced = eval_collection(
        MirCollectionOp::Replace,
        vec![
            Value::Str("a-b".to_owned()),
            bracket("-", 0),
            bracket("+", 0),
        ],
        |types| types.primitive(Primitive::Textus),
    )
    .expect("replace by bracket projections");
    assert_eq!(replaced, Value::Str("a+b".to_owned()));
}
