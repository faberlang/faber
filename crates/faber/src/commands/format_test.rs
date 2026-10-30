//! `faber format` canonical pipeline: HIR re-emit at the source's own locale.

use super::format::{
    format_session, formatted_source_for_write, normalize_trailing_newline, source_for_compare,
};
use radix::module::driver::faber_format::test_gate::{
    assert_canonical_idempotent_with_session, assert_canonical_reparses_with,
    canonical_format_once_with_session,
};
use radix::module::driver::reemit_faber;
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
    let path = radix::module::locale::workspace_locale_pack_path("en");
    LocalePack::from_toml_path(path).expect("load English locale pack")
}

fn latin_pack() -> LocalePack {
    let path = radix::module::locale::workspace_locale_pack_path("la");
    LocalePack::from_toml_path(path).expect("load Latin locale pack")
}

/// Format a real file through the exact CLI canonical pipeline.
fn canonical_cli_pipeline(path: &Path) -> String {
    let source = fs::read_to_string(path).expect("read source");
    let session = format_session(path, None, &source).expect("format session");
    let name = path.display().to_string();
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("format emission locale pack"),
        &name,
        &source,
    );
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

/// Comparison is the glyph set `≺ ≻ ≤ ≥`; the glyphs round-trip through the
/// canonical format unchanged.
#[test]
fn canonical_format_keeps_comparison_glyphs() {
    let dir = std::env::temp_dir().join("faber-format-canonical-cmp");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch");
    let path = dir.join("proba.fab");
    let source = r#"+++
locale = "en"
+++
class Proba {
    mut float x
    mut float y
    fn compara(float other) → bool {
        return self.x ≺ other and self.x ≻ other and self.x ≤ other and self.x ≥ other
    }
}
"#;
    fs::write(&path, source).expect("write");
    let formatted = canonical_cli_pipeline(&path);
    let _ = fs::remove_dir_all(&dir);
    assert!(formatted.contains("self.x ≺ other"), "{formatted}");
    assert!(formatted.contains("self.x ≻ other"), "{formatted}");
    assert!(formatted.contains("self.x ≤ other"), "{formatted}");
    assert!(formatted.contains("self.x ≥ other"), "{formatted}");
}

/// ASCII `<`/`>` as comparison is a retired spelling (PARSE030): the canonical
/// format refuses it with the named diagnostic and names the glyph; it does
/// not migrate the source.
#[test]
fn canonical_format_refuses_ascii_comparison() {
    let dir = std::env::temp_dir().join("faber-format-canonical-ascii-cmp");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch");
    let path = dir.join("proba.fab");
    let source = r#"+++
locale = "en"
+++
class Proba {
    mut float x
    mut float y
    fn compara(float other) → bool {
        return self.x < other and self.x > other
    }
}
"#;
    fs::write(&path, source).expect("write");
    let session = format_session(&path, None, source).expect("format session");
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("format emission locale pack"),
        &path.display().to_string(),
        source,
    );
    let _ = fs::remove_dir_all(&dir);
    assert!(!result.success(), "ASCII comparison must be refused");
    let glyphs: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some("PARSE030"))
        .filter_map(|d| {
            d.args
                .iter()
                .find(|arg| arg.name == "glyph")
                .map(|arg| arg.value.clone())
        })
        .collect();
    assert_eq!(glyphs, vec!["≺".to_string(), "≻".to_string()]);
}

/// `≢` is the one negation of `≡` (ruling 6b5c1c615): it round-trips through
/// the canonical format unchanged.
#[test]
fn canonical_format_prints_the_negation_glyph() {
    let dir = std::env::temp_dir().join("faber-format-canonical-neq");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch");
    let path = dir.join("proba.fab");
    let source = r#"+++
locale = "en"
+++
class Proba {
    mut float x
    fn differs(float other) → bool {
        return self.x ≢ other
    }
}
"#;
    fs::write(&path, source).expect("write");
    let formatted = canonical_cli_pipeline(&path);
    let _ = fs::remove_dir_all(&dir);
    assert!(formatted.contains("self.x ≢ other"), "{formatted}");
    assert!(!formatted.contains('≠'), "{formatted}");
}

