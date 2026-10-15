//! Bidirectional locale fan-out tests.
//!
//! Verifies that every supported reader-locale pack can serve as both input
//! (frontmatter `locale`) and output (session `locale_pack`) surface.
//!
//! Four phases:
//! 1. **Emit** — compile canonical Latin source to all 7 locales (output direction)
//! 2. **Round-trip** — re-compile each localized output in its own locale (stability)
//!    3a. **Latin → all** — Latin input compiled to every locale output
//!    3b. **All → Latin** — every locale input compiled to Latin output
//! 4. **Full fan-out** — every input locale × every output locale (7×7 = 49 pairs)

use crate::codegen::Target;
use crate::driver::reemit_faber;
use crate::driver::{Config, Session};
use crate::locale::LocalePack;
use crate::locale::TypeBindingKind;
use crate::semantic::{CollectionKind, Primitive, TensorFamilyKind};

/// All locale ids with installed reader packs in the workspace stdlib.
const LOCALES: &[&str] = &["la", "zh-Hans", "zh-Hant", "vi", "ar", "hi", "th-TH", "en"];

/// Canonical Latin source: a small program with enough keywords to prove
/// localization actually happened (functio, redde, fixum, nota, incipit,
/// textus type).
const CANONICAL_SOURCE: &str = "\
functio saluta(textus phr) → textus {
  redde phr
}

