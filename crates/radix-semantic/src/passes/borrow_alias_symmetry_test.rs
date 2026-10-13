//! A `var mut` local holds a live mutable borrow of the storage it is bound
//! from. Alias rows are recorded one direction (`holder ← root`), so two
//! aliases of one storage must conflict whichever alias the check starts
//! from: `x ← b` then `y ← a` (with `b ← a`) is the same overlap as the
//! mirrored order, and a read or write through either name sees the live
//! borrow. Sequential mut locals whose earlier borrow died at its last use
//! stay legal.

use crate::SemanticErrorKind;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};

/// The fixtures are written in the English surface.
fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

fn errors(source: &str) -> Vec<(SemanticErrorKind, String)> {
    analyze_en(source)
        .result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| (d.kind.clone(), issue(d).unwrap_or_default().to_string()))
        .collect()
}

fn assert_rejects(source: &str, expected: &str) {
    let found = errors(source);
    assert!(
        found.iter().any(
            |(kind, issue)| *kind == SemanticErrorKind::MutableBorrowConflict && issue == expected
        ),
        "expected {expected:?}, got: {found:?}"
    );
}

fn assert_accepts(source: &str) {
    let found = errors(source);
    assert!(found.is_empty(), "expected no error, got: {found:?}");
}

// =============================================================================
// Two mut locals over one storage: the overlap is seen in both orders
// =============================================================================

#[test]
fn deep_mut_local_then_shallow_mut_local_is_rejected() {
    assert_rejects(
        r"main {
    var list<int> a ← [1, 2, 3]
    var list<int> b ← a
    var mut list<int> x ← b
    var mut list<int> y ← a
    print x.length()
    print y.length()
}",
        "mutable_borrow_conflict",
    );
}

#[test]
fn shallow_mut_local_then_deep_mut_local_is_rejected() {
    assert_rejects(
        r"main {
    var list<int> a ← [1, 2, 3]
    var list<int> b ← a
    var mut list<int> y ← a
    var mut list<int> x ← b
    print x.length()
    print y.length()
}",
        "mutable_borrow_conflict",
    );
}

// =============================================================================
// A read or write through either alias sees the live mut borrow
// =============================================================================

#[test]
fn read_of_the_source_while_a_deep_alias_is_mutably_borrowed_is_rejected() {
    assert_rejects(
        r"main {
    var list<int> a ← [1, 2, 3]
    var list<int> b ← a
    var mut list<int> x ← b
    print a.length()
    print x.length()
}",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn read_of_a_deep_alias_while_the_source_is_mutably_borrowed_is_rejected() {
    assert_rejects(
        r"main {
    var list<int> a ← [1, 2, 3]
    var list<int> b ← a
    var mut list<int> x ← a
    print b.length()
    print x.length()
}",
        "use_while_mutably_borrowed",
    );
}

#[test]
fn write_to_the_source_while_a_deep_alias_is_mutably_borrowed_is_rejected() {
    assert_rejects(
        r"main {
    var list<int> a ← [1, 2, 3]
    var list<int> b ← a
    var mut list<int> x ← b
    a.append(4)
    print x.length()
}",
        "write_while_borrowed",
    );
}

// =============================================================================
// Legal: the mut local's own use, and a borrow that died at its last use
// =============================================================================

#[test]
fn use_through_the_mut_local_itself_stays_legal() {
    assert_accepts(
        r"main {
    var list<int> a ← [1, 2, 3]
    var list<int> b ← a
    var mut list<int> x ← b
    x.append(4)
    print x.length()
}",
    );
}

#[test]
fn second_mut_local_after_the_first_borrows_last_use_is_accepted() {
    assert_accepts(
        r"main {
    var list<int> a ← [1, 2, 3]
    var list<int> b ← a
    var mut list<int> x ← b
    print x.length()
    var mut list<int> y ← a
    print y.length()
}",
    );
}

#[test]
fn read_of_an_alias_after_the_mut_local_last_use_is_accepted() {
    assert_accepts(
        r"main {
    var list<int> a ← [1, 2, 3]
    var list<int> b ← a
    var mut list<int> x ← b
    print x.length()
    print a.length()
    print b.length()
}",
    );
}