/// The retired `≠` (U+2260) is refused with the named diagnostic and the
/// canonical format does not migrate the source.
#[test]
fn canonical_format_refuses_retired_inequality_glyph() {
    let dir = std::env::temp_dir().join("faber-format-canonical-neq-retired");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch");
    let path = dir.join("proba.fab");
    let source = r#"+++
locale = "en"
+++
class Proba {
    mut float x
    fn differs(float other) → bool {
        return self.x ≠ other
    }
}
"#;
    fs::write(&path, source).expect("write");
    let session = format_session(&path, None, source).expect("format session");
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("format emission locale pack"),
        &path.display().to_string(),
        source,
    );
    let _ = fs::remove_dir_all(&dir);
    assert!(
        !result.success(),
        "retired inequality glyph must be refused"
    );
    let replacements: Vec<String> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some("PARSE030"))
        .filter_map(|d| {
            d.args
                .iter()
                .find(|arg| arg.name == "replacement")
                .map(|arg| arg.value.clone())
        })
        .collect();
    assert_eq!(replacements, vec!["≢".to_string()]);
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
fn format_test_gate_matches_reemit_pipeline_for_salve() {
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

/// The freshly built `faber` binary that CLI-spawning tests run.
///
/// Integration-test targets get `CARGO_BIN_EXE_faber` from cargo, which builds
/// the binary before they run. Unit tests (lib and bin targets) do not, and a
/// plain `cargo test` never rebuilds `target/<profile>/faber` for them, so a
/// stale artifact would green or red these tests without testing the code
/// under change. Build it once per test process instead; a failed build fails
/// the test loudly, it never falls back to whatever binary is on disk.
pub(crate) fn faber_binary() -> PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_faber") {
        return PathBuf::from(path);
    }
    static BUILT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    BUILT.get_or_init(build_faber_binary).clone()
}

fn build_faber_binary() -> PathBuf {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut build = Command::new(cargo);
    build
        .args(["build", "--quiet", "-p", "faber", "--bin", "faber"])
        .current_dir(env!("CARGO_MANIFEST_DIR"));
    if !cfg!(debug_assertions) {
        build.arg("--release");
    }
    let status = build
        .status()
        .expect("spawn `cargo build -p faber --bin faber`");
    assert!(
        status.success(),
        "`cargo build -p faber --bin faber` failed; CLI-spawning tests need a fresh binary"
    );

    // The binary sits beside `deps/` of the running test executable (this
    // honors CARGO_TARGET_DIR and shared-target layouts).
    let exe = std::env::current_exe().expect("current test executable");
    let profile_dir = exe
        .parent()
        .and_then(|deps| deps.parent())
        .expect("test executable lives under <target>/<profile>/deps");
    let binary = profile_dir.join(if cfg!(windows) { "faber.exe" } else { "faber" });
    assert!(
        binary.is_file(),
        "built faber binary not found at {}",
        binary.display()
    );
    binary
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

pub(crate) const EN_TYPES_SOURCE: &str = "+++\nlocale = \"en\"\n+++\nfn pack(int n, float x, string name, list<b8> buf) → string {\n    let int count ← n\n    let float scale ← x\n    let string label ← name\n    let list<b8> payload ← buf\n    print count\n    print scale\n    print label\n    print payload.length()\n    return label\n}\n";

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
            && body.contains("list<b8>"),
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
        body.contains("fn pack(int n, float x, string name, list<b8> buf)"),
        "en stdin format must keep the English surface:\n{stdout}"
    );
    assert!(
        !body.contains("functio pack(numerus n"),
        "en stdin format must not relatinize the source:\n{stdout}"
    );
    assert_canonical_reparses_with(body, "en types stdin", Some(&english_pack())).expect("reparse");
}

/// Need: the named-tuple loss probe, verbatim. Labels are an
/// identity-erased alias layer, so canonical re-emission used to rewrite
/// `tuple<gx: int, gw: int>` to `tuple<int, int>` and `pair.gw` to `pair[1]`.
const NAMED_TUPLE_LABEL_PROBE: &str = "+++\nlocale = \"en\"\n+++\nmain {\n    let tuple<gx: int, gw: int> pair ← tuple<gx: int, gw: int> [1, 2]\n    print pair.gw\n}\n";

/// The live Gradus shape the loss was first found in
/// (`gradus/src/gradient.fab`): generic shape parameters, tensor element
/// types, a labeled tuple binding, and a by-label read. The genus field and
/// the generic return type also cover the named aggregate positions.
const GRADUS_SHAPED_TUPLE_LABEL_PROBE: &str = "+++\nlocale = \"en\"\n+++\nclass GradPack<size R, size C> {\n    mut tuple<grad_x: tensor<f32, [R, C]>, grad_w: tensor<f32, [R, C]>> grads\n}\n\nfn backward<size R, size C>(tensor<f32, [R, C]> x, tensor<f32, [R, C]> w) → tuple<grad_x: tensor<f32, [R, C]>, grad_w: tensor<f32, [R, C]>> {\n    let tuple<grad_x: tensor<f32, [R, C]>, grad_w: tensor<f32, [R, C]>> grads ← tuple<grad_x: tensor<f32, [R, C]>, grad_w: tensor<f32, [R, C]>> [x, w]\n    return grads\n}\n\nfn pick_w<size R, size C>(tensor<f32, [R, C]> x, tensor<f32, [R, C]> w) → tensor<f32, [R, C]> {\n    let tuple<grad_x: tensor<f32, [R, C]>, grad_w: tensor<f32, [R, C]>> grads ← tuple<grad_x: tensor<f32, [R, C]>, grad_w: tensor<f32, [R, C]>> [x, w]\n    return grads.grad_w\n}\n";

