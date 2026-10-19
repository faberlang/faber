//! Leaf tests for presentation-trivia placement in canonical Faber emission
//! (goal Stage 5, unit U-5).
//!
//! These tests drive `FaberCodegen::new(types, interner).generate(hir,
//! presentation, surface)` with synthetic HIR + presentation data so the
//! render-slot contract is provable
//! without lowering or FHIR loading (which land in U-3/U-4):
//! - statement, block-end, and program-end comments land at their structural
//!   positions;
//! - comments before annotations render before the annotation sequence;
//! - comment-bearing candidate blocks stay braced while trivia-free eligible
//!   blocks keep compact `ergo` / `sic … secus …` output;
//! - boundary-newline normalization never manufactures blank lines;
//! - explicit `tacet` prints while implicit structural ends print nothing;
//! - rendering is deterministic for the same inputs.

use crate::surface::TypeBindingKind;
use crate::{FaberCodegen, KeywordSurface};
use radix_hir::{
    DefId, HirAnnotation, HirBlock, HirBlockEnd, HirExpression, HirExpressionKind, HirField,
    HirFunction, HirId, HirItem, HirItemKind, HirLiteral, HirMethod, HirModule, HirModuleEnd,
    HirOwnerRef, HirPresentation, HirReceiver, HirSourceAnchorId, HirStatement, HirStatementKind,
    HirStruct, HirTrivia, HirTriviaAttachment, HirTriviaOwner, HirTriviaOwnerKind,
};
use radix_lexer::{Interner, Span, TokenKind};
use radix_syntax::CallablePosture;
use radix_types::TypeTable;
use radix_types::USER_DEF_ID_BASE;

/// Minimal Latin surface for leaf tests (no reader-pack machinery).
struct TestSurface;

impl KeywordSurface for TestSurface {
    fn keyword(&self, kind: TokenKind) -> &str {
        match kind {
            TokenKind::Incipit => "incipit",
            TokenKind::Functio => "functio",
            TokenKind::Redde => "redde",
            TokenKind::Secus => "secus",
            TokenKind::Si => "si",
            TokenKind::Ergo => "ergo",
            TokenKind::Fixum => "fixum",
            TokenKind::Tacet => "tacet",
            TokenKind::Rumpe => "rumpe",
            TokenKind::Perge => "perge",
            TokenKind::Custodi => "custodi",
            TokenKind::Varia => "varia",
            TokenKind::Cape => "cape",
            TokenKind::Fac => "fac",
            TokenKind::Dum => "dum",
            TokenKind::Discerne => "discerne",
            TokenKind::Casu => "casu",
            TokenKind::Genus => "genus",
            TokenKind::Implendum => "implendum",
            TokenKind::Itera => "itera",
            TokenKind::Ex => "ex",
            TokenKind::De => "de",
            TokenKind::Nihil => "nihil",
            _ => "?",
        }
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::Primitive(radix_types::Primitive::Textus) => "textus",
            TypeBindingKind::Primitive(radix_types::Primitive::Numerus) => "numerus",
            // The tensor-family kinds must not be swallowed silently by the
            // catch-all: keep the Latin emit behavior for leaf tests.
            TypeBindingKind::TensorFamily(radix_types::TensorFamilyKind::Tensor) => "tensor",
            TypeBindingKind::TensorFamily(radix_types::TensorFamilyKind::Vector) => "vector",
            TypeBindingKind::TensorFamily(radix_types::TensorFamilyKind::Matrix) => "matrix",
            TypeBindingKind::TensorFamily(radix_types::TensorFamilyKind::Sparsa) => "sparsa",
            TypeBindingKind::TensorFamily(radix_types::TensorFamilyKind::Atomic) => "atomic",
            TypeBindingKind::TensorFamily(radix_types::TensorFamilyKind::Intervallum) => {
                "intervallum"
            }
            _ => "?",
        }
    }
}

/// Half-open span helper so tests avoid raw byte literals.
fn span() -> Span {
    Span::default()
}

/// One line-start comment with the given raw payload.
fn comment(interner: &mut Interner, text: &str) -> HirTrivia {
    HirTrivia::CommentLine {
        text: interner.intern(text),
        span: span(),
    }
}

