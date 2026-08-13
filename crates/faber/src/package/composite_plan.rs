//! Composite application/device plan (EXEC-01-M1: typed shape, typed-fact
//! assembly, versioned serialization).
//!
//! The composite plan is the Faber-owned product join of the DDPP0
//! `CompiledPackage` shape (`docs/factory/direct-device-product-pipeline/
//! ddpp0-contract.md` §ProductShape) and the EXEC-01 campaign row: one
//! analyzed Faber package over a public Gradus model graph becomes one typed
//! plan carrying package identity, model identity, backend selection, the
//! target-neutral `DeviceProgram`, device artifacts with their
//! `content_sha256`/`packet_sha256` identity digests, host/device call
//! facts, submission-region facts, effect/capability requirements, and exact
//! source revisions.
//!
//! SCOPE (M1): typed shape + assembly + deterministic `composite-plan v1`
//! serialization only. M2 owns identity-preservation verification and typed
//! first-divergence diagnostics; M3 owns the bounded synthetic-fixture
//! mechanism proof; M4 owns the real-graph campaign evidence. This module
//! makes no execution claim and adds no CLI or FMIR wire surface.
//!
//! TARGET: internal product-pipeline typed value mirroring
//! [`super::artifact_plan`] — debug/test/tooling surface, not a public ABI.
//! WHY: the campaign join is a build-time typed value; there is no new
//! top-level CLI (EXEC-01 challenge C1).
//!
//! # Assembly rule (DDPP0 §ProductShape)
//!
//! Assembly composes typed facts only: `AnalyzedPackage` → per-target
//! [`ArtifactPlan`](super::artifact_plan::ArtifactPlan) (package identity),
//! the target-neutral `DeviceProgram` plus its carried `DeviceSemantics`
//! (prepared regions are derived from the carried dependency graph), FMIR
//! device-section artifacts with their identity digests, Gradus
//! model-identity facts, and repo revision facts. Emitted text is never
//! reparsed; compiler facts are never reconstructed.
//!
//! # Serialization rules
//!
//! - The document schema is versioned (`composite-plan v1`).
//! - Identity digests stay separate domains (`content_sha256` vs
//!   `packet_sha256`) and never become semantic identity (DDPP0
//!   §IdentityDomains rule 1).
//! - The rendering is deterministic for a fixed plan: maps are ordered,
//!   non-semantic collections are sorted at assembly, and the device
//!   program's kernel/launch/result order is carried verbatim.
//! - No artifact bytes are rendered: the document carries per-artifact
//!   identity facts (digests, byte length, stage, provenance).

#![allow(dead_code)] // M1 shape surface; first consumers arrive in M2/M3/M4

use std::collections::BTreeMap;
use std::path::Path;

use radix::codegen::Target;
use radix::diagnostics::Diagnostic;
use radix_mir::abi::{
    MirKernelDispatchSize, MirKernelResourceAccess, MirKernelWorkgroupCount, MirWorkgroupSize,
    ReducedProjection,
};
use radix_mir::device_program::{
    BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram, DeviceProgramLifetime,
    KernelUnit, LaunchId, LaunchUnit, ObservationCadence, PreparedRegion, RegistryBuffer,
    RegistryVersion, ResultBuffer,
};
use radix_mir::device_semantics::DeviceSemantics;
use radix_mir::kernel_plan::CollectionKernelPlan;
use radix_mir::layout::MirTensorStorageLayout;
use radix_mir::MirType;
use radix_mir_fmir::{
    DeviceTargetId, FmirDeviceArtifact, FmirDeviceBackend,
};
use serde::Serialize;

use super::artifact_plan::plan_package;
use super::compile::AnalyzedPackage;

/// The versioned composite-plan document schema.
pub const COMPOSITE_PLAN_SCHEMA: &str = "composite-plan v1";

/// Gradus content identity, mirroring `gradus:model/artifact`
/// `IdentitasContenuti` and carried as data (algorithm, digest, byte
/// length) — never artifact bytes (EXEC-01 local-corpus boundary).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContentIdentity {
    /// Content-identity algorithm (`algorithmus`; e.g. `sha-256`).
    pub algorithm: String,
    /// Content digest (`digestio`; canonical `sha256:<64-hex>` spelling).
    pub digestio: String,
    /// Content byte length (`longitudo`).
    pub longitudo: i64,
}

