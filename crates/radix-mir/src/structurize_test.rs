//! Region-tree shape and invariants of the target-neutral structurizer.

use crate::structurize::{Region, StructurizeError, structurize, structurize_duplicating_tails};
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

fn loop_(body: Region) -> Region {
    Region::Loop {
        body: Box::new(body),
    }
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
        Region::Loop { body } => collect_blocks(body, out),
        Region::Return(_) | Region::Break | Region::Continue => {}
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
fn while_loop_becomes_loop_with_continue_and_break() {
    // bb0 -> bb1(header); bb1 -> bb2 | bb4; bb2 -> bb3; bb3 -> bb1 (latch); bb4 returns.
    let f = function(vec![goto(1), branch(1, 2, 4), goto(3), goto(1), ret(None)]);
    assert_eq!(
        tree(&f, &[0, 1, 2, 3, 4]),
        seq(vec![
            block(0),
            loop_(seq(vec![
                block(1),
                if_(
                    1,
                    seq(vec![block(2), block(3), Region::Continue]),
                    seq(vec![Region::Break]),
                ),
            ])),
            block(4),
            returns(None),
        ])
    );
}

#[test]
fn break_on_a_data_dependent_condition() {
    // bb1 header -> bb2 | bb5(exit); bb2 -> bb5 (break) | bb3; bb3 -> bb4; bb4 -> bb1.
    let f = function(vec![
        goto(1),
        branch(1, 2, 5),
        branch(2, 5, 3),
        goto(4),
        goto(1),
        ret(None),
    ]);
    assert_eq!(
        tree(&f, &[0, 1, 2, 3, 4, 5]),
        seq(vec![
            block(0),
            loop_(seq(vec![
                block(1),
                if_(
                    1,
                    seq(vec![
                        block(2),
                        if_(
                            2,
                            seq(vec![Region::Break]),
                            seq(vec![block(3), block(4), Region::Continue]),
                        ),
                    ]),
                    seq(vec![Region::Break]),
                ),
            ])),
            block(5),
            returns(None),
        ])
    );
}

#[test]
fn continue_straight_to_the_header() {
    // bb1 header -> bb2 | bb4; bb2 -> bb1 (continue) | bb3; bb3 -> bb1; bb4 returns.
    let f = function(vec![
        goto(1),
        branch(1, 2, 4),
        branch(2, 1, 3),
        goto(1),
        ret(None),
    ]);
    assert_eq!(
        tree(&f, &[0, 1, 2, 3, 4]),
        seq(vec![
            block(0),
            loop_(seq(vec![
                block(1),
                if_(
                    1,
                    seq(vec![
                        block(2),
                        if_(
                            2,
                            seq(vec![Region::Continue]),
                            seq(vec![block(3), Region::Continue]),
                        ),
                    ]),
                    seq(vec![Region::Break]),
                ),
            ])),
            block(4),
            returns(None),
        ])
    );
}

#[test]
fn continue_through_a_shared_step_block_joins_there() {
    // bb2 -> bb4(step) (continue) | bb3; bb3 -> bb4; bb4 -> bb1.
    let f = function(vec![
        goto(1),
        branch(1, 2, 5),
        branch(2, 4, 3),
        goto(4),
        goto(1),
        ret(None),
    ]);
    assert_eq!(
        tree(&f, &[0, 1, 2, 3, 4, 5]),
        seq(vec![
            block(0),
            loop_(seq(vec![
                block(1),
                if_(
                    1,
                    seq(vec![
                        block(2),
                        if_(2, seq(vec![]), seq(vec![block(3)])),
                        block(4),
                        Region::Continue,
                    ]),
                    seq(vec![Region::Break]),
                ),
            ])),
            block(5),
            returns(None),
        ])
    );
}

#[test]
fn nested_loops() {
    // outer header bb1 -> bb2 | bb7; bb2 -> bb3(inner header); bb3 -> bb4 | bb5;
    // bb4 -> bb3; bb5 (inner exit, outer step) -> bb1; bb7 returns.
    let f = function(vec![
        goto(1),
        branch(1, 2, 7),
        goto(3),
        branch(2, 4, 5),
        goto(3),
        goto(1),
        ret(None),
        ret(None),
    ]);
    assert_eq!(
        tree(&f, &[0, 1, 2, 3, 4, 5, 7]),
        seq(vec![
            block(0),
            loop_(seq(vec![
                block(1),
                if_(
                    1,
                    seq(vec![
                        block(2),
                        loop_(seq(vec![
                            block(3),
                            if_(
                                2,
                                seq(vec![block(4), Region::Continue]),
                                seq(vec![Region::Break]),
                            ),
                        ])),
                        block(5),
                        Region::Continue,
                    ]),
                    seq(vec![Region::Break]),
                ),
            ])),
            block(7),
            returns(None),
        ])
    );
}

#[test]
fn early_return_inside_a_loop_body() {
    // bb1 header -> bb2 | bb5; bb2 -> bb3 (return) | bb4; bb4 -> bb1; bb5 returns.
    let f = function(vec![
        goto(1),
        branch(1, 2, 5),
        branch(2, 3, 4),
        ret(None),
        goto(1),
        ret(None),
    ]);
    assert_eq!(
        tree(&f, &[0, 1, 2, 3, 4, 5]),
        seq(vec![
            block(0),
            loop_(seq(vec![
                block(1),
                if_(
                    1,
                    seq(vec![
                        block(2),
                        if_(
                            2,
                            seq(vec![block(3), returns(None)]),
                            seq(vec![block(4), Region::Continue]),
                        ),
                    ]),
                    seq(vec![Region::Break]),
                ),
            ])),
            block(5),
            returns(None),
        ])
    );
}

#[test]
fn loop_with_a_second_entry_is_irreducible_by_name() {
    // bb0 -> bb1 | bb2; bb1 -> bb2; bb2 -> bb1 (back edge) | bb3. The loop {bb1, bb2}
    // is entered at bb1 and at bb2.
    let f = function(vec![branch(1, 1, 2), goto(2), branch(2, 1, 3), ret(None)]);
    assert!(matches!(
        structurize(&f),
        Err(StructurizeError::IrreducibleLoop { header, .. }) if header == bb(1)
    ));
}

#[test]
fn continue_of_an_outer_loop_from_an_inner_loop_fails_closed_by_name() {
    // inner body bb4 jumps straight to the outer header bb1.
    let f = function(vec![
        goto(1),
        branch(1, 2, 6),
        goto(3),
        branch(2, 4, 5),
        branch(3, 1, 3),
        goto(1),
        ret(None),
    ]);
    assert_eq!(
        structurize(&f),
        Err(StructurizeError::MultiLevelExit { target: bb(1) })
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
fn duplicating_tails_places_the_shared_tail_of_cross_edges_in_each_arm() {
    // The same cross edges as above: with tail duplication each arm emits its
    // own copy of the blocks both arms reach, instead of failing closed.
    let f = function(vec![
        branch(1, 1, 2),
        branch(2, 3, 5),
        branch(3, 3, 4),
        ret(None),
        ret(None),
        goto(4),
    ]);
    let region = structurize_duplicating_tails(&f).expect("tails duplicate");
    let mut seen = Vec::new();
    fn blocks(region: &Region, seen: &mut Vec<MirBlockId>) {
        match region {
            Region::Seq(items) => items.iter().for_each(|item| blocks(item, seen)),
            Region::Block(id) => seen.push(*id),
            Region::If {
                then_region,
                else_region,
                ..
            } => {
                blocks(then_region, seen);
                blocks(else_region, seen);
            }
            Region::Loop { body } => blocks(body, seen),
            Region::Return(_) | Region::Break | Region::Continue => {}
        }
    }
    blocks(&region, &mut seen);
    // bb3 and bb4 are each reached from both arms of bb0 and appear twice.
    assert_eq!(seen.iter().filter(|id| **id == bb(3)).count(), 2);
    assert_eq!(seen.iter().filter(|id| **id == bb(4)).count(), 2);
    // The default entry point still fails closed on the same shape.
    assert!(matches!(
        structurize(&f),
        Err(StructurizeError::SharedTail { .. })
    ));
}

#[test]
fn duplicating_tails_keeps_an_arm_that_conditionally_enters_the_other_arm_apart() {
    // bb0 branches to bb1 | bb2. bb1 branches to bb2 | bb4; bb2 -> bb3 -> bb5;
    // bb4 -> bb5. bb2 is reachable from both arms but bb1 can bypass it, so it
    // is not a join: bb2 is emitted once per arm and the join is bb5.
    let f = function(vec![
        branch(0, 1, 2),
        branch(1, 2, 4),
        goto(3),
        goto(5),
        goto(5),
        ret(None),
    ]);
    let region = structurize_duplicating_tails(&f).expect("tails duplicate");
    let Region::Seq(top) = &region else {
        panic!("top is a Seq")
    };
    let Some(Region::If {
        then_region,
        else_region,
        ..
    }) = top.get(1)
    else {
        panic!("bb0 ends in an If: {top:?}");
    };
    let ids = |region: &Region| -> Vec<MirBlockId> {
        fn walk(region: &Region, out: &mut Vec<MirBlockId>) {
            match region {
                Region::Seq(items) => items.iter().for_each(|item| walk(item, out)),
                Region::Block(id) => out.push(*id),
                Region::If {
                    then_region,
                    else_region,
                    ..
                } => {
                    walk(then_region, out);
                    walk(else_region, out);
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(region, &mut out);
        out
    };
    assert!(
        ids(then_region).contains(&bb(2)),
        "then arm carries its own bb2"
    );
    assert!(ids(else_region).contains(&bb(2)), "else arm carries bb2");
    assert!(!ids(then_region).contains(&bb(5)) && !ids(else_region).contains(&bb(5)));
}

#[test]
fn empty_function_fails_by_name() {
    let f = function(Vec::new());
    assert_eq!(structurize(&f), Err(StructurizeError::EmptyFunction));
}
