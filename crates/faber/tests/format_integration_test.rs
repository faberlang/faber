//! Subprocess checks for `faber format --stdout` (requires built `faber` binary).

use radix::module::driver::faber_format::test_gate::assert_canonical_reparses_with;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[path = "support/temp.rs"]
mod temp;
use temp::TempDir;

fn exempla(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(path)
}

/// English reader pack — the product-default surface the CLI uses for
/// untagged sources and `locale = "en"` frontmatter files.
fn english_pack() -> radix::module::locale::LocalePack {
    let path = radix::module::locale::workspace_locale_pack_path("en");
    radix::module::locale::LocalePack::from_toml_path(path).expect("load English locale pack")
}

fn latin_pack() -> radix::module::locale::LocalePack {
    let path = radix::module::locale::workspace_locale_pack_path("la");
    radix::module::locale::LocalePack::from_toml_path(path).expect("load Latin locale pack")
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
    radix::module::driver::split_frontmatter(source)
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

fn run_faber_format_write(path: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(["format", "--write"])
        .arg(path)
        .output()
        .expect("run faber format --write");
    assert!(
        output.status.success(),
        "faber format --write failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn format_convert_and_doc_keep_declaration_member_comments() {
    let temp_root = TempDir::new("faber-format", "declaration-member-comments");
    let fixture = temp_root.join("members.fab");
    fs::write(
        &fixture,
        concat!(
            "+++\nlocale = \"en\"\n+++\n",
            "@ public\n",
            "union Chess {\n",
            "    # Every piece has a rank.\n",
            "    @ shared\n",
            "    int rank\n",
            "    # Every piece has a side.\n",
            "    string side\n\n",
            "    # A pawn and its steps.\n",
            "    Pawn { int steps },\n",
            "    # A rook.\n",
            "    Rook\n",
            "}\n\n",
            "@ public\n",
            "enum Color {\n",
            "    # The light square color.\n",
            "    Light,\n",
            "    # The dark square color.\n",
            "    Dark\n",
            "}\n\n",
            "@ public\n",
            "interface Piece {\n",
            "    # Returns the piece name.\n",
            "    fn name() → string\n",
            "}\n",
        ),
    )
    .expect("write member comments fixture");
    let path = fixture.to_str().expect("utf8 fixture path");
    let expected_docs = [
        "Every piece has a rank.",
        "Every piece has a side.",
        "A pawn and its steps.",
        "A rook.",
        "The light square color.",
        "The dark square color.",
        "Returns the piece name.",
    ];

    run_faber_format_write(&fixture);
    let formatted = fs::read_to_string(&fixture).expect("read formatted member fixture");
    for doc in expected_docs {
        assert!(
            formatted.contains(&format!("# {doc}")),
            "format must preserve declaration member comment `{doc}`:\n{formatted}"
        );
    }
    run_faber_format_write(&fixture);
    let second = fs::read_to_string(&fixture).expect("read second format pass");
    assert_eq!(
        formatted, second,
        "format must be idempotent for member comments"
    );

    let latin = run_faber_command(&["convert", "--to", "la", "--stdout", path]);
    for doc in expected_docs {
        assert!(
            latin.contains(&format!("# {doc}")),
            "convert must preserve declaration member comment `{doc}`:\n{latin}"
        );
    }
    let latin_path = temp_root.join("members-la.fab");
    fs::write(&latin_path, latin).expect("write converted source");
    let back = run_faber_command(&[
        "convert",
        "--to",
        "en",
        "--stdout",
        latin_path.to_str().expect("utf8 converted path"),
    ]);
    for doc in expected_docs {
        assert!(
            back.contains(&format!("# {doc}")),
            "convert round trip must preserve declaration member comment `{doc}`:\n{back}"
        );
    }

    let documented = run_faber_command(&["doc", "--json", path]);
    for doc in expected_docs {
        assert!(
            documented.contains(doc),
            "faber doc must expose formatted member comment `{doc}`:\n{documented}"
        );
    }
}

fn run_faber_command(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_faber"))
        .args(args)
        .output()
        .expect("run faber command");
    assert!(
        output.status.success(),
        "faber {:?} failed:\n{}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("faber stdout is utf8")
}

#[test]
fn format_cmd_stdout_reparses_salve_munde() {
    let path = exempla("incipit/salve-munde.fab");
    let formatted = run_faber_format_stdout(&path);
    assert!(
        formatted.starts_with("+++"),
        "stdout must include frontmatter"
    );
    assert_canonical_reparses_with(
        body_for_parse(&formatted),
        "salve-munde CLI --stdout",
        Some(&english_pack()),
    )
    .expect("reparse");
}

#[test]
fn format_cmd_stdout_is_idempotent_for_salve_munde() {
    let path = exempla("incipit/salve-munde.fab");
    let first = run_faber_format_stdout(&path);
    let temp_root = TempDir::new("faber-format", "idempotent-salve");
    let tmp = temp_root.join("salve.fab");
    fs::write(&tmp, &first).expect("write temp fab");
    let second = run_faber_format_stdout(&tmp);
    let _ = fs::remove_file(&tmp);
    assert_eq!(
        first, second,
        "format(format(x)) must equal format(x) via CLI"
    );
}

/// Need: CLI `format --stdout` keeps English types on an en source.
#[test]
fn format_cmd_en_source_keeps_english_types() {
    let temp_root = TempDir::new("faber-format", "en-types");
    let fixture = temp_root.join("pack.fab");
    fs::write(
        &fixture,
        "+++\nlocale = \"en\"\n+++\nfn pack(int n, float x, string name, list<b8> buf) → string {\n    return name\n}\n",
    )
    .expect("write en types fixture");

    let formatted = run_faber_format_stdout(&fixture);
    let body = body_for_parse(&formatted);
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

/// Surface-faithful: la source stays la through the CLI.
#[test]
fn format_cmd_la_source_stays_latin() {
    let temp_root = TempDir::new("faber-format", "la-types");
    let fixture = temp_root.join("pack.fab");
    fs::write(
        &fixture,
        "+++\nlocale = \"la\"\n+++\nfunctio pack(numerus n, textus nomen) → textus {\n    redde nomen\n}\n",
    )
    .expect("write la fixture");

    let formatted = run_faber_format_stdout(&fixture);
    let body = body_for_parse(&formatted);
    assert!(
        body.contains("functio") && body.contains("numerus") && body.contains("textus"),
        "la format must keep Latin spellings:\n{formatted}"
    );
    assert_canonical_reparses_with(body, "la types CLI --stdout", Some(&latin_pack()))
        .expect("reparse");
}

/// SC-3/verification step 5: CLI `format --stdout` on comment fixture re-parses.
#[test]
fn format_cmd_comment_fixture_cli_reparses() {
    let temp_root = TempDir::new("faber-format", "comment-fixture");
    let fixture = temp_root.join("fixture.fab");
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
    assert_canonical_reparses_with(
        body_for_parse(&formatted),
        "comment CLI --stdout",
        Some(&english_pack()),
    )
    .expect("reparse");
}

/// Numeric twin methods re-emit with their authored spelling in every form:
/// binary (`x.added(y)`), unary (`x.negated()`), in-place (`acc.add(y)`),
/// and effect-hoisted in-place (`xs[f()].add(y)`) — never respelled to an
/// operator, an assignment, or a block with temporaries.
#[test]
fn format_cmd_keeps_numeric_twin_spellings() {
    let temp_root = TempDir::new("faber-format", "numeric-twins");
    let fixture = temp_root.join("twins.fab");
    fs::write(
        &fixture,
        concat!(
            "+++\nlocale = \"en\"\n+++\n",
            "fn bump(int i) → int {\n    return i + 1\n}\n\n",
            "main {\n",
            "    let float x ← 2.0\n",
            "    let float chained ← x.added(1.0).multiplied(3.0)\n",
            "    let float neg ← x.negated()\n",
            "    mut float acc ← 10.0\n",
            "    acc.add(5.0)\n",
            "    mut list<float> xs ← [1.0, 2.0]\n",
            "    xs[bump(0)].add(5.0)\n",
            "    print chained\n    print neg\n    print acc\n    print xs[0]\n",
            "}\n",
        ),
    )
    .expect("write numeric twin fixture");

    let formatted = run_faber_format_stdout(&fixture);
    let body = body_for_parse(&formatted);
    for authored in [
        "x.added(1.0).multiplied(3.0)",
        "x.negated()",
        "acc.add(5.0)",
        "xs[bump(0)].add(5.0)",
    ] {
        assert!(
            body.contains(authored),
            "format must keep the authored twin spelling `{authored}`:\n{formatted}"
        );
    }
    assert!(
        !body.contains("__place"),
        "format must not emit the checker's place temporaries:\n{formatted}"
    );
    assert_canonical_reparses_with(body, "numeric twins CLI --stdout", Some(&english_pack()))
        .expect("reparse");

    // The authored spellings must also stabilize: formatting the formatted
    // source changes nothing.
    let tmp = temp_root.join("pass1.fab");
    fs::write(&tmp, &formatted).expect("write formatted pass");
    let second = run_faber_format_stdout(&tmp);
    let _ = fs::remove_file(&tmp);
    assert_eq!(
        formatted, second,
        "authored twin spellings must survive a second format pass"
    );
}

#[test]
fn format_cmd_keeps_call_site_access_markers() {
    let temp_root = TempDir::new("faber-format", "call-markers");
    let fixture = temp_root.join("markers.fab");
    fs::write(
        &fixture,
        concat!(
            "+++\nlocale = \"en\"\n+++\n",
            "fn bump(mut list<int> a) → void {\n    a.append(1)\n}\n",
            "fn dup(copy list<int> a) → void {\n    print a.length()\n}\n",
            "fn look(list<int> a) → void {\n    print a.length()\n}\n\n",
            "main {\n    mut list<int> xs ← [1]\n",
            "    bump(mut xs)\n    dup(copy xs)\n    look(ref xs)\n    look(xs)\n}\n",
        ),
    )
    .expect("write marker fixture");

    let formatted = run_faber_format_stdout(&fixture);
    let body = body_for_parse(&formatted);
    for authored in ["bump(mut xs)", "dup(copy xs)", "look(ref xs)", "look(xs)"] {
        assert!(
            body.contains(authored),
            "format must keep the call-site marker in `{authored}`:\n{formatted}"
        );
    }
    assert_canonical_reparses_with(body, "call markers CLI --stdout", Some(&english_pack()))
        .expect("reparse");
}
