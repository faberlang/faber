//! D4 runner pins for scaled decimal carriers and the D3 conversion matrix.

use super::*;
use crate::value::{decimal_numeric_binop, round_half_even_quotient};
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBinOp, MirBlockId, MirConstant, MirConversion, MirConversionFlavor, MirFunction,
    MirFunctionId, MirOperand, MirProgram, MirType, MirValidationContext,
};
use radix_types::{NumericWidth, Primitive, Type, TypeTable};

fn run_conversion(source: Type, target: Type, value: Value) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let source_ty = types.intern(source);
    let target_ty = types.intern(target);
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
        store: None,
        flavor: MirConversionFlavor::Runtime,
        source_ty: MirType::semantic(source_ty),
        target_ty: MirType::semantic(target_ty),
        params: vec![],
        recovery: None,
        struct_defaults: vec![],
    };
    runner.eval_runtime_convert(&conversion, value, Span::default())
}

fn decimal(width: NumericWidth) -> Type {
    Type::SizedNumeric(Primitive::Numerus, width)
}

fn integer(width: NumericWidth) -> Type {
    Type::SizedNumeric(Primitive::Numerus, width)
}

fn fractus(width: NumericWidth) -> Type {
    Type::SizedNumeric(Primitive::Fractus, width)
}

#[test]
fn decimal_arithmetic_is_exact_until_the_store() {
    let span = Span::default();
    let scale = 100_000_000_i64;
    let binop = |l: Value, r: Value, op: MirBinOp| {
        decimal_numeric_binop(l, r, op, NumericWidth::D64, span).expect("d64 binop")
    };
    // Stored-shaped results stay stored carriers.
    assert_eq!(
        binop(
            Value::Int(42 * scale / 10),
            Value::Int(scale / 10),
            MirBinOp::Add
        ),
        Value::Int(43 * scale / 10),
    );
    assert_eq!(
        binop(Value::Int(42 * scale), Value::Int(2 * scale), MirBinOp::Mul),
        Value::Int(84 * scale),
    );
    // 1.00000001 x 0.5 = 0.500000005 exactly: the product keeps its scale
    // (9 after dropping zeros) and only the store rounds it, half-even, to
    // 0.50000000.
    let midpoint = binop(
        Value::Int(100_000_001),
        Value::Int(50_000_000),
        MirBinOp::Mul,
    );
    assert_eq!(midpoint, Value::Dec(500_000_005, 9));
    assert_eq!(
        crate::decimal::store_carrier(500_000_005, 9),
        Some(50_000_000)
    );
    // The odd side of the tie rounds up to the even carrier.
    assert_eq!(
        crate::decimal::store_carrier(500_000_015, 9),
        Some(50_000_002)
    );
    // Scales grow through `*` and compare exactly.
    let p = Value::Int(1_999_000_000); // 19.99
    let r = Value::Int(7_250_000); // 0.0725
    let pr = binop(p.clone(), r.clone(), MirBinOp::Mul);
    assert_eq!(pr, Value::Int(144_927_500)); // 1.449275
    let prr = binop(pr, r, MirBinOp::Mul);
    assert_eq!(prr, Value::Dec(1_050_724_375, 10)); // 0.1050724375
    assert_eq!(crate::decimal::format(1_050_724_375, 10), "0.1050724375");
    assert_eq!(
        crate::decimal::store_carrier(1_050_724_375, 10),
        Some(10_507_244)
    );
    // `/` rounds half-even to the larger operand scale.
    let third = binop(Value::Int(scale), Value::Int(3 * scale), MirBinOp::Div);
    assert_eq!(third, Value::Int(33_333_333));
    let wide = binop(Value::Dec(10, 16), Value::Int(3 * scale), MirBinOp::Div);
    assert_eq!(wide, Value::Dec(3, 16));
    assert_eq!(round_half_even_quotient(5, 2), Some(2));
    assert_eq!(round_half_even_quotient(7, 2), Some(4));
    assert_eq!(round_half_even_quotient(-5, 2), Some(-2));
    assert_eq!(round_half_even_quotient(-7, 2), Some(-4));
}

#[test]
fn decimal_runtime_binop_normalizes_float_constants() {
    let mut types = TypeTable::new();
    let d64 = types.sized_numeric(Primitive::Numerus, NumericWidth::D64);
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
        return_ty: MirType::semantic(d64),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let result = runner
        .eval_binop(
            MirBinOp::Add,
            &MirOperand::Constant(MirConstant::Float(4.2)),
            &MirOperand::Constant(MirConstant::Float(0.1)),
            MirType::semantic(d64),
            Span::default(),
        )
        .expect("4.2 + 0.1 d64");
    assert_eq!(result, Value::Int(430_000_000));
}

