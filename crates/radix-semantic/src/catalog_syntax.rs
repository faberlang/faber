//! Lexer and parser diagnostic catalog entries.
//!
//! This module owns syntax-phase code mappings and structured message/help
//! rendering. The parent catalog owns shared reader fallback helpers, and
//! `catalog_semantic` owns analysis-phase mappings.

use super::catalog::{DiagnosticSpec, arg_value, quote_arg};
use radix_diagnostics::DiagnosticArg;
use radix_lexer::{HOMOGLYPH_ISSUE, LexErrorKind};
use radix_parser::ParseErrorKind;

// SYNTAX KINDS

pub(crate) const LEX_ERROR_KINDS: &[LexErrorKind] = &[
    LexErrorKind::UnterminatedString,
    LexErrorKind::NumberInvalid,
    LexErrorKind::EscapeInvalid,
    LexErrorKind::CharacterUnexpected,
    LexErrorKind::CommentSyntaxInvalid,
    LexErrorKind::InlineComment,
];

pub(crate) const PARSE_ERROR_KINDS: &[ParseErrorKind] = &[
    ParseErrorKind::LexError,
    ParseErrorKind::Expected,
    ParseErrorKind::Unexpected,
    ParseErrorKind::ModifierDuplicate,
    ParseErrorKind::AnnotationInvalid,
    ParseErrorKind::DeclarationInvalid,
    ParseErrorKind::ModifierInvalid,
    ParseErrorKind::ParameterInvalid,
    ParseErrorKind::TypeParameterInvalid,
    ParseErrorKind::ClassBodyAbsent,
    ParseErrorKind::ClassNameAbsent,
    ParseErrorKind::FunctionBodyAbsent,
    ParseErrorKind::FunctionNameAbsent,
    ParseErrorKind::CasuArmInvalid,
    ParseErrorKind::CasuValueInvalid,
    ParseErrorKind::PatternInvalid,
    ParseErrorKind::StatementInvalid,
    ParseErrorKind::BlockAbsent,
    ParseErrorKind::ThereforeWrongContext,
    ParseErrorKind::ErgoWrongContext,
    ParseErrorKind::ConditionAbsent,
    ParseErrorKind::AssignmentTargetInvalid,
    ParseErrorKind::CallArgumentInvalid,
    ParseErrorKind::ExpressionInvalid,
    ParseErrorKind::LiteralInvalid,
    ParseErrorKind::MemberAccessInvalid,
    ParseErrorKind::OperatorInvalid,
    ParseErrorKind::UnterminatedGroup,
    ParseErrorKind::TypeInvalid,
    ParseErrorKind::TypeAnnotationInvalid,
    ParseErrorKind::UnterminatedTypeParams,
    ParseErrorKind::ImportInvalid,
    ParseErrorKind::CommentPlacementInvalid,
];

// STABLE CODES

/// Return the stable diagnostic spec for a lexer error kind.
///
/// Lexer codes are intentionally compact because lexing has a small error
/// surface. Add new codes at the end of the `LEX` range rather than reusing or
/// renumbering existing meanings.
#[must_use]
pub fn lex_spec(kind: LexErrorKind) -> DiagnosticSpec {
    match kind {
        LexErrorKind::UnterminatedString => DiagnosticSpec {
            code: "LEX001",
            help: Some("close the string literal before the end of the line or file"),
        },
        LexErrorKind::NumberInvalid => DiagnosticSpec {
            code: "LEX002",
            help: Some("check numeric separators and base prefixes"),
        },
        LexErrorKind::EscapeInvalid => DiagnosticSpec {
            code: "LEX003",
            help: Some("use a supported escape sequence or escape the backslash"),
        },
        LexErrorKind::CharacterUnexpected => DiagnosticSpec {
            code: "LEX004",
            help: Some("remove the character or escape it if it should be literal"),
        },
        LexErrorKind::CommentSyntaxInvalid => DiagnosticSpec {
            code: "LEX006",
            help: Some("prefix comments with # instead of // or /* */"),
        },
        LexErrorKind::InlineComment => DiagnosticSpec {
            code: "LEX007",
            help: Some("put the # comment on its own line above the code it documents"),
        },
    }
}

