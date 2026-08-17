//! Frontmatter locale integration tests. These require stdlib access and verify
//! that `+++\nlocale = "..."\n+++` frontmatter correctly selects reader packs
//! for keyword lexing and type surface resolution.

use super::compile;
use crate::codegen::Target;
use crate::driver::Config;
use std::path::PathBuf;

fn stdlib_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib")
}

#[test]
fn compile_uses_frontmatter_locale_for_lexing() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_stdlib(stdlib_path());
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"zh-Hans\"\n+++\n\n函数 salve() { }";
    let result = compile(&session, "locale.fab", source);

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_frontmatter_locale_overrides_session_pack() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_stdlib(stdlib_path())
        .with_locale_pack(super::zh_locale_pack());
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"la\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source);

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
    let en_pack = crate::locale::LocalePack::from_toml_path(stdlib_path().join("locale/en/pack.toml"))
        .expect("installed en pack should load");
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_stdlib(stdlib_path())
        .with_locale_pack(en_pack)
        .with_locale_pack_explicit();
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"zh-Hans\"\n+++\n\nfn salve() { }";
    let result = compile(&session, "locale.fab", source);

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_rejects_unknown_frontmatter_locale() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_stdlib(stdlib_path());
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"xx\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source);

    assert!(!result.success());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == Some("LOCALE003")));
}

#[test]
fn compile_frontmatter_without_locale_falls_back_to_session() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_locale_pack(super::zh_locale_pack());
    let session = crate::driver::Session::new(config);
    let source = "+++\nterm = \"discerne\"\n+++\n\n函数 salve() { }";
    let result = compile(&session, "locale.fab", source);

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn compile_rejects_frontmatter_locale_without_stdlib() {
    // `Config::default()` now discovers the workspace stdlib, so this test
    // opts out explicitly to prove the no-stdlib error path still works.
    let config = Config { stdlib_path: None, ..Config::default() }.with_target(Target::HirRust);
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"zh-Hans\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source);

    assert!(!result.success());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == Some("LOCALE003")));
}

#[test]
fn compile_rejects_frontmatter_locale_with_parent_traversal() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_stdlib(stdlib_path());
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"../../etc/passwd\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source);
    assert!(!result.success());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == Some("LOCALE003")));
}

#[test]
fn compile_rejects_frontmatter_locale_empty() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_stdlib(stdlib_path());
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source);
    assert!(!result.success());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == Some("LOCALE003")));
}

#[test]
fn compile_rejects_frontmatter_locale_absolute_path() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_stdlib(stdlib_path());
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"/etc\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source);
    assert!(!result.success());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == Some("LOCALE003")));
}

#[test]
fn compile_rejects_frontmatter_locale_absolute_path_deep() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_stdlib(stdlib_path());
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"/etc/passwd\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source);
    assert!(!result.success());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == Some("LOCALE003")));
}

#[test]
fn compile_rejects_frontmatter_locale_with_path_separator() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_stdlib(stdlib_path());
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"la/extra\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source);
    assert!(!result.success());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == Some("LOCALE003")));
}

#[test]
fn compile_rejects_non_string_frontmatter_locale() {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_stdlib(stdlib_path());
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = 42\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source);
    assert!(!result.success());
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.code == Some("LOCALE003")));
}
