//! inf track U4: the runner's unbounded integer (`inf`) carrier — canonical
//! values, exact operations, comparisons and keys, conversions, stores, and
//! the resource ceilings (inf-delivery rulings 36-38, 40, 42, 45).

#![allow(
    clippy::float_cmp,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]

use super::*;
use crate::exact_int::{cmp_numeric, exact_bitnot, exact_int_binop, exact_neg};
use crate::inf::{
    RUNNER_INF_BIT_CEILING, RUNNER_ISSUE_INF_SIZE_EXHAUSTED, clamp_to_width, cmp_magnus_float,
    from_magnus, to_float, to_magnus, wrap_to_width,
};
use crate::value::{Value, compare_sort_values, compare_values, values_equal};
use handroll::Magnus;
use radix_hir::{HirStoreKind, HirStoreSite};
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBinOp, MirBlockId, MirConstant, MirConversion, MirConversionFlavor, MirFunction,
    MirFunctionId, MirOperand, MirProgram, MirType, MirValidationContext,
};
use radix_types::{NumericWidth, Primitive, Type, TypeTable};
use std::cmp::Ordering;

const INF: NumericWidth = NumericWidth::Inf;

fn big(text: &str) -> Value {
    from_magnus(Magnus::parse_decimal(text).expect("decimal"))
}

fn exact(value: &Value) -> Magnus {
    to_magnus(value).expect("integer carrier")
}

fn op(op: MirBinOp, a: &Value, b: &Value) -> Result<Value, RunnerError> {
    exact_int_binop(a, b, op, INF, Span::default()).expect("integer carriers")
}

fn digits(value: &Value) -> String {
    exact(value).to_string()
}

// ---------------------------------------------------------------------------
// Canonical carrier (ruling 46): `Magnus` only beyond [i64::MIN, u64::MAX].
// ---------------------------------------------------------------------------

#[test]
fn the_carrier_is_canonical_at_every_64_bit_boundary() {
    assert_eq!(
        from_magnus(Magnus::from_i64(i64::MIN)),
        Value::Int(i64::MIN)
    );
    assert_eq!(from_magnus(Magnus::from_i64(-1)), Value::Int(-1));
    assert_eq!(
        from_magnus(Magnus::from_i64(i64::MAX)),
        Value::Int(i64::MAX)
    );
    assert_eq!(
        from_magnus(Magnus::from_u64(u64::from(u32::MAX))),
        Value::Int(i64::from(u32::MAX))
    );
    assert_eq!(
        from_magnus(Magnus::from_u64(1 << 63)),
        Value::UInt(1 << 63),
        "above i64::MAX is UInt"
    );
    assert_eq!(
        from_magnus(Magnus::from_u64(u64::MAX)),
        Value::UInt(u64::MAX)
    );
    assert!(matches!(
        from_magnus(Magnus::from_i128(i128::from(u64::MAX) + 1)),
        Value::Magnus(_)
    ));
    assert!(matches!(
        from_magnus(Magnus::from_i128(i128::from(i64::MIN) - 1)),
        Value::Magnus(_)
    ));
}

#[test]
fn arithmetic_results_renormalize_back_into_the_small_carriers() {
    let beyond = big("18446744073709551616");
    let back = op(MirBinOp::Sub, &beyond, &Value::Int(1)).expect("sub");
    assert_eq!(back, Value::UInt(u64::MAX), "2^64 - 1 is a u64 again");
    let zero = op(MirBinOp::Sub, &beyond, &beyond).expect("sub");
    assert_eq!(zero, Value::Int(0));
}

// ---------------------------------------------------------------------------
// Ruling 36: exact arithmetic, floor division, two's-complement bitwise ops.
// ---------------------------------------------------------------------------

fn grid() -> Vec<i128> {
    let edges: [i128; 14] = [
        0,
        1,
        2,
        3,
        7,
        255,
        256,
        i128::from(u64::MAX),
        i128::from(u64::MAX) + 1,
        i128::from(i64::MIN),
        i128::from(i64::MIN) - 1,
        1 << 100,
        (1 << 120) + 12_345,
        123_456_789_012_345_678_901_234_567_890,
    ];
    let mut out = Vec::new();
    for &e in &edges {
        out.push(e);
        out.push(-e);
    }
    out
}

#[test]
fn operations_match_i128_on_a_grid_inside_its_range() {
    for &a in &grid() {
        for &b in &grid() {
            let (va, vb) = (
                from_magnus(Magnus::from_i128(a)),
                from_magnus(Magnus::from_i128(b)),
            );
            let check = |operation: MirBinOp, expected: Option<i128>| {
                let got = op(operation, &va, &vb);
                match expected {
                    Some(want) => assert_eq!(
                        digits(
                            &got.unwrap_or_else(|e| panic!("{a} {operation:?} {b}: {}", e.issue))
                        ),
                        want.to_string(),
                        "{a} {operation:?} {b}"
                    ),
                    None => {
                        got.expect_err("division by zero traps");
                    }
                }
            };
            if let Some(sum) = a.checked_add(b) {
                check(MirBinOp::Add, Some(sum));
            }
            if let Some(difference) = a.checked_sub(b) {
                check(MirBinOp::Sub, Some(difference));
            }
            if let Some(product) = a.checked_mul(b) {
                check(MirBinOp::Mul, Some(product));
            }
            if b == 0 {
                check(MirBinOp::Div, None);
                check(MirBinOp::Mod, None);
            } else {
                check(MirBinOp::Div, Some(crate::exact_int::floor_div(a, b)));
                check(
                    MirBinOp::Mod,
                    Some(crate::exact_int::floor_rem_checked(a, b)),
                );
            }
            check(MirBinOp::BitAnd, Some(a & b));
            check(MirBinOp::BitOr, Some(a | b));
            check(MirBinOp::BitXor, Some(a ^ b));
        }
    }
}

