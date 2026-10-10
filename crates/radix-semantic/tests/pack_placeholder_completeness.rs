//! Pack-template placeholder completeness.
//!
//! One diagnostic whose pack template names a placeholder its emit site does
//! not supply used to abort the whole run. The renderer now falls back to the
//! base message, but the pack and the emit site must still agree: every
//! required `{name}` in a base message/help row is either a keyword/type
//! binding of the same pack or an arg the emit sites of that code supply.
//!
//! `SUPPLIED` lists the codes whose emit sites pass a fixed arg list with no
//! `issue` row; it was read off the emit sites named beside each entry.
//! PARSE012 is not read from that table: its args come from parsing a real
//! bodyless function, so a pack or emit-site drift fails here.
//! Issue rows are out of scope: their arg lists vary per issue.

use radix_diagnostics::{Diagnostic, DiagnosticArg};
use radix_semantic::locale::LocalePack;
use std::collections::BTreeSet;
use std::path::PathBuf;

const LOCALES: [&str; 8] = ["en", "la", "ar", "hi", "vi", "th-TH", "zh-Hans", "zh-Hant"];

/// Args supplied by the emit sites of a code, beyond keyword/type bindings.
const SUPPLIED: &[(&str, &[&str])] = &[
    // radix-parser stmt.rs: missing_block_error
    ("PARSE022", &["block", "ergo"]),
    // radix-parser pattern.rs: PatternInvalid
    ("PARSE023", &["wildcard", "identifier"]),
    // radix-semantic passes/resolve.rs: break/continue/return outside context
    ("SEM030", &["keyword", "context"]),
    ("SEM031", &["keyword", "context"]),
    ("SEM032", &["keyword", "context"]),
    // radix-module diagnostics/convert.rs: convert_from_locale_suggestion
    ("LOCALE002", &["pack", "spelling", "suggestion"]),
    // radix-module diagnostics/convert.rs: convert_from_mixed_script_advisory
    ("LOCALE004", &["identifier", "scripts"]),
    // radix-module driver/mod.rs: resolve_frontmatter_locale_pack passes
    // `locale_id` on every site that knows the id; the sites that do not
    // (non-string key, unresolved session) use `issue` rows.
    ("LOCALE003", &["locale_id"]),
];

/// Known mismatches. Each must remain a gap: when the emit site is fixed the
/// entry must go.
const KNOWN_GAPS: &[(&str, &str)] = &[
    // The UNRES specs are registered in radix-diagnostics but no emit site
    // exists anywhere in the tree (need: grep finds only the spec
    // definitions and the pack rows), so there is no site to supply args.
    ("UNRES001", "keyword"),
    ("UNRES002", "intrinsic"),
    ("UNRES003", "identity"),
];

fn load_pack(locale: &str) -> LocalePack {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../locale")
        .join(locale)
        .join("pack.toml");
    LocalePack::from_toml_path(path).expect("validated reference pack")
}

/// Required `{name}` placeholders of one template string.
fn required_placeholders(template: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        let name = &after[..end];
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            names.insert(name.to_owned());
        }
        rest = &after[end + 1..];
    }
    names
}

/// Arg names the emit sites of `code` supply.
fn supplied_args(code: &str) -> BTreeSet<&'static str> {
    if code == "PARSE012" {
        let result = radix_parser::parse(radix_lexer::lex("functio parse(textus s) → textus\n"));
        let error = result
            .errors
            .iter()
            .find(|err| err.kind == radix_parser::ParseErrorKind::FunctionBodyAbsent)
            .expect("a bodyless function reports FunctionBodyAbsent");
        return error.args.iter().map(|arg| arg.name).collect();
    }
    SUPPLIED
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, args)| args.iter().copied().collect())
        .unwrap_or_default()
}

fn unsupplied_placeholders(pack: &LocalePack, code: &str) -> BTreeSet<String> {
    let Some(row) = pack.diagnostics.get(code) else {
        return BTreeSet::new();
    };
    let mut names = required_placeholders(&row.message);
    if let Some(help) = &row.help {
        names.extend(required_placeholders(help));
    }
    let supplied = supplied_args(code);
    names
        .into_iter()
        .filter(|name| {
            !supplied.contains(name.as_str())
                && !pack.keywords.contains_key(name.as_str())
                && !pack.types.contains_key(name.as_str())
        })
        .collect()
}

