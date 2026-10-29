//! Clap-free phase-inspection services. Printing and process exit live in `radix-cli`.

use super::json::{
    annotation_json, cli_analysis_json, escape_json, frontmatter_json, leading_trivia_json, presentation_json,
};
use crate::diagnostics::DiagnosticConvert;
use std::fmt::Write;

/// Inspection payload plus success. Callers print `text` and exit on `!success`.
#[derive(Debug)]
pub struct SourceInspection {
    pub text: String,
    pub success: bool,
    pub errors: Vec<String>,
}

fn source_file(name: impl Into<String>, source: String) -> Result<crate::driver::SourceFile, String> {
    crate::driver::SourceFile::inline(name, source).map_err(|error| error.to_string())
}

fn format_location(source_file: &crate::driver::SourceFile, offset: u32) -> String {
    let (line, column) = source_file.offset_to_line_col(offset);
    format!("{}:{}:{}", source_file.name.as_str(), line, column)
}

fn format_optional_location(source_file: &crate::driver::SourceFile, span: Option<crate::lexer::Span>) -> String {
    match span {
        Some(span) => format_location(source_file, span.start),
        None => source_file.name.clone(),
    }
}

fn lex_error_message(source_file: &crate::driver::SourceFile, err: &crate::lexer::LexError) -> String {
    crate::diagnostics::Diagnostic::from_lex_error(&source_file.name, &source_file.content, err).message
}

fn parse_error_message(source_file: &crate::driver::SourceFile, err: &crate::parser::ParseError) -> String {
    crate::diagnostics::Diagnostic::from_parse_error(&source_file.name, &source_file.content, err).message
}

fn lower_error_message(source_file: &crate::driver::SourceFile, err: &crate::hir::LowerError) -> String {
    let mut semantic_error =
        crate::semantic::SemanticError::structured(crate::semantic::SemanticErrorKind::LoweringError, err.span);
    semantic_error.args.extend(err.args.clone());
    crate::diagnostics::Diagnostic::from_semantic_error(&source_file.name, &source_file.content, &semantic_error)
        .message
}

fn semantic_error_message(source_file: &crate::driver::SourceFile, err: &crate::semantic::SemanticError) -> String {
    crate::diagnostics::Diagnostic::from_semantic_error(&source_file.name, &source_file.content, err).message
}

/// Tokenize source and return the lex inspection JSON payload.
#[must_use]
pub fn lex_inspection_for_source(name: &str, source: &str) -> SourceInspection {
    let source_file = match source_file(name.to_owned(), source.to_owned()) {
        Ok(file) => file,
        Err(error) => return SourceInspection { text: String::new(), success: false, errors: vec![error] },
    };
    let result = match lex_reader_surface(&source_file) {
        Ok(result) => result,
        Err(error) => return SourceInspection { text: String::new(), success: false, errors: vec![error] },
    };
    SourceInspection { text: lex_json(&source_file, &result), success: result.success(), errors: Vec::new() }
}

/// Lex the peeled body under the file's own frontmatter reader locale, as check
/// and compile do; no frontmatter locale falls back to the no-locale default.
fn lex_reader_surface(source_file: &crate::driver::SourceFile) -> Result<crate::lexer::LexResult, String> {
    let pack = match crate::program::compile::frontmatter_locale_pack_from_frontmatter(
        &crate::driver::Config::default(),
        source_file.frontmatter.as_ref(),
    ) {
        Some(pack) => pack,
        None => crate::driver::default_code_pack(&crate::driver::Config::default())
            .map_err(|diagnostic| diagnostic.message)?,
    };
    Ok(crate::lexer::lex_with_locale_pack(source_file.content.as_str(), &pack))
}

