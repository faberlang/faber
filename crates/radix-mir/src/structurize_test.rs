//! Region-tree shape and invariants of the target-neutral structurizer.

use crate::structurize::{Region, StructurizeError, structurize};
use crate::{
    MirBlock, MirBlockId, MirConstant, MirFunction, MirFunctionId, MirOperand, MirSwitchCase,
    MirTempId, MirTerminator, MirTerminatorKind, MirType,
};
use radix_lexer::Span;
use radix_types::TypeId;

fn bb(id: u32) -> MirBlockId {
    MirBlockId(id)
}

fn cond(n: u32) -> MirOperand {
    MirOperand::Temp(MirTempId(n))
}

fn ret(n: Option<u32>) -> MirTerminatorKind {
    MirTerminatorKind::Return(n.map(|n| MirOperand::Constant(MirConstant::Int(i64::from(n)))))
}

fn goto(to: u32) -> MirTerminatorKind {
    MirTerminatorKind::Goto(bb(to))
}

fn branch(c: u32, then_block: u32, else_block: u32) -> MirTerminatorKind {
    MirTerminatorKind::Branch {
        condition: cond(c),
        then_block: bb(then_block),
        else_block: bb(else_block),
    }
}

fn function(terminators: Vec<MirTerminatorKind>) -> MirFunction {
    let blocks = terminators
        .into_iter()
        .enumerate()
        .map(|(index, kind)| MirBlock {
            id: MirBlockId(u32::try_from(index).expect("small")),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind,
                span: Span::default(),
            },
            span: Span::default(),
        })
        .collect();
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks,
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn block(id: u32) -> Region {
    Region::Block(bb(id))
}

fn returns(n: Option<u32>) -> Region {
    Region::Return(n.map(|n| MirOperand::Constant(MirConstant::Int(i64::from(n)))))
}

fn seq(parts: Vec<Region>) -> Region {
    Region::Seq(parts)
}

fn if_(c: u32, then_region: Region, else_region: Region) -> Region {
    Region::If {
        cond: cond(c),
        then_region: Box::new(then_region),
        else_region: Box::new(else_region),
    }
}

fn collect_blocks(region: &Region, out: &mut Vec<MirBlockId>) {
    match region {
        Region::Seq(parts) => parts.iter().for_each(|part| collect_blocks(part, out)),
        Region::Block(id) => out.push(*id),
        Region::If {
            then_region,
            else_region,
            ..
        } => {
            collect_blocks(then_region, out);
            collect_blocks(else_region, out);
        }
        Region::Return(_) => {}
    }
}

/// Structurize and check that every reachable block appears exactly once.
fn tree(function: &MirFunction, reachable: &[u32]) -> Region {
    let region = structurize(function).expect("structurizes");
    let mut seen = Vec::new();
    collect_blocks(&region, &mut seen);
    seen.sort();
    let expected: Vec<MirBlockId> = reachable.iter().copied().map(bb).collect();
    assert_eq!(seen, expected, "each reachable block exactly once");
    region
}

#[test]
fn straight_line_function() {
    let f = function(vec![goto(1), goto(2), ret(Some(7))]);
    assert_eq!(
        tree(&f, &[0, 1, 2]),
        seq(vec![block(0), block(1), block(2), returns(Some(7))])
    );
}

#[test]
fn lane_form_copy_guard_with_early_return() {
    // bb0: guard, branch bb1 bb2; bb1: return; bb2: body, goto bb3; bb3: return.
    let f = function(vec![branch(3, 1, 2), ret(None), goto(3), ret(None)]);
    assert_eq!(
        tree(&f, &[0, 1, 2, 3]),
        seq(vec![
            block(0),
            if_(
                3,
                seq(vec![block(1), returns(None)]),
                seq(vec![block(2), block(3), returns(None)]),
            ),
        ])
    );
}

#[test]
fn if_else_diamond_with_join() {
    // bb0 -> bb1 | bb2; both -> bb3; bb3 returns.
    let f = function(vec![branch(1, 1, 2), goto(3), goto(3), ret(Some(1))]);
    assert_eq!(
        tree(&f, &[0, 1, 2, 3]),
        seq(vec![
            block(0),
            if_(1, seq(vec![block(1)]), seq(vec![block(2)])),
            block(3),
            returns(Some(1)),
        ])
    );
}

