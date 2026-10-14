#![cfg(feature = "hir-fhir")]

//! FHIR as the semantic core; reader locale as pure rendering.
//!
//! Proves the locale contract end to end through the FHIR artifact
//! intermediary: a Latin-authored source is analyzed once, exported to a
//! FHIR artifact (postcard bytes, the `radix emit -t fhir` path), re-imported
//! via `load_fhir`, and the SAME loaded semantic core is rendered into
//! multiple reader-locale surfaces.
//!
//! The proof has two halves:
//!
//! 1. **FHIR transparency** — every locale's render of the loaded unit is
//!    byte-identical to the direct (no-FHIR) render of the original unit, so
//!    the artifact is lossless for locale rendering.
//! 2. **Locale-as-rendering** — the loaded core renders `la`, `en`, and
//!    `th-TH` surfaces with keyword/type spellings driven by each pack, and
//!    every locale's output reparses + re-emits idempotently under its own
//!    pack. The semantic core (structure, names, values) is locale-neutral;
//!    only the surface changes.
//!
//! This closes the gap between `hir_artifact_round_trip` (FHIR round-trip,
//! Latin surface only) and `faber_render_trivia` (multi-locale render, no
//! FHIR intermediary).

use radix_module::Output;
use radix_module::codegen::{Target, generate_from_analyzed};
use radix_module::driver::{AnalyzeRequest, AnalyzedModule, Config, Session, analyze_source};
use radix_module::hir::artifact::load_fhir;
use radix_module::locale::{KeywordSurface, LocalePack};

/// Installed reader pack for a locale, resolved from the repo stdlib tree.
fn pack_for(locale: &str) -> LocalePack {
    let pack_path = radix_module::locale::workspace_locale_pack_path(locale);
    LocalePack::from_toml_path(&pack_path).unwrap_or_else(|err| panic!("installed {locale} pack should load: {err}"))
}

/// Latin-authored source exercising the packet's round-trip constructs
/// (tuple types/literals, tensor long form) plus entry trivia.
const SOURCE: &str = r#"# Entry comment.
functio pair() → iuncta<numerus, textus> {
  redde iuncta<numerus, textus> [1, "salve"]
}

functio forward(tensor<f32, [2, 2]> input) → tensor<f32, [2, 2]> {
  redde input
}

incipit {
  fixum _ salve ← "salve"
  nota salve
}
"#;

/// Analyze the Latin source once under the `la` pack.
fn analyze_latin_source() -> AnalyzedModule {
    let config = Config::default().with_locale_pack(pack_for("la"));
    let session = Session::new(config);
    match analyze_source(&session, "fhir-locale.fab", SOURCE, AnalyzeRequest::default()) {
        Ok(unit) => unit,
        Err(diagnostics) => panic!("analysis failed: {diagnostics:?}"),
    }
}

/// Export the unit to FHIR artifact bytes (the `radix emit -t fhir` path).
fn export_fhir(unit: &AnalyzedModule) -> Vec<u8> {
    let artifact = radix_module::hir::serialize::from_analyzed(unit, "test_hash", "fhir-locale.fab");
    postcard::to_allocvec(&artifact).expect("postcard serialization should succeed")
}

/// Render an analyzed unit under one reader pack through the radix bridge.
fn render(unit: &AnalyzedModule, pack: &LocalePack) -> String {
    let surface = KeywordSurface::new(pack);
    match generate_from_analyzed(Target::HirFaber, unit, &surface) {
        Ok(Output::Faber(output)) => output.code,
        Ok(_) => panic!("unexpected output kind from Faber target"),
        Err(err) => panic!("Faber emit failed: {err:?}"),
    }
}

#[test]
fn fhir_intermediary_renders_identically_to_direct_format_in_every_locale() {
    let unit = analyze_latin_source();
    let loaded = load_fhir(&export_fhir(&unit)).expect("FHIR artifact should load");

    for locale in ["la", "en", "th-TH"] {
        let pack = pack_for(locale);
        let direct = render(&unit, &pack);
        let via_fhir = render(&loaded, &pack);
        assert_eq!(
            via_fhir, direct,
            "{locale}: FHIR-intermediary render must match the direct render"
        );
    }
}

