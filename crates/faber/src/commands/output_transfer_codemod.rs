//! `faber output-transfer-codemod` — migrate the admitted normal-output
//! statement shape to an imported `⇇` transfer.
//!
//! This command deliberately stays at the Faber command seam. It recognizes
//! only source-level output statements whose spelling belongs to the source's
//! own reader locale, rewrites their payloads, and sends the result through
//! the same HIR-backed canonical emitter as `faber format`. Anything outside
//! that small admitted shape is left alone and receives a machine-readable
//! decline reason.

use radix::module::lexer::{lex, Token, TokenKind};
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Arguments for `faber output-transfer-codemod`.
#[derive(Debug, Clone)]
pub struct OutputTransferCodemodCommand {
    pub paths: Vec<PathBuf>,
    pub check: bool,
    pub stdout: bool,
    pub write: bool,
    pub stdin: bool,
}

/// A stable, structured explanation for a shape the codemod declined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeclineReason {
    pub code: String,
    pub message: String,
    pub start: u32,
    pub end: u32,
}

/// Result of reducing and canonically re-emitting one source document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodemodReport {
    pub source: String,
    pub rewritten_statements: usize,
    pub declines: Vec<DeclineReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    English,
    Latin,
    Unsupported,
}

impl Surface {
    fn output_word(self) -> &'static str {
        match self {
            Self::English => "print",
            Self::Latin => "nota",
            Self::Unsupported => "",
        }
    }

    fn unsupported_words(self) -> &'static [&'static str] {
        match self {
            Self::English => &["warn", "debug", "write"],
            Self::Latin => &["mone", "vide", "scribe"],
            Self::Unsupported => &[],
        }
    }

    fn header(self) -> Option<&'static str> {
        match self {
            Self::English => Some("import from \"norma:console\" const write_partial as output\n"),
            Self::Latin => Some("importa ex \"norma:console\" fixum dic ut output\n"),
            Self::Unsupported => None,
        }
    }
}

#[derive(Debug, Clone)]
struct Replacement {
    start: u32,
    end: u32,
    text: String,
}

/// Run the codemod command over files, matching the `format` command's
/// stdout/check/in-place contract.
pub fn cmd_output_transfer_codemod(command: &OutputTransferCodemodCommand) {
    // `--write` is the explicit spelling of the in-place default. Keeping the
    // field here makes the command shape parallel `faber format`.
    let _write = command.write;
    if command.stdin {
        let mut source = String::new();
        if let Err(err) = std::io::stdin().read_to_string(&mut source) {
            eprintln!("error: failed to read <stdin>: {err}");
            std::process::exit(1);
        }
        let path = Path::new("<stdin>");
        let report = match codemod_single_doc(path, &source) {
            Ok(report) => report,
            Err(message) => {
                eprintln!("error: {message}");
                std::process::exit(1);
            }
        };
        print_declines(path, &report);
        print!("{}", report.source);
        return;
    }

    let files = match super::format::resolve_convert_paths(&command.paths) {
        Ok(files) => files,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    };
    if files.is_empty() {
        eprintln!("error: no .fab files found for output-transfer codemod");
        std::process::exit(1);
    }
    if command.stdout && files.len() > 1 {
        eprintln!(
            "error: --stdout accepts exactly one input file, found {}; codemod one file at a time",
            files.len()
        );
        std::process::exit(1);
    }

    let mut drift_count = 0usize;
    let mut error_count = 0usize;
    for path in &files {
        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("error: failed to read '{}': {err}", path.display());
                error_count += 1;
                continue;
            }
        };
        let report = match codemod_single_doc(path, &source) {
            Ok(report) => report,
            Err(message) => {
                eprintln!("error: {}: {message}", path.display());
                error_count += 1;
                continue;
            }
        };
        print_declines(path, &report);

        if command.check {
            if report.source != source {
                eprintln!("would codemod {}", path.display());
                drift_count += 1;
            }
            continue;
        }
        if command.stdout {
            print!("{}", report.source);
            continue;
        }
        if fs::write(path, &report.source).is_err() {
            eprintln!(
                "error: failed to write '{}': codemod output",
                path.display()
            );
            error_count += 1;
        }
    }

    if command.check {
        if drift_count > 0 {
            eprintln!("{drift_count} file(s) would be codemodded");
            std::process::exit(1);
        }
        if error_count > 0 {
            std::process::exit(1);
        }
        return;
    }
    if error_count > 0 {
        std::process::exit(1);
    }
}