#[test]
fn shifts_are_exact_and_the_right_shift_floors() {
    let one = Value::Int(1);
    assert_eq!(
        digits(&op(MirBinOp::Shl, &one, &Value::Int(200)).expect("shl")),
        "1606938044258990275541962092341162602522202993782792835301376"
    );
    let neg = Value::Int(-1);
    let shifted = op(MirBinOp::Shl, &neg, &Value::Int(100)).expect("shl");
    assert_eq!(
        digits(&op(MirBinOp::Shr, &shifted, &Value::Int(98)).expect("shr")),
        "-4"
    );
    assert_eq!(
        op(MirBinOp::Shr, &Value::Int(-5), &Value::Int(1000)).expect("shr"),
        Value::Int(-1),
        "a huge right shift saturates at the sign"
    );
    assert_eq!(
        op(MirBinOp::Shr, &big("18446744073709551616"), &Value::Int(64)).expect("shr"),
        Value::Int(1)
    );
    assert_eq!(
        op(MirBinOp::Shl, &Value::Int(0), &Value::UInt(u64::MAX)).expect("zero shifts to zero"),
        Value::Int(0)
    );
}

#[test]
fn a_negative_shift_count_is_the_runtime_trap() {
    for operation in [MirBinOp::Shl, MirBinOp::Shr] {
        let error = op(operation, &Value::Int(1), &Value::Int(-1)).expect_err("negative count");
        assert_eq!(error.issue, "runner_negative_shift_count");
    }
}

#[test]
fn a_zero_divisor_is_the_division_trap() {
    for operation in [MirBinOp::Div, MirBinOp::Mod] {
        let error = op(operation, &big("18446744073709551616"), &Value::Int(0)).expect_err("zero");
        assert_eq!(error.issue, "runner_numerus_division_failed");
    }
}

#[test]
fn unary_minus_and_bitnot_are_exact() {
    let beyond = big("18446744073709551616");
    let neg = exact_neg(&beyond, INF, Span::default())
        .expect("int")
        .expect("neg");
    assert_eq!(digits(&neg), "-18446744073709551616");
    let not = exact_bitnot(&beyond, INF, Span::default())
        .expect("int")
        .expect("not");
    assert_eq!(digits(&not), "-18446744073709551617");
    // A small inf operand takes the same path (the result type decides).
    let not = exact_bitnot(&Value::Int(-7), INF, Span::default())
        .expect("int")
        .expect("not");
    assert_eq!(not, Value::Int(6));
    let neg = exact_neg(&Value::Int(i64::MIN), INF, Span::default())
        .expect("int")
        .expect("neg");
    assert_eq!(
        neg,
        Value::UInt(1 << 63),
        "-i64::MIN is exact, no overflow trap"
    );
}

#[test]
fn a_bounded_result_width_keeps_the_64_bit_cap() {
    // The bignum path is selected by the `inf` result width or a Magnus
    // operand, never by magnitude: bounded arithmetic still traps at the cap.
    let error = exact_int_binop(
        &Value::UInt(u64::MAX),
        &Value::Int(1),
        MirBinOp::Add,
        NumericWidth::I64,
        Span::default(),
    )
    .expect("integer carriers")
    .expect_err("u64::MAX + 1 traps in a bounded expression");
    assert_eq!(error.issue, "runner_numerus_overflow");
}

// ---------------------------------------------------------------------------
// Ruling 45: the resource ceilings.
// ---------------------------------------------------------------------------

#[test]
fn a_runaway_shift_is_a_resource_fault_with_its_own_identity() {
    let error = op(MirBinOp::Shl, &Value::Int(1), &Value::Int(4_000_000_000)).expect_err("ceiling");
    assert_eq!(error.issue, RUNNER_ISSUE_INF_SIZE_EXHAUSTED);
    assert_eq!(error.arg("operation"), Some("shift"));
    // A count beyond u64 is refused the same way, without allocating.
    let error = op(
        MirBinOp::Shl,
        &Value::Int(1),
        &big("100000000000000000000000000000"),
    )
    .expect_err("count beyond u64");
    assert_eq!(error.issue, RUNNER_ISSUE_INF_SIZE_EXHAUSTED);
    // Right shifts never grow.
    op(
        MirBinOp::Shr,
        &Value::Int(1),
        &big("100000000000000000000000000000"),
    )
    .expect("shr");
}