/// Return the stable diagnostic spec for a parser error kind.
///
/// Parser codes are grouped by grammar area to make reports scannable and to
/// leave room for new syntax diagnostics without changing existing IDs:
/// generic token errors, declarations, statements, expressions, types, and
/// import and declaration forms each occupy their own range.
#[must_use]
pub fn parse_spec(kind: ParseErrorKind) -> DiagnosticSpec {
    match kind {
        ParseErrorKind::LexError => DiagnosticSpec {
            code: "PARSE000",
            help: Some("fix the lexer error first; parsing continues from clean tokens"),
        },
        ParseErrorKind::Expected => DiagnosticSpec {
            code: "PARSE001",
            help: Some("ensure the expected token is present in this position"),
        },
        ParseErrorKind::Unexpected => DiagnosticSpec {
            code: "PARSE002",
            help: Some("remove or reposition the unexpected token"),
        },
        ParseErrorKind::DeclarationInvalid => DiagnosticSpec {
            code: "PARSE010",
            help: Some("check declaration keywords and required parts"),
        },
        ParseErrorKind::FunctionNameAbsent => DiagnosticSpec {
            code: "PARSE011",
            help: None,
        },
        ParseErrorKind::FunctionBodyAbsent => DiagnosticSpec {
            code: "PARSE012",
            help: None,
        },
        ParseErrorKind::ClassNameAbsent => DiagnosticSpec {
            code: "PARSE013",
            help: None,
        },
        ParseErrorKind::ClassBodyAbsent => DiagnosticSpec {
            code: "PARSE014",
            help: None,
        },
        ParseErrorKind::ParameterInvalid => DiagnosticSpec {
            code: "PARSE015",
            help: Some("check parameter syntax and order"),
        },
        ParseErrorKind::TypeParameterInvalid => DiagnosticSpec {
            code: "PARSE016",
            help: Some("check type parameter list for commas and identifiers"),
        },
        ParseErrorKind::ModifierInvalid => DiagnosticSpec {
            code: "PARSE017",
            help: Some("move or remove the modifier"),
        },
        ParseErrorKind::AnnotationInvalid => DiagnosticSpec {
            code: "PARSE018",
            help: Some("add an annotation name after '@'"),
        },
        ParseErrorKind::ModifierDuplicate => DiagnosticSpec {
            code: "PARSE019",
            help: Some("remove the duplicate modifier"),
        },
        ParseErrorKind::StatementInvalid => DiagnosticSpec {
            code: "PARSE020",
            help: None,
        },
        ParseErrorKind::ConditionAbsent => DiagnosticSpec {
            code: "PARSE021",
            help: Some("add a condition expression"),
        },
        ParseErrorKind::BlockAbsent => DiagnosticSpec {
            code: "PARSE022",
            help: None,
        },
        ParseErrorKind::ThereforeWrongContext => DiagnosticSpec {
            code: "PARSE026",
            help: Some("use 'ergo' for a single-statement body, or wrap the body in braces"),
        },
        ParseErrorKind::ErgoWrongContext => DiagnosticSpec {
            code: "PARSE027",
            help: Some("use '∴' between a closure signature and its body"),
        },
        ParseErrorKind::PatternInvalid => DiagnosticSpec {
            code: "PARSE023",
            help: None,
        },
        ParseErrorKind::CasuArmInvalid => DiagnosticSpec {
            code: "PARSE024",
            help: None,
        },
        ParseErrorKind::CasuValueInvalid => DiagnosticSpec {
            code: "PARSE025",
            help: Some("use a valid case expression"),
        },
        ParseErrorKind::ExpressionInvalid => DiagnosticSpec {
            code: "PARSE030",
            help: Some("check expression syntax"),
        },
        ParseErrorKind::LiteralInvalid => DiagnosticSpec {
            code: "PARSE031",
            help: Some("check the literal form"),
        },
        ParseErrorKind::OperatorInvalid => DiagnosticSpec {
            code: "PARSE032",
            help: Some("check operator placement and operands"),
        },
        ParseErrorKind::UnterminatedGroup => DiagnosticSpec {
            code: "PARSE033",
            help: Some("close the grouping delimiter"),
        },
        ParseErrorKind::CallArgumentInvalid => DiagnosticSpec {
            code: "PARSE034",
            help: Some("check call argument syntax"),
        },
        ParseErrorKind::MemberAccessInvalid => DiagnosticSpec {
            code: "PARSE035",
            help: Some("check member access syntax"),
        },
        ParseErrorKind::AssignmentTargetInvalid => DiagnosticSpec {
            code: "PARSE036",
            help: Some("assign only to valid lvalues"),
        },
        ParseErrorKind::TypeInvalid => DiagnosticSpec {
            code: "PARSE040",
            help: Some("check type syntax"),
        },
        ParseErrorKind::TypeAnnotationInvalid => DiagnosticSpec {
            code: "PARSE041",
            help: Some("check type annotation form"),
        },
        ParseErrorKind::UnterminatedTypeParams => DiagnosticSpec {
            code: "PARSE042",
            help: Some("close the type parameter list with '>'"),
        },
        ParseErrorKind::ImportInvalid => DiagnosticSpec {
            code: "PARSE050",
            help: Some("check import syntax"),
        },
        ParseErrorKind::CommentPlacementInvalid => DiagnosticSpec {
            code: "PARSE060",
            help: Some(
                "place comments before a statement, declaration, or member; or immediately before a block's closing '}' or end of file",
            ),
        },
    }
}

// ISSUE RENDERING

#[must_use]
pub fn lex_message(kind: LexErrorKind, args: &[DiagnosticArg]) -> Option<String> {
    let issue = arg_value(args, "issue");
    match kind {
        LexErrorKind::UnterminatedString => match issue? {
            "unterminated_ascii_literal" => Some("unterminated ascii literal".to_owned()),
            "newline_in_ascii_literal" => {
                Some("unterminated ascii literal (newline in literal)".to_owned())
            }
            "unterminated_backtick_template_literal" => {
                Some("unterminated backtick template literal".to_owned())
            }
            "unterminated_octeti_literal" => Some("unterminated octeti literal".to_owned()),
            "unterminated_regex_literal" => Some("unterminated regex literal".to_owned()),
            "empty_regex_literal" => {
                Some("empty regex literal; write a pattern between the slashes".to_owned())
            }
            "unterminated_string_literal" => Some("unterminated string literal".to_owned()),
            "newline_in_string_literal" => {
                Some("unterminated string literal (newline in string)".to_owned())
            }
            "unterminated_guillemet_block_string_literal" => {
                Some("unterminated guillemet block string literal".to_owned())
            }
            _ => None,
        },
        LexErrorKind::NumberInvalid => match issue? {
            "invalid_hexadecimal_number" => Some("invalid hexadecimal number".to_owned()),
            "invalid_binary_number" => Some("invalid binary number".to_owned()),
            "invalid_octal_number" => Some("invalid octal number".to_owned()),
            "invalid_floating_point_number" => Some("invalid floating-point number".to_owned()),
            "invalid_integer" => Some("invalid integer".to_owned()),
            _ => None,
        },
        LexErrorKind::EscapeInvalid => Some("invalid escape sequence".to_owned()),
        LexErrorKind::CharacterUnexpected => match issue? {
            "unexpected_character" => {
                let ch = arg_value(args, "char")?;
                Some(format!("unexpected character: {ch}"))
            }
            HOMOGLYPH_ISSUE => {
                let ch = arg_value(args, "char")?;
                let suggestion = arg_value(args, "suggestion")?;
                Some(format!(
                    "unexpected character: '{ch}'; did you mean '{suggestion}'?"
                ))
            }
            _ => None,
        },
        LexErrorKind::CommentSyntaxInvalid => match issue? {
            "c_style_line_comment" => Some(
                "C-style // comments are not valid in Faber; use # for line comments".to_owned(),
            ),
            "c_style_block_comment" => Some(
                "C-style /* */ block comments are not valid in Faber; use # for line comments"
                    .to_owned(),
            ),
            "unterminated_c_style_block_comment" => Some(
                "unterminated C-style block comment; Faber uses # for line comments".to_owned(),
            ),
            _ => None,
        },
        LexErrorKind::InlineComment => {
            Some("# comments must start a line; move this comment above the code".to_owned())
        }
    }
}

