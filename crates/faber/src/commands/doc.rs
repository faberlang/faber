//! `faber doc` — render a file's or package's exported API with its doc
//! comments (DECISIONS D7.1–D7.4, D7.6, D7.7).
//!
//! RULES
//! =====
//! - Every comment is a doc comment; there is no doc marker (D7.1). A
//!   declaration's doc is the comment block directly above it with no blank
//!   line in between; a block followed by a blank line is a note (D7.6). The
//!   parser keeps comments and blank lines as leading trivia on the owner, so
//!   the rule is applied here, not in the parser.
//! - Only exported top-level declarations (`@ publica` / `@ interna`) are
//!   rendered; comments on statements or private declarations are never
//!   extracted (D7.2). `genus` members are public by default (D5.2) and render
//!   under their type unless `@ privata`.
//! - The module doc is the comment block directly above the `regio` (en
//!   `module`) declaration (D7.3 via D7.7). A file without `regio` has none.
//! - Names, keywords and annotations are rendered from the source text, so
//!   output stays in the file's own reader locale.
//!
//! Sources load through the package source loader
//! (`radix::program::source_files::load_package_source`), the same per-file
//! peel → locale → lex → parse path package analysis uses. Semantic file
//! interfaces carry neither comments nor source spans, so they cannot serve
//! this command; no semantic analysis is required to document a file.

use radix::module::diagnostics::Diagnostic;
use radix::module::lexer::{Interner, Span};
use radix::module::semantic::passes::visibility_annotation::{
    VisibilityTier, member_visibility, statement_export_tier,
};
use radix::module::syntax::{
    Annotation, ClassMemberKind, MemberVisibility, Program, Stmt, StmtKind, Trivia,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Version of the `faber doc --json` document. Bump on any key change.
pub(crate) const DOC_SCHEMA_VERSION: u32 = 1;

/// Arguments for `faber doc`.
#[derive(Debug, Clone)]
pub struct DocCommand {
    pub input: PathBuf,
    pub json: bool,
}

/// Documented API of one source file.
#[derive(Debug, Serialize)]
pub(crate) struct DocFile {
    pub path: String,
    /// Reader locale id the file was read in, when a pack resolved.
    pub locale: Option<String>,
    pub module: Option<DocModule>,
    pub items: Vec<DocItem>,
}

/// The file's `regio` name and the comment block directly above it.
#[derive(Debug, Serialize)]
pub(crate) struct DocModule {
    pub name: String,
    pub doc: Option<String>,
}

/// One exported top-level declaration.
#[derive(Debug, Serialize)]
pub(crate) struct DocItem {
    pub kind: &'static str,
    pub name: String,
    /// `publica` or `interna`.
    pub visibility: &'static str,
    /// Annotation lines as written, in source order.
    pub annotations: Vec<String>,
    /// Declaration header as written (no body), whitespace-collapsed.
    pub signature: String,
    pub doc: Option<String>,
    /// First sentence of `doc`.
    pub summary: Option<String>,
    pub members: Vec<DocMember>,
}

/// One non-private member of an exported type.
#[derive(Debug, Serialize)]
pub(crate) struct DocMember {
    pub kind: &'static str,
    pub name: String,
    pub visibility: &'static str,
    pub annotations: Vec<String>,
    pub signature: String,
    pub doc: Option<String>,
    pub summary: Option<String>,
}

#[derive(Serialize)]
struct DocJson<'a> {
    schema_version: u32,
    files: &'a [DocFile],
}

pub fn cmd_doc(command: &DocCommand) {
    let files = match document_input(&command.input) {
        Ok(files) => files,
        Err(diagnostics) => {
            super::eprint_compile_diagnostics(&diagnostics);
            faber_cli::exit_status::exit_for_diagnostics(&diagnostics);
        }
    };
    if command.json {
        match render_json(&files) {
            Ok(json) => print!("{json}"),
            Err(err) => {
                eprintln!("error: failed to serialize doc JSON: {err}");
                std::process::exit(1);
            }
        }
    } else {
        print!("{}", render_text(&files));
    }
}