/// One boundary newline.
fn newline() -> HirTrivia {
    HirTrivia::Newline { span: span() }
}

/// A comment run with its terminating newline.
fn comment_run(interner: &mut Interner, text: &str) -> Vec<HirTrivia> {
    vec![comment(interner, text), newline()]
}

fn anchor(id: u32) -> HirSourceAnchorId {
    HirSourceAnchorId(id)
}

fn owner(id: u32, kind: HirTriviaOwnerKind, semantic: Option<HirOwnerRef>) -> HirTriviaOwner {
    HirTriviaOwner {
        id: anchor(id),
        kind,
        semantic,
    }
}

fn attachment(id: u32, leading: Vec<HirTrivia>) -> HirTriviaAttachment {
    HirTriviaAttachment {
        anchor: anchor(id),
        leading,
    }
}

/// Empty presentation with a structural program end at anchor 999.
fn empty_presentation() -> HirPresentation {
    HirPresentation {
        owners: Vec::new(),
        attachments: Vec::new(),
        block_ends: Vec::new(),
        program_end: HirModuleEnd {
            id: anchor(999),
            span: span(),
        },
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
    }
}

fn literal_int(id: u32, value: u64) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
        ty: None,
        span: span(),
    }
}

fn redde_stmt(id: u32, expr_id: u32, value: u64) -> HirStatement {
    HirStatement {
        id: HirId(id),
        kind: HirStatementKind::Redde(Some(literal_int(expr_id, value))),
        span: span(),
    }
}

/// A body block that carries an authored structural end.
fn authored_block(statements: Vec<HirStatement>, end_anchor: u32) -> HirBlock {
    HirBlock {
        statements,
        expr: None,
        span: span(),
        breakable: None,
        end: Some(HirBlockEnd {
            id: anchor(end_anchor),
            span: span(),
        }),
    }
}

fn function_item(
    interner: &mut Interner,
    id: u32,
    def_id: u32,
    name: &str,
    body: Option<HirBlock>,
    annotations: Vec<HirAnnotation>,
) -> HirItem {
    HirItem {
        annotations,
        id: HirId(id),
        def_id: DefId(def_id),
        kind: HirItemKind::Function(Box::new(HirFunction {
            name: interner.intern(name),
            type_params: Vec::new(),
            params: Vec::new(),
            cli_args: None,
            ret_ty: None,
            err_ty: None,
            body,
            posture: CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            test: None,
        })),
        span: span(),
    }
}

fn program(items: Vec<HirItem>, entry: Option<HirBlock>) -> HirModule {
    HirModule {
        items,
        entry,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

fn render(hir: &HirModule, presentation: &HirPresentation, interner: &Interner) -> String {
    let types = TypeTable::new();
    FaberCodegen::new(&types, interner)
        .generate(hir, presentation, &TestSurface)
        .expect("emit should succeed")
        .code
}

// ── Render slots ──

#[test]
fn emits_statement_trivia_before_each_statement() {
    let mut interner = Interner::new();
    let body = HirBlock::simple(
        vec![redde_stmt(1, 10, 1), redde_stmt(2, 11, 2)],
        None,
        span(),
    );
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            Some(body),
            vec![],
        )],
        None,
    );
    let presentation = HirPresentation {
        owners: vec![
            owner(
                1,
                HirTriviaOwnerKind::Statement,
                Some(HirOwnerRef::Hir(HirId(1))),
            ),
            owner(
                2,
                HirTriviaOwnerKind::Statement,
                Some(HirOwnerRef::Hir(HirId(2))),
            ),
        ],
        attachments: vec![
            attachment(1, comment_run(&mut interner, " first comment")),
            attachment(2, comment_run(&mut interner, " second comment")),
        ],
        block_ends: Vec::new(),
        program_end: HirModuleEnd {
            id: anchor(999),
            span: span(),
        },
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

    let code = render(&hir, &presentation, &interner);
    assert!(
        code.contains("# first comment\n    redde 1"),
        "statement trivia must precede its statement:\n{code}"
    );
    assert!(
        code.contains("# second comment\n    redde 2"),
        "second statement trivia must precede its statement:\n{code}"
    );
}

