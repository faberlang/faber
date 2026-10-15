use super::device::{
    EXECUTOR_PLAN_SECTION_VERSION, FmirDeviceSection, FmirExecutorPlanSection, FmirSessionSection,
    WIRE_SESSION_SECTION_VERSION,
};
use super::distributed::{
    FmirDistributedExecutionSection, expected_plan_for_section, wire_plan_to_plan,
};
use super::requirements::{
    ArtifactCompatibilityTable, default_compatibility_table, validate_artifact_combination,
    validate_compiled_in_target_in, validate_duplicate_target_features,
    validate_target_features_in,
};
use super::wire::WireDeviceProgram;
use super::{
    FmirDeviceBackend, FmirDeviceSelection, MaterializationStage, PACKAGE_MIR_ARTIFACT_VERSION,
    WIRE_DEVICE_PROGRAM_VERSION, WIRE_DISTRIBUTED_SECTION_VERSION, logical_distributed_plan_hash,
    validate_wire_distributed_section, validate_wire_program, validate_wire_session_section,
};
use crate::schema::{ARTIFACT_ABI_VERSION, ARTIFACT_REFLECTION_VERSION};

/// Fail-closed artifact-version gate. Versions ratchet as clean breaks;
/// there is no compatibility decoder.
pub(crate) fn check_version(actual: u32) -> Result<(), crate::error::FmirImageError> {
    if actual == PACKAGE_MIR_ARTIFACT_VERSION {
        Ok(())
    } else {
        Err(crate::error::FmirImageError::VersionMismatch {
            actual,
            expected: PACKAGE_MIR_ARTIFACT_VERSION,
        })
    }
}

/// Fail-closed target-tag gate: the image must declare the format it was
/// decoded as.
pub(crate) fn check_target(
    found: &str,
    expected: &'static str,
) -> Result<(), crate::error::FmirImageError> {
    if found == expected {
        Ok(())
    } else {
        Err(crate::error::FmirImageError::TargetMismatch {
            found: found.to_owned(),
            expected,
        })
    }
}

/// Fail-closed toolchain gate: the image must have been produced by the
/// expected compiler version (supplied by the caller, not baked into this
/// crate).
pub(crate) fn check_toolchain(
    found: &str,
    expected: &str,
) -> Result<(), crate::error::FmirImageError> {
    if found == expected {
        Ok(())
    } else {
        Err(crate::error::FmirImageError::ToolchainMismatch {
            found: found.to_owned(),
            expected: expected.to_owned(),
        })
    }
}

