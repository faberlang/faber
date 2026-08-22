//! `faber format` canonical pipeline: HIR re-emit at the source's own locale.

use super::format::{
    format_session, formatted_source_for_write, normalize_trailing_newline, source_for_compare,
};
use radix::module::codegen::faber::compile_canonical;
use radix::module::codegen::faber::test_gate::{
    assert_canonical_idempotent_with_session, assert_canonical_reparses_with,
    canonical_format_once_with_session,
};
use radix::module::locale::LocalePack;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) fn exempla(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(path)
}

fn english_pack() -> LocalePack {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/en/pack.toml");
    LocalePack::from_toml_path(path).expect("load English locale pack")
}

fn latin_pack() -> LocalePack {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/la/pack.toml");
    LocalePack::from_toml_path(path).expect("load Latin locale pack")
}

/// Format a real file through the exact CLI canonical pipeline.
fn canonical_cli_pipeline(path: &Path) -> String {
    let source = fs::read_to_string(path).expect("read source");
    let session = format_session(path, None, &source).expect("format session");
    let name = path.display().to_string();
    let result = compile_canonical(&session, &name, &source);
    assert!(
        result.success(),
        "canonical format failed for {name}: {:?}",
        result.diagnostics
    );
    formatted_source_for_write(path, &source, &result.output.expect("output").code)
        .expect("format write pipeline")
}

/// Strip `#` comment lines so keyword-surface assertions only see emitted
/// code. Canonical reader-locale render preserves structural trivia
/// (comments), which may legitimately mention Latin spellings.
pub(crate) fn code_only(code: &str) -> String {
    code.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn format_canonical_output_reparses_without_errors() {
    let path = exempla("incipit/salve-munde.fab");
    let formatted = canonical_cli_pipeline(&path);
    let body = radix::module::driver::split_frontmatter(&formatted)
        .expect("split")
        .body;
    assert_canonical_reparses_with(body, "salve-munde pipeline", Some(&english_pack()))
        .expect("reparse");
}

#[test]
fn format_canonical_path_preserves_salve_munde_comments() {
    let path = exempla("incipit/salve-munde.fab");
    let formatted = canonical_cli_pipeline(&path);
    assert!(
        formatted.contains("# incipit — canonical hello-world entry point"),
        "canonical format should preserve leading comments"
    );
    assert!(formatted.contains("print \"Salve, Munde!\""));
}

/// Strict comparison is canonicalized to the ≺/≻ glyphs; ≤/≥ stay as-is.
#[test]
fn canonical_format_rewrites_ascii_comparison_to_glyph() {
    let dir = std::env::temp_dir().join("faber-format-canonical-cmp");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch");
    let path = dir.join("proba.fab");
    let source = r#"+++
locale = "en"
+++
class Proba {
    float x
    float y
    fn compara(float other) → bool {
        return self.x < other and self.x > other and self.x ≤ other and self.x ≥ other
    }
}
"#;
    fs::write(&path, source).expect("write");
    let formatted = canonical_cli_pipeline(&path);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        formatted.contains("self.x ≺ other"),
        "strict less-than should emit the ≺ glyph: {formatted}"
    );
    assert!(
        formatted.contains("self.x ≻ other"),
        "strict greater-than should emit the ≻ glyph: {formatted}"
    );
    assert!(formatted.contains("self.x ≤ other"));
    assert!(formatted.contains("self.x ≥ other"));
    assert!(
        !formatted.contains("< other"),
        "ASCII < comparison must not survive: {formatted}"
    );
    assert!(
        !formatted.contains("> other"),
        "ASCII > comparison must not survive: {formatted}"
    );
}

/// `<`/`>` remain ASCII generic application delimiters; en types stay English.
#[test]
fn canonical_format_keeps_ascii_angle_brackets_for_generics() {
    let dir = std::env::temp_dir().join("faber-format-canonical-generics");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch");
    let path = dir.join("proba.fab");
    let source = r#"+++
locale = "en"
+++
fn normaliza(list<float> values) → list<float> {
    return values
}
"#;
    fs::write(&path, source).expect("write");
    let formatted = canonical_cli_pipeline(&path);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        formatted.contains("list<float>"),
        "en generic types stay English and ASCII-delimited: {formatted}"
    );
    assert!(
        !formatted.contains("lista<fractus>"),
        "en format must not relatinize generic types: {formatted}"
    );
}

