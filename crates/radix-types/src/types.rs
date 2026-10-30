//! Semantic type model and shared type table.
//!
//! This module defines the compiler's semantic type vocabulary after parsing
//! has turned source type syntax into compiler data. The [`TypeTable`] is the
//! shared store passed through collection, resolution, lowering, typecheck, and
//! later analyses so every `TypeId` in a semantic result points into one table.
//!
//! TYPE TABLE ROLE
//! ===============
//! `TypeId` is an arena index into one [`TypeTable`]. Within a table, `intern`
//! hash-conses on derived `PartialEq` so structurally identical shapes share one
//! id. Semantic [`TypeTable::equals`] still peels aliases and applies numeric
//! coercion, so matching ids are sufficient but not necessary for `type_equal`.
//!
//! NULLABILITY AND ESCAPES
//! =======================
//! Nullable value types are represented as `Option(T)` or `Union([... Nihil])`
//! depending on the source construct and lowering path. `ignotum` is separate:
//! it is an escape hatch for unknown values, not the nullable marker.
//!
//! ASSIGNABILITY
//! =============
//! Assignment is intentionally more permissive than equality. It allows
//! numerus-to-fractus widening, values flowing into optional destinations, union
//! membership checks, and values flowing into `ignotum`. It does not allow
//! `ignotum` to flow out into a concrete type without an explicit narrowing or
//! cast elsewhere.
//!
//! UNION MEMBER DEDUP
//! ==================
//! Union lowering at alias-resolve and HIR annotation sites deduplicates members
//! with [`TypeTable::equals`], not `TypeId` identity. Whether a nullable union
//! collapses to `Option(T)` vs `Union([...])` is decided at those lowering
//! sites; MIR and backends treat that boundary as an upstream invariant.

use crate::DefId;
use crate::index::{IndexExpr, IndexId};
use crate::instans_precision::{InstansPrecision, is_instans_assignable};
use crate::numeric_width::{BitsWidth, NumericWidth, is_numeric_assignable};
use handroll::{FxHashMap, FxHashSet};
use radix_lexer::Symbol as LexSymbol;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};

/// Arena index into a [`TypeTable`].
///
/// A `TypeId` is meaningful only with the table that created it. Do not compare
/// IDs from different tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeId(pub u32);

/// Per-analysis storage for semantic types.
///
/// The table preloads primitives so passes can cheaply request well-known
/// built-ins, then appends compound and inferred types as semantic information
/// becomes available. It deliberately exposes structural [`Self::equals`] and
/// [`Self::is_assignable`] helpers so callers do not need to know whether two
/// equivalent shapes were allocated at the same index.
#[derive(Clone)]
pub struct TypeTable {
    types: Vec<Type>,
    /// Content-addressed index for O(1) [`Self::intern`] lookup (hash-cons).
    /// Key is the structural [`Type`] (derived `PartialEq` / content hash).
    intern_map: FxHashMap<Type, TypeId>,
    indices: Vec<IndexExpr>,
    primitives: FxHashMap<Primitive, TypeId>,
    unspecified_shape: Option<IndexId>,
    /// Memoized `IndexExpr::Literal(1)` index used as the default interval step.
    /// Keeps `intervallum<T>` on one interned step so repeated calls hash-cons.
    interval_step_one: Option<IndexId>,
    /// Monotonic source for [`IndexExpr::Infer`] vars across lowering passes.
    next_index_var: u32,
    /// Memo for [`Self::equals`] results by ordered `(TypeId, TypeId)` pair.
    /// Interior-mutable: `equals` takes `&self` and is pure over existing entries.
    equals_memo: RefCell<FxHashMap<(TypeId, TypeId), bool>>,
    /// Memo for [`Self::is_assignable`] results by directional `(from, to)` pair.
    ///
    /// The key is NOT canonicalized to a `(min, max)` order like
    /// [`Self::equals_memo`]: `is_assignable` is directional (widening, optional,
    /// and `ignotum`-sink rules are one-way), so `is_assignable(a, b)` and
    /// `is_assignable(b, a)` are distinct queries and must not share a memo entry.
    /// Interior-mutable: `is_assignable` takes `&self` and is pure over existing
    /// entries (the arena is append-only, so results cannot go stale).
    assignable_memo: RefCell<FxHashMap<(TypeId, TypeId), bool>>,
    /// Memo for [`Self::find_equal`] results by candidate [`Type`].
    ///
    /// Caches **positive** results only: once a candidate resolves to an arena
    /// entry, the append-only arena guarantees that match can never go stale.
    /// Misses are not cached — a later [`Self::intern`] may append a matching
    /// type, so a stale negative would be unsound. Cached ids are the first
    /// *semantic* match the linear scan returns (never an `intern_map` id), so
    /// callers render the same representative as the pre-memo scan.
    find_equal_memo: RefCell<FxHashMap<Type, TypeId>>,
    /// First interned [`Type::ModularWord`] width, if any.
    ///
    /// Updated on every `intern` that inserts a `ModularWord` when unset. Snapshot
    /// restore recomputes from the types arena so the flag stays consistent.
    first_modular_word: Option<NumericWidth>,
    /// Raw-bits widths interned through [`Self::intern`] (bit 0 `b8` .. bit 3
    /// `b64`). The `lista<b8>` the `octeti` word names is seeded without
    /// setting a bit, so a module that never writes a `bN` marker has none.
    authored_bits: u8,
    /// Inferred holes left without a type slot by a bare `{ field = value }`
    /// literal (operator ruling 2026-10-14).
    ///
    /// The parser marks the literal's `TypeFormLevel` so the checker can tell
    /// it from an explicit `_ { … }` hole: the bare literal defaults to a
    /// record shaped by its fields, the explicit hole does not. Transient
    /// per-analysis fact; never serialized into a [`TypeTableSnapshot`].
    default_record_holes: FxHashSet<u32>,

    /// Parent enum definition for each variant constructor [`DefId`].
    ///
    /// Populated during typecheck so exhaustiveness and return-path passes can
    /// resolve imported enums whose variants are not declared in the local HIR.
    pub variant_parent: FxHashMap<DefId, DefId>,

    /// Complete resolved generic vectors for checked generic calls, in call
    /// checking order.
    ///
    /// Populated during typecheck (see [`ResolvedGenericCall`]) so consumers
    /// read the binding result instead of re-running inference.
    pub resolved_generic_calls: Vec<ResolvedGenericCall>,

    /// The implendum marked `@ radix contract "ordering"` (norma's
    /// `Orderable`, DECISIONS D1.4) when this unit declares or imports it.
    ///
    /// Populated during typecheck so MIR lowering and backends recognize
    /// ordering bounds without re-reading annotations.
    pub ordering_contract: Option<DefId>,

    /// Selective import bindings that name the ordering contract. The
    /// compiler supplies that contract, so backends emit no import for them.
    pub ordering_contract_bindings: handroll::FxHashSet<DefId>,

    /// The implendum marked `@ radix contract "closing"` (norma's
    /// `Closable`, DECISIONS D8.3) when this unit declares or imports it.
    /// `sermo` conforms without an `implet` clause.
    pub closing_contract: Option<DefId>,

    /// The implendum marked `@ radix contract "sinking"` (norma's `Sink`, need
    /// 0c4d3dc1) when this unit declares or imports it.
    pub sinking_contract: Option<DefId>,

    /// Selective import bindings that name the sinking contract. The compiler
    /// supplies that contract, so backends emit no import for them.
    pub sinking_contract_bindings: handroll::FxHashSet<DefId>,
}

/// Opaque serialized view of a [`TypeTable`] for MIR images.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeTableSnapshot {
    types: Vec<Type>,
    indices: Vec<IndexExpr>,
    unspecified_shape: Option<IndexId>,
    next_index_var: u32,
    /// Raw-bits widths the source authored (bit 0 `b8` .. bit 3 `b64`). The
    /// arena alone cannot tell an authored `b8` from the seeded `octeti` byte
    /// list, so the fact travels beside it.
    authored_bits: u8,
}

impl TypeTableSnapshot {
    #[must_use]
    pub fn type_count(&self) -> usize {
        self.types.len()
    }

    /// Read-only access to the type arena for referential integrity walks.
    #[must_use]
    pub fn types(&self) -> &[Type] {
        &self.types
    }

    /// Read-only access to the index-expression arena for referential integrity walks.
    #[must_use]
    pub fn indices(&self) -> &[IndexExpr] {
        &self.indices
    }

    /// Re-intern `Type::Record` field-name symbols through `remap`,
    /// slot-in-place.
    ///
    /// Record field identities live in this arena (not in a side metadata
    /// map), so a cross-interner merge remaps them here: every `TypeId`
    /// keeps its slot, and a table rebuilt from the mutated snapshot
    /// resolves every pre-remap `TypeId` to the same (re-interned) type.
    /// `remap` decides per symbol; returning the input unchanged (the
    /// generated-identity-token guard) leaves the symbol untouched.
    pub fn remap_record_field_symbols(&mut self, mut remap: impl FnMut(LexSymbol) -> LexSymbol) {
        for ty in &mut self.types {
            if let Type::Record(fields) = ty {
                *fields = std::mem::take(fields)
                    .into_iter()
                    .map(|(name, field_ty)| (remap(name), field_ty))
                    .collect();
            }
        }
    }
}

/// Primitives every table seeds in [`TypeTable::new`] and requires when
/// rebuilding from a snapshot.
///
/// Order is the seeding order; primitive `TypeId`s are assigned by first
/// intern, so it also fixes their ids in a freshly built table.
const REQUIRED_PRIMITIVES: [Primitive; 16] = [
    Primitive::Textus,
    Primitive::Numerus,
    Primitive::Fractus,
    Primitive::Bivalens,
    Primitive::Nihil,
    Primitive::Vacuum,
    Primitive::Numquam,
    Primitive::Ignotum,
    Primitive::Octeti,
    Primitive::Regex,
    Primitive::Match,
    Primitive::Json,
    Primitive::Valor,
    Primitive::Ascii,
    Primitive::Littera,
    Primitive::Instans,
];

impl TypeTable {
    #[must_use]
    pub fn new() -> Self {
        let mut table = Self {
            types: Vec::new(),
            intern_map: FxHashMap::default(),
            indices: Vec::new(),
            primitives: FxHashMap::default(),
            unspecified_shape: None,
            interval_step_one: None,
            next_index_var: 0,
            equals_memo: RefCell::new(FxHashMap::default()),
            assignable_memo: RefCell::new(FxHashMap::default()),
            find_equal_memo: RefCell::new(FxHashMap::default()),
            first_modular_word: None,
            authored_bits: 0,
            default_record_holes: FxHashSet::default(),
            variant_parent: FxHashMap::default(),
            resolved_generic_calls: Vec::new(),
            ordering_contract: None,
            ordering_contract_bindings: handroll::FxHashSet::default(),
            closing_contract: None,
            sinking_contract: None,
            sinking_contract_bindings: handroll::FxHashSet::default(),
        };

        for prim in REQUIRED_PRIMITIVES {
            let id = table.intern(Type::Primitive(prim));
            table.primitives.insert(prim, id);
        }

        table
    }

    /// Store a type in this table and return its local `TypeId`.
    ///
    /// Structurally equal types canonicalize to a single `TypeId` so that
    /// representation identity implies semantic equality. Lookup is O(1) via
    /// the content-addressed [`Self::intern_map`] (hash-cons). Identity uses
    /// derived `PartialEq` (strict structural, not [`equals`](Self::equals))
    /// because alias peeling and numeric coercion must not collapse distinct
    /// representations into one entry.
    ///
    /// Union members are sorted by `TypeId` ordinal before insertion so
    /// order-insensitive unions share one arena entry.
    #[allow(clippy::cast_possible_truncation)]
    pub fn intern(&mut self, ty: Type) -> TypeId {
        if let Type::Bits(width) = &ty {
            self.authored_bits |= Self::bits_mask(*width);
        }
        self.intern_unmarked(ty)
    }

    fn bits_mask(width: BitsWidth) -> u8 {
        match width {
            BitsWidth::B8 => 1,
            BitsWidth::B16 => 2,
            BitsWidth::B32 => 4,
            BitsWidth::B64 => 8,
        }
    }

    /// The narrowest raw-bits width a module wrote through [`Self::intern`],
    /// `None` when only the `octeti` byte list (or nothing) names one. O(1)
    /// probe used by backend rejection.
    #[must_use]
    pub fn first_authored_bits(&self) -> Option<BitsWidth> {
        [
            BitsWidth::B8,
            BitsWidth::B16,
            BitsWidth::B32,
            BitsWidth::B64,
        ]
        .into_iter()
        .find(|width| self.authored_bits & Self::bits_mask(*width) != 0)
    }

    #[allow(clippy::cast_possible_truncation)]
    fn intern_unmarked(&mut self, ty: Type) -> TypeId {
        let ty = Self::canonicalize_for_intern(ty);
        if let Some(&id) = self.intern_map.get(&ty) {
            return id;
        }
        if self.first_modular_word.is_none()
            && let Type::ModularWord(width) = &ty
        {
            self.first_modular_word = Some(*width);
        }
        let id = TypeId(self.types.len() as u32);
        self.types.push(ty.clone());
        self.intern_map.insert(ty, id);
        id
    }

    /// Width of the first interned modular-word type, if any.
    ///
    /// O(1) probe used by backend rejection.
    #[must_use]
    pub fn first_modular_word_width(&self) -> Option<NumericWidth> {
        self.first_modular_word
    }

    /// First interned [`Type::PolicyFloat`] cell, if any.
    ///
    /// Linear scan used by backend gates that have no cell lowering yet
    /// (N7E-1: each fails closed by its own named issue).
    #[must_use]
    pub fn first_policy_float(&self) -> Option<(FloatPolicy, NumericWidth)> {
        self.types.iter().find_map(|ty| match ty {
            Type::PolicyFloat(policy, width) => Some((*policy, *width)),
            _ => None,
        })
    }

    /// First interned [`Type::SaturatingWord`] width, if any.
    ///
    /// Linear scan used by backend rejection (targets without a clamp lowering).
    #[must_use]
    pub fn first_saturating_word_width(&self) -> Option<NumericWidth> {
        self.types.iter().find_map(|ty| match ty {
            Type::SaturatingWord(width) => Some(*width),
            _ => None,
        })
    }

    /// Normalize a type for hash-cons insertion without changing semantic meaning.
    fn canonicalize_for_intern(ty: Type) -> Type {
        match ty {
            Type::Union(mut members) => {
                members.sort_unstable_by_key(|id| id.0);
                Type::Union(members)
            }
            Type::Intersection(mut members) => {
                members.sort_unstable_by_key(|id| id.0);
                Type::Intersection(members)
            }
            other => other,
        }
    }