/// Quoted keyword spelling for a message: an explicit arg of that name when an
/// emit site supplies one, else the Latin keyword. A pack renders the keyword
/// from its own binding, so emit sites do not pass the literal.
fn keyword_or_latin(args: &[DiagnosticArg], keyword: &str) -> String {
    quote_arg(args, keyword).unwrap_or_else(|| format!("'{keyword}'"))
}

#[must_use]
pub fn parse_message(kind: ParseErrorKind, args: &[DiagnosticArg]) -> Option<String> {
    match kind {
        // Lex identity is typed on `Diagnostic::error_kind` after conversion.
        // Do not reconstruct `LexErrorKind` from Debug-text `lex_kind` args.
        ParseErrorKind::LexError => None,
        ParseErrorKind::Expected => expected_message(args),
        ParseErrorKind::Unexpected => unexpected_message(args),
        ParseErrorKind::DeclarationInvalid => Some("invalid declaration".to_owned()),
        ParseErrorKind::ModifierInvalid => Some("invalid modifier".to_owned()),
        ParseErrorKind::ModifierDuplicate => Some("duplicate modifier".to_owned()),
        ParseErrorKind::AnnotationInvalid => invalid_annotation_message(args),
        ParseErrorKind::FunctionNameAbsent => Some(format!(
            "expected function name after {}",
            keyword_or_latin(args, "functio")
        )),
        ParseErrorKind::FunctionBodyAbsent => Some(format!(
            "expected function body for {} declaration",
            keyword_or_latin(args, "functio")
        )),
        ParseErrorKind::ClassNameAbsent => Some(format!(
            "expected class name after {}",
            keyword_or_latin(args, "genus")
        )),
        ParseErrorKind::ClassBodyAbsent => Some(format!(
            "expected class body after {} declaration",
            keyword_or_latin(args, "genus")
        )),
        ParseErrorKind::ParameterInvalid => invalid_parameter_message(args),
        ParseErrorKind::TypeParameterInvalid => invalid_type_parameter_message(args),
        ParseErrorKind::BlockAbsent => quote_arg(args, "block").and_then(|block| {
            quote_arg(args, "ergo").map(|ergo| format!("expected {block} block or {ergo} body"))
        }),
        ParseErrorKind::ThereforeWrongContext => {
            Some("∴ is only for compact clausurae; use ergo for a single-statement body".to_owned())
        }
        ParseErrorKind::ErgoWrongContext => Some(
            "ergo is only for single-statement bodies; use ∴ for a compact clausura".to_owned(),
        ),
        ParseErrorKind::PatternInvalid => invalid_pattern_message(args),
        ParseErrorKind::CasuArmInvalid => quote_arg(args, "casu").and_then(|casu| {
            quote_arg(args, "ceterum")
                .map(|ceterum| format!("expected {casu} arm or {ceterum} default"))
        }),
        ParseErrorKind::CasuValueInvalid => Some("invalid case value".to_owned()),
        ParseErrorKind::StatementInvalid => invalid_statement_message(args),
        ParseErrorKind::ConditionAbsent => Some("missing condition".to_owned()),
        ParseErrorKind::AssignmentTargetInvalid => Some("invalid assignment target".to_owned()),
        ParseErrorKind::CallArgumentInvalid => Some("invalid call argument".to_owned()),
        ParseErrorKind::ExpressionInvalid => invalid_expression_message(args),
        ParseErrorKind::LiteralInvalid => Some("invalid literal".to_owned()),
        ParseErrorKind::MemberAccessInvalid => Some("invalid member access".to_owned()),
        ParseErrorKind::OperatorInvalid => Some("invalid operator".to_owned()),
        ParseErrorKind::UnterminatedGroup => Some("unterminated group".to_owned()),
        ParseErrorKind::TypeInvalid => invalid_type_message(args),
        ParseErrorKind::TypeAnnotationInvalid => Some("invalid type annotation".to_owned()),
        ParseErrorKind::UnterminatedTypeParams => Some("unterminated type parameters".to_owned()),
        ParseErrorKind::ImportInvalid => invalid_import_message(args),
        ParseErrorKind::CommentPlacementInvalid => {
            Some("comment is not in a legal attachment position".to_owned())
        }
    }
}

#[must_use]
pub fn parse_help(kind: ParseErrorKind, args: &[DiagnosticArg]) -> Option<String> {
    match kind {
        ParseErrorKind::Expected => expected_help(args),
        ParseErrorKind::Unexpected => unexpected_help(args),
        ParseErrorKind::AnnotationInvalid => invalid_annotation_help(args),
        ParseErrorKind::FunctionNameAbsent => {
            let functio = keyword_or_latin(args, "functio");
            Some(format!("add a function name after {functio}"))
        }
        ParseErrorKind::FunctionBodyAbsent => {
            let block = quote_arg(args, "block")?;
            Some(format!("add a {block} to the function declaration"))
        }
        ParseErrorKind::ClassNameAbsent => {
            let genus = keyword_or_latin(args, "genus");
            Some(format!("add a class name after {genus}"))
        }
        ParseErrorKind::ClassBodyAbsent => {
            let genus = keyword_or_latin(args, "genus");
            Some(format!("add a block after the {genus} declaration header"))
        }
        ParseErrorKind::ParameterInvalid => invalid_parameter_help(args),
        ParseErrorKind::TypeParameterInvalid => invalid_type_parameter_help(args),
        ParseErrorKind::BlockAbsent => {
            let block = quote_arg(args, "block")?;
            let ergo = quote_arg(args, "ergo")?;
            Some(format!(
                "add {block} or introduce a single statement with {ergo}"
            ))
        }
        ParseErrorKind::ThereforeWrongContext => {
            Some("use 'ergo' for a single-statement body, or wrap the body in braces".to_owned())
        }
        ParseErrorKind::ErgoWrongContext => {
            Some("use '∴' between a closure signature and its body".to_owned())
        }
        ParseErrorKind::PatternInvalid => invalid_pattern_help(args),
        ParseErrorKind::CasuArmInvalid => {
            let casu = quote_arg(args, "casu")?;
            let ceterum = quote_arg(args, "ceterum")?;
            Some(format!(
                "start the dispatch arm with {casu} or close with {ceterum}"
            ))
        }
        ParseErrorKind::StatementInvalid => invalid_statement_help(args),
        ParseErrorKind::ExpressionInvalid => invalid_expression_help(args),
        ParseErrorKind::TypeInvalid => invalid_type_help(args),
        ParseErrorKind::ImportInvalid => invalid_import_help(args),
        _ => None,
    }
}