#[test]
fn if_without_else_arm_goes_straight_to_join() {
    // bb0 -> bb1 | bb2; bb1 -> bb2; bb2 returns.
    let f = function(vec![branch(1, 1, 2), goto(2), ret(None)]);
    assert_eq!(
        tree(&f, &[0, 1, 2]),
        seq(vec![
            block(0),
            if_(1, seq(vec![block(1)]), seq(vec![])),
            block(2),
            returns(None),
        ])
    );
}

#[test]
fn nested_ifs() {
    // bb0 -> bb1 | bb4; bb1 -> bb2 | bb3; bb2,bb3 -> bb5; bb4 -> bb5; bb5 returns.
    let f = function(vec![
        branch(1, 1, 4),
        branch(2, 2, 3),
        goto(5),
        goto(5),
        goto(5),
        ret(None),
    ]);
    assert_eq!(
        tree(&f, &[0, 1, 2, 3, 4, 5]),
        seq(vec![
            block(0),
            if_(
                1,
                seq(vec![
                    block(1),
                    if_(2, seq(vec![block(2)]), seq(vec![block(3)])),
                ]),
                seq(vec![block(4)]),
            ),
            block(5),
            returns(None),
        ])
    );
}

#[test]
fn if_with_return_in_both_arms_has_no_join() {
    let f = function(vec![branch(1, 1, 2), ret(Some(1)), ret(Some(2))]);
    assert_eq!(
        tree(&f, &[0, 1, 2]),
        seq(vec![
            block(0),
            if_(
                1,
                seq(vec![block(1), returns(Some(1))]),
                seq(vec![block(2), returns(Some(2))]),
            ),
        ])
    );
}

#[test]
fn early_return_inside_an_arm_still_joins_the_other_paths() {
    // bb0 -> bb1 | bb2; bb1 -> bb3 | bb4(return); bb3 -> bb5; bb2 -> bb5; bb5 returns.
    let f = function(vec![
        branch(1, 1, 2),
        branch(2, 3, 4),
        goto(5),
        goto(5),
        ret(Some(9)),
        ret(None),
    ]);
    assert_eq!(
        tree(&f, &[0, 1, 2, 3, 4, 5]),
        seq(vec![
            block(0),
            if_(
                1,
                seq(vec![
                    block(1),
                    if_(
                        2,
                        seq(vec![block(3)]),
                        seq(vec![block(4), returns(Some(9))]),
                    ),
                ]),
                seq(vec![block(2)]),
            ),
            block(5),
            returns(None),
        ])
    );
}

#[test]
fn unreachable_blocks_are_ignored() {
    let f = function(vec![ret(None), goto(0)]);
    assert_eq!(tree(&f, &[0]), seq(vec![block(0), returns(None)]));
}

#[test]
fn loop_returns_named_error() {
    // bb0 -> bb1; bb1 -> bb1 (back edge) | bb2.
    let f = function(vec![goto(1), branch(1, 1, 2), ret(None)]);
    assert_eq!(
        structurize(&f),
        Err(StructurizeError::Loop {
            from: bb(1),
            header: bb(1)
        })
    );
}

#[test]
fn switch_fails_closed_by_name() {
    let f = function(vec![
        MirTerminatorKind::Switch {
            value: cond(0),
            cases: vec![MirSwitchCase {
                value: MirConstant::Int(1),
                target: bb(1),
            }],
            default: bb(1),
        },
        ret(None),
    ]);
    assert_eq!(
        structurize(&f),
        Err(StructurizeError::UnsupportedTerminator {
            block: bb(0),
            kind: "switch"
        })
    );
}

#[test]
fn shared_tail_without_common_join_fails_closed() {
    // Cross edges: bb1 -> bb3 | bb5, bb2 -> bb3 | bb4, bb5 -> bb4. Both arms reach bb3 and
    // bb4 but neither can be a join without emitting the other twice.
    let f = function(vec![
        branch(1, 1, 2),
        branch(2, 3, 5),
        branch(3, 3, 4),
        ret(None),
        ret(None),
        goto(4),
    ]);
    assert!(matches!(
        structurize(&f),
        Err(StructurizeError::SharedTail { .. })
    ));
}

#[test]
fn empty_function_fails_by_name() {
    let f = function(Vec::new());
    assert_eq!(structurize(&f), Err(StructurizeError::EmptyFunction));
}