#[test]
fn format_canonical_pipeline_reparses_cura_fixture() {
    let path = exempla("cura/cura.fab");
    let formatted = canonical_cli_pipeline(&path);
    assert!(
        formatted.contains("print \"in arena\"") || formatted.contains("# cura"),
        "cura exempla should keep the arena-scoped body on the en surface:\n{formatted}"
    );
    let body = radix::module::driver::split_frontmatter(&formatted)
        .expect("split")
        .body;
    assert_canonical_reparses_with(body, "cura pipeline", Some(&english_pack())).expect("reparse");
}

#[test]
fn format_canonical_pipeline_preserves_and_reparses_comment_fixture() {
    let dir = std::env::temp_dir().join("faber-format-canonical-comment");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch");
    let path = dir.join("comment.fab");
    let source = "+++\nlocale = \"en\"\n+++\n# lead comment\n\nmain {\n  print \"ok\"\n}\n";
    fs::write(&path, source).expect("write");
    let formatted = canonical_cli_pipeline(&path);
    let session = format_session(&path, None, source).expect("session");
    let _ = fs::remove_dir_all(&dir);
    assert!(formatted.contains("# lead comment"));
    let body = radix::module::driver::split_frontmatter(&formatted)
        .expect("split")
        .body;
    assert_canonical_reparses_with(body, "comment pipeline", Some(&english_pack()))
        .expect("reparse");
    assert_canonical_idempotent_with_session(&session, "comment.fab", source).expect("idempotent");
}

#[test]
fn format_test_gate_matches_compile_canonical_pipeline_for_salve() {
    let path = exempla("incipit/salve-munde.fab");
    let source = fs::read_to_string(&path).expect("read");
    let name = path.display().to_string();
    let session = format_session(&path, None, &source).expect("session");
    let via_gate = canonical_format_once_with_session(&session, &name, &source).expect("gate");
    let via_pipeline = canonical_cli_pipeline(&path);
    let body = radix::module::driver::split_frontmatter(&via_pipeline)
        .expect("split")
        .body;
    assert_eq!(
        normalize_trailing_newline(&via_gate),
        normalize_trailing_newline(body),
        "test_gate and CLI pipeline must agree"
    );
}

pub(crate) fn faber_binary() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_faber") {
        return PathBuf::from(path);
    }
    // Shared-target layouts place the binary under CARGO_TARGET_DIR, not
    // <manifest>/target/debug. Prefer current_exe's parent (cargo test sets
    // CARGO_BIN_EXE when available; fallback walks from the test binary).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent.file_name().and_then(|n| n.to_str()) == Some("deps") {
                if let Some(debug_dir) = parent.parent() {
                    let candidate = debug_dir.join("faber");
                    if candidate.is_file() {
                        return candidate;
                    }
                }
            }
            let sibling = parent.join("faber");
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/faber")
}

pub(crate) fn run_faber_format_stdout(path: &Path) -> String {
    run_faber_format_stdout_with_args(&["format", "--stdout", path.to_str().expect("utf8 path")])
}

