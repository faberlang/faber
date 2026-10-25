//! Subset/compatibility tests for [`schema_subset`] (census-types S2-U4).
//!
//! The oracle is the canonical fixture pair (stage0/fixture-contract.md §3,
//! §8): the canonical shared plan is `filtrum<PersonaQuaerenda, PersonaBrevis>`,
//! so `PersonaBrevis ⊆ PersonaQuaerenda` must hold. Incompatible cases fail
//! with the offending column identity; matching is by public name after alias
//! resolution with order playing no role; and the partiality invariant
//! (D1 invariant 1; design §2.1) is asserted directly.

use crate::schema::{SchemaColumnDef, SchemaDef};
use crate::schema_subset::{
    SchemaSubsetError, extra_input_columns, schema_column_count, schema_subset,
};
use crate::types::{Primitive, TypeId, TypeTable};
use radix_lexer::Interner;

/// One D4 five-field column row (decision-source-column-mapping.md §1.1).
fn col(
    interner: &mut Interner,
    name: &str,
    source: &str,
    ty: TypeId,
    required: bool,
    order: usize,
) -> SchemaColumnDef {
    SchemaColumnDef {
        name: interner.intern(name),
        source: interner.intern(source),
        ty,
        required,
        order,
    }
}

/// `PersonaQuaerenda` (`I`) — fixture-contract §3: `id:numerus` required,
/// `nomen:textus` required (source `plenum`), `activa:bivalens` required
/// (source `statim`; the source-only predicate column).
fn persona_quaerenda(interner: &mut Interner, types: &mut TypeTable) -> SchemaDef {
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let bivalens = types.primitive(Primitive::Bivalens);
    SchemaDef {
        name: interner.intern("PersonaQuaerenda"),
        columns: vec![
            col(interner, "id", "id", numerus, true, 0),
            col(interner, "nomen", "plenum", textus, true, 1),
            col(interner, "activa", "statim", bivalens, true, 2),
        ],
    }
}

/// `PersonaBrevis` (`O`) — fixture-contract §3: the canonical narrower output
/// schema over the same relation.
fn persona_brevis(interner: &mut Interner, types: &mut TypeTable) -> SchemaDef {
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    SchemaDef {
        name: interner.intern("PersonaBrevis"),
        columns: vec![
            col(interner, "id", "id", numerus, true, 0),
            col(interner, "nomen", "plenum", textus, true, 1),
        ],
    }
}

// =============================================================================
// Canonical oracle
// =============================================================================

#[test]
fn canonical_persona_brevis_subset_persona_quaerenda() {
    // The canonical shared plan is `filtrum<PersonaQuaerenda, PersonaBrevis>`
    // (fixture-contract §8): every visible column of O (id, nomen) exists in
    // I by public name with a compatible type, so `O ⊆ I` holds.
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let o = persona_brevis(&mut interner, &mut types);
    let i = persona_quaerenda(&mut interner, &mut types);

    assert_eq!(schema_subset(&types, &o, &i), Ok(()));
}

// =============================================================================
// Incompatible cases fail with column identity
// =============================================================================

#[test]
fn o_column_absent_from_i_fails_with_column_identity() {
    // `PersonaAbsurda`-shaped O (`titulus` does not exist in I) — the
    // schema-side analog of MISSING-1 (design §2.4.2): the failure names the
    // offending column; metadata-level, no row identity.
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let i = persona_quaerenda(&mut interner, &mut types);
    let o = SchemaDef {
        name: interner.intern("PersonaAbsurda"),
        columns: vec![
            col(&mut interner, "id", "id", numerus, true, 0),
            col(&mut interner, "titulus", "titulus", textus, true, 1),
        ],
    };

    assert_eq!(
        schema_subset(&types, &o, &i),
        Err(SchemaSubsetError::AbsentFromInput {
            column: interner.intern("titulus"),
        })
    );
}

#[test]
fn type_incompatible_column_fails_with_column_identity() {
    // O declares `id : textus` where I declares `id : numerus` — the
    // schema-side analog of TYPE-PG-1's incompatible-column cell (design
    // §2.4.2 / §9 fact set): the failure carries the offending column plus
    // expected (O's declared type) and observed (I's type) facts.
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let i = persona_quaerenda(&mut interner, &mut types);
    let o = SchemaDef {
        name: interner.intern("PersonaMala"),
        columns: vec![
            col(&mut interner, "id", "id", textus, true, 0),
            col(&mut interner, "nomen", "plenum", textus, true, 1),
        ],
    };

    assert_eq!(
        schema_subset(&types, &o, &i),
        Err(SchemaSubsetError::IncompatibleType {
            column: interner.intern("id"),
            expected: textus,
            observed: numerus,
        })
    );
}

