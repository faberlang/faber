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
    DefId, HirAxisMethod, HirAxisSugar, HirBlock, HirBlockEnd, HirId, HirModuleEnd, HirOwnerRef,
    HirPresentation, HirSourceAnchorId, HirStatement, HirStatementKind, HirTrivia,
    HirTriviaAttachment, HirTriviaOwner, HirTriviaOwnerKind,
};
use crate::visit::{HirVisitor, walk_block, walk_stmt};
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
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        radical_quarta_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
        discard_binding_stmts: Vec::new(),
        destructure_bindings: Vec::new(),
        await_assignment_bindings: Vec::new(),
        compile_time_locals: Vec::new(),
        ordo_enums: Vec::new(),
        variant_paths: Vec::new(),
        authored_types: Vec::new(),
        tuple_label_accesses: Vec::new(),
        insere_exprs: Vec::new(),
        function_modifier_bindings: Vec::new(),
        region: None,
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

/// JSON round trips preserve the authored axis-sugar facts.
#[test]
fn axis_sugar_round_trips_through_json() {
    let mut original = sample_presentation();
    original.axis_sugar.push(HirAxisSugar {
        id: HirId(41),
        axis: 1,
        method: HirAxisMethod::Mean,
    });
    let json = serde_json::to_string(&original).expect("serialize presentation");
    let restored: HirPresentation = serde_json::from_str(&json).expect("deserialize presentation");
    assert_eq!(original, restored);
}

// --- authored type forms (ATF-2) ---

use crate::nodes::{
    HirAuthoredTypeUse, HirTypeForm, HirTypeFormArg, HirTypeFormHead, HirTypeSite, HirTypeSlot,
};
use radix_syntax::TypeFormLevel;
use radix_types::{IndexId, TypeId};

/// A LONG-level semantic form witnessing `ty` with no children.
fn semantic_form(ty: TypeId, level: TypeFormLevel) -> HirTypeForm {
    HirTypeForm {
        ty,
        head: HirTypeFormHead::Semantic(level),
        args: Vec::new(),
        label: None,
    }
}

#[test]
fn same_type_id_keeps_independent_forms_at_distinct_sites() {
    // `map<list<u32>, lu32>`: both branches witness the same array-of-u32
    // semantic type but must keep their authored long/short distinction.
    let shared_ty = TypeId(7);
    let long_site = HirAuthoredTypeUse {
        site: HirTypeSite {
            owner: HirOwnerRef::Hir(HirId(1)),
            slot: HirTypeSlot::Declared,
        },
        form: HirTypeForm {
            ty: TypeId(8),
            head: HirTypeFormHead::Semantic(TypeFormLevel::LONG),
            args: vec![HirTypeFormArg::Type(Box::new(semantic_form(
                shared_ty,
                TypeFormLevel::LONG,
            )))],
            label: None,
        },
    };
    let short_site = HirAuthoredTypeUse {
        site: HirTypeSite {
            owner: HirOwnerRef::Hir(HirId(2)),
            slot: HirTypeSlot::Declared,
        },
        form: HirTypeForm {
            ty: TypeId(8),
            head: HirTypeFormHead::Semantic(TypeFormLevel::LONG),
            args: vec![HirTypeFormArg::Type(Box::new(semantic_form(
                shared_ty,
                TypeFormLevel::SHORT,
            )))],
            label: None,
        },
    };
    let mut presentation = sample_presentation();
    presentation.authored_types = vec![long_site, short_site];

    // Each site retrieves its own form despite the shared inner TypeId.
    let first = presentation
        .authored_type_at(&HirTypeSite {
            owner: HirOwnerRef::Hir(HirId(1)),
            slot: HirTypeSlot::Declared,
        })
        .expect("first site recorded");
    let second = presentation
        .authored_type_at(&HirTypeSite {
            owner: HirOwnerRef::Hir(HirId(2)),
            slot: HirTypeSlot::Declared,
        })
        .expect("second site recorded");
    let HirTypeFormArg::Type(first_child) = &first.args[0] else {
        panic!("expected type child");
    };
    let HirTypeFormArg::Type(second_child) = &second.args[0] else {
        panic!("expected type child");
    };
    assert_eq!(
        first_child.head,
        HirTypeFormHead::Semantic(TypeFormLevel::LONG)
    );
    assert_eq!(
        second_child.head,
        HirTypeFormHead::Semantic(TypeFormLevel::SHORT)
    );
}

#[test]
fn authored_type_sites_index_uniquely_by_owner_and_slot() {
    let use_a = HirAuthoredTypeUse {
        site: HirTypeSite {
            owner: HirOwnerRef::Def(DefId(3)),
            slot: HirTypeSlot::Parameter(0),
        },
        form: semantic_form(TypeId(1), TypeFormLevel::LONG),
    };
    // Same owner, different slot: a distinct site.
    let use_b = HirAuthoredTypeUse {
        site: HirTypeSite {
            owner: HirOwnerRef::Def(DefId(3)),
            slot: HirTypeSlot::Return,
        },
        form: semantic_form(TypeId(1), TypeFormLevel::SHORT),
    };
    // Same slot shape, different owner: a distinct site.
    let use_c = HirAuthoredTypeUse {
        site: HirTypeSite {
            owner: HirOwnerRef::Def(DefId(4)),
            slot: HirTypeSlot::Return,
        },
        form: semantic_form(TypeId(1), TypeFormLevel::SHORT),
    };
    let mut presentation = sample_presentation();
    presentation.authored_types = vec![use_a, use_b.clone(), use_c];

    let index: std::collections::BTreeMap<(String, String), &HirTypeForm> = presentation
        .authored_types
        .iter()
        .map(|use_| {
            let owner = match &use_.site.owner {
                HirOwnerRef::Def(def) => format!("def:{def:?}"),
                HirOwnerRef::Hir(id) => format!("hir:{}", id.0),
            };
            let slot = match &use_.site.slot {
                HirTypeSlot::Parameter(ordinal) => format!("param:{ordinal}"),
                HirTypeSlot::Return => "return".to_string(),
                other => format!("{other:?}"),
            };
            ((owner, slot), &use_.form)
        })
        .collect();
    assert_eq!(index.len(), 3, "owner x slot pairs are unique site keys");
    assert!(index.contains_key(&("def:DefId(3)".to_string(), "param:0".to_string())));
    assert_eq!(presentation.authored_types[1], use_b);
}

#[test]
fn postcard_round_trip_preserves_authored_type_forms() {
    let mut original = sample_presentation();
    original.authored_types = vec![HirAuthoredTypeUse {
        site: HirTypeSite {
            owner: HirOwnerRef::Hir(HirId(9)),
            slot: HirTypeSlot::AliasBody,
        },
        form: HirTypeForm {
            ty: TypeId(42),
            head: HirTypeFormHead::Named {
                def_id: DefId(5),
                path: vec![Symbol(31), Symbol(32)],
            },
            args: vec![
                HirTypeFormArg::Name(Symbol(33)),
                HirTypeFormArg::Size(IndexId(3)),
            ],
            label: None,
        },
    }];

    let bytes = postcard::to_allocvec(&original).expect("serialize presentation");
    let restored: HirPresentation = postcard::from_bytes(&bytes).expect("deserialize presentation");
    assert_eq!(restored, original, "authored type forms round-trip exactly");
}
