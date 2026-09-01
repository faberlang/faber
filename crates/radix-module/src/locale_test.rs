//! Tests for reader locale pack validation, fallback data, and
//! template rendering errors.
//! Proves: pack validation; fallback and suggestion data generation;
//! template render error propagation.

use super::{
    DiagnosticTemplateRenderError, LOCALE_PACK_SCHEMA_VERSION, LocalePack, LocalePackIssue, TemplateField,
    format_locale_pack_load_error, latin_locale_pack, load_library_locale_pack, merge_library_locale_packs,
    validate_locale_pack,
};
use crate::codegen::Target;
use crate::diagnostics::DiagnosticConvert;
use crate::diagnostics::{Diagnostic, DiagnosticArg};
use crate::driver::{Config, ModuleCompileRequest, Session, compile};
use crate::file_interface::{InterfaceLibraryIdentity, InterfaceMemberKind};
use crate::lexer::{LexError, LexErrorKind, LocaleSuggestion, Span, TokenKind, lex_with_locale_pack};
use crate::locale::TypeBindingKind;
use crate::parser::{ParseError, ParseErrorKind, parse};
use crate::semantic::{CollectionKind, Primitive, SemanticError, SemanticErrorKind, TensorFamilyKind};
use crate::syntax::{BindingPattern, ExprKind, StmtKind};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn temp_pack_path(label: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = crate::test_support::test_temp_dir(&format!("reader-pack-{label}"));
    let path = dir.path().join("pack.toml");
    (dir, path)
}

fn installed_pack(locale: &str) -> LocalePack {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push(format!("../../stdlib/locale/{locale}/pack.toml"));
    LocalePack::from_toml_path(path).unwrap_or_else(|err| panic!("installed {locale} pack should load: {err:?}"))
}

/// Load a pack's **owned** rows from the raw file contents.
///
/// WHY: `LocalePack::from_toml_path` merges `inherits = "la"` parent rows
/// (`merge_inherited_pack`), so a completeness comparison over loaded packs
/// would pass vacuously for the non-`la` packs — precisely the inheritance
/// substitute coverage-gaps D1 forbids. `from_toml_str` keeps only the rows
/// the pack owns.
fn owned_pack(locale: &str) -> LocalePack {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push(format!("../../stdlib/locale/{locale}/pack.toml"));
    let source = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read installed {locale} pack: {err:?}"));
    LocalePack::from_toml_str(&source)
        .unwrap_or_else(|err| panic!("installed {locale} pack should parse owned rows: {err:?}"))
}

fn diagnostic_arg(name: &'static str, value: impl Into<String>) -> DiagnosticArg {
    DiagnosticArg::new(name, value)
}

fn diagnostic_with_code(code: &'static str, args: Vec<DiagnosticArg>) -> Diagnostic {
    Diagnostic::error(code).with_code(code).with_args(args)
}

fn assert_issue_template(pack: &LocalePack, code: &str, issue: &str) {
    assert!(
        pack.diagnostics
            .get(code)
            .and_then(|template| template.issues.get(issue))
            .is_some(),
        "missing {code}.{issue} issue template in {}",
        pack.metadata.id
    );
}

fn pack_key_set<'a>(keys: impl Iterator<Item = &'a String>) -> BTreeSet<String> {
    keys.cloned().collect()
}

/// The lexer registry retains `conversion` as an identity alias for tooling,
/// but the sealed English and Latin reader surfaces intentionally omit it.
/// Completeness compares each installed pack with the scaffold after applying
/// that settled per-locale surface exception; no pack row is synthesized here.
const SEALED_CONVERSIO_ALIAS_OMISSIONS: &[(&str, &str)] = &[("en", "conversion"), ("la", "conversion")];

fn is_sealed_conversio_alias_omission(pack_id: &str, section: &str, key: &str) -> bool {
    section == "keyword"
        && SEALED_CONVERSIO_ALIAS_OMISSIONS
            .iter()
            .any(|&(sealed_locale, omitted_key)| sealed_locale == pack_id && omitted_key == key)
}

fn assert_pack_keys_match(pack_id: &str, section: &str, mut expected: BTreeSet<String>, actual: BTreeSet<String>) {
    for &(_, omitted_key) in SEALED_CONVERSIO_ALIAS_OMISSIONS {
        if is_sealed_conversio_alias_omission(pack_id, section, omitted_key) {
            expected.remove(omitted_key);
        }
    }
    let missing: Vec<_> = expected.difference(&actual).cloned().collect();
    let extra: Vec<_> = actual.difference(&expected).cloned().collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "installed {pack_id} {section} keys must match the generated reader-pack scaffold; missing={missing:?}; extra={extra:?}"
    );
}

fn lex_diagnostic(kind: LexErrorKind, args: Vec<DiagnosticArg>) -> Diagnostic {
    let err = LexError { kind, span: Span { start: 0, end: 1 }, args };
    Diagnostic::from_lex_error("lexer.fab", "?\n", &err)
}

fn parse_diagnostic(kind: ParseErrorKind, args: Vec<DiagnosticArg>) -> Diagnostic {
    let mut err = ParseError::structured(kind, Span { start: 0, end: 1 });
    for arg in args {
        err = err.with_arg(arg.name, arg.value);
    }
    Diagnostic::from_parse_error("parser.fab", "?\n", &err)
}

fn semantic_diagnostic(kind: SemanticErrorKind, args: Vec<DiagnosticArg>) -> Diagnostic {
    let mut err = SemanticError::structured(kind, Span { start: 0, end: 1 });
    for arg in args {
        err = err.with_arg(arg.name, arg.value);
    }
    Diagnostic::from_semantic_error("semantic.fab", "?\n", &err)
}

#[test]
fn locale_pack_completeness_la_matches_generated_scaffold() {
    let scaffold = latin_locale_pack();
    let installed = installed_pack("la");

    assert!(validate_locale_pack(&installed).is_empty(), "installed la pack should validate");
    assert_pack_keys_match(
        "la",
        "keyword",
        pack_key_set(scaffold.keywords.keys()),
        pack_key_set(installed.keywords.keys()),
    );
    assert_pack_keys_match(
        "la",
        "type",
        pack_key_set(scaffold.types.keys()),
        pack_key_set(installed.types.keys()),
    );
    assert_pack_keys_match(
        "la",
        "diagnostic",
        pack_key_set(scaffold.diagnostics.keys()),
        pack_key_set(installed.diagnostics.keys()),
    );
}

/// Every installed pack owns the full scaffold `keyword` + `type` key sets
/// (coverage-gaps D1: `inherits`/`fallback` are not substitutes).
///
/// Compared on **owned** rows (`from_toml_str` on the raw file) — a merged-key
/// comparison would pass vacuously because `from_toml_path` folds `inherits =
/// "la"` rows into the non-`la` packs. Diagnostics stay `la`-scoped (the six
/// non-`la` packs legitimately miss LOCALE003 + PARSE060; that is
/// `reader-locale-coverage-gaps` territory, not this gate).
#[test]
fn locale_pack_completeness_all_installed_packs_own_scaffold_surface_keys() {
    let scaffold = latin_locale_pack();
    for locale in ["la", "en", "th-TH", "ar", "zh-Hans", "zh-Hant", "vi", "hi"] {
        let installed = owned_pack(locale);
        assert!(
            validate_locale_pack(&installed).is_empty(),
            "installed {locale} pack should validate"
        );
        assert_pack_keys_match(
            locale,
            "keyword",
            pack_key_set(scaffold.keywords.keys()),
            pack_key_set(installed.keywords.keys()),
        );
        assert_pack_keys_match(
            locale,
            "type",
            pack_key_set(scaffold.types.keys()),
            pack_key_set(installed.types.keys()),
        );
    }
}

#[test]
fn latin_pack_projects_existing_registries() {
    let pack = latin_locale_pack();

    assert_eq!(pack.metadata.id, "la");
    assert_eq!(pack.metadata.schema_version, LOCALE_PACK_SCHEMA_VERSION);
    assert_eq!(pack.keywords["functio"].canonical, "functio");
    assert_eq!(pack.keywords["si"].canonical, "si");
    assert_eq!(pack.types["textus"].canonical, "textus");
    assert_eq!(pack.types["lista"].canonical, "lista");
    assert!(pack.diagnostics.contains_key("PARSE011"));
    assert!(pack.diagnostics.contains_key("SEM012"));
    assert!(validate_locale_pack(&pack).is_empty());
}

