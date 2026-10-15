//! Subprocess + in-process checks for the `faber.toml` `[check]`
//! `complexity-budget` manifest key and its layering.
//!
//! Layering under test: CLI `--complexity-budget` beats the manifest
//! `[check] complexity-budget` key, which beats the driver's baked default
//! (12). Identity assertions target the rendered label
//! `WARN027:complexity_budget_exceeded` (catalog code + issue), never
//! free-form prose. Warn-count differentials prove which budget reached
//! the pass: `fiver` scores 5 (4 standalone `si`), `steady` scores 12,
//! `branchy` scores 13 — so the counts pin budget 4 (3 warns), the default
//! 12 (1 warn), and the override (1 warn at CLI 12 where the manifest's 4
//! alone would give 3).

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

fn fiver(int seed) → int {
    mut _ n ← 0
    if seed ≡ 1 { n ← 1 }
    if seed ≡ 2 { n ← 2 }
    if seed ≡ 3 { n ← 3 }
    if seed ≡ 4 { n ← 4 }
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

main {
    let five ← fiver(3)
    let hushed ← steady(4)
    let loud ← branchy(5)
    print "probe"
}
"#;

/// The manifest body without the `[check]` section; `[check]` variants
/// append their own section.
const MANIFEST_BASE: &str = r#"
[package]
name = "manifest-budget-check"

[paths]
source = "src"
entry = "main.fab"
"#;

fn manifest_check_package(label: &str, check_section: Option<&str>) -> TempDir {
    let package = TempDir::new("faber-manifest-check", label);
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

/// Write a bare manifest for in-process parse checks and return its path.
fn write_bare_manifest(root: &TempDir, extra_section: Option<&str>) -> std::path::PathBuf {
    let path = root.join("faber.toml");
    let body = match extra_section {
        Some(section) => format!("[package]\nname = \"manifest-check\"\n\n{section}\n"),
        None => "[package]\nname = \"manifest-check\"\n".to_owned(),
    };
    fs::write(&path, body).expect("write bare manifest");
    path
}

#[test]
fn manifest_check_budget_warns_without_cli_flag() {
    let package = manifest_check_package("warn", Some("[check]\ncomplexity-budget = 4\n"));
    let (_, stderr, code) = run_faber_check(&["check"], &package);
    assert_eq!(code, Some(0), "warn-only complexity must exit 0:\n{stderr}");
    // Manifest budget 4: fiver (5), steady (12), and branchy (13) all
    // exceed it — including the score-5 function the layering pins.
    assert_eq!(warn_count(&stderr), 3, "stderr:\n{stderr}");
}

#[test]
fn manifest_check_cli_flag_beats_manifest_key() {
    let package = manifest_check_package("override", Some("[check]\ncomplexity-budget = 4\n"));
    let (_, stderr, code) = run_faber_check(&["check", "--complexity-budget", "12"], &package);
    assert_eq!(code, Some(0), "warn-only complexity must exit 0:\n{stderr}");
    // CLI 12 wins over the manifest's 4: only branchy (13) warns. Had the
    // manifest key won, all three functions would warn (3).
    assert_eq!(warn_count(&stderr), 1, "stderr:\n{stderr}");
}

#[test]
fn manifest_check_absent_key_absent_flag_baked_default() {
    let package = manifest_check_package("default", None);
    let (_, stderr, code) = run_faber_check(&["check"], &package);
    assert_eq!(code, Some(0), "warn-only complexity must exit 0:\n{stderr}");
    // Baked default 12: branchy (13) warns, steady (12) and fiver (5) stay
    // silent.
    assert_eq!(warn_count(&stderr), 1, "stderr:\n{stderr}");
}

#[test]
fn manifest_check_without_section_parses_in_both_views() {
    let root = TempDir::new("faber-manifest-check", "bare");
    let path = write_bare_manifest(&root, None);
    let product = faber_cli::package::read_manifest(&path).expect("product view parses");
    assert!(
        product.check.complexity_budget.is_none()
            && product.check.kernel_complexity_budget.is_none(),
        "absent [check] must deserialize to the default section"
    );
    radix::program::manifest::read_manifest(&path)
        .expect("compiler projection parses a manifest without [check]");
}

#[test]
fn manifest_check_projection_tolerates_check_table() {
    // Read-only tolerance proof: the radix-program compiler projection does
    // not declare `[check]` and must keep parsing a manifest that carries
    // it as an unknown product table (ownership-freeze, RP0 — no edit).
    let root = TempDir::new("faber-manifest-check", "tolerated");
    let path = write_bare_manifest(&root, Some("[check]\ncomplexity-budget = 4\n"));
    radix::program::manifest::read_manifest(&path)
        .expect("compiler projection tolerates [check] as an unknown table");
}

#[test]
fn manifest_check_unknown_key_fails_closed() {
    let bad_section = "[check]\nbogus-budget = 1\n";
    // Product view rejects the unknown key (deny_unknown_fields).
    let root = TempDir::new("faber-manifest-check", "unknown-key");
    let path = write_bare_manifest(&root, Some(bad_section));
    assert!(
        faber_cli::package::read_manifest(&path).is_err(),
        "product view must reject unknown keys inside [check]"
    );
    // The check command fails closed on the strict product-view manifest
    // validation seam, naming the offending key.
    let package = manifest_check_package("unknown-key", Some(bad_section));
    let (_, stderr, code) = run_faber_check(&["check"], &package);
    assert_ne!(
        code,
        Some(0),
        "unknown [check] key must fail closed:\n{stderr}"
    );
    assert_eq!(
        code,
        Some(1),
        "fatal manifest diagnostic exits 1:\n{stderr}"
    );
    assert!(
        stderr.contains("bogus-budget"),
        "the offending key must be named:\n{stderr}"
    );
}
