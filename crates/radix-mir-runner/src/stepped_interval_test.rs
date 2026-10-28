//! Stepped interval construct: a stored step lets the bounds
//! run backwards, and the step itself must stay positive.

use super::*;

use crate::host::BufferHost;
use radix_lexer::Interner;
use radix_mir::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlockId,
    MirCollectionOp, MirIntrinsic, MirOperand, MirProgram, MirRuntimeCall, MirType,
    MirValidationContext, MirValueId,
};
use radix_types::{Primitive, TypeId, TypeTable};

/// One empty function with no params, locals, or blocks.
fn empty_function() -> MirFunction {
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

/// Evaluate an `Interval` construct over `bounds`, stored as Int frame values
/// (`start`, `end`, and an optional step).
fn construct_interval(bounds: &[i64], inclusive: bool) -> Result<Value, RunnerError> {
    let type_table = TypeTable::new();
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(empty_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let function = program.functions.first().unwrap();
    runner.push_frame(function.id, MirBlockId(0));

    let frame = runner.frames.last_mut().unwrap();
    let fields = bounds
        .iter()
        .enumerate()
        .map(|(index, bound)| {
            let id = MirValueId(u32::try_from(index).expect("bound index fits a MirValueId"));
            frame.values.insert(id, Value::Int(*bound));
            MirAggregateItem::Operand(MirOperand::Value(id))
        })
        .collect();

    let aggregate = MirAggregate {
        kind: MirAggregateKind::Interval { inclusive },
        ty: MirType::semantic(TypeId(0)),
        fields: MirAggregateFields::Ordered(fields),
    };

    runner.eval_construct(&aggregate, Span::default())
}

/// A stepped range takes its direction from its bounds: `10‥0 per 2` stores a
/// descending range with a positive step.
#[test]
fn stepped_construct_descending_bounds_accepted() {
    let value = construct_interval(&[10, 0, 2], false).expect("descending stepped interval");
    assert_eq!(
        value,
        Value::Tuple(vec![
            Value::Int(10),
            Value::Int(0),
            Value::Bool(false),
            Value::Int(2),
        ])
    );
}

/// A zero step dies: the step is a stride, never a direction.
#[test]
fn stepped_construct_zero_step_rejected() {
    let error = construct_interval(&[0, 10, 0], false).expect_err("zero step must be rejected");
    assert!(
        error.issue.contains("runner_interval_step_not_positive"),
        "issue must be runner_interval_step_not_positive, got: {}",
        error.issue
    );
}

/// F-082 is unchanged for unstepped ranges: reversed bounds stay rejected.
#[test]
fn stepped_construct_unstepped_reversed_still_rejected() {
    let error = construct_interval(&[5, 1], false).expect_err("reversed bounds must be rejected");
    assert!(
        error.issue.contains("monotonic"),
        "issue must mention monotonic, got: {}",
        error.issue
    );
}

/// Evaluate one interval collection op over `args`, stored as frame values.
/// Materialize returns a `lista<numerus>`; every other interval op returns
/// `numerus`.
fn eval_interval_op(op: MirCollectionOp, args: Vec<Value>) -> Result<Value, RunnerError> {
    let mut type_table = TypeTable::new();
    let numerus = type_table.primitive(Primitive::Numerus);
    let return_ty = if matches!(op, MirCollectionOp::IntervalMaterialize) {
        MirType::semantic(type_table.array(numerus))
    } else {
        MirType::semantic(numerus)
    };
    let context = MirValidationContext::new(&type_table);
    let mut program = MirProgram::new();
    program.functions.push(empty_function());

    let mut host = BufferHost::default();
    let interner = Interner::new();
    let mut runner = crate::Runner::new(&program, &context, &type_table, &interner, &mut host);

    let function = program.functions.first().unwrap();
    runner.push_frame(function.id, MirBlockId(0));

    let frame = runner.frames.last_mut().unwrap();
    let args = args
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let id = MirValueId(u32::try_from(index).expect("argument index fits a MirValueId"));
            frame.values.insert(id, value);
            MirOperand::Value(id)
        })
        .collect();

    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(op),
        args,
        return_ty,
    };
    runner.eval_runtime_call(&call, Span::default())
}

/// A stepped carrier: start, end, inclusivity, and the stride.
fn stepped_carrier(start: i64, end: i64, inclusive: bool, step: i64) -> Value {
    Value::Tuple(vec![
        Value::Int(start),
        Value::Int(end),
        Value::Bool(inclusive),
        Value::Int(step),
    ])
}

