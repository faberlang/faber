//! Subprocess checks for the advisory `@ kernel` complexity budget
//! (`--kernel-complexity-budget`, `[check] kernel-complexity-budget`).
//!
//! Identity assertions target the rendered label
//! `WARN027:complexity_budget_exceeded` (catalog code + issue), never
//! free-form prose. Warn-count differentials prove which budget reached
//! the pass: the kernel fixtures `densa` (two `si`, score 3) and `quieta`
//! (one `si`, score 2) pin the baked kernel default (2 — the census kernel
//! subset max, a tripwire firing on nothing measured today), while `hosta`
//! (four `si`, score 5) stays under the regular default 12 and moves only
//! when the REGULAR budget's layers move. One deny test proves the single
//! WARN027 code promotes findings from BOTH budgets together.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

const WARN_LABEL: &str = "WARN027:complexity_budget_exceeded";

const MAIN_FAB: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn densa(f32 x) → f32 {
    mut f32 n ← 0.0
    if x ≻ 1.0 { n ← 1.0 }
    if x ≻ 2.0 { n ← 2.0 }
    return n
}

@ kernel
@ public
fn quieta(f32 x) → f32 {
    mut f32 n ← 0.0
    if x ≻ 1.0 { n ← 1.0 }
    return n
}

fn hosta(int seed) → int {
    mut int n ← 0
    if seed ≡ 1 { n ← 1 }
    if seed ≡ 2 { n ← 2 }
    if seed ≡ 3 { n ← 3 }
    if seed ≡ 4 { n ← 4 }
    return n
}

