//! Kernel safety tiers through the `faber` binary (operator ruling
//! 2026-10-28): `faber format` round-trips the tier word at the source locale,
//! a misused tier refuses by name, and `faber run --safe` / `faber build
//! --safe` accept the safe switch.
//!
//! Identity assertions target the rendered `CODE:issue` label, never prose.

use radix::module::driver::faber_format::test_gate::assert_canonical_reparses_with;
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

const EN: &str = "+++\nlocale = \"en\"\n+++\n@ kernel\n@ public\nfn plain(f32 x) → f32 {\n    return x + 1.0\n}\n\n@ kernel relaxed\n@ public\nfn loose(f32 x) → f32 {\n    return x + 1.0\n}\n\n@ kernel unsafe\n@ public\nfn raw(f32 x) → f32 {\n    return x + 1.0\n}\n\nmain {\n    print 7\n}\n";

const LA: &str = "+++\nlocale = \"la\"\n+++\n@ nucleum\n@ publica\nfunctio plain(f32 x) → f32 {\n    redde x + 1.0\n}\n\n@ nucleum laxus\n@ publica\nfunctio loose(f32 x) → f32 {\n    redde x + 1.0\n}\n\n@ nucleum intutus\n@ publica\nfunctio raw(f32 x) → f32 {\n    redde x + 1.0\n}\n\nincipit {\n    nota 7\n}\n";

fn pack(id: &str) -> radix::module::locale::LocalePack {
    radix::module::locale::LocalePack::from_toml_path(
        radix::module::locale::workspace_locale_pack_path(id),
    )
    .expect("load locale pack")
}

fn faber(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("spawn faber")
}

fn format_stdout(path: &Path) -> String {
    let output = faber(&["format", "--stdout", path.to_str().expect("utf8 path")]);
    assert!(
        output.status.success(),
        "faber format failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf8 stdout")
}

fn body(formatted: &str) -> &str {
    radix::module::driver::split_frontmatter(formatted)
        .expect("split formatted source")
        .body
}

fn annotation_lines(formatted: &str) -> Vec<&str> {
    formatted
        .lines()
        .filter(|line| {
            line.starts_with("@ ")
                && !line.starts_with("@ public")
                && !line.starts_with("@ publica")
        })
        .collect()
}

#[test]
fn format_keeps_the_english_tier_words() {
    let dir = TempDir::new("faber-kernel-tier", "format-en");
    let file = dir.join("tiers.fab");
    fs::write(&file, EN).expect("write fixture");
    let formatted = format_stdout(&file);
    assert_eq!(
        annotation_lines(&formatted),
        ["@ kernel", "@ kernel relaxed", "@ kernel unsafe"],
        "{formatted}"
    );
    assert_canonical_reparses_with(body(&formatted), "en tiers", Some(&pack("en")))
        .expect("reparse");
}

#[test]
fn format_keeps_the_latin_tier_words() {
    let dir = TempDir::new("faber-kernel-tier", "format-la");
    let file = dir.join("tiers.fab");
    fs::write(&file, LA).expect("write fixture");
    let formatted = format_stdout(&file);
    assert_eq!(
        annotation_lines(&formatted),
        ["@ nucleum", "@ nucleum laxus", "@ nucleum intutus"],
        "{formatted}"
    );
    assert_canonical_reparses_with(body(&formatted), "la tiers", Some(&pack("la")))
        .expect("reparse");
}

#[test]
fn format_is_idempotent_over_the_tier_words() {
    let dir = TempDir::new("faber-kernel-tier", "format-twice");
    let file = dir.join("tiers.fab");
    fs::write(&file, EN).expect("write fixture");
    let first = format_stdout(&file);
    fs::write(&file, &first).expect("write formatted");
    assert_eq!(format_stdout(&file), first);
}

#[test]
fn a_bad_tier_refuses_by_name() {
    let dir = TempDir::new("faber-kernel-tier", "bad");
    for (label, annotation, expected) in [
        (
            "duplicate",
            "@ kernel relaxed unsafe",
            "PARSE018:duplicate_nucleum_tier",
        ),
        ("unknown", "@ kernel fast", "PARSE018:unknown_nucleum_tier"),
        (
            "fragment",
            "@ kernel fragment unsafe",
            "PARSE018:nucleum_tier_on_fragment",
        ),
    ] {
        let file = dir.join(format!("{label}.fab"));
        fs::write(
            &file,
            format!(
                "+++\nlocale = \"en\"\n+++\n{annotation}\nfn k(f32 x) → f32 {{\n    return x\n}}\n"
            ),
        )
        .expect("write fixture");
        let output = faber(&["check", file.to_str().expect("utf8 path")]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(65), "{label}: {stderr}");
        assert!(stderr.contains(expected), "{label}: {stderr}");
    }
}

#[test]
fn a_tier_on_a_non_kernel_declaration_refuses_by_name() {
    let dir = TempDir::new("faber-kernel-tier", "non-kernel");
    let file = dir.join("class.fab");
    fs::write(
        &file,
        "+++\nlocale = \"en\"\n+++\n@ kernel relaxed\nclass P {\n    let int a\n}\n",
    )
    .expect("write fixture");
    let output = faber(&["check", file.to_str().expect("utf8 path")]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(65), "{stderr}");
    // The subprocess prints only the rendered `CODE:issue` identity label.
    let expected = "PARSE018:nucleum_tier_non_kernel";
    assert!(stderr.contains(expected), "{stderr}");
}

#[test]
fn run_accepts_the_safe_switch_and_prints_the_same_answer() {
    let dir = TempDir::new("faber-kernel-tier", "run-safe");
    let file = dir.join("tiers.fab");
    fs::write(&file, EN).expect("write fixture");
    let path = file.to_str().expect("utf8 path");
    let plain = faber(&["run", "--device", "reference", path]);
    let safe = faber(&["run", "--device", "reference", "--safe", path]);
    assert_eq!(
        plain.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&plain.stderr)
    );
    assert_eq!(
        safe.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&safe.stderr)
    );
    assert_eq!(plain.stdout, safe.stdout);
    assert_eq!(String::from_utf8_lossy(&safe.stdout).trim(), "7");
}

#[test]
fn build_and_run_list_the_safe_switch() {
    for command in ["build", "run"] {
        let output = faber(&[command, "--help"]);
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(help.contains("--safe"), "{command}: {help}");
    }
}