#[test]
fn emits_lifted_statement_trivia_before_block_tail() {
    let mut interner = Interner::new();
    let body = HirBlock::simple(Vec::new(), Some(Box::new(literal_int(10, 1))), span());
    let mut presentation = crate::empty_presentation();
    presentation.owners.push(owner(
        1,
        HirTriviaOwnerKind::Statement,
        Some(HirOwnerRef::Hir(HirId(10))),
    ));
    presentation
        .attachments
        .push(attachment(1, comment_run(&mut interner, " tail comment")));
    assert!(
        crate::trivia::TriviaIndex::from_presentation(&presentation).block_has_trivia(&body),
        "a lifted statement's tail comment must prevent comment-free compaction"
    );
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            Some(body),
            vec![],
        )],
        None,
    );
    let code = render(&hir, &presentation, &interner);
    assert!(
        code.contains("    # tail comment\n    1\n"),
        "tail trivia must precede its value:\n{code}"
    );
}

#[test]
fn emits_block_end_trivia_before_closing_brace() {
    let mut interner = Interner::new();
    let body = authored_block(vec![redde_stmt(1, 10, 1)], 20);
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            Some(body),
            vec![],
        )],
        None,
    );
    let presentation = HirPresentation {
        owners: vec![owner(20, HirTriviaOwnerKind::BlockEnd, None)],
        attachments: vec![attachment(20, comment_run(&mut interner, " end of block"))],
        block_ends: vec![HirBlockEnd {
            id: anchor(20),
            span: span(),
        }],
        program_end: HirModuleEnd {
            id: anchor(999),
            span: span(),
        },
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

    let code = render(&hir, &presentation, &interner);
    assert!(
        code.contains("    redde 1\n    # end of block\n}"),
        "block-end trivia must render before the closing brace:\n{code}"
    );
}

#[test]
fn emits_program_end_trivia_before_eof() {
    let mut interner = Interner::new();
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            None,
            vec![],
        )],
        None,
    );
    let presentation = HirPresentation {
        owners: vec![owner(30, HirTriviaOwnerKind::ProgramEnd, None)],
        attachments: vec![attachment(
            30,
            comment_run(&mut interner, " trailing comment"),
        )],
        block_ends: Vec::new(),
        program_end: HirModuleEnd {
            id: anchor(30),
            span: span(),
        },
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

    let code = render(&hir, &presentation, &interner);
    assert!(
        code.trim_end().ends_with("# trailing comment"),
        "program-end trivia must render before EOF:\n{code}"
    );
}

#[test]
fn emits_item_trivia_before_item() {
    let mut interner = Interner::new();
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            None,
            vec![],
        )],
        None,
    );
    let presentation = HirPresentation {
        owners: vec![owner(
            1,
            HirTriviaOwnerKind::Item,
            Some(HirOwnerRef::Hir(HirId(100))),
        )],
        attachments: vec![attachment(1, comment_run(&mut interner, " item comment"))],
        block_ends: Vec::new(),
        program_end: HirModuleEnd {
            id: anchor(999),
            span: span(),
        },
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

    let code = render(&hir, &presentation, &interner);
    assert!(
        code.starts_with("# item comment\nfunctio"),
        "item trivia must render before the item:\n{code}"
    );
}

