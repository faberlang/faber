//! Runner pins for octeti window ↦ sized int via Be/Le.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirFunction, MirFunctionId, MirProgram, MirType, MirValidationContext,
};
use radix_types::{NumericWidth, Primitive, TypeId, TypeTable};

fn eval_bytes(bytes: Vec<u8>, hint: &str, width: NumericWidth) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let _target_ty = type_table.sized_numeric(Primitive::Numerus, width);
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
    runner.convert_to_numerus(
        Value::Octeti(bytes),
        &[hint_sym],
        width,
        None,
        Span::default(),
    )
}

#[test]
fn octeti_endian_be_u32_is_deadbeef() {
    let result = eval_bytes(vec![0xde, 0xad, 0xbe, 0xef], "Be", NumericWidth::U32).expect("be u32");
    assert_eq!(result, Value::UInt(0xDEAD_BEEF));
}

#[test]
fn octeti_endian_le_u32_is_efbeadde() {
    let result = eval_bytes(vec![0xde, 0xad, 0xbe, 0xef], "Le", NumericWidth::U32).expect("le u32");
    assert_eq!(result, Value::UInt(0xEFBE_ADDE));
}

#[test]
fn octeti_endian_short_window_fails() {
    let err = eval_bytes(vec![0xde, 0xad, 0xbe], "Be", NumericWidth::U32)
        .expect_err("3-byte source must fail");
    assert_eq!(
        err.issue, "runner_octeti_endian_width_mismatch",
        "expected width mismatch, got: {err:?}"
    );
}