// PARAMETERS

fn invalid_parameter_message(_args: &[DiagnosticArg]) -> Option<String> {
    None
}

fn invalid_parameter_help(_args: &[DiagnosticArg]) -> Option<String> {
    None
}

// STATEMENTS

fn invalid_statement_message(args: &[DiagnosticArg]) -> Option<String> {
    if let Some(issue) = arg_value(args, "issue") {
        return statement_issue_message(issue);
    }
    let statement = arg_value(args, "statement")?;
    match statement {
        "ad" => Some("ad is only valid in expression position".to_owned()),
        _ => quote_arg(args, "statement")
            .map(|statement| format!("{statement} is not a valid statement form")),
    }
}

fn statement_issue_message(issue: &str) -> Option<String> {
    match issue {
        "discerne_subject_comma_retired" => Some(
            "a comma no longer separates discerne subjects; separate them with 'et'".to_owned(),
        ),
        "discerne_subject_not_path" => {
            Some("a discerne subject is a name or a member path, not an expression".to_owned())
        }
        _ => None,
    }
}

fn invalid_statement_help(args: &[DiagnosticArg]) -> Option<String> {
    if let Some(issue) = arg_value(args, "issue") {
        return statement_issue_help(issue);
    }
    let replacement = arg_value(args, "replacement")?;
    Some(format!("use {replacement}"))
}

fn statement_issue_help(issue: &str) -> Option<String> {
    match issue {
        "discerne_subject_comma_retired" => {
            Some("write 'discerne a et b { … }'; 'et' is the only subject separator".to_owned())
        }
        "discerne_subject_not_path" => {
            Some("bind the value to a name first, then discerne on that name".to_owned())
        }
        _ => None,
    }
}

// PATTERNS

fn invalid_pattern_message(args: &[DiagnosticArg]) -> Option<String> {
    if let Some(issue) = arg_value(args, "issue") {
        return pattern_issue_message(issue);
    }
    quote_arg(args, "wildcard").and_then(|wildcard| {
        arg_value(args, "identifier")
            .map(|identifier| format!("expected pattern such as {wildcard} or {identifier}"))
    })
}

fn pattern_issue_message(issue: &str) -> Option<String> {
    match issue {
        "discerne_pattern_comma_retired" => {
            Some("a comma no longer separates arm patterns; separate them with 'et'".to_owned())
        }
        _ => None,
    }
}

fn invalid_pattern_help(args: &[DiagnosticArg]) -> Option<String> {
    if let Some(issue) = arg_value(args, "issue") {
        return pattern_issue_help(issue);
    }
    let wildcard = quote_arg(args, "wildcard")?;
    let identifier = arg_value(args, "identifier")?;
    Some(format!(
        "use {wildcard}, a literal, or an {identifier} pattern"
    ))
}

fn pattern_issue_help(issue: &str) -> Option<String> {
    match issue {
        "discerne_pattern_comma_retired" => {
            Some("write 'casu a et b { … }'; 'et' is the only pattern separator".to_owned())
        }
        _ => None,
    }
}

// TYPE PARAMETERS

fn invalid_type_parameter_message(args: &[DiagnosticArg]) -> Option<String> {
    let issue = arg_value(args, "issue")?;
    match issue {
        "type_param_after_magnitudo" => {
            let size_param = arg_value(args, "size_param")?;
            Some(format!(
                "type parameters must precede {size_param} parameters in generic lists"
            ))
        }
        _ => None,
    }
}

fn invalid_type_parameter_help(args: &[DiagnosticArg]) -> Option<String> {
    let issue = arg_value(args, "issue")?;
    match issue {
        "type_param_after_magnitudo" => {
            let size_param = arg_value(args, "size_param")?;
            Some(format!(
                "move type parameters before {size_param} parameters"
            ))
        }
        _ => None,
    }
}

// EXPECTED TOKENS

