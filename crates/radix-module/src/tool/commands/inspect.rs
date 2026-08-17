//! Phase-inspection command handlers that emit JSON or MIR text to stdout.
//!
//! Owns [`cmd_lex`], [`cmd_parse`], [`cmd_hir`], [`cmd_mir`], [`cmd_cli_ir`],
//! their hand-written JSON output builders, and [`mir_output_for_source`] /
//! [`source_file_from_input`] helpers reused by other commands. Delegates each
//! compiler phase (lex, parse, HIR lowering, MIR lowering, CLI analysis) to the
//! corresponding `crate::*` library API. Exits with code 1 on errors discovered
//! during the requested phase.

use super::json::{
    annotation_json, cli_analysis_json, escape_json, frontmatter_json, leading_trivia_json, presentation_json,
};
use super::source::{
    format_location, format_optional_location, lex_error_message, lower_error_message, parse_error_message,
    read_source, semantic_error_message, source_file_from_input,
};
use std::path::PathBuf;

pub fn cmd_lex(args: &[String]) {
    let (name, source) = read_source(args);
    let source_file = source_file_from_input(name, source);
    let result = crate::lexer::lex(source_file.content.as_str());

    print_lex_json(&source_file, &result);

    if !result.success() {
        std::process::exit(1);
    }
}

fn print_lex_json(source_file: &crate::driver::SourceFile, result: &crate::lexer::LexResult) {
    println!("{{");
    println!("  \"file\": \"{}\",", escape_json(&source_file.name));
    println!("  \"frontmatter\": {},", frontmatter_json(source_file.frontmatter.as_ref()));
    println!("  \"success\": {},", result.success());
    println!("  \"tokens\": [");

    for (i, token) in result.tokens.iter().enumerate() {
        let comma = if i + 1 < result.tokens.len() { "," } else { "" };
        let kind_display = token_kind_display(&token.kind);
        println!(
            "    {{ \"kind\": \"{}\", \"span\": [{}, {}] }}{}",
            escape_json(&kind_display),
            token.span.start,
            token.span.end,
            comma
        );
    }

    println!("  ],");
    println!("  \"errors\": [");

    for (i, err) in result.errors.iter().enumerate() {
        let comma = if i + 1 < result.errors.len() { "," } else { "" };
        let message = lex_error_message(source_file, err);
        println!(
            "    {{ \"message\": \"{}\", \"span\": [{}, {}] }}{}",
            escape_json(&message),
            err.span.start,
            err.span.end,
            comma
        );
    }

    println!("  ]");
    println!("}}");
}

fn token_kind_display(kind: &crate::lexer::TokenKind) -> String {
    let kind = format!("{kind:?}");
    // WHY: Truncate long token representations to keep output readable.
    if kind.len() > 60 {
        format!("{}...", &kind[..57])
    } else {
        kind
    }
}

/// Parse source and emit a compact `AST` inspection payload.
///
/// This command stops after parsing and reports lexer/parser diagnostics
/// directly, making it useful for grammar work where semantic phases would add
/// distracting failures.
pub fn cmd_parse(args: &[String]) {
    let (name, source) = read_source(args);
    let source_file = source_file_from_input(name, source);
    let lex_result = crate::lexer::lex(source_file.content.as_str());

    if !lex_result.success() {
        print_lexer_errors(&source_file, &lex_result.errors);
        std::process::exit(1);
    }

    let parse_result = crate::parser::parse(lex_result);

    print_parse_json(&source_file, &parse_result);

    if !parse_result.success() {
        std::process::exit(1);
    }
}

