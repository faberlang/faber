//! Integer interval facts for the typecheck flow machinery (need,
//! slices 1-2).
//!
//! A flow fact value ([`FactValue`]) can carry a narrowed type (null/member
//! narrowing, unchanged) and/or an integer interval ([`IntRange`]). Nothing in
//! the checker produces interval facts yet; this module owns the data model,
//! the conservative join, and the invalidation rules that later producers and
//! consumers rely on.
//!
//! BOUNDS
//! ======
//! An interval endpoint is symbolic ([`BoundKey`]) with a strictness flag: a
//! literal, a resolved tensor-axis extent ([`AxisIdentity`], built from the
//! resolved `IndexExpr`, never from rendered text), the live length of a
//! list/text source at a mutation version, or the value of an immutable
//! binding. Two bounds are comparable only when both are literals or both name
//! the identical symbolic key; anything else is incomparable and the join
//! drops that side rather than guess.
//!
//! INVALIDATION (version-1 closed rule, evaluator amendment Q4)
//! ===========================================================
//! Any call, mutating intrinsic, closure call, or store through an alias or
//! projection kills every fact about a mutable source: facts whose subject
//! binding is mutable, and facts with a live-length endpoint on a mutable
//! source. The kill also moves the source's mutation version, so a stale
//! live-length identity can never compare equal to a fresh one. Facts on `let`
//! bindings, captured loop endpoints and fixed tensor extents survive.
//!
//! `FactKey::Field` joins are not built, so interval facts are keyed by binding
//! only; a range value attached to a field-path key is ignored.

// Slice 3 adds the producers; until then only the tests construct bounds, facts
// and queries, so a non-test build sees them as unused.
#![cfg_attr(not(test), allow(dead_code))]

use std::cmp::Ordering;

use handroll::FxHashMap;
use radix_hir::DefId;
use radix_lexer::Symbol;

use crate::{IndexExpr, IndexId, IndexInferState, RuntimeExtentId, TypeId, TypeTable};

/// A symbolic tensor-axis extent, built structurally from a resolved
/// `IndexExpr`. Equality is the precedent in `tensor_index.rs`
/// (`dimensions_provably_equal`): the same size parameter or the same runtime
/// extent identity, extended structurally through affine/divide terms because
/// index ids are not hash-consed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum AxisIdentity {
    Param(Symbol),
    Runtime(RuntimeExtentId),
    Affine {
        base: Box<AxisIdentity>,
        offset: i128,
    },
    Divide {
        numerator: Box<AxisIdentity>,
        divisor: u64,
    },
}

/// The identity of one interval endpoint.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum BoundKey {
    /// A compile-time integer.
    Const(i128),
    /// A non-literal tensor-axis extent.
    Axis(AxisIdentity),
    /// The live length of a list/text source, valid only while the source's
    /// mutation version equals `version`. A length read after a kill has a
    /// different version, so it never equals a stale bound.
    LiveLength { source: DefId, version: u32 },
    /// The value of an immutable binding (a captured loop endpoint or `let`).
    /// Mutable bindings never get a key.
    Binding(DefId),
}

impl BoundKey {
    /// Order two bounds when both are literals or they are the identical
    /// symbolic key; `None` otherwise.
    fn compare(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Const(a), Self::Const(b)) => Some(a.cmp(b)),
            _ if self == other => Some(Ordering::Equal),
            _ => None,
        }
    }

    fn live_length_source(&self) -> Option<(DefId, u32)> {
        match self {
            Self::LiveLength { source, version } => Some((*source, *version)),
            _ => None,
        }
    }
}

/// One interval endpoint: `value ≥ key` (or `value > key` when strict) for a
/// lower endpoint, `value ≤ key` (or `<`) for an upper one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Endpoint {
    pub(super) key: BoundKey,
    pub(super) strict: bool,
}

impl Endpoint {
    pub(super) fn new(key: BoundKey, strict: bool) -> Self {
        Self { key, strict }
    }
}

/// An integer interval with optional symbolic endpoints. An absent endpoint is
/// unbounded on that side.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct IntRange {
    pub(super) lo: Option<Endpoint>,
    pub(super) hi: Option<Endpoint>,
}