/// Package identity: name, version, and the A10 source half — the per-file
/// FNV-1a source hashes (`fnv64:<16-hex>`, same spelling as the FMIR image
/// path's source identity).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackageIdentity {
    pub name: String,
    pub version: Option<String>,
    /// `(package-relative path, fnv64 hex)` pairs, sorted by path.
    pub source_hashes: Vec<(String, String)>,
}

/// Model identity facts carried verbatim from the Gradus model graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelIdentity {
    /// The model artifact's content identity (`IdentitasContenuti`).
    pub content: ContentIdentity,
    /// Architecture id (e.g. `qwen35moe`).
    pub architecture: String,
    /// Tokenizer identity.
    pub tokenizer_identity: String,
    /// Admission facts (bounded, keyed).
    pub admission_facts: BTreeMap<String, String>,
    /// Declared tensor count; `0` when the graph does not carry the fact.
    pub tensor_count: u64,
}

/// The backend leaf of one plan row (v1 default: one backend per row).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendLeaf {
    Metal,
    Cuda,
}

impl BackendLeaf {
    /// Map a typed FMIR backend id onto a productized plan leaf. Fail closed
    /// (returns `None`) for non-productized backend ids — never guessed,
    /// never remapped.
    #[must_use]
    pub fn from_fmir_backend(backend: FmirDeviceBackend) -> Option<Self> {
        match backend {
            FmirDeviceBackend::Metal => Some(Self::Metal),
            FmirDeviceBackend::Cuda => Some(Self::Cuda),
            FmirDeviceBackend::Amd | FmirDeviceBackend::Unknown(_) => None,
        }
    }
}

/// Backend selection row: one leaf + the typed target id + required
/// features + the required-capacity fact. Capability truth only (DDPP0
/// §SelectionPolicy rule 3); no machine is claimed here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackendSelection {
    pub backend: BackendLeaf,
    /// Typed target identity + required features (the typed artifact
    /// requirement surface, not a string allowlist).
    pub target: DeviceTargetId,
    /// Required capacity fact (e.g. `>= 24 GiB VRAM`); `None` when not
    /// declared. Honest capacity: this row never names a machine that
    /// cannot host the pinned artifact (EXEC-01 challenge C4).
    pub required_capacity: Option<String>,
}

/// Host/device call fact: one host callsite → one prepared-region identity
/// (NGAB0-R1: one host call → one prepared submission region).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostCallFact {
    /// Host callsite identity (static symbol or fixed compiled region).
    pub callsite: String,
    /// The prepared submission region this callsite submits.
    pub prepared_region: u32,
}

/// Submission-region fact: prepared-region identity + regime/shape class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubmissionRegionFact {
    /// Prepared submission region identity.
    pub region: u32,
    /// Regime class of the region.
    pub regime: String,
    /// Shape class of the region.
    pub shape_class: String,
}

/// Effect and capability requirements the plan may exercise (DDPP0
/// §ProductShape effect/capability requirements).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct EffectCapabilityRequirements {
    /// Declared host effect requirements.
    pub effects: Vec<String>,
    /// Device capability requirements (target required features, etc.).
    pub capabilities: Vec<String>,
}

/// Exact repo revisions at construction. A plan built against a dirty tree
/// records the dirty state (serialization rule 4).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct SourceRevisions {
    pub radix: Option<String>,
    pub gradus: Option<String>,
    pub faber: Option<String>,
    pub hosts: Option<String>,
    /// `true` when any recorded tree was dirty at construction.
    pub dirty: bool,
}