fn print_parse_json(source_file: &crate::driver::SourceFile, parse_result: &crate::parser::ParseResult) {
    println!("{{");
    println!("  \"file\": \"{}\",", escape_json(&source_file.name));
    println!("  \"frontmatter\": {},", frontmatter_json(source_file.frontmatter.as_ref()));
    println!("  \"body_byte_offset\": {},", source_file.body_byte_offset);
    println!("  \"success\": {},", parse_result.success());

    if let Some(program) = &parse_result.program {
        let interner = &parse_result.interner;
        print_parse_ast_json(program, interner);
    } else {
        println!("  \"ast\": null,");
    }

    println!("  \"errors\": [");
    for (i, err) in parse_result.errors.iter().enumerate() {
        let comma = if i + 1 < parse_result.errors.len() { "," } else { "" };
        println!(
            "    {{ \"message\": \"{}\", \"span\": [{}, {}] }}{}",
            escape_json(&parse_error_message(source_file, err)),
            err.span.start,
            err.span.end,
            comma
        );
    }
    println!("  ]");
    println!("}}");
}

fn print_parse_ast_json(program: &crate::syntax::Program, interner: &crate::lexer::Interner) {
    println!("  \"statements\": {},", program.statements.len());
    println!("  \"ast\": [");

    for (i, stmt) in program.statements.iter().enumerate() {
        let comma = if i + 1 < program.statements.len() { "," } else { "" };
        let kind = format!("{:?}", stmt.kind);
        println!(
            "    {{ \"id\": {}, \"kind\": \"{}\", \"span\": [{}, {}], \"leading_trivia\": [{}], \"annotations\": [{}] }}{}",
            stmt.id,
            debug_variant_name(&kind),
            stmt.span.start,
            stmt.span.end,
            leading_trivia_json(&stmt.leading_trivia, interner),
            annotation_json(&stmt.annotations),
            comma
        );
    }

    println!("  ],");
}

/// Lower parsed source to `HIR` and emit a compact inspection payload.
///
/// `HIR` inspection runs the prerequisite collection and resolution passes before
/// lowering so IDs and spans reflect the same semantic inputs used by codegen.
///
/// When `load_path` is `Some`, the function loads a pre-analyzed `.fhir` artifact
/// through the shared loader (F6) and prints its HIR directly without source
/// compilation.
pub fn cmd_hir(args: &[String], load_path: Option<PathBuf>) {
    if let Some(path) = load_path {
        cmd_hir_load_fhir(path);
        return;
    }

    let (name, source) = read_source(args);
    let source_file = source_file_from_input(name, source);

    let (program, mut interner) = parse_inspect_program(&source_file);
    let (mut resolver, mut types) = prepare_hir_prerequisites(&source_file, &program, &mut interner);
    let (lowered, errors) = crate::hir::lower(&program, &mut resolver, &mut types, &mut interner);
    let hir = lowered.hir;

    print_hir_json(&source_file, &hir, Some(&lowered.presentation), &interner, &errors);

    if !errors.is_empty() {
        std::process::exit(1);
    }
}

