//! Target-neutral capability vocabulary and proof-bearing answers.
//!
//! This module names semantic capabilities without naming a target carrier,
//! trait, lifetime, or source annotation. It contains values and immutable
//! summaries only. Derivation belongs to the semantic authorities that already
//! own type, access, flow, identity, and visibility facts.
//!
//! A capability answer is deliberately not a boolean. A missing or unresolved
//! fact must remain distinguishable from a proof that a capability is absent;
//! later consumers can then reject without guessing.

use crate::{DefId, IndexId, TypeId};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Version of the serialized target-neutral capability vocabulary.
///
/// Increment this when the meaning or wire shape of a capability value changes.
/// Adding a new capability kind is also a vocabulary change because consumers
/// must handle it explicitly.
pub const CAPABILITY_VOCABULARY_VERSION: u16 = 1;

/// Broad subject family for a capability query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityFamily {
    /// A property of a semantic type.
    Type,
    /// A solved property of an index expression.
    Index,
    /// A property of a value or place at a HIR use site.
    Place,
    /// A property of a function boundary.
    Function,
}

/// Type-level capability kinds (T-CAP-01 through T-CAP-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypeCapabilityKind {
    /// The value can be transferred to an owning destination.
    Transferable,
    /// The value can be explicitly duplicated before being owned.
    ExplicitlyDuplicable,
    /// The value has trivial duplication semantics.
    TriviallyDuplicable,
    /// The type admits strict same-type equality.
    StrictEquality,
    /// Strict equality is reflexive for every value of the type.
    ReflexiveEquality,
    /// The type has a canonical hash law consistent with strict equality.
    CanonicalHashEligibility,
    /// Values of the type admit the language's ordering law.
    Ordering,
    /// All observable indexed identity is solved and preserved.
    SolvedIndexedIdentity,
}

impl TypeCapabilityKind {
    /// All type capability rows in their stable ledger order.
    pub const ALL: [Self; 8] = [
        Self::Transferable,
        Self::ExplicitlyDuplicable,
        Self::TriviallyDuplicable,
        Self::StrictEquality,
        Self::ReflexiveEquality,
        Self::CanonicalHashEligibility,
        Self::Ordering,
        Self::SolvedIndexedIdentity,
    ];
}

/// Index-level capability kinds.
///
/// T-CAP-08 is the type-facing capability. This companion kind gives later
/// semantic queries a place to ask whether one existing [`IndexId`] is solved,
/// without allocating a second index identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IndexCapabilityKind {
    /// The index expression has a settled, observable identity.
    SolvedIdentity,
}

impl IndexCapabilityKind {
    /// All index capability rows in their stable order.
    pub const ALL: [Self; 1] = [Self::SolvedIdentity];
}

/// Place/value capability kinds (P-CAP-01 through P-CAP-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlaceCapabilityKind {
    /// The value may be read at the use site.
    Readable,
    /// The place may be written at the use site.
    Writable,
    /// The place admits mutable access at the use site.
    Mutable,
    /// The value may be moved at the use site.
    Movable,
    /// The value may be shared-borrowed at the use site.
    SharedBorrowable,
    /// The value may be mutably borrowed at the use site.
    MutablyBorrowable,
    /// The value may be duplicated and then owned at the use site.
    DuplicateThenOwn,
    /// The source liveness at the use site is settled.
    LivenessAtUse,
}

impl PlaceCapabilityKind {
    /// All place/value capability rows in their stable ledger order.
    pub const ALL: [Self; 8] = [
        Self::Readable,
        Self::Writable,
        Self::Mutable,
        Self::Movable,
        Self::SharedBorrowable,
        Self::MutablyBorrowable,
        Self::DuplicateThenOwn,
        Self::LivenessAtUse,
    ];
}

/// Function-boundary capability kinds (F-CAP-01 through F-CAP-06).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FunctionCapabilityKind {
    /// Parameter access posture is settled.
    ParameterAccessPosture,
    /// Capture posture is settled.
    CapturePosture,
    /// Effects and alternate-exit posture are settled.
    EffectsAndAlternateExit,
    /// Returned-borrow provenance is settled.
    ReturnedBorrowProvenance,
    /// Required region relationships are settled.
    RequiredRegionRelationships,
    /// Exported visibility is settled.
    ExportedVisibility,
}

