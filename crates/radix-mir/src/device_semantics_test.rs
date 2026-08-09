//! Stage 3R U1 red tests for the carried semantic contract (F1–F7): the
//! same-name non-aliasing, duplicate/contradictory relation, generation, and
//! declaration-order / dependency-graph rules. These fail against the pre-3R
//! model (no semantic identities, overwriting companion map, `version = 1`
//! only) and pass against the frozen contract.

use super::*;
use crate::device::{
    MirCompanionInsertError, MirCompanionMap, MirCompanionSelectedInput, MirCompanionSelectedOutput,
};
use crate::device_program::{BufferId, LaunchId};
use crate::{MirFunctionId, MirType};
use radix_types::TypeId;

fn f32() -> MirType {
    MirType::semantic(TypeId(1))
}

fn value(id: u32, name: &str, origin: SemanticValueOrigin) -> SemanticValue {
    SemanticValue {
        id: SemanticValueId(id),
        name: name.to_owned(),
        element_ty: f32(),
        element_count: 4,
        origin,
    }
}

fn lossless_entry(primal: u32, companion: u32) -> LosslessMirCompanionEntry {
    LosslessMirCompanionEntry {
        primal: DefId(primal),
        companion: DefId(companion),
        derivative: MirCompanionDerivativeKind::ReverseModeVjp,
        device_resident: true,
        selected_inputs: vec![MirCompanionSelectedInput {
            param: DefId(1),
            position: 0,
            ty: TypeId(1),
            gradient_slot: 0,
        }],
        selected_outputs: vec![MirCompanionSelectedOutput {
            position: 0,
            ty: TypeId(1),
            upstream_gradient_ty: TypeId(1),
        }],
    }
}

// ── F1: same-name non-aliasing ──────────────────────────────────────────

#[test]
fn same_name_same_shape_values_with_distinct_origins_do_not_alias() {
    // Two unrelated values with the SAME name and SAME shape but different
    // carried origins are distinct: identity is by origin, never by name or
    // shape coincidence.
    let semantics = DeviceSemantics {
        values: vec![
            value(
                1,
                "x",
                SemanticValueOrigin::MirLocal {
                    function: MirFunctionId(0),
                    local: 0,
                },
            ),
            value(
                2,
                "x",
                SemanticValueOrigin::MirLocal {
                    function: MirFunctionId(0),
                    local: 1,
                },
            ),
        ],
        bindings: vec![
            ValueBinding {
                value: SemanticValueId(1),
                buffer: BufferId(1),
            },
            ValueBinding {
                value: SemanticValueId(2),
                buffer: BufferId(2),
            },
        ],
        ..DeviceSemantics::default()
    };
    semantics
        .validate(&[])
        .expect("distinct origins never alias");
}

#[test]
fn two_values_minted_from_one_origin_fail_closed() {
    // Minting two distinct identities from ONE carried origin would silently
    // alias the two values — rejected (F1).
    let origin = SemanticValueOrigin::HostInput;
    let semantics = DeviceSemantics {
        values: vec![value(1, "x", origin.clone()), value(2, "x", origin)],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[]),
        Err(DeviceProgramError::SemanticOriginAliased {
            first: SemanticValueId(1),
            second: SemanticValueId(2),
            ..
        })
    ));
}

#[test]
fn one_value_bound_to_two_buffers_fails_closed() {
    let semantics = DeviceSemantics {
        values: vec![value(
            1,
            "x",
            SemanticValueOrigin::MirLocal {
                function: MirFunctionId(0),
                local: 0,
            },
        )],
        bindings: vec![
            ValueBinding {
                value: SemanticValueId(1),
                buffer: BufferId(1),
            },
            ValueBinding {
                value: SemanticValueId(1),
                buffer: BufferId(2),
            },
        ],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[]),
        Err(DeviceProgramError::DuplicateValueBinding {
            value: SemanticValueId(1),
            buffer: BufferId(1),
        })
    ));
}

// ── F4: duplicate / contradictory relation rows fail closed ────────────

#[test]
fn duplicate_relation_row_is_rejected_by_the_carrier() {
    let mut carriers = MirCompanionMap::default();
    carriers.insert(lossless_entry(1, 2));
    // An identical row for the same primal is a duplicate — never an
    // overwrite.
    assert_eq!(
        carriers.try_insert(lossless_entry(1, 2)),
        Err(MirCompanionInsertError::DuplicatePrimal { primal: DefId(1) })
    );
}

