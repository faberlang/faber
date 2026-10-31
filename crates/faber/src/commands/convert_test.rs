//! Integration tests for `faber convert` (reader-locale source conversion).
//! Ported from the former reader-locale conversion surface; see
//! docs/operator/convert-command.md.

use super::super::format_test::{
    HOOK_SOURCE, code_only, exempla, faber_binary, run_faber_capture, run_faber_format_stdout,
    run_faber_format_stdout_with_args as run_faber_convert_stdout_with_args,
};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn convert_reports_filewise_failure_for_multi_file_kernel_package() {
    let root = tempfile::tempdir().expect("create two-file package root");
    let package = root.path().join("convert-kernel-package");
    write_convert_kernel_package(&package);
    let binary = faber_binary();

    let check = Command::new(&binary)
        .args(["check", package.to_str().expect("utf8 package path")])
        .output()
        .expect("check two-file package");
    assert!(
        check.status.success(),
        "whole-package check must accept the fixture:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );

    let conversion = Command::new(&binary)
        .args(["convert", "--to", "la", "--check", "--no-diagnostics"])
        .arg(&package)
        .output()
        .expect("convert two-file package file-by-file");
    assert!(!conversion.status.success());
    let stderr = String::from_utf8_lossy(&conversion.stderr);
    let has_kernel_contract_diagnostic = stderr.lines().any(|line| {
        line.strip_prefix("error[")
            .and_then(|label| label.split_once(']'))
            .is_some_and(|(identity, _)| identity == "SEM057:call_marker_on_read_only")
    });
    assert!(
        has_kernel_contract_diagnostic,
        "file-at-a-time analysis loses the imported kernel call contract:\n{stderr}"
    );
}

/// Follow-up acceptance test for whole-package conversion. This currently
/// fails because `convert` calls the single-document analyzer for each file;
/// remove the ignore once package analysis and per-file re-emission are wired.
#[test]
#[ignore = "follow-up: convert package inputs using whole-package analysis"]
fn convert_multi_file_kernel_package_then_check_converted_package() {
    let root = tempfile::tempdir().expect("create two-file package root");
    let package = root.path().join("convert-kernel-package");
    write_convert_kernel_package(&package);
    let binary = faber_binary();

    let conversion = Command::new(&binary)
        .args(["convert", "--to", "la", "--write"])
        .arg(&package)
        .output()
        .expect("convert whole package");
    assert!(
        conversion.status.success(),
        "whole-package conversion must succeed:\n{}",
        String::from_utf8_lossy(&conversion.stderr)
    );
    let check = Command::new(&binary)
        .args(["check", package.to_str().expect("utf8 package path")])
        .output()
        .expect("check converted package");
    assert!(
        check.status.success(),
        "converted package must revalidate as a package:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );
}

fn write_convert_kernel_package(package: &Path) {
    let source_dir = package.join("src");
    fs::create_dir_all(&source_dir).expect("create package source directory");
    fs::write(
        package.join("faber.toml"),
        r#"[package]
name = "convert-kernel-package"
version = "0.1.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[locale]
locale = "en"
"#,
    )
    .expect("write package manifest");
    fs::write(
        source_dir.join("kernels.fab"),
        r#"@ kernel
@ public
fn fill(mut tensor<f32, [4]> output) → void {
    output[0] ← 1.0
}
"#,
    )
    .expect("write kernel module");
    fs::write(
        source_dir.join("main.fab"),
        r#"import from "./kernels" kernels
main {
    mut tensor<f32, [4]> output ← tensor<f32, [4]> {}
    do device {
        kernels.fill(mut output)
    }
}
"#,
    )
    .expect("write entry module");
}

#[test]
fn convert_generic_class_revalidates_constructor_fields() {
    let converted = convert_corpus_fixture_to_latin("generic/class.fab");
    assert!(converted.contains("redde Pair<B, A> { a = ego.b, b = ego.a }"));
    assert!(converted.contains("redde Box<T> { v = x }"));
}

#[test]
fn convert_implet_orderable_revalidates_in_latin() {
    let converted = convert_corpus_fixture_to_latin("implet/implet-orderable.fab");
    assert!(converted.contains("genus Person implet Orderable<Person>"));
    assert!(converted.contains("functio larger<T implet Orderable<T>>"));
    assert!(!converted.contains("def_4099"));
}

#[test]
fn convert_intervallum_print_revalidates_stepped_value_in_latin() {
    let converted = convert_corpus_fixture_to_latin("intervallum/print.fab");
    assert!(converted.contains("intervallum<numerus, 3> stepped"));
}

#[test]
fn convert_intervallum_stepped_value_revalidates_in_latin() {
    let converted = convert_corpus_fixture_to_latin("intervallum/stepped-value.fab");
    assert!(converted.contains("intervallum<numerus, 2> evens"));
}

fn convert_corpus_fixture_to_latin(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("corpus")
        .join(relative);
    let converted = run_faber_convert_stdout_with_args(&[
        "convert",
        "--to",
        "la",
        "--stdout",
        path.to_str().expect("utf8 corpus path"),
    ]);
    let root = tempfile::tempdir().expect("create converted fixture directory");
    let converted_path = root.path().join("converted.fab");
    fs::write(&converted_path, &converted).expect("write converted fixture");
    let checked = Command::new(faber_binary())
        .args(["check", "--no-diagnostics"])
        .arg(&converted_path)
        .output()
        .expect("check converted Latin fixture");
    assert!(
        checked.status.success(),
        "converted {relative} must revalidate as Latin:\n{}\n{converted}",
        String::from_utf8_lossy(&checked.stderr)
    );
    converted
}

#[test]
fn convert_resolves_imports_in_corpus_fixture() {
    let fixture = exempla("importa/default-minimal.fab");
    let mut child = Command::new(faber_binary())
        .args(["convert", "--to", "la", "--stdout"])
        .arg(&fixture)
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("corpus-product/reader-locale/th-TH");
    let thai = root.join("src/main.fab");
    let latin = root.join("twins/main.la.fab");

    let thai_output = run_faber_convert_stdout_with_args(&[
        "convert",
        "--to",
        "th-TH",
        "--stdout",
        thai.to_str().expect("utf8 thai path"),
    ]);

    // --to drives the emitter surface: the re-emit localizes
    // reader-locale keywords and types into Thai and no longer matches the
    // Latin twin (which explicitly reads and emits as `la`).
    assert_ne!(
        thai_output,
        run_faber_convert_stdout_with_args(&[
            "convert",
            "--from",
            "la",
            "--to",
            "la",
            "--stdout",
            latin.to_str().expect("utf8 latin path"),
        ]),
        "localized Thai emit must differ from the Latin twin"
    );

    assert!(thai_output.contains("ฟังก์ชัน ต้อนรับ(ข้อความ who) → ข้อความ"));
    assert!(thai_output.contains("กำหนด ข้อความ line ← \"ขอต้อนรับ, §!\"(who)"));
    assert!(thai_output.contains("คืน line"));
    assert!(thai_output.contains("ฟังก์ชัน สอบผ่าน(จํานวน mark) → ตรรกะ"));
    assert!(thai_output.contains("ถ้าไม่ก็ mark ≥ 40 ดังนั้น คืน จริง"));
    assert!(thai_output.contains("ฟังก์ชัน นับคะแนน(รายการ<จํานวน> marks) → จํานวน"));
    assert!(thai_output.contains("แก้ไขได้ จํานวน total ← 0"));
    assert!(thai_output.contains("วน จาก marks กำหนด mark"));
    assert!(thai_output.contains("ถ้า mark ≺ 0 {"));
    assert!(thai_output.contains("ไปต่อ"));
    assert!(thai_output.contains("หยุด"));
    assert!(thai_output.contains("ฟังก์ชัน ลดลง(จํานวน start) → จํานวน"));
    assert!(thai_output.contains("ขณะ current ≻ 0"));
    assert!(thai_output.contains("เริ่ม {"));
    assert!(thai_output.contains("กำหนด จํานวน mark ← 77"));
    assert!(thai_output.contains("กำหนด รายการ<จํานวน> marks ← [-1, 95, 30, 55, 61]"));
    assert!(thai_output.contains("บันทึก สอบผ่าน(mark)"));
    assert!(thai_output.contains("บันทึก นับคะแนน(marks)"));
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

/// Untagged product input defaults to English even when the conversion target
/// is Latin. The source has no frontmatter, so `--to la` must not become the
/// source reader pack.
#[test]
fn convert_untagged_english_stdin_to_latin_without_from() {
    let source =
        radix::module::driver::split_frontmatter(super::super::format_test::EN_TYPES_SOURCE)
            .expect("split English fixture")
            .body
            .to_owned();
    let session = super::super::format::format_session(Path::new("<stdin>"), None, &source)
        .expect("untagged stdin must resolve the product default");
    assert_eq!(
        session
            .config
            .locale_pack
            .as_ref()
            .map(|pack| pack.metadata.id.as_str()),
        Some("en")
    );
    let (stdout, stderr, success) = run_faber_convert_stdin("la", &source);
    assert!(success, "untagged English stdin must read as en: {stderr}");
    assert!(
        stdout.contains("locale = \"la\""),
        "output must declare la: {stdout}"
    );
    assert!(
        stdout.contains("functio pack(numerus n"),
        "target output must use the Latin surface:\n{stdout}"
    );
}

#[test]
fn convert_directory_resolves_each_source_locale_before_emitting_one_target() {
    let root = tempfile::tempdir().expect("create mixed-locale directory");
    let english = root.path().join("english.fab");
    let latin = root.path().join("latin.fab");
    let english_source =
        radix::module::driver::split_frontmatter(super::super::format_test::EN_TYPES_SOURCE)
            .expect("split English fixture")
            .body
            .to_owned();
    fs::write(&english, english_source).expect("write untagged English source");
    fs::write(&latin, super::super::format_test::VALID_BLOCK_SOURCE).expect("write Latin source");

    let output = Command::new(faber_binary())
        .args(["convert", "--to", "en", "--write"])
        .arg(root.path())
        .output()
        .expect("run mixed-locale directory conversion");
    assert!(
        output.status.success(),
        "mixed-locale directory conversion failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let english_output = fs::read_to_string(&english).expect("read English output");
    let latin_output = fs::read_to_string(&latin).expect("read Latin output");
    assert!(english_output.contains("locale = \"en\""));
    assert!(english_output.contains("fn pack(int n"));
    assert!(latin_output.contains("locale = \"en\""));
    assert!(latin_output.contains("fn gradus(int x)"));
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

#[test]
fn convert_rewrites_locale_preserves_other_frontmatter_and_reparses() {
    let fixture = std::env::temp_dir().join("faber-convert-frontmatter-preservation.fab");
    let source = super::super::format_test::VALID_BLOCK_SOURCE.replace(
        "locale = \"la\"\n",
        "locale = \"la\"\nsectio = \"smoke\"\ngroup = \"demo.entry\"\n",
    );
    fs::write(&fixture, source).expect("write metadata fixture");
    let output = run_faber_convert_stdout_with_args(&[
        "convert",
        "--to",
        "en",
        "--stdout",
        fixture.to_str().expect("utf8 fixture path"),
    ]);
    fs::write(&fixture, &output).expect("write converted metadata fixture");
    let reparsed = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);

    assert!(output.contains("locale = \"en\""));
    assert!(output.contains("sectio = \"smoke\""));
    assert!(output.contains("group = \"demo.entry\""));
    assert!(
        reparsed.contains("locale = \"en\"")
            && reparsed.contains("sectio = \"smoke\"")
            && reparsed.contains("group = \"demo.entry\""),
        "converted output must reparse with its target locale and retain metadata:\n{reparsed}"
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

const AUTHORED_TYPE_FORMS_SOURCE: &str = r#"+++
locale = "en"
+++
type Word = wrapping<u32>

fn forms() → int {
    let int scalar_long ← 1
    let u32 scalar_short ← 2
    let list<u32> list_long ← empty
    let list<u32> list_middle ← empty
    let lu32 list_short ← empty
    let tensor<float, [2, 3]> tensor_long ← empty
    let tf32[2, 3] tensor_short ← empty
    let Word alias_use ← 3
    let list<list<list<tensor<float, [2, 2]>>>> nested_long ← empty
    let list<list<list<tf32[2, 2]>>> nested_short ← empty
    return 0
}
"#;

#[test]
fn convert_preserves_authored_type_forms_en_to_la() {
    let fixture = std::env::temp_dir().join("faber-convert-authored-type-forms.fab");
    fs::write(&fixture, AUTHORED_TYPE_FORMS_SOURCE).expect("write authored-type fixture");
    let output = run_faber_convert_stdout_with_args(&[
        "convert",
        "--to",
        "la",
        "--stdout",
        fixture.to_str().expect("utf8 fixture path"),
    ]);
    fs::write(&fixture, &output).expect("rewrite target-locale fixture");
    let second = run_faber_convert_stdout_with_args(&[
        "convert",
        "--to",
        "la",
        "--stdout",
        fixture.to_str().expect("utf8 fixture path"),
    ]);
    let _ = fs::remove_file(&fixture);
    assert_eq!(
        second, output,
        "authored type conversion must be idempotent"
    );

    assert!(
        output.contains("sit numerus scalar_long"),
        "scalar L0 must stay long:\n{output}"
    );
    assert!(
        output.contains("sit u32 scalar_short"),
        "scalar L1 must stay short:\n{output}"
    );
    assert!(
        output.contains("sit lista<u32> list_long"),
        "list L0/L0 must stay long:\n{output}"
    );
    assert!(
        output.contains("sit lista<u32> list_middle"),
        "list L0/L1 must stay mixed:\n{output}"
    );
    assert!(
        output.contains("sit lu32 list_short"),
        "list L1/L1 must stay fused:\n{output}"
    );
    assert!(
        output.contains("sit tensor<fractus, [2, 3]> tensor_long"),
        "tensor L0 must stay long:\n{output}"
    );
    assert!(
        output.contains("sit tf32[2, 3] tensor_short"),
        "tensor L1 must stay fused:\n{output}"
    );
    assert!(
        output.contains("typus Word = modulus<u32>"),
        "alias body must stay authored:\n{output}"
    );
    assert!(
        output.contains("sit Word alias_use"),
        "alias use must not expand:\n{output}"
    );
    assert!(
        output.contains("sit lista<lista<lista<tensor<fractus, [2, 2]>>>> nested_long"),
        "nested long form must stay long:\n{output}"
    );
    assert!(
        output.contains("sit lista<lista<lista<tf32[2, 2]>>> nested_short"),
        "only the nested tensor node may stay fused:\n{output}"
    );
}

#[test]
fn convert_preserves_three_way_list_forms_en_to_la() {
    let fixture = std::env::temp_dir().join("faber-convert-three-way-list-forms.fab");
    fs::write(
        &fixture,
        r#"+++
locale = "en"
+++
fn lists() → int {
    let list<u32> long_form ← empty
    let list<u32> child_short ← empty
    let lu32 root_short ← empty
    return 0
}
"#,
    )
    .expect("write three-way list fixture");
    let output = run_faber_convert_stdout_with_args(&[
        "convert",
        "--to",
        "la",
        "--stdout",
        fixture.to_str().expect("utf8 fixture path"),
    ]);
    let _ = fs::remove_file(&fixture);

    assert!(
        output.contains("lista<u32> long_form"),
        "long list form changed:\n{output}"
    );
    assert!(
        output.contains("lista<u32> child_short"),
        "child-short list form changed:\n{output}"
    );
    assert!(
        output.contains("lu32 root_short"),
        "root-short list form changed:\n{output}"
    );
}

/// D11.8: the formatter keeps the author's spelling of the default policy —
/// `trapping<u8>` (la `exactus<u8>`), bare `u8`, and the long `int<u8>` are
/// one type but three spellings, and each survives a locale round trip.
#[test]
fn convert_preserves_trapping_policy_word_spelling() {
    let fixture = std::env::temp_dir().join("faber-convert-trapping-policy-word.fab");
    fs::write(
        &fixture,
        r#"+++
locale = "en"
+++
fn policy(trapping<u8> a) → trapping<i64> {
    let trapping<u8> spelled ← a
    let u8 bare ← a
    let u8 long_form ← a
    let list<trapping<u16>> inner ← empty
    return 0
}
"#,
    )
    .expect("write trapping fixture");
    let path = fixture.to_str().expect("utf8 fixture path");
    let latin = run_faber_convert_stdout_with_args(&["convert", "--to", "la", "--stdout", path]);
    assert!(
        latin.contains("functio policy(exactus<u8> a) → exactus<i64>"),
        "{latin}"
    );
    assert!(latin.contains("sit exactus<u8> spelled"), "{latin}");
    assert!(latin.contains("sit u8 bare"), "{latin}");
    assert!(latin.contains("sit u8 long_form"), "{latin}");
    assert!(latin.contains("sit lista<exactus<u16>> inner"), "{latin}");

    fs::write(&fixture, &latin).expect("rewrite latin fixture");
    let english = run_faber_convert_stdout_with_args(&["convert", "--to", "en", "--stdout", path]);
    let _ = fs::remove_file(&fixture);
    assert!(
        english.contains("fn policy(trapping<u8> a) → trapping<i64>"),
        "{english}"
    );
    assert!(english.contains("let trapping<u8> spelled"), "{english}");
    assert!(english.contains("let u8 bare"), "{english}");
    assert!(english.contains("let u8 long_form"), "{english}");
    assert!(
        english.contains("let list<trapping<u16>> inner"),
        "{english}"
    );
}

/// D11.9: a convert hint is a `via` clause, in every locale's own word.
const VIA_HINT_SOURCE: &str = r#"+++
locale = "en"
+++
main {
    let list<b8> raw ← |3c 00 00 00|
    let char big ← 'A'
    let u16 a ← raw[0‥2] ↦ u16 via Le
    let f16 b ← a ↦ b16 ↦ f16
    let int c ← "ff" ↦ i32 via Hex ⊥ 0
    let u32 d ← big ↦ u32 via Code
    let char e ← 65 ↦ char via Code ⊥ '?'
    let int via ← c
    print via
}
"#;

#[test]
fn convert_keeps_hints_in_the_via_clause() {
    let fixture = std::env::temp_dir().join("faber-convert-via-hint-rewrite.fab");
    fs::write(&fixture, VIA_HINT_SOURCE).expect("write via fixture");
    let path = fixture.to_str().expect("utf8 fixture path");
    let formatted = run_faber_convert_stdout_with_args(&["format", "--stdout", path]);
    let latin = run_faber_convert_stdout_with_args(&["convert", "--to", "la", "--stdout", path]);
    let _ = fs::remove_file(&fixture);

    for (needle, what) in [
        ("↦ u16 via Le", "the endian read keeps its clause"),
        ("(a ↦ b16) ↦ f16", "the pattern chain stays"),
        ("↦ i32 via Hex ⊥ 0", "the clause binds before the default"),
        ("↦ u32 via Code", "the code-point read"),
        (
            "↦ char via Code ⊥ '?'",
            "the code-point build keeps its clause",
        ),
    ] {
        assert!(formatted.contains(needle), "{what}:\n{formatted}");
    }
    assert!(
        !formatted.contains(", Le>")
            && !formatted.contains(", Hex>")
            && !formatted.contains("<Code>"),
        "no hint stays a type argument:\n{formatted}"
    );
    assert!(
        formatted.contains("let int via ← c") && formatted.contains("print via"),
        "`via` stays an ordinary name:\n{formatted}"
    );
    assert!(latin.contains("↦ u16 via Le"), "{latin}");
    assert!(latin.contains("↦ littera via Code ⊥ '?'"), "{latin}");
}

#[test]
fn format_of_the_via_clause_is_a_fixpoint() {
    let fixture = std::env::temp_dir().join("faber-format-via-hint-fixpoint.fab");
    fs::write(&fixture, VIA_HINT_SOURCE).expect("write via fixture");
    let path = fixture.to_str().expect("utf8 fixture path");
    let once = run_faber_convert_stdout_with_args(&["format", "--stdout", path]);
    fs::write(&fixture, &once).expect("rewrite formatted fixture");
    let twice = run_faber_convert_stdout_with_args(&["format", "--stdout", path]);
    let _ = fs::remove_file(&fixture);
    assert_eq!(once, twice, "parse → emit → parse is a fixpoint");
}

#[test]
fn convert_spells_the_via_clause_in_each_pack_and_reads_it_back() {
    let fixture = std::env::temp_dir().join("faber-convert-via-hint-packs.fab");
    fs::write(&fixture, VIA_HINT_SOURCE).expect("write via fixture");
    let path = fixture.to_str().expect("utf8 fixture path");
    let latin = run_faber_convert_stdout_with_args(&["convert", "--to", "la", "--stdout", path]);
    for (locale, word) in [
        ("en", "via"),
        ("ar", "عبر"),
        ("hi", "द्वारा"),
        ("th-TH", "ผ่านทาง"),
        ("vi", "thông_qua"),
        ("zh-Hans", "经由"),
        ("zh-Hant", "經由"),
    ] {
        let localized =
            run_faber_convert_stdout_with_args(&["convert", "--to", locale, "--stdout", path]);
        assert!(
            localized.contains(&format!(" {word} Le"))
                && localized.contains(&format!(" {word} Hex")),
            "{locale} must spell the clause `{word}`:\n{localized}"
        );
        // Re-reading the localized source and converting back to Latin must
        // give the original Latin output: the clause word round-trips.
        fs::write(&fixture, &localized).expect("write localized fixture");
        let back = run_faber_convert_stdout_with_args(&["convert", "--to", "la", "--stdout", path]);
        assert_eq!(back, latin, "{locale} → la must round-trip the via clause");
    }
    let _ = fs::remove_file(&fixture);
}

#[test]
fn convert_rejects_the_retired_type_argument_hint_pointing_at_the_clause() {
    // The wrapped numeric head is rejected whole (N7d), so its hint spelling
    // is the wrapper diagnostic; the `char` head keeps the hint diagnostic.
    for (source, issue) in [
        (
            "main {\n    let int c ← \"ff\" ↦ int<i32, Hex> ⊥ 0\n}\n",
            "numeric_wrapper_retired",
        ),
        (
            "main {\n    let char e ← 65 ↦ char<Code> ⊥ '?'\n}\n",
            "conversio_hint_type_argument",
        ),
    ] {
        let (stdout, stderr, success) = run_faber_convert_stdin("la", source);
        assert!(!success, "the retired spelling must not convert:\n{stdout}");
        assert!(
            stderr.contains(issue),
            "the rejection names the retired form:\n{stderr}"
        );
    }
}

#[test]
fn convert_preserves_template_application_sugar() {
    // Reader-locale packs re-render the same semantic program with different
    // keyword spellings while retaining glyph shapes. The `"…"(args)` template-
    // application postfix is a glyph shape: `--to en` localizes
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
    // `convert --to <X>` selects the HIR-backed re-emit path with the
    // localized surface.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("corpus-product/reader-locale/th-TH");
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
        "--to th-TH must emit the Thai surface: {stdout}"
    );
    assert!(
        !code_only(&stdout).contains("functio"),
        "--to th-TH must not emit the Latin keyword: {stdout}"
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
    let canonical_en = fs::read_to_string(source_dir.join("main.fab")).expect("read canonical en");
    assert!(
        canonical_en.contains("@ conversion\nfn from_source_error(SourceError err) → DestError {"),
        "canonical en emit must keep the named conversion fn with its marker:\n{canonical_en}"
    );
    assert!(
        canonical_en.contains("return variant Wrapped { payload = detail }"),
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
        la_source.contains("redde finge Wrapped { payload = detail }"),
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

@ conversion
fn from_source_error(SourceError err) → DestError {
    match err {
        case SourceError.Malformed let detail { return variant Wrapped {payload = detail} }
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
            "--write",
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

/// FD-1: the refusal names the lost construct by code, issue and args.
#[test]
fn convert_construct_loss_diagnostic_carries_code_issue_and_construct() {
    let path = Path::new("lossy.fab");
    let session = super::super::format::format_session(path, None, HOOK_SOURCE)
        .expect("hook fixture must resolve a session");
    let emit_pack = faber_cli::package::embedded_conversion_locale_pack("la").expect("la pack");
    let result =
        radix::module::driver::reemit_faber(&session, &emit_pack, "lossy.fab", HOOK_SOURCE);
    assert!(result.output.is_none(), "a lossy conversion writes nothing");
    let refusal = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.issue() == Some("ast_construct_dropped"))
        .expect("the refusal must carry the ast_construct_dropped issue");
    assert_eq!(
        refusal.lookup_key().as_deref(),
        Some("CODEGEN006:ast_construct_dropped")
    );
    let construct = refusal
        .args
        .iter()
        .find(|arg| arg.name == "construct")
        .map(|arg| arg.value.as_str());
    assert!(
        construct.is_some_and(|value| value.starts_with("hook:Praepara")),
        "the construct arg must name the dropped hook: {construct:?}"
    );
}

/// FD-1: `convert` refuses rather than deleting a construct HIR cannot carry.
#[test]
fn convert_refuses_construct_loss() {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("lossy.fab");
    let path_arg = path.to_str().expect("utf8 path");
    fs::write(&path, HOOK_SOURCE).expect("write fixture");

    for args in [
        vec!["convert", "--to", "la", path_arg],
        vec!["convert", "--to", "la", "--write", path_arg],
        vec!["convert", "--to", "la", "--stdout", path_arg],
    ] {
        let output = run_faber_capture(&args, None);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            !output.status.success(),
            "{args:?} must exit non-zero:\n{stderr}"
        );
        assert!(
            !stderr.is_empty(),
            "{args:?} must report the refusal on stderr"
        );
        if args.contains(&"--stdout") {
            assert!(stdout.is_empty(), "{args:?} must print nothing to stdout");
        } else {
            // The summary is the only stdout line: no file was converted.
            assert_eq!(stdout.lines().count(), 1, "{args:?}: {stdout}");
            assert!(
                stdout.contains("1 refused (nothing written)"),
                "{args:?}: {stdout}"
            );
        }
    }
    assert_eq!(
        fs::read_to_string(&path).expect("read back"),
        HOOK_SOURCE,
        "the file must stay byte-identical"
    );
}

/// Source whose reader locale is `la` (frontmatter), so `--to en` drifts.
const LATIN_DRIFT: &str = super::super::format_test::VALID_BLOCK_SOURCE;

fn run_convert(args: &[&str]) -> std::process::Output {
    let mut full = vec!["convert"];
    full.extend_from_slice(args);
    run_faber_capture(&full, None)
}

/// A file already in the `en` surface: the converter's own output.
fn already_converted(dir: &Path) -> (std::path::PathBuf, String) {
    let path = dir.join("done.fab");
    fs::write(&path, LATIN_DRIFT).expect("write fixture");
    let output = run_convert(&["--to", "en", "--stdout", path.to_str().expect("utf8 path")]);
    assert!(output.status.success(), "{output:?}");
    let converted = String::from_utf8(output.stdout).expect("utf8 stdout");
    fs::write(&path, &converted).expect("write converted fixture");
    (path, converted)
}

#[test]
fn cli_convert_default_mode_is_report_only() {
    use clap::Parser;
    let cli = crate::cli::Cli::try_parse_from(["faber", "convert", "--to", "en", "main.fab"])
        .expect("parse convert");
    let Some(crate::cli::Command::Convert(args)) = cli.command else {
        panic!("expected convert command");
    };
    assert!(!args.write && !args.check && !args.stdout && !args.stdin);
}

#[test]
fn cli_convert_modes_conflict() {
    use clap::Parser;
    for pair in [
        ["--write", "--check"],
        ["--write", "--stdout"],
        ["--check", "--stdout"],
        ["--stdin", "--write"],
        ["--stdin", "--stdout"],
    ] {
        let result =
            crate::cli::Cli::try_parse_from(["faber", "convert", "--to", "en", pair[0], pair[1]]);
        assert!(result.is_err(), "{pair:?} must conflict");
    }
}

#[test]
fn convert_default_writes_nothing_and_exits_1_on_drift() {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("drift.fab");
    fs::write(&path, LATIN_DRIFT).expect("write fixture");
    let (_, _) = already_converted(dir.path());
    let root = dir.path().to_str().expect("utf8 path");

    for args in [
        vec!["--to", "en", root],
        vec!["--to", "en", "--check", root],
    ] {
        let output = run_convert(&args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {stdout}");
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines.len(), 2, "{args:?}: {stdout}");
        assert!(lines[0].starts_with("would convert "), "{stdout}");
        assert!(lines[0].ends_with("drift.fab"), "{stdout}");
        assert!(
            lines[1].starts_with("1 of 2 files would be converted; rerun with --write to apply"),
            "{stdout}"
        );
    }
    assert_eq!(fs::read_to_string(&path).expect("read back"), LATIN_DRIFT);
}

#[test]
fn convert_default_exits_0_when_nothing_would_change() {
    let dir = tempfile::tempdir().expect("create scratch");
    let (path, source) = already_converted(dir.path());
    let output = run_convert(&["--to", "en", path.to_str().expect("utf8 path")]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        "1 files already converted"
    );
    assert_eq!(fs::read_to_string(&path).expect("read back"), source);
}

#[test]
fn convert_write_changes_only_files_that_drift() {
    let dir = tempfile::tempdir().expect("create scratch");
    let drift = dir.path().join("drift.fab");
    fs::write(&drift, LATIN_DRIFT).expect("write fixture");
    let (clean, clean_source) = already_converted(dir.path());
    let clean_mtime = fs::metadata(&clean)
        .and_then(|meta| meta.modified())
        .expect("clean mtime");

    let output = run_convert(&["--to", "en", "--write", dir.path().to_str().expect("utf8")]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{stdout}");
    assert!(lines[0].starts_with("converted ") && lines[0].ends_with("drift.fab"));
    assert_eq!(lines[1], "converted 1 of 2 files");
    assert_ne!(fs::read_to_string(&drift).expect("read drift"), LATIN_DRIFT);
    assert_eq!(
        fs::read_to_string(&clean).expect("read clean"),
        clean_source
    );
    assert_eq!(
        fs::metadata(&clean)
            .and_then(|meta| meta.modified())
            .expect("clean mtime"),
        clean_mtime,
        "an unchanged file must not be touched"
    );
}

#[test]
fn convert_counts_refused_files_and_never_writes_them() {
    let dir = tempfile::tempdir().expect("create scratch");
    let lossy = dir.path().join("lossy.fab");
    fs::write(&lossy, HOOK_SOURCE).expect("write lossy fixture");
    let drift = dir.path().join("drift.fab");
    fs::write(&drift, LATIN_DRIFT).expect("write fixture");
    let root = dir.path().to_str().expect("utf8 path");

    let report = run_convert(&["--to", "en", root]);
    let report_out = String::from_utf8_lossy(&report.stdout);
    assert_eq!(report.status.code(), Some(65), "{report_out}");
    assert!(
        report_out
            .lines()
            .last()
            .is_some_and(|line| line.ends_with(", 1 refused (nothing written)")),
        "{report_out}"
    );
    assert_eq!(fs::read_to_string(&drift).expect("read drift"), LATIN_DRIFT);

    let write = run_convert(&["--to", "en", "--write", root]);
    let write_out = String::from_utf8_lossy(&write.stdout);
    assert_eq!(write.status.code(), Some(65), "{write_out}");
    assert_eq!(
        write_out.lines().last(),
        Some("converted 1 of 2 files, 1 refused (nothing written)")
    );
    assert_eq!(
        fs::read_to_string(&lossy).expect("read lossy"),
        HOOK_SOURCE,
        "a refused file is never written"
    );
}
