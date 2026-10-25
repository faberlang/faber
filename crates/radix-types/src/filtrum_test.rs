//! Focused type-model tests for the provider-neutral `filtrum` plan
//! (census-types S3-U1).
//!
//! The oracle is the canonical shared plan (stage0/fixture-contract.md §8):
//! `filtrum<PersonaQuaerenda, PersonaBrevis>` over `people` with the
//! source-only predicate column `activa` and explicit aliases. The tests
//! prove the closed §3.2 variant set, canonical order-row sequence, the
//! non-negative window invariant, capability-requirement identity, and that
//! equality/hash derive from provider-neutral fields only.

use crate::schema::{SchemaColumnDef, SchemaDef};
use crate::schema_subset::{extra_input_columns, schema_subset};
use crate::types::{Primitive, TypeTable};
use crate::{
    CapabilityRequirement, ColumnRef, ComparisonOp, FILTRUM_ENCODING_VERSION_V1, FiltrumPlan,
    OrderRow, Predicate, PredicateValue, SortDirection, SourceIdentity, Window, WindowError,
};
use radix_lexer::Interner;

fn types() -> TypeTable {
    TypeTable::new()
}

/// The fixture-contract §8 schemas: `I = PersonaQuaerenda`
/// (id numerus, nomen textus, activa bivalens) and `O = PersonaBrevis`
/// (id numerus, nomen textus). `nomen` aliases source `plenum` and `activa`
/// aliases source `statim` in `I`. All columns share one `TypeTable` so the
/// `TypeId`s match within the subset proof.
fn fixture_schemas(types: &TypeTable, interner: &mut Interner) -> (SchemaDef, SchemaDef) {
    let input_schema = SchemaDef {
        name: interner.intern("PersonaQuaerenda"),
        columns: vec![
            SchemaColumnDef {
                name: interner.intern("id"),
                source: interner.intern("id"),
                ty: types.primitive(Primitive::Numerus),
                required: true,
                order: 0,
            },
            SchemaColumnDef {
                name: interner.intern("nomen"),
                source: interner.intern("plenum"),
                ty: types.primitive(Primitive::Textus),
                required: true,
                order: 1,
            },
            SchemaColumnDef {
                name: interner.intern("activa"),
                source: interner.intern("statim"),
                ty: types.primitive(Primitive::Bivalens),
                required: true,
                order: 2,
            },
        ],
    };
    let output_schema = SchemaDef {
        name: interner.intern("PersonaBrevis"),
        columns: vec![
            SchemaColumnDef {
                name: interner.intern("id"),
                source: interner.intern("id"),
                ty: types.primitive(Primitive::Numerus),
                required: true,
                order: 0,
            },
            SchemaColumnDef {
                name: interner.intern("nomen"),
                source: interner.intern("plenum"),
                ty: types.primitive(Primitive::Textus),
                required: true,
                order: 1,
            },
        ],
    };
    (input_schema, output_schema)
}

/// The canonical shared plan (fixture-contract §8): source `people`,
/// predicate `activa ≡ verum`, order `nomen asc`, no window, no declared
/// requirements.
fn canonical_plan(types: &TypeTable, interner: &mut Interner) -> FiltrumPlan {
    let (input_schema, output_schema) = fixture_schemas(types, interner);
    FiltrumPlan {
        encoding_version: FILTRUM_ENCODING_VERSION_V1,
        source: SourceIdentity(interner.intern("people")),
        input_schema,
        output_schema,
        predicate: Some(Predicate::Compare {
            column: ColumnRef(interner.intern("activa")),
            op: ComparisonOp::Eq,
            value: PredicateValue::Bivalens(true),
        }),
        order: Some(vec![OrderRow {
            column: ColumnRef(interner.intern("nomen")),
            direction: SortDirection::Asc,
        }]),
        window: None,
        requirements: vec![],
    }
}