impl FunctionCapabilityKind {
    /// All function-boundary capability rows in their stable ledger order.
    pub const ALL: [Self; 6] = [
        Self::ParameterAccessPosture,
        Self::CapturePosture,
        Self::EffectsAndAlternateExit,
        Self::ReturnedBorrowProvenance,
        Self::RequiredRegionRelationships,
        Self::ExportedVisibility,
    ];
}

/// One capability kind across all target-neutral subject families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityKind {
    /// A type capability.
    Type(TypeCapabilityKind),
    /// An index capability.
    Index(IndexCapabilityKind),
    /// A place/value capability.
    Place(PlaceCapabilityKind),
    /// A function-boundary capability.
    Function(FunctionCapabilityKind),
}

impl CapabilityKind {
    /// Return the family owning this kind.
    #[must_use]
    pub const fn family(self) -> CapabilityFamily {
        match self {
            Self::Type(_) => CapabilityFamily::Type,
            Self::Index(_) => CapabilityFamily::Index,
            Self::Place(_) => CapabilityFamily::Place,
            Self::Function(_) => CapabilityFamily::Function,
        }
    }
}

/// Classified reason why a capability proof is not available.
///
/// These variants are intentionally closed and payload-free. The classification
/// is stable for consumers and diagnostics; source spans and richer evidence
/// remain owned by the relevant semantic authority rather than being copied
/// into this vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityReason {
    /// The subject already has a semantic error.
    Error,
    /// Inference has not settled the required fact.
    UnresolvedInference,
    /// An observable index was omitted or remains unspecified.
    UnspecifiedObservableIndex,
    /// The recursive shape is outside the supported recursive law.
    UnsupportedRecursion,
    /// The authoritative fact is absent from the coherent snapshot.
    MissingFact,
    /// A generic parameter has no constraint sufficient for this proof.
    UnconstrainedGeneric,
    /// The semantic shape has no supported representation for the query.
    UnsupportedRepresentation,
    /// An open boundary cannot preserve or revalidate an observable distinction.
    UnpreservableOpenBoundary,
}

impl CapabilityReason {
    /// All classified reasons in their stable wire/diagnostic order.
    pub const ALL: [Self; 8] = [
        Self::Error,
        Self::UnresolvedInference,
        Self::UnspecifiedObservableIndex,
        Self::UnsupportedRecursion,
        Self::MissingFact,
        Self::UnconstrainedGeneric,
        Self::UnsupportedRepresentation,
        Self::UnpreservableOpenBoundary,
    ];

    /// Stable machine-readable reason code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::UnresolvedInference => "unresolved-inference",
            Self::UnspecifiedObservableIndex => "unspecified-observable-index",
            Self::UnsupportedRecursion => "unsupported-recursion",
            Self::MissingFact => "missing-fact",
            Self::UnconstrainedGeneric => "unconstrained-generic",
            Self::UnsupportedRepresentation => "unsupported-representation",
            Self::UnpreservableOpenBoundary => "unpreservable-open-boundary",
        }
    }
}

impl fmt::Display for CapabilityReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// A proof or a classified fail-closed reason.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityAnswer<P> {
    /// The semantic authority proved the requested capability.
    Proven(P),
    /// The proof is unavailable and the consumer must not guess.
    Rejected(CapabilityReason),
}

impl<P> CapabilityAnswer<P> {
    /// Construct a proven answer.
    #[must_use]
    pub const fn proven(proof: P) -> Self {
        Self::Proven(proof)
    }

    /// Construct a fail-closed answer.
    #[must_use]
    pub const fn rejected(reason: CapabilityReason) -> Self {
        Self::Rejected(reason)
    }

    /// Construct the standard missing-authority answer.
    #[must_use]
    pub const fn missing_fact() -> Self {
        Self::Rejected(CapabilityReason::MissingFact)
    }

