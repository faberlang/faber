//! MIR lowering of the membership operators (`∈` / `∉`, membership-glyph 3a).
//!
//! `∈` over a collection is one `Collection::Contains` call; `∉` is the same
//! call followed by the existing boolean `not`. Range containment over a
//! literal `lo‥hi` / `lo…hi` is two scalar compares joined by `and`; over a
//! stored interval value it is one `interval_contains` call. `∉` negates it.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, AnalyzedModule, Config, Session};
use crate::mir::{BufferHost, dump_analyzed_unit, run_source};

fn analyze(source: &str) -> AnalyzedModule {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript),
    );
    crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()).expect("analysis succeeds")
}

fn dump(source: &str) -> String {
    dump_analyzed_unit(analyze(source).application_view()).expect("MIR lowering succeeds")
}

/// The dump with interned-symbol ids blanked: the `non (…)` spelling interns one
/// more source word than the glyph does, which shifts every later `sym#N`.
fn shape(source: &str) -> String {
    dump(source)
        .lines()
        .map(|line| match line.find("sym#") {
            Some(at) => format!("{}sym#_", &line[..at]),
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn program(condition: &str) -> String {
    format!("incipit {{\n    sit lista<numerus> xs ← [1, 2]\n    si {condition} {{\n        nota \"hit\"\n    }}\n}}\n")
}

fn run(condition: &str) -> Vec<String> {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript),
    );
    let mut host = BufferHost::default();
    run_source(&session, "membership.fab", &program(condition), &mut host).expect("runs");
    host.stdout_lines
}

#[test]
fn member_lowers_to_one_contains_call_with_no_negation() {
    let dump = dump(&program("1 ∈ xs"));
    assert_eq!(
        dump.matches("runtime collection contains(_0, const int 1)")
            .count(),
        1,
        "{dump}"
    );
    assert!(!dump.contains(" = not "), "{dump}");
}

#[test]
fn not_member_is_contains_followed_by_not() {
    let dump = dump(&program("3 ∉ xs"));
    assert_eq!(
        dump.matches("runtime collection contains(_0, const int 3)")
            .count(),
        1,
        "{dump}"
    );
    assert_eq!(dump.matches(" = not %").count(), 1, "{dump}");
}

#[test]
fn not_member_dump_equals_the_negated_positive_form() {
    assert_eq!(shape(&program("3 ∉ xs")), shape(&program("non (3 ∈ xs)")));
}

#[test]
fn runner_takes_the_right_branch_for_both_polarities() {
    assert_eq!(run("1 ∈ xs"), ["hit"]);
    assert!(run("3 ∈ xs").is_empty());
    assert_eq!(run("3 ∉ xs"), ["hit"]);
    assert!(run("1 ∉ xs").is_empty());
}

/// Run a Latin-locale body that prints `yes` / `no` for each condition.
fn run_body(body: &str) -> Vec<String> {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript),
    );
    let mut host = BufferHost::default();
    let source = format!("incipit {{\n{body}\n}}\n");
    run_source(&session, "range-membership.fab", &source, &mut host).expect("runs");
    host.stdout_lines
}

fn verdicts(declarations: &str, conditions: &[&str]) -> Vec<String> {
    let mut body = declarations.to_owned();
    for condition in conditions {
        body.push_str(&format!("\n    si {condition} {{ nota \"yes\" }} secus {{ nota \"no\" }}"));
    }
    run_body(&body)
}

#[test]
fn literal_range_lowers_to_scalar_compares_with_no_interval_object() {
    let dump = dump("incipit {\n    sit numerus x ← 5\n    si x ∈ 0‥10 { nota \"hit\" }\n}\n");
    assert!(!dump.contains("interval_contains"), "{dump}");
    assert!(!dump.contains("interval"), "{dump}");
    assert_eq!(dump.matches(" >= ").count(), 1, "{dump}");
    assert_eq!(dump.matches(" < ").count(), 1, "{dump}");
    let inclusive = self::dump("incipit {\n    sit numerus x ← 5\n    si x ∈ 0…10 { nota \"hit\" }\n}\n");
    assert_eq!(inclusive.matches(" <= ").count(), 1, "{inclusive}");
    let outside = self::dump("incipit {\n    sit numerus x ← 5\n    si x ∉ 0‥10 { nota \"hit\" }\n}\n");
    assert_eq!(outside.matches(" = not %").count(), 1, "{outside}");
}

#[test]
fn exclusive_literal_range_includes_low_and_excludes_high() {
    let out = verdicts(
        "    sit numerus lo ← 0\n    sit numerus mid ← 3\n    sit numerus hi ← 4\n    sit numerus below ← -1",
        &[
            "lo ∈ 0‥4",
            "mid ∈ 0‥4",
            "hi ∈ 0‥4",
            "below ∈ 0‥4",
            "hi ∉ 0‥4",
            "mid ∉ 0‥4",
        ],
    );
    assert_eq!(out, ["yes", "yes", "no", "no", "yes", "no"]);
}