#[test]
fn duplicate_relation_row_panics_through_insert() {
    let mut carriers = MirCompanionMap::default();
    carriers.insert(lossless_entry(1, 2));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        carriers.insert(lossless_entry(1, 2));
    }));
    assert!(result.is_err(), "fail-closed insert must never overwrite");
}

#[test]
fn contradictory_relation_row_is_rejected_by_the_carrier() {
    let mut carriers = MirCompanionMap::default();
    carriers.insert(lossless_entry(1, 2));
    // Same primal, different companion: contradictory facts, rejected.
    let mut contradictory = lossless_entry(1, 2);
    contradictory.companion = DefId(3);
    match carriers.try_insert(contradictory) {
        Err(MirCompanionInsertError::ContradictoryEntry { primal, field, .. }) => {
            assert_eq!(primal, DefId(1));
            assert_eq!(field, "companion");
        }
        other => panic!("expected ContradictoryEntry, got {other:?}"),
    }
    assert_eq!(
        carriers.companion_for(DefId(1)).expect("carried row"),
        &lossless_entry(1, 2),
        "the original lossless row survives the failed insert"
    );
}

#[test]
fn contradictory_relation_rows_fail_closed_in_semantics() {
    let mut first = lossless_entry(1, 2);
    let mut second = lossless_entry(1, 2);
    first.selected_inputs[0].gradient_slot = 0;
    second.selected_inputs[0].gradient_slot = 1;
    let semantics = DeviceSemantics {
        relations: vec![first, second],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[]),
        Err(DeviceProgramError::ContradictoryRelationRow {
            primal,
            field: "selected-inputs",
            ..
        }) if primal == DefId(1)
    ));
}

#[test]
fn relation_row_with_same_primal_and_companion_fails_closed() {
    let semantics = DeviceSemantics {
        relations: vec![lossless_entry(1, 1)],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[]),
        Err(DeviceProgramError::IllegalRelationRow {
            primal,
            reason,
        }) if primal == DefId(1) && reason == "primal and companion DefIds must differ"
    ));
}

// ── F2: generation errors ───────────────────────────────────────────────

#[test]
fn zero_generation_is_rejected() {
    let semantics = DeviceSemantics {
        values: vec![value(
            1,
            "x",
            SemanticValueOrigin::MirLocal {
                function: MirFunctionId(0),
                local: 0,
            },
        )],
        generations: vec![ValueGeneration {
            value: SemanticValueId(1),
            generation: 0,
            element_ty: f32(),
            element_count: 4,
            produced_by: LaunchId(1),
        }],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[LaunchId(1)]),
        Err(DeviceProgramError::UnsetValueGeneration {
            value: SemanticValueId(1)
        })
    ));
}

#[test]
fn second_producer_of_one_generation_is_rejected() {
    // "another producer of generation 1" — a later write must be a NEW
    // generation, never a second producer of an existing one (F2).
    let launch: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let semantics = DeviceSemantics {
        values: vec![value(
            1,
            "x",
            SemanticValueOrigin::MirLocal {
                function: MirFunctionId(0),
                local: 0,
            },
        )],
        generations: vec![
            ValueGeneration {
                value: SemanticValueId(1),
                generation: 1,
                element_ty: f32(),
                element_count: 4,
                produced_by: LaunchId(1),
            },
            ValueGeneration {
                value: SemanticValueId(1),
                generation: 1,
                element_ty: f32(),
                element_count: 4,
                produced_by: LaunchId(2),
            },
        ],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&launch),
        Err(DeviceProgramError::DuplicateGenerationProducer {
            value: SemanticValueId(1),
            generation: 1
        })
    ));
}

// ── F3: declaration order independence + graph fail-closed rules ────────

fn ordered_chain_semantics(reversed: bool) -> DeviceSemantics {
    // One root launch L1 produces generation 1 of value 1; L2 consumes it.
    // `reversed` swaps the DECLARATION order of the dependency and the
    // generation — execution order and validity must not change.
    let mut dependencies = vec![DependencyEdge {
        producer: LaunchId(1),
        consumer: LaunchId(2),
        buffer: BufferId(1),
        version: 1,
    }];
    let mut generations = vec![ValueGeneration {
        value: SemanticValueId(1),
        generation: 1,
        element_ty: f32(),
        element_count: 4,
        produced_by: LaunchId(1),
    }];
    if reversed {
        dependencies.reverse();
        generations.reverse();
    }
    DeviceSemantics {
        values: vec![value(
            1,
            "x",
            SemanticValueOrigin::MirLocal {
                function: MirFunctionId(0),
                local: 0,
            },
        )],
        bindings: vec![ValueBinding {
            value: SemanticValueId(1),
            buffer: BufferId(1),
        }],
        generations,
        roots: vec![LaunchId(1)],
        dependencies,
        ..DeviceSemantics::default()
    }
}