    /// Borrow the proof when this answer is proven.
    #[must_use]
    pub fn proof(&self) -> Option<&P> {
        match self {
            Self::Proven(proof) => Some(proof),
            Self::Rejected(_) => None,
        }
    }

    /// Return the classified rejection when this answer is not proven.
    #[must_use]
    pub const fn reason(&self) -> Option<CapabilityReason> {
        match self {
            Self::Proven(_) => None,
            Self::Rejected(reason) => Some(*reason),
        }
    }
}

/// The outcome of one recursive capability walk.
///
/// `Visiting` is a real intermediate state, not a proof. A caller that cannot
/// close the recursive walk must convert it to a rejected
/// [`CapabilityReason::UnsupportedRecursion`] answer.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RecursionOutcome<P> {
    /// The subject is on the active recursion stack.
    Visiting,
    /// The recursive walk closed with a proof.
    Proven(P),
    /// The recursive walk closed with a classified rejection.
    Unsupported(CapabilityReason),
}

impl<P> RecursionOutcome<P> {
    /// Convert a recursive walk outcome to the public fail-closed answer.
    #[must_use]
    pub fn into_answer(self) -> CapabilityAnswer<P> {
        match self {
            Self::Visiting => CapabilityAnswer::Rejected(CapabilityReason::UnsupportedRecursion),
            Self::Proven(proof) => CapabilityAnswer::Proven(proof),
            Self::Unsupported(reason) => CapabilityAnswer::Rejected(reason),
        }
    }
}

/// A settled indexed identity carried by a T-CAP-08 proof.
///
/// `dimensions`, `tuple_order`, and `size_arguments` retain source-semantic
/// order. `None` means that component is not applicable to the type, while a
/// present [`IndexId`] is an observable component that a representation must
/// preserve or validate later.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SolvedIndexedIdentity {
    /// Observable scalar width, when the type has one.
    pub width: Option<IndexId>,
    /// Observable capacity, when the type has one.
    pub capacity: Option<IndexId>,
    /// Observable rank, when the type has one.
    pub rank: Option<IndexId>,
    /// Ordered dimensions of the indexed shape.
    pub dimensions: Vec<IndexId>,
    /// Ordered tuple identity, when tuple order is observable.
    pub tuple_order: Vec<IndexId>,
    /// Ordered size arguments supplied by the source.
    pub size_arguments: Vec<IndexId>,
}

/// Target-neutral proof values for type capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypeCapabilityProof {
    /// T-CAP-01.
    Transferable,
    /// T-CAP-02; distinct from trivial duplication.
    ExplicitlyDuplicable,
    /// T-CAP-03; distinct from explicit duplication.
    TriviallyDuplicable,
    /// T-CAP-04; distinct from reflexivity.
    StrictlyEqual,
    /// T-CAP-05; distinct from strict equality.
    ReflexivelyEqual,
    /// T-CAP-06; distinct from equality eligibility.
    CanonicalHashEligible,
    /// T-CAP-07.
    Ordered,
    /// T-CAP-08.
    SolvedIndexedIdentity(SolvedIndexedIdentity),
}

/// Target-neutral proof value for a solved index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IndexCapabilityProof {
    /// The indexed identity is solved.
    Solved,
}

/// Target-neutral liveness value at a place use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Liveness {
    /// The source remains live at the use.
    Live,
    /// The source is dead at the use.
    Dead,
}

/// Target-neutral proof values for place/value capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlaceCapabilityProof {
    /// P-CAP-01.
    Readable,
    /// P-CAP-02.
    Writable,
    /// P-CAP-03.
    Mutable,
    /// P-CAP-04.
    Movable,
    /// P-CAP-05.
    SharedBorrowable,
    /// P-CAP-06.
    MutablyBorrowable,
    /// P-CAP-07.
    DuplicateThenOwn,
    /// P-CAP-08.
    Liveness(Liveness),
}

/// Parameter access posture, intentionally independent of target references.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParameterAccessPosture {
    /// The parameter is received as an owned value.
    Owned,
    /// The parameter is received through shared access.
    Shared,
    /// The parameter is received through mutable access.
    Mutable,
    /// The parameter is consumed by the boundary.
    Move,
}

