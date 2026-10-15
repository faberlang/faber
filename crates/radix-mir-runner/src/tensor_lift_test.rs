//! `MirIntrinsic::TensorLift` on hand-built values (K4): element stores, NaN
//! and infinity, empty tensors, shape mismatch, and both-sides evaluation are
//! the runner's own contract, independent of the front end.

use super::*;
use crate::host::BufferHost;
use crate::value::TensorValue;
use radix_mir::{MirBlockId, MirIntrinsic, MirLiftOp, MirRuntimeCall, MirType};
use radix_types::{NumericWidth, Primitive, Type, TypeId, TypeTable};

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

fn tensor(values: Vec<Value>, shape: &[i64]) -> Value {
    Value::Tensor(Rc::new(RefCell::new(TensorValue::contiguous(
        values,
        shape.to_vec(),
    ))))
}

fn ints(values: &[i64], shape: &[i64]) -> Value {
    tensor(values.iter().copied().map(Value::Int).collect(), shape)
}

fn floats(values: &[f64], shape: &[i64]) -> Value {
    tensor(values.iter().copied().map(Value::Float).collect(), shape)
}

fn data(value: &Value) -> Vec<Value> {
    let Value::Tensor(cell) = value else {
        panic!("expected a tensor, got {value:?}");
    };
    cell.borrow().logical_values().expect("valid tensor layout")
}

/// Run `op` over `operands` into a result tensor whose element is `element`.
fn lift(op: MirLiftOp, element: Type, operands: Vec<Value>) -> Result<Value, RunnerError> {
    let mut types = TypeTable::default();
    let element = types.intern(element);
    let shape = types.rank0_shape();
    let result = types.tensor_with_shape(element, shape);
    let interner = radix_lexer::Interner::new();
    let context = radix_mir::MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(function());
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let mut args = Vec::new();
    for (index, value) in operands.into_iter().enumerate() {
        let id = radix_mir::MirValueId(u32::try_from(index).unwrap());
        runner.frames.last_mut().unwrap().values.insert(id, value);
        args.push(MirOperand::Value(id));
    }
    runner.eval_runtime_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::TensorLift(op),
            args,
            return_ty: MirType::semantic(result),
        },
        Span::default(),
    )
}

fn u8_element() -> Type {
    Type::SizedNumeric(Primitive::Numerus, NumericWidth::U8)
}

fn f32_element() -> Type {
    Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
}

fn bool_element() -> Type {
    Type::Primitive(Primitive::Bivalens)
}

