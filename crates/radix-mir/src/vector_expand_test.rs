//! `expand_vector_ops`: lane counts, block splitting, refusals, and the
//! validation hand-off. Tests assert structure, issue codes and counts, never
//! diagnostic prose.
// Fixture casts are small known-in-range numbers.
#![allow(clippy::cast_possible_truncation)]

use crate::vector_expand::{
    VECTOR_CROSS_WIDTH, VECTOR_OP_ARITY, VECTOR_OP_OPERAND_UNRESOLVED, VECTOR_OP_WIDTH_UNSUPPORTED,
    VECTOR_SWIZZLE_LANE, VECTOR_SWIZZLE_WIDTH, expand_vector_ops,
};
use crate::{
    DEFERRED_VECTOR_OP_SURVIVED, MirAggregateFields, MirAggregateKind, MirBinOp, MirBlock,
    MirBlockId, MirConstant, MirFunction, MirFunctionId, MirIntrinsic, MirLaneOp, MirLocal,
    MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirProgram, MirProjection,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirType,
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

/// The lane operator that applies the plain binary operator `op`.
fn lane_op(op: MirBinOp) -> MirLaneOp {
    match op {
        MirBinOp::Add => MirLaneOp::Add,
        MirBinOp::Sub => MirLaneOp::Sub,
        MirBinOp::Mul => MirLaneOp::Mul,
        MirBinOp::Div => MirLaneOp::Div,
        MirBinOp::Mod => MirLaneOp::Mod,
        other => panic!("{other:?} is not a lane operator"),
    }
}

fn elementwise_op(op: MirBinOp) -> MirVectorOp {
    MirVectorOp::Elementwise {
        op: lane_op(op),
        checked: false,
    }
}

fn ops_for(width: u64) -> Vec<MirVectorOp> {
    let mut ops: Vec<MirVectorOp> = BIN_OPS.into_iter().map(elementwise_op).collect();
    ops.extend([
        MirVectorOp::Dot,
        MirVectorOp::Norm,
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
            let mut function = function_for(&fixture, &elementwise_op(op));
            assert!(expand(&fixture, &mut function).is_empty());
            assert_eq!(binaries(&function, op), w, "w{width} {op:?}");
            assert_eq!(constructs(&function), vec![w]);
        }

        let mut dot = function_for(&fixture, &MirVectorOp::Dot);
        assert!(expand(&fixture, &mut dot).is_empty());
        assert_eq!(binaries(&dot, MirBinOp::Mul), w);
        assert_eq!(binaries(&dot, MirBinOp::Add), w - 1);
        assert!(constructs(&dot).is_empty());

        let mut length = function_for(&fixture, &MirVectorOp::Norm);
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
        MirVectorOp::Norm,
        vec![MirOperand::Place(projected)],
        fixture.scalar,
    );
    assert_eq!(
        expand(&fixture, &mut function),
        vec![VECTOR_OP_OPERAND_UNRESOLVED]
    );
    let mut constant = function_with(
        &fixture,
        MirVectorOp::Norm,
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

    let mut vector = function_for(&fixture, &elementwise_op(MirBinOp::Add));
    assert!(expand(&fixture, &mut vector).is_empty());
    assert_eq!(vector.temps.len(), 3);
    assert!(matches!(
        &vector.blocks[0].statements[3].kind,
        MirStatementKind::Construct { destination, .. } if *destination == place(2)
    ));
}

/// Operands `[a, marker]`: the vector param and the scalar local `l3`.
fn vector_and_scalar() -> Vec<MirOperand> {
    vec![MirOperand::Place(place(0)), MirOperand::Place(place(3))]
}

fn scalar_function(fixture: &Fixture, op: MirVectorOp) -> MirFunction {
    function_with(fixture, op, vector_and_scalar(), fixture.vector)
}

fn unary_negations(function: &MirFunction) -> usize {
    all_statements(function)
        .filter(|statement| {
            matches!(
                &statement.kind,
                MirStatementKind::Assign { value, .. }
                    if matches!(&value.kind, MirValueKind::Unary { op: crate::MirUnOp::Neg, .. })
            )
        })
        .count()
}

/// The `(lhs, rhs)` operands of every `wanted` lane binary, in order.
fn binary_operands(function: &MirFunction, wanted: MirBinOp) -> Vec<(MirOperand, MirOperand)> {
    all_statements(function)
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::Assign { value, .. } => match &value.kind {
                MirValueKind::Binary { op, lhs, rhs } if *op == wanted => {
                    Some((lhs.clone(), rhs.clone()))
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn is_lane_read(operand: &MirOperand) -> bool {
    matches!(
        operand,
        MirOperand::Place(place)
            if matches!(place.projections.as_slice(), [MirProjection::VectorLane(_)])
    )
}

#[test]
fn a_scalar_peer_unrolls_one_binary_per_lane_in_written_order() {
    for width in 2..=4u64 {
        let fixture = fixture(width);
        for op in BIN_OPS {
            for scalar_first in [false, true] {
                let mut function = scalar_function(
                    &fixture,
                    MirVectorOp::ElementwiseScalar {
                        op: lane_op(op),
                        scalar_first,
                        checked: false,
                    },
                );
                let errors = expand(&fixture, &mut function);
                assert!(
                    errors.is_empty(),
                    "w{width} {op:?} {scalar_first}: {errors:?}"
                );
                assert!(!has_vector_op(&function));
                assert_eq!(constructs(&function), vec![width as usize]);
                let operands = binary_operands(&function, op);
                assert_eq!(operands.len(), width as usize, "w{width} {op:?}");
                for (lhs, rhs) in operands {
                    let (scalar, lane) = if scalar_first { (lhs, rhs) } else { (rhs, lhs) };
                    assert!(is_lane_read(&lane), "w{width} {op:?} {scalar_first}");
                    assert_eq!(
                        scalar,
                        MirOperand::Place(place(3)),
                        "the scalar is read as is"
                    );
                }
                assert!(
                    ValidatedMir::new(program(function), MirValidationContext::new(&fixture.types))
                        .is_ok(),
                    "w{width} {op:?} {scalar_first}"
                );
            }
        }
    }
}

#[test]
fn negate_unrolls_one_negation_per_lane() {
    for width in 2..=4u64 {
        let fixture = fixture(width);
        let mut function = function_with(
            &fixture,
            MirVectorOp::Negate { checked: false },
            a(),
            fixture.vector,
        );
        assert!(expand(&fixture, &mut function).is_empty());
        assert_eq!(unary_negations(&function), width as usize);
        assert_eq!(store_checks(&function), 0);
        assert_eq!(constructs(&function), vec![width as usize]);
        assert!(
            ValidatedMir::new(program(function), MirValidationContext::new(&fixture.types)).is_ok()
        );
    }
}

#[test]
fn the_scalar_forms_check_their_arity() {
    let fixture = fixture(3);
    let mut one = function_with(
        &fixture,
        MirVectorOp::ElementwiseScalar {
            op: MirLaneOp::Mul,
            scalar_first: false,
            checked: false,
        },
        a(),
        fixture.vector,
    );
    assert_eq!(expand(&fixture, &mut one), vec![VECTOR_OP_ARITY.to_owned()]);
    let mut two = function_with(
        &fixture,
        MirVectorOp::Negate { checked: false },
        ab(),
        fixture.vector,
    );
    assert_eq!(expand(&fixture, &mut two), vec![VECTOR_OP_ARITY.to_owned()]);
}

#[test]
fn a_destination_aliasing_the_vector_is_written_after_every_lane_read_of_a_scalar_op() {
    let fixture = fixture(3);
    let args = vec![MirOperand::Place(place(2)), MirOperand::Place(place(3))];
    let mut function = function_with(
        &fixture,
        MirVectorOp::ElementwiseScalar {
            op: MirLaneOp::Mul,
            scalar_first: false,
            checked: false,
        },
        args,
        fixture.vector,
    );
    assert!(expand(&fixture, &mut function).is_empty());
    let statements: Vec<_> = all_statements(&function).collect();
    let write = statements
        .iter()
        .rposition(|statement| match &statement.kind {
            MirStatementKind::Assign { place: target, .. } => *target == place(2),
            _ => false,
        })
        .expect("the destination is written");
    let last_lane_read = statements
        .iter()
        .rposition(|statement| match &statement.kind {
            MirStatementKind::Assign { value, .. } => matches!(
                &value.kind,
                MirValueKind::Binary { lhs, .. } if is_lane_read(lhs)
            ),
            _ => false,
        })
        .expect("lanes are read");
    assert!(
        write > last_lane_read,
        "the result lands after every lane read"
    );
}

/// How many store-checked `Convert` calls the function holds.
fn store_checks(function: &MirFunction) -> usize {
    all_statements(function)
        .filter(|statement| {
            matches!(
                &statement.kind,
                MirStatementKind::RuntimeCall { call, .. }
                    if matches!(
                        &call.intrinsic,
                        MirIntrinsic::Convert(conversion) if conversion.store.is_some()
                    )
            )
        })
        .count()
}

#[test]
fn a_checked_scalar_op_stores_every_lane_under_the_element_type() {
    for width in 2..=4u64 {
        let fixture = fixture(width);
        for scalar_first in [false, true] {
            let mut function = scalar_function(
                &fixture,
                MirVectorOp::ElementwiseScalar {
                    op: MirLaneOp::Mul,
                    scalar_first,
                    checked: true,
                },
            );
            assert!(expand(&fixture, &mut function).is_empty());
            assert_eq!(store_checks(&function), width as usize, "w{width}");
            assert_eq!(binaries(&function, MirBinOp::Mul), width as usize);
            assert_eq!(constructs(&function), vec![width as usize]);
            assert!(
                ValidatedMir::new(program(function), MirValidationContext::new(&fixture.types))
                    .is_ok(),
                "w{width}"
            );
        }
    }
}

#[test]
fn a_checked_negation_stores_every_lane_under_the_element_type() {
    for width in 2..=4u64 {
        let fixture = fixture(width);
        let mut function = function_with(
            &fixture,
            MirVectorOp::Negate { checked: true },
            a(),
            fixture.vector,
        );
        assert!(expand(&fixture, &mut function).is_empty());
        assert_eq!(store_checks(&function), width as usize, "w{width}");
        assert_eq!(unary_negations(&function), width as usize);
        assert!(
            ValidatedMir::new(program(function), MirValidationContext::new(&fixture.types)).is_ok(),
            "w{width}"
        );
    }
}

/// A fixture plus the bool vector type of the same width.
fn mask_fixture(width: u64) -> (Fixture, MirType) {
    let mut fixture = fixture(width);
    let flag = fixture.types.primitive(Primitive::Bivalens);
    let index = fixture.types.intern_index(IndexExpr::Literal(width));
    let mask = fixture.types.vector_with_width(flag, index);
    (fixture, MirType::semantic(mask))
}

/// `function_with` whose first parameter is the bool mask `l0`.
fn mask_function(
    fixture: &Fixture,
    mask: MirType,
    op: MirVectorOp,
    args: Vec<MirOperand>,
    result: MirType,
) -> MirFunction {
    let mut function = function_with(fixture, op, args, result);
    function.params[0] = param(0, mask);
    function
}

fn selects(function: &MirFunction) -> usize {
    function
        .blocks
        .iter()
        .filter(|block| matches!(block.terminator.kind, MirTerminatorKind::Branch { .. }))
        .count()
}

fn unary_nots(function: &MirFunction) -> usize {
    all_statements(function)
        .filter(|statement| {
            matches!(
                &statement.kind,
                MirStatementKind::Assign { value, .. }
                    if matches!(&value.kind, MirValueKind::Unary { op: crate::MirUnOp::Not, .. })
            )
        })
        .count()
}

#[test]
fn comparison_and_logic_lanes_are_plain_binaries_into_a_bool_vector() {
    for width in 2..=4u64 {
        let (fixture, mask) = mask_fixture(width);
        for (lane, op) in [
            (MirLaneOp::Lt, MirBinOp::Lt),
            (MirLaneOp::Gt, MirBinOp::Gt),
            (MirLaneOp::LtEq, MirBinOp::LtEq),
            (MirLaneOp::GtEq, MirBinOp::GtEq),
            (MirLaneOp::Eq, MirBinOp::Eq),
            (MirLaneOp::NotEq, MirBinOp::NotEq),
            (MirLaneOp::And, MirBinOp::And),
            (MirLaneOp::Or, MirBinOp::Or),
        ] {
            let mut pair = function_with(
                &fixture,
                MirVectorOp::Elementwise {
                    op: lane,
                    checked: false,
                },
                ab(),
                mask,
            );
            assert!(expand(&fixture, &mut pair).is_empty(), "w{width} {op:?}");
            assert_eq!(binaries(&pair, op), width as usize, "w{width} {op:?}");
            assert_eq!(constructs(&pair), vec![width as usize]);
            assert_eq!(store_checks(&pair), 0);
            // The fixture's operands are float vectors, which `et`/`aut` lanes
            // would not validate against; the comparison lanes do.
            if !matches!(lane, MirLaneOp::And | MirLaneOp::Or) {
                assert!(
                    ValidatedMir::new(program(pair), MirValidationContext::new(&fixture.types))
                        .is_ok(),
                    "w{width} {op:?}"
                );
            }
            for scalar_first in [false, true] {
                let mut scalar = function_with(
                    &fixture,
                    MirVectorOp::ElementwiseScalar {
                        op: lane,
                        scalar_first,
                        checked: false,
                    },
                    vector_and_scalar(),
                    mask,
                );
                assert!(expand(&fixture, &mut scalar).is_empty());
                assert_eq!(binaries(&scalar, op), width as usize, "w{width} {op:?}");
                for (lhs, rhs) in binary_operands(&scalar, op) {
                    let (peer, lane_read) = if scalar_first { (lhs, rhs) } else { (rhs, lhs) };
                    assert!(is_lane_read(&lane_read));
                    assert_eq!(peer, MirOperand::Place(place(3)));
                }
            }
        }
    }
}

#[test]
fn not_unrolls_one_boolean_negation_per_lane() {
    for width in 2..=4u64 {
        let (fixture, mask) = mask_fixture(width);
        let mut function = mask_function(&fixture, mask, MirVectorOp::Not, a(), mask);
        assert!(expand(&fixture, &mut function).is_empty());
        assert!(!has_vector_op(&function));
        assert_eq!(unary_nots(&function), width as usize);
        assert_eq!(unary_negations(&function), 0);
        assert_eq!(constructs(&function), vec![width as usize]);
        assert!(
            ValidatedMir::new(program(function), MirValidationContext::new(&fixture.types)).is_ok(),
            "w{width}"
        );
    }
}

#[test]
fn select_branches_once_per_lane_and_keeps_the_tail() {
    for width in 2..=4u64 {
        let (fixture, mask) = mask_fixture(width);
        let args = vec![
            MirOperand::Place(place(0)),
            MirOperand::Place(place(1)),
            MirOperand::Place(place(1)),
        ];
        let mut function = mask_function(&fixture, mask, MirVectorOp::Select, args, fixture.vector);
        assert!(expand(&fixture, &mut function).is_empty(), "w{width}");
        assert!(!has_vector_op(&function));
        // The head plus a then, an else and a join block per lane.
        assert_eq!(function.blocks.len(), 1 + 3 * width as usize, "w{width}");
        assert_eq!(selects(&function), width as usize);
        let last = function.blocks.last().expect("a join block");
        assert!(matches!(
            last.terminator.kind,
            MirTerminatorKind::Return(Some(_))
        ));
        // The result vector is built and the marker that followed the call
        // survives, both in the last join.
        assert_eq!(constructs(&function), vec![width as usize]);
        assert!(matches!(
            last.statements.last().map(|statement| &statement.kind),
            Some(MirStatementKind::Assign { place: target, .. }) if *target == place(3)
        ));
        assert!(
            ValidatedMir::new(program(function), MirValidationContext::new(&fixture.types)).is_ok(),
            "w{width}"
        );
    }
}

#[test]
fn select_lets_a_scalar_branch_feed_every_lane() {
    let (fixture, mask) = mask_fixture(3);
    let args = vec![
        MirOperand::Place(place(0)),
        MirOperand::Place(place(3)),
        MirOperand::Place(place(1)),
    ];
    let mut function = mask_function(&fixture, mask, MirVectorOp::Select, args, fixture.vector);
    assert!(expand(&fixture, &mut function).is_empty());
    let then_reads = function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter(|statement| {
            matches!(
                &statement.kind,
                MirStatementKind::Assign { value, .. }
                    if matches!(&value.kind, MirValueKind::Operand(operand) if *operand == MirOperand::Place(place(3)))
            )
        })
        .count();
    assert_eq!(then_reads, 3, "the scalar is read whole in each then block");
    assert!(
        ValidatedMir::new(program(function), MirValidationContext::new(&fixture.types)).is_ok()
    );
}

#[test]
fn select_checks_its_arity() {
    let (fixture, mask) = mask_fixture(3);
    let mut function = mask_function(&fixture, mask, MirVectorOp::Select, ab(), fixture.vector);
    assert_eq!(
        expand(&fixture, &mut function),
        vec![VECTOR_OP_ARITY.to_owned()]
    );
}

#[test]
fn an_unchecked_scalar_op_carries_no_store_node() {
    let fixture = fixture(3);
    let mut function = scalar_function(
        &fixture,
        MirVectorOp::ElementwiseScalar {
            op: MirLaneOp::Add,
            scalar_first: false,
            checked: false,
        },
    );
    assert!(expand(&fixture, &mut function).is_empty());
    assert_eq!(store_checks(&function), 0);
}