#[test]
fn emits_entry_trivia_before_incipit_keyword() {
    let mut interner = Interner::new();
    let entry = HirBlock::simple(vec![redde_stmt(1, 10, 1)], None, span());
    let hir = program(Vec::new(), Some(entry));
    let presentation = HirPresentation {
        owners: vec![owner(1, HirTriviaOwnerKind::Entry, None)],
        attachments: vec![attachment(1, comment_run(&mut interner, " entry comment"))],
        block_ends: Vec::new(),
        program_end: HirModuleEnd {
            id: anchor(999),
            span: span(),
        },
        entry_start_anchor: Some(anchor(1)),
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

    let code = render(&hir, &presentation, &interner);
    assert!(
        code.contains("# entry comment\nincipit"),
        "entry trivia must render before the incipit keyword:\n{code}"
    );
}

#[test]
fn emits_member_trivia_for_struct_fields_and_methods() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let textus = types.primitive(radix_types::Primitive::Textus);
    let field = HirField {
        def_id: DefId(100),
        name: interner.intern("name"),
        ty: textus,
        modifier: None,
        sponte: false,
        annotations: Vec::new(),
        init: None,
        span: span(),
    };
    let method_body = HirBlock::simple(vec![redde_stmt(5, 50, 7)], None, span());
    let method = HirMethod {
        annotations: Vec::new(),
        def_id: DefId(101),
        func: HirFunction {
            name: interner.intern("greet"),
            type_params: Vec::new(),
            params: Vec::new(),
            cli_args: None,
            ret_ty: None,
            err_ty: None,
            body: Some(method_body),
            posture: CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            test: None,
        },
        receiver: HirReceiver::None,
        span: span(),
    };
    let s = HirStruct {
        name: interner.intern("Person"),
        type_params: Vec::new(),
        fields: vec![field],
        methods: vec![method],
        implements: Vec::new(),
    };
    let hir = program(
        vec![HirItem {
            annotations: Vec::new(),
            id: HirId(1),
            def_id: DefId(USER_DEF_ID_BASE),
            kind: HirItemKind::Struct(s),
            span: span(),
        }],
        None,
    );
    let presentation = HirPresentation {
        owners: vec![
            owner(
                1,
                HirTriviaOwnerKind::Member,
                Some(HirOwnerRef::Def(DefId(100))),
            ),
            owner(
                2,
                HirTriviaOwnerKind::Member,
                Some(HirOwnerRef::Def(DefId(101))),
            ),
        ],
        attachments: vec![
            attachment(1, comment_run(&mut interner, " field comment")),
            attachment(2, comment_run(&mut interner, " method comment")),
        ],
        block_ends: Vec::new(),
        program_end: HirModuleEnd {
            id: anchor(999),
            span: span(),
        },
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

    let code = FaberCodegen::new(&types, &interner)
        .generate(&hir, &presentation, &TestSurface)
        .expect("emit should succeed")
        .code;
    assert!(
        code.contains("# field comment\n    textus name"),
        "field trivia must render before the field:\n{code}"
    );
    assert!(
        code.contains("# method comment\n    functio greet"),
        "method trivia must render before the method:\n{code}"
    );
}

#[test]
fn emits_trivia_before_annotation_sequence() {
    let mut interner = Interner::new();
    let annotations = vec![HirAnnotation::with_fields(
        interner.intern("futura"),
        Vec::new(),
        span(),
    )];
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            None,
            annotations,
        )],
        None,
    );
    let presentation = HirPresentation {
        owners: vec![owner(
            1,
            HirTriviaOwnerKind::Item,
            Some(HirOwnerRef::Hir(HirId(100))),
        )],
        attachments: vec![attachment(
            1,
            comment_run(&mut interner, " before annotation"),
        )],
        block_ends: Vec::new(),
        program_end: HirModuleEnd {
            id: anchor(999),
            span: span(),
        },
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

    let code = render(&hir, &presentation, &interner);
    assert!(
        code.starts_with("# before annotation\n@ futura { }\nfunctio"),
        "item trivia must render before the annotation sequence:\n{code}"
    );
}

// ── Compact-joint gating ──

fn si_stmt(
    stmt_id: u32,
    expr_id: u32,
    cond_id: u32,
    cond_value: u64,
    then_block: HirBlock,
) -> HirStatement {
    HirStatement {
        id: HirId(stmt_id),
        kind: HirStatementKind::Expr(HirExpression {
            id: HirId(expr_id),
            kind: HirExpressionKind::Si {
                cond: Box::new(literal_int(cond_id, cond_value)),
                then_block,
                then_catch: None,
                else_block: None,
            },
            ty: None,
            span: span(),
        }),
        span: span(),
    }
}

