//! Runner pins for `octeti ↦ lista<numerus<u8>>` (one-pass packed→array copy).

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirFunction, MirFunctionId, MirProgram, MirType, MirValidationContext,
};
use radix_types::{NumericWidth, Primitive, TypeId, TypeTable};

fn eval_octeti_to_lista(
    bytes: Vec<u8>,
    element_width: Option<NumericWidth>,
) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let octeti = type_table.primitive(Primitive::Octeti);
    let element = match element_width {
        Some(width) => type_table.sized_numeric(Primitive::Numerus, width),
        None => type_table.primitive(Primitive::Numerus),
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
    runner.convert_to_array(
        Value::Octeti(bytes),
        octeti,
        element,
        None,
        &[],
        Span::default(),
    )
}

#[test]
fn octeti_to_lista_u8_copies_bytes_in_one_pass() {
    let result = eval_octeti_to_lista(vec![0xde, 0xad, 0xbe, 0xef], Some(NumericWidth::U8))
        .expect("octeti to lista<u8>");
    let Value::Array(items) = result else {
        panic!("expected lista, got {result:?}");
    };
    assert_eq!(
        items.borrow().as_slice(),
        &[
            Value::Int(0xde),
            Value::Int(0xad),
            Value::Int(0xbe),
            Value::Int(0xef)
        ]
    );
}

#[test]
fn empty_octeti_to_lista_u8_is_empty() {
    let result = eval_octeti_to_lista(vec![], Some(NumericWidth::U8)).expect("empty octeti");
    let Value::Array(items) = result else {
        panic!("expected lista, got {result:?}");
    };
    assert!(items.borrow().is_empty());
}

#[test]
fn octeti_to_lista_unsized_numerus_fails_closed() {
    let err = eval_octeti_to_lista(vec![0xde], None).expect_err("unsized lista must fail");
    assert_eq!(
        err.issue, "runner_list_b8_to_lista_conversion_requires_lista_u8",
        "expected u8-only reject, got: {err:?}"
    );
}
