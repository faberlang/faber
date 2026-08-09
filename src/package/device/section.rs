// Sibling + root items: explicit `use super` lists carry the seams the mir/
// split routes through `use super::*` (wildcard imports are denied).
use super::{
    admit_session_section, device_diag, function_has_shape_construction, wire_program_for_program,
    BTreeMap, BTreeSet, DescriptorBuffer, DescriptorBufferVersion, DescriptorEndOfRunResult,
    DescriptorKernel, DescriptorLaunch, DescriptorResult, DeviceArtifactFormat, DeviceBackend,
    DeviceBufferInitialization, DeviceBufferLifetime, DeviceBufferRole, DeviceDataType,
    DeviceDescriptor, DevicePayloadEncoding, DeviceProgram, DeviceSelection, DeviceSemantics,
    DeviceTargetId, Diagnostic, FmirDeviceArtifact, FmirDeviceArtifactsSection, FmirDeviceBackend,
    FmirDeviceInput, FmirDeviceProgramSection, FmirDeviceSection, FmirDeviceSelection,
    FmirDeviceSymbol, HostDescriptorDataFlow, HostDeviceProgramLifetime, Interner,
    MaterializationStage, MirFunctionId, ValidatedMir, WireBufferLifetime, WireBufferRole,
    WireDeviceProgram, WireInitializationPolicy, WireObservationCadence, WireProgramLifetime,
    WireResourceAccess, DEVICE_RUN_PLAN_VERSION,
};
// Doc-link surface: the host session/descriptor types appear only in
// intra-doc links here (their code uses live in run.rs); the import keeps
// the links resolvable from this module.
#[allow(unused_imports)]
use super::{DeviceProgramLifetime, ProgramSession};

// ---------------------------------------------------------------------------
// FMIR device-section assembly
// ---------------------------------------------------------------------------

/// Package-owned inputs needed to assemble the serialized device section.
pub(crate) struct DeviceSectionBuild<'a> {
    pub(crate) selection: DeviceSelection,
    pub(crate) inputs: &'a BTreeMap<String, Vec<f32>>,
    pub(crate) ptx_target: &'a str,
    pub(crate) repeating_steps: u32,
}

/// The declared ABI/schema version of faber's emitted device artifacts — an
/// input to the `packet_sha256` identity (DDCP2-U3). Matches the radix
/// producer/closeout convention.
const DEVICE_ARTIFACT_ABI_VERSION: u32 = 1;

/// Compute the canonical `content_sha256`/`packet_sha256` identity digests of
/// a declared device artifact over its canonical decoded bytes (DDCP2-U3; the
/// same convention the radix fixtures use). Admission re-verifies both digests
/// against the carried bytes and metadata (B3).
fn device_artifact_with_digests(mut artifact: FmirDeviceArtifact) -> FmirDeviceArtifact {
    artifact.content_sha256 = artifact.compute_content_sha256();
    artifact.packet_sha256 = artifact.compute_packet_sha256();
    artifact
}

