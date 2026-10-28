//! List length ranges on the MIR runner: `list<T, size lo‥hi>`.
//!
//! The method forms report a push past the maximum and a removal below the
//! minimum on `⇥`; the sink `⇜` dies; a conversion checks the length where it
//! runs and takes `⊥`; a range of at least one element reads `first` and
//! `last` as `T`. A failed call leaves the list unchanged.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn session() -> Session {
    let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("installed en pack should load");
    Session::new(
        Config::new()
            .with_locale_pack(pack)
            .with_target(Target::HirRust),
    )
}

fn run(body: &str) -> Vec<String> {
    let source = format!("main {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    match run_source(&session(), "ranged-list.fab", &source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

#[test]
fn a_never_empty_range_reads_first_and_last_as_the_element() {
    let out = run(
        "let list<int, size 1…∞> xs ← [4, 5, 6]\nlet int head ← xs.first()\nlet int tail ← xs.last()\nprint head + tail\nprint xs.length()",
    );
    assert_eq!(out, ["10", "3"]);
}

#[test]
fn a_range_without_a_maximum_grows_freely() {
    let out = run("mut list<int, size 1…∞> xs ← [1]\nxs.append(2)\nxs.append(3)\nprint xs");
    assert_eq!(out, ["[1, 2, 3]"]);
}

#[test]
fn a_push_past_the_maximum_reports_and_leaves_the_list_unchanged() {
    let out = run("mut list<int, size 2‥5> ys ← [10, 20]
do {
    ys.append(30)
    ys.append(40)
    ys.append(50)
    print \"unreachable\"
}
catch err {
    print \"full: §\"(err)
}
print ys");
    assert_eq!(out, ["full: list length range: push past the maximum", "[10, 20, 30, 40]"]);
}

#[test]
fn a_removal_below_the_minimum_reports_and_leaves_the_list_unchanged() {
    let out = run("mut list<int, size 2…4> ys ← [10, 20, 30]
do {
    print ys.remove_last()
    print ys.remove_first()
    print \"unreachable\"
}
catch err {
    print \"min: §\"(err)
}
print ys");
    assert_eq!(out, ["30", "min: list length range: removal below the minimum", "[10, 20]"]);
}

#[test]
fn a_conversion_checks_the_length_where_it_runs() {
    let out = run("let list<int> three ← [1, 2, 3]
let list<int, size 2‥5> fits ← three ↦ list<int, size 2‥5> ⊥ [0, 0]
let list<int, size 5…9> short ← three ↦ list<int, size 5…9> ⊥ [9, 9, 9, 9, 9]
print fits
print short");
    assert_eq!(out, ["[1, 2, 3]", "[9, 9, 9, 9, 9]"]);
}

#[test]
fn a_range_converts_back_to_a_plain_list() {
    let out = run("let list<int, size 1‥4> xs ← [1, 2]\nlet list<int> plain ← xs ↦ list<int>\nprint plain.length()");
    assert_eq!(out, ["2"]);
}

#[test]
#[should_panic(expected = "list length range: push past the maximum")]
fn a_sink_past_the_maximum_dies() {
    // `⇜` has no reporting channel: the failure aborts the run.
    run("mut list<int, size 1…2> xs ← [1]\nxs ⇜ 2\nxs ⇜ 3\nprint \"unreachable\"");
}

#[test]
fn a_sugar_form_and_its_range_are_one_type() {
    let out = run(
        "let list<int, size 3> sized ← [1, 2, 3]\nlet list<int, size 0…3> same ← sized\nlet list<int> plain ← [7]\nlet list<int, size 0…∞> open ← plain\nprint same\nprint open",
    );
    assert_eq!(out, ["[1, 2, 3]", "[7]"]);
}
