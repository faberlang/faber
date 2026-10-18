//! Conversio coverage tier model: every (source family, target family, coverage target)
//! triple is classified Dedicated, Fallback, Rejected, or NotEmitted. Verifies glyph
//! identity, target universe completeness, representative pairs, and precedence invariants.

use super::*;

// --- tier model: glyph / name round-trips -----------------------------------

#[test]
fn tier_glyphs_are_distinct() {
    let glyphs = [
        ConversioCoverage::Dedicated.glyph(),
        ConversioCoverage::Fallback.glyph(),
        ConversioCoverage::Rejected.glyph(),
        ConversioCoverage::NotEmitted.glyph(),
    ];
    assert_eq!(glyphs, ["✓", "◐", "✕", "—"]);
}

#[test]
fn tier_names_are_distinct() {
    let names = [
        ConversioCoverage::Dedicated.name(),
        ConversioCoverage::Fallback.name(),
        ConversioCoverage::Rejected.name(),
        ConversioCoverage::NotEmitted.name(),
    ];
    assert_eq!(names, ["dedicated", "fallback", "rejected", "not-emitted"]);
}

// --- family / target universe -----------------------------------------------

#[test]
fn family_universe_is_complete_and_named() {
    // Membership, not length (defect-audit B6-T3): `universe_spelling` names
    // every family with no wildcard arm, so a new family is a compile error
    // there, and this set-equality fails until `ALL` carries it.
    let expected: Vec<&'static str> = [
        "numerus",
        "fractus",
        "bivalens",
        "textus",
        "ascii",
        "octeti",
        "regex",
        "json",
        "valor",
        "instans",
        "modulus",
        "lista",
        "tabula",
        "copia",
        "intervallum",
        "tensor",
        "vector",
        "matrix",
        "sparsa",
        "cursor",
        "promissum",
    ]
    .into();
    for family in ConversioTypeFamily::ALL {
        assert_eq!(
            family.universe_spelling(),
            family.name(),
            "{family:?} anchor spelling must match name()"
        );
        assert!(!family.name().is_empty(), "{family:?} has no name");
    }
    let all: Vec<&'static str> = ConversioTypeFamily::ALL
        .iter()
        .map(|family| family.universe_spelling())
        .collect();
    assert_eq!(
        all, expected,
        "ConversioTypeFamily::ALL must be exactly the variant domain the exhaustive anchor names"
    );
}

#[test]
fn target_universe_is_full_hir_and_mir_lanes() {
    // HIR lane: rust (Phase 1) + ts/go/faber (Phase 2 Stage 4). MIR lane:
    // llvm-text/wasm-text/wasm/wgsl-text/sexp-struct/sexp (Stage 3).
    // metal-text (frozen probe) and scena (hidden legacy) stay out.
    assert_eq!(
        ConversioCoverageTarget::ALL,
        &[
            ConversioCoverageTarget::Rust,
            ConversioCoverageTarget::TypeScript,
            ConversioCoverageTarget::Go,
            ConversioCoverageTarget::Faber,
            ConversioCoverageTarget::LlvmText,
            ConversioCoverageTarget::WasmText,
            ConversioCoverageTarget::Wasm,
            ConversioCoverageTarget::WgslText,
            ConversioCoverageTarget::SexpStructural,
            ConversioCoverageTarget::Sexp,
        ]
    );
    assert_eq!(ConversioCoverageTarget::Rust.name(), "rust");
    assert_eq!(ConversioCoverageTarget::TypeScript.name(), "ts");
    assert_eq!(ConversioCoverageTarget::Go.name(), "go");
    assert_eq!(ConversioCoverageTarget::Faber.name(), "faber");
    assert_eq!(ConversioCoverageTarget::LlvmText.name(), "llvm-text");
    assert_eq!(ConversioCoverageTarget::WasmText.name(), "wasm-text");
    assert_eq!(ConversioCoverageTarget::Wasm.name(), "wasm");
    assert_eq!(ConversioCoverageTarget::WgslText.name(), "wgsl-text");
    assert_eq!(ConversioCoverageTarget::SexpStructural.name(), "sexp-struct");
    assert_eq!(ConversioCoverageTarget::Sexp.name(), "sexp");
}

