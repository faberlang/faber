//! Package MIR linking/lowering, FMIR image construction, and artifact
//! builders (RP5).
//!
//! RP5: the compiler-owned subset of `crates/faber/src/package/mir/` moved
//! here — multi-unit lowering/linking/rewrites (`lower`, `link`, `sources`,
//! `remap`), FMIR image construction (`image`), artifact builders
//! (`build_package_mir_artifact`, `build_package_fmir_image`,
//! `build_package_fmir_text_image`),
//! runtime-requirement collection (`requirements`), the package-MIR driver
//! (`driver`, `with_lowered_package_mir`), CLI planning (`cli_bind`,
//! `cli_plan`, `cli_values`), package-MIR diagnostics (`diag`), and the
//! compiler half of the artifact-manifest writer (`manifest`).
//!
//! INVARIANT: this path links only package-local file namespace calls into one
//! validated MIR program. It does not use generated Rust metadata as the
//! runtime model, and unsupported package shapes return diagnostics.
//!
//! Faber product surfaces stay faber-side and enter here through explicit
//! [`MirSeams`] (the RP4 `EmissionSeams` posture): executed-lane target
//! resolution, library resolver/cache construction, the `[device]` manifest
//! projection, the device-section builder, the host-construction route
//! policy. Nothing here reads the ambient
//! environment, invokes external tools, or depends on faber's `device/`
//! subtree.
#![allow(dead_code)] // Binary and library targets exercise different package runner surfaces.

#[cfg(feature = "device")]
mod air_backward;
#[cfg(not(feature = "device"))]
#[path = "air_backward_stub.rs"]
mod air_backward;
mod air_specialize;
mod cli_bind;
pub(crate) mod cli_plan;
pub(crate) mod cli_values;
pub(crate) mod diag;
#[cfg(test)]
mod diag_test;
pub(crate) mod driver;
pub(crate) mod image;
mod kernel_import_calls;
pub(crate) mod link;
pub(crate) mod lower;
pub(crate) mod manifest;
#[cfg(feature = "runner")]
pub(crate) mod praefixum_link;
#[cfg(all(test, feature = "runner"))]
#[path = "praefixum_link_test.rs"]
mod praefixum_link_test;
pub(crate) mod remap;

#[cfg(test)]
mod link_order_test;
#[cfg(test)]
mod remap_test;

#[cfg(test)]
mod remap_sentinel_test;

#[cfg(test)]
mod json_import_test;
pub(crate) mod requirements;
pub(crate) mod routes;
pub(crate) mod sources;

use crate::LibraryImportBinding;
use crate::analyze::{AnalysisSeams, AnalyzedProgram, AnalyzedProgramNode, ModuleId};
use crate::import_graph::{ImportResolution, resolve_import};
use crate::library::{
    LibraryInterfaceCache, LibraryResolver, library_cached_analysis, library_cached_file_interface,
    with_library_cached_analysis_mut,
};

// ── Faber-supplied seams (RP5) ───────────────────────────────────────────────

/// Executed-lane analysis target resolution (TARGETLANE001 executed-lane
/// routing). Faber's product resolves the effective MIR-backed target from the
/// config and the manifest `[build] target`; the compiler subset receives the
/// resolved target. A manifest that names an unsupported or unreadable target
/// is an error, never a silent default.
pub type ExecutedLaneTarget =
    fn(&Config, &Path) -> Result<radix_module::codegen::Target, Box<radix_module::Diagnostic>>;

/// Library-interface-cache construction from a driver config. Faber wires its
/// locale-pack loader; the compiler subset receives the wired cache.
pub type LibraryCacheForConfig = fn(&Config) -> LibraryInterfaceCache;

/// Resolved `[device]` manifest projection (ownership-freeze §4/§5.2): the
/// `[device]` table stays faber's product view; the compiler subset receives
/// the resolved config.
pub type DeviceConfigForInput = fn(&Path) -> Result<DeviceManifestConfig, Vec<Diagnostic>>;

/// FMIR device-section construction seam (ownership-freeze §5.1). Faber's
/// `device/` subtree builds the device payload (backend selection, inputs,
/// step count, MSL/PTX artifacts); the compiler subset receives the builder.
/// Returns `Ok(None)` when the package carries no device payload.
pub type DeviceSectionBuilder = fn(
    &PreparedPackageMir<'_>,
    &LoweredMirUnit<'_>,
    &Path,
) -> Result<Option<FmirDeviceSection>, Vec<Diagnostic>>;

