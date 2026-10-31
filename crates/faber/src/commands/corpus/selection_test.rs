use super::selection::{DomainTable, SelectionError, Selectors, select};
use super::{Case, Outcome, TargetExpectation, load_suite};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

fn case(id: &str, category: Option<&str>) -> Case {
    Case {
        id: id.to_owned(),
        category: category.map(str::to_owned),
        domain: None,
        dir: PathBuf::new(),
        input: PathBuf::new(),
        args: Vec::new(),
        stdin: None,
        expect: None,
        target: TargetExpectation {
            outcome: Outcome::Pass,
            phase: None,
            failure_kind: None,
            args: None,
            diagnostic_contains: None,
            reason: None,
            expect: None,
            timeout_secs: None,
            known_fail_why: None,
            rejects: false,
        },
        timeout: Duration::from_secs(1),
    }
}

fn domains(rows: &[(&str, &[&str])]) -> BTreeMap<String, Vec<String>> {
    rows.iter()
        .map(|(domain, categories)| {
            (
                (*domain).to_owned(),
                categories.iter().map(|item| (*item).to_owned()).collect(),
            )
        })
        .collect()
}

/// Six cases over three domains: numerics (arithmetic, bits), text (regex),
/// control (loop).
fn suite() -> (Vec<Case>, DomainTable) {
    let table = DomainTable::from_manifest(domains(&[
        ("numerics", &["arithmetic", "bits"]),
        ("text", &["regex"]),
        ("control", &["loop"]),
    ]))
    .expect("valid table")
    .expect("table present");
    let mut cases = vec![
        case("a/add", Some("arithmetic")),
        case("a/mul", Some("arithmetic")),
        case("b/shift", Some("bits")),
        case("r/match", Some("regex")),
        case("l/each", Some("loop")),
        case("l/while", Some("loop")),
    ];
    DomainTable::tag_cases(Some(&table), &mut cases).expect("tags valid");
    (cases, table)
}

fn ids(cases: &[Case]) -> BTreeSet<&str> {
    cases.iter().map(|case| case.id.as_str()).collect()
}

fn selectors(
    domains: &[&str],
    categories: &[&str],
    cases: &[&str],
    filter: Option<&str>,
) -> Selectors {
    let owned = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect();
    Selectors {
        domains: owned(domains),
        categories: owned(categories),
        cases: owned(cases),
        filter: filter.map(str::to_owned),
    }
}

