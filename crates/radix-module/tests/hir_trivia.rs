//! Goal Stage 3 gate: complete lowering coverage of HIR presentation trivia.
//!
//! These integration proofs run the full parse → collect → resolve → lower
//! frontend and inspect the presentation sidecar that U-4/U-5/U-6 consume:
//!
//! - every syntax boundary owning `leading_trivia` lowers to a stable
//!   `HirSourceAnchorId` owner (statement, item, member, entry);
//! - authored executable blocks and the program carry structural ends;
//! - expanded statements attach exactly once to the first rendered node;
//! - zero-output constructs with comments diagnose instead of dropping;
//! - top-level declaration/entry partitioning preserves attachments;
//! - nested control-flow blocks retain their own end comments;
//! - comments at unnamed points fail in the parser (U-1) with a structured
//!   diagnostic and never reach the presentation, while a genus's closing
//!   `}` is a legal end-trivia boundary (ClassDecl.end_trivia, MT-U2) whose
//!   comment attaches at container close;

use radix_module::hir::{
    HirBlock, HirItemKind, HirModule, HirOwnerRef, HirPresentation, HirSourceAnchorId, HirStatement, HirTrivia,
    HirTriviaOwnerKind, LowerError, LoweredProgram,
};
use radix_module::lexer::Interner;
use radix_module::semantic::{Resolver, TypeTable, passes};

// =============================================================================
// Frontend harness (parse → collect → resolve → lower)
// =============================================================================

fn lower_source(source: &str) -> (LoweredProgram, Vec<LowerError>, Interner) {
    let lex_result = radix_module::lexer::lex(source);
    let parse_result = radix_module::parser::parse(lex_result);
    assert!(parse_result.success(), "parse failed: {:?}", parse_result.errors);
    let program = parse_result.program.expect("expected parsed program");
    let mut interner = parse_result.interner;
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();
    radix_module::builtins::register(&mut resolver, &mut types, &mut interner);
    radix_module::builtins::register_frame_types_if_needed(&program, &mut resolver, &mut types, &mut interner, None);
    if let Err(errors) = passes::collect::collect(&program, &mut resolver, &mut types) {
        panic!("collect failed: {errors:?}");
    }
    if let Err(errors) = passes::resolve::resolve(&program, &mut resolver, &interner, &mut types) {
        panic!("resolve failed: {errors:?}");
    }
    let (lowered, errors) = radix_module::hir::lower(radix_module::hir::LowerRequest {
        program: &program,
        resolver: &mut resolver,
        types: &mut types,
        interner: &mut interner,
        cli_program: None,
        locale_pack: None,
        annotation_contracts: None,
    });
    (lowered, errors, interner)
}

fn comment_texts(trivia: &[HirTrivia], interner: &Interner) -> Vec<String> {
    trivia
        .iter()
        .filter_map(|item| match item {
            HirTrivia::CommentLine { text, .. } => Some(interner.resolve(*text).to_owned()),
            HirTrivia::Newline { .. } => None,
        })
        .collect()
}

fn attachment_trivia<'a>(presentation: &'a HirPresentation, anchor: &HirSourceAnchorId) -> &'a [HirTrivia] {
    presentation
        .attachments
        .iter()
        .find(|attachment| &attachment.anchor == anchor)
        .map_or(&[], |attachment| attachment.leading.as_slice())
}

fn owner_for<'a>(
    presentation: &'a HirPresentation,
    anchor: &HirSourceAnchorId,
) -> Option<&'a radix_module::hir::HirTriviaOwner> {
    presentation.owners.iter().find(|owner| &owner.id == anchor)
}

fn function_body(hir: &HirModule) -> &HirBlock {
    let item = hir
        .items
        .iter()
        .find(|item| matches!(item.kind, HirItemKind::Function(_)))
        .expect("expected a function item");
    match &item.kind {
        HirItemKind::Function(func) => func.body.as_ref().expect("expected a function body"),
        _ => unreachable!(),
    }
}

fn struct_item(hir: &HirModule) -> &radix_module::hir::HirStruct {
    let item = hir
        .items
        .iter()
        .find(|item| matches!(item.kind, HirItemKind::Struct(_)))
        .expect("expected a genus item");
    match &item.kind {
        HirItemKind::Struct(struct_item) => struct_item,
        _ => unreachable!(),
    }
}

// =============================================================================
// Statement / item / member / entry owners
// =============================================================================

