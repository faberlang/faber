//! Behaviour tests for runtime dispatch paths without an end-to-end source compiler.

use super::*;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirFunction, MirFunctionId, MirIntrinsic, MirOperand, MirProgram, MirRuntimeCall, MirType,
    MirValidationContext,
};
use radix_types::{Primitive, TypeTable};

#[test]
fn float_sum_accumulates_f32_elements_wide_at_large_extent() {
    const ELEMENT_COUNT: u32 = (1 << 25) + 1;
    let count = usize::try_from(ELEMENT_COUNT).unwrap();
    let total = sum_float_values(
        std::iter::repeat_with(|| Value::Float(1.0)).take(count),
        Span::default(),
    )
    .expect("large f32 sum must succeed");

    #[allow(clippy::cast_possible_truncation)]
    let got = total as f32;
    #[allow(clippy::cast_possible_truncation)]
    let expected = f64::from(ELEMENT_COUNT) as f32;
    assert_eq!(got.to_bits(), expected.to_bits());
    assert_eq!(got, 33_554_432.0);
}

fn eval_collection(op: MirCollectionOp, values: Vec<Value>) -> Result<Value, RunnerError> {
    let mut types = TypeTable::new();
    let result_ty = types.primitive(Primitive::Valor);
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
        return_ty: MirType::semantic(result_ty),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut host = crate::host::BufferHost::default();
    let mut runner = crate::Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let frame = runner.frames.last_mut().unwrap();
    let args = values
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            let id = MirValueId(u32::try_from(index).unwrap());
            frame.values.insert(id, value);
            MirOperand::Value(id)
        })
        .collect();
    runner.eval_runtime_call(
        &MirRuntimeCall {
            intrinsic: MirIntrinsic::Collection(op),
            args,
            return_ty: MirType::semantic(result_ty),
        },
        Span::default(),
    )
}

fn set(values: &[i64]) -> Value {
    Value::Set(Rc::new(RefCell::new(
        values
            .iter()
            .map(|value| Value::Int(*value).to_key(Span::default()).unwrap())
            .collect(),
    )))
}

fn set_members(value: Value) -> Vec<i64> {
    let Value::Set(values) = value else {
        panic!("runtime set operation returned a non-set value")
    };
    let mut values = values
        .borrow()
        .iter()
        .map(|key| match key {
            MapKey::Int(value) => *value,
            MapKey::UInt(value) => i64::try_from(*value).unwrap(),
            _ => panic!("test set unexpectedly contains a non-integer key"),
        })
        .collect::<Vec<_>>();
    values.sort_unstable();
    values
}

#[test]
fn set_algebra_returns_the_expected_sets_without_mutating_its_operands() {
    let left = set(&[1, 2, 3]);
    let right = set(&[3, 4]);
    for (op, expected) in [
        (MirCollectionOp::Union, vec![1, 2, 3, 4]),
        (MirCollectionOp::Intersection, vec![3]),
        (MirCollectionOp::Difference, vec![1, 2]),
        (MirCollectionOp::SymmetricDifference, vec![1, 2, 4]),
    ] {
        assert_eq!(
            set_members(eval_collection(op, vec![left.clone(), right.clone()]).unwrap()),
            expected
        );
    }
    assert_eq!(
        eval_collection(
            MirCollectionOp::IsSubset,
            vec![set(&[1, 2]), set(&[1, 2, 3])]
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        eval_collection(
            MirCollectionOp::IsSuperset,
            vec![left.clone(), set(&[1, 2])]
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(set_members(left), vec![1, 2, 3]);
    assert_eq!(set_members(right), vec![3, 4]);
}

fn regex(pattern: &str) -> Value {
    Value::Regex(Rc::new(
        crate::regex_program::RegexProgram::compile(pattern).unwrap(),
    ))
}

fn strings(value: Value) -> Vec<String> {
    let Value::Array(values) = value else {
        panic!("runtime regex operation returned a non-list value")
    };
    values
        .borrow()
        .iter()
        .map(|value| match value {
            Value::Str(text) => text.clone(),
            Value::Match(found) => found.text.clone(),
            _ => panic!("runtime regex operation returned an unexpected list element"),
        })
        .collect()
}

#[test]
fn regex_runtime_operations_keep_matches_splits_and_replacements_literal() {
    let pattern = regex("(?P<word>[a-z]+)");
    let text = Value::Str("éab-xy".into());
    let found = eval_collection(
        MirCollectionOp::RegexFindAll,
        vec![pattern.clone(), text.clone()],
    )
    .unwrap();
    assert_eq!(strings(found), vec!["ab", "xy"]);
    assert_eq!(
        strings(
            eval_collection(
                MirCollectionOp::RegexSplit,
                vec![pattern.clone(), text.clone()],
            )
            .unwrap()
        ),
        vec!["é", "-", ""]
    );
    assert_eq!(
        eval_collection(
            MirCollectionOp::RegexReplace,
            vec![pattern.clone(), text.clone(), Value::Str("$1".into())],
        )
        .unwrap(),
        Value::Str("é$1-$1".into())
    );
    assert_eq!(
        eval_collection(
            MirCollectionOp::RegexReplaceEach,
            vec![
                pattern,
                text,
                Value::Array(Rc::new(RefCell::new(vec![
                    Value::Str("A".into()),
                    Value::Str("B".into()),
                ]))),
            ],
        )
        .unwrap(),
        Value::Str("éA-B".into())
    );
}

#[test]
fn interval_intersection_and_union_report_overlaps_and_gaps() {
    let left = interval_value(Value::Int(1), Value::Int(4), false);
    let right = interval_value(Value::Int(3), Value::Int(6), true);
    assert_eq!(
        eval_collection(
            MirCollectionOp::IntervalIntersect,
            vec![left.clone(), right.clone()],
        )
        .unwrap(),
        interval_value(Value::Int(3), Value::Int(4), false)
    );
    assert_eq!(
        eval_collection(MirCollectionOp::IntervalUnion, vec![left, right]).unwrap(),
        interval_value(Value::Int(1), Value::Int(6), true)
    );
    let first = interval_value(Value::Int(1), Value::Int(2), false);
    let second = interval_value(Value::Int(4), Value::Int(5), true);
    assert_eq!(
        eval_collection(
            MirCollectionOp::IntervalIntersect,
            vec![first.clone(), second.clone()],
        )
        .unwrap(),
        Value::Nil
    );
    assert_eq!(
        eval_collection(MirCollectionOp::IntervalUnion, vec![first, second]).unwrap(),
        Value::Nil
    );
}