/// Assemble the FMIR `device` section for a constructed device program.
///
/// Emits both backend artifacts through the S1-3 emitters (Metal MSL always;
/// CUDA PTX through the admitted clang NVPTX compiler when present — a
/// machine without the build-time compiler carries no CUDA artifact and
/// `--backend cuda` fails closed at run time as a missing declared
/// artifact), builds the canonical run-plan payload (with the host input
/// values), and records the selection + runtime requirements.
///
/// # Errors
/// Fail-closed when artifact emission fails (a carried plan or binding that
/// contradicts the typed function facts fails closed, A3).
pub(crate) fn device_section_for_program(
    program: &DeviceProgram,
    semantics: &DeviceSemantics,
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    build: DeviceSectionBuild<'_>,
) -> Result<FmirDeviceSection, Vec<Diagnostic>> {
    let DeviceSectionBuild {
        selection,
        inputs,
        ptx_target,
        repeating_steps,
    } = build;
    // S5-U5c: the emitters re-derive each kernel's body from the validated
    // MIR, so the shape-folded bodies the constructor planned must reach
    // them too — a kernel's function id still references the ORIGINAL
    // (unfolded) body in `validated`. Build a folded validated token: the
    // SAME fold the constructor applied, applied to the program's
    // kernel-referenced functions in a cloned program (a shape-bearing
    // kernel the constructor admitted folds here with identical inputs;
    // unfoldable shapes were already rejected closed there).
    let mut emitter_program = validated.program().clone();
    let kernel_ids: BTreeSet<MirFunctionId> = program
        .kernels
        .iter()
        .map(|kernel| kernel.function)
        .collect();
    for function in &mut emitter_program.functions {
        if !kernel_ids.contains(&function.id) || !function_has_shape_construction(function) {
            continue;
        }
        let outcome =
            radix_mir::static_shape_fold::fold_static_shapes(function, validated.validation())
                .map_err(|error| vec![device_diag("shape fold", error.message)])?;
        *function = outcome.function;
    }
    let emitter_context = validated.validation().clone();
    let emitter_validated =
        ValidatedMir::new(emitter_program, emitter_context).map_err(|errors| {
            errors
                .into_iter()
                .map(|error| device_diag("shape fold", error.message))
                .collect::<Vec<_>>()
        })?;

    let metal_artifact =
        radix_mir_metal::emit_metal_device_artifact(program, &emitter_validated, interner)
            .map_err(|error| vec![device_diag("metal artifact", error.to_string())])?;
    // S3-A5 (Metal lane): the CUDA artifact emission is best-effort — an
    // emitter op the CUDA lane does not support yet (the companion's
    // elementwise surface lands in S3-A7) leaves the image Metal-only, and a
    // later `--backend cuda` request fails closed as a missing declared
    // artifact (the same seam the PTX-compile-unavailable path uses). The
    // Metal artifact is the S3-A5 proof surface.
    let cuda_artifact =
        match radix_mir_llvm::emit_cuda_device_artifact(program, &emitter_validated, interner) {
            Ok(artifact) => Some(artifact),
            Err(error) => {
                eprintln!(
                    "faber: CUDA artifact not emitted (S3-A7 emitter surface): {}",
                    error
                );
                None
            }
        };

    // The CUDA logical-entry → symbol mapping rides the artifact as
    // per-artifact metadata (N3.3): it is an artifact fact, not a program
    // semantic, so it never enters the canonical program bytes.
    let cuda_symbols = cuda_artifact
        .as_ref()
        .map(|artifact| {
            artifact
                .kernels
                .iter()
                .map(|identity| FmirDeviceSymbol {
                    entry: identity.entry.clone(),
                    symbol: identity.symbol.clone(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // The declared artifacts are versioned DeviceArtifact packets (DDCP2-U2):
    // canonical raw bytes + explicit payload encoding, the typed target id +
    // required features, the entrypoint symbol map, and the
    // `content_sha256`/`packet_sha256` identity digests (DDCP2-U3). Both
    // payloads are compiler-input text artifacts (MSL source; PTX text), so
    // `content_sha256` covers the canonical decoded bytes — never a transport
    // spelling. The FNV backend-artifact provenance is removed
    // (ddpp0-contract §FnvRemoval, B4/B5): content identity replaces it.
    let mut artifact = vec![device_artifact_with_digests(FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        format: DeviceArtifactFormat::Msl,
        stage: MaterializationStage::CompilerInput,
        target: DeviceTargetId {
            id: "macos-arm64".to_owned(),
            required_features: Vec::new(),
        },
        abi_version: DEVICE_ARTIFACT_ABI_VERSION,
        bytes: metal_artifact.source.into_bytes(),
        encoding: DevicePayloadEncoding::Text,
        entrypoints: Vec::new(),
        content_sha256: String::new(),
        packet_sha256: String::new(),
        compiler_input_packet_sha256: None,
    })];
    if let Some(cuda_artifact) = &cuda_artifact {
        match radix_mir_llvm::compile_nvvm_to_ptx(&cuda_artifact.source, ptx_target) {
            Ok(ptx) => {
                // The packaged CUDA artifact is PTX (N1.3 §3.1); its content
                // digest covers the canonical PTX bytes, not the NVVM source
                // (B5: `fnv1a64_blob_hash` → `content_sha256`).
                artifact.push(device_artifact_with_digests(FmirDeviceArtifact {
                    backend: FmirDeviceBackend::Cuda,
                    format: DeviceArtifactFormat::Ptx,
                    stage: MaterializationStage::CompilerInput,
                    target: DeviceTargetId {
                        id: "sm_120".to_owned(),
                        required_features: vec!["sm_120".to_owned()],
                    },
                    abi_version: DEVICE_ARTIFACT_ABI_VERSION,
                    bytes: ptx.into_bytes(),
                    encoding: DevicePayloadEncoding::Text,
                    entrypoints: cuda_symbols,
                    content_sha256: String::new(),
                    packet_sha256: String::new(),
                    compiler_input_packet_sha256: None,
                }));
            }
            Err(error) => {
                // Build-time PTX compile unavailable (clang NVPTX missing): the
                // image carries the Metal artifact only and a later `--backend
                // cuda` request fails closed as a missing declared artifact.
                eprintln!(
                    "faber: CUDA PTX artifact not emitted (build-time clang NVPTX unavailable): {error}"
                );
            }
        }
    }

    let wire = wire_program_for_program(program, semantics, repeating_steps);
    let declared_inputs = inputs
        .iter()
        .map(|(name, values)| FmirDeviceInput {
            name: name.clone(),
            values: values.clone(),
        })
        .collect();

    Ok(FmirDeviceSection {
        device_program: FmirDeviceProgramSection {
            v: DEVICE_RUN_PLAN_VERSION,
            program: wire,
        },
        selection: match selection {
            DeviceSelection::Auto => FmirDeviceSelection::Auto,
            DeviceSelection::Metal => FmirDeviceSelection::Metal,
            DeviceSelection::Cuda => FmirDeviceSelection::Cuda,
        },
        artifacts: FmirDeviceArtifactsSection { artifact },
        declared_inputs,
        // The device `runtime_requirements` field is gone with the FNV
        // migration (B4/B5): the packet carries the typed target id +
        // required features per artifact instead of the `device:*` string
        // allowlist, and the backend capability is the artifact's backend id.
        // MD2-W1 (FC16): the single-device constructor passes the optional
        // distributed section through as `None` — single-device packages
        // never require the multi-device section (MD-A15). Distributed-image
        // construction is a later campaign unit (MD3 bound-plan wiring); the
        // codec decode side is the shared schema types.
        distributed: None,
        // GI4-2: the ordinary producer carries no cadence/session section —
        // single-device packages do not require it (the MD-A15 precedent);
        // the session-carrying constructor surface is GI4-4's (the decode
        // driver + the bounded host session writer).
        session: None,
    })
}

// ---------------------------------------------------------------------------
// Descriptor construction + ordinary-command execution seam
// ---------------------------------------------------------------------------

/// The declared backend artifact for a resolved backend, from the image's
/// artifacts section.
pub(crate) fn artifact_for_backend(
    artifacts: &[FmirDeviceArtifact],
    backend: DeviceBackend,
) -> Option<&FmirDeviceArtifact> {
    artifacts.iter().find(|artifact| match backend {
        DeviceBackend::Metal => artifact.backend == FmirDeviceBackend::Metal,
        DeviceBackend::Cuda => artifact.backend == FmirDeviceBackend::Cuda,
    })
}

/// Build the typed host descriptor from the image's WIRE + backend artifact
/// blob (S3-A4: the host consumes the wire — the descriptor is derived
/// exclusively from the carried program facts, never from a thinned slot
/// list).
///
/// The S1-3 typed logical-entry → NVVM-symbol mapping is **consumed here**
/// from the artifact's per-artifact metadata: the CUDA descriptor's kernel
/// entry is the emitted PTX `.entry` symbol, never the logical entry; Metal
/// launches by the logical entry (the emitted MSL kernel name). Slots are
/// carried in binding order so the composite host binds buffers in the
/// emitted kernel's buffer/param order. The wire's typed lifetimes and
/// program regime are mapped onto the host descriptor's
/// [`DeviceBufferLifetime`]/[`DeviceProgramLifetime`] — the host consumes
/// the carried facts; it never re-derives a lifetime from slot role (S2-4).
///
/// # Errors
/// Fail-closed when a carried element-type spelling is outside the campaign
/// dtype surface (never a silent default), or when a result record does not
/// match a writable, observation-point resource of its producing launch.
pub(crate) fn descriptor_for_backend(
    device: &FmirDeviceSection,
    backend: DeviceBackend,
    blob: &[u8],
) -> Result<DeviceDescriptor, Vec<Diagnostic>> {
    // GI4-2: the cadence/session section is admitted fail-closed at the
    // faber codec boundary too (the version ratchet + the carried session
    // facts, the same rule set the radix decode boundary runs). Absent for
    // single-device packages (no session surface — a no-op).
    admit_session_section(device)?;
    let wire = &device.device_program.program;
    validate_wire_results(wire)?;
    let mut kernels = Vec::with_capacity(wire.kernels.len());
    let mut buffer_versions = Vec::new();
    for kernel in &wire.kernels {
        let entry = match backend {
            DeviceBackend::Cuda => device
                .artifacts
                .artifact
                .iter()
                .find(|artifact| artifact.backend == FmirDeviceBackend::Cuda)
                .and_then(|artifact| {
                    artifact
                        .entrypoints
                        .iter()
                        .find(|identity| identity.entry == kernel.entry)
                        .map(|identity| identity.symbol.clone())
                })
                .unwrap_or_else(|| kernel.entry.clone()),
            DeviceBackend::Metal => kernel.entry.clone(),
        };
        let mut buffers = Vec::with_capacity(kernel.resources.len());
        for resource in &kernel.resources {
            let element_ty = wire_element_ty_to_host(&resource.version.element_ty)?;
            add_descriptor_buffer_version(
                &mut buffer_versions,
                resource.buffer.id,
                resource.version.version,
                element_ty,
                resource.version.element_count,
            )?;
            buffers.push(DescriptorBuffer {
                buffer_id: resource.buffer.id,
                buffer_name: resource.buffer.name.clone(),
                // F1: the wire's carried stable semantic value identity —
                // the host consumes it; it never derives identity from
                // names, shapes, binding positions, or declaration order.
                semantic_value: resource.buffer.semantic_value,
                role: wire_role_to_host(resource.buffer.role),
                lifetime: wire_lifetime_to_host(resource.buffer.lifetime),
                // F5 (G4): the wire's carried initialization axis is
                // projected verbatim — the host honors it (zero-fill
                // persistent accumulation state at allocation); it never
                // re-derives initialization from role or lifetime.
                initialization: wire_initialization_to_host(resource.initialization),
                binding: resource.binding.binding,
                element_ty,
                element_count: resource.version.element_count,
                // R2: the host consumes the wire's carried content version —
                // it never re-derives or hardcodes `1` for the A10 graph.
                version: resource.version.version,
            });
        }
        kernels.push(DescriptorKernel {
            entry,
            buffers,
            grid: [
                u32::try_from(kernel.launch.workgroup_count.x).unwrap_or(u32::MAX),
                u32::try_from(kernel.launch.workgroup_count.y).unwrap_or(u32::MAX),
                u32::try_from(kernel.launch.workgroup_count.z).unwrap_or(u32::MAX),
            ],
            block: [
                kernel.launch.workgroup.x,
                kernel.launch.workgroup.y,
                kernel.launch.workgroup.z,
            ],
        });
    }
    for result in &wire.results {
        let element_ty = wire_element_ty_to_host(&result.version.element_ty)?;
        add_descriptor_buffer_version(
            &mut buffer_versions,
            result.buffer.id,
            result.version.version,
            element_ty,
            result.version.element_count,
        )?;
    }

    // S5A-U1: split the wire's DECLARED result rows by observation cadence.
    // The `PerStep` rows are the per-step readbacks (ObservationPoint
    // buffers — the loss); the `EndOfRun` rows are the one-shot end-of-run
    // readback (PerStep / PerProgram buffers — the final forward, final
    // gradients, final params). The host consumes both sets from the
    // descriptor — there is no derivation and no runtime declaration seam.
    let mut results = Vec::new();
    let mut end_of_run_results = Vec::new();
    for result in &wire.results {
        if result.observation.cadence == WireObservationCadence::EndOfRun {
            end_of_run_results.push(DescriptorEndOfRunResult {
                buffer_id: result.buffer.id,
                version: result.version.version,
            });
        } else {
            results.push(DescriptorResult {
                buffer_id: result.buffer.id,
                version: result.version.version,
                produced_by: result.produced_by,
                at_launch: result.observation.at_launch,
            });
        }
    }

    let descriptor = DeviceDescriptor {
        backend,
        module_image: blob.to_vec(),
        kernels,
        launches: wire
            .launches
            .iter()
            .map(|launch| DescriptorLaunch {
                id: launch.id,
                kernel_index: launch.kernel_index,
            })
            .collect(),
        buffer_versions,
        program_lifetime: match wire.lifetime {
            WireProgramLifetime::SingleRun => HostDeviceProgramLifetime::SingleRun,
            WireProgramLifetime::RepeatingStep(_) => HostDeviceProgramLifetime::RepeatingStep,
        },
        // R2/F3: the host consumes the WIRE'S CARRIED dependency edges
        // (real versions, producer/consumer per buffer version) verbatim —
        // the A10 graph is never re-derived from launch order or access
        // facts. The wire's `dependencies` are the materializer's frozen
        // producer/consumer facts (F3).
        data_flow: wire
            .dependencies
            .iter()
            .map(|edge| HostDescriptorDataFlow {
                buffer_id: edge.buffer,
                version: edge.version,
                producer: edge.producer,
                consumer: edge.consumer,
            })
            .collect(),
        // F3: the declared legal execution roots — the launches the graph may
        // start from, carried verbatim.
        roots: wire.roots.clone(),
        // F6 + S5A-U1: the declared observation points — the explicit result
        // rows the host reads back, projected from the wire's observation
        // facts. The `results` are the PerStep rows (read back every step);
        // the `end_of_run_results` are the EndOfRun rows (read back once
        // after the step loop).
        results,
        end_of_run_results,
    };

    descriptor
        .validate()
        .map_err(|error| vec![super::super::host_factory::host_error_diagnostic(&error)])?;
    Ok(descriptor)
}

/// Validate each wire result against the resource facts of its producing
/// launch before projecting the program into a host descriptor. The host
/// descriptor has no result surface, so a result-only or contradictory record
/// would otherwise be able to add metadata without proving a real producer.
/// Result rows are the authoritative readback set, but the host can only read
/// back buffers that match the DECLARED observation cadence (`ObservationPoint`
/// for a `PerStep` row; `PerStep`/`PerProgram` for an `EndOfRun` row) and its receipt
/// is keyed by buffer id; an unsupported lifetime or repeated id therefore
/// fails before host creation.
fn validate_wire_results(wire: &WireDeviceProgram) -> Result<(), Vec<Diagnostic>> {
    let mut result_buffers = BTreeMap::new();
    for (result_index, result) in wire.results.iter().enumerate() {
        if !matches!(result.role, WireBufferRole::Output | WireBufferRole::InOut) {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} has invalid observation role {}",
                    result.role.spelling()
                ),
            )]);
        }
        if result.role != result.buffer.role {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} has observation role {} but buffer {} is {}",
                    result.role.spelling(),
                    result.buffer.id,
                    result.buffer.role.spelling()
                ),
            )]);
        }

        let Some(launch) = wire
            .launches
            .iter()
            .find(|launch| launch.id == result.produced_by)
        else {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names unknown producing launch {}",
                    result.produced_by
                ),
            )]);
        };
        let Some(kernel) = wire.kernels.get(launch.kernel_index as usize) else {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names producing launch {}, whose kernel index {} is unknown",
                    result.produced_by, launch.kernel_index
                ),
            )]);
        };

        let Some(resource) = kernel
            .resources
            .iter()
            .find(|resource| resource.buffer.id == result.buffer.id)
        else {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names buffer {} version {} from producing launch {}, but that launch has no matching resource",
                    result.buffer.id, result.version.version, result.produced_by
                ),
            )]);
        };
        if resource.buffer != result.buffer {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} buffer {} has identity facts that contradict its producing launch {}",
                    result.buffer.id, result.produced_by
                ),
            )]);
        }

        if resource.version.version != result.version.version {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} buffer {} declares version {}, but producing launch {} uses version {}",
                    result.buffer.id,
                    result.version.version,
                    result.produced_by,
                    resource.version.version
                ),
            )]);
        }
        if resource.version.element_ty != result.version.element_ty
            || resource.version.element_count != result.version.element_count
        {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} buffer {} version {} carries shape {}[{}], but producing launch {} carries {}[{}]",
                    result.buffer.id,
                    result.version.version,
                    result.version.element_ty,
                    result.version.element_count,
                    result.produced_by,
                    resource.version.element_ty,
                    resource.version.element_count
                ),
            )]);
        }
        // F6 (Stage 3R): the result row's explicit observation fact must
        // name the producing launch's completion boundary. A result whose
        // observation contradicts its producer is a writable intermediate
        // exposed without an explicit observation fact — rejected before
        // host construction (the same rule the radix decode boundary runs).
        if result.observation.at_launch != result.produced_by {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names producing launch {}, but its explicit observation fact names launch {}; a result is a declared observation point at its producing launch",
                    result.produced_by, result.observation.at_launch
                ),
            )]);
        }
        // S5A-U1: the declared observation cadence must be consistent with
        // the buffer's lifetime class — a PerStep result names an
        // ObservationPoint buffer (read back every step); an EndOfRun result
        // names a PerStep or PerProgram buffer (read back once after the
        // loop). A cadence that contradicts the lifetime fails before host
        // construction (the same rule the radix decode boundary runs).
        match result.observation.cadence {
            WireObservationCadence::PerStep
                if resource.buffer.lifetime != WireBufferLifetime::ObservationPoint =>
            {
                return Err(vec![device_diag(
                    "result",
                    format!(
                        "result {result_index} declares a per-step observation of buffer {} with lifetime {}; only observation-point buffers are read back within every step",
                        result.buffer.id,
                        resource.buffer.lifetime.spelling()
                    ),
                )]);
            }
            WireObservationCadence::EndOfRun
                if !matches!(
                    resource.buffer.lifetime,
                    WireBufferLifetime::PerStep | WireBufferLifetime::PerProgram
                ) =>
            {
                return Err(vec![device_diag(
                    "result",
                    format!(
                        "result {result_index} declares an end-of-run observation of buffer {} with lifetime {}; only per-step and per-program buffers are read back once at the end",
                        result.buffer.id,
                        resource.buffer.lifetime.spelling()
                    ),
                )]);
            }
            _ => {}
        }
        // A result's producing launch must WRITE the observed version: at
        // least one slot of the producing kernel writes (or read-writes)
        // the buffer — the first matching slot may be a read slot of an
        // in-place update (a train_step reads AND writes its param buffers).
        if !kernel.resources.iter().any(|slot| {
            slot.buffer.id == result.buffer.id
                && matches!(
                    slot.access,
                    WireResourceAccess::Write | WireResourceAccess::ReadWrite
                )
        }) {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} names launch {} as producer, but its kernel never writes buffer {}",
                    result.produced_by, result.buffer.id
                ),
            )]);
        }
        if let Some(first_index) = result_buffers.insert(result.buffer.id, result_index) {
            return Err(vec![device_diag(
                "result",
                format!(
                    "result {result_index} repeats observation buffer {} already named by result {first_index}; result buffers must be unique in the host receipt",
                    result.buffer.id
                ),
            )]);
        }
    }
    Ok(())
}

