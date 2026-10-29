//! Four-tier conversio coverage classifier (oracle).
//!
//! The conversio (`↦`) analog of [`super::coverage`]. Where the `HIR` coverage
//! matrix asks "can target X lower grammar production Y over the corpus", this
//! matrix asks "can target X lower the `↦` operator for the family pair
//! (src, tgt)". The universe is the **type-family** cartesian product, not the
//! exempla corpus.
//!
//! Since Phase 2 the classifier is the **cross-check oracle**, not the
//! measurement: the retired harness (`corpus_matrix::exempla_e2e::conversio_target_matrix`,
//! deleted 2026-10-24 with the e2e driver) measured fixture-backed cells through
//! the real frontend, MIR probes, and HIR emit-arm detection; `faber corpus` is
//! the target verifier now. This module predicts the same tiers read-only for
//! cells without fixtures and for the oracle role.
//!
//! ## Tiers
//!
//! | Glyph | Tier | Meaning |
//! |---|---|---|
//! | ✓ | `Dedicated` | the target has a dedicated lowering arm for this pair |
//! | ◐ | `Fallback` | accepted by semantics, but the target only has the unspecialized fallback (not guaranteed to compile) |
//! | ✕ | `Rejected` | the typechecker rejects this pair before any backend runs |
//! | — | `NotEmitted` | the target cannot lower this pair at all |
//!
//! Tier precedence for one (src, tgt, target) cell:
//!
//! ```text
//! Rejected  >  NotEmitted  >  Dedicated  >  Fallback
//! ```
//!
//! `Rejected` is target-independent: it mirrors the semantic `↦` policy in
//! `semantic/passes/typecheck/convert.rs`. `NotEmitted` / `Dedicated` /
//! `Fallback` are per-target and mirror each backend's dispatch.
//!
//! ## Family-level granularity
//!
//! v1 treats each aggregate family (`tensor`, `vector`, `matrix`, `sparsa`,
//! `intervallum`) as one cell. Conditional rejections that depend on concrete
//! type instances — element mismatch, shape mismatch, literal-required sources,
//! sized-numeric widths (e.g. ``fractus<f32>``) — are NOT family-level verdicts.
//! A cell reports the unconditional verdict; the conditional cases are
//! footnoted in `CONVERSIO_MATRIX.md`.
//!
//! ## Honesty scope
//!
//! This reports lowering structure, not runtime value correctness — the same
//! scope as [`super::coverage`]. A `Dedicated` cell means a backend arm exists;
//! it does not guarantee the generated code produces correct values.

/// Four-tier conversio coverage verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConversioCoverage {
    /// ✓ dedicated lowering — a backend arm exists for this family pair.
    Dedicated,
    /// ◐ accepted but only via the unspecialized fallback — not guaranteed to compile.
    Fallback,
    /// ✕ semantically rejected — the typechecker denies this pair before codegen.
    Rejected,
    /// — not emitted by the target — the backend cannot lower this family pair.
    NotEmitted,
}

impl ConversioCoverage {
    /// Markdown glyph used in `CONVERSIO_MATRIX.md`.
    #[must_use]
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Dedicated => "✓",
            Self::Fallback => "◐",
            Self::Rejected => "✕",
            Self::NotEmitted => "—",
        }
    }

    /// Stable machine-readable tier name used in the harness TSV.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Dedicated => "dedicated",
            Self::Fallback => "fallback",
            Self::Rejected => "rejected",
            Self::NotEmitted => "not-emitted",
        }
    }
}

/// Coarse type-family universe for the conversio matrix.
///
/// One cell per family. The ML aggregate families (`Tensor`, `Vector`,
/// `Matrix`, `Sparsa`) and `Intervallum` collapse their shape/element variance
/// into a single row/column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConversioTypeFamily {
    // Scalar lane (HIR).
    Numerus,
    Fractus,
    Bivalens,
    Textus,
    Ascii,
    Octeti,
    // Dynamic / data lane (HIR).
    Regex,
    Json,
    Valor,
    Instans,
    Modulus,
    // Collection lane (HIR).
    Lista,
    Tabula,
    Copia,
    Intervallum,
    // ML aggregate lane (systems).
    Tensor,
    Vector,
    Matrix,
    Sparsa,
    // Stream / async lane (cross).
    Cursor,
    Promissum,
}

impl ConversioTypeFamily {
    /// All families in stable matrix order.
    pub const ALL: &[Self] = &[
        Self::Numerus,
        Self::Fractus,
        Self::Bivalens,
        Self::Textus,
        Self::Ascii,
        Self::Octeti,
        Self::Regex,
        Self::Json,
        Self::Valor,
        Self::Instans,
        Self::Modulus,
        Self::Lista,
        Self::Tabula,
        Self::Copia,
        Self::Intervallum,
        Self::Tensor,
        Self::Vector,
        Self::Matrix,
        Self::Sparsa,
        Self::Cursor,
        Self::Promissum,
    ];

