//! Package MIR linking spike for the systems-lane stepper.
//!
//! INVARIANT: this path links only package-local file namespace calls into one
//! validated MIR program. It does not use generated Rust metadata as the
//! runtime model, and unsupported package shapes return diagnostics.
#![allow(dead_code)] // Binary and library targets exercise different package runner surfaces.

use super::compile::{analyze_package, AnalyzedPackage, AnalyzedPackageUnit};
use super::import_graph::{resolve_import, ImportResolution};
use super::library_resolver_from_config;
use radix::cli::{
    CliCommand, CliDefault, CliExit, CliMode, CliOperand, CliOption, CliProgram, CliType,
};
use radix::diagnostics::{Diagnostic, DiagnosticPhase};
use radix::driver::Config;
use radix::hir::{
    DefId, HirBlock, HirCallArg, HirCape, HirCasuArm, HirExpression, HirExpressionKind,
    HirItemKind, HirObjectField, HirOptionalChainKind, HirStatement, HirStatementKind,
    LibraryProvider,
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
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

type NamespaceCallTargets = HashMap<(PathBuf, DefId, String), DefId>;
type NamespaceExports = HashMap<(PathBuf, DefId), BTreeSet<String>>;
type SourceRewrites = HashMap<(PathBuf, DefId), DefId>;
type CliRecordFieldsByLocal = HashMap<Symbol, Vec<MirRuntimeRecordField>>;
type CliEntryRecords = HashMap<PathBuf, CliRecordFieldsByLocal>;

/// Analyze, link, and validate a package as MIR, then lend the result to a target probe.
///
/// Package ownership remains inside Faber so callers cannot retain validation
/// references or reconstruct library resolution independently.
pub fn with_lowered_package_mir<R>(
    config: &Config,
    input: &Path,
    run: impl for<'a> FnOnce(&LoweredMirUnit<'a>) -> R,
) -> Result<R, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode_and_consumer(
        config,
        input,
        &[],
        CliPlanningMode::Parsed,
        PackageMirConsumer::ExternalTarget,
        |_, lowered| Ok(run(lowered)),
    )
}

// HIR lowering allocates generated DefIds starting at 1_000_000. Package MIR
// linked function-source ids must live above that range or rewritten namespace
// calls can collide with import/local bindings and lower as indirect calls.
const PACKAGE_MIR_SYNTHETIC_DEF_BASE: u32 = 2_000_000_000;
const PACKAGE_MIR_ARTIFACT_VERSION: u32 = 2;
const PACKAGE_MIR_TOOLCHAIN_VERSION: &str = env!("CARGO_PKG_VERSION");
const PACKAGE_MIR_TARGET_NAME: &str = "scena";
const FMIR_TEXT_TARGET_NAME: &str = "fmir-text";
const FMIR_TARGET_NAME: &str = "fmir";
const PACKAGE_MIR_ARTIFACT_DIR: &str = "faber-mir";
const FMIR_BIN_ARTIFACT_DIR: &str = "exe";
const PACKAGE_MIR_MANIFEST_FILE: &str = "image.toml";
const FMIR_TEXT_IMAGE_FILE: &str = "image.fmir.txt";
const FMIR_IMAGE_FILE: &str = "image.fmir";
const FMIR_BIN_ENTRYPOINT_FILE: &str = "run";
const FMIR_BIN_RUNNER_CRATE_DIR: &str = "runner";
const FMIR_BIN_RUNNER_TARGET_DIR: &str = "runner-target";
const FMIR_BIN_RUNNER_PACKAGE_NAME: &str = "faber-fmir-bin-runner";
const FNV1A64_OFFSET: u64 = 0xcbf29ce484222325;
const FNV1A64_PRIME: u64 = 0x100000001b3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageMirArtifact {
    pub(crate) root: PathBuf,
    pub(crate) manifest_path: PathBuf,
    entry: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageFmirTextImage {
    pub(crate) root: PathBuf,
    pub(crate) image_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageFmirImage {
    pub(crate) root: PathBuf,
    pub(crate) image_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageFmirBinaryBundle {
    pub(crate) root: PathBuf,
    pub(crate) entrypoint_path: PathBuf,
    pub(crate) image_path: PathBuf,
}

struct PreparedPackageMir<'a> {
    entry_path: PathBuf,
    source_paths: Vec<PathBuf>,
    runtime_requirements: Vec<String>,
    cli_exit_code: Option<i32>,
    fmir_text_cli: Option<FmirTextCliSection>,
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FmirTextImageFile {
    version: u32,
    target: String,
    package_root: String,
    entry: String,
    entry_function: String,
    toolchain: FmirTextToolchainSection,
    runtime: FmirTextRuntimeSection,
    sources: FmirTextSourcesSection,
    cli: Option<FmirTextCliSection>,
    exit_code: Option<i32>,
    types: FmirTextTypesSection,
    interner: Vec<String>,
    program: FmirTextProgramSection,
}

#[derive(Debug, Serialize, Deserialize)]
struct FmirBinaryImageFile {
    version: u32,
    target: String,
    package_root: String,
    entry: String,
    entry_function: String,
    toolchain: FmirTextToolchainSection,
    runtime: FmirTextRuntimeSection,
    sources: FmirTextSourcesSection,
    cli: Option<FmirTextCliSection>,
    exit_code: Option<i32>,
    types: FmirTextTypesSection,
    interner: Vec<String>,
    program: MirProgram,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FmirTextRuntimeSection {
    requirement: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FmirTextToolchainSection {
    faber_cli_version: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FmirTextSourcesSection {
    source: Vec<FmirTextSourceIdentity>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FmirTextSourceIdentity {
    file: String,
    hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FmirTextTypesSection {
    table: TypeTableSnapshot,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FmirTextProgramSection {
    json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FmirTextCliSection {
    root: FmirTextCliRootSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FmirTextCliRootSection {
    record: String,
    operand: Vec<FmirTextCliOperand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FmirTextCliOperand {
    field: String,
    ty: FmirTextCliValueType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum FmirTextCliValueType {
    Textus,
    Numerus,
    Fractus,
    Bivalens,
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
}

pub(crate) fn run_package_mir<H: Host + ?Sized>(
    config: &Config,
    input: &Path,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let argumenta = host.argumenta().to_vec();
    with_prepared_package_mir(config, input, &argumenta, |prepared, lowered| {
        let image = fmir_package_image_from_lowered(
            prepared,
            lowered,
            prepared.entry_path.clone(),
            FmirPackageImageFormat::Source,
        );
        run_fmir_package_image(image, host)
    })
}

pub(crate) fn build_package_mir_artifact(
    config: &Config,
    input: &Path,
    argumenta: &[String],
) -> Result<PackageMirArtifact, Vec<Diagnostic>> {
    with_prepared_package_mir(config, input, argumenta, |prepared, _| {
        let package_root = package_artifact_root(input)?;
        let artifact_root = package_root.join("target").join(PACKAGE_MIR_ARTIFACT_DIR);
        fs::create_dir_all(&artifact_root)
            .map_err(|error| vec![mir_diag(&prepared.entry_path, error.to_string())])?;
        let manifest_path = artifact_root.join(PACKAGE_MIR_MANIFEST_FILE);
        fs::write(
            &manifest_path,
            package_mir_manifest(prepared, &package_root),
        )
        .map_err(|error| vec![mir_diag(&prepared.entry_path, error.to_string())])?;
        Ok(PackageMirArtifact {
            root: artifact_root,
            manifest_path,
            entry: prepared.entry_path.clone(),
        })
    })
}

pub(crate) fn build_package_fmir_text_image(
    config: &Config,
    input: &Path,
    argumenta: &[String],
) -> Result<PackageFmirTextImage, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode(
        config,
        input,
        argumenta,
        CliPlanningMode::FmirTextRuntime,
        |prepared, lowered| {
            let package_root = package_artifact_root(input)?;
            let artifact_root = package_root.join("target").join(PACKAGE_MIR_ARTIFACT_DIR);
            fs::create_dir_all(&artifact_root)
                .map_err(|error| vec![mir_diag(&prepared.entry_path, error.to_string())])?;
            let image_path = artifact_root.join(FMIR_TEXT_IMAGE_FILE);
            let image = package_fmir_text_image(prepared, lowered, &package_root)?;
            fs::write(&image_path, image)
                .map_err(|error| vec![mir_diag(&prepared.entry_path, error.to_string())])?;
            Ok(PackageFmirTextImage {
                root: artifact_root,
                image_path,
            })
        },
    )
}

pub(crate) fn build_package_fmir_image(
    config: &Config,
    input: &Path,
    argumenta: &[String],
) -> Result<PackageFmirImage, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode(
        config,
        input,
        argumenta,
        CliPlanningMode::FmirTextRuntime,
        |prepared, lowered| {
            let package_root = package_artifact_root(input)?;
            let artifact_root = package_root.join("target").join(PACKAGE_MIR_ARTIFACT_DIR);
            fs::create_dir_all(&artifact_root)
                .map_err(|error| vec![mir_diag(&prepared.entry_path, error.to_string())])?;
            let image_path = artifact_root.join(FMIR_IMAGE_FILE);
            let image = package_fmir_binary_image(prepared, lowered, &package_root)?;
            fs::write(&image_path, image)
                .map_err(|error| vec![mir_diag(&prepared.entry_path, error.to_string())])?;
            Ok(PackageFmirImage {
                root: artifact_root,
                image_path,
            })
        },
    )
}

pub(crate) fn build_package_fmir_binary_bundle(
    config: &Config,
    input: &Path,
    argumenta: &[String],
    release: bool,
) -> Result<PackageFmirBinaryBundle, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode(
        config,
        input,
        argumenta,
        CliPlanningMode::FmirTextRuntime,
        |prepared, lowered| {
            let package_root = package_artifact_root(input)?;
            let artifact_root = package_root
                .join("target")
                .join(PACKAGE_MIR_ARTIFACT_DIR)
                .join(FMIR_BIN_ARTIFACT_DIR);
            fs::create_dir_all(&artifact_root)
                .map_err(|error| vec![mir_diag(&prepared.entry_path, error.to_string())])?;
            let image_path = artifact_root.join(FMIR_IMAGE_FILE);
            let image = package_fmir_binary_image(prepared, lowered, &package_root)?;
            fs::write(&image_path, image)
                .map_err(|error| vec![mir_diag(&prepared.entry_path, error.to_string())])?;

            let entrypoint_path = artifact_root.join(FMIR_BIN_ENTRYPOINT_FILE);
            write_fmir_bin_runner(
                &artifact_root,
                &entrypoint_path,
                &prepared.entry_path,
                release,
            )?;

            Ok(PackageFmirBinaryBundle {
                root: artifact_root,
                entrypoint_path,
                image_path,
            })
        },
    )
}

pub(crate) fn run_package_mir_artifact<H: Host + ?Sized>(
    config: &Config,
    artifact: &PackageMirArtifact,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let manifest = fs::read_to_string(&artifact.manifest_path).map_err(|error| {
        vec![mir_diag(
            &artifact.manifest_path,
            format!("could not read package MIR artifact manifest: {error}"),
        )]
    })?;
    validate_package_mir_manifest(&manifest, &artifact.manifest_path)?;
    run_package_mir(config, &artifact.entry, host)
}

pub(crate) fn run_package_fmir_text_image<H: Host + ?Sized>(
    image: &PackageFmirTextImage,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let image_text = fs::read_to_string(&image.image_path).map_err(|error| {
        vec![mir_diag(
            &image.image_path,
            format!("could not read fmir-text image: {error}"),
        )]
    })?;
    let loaded = load_fmir_text_image(&image_text, &image.image_path)?;
    run_fmir_package_image(loaded, host)
}

pub(crate) fn run_package_fmir_image<H: Host + ?Sized>(
    image: &PackageFmirImage,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    run_fmir_image_path(&image.image_path, host)
}

pub(crate) fn run_fmir_image_path<H: Host + ?Sized>(
    image_path: &Path,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let image_bytes = fs::read(image_path).map_err(|error| {
        vec![mir_diag(
            image_path,
            format!("could not read fmir image: {error}"),
        )]
    })?;
    let loaded = load_fmir_image(&image_bytes, image_path)?;
    run_fmir_package_image(loaded, host)
}

pub fn run_fmir_image_bytes_with_stdio(
    image_bytes: &[u8],
    diagnostic_path: &Path,
    argumenta: Vec<String>,
) -> Result<(), Vec<Diagnostic>> {
    let loaded = load_fmir_image(image_bytes, diagnostic_path)?;
    let mut host = radix::mir::StdioHost::with_argumenta(argumenta);
    run_fmir_package_image(loaded, &mut host)
}

fn fmir_package_image_from_lowered(
    prepared: &PreparedPackageMir<'_>,
    lowered: &LoweredMirUnit<'_>,
    diagnostic_path: PathBuf,
    format: FmirPackageImageFormat,
) -> FmirPackageImage {
    FmirPackageImage {
        diagnostic_path,
        format,
        entry_function: "run_entry".to_owned(),
        runtime_requirements: prepared.runtime_requirements.clone(),
        cli: prepared.fmir_text_cli.clone(),
        exit_code: prepared.cli_exit_code,
        types: lowered.validation.types.snapshot(),
        interner: lowered
            .validation
            .interner
            .map(|interner| interner.strings().to_vec())
            .unwrap_or_default(),
        program: lowered.program.clone(),
    }
}

fn run_fmir_package_image<H: Host + ?Sized>(
    mut image: FmirPackageImage,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    check_fmir_runtime_requirements(&image)?;
    let types = TypeTable::from_snapshot(image.types).map_err(|error| {
        vec![mir_issue_diag(
            &image.diagnostic_path,
            "fmir_image_type_metadata_invalid",
            format!(
                "could not load {} type metadata: {error}",
                image.format.label()
            ),
        )
        .with_arg("format", image.format.label())]
    })?;
    let mut interner = radix::lexer::Interner::from_strings(image.interner);
    bind_fmir_text_runtime_cli(
        &mut image.program,
        image.cli.as_ref(),
        &image.entry_function,
        &mut interner,
        host,
        &image.diagnostic_path,
    )?;
    let mut validation = radix::mir::MirValidationContext::new(&types);
    validation.interner = Some(&interner);
    run_entry(&image.program, &validation, host)
        .map_err(|errors| stepper_diagnostics(&image.diagnostic_path, errors))?;
    if let Some(code) = image.exit_code {
        host.exit(code);
    }
    Ok(())
}

impl FmirPackageImageFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Source => "source-built FMIR",
            Self::FmirText => "fmir-text",
            Self::Fmir => "fmir",
        }
    }
}

fn with_prepared_package_mir<R>(
    config: &Config,
    input: &Path,
    argumenta: &[String],
    run: impl for<'a> FnOnce(&PreparedPackageMir<'a>, &LoweredMirUnit<'a>) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode(config, input, argumenta, CliPlanningMode::Parsed, run)
}

fn with_prepared_package_mir_with_cli_mode<R>(
    config: &Config,
    input: &Path,
    argumenta: &[String],
    cli_mode: CliPlanningMode,
    run: impl for<'a> FnOnce(&PreparedPackageMir<'a>, &LoweredMirUnit<'a>) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode_and_consumer(
        config,
        input,
        argumenta,
        cli_mode,
        PackageMirConsumer::Interpreted,
        run,
    )
}

fn with_prepared_package_mir_with_cli_mode_and_consumer<R>(
    config: &Config,
    input: &Path,
    argumenta: &[String],
    cli_mode: CliPlanningMode,
    consumer: PackageMirConsumer,
    run: impl for<'a> FnOnce(&PreparedPackageMir<'a>, &LoweredMirUnit<'a>) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    let mut package = analyze_package(config, input)?;
    if package
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.is_error())
    {
        return Err(package.diagnostics);
    }
    if consumer == PackageMirConsumer::Interpreted {
        if let Some(diagnostics) = library_import_diagnostics(&package) {
            return Err(diagnostics);
        }
    }
    let cli_plan = plan_cli_package(&mut package, argumenta, cli_mode)?;

    let links = local_namespace_call_targets(config, &package, consumer)?;
    let entry_index = select_entry_unit(&package)?;
    let entry_path = package.units[entry_index].path.clone();
    let source_paths = package.units.iter().map(|unit| unit.path.clone()).collect();
    for unit in &mut package.units {
        rewrite_unit_namespace_calls(unit, &links.calls, &links.namespaces)?;
    }

    let mut lowered = lower_package_units(&mut package, entry_index, &links.sources, &cli_plan)?;
    validate_program(&lowered.program, &lowered.validation).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| mir_lowering_diag(&entry_path, error.message))
            .collect::<Vec<_>>()
    })?;
    if consumer == PackageMirConsumer::Interpreted {
        bridge_norma_providers_to_kernel(&mut lowered, &entry_path)?;
    }
    let runtime_requirements = collect_package_runtime_requirements(&lowered, &cli_plan);
    let prepared = PreparedPackageMir {
        entry_path: entry_path.clone(),
        source_paths,
        runtime_requirements,
        cli_exit_code: cli_plan.exit_code,
        fmir_text_cli: cli_plan.fmir_text_cli.clone(),
        _marker: std::marker::PhantomData,
    };
    run(&prepared, &lowered)
}

fn package_artifact_root(input: &Path) -> Result<PathBuf, Vec<Diagnostic>> {
    super::discover_build_layout(input)
        .map(|layout| layout.package_root)
        .map_err(|diagnostic| vec![*diagnostic])
}

fn package_mir_manifest(prepared: &PreparedPackageMir<'_>, package_root: &Path) -> String {
    let entry = escape_manifest_value(&relative_or_display(package_root, &prepared.entry_path));
    let mut manifest = format!(
        "version = {}\ntarget = \"{}\"\nentry = \"{}\"\nentry_function = \"run_entry\"\n\n[runtime]\n",
        PACKAGE_MIR_ARTIFACT_VERSION, PACKAGE_MIR_TARGET_NAME, entry
    );
    for requirement in &prepared.runtime_requirements {
        manifest.push_str(&format!(
            "requirement = \"{}\"\n",
            escape_manifest_value(requirement)
        ));
    }
    manifest.push_str("\n[sources]\n");
    let mut sources = prepared
        .source_paths
        .iter()
        .map(|source| relative_or_display(package_root, source))
        .collect::<Vec<_>>();
    sources.sort();
    for source in sources {
        manifest.push_str(&format!("file = \"{}\"\n", escape_manifest_value(&source)));
    }
    manifest
}

fn relative_or_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn escape_manifest_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn package_fmir_text_image(
    prepared: &PreparedPackageMir<'_>,
    lowered: &LoweredMirUnit<'_>,
    package_root: &Path,
) -> Result<String, Vec<Diagnostic>> {
    let program_json = serde_json::to_string_pretty(&lowered.program).map_err(|error| {
        vec![mir_diag(
            &prepared.entry_path,
            format!("could not encode fmir-text program: {error}"),
        )]
    })?;
    let mut sources = prepared
        .source_paths
        .iter()
        .map(|path| source_identity(path, package_root))
        .collect::<Result<Vec<_>, _>>()?;
    sources.sort_by(|left, right| left.file.cmp(&right.file));

    let image = FmirTextImageFile {
        version: PACKAGE_MIR_ARTIFACT_VERSION,
        target: FMIR_TEXT_TARGET_NAME.to_owned(),
        package_root: ".".to_owned(),
        entry: relative_or_display(package_root, &prepared.entry_path),
        entry_function: "run_entry".to_owned(),
        toolchain: FmirTextToolchainSection {
            faber_cli_version: PACKAGE_MIR_TOOLCHAIN_VERSION.to_owned(),
        },
        runtime: FmirTextRuntimeSection {
            requirement: prepared.runtime_requirements.clone(),
        },
        sources: FmirTextSourcesSection { source: sources },
        cli: prepared.fmir_text_cli.clone(),
        exit_code: prepared.cli_exit_code,
        types: FmirTextTypesSection {
            table: lowered.validation.types.snapshot(),
        },
        interner: lowered
            .validation
            .interner
            .map(|interner| interner.strings().to_vec())
            .unwrap_or_default(),
        program: FmirTextProgramSection { json: program_json },
    };
    toml::to_string_pretty(&image).map_err(|error| {
        vec![mir_diag(
            &prepared.entry_path,
            format!("could not encode fmir-text image: {error}"),
        )]
    })
}

fn package_fmir_binary_image(
    prepared: &PreparedPackageMir<'_>,
    lowered: &LoweredMirUnit<'_>,
    package_root: &Path,
) -> Result<Vec<u8>, Vec<Diagnostic>> {
    let mut sources = prepared
        .source_paths
        .iter()
        .map(|path| source_identity(path, package_root))
        .collect::<Result<Vec<_>, _>>()?;
    sources.sort_by(|left, right| left.file.cmp(&right.file));

    let image = FmirBinaryImageFile {
        version: PACKAGE_MIR_ARTIFACT_VERSION,
        target: FMIR_TARGET_NAME.to_owned(),
        package_root: ".".to_owned(),
        entry: relative_or_display(package_root, &prepared.entry_path),
        entry_function: "run_entry".to_owned(),
        toolchain: FmirTextToolchainSection {
            faber_cli_version: PACKAGE_MIR_TOOLCHAIN_VERSION.to_owned(),
        },
        runtime: FmirTextRuntimeSection {
            requirement: prepared.runtime_requirements.clone(),
        },
        sources: FmirTextSourcesSection { source: sources },
        cli: prepared.fmir_text_cli.clone(),
        exit_code: prepared.cli_exit_code,
        types: FmirTextTypesSection {
            table: lowered.validation.types.snapshot(),
        },
        interner: lowered
            .validation
            .interner
            .map(|interner| interner.strings().to_vec())
            .unwrap_or_default(),
        program: lowered.program.clone(),
    };
    postcard::to_allocvec(&image).map_err(|error| {
        vec![mir_diag(
            &prepared.entry_path,
            format!("could not encode fmir image: {error}"),
        )]
    })
}

fn write_fmir_bin_runner(
    artifact_root: &Path,
    entrypoint_path: &Path,
    diagnostic_path: &Path,
    release: bool,
) -> Result<(), Vec<Diagnostic>> {
    let runner_root = artifact_root.join(FMIR_BIN_RUNNER_CRATE_DIR);
    let runner_src = runner_root.join("src");
    fs::create_dir_all(&runner_src)
        .map_err(|error| vec![mir_diag(diagnostic_path, error.to_string())])?;
    fs::write(
        runner_root.join("Cargo.toml"),
        render_fmir_bin_runner_cargo_toml(),
    )
    .map_err(|error| vec![mir_diag(diagnostic_path, error.to_string())])?;
    fs::write(runner_src.join("main.rs"), render_fmir_bin_runner_main_rs())
        .map_err(|error| vec![mir_diag(diagnostic_path, error.to_string())])?;

    let built_runner = invoke_fmir_bin_runner_build(
        &runner_root.join("Cargo.toml"),
        &artifact_root.join(FMIR_BIN_RUNNER_TARGET_DIR),
        diagnostic_path,
        release,
    )?;
    fs::copy(&built_runner, entrypoint_path)
        .map_err(|error| vec![mir_diag(diagnostic_path, error.to_string())])?;
    make_fmir_bin_entrypoint_executable(entrypoint_path, diagnostic_path)?;
    Ok(())
}

fn render_fmir_bin_runner_cargo_toml() -> String {
    let faber_cli_path = toml_basic_string_path(Path::new(env!("CARGO_MANIFEST_DIR")));
    format!(
        r#"[package]
name = "{FMIR_BIN_RUNNER_PACKAGE_NAME}"
version = "0.0.0"
edition = "2021"

# This crate was generated by `faber build --target fmir-bin`.
# It is a fixed FMIR runner shell; the user program is embedded as image.fmir.
# Do not edit this file by hand.

[workspace]
# Keep the generated crate independent when a package lives in another
# workspace tree.

[dependencies]
faber = {{ path = "{faber_cli_path}", version = "={PACKAGE_MIR_TOOLCHAIN_VERSION}" }}
"#
    )
}

fn render_fmir_bin_runner_main_rs() -> &'static str {
    r#"fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if let Err(diagnostics) = faber_cli::package::run_fmir_image_bytes_with_stdio(
        include_bytes!("../../image.fmir"),
        std::path::Path::new("embedded:image.fmir"),
        args,
    ) {
        for diagnostic in diagnostics {
            if diagnostic.is_error() {
                eprintln!("error: {}", diagnostic.message);
            } else {
                eprintln!("warning: {}", diagnostic.message);
            }
        }
        eprintln!("fmir image execution failed");
        std::process::exit(1);
    }
}
"#
}

