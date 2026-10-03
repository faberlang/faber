use super::*;
use crate::validate::ValidatedMir;
use crate::*;
use radix_types::{IndexExpr, NumericWidth, Primitive, Type, TypeTable};

fn source_index(mapping: &TensorExpansion, target_index: u64) -> u64 {
    mapping
        .index_terms()
        .iter()
        .map(|(_, stride, extent, source_stride)| (target_index / stride % extent) * source_stride)
        .sum()
}

#[test]
fn mask_repeats_over_heads_queries_and_rows() {
    let mapping = TensorExpansion::new(&[1, 76], &[5, 3, 1, 76]).unwrap();
    assert_eq!(mapping.source_axes, [2, 3]);
    for index in 0..5 * 3 * 76 {
        assert_eq!(source_index(&mapping, index), index % 76);
    }
    let mapping = TensorExpansion::new(&[1, 76], &[5, 3, 8, 76]).unwrap();
    for index in 0..5 * 3 * 8 * 76 {
        assert_eq!(source_index(&mapping, index), index % 76);
    }
}

#[test]
fn inserted_middle_axes_preserve_nontrailing_source_coordinates() {
    let mapping = TensorExpansion::new(&[2, 3], &[2, 5, 3]).unwrap();
    assert_eq!(mapping.source_axes, [0, 2]);
    for index in 0..30 {
        assert_eq!(source_index(&mapping, index), (index / 15) * 3 + index % 3);
    }
}

#[test]
fn exact_extents_win_then_rightmost_axes_break_ties() {
    assert_eq!(
        TensorExpansion::new(&[1], &[1, 8]).unwrap().source_axes,
        [0]
    );
    assert_eq!(
        TensorExpansion::new(&[2], &[2, 2]).unwrap().source_axes,
        [1]
    );
    let scalar = TensorExpansion::new(&[], &[2, 3]).unwrap();
    assert_eq!(source_index(&scalar, 5), 0);
}

#[test]
fn incompatible_zero_and_overflow_shapes_fail_closed() {
    for (source, target) in [
        (vec![2, 3], vec![3, 2]),
        (vec![2, 3], vec![6]),
        (vec![2], vec![0, 2]),
        (vec![0], vec![0]),
        (vec![1], vec![u64::from(u32::MAX), 2]),
    ] {
        assert!(TensorExpansion::new(&source, &target).is_none());
    }
}

fn tensor_ty(types: &mut TypeTable, dims: &[u64]) -> MirType {
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dims = dims
        .iter()
        .map(|d| types.intern_index(IndexExpr::Literal(*d)))
        .collect();
    let shape = types.intern_index(IndexExpr::Tuple(dims));
    MirType::semantic(types.intern(Type::Tensor(f32, shape)))
}

fn expansion_function(types: &mut TypeTable, shape_copy: bool) -> MirFunction {
    let scores = tensor_ty(types, &[5, 3, 1, 76]);
    let mask = tensor_ty(types, &[1, 76]);
    let lista = MirType::semantic(types.array(types.primitive(Primitive::Numerus)));
    let span = radix_lexer::Span::default();
    let mut statements = vec![MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::temp(MirTempId(0))),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorShape),
                args: vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                return_ty: lista,
            },
        },
        span,
    }];
    if shape_copy {
        statements.push(MirStatement {
            kind: MirStatementKind::Assign {
                place: MirPlace::local(MirLocalId(3)),
                value: MirValue {
                    id: MirValueId(0),
                    kind: MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
                    ty: lista,
                    span,
                },
            },
            span,
        });
    }
    statements.push(MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::local(MirLocalId(2))),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorExpand),
                args: vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(1))),
                    if shape_copy {
                        MirOperand::Place(MirPlace::local(MirLocalId(3)))
                    } else {
                        MirOperand::Temp(MirTempId(0))
                    },
                ],
                return_ty: scores,
            },
        },
        span,
    });
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: [(0, scores), (1, mask)]
            .into_iter()
            .map(|(id, ty)| MirParam {
                local: MirLocalId(id),
                name: None,
                ty,
                mode: MirParamMode::Owned,
                span,
            })
            .collect(),
        locals: vec![
            MirLocal {
                id: MirLocalId(2),
                name: None,
                ty: scores,
                mutable: false,
                span,
            },
            MirLocal {
                id: MirLocalId(3),
                name: None,
                ty: lista,
                mutable: false,
                span,
            },
        ],
        temps: vec![MirTemp {
            id: MirTempId(0),
            ty: lista,
            span,
        }],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(2),
                )))),
                span,
            },
            span,
        }],
        return_ty: scores,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

#[test]
fn direct_temp_and_forwarded_shape_lists_fold_to_typed_witnesses() {
    for shape_copy in [false, true] {
        let mut types = TypeTable::new();
        let function = expansion_function(&mut types, shape_copy);
        let validation = MirValidationContext::new(&types);
        ValidatedMir::new(
            MirProgram {
                functions: vec![function.clone()],
            },
            validation.clone(),
        )
        .unwrap();
        let folded = fold_expansion_witnesses(&function, &validation).unwrap();
        assert_eq!(folded.blocks[0].statements.len(), 1);
        let MirStatementKind::RuntimeCall { call, .. } = &folded.blocks[0].statements[0].kind
        else {
            panic!("expansion call");
        };
        assert_eq!(
            call.args[1],
            MirOperand::Place(MirPlace::local(MirLocalId(0)))
        );
        assert!(TensorExpansion::for_call(&folded, call, &validation).is_some());
        ValidatedMir::new(
            MirProgram {
                functions: vec![folded.clone()],
            },
            validation.clone(),
        )
        .unwrap();
        assert_eq!(
            fold_expansion_witnesses(&folded, &validation).unwrap(),
            folded
        );
    }
}

#[test]
fn escaping_shape_list_is_not_erased() {
    let mut types = TypeTable::new();
    let mut function = expansion_function(&mut types, false);
    function.return_ty = function.temps[0].ty;
    function.blocks[0].terminator.kind =
        MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(0))));
    let validation = MirValidationContext::new(&types);
    let folded = fold_expansion_witnesses(&function, &validation).unwrap();
    assert_eq!(
        folded.blocks[0].statements.len(),
        2,
        "escaping shape list remains live"
    );
}

#[test]
fn wrong_witness_shape_is_rejected_even_with_equal_element_counts() {
    let mut types = TypeTable::new();
    let mut function = expansion_function(&mut types, false);
    function.params[0].ty = tensor_ty(&mut types, &[15, 76]);
    let validation = MirValidationContext::new(&types);
    let error = fold_expansion_witnesses(&function, &validation).unwrap_err();
    assert_eq!(error.kind, ShapeFoldKind::NotDeviceMaterializable);
}

#[test]
fn unrelated_shape_producer_is_not_erased_by_expansion_fold() {
    let mut types = TypeTable::new();
    let mut function = expansion_function(&mut types, false);
    let mut unrelated = function.blocks[0].statements[0].clone();
    let MirStatementKind::RuntimeCall { destination, .. } = &mut unrelated.kind else {
        unreachable!();
    };
    *destination = Some(MirPlace::temp(MirTempId(1)));
    function.temps.push(MirTemp {
        id: MirTempId(1),
        ty: function.temps[0].ty,
        span: function.span,
    });
    function.blocks[0].statements.push(unrelated.clone());
    let validation = MirValidationContext::new(&types);
    let folded = fold_expansion_witnesses(&function, &validation).unwrap();
    assert!(folded.blocks[0].statements.contains(&unrelated));
    assert_eq!(folded.blocks[0].statements.len(), 2);
}
