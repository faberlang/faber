//! Provider-neutral `filtrum` plan model (census-types S3-U1).
//!
//! The intensional relational plan as pure data
//! (docs/design/relational-views.md §3, §8): explicit source identity, the
//! input/output schema identities `I` and `O`, the closed predicate AST
//! (§3.2), ordered sort rows, the non-negative limit/offset window, and the
//! declared provider capability requirements.
//!
//! The model is pure data by contract (§1.3 follow-on markers): no field can
//! carry generated SQL text, a provider connection, an arbitrary closure, or
//! a mutation operation. Equality and hashing derive from provider-neutral
//! fields only — generated SQL is never semantic identity (§8.1).
//!
//! This module owns the closed data shape. Construction checks (the `O ⊆ I`
//! subset proof via [`crate::schema_subset`], operator/type admissibility,
//! non-empty conjunctions) are the semantic unit's surface (S3-U2).

use crate::schema::SchemaDef;
use radix_lexer::Symbol as LexSymbol;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

/// Clean-break encoding version carried by every v1 plan (D6 identity
/// component 1; design §8.1, §8.4). A saved plan carrying any other version
/// fails closed at load — no migration shim exists in v1.
pub const FILTRUM_ENCODING_VERSION_V1: u32 = 1;

/// Explicit relation/model identity — the plan's `source` field (design §3.1;
/// D4 §2.4.3). v1 sources are statically declared; the identity is never
/// interpolated as unchecked SQL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceIdentity(pub LexSymbol);

/// Typed column reference — names a column of the input contract `I` (design
/// §3.2). The referenced column's Faber type resolves against `I` at plan
/// construction (S3-U2); `I \ O` source-only predicate and order columns are
/// legal and never surface in the result (design §2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColumnRef(pub LexSymbol);

/// Locked comparison operators (design §3.2 `Compare` row). The six-member
/// set is closed: no operator outside it enters a portable plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ComparisonOp {
    /// `eq` (`=`).
    Eq,
    /// `ne` (`≢`).
    Ne,
    /// `gt` (`>`).
    Gt,
    /// `gte` (`≥`).
    Gte,
    /// `lt` (`<`).
    Lt,
    /// `lte` (`≤`).
    Lte,
}

/// Concrete scalar predicate operand (design §3.2 value roles). The variant
/// set is exactly D5's five type-identity rows (design §6.3); `T ∪ nihil` is
/// a nullability modifier, never a value — `nihil` never appears as a value
/// operand, and null tests are expressed only through [`Predicate::IsNull`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PredicateValue {
    /// `numerus` scalar (i64 carrier; design §6.3).
    Numerus(i64),
    /// `textus` scalar.
    Textus(String),
    /// `bivalens` scalar.
    Bivalens(bool),
    /// `instans` scalar in the provider-neutral RFC3339 wire format (design
    /// §6.3 `instans` row).
    Instans(String),
    /// `fractus` scalar (IEEE-754 double; design §6.3 `fractus` row).
    Fractus(f64),
}

impl PartialEq for PredicateValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Numerus(a), Self::Numerus(b)) => a == b,
            (Self::Textus(a), Self::Textus(b)) | (Self::Instans(a), Self::Instans(b)) => a == b,
            (Self::Bivalens(a), Self::Bivalens(b)) => a == b,
            (Self::Fractus(a), Self::Fractus(b)) => {
                canonical_fractus_bits(*a) == canonical_fractus_bits(*b)
            }
            _ => false,
        }
    }
}

impl Eq for PredicateValue {}

impl Hash for PredicateValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Numerus(v) => v.hash(state),
            Self::Textus(v) | Self::Instans(v) => v.hash(state),
            Self::Bivalens(v) => v.hash(state),
            Self::Fractus(v) => canonical_fractus_bits(*v).hash(state),
        }
    }
}

