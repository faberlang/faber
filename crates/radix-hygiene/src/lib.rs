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

#[must_use]
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

#[must_use]
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
        counts.inline_test_modules += count_inline_test_modules(&file.scrubbed);
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
        "inline #[cfg(test)] mod NAME {",
        counts.inline_test_modules,
        budgets.inline_test_modules,
    );
    assert_budget(
        "#[test] in production files",
        counts.test_attr_in_production,
        budgets.test_attr_in_production,
    );
}

/// One toolchain spawn (`Command::new(…)`) found in a production file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpawn {
    pub path: PathBuf,
    /// 1-based line number of the spawn token in the source file.
    pub line: usize,
}

/// Scan production files for `Command::new(…)` spawns.
///
/// Detection runs on the scrubbed source (comments, strings, and char
/// literals are blanked), so a spawn spelled out in prose does not count —
/// same convention as the hygiene budgets. A match only counts when the
/// `Command` token starts at an identifier boundary, so `MyCommand::new`
/// is not a false positive while `std::process::Command::new` is caught.
#[must_use]
pub fn scan_command_new(files: &[SourceFile]) -> Vec<CommandSpawn> {
    let mut spawns = Vec::new();
    for file in files {
        for (index, line) in file.scrubbed.lines().enumerate() {
            let mut offset = 0;
            while let Some(relative) = line[offset..].find("Command::new") {
                let absolute = offset + relative;
                let preceded_by_identifier = line[..absolute]
                    .chars()
                    .next_back()
                    .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_');
                if !preceded_by_identifier {
                    spawns.push(CommandSpawn {
                        path: file.path.clone(),
                        line: index + 1,
                    });
                }
                offset = absolute + "Command::new".len();
            }
        }
    }
    spawns
}

/// One `std::env::var` / `std::env::var_os` read of a `FABER_*` or `RADIX_*`
/// variable found in a production file.
///
/// Operator ruling 2026-10-14: a `FABER_*` / `RADIX_*` environment variable
/// may only be read by the internal test harness. Compiler and product
/// behavior is CLI flags or `faber.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvRead {
    pub path: PathBuf,
    /// 1-based line of the `env::var(…)` / `env::var_os(…)` call.
    pub line: usize,
    /// The `FABER_*` / `RADIX_*` variable name the read resolves to.
    pub name: String,
}

/// True when `path` is internal test-harness code, which operator ruling
/// 2026-10-14 exempts from the `FABER_*` / `RADIX_*` environment-read ban.
///
/// Exempt: any path with a `tests` or `test_support` component, a `*_test.rs`
/// or `test_support.rs` file, a `golden.rs` file, and the `crates/exempla`
/// corpus e2e harness. Production sources such as
/// `crates/faber/src/package/mod.rs` are not exempt.
#[must_use]
pub fn is_env_harness_path(path: &Path) -> bool {
    let components: Vec<String> = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    let Some(file_name) = components.last() else {
        return false;
    };
    if file_name.ends_with("_test.rs") || file_name == "test_support.rs" || file_name == "golden.rs"
    {
        return true;
    }
    if components
        .iter()
        .any(|component| component == "tests" || component == "test_support")
    {
        return true;
    }
    components
        .windows(2)
        .any(|pair| pair[0] == "crates" && pair[1] == "exempla")
}

const ENV_READ_NEEDLES: [&str; 2] = ["env::var(", "env::var_os("];

/// Scan production files for `std::env::var` / `std::env::var_os` reads of
/// `FABER_*` or `RADIX_*` variables.
///
/// Operator ruling 2026-10-14: those reads are banned outside the internal
/// test harness (`is_env_harness_path`); product behavior is CLI flags or
/// `faber.toml`.
///
/// Detection runs on the scrubbed source, so a comment or a string literal
/// that merely spells the call is not a hit, and a hit must start at an
/// identifier boundary — `myenv::var(…)` and `proto_env::var(…)` are not
/// hits. The name is resolved from the real content: a quoted literal counts
/// only when it starts with `FABER_` or `RADIX_`, so platform variables such
/// as `PATH`, `HOME` or `XDG_CACHE_HOME` do not, and a bare identifier is
/// chased back to the `FABER_*` / `RADIX_*` literal bound to it (the
/// `let name = "FABER_LIBRARY_HOME"; std::env::var_os(name)` shape).
#[must_use]
pub fn scan_tool_env_reads(files: &[SourceFile]) -> Vec<EnvRead> {
    let mut reads = Vec::new();
    for file in files {
        if is_env_harness_path(&file.path) {
            continue;
        }
        let content_lines: Vec<&str> = file.content.lines().collect();
        for (index, line) in file.scrubbed.lines().enumerate() {
            for (char_offset, needle) in env_read_calls(line) {
                let Some(name) = resolve_env_name(file, &content_lines, index, char_offset, needle)
                else {
                    continue;
                };
                reads.push(EnvRead {
                    path: file.path.clone(),
                    line: index + 1,
                    name,
                });
            }
        }
    }
    reads
}