/// One parameter's settled access posture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParameterAccess {
    /// Existing definition identity of the parameter.
    pub parameter: DefId,
    /// Semantic access posture.
    pub posture: ParameterAccessPosture,
}

/// Capture posture for one captured definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapturePosture {
    /// The capture is read through shared access.
    Shared,
    /// The capture is mutated.
    Mutable,
    /// The capture is consumed.
    Consuming,
}

/// One captured definition's settled posture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CaptureRequirement {
    /// Existing definition identity of the capture.
    pub definition: DefId,
    /// Semantic capture posture.
    pub posture: CapturePosture,
}

/// Settled effects and alternate-exit posture.
// The four posture flags are one settled vocabulary record, not independent
// switches; they travel together by design.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EffectPosture {
    /// The body can produce a failure/alternate error result.
    pub may_fail: bool,
    /// The body can suspend.
    pub may_suspend: bool,
    /// The body is a generator/cursor.
    pub is_generator: bool,
    /// The boundary declares or carries an alternate exit.
    pub has_alternate_exit: bool,
}

/// Provenance of a returned borrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReturnedBorrowProvenance {
    /// The return value is not a borrow.
    None,
    /// The borrow originates from an existing parameter definition.
    Parameter(DefId),
    /// The borrow originates from another existing definition.
    Definition(DefId),
}

/// One required relationship between existing boundary definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RegionRelationship {
    /// Definition whose validity must outlive `to`.
    pub from: DefId,
    /// Definition requiring the relationship.
    pub to: DefId,
}

/// Settled required-region relationships for a function boundary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequiredRegionRelationships {
    /// Relationships in deterministic authority order.
    pub relationships: Vec<RegionRelationship>,
}

/// Target-neutral exported visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExportVisibility {
    /// Visible to all permitted consumers.
    Public,
    /// Visible to consumers in the same package.
    Package,
    /// Visible only in the declaring module.
    Module,
}

/// Target-neutral proof values for function-boundary capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FunctionCapabilityProof {
    /// F-CAP-01.
    ParameterAccess(Vec<ParameterAccess>),
    /// F-CAP-02.
    Capture(Vec<CaptureRequirement>),
    /// F-CAP-03.
    Effects(EffectPosture),
    /// F-CAP-04.
    ReturnedBorrow(ReturnedBorrowProvenance),
    /// F-CAP-05.
    RequiredRegions(RequiredRegionRelationships),
    /// F-CAP-06.
    ExportedVisibility(ExportVisibility),
}

pub type TypeCapabilityAnswer = CapabilityAnswer<TypeCapabilityProof>;
pub type IndexCapabilityAnswer = CapabilityAnswer<IndexCapabilityProof>;
pub type PlaceCapabilityAnswer = CapabilityAnswer<PlaceCapabilityProof>;
pub type FunctionCapabilityAnswer = CapabilityAnswer<FunctionCapabilityProof>;

/// Immutable type capability summary keyed by an existing [`TypeId`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeCapabilitySummary {
    /// Existing type-table identity.
    pub type_id: TypeId,
    /// T-CAP-01.
    pub transferable: TypeCapabilityAnswer,
    /// T-CAP-02.
    pub explicitly_duplicable: TypeCapabilityAnswer,
    /// T-CAP-03.
    pub trivially_duplicable: TypeCapabilityAnswer,
    /// T-CAP-04.
    pub strict_equality: TypeCapabilityAnswer,
    /// T-CAP-05.
    pub reflexive_equality: TypeCapabilityAnswer,
    /// T-CAP-06.
    pub canonical_hash_eligibility: TypeCapabilityAnswer,
    /// T-CAP-07.
    pub ordering: TypeCapabilityAnswer,
    /// T-CAP-08.
    pub solved_indexed_identity: TypeCapabilityAnswer,
}