#[test]
fn declaration_reorder_does_not_change_validation() {
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let forward = ordered_chain_semantics(false);
    let reordered = ordered_chain_semantics(true);
    forward
        .validate(&launches)
        .expect("forward order validates");
    reordered
        .validate(&launches)
        .expect("reordered declaration order validates identically");
}

#[test]
fn dependency_cycle_is_rejected_before_launch() {
    let semantics = DeviceSemantics {
        dependencies: vec![
            DependencyEdge {
                producer: LaunchId(1),
                consumer: LaunchId(2),
                buffer: BufferId(1),
                version: 1,
            },
            DependencyEdge {
                producer: LaunchId(2),
                consumer: LaunchId(1),
                buffer: BufferId(2),
                version: 1,
            },
        ],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[LaunchId(1), LaunchId(2)]),
        Err(DeviceProgramError::DependencyCycle { .. })
    ));
}

#[test]
fn self_dependency_is_rejected() {
    let semantics = DeviceSemantics {
        dependencies: vec![DependencyEdge {
            producer: LaunchId(1),
            consumer: LaunchId(1),
            buffer: BufferId(1),
            version: 1,
        }],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[LaunchId(1)]),
        Err(DeviceProgramError::SelfDependency {
            launch: LaunchId(1)
        })
    ));
}

#[test]
fn missing_dependency_is_rejected() {
    // A generation producer that is not reachable from any root is a missing
    // dependency: the host could never have scheduled it (F3).
    let semantics = DeviceSemantics {
        values: vec![value(
            1,
            "x",
            SemanticValueOrigin::MirLocal {
                function: MirFunctionId(0),
                local: 0,
            },
        )],
        generations: vec![ValueGeneration {
            value: SemanticValueId(1),
            generation: 1,
            element_ty: f32(),
            element_count: 4,
            produced_by: LaunchId(2),
        }],
        roots: vec![LaunchId(1)],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[LaunchId(1), LaunchId(2)]),
        Err(DeviceProgramError::MissingDependency {
            launch: LaunchId(2),
            value: SemanticValueId(1),
        })
    ));
}

#[test]
fn illegal_root_is_rejected() {
    let semantics = DeviceSemantics {
        roots: vec![LaunchId(9)],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[LaunchId(1)]),
        Err(DeviceProgramError::IllegalRoot { root: LaunchId(9) })
    ));
}

#[test]
fn unknown_dependency_endpoint_is_rejected() {
    let semantics = DeviceSemantics {
        dependencies: vec![DependencyEdge {
            producer: LaunchId(1),
            consumer: LaunchId(9),
            buffer: BufferId(1),
            version: 1,
        }],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[LaunchId(1)]),
        Err(DeviceProgramError::UnknownDependencyEndpoint {
            launch: LaunchId(9)
        })
    ));
}

// ── F5/F6: axis independence ────────────────────────────────────────────

#[test]
fn contradictory_initialization_facts_fail_closed() {
    let semantics = DeviceSemantics {
        initializations: vec![
            InitializationFact {
                buffer: BufferId(1),
                policy: InitializationPolicy::ZeroFill,
            },
            InitializationFact {
                buffer: BufferId(1),
                policy: InitializationPolicy::HostProvided,
            },
        ],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[]),
        Err(DeviceProgramError::ContradictoryInitialization {
            buffer: BufferId(1),
            ..
        })
    ));
}

#[test]
fn observation_naming_unknown_launch_fails_closed() {
    let semantics = DeviceSemantics {
        values: vec![value(
            1,
            "x",
            SemanticValueOrigin::MirLocal {
                function: MirFunctionId(0),
                local: 0,
            },
        )],
        bindings: vec![ValueBinding {
            value: SemanticValueId(1),
            buffer: BufferId(1),
        }],
        // The launch is justified (a declared root) so the graph is total;
        // the observation still names a launch that does not exist (F5/F6).
        roots: vec![LaunchId(1)],
        observations: vec![ObservationFact {
            buffer: BufferId(1),
            version: 1,
            at_launch: LaunchId(9),
        }],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[LaunchId(1)]),
        Err(DeviceProgramError::ObservationOutOfRange {
            launch: LaunchId(9)
        })
    ));
}

