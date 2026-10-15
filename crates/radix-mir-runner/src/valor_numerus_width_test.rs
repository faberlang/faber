//! `valor ↦ numerus<W>` applies the same magnitude check as `int ↦ W` (need
//! 25d05022, CTO ruling 2026-10-15): a payload that does not fit `W` fails on
//! the `⇥` channel, where `⊥` supplies the default, and a negative payload
//! into an unsigned width fails the same way. Assertions name the issue, never
//! the message text.

use super::*;
use handroll::Magnus;
use radix_lexer::{Interner, Span};
use radix_mir::{MirConversion, MirConversionFlavor, MirValidationContext};
use radix_types::{NumericWidth, Primitive, Type, TypeTable};

fn extract_to(
    target: Type,
    value: Value,
    recovery: Option<MirOperand>,
) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let source = types.primitive(Primitive::Valor);
    let target = types.intern(target);
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let program = MirProgram::new();
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.eval_runtime_convert(
        &MirConversion {
            store: None,
            flavor: MirConversionFlavor::Runtime,
            source_ty: MirType::semantic(source),
            target_ty: MirType::semantic(target),
            params: vec![],
            recovery,
            struct_defaults: vec![],
        },
        value,
        Span::default(),
    )
}

fn sized(width: NumericWidth) -> Type {
    Type::SizedNumeric(Primitive::Numerus, width)
}

fn magnus(digits: &str) -> Value {
    Value::Magnus(std::rc::Rc::new(
        Magnus::parse_decimal(digits).expect("decimal"),
    ))
}

#[test]
fn valor_numerus_width_extracts_in_range_payload() {
    assert_eq!(
        extract_to(sized(NumericWidth::U8), Value::Int(200), None).expect("in range"),
        Value::Int(200)
    );
    assert_eq!(
        extract_to(sized(NumericWidth::U8), Value::Int(255), None).expect("u8 boundary"),
        Value::Int(255)
    );
    assert_eq!(
        extract_to(sized(NumericWidth::I8), Value::Int(-128), None).expect("i8 boundary"),
        Value::Int(-128)
    );
}

#[test]
fn valor_numerus_width_overflow_fails_on_error_channel() {
    for (target, payload) in [
        (sized(NumericWidth::U8), 300),
        (sized(NumericWidth::I8), 128),
        (sized(NumericWidth::U8), -1),
    ] {
        let err = extract_to(target, Value::Int(payload), None).expect_err("must not fit");
        assert_eq!(err.issue, "runner_valor_to_numerus_conversion_failed");
    }
}

#[test]
fn valor_numerus_width_overflow_recovers_with_bottom() {
    assert_eq!(
        extract_to(
            sized(NumericWidth::U8),
            Value::Int(300),
            Some(MirOperand::Constant(MirConstant::Int(7)))
        )
        .expect("⊥ supplies the default"),
        Value::Int(7)
    );
}

#[test]
fn valor_numerus_width_unsized_and_inf_unchanged() {
    assert_eq!(
        extract_to(
            Type::Primitive(Primitive::Numerus),
            Value::Int(1_099_511_627_776),
            None
        )
        .expect("unsized i64"),
        Value::Int(1_099_511_627_776)
    );
    assert!(matches!(
        extract_to(sized(NumericWidth::Inf), magnus("1099511627776"), None)
            .expect("inf keeps every integer carrier"),
        Value::Magnus(_)
    ));
}