/// Load a pre-analyzed `.fhir` artifact and print its HIR as compact JSON.
///
/// Reads the file at `path`, deserializes through the shared [`load_fhir`]
/// loader (F6), and reuses the existing [`print_hir_json`] formatter.
/// Error paths print structured `HirArtifactError` messages to stderr and
/// exit with code 1. Never panics (F7).
#[cfg(feature = "hir-fhir")]
fn cmd_hir_load_fhir(path: PathBuf) {
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error: cannot read '{}': {e}", path.display());
            std::process::exit(1);
        }
    };

    match crate::hir::artifact::load_fhir(&bytes) {
        Ok(unit) => {
            let source_file = crate::driver::SourceFile::inline(path.display().to_string(), String::new())
                .unwrap_or_else(|e| {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                });
            // The loaded unit carries the presentation sidecar reconstructed by
            // the FHIR loader (U-4); print it so loaded-artifact inspection
            // exposes anchors, owner kinds, attachments, and structural ends.
            print_hir_json(&source_file, &unit.hir, Some(&unit.presentation), &unit.interner, &[]);
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

/// Fail closed when a build excludes the FHIR artifact loader.
#[cfg(not(feature = "hir-fhir"))]
fn cmd_hir_load_fhir(path: PathBuf) {
    eprintln!(
        "Error: cannot load '{}' because this radix build excludes FHIR artifacts; rebuild with feature `hir-fhir`",
        path.display()
    );
    std::process::exit(1);
}

fn print_hir_json(
    source_file: &crate::driver::SourceFile,
    hir: &crate::hir::HirModule,
    presentation: Option<&radix_hir::HirPresentation>,
    interner: &crate::lexer::Interner,
    errors: &[crate::hir::LowerError],
) {
    println!("{{");
    println!("  \"file\": \"{}\",", escape_json(&source_file.name));
    println!("  \"frontmatter\": {},", frontmatter_json(source_file.frontmatter.as_ref()));
    println!("  \"success\": {},", errors.is_empty());
    println!("  \"items\": {},", hir.items.len());

    // Presentation sidecar (U-6): populated when lowering (U-3) or the FHIR
    // loader (U-4) supplies one; `null` while the presentation data is absent.
    match presentation {
        Some(presentation) => println!("  \"presentation\": {},", presentation_json(presentation, interner)),
        None => println!("  \"presentation\": null,"),
    }

    println!("  \"hir\": [");

    for (i, item) in hir.items.iter().enumerate() {
        let comma = if i + 1 < hir.items.len() { "," } else { "" };
        let kind = format!("{:?}", item.kind);
        println!(
            "    {{ \"id\": {:?}, \"def_id\": {:?}, \"kind\": \"{}\", \"span\": [{}, {}] }}{}",
            item.id.0,
            item.def_id.0,
            debug_variant_name(&kind),
            item.span.start,
            item.span.end,
            comma
        );
    }

    println!("  ],");
    println!("  \"errors\": [");

    for (i, err) in errors.iter().enumerate() {
        let comma = if i + 1 < errors.len() { "," } else { "" };
        let message = lower_error_message(source_file, err);
        println!(
            "    {{ \"message\": \"{}\", \"span\": [{}, {}] }}{}",
            escape_json(&message),
            err.span.start,
            err.span.end,
            comma
        );
    }

    println!("  ]");
    println!("}}");
}

/// Lower checked source to `MIR` and print the deterministic `MIR` dump.
#[cfg(feature = "mir-core")]
pub fn cmd_mir(args: &[String]) {
    let (name, source) = read_source(args);
    let source_file = source_file_from_input(name, source);

    match mir_output_for_source(&source_file.name, &source_file.raw_content) {
        Ok(output) => print!("{output}"),
        Err(messages) => {
            print_mir_messages(&messages);
            std::process::exit(1);
        }
    }
}

/// Produce `MIR` inspection text without exiting the process.
///
/// Errors are returned already formatted for terminal display because `MIR`
/// inspection is primarily a developer-tool surface, not a library data model.
///
/// # Errors
///
/// Returns a list of formatted diagnostic strings when the source cannot be
/// analyzed or MIR lowering fails.
#[cfg(feature = "mir-core")]
pub fn mir_output_for_source(name: &str, raw_source: &str) -> Result<String, Vec<String>> {
    let source_file = source_file_from_input(name.to_owned(), raw_source.to_owned());
    let session = crate::driver::Session::new(crate::driver::Config::default().with_dev_stdlib());
    let prepared = match crate::tool::commands::compile::prepare_program_module_for_name(
        &source_file.name,
        raw_source,
        &session,
    ) {
        Ok(prepared) => prepared,
        Err(diagnostic) => return Err(format_mir_frontend_diagnostics(&source_file, vec![*diagnostic])),
    };

    let mut analysis = match crate::driver::analyze_source_with_module_import_contract(
        &prepared.session,
        &source_file.name,
        raw_source,
        None,
        prepared.contract,
    ) {
        Ok(analysis) => analysis,
        Err(diagnostics) => return Err(format_mir_frontend_diagnostics(&source_file, diagnostics)),
    };

    match crate::mir::dump_analyzed_unit(&mut analysis) {
        Ok(output) => Ok(output),
        Err(errors) => Err(format_mir_errors(&source_file, errors)),
    }
}

fn print_mir_messages(messages: &[String]) {
    for message in messages {
        eprintln!("{message}");
    }
}

fn format_mir_frontend_diagnostics(
    source_file: &crate::driver::SourceFile,
    diagnostics: Vec<crate::diagnostics::Diagnostic>,
) -> Vec<String> {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            let prefix = if diagnostic.is_error() { "error" } else { "warning" };
            format!(
                "{}: {}: {}",
                prefix,
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

/// Analyze CLI annotations and print the normalized CLI IR.
pub fn cmd_cli_ir(args: &[String]) {
    let (name, source) = read_source(args);
    let source_file = source_file_from_input(name, source);

    let (program, interner) = parse_inspect_program(&source_file);
    let cli_analysis = crate::cli::analyze(&program, &interner);
    println!("{}", cli_analysis_json(&cli_analysis));

    if !cli_analysis.errors.is_empty() {
        std::process::exit(1);
    }
}

fn parse_inspect_program(source_file: &crate::driver::SourceFile) -> (crate::syntax::Program, crate::lexer::Interner) {
    let lex_result = crate::lexer::lex(source_file.content.as_str());
    if !lex_result.success() {
        print_lexer_errors(source_file, &lex_result.errors);
        std::process::exit(1);
    }

    let parse_result = crate::parser::parse(lex_result);
    if !parse_result.success() {
        print_parser_errors(source_file, &parse_result.errors);
        std::process::exit(1);
    }

    let crate::parser::ParseResult { program, interner, .. } = parse_result;
    let Some(program) = program else {
        eprintln!("internal error: successful parse result missing program");
        std::process::exit(1);
    };

    (program, interner)
}

fn debug_variant_name(debug: &str) -> &str {
    if let Some((variant, _)) = debug.split_once('(') {
        variant
    } else {
        debug
    }
}

fn print_lexer_errors(source_file: &crate::driver::SourceFile, errors: &[crate::lexer::LexError]) {
    eprintln!("lexer errors:");
    for err in errors {
        eprintln!(
            "  {}: {}",
            format_location(source_file, err.span.start),
            lex_error_message(source_file, err)
        );
    }
}

fn print_parser_errors(source_file: &crate::driver::SourceFile, errors: &[crate::parser::ParseError]) {
    eprintln!("parser errors:");
    for err in errors {
        eprintln!(
            "  {}: {}",
            format_location(source_file, err.span.start),
            parse_error_message(source_file, err)
        );
    }
}

fn prepare_hir_prerequisites(
    source_file: &crate::driver::SourceFile,
    program: &crate::syntax::Program,
    interner: &mut crate::lexer::Interner,
) -> (crate::semantic::Resolver, crate::semantic::TypeTable) {
    let mut resolver = crate::semantic::Resolver::new();
    let mut types = crate::semantic::TypeTable::new();

    crate::builtins::register(&mut resolver, &mut types, interner);
    crate::builtins::register_frame_types_if_needed(program, &mut resolver, &mut types, interner);

    if let Err(e) = crate::semantic::passes::collect::collect(program, &mut resolver, &mut types) {
        eprintln!("collection errors:");
        for err in e {
            eprintln!("  {:?}: {}", err.kind, semantic_error_message(source_file, &err));
        }
        std::process::exit(1);
    }

    if let Err(e) = crate::semantic::passes::resolve::resolve(program, &mut resolver, interner, &mut types) {
        eprintln!("resolution errors:");
        for err in e {
            eprintln!("  {:?}: {}", err.kind, semantic_error_message(source_file, &err));
        }
        std::process::exit(1);
    }

    (resolver, types)
}
