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
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

fn temp_pack_path(label: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = crate::test_support::test_temp_dir(&format!("reader-pack-{label}"));
    let path = dir.path().join("pack.toml");
    (dir, path)
}

fn installed_pack(locale: &str) -> LocalePack {
    let path = crate::locale::workspace_locale_pack_path(locale);
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
    let path = crate::locale::workspace_locale_pack_path(locale);
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

/// Registry spellings that a sealed reader surface intentionally omits.
///
/// The lexer registry keeps `conversion` as a keyword identity alias for
/// tooling, and the D23.2 null split moves `nihil` out of the keyword block
/// into the type block; the sealed English and Latin reader surfaces declare
/// neither as a keyword. Completeness compares each installed pack with the
/// scaffold after applying these settled per-locale exceptions; no pack row is
/// synthesized here.
pub(super) const SEALED_KEYWORD_ROW_OMISSIONS: &[(&str, &str)] =
    &[("en", "conversion"), ("la", "conversion"), ("la", "nihil")];

pub(super) fn is_sealed_keyword_row_omission(pack_id: &str, section: &str, key: &str) -> bool {
    section == "keyword"
        && SEALED_KEYWORD_ROW_OMISSIONS
            .iter()
            .any(|&(sealed_locale, omitted_key)| sealed_locale == pack_id && omitted_key == key)
}

fn assert_pack_keys_match(pack_id: &str, section: &str, mut expected: BTreeSet<String>, actual: BTreeSet<String>) {
    for &(_, omitted_key) in SEALED_KEYWORD_ROW_OMISSIONS {
        if is_sealed_keyword_row_omission(pack_id, section, omitted_key) {
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

/// Installed keyword and type rows must carry the scaffold's **canonical**, not
/// merely the same key set: the canonical is both the accepted source spelling
/// (`build_type_binding_by_spelling`) and the emitted surface
/// (`build_type_surfaces` → `type_surface`), so a key-only comparison lets the
/// generated and installed `la` definitions ship different spellings for one
/// entry (DEF S03-2 — `nihil`/`modulus` vs `nihil_t`/`modulus_t`).
///
/// Keyword rows are compared the same way: the scaffold's `own`/`copy` rows
/// carry the installed pack's `penes`/`exemplum` (ruling 2026-09-30), so a
/// keyword that drifts from the installed spelling fails here.
fn assert_pack_canonicals_match(pack_id: &str, scaffold: &LocalePack, installed: &LocalePack) {
    let keyword_drift = scaffold.keywords.iter().filter_map(|(key, expected)| {
        let actual = installed.keywords.get(key)?.canonical.as_str();
        let scaffold_canonical = expected.canonical.as_str();
        (actual != scaffold_canonical)
            .then(|| format!("keyword {key}: scaffold {scaffold_canonical:?}, installed {actual:?}"))
    });
    let type_drift = scaffold.types.iter().filter_map(|(key, expected)| {
        let actual = installed.types.get(key)?.canonical.as_str();
        let scaffold_canonical = expected.canonical.as_str();
        (actual != scaffold_canonical)
            .then(|| format!("type {key}: scaffold {scaffold_canonical:?}, installed {actual:?}"))
    });
    let drifted: Vec<String> = keyword_drift.chain(type_drift).collect();
    assert!(
        drifted.is_empty(),
        "installed {pack_id} canonicals must match the generated reader-pack scaffold; drifted={drifted:?}"
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
    assert_pack_canonicals_match("la", &scaffold, &installed);
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

/// `SEM010.nominal_ascription_not_provable` is emitted with `{found}`,
/// `{expected}` and `{implet}`. A pack without its own row silently falls back
/// to the base `SEM010` row (or to `la` through `inherits`), so the issue row
/// must exist in every installed pack. Presence only is asserted here; the row
/// text belongs to translation review.
#[test]
fn locale_pack_completeness_nominal_ascription_row_in_every_pack() {
    for locale in ["la", "en", "th-TH", "ar", "zh-Hans", "zh-Hant", "vi", "hi"] {
        assert_issue_template(&owned_pack(locale), "SEM010", "nominal_ascription_not_provable");
    }
}

/// The thread-body exits are refused by their own named issues; every pack
/// carries a row for each, or the issue falls back to the generic `SEM010` text.
#[test]
fn locale_pack_completeness_thread_body_exit_rows_in_every_pack() {
    for locale in ["la", "en", "th-TH", "ar", "zh-Hans", "zh-Hant", "vi", "hi"] {
        let pack = owned_pack(locale);
        for issue in [
            "break_in_thread_body",
            "return_in_thread_body",
            "return_thread_outside_lane",
            "return_thread_with_value",
            "return_thread_in_nested_body",
        ] {
            assert_issue_template(&pack, "SEM010", issue);
        }
        assert_issue_template(&pack, "PKG001", "return_thread_lowering_pending");
    }
}

#[test]
fn locale_pack_completeness_sinking_contract_row_in_la() {
    assert_issue_template(&owned_pack("la"), "SEM018", "radix_contract_sinking_shape");
}

#[test]
fn locale_pack_completeness_import_target_bound_row_in_la() {
    assert_issue_template(&owned_pack("la"), "SEM017", "import_target_bound");
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
    assert_eq!(
        fallback_render.message,
        "CODE:numerus:textus [locale pack missing row: SEM010.other_type_mismatch]"
    );
    assert_eq!(fallback_render.help.as_deref(), Some("CODEHELP"));
}

#[test]
fn type_arguments_end_message_renders_in_english_and_hindi() {
    let diagnostic = diagnostic_with_code("PARSE001", vec![diagnostic_arg("issue", "type_arguments_end")]);
    for (locale, expected) in [
        ("en", "expected '>' after type arguments"),
        ("hi", "प्रकार तर्कों के बाद '>' अपेक्षित है"),
    ] {
        let rendered = installed_pack(locale)
            .render_diagnostic_text(&diagnostic)
            .expect("template should render")
            .expect("template row should exist");
        assert_eq!(rendered.message, expected, "{locale}.type_arguments_end");
    }
}

#[test]
fn type_issue_templates_render_their_exact_rows_in_all_installed_packs() {
    const ISSUES: [&str; 9] = [
        "type_arguments_end",
        "function_type_start",
        "function_type_parameters_end",
        "function_type_return_arrow",
        "numeric_wrapper_start",
        "tuple_type_label_separator",
        "ratio_type_label_separator",
        "shape_dimensions_end",
        "shape_parenthesis_end",
    ];

    let mut checked = 0;
    for locale in ["en", "la", "ar", "hi", "vi", "th-TH", "zh-Hans", "zh-Hant"] {
        let pack = installed_pack(locale);
        for issue in ISSUES {
            let row = pack.diagnostics["PARSE001"]
                .issues
                .get(issue)
                .expect("explicit type issue row, not base fallback");
            let diagnostic = diagnostic_with_code("PARSE001", vec![diagnostic_arg("issue", issue)]);
            let rendered = pack
                .render_diagnostic_text(&diagnostic)
                .expect("type issue template should render")
                .expect("type issue row should exist");
            assert_eq!(rendered.message, row.message, "{locale}.{issue}");
            checked += 1;
        }
    }
    assert_eq!(checked, 72);
}

#[test]
fn declaration_issue_templates_render_their_exact_rows_in_all_installed_packs() {
    const ISSUES: [&str; 35] = [
        "awaited_binding_assignment",
        "array_binding_pattern_start",
        "array_binding_pattern_end",
        "object_binding_pattern_start",
        "object_binding_pattern_end",
        "function_declaration_parameters_start",
        "function_declaration_parameters_end",
        "class_body_end",
        "interface_body_start",
        "interface_method_parameters_start",
        "interface_method_parameters_end",
        "interface_body_end",
        "type_alias_value_separator",
        "schema_body_start",
        "schema_body_end",
        "enum_body_start",
        "enum_body_end",
        "union_body_start",
        "union_variant_fields_end",
        "union_body_end",
        "import_record_start",
        "import_field_value_separator",
        "import_record_end",
        "probandum_body_start",
        "probandum_body_end",
        "annotation_fields_start",
        "annotation_field_value_separator",
        "annotation_fields_end",
        "nucleum_annotation_fields_start",
        "nucleum_annotation_field_value_separator",
        "nucleum_annotation_fields_end",
        "block_start",
        "block_end",
        "generic_parameters_end",
        "contract_type_arguments_end",
    ];

    let mut checked = 0;
    for locale in ["en", "la", "ar", "hi", "vi", "th-TH", "zh-Hans", "zh-Hant"] {
        let pack = installed_pack(locale);
        for issue in ISSUES {
            let row = pack.diagnostics["PARSE001"]
                .issues
                .get(issue)
                .expect("explicit declaration issue row");
            let diagnostic = diagnostic_with_code("PARSE001", vec![diagnostic_arg("issue", issue)]);
            let rendered = pack
                .render_diagnostic_text(&diagnostic)
                .expect("declaration issue template should render")
                .expect("declaration issue row should exist");
            assert_eq!(rendered.message, row.message, "{locale}.{issue}");
            checked += 1;
        }
    }
    assert_eq!(checked, 280);
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
fn scribe_callee_help_names_both_ways_out_in_every_installed_pack() {
    // `print (a + b) * 2` cannot be fixed by dropping the parentheses, so the
    // help must point at the imported console function and at the `let` binding.
    for locale in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let pack = installed_pack(locale);
        let diagnostic = semantic_diagnostic(
            SemanticErrorKind::VariableUndefined,
            vec![
                diagnostic_arg("issue", "scribe_callee"),
                diagnostic_arg("spelling", "print"),
            ],
        );
        let rendered = pack
            .render_diagnostic_text(&diagnostic)
            .unwrap_or_else(|err| panic!("{locale} scribe_callee should render: {err:?}"))
            .unwrap_or_else(|| panic!("{locale} scribe_callee row should exist"));
        let help = rendered.help.unwrap_or_default();
        assert!(
            help.contains("norma:console"),
            "{locale} help should name the console import: {help}"
        );
        assert!(help.contains("print(...)"), "{locale} help should show the call form: {help}");
        assert!(
            !help.contains("{"),
            "{locale} help should leave no unfilled placeholder: {help}"
        );
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

// ── D23.2: spelling uniqueness is per block, not across blocks ──────────────

#[test]
fn same_spelling_across_keywords_and_types_is_allowed() {
    // D23.2: one word may name both a keyword row and a type row. Latin
    // `modulus` is the live case (the modular-word keyword plus the
    // modulus type); before D23.2 the shared-namespace rule (F-113) forced
    // the invented `nihil_t` / `null_ty` type spellings.
    let pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[keywords]
modulus = "modulus"

[types]
modulus = "modulus"

[llm]
system_prompt_snippet = "emit Latin reader-locale Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect("shared spelling across keyword and type sections must load");

    assert_eq!(pack.keywords["modulus"].canonical, "modulus");
    assert_eq!(pack.types["modulus"].canonical, "modulus");
}

#[test]
fn duplicate_spellings_within_keywords_are_rejected() {
    // A duplicate *within* `[keywords]` is still a pack bug.
    let err = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[keywords]
functio = "dup"
nota = "dup"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect_err("duplicate spelling within [keywords] must fail");

    let super::LocalePackLoadError::Validation(issues) = err else {
        panic!("expected validation error");
    };

    assert!(
        issues.contains(&LocalePackIssue::DuplicateSpelling { spelling: "dup".to_string() }),
        "expected DuplicateSpelling for 'dup' in issues: {issues:?}"
    );
}

#[test]
fn duplicate_spellings_within_types_are_rejected() {
    // A duplicate *within* `[types]` is still a pack bug.
    let err = LocalePack::from_toml_str(
        r#"
[pack]
id = "la-test"
fallback = ["la"]

[types]
textus = "dup"
numerus = "dup"

[llm]
system_prompt_snippet = "emit Chinese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.fab"]
"#,
    )
    .expect_err("duplicate spelling within [types] must fail");

    let super::LocalePackLoadError::Validation(issues) = err else {
        panic!("expected validation error");
    };

    assert!(
        issues.contains(&LocalePackIssue::DuplicateSpelling { spelling: "dup".to_string() }),
        "expected DuplicateSpelling for 'dup' in issues: {issues:?}"
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
fn en_pack_spells_sparse_and_range_types() {
    let pack = installed_pack("en");
    assert_eq!(
        pack.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Sparsa)),
        "sparse"
    );
    assert_eq!(
        pack.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Intervallum)),
        "range"
    );
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
    // Parse pin: `mut list<int> empty ← empty` — the binding is legal; the
    // RHS spelling claims as the marker. The name is unreferenceable in
    // expression position (null-like shadowing).
    let pack = installed_pack("en");
    let result = parse(lex_with_locale_pack("mut list<int> empty ← empty", &pack));
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
                .with_latin_locale()
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

#[cfg(feature = "hir-rust")]
#[test]
fn marker_named_bindings_warn_under_en_and_la_packs() {
    for (locale, source, issue) in [
        (
            "en",
            "main { let list<int> empty ← [1] print empty.length() }",
            "empty_marker_shadowing",
        ),
        (
            "la",
            "incipit { sit lista<numerus> vacua ← [1] nota vacua }",
            "empty_marker_shadowing",
        ),
    ] {
        let pack = installed_pack(locale);
        let result = compile(
            &Session::new(
                Config::default()
                    .with_latin_locale()
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
    let result = parse(lex_with_locale_pack("mut list<int> xs ← vacua", &pack));
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
    assert_eq!(llm.keyword_latin_key("wrapping"), Some("modulus"));
    assert_eq!(llm.keyword_latin_key("tuple"), Some("iuncta"));
    assert_eq!(llm.contextual_keyword_surface("modulus"), "wrapping");
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
        ("mut", "varia"),
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
    // Batch G (the long tail: ego/finge/sub/implet/ad/omnia/inter/intra/
    // per/usque/ante/praefixum/clausura/sponte/tacet) are tokenless; their
    // `[keywords]` rows must still resolve to Latin registry identities and
    // localized emit surfaces. The en pack localizes the statement/expression
    // words; spot-check zh-Hans for the rest.
    let llm = installed_pack("en");
    for (surface, latin_key) in [
        ("self", "ego"),
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
            let compiler = crate::Compiler::new(crate::Config::default().with_latin_locale());
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
fn language_packs_carry_no_library_members() {
    for locale in ["ar", "en", "hi", "la", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let pack = installed_pack(locale);
        assert!(
            pack.library_members.is_empty(),
            "{locale} language pack must not embed library members"
        );
    }
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

/// `fac { … } dum cond cape err { … }` reports PARSE020 `fac_cape_after_dum`;
/// every installed pack owns the row and renders it with its own `cape`/`dum`
/// spellings so the fix (handler before condition) reads in-locale.
#[test]
fn every_installed_pack_renders_fac_cape_after_dum() {
    for locale in ["la", "en", "th-TH", "zh-Hans", "zh-Hant", "ar", "hi", "vi"] {
        assert_issue_template(&owned_pack(locale), "PARSE020", "fac_cape_after_dum");
        let pack = installed_pack(locale);
        let diagnostic = parse_diagnostic(
            ParseErrorKind::StatementInvalid,
            vec![diagnostic_arg("issue", "fac_cape_after_dum")],
        );
        let rendered = pack
            .render_diagnostic_text(&diagnostic)
            .unwrap_or_else(|err| panic!("{locale} template should render: {err:?}"))
            .unwrap_or_else(|| panic!("{locale} PARSE020 row should exist"));
        assert!(!rendered.message.contains("missing row"), "{locale}: {}", rendered.message);
        for keyword in ["cape", "dum"] {
            let spelling = &pack.keywords.get(keyword).expect("keyword row").canonical;
            assert!(
                rendered.message.contains(spelling.as_str()),
                "{locale} message must name `{spelling}`: {}",
                rendered.message
            );
        }
    }
}

/// The parser's item-list separator issues (`item_list_missing_separator`,
/// `item_list_trailing_comma`) are live at every list site; a pack without a
/// row renders the base PARSE001 text plus the missing-row marker. Every
/// installed pack owns both rows and renders without that marker.
#[test]
fn item_list_separator_issue_rows_exist_in_every_installed_pack() {
    for locale in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let owned = owned_pack(locale);
        let issues = &owned.diagnostics["PARSE001"].issues;
        for issue in ["item_list_missing_separator", "item_list_trailing_comma"] {
            assert!(issues.contains_key(issue), "{locale} must own PARSE001.{issue}");
        }
        let diagnostic = parse_diagnostic(
            ParseErrorKind::Expected,
            vec![
                diagnostic_arg("issue", "item_list_missing_separator"),
                diagnostic_arg("family", "item_list"),
                diagnostic_arg("position", "call_arguments"),
            ],
        );
        let rendered = installed_pack(locale)
            .render_diagnostic_text(&diagnostic)
            .unwrap_or_else(|err| panic!("{locale} template should render: {err:?}"))
            .unwrap_or_else(|| panic!("{locale} PARSE001 row should exist"));
        assert!(
            !rendered.message.contains("locale pack missing row"),
            "{locale}: {}",
            rendered.message
        );
    }
}

/// The 14 frontend review issue rows are owned and rendered by
/// every installed pack. The shrink-only absent-row budget gate counts missing
/// rows, so it would accept a pack that deletes one of these and adds an
/// unrelated row elsewhere; name each row here so a deletion fails by name.
#[test]
fn installed_packs_render_frontend_review_issue_rows() {
    const ROWS: &[(&str, &str)] = &[
        ("LEX001", "empty_regex_literal"),
        ("LEX001", "unterminated_regex_literal"),
        ("SEM016", "conversio_failure_channel_value_required"),
        ("PARSE001", "expected_identifier"),
        ("PARSE001", "expected_string_literal"),
        ("PARSE001", "expected_mutability_keyword"),
        ("PARSE001", "await_binding_requires_name"),
        ("PARSE001", "expected_method_declaration"),
        ("PARSE001", "expected_praepara_hook"),
        ("PARSE001", "expected_lane_value"),
        ("PARSE001", "expected_scribe_keyword"),
        ("PARSE001", "expected_incipit_keyword"),
        ("PARSE001", "enum_member_value_number"),
        ("PARSE001", "enum_member_value"),
    ];
    for locale in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let owned = owned_pack(locale);
        for &(code, issue) in ROWS {
            let row = owned
                .diagnostics
                .get(code)
                .and_then(|template| template.issues.get(issue))
                .unwrap_or_else(|| panic!("{locale} must own {code}.{issue}"));
            assert!(
                !row.message.trim().is_empty(),
                "{locale} {code}.{issue} message must not be empty"
            );
            assert!(
                row.help
                    .as_deref()
                    .is_some_and(|help| !help.trim().is_empty()),
                "{locale} {code}.{issue} help must not be empty"
            );
            let diagnostic = diagnostic_with_code(code, vec![diagnostic_arg("issue", issue)]);
            let rendered = installed_pack(locale)
                .render_diagnostic_text(&diagnostic)
                .unwrap_or_else(|err| panic!("{locale} {code}.{issue} template should render: {err:?}"))
                .unwrap_or_else(|| panic!("{locale} {code} row should exist"));
            assert!(
                !rendered.message.contains("locale pack missing row"),
                "{locale} {code}.{issue}: {}",
                rendered.message
            );
        }
    }
}

/// The LEX002 float-overflow/underflow rows and the SEM008 conversion-signature
/// row (corpus coverage I3, batch 1) are owned and rendered by every installed
/// pack. The absent-row budget gate counts missing rows, so it would accept a
/// pack that deletes one of these and adds an unrelated row elsewhere; name each
/// row here so a deletion fails by name.
#[test]
fn installed_packs_render_lex002_float_and_sem008_conversio_signature_rows() {
    const ROWS: &[(&str, &str)] = &[
        ("LEX002", "float_overflow"),
        ("LEX002", "float_underflow"),
        ("SEM008", "conversio_signature"),
    ];
    for locale in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let owned = owned_pack(locale);
        for &(code, issue) in ROWS {
            let row = owned
                .diagnostics
                .get(code)
                .and_then(|template| template.issues.get(issue))
                .unwrap_or_else(|| panic!("{locale} must own {code}.{issue}"));
            assert!(
                !row.message.trim().is_empty(),
                "{locale} {code}.{issue} message must not be empty"
            );
            assert!(
                row.help
                    .as_deref()
                    .is_some_and(|help| !help.trim().is_empty()),
                "{locale} {code}.{issue} help must not be empty"
            );
            let diagnostic = diagnostic_with_code(code, vec![diagnostic_arg("issue", issue)]);
            let rendered = installed_pack(locale)
                .render_diagnostic_text(&diagnostic)
                .unwrap_or_else(|err| panic!("{locale} {code}.{issue} template should render: {err:?}"))
                .unwrap_or_else(|| panic!("{locale} {code} row should exist"));
            assert!(
                !rendered.message.contains("locale pack missing row"),
                "{locale} {code}.{issue}: {}",
                rendered.message
            );
            assert!(
                !rendered
                    .message
                    .contains("locale pack template missing placeholder")
                    && !rendered
                        .help
                        .as_deref()
                        .is_some_and(|help| help.contains("locale pack template missing placeholder")),
                "{locale} {code}.{issue} must not miss a template placeholder: {} / {:?}",
                rendered.message,
                rendered.help
            );
        }
    }
}

/// The four PARSE001 rows for the copy-in operator, duplicate tuple element
/// labels, named `implements` slots and imported field names (corpus coverage
/// I3, batch 2) are owned and rendered by every installed pack, with the
/// placeholders each template names. The absent-row budget gate counts missing
/// rows, so it would accept a pack that deletes one of these and adds an
/// unrelated row elsewhere; name each row here so a deletion fails by name.
#[test]
fn installed_packs_render_parse001_copy_duplicate_implements_import_rows() {
    const ROWS: &[(&str, &[(&str, &str)])] = &[
        ("copy_into_value_expected", &[("operator", "⇇")]),
        ("duplicate_tuple_element_label", &[("label", "a")]),
        ("implements_slot_requires_name", &[]),
        ("import_field_name", &[("expected", "identifier")]),
    ];
    for locale in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let owned = owned_pack(locale);
        for &(issue, args) in ROWS {
            let row = owned
                .diagnostics
                .get("PARSE001")
                .and_then(|template| template.issues.get(issue))
                .unwrap_or_else(|| panic!("{locale} must own PARSE001.{issue}"));
            assert!(
                !row.message.trim().is_empty(),
                "{locale} PARSE001.{issue} message must not be empty"
            );
            assert!(
                row.help
                    .as_deref()
                    .is_some_and(|help| !help.trim().is_empty()),
                "{locale} PARSE001.{issue} help must not be empty"
            );
            let mut diagnostic_args = vec![diagnostic_arg("issue", issue)];
            diagnostic_args.extend(
                args.iter()
                    .map(|(name, value)| diagnostic_arg(name, *value)),
            );
            let diagnostic = parse_diagnostic(ParseErrorKind::Expected, diagnostic_args);
            let rendered = installed_pack(locale)
                .render_diagnostic_text(&diagnostic)
                .unwrap_or_else(|err| panic!("{locale} PARSE001.{issue} template should render: {err:?}"))
                .unwrap_or_else(|| panic!("{locale} PARSE001 row should exist"));
            assert!(
                !rendered.message.contains("locale pack missing row"),
                "{locale} PARSE001.{issue}: {}",
                rendered.message
            );
            assert!(
                !rendered
                    .message
                    .contains("locale pack template missing placeholder")
                    && !rendered
                        .help
                        .as_deref()
                        .is_some_and(|help| help.contains("locale pack template missing placeholder")),
                "{locale} PARSE001.{issue} must not miss a template placeholder: {} / {:?}",
                rendered.message,
                rendered.help
            );
        }
    }
}

/// The four PARSE001 rows for annotation field names, the label/bind-operator
/// distinction, labeled actuals at non-template calls and labeled spreads
/// (corpus coverage I3, batch 3) are owned and rendered by every installed
/// pack, with the placeholders each template names. The absent-row budget gate
/// counts missing rows, so it would accept a pack that deletes one of these and
/// adds an unrelated row elsewhere; name each row here so a deletion fails by
/// name.
#[test]
fn installed_packs_render_parse001_annotation_label_spread_rows() {
    const ROWS: &[(&str, &[(&str, &str)])] = &[
        ("annotation_field_name", &[("expected", "identifier")]),
        (
            "label_bind_distinction",
            &[("label", "key"), ("label_operator", ":"), ("binding_operator", "=")],
        ),
        ("labeled_actual_non_template_call", &[("label", "key")]),
        ("labeled_actual_spread_unsupported", &[]),
    ];
    for locale in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let owned = owned_pack(locale);
        for &(issue, args) in ROWS {
            let row = owned
                .diagnostics
                .get("PARSE001")
                .and_then(|template| template.issues.get(issue))
                .unwrap_or_else(|| panic!("{locale} must own PARSE001.{issue}"));
            assert!(
                !row.message.trim().is_empty(),
                "{locale} PARSE001.{issue} message must not be empty"
            );
            assert!(
                row.help
                    .as_deref()
                    .is_some_and(|help| !help.trim().is_empty()),
                "{locale} PARSE001.{issue} help must not be empty"
            );
            let mut diagnostic_args = vec![diagnostic_arg("issue", issue)];
            diagnostic_args.extend(
                args.iter()
                    .map(|(name, value)| diagnostic_arg(name, *value)),
            );
            let diagnostic = parse_diagnostic(ParseErrorKind::Expected, diagnostic_args);
            let rendered = installed_pack(locale)
                .render_diagnostic_text(&diagnostic)
                .unwrap_or_else(|err| panic!("{locale} PARSE001.{issue} template should render: {err:?}"))
                .unwrap_or_else(|| panic!("{locale} PARSE001 row should exist"));
            assert!(
                !rendered.message.contains("locale pack missing row"),
                "{locale} PARSE001.{issue}: {}",
                rendered.message
            );
            assert!(
                !rendered
                    .message
                    .contains("locale pack template missing placeholder")
                    && !rendered
                        .help
                        .as_deref()
                        .is_some_and(|help| help.contains("locale pack template missing placeholder")),
                "{locale} PARSE001.{issue} must not miss a template placeholder: {} / {:?}",
                rendered.message,
                rendered.help
            );
        }
    }
}

/// The five braced import record field names are reader-pack keyword rows in
/// every installed pack (`ex`, `visibilitas`, `nomen`, `ut`, `omnia`), the
/// parser resolves them through the pack, and every PARSE050 row that names a
/// field renders the pack's own words.
#[test]
fn installed_packs_own_the_import_record_field_words() {
    const FIELDS: [&str; 5] = ["ex", "visibilitas", "nomen", "ut", "omnia"];
    const EN: [&str; 5] = ["from", "visibility", "name", "as", "all"];
    for locale in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let pack = installed_pack(locale);
        let owned = owned_pack(locale);
        let words: Vec<&str> = FIELDS
            .iter()
            .map(|key| {
                owned
                    .keywords
                    .get(*key)
                    .unwrap_or_else(|| panic!("{locale} must own the [keywords] row {key}"))
                    .canonical
                    .as_str()
            })
            .collect();
        if locale == "en" {
            assert_eq!(words, EN);
        }
        if locale == "la" {
            assert_eq!(words, FIELDS);
        }

        let record = format!(
            "{import} {{ {ex} = \"norma:text\", {visibilitas} = {publica}, {nomen} = text, {ut} = t }}\n",
            import = owned.keywords["importa"].canonical,
            ex = words[0],
            visibilitas = words[1],
            publica = owned.keywords["publica"].canonical,
            nomen = words[2],
            ut = words[3],
        );
        let accepted = parse(lex_with_locale_pack(&record, &pack));
        assert!(accepted.errors.is_empty(), "{locale}: {record:?}: {:?}", accepted.errors);

        // Every PARSE050 row that names a field renders the pack's own words.
        for issue in [
            "unknown_import_field",
            "missing_import_source",
            "mixed_wildcard_and_named_import",
            "invalid_import_visibility",
            "import_name_inference_failed",
        ] {
            let diagnostic = parse_diagnostic(
                ParseErrorKind::ImportInvalid,
                vec![diagnostic_arg("issue", issue), diagnostic_arg("path", "p")],
            );
            let rendered = pack
                .render_diagnostic_text(&diagnostic)
                .unwrap_or_else(|err| panic!("{locale} PARSE050.{issue} should render: {err:?}"))
                .unwrap_or_else(|| panic!("{locale} PARSE050.{issue} row should exist"));
            let text = format!("{} {}", rendered.message, rendered.help.unwrap_or_default());
            if locale == "en" {
                for latin in FIELDS.iter().chain(["publica", "privata"].iter()) {
                    assert!(
                        !text
                            .split(|c: char| !c.is_alphanumeric())
                            .any(|word| word == *latin),
                        "en PARSE050.{issue} must not quote the Latin {latin}: {text}"
                    );
                }
            }
        }
    }

    // A Latin field word in an en record is refused by name; the en word is not.
    let en = installed_pack("en");
    let refused = parse(lex_with_locale_pack("import { ex = \"norma:text\" }\n", &en));
    assert!(
        refused.errors.iter().any(|error| {
            error.kind == ParseErrorKind::ImportInvalid
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "wrong_locale_import_field")
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "expected" && arg.value == "from")
        }),
        "{:?}",
        refused.errors
    );
    let diagnostic = Diagnostic::from_parse_error("parser.fab", "import { ex = \"norma:text\" }\n", &refused.errors[0]);
    let rendered = en
        .render_diagnostic_text(&diagnostic)
        .expect("wrong_locale_import_field renders")
        .expect("wrong_locale_import_field row");
    assert_eq!(rendered.help.as_deref(), Some("use `from` for this file's reader locale"));
}

/// PARSE050 `import_privata_removed` names the removed marker and the
/// re-export keyword through the pack's own `[keywords]` rows: en says
/// `private`/`public`, la says `privata`/`publica`, and the packs that own
/// the row name their own words. The parser passes no `keyword` argument,
/// so the pack rows own the words.
#[test]
fn import_privata_removed_names_the_pack_marker_words() {
    for locale in ["en", "la", "hi", "th-TH"] {
        let pack = installed_pack(locale);
        let owned = owned_pack(locale);
        let marker = owned
            .keywords
            .get("privata")
            .unwrap_or_else(|| panic!("{locale} must own the [keywords] row privata"))
            .canonical
            .clone();
        let reexport = owned
            .keywords
            .get("publica")
            .unwrap_or_else(|| panic!("{locale} must own the [keywords] row publica"))
            .canonical
            .clone();
        let diagnostic = parse_diagnostic(
            ParseErrorKind::ImportInvalid,
            vec![diagnostic_arg("issue", "import_privata_removed")],
        );
        let rendered = pack
            .render_diagnostic_text(&diagnostic)
            .unwrap_or_else(|err| panic!("{locale} PARSE050.import_privata_removed should render: {err:?}"))
            .unwrap_or_else(|| panic!("{locale} PARSE050.import_privata_removed row should exist"));
        let text = format!("{} {}", rendered.message, rendered.help.unwrap_or_default());
        assert!(text.contains(&marker), "{locale} must name the marker {marker}: {text}");
        assert!(text.contains(&reexport), "{locale} must name {reexport}: {text}");
        if locale == "en" {
            for latin in ["publica", "privata"] {
                assert!(
                    !text
                        .split(|c: char| !c.is_alphanumeric())
                        .any(|word| word == latin),
                    "en PARSE050.import_privata_removed must not quote the Latin {latin}: {text}"
                );
            }
        }
    }
}

/// The three `ratio`/`promissum` type-syntax rows and the PARSE002
/// declaration-block comma row (corpus coverage I3, batch 4) are owned and
/// rendered by every installed pack, with the placeholders each template names.
/// The absent-row budget gate counts missing rows, so it would accept a pack
/// that deletes one of these and adds an unrelated row elsewhere; name each row
/// here so a deletion fails by name.
///
/// `promissum_alternate_uses_exit_arrow` is emitted with
/// `ParseErrorKind::TypeInvalid` (`crates/radix-parser/src/types.rs:399`), which
/// `catalog_syntax::parse_spec` maps to PARSE040 — not PARSE001. The renderer
/// looks a row up by the diagnostic's code, so the row is owned under PARSE040.
#[test]
fn installed_packs_render_parse001_ratio_and_parse002_declaration_block_comma_rows() {
    const ROWS: &[(ParseErrorKind, &str, &str, &[(&str, &str)])] = &[
        (ParseErrorKind::Expected, "PARSE001", "ratio_field_list_hole_not_sole", &[]),
        (ParseErrorKind::Expected, "PARSE001", "ratio_type_arg_label_required", &[]),
        (
            ParseErrorKind::TypeInvalid,
            "PARSE040",
            "promissum_alternate_uses_exit_arrow",
            &[],
        ),
        (
            ParseErrorKind::Unexpected,
            "PARSE002",
            "declaration_block_comma",
            &[("family", "declaration_block"), ("position", "field")],
        ),
    ];
    for locale in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let owned = owned_pack(locale);
        for &(kind, code, issue, args) in ROWS {
            let row = owned
                .diagnostics
                .get(code)
                .and_then(|template| template.issues.get(issue))
                .unwrap_or_else(|| panic!("{locale} must own {code}.{issue}"));
            assert!(
                !row.message.trim().is_empty(),
                "{locale} {code}.{issue} message must not be empty"
            );
            assert!(
                row.help
                    .as_deref()
                    .is_some_and(|help| !help.trim().is_empty()),
                "{locale} {code}.{issue} help must not be empty"
            );
            let mut diagnostic_args = vec![diagnostic_arg("issue", issue)];
            diagnostic_args.extend(
                args.iter()
                    .map(|(name, value)| diagnostic_arg(name, *value)),
            );
            let diagnostic = parse_diagnostic(kind, diagnostic_args);
            assert_eq!(
                diagnostic.code,
                Some(code),
                "{locale} {code}.{issue} emission code must match the owned row"
            );
            let rendered = installed_pack(locale)
                .render_diagnostic_text(&diagnostic)
                .unwrap_or_else(|err| panic!("{locale} {code}.{issue} template should render: {err:?}"))
                .unwrap_or_else(|| panic!("{locale} {code} row should exist"));
            assert!(
                !rendered.message.contains("locale pack missing row"),
                "{locale} {code}.{issue}: {}",
                rendered.message
            );
            assert!(
                !rendered
                    .message
                    .contains("locale pack template missing placeholder")
                    && !rendered
                        .help
                        .as_deref()
                        .is_some_and(|help| help.contains("locale pack template missing placeholder")),
                "{locale} {code}.{issue} must not miss a template placeholder: {} / {:?}",
                rendered.message,
                rendered.help
            );
        }
    }
}

/// The four PARSE002 range/overflow rows (corpus coverage I3, batch 5) are
/// owned and rendered by every installed pack, with the placeholders each
/// template names. The absent-row budget gate counts missing rows, so it would
/// accept a pack that deletes one of these and adds an unrelated row elsewhere;
/// name each row here so a deletion fails by name.
#[test]
fn installed_packs_render_parse002_range_and_json_rows() {
    const ROWS: &[(&str, &[(&str, &str)])] = &[
        ("enum_value_out_of_range", &[]),
        ("exitus_value_out_of_range", &[]),
        ("json_integer_overflow", &[]),
        ("test_modifier_integer_out_of_range", &[("modifier", "repete")]),
    ];
    for locale in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let owned = owned_pack(locale);
        for &(issue, args) in ROWS {
            let row = owned
                .diagnostics
                .get("PARSE002")
                .and_then(|template| template.issues.get(issue))
                .unwrap_or_else(|| panic!("{locale} must own PARSE002.{issue}"));
            assert!(
                !row.message.trim().is_empty(),
                "{locale} PARSE002.{issue} message must not be empty"
            );
            assert!(
                row.help
                    .as_deref()
                    .is_some_and(|help| !help.trim().is_empty()),
                "{locale} PARSE002.{issue} help must not be empty"
            );
            let mut diagnostic_args = vec![diagnostic_arg("issue", issue)];
            diagnostic_args.extend(
                args.iter()
                    .map(|(name, value)| diagnostic_arg(name, *value)),
            );
            let diagnostic = parse_diagnostic(ParseErrorKind::Unexpected, diagnostic_args);
            assert_eq!(
                diagnostic.code,
                Some("PARSE002"),
                "{locale} PARSE002.{issue} emission code must match the owned row"
            );
            let rendered = installed_pack(locale)
                .render_diagnostic_text(&diagnostic)
                .unwrap_or_else(|err| panic!("{locale} PARSE002.{issue} template should render: {err:?}"))
                .unwrap_or_else(|| panic!("{locale} PARSE002 row should exist"));
            assert!(
                !rendered.message.contains("locale pack missing row"),
                "{locale} PARSE002.{issue}: {}",
                rendered.message
            );
            assert!(
                !rendered
                    .message
                    .contains("locale pack template missing placeholder")
                    && !rendered
                        .help
                        .as_deref()
                        .is_some_and(|help| help.contains("locale pack template missing placeholder")),
                "{locale} PARSE002.{issue} must not miss a template placeholder: {} / {:?}",
                rendered.message,
                rendered.help
            );
        }
    }
}

/// The two `@ nucleum` rows and the two parser-budget rows (corpus coverage I3,
/// batch 6) are owned and rendered by every installed pack, with the
/// placeholders each template names. The absent-row budget gate counts missing
/// rows, so it would accept a pack that deletes one of these and adds an
/// unrelated row elsewhere; name each row here so a deletion fails by name.
///
/// `nesting_too_deep` is emitted under the grammar area of the nesting site, so
/// the same slug is owned under PARSE020, PARSE030 and PARSE040; this test
/// names only the PARSE020 statement-area row.
#[test]
fn installed_packs_render_parse018_nucleum_and_depth_rows() {
    const ROWS: &[(ParseErrorKind, &str, &str, &[(&str, &str)])] = &[
        (ParseErrorKind::AnnotationInvalid, "PARSE018", "duplicate_nucleum", &[]),
        (
            ParseErrorKind::AnnotationInvalid,
            "PARSE018",
            "nucleum_fragment_conflicts_with_fragment",
            &[],
        ),
        (
            ParseErrorKind::StatementInvalid,
            "PARSE020",
            "nesting_too_deep",
            &[("limit", "256")],
        ),
        (
            ParseErrorKind::ExpressionInvalid,
            "PARSE030",
            "expression_too_long",
            &[("limit", "256")],
        ),
    ];
    for locale in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let owned = owned_pack(locale);
        for &(kind, code, issue, args) in ROWS {
            let row = owned
                .diagnostics
                .get(code)
                .and_then(|template| template.issues.get(issue))
                .unwrap_or_else(|| panic!("{locale} must own {code}.{issue}"));
            assert!(
                !row.message.trim().is_empty(),
                "{locale} {code}.{issue} message must not be empty"
            );
            assert!(
                row.help
                    .as_deref()
                    .is_some_and(|help| !help.trim().is_empty()),
                "{locale} {code}.{issue} help must not be empty"
            );
            let mut diagnostic_args = vec![diagnostic_arg("issue", issue)];
            diagnostic_args.extend(
                args.iter()
                    .map(|(name, value)| diagnostic_arg(name, *value)),
            );
            let diagnostic = parse_diagnostic(kind, diagnostic_args);
            assert_eq!(
                diagnostic.code,
                Some(code),
                "{locale} {code}.{issue} emission code must match the owned row"
            );
            let rendered = installed_pack(locale)
                .render_diagnostic_text(&diagnostic)
                .unwrap_or_else(|err| panic!("{locale} {code}.{issue} template should render: {err:?}"))
                .unwrap_or_else(|| panic!("{locale} {code} row should exist"));
            assert!(
                !rendered.message.contains("locale pack missing row"),
                "{locale} {code}.{issue}: {}",
                rendered.message
            );
            assert!(
                !rendered
                    .message
                    .contains("locale pack template missing placeholder")
                    && !rendered
                        .help
                        .as_deref()
                        .is_some_and(|help| help.contains("locale pack template missing placeholder")),
                "{locale} {code}.{issue} must not miss a template placeholder: {} / {:?}",
                rendered.message,
                rendered.help
            );
        }
    }
}

/// The PARSE030/PARSE040 depth rows and the two union-hole rows (corpus
/// coverage I3, batch 7) are owned and rendered by every installed pack, with
/// the placeholders each template names. The absent-row budget gate counts
/// missing rows, so it would accept a pack that deletes one of these and adds
/// an unrelated row elsewhere; name each row here so a deletion fails by name.
///
/// `nesting_too_deep` is emitted under the grammar area of the nesting site, so
/// the same slug is owned under PARSE020, PARSE030 and PARSE040; this test
/// names only the expression-area PARSE030 row and the type-area PARSE040 row.
#[test]
fn installed_packs_render_parse030_parse040_depth_and_union_hole_rows() {
    const ROWS: &[(ParseErrorKind, &str, &str, &[(&str, &str)])] = &[
        (
            ParseErrorKind::ExpressionInvalid,
            "PARSE030",
            "nesting_too_deep",
            &[("limit", "256")],
        ),
        (ParseErrorKind::TypeInvalid, "PARSE040", "nesting_too_deep", &[("limit", "256")]),
        (ParseErrorKind::TypeInvalid, "PARSE040", "unexpected_cap_after_union_hole", &[]),
        (ParseErrorKind::TypeInvalid, "PARSE040", "unexpected_cup_after_union_hole", &[]),
    ];
    for locale in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let owned = owned_pack(locale);
        for &(kind, code, issue, args) in ROWS {
            let row = owned
                .diagnostics
                .get(code)
                .and_then(|template| template.issues.get(issue))
                .unwrap_or_else(|| panic!("{locale} must own {code}.{issue}"));
            assert!(
                !row.message.trim().is_empty(),
                "{locale} {code}.{issue} message must not be empty"
            );
            assert!(
                row.help
                    .as_deref()
                    .is_some_and(|help| !help.trim().is_empty()),
                "{locale} {code}.{issue} help must not be empty"
            );
            let mut diagnostic_args = vec![diagnostic_arg("issue", issue)];
            diagnostic_args.extend(
                args.iter()
                    .map(|(name, value)| diagnostic_arg(name, *value)),
            );
            let diagnostic = parse_diagnostic(kind, diagnostic_args);
            assert_eq!(
                diagnostic.code,
                Some(code),
                "{locale} {code}.{issue} emission code must match the owned row"
            );
            let rendered = installed_pack(locale)
                .render_diagnostic_text(&diagnostic)
                .unwrap_or_else(|err| panic!("{locale} {code}.{issue} template should render: {err:?}"))
                .unwrap_or_else(|| panic!("{locale} {code} row should exist"));
            assert!(
                !rendered.message.contains("locale pack missing row"),
                "{locale} {code}.{issue}: {}",
                rendered.message
            );
            assert!(
                !rendered
                    .message
                    .contains("locale pack template missing placeholder")
                    && !rendered
                        .help
                        .as_deref()
                        .is_some_and(|help| help.contains("locale pack template missing placeholder")),
                "{locale} {code}.{issue} must not miss a template placeholder: {} / {:?}",
                rendered.message,
                rendered.help
            );
        }
    }
}