/// Faber-supplied package-MIR seams (RP5).
///
/// Mirrors the RP3/RP4 `AnalysisSeams`/`EmissionSeams` posture: every faber
/// product input (executed-lane target, library resolver/cache, `[device]`
/// projection, device-section builder, toolchain version) enters as an explicit function — no ambient env
/// reads, no faber product dependency inside `radix-program`
/// (package-api.md §3/§5).
#[derive(Clone, Copy)]
pub struct MirSeams {
    pub analysis: AnalysisSeams,
    pub executed_lane_target: ExecutedLaneTarget,
    pub library_cache_for_config: LibraryCacheForConfig,
    pub device_config_for_input: DeviceConfigForInput,
    pub device_section_builder: DeviceSectionBuilder,
    /// The toolchain version recorded in FMIR images (`faber_cli_version`
    /// field) and validated at decode. Faber's `CARGO_PKG_VERSION` (the
    /// product version); supplied as a seam so image bytes keep the faber
    /// version after the move (RP5 byte-parity).
    pub toolchain_version: &'static str,
}
use radix_mir::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlock, MirBlockId,
    MirCallee, MirClosureCallee, MirClosureEnvironment, MirClosureEnvironmentId, MirClosureValue,
    MirConstant, MirDiagnosticKind, MirFunction, MirFunctionId, MirIntrinsic, MirNamedOperand,
    MirOperand, MirOptionChainLink, MirOptionOp, MirPlace, MirProgram, MirProjection,
    MirProviderKind, MirRuntimeCall, MirStatement, MirStatementKind, MirSwitchCase, MirTerminator,
    MirTerminatorKind, MirType, MirValue, MirValueKind, validate_program,
};
use radix_mir_fmir::{
    FMIR_TARGET_NAME, FMIR_TEXT_TARGET_NAME, FmirBinaryImageFile, FmirDeviceSection,
    FmirDeviceSelection, FmirImageError, FmirTextCliOperand, FmirTextCliRootSection,
    FmirTextCliSection, FmirTextCliValueType, FmirTextImageFile, FmirTextProgramSection,
    FmirTextRuntimeSection, FmirTextSourceIdentity, FmirTextSourcesSection,
    FmirTextToolchainSection, FmirTextTypesSection, PACKAGE_MIR_ARTIFACT_VERSION,
    decode_binary_image, decode_text_image, encode_binary_image, encode_text_image,
    is_known_host_requirement,
};
use radix_mir_lower::lower::{
    LoweredMirUnit, MirRuntimeRecordField, MirRuntimeRecordValue, MirTensorEdgePolicyMetadata,
    MirTensorEdgePolicySelector, lower_analyzed_unit,
};
use radix_mir_runner::{
    Host, RunnerError, RunnerKernelPolicy, RunnerStats, run_entry_with_process_exit,
    run_entry_with_stats_and_kernel_policy,
};
use radix_module::cli::{
    CliCommand, CliDefault, CliExit, CliMode, CliOperand, CliOption, CliProgram, CliType,
};
use radix_module::diagnostics::{Diagnostic, DiagnosticPhase};
use radix_module::driver::Config;
use radix_module::file_interface::{InterfaceLibraryIdentity, InterfaceNominalKind};
use radix_module::hir::{
    DefId, HirBlock, HirCallArg, HirCape, HirCasuArm, HirConst, HirExpression, HirExpressionKind,
    HirId, HirItem, HirItemKind, HirLiteral, HirObjectField, HirOptionalChainKind, HirStatement,
    HirStatementKind,
};
use radix_module::lexer::{Interner, Symbol};
use radix_module::semantic::{
    IndexExpr, Primitive, Resolver, SymbolKind, Type, TypeId, TypeTable, TypeTableSnapshot,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

type NamespaceCallTargets = HashMap<(PathBuf, DefId, String), DefId>;
type NamespaceExports = HashMap<(PathBuf, DefId), BTreeSet<String>>;
type SourceRewrites = HashMap<(PathBuf, DefId), DefId>;
/// Linked const data-member member references: `(caller path, import DefId,
/// member name) → synthetic def id`. Distinct from [`NamespaceCallTargets`]
/// so the call rewrite never mistakes a const member for a callable verb.
type NamespaceDataMemberTargets = HashMap<(PathBuf, DefId, String), DefId>;
/// Linked library genus-method call targets (FMIR e2e-hardening CTO-1): per
/// caller unit, `(entry-side nominal struct DefId, method name) → synthetic
/// def id` for methods on linked library nominals. The rewrite pass turns
/// `receiver.method(args)` into `Path(synthetic)(receiver, args)` so the
/// library's lowered method function (source = the same synthetic) resolves
/// in the merged program. Keyed per caller path because each unit's imported
/// nominal carries its own canonical entry-side `DefId`.
type MethodCallTargets = HashMap<PathBuf, HashMap<(DefId, String), DefId>>;
type CliRecordFieldsByType = HashMap<TypeId, Vec<MirRuntimeRecordField>>;
type CliEntryRecords = HashMap<PathBuf, CliRecordFieldsByType>;

// Package-MIR linked function-source ids live above every HIR-lowering band
// (band table in `radix_types::def_id`) or rewritten namespace calls can
// collide with import/local bindings and lower as indirect calls. The
// constant's home is `radix_types`.
pub use radix_types::PACKAGE_MIR_SYNTHETIC_DEF_BASE;
pub const PACKAGE_MIR_TARGET_NAME: &str = "scena";
pub const PACKAGE_MIR_ARTIFACT_DIR: &str = "faber-mir";
pub const PACKAGE_MIR_MANIFEST_FILE: &str = "image.toml";
pub const FMIR_TEXT_IMAGE_FILE: &str = "image.fmir.txt";
pub const FMIR_IMAGE_FILE: &str = "image.fmir";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageMirArtifact {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    entry: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFmirTextImage {
    pub image_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFmirImage {
    pub image_path: PathBuf,
}

/// Product-owned device-selection request used by all route plumbing.
///
/// RP5: moved with the compiler subset (the moved driver/image/routes code
/// consumes it); faber keeps owning the name on its package surface via
/// re-export, and the resolved `[device]` projection enters radix-program
/// through [`MirSeams::device_config_for_input`] (ownership-freeze §3
/// posture).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceSelection {
    /// Resolve against host capability probes; fail closed when the machine
    /// admits zero or more than one backend.
    Auto,
    /// Select the reference route explicitly: kernels lower as ordinary
    /// functions and run as plain sequential code on the MIR runner, no
    /// device program, no lanes. The oracle device results are checked
    /// against. Never a silent fallback of another selection.
    Reference,
    /// Select the CPU device explicitly: processor cores as a device. The
    /// faber CLI serves it on the CPU device session (kernels on worker
    /// threads); the package-MIR routes here run it as [`Self::Reference`]
    /// (kernels lower as ordinary functions and run single-threaded on the
    /// MIR runner). Never a silent fallback of another selection.
    Cpu,
    /// Select Metal explicitly; never silently falls back.
    Metal,
    /// Select CUDA explicitly; never silently falls back.
    Cuda,
}

impl DeviceSelection {
    /// Parse a selection from its stable spelling (`auto` / `reference` /
    /// `cpu` / `metal` / `cuda`).
    #[must_use]
    pub fn from_spelling(spelling: &str) -> Option<Self> {
        match spelling {
            "auto" => Some(Self::Auto),
            "reference" => Some(Self::Reference),
            "cpu" => Some(Self::Cpu),
            "metal" => Some(Self::Metal),
            "cuda" => Some(Self::Cuda),
            _ => None,
        }
    }

    /// Stable command and diagnostic spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Reference => "reference",
            Self::Cpu => "cpu",
            Self::Metal => "metal",
            Self::Cuda => "cuda",
        }
    }

    /// How kernels lower under this selection: `reference` and `cpu` lower
    /// them as ordinary host functions, every other selection keeps the
    /// device form.
    #[must_use]
    pub const fn kernel_lowering(self) -> radix_types::KernelLowering {
        match self {
            Self::Reference | Self::Cpu => radix_types::KernelLowering::Host,
            Self::Auto | Self::Metal | Self::Cuda => radix_types::KernelLowering::Device,
        }
    }

    /// Whether the runner executes `@ kernel` functions under this selection.
    #[must_use]
    pub const fn kernel_policy(self) -> RunnerKernelPolicy {
        match self {
            Self::Reference | Self::Cpu => RunnerKernelPolicy::Execute,
            Self::Auto | Self::Metal | Self::Cuda => RunnerKernelPolicy::Refuse,
        }
    }
}

