//! Goal Stage 5 gate: localized canonical Faber render preserves structural
//! trivia (unit U-5).
//!
//! These integration proofs run the full parse → collect → resolve → lower
//! frontend once and render the same analyzed unit (HIR + presentation
//! sidecar) through the radix bridge under three reader packs — `la` (Latin),
//! `llm` (pseudocode surface), and `th-TH` (non-Latin script) — and prove:
//!
//! - the same loaded unit renders identical raw comment payloads in every
//!   locale (done-when 1);
//! - statement, block-end, program-end, item, entry, member, and
//!   annotation-preceding comments land at their structural positions
//!   (done-when 2, 3);
//! - comment-bearing `ergo`/`sic … secus …` candidates stay braced while
//!   trivia-free eligible blocks keep compact output (done-when 4, 5);
//! - boundary-newline normalization never manufactures blank lines
//!   (done-when 6);
//! - explicit `tacet` prints while implicit structural ends print nothing
//!   (done-when 7);
//! - rendering is deterministic and every locale's output reparses cleanly
//!   under its own pack (done-when 8).

#![cfg(feature = "hir-faber")]

use radix_module::codegen::{Target, generate_from_analyzed};
use radix_module::driver::{AnalyzeRequest, AnalyzedModule, Config, Session, analyze_source};
use radix_module::locale::{KeywordSurface, LocalePack};

/// Installed reader pack for a locale, resolved from the repo stdlib tree.
fn pack_for(locale: &str) -> LocalePack {
    let pack_path = radix_module::locale::workspace_locale_pack_path(locale);
    LocalePack::from_toml_path(&pack_path).unwrap_or_else(|err| panic!("installed {locale} pack should load: {err}"))
}

/// Analyze Latin source once under the `la` pack. The returned unit is the
/// "same loaded FHIR": HIR + presentation are fixed, only the emit surface
/// changes between locales.
fn analyze_latin_source(source: &str) -> AnalyzedModule {
    let config = Config::default().with_locale_pack(pack_for("la"));
    let session = Session::new(config);
    match analyze_source(&session, "render-trivia.fab", source, AnalyzeRequest::default()) {
        Ok(unit) => unit,
        Err(diagnostics) => panic!("analysis failed: {diagnostics:?}"),
    }
}

/// Render an analyzed unit under one reader pack through the radix bridge.
fn render(unit: &AnalyzedModule, pack: &LocalePack) -> String {
    let surface = KeywordSurface::new(pack);
    match generate_from_analyzed(Target::HirFaber, unit, &surface) {
        Ok(radix_module::Output::Faber(output)) => output.code,
        Ok(_) => panic!("unexpected output kind from Faber target"),
        Err(err) => panic!("Faber emit failed: {err:?}"),
    }
}

/// Raw `#`-comment payloads in rendered output, in render order.
fn comment_payloads(code: &str) -> Vec<&str> {
    code.lines()
        .filter_map(|line| line.trim_start().strip_prefix('#'))
        .map(str::trim_end)
        .collect()
}

/// Source exercising every render slot with line-start comments only (legal
/// attachment points per U-1). `la` spellings; the same unit renders under
/// every locale surface.
const TRIVIA_SOURCE: &str = "\
# item comment
functio workhorse() → numerus {
    # first statement
    fixum numerus primus ← 1
    # second statement
    fixum numerus secundus ← 2

    # braced branch
    si primus ≡ 1 {
        # branch statement
        redde 5
    }
    secus {
        redde 6
    }
    # compact eligible
    si secundus ≡ 2 ergo redde 7
    # final return
    redde 9
}

genus Persona {
    # field comment
    varia textus nomen
    # method comment
    functio saluta() → vacuum {
    }
}

# before annotation
@ futura { }
functio f() → vacuum {
    # explicit tacet
    tacet
    # block end note
}

# before entry
incipit {
    # entry statement
    fixum numerus score ← 1
}
# trailing comment
";

// =============================================================================
// done-when 1: identical raw comment payloads across locales
// =============================================================================

#[test]
fn faber_render_same_unit_renders_identical_comment_payloads_across_locales() {
    let unit = analyze_latin_source(TRIVIA_SOURCE);

    let la = render(&unit, &pack_for("la"));
    let llm = render(&unit, &pack_for("en"));
    let th = render(&unit, &pack_for("th-TH"));

    // The raw payloads (everything after `#`) must be byte-identical.
    let la_payloads = comment_payloads(&la);
    let llm_payloads = comment_payloads(&llm);
    let th_payloads = comment_payloads(&th);
    assert_eq!(
        la_payloads, llm_payloads,
        "la and llm must carry identical comment payloads:\n--- la ---\n{la}\n--- llm ---\n{llm}"
    );
    assert_eq!(
        la_payloads, th_payloads,
        "la and th-TH must carry identical comment payloads:\n--- la ---\n{la}\n--- th ---\n{th}"
    );

    // Every authored payload survives, in source order.
    let expected: Vec<&str> = [
        " item comment",
        " first statement",
        " second statement",
        " braced branch",
        " branch statement",
        " compact eligible",
        " final return",
        " field comment",
        " method comment",
        " before annotation",
        " explicit tacet",
        " block end note",
        " before entry",
        " entry statement",
        " trailing comment",
    ]
    .into_iter()
    .collect();
    assert_eq!(la_payloads, expected, "comment payload order/content must be preserved:\n{la}");
}

// =============================================================================
// done-when 2 & 3: structural positions
// =============================================================================

