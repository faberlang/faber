//! `expand_vector_ops`: lane counts, block splitting, refusals, and the
//! validation hand-off. Tests assert structure, issue codes and counts, never
//! diagnostic prose.

use crate::vector_expand::{
    VECTOR_CROSS_WIDTH, VECTOR_OP_ARITY, VECTOR_OP_OPERAND_UNRESOLVED, VECTOR_OP_WIDTH_UNSUPPORTED,
    VECTOR_SWIZZLE_LANE, VECTOR_SWIZZLE_WIDTH, expand_vector_ops,
};
use crate::{
    DEFERRED_VECTOR_OP_SURVIVED, MirAggregateFields, MirAggregateKind, MirBinOp, MirBlock,
    MirBlockId, MirConstant, MirFunction, MirFunctionId, MirIntrinsic, MirLocal, MirLocalId,
    MirOperand, MirParam, MirParamMode, MirPlace, MirProgram, MirProjection, MirRuntimeCall,
    MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirType,
    MirValidationContext, MirValue, MirValueId, MirValueKind, MirVectorOp, ValidatedMir,
    validate_program,
};
use radix_lexer::Span;
use radix_types::{IndexExpr, NumericWidth, Primitive, TypeTable};

struct Fixture {
    types: TypeTable,
    vector: MirType,
    scalar: MirType,
}

fn fixture(width: u64) -> Fixture {
    let mut types = TypeTable::new();
    let scalar = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let index = types.intern_index(IndexExpr::Literal(width));
    let vector = types.vector_with_width(scalar, index);
    Fixture {
        types,
        vector: MirType::semantic(vector),
        scalar: MirType::semantic(scalar),
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

fn param(id: u32, ty: MirType) -> MirParam {
    MirParam {
        local: MirLocalId(id),
        name: None,
        ty,
        mode: MirParamMode::Owned,
        span: Span::default(),
    }
}

fn place(id: u32) -> MirPlace {
    MirPlace::local(MirLocalId(id))
}

/// `a` = l0, `b` = l1 (both vector params), `out` = l2, `marker` = l3 (scalar).
/// Block 0: `out = VectorOp(args)`, then `marker = 7.0`, then `return out`.
fn function_with(
    fixture: &Fixture,
    op: MirVectorOp,
    args: Vec<MirOperand>,
    result: MirType,
) -> MirFunction {
    let call = MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(place(2)),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::VectorOp(op),
                args,
                return_ty: result,
            },
        },
        span: Span::default(),
    };
    let marker = MirStatement {
        kind: MirStatementKind::Assign {
            place: place(3),
            value: MirValue {
                id: MirValueId(0),
                kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(7.0))),
                ty: fixture.scalar,
                span: Span::default(),
            },
        },
        span: Span::default(),
    };
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![param(0, fixture.vector), param(1, fixture.vector)],
        locals: vec![local(2, result), local(3, fixture.scalar)],
        temps: vec![],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call, marker],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(place(2)))),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: result,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn ab() -> Vec<MirOperand> {
    vec![MirOperand::Place(place(0)), MirOperand::Place(place(1))]
}

fn a() -> Vec<MirOperand> {
    vec![MirOperand::Place(place(0))]
}

fn function_for(fixture: &Fixture, op: &MirVectorOp) -> MirFunction {
    let args = if op.arity() == 2 { ab() } else { a() };
    let result = if op.returns_vector() {
        fixture.vector
    } else {
        fixture.scalar
    };
    function_with(fixture, op.clone(), args, result)
}

fn program(function: MirFunction) -> MirProgram {
    MirProgram {
        functions: vec![function],
    }
}

fn issues(program: &MirProgram, types: &TypeTable) -> Vec<String> {
    let context = MirValidationContext::new(types);
    match validate_program(program, &context) {
        Ok(()) => Vec::new(),
        Err(errors) => errors.into_iter().map(|error| error.issue).collect(),
    }
}

fn expand(fixture: &Fixture, function: &mut MirFunction) -> Vec<String> {
    expand_vector_ops(&fixture.types, function)
        .into_iter()
        .map(|error| error.issue)
        .collect()
}

fn all_statements(function: &MirFunction) -> impl Iterator<Item = &MirStatement> {
    function.blocks.iter().flat_map(|block| &block.statements)
}

fn has_vector_op(function: &MirFunction) -> bool {
    all_statements(function).any(|statement| {
        matches!(
            &statement.kind,
            MirStatementKind::RuntimeCall { call, .. }
                if matches!(call.intrinsic, MirIntrinsic::VectorOp(_))
        )
    })
}

