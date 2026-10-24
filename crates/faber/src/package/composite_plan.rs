//! Composite application/device plan (EXEC-01-M1: typed shape, typed-fact
//! assembly, versioned serialization).
//!
//! The composite plan is the Faber-owned product join of the DDPP0
//! `CompiledPackage` shape (`docs/factory/direct-device-product-pipeline/
//! ddpp0-contract.md` §`ProductShape`) and the EXEC-01 campaign row: one
//! analyzed Faber package over a public Gradus model graph becomes one typed
//! plan carrying package identity, model identity, backend selection, the
//! target-neutral `DeviceProgram`, device artifacts with their
//! `content_sha256`/`packet_sha256` identity digests, host/device call
//! facts, submission-region facts, effect/capability requirements, and exact
//! source revisions.
//!
//! SCOPE (M1+M2): typed shape + assembly + deterministic `composite-plan v1`
//! serialization + identity-preservation verification with typed
//! first-divergence diagnostics. M3 owns the bounded synthetic-fixture
//! mechanism proof; M4 owns the real-graph campaign evidence. This module
//! makes no execution claim and adds no CLI or FMIR wire surface.
//!
//! TARGET: internal product-pipeline typed value mirroring
//! [`radix::program::artifact_plan`] — debug/test/tooling surface, not a public ABI.
//! WHY: the campaign join is a build-time typed value; there is no new
//! top-level CLI (EXEC-01 challenge C1).
//!
//! # Assembly rule (DDPP0 §`ProductShape`)
//!
//! Assembly composes typed facts only: `AnalyzedProgram` → per-target
//! [`ArtifactPlan`](radix::program::artifact_plan::ArtifactPlan) (package identity),
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
//!   §`IdentityDomains` rule 1).
//! - The rendering is deterministic for a fixed plan: maps are ordered,
//!   non-semantic collections are sorted at assembly, and the device
//!   program's kernel/launch/result order is carried verbatim.
//! - No artifact bytes are rendered: the document carries per-artifact
//!   identity facts (digests, byte length, stage, provenance).

#![allow(dead_code)] // M1 shape surface; first consumers arrive in M2/M3/M4

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use radix::module::codegen::Target;
use radix::module::diagnostics::Diagnostic;
use radix_mir::MirType;
use radix_mir::abi::{
    MirKernelDispatchSize, MirKernelResourceAccess, MirKernelWorkgroupCount, MirWorkgroupSize,
    ReducedProjection,
};
use radix_mir::device_program::{
    BufferIdentity, BufferLifetime, BufferRole, BufferVersion, DeviceProgram,
    DeviceProgramLifetime, KernelUnit, LaunchId, LaunchUnit, ObservationCadence, PreparedRegion,
    RegistryBuffer, RegistryVersion, ResultBuffer,
};
use radix_mir::device_semantics::DeviceSemantics;
use radix_mir::kernel_plan::CollectionKernelPlan;
use radix_mir::layout::MirTensorStorageLayout;
use radix_mir_fmir::{DeviceTargetId, FmirDeviceArtifact, FmirDeviceBackend};
use serde::Serialize;

use radix::program::analyze::AnalyzedProgram;
use radix::program::artifact_plan::plan_package;

/// The versioned composite-plan document schema.
pub const COMPOSITE_PLAN_SCHEMA: &str = "composite-plan v1";

/// Gradus content identity, mirroring `gradus:model/artifact`
/// `ContentIdentity` and carried as data (algorithm, digest, byte
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
    /// The model artifact's content identity (`ContentIdentity`).
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
/// §`SelectionPolicy` rule 3); no machine is claimed here.
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
/// §`ProductShape` effect/capability requirements).
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

