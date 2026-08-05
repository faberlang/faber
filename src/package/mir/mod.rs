//! Package MIR linking spike for the systems-lane stepper.
//!
//! INVARIANT: this path links only package-local file namespace calls into one
//! validated MIR program. It does not use generated Rust metadata as the
//! runtime model, and unsupported package shapes return diagnostics.
#![allow(dead_code)] // Binary and library targets exercise different package runner surfaces.

mod bin_runner;
mod cli_bind;
mod cli_plan;
mod cli_values;
mod diag;
mod driver;
mod image;
mod link;
mod lower;
mod manifest;
mod remap;
mod requirements;
mod routes;
mod sources;

use super::compile::{analyze_package, AnalyzedPackage, AnalyzedPackageUnit};
use super::import_graph::{resolve_import, ImportResolution};
use super::library::{
    library_cached_analysis, library_cached_file_interface, with_library_cached_analysis_mut,
    LibraryInterfaceCache,
};
use super::library_resolver_from_config;
use super::LibraryImportBinding;
use crate::library::LibraryResolver;
use faber::device::DeviceSelection;
use radix::cli::{
    CliCommand, CliDefault, CliExit, CliMode, CliOperand, CliOption, CliProgram, CliType,
};
use radix::diagnostics::{Diagnostic, DiagnosticPhase};
use radix::driver::Config;
use radix::hir::{
    DefId, HirBlock, HirCallArg, HirCape, HirCasuArm, HirExpression, HirExpressionKind,
    HirItemKind, HirObjectField, HirOptionalChainKind, HirStatement, HirStatementKind,
};
use radix::lexer::{Interner, Symbol};
use radix::mir::{
    lower_analyzed_unit_allowing_cli_runtime_records_with_context,
    lower_analyzed_unit_with_context, run_entry, validate_program, Host, LoweredMirUnit,
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlock, MirBlockId,
    MirCallee, MirClosureCallee, MirClosureEnvironment, MirClosureEnvironmentId, MirClosureValue,
    MirConstant, MirDiagnosticKind, MirFunction, MirFunctionId, MirIntrinsic, MirNamedOperand,
    MirOperand, MirOptionChainLink, MirOptionOp, MirPlace, MirProgram, MirProjection,
    MirProviderKind, MirRuntimeCall, MirRuntimeRecordField, MirRuntimeRecordValue, MirStatement,
    MirStatementKind, MirSwitchCase, MirTerminator, MirTerminatorKind, MirType, MirValue,
    MirValueKind, StepperError,
};
use radix::semantic::{IndexExpr, Primitive, Type, TypeId, TypeTable, TypeTableSnapshot};
use radix_mir_fmir::{
    decode_binary_image, decode_text_image, encode_binary_image, encode_text_image, fnv1a64,
    is_known_host_requirement, FmirBinaryImageFile, FmirDeviceBackend, FmirDeviceSection,
    FmirDeviceSelection, FmirImageError, FmirTextCliOperand, FmirTextCliRootSection,
    FmirTextCliSection, FmirTextCliValueType, FmirTextImageFile, FmirTextProgramSection,
    FmirTextRuntimeSection, FmirTextSourceIdentity, FmirTextSourcesSection,
    FmirTextToolchainSection, FmirTextTypesSection, FMIR_TARGET_NAME, FMIR_TEXT_TARGET_NAME,
    PACKAGE_MIR_ARTIFACT_VERSION,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

type NamespaceCallTargets = HashMap<(PathBuf, DefId, String), DefId>;
type NamespaceExports = HashMap<(PathBuf, DefId), BTreeSet<String>>;
type SourceRewrites = HashMap<(PathBuf, DefId), DefId>;
type CliRecordFieldsByLocal = HashMap<Symbol, Vec<MirRuntimeRecordField>>;
type CliEntryRecords = HashMap<PathBuf, CliRecordFieldsByLocal>;

// HIR lowering allocates generated DefIds starting at 1_000_000. Package MIR
// linked function-source ids must live above that range or rewritten namespace
// calls can collide with import/local bindings and lower as indirect calls.
const PACKAGE_MIR_SYNTHETIC_DEF_BASE: u32 = 2_000_000_000;
const PACKAGE_MIR_TOOLCHAIN_VERSION: &str = env!("CARGO_PKG_VERSION");
const PACKAGE_MIR_TARGET_NAME: &str = "scena";
const PACKAGE_MIR_ARTIFACT_DIR: &str = "faber-mir";
const FMIR_BIN_ARTIFACT_DIR: &str = "exe";
const PACKAGE_MIR_MANIFEST_FILE: &str = "image.toml";
const FMIR_TEXT_IMAGE_FILE: &str = "image.fmir.txt";
const FMIR_IMAGE_FILE: &str = "image.fmir";
const FMIR_BIN_ENTRYPOINT_FILE: &str = "run";
const FMIR_BIN_RUNNER_CRATE_DIR: &str = "runner";
const FMIR_BIN_RUNNER_PACKAGE_NAME: &str = "faber-fmir-bin-runner";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageMirArtifact {
    pub(crate) root: PathBuf,
    pub(crate) manifest_path: PathBuf,
    entry: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageFmirTextImage {
    pub(crate) image_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageFmirImage {
    pub(crate) image_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageFmirBinaryBundle {
    pub(crate) entrypoint_path: PathBuf,
    pub(crate) image_path: PathBuf,
}

struct PreparedPackageMir<'a> {
    entry_path: PathBuf,
    source_paths: Vec<PathBuf>,
    runtime_requirements: Vec<String>,
    cli_exit_code: Option<i32>,
    fmir_text_cli: Option<FmirTextCliSection>,
    /// S1-6 vertical-slice device inputs (`[device] inputs`, mapped to f32).
    device_inputs: BTreeMap<String, Vec<f32>>,
    /// S1-6 device backend request (`[device] backend`), if declared.
    device_backend: Option<faber::device::DeviceSelection>,
    /// S5-U5b declared training step count (`[device] steps`), if declared.
    /// Absent selects the portable default [`DEFAULT_TRAINING_STEPS`].
    device_steps: Option<u32>,
    /// Whether the package declared any `[device]` surface (backend or
    /// inputs) — the opt-in that constructs a device payload.
    device_declared: bool,
    _marker: std::marker::PhantomData<&'a ()>,
}

struct FmirPackageImage {
    diagnostic_path: PathBuf,
    format: FmirPackageImageFormat,
    entry_function: String,
    runtime_requirements: Vec<String>,
    cli: Option<FmirTextCliSection>,
    exit_code: Option<i32>,
    types: TypeTableSnapshot,
    interner: Vec<String>,
    program: MirProgram,
    /// Optional device payload (N1.7): present only for images whose package
    /// carries a device program. Drives the route's `requires_device` decision
    /// in the S1-5 host factory.
    device: Option<FmirDeviceSection>,
    /// Source-identity hashes (the A10 identity's source half; consumed by
    /// the device route's complete-program identity).
    source_hashes: Vec<String>,
}

#[derive(Clone, Copy)]
enum FmirPackageImageFormat {
    Source,
    FmirText,
    Fmir,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PackageMirConsumer {
    Interpreted,
    ExternalTarget,
}

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
    namespaces: NamespaceExports,
    sources: SourceRewrites,
    /// Next free synthetic def id; the library-lowering pass continues from
    /// here so auto-generated sub-companion sources cannot collide with the
    /// linker's allocations.
    next_synthetic: u32,
    /// Library modules linked into the package MIR program. Each is lowered
    /// alongside package units; its exported functions (including `@ radix
    /// backward` companions) are reachable through synthetic definition ids.
    libraries: Vec<LibraryLinkTarget>,
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
    /// (fallback `auto` when the image carries none).
    fn route_selection(&self) -> faber::device::DeviceSelection {
        self.device
            .as_ref()
            .map(|device| match device.selection {
                FmirDeviceSelection::Auto => faber::device::DeviceSelection::Auto,
                FmirDeviceSelection::Metal => faber::device::DeviceSelection::Metal,
                FmirDeviceSelection::Cuda => faber::device::DeviceSelection::Cuda,
            })
            .unwrap_or(faber::device::DeviceSelection::Auto)
    }
}

// Submodule items are imported here (sibling modules reach each other
// through `use super::*`); the explicit re-exports below carry the seams
// package/mod.rs (and fhir.rs / device_test.rs) depend on.
use bin_runner::*;
use cli_bind::*;
use cli_plan::*;
use cli_values::*;
use diag::*;
use driver::*;
use image::*;
use link::*;
use lower::*;
use manifest::*;
use remap::*;
use requirements::*;
use sources::*;

#[allow(unused_imports)] // external backend harnesses consume the public package-MIR callback.
pub use driver::with_lowered_package_mir;
#[cfg(test)]
pub(crate) use driver::with_interpreted_lowered_package_mir;
// generated fmir-bin runner crates consume this public API.
#[allow(unused_imports)]
pub use routes::run_fmir_image_bytes_with_stdio;
#[allow(unused_imports)] // FMIR stages consume this crate-visible image API.
pub(crate) use routes::{
    build_package_fmir_binary_bundle, build_package_fmir_image, build_package_fmir_text_image,
    fmir_image_route_decision, run_fmir_image_path, run_fmir_image_path_with_selection,
    run_package_fmir_image, run_package_fmir_image_with_selection, run_package_fmir_text_image,
    run_package_fmir_text_image_with_selection,
};
#[allow(unused_imports)]
pub(crate) use routes::{build_package_mir_artifact, run_package_mir, run_package_mir_artifact};
// binary `commands/run` resolves the interpreted consumer through fhir.rs.
pub(crate) use routes::run_package_mir_from_loaded;

#[cfg(test)]
#[path = "mir_test.rs"]
mod tests;

#[cfg(test)]
#[path = "test_support.rs"]
pub(crate) mod test_support;
