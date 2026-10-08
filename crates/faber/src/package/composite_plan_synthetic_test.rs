//! EXEC-01-M3: executed mechanism proof over a bounded synthetic
//! public-Gradus graph fixture.
//!
//! MECHANISM PREREQUISITE — evidence tier T1 fixture, NOT campaign
//! evidence. The fixture (`crates/faber/test-fixtures/composite-plan-synthetic`)
//! is a small `.fab` package importing `gradus:model/artifact` /
//! `gradus:model/gguf_manifest` and declaring its own synthetic
//! `ContentIdentity`. This suite executes the M1/M2 composite-plan
//! construction + identity-preservation machinery over that package-shaped
//! fixture, prints the five identity domains with their preserved values,
//! and proves each mutation probe fails closed with a typed first-divergence
//! diagnostic. It makes no campaign-evidence claim and does not substitute
//! for M4 — the first real-graph campaign evidence, gated on MODEL-04 and
//! the GGUF-M6/M6 capstone package manifest (EXEC-01 challenge C2).

use super::composite_plan::{
    BackendLeaf, BackendSelection, CompositePlanFacts, ContentIdentity,
    EffectCapabilityRequirements, ExpectedBuffer, ExpectedSource, HostCallFact, IdentitySource,
    KernelIdentity, ModelIdentity, SourceRevisions, SubmissionRegionFact, assemble_composite_plan,
    verify_identity_preservation,
};
use super::test_support::{TestDir, test_temp_dir};
use radix::module::codegen::Target;
use radix::module::diagnostics::{Diagnostic, DiagnosticArg};
use radix::module::driver::Config;
use radix::program::analyze::AnalyzedProgram;
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
use std::path::{Path, PathBuf};

fn analyze_with_faber_seams(
    config: &Config,
    input: &Path,
) -> Result<radix::program::analyze::AnalyzedProgram, Vec<Diagnostic>> {
    radix::program::analyze::analyze_package(config, input, &crate::package::faber_analysis_seams())
}

/// The checked-in synthetic fixture package (analyzed via a tempdir copy so
/// the checked-in tree is never written).
fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/composite-plan-synthetic")
}

fn copy_fixture() -> TestDir {
    let dir = test_temp_dir("composite-plan-synthetic");
    copy_package_dir_skipping_target(&fixture_root(), dir.path());
    dir
}

fn copy_package_dir_skipping_target(src: &Path, dest: &Path) {
    for entry in std::fs::read_dir(src).expect("read fixture dir") {
        let entry = entry.expect("read dir entry");
        let dest_path = dest.join(entry.file_name());
        if entry.path().is_dir() {
            if entry.file_name() == "target" {
                continue;
            }
            std::fs::create_dir_all(&dest_path).expect("create sub dir");
            copy_package_dir_skipping_target(&entry.path(), &dest_path);
        } else {
            std::fs::copy(entry.path(), dest_path).expect("copy file");
        }
    }
}

/// The imported Gradus HIR is walked recursively by the def-id closure check.
/// A default Rust test thread's stack is too small for this finite fixture
/// graph, so keep the synthetic proof on an explicit bounded stack.
const SYNTHETIC_ANALYSIS_STACK_SIZE: usize = 4 * 1024 * 1024;

/// Analyze a fresh copy of the synthetic fixture package.
fn analyze_fixture() -> (TestDir, AnalyzedProgram) {
    std::thread::Builder::new()
        .name("composite-plan-synthetic".to_owned())
        .stack_size(SYNTHETIC_ANALYSIS_STACK_SIZE)
        .spawn(|| {
            let dir = copy_fixture();
            let config = Config::default();
            let package = analyze_with_faber_seams(&config, dir.path())
                .expect("synthetic fixture package analyzes");
            (dir, package)
        })
        .expect("spawn synthetic analysis thread")
        .join()
        .expect("synthetic analysis thread must not panic")
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
        shape: None,
        reduced_projection: None,
    }
}