#[test]
fn narrowing_o_column_type_is_incompatible() {
    // Decided cell (named note for S2-U6): type compatibility is directional
    // in the projection — every I value must be usable as an O value, so O
    // narrowing `fractus -> numerus` (would lose non-integral values) is
    // incompatible with column identity.
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let fractus = types.primitive(Primitive::Fractus);
    let i = SchemaDef {
        name: interner.intern("PersonaI"),
        columns: vec![
            col(&mut interner, "id", "id", fractus, true, 0),
            col(&mut interner, "nomen", "plenum", textus, true, 1),
        ],
    };
    let o = SchemaDef {
        name: interner.intern("PersonaArta"),
        columns: vec![
            col(&mut interner, "id", "id", numerus, true, 0),
            col(&mut interner, "nomen", "plenum", textus, true, 1),
        ],
    };

    assert_eq!(
        schema_subset(&types, &o, &i),
        Err(SchemaSubsetError::IncompatibleType {
            column: interner.intern("id"),
            expected: numerus,
            observed: fractus,
        })
    );
}

// =============================================================================
// Ordering / alias interplay
// =============================================================================

#[test]
fn subset_matches_by_public_name_after_alias_resolution_regardless_of_order() {
    // Matching is by public name after alias resolution (fixture-contract §7:
    // result rows are keyed by public names); column order is per-schema
    // canonical metadata. An O that lists its columns in a different order
    // than I is still a compatible subset.
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let i = persona_quaerenda(&mut interner, &mut types);
    let o = SchemaDef {
        name: interner.intern("PersonaBrevis"),
        columns: vec![
            col(&mut interner, "nomen", "plenum", textus, true, 0),
            col(&mut interner, "id", "id", numerus, true, 1),
        ],
    };

    assert_eq!(schema_subset(&types, &o, &i), Ok(()));
}

#[test]
fn same_public_name_different_source_still_matches() {
    // Decided cell (named note for S2-U6): the subset check matches by public
    // name only; source mappings are each schema's own D4 metadata
    // (DUP-RESOLVED is validated within each schema at S2-U2; MISSING-1
    // against a real relation is S5/S6). A matched public name whose O row
    // carries a different source row than I's is still a compatible subset.
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let i = persona_quaerenda(&mut interner, &mut types);
    let o = SchemaDef {
        name: interner.intern("PersonaBrevis"),
        columns: vec![
            col(&mut interner, "id", "id", numerus, true, 0),
            col(&mut interner, "nomen", "nomen_variatio", textus, true, 1),
        ],
    };

    assert_eq!(schema_subset(&types, &o, &i), Ok(()));
}

#[test]
fn subset_is_directional() {
    // PersonaQuaerenda is NOT a subset of PersonaBrevis: the source-only
    // predicate column `activa` (I \ O is legal for the plan, but the reverse
    // direction would expose a column I does not declare).
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let o = persona_quaerenda(&mut interner, &mut types);
    let i = persona_brevis(&mut interner, &mut types);

    assert_eq!(
        schema_subset(&types, &o, &i),
        Err(SchemaSubsetError::AbsentFromInput {
            column: interner.intern("activa"),
        })
    );
}

// =============================================================================
// Nullability cells (decided — named notes for S2-U6)
// =============================================================================

#[test]
fn nullability_relaxation_is_compatible() {
    // O may relax nullability: I required (`textus`) -> O nullable
    // (`textus ∪ nihil`) is a compatible subset — the projection drops no
    // values (a non-NULL I value always fits the optional O cell).
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let email_optional = types.option(textus);
    let i = SchemaDef {
        name: interner.intern("PersonaI"),
        columns: vec![
            col(&mut interner, "id", "id", numerus, true, 0),
            col(&mut interner, "email", "electronica", textus, true, 1),
        ],
    };
    let o = SchemaDef {
        name: interner.intern("PersonaO"),
        columns: vec![
            col(&mut interner, "id", "id", numerus, true, 0),
            col(
                &mut interner,
                "email",
                "electronica",
                email_optional,
                false,
                1,
            ),
        ],
    };

    assert_eq!(schema_subset(&types, &o, &i), Ok(()));
}