/// Assert every installed pack owns `code.issue` and renders it with its own
/// spellings of the named keyword identities.
fn assert_every_installed_pack_renders_issue(kind: ParseErrorKind, code: &str, issue: &str, keywords: &[&str]) {
    for locale in ["la", "en", "th-TH", "zh-Hans", "zh-Hant", "ar", "hi", "vi"] {
        assert_issue_template(&owned_pack(locale), code, issue);
        let pack = installed_pack(locale);
        let diagnostic = parse_diagnostic(kind, vec![diagnostic_arg("issue", issue)]);
        let rendered = pack
            .render_diagnostic_text(&diagnostic)
            .unwrap_or_else(|err| panic!("{locale} template should render: {err:?}"))
            .unwrap_or_else(|| panic!("{locale} {code} row should exist"));
        assert!(!rendered.message.contains("missing row"), "{locale}: {}", rendered.message);
        for keyword in keywords {
            let spelling = &pack.keywords.get(*keyword).expect("keyword row").canonical;
            assert!(
                rendered.message.contains(spelling.as_str())
                    || rendered
                        .help
                        .as_deref()
                        .is_some_and(|help| help.contains(spelling.as_str())),
                "{locale} {issue} must name `{spelling}`: {}",
                rendered.message
            );
        }
    }
}