/// Canonical bit form of an IEEE-754 double (design §6.3 `fractus` row):
/// negative zero collapses to positive zero and every NaN collapses to one
/// bit pattern, so equality and hashing over `fractus` values are total and
/// derive from the value alone.
fn canonical_fractus_bits(value: f64) -> u64 {
    if value.is_nan() {
        // Canonical quiet NaN — all NaN payloads compare equal.
        0x7FF8_0000_0000_0000
    } else if value == 0.0 {
        // IEEE: -0.0 == 0.0; one canonical bit form keeps equality total.
        0
    } else {
        value.to_bits()
    }
}

/// Declared provider capability requirement (design §6.2 `provider-specific`
/// row; goal.md §PostgreSQL/§SQLite). A plan that declares one binds itself
/// to the owning provider and fails closed on the other; the portable core
/// itself never requests one (§6.2 `rejected` row — the portable AST has no
/// expressible array/full-text request).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityRequirement {
    /// `PostgreSQL` regex operators.
    Regex,
    /// `PostgreSQL` arrays.
    Arrays,
    /// `PostgreSQL` full-text search.
    FullText,
    /// `SQLite` JSON-array handling per its adapter.
    JsonArray,
}

/// Closed predicate expression tree (design §3.2, LOCKED). Exactly the eight
/// variants below — no operator outside this set enters a portable plan, and
/// no raw SQL text or arbitrary Faber closure is representable (§1.3).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Predicate {
    /// `Compare(column, op, value)` — a scalar comparison against a column of
    /// `I`; `value` is a concrete scalar of the column's Faber type, never
    /// `nihil`.
    Compare {
        /// Column of `I` being compared.
        column: ColumnRef,
        /// Comparison operator.
        op: ComparisonOp,
        /// Concrete scalar value of the column's type.
        value: PredicateValue,
    },
    /// `Between(column, low, high)` — inclusive bounds `low ≤ column ≤ high`
    /// over an ordering-admissible column of `I` (design §3.3).
    Between {
        /// Column of `I` being range-tested.
        column: ColumnRef,
        /// Inclusive lower bound of the column's type.
        low: PredicateValue,
        /// Inclusive upper bound of the column's type.
        high: PredicateValue,
    },
    /// `In(column, values)` — a finite, non-empty, homogeneous list of the
    /// column's type (equality disjunction; design §3.2).
    In {
        /// Column of `I` being tested for membership.
        column: ColumnRef,
        /// Non-empty value list of the column's type.
        values: Vec<PredicateValue>,
    },
    /// `Like(column, pattern)` — a `textus` column of `I`; `pattern` is a
    /// `textus` literal carrying a SQL `LIKE` pattern (not a regex).
    Like {
        /// `textus` column of `I` being pattern-tested.
        column: ColumnRef,
        /// SQL `LIKE` pattern literal.
        pattern: String,
    },
    /// `IsNull(column, polarity)` — the only null test, on any column of `I`
    /// (design §3.2); `true` tests `IS NULL`, `false` tests `IS NOT NULL`.
    IsNull {
        /// Column of `I` being tested for null.
        column: ColumnRef,
        /// `true` = `IS NULL`, `false` = `IS NOT NULL`.
        polarity: bool,
    },
    /// `And(predicate…)` — one or more children; an empty conjunction has no
    /// representation (the plan omits the predicate field instead).
    And(Vec<Predicate>),
    /// `Or(predicate…)` — one or more children.
    Or(Vec<Predicate>),
    /// `Not(predicate)` — exactly one child.
    Not(Box<Predicate>),
}

/// Sort direction (design §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SortDirection {
    /// Ascending (`asc`).
    Asc,
    /// Descending (`desc`).
    Desc,
}

/// One typed sort row: a column of `I` and a direction (design §3.4).
/// Earlier rows in the plan's order list are primary; later rows are
/// tie-breakers. The list order is canonical — it is preserved by
/// construction (`Vec`) and by serialization (design §8.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrderRow {
    /// Column of `I` to sort by.
    pub column: ColumnRef,
    /// Sort direction.
    pub direction: SortDirection,
}