fn expected_message(args: &[DiagnosticArg]) -> Option<String> {
    if let Some(issue) = arg_value(args, "issue") {
        match issue {
            "secus_si_else_if" => return Some("use 'sin' for else-if".to_owned()),
            "requirit_iace" => {
                return Some("expected 'iace' after the requirit condition".to_owned());
            }
            "requirit_else_rejected" => {
                return Some("'secus' is not the particle on requirit".to_owned());
            }
            "reice_iace" => return Some("expected 'iace' after the reice condition".to_owned()),
            "reice_else_rejected" => {
                return Some("'secus' is not the particle on reice".to_owned());
            }
            "adfirma_else_rejected" => {
                return Some("'secus' is not the particle on adfirma".to_owned());
            }
            "adfirma_message_particle" => {
                return Some("expected 'mori' before the adfirma message".to_owned());
            }
            "expected_keyword" => {
                let keyword = quote_arg(args, "keyword")?;
                return Some(format!("expected {keyword}"));
            }
            "lege_lineam" => {
                let lege = quote_arg(args, "lege")?;
                let lineam = quote_arg(args, "lineam")?;
                return Some(format!("expected {lege} or {lineam}"));
            }
            "expected_mutability_keyword" => {
                let fixum = quote_arg(args, "fixum")?;
                let varia = quote_arg(args, "varia")?;
                return Some(format!("expected {fixum} or {varia}"));
            }
            "expected_method_declaration" => {
                let functio = quote_arg(args, "functio")?;
                return Some(format!(
                    "expected a method declaration; write the method as a {functio} declaration"
                ));
            }
            "expected_praepara_hook" => {
                let praepara = quote_arg(args, "praepara")?;
                let praeparabit = quote_arg(args, "praeparabit")?;
                let postpara = quote_arg(args, "postpara")?;
                let postparabit = quote_arg(args, "postparabit")?;
                return Some(format!(
                    "expected a {praepara} hook; write {praepara}, {praeparabit}, {postpara} or {postparabit}"
                ));
            }
            "expected_scribe_keyword" => {
                let scribe = quote_arg(args, "scribe")?;
                let vide = quote_arg(args, "vide")?;
                let mone = quote_arg(args, "mone")?;
                let nota = quote_arg(args, "nota")?;
                return Some(format!(
                    "expected a {scribe} keyword; write {scribe}, {vide}, {mone} or {nota}"
                ));
            }
            "expected_incipit_keyword" => {
                let incipit = quote_arg(args, "incipit")?;
                let incipiet_keyword = quote_arg(args, "incipiet")?;
                return Some(format!("expected {incipit} or {incipiet_keyword}"));
            }
            "reducta_source" => {
                return Some("expected a reduction source after the reduction operator".to_owned());
            }
            "itera_mode" | "mutability_marker" => {
                let expected = arg_value(args, "expected")?;
                return Some(format!("expected {expected}"));
            }
            "annotation_name" => return Some("expected annotation name".to_owned()),
            "duplicate_rest_pattern" => return Some("rest pattern already specified".to_owned()),
            "test_case_or_setup" => return Some("expected test case or setup".to_owned()),
            "test_modifier_integer" => {
                let modifier = quote_arg(args, "modifier")?;
                return Some(format!("expected integer after {modifier}"));
            }
            "tensor_figura_after_element" => {
                return Some("tensor requires a Figura shape after element type".to_owned());
            }
            "sparsa_figura_after_element" => {
                return Some("sparsa requires a Figura shape after element type".to_owned());
            }
            "vector_width_after_element" => {
                return Some("vector requires a width after element type".to_owned());
            }
            "matrix_figura_after_element" => {
                return Some(
                    "matrix requires a two-dimensional Figura shape after element type".to_owned(),
                );
            }
            "atomic_single_element" => {
                return Some("atomic accepts exactly one element type".to_owned());
            }
            "retired_prefix_predicate" => {
                let predicate = arg_value(args, "predicate")?;
                return Some(format!("{predicate} is no longer prefix predicate syntax"));
            }
            "membership_keyword_retired" => {
                let keyword = quote_arg(args, "keyword")?;
                let glyph = quote_arg(args, "glyph")?;
                return Some(format!(
                    "{keyword} is a retired membership operator; membership is spelled {glyph}"
                ));
            }
            "equality_non_missing_est" => {
                return Some("expected 'est' after 'non' in equality expression".to_owned());
            }
            "est_non_spelling" => {
                return Some("'est non' is not the identity negation spelling".to_owned());
            }
            "closure_body_start" => {
                return Some("expected ':' or '{' after closure parameters".to_owned());
            }
            "ad_ascii_route" => return Some("expected ascii literal route after 'ad'".to_owned()),
            "json_quoted_key" => {
                return Some("JSON keys must be quoted strings; use \"name\": value".to_owned());
            }
            "json_string_key" => return Some("expected JSON string key".to_owned()),
            "field_value_separator" => return Some("expected '='".to_owned()),
            "call_type_args_start" => return Some("expected '<'".to_owned()),
            "gradient_empty_selector_list" => {
                return Some(
                    "the gradient selector list after ∇ must name at least one primal call argument".to_owned(),
                );
            }
            _ => {}
        }
    }

    let expected = arg_value(args, "expected")?;
    Some(format!("expected {expected}"))
}

fn expected_help(args: &[DiagnosticArg]) -> Option<String> {
    if let Some(issue) = arg_value(args, "issue") {
        return expected_help_issue(issue, args);
    }
    expected_help_fallback(args)
}

fn expected_help_issue(issue: &str, args: &[DiagnosticArg]) -> Option<String> {
    match issue {
        "secus_si_else_if" => return Some("replace secus si with sin".to_owned()),
        "requirit_iace" => return Some("write requirit cond iace err".to_owned()),
        "requirit_else_rejected" | "reice_else_rejected" => {
            return Some("use 'iace' for the recoverable payload".to_owned());
        }
        "reice_iace" => return Some("write reice cond iace err".to_owned()),
        "adfirma_else_rejected" => {
            return Some("use 'mori' for the optional panic message".to_owned());
        }
        "adfirma_message_particle" => {
            return Some("write adfirma cond mori message".to_owned());
        }
        "itera_mode" => {
            return Some("choose one iteration mode before the iterable expression".to_owned());
        }
        "annotation_name" => return Some("add an annotation name after @".to_owned()),
        other => expected_help_issue_late(other, args),
    }
}