/// `length()` counts members, so an inclusive range contributes its final
/// partial stride while an exclusive one does not.
#[test]
fn stepped_op_length_counts_members() {
    let inclusive = stepped_carrier(0, 10, true, 2);
    assert_eq!(
        eval_interval_op(MirCollectionOp::IntervalLength, vec![inclusive]).unwrap(),
        Value::Int(6)
    );
    let exclusive = stepped_carrier(0, 10, false, 2);
    assert_eq!(
        eval_interval_op(MirCollectionOp::IntervalLength, vec![exclusive]).unwrap(),
        Value::Int(5)
    );
    let descending = stepped_carrier(10, 0, false, 2);
    assert_eq!(
        eval_interval_op(MirCollectionOp::IntervalLength, vec![descending]).unwrap(),
        Value::Int(5)
    );
}

/// `contains(v)` is true only for on-step members, and a descending carrier
/// keeps its direction from the bounds.
#[test]
fn stepped_op_contains_on_step_membership() {
    let ascending = stepped_carrier(0, 10, true, 2);
    assert_eq!(
        eval_interval_op(
            MirCollectionOp::IntervalContains,
            vec![ascending.clone(), Value::Int(3)],
        )
        .unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        eval_interval_op(
            MirCollectionOp::IntervalContains,
            vec![ascending, Value::Int(4)],
        )
        .unwrap(),
        Value::Bool(true)
    );
    let descending = stepped_carrier(10, 0, false, 2);
    assert_eq!(
        eval_interval_op(
            MirCollectionOp::IntervalContains,
            vec![descending.clone(), Value::Int(9)],
        )
        .unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        eval_interval_op(
            MirCollectionOp::IntervalContains,
            vec![descending, Value::Int(8)],
        )
        .unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn interval_comparisons_keep_large_integers_exact() {
    let base = 1_i64 << 53;
    let contains = Value::Tuple(vec![
        Value::Int(base + 1),
        Value::Int(base + 2),
        Value::Bool(true),
    ]);
    assert_eq!(
        eval_interval_op(
            MirCollectionOp::IntervalContains,
            vec![contains, Value::Int(base)],
        )
        .unwrap(),
        Value::Bool(false)
    );

    let left = Value::Tuple(vec![Value::Int(base), Value::Int(base), Value::Bool(true)]);
    let right = Value::Tuple(vec![
        Value::Int(base + 1),
        Value::Int(base + 1),
        Value::Bool(true),
    ]);
    assert_eq!(
        eval_interval_op(
            MirCollectionOp::IntervalIntersect,
            vec![left.clone(), right.clone()],
        )
        .unwrap(),
        Value::Nil
    );
    assert_eq!(
        eval_interval_op(MirCollectionOp::IntervalUnion, vec![left, right]).unwrap(),
        Value::Nil
    );
}

/// Materializing lists the stepped members in the carrier's direction.
#[test]
fn stepped_op_materialize_lists_stepped_members() {
    let ascending = stepped_carrier(0, 10, true, 2);
    assert_eq!(
        eval_interval_op(MirCollectionOp::IntervalMaterialize, vec![ascending]).unwrap(),
        Value::Array(Rc::new(RefCell::new(vec![
            Value::Int(0),
            Value::Int(2),
            Value::Int(4),
            Value::Int(6),
            Value::Int(8),
            Value::Int(10),
        ])))
    );
    let descending = stepped_carrier(10, 0, false, 2);
    assert_eq!(
        eval_interval_op(MirCollectionOp::IntervalMaterialize, vec![descending]).unwrap(),
        Value::Array(Rc::new(RefCell::new(vec![
            Value::Int(10),
            Value::Int(8),
            Value::Int(6),
            Value::Int(4),
            Value::Int(2),
        ])))
    );
}

/// An unstepped carrier still strides by one.
#[test]
fn stepped_op_materialize_unstepped_defaults_to_one() {
    let unstepped = Value::Tuple(vec![Value::Int(0), Value::Int(3), Value::Bool(false)]);
    assert_eq!(
        eval_interval_op(MirCollectionOp::IntervalMaterialize, vec![unstepped]).unwrap(),
        Value::Array(Rc::new(RefCell::new(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
        ])))
    );
}