impl TypeCapabilitySummary {
    /// Construct a summary that rejects every capability until an authority
    /// supplies the corresponding proof.
    #[must_use]
    pub fn missing(type_id: TypeId) -> Self {
        Self {
            type_id,
            transferable: CapabilityAnswer::missing_fact(),
            explicitly_duplicable: CapabilityAnswer::missing_fact(),
            trivially_duplicable: CapabilityAnswer::missing_fact(),
            strict_equality: CapabilityAnswer::missing_fact(),
            reflexive_equality: CapabilityAnswer::missing_fact(),
            canonical_hash_eligibility: CapabilityAnswer::missing_fact(),
            ordering: CapabilityAnswer::missing_fact(),
            solved_indexed_identity: CapabilityAnswer::missing_fact(),
        }
    }

    /// Return the answer for one type capability kind.
    #[must_use]
    pub fn capability(&self, kind: TypeCapabilityKind) -> &TypeCapabilityAnswer {
        match kind {
            TypeCapabilityKind::Transferable => &self.transferable,
            TypeCapabilityKind::ExplicitlyDuplicable => &self.explicitly_duplicable,
            TypeCapabilityKind::TriviallyDuplicable => &self.trivially_duplicable,
            TypeCapabilityKind::StrictEquality => &self.strict_equality,
            TypeCapabilityKind::ReflexiveEquality => &self.reflexive_equality,
            TypeCapabilityKind::CanonicalHashEligibility => &self.canonical_hash_eligibility,
            TypeCapabilityKind::Ordering => &self.ordering,
            TypeCapabilityKind::SolvedIndexedIdentity => &self.solved_indexed_identity,
        }
    }
}

/// Immutable index capability summary keyed by an existing [`IndexId`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IndexCapabilitySummary {
    /// Existing index-table identity.
    pub index_id: IndexId,
    /// Whether this index has a solved identity.
    pub solved_identity: IndexCapabilityAnswer,
}

impl IndexCapabilitySummary {
    /// Construct a summary that rejects until the index authority supplies a
    /// solved identity.
    #[must_use]
    pub const fn missing(index_id: IndexId) -> Self {
        Self {
            index_id,
            solved_identity: CapabilityAnswer::missing_fact(),
        }
    }
}

/// Immutable function-boundary capability summary keyed by an existing
/// function [`DefId`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionCapabilitySummary {
    /// Existing function or method definition identity.
    pub def_id: DefId,
    /// F-CAP-01.
    pub parameter_access_posture: FunctionCapabilityAnswer,
    /// F-CAP-02.
    pub capture_posture: FunctionCapabilityAnswer,
    /// F-CAP-03.
    pub effects_and_alternate_exit: FunctionCapabilityAnswer,
    /// F-CAP-04.
    pub returned_borrow_provenance: FunctionCapabilityAnswer,
    /// F-CAP-05.
    pub required_region_relationships: FunctionCapabilityAnswer,
    /// F-CAP-06.
    pub exported_visibility: FunctionCapabilityAnswer,
}

impl FunctionCapabilitySummary {
    /// Construct a summary that rejects every capability until semantic
    /// authorities supply the corresponding proofs.
    #[must_use]
    pub fn missing(def_id: DefId) -> Self {
        Self {
            def_id,
            parameter_access_posture: CapabilityAnswer::missing_fact(),
            capture_posture: CapabilityAnswer::missing_fact(),
            effects_and_alternate_exit: CapabilityAnswer::missing_fact(),
            returned_borrow_provenance: CapabilityAnswer::missing_fact(),
            required_region_relationships: CapabilityAnswer::missing_fact(),
            exported_visibility: CapabilityAnswer::missing_fact(),
        }
    }

    /// Return the answer for one function capability kind.
    #[must_use]
    pub fn capability(&self, kind: FunctionCapabilityKind) -> &FunctionCapabilityAnswer {
        match kind {
            FunctionCapabilityKind::ParameterAccessPosture => &self.parameter_access_posture,
            FunctionCapabilityKind::CapturePosture => &self.capture_posture,
            FunctionCapabilityKind::EffectsAndAlternateExit => &self.effects_and_alternate_exit,
            FunctionCapabilityKind::ReturnedBorrowProvenance => &self.returned_borrow_provenance,
            FunctionCapabilityKind::RequiredRegionRelationships => {
                &self.required_region_relationships
            }
            FunctionCapabilityKind::ExportedVisibility => &self.exported_visibility,
        }
    }
}