    /// Find an existing interned type that is semantically equal to `candidate`.
    ///
    /// Uses [`equals`](Self::equals) for comparison so alias peeling and numeric
    /// coercion are honored. Callers that hold `&TypeTable` (immutable) and
    /// cannot call [`intern`](Self::intern) use this to locate the canonical
    /// entry for a candidate constructed from existing child `TypeId`s.
    ///
    /// Results are memoized per candidate (**positive only**): the first
    /// semantic match is cached, so repeated queries for the same shape return
    /// O(1). The cache is valid for the lifetime of the table — the arena is
    /// append-only, so a cached match can never go stale. Misses are not cached
    /// because a later [`Self::intern`] may append a matching type.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn find_equal(&self, candidate: &Type) -> Option<TypeId> {
        // Shared probe borrow is scoped to this `if let`; it is dropped before
        // any mutable borrow below, and `type_equal` never re-enters
        // `find_equal`, so there is no `RefCell` double-borrow or reentrancy.
        if let Some(&cached) = self.find_equal_memo.borrow().get(candidate) {
            return Some(cached);
        }
        // D10.4 `octeti ≡ lista<octetus>` is type identity, but the two keep
        // distinct carriers: the scan never answers a `lista` probe with the
        // byte buffer row, or the reverse.
        let found = self.types.iter().enumerate().find_map(|(index, existing)| {
            let carrier_split = matches!(
                (existing, candidate),
                (Type::Primitive(Primitive::Octeti), Type::Array(_))
                    | (Type::Array(_), Type::Primitive(Primitive::Octeti))
            );
            (!carrier_split && self.type_equal(existing, candidate)).then_some(TypeId(index as u32))
        });
        if let Some(id) = found {
            self.find_equal_memo
                .borrow_mut()
                .insert(candidate.clone(), id);
        }
        found
    }

    #[must_use]
    pub fn get(&self, id: TypeId) -> &Type {
        &self.types[id.0 as usize]
    }

    /// Mark `var` as an inferred hole that a bare `{ field = value }` literal
    /// left without a type slot. The checker reads the mark to default the
    /// hole to a record shaped by the literal's fields (operator ruling
    /// 2026-10-14).
    pub fn mark_default_record_hole(&mut self, var: InferVar) {
        self.default_record_holes.insert(var.0);
    }

    /// Whether `ty` is an inference hole marked by a bare `{ field = value }`
    /// literal (see [`Self::mark_default_record_hole`]). An explicit `_ { … }`
    /// hole is unmarked and keeps failing closed without an expected type.
    #[must_use]
    pub fn is_default_record_hole(&self, ty: TypeId) -> bool {
        matches!(self.get(ty), Type::Infer(var) if self.default_record_holes.contains(&var.0))
    }

    #[must_use]
    pub fn type_count(&self) -> usize {
        self.types.len()
    }

    /// Capture the table-local semantic type arena for MIR image execution.
    #[must_use]
    pub fn snapshot(&self) -> TypeTableSnapshot {
        TypeTableSnapshot {
            types: self.types.clone(),
            indices: self.indices.clone(),
            unspecified_shape: self.unspecified_shape,
            next_index_var: self.next_index_var,
            authored_bits: self.authored_bits,
        }
    }

    /// Rebuild a type table from a serialized MIR image snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if a required primitive type is missing or the
    /// unspecified-shape index is out of bounds or mis-typed.
    #[allow(clippy::cast_possible_truncation)]
    pub fn from_snapshot(snapshot: TypeTableSnapshot) -> Result<Self, String> {
        let mut primitives = FxHashMap::default();
        for (index, ty) in snapshot.types.iter().enumerate() {
            if let Type::Primitive(prim) = ty {
                primitives.insert(*prim, TypeId(index as u32));
            }
        }
        for prim in REQUIRED_PRIMITIVES {
            if !primitives.contains_key(&prim) {
                return Err(format!("missing primitive type {prim:?}"));
            }
        }
        if let Some(shape) = snapshot.unspecified_shape {
            let Some(expr) = snapshot.indices.get(shape.0 as usize) else {
                return Err("unspecified shape index is out of bounds".to_owned());
            };
            if !matches!(expr, IndexExpr::Unspecified) {
                return Err(
                    "unspecified shape index does not reference an unspecified shape".to_owned(),
                );
            }
        }
        let types_len = snapshot.types.len();
        let indices_len = snapshot.indices.len();
        for (index, ty) in snapshot.types.iter().enumerate() {
            if !type_refs_in_bounds(ty, types_len, indices_len) {
                return Err(format!(
                    "snapshot type {index} references an out-of-bounds TypeId or IndexId"
                ));
            }
        }
        for (index, expr) in snapshot.indices.iter().enumerate() {
            if !index_refs_in_bounds(expr, indices_len) {
                return Err(format!(
                    "snapshot index {index} references an out-of-bounds IndexId"
                ));
            }
        }
        let mut intern_map = FxHashMap::default();
        let mut first_modular_word = None;
        // The arena cannot tell an authored `b8` from the seeded `octeti` byte
        // list, so the snapshot carries which widths were authored.
        let authored_bits = snapshot.authored_bits;
        for (index, ty) in snapshot.types.iter().enumerate() {
            // First occupancy wins — matches historical linear-scan first match.
            intern_map.entry(ty.clone()).or_insert(TypeId(index as u32));
            if first_modular_word.is_none()
                && let Type::ModularWord(width) = ty
            {
                first_modular_word = Some(*width);
            }
        }
        let interval_step_one = snapshot
            .indices
            .iter()
            .position(|expr| matches!(expr, IndexExpr::Literal(1)))
            .map(|index| IndexId(index as u32));
        Ok(Self {
            types: snapshot.types,
            intern_map,
            indices: snapshot.indices,
            primitives,
            unspecified_shape: snapshot.unspecified_shape,
            interval_step_one,
            next_index_var: snapshot.next_index_var,
            equals_memo: RefCell::new(FxHashMap::default()),
            assignable_memo: RefCell::new(FxHashMap::default()),
            find_equal_memo: RefCell::new(FxHashMap::default()),
            first_modular_word,
            authored_bits,
            default_record_holes: FxHashSet::default(),
            variant_parent: FxHashMap::default(),
            resolved_generic_calls: Vec::new(),
            ordering_contract: None,
            ordering_contract_bindings: handroll::FxHashSet::default(),
            closing_contract: None,
            sinking_contract: None,
            sinking_contract_bindings: handroll::FxHashSet::default(),
        })
    }

    #[must_use]
    pub fn primitive(&self, prim: Primitive) -> TypeId {
        self.primitives[&prim]
    }

    /// Intern one compiler-owned tensor edge-policy marker type. These marker
    /// types select `limes` behavior and have no runtime value representation.
    pub fn tensor_edge_policy(&mut self, policy: BuiltinTensorEdgePolicy) -> TypeId {
        self.intern(Type::TensorEdgePolicy(policy))
    }

    /// Read a builtin edge-policy marker from a type identity, peeling aliases.
    #[must_use]
    pub fn tensor_edge_policy_kind(&self, id: TypeId) -> Option<BuiltinTensorEdgePolicy> {
        let mut resolved = id;
        let mut seen = FxHashSet::default();
        while let Type::Alias(_, inner) = self.get(resolved) {
            if !seen.insert(resolved) {
                return None;
            }
            resolved = *inner;
        }
        match self.get(resolved) {
            Type::TensorEdgePolicy(policy) => Some(*policy),
            _ => None,
        }
    }

    /// Whether a type contains a compiler-owned policy marker anywhere in its
    /// value shape. Such markers may select intrinsic behavior but cannot be
    /// stored, passed as values, or nested inside runtime payloads.
    #[must_use]
    pub fn contains_tensor_edge_policy(&self, id: TypeId) -> bool {
        fn visit(types: &TypeTable, id: TypeId, seen: &mut FxHashSet<TypeId>) -> bool {
            if !seen.insert(id) {
                return false;
            }
            let child = |child, seen: &mut FxHashSet<TypeId>| visit(types, child, seen);
            match types.get(id) {
                Type::TensorEdgePolicy(_) => true,
                Type::Array(inner)
                | Type::Queue(inner)
                | Type::Stack(inner)
                | Type::Set(inner)
                | Type::Promissum(inner)
                | Type::Cursor(inner)
                | Type::Atomic(inner)
                | Type::Intervallum(inner, _)
                | Type::Option(inner)
                | Type::Ref(_, inner)
                | Type::Alias(_, inner) => child(*inner, seen),
                Type::BoundedArray { element, .. }
                | Type::RangedArray { element, .. }
                | Type::BoundedQueue { element, .. }
                | Type::BoundedStack { element, .. }
                | Type::Tensor(element, _)
                | Type::Vector(element, _)
                | Type::Matrix(element, _)
                | Type::Sparsa(element, _) => child(*element, seen),
                Type::Map(key, value)
                | Type::Ledger(key, value)
                | Type::PromissumFailable(key, value)
                | Type::AsyncCursor(key, value) => child(*key, seen) || child(*value, seen),
                Type::Record(fields) => fields.values().any(|field| child(*field, seen)),
                Type::Func(sig) => {
                    child(sig.ret, seen)
                        || sig.err.is_some_and(|err| child(err, seen))
                        || sig.params.iter().any(|param| child(param.ty, seen))
                        || sig.type_params.iter().any(|param| {
                            matches!(&param.default, Some(SemanticGenericParamDefault::Typus(ty)) if child(*ty, seen))
                        })
                }
                Type::Applied(base, args) => {
                    child(*base, seen)
                        || args.iter().any(|arg| {
                            matches!(arg, AppliedArg::Type(ty) if child(*ty, seen))
                        })
                }
                Type::Union(members) | Type::Tuple(members) | Type::Intersection(members) => {
                    members.iter().any(|member| child(*member, seen))
                }
                Type::Primitive(_)
                | Type::BoundedTextus { .. }
                | Type::BoundedAscii { .. }
                | Type::BoundedOcteti { .. }
                | Type::SizedNumeric(_, _)
                | Type::Bits(_)
                | Type::ModularWord(_)
                | Type::SaturatingWord(_)
                | Type::RangedInt { .. }
                | Type::PolicyFloat(_, _)
                | Type::SizedInstans(_)
                | Type::Struct(_)
                | Type::Enum(_)
                | Type::Interface(_)
                | Type::Param(_)
                | Type::Infer(_)
                | Type::InferUnion(_)
                | Type::MarkerHole { .. }
                | Type::Error => false,
            }
        }
        visit(self, id, &mut FxHashSet::default())
    }

    /// Canonical sort key for union-member dedup.
    ///
    /// `Vec::dedup_by(equals)` only removes **adjacent** equals-equal members,
    /// so the sort must place semantically-equal members next to each other.
    /// The raw `TypeId` ordinal does not do that: a `typus` alias and the type
    /// it resolves to are distinct arena entries whose ordinals can be
    /// separated by unrelated members (aliases are interned during resolve,
    /// after the low-ordinal primitives), even though
    /// [`TypeTable::equals`] peels aliases.
    ///
    /// The key is the member with its top-level alias chain fully peeled,
    /// plus the `numerus` ↔ `numerus<i64>` / `fractus` ↔ `fractus<f64>`
    /// coercion pairs mapped to their primitive spelling (mirroring the
    /// coercion arms in `type_equal`). Key identity refines `equals` in the
    /// direction the sort needs: `equals(a, b)` implies `key(a) == key(b)`,
    /// so every equals-true pair ends up adjacent and the subsequent
    /// `dedup_by(equals)` removes all but the first. The converse need not
    /// hold — same-key members that are not `equals`-equal are merely
    /// adjacent and are kept by `equals`.
    ///
    /// The alias peel terminates by cycle detection (a visited set), not a
    /// fixed hop bound: acyclic chains of any depth peel fully — matching the
    /// unbounded peel in [`equals`](Self::equals), so `equals(a, b)` implies
    /// `key(a) == key(b)` is STRUCTURAL for all acyclic inputs. A cyclic
    /// chain terminates on the first revisited `TypeId` (resolution rejects
    /// alias cycles with SEM007 before lowering reaches this point; the
    /// guard is structural rather than a depth heuristic). O(alias-depth)
    /// per member.
    #[must_use]
    pub fn union_member_sort_key(&self, id: TypeId) -> TypeId {
        let mut resolved = id;
        let mut seen = FxHashSet::default();
        while let Type::Alias(_, inner) = self.get(resolved) {
            if !seen.insert(resolved) {
                break;
            }
            resolved = *inner;
        }
        match self.get(resolved) {
            Type::Primitive(Primitive::Numerus)
            | Type::SizedNumeric(Primitive::Numerus, NumericWidth::I64) => {
                self.primitive(Primitive::Numerus)
            }
            Type::Primitive(Primitive::Fractus)
            | Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64) => {
                self.primitive(Primitive::Fractus)
            }
            _ => resolved,
        }
    }

    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn find_struct(&self, def_id: DefId) -> Option<TypeId> {
        self.types.iter().enumerate().find_map(|(index, ty)| {
            matches!(ty, Type::Struct(found) if *found == def_id).then_some(TypeId(index as u32))
        })
    }

    /// Find an existing `lista<T>` id for the given element type, if one was interned.
    #[must_use]
    pub fn find_array(&self, element: TypeId) -> Option<TypeId> {
        self.intern_map.get(&Type::Array(element)).copied()
    }

    /// Return an existing or freshly interned `lista<T>` id.
    pub fn ensure_array(&mut self, element: TypeId) -> TypeId {
        self.find_array(element)
            .unwrap_or_else(|| self.array(element))
    }

    /// Allocate `T ∪ nihil` as an optional type.
    pub fn option(&mut self, inner: TypeId) -> TypeId {
        self.intern(Type::Option(inner))
    }

    pub fn record(&mut self, fields: impl IntoIterator<Item = (LexSymbol, TypeId)>) -> TypeId {
        self.intern(Type::Record(fields.into_iter().collect()))
    }

    pub fn tuple(&mut self, elements: Vec<TypeId>) -> TypeId {
        self.intern(Type::Tuple(elements))
    }

    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn find_tuple(&self, elements: &[TypeId]) -> Option<TypeId> {
        self.types.iter().enumerate().find_map(|(index, ty)| {
            matches!(ty, Type::Tuple(existing) if existing == elements)
                .then_some(TypeId(index as u32))
        })
    }

    /// Intern the empty tuple as the unit type.
    pub fn unit(&mut self) -> TypeId {
        self.find_tuple(&[])
            .unwrap_or_else(|| self.tuple(Vec::new()))
    }

    /// If `ty` is a nullable carrier (`T ∪ nihil` union or `Option<T>`), return
    /// the non-null payload type. Alias chains are traversed. Returns `None` for
    /// non-nullable types, unions whose non-null arm count is not exactly one,
    /// or cyclic Alias chains (fail-closed).
    #[must_use]
    pub fn nullable_inner(&self, ty: TypeId) -> Option<TypeId> {
        let mut seen = FxHashSet::default();
        self.nullable_inner_with_seen(ty, &mut seen)
    }

    /// Path-local Alias cycle guard for [`Self::nullable_inner`].
    ///
    /// `seen` tracks `Type::Alias` nodes on the current peel path. Insert before
    /// descending; a second encounter of the same `TypeId` means a cycle and
    /// returns `None` instead of overflowing the stack.
    fn nullable_inner_with_seen(&self, ty: TypeId, seen: &mut FxHashSet<TypeId>) -> Option<TypeId> {
        match self.get(ty) {
            Type::Option(inner) => Some(*inner),
            Type::Union(members) => {
                let nihil = self.primitive(Primitive::Nihil);
                if !members.iter().any(|m| self.equals(*m, nihil)) {
                    return None;
                }
                let others: Vec<TypeId> = members
                    .iter()
                    .copied()
                    .filter(|member| !self.equals(*member, nihil))
                    .collect();
                (others.len() == 1).then_some(others[0])
            }
            Type::Alias(_, inner) => {
                if !seen.insert(ty) {
                    return None;
                }
                let inner = *inner;
                self.nullable_inner_with_seen(inner, seen)
            }
            _ => None,
        }
    }

    pub fn reference(&mut self, mutability: SemanticMutability, inner: TypeId) -> TypeId {
        self.intern(Type::Ref(mutability, inner))
    }

    /// Intern an access-discipline marker wrapping `inner`.
    pub fn access_marker(&mut self, marker: AccessMarker, inner: TypeId) -> TypeId {
        self.reference(marker.mutability(), inner)
    }

    /// Peel aliases and at most one access-marker wrapper.
    ///
    /// Returns the marker fact (if present) and the carrier payload. Used by
    /// mode-scoped checks so payload typing stays independent of `ref`/`mut`/`own`.
    #[must_use]
    pub fn peel_access_marker(&self, ty: TypeId) -> (Option<AccessMarker>, TypeId) {
        let mut id = ty;
        let mut marker = None;
        loop {
            match self.get(id) {
                Type::Alias(_, inner) => id = *inner,
                Type::Ref(mutability, inner) if marker.is_none() => {
                    marker = Some(mutability.access_marker());
                    id = *inner;
                }
                _ => return (marker, id),
            }
        }
    }

    /// Whether `ty` carries a declared `ref` access fact.
    #[must_use]
    pub fn is_ref_view(&self, ty: TypeId) -> bool {
        matches!(self.peel_access_marker(ty).0, Some(AccessMarker::Ref))
    }

    /// Whether `ty` carries a declared `mut` access fact.
    #[must_use]
    pub fn is_mut_view(&self, ty: TypeId) -> bool {
        matches!(self.peel_access_marker(ty).0, Some(AccessMarker::Mut))
    }

    /// Whether `ty` carries a declared `mut` or `own` access fact.
    #[must_use]
    pub fn is_mut_or_own_view(&self, ty: TypeId) -> bool {
        matches!(
            self.peel_access_marker(ty).0,
            Some(AccessMarker::Mut | AccessMarker::Own)
        )
    }

    /// Whether `ty` is a declared `own` destination (`own` / `penes`).
    ///
    /// `copy` elaborates to `own` but is a different acquisition event, so it
    /// is not an own-bind destination.
    #[must_use]
    pub fn is_own_view(&self, ty: TypeId) -> bool {
        matches!(self.peel_access_marker(ty).0, Some(AccessMarker::Own))
    }

    /// Whether `ty` is a declared duplicate-then-own acquisition (`copy`).
    #[must_use]
    pub fn is_copy_view(&self, ty: TypeId) -> bool {
        matches!(self.peel_access_marker(ty).0, Some(AccessMarker::Copy))
    }

    /// Whether `ty` is an exclusive value that transfers on own-bind.
    ///
    /// `copy` elaborates to `own` (D-6), so a copy binding is a transferable
    /// source. Unmarked / `ref` / `mut` are not.
    #[must_use]
    pub fn is_exclusive_value(&self, ty: TypeId) -> bool {
        matches!(
            self.peel_access_marker(ty).0,
            Some(AccessMarker::Own | AccessMarker::Copy)
        )
    }

    /// Whether `ty` is a container or buffer carrier whose unmarked bind
    /// aliases its source place (R1, `faber/docs/semantics-rulings.md`).
    ///
    /// This is the one classification both the semantic borrow pass and the
    /// Rust alias-bind emitter consume, so they cannot disagree on which
    /// containers alias. Aliases peel; access markers do not (a marked bind
    /// takes its own acquisition path). Genus and contract instances alias as
    /// well but are decided by each consumer (the Rust emitter carves out
    /// `dyn Trait` carriers), and scalars and text are not containers here.
    ///
    /// Register-class `vector`/`matrix` values carry lanes of their own, so
    /// they are containers here, but an unmarked bind of one COPIES rather
    /// than aliases (operator ruling 7b25889f R1 — `b ← a` copies, `ref` is
    /// refused as for scalars). Consumers asking the bind question use
    /// [`Self::binds_by_sharing`], not this predicate. An optional container
    /// aliases through the option
    /// ([`Self::is_aliasing_container_through_option`]).
    #[must_use]
    pub fn is_aliasing_container(&self, ty: TypeId) -> bool {
        match self.get(ty) {
            Type::Alias(_, inner) => self.is_aliasing_container(*inner),
            Type::Array(_)
            | Type::BoundedArray { .. }
            | Type::RangedArray { .. }
            | Type::Queue(_)
            | Type::BoundedQueue { .. }
            | Type::Stack(_)
            | Type::BoundedStack { .. }
            | Type::Map(..)
            | Type::Ledger(..)
            | Type::Set(_)
            | Type::Tensor(..)
            | Type::Sparsa(..)
            | Type::Vector(..)
            | Type::Matrix(..)
            | Type::BoundedOcteti { .. } => true,
            _ => false,
        }
    }

    /// [`Self::is_aliasing_container`] peeled through `T ∪ nihil` (al2,
    /// advisor ruling 2026-09-30 Q2): an unmarked bind of an optional
    /// container aliases the container place through the option.
    #[must_use]
    pub fn is_aliasing_container_through_option(&self, ty: TypeId) -> bool {
        self.is_aliasing_container(ty)
            || self
                .nullable_inner(ty)
                .is_some_and(|inner| self.is_aliasing_container_through_option(inner))
    }

    /// Whether `ty` is a register-class `vector`/`matrix` value: lanes held
    /// directly, with no storage handle, so every store copies the whole value
    /// (operator ruling 7b25889f R1 — silent values, `b ← a` copies). Aliases
    /// peel; an option of one is not one (the option carries the cell).
    #[must_use]
    pub fn is_register_class_value(&self, ty: TypeId) -> bool {
        match self.get(ty) {
            Type::Alias(_, inner) => self.is_register_class_value(*inner),
            Type::Vector(..) | Type::Matrix(..) => true,
            _ => false,
        }
    }

    /// Whether an unmarked bind of `ty` shares its source's storage instead of
    /// taking a value of its own.
    ///
    /// This is the predicate the bind rules consume — the semantic borrow
    /// pass, the shared-payload check and the Rust alias-bind emitter — so
    /// they cannot disagree about which types alias on `b ← a`. It is
    /// [`Self::is_aliasing_container`] without the register-class silent
    /// values: `b ← a` on a `vector`/`matrix` copies (operator ruling
    /// 7b25889f R1), exactly as it does for a scalar, and only a `ref`-marked
    /// bind would name the same storage — which R1 refuses outright.
    #[must_use]
    pub fn binds_by_sharing(&self, ty: TypeId) -> bool {
        match self.get(ty) {
            Type::Alias(_, inner) => self.binds_by_sharing(*inner),
            _ if self.is_register_class_value(ty) => false,
            _ => self.is_aliasing_container(ty),
        }
    }

    /// [`Self::binds_by_sharing`] peeled through `T ∪ nihil`: an unmarked bind
    /// of an optional container aliases the container place through the
    /// option, and an optional register-class value still copies (R1).
    #[must_use]
    pub fn binds_by_sharing_through_option(&self, ty: TypeId) -> bool {
        self.binds_by_sharing(ty)
            || self
                .nullable_inner(ty)
                .is_some_and(|inner| self.binds_by_sharing_through_option(inner))
    }

    /// Silent values reject `ref`/`mut`/`own`/`copy` markers: the widthless
    /// scalars, and the register-class `vector`/`matrix` (operator ruling
    /// 7b25889f R1 — silent values, `b ← a` copies, `ref` refused as for
    /// scalars).
    ///
    /// Referential types (collections, buffers, `textus`/`octeti`, tensors,
    /// genus instances) keep the marker. Aliases peel. Inference holes do not
    /// reject — the carrier is not yet known.
    #[must_use]
    pub fn is_scalar_for_access_marker(&self, ty: TypeId) -> bool {
        match self.get(ty) {
            Type::Alias(_, inner) => self.is_scalar_for_access_marker(*inner),
            Type::Primitive(prim) => matches!(
                prim,
                Primitive::Numerus
                    | Primitive::Fractus
                    | Primitive::Bivalens
                    | Primitive::Nihil
                    | Primitive::Vacuum
                    | Primitive::Numquam
                    | Primitive::Instans
            ),
            Type::Vector(_, _)
            | Type::Matrix(_, _)
            | Type::SizedNumeric(_, _)
            | Type::Bits(_)
            | Type::ModularWord(_)
            | Type::SaturatingWord(_)
            | Type::RangedInt { .. }
            | Type::PolicyFloat(_, _)
            | Type::SizedInstans(_) => true,
            _ => false,
        }
    }

    /// Allocate a `lista<T>` semantic collection.
    pub fn array(&mut self, element: TypeId) -> TypeId {
        self.intern(Type::Array(element))
    }

    /// Allocate a `lista<T, magnitudo N>` bounded array. Distinct from unbounded [`Type::Array`].
    ///
    /// A bounded list of raw bits (`b8`) is the bounded byte buffer and
    /// lowers to [`Type::BoundedOcteti`]; every other
    /// element type interns a [`Type::BoundedArray`].
    pub fn bounded_array(&mut self, element: TypeId, capacity: IndexId) -> TypeId {
        if matches!(self.get(element), Type::Bits(BitsWidth::B8)) {
            return self.bounded_octeti(capacity);
        }
        self.intern(Type::BoundedArray { element, capacity })
    }

    /// Allocate a `lista<T, magnitudo lo…hi>` length range. The range is
    /// inclusive and `max` is `None` for no upper end. A range that is
    /// `0…∞` *is* [`Type::Array`] and `0…N` *is* the bounded list, so the
    /// sugar forms keep one identity (rule 2 of the ranged-policy design).
    pub fn ranged_array(&mut self, element: TypeId, min: u64, max: Option<u64>) -> TypeId {
        match (min, max) {
            (0, None) => self.array(element),
            (0, Some(max)) => {
                let capacity = self.intern_index(IndexExpr::Literal(max));
                self.bounded_array(element, capacity)
            }
            _ => self.intern(Type::RangedArray { element, min, max }),
        }
    }

    /// Allocate an unbounded `queue<T>` collection.
    pub fn queue(&mut self, element: TypeId) -> TypeId {
        self.intern(Type::Queue(element))
    }

    /// Allocate a bounded `queue<T, N>` collection.
    pub fn bounded_queue(&mut self, element: TypeId, capacity: IndexId) -> TypeId {
        self.intern(Type::BoundedQueue { element, capacity })
    }

    /// Allocate an unbounded `stack<T>` collection.
    pub fn stack(&mut self, element: TypeId) -> TypeId {
        self.intern(Type::Stack(element))
    }

    /// Allocate a bounded `stack<T, N>` collection.
    pub fn bounded_stack(&mut self, element: TypeId, capacity: IndexId) -> TypeId {
        self.intern(Type::BoundedStack { element, capacity })
    }

    /// Allocate a `textus<N>` bounded Unicode buffer.
    pub fn bounded_textus(&mut self, capacity: IndexId) -> TypeId {
        self.intern(Type::BoundedTextus { capacity })
    }

    /// Allocate an `ascii<N>` bounded ASCII buffer.
    pub fn bounded_ascii(&mut self, capacity: IndexId) -> TypeId {
        self.intern(Type::BoundedAscii { capacity })
    }

    /// Allocate an `octeti<N>` bounded byte buffer.
    pub fn bounded_octeti(&mut self, capacity: IndexId) -> TypeId {
        self.intern(Type::BoundedOcteti { capacity })
    }

    /// Allocate a `tabula<K, V>` semantic collection.
    pub fn map(&mut self, key: TypeId, value: TypeId) -> TypeId {
        self.intern(Type::Map(key, value))
    }

    /// Allocate a `tabulae<K, V>` ledger (an insertion-ordered map).
    pub fn ledger(&mut self, key: TypeId, value: TypeId) -> TypeId {
        self.intern(Type::Ledger(key, value))
    }

    /// Allocate a `copia<T>` semantic collection.
    pub fn set(&mut self, element: TypeId) -> TypeId {
        self.intern(Type::Set(element))
    }

    /// Allocate a `promissum<T>` semantic collection.
    pub fn promissum(&mut self, inner: TypeId) -> TypeId {
        self.intern(Type::Promissum(inner))
    }

    /// Allocate a `promissum<T ⇥ E>` failable promise (two channels).
    pub fn promissum_failable(&mut self, success: TypeId, alternate: TypeId) -> TypeId {
        self.intern(Type::PromissumFailable(success, alternate))
    }

    /// Allocate a `cursor<T>` semantic collection.
    pub fn cursor(&mut self, inner: TypeId) -> TypeId {
        self.intern(Type::Cursor(inner))
    }

    /// Allocate an async cursor semantic stream with item and alternate channels.
    pub fn async_cursor(&mut self, item: TypeId, alternate: TypeId) -> TypeId {
        self.intern(Type::AsyncCursor(item, alternate))
    }

    /// Allocate a fresh [`IndexVar`] for surface `_` / inference holes in types.
    pub fn fresh_index_infer_var(&mut self) -> super::index::IndexVar {
        let var = super::index::IndexVar(self.next_index_var);
        self.next_index_var += 1;
        var
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn intern_index(&mut self, expr: IndexExpr) -> IndexId {
        let id = IndexId(self.indices.len() as u32);
        self.indices.push(expr);
        id
    }

    #[must_use]
    pub fn get_index(&self, id: IndexId) -> &IndexExpr {
        &self.indices[id.0 as usize]
    }

    #[must_use]
    pub fn index_count(&self) -> usize {
        self.indices.len()
    }

    /// Allocate the memoized unspecified-shape marker (`IndexExpr::Unspecified`).
    ///
    /// Used by interface index snapshots and unsupported-type lowering where a
    /// shape slot has no concrete value.
    pub fn unspecified_shape(&mut self) -> IndexId {
        if let Some(id) = self.unspecified_shape {
            return id;
        }
        let id = self.intern_index(IndexExpr::Unspecified);
        self.unspecified_shape = Some(id);
        id
    }

    /// Allocate a rank-0 shape `[]`.
    pub fn rank0_shape(&mut self) -> IndexId {
        self.intern_index(IndexExpr::Tuple(Vec::new()))
    }

    /// Allocate a `tensor<T, Figura>` semantic collection.
    pub fn tensor_with_shape(&mut self, element: TypeId, shape: IndexId) -> TypeId {
        self.intern(Type::Tensor(element, shape))
    }

    /// Allocate a `vector<T, size N>` semantic register vector.
    pub fn vector_with_width(&mut self, element: TypeId, width: IndexId) -> TypeId {
        self.intern(Type::Vector(element, width))
    }

    /// Allocate a `matrix<T, [R, C]>` semantic register matrix.
    pub fn matrix_with_shape(&mut self, element: TypeId, shape: IndexId) -> TypeId {
        self.intern(Type::Matrix(element, shape))
    }

    /// Allocate a `sparsa<T, Figura>` semantic collection.
    pub fn sparsa_with_shape(&mut self, element: TypeId, shape: IndexId) -> TypeId {
        self.intern(Type::Sparsa(element, shape))
    }

    /// Allocate an `atomic<T>` storage-sensitive cell type.
    pub fn atomic(&mut self, element: TypeId) -> TypeId {
        self.intern(Type::Atomic(element))
    }

    #[must_use]
    pub fn index_equals(&self, a: IndexId, b: IndexId) -> bool {
        if a == b {
            return true;
        }
        match (self.get_index(a), self.get_index(b)) {
            (IndexExpr::Literal(la), IndexExpr::Literal(lb)) => la == lb,
            (IndexExpr::Param(pa), IndexExpr::Param(pb)) => pa == pb,
            (IndexExpr::Runtime(a), IndexExpr::Runtime(b)) => a == b,
            (
                IndexExpr::Affine {
                    base: a_base,
                    offset: a_offset,
                },
                IndexExpr::Affine {
                    base: b_base,
                    offset: b_offset,
                },
            ) => a_offset == b_offset && self.index_equals(*a_base, *b_base),
            (
                IndexExpr::Divide {
                    numerator: a_numerator,
                    divisor: a_divisor,
                },
                IndexExpr::Divide {
                    numerator: b_numerator,
                    divisor: b_divisor,
                },
            ) => a_divisor == b_divisor && self.index_equals(*a_numerator, *b_numerator),
            (IndexExpr::Infer(va), IndexExpr::Infer(vb)) => va == vb,
            (IndexExpr::Unspecified, IndexExpr::Unspecified) => true,
            (IndexExpr::Tuple(a_dims), IndexExpr::Tuple(b_dims)) => {
                a_dims.len() == b_dims.len()
                    && a_dims
                        .iter()
                        .zip(b_dims.iter())
                        .all(|(a_dim, b_dim)| self.index_equals(*a_dim, *b_dim))
            }
            _ => false,
        }
    }

    /// Allocate an `intervallum<T>` interval over bound type `T` with step 1.
    pub fn intervallum(&mut self, bound: TypeId) -> TypeId {
        let step = self.interval_step_one();
        self.intervallum_with_step(bound, step)
    }

    /// Allocate an `intervallum<T, S>` interval with a compile-time step index.
    ///
    /// An explicit step of `Literal(1)` canonicalizes to the same interned
    /// step as [`Self::intervallum`], so both spellings share one `TypeId`.
    pub fn intervallum_with_step(&mut self, bound: TypeId, step: IndexId) -> TypeId {
        let step = if matches!(self.get_index(step), IndexExpr::Literal(1)) {
            self.interval_step_one()
        } else {
            step
        };
        self.intern(Type::Intervallum(bound, step))
    }

    fn interval_step_one(&mut self) -> IndexId {
        if let Some(id) = self.interval_step_one {
            return id;
        }
        let id = self.intern_index(IndexExpr::Literal(1));
        self.interval_step_one = Some(id);
        id
    }

    /// Allocate a sized `numerus<W>` / `fractus<W>` primitive.
    pub fn sized_numeric(&mut self, prim: Primitive, width: NumericWidth) -> TypeId {
        self.intern(Type::SizedNumeric(prim, width))
    }

    /// Allocate a ranged integer cell (`policy<W, lo…hi>`, inclusive ends).
    ///
    /// A range that is the whole width interns as the plain form the range
    /// abbreviates: `trapping<u8, 0…255>` is `u8`, `saturating<u8, 0…255>` is
    /// `saturating<u8>`, `wrapping<u8, 0…255>` is `wrapping<u8>`. Callers have
    /// already checked that `lo ≤ hi` lies inside the width's range.
    pub fn ranged_int(
        &mut self,
        policy: IntPolicy,
        width: NumericWidth,
        lo: i128,
        hi: i128,
    ) -> TypeId {
        if width.integer_range() == Some((lo, hi)) {
            return match policy {
                IntPolicy::Trapping => self.sized_numeric(Primitive::Numerus, width),
                IntPolicy::Wrapping => self.intern(Type::ModularWord(width)),
                IntPolicy::Saturating => self.intern(Type::SaturatingWord(width)),
            };
        }
        // The plain form the cell refines is interned with it, so a read-only
        // checker can find it again (`ranged_int_base_existing`).
        if let Some((width_lo, width_hi)) = width.integer_range() {
            self.ranged_int(policy, width, width_lo, width_hi);
        }
        self.intern(Type::RangedInt {
            policy,
            width,
            lo,
            hi,
        })
    }

    /// The policy, width and inclusive range of a ranged integer cell, aliases
    /// peeled. `None` for every other type, including the plain forms.
    #[must_use]
    pub fn ranged_int_parts(&self, ty: TypeId) -> Option<(IntPolicy, NumericWidth, i128, i128)> {
        match self.get(ty) {
            Type::RangedInt {
                policy,
                width,
                lo,
                hi,
            } => Some((*policy, *width, *lo, *hi)),
            Type::Alias(_, inner) => self.ranged_int_parts(*inner),
            _ => None,
        }
    }

    /// The plain type a ranged integer refines: `u8` for a trapping cell,
    /// `wrapping<u8>` or `saturating<u8>` otherwise. Any other type is itself.
    ///
    /// WHY: arithmetic is exact and a range applies only at a store, so a value
    /// read from a ranged cell computes as its width.
    pub fn ranged_int_base(&mut self, ty: TypeId) -> TypeId {
        match self.ranged_int_parts(ty) {
            Some((policy, width, _, _)) => {
                let (lo, hi) = width.integer_range().unwrap_or((0, 0));
                self.ranged_int(policy, width, lo, hi)
            }
            None => ty,
        }
    }

    /// [`Self::ranged_int_base`] for a read-only caller. Every ranged cell is
    /// interned together with the plain form it refines, so the lookup finds
    /// it; a type that is not a ranged cell is itself.
    #[must_use]
    pub fn ranged_int_base_existing(&self, ty: TypeId) -> TypeId {
        let Some((policy, width, _, _)) = self.ranged_int_parts(ty) else {
            return ty;
        };
        let candidate = match policy {
            IntPolicy::Trapping => Type::SizedNumeric(Primitive::Numerus, width),
            IntPolicy::Wrapping => Type::ModularWord(width),
            IntPolicy::Saturating => Type::SaturatingWord(width),
        };
        self.find_equal(&candidate).unwrap_or(ty)
    }

    /// Allocate a raw-bits `bN` type.
    pub fn bits(&mut self, width: BitsWidth) -> TypeId {
        self.intern(Type::Bits(width))
    }

    /// Allocate `lista<b8>`, the type the retired `octeti` word, the
    /// `|de ad be ef|` literal and `insere` of a file produce (need,
    /// ruling B9).
    pub fn octeti_list(&mut self) -> TypeId {
        // Seeded without marking the width: the byte list is the host-carried
        // `octeti` word, not an authored `b8` (device gates key on the latter).
        let byte = self.intern_unmarked(Type::Bits(BitsWidth::B8));
        self.array(byte)
    }

    /// Allocate `instans<N>` with an explicit precision marker.
    pub fn sized_instans(&mut self, precision: InstansPrecision) -> TypeId {
        self.intern(Type::SizedInstans(precision))
    }

    /// Return an existing sized-numeric id if one is already interned.
    ///
    /// WHY: codegen holds `&TypeTable` (immutable) and cannot intern. The octeti
    /// unify path routes octeti receivers through the lista codegen path with a
    /// `numerus<u8>` element type; this lookup finds that id without mutation.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn find_sized_numeric(&self, prim: Primitive, width: NumericWidth) -> Option<TypeId> {
        self.types.iter().enumerate().find_map(|(index, ty)| {
            matches!(ty, Type::SizedNumeric(p, w) if *p == prim && *w == width)
                .then_some(TypeId(index as u32))
        })
    }

    pub fn function(&mut self, sig: FuncSig) -> TypeId {
        self.intern(Type::Func(sig))
    }

    /// Return whether two table-local type IDs describe the same semantic shape.
    ///
    /// Results are memoized by ordered pair so recursive structural walks do
    /// not repeat work for the same `(TypeId, TypeId)`. The memo is valid for
    /// the lifetime of the table: adding new interned types does not change
    /// [`type_equal`](Self::type_equal) for existing pairs.
    #[must_use]
    pub fn equals(&self, a: TypeId, b: TypeId) -> bool {
        if a == b {
            return true;
        }
        let key = if a.0 <= b.0 { (a, b) } else { (b, a) };
        if let Some(&cached) = self.equals_memo.borrow().get(&key) {
            return cached;
        }
        let result = self.type_equal(self.get(a), self.get(b));
        self.equals_memo.borrow_mut().insert(key, result);
        result
    }

    fn applied_args_equal(&self, a: AppliedArg, b: AppliedArg) -> bool {
        match (a, b) {
            (AppliedArg::Type(a), AppliedArg::Type(b)) => self.equals(a, b),
            (AppliedArg::Size(a), AppliedArg::Size(b)) => self.index_equals(a, b),
            (AppliedArg::Type(_), AppliedArg::Size(_))
            | (AppliedArg::Size(_), AppliedArg::Type(_)) => false,
        }
    }

    /// Compare two type values structurally, using [`equals`](Self::equals) for
    /// child `TypeId`s so alias peeling and numeric coercion are honored. The
    /// comparison core is [`Self::type_equal_with_seen`]; this entry point
    /// starts a fresh alias-path guard per comparison.
    ///
    /// Cyclic alias chains (`typus A = B` / `typus B = A`) fail closed through
    /// [`Self::type_equal_with_seen`] instead of overflowing the stack.
    fn type_equal(&self, a: &Type, b: &Type) -> bool {
        let mut seen = FxHashSet::default();
        self.type_equal_with_seen(a, b, &mut seen)
    }

    /// Structural comparison core with an alias-path cycle guard.
    ///
    /// `seen` tracks the interned reference pair currently on the comparison
    /// path. Types are interned, so `&Type` identity is stable per id; a
    /// repeated pair on one path means a cyclic alias chain and returns
    /// `false` (fail-closed), mirroring [`Self::nullable_inner_with_seen`].
    #[allow(clippy::too_many_lines, clippy::match_same_arms)]
    fn type_equal_with_seen(
        &self,
        a: &Type,
        b: &Type,
        seen: &mut FxHashSet<(usize, usize)>,
    ) -> bool {
        let key = (
            std::ptr::from_ref(a) as usize,
            std::ptr::from_ref(b) as usize,
        );
        if !seen.insert(key) {
            return false;
        }
        match (a, b) {
            (Type::Primitive(pa), Type::Primitive(pb)) => pa == pb,
            // D10.4: `octeti` is exactly `lista<octetus>` (`octetus ≡
            // numerus<u8>`) for type identity — one type with a byte-buffer
            // runtime representation, not two mutually assignable ones (L2-10).
            (Type::Primitive(Primitive::Octeti), Type::Array(inner))
            | (Type::Array(inner), Type::Primitive(Primitive::Octeti)) => matches!(
                self.get(*inner),
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::U8)
            ),
            (Type::SizedNumeric(pa, aw), Type::SizedNumeric(pb, bw)) => pa == pb && aw == bw,
            (Type::Bits(aw), Type::Bits(bw)) => aw == bw,
            (Type::ModularWord(aw), Type::ModularWord(bw))
            | (Type::SaturatingWord(aw), Type::SaturatingWord(bw)) => aw == bw,
            (
                Type::RangedInt {
                    policy: ap,
                    width: aw,
                    lo: alo,
                    hi: ahi,
                },
                Type::RangedInt {
                    policy: bp,
                    width: bw,
                    lo: blo,
                    hi: bhi,
                },
            ) => ap == bp && aw == bw && alo == blo && ahi == bhi,
            (Type::PolicyFloat(ap, aw), Type::PolicyFloat(bp, bw)) => ap == bp && aw == bw,
            (Type::SizedInstans(ap), Type::SizedInstans(bp)) => ap == bp,
            // Bare seconds default and explicit `instans<N>` are distinct semantic types.
            (Type::Primitive(Primitive::Instans), Type::SizedInstans(_))
            | (Type::SizedInstans(_), Type::Primitive(Primitive::Instans)) => false,
            (
                Type::Primitive(Primitive::Numerus),
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::I64),
            )
            | (
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::I64),
                Type::Primitive(Primitive::Numerus),
            ) => true,
            (
                Type::Primitive(Primitive::Fractus),
                Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64),
            )
            | (
                Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64),
                Type::Primitive(Primitive::Fractus),
            ) => true,
            (Type::Tensor(ae, a_shape), Type::Tensor(be, b_shape)) => {
                self.equals(*ae, *be) && self.index_equals(*a_shape, *b_shape)
            }
            (Type::Vector(ae, a_width), Type::Vector(be, b_width)) => {
                self.equals(*ae, *be) && self.index_equals(*a_width, *b_width)
            }
            (Type::Matrix(ae, a_shape), Type::Matrix(be, b_shape)) => {
                self.equals(*ae, *be) && self.index_equals(*a_shape, *b_shape)
            }
            (Type::Sparsa(ae, a_shape), Type::Sparsa(be, b_shape)) => {
                self.equals(*ae, *be) && self.index_equals(*a_shape, *b_shape)
            }
            (Type::Atomic(ae), Type::Atomic(be)) => self.equals(*ae, *be),
            (Type::Intervallum(ae, a_step), Type::Intervallum(be, b_step)) => {
                self.equals(*ae, *be) && self.index_equals(*a_step, *b_step)
            }
            (Type::Array(ae), Type::Array(be))
            | (Type::Option(ae), Type::Option(be))
            | (Type::Set(ae), Type::Set(be))
            | (Type::Promissum(ae), Type::Promissum(be))
            | (Type::Cursor(ae), Type::Cursor(be))
            | (Type::Queue(ae), Type::Queue(be))
            | (Type::Stack(ae), Type::Stack(be)) => self.equals(*ae, *be),
            (
                Type::BoundedArray {
                    element: ae,
                    capacity: ac,
                },
                Type::BoundedArray {
                    element: be,
                    capacity: bc,
                },
            ) => self.equals(*ae, *be) && self.index_equals(*ac, *bc),
            (
                Type::RangedArray {
                    element: ae,
                    min: a_min,
                    max: a_max,
                },
                Type::RangedArray {
                    element: be,
                    min: b_min,
                    max: b_max,
                },
            ) => self.equals(*ae, *be) && a_min == b_min && a_max == b_max,
            (
                Type::BoundedQueue {
                    element: ae,
                    capacity: ac,
                },
                Type::BoundedQueue {
                    element: be,
                    capacity: bc,
                },
            )
            | (
                Type::BoundedStack {
                    element: ae,
                    capacity: ac,
                },
                Type::BoundedStack {
                    element: be,
                    capacity: bc,
                },
            ) => self.equals(*ae, *be) && self.index_equals(*ac, *bc),
            (Type::BoundedTextus { capacity: ac }, Type::BoundedTextus { capacity: bc })
            | (Type::BoundedAscii { capacity: ac }, Type::BoundedAscii { capacity: bc })
            | (Type::BoundedOcteti { capacity: ac }, Type::BoundedOcteti { capacity: bc }) => {
                self.index_equals(*ac, *bc)
            }
            (Type::PromissumFailable(at, ae), Type::PromissumFailable(bt, be)) => {
                self.equals(*at, *bt) && self.equals(*ae, *be)
            }
            (Type::AsyncCursor(ai, ae), Type::AsyncCursor(bi, be)) => {
                self.equals(*ai, *bi) && self.equals(*ae, *be)
            }
            (Type::Map(ak, av), Type::Map(bk, bv))
            | (Type::Ledger(ak, av), Type::Ledger(bk, bv)) => {
                self.equals(*ak, *bk) && self.equals(*av, *bv)
            }
            (Type::Record(a_fields), Type::Record(b_fields)) => {
                a_fields.len() == b_fields.len()
                    && a_fields.iter().all(|(name, a_ty)| {
                        b_fields
                            .get(name)
                            .is_some_and(|b_ty| self.equals(*a_ty, *b_ty))
                    })
            }
            (Type::Tuple(a_elems), Type::Tuple(b_elems)) => {
                a_elems.len() == b_elems.len()
                    && a_elems
                        .iter()
                        .zip(b_elems.iter())
                        .all(|(a_elem, b_elem)| self.equals(*a_elem, *b_elem))
            }
            (Type::Ref(am, ai), Type::Ref(bm, bi)) => am == bm && self.equals(*ai, *bi),
            (Type::Struct(a_def), Type::Struct(b_def))
            | (Type::Enum(a_def), Type::Enum(b_def))
            | (Type::Interface(a_def), Type::Interface(b_def)) => a_def == b_def,
            (Type::Alias(_, a_inner), _) => self.type_equal_with_seen(self.get(*a_inner), b, seen),
            (_, Type::Alias(_, b_inner)) => self.type_equal_with_seen(a, self.get(*b_inner), seen),
            (Type::Func(a_sig), Type::Func(b_sig)) => {
                a_sig.type_params.len() == b_sig.type_params.len()
                    && a_sig.type_params.iter().zip(b_sig.type_params.iter()).all(
                        |(a_param, b_param)| {
                            a_param.name == b_param.name
                                && a_param.sort == b_param.sort
                                && self.type_param_constraint_equal(
                                    &a_param.constraint,
                                    &b_param.constraint,
                                )
                                && self.generic_param_defaults_equal(
                                    a_param.default.as_ref(),
                                    b_param.default.as_ref(),
                                )
                        },
                    )
                    && a_sig.params.len() == b_sig.params.len()
                    && a_sig
                        .params
                        .iter()
                        .zip(b_sig.params.iter())
                        .all(|(a_param, b_param)| {
                            a_param.mode == b_param.mode
                                && a_param.optional == b_param.optional
                                && a_param.sponte == b_param.sponte
                                && self.equals(a_param.ty, b_param.ty)
                        })
                    && self.equals(a_sig.ret, b_sig.ret)
                    && match (a_sig.err, b_sig.err) {
                        (Some(a_err), Some(b_err)) => self.equals(a_err, b_err),
                        (None, None) => true,
                        _ => false,
                    }
                    && a_sig.is_async == b_sig.is_async
                    && a_sig.is_generator == b_sig.is_generator
            }
            (Type::Param(a_name), Type::Param(b_name)) => a_name == b_name,
            (Type::Applied(a_base, a_args), Type::Applied(b_base, b_args)) => {
                a_args.len() == b_args.len()
                    && self.equals(*a_base, *b_base)
                    && a_args
                        .iter()
                        .zip(b_args.iter())
                        .all(|(a_arg, b_arg)| self.applied_args_equal(*a_arg, *b_arg))
            }
            (Type::Infer(a_var), Type::Infer(b_var)) => a_var == b_var,
            (Type::InferUnion(a_var), Type::InferUnion(b_var)) => a_var == b_var,
            (
                Type::MarkerHole {
                    family: af,
                    var: av,
                },
                Type::MarkerHole {
                    family: bf,
                    var: bv,
                },
            ) => af == bf && av == bv,
            (Type::Union(a_types), Type::Union(b_types)) => {
                self.union_members_equal(a_types, b_types)
            }
            (Type::Intersection(a_types), Type::Intersection(b_types)) => {
                self.union_members_equal(a_types, b_types)
            }
            (Type::Error, Type::Error) => true,
            _ => false,
        }
    }

    fn union_members_equal(&self, a_types: &[TypeId], b_types: &[TypeId]) -> bool {
        if a_types.len() != b_types.len() {
            return self.union_members_equal_semantic(a_types, b_types);
        }
        if a_types.is_empty() {
            return true;
        }

        // Sort by the semantic key used for union deduplication. Semantically
        // equal members usually share a key, so matching can stay within
        // corresponding key runs instead of scanning the entire other union.
        let mut a_sorted: Vec<TypeId> = a_types.to_vec();
        let mut b_sorted: Vec<TypeId> = b_types.to_vec();
        a_sorted.sort_unstable_by_key(|id| (self.union_member_sort_key(*id).0, id.0));
        b_sorted.sort_unstable_by_key(|id| (self.union_member_sort_key(*id).0, id.0));

        // Fast path: identical TypeId multisets (common after hash-cons).
        if a_sorted == b_sorted {
            return true;
        }

        // Equal keys are only a necessary condition: aliases and numeric
        // coercions still need semantic matching, and duplicate members must
        // consume distinct entries on the other side. A keyed-path failure
        // must fall back because equals can coerce at any nesting depth.
        let mut a_start = 0;
        let mut b_start = 0;
        let keyed_match =
            'keyed: {
                while a_start < a_sorted.len() {
                    let a_key = self.union_member_sort_key(a_sorted[a_start]);
                    let a_end = a_sorted[a_start..]
                        .iter()
                        .position(|id| self.union_member_sort_key(*id) != a_key)
                        .map_or(a_sorted.len(), |offset| a_start + offset);

                    if b_start == b_sorted.len()
                        || self.union_member_sort_key(b_sorted[b_start]) != a_key
                    {
                        break 'keyed false;
                    }
                    let b_end = b_sorted[b_start..]
                        .iter()
                        .position(|id| self.union_member_sort_key(*id) != a_key)
                        .map_or(b_sorted.len(), |offset| b_start + offset);
                    if a_end - a_start != b_end - b_start {
                        break 'keyed false;
                    }

                    let mut matched = vec![false; b_end - b_start];
                    for a_ty in &a_sorted[a_start..a_end] {
                        let Some(index) = b_sorted[b_start..b_end].iter().enumerate().find_map(
                            |(index, b_ty)| {
                                (!matched[index] && self.equals(*a_ty, *b_ty)).then_some(index)
                            },
                        ) else {
                            break 'keyed false;
                        };
                        matched[index] = true;
                    }

                    a_start = a_end;
                    b_start = b_end;
                }
                b_start == b_sorted.len()
            };

        if keyed_match {
            return true;
        }

        self.union_members_equal_semantic(a_types, b_types)
    }

    /// Full semantic multiset matching used when the keyed positive path cannot
    /// establish equality. The key is only an optimization and never proves
    /// inequality because nested coercions can make different keys equal.
    fn union_members_equal_semantic(&self, a_types: &[TypeId], b_types: &[TypeId]) -> bool {
        if a_types.len() != b_types.len() {
            return false;
        }
        if a_types.is_empty() {
            return true;
        }

        let mut a_sorted: Vec<TypeId> = a_types.to_vec();
        let mut b_sorted: Vec<TypeId> = b_types.to_vec();
        a_sorted.sort_unstable_by_key(|id| id.0);
        b_sorted.sort_unstable_by_key(|id| id.0);

        if a_sorted == b_sorted {
            return true;
        }

        let mut matched = vec![false; b_sorted.len()];
        for a_ty in &a_sorted {
            let Some(index) = b_sorted.iter().enumerate().find_map(|(index, b_ty)| {
                (!matched[index] && self.equals(*a_ty, *b_ty)).then_some(index)
            }) else {
                return false;
            };
            matched[index] = true;
        }

        true
    }

    fn type_param_constraint_equal(
        &self,
        a: &TypeParamConstraint,
        b: &TypeParamConstraint,
    ) -> bool {
        match (a, b) {
            (TypeParamConstraint::Any, TypeParamConstraint::Any) => true,
            (TypeParamConstraint::OneOf(a_types), TypeParamConstraint::OneOf(b_types)) => {
                self.union_members_equal(a_types, b_types)
            }
            (TypeParamConstraint::Implet(a_refs), TypeParamConstraint::Implet(b_refs)) => {
                a_refs.len() == b_refs.len()
                    && a_refs.iter().zip(b_refs).all(|(a_ref, b_ref)| {
                        a_ref.contract == b_ref.contract
                            && a_ref.args.len() == b_ref.args.len()
                            && a_ref
                                .args
                                .iter()
                                .zip(&b_ref.args)
                                .all(|(a_arg, b_arg)| self.equals(*a_arg, *b_arg))
                    })
            }
            _ => false,
        }
    }

    fn generic_param_defaults_equal(
        &self,
        a: Option<&SemanticGenericParamDefault>,
        b: Option<&SemanticGenericParamDefault>,
    ) -> bool {
        match (a, b) {
            (
                Some(SemanticGenericParamDefault::Typus(a_ty)),
                Some(SemanticGenericParamDefault::Typus(b_ty)),
            ) => self.equals(*a_ty, *b_ty),
            (
                Some(SemanticGenericParamDefault::Magnitudo(a_value)),
                Some(SemanticGenericParamDefault::Magnitudo(b_value)),
            ) => a_value == b_value,
            (None, None) => true,
            _ => false,
        }
    }

    /// Return whether a value of type `from` can be used where `to` is expected.
    ///
    /// This is a semantic compatibility relation, not identity. It encodes the
    /// language's implicit widening and optional/union acceptance rules while
    /// keeping `ignotum` one-way: concrete values may flow into it, but unknown
    /// values do not silently become concrete.
    #[must_use]
    #[allow(clippy::match_same_arms)]
    pub fn is_assignable(&self, from: TypeId, to: TypeId) -> bool {
        if self.equals(from, to) {
            return true;
        }
        // Directional key: `is_assignable(a, b)` and `is_assignable(b, a)` are
        // distinct queries (widening / optional / `ignotum`-sink rules are
        // one-way), so unlike `equals_memo` the pair is NOT min/max ordered.
        let key = (from, to);
        if let Some(&cached) = self.assignable_memo.borrow().get(&key) {
            return cached;
        }

        let result = self.is_assignable_inner(from, to);
        self.assignable_memo.borrow_mut().insert(key, result);
        result
    }

    /// Unmemoized recursion core of [`Self::is_assignable`].
    ///
    /// Holds the structural match body; recursive calls re-enter the memoized
    /// [`Self::is_assignable`], so every subquery is cached too. Split out so the
    /// memo plumbing does not count toward the pedantic `too_many_lines` gate
    /// on the public entry point.
    #[allow(clippy::match_same_arms, clippy::too_many_lines)]
    fn is_assignable_inner(&self, from: TypeId, to: TypeId) -> bool {
        let from_ty = self.get(from);
        let to_ty = self.get(to);

        match (from_ty, to_ty) {
            // Peel alias wrappers so numeric/instans carrier rules fire on
            // the underlying type.
            (Type::Alias(_, a_inner), _) => self.is_assignable(*a_inner, to),
            (_, Type::Alias(_, b_inner)) => self.is_assignable(from, *b_inner),

            // `ignotum` is an unknown sink: values can flow into it, but not out without cast/narrowing.
            (Type::Primitive(Primitive::Ignotum), _) => false,
            // `ignotum` accepts any concrete value; must fire before tighter rejections.
            (_, Type::Primitive(Primitive::Ignotum)) => true,

            (Type::Union(from_types), _) => from_types
                .iter()
                .all(|member| self.is_assignable(*member, to)),

            (_, Type::Union(to_types)) => to_types
                .iter()
                .any(|member| self.is_assignable(from, *member)),

            // `nihil` can satisfy optional destinations without inventing a concrete inner value.
            (Type::Primitive(Primitive::Nihil), Type::Option(_)) => true,

            (Type::Option(from_inner), Type::Option(to_inner)) => {
                self.is_assignable(*from_inner, *to_inner)
            }

            // WHY: bare `←` may assign `Option<T>` into `T`; Rust codegen unwraps.
            // Tuples are NOT defaultable: rejecting bare unwrap of Option<iuncta<...>>.
            (Type::Option(from_inner), _) if matches!(self.get(*from_inner), Type::Tuple(_)) => {
                false
            }
            (Type::Option(from_inner), _) => self.is_assignable(*from_inner, to),

            (_, Type::Option(inner)) => self.is_assignable(from, *inner),

            // Numeric widening is implicit; narrowing remains an explicit operation.
            (from_ty, to_ty) if is_numeric_carrier(from_ty) && is_numeric_carrier(to_ty) => {
                is_numeric_assignable(self, from, to)
            }

            // WHY: coarse→fine instans widening is lossless; fine→coarse requires `↦`.
            (from_ty, to_ty) if is_instans_carrier(from_ty) && is_instans_carrier(to_ty) => {
                is_instans_assignable(self, from, to)
            }

            // ASCII machine text widens to Unicode `textus`. After D17, `'x'`
            // is `ascii<1>` (`BoundedAscii { 1 }`) and uses the same payload
            // widening. Multi-char `ascii<N>` reaches textus through the
            // explicit D7 bounded-string conversio, not implicit widening.
            (from_ty, Type::Primitive(Primitive::Textus))
                if self.ascii_payload_widens_to_textus_ty(from_ty) =>
            {
                true
            }

            // D10.5 widening chain `ascii<1> → littera → textus`: a one-letter
            // ASCII string is a scalar, and a scalar is a one-letter string, so
            // `s[i] ≡ '\n'` and `textus t ← c` keep working. Bare / multi-char
            // `ascii` never widens to `littera` (it is not one scalar).
            (Type::BoundedAscii { capacity }, Type::Primitive(Primitive::Littera)) => {
                matches!(self.get_index(*capacity), IndexExpr::Literal(1))
            }
            (Type::Primitive(Primitive::Littera), Type::Primitive(Primitive::Textus)) => true,

            // Scalar Faber values widen into dynamic `valor` for captured template params.
            (
                Type::Primitive(
                    Primitive::Nihil
                    | Primitive::Bivalens
                    | Primitive::Numerus
                    | Primitive::Fractus
                    | Primitive::Textus
                    | Primitive::Ascii,
                ),
                Type::Primitive(Primitive::Valor),
            ) => true,
            (
                Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _),
                Type::Primitive(Primitive::Valor),
            ) => true,
            (Type::Array(_) | Type::Map(_, _), Type::Primitive(Primitive::Valor)) => true,
            // Octeti is semantic sugar for `lista<numerus<u8>>` and `lista<T>` widens to
            // valor, so octeti widens to valor too.
            (Type::Primitive(Primitive::Octeti), Type::Primitive(Primitive::Valor)) => true,

            // Structural assignability: type-argument positions are invariant
            // everywhere (ruling 21 row 25, ruling 51 A7). A container of one
            // element type never stands in for a container of another, because
            // the shared storage would be written through the wrong policy.
            // Fresh literals are not affected: their elements are stores into
            // the expected element type and are typed before they get here.

            // Applied: generic application is invariant (type args are structural).
            (Type::Applied(_, _), Type::Applied(_, _)) => self.equals(from, to),

            (Type::Tuple(from_elems), Type::Tuple(to_elems)) => {
                from_elems.len() == to_elems.len()
                    && from_elems
                        .iter()
                        .zip(to_elems.iter())
                        .all(|(f, t)| self.equals(*f, *t))
            }

            // Steps are part of the type identity: assigning across steps is a
            // type error, never a silent conversion.
            (Type::Intervallum(from_bound, from_step), Type::Intervallum(to_bound, to_step)) => {
                self.index_equals(*from_step, *to_step)
                    && self.is_assignable(*from_bound, *to_bound)
            }

            (Type::Array(from_elem), Type::Array(to_elem))
            | (Type::Set(from_elem), Type::Set(to_elem))
            | (Type::Promissum(from_elem), Type::Promissum(to_elem))
            | (Type::Cursor(from_elem), Type::Cursor(to_elem))
            | (Type::Queue(from_elem), Type::Queue(to_elem))
            | (Type::Stack(from_elem), Type::Stack(to_elem)) => self.equals(*from_elem, *to_elem),

            // Infallible promissum<T> widens to promissum<T ⇥ E> (numquam bottom).
            (Type::PromissumFailable(ft, fe), Type::PromissumFailable(tt, te)) => {
                self.equals(*ft, *tt) && self.equals(*fe, *te)
            }
            (Type::Promissum(ft), Type::PromissumFailable(tt, _te)) => self.equals(*ft, *tt),
            // Failable promises do not narrow to infallible (erases the alternate channel).
            (Type::PromissumFailable(_, _), Type::Promissum(_)) => false,

            (Type::AsyncCursor(fi, fe), Type::AsyncCursor(ti, te)) => {
                self.equals(*fi, *ti) && self.equals(*fe, *te)
            }

            (Type::Map(from_key, from_val), Type::Map(to_key, to_val))
            | (Type::Ledger(from_key, from_val), Type::Ledger(to_key, to_val)) => {
                self.equals(*from_key, *to_key) && self.equals(*from_val, *to_val)
            }

            // Indexed containers: element, shape and width all invariant.
            (Type::Tensor(from_elem, from_shape), Type::Tensor(to_elem, to_shape))
            | (Type::Vector(from_elem, from_shape), Type::Vector(to_elem, to_shape))
            | (Type::Matrix(from_elem, from_shape), Type::Matrix(to_elem, to_shape))
            | (Type::Sparsa(from_elem, from_shape), Type::Sparsa(to_elem, to_shape)) => {
                self.equals(*from_elem, *to_elem) && self.index_equals(*from_shape, *to_shape)
            }

            // Bounded buffers: element invariant, capacity invariant except
            // for ascii's directional widening rule. Bare primitives are not
            // assignable to or from bounded forms (D4/D5); the one-byte char
            // carrier is the sole implicit bridge into bare ascii (D17).
            (
                Type::BoundedArray {
                    element: from_elem,
                    capacity: from_cap,
                },
                Type::BoundedArray {
                    element: to_elem,
                    capacity: to_cap,
                },
            ) => self.equals(*from_elem, *to_elem) && self.index_equals(*from_cap, *to_cap),
            (
                Type::RangedArray {
                    element: from_elem,
                    min: from_min,
                    max: from_max,
                },
                Type::RangedArray {
                    element: to_elem,
                    min: to_min,
                    max: to_max,
                },
            ) => self.equals(*from_elem, *to_elem) && from_min == to_min && from_max == to_max,
            (
                Type::BoundedQueue {
                    element: from_elem,
                    capacity: from_cap,
                },
                Type::BoundedQueue {
                    element: to_elem,
                    capacity: to_cap,
                },
            )
            | (
                Type::BoundedStack {
                    element: from_elem,
                    capacity: from_cap,
                },
                Type::BoundedStack {
                    element: to_elem,
                    capacity: to_cap,
                },
            ) => self.equals(*from_elem, *to_elem) && self.index_equals(*from_cap, *to_cap),
            (
                Type::BoundedTextus { capacity: from_cap },
                Type::BoundedTextus { capacity: to_cap },
            )
            | (
                Type::BoundedOcteti { capacity: from_cap },
                Type::BoundedOcteti { capacity: to_cap },
            ) => self.index_equals(*from_cap, *to_cap),
            (
                Type::BoundedAscii { capacity: from_cap },
                Type::BoundedAscii { capacity: to_cap },
            ) => self.is_bounded_ascii_capacity_assignable(*from_cap, *to_cap),
            (Type::BoundedAscii { capacity }, Type::Primitive(Primitive::Ascii)) => {
                self.bounded_ascii_capacity_is_one(*capacity)
            }

            _ => false,
        }
    }

    fn is_bounded_ascii_capacity_assignable(&self, from: IndexId, to: IndexId) -> bool {
        match (self.get_index(from), self.get_index(to)) {
            (IndexExpr::Literal(from), IndexExpr::Literal(to)) => from <= to,
            _ => false,
        }
    }

    fn bounded_ascii_capacity_is_one(&self, capacity: IndexId) -> bool {
        matches!(self.get_index(capacity), IndexExpr::Literal(1))
    }

    /// Bare `ascii` and D17 `'x'` (`ascii<1>`) widen to `textus`.
    ///
    /// Multi-char `ascii<N>` does not widen implicitly; the D7 bounded-string
    /// conversio owns bounded→unbounded conversion explicitly.
    #[must_use]
    pub fn ascii_payload_widens_to_textus(&self, from: TypeId) -> bool {
        self.ascii_payload_widens_to_textus_ty(self.get(from))
    }

    fn ascii_payload_widens_to_textus_ty(&self, from_ty: &Type) -> bool {
        match from_ty {
            Type::Alias(_, inner) => self.ascii_payload_widens_to_textus(*inner),
            Type::Primitive(Primitive::Ascii) => true,
            Type::BoundedAscii { capacity } => {
                matches!(self.get_index(*capacity), IndexExpr::Literal(1))
            }
            _ => false,
        }
    }
}

