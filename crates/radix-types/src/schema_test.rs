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