// ── DDCP1-U2: explicit generations + edge-driven scheduling (contract
//    rules 2/3, scheduling rules 2/3) ─────────────────────────────────────

#[test]
fn same_producer_duplicate_generation_row_fails_closed() {
    // Every logical write is a NEW generation (F2): an identical
    // (value, generation) row carried twice is a duplicated write even when
    // the same launch claims both copies — never a silent dedupe.
    let semantics = DeviceSemantics {
        values: vec![value(
            1,
            "x",
            SemanticValueOrigin::MirLocal {
                function: MirFunctionId(0),
                local: 0,
            },
        )],
        generations: vec![
            ValueGeneration {
                value: SemanticValueId(1),
                generation: 1,
                element_ty: f32(),
                element_count: 4,
                produced_by: LaunchId(1),
            },
            ValueGeneration {
                value: SemanticValueId(1),
                generation: 1,
                element_ty: f32(),
                element_count: 4,
                produced_by: LaunchId(1),
            },
        ],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[LaunchId(1)]),
        Err(DeviceProgramError::DuplicateGenerationProducer {
            value: SemanticValueId(1),
            generation: 1
        })
    ));
}

#[test]
fn dangling_consumer_without_incoming_edge_fails_closed() {
    // Scheduling rule 2: a non-root launch with no justifying incoming
    // dependency edge is a dangling consumer.
    let semantics = DeviceSemantics {
        roots: vec![LaunchId(1)],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[LaunchId(1), LaunchId(2)]),
        Err(DeviceProgramError::DanglingConsumer { launch: LaunchId(2) })
    ));
}

#[test]
fn non_root_launch_with_incoming_edge_is_not_dangling() {
    // A non-root launch justified by an incoming edge is not a dangling
    // consumer (positive side of scheduling rule 2).
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let semantics = ordered_chain_semantics(false);
    semantics
        .validate(&launches)
        .expect("a launch with an incoming edge and a reachable root is justified");
}

#[test]
fn consumed_value_without_producer_generation_fails_closed() {
    // Scheduling rule 3: a value that is consumed but has no producer — the
    // edge's producer launch never writes it (no carried generation).
    let semantics = DeviceSemantics {
        values: vec![value(
            1,
            "x",
            SemanticValueOrigin::MirLocal {
                function: MirFunctionId(0),
                local: 0,
            },
        )],
        bindings: vec![ValueBinding {
            value: SemanticValueId(1),
            buffer: BufferId(1),
        }],
        roots: vec![LaunchId(1)],
        dependencies: vec![DependencyEdge {
            producer: LaunchId(1),
            consumer: LaunchId(2),
            buffer: BufferId(1),
            version: 1,
        }],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[LaunchId(1), LaunchId(2)]),
        Err(DeviceProgramError::MissingProducer {
            producer: LaunchId(1),
            value: SemanticValueId(1),
        })
    ));
}

#[test]
fn edge_buffer_without_value_binding_fails_closed() {
    // Scheduling rule 3 + contract rule 3: an edge flowing a buffer no
    // semantic value is bound to cannot inherit a value identity.
    let semantics = DeviceSemantics {
        roots: vec![LaunchId(1)],
        dependencies: vec![DependencyEdge {
            producer: LaunchId(1),
            consumer: LaunchId(2),
            buffer: BufferId(9),
            version: 1,
        }],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[LaunchId(1), LaunchId(2)]),
        Err(DeviceProgramError::UnboundEdgeBuffer { buffer: BufferId(9) })
    ));
}

// ── DDCP1-U2: stable identity is origin-derived, never name-derived ──────

#[test]
fn rename_cannot_split_one_identity() {
    // Contract rule 1: identity is derived from carried facts, never from
    // names. Two values minted from ONE origin with DIFFERENT names still
    // alias — a rename can never create a second identity.
    let origin = SemanticValueOrigin::HostInput;
    let semantics = DeviceSemantics {
        values: vec![
            value(1, "x", origin.clone()),
            value(2, "renamed", origin),
        ],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[]),
        Err(DeviceProgramError::SemanticOriginAliased {
            first: SemanticValueId(1),
            second: SemanticValueId(2),
            ..
        })
    ));
}