impl Default for TypeTable {
    fn default() -> Self {
        Self::new()
    }
}

/// One checked argument in a nominal or alias application.
///
/// Type and compile-time index arguments remain distinct all the way through
/// the semantic arena. Unsupported consumers must reject `Size` explicitly
/// rather than manufacturing a `TypeId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppliedArg {
    Type(TypeId),
    Size(IndexId),
}

/// Semantic type shapes understood by Radix after parsing.
///
/// Variants encode compiler meaning rather than one-to-one source spelling.
/// For example, collection names lower into structural collection variants,
/// and user-defined declarations are represented by `DefId` handles into the
/// resolver's symbol table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Type {
    Primitive(Primitive),

    /// Compiler-owned type-only selector for `limes<Policy>()`.
    TensorEdgePolicy(BuiltinTensorEdgePolicy),

    /// `lista<T>` collection.
    Array(TypeId),

    /// `lista<T, magnitudo N>` bounded array. Distinct from unbounded [`Type::Array`].
    BoundedArray {
        element: TypeId,
        capacity: IndexId,
    },

    /// `lista<T, magnitudo lo‥hi>` / `lista<T, magnitudo lo…hi>` length range (need, 5/5):
    /// the list's length stays inside `[min, max]` after every store. `max` is
    /// `None` for an unbounded top end (`lista<T, magnitudo 1…∞>`). `lista<T>` is the
    /// range `0…∞` and `lista<T, magnitudo N>` is `0…N`, so those two are interned as
    /// [`Type::Array`] and [`Type::BoundedArray`], never as this variant: it
    /// names only a range that is neither (see [`TypeTable::ranged_array`]).
    RangedArray {
        element: TypeId,
        min: u64,
        max: Option<u64>,
    },

    /// `queue<T>` FIFO collection.
    Queue(TypeId),

    /// `queue<T, N>` bounded FIFO collection. Distinct from [`Type::Queue`].
    BoundedQueue {
        element: TypeId,
        capacity: IndexId,
    },

    /// `stack<T>` LIFO collection.
    Stack(TypeId),

    /// `stack<T, N>` bounded LIFO collection. Distinct from [`Type::Stack`].
    BoundedStack {
        element: TypeId,
        capacity: IndexId,
    },

    /// `textus<N>` bounded Unicode buffer. Distinct from [`Primitive::Textus`].
    BoundedTextus {
        capacity: IndexId,
    },

    /// `ascii<N>` bounded ASCII buffer. Distinct from [`Primitive::Ascii`].
    BoundedAscii {
        capacity: IndexId,
    },

    /// `octeti<N>` bounded byte buffer. Distinct from [`Primitive::Octeti`].
    BoundedOcteti {
        capacity: IndexId,
    },

    /// `tabula<K, V>` collection.
    Map(TypeId, TypeId),

    /// `tabulae<K, V>` ledger: a map that keeps its entries in insertion order.
    Ledger(TypeId, TypeId),

    /// Structural record type used for compiler-synthesized contracts.
    Record(FxHashMap<LexSymbol, TypeId>),

    /// `copia<T>` collection.
    Set(TypeId),

    /// `promissum<T>` async result handle (infallible: alternate channel is numquam).
    Promissum(TypeId),

    /// `promissum<T ⇥ E>` failable async result handle (two channels: success T, alternate E).
    PromissumFailable(TypeId, TypeId),

    /// `cursor<T>` synchronous iterator element stream.
    Cursor(TypeId),

    /// Async generator stream. Pulling yields a promised step with item and alternate channels.
    AsyncCursor(TypeId, TypeId),

    /// `tensor<T, Figura>` fixed-length container; element type unrestricted under indexed-types stance.
    Tensor(TypeId, IndexId),

    /// `vector<T, size N>` register-class numeric vector; width is a single dimension.
    Vector(TypeId, IndexId),

    /// `matrix<T, [R, C]>` register-class numeric matrix; shape is exactly two dimensions.
    Matrix(TypeId, IndexId),

    /// `sparsa<T, Figura>` sparse tensor; omitted coordinates equal zero; numeric-only in v1.
    Sparsa(TypeId, IndexId),

    /// `atomic<T>` storage cell; V1 permits `i32` and `u32` elements only.
    Atomic(TypeId),

    /// `intervallum<T, S>` numeric interval (bounds share `T`).
    ///
    /// The second field is the compile-time integer step; the one-argument
    /// spelling `intervallum<T>` is step `Literal(1)`.
    Intervallum(TypeId, IndexId),

    /// Sized `numerus<W>` or `fractus<W>` primitive.
    SizedNumeric(Primitive, NumericWidth),

    /// Raw bits `b8` / `b16` / `b32` / `b64`: not a number, no widening to or
    /// from any numeric type or other width. Equal only to the same width.
    Bits(BitsWidth),

    /// Fixed-width unsigned modular word. The first supported width is `u32`.
    ModularWord(NumericWidth),

    /// Fixed-width saturating integer (`saturatus<W>`, en `saturating<W>`):
    /// arithmetic clamps at both ends of `W` (D11.1). Signed or unsigned.
    SaturatingWord(NumericWidth),

    /// Ranged integer cell (`trapping|wrapping|saturating<W, lo‥hi>`): the
    /// width stays the representation, and the range is a refinement a store
    /// applies inside it. Both ends are finite and inclusive
    /// here (`lo…hi`; the half-open `lo‥hi` is stored as `lo…hi-1`), and the
    /// range is strictly inside the width: a range that is the whole width
    /// interns as the plain form (`u8`, `wrapping<u8>`, `saturating<u8>`), so
    /// each existing spelling is the one explicit ranged form it abbreviates.
    /// Constructible only through [`TypeTable::ranged_int`].
    RangedInt {
        policy: IntPolicy,
        width: NumericWidth,
        #[serde(with = "wide_bound")]
        lo: i128,
        #[serde(with = "wide_bound")]
        hi: i128,
    },

    /// Policy-carrying float cell (`trapping<W>` or `saturating<W>` with `W`
    /// a float width, D5 of the N7e delivery). One variant for both policies:
    /// the policy is data. The width is always a float width. Constructible
    /// only through the type table; the front end does not resolve it yet.
    PolicyFloat(FloatPolicy, NumericWidth),

    /// Sized `instans<N>` primitive (`ms`, `us`, `ns`).
    SizedInstans(InstansPrecision),

    /// Optional value type written as `T ∪ nihil` in Faber type positions.
    Option(TypeId),

    Ref(SemanticMutability, TypeId),

    Struct(DefId),

    Enum(DefId),

    Interface(DefId),

    /// Resolved type alias retaining the alias declaration identity.
    Alias(DefId, TypeId),

    Func(FuncSig),

    /// Generic type parameter that has not been substituted.
    Param(LexSymbol),

    Applied(TypeId, Vec<AppliedArg>),

    Infer(InferVar),

    /// Union-shaped inference hole written as a standalone `∪` in type
    /// position. Distinct from `Infer` (monomorphic hole) and `Union`
    /// (explicit member list): solving accumulates member types into the
    /// variable instead of binding one inhabitant.
    InferUnion(InferVar),

    /// Family-preserving marker hole: `numerus<_>`, `fractus<_>`, `modulus<_>`,
    /// `instans<_>`. The family is identity; only the marker is inferred.
    /// Distinct from [`Type::Infer`] so an unsolved hole cannot reach a
    /// width-position match.
    MarkerHole {
        family: MarkerFamily,
        var: InferVar,
    },

    Union(Vec<TypeId>),

    /// Nominal intersection: `Readable ∩ Seekable`. Members are canonicalized
    /// BO-S-4-identically to [`Type::Union`] (sorted by `TypeId` ordinal,
    /// deduplicated, single-member degenerate collapsed at lowering). Operands
    /// are nominal-only (interfaces/structs); primitive meets are rejected at
    /// lowering, not modeled here.
    Intersection(Vec<TypeId>),

    /// Ordered positional aggregate.
    Tuple(Vec<TypeId>),

    /// Recovery sentinel used after an earlier type error.
    Error,
}

