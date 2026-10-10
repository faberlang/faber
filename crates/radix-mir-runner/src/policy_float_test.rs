//! N7E-3 (D1, D2, D3): the runner evaluates `trapping<W>` and `saturating<W>`
//! float cells. A `saturating` cell is the bare float of its width (IEEE, no
//! clamp); a `trapping` cell checks only where a value is stored into it (the
//! implicit store node, and the end of a conversion row), never per operation.
//! Assertions use the issue, code and args of the failure, never its prose.

use super::*;
use crate::store::RUNNER_ISSUE_IMPLICIT_STORE_TRAP;
use handroll::Magnus;
use radix_hir::{HirStoreKind, HirStoreSite};
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBinOp, MirBlockId, MirConstant, MirConversion, MirConversionFlavor, MirFunction,
    MirFunctionId, MirOperand, MirProgram, MirType, MirUnOp, MirValidationContext, MirValue,
    MirValueId, MirValueKind,
};
use radix_types::{FloatPolicy, NumericWidth, Primitive, Type, TypeTable};

const OVERFLOW_ISSUE: &str = "runner_numerus_overflow";

fn cell(policy: FloatPolicy, width: NumericWidth) -> Type {
    Type::PolicyFloat(policy, width)
}

fn bare(width: NumericWidth) -> Type {
    Type::SizedNumeric(Primitive::Fractus, width)
}

fn inf_type() -> Type {
    Type::SizedNumeric(Primitive::Numerus, NumericWidth::Inf)
}

/// Run `body` with a runner positioned in an empty frame over `types`.
fn with_runner<R>(
    types: &TypeTable,
    interner: &Interner,
    body: impl FnOnce(&mut Runner<'_, crate::host::BufferHost>) -> R,
) -> R {
    let context = MirValidationContext::new(types);
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, types, interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    body(&mut runner)
}

fn site(interner: &mut Interner, slot: &str) -> HirStoreSite {
    HirStoreSite {
        kind: HirStoreKind::Declaration,
        slot: Some(interner.intern(slot)),
        inferred_from: None,
        inferred_text: None,
    }
}

/// The implicit store of `value` into a cell, as `eval_convert` calls it.
fn store(
    policy: FloatPolicy,
    width: NumericWidth,
    value: f64,
) -> Option<Result<Value, RunnerError>> {
    let mut types = TypeTable::new();
    let target = types.intern(cell(policy, width));
    let mut interner = Interner::new();
    let site = site(&mut interner, "u");
    with_runner(&types, &interner, |runner| {
        runner.eval_implicit_store(&site, target, &Value::Float(value), Span::default())
    })
}

fn arg<'a>(error: &'a RunnerError, name: &str) -> Option<&'a str> {
    error
        .args
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn convert(
    source: Type,
    target: Type,
    value: Value,
    recovery: Option<MirOperand>,
) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let source_ty = types.intern(source);
    let target_ty = types.intern(target);
    let interner = Interner::new();
    with_runner(&types, &interner, |runner| {
        let conversion = MirConversion {
            store: None,
            flavor: MirConversionFlavor::Runtime,
            source_ty: MirType::semantic(source_ty),
            target_ty: MirType::semantic(target_ty),
            params: vec![],
            recovery,
            struct_defaults: vec![],
        };
        runner.eval_runtime_convert(&conversion, value, Span::default())
    })
}

const NON_FINITE: [(f64, &str); 3] = [
    (f64::INFINITY, "inf"),
    (f64::NEG_INFINITY, "-inf"),
    (f64::NAN, "NaN"),
];