/// The composite application/device plan (EXEC-01): the Faber-owned join of
/// the package, model, backend, device-program, artifact, and source
/// identity domains. Shape-only in M1 — M2 adds identity verification; M3
/// and M4 own the executed proofs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositePlan {
    /// Versioned document schema (`composite-plan v1`).
    pub schema: &'static str,
    pub package_identity: PackageIdentity,
    pub model_identity: ModelIdentity,
    pub backend_selection: BackendSelection,
    /// Target-neutral device program, carried verbatim from radix-mir.
    pub device_program: DeviceProgram,
    /// Prepared submission regions derived from the carried semantics
    /// (region identity + member launches).
    pub prepared_regions: Vec<PreparedRegion>,
    /// Declared device artifacts with `content_sha256`/`packet_sha256`,
    /// materialization stage, and compiler-input parent provenance.
    pub device_artifacts: Vec<FmirDeviceArtifact>,
    pub host_call_facts: Vec<HostCallFact>,
    pub submission_region_facts: Vec<SubmissionRegionFact>,
    pub effect_capability_requirements: EffectCapabilityRequirements,
    pub source_revisions: SourceRevisions,
}

/// Typed facts the composite-plan assembly composes that are not derivable
/// from the analyzed package itself.
#[derive(Debug, Clone)]
pub struct CompositePlanFacts {
    pub model_identity: ModelIdentity,
    pub backend_selection: BackendSelection,
    /// Target-neutral device program (typed facts, never emitted text).
    pub device_program: DeviceProgram,
    /// Carried semantic facts; prepared regions are derived from them with
    /// the same rule the runtime uses.
    pub semantics: DeviceSemantics,
    /// FMIR device-section artifacts with their identity digests (carried
    /// verbatim from the typed artifact packets).
    pub device_artifacts: Vec<FmirDeviceArtifact>,
    pub host_call_facts: Vec<HostCallFact>,
    pub submission_region_facts: Vec<SubmissionRegionFact>,
    pub effect_capability_requirements: EffectCapabilityRequirements,
    pub source_revisions: SourceRevisions,
}

/// Assemble a composite plan from a typed analyzed package and typed facts.
///
/// Package identity (name, version, A10 source half) is derived from the
/// analyzed package; the remaining facts are composed verbatim. Prepared
/// regions are derived from the carried semantics (the same deterministic
/// derivation the runtime applies to the carried dependency graph). The
/// result is canonical: non-semantic collections are sorted so serialization
/// is deterministic for a fixed plan.
///
/// # Errors
/// Fails closed when a package source file cannot be read for the A10
/// source half (the same rule as the FMIR image path).
pub(crate) fn assemble_composite_plan(
    package: &AnalyzedPackage,
    target: Target,
    facts: CompositePlanFacts,
) -> Result<CompositePlan, Vec<Diagnostic>> {
    let artifact_plan = plan_package(package, target);
    let launches: Vec<LaunchId> = facts
        .device_program
        .launches
        .iter()
        .map(|launch| launch.id)
        .collect();
    let prepared_regions = facts.semantics.derive_prepared_regions(&launches);

    let mut device_artifacts = facts.device_artifacts;
    device_artifacts.sort_by(|left, right| {
        (left.backend.spelling(), left.content_sha256.as_str()).cmp(&(
            right.backend.spelling(),
            right.content_sha256.as_str(),
        ))
    });

    let mut host_call_facts = facts.host_call_facts;
    host_call_facts.sort_by(|left, right| {
        (left.callsite.as_str(), left.prepared_region).cmp(&(right.callsite.as_str(), right.prepared_region))
    });

    let mut submission_region_facts = facts.submission_region_facts;
    submission_region_facts.sort_by(|left, right| {
        (left.region, left.regime.as_str(), left.shape_class.as_str()).cmp(&(
            right.region,
            right.regime.as_str(),
            right.shape_class.as_str(),
        ))
    });

    let mut effect_capability_requirements = facts.effect_capability_requirements;
    effect_capability_requirements.effects.sort();
    effect_capability_requirements.capabilities.sort();

    Ok(CompositePlan {
        schema: COMPOSITE_PLAN_SCHEMA,
        package_identity: PackageIdentity {
            name: artifact_plan.package.name,
            version: artifact_plan.package.version,
            source_hashes: package_source_hashes(package)?,
        },
        model_identity: facts.model_identity,
        backend_selection: facts.backend_selection,
        device_program: facts.device_program,
        prepared_regions,
        device_artifacts,
        host_call_facts,
        submission_region_facts,
        effect_capability_requirements,
        source_revisions: facts.source_revisions,
    })
}