impl Hash for Type {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Primitive(p) => p.hash(state),
            Self::TensorEdgePolicy(policy) => policy.hash(state),
            Self::Array(t)
            | Self::Set(t)
            | Self::Promissum(t)
            | Self::Cursor(t)
            | Self::Atomic(t)
            | Self::Option(t) => t.hash(state),
            Self::Intervallum(t, step) => {
                t.hash(state);
                step.hash(state);
            }
            Self::PromissumFailable(t, e) | Self::AsyncCursor(t, e) => {
                t.hash(state);
                e.hash(state);
            }
            Self::Map(k, v) | Self::Ledger(k, v) => {
                k.hash(state);
                v.hash(state);
            }
            Self::Record(fields) => {
                // FxHashMap has no Hash; hash fields in sorted key order.
                let mut items: Vec<(LexSymbol, TypeId)> =
                    fields.iter().map(|(&k, &v)| (k, v)).collect();
                items.sort_unstable_by_key(|(k, _)| k.0);
                items.len().hash(state);
                for (k, v) in items {
                    k.hash(state);
                    v.hash(state);
                }
            }
            Self::Tensor(e, s) | Self::Vector(e, s) | Self::Matrix(e, s) | Self::Sparsa(e, s) => {
                e.hash(state);
                s.hash(state);
            }
            Self::BoundedArray { element, capacity }
            | Self::BoundedQueue { element, capacity }
            | Self::BoundedStack { element, capacity } => {
                element.hash(state);
                capacity.hash(state);
            }
            Self::RangedArray { element, min, max } => {
                element.hash(state);
                min.hash(state);
                max.hash(state);
            }
            Self::Queue(element) | Self::Stack(element) => element.hash(state),
            Self::BoundedTextus { capacity }
            | Self::BoundedAscii { capacity }
            | Self::BoundedOcteti { capacity } => capacity.hash(state),
            Self::SizedNumeric(p, w) => {
                p.hash(state);
                w.hash(state);
            }
            Self::Bits(w) => w.hash(state),
            Self::ModularWord(w) | Self::SaturatingWord(w) => w.hash(state),
            Self::RangedInt {
                policy,
                width,
                lo,
                hi,
            } => {
                policy.hash(state);
                width.hash(state);
                lo.hash(state);
                hi.hash(state);
            }
            Self::PolicyFloat(policy, w) => {
                policy.hash(state);
                w.hash(state);
            }
            Self::SizedInstans(p) => p.hash(state),
            Self::Ref(m, t) => {
                m.hash(state);
                t.hash(state);
            }
            Self::Struct(d) | Self::Enum(d) | Self::Interface(d) => d.hash(state),
            Self::Alias(d, t) => {
                d.hash(state);
                t.hash(state);
            }
            Self::Func(sig) => sig.hash(state),
            Self::Param(name) => name.hash(state),
            Self::Applied(base, args) => {
                base.hash(state);
                args.hash(state);
            }
            Self::Infer(v) | Self::InferUnion(v) => v.hash(state),
            Self::MarkerHole { family, var } => {
                family.hash(state);
                var.hash(state);
            }
            Self::Union(members) | Self::Tuple(members) | Self::Intersection(members) => {
                members.hash(state);
            }
            Self::Error => {}
        }
    }
}

