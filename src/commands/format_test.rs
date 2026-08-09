//! `faber format` author pipeline: `compile_author`, normalization, re-parse.

use super::format::{formatted_source_for_write, normalize_trailing_newline, source_for_compare};
use radix::driver::{Config, Session};
use radix::forma::test_gate::{assert_author_reparses, author_format_once_with_session};
use radix::locale::LocalePack;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn exempla(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../radix/corpus")
        .join(path)
}

fn english_pack() -> LocalePack {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../radix/stdlib/locale/en/pack.toml");
    LocalePack::from_toml_path(path).expect("load English locale pack")
}

fn english_session() -> Session {
    Session::new(Config::default().with_locale_pack(english_pack()))
}

fn author_format_pipeline(name: &str, source: &str) -> String {
    let session = english_session();
    let result = radix::forma::compile_author(&session, name, source);
    assert!(
        result.success(),
        "author format failed for {name}: {:?}",
        result.diagnostics
    );
    normalize_trailing_newline(&result.output.expect("output").code)
}

fn assert_author_idempotent(name: &str, source: &str) {
    let session = english_session();
    let first =
        author_format_once_with_session(&session, name, source).expect("first author format pass");
    let second =
        author_format_once_with_session(&session, name, &first).expect("second author format pass");
    assert_eq!(first, second, "{name}: author(author(x)) != author(x)");
}

/// Strip `#` comment lines so keyword-surface assertions only see emitted
/// code. Canonical reader-locale render preserves structural trivia
/// (comments), which may legitimately mention Latin spellings.
fn code_only(code: &str) -> String {
    code.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
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

/// Strict comparison is canonicalized to the ≺/≻ glyphs; ≤/≥ stay as-is.
#[test]
fn author_format_rewrites_ascii_comparison_to_glyph() {
    let source = r"class Proba {
    float x
    float y
    fn compara(float other) → bool {
        return self.x < other and self.x > other and self.x ≤ other and self.x ≥ other
    }
}";
    let formatted = author_format_pipeline("proba.fab", source);
    assert!(
        formatted.contains("ego.x ≺ other"),
        "strict less-than should emit the ≺ glyph: {formatted}"
    );
    assert!(
        formatted.contains("ego.x ≻ other"),
        "strict greater-than should emit the ≻ glyph: {formatted}"
    );
    assert!(formatted.contains("ego.x ≤ other"));
    assert!(formatted.contains("ego.x ≥ other"));
    assert!(
        !formatted.contains("< other"),
        "ASCII < comparison must not survive: {formatted}"
    );
    assert!(
        !formatted.contains("> other"),
        "ASCII > comparison must not survive: {formatted}"
    );
}

/// `<`/`>` remain ASCII generic application delimiters.
#[test]
fn author_format_keeps_ascii_angle_brackets_for_generics() {
    let source = r"fn normaliza(list<float> values) → list<float> {
    return values
}";
    let formatted = author_format_pipeline("proba.fab", source);
    assert!(
        formatted.contains("lista<fractus>"),
        "generic delimiters stay ASCII: {formatted}"
    );
}

/// Regression: `format --locale` must resolve provider/relative imports via the
/// import contract. The forma re-emit path used to report SEM002
/// unknown-qualified-type on any file with imports.
#[test]
fn format_locale_resolves_imports_in_corpus_fixture() {
    let fixture = exempla("importa/default-minimal.fab");
    let home = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("faber crate must sit in the faberlang container")
        .to_path_buf();
    let mut child = Command::new(faber_binary())
        .args(["format", "--locale", "la", "--stdout"])
        .arg(&fixture)
        .env("FABER_LIBRARY_HOME", &home)
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
        "faber format --locale la failed on importing fixture: {stderr}"
    );
    assert!(
        !stderr.contains("SEM002"),
        "imports must resolve via the import contract: {stderr}"
    );
    assert!(
        stdout.contains("importa ex \"norma:chorda\""),
        "import line should survive re-emit"
    );
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
    let source = "# lead comment\n\nmain {\n  print \"ok\"\n}\n";
    let formatted = author_format_pipeline("comment.fab", source);
    assert!(formatted.contains("# lead comment"));
    assert_author_reparses(&formatted, "comment pipeline").expect("reparse");
    assert_author_idempotent("comment.fab", source);
}

