//! Production-source hygiene budgets for Faber workspace crates.
//!
//! Scans non-test Rust sources for banned patterns (`.unwrap()`, `panic!(`, and
//! similar) and optional structural test-boundary violations.

use std::fs;
use std::path::{Path, PathBuf};

/// Monotonic ceilings for banned production patterns.
#[derive(Debug, Clone, Copy)]
pub struct Budgets {
    pub unwrap: usize,
    pub expect: usize,
    pub panic: usize,
    pub unreachable: usize,
    pub todo: usize,
    pub unimplemented: usize,
    pub let_underscore: usize,
    pub inline_test_modules: usize,
    pub test_attr_in_production: usize,
}

/// Observed production-only counts from one scan pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub unwrap: usize,
    pub expect: usize,
    pub panic: usize,
    pub unreachable: usize,
    pub todo: usize,
    pub unimplemented: usize,
    pub let_underscore: usize,
    pub inline_test_modules: usize,
    pub test_attr_in_production: usize,
}

/// Scanner configuration for one crate integration test.
#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub source_roots: Vec<PathBuf>,
    pub exclude_path_suffixes: Vec<String>,
    pub subtract_self_expect: bool,
    pub check_companion_convention: bool,
}

#[derive(Clone)]
pub struct SourceFile {
    pub path: PathBuf,
    pub content: String,
    pub scrubbed: String,
}

pub fn collect_production_files(config: &ScanConfig) -> Vec<SourceFile> {
    let mut files = Vec::new();
    for root in &config.source_roots {
        collect_rs_files(root, config, &mut files);
    }
    files
}

fn collect_rs_files(dir: &Path, config: &ScanConfig, out: &mut Vec<SourceFile>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "tests") {
                continue;
            }
            collect_rs_files(&path, config, out);
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.ends_with("_test.rs") || name.ends_with(".test.rs") {
            continue;
        }
        if is_test_support_path(&path) {
            continue;
        }
        if config
            .exclude_path_suffixes
            .iter()
            .any(|suffix| path.ends_with(suffix))
        {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let scrubbed = scrub_rust_source(&content);
        out.push(SourceFile {
            path,
            content,
            scrubbed,
        });
    }
}

pub fn count_budgets(files: &[SourceFile], subtract_self_expect: bool) -> Counts {
    let mut counts = Counts::default();
    for file in files {
        counts.unwrap += count_substring(&file.scrubbed, ".unwrap()");
        counts.expect += count_expect(&file.scrubbed, subtract_self_expect);
        counts.panic += count_substring(&file.scrubbed, "panic!(");
        counts.unreachable += count_substring(&file.scrubbed, "unreachable!(");
        counts.todo += count_substring(&file.scrubbed, "todo!(");
        counts.unimplemented += count_substring(&file.scrubbed, "unimplemented!(");
        counts.let_underscore += count_let_underscore(&file.scrubbed);
        if file.scrubbed.contains("#[cfg(test)]") && file.scrubbed.contains("mod tests {") {
            counts.inline_test_modules += 1;
        }
        if file.scrubbed.contains("#[test]") {
            counts.test_attr_in_production += 1;
        }
    }
    counts
}

pub fn assert_budgets(counts: Counts, budgets: Budgets) {
    assert_budget(".unwrap()", counts.unwrap, budgets.unwrap);
    assert_budget(".expect(", counts.expect, budgets.expect);
    assert_budget("panic!(", counts.panic, budgets.panic);
    assert_budget("unreachable!(", counts.unreachable, budgets.unreachable);
    assert_budget("todo!(", counts.todo, budgets.todo);
    assert_budget(
        "unimplemented!(",
        counts.unimplemented,
        budgets.unimplemented,
    );
    assert_budget("let _ =", counts.let_underscore, budgets.let_underscore);
    assert_budget(
        "inline #[cfg(test)] mod tests {",
        counts.inline_test_modules,
        budgets.inline_test_modules,
    );
    assert_budget(
        "#[test] in production files",
        counts.test_attr_in_production,
        budgets.test_attr_in_production,
    );
}

