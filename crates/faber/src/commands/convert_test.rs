//! Integration tests for `faber convert` (reader-locale source conversion).
//! Ported from the former reader-locale conversion surface; see
//! docs/operator/convert-command.md.

use super::super::format_test::{
    code_only, exempla, faber_binary, run_faber_format_stdout,
    run_faber_format_stdout_with_args as run_faber_convert_stdout_with_args,
};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
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

    assert!(thai_output.contains("ฟังก์ชัน ทักทาย(ข้อความ name) → ข้อความ"));
    assert!(thai_output.contains("คงที่ ข้อความ greeting ← \"สวัสดี, §!\"(name)"));
    assert!(thai_output.contains("คืน greeting"));
    assert!(thai_output.contains("ฟังก์ชัน ผ่าน(จํานวน score) → ตรรกะ"));
    assert!(thai_output.contains("ถ้าไม่ก็ score ≥ 50 ดังนั้น คืน จริง"));
    assert!(thai_output.contains("ฟังก์ชัน นับผ่าน(รายการ<จํานวน> scores) → จํานวน"));
    assert!(thai_output.contains("แปร จํานวน total ← 0"));
    assert!(thai_output.contains("วน จาก scores คงที่ score"));
    assert!(thai_output.contains("ถ้า score ≺ 0 {"));
    assert!(thai_output.contains("ไปต่อ"));
    assert!(thai_output.contains("หยุด"));
    assert!(thai_output.contains("ฟังก์ชัน นับถอยหลัง(จํานวน start) → จํานวน"));
    assert!(thai_output.contains("ขณะ current ≻ 0"));
    assert!(thai_output.contains("เริ่ม {"));
    assert!(thai_output.contains("คงที่ จํานวน score ← 82"));
    assert!(thai_output.contains("คงที่ รายการ<จํานวน> scores ← [-1, 82, 41, 60]"));
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
        .args(["convert", "--to", "en"])
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
    const int scalar_long ← 1
    const u32 scalar_short ← 2
    const list<int<u32>> list_long ← empty
    const list<u32> list_middle ← empty
    const lu32 list_short ← empty
    const tensor<float, [2, 3]> tensor_long ← empty
    const tf32[2, 3] tensor_short ← empty
    const Word alias_use ← 3
    const list<list<list<tensor<float, [2, 2]>>>> nested_long ← empty
    const list<list<list<tf32[2, 2]>>> nested_short ← empty
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
        output.contains("fixum numerus scalar_long"),
        "scalar L0 must stay long:\n{output}"
    );
    assert!(
        output.contains("fixum u32 scalar_short"),
        "scalar L1 must stay short:\n{output}"
    );
    assert!(
        output.contains("fixum lista<numerus<u32>> list_long"),
        "list L0/L0 must stay long:\n{output}"
    );
    assert!(
        output.contains("fixum lista<u32> list_middle"),
        "list L0/L1 must stay mixed:\n{output}"
    );
    assert!(
        output.contains("fixum lu32 list_short"),
        "list L1/L1 must stay fused:\n{output}"
    );
    assert!(
        output.contains("fixum tensor<fractus, [2, 3]> tensor_long"),
        "tensor L0 must stay long:\n{output}"
    );
    assert!(
        output.contains("fixum tf32[2, 3] tensor_short"),
        "tensor L1 must stay fused:\n{output}"
    );
    assert!(
        output.contains("typus Word = modulus<u32>"),
        "alias body must stay authored:\n{output}"
    );
    assert!(
        output.contains("fixum Word alias_use"),
        "alias use must not expand:\n{output}"
    );
    assert!(
        output.contains("fixum lista<lista<lista<tensor<fractus, [2, 2]>>>> nested_long"),
        "nested long form must stay long:\n{output}"
    );
    assert!(
        output.contains("fixum lista<lista<lista<tf32[2, 2]>>> nested_short"),
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
    const list<int<u32>> long_form ← empty
    const list<u32> child_short ← empty
    const lu32 root_short ← empty
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
        output.contains("lista<numerus<u32>> long_form"),
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
    const trapping<u8> spelled ← a
    const u8 bare ← a
    const int<u8> long_form ← a
    const list<trapping<u16>> inner ← empty
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
    assert!(latin.contains("fixum exactus<u8> spelled"), "{latin}");
    assert!(latin.contains("fixum u8 bare"), "{latin}");
    assert!(latin.contains("fixum numerus<u8> long_form"), "{latin}");
    assert!(latin.contains("fixum lista<exactus<u16>> inner"), "{latin}");

    fs::write(&fixture, &latin).expect("rewrite latin fixture");
    let english = run_faber_convert_stdout_with_args(&["convert", "--to", "en", "--stdout", path]);
    let _ = fs::remove_file(&fixture);
    assert!(
        english.contains("fn policy(trapping<u8> a) → trapping<i64>"),
        "{english}"
    );
    assert!(english.contains("const trapping<u8> spelled"), "{english}");
    assert!(english.contains("const u8 bare"), "{english}");
    assert!(english.contains("const int<u8> long_form"), "{english}");
    assert!(
        english.contains("const list<trapping<u16>> inner"),
        "{english}"
    );
}

/// D11.9: a convert hint is a `via` clause, in every locale's own word.
const VIA_HINT_SOURCE: &str = r#"+++
locale = "en"
+++
main {
    const bytes raw ← |3c 00 00 00|
    const char big ← 'A'
    const int<u16> a ← raw[0‥2] ↦ int<u16> via Le
    const f16 b ← a ↦ f16 via Bits
    const int c ← "ff" ↦ int<i32> via Hex ⊥ 0
    const int<u32> d ← big ↦ int<u32> via Code
    const char e ← 65 ↦ char via Code ⊥ '?'
    const int via ← c
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
        ("↦ int<u16> via Le", "the endian read keeps its clause"),
        ("↦ f16 via Bits", "the bitcast keeps its clause"),
        (
            "↦ int<i32> via Hex ⊥ 0",
            "the clause binds before the default",
        ),
        ("↦ int<u32> via Code", "the code-point read"),
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
        formatted.contains("const int via ← c") && formatted.contains("print via"),
        "`via` stays an ordinary name:\n{formatted}"
    );
    assert!(latin.contains("↦ numerus<u16> via Le"), "{latin}");
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
                && localized.contains(&format!(" {word} Bits")),
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
    for source in [
        "main {\n    const int c ← \"ff\" ↦ int<i32, Hex> ⊥ 0\n}\n",
        "main {\n    const char e ← 65 ↦ char<Code> ⊥ '?'\n}\n",
    ] {
        let (stdout, stderr, success) = run_faber_convert_stdin("la", source);
        assert!(!success, "the retired spelling must not convert:\n{stdout}");
        assert!(
            stderr.contains("conversio_hint_type_argument"),
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
