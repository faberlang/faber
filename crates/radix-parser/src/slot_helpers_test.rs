//! Unit tests for the dual-mode grammar-slot keyword helpers.
//!
//! A grammar slot accepts a keyword in either the legacy reserved-token form
//! (`TokenKind::Iuncta`) or the tokenless surface form (`Ident` matching the
//! Latin key, or a reader-pack surface spelling resolving to it). These tests
//! pin both modes directly on the helpers, independent of any dispatch site,
//! so the token→spelling migration cannot silently break slot recognition.

use super::{ParseOptions, Parser, parse};
use radix_lexer::{KeywordPack, LexResult, TokenKind, lex, lex_with_locale_pack};
use radix_syntax::{BindingPattern, ExprKind, StmtKind};

fn parse_ok_with_slot_pack(source: &str) -> super::ParseResult {
    let result = parse(lex_with_locale_pack(source, &SlotTestPack));
    assert!(
        result.success(),
        "expected parse success under slot-test pack, got errors: {:?}",
        result.errors
    );
    result
}

/// Minimal reader pack: localizes `iuncta` to `本地` and `tacebit` to `await`
/// as tokenless keywords (no token emission), with no Latin fallback declared.
struct SlotTestPack;

impl KeywordPack for SlotTestPack {
    fn pack_id(&self) -> &'static str {
        "slot-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "本地" => Some("iuncta"),
            "await" => Some("tacebit"),
            // Genus, statements, and literals let class-body regression tests
            // exercise localized type spellings and statement terminals end to
            // end.
            "genus" => Some("genus"),
            "fn" => Some("functio"),
            "mut" => Some("varia"),
            "if" => Some("si"),
            "then" => Some("ergo"),
            "return" => Some("redde"),
            "true" => Some("verum"),
            "false" => Some("falsum"),
            "print" => Some("nota"),
            "read" => Some("lege"),
            _ => None,
        }
    }

    fn type_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "bool" => Some("bivalens"),
            "int" => Some("numerus"),
            "string" => Some("textus"),
            "void" => Some("vacuum"),
            "list" => Some("lista"),
            _ => None,
        }
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

/// Minimal pack-authoritative surface for the queue/stack sealing pin. Its
/// ordinary type rows are localized, while `queue` remains a canonical Latin
/// type spelling supplied by the explicit `la` fallback policy.
struct QueueFallbackPack {
    has_latin_fallback: bool,
}

impl KeywordPack for QueueFallbackPack {
    fn pack_id(&self) -> &'static str {
        "queue-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "fn" => Some("functio"),
            "return" => Some("redde"),
            _ => None,
        }
    }

    fn type_latin_key(&self, text: &str) -> Option<&str> {
        (text == "int").then_some("numerus")
    }

    fn has_latin_fallback(&self) -> bool {
        self.has_latin_fallback
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

/// Build a parser over `source` on the plain Latin surface (no reader pack).
fn parser_over(source: &str) -> Parser {
    let lexed = lex(source);
    Parser::new(lexed.tokens, lexed.interner, ParseOptions::default())
}

/// Build a parser over `source`, keeping the reader-pack identity maps the
/// lexer recorded (the same wiring `parse_with_options` uses).
fn parser_over_with_pack(source: &str) -> Parser {
    let LexResult {
        tokens,
        reader_keyword_latin_keys,
        reader_keyword_surfaces,
        reader_type_latin_keys,
        reader_type_surfaces,
        keyword_surface,
        interner,
        ..
    } = lex_with_locale_pack(source, &SlotTestPack);
    Parser::new_with_reader_identities(
        tokens,
        interner,
        ParseOptions::default(),
        reader_keyword_latin_keys,
        reader_keyword_surfaces,
        reader_type_latin_keys,
        reader_type_surfaces,
        keyword_surface,
    )
}

#[test]
fn latin_fallback_allows_queue_and_stack_but_nonfallback_seals_them() {
    let source = "fn f(queue<int> q, stack<int> s) → stack<int> { return s }";

    let fallback = parse(lex_with_locale_pack(
        source,
        &QueueFallbackPack {
            has_latin_fallback: true,
        },
    ));
    assert!(
        fallback.success(),
        "la fallback must retain canonical queue spelling: {:?}",
        fallback.errors
    );

    let sealed = parse(lex_with_locale_pack(
        source,
        &QueueFallbackPack {
            has_latin_fallback: false,
        },
    ));
    assert!(
        sealed.errors.iter().any(|error| {
            error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "wrong_locale_type")
        }),
        "non-fallback pack must seal canonical queue spelling: {:?}",
        sealed.errors
    );
}