// --- acceptance criteria: representative pairs -------------------------------

#[test]
fn rust_instans_to_numerus_is_fallback_numerus_to_instans_is_dedicated() {
    // `instans ↦ numerus`: semantic accepts, no Rust arm, fallback won't compile.
    assert_eq!(
        classify_conversio_coverage(
            ConversioTypeFamily::Instans,
            ConversioTypeFamily::Numerus,
            ConversioCoverageTarget::Rust
        ),
        ConversioCoverage::Fallback
    );
    // `numerus ↦ instans` counts epoch units through `Instans::from_epoch_integer`
    // (rg1 `55a44a9fc`): a dedicated arm.
    assert_eq!(
        classify_conversio_coverage(
            ConversioTypeFamily::Numerus,
            ConversioTypeFamily::Instans,
            ConversioCoverageTarget::Rust
        ),
        ConversioCoverage::Dedicated
    );
}

#[test]
fn octeti_numerus_is_rejected_in_both_directions() {
    // Byte reinterpretation is denied by the typechecker before any backend.
    assert_eq!(
        classify_conversio_coverage(
            ConversioTypeFamily::Octeti,
            ConversioTypeFamily::Numerus,
            ConversioCoverageTarget::Rust
        ),
        ConversioCoverage::Rejected
    );
    assert_eq!(
        classify_conversio_coverage(
            ConversioTypeFamily::Numerus,
            ConversioTypeFamily::Octeti,
            ConversioCoverageTarget::Rust
        ),
        ConversioCoverage::Rejected
    );
    // Rejection is target-independent.
    assert_eq!(
        classify_conversio_coverage(
            ConversioTypeFamily::Octeti,
            ConversioTypeFamily::Numerus,
            ConversioCoverageTarget::WasmText
        ),
        ConversioCoverage::Rejected
    );
}

#[test]
fn rust_ascii_numerus_is_fallback() {
    // `ascii → numerus`: no dedicated arm; fallback parse compiles via Deref.
    assert_eq!(
        classify_conversio_coverage(
            ConversioTypeFamily::Ascii,
            ConversioTypeFamily::Numerus,
            ConversioCoverageTarget::Rust
        ),
        ConversioCoverage::Fallback
    );
}

#[test]
fn rust_tensor_to_tensor_is_dedicated() {
    // Same-element tensor conversio has a dedicated `convert_elements` arm.
    assert_eq!(
        classify_conversio_coverage(
            ConversioTypeFamily::Tensor,
            ConversioTypeFamily::Tensor,
            ConversioCoverageTarget::Rust
        ),
        ConversioCoverage::Dedicated
    );
}

#[test]
fn rust_regex_to_textus_is_fallback() {
    // Manual gap inventory: `regex → textus` via fallback `format!` (Display).
    assert_eq!(
        classify_conversio_coverage(
            ConversioTypeFamily::Regex,
            ConversioTypeFamily::Textus,
            ConversioCoverageTarget::Rust
        ),
        ConversioCoverage::Fallback
    );
}

// --- Rust dedicated (✓) representative pairs --------------------------------

#[test]
fn rust_scalar_bridges_are_dedicated() {
    use ConversioCoverageTarget::Rust;
    use ConversioTypeFamily::*;
    let dedicated = [
        (Textus, Numerus),
        (Numerus, Fractus),
        (Numerus, Ascii),
        (Octeti, Textus),
        (Ascii, Regex),
        (Instans, Textus),
    ];
    for (src, tgt) in dedicated {
        assert_eq!(
            classify_conversio_coverage(src, tgt, Rust),
            ConversioCoverage::Dedicated,
            "{src:?} ↦ {tgt:?} should be Dedicated"
        );
    }
}