#[test]
fn comment_before_block_statement_attaches_to_statement_owner() {
    let source = "functio f() → vacuum {\n  # before statement\n  nota \"hi\"\n}\n";
    let (lowered, errors, interner) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;

    let owners: Vec<_> = presentation
        .owners
        .iter()
        .filter(|owner| owner.kind == HirTriviaOwnerKind::Statement)
        .collect();
    assert_eq!(owners.len(), 1, "one statement owner expected: {owners:?}");
    let owner = owners[0];

    assert_eq!(
        comment_texts(attachment_trivia(presentation, &owner.id), &interner),
        vec![" before statement"]
    );

    // The owner back-references the rendered `nota` statement.
    let block = function_body(&lowered.hir);
    let HirStatement { id, .. } = &block.statements[0];
    assert_eq!(owner.semantic, Some(HirOwnerRef::Hir(*id)));
}

#[test]
fn comment_before_top_level_item_attaches_to_item_owner() {
    let source = "# before item\nfunctio f() → vacuum { }\n";
    let (lowered, errors, interner) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;

    let owners: Vec<_> = presentation
        .owners
        .iter()
        .filter(|owner| owner.kind == HirTriviaOwnerKind::Item)
        .collect();
    assert_eq!(owners.len(), 1, "one item owner expected: {owners:?}");
    let owner = owners[0];

    assert_eq!(
        comment_texts(attachment_trivia(presentation, &owner.id), &interner),
        vec![" before item"]
    );

    // The owner back-references the rendered function item.
    let item_id = lowered
        .hir
        .items
        .iter()
        .find(|item| matches!(item.kind, HirItemKind::Function(_)))
        .expect("function item")
        .id;
    assert_eq!(owner.semantic, Some(HirOwnerRef::Hir(item_id)));
}

#[test]
fn comment_before_genus_field_attaches_to_member_owner() {
    let source = "genus Persona {\n  # before field\n  varia textus nomen\n}\n";
    let (lowered, errors, interner) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;

    let owners: Vec<_> = presentation
        .owners
        .iter()
        .filter(|owner| owner.kind == HirTriviaOwnerKind::Member)
        .collect();
    assert_eq!(owners.len(), 1, "one member owner expected: {owners:?}");
    let owner = owners[0];

    assert_eq!(
        comment_texts(attachment_trivia(presentation, &owner.id), &interner),
        vec![" before field"]
    );

    // The owner back-references the field's own def id.
    let struct_item = struct_item(&lowered.hir);
    assert_eq!(owner.semantic, Some(HirOwnerRef::Def(struct_item.fields[0].def_id)));
}

#[test]
fn comment_before_genus_method_attaches_to_member_owner() {
    let source = "genus Persona {\n  # before method\n  functio saluta() → vacuum { }\n}\n";
    let (lowered, errors, interner) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;

    let owners: Vec<_> = presentation
        .owners
        .iter()
        .filter(|owner| owner.kind == HirTriviaOwnerKind::Member)
        .collect();
    assert_eq!(owners.len(), 1, "one member owner expected: {owners:?}");
    let owner = owners[0];

    assert_eq!(
        comment_texts(attachment_trivia(presentation, &owner.id), &interner),
        vec![" before method"]
    );

    // The owner back-references the method's own def id.
    let struct_item = struct_item(&lowered.hir);
    assert_eq!(owner.semantic, Some(HirOwnerRef::Def(struct_item.methods[0].def_id)));
}

#[test]
fn comment_before_implendum_method_attaches_to_member_owner() {
    let source = "implendum Forma {\n  # before method\n  functio ping() → vacuum\n}\n";
    let (lowered, errors, interner) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;

    let owners: Vec<_> = presentation
        .owners
        .iter()
        .filter(|owner| owner.kind == HirTriviaOwnerKind::Member)
        .collect();
    assert_eq!(owners.len(), 1, "one member owner expected: {owners:?}");
    let owner = owners[0];

    assert_eq!(
        comment_texts(attachment_trivia(presentation, &owner.id), &interner),
        vec![" before method"]
    );

    // Interface signatures have semantic definition ids, even though they
    // carry no body node. The trivia owner refers to that method definition.
    let interface = lowered
        .hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Interface(interface) => Some(interface),
            _ => None,
        })
        .expect("interface item");
    assert_eq!(owner.semantic, Some(HirOwnerRef::Def(interface.methods[0].def_id)));
}

#[test]
fn comment_before_explicit_entry_attaches_to_entry_owner() {
    let source = "# before entry\nincipit {\n  nota 1\n}\n";
    let (lowered, errors, interner) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;

    let owners: Vec<_> = presentation
        .owners
        .iter()
        .filter(|owner| owner.kind == HirTriviaOwnerKind::Entry)
        .collect();
    assert_eq!(owners.len(), 1, "one entry owner expected: {owners:?}");
    let owner = owners[0];

    assert_eq!(
        comment_texts(attachment_trivia(presentation, &owner.id), &interner),
        vec![" before entry"]
    );

    // The entry-start anchor points at the entry owner.
    assert_eq!(presentation.entry_start_anchor, Some(owner.id.clone()));
}

// =============================================================================
// Structural ends
// =============================================================================