pub struct PreparedPackageMir<'a> {
    pub entry_path: PathBuf,
    pub source_paths: Vec<PathBuf>,
    pub runtime_requirements: Vec<String>,
    pub cli_exit_code: Option<i32>,
    pub fmir_text_cli: Option<FmirTextCliSection>,
    /// S1-6 vertical-slice device inputs (`[device] inputs`, mapped to f32).
    pub device_inputs: BTreeMap<String, Vec<f32>>,
    /// S1-6 device request (`[device] device`), if declared.
    pub device_selection: Option<DeviceSelection>,
    /// S5-U5b declared training step count (`[device] steps`), if declared.
    /// Absent selects the portable default [`DEFAULT_TRAINING_STEPS`].
    pub device_steps: Option<u32>,
    /// Whether the package declared any `[device]` surface (device or
    /// inputs) — the opt-in that constructs a device payload.
    pub device_declared: bool,
    _marker: std::marker::PhantomData<&'a ()>,
}

#[derive(Default)]
pub struct DeviceManifestConfig {
    pub inputs: BTreeMap<String, Vec<f32>>,
    pub selection: Option<DeviceSelection>,
    pub steps: Option<u32>,
    pub declared: bool,
}

pub struct FmirPackageImage {
    pub diagnostic_path: PathBuf,
    pub format: FmirPackageImageFormat,
    pub entry_function: String,
    pub runtime_requirements: Vec<String>,
    pub cli: Option<FmirTextCliSection>,
    pub exit_code: Option<i32>,
    pub types: TypeTableSnapshot,
    pub interner: Vec<String>,
    pub program: MirProgram,
    /// Optional device payload (N1.7): present only for images whose package
    /// carries a device program. Drives the route's `requires_device` decision
    /// in the S1-5 host factory.
    pub device: Option<FmirDeviceSection>,
    /// Source-identity hashes (the A10 identity's source half; consumed by
    /// the device route's complete-program identity).
    pub source_hashes: Vec<String>,
    /// Struct/variant validation metadata for the merged program (S1 U2
    /// VALUE members): carried in memory by source-built/loaded images so the
    /// run path re-validates with the merged variant surface. Decoded
    /// artifact images carry the default (empty) metadata.
    pub validation: FmirValidationMetadata,
}

