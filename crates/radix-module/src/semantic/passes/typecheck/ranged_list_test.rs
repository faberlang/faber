//! List length ranges: `list<T, lo‥hi>` and `list<T, lo…hi>`.
//!
//! A length range is the second argument of `list`. The trapping policy is the
//! only one, the ends are integer literals or `∞`, a range whose minimum is one
//! or more never holds an empty list (so `first` and `last` return `T`), and
//! `list<T>` and `list<T, N>` are the ranges `0…∞` and `0…N` (one type, not a
//! new family).

use crate::driver::AnalyzeRequest;
use crate::{Config, Target};

fn pack(id: &str) -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(id))
        .unwrap_or_else(|err| panic!("installed {id} pack should load: {err:?}"))
}

fn english(source: &str) -> Vec<crate::Diagnostic> {
    let session = crate::driver::Session::new(
        Config::new()
            .with_locale_pack(pack("en"))
            .with_target(Target::HirRust),
    );
    match crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()) {
        Ok(unit) => unit.diagnostics,
        Err(diagnostics) => diagnostics,
    }
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

#[test]
fn a_range_is_the_second_argument_of_list() {
    assert_accepts(
        r"
main {
    mut list<int, 4‥8> a ← [1, 2, 3, 4]
    mut list<int, 1…∞> b ← [1]
    let list<int, 2…2> c ← [1, 2]
    print a
    print b
    print c
}
",
    );
}

#[test]
fn a_range_with_a_minimum_of_one_never_returns_none() {
    // `first` and `last` are the element, not `int ∪ none`.
    assert_accepts(
        r"
main {
    let list<int, 1…∞> xs ← [1, 2, 3]
    let int head ← xs.first()
    let int tail ← xs.last()
    print head + tail
}
",
    );
}

#[test]
fn a_plain_list_still_returns_an_optional_element() {
    refused(
        r"
main {
    let list<int> xs ← [1, 2, 3]
    let int head ← xs.first()
    print head
}
",
        "optional_into_plain_cell",
    );
}

#[test]
fn growing_past_the_maximum_reports_on_the_error_channel() {
    // `append` on a finite range is failable, so it needs a handler.
    assert_accepts(
        r#"
main {
    mut list<int, 1‥3> xs ← [1]
    do {
        xs.append(2)
    }
    catch err {
        print err
    }
}
"#,
    );
}

#[test]
fn a_literal_outside_the_range_is_refused_at_compile_time() {
    refused(
        "main { let list<int, 2‥5> a ← [1]\n print a }",
        "ranged_list_length_out_of_range",
    );
    refused(
        "main { let list<int, 2‥5> a ← [1, 2, 3, 4, 5]\n print a }",
        "ranged_list_length_out_of_range",
    );
}

#[test]
fn empty_is_refused_for_a_range_that_never_holds_zero() {
    refused(
        "main { let list<int, 1…∞> a ← empty\n print a }",
        "ranged_list_length_out_of_range",
    );
}

#[test]
fn a_converted_list_is_checked_where_it_is_converted() {
    assert_accepts(
        r"
main {
    let list<int> src ← [1, 2, 3]
    let list<int, 2‥5> ok ← src ↦ list<int, 2‥5> ⊥ [0, 0]
    let list<int> back ← ok ↦ list<int>
    print back
}
",
    );
}

#[test]
fn the_sugar_and_the_range_are_one_type() {
    assert_accepts(
        r"
main {
    let list<int> plain ← [7, 8]
    let list<int, 0…∞> open ← plain
    let list<int, 3> sized ← [1, 2, 3]
    let list<int, 0…3> same ← sized
    print open
    print same
}
",
    );
}

#[test]
fn a_range_that_holds_no_length_is_refused() {
    refused("main { let list<int, 4‥4> a ← [1]\n print a }", "ranged_list_range_empty");
    refused("main { let list<int, 5…3> a ← [1]\n print a }", "ranged_list_range_empty");
}

#[test]
fn a_negative_end_is_refused() {
    refused("main { let list<int, -1‥3> a ← [1]\n print a }", "ranged_list_bound_negative");
}

#[test]
fn a_named_end_is_refused() {
    refused(
        "main {\n let int n ← 3\n let list<int, 1‥n> a ← [1]\n print a }",
        "ranged_list_bound_not_constant",
    );
}

#[test]
fn a_policy_word_around_a_list_is_refused_by_name() {
    for word in ["saturating", "trapping", "wrapping"] {
        refused(
            &format!("main {{ let {word}<list<int>, 1…3> a ← [1]\n print a }}"),
            "ranged_list_policy_word",
        );
    }
}