#[test]
fn rust_cross_collection_bridges_are_dedicated() {
    use ConversioCoverageTarget::Rust;
    use ConversioTypeFamily::*;
    let dedicated = [
        (Lista, Copia),
        (Copia, Lista),
        (Tensor, Lista),
        (Lista, Cursor),
        (Tabula, Lista),
    ];
    for (src, tgt) in dedicated {
        assert_eq!(
            classify_conversio_coverage(src, tgt, Rust),
            ConversioCoverage::Dedicated,
            "{src:?} ↦ {tgt:?} should be Dedicated"
        );
    }
}

#[test]
fn rust_valor_extraction_and_boxing_are_dedicated() {
    use ConversioCoverageTarget::Rust;
    use ConversioTypeFamily::*;
    // `valor ↦ T` scalar extraction.
    assert_eq!(classify_conversio_coverage(Valor, Numerus, Rust), ConversioCoverage::Dedicated);
    assert_eq!(classify_conversio_coverage(Valor, Fractus, Rust), ConversioCoverage::Dedicated);
    assert_eq!(classify_conversio_coverage(Valor, Ascii, Rust), ConversioCoverage::Dedicated);
    assert_eq!(classify_conversio_coverage(Valor, Octeti, Rust), ConversioCoverage::Dedicated);
    // `valor ↦ instans` precision bridge.
    assert_eq!(classify_conversio_coverage(Valor, Instans, Rust), ConversioCoverage::Dedicated);
    // `T ↦ valor` boxing.
    assert_eq!(classify_conversio_coverage(Numerus, Valor, Rust), ConversioCoverage::Dedicated);
    assert_eq!(classify_conversio_coverage(Tensor, Valor, Rust), ConversioCoverage::Dedicated);
    assert_eq!(classify_conversio_coverage(Instans, Valor, Rust), ConversioCoverage::Dedicated);
}

// --- semantic rejection (✕) categories --------------------------------------

#[test]
fn promissum_boundary_is_rejected() {
    use ConversioTypeFamily::*;
    // Exactly one side `promissum` is rejected; both sides accepted.
    assert_eq!(
        classify_conversio_coverage(Numerus, Promissum, ConversioCoverageTarget::Rust),
        ConversioCoverage::Rejected
    );
    assert_eq!(
        classify_conversio_coverage(Promissum, Numerus, ConversioCoverageTarget::Rust),
        ConversioCoverage::Rejected
    );
    assert_ne!(
        classify_conversio_coverage(Promissum, Promissum, ConversioCoverageTarget::Rust),
        ConversioCoverage::Rejected
    );
}

#[test]
fn sparsa_only_bridges_tensor_or_sparsa() {
    use ConversioTypeFamily::*;
    assert_eq!(
        classify_conversio_coverage(Sparsa, Numerus, ConversioCoverageTarget::Rust),
        ConversioCoverage::Rejected
    );
    assert_eq!(
        classify_conversio_coverage(Lista, Sparsa, ConversioCoverageTarget::Rust),
        ConversioCoverage::Rejected
    );
    assert_ne!(
        classify_conversio_coverage(Sparsa, Tensor, ConversioCoverageTarget::Rust),
        ConversioCoverage::Rejected
    );
    assert_ne!(
        classify_conversio_coverage(Sparsa, Sparsa, ConversioCoverageTarget::Rust),
        ConversioCoverage::Rejected
    );
}

#[test]
fn tensor_scalar_extract_is_rejected() {
    use ConversioTypeFamily::*;
    assert_eq!(
        classify_conversio_coverage(Tensor, Numerus, ConversioCoverageTarget::Rust),
        ConversioCoverage::Rejected
    );
    assert_eq!(
        classify_conversio_coverage(Fractus, Tensor, ConversioCoverageTarget::Rust),
        ConversioCoverage::Rejected
    );
}