#[test]
fn policy_float_trapping_store_rejects_non_finite() {
    for width in [NumericWidth::F32, NumericWidth::F64] {
        for (value, text) in NON_FINITE {
            let error = store(FloatPolicy::Trapping, width, value)
                .expect("a trapping cell checks a float store")
                .expect_err("a non-finite value does not store");
            assert_eq!(error.issue, RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
            assert_eq!(error.code, RUNNER_CODE_INTERNAL);
            assert_eq!(arg(&error, "value"), Some(text));
            assert_eq!(arg(&error, "type"), Some(width.rust_type()));
            assert_eq!(arg(&error, "slot"), Some("u"));
            assert_eq!(arg(&error, "store"), Some("declaration"));
        }
    }
}

#[test]
fn policy_float_trapping_store_keeps_a_finite_value() {
    for width in [NumericWidth::F32, NumericWidth::F64] {
        for value in [
            0.0,
            -0.0,
            1.5,
            f64::from(f32::MAX),
            f64::from(f32::MIN_POSITIVE),
        ] {
            // `None` hands the value to the ordinary conversion: nothing clamps.
            assert!(store(FloatPolicy::Trapping, width, value).is_none());
        }
    }
    // A finite f64 beyond the f32 range rounds to infinity in an `f32` cell.
    assert!(
        store(FloatPolicy::Trapping, NumericWidth::F32, 1.0e300)
            .is_some_and(|stored| stored.is_err())
    );
    assert!(store(FloatPolicy::Trapping, NumericWidth::F64, 1.0e300).is_none());
}

#[test]
fn policy_float_saturating_store_is_bare_ieee() {
    for width in [NumericWidth::F32, NumericWidth::F64] {
        for (value, _) in NON_FINITE {
            assert!(
                store(FloatPolicy::Saturating, width, value).is_none(),
                "a saturating cell has no store check and no clamp"
            );
        }
    }
}

#[test]
fn policy_float_saturating_conversion_keeps_infinity_and_nan() {
    for width in [NumericWidth::F32, NumericWidth::F64] {
        for (value, _) in NON_FINITE {
            let stored = convert(
                bare(NumericWidth::F64),
                cell(FloatPolicy::Saturating, width),
                Value::Float(value),
                None,
            )
            .expect("a saturating cell cannot fail");
            let Value::Float(stored) = stored else {
                panic!("expected a float carrier");
            };
            assert_eq!(stored.is_nan(), value.is_nan());
            assert_eq!(stored.is_infinite(), value.is_infinite());
        }
    }
}

#[test]
fn policy_float_narrow_formats_trap_on_rounding_to_infinity() {
    // binary16: 65504 is the largest finite value; 65520 is the tie that
    // rounds up to infinity (nearest-even).
    let half_edge = [
        (65_504.0, false),
        (65_519.0, false),
        (65_520.0, true),
        (1.0e10, true),
    ];
    for (value, traps) in half_edge {
        let stored = store(FloatPolicy::Trapping, NumericWidth::F16, value);
        assert_eq!(stored.is_some(), traps, "f16 {value}");
        assert_eq!(
            store(FloatPolicy::Trapping, NumericWidth::F16, -value).is_some(),
            traps
        );
    }
    // bfloat16: (2 - 2^-7) * 2^127 is the largest finite value; the tie at
    // (2 - 2^-8) * 2^127 rounds up to infinity.
    let largest = (2.0 - 2f64.powi(-7)) * 2f64.powi(127);
    let tie = (2.0 - 2f64.powi(-8)) * 2f64.powi(127);
    assert!(store(FloatPolicy::Trapping, NumericWidth::BF16, largest).is_none());
    assert!(store(FloatPolicy::Trapping, NumericWidth::BF16, tie.next_down()).is_none());
    assert!(store(FloatPolicy::Trapping, NumericWidth::BF16, tie).is_some());
    for width in [NumericWidth::F16, NumericWidth::BF16] {
        for (value, text) in NON_FINITE {
            let error = store(FloatPolicy::Trapping, width, value)
                .expect("a trapping cell checks a float store")
                .expect_err("a non-finite value does not store");
            assert_eq!(error.issue, RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
            assert_eq!(arg(&error, "value"), Some(text));
            assert_eq!(arg(&error, "type"), Some(width.rust_type()));
        }
        // A saturating narrow cell never checks.
        assert!(store(FloatPolicy::Saturating, width, 1.0e300).is_none());
    }
}

fn huge() -> Value {
    let big = Magnus::parse_decimal(&format!("1{}", "0".repeat(400))).expect("decimal");
    Value::Magnus(std::rc::Rc::new(big))
}

#[test]
fn policy_float_conversion_of_inf_follows_the_cell() {
    // 6a: `inf` beyond the finite range gives infinity for `saturating<f64>`
    // (as for bare `f64`) and raises the trapping overflow error for
    // `trapping<f64>`.
    let saturated = convert(
        inf_type(),
        cell(FloatPolicy::Saturating, NumericWidth::F64),
        huge(),
        None,
    )
    .expect("saturating cannot fail");
    assert_eq!(saturated, Value::Float(f64::INFINITY));
    let bare_float = convert(inf_type(), bare(NumericWidth::F64), huge(), None).expect("bare");
    assert_eq!(bare_float, Value::Float(f64::INFINITY));

    let error = convert(
        inf_type(),
        cell(FloatPolicy::Trapping, NumericWidth::F64),
        huge(),
        None,
    )
    .expect_err("an `inf` beyond the finite range does not fit a trapping cell");
    assert_eq!(error.issue, OVERFLOW_ISSUE);
    assert_eq!(error.code, RUNNER_CODE_INTERNAL);
}

#[test]
fn policy_float_conversion_into_trapping_checks_the_result() {
    // An f64 beyond the f32 range overflows the `f32` cell, not the `f64` one.
    let error = convert(
        bare(NumericWidth::F64),
        cell(FloatPolicy::Trapping, NumericWidth::F32),
        Value::Float(1.0e300),
        None,
    )
    .expect_err("1e300 does not fit trapping<f32>");
    assert_eq!(error.issue, OVERFLOW_ISSUE);
    let kept = convert(
        bare(NumericWidth::F64),
        cell(FloatPolicy::Trapping, NumericWidth::F64),
        Value::Float(1.0e300),
        None,
    )
    .expect("1e300 fits trapping<f64>");
    assert_eq!(kept, Value::Float(1.0e300));
    // NaN is not finite either.
    let error = convert(
        bare(NumericWidth::F64),
        cell(FloatPolicy::Trapping, NumericWidth::F64),
        Value::Float(f64::NAN),
        None,
    )
    .expect_err("NaN does not fit trapping<f64>");
    assert_eq!(error.issue, OVERFLOW_ISSUE);
    // An authored `⊥` recovers the check, as it does for the integer cells.
    let recovered = convert(
        bare(NumericWidth::F64),
        cell(FloatPolicy::Trapping, NumericWidth::F32),
        Value::Float(1.0e300),
        Some(MirOperand::Constant(MirConstant::Float(7.0))),
    )
    .expect("the recovery value stands");
    assert_eq!(recovered, Value::Float(7.0));
}

#[test]
fn policy_float_tensor_elements_reach_the_store_check() {
    let mut types = TypeTable::new();
    let trapping = types.intern(cell(FloatPolicy::Trapping, NumericWidth::F32));
    let saturating = types.intern(cell(FloatPolicy::Saturating, NumericWidth::F32));
    let interner = Interner::new();
    with_runner(&types, &interner, |runner| {
        let error = runner
            .store_tensor_element(trapping, Value::Float(f64::INFINITY), Span::default())
            .expect_err("an infinite element does not store in a trapping cell");
        assert_eq!(error.issue, RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
        assert_eq!(arg(&error, "store"), Some("collection element"));
        assert_eq!(
            runner
                .store_tensor_element(saturating, Value::Float(f64::INFINITY), Span::default())
                .expect("saturating elements store as is"),
            Value::Float(f64::INFINITY)
        );
    });
}

/// Evaluate one scalar operation over constants at the result type `ty`.
fn evaluate(
    types: &TypeTable,
    ty: radix_types::TypeId,
    kind: MirValueKind,
) -> Result<Value, RunnerError> {
    let interner = Interner::new();
    with_runner(types, &interner, |runner| {
        let value = MirValue {
            id: MirValueId(0),
            kind,
            ty: MirType::semantic(ty),
            span: Span::default(),
        };
        runner.eval_value(&value, Span::default())
    })
}

fn constant(value: f64) -> MirOperand {
    MirOperand::Constant(MirConstant::Float(value))
}

/// Same bits, treating every NaN as one value.
fn same_bits(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Float(a), Value::Float(b)) => {
            (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
        }
        (a, b) => a == b,
    }
}

#[test]
fn policy_float_operations_match_the_bare_float() {
    // D2, D3: `+ - * / %` and unary `-` evaluate on a cell exactly as on the
    // bare float of its width (`÷` lowers to the same `Div` for floats). The
    // table includes the operands that overflow, divide by zero and produce
    // NaN; nothing clamps or traps on either side.
    let f32_max = f64::from(f32::MAX);
    let operands = [
        (1.5, 2.25),
        (-7.5, 2.0),
        (f32_max, f32_max),
        (f32_max, -f32_max),
        (0.0, 0.0),
        (1.0, 0.0),
        (-1.0, 0.0),
        (f64::INFINITY, 1.0),
        (f64::INFINITY, f64::INFINITY),
        (f64::NAN, 1.0),
        (1.0e-45, 3.0),
    ];
    let binary = [
        MirBinOp::Add,
        MirBinOp::Sub,
        MirBinOp::Mul,
        MirBinOp::Div,
        MirBinOp::Mod,
    ];
    for width in [NumericWidth::F32, NumericWidth::F64] {
        let mut types = TypeTable::new();
        let bare_ty = types.intern(bare(width));
        for policy in [FloatPolicy::Saturating, FloatPolicy::Trapping] {
            let cell_ty = types.intern(cell(policy, width));
            for op in binary {
                for (lhs, rhs) in operands {
                    let kind = |lhs: f64, rhs: f64| MirValueKind::Binary {
                        op,
                        lhs: constant(lhs),
                        rhs: constant(rhs),
                    };
                    let want = evaluate(&types, bare_ty, kind(lhs, rhs));
                    let got = evaluate(&types, cell_ty, kind(lhs, rhs));
                    match (&want, &got) {
                        (Ok(want), Ok(got)) => assert!(
                            same_bits(want, got),
                            "{policy:?} {width:?} {op:?} {lhs} {rhs}: {got:?} != {want:?}"
                        ),
                        (Err(want), Err(got)) => assert_eq!(want.issue, got.issue),
                        _ => panic!("{policy:?} {width:?} {op:?} {lhs} {rhs}: {got:?} vs {want:?}"),
                    }
                }
            }
            for (operand, _) in operands {
                let kind = MirValueKind::Unary {
                    op: MirUnOp::Neg,
                    operand: constant(operand),
                };
                let want = evaluate(&types, bare_ty, kind.clone()).expect("bare neg");
                let got = evaluate(&types, cell_ty, kind).expect("cell neg");
                assert!(same_bits(&want, &got), "{policy:?} {width:?} neg {operand}");
            }
        }
    }
}

#[test]
fn policy_float_arithmetic_overflow_is_infinity_not_a_trap() {
    // D1 is store-only: `m * m` on a `trapping<f32>` is bare IEEE and does not
    // trap; only a store of the result does (above).
    let mut types = TypeTable::new();
    let ty = types.intern(cell(FloatPolicy::Trapping, NumericWidth::F32));
    let product = evaluate(
        &types,
        ty,
        MirValueKind::Binary {
            op: MirBinOp::Mul,
            lhs: constant(f64::from(f32::MAX)),
            rhs: constant(f64::from(f32::MAX)),
        },
    )
    .expect("an unstored product does not trap");
    assert_eq!(product, Value::Float(f64::INFINITY));
}

/// Run the five-argument fuzzy `≈` / `≉` over two cell-typed locals (so the
/// comparison width is read from the slot type) and return its boolean.
fn fuzzy(
    policy: FloatPolicy,
    width: NumericWidth,
    left: f64,
    right: f64,
    negated: bool,
) -> Result<Value, RunnerError> {
    use radix_mir::{MirCollectionOp, MirIntrinsic, MirLocal, MirLocalId, MirPlace, MirPlaceBase};
    use radix_mir::{MirRuntimeCall, MirType};

    let mut types = TypeTable::new();
    let ty = types.intern(cell(policy, width));
    let bool_ty = types.primitive(Primitive::Bivalens);
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: (0..2)
            .map(|id| MirLocal {
                id: MirLocalId(id),
                ty: MirType::semantic(ty),
                name: None,
                mutable: false,
                span: Span::default(),
            })
            .collect(),
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(types.primitive(Primitive::Vacuum)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let frame = runner.frames.last_mut().expect("frame");
    frame.locals[0] = Some(Value::Float(left));
    frame.locals[1] = Some(Value::Float(right));
    let place = |id| {
        MirOperand::Place(MirPlace {
            base: MirPlaceBase::Local(MirLocalId(id)),
            projections: vec![],
        })
    };
    runner.eval_runtime_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(MirCollectionOp::Approximate),
            args: vec![
                place(0),
                place(1),
                constant(1e-9),
                constant(0.0),
                MirOperand::Constant(MirConstant::Bool(negated)),
            ],
            return_ty: MirType::semantic(bool_ty),
        },
        Span::default(),
    )
}

#[test]
fn policy_float_fuzzy_comparison_runs_on_the_bare_float() {
    // `q ≈ 7.0` over a saturating or trapping cell is the comparison of the
    // bare float of its width; `≉` is its negation. No width limit applies.
    for policy in [FloatPolicy::Saturating, FloatPolicy::Trapping] {
        for width in [NumericWidth::F32, NumericWidth::F64] {
            let same = fuzzy(policy, width, 7.0, 7.0, false).expect("≈ over a cell");
            assert_eq!(same, Value::Bool(true), "{policy:?} {width:?} 7 ≈ 7");
            let apart = fuzzy(policy, width, 7.0, 8.0, false).expect("≈ over a cell");
            assert_eq!(apart, Value::Bool(false), "{policy:?} {width:?} 7 ≈ 8");
            let negated = fuzzy(policy, width, 7.0, 8.0, true).expect("≉ over a cell");
            assert_eq!(negated, Value::Bool(true), "{policy:?} {width:?} 7 ≉ 8");
        }
    }
}