impl IntRange {
    pub(super) fn from_lo(key: BoundKey, strict: bool) -> Self {
        Self {
            lo: Some(Endpoint::new(key, strict)),
            hi: None,
        }
    }

    pub(super) fn from_hi(key: BoundKey, strict: bool) -> Self {
        Self {
            lo: None,
            hi: Some(Endpoint::new(key, strict)),
        }
    }

    pub(super) fn is_unbounded(&self) -> bool {
        self.lo.is_none() && self.hi.is_none()
    }

    /// Both facts hold (conjunction): per side keep the stronger endpoint, or
    /// the existing one when the two are incomparable (either is true, so
    /// keeping one is sound).
    pub(super) fn meet(&self, other: &Self) -> Self {
        Self {
            lo: meet_side(self.lo.as_ref(), other.lo.as_ref(), Ordering::Greater),
            hi: meet_side(self.hi.as_ref(), other.hi.as_ref(), Ordering::Less),
        }
    }

    /// The conservative hull across two normal edges: per side keep the weaker
    /// endpoint; a side whose endpoints cannot be compared is dropped. `None`
    /// when nothing useful remains.
    pub(super) fn hull(&self, other: &Self) -> Option<Self> {
        let joined = Self {
            lo: hull_side(self.lo.as_ref(), other.lo.as_ref(), Ordering::Less),
            hi: hull_side(self.hi.as_ref(), other.hi.as_ref(), Ordering::Greater),
        };
        (!joined.is_unbounded()).then_some(joined)
    }

    /// Whether this interval proves `value ≥ key` (`>` when `strict`).
    pub(super) fn proves_at_least(&self, key: &BoundKey, strict: bool) -> bool {
        let Some(lo) = &self.lo else { return false };
        match lo.key.compare(key) {
            Some(Ordering::Greater) => true,
            Some(Ordering::Equal) => lo.strict || !strict,
            _ => false,
        }
    }

    /// Whether this interval proves `value ≤ key` (`<` when `strict`).
    pub(super) fn proves_at_most(&self, key: &BoundKey, strict: bool) -> bool {
        let Some(hi) = &self.hi else { return false };
        match hi.key.compare(key) {
            Some(Ordering::Less) => true,
            Some(Ordering::Equal) => hi.strict || !strict,
            _ => false,
        }
    }

    fn endpoints(&self) -> impl Iterator<Item = &Endpoint> {
        self.lo.iter().chain(self.hi.iter())
    }

    /// Every `(source, version)` live-length identity an endpoint mentions.
    fn live_length_sources(&self) -> impl Iterator<Item = (DefId, u32)> + '_ {
        self.endpoints()
            .filter_map(|endpoint| endpoint.key.live_length_source())
    }
}

/// `want` is `Greater` when the stronger endpoint is the larger one (lower
/// bounds) and `Less` when it is the smaller one (upper bounds).
fn meet_side(a: Option<&Endpoint>, b: Option<&Endpoint>, want: Ordering) -> Option<Endpoint> {
    match (a, b) {
        (Some(a), Some(b)) => Some(match a.key.compare(&b.key) {
            Some(Ordering::Equal) => Endpoint::new(a.key.clone(), a.strict || b.strict),
            Some(order) if order == want => a.clone(),
            Some(_) => b.clone(),
            None => a.clone(),
        }),
        (Some(only), None) | (None, Some(only)) => Some(only.clone()),
        (None, None) => None,
    }
}

/// `weaker` is `Less` for lower bounds (the smaller is weaker) and `Greater`
/// for upper bounds.
fn hull_side(a: Option<&Endpoint>, b: Option<&Endpoint>, weaker: Ordering) -> Option<Endpoint> {
    let (a, b) = (a?, b?);
    match a.key.compare(&b.key)? {
        Ordering::Equal => Some(Endpoint::new(a.key.clone(), a.strict && b.strict)),
        order if order == weaker => Some(a.clone()),
        _ => Some(b.clone()),
    }
}

/// What a condition (or other fact producer) establishes about one subject: a
/// narrowed type, an integer interval, or both.
#[derive(Clone, Debug, Default)]
pub(super) struct FactValue {
    pub(super) ty: Option<TypeId>,
    pub(super) range: Option<IntRange>,
}

