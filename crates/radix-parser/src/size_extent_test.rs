//! Unit tests for the spelled size lift in figura extent positions.

use super::{ParseResult, parse};
use radix_lexer::{KeywordPack, lex_with_locale_pack};
use radix_syntax::{FiguraExpr, LiftExtent, StmtKind, TypeExprKind};

struct EnSizePack;

impl KeywordPack for EnSizePack {
    fn pack_id(&self) -> &'static str {
        "en-size-extent-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<radix_lexer::TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "size" => Some("magnitudo"),
            "const" => Some("fixum"),
            "let" => Some("sit"),
            "mut" => Some("varia"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_source(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &EnSizePack))
}

#[test]
fn spelled_lift_extent_parses_as_lift_shape() {
    let result = parse_source("mut tf32[2, size n] a ← tf32[2, size n] {}");
    assert!(result.success(), "{:?}", result.errors);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected var declaration");
    };
    let Some(ty) = &decl.ty else {
        panic!("expected annotated type");
    };
    let TypeExprKind::Tensor(_, figura) = &ty.kind else {
        panic!("expected tensor type with extents");
    };
    let FiguraExpr::Tuple(dims) = figura else {
        panic!("expected tuple shape");
    };
    assert!(matches!(dims[0], FiguraExpr::Literal(2)));
    let FiguraExpr::Lift(extent) = &dims[1] else {
        panic!("expected spelled lift in second extent");
    };
    assert!(matches!(&**extent, LiftExtent::Name(_)));
}

#[test]
fn static_spelled_lift_literal_parses() {
    let result = parse_source("mut tf32[2, size 3] a ← tf32[2, size 3] {}");
    assert!(result.success(), "{:?}", result.errors);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected var declaration");
    };
    let Some(ty) = &decl.ty else {
        panic!("expected annotated type");
    };
    let TypeExprKind::Tensor(_, figura) = &ty.kind else {
        panic!("expected tensor type with extents");
    };
    let FiguraExpr::Tuple(dims) = figura else {
        panic!("expected tuple shape");
    };
    let FiguraExpr::Lift(extent) = &dims[1] else {
        panic!("expected spelled lift in second extent");
    };
    assert!(matches!(&**extent, LiftExtent::Literal(3)));
}

#[test]
fn construct_type_expression_parses_at_store_rhs() {
    let result = parse_source("const size N = 2\nlet tf32[2, N] a ← tf32[2, N] {}");
    assert!(result.success(), "{:?}", result.errors);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[1].kind else {
        panic!("expected const binding");
    };
    let init = decl.init.as_ref().expect("initializer");
    assert!(
        matches!(init.kind, radix_syntax::ExprKind::Construct(_)),
        "got {:?}",
        init.kind
    );
}

#[test]
fn construct_type_expression_refuses_argument_position() {
    let result = parse_source("let x ← f(tf32[2, 3])");
    let error = result.errors.first().expect("expected parser refusal");
    assert!(
        error
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "constructor_store_position_only"),
        "got {error:?}"
    );
}
