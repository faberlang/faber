//! List length ranges: `list<T, size lo‥hi>` and `list<T, size lo…hi>`.
//!
//! A length range is the second argument of `list`. The trapping policy is the
//! only one, the ends are integer literals or `∞`, a range whose minimum is one
//! or more never holds an empty list (so `first` and `last` return `T`), and
//! `list<T>` and `list<T, size N>` are the ranges `0…∞` and `0…N` (one type, not a
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
    mut list<int, size 4‥8> a ← [1, 2, 3, 4]
    mut list<int, size 1…∞> b ← [1]
    let list<int, size 2…2> c ← [1, 2]
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
    let list<int, size 1…∞> xs ← [1, 2, 3]
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
    mut list<int, size 1‥3> xs ← [1]
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
fn a_zero_based_range_accepts_each_literal_length_through_its_maximum() {
    for (name, literal) in [("empty", "[]"), ("short", "[1, 2]"), ("full", "[1, 2, 3]")] {
        assert_accepts(&format!("main {{ let list<int, size 0…3> {name} ← {literal}\n print {name} }}"));
    }
    refused(
        "main { let list<int, size 0…3> too_many ← [1, 2, 3, 4]\n print too_many }",
        "incompatible_tensor_index",
    );
}

#[test]
fn a_literal_outside_the_range_is_refused_at_compile_time() {
    refused(
        "main { let list<int, size 2‥5> a ← [1]\n print a }",
        "ranged_list_length_out_of_range",
    );
    refused(
        "main { let list<int, size 2‥5> a ← [1, 2, 3, 4, 5]\n print a }",
        "ranged_list_length_out_of_range",
    );
}

#[test]
fn empty_is_refused_for_a_range_that_never_holds_zero() {
    refused(
        "main { let list<int, size 1…∞> a ← empty\n print a }",
        "ranged_list_length_out_of_range",
    );
}

#[test]
fn a_converted_list_is_checked_where_it_is_converted() {
    assert_accepts(
        r"
main {
    let list<int> src ← [1, 2, 3]
    let list<int, size 2‥5> ok ← src ↦ list<int, size 2‥5> ⊥ [0, 0]
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
    let list<int, size 0…∞> open ← plain
    let list<int, size 3> sized ← [1, 2, 3]
    let list<int, size 0…3> same ← sized
    print open
    print same
}
",
    );
}

#[test]
fn a_range_that_holds_no_length_is_refused() {
    refused("main { let list<int, size 4‥4> a ← [1]\n print a }", "ranged_list_range_empty");
    refused("main { let list<int, size 5…3> a ← [1]\n print a }", "ranged_list_range_empty");
}

#[test]
fn a_negative_end_is_refused() {
    refused(
        "main { let list<int, size -1‥3> a ← [1]\n print a }",
        "ranged_list_bound_negative",
    );
}

#[test]
fn a_named_end_is_refused() {
    refused(
        "main {\n let int n ← 3\n let list<int, size 1‥n> a ← [1]\n print a }",
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

#[test]
fn a_bare_count_is_retired_by_name() {
    // `list<T, 4>` and `list<b8, 4>` are retired for `list<T, size 4>`.
    refused("main { let list<int, 4> a ← [1, 2, 3, 4]\n print a }", "list_count_needs_size");
    refused("main { let list<b8, 4> a ← []\n print a }", "list_count_needs_size");
    refused(
        "fn take<size N>(list<int, N> x) → int { return 0 }\nmain { print 1 }",
        "list_count_needs_size",
    );
}

#[test]
fn a_bare_range_bounds_a_value_so_a_list_refuses_it() {
    refused("main { let list<int, 1…8> a ← [1]\n print a }", "list_count_needs_size");
    refused("main { let list<int, 1‥∞> a ← [1]\n print a }", "list_count_needs_size");
}

#[test]
fn size_n_is_size_zero_to_n_and_the_hole_stays_bare() {
    assert_accepts(
        r"
fn take<size N>(list<int, size N> x) → int { return 0 }
main {
    let list<int, size 3> a ← [1, 2, 3]
    let list<int, size 0…3> b ← a
    let list<int, size 0‥4> c ← a
    let list<int, _> h ← [1]
    print b
    print c
    print h
}
",
    );
}

#[test]
fn a_size_range_names_its_own_bound_errors() {
    refused("main { let list<int, size 4‥4> a ← [1]\n print a }", "ranged_list_range_empty");
    refused(
        "main { let list<int, size -1‥3> a ← [1]\n print a }",
        "ranged_list_bound_negative",
    );
}

#[test]
fn a_kernel_and_the_air_lane_need_a_finite_upper_size() {
    // A kernel unrolls every slot up to the maximum, so `size …∞` is refused
    // by name there; a finite maximum is not refused by the legality rule.
    refused(
        "@ kernel\n@ public\nfn launch(list<int, size 1…∞> xs, u32 lane) → void {\n}\nmain { print 1 }",
        "nucleum_unbounded_list_size",
    );
    let finite =
        english("@ kernel\n@ public\nfn launch(list<int, size 1…8> xs, u32 lane) → void {\n}\nmain { print 1 }");
    assert!(
        !has_issue(&finite, "nucleum_unbounded_list_size"),
        "a finite size is legal: {finite:?}"
    );
    // The reverse-AD lane unrolls the same way.
    let air = english("@ radix lane \"air\"\nfn p(list<int, size 1…∞> xs) → int {\n    return 1\n}\nmain { print 1 }");
    assert!(
        air.iter().any(|diagnostic| diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "air_unbounded_list_size")),
        "diagnostics: {air:?}"
    );
}

// Vector lane counts: the long form takes `size`, as a list does.

#[test]
fn a_bare_vector_lane_count_is_retired_by_name() {
    for source in [
        "main { let vector<f32, 4> v ← [1.0, 2.0, 3.0, 4.0] ↦ vector<f32, size 4>\n print v }",
        "fn take<size N>(vector<f32, N> v) → int { return 0 }\nmain { print 1 }",
        "fn take<size N>(vector<f32, N + 1> v) → int { return 0 }\nmain { print 1 }",
        "fn take(vector<int, 2> v) → int { return 0 }\nmain { print 1 }",
    ] {
        refused(source, "vector_width_needs_size");
    }
}

#[test]
fn a_sized_vector_lane_count_and_the_unchanged_forms_check() {
    assert_accepts(
        r"
fn take<size N>(vector<f32, size N> v, vf32[N] w) → int { return 0 }
fn grow<size N>(vector<f32, size (N + 1)> v) → int { return 0 }
fn four(vector<f32, size 4> a, vf32[4] b, vector<f32, [4]> c, vector<f32, _> d) → int { return 0 }
main {
    let vector<f32, size 3> v ← [1.0, 2.0, 3.0] ↦ vector<f32, size 3>
    print v
}
",
    );
}

#[test]
fn size_n_and_the_shorthand_are_one_vector_type() {
    assert_accepts(
        r"
fn pass(vector<f32, size 4> a) → vf32[4] { return a }
main { print 1 }
",
    );
}
