//! Runtime conversio must round at the requested float width (need,
//! GAPS #1, ruling 26(d)).

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirFunction, MirFunctionId, MirProgram, MirType, MirValidationContext,
};
use radix_types::{NumericWidth, Primitive, TypeId, TypeTable};

fn convert(value: Value, width: NumericWidth) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let _target_ty = type_table.sized_numeric(Primitive::Fractus, width);
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
    let function = program.functions.first().expect("throwaway function");
    runner.push_frame(function.id, MirBlockId(0));
    runner.convert_to_fractus(value, &[], width, None, Span::default())
}

#[test]
fn f16_bf16_narrow_rounds_each_source_at_target_width() {
    assert_eq!(
        convert(Value::Float(0.1), NumericWidth::F16).expect("float to f16"),
        Value::Float(0.099_975_585_937_5),
    );
    assert_eq!(
        convert(Value::Float(0.1), NumericWidth::BF16).expect("float to bf16"),
        Value::Float(0.100_097_656_25),
    );
    assert_eq!(
        convert(Value::Float(2049.0), NumericWidth::F16).expect("float tie to f16"),
        Value::Float(2048.0),
    );
    assert_eq!(
        convert(Value::Float(65_519.0), NumericWidth::F16).expect("in-range float to f16"),
        Value::Float(65_504.0),
    );
    assert_eq!(
        convert(Value::Int(2049), NumericWidth::F16).expect("int to f16"),
        Value::Float(2048.0),
    );
    assert_eq!(
        convert(Value::Int(16_777_217), NumericWidth::BF16).expect("int to bf16"),
        Value::Float(16_777_216.0),
    );
    // This integer is one above an exact bf16 midpoint. Converting through
    // f64 first lands on the midpoint and rounds down; exact rounding goes up.
    let above_bf16_midpoint = 9_007_199_254_740_992_i64 + 35_184_372_088_832 + 1;
    assert_eq!(
        convert(Value::Int(above_bf16_midpoint), NumericWidth::BF16)
            .expect("single-round int to bf16"),
        Value::Float(9_077_567_998_918_656.0),
    );
    assert_eq!(
        convert(Value::Str("0.1".to_owned()), NumericWidth::F16).expect("text to f16"),
        Value::Float(0.099_975_585_937_5),
    );
    assert_eq!(
        convert(Value::Float(0.1), NumericWidth::F32).expect("float to f32"),
        Value::Float(0.1_f64 as f32 as f64),
    );
}