#[test]
fn regex_target_requires_text_source() {
    use ConversioTypeFamily::*;
    assert_eq!(
        classify_conversio_coverage(Textus, Regex, ConversioCoverageTarget::Rust),
        ConversioCoverage::Dedicated
    );
    assert_eq!(
        classify_conversio_coverage(Ascii, Regex, ConversioCoverageTarget::Rust),
        ConversioCoverage::Dedicated
    );
    assert_eq!(
        classify_conversio_coverage(Numerus, Regex, ConversioCoverageTarget::Rust),
        ConversioCoverage::Rejected
    );
}

#[test]
fn cross_collection_outside_allow_list_is_rejected() {
    use ConversioTypeFamily::*;
    // `lista → tabula` is explicitly unsupported; `copia → tensor` is not allowed.
    assert_eq!(
        classify_conversio_coverage(Lista, Tabula, ConversioCoverageTarget::Rust),
        ConversioCoverage::Rejected
    );
    assert_eq!(
        classify_conversio_coverage(Copia, Tensor, ConversioCoverageTarget::Rust),
        ConversioCoverage::Rejected
    );
    // Same-family collection pairs are not cross-collection rejections.
    assert_ne!(
        classify_conversio_coverage(Lista, Lista, ConversioCoverageTarget::Rust),
        ConversioCoverage::Rejected
    );
}

// --- wasm-text MIR column ----------------------------------------------------

#[test]
fn wasm_text_conversio_pairs_match_v1_allow_list() {
    // wasm-text lowers a pair only when a v1 Convert-symbol row exists
    // (runtime_value_v1_symbol): text↔number plus bivalens→textus. Every
    // other pair — even two scalar-representable families — fails closed as
    // NotEmitted (mir-gap:convert_*).
    assert_eq!(
        classify_conversio_coverage(
            ConversioTypeFamily::Numerus,
            ConversioTypeFamily::Fractus,
            ConversioCoverageTarget::WasmText
        ),
        ConversioCoverage::NotEmitted
    );
    assert_eq!(
        classify_conversio_coverage(
            ConversioTypeFamily::Textus,
            ConversioTypeFamily::Numerus,
            ConversioCoverageTarget::WasmText
        ),
        ConversioCoverage::Dedicated
    );
}

#[test]
fn wasm_text_ml_families_are_not_emitted() {
    // `scalar_ty` rejects ML aggregate and several scalar families; the column
    // is largely `—` for them — the visibility this matrix exists to surface.
    // Each pair is semantically accepted so the wasm-text tier is observable
    // (semantic `✕` would mask it via precedence).
    use ConversioCoverageTarget::WasmText;
    use ConversioTypeFamily::*;
    // (non-representable family, semantically-valid partner) -> NotEmitted.
    let cases = [
        (Tensor, Tensor),
        (Sparsa, Tensor),
        (Vector, Lista),
        (Matrix, Lista),
        (Intervallum, Lista),
        (Ascii, Numerus),
        (Instans, Numerus),
        (Valor, Numerus),
        (Modulus, Numerus),
    ];
    for (src, tgt) in cases {
        assert_eq!(
            classify_conversio_coverage(src, tgt, WasmText),
            ConversioCoverage::NotEmitted,
            "{src:?} ↦ {tgt:?} should not be emitted by wasm-text"
        );
    }
}

#[test]
fn wasm_text_has_no_fallback_tier() {
    // MIR lowers `↦` to a runtime intrinsic the runtime owns; there is no
    // unspecialized codegen fallback. Every non-rejected cell is Dedicated or
    // NotEmitted — never Fallback.
    for &src in ConversioTypeFamily::ALL {
        for &tgt in ConversioTypeFamily::ALL {
            let verdict = classify_conversio_coverage(src, tgt, ConversioCoverageTarget::WasmText);
            assert_ne!(
                verdict,
                ConversioCoverage::Fallback,
                "wasm-text must never report Fallback ({src:?} ↦ {tgt:?})"
            );
        }
    }
}

// --- wasm binary + sexp regex-source pins ------------------------------------

