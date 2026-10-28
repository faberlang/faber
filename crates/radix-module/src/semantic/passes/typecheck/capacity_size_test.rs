//! Capacity slots take `size`: `queue<T, size N>`,
//! `stack<T, size N>`, `string<size N>` and `ascii<size N>`.
//!
//! The bare forms `queue<T, 4>`, `stack<T, 4>`, `string<4>` and `ascii<4>` are
//! retired by name (`SEM008 capacity_needs_size`), like the list's bare count.
//! The `_` hole stays bare.

use crate::driver::AnalyzeRequest;
use crate::{Config, Target};

fn english(source: &str) -> Vec<crate::Diagnostic> {
    let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .unwrap_or_else(|err| panic!("installed en pack should load: {err:?}"));
    let session = crate::driver::Session::new(
        Config::new()
            .with_locale_pack(pack)
            .with_target(Target::HirRust),
    );
    match crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()) {
        Ok(unit) => unit.diagnostics,
        Err(diagnostics) => diagnostics,
    }
}

fn has_issue(diagnostics: &[crate::Diagnostic], issue: &str) -> bool {
    diagnostics.iter().any(|diagnostic| {
        diagnostic.is_error()
            && diagnostic
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == issue)
    })
}

fn refused(source: &str, issue: &str) {
    let diagnostics = english(source);
    assert!(
        has_issue(&diagnostics, issue),
        "expected error issue {issue:?}, got: {diagnostics:?}",
    );
}

fn assert_accepts(source: &str) {
    let diagnostics = english(source);
    assert!(
        !diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn a_literal_size_is_a_capacity() {
    assert_accepts(
        r#"
main {
    mut queue<int, size 4> q ← empty
    mut stack<int, size 2> s ← empty
    let string<size 8> t ← "ab" ↦ string<size 8>
    let ascii<size 1> c ← 'x'
    print t
    print c
    print q.length()
    print s.length()
}
"#,
    );
}

#[test]
fn a_size_name_is_a_capacity() {
    assert_accepts(
        r"
fn depth<size N>(queue<int, size N> q, stack<int, size N> s) → int { return 0 }
fn width<size N>(ascii<size N> a, string<size N> t) → int { return 0 }
main { print 1 }
",
    );
}

#[test]
fn the_hole_stays_bare() {
    assert_accepts(
        r"
main {
    mut queue<int, _> q ← empty
    print q.length()
}
",
    );
}

#[test]
fn a_bare_capacity_is_retired_by_name() {
    refused("main { mut queue<int, 4> q ← empty\n print q.length() }", "capacity_needs_size");
    refused("main { mut stack<int, 4> s ← empty\n print s.length() }", "capacity_needs_size");
    refused(
        r#"main { let string<8> t ← "a" ↦ string<8>
 print t }"#,
        "capacity_needs_size",
    );
    refused("main { let ascii<4> c ← 'x'\n print c }", "capacity_needs_size");
    refused(
        "fn depth<size N>(queue<int, N> q) → int { return 0 }\nmain { print 1 }",
        "capacity_needs_size",
    );
    refused(
        "fn width<size N>(ascii<N> a) → int { return 0 }\nmain { print 1 }",
        "capacity_needs_size",
    );
}

#[test]
fn a_size_is_the_same_capacity_as_the_old_bare_count() {
    // `size 4` is the type the bare `4` was: the capacity is one index term.
    assert_accepts(
        r"
fn take(queue<int, size 4> q) → int { return q.length() }
main {
    mut queue<int, size 4> q ← empty
    print take(q)
}
",
    );
}