/// Builtin tensor-edge policy identities. These markers are accepted as the
/// `limes` type argument and are never runtime values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BuiltinTensorEdgePolicy {
    Clamp,
    Reflect,
    Wrap,
}

impl BuiltinTensorEdgePolicy {
    pub const ALL: [Self; 3] = [Self::Clamp, Self::Reflect, Self::Wrap];

    /// Canonical Latin source type spelling.
    #[must_use]
    pub const fn latin(self) -> &'static str {
        match self {
            Self::Clamp => "clampa",
            Self::Reflect => "reflexa",
            Self::Wrap => "involuta",
        }
    }

    /// Compiler identity for a canonical Latin source type spelling.
    #[must_use]
    pub fn from_latin(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|policy| policy.latin() == text)
    }
}

/// Wire form of a ranged-integer end. The ends are `i128` in memory (a `u64`
/// width reaches `2^64 - 1`, past `i64`), but the msgpack envelope has no
/// 128-bit integer, so an end travels as its two 64-bit halves: the signed
/// high word, then the unsigned low word.
mod wide_bound {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S: Serializer>(value: &i128, serializer: S) -> Result<S::Ok, S::Error> {
        let high = i64::try_from(*value >> 64).unwrap_or_default();
        let low = u64::try_from(*value & i128::from(u64::MAX)).unwrap_or_default();
        (high, low).serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<i128, D::Error> {
        let (high, low) = <(i64, u64)>::deserialize(deserializer)?;
        Ok((i128::from(high) << 64) | i128::from(low))
    }
}

/// Overflow policy of a [`Type::RangedInt`] cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IntPolicy {
    /// `trapping<W, lo‥hi>`: a store outside the range traps.
    Trapping,
    /// `wrapping<W, lo‥hi>`: a store reduces modulo `hi - lo + 1`, offset by `lo`.
    Wrapping,
    /// `saturating<W, lo‥hi>`: a store clamps into the range.
    Saturating,
}

