//! Validate import path specifiers for forbidden forms before resolution.
//!
//! Runs unconditionally (all modes). Rejects:
//! - `..` segments (parent traversal)
//! - Absolute paths (`/` or Windows drive letter)
//! - Unknown schemes (not `provider:module`, `./rel`, or `§name/rest`)
//! - Unknown `§name` (not in templates map)
//! - Bare `§/…` (glyph without template name)
//! - Multiple `§` holes
//! - `§N` / `§0` positional holes

use std::collections::BTreeMap;

use crate::{SemanticError, SemanticErrorKind};
use radix_lexer::Interner;
use radix_syntax::{Program, StmtKind};

/// Byte length of the `§` character in UTF-8.
const GLYPH_BYTE_LEN: usize = 2;

/// Validate every `importa ex "…"` specifier in the program.
///
/// `templates` is `None` in script/bare-file mode: `§`-related checks are
/// skipped (only absolute, traversal, and scheme forms are validated).
///
/// # Errors
///
/// Returns [`SemanticError`]s for each forbidden import form found.
pub fn validate_import_paths(
    program: &Program,
    interner: &Interner,
    templates: Option<&BTreeMap<String, String>>,
) -> Result<(), Vec<SemanticError>> {
    // Default set of known template names used even when `templates` is `None`.
    // Currently only `§src` is a default (mapped from `[paths].source`).
    let default_template_names: [&str; 1] = ["src"];
    let mut errors = Vec::new();

    for stmt in &program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        if decl.target.is_some() {
            continue;
        }
        let path = interner.resolve(decl.path);

        // Check 1: `..` segment anywhere in the path.
        if contains_parent_traversal(path) {
            errors.push(
                SemanticError::structured(SemanticErrorKind::ImportAbsent, decl.span)
                    .with_arg("issue", "import_parent_traversal")
                    .with_arg("path", path),
            );
            continue;
        }

        // Check 2: Absolute path (Unix `/` or Windows drive letter).
        if is_absolute_path(path) {
            errors.push(
                SemanticError::structured(SemanticErrorKind::ImportAbsent, decl.span)
                    .with_arg("issue", "import_absolute_path")
                    .with_arg("path", path),
            );
            continue;
        }

        // Check 3: Determine the path form.
        if path.starts_with('§') {
            // Template form: validate `§name/rest`.
            if templates.is_none() {
                // In no-template modes (script/bare), reject all `§` forms
                // including default templates like §src.
                errors.push(
                    SemanticError::structured(SemanticErrorKind::ImportAbsent, decl.span)
                        .with_arg("issue", "import_unknown_template")
                        .with_arg("name", path),
                );
                continue;
            }

            if let Some(templates) = templates {
                validate_template_specifier(
                    path,
                    templates,
                    &default_template_names,
                    decl.span,
                    &mut errors,
                );
            }
        } else if is_valid_provider_import(path) {
            // Provider form: `provider:module` — allowed.
        } else if path.starts_with("./") {
            // Relative form: `./rel` — allowed (no `..` already checked above).
        } else {
            // Not a known form: reject as unknown scheme.
            errors.push(
                SemanticError::structured(SemanticErrorKind::ImportAbsent, decl.span)
                    .with_arg("issue", "import_unknown_scheme")
                    .with_arg("path", path),
            );
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validate a `§name/rest` template specifier.
fn validate_template_specifier(
    path: &str,
    templates: &BTreeMap<String, String>,
    default_names: &[&str],
    span: radix_lexer::Span,
    errors: &mut Vec<SemanticError>,
) {
    // `§` is multi-byte; use the constant to skip past it.
    let after_glyph = &path[GLYPH_BYTE_LEN..];

    // Bare `§/…` — no template name after the glyph.
    if after_glyph.is_empty() || after_glyph.starts_with('/') {
        errors.push(
            SemanticError::structured(SemanticErrorKind::ImportAbsent, span)
                .with_arg("issue", "import_bare_glyph"),
        );
        return;
    }

    // Check for `§N` / `§0` positional holes.
    if after_glyph.starts_with(|c: char| c.is_ascii_digit()) {
        errors.push(
            SemanticError::structured(SemanticErrorKind::ImportAbsent, span)
                .with_arg("issue", "import_positional_hole"),
        );
        return;
    }

    // Count `§` holes — only one allowed.
    let hole_count = path.matches('§').count();
    if hole_count > 1 {
        errors.push(
            SemanticError::structured(SemanticErrorKind::ImportAbsent, span)
                .with_arg("issue", "import_multiple_holes")
                .with_arg("path", path),
        );
        return;
    }

    // Check that the template name is known.
    let name = strip_template_name(path);
    if name.is_empty() {
        errors.push(
            SemanticError::structured(SemanticErrorKind::ImportAbsent, span)
                .with_arg("issue", "import_bare_glyph"),
        );
        return;
    }

    let is_known = templates.contains_key(name) || default_names.contains(&name);
    if !is_known {
        errors.push(
            SemanticError::structured(SemanticErrorKind::ImportAbsent, span)
                .with_arg("issue", "import_unknown_template")
                .with_arg("name", name),
        );
    }
}

/// Extract the template name from a `§name/rest` specifier.
///
/// Returns `""` if the glyph is not followed by a name.
fn strip_template_name(path: &str) -> &str {
    let after_glyph = &path[GLYPH_BYTE_LEN..];
    if let Some(slash) = after_glyph.find('/') {
        &after_glyph[..slash]
    } else {
        after_glyph
    }
}

/// Returns `true` if the path contains a `..` segment.
///
/// Single split over `/` components (one pass) instead of four substring scans.
fn contains_parent_traversal(path: &str) -> bool {
    path.split('/').any(|component| component == "..")
}

/// Returns `true` if the path is absolute (Unix or Windows).
fn is_absolute_path(path: &str) -> bool {
    path.starts_with('/') || path.chars().nth(1) == Some(':')
}

/// Returns `true` if the path is a valid `provider:module` form.
///
/// A valid provider import has the form `name:module` where `name` is a
/// non-empty alphanumeric identifier and `module` is a non-empty identifier
/// without `//` (which would indicate a URL scheme like `https://…`).
fn is_valid_provider_import(path: &str) -> bool {
    let Some((provider, module)) = path.split_once(':') else {
        return false;
    };
    if provider.is_empty() || module.is_empty() {
        return false;
    }
    // Provider must be alphanumeric (no slashes, dots, etc.).
    if !provider.chars().all(char::is_alphanumeric) {
        return false;
    }
    // Module must not contain `//` (reject URLs like `https://example.com/lib`).
    if module.contains("//") {
        return false;
    }
    true
}