/// A comma between `discerne` subjects reports PARSE020
/// `discerne_subject_comma_retired` and names the pack's own `et`.
#[test]
fn every_installed_pack_renders_discerne_subject_comma_retired() {
    assert_every_installed_pack_renders_issue(
        ParseErrorKind::StatementInvalid,
        "PARSE020",
        "discerne_subject_comma_retired",
        &["discerne", "et"],
    );
}

/// A `discerne` subject that is not a name or member path reports PARSE020
/// `discerne_subject_not_path` in every pack.
#[test]
fn every_installed_pack_renders_discerne_subject_not_path() {
    assert_every_installed_pack_renders_issue(
        ParseErrorKind::StatementInvalid,
        "PARSE020",
        "discerne_subject_not_path",
        &["discerne"],
    );
}

/// A comma between arm patterns reports PARSE023
/// `discerne_pattern_comma_retired` and names the pack's own `et`.
#[test]
fn every_installed_pack_renders_discerne_pattern_comma_retired() {
    assert_every_installed_pack_renders_issue(
        ParseErrorKind::PatternInvalid,
        "PARSE023",
        "discerne_pattern_comma_retired",
        &["casu", "et"],
    );
}

fn suggestion_candidate(file: &str, spelling: &str, suggestion: &str, span: Span) -> Diagnostic {
    Diagnostic::from_locale_suggestion(
        file,
        "0123456789012345678901234567890123456789",
        &LocaleSuggestion {
            pack_id: "test".to_owned(),
            spelling: spelling.to_owned(),
            suggestion: suggestion.to_owned(),
            span,
        },
    )
}