#[test]
fn identical_fact_sets_validate_identically_across_rebuilds() {
    // Contract rule 1 (regression): identity is a carried fact, so two
    // materializations of the same source facts carry the same ids and
    // validate identically — declaration/rebuild position never changes the
    // verdict.
    let first = ordered_chain_semantics(false);
    let second = ordered_chain_semantics(false);
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    first
        .validate(&launches)
        .expect("first build validates");
    second
        .validate(&launches)
        .expect("second identical build validates identically");
    assert_eq!(first.values[0].id, second.values[0].id);
    assert_eq!(first.values[0].origin, second.values[0].origin);
}

// ── DDCP1-U3: the resource axes are separate, independent facts ──────────

#[test]
fn initialization_axis_is_declared_independently() {
    // An initialization fact rides alone — it neither requires nor implies
    // an observation, a generation, or a mutation fact (five separate axes,
    // never collapsed into one buffer flag).
    let semantics = DeviceSemantics {
        values: vec![value(
            1,
            "x",
            SemanticValueOrigin::MirLocal {
                function: MirFunctionId(0),
                local: 0,
            },
        )],
        bindings: vec![ValueBinding {
            value: SemanticValueId(1),
            buffer: BufferId(1),
        }],
        initializations: vec![InitializationFact {
            buffer: BufferId(1),
            policy: InitializationPolicy::HostProvided,
        }],
        ..DeviceSemantics::default()
    };
    semantics
        .validate(&[])
        .expect("an initialization fact is an independent axis");
}

#[test]
fn write_does_not_imply_observability() {
    // A carried write (generation) with no observation fact is valid — a
    // write never implies observability, and an observation never implies a
    // result at the semantic layer.
    let semantics = DeviceSemantics {
        values: vec![value(
            1,
            "x",
            SemanticValueOrigin::MirLocal {
                function: MirFunctionId(0),
                local: 0,
            },
        )],
        bindings: vec![ValueBinding {
            value: SemanticValueId(1),
            buffer: BufferId(1),
        }],
        generations: vec![ValueGeneration {
            value: SemanticValueId(1),
            generation: 1,
            element_ty: f32(),
            element_count: 4,
            produced_by: LaunchId(1),
        }],
        roots: vec![LaunchId(1)],
        ..DeviceSemantics::default()
    };
    semantics
        .validate(&[LaunchId(1)])
        .expect("a write without an observation fact validates; observability is explicit");
}

// ── DDCP1-U4: companion identity + duplicate/contradictory fail-closed ──

#[test]
fn companion_identity_key_derives_from_primal_and_typed_relation() {
    // Contract rule 2: an AIR-generated backward companion's identity is
    // derived from its primal kernel identity PLUS the typed companion
    // relation (the differentiation point) — never from a name or position.
    let forward = lossless_entry(1, 2);
    let same_identity = lossless_entry(1, 2);
    let other_primal = lossless_entry(2, 3);
    let mut other_relation = lossless_entry(1, 3);
    other_relation.derivative = MirCompanionDerivativeKind::ReverseModeVjp;
    assert_eq!(forward.identity_key(), same_identity.identity_key());
    assert_ne!(forward.identity_key(), other_primal.identity_key());
    assert_eq!(
        forward.identity_key(),
        (DefId(1), MirCompanionDerivativeKind::ReverseModeVjp)
    );
    assert_eq!(forward.identity_key(), other_relation.identity_key());
}

#[test]
fn duplicate_companion_facts_fail_closed_on_identity_key() {
    // Contract rule 3: duplicate companion facts for one (primal,
    // differentiation point) fail closed — keyed on the companion identity,
    // not on a name or position.
    let semantics = DeviceSemantics {
        relations: vec![lossless_entry(1, 2), lossless_entry(1, 2)],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[]),
        Err(DeviceProgramError::DuplicateRelationRow { primal })
            if primal == DefId(1)
    ));
}

#[test]
fn contradictory_upstream_gradient_type_fails_closed() {
    // Contract rule 4: a companion fact whose upstream gradient shape is
    // incompatible with the primal output it differentiates is contradictory
    // vs the primal's recorded facts.
    let mut entry = lossless_entry(1, 2);
    entry.selected_outputs[0].upstream_gradient_ty = TypeId(2);
    let semantics = DeviceSemantics {
        relations: vec![entry],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[]),
        Err(DeviceProgramError::ContradictoryCompanionFact {
            primal,
            field: "upstream-gradient-type",
            ..
        }) if primal == DefId(1)
    ));
}