fn scores_version() -> BufferVersion {
    BufferVersion {
        version: 1,
        element_ty: MirType::semantic(TypeId(0)),
        element_count: 2,
        shape: None,
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

/// The synthetic Gradus model identity: exactly the values the fixture's
/// `identitas_synthetica` declares, carried as data.
fn fixture_model() -> ModelIdentity {
    ModelIdentity {
        content: ContentIdentity {
            algorithm: "sha-256".to_owned(),
            digestio: format!("sha256:{}", "a".repeat(64)),
            longitudo: 123_456_789,
        },
        architecture: "syntheticum".to_owned(),
        tokenizer_identity: "synthetic-tokenizer".to_owned(),
        admission_facts: BTreeMap::from([("source".to_owned(), "synthetic-fixture".to_owned())]),
        tensor_count: 2,
    }
}

fn fixture_backend() -> BackendSelection {
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
        radix: Some("synthetic-radix-rev".to_owned()),
        gradus: Some("synthetic-gradus-rev".to_owned()),
        faber: Some("synthetic-faber-rev".to_owned()),
        hosts: Some("synthetic-hosts-rev".to_owned()),
        dirty: false,
    }
}

fn fixture_facts() -> CompositePlanFacts {
    CompositePlanFacts {
        model_identity: fixture_model(),
        backend_selection: fixture_backend(),
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

/// The A10 source half of the analyzed fixture, computed with the same
/// FNV-1a idiom the composite-plan assembly uses (the fixture files are the
/// typed source of truth for the source domain).
fn expected_source_hashes(package: &AnalyzedProgram) -> Vec<(String, String)> {
    let mut hashes: Vec<(String, String)> = package
        .nodes
        .values()
        .map(|unit| {
            let bytes = std::fs::read(&unit.path).expect("read fixture source unit");
            let relative = unit
                .path
                .strip_prefix(&package.spec.package_root)
                .unwrap_or(&unit.path)
                .display()
                .to_string();
            (relative, fnv64_hex(&bytes))
        })
        .collect();
    hashes.sort_by(|left, right| left.0.cmp(&right.0));
    hashes
}

fn fixture_identity_source(package: &AnalyzedProgram) -> IdentitySource {
    IdentitySource {
        model: fixture_model(),
        backend: fixture_backend(),
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
            source_hashes: expected_source_hashes(package),
            revisions: fixture_revisions(),
        },
    }
}

/// Standard FNV-1a 64-bit (the preserved source-identity idiom, ddpp0-contract
/// §`FnvRemoval`).
fn fnv1a64(bytes: &[u8]) -> u64 {
    const FNV1A64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV1A64_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV1A64_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV1A64_PRIME);
    }
    hash
}