impl FactValue {
    pub(super) fn ty(ty: TypeId) -> Self {
        Self {
            ty: Some(ty),
            range: None,
        }
    }

    // No producer yet (slice 3); the control-flow tests drive it.
    #[allow(dead_code)]
    pub(super) fn range(range: IntRange) -> Self {
        Self {
            ty: None,
            range: Some(range),
        }
    }
}

/// The interval facts and mutation versions at one control-flow edge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct RangeSnapshot {
    facts: FxHashMap<DefId, IntRange>,
    versions: FxHashMap<DefId, u32>,
}

impl RangeSnapshot {
    /// Drop facts naming a live-length identity older than its source's
    /// current version.
    fn prune_stale(&mut self) {
        let versions = &self.versions;
        self.facts.retain(|_, range| {
            range
                .live_length_sources()
                .all(|(source, version)| versions.get(&source).copied().unwrap_or(0) == version)
        });
    }
}

/// The value a fact had before [`RangeFlow::assume`], for [`RangeFlow::retract`].
pub(super) struct RangeUndo {
    prior: Option<IntRange>,
    applied: IntRange,
}

/// The live interval facts, owned by the checker next to the type facts.
#[derive(Debug, Default)]
pub(super) struct RangeFlow {
    now: RangeSnapshot,
    /// Source of fresh mutation versions. Deliberately outside
    /// [`RangeSnapshot`]: a restore never rewinds it, so a version minted on
    /// one edge is never reused on another.
    next_version: u32,
}

impl RangeFlow {
    pub(super) fn snapshot(&self) -> RangeSnapshot {
        self.now.clone()
    }

    pub(super) fn restore(&mut self, snapshot: &RangeSnapshot) {
        self.now.clone_from(snapshot);
    }

    pub(super) fn get(&self, subject: DefId) -> Option<&IntRange> {
        self.now.facts.get(&subject)
    }

    fn version_of(&self, source: DefId) -> u32 {
        self.now.versions.get(&source).copied().unwrap_or(0)
    }

    /// The canonical live-length identity of `source` right now.
    pub(super) fn live_length(&self, source: DefId) -> BoundKey {
        BoundKey::LiveLength {
            source,
            version: self.version_of(source),
        }
    }

    /// Conjoin `range` into the subject's fact.
    pub(super) fn assume(&mut self, subject: DefId, range: &IntRange) -> RangeUndo {
        let prior = self.now.facts.get(&subject).cloned();
        let applied = prior
            .as_ref()
            .map_or_else(|| range.clone(), |prior| prior.meet(range));
        self.now.facts.insert(subject, applied.clone());
        RangeUndo { prior, applied }
    }

    /// Take back an [`Self::assume`]. A fact a kill or store changed in the
    /// meantime is left as it is: restoring the prior value over it could
    /// resurrect a stale fact.
    pub(super) fn retract(&mut self, subject: DefId, undo: RangeUndo) {
        if self.now.facts.get(&subject) != Some(&undo.applied) {
            return;
        }
        match undo.prior {
            Some(prior) => self.now.facts.insert(subject, prior),
            None => self.now.facts.remove(&subject),
        };
    }

    fn fresh_version(&mut self) -> u32 {
        self.next_version += 1;
        self.next_version
    }

    /// A binding was stored to: its own interval dies, its live length is a new
    /// identity, and every fact bounded by the old length dies with it.
    pub(super) fn drop_subject(&mut self, subject: DefId) {
        self.now.facts.remove(&subject);
        if self.now.facts.values().any(|range| {
            range
                .live_length_sources()
                .any(|(source, _)| source == subject)
        }) {
            let version = self.fresh_version();
            self.now.versions.insert(subject, version);
            self.prune_stale();
        }
    }

    /// Kill every fact about a mutable source (the version-1 closed rule):
    /// facts whose subject is mutable, and facts bounded by the live length of
    /// a mutable source. Every killed source gets a fresh version.
    pub(super) fn kill_mutable_sources(&mut self, is_mutable: impl Fn(DefId) -> bool) {
        let mut moved: Vec<DefId> = Vec::new();
        self.now.facts.retain(|subject, range| {
            let mut live = !is_mutable(*subject);
            for (source, _) in range.live_length_sources() {
                if is_mutable(source) {
                    live = false;
                    moved.push(source);
                }
            }
            if !live {
                moved.push(*subject);
            }
            live
        });
        moved.sort_by_key(|def| def.0);
        moved.dedup();
        for source in moved {
            let version = self.fresh_version();
            self.now.versions.insert(source, version);
        }
        self.prune_stale();
    }

