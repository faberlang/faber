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
mod tests {
    use super::*;
    use crate::types::{Primitive, TypeTable};

    #[test]
    fn schema_def_carries_ordered_five_field_column_metadata() {
        let types = TypeTable::new();
        let textus = types.primitive(Primitive::Textus);
        let numerus = types.primitive(Primitive::Numerus);

        let nomen = SchemaColumnDef {
            name: LexSymbol(1),
            source: LexSymbol(2),
            ty: textus,
            required: true,
            order: 0,
        };
        let actas = SchemaColumnDef {
            name: LexSymbol(3),
            source: LexSymbol(3),
            ty: numerus,
            required: false,
            order: 1,
        };

        let def = SchemaDef {
            name: LexSymbol(4),
            columns: vec![nomen, actas],
        };

        // Canonical order is preserved by construction (Vec), and each column
        // carries the full D4 five-field list.
        assert_eq!(def.name, LexSymbol(4));
        assert_eq!(def.columns.len(), 2);
        assert_eq!(def.columns[0].order, 0);
        assert_eq!(def.columns[0].name, LexSymbol(1));
        assert_eq!(def.columns[0].source, LexSymbol(2));
        assert_eq!(def.columns[0].ty, textus);
        assert!(def.columns[0].required);
        assert_eq!(def.columns[1].order, 1);
        assert_eq!(def.columns[1].name, LexSymbol(3));
        assert_eq!(def.columns[1].source, LexSymbol(3));
        assert_eq!(def.columns[1].ty, numerus);
        assert!(!def.columns[1].required);
    }

    #[test]
    fn column_nullability_and_order_are_independent_fields() {
        // Required/nullability (field 4) and order (field 5) are separate
        // facts: two columns may share a nullability state while holding
        // distinct canonical order indices.
        let types = TypeTable::new();
        let ty = types.primitive(Primitive::Textus);
        let required = SchemaColumnDef {
            name: LexSymbol(1),
            source: LexSymbol(1),
            ty,
            required: true,
            order: 0,
        };
        let nullable = SchemaColumnDef {
            name: LexSymbol(2),
            source: LexSymbol(2),
            ty,
            required: false,
            order: 1,
        };
        let def = SchemaDef {
            name: LexSymbol(3),
            columns: vec![required, nullable],
        };

        assert_ne!(def.columns[0].required, def.columns[1].required);
        assert_ne!(def.columns[0].order, def.columns[1].order);
    }
}