/// A comment-bearing single-return branch stays braced — never `ergo`.
#[test]
fn comment_bearing_branch_stays_braced_not_ergo() {
    let mut interner = Interner::new();
    let then_block = HirBlock::simple(vec![redde_stmt(2, 20, 5)], None, span());
    let body = HirBlock::simple(vec![si_stmt(1, 1, 0, 1, then_block)], None, span());
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            Some(body),
            vec![],
        )],
        None,
    );
    let presentation = HirPresentation {
        owners: vec![owner(
            2,
            HirTriviaOwnerKind::Statement,
            Some(HirOwnerRef::Hir(HirId(2))),
        )],
        attachments: vec![attachment(2, comment_run(&mut interner, " keep braced"))],
        block_ends: Vec::new(),
        program_end: HirModuleEnd {
            id: anchor(999),
            span: span(),
        },
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

    let code = render(&hir, &presentation, &interner);
    assert!(
        code.contains("si 1 {\n        # keep braced\n        redde 5\n    }"),
        "comment-bearing branch must stay braced:\n{code}"
    );
    assert!(
        !code.contains("ergo"),
        "compact ergo must not absorb a comment-bearing branch:\n{code}"
    );
}

/// A trivia-free eligible single-return branch keeps the compact `ergo` form.
#[test]
fn trivia_free_eligible_block_keeps_compact_ergo() {
    let mut interner = Interner::new();
    let then_block = HirBlock::simple(vec![redde_stmt(2, 20, 5)], None, span());
    let body = HirBlock::simple(vec![si_stmt(1, 1, 0, 1, then_block)], None, span());
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            Some(body),
            vec![],
        )],
        None,
    );

    let code = render(&hir, &empty_presentation(), &interner);
    assert!(
        code.contains("si 1 ergo redde 5"),
        "trivia-free eligible block must stay compact:\n{code}"
    );
}

fn si_else_stmt(
    stmt_id: u32,
    expr_id: u32,
    cond_id: u32,
    cond_value: u64,
    then_block: HirBlock,
    else_block: HirBlock,
) -> HirStatement {
    HirStatement {
        id: HirId(stmt_id),
        kind: HirStatementKind::Expr(HirExpression {
            id: HirId(expr_id),
            kind: HirExpressionKind::Si {
                cond: Box::new(literal_int(cond_id, cond_value)),
                then_block,
                then_catch: None,
                else_block: Some(else_block),
            },
            ty: None,
            span: span(),
        }),
        span: span(),
    }
}

/// A comment-bearing branch prevents `sic … secus …` compaction.
#[test]
fn comment_bearing_branch_prevents_sic_secus_compaction() {
    let mut interner = Interner::new();
    let then_block = HirBlock::simple(vec![redde_stmt(2, 20, 5)], None, span());
    let else_block = HirBlock::simple(vec![redde_stmt(3, 30, 6)], None, span());
    let body = HirBlock::simple(
        vec![si_else_stmt(1, 1, 0, 1, then_block, else_block)],
        None,
        span(),
    );
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            Some(body),
            vec![],
        )],
        None,
    );
    let presentation = HirPresentation {
        owners: vec![owner(
            2,
            HirTriviaOwnerKind::Statement,
            Some(HirOwnerRef::Hir(HirId(2))),
        )],
        attachments: vec![attachment(2, comment_run(&mut interner, " keep braced"))],
        block_ends: Vec::new(),
        program_end: HirModuleEnd {
            id: anchor(999),
            span: span(),
        },
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

    let code = render(&hir, &presentation, &interner);
    assert!(
        code.contains("si 1 {\n        # keep braced\n        redde 5\n    }\n    secus"),
        "comment-bearing branch must render as a braced si/secus chain:\n{code}"
    );
    assert!(
        !code.contains("sic"),
        "compact sic/secus must not absorb a comment-bearing branch:\n{code}"
    );
}

/// Trivia-free `redde` branches keep the statement `ergo` chain.
///
/// The compact `sic … secus …` expression form has no statement slot: a branch
/// whose value is an explicit `redde` would lose its return, so those chains
/// must re-emit as `si … ergo redde … secus ergo redde …`.
#[test]
fn trivia_free_blocks_keep_sic_secus_compact() {
    let mut interner = Interner::new();
    let then_block = HirBlock::simple(vec![redde_stmt(2, 20, 5)], None, span());
    let else_block = HirBlock::simple(vec![redde_stmt(3, 30, 6)], None, span());
    let body = HirBlock::simple(
        vec![si_else_stmt(1, 1, 0, 1, then_block, else_block)],
        None,
        span(),
    );
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            Some(body),
            vec![],
        )],
        None,
    );

    let code = render(&hir, &empty_presentation(), &interner);
    assert!(
        code.contains("si 1 ergo redde 5\n    secus ergo redde 6"),
        "redde branches must keep the statement ergo chain:\n{code}"
    );
    assert!(
        !code.contains("sic"),
        "compact sic/secus must not absorb a redde branch:\n{code}"
    );
}