    /// Exhaustive domain anchor for [`ALL`](Self::ALL): one arm per family,
    /// no wildcard (defect-audit B6-T3).
    ///
    /// Adding a family fails this match's compilation, and the coverage
    /// tests then fail until `ALL` carries the new family — the `ALL.len()`
    /// assertion this replaces stayed green when `ALL` lagged the enum. The
    /// tests pin each spelling to [`name`](Self::name) so this anchor and
    /// the display name cannot drift apart.
    #[must_use]
    pub fn universe_spelling(self) -> &'static str {
        match self {
            Self::Numerus => "numerus",
            Self::Fractus => "fractus",
            Self::Bivalens => "bivalens",
            Self::Textus => "textus",
            Self::Ascii => "ascii",
            Self::Octeti => "octeti",
            Self::Regex => "regex",
            Self::Json => "json",
            Self::Valor => "valor",
            Self::Instans => "instans",
            Self::Modulus => "modulus",
            Self::Lista => "lista",
            Self::Tabula => "tabula",
            Self::Copia => "copia",
            Self::Intervallum => "intervallum",
            Self::Tensor => "tensor",
            Self::Vector => "vector",
            Self::Matrix => "matrix",
            Self::Sparsa => "sparsa",
            Self::Cursor => "cursor",
            Self::Promissum => "promissum",
        }
    }

    /// Display name matching the matrix column/row labels.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Numerus => "numerus",
            Self::Fractus => "fractus",
            Self::Bivalens => "bivalens",
            Self::Textus => "textus",
            Self::Ascii => "ascii",
            Self::Octeti => "octeti",
            Self::Regex => "regex",
            Self::Json => "json",
            Self::Valor => "valor",
            Self::Instans => "instans",
            Self::Modulus => "modulus",
            Self::Lista => "lista",
            Self::Tabula => "tabula",
            Self::Copia => "copia",
            Self::Intervallum => "intervallum",
            Self::Tensor => "tensor",
            Self::Vector => "vector",
            Self::Matrix => "matrix",
            Self::Sparsa => "sparsa",
            Self::Cursor => "cursor",
            Self::Promissum => "promissum",
        }
    }

    /// Map a semantic type to its matrix family, when the type has a family.
    ///
    /// Used by the measured harness to attribute emit-arm records and MIR
    /// verdicts to cells. `None` for families the matrix does not track
    /// (`nihil`, `vacuum`, `numquam`, `ignotum`, options, refs, records,
    /// atomics — conditional/instance-level shapes, not family cells).
    #[must_use]
    pub fn from_type(ty: &crate::semantic::Type) -> Option<Self> {
        use crate::semantic::{Primitive, Type};
        match ty {
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => Some(Self::Numerus),
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => Some(Self::Fractus),
            Type::Primitive(Primitive::Bivalens) => Some(Self::Bivalens),
            Type::Primitive(Primitive::Textus) => Some(Self::Textus),
            Type::Primitive(Primitive::Ascii) => Some(Self::Ascii),
            Type::Primitive(Primitive::Octeti) => Some(Self::Octeti),
            Type::Primitive(Primitive::Regex) => Some(Self::Regex),
            Type::Primitive(Primitive::Json) => Some(Self::Json),
            Type::Primitive(Primitive::Valor) => Some(Self::Valor),
            Type::Primitive(Primitive::Instans) | Type::SizedInstans(_) => Some(Self::Instans),
            Type::ModularWord(_) => Some(Self::Modulus),
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. } => Some(Self::Lista),
            Type::Map(_, _) => Some(Self::Tabula),
            Type::Set(_) => Some(Self::Copia),
            Type::Intervallum(..) => Some(Self::Intervallum),
            Type::Tensor(_, _) => Some(Self::Tensor),
            Type::Vector(_, _) => Some(Self::Vector),
            Type::Matrix(_, _) => Some(Self::Matrix),
            Type::Sparsa(_, _) => Some(Self::Sparsa),
            Type::Cursor(_) | Type::AsyncCursor(_, _) => Some(Self::Cursor),
            Type::Promissum(_) | Type::PromissumFailable(_, _) => Some(Self::Promissum),
            _ => None,
        }
    }

    /// Resolve a `TypeId` in the analyzed unit's type table to a family.
    #[must_use]
    pub fn from_type_id(types: &crate::semantic::TypeTable, id: crate::semantic::TypeId) -> Option<Self> {
        Self::from_type(types.get(id))
    }
}

