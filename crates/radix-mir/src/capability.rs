//! Target-capability validation vocabulary for MIR-backed emitters.
//!
//! TARGET: shared lowerability seam for all MIR text/binary targets
//! (llvm-text, wasm-text, metal-text, and future GPU targets).
//!
//! WHY: before this module, each emitter rejected unsupported MIR shapes with
//! ad-hoc `unsupported(shape: impl Into<String>)` calls deep in its print path,
//! producing opaque flat strings (`"MIR-to-LLVM unsupported: try_call"`) with no
//! category and no pre-emission decision. Stage 2 of the MIR LLVM campaign
//! introduces this target-neutral vocabulary so emitters receive a classified
//! [`Lowerability`] verdict *before* they start printing, and so later stages
//! (layout, ABI, failable CFG) plug facts into a shared classification rather
//! than re-deriving it per backend.
//!
//! BOUNDARY: this module classifies *target lowerability* — can this target
//! print this MIR shape? — and must NOT duplicate semantic typechecking. Type
//! correctness is owned by [`crate::validate::MirValidationContext`]; a
//! type-correct program can still be non-lowerable (e.g. an unresolved generic
//! `Type::Param` that survived to MIR, or a `try_call` whose CFG contract the
//! target has not implemented).
//!
//! RECIPROCITY: Wasm and other MIR targets adopt the vocabulary by importing
//! [`CapabilityGap`] and producing a [`Lowerability`] verdict from their own
//! classifier. Each target owns its classifier (which shapes it rejects and into
//! which category); the vocabulary is shared. See
//! `docs/factory/mir-llvm/goals/02-target-capability-validation.md`.

/// A classified reason a MIR-backed target cannot lower a shape.
///
/// One gap per rejection site. Categories are the five named in
/// `goals/02-target-capability-validation.md`; the owning stage column is the
/// campaign stage that supplies the missing fact or removes the rejection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityGap {
    /// A MIR shape the target does not print at all (e.g. indirect value
    /// callee). Owner: callable/representation stages, or target policy.
    UnsupportedMirShape { shape: String, slug: &'static str },
    /// The target needs a layout fact MIR does not yet carry (e.g. an
    /// unresolved `Type::Param`/`Infer`/`Error`, or a missing `MirLayoutId`).
    /// Owner: Stage 3 (layout-facts).
    MissingLayoutFact { shape: String, slug: &'static str },
    /// The target needs a function/runtime ABI fact MIR does not yet carry
    /// (e.g. `try_call` failable CFG contract). Owner: Stage 4/8
    /// (function-runtime-abi / failable-call-cfg).
    MissingAbiFact { shape: String, slug: &'static str },
    /// The target needs a runtime-boundary classification (core semantics vs
    /// host integration vs device-forbidden) before it can emit the call.
    /// Owner: Stage 1 (runtime-boundaries) ledger; this stage is complete, so a
    /// gap here means a helper family lacks a classification.
    MissingRuntimeBoundary { shape: String, slug: &'static str },
    /// The target deliberately rejects a shape by policy (e.g. device-subset
    /// forbids host-integration helpers). Owner: target policy / MIR GPU.
    TargetPolicyRejection { shape: String, slug: &'static str },
}

impl CapabilityGap {
    /// Human-readable shape text, e.g. `"try_call"`. Stable enough for corpus
    /// guards; emitters format the final diagnostic by combining this with a
    /// target prefix.
    #[must_use]
    pub fn shape(&self) -> &str {
        match self {
            Self::UnsupportedMirShape { shape, .. }
            | Self::MissingLayoutFact { shape, .. }
            | Self::MissingAbiFact { shape, .. }
            | Self::MissingRuntimeBoundary { shape, .. }
            | Self::TargetPolicyRejection { shape, .. } => shape,
        }
    }

    /// Stable category slug for machine classification and filtering
    /// (e.g. device-subset queries). Matches the variant name in kebab-case.
    #[must_use]
    pub fn slug(&self) -> &'static str {
        match self {
            Self::UnsupportedMirShape { slug, .. } => slug,
            Self::MissingLayoutFact { slug, .. } => slug,
            Self::MissingAbiFact { slug, .. } => slug,
            Self::MissingRuntimeBoundary { slug, .. } => slug,
            Self::TargetPolicyRejection { slug, .. } => slug,
        }
    }

    /// Default slug for the catch-all deep-emit fallback category.
    pub const UNSUPPORTED_MIR_SHAPE_SLUG: &'static str = "unsupported-mir-shape";
}

/// Pre-emission lowerability verdict for a MIR program against one target.
///
/// `Capable` means the target's classifier found no structural gap; deep emit
/// may still reject shapes the pre-pass does not enumerate (the deep-emit
/// `unsupported()` path remains the safety net, classified to the default
/// category). `Rejected` carries the classified gaps; the first gap drives the
/// diagnostic shape substring so corpus guards stay byte-stable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lowerability {
    Capable,
    Rejected(Vec<CapabilityGap>),
}

impl Lowerability {
    /// `true` when the target reports no structural lowerability gap.
    #[must_use]
    pub fn is_capable(&self) -> bool {
        matches!(self, Self::Capable)
    }

    /// First gap, if any. Diagnostics use this for a stable shape substring.
    #[must_use]
    pub fn first_gap(&self) -> Option<&CapabilityGap> {
        match self {
            Self::Capable => None,
            Self::Rejected(gaps) => gaps.first(),
        }
    }

    /// All classified gaps when rejected; empty when capable.
    #[must_use]
    pub fn gaps(&self) -> &[CapabilityGap] {
        match self {
            Self::Capable => &[],
            Self::Rejected(gaps) => gaps.as_slice(),
        }
    }
}
