//! Runner pins for `n ↦ ascii<N, Hex|Bin|Oct>`.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirFunction, MirFunctionId, MirProgram, MirType, MirValidationContext,
};
use radix_types::{IndexExpr, TypeId, TypeTable};

fn eval_pack(value: Value, hint: &str, width: u64) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let capacity = type_table.intern_index(IndexExpr::Literal(width));
    let _target_ty = type_table.bounded_ascii(capacity);
    let mut interner = Interner::new();
    let hint_sym = interner.intern(hint);
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
    runner.convert_to_ascii_format_pack(value, capacity, &[hint_sym], None, Span::default())
}

#[test]
fn hex_255_width_2_is_ff() {
    let result = eval_pack(Value::Int(255), "Hex", 2).expect("hex 255");
    assert_eq!(result, Value::Str("ff".to_owned()));
}

#[test]
fn hex_37_width_8_zero_pads() {
    let result = eval_pack(Value::Int(37), "Hex", 8).expect("hex 37");
    assert_eq!(result, Value::Str("00000025".to_owned()));
}

#[test]
fn bin_255_width_8() {
    let result = eval_pack(Value::Int(255), "Bin", 8).expect("bin 255");
    assert_eq!(result, Value::Str("11111111".to_owned()));
}

#[test]
fn oct_255_width_3() {
    let result = eval_pack(Value::Int(255), "Oct", 3).expect("oct 255");
    assert_eq!(result, Value::Str("377".to_owned()));
}

#[test]
fn hex_width_1_uses_ascii_byte() {
    let result = eval_pack(Value::Int(15), "Hex", 1).expect("hex 15");
    assert_eq!(result, Value::Ascii(b'f'));
}

#[test]
fn overflow_fails_closed() {
    let err = eval_pack(Value::Int(256), "Hex", 2).expect_err("256 needs 3 hex digits");
    assert_eq!(
        err.issue, "runner_ascii_pack_width_overflow",
        "expected overflow, got: {err:?}"
    );
}

#[test]
fn negative_fails_closed() {
    let err = eval_pack(Value::Int(-1), "Hex", 2).expect_err("negative source");
    assert_eq!(
        err.issue, "runner_ascii_pack_negative_source",
        "expected negative reject, got: {err:?}"
    );
}