/// Targets with a conversio emit path.
///
/// Phase 1 covered the Rust `HIR` backend and the `wasm-text` `MIR` lane.
/// Phase 2 Stage 3 added the remaining MIR text targets; Stage 4 adds the
/// `ts`/`go`/`faber` HIR backends. Each variant's tier logic mirrors that
/// backend's dispatch. `metal-text` (frozen probe) and `scena` (hidden
/// legacy) are intentionally absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConversioCoverageTarget {
    /// Native Rust `HIR` backend — owns the `emit_unspecialized_conversio_target`
    /// fallback that the `Fallback` (◐) tier surfaces.
    Rust,
    /// TypeScript `HIR` backend — dedicated arms per target type plus a
    /// `x as T` type-assertion fallback; `modulus` hard-blocked by the driver.
    TypeScript,
    /// Go `HIR` backend — scalar/text/valor/regex arms plus a `.(T)` type-
    /// assertion fallback; `modulus` hard-blocked; `↦ <range>` clamp rejected.
    Go,
    /// Faber `HIR` printer — re-emits the `↦` spelling; every semantic-
    /// accepted pair is dedicated (no unspecialized fallback).
    Faber,
    /// `llvm-text` MIR probe — per-type runtime modules (`octeti`, `instans`,
    /// `tensor`, `sparsa`, `regex_rt`, `valor_aggregate`) plus text↔scalar and
    /// valor↔scalar host-ABI conversion symbols. The richest MIR lane.
    LlvmText,
    /// `wasm-text` MIR probe — lowers `↦` to a generic `Convert` runtime import
    /// gated by the v1 convert-symbol allow-list (text↔number today).
    WasmText,
    /// `wasm` binary MIR probe — wasm-text emit plus binary encode.
    Wasm,
    /// `wgsl-text` MIR probe — GPU-shader emit; no `Convert` handling.
    WgslText,
    /// `sexp-struct` MIR probe — structural S-expression lowerability.
    SexpStructural,
    /// `sexp` MIR probe — S-expression text emit.
    Sexp,
}

impl ConversioCoverageTarget {
    /// Display name matching the matrix column headers.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "ts",
            Self::Go => "go",
            Self::Faber => "faber",
            Self::LlvmText => "llvm-text",
            Self::WasmText => "wasm-text",
            Self::Wasm => "wasm",
            Self::WgslText => "wgsl-text",
            Self::SexpStructural => "sexp-struct",
            Self::Sexp => "sexp",
        }
    }

    /// All targets in stable column order (HIR lane then MIR lane).
    pub const ALL: &[Self] = &[
        Self::Rust,
        Self::TypeScript,
        Self::Go,
        Self::Faber,
        Self::LlvmText,
        Self::WasmText,
        Self::Wasm,
        Self::WgslText,
        Self::SexpStructural,
        Self::Sexp,
    ];
}

