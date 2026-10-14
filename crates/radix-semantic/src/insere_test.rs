//! Build-time file embeds fold to literals (DECISIONS D8.10).
//!
//! Contract:
//! - `fixum textus S = insere "p"` folds to a `textus` literal when the
//!   bytes are valid UTF-8, else SEM061 `insere_invalid_utf8`;
//! - `fixum octeti B = insere "p"` folds to an `octeti` literal of the raw
//!   bytes;
//! - any other declared type is SEM061 `insere_type_invalid`;
//! - driver rejections (missing file, absolute path, `..` escape, no source
//!   root) surface as SEM061 with the rejection's issue;
//! - a genus `generis` field folds the same way.

use crate::insere::{InsereFiles, InsereRejection, InsereSource};
use crate::test_support::{analyze_source_with_config, issue};
use crate::{PassConfig, SemanticErrorKind};
use radix_hir::{HirExpression, HirExpressionKind, HirItemKind, HirLiteral};
use radix_target::Target;

fn config_with(path: &str, source: InsereSource) -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    let mut files = InsereFiles::default();
    files.insert(path, source);
    config.insere_files = files;
    config
}

fn static_value(
    source: &str,
    config: PassConfig,
    name: &str,
) -> (HirExpression, radix_lexer::Interner) {
    let fixture = analyze_source_with_config(source, config);
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    let value = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if fixture.interner.resolve(konst.name) == name => {
                Some(konst.value.clone())
            }
            _ => None,
        })
        .expect("static item");
    (value, fixture.interner)
}

fn rejects_with(source: &str, config: PassConfig, expected: &str) {
    let fixture = analyze_source_with_config(source, config);
    assert!(
        fixture.diagnostics.iter().any(|err| {
            err.kind == SemanticErrorKind::InsereInvalid && issue(err) == Some(expected)
        }),
        "expected SEM061 {expected:?}, got: {:?}",
        fixture.diagnostics
    );
}

#[test]
fn textus_embed_folds_to_escaped_text_literal() {
    let config = config_with(
        "schema.sql",
        InsereSource::Bytes(b"CREATE \"t\";\n\\x".to_vec()),
    );
    let (value, interner) = static_value(
        "fixum textus SCHEMA = insere \"schema.sql\"\n",
        config,
        "SCHEMA",
    );
    let HirExpressionKind::Literal(HirLiteral::String(sym)) = value.kind else {
        panic!("expected textus literal, got {:?}", value.kind);
    };
    assert_eq!(interner.resolve(sym), "CREATE \\\"t\\\";\\n\\\\x");
}

#[test]
fn textus_literal_payload_writes_nul_as_unicode_escape() {
    let payload = crate::insere::textus_literal_payload("a\0b");
    assert_eq!(payload, "a\\u{0}b");
    assert_eq!(
        radix_lexer::textus_literal::decode_textus_literal(&payload),
        "a\0b"
    );
}

#[test]
fn textus_embed_rejects_invalid_utf8() {
    let config = config_with("bad.txt", InsereSource::Bytes(vec![0x66, 0xff, 0xfe]));
    rejects_with(
        "fixum textus S = insere \"bad.txt\"\n",
        config,
        "insere_invalid_utf8",
    );
}

#[test]
fn octeti_embed_folds_to_raw_bytes() {
    let config = config_with("logo.bin", InsereSource::Bytes(vec![0x00, 0xff, 0x10]));
    let (value, interner) =
        static_value("fixum octeti LOGO = insere \"logo.bin\"\n", config, "LOGO");
    let HirExpressionKind::Literal(HirLiteral::Octeti(sym)) = value.kind else {
        panic!("expected octeti literal, got {:?}", value.kind);
    };
    assert_eq!(interner.resolve(sym), "00ff10");
}

#[test]
fn octeti_embed_accepts_invalid_utf8() {
    let config = config_with("raw.bin", InsereSource::Bytes(vec![0xff, 0xfe]));
    let _ = static_value("fixum octeti RAW = insere \"raw.bin\"\n", config, "RAW");
}

#[test]
fn other_declared_type_is_rejected() {
    let config = config_with("n.txt", InsereSource::Bytes(b"3".to_vec()));
    rejects_with(
        "fixum numerus N = insere \"n.txt\"\n",
        config,
        "insere_type_invalid",
    );
}

#[test]
fn driver_rejections_surface_their_issue() {
    for rejection in [
        InsereRejection::Missing,
        InsereRejection::AbsolutePath,
        InsereRejection::EscapesPackage,
        InsereRejection::Unreadable,
    ] {
        let config = config_with("p.txt", InsereSource::Rejected(rejection));
        rejects_with(
            "fixum textus S = insere \"p.txt\"\n",
            config,
            rejection.issue(),
        );
    }
}

#[test]
fn analysis_without_driver_context_has_no_source_root() {
    rejects_with(
        "fixum textus S = insere \"p.txt\"\n",
        PassConfig::for_target(Target::HirRust),
        "insere_no_source_root",
    );
}

#[test]
fn genus_static_field_embed_folds() {
    let config = config_with("n.txt", InsereSource::Bytes(b"salve".to_vec()));
    let fixture = analyze_source_with_config(
        "genus Nuntius {\n    generis textus SALUTATIO = insere \"n.txt\"\n}\n",
        config,
    );
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    let field_init = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Struct(genus) => genus.fields.first().and_then(|f| f.init.clone()),
            _ => None,
        })
        .expect("genus field init");
    let HirExpressionKind::Literal(HirLiteral::String(sym)) = field_init.kind else {
        panic!("expected textus literal, got {:?}", field_init.kind);
    };
    assert_eq!(fixture.interner.resolve(sym), "salve");
}
