//! K5 (gpu-reset rule 15): the lifted built-in math functions on the runner,
//! the correctness oracle. Each element is the scalar function, IEEE 754 per
//! element: a domain miss is NaN, an overflow is infinity, and the result is
//! stored at the result element width.
// Test fixtures: exact float expectations and small, known-in-range numeric casts.
#![allow(
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::float_cmp
)]

use super::*;

use radix_mir::{MirCollectionOp, MirIntrinsic, MirRuntimeCall, MirType};
use radix_types::{MirMathFunction, NumericWidth, Primitive, Type, TypeId};

fn function() -> MirFunction {
    MirFunction {
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
    }
}

fn tensor_value(data: Vec<Value>, shape: Vec<i64>) -> Value {
    use crate::value::TensorValue;
    Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(data, shape))))
}

fn float_element(width: NumericWidth) -> Type {
    Type::SizedNumeric(Primitive::Fractus, width)
}

fn int_element(width: NumericWidth) -> Type {
    Type::SizedNumeric(Primitive::Numerus, width)
}

/// Run `function` over `input` (given shape) with `element` as the result
/// element type.
fn run(
    function_under_test: MirMathFunction,
    element: Type,
    shape: &[i64],
    input: Vec<Value>,
) -> Result<Value, RunnerError> {
    run_op(
        MirCollectionOp::TensorMath(function_under_test),
        element,
        shape,
        input,
    )
}

fn run_op(
    op: MirCollectionOp,
    element: Type,
    shape: &[i64],
    input: Vec<Value>,
) -> Result<Value, RunnerError> {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::MirValidationContext;
    use radix_types::TypeTable;

    let mut host = BufferHost::default();
    let mut types = TypeTable::default();
    let element = types.intern(element);
    let rank_shape = types.rank0_shape();
    let tensor = types.tensor_with_shape(element, rank_shape);
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(function());
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    let fn0 = program.functions.first().unwrap();
    runner.push_frame(fn0.id, MirBlockId(0));
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), tensor_value(input, shape.to_vec()));
    runner.eval_runtime_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(op),
            args: vec![MirOperand::Value(MirValueId(0))],
            return_ty: MirType::semantic(tensor),
        },
        Span::default(),
    )
}

fn floats(values: &[f64]) -> Vec<Value> {
    values.iter().copied().map(Value::Float).collect()
}

fn ints(values: &[i64]) -> Vec<Value> {
    values.iter().copied().map(Value::Int).collect()
}

fn elements(result: Value) -> (Vec<Value>, Vec<i64>) {
    let Value::Tensor(cell) = result else {
        panic!("expected a tensor, got {result:?}");
    };
    let tensor = cell.borrow();
    (
        tensor.logical_values().expect("valid tensor layout"),
        tensor.shape.clone(),
    )
}

fn float_result(
    function: MirMathFunction,
    width: NumericWidth,
    shape: &[i64],
    input: &[f64],
) -> Vec<f64> {
    let (data, out_shape) = elements(
        run(function, float_element(width), shape, floats(input)).expect("float math runs"),
    );
    assert_eq!(out_shape, shape, "shape is preserved");
    data.into_iter()
        .map(|value| match value {
            Value::Float(number) => number,
            other => panic!("expected float, got {other:?}"),
        })
        .collect()
}

fn same(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || (a == b && a.is_sign_negative() == b.is_sign_negative())
}

#[track_caller]
fn assert_floats(actual: &[f64], expected: &[f64]) {
    assert_eq!(actual.len(), expected.len(), "{actual:?} vs {expected:?}");
    for (a, e) in actual.iter().zip(expected) {
        assert!(same(*a, *e), "{actual:?} vs {expected:?}");
    }
}

#[test]
fn sqrt_domain_miss_is_nan_and_zero_keeps_its_sign() {
    let out = float_result(
        MirMathFunction::Sqrt,
        NumericWidth::F64,
        &[6],
        &[4.0, -1.0, 0.0, -0.0, f64::INFINITY, f64::NAN],
    );
    assert_floats(&out, &[2.0, f64::NAN, 0.0, -0.0, f64::INFINITY, f64::NAN]);
    let negative_infinity = float_result(
        MirMathFunction::Sqrt,
        NumericWidth::F64,
        &[1],
        &[f64::NEG_INFINITY],
    );
    assert!(negative_infinity[0].is_nan());
}

#[test]
fn exp_overflows_to_infinity_and_underflows_to_zero() {
    let out = float_result(
        MirMathFunction::Exp,
        NumericWidth::F64,
        &[6],
        &[0.0, 1.0, 710.0, -746.0, f64::INFINITY, f64::NEG_INFINITY],
    );
    assert_floats(
        &out,
        &[
            1.0,
            std::f64::consts::E,
            f64::INFINITY,
            0.0,
            f64::INFINITY,
            0.0,
        ],
    );
    assert!(float_result(MirMathFunction::Exp, NumericWidth::F64, &[1], &[f64::NAN])[0].is_nan());
}