#[derive(Clone, Copy)]
pub enum FmirPackageImageFormat {
    Source,
    FmirText,
    Fmir,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PackageMirConsumer {
    Interpreted,
    ExternalTarget,
}

/// Explicit scope for a package test MIR link.
///
/// Roots are [`AnalyzedProgram::roots`]. The consumer names one remapping
/// host and the original HIR identities it will execute. The generic package
/// linker does not depend on `proba`; it only carries these source identities
/// through its synthetic function remap.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestPackageScope {
    /// Named remapping/type-identity host for the linked test program.
    pub remap_anchor: PathBuf,
    /// Original `(module path, HIR definition)` identities to hand back after
    /// merged MIR lowering.
    pub functions: Vec<(PathBuf, radix_module::hir::DefId)>,
}

impl TestPackageScope {
    /// Test-link scope from an analyzed program: explicit program roots plus
    /// one named remapping host.
    #[must_use]
    pub fn for_program(
        package: &AnalyzedProgram,
        functions: Vec<(PathBuf, radix_module::hir::DefId)>,
    ) -> Self {
        Self {
            remap_anchor: package.test_remap_anchor(),
            functions,
        }
    }
}

/// Caller-visible identity map for functions in a linked package test MIR.
pub type TestFunctionIds = HashMap<(PathBuf, radix_module::hir::DefId), radix_mir::MirFunctionId>;