fn add_descriptor_buffer_version(
    versions: &mut Vec<DescriptorBufferVersion>,
    buffer_id: u32,
    version: u32,
    element_ty: DeviceDataType,
    element_count: u64,
) -> Result<(), Vec<Diagnostic>> {
    if let Some(existing) = versions
        .iter()
        .find(|existing| existing.buffer_id == buffer_id && existing.version == version)
    {
        if existing.element_ty != element_ty || existing.element_count != element_count {
            return Err(vec![device_diag(
                "buffer version",
                format!("buffer {buffer_id} version {version} carries conflicting shape facts"),
            )]);
        }
        return Ok(());
    }

    versions.push(DescriptorBufferVersion {
        buffer_id,
        version,
        element_ty,
        element_count,
    });
    Ok(())
}

fn wire_role_to_host(role: WireBufferRole) -> DeviceBufferRole {
    match role {
        WireBufferRole::Input => DeviceBufferRole::Input,
        WireBufferRole::Output => DeviceBufferRole::Output,
        WireBufferRole::InOut => DeviceBufferRole::InOut,
    }
}

/// Map the wire's typed lifetime onto the host descriptor's typed lifetime.
/// The wire is the typed section (deny_unknown_fields admission), so the
/// mapping is a total function over the three-class enum (N3.4).
fn wire_lifetime_to_host(lifetime: WireBufferLifetime) -> DeviceBufferLifetime {
    match lifetime {
        WireBufferLifetime::PerProgram => DeviceBufferLifetime::PerProgram,
        WireBufferLifetime::PerStep => DeviceBufferLifetime::PerStep,
        WireBufferLifetime::ObservationPoint => DeviceBufferLifetime::ObservationPoint,
    }
}

