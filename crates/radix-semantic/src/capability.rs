//! Semantic capability query interfaces.
//!
//! This module is the seam between existing semantic authorities and later
//! capability derivation. It owns no type, access, region, function, resolved
//! use, qualified identity, or visibility table. Implementors must answer from
//! the immutable authorities belonging to one analyzed snapshot.

use crate::SemanticParamMode;
use crate::access_facts::AccessFactTable;
use crate::function_facts::{CaptureMode, FunctionFactTable};
use crate::passes::borrow_region::RegionFacts;
use crate::passes::visibility_annotation::VisibilityTier;
use handroll::FxHashMap;
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{
    DefId, HirEnum, HirExpressionKind, HirFunction, HirId, HirItemKind, HirModule, HirStruct,
};
use radix_types::{
    AccessMarker, AppliedArg, IndexExpr, IndexId, NumericWidth, Primitive, Type, TypeId,
};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

pub use radix_types::{
    CAPABILITY_VOCABULARY_VERSION, CapabilityAnswer, CapabilityFamily, CapabilityKind,
    CapabilityReason, CapturePosture, CaptureRequirement, EffectPosture, ExportVisibility,
    FunctionCapabilityAnswer, FunctionCapabilityKind, FunctionCapabilityProof,
    FunctionCapabilitySummary, IndexCapabilityAnswer, IndexCapabilityKind, IndexCapabilityProof,
    IndexCapabilitySummary, Liveness, ParameterAccess, ParameterAccessPosture,
    PlaceCapabilityAnswer, PlaceCapabilityKind, PlaceCapabilityProof, RecursionOutcome,
    RegionRelationship, RequiredRegionRelationships, ReturnedBorrowProvenance,
    SolvedIndexedIdentity, TypeCapabilityAnswer, TypeCapabilityKind, TypeCapabilityProof,
    TypeCapabilitySummary,
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

/// One durable last-use fact: the borrow binding and its final use site.
///
/// `site` is the last use's HIR identity, not a CFG location — the CFG is an
/// incidental [`RegionFacts`] structure and is never serialized (SCR-04
/// persistence law).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LastUseFact {
    /// Borrow binding the region engine attributed the region to.
    pub borrower: DefId,
    /// HIR identity of the region's last use.
    pub site: HirId,
}

/// Durable codegen-facing reduction of one function's flow-sensitive borrow
/// regions (SCR-04). Carries only stable signature/provenance facts; the
/// full CFG and `RegionFacts` layout stay behind the analysis boundary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionProvenanceSummary {
    /// Existing function or method definition identity.
    pub function: DefId,
    /// F-CAP-04: returned-borrow provenance.
    pub returned_borrow: ReturnedBorrowProvenance,
    /// F-CAP-05: required region relationships in deterministic order.
    pub required_regions: Vec<RegionRelationship>,
    /// P-CAP-08 durable portion: last use per borrow binding.
    pub last_uses: Vec<LastUseFact>,
}

/// Durable provenance summaries for every function and method of one
/// analyzed snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionProvenanceTable {
    rows: Vec<FunctionProvenanceSummary>,
}

impl FunctionProvenanceTable {
    /// Wrap pre-reduced rows, assumed already keyed by distinct functions.
    #[must_use]
    pub fn from_rows(rows: Vec<FunctionProvenanceSummary>) -> Self {
        Self { rows }
    }

    /// The summary for one function, when present.
    #[must_use]
    pub fn get(&self, function: DefId) -> Option<&FunctionProvenanceSummary> {
        self.rows.iter().find(|row| row.function == function)
    }

    /// Iterate every row in construction order.
    pub fn iter(&self) -> impl Iterator<Item = &FunctionProvenanceSummary> {
        self.rows.iter()
    }

    /// Whether the table carries no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
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

// ── Derived queries (SCR-03) ─────────────────────────────────────────────────

/// Structural predicate being proved for one type.
///
/// Each law keeps `copy`, trivial copy, general duplication, strict equality,
/// reflexive equality, and ordering distinct; two laws may agree on a type
/// today without ever collapsing into one kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeLaw {
    /// T-CAP-01: an owning destination can receive the value.
    Transfer,
    /// T-CAP-02: explicit duplicate-then-own acquisition is admitted.
    ExplicitDuplicate,
    /// T-CAP-03: duplication has trivial (bitwise) semantics.
    TrivialDuplicate,
    /// T-CAP-04: strict same-type equality is admitted.
    StrictEquality,
    /// T-CAP-05: strict equality is reflexive for every value.
    ReflexiveEquality,
    /// T-CAP-06: hash values consistently with strict, reflexive equality.
    CanonicalHash,
    /// T-CAP-07: the language ordering law is admitted.
    Ordering,
}