#[test]
fn generated_latin_pack_templates_render_as_safe_code_fallbacks() {
    let pack = latin_locale_pack();
    let diagnostic = Diagnostic::error("SEM010").with_code("SEM010");

    let rendered = pack
        .render_diagnostic_text(&diagnostic)
        .expect("generated Latin template should render")
        .expect("known catalog code should have a template");

    assert_eq!(rendered.message, "SEM010");
    assert_eq!(
        rendered.help.as_deref(),
        Some("make the expression type match the expected type")
    );
}

#[test]
fn toml_pack_loads_surface_rows_aliases_and_llm_artifacts() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "th-TH"
fallback = ["la"]

[keywords]
functio = "ฟังก์ชัน"
si = "ถ้า"
sin = "ถ้าไม่"
secus = "มิฉะนั้น"

[keyword_aliases]
functio = ["ฟังก์ชั่น"]

[types]
textus = "ข้อความ"

[type_aliases]
textus = ["สายอักขระ"]

[diagnostics.PARSE011]
message = "MSG:{functio}"
help = "HELP:{textus}"

[llm]
system_prompt_snippet = "emit Thai reader-locale Faber"
exemplars = ["./exemplars/salve-munde.th-TH.fab"]
"#,
    )
    .expect("pack should validate");

    assert_eq!(pack.metadata.id, "th-TH");
    assert_eq!(pack.metadata.fallback, vec!["la"]);
    assert_eq!(pack.keywords["functio"].canonical, "ฟังก์ชัน");
    assert_eq!(pack.keywords["functio"].aliases, vec!["ฟังก์ชั่น"]);
    assert_eq!(pack.types["textus"].aliases, vec!["สายอักขระ"]);
    assert_eq!(
        pack.type_binding_kind("ข้อความ"),
        Some(TypeBindingKind::Primitive(Primitive::Textus))
    );
    assert_eq!(
        pack.type_binding_kind("สายอักขระ"),
        Some(TypeBindingKind::Primitive(Primitive::Textus))
    );
    assert_eq!(pack.diagnostics["PARSE011"].message, "MSG:{functio}");
    assert_eq!(pack.llm.exemplars, vec!["./exemplars/salve-munde.th-TH.fab"]);
}

#[test]
fn toml_pack_loads_scoped_library_member_rows() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "en"
fallback = ["la"]

[[library_members]]
provider = "package"
package = "demo"
module_path = ["math"]
kind = "function"
canonical = "summa"
surface = "sum"

[[library_members]]
provider = "package"
package = "demo"
module_path = ["math"]
owner = "Vector"
kind = "method"
canonical = "longitudo"
surface = "length"

[llm]
system_prompt_snippet = "emit English reader-locale Faber"
exemplars = ["./exemplars/salve-munde.en.fab"]
"#,
    )
    .expect("library-member rows should validate");

    let identity = InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("demo".to_owned()),
        module_path: vec!["math".to_owned()],
    };
    assert_eq!(
        pack.library_member_surface(&identity, None, InterfaceMemberKind::Function, "summa"),
        Some("sum")
    );
    assert_eq!(
        pack.library_member_canonical(&identity, None, InterfaceMemberKind::Function, "sum"),
        Some("summa")
    );
    assert_eq!(
        pack.library_member_surface(&identity, Some("Vector"), InterfaceMemberKind::Method, "longitudo"),
        Some("length")
    );
    assert!(pack.defines_library_identity(&identity));
    assert!(!pack.defines_library_identity(&InterfaceLibraryIdentity {
        provider: "package".to_owned(),
        package: Some("other".to_owned()),
        module_path: vec!["math".to_owned()],
    }));
}

#[test]
fn library_member_surface_collisions_are_rejected_per_owner_scope() {
    let err = LocalePack::from_toml_str(
        r#"
[pack]
id = "en"

[[library_members]]
provider = "package"
package = "demo"
module_path = ["math"]
owner = "Vector"
kind = "method"
canonical = "longitudo"
surface = "length"

[[library_members]]
provider = "package"
package = "demo"
module_path = ["math"]
owner = "Vector"
kind = "method"
canonical = "latitudo"
surface = "length"

[llm]
system_prompt_snippet = "emit English reader-locale Faber"
exemplars = ["salve-munde.en.fab"]
"#,
    )
    .expect_err("duplicate owner surface should fail validation");
    let super::LocalePackLoadError::Validation(issues) = err else {
        panic!("expected validation errors");
    };
    assert!(issues.iter().any(|issue| matches!(
        issue,
        LocalePackIssue::DuplicateLibraryMemberSurface { owner: Some(owner), surface, .. }
            if owner == "Vector" && surface == "length"
    )));
}

#[test]
fn toml_pack_loads_from_path_and_checks_exemplars() {
    let (_dir, path) = temp_pack_path("load");
    let exemplar_dir = path.parent().expect("temp path parent").join("exemplars");
    std::fs::create_dir_all(&exemplar_dir).expect("create exemplar dir");
    std::fs::write(exemplar_dir.join("salve-munde.fab"), "incipit { nota \"ok\" }").expect("write exemplar");
    std::fs::write(
        &path,
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[keywords]
functio = "functio"

[diagnostics.PARSE011]
message = "MSG:{functio}"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("write pack");

    let pack = LocalePack::from_toml_path(&path).expect("load pack");

    assert_eq!(pack.metadata.id, "la-test");
    assert_eq!(pack.keywords["functio"].canonical, "functio");
}

#[test]
fn toml_pack_from_path_rejects_missing_exemplar_artifacts() {
    let (_dir, path) = temp_pack_path("missing-exemplar");
    std::fs::write(
        &path,
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[keywords]
functio = "functio"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/missing.fab"]
"#,
    )
    .expect("write pack");

    let err = LocalePack::from_toml_path(&path).expect_err("missing exemplar should fail");
    let super::LocalePackLoadError::Validation(issues) = err else {
        panic!("expected validation error");
    };
    assert!(
        issues
            .iter()
            .any(|issue| matches!(issue, LocalePackIssue::MissingLlmExemplar { .. }))
    );
}

#[test]
fn path_loaded_pack_inherits_parent_rows() {
    let (_dir, pack_path) = temp_pack_path("inherit");
    let root = pack_path.parent().expect("temp path parent").to_path_buf();
    let parent_dir = root.join("parent");
    let child_dir = root.join("child");
    std::fs::create_dir_all(&parent_dir).expect("create parent");
    std::fs::create_dir_all(&child_dir).expect("create child");
    std::fs::write(
        parent_dir.join("pack.toml"),
        r#"
[pack]
id = "parent"
fallback = ["la"]

[keywords]
functio = "KW_PARENT"

[diagnostics.PARSE011]
message = "PARENT:{functio}"

[llm]
system_prompt_snippet = "emit parent"
exemplars = ["./salve.fab"]
"#,
    )
    .expect("write parent");
    std::fs::write(parent_dir.join("salve.fab"), "KW_PARENT salve {}").expect("write parent exemplar");
    std::fs::write(
        child_dir.join("pack.toml"),
        r#"
[pack]
id = "child"
inherits = "parent"
fallback = ["la"]

[keywords]
nota = "KW_NOTA"

[diagnostics.SEM001]
message = "CHILD:{spelling}"

[llm]
system_prompt_snippet = "emit child"
exemplars = ["./salve.fab"]
"#,
    )
    .expect("write child");
    std::fs::write(child_dir.join("salve.fab"), "KW_NOTA \"ok\"").expect("write child exemplar");

    let pack = LocalePack::from_toml_path(child_dir.join("pack.toml")).expect("load child pack");

    assert_eq!(pack.keywords["functio"].canonical, "KW_PARENT");
    assert_eq!(pack.keywords["nota"].canonical, "KW_NOTA");
    assert!(pack.diagnostics.contains_key("PARSE011"));
    assert!(pack.diagnostics.contains_key("SEM001"));
}

#[test]
fn alias_tables_can_define_surface_rows() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keyword_aliases]
functio = ["函数", "函數"]

