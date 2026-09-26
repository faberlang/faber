//! Runner pins for the `¶` format operator (D2.1, D2.5, D2.7).
//!
//! The runner executes the shared reference renderer; these pin that the
//! runtime call reaches it for every value family.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirConstant, MirFunction, MirFunctionId, MirIntrinsic, MirOperand, MirProgram,
    MirRuntimeCall, MirType, MirValidationContext,
};
use radix_types::{Primitive, TypeTable};

fn format(value: MirConstant, spec: &str) -> Result<Value, RunnerError> {
    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let mut interner = Interner::new();
    let spec = interner.intern(spec);
    let value = match value {
        MirConstant::String(_) => MirConstant::String(interner.intern("Aurelia")),
        other => other,
    };
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
        return_ty: MirType::semantic(textus),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::FormatValue { spec },
        args: vec![MirOperand::Constant(value)],
        return_ty: MirType::semantic(textus),
    };
    runner.eval_runtime_call(&call, Span::default())
}

fn text(value: MirConstant, spec: &str) -> String {
    match format(value, spec) {
        Ok(Value::Str(text)) => text,
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn formats_each_value_family() {
    assert_eq!(text(MirConstant::Int(42), ".2"), "42.00");
    assert_eq!(text(MirConstant::Int(-42), "x"), "-2a");
    assert_eq!(text(MirConstant::UInt(u64::MAX), "x"), "ffffffffffffffff");
    assert_eq!(text(MirConstant::Float(12.5), "08.2"), "00012.50");
    assert_eq!(text(MirConstant::Float(3.0), ">6"), "   3.0");
    assert_eq!(text(MirConstant::Float(f64::NAN), ".2"), "NaN");
    assert_eq!(text(MirConstant::Float(f64::NEG_INFINITY), ""), "-∞");
    assert_eq!(
        text(MirConstant::String(radix_lexer::Symbol(0)), "*>6.3"),
        "***Aur"
    );
}

#[test]
fn a_spec_that_does_not_parse_is_an_internal_error() {
    assert!(format(MirConstant::Int(1), "%d").is_err());
}