#[test]
fn fhir_core_renders_localized_surfaces_and_reparses_idempotently() {
    let unit = analyze_latin_source();
    let loaded = load_fhir(&export_fhir(&unit)).expect("FHIR artifact should load");

    let la = render(&loaded, &pack_for("la"));
    let en = render(&loaded, &pack_for("en"));
    let th_pack = pack_for("th-TH");
    let th = render(&loaded, &th_pack);
    let th_numerus = th_pack.type_surface(radix_module::locale::TypeBindingKind::Primitive(
        radix_module::semantic::Primitive::Numerus,
    ));

    // Latin surface: the semantic core keeps its authored spellings.
    assert!(
        la.contains("functio pair() → iuncta<numerus, textus>"),
        "la tuple fn must keep Latin spelling:\n{la}"
    );
    assert!(
        la.contains("redde iuncta<numerus, textus> [1, \"salve\"]"),
        "la typed tuple literal must survive:\n{la}"
    );
    assert!(
        la.contains("tensor<f32, [2, 2]>"),
        "la tensor long form must be preserved:\n{la}"
    );

    // English surface: keywords/types render through the en pack; the
    // structure (names, values, tensor long form) is unchanged.
    assert!(
        en.contains("fn pair() → tuple<int, string>"),
        "en tuple fn must localize keyword + type:\n{en}"
    );
    assert!(
        en.contains("return tuple<int, string> [1, \"salve\"]"),
        "en typed tuple literal must survive:\n{en}"
    );
    assert!(en.contains("tensor<f32, [2, 2]>"), "en tensor long form:\n{en}");
    assert!(en.contains("main {"), "en entry keyword must localize:\n{en}");
    assert!(
        en.contains("const _ salve ← \"salve\""),
        "en const binding must preserve the authored inferred-type marker:\n{en}"
    );
    assert!(en.contains("print salve"), "en nota must localize:\n{en}");
    assert!(!en.contains("functio"), "en render must not keep Latin functio:\n{en}");
    assert!(!en.contains("iuncta"), "en render must not keep Latin iuncta:\n{en}");

    // Thai surface: non-Latin script localization for keywords and types.
    assert!(
        th.contains(&format!("ฟังก์ชัน pair() → ทูเพิล<{th_numerus}, ข้อความ>")),
        "th tuple fn must localize keyword + type:\n{th}"
    );
    assert!(
        th.contains(&format!("คืน ทูเพิล<{th_numerus}, ข้อความ> [1, \"salve\"]")),
        "th typed tuple literal must survive:\n{th}"
    );
    assert!(th.contains("เริ่ม {"), "th entry keyword must localize:\n{th}");
    assert!(
        th.contains("คงที่ _ salve ← \"salve\""),
        "th const binding must preserve the authored inferred-type marker:\n{th}"
    );
    assert!(th.contains("บันทึก salve"), "th nota must localize:\n{th}");

    // Semantic-core integrity: every locale's render reparses under its own
    // pack and re-emits byte-identically (same HIR, different skins).
    for (locale, code) in [("la", &la), ("en", &en), ("th-TH", &th)] {
        let pack = pack_for(locale);
        let config = Config::default().with_resolved_locale_pack(pack.clone());
        let session = Session::new(config);
        let result = radix_module::driver::reemit_faber(&session, &pack, "fhir-locale.re.fab", code);
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

#[test]
fn fhir_double_locale_round_trip_la_thai_la_restores_latin_surface() {
    // la -> fhir: export the Latin core.
    let loaded_la = load_fhir(&export_fhir(&analyze_latin_source())).expect("la FHIR should load");
    let la_before = render(&loaded_la, &pack_for("la"));
    let th = render(&loaded_la, &pack_for("th-TH"));

    // thai -> fhir: the Thai render must re-analyze under the th-TH pack and
    // export to a fresh FHIR artifact (proves the localized surface is real,
    // parseable Faber, not a code-echo scaffold).
    let th_config = Config::default().with_locale_pack(pack_for("th-TH"));
    let th_session = Session::new(th_config);
    let th_unit = analyze_source(&th_session, "fhir-locale-th.fab", &th, AnalyzeRequest::default())
        .unwrap_or_else(|diagnostics| panic!("Thai render must re-analyze under th-TH: {diagnostics:?}"));
    let loaded_th = load_fhir(&export_fhir(&th_unit)).expect("thai FHIR should load");

    // fhir -> la: render the re-imported Thai-core FHIR back into Latin.
    let la_after = render(&loaded_th, &pack_for("la"));

    // The Latin surface is restored byte-for-byte: the Thai hop is a pure
    // rendering of the same semantic core, so la -> fhir -> th-TH -> fhir ->
    // la is a round trip, not a one-way conversion.
    assert_eq!(
        la_after, la_before,
        "la -> fhir -> th-TH -> fhir -> la must restore the Latin surface\n--- before ---\n{la_before}\n--- after ---\n{la_after}"
    );
}