pub(crate) fn run_faber_format_stdout_with_args(args: &[&str]) -> String {
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

/// Semantically valid block source (canonical reader-locale re-emit runs
/// semantic analysis, so locale regression fixtures must name real
/// identifiers). Latin-identity surface, read under the la reader (the
/// product-default en pack is pack-authoritative).
pub(crate) const VALID_BLOCK_SOURCE: &str = "+++\nlocale = \"la\"\n+++\nfunctio gradus(numerus x) → numerus {\n    si x ≻ 0 {\n        redde x\n    } sin x ≡ 0 {\n        redde 0\n    } secus {\n        redde 0 - x\n    }\n}\n";

pub(crate) const EN_TYPES_SOURCE: &str = "+++\nlocale = \"en\"\n+++\nfn pack(int n, float x, string name, bytes buf) → string {\n    const int count ← n\n    const float scale ← x\n    const string label ← name\n    const bytes payload ← buf\n    print count\n    print scale\n    print label\n    print payload.length()\n    return label\n}\n";

/// Need: `faber format --stdout` on an en-frontmatter source must
/// keep English type spellings (the author engine relatinized them).
#[test]
fn format_cli_en_source_keeps_english_types() {
    let fixture = std::env::temp_dir().join("faber-format-en-types-cli.fab");
    fs::write(&fixture, EN_TYPES_SOURCE).expect("write en types fixture");

    let formatted = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);

    let body = radix::module::driver::split_frontmatter(&formatted)
        .expect("split")
        .body;
    assert!(
        body.contains("int")
            && body.contains("float")
            && body.contains("string")
            && body.contains("bytes"),
        "en format must keep English type spellings:\n{formatted}"
    );
    assert!(
        !(body.contains("numerus")
            || body.contains("fractus")
            || body.contains("textus")
            || body.contains("octeti")),
        "en format must not relatinize types:\n{formatted}"
    );
    assert_canonical_reparses_with(body, "en types CLI --stdout", Some(&english_pack()))
        .expect("reparse");
}

/// Surface-faithful the other way: la source stays la through the CLI.
#[test]
fn format_cli_la_source_stays_latin() {
    let fixture = std::env::temp_dir().join("faber-format-la-types-cli.fab");
    fs::write(&fixture, VALID_BLOCK_SOURCE).expect("write la fixture");

    let formatted = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);

    let body = radix::module::driver::split_frontmatter(&formatted)
        .expect("split")
        .body;
    assert!(
        body.contains("functio") && body.contains("numerus") && body.contains("redde"),
        "la format must keep Latin keyword and type spellings:\n{formatted}"
    );
    assert!(
        !code_only(body).contains("fn ") && !code_only(body).contains("int "),
        "la format must not anglicize the surface:\n{formatted}"
    );
    assert_canonical_reparses_with(body, "la types CLI --stdout", Some(&latin_pack()))
        .expect("reparse");
}

/// CLI format is idempotent: format(format(x)) is byte-identical to format(x).
#[test]
fn format_cli_en_source_is_idempotent() {
    let fixture = std::env::temp_dir().join("faber-format-en-idempotent.fab");
    fs::write(&fixture, EN_TYPES_SOURCE).expect("write en fixture");
    let first = run_faber_format_stdout(&fixture);
    fs::write(&fixture, &first).expect("rewrite first pass");
    let second = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);
    assert_eq!(
        first, second,
        "format(format(x)) must equal format(x) via CLI"
    );
}

/// Verification plan step 5: CLI `format --stdout` on comment fixture re-parses.
#[test]
fn format_cli_comment_fixture_reparses() {
    let fixture = std::env::temp_dir().join("faber-format-comment-unit.fab");
    fs::write(
        &fixture,
        "+++\nlocale = \"en\"\n+++\n# lead comment\n\nmain {\n  print \"ok\"\n}\n",
    )
    .expect("write comment fixture");

    let formatted = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);

    assert!(
        formatted.contains("# lead comment"),
        "CLI must preserve leading comment:\n{formatted}"
    );
    let body = radix::module::driver::split_frontmatter(&formatted)
        .expect("split")
        .body;
    assert_canonical_reparses_with(body, "comment CLI --stdout", Some(&english_pack()))
        .expect("reparse");
}