/// Fail-closed artifact digest admission (DDCP2-U3 / contract §`IdentityDomains`
/// rule 5; B3): each artifact's stored `content_sha256` is re-verified
/// against the canonical decoded raw payload bytes, and its stored
/// `packet_sha256` against the versioned artifact metadata + `content_sha256`
/// — provenance is verified, not decorative; a tampered or miscomputed
/// digest fails admission. The stage/provenance rule is enforced too: a
/// `FinalizedBinary` artifact MUST carry its hash-bound parent
/// `compiler_input_packet_sha256` (never silently replaced), a
/// `CompilerInput` artifact MUST NOT. The parent digest is also verified
/// against the section, not just spelled: it must be the `packet_sha256`
/// of the `CompilerInput` sibling the artifact names (same backend and
/// target — the finalize step may change the format by design, `msl` →
/// `metallib`), and a wrong-parent digest or a missing sibling fails
/// admission.
pub(crate) fn check_artifact_digests(
    device: &FmirDeviceSection,
) -> Result<(), crate::error::FmirImageError> {
    for artifact in &device.artifacts.artifact {
        let backend = artifact.backend.spelling();
        let computed_content = artifact.compute_content_sha256();
        if artifact.content_sha256 != computed_content {
            return Err(crate::error::FmirImageError::ArtifactDigestMismatch {
                backend,
                field: "content_sha256",
                stored: artifact.content_sha256.clone(),
                computed: computed_content,
            });
        }
        let computed_packet = artifact.compute_packet_sha256();
        if artifact.packet_sha256 != computed_packet {
            return Err(crate::error::FmirImageError::ArtifactDigestMismatch {
                backend,
                field: "packet_sha256",
                stored: artifact.packet_sha256.clone(),
                computed: computed_packet,
            });
        }
        match artifact.stage {
            MaterializationStage::FinalizedBinary => match &artifact.compiler_input_packet_sha256 {
                Some(parent) if parent.starts_with("sha256:") => {}
                _ => {
                    return Err(crate::error::FmirImageError::CompilerInputProvenance {
                        backend,
                        detail: "a finalized-binary artifact must carry its hash-bound parent compiler_input_packet_sha256 (never silently replaced)".to_owned(),
                    });
                }
            },
            MaterializationStage::CompilerInput => {
                if artifact.compiler_input_packet_sha256.is_some() {
                    return Err(crate::error::FmirImageError::CompilerInputProvenance {
                        backend,
                        detail:
                            "a compiler-input artifact must not carry a parent packet provenance"
                                .to_owned(),
                    });
                }
            }
        }
    }
    // Hash-bound parent verification (P3-1): the spelled parent digest must
    // be the packet identity of the `CompilerInput` sibling the finalized
    // binary names — same backend and target (the finalize step may change
    // the format by design: `msl` → `metallib`). A wrong-parent digest, a
    // missing sibling, or an ambiguous sibling set fails closed; the
    // `sha256:` prefix test above stays only as the cheap pre-check.
    for artifact in &device.artifacts.artifact {
        if artifact.stage != MaterializationStage::FinalizedBinary {
            continue;
        }
        // The per-artifact stage rule above already rejected a missing
        // parent digest; here the spelled value must name a sibling.
        let Some(parent) = artifact.compiler_input_packet_sha256.as_deref() else {
            continue;
        };
        let backend = artifact.backend.spelling();
        let mut named: Option<&str> = None;
        for input in device.artifacts.artifact.iter().filter(|input| {
            input.stage == MaterializationStage::CompilerInput
                && input.backend.spelling() == backend
                && input.target.id == artifact.target.id
        }) {
            match named {
                None => named = Some(input.packet_sha256.as_str()),
                Some(sibling) if sibling == input.packet_sha256 => {}
                Some(_) => {
                    return Err(crate::error::FmirImageError::CompilerInputProvenance {
                        backend,
                        detail: "the section names more than one compiler-input sibling for the finalized binary (ambiguous hash-bound parent)".to_owned(),
                    });
                }
            }
        }
        match named {
            Some(sibling) if parent == sibling => {}
            Some(_) => {
                return Err(crate::error::FmirImageError::CompilerInputProvenance {
                    backend,
                    detail: "the finalized binary's compiler_input_packet_sha256 does not match the packet_sha256 of the compiler-input sibling it names (hash-bound parent verified, not spelled)".to_owned(),
                });
            }
            None => {
                return Err(crate::error::FmirImageError::CompilerInputProvenance {
                    backend,
                    detail: "the finalized binary's compiler_input_packet_sha256 names no compiler-input sibling artifact in this section (hash-bound parent verified, not spelled)".to_owned(),
                });
            }
        }
    }
    Ok(())
}

/// Fail-closed artifact ABI-version admission (DDCP3-U0 (c) / CTO9-6 c):
/// every artifact's declared `abi_version` must be in the accepted set —
/// the default is an **exact-match clean break** against
/// [`ARTIFACT_ABI_VERSION`], which rides the coordinated boundary
/// (`HOST_CALL_ABI_VERSION == DEVICE_RUN_PLAN_VERSION`). A deliberately
/// admitted minor-compatibility lane is an explicit decision, never a
/// silent addition.
pub(crate) fn check_artifact_abi_versions(
    device: &FmirDeviceSection,
) -> Result<(), crate::error::FmirImageError> {
    for artifact in &device.artifacts.artifact {
        if artifact.abi_version != ARTIFACT_ABI_VERSION {
            return Err(crate::error::FmirImageError::ArtifactAbiVersionMismatch {
                backend: artifact.backend.spelling(),
                actual: artifact.abi_version,
                accepted: ARTIFACT_ABI_VERSION,
            });
        }
    }
    Ok(())
}