/// Map the wire's typed initialization policy (F5) onto the host descriptor's
/// typed initialization axis. Total over the three-class enum; the host
/// honors it at allocation (zero-fill persistent state), never re-deriving it
/// from role or lifetime.
fn wire_initialization_to_host(
    initialization: WireInitializationPolicy,
) -> DeviceBufferInitialization {
    match initialization {
        WireInitializationPolicy::ZeroFill => DeviceBufferInitialization::ZeroFill,
        WireInitializationPolicy::HostProvided => DeviceBufferInitialization::HostProvided,
        WireInitializationPolicy::KernelInitialized => {
            DeviceBufferInitialization::KernelInitialized
        }
    }
}

/// Map the wire's element-type spelling onto the host's typed element type.
/// The campaign dtype surface pins f32 (the S1-1 schema); an unknown spelling
/// fails closed — never a silent default and never an unreachable arm.
fn wire_element_ty_to_host(spelling: &str) -> Result<DeviceDataType, Vec<Diagnostic>> {
    match spelling {
        "f32" => Ok(DeviceDataType::F32),
        other => Err(vec![device_diag(
            "element type",
            format!("device program element type `{other}` is outside the campaign dtype surface"),
        )]),
    }
}

/// Map the wire's named declared inputs onto buffer ids (via the wire's
/// input-buffer identities).
///
/// The map covers BOTH the program's read-only input buffers AND a
/// `RepeatingStep` program's trainable parameters — InOut buffers with
/// `HostProvided` initialization (the once-init param values). The host
/// consumes the map through [`ProgramSession::init_params`] for
/// `RepeatingStep` sessions and through per-execution copy-in for
/// `SingleRun` sessions; the extra entries are inert in either mode's copy
/// loop (the host copies only declared Input slots per execution).
pub(super) fn inputs_by_buffer_id(device: &FmirDeviceSection) -> BTreeMap<u32, Vec<f32>> {
    let mut by_name: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    for input in &device.declared_inputs {
        by_name.insert(input.name.clone(), input.values.clone());
    }
    let mut by_id: BTreeMap<u32, Vec<f32>> = BTreeMap::new();
    for kernel in &device.device_program.program.kernels {
        for resource in &kernel.resources {
            let is_input = resource.buffer.role == WireBufferRole::Input;
            let is_host_provided_param = resource.buffer.role == WireBufferRole::InOut
                && resource.initialization == WireInitializationPolicy::HostProvided;
            if (is_input || is_host_provided_param) && !by_id.contains_key(&resource.buffer.id) {
                if let Some(values) = by_name.get(&resource.buffer.name) {
                    by_id.insert(resource.buffer.id, values.clone());
                }
            }
        }
    }
    by_id
}