#[derive(Default)]
struct CliPackagePlan {
    entry_records: CliEntryRecords,
    dispatch: Option<CliDispatchPlan>,
    exit_code: Option<i32>,
    uses_cli_runtime: bool,
    fmir_text_cli: Option<FmirTextCliSection>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CliPlanningMode {
    Parsed,
    FmirTextRuntime,
    /// `faber check --lower`: plan a single-command root entry without
    /// argumenta, since the argv is a runtime question.
    Check,
}

struct CliDispatchPlan {
    unit_path: PathBuf,
    function: Symbol,
    record_type_rewrite: Option<CliRecordTypeRewrite>,
}

struct CliRecordTypeRewrite {
    types: Vec<(TypeId, TypeId)>,
}

struct PlannedCliOption<'a> {
    option: &'a CliOption,
    binding_name: String,
}

#[derive(Clone)]
struct PlannedCliOperand<'a> {
    operand: &'a CliOperand,
    binding_name: String,
}

struct PackageMirLinks {
    calls: NamespaceCallTargets,
    /// Call-site-specific targets for concrete AIR instances. Namespace calls
    /// share one import/member target until package planning resolves their
    /// concrete generic key; this map then gives each call its instance
    /// synthetic source identity.
    air_calls: HashMap<(PathBuf, u32, u32, DefId), DefId>,
    namespaces: NamespaceExports,
    sources: SourceRewrites,
    /// Optional/default parameter metadata for each linked function source,
    /// keyed by the synthetic definition id assigned by the package linker.
    imported_function_params: HashMap<DefId, link::ImportedFunctionParams>,
    /// Next free synthetic def id; the library-lowering pass continues from
    /// here so auto-generated sub-companion sources cannot collide with the
    /// linker's allocations.
    next_synthetic: u32,
    /// Library modules linked into the package MIR program. Each is lowered
    /// alongside package units; its exported functions (including `@ radix
    /// backward` companions) are reachable through synthetic definition ids.
    libraries: Vec<LibraryLinkTarget>,
    /// Linked const data-member member references: `(caller path, import
    /// DefId, member name) → synthetic def id`. The entry rewrite turns a
    /// `Field(Path(namespace), member)` value reference into `Path(synthetic)`
    /// so the const materializes through the entry's top-level-const seam.
    data_member_targets: NamespaceDataMemberTargets,
    /// Const data members linked into the package MIR program (sibling units
    /// and linked libraries). Each carries its synthetic def id and the source
    /// const declaration; the entry lowering transplants the const value into
    /// the entry analysis before the entry lowers.
    data_members: Vec<DataMemberLink>,
    /// Library genus-method call targets keyed by caller unit path
    /// (FMIR e2e-hardening CTO-1).
    method_targets: MethodCallTargets,
    /// Imported-device routes joined to synthetic template source `DefId`s
    /// (KPC-WIRE): each device-bearing linked-library function carries its
    /// provider identity + v2 device contract, ready for the device-aware
    /// package instantiation index.
    imported_device_routes: Vec<(DefId, radix_mir_lower::lower::ImportedDeviceRoute)>,
}

/// One linked const data member. The linker allocates a synthetic def id in
/// the package-MIR range ([`PACKAGE_MIR_SYNTHETIC_DEF_BASE`] discipline); the
/// lowering pass transplants the const's value into the entry analysis so a
/// rewritten `Path(synthetic_def)` member reference materializes through the
/// existing top-level-const seam.
struct DataMemberLink {
    /// Synthetic def id for the const member (no HIR/function-range collision).
    synthetic: DefId,
    /// Source unit path declaring the const (sibling unit or library module).
    source_path: PathBuf,
    /// The const declaration, cloned from the source analysis (source-interner
    /// symbols and source-analysis type ids).
    konst: HirConst,
}

/// One library module lowered into the package MIR program.
struct LibraryLinkTarget {
    /// Library source file path; the `source_rewrites` key for this module.
    path: PathBuf,
    /// Binding used to load the analyzed program from the library cache.
    import: LibraryImportBinding,
}