/// `--stdin` reads exactly one source document from stdin and prints the
/// formatted result to stdout — byte-identical to `--stdout` on the same
/// source (the same canonical pipeline, different input mode).
#[test]
fn format_stdin_roundtrip_matches_canonical_pipeline() {
    let source = "+++\nlocale = \"la\"\n+++\nincipit {\n  nota \"ok\"\n}\n";
    let fixture = std::env::temp_dir().join("faber-format-stdin-roundtrip.fab");
    fs::write(&fixture, source).expect("write roundtrip fixture");
    let expected = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);

    let (stdout, stderr, success) = run_faber_format_stdin(source);
    assert!(success, "faber format --stdin must succeed: {stderr}");
    assert_eq!(
        normalize_trailing_newline(&stdout),
        expected,
        "--stdin output must match the --stdout canonical pipeline byte-exactly"
    );
    assert!(
        stdout.contains("incipit {"),
        "--stdin output must be the formatted source:\n{stdout}"
    );
}

/// The stdin path must honor an English frontmatter locale just like the file
/// path; otherwise the canonical emitter falls back to Latin and cannot parse
/// the English source at all.
#[test]
fn format_stdin_en_source_keeps_english_surface() {
    let (stdout, stderr, success) = run_faber_format_stdin(EN_TYPES_SOURCE);
    assert!(
        success,
        "faber format --stdin must accept en source: {stderr}"
    );
    let body = radix::module::driver::split_frontmatter(&stdout)
        .expect("split")
        .body;
    assert!(
        body.contains("fn pack(int n, float x, string name, bytes buf)"),
        "en stdin format must keep the English surface:\n{stdout}"
    );
    assert!(
        !body.contains("functio pack(numerus n"),
        "en stdin format must not relatinize the source:\n{stdout}"
    );
    assert_canonical_reparses_with(body, "en types stdin", Some(&english_pack())).expect("reparse");
}