fn error_at(
    file: &str,
    phase: crate::diagnostics::DiagnosticPhase,
    code: &'static str,
    issue: &str,
    span: Span,
) -> Diagnostic {
    Diagnostic::error("error")
        .with_phase(phase)
        .with_code(code)
        .with_args(vec![DiagnosticArg::new("issue", issue)])
        .with_source_context(file, "0123456789012345678901234567890123456789", span)
}

fn kept_spellings(candidates: Vec<Diagnostic>, diagnostics: &[Diagnostic]) -> Vec<String> {
    Diagnostic::filter_locale_suggestions(candidates, diagnostics)
        .iter()
        .filter_map(|d| {
            d.args
                .iter()
                .find(|arg| arg.name == "spelling")
                .map(|arg| arg.value.clone())
        })
        .collect()
}

#[test]
fn locale_suggestion_filter_keeps_accent_matches_and_guesses_at_error_spans_only() {
    use crate::diagnostics::DiagnosticPhase;

    let guess = suggestion_candidate("a.fab", "clamq", "clamp", Span { start: 10, end: 15 });
    let accent = suggestion_candidate("a.fab", "ham", "hàm", Span { start: 20, end: 23 });
    let candidates = vec![guess, accent];

    // No error anywhere: the accent match stays, the edit-distance guess goes.
    assert_eq!(kept_spellings(candidates.clone(), &[]), vec!["ham"]);

    // A parse error overlapping the span, or a name-lookup failure at it, keeps the guess.
    for error in [
        error_at(
            "a.fab",
            DiagnosticPhase::Parse,
            "PARSE030",
            "expected_expression",
            Span { start: 12, end: 14 },
        ),
        error_at(
            "a.fab",
            DiagnosticPhase::Analysis,
            "SEM001",
            "unknown_identifier",
            Span { start: 10, end: 15 },
        ),
        error_at(
            "a.fab",
            DiagnosticPhase::Analysis,
            "SEM008",
            "undefined_name",
            Span { start: 10, end: 15 },
        ),
    ] {
        assert_eq!(kept_spellings(candidates.clone(), &[error]), vec!["clamq", "ham"]);
    }

    // Errors elsewhere in the file, in another file, or of another kind keep it out.
    for error in [
        error_at(
            "a.fab",
            DiagnosticPhase::Parse,
            "PARSE030",
            "expected_expression",
            Span { start: 30, end: 33 },
        ),
        error_at(
            "b.fab",
            DiagnosticPhase::Parse,
            "PARSE030",
            "expected_expression",
            Span { start: 10, end: 15 },
        ),
        error_at(
            "a.fab",
            DiagnosticPhase::Analysis,
            "SEM001",
            "scribe_callee",
            Span { start: 10, end: 15 },
        ),
        error_at(
            "a.fab",
            DiagnosticPhase::Analysis,
            "SEM010",
            "type_mismatch",
            Span { start: 10, end: 15 },
        ),
    ] {
        assert_eq!(kept_spellings(candidates.clone(), &[error]), vec!["ham"]);
    }
}