/// Document one `.fab` file, or every package source file under a package
/// directory / manifest.
pub(crate) fn document_input(input: &Path) -> Result<Vec<DocFile>, Vec<Diagnostic>> {
    let input = faber_cli::package::absolutize_path(input);
    if !input.exists() {
        return Err(vec![Diagnostic::io_error(
            &input,
            &std::io::Error::from(std::io::ErrorKind::NotFound),
        )]);
    }
    let is_package = input.is_dir() || input.file_name().is_some_and(|name| name == "faber.toml");
    let spec = faber_cli::package::discover_package(&input);
    let (spec, paths) = if is_package {
        let spec = spec.map_err(|diagnostic| vec![*diagnostic])?;
        let mut paths =
            radix::program::source_files::package_source_files(&spec.source_root, false)?;
        paths.sort();
        (Some(spec), paths)
    } else {
        // A lone file still borrows its package's manifest (locale default,
        // bodyless library functions) when one is discoverable.
        (spec.ok(), vec![input.clone()])
    };
    let manifest = match spec.as_ref().filter(|spec| spec.manifest_backed) {
        Some(spec) => Some(
            radix::program::manifest::read_manifest(&spec.package_root.join("faber.toml"))
                .map_err(|diagnostic| vec![*diagnostic])?,
        ),
        None => None,
    };
    let display_root = if is_package {
        spec.as_ref().map(|spec| spec.package_root.clone())
    } else {
        None
    };

    let config = radix::module::Config::default();
    let mut diagnostics = Vec::new();
    let mut files = Vec::new();
    for path in &paths {
        let Some(loaded) = radix::program::source_files::load_package_source(
            path,
            manifest.as_ref(),
            &config,
            faber_cli::package::load_locale_pack_for_input,
            &mut diagnostics,
        ) else {
            continue;
        };
        let display = display_root
            .as_deref()
            .and_then(|root| path.strip_prefix(root).ok())
            .unwrap_or(path)
            .display()
            .to_string();
        let locale = loaded
            .locale_pack
            .as_ref()
            .map(|pack| pack.metadata.id.as_str());
        files.push(document_program(
            &display,
            locale,
            &loaded.program,
            &loaded.interner,
            &loaded.body,
        ));
    }
    if diagnostics.iter().any(Diagnostic::is_error) {
        return Err(diagnostics);
    }
    Ok(files)
}

/// Build the documented API of one parsed file. `body` is the peeled source
/// the program's spans index into.
pub(crate) fn document_program(
    path: &str,
    locale: Option<&str>,
    program: &Program,
    interner: &Interner,
    body: &str,
) -> DocFile {
    let module = program.region.as_ref().map(|region| DocModule {
        name: interner.resolve(region.name.name).to_owned(),
        doc: doc_block(&region.leading_trivia, None, interner),
    });
    let items = program
        .statements
        .iter()
        .filter_map(|stmt| document_stmt(stmt, interner, body))
        .collect();
    DocFile {
        path: path.to_owned(),
        locale: locale.map(str::to_owned),
        module,
        items,
    }
}