/// Render the plan as a deterministic `composite-plan v1` document.
///
/// The document renders identity facts only — per-artifact digests, byte
/// lengths, stages, and provenance — never artifact bytes and never a
/// transport spelling (serialization rule 3).
///
/// # Errors
/// Serialization failure (a serde error is surfaced as a string, matching
/// [`super::artifact_plan::ArtifactPlan::to_debug_json`]).
pub(crate) fn render_composite_plan_v1(plan: &CompositePlan) -> Result<String, String> {
    serde_json::to_string_pretty(&CompositePlanDocument::from(plan))
        .map_err(|error| error.to_string())
}

/// The A10 source half of the package identity: per-file FNV-1a source
/// hashes over the analyzed package's source units (same spelling as the
/// FMIR image path), sorted by path.
fn package_source_hashes(package: &AnalyzedPackage) -> Result<Vec<(String, String)>, Vec<Diagnostic>> {
    let mut hashes = Vec::new();
    for unit in &package.units {
        let bytes = std::fs::read(&unit.path).map_err(|error| {
            vec![crate::package_diagnostic_error(format!(
                "could not read composite-plan source identity for {}: {error}",
                unit.path.display()
            ))
            .with_arg("issue", "composite_plan_source_identity_unreadable")]
        })?;
        hashes.push((
            relative_or_display(&package.spec.package_root, &unit.path),
            fnv64_hex(&bytes),
        ));
    }
    hashes.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(hashes)
}

fn relative_or_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

// ---------------------------------------------------------------------------
// FNV-1a source identity (the preserved S1/S2 source half; ddpp0-contract
// §FnvRemoval keeps this surface separate from backend-artifact provenance).
// ---------------------------------------------------------------------------

/// FNV-1a 64-bit over `bytes` — the same idiom as the FMIR image path's
/// source identity ([`super::mir`]), replicated locally so the composite
/// plan's A10 source half agrees with the image's.
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

// ---------------------------------------------------------------------------
// `composite-plan v1` document rendering.
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct CompositePlanDocument {
    schema: String,
    package_identity: PackageIdentity,
    model_identity: ModelIdentity,
    backend_selection: BackendSelection,
    device_program: ProgramDocument,
    prepared_regions: Vec<RegionDocument>,
    device_artifacts: Vec<DeviceArtifactDocument>,
    host_call_facts: Vec<HostCallFact>,
    submission_region_facts: Vec<SubmissionRegionFact>,
    effect_capability_requirements: EffectCapabilityRequirements,
    source_revisions: SourceRevisions,
}

impl From<&CompositePlan> for CompositePlanDocument {
    fn from(plan: &CompositePlan) -> Self {
        Self {
            schema: plan.schema.to_owned(),
            package_identity: plan.package_identity.clone(),
            model_identity: plan.model_identity.clone(),
            backend_selection: plan.backend_selection.clone(),
            device_program: ProgramDocument::from(&plan.device_program),
            prepared_regions: plan
                .prepared_regions
                .iter()
                .map(RegionDocument::from)
                .collect(),
            device_artifacts: plan
                .device_artifacts
                .iter()
                .map(DeviceArtifactDocument::from)
                .collect(),
            host_call_facts: plan.host_call_facts.clone(),
            submission_region_facts: plan.submission_region_facts.clone(),
            effect_capability_requirements: plan.effect_capability_requirements.clone(),
            source_revisions: plan.source_revisions.clone(),
        }
    }
}

/// One declared device artifact rendered as its identity facts only: the
/// versioned packet metadata, `content_sha256`/`packet_sha256`, the
/// materialization stage, and the compiler-input parent provenance. Raw
/// payload bytes are never rendered (serialization rule 3).
#[derive(Serialize)]
struct DeviceArtifactDocument {
    backend: String,
    format: String,
    stage: String,
    target: DeviceTargetId,
    abi_version: u32,
    byte_length: usize,
    encoding: String,
    content_sha256: String,
    packet_sha256: String,
    compiler_input_packet_sha256: Option<String>,
}

