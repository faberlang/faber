//! Driver resolution of build-time embeds (DECISIONS D8.10).
//!
//! Contract: `fixum T X = insere "path"` reads a package-relative file
//! during analysis. The package root is the nearest `faber.toml` ancestor of
//! the source file, else the file's directory. Absolute paths, `..` that
//! leaves the root, and missing files are SEM061 errors; the read file is
//! recorded as a build input on the analyzed module. In the en surface the
//! keyword is `embed`, contextual: it stays an ordinary identifier elsewhere.

use std::fs;
use std::path::Path;

use crate::driver::{AnalyzeRequest, AnalyzedModule, Config, Session, analyze_source};
use crate::hir::{HirExpressionKind, HirItemKind, HirLiteral};
use radix_diagnostics::Diagnostic;

fn analyze_file(path: &Path) -> Result<AnalyzedModule, Vec<Diagnostic>> {
    let source = fs::read_to_string(path).expect("read source");
    let session = Session::new(Config::default().with_dev_locale_root());
    analyze_source(&session, &path.display().to_string(), &source, AnalyzeRequest::default())
}

fn write(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(path, bytes).expect("write");
}

fn static_literal(unit: &AnalyzedModule, name: &str) -> HirLiteral {
    unit.hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if unit.interner.resolve(konst.name) == name => match &konst.value.kind {
                HirExpressionKind::Literal(literal) => Some(literal.clone()),
                other => panic!("expected folded literal, got {other:?}"),
            },
            _ => None,
        })
        .expect("static item")
}

fn assert_insere_issue(result: Result<AnalyzedModule, Vec<Diagnostic>>, issue: &str) {
    let diagnostics = match result {
        Ok(unit) => unit.diagnostics,
        Err(diagnostics) => diagnostics,
    };
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == Some("SEM061") && d.issue() == Some(issue)),
        "expected SEM061 {issue}, got {diagnostics:?}"
    );
}

#[test]
fn textus_embed_reads_file_next_to_single_file_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("schema.sql"), b"CREATE TABLE t;\n");
    let main = dir.path().join("main.fab");
    write(
        &main,
        b"fixum textus SCHEMA = insere \"schema.sql\"\nincipit {\n    nota SCHEMA\n}\n",
    );
    let unit = analyze_file(&main).unwrap_or_else(|d| panic!("analysis failed: {d:?}"));
    let HirLiteral::String(sym) = static_literal(&unit, "SCHEMA") else {
        panic!("expected textus literal");
    };
    assert_eq!(unit.interner.resolve(sym), "CREATE TABLE t;\\n");
    let expected = dir
        .path()
        .join("schema.sql")
        .canonicalize()
        .expect("canonical");
    assert_eq!(unit.embedded_inputs, vec![expected]);
}

#[test]
fn textus_embed_rejects_invalid_utf8() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("bad.txt"), &[0x61, 0xff]);
    let main = dir.path().join("main.fab");
    write(&main, b"fixum textus S = insere \"bad.txt\"\nincipit {\n    nota S\n}\n");
    assert_insere_issue(analyze_file(&main), "insere_invalid_utf8");
}

#[test]
fn octeti_embed_reads_raw_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("logo.bin"), &[0x89, 0x50, 0x00, 0xff]);
    let main = dir.path().join("main.fab");
    write(
        &main,
        b"fixum lista<b8> LOGO = insere \"logo.bin\"\nincipit {\n    nota \"ok\"\n}\n",
    );
    let unit = analyze_file(&main).unwrap_or_else(|d| panic!("analysis failed: {d:?}"));
    let HirLiteral::Octeti(sym) = static_literal(&unit, "LOGO") else {
        panic!("expected octeti literal");
    };
    assert_eq!(unit.interner.resolve(sym), "895000ff");
}

#[test]
fn missing_file_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let main = dir.path().join("main.fab");
    write(&main, b"fixum textus S = insere \"absens.txt\"\nincipit {\n    nota S\n}\n");
    assert_insere_issue(analyze_file(&main), "insere_file_missing");
}

#[test]
fn absolute_path_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let data = dir.path().join("data.txt");
    write(&data, b"x");
    let main = dir.path().join("main.fab");
    let source = format!("fixum textus S = insere \"{}\"\nincipit {{\n    nota S\n}}\n", data.display());
    write(&main, source.as_bytes());
    assert_insere_issue(analyze_file(&main), "insere_path_absolute");
}

#[test]
fn parent_escape_from_the_package_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("secret.txt"), b"x");
    let main = dir.path().join("pkg").join("main.fab");
    write(&main, b"fixum textus S = insere \"../secret.txt\"\nincipit {\n    nota S\n}\n");
    assert_insere_issue(analyze_file(&main), "insere_path_escapes_package");
}

#[test]
fn package_sources_resolve_against_the_manifest_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("faber.toml"), b"[package]\nname = \"p\"\n");
    write(&dir.path().join("data").join("x.txt"), b"salve");
    let main = dir.path().join("src").join("main.fab");
    // `..` inside the root is fine; it only may not climb above it.
    write(
        &main,
        b"fixum textus S = insere \"src/../data/x.txt\"\nincipit {\n    nota S\n}\n",
    );
    let unit = analyze_file(&main).unwrap_or_else(|d| panic!("analysis failed: {d:?}"));
    let HirLiteral::String(sym) = static_literal(&unit, "S") else {
        panic!("expected textus literal");
    };
    assert_eq!(unit.interner.resolve(sym), "salve");
}

#[test]
fn en_embed_spelling_is_contextual() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("x.txt"), b"hello");
    let main = dir.path().join("main.fab");
    write(
        &main,
        b"+++\nlocale = \"en\"\n+++\nconst string S = embed \"x.txt\"\nfn embed(int x) \xe2\x86\x92 int {\n    return x\n}\nfn local() \xe2\x86\x92 int {\n    let int embed \xe2\x86\x90 3\n    return embed\n}\nmain {\n    print S\n    print embed(local())\n}\n",
    );
    let unit = analyze_file(&main).unwrap_or_else(|d| panic!("analysis failed: {d:?}"));
    let HirLiteral::String(sym) = static_literal(&unit, "S") else {
        panic!("expected textus literal");
    };
    assert_eq!(unit.interner.resolve(sym), "hello");
}

#[test]
fn faber_reemission_prints_the_embed_not_the_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("x.txt"), b"salve");
    write(&dir.path().join("x.bin"), &[1, 2]);
    let main = dir.path().join("main.fab");
    let source = "fixum textus S = insere \"x.txt\"\n\nfixum lista<b8> B = insere \"x.bin\"\n\ngenus G {\n    fixum textus T = insere \"x.txt\"\n}\n\nincipit {\n    nota S\n}\n";
    write(&main, source.as_bytes());
    let session = Session::new(
        Config::default()
            .with_target(crate::codegen::Target::HirFaber)
            .with_dev_locale_root(),
    );
    let result = crate::driver::compile(
        &session,
        &main.display().to_string(),
        source,
        crate::driver::ModuleCompileRequest::default(),
    );
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);
    let crate::Output::Faber(out) = result.output.expect("output") else {
        panic!("expected faber output");
    };
    assert!(out.code.contains("fixum textus S = insere \"x.txt\""), "{}", out.code);
    assert!(out.code.contains("fixum lista<b8> B = insere \"x.bin\""), "{}", out.code);
    assert!(out.code.contains("fixum textus T = insere \"x.txt\""), "{}", out.code);
    assert!(!out.code.contains("salve"), "{}", out.code);
}