#[test]
fn comment_before_block_close_attaches_to_block_end_owner() {
    let source = "functio f() → vacuum {\n  nota 1\n  # before close\n}\n";
    let (lowered, errors, interner) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;

    // The authored function body owns exactly one structural end, recorded
    // both on the block and in the presentation block_ends list.
    let body = function_body(&lowered.hir);
    let end = body.end.as_ref().expect("authored block end");
    assert_eq!(presentation.block_ends.len(), 1, "{:?}", presentation.block_ends);
    assert_eq!(presentation.block_ends[0].id, end.id);

    // The end's comment is attached under the BlockEnd owner.
    let end_owner = owner_for(presentation, &end.id)
        .filter(|owner| owner.kind == HirTriviaOwnerKind::BlockEnd)
        .expect("block-end owner");
    assert_eq!(
        comment_texts(attachment_trivia(presentation, &end_owner.id), &interner),
        vec![" before close"]
    );
}

#[test]
fn comment_before_eof_attaches_to_program_end_owner() {
    let source = "functio f() → vacuum { }\n# trailing\n";
    let (lowered, errors, interner) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;

    let program_end = &presentation.program_end;
    let end_owner = owner_for(presentation, &program_end.id)
        .filter(|owner| owner.kind == HirTriviaOwnerKind::ProgramEnd)
        .expect("program-end owner");
    assert_eq!(
        comment_texts(attachment_trivia(presentation, &end_owner.id), &interner),
        vec![" trailing"]
    );
}

#[test]
fn comment_only_file_attaches_to_program_end_owner() {
    let source = "# only comment\n";
    let (lowered, errors, interner) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;
    assert!(lowered.hir.items.is_empty());

    let program_end = &presentation.program_end;
    let end_owner = owner_for(presentation, &program_end.id)
        .filter(|owner| owner.kind == HirTriviaOwnerKind::ProgramEnd)
        .expect("program-end owner");
    assert_eq!(
        comment_texts(attachment_trivia(presentation, &end_owner.id), &interner),
        vec![" only comment"]
    );
}

#[test]
fn synthesized_entry_block_carries_no_structural_end() {
    // Top-level executable statements form an implicit entry block that the
    // compiler synthesizes; it must not impersonate an authored boundary.
    let source = "nota 42\n";
    let (lowered, errors, _) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;

    assert_eq!(presentation.block_ends.len(), 0, "{:?}", presentation.block_ends);
    let entry = lowered.hir.entry.as_ref().expect("implicit entry block");
    assert!(entry.end.is_none(), "synthesized entry block must not carry an authored end");
}

// =============================================================================
// Unnamed point (parser diagnostic via U-1) and container-close attachment
// =============================================================================

#[test]
fn comment_before_genus_close_attaches_as_class_end_trivia() {
    // MT-U2 (parser 0976d628a): a full-line comment before a genus's closing
    // `}` is a legal attachment point — the parser retains it as
    // `ClassDecl.end_trivia` (mirroring `BlockStmt::end_trivia`), so the
    // frontend accepts it instead of failing with PARSE060
    // invalid_comment_placement. U-1 invalid placement still applies to
    // genuinely unnamed points and is covered parser-side (e.g. a comment
    // between annotations).
    let source = "genus Canis {\n  varia textus nomen\n  # before close\n}\n";
    let lex_result = radix_module::lexer::lex(source);
    let parse_result = radix_module::parser::parse(lex_result);
    assert!(parse_result.success(), "expected parse success: {:?}", parse_result.errors);
    assert!(
        !parse_result
            .errors
            .iter()
            .any(|err| err.kind == radix_module::parser::ParseErrorKind::CommentPlacementInvalid),
        "genus-close comment is a legal ClassDecl.end_trivia attachment"
    );

    let program = parse_result.program.expect("expected parsed program");
    let radix_module::syntax::StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected genus declaration");
    };
    let comments: Vec<String> = class
        .end_trivia
        .iter()
        .filter_map(|item| match item {
            radix_module::syntax::Trivia::CommentLine(comment) => {
                Some(parse_result.interner.resolve(comment.text).to_owned())
            }
            radix_module::syntax::Trivia::Newline(_) => None,
        })
        .collect();
    assert_eq!(comments, vec![" before close"], "comment must attach at the genus close");
    assert_eq!(class.members.len(), 1);

    // The container still lowers cleanly: no errors and one item. The comment
    // is retained at the syntax boundary (the HIR presentation has no
    // genus-close end owner yet) and is never mis-attached or diagnosed.
    let (lowered, errors, _interner) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    assert_eq!(lowered.hir.items.len(), 1);
}

// =============================================================================
// Expansion and zero-output rules
// =============================================================================

