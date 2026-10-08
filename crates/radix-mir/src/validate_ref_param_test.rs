//! A definition callee's `Ref` parameter wrapper: only the `Mutable` wrapper
//! (a semantic signature's spelling of a `mut` parameter) is stripped, so a
//! genuine reference parameter with another access still matches a `Ref`
//! argument of the same inner type.

use super::*;
use crate::{
    MirBlock, MirBlockId, MirCallee, MirFunction, MirFunctionId, MirLocal, MirLocalId, MirOperand,
    MirPlace, MirProgram, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind,
    MirType,
};
use radix_hir::DefId;
use radix_lexer::Span;
use radix_types::{Primitive, SemanticMutability, TypeId, TypeTable};

fn caller_with_call(arg_ty: TypeId, vacuum: TypeId) -> MirProgram {
    let span = Span::default();
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: MirType::semantic(arg_ty),
                mutable: false,
                span,
            }],
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Call {
                        destination: None,
                        callee: MirCallee::definition(DefId(7)),
                        args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                    },
                    span,
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span,
                },
                span,
            }],
            return_ty: MirType::semantic(vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        }],
    }
}

fn call_errors(types: &TypeTable, param_ty: TypeId, arg_ty: TypeId, vacuum: TypeId) -> Vec<String> {
    let mut context = MirValidationContext::new(types);
    context.functions.insert(
        DefId(7),
        MirFunctionSignature {
            params: vec![MirParamSignature {
                ty: MirType::semantic(param_ty),
                optional: false,
            }],
            return_ty: MirType::semantic(vacuum),
            error_ty: None,
        },
    );
    match validate_program(&caller_with_call(arg_ty, vacuum), &context) {
        Ok(()) => Vec::new(),
        Err(errors) => errors.into_iter().map(|error| error.message).collect(),
    }
}

#[test]
fn ref_argument_matches_immutable_and_copy_ref_parameters() {
    for mutability in [SemanticMutability::Immutable, SemanticMutability::Copy] {
        let mut types = TypeTable::new();
        let inner = types.primitive(Primitive::Numerus);
        let vacuum = types.primitive(Primitive::Vacuum);
        let reference = types.reference(mutability, inner);
        assert_eq!(
            call_errors(&types, reference, reference, vacuum),
            Vec::<String>::new()
        );
    }
}

#[test]
fn payload_argument_matches_mutable_ref_parameter() {
    let mut types = TypeTable::new();
    let inner = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);
    let reference = types.reference(SemanticMutability::Mutable, inner);
    assert_eq!(
        call_errors(&types, reference, inner, vacuum),
        Vec::<String>::new()
    );
}