impl FmirPackageImage {
    /// The selection request recorded in the image's `device` section
    /// (fallback `auto` when the image carries none). An unknown selection id
    /// recorded on the image is representable on the packet but rejected
    /// fail-closed here — never silently remapped to `auto` or a compiled-in
    /// backend (DDCP2 gate bullet 4). The same holds for an explicit `amd`
    /// selection (AMD-A3): AMDGPU is a radix compiler leaf, not a productized
    /// faber backend — the product backend set stays Metal + Cuda
    /// (amd-gpu-target goal boundary) until the operator authorizes the
    /// product-backend stage, so `amd` fails closed at selection resolution.
    fn route_selection(&self) -> Result<DeviceSelection, Vec<Diagnostic>> {
        let Some(device) = &self.device else {
            return Ok(DeviceSelection::Auto);
        };
        match &device.selection {
            FmirDeviceSelection::Auto => Ok(DeviceSelection::Auto),
            FmirDeviceSelection::Metal => Ok(DeviceSelection::Metal),
            FmirDeviceSelection::Cuda => Ok(DeviceSelection::Cuda),
            FmirDeviceSelection::Amd => Err(vec![mir_diag(
                &self.diagnostic_path,
                "device section records an explicit `amd` backend selection; faber's productized backend set is `metal` + `cuda` (AMDGPU is a compiler leaf, not a product backend — amd-gpu-target boundary); an explicit selection must name a productized backend or `auto` (never guessed, never silently remapped)"
                    .to_owned(),
            )]),
            FmirDeviceSelection::Unknown(id) => Err(vec![mir_diag(
                &self.diagnostic_path,
                format!(
                    "device section records unknown backend selection `{id}`; an explicit selection must name a compiled-in backend (never guessed, never silently remapped)"
                ),
            )]),
        }
    }
}

/// FNV-1a 64-bit idiom for the S1/S2 SOURCE-identity surface only
/// ([`crate::mir::fnv64_hex`] — the embedded-image fingerprint and the A10
/// identity's source half). This is the separate source-identity FNV surface
/// explicitly preserved by ddpp0-contract §`FnvRemoval`; it is NOT the removed
/// backend-artifact FNV provenance (B1–B7). Standard FNV-1a, identical to the
/// deleted `radix_mir_fmir::fnv1a64`.
#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    const FNV1A64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV1A64_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = FNV1A64_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV1A64_PRIME);
    }
    hash
}

/// FNV-1a provenance hash idiom for image artifacts: a fixed-width hex
/// digest under an `fnv64:` prefix.
#[must_use]
pub fn fnv64_hex(bytes: &[u8]) -> String {
    format!("fnv64:{:016x}", fnv1a64(bytes))
}

/// Read a package source file and produce its fmir-text source-identity
/// record (relative path + FNV hash). Source identities ride images that
/// carry a device payload (the A10 identity's source half).
///
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn source_identity(
    path: &Path,
    package_root: &Path,
) -> Result<FmirTextSourceIdentity, Vec<Diagnostic>> {
    let bytes = fs::read(path).map_err(|error| {
        vec![mir_diag(
            path,
            format!("could not read fmir-text source identity: {error}"),
        )]
    })?;
    Ok(FmirTextSourceIdentity {
        file: relative_or_display(package_root, path),
        hash: fnv64_hex(&bytes),
    })
}

// Submodule items are imported here (sibling modules reach each other
// through `use super::*`); the explicit re-exports below carry the seams
// faber's package surface (and fhir.rs) depend on.
use cli_bind::bind_fmir_text_runtime_cli;
use cli_plan::{
    NominalImportContext, build_nominal_remap, build_variant_remap, command_cli_args_type,
    exported_nominal_names, import_semantic_type_with_nominal, intern_param_symbols,
    intern_record_field_symbols, parse_cli_arguments, plan_cli_package, planned_cli_operands,
    planned_cli_options, push_type_rewrite, record_shared_nominals, source_nominal_names,
};
use cli_values::{
    MissingOperandPolicy, cli_default_value, cli_operand_consumes_many, cli_operand_record_fields,
    cli_option_is_nullable, cli_option_value, cli_record_type, command_has_unsupported_options,
    command_in_unit, command_unit_id, has_unsupported_package_mir_operands,
    is_package_mir_supported_option, matching_cli_command, package_mir_cli_exit_code,
    plan_cli_command_records, push_cli_option_default_diagnostic, unsupported_cli_diagnostic,
};
use diag::{
    mir_error_diag, mir_issue_diag, mir_lowering_diag, mir_validation_diag, runner_diagnostics,
};
use driver::{
    with_prepared_package_mir, with_prepared_package_mir_from_loaded,
    with_prepared_package_mir_with_cli_mode,
};
use image::{
    FmirValidationMetadata, fmir_package_image_from_lowered, package_fmir_binary_image,
    package_fmir_text_image, run_fmir_package_image, run_fmir_package_image_with_stats,
};
use link::{
    local_namespace_call_targets, rewrite_analysis_namespace_calls, rewrite_unit_namespace_calls,
};
use lower::lower_package_units;
use manifest::{
    package_artifact_dir, package_artifact_root, package_mir_manifest,
    validate_package_mir_manifest, write_package_image,
};
use remap::{remap_function_text_symbols, remap_optional_symbol, remap_program_text_symbols};
use requirements::{check_fmir_runtime_requirements, collect_package_runtime_requirements};
use sources::{
    extend_unmapped_library_sources, rewrite_closure_environment_sources,
    rewrite_generic_param_sources, rewrite_program_sources, rewrite_templates_sources,
};

