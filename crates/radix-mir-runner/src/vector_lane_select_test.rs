//! Runner pins for the run-time lane select on a vector (`v[i]`).
//!
//! The checked-index law: an `Index` projection on a `vector<T, size N>` fails
//! exactly as the same projection on a `lista` does, so every pin compares the
//! vector failure with the lista failure instead of naming a message.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirConstant, MirFunction, MirFunctionId, MirOperand, MirProgram, MirProjection,
    MirType, MirValidationContext,
};
use radix_types::{TypeId, TypeTable};
use std::cell::RefCell;
use std::rc::Rc;

fn lanes() -> Vec<Value> {
    vec![Value::Float(1.0), Value::Float(2.0), Value::Float(3.0)]
}

fn vector() -> Value {
    Value::Vector(Rc::new(RefCell::new(lanes())))
}

fn lista() -> Value {
    Value::Array(Rc::new(RefCell::new(lanes())))
}

fn index(at: i64) -> MirProjection {
    MirProjection::Index(MirOperand::Constant(MirConstant::Int(at)))
}

fn with_runner<T>(body: impl FnOnce(&mut crate::Runner<'_, crate::host::BufferHost>) -> T) -> T {
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
    body(&mut runner)
}

fn read(receiver: Value, at: i64) -> Result<Value, RunnerError> {
    with_runner(|runner| runner.project(receiver, &index(at), Span::default()))
}

fn write(receiver: Value, at: i64) -> Result<Value, RunnerError> {
    with_runner(|runner| {
        runner.project_write(receiver, &index(at), Value::Float(9.0), Span::default())
    })
}

fn failure_identity(error: &RunnerError) -> (&str, &'static str) {
    (error.issue.as_str(), error.code)
}

#[test]
fn in_range_lane_reads_the_lane() {
    for (at, expected) in [(0, 1.0), (1, 2.0), (2, 3.0)] {
        assert_eq!(read(vector(), at).unwrap(), Value::Float(expected));
    }
}

#[test]
fn past_the_width_fails_like_a_lista() {
    let vector_error = read(vector(), 3).unwrap_err();
    let lista_error = read(lista(), 3).unwrap_err();
    assert_eq!(
        failure_identity(&vector_error),
        failure_identity(&lista_error)
    );
}

#[test]
fn negative_lane_fails_like_a_lista() {
    let vector_error = read(vector(), -1).unwrap_err();
    let lista_error = read(lista(), -1).unwrap_err();
    assert_eq!(
        failure_identity(&vector_error),
        failure_identity(&lista_error)
    );
}

#[test]
fn in_range_lane_write_replaces_only_that_lane() {
    let Value::Vector(cells) = write(vector(), 1).unwrap() else {
        panic!("a lane write keeps the vector carrier");
    };
    assert_eq!(
        *cells.borrow(),
        vec![Value::Float(1.0), Value::Float(9.0), Value::Float(3.0)]
    );
}

#[test]
fn out_of_range_lane_write_fails_like_a_lista() {
    for at in [3, -1] {
        let vector_error = write(vector(), at).unwrap_err();
        let lista_error = write(lista(), at).unwrap_err();
        assert_eq!(
            failure_identity(&vector_error),
            failure_identity(&lista_error)
        );
    }
}
