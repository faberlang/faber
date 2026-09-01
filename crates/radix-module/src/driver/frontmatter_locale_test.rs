//! Frontmatter locale integration tests. These require stdlib access and verify
//! that `+++\nlocale = "..."\n+++` frontmatter correctly selects reader packs
//! for keyword lexing and type surface resolution.

use super::compile;
use crate::codegen::Target;
use crate::driver::Config;
use crate::driver::ModuleCompileRequest;

#[test]
fn compile_uses_frontmatter_locale_for_lexing() {
    let config = Config::default()
        .with_target(Target::HirRust)
        ;
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"zh-Hans\"\n+++\n\n函数 salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn frontmatter_diagnostics_report_physical_line_and_body_source_line() {
    let config = Config::default()
        .with_target(Target::HirRust)
        ;
    let session = crate::driver::Session::new(config);
    let source = "+++
locale = \"en\"
+++

fn broken(int value) → int {
    return missing
}
";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == Some("SEM008"))
        .expect("undefined name diagnostic");

    assert_eq!(diagnostic.line, Some(6));
    assert_eq!(diagnostic.source_line.as_deref(), Some("    return missing"));
    assert_eq!(diagnostic.file, "locale.fab");
    assert!(diagnostic.phase.to_string().contains("analysis"));
    let rendered = crate::diagnostics::render_expanded(diagnostic);
    assert!(rendered.contains("error[SEM008:undefined_name] analysis locale.fab"));
    assert!(rendered.contains("phase: analysis"));
    assert!(rendered.contains("source: \u{2068}    return missing\u{2069}"));
}

#[test]
fn compile_frontmatter_locale_overrides_session_pack() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(super::zh_locale_pack());
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"la\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|d| d.code.is_some_and(|code| code.starts_with("LOCALE"))),
        "frontmatter locale 'la' should be used, not session zh pack (no locale diagnostics expected)"
    );
}

#[test]
fn explicit_session_pack_wins_over_frontmatter_locale() {
    // An explicitly marked session pack (CLI `--locale` semantics) is
    // authoritative: the source declares the zh-Hans frontmatter surface, but
    // the explicit `en` pack must win and lex the English `fn` keyword.
    let en_pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("installed en pack should load");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_resolved_locale_pack(en_pack);
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"zh-Hans\"\n+++\n\nfn salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_rejects_unknown_frontmatter_locale() {
    let config = Config::default()
        .with_target(Target::HirRust)
        ;
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"xx\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("LOCALE003"))
    );
}

#[test]
fn compile_frontmatter_without_locale_falls_back_to_session() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(super::zh_locale_pack());
    let session = crate::driver::Session::new(config);
    let source = "+++\nterm = \"discerne\"\n+++\n\n函数 salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_rejects_frontmatter_locale_without_locale_root() {
    // `Config::default()` now discovers the workspace language packs, so this
    // test opts out explicitly to prove the no-root error path still works.
    let config = Config { locale_root: None, ..Config::default() }.with_target(Target::HirRust);
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"zh-Hans\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());

    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("LOCALE003"))
    );
}

#[test]
fn compile_rejects_frontmatter_locale_with_parent_traversal() {
    let config = Config::default()
        .with_target(Target::HirRust)
        ;
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"../../etc/passwd\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("LOCALE003"))
    );
}

#[test]
fn compile_rejects_frontmatter_locale_empty() {
    let config = Config::default()
        .with_target(Target::HirRust)
        ;
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("LOCALE003"))
    );
}

#[test]
fn compile_rejects_frontmatter_locale_absolute_path() {
    let config = Config::default()
        .with_target(Target::HirRust)
        ;
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"/etc\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("LOCALE003"))
    );
}

#[test]
fn compile_rejects_frontmatter_locale_absolute_path_deep() {
    let config = Config::default()
        .with_target(Target::HirRust)
        ;
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"/etc/passwd\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("LOCALE003"))
    );
}

#[test]
fn compile_rejects_frontmatter_locale_with_path_separator() {
    let config = Config::default()
        .with_target(Target::HirRust)
        ;
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"la/extra\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("LOCALE003"))
    );
}

#[test]
fn compile_rejects_non_string_frontmatter_locale() {
    let config = Config::default()
        .with_target(Target::HirRust)
        ;
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = 42\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());
    assert!(!result.success());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == Some("LOCALE003"))
    );
}