#[test]
fn the_bit_ceiling_admits_its_limit_and_refuses_one_bit_more() {
    let limit = RUNNER_INF_BIT_CEILING - 1;
    let largest = op(MirBinOp::Shl, &Value::Int(1), &Value::UInt(limit)).expect("2^(2^31 - 1)");
    assert_eq!(exact(&largest).bit_len(), RUNNER_INF_BIT_CEILING);
    let error = op(MirBinOp::Shl, &Value::Int(1), &Value::UInt(limit + 1)).expect_err("one more");
    assert_eq!(error.issue, RUNNER_ISSUE_INF_SIZE_EXHAUSTED);
    // `+` and `-` are post-checked: the sum of two ceiling-sized values is refused.
    let error = op(MirBinOp::Add, &largest, &largest).expect_err("sum past the ceiling");
    assert_eq!(error.issue, RUNNER_ISSUE_INF_SIZE_EXHAUSTED);
}

#[test]
fn a_multiplication_past_the_work_ceiling_is_refused_before_it_runs() {
    let wide = op(MirBinOp::Shl, &Value::Int(1), &Value::UInt(1 << 20)).expect("2^(2^20)");
    // 2^15 limbs each: 2^30 limb products, past the 2^27 work ceiling.
    let error = op(MirBinOp::Mul, &wide, &wide).expect_err("work ceiling");
    assert_eq!(error.issue, RUNNER_ISSUE_INF_SIZE_EXHAUSTED);
    assert_eq!(error.arg("operation"), Some("multiplication"));
}

// ---------------------------------------------------------------------------
// Ruling 37: exact comparison, equality, hashing, ordering.
// ---------------------------------------------------------------------------

#[test]
fn comparison_is_exact_against_every_integer_carrier() {
    let beyond = big("18446744073709551616");
    let below = big("-9223372036854775809");
    assert_eq!(
        cmp_numeric(&beyond, &Value::UInt(u64::MAX)),
        Some(Ordering::Greater)
    );
    assert_eq!(
        cmp_numeric(&Value::UInt(u64::MAX), &beyond),
        Some(Ordering::Less)
    );
    assert_eq!(
        cmp_numeric(&below, &Value::Int(i64::MIN)),
        Some(Ordering::Less)
    );
    assert_eq!(cmp_numeric(&beyond, &beyond), Some(Ordering::Equal));
    assert!(values_equal(&beyond, &big("18446744073709551616")));
    assert!(!values_equal(&beyond, &big("18446744073709551617")));
    assert!(values_equal(&Value::Int(5), &Value::UInt(5)));
    let greater = compare_values(
        beyond.clone(),
        Value::UInt(u64::MAX),
        MirBinOp::Gt,
        Span::default(),
    )
    .expect("compare");
    assert_eq!(greater, Value::Bool(true));
}

#[test]
fn comparison_with_a_float_never_rounds_the_integer() {
    let two_64 = Magnus::parse_decimal("18446744073709551616").expect("decimal");
    let plus_one = Magnus::parse_decimal("18446744073709551617").expect("decimal");
    let float = 18_446_744_073_709_551_616.0_f64;
    assert_eq!(cmp_magnus_float(&two_64, float), Some(Ordering::Equal));
    assert_eq!(
        cmp_magnus_float(&plus_one, float),
        Some(Ordering::Greater),
        "2^64 + 1 > 2^64 as f64"
    );
    assert_eq!(cmp_magnus_float(&two_64, f64::NAN), None);
    assert_eq!(
        cmp_magnus_float(&two_64, f64::INFINITY),
        Some(Ordering::Less)
    );
    assert_eq!(
        cmp_magnus_float(&two_64, f64::NEG_INFINITY),
        Some(Ordering::Greater)
    );
    assert_eq!(cmp_magnus_float(&two_64, 1.0e300), Some(Ordering::Less));
    assert_eq!(
        cmp_magnus_float(&-two_64.clone(), -1.0e300),
        Some(Ordering::Greater)
    );
    // A fraction orders between neighbours: 2^64 < 2^64 + 0.5 is not representable,
    // so pin the truncation-then-fraction path on a small value.
    assert_eq!(
        cmp_magnus_float(&Magnus::from_i64(5), 5.5),
        Some(Ordering::Less)
    );
    assert_eq!(
        cmp_magnus_float(&Magnus::from_i64(-5), -5.5),
        Some(Ordering::Greater)
    );
    let value = cmp_numeric(&big("18446744073709551617"), &Value::Float(float));
    assert_eq!(value, Some(Ordering::Greater));
}

#[test]
fn keys_hash_by_value_and_small_and_big_never_collide_falsely() {
    let beyond = big("18446744073709551616");
    let same = big("18446744073709551616");
    let other = big("18446744073709551617");
    let span = Span::default();
    assert_eq!(
        beyond.to_key(span).expect("key"),
        same.to_key(span).expect("key")
    );
    assert_ne!(
        beyond.to_key(span).expect("key"),
        other.to_key(span).expect("key")
    );
    // The canonical form keeps one spelling per integer: 5 is the same key
    // whatever carrier computed it.
    assert_eq!(
        Value::Int(5).to_key(span).expect("key"),
        Value::UInt(5).to_key(span).expect("key")
    );
    let roundtrip = beyond.to_key(span).expect("key").to_value();
    assert_eq!(roundtrip, beyond);
    assert_eq!(
        beyond.to_key(span).expect("key").document_key(),
        "18446744073709551616"
    );
}

#[test]
fn sorting_orders_the_whole_integer_line() {
    let mut values = [
        big("18446744073709551616"),
        Value::Int(-3),
        big("-18446744073709551616"),
        Value::UInt(u64::MAX),
        Value::Int(7),
    ];
    values.sort_by(compare_sort_values);
    let rendered: Vec<String> = values.iter().map(digits).collect();
    assert_eq!(
        rendered,
        [
            "-18446744073709551616",
            "-3",
            "7",
            "18446744073709551615",
            "18446744073709551616"
        ]
    );
}