#[test]
fn slot_helper_matches_legacy_reserved_token_form() {
    let mut parser = parser_over("iuncta");
    assert!(parser.check_keyword_ident(TokenKind::Iuncta));
    assert!(!parser.check_keyword_ident(TokenKind::Modulus));
    assert!(parser.eat_keyword_ident(TokenKind::Iuncta));
    assert!(
        !parser.check_keyword_ident(TokenKind::Iuncta),
        "consumed token must not re-match"
    );
}

#[test]
fn slot_helper_leaves_undeclared_latin_spelling_an_identifier_under_pack() {
    // The pack does not localize `iuncta`, so it lexes as an `Ident` with no
    // recorded latin key. Under a non-`la` pack the parser is
    // pack-authoritative: an undeclared spelling stays an ordinary identifier
    // and cannot claim the `iuncta` grammar slot (S2-U2). The registry-direct
    // (`la`/no-pack) claim of the same spelling is pinned by
    // `slot_helper_matches_legacy_reserved_token_form`.
    let mut parser = parser_over_with_pack("iuncta");
    assert!(!parser.check_keyword_ident(TokenKind::Iuncta));
    assert!(!parser.eat_keyword_ident(TokenKind::Iuncta));
}

#[test]
fn slot_helper_matches_localized_pack_spelling() {
    let mut parser = parser_over_with_pack("本地");
    assert!(parser.check_keyword_ident(TokenKind::Iuncta));
    assert!(!parser.check_keyword_ident(TokenKind::Modulus));
    assert!(parser.eat_keyword_ident(TokenKind::Iuncta));
}

#[test]
fn slot_helper_matches_tokenless_precedent_via_pack_key() {
    // Existing tokenless precedent: `await` → `tacebit` through the pack map.
    let mut parser = parser_over_with_pack("await");
    assert!(parser.check_keyword_ident(TokenKind::Tacebit));
    assert!(!parser.check_keyword_ident(TokenKind::Iuncta));
    assert!(parser.eat_keyword_ident(TokenKind::Tacebit));
}

#[test]
fn slot_helper_rejects_ordinary_identifier() {
    let mut parser = parser_over("nomine");
    assert!(!parser.check_keyword_ident(TokenKind::Iuncta));
    assert!(!parser.eat_keyword_ident(TokenKind::Iuncta));
}

#[test]
fn slot_helper_expect_errors_when_absent() {
    let mut parser = parser_over("nomine");
    let result = parser.expect_keyword_ident(TokenKind::Iuncta);
    let err = result.expect_err("missing slot keyword must produce an error");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "expected_keyword"),
        "missing slot keyword must carry the expected_keyword issue"
    );
}

#[test]
fn name_slots_accept_keyword_spellings_on_en_pack_surface() {
    // Green guards: declaration and parameter slots keep surface spellings
    // (`print`/`read`/`nota`) under a pack that maps print→nota, read→lege.
    for source in [
        "fn read(string read) → void {}\n",
        "fn print(string print) → void {}\n",
        "fn nota(string nota) → void {}\n",
    ] {
        let result = parse_ok_with_slot_pack(source);
        let program = result.program.as_ref().expect("program");
        let StmtKind::Func(decl) = &program.statements[0].kind else {
            panic!("expected function declaration for {source}");
        };
        let expected = if source.contains("read") {
            "read"
        } else if source.contains("print") {
            "print"
        } else {
            "nota"
        };
        let name = result.interner.resolve(decl.name.name);
        let param = result.interner.resolve(decl.params[0].name.name);
        assert_eq!(
            name, expected,
            "function name surface spelling for {source}"
        );
        assert_eq!(param, expected, "parameter surface spelling for {source}");
    }

    let bind = parse_ok_with_slot_pack("mut string read ← \"data\"\n");
    let program = bind.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let BindingPattern::Ident(name) = &var.binding else {
        panic!("expected identifier binding");
    };
    assert_eq!(bind.interner.resolve(name.name), "read");
}

