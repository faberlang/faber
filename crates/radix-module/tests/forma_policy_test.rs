//! FORMAT-PRETTY S1 + S3 — rule-slug registry, policy identity, and engine
//! routing.
//!
//! The registry is the shared source of truth for faber's future `--policy`
//! flag and the manifest `[format]` schema: unknown slugs fail clearly, and
//! each registered policy routes to its own engine — never a silent fallback.

use radix_module::forma::{compile_author_str, compile_author_with_policy_str, FormatPolicy, PolicyError};
use std::fs;
use std::path::PathBuf;

#[test]
fn default_policy_is_normalise_v1() {
    assert_eq!(FormatPolicy::default(), FormatPolicy::NormaliseV1);
}

#[test]
fn registry_resolves_registered_slugs() {
    assert_eq!(FormatPolicy::from_slug("normalise-v1"), Ok(FormatPolicy::NormaliseV1));
    assert_eq!(FormatPolicy::from_slug("pretty-v1"), Ok(FormatPolicy::PrettyV1));
}

#[test]
fn registry_slugs_round_trip() {
    for policy in FormatPolicy::ALL {
        assert_eq!(FormatPolicy::from_slug(policy.slug()), Ok(*policy), "{}", policy.slug());
    }
}

#[test]
fn registry_rejects_unknown_slugs_clearly() {
    match FormatPolicy::from_slug("nope-v9") {
        Err(PolicyError::UnknownSlug(slug)) => assert_eq!(slug, "nope-v9"),
        other => panic!("expected UnknownSlug, got {other:?}"),
    }
}

#[test]
fn unknown_slug_error_names_registered_slugs() {
    let err = FormatPolicy::from_slug("nope-v9").unwrap_err();
    let message = err.to_string();
    assert!(message.contains("unknown format policy slug `nope-v9`"), "{message}");
    assert!(message.contains("normalise-v1"), "{message}");
    assert!(message.contains("pretty-v1"), "{message}");
}

#[test]
fn both_registered_policies_are_implemented() {
    assert!(FormatPolicy::NormaliseV1.is_implemented());
    assert!(FormatPolicy::PrettyV1.is_implemented());
}

#[test]
fn pretty_v1_routes_to_the_pretty_engine() {
    // A compact one-line si body expands to one statement per line under the
    // pretty layout, proving the policy routes to the pretty engine rather
    // than the normaliser.
    let source = "functio f() → numerus {\n    si a ≻ b { redde a } redde b\n}\n";
    let result = compile_author_with_policy_str("p.fab", source, FormatPolicy::PrettyV1)
        .unwrap_or_else(|err| panic!("pretty-v1 compile: {err}"));
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let code = result.output.expect("output present").code;
    assert_eq!(
        code,
        "functio f() → numerus {\n    si a ≻ b {\n        redde a\n    }\n    redde b\n}\n"
    );
}

#[test]
fn normalise_v1_is_byte_identical_to_compile_author() {
    // Representative surface: comments, blank lines, blocks, si/sin/secus and
    // fac/cape chains, frontmatter locale, plus real corpus files (frontmatter
    // keyword resources, hello-world program) read from the radix corpus.
    let corpus_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let fixtures = [
        (
            "comments.fab",
            "# one\n# two\n\nfunctio f() → vacuum { }\n".to_owned(),
        ),
        (
            "chain.fab",
            "incipit {\n  si a { nota \"A\" } sin b { nota \"B\" } secus { nota \"F\" }\n  fac { iace \"boom\" } cape err { nota err.nuntius }\n}\n"
                .to_owned(),
        ),
        (
            "locale.fab",
            "+++\nlocale = \"en\"\n+++\nmain {\n  print \"ok\"\n}\n".to_owned(),
        ),
        (
            "incipit/salve-munde.fab",
            fs::read_to_string(corpus_dir.join("incipit/salve-munde.fab")).expect("read salve-munde.fab"),
        ),
        (
            "destructura/objectum.fab",
            fs::read_to_string(corpus_dir.join("destructura/objectum.fab")).expect("read objectum.fab"),
        ),
    ];
    for (name, source) in fixtures {
        let via_policy = compile_author_with_policy_str(name, &source, FormatPolicy::NormaliseV1)
            .unwrap_or_else(|err| panic!("{name}: policy compile: {err}"));
        assert!(
            via_policy.success(),
            "{name}: policy compile diagnostics: {:?}",
            via_policy.diagnostics
        );
        let via_author = compile_author_str(name, &source);
        assert!(
            via_author.success(),
            "{name}: author compile diagnostics: {:?}",
            via_author.diagnostics
        );
        assert_eq!(
            via_policy.output.map(|o| o.code),
            via_author.output.map(|o| o.code),
            "{name}: normalise-v1 must equal compile_author byte-for-byte"
        );
    }
}