fn print_declines(path: &Path, report: &CodemodReport) {
    for decline in &report.declines {
        let json = serde_json::to_string(decline).unwrap_or_else(|_| {
            format!(
                "{{\"code\":\"{}\",\"message\":\"{}\",\"start\":{},\"end\":{}}}",
                decline.code, decline.message, decline.start, decline.end
            )
        });
        eprintln!("declined {}: {json}", path.display());
    }
}

/// Reduce and re-emit one document through `reemit_faber`.
#[cfg(feature = "hir-faber")]
pub fn codemod_single_doc(path: &Path, source: &str) -> Result<CodemodReport, String> {
    let (rewritten, rewritten_statements, declines) = rewrite_source(source)?;
    let session = super::format::format_session(path, None, &rewritten)
        .map_err(|diagnostic| diagnostic.message.clone())?;
    let name = path.display().to_string();
    let result = radix::module::codegen::faber::reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("codemod emission locale pack"),
        &name,
        &rewritten,
    );

    for diagnostic in &result.diagnostics {
        if !diagnostic.is_error() {
            eprintln!("warning: {}: {}", path.display(), diagnostic.message);
        }
    }
    if result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.is_error())
    {
        let diagnostics = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.is_error())
            .map(|diagnostic| diagnostic.message.clone())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!("canonical codemod failed: {diagnostics}"));
    }
    let output = result
        .output
        .ok_or_else(|| "canonical codemod produced no output".to_owned())?;
    let source = super::format::formatted_source_for_write(path, source, &output.code)?;
    Ok(CodemodReport {
        source,
        rewritten_statements,
        declines,
    })
}

#[cfg(not(feature = "hir-faber"))]
pub fn codemod_single_doc(_path: &Path, _source: &str) -> Result<CodemodReport, String> {
    Err("output-transfer codemod requires a faber build with feature `hir-faber`".to_owned())
}