fn constructs(function: &MirFunction) -> Vec<usize> {
    all_statements(function)
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::Construct { aggregate, .. } => {
                assert_eq!(aggregate.kind, MirAggregateKind::Vector);
                let MirAggregateFields::Ordered(fields) = &aggregate.fields else {
                    panic!("vector construct must be ordered");
                };
                Some(fields.len())
            }
            _ => None,
        })
        .collect()
}

fn binaries(function: &MirFunction, wanted: MirBinOp) -> usize {
    all_statements(function)
        .filter(|statement| {
            matches!(
                &statement.kind,
                MirStatementKind::Assign { value, .. }
                    if matches!(&value.kind, MirValueKind::Binary { op, .. } if *op == wanted)
            )
        })
        .count()
}

const BIN_OPS: [MirBinOp; 5] = [
    MirBinOp::Add,
    MirBinOp::Sub,
    MirBinOp::Mul,
    MirBinOp::Div,
    MirBinOp::Mod,
];

fn ops_for(width: u64) -> Vec<MirVectorOp> {
    let mut ops: Vec<MirVectorOp> = BIN_OPS.into_iter().map(MirVectorOp::Elementwise).collect();
    ops.extend([
        MirVectorOp::Dot,
        MirVectorOp::Length,
        MirVectorOp::Normalized,
        MirVectorOp::Crea,
        MirVectorOp::Swizzle((0..width as u8).rev().collect()),
    ]);
    if width == 3 {
        ops.push(MirVectorOp::Cross);
    }
    ops
}

#[test]
fn every_op_expands_validates_and_leaves_no_vector_op() {
    for width in 2..=4 {
        let fixture = fixture(width);
        for op in ops_for(width) {
            let mut function = function_for(&fixture, &op);
            let errors = expand(&fixture, &mut function);
            assert!(errors.is_empty(), "w{width} {op:?}: {errors:?}");
            assert!(!has_vector_op(&function), "w{width} {op:?}");
            assert_eq!(
                issues(&program(function.clone()), &fixture.types),
                Vec::<String>::new(),
                "w{width} {op:?}"
            );
            assert!(
                ValidatedMir::new(program(function), MirValidationContext::new(&fixture.types))
                    .is_ok(),
                "w{width} {op:?}"
            );
        }
    }
}

#[test]
fn unexpanded_function_is_still_rejected_as_a_survivor() {
    let fixture = fixture(3);
    let function = function_for(&fixture, &MirVectorOp::Dot);
    assert_eq!(
        issues(&program(function), &fixture.types),
        vec![DEFERRED_VECTOR_OP_SURVIVED.to_owned()]
    );
}

#[test]
fn lane_count_equals_the_width() {
    for width in 2..=4u64 {
        let w = width as usize;
        let fixture = fixture(width);

        for op in BIN_OPS {
            let mut function = function_for(&fixture, &MirVectorOp::Elementwise(op));
            assert!(expand(&fixture, &mut function).is_empty());
            assert_eq!(binaries(&function, op), w, "w{width} {op:?}");
            assert_eq!(constructs(&function), vec![w]);
        }

        let mut dot = function_for(&fixture, &MirVectorOp::Dot);
        assert!(expand(&fixture, &mut dot).is_empty());
        assert_eq!(binaries(&dot, MirBinOp::Mul), w);
        assert_eq!(binaries(&dot, MirBinOp::Add), w - 1);
        assert!(constructs(&dot).is_empty());

        let mut length = function_for(&fixture, &MirVectorOp::Length);
        assert!(expand(&fixture, &mut length).is_empty());
        assert_eq!(binaries(&length, MirBinOp::Mul), w);

        let mut crea = function_for(&fixture, &MirVectorOp::Crea);
        assert!(expand(&fixture, &mut crea).is_empty());
        assert_eq!(constructs(&crea), vec![w]);
        assert_eq!(binaries(&crea, MirBinOp::Mul), 0);

        let mut swizzle = function_for(&fixture, &MirVectorOp::Swizzle((0..width as u8).collect()));
        assert!(expand(&fixture, &mut swizzle).is_empty());
        assert_eq!(constructs(&swizzle), vec![w]);
    }

    let fixture = fixture(3);
    let mut cross = function_for(&fixture, &MirVectorOp::Cross);
    assert!(expand(&fixture, &mut cross).is_empty());
    assert_eq!(binaries(&cross, MirBinOp::Mul), 6);
    assert_eq!(binaries(&cross, MirBinOp::Sub), 3);
    assert_eq!(constructs(&cross), vec![3]);
}