// ---------------------------------------------------------------------------
// Ruling 40: the conversion row helpers.
// ---------------------------------------------------------------------------

#[test]
fn floats_round_nearest_even_and_overflow_to_infinity() {
    // 2^53 + 1 ties to even 2^53; 2^53 + 3 ties to 2^53 + 4.
    let f64_of = |text: &str| {
        to_float(
            &Magnus::parse_decimal(text).expect("decimal"),
            NumericWidth::F64,
        )
    };
    assert_eq!(
        f64_of("9007199254740993").to_bits(),
        9_007_199_254_740_992.0_f64.to_bits()
    );
    assert_eq!(
        f64_of("9007199254740995").to_bits(),
        9_007_199_254_740_996.0_f64.to_bits()
    );
    assert_eq!(
        f64_of("-9007199254740993").to_bits(),
        (-9_007_199_254_740_992.0_f64).to_bits()
    );
    // 2^1024 and beyond is infinity; the largest finite value is not.
    assert!(to_float(&(Magnus::one().shl(1024).expect("shl")), NumericWidth::F64).is_infinite());
    assert_eq!(
        to_float(&-(Magnus::one().shl(1100).expect("shl")), NumericWidth::F64),
        f64::NEG_INFINITY
    );
    // The round-to-nearest boundary is the midpoint 2^1024 - 2^970: a tie rounds
    // to even, which is the infinity side; one below it is the largest finite.
    let midpoint = Magnus::one().shl(1024).expect("shl") - Magnus::one().shl(970).expect("shl");
    assert!(to_float(&midpoint, NumericWidth::F64).is_infinite());
    assert!(to_float(&(midpoint - Magnus::one()), NumericWidth::F64).is_finite());
}

#[test]
fn f32_rounding_has_no_double_rounding() {
    // 2^24 + 1 + 2^-29 style case, built from an integer: a value just above
    // the f32 tie point whose f64 rounding lands exactly on the tie.
    // 2^60 + 2^36 + 1: f32 keeps 24 bits (spacing 2^37), the tie point is
    // 2^60 + 2^36, and the trailing +1 makes the exact value round UP;
    // rounding to f64 first (53 bits, spacing 2^8) would drop the +1 and tie.
    let value =
        Magnus::one().shl(60).expect("shl") + Magnus::one().shl(36).expect("shl") + Magnus::one();
    let direct = to_float(&value, NumericWidth::F32);
    let expected = (1_u64 << 60) as f32; // the tie-down neighbour
    let up = f32::from_bits(expected.to_bits() + 1);
    assert_eq!(
        direct as f32, up,
        "the exact value is above the tie, so it rounds up"
    );
    // f32 overflow: 2^128 is infinity, (2 - 2^-23) * 2^127 is finite.
    assert!(to_float(&Magnus::one().shl(128).expect("shl"), NumericWidth::F32).is_infinite());
    let max_f32 = (Magnus::from_u64((1 << 24) - 1)).shl(104).expect("shl");
    assert_eq!(to_float(&max_f32, NumericWidth::F32), f64::from(f32::MAX));
}

#[test]
fn f16_overflows_at_the_round_to_nearest_boundary() {
    let f16_of = |value: i64| to_float(&Magnus::from_i64(value), NumericWidth::F16);
    assert_eq!(f16_of(65_504), 65_504.0);
    assert!(f16_of(65_519).is_finite());
    assert!(f16_of(65_520).is_infinite());
    assert!(f16_of(-70_000).is_infinite() && f16_of(-70_000).is_sign_negative());
}

#[test]
fn wrapping_reduces_and_saturating_clamps() {
    let beyond = Magnus::parse_decimal("18446744073709551621").expect("2^64 + 5");
    assert_eq!(wrap_to_width(&beyond, NumericWidth::U8), Value::Int(5));
    assert_eq!(wrap_to_width(&beyond, NumericWidth::U64), Value::UInt(5));
    let neg = Magnus::parse_decimal("-18446744073709551621").expect("-(2^64 + 5)");
    assert_eq!(wrap_to_width(&neg, NumericWidth::U8), Value::Int(251));
    assert_eq!(
        wrap_to_width(&Magnus::from_i64(200), NumericWidth::I8),
        Value::Int(-56)
    );
    assert_eq!(
        wrap_to_width(&Magnus::from_i64(-200), NumericWidth::I8),
        Value::Int(56)
    );
    assert_eq!(clamp_to_width(&beyond, NumericWidth::U8), Value::Int(255));
    assert_eq!(
        clamp_to_width(&beyond, NumericWidth::U64),
        Value::UInt(u64::MAX)
    );
    assert_eq!(clamp_to_width(&neg, NumericWidth::U8), Value::Int(0));
    assert_eq!(clamp_to_width(&neg, NumericWidth::I8), Value::Int(-128));
    assert_eq!(
        clamp_to_width(&beyond, NumericWidth::I64),
        Value::Int(i64::MAX)
    );
}

// ---------------------------------------------------------------------------
// Conversions and stores through a runner.
// ---------------------------------------------------------------------------

