//! C2 runner walker pins for queue/stack core collection operations.

use super::lib_test::evaluate_collection_call;
use crate::Value;
use crate::host::BufferHost;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirLocal, MirOperand,
    MirPlace, MirProgram, MirProjection, MirRuntimeCall, MirType, MirValidationContext, MirValueId,
};
use radix_types::{IndexExpr, Primitive, TypeId, TypeTable};
use std::cell::RefCell;
use std::rc::Rc;

fn call(op: MirCollectionOp, args: usize) -> MirRuntimeCall {
    let operands = (0..args)
        .map(|index| {
            MirOperand::Value(MirValueId(
                u32::try_from(index).expect("arg count fits u32"),
            ))
        })
        .collect();
    MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(op),
        args: operands,
        return_ty: MirType::semantic(TypeId(0)),
    }
}

#[test]
fn queue_walk_is_fifo_and_length_tracks_live_window() {
    let items = Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(2)]));
    let push = call(MirCollectionOp::Append, 2);
    assert_eq!(
        evaluate_collection_call(
            &push,
            vec![
                (MirValueId(0), Value::Array(items.clone())),
                (MirValueId(1), Value::Int(3))
            ],
        )
        .expect("queue push"),
        Value::Unit
    );
    assert_eq!(
        *items.borrow(),
        vec![Value::Int(1), Value::Int(2), Value::Int(3)]
    );

    let pop = call(MirCollectionOp::RemoveFirst, 1);
    assert_eq!(
        evaluate_collection_call(&pop, vec![(MirValueId(0), Value::Array(items.clone()))])
            .expect("queue pop"),
        Value::Int(1)
    );
    let length = call(MirCollectionOp::Length, 1);
    assert_eq!(
        evaluate_collection_call(&length, vec![(MirValueId(0), Value::Array(items.clone()))])
            .expect("queue length"),
        Value::Int(2)
    );
}

#[test]
fn bounded_push_through_list_index_checks_projected_capacity() {
    let mut types = TypeTable::new();
    let element = types.primitive(Primitive::Numerus);
    let capacity = types.intern_index(IndexExpr::Literal(1));
    let inner = types.bounded_array(element, capacity);
    let outer = types.array(inner);
    let vacuum = types.primitive(Primitive::Vacuum);
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: vec![MirLocal {
            id: radix_mir::MirLocalId(0),
            name: None,
            ty: MirType::semantic(outer),
            mutable: true,
            span: Span::default(),
        }],
        temps: Vec::new(),
        blocks: Vec::new(),
        return_ty: MirType::semantic(vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let context = MirValidationContext::new(&types);
    let interner = Interner::new();
    let mut host = BufferHost::default();
    let mut runner = super::Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner.frames.last_mut().unwrap().seed_local(
        radix_mir::MirLocalId(0),
        Value::Array(Rc::new(RefCell::new(vec![Value::Array(Rc::new(
            RefCell::new(vec![Value::Int(7)]),
        ))]))),
    );
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Append),
        args: vec![
            MirOperand::Place({
                let mut place = MirPlace::local(radix_mir::MirLocalId(0));
                place
                    .projections
                    .push(MirProjection::Index(MirOperand::Constant(
                        radix_mir::MirConstant::Int(0),
                    )));
                place
            }),
            MirOperand::Value(MirValueId(0)),
        ],
        return_ty: MirType::semantic(vacuum),
    };
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), Value::Int(8));
    let error = runner
        .eval_runtime_call(&call, Span::default())
        .expect_err("append through an index must honor the inner list capacity");
    assert_eq!(error.issue, "runner_collection_push_capacity");
}