// ---------------------------------------------------------------------------
// Translated-pack rows must not ship English text (need, R12).
//
// The key-set completeness tests above prove a row EXISTS in each pack; they
// cannot see a row whose text was copied from `en` unchanged, and two rulings
// of 2026-10-13 lean on that distinction. The two tests below close the gap:
// every diagnostics text field of a translated pack that still equals the
// `en` text must carry an exemption in its pack's `locale/identical-to-en/<pack>.toml`, and
// every exemption must still be live. `la` is deliberately outside the gate:
// it is the canonical Latin pack whose issue rows were English before the R12
// range, so its rows are fallback data, not translation debt.
// ---------------------------------------------------------------------------

/// Packs the identical-to-en gate treats as real translations (every installed
/// pack except the `en` comparison source and the canonical `la`).
const TRANSLATED_PACKS: &[&str] = &["ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"];

/// Current exemption-entry count across `locale/identical-to-en/<pack>.toml`.
///
/// The list may only shrink: lower this constant in the same change that
/// deletes an entry, and never raise it. `identical_to_en_entries_are_unique`
/// closes the duplicate route into the same budget.
const IDENTICAL_TO_EN_ENTRY_BUDGET: usize = 6;

/// `locale/identical-to-en/`, holding one shrink-only exemption list per pack.
fn identical_to_en_dir() -> PathBuf {
    crate::locale::workspace_locale_root().join("identical-to-en")
}

