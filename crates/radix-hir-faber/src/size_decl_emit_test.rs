//! `const size` declarations survive a Faber format round trip byte-identically.
//!
//! `SizeDecl` lowers to a `size_decl`-flagged `HirLocal`; the emitter must print
//! the ruled modifier-first spelling (`const size` en / `const magnitudo` la)
//! with `←` for the runtime lift and `=` for the static natural, not an
//! inferred immutable binding.

use super::*;
use crate::FaberCodegen;
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    HirBlock, HirExpression, HirExpressionKind, HirId, HirLiteral, HirLocal, HirModule,
    HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, KeywordPack, Span, TokenKind};
use radix_syntax::StmtKind;
use radix_types::{DefId, TypeTable};

/// English reader surface (`en`): the pack's `size`/`const` spellings.
struct EnSurface;

impl KeywordSurface for EnSurface {
    fn keyword(&self, kind: TokenKind) -> &str {
        match kind {
            TokenKind::Magnitudo => "size",
            TokenKind::Incipit => "incipit",
            _ => panic!("unexpected keyword: {kind:?}"),
        }
    }

    fn contextual_keyword(&self, key: &'static str) -> &'static str {
        match key {
            "fixum" => "const",
            _ => key,
        }
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "numerus"
    }
}

/// Minimal `en` pack for re-parsing the emitted English form.
struct EnPack;

impl KeywordPack for EnPack {
    fn pack_id(&self) -> &'static str {
        "en-size-decl"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&'static str> {
        match text {
            "size" => Some("magnitudo"),
            "const" => Some("fixum"),
            "let" => Some("sit"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn size_decl_module(interner: &mut Interner, name: &str, size_static: bool) -> HirModule {
    let local = HirLocal {
        def_id: DefId(1),
        name: interner.intern(name),
        ty: None,
        init: Some(HirExpression {
            id: HirId(2),
            kind: HirExpressionKind::Literal(HirLiteral::Int(3)),
            ty: None,
            span: Span::default(),
        }),
        mutable: false,
        is_await: false,
        runtime_provided: false,
        await_store: None,
        size_decl: true,
        size_static,
    };
    HirModule {
        items: Vec::new(),
        entry: Some(HirBlock::simple(
            vec![HirStatement {
                id: HirId(3),
                kind: HirStatementKind::Local(local),
                span: Span::default(),
            }],
            None,
            Span::default(),
        )),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

fn emit_statement(hir: &HirModule, interner: &Interner, types: &TypeTable) -> String {
    let HirStatementKind::Local(local) = &hir.entry.as_ref().expect("entry").statements[0].kind
    else {
        panic!("expected a local statement");
    };
    let mut writer = CodeWriter::new();
    FaberCodegen::new(types, interner).write_stmt(
        &HirStatement {
            id: HirId(3),
            kind: HirStatementKind::Local(local.clone()),
            span: Span::default(),
        },
        &FxHashMap::default(),
        &EnSurface,
        &mut writer,
    );
    writer.finish()
}

#[test]
fn size_decl_round_trips_through_faber_spelling() {
    // Runtime lift keeps the `←` capture glyph.
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let hir = size_decl_module(&mut interner, "H", false);
    let code = emit_statement(&hir, &interner, &types);
    assert_eq!(code.trim_end(), "const size H ← 3", "{code}");

    // Static natural uses the compile-time `=` glyph per the glyph law.
    let mut interner = Interner::new();
    let hir = size_decl_module(&mut interner, "N", true);
    let code = emit_statement(&hir, &interner, &types);
    assert_eq!(code.trim_end(), "const size N = 3", "{code}");

    // The emitted spellings re-parse (en pack) as size declarations.
    for expected in ["const size H ← 3", "const size N = 3"] {
        let source = format!("{{ {expected} }}");
        let parsed = radix_parser::parse(radix_lexer::lex_with_locale_pack(&source, &EnPack));
        assert!(
            parsed.success(),
            "emitted size form must re-parse, got:\n{}\nerrors: {:?}",
            source,
            parsed.errors
        );
        let program = parsed.program.expect("program");
        let StmtKind::Block(body) = &program.statements[0].kind else {
            panic!("expected a block statement, got: {:?}", program.statements);
        };
        assert_eq!(body.statements.len(), 1, "{source}");
        assert!(
            matches!(body.statements[0].kind, StmtKind::SizeDecl(_)),
            "emitted form must re-parse as SizeDecl, got: {:?}",
            body.statements[0].kind
        );
    }
}