incipit {
  sit _ greeting ← saluta(\"Salve\")
  nota greeting
}
";

/// Load a reader pack from the workspace language-pack tree.
fn load_pack(locale: &str) -> LocalePack {
    let pack_path = crate::locale::workspace_locale_pack_path(locale);
    LocalePack::from_toml_path(&pack_path).unwrap_or_else(|err| panic!("failed to load pack for {locale}: {err}"))
}

/// Build a session that emits in `output_locale`.
fn session_for(output_locale: &str) -> Session {
    let config = Config::default()
        .with_target(Target::HirFaber)
        .with_locale_pack(load_pack(output_locale));
    Session::new(config)
}

/// Build a source session whose frontmatter selects the reader locale. The
/// output pack is passed separately to `reemit_faber` so the two decisions
/// cannot accidentally collapse into one.
fn source_session() -> Session {
    Session::new(Config::default().with_target(Target::HirFaber))
}

/// Prepend frontmatter locale to a source body.
fn with_frontmatter(locale: &str, body: &str) -> String {
    format!("+++\nlocale = \"{locale}\"\n+++\n{body}")
}

/// Emit canonical Latin source to every locale. Returns (locale, emitted_code) pairs.
///
/// The Latin input is compiled under `la` frontmatter (the la identity
/// surface), so its keywords claim under la authority while the session pack
/// drives the localized emit surface — the S2-U2 pack-authoritative shape.
fn emit_to_all_locales() -> Vec<(&'static str, String)> {
    let mut emitted = Vec::with_capacity(LOCALES.len());
    for &locale in LOCALES {
        let session = source_session();
        let emit_pack = load_pack(locale);
        let source = with_frontmatter("la", CANONICAL_SOURCE);
        let result = reemit_faber(&session, &emit_pack, "canonical.fab", &source);
        assert!(result.success(), "emit la → {locale} failed: {:?}", result.diagnostics);
        let code = result.output.expect("emit should produce output").code;
        assert!(!code.is_empty(), "emit to {locale} produced empty output");
        emitted.push((locale, code));
    }
    emitted
}

// ── Phase 1: emit canonical Latin source to every locale ──

#[test]
fn phase1_emit_canonical_to_every_locale() {
    let emitted = emit_to_all_locales();

    // Verify each locale produced different output (keywords were localized).
    let latin = &emitted[0].1;
    for (locale, code) in &emitted[1..] {
        assert!(
            code != latin,
            "emit to {locale} is identical to Latin — keywords were not localized"
        );
    }

    // Verify each output contains expected keyword from the locale's pack.
    for (locale, code) in &emitted {
        let pack = load_pack(locale);
        // The 'functio' keyword should be localized in the emitted output.
        let functio_surface = pack.contextual_keyword_surface("functio");
        assert!(
            code.contains(functio_surface),
            "emit to {locale} missing localized 'functio' keyword '{functio_surface}'",
        );
    }
}

// ── Phase 2: same-locale round-trip (stability) ──

#[test]
fn phase2_roundtrip_every_locale() {
    let emitted = emit_to_all_locales();

    for (locale, body) in &emitted {
        let source = with_frontmatter(locale, body);
        let session = source_session();
        let emit_pack = load_pack(locale);
        let result = reemit_faber(&session, &emit_pack, "roundtrip.fab", &source);
        assert!(
            result.success(),
            "round-trip {locale} → {locale} failed: {:?}",
            result.diagnostics
        );

        let re_emitted = result.output.expect("round-trip output").code;
        assert_eq!(
            body, &re_emitted,
            "round-trip {locale} → {locale} is not stable (emit changed on re-compile)"
        );
    }
}

// ── Phase 3a: Latin input → every locale output ──
// Latin types are always registered as builtins, so la → X always works. The
// Latin input is declared under `la` frontmatter (la identity surface): its
// keywords claim under la authority, and the session pack is the emit surface.

#[test]
fn phase3a_latin_input_to_every_output() {
    for &output_locale in LOCALES {
        let session = source_session();
        let emit_pack = load_pack(output_locale);
        let source = with_frontmatter("la", CANONICAL_SOURCE);
        let result = reemit_faber(&session, &emit_pack, "latin.fab", &source);
        assert!(result.success(), "la → {output_locale} failed: {:?}", result.diagnostics);
        assert!(result.output.is_some(), "la → {output_locale} produced no output");
    }
}

// ── Phase 3b: every locale input → Latin output ──
// This tests the frontmatter input direction: localized source compiled with
// a Latin output session. The frontmatter-resolved input pack provides
// type spellings for the semantic analyzer.

#[test]
fn phase3b_every_input_to_latin_output() {
    let emitted = emit_to_all_locales();

    for (input_locale, body) in &emitted {
        let source = with_frontmatter(input_locale, body);
        // Latin output session (no locale_pack → Latin emit)
        let session = Session::new(Config::default().with_target(Target::HirFaber));
        let emit_pack = crate::locale::latin_locale_pack();
        let result = reemit_faber(&session, &emit_pack, "fanout.fab", &source);
        assert!(result.success(), "{input_locale} → la failed: {:?}", result.diagnostics);
        assert!(result.output.is_some(), "{input_locale} → la produced no output");
    }
}

// ── Phase 4: full cross-locale fan-out (every input → every output) ──

#[test]
fn phase4_full_cross_locale_fanout() {
    let emitted = emit_to_all_locales();
    let mut failures = Vec::new();

    for (input_locale, body) in &emitted {
        for &output_locale in LOCALES {
            let source = with_frontmatter(input_locale, body);
            let session = source_session();
            let emit_pack = load_pack(output_locale);
            let result = reemit_faber(&session, &emit_pack, "fanout.fab", &source);

            if !result.success() {
                let errors: Vec<String> = result
                    .diagnostics
                    .iter()
                    .filter(|d| d.is_error())
                    .map(|d| format!("{:?}:{:?}", d.code, d.issue()))
                    .collect();
                failures.push(format!("{input_locale} → {output_locale}: {}", errors.join("; ")));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} locale pairs failed:\n{}",
        failures.len(),
        LOCALES.len() * LOCALES.len(),
        failures.join("\n")
    );
}

// ── Phase 2 minimum proof: tensor family + vacua under en ──
// The `en` pack owns explicit loanword rows for the tensor family and `vacua`
// (no inherits = "la" substitute). This test proves format under `en` prints
// pack-mapped heads (equal to the pack rows, not a literal list) and that the
// emitted program re-parses stably under `en`.

/// Latin source exercising tensor + vacua + a collection + a bare primitive,
/// shaped like the corpus `conversio/tensor.fab` and `sparsa/*` vacua fixtures.
/// Compiled under `la` frontmatter (the la identity surface): the Latin
/// keywords claim under la authority while the session pack drives the
/// localized emit surface.
const TENSOR_VACUA_SOURCE: &str = "\
incipit {
  sit lista<f32> xs ← vacua
  sit tensor<f32, []> empty ← vacua
  sit numerus n ← 0
  nota n
}
";

#[test]
fn en_tensor_vacua_roundtrip_uses_pack_surface() {
    let en_pack = load_pack("en");
    let session = source_session();

    let source = with_frontmatter("la", TENSOR_VACUA_SOURCE);
    let result = reemit_faber(&session, &en_pack, "tensor-vacua.fab", &source);
    assert!(result.success(), "en tensor+vacua emit failed: {:?}", result.diagnostics);
    let body = result.output.expect("emit should produce output").code;
    assert!(!body.is_empty(), "en tensor+vacua emit produced empty output");

    // Every head used by the fixture must equal the installed en pack row —
    // asserting the pack surface, never a hardcoded literal list. Sized
    // `fractus<f32>` emits as the `f32` width marker, so the bare `numerus`
    // binding carries the primitive pack-surface proof.
    for (head, expected) in [
        (
            "tensor",
            en_pack.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Tensor)),
        ),
        ("vacua", en_pack.contextual_keyword_surface("vacua")),
        ("list", en_pack.type_surface(TypeBindingKind::Collection(CollectionKind::Lista))),
        ("int", en_pack.type_surface(TypeBindingKind::Primitive(Primitive::Numerus))),
    ] {
        assert!(
            body.contains(expected),
            "emitted en output is missing pack surface {head:?} ({expected:?}):\n{body}"
        );
    }
    assert!(
        !body.contains("vacua"),
        "en emit must stay on canonical `empty` even with an empty-named binding, got:\n{body}"
    );

    // Re-parse the emitted (English-surface) program under the en session and
    // require stable re-emission (phase-2 same-HIR stability proof).
    let source = with_frontmatter("en", &body);
    let re_result = reemit_faber(&session, &en_pack, "tensor-vacua-roundtrip.fab", &source);
    assert!(
        re_result.success(),
        "en re-parse of emitted tensor+vacua failed: {:?}",
        re_result.diagnostics
    );
    let re_emitted = re_result.output.expect("round-trip output").code;
    assert_eq!(
        re_emitted, body,
        "en tensor+vacua re-emission is not stable (emit changed on re-compile)"
    );
}