#[test]
fn localized_type_spelling_after_initialized_field_is_not_an_operand() {
    // Regression: the field-boundary guard matched type spellings against the
    // latin conversio-type list by raw source text. Under a pack that
    // localizes `textus` → `string`, the guard missed the second field's type
    // and the retired `true`-prefix predicate check fired (PARSE001) on
    // `bool visible = true` followed by `string name`.
    let mut parser =
        parser_over_with_pack("genus X {\n    mut bool visible = true\n    mut string name\n}\n");
    let result = parser.parse_program();
    assert!(
        result.errors.is_empty(),
        "localized type spelling after an initialized field must not fire the retired prefix predicate: {:?}",
        result.errors
    );
}

#[test]
fn statement_terminal_literal_is_not_a_retired_predicate() {
    // Regression: `return false` followed by the next statement's identifier
    // (`j ← j + 1`) read as the retired `falsum <operand>` prefix form and
    // fired PARSE001. A literal in operand position is a value; only a
    // statement-start literal followed by an operand is the retired form.
    let mut parser = parser_over_with_pack(
        "genus X {\n    fn f() → bool {\n        mut int j ← 0\n        if j ≡ 1 then return false\n        j ← j + 1\n        return true\n    }\n}\n",
    );
    let result = parser.parse_program();
    assert!(
        result.errors.is_empty(),
        "statement-terminal literal must not fire the retired prefix predicate: {:?}",
        result.errors
    );

    // The retired standalone form must still be rejected.
    let mut parser =
        parser_over_with_pack("genus X {\n    fn f() → bool {\n        false j\n    }\n}\n");
    let result = parser.parse_program();
    assert!(
        result.errors.iter().any(|error| {
            matches!(error.kind, super::ParseErrorKind::Expected)
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "retired_prefix_predicate")
        }),
        "standalone `false <operand>` must still be rejected: {:?}",
        result.errors
    );
}

#[test]
fn vacua_claim_is_surface_gated() {
    // Registry-direct (`la`/no-pack): the undeclared Latin `vacua` claims the
    // empty-collection marker through the raw-source fallback (the pack-less
    // lexer records no key for the tokenless `vacua`).
    let mut parser = parser_over("varia lista words ← vacua");
    let result = parser.parse_program();
    assert!(
        result.errors.is_empty(),
        "la `vacua` must parse: {:?}",
        result.errors
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let init = var.init.as_deref().expect("initializer");
    assert!(
        matches!(init.kind, ExprKind::Vacua(_)),
        "registry-direct surface must claim the `vacua` marker"
    );

    // Pack-authoritative (non-`la` pack): the pack does not declare `vacua`,
    // so the undeclared Latin spelling is an ordinary identifier expression
    // and must not claim the empty-collection marker.
    let mut parser = parser_over_with_pack("mut list words ← vacua");
    let result = parser.parse_program();
    assert!(
        result.errors.is_empty(),
        "pack `vacua` must parse as an ordinary identifier: {:?}",
        result.errors
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let init = var.init.as_deref().expect("initializer");
    assert!(
        matches!(init.kind, ExprKind::Ident(_)),
        "undeclared Latin `vacua` must not claim the marker under a non-la pack"
    );
}

/// KW-U1: en `print(print)` is a user call; bare `print print` stays the output statement.
#[test]
fn en_print_paren_form_is_user_call_not_scribe() {
    let mut parser =
        parser_over_with_pack("fn print(string print) { print print }\nprint(print)\n");
    let result = parser.parse_program();
    assert!(
        result.errors.is_empty(),
        "en print fixture must parse: {:?}",
        result.errors
    );
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 2);

    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!(
            "expected function declaration, got {:?}",
            program.statements[0].kind
        );
    };
    let body = func.body.as_ref().expect("function body");
    assert!(
        matches!(body.statements[0].kind, StmtKind::Scribe(_)),
        "bare `print print` stays the output statement, got {:?}",
        body.statements[0].kind
    );

    let StmtKind::Expr(expr_stmt) = &program.statements[1].kind else {
        panic!(
            "expected expression statement, got {:?}",
            program.statements[1].kind
        );
    };
    let ExprKind::Call(call) = &expr_stmt.expr.kind else {
        panic!("expected call, got {:?}", expr_stmt.expr.kind);
    };
    let ExprKind::Ident(ident) = &call.callee.kind else {
        panic!("expected ident callee, got {:?}", call.callee.kind);
    };
    assert_eq!(result.interner.resolve(ident.name), "print");
}
