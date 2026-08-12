//! Parser tests for the relational-view `schema` / `columna` grammar
//! (census-types S2-U1; docs/design/relational-views.md §2 + §7,
//! stage0/fixture-contract.md §7, stage0/decision-source-column-mapping.md §1.4).
//!
//! The grammar surface: `schema Name { columna<T> publicName [: sourceName] … }`,
//! where `T` reuses the ordinary type parser (`T ∪ nihil` spells nullable
//! columns) and `publicName : sourceName` is the D4 alias row with the existing
//! `Colon` token as the separator.

use crate::test_support::{
    assert_parse_error_issue, assert_parse_error_matches, parse_ok, parse_program, symbol_name,
    ParseExpectation,
};
use crate::{ParseErrorKind, ParseOptions, Parser};
use radix_lexer::{lex, Span, Token, TokenKind};
use radix_syntax::{StmtKind, Trivia, TypeExprKind};

/// Owned summary of one parsed schema column, so tests compare content instead
/// of reaching through interner symbols.
#[derive(Debug)]
struct ColumnSummary {
    name: String,
    source_alias: Option<String>,
    nullable: bool,
    type_name: String,
}

/// Parse `source`, require exactly one `schema` statement, and summarize it.
fn parse_schema_columns(source: &str) -> (String, Vec<ColumnSummary>) {
    let result = parse_ok(source);
    let program = result.program.as_ref().expect("program");
    assert_eq!(
        program.statements.len(),
        1,
        "exactly one schema declaration expected"
    );
    let StmtKind::Schema(schema) = &program.statements[0].kind else {
        panic!("expected schema declaration");
    };
    let name = symbol_name(&result, schema.name.name).to_owned();
    let columns = schema
        .columns
        .iter()
        .map(|column| ColumnSummary {
            name: symbol_name(&result, column.name.name).to_owned(),
            source_alias: column
                .source_alias
                .as_ref()
                .map(|alias| symbol_name(&result, alias.name).to_owned()),
            nullable: matches!(column.ty.kind, TypeExprKind::Union(_)),
            type_name: match &column.ty.kind {
                TypeExprKind::Named(ident, _) => symbol_name(&result, ident.name).to_owned(),
                TypeExprKind::Union(_) => "union".to_owned(),
                other => format!("{other:?}"),
            },
        })
        .collect();
    (name, columns)
}

// =============================================================================
// Parse-positive
// =============================================================================

#[test]
fn parses_schema_with_required_columns() {
    let (name, columns) = parse_schema_columns(
        "schema PersonaQuaerenda {
            columna numerus id
            columna textus nomen : plenum
            columna bivalens activa : statim
        }",
    );
    assert_eq!(name, "PersonaQuaerenda");
    assert_eq!(columns.len(), 3);

    // Identity mapping (no alias row).
    assert_eq!(columns[0].name, "id");
    assert_eq!(columns[0].source_alias, None);
    assert_eq!(columns[0].type_name, "numerus");
    assert!(!columns[0].nullable);

    // Visible-column rename (alias row).
    assert_eq!(columns[1].name, "nomen");
    assert_eq!(columns[1].source_alias.as_deref(), Some("plenum"));
    assert_eq!(columns[1].type_name, "textus");

    // Source-only predicate-column rename.
    assert_eq!(columns[2].name, "activa");
    assert_eq!(columns[2].source_alias.as_deref(), Some("statim"));
    assert_eq!(columns[2].type_name, "bivalens");
}

#[test]
fn parses_nullable_columns_through_union_nihil() {
    let (_, columns) = parse_schema_columns(
        "schema PersonaAmpla {
            columna numerus id
            columna textus ∪ nihil email : electronica
            columna instans ∪ nihil created_at : creata
        }",
    );
    assert_eq!(columns.len(), 3);
    assert_eq!(columns[0].type_name, "numerus");
    assert!(!columns[0].nullable);
    assert_eq!(columns[1].type_name, "union");
    assert!(columns[1].nullable);
    assert_eq!(columns[1].name, "email");
    assert_eq!(columns[1].source_alias.as_deref(), Some("electronica"));
    assert!(columns[2].nullable);
    assert_eq!(columns[2].name, "created_at");
    assert_eq!(columns[2].source_alias.as_deref(), Some("creata"));
}

