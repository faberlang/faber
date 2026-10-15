//! D10.6 runner pins: an `ascii<1>` one-letter source routes through the
//! `littera` digit/`Code` rows, never the whole-string numerus parse.
// Test fixtures: exact small integer expectations from known scalar values.
#![allow(clippy::cast_precision_loss, clippy::cast_sign_loss)]

use super::*;
use radix_lexer::{Interner, Span, Symbol};
use radix_mir::{
    MirBlockId, MirConstant, MirConversion, MirConversionFlavor, MirFunction, MirFunctionId,
    MirOperand, MirProgram, MirType, MirValidationContext,
};
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};

fn run_conversion<S: FnOnce(&mut TypeTable) -> Type>(
    interner: &Interner,
    source_of: S,
    target: Type,
    params: Vec<Symbol>,
    value: Value,
    recovery: Option<MirOperand>,
) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let source = source_of(&mut types);
    let source_ty = types.intern(source);
    let target_ty = types.intern(target);
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
        return_ty: MirType::semantic(target_ty),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let conversion = MirConversion {
        store: None,
        flavor: MirConversionFlavor::Runtime,
        source_ty: MirType::semantic(source_ty),
        target_ty: MirType::semantic(target_ty),
        params,
        recovery,
        struct_defaults: vec![],
    };
    runner.eval_runtime_convert(&conversion, value, Span::default())
}

fn one_letter(types: &mut TypeTable) -> Type {
    let capacity = types.intern_index(IndexExpr::Literal(1));
    Type::BoundedAscii { capacity }
}

fn int() -> Type {
    Type::Primitive(Primitive::Numerus)
}

#[test]
fn ascii_digit_parses_its_digit() {
    let interner = Interner::new();
    assert_eq!(
        run_conversion(
            &interner,
            one_letter,
            int(),
            vec![],
            Value::Ascii(b'7'),
            None,
        )
        .expect("'7' parses as its digit"),
        Value::Int(7),
    );
    assert_eq!(
        run_conversion(
            &interner,
            |_| Type::Primitive(Primitive::Ascii),
            int(),
            vec![],
            Value::Ascii(b'7'),
            None,
        )
        .expect("primitive ascii source parses as its digit"),
        Value::Int(7),
    );
}

#[test]
fn ascii_non_digit_recovers_or_fails() {
    let interner = Interner::new();
    assert_eq!(
        run_conversion(
            &interner,
            one_letter,
            int(),
            vec![],
            Value::Ascii(b'a'),
            Some(MirOperand::Constant(MirConstant::Int(-1))),
        )
        .expect("recovery supplies the value for a non-digit"),
        Value::Int(-1),
    );
    let result = run_conversion(
        &interner,
        one_letter,
        int(),
        vec![],
        Value::Ascii(b'a'),
        None,
    );
    assert!(result.is_err(), "non-digit without recovery fails");
}

#[test]
fn ascii_code_point_hint_reads_the_scalar() {
    let mut interner = Interner::new();
    let params = vec![interner.intern("Code")];
    assert_eq!(
        run_conversion(
            &interner,
            one_letter,
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32),
            params,
            Value::Ascii(b'a'),
            None,
        )
        .expect("the Code hint reads the scalar value"),
        Value::UInt(97),
    );
}