fn invoke_fmir_bin_runner_build(
    manifest_path: &Path,
    target_dir: &Path,
    diagnostic_path: &Path,
    release: bool,
) -> Result<PathBuf, Vec<Diagnostic>> {
    let mut command = std::process::Command::new("cargo");
    command
        .arg("build")
        .arg("--manifest-path")
        .arg(manifest_path)
        .arg("--target-dir")
        .arg(target_dir);

    if release {
        command.arg("--release");
    }

    let status = command.status().map_err(|error| {
        vec![mir_diag(
            diagnostic_path,
            format!("failed to spawn cargo for fmir-bin runner: {error}"),
        )]
    })?;
    if !status.success() {
        return Err(vec![mir_diag(
            diagnostic_path,
            format!("fmir-bin runner cargo build exited with status {status}"),
        )]);
    }

    let profile = if release { "release" } else { "debug" };
    Ok(target_dir.join(profile).join(format!(
        "{FMIR_BIN_RUNNER_PACKAGE_NAME}{}",
        std::env::consts::EXE_SUFFIX
    )))
}

fn toml_basic_string_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[cfg(unix)]
fn make_fmir_bin_entrypoint_executable(
    entrypoint_path: &Path,
    diagnostic_path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::metadata(entrypoint_path)
        .map_err(|error| vec![mir_diag(diagnostic_path, error.to_string())])?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(entrypoint_path, permissions)
        .map_err(|error| vec![mir_diag(diagnostic_path, error.to_string())])
}

#[cfg(not(unix))]
fn make_fmir_bin_entrypoint_executable(
    _entrypoint_path: &Path,
    _diagnostic_path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    Ok(())
}

fn source_identity(
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
        hash: format!("fnv64:{:016x}", fnv1a64(&bytes)),
    })
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = FNV1A64_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV1A64_PRIME);
    }
    hash
}

fn check_fmir_runtime_requirements(image: &FmirPackageImage) -> Result<(), Vec<Diagnostic>> {
    let unsupported = image
        .runtime_requirements
        .iter()
        .filter(|requirement| !is_known_fmir_runtime_requirement(requirement))
        .cloned()
        .collect::<Vec<_>>();
    if unsupported.is_empty() {
        return Ok(());
    }
    Err(unsupported
        .into_iter()
        .map(|requirement| {
            mir_issue_diag(
                &image.diagnostic_path,
                "fmir_runtime_requirement_unsupported",
                format!(
                    "{} image declares unsupported runtime requirement `{requirement}`",
                    image.format.label()
                ),
            )
            .with_arg("format", image.format.label())
            .with_arg("requirement", requirement)
        })
        .collect())
}

fn is_known_fmir_runtime_requirement(requirement: &str) -> bool {
    match requirement {
        "host:argv" | "host:exit" | "host:stdout" | "host:stderr" | "host:stdin" | "host:fs"
        | "host:env" | "host:cwd" | "host:pid" | "host:random" | "host:process" => true,
        _ => is_known_fmir_kernel_requirement(requirement),
    }
}

fn is_known_fmir_kernel_requirement(requirement: &str) -> bool {
    let Some(rest) = requirement.strip_prefix("kernel:") else {
        return false;
    };
    let Some((module_name, verb)) = rest.split_once('.') else {
        return false;
    };
    let Some(module) = radix::kernel::resolve_kernel_module_name(module_name) else {
        return false;
    };
    radix::kernel::kernel_module_supports_verb(module, verb)
}

fn bind_fmir_text_runtime_cli<H: Host + ?Sized>(
    program: &mut MirProgram,
    cli: Option<&FmirTextCliSection>,
    entry_function: &str,
    interner: &mut Interner,
    host: &H,
    path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let Some(cli) = cli else {
        return Ok(());
    };
    if cli.root.operand.is_empty() {
        return Ok(());
    }
    let argumenta = host.argumenta();
    if argumenta.len() != cli.root.operand.len() {
        return Err(vec![mir_diag(
            path,
            format!(
                "fmir-text image expected {} runtime argument(s), got {}",
                cli.root.operand.len(),
                argumenta.len()
            ),
        )]);
    }
    let mut bindings = Vec::new();
    for (operand, raw) in cli.root.operand.iter().zip(argumenta.iter()) {
        let name = interner.intern(&operand.field);
        let value = fmir_text_runtime_cli_operand(&operand.ty, raw, interner, path)?;
        bindings.push(MirNamedOperand { name, value });
    }
    if patch_fmir_text_cli_record(program, cli, entry_function, interner, &bindings) {
        Ok(())
    } else {
        Err(vec![mir_diag(
            path,
            "fmir-text image could not bind runtime CLI record",
        )])
    }
}

fn fmir_text_runtime_cli_operand(
    ty: &FmirTextCliValueType,
    raw: &str,
    interner: &mut Interner,
    path: &Path,
) -> Result<MirOperand, Vec<Diagnostic>> {
    let constant = match ty {
        FmirTextCliValueType::Textus => MirConstant::String(interner.intern(raw)),
        FmirTextCliValueType::Numerus => MirConstant::Int(raw.parse::<i64>().map_err(|_| {
            vec![mir_diag(
                path,
                format!("fmir-text runtime argument `{raw}` is not numerus"),
            )]
        })?),
        FmirTextCliValueType::Fractus => MirConstant::Float(raw.parse::<f64>().map_err(|_| {
            vec![mir_diag(
                path,
                format!("fmir-text runtime argument `{raw}` is not fractus"),
            )]
        })?),
        FmirTextCliValueType::Bivalens => MirConstant::Bool(raw.parse::<bool>().map_err(|_| {
            vec![mir_diag(
                path,
                format!("fmir-text runtime argument `{raw}` is not bivalens"),
            )]
        })?),
    };
    Ok(MirOperand::Constant(constant))
}

fn patch_fmir_text_cli_record(
    program: &mut MirProgram,
    cli: &FmirTextCliSection,
    entry_function: &str,
    interner: &Interner,
    bindings: &[MirNamedOperand],
) -> bool {
    if cli.root.record.is_empty() {
        return false;
    }
    for function in &mut program.functions {
        if !fmir_text_function_matches_entry(function, entry_function, interner) {
            continue;
        }
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                let MirStatementKind::Construct { aggregate, .. } = &mut statement.kind else {
                    continue;
                };
                if !matches!(aggregate.kind, MirAggregateKind::Record) {
                    continue;
                }
                let MirAggregateFields::Named(fields) = &mut aggregate.fields else {
                    continue;
                };
                if patch_fmir_text_cli_record_fields(fields, bindings) {
                    return true;
                }
            }
        }
    }
    false
}

fn fmir_text_function_matches_entry(
    function: &MirFunction,
    entry_function: &str,
    interner: &Interner,
) -> bool {
    if entry_function == "run_entry"
        && function.source.is_none()
        && function.name.is_none()
        && function.params.is_empty()
    {
        return true;
    }
    function
        .name
        .map(|name| interner.resolve(name) == entry_function)
        .unwrap_or(false)
}

fn patch_fmir_text_cli_record_fields(
    fields: &mut [MirNamedOperand],
    bindings: &[MirNamedOperand],
) -> bool {
    if fields.len() != bindings.len() {
        return false;
    }
    let field_names = fields
        .iter()
        .map(|field| field.name)
        .collect::<HashSet<_>>();
    let binding_names = bindings
        .iter()
        .map(|binding| binding.name)
        .collect::<HashSet<_>>();
    if field_names != binding_names {
        return false;
    }
    for binding in bindings {
        if let Some(field) = fields.iter_mut().find(|field| field.name == binding.name) {
            field.value = binding.value.clone();
        }
    }
    true
}

fn load_fmir_text_image(text: &str, path: &Path) -> Result<FmirPackageImage, Vec<Diagnostic>> {
    let image: FmirTextImageFile = toml::from_str(text).map_err(|error| {
        vec![mir_issue_diag(
            path,
            "fmir_text_image_parse_failed",
            format!("could not parse fmir-text image: {error}"),
        )]
    })?;
    if image.version != PACKAGE_MIR_ARTIFACT_VERSION {
        return Err(vec![mir_issue_diag(
            path,
            "fmir_text_image_version_unsupported",
            format!(
                "unsupported fmir-text image version {}; expected {}",
                image.version, PACKAGE_MIR_ARTIFACT_VERSION
            ),
        )
        .with_arg("actual", image.version.to_string())
        .with_arg("expected", PACKAGE_MIR_ARTIFACT_VERSION.to_string())]);
    }
    if image.target != FMIR_TEXT_TARGET_NAME {
        return Err(vec![mir_diag(
            path,
            format!("fmir-text image target must be `{FMIR_TEXT_TARGET_NAME}`"),
        )]);
    }
    check_fmir_toolchain(&image.toolchain, "fmir-text", path)?;
    let program = serde_json::from_str(&image.program.json).map_err(|error| {
        vec![mir_diag(
            path,
            format!("could not decode fmir-text MIR program: {error}"),
        )]
    })?;
    Ok(FmirPackageImage {
        diagnostic_path: path.to_path_buf(),
        format: FmirPackageImageFormat::FmirText,
        entry_function: image.entry_function,
        runtime_requirements: image.runtime.requirement,
        interner: image.interner,
        cli: image.cli,
        exit_code: image.exit_code,
        types: image.types.table,
        program,
    })
}

