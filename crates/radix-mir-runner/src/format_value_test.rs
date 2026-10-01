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

fn decimal(value: &Value, spec: &str) -> String {
    let width = Some(radix_types::NumericWidth::D64);
    crate::runtime::format_value(value, spec, width, Span::default()).expect("decimal formats")
}

#[test]
fn a_stored_decimal_formats_from_its_scale_8_carrier() {
    // `12.5`, `0.25`, `0.35` stored as scale-8 carriers.
    let p = Value::Int(1_250_000_000);
    assert_eq!(decimal(&p, ".2"), "12.50");
    assert_eq!(decimal(&Value::Int(25_000_000), ".1"), "0.2");
    assert_eq!(decimal(&Value::Int(35_000_000), ".1"), "0.4");
    assert_eq!(decimal(&p, ">8"), "    12.5");
    assert_eq!(decimal(&p, ""), "12.5");
}

#[test]
fn an_unstored_decimal_formats_exactly_at_its_own_scale() {
    // `1.5 * 2.25` is exactly 3.375 at scale 3: the tie rounds to even.
    assert_eq!(decimal(&Value::Dec(3375, 3), ".2"), "3.38");
    assert_eq!(decimal(&Value::Dec(3125, 3), ".2"), "3.12");
    assert_eq!(decimal(&Value::Dec(-3375, 3), ".2"), "-3.38");
}

#[test]
fn an_integer_without_decimal_width_still_formats_as_an_integer() {
    let value = Value::Int(1_250_000_000);
    let text = crate::runtime::format_value(&value, ".2", None, Span::default()).expect("formats");
    assert_eq!(text, "1250000000.00");
}