impl From<&FmirDeviceArtifact> for DeviceArtifactDocument {
    fn from(artifact: &FmirDeviceArtifact) -> Self {
        Self {
            backend: artifact.backend.spelling(),
            format: artifact.format.spelling(),
            stage: artifact.stage.spelling().to_owned(),
            target: artifact.target.clone(),
            abi_version: artifact.abi_version,
            byte_length: artifact.bytes.len(),
            encoding: artifact_encoding_spelling(artifact),
            content_sha256: artifact.content_sha256.clone(),
            packet_sha256: artifact.packet_sha256.clone(),
            compiler_input_packet_sha256: artifact.compiler_input_packet_sha256.clone(),
        }
    }
}

/// The explicit payload-encoding declaration, without the base64 transport
/// spelling (the decoded byte length is identity-relevant; the base64 is a
/// transport spelling and is not rendered).
fn artifact_encoding_spelling(artifact: &FmirDeviceArtifact) -> String {
    match &artifact.encoding {
        radix_mir_fmir::DevicePayloadEncoding::Text => "text".to_owned(),
        radix_mir_fmir::DevicePayloadEncoding::Binary {
            encoding_tag,
            decoded_byte_length,
            ..
        } => format!("binary:{encoding_tag}:{decoded_byte_length}"),
    }
}

/// The target-neutral device program rendered from the typed program value:
/// kernels, launches, the derived buffer registry, the lifetime regime, and
/// the results — all in program order (deterministic for a fixed program).
#[derive(Serialize)]
struct ProgramDocument {
    kernels: Vec<KernelDocument>,
    launches: Vec<LaunchDocument>,
    buffers: Vec<RegistryBufferDocument>,
    lifetime: &'static str,
    results: Vec<ResultDocument>,
}

impl From<&DeviceProgram> for ProgramDocument {
    fn from(program: &DeviceProgram) -> Self {
        Self {
            kernels: program.kernels.iter().map(KernelDocument::from).collect(),
            launches: program.launches.iter().map(LaunchDocument::from).collect(),
            buffers: program
                .buffer_registry()
                .buffers
                .iter()
                .map(RegistryBufferDocument::from)
                .collect(),
            lifetime: program_lifetime_spelling(program.lifetime),
            results: program.results.iter().map(ResultDocument::from).collect(),
        }
    }
}

#[derive(Serialize)]
struct KernelDocument {
    function: u32,
    entry: String,
    plan: String,
    launch: LaunchPlanDocument,
    resources: Vec<ResourceDocument>,
}

impl From<&KernelUnit> for KernelDocument {
    fn from(kernel: &KernelUnit) -> Self {
        Self {
            function: kernel.function.0,
            entry: kernel.entry.clone(),
            plan: plan_spelling(&kernel.plan).to_owned(),
            launch: LaunchPlanDocument::from(&kernel.launch),
            resources: kernel.resources.iter().map(ResourceDocument::from).collect(),
        }
    }
}

#[derive(Serialize)]
struct LaunchPlanDocument {
    workgroup: WorkgroupDocument,
    dispatch_size: DispatchDocument,
    workgroup_count: WorkgroupCountDocument,
}

impl From<&radix_mir::device_program::KernelLaunchPlan> for LaunchPlanDocument {
    fn from(launch: &radix_mir::device_program::KernelLaunchPlan) -> Self {
        Self {
            workgroup: WorkgroupDocument::from(&launch.workgroup),
            dispatch_size: DispatchDocument::from(&launch.dispatch_size),
            workgroup_count: WorkgroupCountDocument::from(&launch.workgroup_count),
        }
    }
}

#[derive(Serialize)]
struct WorkgroupDocument {
    x: u32,
    y: u32,
    z: u32,
}

impl From<&MirWorkgroupSize> for WorkgroupDocument {
    fn from(size: &MirWorkgroupSize) -> Self {
        Self {
            x: size.x,
            y: size.y,
            z: size.z,
        }
    }
}