/// The five campaign identity domains (EXEC-01 challenge C3), in
/// verification order. The plan preserves each domain verbatim from its
/// typed source of truth; verification fails closed on the first divergence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityDomain {
    /// Gradus model identity (`ContentIdentity` + architecture +
    /// tokenizer identity + admission facts).
    Model,
    /// Backend selection + device artifacts (digests, stage, provenance).
    Backend,
    /// Kernel units (function id + entry) + prepared submission regions.
    Kernel,
    /// Buffer identities + versions (registry agreement).
    Buffer,
    /// Package source half (A10) + exact repo revisions.
    Source,
}

impl IdentityDomain {
    /// Stable diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Backend => "backend",
            Self::Kernel => "kernel",
            Self::Buffer => "buffer",
            Self::Source => "source",
        }
    }
}

/// Typed first-divergence diagnostic for identity verification: names the
/// first failing identity domain and the divergence detail. A divergence
/// always rejects — there is no silent repair and no FNV fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompositePlanError {
    /// The plan diverged from its typed source of truth in one domain.
    DomainMismatch {
        /// The identity domain that failed first.
        domain: IdentityDomain,
        /// The divergence detail.
        detail: String,
    },
}

impl CompositePlanError {
    /// The identity domain that diverged first.
    #[must_use]
    pub fn domain(&self) -> IdentityDomain {
        match self {
            Self::DomainMismatch { domain, .. } => *domain,
        }
    }

    /// A structured diagnostic (code + issue + named args) for the build
    /// path: the issue names the identity-divergence class and the `domain`
    /// arg names the first failing domain.
    #[must_use]
    pub fn to_diagnostic(&self) -> Diagnostic {
        match self {
            Self::DomainMismatch { domain, detail } => {
                crate::package_diagnostic_error(format!(
                    "composite-plan identity divergence: the {} identity domain does not match its typed source of truth",
                    domain.spelling()
                ))
                .with_arg("issue", "composite_plan_identity_divergence")
                .with_arg("domain", domain.spelling())
                .with_arg("detail", detail.clone())
            }
        }
    }
}

/// Expected kernel identity facts (the kernel domain's typed source of
/// truth): the kernel unit's MIR function id and target-neutral entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelIdentity {
    /// Expected MIR function id.
    pub function: u32,
    /// Expected target-neutral logical entry name.
    pub entry: String,
}

/// Expected buffer identity + version facts (the buffer domain's typed
/// source of truth), in the derived registry's first-reference order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedBuffer {
    /// Expected identity facts (id, name, role, storage, lifetime).
    pub identity: BufferIdentity,
    /// Expected content versions for the buffer.
    pub versions: Vec<BufferVersion>,
}

/// Expected source-identity facts (the source domain's typed source of
/// truth): the A10 source half and the exact repo revisions at construction.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExpectedSource {
    /// Expected per-file FNV-1a source hashes (A10 source half).
    pub source_hashes: Vec<(String, String)>,
    /// Expected exact repo revisions.
    pub revisions: SourceRevisions,
}

/// The typed sources of truth the composite plan is verified against, one
/// per campaign identity domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentitySource {
    /// Expected Gradus model identity.
    pub model: ModelIdentity,
    /// Expected backend selection.
    pub backend: BackendSelection,
    /// Expected declared device artifacts (identity digests carried
    /// verbatim from the typed artifact packets).
    pub artifacts: Vec<FmirDeviceArtifact>,
    /// Expected kernel identities, in program order.
    pub kernels: Vec<KernelIdentity>,
    /// Expected prepared-region ids, in derived order.
    pub regions: Vec<u32>,
    /// Expected buffer identity + version facts, in the derived registry's
    /// first-reference order.
    pub buffers: Vec<ExpectedBuffer>,
    /// Expected source identity facts.
    pub source_identity: ExpectedSource,
}

