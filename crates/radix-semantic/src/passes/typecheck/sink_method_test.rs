//! `immerge` — the contract's `sink` method form on the collection receivers.
//!
//! `xs.sink(v)` is the method twin of the `⇜` sink-into operator: the
//! canonical Latin `immerge` is registered once on `lista` and once on
//! `copia`, both lowering to the `collection Append` MIR op that `appende` /
//! `adde` already use. The English surface is `sink`; the item must be exactly
//! the receiver's `T`.
//!
//! Fixtures are written in the English surface.

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

fn assert_accepts(source: &str) {
    let result = analyze_en(source);
    assert!(
        !result.diagnostics.iter().any(|d| d.is_error()),
        "expected no error, got: {:?}",
        result.diagnostics
    );
}

fn assert_rejects(source: &str, kind: SemanticErrorKind, expected: &str) {
    let result = analyze_en(source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.kind == kind && issue(d) == Some(expected)),
        "expected {kind:?} issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

// =============================================================================
// Accepted shapes
// =============================================================================

#[test]
fn sink_method_appends_on_a_list() {
    assert_accepts("main {\n    mut list<int> xs ← []\n    xs.sink(3)\n    print xs.length()\n}");
}

#[test]
fn sink_method_appends_on_a_set() {
    assert_accepts("main {\n    mut set<int> ys ← []\n    ys.sink(3)\n    print ys.length()\n}");
}

// =============================================================================
// Rejected shapes
// =============================================================================

#[test]
fn sink_method_rejects_a_wider_item() {
    assert_rejects(
        "main {\n    mut list<string> xs ← []\n    xs.sink(1)\n}",
        SemanticErrorKind::TypeMismatch,
        "argument_type_mismatch",
    );
}