#[test]
fn parses_all_five_fixture_alias_cases() {
    // fixture-contract §7: every alias-resolution expectation from the `people`
    // fixture, exercised through the S2 grammar in one source body.
    let (_, columns) = parse_schema_columns(
        "schema PersonaQuaerenda {
            columna numerus id
            columna textus nomen : plenum
            columna bivalens activa : statim
            columna textus ∪ nihil email : electronica
            columna instans ∪ nihil created_at : creata
            columna numerus ∪ nihil salarium : stipendium
        }",
    );
    assert_eq!(columns.len(), 6);

    // 1. Identity mapping (`id` → `id`): no alias row.
    assert_eq!(columns[0].name, "id");
    assert_eq!(columns[0].source_alias, None);

    // 2. Visible-column rename (`nomen` → `plenum`).
    assert_eq!(columns[1].name, "nomen");
    assert_eq!(columns[1].source_alias.as_deref(), Some("plenum"));

    // 3. Source-only predicate-column rename (`activa` → `statim`).
    assert_eq!(columns[2].name, "activa");
    assert_eq!(columns[2].source_alias.as_deref(), Some("statim"));

    // 4. Extra-column renames (`email` → `electronica`, `created_at` → `creata`).
    assert_eq!(columns[3].name, "email");
    assert_eq!(columns[3].source_alias.as_deref(), Some("electronica"));
    assert_eq!(columns[4].name, "created_at");
    assert_eq!(columns[4].source_alias.as_deref(), Some("creata"));

    // 5. Failure-carrier rename (`salarium` → `stipendium`).
    assert_eq!(columns[5].name, "salarium");
    assert_eq!(columns[5].source_alias.as_deref(), Some("stipendium"));
}

#[test]
fn preserves_canonical_column_order() {
    // Canonical column order is explicit relational metadata (D4 field 5);
    // the parsed column list must be exactly source order.
    let (_, columns) = parse_schema_columns(
        "schema PersonaBrevis {
            columna numerus id
            columna textus nomen : plenum
        }",
    );
    assert_eq!(columns.len(), 2);
    assert_eq!(columns[0].name, "id");
    assert_eq!(columns[1].name, "nomen");
}

#[test]
fn parses_empty_schema() {
    let (name, columns) = parse_schema_columns("schema Empty {}");
    assert_eq!(name, "Empty");
    assert!(columns.is_empty());
}

#[test]
fn duplicate_visible_names_parse_without_error() {
    // Duplicate visible names (DUPLICATE-VISIBLE) and duplicate resolved source
    // names (DUP-RESOLVED) are semantic rejections (S2-U2); the parser surface
    // accepts them so the semantic layer raises the pinned issue. This test
    // locks the surface boundary: both shapes parse cleanly.
    let _ = parse_ok(
        "schema Duplicata {
            columna numerus id
            columna numerus id
        }",
    );
    let _ = parse_ok(
        "schema Collidens {
            columna numerus id
            columna textus nomen : plenum
            columna textus plenum : plenum
        }",
    );
}

// =============================================================================
// Parse-negative (grammar-level rejections + malformed columns)
// =============================================================================

#[test]
fn rejects_method_like_member_in_schema() {
    // Method-like members are not schema surface (no genus semantics): the
    // pinned `schema_method` issue identity is raised at parse time.
    assert_parse_error_issue(
        "schema S { columna numerus id functio currere() {} }",
        "schema_method",
    );
}

#[test]
fn rejects_inheritance_syntax_on_schema() {
    // `sub`/`implet` inheritance syntax is rejected with the pinned
    // `schema_inheritance` issue identity, both in the header and in the body.
    assert_parse_error_issue("schema S sub T { }", "schema_inheritance");
    assert_parse_error_issue("schema S implet T { }", "schema_inheritance");
    assert_parse_error_issue(
        "schema S { columna numerus id sub T }",
        "schema_inheritance",
    );
    assert_parse_error_issue(
        "schema S { columna numerus id implet T }",
        "schema_inheritance",
    );
}

#[test]
fn rejects_nested_column_declarations() {
    // A `columna` keyword inside a column slot (type position or name/alias
    // position) is a nested column declaration; the pinned
    // `schema_nested_column` issue identity is raised at parse time.
    assert_parse_error_issue(
        "schema S { columna columna numerus id }",
        "schema_nested_column",
    );
    assert_parse_error_issue(
        "schema S { columna numerus id columna textus email }",
        "schema_nested_column",
    );
}

#[test]
fn missing_column_name_is_a_parse_error() {
    let result = parse_program("schema S { columna numerus : plenum }");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == ParseErrorKind::Expected),
        "missing column name must be a structured expected error, got {:?}",
        result.errors
    );
}