#[test]
fn format_test_gate_matches_compile_author_pipeline_for_salve() {
    let path = exempla("incipit/salve-munde.fab");
    let source = fs::read_to_string(&path).expect("read");
    let name = path.display().to_string();
    let via_gate =
        author_format_once_with_session(&english_session(), &name, &source).expect("gate");
    let via_pipeline = author_format_pipeline(&name, &source);
    assert_eq!(
        via_gate, via_pipeline,
        "test_gate and CLI pipeline must agree"
    );
}

fn faber_binary() -> PathBuf {
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
    fs::write(&fixture, "# lead comment\n\nmain {\n  print \"ok\"\n}\n")
        .expect("write comment fixture");

    let formatted = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);

    assert!(
        formatted.contains("# lead comment"),
        "CLI must preserve leading comment:\n{formatted}"
    );
    assert_author_reparses(&formatted, "comment CLI --stdout").expect("reparse");
}

// ── FORMAT-PRETTY S4 steady-state flag surface ────────────────────────────

/// Run `faber format --stdin` feeding `source` on stdin; return (stdout,
/// stderr, success).
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

/// S4: `--stdin` reads exactly one source document from stdin and prints the
/// formatted result to stdout — byte-identical to `--stdout` on the same
/// source (the same author pipeline, different input mode).
#[test]
fn format_stdin_roundtrip_matches_author_pipeline() {
    let source = "incipit {\n  nota \"ok\"\n}\n";
    let fixture = std::env::temp_dir().join("faber-format-stdin-roundtrip.fab");
    fs::write(&fixture, source).expect("write roundtrip fixture");
    let expected = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);

    let (stdout, stderr, success) = run_faber_format_stdin(source);
    assert!(
        success,
        "faber format --stdin must succeed: {stderr}"
    );
    assert_eq!(
        normalize_trailing_newline(&stdout),
        expected,
        "--stdin output must match the --stdout author pipeline byte-exactly"
    );
    assert!(
        stdout.contains("incipit {"),
        "--stdin output must be the formatted source:\n{stdout}"
    );
}

/// S4: `--stdout` is tightened to EXACTLY ONE input file — multiple files now
/// fail clearly instead of printing `=== path ===` separators.
#[test]
fn format_stdout_rejects_multiple_files() {
    let first = std::env::temp_dir().join("faber-format-multi-a.fab");
    let second = std::env::temp_dir().join("faber-format-multi-b.fab");
    fs::write(&first, "incipit {\n  nota \"a\"\n}\n").expect("write first fixture");
    fs::write(&second, "incipit {\n  nota \"b\"\n}\n").expect("write second fixture");

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

/// S4: an unknown `--policy` slug fails clearly, exit nonzero, with a message
/// distinct from formatting-difference output.
#[test]
fn format_unknown_policy_slug_fails_clearly() {
    let fixture = std::env::temp_dir().join("faber-format-policy-bogus.fab");
    fs::write(&fixture, "incipit {\n  nota \"ok\"\n}\n").expect("write fixture");

    let output = Command::new(faber_binary())
        .args(["format", "--policy", "not-a-policy"])
        .arg(&fixture)
        .output()
        .expect("run faber format --policy bogus");
    let _ = fs::remove_file(&fixture);

    assert!(
        !output.status.success(),
        "unknown policy slug must exit nonzero"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not-a-policy"),
        "the error must name the offending slug: {stderr}"
    );
    assert!(
        stderr.contains("registered slugs"),
        "the error must point at the rule-slug registry: {stderr}"
    );
}

/// S4: `--policy normalise-v1` is the built-in baseline — output must be
/// byte-identical to the flagless author pipeline.
#[test]
fn format_policy_normalise_v1_matches_default_output() {
    let fixture = std::env::temp_dir().join("faber-format-policy-normalise.fab");
    fs::write(&fixture, "functio gradus(numerus x) → numerus {\n    si x ≻ 0 ergo redde x\n    secus ergo redde 0 - x\n}\n")
        .expect("write fixture");

    let with_policy = run_faber_format_stdout_with_args(&[
        "format",
        "--policy",
        "normalise-v1",
        "--stdout",
        fixture.to_str().expect("utf8 path"),
    ]);
    let default = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);

    assert_eq!(
        with_policy, default,
        "--policy normalise-v1 must be byte-identical to the default author output"
    );
}

