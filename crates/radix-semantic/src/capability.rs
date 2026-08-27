//! Semantic capability query interfaces.
//!
//! This module is the seam between existing semantic authorities and later
//! capability derivation. It owns no type, access, region, function, resolved
//! use, qualified identity, or visibility table. Implementors must answer from
//! the immutable authorities belonging to one analyzed snapshot.
//!
//! [`FailClosedCapabilityQueries`] is deliberately the only implementation in
//! this unit. It makes an absent implementation explicit and safe while the
//! derived queries land in SCR-03.

use radix_hir::HirId;
use radix_types::{DefId, IndexId, TypeId};
use serde::{Deserialize, Serialize};

pub use radix_types::{
    CapabilityAnswer, CapabilityFamily, CapabilityKind, CapabilityReason, CapturePosture,
    CaptureRequirement, EffectPosture, ExportVisibility, FunctionCapabilityAnswer,
    FunctionCapabilityKind, FunctionCapabilityProof, FunctionCapabilitySummary,
    IndexCapabilityAnswer, IndexCapabilityKind, IndexCapabilityProof, IndexCapabilitySummary,
    Liveness, ParameterAccess, ParameterAccessPosture, PlaceCapabilityAnswer, PlaceCapabilityKind,
    PlaceCapabilityProof, RecursionOutcome, RegionRelationship, RequiredRegionRelationships,
    ReturnedBorrowProvenance, SolvedIndexedIdentity, TypeCapabilityAnswer, TypeCapabilityKind,
    TypeCapabilityProof, TypeCapabilitySummary, CAPABILITY_VOCABULARY_VERSION,
};

/// Immutable place/value capability summary keyed by an existing [`HirId`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlaceCapabilitySummary {
    /// Existing HIR use-site identity.
    pub hir_id: HirId,
    /// P-CAP-01.
    pub readable: PlaceCapabilityAnswer,
    /// P-CAP-02.
    pub writable: PlaceCapabilityAnswer,
    /// P-CAP-03.
    pub mutable: PlaceCapabilityAnswer,
    /// P-CAP-04.
    pub movable: PlaceCapabilityAnswer,
    /// P-CAP-05.
    pub shared_borrowable: PlaceCapabilityAnswer,
    /// P-CAP-06.
    pub mutably_borrowable: PlaceCapabilityAnswer,
    /// P-CAP-07.
    pub duplicate_then_own: PlaceCapabilityAnswer,
    /// P-CAP-08.
    pub liveness_at_use: PlaceCapabilityAnswer,
}

impl PlaceCapabilitySummary {
    /// Construct a summary that rejects every capability until an authority
    /// supplies the corresponding proof.
    #[must_use]
    pub fn missing(hir_id: HirId) -> Self {
        Self {
            hir_id,
            readable: CapabilityAnswer::missing_fact(),
            writable: CapabilityAnswer::missing_fact(),
            mutable: CapabilityAnswer::missing_fact(),
            movable: CapabilityAnswer::missing_fact(),
            shared_borrowable: CapabilityAnswer::missing_fact(),
            mutably_borrowable: CapabilityAnswer::missing_fact(),
            duplicate_then_own: CapabilityAnswer::missing_fact(),
            liveness_at_use: CapabilityAnswer::missing_fact(),
        }
    }

    /// Return the answer for one place/value capability kind.
    #[must_use]
    pub fn capability(&self, kind: PlaceCapabilityKind) -> &PlaceCapabilityAnswer {
        match kind {
            PlaceCapabilityKind::Readable => &self.readable,
            PlaceCapabilityKind::Writable => &self.writable,
            PlaceCapabilityKind::Mutable => &self.mutable,
            PlaceCapabilityKind::Movable => &self.movable,
            PlaceCapabilityKind::SharedBorrowable => &self.shared_borrowable,
            PlaceCapabilityKind::MutablyBorrowable => &self.mutably_borrowable,
            PlaceCapabilityKind::DuplicateThenOwn => &self.duplicate_then_own,
            PlaceCapabilityKind::LivenessAtUse => &self.liveness_at_use,
        }
    }
}