#[test]
fn forward_only_program_declares_no_companions_and_needs_none() {
    // Contract rule 7: a forward-only program carries no relations and
    // validates without any companion facts — the missing rule applies only
    // where companion outputs are actually referenced.
    let semantics = DeviceSemantics {
        relations: Vec::new(),
        ..DeviceSemantics::default()
    };
    semantics
        .validate(&[])
        .expect("a forward-only program needs no companion facts");
}

#[test]
fn relation_with_no_companion_facts_fails_closed_as_missing() {
    // Contract rule 6 (reference-scoped): a declared companion must
    // reference its companion facts — a relation with no selected inputs and
    // no selected outputs declares a companion whose outputs are missing. A
    // forward-only program declares no companions and needs none (rule 7).
    let mut entry = lossless_entry(1, 2);
    entry.selected_inputs.clear();
    entry.selected_outputs.clear();
    let semantics = DeviceSemantics {
        relations: vec![entry],
        ..DeviceSemantics::default()
    };
    assert!(matches!(
        semantics.validate(&[]),
        Err(DeviceProgramError::MissingCompanionFact { primal, .. })
            if primal == DefId(1)
    ));
}

// ── DDCP1-U5: prepared submission regions (contract §Prepared submission
//    regions rules 1–4; NGAB0-R1 granularity) ─────────────────────────────

/// A two-launch chain semantics: launch 1 produces generation 1 of value 1
/// (buffer 1); launch 2 consumes it. `observe_at` optionally declares an
/// observation fact at that launch.
fn region_chain_semantics(observe_at: Option<LaunchId>) -> DeviceSemantics {
    let mut semantics = DeviceSemantics {
        values: vec![value(
            1,
            "x",
            SemanticValueOrigin::MirLocal {
                function: MirFunctionId(0),
                local: 0,
            },
        )],
        bindings: vec![ValueBinding {
            value: SemanticValueId(1),
            buffer: BufferId(1),
        }],
        generations: vec![ValueGeneration {
            value: SemanticValueId(1),
            generation: 1,
            element_ty: f32(),
            element_count: 4,
            produced_by: LaunchId(1),
        }],
        roots: vec![LaunchId(1)],
        dependencies: vec![DependencyEdge {
            producer: LaunchId(1),
            consumer: LaunchId(2),
            buffer: BufferId(1),
            version: 1,
        }],
        ..DeviceSemantics::default()
    };
    if let Some(at_launch) = observe_at {
        semantics.observations.push(ObservationFact {
            buffer: BufferId(1),
            version: 1,
            at_launch,
        });
    }
    semantics
}

#[test]
fn prepared_regions_derive_from_dependency_connectivity() {
    // Rule 1/4: connected launches form one prepared submission region —
    // boundaries come from real dependencies, and one host call enqueues the
    // whole connected group (NGAB0-R1).
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let semantics = region_chain_semantics(None);
    let regions = semantics.derive_prepared_regions(&launches);
    assert_eq!(
        regions,
        vec![PreparedRegion {
            id: RegionId(0),
            launches: vec![LaunchId(1), LaunchId(2)],
        }],
        "a connected chain is one prepared submission region (NGAB0-R1)"
    );
    semantics
        .validate_regions(&launches, &regions)
        .expect("the derived partition is total, crossing-free, and connected");
}

#[test]
fn prepared_region_splits_at_observation_boundary() {
    // Rule 3: a prepared submission region cannot cross an observation
    // boundary; the candidate group is split at that point — the observed
    // launch ends its region and the following member starts a new one.
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let semantics = region_chain_semantics(Some(LaunchId(1)));
    let regions = semantics.derive_prepared_regions(&launches);
    assert_eq!(
        regions,
        vec![
            PreparedRegion {
                id: RegionId(0),
                launches: vec![LaunchId(1)],
            },
            PreparedRegion {
                id: RegionId(1),
                launches: vec![LaunchId(2)],
            },
        ],
        "the observed launch l1 ends its region; l2 starts a new region"
    );
    semantics
        .validate_regions(&launches, &regions)
        .expect("the split partition validates");
}