#[test]
fn malformed_alias_is_a_parse_error() {
    // A `:` with no source name after it is a malformed alias row.
    assert_parse_error_matches(
        "schema S { columna numerus id : }",
        ParseExpectation::Kind(ParseErrorKind::Expected),
    );
}

#[test]
fn schema_without_name_is_a_parse_error() {
    let result = parse_program("schema { columna numerus id }");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == ParseErrorKind::Expected),
        "missing schema name must be a structured expected error, got {:?}",
        result.errors
    );
}

#[test]
fn schema_without_body_is_a_parse_error() {
    let result = parse_program("schema S");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == ParseErrorKind::Expected),
        "missing schema body must be a structured expected error, got {:?}",
        result.errors
    );
}

// =============================================================================
// Trivia / comment claims
// =============================================================================

#[test]
fn comment_before_schema_close_is_claimed() {
    // A full-line comment before a schema's closing `}` is legal
    // SchemaDecl.end_trivia (mirrors ClassDecl), so it is claimed and no
    // invalid-placement diagnostic is reported.
    let source = "schema S {\n  columna numerus id\n  # before close\n}\n";
    let result = parse_program(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().expect("program");
    let StmtKind::Schema(schema) = &program.statements[0].kind else {
        panic!("expected schema declaration");
    };
    let comments: Vec<&str> = schema
        .end_trivia
        .iter()
        .filter_map(|item| match item {
            Trivia::CommentLine(comment) => Some(result.interner.resolve(comment.text)),
            Trivia::Newline(_) => None,
        })
        .collect();
    assert_eq!(comments, vec![" before close"]);
    assert_eq!(schema.columns.len(), 1);
}

#[test]
fn comment_between_columns_is_claimed_as_column_trivia() {
    let source =
        "schema S {\n  columna numerus id\n  # between\n  columna textus nomen : plenum\n}\n";
    let result = parse_program(source);
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().expect("program");
    let StmtKind::Schema(schema) = &program.statements[0].kind else {
        panic!("expected schema declaration");
    };
    let comments: Vec<&str> = schema.columns[1]
        .leading_trivia
        .iter()
        .filter_map(|item| match item {
            Trivia::CommentLine(comment) => Some(result.interner.resolve(comment.text)),
            Trivia::Newline(_) => None,
        })
        .collect();
    assert_eq!(comments, vec![" between"]);
}

#[test]
fn trailing_comment_after_column_is_not_a_nested_column() {
    // The lexer rejects a mid-line `#` after code (`inline_hash_after_code`),
    // so this regression feeds the parser a token stream where a `LineComment`
    // directly trails the column name — before the newline, with the next
    // `columna` on a fresh line. The nested-column guard must scan the raw
    // trivia run (LineComment, Newline) for the newline instead of assuming
    // the raw next token is the newline.
    // Regression: `columna numerus id # comment` falsely raised
    // `schema_nested_column`.
    let mut lexed = lex("schema S {\n  columna numerus id\n  columna textus nomen\n}\n");
    let ident_idx = lexed
        .tokens
        .iter()
        .position(|tok| {
            matches!(tok.kind, TokenKind::Ident(sym) if lexed.interner.resolve(sym) == "id")
        })
        .expect("column-name ident in fixture");
    let comment_sym = lexed.interner.intern_raw(" primary key");
    let comment_span = Span::new(
        lexed.tokens[ident_idx].span.end,
        lexed.tokens[ident_idx + 1].span.start,
    );
    lexed.tokens.insert(
        ident_idx + 1,
        Token::new(TokenKind::LineComment(comment_sym), comment_span),
    );

    let mut parser = Parser::new(lexed.tokens, lexed.interner, ParseOptions::default());
    let result = parser.parse_program();
    assert!(result.success(), "{:?}", result.errors);

    let program = result.program.as_ref().expect("program");
    let StmtKind::Schema(schema) = &program.statements[0].kind else {
        panic!("expected schema declaration");
    };
    assert_eq!(schema.columns.len(), 2);
    // The trailing comment is claimed as the next column's leading trivia.
    let comments: Vec<&str> = schema.columns[1]
        .leading_trivia
        .iter()
        .filter_map(|item| match item {
            Trivia::CommentLine(comment) => Some(result.interner.resolve(comment.text)),
            Trivia::Newline(_) => None,
        })
        .collect();
    assert_eq!(comments, vec![" primary key"]);
}