// The moved helpers live in `pub(crate)` submodules; re-export the ones
// faber's package surface re-exports (`pub use` so `radix_program::mir::…`
// reaches them for dependency crates).
pub use diag::mir_diag;
pub use image::{load_fmir_image, load_fmir_text_image};
pub use manifest::{escape_toml_basic_string, relative_or_display};

// The LLVM package-emission lane (package/llvm.rs) canonicalizes direct
// definition callees before the host-partition boundary-ABI gate.
/// CLI-checking interpreted probe used by `faber check --lower`.
pub use driver::with_check_lowered_package_mir;
/// Test-facing interpreted-consumer probe (the `faber run` consumer). Faber's
/// device tests and the radix-program mir tests use it; exposed as a normal
/// entry so dependency crates can reach it.
pub use driver::with_interpreted_lowered_package_mir;
/// [`with_linked_test_package_mir`] with the whole lowered unit (device
/// session test routes).
pub use driver::with_linked_test_package_lowered_mir;
/// Link an already-analyzed package's explicit test roots into one validated
/// MIR program and hand back the original-test identity map.
pub use driver::with_linked_test_package_mir;
#[allow(unused_imports)] // external backend harnesses consume the public package-MIR callback.
pub use driver::with_lowered_package_mir;
pub use lower::canonicalize_direct_definition_callees;

/// Imported target-entry facts from package lowering (KPC-EMIT,
/// `device-fragments-v2`): the concrete `ImportedDeviceRegistration` rows on
/// a package-lowered [`LoweredMirUnit`] translated to target-admission facts.
///
/// This is the package-path exposure of the registration carrier — the same
/// translation the radix-module driver performs for its text-target handoff.
/// Consumers merge these facts with a local entry-role map through
/// [`radix_mir::device::merge_imported_entry_roles`]: a public, composable
/// Kernel route admits its concrete instances as launch entries; private
/// helpers and Fragment roles stay composition-only. Role and launch facts
/// come from the registration's carried v2 contract, never from symbol
/// spelling, emitted text, or a second ambient cache.
#[must_use]
pub fn imported_entry_facts(
    lowered: &LoweredMirUnit<'_>,
) -> Vec<radix_mir::device::MirImportedEntryFact> {
    lowered
        .imported_device_registrations
        .iter()
        .map(|registration| {
            use radix_mir::MirDeviceRole;
            use radix_mir::device::MirImportedEntryFact;
            use radix_module::file_interface::{InterfaceDeviceRole, InterfaceDeviceVisibility};
            let contract = &registration.route.contract;
            let role = match contract.role {
                InterfaceDeviceRole::Kernel => MirDeviceRole::Kernel,
                InterfaceDeviceRole::Fragment => MirDeviceRole::Fragment,
            };
            MirImportedEntryFact {
                source: registration.key.source,
                function: registration.function,
                role,
                launchable: contract.role == InterfaceDeviceRole::Kernel
                    && contract.visibility == InterfaceDeviceVisibility::Public
                    && contract.is_composable(),
            }
        })
        .collect()
}

pub use routes::FmirImageRouteDecision;
#[cfg(feature = "hir-fhir")]
pub use routes::run_package_mir_from_loaded;
pub use routes::with_interpreted_bound_package_mir;
#[allow(unused_imports)]
pub use routes::{
    PackageRunStats, build_package_mir_artifact, run_package_mir, run_package_mir_artifact,
    run_package_mir_with_selection, run_package_mir_with_selection_and_stats,
};
#[allow(unused_imports)] // FMIR stages consume this crate-visible image API.
pub use routes::{
    build_package_fmir_image, build_package_fmir_text_image, fmir_image_route_decision,
    run_fmir_image_path, run_fmir_image_path_with_selection, run_package_fmir_built_image,
    run_package_fmir_image, run_package_fmir_image_with_selection, run_package_fmir_text_image,
    run_package_fmir_text_image_with_selection,
};

