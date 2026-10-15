//! Driver resolution of `insere` in every `=` position (st1 U4).
//!
//! Contract: the driver walks the whole AST for embeds, so a block-level `=`
//! constant, a genus field default under any modifier, and an embed inside a
//! `praefixum` body all read their file; the package-root path rules apply to
//! each. Faber re-emission prints the authored embed at every position, and a
//! `praefixum` body that returns an embed evaluates to the file's text.

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

fn debug_hir(unit: &AnalyzedModule) -> String {
    format!("{:?}", unit.hir)
}

fn faber_output(main: &Path, source: &str) -> String {
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
    out.code
}

#[test]
fn block_level_embed_reads_the_file_and_records_the_build_input() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("x.txt"), b"salve");
    let main = dir.path().join("main.fab");
    write(
        &main,
        b"functio f() \xe2\x86\x92 textus {\n    fixum textus x = insere \"x.txt\"\n    redde x\n}\nincipit {\n    nota f()\n}\n",
    );
    let unit = analyze_file(&main).unwrap_or_else(|d| panic!("analysis failed: {d:?}"));
    assert!(!debug_hir(&unit).contains("Insere("), "an embed survived analysis");
    let expected = dir.path().join("x.txt").canonicalize().expect("canonical");
    assert_eq!(unit.embedded_inputs, vec![expected]);
}

#[test]
fn field_default_embed_reads_the_file_under_every_modifier() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("x.txt"), b"salve");
    let main = dir.path().join("main.fab");
    write(
        &main,
        b"genus G {\n    sit textus a = insere \"x.txt\"\n    varia textus b = insere \"x.txt\"\n    generis textus c = insere \"x.txt\"\n}\nincipit {\n    nota \"ok\"\n}\n",
    );
    let unit = analyze_file(&main).unwrap_or_else(|d| panic!("analysis failed: {d:?}"));
    let HirItemKind::Struct(genus) = &unit.hir.items[0].kind else {
        panic!("expected genus");
    };
    assert_eq!(genus.fields.len(), 3);
    for field in &genus.fields {
        let init = field.init.as_ref().expect("field default");
        assert!(
            matches!(init.kind, HirExpressionKind::Literal(HirLiteral::String(_))),
            "{:?}",
            init.kind
        );
    }
}

#[test]
fn block_level_embed_keeps_the_package_root_rules() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("secret.txt"), b"x");
    let main = dir.path().join("pkg").join("main.fab");
    write(
        &main,
        b"functio f() \xe2\x86\x92 textus {\n    fixum textus x = insere \"../secret.txt\"\n    redde x\n}\nincipit {\n    nota f()\n}\n",
    );
    let diagnostics = match analyze_file(&main) {
        Ok(unit) => unit.diagnostics,
        Err(diagnostics) => diagnostics,
    };
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == Some("SEM061") && d.issue() == Some("insere_path_escapes_package")),
        "{diagnostics:?}"
    );
}

#[test]
fn praefixum_body_that_returns_an_embed_evaluates_to_the_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("x.txt"), b"salve");
    let main = dir.path().join("main.fab");
    write(
        &main,
        b"fixum textus S = praefixum {\n    redde insere \"x.txt\"\n}\nincipit {\n    nota S\n}\n",
    );
    let unit = analyze_file(&main).unwrap_or_else(|d| panic!("analysis failed: {d:?}"));
    let value = unit
        .hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if unit.interner.resolve(konst.name) == "S" => Some(&konst.value.kind),
            _ => None,
        })
        .expect("constant S");
    let HirExpressionKind::Literal(HirLiteral::String(sym)) = value else {
        panic!("expected evaluated textus literal, got {value:?}");
    };
    assert_eq!(unit.interner.resolve(*sym), "salve");
}

#[test]
fn praefixum_default_of_an_instance_field_evaluates_like_a_module_constant() {
    // One initializer surface: a `fixum`/`varia` field default takes the same
    // `praefixum` body (and so the same embed) a `generis` field does.
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("x.txt"), b"salve");
    let main = dir.path().join("main.fab");
    write(
        &main,
        b"genus G {\n    varia textus v = praefixum {\n        redde insere \"x.txt\"\n    }\n    sit numerus n = praefixum {\n        redde 3 + 4\n    }\n}\nincipit {\n    nota \"ok\"\n}\n",
    );
    let unit = analyze_file(&main).unwrap_or_else(|d| panic!("analysis failed: {d:?}"));
    let HirItemKind::Struct(genus) = &unit.hir.items[0].kind else {
        panic!("expected genus");
    };
    let kinds: Vec<_> = genus
        .fields
        .iter()
        .map(|field| &field.init.as_ref().expect("field default").kind)
        .collect();
    assert!(
        matches!(kinds[0], HirExpressionKind::Literal(HirLiteral::String(_))),
        "{:?}",
        kinds[0]
    );
    assert!(
        matches!(kinds[1], HirExpressionKind::Literal(HirLiteral::Int(7))),
        "{:?}",
        kinds[1]
    );
}

#[test]
fn faber_reemission_prints_the_embed_at_every_position() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(&dir.path().join("x.txt"), b"salve");
    let main = dir.path().join("main.fab");
    let source = "genus G {\n    sit textus a = insere \"x.txt\"\n    varia textus b = insere \"x.txt\"\n}\n\nincipit {\n    fixum textus l = insere \"x.txt\"\n    nota l\n}\n";
    write(&main, source.as_bytes());
    let code = faber_output(&main, source);
    assert!(code.contains("sit textus a = insere \"x.txt\""), "{code}");
    assert!(code.contains("varia textus b = insere \"x.txt\""), "{code}");
    assert!(code.contains("fixum textus l = insere \"x.txt\""), "{code}");
    assert!(!code.contains("salve"), "{code}");
}