/// Read-only derived capability queries over one coherent analysis snapshot.
///
/// Every authority is borrowed, not copied: [`TypeTable`] strict identity and
/// `IndexExpr`, HIR nominal declarations, [`AccessFactTable`] events,
/// [`FunctionFactTable`] rows, optional [`RegionFacts`], and a caller-supplied
/// visibility-tier view. The only owned state is memoized derived answers and
/// lazily built lookup indexes over the borrowed authorities — never a second
/// mutable authority.
///
/// Fail-closed laws (binding cases of the delivery card):
///
/// - a subject containing `Type::Error` answers [`CapabilityReason::Error`];
/// - unresolved inference and generic parameters never become guesses;
/// - an observable `IndexExpr::Unspecified` answers
///   [`CapabilityReason::UnspecifiedObservableIndex`];
/// - recursive shapes answer [`CapabilityReason::UnsupportedRecursion`];
/// - a solved shape that simply does not admit a capability answers
///   [`CapabilityReason::UnsupportedRepresentation`] — the closed reason
///   vocabulary carries no negative-proof variant, and this reason records a
///   settled negative rather than an unknown.
pub struct DerivedCapabilityQueries<'a> {
    types: &'a radix_types::TypeTable,
    hir: &'a HirModule,
    access_facts: &'a AccessFactTable,
    function_facts: &'a FunctionFactTable,
    regions: Option<&'a RegionFacts>,
    provenance: Option<&'a FunctionProvenanceTable>,
    visibility: FxHashMap<DefId, VisibilityTier>,
    memo: RefCell<QueryMemo<'a>>,
}

#[derive(Default)]
struct QueryMemo<'a> {
    type_answers: FxHashMap<(TypeId, TypeCapabilityKind), TypeCapabilityAnswer>,
    index_answers: FxHashMap<IndexId, IndexCapabilityAnswer>,
    place_answers: FxHashMap<(HirId, PlaceCapabilityKind), PlaceCapabilityAnswer>,
    function_answers: FxHashMap<(DefId, FunctionCapabilityKind), FunctionCapabilityAnswer>,
    sites: Option<FxHashMap<HirId, PlaceSite>>,
    nominal: Option<FxHashMap<DefId, NominalDecl<'a>>>,
    functions: Option<FxHashMap<DefId, &'a HirFunction>>,
}