fn lex_json(source_file: &crate::driver::SourceFile, result: &crate::lexer::LexResult) -> String {
    let mut out = String::new();
    writeln!(out, "{{").ok();
    writeln!(out, "  \"file\": \"{}\",", escape_json(&source_file.name)).ok();
    writeln!(
        out,
        "  \"frontmatter\": {},",
        frontmatter_json(source_file.frontmatter.as_ref())
    )
    .ok();
    writeln!(out, "  \"body_byte_offset\": {},", source_file.body_byte_offset).ok();
    writeln!(out, "  \"success\": {},", result.success()).ok();
    writeln!(out, "  \"tokens\": [").ok();
    for (i, token) in result.tokens.iter().enumerate() {
        let comma = if i + 1 < result.tokens.len() { "," } else { "" };
        writeln!(
            out,
            "    {{ \"kind\": \"{}\", \"span\": [{}, {}]{} }}{}",
            escape_json(&token_kind_display(&token.kind)),
            token.span.start,
            token.span.end,
            ident_fields_json(result, &token.kind),
            comma
        )
        .ok();
    }
    writeln!(out, "  ],").ok();
    writeln!(out, "  \"errors\": [").ok();
    for (i, err) in result.errors.iter().enumerate() {
        let comma = if i + 1 < result.errors.len() { "," } else { "" };
        writeln!(
            out,
            "    {{ \"message\": \"{}\", \"span\": [{}, {}] }}{}",
            escape_json(&lex_error_message(source_file, err)),
            err.span.start,
            err.span.end,
            comma
        )
        .ok();
    }
    writeln!(out, "  ]").ok();
    writeln!(out, "}}").ok();
    out
}

/// Identifier spelling and, when the parser may claim it as a keyword, its
/// Latin registry identity. Keywords lex as identifiers on every surface, so
/// this is the locale-normalized view of the token stream.
fn ident_fields_json(result: &crate::lexer::LexResult, kind: &crate::lexer::TokenKind) -> String {
    let crate::lexer::TokenKind::Ident(symbol) = kind else {
        return String::new();
    };
    let text = result.interner.resolve(*symbol);
    let mut fields = format!(", \"text\": \"{}\"", escape_json(text));
    if let Some(latin) = result.reader_keyword_latin_keys.get(text) {
        write!(fields, ", \"keyword\": \"{}\"", escape_json(latin)).ok();
    }
    fields
}

fn token_kind_display(kind: &crate::lexer::TokenKind) -> String {
    let kind = format!("{kind:?}");
    if kind.len() > 60 {
        format!("{}...", &kind[..57])
    } else {
        kind
    }
}

/// Parse source and return the AST inspection JSON payload.
#[must_use]
pub fn parse_inspection_for_source(name: &str, source: &str) -> SourceInspection {
    let source_file = match source_file(name.to_owned(), source.to_owned()) {
        Ok(file) => file,
        Err(error) => return SourceInspection { text: String::new(), success: false, errors: vec![error] },
    };
    let lex_result = match lex_reader_surface(&source_file) {
        Ok(result) => result,
        Err(error) => return SourceInspection { text: String::new(), success: false, errors: vec![error] },
    };
    if !lex_result.success() {
        return SourceInspection {
            text: String::new(),
            success: false,
            errors: lexer_error_lines(&source_file, &lex_result.errors),
        };
    }
    let parse_result = crate::parser::parse(lex_result);
    SourceInspection {
        text: parse_json(&source_file, &parse_result),
        success: parse_result.success(),
        errors: Vec::new(),
    }
}

fn parse_json(source_file: &crate::driver::SourceFile, parse_result: &crate::parser::ParseResult) -> String {
    let mut out = String::new();
    writeln!(out, "{{").ok();
    writeln!(out, "  \"file\": \"{}\",", escape_json(&source_file.name)).ok();
    writeln!(
        out,
        "  \"frontmatter\": {},",
        frontmatter_json(source_file.frontmatter.as_ref())
    )
    .ok();
    writeln!(out, "  \"body_byte_offset\": {},", source_file.body_byte_offset).ok();
    writeln!(out, "  \"success\": {},", parse_result.success()).ok();
    if let Some(program) = &parse_result.program {
        write_parse_ast_json(&mut out, program, &parse_result.interner);
    } else {
        writeln!(out, "  \"ast\": null,").ok();
    }
    writeln!(out, "  \"errors\": [").ok();
    for (i, err) in parse_result.errors.iter().enumerate() {
        let comma = if i + 1 < parse_result.errors.len() { "," } else { "" };
        writeln!(
            out,
            "    {{ \"message\": \"{}\", \"span\": [{}, {}] }}{}",
            escape_json(&parse_error_message(source_file, err)),
            err.span.start,
            err.span.end,
            comma
        )
        .ok();
    }
    writeln!(out, "  ]").ok();
    writeln!(out, "}}").ok();
    out
}