#[test]
fn expanded_statement_attaches_once_to_first_rendered_node() {
    let source = "functio f() → vacuum {\n  # destructure\n  sit [a, b] ← [1, 2]\n  tacet\n}\n";
    let (lowered, errors, interner) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;

    // The array destructure renders the once-evaluated subject local and two
    // element locals (D14.2); the block also holds the trailing `tacet`.
    let body = function_body(&lowered.hir);
    assert_eq!(body.statements.len(), 4, "subject local + two destructure locals + tacet");
    let first_id = body.statements[0].id;
    let second_id = body.statements[1].id;

    // Exactly one attachment carries the destructure comment...
    let destructure_attachments: Vec<_> = presentation
        .attachments
        .iter()
        .filter(|attachment| {
            comment_texts(&attachment.leading, &interner)
                .iter()
                .any(|comment| comment == " destructure")
        })
        .collect();
    assert_eq!(
        destructure_attachments.len(),
        1,
        "the comment must attach exactly once: {destructure_attachments:?}"
    );

    // ...to the first rendered node.
    let owner = owner_for(presentation, &destructure_attachments[0].anchor).expect("owner");
    assert_eq!(owner.kind, HirTriviaOwnerKind::Statement);
    assert_eq!(owner.semantic, Some(HirOwnerRef::Hir(first_id)));

    // The second rendered statement has no owner pointing at it.
    assert!(
        presentation
            .owners
            .iter()
            .all(|owner| owner.semantic != Some(HirOwnerRef::Hir(second_id))),
        "expansion must attach once, to the first rendered node only"
    );
}

#[test]
fn comment_on_zero_output_construct_diagnoses() {
    // `sit [a, b]` without an initializer parses but lowers to zero
    // rendered statements. A leading comment then has no owner: the lowerer
    // must diagnose rather than drop or guess a fallback.
    let source = "functio f() → vacuum {\n  # orphaned\n  sit [a, b]\n}\n";
    let (_, errors, _) = lower_source(source);
    assert!(
        errors.iter().any(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "comment_without_owner")
        }),
        "expected zero-output comment diagnosis: {errors:?}"
    );
}

// =============================================================================
// Partitioning and nesting
// =============================================================================

#[test]
fn partitioning_preserves_attachments() {
    // Mixed top-level items and executable statements: the item comment and
    // the implicit-entry statement comment both survive declaration/entry
    // partitioning.
    let source = "# item comment\nfunctio f() → vacuum { }\n# stmt comment\nnota 42\n";
    let (lowered, errors, interner) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;

    let item_comments: Vec<Vec<String>> = presentation
        .owners
        .iter()
        .filter(|owner| owner.kind == HirTriviaOwnerKind::Item)
        .map(|owner| comment_texts(attachment_trivia(presentation, &owner.id), &interner))
        .collect();
    assert!(item_comments.contains(&vec![" item comment".to_owned()]), "{item_comments:?}");

    let stmt_comments: Vec<Vec<String>> = presentation
        .owners
        .iter()
        .filter(|owner| owner.kind == HirTriviaOwnerKind::Statement)
        .map(|owner| comment_texts(attachment_trivia(presentation, &owner.id), &interner))
        .collect();
    assert!(stmt_comments.contains(&vec![" stmt comment".to_owned()]), "{stmt_comments:?}");
}

#[test]
fn nested_control_flow_blocks_retain_their_end_comments() {
    let source =
        "functio f(numerus n) → vacuum {\n  si n ≺ 0 {\n    nota \"neg\"\n    # inner end\n  }\n  # outer end\n}\n";
    let (lowered, errors, interner) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;

    // Function body + `si` then-block each own a structural end.
    assert_eq!(presentation.block_ends.len(), 2, "{:?}", presentation.block_ends);

    let end_comments: Vec<Vec<String>> = presentation
        .owners
        .iter()
        .filter(|owner| owner.kind == HirTriviaOwnerKind::BlockEnd)
        .map(|owner| comment_texts(attachment_trivia(presentation, &owner.id), &interner))
        .collect();
    assert_eq!(end_comments.len(), 2, "{end_comments:?}");
    assert!(
        end_comments.contains(&vec![" inner end".to_owned()]),
        "nested block must keep its own end comment: {end_comments:?}"
    );
    assert!(end_comments.contains(&vec![" outer end".to_owned()]), "{end_comments:?}");
}

#[test]
fn authored_executable_blocks_carry_structural_ends() {
    // A function body plus a nested `si` brace block: every authored
    // executable block owns one structural end recorded in the sidecar.
    let source = "functio f() → vacuum {\n  si verum {\n    tacet\n  }\n}\n";
    let (lowered, errors, _) = lower_source(source);
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    let presentation = &lowered.presentation;

    assert_eq!(presentation.block_ends.len(), 2, "{:?}", presentation.block_ends);
    let body = function_body(&lowered.hir);
    assert!(body.end.is_some(), "authored function body must carry an end");
}
