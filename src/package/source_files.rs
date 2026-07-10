//! Stable, package-contained discovery of Faber source files.

use std::fs;
use std::path::{Path, PathBuf};

use radix::diagnostics::Diagnostic;
use radix::driver::{peel_raw_source, source_load_diagnostic, FileFrontmatter};
use radix::lexer::Interner;
use radix::syntax::Program;

use super::frontmatter::validate_frontmatter_against_manifest;
use super::manifest::FaberManifest;

pub(super) struct LoadedPackageSource {
    pub(super) raw_source: String,
    pub(super) body: String,
    pub(super) frontmatter: Option<FileFrontmatter>,
    pub(super) program: Program,
    pub(super) interner: Interner,
}

pub(super) fn package_source_files(source_root: &Path) -> Result<Vec<PathBuf>, Vec<Diagnostic>> {
    let canonical_root = fs::canonicalize(source_root)
        .map_err(|error| vec![Diagnostic::io_error(source_root, error)])?;
    let mut pending = vec![source_root.to_path_buf()];
    let mut files = Vec::new();
    let mut diagnostics = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                diagnostics.push(Diagnostic::io_error(&directory, error));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    diagnostics.push(Diagnostic::io_error(&directory, error));
                    continue;
                }
            };
            let path = entry.path();
            let canonical_path = match fs::canonicalize(&path) {
                Ok(canonical_path) => canonical_path,
                Err(error) => {
                    diagnostics.push(Diagnostic::io_error(&path, error));
                    continue;
                }
            };
            if !canonical_path.starts_with(&canonical_root) {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "package source resolves outside the source root: {}",
                        path.display()
                    ))
                    .with_file(path.display().to_string())
                    .with_arg("issue", "package_source_symlink_escape"),
                );
                continue;
            }
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "fab") {
                files.push(path);
            }
        }
    }
    if diagnostics.is_empty() {
        files.sort();
        Ok(files)
    } else {
        Err(diagnostics)
    }
}

pub(super) fn load_package_source(
    path: &Path,
    manifest: Option<&FaberManifest>,
    reader_pack: Option<&radix::reader_locale::ReaderLocalePack>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<LoadedPackageSource> {
    let raw_source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            diagnostics.push(Diagnostic::io_error(path, error));
            return None;
        }
    };

    let display_name = path.display().to_string();
    let peeled = match peel_raw_source(&display_name, &raw_source) {
        Ok(peeled) => peeled,
        Err(error) => {
            diagnostics.push(source_load_diagnostic(&display_name, error));
            return None;
        }
    };
    if let Some(manifest) = manifest {
        if let Some(diagnostic) =
            validate_frontmatter_against_manifest(path, peeled.frontmatter.as_ref(), manifest)
        {
            diagnostics.push(diagnostic);
            return None;
        }
    }

    let radix::driver::PeeledSource {
        body, frontmatter, ..
    } = peeled;
    let body = body.to_owned();
    let lex_result = match reader_pack {
        Some(pack) => radix::lexer::lex_with_reader_pack(&body, pack),
        None => radix::lexer::lex(&body),
    };
    diagnostics.extend(
        lex_result.reader_fallbacks.iter().map(|fallback| {
            Diagnostic::from_reader_locale_fallback(&display_name, &body, fallback)
        }),
    );
    diagnostics.extend(lex_result.reader_suggestions.iter().map(|suggestion| {
        Diagnostic::from_reader_locale_suggestion(&display_name, &body, suggestion)
    }));
    if !lex_result.success() {
        diagnostics.extend(
            lex_result
                .errors
                .iter()
                .map(|error| Diagnostic::from_lex_error(&display_name, &body, error)),
        );
        return None;
    }

    let parse = radix::parser::parse_with_options(
        lex_result,
        radix::parser::ParseOptions {
            allow_bodyless_functions: manifest.is_some_and(|manifest| manifest.build.kind == "lib"),
        },
    );
    if !parse.success() {
        diagnostics.extend(
            parse
                .errors
                .iter()
                .map(|error| Diagnostic::from_parse_error(&display_name, &body, error)),
        );
        return None;
    }

    let radix::parser::ParseResult {
        program, interner, ..
    } = parse;
    let Some(program) = program else {
        diagnostics.push(
            Diagnostic::error("successful package parse result missing program")
                .with_file(path.display().to_string()),
        );
        return None;
    };

    Some(LoadedPackageSource {
        raw_source,
        body,
        frontmatter,
        program,
        interner,
    })
}