#[test]
fn canonical_plan_instantiates_the_fixture_contract_section_8_shape() {
    let types = types();
    let mut interner = Interner::new();
    let plan = canonical_plan(&types, &mut interner);

    // Every locked plan field (§3.1) is carried explicitly, including the
    // encoding version (D6 identity component 1).
    assert_eq!(plan.encoding_version, FILTRUM_ENCODING_VERSION_V1);
    assert_eq!(plan.source, SourceIdentity(interner.intern("people")));
    assert_eq!(plan.input_schema.name, interner.intern("PersonaQuaerenda"));
    assert_eq!(plan.output_schema.name, interner.intern("PersonaBrevis"));

    // The model carries `I` and `O`, so U2 can make the landed subset proof a
    // required construction invariant rather than a provider check.
    assert!(schema_subset(&types, &plan.output_schema, &plan.input_schema).is_ok());

    // `activa` is a source-only predicate column: it lives in `I \ O` and is
    // never part of the visible result.
    assert_eq!(
        extra_input_columns(&plan.output_schema, &plan.input_schema),
        vec![interner.intern("activa")]
    );

    // The predicate resolves against `I`'s column identity.
    assert_eq!(
        plan.predicate.as_ref(),
        Some(&Predicate::Compare {
            column: ColumnRef(interner.intern("activa")),
            op: ComparisonOp::Eq,
            value: PredicateValue::Bivalens(true),
        })
    );
}

#[test]
fn predicate_ast_is_closed_to_the_eight_locked_variants() {
    let mut interner = Interner::new();
    let c = ColumnRef(interner.intern("activa"));
    let n = PredicateValue::Numerus(1);

    let predicates = [
        Predicate::Compare {
            column: c,
            op: ComparisonOp::Eq,
            value: n.clone(),
        },
        Predicate::Between {
            column: c,
            low: n.clone(),
            high: n.clone(),
        },
        Predicate::In {
            column: c,
            values: vec![n.clone()],
        },
        Predicate::Like {
            column: c,
            pattern: "a%".to_owned(),
        },
        Predicate::IsNull {
            column: c,
            polarity: true,
        },
        Predicate::And(vec![Predicate::IsNull {
            column: c,
            polarity: true,
        }]),
        Predicate::Or(vec![Predicate::IsNull {
            column: c,
            polarity: false,
        }]),
        Predicate::Not(Box::new(Predicate::IsNull {
            column: c,
            polarity: true,
        })),
    ];
    assert_eq!(predicates.len(), 8);

    // The exhaustive match is the closedness proof: any operator added to
    // the model breaks this test at compile time. No variant carries SQL text
    // or a closure (design §1.3, §3.2).
    for predicate in predicates {
        match &predicate {
            Predicate::Compare { .. } => 0,
            Predicate::Between { .. } => 1,
            Predicate::In { .. } => 2,
            Predicate::Like { .. } => 3,
            Predicate::IsNull { .. } => 4,
            Predicate::And(_) => 5,
            Predicate::Or(_) => 6,
            Predicate::Not(_) => 7,
        };
    }
}

#[test]
fn comparison_operators_are_the_six_locked_members() {
    let ops = [
        ComparisonOp::Eq,
        ComparisonOp::Ne,
        ComparisonOp::Gt,
        ComparisonOp::Gte,
        ComparisonOp::Lt,
        ComparisonOp::Lte,
    ];
    assert_eq!(ops.len(), 6);
    for op in ops {
        match op {
            ComparisonOp::Eq
            | ComparisonOp::Ne
            | ComparisonOp::Gt
            | ComparisonOp::Gte
            | ComparisonOp::Lt
            | ComparisonOp::Lte => {}
        }
    }
}