/// A source that already is canonical output must format to itself — for the
/// named-tuple probes that is exactly the labels-and-label-access assertion.
fn assert_format_fixed_point(source: &str, what: &str) -> String {
    let (stdout, stderr, success) = run_faber_format_stdin(source);
    assert!(success, "faber format --stdin must accept {what}: {stderr}");
    assert_eq!(
        stdout, source,
        "{what} must be a formatting fixed point:\n{stdout}"
    );
    stdout
}

#[test]
fn format_stdin_preserves_named_tuple_labels_and_label_access() {
    let formatted = assert_format_fixed_point(NAMED_TUPLE_LABEL_PROBE, "the named-tuple probe");
    for needle in ["tuple<gx: int, gw: int>", "pair.gw"] {
        assert!(
            formatted.contains(needle),
            "named-tuple format must retain {needle:?}:\n{formatted}"
        );
    }
    assert!(
        !formatted.contains("pair[1]"),
        "named-tuple format must not rewrite the label read to an index:\n{formatted}"
    );
}

#[test]
fn format_stdin_preserves_gradus_shaped_generic_tuple_labels() {
    let formatted =
        assert_format_fixed_point(GRADUS_SHAPED_TUPLE_LABEL_PROBE, "the Gradus-shaped probe");
    for needle in [
        // Genus field annotation and local binding annotation.
        "tuple<grad_x: tensor<f32, [R, C]>, grad_w: tensor<f32, [R, C]>> grads",
        // By-label read.
        "grads.grad_w",
    ] {
        assert!(
            formatted.contains(needle),
            "Gradus-shaped format must retain {needle:?}:\n{formatted}"
        );
    }
    assert!(
        !formatted.contains("grads[1]"),
        "Gradus-shaped format must not rewrite the label read to an index:\n{formatted}"
    );
}

/// Comments inside expression-owned blocks (closure, comptime, trap bodies)
/// survive `faber format`. Closure and trap blocks are legal as an
/// initializer, a call argument and an expression statement. A comptime block
/// is legal only as a constant initializer (SEM064 elsewhere), so its three
/// legal initializer sites are a module constant, a block-level constant and a
/// class field default. Each source is canonical, so it must format to itself.
#[test]
fn format_stdin_preserves_comments_in_expression_owned_blocks() {
    const FRONT: &str = "+++\nlocale = \"la\"\n+++\n";
    const BODIES: [(&str, &str, &str); 3] = [
        // (kind, block-opening head, body statement)
        ("closure", "clausura → vacuum {", "tacet"),
        ("trap", "capta {", "1"),
        ("comptime", "praefixum {", "redde 1"),
    ];
    let mut cases: Vec<(String, String)> = Vec::new();
    for (kind, head, stmt) in BODIES {
        let block = |indent: &str| {
            format!(
                "{head}\n{indent}    # lead\n{indent}    {stmt}\n{indent}    # tail\n{indent}}}"
            )
        };
        if kind == "comptime" {
            cases.push((
                format!("{kind} module constant"),
                format!("{FRONT}fixum numerus x = {}\n", block("")),
            ));
            cases.push((
                format!("{kind} block-level constant"),
                format!(
                    "{FRONT}functio f() → numerus {{\n    fixum numerus x = {}\n    redde x\n}}\n",
                    block("    ")
                ),
            ));
            cases.push((
                format!("{kind} class field default"),
                format!(
                    "{FRONT}genus G {{\n    sit numerus x = {}\n}}\n",
                    block("    ")
                ),
            ));
            continue;
        }
        cases.push((
            format!("{kind} initializer"),
            format!(
                "{FRONT}functio f() → vacuum {{\n    sit ∪ g ← {}\n}}\n",
                block("    ")
            ),
        ));
        cases.push((
            format!("{kind} call argument"),
            format!(
                "{FRONT}functio h(∪ g) → vacuum {{\n    tacet\n}}\n\nfunctio f() → vacuum {{\n    h({})\n}}\n",
                block("    ")
            ),
        ));
        cases.push((
            format!("{kind} expression statement"),
            format!(
                "{FRONT}functio f() → vacuum {{\n    {}\n}}\n",
                block("    ")
            ),
        ));
    }
    assert_eq!(cases.len(), 9);
    for (what, source) in &cases {
        let formatted = assert_format_fixed_point(source, what);
        for comment in ["# lead", "# tail"] {
            assert!(
                formatted.contains(comment),
                "{what} must keep {comment:?}:\n{formatted}"
            );
        }
    }
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
        stderr.contains("use --check or --write for multiple files")
            && !stderr.contains("in-place"),
        "the rejection must point at --check/--write, not an in-place default: {stderr}"
    );
    assert!(
        !stderr.contains("==="),
        "no separator output may survive the tightening: {stderr}"
    );
}

/// `--write` writes the formatted source back to the file, byte-identical to
/// what `--stdout` would print for the same file.
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

// ── FD-1: format refuses to drop source constructs ────────────────────────