/// `env::var(` / `env::var_os(` occurrences on one scrubbed line, as a char
/// offset plus the matched needle, in source order.
fn env_read_calls(line: &str) -> Vec<(usize, &'static str)> {
    let mut calls = Vec::new();
    for needle in ENV_READ_NEEDLES {
        for (byte_offset, _matched) in line.match_indices(needle) {
            let preceded_by_identifier = line[..byte_offset]
                .chars()
                .next_back()
                .is_some_and(is_ident_continue);
            if !preceded_by_identifier {
                calls.push((line[..byte_offset].chars().count(), needle));
            }
        }
    }
    calls.sort_by_key(|(offset, _)| *offset);
    calls
}

/// Resolve the variable name of one call from the real (unscrubbed) content:
/// the call line plus at most the two following lines, so a multi-line call
/// resolves too.
fn resolve_env_name(
    file: &SourceFile,
    content_lines: &[&str],
    index: usize,
    char_offset: usize,
    needle: &str,
) -> Option<String> {
    let window = content_window(content_lines, index, char_offset);
    let rest = window.strip_prefix(needle)?.trim_start();
    if let Some(literal) = quoted_literal(rest) {
        return is_tool_env_name(&literal).then_some(literal);
    }
    let identifier = leading_identifier(rest)?;
    chase_bound_literal(&file.content, identifier)
}

/// The content of the call line from `char_offset` onward plus at most the two
/// following lines, joined by newlines.
fn content_window(content_lines: &[&str], index: usize, char_offset: usize) -> String {
    let mut window = String::new();
    for (offset, line) in content_lines.iter().enumerate().skip(index).take(3) {
        if offset == index {
            window.extend(line.chars().skip(char_offset));
        } else {
            window.push_str(line);
        }
        window.push('\n');
    }
    window
}

/// The literal body of a leading `"…"` argument, if the argument is a quoted
/// literal.
fn quoted_literal(text: &str) -> Option<String> {
    let mut chars = text.strip_prefix('"')?.chars();
    let mut literal = String::new();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                literal.push('\\');
                literal.push(chars.next()?);
            }
            '"' => return Some(literal),
            other => literal.push(other),
        }
    }
    None
}

/// The leading bare identifier of an argument, if any.
fn leading_identifier(rest: &str) -> Option<&str> {
    if !is_ident_start(rest.chars().next()?) {
        return None;
    }
    let mut end = 0;
    for ch in rest.chars() {
        if !is_ident_continue(ch) {
            break;
        }
        end += ch.len_utf8();
    }
    Some(&rest[..end])
}

/// Chase `identifier` through the whole file content to the quoted
/// `FABER_*` / `RADIX_*` literal it is bound to (the indirect-name shape).
fn chase_bound_literal(content: &str, identifier: &str) -> Option<String> {
    for line in content.lines() {
        let mut offset = 0;
        while let Some(relative) = line[offset..].find(identifier) {
            let start = offset + relative;
            let end = start + identifier.len();
            offset = end;
            let bounded_before = line[..start]
                .chars()
                .next_back()
                .is_none_or(|ch| !is_ident_continue(ch));
            let bounded_after = line[end..]
                .chars()
                .next()
                .is_none_or(|ch| !is_ident_continue(ch));
            if !bounded_before || !bounded_after {
                continue;
            }
            let Some(rest) = line[end..].trim_start().strip_prefix('=') else {
                continue;
            };
            let Some(literal) = quoted_literal(rest.trim_start()) else {
                continue;
            };
            if is_tool_env_name(&literal) {
                return Some(literal);
            }
        }
    }
    None
}

fn is_tool_env_name(name: &str) -> bool {
    name.starts_with("FABER_") || name.starts_with("RADIX_")
}