// ── Boundary-newline normalization ──

/// The emitter's own statement newline plus the first stored leading newline
/// must not manufacture a blank line; intentional blank lines are preserved.
#[test]
fn boundary_newline_normalization_does_not_manufacture_blank_lines() {
    let mut interner = Interner::new();
    let body = HirBlock::simple(
        vec![redde_stmt(1, 10, 1), redde_stmt(2, 11, 2)],
        None,
        span(),
    );
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            Some(body),
            vec![],
        )],
        None,
    );
    let presentation = HirPresentation {
        owners: vec![
            owner(
                1,
                HirTriviaOwnerKind::Statement,
                Some(HirOwnerRef::Hir(HirId(1))),
            ),
            owner(
                2,
                HirTriviaOwnerKind::Statement,
                Some(HirOwnerRef::Hir(HirId(2))),
            ),
        ],
        // First statement: boundary newline + comment (no blank line).
        // Second statement: boundary newline + one intentional blank line + comment.
        attachments: vec![
            attachment(
                1,
                vec![newline(), comment(&mut interner, " first"), newline()],
            ),
            attachment(
                2,
                vec![
                    newline(),
                    newline(),
                    comment(&mut interner, " second"),
                    newline(),
                ],
            ),
        ],
        block_ends: Vec::new(),
        program_end: HirModuleEnd {
            id: anchor(999),
            span: span(),
        },
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

    let code = render(&hir, &presentation, &interner);
    assert!(
        code.contains("    # first\n    redde 1"),
        "boundary newline must not become a blank line:\n{code}"
    );
    assert!(
        code.contains("    # first\n    redde 1\n\n    # second"),
        "one intentional blank line before the second comment must survive:\n{code}"
    );
}

// ── file-start boundary newlines ──

/// A file-start trivia run keeps no leading blank lines.
///
/// Round 1 of a source whose body begins with two or more blank lines (e.g.
/// after `+++` frontmatter) must render the same as round 2 — canonical output
/// never begins with blank lines.
#[test]
fn file_start_trivia_keeps_no_leading_blank_lines() {
    let mut interner = Interner::new();
    let body = authored_block(vec![redde_stmt(1, 10, 1)], 20);
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            Some(body),
            vec![],
        )],
        None,
    );
    // Two leading newlines (frontmatter gap) then a comment.
    let leading = vec![newline(), newline(), comment(&mut interner, " file head")];
    let presentation = HirPresentation {
        owners: vec![owner(
            5,
            HirTriviaOwnerKind::Item,
            Some(HirOwnerRef::Hir(HirId(100))),
        )],
        attachments: vec![attachment(5, leading)],
        block_ends: Vec::new(),
        program_end: HirModuleEnd {
            id: anchor(999),
            span: span(),
        },
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

    let code = render(&hir, &presentation, &interner);
    assert!(
        code.starts_with("# file head\nfunctio noop()"),
        "canonical output must not begin with blank lines:\n{code}"
    );
}

// ── tacet vs structural ends ──

/// Explicit source `tacet` prints; an implicit structural end prints nothing.
#[test]
fn explicit_tacet_prints_implicit_ends_do_not() {
    let mut interner = Interner::new();
    let body = authored_block(
        vec![HirStatement {
            id: HirId(1),
            kind: HirStatementKind::Tacet,
            span: span(),
        }],
        20,
    );
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            Some(body),
            vec![],
        )],
        None,
    );
    // The block end anchor has NO attachment: the structural end is silent.
    let presentation = HirPresentation {
        owners: vec![owner(20, HirTriviaOwnerKind::BlockEnd, None)],
        attachments: Vec::new(),
        block_ends: vec![HirBlockEnd {
            id: anchor(20),
            span: span(),
        }],
        program_end: HirModuleEnd {
            id: anchor(999),
            span: span(),
        },
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

    let code = render(&hir, &presentation, &interner);
    assert_eq!(
        code.matches("tacet").count(),
        1,
        "explicit tacet must print exactly once; structural ends print nothing:\n{code}"
    );
    assert!(
        code.contains("    tacet\n}"),
        "the braced block must close after the tacet statement:\n{code}"
    );
}

