//! `lower_device_copy_into`: the device lowering of `⇇` is the forwarding
//! assign the retired whole-tensor store used. Tests assert structure and
//! validation, never diagnostic prose.

use crate::device_copy_into::lower_device_copy_into;
use crate::{
    MirBlock, MirBlockId, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic, MirLocal,
    MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirPlaceBase, MirProgram,
    MirProjection, MirRuntimeCall, MirStatement, MirStatementKind, MirTempId, MirTerminator,
    MirTerminatorKind, MirType, MirValidationContext, MirValue, MirValueId, MirValueKind,
    validate_program,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, NumericWidth, Primitive, TypeTable};

fn tensor_ty(types: &mut TypeTable, dims: &[u64]) -> MirType {
    let element = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim_ids: Vec<_> = dims
        .iter()
        .map(|&dim| types.intern_index(IndexExpr::Literal(dim)))
        .collect();
    let shape = types.intern_index(IndexExpr::Tuple(dim_ids));
    MirType::semantic(types.tensor_with_shape(element, shape))
}

fn param(id: u32, ty: MirType, mode: MirParamMode) -> MirParam {
    MirParam {
        local: MirLocalId(id),
        name: None,
        ty,
        mode,
        span: Span::default(),
    }
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

fn place(id: u32) -> MirPlace {
    MirPlace::local(MirLocalId(id))
}

fn copy_into(dest: MirOperand, source: MirOperand, vacuum: MirType) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: None,
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorCopyInto),
                args: vec![dest, source],
                return_ty: vacuum,
            },
        },
        span: Span::default(),
    }
}

/// `_0` = source tensor param, `_1` = `mut` destination tensor param.
fn kernel_function(tensor: MirType, vacuum: MirType, statements: Vec<MirStatement>) -> MirFunction {
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![
            param(0, tensor, MirParamMode::Owned),
            param(1, tensor, MirParamMode::MutRef),
        ],
        locals: vec![local(0, tensor), local(1, tensor)],
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
    }
}

fn assigns(function: &MirFunction) -> Vec<(&MirPlace, &MirValue)> {
    function.blocks[0]
        .statements
        .iter()
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::Assign { place, value } => Some((place, value)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_param_destination_becomes_the_forwarding_assign() {
    let mut types = TypeTable::new();
    let tensor = tensor_ty(&mut types, &[2, 3]);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let mut function = kernel_function(
        tensor,
        vacuum,
        vec![copy_into(
            MirOperand::Place(place(1)),
            MirOperand::Place(place(0)),
            vacuum,
        )],
    );

    assert_eq!(lower_device_copy_into(&mut function), 1);

    let assigns = assigns(&function);
    assert_eq!(assigns.len(), 1, "the copy-into call is gone");
    let (target, value) = assigns[0];
    assert_eq!(target, &place(1));
    assert_eq!(value.ty, tensor, "typed by the destination");
    assert_eq!(
        value.kind,
        MirValueKind::Operand(MirOperand::Place(place(0)))
    );

    let program = MirProgram {
        functions: vec![function],
    };
    let context = MirValidationContext::new(&types);
    assert!(
        validate_program(&program, &context).is_ok(),
        "the lowered device body validates"
    );
}

#[test]
fn a_temp_source_is_forwarded_and_value_ids_stay_unique() {
    let mut types = TypeTable::new();
    let tensor = tensor_ty(&mut types, &[2, 3]);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let existing = MirStatement {
        kind: MirStatementKind::Assign {
            place: place(0),
            value: MirValue {
                id: MirValueId(4),
                kind: MirValueKind::Operand(MirOperand::Place(place(0))),
                ty: tensor,
                span: Span::default(),
            },
        },
        span: Span::default(),
    };
    let mut function = kernel_function(
        tensor,
        vacuum,
        vec![
            existing,
            copy_into(
                MirOperand::Place(place(1)),
                MirOperand::Temp(MirTempId(0)),
                vacuum,
            ),
        ],
    );

    assert_eq!(lower_device_copy_into(&mut function), 1);

    let assigns = assigns(&function);
    assert_eq!(assigns.len(), 2);
    assert_eq!(
        assigns[1].1.id,
        MirValueId(5),
        "a fresh id past the maximum"
    );
    assert_eq!(
        assigns[1].1.kind,
        MirValueKind::Operand(MirOperand::Temp(MirTempId(0)))
    );
}

#[test]
fn a_projected_or_temp_destination_is_left_for_the_gate() {
    let mut types = TypeTable::new();
    let tensor = tensor_ty(&mut types, &[2, 3]);
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let projected = MirPlace {
        base: MirPlaceBase::Local(MirLocalId(1)),
        projections: vec![MirProjection::Field(radix_lexer::Symbol(0))],
    };
    let temp_dest = MirPlace {
        base: MirPlaceBase::Temp(MirTempId(0)),
        projections: vec![],
    };
    let mut function = kernel_function(
        tensor,
        vacuum,
        vec![
            copy_into(
                MirOperand::Place(projected),
                MirOperand::Place(place(0)),
                vacuum,
            ),
            copy_into(
                MirOperand::Place(temp_dest),
                MirOperand::Place(place(0)),
                vacuum,
            ),
        ],
    );
    let before = function.clone();

    assert_eq!(lower_device_copy_into(&mut function), 0);
    assert_eq!(
        function, before,
        "nothing rewrites: the device gate rejects by name"
    );
}
