//! Unit tests for the HIR presentation JSON builder.
//!
//! Constructs `HirPresentation` values directly (the sidecar types are public
//! foundation model) and asserts the deterministic inspection JSON that
//! `presentation_json` produces. Covers every owner kind, the semantic
//! back-reference shapes, ordered trivia, structural ends, and the null
//! entry-anchor case.

use super::{annotation_json, presentation_json};
use crate::lexer::{Interner, Span, Symbol};

fn sample_interner(comment_text: &str) -> (Interner, Symbol) {
    let mut interner = Interner::new();
    let symbol = interner.intern_raw(comment_text);
    (interner, symbol)
}

fn span(start: u32, end: u32) -> Span {
    Span { start, end }
}

fn owner(
    id: u32,
    kind: radix_hir::HirTriviaOwnerKind,
    semantic: Option<radix_hir::HirOwnerRef>,
) -> radix_hir::HirTriviaOwner {
    radix_hir::HirTriviaOwner { id: radix_hir::HirSourceAnchorId(id), kind, semantic }
}

#[test]
fn presentation_json_exposes_owners_attachments_and_ends() {
    let (interner, comment_symbol) = sample_interner("statement comment");

    let presentation = radix_hir::HirPresentation {
        owners: vec![
            owner(
                0,
                radix_hir::HirTriviaOwnerKind::Item,
                Some(radix_hir::HirOwnerRef::Hir(radix_hir::HirId(1))),
            ),
            owner(
                1,
                radix_hir::HirTriviaOwnerKind::Member,
                Some(radix_hir::HirOwnerRef::Def(radix_hir::DefId(5))),
            ),
            owner(2, radix_hir::HirTriviaOwnerKind::Entry, None),
            owner(3, radix_hir::HirTriviaOwnerKind::BlockEnd, None),
            owner(4, radix_hir::HirTriviaOwnerKind::ProgramEnd, None),
        ],
        attachments: vec![
            radix_hir::HirTriviaAttachment {
                anchor: radix_hir::HirSourceAnchorId(0),
                leading: vec![radix_hir::HirTrivia::CommentLine { text: comment_symbol, span: span(0, 7) }],
            },
            radix_hir::HirTriviaAttachment {
                anchor: radix_hir::HirSourceAnchorId(1),
                leading: vec![radix_hir::HirTrivia::Newline { span: span(7, 8) }],
            },
        ],
        block_ends: vec![radix_hir::HirBlockEnd { id: radix_hir::HirSourceAnchorId(3), span: span(30, 31) }],
        program_end: radix_hir::HirModuleEnd { id: radix_hir::HirSourceAnchorId(4), span: span(60, 60) },
        entry_start_anchor: Some(radix_hir::HirSourceAnchorId(2)),
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
    };

    let json = presentation_json(&presentation, &interner);

    // Owner records carry anchor, kind, and semantic back-reference.
    assert!(json.contains("\"owners\""), "missing owners array:\n{json}");
    assert!(json.contains("\"anchor\": 0"), "missing item owner anchor:\n{json}");
    assert!(json.contains("\"kind\": \"Item\""), "missing item owner kind:\n{json}");
    assert!(json.contains("\"semantic\": { \"hir\": 1 }"), "missing Hir owner ref:\n{json}");
    assert!(json.contains("\"kind\": \"Member\""), "missing member owner kind:\n{json}");
    assert!(json.contains("\"semantic\": { \"def\": 5 }"), "missing Def owner ref:\n{json}");
    assert!(json.contains("\"kind\": \"Entry\""), "missing entry owner kind:\n{json}");
    assert!(json.contains("\"kind\": \"BlockEnd\""), "missing block-end owner kind:\n{json}");
    assert!(
        json.contains("\"kind\": \"ProgramEnd\""),
        "missing program-end owner kind:\n{json}"
    );

    // Attachments preserve ordered trivia with raw comment payload.
    assert!(json.contains("\"attachments\""), "missing attachments array:\n{json}");
    assert!(
        json.contains("\"kind\": \"CommentLine\""),
        "missing CommentLine trivia:\n{json}"
    );
    assert!(
        json.contains("\"text\": \"statement comment\""),
        "missing raw comment text:\n{json}"
    );
    assert!(json.contains("\"kind\": \"Newline\""), "missing Newline trivia:\n{json}");
    assert!(json.contains("\"span\": [0, 7]"), "missing comment span:\n{json}");

    // Structural ends and entry anchor.
    assert!(json.contains("\"block_ends\""), "missing block_ends:\n{json}");
    assert!(
        json.contains("\"anchor\": 3, \"span\": [30, 31]"),
        "missing block end anchor:\n{json}"
    );
    assert!(json.contains("\"program_end\""), "missing program_end:\n{json}");
    assert!(
        json.contains("\"anchor\": 4, \"span\": [60, 60]"),
        "missing program end anchor:\n{json}"
    );
    assert!(
        json.contains("\"entry_start_anchor\": 2"),
        "missing entry start anchor:\n{json}"
    );
}

#[test]
fn presentation_json_emits_null_semantic_refs_and_null_entry_anchor() {
    let (interner, comment_symbol) = sample_interner("comment");

    let presentation = radix_hir::HirPresentation {
        owners: vec![owner(0, radix_hir::HirTriviaOwnerKind::Statement, None)],
        attachments: vec![radix_hir::HirTriviaAttachment {
            anchor: radix_hir::HirSourceAnchorId(0),
            leading: vec![radix_hir::HirTrivia::CommentLine { text: comment_symbol, span: span(3, 10) }],
        }],
        block_ends: Vec::new(),
        program_end: radix_hir::HirModuleEnd { id: radix_hir::HirSourceAnchorId(1), span: span(11, 11) },
        entry_start_anchor: None,
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
    };

    let json = presentation_json(&presentation, &interner);

    assert!(json.contains("\"semantic\": null"), "expected null semantic ref:\n{json}");
    assert!(
        json.contains("\"entry_start_anchor\": null"),
        "expected null entry anchor:\n{json}"
    );
    assert!(json.contains("\"block_ends\": []"), "expected empty block_ends array:\n{json}");
}

#[test]
fn presentation_json_escapes_comment_payloads() {
    let (interner, comment_symbol) = sample_interner("quoted \" text \\ end");

    let presentation = radix_hir::HirPresentation {
        owners: Vec::new(),
        attachments: vec![radix_hir::HirTriviaAttachment {
            anchor: radix_hir::HirSourceAnchorId(0),
            leading: vec![radix_hir::HirTrivia::CommentLine { text: comment_symbol, span: span(0, 0) }],
        }],
        block_ends: Vec::new(),
        program_end: radix_hir::HirModuleEnd { id: radix_hir::HirSourceAnchorId(1), span: span(0, 0) },
        entry_start_anchor: None,
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
    };

    let json = presentation_json(&presentation, &interner);

    assert!(
        json.contains(r#""text": "quoted \" text \\ end""#),
        "comment payload must be JSON-escaped:\n{json}"
    );
}

#[test]
fn annotation_json_reports_interna_kind() {
    use radix_syntax::AnnotationKind;

    let annotations = vec![crate::syntax::Annotation { kind: AnnotationKind::Interna, span: span(0, 9) }];
    let json = annotation_json(&annotations);
    assert!(json.contains("\"Interna\""), "missing Interna kind: {json}");
    assert!(json.contains("[0, 9]"), "missing Interna span: {json}");
}