#[derive(Serialize)]
struct DispatchDocument {
    x: u64,
    y: u64,
    z: u64,
}

impl From<&MirKernelDispatchSize> for DispatchDocument {
    fn from(size: &MirKernelDispatchSize) -> Self {
        Self {
            x: size.x,
            y: size.y,
            z: size.z,
        }
    }
}

#[derive(Serialize)]
struct WorkgroupCountDocument {
    x: u64,
    y: u64,
    z: u64,
}

impl From<&MirKernelWorkgroupCount> for WorkgroupCountDocument {
    fn from(count: &MirKernelWorkgroupCount) -> Self {
        Self {
            x: count.x,
            y: count.y,
            z: count.z,
        }
    }
}

#[derive(Serialize)]
struct ResourceDocument {
    buffer: BufferIdentityDocument,
    version: BufferVersionDocument,
    binding: BindingDocument,
    access: &'static str,
}

impl From<&radix_mir::device_program::DeviceResource> for ResourceDocument {
    fn from(resource: &radix_mir::device_program::DeviceResource) -> Self {
        Self {
            buffer: BufferIdentityDocument::from(&resource.buffer),
            version: BufferVersionDocument::from(&resource.version),
            binding: BindingDocument::from(&resource.binding),
            access: access_spelling(resource.access),
        }
    }
}

#[derive(Serialize)]
struct BindingDocument {
    group: u32,
    binding: u32,
}

impl From<&radix_mir::device_program::Binding> for BindingDocument {
    fn from(binding: &radix_mir::device_program::Binding) -> Self {
        Self {
            group: binding.group,
            binding: binding.binding,
        }
    }
}

#[derive(Serialize)]
struct BufferIdentityDocument {
    id: u32,
    name: String,
    role: &'static str,
    storage: &'static str,
    lifetime: &'static str,
}

impl From<&BufferIdentity> for BufferIdentityDocument {
    fn from(identity: &BufferIdentity) -> Self {
        Self {
            id: identity.id.0,
            name: identity.name.clone(),
            role: role_spelling(identity.role),
            storage: storage_spelling(identity.storage),
            lifetime: lifetime_spelling(identity.lifetime),
        }
    }
}

#[derive(Serialize)]
struct BufferVersionDocument {
    version: u32,
    element_type: String,
    element_count: u64,
    reduced_projection: Option<ReducedProjectionDocument>,
}

impl From<&BufferVersion> for BufferVersionDocument {
    fn from(version: &BufferVersion) -> Self {
        Self {
            version: version.version,
            element_type: data_type_spelling(version.element_ty),
            element_count: version.element_count,
            reduced_projection: version
                .reduced_projection
                .map(ReducedProjectionDocument::from),
        }
    }
}

#[derive(Serialize)]
struct ReducedProjectionDocument {
    axis_extent: u64,
    inner_stride: u64,
}

impl From<ReducedProjection> for ReducedProjectionDocument {
    fn from(projection: ReducedProjection) -> Self {
        Self {
            axis_extent: projection.axis_extent,
            inner_stride: projection.inner_stride,
        }
    }
}

#[derive(Serialize)]
struct LaunchDocument {
    id: u32,
    kernel_index: usize,
}

impl From<&LaunchUnit> for LaunchDocument {
    fn from(launch: &LaunchUnit) -> Self {
        Self {
            id: launch.id.0,
            kernel_index: launch.kernel_index,
        }
    }
}

#[derive(Serialize)]
struct ResultDocument {
    buffer: BufferIdentityDocument,
    version: BufferVersionDocument,
    role: &'static str,
    produced_by: u32,
    cadence: &'static str,
}

impl From<&ResultBuffer> for ResultDocument {
    fn from(result: &ResultBuffer) -> Self {
        Self {
            buffer: BufferIdentityDocument::from(&result.buffer),
            version: BufferVersionDocument::from(&result.version),
            role: role_spelling(result.role),
            produced_by: result.produced_by.0,
            cadence: cadence_spelling(result.cadence),
        }
    }
}

#[derive(Serialize)]
struct RegistryBufferDocument {
    identity: BufferIdentityDocument,
    versions: Vec<RegistryVersionDocument>,
}