/// A suite hook (`setup`) that HIR does not carry.
pub(crate) const HOOK_SOURCE: &str = "+++\nlocale = \"en\"\n+++\ndescribe \"hooks\" {\n    setup {\n        print \"before each\"\n    }\n    test \"sample\" {\n        assert true\n    }\n}\n\nmain {}\n";

/// A top-level statement beside an explicit `main`: analysis rejects it
/// (`SEM008:top_level_statement_beside_entry`) before format can drop it.
const BESIDE_SOURCE: &str = "+++\nlocale = \"en\"\n+++\nconst int aetas = 25\n\nif aetas ∈ 0‥100 {\n    print \"in range\"\n}\n\nmain {\n    print \"main\"\n}\n";

/// The hook fixture without the hook: nothing is lost, so format succeeds.
const CONTROL_SOURCE: &str = "+++\nlocale = \"en\"\n+++\nconst int aetas = 25\n\ndescribe \"plain\" {\n    test \"sample\" {\n        assert true\n    }\n}\n\nmain {\n    print \"main\"\n}\n";

pub(crate) fn run_faber_capture(args: &[&str], stdin: Option<&str>) -> std::process::Output {
    let mut child = Command::new(faber_binary())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn faber");
    let mut pipe = child.stdin.take().expect("stdin");
    if let Some(text) = stdin {
        pipe.write_all(text.as_bytes()).expect("write stdin");
    }
    drop(pipe);
    child.wait_with_output().expect("wait for faber")
}

/// Every format mode on a lossy file: non-zero exit, the loss named on stderr,
/// the file on disk untouched. Text modes (`--stdout`, `--stdin`) print nothing
/// to stdout; report and write modes print only their summary line (the refusal
/// is counted, never the source).
fn assert_format_refuses(label: &str, source: &str, construct: &str) {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("lossy.fab");
    let path_arg = path.to_str().expect("utf8 path");
    fs::write(&path, source).expect("write fixture");

    let default_mode = run_faber_capture(&["format", path_arg], None);
    let write_mode = run_faber_capture(&["format", "--write", path_arg], None);
    let stdout_mode = run_faber_capture(&["format", "--stdout", path_arg], None);
    let stdin_mode = run_faber_capture(&["format", "--stdin"], Some(source));
    let check_mode = run_faber_capture(&["format", "--check", path_arg], None);
    for (mode, output, prints_summary) in [
        ("default", &default_mode, true),
        ("--write", &write_mode, true),
        ("--stdout", &stdout_mode, false),
        ("--stdin", &stdin_mode, false),
        ("--check", &check_mode, true),
    ] {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "{label}: format {mode} must exit non-zero:\n{stderr}"
        );
        // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
        assert!(
            stderr.contains("CODEGEN006")
                && stderr.contains("ast_construct_dropped")
                && stderr.contains(construct),
            "{label}: format {mode} must name the loss `{construct}`:\n{stderr}"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        if prints_summary {
            assert!(
                stdout.lines().count() == 1 && stdout.contains("1 refused (nothing written)"),
                "{label}: format {mode} must print only the refusal count:\n{stdout}"
            );
        } else {
            assert!(
                output.stdout.is_empty(),
                "{label}: format {mode} must print nothing to stdout"
            );
        }
    }
    assert_eq!(
        fs::read_to_string(&path).expect("read back"),
        source,
        "{label}: the file must stay byte-identical"
    );
}

#[test]
fn format_refuses_hook_loss() {
    assert_format_refuses("hook", HOOK_SOURCE, "hook:Praepara");
}

#[test]
fn format_refuses_top_level_statement_loss() {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("beside.fab");
    let path_arg = path.to_str().expect("utf8 path");
    fs::write(&path, BESIDE_SOURCE).expect("write fixture");

    for (mode, args, prints_summary) in [
        ("default", vec!["format", path_arg], true),
        ("--write", vec!["format", "--write", path_arg], true),
        ("--stdout", vec!["format", "--stdout", path_arg], false),
        ("--check", vec!["format", "--check", path_arg], true),
        ("check", vec!["check", path_arg], false),
    ] {
        let output = run_faber_capture(&args, None);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "beside: {mode} must exit non-zero:\n{stderr}"
        );
        // ALLOW_DIAGNOSTIC_RENDER_TEXT: subprocess CLI stderr renderer contract.
        assert!(
            stderr.contains("SEM008") && stderr.contains("top_level_statement_beside_entry"),
            "beside: {mode} must name the SEM008 issue:\n{stderr}"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        if prints_summary {
            assert!(
                stdout.lines().count() == 1 && stdout.contains("1 refused (nothing written)"),
                "beside: {mode} must print only the refusal count:\n{stdout}"
            );
        } else {
            assert!(
                output.stdout.is_empty(),
                "beside: {mode} must print nothing to stdout"
            );
        }
    }
    assert_eq!(
        fs::read_to_string(&path).expect("read back"),
        BESIDE_SOURCE,
        "beside: the file must stay byte-identical"
    );
}

