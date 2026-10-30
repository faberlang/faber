//! Installed-binary proofs for the single-file read lanes:
//! `faber check`, `lex` and `parse` accept a Latin file declaring
//! `+++ locale = "la" +++` with no workspace `locale/` tree beside the
//! binary.
//!
//! The input lives outside the checkout and each child runs with that
//! directory as its working directory, so nothing in the workspace beside
//! the binary serves the read. These subprocess tests drive the whole
//! command lane (`commands::run` → the lane handlers → the `_with_config`
//! services); the root-cleared condition on the exact config the lanes
//! install is pinned in-process by `src/commands/locale_lanes_test.rs`.

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

const LA_SOURCE: &str = "+++\nlocale = \"la\"\n+++\n@ publica\nfunctio salve() { }\n";

fn run_faber(dir: &TempDir, args: &[&str]) -> (String, String, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn faber");

    let mut stdout = String::new();
    child
        .stdout
        .take()
        .expect("stdout")
        .read_to_string(&mut stdout)
        .expect("read stdout");

    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    let status = child.wait().expect("wait");

    (stdout, stderr, status.success())
}

/// A Latin frontmatter source in its own directory outside the checkout.
fn latin_file(label: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new("faber-locale-lanes", label);
    let file = dir.join("main.fab");
    fs::write(&file, LA_SOURCE).expect("write Latin source");
    (dir, file)
}

#[test]
fn check_lane_reads_declared_locale_on_installed_binary() {
    let (dir, file) = latin_file("check");
    let (stdout, stderr, ok) = run_faber(&dir, &["check", file.to_str().expect("file path")]);

    assert!(
        ok,
        "faber check must read the Latin frontmatter file: {stderr}"
    );
    assert!(stdout.is_empty(), "check should not write stdout: {stdout}");
    assert!(
        stderr.contains("ok:"),
        "expected the check success marker on stderr:\n{stderr}"
    );
}

#[test]
fn lex_lane_reads_declared_locale_on_installed_binary() {
    let (dir, file) = latin_file("lex");
    let (stdout, stderr, ok) = run_faber(&dir, &["lex", file.to_str().expect("file path")]);

    assert!(
        ok,
        "faber lex must read the Latin frontmatter file: {stderr}"
    );
    assert!(
        stdout.contains("\"keyword\": \"functio\""),
        "the Latin `functio` must lex as a keyword:\n{stdout}"
    );
}

#[test]
fn parse_lane_reads_declared_locale_on_installed_binary() {
    let (dir, file) = latin_file("parse");
    let (stdout, stderr, ok) = run_faber(&dir, &["parse", file.to_str().expect("file path")]);

    assert!(
        ok,
        "faber parse must read the Latin frontmatter file: {stderr}"
    );
    assert!(
        stdout.contains("\"success\": true"),
        "the Latin source must parse cleanly:\n{stdout}"
    );
    assert!(
        stdout.contains("\"kind\": \"Publica\""),
        "the AST must carry the Latin `Publica` annotation kind:\n{stdout}"
    );
}