[type_aliases]
textus = ["文本", "文字"]

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");

    assert_eq!(pack.keywords["functio"].canonical, "函数");
    assert_eq!(pack.keywords["functio"].aliases, vec!["函數"]);
    assert_eq!(pack.types["textus"].canonical, "文本");
    assert_eq!(pack.types["textus"].aliases, vec!["文字"]);
}

#[test]
fn pack_keyword_lookup_resolves_canonical_alias_suggestions() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = { canonical = "函数", aliases = ["函數"] }
incipit = "入口"
si = "如果"
sin = "否则如果"
secus = "否则"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");

    assert_eq!(pack.keyword_token_kind("入口"), Some(TokenKind::Incipit));
    assert_eq!(pack.keyword_token_kind("函数"), Some(TokenKind::Functio));
    assert_eq!(pack.keyword_token_kind("functio"), None);
    assert_eq!(pack.keyword_suggestion("函教"), Some("函数".to_string()));
}

/// PRP-055: long ordinary identifiers fall outside the pack length window and
/// return `None` without an edit-distance hit (fast-reject pre-check).
#[test]
fn keyword_suggestion_fast_rejects_obvious_non_keywords() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = { canonical = "函数", aliases = ["函數"] }
si = "如果"
sin = "否则如果"
secus = "否则"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");

    // Far longer than any keyword spelling ± threshold → pack-level reject.
    assert_eq!(pack.keyword_suggestion("this_is_a_perfectly_ordinary_identifier_name"), None);
    // Far shorter / empty-like relative to multi-char keywords still eligible by
    // length may return None after distance scoring — length gate must not
    // invent suggestions for unrelated short tokens either.
    assert_eq!(pack.keyword_suggestion("xyzzy_not_a_keyword_at_all"), None);
}

/// PRP-055: near-miss typos within the distance threshold still suggest the
/// canonical spelling after the length gate (no semantic change).
#[test]
fn keyword_suggestion_preserves_near_miss_and_exact_fold_hits() {
    let pack = latin_locale_pack();

    // One-edit miss on a long keyword (threshold 2) still suggests.
    assert_eq!(pack.keyword_suggestion("functiu"), Some("functio".to_string()));
    // One-edit miss on another long keyword.
    assert_eq!(pack.keyword_suggestion("ceterom"), Some("ceterum".to_string()));
    // Distinctive multi-char typo (not shared with other short keywords).
    assert_eq!(pack.keyword_suggestion("functiio"), Some("functio".to_string()));
    // Unrelated short token: distance beyond short-keyword threshold stays None.
    assert_eq!(pack.keyword_suggestion("qq"), None);
    // Far-too-long identifier: pack-level length gate rejects.
    assert_eq!(pack.keyword_suggestion("supercalifragilisticexpialidocious_ident"), None);

    let english = installed_pack("en");
    for ident in ["n", "a", "i", "r", "f", "o", "s"] {
        assert_eq!(
            english.keyword_suggestion(ident),
            None,
            "one-character identifier {ident:?} must not be suggested as a keyword typo"
        );
    }

    // Chinese near-miss still works on a localized pack (exact-fold path unchanged).
    let zh = LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = { canonical = "函数", aliases = ["函數"] }
si = "如果"
sin = "否则如果"
secus = "否则"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");
    assert_eq!(zh.keyword_suggestion("函教"), Some("函数".to_string()));
}

#[test]
fn pack_renders_diagnostic_templates_with_args_and_surface_placeholders() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[keywords]
functio = "KW_FUNCTIO"

[types]
textus = "TY_TEXTUS"

[diagnostics.PARSE011]
message = "MSG:{name}:{functio}"
help = "HELP:{textus}"

[llm]
system_prompt_snippet = "emit test Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("pack should validate");
    let diagnostic = diagnostic_with_code("PARSE011", vec![diagnostic_arg("name", "salve")]);

    let rendered = pack
        .render_diagnostic_text(&diagnostic)
        .expect("template should render")
        .expect("template row should exist");

    assert_eq!(rendered.message, "MSG:salve:KW_FUNCTIO");
    assert_eq!(rendered.help.as_deref(), Some("HELP:TY_TEXTUS"));
}

#[test]
fn pack_renders_optional_template_fragments_by_arg_presence() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.SEM017]
message = "MSG:{method}{?reason|:|}"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("pack should validate");

    let with_reason = diagnostic_with_code(
        "SEM017",
        vec![diagnostic_arg("method", "future"), diagnostic_arg("reason", "backend")],
    );
    let without_reason = diagnostic_with_code("SEM017", vec![diagnostic_arg("method", "future")]);

    assert_eq!(
        pack.render_diagnostic_text(&with_reason)
            .expect("template should render")
            .expect("template row should exist")
            .message,
        "MSG:future:backend"
    );
    assert_eq!(
        pack.render_diagnostic_text(&without_reason)
            .expect("template should render")
            .expect("template row should exist")
            .message,
        "MSG:future"
    );
}

#[test]
fn issue_templates_take_precedence_and_code_template_is_fallback() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.SEM010]
message = "CODE:{expected}:{actual}"
help = "CODEHELP"

[diagnostics.SEM010.issues.initializer_annotation_mismatch]
message = "ISSUE:{expected}:{actual}"
help = "ISSUEHELP"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("pack should validate");

    let issue = diagnostic_with_code(
        "SEM010",
        vec![
            diagnostic_arg("issue", "initializer_annotation_mismatch"),
            diagnostic_arg("expected", "numerus"),
            diagnostic_arg("actual", "textus"),
        ],
    );
    let fallback = diagnostic_with_code(
        "SEM010",
        vec![
            diagnostic_arg("issue", "other_type_mismatch"),
            diagnostic_arg("expected", "numerus"),
            diagnostic_arg("actual", "textus"),
        ],
    );

    let issue_render = pack
        .render_diagnostic_text(&issue)
        .expect("issue template should render")
        .expect("issue row should exist");
    let fallback_render = pack
        .render_diagnostic_text(&fallback)
        .expect("fallback template should render")
        .expect("code row should exist");

    assert_eq!(issue_render.message, "ISSUE:numerus:textus");
    assert_eq!(issue_render.help.as_deref(), Some("ISSUEHELP"));
    assert_eq!(fallback_render.message, "CODE:numerus:textus");
    assert_eq!(fallback_render.help.as_deref(), Some("CODEHELP"));
}

#[test]
fn pack_render_returns_none_when_code_has_no_template_row() {
    let pack = latin_locale_pack();
    let diagnostic = Diagnostic::error("uncoded").with_code("NOPE999");

    assert_eq!(pack.render_diagnostic_text(&diagnostic), Ok(None));
}

#[test]
fn normalized_diagnostics_render_against_installed_pack_rows_without_prose_assertions() {
    for locale in ["zh-Hans", "zh-Hant", "ar", "hi", "vi"] {
        let pack = installed_pack(locale);
        assert_issue_template(&pack, "LEX004", "unexpected_character");
        assert_issue_template(&pack, "SEM001", "unknown_identifier");
        assert_issue_template(&pack, "SEM010", "initializer_annotation_mismatch");

        let diagnostics = [
            lex_diagnostic(
                LexErrorKind::CharacterUnexpected,
                vec![
                    diagnostic_arg("issue", "unexpected_character"),
                    diagnostic_arg("char", "１"),
                ],
            ),
            semantic_diagnostic(
                SemanticErrorKind::VariableUndefined,
                vec![
                    diagnostic_arg("issue", "unknown_identifier"),
                    diagnostic_arg("spelling", "nomen"),
                ],
            ),
            semantic_diagnostic(
                SemanticErrorKind::TypeMismatch,
                vec![
                    diagnostic_arg("issue", "initializer_annotation_mismatch"),
                    diagnostic_arg("expected", "numerus"),
                    diagnostic_arg("actual", "textus"),
                ],
            ),
        ];

        for diagnostic in diagnostics {
            assert!(diagnostic.code.is_some(), "{locale} diagnostic should be coded");
            assert!(diagnostic.issue().is_some(), "{locale} diagnostic should carry an issue");
            let rendered = pack
                .render_diagnostic_text(&diagnostic)
                .unwrap_or_else(|err| panic!("{locale} template should render: {err:?}"))
                .unwrap_or_else(|| panic!("{locale} template row should exist for {:?}", diagnostic.lookup_key()));
            assert!(!rendered.message.is_empty());
        }
    }
}