/// S4: `--policy pretty-v1` selects the pretty engine and succeeds on a plain
/// block fixture (the engine may leave constructs unchanged fail-closed, but
/// must never error on a supported block shape).
#[test]
fn format_policy_pretty_v1_succeeds_on_block_fixture() {
    let fixture = std::env::temp_dir().join("faber-format-policy-pretty.fab");
    fs::write(&fixture, "incipit {\n    nota \"ok\"\n}\n").expect("write fixture");

    let formatted = run_faber_format_stdout_with_args(&[
        "format",
        "--policy",
        "pretty-v1",
        "--stdout",
        fixture.to_str().expect("utf8 path"),
    ]);
    let _ = fs::remove_file(&fixture);

    assert!(
        formatted.contains("incipit {"),
        "pretty-v1 must emit the formatted block:\n{formatted}"
    );
    assert_author_reparses(&formatted, "pretty-v1 block fixture").expect("reparse");
}

/// S4 locale-interplay contract: `--policy` cannot be honored on the
/// reader-locale re-emit path (canonical HIR output) — the combination must be
/// rejected explicitly, never silently downgraded.
#[test]
fn format_policy_with_locale_is_rejected() {
    let fixture = std::env::temp_dir().join("faber-format-policy-locale.fab");
    fs::write(&fixture, "incipit {\n  nota \"ok\"\n}\n").expect("write fixture");

    let output = Command::new(faber_binary())
        .args([
            "format",
            "--locale",
            "en",
            "--policy",
            "normalise-v1",
            "--stdout",
        ])
        .arg(&fixture)
        .output()
        .expect("run faber format --locale --policy");
    let _ = fs::remove_file(&fixture);

    assert!(
        !output.status.success(),
        "--policy with --locale must be rejected, not silently downgraded"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--policy"),
        "the rejection must name --policy: {stderr}"
    );
    assert!(
        stderr.contains("--locale"),
        "the rejection must name --locale: {stderr}"
    );
}

/// S4: `--write` is the explicit spelling of the in-place default — it writes
/// the formatted source back to the file, byte-identical to what `--stdout`
/// would print for the same file.
#[test]
fn format_write_flag_writes_in_place() {
    let fixture = std::env::temp_dir().join("faber-format-write-flag.fab");
    let unformatted = "incipit {\n  nota \"ok\"\n}\n";
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

#[test]
fn format_locale_thai_localizes_surface() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/reader-locale/th-TH");
    let thai = root.join("src/main.fab");
    let latin = root.join("twins/main.la.fab");

    let thai_output = run_faber_format_stdout_with_args(&[
        "format",
        "--locale",
        "th-TH",
        "--stdout",
        thai.to_str().expect("utf8 thai path"),
    ]);

    // --locale drives the emitter surface: the re-emit localizes
    // reader-locale keywords and types into Thai and no longer matches the
    // Latin twin (which re-emits as `--locale la`).
    assert_ne!(
        thai_output,
        run_faber_format_stdout_with_args(&[
            "format",
            "--locale",
            "la",
            "--stdout",
            latin.to_str().expect("utf8 latin path"),
        ]),
        "localized Thai emit must differ from the Latin twin"
    );

    assert!(thai_output.contains("ฟังก์ชัน ทักทาย(ข้อความ name) → ข้อความ"));
    assert!(thai_output.contains("คงที่ ข้อความ greeting ← \"สวัสดี, §!\"(name)"));
    assert!(thai_output.contains("คืน greeting"));
    assert!(thai_output.contains("ฟังก์ชัน ผ่าน(จำนวน score) → ตรรกะ"));
    assert!(thai_output.contains("เช่นนั้น จริง มิฉะนั้น score ≥ 50 เช่นนั้น จริง มิฉะนั้น เท็จ"));
    assert!(thai_output.contains("ฟังก์ชัน นับผ่าน(รายการ<จำนวน> scores) → จำนวน"));
    assert!(thai_output.contains("แปร จำนวน total ← 0"));
    assert!(thai_output.contains("วน จาก scores คงที่ score"));
    assert!(thai_output.contains("ถ้า score ≺ 0 {"));
    assert!(thai_output.contains("ไปต่อ"));
    assert!(thai_output.contains("หยุด"));
    assert!(thai_output.contains("ฟังก์ชัน นับถอยหลัง(จำนวน start) → จำนวน"));
    assert!(thai_output.contains("ขณะ current ≻ 0"));
    assert!(thai_output.contains("เริ่ม {"));
    assert!(thai_output.contains("คงที่ จำนวน score ← 82"));
    assert!(thai_output.contains("คงที่ รายการ<จำนวน> scores ← [-1, 82, 41, 60]"));
    assert!(thai_output.contains("บันทึก ผ่าน(score)"));
    assert!(thai_output.contains("บันทึก นับผ่าน(scores)"));
    // The Latin keyword surface must not survive localized re-emit, and the
    // template-application glyph `"…"(args)` must not expand to a named
    // `scriptum(...)` call. Comments are preserved trivia and may mention
    // Latin spellings, so the negative assertions run against code-only text.
    let thai_code = code_only(&thai_output);
    assert!(!thai_code.contains("scriptum"));
    assert!(!thai_code.contains("functio"));
    assert!(!thai_code.contains("fixum"));
    assert!(!thai_code.contains("textus"));
    assert!(!thai_code.contains("numerus"));
    assert!(!thai_code.contains("bivalens"));
    assert!(!thai_code.contains("lista<"));
    assert!(!thai_code.contains("itera"));
    assert!(!thai_code.contains("incipit"));
}