/// Goal done-when proof under a non-Latin locale: Latin tensor + `vacua`
/// source → emit under `th-TH` shows Thai heads → re-parse under `th-TH`
/// yields the same HIR (stable re-emission). Mirrors the en test; the th-TH
/// pack surface is Thai script, so `contains` also proves the emitted heads
/// are non-Latin (Latin `tensor`/`vacua` would not appear). The Latin input
/// is declared under `la` frontmatter (la identity surface); the session pack
/// drives the localized emit surface.
#[test]
fn th_tensor_vacua_roundtrip_uses_pack_surface() {
    let th_pack = load_pack("th-TH");
    let session = source_session();

    // 1. Emit under th-TH: la-frontmatter Latin source compiles and prints
    // pack surfaces.
    let source = with_frontmatter("la", TENSOR_VACUA_SOURCE);
    let result = reemit_faber(&session, &th_pack, "tensor-vacua.fab", &source);
    assert!(result.success(), "th-TH tensor+vacua emit failed: {:?}", result.diagnostics);
    let body = result.output.expect("emit should produce output").code;
    assert!(!body.is_empty(), "th-TH tensor+vacua emit produced empty output");

    // 2. Non-hardcode proof extended to th-TH: every head used by the fixture
    // must equal the installed th-TH pack row — pack-driven, never a literal
    // list. Sized `fractus<f32>` emits as the `f32` width marker (D5), so the
    // bare `numerus` binding carries the primitive pack-surface proof.
    for (head, expected) in [
        (
            "tensor",
            th_pack.type_surface(TypeBindingKind::TensorFamily(TensorFamilyKind::Tensor)),
        ),
        ("vacua", th_pack.contextual_keyword_surface("vacua")),
        ("list", th_pack.type_surface(TypeBindingKind::Collection(CollectionKind::Lista))),
        ("int", th_pack.type_surface(TypeBindingKind::Primitive(Primitive::Numerus))),
    ] {
        assert!(
            body.contains(expected),
            "emitted th-TH output is missing pack surface {head:?} ({expected:?}):\n{body}"
        );
    }
    // The th-TH tensor/vacua surfaces are Thai script; Latin heads must not
    // leak into the emitted program.
    assert!(
        !body.contains("tensor"),
        "emitted th-TH output leaks the Latin tensor head:\n{body}"
    );
    assert!(
        !body.contains("vacua"),
        "emitted th-TH output leaks the Latin vacua head:\n{body}"
    );

    // 3. Re-parse the emitted (Thai-surface) program under the th-TH session
    // and require stable re-emission — the goal's "same HIR" proof: the parser
    // canonicalizes the Thai surface to Latin identity before semantics, so
    // lowering is unchanged.
    let source = with_frontmatter("th-TH", &body);
    let re_result = reemit_faber(&session, &th_pack, "tensor-vacua-roundtrip.fab", &source);
    assert!(
        re_result.success(),
        "th-TH re-parse of emitted tensor+vacua failed: {:?}",
        re_result.diagnostics
    );
    let re_emitted = re_result.output.expect("round-trip output").code;
    assert_eq!(
        re_emitted, body,
        "th-TH tensor+vacua re-emission is not stable (emit changed on re-compile)"
    );
}