pub fn assert_companion_tests_use_cfg_path_module_convention(files: &[SourceFile]) {
    for file in files {
        let Some(companion) = companion_test_path(&file.path) else {
            continue;
        };
        if !companion.exists() {
            continue;
        }

        let companion_name = companion.file_name().unwrap_or_default().to_string_lossy();
        let expected = format!("#[cfg(test)]\n#[path = \"{companion_name}\"]\nmod tests;");
        assert!(
            file.content.contains(&expected),
            "{} has companion test {}, but is missing the repo convention:\n{}",
            file.path.display(),
            companion.display(),
            expected
        );
    }
}

fn is_test_support_path(path: &Path) -> bool {
    if path
        .file_name()
        .is_some_and(|name| name == "test_support.rs")
    {
        return true;
    }
    path.components()
        .any(|component| component.as_os_str() == "test_support")
}

fn companion_test_path(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_string_lossy();
    Some(path.with_file_name(format!("{stem}_test.rs")))
}

fn assert_budget(name: &str, observed: usize, budget: usize) {
    assert!(
        observed <= budget,
        "{name} budget exceeded: found {observed}, max {budget}."
    );
}

fn count_substring(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

fn count_expect(haystack: &str, subtract_self_expect: bool) -> usize {
    let mut count = count_substring(haystack, ".expect(");
    if subtract_self_expect {
        count = count.saturating_sub(count_substring(haystack, "self.expect("));
    }
    count
}

fn count_let_underscore(haystack: &str) -> usize {
    haystack
        .lines()
        .filter(|line| line.contains("let _ ="))
        .count()
}

pub fn scrub_rust_source(source: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Code,
        LineComment,
        BlockComment,
        String,
        Char,
    }

    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut state = State::Code;

    while let Some(ch) = chars.next() {
        match state {
            State::Code => match ch {
                '/' if chars.peek() == Some(&'/') => {
                    out.push(' ');
                    out.push(' ');
                    chars.next();
                    state = State::LineComment;
                }
                '/' if chars.peek() == Some(&'*') => {
                    out.push(' ');
                    out.push(' ');
                    chars.next();
                    state = State::BlockComment;
                }
                '"' => {
                    out.push(' ');
                    state = State::String;
                }
                '\'' => {
                    // Distinguish lifetimes ('ident) from char literals ('x' / '\x').
                    // A lifetime starts with ' followed by an identifier character.
                    out.push(' ');
                    if let Some(&next) = chars.peek() {
                        if next.is_ascii_alphabetic() || next == '_' {
                            // Lifetime: skip the identifier but keep emitting spaces
                            while let Some(&c) = chars.peek() {
                                if c.is_ascii_alphanumeric() || c == '_' {
                                    chars.next();
                                    out.push(' ');
                                } else {
                                    break;
                                }
                            }
                        } else {
                            state = State::Char;
                        }
                    }
                }
                _ => out.push(ch),
            },
            State::LineComment => {
                if ch == '\n' {
                    out.push('\n');
                    state = State::Code;
                } else {
                    out.push(' ');
                }
            }
            State::BlockComment => {
                if ch == '*' && chars.peek() == Some(&'/') {
                    out.push(' ');
                    out.push(' ');
                    chars.next();
                    state = State::Code;
                } else if ch == '\n' {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
            }
            State::String => {
                if ch == '\\' {
                    out.push(' ');
                    if let Some(escaped) = chars.next() {
                        out.push(if escaped == '\n' { '\n' } else { ' ' });
                    }
                } else if ch == '"' {
                    out.push(' ');
                    state = State::Code;
                } else if ch == '\n' {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
            }
            State::Char => {
                if ch == '\\' {
                    out.push(' ');
                    if let Some(escaped) = chars.next() {
                        out.push(if escaped == '\n' { '\n' } else { ' ' });
                    }
                } else if ch == '\'' {
                    out.push(' ');
                    state = State::Code;
                } else if ch == '\n' {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
            }
        }
    }

    out
}