#[test]
fn a_u8_element_store_traps_per_element() {
    let err = lift(
        MirLiftOp::Add,
        u8_element(),
        vec![ints(&[200, 1], &[2]), Value::Int(100)],
    )
    .expect_err("300 leaves u8");
    assert_eq!(err.issue, crate::store::RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
    assert_eq!(err.arg("value"), Some("300"));
    assert_eq!(err.arg("type"), Some("u8"));
    let fits = lift(
        MirLiftOp::Add,
        u8_element(),
        vec![ints(&[200, 1], &[2]), Value::Int(55)],
    )
    .expect("255 fits");
    assert_eq!(data(&fits), vec![Value::Int(255), Value::Int(56)]);
    let below = lift(
        MirLiftOp::Sub,
        u8_element(),
        vec![ints(&[0], &[1]), Value::Int(1)],
    )
    .expect_err("-1 leaves u8");
    assert_eq!(below.issue, crate::store::RUNNER_ISSUE_IMPLICIT_STORE_TRAP);
    assert_eq!(below.arg("value"), Some("-1"));
    assert_eq!(below.arg("type"), Some("u8"));
}

#[test]
fn integer_division_floors_and_a_zero_divisor_traps() {
    let floored = lift(
        MirLiftOp::Div,
        Type::Primitive(Primitive::Numerus),
        vec![ints(&[7, -7], &[2]), Value::Int(2)],
    )
    .unwrap();
    assert_eq!(data(&floored), vec![Value::Int(3), Value::Int(-4)]);
    let err = lift(
        MirLiftOp::Mod,
        Type::Primitive(Primitive::Numerus),
        vec![ints(&[7], &[1]), Value::Int(0)],
    )
    .expect_err("zero divisor");
    assert_eq!(err.issue, "runner_numerus_division_failed");
}

#[test]
fn float_mod_floors_and_takes_the_divisor_sign() {
    let positive = lift(
        MirLiftOp::Mod,
        f32_element(),
        vec![floats(&[-7.5, 7.5], &[2]), Value::Float(2.0)],
    )
    .unwrap();
    assert_eq!(data(&positive), vec![Value::Float(0.5), Value::Float(1.5)]);
    let negative = lift(
        MirLiftOp::Mod,
        f32_element(),
        vec![floats(&[-7.5, 7.5], &[2]), Value::Float(-2.0)],
    )
    .unwrap();
    assert_eq!(
        data(&negative),
        vec![Value::Float(-1.5), Value::Float(-0.5)]
    );
}

#[test]
fn true_division_converts_integers_and_traps_only_an_integer_zero() {
    let quotient = lift(
        MirLiftOp::TrueDiv,
        f32_element(),
        vec![ints(&[1, 3], &[2]), Value::Int(2)],
    )
    .unwrap();
    assert_eq!(data(&quotient), vec![Value::Float(0.5), Value::Float(1.5)]);
    let err = lift(
        MirLiftOp::TrueDiv,
        f32_element(),
        vec![ints(&[1], &[1]), Value::Int(0)],
    )
    .expect_err("integer zero divisor");
    assert_eq!(err.issue, "runner_numerus_division_failed");
    let ieee = lift(
        MirLiftOp::TrueDiv,
        f32_element(),
        vec![floats(&[1.0, -1.0, 0.0], &[3]), Value::Float(0.0)],
    )
    .unwrap();
    let values = data(&ieee);
    assert_eq!(values[0], Value::Float(f64::INFINITY));
    assert_eq!(values[1], Value::Float(f64::NEG_INFINITY));
    assert!(matches!(values[2], Value::Float(f) if f.is_nan()));
}

#[test]
fn nan_compares_false_and_unequal_to_itself() {
    let nan = floats(&[f64::NAN, 1.0], &[2]);
    let lt = lift(
        MirLiftOp::Lt,
        bool_element(),
        vec![nan.clone(), nan.clone()],
    )
    .unwrap();
    assert_eq!(data(&lt), vec![Value::Bool(false), Value::Bool(false)]);
    let eq = lift(
        MirLiftOp::Eq,
        bool_element(),
        vec![nan.clone(), nan.clone()],
    )
    .unwrap();
    assert_eq!(data(&eq), vec![Value::Bool(false), Value::Bool(true)]);
    let ne = lift(MirLiftOp::NotEq, bool_element(), vec![nan.clone(), nan]).unwrap();
    assert_eq!(data(&ne), vec![Value::Bool(true), Value::Bool(false)]);
}

#[test]
fn extrema_propagate_nan_like_the_scalar_operator() {
    let out = lift(
        MirLiftOp::Max,
        f32_element(),
        vec![floats(&[1.0, f64::NAN], &[2]), floats(&[2.0, 5.0], &[2])],
    )
    .unwrap();
    let values = data(&out);
    assert_eq!(values[0], Value::Float(2.0));
    assert!(matches!(values[1], Value::Float(f) if f.is_nan()));
    let min = lift(
        MirLiftOp::Min,
        Type::Primitive(Primitive::Numerus),
        vec![ints(&[3, -3], &[2]), ints(&[1, 9], &[2])],
    )
    .unwrap();
    assert_eq!(data(&min), vec![Value::Int(1), Value::Int(-3)]);
}

#[test]
fn an_empty_tensor_lifts_to_an_empty_tensor() {
    let out = lift(
        MirLiftOp::Add,
        f32_element(),
        vec![floats(&[], &[0]), Value::Float(1.0)],
    )
    .unwrap();
    assert!(data(&out).is_empty());
    let Value::Tensor(cell) = &out else {
        unreachable!()
    };
    assert_eq!(cell.borrow().shape, vec![0]);
}

#[test]
fn rank_two_and_three_keep_their_shape() {
    for shape in [vec![2, 2], vec![2, 1, 2]] {
        let out = lift(
            MirLiftOp::Mul,
            f32_element(),
            vec![floats(&[1.0, 2.0, 3.0, 4.0], &shape), Value::Float(2.0)],
        )
        .unwrap();
        let Value::Tensor(cell) = &out else {
            unreachable!()
        };
        assert_eq!(cell.borrow().shape, shape);
        assert_eq!(cell.borrow().value_at(3), Some(Value::Float(8.0)));
    }
}

#[test]
fn two_tensors_of_different_shapes_are_an_error() {
    let err = lift(
        MirLiftOp::Gt,
        bool_element(),
        vec![floats(&[1.0, 2.0], &[2]), floats(&[1.0, 2.0, 3.0], &[3])],
    )
    .expect_err("shape mismatch");
    assert_eq!(err.issue, "runner_tensor_lift_shape_mismatch");
    // Same shape, different backing lengths: the backing is checked too.
    let err = lift(
        MirLiftOp::Gt,
        bool_element(),
        vec![floats(&[1.0, 2.0], &[2]), floats(&[1.0], &[2])],
    )
    .expect_err("backing mismatch");
    assert_eq!(err.issue, "runner_tensor_lift_backing_length_mismatch");
}

#[test]
fn select_coalesce_and_logic_take_scalar_and_tensor_operands() {
    let mask = tensor(vec![Value::Bool(true), Value::Bool(false)], &[2]);
    let select = lift(
        MirLiftOp::Select,
        f32_element(),
        vec![mask.clone(), floats(&[1.0, 2.0], &[2]), Value::Float(9.0)],
    )
    .unwrap();
    assert_eq!(data(&select), vec![Value::Float(1.0), Value::Float(9.0)]);
    let or = lift(
        MirLiftOp::Or,
        bool_element(),
        vec![mask.clone(), Value::Bool(false)],
    )
    .unwrap();
    assert_eq!(data(&or), vec![Value::Bool(true), Value::Bool(false)]);
    let not = lift(MirLiftOp::Not, bool_element(), vec![mask]).unwrap();
    assert_eq!(data(&not), vec![Value::Bool(false), Value::Bool(true)]);
    let holes = tensor(vec![Value::Int(4), Value::Nil], &[2]);
    let filled = lift(
        MirLiftOp::Coalesce,
        Type::Primitive(Primitive::Numerus),
        vec![holes, Value::Int(0)],
    )
    .unwrap();
    assert_eq!(data(&filled), vec![Value::Int(4), Value::Int(0)]);
}

#[test]
fn f32_results_round_at_the_element_width() {
    let out = lift(
        MirLiftOp::Div,
        f32_element(),
        vec![floats(&[1.0], &[1]), Value::Float(3.0)],
    )
    .unwrap();
    assert_eq!(data(&out), vec![Value::Float(f64::from(1.0_f32 / 3.0_f32))]);
}

#[test]
fn whole_equality_is_one_verdict_over_shape_and_every_element() {
    let a = floats(&[1.0, 2.0, 3.0, 4.0], &[2, 2]);
    let same = floats(&[1.0, 2.0, 3.0, 4.0], &[2, 2]);
    let other = floats(&[1.0, 2.0, 3.0, 5.0], &[2, 2]);
    let verdict = |op, left: &Value, right: &Value| {
        lift(op, bool_element(), vec![left.clone(), right.clone()]).unwrap()
    };
    assert_eq!(verdict(MirLiftOp::WholeEq, &a, &same), Value::Bool(true));
    assert_eq!(
        verdict(MirLiftOp::WholeNotEq, &a, &same),
        Value::Bool(false)
    );
    assert_eq!(verdict(MirLiftOp::WholeEq, &a, &other), Value::Bool(false));
    assert_eq!(
        verdict(MirLiftOp::WholeNotEq, &a, &other),
        Value::Bool(true)
    );
}

#[test]
fn whole_equality_across_shapes_is_false_never_an_error() {
    let flat = floats(&[1.0, 2.0, 3.0, 4.0], &[4]);
    let square = floats(&[1.0, 2.0, 3.0, 4.0], &[2, 2]);
    let shorter = floats(&[1.0, 2.0], &[2]);
    for other in [&square, &shorter] {
        let eq = lift(
            MirLiftOp::WholeEq,
            bool_element(),
            vec![flat.clone(), other.clone()],
        )
        .unwrap();
        assert_eq!(eq, Value::Bool(false));
        let ne = lift(
            MirLiftOp::WholeNotEq,
            bool_element(),
            vec![flat.clone(), other.clone()],
        )
        .unwrap();
        assert_eq!(ne, Value::Bool(true));
    }
}

#[test]
fn whole_equality_follows_ieee_nan_and_compares_holes_and_bools() {
    // A tensor holding NaN is not equal to itself (IEEE, no special case).
    let nan = floats(&[f64::NAN, 1.0], &[2]);
    let eq = lift(
        MirLiftOp::WholeEq,
        bool_element(),
        vec![nan.clone(), nan.clone()],
    )
    .unwrap();
    assert_eq!(eq, Value::Bool(false));
    let ne = lift(
        MirLiftOp::WholeNotEq,
        bool_element(),
        vec![nan.clone(), nan],
    )
    .unwrap();
    assert_eq!(ne, Value::Bool(true));
    let holes = tensor(vec![Value::Int(4), Value::Nil], &[2]);
    let eq = lift(
        MirLiftOp::WholeEq,
        bool_element(),
        vec![holes.clone(), holes],
    )
    .unwrap();
    assert_eq!(eq, Value::Bool(true));
    let mask = tensor(vec![Value::Bool(true), Value::Bool(false)], &[2]);
    let flipped = tensor(vec![Value::Bool(false), Value::Bool(false)], &[2]);
    let eq = lift(
        MirLiftOp::WholeEq,
        bool_element(),
        vec![mask.clone(), flipped],
    )
    .unwrap();
    assert_eq!(eq, Value::Bool(false));
    let eq = lift(MirLiftOp::WholeEq, bool_element(), vec![mask.clone(), mask]).unwrap();
    assert_eq!(eq, Value::Bool(true));
}

#[test]
fn whole_equality_needs_two_tensors() {
    let err = lift(
        MirLiftOp::WholeEq,
        bool_element(),
        vec![floats(&[1.0], &[1]), Value::Float(1.0)],
    )
    .expect_err("a scalar is not a whole tensor");
    assert_eq!(
        err.issue, "runner_tensor_equality_needs_two_tensor_operands",
        "{err:?}"
    );
}