#[test]
fn inclusive_literal_range_includes_both_ends() {
    let out = verdicts(
        "    sit numerus lo ← 0\n    sit numerus hi ← 4\n    sit numerus over ← 5",
        &["lo ∈ 0…4", "hi ∈ 0…4", "over ∈ 0…4", "hi ∉ 0…4", "over ∉ 0…4"],
    );
    assert_eq!(out, ["yes", "yes", "no", "no", "yes"]);
}

#[test]
fn literal_ranges_accept_constants_on_the_left() {
    let out = verdicts("", &["3 ∈ 0‥4", "4 ∈ 0‥4", "4 ∈ 0…4", "5 ∉ 0…4"]);
    assert_eq!(out, ["yes", "no", "yes", "yes"]);
}

#[test]
fn sized_integer_operands_use_the_same_bounds() {
    let out = verdicts(
        "    sit i32 a ← 1\n    sit i32 b ← 3\n    sit u8 c ← 4",
        &["a ∈ 1‥3", "b ∈ 1‥3", "b ∈ 1…3", "c ∈ 1…3", "c ∉ 1…3"],
    );
    assert_eq!(out, ["yes", "no", "yes", "no", "yes"]);
}

#[test]
fn stored_interval_value_uses_the_interval_bounds_and_exclusivity() {
    let out = verdicts(
        "    sit intervallum<numerus> half ← 0‥4\n    sit intervallum<numerus> closed ← 0…4",
        &[
            "3 ∈ half",
            "4 ∈ half",
            "4 ∈ closed",
            "5 ∈ closed",
            "4 ∉ half",
            "5 ∉ closed",
            "0 ∈ half",
        ],
    );
    assert_eq!(out, ["yes", "no", "yes", "no", "yes", "yes", "yes"]);
}

#[test]
fn stored_interval_membership_is_one_interval_contains_call() {
    let dump = dump("incipit {\n    sit intervallum<numerus> r ← 0‥4\n    si 3 ∉ r { nota \"hit\" }\n}\n");
    assert_eq!(dump.matches("interval_contains").count(), 1, "{dump}");
    assert_eq!(dump.matches(" = not %").count(), 1, "{dump}");
}

#[test]
fn range_operands_evaluate_left_to_right_and_once() {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript),
    );
    let mut host = BufferHost::default();
    let source = "functio tick(numerus v) → numerus {\n    nota v\n    redde v\n}\n\
        incipit {\n    si tick(5) ∈ tick(0)‥tick(10) { nota \"hit\" }\n}\n";
    run_source(&session, "range-order.fab", source, &mut host).expect("runs");
    assert_eq!(host.stdout_lines, ["5", "0", "10", "hit"]);
}

const KERNEL: &str = r"
@ kernel
@ public
fn k(ti32[8] a, mut tensor<wrapping<i32>, [8]> o) → void {
    for from a at [i] thread lane let c {
        if c ∈ 1‥3 {
            o[i] ← 1
        } else {
            o[i] ← 0
        }
        # K1 ruling e24149ab: explicit wrapping storage permits this accumulator store.
        if c ∉ 2…3 {
            o[i] ← o[i] + 10
        }
    }
}
main {
    mut ti32[8] a ← ti32[8] {}
    a[0] ← 0
    a[1] ← 1
    a[2] ← 2
    a[3] ← 3
    a[4] ← 4
    mut tensor<wrapping<i32>, [8]> b ← tensor<wrapping<i32>, [8]> {}
    k(a, mut b)
    print b[0]
    print b[1]
    print b[2]
    print b[3]
    print b[4]
}
";

#[test]
fn range_membership_runs_inside_a_lane_loop_kernel_on_the_cpu_route() {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript)
            .with_host_kernels(),
    );
    let mut host = BufferHost::default();
    let source = format!("+++\nlocale = \"en\"\n+++\n{KERNEL}");
    run_source(&session, "range-kernel.fab", &source, &mut host).expect("the CPU route runs the kernel");
    // 0: out of 1‥3, out of 2…3 -> 0 + 10; 1: in, not in the closed -> 1 + 10;
    // 2: in both -> 1; 3: out of 1‥3 (half-open), in 2…3 -> 0; 4: 0 + 10.
    assert_eq!(host.stdout_lines, ["10", "11", "1", "0", "10"]);
}

#[test]
fn range_membership_lowers_in_device_kernel_form_without_interval_ops() {
    let unit = crate::program::analyze_prepared_program_name(
        &Session::new(
            Config::default()
                .with_latin_locale()
                .with_target(Target::HirTypeScript),
        ),
        "range-kernel-device.fab",
        &format!("+++\nlocale = \"en\"\n+++\n{KERNEL}"),
    )
    .expect("analyzes");
    let lowered = crate::mir::lower_analyzed_unit(unit.application_view(), crate::mir::MirLoweringOptions::default())
        .expect("device kernel form lowers");
    let text = radix_mir::dump::dump_program(lowered.validated.program());
    assert!(!text.contains("interval"), "{text}");
}