/// Bounded acceptance (delivery unit 2 §2.1): the parser accepts localized
/// surface **only** for spellings mapped in the active pack under a declared
/// locale. An owned `vacua` gloss parses as the empty-collection marker; an
/// unmapped spelling keeps current behavior — it lexes as an ordinary
/// identifier and falls through to ordinary identifier semantics (undefined
/// variable), with no new implicit acceptance.
#[test]
fn th_locale_vacua_acceptance_is_bounded_to_owned_rows() {
    let session = session_for("th-TH");

    // Owned row: the th-TH `vacua` gloss (`เซตว่าง`) parses as the
    // empty-collection marker. The fixture uses the declared th-TH surface
    // for its support keywords (`เริ่ม` incipit, `คงที่` fixum, `บันทึก` nota,
    // `เทนเซอร์` tensor, `เศษ` fractus) — pack-authoritative, no Latin
    // support vocabulary under the non-la pack.
    let owned = "เริ่ม {\n  กำหนด เทนเซอร์<f32, []> empty ← เซตว่าง\n  บันทึก empty\n}\n";
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "vacua-owned.fab",
        owned,
    );
    assert!(
        result.success(),
        "owned th-TH vacua gloss should compile as the empty-collection marker: {:?}",
        result.diagnostics
    );

    // Unmapped spelling in expression position: lexes as an ordinary
    // identifier and is NOT claimed as `vacua` — the initializer is an
    // undefined variable reference, so compilation fails. If the parser
    // started accepting unmapped spellings, this would (wrongly) succeed.
    let unmapped = "เริ่ม {\n  กำหนด เทนเซอร์<f32, []> empty ← หุ่นจำลอง\n  บันทึก empty\n}\n";
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "vacua-unmapped.fab",
        unmapped,
    );
    assert!(
        !result.success(),
        "unmapped th-TH spelling must not be implicitly accepted as vacua"
    );
}

#[test]
fn en_vacua_marker_rejects_after_alias_removal() {
    let session = session_for("en");
    let source = with_frontmatter("en", "main {\n  mut list<int> xs ← vacua\n  print xs.length()\n}\n");
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "en-vacua-reject.fab",
        &source,
    );
    assert!(
        !result.success(),
        "en `vacua` must not claim as the empty-collection marker: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.code == Some("SEM001") && d.issue() == Some("unknown_identifier")),
        "en `vacua` must fail as an unknown identifier, got: {:?}",
        result.diagnostics
    );
}

fn assert_bare_nihil_value_named_error(locale: &str, body: &str, replacement: &str) {
    let session = session_for(locale);
    let source = with_frontmatter(locale, body);
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "bare-nihil-value.fab",
        &source,
    );
    assert!(!result.success(), "{locale} bare `nihil` value must be rejected");
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|d| d.is_error() && d.code == Some("PARSE030") && d.issue() == Some("retired_null_value_spelling"))
        .unwrap_or_else(|| {
            panic!(
                "{locale} bare `nihil` value must give retired_null_value_spelling, got: {:?}",
                result.diagnostics
            )
        });
    assert!(
        diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "replacement" && arg.value == replacement),
        "{locale} replacement must be `{replacement}`: {diagnostic:?}"
    );
}

#[test]
fn en_bare_nihil_value_is_named_retired_error() {
    assert_bare_nihil_value_named_error("en", "main {\n  let int union none n ← nihil\n  print n\n}\n", "null");
}

#[test]
fn la_bare_nihil_value_is_named_retired_error() {
    assert_bare_nihil_value_named_error("la", "incipit {\n  sit numerus ∪ nihil n ← nihil\n  scribe n\n}\n", "nulla");
}

#[test]
fn en_empty_marker_compiles() {
    let session = session_for("en");
    let source = with_frontmatter("en", "main {\n  mut list<int> xs ← empty\n  print xs.length()\n}\n");
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "en-empty-marker.fab",
        &source,
    );
    assert!(result.success(), "en `empty` marker must compile: {:?}", result.diagnostics);
}

#[test]
fn en_empty_named_binding_compiles_with_marker_rhs() {
    let session = session_for("en");
    let source = with_frontmatter("en", "main {\n  mut list<int> empty ← empty\n  print 0\n}\n");
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "en-empty-shadow.fab",
        &source,
    );
    assert!(
        result.success(),
        "en `mut list<int> empty ← empty` must compile (RHS is the marker): {:?}",
        result.diagnostics
    );
}

#[test]
fn la_vacua_marker_still_compiles() {
    let session = session_for("la");
    let source = with_frontmatter("la", "incipit {\n  varia lista<numerus> xs ← vacua\n  nota xs.longitudo()\n}\n");
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "la-vacua-marker.fab",
        &source,
    );
    assert!(
        result.success(),
        "la `vacua` marker must still compile: {:?}",
        result.diagnostics
    );
}