#[test]
fn installed_latin_pack_has_structural_templates_for_catalog_codes() {
    let pack = latin_locale_pack();
    for diagnostic in [
        parse_diagnostic(ParseErrorKind::FunctionNameAbsent, vec![diagnostic_arg("functio", "functio")]),
        parse_diagnostic(
            ParseErrorKind::ExpressionInvalid,
            vec![diagnostic_arg("issue", "type_call_constructor")],
        ),
        semantic_diagnostic(
            SemanticErrorKind::TypeMismatch,
            vec![
                diagnostic_arg("issue", "initializer_annotation_mismatch"),
                diagnostic_arg("expected", "numerus"),
                diagnostic_arg("actual", "textus"),
            ],
        ),
    ] {
        assert!(
            pack.diagnostics
                .contains_key(diagnostic.code.expect("coded diagnostic"))
        );
    }
}

#[test]
fn reader_suggestion_code_is_known_to_pack_templates() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "zh-Hans"
fallback = ["la"]

[keywords]
functio = "函数"

[diagnostics.LOCALE002]
message = "SUGGEST:{spelling}:{suggestion}:{pack}"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.zh-Hans.fab"]
"#,
    )
    .expect("pack should validate");
    let suggestion = Diagnostic::from_locale_suggestion(
        "reader.fab",
        "函教 salve {}",
        &LocaleSuggestion {
            span: Span { start: 0, end: 6 },
            spelling: "函教".to_string(),
            suggestion: "函数".to_string(),
            pack_id: "zh-Hans".to_string(),
        },
    );

    assert_eq!(
        pack.render_diagnostic_text(&suggestion)
            .expect("suggestion should render")
            .expect("suggestion row should exist")
            .message,
        "SUGGEST:函教:函数:zh-Hans"
    );
}

#[test]
fn installed_nonlatin_packs_keep_required_structural_rows() {
    for locale in ["th-TH", "zh-Hans", "zh-Hant", "ar", "hi", "vi"] {
        let pack = installed_pack(locale);
        assert_eq!(pack.metadata.id, locale);
        assert!(validate_locale_pack(&pack).is_empty(), "{locale} pack should validate");
        assert!(pack.diagnostics.contains_key("LOCALE002"));
        assert_issue_template(&pack, "LEX004", "unexpected_character");
        assert_issue_template(&pack, "SEM001", "unknown_identifier");
    }
}

#[test]
fn installed_nonlatin_sem010_issue_rows_are_structural_smoke_not_prose_tables() {
    for locale in ["zh-Hans", "zh-Hant", "ar", "hi", "vi"] {
        let pack = installed_pack(locale);
        assert_issue_template(&pack, "SEM010", "initializer_annotation_mismatch");
    }
}

#[test]
fn pack_render_reports_missing_template_placeholder() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.PARSE011]
message = "MSG:{missing}"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("pack should validate");
    let diagnostic = diagnostic_with_code("PARSE011", vec![]);

    assert_eq!(
        pack.render_diagnostic_text(&diagnostic)
            .expect_err("missing placeholder should fail"),
        DiagnosticTemplateRenderError::MissingPlaceholder {
            code: "PARSE011".to_string(),
            field: TemplateField::Message,
            placeholder: "missing".to_string(),
        }
    );
}

#[test]
fn keyword_group_members_must_be_localized_together() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[keywords]
si = "IF"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect_err("partial keyword group should fail");
    let super::LocalePackLoadError::Validation(issues) = pack else {
        panic!("expected validation error");
    };

    assert!(
        issues.iter().any(
            |issue| matches!(issue, LocalePackIssue::MissingKeywordGroupMember { keyword, .. } if keyword == "sin")
        )
    );
    assert!(issues.iter().any(
        |issue| matches!(issue, LocalePackIssue::MissingKeywordGroupMember { keyword, .. } if keyword == "secus")
    ));
}

#[test]
fn diagnostic_templates_reject_positional_and_malformed_placeholders() {
    for (source, expected) in [
        (
            r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.PARSE011]
message = "MSG:{0}"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
            LocalePackIssue::PositionalTemplatePlaceholder {
                code: "PARSE011".to_string(),
                field: TemplateField::Message,
                placeholder: "0".to_string(),
            },
        ),
        (
            r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.PARSE011]
message = "MSG:{"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
            LocalePackIssue::UnclosedTemplatePlaceholder {
                code: "PARSE011".to_string(),
                field: TemplateField::Message,
            },
        ),
    ] {
        let err = LocalePack::from_toml_str(source).expect_err("template should fail validation");
        let super::LocalePackLoadError::Validation(issues) = err else {
            panic!("expected validation error");
        };
        assert!(issues.contains(&expected), "missing {expected:?} in {issues:?}");
    }
}

#[test]
fn unknown_registry_keys_are_reported_as_schema_issues() {
    let err = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la-test"]

[keywords]
unknown_keyword = "KW"

[types]
unknown_type = "TY"

[diagnostics.NOPE999]
message = "MSG"

[llm]
system_prompt_snippet = ""
exemplars = []
"#,
    )
    .expect_err("unknown keys should fail");
    let super::LocalePackLoadError::Validation(issues) = err else {
        panic!("expected validation error");
    };

    assert!(issues.contains(&LocalePackIssue::SelfFallback { id: "la-test".to_string() }));
    assert!(issues.contains(&LocalePackIssue::UnknownKeyword { key: "unknown_keyword".to_string() }));
    assert!(issues.contains(&LocalePackIssue::UnknownType { key: "unknown_type".to_string() }));
    assert!(issues.contains(&LocalePackIssue::UnknownDiagnosticCode { code: "NOPE999".to_string() }));
    assert!(issues.contains(&LocalePackIssue::EmptyLlmPromptSnippet));
    assert!(issues.contains(&LocalePackIssue::EmptyLlmExemplars));
}

// === Phase 0: reverse-lookup surface accessors (reader-locale-emit goal) ===

#[test]
fn latin_pack_surfaces_resolve_to_latin_with_no_fallback() {
    let pack = latin_locale_pack();

    // Keyword kinds resolve to their Latin spelling (the default surface).
    assert_eq!(pack.keyword_surface(TokenKind::Incipit), Some("incipit"));
    // Batch E words are tokenless; their emit surface resolves by Latin key.
    for key in [
        "discretio",
        "fixum",
        "functio",
        "genus",
        "implendum",
        "ordo",
        "sit",
        "typus",
        "varia",
    ] {
        assert_eq!(pack.contextual_keyword_surface(key), key, "{key} surface should be {key}");
    }
    // Batch C words are tokenless; their emit surface resolves by Latin key.
    assert_eq!(pack.contextual_keyword_surface("nihil"), "nihil");
    assert_eq!(pack.contextual_keyword_surface("verum"), "verum");
    assert_eq!(pack.contextual_keyword_surface("falsum"), "falsum");
    // `vacua` is a tokenless contextual keyword; its emit surface resolves by
    // Latin key exactly like the batch C words.
    assert_eq!(pack.contextual_keyword_surface("vacua"), "vacua");

    // Type identities resolve to their canonical Latin spelling.
    assert_eq!(pack.type_surface(TypeBindingKind::Primitive(Primitive::Textus)), "textus");
    assert_eq!(pack.type_surface(TypeBindingKind::Primitive(Primitive::Numerus)), "numerus");
    assert_eq!(pack.type_surface(TypeBindingKind::Collection(CollectionKind::Lista)), "lista");
    assert_eq!(pack.type_surface(TypeBindingKind::Collection(CollectionKind::Tabula)), "tabula");

    // The tensor family resolves to its identity spellings on the Latin pack.
    assert_eq!(
        pack.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Tensor)),
        "tensor"
    );
    assert_eq!(
        pack.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Vector)),
        "vector"
    );
    assert_eq!(
        pack.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Matrix)),
        "matrix"
    );
    assert_eq!(
        pack.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Sparsa)),
        "sparsa"
    );
    assert_eq!(
        pack.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Atomic)),
        "atomic"
    );
    assert_eq!(
        pack.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Intervallum)),
        "intervallum"
    );

    // The Latin pack is the default surface; nothing falls back.
    assert_eq!(pack.fallback_count(), 0);
    assert!(pack.keyword_fallbacks().is_empty());
    assert!(pack.type_fallbacks().is_empty());
}

