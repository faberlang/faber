//! Runner pins for the checker's index proofs (need, slice 4): a
//! `ProvenIndex` projection reads or writes its slot without the bounds test,
//! and an `Index` projection nothing proved keeps it.
//!
//! The absence of the check is observed where it matters: an index outside the
//! container is a structured runner error through `Index`, while the same
//! index through `ProvenIndex` on a list never builds that error (a proof that
//! turns out wrong is a compiler bug and panics in the read, not a fault).
//! Text and tensor stores have no separate test to omit and keep their
//! checked behaviour; a dense tensor read skips only the logical-extent compare.

use super::*;
use crate::value::TensorValue;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirConstant, MirFunction, MirFunctionId, MirOperand, MirProgram, MirProjection,
    MirType, MirValidationContext,
};
use radix_types::{TypeId, TypeTable};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn at(position: i64) -> MirOperand {
    MirOperand::Constant(MirConstant::Int(position))
}

/// Run `body` against a runner with one empty frame.
fn with_runner<R>(body: impl FnOnce(&mut Runner<'_, crate::host::BufferHost>) -> R) -> R {
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
    body(&mut runner)
}

fn list(values: &[i64]) -> Value {
    Value::Array(Rc::new(RefCell::new(
        values.iter().copied().map(Value::Int).collect(),
    )))
}

fn tensor(count: i64) -> Value {
    Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        (0..count).map(Value::Int).collect(),
        vec![count],
    ))))
}

fn read(value: Value, projection: &MirProjection) -> Result<Value, RunnerError> {
    with_runner(|runner| runner.project(value, projection, Span::default()))
}

fn write(value: Value, projection: &MirProjection, assigned: Value) -> Result<Value, RunnerError> {
    with_runner(|runner| runner.project_write(value, projection, assigned, Span::default()))
}

/// Whether the closure panics (a proven read of a position nothing proved).
fn panics<R>(body: impl FnOnce() -> R) -> bool {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = catch_unwind(AssertUnwindSafe(body));
    std::panic::set_hook(hook);
    outcome.is_err()
}

// ── lists ─────────────────────────────────────────────────────────────────

#[test]
fn a_proven_list_read_returns_its_element() {
    let projection = MirProjection::ProvenIndex(at(2));
    assert_eq!(
        read(list(&[10, 20, 30]), &projection).unwrap(),
        Value::Int(30)
    );
}

#[test]
fn an_unproven_list_read_past_the_end_still_faults() {
    let error = read(list(&[10, 20, 30]), &MirProjection::Index(at(3)))
        .expect_err("an unproven read past the end keeps its bounds fault");
    assert_eq!(error.code, "runner_internal");
    let error = read(list(&[10, 20, 30]), &MirProjection::Index(at(-1)))
        .expect_err("an unproven negative read keeps its fault");
    assert_eq!(error.code, "runner_internal");
}

#[test]
fn a_proven_list_read_builds_no_bounds_fault() {
    let outside = MirProjection::ProvenIndex(at(3));
    assert!(
        panics(|| read(list(&[10, 20, 30]), &outside)),
        "the proven path has no fault to return: a wrong proof panics"
    );
}

#[test]
fn a_proven_list_write_stores_without_the_bounds_test() {
    let projection = MirProjection::ProvenIndex(at(1));
    let stored = write(list(&[1, 2, 3]), &projection, Value::Int(9)).unwrap();
    assert_eq!(
        read(stored, &MirProjection::Index(at(1))).unwrap(),
        Value::Int(9)
    );
    assert!(panics(|| write(
        list(&[1, 2, 3]),
        &MirProjection::ProvenIndex(at(5)),
        Value::Int(0)
    )));
    let error = write(
        list(&[1, 2, 3]),
        &MirProjection::Index(at(5)),
        Value::Int(0),
    )
    .expect_err("an unproven write past the end keeps its fault");
    assert_eq!(error.code, "runner_internal");
}

// ── text ──────────────────────────────────────────────────────────────────

#[test]
fn a_proven_text_read_keeps_the_checked_scalar_rule() {
    // Text has no separate length test to omit: the proven read is the
    // checked read, so the ASCII and non-ASCII scalar rules are unchanged.
    for text in ["abc", "a\u{e9}"] {
        assert_eq!(
            read(Value::Str(text.into()), &MirProjection::ProvenIndex(at(1))).unwrap(),
            read(Value::Str(text.into()), &MirProjection::Index(at(1))).unwrap()
        );
    }
}

#[test]
fn a_text_read_past_the_end_still_faults_either_way() {
    read(Value::Str("abc".into()), &MirProjection::Index(at(3)))
        .expect_err("an unproven read past the end keeps its fault");
    read(Value::Str("abc".into()), &MirProjection::ProvenIndex(at(3)))
        .expect_err("a proven text read has no separate length test");
}

// ── tensors ───────────────────────────────────────────────────────────────

#[test]
fn a_proven_tensor_read_returns_its_element_and_a_write_still_lands() {
    let value = tensor(4);
    assert_eq!(
        read(value.clone(), &MirProjection::ProvenIndex(at(3))).unwrap(),
        Value::Int(3)
    );
    write(
        value.clone(),
        &MirProjection::ProvenIndex(at(3)),
        Value::Int(70),
    )
    .unwrap();
    assert_eq!(
        read(value.clone(), &MirProjection::Index(at(3))).unwrap(),
        Value::Int(70)
    );
}

#[test]
fn a_wrong_tensor_proof_is_still_memory_safe() {
    let value = tensor(4);
    read(value.clone(), &MirProjection::ProvenIndex(at(4))).expect_err("storage has no slot 4");
    write(value, &MirProjection::ProvenIndex(at(4)), Value::Int(0))
        .expect_err("storage has no slot 4");
}

#[test]
fn an_unproven_tensor_access_past_the_extent_still_faults() {
    let value = tensor(4);
    read(value.clone(), &MirProjection::Index(at(4)))
        .expect_err("an unproven read past the extent keeps its fault");
    write(value, &MirProjection::Index(at(4)), Value::Int(0))
        .expect_err("an unproven write past the extent keeps its fault");
}
