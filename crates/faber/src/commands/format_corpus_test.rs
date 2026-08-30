//! Format golden corpus harness (faber-owned).
//!
//! Reads every `product-corpus/format/*.fab` fixture and formats it through
//! the exact CLI path FR-2 will install: [`super::format::format_session`] +
//! [`radix::module::codegen::faber::reemit_faber`] +
//! [`super::format::formatted_source_for_write`]. Output must match
//! `*.format.expected`. Each fixture is idempotent, byte-stable, and
//! re-parses under its own locale pack.
//!
//! Gated on `hir-faber` because the canonical HIR emitter is.

#![cfg(feature = "hir-faber")]

use super::format::{format_session, formatted_source_for_write};
use radix::module::codegen::faber::reemit_faber;
use radix::module::codegen::faber::test_gate::{
    assert_canonical_idempotent_with_session, lex_parse_with_pack,
};
use std::fs;
use std::path::{Path, PathBuf};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../product-corpus/format")
}

/// Fixture sources in the corpus (one `*.fab` per pinned expectation).
fn fixture_sources() -> Vec<PathBuf> {
    let mut fixtures = fs::read_dir(corpus_dir())
        .expect("read product-corpus/format")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("fab"))
        .collect::<Vec<_>>();
    fixtures.sort();
    fixtures
}

/// Format a fixture through the exact CLI canonical pipeline.
fn canonical_pipeline(
    path: &Path,
    source: &str,
) -> Result<(radix::module::driver::Session, String, String), String> {
    let session = format_session(path, None, source)
        .map_err(|err| format!("format session: {}", err.message))?;
    let name = path.display().to_string();
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("format emission locale pack"),
        &name,
        source,
    );
    if !result.success() {
        return Err(format!(
            "canonical format failed on {name}: {:?}",
            result.diagnostics
        ));
    }
    let body = result.output.expect("canonical output").code;
    let written = formatted_source_for_write(path, source, &body)
        .map_err(|err| format!("format write pipeline: {err}"))?;
    Ok((session, body, written))
}

fn fixture_stem(path: &Path) -> &str {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .expect("utf8 fixture stem")
}

fn is_en_source(source: &str) -> bool {
    let Some(after_open) = source.strip_prefix("+++\n") else {
        return false;
    };
    let front = after_open.split("+++").next().unwrap_or("");
    front.lines().any(|line| line.trim() == "locale = \"en\"")
}

/// The golden corpus is byte-exact against the canonical HIR pin, idempotent
/// under it, and re-parses under each fixture's own locale pack.
#[test]
fn format_corpus_is_byte_exact_idempotent_and_reparses() {
    let fixtures = fixture_sources();
    assert!(
        fixtures.len() >= 12,
        "corpus should cover the la shapes plus ≥3 en fixtures, found {}",
        fixtures.len()
    );

    let mut en_fixtures = 0usize;
    let mut failures = Vec::new();
    for fixture in &fixtures {
        let stem = fixture_stem(fixture);
        let expected_path = corpus_dir().join(format!("{stem}.format.expected"));
        let source = fs::read_to_string(fixture).expect("read fixture source");
        if is_en_source(&source) {
            en_fixtures += 1;
        }

        let (session, body, formatted) = match canonical_pipeline(fixture, &source) {
            Ok(result) => result,
            Err(detail) => {
                failures.push(format!("{stem}: {detail}"));
                continue;
            }
        };
        let expected = match fs::read_to_string(&expected_path) {
            Ok(expected) => expected,
            Err(err) => {
                failures.push(format!("{stem}: missing expectation: {err}"));
                continue;
            }
        };
        if formatted != expected {
            failures.push(format!(
                "{stem}: canonical output must be byte-identical to the pinned .format.expected\n===== expected\n{expected}===== actual\n{formatted}"
            ));
        }

        match canonical_pipeline(fixture, &formatted) {
            Ok((_, _, reformatted)) if reformatted == formatted => {}
            Ok((_, _, _)) => {
                failures.push(format!("{stem}: format(format(x)) must equal format(x)"));
            }
            Err(detail) => failures.push(format!("{stem} second pass: {detail}")),
        }

        let pack = session.config.locale_pack.as_ref();
        if let Err(detail) = lex_parse_with_pack(&body, pack) {
            failures.push(format!("{stem} re-parse under own locale: {detail}"));
        }

        let name = fixture.display().to_string();
        if let Err(detail) = assert_canonical_idempotent_with_session(&session, &name, &source) {
            failures.push(format!("{stem} canonical test_gate idempotent: {detail}"));
        }

        if is_en_source(&source)
            && (body.contains("numerus")
                || body.contains("fractus")
                || body.contains("textus")
                || body.contains("octeti"))
        {
            failures.push(format!(
                "{stem}: en format must not relatinize types:\n{body}"
            ));
        }
        if stem == "en-types"
            && !(body.contains("int")
                && body.contains("float")
                && body.contains("string")
                && body.contains("bytes"))
        {
            failures.push(format!(
                "{stem}: primitive en type spellings must stay English:\n{body}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "format corpus failures:\n{}",
        failures.join("\n")
    );
    assert!(
        en_fixtures >= 3,
        "corpus must pin ≥3 en-source fixtures, found {en_fixtures}"
    );
}

/// D-A: no leftover policy pin filenames under the format corpus.
#[test]
fn format_corpus_has_no_policy_pin_filenames() {
    let leftover: Vec<_> = fs::read_dir(corpus_dir())
        .expect("read product-corpus/format")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("normalise-v1") || name.contains("pretty-v1"))
        .collect();
    assert!(
        leftover.is_empty(),
        "product-corpus/format must not keep policy pin filenames: {leftover:?}"
    );
}