#[derive(Clone, Copy)]
enum NominalDecl<'a> {
    Struct(&'a HirStruct),
    Enum(&'a HirEnum),
}

impl<'a> DerivedCapabilityQueries<'a> {
    /// Build the query layer over the four always-present authorities.
    #[must_use]
    pub fn new(
        types: &'a radix_types::TypeTable,
        hir: &'a HirModule,
        access_facts: &'a AccessFactTable,
        function_facts: &'a FunctionFactTable,
    ) -> Self {
        Self {
            types,
            hir,
            access_facts,
            function_facts,
            regions: None,
            provenance: None,
            visibility: FxHashMap::default(),
            memo: RefCell::new(QueryMemo::default()),
        }
    }

    /// Build the query layer from one coherent [`AnalyzedApplication`].
    ///
    /// Region facts and visibility tiers are absent by default; the
    /// corresponding builders attach them without re-running analysis.
    #[must_use]
    pub fn from_application(app: &'a crate::AnalyzedApplication) -> Self {
        Self::new(
            app.types(),
            app.hir(),
            app.access_facts(),
            app.function_facts(),
        )
        .with_provenance(app.provenance())
    }

    /// Attach region facts produced from the same snapshot's access facts.
    #[must_use]
    pub fn with_regions(mut self, regions: &'a RegionFacts) -> Self {
        self.regions = Some(regions);
        self
    }

    /// Attach settled visibility tiers keyed by existing definition identity.
    ///
    /// The map is a borrowed read view of the driver's export-surface facts;
    /// without it, exported-visibility queries fail closed with
    /// [`CapabilityReason::MissingFact`].
    #[must_use]
    pub fn with_visibility(
        mut self,
        tiers: impl IntoIterator<Item = (DefId, VisibilityTier)>,
    ) -> Self {
        self.visibility = tiers.into_iter().collect();
        self
    }

    /// Attach the durable provenance summaries from the same snapshot.
    ///
    /// F-CAP-04 and F-CAP-05 answer from this table; without it they fail
    /// closed with [`CapabilityReason::MissingFact`].
    #[must_use]
    pub fn with_provenance(mut self, provenance: &'a FunctionProvenanceTable) -> Self {
        self.provenance = Some(provenance);
        self
    }

    /// Whether region facts were attached to this query layer.
    #[must_use]
    pub fn has_regions(&self) -> bool {
        self.regions.is_some()
    }

    // ── classification helpers ───────────────────────────────────────────

    /// Terminal classification failure for one type, including the recursion
    /// guard: a type already on the active walk is an unsupported recursion.
    fn classification_failure(&self, ty: TypeId, visiting: &[TypeId]) -> Option<CapabilityReason> {
        if visiting.contains(&ty) {
            return Some(CapabilityReason::UnsupportedRecursion);
        }
        match self.types.get(ty) {
            Type::Error => Some(CapabilityReason::Error),
            Type::Infer(_) | Type::InferUnion(_) | Type::MarkerHole { .. } => {
                Some(CapabilityReason::UnresolvedInference)
            }
            Type::Param(_) => Some(CapabilityReason::UnconstrainedGeneric),
            _ => None,
        }
    }

    /// Direct structural child types of one type (nominal fields excluded).
    fn child_types(&self, ty: TypeId) -> Vec<TypeId> {
        match self.types.get(ty) {
            Type::Array(t)
            | Type::Queue(t)
            | Type::Stack(t)
            | Type::Set(t)
            | Type::Promissum(t)
            | Type::Cursor(t)
            | Type::Atomic(t)
            | Type::Intervallum(t, _)
            | Type::Option(t)
            | Type::Alias(_, t)
            | Type::Ref(_, t) => vec![*t],
            Type::BoundedArray { element, .. }
            | Type::RangedArray { element, .. }
            | Type::BoundedQueue { element, .. }
            | Type::BoundedStack { element, .. } => vec![*element],
            Type::Map(k, v) | Type::PromissumFailable(k, v) | Type::AsyncCursor(k, v) => {
                vec![*k, *v]
            }
            Type::Tensor(t, _) | Type::Vector(t, _) | Type::Matrix(t, _) | Type::Sparsa(t, _) => {
                vec![*t]
            }
            Type::Record(fields) => {
                let mut rows: Vec<_> = fields.values().copied().collect();
                rows.sort_unstable_by_key(|id| id.0);
                rows
            }
            Type::Union(members) | Type::Tuple(members) | Type::Intersection(members) => {
                members.clone()
            }
            Type::Func(sig) => {
                let mut out: Vec<TypeId> = sig.params.iter().map(|p| p.ty).collect();
                out.push(sig.ret);
                out.extend(sig.err);
                out
            }
            Type::Applied(base, args) => {
                let mut out = vec![*base];
                out.extend(args.iter().filter_map(|arg| match arg {
                    AppliedArg::Type(t) => Some(*t),
                    AppliedArg::Size(_) => None,
                }));
                out
            }
            _ => Vec::new(),
        }
    }

    /// Walk a whole type (including nominal fields) proving only that it is
    /// free of error, unresolved inference, unconstrained generics, and
    /// unsupported recursion.
    fn classify_only(
        &self,
        ty: TypeId,
        visiting: &mut Vec<TypeId>,
    ) -> Result<(), CapabilityReason> {
        if let Some(reason) = self.classification_failure(ty, visiting) {
            return Err(reason);
        }
        match self.types.get(ty) {
            Type::Alias(_, inner) => {
                visiting.push(ty);
                let result = self.classify_only(*inner, visiting);
                visiting.pop();
                result
            }
            Type::Struct(_) | Type::Enum(_) => {
                visiting.push(ty);
                let result = self
                    .nominal_field_types(ty)
                    .and_then(|fields| self.classify_fields(&fields, visiting));
                visiting.pop();
                result
            }
            _ => {
                for child in self.child_types(ty) {
                    self.classify_only(child, visiting)?;
                }
                Ok(())
            }
        }
    }

    fn classify_fields(
        &self,
        fields: &[TypeId],
        visiting: &mut Vec<TypeId>,
    ) -> Result<(), CapabilityReason> {
        for field in fields {
            self.classify_only(*field, visiting)?;
        }
        Ok(())
    }

    // ── nominal declarations ─────────────────────────────────────────────

    fn nominal_decl(&self, def_id: DefId) -> Option<NominalDecl<'a>> {
        let mut memo = self.memo.borrow_mut();
        if memo.nominal.is_none() {
            let mut map = FxHashMap::default();
            for item in &self.hir.items {
                match &item.kind {
                    HirItemKind::Struct(strukt) => {
                        map.insert(item.def_id, NominalDecl::Struct(strukt));
                    }
                    HirItemKind::Enum(enumeratio) => {
                        map.insert(item.def_id, NominalDecl::Enum(enumeratio));
                    }
                    _ => {}
                }
            }
            memo.nominal = Some(map);
        }
        memo.nominal.as_ref()?.get(&def_id).copied()
    }

    /// Field types of one nominal declaration.
    ///
    /// A missing declaration (imported nominal) is
    /// [`CapabilityReason::MissingFact`]. Nominal recursion is caught by the
    /// caller's type-identity walk, which pushes the nominal `TypeId` before
    /// descending into these fields.
    fn nominal_field_types(&self, ty: TypeId) -> Result<Vec<TypeId>, CapabilityReason> {
        let def = match self.types.get(ty) {
            Type::Struct(def) | Type::Enum(def) => *def,
            _ => return Ok(Vec::new()),
        };
        match self.nominal_decl(def) {
            Some(NominalDecl::Struct(strukt)) => Ok(strukt.fields.iter().map(|f| f.ty).collect()),
            Some(NominalDecl::Enum(enumeratio)) => {
                let mut fields: Vec<TypeId> =
                    enumeratio.commune_fields.iter().map(|f| f.ty).collect();
                for variant in &enumeratio.variants {
                    fields.extend(variant.fields.iter().map(|f| f.ty));
                }
                Ok(fields)
            }
            None => Err(CapabilityReason::MissingFact),
        }
    }

    // ── type laws ────────────────────────────────────────────────────────

    fn primitive_law(prim: Primitive, law: TypeLaw) -> bool {
        match law {
            TypeLaw::Transfer | TypeLaw::ExplicitDuplicate => true,
            TypeLaw::TrivialDuplicate => matches!(
                prim,
                Primitive::Numerus
                    | Primitive::Fractus
                    | Primitive::Bivalens
                    | Primitive::Instans
                    | Primitive::Nihil
            ),
            TypeLaw::StrictEquality => matches!(
                prim,
                Primitive::Numerus
                    | Primitive::Fractus
                    | Primitive::Bivalens
                    | Primitive::Ascii
                    | Primitive::Textus
                    | Primitive::Instans
                    | Primitive::Nihil
            ),
            // Floating equality is not reflexive for every value (NaN), so a
            // bare `fractus` never proves reflexivity.
            TypeLaw::ReflexiveEquality => matches!(
                prim,
                Primitive::Numerus
                    | Primitive::Bivalens
                    | Primitive::Ascii
                    | Primitive::Textus
                    | Primitive::Instans
                    | Primitive::Nihil
            ),
            // D3.4 admits canonical hash keys only for non-floating values;
            // the hashability walk also recurses through tuple/genus fields.
            TypeLaw::CanonicalHash => matches!(
                prim,
                Primitive::Numerus
                    | Primitive::Bivalens
                    | Primitive::Ascii
                    | Primitive::Textus
                    | Primitive::Instans
                    | Primitive::Nihil
            ),
            TypeLaw::Ordering => matches!(
                prim,
                Primitive::Numerus | Primitive::Fractus | Primitive::Instans
            ),
        }
    }

    fn sized_numeric_law(prim: Primitive, width: NumericWidth, law: TypeLaw) -> bool {
        if matches!(law, TypeLaw::CanonicalHash) {
            return prim != Primitive::Fractus;
        }
        match prim {
            Primitive::Fractus => !matches!(law, TypeLaw::ReflexiveEquality),
            // The unbounded `inf` is a heap bignum on every carrier: duplicating
            // it is an explicit `clone`, never a bitwise copy.
            _ => !(width.is_unbounded() && matches!(law, TypeLaw::TrivialDuplicate)),
        }
    }

    fn type_law(
        &self,
        ty: TypeId,
        law: TypeLaw,
        visiting: &mut Vec<TypeId>,
    ) -> Result<bool, CapabilityReason> {
        if let Some(reason) = self.classification_failure(ty, visiting) {
            return Err(reason);
        }
        match self.types.get(ty) {
            Type::Alias(_, inner) => {
                visiting.push(ty);
                let result = self.type_law(*inner, law, visiting);
                visiting.pop();
                result
            }
            Type::Ref(mutability, inner) => match (law, mutability.access_marker()) {
                (
                    TypeLaw::Transfer | TypeLaw::ExplicitDuplicate,
                    AccessMarker::Own | AccessMarker::Copy,
                ) => self.type_law(*inner, law, visiting),
                // Duplicating a shared borrow copies the view itself; the
                // pointee's own duplication law is not consulted, but its
                // classification failures still propagate.
                (TypeLaw::TrivialDuplicate, AccessMarker::Ref) => {
                    self.classify_only(*inner, visiting)?;
                    Ok(true)
                }
                // Transfers and acquisitions that cannot take ownership, and
                // duplications of any view other than a shared borrow, still
                // propagate classification failures from the pointee.
                (TypeLaw::Transfer | TypeLaw::ExplicitDuplicate | TypeLaw::TrivialDuplicate, _) => {
                    self.classify_only(*inner, visiting)?;
                    Ok(false)
                }
                (_, _) => self.type_law(*inner, law, visiting),
            },
            Type::Primitive(prim) => Ok(Self::primitive_law(*prim, law)),
            Type::SizedNumeric(prim, width) => Ok(Self::sized_numeric_law(*prim, *width, law)),
            Type::ModularWord(_) | Type::SaturatingWord(_) | Type::SizedInstans(_) => Ok(true),
            // N7E-1: a policy-float cell carries the laws of the bare float of
            // its width (D2/D3: it evaluates as that float).
            Type::PolicyFloat(_, width) => {
                Ok(Self::sized_numeric_law(Primitive::Fractus, *width, law))
            }
            Type::Option(inner) => {
                Ok(self.type_law(*inner, law, visiting)? && !matches!(law, TypeLaw::Ordering))
            }
            Type::Tuple(members) | Type::Union(members) | Type::Intersection(members) => {
                let mut holds = true;
                for &member in members {
                    if !self.type_law(member, law, visiting)? {
                        holds = false;
                    }
                }
                Ok(holds && !matches!(law, TypeLaw::Ordering))
            }
            Type::Struct(_) | Type::Enum(_) => {
                visiting.push(ty);
                let result = self.nominal_field_types(ty).and_then(|fields| {
                    let mut holds = true;
                    for field in &fields {
                        if !self.type_law(*field, law, visiting)? {
                            holds = false;
                        }
                    }
                    Ok(holds)
                });
                visiting.pop();
                result
            }
            Type::Interface(_) => Ok(matches!(
                law,
                TypeLaw::Transfer | TypeLaw::ExplicitDuplicate
            )),
            Type::Func(_) => {
                for child in self.child_types(ty) {
                    self.classify_only(child, visiting)?;
                }
                Ok(matches!(
                    law,
                    TypeLaw::Transfer | TypeLaw::ExplicitDuplicate
                ))
            }
            Type::Applied(_, _) => {
                for child in self.child_types(ty) {
                    self.classify_only(child, visiting)?;
                }
                // Generic application substitution is not reconstructed here;
                // an applied nominal cannot prove structural laws.
                Ok(false)
            }
            // Collections, buffers, streams, and cells: element types are
            // classified for failure reasons, but container-shape laws
            // (trivial copy, equality, ordering) are not settled for them.
            _ => {
                for child in self.child_types(ty) {
                    self.classify_only(child, visiting)?;
                }
                Ok(matches!(
                    law,
                    TypeLaw::Transfer | TypeLaw::ExplicitDuplicate
                ))
            }
        }
    }

    // ── index solving ────────────────────────────────────────────────────

    fn index_solved(&self, index_id: IndexId) -> Result<(), CapabilityReason> {
        match self.types.get_index(index_id) {
            IndexExpr::Literal(_) | IndexExpr::Param(_) | IndexExpr::Runtime(_) => Ok(()),
            IndexExpr::Affine { base, .. } => {
                if radix_types::evaluate_index_dimension(index_id, self.types).is_err() {
                    return Err(CapabilityReason::UnsupportedRepresentation);
                }
                self.index_solved(*base)
            }
            IndexExpr::Divide { numerator, .. } => {
                if radix_types::evaluate_index_dimension(index_id, self.types).is_err() {
                    return Err(CapabilityReason::UnsupportedRepresentation);
                }
                self.index_solved(*numerator)
            }
            IndexExpr::Infer(_) => Err(CapabilityReason::UnresolvedInference),
            IndexExpr::Unspecified => Err(CapabilityReason::UnspecifiedObservableIndex),
            IndexExpr::Tuple(dims) => {
                for dim in dims {
                    self.index_solved(*dim)?;
                }
                Ok(())
            }
        }
    }

    // ── T-CAP-08 solved indexed identity ─────────────────────────────────

    fn merge_optional_index(
        &self,
        slot: &mut Option<IndexId>,
        candidate: IndexId,
    ) -> Result<(), CapabilityReason> {
        match *slot {
            None => {
                *slot = Some(candidate);
                Ok(())
            }
            Some(existing) if self.types.index_equals(existing, candidate) => Ok(()),
            // Two distinct observable values for one component cannot be
            // represented by the summary; fail closed instead of dropping one.
            Some(_) => Err(CapabilityReason::UnsupportedRepresentation),
        }
    }

    fn record_shape(
        &self,
        shape: IndexId,
        identity: &mut SolvedIndexedIdentity,
    ) -> Result<(), CapabilityReason> {
        self.index_solved(shape)?;
        if let IndexExpr::Tuple(dims) = self.types.get_index(shape) {
            for dim in dims {
                identity.dimensions.push(*dim);
                identity.tuple_order.push(*dim);
            }
        } else {
            identity.dimensions.push(shape);
        }
        Ok(())
    }

    fn solved_identity(
        &self,
        ty: TypeId,
        visiting: &mut Vec<TypeId>,
        identity: &mut SolvedIndexedIdentity,
    ) -> Result<(), CapabilityReason> {
        if let Some(reason) = self.classification_failure(ty, visiting) {
            return Err(reason);
        }
        match self.types.get(ty) {
            Type::Alias(_, inner) | Type::Ref(_, inner) => {
                visiting.push(ty);
                let result = self.solved_identity(*inner, visiting, identity);
                visiting.pop();
                result
            }
            Type::Vector(_, width) => {
                self.index_solved(*width)?;
                self.merge_optional_index(&mut identity.width, *width)?;
                self.child_identity(ty, visiting, identity)
            }
            Type::BoundedArray { capacity, .. }
            | Type::BoundedQueue { capacity, .. }
            | Type::BoundedStack { capacity, .. } => {
                self.index_solved(*capacity)?;
                self.merge_optional_index(&mut identity.capacity, *capacity)?;
                self.child_identity(ty, visiting, identity)
            }
            Type::BoundedTextus { capacity }
            | Type::BoundedAscii { capacity }
            | Type::BoundedOcteti { capacity } => {
                self.index_solved(*capacity)?;
                self.merge_optional_index(&mut identity.capacity, *capacity)?;
                Ok(())
            }
            Type::Tensor(_, shape) | Type::Matrix(_, shape) | Type::Sparsa(_, shape) => {
                self.record_shape(*shape, identity)?;
                self.child_identity(ty, visiting, identity)
            }
            Type::Applied(_, args) => {
                for arg in args {
                    match arg {
                        AppliedArg::Size(size) => {
                            self.index_solved(*size)?;
                            identity.size_arguments.push(*size);
                        }
                        AppliedArg::Type(inner) => {
                            self.solved_identity(*inner, visiting, identity)?;
                        }
                    }
                }
                Ok(())
            }
            Type::Struct(_) | Type::Enum(_) => {
                visiting.push(ty);
                let result = self.nominal_field_types(ty).and_then(|fields| {
                    for field in fields {
                        self.solved_identity(field, visiting, identity)?;
                    }
                    Ok(())
                });
                visiting.pop();
                result
            }
            _ => self.child_identity(ty, visiting, identity),
        }
    }

    fn child_identity(
        &self,
        ty: TypeId,
        visiting: &mut Vec<TypeId>,
        identity: &mut SolvedIndexedIdentity,
    ) -> Result<(), CapabilityReason> {
        for child in self.child_types(ty) {
            self.solved_identity(child, visiting, identity)?;
        }
        Ok(())
    }

    // ── derivation entry points ──────────────────────────────────────────

    fn derive_type(&self, ty: TypeId, kind: TypeCapabilityKind) -> TypeCapabilityAnswer {
        // Classification failures outrank law-specific answers: an error,
        // unresolved, generic, or recursive subject never reaches a law.
        if let Some(reason) = self.classification_failure(ty, &[]) {
            return CapabilityAnswer::Rejected(reason);
        }
        let law_proof = |law: TypeLaw, proof: TypeCapabilityProof| match self.type_law(
            ty,
            law,
            &mut Vec::new(),
        ) {
            Ok(true) => CapabilityAnswer::Proven(proof),
            Ok(false) => CapabilityAnswer::Rejected(CapabilityReason::UnsupportedRepresentation),
            Err(reason) => CapabilityAnswer::Rejected(reason),
        };
        match kind {
            TypeCapabilityKind::Transferable => {
                law_proof(TypeLaw::Transfer, TypeCapabilityProof::Transferable)
            }
            TypeCapabilityKind::ExplicitlyDuplicable => law_proof(
                TypeLaw::ExplicitDuplicate,
                TypeCapabilityProof::ExplicitlyDuplicable,
            ),
            TypeCapabilityKind::TriviallyDuplicable => law_proof(
                TypeLaw::TrivialDuplicate,
                TypeCapabilityProof::TriviallyDuplicable,
            ),
            TypeCapabilityKind::StrictEquality => {
                law_proof(TypeLaw::StrictEquality, TypeCapabilityProof::StrictlyEqual)
            }
            TypeCapabilityKind::ReflexiveEquality => law_proof(
                TypeLaw::ReflexiveEquality,
                TypeCapabilityProof::ReflexivelyEqual,
            ),
            TypeCapabilityKind::CanonicalHashEligibility => law_proof(
                TypeLaw::CanonicalHash,
                TypeCapabilityProof::CanonicalHashEligible,
            ),
            TypeCapabilityKind::Ordering => {
                law_proof(TypeLaw::Ordering, TypeCapabilityProof::Ordered)
            }
            TypeCapabilityKind::SolvedIndexedIdentity => {
                let mut identity = SolvedIndexedIdentity {
                    width: None,
                    capacity: None,
                    rank: None,
                    dimensions: Vec::new(),
                    tuple_order: Vec::new(),
                    size_arguments: Vec::new(),
                };
                match self.solved_identity(ty, &mut Vec::new(), &mut identity) {
                    Ok(()) => CapabilityAnswer::Proven(TypeCapabilityProof::SolvedIndexedIdentity(
                        identity,
                    )),
                    Err(reason) => CapabilityAnswer::Rejected(reason),
                }
            }
        }
    }

    fn derive_index(&self, index_id: IndexId) -> IndexCapabilityAnswer {
        match self.index_solved(index_id) {
            Ok(()) => CapabilityAnswer::Proven(IndexCapabilityProof::Solved),
            Err(reason) => CapabilityAnswer::Rejected(reason),
        }
    }

    // ── place/value derivation ───────────────────────────────────────────

    /// One extracted use-site record. The HIR visitor cannot retain borrowed
    /// expression references, so the query layer keeps only the fields a
    /// place query needs: identity, type, span, and the source binding.
    fn place_site(&self, hir_id: HirId) -> Option<PlaceSite> {
        let mut memo = self.memo.borrow_mut();
        if memo.sites.is_none() {
            let mut map = FxHashMap::default();
            let mut collector = PlaceSiteCollector { map: &mut map };
            if let Some(entry) = self.hir.entry.as_ref() {
                collector.visit_block(entry);
            }
            for item in &self.hir.items {
                match &item.kind {
                    HirItemKind::Function(function) => {
                        if let Some(body) = function.body.as_ref() {
                            collector.visit_block(body);
                        }
                    }
                    HirItemKind::Struct(strukt) => {
                        for method in &strukt.methods {
                            if let Some(body) = method.func.body.as_ref() {
                                collector.visit_block(body);
                            }
                        }
                    }
                    _ => {}
                }
            }
            memo.sites = Some(map);
        }
        memo.sites.as_ref()?.get(&hir_id).copied()
    }

    fn derive_place(&self, hir_id: HirId, kind: PlaceCapabilityKind) -> PlaceCapabilityAnswer {
        use PlaceCapabilityProof as Proof;

        let Some(site) = self.place_site(hir_id) else {
            return CapabilityAnswer::Rejected(CapabilityReason::MissingFact);
        };
        if let Some(ty) = site.ty
            && let Err(reason) = self.classify_only(ty, &mut Vec::new())
        {
            return CapabilityAnswer::Rejected(reason);
        }
        // Place/value capability is keyed to a source-binding use; other
        // expression shapes have no binding to query.
        let Some(source) = site.source else {
            return CapabilityAnswer::Rejected(CapabilityReason::MissingFact);
        };
        let dead_events: Vec<_> = self.access_facts.dead_sources_of(source).collect();
        let dead_here = dead_events.iter().any(|fact| fact.site == Some(hir_id));
        let killed_before = dead_events
            .iter()
            .any(|fact| fact.site_span.end <= site.span.start);
        let marker = site.ty.and_then(|ty| self.types.peel_access_marker(ty).0);

        match kind {
            PlaceCapabilityKind::Readable => {
                if killed_before && !dead_here {
                    // The recorded kill makes readability unprovable here.
                    CapabilityAnswer::Rejected(CapabilityReason::MissingFact)
                } else {
                    CapabilityAnswer::Proven(Proof::Readable)
                }
            }
            PlaceCapabilityKind::Writable | PlaceCapabilityKind::Mutable => {
                if marker == Some(AccessMarker::Mut) {
                    let proof = match kind {
                        PlaceCapabilityKind::Writable => Proof::Writable,
                        _ => Proof::Mutable,
                    };
                    CapabilityAnswer::Proven(proof)
                } else {
                    CapabilityAnswer::Rejected(CapabilityReason::UnsupportedRepresentation)
                }
            }
            PlaceCapabilityKind::Movable => {
                if dead_here {
                    // The own-transfer event at this site proves the move.
                    CapabilityAnswer::Proven(Proof::Movable)
                } else if killed_before {
                    CapabilityAnswer::Rejected(CapabilityReason::MissingFact)
                } else if matches!(marker, Some(AccessMarker::Ref | AccessMarker::Mut)) {
                    CapabilityAnswer::Rejected(CapabilityReason::UnsupportedRepresentation)
                } else {
                    CapabilityAnswer::Proven(Proof::Movable)
                }
            }
            PlaceCapabilityKind::SharedBorrowable => {
                if matches!(
                    marker,
                    Some(AccessMarker::Mut | AccessMarker::Own | AccessMarker::Copy)
                ) {
                    CapabilityAnswer::Rejected(CapabilityReason::UnsupportedRepresentation)
                } else if killed_before {
                    CapabilityAnswer::Rejected(CapabilityReason::MissingFact)
                } else {
                    CapabilityAnswer::Proven(Proof::SharedBorrowable)
                }
            }
            PlaceCapabilityKind::MutablyBorrowable => {
                if marker == Some(AccessMarker::Mut) && !killed_before {
                    CapabilityAnswer::Proven(Proof::MutablyBorrowable)
                } else {
                    CapabilityAnswer::Rejected(CapabilityReason::UnsupportedRepresentation)
                }
            }
            PlaceCapabilityKind::DuplicateThenOwn => {
                if marker == Some(AccessMarker::Copy) {
                    CapabilityAnswer::Proven(Proof::DuplicateThenOwn)
                } else if let Some(ty) = site.ty {
                    match self.type_capability(ty, TypeCapabilityKind::TriviallyDuplicable) {
                        CapabilityAnswer::Proven(_) => {
                            CapabilityAnswer::Proven(Proof::DuplicateThenOwn)
                        }
                        CapabilityAnswer::Rejected(reason) => CapabilityAnswer::Rejected(reason),
                    }
                } else {
                    CapabilityAnswer::Rejected(CapabilityReason::MissingFact)
                }
            }
            PlaceCapabilityKind::LivenessAtUse => {
                if dead_here {
                    CapabilityAnswer::Proven(Proof::Liveness(Liveness::Dead))
                } else if dead_events.is_empty() {
                    // No recorded kill anywhere: the source stays live.
                    CapabilityAnswer::Proven(Proof::Liveness(Liveness::Live))
                } else {
                    // A kill exists elsewhere (other path or scope end); this
                    // use's liveness is not provable from the event table.
                    CapabilityAnswer::Rejected(CapabilityReason::MissingFact)
                }
            }
        }
    }

    // ── function-boundary derivation ─────────────────────────────────────

    fn function_decl(&self, def_id: DefId) -> Option<&'a HirFunction> {
        let mut memo = self.memo.borrow_mut();
        if memo.functions.is_none() {
            let mut map = FxHashMap::default();
            for item in &self.hir.items {
                match &item.kind {
                    HirItemKind::Function(function) => {
                        map.insert(item.def_id, function.as_ref());
                    }
                    HirItemKind::Struct(strukt) => {
                        for method in &strukt.methods {
                            map.insert(method.def_id, &method.func);
                        }
                    }
                    _ => {}
                }
            }
            memo.functions = Some(map);
        }
        memo.functions.as_ref()?.get(&def_id).copied()
    }

    fn derive_function(
        &self,
        def_id: DefId,
        kind: FunctionCapabilityKind,
    ) -> FunctionCapabilityAnswer {
        use FunctionCapabilityProof as Proof;

        if kind == FunctionCapabilityKind::ReturnedBorrowProvenance
            || kind == FunctionCapabilityKind::RequiredRegionRelationships
        {
            // SCR-04: the durable provenance summary is the sole authority
            // for the region rows; live region facts are never re-derived
            // here and a missing table stays MissingFact.
            let Some(table) = self.provenance else {
                return CapabilityAnswer::Rejected(CapabilityReason::MissingFact);
            };
            let Some(row) = table.get(def_id) else {
                return CapabilityAnswer::Rejected(CapabilityReason::MissingFact);
            };
            return match kind {
                FunctionCapabilityKind::ReturnedBorrowProvenance => {
                    CapabilityAnswer::Proven(Proof::ReturnedBorrow(row.returned_borrow))
                }
                _ => {
                    CapabilityAnswer::Proven(Proof::RequiredRegions(RequiredRegionRelationships {
                        relationships: row.required_regions.clone(),
                    }))
                }
            };
        }
        let Some(fact) = self.function_facts.get(def_id) else {
            return CapabilityAnswer::Rejected(CapabilityReason::MissingFact);
        };
        match kind {
            FunctionCapabilityKind::ParameterAccessPosture => {
                let Some(function) = self.function_decl(def_id) else {
                    return CapabilityAnswer::Rejected(CapabilityReason::MissingFact);
                };
                let mut out = Vec::with_capacity(function.params.len());
                for param in &function.params {
                    if let Err(reason) = self.classify_only(param.ty, &mut Vec::new()) {
                        return CapabilityAnswer::Rejected(reason);
                    }
                    let Some(mode) = fact.param_modes.get(&param.def_id).copied() else {
                        return CapabilityAnswer::Rejected(CapabilityReason::MissingFact);
                    };
                    out.push(ParameterAccess {
                        parameter: param.def_id,
                        posture: match mode {
                            SemanticParamMode::Owned => ParameterAccessPosture::Owned,
                            SemanticParamMode::Ref => ParameterAccessPosture::Shared,
                            SemanticParamMode::MutRef => ParameterAccessPosture::Mutable,
                            SemanticParamMode::Move => ParameterAccessPosture::Move,
                        },
                    });
                }
                CapabilityAnswer::Proven(Proof::ParameterAccess(out))
            }
            FunctionCapabilityKind::CapturePosture => {
                let captures = fact
                    .captures
                    .iter()
                    .map(|capture| CaptureRequirement {
                        definition: capture.def_id,
                        posture: match capture.mode {
                            CaptureMode::Read => CapturePosture::Shared,
                            CaptureMode::Mutate => CapturePosture::Mutable,
                            CaptureMode::Consume => CapturePosture::Consuming,
                        },
                    })
                    .collect();
                CapabilityAnswer::Proven(Proof::Capture(captures))
            }
            FunctionCapabilityKind::EffectsAndAlternateExit => {
                CapabilityAnswer::Proven(Proof::Effects(EffectPosture {
                    may_fail: fact.may_fail,
                    may_suspend: fact.is_async || fact.requires_await,
                    is_generator: fact.is_generator,
                    has_alternate_exit: fact.err_ty.is_some(),
                }))
            }
            FunctionCapabilityKind::ExportedVisibility => match self.visibility.get(&def_id) {
                Some(VisibilityTier::Publica) => {
                    CapabilityAnswer::Proven(Proof::ExportedVisibility(ExportVisibility::Public))
                }
                Some(VisibilityTier::Interna) => {
                    CapabilityAnswer::Proven(Proof::ExportedVisibility(ExportVisibility::Package))
                }
                Some(VisibilityTier::Privata) => {
                    CapabilityAnswer::Proven(Proof::ExportedVisibility(ExportVisibility::Module))
                }
                None => CapabilityAnswer::Rejected(CapabilityReason::MissingFact),
            },
            // Handled by the SCR-04 guard above; fail closed if ever reached.
            FunctionCapabilityKind::ReturnedBorrowProvenance
            | FunctionCapabilityKind::RequiredRegionRelationships => {
                CapabilityAnswer::Rejected(CapabilityReason::MissingFact)
            }
        }
    }
}