/// Non-negative limit/offset window (design §3.4). Either or both fields may
/// be present; `limit = 0` is valid and yields the empty result (D2; design
/// §4). The fields are unsigned, so a negative window has no representation
/// after construction; [`Window::new`] is the validated entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Window {
    /// Non-negative row limit; `0` is valid (empty result).
    pub limit: Option<u64>,
    /// Non-negative row offset.
    pub offset: Option<u64>,
}

/// Structured rejection of a non-canonical window (design §3.4: negative
/// values are rejected at plan validation, before provider lowering; an empty
/// window is represented as absence).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowError {
    /// Negative limit.
    NegativeLimit(i64),
    /// Negative offset.
    NegativeOffset(i64),
}

impl Window {
    /// Canonical validated window construction.
    ///
    /// Returns `Ok(None)` when both fields are absent — an empty window is
    /// the plan's "no window" representation (design §3.4: either or both
    /// present). Returns a structured error for a negative limit or offset;
    /// after construction both fields are unsigned.
    ///
    /// # Errors
    ///
    /// Returns [`WindowError::NegativeLimit`] or
    /// [`WindowError::NegativeOffset`] when the corresponding value is
    /// negative.
    ///
    /// The guarded `as u64` casts below are safe: every negative input has
    /// already returned an error in the preceding match arm.
    #[allow(clippy::cast_sign_loss)]
    pub fn new(limit: Option<i64>, offset: Option<i64>) -> Result<Option<Self>, WindowError> {
        let limit = match limit {
            Some(v) if v < 0 => return Err(WindowError::NegativeLimit(v)),
            Some(v) => Some(v as u64),
            None => None,
        };
        let offset = match offset {
            Some(v) if v < 0 => return Err(WindowError::NegativeOffset(v)),
            Some(v) => Some(v as u64),
            None => None,
        };
        if limit.is_none() && offset.is_none() {
            return Ok(None);
        }
        Ok(Some(Self { limit, offset }))
    }
}

/// The intensional relational plan — `filtrum<I,O>` as pure data (design
/// §3.1; §3.5). Field order is the canonical serialization order (design
/// §8.2): encoding version, source identity, schema identities `I` and `O`,
/// then the plan-content fields predicate, order, and window, and finally the
/// capability requirements (identity component 5).
///
/// The model carries both schema identities so the semantic unit can make the
/// landed `O ⊆ I` proof ([`crate::schema_subset`]) a required construction
/// invariant rather than a provider check (S3-U2). Projection is derived from
/// `O` — the plan carries no separate unchecked `select = [...]` claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FiltrumPlan {
    /// Clean-break serialization version (design §8.2 field 1; D6 identity
    /// component 1).
    pub encoding_version: u32,
    /// Explicit relation/model identity (field 2; identity component 2).
    pub source: SourceIdentity,
    /// Input contract `I` — the columns available to predicates, ordering,
    /// and projection (field 3; identity component 3).
    pub input_schema: SchemaDef,
    /// Visible result heading `O` — a compatible subset of `I` (field 3;
    /// identity component 3).
    pub output_schema: SchemaDef,
    /// Closed predicate tree; absent = no author restriction (field 5; plan
    /// content). Provider policy still applies at execution (design §5).
    pub predicate: Option<Predicate>,
    /// Ordered sort rows; absent = no order promise (field 6; plan content).
    /// Earlier rows are primary, later rows tie-breakers (design §3.4, §4.2).
    pub order: Option<Vec<OrderRow>>,
    /// Non-negative limit/offset window; absent = no window (field 7; plan
    /// content).
    pub window: Option<Window>,
    /// Declared provider capability requirements; empty = default (shared)
    /// capabilities only (field 8; identity component 5).
    pub requirements: Vec<CapabilityRequirement>,
}