fn run(sel: &Selectors) -> Result<BTreeSet<String>, SelectionError> {
    let (cases, table) = suite();
    select(cases, Some(&table), sel)
        .map(|picked| picked.iter().map(|case| case.id.clone()).collect())
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn corpus_selection_without_selectors_keeps_every_case() {
    let (cases, table) = suite();
    let total = ids(&cases).len();
    let picked = select(cases, Some(&table), &Selectors::default())
        .ok()
        .expect("all");
    assert_eq!(ids(&picked).len(), total);
}

#[test]
fn corpus_selection_domain_selects_the_cases_of_its_categories() {
    assert_eq!(
        run(&selectors(&["numerics"], &[], &[], None)).ok(),
        Some(set(&["a/add", "a/mul", "b/shift"]))
    );
}

#[test]
fn corpus_selection_category_and_case_lists_add_up() {
    assert_eq!(
        run(&selectors(&[], &["regex", "bits"], &["l/each"], None)).ok(),
        Some(set(&["r/match", "b/shift", "l/each"]))
    );
}

#[test]
fn corpus_selection_filter_unions_with_the_other_selectors() {
    assert_eq!(
        run(&selectors(&["text"], &[], &["a/add"], Some("l/"))).ok(),
        Some(set(&["r/match", "a/add", "l/each", "l/while"]))
    );
}

#[test]
fn corpus_selection_overlapping_selectors_do_not_narrow() {
    assert_eq!(
        run(&selectors(&["numerics"], &["arithmetic"], &["a/add"], None)).ok(),
        Some(set(&["a/add", "a/mul", "b/shift"]))
    );
}

#[test]
fn corpus_selection_every_domain_selects_the_whole_suite() {
    let (cases, table) = suite();
    let all: BTreeSet<String> = cases.iter().map(|case| case.id.clone()).collect();
    let every = selectors(&["numerics", "text", "control"], &[], &[], None);
    assert_eq!(run(&every).ok(), Some(all));
    drop(table);
}

#[test]
fn corpus_selection_unknown_case_domain_or_category_is_a_usage_error() {
    for sel in [
        selectors(&[], &[], &["a/add", "nope"], None),
        selectors(&["nope"], &[], &[], None),
        selectors(&[], &["nope"], &[], None),
    ] {
        assert!(matches!(run(&sel), Err(SelectionError::Usage(_))));
    }
}

#[test]
fn corpus_selection_filter_matching_nothing_alone_is_empty() {
    assert!(matches!(
        run(&selectors(&[], &[], &[], Some("zzz"))),
        Err(SelectionError::Empty(_))
    ));
}

#[test]
fn corpus_selection_domain_selectors_need_a_domain_table() {
    let cases = vec![case("a/add", None), case("a/mul", None)];
    assert!(matches!(
        select(cases, None, &selectors(&["numerics"], &[], &[], None)),
        Err(SelectionError::Usage(_))
    ));
    let cases = vec![case("a/add", None), case("a/mul", None)];
    let picked = select(cases, None, &selectors(&[], &[], &["a/mul"], Some("add")))
        .ok()
        .expect("case and filter work without a table");
    assert_eq!(ids(&picked), BTreeSet::from(["a/add", "a/mul"]));
}

#[test]
fn corpus_selection_table_rejects_a_category_in_two_domains() {
    assert!(DomainTable::from_manifest(domains(&[("x", &["c"]), ("y", &["c"])])).is_err());
    assert!(DomainTable::from_manifest(domains(&[("x", &[])])).is_err());
    assert!(matches!(
        DomainTable::from_manifest(BTreeMap::new()),
        Ok(None)
    ));
}

#[test]
fn corpus_selection_tags_are_checked_against_the_table() {
    let table = DomainTable::from_manifest(domains(&[("x", &["c"])]))
        .expect("valid")
        .expect("present");
    assert!(DomainTable::tag_cases(Some(&table), &mut [case("u", None)]).is_err());
    assert!(DomainTable::tag_cases(Some(&table), &mut [case("u", Some("other"))]).is_err());
    let mut wrong_domain = case("u", Some("c"));
    wrong_domain.domain = Some("y".to_owned());
    assert!(DomainTable::tag_cases(Some(&table), &mut [wrong_domain]).is_err());
    assert!(DomainTable::tag_cases(None, &mut [case("u", Some("c"))]).is_err());
}

/// The committed suites: running every domain selects exactly the suite, and a
/// category selector selects exactly the cases tagged with it.
#[test]
fn corpus_selection_every_domain_of_a_committed_suite_selects_the_suite() {
    // The `full` suite is a private-tree fixture (it pins every target,
    // device lanes included), so its leg compiles out of the public cut.
    fn every_domain_selects_the_suite(name: &str) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus-suites")
            .join(name);
        let loaded = load_suite(&root, "runner").unwrap_or_else(|message| panic!("{message}"));
        let table = loaded.domains.expect("committed suite defines domains");
        let all: BTreeSet<String> = loaded.cases.iter().map(|case| case.id.clone()).collect();
        assert!(!all.is_empty());
        let every = Selectors {
            domains: table.domain_names(),
            ..Selectors::default()
        };
        let picked = select(loaded.cases, Some(&table), &every)
            .ok()
            .expect("selection");
        let picked: BTreeSet<String> = picked.iter().map(|case| case.id.clone()).collect();
        assert_eq!(picked, all, "suite {name}");
    }
    every_domain_selects_the_suite("public");
    #[cfg(feature = "device")]
    every_domain_selects_the_suite("full");
}