/// `locale/identical-to-en/<pack>.toml`, one pack's shrink-only exemption list.
fn identical_to_en_path(pack: &str) -> PathBuf {
    identical_to_en_dir().join(format!("{pack}.toml"))
}

/// `locale/absent-rows-budget.toml`, one shrink-only absent-row budget per pack.
fn absent_rows_budget_path() -> PathBuf {
    crate::locale::workspace_locale_root().join("absent-rows-budget.toml")
}

#[derive(serde::Deserialize)]
struct IdenticalToEnFile {
    #[serde(default)]
    entry: Vec<IdenticalToEnEntry>,
}

#[derive(serde::Deserialize)]
struct IdenticalToEnEntry {
    pack: String,
    code: String,
    /// Absent for a code's base row; present for an issue row.
    #[serde(default)]
    issue: Option<String>,
    /// `message` or `help` — every text field the row type has.
    field: String,
    reason: String,
}

fn load_identical_to_en_entries() -> Vec<IdenticalToEnEntry> {
    let mut entries = Vec::new();
    for pack in TRANSLATED_PACKS {
        let path = identical_to_en_path(pack);
        let source = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err:?}", path.display()));
        let parsed: IdenticalToEnFile =
            toml::from_str(&source).unwrap_or_else(|err| panic!("parse {}: {err:?}", path.display()));
        for entry in parsed.entry {
            assert!(
                entry.pack == *pack,
                "{}: entry {} names pack {:?}, expected {:?}",
                path.display(),
                entry.code,
                entry.pack,
                pack
            );
            entries.push(entry);
        }
    }
    entries
}

/// The verdict for the identical-to-en exemption list length against its
/// budget. The list may only shrink, so fewer entries is fine and more is
/// growth the gate must refuse.
fn growth_verdict(entries: usize, budget: usize) -> Result<(), String> {
    if entries > budget {
        return Err(format!(
            "identical-to-en list grew to {entries} entries against a budget of {budget}"
        ));
    }
    Ok(())
}

#[test]
fn identical_to_en_growth_equal_is_ok() {
    assert_eq!(growth_verdict(6, 6), Ok(()));
}

#[test]
fn identical_to_en_growth_one_more_is_err() {
    let err = growth_verdict(7, 6).expect_err("growth must be an error");
    assert!(
        err.contains('7') && err.contains('6'),
        "error should name both numbers: {err:?}"
    );
}

#[test]
fn identical_to_en_growth_fewer_is_ok() {
    assert_eq!(growth_verdict(5, 6), Ok(()));
}

/// The exemption list is budgeted (`IDENTICAL_TO_EN_ENTRY_BUDGET`): adding an
/// entry fails this gate until the constant is deliberately changed, and the
/// constant may only go down as entries are deleted.
#[test]
fn locale_pack_completeness_identical_to_en_list_does_not_grow() {
    let entries = load_identical_to_en_entries().len();
    growth_verdict(entries, IDENTICAL_TO_EN_ENTRY_BUDGET).unwrap_or_else(|err| panic!("{err}"));
}