fn write_parse_ast_json(out: &mut String, program: &crate::syntax::Program, interner: &crate::lexer::Interner) {
    writeln!(out, "  \"statements\": {},", program.statements.len()).ok();
    writeln!(out, "  \"ast\": [").ok();
    for (i, stmt) in program.statements.iter().enumerate() {
        let comma = if i + 1 < program.statements.len() { "," } else { "" };
        let kind = format!("{:?}", stmt.kind);
        writeln!(
            out,
            "    {{ \"id\": {}, \"kind\": \"{}\", \"span\": [{}, {}], \"leading_trivia\": [{}], \"annotations\": [{}] }}{}",
            stmt.id,
            debug_variant_name(&kind),
            stmt.span.start,
            stmt.span.end,
            leading_trivia_json(&stmt.leading_trivia, interner),
            annotation_json(&stmt.annotations),
            comma
        ).ok();
    }
    writeln!(out, "  ],").ok();
}

/// Lower parsed source to HIR and return the inspection JSON payload.
#[must_use]
pub fn hir_inspection_for_source(name: &str, source: &str) -> SourceInspection {
    let source_file = match source_file(name.to_owned(), source.to_owned()) {
        Ok(file) => file,
        Err(error) => return SourceInspection { text: String::new(), success: false, errors: vec![error] },
    };
    let (mut program, mut interner) = match parse_inspect_program(&source_file) {
        Ok(parsed) => parsed,
        Err(errors) => return SourceInspection { text: String::new(), success: false, errors },
    };
    // Imports resolve as check does: the program layer discovers the import
    // contract for an on-disk source (in-memory names keep an empty one).
    let session = crate::driver::Session::new(crate::driver::Config::default().with_dev_locale_root());
    let prepared = match crate::program::prepare_program_module_for_name(&source_file.name, source, &session) {
        Ok(prepared) => prepared,
        Err(diagnostic) => {
            return SourceInspection {
                text: String::new(),
                success: false,
                errors: vec![format!("error: {}", diagnostic.message)],
            };
        }
    };
    let contract = prepared.contract;
    crate::program::apply_default_import_names(&mut program, &mut interner, &contract.default_import_names);
    let (mut resolver, mut types) = match prepare_hir_prerequisites(&source_file, &program, &mut interner, &contract) {
        Ok(ready) => ready,
        Err(errors) => return SourceInspection { text: String::new(), success: false, errors },
    };
    let (lowered, errors) = crate::hir::lower(crate::hir::LowerRequest {
        program: &program,
        resolver: &mut resolver,
        types: &mut types,
        interner: &mut interner,
        cli_program: None,
        locale_pack: None,
        annotation_contracts: None,
    });
    SourceInspection {
        text: hir_inspection_json(&source_file, &lowered.hir, Some(&lowered.presentation), &interner, &errors),
        success: errors.is_empty(),
        errors: Vec::new(),
    }
}

/// Format an already-loaded HIR module as the inspect JSON payload.
#[must_use]
pub fn hir_inspection_json(
    source_file: &crate::driver::SourceFile,
    hir: &crate::hir::HirModule,
    presentation: Option<&radix_hir::HirPresentation>,
    interner: &crate::lexer::Interner,
    errors: &[crate::hir::LowerError],
) -> String {
    let mut out = String::new();
    writeln!(out, "{{").ok();
    writeln!(out, "  \"file\": \"{}\",", escape_json(&source_file.name)).ok();
    writeln!(
        out,
        "  \"frontmatter\": {},",
        frontmatter_json(source_file.frontmatter.as_ref())
    )
    .ok();
    writeln!(out, "  \"success\": {},", errors.is_empty()).ok();
    writeln!(out, "  \"items\": {},", hir.items.len()).ok();
    match presentation {
        Some(presentation) => {
            writeln!(out, "  \"presentation\": {},", presentation_json(presentation, interner)).ok();
        }
        None => {
            writeln!(out, "  \"presentation\": null,").ok();
        }
    }
    writeln!(out, "  \"hir\": [").ok();
    for (i, item) in hir.items.iter().enumerate() {
        let comma = if i + 1 < hir.items.len() { "," } else { "" };
        let kind = format!("{:?}", item.kind);
        writeln!(
            out,
            "    {{ \"id\": {:?}, \"def_id\": {:?}, \"kind\": \"{}\", \"span\": [{}, {}] }}{}",
            item.id.0,
            item.def_id.0,
            debug_variant_name(&kind),
            item.span.start,
            item.span.end,
            comma
        )
        .ok();
    }
    writeln!(out, "  ],").ok();
    writeln!(out, "  \"errors\": [").ok();
    for (i, err) in errors.iter().enumerate() {
        let comma = if i + 1 < errors.len() { "," } else { "" };
        writeln!(
            out,
            "    {{ \"message\": \"{}\", \"span\": [{}, {}] }}{}",
            escape_json(&lower_error_message(source_file, err)),
            err.span.start,
            err.span.end,
            comma
        )
        .ok();
    }
    writeln!(out, "  ]").ok();
    writeln!(out, "}}").ok();
    out
}