/// Assert each production file that has a companion `*_test.rs` embeds the
/// repo `#[cfg(test)]` + `#[path = …]` module wiring.
///
/// A lane-gated companion may stack a feature gate between `#[cfg(test)]`
/// and the `#[path]` line (`#[cfg(feature = "mir-core")]`, the U8
/// feature-granularity convention), which the check accepts.
///
/// # Panics
///
/// Panics when a companion test file exists but the production source is
/// missing the expected module attributes.
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
            is_companion_wired(&file.content, &companion_name),
            "{} has companion test {}, but is missing the repo convention:\n{}",
            file.path.display(),
            companion.display(),
            expected
        );
    }
}

/// True when the production source wires `mod tests;` via the `#[cfg(test)]` +
/// `#[path = …]` pair, with zero or more stacked `#[cfg(feature = …)]` gates
/// allowed between the two (the feature-granularity convention).
fn is_companion_wired(content: &str, companion_name: &str) -> bool {
    let needle = format!("#[path = \"{companion_name}\"]\nmod tests;");
    let Some(pos) = content.find(&needle) else {
        return false;
    };
    let above: Vec<&str> = content[..pos]
        .lines()
        .rev()
        .take_while(|line| line.trim_start().starts_with("#["))
        .map(str::trim)
        .collect();
    above.last().copied() == Some("#[cfg(test)]")
        && above
            .iter()
            .all(|attr| *attr == "#[cfg(test)]" || attr.starts_with("#[cfg(feature = "))
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

/// Count each cfg(test)-gated inline `mod NAME { … }` body, including
/// `#[cfg(all(test, …))]` and stacked attributes. Companion `mod NAME;`
/// wiring is not counted.
#[must_use]
pub fn count_inline_test_modules(scrubbed: &str) -> usize {
    let chars: Vec<char> = scrubbed.chars().collect();
    let mut count = 0;
    let mut i = 0;
    let mut pending_attrs: Vec<String> = Vec::new();

    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        if chars[i] == '#'
            && i + 1 < chars.len()
            && chars[i + 1] == '['
            && let Some((attr, end)) = parse_attribute(&chars, i)
        {
            pending_attrs.push(attr);
            i = end;
            continue;
        }
        if let Some((end, is_inline)) = parse_mod_item(&chars, i) {
            if is_inline && pending_attrs.iter().any(|attr| cfg_enables_test(attr)) {
                count += 1;
            }
            pending_attrs.clear();
            i = end;
            continue;
        }
        pending_attrs.clear();
        i += 1;
    }
    count
}

fn parse_attribute(chars: &[char], start: usize) -> Option<(String, usize)> {
    if start + 1 >= chars.len() || chars[start] != '#' || chars[start + 1] != '[' {
        return None;
    }
    let end = skip_balanced(chars, start + 1, '[', ']')?;
    Some((chars[start..end].iter().collect(), end))
}

/// Returns `(end_index, is_inline_body)` for `pub(…)? mod NAME {` / `;`.
fn parse_mod_item(chars: &[char], start: usize) -> Option<(usize, bool)> {
    let mut i = start;
    if ident_at(chars, i, "pub") {
        i += 3;
        i = skip_ws(chars, i);
        if i < chars.len() && chars[i] == '(' {
            i = skip_balanced(chars, i, '(', ')')?;
            i = skip_ws(chars, i);
        }
    }
    if !ident_at(chars, i, "mod") {
        return None;
    }
    i += 3;
    i = skip_ws(chars, i);
    if i >= chars.len() || !is_ident_start(chars[i]) {
        return None;
    }
    i += 1;
    while i < chars.len() && is_ident_continue(chars[i]) {
        i += 1;
    }
    i = skip_ws(chars, i);
    match chars.get(i) {
        Some('{') => Some((i + 1, true)),
        Some(';') => Some((i + 1, false)),
        _ => None,
    }
}

fn cfg_enables_test(attr: &str) -> bool {
    let compact: String = attr.chars().filter(|ch| !ch.is_whitespace()).collect();
    let Some(inner) = compact
        .strip_prefix("#[cfg(")
        .and_then(|rest| rest.strip_suffix(")]"))
    else {
        return false;
    };
    ident_tokens(&strip_not_groups(inner)).any(|token| token == "test")
}