fn load_fmir_image(bytes: &[u8], path: &Path) -> Result<FmirPackageImage, Vec<Diagnostic>> {
    let image: FmirBinaryImageFile = postcard::from_bytes(bytes).map_err(|error| {
        vec![mir_diag(
            path,
            format!("could not decode fmir image: {error}"),
        )]
    })?;
    if image.version != PACKAGE_MIR_ARTIFACT_VERSION {
        return Err(vec![mir_issue_diag(
            path,
            "fmir_image_version_unsupported",
            format!(
                "unsupported fmir image version {}; expected {}",
                image.version, PACKAGE_MIR_ARTIFACT_VERSION
            ),
        )
        .with_arg("actual", image.version.to_string())
        .with_arg("expected", PACKAGE_MIR_ARTIFACT_VERSION.to_string())]);
    }
    if image.target != FMIR_TARGET_NAME {
        return Err(vec![mir_diag(
            path,
            format!("fmir image target must be `{FMIR_TARGET_NAME}`"),
        )]);
    }
    check_fmir_toolchain(&image.toolchain, "fmir", path)?;
    Ok(FmirPackageImage {
        diagnostic_path: path.to_path_buf(),
        format: FmirPackageImageFormat::Fmir,
        entry_function: image.entry_function,
        runtime_requirements: image.runtime.requirement,
        interner: image.interner,
        cli: image.cli,
        exit_code: image.exit_code,
        types: image.types.table,
        program: image.program,
    })
}

fn check_fmir_toolchain(
    toolchain: &FmirTextToolchainSection,
    label: &str,
    path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    if toolchain.faber_cli_version == PACKAGE_MIR_TOOLCHAIN_VERSION {
        return Ok(());
    }
    Err(vec![mir_diag(
        path,
        format!(
            "unsupported {label} image toolchain {}; expected {}",
            toolchain.faber_cli_version, PACKAGE_MIR_TOOLCHAIN_VERSION
        ),
    )])
}

fn validate_package_mir_manifest(manifest: &str, path: &Path) -> Result<(), Vec<Diagnostic>> {
    let has_version = manifest
        .lines()
        .any(|line| line.trim() == format!("version = {PACKAGE_MIR_ARTIFACT_VERSION}"));
    let has_target = manifest
        .lines()
        .any(|line| line.trim() == format!("target = \"{PACKAGE_MIR_TARGET_NAME}\""));
    let has_entry = manifest
        .lines()
        .any(|line| line.trim_start().starts_with("entry = \""));
    let has_runtime = manifest.lines().any(|line| line.trim() == "[runtime]");
    if has_version && has_target && has_entry && has_runtime {
        return Ok(());
    }
    Err(vec![mir_issue_diag(
        path,
        "package_mir_artifact_manifest_metadata_missing",
        "package MIR artifact manifest is missing required v1 metadata",
    )])
}

fn collect_package_runtime_requirements(
    lowered: &LoweredMirUnit<'_>,
    cli_plan: &CliPackagePlan,
) -> Vec<String> {
    let mut requirements = BTreeSet::new();
    let interner = lowered.validation.interner;
    if cli_plan.uses_cli_runtime {
        requirements.insert("host:argv".to_owned());
    }
    if cli_plan.exit_code.is_some() {
        requirements.insert("host:exit".to_owned());
    }
    for function in &lowered.program.functions {
        for block in &function.blocks {
            for statement in &block.statements {
                let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                    continue;
                };
                collect_runtime_call_requirement(call, interner, &mut requirements);
            }
        }
    }
    requirements.into_iter().collect()
}

fn collect_runtime_call_requirement(
    call: &MirRuntimeCall,
    interner: Option<&Interner>,
    requirements: &mut BTreeSet<String>,
) {
    match &call.intrinsic {
        MirIntrinsic::Diagnostic(MirDiagnosticKind::Mone) => {
            requirements.insert("host:stderr".to_owned());
        }
        MirIntrinsic::Diagnostic(_) => {
            requirements.insert("host:stdout".to_owned());
        }
        MirIntrinsic::Provider(provider) => {
            if let MirProviderKind::Kernel(module) = provider.kind {
                let verb = interner
                    .map(|interner| interner.resolve(provider.name).to_owned())
                    .unwrap_or_else(|| format!("#{}", provider.name.0));
                collect_kernel_host_requirements(module, &verb, requirements);
                requirements.insert(format!("kernel:{}.{}", module.name(), verb));
            }
        }
        MirIntrinsic::Assert
        | MirIntrinsic::FormatString { .. }
        | MirIntrinsic::Convert(_)
        | MirIntrinsic::Collection(_)
        | MirIntrinsic::Atomic(_)
        | MirIntrinsic::Panic
        | MirIntrinsic::SermoOpen
        | MirIntrinsic::SermoSetOpener
        | MirIntrinsic::Sermo(_)
        | MirIntrinsic::Cede
        | MirIntrinsic::GpuBuiltin(_) => {}
        MirIntrinsic::ReadLine => {
            requirements.insert("host:stdin".to_owned());
        }
        MirIntrinsic::CursorStream(_) => {}
    }
}

fn collect_kernel_host_requirements(
    module: radix::kernel::KernelModule,
    verb: &str,
    requirements: &mut BTreeSet<String>,
) {
    match module {
        radix::kernel::KernelModule::Solum => {
            requirements.insert("host:fs".to_owned());
        }
        radix::kernel::KernelModule::Processus => match verb {
            "argumenta" => {
                requirements.insert("host:argv".to_owned());
            }
            "lege" | "scribe" => {
                requirements.insert("host:env".to_owned());
            }
            "sedes" | "muta" => {
                requirements.insert("host:cwd".to_owned());
            }
            "identitas" => {
                requirements.insert("host:pid".to_owned());
            }
            "exi" => {
                requirements.insert("host:exit".to_owned());
            }
            "exsequi" | "genera" => {
                requirements.insert("host:process".to_owned());
            }
            _ => {}
        },
        radix::kernel::KernelModule::Aleator => {
            requirements.insert("host:random".to_owned());
        }
        radix::kernel::KernelModule::Json => {}
        radix::kernel::KernelModule::Consolum => match verb {
            "dic" | "scribe" => {
                requirements.insert("host:stdout".to_owned());
            }
            "mone" => {
                requirements.insert("host:stderr".to_owned());
            }
            _ => {}
        },
    }
}

fn select_entry_unit(package: &AnalyzedPackage) -> Result<usize, Vec<Diagnostic>> {
    let entries = package
        .units
        .iter()
        .enumerate()
        .filter_map(|(index, unit)| unit.is_entry.then_some(index))
        .collect::<Vec<_>>();
    match entries.as_slice() {
        [index] => Ok(*index),
        [] => Err(vec![crate::package_diagnostic_error(
            "package MIR run requires exactly one entry unit",
        )
        .with_file(package.spec.entry.display().to_string())]),
        _ => Err(vec![crate::package_diagnostic_error(
            "package MIR run found multiple entry units",
        )
        .with_file(package.spec.entry.display().to_string())]),
    }
}

fn library_import_diagnostics(package: &AnalyzedPackage) -> Option<Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    for unit in &package.units {
        // Allow `norma:<kernel-manifest-module>` through: the post-lowering
        // bridge satisfies those via the in-process stepper kernel. Everything
        // else (e.g. `norma:chorda`, external libs) still fails closed here.
        let mut imports = unit
            .analysis
            .libraries
            .bindings
            .values()
            .map(|binding| &binding.identity)
            .filter(|identity| !is_bridged_norma_module(identity))
            .map(library_identity_label)
            .collect::<BTreeSet<_>>();
        diagnostics.extend(imports.pop_first().map(|first| {
            crate::package_diagnostic_error(format!(
                "package MIR does not yet support library imports such as `{first}`; use compiled package execution for this surface"
            ))
            .with_file(unit.path.display().to_string())
            .with_arg("issue", "package_mir_library_imports_unsupported")
            .with_arg("import", first)
        }));
    }
    (!diagnostics.is_empty()).then_some(diagnostics)
}

/// Whether `identity` is a `norma:<kernel-manifest-module>` import that the
/// interpreted-package bridge can satisfy through the stepper kernel.
fn is_bridged_norma_module(identity: &radix::hir::LibraryIdentity) -> bool {
    let radix::hir::LibraryProvider::Builtin(name) = &identity.provider else {
        return false;
    };
    name == "norma" && is_bridged_norma_import_path(&library_identity_label(identity))
}

/// Whether an import path string (`norma:solum`) names a kernel-manifest
/// module the interpreted-package bridge can satisfy. Shared by the
/// library-import allowlist (identity-based) and the namespace-link pass
/// (path-based) so the two rejection sites agree.
fn is_bridged_norma_import_path(path: &str) -> bool {
    path.strip_prefix("norma:")
        .and_then(radix::kernel::resolve_kernel_module_name)
        .is_some()
}

fn library_identity_label(identity: &radix::hir::LibraryIdentity) -> String {
    let provider = match &identity.provider {
        LibraryProvider::Builtin(name) | LibraryProvider::Package(name) => name.as_str(),
    };
    format!("{provider}:{}", identity.module_path.join("/"))
}

/// Bridge interpreted `norma:<kernel-manifest-module>` providers to the
/// in-process stepper kernel.
///
/// Post-validation transform (see
/// `docs/factory/faber-script-runtime/stage1b-package-host-bridge.md`). For
/// each `Package` provider whose module resolves to a kernel-manifest module,
/// rewrite `kind` to `Kernel(module)` when the called verb is in the manifest
/// subset; otherwise fail closed with an actionable diagnostic. Compiled
/// package execution is unaffected (it never runs this path).
///
/// RETIRE: delete this pass once core-stdlib Stage 8 routes `norma:*` over
/// `ad` to the Rust frame runtime in the stepper.
fn bridge_norma_providers_to_kernel(
    lowered: &mut LoweredMirUnit,
    entry_path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let Some(interner) = lowered.validation.interner else {
        return Err(vec![mir_diag(
            entry_path,
            "package MIR kernel bridge requires interner context",
        )]);
    };
    let mut diagnostics = Vec::new();
    for function in &mut lowered.program.functions {
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                let MirStatementKind::RuntimeCall { call, .. } = &mut statement.kind else {
                    continue;
                };
                let MirRuntimeCall {
                    intrinsic: MirIntrinsic::Provider(provider),
                    ..
                } = call
                else {
                    continue;
                };
                if !matches!(provider.kind, MirProviderKind::Package) {
                    continue;
                }
                let Some(path_symbol) = provider.module.first() else {
                    continue;
                };
                let Some(norma_module) = interner.resolve(*path_symbol).strip_prefix("norma:")
                else {
                    continue;
                };
                let Some(module) = radix::kernel::resolve_kernel_module_name(norma_module) else {
                    // Non-manifest `norma:*` should have been rejected by
                    // `library_import_diagnostics`; leave as Package and let the
                    // stepper's `host.provider()` report it unsupported.
                    continue;
                };
                let verb = interner.resolve(provider.name);
                if radix::kernel::kernel_module_supports_verb(module, verb) {
                    provider.kind = MirProviderKind::Kernel(module);
                } else {
                    diagnostics.push(mir_diag(
                        entry_path,
                        format!(
                            "package MIR kernel bridge does not support `norma:{norma_module}.{verb}`; use compiled package execution for this surface"
                        ),
                    ));
                }
            }
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn plan_cli_package(
    package: &mut AnalyzedPackage,
    argumenta: &[String],
    mode: CliPlanningMode,
) -> Result<CliPackagePlan, Vec<Diagnostic>> {
    let mut plan = CliPackagePlan::default();
    let mut diagnostics = Vec::new();
    let Some(entry_index) = package.units.iter().position(|unit| unit.is_entry) else {
        return Ok(plan);
    };
    let Some(cli_program) = package.units[entry_index].analysis.cli_program.clone() else {
        return Ok(plan);
    };
    plan.uses_cli_runtime = true;

    match cli_program.mode {
        CliMode::SingleCommand => {
            if mode == CliPlanningMode::FmirTextRuntime {
                if let Some((records, cli_section, exit_code)) =
                    plan_fmir_text_runtime_cli_root_entry(
                        &mut package.units[entry_index],
                        &cli_program,
                        &mut diagnostics,
                    )
                {
                    plan.exit_code = exit_code;
                    plan.entry_records
                        .insert(package.units[entry_index].path.clone(), records);
                    plan.fmir_text_cli = Some(cli_section);
                }
            } else if let Some((records, exit_code)) = plan_cli_root_entry(
                &mut package.units[entry_index],
                &cli_program,
                argumenta,
                &mut diagnostics,
            ) {
                plan.exit_code = exit_code;
                plan.entry_records
                    .insert(package.units[entry_index].path.clone(), records);
            }
        }
        CliMode::Subcommand => {
            if mode == CliPlanningMode::FmirTextRuntime {
                diagnostics.push(unsupported_cli_diagnostic(
                    &package.spec.entry,
                    "fmir-text runtime CLI subcommand dispatch",
                ));
                return Err(diagnostics);
            }
            plan.dispatch = plan_cli_subcommand(
                package,
                &cli_program,
                argumenta,
                &mut plan.entry_records,
                &mut diagnostics,
            );
        }
        CliMode::NotCli => {}
    }

    if diagnostics.is_empty() {
        Ok(plan)
    } else {
        Err(diagnostics)
    }
}

fn plan_fmir_text_runtime_cli_root_entry(
    unit: &mut AnalyzedPackageUnit,
    cli_program: &CliProgram,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(CliRecordFieldsByLocal, FmirTextCliSection, Option<i32>)> {
    let diagnostic_count = diagnostics.len();
    if !cli_program.commands.is_empty() {
        diagnostics.push(unsupported_cli_diagnostic(
            &unit.path,
            "fmir-text runtime CLI subcommand dispatch",
        ));
    }
    if cli_program
        .global_options
        .iter()
        .chain(&cli_program.options)
        .next()
        .is_some()
    {
        diagnostics.push(unsupported_cli_diagnostic(
            &unit.path,
            "fmir-text runtime CLI options",
        ));
    }
    let exit_code = package_mir_cli_exit_code(&cli_program.exit, None, &[], unit, diagnostics)?;
    let operands = cli_program
        .global_operands
        .iter()
        .chain(&cli_program.operands)
        .collect::<Vec<_>>();
    if operands.iter().any(|operand| {
        operand.rest
            || operand.default.is_some()
            || matches!(
                operand.ty,
                CliType::Octeti | CliType::ListaTextus | CliType::ListaNumerus
            )
    }) {
        diagnostics.push(unsupported_cli_diagnostic(
            &unit.path,
            "fmir-text runtime CLI operands beyond required scalar values",
        ));
    }
    if diagnostics.len() != diagnostic_count {
        return None;
    }

    let Some(args_name) = unit.analysis.hir.entry_args_name else {
        if operands.is_empty() {
            return Some((
                HashMap::new(),
                FmirTextCliSection {
                    root: FmirTextCliRootSection {
                        record: String::new(),
                        operand: Vec::new(),
                    },
                },
                exit_code,
            ));
        }
        diagnostics.push(unsupported_cli_diagnostic(
            &unit.path,
            "CLI argument records",
        ));
        return None;
    };

    let planned = planned_cli_operands(&unit.analysis.interner, operands.into_iter());
    let mut fields = Vec::new();
    let mut image_operands = Vec::new();
    for operand in planned {
        let ty = fmir_text_cli_value_type(&operand.operand.ty)?;
        fields.push(MirRuntimeRecordField {
            name: unit.analysis.interner.intern(&operand.binding_name),
            value: MirRuntimeRecordValue::Operand(fmir_text_cli_placeholder_value(unit, &ty)),
        });
        image_operands.push(FmirTextCliOperand {
            field: operand.binding_name,
            ty,
        });
    }
    let record = unit.analysis.interner.resolve(args_name).to_owned();
    Some((
        HashMap::from([(args_name, fields)]),
        FmirTextCliSection {
            root: FmirTextCliRootSection {
                record,
                operand: image_operands,
            },
        },
        exit_code,
    ))
}

fn fmir_text_cli_value_type(ty: &CliType) -> Option<FmirTextCliValueType> {
    match ty {
        CliType::Textus | CliType::Ignotum => Some(FmirTextCliValueType::Textus),
        CliType::Numerus => Some(FmirTextCliValueType::Numerus),
        CliType::Fractus => Some(FmirTextCliValueType::Fractus),
        CliType::Bivalens => Some(FmirTextCliValueType::Bivalens),
        CliType::Octeti | CliType::ListaTextus | CliType::ListaNumerus => None,
    }
}

fn fmir_text_cli_placeholder_value(
    unit: &mut AnalyzedPackageUnit,
    ty: &FmirTextCliValueType,
) -> MirOperand {
    let constant = match ty {
        FmirTextCliValueType::Textus => {
            MirConstant::String(unit.analysis.interner.intern("__fmir_runtime_arg__"))
        }
        FmirTextCliValueType::Numerus => MirConstant::Int(0),
        FmirTextCliValueType::Fractus => MirConstant::Float(0.0),
        FmirTextCliValueType::Bivalens => MirConstant::Bool(false),
    };
    MirOperand::Constant(constant)
}

fn plan_cli_root_entry(
    unit: &mut AnalyzedPackageUnit,
    cli_program: &CliProgram,
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(CliRecordFieldsByLocal, Option<i32>)> {
    let diagnostic_count = diagnostics.len();
    if !cli_program.commands.is_empty() {
        diagnostics.push(unsupported_cli_diagnostic(
            &unit.path,
            "CLI subcommand dispatch",
        ));
    }
    if cli_program
        .global_options
        .iter()
        .chain(&cli_program.options)
        .any(|option| !is_package_mir_supported_option(option))
    {
        diagnostics.push(unsupported_cli_diagnostic(
            &unit.path,
            "CLI options beyond root boolean flags and scalar values",
        ));
    }
    let operands = cli_program
        .global_operands
        .iter()
        .chain(&cli_program.operands)
        .collect::<Vec<_>>();
    if has_unsupported_package_mir_operands(operands.iter().copied()) {
        diagnostics.push(unsupported_cli_diagnostic(
            &unit.path,
            "CLI operands beyond supported positional values",
        ));
    }
    if diagnostics.len() != diagnostic_count {
        return None;
    }
    let options = planned_cli_options(
        &unit.analysis.interner,
        cli_program
            .global_options
            .iter()
            .chain(&cli_program.options),
    );
    let parsed = parse_cli_arguments(unit, &options, argumenta, diagnostics)?;
    let operands = planned_cli_operands(
        &unit.analysis.interner,
        cli_program
            .global_operands
            .iter()
            .chain(&cli_program.operands),
    );
    let mut fields = parsed.option_fields;
    fields.extend(cli_operand_record_fields(
        unit,
        &operands,
        &parsed.positionals,
        diagnostics,
    )?);
    let Some(args_name) = unit.analysis.hir.entry_args_name else {
        if !fields.is_empty() {
            diagnostics.push(unsupported_cli_diagnostic(
                &unit.path,
                "CLI argument records",
            ));
            return None;
        }
        let exit_code = package_mir_cli_exit_code(&cli_program.exit, None, &[], unit, diagnostics)?;
        return Some((HashMap::new(), exit_code));
    };
    let exit_code = package_mir_cli_exit_code(
        &cli_program.exit,
        Some(args_name),
        &fields,
        unit,
        diagnostics,
    )?;
    Some((HashMap::from([(args_name, fields)]), exit_code))
}

struct ParsedCliArguments {
    option_fields: Vec<MirRuntimeRecordField>,
    positionals: Vec<String>,
    consumed: usize,
}

fn planned_cli_options<'a>(
    interner: &Interner,
    options: impl Iterator<Item = &'a CliOption>,
) -> Vec<PlannedCliOption<'a>> {
    options
        .map(|option| PlannedCliOption {
            option,
            binding_name: interner.resolve(option.binding_symbol).to_owned(),
        })
        .collect()
}

fn planned_cli_operands<'a>(
    interner: &Interner,
    operands: impl Iterator<Item = &'a CliOperand>,
) -> Vec<PlannedCliOperand<'a>> {
    operands
        .map(|operand| PlannedCliOperand {
            operand,
            binding_name: interner.resolve(operand.binding_symbol).to_owned(),
        })
        .collect()
}

