//! DDCP3-U4 typed-completeness tests (gate bullet 5 + `HotPathGate` end-state +
//! execution-descriptor domain separation). Test owner: hand-8.
//!
//! The fixture is the NGAB0-U11 vertical shape: one Compute kernel
//! `scale_kernel` (f32 param → f32 return — the return-buffer kernel ABI)
//! and one host function `run_scale` whose single call statement crosses the
//! boundary into one prepared submission region (region identity 1). The
//! typed artifacts — the derived `HostPartition`, the `PartitionRecord`
//! (DDCP3-U3), the typed `HostDeviceCall` bindings, and the packet-facts
//! view of the revised `DeviceArtifact` packet — must jointly carry every
//! symbol/resource/dimension/argument-layout fact a backend materializer
//! needs for preparation.

use super::*;
use crate::boundary_abi::{MemberKernel, RegionArgumentLayout, derive_host_device_calls};
use crate::device::{MirDeviceFunctionMetadata, MirDeviceFunctionMetadataMap};
use crate::device_program::LaunchId;
use crate::device_semantics::DeviceLegalityFacts;
use crate::host_partition::{
    PartitionRecord, derive_host_partition_with_metadata, derive_partition_record_with_metadata,
};
use crate::{
    MirBlock, MirBlockId, MirCallee, MirDeviceRole, MirFunction, MirFunctionId, MirFunctionOrigin,
    MirFunctionVisibility, MirKernelShaderStage, MirLocal, MirLocalId, MirOperand, MirParam,
    MirParamMode, MirPlace, MirProgram, MirStatement, MirStatementKind,
    MirSyntheticFunctionIdentity, MirTerminator, MirTerminatorKind, MirType, MirValidationContext,
    ValidatedMir,
};
use radix_lexer::{Interner, Span};
use radix_types::{NumericWidth, Primitive, TypeTable};

fn f32(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

fn local(id: u32, ty: MirType) -> MirLocal {
    MirLocal {
        id: MirLocalId(id),
        ty,
        name: None,
        mutable: false,
        span: Span::default(),
    }
}

fn param(local_id: u32, ty: MirType) -> MirParam {
    MirParam {
        local: MirLocalId(local_id),
        ty,
        name: None,
        mode: MirParamMode::Owned,
        span: Span::default(),
    }
}

/// Explicit source-entry facts for this named-kernel fixture. The KCM
/// admission contract no longer infers launchability from the compute-stage
/// marker alone, so the fixture carries the same public Source row as the
/// product lowering path.
fn fixture_metadata(program: &MirProgram) -> MirDeviceFunctionMetadataMap {
    program
        .functions
        .iter()
        .filter(|function| function.shader_stage == Some(MirKernelShaderStage::Compute))
        .map(|function| {
            (
                function.id,
                MirDeviceFunctionMetadata {
                    identity: MirSyntheticFunctionIdentity {
                        parent: MirFunctionId(0),
                        ordinal: 0,
                    },
                    role: MirDeviceRole::Kernel,
                    shader_stage: MirKernelShaderStage::Compute,
                    visibility: MirFunctionVisibility::Public,
                    origin: MirFunctionOrigin::Source,
                    lane_domain_local: None,
                    refused_backends: Default::default(),
                    tier: crate::device::KernelTier::Default,
                },
            )
        })
        .collect()
}

/// The minimal one-region vertical fixture (NGAB0-U11 shape): one Compute
/// kernel f1 `scale_kernel` and one host function f2 `run_scale` whose single
/// call statement crosses the boundary.
fn vertical_program(types: &mut TypeTable, interner: &mut Interner) -> MirProgram {
    let f32_ty = f32(types);
    MirProgram {
        functions: vec![
            MirFunction {
                id: MirFunctionId(1),
                source: Some(radix_hir::DefId(1)),
                name: Some(interner.intern("scale_kernel")),
                params: vec![param(10, f32_ty)],
                locals: vec![local(10, f32_ty)],
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: Vec::new(),
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                            MirLocalId(10),
                        )))),
                        span: Span::default(),
                    },
                    span: Span::default(),
                }],
                return_ty: f32_ty,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: Some(MirKernelShaderStage::Compute),
                span: Span::default(),
            },
            MirFunction {
                id: MirFunctionId(2),
                source: None,
                name: Some(interner.intern("run_scale")),
                params: vec![param(20, f32_ty)],
                locals: vec![local(20, f32_ty), local(30, f32_ty)],
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: vec![MirStatement {
                        kind: MirStatementKind::Call {
                            destination: Some(MirPlace::local(MirLocalId(30))),
                            callee: MirCallee::Function(MirFunctionId(1)),
                            args: vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
                        },
                        span: Span::default(),
                    }],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                            MirLocalId(30),
                        )))),
                        span: Span::default(),
                    },
                    span: Span::default(),
                }],
                return_ty: f32_ty,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: Span::default(),
            },
        ],
    }
}