/// The explicit result buffer ids the run reads back (S2-4).
///
/// Result rows are the authoritative readback set. `validate_wire_results`
/// proves that each row names a unique `ObservationPoint` resource before this
/// function is used, so no valid result can disappear through a role/lifetime
/// re-derivation. Test-only since U5: the host projects readbacks from the
/// descriptor's carried observation facts, so the route no longer selects
/// outputs itself.
#[cfg(test)]
pub(crate) fn observation_buffer_ids(device: &FmirDeviceSection) -> Vec<u32> {
    let mut ids = Vec::new();
    for result in &device.device_program.program.results {
        if !ids.contains(&result.buffer.id) {
            ids.push(result.buffer.id);
        }
    }
    ids
}

// ---------------------------------------------------------------------------
// Wire-derived A10 resource graph (S3-A4)
// ---------------------------------------------------------------------------

/// One wire-derived A10 graph buffer (identity + content version).
#[cfg(test)]
pub(crate) struct WireGraphBuffer {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) version: u32,
    pub(crate) element_count: u64,
}

/// One wire-derived inter-kernel data-flow edge.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WireGraphEdge {
    pub(crate) buffer_id: u32,
    pub(crate) version: u32,
    pub(crate) producer: u32,
    pub(crate) consumer: u32,
}