#[test]
fn decimal_conversio_matrix_rounds() {
    let d64 = decimal(NumericWidth::D64);
    let i64 = integer(NumericWidth::I64);
    let u32 = integer(NumericWidth::U32);
    let f64 = fractus(NumericWidth::F64);

    assert_eq!(
        run_conversion(
            Type::Primitive(Primitive::Textus),
            d64.clone(),
            Value::Str("2.000000005".to_owned()),
        )
        .expect("textus to d64 midpoint"),
        Value::Int(200_000_000),
    );
    assert_eq!(
        run_conversion(
            Type::Primitive(Primitive::Textus),
            d64.clone(),
            Value::Str("2.000000015".to_owned()),
        )
        .expect("textus to d64 odd midpoint"),
        Value::Int(200_000_002),
    );
    assert_eq!(
        run_conversion(i64.clone(), d64.clone(), Value::Int(42),).expect("integer to d64"),
        Value::Int(4_200_000_000),
    );
    assert_eq!(
        run_conversion(fractus(NumericWidth::F64), d64.clone(), Value::Float(4.2),)
            .expect("fractus to d64"),
        Value::Int(420_000_000),
    );
    assert_eq!(
        run_conversion(d64.clone(), i64.clone(), Value::Int(450_000_000),)
            .expect("4.5 to integer uses even tie"),
        Value::Int(4),
    );
    assert_eq!(
        run_conversion(d64.clone(), u32, Value::Int(350_000_000),)
            .expect("3.5 to unsigned integer uses even tie"),
        Value::UInt(4),
    );
    assert_eq!(
        run_conversion(d64.clone(), f64, Value::Int(420_000_000)).expect("d64 to fractus"),
        Value::Float(4.2),
    );
    assert_eq!(
        run_conversion(
            d64,
            Type::Primitive(Primitive::Textus),
            Value::Int(420_000_000),
        )
        .expect("d64 to textus display"),
        Value::Str("4.2".to_owned()),
    );
}

#[test]
fn decimal_overflow_traps_at_the_128_bit_bound_and_the_store() {
    // Past the d64 range an intermediate is still exact; only the store traps.
    let sum = decimal_numeric_binop(
        Value::Int(i64::MAX),
        Value::Int(2),
        MirBinOp::Add,
        NumericWidth::D64,
        Span::default(),
    )
    .expect("exact sum beyond the d64 range");
    assert_eq!(sum, Value::Dec(i128::from(i64::MAX) + 2, 8));
    assert_eq!(
        crate::decimal::store_carrier(i128::from(i64::MAX) + 2, 8),
        None
    );
    // The intermediate itself is bounded by the 128-bit carrier.
    let error = decimal_numeric_binop(
        Value::Dec(i128::MAX, 0),
        Value::Int(2),
        MirBinOp::Mul,
        NumericWidth::D64,
        Span::default(),
    )
    .expect_err("i128 carrier overflow");
    assert_eq!(error.issue, "runner_numerus_overflow");
}

#[test]
fn negative_signed_to_unsigned_conversion_fails_closed() {
    let i32 = integer(NumericWidth::I32);
    let u32 = integer(NumericWidth::U32);
    let error = run_conversion(i32.clone(), u32.clone(), Value::Int(-2))
        .expect_err("negative signed value has no unsigned carrier");
    assert_eq!(
        error.issue,
        "runner_numerus_to_numerus_conversion_out_of_range",
    );
    assert_eq!(
        run_conversion(i32, u32, Value::Int(5)).expect("non-negative converts"),
        Value::Int(5),
    );
}

#[test]
fn variant_payload_fields_display_at_their_declared_width() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let cost = interner.intern("value");
    let ratio = interner.intern("ratio");
    let count = interner.intern("count");
    let def = radix_hir::DefId(21);
    let d64 = types.intern(decimal(NumericWidth::D64));
    let f32 = types.intern(fractus(NumericWidth::F32));
    let int = types.intern(integer(NumericWidth::I64));
    let mut context = MirValidationContext::new(&types);
    context.variant_fields.insert(
        def,
        [
            (cost, MirType::semantic(d64)),
            (ratio, MirType::semantic(f32)),
            (count, MirType::semantic(int)),
        ]
        .into_iter()
        .collect(),
    );
    let widths = index_field_float_widths(&context, &types);
    assert_eq!(widths.get(&(def, cost)), Some(&NumericWidth::D64));
    assert_eq!(widths.get(&(def, ratio)), Some(&NumericWidth::F32));
    assert_eq!(widths.get(&(def, count)), None);

    let mut names = value::DisplayNames::default();
    names.insert(def, "Item.Cost".to_owned());
    let structs = handroll::FxHashMap::default();
    let mut variants = handroll::FxHashMap::default();
    variants.insert(def, vec![cost, ratio, count]);
    let order = value::FieldOrder {
        float_widths: &widths,
        names: &names,
        ..value::FieldOrder::new(&structs, &variants)
    };
    let fields: handroll::FxHashMap<_, _> = [
        (cost, Value::Int(125_000_000)),
        (ratio, Value::Float(f64::from(0.1_f32))),
        (count, Value::Int(3)),
    ]
    .into_iter()
    .collect();
    let variant = Value::EnumVariant(def, std::rc::Rc::new(std::cell::RefCell::new(fields)));
    assert_eq!(
        variant
            .display(&interner, order, Span::default())
            .expect("variant display"),
        "Item.Cost { value = 1.25, ratio = 0.1, count = 3 }"
    );
}