    fn prune_stale(&mut self) {
        self.now.prune_stale();
    }

    /// The join over the normal edges that reach a merge point.
    ///
    /// A fact survives only when every edge carries it; its endpoints take the
    /// conservative hull and an incomparable side is dropped. A source whose
    /// version differs between edges gets a fresh one, which prunes every fact
    /// bounded by its old live length. No edge at all joins to `incoming`.
    pub(super) fn join(
        &mut self,
        incoming: &RangeSnapshot,
        edges: &[&RangeSnapshot],
    ) -> RangeSnapshot {
        let Some((first, rest)) = edges.split_first() else {
            return incoming.clone();
        };
        let mut sources: Vec<DefId> = edges
            .iter()
            .flat_map(|edge| edge.versions.keys().copied())
            .collect();
        sources.sort_by_key(|def| def.0);
        sources.dedup();
        let mut versions = FxHashMap::default();
        for source in sources {
            let version = first.versions.get(&source).copied().unwrap_or(0);
            let agree = rest
                .iter()
                .all(|edge| edge.versions.get(&source).copied().unwrap_or(0) == version);
            versions.insert(source, if agree { version } else { self.fresh_version() });
        }
        let mut facts = FxHashMap::default();
        for (subject, range) in &first.facts {
            let joined = rest.iter().try_fold(range.clone(), |acc, edge| {
                edge.facts.get(subject).and_then(|other| acc.hull(other))
            });
            if let Some(joined) = joined {
                facts.insert(*subject, joined);
            }
        }
        let mut joined = RangeSnapshot { facts, versions };
        joined.prune_stale();
        joined
    }
}

/// The canonical bound for a tensor axis: `Const` when the resolved extent is
/// a literal, otherwise a structural [`AxisIdentity`]. `None` for an
/// unresolved hole, a tuple shape or the unspecified marker: those prove
/// nothing.
pub(super) fn axis_bound(
    types: &mut TypeTable,
    infer: &mut IndexInferState,
    index: IndexId,
) -> Option<BoundKey> {
    let canonical = infer.canonicalize(types, index);
    match axis_term(types, canonical)? {
        AxisTerm::Const(value) => Some(BoundKey::Const(value)),
        AxisTerm::Symbolic(identity) => Some(BoundKey::Axis(identity)),
    }
}

enum AxisTerm {
    Const(i128),
    Symbolic(AxisIdentity),
}

fn axis_term(types: &TypeTable, id: IndexId) -> Option<AxisTerm> {
    match types.get_index(id) {
        IndexExpr::Literal(value) => Some(AxisTerm::Const(i128::from(*value))),
        IndexExpr::Param(name) => Some(AxisTerm::Symbolic(AxisIdentity::Param(*name))),
        IndexExpr::Runtime(origin) => {
            Some(AxisTerm::Symbolic(AxisIdentity::Runtime(origin.clone())))
        }
        IndexExpr::Affine { base, offset } => match axis_term(types, *base)? {
            AxisTerm::Const(value) => {
                let sum = value.checked_add(*offset)?;
                (sum >= 0).then_some(AxisTerm::Const(sum))
            }
            AxisTerm::Symbolic(base) => Some(AxisTerm::Symbolic(AxisIdentity::Affine {
                base: Box::new(base),
                offset: *offset,
            })),
        },
        IndexExpr::Divide { numerator, divisor } => match axis_term(types, *numerator)? {
            AxisTerm::Const(value) => {
                let divisor = i128::from(*divisor);
                (divisor > 0 && value % divisor == 0).then_some(AxisTerm::Const(value / divisor))
            }
            AxisTerm::Symbolic(numerator) => Some(AxisTerm::Symbolic(AxisIdentity::Divide {
                numerator: Box::new(numerator),
                divisor: *divisor,
            })),
        },
        IndexExpr::Tuple(_) | IndexExpr::Infer(_) | IndexExpr::Unspecified => None,
    }
}
