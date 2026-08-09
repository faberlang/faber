//! NGAB1-U3 facts-survive proof tests (radix-mir side).
//!
//! Proves the phase thesis — resource identity, lifetimes, mutation rules,
//! and observation facts from the semantic program survive to the device
//! program AND the ABI — at the MIR surface: a validated MIR program whose
//! kernel ABI carries the semantic facts is derived through
//! [`derive_host_partition`] into a typed [`DeviceProgram`] plus the
//! versioned boundary surface, and every fact is asserted to survive with
//! its exact typed value — never dropped, never re-derived from text.
//!
//! The fixture is the NGAB0-U11 shape at the MIR level: one scalar host
//! function `run_scale` calling one device kernel `scale_kernel` through one
//! prepared submission region (NGAB0-R1). The kernel's ABI resources are
//! synthesized by the shared
//! [`MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry`]
//! synthesis — the same typed path the emitters and the faber materializer
//! consume — and the assertions compare the derived program against those
//! *carried semantic facts* directly (the independent synthesis is the source
//! of truth, so nothing is self-confirmed by the partition step).

use super::*;
use crate::boundary_abi::HOST_CALL_ABI_VERSION;
use crate::device_program::Binding;
use crate::{
    MirBlock, MirBlockId, MirCallee, MirFunction, MirFunctionId, MirKernelShaderStage, MirLocal,
    MirLocalId, MirOperand, MirParam, MirParamMode, MirPlace, MirProgram, MirStatement,
    MirStatementKind, MirTerminator, MirTerminatorKind, MirType, MirValidationContext,
    ValidatedMir,
};
use radix_lexer::{Interner, Span};
use radix_types::{IndexExpr, NumericWidth, Primitive, TypeTable};

/// f32 (`fractus<f32>`) — the fixture's scalar surface.
fn f32(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

fn local(id: u32, ty: MirType) -> MirLocal {
    MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable: true,
        span: Span::default(),
    }
}

fn param(local_id: u32, ty: MirType) -> MirParam {
    MirParam {
        local: MirLocalId(local_id),
        name: None,
        ty,
        mode: MirParamMode::Owned,
        span: Span::default(),
    }
}

/// `tensor<f32, [n]>` — a literal-shaped device view (the vector call shape).
fn view(types: &mut TypeTable, element: MirType, count: u64) -> MirType {
    let shape = types.intern_index(IndexExpr::Literal(count));
    MirType::semantic(types.tensor_with_shape(element.semantic_id(), shape))
}

/// A return-buffer kernel unit: `id`'s only param (local `id*10`) is returned
/// as-is. The ABI synthesis derives the resources from the param/return types.
fn kernel_unit(id: u32, name: &str, ty: MirType, interner: &mut Interner) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: Some(interner.intern(name)),
        params: vec![param(id * 10, ty)],
        locals: vec![local(id * 10, ty)],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(id * 10),
                )))),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: Some(MirKernelShaderStage::Compute),
        span: Span::default(),
    }
}

