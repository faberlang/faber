//! `faber format` author pipeline: compile_author, normalization, re-parse.

use super::format::{formatted_source_for_write, normalize_trailing_newline, source_for_compare};
use radix::driver::{Config, Session};
use radix::forma::test_gate::{
    assert_author_idempotent, assert_author_reparses, author_format_once,
};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn exempla(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/corpus")
        .join(path)
}

fn author_format_pipeline(name: &str, source: &str) -> String {
    let session = Session::new(Config::default());
    let result = radix::forma::compile_author(&session, name, source);
    assert!(
        result.success(),
        "author format failed for {name}: {:?}",
        result.diagnostics
    );
    normalize_trailing_newline(&result.output.expect("output").code)
}

#[test]
fn format_author_output_reparses_without_errors() {
    let path = exempla("incipit/salve-munde.fab");
    let source = fs::read_to_string(&path).expect("read salve-munde.fab");
    let formatted = author_format_pipeline(&path.display().to_string(), &source);
    assert_author_reparses(&formatted, "salve-munde pipeline").expect("reparse");
}

#[test]
fn format_author_path_preserves_salve_munde_comments() {
    let path = exempla("incipit/salve-munde.fab");
    let source = fs::read_to_string(&path).expect("read salve-munde.fab");
    let formatted = author_format_pipeline(&path.display().to_string(), &source);
    assert!(
        formatted.contains("# incipit — canonical hello-world entry point"),
        "author format should preserve leading comments"
    );
    assert!(formatted.contains("nota \"Salve, Munde!\""));
}

#[test]
fn format_author_pipeline_reparses_cura_fixture() {
    let path = exempla("cura/cura.fab");
    let source = fs::read_to_string(&path).expect("read cura.fab");
    let formatted = author_format_pipeline(&path.display().to_string(), &source);
    assert!(
        formatted.contains("cura \"arena\"") || formatted.contains("cura \"page\""),
        "cura exempla should keep quoted arena/page routes:\n{formatted}"
    );
    assert_author_reparses(&formatted, "cura pipeline").expect("reparse");
}

#[test]
fn format_author_pipeline_preserves_and_reparses_comment_fixture() {
    let source = "# lead comment\n\nincipit {\n  nota \"ok\"\n}\n";
    let formatted = author_format_pipeline("comment.fab", source);
    assert!(formatted.contains("# lead comment"));
    assert_author_reparses(&formatted, "comment pipeline").expect("reparse");
    assert_author_idempotent("comment.fab", source).expect("idempotent");
}

#[test]
fn format_test_gate_matches_compile_author_pipeline_for_salve() {
    let path = exempla("incipit/salve-munde.fab");
    let source = fs::read_to_string(&path).expect("read");
    let name = path.display().to_string();
    let via_gate = author_format_once(&name, &source).expect("gate");
    let via_pipeline = author_format_pipeline(&name, &source);
    assert_eq!(
        via_gate, via_pipeline,
        "test_gate and CLI pipeline must agree"
    );
}

fn faber_binary() -> PathBuf {
    std::env::var("CARGO_BIN_EXE_faber")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/faber"))
}

fn run_faber_format_stdout(path: &Path) -> String {
    run_faber_format_stdout_with_args(&["format", "--stdout", path.to_str().expect("utf8 path")])
}

fn run_faber_format_stdout_with_args(args: &[&str]) -> String {
    let mut child = Command::new(faber_binary())
        .args(args)
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
    assert!(
        child.wait().expect("wait").success(),
        "faber format --stdout failed: {stderr}"
    );
    normalize_trailing_newline(&stdout)
}

/// Verification plan step 5: CLI `format --stdout` on comment fixture re-parses.
#[test]
fn format_cli_comment_fixture_reparses() {
    let fixture = std::env::temp_dir().join("faber-format-comment-unit.fab");
    fs::write(&fixture, "# lead comment\n\nincipit {\n  nota \"ok\"\n}\n")
        .expect("write comment fixture");

    let formatted = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);

    assert!(
        formatted.contains("# lead comment"),
        "CLI must preserve leading comment:\n{formatted}"
    );
    assert_author_reparses(&formatted, "comment CLI --stdout").expect("reparse");
}

