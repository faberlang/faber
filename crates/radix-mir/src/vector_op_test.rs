//! `MirIntrinsic::VectorOp`: arity, result shape, and the survivor net.
//!
//! A deferred vector op is legal inside a template and nowhere else; every
//! function that reaches validation must have been expanded first. Tests
//! assert the stable issue code and error counts, never diagnostic prose.

use crate::{
    DEFERRED_VECTOR_OP_SURVIVED, MirBlock, MirBlockId, MirConstant, MirFunction, MirFunctionId,
    MirIntrinsic, MirLaneOp, MirLocal, MirLocalId, MirOperand, MirPlace, MirPlaceBase, MirProgram,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirType,
    MirValidationContext, MirVectorOp, ValidatedMir, validate_program,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, Primitive, TypeTable};

fn ops() -> Vec<MirVectorOp> {
    vec![
        MirVectorOp::Elementwise {
            op: MirLaneOp::Add,
            checked: false,
        },
        MirVectorOp::Dot,
        MirVectorOp::Cross,
        MirVectorOp::Norm,
        MirVectorOp::Normalized,
        MirVectorOp::Crea,
        MirVectorOp::Swizzle(vec![2, 1, 0]),
    ]
}

/// One function whose only statement is a `VectorOp` call with `arg_count`
/// constant operands, assigned to a local of the given result type.
fn program_with(
    op: MirVectorOp,
    arg_count: usize,
    types: &mut TypeTable,
    vector_result: bool,
) -> MirProgram {
    let element = types.primitive(Primitive::Fractus);
    let width = types.intern_index(IndexExpr::Literal(3));
    let vector = types.vector_with_width(element, width);
    let return_ty = MirType::semantic(if vector_result { vector } else { element });
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let args = (0..arg_count)
        .map(|_| MirOperand::Constant(MirConstant::Float(1.0)))
        .collect();
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: vec![],
            locals: vec![MirLocal {
                id: MirLocalId(0),
                ty: return_ty,
                name: None,
                mutable: false,
                span: Span::default(),
            }],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::RuntimeCall {
                        destination: Some(MirPlace {
                            base: MirPlaceBase::Local(MirLocalId(0)),
                            projections: vec![],
                        }),
                        call: MirRuntimeCall {
                            intrinsic: MirIntrinsic::VectorOp(op),
                            args,
                            return_ty,
                        },
                    },
                    span: Span::default(),
                }],
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
    match validate_program(program, &context) {
        Ok(()) => Vec::new(),
        Err(errors) => errors.into_iter().map(|error| error.issue).collect(),
    }
}

#[test]
fn arity_matches_the_operand_count_each_op_takes() {
    for op in ops() {
        let want = match op {
            MirVectorOp::Elementwise { .. } | MirVectorOp::Dot | MirVectorOp::Cross => 2,
            _ => 1,
        };
        assert_eq!(op.arity(), want, "{op:?}");
    }
}

#[test]
fn survivor_is_rejected_by_name_for_every_op() {
    for op in ops() {
        let mut types = TypeTable::new();
        let program = program_with(op.clone(), op.arity(), &mut types, op.returns_vector());
        assert_eq!(
            issues(&program, &types),
            vec![DEFERRED_VECTOR_OP_SURVIVED.to_owned()],
            "{op:?}"
        );
    }
}

#[test]
fn survivor_blocks_validated_mir_construction() {
    let mut types = TypeTable::new();
    let program = program_with(MirVectorOp::Dot, 2, &mut types, false);
    let context = MirValidationContext::new(&types);
    let Err(errors) = ValidatedMir::new(program, context) else {
        panic!("a surviving VectorOp must not validate");
    };
    assert!(
        errors
            .iter()
            .any(|error| error.issue == DEFERRED_VECTOR_OP_SURVIVED)
    );
}

#[test]
fn wrong_arity_is_rejected_in_addition_to_the_survivor() {
    for op in ops() {
        let mut types = TypeTable::new();
        let program = program_with(op.clone(), op.arity() + 1, &mut types, op.returns_vector());
        let found = issues(&program, &types);
        assert_eq!(found.len(), 2, "{op:?}: {found:?}");
        assert!(
            found.contains(&DEFERRED_VECTOR_OP_SURVIVED.to_owned()),
            "{op:?}"
        );
        assert!(
            found
                .iter()
                .any(|issue| issue != DEFERRED_VECTOR_OP_SURVIVED),
            "{op:?}"
        );
    }
}

#[test]
fn vector_result_ops_reject_a_scalar_result_type() {
    for op in ops().into_iter().filter(MirVectorOp::returns_vector) {
        let mut types = TypeTable::new();
        let program = program_with(op.clone(), op.arity(), &mut types, false);
        assert_eq!(issues(&program, &types).len(), 2, "{op:?}");
    }
}