/// The NGAB0-U11 shape at the MIR level: one Compute kernel f1
/// `scale_kernel` (one f32 parameter, f32 return — the return-buffer kernel
/// ABI) and one host function f2 `run_scale` whose single call statement
/// crosses the boundary into the prepared region. The kernel's ABI resources
/// are the semantic facts the shared signature synthesis derives from the
/// function's param/return types.
fn u11_program(types: &mut TypeTable, interner: &mut Interner) -> MirProgram {
    let f32_ty = f32(types);
    MirProgram {
        functions: vec![
            MirFunction {
                id: MirFunctionId(1),
                source: None,
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

/// The derived partition for the U11 MIR fixture — validation, derivation,
/// and the versioned ABI gate all run exactly as the product path runs them.
fn derive_u11(types: &mut TypeTable, interner: &mut Interner) -> HostPartition {
    let program = u11_program(types, interner);
    let context = MirValidationContext::new(types);
    let validated = ValidatedMir::new(program, context).expect("the U11 MIR fixture validates");
    derive_host_partition(&validated, interner).expect("the U11 partition derives")
}

/// Resource identity + mutation rules survive: the derived [`DeviceProgram`]
/// carries every storage-buffer resource of the kernel's semantic ABI —
/// binding, access (the mutation rule fact, verbatim), element type, element
/// count, and the mapped program role — with exact typed values. Nothing is
/// dropped and nothing is re-derived from text.
#[test]
fn resource_identity_and_mutation_rules_survive_to_device_program() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let program = u11_program(&mut types, &mut interner);
    let context = MirValidationContext::new(&types);
    let validated = ValidatedMir::new(program, context).expect("the U11 MIR fixture validates");
    let kernel = validated
        .program()
        .function_by_id(MirFunctionId(1))
        .unwrap();

    // The semantic facts: the shared ABI synthesis from the kernel function
    // (the source of truth — the same typed path the emitters consume).
    let signature =
        crate::abi::MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
            kernel,
            validated.validation(),
            &interner,
        )
        .expect("the U11 kernel's ABI synthesizes");

    let partition =
        derive_host_partition(&validated, &interner).expect("the U11 partition derives");
    let derived = &partition.device_program.kernels[0];

    // Every storage-buffer resource of the semantic signature survives, in
    // binding order, with exact typed values.
    let semantic: Vec<_> = signature
        .resources()
        .filter(|resource| resource.kind == crate::abi::MirKernelResourceKind::StorageBuffer)
        .collect();
    assert_eq!(derived.resources.len(), semantic.len());
    for (derived_slot, resource) in derived.resources.iter().zip(&semantic) {
        // Binding facts: carried from the semantic resource.
        assert_eq!(
            derived_slot.binding,
            Binding {
                group: resource.group,
                binding: resource.binding,
            },
            "binding identity survives verbatim"
        );
        // Mutation rule: the slot's read/write access survives verbatim — a
        // read-only input stays Read, a written output stays Write (the fact
        // the boundary gate uses to reject in-place mutation).
        assert_eq!(
            derived_slot.access, resource.access,
            "the slot's access (mutation rule fact) survives verbatim"
        );
        // Content version: element type and count carried from the resource.
        assert_eq!(derived_slot.version.element_ty, resource.element_ty);
        assert_eq!(derived_slot.version.element_count, resource.element_count);
        assert_eq!(
            derived_slot.version.reduced_projection, resource.reduced_projection,
            "the reduced-buffer projection fact survives when carried"
        );
        // Program role: the documented ABI role/access map (never text).
        assert_eq!(
            derived_slot.buffer.role,
            BufferRole::from_abi_role(resource.role, resource.access)
                .expect("the pair maps to a coherent program role")
        );
    }

    // The U11 shape: one 1-element f32 input at binding 0 (read-only) and
    // one 1-element f32 output at binding 1 (written) — the exact semantic
    // facts of the fixture.
    assert_eq!(derived.resources[0].buffer.role, BufferRole::Input);
    assert_eq!(
        derived.resources[0].access,
        crate::abi::MirKernelResourceAccess::Read
    );
    assert_eq!(derived.resources[0].binding.binding, 0);
    assert_eq!(derived.resources[0].version.element_count, 1);
    assert_eq!(derived.resources[1].buffer.role, BufferRole::Output);
    assert_eq!(
        derived.resources[1].access,
        crate::abi::MirKernelResourceAccess::Write
    );
    assert_eq!(derived.resources[1].binding.binding, 1);
    assert_eq!(derived.resources[1].version.element_count, 1);
    assert_eq!(
        derived.resources[0].version.element_ty, derived.resources[1].version.element_ty,
        "in/out ride one element type — the carried f32 fact"
    );
}

/// Lifetimes survive: the one-shot boundary-call regime of the NGAB0-U11
/// vertical slice is carried as the program lifetime (`SingleRun`) and every
/// buffer lifetime (`PerProgram`), and the observation rows are declared for
/// exactly the output-role buffers — produced by the kernel's launch, read
/// back once at end of run. No observation is inferred from role or text.
#[test]
fn lifetimes_and_observation_facts_survive_to_device_program() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let partition = derive_u11(&mut types, &mut interner);
    let program = &partition.device_program;

    // Program lifetime: the declared single-run regime.
    assert_eq!(program.lifetime, DeviceProgramLifetime::SingleRun);
    // Buffer lifetime: every slot's storage lives for the whole program.
    for kernel in &program.kernels {
        for slot in &kernel.resources {
            assert_eq!(slot.buffer.lifetime, BufferLifetime::PerProgram);
            assert_eq!(
                slot.buffer.storage,
                MirTensorStorageLayout::DeviceHandle,
                "device-program buffers are device-resident"
            );
        }
    }

    // One ordered launch over the kernel.
    assert_eq!(program.launches.len(), 1);
    assert_eq!(program.launches[0].kernel_index, 0);
    let launch_id = program.launches[0].id;

    // Observations: exactly one declared result row — the output-role buffer,
    // produced by the single launch, read back once at the end of the run.
    // A declared observation point, never an inference from role/lifetime.
    let output = &program.kernels[0].resources[1];
    assert_eq!(program.results.len(), 1);
    let result = &program.results[0];
    assert_eq!(result.buffer.id, output.buffer.id);
    assert_eq!(result.role, BufferRole::Output);
    assert_eq!(result.buffer.role, BufferRole::Output);
    assert_eq!(result.produced_by, launch_id);
    assert_eq!(result.cadence, ObservationCadence::EndOfRun);

    // The derived program passes the shared schema (the derivation ran the
    // gate already, but the observation/lifetime cross-checks are proven here
    // on the carried facts: EndOfRun × PerProgram is the legal pair).
    program
        .validate()
        .expect("derived program passes shared schema validation");
}

/// The ABI surface survives: the derived partition carries the declared
/// boundary call — one host function → one prepared region containing the
/// kernel (NGAB0-R1) — riding the versioned call ABI, and the typed facts
/// (identity, lifetimes, mutation/observation rules) are what the compile-time
/// gate consumed to accept the fixture. The kernel's identity fact on the
/// device program is the carried logical entry name, never a text-derived
/// symbol.
#[test]
fn boundary_abi_and_entry_identity_survive() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let partition = derive_u11(&mut types, &mut interner);

    // The versioned typed boundary.
    assert_eq!(partition.call_abi_version, HOST_CALL_ABI_VERSION);
    // One declared boundary call: host f2 → the minimal one-kernel region.
    assert_eq!(partition.boundary_calls.len(), 1);
    let call = &partition.boundary_calls[0];
    assert_eq!(call.host, MirFunctionId(2));
    assert_eq!(call.region.kernels, vec![MirFunctionId(1)]);
    assert!(
        call.region.is_minimal(),
        "the fixture's prepared region is the minimal one-kernel case"
    );
    // Derivation succeeding IS the ABI gate accepting the carried crossing
    // values (exact type, shape, lifetime, mutation, observation).

    // Entry identity: the kernel's logical entry name is the carried source
    // name fact — the only String fact on the typed program.
    assert_eq!(
        partition.device_program.kernels[0].function,
        MirFunctionId(1)
    );
    assert_eq!(
        partition.device_program.kernels[0].entry, "scale_kernel",
        "the entry identity is the carried logical name, not a text-derived symbol"
    );
    // No device program carries source text: the one String fact on the
    // kernel unit is the entry name (everything else is typed). The buffer
    // names are the deterministic wire/diagnostic labels of the semantic
    // slots (source_name carried when present, else role/binding fallback).
    let kernel = &partition.device_program.kernels[0];
    assert_eq!(
        kernel.resources[0].buffer.name, "input_0",
        "the input slot name is the deterministic role/binding label"
    );
    assert_eq!(
        kernel.resources[1].buffer.name, "output_1",
        "the output slot name is the deterministic role/binding label"
    );
    assert_eq!(partition.device_kernels, vec![MirFunctionId(1)]);
    assert!(!partition.host_functions.contains(&MirFunctionId(1)));
}

