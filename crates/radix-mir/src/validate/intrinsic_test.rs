use super::*;
use crate::{
    MirAtomicOp, MirBlock, MirBlockId, MirConstant, MirFunction, MirFunctionId, MirIntrinsic,
    MirLocal, MirLocalId, MirOperand, MirPlace, MirProgram, MirRuntimeCall, MirStatement,
    MirStatementKind, MirTerminator, MirTerminatorKind, MirType,
};
use radix_lexer::Span;
use radix_types::{Primitive, TypeTable};

fn place(id: u32) -> MirOperand {
    MirOperand::Place(MirPlace::local(MirLocalId(id)))
}

fn runtime(intrinsic: MirIntrinsic, args: Vec<MirOperand>, return_ty: MirType) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: None,
            call: MirRuntimeCall {
                intrinsic,
                args,
                return_ty,
            },
        },
        span: Span::default(),
    }
}

fn int(value: i64) -> MirOperand {
    MirOperand::Constant(MirConstant::Int(value))
}

fn float(value: f64) -> MirOperand {
    MirOperand::Constant(MirConstant::Float(value))
}

fn atomic(op: MirAtomicOp, args: Vec<MirOperand>, result: MirType) -> MirStatement {
    runtime(MirIntrinsic::Atomic(op), args, result)
}

fn collection(op: crate::MirCollectionOp, result: MirType) -> MirStatement {
    runtime(MirIntrinsic::Collection(op), vec![place(0)], result)
}

fn program(types: &TypeTable, locals: Vec<MirLocal>, statements: Vec<MirStatement>) -> MirProgram {
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals,
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements,
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: vacuum,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        }],
    }
}

fn issues(program: &MirProgram, types: &TypeTable) -> Vec<String> {
    let context = MirValidationContext::new(types);
    validate_program(program, &context)
        .err()
        .unwrap_or_default()
        .into_iter()
        .map(|error| error.issue)
        .collect()
}

fn local(id: u32, ty: MirType) -> MirLocal {
    MirLocal {
        id: MirLocalId(id),
        ty,
        name: None,
        mutable: true,
        span: Span::default(),
    }
}

#[test]
fn atomic_operations_validate_matching_types_and_reject_mismatches() {
    let mut types = TypeTable::new();
    let number_id = types.primitive(Primitive::Numerus);
    let number = MirType::semantic(number_id);
    let atomic_ty = MirType::semantic(types.atomic(number_id));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let boolean = MirType::semantic(types.primitive(Primitive::Bivalens));
    let receiver = place(0);
    let statements = vec![
        atomic(MirAtomicOp::Load, vec![receiver.clone()], number),
        atomic(MirAtomicOp::Store, vec![receiver.clone(), int(7)], vacuum),
        atomic(
            MirAtomicOp::Exchange,
            vec![receiver.clone(), int(8)],
            number,
        ),
        atomic(
            MirAtomicOp::CompareExchange,
            vec![receiver.clone(), int(8), int(9)],
            boolean,
        ),
        atomic(
            MirAtomicOp::Store,
            vec![receiver.clone(), float(1.0)],
            MirType::semantic(types.primitive(Primitive::Textus)),
        ),
        atomic(MirAtomicOp::Load, vec![receiver], vacuum),
    ];
    let mir = program(&types, vec![local(0, atomic_ty)], statements);
    let found = issues(&mir, &types);

    assert_eq!(
        found,
        [
            "mir_validation_atomic_store_does_not_return_vacuum",
            "mir_validation_atomic_store_value_type_mismatch",
            "mir_validation_atomic_load_result_type_mismatch",
        ]
    );
}

#[test]
fn map_key_and_value_snapshots_preserve_their_element_types() {
    let mut types = TypeTable::new();
    let number_id = types.primitive(Primitive::Numerus);
    let text_id = types.primitive(Primitive::Textus);
    let map_ty = MirType::semantic(types.map(number_id, text_id));
    let number_list = MirType::semantic(types.array(number_id));
    let text_list = MirType::semantic(types.array(text_id));
    let wrong_list = MirType::semantic(types.array(number_id));
    let mir = program(
        &types,
        vec![local(0, map_ty)],
        vec![
            collection(crate::MirCollectionOp::Keys, number_list),
            collection(crate::MirCollectionOp::Values, text_list),
            collection(crate::MirCollectionOp::Values, wrong_list),
        ],
    );
    let found = issues(&mir, &types);

    assert_eq!(
        found,
        ["mir_validation_collection_values_result_is_not_an_array_of_the_map_value_type"]
    );
}

#[test]
fn shape_checks_validate_expression_trees_and_bad_witnesses() {
    let mut types = TypeTable::new();
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let wrong_result = MirType::semantic(types.primitive(Primitive::Numerus));
    let nested = crate::MirShapeExpr::Product(vec![
        crate::MirShapeExpr::Literal(3),
        crate::MirShapeExpr::Affine {
            base: Box::new(crate::MirShapeExpr::TensorFieldAxis {
                argument: 1,
                fields: vec![],
                axis: 0,
            }),
            offset: 1,
        },
    ]);
    let check = crate::MirShapeCheck {
        relations: vec![crate::MirShapeRelation {
            left: crate::MirShapeExpr::ScalarArgument { argument: 0 },
            right: crate::MirShapeExpr::Divide {
                numerator: Box::new(crate::MirShapeExpr::TensorAxis {
                    argument: 2,
                    axis: 0,
                }),
                divisor: 0,
            },
        }],
        size_bindings: vec![crate::MirShapeBinding {
            parameter: 0,
            witness: nested.clone(),
        }],
    };
    let mir = program(
        &types,
        vec![],
        vec![
            runtime(MirIntrinsic::TensorShapeCheck(check), vec![], vacuum),
            runtime(MirIntrinsic::TensorShapeSize(nested), vec![], wrong_result),
        ],
    );
    let found = issues(&mir, &types);

    let missing = "mir_validation_tensor_shape_check_references_a_missing_argument";
    assert_eq!(
        found
            .iter()
            .filter(|issue| issue.as_str() == missing)
            .count(),
        4
    );
    assert!(
        found
            .iter()
            .any(|issue| issue == "mir_validation_tensor_shape_check_divides_by_zero")
    );
    assert!(
        found
            .iter()
            .any(|issue| issue == "mir_validation_tensor_shape_size_does_not_return_fractus")
    );
}