#[test]
fn localized_pack_keyword_surface_resolves_and_round_trips() {
    let pack = installed_pack("th-TH");

    // Localized surface spelling for `incipit`.
    let surface = pack
        .keyword_surface(TokenKind::Incipit)
        .expect("incipit has a localized row");
    assert_eq!(surface, "เริ่ม");

    // Round-trip invariant: the localized surface re-lexes to the same kind.
    // This is the reader-locale losslessness contract at the accessor level.
    assert_eq!(pack.keyword_token_kind(surface), Some(TokenKind::Incipit));

    // A non-keyword kind (literal) has no reader surface.
    assert_eq!(pack.keyword_surface(TokenKind::Integer(0)), None);
}

#[test]
fn localized_pack_type_surface_resolves_and_round_trips() {
    let pack = installed_pack("th-TH");

    let textus = TypeBindingKind::Primitive(Primitive::Textus);
    let surface = pack.type_surface(textus);
    assert_eq!(surface, "ข้อความ");

    // Round-trip: localized type surface re-lexes to the same type identity.
    assert_eq!(pack.type_binding_kind(surface), Some(textus));
}

#[test]
fn partial_pack_reports_keyword_and_type_fallbacks() {
    // A pack that localizes only a few rows and does NOT inherit `la`, so its
    // coverage gap is real and visible (req. #10). `from_toml_str` checks
    // structural validity (non-empty exemplars) but not exemplar path existence.
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "th-partial"
fallback = ["la"]

[keywords]
functio = "ฟังก์ชัน"

[types]
textus = "ข้อความ"

[llm]
system_prompt_snippet = "partial pack"
exemplars = ["./salve-munde.fab"]
"#,
    )
    .expect("partial pack should validate");

    // Localized rows resolve to the Thai surface (`functio` is tokenless, so
    // its emit surface resolves by Latin key).
    assert_eq!(pack.contextual_keyword_surface("functio"), "ฟังก์ชัน");
    assert_eq!(pack.type_surface(TypeBindingKind::Primitive(Primitive::Textus)), "ข้อความ");

    // A keyword with no localized row falls back to Latin visibly.
    assert_eq!(pack.keyword_surface(TokenKind::Incipit), Some("incipit"));
    assert!(pack.keyword_fallbacks().contains(&"incipit"));

    // A type with no localized row falls back to Latin visibly.
    assert_eq!(pack.type_surface(TypeBindingKind::Primitive(Primitive::Numerus)), "numerus");
    assert!(
        pack.type_fallbacks()
            .contains(&TypeBindingKind::Primitive(Primitive::Numerus))
    );

    // Coverage gap is reported, not silent.
    assert!(pack.fallback_count() > 0);
    assert!(!pack.keyword_fallbacks().is_empty());
    assert!(!pack.type_fallbacks().is_empty());
}

#[test]
fn render_extra_args_ignored_silently() {
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[diagnostics.PARSE011]
message = "MSG:{name}"

[llm]
system_prompt_snippet = "emit Latin Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("pack should validate");
    let diagnostic = diagnostic_with_code(
        "PARSE011",
        vec![
            diagnostic_arg("name", "salve"),
            diagnostic_arg("extra_unused", "ignored"),
        ],
    );

    let rendered = pack
        .render_diagnostic_text(&diagnostic)
        .expect("template should render")
        .expect("template row should exist");

    assert_eq!(rendered.message, "MSG:salve");
}

#[test]
fn keyword_token_kind_and_type_binding_use_spelling_index() {
    let pack = latin_locale_pack();
    // Latin pack binds Latin spellings; map path must match linear path results.
    assert_eq!(pack.keyword_token_kind("incipit"), Some(crate::lexer::TokenKind::Incipit));
    assert_eq!(pack.keyword_token_kind("not_a_keyword_xyz"), None);
    assert!(pack.type_binding_kind("numerus").is_some());
    assert_eq!(pack.type_binding_kind("not_a_type_xyz"), None);
}

// ── F-113: duplicate spellings must be caught across keywords and types ─────

#[test]
fn duplicate_spellings_across_keywords_and_types_are_rejected() {
    // A pack that uses a spelling in both a keyword and a type must fail
    // validation — spellings are a shared namespace (F-113).
    let err = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[keywords]
textus = "文本"

[types]
textus = "文本"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect_err("shared spelling across keyword and type sections must fail");

    let super::LocalePackLoadError::Validation(issues) = err else {
        panic!("expected validation error");
    };

    assert!(
        issues.contains(&LocalePackIssue::DuplicateSpelling { spelling: "文本".to_string() }),
        "expected DuplicateSpelling for '文本' in issues: {issues:?}"
    );
}

#[test]
fn duplicate_spellings_across_keyword_and_type_aliases_are_rejected() {
    // A pack where a keyword alias collides with a type canonical must fail.
    let err = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[keywords]
textus = { canonical = "real_textus", aliases = ["文本"] }

[types]
textus = "文本"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect_err("keyword alias colliding with type spelling must fail");

    let super::LocalePackLoadError::Validation(issues) = err else {
        panic!("expected validation error");
    };

    assert!(
        issues.contains(&LocalePackIssue::DuplicateSpelling { spelling: "文本".to_string() }),
        "expected DuplicateSpelling for '文本' in issues: {issues:?}"
    );
}

#[test]
fn llm_pack_resolves_morphology_keyword_latin_keys() {
    // The en pack must resolve the 7 new contextual keywords (token_kind: None)
    // to their Latin registry identity through the cache fix.
    let pack = installed_pack("en");

    // cede must resolve to "cede" (yield surface) not be dropped
    assert_eq!(pack.keyword_latin_key("yield"), Some("cede"));
    // tacebit = "await"
    assert_eq!(pack.keyword_latin_key("await"), Some("tacebit"));
    // fiet = "async"
    assert_eq!(pack.keyword_latin_key("async"), Some("fiet"));
    // fiunt = "generator"
    assert_eq!(pack.keyword_latin_key("generator"), Some("fiunt"));
    // fient = "async_generator"
    assert_eq!(pack.keyword_latin_key("async_generator"), Some("fient"));
    // figendum = "await_const"
    assert_eq!(pack.keyword_latin_key("await_const"), Some("figendum"));
    // variandum = "await_var"
    assert_eq!(pack.keyword_latin_key("await_var"), Some("variandum"));
    // reddet = "return_await"
    assert_eq!(pack.keyword_latin_key("return_await"), Some("reddet"));
}

#[test]
fn en_pack_empty_marker_rejects_vacua_alias() {
    // Goal en-pack-surface-fidelity V-5: en accepts only `empty` as the
    // empty-collection marker. The intrinsic row `vacua = "is_empty"` is a
    // separate concept and stays.
    let pack = installed_pack("en");
    assert!(
        validate_locale_pack(&pack).is_empty(),
        "en pack should validate: {:?}",
        validate_locale_pack(&pack)
    );
    assert_eq!(pack.keyword_latin_key("empty"), Some("vacua"));
    assert_eq!(
        pack.keyword_latin_key("vacua"),
        None,
        "en must not accept the Latin marker spelling after alias removal"
    );
    assert_eq!(pack.contextual_keyword_surface("vacua"), "empty");
    assert!(
        pack.contextual_keyword_aliases("vacua").is_empty(),
        "en marker row has no transition alias"
    );
    assert_eq!(pack.intrinsic_surface("vacua"), "is_empty");

    let la = installed_pack("la");
    assert_eq!(la.keyword_latin_key("vacua"), Some("vacua"));
    assert_eq!(la.keyword_latin_key("empty"), None, "la must not treat `empty` as the marker");
    assert!(
        la.contextual_keyword_aliases("vacua").is_empty(),
        "la identity row has no marker alias"
    );
}