/// One extracted use-site record for place queries.
#[derive(Debug, Clone, Copy)]
struct PlaceSite {
    ty: Option<TypeId>,
    span: radix_lexer::Span,
    source: Option<DefId>,
}

struct PlaceSiteCollector<'m> {
    map: &'m mut FxHashMap<HirId, PlaceSite>,
}

impl HirVisitor for PlaceSiteCollector<'_> {
    fn visit_expr(&mut self, expr: &radix_hir::HirExpression) {
        self.map.insert(
            expr.id,
            PlaceSite {
                ty: expr.ty,
                span: expr.span,
                source: match expr.kind {
                    HirExpressionKind::Path(def_id) => Some(def_id),
                    _ => None,
                },
            },
        );
        walk_expr(self, expr);
    }
}

impl TypeCapabilityQueries for DerivedCapabilityQueries<'_> {
    fn type_capability(&self, type_id: TypeId, kind: TypeCapabilityKind) -> TypeCapabilityAnswer {
        if let Some(answer) = self.memo.borrow().type_answers.get(&(type_id, kind)) {
            return answer.clone();
        }
        let answer = self.derive_type(type_id, kind);
        self.memo
            .borrow_mut()
            .type_answers
            .insert((type_id, kind), answer.clone());
        answer
    }
}