fn fnv64_hex(bytes: &[u8]) -> String {
    format!("fnv64:{:016x}", fnv1a64(bytes))
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
fn synthetic_fixture_package_analyzes_with_gradus_imports() {
    // The bounded synthetic graph is a real package importing the Gradus
    // model identity surface; it must analyze through the ordinary package
    // path before the composite-plan machinery runs over it.
    let (_dir, package) = analyze_fixture();
    assert!(!package.nodes.is_empty(), "fixture must carry source units");
    assert!(
        package.nodes.values().any(|unit| unit.is_entry),
        "fixture must carry an entry unit"
    );
    println!(
        "PASS synthetic fixture package analyzes ({} source unit(s), gradus:model imports)",
        package.nodes.len()
    );
}

#[test]
fn synthetic_plan_preserves_five_identity_domains() {
    let (_dir, package) = analyze_fixture();
    let source = fixture_identity_source(&package);
    let plan = assemble_composite_plan(&package, Target::MirFmir, fixture_facts(), &source)
        .expect("synthetic composite plan constructs with identity preservation");

    assert_eq!(plan.model_identity, source.model);
    assert_eq!(plan.backend_selection, source.backend);
    assert_eq!(plan.device_artifacts, source.artifacts);
    assert_eq!(
        plan.package_identity.source_hashes,
        source.source_identity.source_hashes
    );
    assert_eq!(plan.source_revisions, source.source_identity.revisions);
    assert_eq!(plan.prepared_regions.len(), 1);
    verify_identity_preservation(&plan, &source).expect("re-verification passes");

    println!(
        "PASS model_identity algorithm={} digest={} longitudo={} architecture={} tokenizer={} tensors={}",
        plan.model_identity.content.algorithm,
        plan.model_identity.content.digestio,
        plan.model_identity.content.longitudo,
        plan.model_identity.architecture,
        plan.model_identity.tokenizer_identity,
        plan.model_identity.tensor_count
    );
    println!(
        "PASS backend_identity leaf={:?} target={} features={:?} capacity={:?}",
        plan.backend_selection.backend,
        plan.backend_selection.target.id,
        plan.backend_selection.target.required_features,
        plan.backend_selection.required_capacity
    );
    println!(
        "PASS kernel_identity kernels={} entries={:?} regions={}",
        plan.device_program.kernels.len(),
        plan.device_program
            .kernels
            .iter()
            .map(|kernel| kernel.entry.clone())
            .collect::<Vec<_>>(),
        plan.prepared_regions.len()
    );
    println!(
        "PASS buffer_identity buffers={} versions={}",
        plan.device_program.buffer_registry().buffers.len(),
        plan.device_program
            .buffer_registry()
            .buffers
            .iter()
            .map(|entry| entry.versions.len())
            .sum::<usize>()
    );
    println!(
        "PASS source_identity hashes={} radix={:?} gradus={:?} faber={:?} hosts={:?} dirty={}",
        plan.package_identity.source_hashes.len(),
        plan.source_revisions.radix,
        plan.source_revisions.gradus,
        plan.source_revisions.faber,
        plan.source_revisions.hosts,
        plan.source_revisions.dirty
    );
    println!("PASS five identity domains preserved (5 PASS / 0 FAIL)");
}

#[test]
fn synthetic_probe_mutated_model_digest_fails_closed() {
    let (_dir, package) = analyze_fixture();
    let source = fixture_identity_source(&package);
    let mut facts = fixture_facts();
    facts.model_identity.content.digestio = format!("sha256:{}", "b".repeat(64));
    let error = assemble_composite_plan(&package, Target::MirFmir, facts, &source).unwrap_err();
    assert_first_domain(&error, "model");
    println!("PASS probe mutated-model-digest rejects with first-failing domain `model`");
}

#[test]
fn synthetic_probe_swapped_tensor_descriptor_fails_closed() {
    let (_dir, package) = analyze_fixture();
    let source = fixture_identity_source(&package);
    let mut facts = fixture_facts();
    let resources = &mut facts.device_program.kernels[0].resources;
    let weights_version = resources[0].version.clone();
    resources[0].version = resources[1].version.clone();
    resources[1].version = weights_version;
    let error = assemble_composite_plan(&package, Target::MirFmir, facts, &source).unwrap_err();
    assert_first_domain(&error, "buffer");
    println!("PASS probe swapped-tensor-descriptor rejects with first-failing domain `buffer`");
}

#[test]
fn synthetic_probe_missing_backend_artifact_fails_closed() {
    let (_dir, package) = analyze_fixture();
    let source = fixture_identity_source(&package);
    let mut facts = fixture_facts();
    facts.device_artifacts = Vec::new();
    let error = assemble_composite_plan(&package, Target::MirFmir, facts, &source).unwrap_err();
    assert_first_domain(&error, "backend");
    println!("PASS probe missing-backend-artifact rejects with first-failing domain `backend`");
}

#[test]
fn synthetic_probe_stale_source_revision_fails_closed() {
    let (_dir, package) = analyze_fixture();
    let source = fixture_identity_source(&package);
    let mut facts = fixture_facts();
    facts.source_revisions.radix = Some("stale0000".to_owned());
    let error = assemble_composite_plan(&package, Target::MirFmir, facts, &source).unwrap_err();
    assert_first_domain(&error, "source");
    println!("PASS probe stale-source-revision rejects with first-failing domain `source`");
}

#[test]
fn synthetic_probe_altered_buffer_version_fails_closed() {
    let (_dir, package) = analyze_fixture();
    let source = fixture_identity_source(&package);
    let mut facts = fixture_facts();
    facts.device_program.kernels[0].resources[0].version.version = 2;
    let error = assemble_composite_plan(&package, Target::MirFmir, facts, &source).unwrap_err();
    assert_first_domain(&error, "buffer");
    println!("PASS probe altered-buffer-version rejects with first-failing domain `buffer`");
}

#[test]
fn synthetic_probe_renamed_kernel_entry_fails_closed() {
    let (_dir, package) = analyze_fixture();
    let source = fixture_identity_source(&package);
    let mut facts = fixture_facts();
    facts.device_program.kernels[0].entry = "renamed".to_owned();
    let error = assemble_composite_plan(&package, Target::MirFmir, facts, &source).unwrap_err();
    assert_first_domain(&error, "kernel");
    println!("PASS probe renamed-kernel-entry rejects with first-failing domain `kernel`");
}
