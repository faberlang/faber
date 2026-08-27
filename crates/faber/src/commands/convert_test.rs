//! Integration tests for `faber convert` (reader-locale source conversion).
//! Ported from the former `faber format --locale` surface; see
//! docs/operator/convert-command.md.

use super::super::format_test::{
    code_only, exempla, faber_binary, run_faber_format_stdout,
    run_faber_format_stdout_with_args as run_faber_convert_stdout_with_args,
};
use std::fs;
use std::io::{Read, Write};
use std::process::{Command, Stdio};

#[test]
fn convert_resolves_imports_in_corpus_fixture() {
    let fixture = exempla("importa/default-minimal.fab");
    let home = radix::module::paths::faberlang_home()
        .expect("faber crate must sit in the faberlang container");
    let mut child = Command::new(faber_binary())
        .args(["convert", "--to", "la", "--stdout"])
        .arg(&fixture)
        .env("FABER_LIBRARY_HOME", &home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn faber convert");
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
        "faber convert --to la failed on importing fixture: {stderr}"
    );
    assert!(
        !stderr.contains("error"),
        "imports must resolve without errors: {stderr}"
    );
    assert!(
        stdout.contains("importa ex \"norma:text\""),
        "import line should survive re-emit"
    );
}

#[test]
fn convert_thai_localizes_surface() {
    let root = radix::module::paths::faberlang_home()
        .expect("faberlang container root")
        .join("examples/reader-locale/th-TH");
    let thai = root.join("src/main.fab");
    let latin = root.join("twins/main.la.fab");

    let thai_output = run_faber_convert_stdout_with_args(&[
        "convert",
        "--to",
        "th-TH",
        "--stdout",
        thai.to_str().expect("utf8 thai path"),
    ]);

    // --locale drives the emitter surface: the re-emit localizes
    // reader-locale keywords and types into Thai and no longer matches the
    // Latin twin (which re-emits as `--locale la`).
    assert_ne!(
        thai_output,
        run_faber_convert_stdout_with_args(&[
            "convert",
            "--to",
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
    assert!(thai_output.contains("ถ้าไม่ก็ score ≥ 50 ดังนั้น คืน จริง"));
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
fn convert_la_emits_latin_surface() {
    let path = exempla("incipit/salve-munde.fab");

    let locale_output = run_faber_convert_stdout_with_args(&[
        "convert",
        "--to",
        "la",
        "--stdout",
        path.to_str().expect("utf8 path"),
    ]);

    assert!(locale_output.contains("incipit {"));
    assert!(locale_output.contains("nota \"Salve, Munde!\""));
}

/// `convert --stdin` must install the requested target pack. The old virtual
/// source branch silently fell back to Latin, making `--to en` a no-op.
#[test]
fn convert_stdin_to_english_localizes_latin_source() {
    let source = super::super::format_test::VALID_BLOCK_SOURCE;
    let (stdout, stderr, success) = run_faber_convert_stdin("en", source);
    assert!(success, "faber convert --stdin must succeed: {stderr}");
    assert!(
        stdout.contains("fn gradus(int x)"),
        "convert --stdin --to en must emit the English surface:\n{stdout}"
    );
    assert!(
        !code_only(&stdout).contains("functio"),
        "convert --stdin --to en must not silently preserve Latin:\n{stdout}"
    );
}

/// Conversion rewrites the frontmatter locale with the emitted body so the
/// result can be formatted again under its own declared surface.
#[test]
fn convert_rewrites_frontmatter_to_target_locale() {
    let fixture = std::env::temp_dir().join("faber-convert-frontmatter-locale.fab");
    let source = super::super::format_test::VALID_BLOCK_SOURCE;
    fs::write(&fixture, source).expect("write Latin fixture");
    let output = run_faber_convert_stdout_with_args(&[
        "convert",
        "--to",
        "en",
        "--stdout",
        fixture.to_str().expect("utf8 path"),
    ]);
    let _ = fs::remove_file(&fixture);

    assert!(
        output.contains("locale = \"en\""),
        "converted frontmatter must declare the target locale:\n{output}"
    );
    assert!(
        output.contains("fn gradus(int x)"),
        "converted body must use the target locale:\n{output}"
    );
}

/// `--from` selects the source reader pack when frontmatter is stale.
#[test]
fn convert_from_override_reads_mislabeled_english_source() {
    let fixture = std::env::temp_dir().join("faber-convert-from-override.fab");
    let source =
        super::super::format_test::EN_TYPES_SOURCE.replace("locale = \"en\"", "locale = \"la\"");
    fs::write(&fixture, source).expect("write mislabeled fixture");
    let output = Command::new(faber_binary())
        .args(["convert", "--from", "en", "--to", "la", "--stdout"])
        .arg(&fixture)
        .output()
        .expect("run faber convert --from en --to la");
    let _ = fs::remove_file(&fixture);

    assert!(
        output.status.success(),
        "--from must override stale frontmatter: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 converted source");
    assert!(stdout.contains("locale = \"la\""));
    assert!(
        stdout.contains("functio pack(numerus n"),
        "--from en must parse English and emit Latin:\n{stdout}"
    );
}

fn run_faber_convert_stdin(to: &str, source: &str) -> (String, String, bool) {
    let mut child = Command::new(faber_binary())
        .args(["convert", "--to", to, "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn faber convert --stdin");
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

#[test]
fn convert_preserves_template_application_sugar() {
    // Reader-locale packs re-render the same semantic program with different
    // keyword spellings while retaining glyph shapes. The `"…"(args)` template-
    // application postfix is a glyph shape: `--locale en` localizes
    // `nota` → `print` but must keep `print "val § here"(n)` instead of
    // expanding into `print format("val § here", n)`.
    let fixture = std::env::temp_dir().join("faber-format-rl-template-sugar.fab");
    fs::write(
        &fixture,
        "+++\nlocale = \"la\"\n+++\nfunctio monstra(textus n) {\n  nota \"val § here\"(n)\n}\n",
    )
    .expect("write rl template-sugar fixture");

    let llm_output = run_faber_convert_stdout_with_args(&[
        "convert",
        "--to",
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
fn convert_localizes() {
    // A bare --locale=<X> selects the HIR-backed re-emit path with the
    // localized surface (author mode when --locale is absent).
    let root = radix::module::paths::faberlang_home()
        .expect("faberlang container root")
        .join("examples/reader-locale/th-TH");
    let thai = root.join("src/main.fab");

    let output = Command::new(faber_binary())
        .args([
            "convert",
            "--to",
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
fn convert_check_passes_on_braced_futura_exempla() {
    let path = exempla("annotation-sugar/futura-braced.fab");
    let mut child = Command::new(faber_binary())
        .args([
            "convert",
            "--check",
            "--to",
            "en",
            path.to_str().expect("utf8 path"),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn faber convert --check --to en");
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

/// CTR-11 closeout regression (conversio-fn-enrollment acceptance 7): a named
/// standalone `@ conversion` function survives a real English → Latin →
/// English `faber convert` round trip on a package — name, body, and
/// annotation identity retained, both emitted locales re-checked with the
/// same diagnostics set, and no synthetic `conversio_<dest>_ex_<src>` output.
/// This is the user-visible proof for the bug class that motivated the goal:
/// the old emitter dropped the union arm and emitted a synthetic function
/// that did not re-check. A conversion fn that silently drops in either
/// direction fails twice here — the marker/name/body assertions fail on the
/// emitted text, and the re-check loses the registry row so the `↦` and `⇥`
/// use-sites stop checking (`failable_call_conversion_absent`).
#[test]
fn convert_round_trip_retains_named_conversion_fn_en_la_en() {
    let root = tempfile::tempdir().expect("create round-trip fixture root");
    let package = root.path().join("ctr11roundtrip");
    let source_dir = package.join("src");
    fs::create_dir_all(&source_dir).expect("create package source dir");
    write_round_trip_manifest(&package, "en");
    fs::write(source_dir.join("main.fab"), ROUND_TRIP_EN_SOURCE).expect("write en source");

    let binary = faber_binary();

    // Canonicalize under the package's own en surface first, so every later
    // state is an emitter product and text/diagnostics compare exactly.
    run_convert_in_place(&binary, &source_dir, "en");
    let canonical_en =
        fs::read_to_string(source_dir.join("main.fab")).expect("read canonical en");
    assert!(
        canonical_en.contains("@ conversion\nfn from_source_error(SourceError err) → DestError {"),
        "canonical en emit must keep the named conversion fn with its marker:\n{canonical_en}"
    );
    assert!(
        canonical_en.contains("return variant Wrapped {payload = detail}"),
        "canonical en emit must keep the conversion body:\n{canonical_en}"
    );
    assert!(
        canonical_en.contains("return err ↦ DestError"),
        "canonical en emit must keep the explicit-conversion use-site:\n{canonical_en}"
    );
    assert!(
        !canonical_en.contains("conversio_"),
        "no synthetic conversio_<dest>_ex_<src> name may appear:\n{canonical_en}"
    );
    let en_diagnostics = run_round_trip_check(&binary, &package);

    // English → Latin: the marker, name, body, and both registry consumers
    // must re-emit on the Latin surface and re-check with the same
    // diagnostics set.
    run_convert_in_place(&binary, &source_dir, "la");
    write_round_trip_manifest(&package, "la");
    let la_source = fs::read_to_string(source_dir.join("main.fab")).expect("read la source");
    assert!(
        la_source.contains("@ conversio\nfunctio from_source_error(SourceError err) → DestError {"),
        "la emit must keep the named conversion fn with its marker:\n{la_source}"
    );
    assert!(
        la_source.contains("redde finge Wrapped {payload = detail}"),
        "la emit must keep the conversion body:\n{la_source}"
    );
    assert!(
        la_source.contains("functio propagate() → vacuum ⇥ DestError {"),
        "la emit must keep the failable-call propagation use-site:\n{la_source}"
    );
    assert!(
        la_source.contains("redde err ↦ DestError"),
        "la emit must keep the explicit-conversion use-site:\n{la_source}"
    );
    assert!(
        !la_source.contains("conversio_"),
        "no synthetic conversio_<dest>_ex_<src> name may appear:\n{la_source}"
    );
    assert!(
        !code_only(&la_source).contains("@ conversion"),
        "the en marker spelling must not survive the la emit:\n{la_source}"
    );
    let la_diagnostics = run_round_trip_check(&binary, &package);
    assert_eq!(
        en_diagnostics, la_diagnostics,
        "en and la outputs must carry the same diagnostics set"
    );

    // Latin → English: the round trip must land back on the canonical en
    // emit byte for byte — full name/body/annotation identity — and re-check
    // with the same diagnostics set.
    run_convert_in_place(&binary, &source_dir, "en");
    write_round_trip_manifest(&package, "en");
    let round_trip_en =
        fs::read_to_string(source_dir.join("main.fab")).expect("read round-trip en");
    assert_eq!(
        round_trip_en, canonical_en,
        "en → la → en must return the canonical en source exactly"
    );
    let round_trip_diagnostics = run_round_trip_check(&binary, &package);
    assert_eq!(
        en_diagnostics, round_trip_diagnostics,
        "the round-tripped en output must carry the same diagnostics set"
    );
}

/// EN fixture: one owned error-union pair, the named standalone conversion
/// fn enrolled for `SourceError → DestError`, and both registry consumers
/// (`↦` explicit conversion, `⇥` failable-call propagation) at use-sites.
const ROUND_TRIP_EN_SOURCE: &str = r#"+++
locale = "en"
+++

union SourceError {
    Malformed {
        string detail
    }
}

union DestError {
    Wrapped {
        string payload
    }
}

@ conversion fn from_source_error(SourceError err) → DestError {
    match err {
        case SourceError.Malformed const detail { return variant Wrapped {payload = detail} }
        case _ { return variant Wrapped {payload = "unmatched"} }
    }
}

fn fail_source() → void ⇥ SourceError {
    throw variant Malformed {detail = "ctr11 round trip"}
}

fn propagate() → void ⇥ DestError {
    fail_source()
}

fn explicit_convert(SourceError err) → DestError {
    return err ↦ DestError
}

main {
}
"#;

fn write_round_trip_manifest(package: &std::path::Path, locale: &str) {
    fs::write(
        package.join("faber.toml"),
        format!(
            "[package]\nname = \"ctr11-roundtrip\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n\
             [paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n\
             [locale]\nlocale = \"{locale}\"\n"
        ),
    )
    .expect("write round-trip manifest");
}

fn run_convert_in_place(binary: &std::path::Path, source_dir: &std::path::Path, to: &str) {
    let output = Command::new(binary)
        .args([
            "convert",
            source_dir.to_str().expect("utf8 source dir"),
            "--to",
            to,
        ])
        .output()
        .expect("run faber convert in place");
    assert!(
        output.status.success(),
        "faber convert --to {to} must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Check the package and return its diagnostics header lines
/// (`warning[CODE:issue]: path:line`). Header lines only: the source-context
/// lines below each header restate the flagged line in the active locale's
/// spellings, so they differ between en and la by design; the codes, issues,
/// and locations are the semantic diagnostics set.
fn run_round_trip_check(binary: &std::path::Path, package: &std::path::Path) -> Vec<String> {
    let output = Command::new(binary)
        .args(["check", package.to_str().expect("utf8 package")])
        .output()
        .expect("run faber check");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "round-trip package must re-check cleanly:\n{stderr}"
    );
    assert!(
        stderr.contains("ok:"),
        "successful check must report the ok verdict:\n{stderr}"
    );
    stderr
        .lines()
        .filter(|line| line.starts_with("warning[") || line.starts_with("error["))
        .map(str::to_owned)
        .collect()
}