#[test]
fn ln_and_log10_domain_edges() {
    for function in [MirMathFunction::Ln, MirMathFunction::Log10] {
        let out = float_result(
            function,
            NumericWidth::F64,
            &[5],
            &[1.0, 0.0, -0.0, -1.0, f64::INFINITY],
        );
        assert_floats(
            &out,
            &[
                0.0,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                f64::NAN,
                f64::INFINITY,
            ],
        );
    }
    let out = float_result(
        MirMathFunction::Log10,
        NumericWidth::F64,
        &[3],
        &[10.0, 100.0, 1000.0],
    );
    assert_floats(&out, &[1.0, 2.0, 3.0]);
}

#[test]
fn trig_special_values() {
    let sin = float_result(
        MirMathFunction::Sin,
        NumericWidth::F64,
        &[4],
        &[0.0, -0.0, f64::INFINITY, f64::NAN],
    );
    assert_floats(&sin, &[0.0, -0.0, f64::NAN, f64::NAN]);
    let cos = float_result(
        MirMathFunction::Cos,
        NumericWidth::F64,
        &[3],
        &[0.0, f64::INFINITY, f64::NAN],
    );
    assert_floats(&cos, &[1.0, f64::NAN, f64::NAN]);
    let tan = float_result(
        MirMathFunction::Tan,
        NumericWidth::F64,
        &[3],
        &[0.0, -0.0, f64::INFINITY],
    );
    assert_floats(&tan, &[0.0, -0.0, f64::NAN]);
}

#[test]
fn abs_clears_the_sign_bit_of_floats() {
    let out = float_result(
        MirMathFunction::Abs,
        NumericWidth::F64,
        &[6],
        &[-2.5, 3.0, -0.0, 0.0, f64::NEG_INFINITY, f64::NAN],
    );
    assert_floats(&out, &[2.5, 3.0, 0.0, 0.0, f64::INFINITY, f64::NAN]);
}

#[test]
fn f32_elements_round_through_f32() {
    // sqrt(2) in f64 is not an f32; the stored element is the nearest f32.
    let out = float_result(MirMathFunction::Sqrt, NumericWidth::F32, &[1], &[2.0]);
    assert_eq!(out[0], f64::from(2.0f32.sqrt()));
    assert_ne!(out[0], 2.0f64.sqrt());
    // f32 overflow: exp(100) fits f64 but not f32.
    let overflow = float_result(MirMathFunction::Exp, NumericWidth::F32, &[1], &[100.0]);
    assert_eq!(overflow[0], f64::INFINITY);
    let wide = float_result(MirMathFunction::Exp, NumericWidth::F64, &[1], &[100.0]);
    assert!(wide[0].is_finite());
}

#[test]
fn empty_tensor_stays_empty_with_its_shape() {
    for function in MirMathFunction::ALL {
        let (data, shape) = elements(
            run(function, float_element(NumericWidth::F32), &[0], vec![]).expect("empty runs"),
        );
        assert!(data.is_empty(), "{}", function.name());
        assert_eq!(shape, vec![0]);
    }
}

#[test]
fn shape_is_preserved_for_ranks_one_to_three() {
    for shape in [vec![3], vec![2, 3], vec![2, 1, 2]] {
        let count: i64 = shape.iter().product();
        let input: Vec<f64> = (0..count).map(|value| value as f64 + 1.0).collect();
        for function in MirMathFunction::ALL {
            let out = float_result(function, NumericWidth::F64, &shape, &input);
            assert_eq!(out.len() as i64, count, "{}", function.name());
        }
    }
}

#[test]
fn integer_abs_is_exact_and_typed_by_the_result_element() {
    // The range rule types `absolutum` of an `i8` as `u8`, so `|-128|` fits.
    let (data, _) = elements(
        run(
            MirMathFunction::Abs,
            int_element(NumericWidth::U8),
            &[3],
            ints(&[-128, 5, 0]),
        )
        .expect("|-128| fits u8"),
    );
    assert_eq!(data, ints(&[128, 5, 0]));
    // A device function keeps the element type: `|-128|` does not fit `i8`.
    let err = run(
        MirMathFunction::Abs,
        int_element(NumericWidth::I8),
        &[1],
        ints(&[-128]),
    )
    .expect_err("128 leaves i8");
    assert_eq!(err.issue, crate::store::RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
}

#[test]
fn integer_elements_are_rejected_by_float_only_functions() {
    for function in MirMathFunction::ALL.into_iter().filter(|f| f.float_only()) {
        let err = run(function, int_element(NumericWidth::I32), &[1], ints(&[4]))
            .expect_err("float-only function on integers");
        assert_eq!(
            err.issue,
            format!("runner_tensor_{}_element_type_mismatch", function.name())
        );
    }
}

/// Need: the mean is sum / count, so an empty source is 0 / 0 = NaN
/// (not a 0.0 fallback), and a non-empty source divides by the element count.
#[test]
fn mean_is_sum_over_count_and_empty_source_is_nan() {
    let width = NumericWidth::F64;
    let empty = run_op(
        MirCollectionOp::TensorMean,
        float_element(width),
        &[0],
        floats(&[]),
    )
    .expect("empty mean runs");
    let Value::Float(number) = empty else {
        panic!("expected float, got {empty:?}");
    };
    assert!(number.is_nan(), "empty mean is NaN, got {number}");

    let full = run_op(
        MirCollectionOp::TensorMean,
        float_element(width),
        &[4],
        floats(&[3.0, 1.0, 3.0, 2.0]),
    )
    .expect("mean runs");
    assert!(matches!(full, Value::Float(number) if number == 2.25));
}