#[test]
fn swizzle_reads_the_named_lanes_in_order() {
    let fixture = fixture(3);
    let mut function = function_for(&fixture, &MirVectorOp::Swizzle(vec![2, 0, 1]));
    assert!(expand(&fixture, &mut function).is_empty());
    let lanes: Vec<u8> = all_statements(&function)
        .find_map(|statement| match &statement.kind {
            MirStatementKind::Construct { aggregate, .. } => Some(aggregate),
            _ => None,
        })
        .map(|aggregate| {
            let MirAggregateFields::Ordered(fields) = &aggregate.fields else {
                panic!("ordered");
            };
            fields
                .iter()
                .map(|field| {
                    let crate::MirAggregateItem::Operand(MirOperand::Place(place)) = field else {
                        panic!("swizzle field must read a source lane");
                    };
                    let [MirProjection::VectorLane(lane)] = place.projections.as_slice() else {
                        panic!("one lane projection");
                    };
                    *lane
                })
                .collect()
        })
        .expect("a vector construct");
    assert_eq!(lanes, vec![2, 0, 1]);
}

#[test]
fn normalized_splits_the_block_and_keeps_the_tail() {
    for width in 2..=4 {
        let fixture = fixture(width);
        let mut function = function_for(&fixture, &MirVectorOp::Normalized);
        assert!(expand(&fixture, &mut function).is_empty());

        assert_eq!(function.blocks.len(), 4, "head, scale, zero, join");
        let ids: Vec<u32> = function.blocks.iter().map(|block| block.id.0).collect();
        assert_eq!(ids, vec![0, 1, 2, 3]);
        let MirTerminatorKind::Branch {
            then_block,
            else_block,
            ..
        } = function.blocks[0].terminator.kind
        else {
            panic!("head must branch");
        };
        assert_eq!((then_block.0, else_block.0), (1, 2));
        for arm in [1, 2] {
            assert!(matches!(
                function.blocks[arm].terminator.kind,
                MirTerminatorKind::Goto(MirBlockId(3))
            ));
        }
        // Join: only the marker that followed the call (the destination is
        // written directly by both arms), then the original return.
        let join = &function.blocks[3];
        assert_eq!(join.statements.len(), 1);
        assert!(matches!(
            join.terminator.kind,
            MirTerminatorKind::Return(Some(_))
        ));
        assert_eq!(constructs(&function), vec![width as usize, width as usize]);
        assert_eq!(binaries(&function, MirBinOp::Div), width as usize);
        assert_eq!(binaries(&function, MirBinOp::Gt), 1);
    }
}

#[test]
fn two_normalized_calls_in_one_block_both_expand() {
    let fixture = fixture(3);
    let mut function = function_for(&fixture, &MirVectorOp::Normalized);
    let second = function.blocks[0].statements[0].clone();
    function.blocks[0].statements.insert(1, second);
    assert!(expand(&fixture, &mut function).is_empty());
    assert!(!has_vector_op(&function));
    assert_eq!(function.blocks.len(), 7);
    assert_eq!(
        issues(&program(function), &fixture.types),
        Vec::<String>::new()
    );
}

#[test]
fn cross_outside_width_three_is_refused_and_still_a_survivor() {
    for width in [2, 4] {
        let fixture = fixture(width);
        let mut function = function_for(&fixture, &MirVectorOp::Cross);
        assert_eq!(expand(&fixture, &mut function), vec![VECTOR_CROSS_WIDTH]);
        assert!(has_vector_op(&function));
        assert!(
            issues(&program(function), &fixture.types)
                .contains(&DEFERRED_VECTOR_OP_SURVIVED.to_owned())
        );
    }
}

#[test]
fn swizzle_pattern_must_match_width_and_lanes() {
    let fixture = fixture(3);
    let mut short = function_for(&fixture, &MirVectorOp::Swizzle(vec![0, 1]));
    assert_eq!(expand(&fixture, &mut short), vec![VECTOR_SWIZZLE_WIDTH]);
    let mut out_of_range = function_for(&fixture, &MirVectorOp::Swizzle(vec![0, 1, 3]));
    assert_eq!(
        expand(&fixture, &mut out_of_range),
        vec![VECTOR_SWIZZLE_LANE]
    );
}

#[test]
fn width_outside_one_to_four_is_refused() {
    for width in [0, 5, 8] {
        let fixture = fixture(width);
        let mut function = function_for(&fixture, &MirVectorOp::Dot);
        assert_eq!(
            expand(&fixture, &mut function),
            vec![VECTOR_OP_WIDTH_UNSUPPORTED],
            "w{width}"
        );
        assert!(has_vector_op(&function));
    }
}

