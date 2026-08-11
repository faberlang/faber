//! Path filters for which `*.proba` sources `faber test` loads.

use std::path::{Component, Path};

/// Include/exclude patterns for test sources relative to the package source root.
///
/// Patterns use a small glob dialect (`*` = any run of characters, including `/`).
/// Matching is against the relative path with forward slashes (e.g. `math.proba`,
/// `nested/extra.proba`). Bare names also match as a file-name suffix
/// (`math` matches `src/math.proba` → relative `math.proba`).
#[derive(Debug, Clone, Default)]
pub struct TestSourceFilter {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

impl TestSourceFilter {
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    /// Whether a discovered test source path should be loaded.
    ///
    /// Product `*.fab` files are never filtered here — only callers should pass
    /// `*.proba` (or other test-only roots).
    pub fn allows_path(&self, source_root: &Path, path: &Path) -> bool {
        let rel = relative_display(source_root, path);
        self.allows_relative(&rel)
    }

    pub fn allows_relative(&self, rel: &str) -> bool {
        let rel = normalize_rel(rel);
        if !self.include.is_empty() {
            let included = self
                .include
                .iter()
                .any(|pattern| path_pattern_matches(pattern, &rel));
            if !included {
                return false;
            }
        }
        if self
            .exclude
            .iter()
            .any(|pattern| path_pattern_matches(pattern, &rel))
        {
            return false;
        }
        true
    }
}

fn normalize_rel(rel: &str) -> String {
    rel.replace('\\', "/").trim_start_matches("./").to_owned()
}

fn relative_display(source_root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(source_root).unwrap_or(path);
    let mut parts = Vec::new();
    for component in relative.components() {
        if let Component::Normal(part) = component {
            parts.push(part.to_string_lossy());
        }
    }
    parts.join("/")
}

/// Match `pattern` against a normalized relative path.
///
/// - `*` matches any sequence (including `/`)
/// - no other metacharacters
/// - if the pattern has no `/` and no `*`, also match when the file name equals
///   the pattern or `pattern.proba` / `pattern.fab`
pub fn path_pattern_matches(pattern: &str, rel: &str) -> bool {
    let pattern = normalize_rel(pattern);
    let rel = normalize_rel(rel);
    if pattern.is_empty() {
        return rel.is_empty();
    }
    if glob_match(&pattern, &rel) {
        return true;
    }
    // Bare stem convenience: `--include math` matches `math.proba`.
    if !pattern.contains('/') && !pattern.contains('*') {
        let file_name = rel.rsplit('/').next().unwrap_or(&rel);
        if file_name == pattern
            || file_name == format!("{pattern}.proba")
            || file_name == format!("{pattern}.fab")
            || file_name.trim_end_matches(".proba") == pattern
            || file_name.trim_end_matches(".fab") == pattern
        {
            return true;
        }
    }
    false
}

fn glob_match(pattern: &str, text: &str) -> bool {
    // Recursive glob with only `*`.
    fn rec(p: &[u8], t: &[u8]) -> bool {
        match (p.first(), t.first()) {
            (None, None) => true,
            (Some(b'*'), _) => {
                // Match zero or more chars.
                if rec(&p[1..], t) {
                    return true;
                }
                if !t.is_empty() && rec(p, &t[1..]) {
                    return true;
                }
                false
            }
            (Some(pc), Some(tc)) if pc == tc => rec(&p[1..], &t[1..]),
            _ => false,
        }
    }
    rec(pattern.as_bytes(), text.as_bytes())
}

#[cfg(test)]
#[path = "test_source_filter_test.rs"]
mod test_source_filter_test;
