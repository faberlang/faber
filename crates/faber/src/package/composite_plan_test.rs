//! EXEC-01-M2 mutation probes: identity-preservation verification and typed
//! first-divergence diagnostics over hand-built typed fixtures.
//!
//! Every probe assembles the plan from typed facts and verifies it against a
//! pristine [`IdentitySource`]; the mutation in one identity domain must
//! reject construction with a typed diagnostic naming the first failing
//! domain. No silent repair, no FNV fallback (DDPP0 §`IdentityDomains` rule 1).

use super::*;
use crate::package::PackageSpec;
use radix::module::codegen::Target;
use radix::module::diagnostics::{Diagnostic, DiagnosticArg};
use radix_mir::abi::{
    MirKernelDispatchSize, MirKernelResourceAccess, MirKernelWorkgroupCount, MirWorkgroupSize,
};
use radix_mir::device_program::{
    Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram,
    DeviceProgramLifetime, DeviceResource, KernelLaunchPlan, KernelUnit, LaunchId, LaunchUnit,
    ObservationCadence, ResultBuffer,
};
use radix_mir::device_semantics::DeviceSemantics;
use radix_mir::kernel_plan::CollectionKernelPlan;
use radix_mir::layout::MirTensorStorageLayout;
use radix_mir::nodes::{MirFunctionId, MirType};
use radix_mir_fmir::{
    DeviceArtifactFormat, DevicePayloadEncoding, DeviceTargetId, FmirDeviceArtifact,
    FmirDeviceBackend, MaterializationStage,
};
use radix_types::TypeId;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// An empty analyzed package with no source units (the A10 source half is
/// empty; package identity falls back to the root name).
fn empty_package(label: &str) -> AnalyzedProgram {
    AnalyzedProgram::from_spec(PackageSpec {
        package_root: PathBuf::from(label),
        source_root: PathBuf::from(label).join("src"),
        entry: PathBuf::from(label).join("src/main.fab"),
        templates: BTreeMap::new(),
        manifest_backed: false,
    })
}

fn weights_identity() -> BufferIdentity {
    BufferIdentity {
        id: BufferId(1),
        name: "weights".to_owned(),
        role: BufferRole::Input,
        storage: MirTensorStorageLayout::DeviceHandle,
        lifetime: BufferLifetime::PerProgram,
    }
}

fn scores_identity() -> BufferIdentity {
    BufferIdentity {
        id: BufferId(2),
        name: "scores".to_owned(),
        role: BufferRole::Output,
        storage: MirTensorStorageLayout::DeviceHandle,
        lifetime: BufferLifetime::ObservationPoint,
    }
}

fn weights_version() -> BufferVersion {
    BufferVersion {
        version: 1,
        element_ty: MirType::semantic(TypeId(0)),
        element_count: 4,
        reduced_projection: None,
    }
}

fn scores_version() -> BufferVersion {
    BufferVersion {
        version: 1,
        element_ty: MirType::semantic(TypeId(0)),
        element_count: 2,
        reduced_projection: None,
    }
}

fn fixture_program() -> DeviceProgram {
    let kernel = KernelUnit {
        function: MirFunctionId(7),
        entry: "matmul".to_owned(),
        library_operation: radix_mir::device_plan_compiler::LibraryOperation::Generated,
        plan: CollectionKernelPlan::Elementwise,
        resources: vec![
            DeviceResource {
                buffer: weights_identity(),
                version: weights_version(),
                binding: Binding {
                    group: 0,
                    binding: 0,
                },
                access: MirKernelResourceAccess::Read,
            },
            DeviceResource {
                buffer: scores_identity(),
                version: scores_version(),
                binding: Binding {
                    group: 0,
                    binding: 1,
                },
                access: MirKernelResourceAccess::Write,
            },
        ],
        launch: KernelLaunchPlan {
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
            workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
        },
    };
    DeviceProgram {
        kernels: vec![kernel],
        launches: vec![LaunchUnit {
            id: LaunchId(1),
            kernel_index: 0,
        }],
        lifetime: DeviceProgramLifetime::SingleRun,
        results: vec![ResultBuffer {
            buffer: scores_identity(),
            version: scores_version(),
            role: BufferRole::Output,
            produced_by: LaunchId(1),
            cadence: ObservationCadence::PerStep,
        }],
    }
}

fn fixture_model() -> ModelIdentity {
    ModelIdentity {
        content: ContentIdentity {
            algorithm: "sha-256".to_owned(),
            digestio: format!("sha256:{}", "a".repeat(64)),
            longitudo: 22_663_387_424,
        },
        architecture: "qwen35moe".to_owned(),
        tokenizer_identity: "qwen2-tokenizer".to_owned(),
        admission_facts: BTreeMap::new(),
        tensor_count: 3,
    }
}