// ── NGAB1-U4: batch compatible call shapes ────────────────────────────────
//
// After the first accepted call shape (the scalar), compatible shapes batch
// through the same ABI without new mechanisms: the vector/2D extents resolve
// through the shared extent synthesis (the `MirLayoutKind` variant dispatch —
// the extensible table, never per-shape code), and the prepared region is
// the call's submission unit (NGAB0-R1), carrying the batch's kernels rather
// than one ABI row per kernel.

/// The vector call shape derives through the full partition: a 4-element
/// device-view argument crosses the versioned ABI gate (the shape check
/// resolves the view's literal extent) and the element-count fact survives
/// into the derived device program's resources.
#[test]
fn vector_arg_shape_survives_derive() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let f32_ty = f32(&mut types);
    let vector_ty = view(&mut types, f32_ty, 4);
    let program = MirProgram {
        functions: vec![
            MirFunction {
                id: MirFunctionId(1),
                source: None,
                name: Some(interner.intern("vec_kernel")),
                params: vec![param(10, vector_ty)],
                locals: vec![local(10, vector_ty)],
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
                return_ty: vector_ty,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: Some(MirKernelShaderStage::Compute),
                span: Span::default(),
            },
            MirFunction {
                id: MirFunctionId(2),
                source: None,
                name: Some(interner.intern("run_vec")),
                params: vec![param(20, vector_ty)],
                locals: vec![local(20, vector_ty), local(30, vector_ty)],
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
                return_ty: vector_ty,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: Span::default(),
            },
        ],
    };
    let context = MirValidationContext::new(&types);
    let validated = ValidatedMir::new(program, context).expect("the vector fixture MIR validates");
    let partition =
        derive_host_partition(&validated, &interner).expect("the vector partition derives");

    // One boundary call; the gate accepted the exact 4-element crossing.
    assert_eq!(partition.boundary_calls.len(), 1);
    assert_eq!(
        partition.boundary_calls[0].region.kernels,
        vec![MirFunctionId(1)]
    );
    assert_eq!(partition.call_abi_version, HOST_CALL_ABI_VERSION);

    // The vector element-count fact survives into the derived device program.
    let kernel = &partition.device_program.kernels[0];
    assert_eq!(kernel.entry, "vec_kernel");
    assert_eq!(kernel.resources.len(), 2);
    assert_eq!(kernel.resources[0].version.element_count, 4);
    assert_eq!(kernel.resources[1].version.element_count, 4);
    assert_eq!(
        kernel.resources[0].version.element_ty, kernel.resources[1].version.element_ty,
        "in/out ride one element type — the carried f32 fact"
    );
}

