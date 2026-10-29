//! Runner pin for `vector<T, size N> ↦ lista<T>` register-carrier conversion.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirFunction, MirFunctionId, MirProgram, MirType, MirValidationContext,
};
use radix_types::{IndexExpr, NumericWidth, Primitive, TypeId, TypeTable};
use std::cell::RefCell;
use std::rc::Rc;

fn eval_vector_to_lista() -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let element = type_table.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let width = type_table.intern_index(IndexExpr::Literal(3));
    let vector = type_table.vector_with_width(element, width);
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
        Value::Vector(Rc::new(RefCell::new(vec![
            Value::Float(1.0),
            Value::Float(2.0),
            Value::Float(3.0),
        ]))),
        vector,
        element,
        None,
        &[],
        Span::default(),
    )
}

#[test]
fn vector_to_lista_f32_copies_lanes_in_order() {
    let result = eval_vector_to_lista().expect("vector<f32, magnitudo 3> to lista<f32>");
    let Value::Array(items) = result else {
        panic!("expected lista, got {result:?}");
    };
    assert_eq!(
        items.borrow().as_slice(),
        &[Value::Float(1.0), Value::Float(2.0), Value::Float(3.0)]
    );
}