#[cfg(feature = "hir-faber")]
fn rewrite_source(source: &str) -> Result<(String, usize, Vec<DeclineReason>), String> {
    let split = radix::module::driver::split_frontmatter(source)
        .map_err(|error| format!("invalid frontmatter: {error}"))?;
    let body_offset = split.body_byte_offset as usize;
    let body = &source[body_offset..];
    let surface = source_surface(source, body);
    let lexed = lex(body);
    let mut declines = Vec::new();
    if !lexed.errors.is_empty() {
        declines.push(DeclineReason {
            code: "lex_error".to_owned(),
            message: "source could not be safely tokenized; no output shape was guessed".to_owned(),
            start: body_offset as u32,
            end: source.len() as u32,
        });
        return Ok((source.to_owned(), 0, declines));
    }

    // The None arm covers every surface without a safe source-local header
    // (unknown reader locale): such sources decline instead of panicking.
    let Some(header) = surface.header() else {
        if lexed
            .tokens
            .iter()
            .any(|token| token_text(body, token) == "print" || token_text(body, token) == "nota")
        {
            declines.push(reason(
                "unsupported_locale",
                "the codemod has no safe source-local header for this reader locale",
                body_offset,
                0,
                source.len() - body_offset,
            ));
        }
        return Ok((source.to_owned(), 0, declines));
    };

    if let Some(token) = lexed
        .tokens
        .iter()
        .find(|token| token_text(body, token) == "\"norma:console\"")
    {
        declines.push(reason(
            "existing_console_import",
            "an existing console import was not merged by guesswork",
            body_offset,
            token.span.start as usize,
            token.span.end as usize,
        ));
        return Ok((source.to_owned(), 0, declines));
    }

    if let Some(token) = lexed
        .tokens
        .iter()
        .find(|token| token_text(body, token) == "output")
    {
        declines.push(reason(
            "output_alias_collision",
            "the fixed codemod alias `output` is already used in this source",
            body_offset,
            token.span.start as usize,
            token.span.end as usize,
        ));
        return Ok((source.to_owned(), 0, declines));
    }

    let mut replacements = Vec::new();
    let mut index = 0usize;
    while index < lexed.tokens.len() {
        let token = &lexed.tokens[index];
        let word = token_text(body, token);
        let is_output = word == surface.output_word();
        let is_unsupported = surface.unsupported_words().contains(&word);
        if !is_output && !is_unsupported {
            index += 1;
            continue;
        }
        if !at_statement_boundary(&lexed.tokens, index) {
            index += 1;
            continue;
        }

        let end = statement_end(&lexed.tokens, index + 1);
        let next = next_code_index(&lexed.tokens, index + 1, end);
        if is_output
            && next.is_some_and(|next| matches!(lexed.tokens[next].kind, TokenKind::LParen))
        {
            declines.push(reason(
                "output_call_not_statement",
                "a call-shaped expression was not treated as an output statement",
                body_offset,
                token.span.start as usize,
                lexed.tokens[end.saturating_sub(1)].span.end as usize,
            ));
            index = end.max(index + 1);
            continue;
        }
        if is_unsupported {
            declines.push(reason(
                "unsupported_output_kind",
                "only the normal print/nota output statement maps to write_partial",
                body_offset,
                token.span.start as usize,
                lexed.tokens[end.saturating_sub(1)].span.end as usize,
            ));
            index = end.max(index + 1);
            continue;
        }

        let Some((args, last_end)) = statement_arguments(body, &lexed.tokens, index + 1, end)
        else {
            declines.push(reason(
                "unsupported_value_shape",
                "output values must be one or more complete, single-line expressions",
                body_offset,
                token.span.start as usize,
                lexed.tokens[end.saturating_sub(1)].span.end as usize,
            ));
            index = end.max(index + 1);
            continue;
        };
        if args.is_empty() {
            declines.push(reason(
                "empty_output_values",
                "an output statement must contain at least one value",
                body_offset,
                token.span.start as usize,
                last_end as usize,
            ));
            index = end.max(index + 1);
            continue;
        }

        let holes = std::iter::repeat("§")
            .take(args.len())
            .collect::<Vec<_>>()
            .join(" ");
        let constructor = format!("output ⇇ \"{holes}\\n\"({})", args.join(", "));
        replacements.push(Replacement {
            start: token.span.start,
            end: last_end,
            text: constructor,
        });
        index = end.max(index + 1);
    }

    if replacements.is_empty() {
        return Ok((source.to_owned(), 0, declines));
    }

    let mut rewritten_body = body.to_owned();
    for replacement in replacements.iter().rev() {
        let start = replacement.start as usize;
        let end = replacement.end as usize;
        rewritten_body.replace_range(start..end, &replacement.text);
    }
    let mut rewritten = String::with_capacity(source.len() + 96);
    rewritten.push_str(&source[..body_offset]);
    rewritten.push_str(header);
    rewritten.push_str(&rewritten_body);
    Ok((rewritten, replacements.len(), declines))
}

#[cfg(feature = "hir-faber")]
fn source_surface(source: &str, body: &str) -> Surface {
    let locale = radix::module::driver::split_frontmatter(source)
        .ok()
        .and_then(|split| split.frontmatter_text)
        .and_then(|frontmatter| {
            frontmatter.lines().find_map(|line| {
                let value = line
                    .trim()
                    .strip_prefix("locale")?
                    .split_once('=')?
                    .1
                    .trim();
                Some(value.trim_matches(['\"', '\'']).to_owned())
            })
        });
    match locale.as_deref() {
        Some(locale) if locale == "la" || locale.starts_with("la-") => Surface::Latin,
        Some(locale) if locale == "en" || locale.starts_with("en-") => Surface::English,
        Some(_) => Surface::Unsupported,
        None => {
            let lexed = lex(body);
            if lexed
                .tokens
                .iter()
                .any(|token| token_text(body, token) == "nota")
            {
                Surface::Latin
            } else {
                Surface::English
            }
        }
    }
}

#[cfg(feature = "hir-faber")]
fn token_text<'a>(source: &'a str, token: &Token) -> &'a str {
    &source[token.span.start as usize..token.span.end as usize]
}

#[cfg(feature = "hir-faber")]
fn is_trivia(token: &Token) -> bool {
    matches!(token.kind, TokenKind::LineComment(_) | TokenKind::Newline)
}