#[test]
fn wasm_binary_lane_mirrors_wasm_text_and_sexp_regex_sources_fail_closed() {
    // The `wasm` binary column shares the wasm-text prediction arm; the sexp
    // lanes refuse every regex source (RX-10f) before the tensor allow-list
    // runs, so no regex cell can be dedicated on either lane.
    use ConversioCoverageTarget::{Sexp, SexpStructural, Wasm, WasmText};
    use ConversioTypeFamily::{Fractus, Numerus, Regex, Tensor, Textus};
    for target in [WasmText, Wasm] {
        assert_eq!(
            classify_conversio_coverage(Textus, Numerus, target),
            ConversioCoverage::Dedicated
        );
        assert_eq!(
            classify_conversio_coverage(Numerus, Fractus, target),
            ConversioCoverage::NotEmitted
        );
    }
    for target in [SexpStructural, Sexp] {
        assert_eq!(
            classify_conversio_coverage(Regex, Tensor, target),
            ConversioCoverage::NotEmitted
        );
    }
}

// --- TypeScript / Go / Faber prediction arms ---------------------------------

#[test]
fn ts_blocks_modulus_words_and_go_lowers_them() {
    // Go lowers words (the block survives on TypeScript only): a semantic-
    // accepted word cell is emitted, scalar targets through a dedicated arm.
    assert_eq!(
        classify_conversio_coverage(
            ConversioTypeFamily::Modulus,
            ConversioTypeFamily::Textus,
            ConversioCoverageTarget::Go
        ),
        ConversioCoverage::Dedicated
    );
    assert_eq!(
        classify_conversio_coverage(
            ConversioTypeFamily::Numerus,
            ConversioTypeFamily::Modulus,
            ConversioCoverageTarget::Go
        ),
        ConversioCoverage::Dedicated
    );
    // Ruling 24: a non-integer source into a word is a semantic rejection on
    // every target.
    for &target in ConversioCoverageTarget::ALL {
        assert_eq!(
            classify_conversio_coverage(ConversioTypeFamily::Textus, ConversioTypeFamily::Modulus, target),
            ConversioCoverage::Rejected,
            "{target:?}: textus ↦ modulus is unsupported_conversio"
        );
    }
    for &target in &[ConversioCoverageTarget::TypeScript] {
        for &tgt in ConversioTypeFamily::ALL {
            // Semantic rejections (Rejected) win over the driver block
            // (NotEmitted) — `✕ > —`; the modulus block must still deny
            // every semantic-accepted modulus cell.
            let verdict = classify_conversio_coverage(ConversioTypeFamily::Modulus, tgt, target);
            assert!(
                matches!(verdict, ConversioCoverage::NotEmitted | ConversioCoverage::Rejected),
                "{target:?} must block modulus sources ({tgt:?}), got {verdict:?}"
            );
            let verdict = classify_conversio_coverage(tgt, ConversioTypeFamily::Modulus, target);
            assert!(
                matches!(verdict, ConversioCoverage::NotEmitted | ConversioCoverage::Rejected),
                "{target:?} must block modulus targets ({tgt:?}), got {verdict:?}"
            );
        }
    }
}

#[test]
fn ts_mirrors_rust_scalar_bridges_and_clamp() {
    use ConversioCoverageTarget::TypeScript;
    assert_eq!(
        classify_conversio_coverage(ConversioTypeFamily::Textus, ConversioTypeFamily::Numerus, TypeScript),
        ConversioCoverage::Dedicated
    );
    assert_eq!(
        classify_conversio_coverage(ConversioTypeFamily::Numerus, ConversioTypeFamily::Ascii, TypeScript),
        ConversioCoverage::Dedicated
    );
    // The operand clamp (`x ↦ 0‥100`) is dedicated via FaberIntervallum.coerce.
    assert_eq!(
        classify_conversio_coverage(ConversioTypeFamily::Numerus, ConversioTypeFamily::Intervallum, TypeScript),
        ConversioCoverage::Dedicated
    );
    // No json arm — the type-assertion fallback covers it (frontend rejects
    // json-family conversions anyway, so the measured cell is ✕).
    assert_eq!(
        classify_conversio_coverage(ConversioTypeFamily::Textus, ConversioTypeFamily::Json, TypeScript),
        ConversioCoverage::Fallback
    );
}

