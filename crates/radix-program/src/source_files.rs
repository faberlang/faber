//! Stable, package-contained discovery of Faber source files.

use std::fs;
use std::path::{Path, PathBuf};

/// Maximum number of Faber source files discovered under one package source
/// root. This bounds directory volume before parsing or analysis begins.
pub const MAX_PACKAGE_SOURCE_FILES: usize = 4096;
/// Maximum aggregate byte length of Faber source files discovered under one
/// package source root.
pub const MAX_PACKAGE_SOURCE_BYTES: u64 = 64 * 1024 * 1024;

use radix_module::diagnostics::Diagnostic;
use radix_module::driver::{
    Config, FileFrontmatter, peel_raw_source, source_load_diagnostic, source_reads_as_other_locale,
};
use radix_module::lexer::Interner;
use radix_module::syntax::Program;

use crate::frontmatter::validate_frontmatter_against_manifest;
use crate::library::LocaleLoader;
use crate::manifest::FaberManifest;
use radix_module::diagnostics::DiagnosticConvert;

pub struct LoadedPackageSource {
    pub raw_source: String,
    pub body: String,
    pub body_byte_offset: u32,
    pub frontmatter: Option<FileFrontmatter>,
    pub locale_pack: Option<radix_module::locale::LocalePack>,
    /// A locale is declared by `--locale`, the manifest, or the source
    /// frontmatter; the product default is not a declaration.
    pub locale_declared: bool,
    pub program: Program,
    pub interner: Interner,
}

/// Walk `source_root` for package sources.
///
/// - Always includes `*.fab` product modules.
/// - When `include_proba` is true (the `faber test` path), also includes
///   `*.proba` test sources. Normal `faber build` / `faber run` keep
///   `include_proba = false` so tests never enter the production graph.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn package_source_files(
    source_root: &Path,
    include_proba: bool,
) -> Result<Vec<PathBuf>, Vec<Diagnostic>> {
    let canonical_root = fs::canonicalize(source_root)
        .map_err(|error| vec![Diagnostic::io_error(source_root, &error)])?;
    let mut pending = vec![source_root.to_path_buf()];
    let mut files = Vec::new();
    let mut total_bytes = 0_u64;
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
                if files.len() >= MAX_PACKAGE_SOURCE_FILES {
                    return Err(vec![package_source_limit_diagnostic(
                        &canonical_root,
                        "package_source_file_limit",
                        format!(
                            "package source file limit exceeded: maximum is {MAX_PACKAGE_SOURCE_FILES} files"
                        ),
                        (files.len() + 1).to_string(),
                        MAX_PACKAGE_SOURCE_FILES.to_string(),
                    )]);
                }
                let byte_length = match fs::metadata(&path) {
                    Ok(metadata) => metadata.len(),
                    Err(error) => {
                        diagnostics.push(Diagnostic::io_error(&path, &error));
                        continue;
                    }
                };
                let Some(new_total_bytes) = total_bytes.checked_add(byte_length) else {
                    return Err(vec![package_source_limit_diagnostic(
                        &canonical_root,
                        "package_source_byte_limit",
                        format!(
                            "package source byte limit exceeded: maximum is {MAX_PACKAGE_SOURCE_BYTES} bytes"
                        ),
                        u64::MAX.to_string(),
                        MAX_PACKAGE_SOURCE_BYTES.to_string(),
                    )]);
                };
                if new_total_bytes > MAX_PACKAGE_SOURCE_BYTES {
                    return Err(vec![package_source_limit_diagnostic(
                        &canonical_root,
                        "package_source_byte_limit",
                        format!(
                            "package source byte limit exceeded: maximum is {MAX_PACKAGE_SOURCE_BYTES} bytes"
                        ),
                        new_total_bytes.to_string(),
                        MAX_PACKAGE_SOURCE_BYTES.to_string(),
                    )]);
                }
                total_bytes = new_total_bytes;
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

fn package_source_limit_diagnostic(
    source_root: &Path,
    issue: &str,
    message: String,
    observed: String,
    limit: String,
) -> Diagnostic {
    crate::package_diagnostic_error(message)
        .with_file(source_root.display().to_string())
        .with_arg("issue", issue)
        .with_arg("observed", observed)
        .with_arg("limit", limit)
}

#[must_use]
pub fn is_package_source_path(path: &Path, include_proba: bool) -> bool {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("fab") => true,
        Some("proba") => include_proba,
        _ => false,
    }
}

#[must_use]
pub fn is_proba_source_path(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()) == Some("proba")
}

#[cfg(test)]
#[path = "source_files_test.rs"]
mod source_files_test;