fn brancha(int seed) → int {
    mut int n ← 0
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

main {
    let _ h ← hosta(3)
    let _ b ← brancha(4)
    print "probe"
}
"#;

/// The manifest body without the `[check]` section; `[check]` variants
/// append their own section.
const MANIFEST_BASE: &str = r#"
[package]
name = "kernel-complexity-check"

[paths]
source = "src"
entry = "main.fab"
"#;

fn kernel_complexity_package(label: &str, check_section: Option<&str>) -> TempDir {
    let package = TempDir::new("faber-kernel-complexity", label);
    let src = package.join("src");
    fs::create_dir_all(&src).expect("create src");
    let manifest = match check_section {
        Some(section) => format!("{MANIFEST_BASE}\n{section}\n"),
        None => format!("{MANIFEST_BASE}\n"),
    };
    fs::write(package.join("faber.toml"), manifest).expect("write manifest");
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
fn kernel_complexity_baked_default_matches_the_regular_budget() {
    let package = kernel_complexity_package("default", None);
    let (_, stderr, code) = run_faber_check(&["check"], &package);
    assert_eq!(
        code,
        Some(0),
        "advisory kernel budget must exit 0:\n{stderr}"
    );
    // Baked kernel default = regular default 12 (operator 2026-10-23):
    // densa (3) and quieta (2) stay silent; only brancha (13) warns, hosta
    // (5) silent. The kernel tier alone is isolated by the tests below.
    assert_eq!(warn_count(&stderr), 1, "stderr:\n{stderr}");
}

#[test]
fn kernel_complexity_score_2_kernel_stays_silent_at_default() {
    // Tighten the kernel budget to 1: quieta (2) now warns too, proving it
    // was the kernel default 2 that kept it silent above.
    let package = kernel_complexity_package("tight", None);
    let (_, stderr, code) =
        run_faber_check(&["check", "--kernel-complexity-budget", "1"], &package);
    assert_eq!(
        code,
        Some(0),
        "advisory kernel budget must exit 0:\n{stderr}"
    );
    // Kernel budget 1: densa (3) + quieta (2) warn; brancha (13) still
    // warns under the untouched regular default 12; hosta (5) silent.
    assert_eq!(warn_count(&stderr), 3, "stderr:\n{stderr}");
}

#[test]
fn kernel_complexity_cli_override_silences_kernel_tier() {
    let package = kernel_complexity_package("loose", None);
    let (_, stderr, code) =
        run_faber_check(&["check", "--kernel-complexity-budget", "3"], &package);
    assert_eq!(code, Some(0), "check must stay green:\n{stderr}");
    // Kernel budget 3: densa (3) at budget, quieta (2) under — both silent.
    // Only brancha (13) over the regular default remains.
    assert_eq!(warn_count(&stderr), 1, "stderr:\n{stderr}");
}

#[test]
fn kernel_complexity_manifest_key_applies_without_cli_flag() {
    let package =
        kernel_complexity_package("manifest", Some("[check]\nkernel-complexity-budget = 1\n"));
    let (_, stderr, code) = run_faber_check(&["check"], &package);
    assert_eq!(
        code,
        Some(0),
        "advisory kernel budget must exit 0:\n{stderr}"
    );
    // Manifest kernel budget 1: densa (3) + quieta (2) + brancha (13).
    assert_eq!(warn_count(&stderr), 3, "stderr:\n{stderr}");
}

#[test]
fn kernel_complexity_cli_flag_beats_manifest_key() {
    let package =
        kernel_complexity_package("override", Some("[check]\nkernel-complexity-budget = 1\n"));
    let (_, stderr, code) =
        run_faber_check(&["check", "--kernel-complexity-budget", "3"], &package);
    assert_eq!(code, Some(0), "check must stay green:\n{stderr}");
    // CLI 3 wins over the manifest's 1: quieta (2) silent again — only
    // brancha (13) warns. Had the manifest key won, the count would be 3.
    assert_eq!(warn_count(&stderr), 1, "stderr:\n{stderr}");
}

#[test]
fn kernel_complexity_layering_is_independent_of_the_regular_budget() {
    let package = kernel_complexity_package("independent", None);

    // Loosening the REGULAR budget must not loosen the kernel tier.
    // The kernel budget is pinned to 2 so the two tiers differ (their baked
    // defaults are equal).
    let (_, stderr, code) = run_faber_check(
        &[
            "check",
            "--complexity-budget",
            "13",
            "--kernel-complexity-budget",
            "2",
        ],
        &package,
    );
    assert_eq!(code, Some(0), "warn-only complexity must exit 0:\n{stderr}");
    // densa (3 > kernel 2) still warns; brancha (13) is now at the regular
    // budget and goes silent — proving the regular flag moved only brancha.
    assert_eq!(warn_count(&stderr), 1, "stderr:\n{stderr}");

    // Tightening the REGULAR budget must not tighten the kernel tier.
    let (_, stderr, code) = run_faber_check(
        &[
            "check",
            "--complexity-budget",
            "4",
            "--kernel-complexity-budget",
            "2",
        ],
        &package,
    );
    assert_eq!(code, Some(0), "warn-only complexity must exit 0:\n{stderr}");
    // hosta (5 > 4) and brancha (13 > 4) warn; quieta (2 ≤ kernel 2) stays
    // silent — the regular flag did not drag the kernel budget down to 4.
    assert_eq!(warn_count(&stderr), 3, "stderr:\n{stderr}");

    // Loosening the KERNEL budget must not loosen the regular tier.
    let (_, stderr, code) =
        run_faber_check(&["check", "--kernel-complexity-budget", "13"], &package);
    assert_eq!(code, Some(0), "warn-only complexity must exit 0:\n{stderr}");
    // densa (3) and quieta (2) silent under kernel 13; brancha (13 > 12)
    // still warns — the kernel flag did not raise the regular budget.
    assert_eq!(warn_count(&stderr), 1, "stderr:\n{stderr}");
}

#[test]
fn kernel_complexity_deny_warn027_promotes_both_budgets_together() {
    // One diagnostic code covers both tiers: `--deny WARN027` promotes the
    // kernel-budget finding (densa) AND the regular-budget finding
    // (brancha) in the same run — a documented consequence of sharing
    // WARN027, asserted here so it can never silently diverge.
    let package = kernel_complexity_package("deny", None);
    // Kernel budget 2 so densa (3) produces a kernel-tier finding.
    let (_, stderr, code) = run_faber_check(
        &[
            "check",
            "--deny",
            "WARN027",
            "--kernel-complexity-budget",
            "2",
        ],
        &package,
    );
    assert_ne!(
        code,
        Some(0),
        "promoted warnings must fail the check:\n{stderr}"
    );
    assert_eq!(code, Some(1), "fatal promoted warnings exit 1:\n{stderr}");
    assert!(
        warn_count(&stderr) >= 2,
        "both budget tiers' findings present:\n{stderr}"
    );
}