impl From<&RegistryBuffer> for RegistryBufferDocument {
    fn from(entry: &RegistryBuffer) -> Self {
        Self {
            identity: BufferIdentityDocument::from(&entry.identity),
            versions: entry.versions.iter().map(RegistryVersionDocument::from).collect(),
        }
    }
}

#[derive(Serialize)]
struct RegistryVersionDocument {
    version: BufferVersionDocument,
    produced_by: Vec<u32>,
    consumed_by: Vec<u32>,
}

impl From<&RegistryVersion> for RegistryVersionDocument {
    fn from(entry: &RegistryVersion) -> Self {
        Self {
            version: BufferVersionDocument::from(&entry.version),
            produced_by: entry.produced_by.iter().map(|id| id.0).collect(),
            consumed_by: entry.consumed_by.iter().map(|id| id.0).collect(),
        }
    }
}

#[derive(Serialize)]
struct RegionDocument {
    id: u32,
    launches: Vec<u32>,
}

impl From<&PreparedRegion> for RegionDocument {
    fn from(region: &PreparedRegion) -> Self {
        Self {
            id: region.id.0,
            launches: region.launches.iter().map(|id| id.0).collect(),
        }
    }
}

/// Stable plan spelling for a kernel recipe variant (no backend tag; the
/// plan never carries one).
fn plan_spelling(plan: &CollectionKernelPlan) -> &'static str {
    match plan {
        CollectionKernelPlan::Elementwise => "elementwise",
        CollectionKernelPlan::TiledMatMul(_) => "tiled-matmul",
        CollectionKernelPlan::TreeReduction(_) => "tree-reduction",
        CollectionKernelPlan::Transpose(_) => "transpose",
        CollectionKernelPlan::AxisReduction(_) => "axis-reduction",
        CollectionKernelPlan::RowSoftmax(_) => "row-softmax",
        CollectionKernelPlan::LayerNormalization(_) => "layer-normalization",
        CollectionKernelPlan::Gather(_) => "gather",
        CollectionKernelPlan::RmsNormalization(_) => "rms-normalization",
        CollectionKernelPlan::Rope(_) => "rope",
        CollectionKernelPlan::CausalMaskedSoftmax(_) => "causal-masked-softmax",
    }
}

/// Element-type spelling mirroring the S1-1 campaign dtype pin: the wire's
/// `data_type_spelling` renders the campaign dtype surface, so the plan
/// document renders the same fact. A future dtype widening updates both
/// surfaces together — never inferred from emitted text.
fn data_type_spelling(_ty: MirType) -> String {
    "f32".to_owned()
}

fn program_lifetime_spelling(lifetime: DeviceProgramLifetime) -> &'static str {
    match lifetime {
        DeviceProgramLifetime::SingleRun => "single-run",
        DeviceProgramLifetime::RepeatingStep => "repeating-step",
    }
}

fn role_spelling(role: BufferRole) -> &'static str {
    match role {
        BufferRole::Input => "input",
        BufferRole::Output => "output",
        BufferRole::InOut => "in-out",
    }
}

fn storage_spelling(storage: MirTensorStorageLayout) -> &'static str {
    match storage {
        MirTensorStorageLayout::DeviceHandle => "device-handle",
        MirTensorStorageLayout::HostOwned => "host-owned",
    }
}

fn lifetime_spelling(lifetime: BufferLifetime) -> &'static str {
    match lifetime {
        BufferLifetime::PerProgram => "per-program",
        BufferLifetime::PerStep => "per-step",
        BufferLifetime::ObservationPoint => "observation-point",
    }
}

fn access_spelling(access: MirKernelResourceAccess) -> &'static str {
    match access {
        MirKernelResourceAccess::Read => "read",
        MirKernelResourceAccess::Write => "write",
        MirKernelResourceAccess::ReadWrite => "read-write",
    }
}

fn cadence_spelling(cadence: ObservationCadence) -> &'static str {
    match cadence {
        ObservationCadence::PerStep => "per-step",
        ObservationCadence::EndOfRun => "end-of-run",
    }
}