fn expected_help_issue_late(issue: &str, args: &[DiagnosticArg]) -> Option<String> {
    match issue {
        "duplicate_rest_pattern" => {
            let keyword = quote_arg(args, "keyword")?;
            return Some(format!("keep only one {keyword} rest binding"));
        }
        "mutability_marker" => {
            return Some("add sit or varia before the binding name".to_owned());
        }
        "test_case_or_setup" => {
            let proba = quote_arg(args, "proba")?;
            let probandum = quote_arg(args, "probandum")?;
            let setup = quote_arg(args, "setup")?;
            return Some(format!(
                "start the body item with {proba}, {probandum}, or {setup}"
            ));
        }
        "test_modifier_integer" => {
            let modifier = quote_arg(args, "modifier")?;
            return Some(format!("put an integer literal after {modifier}"));
        }
        "tensor_figura_after_element" => {
            return Some("add a comma and Figura shape, such as tensor<T, []>".to_owned());
        }
        "sparsa_figura_after_element" => {
            return Some("add a comma and Figura shape, such as sparsa<T, []>".to_owned());
        }
        "vector_width_after_element" => {
            return Some("add a comma and vector width, such as vector<T, [4]>".to_owned());
        }
        "matrix_figura_after_element" => {
            return Some("add a comma and shape, such as matrix<T, [4, 4]>".to_owned());
        }
        "atomic_single_element" => {
            return Some("remove the extra type parameter; use atomic<T>".to_owned());
        }
        "retired_prefix_predicate" => {
            let replacement = arg_value(args, "replacement")?;
            return Some(format!("rewrite with {replacement}"));
        }
        "equality_non_missing_est" => {
            return Some("write identity negation as non est".to_owned());
        }
        "membership_keyword_retired" => {
            let glyph = quote_arg(args, "glyph")?;
            return Some(format!(
                "write {glyph} (or ∉ to negate) between the element and the collection or range"
            ));
        }
        "est_non_spelling" => return Some("use non est for identity negation".to_owned()),
        "closure_body_start" => {
            return Some("add : for an expression body or { ... } for a block body".to_owned());
        }
        "ad_ascii_route" => {
            return Some("write the route as an ASCII literal after ad".to_owned());
        }
        "json_quoted_key" => return Some("quote JSON object keys".to_owned()),
        "json_string_key" => {
            return Some("start the JSON key with a string literal".to_owned());
        }
        "field_value_separator" => {
            return Some("use = between field name and value".to_owned());
        }
        "call_type_args_start" => {
            return Some("start call-site type arguments with <".to_owned());
        }
        "gradient_empty_selector_list" => {
            return Some(
                "name the primal call arguments whose adjoints you want, e.g. loss(x, y) ∇ [x]"
                    .to_owned(),
            );
        }
        _ => {}
    }
    expected_help_fallback(args)
}

fn expected_help_fallback(args: &[DiagnosticArg]) -> Option<String> {
    let expected = arg_value(args, "expected")?;
    Some(format!("provide {expected} in this grammar position"))
}

// UNEXPECTED TOKENS

fn unexpected_message(args: &[DiagnosticArg]) -> Option<String> {
    let issue = arg_value(args, "issue")?;
    match issue {
        "json_unknown_identifier_value" => {
            Some("JSON identifiers are not valid JSON values".to_owned())
        }
        "json_faber_keyword_value" => Some("Faber keywords are not valid JSON values".to_owned()),
        "expected_json_value" => Some("expected JSON value".to_owned()),
        "json_integer_underflow" => Some("JSON integer literal underflows numerus".to_owned()),
        "expected_json_number_after_minus" => Some("expected number after '-' in JSON".to_owned()),
        "conversio_assign_requires_concrete_type" => Some(
            "conversion-directed assignment '↤' requires a concrete destination type".to_owned(),
        ),
        "conversio_assign_await_unsupported" => {
            Some("'↤' is not supported on awaited bindings".to_owned())
        }
        "allocator_surface_removed" => quote_arg(args, "keyword")
            .map(|keyword| format!("{keyword} was removed; allocation is not a language surface")),
        "inheritance_removed" => quote_arg(args, "keyword").map(|keyword| {
            format!("{keyword} was removed; class inheritance is not a language surface")
        }),
        "nexum_removed" => Some(
            "'nexum' was removed; change notification is a library pattern over 'ad'".to_owned(),
        ),
        "construction_spread_removed" => {
            Some("'sparge' is not valid in a construction literal".to_owned())
        }
        "construction_source_repeated" => {
            Some("a construction literal takes exactly one 'ex' source".to_owned())
        }
        _ => None,
    }
}

fn unexpected_help(args: &[DiagnosticArg]) -> Option<String> {
    let issue = arg_value(args, "issue")?;
    match issue {
        "json_unknown_identifier_value" | "json_faber_keyword_value" => {
            Some("use true, false, null, a string, a number, an object, or an array".to_owned())
        }
        "expected_json_value" => Some("provide a JSON value".to_owned()),
        "json_integer_underflow" => Some("use a numerus value within range".to_owned()),
        "expected_json_number_after_minus" => Some("put a number after '-' in JSON".to_owned()),
        "conversio_assign_requires_concrete_type" => {
            Some("write a concrete type for the binding, e.g. 'sit numerus x ↤ value'".to_owned())
        }
        "conversio_assign_await_unsupported" => {
            Some("keep '←' for 'figendum'/'variandum' awaited bindings".to_owned())
        }
        "allocator_surface_removed" => quote_arg(args, "keyword").map(|keyword| {
            format!(
                "delete the {keyword} wrapper or modifier; allocation is managed by the target runtime"
            )
        }),
        "inheritance_removed" => Some(
            "inheritance was removed; use a contract (`implet`) or composition".to_owned(),
        ),
        "nexum_removed" => {
            Some("delete 'nexum' and declare an ordinary field: '<type> <name>'".to_owned())
        }
        "construction_spread_removed" => Some(
            "name each field; copy-with-changes is 'Genus { field = value } ex source'".to_owned(),
        ),
        "construction_source_repeated" => Some(
            "copy from one value; set the other changes as braced fields".to_owned(),
        ),
        _ => None,
    }
}

// ANNOTATIONS

fn invalid_annotation_message(args: &[DiagnosticArg]) -> Option<String> {
    let issue = arg_value(args, "issue")?;
    match issue {
        "invalid_modifier" => {
            let annotation = arg_value(args, "annotation")?;
            Some(format!("invalid @ {annotation} modifier"))
        }
        "empty_radix_typus_domain" => {
            Some("@ radix typus domain requires at least one allowed type".to_owned())
        }
        "expected_annotation_ident" => {
            let context = arg_value(args, "context")?;
            Some(format!("expected {context}"))
        }
        "unexpected_annotation_token" => {
            let context = arg_value(args, "context")?;
            Some(format!("unexpected token after {context}"))
        }
        "nucleum_lane_removed" => {
            let name = arg_value(args, "name")?;
            Some(format!(
                "the @ nucleum lane binding '{name}' was removed; a kernel never names its lane"
            ))
        }
        "wrong_locale_annotation" => {
            let annotation = arg_value(args, "annotation")?;
            Some(format!(
                "annotation `@ {annotation}` is not declared on this reader surface"
            ))
        }
        _ => None,
    }
}