/// Read-only type and index capability query interface.
///
/// The returned answer must preserve [`CapabilityReason`] distinctions. In
/// particular, an unresolved query must not be represented as a negative proof.
/// The implementation is expected to consult [`radix_types::TypeTable`] and
/// existing index authority, not create another mutable store.
pub trait TypeCapabilityQueries {
    /// Query one type capability by its existing [`TypeId`].
    fn type_capability(&self, type_id: TypeId, kind: TypeCapabilityKind) -> TypeCapabilityAnswer;
}

/// Read-only solved-index capability query interface.
pub trait IndexCapabilityQueries {
    /// Query one solved-index capability by its existing [`IndexId`].
    fn index_capability(
        &self,
        index_id: IndexId,
        kind: IndexCapabilityKind,
    ) -> IndexCapabilityAnswer;
}

/// Read-only place/value capability query interface.
pub trait PlaceCapabilityQueries {
    /// Query one place capability at its existing HIR use-site identity.
    fn place_capability(&self, hir_id: HirId, kind: PlaceCapabilityKind) -> PlaceCapabilityAnswer;
}

/// Read-only function-boundary capability query interface.
pub trait FunctionCapabilityQueries {
    /// Query one function-boundary capability by its existing function or
    /// method [`DefId`]. Parameter-level proof values retain parameter IDs.
    fn function_capability(
        &self,
        def_id: DefId,
        kind: FunctionCapabilityKind,
    ) -> FunctionCapabilityAnswer;
}

/// Combined read-only capability query interface.
pub trait CapabilityQueries:
    TypeCapabilityQueries + IndexCapabilityQueries + PlaceCapabilityQueries + FunctionCapabilityQueries
{
}

impl<T> CapabilityQueries for T where
    T: TypeCapabilityQueries
        + IndexCapabilityQueries
        + PlaceCapabilityQueries
        + FunctionCapabilityQueries
{
}

/// Compatibility name for consumers that refer to the combined interface as a
/// singular query provider.
pub trait CapabilityQuery: CapabilityQueries {}

impl<T> CapabilityQuery for T where T: CapabilityQueries {}

/// Empty query provider used until SCR-03 installs derived implementations.
///
/// Every answer is [`CapabilityReason::MissingFact`]. This is intentionally a
/// value rather than an optional provider: a caller cannot accidentally treat
/// an absent implementation as an affirmative or negative semantic result.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FailClosedCapabilityQueries;

impl TypeCapabilityQueries for FailClosedCapabilityQueries {
    fn type_capability(&self, _type_id: TypeId, _kind: TypeCapabilityKind) -> TypeCapabilityAnswer {
        CapabilityAnswer::Rejected(CapabilityReason::MissingFact)
    }
}

impl IndexCapabilityQueries for FailClosedCapabilityQueries {
    fn index_capability(
        &self,
        _index_id: IndexId,
        _kind: IndexCapabilityKind,
    ) -> IndexCapabilityAnswer {
        CapabilityAnswer::Rejected(CapabilityReason::MissingFact)
    }
}

impl PlaceCapabilityQueries for FailClosedCapabilityQueries {
    fn place_capability(
        &self,
        _hir_id: HirId,
        _kind: PlaceCapabilityKind,
    ) -> PlaceCapabilityAnswer {
        CapabilityAnswer::Rejected(CapabilityReason::MissingFact)
    }
}

impl FunctionCapabilityQueries for FailClosedCapabilityQueries {
    fn function_capability(
        &self,
        _def_id: DefId,
        _kind: FunctionCapabilityKind,
    ) -> FunctionCapabilityAnswer {
        CapabilityAnswer::Rejected(CapabilityReason::MissingFact)
    }
}

/// Keep the proof and summary types reachable from the semantic capability
/// namespace without adding semantic derivation to this unit.
pub type TypeProof = TypeCapabilityProof;
pub type PlaceProof = PlaceCapabilityProof;
pub type FunctionProof = FunctionCapabilityProof;
pub type IndexProof = IndexCapabilityProof;
pub type TypeSummary = TypeCapabilitySummary;
pub type IndexSummary = IndexCapabilitySummary;
pub type FunctionSummary = FunctionCapabilitySummary;
