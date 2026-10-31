//! Hand-written JSON builders for CLI inspection command output.
//!
//! Owns the narrow, deterministic JSON serializers used by [`super::inspect`]
//! and [`escape_json`] for string safety. Delegates no compilation; this is a
//! pure formatting surface that produces machine-readable inspection payloads
//! without depending on a serialization framework contract.

use crate::diagnostics::DiagnosticConvert;
use std::fmt::Write;
pub(crate) fn frontmatter_json(frontmatter: Option<&crate::driver::FileFrontmatter>) -> String {
    match frontmatter {
        None => "null".to_owned(),
        Some(frontmatter) => serde_json::to_string(frontmatter).unwrap_or_else(|_| "null".to_owned()),
    }
}

pub(crate) fn leading_trivia_json(trivia: &[crate::syntax::Trivia], interner: &crate::lexer::Interner) -> String {
    trivia
        .iter()
        .map(|item| match item {
            crate::syntax::Trivia::CommentLine(comment) => format!(
                "{{ \"kind\": \"CommentLine\", \"text\": \"{}\", \"span\": [{}, {}] }}",
                escape_json(interner.resolve(comment.text)),
                comment.span.start,
                comment.span.end
            ),
            crate::syntax::Trivia::Newline(newline) => format!(
                "{{ \"kind\": \"Newline\", \"span\": [{}, {}] }}",
                newline.span.start, newline.span.end
            ),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// HIR presentation sidecar inspection JSON (U-6).
///
/// Serializes the presentation layer — typed owners (anchor + kind + semantic
/// back-reference), ordered trivia attachments, structural block ends, the
/// program end, and the entry-start anchor — into the same narrow, hand-written
/// JSON surface as the other inspection builders. Debug tooling only; never
/// changes semantic behavior.
pub(crate) fn presentation_json(
    presentation: &radix_hir::HirPresentation,
    interner: &crate::lexer::Interner,
) -> String {
    let mut out = String::new();
    out.push_str("{\n");

    out.push_str("  \"owners\": [\n");
    for (i, owner) in presentation.owners.iter().enumerate() {
        let comma = if i + 1 < presentation.owners.len() { "," } else { "" };
        let semantic = match &owner.semantic {
            Some(radix_hir::HirOwnerRef::Hir(id)) => format!("{{ \"hir\": {} }}", id.0),
            Some(radix_hir::HirOwnerRef::Def(def)) => format!("{{ \"def\": {} }}", def.0),
            None => "null".to_owned(),
        };
        writeln!(
            out,
            "    {{ \"anchor\": {}, \"kind\": \"{}\", \"semantic\": {} }}{}",
            owner.id.0,
            hir_owner_kind_name(&owner.kind),
            semantic,
            comma
        )
        .ok();
    }
    out.push_str("  ],\n");

    out.push_str("  \"attachments\": [\n");
    for (i, attachment) in presentation.attachments.iter().enumerate() {
        let comma = if i + 1 < presentation.attachments.len() {
            ","
        } else {
            ""
        };
        writeln!(
            out,
            "    {{ \"anchor\": {}, \"leading\": [{}] }}{}",
            attachment.anchor.0,
            hir_trivia_json(&attachment.leading, interner),
            comma
        )
        .ok();
    }
    out.push_str("  ],\n");

    let block_ends = presentation
        .block_ends
        .iter()
        .map(|end| {
            format!(
                "{{ \"anchor\": {anchor}, \"span\": [{start}, {finish}] }}",
                anchor = end.id.0,
                start = end.span.start,
                finish = end.span.end
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    writeln!(out, "  \"block_ends\": [{block_ends}],").ok();

    let program_end = &presentation.program_end;
    writeln!(
        out,
        "  \"program_end\": {{ \"anchor\": {}, \"span\": [{}, {}] }},",
        program_end.id.0, program_end.span.start, program_end.span.end
    )
    .ok();

    match &presentation.entry_start_anchor {
        Some(anchor) => {
            writeln!(out, "  \"entry_start_anchor\": {}", anchor.0).ok();
        }
        None => {
            writeln!(out, "  \"entry_start_anchor\": null").ok();
        }
    }

    write!(out, "}}").ok();
    out
}

/// Owner-kind name for inspection JSON.
fn hir_owner_kind_name(kind: &radix_hir::HirTriviaOwnerKind) -> &'static str {
    match kind {
        radix_hir::HirTriviaOwnerKind::Item => "Item",
        radix_hir::HirTriviaOwnerKind::Statement => "Statement",
        radix_hir::HirTriviaOwnerKind::Member => "Member",
        radix_hir::HirTriviaOwnerKind::UnionSharedField { .. } => "UnionSharedField",
        radix_hir::HirTriviaOwnerKind::Entry => "Entry",
        radix_hir::HirTriviaOwnerKind::BlockEnd => "BlockEnd",
        radix_hir::HirTriviaOwnerKind::ProgramEnd => "ProgramEnd",
        radix_hir::HirTriviaOwnerKind::Region => "Region",
    }
}

/// Ordered HIR trivia entries for inspection JSON.
fn hir_trivia_json(trivia: &[radix_hir::HirTrivia], interner: &crate::lexer::Interner) -> String {
    trivia
        .iter()
        .map(|item| match item {
            radix_hir::HirTrivia::CommentLine { text, span } => format!(
                "{{ \"kind\": \"CommentLine\", \"text\": \"{}\", \"span\": [{}, {}] }}",
                escape_json(interner.resolve(*text)),
                span.start,
                span.end
            ),
            radix_hir::HirTrivia::Newline { span } => {
                format!("{{ \"kind\": \"Newline\", \"span\": [{}, {}] }}", span.start, span.end)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn annotation_json(annotations: &[crate::syntax::Annotation]) -> String {
    annotations
        .iter()
        .map(|annotation| {
            let kind = match &annotation.kind {
                crate::syntax::AnnotationKind::Cli(_) => "Cli",
                crate::syntax::AnnotationKind::Imperium(_) => "Imperium",
                crate::syntax::AnnotationKind::Optio(_) => "Optio",
                crate::syntax::AnnotationKind::Operandus(_) => "Operandus",
                crate::syntax::AnnotationKind::RadixTypusDomain(_) => "RadixTypusDomain",
                crate::syntax::AnnotationKind::RadixLane(_) => "RadixLane",
                crate::syntax::AnnotationKind::Nucleum(_) => "Nucleum",
                crate::syntax::AnnotationKind::Conversio => "Conversio",
                crate::syntax::AnnotationKind::Ad(_) => "Ad",
                crate::syntax::AnnotationKind::Statement(_) => "Statement",
                crate::syntax::AnnotationKind::Futura => "Futura",
                crate::syntax::AnnotationKind::Cursor => "Cursor",
                crate::syntax::AnnotationKind::Tag => "Tag",
                crate::syntax::AnnotationKind::Solum => "Solum",
                crate::syntax::AnnotationKind::Omitte => "Omitte",
                crate::syntax::AnnotationKind::Metior => "Metior",
                crate::syntax::AnnotationKind::Publica => "Publica",
                crate::syntax::AnnotationKind::Protecta => "Protecta",
                crate::syntax::AnnotationKind::Privata => "Privata",
                crate::syntax::AnnotationKind::Interna => "Interna",
                crate::syntax::AnnotationKind::Braced(_) => "Braced",
            };
            format!(
                "{{ \"kind\": \"{}\", \"span\": [{}, {}] }}",
                kind, annotation.span.start, annotation.span.end
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn cli_analysis_json(analysis: &crate::cli::CliAnalysis) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    writeln!(out, "  \"mode\": \"{}\",", cli_mode_name(&analysis.mode)).ok();
    writeln!(out, "  \"success\": {},", analysis.errors.is_empty()).ok();
    if let Some(program) = &analysis.program {
        out.push_str("  \"program\": ");
        out.push_str(&cli_program_json(program, 2));
        out.push_str(",\n");
    } else {
        out.push_str("  \"program\": null,\n");
    }
    write!(out, "  \"errors\": {}\n}}", cli_analysis_errors_json(&analysis.errors)).ok();
    out
}

fn cli_analysis_errors_json(errors: &[crate::semantic::SemanticError]) -> String {
    format!(
        "[{}]",
        errors
            .iter()
            .map(cli_analysis_error_json)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn cli_analysis_error_json(err: &crate::semantic::SemanticError) -> String {
    let diagnostic = crate::diagnostics::Diagnostic::from_semantic_error("<cli>", "", err);
    format!(
        "{{ \"message\": \"{}\", \"span\": [{}, {}] }}",
        escape_json(&diagnostic.message),
        err.span.start,
        err.span.end
    )
}

fn cli_program_json(program: &crate::cli::CliProgram, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let inner = " ".repeat(indent + 2);
    let mut out = String::new();
    out.push_str("{\n");
    writeln!(out, "{inner}\"name\": \"{}\",", escape_json(&program.name)).ok();
    writeln!(out, "{inner}\"entry_args\": \"{}\",", escape_json(&program.entry_args)).ok();
    writeln!(out, "{inner}\"mode\": \"{}\",", cli_mode_name(&program.mode)).ok();
    writeln!(out, "{inner}\"version\": {},", json_string_opt(program.version.as_deref())).ok();
    writeln!(
        out,
        "{inner}\"description\": {},",
        json_string_opt(program.description.as_deref())
    )
    .ok();
    writeln!(out, "{inner}\"global_options\": {},", cli_options_json(&program.global_options)).ok();
    writeln!(
        out,
        "{inner}\"global_operands\": {},",
        cli_operands_json(&program.global_operands)
    )
    .ok();
    writeln!(out, "{inner}\"options\": {},", cli_options_json(&program.options)).ok();
    writeln!(out, "{inner}\"operands\": {},", cli_operands_json(&program.operands)).ok();
    writeln!(out, "{inner}\"commands\": {}", cli_commands_json(&program.commands)).ok();
    write!(out, "{pad}}}").ok();
    out
}

fn cli_commands_json(commands: &[crate::cli::CliCommand]) -> String {
    format!(
        "[{}]",
        commands
            .iter()
            .map(cli_command_json)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn cli_command_json(command: &crate::cli::CliCommand) -> String {
    format!(
        "{{ \"path\": [{}], \"function\": \"{}\", \"aliases\": [{}], \"description\": {}, \"options\": {}, \"operands\": {} }}",
        json_string_array_items(&command.path),
        escape_json(&command.function),
        json_string_array_items(&command.aliases),
        json_string_opt(command.description.as_deref()),
        cli_options_json(&command.options),
        cli_operands_json(&command.operands)
    )
}

fn cli_options_json(options: &[crate::cli::CliOption]) -> String {
    format!(
        "[{}]",
        options
            .iter()
            .map(cli_option_json)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn cli_option_json(option: &crate::cli::CliOption) -> String {
    format!(
        "{{ \"binding\": \"{}\", \"type\": \"{}\", \"short\": {}, \"long\": {}, \"global\": {}, \"flag\": {}, \"default\": {} }}",
        escape_json(&option.binding),
        cli_type_name(&option.ty),
        json_string_opt(option.short.as_deref()),
        json_string_opt(option.long.as_deref()),
        option.global,
        option.flag,
        cli_default_json(option.default.as_ref())
    )
}

fn cli_operands_json(operands: &[crate::cli::CliOperand]) -> String {
    format!(
        "[{}]",
        operands
            .iter()
            .map(cli_operand_json)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn cli_operand_json(operand: &crate::cli::CliOperand) -> String {
    format!(
        "{{ \"binding\": \"{}\", \"type\": \"{}\", \"rest\": {}, \"global\": {}, \"default\": {} }}",
        escape_json(&operand.binding),
        cli_type_name(&operand.ty),
        operand.rest,
        operand.global,
        cli_default_json(operand.default.as_ref())
    )
}

fn cli_default_json(default: Option<&crate::cli::CliDefault>) -> String {
    match default {
        Some(crate::cli::CliDefault::Text(value)) => {
            format!("{{ \"kind\": \"text\", \"value\": \"{}\" }}", escape_json(value))
        }
        Some(crate::cli::CliDefault::Integer(value)) => format!("{{ \"kind\": \"integer\", \"value\": {value} }}"),
        Some(crate::cli::CliDefault::Float(value)) => format!("{{ \"kind\": \"float\", \"value\": {value} }}"),
        Some(crate::cli::CliDefault::Bool(value)) => format!("{{ \"kind\": \"bool\", \"value\": {value} }}"),
        Some(crate::cli::CliDefault::Nil) => "{ \"kind\": \"nil\" }".to_owned(),
        Some(crate::cli::CliDefault::Expr(value)) => {
            format!("{{ \"kind\": \"expr\", \"value\": \"{}\" }}", escape_json(value))
        }
        None => "null".to_owned(),
    }
}

fn cli_mode_name(mode: &crate::cli::CliMode) -> &'static str {
    match mode {
        crate::cli::CliMode::NotCli => "not-cli",
        crate::cli::CliMode::SingleCommand => "single-command",
        crate::cli::CliMode::Subcommand => "subcommand",
    }
}

fn cli_type_name(ty: &crate::cli::CliType) -> &'static str {
    match ty {
        crate::cli::CliType::Textus => "textus",
        crate::cli::CliType::Numerus => "numerus",
        crate::cli::CliType::Fractus => "fractus",
        crate::cli::CliType::Bivalens => "bivalens",
        crate::cli::CliType::Octeti => "lista<b8>",
        crate::cli::CliType::Ignotum => "ignotum",
        crate::cli::CliType::ListaTextus => "lista<textus>",
        crate::cli::CliType::ListaNumerus => "lista<numerus>",
    }
}

fn json_string_opt(value: Option<&str>) -> String {
    match value {
        Some(value) => format!("\"{}\"", escape_json(value)),
        None => "null".to_owned(),
    }
}

fn json_string_array_items(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("\"{}\"", escape_json(value)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Escape special characters for hand-written `JSON` inspection strings.
///
/// The inspection commands use narrow, deterministic `JSON` builders instead of a
/// public serialization contract. This helper keeps those surfaces valid for
/// strings that come from source text or diagnostics.
#[must_use]
pub(crate) fn escape_json(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => {
                write!(escaped, "\\u{:04x}", ch as u32).ok();
            }
            ch => escaped.push(ch),
        }
    }
    escaped
}

#[cfg(test)]
#[path = "json_test.rs"]
mod tests;