fn document_stmt(stmt: &Stmt, interner: &Interner, body: &str) -> Option<DocItem> {
    let visibility = match statement_export_tier(stmt, interner)? {
        VisibilityTier::Publica => "publica",
        VisibilityTier::Interna => "interna",
        VisibilityTier::Privata => return None,
    };
    let src = Source(body);
    let anchor = first_annotation_start(&stmt.annotations);
    let doc = doc_block(&stmt.leading_trivia, anchor, interner);
    let mut members = Vec::new();
    let (kind, name, header_end) = match &stmt.kind {
        StmtKind::Func(func) => (
            "function",
            interner.resolve(func.name.name).to_owned(),
            func.body
                .as_ref()
                .map_or(stmt.span.end, |block| block.span.start),
        ),
        StmtKind::Class(class) => {
            members = class_members(class, src, interner);
            (
                "genus",
                interner.resolve(class.name.name).to_owned(),
                src.brace_after(class.name.span.end, stmt.span.end),
            )
        }
        StmtKind::Interface(interface) => {
            members = interface
                .methods
                .iter()
                .map(|method| {
                    DocMember::new(
                        "method",
                        interner.resolve(method.name.name),
                        "publica",
                        &method.annotations,
                        &method.leading_trivia,
                        method.span,
                        src,
                        interner,
                    )
                })
                .collect();
            (
                "contract",
                interner.resolve(interface.name.name).to_owned(),
                src.brace_after(interface.name.span.end, stmt.span.end),
            )
        }
        StmtKind::Enum(decl) => {
            members = decl
                .members
                .iter()
                .map(|member| {
                    DocMember::new(
                        "member",
                        interner.resolve(member.name.name),
                        "publica",
                        &[],
                        &member.leading_trivia,
                        member.span,
                        src,
                        interner,
                    )
                })
                .collect();
            (
                "enum",
                interner.resolve(decl.name.name).to_owned(),
                src.brace_after(decl.name.span.end, stmt.span.end),
            )
        }
        StmtKind::Union(decl) => {
            let fields = decl.shared_fields.iter().filter_map(|field| {
                let visibility = match member_visibility(&field.annotations, interner) {
                    MemberVisibility::Publica => "publica",
                    MemberVisibility::Interna => "interna",
                    MemberVisibility::Privata => return None,
                };
                Some(DocMember::new(
                    "field",
                    interner.resolve(field.field.name.name),
                    visibility,
                    &field.annotations,
                    &field.leading_trivia,
                    field.span,
                    src,
                    interner,
                ))
            });
            let variants = decl.variants.iter().map(|variant| {
                DocMember::new(
                    "variant",
                    interner.resolve(variant.name.name),
                    "publica",
                    &[],
                    &variant.leading_trivia,
                    variant.span,
                    src,
                    interner,
                )
            });
            members = fields.chain(variants).collect();
            (
                "union",
                interner.resolve(decl.name.name).to_owned(),
                src.brace_after(decl.name.span.end, stmt.span.end),
            )
        }
        StmtKind::TypeAlias(alias) => (
            "alias",
            interner.resolve(alias.name.name).to_owned(),
            stmt.span.end,
        ),
        StmtKind::Schema(schema) => (
            "schema",
            interner.resolve(schema.name.name).to_owned(),
            stmt.span.end,
        ),
        StmtKind::Var(var) => {
            let name = match &var.binding {
                radix::module::syntax::BindingPattern::Ident(ident) => {
                    interner.resolve(ident.name).to_owned()
                }
                _ => src.collapsed(stmt.span.start, stmt.span.end),
            };
            ("value", name, stmt.span.end)
        }
        _ => return None,
    };
    let summary = doc.as_deref().map(first_sentence);
    Some(DocItem {
        kind,
        name,
        visibility,
        annotations: src.annotations(&stmt.annotations),
        signature: src.collapsed(stmt.span.start, header_end),
        doc,
        summary,
        members,
    })
}

fn class_members(
    class: &radix::module::syntax::ClassDecl,
    src: Source<'_>,
    interner: &Interner,
) -> Vec<DocMember> {
    class
        .members
        .iter()
        .filter_map(|member| {
            let visibility = match member_visibility(&member.annotations, interner) {
                MemberVisibility::Publica => "publica",
                MemberVisibility::Interna => "interna",
                MemberVisibility::Privata => return None,
            };
            let (kind, name, end) = match &member.kind {
                ClassMemberKind::Field(field) => ("field", field.name.name, member.span.end),
                ClassMemberKind::Method(method) => (
                    "method",
                    method.name.name,
                    method
                        .body
                        .as_ref()
                        .map_or(member.span.end, |block| block.span.start),
                ),
            };
            Some(DocMember::new(
                kind,
                interner.resolve(name),
                visibility,
                &member.annotations,
                &member.leading_trivia,
                Span::new(member.span.start, end),
                src,
                interner,
            ))
        })
        .collect()
}

impl DocMember {
    #[allow(clippy::too_many_arguments)]
    fn new(
        kind: &'static str,
        name: &str,
        visibility: &'static str,
        annotations: &[Annotation],
        leading_trivia: &[Trivia],
        span: Span,
        src: Source<'_>,
        interner: &Interner,
    ) -> Self {
        // A member span may open at its first annotation; the signature
        // starts after the last one.
        let start = annotations
            .iter()
            .map(|annotation| annotation.span.end)
            .max()
            .map_or(span.start, |end| end.max(span.start));
        let doc = doc_block(
            leading_trivia,
            first_annotation_start(annotations),
            interner,
        );
        Self {
            kind,
            name: name.to_owned(),
            visibility,
            annotations: src.annotations(annotations),
            signature: src.collapsed(start, span.end),
            summary: doc.as_deref().map(first_sentence),
            doc,
        }
    }
}

fn first_annotation_start(annotations: &[Annotation]) -> Option<u32> {
    annotations
        .iter()
        .map(|annotation| annotation.span.start)
        .min()
}

