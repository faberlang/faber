//! Completeness gate for issue keys selected by the syntax and semantic
//! diagnostic catalogs.
//!
//! The catalog renderers use exhaustive matches over issue strings rather
//! than a separate issue registry. This test reads those match patterns and
//! checks that the English and Latin packs own each selected issue key. The
//! source catalogs are included so adding a new match arm without a pack row
//! fails the gate.

use super::LocalePack;
use radix_diagnostics::DiagnosticArg;
use radix_semantic::catalog_semantic::semantic_help;
use radix_semantic::error::SemanticErrorKind;
use std::collections::BTreeSet;

const SEMANTIC_CATALOG: &str = include_str!("../../radix-semantic/src/catalog_semantic.rs");
const SYNTAX_CATALOG: &str = include_str!("../../radix-semantic/src/catalog_syntax.rs");

const INSTALLED_LOCALES: &[&str] = &["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"];
const SEM008_ISSUES: &[&str] = &["callable_posture_conflict", "comment_without_owner"];
/// SEM064 issues raised by build-time evaluation, outside the catalog `match`.
const SEM064_ISSUES: &[&str] = &["praefixum_import_unavailable"];

fn owned_installed_pack(locale: &str) -> LocalePack {
    let path = crate::locale::workspace_locale_pack_path(locale);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read installed {locale} pack: {err:?}"));
    LocalePack::from_toml_str(&source)
        .unwrap_or_else(|err| panic!("installed {locale} pack should parse owned rows: {err:?}"))
}

fn starts_issue_match(line: &str) -> bool {
    line.contains("match arg_value(args, \"issue\")")
        || line.contains("match issue {")
        || line.contains("match issue? {")
}

fn brace_delta_outside_strings_and_comments(line: &str) -> isize {
    let mut delta = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            break;
        }
        match ch {
            '"' => in_string = true,
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }
    delta
}

fn arm_pattern_before_arrow(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0;
    while i + 1 < bytes.len() {
        let byte = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'/' && bytes[i + 1] == b'/' {
            return None;
        } else if byte == b'"' {
            in_string = true;
        } else if byte == b'=' && bytes[i + 1] == b'>' {
            return Some(&line[..i]);
        }
        i += 1;
    }
    None
}

fn identifier_literals(pattern: &str) -> impl Iterator<Item = String> + '_ {
    let bytes = pattern.as_bytes();
    let mut values = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        i += 1;
        let start = i;
        let mut escaped = false;
        while i < bytes.len() {
            if escaped {
                escaped = false;
            } else if bytes[i] == b'\\' {
                escaped = true;
            } else if bytes[i] == b'"' {
                let value = &pattern[start..i];
                if !value.is_empty()
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    values.push(value.to_owned());
                }
                i += 1;
                break;
            }
            i += 1;
        }
    }
    values.into_iter()
}

fn catalog_issue_keys(source: &str) -> BTreeSet<String> {
    let mut issues = BTreeSet::new();
    let mut in_issue_match = false;
    let mut depth = 0isize;

    for line in source.lines() {
        if !in_issue_match && starts_issue_match(line) {
            in_issue_match = true;
            depth = 0;
        }
        if !in_issue_match {
            continue;
        }

        if depth == 1
            && let Some(pattern) = arm_pattern_before_arrow(line)
        {
            issues.extend(identifier_literals(pattern));
        }

        depth += brace_delta_outside_strings_and_comments(line);
        if depth <= 0 {
            in_issue_match = false;
            depth = 0;
        }
    }

    issues
}

fn issue_families(pack: &LocalePack, issue: &str) -> BTreeSet<String> {
    pack.diagnostics
        .iter()
        .filter(|(_, family)| family.issues.contains_key(issue))
        .map(|(code, _)| code.clone())
        .collect()
}

fn all_pack_issue_keys(pack: &LocalePack) -> BTreeSet<String> {
    pack.diagnostics
        .values()
        .flat_map(|family| family.issues.keys().cloned())
        .collect()
}