#[test]
fn format_canonical_reader_locale_thai_matches_latin_twin() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/reader-locale/th-TH");
    let thai = root.join("src/main.fab");
    let latin = root.join("twins/main.la.fab");

    let thai_output = run_faber_format_stdout_with_args(&[
        "format",
        "--canonical",
        "--reader-locale",
        "th-TH",
        "--stdout",
        thai.to_str().expect("utf8 thai path"),
    ]);
    let latin_output = run_faber_format_stdout_with_args(&[
        "format",
        "--canonical",
        "--stdout",
        latin.to_str().expect("utf8 latin path"),
    ]);

    assert_eq!(thai_output, latin_output);
    assert!(thai_output.contains("functio ทักทาย(textus name) → textus"));
    assert!(thai_output.contains("fixum textus greeting ← scriptum(\"สวัสดี, §!\", name)"));
    assert!(thai_output.contains("functio ผ่าน(numerus score) → bivalens"));
    assert!(thai_output.contains("score ≥ 80 sic verum secus score ≥ 50 sic verum secus falsum"));
    assert!(thai_output.contains("functio นับผ่าน(lista<numerus> scores) → numerus"));
    assert!(thai_output.contains("itera ex scores fixum score"));
    assert!(thai_output.contains("perge"));
    assert!(thai_output.contains("rumpe"));
    assert!(thai_output.contains("functio นับถอยหลัง(numerus start) → numerus"));
    assert!(thai_output.contains("dum current > 0"));
    assert!(thai_output.contains("incipit {"));
    assert!(thai_output.contains("fixum numerus score ← 82"));
    assert!(thai_output.contains("fixum lista<numerus> scores ← [-1, 82, 41, 60]"));
    assert!(thai_output.contains("nota ผ่าน(score)"));
    assert!(thai_output.contains("nota นับผ่าน(scores)"));
    assert!(!thai_output.contains("ฟังก์ชัน"));
    assert!(!thai_output.contains("คงที่"));
    assert!(!thai_output.contains("แปร"));
    assert!(!thai_output.contains("ข้อความ"));
    assert!(!thai_output.contains("รายการ"));
    assert!(!thai_output.contains("จำนวน"));
    assert!(!thai_output.contains("ตรรกะ"));
    assert!(!thai_output.contains("ถ้า"));
    assert!(!thai_output.contains("มิฉะนั้น"));
    assert!(!thai_output.contains("วน"));
    assert!(!thai_output.contains("จาก"));
    assert!(!thai_output.contains("ข้าม"));
    assert!(!thai_output.contains("หยุด"));
    assert!(!thai_output.contains("ขณะ"));
    assert!(!thai_output.contains("เริ่ม"));
    assert!(!thai_output.contains("แสดง"));
    assert!(!thai_output.contains("คืน"));
    assert!(!thai_output.contains("จริง"));
    assert!(!thai_output.contains("เท็จ"));
}

#[test]
fn format_reader_locale_la_without_canonical_matches_canonical_latin() {
    let path = exempla("incipit/salve-munde.fab");

    let locale_output = run_faber_format_stdout_with_args(&[
        "format",
        "--reader-locale",
        "la",
        "--stdout",
        path.to_str().expect("utf8 path"),
    ]);
    let canonical_output = run_faber_format_stdout_with_args(&[
        "format",
        "--canonical",
        "--reader-locale",
        "la",
        "--stdout",
        path.to_str().expect("utf8 path"),
    ]);

    assert_eq!(locale_output, canonical_output);
    assert!(locale_output.contains("incipit {"));
    assert!(locale_output.contains("nota \"Salve, Munde!\""));
}

#[test]
fn format_reader_locale_without_canonical_errors() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/reader-locale/th-TH");
    let thai = root.join("src/main.fab");

    let output = Command::new(faber_binary())
        .args([
            "format",
            "--reader-locale",
            "th-TH",
            "--stdout",
            thai.to_str().expect("utf8 thai path"),
        ])
        .output()
        .expect("run faber format");

    assert!(
        !output.status.success(),
        "reader-locale author formatting should fail until localized output exists"
    );
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(
        stderr.contains("format --reader-locale currently requires --canonical"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn format_canonical_check_passes_on_braced_futura_exempla() {
    let path = exempla("annotation-sugar/futura-braced.fab");
    let mut child = Command::new(faber_binary())
        .args([
            "format",
            "--check",
            "--canonical",
            path.to_str().expect("utf8 path"),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn faber format --check --canonical");
    let status = child.wait().expect("wait");
    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    assert!(
        status.success(),
        "canonical check must pass on braced futura exempla: {stderr}"
    );
}

#[test]
fn format_check_compare_keeps_frontmatter_in_baseline() {
    let path = exempla("incipit/salve-munde.fab");
    let raw = fs::read_to_string(&path).expect("read");
    let compare = source_for_compare(&path, &raw);
    assert!(
        compare.starts_with("+++"),
        "compare baseline must be the full source, including frontmatter"
    );
}

#[test]
fn format_write_reprepends_exact_frontmatter_slice() {
    let path = exempla("incipit/salve-munde.fab");
    let raw = fs::read_to_string(&path).expect("read");
    let formatted_body = author_format_pipeline(&path.display().to_string(), &raw);
    let formatted =
        formatted_source_for_write(&path, &raw, &formatted_body).expect("format source");
    let split = radix::driver::split_frontmatter(&raw).expect("split frontmatter");
    let body_start = split.body_byte_offset as usize;

    assert_eq!(
        &formatted[..body_start],
        &raw[..body_start],
        "format write must preserve the exact frontmatter prefix"
    );
    assert!(formatted.starts_with("+++"));
    assert!(formatted.contains("term = \"incipit\""));
    assert_author_reparses(&formatted[body_start..], "formatted salve-munde body")
        .expect("reparse");
}
