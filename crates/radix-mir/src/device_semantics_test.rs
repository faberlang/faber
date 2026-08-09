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