fn strip_not_groups(pred: &str) -> String {
    let chars: Vec<char> = pred.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if ident_at(&chars, i, "not") {
            let j = skip_ws(&chars, i + 3);
            if j < chars.len()
                && chars[j] == '('
                && let Some(end) = skip_balanced(&chars, j, '(', ')')
            {
                i = end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn ident_tokens(src: &str) -> impl Iterator<Item = &str> {
    src.split(|ch: char| !is_ident_continue(ch))
        .filter(|token| !token.is_empty())
}

fn ident_at(chars: &[char], i: usize, ident: &str) -> bool {
    let ident: Vec<char> = ident.chars().collect();
    if i + ident.len() > chars.len() || chars[i..i + ident.len()] != ident {
        return false;
    }
    let end = i + ident.len();
    (i == 0 || !is_ident_continue(chars[i - 1]))
        && (end == chars.len() || !is_ident_continue(chars[end]))
}

fn skip_ws(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

fn skip_balanced(chars: &[char], open_at: usize, open: char, close: char) -> Option<usize> {
    if chars.get(open_at) != Some(&open) {
        return None;
    }
    let mut depth = 0;
    for (offset, ch) in chars[open_at..].iter().enumerate() {
        if *ch == open {
            depth += 1;
        } else if *ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(open_at + offset + 1);
            }
        }
    }
    None
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn looks_like_char_literal<I>(chars: &std::iter::Peekable<I>) -> bool
where
    I: Iterator<Item = char> + Clone,
{
    let mut lookahead = chars.clone();
    let Some(first) = lookahead.next() else {
        return false;
    };

    match first {
        '\\' => lookahead.next().is_some() && lookahead.next() == Some('\''),
        first if first != '\'' && first != '\n' => lookahead.next() == Some('\''),
        _ => false,
    }
}

/// Detects the opening of a raw string (`r"`, `r#"`, `br##"`, ...) whose
/// first character `first` (`r` or `b`) was just consumed. Returns the number
/// of further characters through the opening quote and the `#` count.
fn raw_string_open<I>(first: char, chars: &std::iter::Peekable<I>) -> Option<(usize, usize)>
where
    I: Iterator<Item = char> + Clone,
{
    let mut lookahead = chars.clone();
    let mut consumed = 0;
    if first == 'b' {
        if lookahead.next() != Some('r') {
            return None;
        }
        consumed += 1;
    }
    let mut hashes = 0;
    loop {
        match lookahead.next()? {
            '#' => {
                hashes += 1;
                consumed += 1;
            }
            '"' => return Some((consumed + 1, hashes)),
            _ => return None,
        }
    }
}

fn closes_raw_string<I>(chars: &std::iter::Peekable<I>, hashes: usize) -> bool
where
    I: Iterator<Item = char> + Clone,
{
    let mut lookahead = chars.clone();
    (0..hashes).all(|_| lookahead.next() == Some('#'))
}

/// Strip comments and string/char literal bodies so budget scans see code only.
#[must_use]
// State machine for full-file scrubbing is naturally long; split deferred.
#[allow(clippy::too_many_lines)]
pub fn scrub_rust_source(source: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Code,
        LineComment,
        BlockComment,
        String,
        RawString(usize),
        Char,
    }

    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut state = State::Code;
    let mut block_depth = 0usize;
    let mut prev_ident = false;

    while let Some(ch) = chars.next() {
        let was_ident = prev_ident;
        prev_ident = ch.is_ascii_alphanumeric() || ch == '_';
        match state {
            State::Code => match ch {
                'r' | 'b' if !was_ident => {
                    if let Some((consumed, hashes)) = raw_string_open(ch, &chars) {
                        for _ in 0..=consumed {
                            out.push(' ');
                        }
                        for _ in 0..consumed {
                            chars.next();
                        }
                        prev_ident = false;
                        state = State::RawString(hashes);
                    } else {
                        out.push(ch);
                    }
                }
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
                    block_depth = 1;
                    state = State::BlockComment;
                }
                '"' => {
                    out.push(' ');
                    state = State::String;
                }
                '\'' => {
                    // Distinguish lifetimes ('ident) from char literals ('x' / '\x').
                    // A closing quote within the short literal forms takes precedence
                    // over the shared alphabetic prefix of a lifetime and char literal.
                    out.push(' ');
                    if looks_like_char_literal(&chars) {
                        state = State::Char;
                    } else if let Some(&next) = chars.peek() {
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
                if ch == '/' && chars.peek() == Some(&'*') {
                    out.push(' ');
                    out.push(' ');
                    chars.next();
                    block_depth += 1;
                } else if ch == '*' && chars.peek() == Some(&'/') {
                    out.push(' ');
                    out.push(' ');
                    chars.next();
                    block_depth -= 1;
                    if block_depth == 0 {
                        state = State::Code;
                    }
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
            State::RawString(hashes) => {
                if ch == '"' && closes_raw_string(&chars, hashes) {
                    out.push(' ');
                    for _ in 0..hashes {
                        chars.next();
                        out.push(' ');
                    }
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