#[test]
fn format_control_file_unchanged() {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("control.fab");
    let path_arg = path.to_str().expect("utf8 path");
    fs::write(&path, CONTROL_SOURCE).expect("write fixture");

    let stdout_mode = run_faber_capture(&["format", "--stdout", path_arg], None);
    assert!(
        stdout_mode.status.success(),
        "control must format: {}",
        String::from_utf8_lossy(&stdout_mode.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&stdout_mode.stdout), CONTROL_SOURCE);

    let in_place = run_faber_capture(&["format", "--write", path_arg], None);
    assert!(in_place.status.success(), "control must format in place");
    assert_eq!(
        fs::read_to_string(&path).expect("read back"),
        CONTROL_SOURCE
    );
}

/// Need: `faber format` keeps the authored `ᵀ` glyph, including on
/// a compound receiver (which re-emits parenthesized), and never rewrites it
/// to the `.transpose()` method form.
#[test]
fn format_cli_keeps_the_transpose_glyph() {
    let source = "+++\nlocale = \"en\"\n+++\nfn product_transposed(tensor<f32, [2, 2]> a, tensor<f32, [2, 2]> b) → tensor<f32, [2, 2]> {\n    return (a · b)ᵀ.materialize()\n}\n\nfn flipped(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> {\n    return aᵀ.materialize()\n}\n";
    let fixture = std::env::temp_dir().join("faber-format-glyph-transpose-cli.fab");
    fs::write(&fixture, source).expect("write glyph transpose fixture");

    let formatted = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);

    let body = radix::module::driver::split_frontmatter(&formatted)
        .expect("split")
        .body;
    assert!(
        body.contains("return (a · b)ᵀ") && body.contains("return aᵀ"),
        "format must keep the transpose glyph:\n{formatted}"
    );
    assert!(
        !body.contains("transpose("),
        "format must not rewrite the glyph to a method call:\n{formatted}"
    );
    assert_canonical_reparses_with(body, "glyph transpose CLI --stdout", Some(&english_pack()))
        .expect("reparse");
}

/// Need: the glyph id set covers the rounding delimiters and the
/// transpose suffix alike, so `faber format` keeps `⌊x⌋`, `⌈x⌉`, `⌊x⌉` and `ᵀ`
/// exactly as authored in both code locales: the output equals the input and
/// re-parses.
#[test]
fn format_cli_round_trips_rounding_and_transpose_glyphs() {
    let english = "+++\nlocale = \"en\"\n+++\nfn flipped(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> {\n    return aᵀ.materialize()\n}\n\nmain {\n    let float half ← 2.5\n    print ⌊half⌋\n    print ⌈half⌉\n    print ⌊half⌉\n}\n";
    let latin = "+++\nlocale = \"la\"\n+++\nfunctio versa(tensor<f32, [2, 2]> a) → tensor<f32, [2, 2]> {\n    redde aᵀ.materialize()\n}\n\nincipit {\n    sit fractus dimidium ← 2.5\n    nota ⌊dimidium⌋\n    nota ⌈dimidium⌉\n    nota ⌊dimidium⌉\n}\n";

    for (locale, source, pack) in [("en", english, english_pack()), ("la", latin, latin_pack())] {
        let fixture =
            std::env::temp_dir().join(format!("faber-format-glyph-roundtrip-{locale}.fab"));
        fs::write(&fixture, source).expect("write glyph round-trip fixture");

        let formatted = run_faber_format_stdout(&fixture);
        let _ = fs::remove_file(&fixture);

        assert_eq!(
            formatted, source,
            "format must be byte-identical for the {locale} glyph source"
        );
        let body = radix::module::driver::split_frontmatter(&formatted)
            .expect("split")
            .body;
        assert_canonical_reparses_with(body, "glyph round-trip CLI --stdout", Some(&pack))
            .expect("reparse");
    }
}

/// Need: `faber format` keeps an authored in-place numeric twin
/// (`a.add(2)`) instead of respelling it as the desugared `a ← a + 2`.
#[test]
fn format_cli_keeps_the_in_place_numeric_twin() {
    let source = "+++\nlocale = \"en\"\n+++\nmain {\n    mut int a ← 1\n    a.add(2)\n    a.negate()\n    print a\n}\n";
    let fixture = std::env::temp_dir().join("faber-format-numeric-twin-cli.fab");
    fs::write(&fixture, source).expect("write numeric twin fixture");

    let formatted = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);

    let body = radix::module::driver::split_frontmatter(&formatted)
        .expect("split")
        .body;
    assert!(
        body.contains("a.add(2)") && body.contains("a.negate()"),
        "format must keep the in-place twin method spelling:\n{formatted}"
    );
    assert!(
        !body.contains("a ← a"),
        "format must not respell the twin as an assignment:\n{formatted}"
    );
    assert_canonical_reparses_with(body, "numeric twin CLI --stdout", Some(&english_pack()))
        .expect("reparse");
}