fn parse_cli_arguments(
    unit: &mut AnalyzedPackageUnit,
    options: &[PlannedCliOption<'_>],
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ParsedCliArguments> {
    parse_cli_arguments_with_mode(unit, options, argumenta, diagnostics, false)
}

fn parse_leading_cli_options(
    unit: &mut AnalyzedPackageUnit,
    options: &[PlannedCliOption<'_>],
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ParsedCliArguments> {
    parse_cli_arguments_with_mode(unit, options, argumenta, diagnostics, true)
}

fn parse_cli_arguments_with_mode(
    unit: &mut AnalyzedPackageUnit,
    options: &[PlannedCliOption<'_>],
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
    stop_at_first_positional: bool,
) -> Option<ParsedCliArguments> {
    if options.is_empty() {
        return Some(ParsedCliArguments {
            option_fields: Vec::new(),
            positionals: argumenta.to_vec(),
            consumed: if stop_at_first_positional {
                0
            } else {
                argumenta.len()
            },
        });
    }

    let mut option_fields = options
        .iter()
        .map(|option| cli_option_default_field(unit, option, diagnostics))
        .collect::<Option<Vec<_>>>()?;
    let mut positionals = Vec::new();

    let mut argument_index = 0;
    while argument_index < argumenta.len() {
        let argument = &argumenta[argument_index];
        if let Some(name) = argument.strip_prefix("--") {
            let (name, inline_value) = name
                .split_once('=')
                .map(|(name, value)| (name, Some(value.to_owned())))
                .unwrap_or((name, None));
            let Some(option_index) = options
                .iter()
                .position(|option| option.option.long.as_deref() == Some(name))
            else {
                push_cli_option_match_diagnostic(unit, argument, diagnostics);
                return None;
            };
            let option = &options[option_index];
            if option.option.flag {
                option_fields[option_index].value =
                    MirRuntimeRecordValue::Operand(MirOperand::Constant(MirConstant::Bool(true)));
                argument_index += 1;
                continue;
            }
            let raw = match inline_value {
                Some(value) => value,
                None => {
                    argument_index += 1;
                    let Some(value) = argumenta.get(argument_index) else {
                        push_cli_option_missing_value_diagnostic(unit, argument, diagnostics);
                        return None;
                    };
                    value.clone()
                }
            };
            option_fields[option_index].value =
                MirRuntimeRecordValue::Operand(cli_option_value(unit, option, &raw, diagnostics)?);
            argument_index += 1;
            continue;
        }
        if let Some(name) = argument.strip_prefix('-') {
            if !name.is_empty() {
                let Some(option_index) = options
                    .iter()
                    .position(|option| option.option.short.as_deref() == Some(name))
                else {
                    push_cli_option_match_diagnostic(unit, argument, diagnostics);
                    return None;
                };
                let option = &options[option_index];
                if option.option.flag {
                    option_fields[option_index].value = MirRuntimeRecordValue::Operand(
                        MirOperand::Constant(MirConstant::Bool(true)),
                    );
                    argument_index += 1;
                    continue;
                }
                argument_index += 1;
                let Some(raw) = argumenta.get(argument_index) else {
                    push_cli_option_missing_value_diagnostic(unit, argument, diagnostics);
                    return None;
                };
                option_fields[option_index].value = MirRuntimeRecordValue::Operand(
                    cli_option_value(unit, option, raw, diagnostics)?,
                );
                argument_index += 1;
                continue;
            }
        }
        if stop_at_first_positional {
            return Some(ParsedCliArguments {
                option_fields,
                positionals: argumenta[argument_index..].to_vec(),
                consumed: argument_index,
            });
        }
        positionals.push(argument.clone());
        argument_index += 1;
    }

    Some(ParsedCliArguments {
        option_fields,
        positionals,
        consumed: argumenta.len(),
    })
}

fn cli_option_default_field(
    unit: &mut AnalyzedPackageUnit,
    option: &PlannedCliOption<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MirRuntimeRecordField> {
    let value = if option.option.flag {
        let value = match &option.option.default {
            Some(CliDefault::Bool(value)) => *value,
            _ => false,
        };
        MirOperand::Constant(MirConstant::Bool(value))
    } else {
        let Some(default) = &option.option.default else {
            return Some(MirRuntimeRecordField {
                name: unit.analysis.interner.intern(&option.binding_name),
                value: MirRuntimeRecordValue::Operand(MirOperand::Constant(MirConstant::Nil)),
            });
        };
        cli_default_value(unit, &option.option.ty, default).or_else(|| {
            push_cli_option_default_diagnostic(unit, option, diagnostics);
            None
        })?
    };
    Some(MirRuntimeRecordField {
        name: unit.analysis.interner.intern(&option.binding_name),
        value: MirRuntimeRecordValue::Operand(value),
    })
}

fn push_cli_option_match_diagnostic(
    unit: &AnalyzedPackageUnit,
    option: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR could not match CLI option `{option}`; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string()),
    );
}

fn push_cli_option_missing_value_diagnostic(
    unit: &AnalyzedPackageUnit,
    option: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR expected a value for CLI option `{option}`; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string()),
    );
}

fn plan_cli_subcommand(
    package: &mut AnalyzedPackage,
    cli_program: &CliProgram,
    argumenta: &[String],
    entry_records: &mut CliEntryRecords,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CliDispatchPlan> {
    let entry_path = package.spec.entry.clone();
    let diagnostic_count = diagnostics.len();
    if cli_program
        .global_options
        .iter()
        .any(|option| !is_package_mir_supported_option(option))
    {
        diagnostics.push(unsupported_cli_diagnostic(
            &entry_path,
            "CLI global options beyond boolean flags and scalar values",
        ));
    }
    if !cli_program.options.is_empty() {
        diagnostics.push(unsupported_cli_diagnostic(
            &entry_path,
            "root-local CLI options in subcommand mode",
        ));
    }
    if cli_program.commands.iter().any(|command| {
        has_unsupported_package_mir_operands(
            cli_program
                .global_operands
                .iter()
                .chain(command.operands.iter()),
        )
    }) {
        diagnostics.push(unsupported_cli_diagnostic(
            &entry_path,
            "CLI command operands beyond supported positional values",
        ));
    }
    if !cli_program.operands.is_empty() {
        diagnostics.push(unsupported_cli_diagnostic(
            &entry_path,
            "root-local CLI operands in subcommand mode",
        ));
    }
    if cli_program.exit.is_some() {
        diagnostics.push(unsupported_cli_diagnostic(
            &entry_path,
            "CLI exit expressions",
        ));
    }
    if cli_program
        .commands
        .iter()
        .any(command_has_unsupported_options)
    {
        diagnostics.push(unsupported_cli_diagnostic(
            &entry_path,
            "CLI command options beyond boolean flags and scalar values",
        ));
    }
    if diagnostics.len() != diagnostic_count {
        return None;
    }

    let entry_index = package.units.iter().position(|unit| unit.is_entry)?;
    let global_options = planned_cli_options(
        &package.units[entry_index].analysis.interner,
        cli_program.global_options.iter(),
    );
    let global_operands = planned_cli_operands(
        &package.units[entry_index].analysis.interner,
        cli_program.global_operands.iter(),
    );
    let parsed_globals = parse_leading_cli_options(
        &mut package.units[entry_index],
        &global_options,
        argumenta,
        diagnostics,
    )?;
    let command_argumenta = parsed_globals.positionals;

    let Some(command_match) = matching_cli_command(&cli_program.commands, &command_argumenta)
    else {
        diagnostics.push(
            crate::package_diagnostic_error(format!(
                "package MIR could not match CLI command `{}`; use compiled package execution for this surface",
                command_argumenta.join(" ")
            ))
            .with_file(entry_path.display().to_string()),
        );
        return None;
    };
    let command_args = &command_argumenta[command_match.consumed..];
    if let Some(args_name) = package.units[entry_index].analysis.hir.entry_args_name {
        let mut entry_fields = parsed_globals.option_fields.clone();
        let global_operand_args_len = cli_operand_consumed_len(&global_operands, command_args);
        entry_fields.extend(cli_operand_record_fields(
            &mut package.units[entry_index],
            &global_operands,
            &command_args[..global_operand_args_len],
            diagnostics,
        )?);
        if !entry_fields.is_empty() {
            entry_records
                .entry(package.units[entry_index].path.clone())
                .or_default()
                .insert(args_name, entry_fields);
        }
    }
    let command = command_match.command;
    let Some(unit_index) = command_unit_index(package, command) else {
        diagnostics.push(
            crate::package_diagnostic_error(format!(
                "package MIR could not resolve CLI command module for `{}`",
                command.path.join(" ")
            ))
            .with_file(entry_path.display().to_string()),
        );
        return None;
    };
    let target_command = command_in_unit(&package.units[unit_index], command)
        .cloned()
        .unwrap_or_else(|| command.clone());
    let mut record_type_rewrite = selected_command_record_type_rewrite(
        package,
        unit_index,
        &target_command,
        &global_options,
        &global_operands,
    );
    let unit = &mut package.units[unit_index];
    let unit_path = unit.path.clone();
    let global_fields = if unit_index == entry_index {
        parsed_globals.option_fields
    } else {
        let global_argumenta = &argumenta[..parsed_globals.consumed];
        parse_cli_arguments(unit, &global_options, global_argumenta, diagnostics)?.option_fields
    };
    let record_fields = plan_cli_command_records(
        unit,
        &target_command,
        command_args,
        global_fields,
        &global_operands,
        diagnostics,
    )?;
    if let (Some(rewrite), Some(fields)) = (&mut record_type_rewrite, &record_fields) {
        add_cli_runtime_field_type_rewrites(package, unit_index, entry_index, rewrite, fields);
    }
    if let Some(fields) = record_fields {
        entry_records
            .entry(unit_path.clone())
            .or_default()
            .extend(fields);
    }

    Some(CliDispatchPlan {
        unit_path,
        function: target_command.function_symbol,
        record_type_rewrite,
    })
}

fn cli_operand_consumed_len(operands: &[PlannedCliOperand<'_>], argumenta: &[String]) -> usize {
    let mut consumed = 0;
    for operand in operands {
        if cli_operand_consumes_many(operand.operand) {
            return argumenta.len();
        }
        if consumed < argumenta.len() {
            consumed += 1;
        }
    }
    consumed
}

fn selected_command_record_type_rewrite(
    package: &mut AnalyzedPackage,
    unit_index: usize,
    command: &CliCommand,
    global_options: &[PlannedCliOption<'_>],
    global_operands: &[PlannedCliOperand<'_>],
) -> Option<CliRecordTypeRewrite> {
    let entry_index = package.units.iter().position(|unit| unit.is_entry)?;
    if entry_index == unit_index {
        return None;
    }
    let from = command_cli_args_type(&package.units[unit_index], command)?;
    let field_names = global_options
        .iter()
        .map(|option| {
            (
                option.binding_name.clone(),
                option.option.ty.clone(),
                false,
                cli_option_is_nullable(option.option),
            )
        })
        .chain(command.options.iter().map(|option| {
            let name = package.units[unit_index]
                .analysis
                .interner
                .resolve(option.binding_symbol)
                .to_owned();
            (
                name,
                option.ty.clone(),
                false,
                cli_option_is_nullable(option),
            )
        }))
        .chain(global_operands.iter().map(|operand| {
            (
                operand.binding_name.clone(),
                operand.operand.ty.clone(),
                operand.operand.rest,
                false,
            )
        }))
        .chain(command.operands.iter().map(|operand| {
            let name = package.units[unit_index]
                .analysis
                .interner
                .resolve(operand.binding_symbol)
                .to_owned();
            (name, operand.ty.clone(), operand.rest, false)
        }))
        .collect::<Vec<_>>();
    let entry = &mut package.units[entry_index];
    let mut fields = Vec::new();
    for (name, ty, rest, nullable) in field_names {
        let symbol = entry.analysis.interner.intern(&name);
        let mut ty = cli_record_type(&mut entry.analysis.types, &ty, rest)?;
        if nullable {
            ty = entry.analysis.types.option(ty);
        }
        fields.push((symbol, ty));
    }
    let to = entry.analysis.types.record(fields);
    Some(CliRecordTypeRewrite {
        types: vec![(from, to)],
    })
}

fn add_cli_runtime_field_type_rewrites(
    package: &mut AnalyzedPackage,
    unit_index: usize,
    entry_index: usize,
    rewrite: &mut CliRecordTypeRewrite,
    fields_by_local: &CliRecordFieldsByLocal,
) {
    if unit_index == entry_index {
        return;
    }
    let mut imported = HashMap::new();
    if unit_index < entry_index {
        let (before_entry, entry_and_after) = package.units.split_at_mut(entry_index);
        let source_types = &before_entry[unit_index].analysis.types;
        let target_types = &mut entry_and_after[0].analysis.types;
        add_runtime_field_type_rewrites(
            source_types,
            target_types,
            rewrite,
            fields_by_local,
            &mut imported,
        );
    } else {
        let (before_source, source_and_after) = package.units.split_at_mut(unit_index);
        let target_types = &mut before_source[entry_index].analysis.types;
        let source_types = &source_and_after[0].analysis.types;
        add_runtime_field_type_rewrites(
            source_types,
            target_types,
            rewrite,
            fields_by_local,
            &mut imported,
        );
    }
}

fn add_runtime_field_type_rewrites(
    source_types: &TypeTable,
    target_types: &mut TypeTable,
    rewrite: &mut CliRecordTypeRewrite,
    fields_by_local: &CliRecordFieldsByLocal,
    imported: &mut HashMap<TypeId, TypeId>,
) {
    for fields in fields_by_local.values() {
        for field in fields {
            if let MirRuntimeRecordValue::Array { ty, .. } = &field.value {
                let source = ty.semantic_id();
                let target = import_semantic_type(source_types, target_types, source, imported);
                push_type_rewrite(&mut rewrite.types, source, target);
            }
        }
    }
}

fn push_type_rewrite(rewrites: &mut Vec<(TypeId, TypeId)>, source: TypeId, target: TypeId) {
    if source == target || rewrites.iter().any(|(existing, _)| *existing == source) {
        return;
    }
    rewrites.push((source, target));
}

fn import_semantic_type(
    source: &TypeTable,
    target: &mut TypeTable,
    ty: TypeId,
    imported: &mut HashMap<TypeId, TypeId>,
) -> TypeId {
    if let Some(existing) = imported.get(&ty).copied() {
        return existing;
    }
    let imported_ty = match source.get(ty).clone() {
        Type::Primitive(primitive) => target.primitive(primitive),
        Type::Array(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target
                .find_array(inner)
                .unwrap_or_else(|| target.array(inner))
        }
        Type::Map(key, value) => {
            let key = import_semantic_type(source, target, key, imported);
            let value = import_semantic_type(source, target, value, imported);
            target.map(key, value)
        }
        Type::Record(fields) => {
            let fields = fields
                .into_iter()
                .map(|(name, field_ty)| {
                    (
                        name,
                        import_semantic_type(source, target, field_ty, imported),
                    )
                })
                .collect();
            target.intern(Type::Record(fields))
        }
        Type::Set(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.set(inner)
        }
        Type::Promissum(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.promissum(inner)
        }
        Type::Cursor(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.cursor(inner)
        }
        Type::Tensor(inner, shape) => {
            let inner = import_semantic_type(source, target, inner, imported);
            let shape = import_index_expr(source, target, shape);
            target.tensor_with_shape(inner, shape)
        }
        Type::Vector(inner, width) => {
            let inner = import_semantic_type(source, target, inner, imported);
            let width = import_index_expr(source, target, width);
            target.vector_with_width(inner, width)
        }
        Type::Matrix(inner, shape) => {
            let inner = import_semantic_type(source, target, inner, imported);
            let shape = import_index_expr(source, target, shape);
            target.matrix_with_shape(inner, shape)
        }
        Type::Sparsa(inner, shape) => {
            let inner = import_semantic_type(source, target, inner, imported);
            let shape = import_index_expr(source, target, shape);
            target.sparsa_with_shape(inner, shape)
        }
        Type::Atomic(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.atomic(inner)
        }
        Type::Intervallum(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.intern(Type::Intervallum(inner))
        }
        Type::SizedNumeric(primitive, width) => target.sized_numeric(primitive, width),
        Type::ModularWord(width) => target.intern(Type::ModularWord(width)),
        Type::SizedInstans(precision) => target.intern(Type::SizedInstans(precision)),
        Type::Option(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.option(inner)
        }
        Type::Ref(mutability, inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.reference(mutability, inner)
        }
        Type::Alias(def_id, inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.intern(Type::Alias(def_id, inner))
        }
        Type::Func(mut sig) => {
            for param in &mut sig.params {
                param.ty = import_semantic_type(source, target, param.ty, imported);
            }
            sig.ret = import_semantic_type(source, target, sig.ret, imported);
            if let Some(error_ty) = sig.err {
                sig.err = Some(import_semantic_type(source, target, error_ty, imported));
            }
            target.function(sig)
        }
        Type::Applied(base, args) => {
            let base = import_semantic_type(source, target, base, imported);
            let args = args
                .into_iter()
                .map(|arg| import_semantic_type(source, target, arg, imported))
                .collect();
            target.intern(Type::Applied(base, args))
        }
        Type::Union(members) => {
            let members = members
                .into_iter()
                .map(|member| import_semantic_type(source, target, member, imported))
                .collect();
            target.intern(Type::Union(members))
        }
        other @ (Type::Struct(_)
        | Type::Enum(_)
        | Type::Interface(_)
        | Type::Param(_)
        | Type::Infer(_)
        | Type::Error) => target.intern(other),
    };
    imported.insert(ty, imported_ty);
    imported_ty
}

fn import_index_expr(
    source: &TypeTable,
    target: &mut TypeTable,
    index: radix::semantic::IndexId,
) -> radix::semantic::IndexId {
    match source.get_index(index).clone() {
        IndexExpr::Tuple(items) => {
            let items = items
                .into_iter()
                .map(|item| import_index_expr(source, target, item))
                .collect();
            target.intern_index(IndexExpr::Tuple(items))
        }
        other => target.intern_index(other),
    }
}

fn command_cli_args_type(unit: &AnalyzedPackageUnit, command: &CliCommand) -> Option<TypeId> {
    unit.analysis.hir.items.iter().find_map(|item| {
        let HirItemKind::Function(function) = &item.kind else {
            return None;
        };
        (function.name == command.function_symbol)
            .then(|| function.cli_args.as_ref().map(|param| param.ty))
            .flatten()
    })
}

fn plan_cli_command_records(
    unit: &mut AnalyzedPackageUnit,
    command: &CliCommand,
    argumenta: &[String],
    global_fields: Vec<MirRuntimeRecordField>,
    global_operands: &[PlannedCliOperand<'_>],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Option<CliRecordFieldsByLocal>> {
    let mut operands = global_operands.to_vec();
    operands.extend(planned_cli_operands(
        &unit.analysis.interner,
        command.operands.iter(),
    ));
    let options = planned_cli_options(&unit.analysis.interner, command.options.iter());
    let parsed = parse_cli_arguments(unit, &options, argumenta, diagnostics)?;
    let mut fields = global_fields;
    fields.extend(parsed.option_fields);
    fields.extend(cli_operand_record_fields(
        unit,
        &operands,
        &parsed.positionals,
        diagnostics,
    )?);
    let Some(args_binding) = &command.args_binding else {
        if !fields.is_empty() {
            diagnostics.push(unsupported_cli_diagnostic(
                &unit.path,
                "CLI argument records",
            ));
            return None;
        }
        return Some(None);
    };
    let args_symbol = unit.analysis.interner.intern(args_binding);
    Some(Some(HashMap::from([(args_symbol, fields)])))
}

fn cli_operand_record_fields(
    unit: &mut AnalyzedPackageUnit,
    operands: &[PlannedCliOperand<'_>],
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<MirRuntimeRecordField>> {
    let mut index = 0;
    let mut fields = Vec::new();
    for operand in operands {
        let value = if cli_operand_consumes_many(operand.operand) {
            let raw = argumenta[index..].iter().collect::<Vec<_>>();
            index = argumenta.len();
            cli_operand_list_value(unit, operand, raw, diagnostics)?
        } else if let Some(value) = argumenta.get(index) {
            index += 1;
            MirRuntimeRecordValue::Operand(cli_operand_value(unit, operand, value, diagnostics)?)
        } else if let Some(default) = &operand.operand.default {
            MirRuntimeRecordValue::Operand(cli_default_operand_value(
                unit,
                operand,
                default,
                diagnostics,
            )?)
        } else {
            push_cli_operand_missing_diagnostic(unit, operand, diagnostics);
            return None;
        };
        fields.push(MirRuntimeRecordField {
            name: unit.analysis.interner.intern(&operand.binding_name),
            value,
        });
    }
    if argumenta.get(index).is_some() {
        diagnostics.push(unsupported_cli_diagnostic(
            &unit.path,
            "CLI argument parsing",
        ));
        return None;
    }
    Some(fields)
}

fn cli_operand_consumes_many(operand: &CliOperand) -> bool {
    operand.rest || matches!(operand.ty, CliType::ListaTextus | CliType::ListaNumerus)
}

fn cli_operand_value(
    unit: &mut AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    value: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MirOperand> {
    let constant = match &operand.operand.ty {
        CliType::Textus | CliType::Ignotum => {
            let payload = textus_literal_payload(value);
            MirConstant::String(unit.analysis.interner.intern(&payload))
        }
        CliType::Numerus => match value.parse::<i64>() {
            Ok(value) => MirConstant::Int(value),
            Err(_) => {
                push_cli_operand_parse_diagnostic(unit, operand, "numerus", value, diagnostics);
                return None;
            }
        },
        CliType::Fractus => match value.parse::<f64>() {
            Ok(value) => MirConstant::Float(value),
            Err(_) => {
                push_cli_operand_parse_diagnostic(unit, operand, "fractus", value, diagnostics);
                return None;
            }
        },
        CliType::Bivalens => match value.parse::<bool>() {
            Ok(value) => MirConstant::Bool(value),
            Err(_) => {
                push_cli_operand_parse_diagnostic(unit, operand, "bivalens", value, diagnostics);
                return None;
            }
        },
        CliType::Octeti => MirConstant::Octeti(value.as_bytes().to_vec()),
        CliType::ListaTextus | CliType::ListaNumerus => return None,
    };
    Some(MirOperand::Constant(constant))
}

fn cli_operand_list_value(
    unit: &mut AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    values: Vec<&String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MirRuntimeRecordValue> {
    let ty = cli_record_type(
        &mut unit.analysis.types,
        &operand.operand.ty,
        operand.operand.rest,
    )?;
    let items = values
        .into_iter()
        .map(|value| cli_operand_list_item_value(unit, operand, value, diagnostics))
        .collect::<Option<Vec<_>>>()?;
    Some(MirRuntimeRecordValue::Array {
        ty: MirType::semantic(ty),
        items,
    })
}

fn cli_operand_list_item_value(
    unit: &mut AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    value: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MirOperand> {
    match &operand.operand.ty {
        CliType::Textus | CliType::Ignotum | CliType::ListaTextus => {
            let payload = textus_literal_payload(value);
            Some(MirOperand::Constant(MirConstant::String(
                unit.analysis.interner.intern(&payload),
            )))
        }
        CliType::Numerus | CliType::ListaNumerus => match value.parse::<i64>() {
            Ok(value) => Some(MirOperand::Constant(MirConstant::Int(value))),
            Err(_) => {
                push_cli_operand_parse_diagnostic(unit, operand, "numerus", value, diagnostics);
                None
            }
        },
        CliType::Fractus => match value.parse::<f64>() {
            Ok(value) => Some(MirOperand::Constant(MirConstant::Float(value))),
            Err(_) => {
                push_cli_operand_parse_diagnostic(unit, operand, "fractus", value, diagnostics);
                None
            }
        },
        CliType::Bivalens => match value.parse::<bool>() {
            Ok(value) => Some(MirOperand::Constant(MirConstant::Bool(value))),
            Err(_) => {
                push_cli_operand_parse_diagnostic(unit, operand, "bivalens", value, diagnostics);
                None
            }
        },
        CliType::Octeti => None,
    }
}

fn cli_option_value(
    unit: &mut AnalyzedPackageUnit,
    option: &PlannedCliOption<'_>,
    value: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MirOperand> {
    let constant = match &option.option.ty {
        CliType::Textus | CliType::Ignotum => {
            let payload = textus_literal_payload(value);
            MirConstant::String(unit.analysis.interner.intern(&payload))
        }
        CliType::Numerus => match value.parse::<i64>() {
            Ok(value) => MirConstant::Int(value),
            Err(_) => {
                push_cli_option_parse_diagnostic(unit, option, "numerus", value, diagnostics);
                return None;
            }
        },
        CliType::Fractus => match value.parse::<f64>() {
            Ok(value) => MirConstant::Float(value),
            Err(_) => {
                push_cli_option_parse_diagnostic(unit, option, "fractus", value, diagnostics);
                return None;
            }
        },
        CliType::Bivalens => match value.parse::<bool>() {
            Ok(value) => MirConstant::Bool(value),
            Err(_) => {
                push_cli_option_parse_diagnostic(unit, option, "bivalens", value, diagnostics);
                return None;
            }
        },
        CliType::Octeti | CliType::ListaTextus | CliType::ListaNumerus => return None,
    };
    Some(MirOperand::Constant(constant))
}

fn cli_default_operand_value(
    unit: &mut AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    default: &CliDefault,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MirOperand> {
    cli_default_value(unit, &operand.operand.ty, default).or_else(|| {
        push_cli_operand_default_diagnostic(unit, operand, diagnostics);
        None
    })
}

fn cli_default_value(
    unit: &mut AnalyzedPackageUnit,
    ty: &CliType,
    default: &CliDefault,
) -> Option<MirOperand> {
    let constant = match (ty, default) {
        (CliType::Textus | CliType::Ignotum, CliDefault::Text(value)) => {
            let payload = textus_literal_payload(value);
            MirConstant::String(unit.analysis.interner.intern(&payload))
        }
        (CliType::Numerus, CliDefault::Integer(value)) => MirConstant::Int(*value),
        (CliType::Fractus, CliDefault::Float(value)) => MirConstant::Float(*value),
        (CliType::Fractus, CliDefault::Integer(value)) => MirConstant::Float(*value as f64),
        (CliType::Bivalens, CliDefault::Bool(value)) => MirConstant::Bool(*value),
        _ => return None,
    };
    Some(MirOperand::Constant(constant))
}

fn push_cli_operand_parse_diagnostic(
    unit: &AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    ty: &str,
    value: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name = operand.binding_name.as_str();
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR could not parse CLI operand `{name}` value `{value}` as {ty}; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string()),
    );
}

fn push_cli_option_parse_diagnostic(
    unit: &AnalyzedPackageUnit,
    option: &PlannedCliOption<'_>,
    ty: &str,
    value: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name = option.binding_name.as_str();
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR could not parse CLI option `{name}` value `{value}` as {ty}; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string()),
    );
}

fn push_cli_option_default_diagnostic(
    unit: &AnalyzedPackageUnit,
    option: &PlannedCliOption<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name = option.binding_name.as_str();
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR does not yet support CLI option `{name}` default value; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string())
        .with_arg("issue", "package_mir_cli_option_default_unsupported")
        .with_arg("option", name),
    );
}

fn push_cli_operand_default_diagnostic(
    unit: &AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name = operand.binding_name.as_str();
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR does not yet support CLI operand `{name}` default value; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string())
        .with_arg("issue", "package_mir_cli_operand_default_unsupported")
        .with_arg("operand", name),
    );
}

fn push_cli_operand_missing_diagnostic(
    unit: &AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name = operand.binding_name.as_str();
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR expected CLI operand `{name}` but no value was provided; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string()),
    );
}

fn command_has_unsupported_options(command: &CliCommand) -> bool {
    command
        .options
        .iter()
        .any(|option| !is_package_mir_supported_option(option))
}

struct MatchedCliCommand<'a> {
    command: &'a CliCommand,
    consumed: usize,
}

fn matching_cli_command<'a>(
    commands: &'a [CliCommand],
    argumenta: &[String],
) -> Option<MatchedCliCommand<'a>> {
    let mut routes = commands
        .iter()
        .flat_map(|command| {
            command_routes(command)
                .into_iter()
                .map(move |route| (command, route))
        })
        .collect::<Vec<_>>();
    routes.sort_by_key(|(_, route)| std::cmp::Reverse(route.len()));
    routes
        .into_iter()
        .find(|(_, route)| cli_route_matches(route, argumenta))
        .map(|(command, route)| MatchedCliCommand {
            command,
            consumed: route.len(),
        })
}

fn command_routes(command: &CliCommand) -> Vec<Vec<&str>> {
    std::iter::once(command.path.iter().map(String::as_str).collect::<Vec<_>>())
        .chain(command.aliases.iter().map(|alias| alias_path(alias)))
        .collect()
}

fn alias_path(alias: &str) -> Vec<&str> {
    alias.split('/').filter(|part| !part.is_empty()).collect()
}

fn cli_route_matches(route: &[&str], argumenta: &[String]) -> bool {
    argumenta.len() >= route.len()
        && route
            .iter()
            .enumerate()
            .all(|(index, part)| argumenta[index] == *part)
}

fn command_unit_index(package: &AnalyzedPackage, command: &CliCommand) -> Option<usize> {
    if command.module_path.is_some() {
        return package
            .units
            .iter()
            .enumerate()
            .find(|unit| {
                !unit.1.is_entry
                    && unit.1.analysis.cli_program.as_ref().is_some_and(|program| {
                        program.commands.iter().any(|candidate| {
                            candidate.path == command.path && candidate.function == command.function
                        })
                    })
            })
            .map(|(index, _)| index);
    }
    package.units.iter().position(|unit| unit.is_entry)
}

fn command_in_unit<'a>(
    unit: &'a AnalyzedPackageUnit,
    command: &CliCommand,
) -> Option<&'a CliCommand> {
    unit.analysis
        .cli_program
        .as_ref()?
        .commands
        .iter()
        .find(|candidate| candidate.path == command.path && candidate.function == command.function)
}