#[test]
fn go_scalar_bridges_are_dedicated_collections_fallback() {
    use ConversioCoverageTarget::Go;
    assert_eq!(
        classify_conversio_coverage(ConversioTypeFamily::Textus, ConversioTypeFamily::Numerus, Go),
        ConversioCoverage::Dedicated
    );
    assert_eq!(
        classify_conversio_coverage(ConversioTypeFamily::Numerus, ConversioTypeFamily::Ascii, Go),
        ConversioCoverage::Dedicated
    );
    assert_eq!(
        classify_conversio_coverage(ConversioTypeFamily::Valor, ConversioTypeFamily::Numerus, Go),
        ConversioCoverage::Dedicated
    );
    // Collections fall to the `.(T)` type-assertion fallback.
    assert_eq!(
        classify_conversio_coverage(ConversioTypeFamily::Lista, ConversioTypeFamily::Lista, Go),
        ConversioCoverage::Fallback
    );
    assert_eq!(
        classify_conversio_coverage(ConversioTypeFamily::Numerus, ConversioTypeFamily::Lista, Go),
        ConversioCoverage::Fallback
    );
}

#[test]
fn faber_is_dedicated_for_every_accepted_pair() {
    use ConversioCoverageTarget::Faber;
    for &src in ConversioTypeFamily::ALL {
        for &tgt in ConversioTypeFamily::ALL {
            let verdict = classify_conversio_coverage(src, tgt, Faber);
            assert!(
                matches!(verdict, ConversioCoverage::Dedicated | ConversioCoverage::Rejected),
                "faber must be Dedicated or Rejected ({src:?} ↦ {tgt:?}), got {verdict:?}"
            );
        }
    }
}

// --- precedence invariants ---------------------------------------------------

#[test]
fn rejected_is_target_independent() {
    // The byte-reinterpretation denial must hold for every target.
    for &target in ConversioCoverageTarget::ALL {
        assert_eq!(
            classify_conversio_coverage(ConversioTypeFamily::Octeti, ConversioTypeFamily::Bivalens, target),
            ConversioCoverage::Rejected,
            "{target:?} must honor the octeti/bivalens rejection"
        );
    }
}

#[test]
fn rust_never_reports_not_emitted() {
    // The Rust backend always reaches a lowering (dedicated or fallback) for a
    // semantic-accepted pair; it never refuses to emit a whole category.
    for &src in ConversioTypeFamily::ALL {
        for &tgt in ConversioTypeFamily::ALL {
            let verdict = classify_conversio_coverage(src, tgt, ConversioCoverageTarget::Rust);
            assert_ne!(
                verdict,
                ConversioCoverage::NotEmitted,
                "rust must never report NotEmitted ({src:?} ↦ {tgt:?})"
            );
        }
    }
}

// --- family mapping (measured attribution) -----------------------------------

#[test]
fn from_type_maps_semantic_types_to_families() {
    use crate::semantic::{Primitive, Type, TypeTable};
    let mut types = TypeTable::default();
    let numerus = types.primitive(Primitive::Numerus);
    let fractus = types.primitive(Primitive::Fractus);
    let textus = types.primitive(Primitive::Textus);
    let valor = types.primitive(Primitive::Valor);
    let lista = types.array(numerus);
    let shape3 = types.intern_index(crate::semantic::IndexExpr::Literal(3));
    let tensor = types.intern(Type::Tensor(numerus, shape3));
    let promissum = types.promissum(numerus);
    let nihil = types.primitive(Primitive::Nihil);

    let case = |ty, expected: Option<ConversioTypeFamily>| {
        assert_eq!(ConversioTypeFamily::from_type_id(&types, ty), expected, "type id {ty:?}");
    };

    case(numerus, Some(ConversioTypeFamily::Numerus));
    case(fractus, Some(ConversioTypeFamily::Fractus));
    case(textus, Some(ConversioTypeFamily::Textus));
    case(valor, Some(ConversioTypeFamily::Valor));
    case(lista, Some(ConversioTypeFamily::Lista));
    case(tensor, Some(ConversioTypeFamily::Tensor));
    case(promissum, Some(ConversioTypeFamily::Promissum));
    case(nihil, None);
}