// ── Determinism ──

#[test]
fn rendering_is_deterministic() {
    let mut interner = Interner::new();
    let body = authored_block(vec![redde_stmt(1, 10, 1)], 20);
    let hir = program(
        vec![function_item(
            &mut interner,
            100,
            USER_DEF_ID_BASE,
            "noop",
            Some(body),
            vec![],
        )],
        None,
    );
    let presentation = HirPresentation {
        owners: vec![
            owner(
                1,
                HirTriviaOwnerKind::Statement,
                Some(HirOwnerRef::Hir(HirId(1))),
            ),
            owner(20, HirTriviaOwnerKind::BlockEnd, None),
        ],
        attachments: vec![
            attachment(1, comment_run(&mut interner, " statement comment")),
            attachment(20, comment_run(&mut interner, " block end comment")),
        ],
        block_ends: vec![HirBlockEnd {
            id: anchor(20),
            span: span(),
        }],
        program_end: HirModuleEnd {
            id: anchor(999),
            span: span(),
        },
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

    let first = render(&hir, &presentation, &interner);
    let second = render(&hir, &presentation, &interner);
    assert_eq!(first, second, "same inputs must render deterministically");
}

/// Render `<receiver>.transpose()` as the printer sees it, optionally with
/// the method-call node id recorded as an authored `ᵀ` glyph.
fn render_transpose(receiver: HirExpressionKind, glyph: bool) -> String {
    let types = TypeTable::new();
    let mut interner = Interner::new();
    let method = interner.intern("transpose");
    let mut codegen = FaberCodegen::new(&types, &interner);
    let node = HirId(100);
    if glyph {
        codegen.glyph_ids.insert(node);
    }
    let call = HirExpression {
        id: node,
        kind: HirExpressionKind::MethodCall(
            Box::new(HirExpression {
                id: HirId(101),
                kind: receiver,
                ty: None,
                span: span(),
            }),
            method,
            Vec::new(),
            Vec::new(),
            None,
        ),
        ty: None,
        span: span(),
    };
    let mut writer = radix_codegen_shared::CodeWriter::new();
    codegen.write_expr(
        &call,
        &rustc_hash::FxHashMap::default(),
        &TestSurface,
        &mut writer,
    );
    writer.finish()
}

fn sum_of_two_literals() -> HirExpressionKind {
    let literal = |id: u32, value: u64| {
        Box::new(HirExpression {
            id: HirId(id),
            kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
            ty: None,
            span: span(),
        })
    };
    HirExpressionKind::Binary(radix_hir::HirBinOp::Add, literal(102, 1), literal(103, 2))
}

#[test]
fn glyph_transpose_prints_the_glyph_for_a_recorded_id() {
    assert_eq!(
        render_transpose(HirExpressionKind::Literal(HirLiteral::Int(1)), true),
        "1ᵀ"
    );
}

#[test]
fn glyph_transpose_parenthesizes_a_compound_receiver() {
    assert_eq!(render_transpose(sum_of_two_literals(), true), "(1 + 2)ᵀ");
}

#[test]
fn transpose_without_a_recorded_glyph_stays_a_method_call() {
    assert_eq!(
        render_transpose(sum_of_two_literals(), false),
        "(1 + 2).transpose()"
    );
}

/// An English-pack stand-in: the rounding twins project to their `en`
/// intrinsic spellings, as a non-`la` reader pack does.
struct EnRoundingSurface;

impl KeywordSurface for EnRoundingSurface {
    fn keyword(&self, kind: TokenKind) -> &str {
        TestSurface.keyword(kind)
    }

    fn contextual_keyword(&self, key: &'static str) -> &str {
        key
    }

    fn intrinsic_surface(&self, canonical: &str) -> Option<String> {
        match canonical {
            "pavimentum" => Some("floor".to_owned()),
            "tectum" => Some("ceiling".to_owned()),
            "rotunda" => Some("round".to_owned()),
            _ => None,
        }
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        TestSurface.type_surface(kind)
    }
}

/// Render `(1 + 2).<method>()` as the printer sees a rounding twin, optionally
/// with the node id recorded as an authored rounding glyph.
fn render_rounding(method: &str, glyph: bool, surface: &impl KeywordSurface) -> String {
    let types = TypeTable::new();
    let mut interner = Interner::new();
    let method = interner.intern(method);
    let mut codegen = FaberCodegen::new(&types, &interner);
    let node = HirId(300);
    if glyph {
        codegen.glyph_ids.insert(node);
    }
    let call = HirExpression {
        id: node,
        kind: HirExpressionKind::MethodCall(
            Box::new(HirExpression {
                id: HirId(301),
                kind: sum_of_two_literals(),
                ty: None,
                span: span(),
            }),
            method,
            Vec::new(),
            Vec::new(),
            None,
        ),
        ty: None,
        span: span(),
    };
    let mut writer = radix_codegen_shared::CodeWriter::new();
    codegen.write_expr(
        &call,
        &rustc_hash::FxHashMap::default(),
        surface,
        &mut writer,
    );
    writer.finish()
}

#[test]
fn rounding_glyph_prints_its_delimiters_for_a_recorded_id() {
    // The same three delimiter pairs in every locale: the glyphs are not
    // translated, only the method spellings are.
    for (canonical, localized, glyph) in [
        ("pavimentum", "floor", "⌊1 + 2⌋"),
        ("tectum", "ceiling", "⌈1 + 2⌉"),
        ("rotunda", "round", "⌊1 + 2⌉"),
    ] {
        assert_eq!(render_rounding(canonical, true, &TestSurface), glyph);
        assert_eq!(render_rounding(localized, true, &EnRoundingSurface), glyph);
    }
}

#[test]
fn rounding_twin_without_a_recorded_glyph_stays_a_method_call() {
    assert_eq!(
        render_rounding("pavimentum", false, &TestSurface),
        "(1 + 2).pavimentum()"
    );
    assert_eq!(
        render_rounding("floor", false, &EnRoundingSurface),
        "(1 + 2).floor()"
    );
}

/// Render `<place> ← <place> <op> 2` (or the unary form) as the printer sees
/// the desugared in-place numeric twin, optionally recorded on the sidecar.
fn render_in_place_twin(unary: bool, recorded: bool) -> String {
    let types = TypeTable::new();
    let interner = Interner::new();
    let mut codegen = FaberCodegen::new(&types, &interner);
    let node = HirId(200);
    if recorded {
        codegen.numeric_twin_ids.insert(node);
    }
    let literal = |id: u32, value: u64| {
        Box::new(HirExpression {
            id: HirId(id),
            kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
            ty: None,
            span: span(),
        })
    };
    let value = if unary {
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, literal(202, 1))
    } else {
        HirExpressionKind::Binary(radix_hir::HirBinOp::Add, literal(202, 1), literal(203, 2))
    };
    let assign = HirExpression {
        id: node,
        kind: HirExpressionKind::Assign(
            literal(201, 1),
            Box::new(HirExpression {
                id: HirId(204),
                kind: value,
                ty: None,
                span: span(),
            }),
        ),
        ty: None,
        span: span(),
    };
    let mut writer = radix_codegen_shared::CodeWriter::new();
    codegen.write_expr(
        &assign,
        &rustc_hash::FxHashMap::default(),
        &TestSurface,
        &mut writer,
    );
    writer.finish()
}

#[test]
fn recorded_in_place_numeric_twin_prints_the_method_call() {
    assert_eq!(render_in_place_twin(false, true), "1.adde(2)");
    assert_eq!(render_in_place_twin(true, true), "1.nega()");
}

#[test]
fn unrecorded_assignment_stays_an_assignment() {
    assert_eq!(render_in_place_twin(false, false), "1 ← 1 + 2");
}