/// The comment block directly above an anchor (D7.6).
///
/// Leading trivia holds every comment and newline token before the owner,
/// including the newline that ends the previous line and the newlines that
/// end annotation lines. Walking backward from the owner, a comment line
/// joins the doc while at most one newline separates it from what follows; a
/// second consecutive newline is a blank line and ends the block. Newlines
/// at or after `anchor` (the first annotation) end annotation lines and are
/// ignored.
fn doc_block(trivia: &[Trivia], anchor: Option<u32>, interner: &Interner) -> Option<String> {
    let mut lines = Vec::new();
    let mut newlines = 0usize;
    for item in trivia.iter().rev() {
        match item {
            Trivia::Newline(newline) => {
                if anchor.is_some_and(|anchor| newline.span.start >= anchor) {
                    continue;
                }
                newlines += 1;
                if newlines > 1 {
                    break;
                }
            }
            Trivia::CommentLine(comment) => {
                let text = interner.resolve(comment.text);
                lines.push(text.strip_prefix(' ').unwrap_or(text).trim_end().to_owned());
                newlines = 0;
            }
        }
    }
    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    Some(lines.join("\n"))
}

/// The first sentence of a doc: text through the first `.` that ends a
/// word, or the first line when no sentence end is present.
fn first_sentence(doc: &str) -> String {
    let flat = doc.split_whitespace().collect::<Vec<_>>().join(" ");
    let bytes = flat.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'.' && bytes.get(index + 1).is_none_or(|next| *next == b' ') {
            return flat[..=index].to_owned();
        }
    }
    doc.lines().next().unwrap_or_default().trim().to_owned()
}

/// Source-text slicing for signatures.
#[derive(Clone, Copy)]
struct Source<'a>(&'a str);

impl<'a> Source<'a> {
    fn slice(self, start: u32, end: u32) -> &'a str {
        self.0.get(start as usize..end as usize).unwrap_or_default()
    }

    /// A span's text as one line: each line trimmed, comment lines dropped.
    fn collapsed(self, start: u32, end: u32) -> String {
        self.slice(start, end)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn annotations(self, annotations: &[Annotation]) -> Vec<String> {
        annotations
            .iter()
            .map(|annotation| self.collapsed(annotation.span.start, annotation.span.end))
            .collect()
    }

    /// Offset of the first `{` at or after `from` (a type body opener), or
    /// `fallback` when the declaration has no braced body.
    fn brace_after(self, from: u32, fallback: u32) -> u32 {
        self.slice(from, fallback)
            .find('{')
            .map_or(fallback, |offset| from + offset as u32)
    }
}

/// Plain-text rendering: per file, the module line and doc, then each item's
/// annotations, signature and indented doc, with members indented beneath.
pub(crate) fn render_text(files: &[DocFile]) -> String {
    let mut out = String::new();
    for (index, file) in files.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&format!("== {} ==\n", file.path));
        if let Some(module) = &file.module {
            out.push_str(&format!("\n{}\n", module.name));
            push_doc(&mut out, module.doc.as_deref(), "    ");
        }
        for item in &file.items {
            out.push('\n');
            for annotation in &item.annotations {
                out.push_str(annotation);
                out.push('\n');
            }
            out.push_str(&item.signature);
            out.push('\n');
            push_doc(&mut out, item.doc.as_deref(), "    ");
            for member in &item.members {
                for annotation in &member.annotations {
                    out.push_str(&format!("  {annotation}\n"));
                }
                out.push_str(&format!("  {}\n", member.signature));
                push_doc(&mut out, member.doc.as_deref(), "      ");
            }
        }
    }
    out
}

fn push_doc(out: &mut String, doc: Option<&str>, indent: &str) {
    for line in doc.into_iter().flat_map(str::lines) {
        if line.is_empty() {
            out.push('\n');
        } else {
            out.push_str(indent);
            out.push_str(line);
            out.push('\n');
        }
    }
}

/// Machine-readable rendering: one compact JSON document, newline-terminated.
pub(crate) fn render_json(files: &[DocFile]) -> Result<String, serde_json::Error> {
    let mut json = serde_json::to_string(&DocJson {
        schema_version: DOC_SCHEMA_VERSION,
        files,
    })?;
    json.push('\n');
    Ok(json)
}

#[cfg(test)]
#[path = "doc_test.rs"]
mod doc_test;