fn invalid_annotation_help(args: &[DiagnosticArg]) -> Option<String> {
    let issue = arg_value(args, "issue")?;
    match issue {
        "invalid_modifier" => {
            let annotation = arg_value(args, "annotation")?;
            Some(format!("use a supported @ {annotation} modifier"))
        }
        "empty_radix_typus_domain" => Some("list at least one allowed type after in".to_owned()),
        "expected_annotation_ident" => {
            let expected = quote_arg(args, "expected")?;
            Some(format!("add {expected} in this annotation position"))
        }
        "unexpected_annotation_token" => Some("remove the trailing annotation token".to_owned()),
        "nucleum_lane_removed" => Some(
            "bind the iteration coordinate with `apud` on a `filum` loop: `itera ex t apud [i] filum f sit v`"
                .to_owned(),
        ),
        "wrong_locale_annotation" => {
            let expected = quote_arg(args, "expected")?;
            Some(format!("use `@ {expected}` for this file's reader locale"))
        }
        _ => None,
    }
}

// TYPES

fn invalid_type_message(args: &[DiagnosticArg]) -> Option<String> {
    let issue = arg_value(args, "issue")?;
    match issue {
        "expected_type_name" => Some("expected type name".to_owned()),
        "wrong_locale_type" => {
            let ty = arg_value(args, "type")?;
            Some(format!(
                "type `{ty}` is not declared on this reader surface"
            ))
        }
        "null_value_in_type_position" => {
            let value = arg_value(args, "value")?;
            Some(format!("`{value}` is the null value, not a type"))
        }
        "est_value_literal_names_equality" => {
            let value = arg_value(args, "value")?;
            Some(format!("`est` tests types; `{value}` is a value"))
        }
        "lista_sugar_bracket_form" => Some("lista sugar has no bracket form".to_owned()),
        "matrix_sugar_missing_shape" => {
            Some("matrix sugar requires a two-dimensional bracket shape".to_owned())
        }
        "lista_sugar_angle_bracket_form" => Some("lista sugar is bare l+width only".to_owned()),
        "vector_sugar_angle_bracket_form" => Some("vector sugar uses bracket width".to_owned()),
        "type_sugar_angle_bracket_form" => {
            let sugar = arg_value(args, "sugar")?;
            Some(format!("{sugar} sugar uses bracket Figura"))
        }
        "numeric_wrapper_retired" => {
            let form = arg_value(args, "form")?;
            Some(format!("`{form}` is a retired numeric type spelling"))
        }
        "range_bound_required" => Some("a range needs both ends".to_owned()),
        _ => None,
    }
}

fn invalid_type_help(args: &[DiagnosticArg]) -> Option<String> {
    let issue = arg_value(args, "issue")?;
    match issue {
        "expected_type_name" => {
            Some("start the type with an identifier, _, or function type".to_owned())
        }
        "wrong_locale_type" => Some(match arg_value(args, "expected") {
            Some(expected) => {
                format!("use the type spelling declared by this reader locale: `{expected}`")
            }
            None => "use the type spelling declared by this reader locale".to_owned(),
        }),
        "null_value_in_type_position" => {
            let ty = arg_value(args, "type")?;
            Some(format!(
                "the null type is `{ty}`; write `{ty}` where a type is required"
            ))
        }
        "est_value_literal_names_equality" => {
            let ty = arg_value(args, "type")?;
            Some(format!(
                "compare values with the equality family: `≡` (strict), `≅` (promoted-exact), `≈` (fuzzy); to test for the null type, write `x est {ty}`"
            ))
        }
        "lista_sugar_bracket_form" => Some("use tensor sugar for shaped buffers".to_owned()),
        "matrix_sugar_missing_shape" => Some("write matrix sugar as mf32[4, 4]".to_owned()),
        "lista_sugar_angle_bracket_form" => {
            Some("use lista<...> for angle-bracket form".to_owned())
        }
        "vector_sugar_angle_bracket_form" => {
            Some("write the width with vector sugar brackets".to_owned())
        }
        "type_sugar_angle_bracket_form" => {
            let sugar = arg_value(args, "sugar")?;
            Some(format!("write the shape with {sugar} width sugar brackets"))
        }
        "numeric_wrapper_retired" => {
            let form = arg_value(args, "form")?;
            let bare = arg_value(args, "bare")?;
            Some(format!(
                "write `{bare}`, not `{form}`; a width marker is its own type"
            ))
        }
        "range_bound_required" => Some(
            "write both ends, such as 0‥8 or 0…7, with ∞ or -∞ for no bound on a side".to_owned(),
        ),
        _ => None,
    }
}

// EXPRESSIONS

fn invalid_expression_message(args: &[DiagnosticArg]) -> Option<String> {
    let issue = arg_value(args, "issue")?;
    match issue {
        "conversio_recovery_uses_vel" => Some("use ⊥ for a conversion default".to_owned()),
        "conversio_via_missing_hint" => Some("`via` needs a convert hint after it".to_owned()),
        "conversio_hint_type_argument" => Some("a convert hint is not a type argument".to_owned()),
        "conversio_hint_tail_argument" => {
            Some("a convert hint does not follow the target in angle brackets".to_owned())
        }
        "conditional_nested" => {
            Some("a '✓ ✗' conditional cannot appear inside another '✓ ✗'".to_owned())
        }
        "format_spec_not_literal" => {
            Some("the '¶' format spec must be a string literal".to_owned())
        }
        "format_chained" => Some("'¶' does not chain".to_owned()),
        "copy_into_chain" => Some("'⇇' does not chain".to_owned()),
        "itera_source_sort_reduce_unbound" => {
            Some("a sort or reduce expression cannot be a loop source".to_owned())
        }
        "conditional_question_colon_removed" => {
            Some("'c ? a : b' was removed; write 'c ✓ a ✗ b'".to_owned())
        }
        "conditional_sic_removed" => {
            Some("'c sic a secus b' was removed; write 'c ✓ a ✗ b'".to_owned())
        }
        "recovery_uses_exit_arrow" => {
            Some("'⇥' names an error type; use '⊥' for a default".to_owned())
        }
        "default_with_propagation" => Some(
            "an expression either propagates with '⇥' or defaults with '⊥', never both".to_owned(),
        ),
        "default_requires_failable" => {
            Some("'⊥' default follows a failable call or a conversion '↦ T' or '↤'".to_owned())
        }
        "expected_expression" => Some("expected expression".to_owned()),
        "expression_ad_block" => Some("expression-form ad must not have a block".to_owned()),
        "closure_block_body" => Some("closure block body must use 'fac'".to_owned()),
        "closure_fac_body_dum" => Some("closure fac body cannot use 'dum'".to_owned()),
        "retired_null_value_spelling" => {
            let replacement = arg_value(args, "replacement")?;
            Some(format!(
                "`nihil` is not the null value; the null value is `{replacement}`"
            ))
        }
        _ => None,
    }
}

