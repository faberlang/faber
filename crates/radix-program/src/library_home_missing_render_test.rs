//! ON-R12 regression: a `LibraryHomeMissing` failure must render its full
//! message — the specifier and the --library-home setup hint — and a
//! position that exists in the file it names. Guards against the coded
//! empty-body form `error[PKG001:missing_library_home]: <file>:<byte>`,
//! where the renderer drops the message and the byte offset masquerades as
//! a line number.

use crate::import_graph::library_resolve_diagnostic;
use crate::library::LibraryResolver;
use radix_module::diagnostics::render_plain;
use radix_module::syntax::StmtKind;
use std::fs;
use std::path::PathBuf;

fn temp_root(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "radix-program-library-home-missing-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&path).expect("mkdir");
    path
}

#[test]
fn library_home_missing_renders_message_hint_and_real_line() {
    let root = temp_root("render");
    let file = root.join("main.fab");
    let source = "functio main() → vacuum {\n    tacet\n}\n\nimporta ex \"prov:mod\" mod\n";
    fs::write(&file, source).expect("write source");

    // Construct the failure through the resolver with no library home.
    let resolver = LibraryResolver::with_optional_home(None);
    let err = resolver
        .resolve("prov:mod")
        .expect_err("no home must fail closed");

    // Attach the failing import's position with source context, exactly as
    // the package-analysis carriage does (`library_imports_from_cached`).
    let lex = radix_module::lexer::lex(source);
    assert!(lex.success(), "lex failed");
    let parse = radix_module::parser::parse_with_options(
        lex,
        radix_module::parser::ParseOptions {
            allow_bodyless_functions: false,
        },
    );
    assert!(parse.success(), "parse failed");
    let program = parse.program.expect("program");
    let decl = program
        .statements
        .iter()
        .find_map(|stmt| match &stmt.kind {
            StmtKind::Import(decl) if parse.interner.resolve(decl.path) == "prov:mod" => Some(decl),
            _ => None,
        })
        .expect("import decl for prov:mod");

    let diagnostic = library_resolve_diagnostic(&file, err).with_source_context(
        &file.display().to_string(),
        source,
        decl.span,
    );
    let _ = fs::remove_dir_all(&root);

    assert_eq!(
        diagnostic.issue(),
        Some("missing_library_home"),
        "the failure must stay machine-identifiable: {diagnostic:?}"
    );
    let rendered = render_plain(&diagnostic);
    // Assert on the header line: the source snippet below it names the
    // specifier on its own, which would mask a dropped message.
    let header = rendered.lines().next().unwrap_or("");
    assert!(
        header.contains("prov:mod"),
        "rendered header must name the specifier: {rendered}"
    );
    assert!(
        header.contains("--library-home"),
        "rendered header must carry the setup hint: {rendered}"
    );
    let expected_line = source[..decl.span.start as usize]
        .bytes()
        .filter(|&byte| byte == b'\n')
        .count()
        + 1;
    assert!(
        rendered.contains(&format!("{}:{expected_line}", file.display())),
        "rendered location must be a real 1-based line of the file, \
         not a byte offset: {rendered}"
    );
}