pub mod test_support;

#[cfg(test)]
#[path = "mir_test.rs"]
mod tests;

#[cfg(test)]
#[path = "nominal_test.rs"]
mod nominal_tests;

#[cfg(test)]
#[path = "reachability_test.rs"]
mod reachability_tests;
#[cfg(test)]
#[path = "transitive_generic_reach_test.rs"]
mod transitive_generic_reach_tests;

#[cfg(test)]
#[path = "lane_test.rs"]
mod lane_tests;

#[cfg(test)]
#[path = "receiver_method_lane_test.rs"]
mod receiver_method_lane_tests;

#[cfg(test)]
#[path = "mixed_shape_link_test.rs"]
mod mixed_shape_link_tests;

#[cfg(test)]
#[path = "generic_field_argument_link_test.rs"]
mod generic_field_argument_link_tests;

#[cfg(test)]
#[path = "local_module_device_metadata_test.rs"]
mod local_module_device_metadata_tests;

#[cfg(test)]
#[path = "helper_order_device_test.rs"]
mod helper_order_device_tests;

#[cfg(test)]
#[path = "sibling_generic_device_entry_test.rs"]
mod sibling_generic_device_entry_tests;

#[path = "generic_device_link_test.rs"]
mod generic_device_link_tests;

// The primal/companion instantiation this probe asserts is produced by the
// AIR backward bundle, so like `air_backward` the probe exists only with the
// device lane (the public cut has no companion to reach).
#[cfg(all(test, feature = "device"))]
#[path = "generic_kernel_backward_test.rs"]
mod generic_kernel_backward_tests;

#[cfg(test)]
#[path = "kernel_import_calls_test.rs"]
mod kernel_import_calls_tests;

#[cfg(test)]
#[path = "consumer_device_composition_test.rs"]
mod consumer_device_composition_tests;

#[cfg(test)]
#[path = "method_call_mir_test.rs"]
mod method_call_mir_tests;

#[cfg(test)]
#[path = "monomorphized_field_assign_link_test.rs"]
mod monomorphized_field_assign_link_tests;
#[cfg(test)]
#[path = "namespace_type_arg_link_test.rs"]
mod namespace_type_arg_link_tests;

#[cfg(test)]
#[path = "imported_failable_call_test.rs"]
mod imported_failable_call_tests;

#[cfg(test)]
#[path = "imported_optional_default_test.rs"]
mod imported_optional_default_tests;

#[cfg(test)]
#[path = "generic_size_forwarding_test.rs"]
mod generic_size_forwarding_tests;

#[cfg(test)]
#[path = "generic_pin_link_test.rs"]
mod generic_pin_link_tests;

#[cfg(test)]
#[path = "mut_param_link_test.rs"]
mod mut_param_link_tests;

#[cfg(test)]
#[path = "tensor_edge_policy_link_test.rs"]
mod tensor_edge_policy_link_tests;

#[cfg(test)]
#[path = "const_member_lane_test.rs"]
mod const_member_lane_tests;

#[cfg(test)]
#[path = "closure_env_link_test.rs"]
mod closure_env_link_tests;

#[cfg(test)]
#[path = "display_tokens_link_test.rs"]
mod display_tokens_link_tests;
#[cfg(test)]
#[path = "private_helper_link_test.rs"]
mod private_helper_link_tests;

#[cfg(test)]
#[path = "stats_route_test.rs"]
mod stats_route_tests;

#[cfg(test)]
#[path = "routes_test.rs"]
mod routes_tests;

#[cfg(test)]
#[path = "device_matrix_test.rs"]
mod device_matrix_tests;

#[cfg(test)]
#[path = "lowering_symbol_remap_test.rs"]
mod lowering_symbol_remap_tests;

#[cfg(test)]
#[path = "praefixum_clone_link_test.rs"]
mod praefixum_clone_link_tests;

#[cfg(test)]
#[path = "const_index_link_test.rs"]
mod const_index_link_tests;

#[cfg(test)]
#[path = "selective_library_import_link_test.rs"]
mod selective_library_import_link_tests;