fn fixture_backend_selection() -> BackendSelection {
    BackendSelection {
        backend: BackendLeaf::Metal,
        target: DeviceTargetId {
            id: "macos-arm64".to_owned(),
            required_features: Vec::new(),
        },
        required_capacity: None,
    }
}

fn fixture_artifact() -> FmirDeviceArtifact {
    let mut artifact = FmirDeviceArtifact {
        backend: FmirDeviceBackend::Metal,
        format: DeviceArtifactFormat::Msl,
        stage: MaterializationStage::CompilerInput,
        target: DeviceTargetId {
            id: "macos-arm64".to_owned(),
            required_features: Vec::new(),
        },
        abi_version: 8,
        bytes: b"kernel void matmul".to_vec(),
        encoding: DevicePayloadEncoding::Text,
        entrypoints: Vec::new(),
        content_sha256: String::new(),
        packet_sha256: String::new(),
        compiler_input_packet_sha256: None,
        reflection: None,
    };
    artifact.content_sha256 = artifact.compute_content_sha256();
    artifact.packet_sha256 = artifact.compute_packet_sha256();
    artifact
}

fn fixture_revisions() -> SourceRevisions {
    SourceRevisions {
        radix: Some("557e166fe".to_owned()),
        gradus: Some("deadbeef".to_owned()),
        faber: Some("12345678".to_owned()),
        hosts: Some("abcdef12".to_owned()),
        dirty: false,
    }
}

fn fixture_facts() -> CompositePlanFacts {
    CompositePlanFacts {
        model_identity: fixture_model(),
        backend_selection: fixture_backend_selection(),
        device_program: fixture_program(),
        semantics: DeviceSemantics::default(),
        device_artifacts: vec![fixture_artifact()],
        host_call_facts: vec![HostCallFact {
            callsite: "call_0".to_owned(),
            prepared_region: 0,
        }],
        submission_region_facts: vec![SubmissionRegionFact {
            region: 0,
            regime: "single-run".to_owned(),
            shape_class: "one-kernel".to_owned(),
        }],
        effect_capability_requirements: EffectCapabilityRequirements {
            effects: vec!["ad".to_owned()],
            capabilities: Vec::new(),
        },
        source_revisions: fixture_revisions(),
    }
}

fn fixture_identity_source() -> IdentitySource {
    IdentitySource {
        model: fixture_model(),
        backend: fixture_backend_selection(),
        artifacts: vec![fixture_artifact()],
        kernels: vec![KernelIdentity {
            function: 7,
            entry: "matmul".to_owned(),
        }],
        regions: vec![0],
        buffers: vec![
            ExpectedBuffer {
                identity: weights_identity(),
                versions: vec![weights_version()],
            },
            ExpectedBuffer {
                identity: scores_identity(),
                versions: vec![scores_version()],
            },
        ],
        source_identity: ExpectedSource {
            source_hashes: Vec::new(),
            revisions: fixture_revisions(),
        },
    }
}

/// Assert the rejection names the given first-failing identity domain with
/// the typed divergence issue.
fn assert_first_domain(diagnostics: &[Diagnostic], domain: &str) {
    assert!(
        diagnostics.iter().any(|diag| {
            diag.args.contains(&DiagnosticArg::new(
                "issue",
                "composite_plan_identity_divergence",
            )) && diag.args.contains(&DiagnosticArg::new("domain", domain))
        }),
        "expected first-failing domain `{domain}`, got: {diagnostics:?}"
    );
}

#[test]
fn pristine_plan_preserves_all_five_identity_domains() {
    let package = empty_package("exec-01-m2-pristine");
    let facts = fixture_facts();
    let source = fixture_identity_source();
    let plan = assemble_composite_plan(&package, Target::MirFmir, facts, &source)
        .expect("a plan assembled from its typed sources verifies");
    assert_eq!(plan.model_identity, source.model);
    assert_eq!(plan.backend_selection, source.backend);
    assert_eq!(plan.device_artifacts, source.artifacts);
    assert_eq!(plan.source_revisions, source.source_identity.revisions);
    assert_eq!(plan.prepared_regions.len(), 1);
    assert_eq!(plan.prepared_regions[0].id.0, 0);
    // Verification is stable on the assembled plan.
    verify_identity_preservation(&plan, &source).expect("re-verification passes");
}

