//! FORMAT-PRETTY S2 + S6 — formatter golden corpus harness (faber-owned).
//!
//! Reads every `corpus/format/*.fab` fixture and formats it through the exact
//! CLI author pipeline ([`super::format::format_session`] + policy compile +
//! [`super::format::formatted_source_for_write`]) under two named policies:
//!
//!   - `normalise-v1` (S2 admission): the S1-pinned baseline, byte-identical
//!     to `compile_author` — output must match `*.normalise-v1.expected`.
//!   - `pretty-v1` (S6 admission): the pretty layout policy — output must
//!     match the reviewed `*.pretty-v1.expected` snapshots.
//!
//! Both policies assert idempotence — `format(format(x)) == format(x)` — so
//! the admitted expectations stay honest against the CLI surface.
//!
//! Locale fixtures (`*.en.normalise-v1.expected`, `*.la.normalise-v1.expected`)
//! pin the reader-locale re-emit surface and are cfg-gated on `hir-faber` so
//! narrow builds stay cheap (delivery §S2 coverage). pretty-v1 has no locale
//! variants: the canonical re-emit path does not honor a format policy
//! (`--policy` + `--locale` are rejected at the CLI boundary).

use super::format::{format_session, formatted_source_for_write};
use radix::forma::{compile_author_with_policy, FormatPolicy};
use std::fs;
use std::path::{Path, PathBuf};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/format")
}

/// Fixture sources in the corpus (one `*.fab` per pinned expectation).
fn fixture_sources() -> Vec<PathBuf> {
    let mut fixtures = fs::read_dir(corpus_dir())
        .expect("read corpus/format")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("fab"))
        .collect::<Vec<_>>();
    fixtures.sort();
    fixtures
}

/// Format a fixture with a named policy through the exact CLI author pipeline
/// (`format_session` + policy compile + write pipeline).
fn policy_pipeline(path: &Path, source: &str, policy: FormatPolicy) -> String {
    let session = format_session(path, None, source).expect("format session");
    let name = path.display().to_string();
    let result = compile_author_with_policy(&session, &name, source, policy)
        .expect("policy must be implemented");
    assert!(
        result.success(),
        "{policy:?} failed on {}: {:?}",
        name,
        result.diagnostics
    );
    let body = result.output.expect("author output").code;
    formatted_source_for_write(path, source, &body).expect("format write pipeline")
}

/// Format a fixture with the `normalise-v1` policy (the S1-pinned baseline).
fn normalise_v1_pipeline(path: &Path, source: &str) -> String {
    policy_pipeline(path, source, FormatPolicy::NormaliseV1)
}

/// Format a fixture with the `pretty-v1` layout policy (FORMAT-PRETTY S6).
fn pretty_v1_pipeline(path: &Path, source: &str) -> String {
    policy_pipeline(path, source, FormatPolicy::PrettyV1)
}

/// The `hir-faber`-gated reader-locale re-emit pipeline (`--locale <X>`).
#[cfg(feature = "hir-faber")]
fn locale_pipeline(path: &Path, source: &str, locale: &str) -> String {
    let session = format_session(path, Some(locale), source).expect("locale session");
    let name = path.display().to_string();
    let result = radix::forma::compile_canonical(&session, &name, source);
    assert!(
        result.success(),
        "reader-locale {locale} re-emit failed on {}: {:?}",
        name,
        result.diagnostics
    );
    let body = result.output.expect("locale output").code;
    formatted_source_for_write(path, source, &body).expect("format write pipeline")
}

/// The golden corpus is byte-exact against the `normalise-v1` baseline and
/// idempotent under it.
#[test]
fn normalise_v1_corpus_is_byte_exact_and_idempotent() {
    let fixtures = fixture_sources();
    assert!(
        fixtures.len() >= 8,
        "corpus should cover the delivery shapes, found {} fixtures",
        fixtures.len()
    );
    for fixture in fixtures {
        let stem = fixture
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("utf8 fixture stem");
        let expected_path = corpus_dir().join(format!("{stem}.normalise-v1.expected"));
        let expected = fs::read_to_string(&expected_path).unwrap_or_else(|err| {
            panic!("missing expectation for {stem}: {err}")
        });
        let source = fs::read_to_string(&fixture).expect("read fixture source");

        let formatted = normalise_v1_pipeline(&fixture, &source);
        assert_eq!(
            formatted, expected,
            "fixture {stem}: normalise-v1 output must be byte-identical to the pinned .expected"
        );

        // Idempotence: formatting the formatted output is a no-op.
        let reformatted = normalise_v1_pipeline(&fixture, &formatted);
        assert_eq!(
            formatted, reformatted,
            "fixture {stem}: format(format(x)) must equal format(x)"
        );
    }
}

/// The `pretty-v1` layout policy is pinned byte-exact on the corpus and
/// idempotent under it (FORMAT-PRETTY S6 admission).
#[test]
fn pretty_v1_corpus_is_byte_exact_and_idempotent() {
    let fixtures = fixture_sources();
    assert!(
        fixtures.len() >= 8,
        "corpus should cover the delivery shapes, found {} fixtures",
        fixtures.len()
    );
    for fixture in fixtures {
        let stem = fixture
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("utf8 fixture stem");
        let expected_path = corpus_dir().join(format!("{stem}.pretty-v1.expected"));
        let expected = fs::read_to_string(&expected_path).unwrap_or_else(|err| {
            panic!("missing pretty-v1 expectation for {stem}: {err}")
        });
        let source = fs::read_to_string(&fixture).expect("read fixture source");

        let formatted = pretty_v1_pipeline(&fixture, &source);
        assert_eq!(
            formatted, expected,
            "fixture {stem}: pretty-v1 output must be byte-identical to the admitted .expected"
        );

        // Idempotence: formatting the formatted output is a no-op.
        let reformatted = pretty_v1_pipeline(&fixture, &formatted);
        assert_eq!(
            formatted, reformatted,
            "fixture {stem}: format(format(x)) must equal format(x) under pretty-v1"
        );
    }
}

/// Reader-locale re-emit expectations (en/la) stay byte-exact. Gated on
/// `hir-faber` so narrow builds skip the canonical re-emit surface.
#[cfg(feature = "hir-faber")]
#[test]
fn normalise_v1_locale_corpus_is_byte_exact() {
    let mut locale_fixtures = 0usize;
    for fixture in fixture_sources() {
        let stem = fixture
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("utf8 fixture stem");
        for locale in ["en", "la"] {
            let expected_path = corpus_dir().join(format!("{stem}.{locale}.normalise-v1.expected"));
            if !expected_path.exists() {
                continue;
            }
            locale_fixtures += 1;
            let expected = fs::read_to_string(&expected_path).expect("read locale expectation");
            let source = fs::read_to_string(&fixture).expect("read fixture source");
            let formatted = locale_pipeline(&fixture, &source, locale);
            assert_eq!(
                formatted, expected,
                "fixture {stem} locale {locale}: reader-locale re-emit must be byte-identical"
            );
        }
    }
    assert!(
        locale_fixtures >= 2,
        "the corpus should pin en + la locale output, found {locale_fixtures}"
    );
}