/// Two entries naming the same (pack, code, issue, field) are growth by
/// another route: the second can never exempt a row the first does not, so a
/// duplicate is dead weight that must fail before it lands.
#[test]
fn identical_to_en_entries_are_unique() {
    let mut seen = BTreeSet::new();
    let mut duplicates = Vec::new();
    for entry in load_identical_to_en_entries() {
        let name = format!(
            "{} {} {} {}",
            entry.pack,
            entry.code,
            entry.issue.as_deref().unwrap_or("-"),
            entry.field
        );
        let key = (entry.pack, entry.code, entry.issue, entry.field);
        if !seen.insert(key) {
            duplicates.push(name);
        }
    }
    assert!(
        duplicates.is_empty(),
        "identical-to-en entries repeat a (pack, code, issue, field): {}",
        duplicates.join(", ")
    );
}

/// The two sanctioned reason classes for exemption entries.
fn reason_class_is_sanctioned(reason: &str) -> bool {
    reason == "language-neutral" || reason.starts_with("untranslated-debt:")
}

/// A field is exempt when an entry names the exact (pack, code, issue, field).
fn is_exempt(exemptions: &[IdenticalToEnEntry], pack: &str, code: &str, issue: Option<&str>, field: &str) -> bool {
    exemptions.iter().any(|entry| {
        entry.pack == pack && entry.code == code && entry.issue.as_deref() == issue && entry.field == field
    })
}

/// Every text field of `pack` whose value equals the `en` pack's value for the
/// same code/issue/field, as `(code, issue, field)` triples.
///
/// Rows or issue rows missing from `en` are skipped: their absence is the
/// scaffold key-set gate's business, not an equality fact. A translated row
/// missing only `help` compares `message` alone.
fn fields_identical_to_en(pack: &LocalePack, en_pack: &LocalePack) -> Vec<(String, Option<String>, String)> {
    let mut identical = Vec::new();
    for (code, template) in &pack.diagnostics {
        let Some(en_template) = en_pack.diagnostics.get(code) else {
            continue;
        };
        let rows =
            std::iter::once((None, template, en_template)).chain(template.issues.iter().filter_map(|(issue, row)| {
                en_template
                    .issues
                    .get(issue)
                    .map(|en_row| (Some(issue.as_str()), row, en_row))
            }));
        for (issue, row, en_row) in rows {
            for (field, ours, theirs) in [
                ("message", Some(row.message.as_str()), Some(en_row.message.as_str())),
                ("help", row.help.as_deref(), en_row.help.as_deref()),
            ] {
                if ours.is_some() && ours == theirs {
                    identical.push((code.clone(), issue.map(str::to_string), field.to_string()));
                }
            }
        }
    }
    identical
}

/// Every diagnostics text field of `en_pack` that `pack` does not have, as
/// `(code, issue, field)` triples, sorted for a deterministic report.
///
/// `issue` is `None` for a code's base row. A missing code row reports its
/// base `message` (and `help` when `en` has one) plus every issue row of that
/// code; a missing issue row reports its `message` (and `help` when `en` has
/// one). A row that exists but drops `help` reports only that field: `message`
/// is not optional in the pack shape, so a present row never lacks it.
fn fields_absent_from_pack(pack: &LocalePack, en_pack: &LocalePack) -> Vec<(String, Option<String>, String)> {
    let mut absent = Vec::new();
    for (code, en_template) in &en_pack.diagnostics {
        let template = pack.diagnostics.get(code);
        let rows = std::iter::once((None, en_template)).chain(
            en_template
                .issues
                .iter()
                .map(|(issue, row)| (Some(issue.as_str()), row)),
        );
        for (issue, en_row) in rows {
            let row = match issue {
                Some(issue) => template.and_then(|template| template.issues.get(issue)),
                None => template,
            };
            let Some(row) = row else {
                absent.push((code.clone(), issue.map(str::to_string), "message".to_string()));
                if en_row.help.is_some() {
                    absent.push((code.clone(), issue.map(str::to_string), "help".to_string()));
                }
                continue;
            };
            if en_row.help.is_some() && row.help.is_none() {
                absent.push((code.clone(), issue.map(str::to_string), "help".to_string()));
            }
        }
    }
    absent.sort();
    absent
}

#[test]
fn fields_absent_from_pack_reports_missing_issue_row_and_missing_help() {
    let en_pack = LocalePack::from_toml_str(
        r#"
[pack]
id = "en"
fallback = ["la"]

[diagnostics.PARSE011]
message = "expected function name"
help = "add a function name"

[diagnostics.PARSE011.issues.dropped_row]
message = "issue row without help"

[diagnostics.PARSE011.issues.dropped_help]
message = "issue row with help"
help = "issue help text"

[llm]
system_prompt_snippet = "emit English reader-locale Faber"
exemplars = ["./exemplars/salve-munde.en.fab"]
"#,
    )
    .expect("en fixture pack should validate");

    // The translated pack keeps the complete code row and only
    // `dropped_help`'s message: exactly two absences.
    let translated = LocalePack::from_toml_str(
        r#"
[pack]
id = "th-TH"
fallback = ["la"]

[diagnostics.PARSE011]
message = "ข้อความหลัก"
help = "ความช่วยเหลือหลัก"

[diagnostics.PARSE011.issues.dropped_help]
message = "ข้อความปัญหา"

[llm]
system_prompt_snippet = "emit Thai reader-locale Faber"
exemplars = ["./exemplars/salve-munde.th-TH.fab"]
"#,
    )
    .expect("translated fixture pack should validate");

    assert_eq!(
        fields_absent_from_pack(&translated, &en_pack),
        vec![
            ("PARSE011".to_string(), Some("dropped_help".to_string()), "help".to_string()),
            ("PARSE011".to_string(), Some("dropped_row".to_string()), "message".to_string()),
        ]
    );

    let complete = LocalePack::from_toml_str(
        r#"
[pack]
id = "vi"
fallback = ["la"]

[diagnostics.PARSE011]
message = "thông báo chính"
help = "trợ giúp chính"

[diagnostics.PARSE011.issues.dropped_row]
message = "hàng vấn đề không có trợ giúp"

[diagnostics.PARSE011.issues.dropped_help]
message = "hàng vấn đề có trợ giúp"
help = "trợ giúp vấn đề"

[llm]
system_prompt_snippet = "emit Vietnamese reader-locale Faber"
exemplars = ["./exemplars/salve-munde.vi.fab"]
"#,
    )
    .expect("complete fixture pack should validate");

    assert_eq!(fields_absent_from_pack(&complete, &en_pack), Vec::new());
}

/// The verdict for a pack's absent-row count against its exact budget.
///
/// The budget is exact: it may only go down, and it must be lowered in the
/// same change that translates rows. Under budget is therefore an error too,
/// not harmless slack.
fn budget_verdict(actual: usize, budget: usize) -> Result<(), String> {
    match actual.cmp(&budget) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Greater => Err(format!(
            "pack is over its absent-rows budget: {actual} absent rows against a budget of {budget}"
        )),
        std::cmp::Ordering::Less => Err(format!("lower the budget to {actual}")),
    }
}

#[test]
fn absent_rows_budget_equal_is_ok() {
    assert_eq!(budget_verdict(7, 7), Ok(()));
}

#[test]
fn absent_rows_budget_actual_over_budget_is_err() {
    let err = budget_verdict(6, 5).expect_err("over budget must be an error");
    assert!(
        err.contains('6') && err.contains('5'),
        "error should name both numbers: {err:?}"
    );
    assert!(err.contains("over"), "error should say the pack is over budget: {err:?}");
}

#[test]
fn absent_rows_budget_actual_under_budget_is_err() {
    let err = budget_verdict(3, 5).expect_err("under budget must be an error");
    // The budget is exact: it may only go down, and the fix is to lower it.
    assert!(err.contains("lower the budget to 3"), "error should say how to fix it: {err:?}");
}