#[test]
fn from_type_maps_collection_and_ml_families() {
    use crate::semantic::{Primitive, Type, TypeTable};
    let mut types = TypeTable::default();
    let numerus = types.primitive(Primitive::Numerus);
    let ascii = types.primitive(Primitive::Ascii);
    let octeti = types.primitive(Primitive::Octeti);
    let regex = types.primitive(Primitive::Regex);
    let json = types.primitive(Primitive::Json);
    let instans = types.primitive(Primitive::Instans);
    let bivalens = types.primitive(Primitive::Bivalens);

    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, ascii),
        Some(ConversioTypeFamily::Ascii)
    );
    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, octeti),
        Some(ConversioTypeFamily::Octeti)
    );
    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, regex),
        Some(ConversioTypeFamily::Regex)
    );
    assert_eq!(ConversioTypeFamily::from_type_id(&types, json), Some(ConversioTypeFamily::Json));
    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, instans),
        Some(ConversioTypeFamily::Instans)
    );
    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, bivalens),
        Some(ConversioTypeFamily::Bivalens)
    );

    // Sized numeric, modular words, and the remaining aggregates map to their
    // families.
    let sized = types.sized_numeric(Primitive::Numerus, crate::semantic::NumericWidth::U32);
    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, sized),
        Some(ConversioTypeFamily::Numerus)
    );
    let modular = types.intern(Type::ModularWord(crate::semantic::NumericWidth::U32));
    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, modular),
        Some(ConversioTypeFamily::Modulus)
    );
    let shape4 = types.intern_index(crate::semantic::IndexExpr::Literal(4));
    let vector = types.intern(Type::Vector(numerus, shape4));
    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, vector),
        Some(ConversioTypeFamily::Vector)
    );
    let shape2 = types.intern_index(crate::semantic::IndexExpr::Literal(2));
    let matrix = types.intern(Type::Matrix(numerus, shape2));
    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, matrix),
        Some(ConversioTypeFamily::Matrix)
    );
    let sparsa = types.intern(Type::Sparsa(numerus, shape2));
    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, sparsa),
        Some(ConversioTypeFamily::Sparsa)
    );
    let copia = types.set(numerus);
    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, copia),
        Some(ConversioTypeFamily::Copia)
    );
    let tabula = types.map(numerus, numerus);
    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, tabula),
        Some(ConversioTypeFamily::Tabula)
    );
    let cursor = types.cursor(numerus);
    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, cursor),
        Some(ConversioTypeFamily::Cursor)
    );
    let intervallum = types.intervallum(numerus);
    assert_eq!(
        ConversioTypeFamily::from_type_id(&types, intervallum),
        Some(ConversioTypeFamily::Intervallum)
    );
}

#[test]
fn tensor_octeti_is_dedicated_on_rust_and_declined_by_name_on_go_sexp_and_ts_encode() {
    use ConversioCoverageTarget::{Go, Rust, Sexp, TypeScript};
    use ConversioTypeFamily::{Octeti, Tensor};
    for (src, tgt) in [(Tensor, Octeti), (Octeti, Tensor)] {
        assert_eq!(classify_conversio_coverage(src, tgt, Rust), ConversioCoverage::Dedicated);
        for target in [Go, Sexp] {
            assert_eq!(classify_conversio_coverage(src, tgt, target), ConversioCoverage::NotEmitted);
        }
    }
    // TypeScript reads bytes into a tensor and declines only the encode.
    assert_eq!(
        classify_conversio_coverage(Tensor, Octeti, TypeScript),
        ConversioCoverage::NotEmitted
    );
    assert_ne!(
        classify_conversio_coverage(Octeti, Tensor, TypeScript),
        ConversioCoverage::NotEmitted
    );
}