/// Predict the coverage tier for one (src, tgt, target) cell without emitting.
///
/// Pure: no I/O, no mutation, no `TypeTable` or HIR. The harness calls this once
/// per family pair per target.
#[must_use]
pub fn classify_conversio_coverage(
    src: ConversioTypeFamily,
    tgt: ConversioTypeFamily,
    target: ConversioCoverageTarget,
) -> ConversioCoverage {
    if semantic_conversio_rejected(src, tgt) {
        return ConversioCoverage::Rejected;
    }
    match target {
        ConversioCoverageTarget::Rust => {
            if rust_has_dedicated_conversio_arm(src, tgt) {
                ConversioCoverage::Dedicated
            } else {
                ConversioCoverage::Fallback
            }
        }
        ConversioCoverageTarget::TypeScript => {
            // `octeti ↦ tensor via Le|Be` reads the bytes through a `DataView`
            // on TypeScript; its inverse `tensor ↦ octeti via Le|Be` declines
            // by name (`ts_tensor_to_list_b8_unsupported`). Go declines
            // both directions.
            if driver_modular_word_blocked(src, tgt)
                || (src == ConversioTypeFamily::Tensor && tgt == ConversioTypeFamily::Octeti)
            {
                ConversioCoverage::NotEmitted
            } else if ts_has_dedicated_conversio_arm(src, tgt) {
                ConversioCoverage::Dedicated
            } else {
                ConversioCoverage::Fallback
            }
        }
        ConversioCoverageTarget::Go => {
            // No modulus block: the Go backend lowers words (measured; the
            // block now survives on TypeScript only).
            if go_family_unrepresentable(src) || go_family_unrepresentable(tgt) || octeti_tensor_declined(src, tgt) {
                // Go's `type_to_go` rejects these families outright; no
                // lowering exists, not even the `.(T)` type-assertion fallback.
                ConversioCoverage::NotEmitted
            } else if go_has_dedicated_conversio_arm(src, tgt) {
                ConversioCoverage::Dedicated
            } else {
                ConversioCoverage::Fallback
            }
        }
        ConversioCoverageTarget::Faber => {
            // The Faber backend re-emits the `↦` spelling itself; every
            // semantic-accepted pair is a dedicated arm (no fallback exists).
            ConversioCoverage::Dedicated
        }
        ConversioCoverageTarget::LlvmText => {
            if mir_llvm_text_lowers_conversio(src, tgt) {
                ConversioCoverage::Dedicated
            } else {
                ConversioCoverage::NotEmitted
            }
        }
        // The `wasm` binary lane shares the wasm-text emit path plus a
        // binary-encode step; the prediction mirrors wasm-text for both.
        ConversioCoverageTarget::WasmText | ConversioCoverageTarget::Wasm => {
            if wasm_text_lowers_conversio(src, tgt) {
                ConversioCoverage::Dedicated
            } else {
                ConversioCoverage::NotEmitted
            }
        }
        // GPU (wgsl-text) lanes have no `Convert` handling today;
        // semantic-accepted pairs are not emitted. Measurement re-grounds
        // these columns; the prediction is the conservative oracle.
        ConversioCoverageTarget::WgslText => ConversioCoverage::NotEmitted,
        // S-expression lanes admit the tensor capability cluster
        // (35a4bb122): tensor-target conversions from tensor-shaped and
        // text-shaped sources, text formatting from scalar/text sources,
        // and the identity vector cell. Everything else is not emitted.
        ConversioCoverageTarget::SexpStructural | ConversioCoverageTarget::Sexp => {
            // The structural dump carries `octeti ↦ tensor via Le|Be` and its
            // inverse; the executable `sexp` emitter has no route for either
            // ("tensor conversion contract").
            if target == ConversioCoverageTarget::Sexp && octeti_tensor_declined(src, tgt) {
                ConversioCoverage::NotEmitted
            } else if sexp_lowers_conversio(src, tgt) {
                ConversioCoverage::Dedicated
            } else {
                ConversioCoverage::NotEmitted
            }
        }
    }
}

/// `octeti ↦ tensor<E, [shape]> via Le|Be` and its inverse
/// `tensor<E, [shape]> ↦ octeti via Le|Be`: the pairs TypeScript and Go decline
/// by name rather than mis-emit through the list or text arms.
fn octeti_tensor_declined(src: ConversioTypeFamily, tgt: ConversioTypeFamily) -> bool {
    matches!(
        (src, tgt),
        (ConversioTypeFamily::Octeti, ConversioTypeFamily::Tensor)
            | (ConversioTypeFamily::Tensor, ConversioTypeFamily::Octeti)
    )
}