fn has_unsupported_package_mir_operands<'a>(
    operands: impl IntoIterator<Item = &'a CliOperand>,
) -> bool {
    let operands = operands.into_iter().collect::<Vec<_>>();
    operands.iter().enumerate().any(|(index, operand)| {
        !is_package_mir_supported_operand(operand, index + 1 == operands.len())
    })
}

fn is_package_mir_supported_operand(operand: &CliOperand, is_final: bool) -> bool {
    if !is_package_mir_supported_operand_default(operand) {
        return false;
    }
    if operand.rest {
        return is_final && !matches!(operand.ty, CliType::Octeti);
    }
    if matches!(operand.ty, CliType::ListaTextus | CliType::ListaNumerus) {
        return is_final;
    }
    true
}

fn is_package_mir_supported_operand_default(operand: &CliOperand) -> bool {
    operand.default.as_ref().is_none_or(|default| {
        is_package_mir_scalar_default(&operand.ty, default)
            && !matches!(
                operand.ty,
                CliType::Octeti | CliType::ListaTextus | CliType::ListaNumerus
            )
    })
}

fn is_package_mir_scalar_default(ty: &CliType, default: &CliDefault) -> bool {
    matches!(
        (ty, default),
        (CliType::Textus | CliType::Ignotum, CliDefault::Text(_))
            | (CliType::Numerus, CliDefault::Integer(_))
            | (
                CliType::Fractus,
                CliDefault::Float(_) | CliDefault::Integer(_)
            )
            | (CliType::Bivalens, CliDefault::Bool(_))
    )
}

fn is_package_mir_supported_option(option: &CliOption) -> bool {
    if option.flag {
        return matches!(&option.ty, CliType::Bivalens);
    }
    is_package_mir_scalar_option(option)
}