#[test]
fn en_empty_named_binding_rhs_claims_as_marker() {
    // Parse pin: `var list<int> empty ← empty` — the binding is legal; the
    // RHS spelling claims as the marker. The name is unreferenceable in
    // expression position (null-like shadowing).
    let pack = installed_pack("en");
    let result = parse(lex_with_locale_pack("var list<int> empty ← empty", &pack));
    assert!(
        result.success(),
        "en empty-named binding with marker RHS must parse: {:?}",
        result.errors
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration, got {:?}", program.statements[0].kind);
    };
    let BindingPattern::Ident(name) = &var.binding else {
        panic!("expected identifier binding, got {:?}", var.binding);
    };
    assert_eq!(result.interner.resolve(name.name), "empty", "binding name `empty` is legal");
    let init = var.init.as_deref().expect("initializer");
    assert!(
        matches!(init.kind, ExprKind::Vacua(_)),
        "RHS `empty` must claim as the marker, got {:?}",
        init.kind
    );

    let reference = compile(
        &Session::new(
            Config::default()
                .with_target(Target::HirRust)
                .with_resolved_locale_pack(pack),
        ),
        "en-empty-reference.fab",
        "main { print empty }",
        ModuleCompileRequest::default(),
    );
    assert!(
        reference.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == Some("SEM014") && diagnostic.issue() == Some("vacua_requires_declared_type")
        }),
        "bare en `empty` reference must require a declared collection type: {:?}",
        reference.diagnostics
    );
}

