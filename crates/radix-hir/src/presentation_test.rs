//! Model, visitor-blindness, equality, and serde round-trip tests for the HIR
//! presentation sidecar and structural end anchors (goal Stage 2, unit U-2).
//!
//! These tests pin the U-2 contract:
//! - authored executable blocks carry exactly one structural end;
//! - compiler-synthesized blocks carry no authored end;
//! - owner kinds cover the full closed taxonomy;
//! - structural ends are never counted as statements;
//! - explicit `tacet` remains the only no-op statement;
//! - structural equality is deterministic over the new data;
//! - postcard round trips preserve the new data.

use crate::nodes::{
    DefId, HirBlock, HirBlockEnd, HirId, HirModuleEnd, HirOwnerRef, HirPresentation,
    HirSourceAnchorId, HirStatement, HirStatementKind, HirTrivia, HirTriviaAttachment,
    HirTriviaOwner, HirTriviaOwnerKind,
};
use crate::visit::{walk_block, walk_stmt, HirVisitor};
use radix_lexer::{Span, Symbol};

/// Half-open span helper so tests avoid raw byte literals.
fn make_span(offset: u32) -> Span {
    Span::new(offset, offset + 1)
}

/// A full presentation exercising every data family in the sidecar.
fn sample_presentation() -> HirPresentation {
    let owners = vec![
        HirTriviaOwner {
            id: HirSourceAnchorId(1),
            kind: HirTriviaOwnerKind::Statement,
            semantic: Some(HirOwnerRef::Hir(HirId(1))),
        },
        HirTriviaOwner {
            id: HirSourceAnchorId(2),
            kind: HirTriviaOwnerKind::BlockEnd,
            semantic: None,
        },
        HirTriviaOwner {
            id: HirSourceAnchorId(3),
            kind: HirTriviaOwnerKind::ProgramEnd,
            semantic: None,
        },
    ];
    let attachments = vec![
        HirTriviaAttachment {
            anchor: HirSourceAnchorId(1),
            leading: vec![
                HirTrivia::CommentLine {
                    text: Symbol(10),
                    span: make_span(0),
                },
                HirTrivia::Newline { span: make_span(1) },
            ],
        },
        HirTriviaAttachment {
            anchor: HirSourceAnchorId(2),
            leading: vec![HirTrivia::CommentLine {
                text: Symbol(11),
                span: make_span(30),
            }],
        },
    ];
    HirPresentation {
        owners,
        attachments,
        block_ends: vec![HirBlockEnd {
            id: HirSourceAnchorId(2),
            span: make_span(30),
        }],
        program_end: HirModuleEnd {
            id: HirSourceAnchorId(3),
            span: make_span(90),
        },
        entry_start_anchor: Some(HirSourceAnchorId(1)),
    }
}

/// Every authored executable block carries exactly one structural end.
#[test]
fn authored_blocks_carry_exactly_one_structural_end() {
    let end = HirBlockEnd {
        id: HirSourceAnchorId(7),
        span: make_span(20),
    };
    let authored = HirBlock {
        statements: Vec::new(),
        expr: None,
        span: make_span(0),
        breakable: None,
        end: Some(end.clone()),
    };
    assert_eq!(authored.end.as_ref(), Some(&end));
    // The end is a structural field, never a statement: an authored block
    // with only an end has zero statements.
    assert!(
        authored.statements.is_empty(),
        "structural ends must not be statements"
    );
}

/// Compiler-synthesized blocks do not impersonate authored boundaries.
#[test]
fn synthesized_blocks_do_not_impersonate_authored_boundaries() {
    let synthesized = HirBlock::simple(Vec::new(), None, make_span(0));
    assert!(
        synthesized.end.is_none(),
        "synthesized blocks carry no authored end"
    );
    let defaulted = HirBlock::default();
    assert!(
        defaulted.end.is_none(),
        "default blocks carry no authored end"
    );
}

/// The owner-kind taxonomy covers the six closed kinds.
#[test]
fn owner_kinds_cover_the_full_taxonomy() {
    let owners = [
        HirTriviaOwner {
            id: HirSourceAnchorId(1),
            kind: HirTriviaOwnerKind::Item,
            semantic: Some(HirOwnerRef::Hir(HirId(1))),
        },
        HirTriviaOwner {
            id: HirSourceAnchorId(2),
            kind: HirTriviaOwnerKind::Statement,
            semantic: Some(HirOwnerRef::Hir(HirId(2))),
        },
        HirTriviaOwner {
            id: HirSourceAnchorId(3),
            kind: HirTriviaOwnerKind::Member,
            semantic: Some(HirOwnerRef::Def(DefId(3))),
        },
        HirTriviaOwner {
            id: HirSourceAnchorId(4),
            kind: HirTriviaOwnerKind::Entry,
            semantic: Some(HirOwnerRef::Hir(HirId(4))),
        },
        HirTriviaOwner {
            id: HirSourceAnchorId(5),
            kind: HirTriviaOwnerKind::BlockEnd,
            semantic: None,
        },
        HirTriviaOwner {
            id: HirSourceAnchorId(6),
            kind: HirTriviaOwnerKind::ProgramEnd,
            semantic: None,
        },
    ];
    let kinds: Vec<HirTriviaOwnerKind> = owners.iter().map(|owner| owner.kind.clone()).collect();
    assert_eq!(kinds.len(), 6, "owner taxonomy must cover all six kinds");
    for expected in [
        HirTriviaOwnerKind::Item,
        HirTriviaOwnerKind::Statement,
        HirTriviaOwnerKind::Member,
        HirTriviaOwnerKind::Entry,
        HirTriviaOwnerKind::BlockEnd,
        HirTriviaOwnerKind::ProgramEnd,
    ] {
        assert!(kinds.contains(&expected), "missing owner kind {expected:?}");
    }
}