pub fn load_package_source(
    path: &Path,
    manifest: Option<&FaberManifest>,
    config: &Config,
    locale_loader: LocaleLoader,
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
    if let Some(manifest) = manifest
        && let Some(diagnostic) =
            validate_frontmatter_against_manifest(path, peeled.frontmatter.as_ref(), manifest)
    {
        diagnostics.push(diagnostic);
        return None;
    }

    let locale_pack = resolve_locale_pack(
        path,
        config,
        locale_loader,
        peeled.frontmatter.as_ref(),
        diagnostics,
    )?;
    // A locale is declared by the caller (`--locale`), the manifest, or the
    // source frontmatter; the product default is not a declaration.
    let locale_declared = config.locale_pack_explicit
        || manifest.is_some_and(|manifest| manifest.locale.locale.is_some())
        || peeled
            .frontmatter
            .as_ref()
            .and_then(FileFrontmatter::locale_result)
            .is_some();
    let body_byte_offset = peeled.body_byte_offset;
    let radix_module::driver::PeeledSource {
        body, frontmatter, ..
    } = peeled;
    let body = body.to_owned();
    let source_diagnostic_start = diagnostics.len();
    // WHY: a wrong-locale read hides behind this file's lex and parse errors.
    // The note is advisory only; it never changes the locale the file is read in.
    // TEMPORARY STAGING: a source with no declared locale reads
    // the no-locale default (`en` while `RADIX_DEFAULT_CODE_LOCALE=en`); unset
    // keeps the pack-less (Latin) reading. The resolved pack also feeds analysis.
    let locale_pack = match locale_pack {
        Some(pack) => Some(pack),
        None => radix_module::driver::default_code_pack(config),
    };
    let other_locale_note = || {
        if locale_declared {
            return None;
        }
        let baseline = locale_pack.as_ref()?;
        source_reads_as_other_locale(config, &display_name, &body, baseline)
    };
    let lex_result = match locale_pack.as_ref() {
        Some(pack) => radix_module::lexer::lex_with_locale_pack(&body, pack),
        None => radix_module::lexer::lex(&body),
    };
    // Reader-locale suggestions wait for the parse result: an edit-distance
    // guess is shown only where a parse error sits at its span. Name-lookup
    // errors are found later, by the per-file analysis, which filters its own.
    let locale_candidates: Vec<Diagnostic> = lex_result
        .locale_suggestions
        .iter()
        .map(|suggestion| Diagnostic::from_locale_suggestion(&display_name, &body, suggestion))
        .collect();
    diagnostics.extend(
        lex_result
            .mixed_script_advisories
            .iter()
            .map(|advisory| Diagnostic::from_mixed_script_advisory(&display_name, &body, advisory)),
    );
    if !lex_result.success() {
        diagnostics.extend(
            lex_result
                .errors
                .iter()
                .map(|error| Diagnostic::from_lex_error(&display_name, &body, error)),
        );
        let kept = Diagnostic::filter_locale_suggestions(locale_candidates, diagnostics);
        diagnostics.extend(kept);
        if let Some(note) = other_locale_note() {
            diagnostics.insert(source_diagnostic_start, note);
        }
        radix_module::driver::adjust_diagnostic_lines_for_body(
            &mut diagnostics[source_diagnostic_start..],
            &raw_source,
            body_byte_offset,
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
    }
    let kept = Diagnostic::filter_locale_suggestions(locale_candidates, diagnostics);
    diagnostics.extend(kept);
    if !parse.success() {
        if let Some(note) = other_locale_note() {
            diagnostics.insert(source_diagnostic_start, note);
        }
        radix_module::driver::adjust_diagnostic_lines_for_body(
            &mut diagnostics[source_diagnostic_start..],
            &raw_source,
            body_byte_offset,
        );
        return None;
    }

    radix_module::driver::adjust_diagnostic_lines_for_body(
        &mut diagnostics[source_diagnostic_start..],
        &raw_source,
        body_byte_offset,
    );

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
        body_byte_offset,
        frontmatter,
        locale_pack,
        locale_declared,
        program,
        interner,
    })
}

fn resolve_locale_pack(
    path: &Path,
    config: &Config,
    locale_loader: LocaleLoader,
    frontmatter: Option<&FileFrontmatter>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Option<radix_module::locale::LocalePack>> {
    if config.locale_pack_explicit {
        return Some(config.locale_pack.clone());
    }

    match frontmatter.and_then(FileFrontmatter::locale_result) {
        Some(Err(())) => {
            diagnostics.push(
                crate::package_diagnostic_error("frontmatter 'locale' key must be a string")
                    .with_file(path.display().to_string())
                    .with_arg("issue", "frontmatter_locale_type"),
            );
            None
        }
        Some(Ok(locale)) => match locale_loader(path, Some(locale)) {
            Ok(Some(pack)) => Some(Some(pack)),
            Ok(None) => {
                diagnostics.push(
                    crate::package_diagnostic_error(format!(
                        "reader locale '{locale}' was declared but no locale pack was supplied"
                    ))
                    .with_file(path.display().to_string())
                    .with_arg("issue", "locale_pack_missing"),
                );
                None
            }
            Err(diagnostic) => {
                diagnostics.push(*diagnostic);
                None
            }
        },
        None => match config.locale_pack.clone() {
            Some(pack) => Some(Some(pack)),
            None => match locale_loader(path, None) {
                Ok(pack) => Some(pack),
                Err(diagnostic) => {
                    diagnostics.push(*diagnostic);
                    None
                }
            },
        },
    }
}
