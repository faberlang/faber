//! Relational schema metadata model (census-types S2-U2).
//!
//! A `schema` declaration names a type-namespace identity and carries stable
//! ordered per-column relational metadata — the D4 five-field list
//! (docs/design/relational-views.md §2.4.1;
//! stage0/decision-source-column-mapping.md §1.1): public name, source
//! name/alias, Faber type, required/nullability (via `T ∪ nihil` in the type
//! position), and canonical order.
//!
//! The model is deliberately relational, not a genus field list (D1 lock 1 —
//! the stop-and-reuse-`genus` trigger binds: no methods, no inheritance, no
//! nested columns, no DDL authority attach to the schema). Schema names are
//! type-level identities: they resolve in the type namespace and are never
//! runtime values (zero MIR/backend lowering).
//!
//! The metadata is built at semantic resolution time
//! (`crates/radix/src/semantic/passes/resolve.rs`) and stored on the
//! resolver; this module owns the pure data shape so later stages
//! (subset/compatibility checks, HIR items, providers) consume one stable
//! model.

use crate::types::TypeId;
use radix_lexer::Symbol as LexSymbol;
use serde::{Deserialize, Serialize};

/// Ordered per-column relational metadata for one schema column (D4 five-field
/// list; stage0/decision-source-column-mapping.md §1.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaColumnDef {
    /// Public name — the schema-facing visible name; result rows are keyed by
    /// it (field 1).
    pub name: LexSymbol,

    /// Source name / alias — the provider column identity the renderer
    /// selects and renames from, after alias resolution (field 2). A column
    /// without an explicit `publicName : sourceName` alias row carries the
    /// identity mapping (source == public name).
    pub source: LexSymbol,

    /// Faber type — the typed column contract (`T` or `T ∪ nihil`), interned
    /// in the shared [`TypeTable`] (field 3).
    pub ty: TypeId,

    /// Required / nullability — `true` when the column is required
    /// (non-`NULL`, must exist); `false` when the type position writes
    /// `T ∪ nihil` (field 4).
    pub required: bool,

    /// Canonical order index (0-based) — fixed column order is explicit
    /// relational metadata (field 5).
    pub order: usize,
}

/// Relational schema definition: schema name + ordered per-column metadata.
///
/// A schema is a partial application-owned relational heading
/// (docs/design/relational-views.md §2.1): it names exactly the columns the
/// application needs, and extra source columns stay invisible. The definition
/// is self-contained (name + ordered metadata) so consumers such as the
/// subset/compatibility checks and HIR items do not need the resolver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaDef {
    /// Schema name (type-namespace identity; never a runtime value).
    pub name: LexSymbol,

    /// Ordered per-column metadata in canonical column order.
    pub columns: Vec<SchemaColumnDef>,
}

#[cfg(test)]
#[path = "schema_test.rs"]
mod tests;