impl IntPolicy {
    /// Policy word used in diagnostics and pretty-print.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Trapping => "trapping",
            Self::Wrapping => "wrapping",
            Self::Saturating => "saturating",
        }
    }

    /// Canonical Latin identity of the policy word (`exactus`, `modulus`, `saturatus`).
    #[must_use]
    pub fn latin(self) -> &'static str {
        match self {
            Self::Trapping => "exactus",
            Self::Wrapping => "modulus",
            Self::Saturating => "saturatus",
        }
    }
}

/// Overflow policy of a [`Type::PolicyFloat`] cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FloatPolicy {
    /// `trapping<W>`: a non-finite value stored into the cell traps.
    Trapping,
    /// `saturating<W>`: evaluates as the bare float; the cell is a type identity.
    Saturating,
}

impl FloatPolicy {
    /// Policy word used in diagnostics and pretty-print (`trapping`/`saturating`).
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Trapping => "trapping",
            Self::Saturating => "saturating",
        }
    }
}

/// Built-in primitive and special semantic types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Primitive {
    /// ASCII-only machine text.
    Ascii,

    Textus,

    Numerus,

    Fractus,

    Bivalens,

    /// Null value; nullable slots are modeled with option/union types.
    Nihil,

    Vacuum,

    /// Never-returning control-flow type.
    Numquam,

    /// Unknown escape type for interop or intentionally unchecked values.
    Ignotum,

    /// Raw byte buffer: the internal carrier of `lista<b8>`. It has no
    /// source-level type spelling (the `octeti`/`bytes` word is retired), so it
    /// has no `PrimitiveTypeSpec` row and no locale row.
    Octeti,

    Regex,

    /// One successful regex match (RD-3): internal only, with no source-level
    /// type spelling. It arises from a `regex` operation and is never named in
    /// a type position, so it has no `PrimitiveTypeSpec` row and no locale row.
    Match,

    /// Object-rooted JSON document, backed by `faber::Json`.
    Json,

    /// Canonical dynamic data value for data formats, backed by `faber::Valor`.
    Valor,

    /// Absolute point-in-time (bare spelling = Unix seconds).
    Instans,

    /// One Unicode scalar value (D10.2): Rust `char`, Go `rune`. Four bytes,
    /// never allocates. Element of `textus` / `ascii` iteration and of
    /// `ascii[i]`; `ascii<1> → littera → textus` widen implicitly (D10.5).
    Littera,
}