/// Mirrors the mir-sexp emitter's admitted conversio surface
/// (`radix-mir-sexp/src/capability.rs`): `X ↦ tensor` for tensor-shaped
/// sources, scalar/text `X ↦ textus` formatting, `vector ↦ vector`,
/// `tabula ↦ lista`, and the scalar `valor` boxing and extraction rows.
fn sexp_lowers_conversio(src: ConversioTypeFamily, tgt: ConversioTypeFamily) -> bool {
    // Every regex construct is MIR-to-sexp unsupported (`regex_engine_diverges`,
    // RX-10f): a regex source is never lowered, whatever the target.
    if src == ConversioTypeFamily::Regex {
        return false;
    }
    // `tabula ↦ lista` projects the keys or the values (`map_list.rs`); scalar
    // `↦ valor` boxes by identity and `valor ↦ scalar` checks the carrier
    // (`valor.rs`), both under their own prefix rows ahead of the text rows.
    if src == ConversioTypeFamily::Tabula && tgt == ConversioTypeFamily::Lista {
        return true;
    }
    // `tensor ↦ lista` is the flat data as a list (`tensor_list.rs`).
    if src == ConversioTypeFamily::Tensor && tgt == ConversioTypeFamily::Lista {
        return true;
    }
    let boxable = |family: ConversioTypeFamily| {
        matches!(
            family,
            ConversioTypeFamily::Numerus
                | ConversioTypeFamily::Fractus
                | ConversioTypeFamily::Bivalens
                | ConversioTypeFamily::Textus
        )
    };
    // A `lista`, `tabula` or `tensor` boxes behind a `valor` (a genus too, which
    // has no family), and a `valor` extracts to a `lista` or `tabula`
    // (`valor_carrier.rs`); `valor ↦ tensor` is the tensor row below.
    let composite_source = matches!(
        src,
        ConversioTypeFamily::Lista | ConversioTypeFamily::Tabula | ConversioTypeFamily::Tensor
    );
    if tgt == ConversioTypeFamily::Valor && (boxable(src) || src == ConversioTypeFamily::Modulus || composite_source) {
        return true;
    }
    // `valor ↦ valor` is the identity (a `{…}` object literal boxed).
    if src == ConversioTypeFamily::Valor
        && (boxable(tgt)
            || matches!(
                tgt,
                ConversioTypeFamily::Valor | ConversioTypeFamily::Lista | ConversioTypeFamily::Tabula
            ))
    {
        return true;
    }
    if tgt == ConversioTypeFamily::Tensor {
        return matches!(
            src,
            ConversioTypeFamily::Bivalens
                | ConversioTypeFamily::Textus
                | ConversioTypeFamily::Ascii
                | ConversioTypeFamily::Octeti
                | ConversioTypeFamily::Json
                | ConversioTypeFamily::Valor
                | ConversioTypeFamily::Modulus
                | ConversioTypeFamily::Lista
                | ConversioTypeFamily::Tensor
                | ConversioTypeFamily::Vector
                | ConversioTypeFamily::Matrix
                | ConversioTypeFamily::Sparsa
        );
    }
    if tgt == ConversioTypeFamily::Textus {
        return matches!(
            src,
            ConversioTypeFamily::Numerus
                | ConversioTypeFamily::Fractus
                | ConversioTypeFamily::Bivalens
                | ConversioTypeFamily::Textus
                | ConversioTypeFamily::Ascii
                | ConversioTypeFamily::Octeti
                | ConversioTypeFamily::Regex
        );
    }
    // Integer ↦ integer slot and integer/float ↦ float lower through the exact
    // Racket carriers (`is_integer_conversion` / `is_float_conversion`; words
    // count as integers).
    let is_integer =
        |family: ConversioTypeFamily| matches!(family, ConversioTypeFamily::Numerus | ConversioTypeFamily::Modulus);
    if is_integer(src) && (is_integer(tgt) || tgt == ConversioTypeFamily::Fractus) {
        return true;
    }
    if src == ConversioTypeFamily::Fractus && tgt == ConversioTypeFamily::Fractus {
        return true;
    }
    src == ConversioTypeFamily::Vector && tgt == ConversioTypeFamily::Vector
}

// ===========================================================================
// Semantic rejection (✕) — target-independent
//
// Mirrors `semantic/passes/typecheck/convert.rs`. Only unconditional family
// rejections are encoded here; conditional rejections (element/shape mismatch,
// literal-required sources, sized-numeric widths) are footnoted, not cells.
// ===========================================================================

fn is_numeric_family(family: ConversioTypeFamily) -> bool {
    matches!(
        family,
        ConversioTypeFamily::Numerus | ConversioTypeFamily::Fractus | ConversioTypeFamily::Bivalens
    )
}

fn is_scalar_extract_family(family: ConversioTypeFamily) -> bool {
    matches!(family, ConversioTypeFamily::Numerus | ConversioTypeFamily::Fractus)
}

/// `true` when the typechecker rejects the (src, tgt) family pair.
fn semantic_conversio_rejected(src: ConversioTypeFamily, tgt: ConversioTypeFamily) -> bool {
    // Ambiguous byte-reinterpretation pairs (`{numerus,fractus,bivalens} ↔ octeti`).
    if is_numeric_family(src) && tgt == ConversioTypeFamily::Octeti {
        return true;
    }
    if src == ConversioTypeFamily::Octeti && is_numeric_family(tgt) {
        return true;
    }

    // D11.6 ruling 24: `↦` into a `wrapping`/`saturating` word admits integer
    // sources (the `numerus` family, words included) only; float, text, and
    // every other family are `unsupported_conversio`.
    if tgt == ConversioTypeFamily::Modulus
        && !matches!(src, ConversioTypeFamily::Numerus | ConversioTypeFamily::Modulus)
    {
        return true;
    }

    // Async boundary: `promissum` converts only to `promissum`.
    if (src == ConversioTypeFamily::Promissum) != (tgt == ConversioTypeFamily::Promissum) {
        return true;
    }

    // Dense/sparse representation: `sparsa` converts only to/from `tensor`/`sparsa`.
    if matches!(src, ConversioTypeFamily::Sparsa)
        && !matches!(tgt, ConversioTypeFamily::Tensor | ConversioTypeFamily::Sparsa)
    {
        return true;
    }
    if matches!(tgt, ConversioTypeFamily::Sparsa)
        && !matches!(src, ConversioTypeFamily::Tensor | ConversioTypeFamily::Sparsa)
    {
        return true;
    }

    // Tensor ↔ scalar extraction holdouts.
    if src == ConversioTypeFamily::Tensor && is_scalar_extract_family(tgt) {
        return true;
    }
    if tgt == ConversioTypeFamily::Tensor && is_scalar_extract_family(src) {
        return true;
    }

    // `valor` extraction holdouts (promissum handled above).
    if src == ConversioTypeFamily::Valor && matches!(tgt, ConversioTypeFamily::Regex | ConversioTypeFamily::Cursor) {
        return true;
    }
    if tgt == ConversioTypeFamily::Valor && src == ConversioTypeFamily::Cursor {
        return true;
    }

    // `↦ regex` requires a text-shaped source.
    if tgt == ConversioTypeFamily::Regex && !matches!(src, ConversioTypeFamily::Textus | ConversioTypeFamily::Ascii) {
        return true;
    }

    // Cross-collection pairs outside the allow-list.
    if collection_conversio_rejected(src, tgt) {
        return true;
    }

    false
}

