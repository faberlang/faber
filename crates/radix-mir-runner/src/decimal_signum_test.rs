//! D11.7 runner pins for `signum` on a decimal receiver.
//!
//! A stored `d64` is a scale-8 `Value::Int(carrier)` (D11.7), so a `signum`
//! that returns the carrier `±1` under a d64 result type would mean `±10^-8`
//! — not the decimal sign. The op returns the exact decimal `-1`/`0`/`1`
//! (scale 0) instead, while a non-decimal receiver keeps the exact integer
//! path.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirOperand, MirProgram,
    MirRuntimeCall, MirType, MirValidationContext,
};
use radix_types::{NumericWidth, Primitive, Type, TypeTable};

/// One `MirCollectionOp::Signum` over a receiver parked in frame value `v0`,
/// with `return_ty` as the declared result type.
fn eval_signum(receiver: Value, return_ty: Type) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let return_ty = types.intern(return_ty);
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(return_ty),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner
        .frames
        .last_mut()
        .expect("signum frame")
        .values
        .insert(MirValueId(0), receiver);

    runner.eval_runtime_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(MirCollectionOp::Signum),
            args: vec![MirOperand::Value(MirValueId(0))],
            return_ty: MirType::semantic(return_ty),
        },
        Span::default(),
    )
}

fn d64() -> Type {
    Type::SizedNumeric(Primitive::Numerus, NumericWidth::D64)
}

#[test]
fn decimal_signum_returns_the_decimal_sign() {
    // Stored d64 carrier at scale 8: -2.25 is the carrier -225_000_000.
    assert_eq!(
        eval_signum(Value::Int(-225_000_000), d64()).expect("signum"),
        Value::Int(-100_000_000),
    );
    assert_eq!(
        eval_signum(Value::Int(150_000_000), d64()).expect("signum"),
        Value::Int(100_000_000),
    );
    assert_eq!(
        eval_signum(Value::Int(0), d64()).expect("signum"),
        Value::Int(0),
    );
}

#[test]
fn decimal_signum_reads_an_exact_intermediate() {
    // -1.5 as an exact intermediate (carrier -15 at scale 1).
    assert_eq!(
        eval_signum(Value::Dec(-15, 1), d64()).expect("signum"),
        Value::Int(-100_000_000),
    );
}

#[test]
fn non_decimal_signum_keeps_the_exact_integer_path() {
    assert_eq!(
        eval_signum(Value::Int(-7), Type::Primitive(Primitive::Numerus)).expect("signum"),
        Value::Int(-1),
    );
}