#[test]
fn tuple_prints_as_iuncta_at_every_depth_with_per_element_widths() {
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let d64 = types.intern(decimal(NumericWidth::D64));
    let int = types.intern(integer(NumericWidth::I64));
    let pair = types.tuple(vec![d64, int]);
    let nested = types.tuple(vec![pair, int]);
    let list = types.array(pair);
    let option = types.option(pair);

    // The declared tuple type tells the two `Int` carriers apart, through
    // collections and options, and nests.
    let shape = tuple_parts(pair, &types, 0).expect("tuple shape");
    assert_eq!(shape.0[0].width, Some(NumericWidth::D64));
    assert_eq!(shape.0[1].width, None);
    assert_eq!(tuple_parts(list, &types, 0), Some(shape.clone()));
    assert_eq!(tuple_parts(option, &types, 0), Some(shape.clone()));
    assert_eq!(tuple_parts(d64, &types, 0), None);
    let outer = tuple_parts(nested, &types, 0).expect("nested shape");
    assert_eq!(outer.0[0].tuple, Some(shape.clone()));

    let structs = handroll::FxHashMap::default();
    let variants = handroll::FxHashMap::default();
    let order = value::FieldOrder::new(&structs, &variants);
    let tuple = Value::Tuple(vec![Value::Int(125_000_000), Value::Int(4)]);
    let show = |value: &Value, shape: Option<&value::TupleParts>| {
        value
            .display(&interner, order.with_tuple(shape), Span::default())
            .expect("display")
    };
    assert_eq!(show(&tuple, Some(&shape)), "iuncta [1.25, 4]");
    let items = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(vec![
        tuple.clone(),
    ])));
    assert_eq!(show(&items, Some(&shape)), "[iuncta [1.25, 4]]");
    let wrapped = Value::Tuple(vec![tuple.clone(), Value::Str("x".into())]);
    let wrapped_shape = value::TupleParts(vec![
        value::TuplePart {
            width: None,
            bits: None,
            tuple: Some(shape.clone()),
        },
        value::TuplePart {
            width: None,
            bits: None,
            tuple: None,
        },
    ]);
    assert_eq!(
        show(&wrapped, Some(&wrapped_shape)),
        "iuncta [iuncta [1.25, 4], \"x\"]"
    );

    // An interval is the same carrier with no tuple shape: its display is
    // unchanged.
    let interval = Value::Tuple(vec![Value::Int(1), Value::Int(3), Value::Bool(false)]);
    assert_eq!(show(&interval, None), format!("{interval:?}"));
}

#[test]
fn tuple_fields_index_by_declared_type() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let pair_field = interner.intern("pair");
    let count_field = interner.intern("count");
    let def = radix_hir::DefId(31);
    let d64 = types.intern(decimal(NumericWidth::D64));
    let int = types.intern(integer(NumericWidth::I64));
    let pair = types.tuple(vec![d64, int]);
    let mut context = MirValidationContext::new(&types);
    context.struct_fields.insert(
        def,
        [
            (pair_field, MirType::semantic(pair)),
            (count_field, MirType::semantic(int)),
        ]
        .into_iter()
        .collect(),
    );
    let fields = index_field_tuple_parts(&context, &types);
    assert!(fields.contains_key(&(def, pair_field)));
    assert!(!fields.contains_key(&(def, count_field)));

    let mut names = value::DisplayNames::default();
    names.insert(def, "Box".to_owned());
    let mut structs = handroll::FxHashMap::default();
    structs.insert(def, vec![pair_field, count_field]);
    let variants = handroll::FxHashMap::default();
    let order = value::FieldOrder {
        names: &names,
        tuple_fields: &fields,
        ..value::FieldOrder::new(&structs, &variants)
    };
    let values: handroll::FxHashMap<_, _> = [
        (
            pair_field,
            Value::Tuple(vec![Value::Int(125_000_000), Value::Int(3)]),
        ),
        (count_field, Value::Int(2)),
    ]
    .into_iter()
    .collect();
    let boxed = Value::Struct(def, std::rc::Rc::new(std::cell::RefCell::new(values)));
    assert_eq!(
        boxed
            .display(&interner, order, Span::default())
            .expect("genus display"),
        "Box { pair = iuncta [1.25, 3], count = 2 }"
    );
}