#[test]
fn faber_render_comments_render_at_structural_positions() {
    let unit = analyze_latin_source(TRIVIA_SOURCE);
    let code = render(&unit, &pack_for("la"));

    // Item trivia precedes the item.
    assert!(
        code.contains("# item comment\nfunctio workhorse"),
        "item comment must precede the item:\n{code}"
    );
    // Statement trivia precedes each statement.
    assert!(
        code.contains("# first statement\n    fixum numerus primus ← 1"),
        "statement comment must precede its statement:\n{code}"
    );
    assert!(
        code.contains("# second statement\n    fixum numerus secundus ← 2"),
        "second statement comment must precede its statement:\n{code}"
    );
    // Block-end trivia precedes the function's closing brace.
    assert!(
        code.contains("# block end note\n}"),
        "block-end comment must precede the closing brace:\n{code}"
    );
    // Program-end trivia renders last, before EOF.
    assert!(
        code.trim_end().ends_with("# trailing comment"),
        "program-end comment must render before EOF:\n{code}"
    );
    // Member trivia precedes the field and the method.
    assert!(
        code.contains("# field comment\n    varia textus nomen"),
        "field comment must precede the field:\n{code}"
    );
    assert!(
        code.contains("# method comment\n    functio saluta"),
        "method comment must precede the method:\n{code}"
    );
    // Entry trivia precedes the incipit keyword.
    assert!(
        code.contains("# before entry\nincipit"),
        "entry comment must precede the incipit keyword:\n{code}"
    );
    // Annotation-preceding trivia renders before the annotation sequence.
    assert!(
        code.contains("# before annotation\n@ futura { }\nfunctio f"),
        "annotation-preceding comment must render before the annotation:\n{code}"
    );
}

// =============================================================================
// done-when 4 & 5: compact-joint gating
// =============================================================================

#[test]
fn faber_render_comment_bearing_branches_stay_braced_eligible_blocks_stay_compact() {
    let unit = analyze_latin_source(TRIVIA_SOURCE);
    let code = render(&unit, &pack_for("la"));

    // The comment-bearing then-branch must render braced — never `ergo`.
    assert!(
        code.contains("si primus ≡ 1 {\n        # branch statement\n        redde 5\n    }\n    secus"),
        "comment-bearing branch must stay braced:\n{code}"
    );
    // The trivia-free eligible branch keeps the compact `ergo` form.
    assert!(
        code.contains("si secundus ≡ 2 ergo redde 7"),
        "trivia-free eligible block must stay compact:\n{code}"
    );
}

// =============================================================================
// done-when 7: explicit tacet vs implicit structural ends
// =============================================================================

#[test]
fn faber_render_explicit_tacet_prints_implicit_ends_do_not() {
    let unit = analyze_latin_source(TRIVIA_SOURCE);
    let code = render(&unit, &pack_for("la"));

    assert_eq!(
        code.lines().filter(|line| line.trim() == "tacet").count(),
        1,
        "explicit source tacet must print exactly once; implicit structural ends print nothing:\n{code}"
    );
    assert!(
        code.contains("# explicit tacet\n    tacet\n    # block end note\n}"),
        "tacet renders before the block-end comment and closing brace:\n{code}"
    );
}

// =============================================================================
// done-when 6: boundary newline normalization
// =============================================================================

#[test]
fn faber_render_boundary_newlines_do_not_manufacture_blank_lines() {
    let unit = analyze_latin_source(TRIVIA_SOURCE);
    let code = render(&unit, &pack_for("la"));

    // The blank line authored between `fixum numerus secundus` and the `#
    // braced branch` comment is intentional and survives exactly once; the
    // emitter's own statement newline plus the first stored newline must NOT
    // add a second blank line.
    assert!(
        code.contains("# second statement\n    fixum numerus secundus ← 2\n\n    # braced branch"),
        "one intentional blank line must survive, no more:\n{code}"
    );
    assert!(
        code.contains("# first statement\n    fixum numerus primus ← 1"),
        "no manufactured blank line after the first statement:\n{code}"
    );
}

// =============================================================================
// done-when 8: determinism and reparse
// =============================================================================

#[test]
fn faber_render_rendering_is_deterministic_across_runs() {
    let unit = analyze_latin_source(TRIVIA_SOURCE);
    let pack = pack_for("la");
    let first = render(&unit, &pack);
    let second = render(&unit, &pack);
    assert_eq!(first, second, "same inputs must render deterministically");
}

/// Every locale's rendered output must reparse cleanly under its own pack and
/// re-emit idempotently (round-trip), proving the rendered comments claim
/// legal owners in the U-1 parser.
#[test]
fn faber_render_every_locale_output_reparses_and_round_trips() {
    let unit = analyze_latin_source(TRIVIA_SOURCE);
    let locale_codes: Vec<(String, String, LocalePack)> = ["la", "en", "th-TH"]
        .into_iter()
        .map(|locale| {
            let pack = pack_for(locale);
            let code = render(&unit, &pack);
            (locale.to_owned(), code, pack)
        })
        .collect();

    for (locale, code, pack) in &locale_codes {
        let config = Config::default().with_resolved_locale_pack(pack.clone());
        let session = Session::new(config);
        let result = radix_module::driver::reemit_faber(&session, pack, "render-trivia.re.fab", code);
        assert!(
            result.success(),
            "{locale}: rendered output must reparse cleanly: {:?}\n{code}",
            result.diagnostics
        );
        let reparsed = result
            .output
            .expect("{locale}: re-emit should produce output")
            .code;
        assert_eq!(&reparsed, code, "{locale}: re-emit of the rendered output must be idempotent");
    }
}