fn cli_option_is_nullable(option: &CliOption) -> bool {
    option.default.is_none() && !option.flag
}

fn is_package_mir_scalar_option(option: &CliOption) -> bool {
    matches!(
        &option.ty,
        CliType::Textus
            | CliType::Ignotum
            | CliType::Numerus
            | CliType::Fractus
            | CliType::Bivalens
    ) && option
        .default
        .as_ref()
        .is_none_or(|default| is_package_mir_scalar_default(&option.ty, default))
}

fn cli_record_type(
    types: &mut radix::semantic::TypeTable,
    ty: &CliType,
    rest: bool,
) -> Option<TypeId> {
    let base = match ty {
        CliType::Textus | CliType::Ignotum => Primitive::Textus,
        CliType::Numerus => Primitive::Numerus,
        CliType::Fractus => Primitive::Fractus,
        CliType::Bivalens => Primitive::Bivalens,
        CliType::Octeti => Primitive::Octeti,
        CliType::ListaTextus => {
            let textus = types.primitive(Primitive::Textus);
            return Some(types.array(textus));
        }
        CliType::ListaNumerus => {
            let numerus = types.primitive(Primitive::Numerus);
            return Some(types.array(numerus));
        }
    };
    let base = types.primitive(base);
    if rest {
        Some(types.array(base))
    } else {
        Some(base)
    }
}

