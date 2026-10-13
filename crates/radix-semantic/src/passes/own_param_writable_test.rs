//! OWN-A: `own` and `copy` parameters are writable by the callee.
//!
//! The marker says everything: unmarked reads, `ref` reads, `mut` writes the
//! caller's value, `own`/`copy` write the callee's own value. Each of the four
//! mutation forms (method call, field store, whole reassignment, element
//! store) is accepted on an `own` and on a `copy` parameter and still rejected
//! on an unmarked and on a `ref` parameter. Tests assert issue slugs only.

use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};
use crate::{SemanticErrorKind, WarningKind};

fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

const CELL: &str = "class Cell {
    var int n = 0

    fn inc() → void {
        self.n ← self.n + 1
    }
}
";

/// One function per mutation form, with `marker` spelled on the parameter.
fn form(marker: &str, form: &str) -> String {
    let body = match form {
        "method" => ("list<int> xs", "xs.append(9)"),
        "class_method" => ("Cell xs", "xs.inc()"),
        "field" => ("Cell xs", "xs.n ← 5"),
        "reassign" => ("list<int> xs", "xs ← [1, 2]"),
        "element" => ("list<int> xs", "xs[0] ← 7"),
        other => panic!("unknown form {other}"),
    };
    format!(
        "{CELL}\nfn f({marker} {}) → void {{\n    {}\n}}\nmain {{}}\n",
        body.0, body.1
    )
}

const FORMS: [&str; 5] = ["method", "class_method", "field", "reassign", "element"];

fn errors(source: &str) -> Vec<String> {
    analyze_en(source)
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| issue(d).unwrap_or("<no issue>").to_owned())
        .collect()
}

fn assert_accepts(source: &str) {
    let errs = errors(source);
    assert!(
        errs.is_empty(),
        "expected acceptance, got {errs:?}\n{source}"
    );
}

fn assert_rejects_sem020_or_sem056(source: &str) {
    let result = analyze_en(source);
    assert!(
        result.diagnostics.iter().any(|d| d.is_error()
            && matches!(
                d.kind,
                SemanticErrorKind::ImmutableAssignment | SemanticErrorKind::AssignToImmutableBorrow
            )),
        "expected a mutability rejection, got {:?}\n{source}",
        result.diagnostics
    );
}

#[test]
fn own_parameter_accepts_every_mutation_form() {
    for name in FORMS {
        assert_accepts(&form("own", name));
    }
}

#[test]
fn copy_parameter_accepts_every_mutation_form() {
    for name in FORMS {
        assert_accepts(&form("copy", name));
    }
}

#[test]
fn unmarked_parameter_rejects_every_mutation_form() {
    for name in FORMS {
        assert_rejects_sem020_or_sem056(&form("", name));
    }
}

#[test]
fn ref_parameter_rejects_every_mutation_form() {
    for name in FORMS {
        assert_rejects_sem020_or_sem056(&form("ref", name));
    }
}

#[test]
fn mut_parameter_accepts_every_mutation_form() {
    for name in FORMS {
        assert_accepts(&form("mut", name));
    }
}

#[test]
fn own_parameter_mutated_then_forwarded_to_mut_is_accepted() {
    assert_accepts(
        "fn poke(mut list<int> xs) → void {
    xs.append(4)
}
fn f(own list<int> xs) → int {
    xs.append(1)
    poke(mut xs)
    return xs.length()
}
main {}",
    );
}

#[test]
fn own_tensor_parameter_may_be_rebound_whole() {
    assert_accepts(
        "fn t(tf32[3, 3] a, own tf32[3, 3] b) → void {
    b ← a + a
}
main {}",
    );
}

#[test]
fn mut_tensor_parameter_may_be_rebound_whole_on_the_host() {
    assert_accepts(
        "fn t(tf32[3, 3] a, mut tf32[3, 3] b) → void {
    b ← a + a
}
main {}",
    );
}

fn warns_unused_own(source: &str) -> bool {
    analyze_en(source).diagnostics.iter().any(|d| {
        matches!(
            d.kind,
            SemanticErrorKind::Warning(WarningKind::UnusedMoveParam)
        ) && issue(d) == Some("unused_own_parameter")
    })
}

#[test]
fn warn009_counts_mutation_as_use_of_an_own_parameter() {
    for name in FORMS {
        assert!(
            !warns_unused_own(&form("own", name)),
            "mutation form {name} must count as use"
        );
    }
}

#[test]
fn warn009_still_reports_an_untouched_own_parameter() {
    assert!(warns_unused_own(
        "fn f(own list<int> xs) → void {}\nmain {}"
    ));
}