fn inf_type() -> Type {
    Type::SizedNumeric(Primitive::Numerus, INF)
}

fn sized(width: NumericWidth) -> Type {
    Type::SizedNumeric(Primitive::Numerus, width)
}

fn with_runner<T>(
    types_in: impl FnOnce(&mut TypeTable) -> Vec<radix_types::TypeId>,
    body: impl FnOnce(&mut Runner<'_, crate::host::BufferHost>, &[radix_types::TypeId], &Interner) -> T,
) -> T {
    let mut types = TypeTable::new();
    let ids = types_in(&mut types);
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
        return_ty: MirType::semantic(ids[0]),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    body(&mut runner, &ids, &interner)
}

fn convert(
    source: Type,
    target: Type,
    value: Value,
    params: &[&str],
    recovery: Option<MirOperand>,
) -> Result<Value, RunnerError> {
    convert_ty(
        |types| (types.intern(source), types.intern(target)),
        value,
        params,
        recovery,
    )
}

/// Run one runtime `↦` conversion. `build` interns the source and target in the
/// fresh table, so a nullable carrier (which needs its member `TypeId`s) can be
/// constructed before the runner borrows the table.
fn convert_ty(
    build: impl FnOnce(&mut TypeTable) -> (radix_types::TypeId, radix_types::TypeId),
    value: Value,
    params: &[&str],
    recovery: Option<MirOperand>,
) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let (source_ty, target_ty) = build(&mut types);
    let mut interner = Interner::new();
    let params = params.iter().map(|p| interner.intern(p)).collect();
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
        params,
        recovery,
        struct_defaults: vec![],
    };
    runner.eval_runtime_convert(&conversion, value, Span::default())
}

fn from_inf(target: Type, value: Value) -> Result<Value, RunnerError> {
    convert(inf_type(), target, value, &[], None)
}

fn into_inf(source: Type, value: Value, params: &[&str]) -> Result<Value, RunnerError> {
    convert(source, inf_type(), value, params, None)
}

const OUT_OF_RANGE: &str = "runner_numerus_to_numerus_conversion_out_of_range";

#[test]
fn inf_to_a_bounded_integer_is_a_magnitude_checked_narrowing() {
    let beyond = big("18446744073709551616");
    assert_eq!(
        from_inf(sized(NumericWidth::U8), Value::Int(200)).expect("fits"),
        Value::Int(200)
    );
    assert_eq!(
        from_inf(sized(NumericWidth::U8), Value::Int(300))
            .expect_err("no")
            .issue,
        OUT_OF_RANGE
    );
    assert_eq!(
        from_inf(sized(NumericWidth::U8), Value::Int(-1))
            .expect_err("no")
            .issue,
        OUT_OF_RANGE
    );
    for width in [NumericWidth::U64, NumericWidth::I64, NumericWidth::I8] {
        assert_eq!(
            from_inf(sized(width), beyond.clone())
                .expect_err("beyond")
                .issue,
            OUT_OF_RANGE
        );
    }
    assert_eq!(
        from_inf(sized(NumericWidth::U64), Value::UInt(u64::MAX)).expect("u64::MAX fits u64"),
        Value::UInt(u64::MAX)
    );
    // A `⊥` recovery supplies the value instead.
    assert_eq!(
        convert(
            inf_type(),
            sized(NumericWidth::U8),
            beyond,
            &[],
            Some(MirOperand::Constant(MirConstant::Int(7)))
        )
        .expect("recovery"),
        Value::Int(7)
    );
}

#[test]
fn inf_to_policy_words_is_total() {
    let beyond = big("18446744073709551621");
    assert_eq!(
        from_inf(Type::ModularWord(NumericWidth::U8), beyond.clone()).expect("wrapping"),
        Value::Int(5)
    );
    assert_eq!(
        from_inf(Type::SaturatingWord(NumericWidth::U8), beyond).expect("saturating"),
        Value::Int(255)
    );
}

#[test]
fn inf_to_floats_follows_the_ieee_default() {
    let fractus = |width| Type::SizedNumeric(Primitive::Fractus, width);
    let beyond = big("18446744073709551616");
    assert_eq!(
        from_inf(fractus(NumericWidth::F64), beyond).expect("f64"),
        Value::Float(18_446_744_073_709_551_616.0)
    );
    let huge = from_magnus(Magnus::one().shl(1100).expect("shl"));
    assert_eq!(
        from_inf(fractus(NumericWidth::F64), huge.clone()).expect("f64"),
        Value::Float(f64::INFINITY)
    );
    assert_eq!(
        from_inf(Type::Primitive(Primitive::Fractus), huge).expect("default float"),
        Value::Float(f64::INFINITY)
    );
    // An `inf`-typed value a 64-bit carrier still holds takes the same path,
    // so `f16` overflows to infinity too.
    assert_eq!(
        from_inf(fractus(NumericWidth::F16), Value::Int(70_000)).expect("f16"),
        Value::Float(f64::INFINITY)
    );
}