#[test]
fn mutated_model_digest_rejects_with_model_domain() {
    let package = empty_package("exec-01-m2-model-digest");
    let source = fixture_identity_source();
    let mut facts = fixture_facts();
    facts.model_identity.content.digestio = format!("sha256:{}", "b".repeat(64));
    let error = assemble_composite_plan(&package, Target::MirFmir, facts, &source).unwrap_err();
    assert_first_domain(&error, "model");
}

#[test]
fn swapped_tensor_descriptor_rejects_with_buffer_domain() {
    let package = empty_package("exec-01-m2-swap-tensors");
    let source = fixture_identity_source();
    let mut facts = fixture_facts();
    // Swap the two tensor descriptors (buffer content versions) at the
    // kernel slots: buffer 1 now binds the scores version and buffer 2 the
    // weights version.
    let resources = &mut facts.device_program.kernels[0].resources;
    let weights_version = resources[0].version.clone();
    resources[0].version = resources[1].version.clone();
    resources[1].version = weights_version;
    let error = assemble_composite_plan(&package, Target::MirFmir, facts, &source).unwrap_err();
    assert_first_domain(&error, "buffer");
}

#[test]
fn missing_backend_artifact_rejects_with_backend_domain() {
    let package = empty_package("exec-01-m2-missing-artifact");
    let source = fixture_identity_source();
    let mut facts = fixture_facts();
    facts.device_artifacts = Vec::new();
    let error = assemble_composite_plan(&package, Target::MirFmir, facts, &source).unwrap_err();
    assert_first_domain(&error, "backend");
}

#[test]
fn stale_source_revision_rejects_with_source_domain() {
    let package = empty_package("exec-01-m2-stale-revision");
    let source = fixture_identity_source();
    let mut facts = fixture_facts();
    facts.source_revisions.radix = Some("stale0000".to_owned());
    let error = assemble_composite_plan(&package, Target::MirFmir, facts, &source).unwrap_err();
    assert_first_domain(&error, "source");
}

#[test]
fn altered_buffer_version_rejects_with_buffer_domain() {
    let package = empty_package("exec-01-m2-buffer-version");
    let source = fixture_identity_source();
    let mut facts = fixture_facts();
    facts.device_program.kernels[0].resources[0].version.version = 2;
    let error = assemble_composite_plan(&package, Target::MirFmir, facts, &source).unwrap_err();
    assert_first_domain(&error, "buffer");
}

#[test]
fn renamed_kernel_entry_rejects_with_kernel_domain() {
    let package = empty_package("exec-01-m2-kernel-entry");
    let source = fixture_identity_source();
    let mut facts = fixture_facts();
    facts.device_program.kernels[0].entry = "renamed".to_owned();
    let error = assemble_composite_plan(&package, Target::MirFmir, facts, &source).unwrap_err();
    assert_first_domain(&error, "kernel");
}

#[test]
fn mutated_artifact_digest_is_rejected_not_repaired() {
    // A mutated `content_sha256` fails closed through digest re-verification
    // over the carried bytes (DDPP0 §IdentityDomains rule 1) — the plan is
    // rejected, never silently rewritten to the recomputed digest.
    let package = empty_package("exec-01-m2-artifact-digest");
    let source = fixture_identity_source();
    let mut facts = fixture_facts();
    facts.device_artifacts[0].content_sha256 = format!("sha256:{}", "c".repeat(64));
    let error = assemble_composite_plan(&package, Target::MirFmir, facts, &source).unwrap_err();
    assert_first_domain(&error, "backend");
    let detail = error
        .iter()
        .find(|diag| {
            diag.args.contains(&DiagnosticArg::new(
                "issue",
                "composite_plan_identity_divergence",
            ))
        })
        .expect("typed divergence diagnostic present");
    assert!(
        detail
            .args
            .iter()
            .any(|arg| arg.name == "detail" && arg.value.contains("content_sha256"))
    );
}

#[test]
fn typed_error_names_the_first_failing_domain() {
    let package = empty_package("exec-01-m2-typed-error");
    let source = fixture_identity_source();
    let plan = assemble_composite_plan(&package, Target::MirFmir, fixture_facts(), &source)
        .expect("pristine plan assembles");
    let mut divergent = plan.clone();
    divergent.model_identity.content.longitudo = -1;
    let error = verify_identity_preservation(&divergent, &source).unwrap_err();
    assert_eq!(error.domain(), IdentityDomain::Model);
    let diagnostic = error.to_diagnostic();
    assert!(diagnostic.args.contains(&DiagnosticArg::new(
        "issue",
        "composite_plan_identity_divergence"
    )));
    assert!(
        diagnostic
            .args
            .contains(&DiagnosticArg::new("domain", "model"))
    );
}