/// Assemble a composite plan from a typed analyzed package and typed facts,
/// verifying every identity domain against its typed source of truth.
///
/// Package identity (name, version, A10 source half) is derived from the
/// analyzed package; the remaining facts are composed verbatim. Prepared
/// regions are derived from the carried semantics (the same deterministic
/// derivation the runtime applies to the carried dependency graph). The
/// result is canonical: non-semantic collections are sorted so serialization
/// is deterministic for a fixed plan.
///
/// Construction fails closed when [`verify_identity_preservation`] finds a
/// divergence in any of the five identity domains; the typed diagnostic
/// names the first failing domain (no silent repair, no FNV fallback).
///
/// # Errors
/// Fails closed when a package source file cannot be read for the A10
/// source half (the same rule as the FMIR image path), or when identity
/// verification rejects the plan.
pub(crate) fn assemble_composite_plan(
    package: &AnalyzedProgram,
    target: Target,
    facts: CompositePlanFacts,
    source: &IdentitySource,
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
    device_artifacts.sort_by_key(artifact_sort_key);

    let mut host_call_facts = facts.host_call_facts;
    host_call_facts.sort_by(|left, right| {
        (left.callsite.as_str(), left.prepared_region)
            .cmp(&(right.callsite.as_str(), right.prepared_region))
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

    let plan = CompositePlan {
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
    };
    verify_identity_preservation(&plan, source).map_err(|error| vec![error.to_diagnostic()])?;
    Ok(plan)
}

/// Verify the plan preserves all five identity domains against their typed
/// sources of truth. The domains are checked in fixed order (model, backend,
/// kernel, buffer, source); the first divergence fails closed with a typed
/// diagnostic naming the failing domain. No silent repair, no FNV fallback.
///
/// # Errors
/// Returns the first [`CompositePlanError`] the plan violates.
pub(crate) fn verify_identity_preservation(
    plan: &CompositePlan,
    source: &IdentitySource,
) -> Result<(), CompositePlanError> {
    verify_model_identity(plan, source)?;
    verify_backend_identity(plan, source)?;
    verify_kernel_identity(plan, source)?;
    verify_buffer_identity(plan, source)?;
    verify_source_identity(plan, source)?;
    Ok(())
}

/// Model domain: exact agreement with the expected Gradus model identity,
/// then canonical well-formedness of the carried `ContentIdentity` facts
/// (the `gradus:model/artifact` admission rules) so a digest mutated away
/// from canonical form is rejected even when the expected facts agree.
fn verify_model_identity(
    plan: &CompositePlan,
    source: &IdentitySource,
) -> Result<(), CompositePlanError> {
    let found = &plan.model_identity;
    let expected = &source.model;
    if found != expected {
        return Err(CompositePlanError::DomainMismatch {
            domain: IdentityDomain::Model,
            detail: model_divergence_detail(found, expected)
                .unwrap_or_else(|| "model identity differs from its typed source".to_owned()),
        });
    }
    if found.content.algorithm != "sha-256" {
        return Err(CompositePlanError::DomainMismatch {
            domain: IdentityDomain::Model,
            detail: format!(
                "content identity algorithm `{}` is not the canonical `sha-256`",
                found.content.algorithm
            ),
        });
    }
    if !is_canonical_sha256_digestio(&found.content.digestio) {
        return Err(CompositePlanError::DomainMismatch {
            domain: IdentityDomain::Model,
            detail: format!(
                "content digest `{}` is not a canonical `sha256:<64 lowercase hex>` value",
                found.content.digestio
            ),
        });
    }
    if found.content.longitudo <= 0 {
        return Err(CompositePlanError::DomainMismatch {
            domain: IdentityDomain::Model,
            detail: format!(
                "content byte length {} is not positive",
                found.content.longitudo
            ),
        });
    }
    Ok(())
}

/// The first divergent field of the model domain, for the typed diagnostic.
fn model_divergence_detail(found: &ModelIdentity, expected: &ModelIdentity) -> Option<String> {
    if found.content != expected.content {
        return Some(
            "content identity (algorithm/digest/byte length) differs from its typed source"
                .to_owned(),
        );
    }
    if found.architecture != expected.architecture {
        return Some(format!(
            "architecture `{}` differs from expected `{}`",
            found.architecture, expected.architecture
        ));
    }
    if found.tokenizer_identity != expected.tokenizer_identity {
        return Some("tokenizer identity differs from its typed source".to_owned());
    }
    if found.admission_facts != expected.admission_facts {
        return Some("admission facts differ from their typed source".to_owned());
    }
    if found.tensor_count != expected.tensor_count {
        return Some(format!(
            "tensor count {} differs from expected {}",
            found.tensor_count, expected.tensor_count
        ));
    }
    None
}

/// Backend domain: selection agreement, per-artifact digest re-verification
/// (recompute and compare over the carried bytes — never repair), then exact
/// artifact-set agreement (no missing, replaced, or mutated artifact).
fn verify_backend_identity(
    plan: &CompositePlan,
    source: &IdentitySource,
) -> Result<(), CompositePlanError> {
    if plan.backend_selection != source.backend {
        return Err(CompositePlanError::DomainMismatch {
            domain: IdentityDomain::Backend,
            detail: "backend selection differs from its typed source".to_owned(),
        });
    }
    for artifact in &plan.device_artifacts {
        let content_digest = artifact.compute_content_sha256();
        if artifact.content_sha256 != content_digest {
            return Err(CompositePlanError::DomainMismatch {
                domain: IdentityDomain::Backend,
                detail: format!(
                    "artifact `{}` content_sha256 `{}` does not match the canonical digest `{}` of its bytes",
                    artifact.backend.spelling(),
                    artifact.content_sha256,
                    content_digest
                ),
            });
        }
        let packet_digest = artifact.compute_packet_sha256();
        if artifact.packet_sha256 != packet_digest {
            return Err(CompositePlanError::DomainMismatch {
                domain: IdentityDomain::Backend,
                detail: format!(
                    "artifact `{}` packet_sha256 `{}` does not match the canonical packet digest `{}`",
                    artifact.backend.spelling(),
                    artifact.packet_sha256,
                    packet_digest
                ),
            });
        }
    }
    let mut expected_artifacts = source.artifacts.clone();
    expected_artifacts.sort_by_key(artifact_sort_key);
    if plan.device_artifacts != expected_artifacts {
        return Err(CompositePlanError::DomainMismatch {
            domain: IdentityDomain::Backend,
            detail: "device artifacts differ from their typed source (missing, replaced, or mutated artifact)"
                .to_owned(),
        });
    }
    Ok(())
}

/// Kernel domain: kernel function ids and entries in program order, prepared
/// region ids in derived order, and the NGAB0-R1 region partition rule
/// (every launch in exactly one region, every region member declared).
fn verify_kernel_identity(
    plan: &CompositePlan,
    source: &IdentitySource,
) -> Result<(), CompositePlanError> {
    if plan.device_program.kernels.len() != source.kernels.len() {
        return Err(CompositePlanError::DomainMismatch {
            domain: IdentityDomain::Kernel,
            detail: format!(
                "kernel count {} differs from expected {}",
                plan.device_program.kernels.len(),
                source.kernels.len()
            ),
        });
    }
    for (kernel_slot, kernel) in plan.device_program.kernels.iter().enumerate() {
        let expected = &source.kernels[kernel_slot];
        if kernel.function.0 != expected.function {
            return Err(CompositePlanError::DomainMismatch {
                domain: IdentityDomain::Kernel,
                detail: format!(
                    "kernel {kernel_slot} function id {} differs from expected {}",
                    kernel.function.0, expected.function
                ),
            });
        }
        if kernel.entry != expected.entry {
            return Err(CompositePlanError::DomainMismatch {
                domain: IdentityDomain::Kernel,
                detail: format!(
                    "kernel {kernel_slot} entry `{}` differs from expected `{}`",
                    kernel.entry, expected.entry
                ),
            });
        }
    }
    if plan.prepared_regions.len() != source.regions.len() {
        return Err(CompositePlanError::DomainMismatch {
            domain: IdentityDomain::Kernel,
            detail: format!(
                "prepared-region count {} differs from expected {}",
                plan.prepared_regions.len(),
                source.regions.len()
            ),
        });
    }
    for (region_slot, region) in plan.prepared_regions.iter().enumerate() {
        if region.id.0 != source.regions[region_slot] {
            return Err(CompositePlanError::DomainMismatch {
                domain: IdentityDomain::Kernel,
                detail: format!(
                    "prepared region {region_slot} id {} differs from expected {}",
                    region.id.0, source.regions[region_slot]
                ),
            });
        }
    }
    verify_region_partition(plan)
}

/// NGAB0-R1 partition rule: every declared launch belongs to exactly one
/// prepared region and every region member is a declared launch.
fn verify_region_partition(plan: &CompositePlan) -> Result<(), CompositePlanError> {
    let declared: BTreeSet<LaunchId> = plan
        .device_program
        .launches
        .iter()
        .map(|launch| launch.id)
        .collect();
    let mut covered: BTreeSet<LaunchId> = BTreeSet::new();
    for region in &plan.prepared_regions {
        for launch in &region.launches {
            if !declared.contains(launch) {
                return Err(CompositePlanError::DomainMismatch {
                    domain: IdentityDomain::Kernel,
                    detail: format!(
                        "prepared region {} references unknown launch {}",
                        region.id.0, launch.0
                    ),
                });
            }
            if !covered.insert(*launch) {
                return Err(CompositePlanError::DomainMismatch {
                    domain: IdentityDomain::Kernel,
                    detail: format!(
                        "launch {} appears in more than one prepared region",
                        launch.0
                    ),
                });
            }
        }
    }
    if covered.len() != declared.len() {
        return Err(CompositePlanError::DomainMismatch {
            domain: IdentityDomain::Kernel,
            detail: format!(
                "launches are not covered exactly once by prepared regions ({} covered, {} declared)",
                covered.len(),
                declared.len()
            ),
        });
    }
    Ok(())
}

/// Buffer domain: the derived buffer registry agrees with the expected
/// buffer identity + version facts (in first-reference order), and no buffer
/// carries the reserved version 0.
fn verify_buffer_identity(
    plan: &CompositePlan,
    source: &IdentitySource,
) -> Result<(), CompositePlanError> {
    let registry = plan.device_program.buffer_registry();
    if registry.buffers.len() != source.buffers.len() {
        return Err(CompositePlanError::DomainMismatch {
            domain: IdentityDomain::Buffer,
            detail: format!(
                "buffer count {} differs from expected {}",
                registry.buffers.len(),
                source.buffers.len()
            ),
        });
    }
    for (buffer_slot, entry) in registry.buffers.iter().enumerate() {
        let expected = &source.buffers[buffer_slot];
        if entry.identity != expected.identity {
            return Err(CompositePlanError::DomainMismatch {
                domain: IdentityDomain::Buffer,
                detail: format!(
                    "buffer {} identity facts differ from their typed source",
                    entry.identity.id.0
                ),
            });
        }
        if entry.versions.len() != expected.versions.len() {
            return Err(CompositePlanError::DomainMismatch {
                domain: IdentityDomain::Buffer,
                detail: format!(
                    "buffer {} version count {} differs from expected {}",
                    entry.identity.id.0,
                    entry.versions.len(),
                    expected.versions.len()
                ),
            });
        }
        for (version_slot, version_entry) in entry.versions.iter().enumerate() {
            if version_entry.version.version == 0 {
                return Err(CompositePlanError::DomainMismatch {
                    domain: IdentityDomain::Buffer,
                    detail: format!(
                        "buffer {} carries the reserved version 0",
                        entry.identity.id.0
                    ),
                });
            }
            if version_entry.version != expected.versions[version_slot] {
                return Err(CompositePlanError::DomainMismatch {
                    domain: IdentityDomain::Buffer,
                    detail: format!(
                        "buffer {} version {} differs from its typed source",
                        entry.identity.id.0, version_entry.version.version
                    ),
                });
            }
        }
    }
    Ok(())
}

/// Source domain: exact agreement of the A10 source half and the exact repo
/// revisions recorded at construction.
fn verify_source_identity(
    plan: &CompositePlan,
    source: &IdentitySource,
) -> Result<(), CompositePlanError> {
    if plan.package_identity.source_hashes != source.source_identity.source_hashes {
        return Err(CompositePlanError::DomainMismatch {
            domain: IdentityDomain::Source,
            detail: "package source hashes (A10 source half) differ from their typed source"
                .to_owned(),
        });
    }
    if plan.source_revisions != source.source_identity.revisions {
        return Err(CompositePlanError::DomainMismatch {
            domain: IdentityDomain::Source,
            detail: "source revisions differ from their typed source (stale or altered revision)"
                .to_owned(),
        });
    }
    Ok(())
}

/// Canonical artifact order key: backend id, then content digest.
fn artifact_sort_key(artifact: &FmirDeviceArtifact) -> (String, String) {
    (artifact.backend.spelling(), artifact.content_sha256.clone())
}

/// Whether `digestio` is a canonical `sha256:` digest: the prefix plus 64
/// lowercase hex digits (the `gradus:model/artifact` admission rule).
fn is_canonical_sha256_digestio(digestio: &str) -> bool {
    let Some(hex) = digestio.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Render the plan as a deterministic `composite-plan v1` document.
///
/// The document renders identity facts only — per-artifact digests, byte
/// lengths, stages, and provenance — never artifact bytes and never a
/// transport spelling (serialization rule 3).
///
/// # Errors
/// Serialization failure (a serde error is surfaced as a string, matching
/// [`radix::program::artifact_plan::ArtifactPlan::to_debug_json`]).
pub(crate) fn render_composite_plan_v1(plan: &CompositePlan) -> Result<String, String> {
    serde_json::to_string_pretty(&CompositePlanDocument::from(plan))
        .map_err(|error| error.to_string())
}

/// The A10 source half of the package identity: per-file FNV-1a source
/// hashes over the analyzed package's source units (same spelling as the
/// FMIR image path), sorted by path.
fn package_source_hashes(
    package: &AnalyzedProgram,
) -> Result<Vec<(String, String)>, Vec<Diagnostic>> {
    let mut hashes = Vec::new();
    for unit in package.nodes.values() {
        let bytes = std::fs::read(&unit.path).map_err(|error| {
            vec![
                crate::package_diagnostic_error(format!(
                    "could not read composite-plan source identity for {}: {error}",
                    unit.path.display()
                ))
                .with_arg("issue", "composite_plan_source_identity_unreadable"),
            ]
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
            resources: kernel
                .resources
                .iter()
                .map(ResourceDocument::from)
                .collect(),
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
            versions: entry
                .versions
                .iter()
                .map(RegistryVersionDocument::from)
                .collect(),
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
        CollectionKernelPlan::QuantizedMatMul(_) => "quantized-matmul",
        CollectionKernelPlan::GroupedMatMul(_) => "grouped-matmul",
        CollectionKernelPlan::BatchedAttention(_) => "batched-attention",
        CollectionKernelPlan::SsmConv1d(_) => "ssm-conv1d",
        CollectionKernelPlan::SsmScan(_) => "ssm-scan",
        CollectionKernelPlan::DistributedSum(_) => "distributed-sum",
        CollectionKernelPlan::LaneGrid { .. } => "lane-grid",
        CollectionKernelPlan::SingleLane => "single-lane",
        CollectionKernelPlan::GridReduce { .. } => "grid-reduce",
        CollectionKernelPlan::Scan { .. } => "scan",
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
        // MLC-4: the sparse marker is rendered by name; never coerced to a host/device row.
        MirTensorStorageLayout::Sparse => "sparse",
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

#[cfg(test)]
#[path = "composite_plan_test.rs"]
mod tests;