#[test]
fn nullability_tightening_is_rejected_with_column_identity() {
    // Decided cell (named note for S2-U6): a NULL must never reach a required
    // cell. O requiring a column I offers only optionally (`T ∪ nihil`) is
    // incompatible — required and nullable are distinct states (D1 invariant
    // 3; design §2.2), so the failure carries the offending column identity.
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let email_optional = types.option(textus);
    let i = SchemaDef {
        name: interner.intern("PersonaI"),
        columns: vec![
            col(&mut interner, "id", "id", numerus, true, 0),
            col(
                &mut interner,
                "email",
                "electronica",
                email_optional,
                false,
                1,
            ),
        ],
    };
    let o = SchemaDef {
        name: interner.intern("PersonaStricta"),
        columns: vec![
            col(&mut interner, "id", "id", numerus, true, 0),
            col(&mut interner, "email", "electronica", textus, true, 1),
        ],
    };

    assert_eq!(
        schema_subset(&types, &o, &i),
        Err(SchemaSubsetError::NullabilityTightened {
            column: interner.intern("email"),
            expected: textus,
            observed: email_optional,
        })
    );
}

#[test]
fn widening_o_column_type_is_compatible() {
    // Projection-direction consequence: an O that *widens* a column type is a
    // compatible subset (i32 -> i64 is an implicit integer widening; integer
    // -> float is not, d11-6 ruling 16).
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let numerus = types.sized_numeric(Primitive::Numerus, crate::NumericWidth::I32);
    let textus = types.primitive(Primitive::Textus);
    let numerus_lata = types.sized_numeric(Primitive::Numerus, crate::NumericWidth::I64);
    let i = SchemaDef {
        name: interner.intern("PersonaI"),
        columns: vec![
            col(&mut interner, "id", "id", numerus, true, 0),
            col(&mut interner, "nomen", "plenum", textus, true, 1),
        ],
    };
    let o = SchemaDef {
        name: interner.intern("PersonaLata"),
        columns: vec![
            col(&mut interner, "id", "id", numerus_lata, true, 0),
            col(&mut interner, "nomen", "plenum", textus, true, 1),
        ],
    };

    assert_eq!(schema_subset(&types, &o, &i), Ok(()));
}

// =============================================================================
// Partiality invariant (D1 invariant 1; design §2.1)
// =============================================================================

#[test]
fn partiality_invariant_schema_carries_exactly_its_declared_columns() {
    // A schema carries exactly its declared columns and never implies a
    // complete description of the source relation. The `people` relation has
    // six source columns (fixture-contract §2); PersonaQuaerenda declares
    // three and PersonaBrevis two — the counts are exactly the declared
    // counts, and the wider source metadata never invalidates the subset
    // proof.
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let o = persona_brevis(&mut interner, &mut types);
    let i = persona_quaerenda(&mut interner, &mut types);

    // Exactness: declared count only — never the wider relation's arity.
    assert_eq!(schema_column_count(&i), 3);
    assert_eq!(schema_column_count(&o), 2);

    // The legal `I \ O` set: `activa` is the source-only predicate column.
    assert_eq!(extra_input_columns(&o, &i), vec![interner.intern("activa")]);

    // Extra columns stay invisible and harmless: the subset proof passes and
    // names only the declared columns.
    assert_eq!(schema_subset(&types, &o, &i), Ok(()));
}

#[test]
fn empty_output_schema_is_trivially_a_subset() {
    // An empty output heading (no visible columns) is vacuously a subset of
    // any input schema; the reverse (a non-empty O over an empty I) fails
    // with the first offending column identity.
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let o = SchemaDef {
        name: interner.intern("PersonaVacua"),
        columns: vec![],
    };
    let i = persona_quaerenda(&mut interner, &mut types);
    assert_eq!(schema_subset(&types, &o, &i), Ok(()));

    let o2 = persona_brevis(&mut interner, &mut types);
    let i2 = SchemaDef {
        name: interner.intern("SchemaVacua"),
        columns: vec![],
    };
    assert_eq!(
        schema_subset(&types, &o2, &i2),
        Err(SchemaSubsetError::AbsentFromInput {
            column: interner.intern("id"),
        })
    );
}