/// Derive the A10 resource graph from the wire's COMPLETE facts (N3.3): the
/// per-buffer identity + content version, and the producer/consumer edges
/// from the carried ordered access + launches. This is the same derivation
/// as the radix-mir `BufferRegistry` over the program — the host consumes
/// the carried facts instead of re-deriving topology from launch order or a
/// slot-role string (no coincidence-based first-writer rule, no hardcoded
/// version). Test-only since U5: the descriptor consumes the wire's CARRIED
/// `dependencies` verbatim.
#[cfg(test)]
pub(crate) fn wire_resource_graph(
    device: &FmirDeviceSection,
) -> (Vec<WireGraphBuffer>, Vec<WireGraphEdge>) {
    let wire = &device.device_program.program;
    let mut buffers: Vec<WireGraphBuffer> = Vec::new();
    let mut producers: Vec<(u32, u32, u32)> = Vec::new();
    let mut consumers: Vec<(u32, u32, u32)> = Vec::new();
    for launch in &wire.launches {
        let Some(kernel) = wire.kernels.get(launch.kernel_index as usize) else {
            continue;
        };
        for resource in &kernel.resources {
            let id = resource.buffer.id;
            if !buffers
                .iter()
                .any(|buffer| buffer.id == id && buffer.version == resource.version.version)
            {
                buffers.push(WireGraphBuffer {
                    id,
                    name: resource.buffer.name.clone(),
                    version: resource.version.version,
                    element_count: resource.version.element_count,
                });
            }
            match resource.access {
                WireResourceAccess::Read => {
                    consumers.push((id, resource.version.version, launch.id));
                }
                WireResourceAccess::Write => {
                    producers.push((id, resource.version.version, launch.id));
                }
                WireResourceAccess::ReadWrite => {
                    consumers.push((id, resource.version.version, launch.id));
                    producers.push((id, resource.version.version, launch.id));
                }
            }
        }
    }
    // Results contribute the observed versions to the chain.
    for result in &wire.results {
        if !buffers
            .iter()
            .any(|buffer| buffer.id == result.buffer.id && buffer.version == result.version.version)
        {
            buffers.push(WireGraphBuffer {
                id: result.buffer.id,
                name: result.buffer.name.clone(),
                version: result.version.version,
                element_count: result.version.element_count,
            });
        }
    }
    // Data-flow edges (mirrors `BufferRegistry::data_flow_pairs`): every
    // producer/consumer launch pair per (buffer, version), excluding
    // self-edges.
    let mut edges: Vec<WireGraphEdge> = Vec::new();
    for (buffer_id, version, producer) in &producers {
        for (consumer_id, consumer_version, consumer) in &consumers {
            if consumer_id == buffer_id && consumer_version == version && consumer != producer {
                edges.push(WireGraphEdge {
                    buffer_id: *buffer_id,
                    version: *version,
                    producer: *producer,
                    consumer: *consumer,
                });
            }
        }
    }
    (buffers, edges)
}

/// The wire's logical name for a buffer id (diagnostics).
pub(super) fn wire_buffer_name(device: &FmirDeviceSection, id: u32) -> String {
    device
        .device_program
        .program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .find(|resource| resource.buffer.id == id)
        .map(|resource| resource.buffer.name.clone())
        .unwrap_or_else(|| "<unknown>".to_owned())
}
