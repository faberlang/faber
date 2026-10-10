//! Runner pins for `textus ↦ textus<N>`: the value stays a
//! text; the width check counts Unicode scalars at the store.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirFunction, MirFunctionId, MirProgram, MirType, MirValidationContext,
};
use radix_types::{IndexExpr, TypeId, TypeTable};

fn eval_store(value: Value, width: u64) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let capacity = type_table.intern_index(IndexExpr::Literal(width));
    let _target_ty = type_table.bounded_textus(capacity);
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
    runner.convert_to_bounded_textus(value, capacity, None, Span::default())
}

#[test]
fn store_within_the_bound_keeps_the_text() {
    let result = eval_store(Value::Str("abcd".to_owned()), 8).expect("fits");
    assert_eq!(result, Value::Str("abcd".to_owned()));
}

#[test]
fn bound_counts_scalars_not_bytes() {
    let result = eval_store(Value::Str("héllo".to_owned()), 5).expect("5 scalars fit");
    assert_eq!(result, Value::Str("héllo".to_owned()));
}

#[test]
fn store_over_the_bound_fails_with_the_rust_text() {
    let err = eval_store(Value::Str("abcdef".to_owned()), 4).expect_err("6 > 4");
    assert!(
        err.message
            .contains("textus to bounded textus conversion failed"),
        "got: {err:?}"
    );
}