fn installed_issue_row_has_message_and_help(locale: &str, issue: &str) -> bool {
    let path = crate::locale::workspace_locale_pack_path(locale);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read installed {locale} pack: {err:?}"));
    let pack: toml::Value =
        toml::from_str(&source).unwrap_or_else(|err| panic!("installed {locale} pack should be valid TOML: {err:?}"));
    pack.get("diagnostics")
        .and_then(|diagnostics| diagnostics.get("SEM008"))
        .and_then(|family| family.get("issues"))
        .and_then(|issues| issues.get(issue))
        .is_some_and(|row| row.get("message").is_some() && row.get("help").is_some())
}

#[test]
fn en_and_la_cover_every_catalog_issue_key() {
    let mut catalog_issues = catalog_issue_keys(SEMANTIC_CATALOG);
    catalog_issues.extend(catalog_issue_keys(SYNTAX_CATALOG));
    catalog_issues.insert(radix_lexer::HOMOGLYPH_ISSUE.to_owned());
    catalog_issues.extend(SEM008_ISSUES.iter().map(|issue| (*issue).to_owned()));
    catalog_issues.extend(SEM064_ISSUES.iter().map(|issue| (*issue).to_owned()));

    let en = owned_installed_pack("en");
    let la = owned_installed_pack("la");
    for (locale, pack) in [("en", &en), ("la", &la)] {
        let present = all_pack_issue_keys(pack);
        let missing: BTreeSet<String> = catalog_issues
            .iter()
            .filter(|issue| !present.contains(*issue))
            .cloned()
            .collect();
        assert!(missing.is_empty(), "{locale} pack has no row for catalog issues: {missing:?}");
    }

    for issue in &catalog_issues {
        assert_eq!(
            issue_families(&en, issue),
            issue_families(&la, issue),
            "en and la must keep {issue} under the same diagnostic code"
        );
    }
}

#[test]
fn sem008_comment_and_callable_posture_rows_exist_in_every_installed_pack() {
    for &locale in INSTALLED_LOCALES {
        let pack = owned_installed_pack(locale);
        let sem008 = pack.diagnostics.get("SEM008").expect("SEM008 family row");
        for &issue in SEM008_ISSUES {
            assert!(sem008.issues.contains_key(issue), "{locale} is missing SEM008.{issue}");
            assert!(
                installed_issue_row_has_message_and_help(locale, issue),
                "{locale} SEM008.{issue} needs message and help fields"
            );
        }
    }
}

fn installed_cell(locale: &str, code: &str, issue: &str, field: &str) -> String {
    let path = crate::locale::workspace_locale_pack_path(locale);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read installed {locale} pack: {err:?}"));
    let pack: toml::Value =
        toml::from_str(&source).unwrap_or_else(|err| panic!("installed {locale} pack should be valid TOML: {err:?}"));
    pack.get("diagnostics")
        .and_then(|diagnostics| diagnostics.get(code))
        .and_then(|family| family.get("issues"))
        .and_then(|issues| issues.get(issue))
        .and_then(|row| row.get(field))
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("{locale} {code}.{issue}.{field} must be a string cell"))
        .to_owned()
}

#[test]
fn no_installed_pack_string_uses_retired_width_generic_forms() {
    for &locale in INSTALLED_LOCALES {
        let path = crate::locale::workspace_locale_pack_path(locale);
        let source =
            std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read installed {locale} pack: {err:?}"));
        for form in ["numerus<", "int<", "float<", "fractus<"] {
            assert!(
                !source.contains(form),
                "{locale} pack must not contain the retired width form {form}"
            );
        }
    }
}

#[test]
fn en_via_bits_retired_help_equals_the_catalog_help() {
    let catalog = semantic_help(
        SemanticErrorKind::ConversionInvalid,
        &[DiagnosticArg::new("issue", "conversio_via_bits_retired")],
    )
    .expect("catalog carries help for conversio_via_bits_retired");
    assert_eq!(installed_cell("en", "SEM016", "conversio_via_bits_retired", "help"), catalog);
}