/// Semantic collection family set (`Array`/`Set`/`Map`/`Tensor`/`Cursor`/
/// `Promissum`). `Vector`, `Matrix`, `Sparsa`, and `Intervallum` are NOT
/// collection families here — they have their own lowering or rejection paths.
fn is_semantic_collection_family(family: ConversioTypeFamily) -> bool {
    matches!(
        family,
        ConversioTypeFamily::Lista
            | ConversioTypeFamily::Copia
            | ConversioTypeFamily::Tabula
            | ConversioTypeFamily::Tensor
            | ConversioTypeFamily::Cursor
            | ConversioTypeFamily::Promissum
    )
}

/// Cross-collection rejection mirroring `is_allowed_collection_conversio`.
///
/// Allowed: `lista ↔ copia`, `tensor ↔ lista`, `cursor ↔ lista`, `tabula → lista`.
/// Everything else between two distinct collection families is rejected.
fn collection_conversio_rejected(src: ConversioTypeFamily, tgt: ConversioTypeFamily) -> bool {
    if src == tgt {
        return false;
    }
    if !is_semantic_collection_family(src) || !is_semantic_collection_family(tgt) {
        return false;
    }
    let allowed = matches!(
        (src, tgt),
        (
            ConversioTypeFamily::Lista,
            ConversioTypeFamily::Copia | ConversioTypeFamily::Tensor | ConversioTypeFamily::Cursor
        ) | (
            ConversioTypeFamily::Copia
                | ConversioTypeFamily::Tensor
                | ConversioTypeFamily::Cursor
                | ConversioTypeFamily::Tabula,
            ConversioTypeFamily::Lista
        )
    );
    !allowed
}

// ===========================================================================
// Rust HIR backend — Dedicated (✓) vs Fallback (◐)
//
// Mirrors `codegen/rust/expr/convert.rs` and its `convert/` + `valor_conversio/`
// submodules. A pair is `Dedicated` when an explicit arm or a `try_emit_*`
// path owns it; otherwise the semantic-accepted pair reaches
// `emit_unspecialized_conversio_target` (the `Fallback` tier).
// ===========================================================================

/// `true` when the Rust backend has a dedicated lowering for this pair.
fn rust_has_dedicated_conversio_arm(src: ConversioTypeFamily, tgt: ConversioTypeFamily) -> bool {
    // Scalar / byte / truthiness arms (the explicit primitive match).
    if rust_dedicated_scalar_arm(src, tgt) {
        return true;
    }

    // Dedicated composite paths.
    rust_dedicated_composite_arm(src, tgt)
}

fn rust_dedicated_scalar_arm(src: ConversioTypeFamily, tgt: ConversioTypeFamily) -> bool {
    use ConversioTypeFamily::{Ascii, Bivalens, Fractus, Instans, Numerus, Octeti, Regex, Textus};
    matches!(
        (src, tgt),
        // Text parsing and truthiness.
        (Textus | Fractus | Bivalens, Numerus)
            | (Textus | Numerus | Bivalens, Fractus)
            | (Textus | Numerus | Fractus | Ascii, Bivalens)
            | (Numerus | Fractus | Ascii | Octeti | Instans, Textus)
            // D10.6: integer values pack into the ASCII carrier via their digits.
            | (Textus | Numerus | Octeti, Ascii)
            | (Textus | Ascii, Regex | Octeti)
    )
}