/// Fail-closed artifact reflection-version admission: a carried reflection's
/// own nested `v` must equal [`ARTIFACT_REFLECTION_VERSION`] (exact-match
/// clean break). The carrier version is checked independently of the artifact
/// ABI version and the outer image versions, because `packet_sha256` folds the
/// supplied `v` without validating it — a re-hashed packet with a foreign `v`
/// is otherwise self-consistent.
pub(crate) fn check_artifact_reflection_versions(
    device: &FmirDeviceSection,
) -> Result<(), crate::error::FmirImageError> {
    for artifact in &device.artifacts.artifact {
        if let Some(reflection) = &artifact.reflection
            && reflection.v != ARTIFACT_REFLECTION_VERSION
        {
            return Err(
                crate::error::FmirImageError::ArtifactReflectionVersionMismatch {
                    backend: artifact.backend.spelling(),
                    actual: reflection.v,
                    expected: ARTIFACT_REFLECTION_VERSION,
                },
            );
        }
    }
    Ok(())
}

/// Fail-closed typed target admission (DDCP2-U3; DDCP3-U0 (b)/(d)/(e)/(f)):
/// every required feature of every artifact must be a known feature
/// (table-driven) with **no duplicates** ([`FmirImageError::DuplicateRequiredFeature`]
/// — CTO9-6 b), and the compiled-in leaves (Metal/CUDA) keep exhaustive
/// target validation against their leaf target id + the backend/format/stage/
/// feature combination rules (the assembled compatibility table — CTO9-6
/// d/e). Unknown features, incompatible target ids, and incompatible
/// combinations fail closed with the named rule — never guessed, never
/// silently dropped. Unknown-backend artifacts are validated as far as their
/// declared fields allow (CTO9-6 f preserve-until-selection): duplicates are
/// structurally invalid regardless of backend, but the feature ALLOWLIST and
/// the leaf-specific combination rules are leaf knowledge and do not apply —
/// the artifact is admitted at load and rejected at selection use instead.
pub(crate) fn check_artifact_targets(
    device: &FmirDeviceSection,
    table: &ArtifactCompatibilityTable,
) -> Result<(), crate::error::FmirImageError> {
    for artifact in &device.artifacts.artifact {
        if let FmirDeviceBackend::Unknown(_) = &artifact.backend {
            validate_duplicate_target_features(&artifact.target)?;
        } else {
            validate_target_features_in(&artifact.target, table)?;
            validate_compiled_in_target_in(&artifact.backend, &artifact.target, table)?;
            validate_artifact_combination(
                &artifact.backend,
                &artifact.format,
                artifact.stage,
                &artifact.target,
                table,
            )?;
        }
    }
    Ok(())
}