// ── FMT-1: report by default, `--write` applies ───────────────────────────

/// Drifted source: formats to a different (canonical) layout.
const DRIFT_SOURCE: &str = "+++\nlocale = \"en\"\n+++\nmain {\n  print \"ok\"\n}\n";

fn run_faber_in(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(faber_binary())
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .expect("run faber")
}

fn stdout_lines(output: &std::process::Output) -> Vec<String> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Back-date a file so a later write is observable through its mtime.
fn backdate(path: &Path) -> std::time::SystemTime {
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open for mtime")
        .set_modified(past)
        .expect("set mtime");
    fs::metadata(path)
        .expect("metadata")
        .modified()
        .expect("mtime")
}

#[test]
fn format_default_reports_drift_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("drift.fab");
    fs::write(&path, DRIFT_SOURCE).expect("write fixture");
    let before = backdate(&path);
    let path_arg = path.to_str().expect("utf8 path");

    let output = run_faber_capture(&["format", path_arg], None);
    assert_eq!(output.status.code(), Some(1), "drift exits 1");
    assert_eq!(
        stdout_lines(&output),
        vec![
            format!("would reformat {path_arg}"),
            "1 of 1 files would be reformatted; rerun with --write to apply".to_owned(),
        ]
    );
    assert_eq!(fs::read_to_string(&path).expect("read"), DRIFT_SOURCE);
    assert_eq!(
        fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        before,
        "report mode must not touch the file"
    );

    // `--check` is the explicit spelling of the default.
    let checked = run_faber_capture(&["format", "--check", path_arg], None);
    assert_eq!(checked.status.code(), Some(1));
    assert_eq!(stdout_lines(&checked), stdout_lines(&output));
}

#[test]
fn format_default_on_formatted_file_exits_zero() {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("clean.fab");
    fs::write(&path, CONTROL_SOURCE).expect("write fixture");

    let output = run_faber_capture(&["format", path.to_str().expect("utf8 path")], None);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(stdout_lines(&output), vec!["1 files already formatted"]);
}

#[test]
fn format_write_writes_only_changed_files() {
    let dir = tempfile::tempdir().expect("create scratch");
    let drift = dir.path().join("a-drift.fab");
    let clean = dir.path().join("b-clean.fab");
    fs::write(&drift, DRIFT_SOURCE).expect("write drift");
    fs::write(&clean, CONTROL_SOURCE).expect("write clean");
    let clean_mtime = backdate(&clean);
    let drift_arg = drift.to_str().expect("utf8 path");
    let clean_arg = clean.to_str().expect("utf8 path");

    let output = run_faber_capture(&["format", "--write", drift_arg, clean_arg], None);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout_lines(&output),
        vec![
            format!("formatted {drift_arg}"),
            "formatted 1 of 2 files".to_owned(),
        ]
    );
    assert_ne!(fs::read_to_string(&drift).expect("read"), DRIFT_SOURCE);
    assert_eq!(fs::read_to_string(&clean).expect("read"), CONTROL_SOURCE);
    assert_eq!(
        fs::metadata(&clean)
            .expect("metadata")
            .modified()
            .expect("mtime"),
        clean_mtime,
        "an unchanged file must not be touched"
    );

    // A second run has nothing left to do.
    let again = run_faber_capture(&["format", "--write", drift_arg, clean_arg], None);
    assert_eq!(again.status.code(), Some(0));
    assert_eq!(stdout_lines(&again), vec!["formatted 0 of 2 files"]);
}

#[test]
fn format_without_paths_reports_the_current_directory() {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("drift.fab");
    fs::write(&path, DRIFT_SOURCE).expect("write fixture");

    let output = run_faber_in(dir.path(), &["format"]);
    assert_eq!(output.status.code(), Some(1));
    let lines = stdout_lines(&output);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("would reformat ") && lines[0].ends_with("drift.fab"));
    assert_eq!(fs::read_to_string(&path).expect("read"), DRIFT_SOURCE);

    let written = run_faber_in(dir.path(), &["format", "--write"]);
    assert_eq!(written.status.code(), Some(0));
    assert_ne!(fs::read_to_string(&path).expect("read"), DRIFT_SOURCE);
}

#[test]
fn format_write_counts_refused_files_and_never_writes_them() {
    let dir = tempfile::tempdir().expect("create scratch");
    let refused = dir.path().join("a-lossy.fab");
    let drift = dir.path().join("b-drift.fab");
    fs::write(&refused, HOOK_SOURCE).expect("write lossy");
    fs::write(&drift, DRIFT_SOURCE).expect("write drift");
    let refused_arg = refused.to_str().expect("utf8 path");
    let drift_arg = drift.to_str().expect("utf8 path");

    let report = run_faber_capture(&["format", refused_arg, drift_arg], None);
    assert_eq!(
        report.status.code(),
        Some(65),
        "a refused source is a data error"
    );
    assert_eq!(
        stdout_lines(&report).last().map(String::as_str),
        Some(
            "1 of 2 files would be reformatted; rerun with --write to apply, 1 refused (nothing written)"
        )
    );

    let written = run_faber_capture(&["format", "--write", refused_arg, drift_arg], None);
    assert_eq!(
        written.status.code(),
        Some(65),
        "a refused file fails the run as a data error"
    );
    assert_eq!(
        stdout_lines(&written),
        vec![
            format!("formatted {drift_arg}"),
            "formatted 1 of 2 files, 1 refused (nothing written)".to_owned(),
        ]
    );
    assert_eq!(fs::read_to_string(&refused).expect("read"), HOOK_SOURCE);
    assert_ne!(
        fs::read_to_string(&drift).expect("read"),
        DRIFT_SOURCE,
        "the formattable file is still written"
    );
}