#[test]
fn floats_into_inf_truncate_and_nan_and_infinity_fail() {
    let fractus = Type::Primitive(Primitive::Fractus);
    let exact_of = |value: f64| into_inf(fractus.clone(), Value::Float(value), &[]);
    assert_eq!(exact_of(-2.9).expect("trunc"), Value::Int(-2));
    assert_eq!(exact_of(0.99).expect("trunc"), Value::Int(0));
    assert_eq!(
        digits(&exact_of(1e25).expect("exact")),
        "10000000000000000905969664"
    );
    assert_eq!(
        exact_of(f64::NAN).expect_err("NaN").issue,
        "runner_fractus_to_numerus_conversion_out_of_range"
    );
    assert!(exact_of(f64::INFINITY).is_err());
    assert!(exact_of(f64::NEG_INFINITY).is_err());
}

#[test]
fn text_parses_any_length_in_every_radix_and_formats_back() {
    let textus = Type::Primitive(Primitive::Textus);
    let parse =
        |text: &str, params: &[&str]| into_inf(textus.clone(), Value::Str(text.to_owned()), params);
    assert_eq!(
        digits(&parse("123456789012345678901234567890", &[]).expect("digits")),
        "123456789012345678901234567890"
    );
    assert_eq!(
        digits(&parse("-99999999999999999999", &[]).expect("neg")),
        "-99999999999999999999"
    );
    assert_eq!(
        digits(&parse("ffffffffffffffffffff", &["Hex"]).expect("hex")),
        "1208925819614629174706175"
    );
    assert_eq!(parse("101", &["Bin"]).expect("bin"), Value::Int(5));
    assert_eq!(parse("17", &["Oct"]).expect("oct"), Value::Int(15));
    for bad in ["", "-", "12x", " 1", "1.5"] {
        assert_eq!(
            parse(bad, &[]).expect_err(bad).issue,
            "runner_textus_to_numerus_conversion_failed",
            "{bad:?}"
        );
    }
    assert_eq!(
        from_inf(textus_type(), big("-18446744073709551616")).expect("text"),
        Value::Str("-18446744073709551616".to_owned())
    );
}

fn textus_type() -> Type {
    Type::Primitive(Primitive::Textus)
}

#[test]
fn d64_conversions_follow_the_decimal_rules() {
    let d64 = sized(NumericWidth::D64);
    let beyond = big("18446744073709551616");
    // `inf → d64` is range-checked: the d64 range is ±92,233,720,368.54775807.
    assert_eq!(
        from_inf(d64.clone(), Value::Int(200)).expect("fits"),
        Value::Int(20_000_000_000)
    );
    assert_eq!(
        from_inf(d64.clone(), Value::Int(92_233_720_368)).expect("fits"),
        Value::Int(9_223_372_036_800_000_000)
    );
    assert!(from_inf(d64.clone(), Value::Int(92_233_720_369)).is_err());
    assert!(from_inf(d64, beyond).is_err());
    // `d64 → inf` is the integer reduction of the decimal value.
    assert_eq!(
        into_inf(sized(NumericWidth::D64), Value::Int(1_275_000_000), &[]).expect("12.75"),
        Value::Int(13)
    );
}

#[test]
fn octeti_via_endian_encodes_minimal_two_complement_and_decodes_exactly() {
    let octeti = Type::Primitive(Primitive::Octeti);
    let encode = |value: Value, endian: &str| {
        convert(inf_type(), octeti.clone(), value, &[endian], None).expect("encode")
    };
    let bytes = |value: Value| match value {
        Value::Octeti(bytes) => bytes,
        other => panic!("not octeti: {other:?}"),
    };
    assert_eq!(bytes(encode(Value::Int(0), "Be")), [0]);
    assert_eq!(bytes(encode(Value::Int(127), "Be")), [0x7f]);
    assert_eq!(bytes(encode(Value::Int(128), "Be")), [0x00, 0x80]);
    assert_eq!(bytes(encode(Value::Int(-1), "Be")), [0xff]);
    assert_eq!(bytes(encode(Value::Int(-128), "Be")), [0x80]);
    assert_eq!(bytes(encode(Value::Int(-129), "Be")), [0xff, 0x7f]);
    assert_eq!(bytes(encode(Value::Int(258), "Le")), [0x02, 0x01]);
    let beyond = big("18446744073709551616");
    assert_eq!(
        bytes(encode(beyond.clone(), "Le")),
        [0, 0, 0, 0, 0, 0, 0, 0, 1]
    );
    for value in [
        Value::Int(0),
        Value::Int(255),
        Value::Int(-255),
        beyond,
        big("-18446744073709551617"),
    ] {
        for endian in ["Be", "Le"] {
            let encoded = encode(value.clone(), endian);
            let decoded =
                convert(octeti.clone(), inf_type(), encoded, &[endian], None).expect("decode");
            assert_eq!(decoded, value, "{endian} round trip");
        }
    }
}

#[test]
fn ascii_pack_renders_inf_digits_with_the_fits_n_check() {
    // Through the conversion entry: `2^64 ↦ ascii<17> via Hex` fits, ascii<16> does not.
    let convert_pack = |capacity: u64, value: Value| {
        let mut types = TypeTable::new();
        let source_ty = types.intern(inf_type());
        let literal = types.intern_index(radix_types::IndexExpr::Literal(capacity));
        let target_ty = types.intern(Type::BoundedAscii { capacity: literal });
        let mut interner = Interner::new();
        let hex = interner.intern("Hex");
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
            params: vec![hex],
            recovery: None,
            struct_defaults: vec![],
        };
        runner.eval_runtime_convert(&conversion, value, Span::default())
    };
    let beyond = big("18446744073709551616");
    assert_eq!(
        convert_pack(17, beyond.clone()).expect("fits"),
        Value::Str("10000000000000000".to_owned())
    );
    assert_eq!(
        convert_pack(20, beyond.clone()).expect("padded"),
        Value::Str("00010000000000000000".to_owned())
    );
    assert_eq!(
        convert_pack(16, beyond.clone())
            .expect_err("does not fit")
            .issue,
        "runner_ascii_pack_width_overflow"
    );
    assert_eq!(
        convert_pack(40, big("-18446744073709551616"))
            .expect_err("negative")
            .issue,
        "runner_ascii_pack_negative_source"
    );
}