impl Primitive {
    /// Resolve a source-level primitive spelling to its semantic primitive.
    ///
    /// `objectum` and `quidlibet` remain accepted aliases for the unknown
    /// escape type; they do not introduce separate semantic types.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        lookup_primitive_type_spec(name).map(|spec| spec.primitive)
    }
}

/// Reader-surface row for one primitive type spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrimitiveTypeSpec {
    /// Source spelling accepted in type positions.
    pub text: &'static str,

    /// Semantic primitive identity represented by this spelling.
    pub primitive: Primitive,

    /// Whether this spelling is the canonical Latin surface row.
    pub canonical: bool,
}

/// Latin primitive type surface used before TOML reader packs are loaded.
pub static PRIMITIVE_TYPE_SPECS: &[PrimitiveTypeSpec] = &[
    PrimitiveTypeSpec {
        text: "ascii",
        primitive: Primitive::Ascii,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "textus",
        primitive: Primitive::Textus,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "littera",
        primitive: Primitive::Littera,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "numerus",
        primitive: Primitive::Numerus,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "fractus",
        primitive: Primitive::Fractus,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "bivalens",
        primitive: Primitive::Bivalens,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "nihil",
        primitive: Primitive::Nihil,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "vacuum",
        primitive: Primitive::Vacuum,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "numquam",
        primitive: Primitive::Numquam,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "ignotum",
        primitive: Primitive::Ignotum,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "regex",
        primitive: Primitive::Regex,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "json",
        primitive: Primitive::Json,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "valor",
        primitive: Primitive::Valor,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "instans",
        primitive: Primitive::Instans,
        canonical: true,
    },
    PrimitiveTypeSpec {
        text: "objectum",
        primitive: Primitive::Ignotum,
        canonical: false,
    },
    PrimitiveTypeSpec {
        text: "quidlibet",
        primitive: Primitive::Ignotum,
        canonical: false,
    },
];

/// Return primitive type rows in deterministic declaration order.
#[must_use]
pub fn primitive_type_specs() -> &'static [PrimitiveTypeSpec] {
    PRIMITIVE_TYPE_SPECS
}

/// Look up a primitive type spelling in the active static Latin surface.
#[must_use]
pub fn lookup_primitive_type_spec(text: &str) -> Option<&'static PrimitiveTypeSpec> {
    PRIMITIVE_TYPE_SPECS.iter().find(|spec| spec.text == text)
}

/// Source-level collection families with fixed generic arity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectionKind {
    Lista,
    Tabula,
    /// The ledger: an insertion-ordered map. The spelling `tabulae` sits one
    /// letter from `tabula` on purpose ("two shades of the same thing"; Roman
    /// account books were called tabulae). It is not a typo; do not "fix" it.
    Tabulae,
    Copia,
    Promissum,
    Cursor,
    Queue,
    Stack,
}

impl CollectionKind {
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        lookup_collection_type_spec(name).map(|spec| spec.kind)
    }

    #[must_use]
    pub fn arity(self) -> usize {
        match self {
            Self::Lista
            | Self::Copia
            | Self::Promissum
            | Self::Cursor
            | Self::Queue
            | Self::Stack => 1,
            Self::Tabula | Self::Tabulae => 2,
        }
    }

    /// Whether `n` is a legal applied-argument count for this family.
    ///
    /// `lista`, `queue`, and `stack` accept one type argument or a type plus a
    /// magnitudo capacity. Other families stay exact.
    #[must_use]
    pub fn accepts_arity(self, n: usize) -> bool {
        match self {
            Self::Lista | Self::Queue | Self::Stack => n == 1 || n == 2,
            other => n == other.arity(),
        }
    }

    /// Diagnostic text for incorrect generic arity.
    #[must_use]
    pub fn arity_error(self) -> &'static str {
        match self {
            Self::Lista => "lista requires one or two type parameters",
            Self::Tabula => "tabula requires two type parameters",
            Self::Tabulae => "tabulae requires two type parameters",
            Self::Copia => "copia requires one type parameter",
            Self::Promissum => "promissum requires one type parameter",
            Self::Cursor => "cursor requires one type parameter",
            Self::Queue => "queue requires one or two type parameters",
            Self::Stack => "stack requires one or two type parameters",
        }
    }

    pub fn lower(self, types: &mut TypeTable, params: &[TypeId]) -> TypeId {
        debug_assert_eq!(params.len(), self.arity());
        match self {
            Self::Lista => types.array(params[0]),
            Self::Tabula => types.map(params[0], params[1]),
            Self::Tabulae => types.ledger(params[0], params[1]),
            Self::Copia => types.set(params[0]),
            Self::Promissum => types.promissum(params[0]),
            Self::Cursor => types.cursor(params[0]),
            Self::Queue => types.queue(params[0]),
            Self::Stack => types.stack(params[0]),
        }
    }
}

/// Reader-surface row for one collection type spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollectionTypeSpec {
    /// Source spelling accepted in type positions.
    pub text: &'static str,

    /// Semantic collection family represented by this spelling.
    pub kind: CollectionKind,

    /// Whether this spelling is the canonical Latin surface row.
    pub canonical: bool,
}

/// Latin collection type surface used before TOML reader packs are loaded.
pub static COLLECTION_TYPE_SPECS: &[CollectionTypeSpec] = &[
    CollectionTypeSpec {
        text: "lista",
        kind: CollectionKind::Lista,
        canonical: true,
    },
    CollectionTypeSpec {
        text: "tabula",
        kind: CollectionKind::Tabula,
        canonical: true,
    },
    // `tabulae` (ledger) is one letter from `tabula` (map) by design: "two
    // shades of the same thing" (operator ruling 2026-10-10, need).
    CollectionTypeSpec {
        text: "tabulae",
        kind: CollectionKind::Tabulae,
        canonical: true,
    },
    CollectionTypeSpec {
        text: "copia",
        kind: CollectionKind::Copia,
        canonical: true,
    },
    CollectionTypeSpec {
        text: "promissum",
        kind: CollectionKind::Promissum,
        canonical: true,
    },
    CollectionTypeSpec {
        text: "cursor",
        kind: CollectionKind::Cursor,
        canonical: true,
    },
    CollectionTypeSpec {
        text: "queue",
        kind: CollectionKind::Queue,
        canonical: true,
    },
    CollectionTypeSpec {
        text: "stack",
        kind: CollectionKind::Stack,
        canonical: true,
    },
];

