//! Integration tests for `faber convert` (reader-locale source conversion).
//! Ported from the former `faber format --locale` surface; see
//! docs/operator/convert-command.md.

use super::super::format_test::{
    code_only, exempla, faber_binary, run_faber_format_stdout,
    run_faber_format_stdout_with_args as run_faber_convert_stdout_with_args, temp_format_package,
    VALID_BLOCK_SOURCE,
};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn convert_resolves_imports_in_corpus_fixture() {
    let fixture = exempla("importa/default-minimal.fab");
    let home =
        radix::paths::faberlang_home().expect("faber crate must sit in the faberlang container");
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
        !stderr.contains("SEM002"),
        "imports must resolve via the import contract: {stderr}"
    );
    assert!(
        stdout.contains("importa ex \"norma:chorda\""),
        "import line should survive re-emit"
    );
}

#[test]
#[test]
fn convert_is_not_affected_by_manifest_format_policy() {
    let root_with = std::env::temp_dir().join("faber-format-s5-locale-with");
    let root_without = std::env::temp_dir().join("faber-format-s5-locale-without");
    let _ = fs::remove_dir_all(&root_with);
    let _ = fs::remove_dir_all(&root_without);
    let fixture_with = temp_format_package(&root_with, Some("pretty-v1"), VALID_BLOCK_SOURCE);
    let fixture_without = temp_format_package(&root_without, None, VALID_BLOCK_SOURCE);

    let run = |fixture: &Path| {
        Command::new(faber_binary())
            .args(["convert", "--to", "en", "--stdout"])
            .arg(fixture)
            .output()
            .expect("run faber convert --to en")
    };
    let with = run(&fixture_with);
    let without = run(&fixture_without);
    let stderr_with = String::from_utf8_lossy(&with.stderr);
    let stderr_without = String::from_utf8_lossy(&without.stderr);
    let _ = fs::remove_dir_all(&root_with);
    let _ = fs::remove_dir_all(&root_without);

    assert!(
        with.status.success(),
        "manifest policy + --locale must succeed: {stderr_with}"
    );
    assert!(
        without.status.success(),
        "no-[format] package + --locale must succeed: {stderr_without}"
    );
    assert_eq!(
        with.stdout, without.stdout,
        "the [format] table must not change locale re-emission (byte-identical)"
    );
    assert!(
        !stderr_with.contains("--policy"),
        "manifest-declared policy must not trip the CLI --policy/--locale conflict: {stderr_with}"
    );
}

#[test]
#[test]
fn convert_thai_localizes_surface() {
    let root = radix::paths::faberlang_home()
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

#[test]
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
#[test]
fn convert_localizes() {
    // A bare --locale=<X> selects the HIR-backed re-emit path with the
    // localized surface (author mode when --locale is absent).
    let root = radix::paths::faberlang_home()
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
