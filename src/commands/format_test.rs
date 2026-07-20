//! `faber format` author pipeline: `compile_author`, normalization, re-parse.

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
    std::env::var("CARGO_BIN_EXE_faber").map_or_else(
        |_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/faber"),
        PathBuf::from,
    )
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
fn format_canonical_reader_locale_thai_localizes_surface() {
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

    // Phase 2: --reader-locale drives the emitter surface, so the canonical
    // re-emit localizes reader-locale keywords and types into Thai and no longer
    // matches the Latin twin.
    assert_ne!(
        thai_output,
        run_faber_format_stdout_with_args(&[
            "format",
            "--canonical",
            "--stdout",
            latin.to_str().expect("utf8 latin path"),
        ]),
        "localized Thai emit must differ from the Latin twin"
    );

    assert!(thai_output.contains("ฟังก์ชัน ทักทาย(ข้อความ name) → ข้อความ"));
    assert!(thai_output.contains("คงที่ ข้อความ greeting ← scriptum"));
    assert!(thai_output.contains("คืน greeting"));
    assert!(thai_output.contains("ฟังก์ชัน ผ่าน(จำนวน score) → ตรรกะ"));
    assert!(thai_output.contains("sic จริง มิฉะนั้น score ≥ 50 sic จริง มิฉะนั้น เท็จ"));
    assert!(thai_output.contains("ฟังก์ชัน นับผ่าน(รายการ<จำนวน> scores) → จำนวน"));
    assert!(thai_output.contains("แปร จำนวน total ← 0"));
    assert!(thai_output.contains("วน จาก scores คงที่ score"));
    assert!(thai_output.contains("ถ้า score < 0 {"));
    assert!(thai_output.contains("ข้าม"));
    assert!(thai_output.contains("หยุด"));
    assert!(thai_output.contains("ฟังก์ชัน นับถอยหลัง(จำนวน start) → จำนวน"));
    assert!(thai_output.contains("ขณะ current > 0"));
    assert!(thai_output.contains("เริ่ม {"));
    assert!(thai_output.contains("คงที่ จำนวน score ← 82"));
    assert!(thai_output.contains("คงที่ รายการ<จำนวน> scores ← [-1, 82, 41, 60]"));
    assert!(thai_output.contains("แสดง ผ่าน(score)"));
    assert!(thai_output.contains("แสดง นับผ่าน(scores)"));
    // The Latin keyword surface must not survive localized re-emit. `scriptum`
    // (a builtin name) and `sic` (not a localized keyword token) stay Latin.
    assert!(!thai_output.contains("functio"));
    assert!(!thai_output.contains("fixum"));
    assert!(!thai_output.contains("textus"));
    assert!(!thai_output.contains("numerus"));
    assert!(!thai_output.contains("bivalens"));
    assert!(!thai_output.contains("lista<"));
    assert!(!thai_output.contains("itera"));
    assert!(!thai_output.contains("incipit"));
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
fn format_reader_locale_without_canonical_localizes() {
    // Phase 2 removed the "--reader-locale requires --canonical" gate. A bare
    // --reader-locale=<X> now selects the canonical re-emit path with the
    // localized surface (Latin default when --reader-locale is absent).
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
        output.status.success(),
        "reader-locale formatting must succeed without --canonical: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(
        stdout.contains("ฟังก์ชัน"),
        "bare --reader-locale=th-TH must emit the Thai surface: {stdout}"
    );
    assert!(
        !stdout.contains("functio"),
        "bare --reader-locale=th-TH must not emit the Latin keyword: {stdout}"
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

// ── normalize_trailing_newline ────────────────────────────────────────────

#[test]
fn normalize_trailing_newline_preserves_text_with_newline() {
    assert_eq!(normalize_trailing_newline("hello\n"), "hello\n");
}

#[test]
fn normalize_trailing_newline_removes_trailing_whitespace() {
    assert_eq!(normalize_trailing_newline("hello\n\n\n"), "hello\n");
}

#[test]
fn normalize_trailing_newline_adds_newline_to_text_without() {
    assert_eq!(normalize_trailing_newline("hello"), "hello\n");
}

#[test]
fn normalize_trailing_newline_handles_empty_string() {
    assert_eq!(normalize_trailing_newline(""), "");
}

#[test]
fn normalize_trailing_newline_handles_only_newlines() {
    assert_eq!(normalize_trailing_newline("\n\n\n"), "");
}

#[test]
fn normalize_trailing_newline_preserves_internal_newlines() {
    assert_eq!(
        normalize_trailing_newline("line1\nline2\n"),
        "line1\nline2\n"
    );
}

#[test]
fn normalize_trailing_newline_preserves_trailing_whitespace_lines() {
    assert_eq!(normalize_trailing_newline("text\n  \n"), "text\n  \n");
}

// ── formatted_source_for_write ─────────────────────────────────────────────

#[test]
fn formatted_source_for_write_without_frontmatter_returns_body_only() {
    let path = Path::new("/tmp/test.fab");
    let raw = "incipit {\n  nota \"ok\"\n}\n";
    let formatted_body = "incipit {\n    nota \"ok\"\n}\n";
    let result = formatted_source_for_write(path, raw, formatted_body).expect("format source");
    assert_eq!(result, formatted_body);
}

#[test]
fn formatted_source_for_write_preserves_frontmatter_prefix() {
    let path = Path::new("/tmp/test.fab");
    let raw = "+++\nterm = \"test\"\n+++\nincipit {\n  nota \"ok\"\n}\n";
    let formatted_body = "incipit {\n    nota \"ok\"\n}\n";
    let result = formatted_source_for_write(path, raw, formatted_body).expect("format source");
    assert!(result.starts_with("+++\nterm = \"test\"\n+++\n"));
    assert!(result.contains("incipit {"));
    assert_eq!(
        result,
        "+++\nterm = \"test\"\n+++\nincipit {\n    nota \"ok\"\n}\n"
    );
}

#[test]
fn formatted_source_for_write_rejects_bad_frontmatter() {
    let path = Path::new("/tmp/test.fab");
    // Missing closing +++
    let raw = "+++\nterm = \"test\"\nincipit {\n  nota \"ok\"\n}\n";
    let formatted_body = "incipit {\n    nota \"ok\"\n}\n";
    let result = formatted_source_for_write(path, raw, formatted_body);
    assert!(result.is_err());
}

// ── source_for_compare ────────────────────────────────────────────────────

#[test]
fn source_for_compare_returns_raw_source_unchanged() {
    let path = Path::new("/tmp/test.fab");
    let raw = "incipit {}\n";
    assert_eq!(source_for_compare(path, raw), raw);
}

#[test]
fn source_for_compare_handles_source_with_frontmatter() {
    let path = Path::new("/tmp/test.fab");
    let raw = "+++\nterm = \"test\"\n+++\nincipit {\n  nota \"ok\"\n}\n";
    assert_eq!(source_for_compare(path, raw), raw);
}