fn run_faber_format_stdin(source: &str) -> (String, String, bool) {
    let mut child = Command::new(faber_binary())
        .args(["format", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn faber format --stdin");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(source.as_bytes())
        .expect("write stdin source");
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
    let success = child.wait().expect("wait").success();
    (stdout, stderr, success)
}

/// `--stdout` is tightened to EXACTLY ONE input file — multiple files now
/// fail clearly instead of printing `=== path ===` separators.
#[test]
fn format_stdout_rejects_multiple_files() {
    let first = std::env::temp_dir().join("faber-format-multi-a.fab");
    let second = std::env::temp_dir().join("faber-format-multi-b.fab");
    fs::write(
        &first,
        "+++\nlocale = \"en\"\n+++\nmain {\n  print \"a\"\n}\n",
    )
    .expect("write first fixture");
    fs::write(
        &second,
        "+++\nlocale = \"en\"\n+++\nmain {\n  print \"b\"\n}\n",
    )
    .expect("write second fixture");

    let output = Command::new(faber_binary())
        .args(["format", "--stdout"])
        .arg(&first)
        .arg(&second)
        .output()
        .expect("run faber format --stdout with two files");
    let _ = fs::remove_file(&first);
    let _ = fs::remove_file(&second);

    assert!(
        !output.status.success(),
        "--stdout with multiple files must fail closed"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("exactly one input file"),
        "the rejection must name the single-file contract: {stderr}"
    );
    assert!(
        !stderr.contains("==="),
        "no separator output may survive the tightening: {stderr}"
    );
}

/// `--write` is the explicit spelling of the in-place default — it writes
/// the formatted source back to the file, byte-identical to what `--stdout`
/// would print for the same file.
#[test]
fn format_write_flag_writes_in_place() {
    let fixture = std::env::temp_dir().join("faber-format-write-flag.fab");
    let unformatted = "+++\nlocale = \"en\"\n+++\nmain {\n  print \"ok\"\n}\n";
    fs::write(&fixture, unformatted).expect("write fixture");

    let expected = run_faber_format_stdout(&fixture);
    let output = Command::new(faber_binary())
        .args(["format", "--write"])
        .arg(&fixture)
        .output()
        .expect("run faber format --write");
    let rewritten = fs::read_to_string(&fixture).expect("read rewritten fixture");
    let _ = fs::remove_file(&fixture);

    assert!(
        output.status.success(),
        "faber format --write must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        normalize_trailing_newline(&rewritten),
        expected,
        "--write must write the formatted source back in place"
    );
}

/// A leftover `[format]` table is an unknown manifest table (zero adopters;
/// D-A). Fail closed; do not silently ignore.
#[test]
fn format_legacy_format_table_fails_as_unknown_table() {
    let root = std::env::temp_dir().join("faber-format-legacy-format-table");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).expect("create package src");
    fs::write(
        root.join("faber.toml"),
        "[package]\nname = \"format-pkg\"\nversion = \"0.1.0\"\nedition = \"0.1\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n[format]\npolicy = \"normalise-v1\"\n",
    )
    .expect("write faber.toml");
    let fixture = root.join("src").join("main.fab");
    fs::write(&fixture, EN_TYPES_SOURCE).expect("write source");

    let output = Command::new(faber_binary())
        .args(["format", "--stdout"])
        .arg(&fixture)
        .output()
        .expect("run faber format on leftover [format] package");
    let _ = fs::remove_dir_all(&root);

    assert!(
        !output.status.success(),
        "a leftover [format] table must fail as an unknown table"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("format") || stderr.contains("unknown"),
        "the error must name the unknown table: {stderr}"
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
    let formatted = canonical_cli_pipeline(&path);
    let split = radix::module::driver::split_frontmatter(&raw).expect("split frontmatter");
    let body_start = split.body_byte_offset as usize;

    assert_eq!(
        &formatted[..body_start],
        &raw[..body_start],
        "format write must preserve the exact frontmatter prefix"
    );
    assert!(formatted.starts_with("+++"));
    assert!(formatted.contains("term = \"incipit\""));
    assert_canonical_reparses_with(
        &formatted[body_start..],
        "formatted salve-munde body",
        Some(&english_pack()),
    )
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
    let path = Path::new("test.fab");
    let raw = "incipit {\n  nota \"ok\"\n}\n";
    let formatted_body = "incipit {\n    nota \"ok\"\n}\n";
    let result = formatted_source_for_write(path, raw, formatted_body).expect("format source");
    assert_eq!(result, formatted_body);
}

#[test]
fn formatted_source_for_write_preserves_frontmatter_prefix() {
    let path = Path::new("test.fab");
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
    let path = Path::new("test.fab");
    // Missing closing +++
    let raw = "+++\nterm = \"test\"\nincipit {\n  nota \"ok\"\n}\n";
    let formatted_body = "incipit {\n    nota \"ok\"\n}\n";
    let result = formatted_source_for_write(path, raw, formatted_body);
    assert!(result.is_err());
}

// ── source_for_compare ────────────────────────────────────────────────────

#[test]
fn source_for_compare_returns_raw_source_unchanged() {
    let path = Path::new("test.fab");
    let raw = "incipit {}\n";
    assert_eq!(source_for_compare(path, raw), raw);
}

#[test]
fn source_for_compare_handles_source_with_frontmatter() {
    let path = Path::new("test.fab");
    let raw = "+++\nterm = \"test\"\n+++\nincipit {\n  nota \"ok\"\n}\n";
    assert_eq!(source_for_compare(path, raw), raw);
}

#[test]
fn source_for_compare_handles_empty_input() {
    let path = Path::new("test.fab");
    assert_eq!(source_for_compare(path, ""), "");
}

// ── normalize_trailing_newline edge cases ─────────────────────────────────

#[test]
fn normalize_trailing_newline_handles_mixed_whitespace() {
    assert_eq!(normalize_trailing_newline("text\n \t \n"), "text\n \t \n");
}

// ── formatted_source_for_write edge cases ─────────────────────────────────

#[test]
fn formatted_source_for_write_handles_empty_body() {
    let path = Path::new("test.fab");
    let raw = "+++\nterm = \"test\"\n+++\n";
    let formatted_body = "";
    let result = formatted_source_for_write(path, raw, formatted_body).expect("format source");
    assert!(result.starts_with("+++\nterm = \"test\"\n+++\n"));
}

#[test]
fn formatted_source_for_write_handles_no_frontmatter_with_empty_raw() {
    let path = Path::new("test.fab");
    let result = formatted_source_for_write(path, "", "").expect("format source");
    assert_eq!(result, "");
}