/// The derived partition + record for the vertical fixture — validation,
/// derivation, the boundary-ABI gate, and the legality gate all run exactly
/// as the product path runs them.
fn derive_vertical(
    types: &mut TypeTable,
    interner: &mut Interner,
) -> (HostPartition, PartitionRecord) {
    let program = vertical_program(types, interner);
    let context = MirValidationContext::new(types);
    let validated = ValidatedMir::new(program, context).expect("the vertical fixture validates");
    let metadata = fixture_metadata(validated.program());
    let partition = derive_host_partition_with_metadata(&validated, interner, &metadata)
        .expect("the vertical partition derives (boundary-ABI gate green)");
    let record = derive_partition_record_with_metadata(
        &validated,
        interner,
        "fixture-u11",
        &DeviceLegalityFacts::default(),
        &metadata,
    )
    .expect("the vertical partition record derives (legality gate green)");
    (partition, record)
}

/// The packet-facts view of the revised `DeviceArtifact` packet for the
/// derived partition — built the way a packet producer emits it (one
/// entrypoint-symbol row + one reflection row per kernel unit, from the typed
/// program facts, never from text).
fn packet_facts_for(partition: &HostPartition) -> ArtifactPacketFacts {
    ArtifactPacketFacts {
        entrypoint_symbols: partition
            .device_program
            .kernels
            .iter()
            .map(|unit| EntrypointSymbolFact {
                entry: unit.entry.clone(),
                symbol: format!("faber_device_{}", unit.entry),
            })
            .collect(),
        reflected_kernels: partition
            .device_program
            .kernels
            .iter()
            .map(|unit| ReflectedKernelFact {
                entry: unit.entry.clone(),
                resources: unit
                    .resources
                    .iter()
                    .map(|resource| ReflectedResourceFact {
                        name: resource.buffer.name.clone(),
                        binding: resource.binding.binding,
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// The compiler-owned neutral device entry of the fixture's single member
/// kernel: `k<function id>`, here `k1` (ruling ca2499dc). Source spellings
/// such as `scale_kernel` are decoration and never appear in device entry
/// facts.
fn fixture_kernel_entry(partition: &HostPartition) -> String {
    partition.device_program.kernels[0].entry.clone()
}

/// The completeness gate result for the vertical fixture, exactly as the
/// product path runs it.
fn vertical_bundles(types: &mut TypeTable, interner: &mut Interner) -> Vec<RegionPreparationFacts> {
    let (partition, record) = derive_vertical(types, interner);
    let calls = derive_host_device_calls(&partition, &record);
    let packet = packet_facts_for(&partition);
    check_preparation_completeness(&partition, &record, &calls, &packet)
        .expect("the vertical fixture passes the typed completeness gate")
}

#[test]
fn completeness_gate_derives_the_full_preparation_bundle() {
    // Gate bullet 5: the gate proves the typed artifacts jointly carry every
    // symbol, resource, dimension, and argument layout a backend materializer
    // needs — the complete per-region preparation bundle is derived from the
    // typed surfaces, never from emitted text.
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let (partition, record) = derive_vertical(&mut types, &mut interner);
    let calls = derive_host_device_calls(&partition, &record);
    let packet = packet_facts_for(&partition);
    let bundles = check_preparation_completeness(&partition, &record, &calls, &packet)
        .expect("the vertical fixture passes the typed completeness gate");

    assert_eq!(bundles.len(), 1);
    let region = &bundles[0];

    // Symbol facts: the stable region identity (the direct region symbol's
    // suffix) and the member kernel/entrypoint identities.
    assert_eq!(region.region, 1);
    assert_eq!(
        region.members,
        vec![MemberKernel {
            kernel: MirFunctionId(1),
            entry: fixture_kernel_entry(&partition),
        }]
    );

    // Argument layout: the bounded numeric dispatch extent, equal to the
    // typed launch plan's derived count (fixed at preparation).
    let plan = &partition.device_program.kernels[0].launch;
    assert_eq!(
        region.argument_layout,
        RegionArgumentLayout {
            dispatch_extent: plan.workgroup_count.x * u64::from(plan.workgroup.x),
        }
    );

    // Resource facts: every typed buffer slot of the member kernel, with its
    // binding, role, element type, and element count.
    assert!(
        !region.resources.is_empty(),
        "the region carries typed resource facts"
    );
    for resource in &region.resources {
        assert_eq!(resource.kernel, MirFunctionId(1));
        assert!(
            resource.element_count > 0,
            "a typed element-count shape fact"
        );
    }

    // Dimension facts: the typed workgroup/dispatch/workgroup-count triples.
    assert_eq!(region.dimensions.len(), 1);
    let dimension = &region.dimensions[0];
    assert_eq!(dimension.kernel, 1);
    assert_eq!(dimension.workgroup_count.0, plan.workgroup_count.x);
    assert_eq!(dimension.workgroup.0, u64::from(plan.workgroup.x));

    // The typed HostDeviceCall row: the boundary call's typed bindings with
    // the real callsite (never the sentinel fallback).
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call.region, 1);
    assert_eq!(call.callsite.block, MirBlockId(0));
    assert_eq!(call.callsite.statement, 0);
    assert_eq!(call.members, region.members);
    assert_eq!(call.argument_layout, region.argument_layout);
    assert!(
        !call.bindings.is_empty(),
        "the typed bindings are declared per region"
    );
    assert!(
        call.effect_boundary.is_empty(),
        "a legal region records no effect boundary (fail-closed envelope)"
    );

    // The record cross-check: the region row + callsite mapping agree.
    assert_eq!(record.regions.len(), 1);
    assert_eq!(record.regions[0].region, 1);
    assert_eq!(record.callsites.len(), 1);
}

#[test]
fn completeness_gate_fails_closed_on_missing_packet_entrypoint_symbol() {
    // A packet whose entrypoint symbol map lacks the member's entry fails
    // closed with the named rule — the packet does not carry the symbol fact.
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let (partition, record) = derive_vertical(&mut types, &mut interner);
    let calls = derive_host_device_calls(&partition, &record);
    let mut packet = packet_facts_for(&partition);
    packet.entrypoint_symbols.clear();
    assert!(matches!(
        check_preparation_completeness(&partition, &record, &calls, &packet),
        Err(CompletenessError::AbsentPacketEntrypointSymbol {
            region: 1,
            kernel: MirFunctionId(1),
            entry,
        }) if entry == fixture_kernel_entry(&partition)
    ));

    // An empty emitted symbol is equally a completeness failure.
    packet.entrypoint_symbols.push(EntrypointSymbolFact {
        entry: fixture_kernel_entry(&partition),
        symbol: String::new(),
    });
    assert!(matches!(
        check_preparation_completeness(&partition, &record, &calls, &packet),
        Err(CompletenessError::AbsentPacketEntrypointSymbol { .. })
    ));
}

#[test]
fn completeness_gate_fails_closed_on_missing_packet_reflection() {
    // A packet whose reflection lacks a row for the member fails closed.
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let (partition, record) = derive_vertical(&mut types, &mut interner);
    let calls = derive_host_device_calls(&partition, &record);
    let mut packet = packet_facts_for(&partition);
    packet.reflected_kernels.clear();
    assert!(matches!(
        check_preparation_completeness(&partition, &record, &calls, &packet),
        Err(CompletenessError::AbsentPacketReflectionRow {
            region: 1,
            kernel: MirFunctionId(1),
            entry,
        }) if entry == fixture_kernel_entry(&partition)
    ));

    // A reflection whose declared resource bindings disagree with the typed
    // partition facts fails closed (PacketReflectionMismatch).
    let mut packet = packet_facts_for(&partition);
    for kernel in &mut packet.reflected_kernels {
        kernel.resources.clear();
    }
    assert!(matches!(
        check_preparation_completeness(&partition, &record, &calls, &packet),
        Err(CompletenessError::PacketReflectionMismatch { .. })
    ));
}

#[test]
fn completeness_gate_fails_closed_on_missing_or_inconsistent_typed_calls() {
    // No typed HostDeviceCall row for the derived region fails closed.
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let (partition, record) = derive_vertical(&mut types, &mut interner);
    let packet = packet_facts_for(&partition);
    assert!(matches!(
        check_preparation_completeness(&partition, &record, &[], &packet),
        Err(CompletenessError::AbsentHostDeviceCall { region: 1 })
    ));

    // A HostDeviceCall whose argument layout disagrees with the derived
    // partition fails closed (never silently reconstructed).
    let mut calls = derive_host_device_calls(&partition, &record);
    calls[0].argument_layout.dispatch_extent += 1;
    assert!(matches!(
        check_preparation_completeness(&partition, &record, &calls, &packet),
        Err(CompletenessError::HostDeviceCallMismatch { region: 1, .. })
    ));
}

#[test]
fn completeness_gate_fails_closed_on_missing_or_inconsistent_record() {
    // A record without the derived region's row fails closed.
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let (partition, mut record) = derive_vertical(&mut types, &mut interner);
    let calls = derive_host_device_calls(&partition, &record);
    let packet = packet_facts_for(&partition);
    record.regions.clear();
    assert!(matches!(
        check_preparation_completeness(&partition, &record, &calls, &packet),
        Err(CompletenessError::AbsentRegionRecord { region: 1 })
    ));

    // A record region row that disagrees with the derived partition fails
    // closed.
    let (partition, mut record) = derive_vertical(&mut types, &mut interner);
    let calls = derive_host_device_calls(&partition, &record);
    record.regions[0].member_kernels.clear();
    assert!(matches!(
        check_preparation_completeness(&partition, &record, &calls, &packet),
        Err(CompletenessError::RegionRecordMismatch { region: 1 })
    ));

    // A record without the callsite mapping fails closed.
    let (partition, mut record) = derive_vertical(&mut types, &mut interner);
    let calls = derive_host_device_calls(&partition, &record);
    record.callsites.clear();
    assert!(matches!(
        check_preparation_completeness(&partition, &record, &calls, &packet),
        Err(CompletenessError::AbsentCallsite { region: 1 })
    ));
}

#[test]
fn hot_path_gate_end_state_typed_artifact_level_proof() {
    // Gate 4 typed-artifact-level proof (the U2 proof is the emission-level
    // proof): the vertical fixture's prepared region + direct call satisfy
    // the full HotPathGate rule set end-to-end over the typed artifacts.
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let (partition, record) = derive_vertical(&mut types, &mut interner);
    let calls = derive_host_device_calls(&partition, &record);
    let bundles = vertical_bundles(&mut types, &mut interner);
    let region = &bundles[0];

    // Rule 1 (ABI denylist): the typed argument layout is bounded numeric
    // fields only — a single numeric dispatch extent, no strings, maps,
    // `Valor`, route/backend tags, or tensor payloads. (The U2 emission-level
    // fixture greps the emitted call for the same denial; the typed-level
    // proof is the layout's shape: exactly the numeric dispatch extent.)
    let _ = &calls[0].argument_layout;
    assert_eq!(
        region.argument_layout,
        RegionArgumentLayout {
            dispatch_extent: region.argument_layout.dispatch_extent,
        },
        "the argument layout carries exactly the bounded numeric dispatch extent"
    );

    // Rule 3 (preparation counters): every count is fixed at prepare — the
    // bundle's dispatch extent equals the typed launch plan's derived count,
    // with nothing derived after preparation.
    let plan = &partition.device_program.kernels[0].launch;
    assert_eq!(
        region.argument_layout.dispatch_extent,
        plan.workgroup_count.x * u64::from(plan.workgroup.x)
    );

    // Rule 4 (zero post-prepare lookup): the bundle is fully concrete (no
    // `Option`/map field — nothing to resolve after prepare) and the region
    // is referenced by its fixed identity only.
    assert_eq!(region.region, 1);
    assert_eq!(region.members.len(), 1);
    assert_eq!(region.dimensions.len(), 1);

    // Rule 5 (syncs from declared observations, never kernel count): the
    // region's declared-observation count derives from the declared result
    // rows of its member launches — equal to the program's declared results,
    // independent of any kernel-count heuristic.
    assert_eq!(region.declared_observations, 1);
    assert_eq!(
        region.declared_observations,
        u32::try_from(partition.device_program.results.len()).unwrap_or(u32::MAX),
        "the sync/readback count is the declared observation count"
    );
    assert_eq!(
        partition.device_program.results[0].produced_by,
        LaunchId(1),
        "the declared observation is the output buffer's EndOfRun readback"
    );
}

#[test]
fn execution_descriptor_hash_is_a_distinct_typed_domain_never_fnv() {
    // CTO9-5 / §IdentityDomains row 4: the execution-descriptor identity is a
    // distinct typed domain/purpose from artifact identity (`content_sha256`/
    // `packet_sha256`) — never interchangeable and never FNV.
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let (partition, record) = derive_vertical(&mut types, &mut interner);
    let calls = derive_host_device_calls(&partition, &record);
    assert_eq!(calls.len(), 1);
    let descriptor = calls[0].execution_descriptor();

    // Collision-resistant digest (never FNV): canonical `sha256:<64-hex>`.
    assert!(descriptor.digest().starts_with("sha256:"));
    assert_eq!(
        descriptor.digest().len(),
        "sha256:".len() + 64,
        "the execution-descriptor digest is a SHA-256, never an FNV hash"
    );

    // Canonical: identical typed calls always produce identical descriptors.
    assert_eq!(calls[0].execution_descriptor(), descriptor);

    // The descriptor covers the typed call semantics: a different region
    // (same payload shape) yields a different descriptor.
    let mut other = calls[0].clone();
    other.region = 2;
    assert_ne!(other.execution_descriptor(), descriptor);

    // The canonical bytes begin with the execution-descriptor domain tag — the
    // type-level distinction from the artifact digest domains (the tag
    // participates in the digest, so the domains can never collide).
    let bytes = calls[0].canonical_descriptor_bytes();
    assert!(
        bytes.starts_with(b"faber.execution-descriptor.v1"),
        "the descriptor bytes carry the execution-descriptor domain tag"
    );
    assert!(
        bytes.len() > b"faber.execution-descriptor.v1".len(),
        "the domain tag alone is not the descriptor"
    );
}
