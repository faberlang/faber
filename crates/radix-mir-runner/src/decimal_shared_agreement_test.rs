//! S07-12 equality test: the runner's observable decimal behavior is
//! `radix_codegen_shared::decimal` — the one carrier-math copy — on the same
//! boundary texts (round-half-even midpoints, width-scale overflow, the
//! >=39-digit parse wrap, a beyond-i128 divisor, malformed text).

use crate::value::{Value, decimal_scale_factor, round_half_even_quotient};
use crate::{Runner, RunnerError};
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirConversion, MirConversionFlavor, MirFunction, MirFunctionId, MirProgram,
    MirType, MirValidationContext,
};
use radix_types::{NumericWidth, Primitive, Type, TypeTable};

fn run_text_to_decimal(text: &str, width: NumericWidth) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let source_ty = types.intern(Type::Primitive(Primitive::Textus));
    let target_ty = types.intern(Type::SizedNumeric(Primitive::Numerus, width));
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
        return_ty: MirType::semantic(target_ty),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let conversion = MirConversion {
        flavor: MirConversionFlavor::Runtime,
        source_ty: MirType::semantic(source_ty),
        target_ty: MirType::semantic(target_ty),
        params: vec![],
        recovery: None,
        struct_defaults: vec![],
    };
    runner.eval_runtime_convert(&conversion, Value::Str(text.to_owned()), Span::default())
}

#[test]
fn runner_text_to_decimal_agrees_with_shared_decimal_on_boundary_texts() {
    let cases: &[(&str, NumericWidth)] = &[
        // Round-half-even midpoints at both width scales.
        ("2.50000005", NumericWidth::D32),
        ("2.50000015", NumericWidth::D32),
        ("-2.50000015", NumericWidth::D32),
        ("0.0000000005", NumericWidth::D64),
        ("0.00000000015", NumericWidth::D64),
        // Plain carriers at both scales.
        ("4.2", NumericWidth::D32),
        ("4.2", NumericWidth::D64),
        ("1e-3", NumericWidth::D32),
        // Width-scale overflow: the shared tier's `None` is the runner's
        // explicit conversion error.
        ("1e40", NumericWidth::D32),
        ("2e29", NumericWidth::D64),
        // The >=39-digit parse wrap (S07-1b sibling boundary): a mantissa
        // beyond i128 is an explicit failure on both tiers.
        ("340282366920938463463374607431768211456", NumericWidth::D32),
        // A beyond-i128 divisor rounds to the shared tier's reasoned zero.
        ("1e-100", NumericWidth::D32),
        // Malformed text fails on both tiers.
        ("4.2eX", NumericWidth::D32),
    ];

    for (text, width) in cases {
        let observed = run_text_to_decimal(text, *width);
        match radix_codegen_shared::decimal::decimal_text_to_carrier(text, *width) {
            Some(expected) => assert_eq!(
                observed.unwrap_or_else(|err| panic!("text {text:?} at {width:?}: {err:?}")),
                Value::Int(expected),
                "text {text:?} at {width:?}: runner must yield the shared tier's carrier"
            ),
            None => assert!(
                observed.is_err(),
                "text {text:?} at {width:?}: shared tier fails explicitly, so must the runner"
            ),
        }
    }
}

#[test]
fn runner_decimal_helper_symbols_agree_with_the_shared_crate() {
    // `crate::value` re-exports the shared symbols (S07-12); the runner-visible
    // helpers and the shared crate agree on every boundary input, including
    // the checked-overflow `None`s.
    for width in [NumericWidth::D32, NumericWidth::D64, NumericWidth::I32] {
        assert_eq!(
            decimal_scale_factor(width),
            radix_codegen_shared::decimal::decimal_scale_factor(width),
            "scale factor must be the shared crate's for {width:?}"
        );
    }
    for (numerator, denominator) in [
        (5_i128, 2_i128),
        (7, 2),
        (-5, 2),
        (-7, 2),
        (6, 2),
        (1, 0),
        (i128::MAX, 2),
    ] {
        assert_eq!(
            round_half_even_quotient(numerator, denominator),
            radix_codegen_shared::decimal::round_half_even_quotient(numerator, denominator),
            "quotient must be the shared crate's for {numerator}/{denominator}"
        );
    }
}