#[test]
fn format_text_modes_print_no_summary() {
    let dir = tempfile::tempdir().expect("create scratch");
    let path = dir.path().join("drift.fab");
    fs::write(&path, DRIFT_SOURCE).expect("write fixture");

    let stdout_mode = run_faber_capture(
        &["format", "--stdout", path.to_str().expect("utf8 path")],
        None,
    );
    assert_eq!(stdout_mode.status.code(), Some(0));
    let text = String::from_utf8_lossy(&stdout_mode.stdout);
    assert!(
        text.starts_with("+++") && !text.contains("reformat"),
        "{text}"
    );
    assert_eq!(fs::read_to_string(&path).expect("read"), DRIFT_SOURCE);

    let stdin_mode = run_faber_capture(&["format", "--stdin"], Some(DRIFT_SOURCE));
    assert_eq!(stdin_mode.status.code(), Some(0));
    assert_eq!(stdin_mode.stdout, stdout_mode.stdout);
}

#[test]
fn format_usage_errors_exit_sixty_four() {
    let output = run_faber_capture(&["format", "--write", "--check", "x.fab"], None);
    assert_eq!(output.status.code(), Some(64));
}

// ── membership-glyph 3a: `∈` / `∉` survive the format round trip ──────────

const MEMBERSHIP_EN_SOURCE: &str = "+++\nlocale = \"en\"\n+++\nmain {\n    let list<int> xs ← [1, 2]\n    if 1 ∈ xs {\n        print \"in\"\n    }\n    if 3 ∉ xs {\n        print \"out\"\n    }\n    if 5 ∉ 0 ‥ 10 {\n        print \"outside\"\n    }\n}\n";

#[test]
fn format_stdin_keeps_membership_glyphs_at_the_source_locale() {
    let (stdout, stderr, success) = run_faber_format_stdin(MEMBERSHIP_EN_SOURCE);
    assert!(success, "faber format --stdin must accept ∈/∉: {stderr}");
    let body = radix::module::driver::split_frontmatter(&stdout)
        .expect("split")
        .body;
    assert!(body.contains("if 1 ∈ xs {"), "∈ must be kept:\n{stdout}");
    assert!(body.contains("if 3 ∉ xs {"), "∉ must be kept:\n{stdout}");
    assert!(
        body.contains("if 5 ∉ 0 ‥ 10 {"),
        "range ∉ must be kept:\n{stdout}"
    );
    assert!(
        !body.contains("inter") && !body.contains("between"),
        "no keyword spelling may appear:\n{stdout}"
    );
    assert_eq!(
        assert_format_fixed_point(MEMBERSHIP_EN_SOURCE, "membership glyphs"),
        stdout
    );
}

#[test]
fn format_stdin_keeps_a_negated_glyph_membership_as_a_negation() {
    // `not (x ∈ xs)` is a unary not over `∈`: the glyph is printed and the
    // negation stays a negation (no `∉` is invented).
    let source = "+++\nlocale = \"en\"\n+++\nmain {\n    let list<int> xs ← [1, 2]\n    if not (3 ∈ xs) {\n        print \"out\"\n    }\n}\n";
    let (stdout, stderr, success) = run_faber_format_stdin(source);
    assert!(success, "{stderr}");
    assert!(stdout.contains("if not (3 ∈ xs) {"), "{stdout}");
}

#[test]
fn convert_keeps_membership_glyphs_across_locales() {
    let (stdout, stderr, success) = run_faber_capture_stdout(
        &["convert", "--from", "en", "--to", "la", "--stdin"],
        MEMBERSHIP_EN_SOURCE,
    );
    assert!(success, "{stderr}");
    assert!(stdout.contains("si 1 ∈ xs {"), "{stdout}");
    assert!(stdout.contains("si 3 ∉ xs {"), "{stdout}");
    assert!(stdout.contains("si 5 ∉ 0 ‥ 10 {"), "{stdout}");
}