fn store_site() -> HirStoreSite {
    HirStoreSite {
        kind: HirStoreKind::Declaration,
        slot: None,
        inferred_from: None,
        inferred_text: None,
    }
}

#[test]
fn an_inf_beyond_64_bits_stores_into_a_bounded_slot_with_the_store_identity() {
    let value = big("18446744073709551616");
    with_runner(
        |types| {
            vec![
                types.intern(sized(NumericWidth::U8)),
                types.intern(Type::ModularWord(NumericWidth::U8)),
                types.intern(Type::SaturatingWord(NumericWidth::U8)),
                types.intern(inf_type()),
            ]
        },
        |runner, ids, _| {
            let trap = runner
                .eval_implicit_store(&store_site(), ids[0], &value, Span::default())
                .expect("an integer slot")
                .expect_err("2^64 does not fit u8");
            assert_eq!(trap.issue, crate::store::RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
            assert_eq!(trap.arg("value"), Some("18446744073709551616"));
            assert_eq!(trap.arg("type"), Some("u8"));
            let wrapped = runner
                .eval_implicit_store(&store_site(), ids[1], &value, Span::default())
                .expect("word slot")
                .expect("wrapping is total");
            assert_eq!(wrapped, Value::Int(0));
            let clamped = runner
                .eval_implicit_store(&store_site(), ids[2], &value, Span::default())
                .expect("word slot")
                .expect("saturating is total");
            assert_eq!(clamped, Value::Int(255));
            // A store into an `inf` slot is not a checked slot at all.
            assert!(
                runner
                    .eval_implicit_store(&store_site(), ids[3], &value, Span::default())
                    .is_none()
            );
        },
    );
}

#[test]
fn display_renders_the_exact_decimal_digits() {
    let interner = Interner::new();
    let structs = crate::value::StructFieldOrder::default();
    let variants = crate::value::StructFieldOrder::default();
    let order = FieldOrder::new(&structs, &variants);
    let rendered = big("-18446744073709551616")
        .display(&interner, order, Span::default())
        .expect("display");
    assert_eq!(rendered, "-18446744073709551616");
}

#[test]
fn constants_read_back_through_the_canonical_carrier() {
    // The runner reads `MirConstant::Magnus` digits; a value the 64-bit
    // carriers hold comes back as `Int`/`UInt`.
    with_runner(
        |types| vec![types.intern(inf_type())],
        |runner, _, _| {
            let mut read = |digits: &str| {
                runner
                    .eval_operand(
                        &MirOperand::Constant(MirConstant::Magnus(digits.to_owned())),
                        Span::default(),
                    )
                    .expect("constant")
            };
            assert_eq!(read("5"), Value::Int(5));
            assert_eq!(read("18446744073709551615"), Value::UInt(u64::MAX));
            assert_eq!(
                digits(&read("18446744073709551616")),
                "18446744073709551616"
            );
            assert!(matches!(read("18446744073709551616"), Value::Magnus(_)));
            assert!(
                runner
                    .eval_operand(
                        &MirOperand::Constant(MirConstant::Magnus("12x".to_owned())),
                        Span::default()
                    )
                    .is_err()
            );
        },
    );
}

// ---------------------------------------------------------------------------
// U8a: `inf` <-> `valor` and any-length JSON integer tokens (ruling 40).
// ---------------------------------------------------------------------------

/// The recoverable `valor ↦ numerus` failure, by structured identity (code and
/// issue), never by its prose message.
fn assert_valor_to_numerus_failure(err: &RunnerError) {
    assert_eq!(err.code, RUNNER_CODE_CONVERSION);
    assert_eq!(err.issue, "runner_valor_to_numerus_conversion_failed");
}

fn valor_type() -> Type {
    Type::Primitive(Primitive::Valor)
}

/// isint-carrier law: `est inf` holds every integer carrier; `est int` and
/// `est int<w>` hold an integer inside `i64` only (widths are erased, as in
/// `valor ↦ int<w>`), so the test agrees with the extraction and with the Rust
/// `Valor::Numerus` tag.
#[test]
fn est_integer_tags_follow_the_carrier_law() {
    use crate::runtime::integer_tag_matches;
    let int = Type::Primitive(Primitive::Numerus);
    let u64_width = sized(NumericWidth::U64);
    let inf = inf_type();
    let beyond = big("18446744073709551616");
    let below = big("-9223372036854775809");
    let rows = [
        (Value::Int(7), true, true),
        (Value::Int(i64::MIN), true, true),
        // A `u64` carrier holding a small value is still inside `i64`.
        (Value::UInt(7), true, true),
        (Value::UInt(i64::MAX as u64), true, true),
        (Value::UInt(i64::MAX as u64 + 1), false, true),
        (Value::UInt(u64::MAX), false, true),
        (beyond, false, true),
        (below, false, true),
    ];
    for (value, bounded, unbounded) in rows {
        assert_eq!(integer_tag_matches(&value, &int), bounded, "int {value:?}");
        assert_eq!(
            integer_tag_matches(&value, &u64_width),
            bounded,
            "u64 {value:?}"
        );
        assert_eq!(
            integer_tag_matches(&value, &inf),
            unbounded,
            "inf {value:?}"
        );
    }
    for value in [
        Value::Float(1.5),
        Value::Str("7".to_owned()),
        Value::Bool(true),
    ] {
        assert!(!integer_tag_matches(&value, &int), "{value:?}");
        assert!(!integer_tag_matches(&value, &inf), "{value:?}");
    }
}

#[test]
fn valor_to_inf_extracts_every_integer_carrier_and_nothing_else() {
    let beyond = big("18446744073709551616");
    for value in [
        Value::Int(0),
        Value::Int(i64::MIN),
        Value::UInt(u64::MAX),
        beyond.clone(),
        big("-18446744073709551617"),
    ] {
        assert_eq!(
            convert(valor_type(), inf_type(), value.clone(), &[], None).expect("integer valor"),
            value
        );
    }
    // The Rust `Magnus::from_valor` accepts only the integer variants.
    for value in [
        Value::Float(1.0),
        Value::Str("7".to_owned()),
        Value::Bool(true),
    ] {
        let err = convert(valor_type(), inf_type(), value, &[], None).expect_err("not integer");
        assert_valor_to_numerus_failure(&err);
    }
    // A `⊥` recovery supplies the value instead.
    assert_eq!(
        convert(
            valor_type(),
            inf_type(),
            Value::Float(1.0),
            &[],
            Some(MirOperand::Constant(MirConstant::Int(9))),
        )
        .expect("recovered"),
        Value::Int(9)
    );
}

fn inf_ty(types: &mut TypeTable) -> radix_types::TypeId {
    types.intern(Type::SizedNumeric(Primitive::Numerus, INF))
}

/// The two nullable carriers the table uses for `valor ∪ none`: a union with
/// `nihil`, and an `Option<valor>` (`types.rs` collapses between them at
/// lowering). Both must read a payload by the `valor` law.
fn nullable_valor(types: &mut TypeTable, option: bool) -> radix_types::TypeId {
    let valor = types.intern(Type::Primitive(Primitive::Valor));
    if option {
        types.option(valor)
    } else {
        let nihil = types.primitive(Primitive::Nihil);
        types.intern(Type::Union(vec![valor, nihil]))
    }
}

/// inf-nul: `valor ∪ none ↦ inf` reads its payload by the `valor ↦ inf` law. A
/// nullable source is a union/option carrier, not the bare `Primitive::Valor`
/// that routes `convert_to_target` through the dynamic extraction, so before
/// the fix a float payload truncated on its own shape (1.5 → 1) instead of
/// failing like the non-nullable path.
#[test]
fn nullable_valor_to_inf_reads_its_payload_by_the_valor_law() {
    let beyond = big("18446744073709551616");
    for option in [false, true] {
        let build = move |types: &mut TypeTable| (nullable_valor(types, option), inf_ty(types));
        // Integer payloads cross exactly, as the non-nullable path does.
        assert_eq!(
            convert_ty(build, Value::Int(7), &[], None).expect("integer payload"),
            Value::Int(7)
        );
        assert_eq!(
            convert_ty(build, beyond.clone(), &[], None).expect("beyond-64-bit payload"),
            beyond
        );
        // Both fractional and integral floats fail by the same valor law;
        // neither nullable carrier may truncate or change the fault's facts.
        for value in [Value::Float(1.5), Value::Float(3.0)] {
            let nullable = convert_ty(build, value.clone(), &[], None).expect_err("float payload");
            let non_nullable = convert(valor_type(), inf_type(), value, &[], None)
                .expect_err("non-nullable float");
            assert_valor_to_numerus_failure(&nullable);
            assert_valor_to_numerus_failure(&non_nullable);
            // Re-express message equality as full structured fault equality:
            // matching code/issue alone would miss changed diagnostic facts.
            assert_eq!(nullable.args, non_nullable.args);
            assert_eq!(
                nullable.arg("detail"),
                Some("valor to numerus conversion failed")
            );
        }
        // `⊥` supplies the fallback on the nullable path, as on the
        // non-nullable one.
        assert_eq!(
            convert_ty(
                build,
                Value::Float(1.0),
                &[],
                Some(MirOperand::Constant(MirConstant::Int(9))),
            )
            .expect("recovered"),
            Value::Int(9)
        );
    }
}

#[test]
fn inf_to_valor_keeps_the_canonical_carrier_and_narrowing_back_is_checked() {
    let beyond = big("18446744073709551616");
    assert_eq!(
        convert(inf_type(), valor_type(), beyond.clone(), &[], None).expect("box"),
        beyond
    );
    // `valor ↦ int<W>` from a beyond-64-bit integer fails (Rust `i64::from_valor` is `None`).
    let err =
        convert(valor_type(), sized(NumericWidth::I64), beyond, &[], None).expect_err("beyond i64");
    assert_valor_to_numerus_failure(&err);
}