#[test]
fn order_rows_preserve_the_canonical_primary_to_tie_breaker_sequence() {
    let types = types();
    let mut interner = Interner::new();
    let mut plan = canonical_plan(&types, &mut interner);

    // Earlier rows are primary, later rows are tie-breakers (design §3.4);
    // the list is the canonical order by construction and participates in
    // plan equality.
    let tie_breaker = OrderRow {
        column: ColumnRef(interner.intern("id")),
        direction: SortDirection::Desc,
    };
    plan.order
        .as_mut()
        .expect("order present")
        .push(tie_breaker);

    let order = plan.order.as_ref().expect("order present");
    assert_eq!(order[0].column, ColumnRef(interner.intern("nomen")));
    assert_eq!(order[0].direction, SortDirection::Asc);
    assert_eq!(order[1].column, ColumnRef(interner.intern("id")));
    assert_eq!(order[1].direction, SortDirection::Desc);

    let mut reordered = plan.clone();
    reordered.order.as_mut().expect("order present").swap(0, 1);
    assert_ne!(plan, reordered, "row sequence is canonical, not a set");

    let mut other_direction = plan.clone();
    other_direction.order.as_mut().expect("order present")[0].direction = SortDirection::Desc;
    assert_ne!(plan, other_direction, "direction participates in identity");
}

#[test]
fn window_rejects_negative_values_and_accepts_zero() {
    // Negative limit or offset is rejected at plan validation (design §3.4).
    assert_eq!(
        Window::new(Some(-1), None),
        Err(WindowError::NegativeLimit(-1))
    );
    assert_eq!(
        Window::new(None, Some(-3)),
        Err(WindowError::NegativeOffset(-3))
    );
    assert_eq!(
        Window::new(Some(-1), Some(-3)),
        Err(WindowError::NegativeLimit(-1))
    );

    // `limit = 0` is valid and yields the empty result (design §3.4, §4).
    assert_eq!(
        Window::new(Some(0), None),
        Ok(Some(Window {
            limit: Some(0),
            offset: None
        }))
    );
    assert_eq!(
        Window::new(Some(10), Some(20)),
        Ok(Some(Window {
            limit: Some(10),
            offset: Some(20)
        }))
    );

    // An empty window is the plan's "no window" representation.
    assert_eq!(Window::new(None, None), Ok(None));
}

#[test]
fn fractus_values_compare_and_hash_by_canonical_bits() {
    // IEEE negative zero and positive zero are equal values; the model keeps
    // one canonical bit form so Eq and Hash agree (design §6.3 `fractus`).
    assert_eq!(PredicateValue::Fractus(-0.0), PredicateValue::Fractus(0.0));

    let mut seen = std::collections::HashSet::new();
    seen.insert(PredicateValue::Fractus(-0.0));
    assert!(seen.contains(&PredicateValue::Fractus(0.0)));

    // Every NaN payload collapses to one canonical form.
    assert_eq!(
        PredicateValue::Fractus(f64::NAN),
        PredicateValue::Fractus(f64::NAN * 2.0)
    );

    assert_ne!(PredicateValue::Fractus(1.5), PredicateValue::Fractus(2.5));
}

#[test]
fn capability_requirements_are_the_named_provider_specific_set() {
    let types = types();
    let mut interner = Interner::new();
    let mut plan = canonical_plan(&types, &mut interner);

    // The four v1 provider-specific capabilities (design §6.2); declaring
    // none is the portable default.
    let requirements = [
        CapabilityRequirement::Regex,
        CapabilityRequirement::Arrays,
        CapabilityRequirement::FullText,
        CapabilityRequirement::JsonArray,
    ];
    assert_eq!(requirements.len(), 4);

    plan.requirements = requirements.to_vec();
    assert_eq!(plan.requirements.len(), 4);

    let mut empty = plan.clone();
    empty.requirements.clear();
    assert_ne!(plan, empty, "requirements participate in plan identity");
}

#[test]
fn equal_plans_built_independently_are_equal() {
    let types = types();
    let mut interner = Interner::new();
    let a = canonical_plan(&types, &mut interner);
    let mut interner_b = Interner::new();
    let b = canonical_plan(&types, &mut interner_b);

    // Equality derives from provider-neutral fields: two independent
    // constructions of the same logical plan are equal.
    assert_eq!(a, b);

    // Predicate, order, window, and requirements are plan content and
    // participate in equality; a change in any one changes the plan.
    let mut c = b.clone();
    c.window = Window::new(Some(5), None).expect("valid window");
    assert_ne!(a, c);

    let mut d = b;
    d.predicate = None;
    assert_ne!(a, d, "absent predicate = no author restriction");
}
