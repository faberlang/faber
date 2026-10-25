//! Checked schema subset/compatibility operations (census-types S2-U4).
//!
//! [`schema_subset`] proves **`O ⊆ I`** for two relational schema definitions
//! over the S2-U2 metadata model ([`SchemaDef`]): every visible column of `O`
//! exists in `I` by public name (after alias resolution — the public name is
//! the post-alias visible identity result rows are keyed by; fixture-contract
//! §7) with a compatible Faber type (docs/design/relational-views.md §2.3,
//! §4.5; D1 lock 2).
//!
//! The operation is pure metadata: no DB, no plan, no provider. It is the
//! schema-side proof that a narrower compatible output schema proves `O ⊆ I`
//! (plan.md S2 acceptance item 3), for S3's `filtrum<I,O>` typecheck to
//! consume before plan construction.
//!
//! # Decided rule cells (named for S2-U6 review)
//!
//! - **Matching key**: public name. Columns match when `O.name == I.name`;
//!   the source mapping (D4 field 2) is not compared — each schema's source
//!   rows are its own metadata, validated within the schema (DUP-RESOLVED,
//!   S2-U2) and against the relation at admission (MISSING-1, S5/S6), never
//!   by the subset check. Column order is per-schema canonical metadata and
//!   plays no role in the subset relation (matching is a name-based set
//!   membership, design §2.3 "Membership in `I` and `O` is a per-schema
//!   property").
//! - **Type-compatibility direction**: `is_assignable(I.ty, O.ty)` — a value of
//!   `I`'s column type must be usable where `O` expects one, because
//!   projection data flows `I → O` (design §4.5 "checked by name and
//!   compatible type"). The language's own semantic-compatibility relation
//!   ([`TypeTable::is_assignable`]) supplies the implicit edges — lossless
//!   integer widening (`i32 → i64`), `ascii → textus`, coarse → fine
//!   `instans` — and never crosses number families (integer → float needs an
//!   explicit `↦`), so an `O` that *widens* a column type is compatible and
//!   an `O` that *narrows* one is not.
//! - **Nullability**: may only be relaxed, never tightened. `O` may not
//!   require a column `I` offers only optionally (`T ∪ nihil`): a `NULL` must
//!   never reach a required cell — required and nullable are distinct states
//!   (D1 invariant 3; design §2.2) and `NULL` acceptance is exactly scoped to
//!   `T ∪ nihil`.
//!
//! # Partiality invariant (D1 invariant 1; design §2.1)
//!
//! A schema carries exactly its declared columns and never implies a complete
//! description of the source relation: there is no arity or completeness
//! field, [`schema_column_count`] is exactly the declared column count, and
//! [`extra_input_columns`] names the legal `I \ O` set.

use crate::schema::{SchemaColumnDef, SchemaDef};
use crate::types::{TypeId, TypeTable};
use radix_lexer::Symbol as LexSymbol;

/// Structured failure of an `O ⊆ I` subset proof, identifying the offending
/// `O` column (design §2.3; §9 fact vocabulary: column identity plus
/// expected/observed type where a type is the failure).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaSubsetError {
    /// An `O` column is absent from `I` by public name after alias
    /// resolution — the schema-side analog of MISSING-1 (design §2.4.2).
    /// Metadata-level: no row identity exists.
    AbsentFromInput { column: LexSymbol },

    /// A matched column's Faber type is incompatible in the projection
    /// direction: a value of `I`'s type cannot be used where `O`'s type is
    /// expected. `expected` is `O`'s declared type; `observed` is `I`'s.
    IncompatibleType {
        column: LexSymbol,
        expected: TypeId,
        observed: TypeId,
    },

    /// `O` tightens nullability: it requires a column `I` offers only
    /// optionally (`T ∪ nihil`). A `NULL` must never reach a required cell
    /// (design §2.2 — required and nullable are distinct states). `expected`
    /// is `O`'s declared type; `observed` is `I`'s.
    NullabilityTightened {
        column: LexSymbol,
        expected: TypeId,
        observed: TypeId,
    },
}

/// Prove `O ⊆ I` (checked subset; D1 lock 2): every visible column of `O`
/// exists in `I` by public name with a compatible Faber type.
///
/// Returns `Ok(())` when the subset holds; otherwise the first offending
/// column in `O`'s canonical order, with column identity and the relevant
/// expected/observed facts.
///
/// Precondition: both schemas are well-formed per S2-U2 (no duplicate
/// visible names — DUPLICATE-VISIBLE — and no duplicate resolved source
/// names); the check matches by public name and does not re-validate
/// per-schema well-formedness.
///
/// # Errors
///
/// Returns [`SchemaSubsetError::AbsentFromInput`] when an `O` column is
/// absent from `I`, [`SchemaSubsetError::IncompatibleType`] when the
/// matched column types are incompatible in the projection direction, and
/// [`SchemaSubsetError::NullabilityTightened`] when `O` requires a column
/// `I` offers only optionally.
pub fn schema_subset(
    types: &TypeTable,
    o: &SchemaDef,
    i: &SchemaDef,
) -> Result<(), SchemaSubsetError> {
    for o_col in &o.columns {
        let Some(i_col) = i.columns.iter().find(|c| c.name == o_col.name) else {
            return Err(SchemaSubsetError::AbsentFromInput { column: o_col.name });
        };
        schema_column_compatible(types, i_col, o_col)?;
    }
    Ok(())
}

/// Per-column compatibility of a matched `(I, O)` column pair (same public
/// name): nullability may only be relaxed, and every `I` value must be usable
/// as an `O` value.
fn schema_column_compatible(
    types: &TypeTable,
    i_col: &SchemaColumnDef,
    o_col: &SchemaColumnDef,
) -> Result<(), SchemaSubsetError> {
    // Nullability is a distinct state from required (design §2.2); a
    // compatible subset may relax it (I required → O nullable) but never
    // tighten it.
    if o_col.required && !i_col.required {
        return Err(SchemaSubsetError::NullabilityTightened {
            column: o_col.name,
            expected: o_col.ty,
            observed: i_col.ty,
        });
    }
    // Projection direction: the value of I's column flows into O's column.
    if !types.is_assignable(i_col.ty, o_col.ty) {
        return Err(SchemaSubsetError::IncompatibleType {
            column: o_col.name,
            expected: o_col.ty,
            observed: i_col.ty,
        });
    }
    Ok(())
}

/// Partiality invariant (D1 invariant 1; design §2.1): the number of columns
/// a schema carries is exactly its declared column count — never an implied
/// arity against a wider source relation. The column `Vec` is the complete
/// claim; no hidden, defaulted, or completeness-inferred column participates
/// in [`schema_subset`].
#[must_use]
pub fn schema_column_count(def: &SchemaDef) -> usize {
    def.columns.len()
}

/// Legal `I \ O` set (design §2.3): the public names of `I`'s columns that
/// `O` does not declare, in `I`'s canonical order — source-only predicate
/// columns and extra source columns. Their presence never invalidates the
/// subset proof: a wider source contract satisfies the schema and the extra
/// columns stay invisible and harmless (goal success criterion 8).
/// Provider-metadata admission of a real relation is S5/S6.
#[must_use]
pub fn extra_input_columns(o: &SchemaDef, i: &SchemaDef) -> Vec<LexSymbol> {
    i.columns
        .iter()
        .filter(|i_col| !o.columns.iter().any(|o_col| o_col.name == i_col.name))
        .map(|i_col| i_col.name)
        .collect()
}