#[test]
fn marker_named_bindings_warn_under_en_and_la_packs() {
    for (locale, source, issue) in [
        (
            "en",
            "main { const list<int> empty ← [1] print empty.length() }",
            "empty_marker_shadowing",
        ),
        (
            "la",
            "incipit { fixum lista<numerus> vacua ← [1] nota vacua }",
            "empty_marker_shadowing",
        ),
    ] {
        let pack = installed_pack(locale);
        let result = compile(
            &Session::new(
                Config::default()
                    .with_target(Target::HirRust)
                    .with_resolved_locale_pack(pack),
            ),
            &format!("{locale}-marker-shadow.fab"),
            source,
            ModuleCompileRequest::default(),
        );
        assert!(
            result.success(),
            "{locale} marker receiver should resolve, got {:?}",
            result.diagnostics
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == Some("WARN021") && diagnostic.issue() == Some(issue) }),
            "{locale} marker-named binding should emit WARN021, got {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn en_vacua_spelling_does_not_claim_marker() {
    let pack = installed_pack("en");
    let result = parse(lex_with_locale_pack("var list<int> xs ← vacua", &pack));
    assert!(
        result.success(),
        "en `vacua` must parse as an ordinary identifier: {:?}",
        result.errors
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(var) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let init = var.init.as_deref().expect("initializer");
    assert!(
        matches!(init.kind, ExprKind::Ident(_)),
        "en `vacua` must not claim the empty-collection marker, got {:?}",
        init.kind
    );
}

#[test]
fn latin_pack_resolves_morphology_keyword_surfaces() {
    let pack = latin_locale_pack();

    // Latin pack carries all 7 new contextual keywords as identity rows.
    for key in &["fiet", "fiunt", "fient", "figendum", "variandum", "reddet", "tacebit"] {
        assert_eq!(pack.keyword_latin_key(key), Some(*key), "Latin pack should resolve {key}");
    }

    // The pack should validate cleanly.
    assert!(validate_locale_pack(&pack).is_empty());
}

#[test]
fn installed_packs_resolve_batch_a_keyword_surfaces() {
    // Batch A (`modulus`, `iuncta`) are tokenless; their `[keywords]` rows must
    // still resolve to Latin registry identities and localized emit surfaces.
    let zh = installed_pack("zh-Hans");
    assert_eq!(zh.keyword_latin_key("模数"), Some("modulus"));
    assert_eq!(zh.contextual_keyword_surface("modulus"), "模数");
    // zh-Hans carries an iuncta row → the emit surface is localized.
    assert_eq!(zh.contextual_keyword_surface("iuncta"), "元组");

    let llm = installed_pack("en");
    assert_eq!(llm.keyword_latin_key("uint"), Some("modulus"));
    assert_eq!(llm.keyword_latin_key("tuple"), Some("iuncta"));
    assert_eq!(llm.contextual_keyword_surface("modulus"), "uint");
    assert_eq!(llm.contextual_keyword_surface("iuncta"), "tuple");
}

#[test]
fn installed_packs_resolve_batch_b_keyword_surfaces() {
    // Batch B (`nota`, `mone`, `vide`, `lege`, `lineam`, `scriptum`) are
    // tokenless; their `[keywords]` rows must still resolve to Latin registry
    // identities and localized emit surfaces. The en pack localizes all six.
    let llm = installed_pack("en");
    for (surface, latin_key) in [
        ("print", "nota"),
        ("warn", "mone"),
        ("debug", "vide"),
        ("read", "lege"),
        ("line", "lineam"),
        ("format", "scriptum"),
    ] {
        assert_eq!(
            llm.keyword_latin_key(surface),
            Some(latin_key),
            "{surface} should resolve to {latin_key}"
        );
        assert_eq!(
            llm.contextual_keyword_surface(latin_key),
            surface,
            "{latin_key} surface should be {surface}"
        );
    }
}

#[test]
fn installed_packs_resolve_batch_d_keyword_surfaces() {
    // Batch D (`aut`, `est`, `et`, `non`, `vel`) are tokenless; their
    // `[keywords]` rows must still resolve to Latin registry identities and
    // localized emit surfaces. Every installed pack localizes all five; the
    // llm pack uses LLM-optimized English spellings.
    let llm = installed_pack("en");
    for (surface, latin_key) in [
        ("or", "aut"),
        ("is", "est"),
        ("and", "et"),
        ("not", "non"),
        ("coalesce", "vel"),
    ] {
        assert_eq!(
            llm.keyword_latin_key(surface),
            Some(latin_key),
            "{surface} should resolve to {latin_key}"
        );
        assert_eq!(
            llm.contextual_keyword_surface(latin_key),
            surface,
            "{latin_key} surface should be {surface}"
        );
    }
    // zh-Hans localizes all five too (或 / 是 / 且 / 非 / 兜底).
    let zh = installed_pack("zh-Hans");
    assert_eq!(zh.contextual_keyword_surface("vel"), "兜底");
    assert_eq!(zh.keyword_latin_key("兜底"), Some("vel"));
}

#[test]
fn installed_packs_resolve_batch_e_keyword_surfaces() {
    // Batch E (`functio`, `genus`, `ordo`, `implendum`, `typus`,
    // `discretio`, `fixum`, `varia`, `sit`) are tokenless; their
    // `[keywords]` rows must still resolve to Latin registry identities and
    // localized emit surfaces. Every installed pack localizes all nine; the
    // llm pack uses LLM-optimized English spellings.
    let llm = installed_pack("en");
    for (surface, latin_key) in [
        ("fn", "functio"),
        ("class", "genus"),
        ("enum", "ordo"),
        ("interface", "implendum"),
        ("type", "typus"),
        ("union", "discretio"),
        ("const", "fixum"),
        ("var", "varia"),
        ("let", "sit"),
    ] {
        assert_eq!(
            llm.keyword_latin_key(surface),
            Some(latin_key),
            "{surface} should resolve to {latin_key}"
        );
        assert_eq!(
            llm.contextual_keyword_surface(latin_key),
            surface,
            "{latin_key} surface should be {surface}"
        );
    }
    // zh-Hans localizes all nine too (函数 / 类 / 枚举 / 契约 / 类型 / 判别 /
    // 常量 / 变量 / 设).
    let zh = installed_pack("zh-Hans");
    assert_eq!(zh.contextual_keyword_surface("functio"), "函数");
    assert_eq!(zh.keyword_latin_key("函数"), Some("functio"));
    assert_eq!(zh.contextual_keyword_surface("fixum"), "常量");
}

#[test]
fn installed_packs_resolve_batch_f_keyword_surfaces() {
    // Batch F (control flow: si/secus/sin/ergo/discerne/elige/dum/fac/casu/
    // ceterum/rumpe/perge/redde) are tokenless; their `[keywords]` rows must
    // still resolve to Latin registry identities and localized emit surfaces.
    // Every installed pack localizes all thirteen; the en pack uses
    // LLM-optimized English spellings.
    let llm = installed_pack("en");
    for (surface, latin_key) in [
        ("if", "si"),
        ("else", "secus"),
        ("elif", "sin"),
        ("then", "ergo"),
        ("match", "discerne"),
        ("switch", "elige"),
        ("while", "dum"),
        ("do", "fac"),
        ("case", "casu"),
        ("default", "ceterum"),
        ("break", "rumpe"),
        ("continue", "perge"),
        ("return", "redde"),
    ] {
        assert_eq!(
            llm.keyword_latin_key(surface),
            Some(latin_key),
            "{surface} should resolve to {latin_key}"
        );
        assert_eq!(
            llm.contextual_keyword_surface(latin_key),
            surface,
            "{latin_key} surface should be {surface}"
        );
    }
    // zh-Hans localizes them too (如果 / 否则 / 否则如果 / 则 / 匹配 / 选择 /
    // 当 / 执行 / 情况 / 默认 / 中断 / 继续 / 返回).
    let zh = installed_pack("zh-Hans");
    assert_eq!(zh.contextual_keyword_surface("si"), "如果");
    assert_eq!(zh.keyword_latin_key("如果"), Some("si"));
    assert_eq!(zh.contextual_keyword_surface("redde"), "返回");
}

#[test]
fn installed_packs_resolve_batch_g_keyword_surfaces() {
    // Batch G (the long tail: ego/finge/sub/implet/cura/ad/omnia/inter/intra/
    // per/usque/ante/praefixum/clausura/sponte/tacet) are tokenless; their
    // `[keywords]` rows must still resolve to Latin registry identities and
    // localized emit surfaces. The en pack localizes the statement/expression
    // words; spot-check zh-Hans for the rest.
    let llm = installed_pack("en");
    for (surface, latin_key) in [
        ("self", "ego"),
        ("with", "cura"),
        ("call", "ad"),
        ("all", "omnia"),
        ("within", "intra"),
        ("between", "inter"),
        ("step", "per"),
        ("until", "usque"),
        ("before", "ante"),
        ("pass", "tacet"),
    ] {
        assert_eq!(
            llm.keyword_latin_key(surface),
            Some(latin_key),
            "{surface} should resolve to {latin_key}"
        );
        assert_eq!(
            llm.contextual_keyword_surface(latin_key),
            surface,
            "{latin_key} surface should be {surface}"
        );
    }
    let zh = installed_pack("zh-Hans");
    assert_eq!(zh.contextual_keyword_surface("ego"), "自身");
    assert_eq!(zh.keyword_latin_key("自身"), Some("ego"));
    assert_eq!(zh.contextual_keyword_surface("clausura"), "闭包");
    assert_eq!(zh.contextual_keyword_surface("sponte"), "可选");
}

#[test]
fn installed_human_packs_resolve_morphology_keyword_surfaces() {
    let cases: &[(&str, &[(&str, &str)])] = &[
        (
            "ar",
            &[
                ("سلم", "cede"),
                ("غيرمتزامن", "fiet"),
                ("مولد", "fiunt"),
                ("مولد_غيرمتزامن", "fient"),
                ("انتظر_ثابت", "figendum"),
                ("انتظر_متغير", "variandum"),
                ("أعد_منتظرا", "reddet"),
                ("انتظر", "tacebit"),
            ],
        ),
        (
            "hi",
            &[
                ("आगेबढ़ो", "cede"),
                ("async", "fiet"),
                ("जनक", "fiunt"),
                ("async_जनक", "fient"),
                ("रुको_स्थिर", "figendum"),
                ("रुको_चर", "variandum"),
                ("रुको_लौटाओ", "reddet"),
                ("रुको", "tacebit"),
            ],
        ),
        (
            "th-TH",
            &[
                ("ให้", "cede"),
                ("อะซิงก์", "fiet"),
                ("สตรีม", "fiunt"),
                ("สตรีมอะซิงก์", "fient"),
                ("รอคง", "figendum"),
                ("รอแปร", "variandum"),
                ("รอคืน", "reddet"),
                ("รอทิ้ง", "tacebit"),
            ],
        ),
        (
            "vi",
            &[
                ("nhường", "cede"),
                ("async", "fiet"),
                ("sinh", "fiunt"),
                ("async_sinh", "fient"),
                ("đợi_hằng", "figendum"),
                ("đợi_biến", "variandum"),
                ("đợi_trả", "reddet"),
                ("đợi_bỏ", "tacebit"),
            ],
        ),
        (
            "zh-Hans",
            &[
                ("让出", "cede"),
                ("异步", "fiet"),
                ("流", "fiunt"),
                ("异流", "fient"),
                ("等定", "figendum"),
                ("等变", "variandum"),
                ("等返", "reddet"),
                ("等弃", "tacebit"),
            ],
        ),
        (
            "zh-Hant",
            &[
                ("讓出", "cede"),
                ("異步", "fiet"),
                ("流", "fiunt"),
                ("異流", "fient"),
                ("等定", "figendum"),
                ("等變", "variandum"),
                ("等返", "reddet"),
                ("等棄", "tacebit"),
            ],
        ),
    ];

    for (locale, spellings) in cases {
        let pack = installed_pack(locale);
        assert!(validate_locale_pack(&pack).is_empty(), "{locale} pack should validate cleanly");
        for (surface, latin) in *spellings {
            assert_eq!(
                pack.keyword_latin_key(surface),
                Some(*latin),
                "{locale} surface {surface} should resolve to {latin}"
            );
        }
    }
}

/// Census relational-view reader rows (census-types S2-U5): every installed
/// pack owns the two declaration-keyword rows and the three type-name rows.
/// Both keyword rows must be present together — the `relational_view`
/// keyword-group completeness validation (`validate_keyword_groups`) rejects
/// a pack that localizes only one of `schema`/`columna`, and the owned-rows
/// completeness gate requires the full scaffold key set in every pack.
#[test]
fn installed_packs_carry_census_reader_surface_rows() {
    for locale in ["la", "en", "th-TH", "ar", "zh-Hans", "zh-Hant", "vi", "hi"] {
        let pack = owned_pack(locale);
        for key in ["schema", "columna"] {
            let binding = pack
                .keywords
                .get(key)
                .unwrap_or_else(|| panic!("{locale} pack is missing [keywords] {key}"));
            assert_eq!(binding.key, key, "{locale} {key} row must map to its canonical key");
        }
        for key in ["filtrum", "series", "census"] {
            let binding = pack
                .types
                .get(key)
                .unwrap_or_else(|| panic!("{locale} pack is missing [types] {key}"));
            assert_eq!(binding.key, key, "{locale} {key} row must map to its canonical key");
        }
    }
}

/// Census diagnostics rows (census-types S2-U5/S2-U5-rehome): each pinned
/// schema issue key has a `message`/`help` row in every pack under the
/// **family its raised code uses** — `schema_duplicate_visible`/`source` under
/// SEM005 (DuplicateDefinition), `schema_unknown_type` under SEM002
/// (UndefinedType), `schema_method`/`schema_inheritance` under PARSE010
/// (parse-time InvalidDeclaration), and `schema_nested_column` under SEM008
/// (LoweringError). The issue keys are the stable identity both the semantic
/// unit (catalog) and the locale packs cite; the family must match the raising
/// code so the family-scoped render lookup (`code` → family → issue key)
/// reaches the localized row.
#[test]
fn installed_packs_carry_census_diagnostic_issue_rows() {
    // key -> the family its raising code uses (mirrors the code-level tests
    // in schema_resolve_test.rs: SEM005/SEM002/PARSE010/SEM008).
    let family_by_key: &[(&str, &str)] = &[
        ("schema_duplicate_visible", "SEM005"),
        ("schema_duplicate_source", "SEM005"),
        ("schema_unknown_type", "SEM002"),
        ("schema_method", "PARSE010"),
        ("schema_inheritance", "PARSE010"),
        ("schema_nested_column", "SEM008"),
    ];
    for locale in ["la", "en", "th-TH", "ar", "zh-Hans", "zh-Hant", "vi", "hi"] {
        let pack = owned_pack(locale);
        for (key, family) in family_by_key {
            let template = pack
                .diagnostics
                .get(*family)
                .and_then(|fam| fam.issues.get(*key))
                .unwrap_or_else(|| panic!("{locale} pack is missing {family}.{key}"));
            assert!(!template.message.is_empty(), "{locale} {family}.{key} needs a message");
        }
        // SEM008 carries only schema_nested_column among the six keys: the
        // other five raise under SEM005/SEM002/PARSE010, so their rows must
        // not sit under SEM008 (family-scoped lookup would never reach them).
        let sem008 = pack.diagnostics.get("SEM008").expect("SEM008 family row");
        for (key, family) in family_by_key {
            if *family == "SEM008" {
                continue;
            }
            assert!(
                !sem008.issues.contains_key(*key),
                "{locale} SEM008 must not carry {key}; it raises under {family}"
            );
        }
    }
}

/// Focused render proof (census S2-U5-rehome): each pinned schema issue key,
/// raised through the real compiler, resolves its **localized pack row** —
/// not the catalog fallback — because the row now lives under the family the
/// raised code carries. Before the re-home, five of the six rows sat under
/// SEM008 while their diagnostics carried SEM005/SEM002/PARSE010, so the
/// family-scoped lookup missed them and only the fallback text rendered.
#[test]
fn census_diagnostic_rows_render_from_their_raising_families() {
    // Raised key → (raising family, a source that raises it). Mirrors the
    // code-level assertions in schema_resolve_test.rs.
    let cases: &[(&str, &str, &str)] = &[
        (
            "schema_duplicate_visible",
            "SEM005",
            "schema S {\n            columna numerus id\n            columna textus nomen\n            columna bivalens nomen\n        }",
        ),
        (
            "schema_duplicate_source",
            "SEM005",
            "schema S {\n            columna textus nomen : plenum\n            columna textus plenum\n        }",
        ),
        (
            "schema_unknown_type",
            "SEM002",
            "schema S {\n            columna IgnotaRes nomen\n        }",
        ),
        (
            "schema_method",
            "PARSE010",
            "schema S {\n            columna numerus id\n            functio f() → vacuum {}\n        }",
        ),
        ("schema_inheritance", "PARSE010", "schema S sub T { }"),
        (
            "schema_nested_column",
            "SEM008",
            "schema A {\n            columna numerus id\n        }\n\n        schema B {\n            columna A interior\n        }",
        ),
    ];

    for locale in ["la", "en", "th-TH", "ar", "zh-Hans", "zh-Hant", "vi", "hi"] {
        let pack = owned_pack(locale);
        for (key, family, source) in cases {
            let compiler = crate::Compiler::new(crate::Config::default());
            let result = compiler.compile_str("render.fab", source);
            let diag = result
                .diagnostics
                .iter()
                .find(|d| d.code == Some(*family) && d.issue() == Some(*key))
                .unwrap_or_else(|| panic!("{locale} {family}.{key} should raise through the compiler"));

            let rendered = pack
                .render_diagnostic_text(diag)
                .unwrap_or_else(|err| panic!("{locale} {family}.{key} template should render: {err:?}"))
                .unwrap_or_else(|| {
                    panic!("{locale} {family}.{key} raised diagnostic must resolve a pack row (family-scoped lookup)")
                });

            let row = pack
                .diagnostics
                .get(*family)
                .and_then(|fam| fam.issues.get(*key))
                .expect("row exists under the raising family");
            assert_eq!(
                rendered.message, row.message,
                "{locale} {family}.{key} must render the localized pack row, not the fallback"
            );
        }
    }
}

const DEMO_LA_FRAGMENT: &str = r#"
[pack]
id = "la"
schema_version = 1

[[library_members]]
provider = "demo"
module_path = ["canvas"]
kind = "type"
canonical = "Point"
surface = "Punctum"

[[library_members]]
provider = "demo"
module_path = ["canvas"]
kind = "function"
canonical = "make_point"
surface = "fac_punctum"
"#;

fn demo_canvas_identity() -> InterfaceLibraryIdentity {
    InterfaceLibraryIdentity { provider: "demo".to_owned(), package: None, module_path: vec!["canvas".to_owned()] }
}

#[test]
fn library_member_fragment_loads_without_reader_pack_tables() {
    let (_dir, path) = temp_pack_path("library-fragment");
    std::fs::write(&path, DEMO_LA_FRAGMENT).expect("write fragment");

    let pack = LocalePack::from_library_members_path(&path).expect("member fragment should load");
    let identity = demo_canvas_identity();
    assert_eq!(pack.metadata.id, "la");
    assert!(pack.keywords.is_empty());
    assert_eq!(
        pack.library_member_surface(&identity, None, InterfaceMemberKind::Type, "Point"),
        Some("Punctum")
    );
    assert_eq!(
        pack.library_member_surface(&identity, None, InterfaceMemberKind::Function, "make_point"),
        Some("fac_punctum")
    );
}

#[test]
fn reader_pack_loader_rejects_member_only_fragment() {
    let err = LocalePack::from_toml_str(DEMO_LA_FRAGMENT).expect_err("full reader loader needs llm + groups");
    match err {
        super::LocalePackLoadError::Validation(issues) => {
            assert!(
                issues.iter().any(|issue| matches!(
                    issue,
                    LocalePackIssue::EmptyLlmPromptSnippet | LocalePackIssue::EmptyLlmExemplars
                )),
                "expected empty-llm issues, got {issues:?}"
            );
        }
        other => panic!("expected validation, got {other}"),
    }
}

#[test]
fn merge_library_members_replaces_same_key_and_keeps_other_libraries() {
    let (_consumer_dir, consumer_path) = temp_pack_path("consumer-overlay");
    std::fs::write(
        &consumer_path,
        r#"
[pack]
id = "la"
schema_version = 1

[[library_members]]
provider = "other"
module_path = ["math"]
kind = "function"
canonical = "summa"
surface = "sum"

[[library_members]]
provider = "demo"
module_path = ["canvas"]
kind = "type"
canonical = "Point"
surface = "Overlook"
"#,
    )
    .expect("write consumer fragment");
    let consumer = LocalePack::from_library_members_path(&consumer_path).expect("consumer fragment");

    let (_dir, path) = temp_pack_path("overlay");
    std::fs::write(&path, DEMO_LA_FRAGMENT).expect("write fragment");
    let fragment = LocalePack::from_library_members_path(&path).expect("fragment");

    let mut merged = latin_locale_pack();
    merged.merge_library_members_from(&consumer);
    merged.merge_library_members_from(&fragment);

    let canvas = demo_canvas_identity();
    let other =
        InterfaceLibraryIdentity { provider: "other".to_owned(), package: None, module_path: vec!["math".to_owned()] };
    assert_eq!(
        merged.library_member_surface(&canvas, None, InterfaceMemberKind::Type, "Point"),
        Some("Punctum")
    );
    assert_eq!(
        merged.library_member_surface(&other, None, InterfaceMemberKind::Function, "summa"),
        Some("sum")
    );
    assert_eq!(merged.keywords["functio"].canonical, "functio");
}

#[test]
fn load_library_locale_pack_skips_missing_and_loads_present() {
    let dir = crate::test_support::test_temp_dir("lib-locale-home");
    let root = dir.path();
    assert!(
        load_library_locale_pack(root, "la")
            .expect("missing is ok")
            .is_none()
    );

    let pack_dir = root.join("locale/la");
    std::fs::create_dir_all(&pack_dir).expect("locale dir");
    std::fs::write(pack_dir.join("pack.toml"), DEMO_LA_FRAGMENT).expect("write pack");

    let loaded = load_library_locale_pack(root, "la")
        .expect("present pack")
        .expect("Some");
    let identity = demo_canvas_identity();
    assert_eq!(
        loaded.library_member_surface(&identity, None, InterfaceMemberKind::Type, "Point"),
        Some("Punctum")
    );

    let mut pack = latin_locale_pack();
    merge_library_locale_packs(&mut pack, [root]).expect("merge");
    assert_eq!(
        pack.library_member_surface(&identity, None, InterfaceMemberKind::Type, "Point"),
        Some("Punctum")
    );
}

#[test]
fn format_locale_pack_load_error_lists_validation_issues() {
    let err = LocalePack::from_toml_str(DEMO_LA_FRAGMENT).expect_err("full loader fails");
    let rendered = format_locale_pack_load_error(&err);
    assert!(rendered.contains("EmptyLlm"), "got {rendered}");
}
