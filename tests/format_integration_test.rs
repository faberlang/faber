//! Subprocess checks for `faber format --stdout` (requires built `faber` binary).

use forma::test_gate::{assert_author_idempotent, assert_author_reparses};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn exempla(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../radix/crates/exempla/corpus")
        .join(path)
}

fn normalize_trailing_newline(text: &str) -> String {
    let trimmed = text.trim_end_matches('\n');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

fn body_for_parse(source: &str) -> &str {
    radix::driver::split_frontmatter(source)
        .expect("split formatted source")
        .body
}

fn run_faber_format_stdout(path: &Path) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["format", "--stdout", path.to_str().expect("utf8 path")])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn faber format");
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
    assert!(status.success(), "faber format --stdout failed: {stderr}");
    normalize_trailing_newline(&stdout)
}

#[test]
fn format_cmd_stdout_reparses_salve_munde() {
    let path = exempla("incipit/salve-munde.fab");
    let formatted = run_faber_format_stdout(&path);
    assert!(
        formatted.starts_with("+++"),
        "stdout must include frontmatter"
    );
    assert_author_reparses(body_for_parse(&formatted), "salve-munde CLI --stdout")
        .expect("reparse");
}

#[test]
fn format_cmd_stdout_is_idempotent_for_salve_munde() {
    let path = exempla("incipit/salve-munde.fab");
    let first = run_faber_format_stdout(&path);
    let tmp = std::env::temp_dir().join("faber-format-idempotent-salve.fab");
    fs::write(&tmp, &first).expect("write temp fab");
    let second = run_faber_format_stdout(&tmp);
    let _ = fs::remove_file(&tmp);
    assert_eq!(
        first, second,
        "format(format(x)) must equal format(x) via CLI"
    );
}

#[test]
fn format_cmd_stdout_reparses_cura() {
    let path = exempla("cura/cura.fab");
    let formatted = run_faber_format_stdout(&path);
    assert!(
        formatted.contains("cura \"arena\"") || formatted.contains("cura \"page\""),
        "cura routes must stay quoted:\n{formatted}"
    );
    assert_author_reparses(body_for_parse(&formatted), "cura CLI --stdout").expect("reparse");
}

/// SC-3/verification step 5: CLI `format --stdout` on comment fixture re-parses.
#[test]
fn format_cmd_comment_fixture_cli_reparses() {
    let fixture = std::env::temp_dir().join("faber-format-comment-fixture.fab");
    fs::write(&fixture, "# lead comment\n\nincipit {\n  nota \"ok\"\n}\n")
        .expect("write comment fixture");

    let formatted = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);

    assert!(
        formatted.contains("# lead comment"),
        "CLI must preserve leading comment:\n{formatted}"
    );
    assert_author_reparses(&formatted, "comment CLI --stdout").expect("reparse");
    assert_author_idempotent(
        "comment.fab",
        "# lead comment\n\nincipit {\n  nota \"ok\"\n}\n",
    )
    .expect("idempotent");
}
