//! Subprocess checks for `faber check --complexity-budget`.
//!
//! Identity assertions target the rendered label
//! `WARN027:complexity_budget_exceeded` (catalog code + issue), never
//! free-form prose. Warn-count differentials prove the budget value reached
//! the pass: `branchy` scores 13 and `steady` scores 12 (base 1 plus one
//! decision point per standalone `si`), so the pair pins the baked default
//! (12) and both override directions.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

const WARN_LABEL: &str = "WARN027:complexity_budget_exceeded";

/// `branchy` = 12 standalone `si` branches (score 13, over the baked default
/// 12); `steady` = 11 branches (score 12, exactly at it). Identifiers avoid
/// pack keyword near-misses so LOCALE002 suggestions stay out of the counts.
const MAIN_FAB: &str = r#"+++
locale = "en"
+++

fn branchy(int seed) → int {
    mut _ n ← 0
    if seed ≡ 1 { n ← 1 }
    if seed ≡ 2 { n ← 2 }
    if seed ≡ 3 { n ← 3 }
    if seed ≡ 4 { n ← 4 }
    if seed ≡ 5 { n ← 5 }
    if seed ≡ 6 { n ← 6 }
    if seed ≡ 7 { n ← 7 }
    if seed ≡ 8 { n ← 8 }
    if seed ≡ 9 { n ← 9 }
    if seed ≡ 10 { n ← 10 }
    if seed ≡ 11 { n ← 11 }
    if seed ≡ 12 { n ← 12 }
    return n
}

fn steady(int seed) → int {
    mut _ n ← 0
    if seed ≡ 1 { n ← 1 }
    if seed ≡ 2 { n ← 2 }
    if seed ≡ 3 { n ← 3 }
    if seed ≡ 4 { n ← 4 }
    if seed ≡ 5 { n ← 5 }
    if seed ≡ 6 { n ← 6 }
    if seed ≡ 7 { n ← 7 }
    if seed ≡ 8 { n ← 8 }
    if seed ≡ 9 { n ← 9 }
    if seed ≡ 10 { n ← 10 }
    if seed ≡ 11 { n ← 11 }
    return n
}

main {
    let _ loud ← branchy(3)
    let _ hushed ← steady(4)
    print "probe"
}
"#;

fn budget_fixture_package(label: &str) -> TempDir {
    let package = TempDir::new("faber-complexity-budget", label);
    let src = package.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(
        package.join("faber.toml"),
        r#"
[package]
name = "complexity-budget-check"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    fs::write(src.join("main.fab"), MAIN_FAB).expect("write entry");
    package
}

fn run_faber_check(args: &[&str], package: &Path) -> (String, String, Option<i32>) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_faber"));
    command.args(args).arg(package);
    let output = command
        .stdin(Stdio::null())
        .output()
        .expect("spawn faber check");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code(),
    )
}

fn warn_count(stderr: &str) -> usize {
    stderr.matches(WARN_LABEL).count()
}

#[test]
fn check_complexity_budget_baked_default_applies_without_flag() {
    let package = budget_fixture_package("default");
    let (_, stderr, code) = run_faber_check(&["check"], &package);
    assert_eq!(code, Some(0), "warn-only complexity must exit 0:\n{stderr}");
    // Baked default 12: branchy (13) warns, steady (12) stays silent.
    assert_eq!(warn_count(&stderr), 1, "stderr:\n{stderr}");
}

#[test]
fn check_complexity_budget_warns_at_budget_n() {
    let package = budget_fixture_package("warn");
    let (_, stderr, code) = run_faber_check(&["check", "--complexity-budget", "5"], &package);
    assert_eq!(code, Some(0), "warn-only complexity must exit 0:\n{stderr}");
    // Budget 5: both branchy (13) and steady (12) exceed it.
    assert_eq!(warn_count(&stderr), 2, "stderr:\n{stderr}");
}

#[test]
fn check_complexity_budget_silent_at_higher_budget() {
    let package = budget_fixture_package("silent");
    let (_, stderr, code) = run_faber_check(&["check", "--complexity-budget", "13"], &package);
    assert_eq!(code, Some(0), "check must stay green:\n{stderr}");
    // Budget 13: branchy (13) is at the budget, steady (12) under it.
    assert_eq!(warn_count(&stderr), 0, "stderr:\n{stderr}");
}

#[test]
fn check_complexity_budget_invalid_values_fail_with_clap_standard_error() {
    let package = budget_fixture_package("invalid");
    for invalid in ["0", "abc"] {
        let (_, stderr, code) =
            run_faber_check(&["check", "--complexity-budget", invalid], &package);
        assert_ne!(code, Some(0), "budget {invalid} must be rejected");
        assert_eq!(
            code,
            Some(2),
            "clap standard error exit for {invalid}:\n{stderr}"
        );
        assert!(
            stderr.contains("invalid value") && stderr.contains("--complexity-budget"),
            "clap standard error text expected for {invalid}:\n{stderr}"
        );
    }
}