impl IndexCapabilityQueries for DerivedCapabilityQueries<'_> {
    fn index_capability(
        &self,
        index_id: IndexId,
        _kind: IndexCapabilityKind,
    ) -> IndexCapabilityAnswer {
        if let Some(answer) = self.memo.borrow().index_answers.get(&index_id) {
            return answer.clone();
        }
        let answer = self.derive_index(index_id);
        self.memo
            .borrow_mut()
            .index_answers
            .insert(index_id, answer.clone());
        answer
    }
}

impl PlaceCapabilityQueries for DerivedCapabilityQueries<'_> {
    fn place_capability(&self, hir_id: HirId, kind: PlaceCapabilityKind) -> PlaceCapabilityAnswer {
        if let Some(answer) = self.memo.borrow().place_answers.get(&(hir_id, kind)) {
            return answer.clone();
        }
        let answer = self.derive_place(hir_id, kind);
        self.memo
            .borrow_mut()
            .place_answers
            .insert((hir_id, kind), answer.clone());
        answer
    }
}

impl FunctionCapabilityQueries for DerivedCapabilityQueries<'_> {
    fn function_capability(
        &self,
        def_id: DefId,
        kind: FunctionCapabilityKind,
    ) -> FunctionCapabilityAnswer {
        if let Some(answer) = self.memo.borrow().function_answers.get(&(def_id, kind)) {
            return answer.clone();
        }
        let answer = self.derive_function(def_id, kind);
        self.memo
            .borrow_mut()
            .function_answers
            .insert((def_id, kind), answer.clone());
        answer
    }
}