/// A translated pack must not carry diagnostics text copied from `en`.
///
/// Equality with `en` is tolerated only through `locale/identical-to-en/<pack>.toml`,
/// so copied English is always a recorded, reviewable decision. The list may
/// only shrink: a translation must delete its entry.
#[test]
fn locale_pack_completeness_translated_packs_do_not_ship_english_rows_unless_exempt() {
    let en_pack = owned_pack("en");
    let exemptions = load_identical_to_en_entries();
    let mut failures = Vec::new();
    for locale in TRANSLATED_PACKS {
        let pack = owned_pack(locale);
        for (code, issue, field) in fields_identical_to_en(&pack, &en_pack) {
            if !is_exempt(&exemptions, locale, &code, issue.as_deref(), &field) {
                let row = issue
                    .as_ref()
                    .map_or_else(String::new, |i| format!("issues.{i}."));
                failures.push(format!("{locale} ships English text for {code}.{row}{field}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "translated packs ship diagnostics text byte-equal to en without an exemption in {}:\n  {}",
        identical_to_en_dir().display(),
        failures.join("\n  ")
    );
}

/// An exemption entry must name a live field that still equals `en`.
///
/// The list cannot rot: a deleted row, a deleted field, a renamed issue, or a
/// field that HAS been translated makes the entry stale, and the test fails
/// until the entry is removed (or corrected). Unknown packs, unknown fields,
/// and off-vocabulary reasons are stale too.
#[test]
fn locale_pack_completeness_identical_to_en_exemptions_must_stay_live() {
    let en_pack = owned_pack("en");
    let exemptions = load_identical_to_en_entries();
    let packs: HashMap<&str, LocalePack> = TRANSLATED_PACKS
        .iter()
        .map(|locale| (*locale, owned_pack(locale)))
        .collect();
    let mut stale = Vec::new();
    for entry in &exemptions {
        let name = format!(
            "{} {} {} {}",
            entry.pack,
            entry.code,
            entry.issue.as_deref().unwrap_or("-"),
            entry.field
        );
        let Some(pack) = packs.get(entry.pack.as_str()) else {
            stale.push(format!("{name}: pack is not one of the translated packs this gate covers"));
            continue;
        };
        let Some(template) = pack.diagnostics.get(&entry.code) else {
            stale.push(format!("{name}: row no longer exists; delete the entry"));
            continue;
        };
        let row = match entry.issue.as_deref() {
            Some(issue) => match template.issues.get(issue) {
                Some(row) => row,
                None => {
                    stale.push(format!("{name}: issue row no longer exists; delete the entry"));
                    continue;
                }
            },
            None => template,
        };
        let ours = match entry.field.as_str() {
            "message" => Some(row.message.as_str()),
            "help" => row.help.as_deref(),
            _ => None,
        };
        let Some(ours) = ours else {
            stale.push(format!("{name}: field no longer exists; delete the entry"));
            continue;
        };
        let Some(theirs) = english_field_text(&en_pack, &entry.code, entry.issue.as_deref(), &entry.field) else {
            stale.push(format!("{name}: the en side of the field no longer exists; delete the entry"));
            continue;
        };
        if ours != theirs {
            stale.push(format!(
                "{name}: translated ({ours:?} != {theirs:?}); delete the entry — the list may only shrink"
            ));
        }
        if !reason_class_is_sanctioned(&entry.reason) {
            stale.push(format!(
                "{name}: reason {:?} is not one of the two sanctioned classes (language-neutral, untranslated-debt)",
                entry.reason
            ));
        }
    }
    assert!(
        stale.is_empty(),
        "stale exemptions in {}:\n  {}",
        identical_to_en_dir().display(),
        stale.join("\n  ")
    );
}

/// `locale/absent-rows-budget.toml`.
#[derive(serde::Deserialize)]
struct AbsentRowsBudgetFile {
    budget: BTreeMap<String, usize>,
}

/// A translated pack's absent-row count must equal its recorded budget.
///
/// The budget file is exact and shrink-only: over budget is unrecorded
/// translation debt, and under budget means a translation landed without
/// lowering the number in the same change. The `en` pack is the comparison
/// source and `la` is outside the gate, matching the identical-to-en gate.
#[test]
fn locale_pack_completeness_translated_packs_have_no_unbudgeted_absent_rows() {
    let path = absent_rows_budget_path();
    let source = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err:?}", path.display()));
    let parsed: AbsentRowsBudgetFile =
        toml::from_str(&source).unwrap_or_else(|err| panic!("parse {}: {err:?}", path.display()));
    let en_pack = owned_pack("en");
    let mut failures = Vec::new();
    for locale in TRANSLATED_PACKS {
        let Some(&budget) = parsed.budget.get(*locale) else {
            panic!("{} has no budget entry for {locale}", path.display());
        };
        let pack = owned_pack(locale);
        let absent = fields_absent_from_pack(&pack, &en_pack);
        let actual = absent.len();
        if let Err(err) = budget_verdict(actual, budget) {
            let mut failure = format!("{locale}: {err}");
            if actual > budget {
                let keys: Vec<String> = absent
                    .iter()
                    .take(20)
                    .map(|(code, issue, field)| match issue {
                        Some(issue) => format!("{code}.issues.{issue}.{field}"),
                        None => format!("{code}.{field}"),
                    })
                    .collect();
                failure.push_str(&format!("\n    {}", keys.join("\n    ")));
            }
            failures.push(failure);
        }
    }
    assert!(
        failures.is_empty(),
        "translated packs have unbudgeted absent rows against {}:\n  {}",
        path.display(),
        failures.join("\n  ")
    );
}

/// The `en` pack's text for one exempted field, if the row and field exist.
fn english_field_text<'a>(en_pack: &'a LocalePack, code: &str, issue: Option<&str>, field: &str) -> Option<&'a str> {
    let template = en_pack.diagnostics.get(code)?;
    let row = match issue {
        Some(issue) => template.issues.get(issue)?,
        None => template,
    };
    match field {
        "message" => Some(row.message.as_str()),
        "help" => row.help.as_deref(),
        _ => None,
    }
}

/// The brace-bearing pack rows (corpus coverage I3, template escaping) must
/// render their literal braces instead of being read as placeholders. `{{` is
/// the renderer's literal-brace escape, so a pack doubles the
/// opening brace of a quoted braced form. LEX003 is owned by all eight packs;
/// the SEM016/PARSE002 brace rows are `en`/`la` rows, so the other packs fall
/// back to their base row and are only checked for a clean render.
#[test]
fn installed_packs_render_braced_templates_literally() {
    const PLAIN_ROWS: &[(&str, &str, &[(&str, &str)])] = &[
        ("PARSE002", "construction_spread_removed", &[("sparge", "…"), ("ex", "…")]),
        ("SEM016", "record_conversion_requires_reconstruction", &[]),
        ("SEM016", "inferred_constructor_ambiguous_union", &[]),
        ("SEM016", "inferred_constructor_entries_need_map", &[]),
        ("SEM016", "inferred_constructor_fields_for_map", &[]),
    ];
    for locale in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let pack = installed_pack(locale);

        let escape = diagnostic_with_code(
            "LEX003",
            vec![diagnostic_arg("issue", "unknown_escape"), diagnostic_arg("escape", "q")],
        );
        let rendered = pack
            .render_diagnostic_text(&escape)
            .unwrap_or_else(|err| panic!("{locale} LEX003.unknown_escape should render: {err:?}"))
            .unwrap_or_else(|| panic!("{locale} LEX003 row should exist"));
        assert!(
            rendered.message.contains("u{...}"),
            "{locale} LEX003.unknown_escape must render `u{{{{...}}` as `u{{...}}`: {}",
            rendered.message
        );
        assert!(
            !rendered
                .message
                .contains("locale pack template missing placeholder"),
            "{locale} LEX003.unknown_escape must not miss a template placeholder: {}",
            rendered.message
        );

        let no_expected =
            diagnostic_with_code("SEM016", vec![diagnostic_arg("issue", "inferred_constructor_no_expected_type")]);
        let rendered = pack
            .render_diagnostic_text(&no_expected)
            .unwrap_or_else(|err| {
                panic!("{locale} SEM016.inferred_constructor_no_expected_type should render: {err:?}")
            })
            .unwrap_or_else(|| panic!("{locale} SEM016 row should exist"));
        assert!(
            !rendered
                .message
                .contains("locale pack template missing placeholder"),
            "{locale} SEM016.inferred_constructor_no_expected_type must not miss a template placeholder: {}",
            rendered.message
        );
        // The row is `en`/`la` owned; the other packs fall back to the base
        // SEM016 row, so only the owning templates can prove the literal brace.
        if pack
            .diagnostics
            .get("SEM016")
            .and_then(|template| template.issues.get("inferred_constructor_no_expected_type"))
            .is_some()
        {
            assert!(
                rendered.message.contains("{ … }"),
                "{locale} SEM016.inferred_constructor_no_expected_type must render its literal braces: {}",
                rendered.message
            );
        }

        for &(code, issue, args) in PLAIN_ROWS {
            let mut diagnostic_args = vec![diagnostic_arg("issue", issue)];
            diagnostic_args.extend(
                args.iter()
                    .map(|(name, value)| diagnostic_arg(name, *value)),
            );
            let diagnostic = diagnostic_with_code(code, diagnostic_args);
            let rendered = pack
                .render_diagnostic_text(&diagnostic)
                .unwrap_or_else(|err| panic!("{locale} {code}.{issue} should render: {err:?}"))
                .unwrap_or_else(|| panic!("{locale} {code} row should exist"));
            assert!(
                !rendered
                    .message
                    .contains("locale pack template missing placeholder")
                    && !rendered
                        .help
                        .as_deref()
                        .is_some_and(|help| help.contains("locale pack template missing placeholder")),
                "{locale} {code}.{issue} must not miss a template placeholder: {} / {:?}",
                rendered.message,
                rendered.help
            );
        }
    }
}