#[test]
fn symbolic_width_is_refused() {
    let mut fixture = fixture(3);
    let scalar = fixture.scalar.semantic_id();
    let param_index = fixture
        .types
        .intern_index(IndexExpr::Param(radix_lexer::Symbol(1)));
    let symbolic = MirType::semantic(fixture.types.vector_with_width(scalar, param_index));
    let mut function = function_for(&fixture, &MirVectorOp::Dot);
    function.params[0].ty = symbolic;
    assert_eq!(
        expand(&fixture, &mut function),
        vec![VECTOR_OP_WIDTH_UNSUPPORTED]
    );
}

#[test]
fn operand_count_must_match_arity() {
    let fixture = fixture(3);
    let mut function = function_with(&fixture, MirVectorOp::Dot, a(), fixture.scalar);
    assert_eq!(expand(&fixture, &mut function), vec![VECTOR_OP_ARITY]);
}

#[test]
fn projected_and_constant_operands_are_unresolved() {
    let fixture = fixture(3);
    let mut projected = place(0);
    projected.projections.push(MirProjection::VectorLane(0));
    let mut function = function_with(
        &fixture,
        MirVectorOp::Length,
        vec![MirOperand::Place(projected)],
        fixture.scalar,
    );
    assert_eq!(
        expand(&fixture, &mut function),
        vec![VECTOR_OP_OPERAND_UNRESOLVED]
    );
    let mut constant = function_with(
        &fixture,
        MirVectorOp::Length,
        vec![MirOperand::Constant(MirConstant::Float(1.0))],
        fixture.scalar,
    );
    assert_eq!(
        expand(&fixture, &mut constant),
        vec![VECTOR_OP_OPERAND_UNRESOLVED]
    );
}

#[test]
fn destination_aliasing_an_operand_is_written_after_every_lane_read() {
    let fixture = fixture(3);
    let mut function = function_with(
        &fixture,
        MirVectorOp::Swizzle(vec![2, 1, 0]),
        vec![MirOperand::Place(place(2))],
        fixture.vector,
    );
    function.locals[0].ty = fixture.vector;
    assert!(expand(&fixture, &mut function).is_empty());
    // The construct targets a fresh temp; only the final assign names `out`.
    let writes_out: Vec<bool> = all_statements(&function)
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::Construct { destination, .. } => Some(*destination == place(2)),
            _ => None,
        })
        .collect();
    assert_eq!(writes_out, vec![false]);
}

#[test]
fn a_function_without_vector_ops_is_untouched() {
    let fixture = fixture(3);
    let mut function = function_for(&fixture, &MirVectorOp::Crea);
    function.blocks[0].statements.remove(0);
    let before = function.clone();
    assert!(expand(&fixture, &mut function).is_empty());
    assert_eq!(function, before);
}

#[test]
fn aliased_normalized_destination_is_copied_in_the_join() {
    let fixture = fixture(3);
    let mut function = function_with(
        &fixture,
        MirVectorOp::Normalized,
        vec![MirOperand::Place(place(2))],
        fixture.vector,
    );
    assert!(expand(&fixture, &mut function).is_empty());
    assert_eq!(function.blocks.len(), 4);
    // Copy into `out` plus the marker; both arms write a fresh temp.
    assert_eq!(function.blocks[3].statements.len(), 2);
    for arm in [1, 2] {
        let writes_out = function.blocks[arm].statements.iter().any(|statement| {
            matches!(
                &statement.kind,
                MirStatementKind::Assign { place: written, .. } if *written == place(2)
            )
        });
        assert!(
            !writes_out,
            "arm {arm} must not write the aliased destination"
        );
    }
    assert_eq!(
        issues(&program(function), &fixture.types),
        Vec::<String>::new()
    );
}

#[test]
fn direct_destination_is_written_by_the_final_lane_statement() {
    let fixture = fixture(3);
    let mut dot = function_for(&fixture, &MirVectorOp::Dot);
    assert!(expand(&fixture, &mut dot).is_empty());
    // Three products and one partial sum in temps; the last add writes the
    // destination itself, with no result temp and no trailing copy.
    assert_eq!(dot.temps.len(), 4);
    let last_lane = &dot.blocks[0].statements[4];
    assert!(matches!(
        &last_lane.kind,
        MirStatementKind::Assign { place: written, .. } if *written == place(2)
    ));

    let mut vector = function_for(&fixture, &MirVectorOp::Elementwise(MirBinOp::Add));
    assert!(expand(&fixture, &mut vector).is_empty());
    assert_eq!(vector.temps.len(), 3);
    assert!(matches!(
        &vector.blocks[0].statements[3].kind,
        MirStatementKind::Construct { destination, .. } if *destination == place(2)
    ));
}
