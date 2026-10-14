//! Workspace census guard for inline regex literals (ruling d381ae56).
//!
//! The lexer now opens a raw `/.../` body after the regex type word or `_`.
//! This test proves the new rule is a no-op on existing source: every `.fab`
//! file under the corpus roots must lex with no `RegexString` token and no
//! `unterminated_regex_literal` / `empty_regex_literal` issue. The failure
//! message lists each offending file and line so a real collision is fixed at
//! the source instead of by weakening this guard.
//!
//! A genuine existing division whose left operand is a `regex` binding (or
//! `_`) is listed in [`ALLOWED_DIVISIONS`] with a comment naming the file and
//! line; nothing else may be skipped.

use crate::{LexError, TokenKind, lex};
use std::fs;
use std::path::{Path, PathBuf};

/// Existing divisions that the regex-literal rule would reclassify, named by
/// repo-relative path and source line. Empty means no existing file collides;
/// add an entry only with a comment naming the binding and why the `/` is
/// arithmetic.
const ALLOWED_DIVISIONS: &[(&str, u32)] = &[];

/// Issue facts this census treats as a regex-literal regression.
const REGEX_ISSUES: &[&str] = &["unterminated_regex_literal", "empty_regex_literal"];

/// Guards against an empty walk passing the census vacuously.
const MIN_FILES_VISITED: usize = 500;

#[test]
fn no_existing_fab_file_lexes_an_inline_regex_literal() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in ["corpus", "lib", "acceptance", "examples", "corpus-product"] {
        collect_fab_files(&root.join(dir), &mut files);
    }
    assert!(
        files.len() >= MIN_FILES_VISITED,
        "regex census visited only {} .fab files under {}; expected at least {}",
        files.len(),
        root.display(),
        MIN_FILES_VISITED
    );

    let mut failures: Vec<String> = Vec::new();
    for path in &files {
        let Ok(source) = fs::read_to_string(path) else {
            continue;
        };
        let result = lex(&source);
        for token in &result.tokens {
            let line = line_of(&source, token.span.start);
            if matches!(token.kind, TokenKind::RegexString(_)) && !allowed(&root, path, line) {
                failures.push(format!("{}:{line}: RegexString token", path.display()));
            }
        }
        for error in &result.errors {
            let line = line_of(&source, error.span.start);
            if allowed(&root, path, line) {
                continue;
            }
            for issue in REGEX_ISSUES {
                if has_issue(error, issue) {
                    failures.push(format!("{}:{line}: {issue}", path.display()));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "existing .fab files must not lex inline regex literals; offenders:\n{}",
        failures.join("\n")
    );
}

/// Whether `error` carries exactly this `issue` fact.
fn has_issue(error: &LexError, issue: &str) -> bool {
    error.args
        .iter()
        .any(|arg| arg.name == "issue" && arg.value.as_str() == issue)
}

/// Recursively collect `.fab` files below `dir`; a missing root is skipped.
fn collect_fab_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fab_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "fab") {
            out.push(path);
        }
    }
}

/// One-based source line containing the byte `offset`.
fn line_of(source: &str, offset: u32) -> u32 {
    let end = usize::try_from(offset).unwrap_or(source.len()).min(source.len());
    #[allow(clippy::cast_possible_truncation)]
    let line = source[..end].bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
    line
}

/// Whether this exact path and line is a consciously allowed division.
fn allowed(root: &Path, path: &Path, line: u32) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let relative = relative.to_string_lossy().replace('\\', "/");
    ALLOWED_DIVISIONS
        .iter()
        .any(|(allowed_path, allowed_line)| *allowed_path == relative && *allowed_line == line)
}