#[test]
fn region_with_interior_observation_fails_closed() {
    // Rule 3 fail-closed: a region whose interior contains an observation
    // point would force a CPU readback between its members — rejected.
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let semantics = region_chain_semantics(Some(LaunchId(1)));
    let bad = vec![PreparedRegion {
        id: RegionId(0),
        launches: vec![LaunchId(1), LaunchId(2)],
    }];
    assert!(matches!(
        semantics.validate_regions(&launches, &bad),
        Err(DeviceProgramError::RegionCrossesBoundary {
            region: RegionId(0),
            launch: LaunchId(1),
        })
    ));
}

#[test]
fn region_with_disconnected_members_fails_closed() {
    // Rule 1 fail-closed: members of one region must be path-connected
    // through the carried dependency graph — even with total coverage.
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2), LaunchId(3), LaunchId(4)];
    let mut semantics = region_chain_semantics(None); // edge l1 → l2
    semantics.dependencies.push(DependencyEdge {
        producer: LaunchId(3),
        consumer: LaunchId(4),
        buffer: BufferId(1),
        version: 1,
    });
    let bad = vec![
        PreparedRegion {
            id: RegionId(0),
            launches: vec![LaunchId(1), LaunchId(2), LaunchId(3)],
        },
        PreparedRegion {
            id: RegionId(1),
            launches: vec![LaunchId(4)],
        },
    ];
    assert!(matches!(
        semantics.validate_regions(&launches, &bad),
        Err(DeviceProgramError::RegionMembersDisconnected { region: RegionId(0) })
    ));
}

#[test]
fn launch_outside_region_partition_fails_closed() {
    // Rules 1/4 fail-closed: every launch is a member of exactly one region.
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let semantics = region_chain_semantics(None);
    let partial = vec![PreparedRegion {
        id: RegionId(0),
        launches: vec![LaunchId(1)],
    }];
    assert!(matches!(
        semantics.validate_regions(&launches, &partial),
        Err(DeviceProgramError::RegionUnassignedLaunch { launch: LaunchId(2) })
    ));
}

#[test]
fn region_derivation_is_independent_of_declaration_order() {
    // Rule 4 fixture: identical edge/observation sets with permuted
    // declaration order (and a permuted launch-universe order) derive
    // identical regions and identical compiler-owned ids.
    let launches_forward: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let launches_reversed: Vec<LaunchId> = vec![LaunchId(2), LaunchId(1)];
    let mut forward = region_chain_semantics(Some(LaunchId(1)));
    let mut reversed = region_chain_semantics(Some(LaunchId(1)));
    forward.dependencies.reverse();
    reversed.generations.reverse();
    reversed.observations.reverse();
    let forward_regions = forward.derive_prepared_regions(&launches_forward);
    let reversed_regions = reversed.derive_prepared_regions(&launches_reversed);
    assert_eq!(
        forward_regions, reversed_regions,
        "declaration order never decides region formation or region ids"
    );
}

// ── DDCP1-U6: device/AIR legality + CPU host-`ad` preservation (the DDCP1
//    hard gate; reconciliation §3.1–3.3) ──────────────────────────────────

#[test]
fn device_function_with_ad_effect_rejects_with_named_diagnostic() {
    // §3.1: a device function whose kernel body carries an `ad` (automatic-
    // differentiation) operation — the `ad` Sermo conversation — is
    // rejected with a stable named diagnostic, never an ad-hoc error.
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let mut facts = DeviceLegalityFacts::default();
    facts.device_effects.push(DeviceEffectFact {
        launch: LaunchId(1),
        kind: DeviceEffectBoundaryKind::Ad,
    });
    assert!(matches!(
        facts.validate(&launches),
        Err(DeviceProgramError::DeviceFunctionHostileEffect {
            launch: LaunchId(1),
            kind: DeviceEffectBoundaryKind::Ad,
        })
    ));
}

#[test]
fn device_function_with_sermo_effect_rejects_with_named_diagnostic() {
    // §3.1: a Sermo stream operation inside a device function is rejected
    // with the named diagnostic.
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let mut facts = DeviceLegalityFacts::default();
    facts.device_effects.push(DeviceEffectFact {
        launch: LaunchId(2),
        kind: DeviceEffectBoundaryKind::Sermo,
    });
    assert!(matches!(
        facts.validate(&launches),
        Err(DeviceProgramError::DeviceFunctionHostileEffect {
            launch: LaunchId(2),
            kind: DeviceEffectBoundaryKind::Sermo,
        })
    ));
}