#[test]
fn format_locale_la_emits_latin_surface() {
    let path = exempla("incipit/salve-munde.fab");

    let locale_output = run_faber_format_stdout_with_args(&[
        "format",
        "--locale",
        "la",
        "--stdout",
        path.to_str().expect("utf8 path"),
    ]);

    assert!(locale_output.contains("incipit {"));
    assert!(locale_output.contains("nota \"Salve, Munde!\""));
}

#[test]
fn format_locale_preserves_template_application_sugar() {
    // Reader-locale packs re-render the same semantic program with different
    // keyword spellings while retaining glyph shapes. The `"…"(args)` template-
    // application postfix is a glyph shape: `--locale en` localizes
    // `nota` → `print` but must keep `print "val § here"(n)` instead of
    // expanding into `print format("val § here", n)`.
    let fixture = std::env::temp_dir().join("faber-format-rl-template-sugar.fab");
    fs::write(
        &fixture,
        "functio monstra(textus n) {\n  nota \"val § here\"(n)\n}\n",
    )
    .expect("write rl template-sugar fixture");

    let llm_output = run_faber_format_stdout_with_args(&[
        "format",
        "--locale",
        "en",
        "--stdout",
        fixture.to_str().expect("utf8 path"),
    ]);
    let author_output = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);

    assert!(
        llm_output.contains("print \"val § here\"(n)"),
        "reader-locale must localize nota→print and keep the postfix sugar:\n{llm_output}"
    );
    assert!(
        !llm_output.contains("format("),
        "reader-locale must not expand template application into a named call:\n{llm_output}"
    );
    assert!(
        author_output.contains("nota \"val § here\"(n)"),
        "author surface must keep the sugar:\n{author_output}"
    );
}

#[test]
fn format_locale_localizes() {
    // A bare --locale=<X> selects the HIR-backed re-emit path with the
    // localized surface (author mode when --locale is absent).
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/reader-locale/th-TH");
    let thai = root.join("src/main.fab");

    let output = Command::new(faber_binary())
        .args([
            "format",
            "--locale",
            "th-TH",
            "--stdout",
            thai.to_str().expect("utf8 thai path"),
        ])
        .output()
        .expect("run faber format");

    assert!(
        output.status.success(),
        "reader-locale formatting must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(
        stdout.contains("ฟังก์ชัน"),
        "bare --locale=th-TH must emit the Thai surface: {stdout}"
    );
    assert!(
        !code_only(&stdout).contains("functio"),
        "bare --locale=th-TH must not emit the Latin keyword: {stdout}"
    );
}

#[test]
fn format_locale_check_passes_on_braced_futura_exempla() {
    let path = exempla("annotation-sugar/futura-braced.fab");
    let mut child = Command::new(faber_binary())
        .args([
            "format",
            "--check",
            "--locale",
            "en",
            path.to_str().expect("utf8 path"),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn faber format --check --locale en");
    let status = child.wait().expect("wait");
    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    assert!(
        status.success(),
        "locale check must pass on braced futura exempla: {stderr}"
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