fn textus_literal_payload(text: &str) -> String {
    let mut escaped = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn unsupported_cli_diagnostic(path: &Path, surface: &str) -> Diagnostic {
    crate::package_diagnostic_error(format!(
        "package MIR does not yet support {surface}; use compiled package execution for this surface"
    ))
    .with_file(path.display().to_string())
    .with_arg("issue", "package_mir_cli_surface_unsupported")
    .with_arg("surface", surface)
}

fn package_mir_cli_exit_code(
    exit: &Option<CliExit>,
    args_name: Option<Symbol>,
    fields: &[MirRuntimeRecordField],
    unit: &AnalyzedPackageUnit,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Option<i32>> {
    let Some(exit) = exit else {
        return Some(None);
    };
    match exit {
        CliExit::Fixed(code) => match i32::try_from(*code) {
            Ok(code) => Some(Some(code)),
            Err(_) => {
                diagnostics.push(unsupported_cli_diagnostic(
                    &unit.path,
                    "CLI exit codes outside i32 range",
                ));
                None
            }
        },
        CliExit::Field { object, field } => {
            let args_name = args_name
                .map(|symbol| unit.analysis.interner.resolve(symbol))
                .unwrap_or("");
            if object != args_name {
                diagnostics.push(unsupported_cli_diagnostic(
                    &unit.path,
                    "CLI dynamic exit expressions",
                ));
                return None;
            }
            let value = fields.iter().find_map(|record_field| {
                let name = unit.analysis.interner.resolve(record_field.name);
                (name == field).then(|| package_mir_runtime_record_i32(&record_field.value))?
            });
            match value {
                Some(code) => Some(Some(code)),
                None => {
                    diagnostics.push(unsupported_cli_diagnostic(
                        &unit.path,
                        "CLI dynamic exit expressions",
                    ));
                    None
                }
            }
        }
        CliExit::Binding(_) | CliExit::Unsupported => {
            diagnostics.push(unsupported_cli_diagnostic(
                &unit.path,
                "CLI dynamic exit expressions",
            ));
            None
        }
    }
}

fn package_mir_runtime_record_i32(value: &MirRuntimeRecordValue) -> Option<i32> {
    let MirRuntimeRecordValue::Operand(MirOperand::Constant(MirConstant::Int(value))) = value
    else {
        return None;
    };
    i32::try_from(*value).ok()
}

fn local_namespace_call_targets(
    config: &Config,
    package: &AnalyzedPackage,
    consumer: PackageMirConsumer,
) -> Result<PackageMirLinks, Vec<Diagnostic>> {
    let library_resolver = library_resolver_from_config(config);
    let units_by_path = package
        .units
        .iter()
        .map(|unit| (unit.path.clone(), unit))
        .collect::<BTreeMap<_, _>>();
    let mut targets = HashMap::new();
    let mut namespaces = HashMap::new();
    let mut source_rewrites = HashMap::new();
    let mut next_synthetic = PACKAGE_MIR_SYNTHETIC_DEF_BASE;
    let mut diagnostics = Vec::new();

    for unit in &package.units {
        for item in &unit.analysis.hir.items {
            let HirItemKind::Import(import) = &item.kind else {
                continue;
            };
            let import_path = unit.analysis.interner.resolve(import.path);
            let resolution =
                resolve_import(&package.spec, &library_resolver, &unit.path, import_path);
            let ImportResolution::Local(target_path) = resolution else {
                if matches!(resolution, ImportResolution::Library(_))
                    && !is_bridged_norma_import_path(import_path)
                    && consumer == PackageMirConsumer::Interpreted
                {
                    diagnostics.push(
                        crate::package_diagnostic_error(format!(
                            "package MIR does not yet support library imports such as `{import_path}`; use compiled package execution for this surface"
                        ))
                        .with_file(unit.path.display().to_string())
                        .with_arg("issue", "package_mir_library_imports_unsupported")
                        .with_arg("import", import_path),
                    );
                }
                continue;
            };
            let Some(sibling) = units_by_path.get(&target_path).copied() else {
                continue;
            };
            for import_item in &import.items {
                let binding = unit
                    .analysis
                    .interner
                    .resolve(import_item.alias.unwrap_or(import_item.name));
                let exports = unit
                    .namespace_exports
                    .get(binding)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .collect::<BTreeSet<_>>();
                namespaces.insert((unit.path.clone(), import_item.def_id), exports.clone());
                for function in exported_top_level_functions(sibling, &exports) {
                    let synthetic = *source_rewrites
                        .entry((sibling.path.clone(), function.def_id))
                        .or_insert_with(|| {
                            let def_id = DefId(next_synthetic);
                            next_synthetic += 1;
                            def_id
                        });
                    targets.insert(
                        (unit.path.clone(), import_item.def_id, function.name),
                        synthetic,
                    );
                }
            }
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    Ok(PackageMirLinks {
        calls: targets,
        namespaces,
        sources: source_rewrites,
    })
}

struct ExportedFunction {
    name: String,
    def_id: DefId,
}

fn exported_top_level_functions(
    unit: &AnalyzedPackageUnit,
    exports: &BTreeSet<String>,
) -> Vec<ExportedFunction> {
    unit.analysis
        .hir
        .items
        .iter()
        .filter_map(|item| {
            let HirItemKind::Function(function) = &item.kind else {
                return None;
            };
            let name = unit.analysis.interner.resolve(function.name).to_owned();
            exports.contains(&name).then_some(ExportedFunction {
                name,
                def_id: item.def_id,
            })
        })
        .collect()
}

fn rewrite_unit_namespace_calls(
    unit: &mut AnalyzedPackageUnit,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    if let Some(entry) = &mut unit.analysis.hir.entry {
        rewrite_block(
            &unit.path,
            entry,
            &unit.analysis.interner,
            targets,
            namespaces,
            &mut diagnostics,
        );
    }
    for item in &mut unit.analysis.hir.items {
        if let HirItemKind::Function(function) = &mut item.kind {
            if let Some(body) = &mut function.body {
                rewrite_block(
                    &unit.path,
                    body,
                    &unit.analysis.interner,
                    targets,
                    namespaces,
                    &mut diagnostics,
                );
            }
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics
            .into_iter()
            .map(|message| {
                crate::package_diagnostic_error(message).with_file(unit.path.display().to_string())
            })
            .collect())
    }
}

fn rewrite_block(
    unit_path: &Path,
    block: &mut HirBlock,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    for stmt in &mut block.statements {
        rewrite_stmt(unit_path, stmt, interner, targets, namespaces, diagnostics);
    }
    if let Some(expr) = &mut block.expr {
        rewrite_expr(unit_path, expr, interner, targets, namespaces, diagnostics);
    }
}

fn rewrite_stmt(
    unit_path: &Path,
    stmt: &mut HirStatement,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match &mut stmt.kind {
        HirStatementKind::Local(local) => {
            if let Some(init) = &mut local.init {
                rewrite_expr(unit_path, init, interner, targets, namespaces, diagnostics);
            }
        }
        HirStatementKind::Expr(expr) => {
            rewrite_expr(unit_path, expr, interner, targets, namespaces, diagnostics)
        }
        HirStatementKind::Redde(Some(expr)) => {
            rewrite_expr(unit_path, expr, interner, targets, namespaces, diagnostics)
        }
        HirStatementKind::IncDec(inc_dec) => rewrite_expr(
            unit_path,
            &mut inc_dec.target,
            interner,
            targets,
            namespaces,
            diagnostics,
        ),
        HirStatementKind::Custodi(custodi) => {
            for clause in &mut custodi.clauses {
                rewrite_expr(
                    unit_path,
                    &mut clause.cond,
                    interner,
                    targets,
                    namespaces,
                    diagnostics,
                );
                rewrite_block(
                    unit_path,
                    &mut clause.body,
                    interner,
                    targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirStatementKind::Redde(None)
        | HirStatementKind::Rumpe
        | HirStatementKind::Perge
        | HirStatementKind::Tacet => {}
    }
}

fn rewrite_expr(
    unit_path: &Path,
    expr: &mut HirExpression,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match &mut expr.kind {
        HirExpressionKind::Binary(_, lhs, rhs) | HirExpressionKind::Assign(lhs, rhs) => {
            rewrite_expr(unit_path, lhs, interner, targets, namespaces, diagnostics);
            rewrite_expr(unit_path, rhs, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::Unary(_, inner)
        | HirExpressionKind::Cede(inner)
        | HirExpressionKind::Panic(inner)
        | HirExpressionKind::Throw(inner) => {
            rewrite_expr(unit_path, inner, interner, targets, namespaces, diagnostics)
        }
        HirExpressionKind::Call(callee, type_args, args) => {
            rewrite_call_args(unit_path, args, interner, targets, namespaces, diagnostics);
            if let HirExpressionKind::Field(receiver, method) = &callee.kind {
                if let Some(target_def) =
                    namespace_call_target(unit_path, receiver, *method, interner, targets)
                {
                    if !type_args.is_empty() {
                        diagnostics.push(
                            "package MIR does not support type arguments on namespace calls"
                                .to_owned(),
                        );
                        return;
                    }
                    let call_args = std::mem::take(args);
                    let callee = HirExpression {
                        id: receiver.id,
                        kind: HirExpressionKind::Path(target_def),
                        ty: None,
                        span: receiver.span,
                    };
                    expr.kind = HirExpressionKind::Call(Box::new(callee), Vec::new(), call_args);
                    return;
                }
                if let Some(message) =
                    namespace_call_diagnostic(unit_path, receiver, *method, interner, namespaces)
                {
                    diagnostics.push(message);
                    return;
                }
            }
            rewrite_expr(
                unit_path,
                callee,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
        }
        HirExpressionKind::MethodCall(receiver, method, type_args, args) => {
            rewrite_expr(
                unit_path,
                receiver,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
            rewrite_call_args(unit_path, args, interner, targets, namespaces, diagnostics);
            if let Some(target_def) =
                namespace_call_target(unit_path, receiver, *method, interner, targets)
            {
                if !type_args.is_empty() {
                    diagnostics.push(
                        "package MIR does not support type arguments on namespace calls".to_owned(),
                    );
                    return;
                }
                let call_args = std::mem::take(args);
                let callee = HirExpression {
                    id: receiver.id,
                    kind: HirExpressionKind::Path(target_def),
                    ty: None,
                    span: receiver.span,
                };
                expr.kind = HirExpressionKind::Call(Box::new(callee), Vec::new(), call_args);
            } else if let Some(message) =
                namespace_call_diagnostic(unit_path, receiver, *method, interner, namespaces)
            {
                diagnostics.push(message);
            }
        }
        HirExpressionKind::Field(object, _) => rewrite_expr(
            unit_path,
            object,
            interner,
            targets,
            namespaces,
            diagnostics,
        ),
        HirExpressionKind::Index(object, index) => {
            rewrite_expr(
                unit_path,
                object,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
            rewrite_expr(unit_path, index, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::OptionalChain(object, chain) => {
            rewrite_expr(
                unit_path,
                object,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
            rewrite_optional_chain(unit_path, chain, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::NonNull(object, chain) => {
            rewrite_expr(
                unit_path,
                object,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
            rewrite_non_null_chain(unit_path, chain, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::Block(block) | HirExpressionKind::Loop(block) => {
            rewrite_block(unit_path, block, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::Si {
            cond,
            then_block,
            then_catch,
            else_block,
        } => {
            rewrite_expr(unit_path, cond, interner, targets, namespaces, diagnostics);
            rewrite_block(
                unit_path,
                then_block,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
            if let Some(cape) = then_catch {
                rewrite_cape(unit_path, cape, interner, targets, namespaces, diagnostics);
            }
            if let Some(block) = else_block {
                rewrite_block(unit_path, block, interner, targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Discerne {
            scrutinees, arms, ..
        } => {
            for scrutinee in scrutinees {
                rewrite_expr(
                    unit_path,
                    scrutinee,
                    interner,
                    targets,
                    namespaces,
                    diagnostics,
                );
            }
            for arm in arms {
                rewrite_casu_arm(unit_path, arm, interner, targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Dum(cond, block) => {
            rewrite_expr(unit_path, cond, interner, targets, namespaces, diagnostics);
            rewrite_block(unit_path, block, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::Itera(_, _, _, iterable, block) => {
            rewrite_expr(
                unit_path,
                iterable,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
            rewrite_block(unit_path, block, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::Intervallum {
            start, end, step, ..
        } => {
            rewrite_expr(unit_path, start, interner, targets, namespaces, diagnostics);
            rewrite_expr(unit_path, end, interner, targets, namespaces, diagnostics);
            if let Some(step) = step {
                rewrite_expr(unit_path, step, interner, targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Array(elements) => {
            for element in elements {
                match element {
                    radix::hir::HirArrayElement::Expr(expr)
                    | radix::hir::HirArrayElement::Spread(expr) => {
                        rewrite_expr(unit_path, expr, interner, targets, namespaces, diagnostics);
                    }
                }
            }
        }
        HirExpressionKind::Struct(_, fields) => {
            for (_, value) in fields {
                rewrite_expr(unit_path, value, interner, targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Tuple(items)
        | HirExpressionKind::Scribe(_, items)
        | HirExpressionKind::Scriptum(_, items) => {
            for item in items {
                rewrite_expr(unit_path, item, interner, targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Adfirma(cond, message) => {
            rewrite_expr(unit_path, cond, interner, targets, namespaces, diagnostics);
            if let Some(message) = message {
                rewrite_expr(
                    unit_path,
                    message,
                    interner,
                    targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Handled { body, catch } => {
            rewrite_block(unit_path, body, interner, targets, namespaces, diagnostics);
            rewrite_cape(unit_path, catch, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::Tempta {
            body,
            catch,
            finally,
        } => {
            rewrite_block(unit_path, body, interner, targets, namespaces, diagnostics);
            if let Some(block) = catch {
                rewrite_block(unit_path, block, interner, targets, namespaces, diagnostics);
            }
            if let Some(block) = finally {
                rewrite_block(unit_path, block, interner, targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Clausura(_, _, _, body) => {
            rewrite_expr(unit_path, body, interner, targets, namespaces, diagnostics)
        }
        HirExpressionKind::Verte {
            source, entries, ..
        } => {
            rewrite_expr(
                unit_path,
                source,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
            if let Some(entries) = entries {
                for entry in entries {
                    rewrite_object_field(
                        unit_path,
                        entry,
                        interner,
                        targets,
                        namespaces,
                        diagnostics,
                    );
                }
            }
        }
        HirExpressionKind::Conversio {
            source, recovery, ..
        } => {
            rewrite_expr(
                unit_path,
                source,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
            if let Some(recovery) = recovery {
                rewrite_expr(
                    unit_path,
                    recovery,
                    interner,
                    targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Ad { opener, .. } => {
            if let Some(opener) = opener {
                rewrite_expr(
                    unit_path,
                    opener,
                    interner,
                    targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Ref(_, inner) | HirExpressionKind::Deref(inner) => {
            rewrite_expr(unit_path, inner, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::TypeCheck { expr: inner, .. } => {
            rewrite_expr(unit_path, inner, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::Path(_)
        | HirExpressionKind::Literal(_)
        | HirExpressionKind::Vacua
        | HirExpressionKind::ReadLine
        | HirExpressionKind::Error => {}
    }
}

fn namespace_call_target(
    unit_path: &Path,
    receiver: &HirExpression,
    method: Symbol,
    interner: &Interner,
    targets: &NamespaceCallTargets,
) -> Option<DefId> {
    let HirExpressionKind::Path(namespace_def) = &receiver.kind else {
        return None;
    };
    let method_name = interner.resolve(method).to_owned();
    targets
        .get(&(unit_path.to_path_buf(), *namespace_def, method_name))
        .copied()
}

fn namespace_call_diagnostic(
    unit_path: &Path,
    receiver: &HirExpression,
    method: radix::lexer::Symbol,
    interner: &Interner,
    namespaces: &NamespaceExports,
) -> Option<String> {
    let (namespace_def, mut fields) = namespace_receiver_path(receiver, interner)?;
    let exports = namespaces.get(&(unit_path.to_path_buf(), namespace_def))?;
    let method_name = interner.resolve(method).to_owned();
    if fields.is_empty() {
        if exports.contains(&method_name) {
            return Some(format!(
                "package MIR does not yet support non-function namespace member `{method_name}`"
            ));
        }
        return Some(format!("namespace does not export `{method_name}`"));
    }
    fields.push(method_name);
    let qualified = fields.join(".");
    Some(format!(
        "package MIR does not yet support nested namespace call `{qualified}`"
    ))
}

fn namespace_receiver_path(
    expr: &HirExpression,
    interner: &Interner,
) -> Option<(DefId, Vec<String>)> {
    match &expr.kind {
        HirExpressionKind::Path(def_id) => Some((*def_id, Vec::new())),
        HirExpressionKind::Field(object, field) => {
            let (def_id, mut fields) = namespace_receiver_path(object, interner)?;
            fields.push(interner.resolve(*field).to_owned());
            Some((def_id, fields))
        }
        _ => None,
    }
}

fn rewrite_call_args(
    unit_path: &Path,
    args: &mut [HirCallArg],
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    for arg in args {
        rewrite_expr(
            unit_path,
            &mut arg.expr,
            interner,
            targets,
            namespaces,
            diagnostics,
        );
    }
}

fn rewrite_cape(
    unit_path: &Path,
    cape: &mut HirCape,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    rewrite_block(
        unit_path,
        &mut cape.body,
        interner,
        targets,
        namespaces,
        diagnostics,
    );
}

fn rewrite_casu_arm(
    unit_path: &Path,
    arm: &mut HirCasuArm,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    if let Some(guard) = &mut arm.guard {
        rewrite_expr(unit_path, guard, interner, targets, namespaces, diagnostics);
    }
    rewrite_expr(
        unit_path,
        &mut arm.body,
        interner,
        targets,
        namespaces,
        diagnostics,
    );
}

fn rewrite_object_field(
    unit_path: &Path,
    field: &mut HirObjectField,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match &mut field.key {
        radix::hir::HirObjectKey::Computed(key) | radix::hir::HirObjectKey::Spread(key) => {
            rewrite_expr(unit_path, key, interner, targets, namespaces, diagnostics);
        }
        radix::hir::HirObjectKey::Ident(_) | radix::hir::HirObjectKey::String(_) => {}
    }
    if let Some(value) = &mut field.value {
        rewrite_expr(unit_path, value, interner, targets, namespaces, diagnostics);
    }
}

fn rewrite_optional_chain(
    unit_path: &Path,
    chain: &mut HirOptionalChainKind,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match chain {
        HirOptionalChainKind::Member(_) => {}
        HirOptionalChainKind::Index(index) => {
            rewrite_expr(unit_path, index, interner, targets, namespaces, diagnostics)
        }
        HirOptionalChainKind::Call(args) => {
            rewrite_call_args(unit_path, args, interner, targets, namespaces, diagnostics)
        }
    }
}

fn rewrite_non_null_chain(
    unit_path: &Path,
    chain: &mut radix::hir::HirNonNullKind,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match chain {
        radix::hir::HirNonNullKind::Member(_) => {}
        radix::hir::HirNonNullKind::Index(index) => {
            rewrite_expr(unit_path, index, interner, targets, namespaces, diagnostics)
        }
        radix::hir::HirNonNullKind::Call(args) => {
            rewrite_call_args(unit_path, args, interner, targets, namespaces, diagnostics)
        }
    }
}

fn lower_package_units<'a>(
    package: &'a mut AnalyzedPackage,
    entry_index: usize,
    source_rewrites: &SourceRewrites,
    cli_plan: &CliPackagePlan,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    struct PendingUnit<'a> {
        lowered: LoweredMirUnit<'a>,
        dispatch_function: Option<MirFunctionId>,
    }

    let (before, rest) = package.units.split_at_mut(entry_index);
    let Some((entry, after)) = rest.split_first_mut() else {
        unreachable!("entry index selected from package units");
    };
    let entry_path = entry.path.clone();
    let mut pending = Vec::new();

    for unit in before.iter_mut().chain(after.iter_mut()) {
        let unit_path = unit.path.clone();
        let source_interner = unit.analysis.interner.clone();
        let mut lowered = lower_unit(unit, &cli_plan.entry_records)?;
        remap_program_text_symbols(
            &mut lowered.program,
            &source_interner,
            &mut entry.analysis.interner,
        );
        let source_to_entry_types =
            import_lowered_semantic_types(lowered.validation.types, &mut entry.analysis.types);
        rewrite_lowered_type_ids(&mut lowered, &source_to_entry_types);
        if let Some(rewrite) = cli_plan
            .dispatch
            .as_ref()
            .filter(|dispatch| dispatch.unit_path == unit_path)
            .and_then(|dispatch| dispatch.record_type_rewrite.as_ref())
        {
            let dispatch_rewrites =
                imported_dispatch_type_rewrites(rewrite, &source_to_entry_types);
            rewrite_lowered_type_ids(&mut lowered, &dispatch_rewrites);
        }
        rewrite_program_sources(&mut lowered.program, &unit_path, source_rewrites);
        ensure_unique_definition_sources(&lowered.program, &unit_path)?;
        let dispatch_function =
            selected_cli_dispatch_function(cli_plan, &unit_path, &lowered.program);
        pending.push(PendingUnit {
            lowered,
            dispatch_function,
        });
    }

    let mut merged = lower_unit(entry, &cli_plan.entry_records)?;
    ensure_unique_definition_sources(&merged.program, &entry_path)?;
    let mut dispatch_function =
        selected_cli_dispatch_function(cli_plan, &entry_path, &merged.program);

    for mut unit in pending {
        if let Some(local_id) = unit.dispatch_function {
            let offset = merged.program.functions.len() as u32;
            dispatch_function = Some(MirFunctionId(local_id.0 + offset));
        }
        append_shifted_program(&mut merged, &mut unit.lowered);
        ensure_unique_definition_sources(&merged.program, &entry_path)?;
    }

    if cli_plan.dispatch.is_some() {
        let Some(function) = dispatch_function else {
            return Err(vec![mir_diag(
                &entry_path,
                "package MIR could not find selected CLI command function",
            )]);
        };
        install_cli_dispatch_entry(&mut merged, function, &entry_path)?;
    }

    Ok(merged)
}

fn selected_cli_dispatch_function(
    cli_plan: &CliPackagePlan,
    unit_path: &Path,
    program: &MirProgram,
) -> Option<MirFunctionId> {
    let dispatch = cli_plan.dispatch.as_ref()?;
    if dispatch.unit_path != unit_path {
        return None;
    }
    find_cli_dispatch_function(program, dispatch.function)
}

fn find_cli_dispatch_function(program: &MirProgram, function: Symbol) -> Option<MirFunctionId> {
    program
        .functions
        .iter()
        .find(|candidate| candidate.name == Some(function))
        .map(|candidate| candidate.id)
}

fn import_lowered_semantic_types(
    source: &TypeTable,
    target: &mut TypeTable,
) -> Vec<(TypeId, TypeId)> {
    let mut imported = HashMap::new();
    let mut rewrites = Vec::new();
    for index in 0..source.type_count() {
        let source_ty = TypeId(index as u32);
        let target_ty = import_semantic_type(source, target, source_ty, &mut imported);
        push_type_rewrite(&mut rewrites, source_ty, target_ty);
    }
    rewrites
}

fn imported_dispatch_type_rewrites(
    rewrite: &CliRecordTypeRewrite,
    imported: &[(TypeId, TypeId)],
) -> Vec<(TypeId, TypeId)> {
    rewrite
        .types
        .iter()
        .filter_map(|(from, to)| {
            let imported_from = imported
                .iter()
                .find_map(|(source, target)| (*source == *from).then_some(*target))
                .unwrap_or(*from);
            (imported_from != *to).then_some((imported_from, *to))
        })
        .collect()
}

fn rewrite_lowered_type_ids(lowered: &mut LoweredMirUnit<'_>, rewrites: &[(TypeId, TypeId)]) {
    for function in &mut lowered.program.functions {
        rewrite_type_id(&mut function.return_ty, rewrites);
        if let Some(error_ty) = &mut function.error_ty {
            rewrite_type_id(error_ty, rewrites);
        }
        for param in &mut function.params {
            rewrite_type_id(&mut param.ty, rewrites);
        }
        for local in &mut function.locals {
            rewrite_type_id(&mut local.ty, rewrites);
        }
        for temp in &mut function.temps {
            rewrite_type_id(&mut temp.ty, rewrites);
        }
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                rewrite_statement_type_id(statement, rewrites);
            }
        }
    }
    for environment in &mut lowered.closure_environments {
        rewrite_type_id(&mut environment.value_ty, rewrites);
        for capture in &mut environment.captures {
            rewrite_type_id(&mut capture.ty, rewrites);
        }
    }
}

fn rewrite_type_id(ty: &mut radix::mir::MirType, rewrites: &[(TypeId, TypeId)]) {
    if let Some((_, to)) = rewrites.iter().find(|(from, _)| ty.semantic_id() == *from) {
        *ty = radix::mir::MirType::semantic(*to);
    }
}

fn rewrite_statement_type_id(statement: &mut MirStatement, rewrites: &[(TypeId, TypeId)]) {
    match &mut statement.kind {
        MirStatementKind::Assign { value, .. } => rewrite_value_type_id(value, rewrites),
        MirStatementKind::Call { .. } => {}
        MirStatementKind::RuntimeCall { call, .. } => {
            rewrite_type_id(&mut call.return_ty, rewrites);
        }
        MirStatementKind::Construct { aggregate, .. } => {
            rewrite_type_id(&mut aggregate.ty, rewrites);
        }
    }
}

fn rewrite_value_type_id(value: &mut MirValue, rewrites: &[(TypeId, TypeId)]) {
    rewrite_type_id(&mut value.ty, rewrites);
}

fn install_cli_dispatch_entry(
    lowered: &mut LoweredMirUnit<'_>,
    command: MirFunctionId,
    entry_path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let Some(entry_index) = lowered
        .program
        .functions
        .iter()
        .position(|function| is_explicit_entry_function(function, lowered.validation.types))
    else {
        return Err(vec![mir_diag(
            entry_path,
            "package MIR could not find root CLI entry function",
        )]);
    };
    let span = lowered.program.functions[entry_index].span;
    lowered.program.functions[entry_index].locals.clear();
    lowered.program.functions[entry_index].temps.clear();
    lowered.program.functions[entry_index].blocks = vec![MirBlock {
        id: MirBlockId(0),
        statements: vec![MirStatement {
            kind: MirStatementKind::Call {
                destination: None,
                callee: MirCallee::Function(command),
                args: Vec::new(),
            },
            span,
        }],
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span,
        },
        span,
    }];
    Ok(())
}

fn is_explicit_entry_function(function: &MirFunction, types: &radix::semantic::TypeTable) -> bool {
    function.source.is_none()
        && function.name.is_none()
        && function.params.is_empty()
        && matches!(
            types.get(function.return_ty.semantic_id()),
            Type::Primitive(Primitive::Vacuum)
        )
}

fn remap_program_text_symbols(program: &mut MirProgram, source: &Interner, target: &mut Interner) {
    for function in &mut program.functions {
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                remap_statement_text_symbols(statement, source, target);
            }
            remap_terminator_text_symbols(&mut block.terminator, source, target);
        }
    }
}

fn remap_statement_text_symbols(
    statement: &mut MirStatement,
    source: &Interner,
    target: &mut Interner,
) {
    match &mut statement.kind {
        MirStatementKind::Assign { place, value } => {
            remap_place_text_symbols(place, source, target);
            remap_value_text_symbols(value, source, target);
        }
        MirStatementKind::Call {
            destination,
            callee,
            args,
        } => {
            if let Some(destination) = destination {
                remap_place_text_symbols(destination, source, target);
            }
            remap_callee_text_symbols(callee, source, target);
            for arg in args {
                remap_operand_text_symbols(arg, source, target);
            }
        }
        MirStatementKind::RuntimeCall { destination, call } => {
            if let Some(destination) = destination {
                remap_place_text_symbols(destination, source, target);
            }
            remap_runtime_call_text_symbols(call, source, target);
        }
        MirStatementKind::Construct {
            destination,
            aggregate,
        } => {
            remap_place_text_symbols(destination, source, target);
            remap_aggregate_text_symbols(aggregate, source, target);
        }
    }
}

fn remap_terminator_text_symbols(
    terminator: &mut MirTerminator,
    source: &Interner,
    target: &mut Interner,
) {
    match &mut terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            remap_operand_text_symbols(operand, source, target)
        }
        MirTerminatorKind::TryCall {
            destination,
            callee,
            args,
            error_place,
            ..
        } => {
            if let Some(destination) = destination {
                remap_place_text_symbols(destination, source, target);
            }
            remap_callee_text_symbols(callee, source, target);
            for arg in args {
                remap_operand_text_symbols(arg, source, target);
            }
            remap_place_text_symbols(error_place, source, target);
        }
        MirTerminatorKind::Branch { condition, .. } => {
            remap_operand_text_symbols(condition, source, target)
        }
        MirTerminatorKind::Switch { value, cases, .. } => {
            remap_operand_text_symbols(value, source, target);
            for case in cases {
                remap_constant_text_symbols(&mut case.value, source, target);
            }
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

fn remap_value_text_symbols(value: &mut MirValue, source: &Interner, target: &mut Interner) {
    match &mut value.kind {
        MirValueKind::Operand(operand) => remap_operand_text_symbols(operand, source, target),
        MirValueKind::Closure(closure) => {
            remap_operand_text_symbols(&mut closure.environment, source, target)
        }
        MirValueKind::Unary { operand, .. } => remap_operand_text_symbols(operand, source, target),
        MirValueKind::Binary { lhs, rhs, .. } => {
            remap_operand_text_symbols(lhs, source, target);
            remap_operand_text_symbols(rhs, source, target);
        }
        MirValueKind::Option(op) => remap_option_text_symbols(op, source, target),
    }
}

fn remap_operand_text_symbols(operand: &mut MirOperand, source: &Interner, target: &mut Interner) {
    match operand {
        MirOperand::Place(place) => remap_place_text_symbols(place, source, target),
        MirOperand::Constant(constant) => remap_constant_text_symbols(constant, source, target),
        MirOperand::Temp(_) | MirOperand::Value(_) => {}
    }
}

fn remap_place_text_symbols(place: &mut MirPlace, source: &Interner, target: &mut Interner) {
    for projection in &mut place.projections {
        match projection {
            MirProjection::Field(field) => {
                *field = target.intern(source.resolve(*field));
            }
            MirProjection::VariantField { field, .. } => {
                *field = target.intern(source.resolve(*field));
            }
            MirProjection::Index(operand) => remap_operand_text_symbols(operand, source, target),
            MirProjection::ClosureCapture { .. }
            | MirProjection::VectorLane(_)
            | MirProjection::MatrixCell { .. } => {}
        }
    }
}

fn remap_callee_text_symbols(callee: &mut MirCallee, source: &Interner, target: &mut Interner) {
    match callee {
        MirCallee::Closure(closure) => {
            remap_operand_text_symbols(&mut closure.environment, source, target)
        }
        MirCallee::Value(operand) => remap_operand_text_symbols(operand, source, target),
        MirCallee::Function(_) | MirCallee::Definition { .. } => {}
    }
}

fn remap_runtime_call_text_symbols(
    call: &mut MirRuntimeCall,
    source: &Interner,
    target: &mut Interner,
) {
    if let radix::mir::MirIntrinsic::FormatString { template } = &mut call.intrinsic {
        *template = target.intern(source.resolve(*template));
    }
    if let radix::mir::MirIntrinsic::Convert(conversion) = &mut call.intrinsic {
        if let Some(recovery) = &mut conversion.recovery {
            remap_operand_text_symbols(recovery, source, target);
        }
        for defaults in &mut conversion.struct_defaults {
            for field in &mut defaults.fields {
                field.name = target.intern(source.resolve(field.name));
                remap_operand_text_symbols(&mut field.value, source, target);
            }
        }
    }
    for arg in &mut call.args {
        remap_operand_text_symbols(arg, source, target);
    }
}

fn remap_aggregate_text_symbols(
    aggregate: &mut MirAggregate,
    source: &Interner,
    target: &mut Interner,
) {
    match &mut aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        remap_operand_text_symbols(operand, source, target)
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                item.name = target.intern(source.resolve(item.name));
                remap_operand_text_symbols(&mut item.value, source, target);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                remap_operand_text_symbols(&mut item.key, source, target);
                remap_operand_text_symbols(&mut item.value, source, target);
            }
        }
    }
}

fn remap_option_text_symbols(op: &mut MirOptionOp, source: &Interner, target: &mut Interner) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand) => remap_operand_text_symbols(operand, source, target),
        MirOptionOp::Unwrap { value, .. } => remap_operand_text_symbols(value, source, target),
        MirOptionOp::Coalesce { value, fallback } => {
            remap_operand_text_symbols(value, source, target);
            remap_operand_text_symbols(fallback, source, target);
        }
        MirOptionOp::Chain { base, link } => {
            remap_operand_text_symbols(base, source, target);
            remap_option_chain_text_symbols(link, source, target);
        }
        MirOptionOp::None => {}
    }
}

fn remap_option_chain_text_symbols(
    link: &mut MirOptionChainLink,
    source: &Interner,
    target: &mut Interner,
) {
    match link {
        MirOptionChainLink::Index(operand) => remap_operand_text_symbols(operand, source, target),
        MirOptionChainLink::Call { callee, args } => {
            remap_callee_text_symbols(callee, source, target);
            for arg in args {
                remap_operand_text_symbols(arg, source, target);
            }
        }
        MirOptionChainLink::Field(field) => {
            *field = target.intern(source.resolve(*field));
        }
        MirOptionChainLink::VariantField { field, .. } => {
            *field = target.intern(source.resolve(*field));
        }
    }
}

fn remap_constant_text_symbols(
    constant: &mut MirConstant,
    source: &Interner,
    target: &mut Interner,
) {
    match constant {
        MirConstant::String(symbol) | MirConstant::Ascii(symbol) => {
            *symbol = target.intern(source.resolve(*symbol));
        }
        MirConstant::Regex { pattern, flags } => {
            *pattern = target.intern(source.resolve(*pattern));
            if let Some(flags) = flags {
                *flags = target.intern(source.resolve(*flags));
            }
        }
        MirConstant::Int(_)
        | MirConstant::Float(_)
        | MirConstant::Bool(_)
        | MirConstant::Nil
        | MirConstant::Unit
        | MirConstant::Octeti(_)
        | MirConstant::Function(_) => {}
    }
}

fn rewrite_program_sources(
    program: &mut MirProgram,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    if source_rewrites.is_empty() {
        return;
    }
    for function in &mut program.functions {
        if let Some(source) = function.source {
            if let Some(rewritten) = rewritten_source(unit_path, source, source_rewrites) {
                function.source = Some(rewritten);
            }
        }
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                rewrite_statement_sources(statement, unit_path, source_rewrites);
            }
            rewrite_terminator_sources(&mut block.terminator, unit_path, source_rewrites);
        }
    }
}

fn rewritten_source(
    unit_path: &Path,
    source: DefId,
    source_rewrites: &SourceRewrites,
) -> Option<DefId> {
    source_rewrites
        .get(&(unit_path.to_path_buf(), source))
        .copied()
}

fn rewrite_statement_sources(
    statement: &mut MirStatement,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match &mut statement.kind {
        MirStatementKind::Assign { value, .. } => {
            rewrite_value_sources(value, unit_path, source_rewrites)
        }
        MirStatementKind::Call { callee, args, .. } => {
            rewrite_callee_sources(callee, unit_path, source_rewrites);
            for arg in args {
                rewrite_operand_sources(arg, unit_path, source_rewrites);
            }
        }
        MirStatementKind::RuntimeCall { call, .. } => {
            for arg in &mut call.args {
                rewrite_operand_sources(arg, unit_path, source_rewrites);
            }
        }
        MirStatementKind::Construct { aggregate, .. } => {
            rewrite_aggregate_sources(aggregate, unit_path, source_rewrites)
        }
    }
}

fn rewrite_terminator_sources(
    terminator: &mut MirTerminator,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match &mut terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            rewrite_operand_sources(operand, unit_path, source_rewrites);
        }
        MirTerminatorKind::TryCall {
            callee,
            args,
            error_place,
            ..
        } => {
            rewrite_callee_sources(callee, unit_path, source_rewrites);
            for arg in args {
                rewrite_operand_sources(arg, unit_path, source_rewrites);
            }
            rewrite_place_sources(error_place, unit_path, source_rewrites);
        }
        MirTerminatorKind::Branch { condition, .. } => {
            rewrite_operand_sources(condition, unit_path, source_rewrites)
        }
        MirTerminatorKind::Switch { value, cases, .. } => {
            rewrite_operand_sources(value, unit_path, source_rewrites);
            for case in cases {
                rewrite_constant_sources(&mut case.value, unit_path, source_rewrites);
            }
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

fn rewrite_value_sources(value: &mut MirValue, unit_path: &Path, source_rewrites: &SourceRewrites) {
    match &mut value.kind {
        MirValueKind::Operand(operand) => {
            rewrite_operand_sources(operand, unit_path, source_rewrites)
        }
        MirValueKind::Closure(closure) => {
            rewrite_operand_sources(&mut closure.environment, unit_path, source_rewrites)
        }
        MirValueKind::Unary { operand, .. } => {
            rewrite_operand_sources(operand, unit_path, source_rewrites)
        }
        MirValueKind::Binary { lhs, rhs, .. } => {
            rewrite_operand_sources(lhs, unit_path, source_rewrites);
            rewrite_operand_sources(rhs, unit_path, source_rewrites);
        }
        MirValueKind::Option(op) => rewrite_option_sources(op, unit_path, source_rewrites),
    }
}

fn rewrite_operand_sources(
    operand: &mut MirOperand,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match operand {
        MirOperand::Place(place) => rewrite_place_sources(place, unit_path, source_rewrites),
        MirOperand::Constant(constant) => {
            rewrite_constant_sources(constant, unit_path, source_rewrites)
        }
        MirOperand::Temp(_) | MirOperand::Value(_) => {}
    }
}

fn rewrite_place_sources(place: &mut MirPlace, unit_path: &Path, source_rewrites: &SourceRewrites) {
    for projection in &mut place.projections {
        match projection {
            MirProjection::VariantField { variant, .. } => {
                if let Some(rewritten) = rewritten_source(unit_path, *variant, source_rewrites) {
                    *variant = rewritten;
                }
            }
            MirProjection::ClosureCapture { source, .. } => {
                if let Some(rewritten) = rewritten_source(unit_path, *source, source_rewrites) {
                    *source = rewritten;
                }
            }
            MirProjection::Index(operand) => {
                rewrite_operand_sources(operand, unit_path, source_rewrites)
            }
            MirProjection::Field(_)
            | MirProjection::VectorLane(_)
            | MirProjection::MatrixCell { .. } => {}
        }
    }
}

fn rewrite_constant_sources(
    _constant: &mut MirConstant,
    _unit_path: &Path,
    _source_rewrites: &SourceRewrites,
) {
}

fn rewrite_callee_sources(
    callee: &mut MirCallee,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match callee {
        MirCallee::Definition { source, .. } => {
            if let Some(rewritten) = rewritten_source(unit_path, *source, source_rewrites) {
                *source = rewritten;
            }
        }
        MirCallee::Value(operand) => rewrite_operand_sources(operand, unit_path, source_rewrites),
        MirCallee::Function(_) | MirCallee::Closure(_) => {}
    }
}

fn rewrite_aggregate_sources(
    aggregate: &mut MirAggregate,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match &mut aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        rewrite_operand_sources(operand, unit_path, source_rewrites);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                rewrite_operand_sources(&mut item.value, unit_path, source_rewrites);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                rewrite_operand_sources(&mut item.key, unit_path, source_rewrites);
                rewrite_operand_sources(&mut item.value, unit_path, source_rewrites);
            }
        }
    }
}

fn rewrite_option_sources(
    op: &mut MirOptionOp,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand)
        | MirOptionOp::Unwrap { value: operand, .. } => {
            rewrite_operand_sources(operand, unit_path, source_rewrites)
        }
        MirOptionOp::Coalesce { value, fallback } => {
            rewrite_operand_sources(value, unit_path, source_rewrites);
            rewrite_operand_sources(fallback, unit_path, source_rewrites);
        }
        MirOptionOp::Chain { base, link } => {
            rewrite_operand_sources(base, unit_path, source_rewrites);
            rewrite_option_chain_sources(link, unit_path, source_rewrites);
        }
        MirOptionOp::None => {}
    }
}

fn rewrite_option_chain_sources(
    link: &mut MirOptionChainLink,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match link {
        MirOptionChainLink::VariantField { variant, .. } => {
            if let Some(rewritten) = rewritten_source(unit_path, *variant, source_rewrites) {
                *variant = rewritten;
            }
        }
        MirOptionChainLink::Index(operand) => {
            rewrite_operand_sources(operand, unit_path, source_rewrites)
        }
        MirOptionChainLink::Call { callee, args } => {
            rewrite_callee_sources(callee, unit_path, source_rewrites);
            for arg in args {
                rewrite_operand_sources(arg, unit_path, source_rewrites);
            }
        }
        MirOptionChainLink::Field(_) => {}
    }
}

fn lower_unit<'a>(
    unit: &'a mut AnalyzedPackageUnit,
    cli_entry_records: &CliEntryRecords,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    let result = if unit.analysis.cli_program.is_some() {
        let fields = cli_entry_records
            .get(&unit.path)
            .cloned()
            .unwrap_or_default();
        lower_analyzed_unit_allowing_cli_runtime_records_with_context(&mut unit.analysis, fields)
    } else {
        lower_analyzed_unit_with_context(&mut unit.analysis)
    };
    result.map_err(|errors| {
        errors
            .into_iter()
            .map(|error| mir_lowering_diag(&unit.path, error.message))
            .collect()
    })
}

fn append_shifted_program(merged: &mut LoweredMirUnit<'_>, lowered: &mut LoweredMirUnit<'_>) {
    let offset = merged.program.functions.len() as u32;
    shift_program_ids(
        &mut lowered.program,
        &mut lowered.closure_environments,
        offset,
    );
    for environment in &lowered.closure_environments {
        merged
            .validation
            .closure_environments
            .insert(environment.id, environment.clone());
    }
    merged
        .program
        .functions
        .append(&mut lowered.program.functions);
}

fn ensure_unique_definition_sources(
    program: &MirProgram,
    path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let mut seen = HashSet::new();
    for function in &program.functions {
        let Some(source) = function.source else {
            continue;
        };
        if !seen.insert(source) {
            return Err(vec![mir_diag(
                path,
                format!(
                    "package MIR link found duplicate function source def#{}",
                    source.0
                ),
            )]);
        }
    }
    Ok(())
}

fn shift_program_ids(
    program: &mut MirProgram,
    closure_environments: &mut [MirClosureEnvironment],
    offset: u32,
) {
    if offset == 0 {
        return;
    }
    for function in &mut program.functions {
        shift_function_id(&mut function.id, offset);
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                shift_statement_ids(statement, offset);
            }
            shift_terminator_ids(&mut block.terminator, offset);
        }
    }
    for environment in closure_environments {
        shift_environment_id(&mut environment.id, offset);
        shift_function_id(&mut environment.function, offset);
        for capture in &mut environment.captures {
            shift_place_ids(&mut capture.source_place, offset);
        }
    }
}

fn shift_statement_ids(statement: &mut MirStatement, offset: u32) {
    match &mut statement.kind {
        MirStatementKind::Assign { place, value } => {
            shift_place_ids(place, offset);
            shift_value_ids(value, offset);
        }
        MirStatementKind::Call {
            destination,
            callee,
            args,
        } => {
            if let Some(destination) = destination {
                shift_place_ids(destination, offset);
            }
            shift_callee_ids(callee, offset);
            for arg in args {
                shift_operand_ids(arg, offset);
            }
        }
        MirStatementKind::RuntimeCall { destination, call } => {
            if let Some(destination) = destination {
                shift_place_ids(destination, offset);
            }
            shift_runtime_call_ids(call, offset);
        }
        MirStatementKind::Construct {
            destination,
            aggregate,
        } => {
            shift_place_ids(destination, offset);
            shift_aggregate_ids(aggregate, offset);
        }
    }
}

fn shift_terminator_ids(terminator: &mut MirTerminator, offset: u32) {
    match &mut terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            shift_operand_ids(operand, offset);
        }
        MirTerminatorKind::TryCall {
            destination,
            callee,
            args,
            error_place,
            ..
        } => {
            if let Some(destination) = destination {
                shift_place_ids(destination, offset);
            }
            shift_callee_ids(callee, offset);
            for arg in args {
                shift_operand_ids(arg, offset);
            }
            shift_place_ids(error_place, offset);
        }
        MirTerminatorKind::Branch { condition, .. } => shift_operand_ids(condition, offset),
        MirTerminatorKind::Switch { value, cases, .. } => {
            shift_operand_ids(value, offset);
            for case in cases {
                shift_switch_case_ids(case, offset);
            }
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

fn shift_value_ids(value: &mut MirValue, offset: u32) {
    match &mut value.kind {
        MirValueKind::Operand(operand) => shift_operand_ids(operand, offset),
        MirValueKind::Closure(closure) => shift_closure_value_ids(closure, offset),
        MirValueKind::Unary { operand, .. } => shift_operand_ids(operand, offset),
        MirValueKind::Binary { lhs, rhs, .. } => {
            shift_operand_ids(lhs, offset);
            shift_operand_ids(rhs, offset);
        }
        MirValueKind::Option(op) => shift_option_ids(op, offset),
    }
}

fn shift_operand_ids(operand: &mut MirOperand, offset: u32) {
    match operand {
        MirOperand::Place(place) => shift_place_ids(place, offset),
        MirOperand::Constant(constant) => shift_constant_ids(constant, offset),
        MirOperand::Temp(_) | MirOperand::Value(_) => {}
    }
}

fn shift_place_ids(place: &mut MirPlace, offset: u32) {
    for projection in &mut place.projections {
        match projection {
            MirProjection::ClosureCapture { environment, .. } => {
                shift_environment_id(environment, offset)
            }
            MirProjection::Index(operand) => shift_operand_ids(operand, offset),
            MirProjection::Field(_)
            | MirProjection::VariantField { .. }
            | MirProjection::VectorLane(_)
            | MirProjection::MatrixCell { .. } => {}
        }
    }
}

fn shift_constant_ids(constant: &mut MirConstant, offset: u32) {
    if let MirConstant::Function(id) = constant {
        shift_function_id(id, offset);
    }
}

fn shift_callee_ids(callee: &mut MirCallee, offset: u32) {
    match callee {
        MirCallee::Function(id) => shift_function_id(id, offset),
        MirCallee::Closure(closure) => shift_closure_callee_ids(closure, offset),
        MirCallee::Value(operand) => shift_operand_ids(operand, offset),
        MirCallee::Definition { .. } => {}
    }
}

fn shift_runtime_call_ids(call: &mut MirRuntimeCall, offset: u32) {
    for arg in &mut call.args {
        shift_operand_ids(arg, offset);
    }
}

fn shift_aggregate_ids(aggregate: &mut MirAggregate, offset: u32) {
    match &mut aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        shift_operand_ids(operand, offset);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                shift_operand_ids(&mut item.value, offset);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                shift_operand_ids(&mut item.key, offset);
                shift_operand_ids(&mut item.value, offset);
            }
        }
    }
}

fn shift_switch_case_ids(case: &mut MirSwitchCase, offset: u32) {
    shift_constant_ids(&mut case.value, offset);
}

fn shift_option_ids(op: &mut MirOptionOp, offset: u32) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand)
        | MirOptionOp::Unwrap { value: operand, .. } => shift_operand_ids(operand, offset),
        MirOptionOp::Coalesce { value, fallback } => {
            shift_operand_ids(value, offset);
            shift_operand_ids(fallback, offset);
        }
        MirOptionOp::Chain { base, link } => {
            shift_operand_ids(base, offset);
            shift_option_chain_link_ids(link, offset);
        }
        MirOptionOp::None => {}
    }
}

fn shift_option_chain_link_ids(link: &mut MirOptionChainLink, offset: u32) {
    match link {
        MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => {}
        MirOptionChainLink::Index(operand) => shift_operand_ids(operand, offset),
        MirOptionChainLink::Call { callee, args } => {
            shift_callee_ids(callee, offset);
            for arg in args {
                shift_operand_ids(arg, offset);
            }
        }
    }
}

fn shift_closure_value_ids(closure: &mut MirClosureValue, offset: u32) {
    shift_function_id(&mut closure.function, offset);
    shift_environment_id(&mut closure.environment_id, offset);
    shift_operand_ids(&mut closure.environment, offset);
}

fn shift_closure_callee_ids(closure: &mut MirClosureCallee, offset: u32) {
    shift_function_id(&mut closure.function, offset);
    shift_environment_id(&mut closure.environment_id, offset);
    shift_operand_ids(&mut closure.environment, offset);
}

fn shift_function_id(id: &mut MirFunctionId, offset: u32) {
    id.0 += offset;
}

fn shift_environment_id(id: &mut MirClosureEnvironmentId, offset: u32) {
    id.0 += offset;
}

fn mir_diag(path: &Path, message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message).with_file(path.display().to_string())
}

fn mir_lowering_diag(path: &Path, message: impl Into<String>) -> Diagnostic {
    mir_diag(path, message).with_phase(DiagnosticPhase::Mir)
}

fn mir_issue_diag(path: &Path, issue: &'static str, message: impl Into<String>) -> Diagnostic {
    mir_diag(path, message).with_arg("issue", issue)
}

fn stepper_diagnostics(path: &Path, errors: Vec<StepperError>) -> Vec<Diagnostic> {
    errors
        .into_iter()
        .map(|error| mir_diag(path, error.message))
        .collect()
}

#[cfg(test)]
#[path = "mir_test.rs"]
mod tests;

#[cfg(test)]
#[path = "mir_test_support.rs"]
pub(crate) mod test_support;
