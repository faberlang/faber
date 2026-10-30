//! Frontmatter locale integration tests. These require stdlib access and verify
//! that `+++\nlocale = "..."\n+++` frontmatter correctly selects reader packs
//! for keyword lexing and type surface resolution.

use super::compile;
use crate::codegen::Target;
use crate::driver::Config;
use crate::driver::LocalePackLoader;
use crate::driver::ModuleCompileRequest;

#[cfg(feature = "hir-rust")]
#[test]
fn compile_uses_frontmatter_locale_for_lexing() {
    let config = Config::default().with_target(Target::HirRust);
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"zh-Hans\"\n+++\n\n函数 salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

/// A config with no language-pack root still reads a source in its declared
/// frontmatter locale when the product loader is installed (an installed
/// `faber` embeds the reader packs): the loader is the frontmatter locale's
/// source when the workspace `locale/` tree cannot serve the id.
#[cfg(feature = "hir-rust")]
#[test]
fn compile_uses_frontmatter_locale_through_pack_loader_without_root() {
    let loader = LocalePackLoader::new(|locale_id| match locale_id {
        "la" => Ok(crate::locale::latin_locale_pack()),
        other => Err(format!("no pack for locale '{other}'")),
    });
    let mut config = Config::default().with_target(Target::HirRust);
    config.locale_root = None;
    let config = config.with_locale_pack_loader(loader);
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"la\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());

    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn frontmatter_diagnostics_report_physical_line_and_body_source_line() {
    let config = Config::default().with_target(Target::HirRust);
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

    assert_eq!(diagnostic.issue(), Some("undefined_name"));
    assert_eq!(diagnostic.line, Some(6));
    assert_eq!(diagnostic.source_line.as_deref(), Some("    return missing"));
    assert_eq!(diagnostic.file, "locale.fab");
    assert!(diagnostic.phase.to_string().contains("analysis"));
}

#[cfg(feature = "hir-rust")]
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

#[cfg(feature = "hir-rust")]
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
    let config = Config::default().with_target(Target::HirRust);
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
fn locale_unknown_diagnostic_carries_the_locale_id_it_names() {
    // LOCALE003's pack message is `unknown or invalid reader locale
    // '{locale_id}'`: every emit site supplies the arg.
    let config = Config::default().with_target(Target::HirRust);
    let session = crate::driver::Session::new(config);
    let source = "+++\nlocale = \"xx\"\n+++\n\nfunctio salve() { }";
    let result = compile(&session, "locale.fab", source, ModuleCompileRequest::default());

    let diagnostic = result
        .diagnostics
        .iter()
        .find(|d| d.code == Some("LOCALE003"))
        .expect("LOCALE003 for an unknown frontmatter locale");
    assert_eq!(
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == "locale_id")
            .map(|arg| arg.value.as_str()),
        Some("xx")
    );
}

#[cfg(feature = "hir-rust")]
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
    let config = Config::default().with_target(Target::HirRust);
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
    let config = Config::default().with_target(Target::HirRust);
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
    let config = Config::default().with_target(Target::HirRust);
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
    let config = Config::default().with_target(Target::HirRust);
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
    let config = Config::default().with_target(Target::HirRust);
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
    let config = Config::default().with_target(Target::HirRust);
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

/// LOCALE003 emit sites supply `locale_id` where the id is known and an
/// `issue` row where it is not, so the `en` pack renders every variant
/// without a missing-placeholder fallback.
#[test]
fn locale_unknown_diagnostics_render_through_the_en_pack() {
    let pack = crate::locale::LocalePack::from_toml_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locale/en/pack.toml"),
    )
    .expect("en pack");
    for (id, issue, expect) in [
        (
            "../../etc/passwd",
            "invalid_id",
            "'../../etc/passwd' is not a valid locale identifier",
        ),
        ("", "invalid_id", "is not a valid locale identifier"),
        ("zz-nope", "no_root", "'zz-nope' but no language-pack root"),
    ] {
        let source = format!("+++\nlocale = \"{id}\"\n+++\n\nfn salve() {{ }}");
        let mut config = Config::default().with_target(Target::HirRust);
        if issue == "no_root" {
            config.locale_root = None;
        }
        let session = crate::driver::Session::new(config);
        let result = compile(&session, "locale.fab", &source, ModuleCompileRequest::default());
        let diagnostic = result
            .diagnostics
            .iter()
            .find(|d| d.code == Some("LOCALE003"))
            .unwrap_or_else(|| panic!("no LOCALE003 for {id:?}: {:?}", result.diagnostics));
        assert!(
            diagnostic
                .args
                .iter()
                .any(|a| a.name == "locale_id" && a.value == id),
            "locale_id arg for {id:?}: {:?}",
            diagnostic.args
        );
        let rendered = pack
            .render_diagnostic_text(diagnostic)
            .expect("renders without a missing placeholder")
            .expect("pack row");
        assert!(rendered.message.contains(expect), "{issue}: {}", rendered.message);
    }
}