/// Produce MIR inspection text without exiting the process.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
#[cfg(feature = "mir-core")]
pub fn mir_output_for_source(name: &str, raw_source: &str) -> Result<String, Vec<String>> {
    mir_output_for_source_with_config(name, raw_source, crate::driver::Config::default().with_dev_locale_root())
}

/// [`mir_output_for_source`] under a caller-chosen driver config, so the
/// product CLI can read the source in its default reader locale (a file's own
/// frontmatter `locale` still wins, as in analysis).
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
#[cfg(feature = "mir-core")]
pub fn mir_output_for_source_with_config(
    name: &str,
    raw_source: &str,
    config: crate::driver::Config,
) -> Result<String, Vec<String>> {
    let source_file = source_file(name.to_owned(), raw_source.to_owned()).map_err(|error| vec![error])?;
    let session = crate::driver::Session::new(config);
    let prepared = match crate::program::prepare_program_module_for_name(&source_file.name, raw_source, &session) {
        Ok(prepared) => prepared,
        Err(diagnostic) => return Err(format_mir_frontend_diagnostics(&source_file, vec![*diagnostic])),
    };

    let analysis = match crate::driver::analyze_source(
        &prepared.session,
        &source_file.name,
        raw_source,
        crate::driver::AnalyzeRequest { cli_program: None, contract: prepared.contract },
    ) {
        Ok(analysis) => analysis,
        Err(diagnostics) => return Err(format_mir_frontend_diagnostics(&source_file, diagnostics)),
    };

    match crate::mir::dump_analyzed_unit(analysis.application_view()) {
        Ok(output) => Ok(output),
        Err(errors) => Err(format_mir_errors(&source_file, errors)),
    }
}

#[cfg(feature = "mir-core")]
fn format_mir_frontend_diagnostics(
    source_file: &crate::driver::SourceFile,
    diagnostics: Vec<crate::diagnostics::Diagnostic>,
) -> Vec<String> {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            let prefix = if diagnostic.is_error() { "error" } else { "warning" };
            let label = diagnostic
                .lookup_key()
                .map(|key| format!("[{key}]"))
                .unwrap_or_default();
            format!(
                "{prefix}{label}: {}: {}",
                format_optional_location(source_file, diagnostic.span),
                diagnostic.message
            )
        })
        .collect()
}

#[cfg(feature = "mir-core")]
fn format_mir_errors(source_file: &crate::driver::SourceFile, errors: Vec<crate::mir::MirError>) -> Vec<String> {
    errors
        .into_iter()
        .map(|err| format!("error: {}: {}", format_location(source_file, err.span.start), err.message))
        .collect()
}

/// Analyze CLI annotations and return the normalized CLI IR JSON payload.
#[must_use]
pub fn cli_ir_inspection_for_source(name: &str, source: &str) -> SourceInspection {
    let source_file = match source_file(name.to_owned(), source.to_owned()) {
        Ok(file) => file,
        Err(error) => return SourceInspection { text: String::new(), success: false, errors: vec![error] },
    };
    let (program, interner) = match parse_inspect_program(&source_file) {
        Ok(parsed) => parsed,
        Err(errors) => return SourceInspection { text: String::new(), success: false, errors },
    };
    let cli_analysis = crate::cli::analyze(&program, &interner);
    SourceInspection {
        text: format!("{}\n", cli_analysis_json(&cli_analysis)),
        success: cli_analysis.errors.is_empty(),
        errors: Vec::new(),
    }
}