/// Fail-closed entrypoint-identity admission (DDCP3-U0 (b) / CTO9-6 b): the
/// entrypoint symbol map is canonical-ordered at hash time, so a repeated
/// entrypoint identity has no well-defined set identity — duplicates are
/// rejected at admission with the named rule.
pub(crate) fn check_artifact_entrypoints(
    device: &FmirDeviceSection,
) -> Result<(), crate::error::FmirImageError> {
    for artifact in &device.artifacts.artifact {
        for (index, entrypoint) in artifact.entrypoints.iter().enumerate() {
            if artifact.entrypoints[..index]
                .iter()
                .any(|seen| seen.entry == entrypoint.entry)
            {
                return Err(crate::error::FmirImageError::DuplicateEntrypoint {
                    entry: entrypoint.entry.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Fail-closed backend-selection admission (DDCP2-U3; DDCP3-U0 (f) /
/// CTO9-6 f): an explicit selection (Metal/Cuda/Amd) must have an artifact
/// for the selected backend — a missing artifact fails closed with the
/// named rule. `Auto` is admitted (selection resolves at run time); an
/// **explicit selection of an unknown backend fails at selection
/// resolution** with [`FmirImageError::NonAdmittedSelectedBackend`] —
/// aligned with faber's landed fail-closed `Unknown(_)` arm. Unknown
/// backend ARTIFACTS are preserved-until-selection (admitted at load,
/// rejected at selection use); never silently remapped.
pub(crate) fn check_selection_artifacts(
    device: &FmirDeviceSection,
) -> Result<(), crate::error::FmirImageError> {
    match &device.selection {
        FmirDeviceSelection::Metal | FmirDeviceSelection::Cuda | FmirDeviceSelection::Amd => {
            let selected = device.selection.spelling();
            let present = device
                .artifacts
                .artifact
                .iter()
                .any(|artifact| artifact.backend.spelling() == selected);
            if !present {
                return Err(crate::error::FmirImageError::AbsentArtifactForSelection {
                    selection: selected,
                });
            }
        }
        FmirDeviceSelection::Unknown(backend) => {
            return Err(crate::error::FmirImageError::NonAdmittedSelectedBackend {
                backend: backend.clone(),
            });
        }
        FmirDeviceSelection::Auto => {}
    }
    Ok(())
}

/// Fail-closed payload-encoding admission (contract §Payload encoding):
/// every artifact's explicit encoding declaration must be consistent with
/// its canonical raw bytes — text payloads canonical UTF-8 (valid UTF-8, no
/// BOM, no alternative-encoding disguise); binary payloads the declared
/// `binary:<encoding-tag>:<decoded-byte-length>:<canonical-unpadded-base64>`
/// spelling. No binary is smuggled through UTF-8 or base64 without this
/// schema-level declaration.
pub(crate) fn check_artifact_encodings(
    device: &FmirDeviceSection,
) -> Result<(), crate::error::FmirImageError> {
    for artifact in &device.artifacts.artifact {
        artifact.validate_encoding().map_err(|error| {
            crate::error::FmirImageError::ArtifactEncodingMismatch {
                backend: artifact.backend.spelling(),
                detail: error.to_string(),
            }
        })?;
    }
    Ok(())
}

/// Combined device-section admission with a caller-supplied compatibility
/// table (DDCP3-U0 (e) / CTO9-6 e): the table the caller assembles from the
/// compiled-in leaves' rows (`radix-mir-metal::compat::metal_compatibility_rows`
/// / `radix-mir-llvm::compat::cuda_compatibility_rows`) replaces the FMIR
/// default table for the target/combination validation. Run after the
/// version / target / toolchain gates (a device payload is admitted only from
/// the exact producing toolchain version — N1.7).
///
/// Order (Stage 3R F7/S8; DDCP3-U0 revision): the **nested** device-program
/// wire version is admitted independently of the outer image version and
/// before any field interpretation; then the carried F1–F7 semantic facts are
/// validated fail-closed; then the payload encodings, the artifact ABI
/// versions (c), the typed targets + combinations (b/d/e), the entrypoint
/// identity uniqueness (b), the digest re-verification over the **canonical
/// packet identity** (b — a stored packet hashed over a non-canonical order
/// fails here), and the backend selection (f).
///
/// # Errors
///
/// Returns [`FmirImageError::DeviceProgramVersionMismatch`] when the
/// nested device-program wire version is unsupported, and the named
/// [`FmirImageError`] variant for the first fail-closed rule the section
/// violates (payload encodings, artifact ABI versions, typed targets +
/// combinations, entrypoint uniqueness, digest re-verification, backend
/// selection, and the optional distributed/session sections).
fn admit_executor_plan_section(
    section: &FmirExecutorPlanSection,
    kernel_count: usize,
) -> Result<(), crate::error::FmirImageError> {
    if section.v != EXECUTOR_PLAN_SECTION_VERSION {
        return Err(
            crate::error::FmirImageError::ExecutorPlanSectionVersionMismatch {
                actual: section.v,
                expected: EXECUTOR_PLAN_SECTION_VERSION,
            },
        );
    }
    if section.plans.len() != kernel_count {
        return Err(crate::error::FmirImageError::ExecutorPlanSectionInvalid {
            detail: format!(
                "selection count {} does not match kernel count {}",
                section.plans.len(),
                kernel_count
            ),
        });
    }
    admit_executor_plan_entries(section)?;
    Ok(())
}

fn admit_executor_plan_entries(
    section: &FmirExecutorPlanSection,
) -> Result<(), crate::error::FmirImageError> {
    for (position, plan) in section.plans.iter().enumerate() {
        if usize::try_from(plan.kernel_index).ok() != Some(position) {
            return Err(crate::error::FmirImageError::ExecutorPlanSectionInvalid {
                detail: format!(
                    "selection index {} is not the ordered kernel index {}",
                    plan.kernel_index, position
                ),
            });
        }
        if plan.logical_signature.operation.is_empty() || plan.provenance.pass.is_empty() {
            return Err(crate::error::FmirImageError::ExecutorPlanSectionInvalid {
                detail: format!("selection {position} has incomplete signature/provenance"),
            });
        }
        if plan.primary_bind.dims.is_empty()
            || plan.primary_bind.dims.len() != plan.primary_bind.strides.len()
            || plan.primary_bind.dims.contains(&0)
            || plan.primary_bind.strides.contains(&0)
            || plan.primary_bind.grid.contains(&0)
            || plan.primary_bind.layout.is_empty()
        {
            return Err(crate::error::FmirImageError::ExecutorPlanSectionInvalid {
                detail: format!("selection {position} has an invalid primary bind"),
            });
        }
        if plan
            .logical_signature
            .inputs
            .iter()
            .chain(&plan.logical_signature.outputs)
            .any(String::is_empty)
            || plan.provenance.source_nodes.is_empty()
        {
            return Err(crate::error::FmirImageError::ExecutorPlanSectionInvalid {
                detail: format!("selection {position} has empty logical names or provenance"),
            });
        }
        for bind in &plan.binds {
            if bind.name.is_empty()
                || bind.dims.is_empty()
                || bind.dims.len() != bind.strides.len()
                || bind.dims.contains(&0)
                || bind.strides.contains(&0)
            {
                return Err(crate::error::FmirImageError::ExecutorPlanSectionInvalid {
                    detail: format!("selection {position} has an invalid bind for {}", bind.name),
                });
            }
        }
        for uniform in &plan.uniforms {
            if uniform.name.is_empty()
                || !matches!(uniform.ty.as_str(), "F32" | "F16" | "U32" | "I32")
            {
                return Err(crate::error::FmirImageError::ExecutorPlanSectionInvalid {
                    detail: format!("selection {position} has an unknown uniform type"),
                });
            }
        }
        for requirement in &plan.backend_requirements {
            if requirement.backend.is_empty() || requirement.feature.is_empty() {
                return Err(crate::error::FmirImageError::ExecutorPlanSectionInvalid {
                    detail: format!("selection {position} has an incomplete backend requirement"),
                });
            }
        }
        for offset in &plan.offsets {
            if offset.name.is_empty() || offset.strides.is_empty() || offset.strides.contains(&0) {
                return Err(crate::error::FmirImageError::ExecutorPlanSectionInvalid {
                    detail: format!(
                        "selection {position} has an invalid offset/stride row for {}",
                        offset.name
                    ),
                });
            }
        }
        if let Some(workspace) = plan.workspace
            && (workspace.alignment == 0 || workspace.bytes % u64::from(workspace.alignment) != 0)
        {
            return Err(crate::error::FmirImageError::ExecutorPlanSectionInvalid {
                detail: format!("selection {position} has an invalid workspace alignment"),
            });
        }
    }
    Ok(())
}

/// Admits the device section against the artifact compatibility table.
///
/// # Errors
/// Returns `FmirImageError` when the nested device-program version mismatches,
/// the program fails wire validation, the executor-plan section is invalid, or
/// an artifact's ABI version or nested reflection version is not the accepted
/// one.
pub fn admit_device_section_with_compat(
    device: &FmirDeviceSection,
    table: &ArtifactCompatibilityTable,
) -> Result<(), crate::error::FmirImageError> {
    // S8: the nested device-program version fails closed independently of the
    // outer FMIR version, with a focused diagnostic.
    if device.device_program.v != WIRE_DEVICE_PROGRAM_VERSION {
        return Err(crate::error::FmirImageError::DeviceProgramVersionMismatch {
            actual: device.device_program.v,
            expected: WIRE_DEVICE_PROGRAM_VERSION,
        });
    }
    validate_wire_program(&device.device_program.program)?;
    if let Some(executor_plan) = &device.executor_plan {
        admit_executor_plan_section(executor_plan, device.device_program.program.kernels.len())?;
    }
    check_artifact_encodings(device)?;
    check_artifact_abi_versions(device)?;
    check_artifact_reflection_versions(device)?;
    check_artifact_targets(device, table)?;
    check_artifact_entrypoints(device)?;
    check_artifact_digests(device)?;
    check_selection_artifacts(device)?;
    // MD2-W1: the optional distributed execution section is admitted
    // fail-closed at load — its own version ratchet, wire-level section
    // validation, MD2-V1's gate on the reconstructed plan, the equivalence
    // gate, and the H1 hash cross-check. Absent for single-device packages.
    if let Some(distributed) = &device.distributed {
        admit_distributed_section(distributed, &device.device_program.program)?;
    }
    // GI4-2: the optional cadence/session section is admitted fail-closed at
    // load — its own version ratchet (the MD2-W1 sibling-field precedent; no
    // `WIRE_DEVICE_PROGRAM_VERSION` bump) + the wire-level session-facts
    // validation. Absent for packages without an inference-session surface
    // (single-device packages do not require the section — MD-A15).
    if let Some(session) = &device.session {
        admit_session_section(session, &device.device_program.program)?;
    }
    Ok(())
}

/// Combined device-section admission with the FMIR **default** compatibility
/// table (the current Metal/CUDA facts as data). The mid-flight faber
/// consumer's decode path (`decode_text_image` / `decode_binary_image`)
/// admits through here; a caller that assembles the table from the compiled-
/// in leaves uses [`admit_device_section_with_compat`]. The FMIR default
/// table's removal + faber caller-supplied assembly is a recorded
/// same-boundary contract item for DDPP1-U5/U6 + DDCP4 (NOT removed here).
pub(crate) fn admit_device_section(
    device: &FmirDeviceSection,
) -> Result<(), crate::error::FmirImageError> {
    admit_device_section_with_compat(device, &default_compatibility_table())
}

/// Fail-closed admission of the optional cadence/session section (GI4-2),
/// run when the section is present, in order:
///
/// 1. **version ratchet** (A7) — [`WIRE_SESSION_SECTION_VERSION`] is checked
///    before any field interpretation (the `admit_device_section` pattern,
///    FC4), independently of the nested device-program wire version and the
///    outer image version;
/// 2. **wire-level session-facts validation** — the carried session facts
///    are admitted fail-closed against the carried single-device program
///    ([`validate_wire_session_section`] — the equivalence-gate discipline:
///    admission inspects the actually-delivered session facts, never a
///    self-declared hash).
pub(crate) fn admit_session_section(
    section: &FmirSessionSection,
    wire_program: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    // A7: the section's own version ratchet fails closed BEFORE any field
    // interpretation — independently of the nested device-program wire
    // version and the outer image version.
    if section.v != WIRE_SESSION_SECTION_VERSION {
        return Err(
            crate::error::FmirImageError::SessionSectionVersionMismatch {
                actual: section.v,
                expected: WIRE_SESSION_SECTION_VERSION,
            },
        );
    }
    validate_wire_session_section(section, wire_program)?;
    Ok(())
}

/// Fail-closed admission of the optional distributed execution section
/// (MD2-W1). Runs when the section is present, in order:
///
/// 1. **version ratchet** (A7) — `WIRE_DISTRIBUTED_SECTION_VERSION` is
///    checked before any field interpretation (the `admit_device_section`
///    pattern, FC4);
/// 2. **wire-level section validation** — the declared placement facts
///    resolve against the carried single-device program; the declared hash
///    is the canonical spelling;
/// 3. **MD2-V1's gate** — the reconstructed delivered plan passes the
///    fail-before-launch checks (stale versions, topology, owners, replicas,
///    completeness, cycles, admissible collectives);
/// 4. **equivalence gate** — re-derive `(program, declared placement)` and
///    compare canonical bytes with the actually delivered plan (stale /
///    tampered fails with a focused diagnostic);
/// 5. **H1 hash cross-check** — the section's declared
///    `logical_distributed_plan_hash` equals the hash recomputed over the
///    delivered plan.
pub(crate) fn admit_distributed_section(
    section: &FmirDistributedExecutionSection,
    wire_program: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    // A7: the section's own version ratchet fails closed BEFORE any field
    // interpretation — independently of the nested device-program wire
    // version and the outer image version.
    if section.v != WIRE_DISTRIBUTED_SECTION_VERSION {
        return Err(
            crate::error::FmirImageError::DistributedSectionVersionMismatch {
                actual: section.v,
                expected: WIRE_DISTRIBUTED_SECTION_VERSION,
            },
        );
    }
    validate_wire_distributed_section(section, wire_program)?;
    let delivered = wire_plan_to_plan(&section.logical_plan)?;
    delivered.validate().map_err(|error| {
        crate::error::FmirImageError::DistributedSectionInvalid {
            detail: format!("delivered logical plan fails the fail-before-launch gate: {error}"),
        }
    })?;
    equivalence_gate(section, wire_program)?;
    // MD2-H1 cross-check: the declared hash must equal the hash recomputed
    // over the delivered plan (reconstructed from the wire — the section
    // mirrors D1's classes, calls V1's gate, and cross-checks H1's hash).
    let recomputed = logical_distributed_plan_hash(&delivered);
    if recomputed != section.logical_distributed_plan_hash {
        return Err(
            crate::error::FmirImageError::DistributedSectionHashMismatch {
                expected: section.logical_distributed_plan_hash.clone(),
                recomputed,
            },
        );
    }
    Ok(())
}

/// The equivalence gate (MD2-W1, exit-gate bullet 4): re-derive
/// `(program, declared placement)` from the carried single-device program and
/// the declared placement facts and compare **canonical bytes** with the
/// **actually delivered plan** (reconstructed from the section). A mutated
/// delivered plan (partition owner, graph edge, transfer byte contract) or a
/// mutated declared placement fails with a focused stale/tampered diagnostic.
/// The gate inspects the delivered bytes — it never trusts the section's
/// self-declared `logical_distributed_plan_hash`.
pub(crate) fn equivalence_gate(
    section: &FmirDistributedExecutionSection,
    wire_program: &WireDeviceProgram,
) -> Result<(), crate::error::FmirImageError> {
    let delivered = wire_plan_to_plan(&section.logical_plan)?;
    let expected = expected_plan_for_section(section, wire_program)?;
    let delivered_bytes = radix_mir::distributed::encode_logical_plan(&delivered);
    let expected_bytes = radix_mir::distributed::encode_logical_plan(&expected);
    if delivered_bytes != expected_bytes {
        return Err(
            crate::error::FmirImageError::DistributedSectionEquivalenceMismatch {
                detail: format!(
                    "the delivered logical plan ({} partitions, {} operations) is not the deterministic derivation of the declared program + placement — stale or tampered; admission re-derives (program, declared placement) and compares canonical bytes with the delivered plan",
                    delivered.partitions.len(),
                    delivered.graph.operations.len(),
                ),
            },
        );
    }
    Ok(())
}