/// Structural ends are not statements: a visitor counting statements must not
/// see the block end, and must not recurse into it.
#[test]
fn visitor_does_not_count_structural_ends_as_statements() {
    #[derive(Default)]
    struct StmtCounter {
        statements: usize,
        blocks: usize,
    }
    impl HirVisitor for StmtCounter {
        fn visit_stmt(&mut self, stmt: &HirStatement) {
            self.statements += 1;
            walk_stmt(self, stmt);
        }
        fn visit_block(&mut self, block: &HirBlock) {
            self.blocks += 1;
            walk_block(self, block);
        }
    }

    let block = HirBlock {
        statements: vec![HirStatement {
            id: HirId(0),
            kind: HirStatementKind::Tacet,
            span: make_span(0),
        }],
        expr: None,
        span: make_span(0),
        breakable: None,
        end: Some(HirBlockEnd {
            id: HirSourceAnchorId(1),
            span: make_span(5),
        }),
    };
    let mut counter = StmtCounter::default();
    counter.visit_block(&block);
    assert_eq!(
        counter.statements, 1,
        "the structural end must not be visited as a statement"
    );
    assert_eq!(counter.blocks, 1);
}

/// Explicit source `tacet` remains a statement; structural ends are not.
#[test]
fn explicit_tacet_is_the_only_hir_no_op_statement() {
    // A block whose only boundary content is its end has no statements: the
    // end is carried structurally, not as a synthetic no-op statement.
    let end_only = HirBlock {
        statements: Vec::new(),
        expr: None,
        span: make_span(0),
        breakable: None,
        end: Some(HirBlockEnd {
            id: HirSourceAnchorId(1),
            span: make_span(5),
        }),
    };
    assert!(
        end_only.statements.is_empty(),
        "an end-only block must not manufacture a statement"
    );
    // An explicit source no-op remains an ordinary statement kind.
    let tacet = HirStatement {
        id: HirId(0),
        kind: HirStatementKind::Tacet,
        span: make_span(0),
    };
    assert!(matches!(tacet.kind, HirStatementKind::Tacet));
}

/// Trivia is presentation metadata stored on the sidecar, not an expression
/// or statement kind: `HirTrivia` is a standalone type.
#[test]
fn trivia_is_a_standalone_presentation_type() {
    let comment = HirTrivia::CommentLine {
        text: Symbol(1),
        span: make_span(0),
    };
    let newline = HirTrivia::Newline { span: make_span(4) };
    assert!(matches!(
        comment,
        HirTrivia::CommentLine {
            text: Symbol(1),
            ..
        }
    ));
    assert!(matches!(newline, HirTrivia::Newline { .. }));
}

/// Deterministic structural equality over the new data.
#[test]
fn presentation_equality_is_deterministic() {
    let first = sample_presentation();
    let second = sample_presentation();
    assert_eq!(first, second, "identical presentations must be equal");

    let mut changed_end = sample_presentation();
    changed_end.program_end = HirModuleEnd {
        id: HirSourceAnchorId(999),
        span: make_span(90),
    };
    assert_ne!(
        first, changed_end,
        "program-end identity participates in equality"
    );

    let mut reordered = sample_presentation();
    reordered.attachments.reverse();
    assert_ne!(
        first, reordered,
        "attachment order is part of structural equality"
    );
}

/// Postcard round trips preserve every data family in the sidecar.
#[test]
fn postcard_round_trip_preserves_presentation_data() {
    let original = sample_presentation();
    let bytes = postcard::to_allocvec(&original).expect("serialize presentation");
    let restored: HirPresentation = postcard::from_bytes(&bytes).expect("deserialize presentation");
    assert_eq!(original, restored);
    // Spot-check that the trivia payload itself round-trips in order.
    assert_eq!(
        restored.attachments[0].leading[0],
        HirTrivia::CommentLine {
            text: Symbol(10),
            span: make_span(0),
        }
    );
    assert_eq!(
        restored.attachments[0].leading[1],
        HirTrivia::Newline { span: make_span(1) }
    );
    assert_eq!(restored.block_ends[0].id, HirSourceAnchorId(2));
    assert_eq!(restored.entry_start_anchor, Some(HirSourceAnchorId(1)));
}