fn invalid_expression_help(args: &[DiagnosticArg]) -> Option<String> {
    let issue = arg_value(args, "issue")?;
    match issue {
        "conversio_recovery_uses_vel" => Some("use ⊥; vel is nullish coalescing only".to_owned()),
        "conversio_via_missing_hint" => Some(
            "name the hint right after via, as in `x ↦ u32 via Hex`; the hints are Hex, Bin, Oct, Be, Le, and Code".to_owned(),
        ),
        "conversio_hint_type_argument" => Some(
            "write the hint as a via clause after the target: `u32 via Le`, `ascii<size 8> via Hex`, `littera via Code`".to_owned(),
        ),
        "conversio_hint_tail_argument" => Some(
            "write the hint as a via clause after the target: `octeti<16> via Le`".to_owned(),
        ),
        "conditional_nested" => {
            Some("use 'si'/'sin', 'discerne', or a function that 'redde's each case".to_owned())
        }
        "format_spec_not_literal" => Some(
            "write the spec in quotes, like 'x ¶ \".2\"'; a computed spec is not allowed"
                .to_owned(),
        ),
        "format_chained" => Some("format once; parenthesize to format a formatted text".to_owned()),
        "copy_into_chain" => {
            Some("write one 'target ⇇ value' per statement; '⇇' has no result to chain".to_owned())
        }
        "itera_source_sort_reduce_unbound" => Some(
            "bind it to a name first, then loop over the name: `let list<int> ys ← sort via asc from xs`, then `for from ys const x`"
                .to_owned(),
        ),
        "recovery_uses_exit_arrow" => {
            Some("write '↦ T ⊥ value' (or '↤ … ⊥ value'); '⇥' never takes a value".to_owned())
        }
        "default_with_propagation" => {
            Some("keep the '⊥' default, or drop it and let the error propagate".to_owned())
        }
        "default_requires_failable" => Some(
            "put '⊥' right after the call or conversion it defaults; use 'capta' to inspect the error"
                .to_owned(),
        ),
        "expected_expression" => Some("provide a valid expression in this position".to_owned()),
        "expression_ad_block" => Some("use ad 'route' (opener) ↦ T without a block".to_owned()),
        "closure_block_body" => {
            let fac_body = arg_value(args, "fac_body").unwrap_or("fac { ... }");
            Some(format!(
                "write the compact closure body after ∴ as {fac_body}"
            ))
        }
        "closure_fac_body_dum" => Some("remove dum from closure fac bodies".to_owned()),
        "retired_null_value_spelling" => {
            let replacement = arg_value(args, "replacement")?;
            Some(format!(
                "write `{replacement}` for the null value; `nihil` names only the null type"
            ))
        }
        _ => None,
    }
}

// IMPORTS

fn invalid_import_message(args: &[DiagnosticArg]) -> Option<String> {
    let issue = arg_value(args, "issue")?;
    match issue {
        "unknown_import_field" => Some("unknown import field".to_owned()),
        "wrong_locale_import_field" => {
            let field = arg_value(args, "field")?;
            Some(format!(
                "import field `{field}` is not declared on this reader surface"
            ))
        }
        "duplicate_import_field" => {
            let field = arg_value(args, "field")?;
            Some(format!("duplicate import field `{field}`"))
        }
        "missing_import_source" => Some("import record requires an ex source".to_owned()),
        "mixed_wildcard_and_named_import" => {
            Some("wildcard import cannot also name a binding".to_owned())
        }
        "invalid_import_visibility" => Some("invalid import visibility".to_owned()),
        "import_privata_removed" => {
            Some("`privata` on imports was removed; imports do not re-export by default".to_owned())
        }
        "import_name_inference_failed" => {
            let path = arg_value(args, "path")?;
            Some(format!("could not infer import name from {path}"))
        }
        "import_target_unknown" => {
            let target = arg_value(args, "target")?;
            Some(format!("`{target}` is not a target name"))
        }
        "import_target_binding_required" => Some(
            "a target import names its binding; none is inferred from the specifier".to_owned(),
        ),
        _ => None,
    }
}

fn invalid_import_help(args: &[DiagnosticArg]) -> Option<String> {
    let issue = arg_value(args, "issue")?;
    match issue {
        "unknown_import_field" => Some("use ex, visibilitas, nomen, ut, or omnia".to_owned()),
        "wrong_locale_import_field" => {
            let expected = arg_value(args, "expected")?;
            Some(format!("use `{expected}` for this file's reader locale"))
        }
        "duplicate_import_field" => {
            Some("each import field may appear once; remove the repeated field".to_owned())
        }
        "missing_import_source" => Some("add ex = \"path\" to the import record".to_owned()),
        "mixed_wildcard_and_named_import" => {
            Some("use omnia by itself, or use nomen/ut for a named import".to_owned())
        }
        "invalid_import_visibility" => Some("use publica".to_owned()),
        "import_privata_removed" => Some("drop the marker, or use publica to re-export".to_owned()),
        "import_name_inference_failed" => {
            let path = arg_value(args, "path")?;
            Some(format!("spell nomen or an explicit alias for {path}"))
        }
        "import_target_unknown" => match arg_value(args, "suggestion") {
            Some(suggestion) => Some(format!("did you mean `{suggestion}`?")),
            None => Some("write a target name from `faber targets`".to_owned()),
        },
        "import_target_binding_required" => Some(
            "write a binding name after the specifier, e.g. `import python \"math\" m`".to_owned(),
        ),
        _ => None,
    }
}