#[test]
fn every_pack_placeholder_is_supplied_by_its_emit_sites() {
    let mut failures = Vec::new();
    for locale in LOCALES {
        let pack = load_pack(locale);
        for code in pack.diagnostics.keys() {
            if KNOWN_GAPS.iter().any(|(c, _)| c == code) {
                continue;
            }
            let missing = unsupplied_placeholders(&pack, code);
            if !missing.is_empty() {
                failures.push(format!("{locale}.{code}: unsupplied {missing:?}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "placeholder mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn known_gaps_are_still_gaps() {
    for locale in LOCALES {
        let pack = load_pack(locale);
        for (code, placeholder) in KNOWN_GAPS {
            if !pack.diagnostics.contains_key(*code) {
                continue;
            }
            let missing = unsupplied_placeholders(&pack, code);
            assert!(
                missing.contains(*placeholder),
                "{locale}.{code} no longer lacks `{placeholder}`: remove it from KNOWN_GAPS"
            );
        }
    }
}

#[test]
fn supplied_codes_render_in_every_pack() {
    for locale in LOCALES {
        let pack = load_pack(locale);
        let codes = SUPPLIED.iter().map(|(code, _)| *code).chain(["PARSE012"]);
        for code in codes {
            // Diagnostics stay `la`-scoped (radix-module locale_test.rs): the
            // six non-`la`, non-`en` packs have no LOCALE003 row and fall
            // back to the base message.
            if code == "LOCALE003" && !pack.diagnostics.contains_key(code) {
                continue;
            }
            let diagnostic = Diagnostic::error(code).with_code(code).with_args(
                supplied_args(code)
                    .into_iter()
                    .map(|name| DiagnosticArg::new(name, "x"))
                    .collect::<Vec<_>>(),
            );
            pack.render_diagnostic_text(&diagnostic)
                .unwrap_or_else(|err| panic!("{locale}.{code}: {err}"))
                .unwrap_or_else(|| panic!("{locale}.{code}: no pack row"));
        }
    }
}

/// An `en` file's parse diagnostics print the file locale's own keyword, not
/// the Latin literal: the emit sites pass no `functio` /
/// `genus` arg, so the pack keyword binding supplies the spelling.
#[test]
fn declaration_parse_messages_use_the_pack_keyword() {
    let cases = [
        (
            "functio () → textus {}\n",
            radix_parser::ParseErrorKind::FunctionNameAbsent,
            "functio",
        ),
        (
            "functio parse(textus s) → textus\n",
            radix_parser::ParseErrorKind::FunctionBodyAbsent,
            "functio",
        ),
        (
            "genus {}\n",
            radix_parser::ParseErrorKind::ClassNameAbsent,
            "genus",
        ),
        (
            "genus Nomen\n",
            radix_parser::ParseErrorKind::ClassBodyAbsent,
            "genus",
        ),
    ];
    let pack = load_pack("en");
    let spelling = |key: &str| {
        pack.keywords
            .get(key)
            .expect("keyword binding")
            .canonical
            .clone()
    };
    assert_ne!(
        spelling("functio"),
        "functio",
        "en must spell functio differently"
    );
    for (source, kind, keyword) in cases {
        let result = radix_parser::parse(radix_lexer::lex(source));
        let error = result
            .errors
            .iter()
            .find(|err| err.kind == kind)
            .unwrap_or_else(|| panic!("{kind:?} not reported for {source:?}"));
        assert!(
            error.args.iter().all(|arg| arg.name != keyword),
            "{kind:?} must not pass a literal `{keyword}` arg"
        );
        let code = radix_semantic::catalog_syntax::parse_spec(kind).code;
        let diagnostic = Diagnostic::error("x")
            .with_code(code)
            .with_args(error.args.clone());
        let rendered = pack
            .render_diagnostic_text(&diagnostic)
            .expect("renders")
            .expect("pack row");
        let want = format!("'{}'", spelling(keyword));
        assert!(
            rendered.message.contains(&want),
            "{code}: {}",
            rendered.message
        );
        assert!(
            !rendered.message.contains(&format!("'{keyword}'")),
            "{code}: {}",
            rendered.message
        );
    }
}