/// Two-kernel composition derives as ONE boundary call → ONE prepared region
/// containing the batch's kernels (in invocation order) — the region, not the
/// kernel, is the call's submission unit; there are no per-kernel ABI rows.
/// Both kernels' typed facts (identity, ordered launches, declared
/// observations) survive into the derived device program.
#[test]
fn two_kernel_composition_derives_one_region() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let f32_ty = f32(&mut types);
    let program = MirProgram {
        functions: vec![
            kernel_unit(1, "scale_kernel", f32_ty, &mut interner),
            kernel_unit(3, "offset_kernel", f32_ty, &mut interner),
            MirFunction {
                id: MirFunctionId(2),
                source: None,
                name: Some(interner.intern("run_batch")),
                params: vec![param(20, f32_ty)],
                locals: vec![local(20, f32_ty), local(30, f32_ty), local(31, f32_ty)],
                temps: Vec::new(),
                blocks: vec![MirBlock {
                    id: MirBlockId(0),
                    statements: vec![
                        MirStatement {
                            kind: MirStatementKind::Call {
                                destination: Some(MirPlace::local(MirLocalId(30))),
                                callee: MirCallee::Function(MirFunctionId(1)),
                                args: vec![MirOperand::Place(MirPlace::local(MirLocalId(20)))],
                            },
                            span: Span::default(),
                        },
                        MirStatement {
                            kind: MirStatementKind::Call {
                                destination: Some(MirPlace::local(MirLocalId(31))),
                                callee: MirCallee::Function(MirFunctionId(3)),
                                args: vec![MirOperand::Place(MirPlace::local(MirLocalId(30)))],
                            },
                            span: Span::default(),
                        },
                    ],
                    terminator: MirTerminator {
                        kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                            MirLocalId(31),
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
    };
    let context = MirValidationContext::new(&types);
    let validated = ValidatedMir::new(program, context).expect("the two-kernel MIR validates");
    let partition =
        derive_host_partition(&validated, &interner).expect("the two-kernel partition derives");

    // The batch rides ONE boundary call → ONE prepared region, in invocation
    // order — never per-kernel ABI rows (NGAB0-R1 granularity).
    assert_eq!(
        partition.device_kernels,
        vec![MirFunctionId(1), MirFunctionId(3)]
    );
    assert_eq!(partition.boundary_calls.len(), 1);
    let call = &partition.boundary_calls[0];
    assert_eq!(call.host, MirFunctionId(2));
    assert_eq!(
        call.region.kernels,
        vec![MirFunctionId(1), MirFunctionId(3)],
        "the batch's kernels ride one prepared region"
    );
    assert!(!call.region.is_minimal());

    // Both kernels derive into the typed device program with their entry
    // identities, ordered launches, and declared observations.
    let program = &partition.device_program;
    assert_eq!(program.kernels.len(), 2);
    assert_eq!(program.kernels[0].entry, "scale_kernel");
    assert_eq!(program.kernels[1].entry, "offset_kernel");
    assert_eq!(program.launches.len(), 2);
    assert_eq!(program.launches[0].kernel_index, 0);
    assert_eq!(program.launches[1].kernel_index, 1);
    assert_eq!(program.results.len(), 2);
    assert_eq!(program.results[0].produced_by, program.launches[0].id);
    assert_eq!(program.results[1].produced_by, program.launches[1].id);
    program
        .validate()
        .expect("the derived program passes the shared schema");
}