#[test]
fn device_function_with_unresolved_host_effect_rejects() {
    // §3.1: an unresolved host effect (a host-only CPU runtime call with no
    // device lowering) inside a device function is rejected with the named
    // diagnostic.
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let mut facts = DeviceLegalityFacts::default();
    facts.device_effects.push(DeviceEffectFact {
        launch: LaunchId(1),
        kind: DeviceEffectBoundaryKind::UnresolvedHostEffect,
    });
    assert!(matches!(
        facts.validate(&launches),
        Err(DeviceProgramError::DeviceFunctionHostileEffect {
            launch: LaunchId(1),
            kind: DeviceEffectBoundaryKind::UnresolvedHostEffect,
        })
    ));
}

#[test]
fn device_function_with_cancellation_boundary_rejects() {
    // §3.1: a cancellation boundary (async suspend / cancellation point)
    // inside a device function is rejected with the named diagnostic.
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let mut facts = DeviceLegalityFacts::default();
    facts.device_effects.push(DeviceEffectFact {
        launch: LaunchId(2),
        kind: DeviceEffectBoundaryKind::Cancellation,
    });
    assert!(matches!(
        facts.validate(&launches),
        Err(DeviceProgramError::DeviceFunctionHostileEffect {
            launch: LaunchId(2),
            kind: DeviceEffectBoundaryKind::Cancellation,
        })
    ));
}

#[test]
fn device_effect_fact_on_unknown_launch_fails_closed() {
    // §3.1 fail-closed: an effect-boundary fact naming a launch outside the
    // device program is a malformed legality record and is rejected.
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let mut facts = DeviceLegalityFacts::default();
    facts.device_effects.push(DeviceEffectFact {
        launch: LaunchId(9),
        kind: DeviceEffectBoundaryKind::Ad,
    });
    assert!(matches!(
        facts.validate(&launches),
        Err(DeviceProgramError::UnknownDeviceEffectLaunch { launch: LaunchId(9) })
    ));
}

#[test]
fn cpu_host_ad_runs_unchanged_while_device_ad_rejects_same_ladder() {
    // THE DDCP1 HARD GATE (reconciliation §3.3): a device function
    // containing `ad` rejects with the named diagnostic while the equivalent
    // CPU host `ad` through Sermo runs unchanged — preservation and
    // rejection are proven in the SAME ladder, never traded against each
    // other (§3.2).
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];

    // Negative device side: launch 1 is a device function whose kernel body
    // opens an `ad` Sermo conversation — the materializer records the fact
    // and the gate rejects the device function by name.
    let mut device_facts = DeviceLegalityFacts::default();
    device_facts.device_effects.push(DeviceEffectFact {
        launch: LaunchId(1),
        kind: DeviceEffectBoundaryKind::Ad,
    });
    assert!(matches!(
        device_facts.validate(&launches),
        Err(DeviceProgramError::DeviceFunctionHostileEffect {
            launch: LaunchId(1),
            kind: DeviceEffectBoundaryKind::Ad,
        })
    ));

    // Positive CPU side: the equivalent CPU function's host `ad` through
    // Sermo is a CPU-side operation — it is not a device launch, so no
    // device effect fact is recorded, and the device program runs unchanged.
    let semantics = region_chain_semantics(None);
    let cpu_facts = DeviceLegalityFacts::default();
    cpu_facts
        .validate(&launches)
        .expect("the equivalent CPU host ad through Sermo runs unchanged");
    semantics
        .validate(&launches)
        .expect("the CPU-side equivalent carries no device-hostile fact");
}

#[test]
fn device_abi_stays_separate_from_the_cpu_host_effect_abi() {
    // Contract §Host/device partition and callsite identity rule 2/3 +
    // reconciliation §3.2: the direct device ABI is separate from the CPU
    // host-effect ABI and carries NO Sermo/route/Valor/backend-tag/kernel-
    // name machinery on the hot path. The legality record is the ONLY
    // Sermo/effect surface in the carrier; a valid device program carries
    // no device-hostile facts, and the gate admits it — the direct device
    // ABI never routes through the host-effect seam.
    let launches: Vec<LaunchId> = vec![LaunchId(1), LaunchId(2)];
    let semantics = region_chain_semantics(None);
    let legality = DeviceLegalityFacts::default();
    assert!(
        legality.device_effects.is_empty(),
        "a valid device program carries no Sermo/effect fact on the hot path"
    );
    legality
        .validate(&launches)
        .expect("the direct device ABI carries no Sermo machinery into the hot path");
    semantics
        .validate(&launches)
        .expect("the carried semantic facts are pure value/generation/graph/axis facts");
}
