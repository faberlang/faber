//! Stable, package-contained discovery of Faber source files.

use std::fs;
use std::path::{Path, PathBuf};

use radix_module::diagnostics::Diagnostic;
use radix_module::driver::{peel_raw_source, source_load_diagnostic, FileFrontmatter};
use radix_module::lexer::Interner;
use radix_module::syntax::Program;

use crate::frontmatter::validate_frontmatter_against_manifest;
use crate::manifest::FaberManifest;
use radix_module::diagnostics::DiagnosticConvert;

pub struct LoadedPackageSource {
    pub raw_source: String,
    pub body: String,
    pub frontmatter: Option<FileFrontmatter>,
    pub program: Program,
    pub interner: Interner,
}

/// Walk `source_root` for package sources.
///
/// - Always includes `*.fab` product modules.
/// - When `include_proba` is true (the `faber test` path), also includes
///   `*.proba` test sources. Normal `faber build` / `faber run` keep
///   `include_proba = false` so tests never enter the production graph.
pub fn package_source_files(
    source_root: &Path,
    include_proba: bool,
) -> Result<Vec<PathBuf>, Vec<Diagnostic>> {
    let canonical_root = fs::canonicalize(source_root)
        .map_err(|error| vec![Diagnostic::io_error(source_root, &error)])?;
    let mut pending = vec![source_root.to_path_buf()];
    let mut files = Vec::new();
    let mut diagnostics = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                diagnostics.push(Diagnostic::io_error(&directory, &error));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    diagnostics.push(Diagnostic::io_error(&directory, &error));
                    continue;
                }
            };
            let path = entry.path();
            let canonical_path = match fs::canonicalize(&path) {
                Ok(canonical_path) => canonical_path,
                Err(error) => {
                    diagnostics.push(Diagnostic::io_error(&path, &error));
                    continue;
                }
            };
            if !canonical_path.starts_with(&canonical_root) {
                diagnostics.push(
                    crate::package_diagnostic_error(format!(
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
            } else if is_package_source_path(&path, include_proba) {
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

pub fn is_package_source_path(path: &Path, include_proba: bool) -> bool {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("fab") => true,
        Some("proba") => include_proba,
        _ => false,
    }
}

pub fn is_proba_source_path(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some("proba")
}

#[cfg(test)]
#[path = "source_files_test.rs"]
mod source_files_test;

pub fn load_package_source(
    path: &Path,
    manifest: Option<&FaberManifest>,
    locale_pack: Option<&radix_module::locale::LocalePack>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<LoadedPackageSource> {
    let raw_source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            diagnostics.push(Diagnostic::io_error(path, &error));
            return None;
        }
    };

    let display_name = path.display().to_string();
    let peeled = match peel_raw_source(&display_name, &raw_source) {
        Ok(peeled) => peeled,
        Err(error) => {
            diagnostics.push(source_load_diagnostic(&display_name, &error));
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

    let radix_module::driver::PeeledSource {
        body, frontmatter, ..
    } = peeled;
    let body = body.to_owned();
    let lex_result = match locale_pack {
        Some(pack) => radix_module::lexer::lex_with_locale_pack(&body, pack),
        None => radix_module::lexer::lex(&body),
    };
    diagnostics.extend(
        lex_result
            .locale_suggestions
            .iter()
            .map(|suggestion| Diagnostic::from_locale_suggestion(&display_name, &body, suggestion)),
    );
    if !lex_result.success() {
        diagnostics.extend(
            lex_result
                .errors
                .iter()
                .map(|error| Diagnostic::from_lex_error(&display_name, &body, error)),
        );
        return None;
    }

    let parse = radix_module::parser::parse_with_options(
        lex_result,
        radix_module::parser::ParseOptions {
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

    let radix_module::parser::ParseResult {
        program, interner, ..
    } = parse;
    let Some(program) = program else {
        diagnostics.push(
            crate::package_diagnostic_error("successful package parse result missing program")
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