fn parse_inspect_program(
    source_file: &crate::driver::SourceFile,
) -> Result<(crate::syntax::Program, crate::lexer::Interner), Vec<String>> {
    let lex_result = lex_reader_surface(source_file).map_err(|error| vec![error])?;
    if !lex_result.success() {
        return Err(lexer_error_lines(source_file, &lex_result.errors));
    }
    let parse_result = crate::parser::parse(lex_result);
    if !parse_result.success() {
        return Err(parser_error_lines(source_file, &parse_result.errors));
    }
    let crate::parser::ParseResult { program, interner, .. } = parse_result;
    let Some(program) = program else {
        return Err(vec![format!(
            "internal error: {}",
            crate::driver::MISSING_PARSE_PROGRAM_MESSAGE
        )]);
    };
    Ok((program, interner))
}

fn debug_variant_name(debug: &str) -> &str {
    if let Some((variant, _)) = debug.split_once('(') {
        variant
    } else {
        debug
    }
}

fn lexer_error_lines(source_file: &crate::driver::SourceFile, errors: &[crate::lexer::LexError]) -> Vec<String> {
    let mut lines = vec!["lexer errors:".to_owned()];
    for err in errors {
        lines.push(format!(
            "  {}: {}",
            format_location(source_file, err.span.start),
            lex_error_message(source_file, err)
        ));
    }
    lines
}

fn parser_error_lines(source_file: &crate::driver::SourceFile, errors: &[crate::parser::ParseError]) -> Vec<String> {
    let mut lines = vec!["parser errors:".to_owned()];
    for err in errors {
        lines.push(format!(
            "  {}: {}",
            format_location(source_file, err.span.start),
            parse_error_message(source_file, err)
        ));
    }
    lines
}

fn prepare_hir_prerequisites(
    source_file: &crate::driver::SourceFile,
    program: &crate::syntax::Program,
    interner: &mut crate::lexer::Interner,
    contract: &crate::driver::ModuleImportContract,
) -> Result<(crate::semantic::Resolver, crate::semantic::TypeTable), Vec<String>> {
    let mut resolver = crate::semantic::Resolver::new();
    let mut types = crate::semantic::TypeTable::new();
    crate::builtins::register(&mut resolver, &mut types, interner);
    crate::builtins::register_frame_types_if_needed(program, &mut resolver, &mut types, interner, None);
    resolver.set_namespace_exports_from_strings(&contract.namespace_exports, interner);
    resolver.set_namespace_seams_from_strings(&contract.namespace_seams, interner);
    resolver.set_namespace_declarations_from_strings(&contract.namespace_declarations, interner);

    if let Err(errors) = crate::semantic::passes::collect::collect(program, &mut resolver, &mut types) {
        let mut lines = vec!["collection errors:".to_owned()];
        for err in errors {
            lines.push(format!("  {:?}: {}", err.kind, semantic_error_message(source_file, &err)));
        }
        return Err(lines);
    }
    // Import bindings install after collection (as in the full pipeline) and
    // under the source's own reader locale; an unsupported export is skipped.
    let pack = crate::program::compile::frontmatter_locale_pack_from_frontmatter(
        &crate::driver::Config::default(),
        source_file.frontmatter.as_ref(),
    );
    if let Err(error) = resolver.set_file_interfaces_from_snapshots_with_locale(
        &contract.file_interfaces,
        interner,
        &mut types,
        pack.as_ref(),
    ) {
        return Err(vec![format!("import errors:\n  file interface install failed: {error:?}")]);
    }
    if let Err(errors) = crate::semantic::passes::resolve::resolve(program, &mut resolver, interner, &mut types) {
        let mut lines = vec!["resolution errors:".to_owned()];
        for err in errors {
            lines.push(format!("  {:?}: {}", err.kind, semantic_error_message(source_file, &err)));
        }
        return Err(lines);
    }
    Ok((resolver, types))
}

#[cfg(test)]
#[path = "inspect_test.rs"]
mod tests;