#[test]
fn bounded_push_through_struct_field_checks_projected_capacity() {
    let mut types = TypeTable::new();
    let element = types.primitive(Primitive::Numerus);
    let capacity = types.intern_index(IndexExpr::Literal(1));
    let inner = types.bounded_array(element, capacity);
    let record = types.intern(radix_types::Type::Struct(radix_hir::DefId(9)));
    let vacuum = types.primitive(Primitive::Vacuum);
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: vec![MirLocal {
            id: radix_mir::MirLocalId(0),
            name: None,
            ty: MirType::semantic(record),
            mutable: true,
            span: Span::default(),
        }],
        temps: Vec::new(),
        blocks: Vec::new(),
        return_ty: MirType::semantic(vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let mut context = MirValidationContext::new(&types);
    let mut interner = Interner::new();
    let field = interner.intern("items");
    context.struct_fields.insert(
        radix_hir::DefId(9),
        handroll::FxHashMap::from_iter([(field, MirType::semantic(inner))]),
    );
    let mut host = BufferHost::default();
    let mut runner = super::Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner.frames.last_mut().unwrap().seed_local(
        radix_mir::MirLocalId(0),
        Value::Struct(
            radix_hir::DefId(9),
            Rc::new(RefCell::new(handroll::FxHashMap::from_iter([(
                field,
                Value::Array(Rc::new(RefCell::new(vec![Value::Int(7)]))),
            )]))),
        ),
    );
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Append),
        args: vec![
            MirOperand::Place({
                let mut place = MirPlace::local(radix_mir::MirLocalId(0));
                place.projections.push(MirProjection::Field(field));
                place
            }),
            MirOperand::Value(MirValueId(0)),
        ],
        return_ty: MirType::semantic(vacuum),
    };
    runner
        .frames
        .last_mut()
        .unwrap()
        .values
        .insert(MirValueId(0), Value::Int(8));
    let error = runner
        .eval_runtime_call(&call, Span::default())
        .expect_err("append through a field must honor the field list capacity");
    assert_eq!(error.issue, "runner_collection_push_capacity");
}

#[test]
fn stack_walk_is_lifo_and_empty_pop_is_nihil() {
    let items = Rc::new(RefCell::new(vec![Value::Str("bottom".to_owned())]));
    let push = call(MirCollectionOp::Append, 2);
    evaluate_collection_call(
        &push,
        vec![
            (MirValueId(0), Value::Array(items.clone())),
            (MirValueId(1), Value::Str("top".to_owned())),
        ],
    )
    .expect("stack push");

    let pop = call(MirCollectionOp::RemoveLast, 1);
    assert_eq!(
        evaluate_collection_call(&pop, vec![(MirValueId(0), Value::Array(items.clone()))])
            .expect("stack pop"),
        Value::Str("top".to_owned())
    );
    assert_eq!(
        evaluate_collection_call(
            &pop,
            vec![(
                MirValueId(0),
                Value::Array(Rc::new(RefCell::new(Vec::new())))
            )],
        )
        .expect("empty stack pop"),
        Value::Nil
    );
}

#[test]
fn bounded_push_at_capacity_uses_the_recoverable_capacity_pin() {
    let mut types = TypeTable::new();
    let element = types.primitive(Primitive::Numerus);
    let capacity = types.intern_index(IndexExpr::Literal(1));
    let queue = types.bounded_queue(element, capacity);
    let vacuum = types.primitive(Primitive::Vacuum);
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: radix_mir::MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: vec![MirLocal {
            id: radix_mir::MirLocalId(0),
            name: None,
            ty: MirType::semantic(queue),
            mutable: true,
            span: Span::default(),
        }],
        temps: Vec::new(),
        blocks: Vec::new(),
        return_ty: MirType::semantic(vacuum),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    });
    let context = MirValidationContext::new(&types);
    let interner = Interner::new();
    let mut host = BufferHost::default();
    let mut runner = super::Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner
        .frames
        .last_mut()
        .expect("bounded queue frame")
        .seed_local(
            radix_mir::MirLocalId(0),
            Value::Array(Rc::new(RefCell::new(vec![Value::Int(7)]))),
        );
    let call = MirRuntimeCall {
        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Append),
        args: vec![
            MirOperand::Place(MirPlace::local(radix_mir::MirLocalId(0))),
            MirOperand::Value(MirValueId(0)),
        ],
        return_ty: MirType::semantic(vacuum),
    };
    runner
        .frames
        .last_mut()
        .expect("bounded queue frame")
        .values
        .insert(MirValueId(0), Value::Int(8));
    let error = runner
        .eval_runtime_call(&call, Span::default())
        .expect_err("bounded push must leave through the recoverable capacity path");
    assert_eq!(error.issue, "runner_collection_push_capacity");
}
