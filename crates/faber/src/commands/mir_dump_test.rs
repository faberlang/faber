//! `faber mir` reads a file in the product reader locale, as `check`, `build`
//! and `run` do (it used to read plain source as Latin and fail to parse).

use std::process::Command;

fn run_mir(args: &[&str]) -> (bool, String, String) {
    let binary = super::format_test::faber_binary()
        .canonicalize()
        .expect("locate faber binary");
    let output = Command::new(binary)
        .arg("mir")
        .args(args)
        .output()
        .expect("run faber mir");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn mir_reads_plain_english_source_without_frontmatter() {
    let root = tempfile::tempdir().expect("create fixture root");
    let file = root.path().join("plain.fab");
    std::fs::write(&file, "fn f() → int { return 3 }\n").expect("write source");
    let path = file.to_str().expect("utf-8 path");
    let (ok, stdout, stderr) = run_mir(&[path]);
    assert!(ok, "plain en source must parse:\n{stderr}");
    assert!(
        stdout.contains("function f0"),
        "MIR text on stdout:\n{stdout}"
    );
}

#[test]
fn mir_keeps_a_files_own_frontmatter_locale() {
    let root = tempfile::tempdir().expect("create fixture root");
    let file = root.path().join("latin.fab");
    std::fs::write(
        &file,
        "+++\nlocale = \"la\"\n+++\nfunctio f() → numerus { redde 3 }\n",
    )
    .expect("write source");
    let path = file.to_str().expect("utf-8 path");
    let (ok, stdout, stderr) = run_mir(&[path]);
    assert!(ok, "a la file with frontmatter must parse:\n{stderr}");
    assert!(
        stdout.contains("function f0"),
        "MIR text on stdout:\n{stdout}"
    );
}

#[test]
fn mir_reads_plain_english_source_on_stdin() {
    use std::io::Write;
    let binary = super::format_test::faber_binary()
        .canonicalize()
        .expect("locate faber binary");
    let mut child = Command::new(binary)
        .args(["mir", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn faber mir -");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"fn f() \xe2\x86\x92 int { return 3 }\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait for faber mir");
    assert!(
        output.status.success(),
        "stdin en source must parse:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