fn rust_dedicated_composite_arm(src: ConversioTypeFamily, tgt: ConversioTypeFamily) -> bool {
    use ConversioTypeFamily::{
        Ascii, Bivalens, Copia, Cursor, Fractus, Instans, Intervallum, Json, Lista, Matrix, Numerus, Octeti, Sparsa,
        Tabula, Tensor, Textus, Valor, Vector,
    };

    // Tensor / sparsa element bridges.
    if matches!((src, tgt), (Tensor | Sparsa, Tensor) | (Tensor, Sparsa)) {
        return true;
    }

    // `octeti ↦ tensor<E, [shape]> via Le|Be`: chunked `from_*_bytes` decode;
    // `tensor<E, [shape]> ↦ octeti via Le|Be`: `to_*_bytes` flatten.
    if matches!((src, tgt), (Octeti, Tensor) | (Tensor, Octeti)) {
        return true;
    }

    // Cross-collection bridges (`try_emit_collection_conversio`).
    if matches!(
        (src, tgt),
        (Lista, Copia | Tensor | Cursor | Vector | Matrix) | (Copia | Tensor | Cursor | Tabula, Lista)
    ) {
        return true;
    }

    // Instans precision, `valor`/`textus → instans`, and `numerus → instans`
    // (epoch units via `Instans::from_epoch_integer`, rg1 `55a44a9fc`).
    if matches!((src, tgt), (Instans | Valor | Textus | Numerus, Instans)) {
        return true;
    }

    // JSON genus / wire arms.
    if matches!((src, tgt), (Json, Valor | Textus | Json) | (Valor | Textus, Json)) {
        return true;
    }

    // Scalar `valor ↦ T` extraction (nihil is not a matrix family).
    if src == Valor && matches!(tgt, Bivalens | Numerus | Fractus | Textus | Ascii | Octeti) {
        return true;
    }

    // Aggregate `valor ↦ lista|tabula` extraction.
    if src == Valor && matches!(tgt, Lista | Tabula) {
        return true;
    }

    // `T ↦ valor` boxing (`try_emit_into_valor_conversio`).
    if tgt == Valor
        && matches!(
            src,
            Numerus | Fractus | Bivalens | Textus | Ascii | Instans | Octeti | Tensor | Lista | Tabula
        )
    {
        return true;
    }

    // `intervallum` clamp and materialization.
    if matches!(tgt, Intervallum) && matches!(src, Numerus | Fractus | Intervallum) {
        return true;
    }
    if src == Intervallum && matches!(tgt, Lista | Tensor | Intervallum) {
        return true;
    }

    false
}

// ===========================================================================
// TypeScript / Go HIR backends — Dedicated (✓) vs Fallback (◐)
//
// Both backends mirror the Rust scalar/text bridge set with per-backend
// differences: TS has collection/tensor/vector arms, Go covers scalars plus
// valor/regex; each has a type-assertion fallback for the rest. The driver
// hard-blocks `modulus` on both (modular edge-vector parity), so modular-word
// cells render `—`. These are oracle predictions; measurement owns the cells.
// ===========================================================================

/// The TypeScript backend hard-blocks `modulus` (`modulus is not supported by
/// the ts target until modular edge-vector parity is landed`). Go lowers
/// words; the driver's own `reject_modular_word_target` now covers only the
/// Metal/WGSL text lanes.
fn driver_modular_word_blocked(src: ConversioTypeFamily, tgt: ConversioTypeFamily) -> bool {
    src == ConversioTypeFamily::Modulus || tgt == ConversioTypeFamily::Modulus
}

/// `true` when the TypeScript backend has a dedicated arm for the pair.
fn ts_has_dedicated_conversio_arm(src: ConversioTypeFamily, tgt: ConversioTypeFamily) -> bool {
    use ConversioTypeFamily::Json;
    // TS mirrors the Rust scalar/byte/truthiness bridges, the tensor/sparsa
    // element bridges, cross-collection bridges, instans precision, valor
    // extraction/boxing, and the intervallum clamp. It has no json arms (json
    // targets fall to the type-assertion fallback) — and the frontend rejects
    // json-family conversions anyway, so the disagreement is semantic drift.
    rust_has_dedicated_conversio_arm(src, tgt) && tgt != Json && src != Json
}

/// `true` when the Go backend has a dedicated arm for the pair.
fn go_has_dedicated_conversio_arm(src: ConversioTypeFamily, tgt: ConversioTypeFamily) -> bool {
    use ConversioTypeFamily::{Ascii, Bivalens, Fractus, Instans, Modulus, Numerus, Octeti, Regex, Textus, Valor};
    // Go's dedicated set: scalar/text/byte/truthiness bridges, instans
    // formatting/parsing, valor boxing (`any(...)`), valor→scalar/byte
    // extraction, and regex sources. Tensor/sparsa targets have no dedicated
    // arm — they are representable (`rt.TensorTensor`/`rt.SparsaSparsa`) but lower
    // through the `.(T)` type-assertion fallback (Fallback tier), and
    // vector/matrix/intervallum families are not representable in Go at all
    // (NotEmitted; see `go_family_unrepresentable`).
    if rust_dedicated_scalar_arm(src, tgt) {
        return true;
    }
    if matches!(
        (src, tgt),
        (Instans | Valor | Textus | Numerus, Instans) | (Numerus | Fractus | Bivalens | Textus | Ascii, Valor)
    ) {
        return true;
    }
    // Word (`wrapping`/`saturating`) sources reduce to the scalar/text/byte/
    // valor/instans targets with dedicated arms; `numerus ↦ modulus` reduces.
    // Collection and ML targets take the type-assertion fallback.
    if matches!(
        (src, tgt),
        (
            Modulus,
            Numerus | Fractus | Bivalens | Textus | Ascii | Octeti | Valor | Instans | Modulus
        ) | (Numerus, Modulus)
    ) {
        return true;
    }
    // Regex targets and valor-sourced regex have dedicated Go arms.
    if tgt == Regex && matches!(src, Textus | Ascii | Valor) {
        return true;
    }
    // `valor ↦ T` scalar/byte extraction only; collection and ML targets fall
    // to the type-assertion fallback.
    matches!((src, tgt), (Valor, Numerus | Fractus | Bivalens | Textus | Ascii | Octeti))
}