#[cfg(feature = "hir-faber")]
fn next_code_index(tokens: &[Token], mut index: usize, end: usize) -> Option<usize> {
    while index < end && is_trivia(&tokens[index]) {
        index += 1;
    }
    (index < end).then_some(index)
}

#[cfg(feature = "hir-faber")]
fn at_statement_boundary(tokens: &[Token], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    let mut cursor = index;
    while cursor > 0 && is_trivia(&tokens[cursor - 1]) {
        if matches!(tokens[cursor - 1].kind, TokenKind::Newline) {
            return true;
        }
        cursor -= 1;
    }
    matches!(
        tokens
            .get(cursor.saturating_sub(1))
            .map(|token| &token.kind),
        Some(TokenKind::LBrace | TokenKind::RBrace | TokenKind::Semicolon)
    )
}

#[cfg(feature = "hir-faber")]
fn statement_end(tokens: &[Token], start: usize) -> usize {
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    for index in start..tokens.len() {
        match tokens[index].kind {
            TokenKind::LParen => parens += 1,
            TokenKind::RParen => parens = parens.saturating_sub(1),
            TokenKind::LBracket => brackets += 1,
            TokenKind::RBracket => brackets = brackets.saturating_sub(1),
            TokenKind::LBrace => braces += 1,
            TokenKind::RBrace if braces > 0 => braces -= 1,
            TokenKind::RBrace | TokenKind::Semicolon | TokenKind::Eof
                if parens == 0 && brackets == 0 && braces == 0 =>
            {
                return index;
            }
            TokenKind::Newline if parens == 0 && brackets == 0 && braces == 0 => return index,
            TokenKind::LineComment(_) if parens == 0 && brackets == 0 && braces == 0 => {
                return index;
            }
            _ => {}
        }
    }
    tokens.len().saturating_sub(1)
}

#[cfg(feature = "hir-faber")]
fn statement_arguments<'a>(
    source: &'a str,
    tokens: &'a [Token],
    start: usize,
    end: usize,
) -> Option<(Vec<String>, u32)> {
    let mut significant = Vec::new();
    for index in start..end {
        if is_trivia(&tokens[index]) {
            return None;
        }
        significant.push(index);
    }
    if significant.is_empty() {
        return Some((Vec::new(), tokens[start.saturating_sub(1)].span.end));
    }

    let mut parts = Vec::<(usize, usize)>::new();
    let mut part_start = 0usize;
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    for (position, index) in significant.iter().copied().enumerate() {
        match tokens[index].kind {
            TokenKind::LParen => parens += 1,
            TokenKind::RParen => parens = parens.checked_sub(1)?,
            TokenKind::LBracket => brackets += 1,
            TokenKind::RBracket => brackets = brackets.checked_sub(1)?,
            TokenKind::LBrace => braces += 1,
            TokenKind::RBrace => braces = braces.checked_sub(1)?,
            TokenKind::Comma if parens == 0 && brackets == 0 && braces == 0 => {
                if part_start == position {
                    return None;
                }
                parts.push((part_start, position));
                part_start = position + 1;
            }
            _ => {}
        }
    }
    if part_start == significant.len() {
        return None;
    }
    parts.push((part_start, significant.len()));
    if parens != 0 || brackets != 0 || braces != 0 {
        return None;
    }

    let mut args = Vec::with_capacity(parts.len());
    let mut last_end = 0u32;
    for (start, end) in parts {
        let first = &tokens[significant[start]];
        let last = &tokens[significant[end - 1]];
        let arg = source[first.span.start as usize..last.span.end as usize]
            .trim()
            .to_owned();
        if arg.is_empty() || arg.contains('\n') || arg.contains('\r') {
            return None;
        }
        args.push(arg);
        last_end = last.span.end;
    }
    Some((args, last_end))
}

#[cfg(feature = "hir-faber")]
fn reason(
    code: &str,
    message: &str,
    body_offset: usize,
    start: usize,
    end: usize,
) -> DeclineReason {
    DeclineReason {
        code: code.to_owned(),
        message: message.to_owned(),
        start: (body_offset + start) as u32,
        end: (body_offset + end) as u32,
    }
}

#[cfg(test)]
#[path = "output_transfer_codemod_test.rs"]
mod tests;
