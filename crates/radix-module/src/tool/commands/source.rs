//! Source I/O and location formatting for CLI command handlers.
//!
//! Owns [`read_source`] (stdin / file reading with process-exit on I/O failure),
//! [`source_file_from_input`], and line-column formatting helpers
//! ([`format_location`], [`format_optional_location`]) plus message constructors
//! that delegate to `crate::diagnostics::DiagnosticConvert`. This is the text
//! loading gateway for every command that accepts source input.

use crate::diagnostics::DiagnosticConvert;
use std::io::{self, Read};
use std::path::PathBuf;

/// Read source from a file argument or stdin.
///
/// This is the process-facing loader for single-file commands. It exits on I/O
/// failure because command handlers already report diagnostics through stderr,
/// while library callers should use `Compiler` directly for non-exiting flows.
#[must_use]
pub fn read_source(args: &[String]) -> (String, String) {
    if args.is_empty() || args[0] == "-" {
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .unwrap_or_else(|err| {
                eprintln!("error: failed to read stdin: {err}");
                std::process::exit(1);
            });
        ("<stdin>".to_owned(), source)
    } else {
        let path = PathBuf::from(&args[0]);
        let source = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("error: cannot read '{}': {}", path.display(), e);
            std::process::exit(1);
        });
        (args[0].clone(), source)
    }
}

// consumed by SourceFile::inline
pub(crate) fn source_file_from_input(name: String, source: String) -> crate::driver::SourceFile {
    crate::driver::SourceFile::inline(name, source).unwrap_or_else(|error| {
        eprintln!("error: {error}");
        std::process::exit(1);
    })
}

/// Format an offset in a loaded source file for terminal diagnostics.
#[must_use]
pub fn format_location(source_file: &crate::driver::SourceFile, offset: u32) -> String {
    let (line, column) = source_file.offset_to_line_col(offset);
    format!("{}:{}:{}", source_file.name.as_str(), line, column)
}

pub(crate) fn parse_error_message(source_file: &crate::driver::SourceFile, err: &crate::parser::ParseError) -> String {
    crate::diagnostics::Diagnostic::from_parse_error(&source_file.name, &source_file.content, err).message
}

pub(crate) fn lex_error_message(source_file: &crate::driver::SourceFile, err: &crate::lexer::LexError) -> String {
    crate::diagnostics::Diagnostic::from_lex_error(&source_file.name, &source_file.content, err).message
}

pub(crate) fn lower_error_message(source_file: &crate::driver::SourceFile, err: &crate::hir::LowerError) -> String {
    let mut semantic_error =
        crate::semantic::SemanticError::structured(crate::semantic::SemanticErrorKind::LoweringError, err.span);
    semantic_error.args.extend(err.args.clone());
    crate::diagnostics::Diagnostic::from_semantic_error(&source_file.name, &source_file.content, &semantic_error)
        .message
}

pub(crate) fn semantic_error_message(
    source_file: &crate::driver::SourceFile,
    err: &crate::semantic::SemanticError,
) -> String {
    crate::diagnostics::Diagnostic::from_semantic_error(&source_file.name, &source_file.content, err).message
}

#[must_use]
pub(crate) fn format_optional_location(
    source_file: &crate::driver::SourceFile,
    span: Option<crate::lexer::Span>,
) -> String {
    span.map_or_else(|| source_file.name.clone(), |span| format_location(source_file, span.start))
}