/// Families the Go backend cannot represent as values at all.
///
/// Mirrors `radix-hir-go`'s `types::type_to_go`, which returns
/// `unsupported` for `vector` and `matrix`, and `codegen-error:... not
/// emitted` for `intervallum`. Any conversio involving one of these families
/// cannot be emitted — not even through the `.(T)` type-assertion fallback.
fn go_family_unrepresentable(family: ConversioTypeFamily) -> bool {
    matches!(
        family,
        ConversioTypeFamily::Vector | ConversioTypeFamily::Matrix | ConversioTypeFamily::Intervallum
    )
}

// ===========================================================================
// MIR lanes — Dedicated (✓) vs NotEmitted (—)
//
// `↦` lowers at HIR→MIR into `MirIntrinsic::Convert` (+ dedicated collection
// ops). Each MIR text target emits the intrinsic per its own runtime surface.
// There is no unspecialized fallback in any MIR lane: either the target emits
// the runtime call (Dedicated) or it cannot represent the family (NotEmitted).
// MIR columns therefore have no `Fallback` tier.
// ===========================================================================

/// Families llvm-text's per-type runtime modules can represent as values.
///
/// llvm-text owns `octeti`, `instans`, `tensor`, `sparsa`, `regex_rt`, and
/// `valor_aggregate` runtime modules plus host-ABI text↔scalar / valor↔scalar
/// symbols. Register-class and async families (`json`, `vector`, `matrix`,
/// `modulus`, `cursor`, `promissum`) have no value profile today.
fn mir_llvm_text_representable(family: ConversioTypeFamily) -> bool {
    use ConversioTypeFamily::{
        Ascii, Bivalens, Copia, Fractus, Instans, Intervallum, Lista, Numerus, Octeti, Regex, Sparsa, Tabula, Tensor,
        Textus, Valor,
    };
    matches!(
        family,
        Numerus
            | Fractus
            | Bivalens
            | Textus
            | Ascii
            | Octeti
            | Regex
            | Instans
            | Valor
            | Lista
            | Tabula
            | Copia
            | Intervallum
            | Tensor
            | Sparsa
    )
}

/// `true` when llvm-text can lower the pair to a runtime conversion call.
fn mir_llvm_text_lowers_conversio(src: ConversioTypeFamily, tgt: ConversioTypeFamily) -> bool {
    mir_llvm_text_representable(src) && mir_llvm_text_representable(tgt)
}

/// `true` when wasm-text can lower the pair to a runtime `Convert` call.
///
/// Conservative oracle: predicts the Phase-2-era v1 allow-list (text↔scalar
/// parse/render plus `bivalens → textus`). The current
/// `radix-mir-wasm` dispatch (`convert_v1_symbol`) has grown well beyond it —
/// inline core-Wasm scalar coercions (incl. `modulus`), octeti/instans/regex/
/// valor rows, lista↔copia, tabula→lista, tensor/sparsa bridges, and
/// lista↔cursor no-ops. Measurement owns these cells: the harness re-grounds
/// fixture-backed verdicts and `CONVERSIO_MATRIX.md` → Measured divergences
/// records the drift (wasm-text: 59 cells predicted `—` measured ✓). The
/// prediction stays conservative so a cell is never claimed dedicated without
/// a fixture.
fn wasm_text_lowers_conversio(src: ConversioTypeFamily, tgt: ConversioTypeFamily) -> bool {
    matches!(
        (src, tgt),
        (
            ConversioTypeFamily::Textus,
            ConversioTypeFamily::Numerus | ConversioTypeFamily::Fractus
        ) | (
            ConversioTypeFamily::Numerus | ConversioTypeFamily::Fractus | ConversioTypeFamily::Bivalens,
            ConversioTypeFamily::Textus
        )
    )
}

#[cfg(test)]
#[path = "conversio_coverage_test.rs"]
mod tests;