fn run_faber_capture_stdout(args: &[&str], stdin: &str) -> (String, String, bool) {
    let output = run_faber_capture(args, Some(stdin));
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

// `faber format --width N` precedence and command wiring.

const WIDTH_SOURCE: &str = "functio f(textus alpha, textus beta, textus gamma, textus delta) → textus {\n    redde alpha\n}\n";

fn width_package(width_line: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create scratch");
    fs::write(
        dir.path().join("faber.toml"),
        format!(
            "[package]\nname = \"w\"\nversion = \"0.1.0\"\nedition = \"1\"\n\n[paths]\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n\n[locale]\nlocale = \"la\"\n{width_line}"
        ),
    )
    .expect("write manifest");
    fs::write(dir.path().join("main.fab"), WIDTH_SOURCE).expect("write source");
    dir
}

fn stdout_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn max_line(text: &str) -> usize {
    text.lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0)
}

#[test]
fn format_width_precedence_flag_then_manifest_then_none() {
    let with_key = width_package("\n[format]\nwidth = 50\n");
    let path = with_key.path().join("main.fab");
    let path_arg = path.to_str().expect("utf8 path");

    // The manifest width applies without a flag.
    let manifest = run_faber_capture(&["format", "--stdout", path_arg], None);
    assert!(manifest.status.success(), "{manifest:?}");
    let manifest_out = stdout_of(&manifest);
    assert!(max_line(&manifest_out) <= 50, "{manifest_out}");
    assert!(manifest_out.lines().count() > WIDTH_SOURCE.lines().count());

    // The flag wins over the manifest, wider or narrower.
    let wide = run_faber_capture(&["format", "--stdout", "--width", "500", path_arg], None);
    assert_eq!(
        stdout_of(&wide).lines().count(),
        WIDTH_SOURCE.lines().count()
    );
    let narrow = run_faber_capture(&["format", "--stdout", "--width", "30", path_arg], None);
    assert!(
        stdout_of(&narrow).lines().count() > manifest_out.lines().count(),
        "{narrow:?}"
    );

    // With neither, nothing wraps.
    let without = width_package("");
    let path = without.path().join("main.fab");
    let plain = run_faber_capture(&["format", "--stdout", path.to_str().expect("utf8")], None);
    assert!(plain.status.success(), "{plain:?}");
    assert!(
        stdout_of(&plain).contains(
            "functio f(textus alpha, textus beta, textus gamma, textus delta) → textus {"
        )
    );
}

#[test]
fn format_width_invalid_manifest_value_is_refused() {
    let dir = width_package("\n[format]\nwidth = 0\n");
    let path = dir.path().join("main.fab");
    let output = run_faber_capture(&["format", "--stdout", path.to_str().expect("utf8")], None);
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("package_format_width_invalid"),
        "{output:?}"
    );
}

#[test]
fn format_width_stdin_uses_only_the_flag() {
    // Stdin has no package, so the source names its own locale.
    let source = format!("+++\nlocale = \"la\"\n+++\n{WIDTH_SOURCE}");
    let plain = run_faber_capture(&["format", "--stdin"], Some(&source));
    assert!(plain.status.success(), "{plain:?}");
    assert!(
        stdout_of(&plain).contains(
            "functio f(textus alpha, textus beta, textus gamma, textus delta) → textus {"
        )
    );
    let wrapped = run_faber_capture(&["format", "--stdin", "--width", "40"], Some(&source));
    assert!(wrapped.status.success(), "{wrapped:?}");
    assert!(
        max_line(&stdout_of(&wrapped)) <= 40,
        "{}",
        stdout_of(&wrapped)
    );
}

/// `faber format` on an import-bearing file resolves the import target's own
/// frontmatter locale through the format session's embedded reader packs: on
/// an installed binary, with no workspace `locale/` tree beside it, formatting
/// must not fail on the imported Latin module. The discovered root is cleared
/// on the exact session the command mints, so only the embedded loader can
/// serve the import's declared locale.
#[test]
fn format_resolves_import_locale_through_loader_without_root() {
    let dir = tempfile::tempdir().expect("create scratch");
    let provider = dir.path().join("valida.fab");
    fs::write(
        &provider,
        "+++\nlocale = \"la\"\n+++\n\n@ publica\nfunctio nomen(textus s) → textus {\n    redde s\n}\n",
    )
    .expect("write provider");
    let path = dir.path().join("main.fab");
    let source = "+++\nlocale = \"en\"\n+++\nimport from \"./valida\" valida\n\nmain {\n    print valida.nomen(\"a\")\n}\n";
    fs::write(&path, source).expect("write main");

    let mut session = format_session(&path, None, source).expect("format session");
    session.config.locale_root = None; // the installed-binary condition

    let name = path.display().to_string();
    let result = reemit_faber(
        &session,
        session.config.locale_pack.as_ref().expect("emission pack"),
        &name,
        source,
    );
    assert!(
        result.success(),
        "the import's declared Latin locale must resolve through the embedded loader: {:?}",
        result.diagnostics
    );
    let code = result.output.expect("formatted output").code;
    assert!(
        code.contains("valida.nomen"),
        "the formatted source must keep the resolved import call:\n{code}"
    );
    let _ = fs::remove_dir_all(&dir);
}