/// Return collection type rows in deterministic declaration order.
#[must_use]
pub fn collection_type_specs() -> &'static [CollectionTypeSpec] {
    COLLECTION_TYPE_SPECS
}

/// Look up a collection type spelling in the active static Latin surface.
#[must_use]
pub fn lookup_collection_type_spec(text: &str) -> Option<&'static CollectionTypeSpec> {
    COLLECTION_TYPE_SPECS.iter().find(|spec| spec.text == text)
}

/// Source-level tensor-family type identities with dedicated shape/width
/// lowering.
///
/// Parallel to [`CollectionKind`] but deliberately separate: collections lower
/// through `CollectionKind::lower(types, &[TypeId])`, while the tensor family
/// carries dedicated shape/width lowering (`Type::Tensor(elem, shape)`,
/// `Type::Vector(elem, width)`, …) that a flat type-argument list cannot
/// express. Pack completeness and `type_surface` need a closed kind set, so
/// the family gets its own explicit kind rather than free-form keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TensorFamilyKind {
    Tensor,
    Vector,
    Matrix,
    Sparsa,
    Atomic,
    Intervallum,
}

/// Reader-surface row for one tensor-family type spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TensorFamilyTypeSpec {
    /// Source spelling accepted in type positions.
    pub text: &'static str,

    /// Semantic tensor-family identity represented by this spelling.
    pub kind: TensorFamilyKind,

    /// Whether this spelling is the canonical Latin surface row.
    pub canonical: bool,
}

/// Latin tensor-family type surface used before TOML reader packs are loaded.
pub static TENSOR_FAMILY_TYPE_SPECS: &[TensorFamilyTypeSpec] = &[
    TensorFamilyTypeSpec {
        text: "tensor",
        kind: TensorFamilyKind::Tensor,
        canonical: true,
    },
    TensorFamilyTypeSpec {
        text: "vector",
        kind: TensorFamilyKind::Vector,
        canonical: true,
    },
    TensorFamilyTypeSpec {
        text: "matrix",
        kind: TensorFamilyKind::Matrix,
        canonical: true,
    },
    TensorFamilyTypeSpec {
        text: "sparsa",
        kind: TensorFamilyKind::Sparsa,
        canonical: true,
    },
    TensorFamilyTypeSpec {
        text: "atomic",
        kind: TensorFamilyKind::Atomic,
        canonical: true,
    },
    TensorFamilyTypeSpec {
        text: "intervallum",
        kind: TensorFamilyKind::Intervallum,
        canonical: true,
    },
];

/// Return tensor-family type rows in deterministic declaration order.
#[must_use]
pub fn tensor_family_type_specs() -> &'static [TensorFamilyTypeSpec] {
    TENSOR_FAMILY_TYPE_SPECS
}

/// Look up a tensor-family type spelling in the active static Latin surface.
#[must_use]
pub fn lookup_tensor_family_type_spec(text: &str) -> Option<&'static TensorFamilyTypeSpec> {
    TENSOR_FAMILY_TYPE_SPECS
        .iter()
        .find(|spec| spec.text == text)
}

fn is_numeric_carrier(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Primitive(Primitive::Numerus | Primitive::Fractus)
            | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
            | Type::ModularWord(_)
            | Type::SaturatingWord(_)
            | Type::RangedInt { .. }
            | Type::PolicyFloat(_, _)
    )
}

/// Diagnostic when `intervallum<T>` is declared with an unsupported bound `T`.
pub const INTERVALLUM_BOUND_MSG: &str = "intervallum bound type must be numerus or instans";

/// v1 `intervallum<T>` bound types: `numerus` (default + sized) and `instans` (default + sized).
#[must_use]
pub fn is_intervallum_bound_type(types: &TypeTable, ty: TypeId) -> bool {
    matches!(
        types.get(ty),
        Type::Primitive(Primitive::Numerus | Primitive::Instans)
            | Type::SizedNumeric(Primitive::Numerus, _)
            | Type::SizedInstans(_)
    )
}

/// Diagnostic when `atomic<T>` is declared with an unsupported element `T`.
pub const ATOMIC_ELEMENT_MSG: &str = "atomic element type must be i32 or u32";

/// v1 `atomic<T>` element types: storage-class signed and unsigned 32-bit integers.
#[must_use]
pub fn is_atomic_element_type(types: &TypeTable, ty: TypeId) -> bool {
    matches!(
        types.get(ty),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::I32 | NumericWidth::U32)
    )
}

fn is_instans_carrier(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Primitive(Primitive::Instans) | Type::SizedInstans(_)
    )
}

/// Check that every `IndexId` embedded in an `IndexExpr` is within bounds.
///
/// Used by [`TypeTable::from_snapshot`] alongside [`type_refs_in_bounds`] so
/// that nested `IndexExpr::Tuple` children cannot smuggle dangling references
/// past the snapshot integrity check.
fn index_refs_in_bounds(expr: &IndexExpr, indices_len: usize) -> bool {
    let index_in_bounds = |id: &IndexId| (id.0 as usize) < indices_len;
    match expr {
        IndexExpr::Tuple(dims) => dims.iter().all(index_in_bounds),
        IndexExpr::Affine { base, .. } => index_in_bounds(base),
        IndexExpr::Divide { numerator, .. } => index_in_bounds(numerator),
        IndexExpr::Literal(_)
        | IndexExpr::Param(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Infer(_)
        | IndexExpr::Unspecified => true,
    }
}

/// Check that every `TypeId` and `IndexId` embedded in `ty` is within bounds.
///
/// Used by [`TypeTable::from_snapshot`] to reject corrupt snapshots that
/// reference entries beyond the serialized arena.
fn type_refs_in_bounds(ty: &Type, types_len: usize, indices_len: usize) -> bool {
    let type_in_bounds = |id: &TypeId| (id.0 as usize) < types_len;
    let index_in_bounds = |id: &IndexId| (id.0 as usize) < indices_len;
    match ty {
        Type::Array(e)
        | Type::Set(e)
        | Type::Promissum(e)
        | Type::Cursor(e)
        | Type::Queue(e)
        | Type::Stack(e)
        | Type::Option(e)
        | Type::Atomic(e)
        | Type::Ref(_, e)
        | Type::Alias(_, e) => type_in_bounds(e),
        Type::Map(k, v)
        | Type::Ledger(k, v)
        | Type::PromissumFailable(k, v)
        | Type::AsyncCursor(k, v) => type_in_bounds(k) && type_in_bounds(v),
        Type::Intervallum(e, step) => type_in_bounds(e) && index_in_bounds(step),
        Type::Record(fields) => fields.values().all(type_in_bounds),
        Type::Tuple(elems) => elems.iter().all(type_in_bounds),
        Type::Union(members) | Type::Intersection(members) => members.iter().all(type_in_bounds),
        Type::Applied(base, args) => {
            type_in_bounds(base)
                && args.iter().all(|arg| match arg {
                    AppliedArg::Type(id) => type_in_bounds(id),
                    AppliedArg::Size(id) => index_in_bounds(id),
                })
        }
        Type::Func(sig) => {
            sig.params.iter().all(|p| type_in_bounds(&p.ty))
                && type_in_bounds(&sig.ret)
                && sig.err.is_none_or(|e| type_in_bounds(&e))
                && sig.type_params.iter().all(|param| match &param.constraint {
                    TypeParamConstraint::Any => true,
                    TypeParamConstraint::OneOf(ids) => ids.iter().all(type_in_bounds),
                    TypeParamConstraint::Implet(refs) => refs
                        .iter()
                        .all(|contract| contract.args.iter().all(type_in_bounds)),
                })
        }
        Type::Tensor(e, s) | Type::Vector(e, s) | Type::Matrix(e, s) | Type::Sparsa(e, s) => {
            type_in_bounds(e) && index_in_bounds(s)
        }
        Type::BoundedArray { element, capacity }
        | Type::BoundedQueue { element, capacity }
        | Type::BoundedStack { element, capacity } => {
            type_in_bounds(element) && index_in_bounds(capacity)
        }
        Type::RangedArray { element, .. } => type_in_bounds(element),
        Type::BoundedTextus { capacity }
        | Type::BoundedAscii { capacity }
        | Type::BoundedOcteti { capacity } => index_in_bounds(capacity),
        Type::Primitive(_)
        | Type::TensorEdgePolicy(_)
        | Type::SizedNumeric(_, _)
        | Type::Bits(_)
        | Type::ModularWord(_)
        | Type::SaturatingWord(_)
        | Type::RangedInt { .. }
        | Type::PolicyFloat(_, _)
        | Type::SizedInstans(_)
        | Type::Struct(_)
        | Type::Enum(_)
        | Type::Interface(_)
        | Type::Param(_)
        | Type::Infer(_)
        | Type::InferUnion(_)
        | Type::MarkerHole { .. }
        | Type::Error => true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticMutability {
    Immutable,
    Mutable,
    /// Exclusive value (`own` / `penes`).
    Own,
    /// Duplicate-then-own (`copy` / `exemplum`).
    Copy,
}

/// Access-discipline marker fact stored in the type table.
///
/// `ref`/`mut` re-spell the existing `De`/`In` identities. `own`/`copy` are
/// new identities. All four wrap the inner type as [`Type::Ref`] plus the
/// matching [`SemanticMutability`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccessMarker {
    /// `de` / `ref`.
    Ref,
    /// `in` / `mut`.
    Mut,
    /// `own` / `penes`.
    Own,
    /// `copy` / `exemplum`.
    Copy,
}

impl AccessMarker {
    /// Type-table mutability used to intern this marker as [`Type::Ref`].
    #[must_use]
    pub fn mutability(self) -> SemanticMutability {
        match self {
            Self::Ref => SemanticMutability::Immutable,
            Self::Mut => SemanticMutability::Mutable,
            Self::Own => SemanticMutability::Own,
            Self::Copy => SemanticMutability::Copy,
        }
    }
}

impl SemanticMutability {
    /// Marker identity stored by this [`Type::Ref`] wrapper.
    #[must_use]
    pub fn access_marker(self) -> AccessMarker {
        match self {
            Self::Immutable => AccessMarker::Ref,
            Self::Mutable => AccessMarker::Mut,
            Self::Own => AccessMarker::Own,
            Self::Copy => AccessMarker::Copy,
        }
    }
}

/// Semantic callable contract.
///
/// The signature captures parameter passing modes, return type, optional error
/// type, and async/generator modifiers so backends and analyses can reason
/// about calls without re-reading syntax annotations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FuncSig {
    /// Ordered declaration row of generic parameters.
    ///
    /// Sort, constraint, and default travel with each name so no consumer has
    /// to reconstruct the kind from where the name occurs in the signature.
    pub type_params: Vec<GenericParamDescriptor>,
    pub params: Vec<ParamType>,
    pub ret: TypeId,
    pub err: Option<TypeId>,
    pub is_async: bool,
    pub is_generator: bool,
}

/// Declared sort of one generic parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GenericParamSort {
    /// Type parameter (`T`, `U`, …).
    Typus,
    /// Compile-time size/index parameter (`magnitudo N`).
    Magnitudo,
}

/// Declaration-time `=` default of one generic parameter.
///
/// The variant always matches the parameter's [`GenericParamSort`]; the parser
/// enforces the pairing at the source spelling.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticGenericParamDefault {
    /// Type default, legal on [`GenericParamSort::Typus`] parameters.
    Typus(TypeId),
    /// Natural-literal default, legal on [`GenericParamSort::Magnitudo`]
    /// parameters.
    Magnitudo(u64),
}

/// One ordered generic parameter of a callable contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GenericParamDescriptor {
    pub name: LexSymbol,
    pub sort: GenericParamSort,
    /// Source-declared admission rule for concrete type arguments.
    pub constraint: TypeParamConstraint,
    pub default: Option<SemanticGenericParamDefault>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypeParamConstraint {
    Any,
    OneOf(Vec<TypeId>),
    /// `T implet C<…> ∩ D<…>` (DECISIONS D1.1): every witness must nominally
    /// declare each contract with these arguments (D1.3). Arguments may
    /// mention the callable's own type parameters.
    Implet(Vec<ContractRef>),
}

/// One contract named by an `implet` clause, with its type arguments.
///
/// A genus header (`genus Persona implet Orderable<Persona>`) and a type
/// parameter bound (`T implet Orderable<T>`) share this shape (D1.1/D1.2).
/// `args` is empty for a non-generic contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContractRef {
    pub contract: DefId,
    pub args: Vec<TypeId>,
}

/// How the call binder resolved one generic parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GenericBindingSource {
    /// Bound by the caller's explicit generic prefix.
    Explicit,
    /// Solved from value arguments, receiver shapes, or the expected result.
    Inferred,
    /// Still unbound after inference; the declaration's `=` default fired.
    Defaulted,
}

/// Resolved witness of one generic parameter at a checked call.
///
/// The variant matches the parameter's declared [`GenericParamSort`]: type
/// parameters bind a type, size parameters bind an index expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GenericWitness {
    Typus(TypeId),
    Magnitudo(IndexId),
}

/// One parameter's resolved binding, in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResolvedGenericBinding {
    pub name: LexSymbol,
    pub source: GenericBindingSource,
    pub witness: GenericWitness,
}

/// Complete ordered resolved generic vector for one checked generic call.
///
/// Recorded by the typecheck binder so downstream specialization never has to
/// repeat inference or guess which defaults fired. Rows accumulate in call
/// checking order; the span identifies the call site within the source module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedGenericCall {
    pub call_span: radix_lexer::Span,
    /// Resolved declaration identity when the call was a direct named call.
    /// Older artifact readers may omit this field; the span remains useful for
    /// diagnostics and for callers that do not need instance planning.
    #[serde(default)]
    pub callee: Option<DefId>,
    pub bindings: Vec<ResolvedGenericBinding>,
}

/// Type and passing policy for one function parameter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParamType {
    pub ty: TypeId,
    pub mode: SemanticParamMode,
    /// Whether callers may omit this argument (a `sponte` marker or a default).
    pub optional: bool,
    /// Whether the parameter is a source `sponte` slot: an optional value may be
    /// stored into it. A merely defaulted parameter is omittable but not sponte.
    pub sponte: bool,
}

/// Parameter ownership/borrowing mode as seen by semantic analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SemanticParamMode {
    Owned,
    Ref,
    MutRef,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InferVar(pub u32);

/// Closed-marker family for [`Type::MarkerHole`].
///
/// Width and precision enums stay closed; this names which family the hole
/// belongs to so `modulus<_>` cannot become `saturatus<u8>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarkerFamily {
    Modulus,
    Saturatus,
    Instans,
}

#[cfg(test)]
#[path = "types_test.rs"]
mod types_test;

#[cfg(test)]
#[path = "aliasing_container_test.rs"]
mod aliasing_container_test;
