//! FMIR image construction and loading (source-built, text, and binary formats).

use super::{
    DefId, Diagnostic, FMIR_TARGET_NAME, FMIR_TEXT_TARGET_NAME, FmirBinaryImageFile,
    FmirDeviceSection, FmirImageError, FmirPackageImage, FmirPackageImageFormat,
    FmirTextCliSection, FmirTextImageFile, FmirTextProgramSection, FmirTextRuntimeSection,
    FmirTextSourcesSection, FmirTextToolchainSection, FmirTextTypesSection, HashMap, HashSet, Host,
    LoweredMirUnit, MirClosureEnvironment, MirProgram, MirSeams, MirType,
    PACKAGE_MIR_ARTIFACT_VERSION, PackageRunStats, Path, PathBuf, PreparedPackageMir,
    RunnerKernelPolicy, Symbol, TypeTable, TypeTableSnapshot, bind_fmir_text_runtime_cli,
    check_fmir_runtime_requirements, decode_binary_image, decode_text_image, encode_binary_image,
    encode_text_image, fnv64_hex, fs, mir_diag, mir_issue_diag, relative_or_display,
    run_entry_with_process_exit, run_entry_with_stats_and_kernel_policy, runner_diagnostics,
    source_identity,
};

/// Struct, variant, and closure validation metadata carried by in-memory package FMIR
/// images (codex-gap S1 U2 VALUE members).
///
/// The source-built run path re-validates the merged program inside
/// [`run_fmir_package_image`] with a fresh [`radix_mir::MirValidationContext`].
/// Enum-variant constructs require the merged variant surface (parent
/// metadata, variant lists, variant field types) to validate — an empty
/// context errors with "enum variant is missing parent metadata". Captured
/// closures likewise require their environment layouts when the fresh image
/// context validates and executes an indirect call. The image therefore
/// carries the maps the merge proved; the decoded-artifact routes (fmir /
/// fmir-text wire formats) do not serialize this section and keep the previous
/// empty-metadata behavior for those images.
#[derive(Debug, Clone, Default)]
pub struct FmirValidationMetadata {
    pub struct_fields: HashMap<DefId, HashMap<Symbol, MirType>>,
    pub struct_field_order: HashMap<DefId, Vec<Symbol>>,
    pub variant_field_order: HashMap<DefId, Vec<Symbol>>,
    pub display_names: HashMap<DefId, String>,
    pub display_tokens: radix_mir::MirDisplayTokens,
    pub struct_type_params: HashMap<DefId, Vec<Symbol>>,
    /// Struct → the contracts its `implet` clause names. In-memory like the
    /// rest of this section: a decoded image does not carry it.
    pub struct_contracts: HashMap<DefId, HashSet<DefId>>,
    pub enum_type_params: HashMap<DefId, Vec<Symbol>>,
    pub optional_struct_fields: HashMap<DefId, HashSet<Symbol>>,
    pub nullable_struct_fields: HashMap<DefId, HashSet<Symbol>>,
    pub opaque_structs: HashSet<DefId>,
    pub json_struct_field_keys: HashMap<DefId, HashMap<Symbol, String>>,
    pub variant_parents: HashMap<DefId, DefId>,
    pub enum_variants: HashMap<DefId, Vec<DefId>>,
    pub variant_positions: HashMap<DefId, usize>,
    pub variant_fields: HashMap<DefId, HashMap<Symbol, MirType>>,
    pub closure_environments: Vec<MirClosureEnvironment>,
    /// Tier-1 `@ ad` route table (route → handler source `DefId`).
    pub ad_routes: HashMap<String, DefId>,
}

/// Snapshot the merged program's struct/variant validation surface from a
/// validated context (defs are stable across the merged type table).
pub(super) fn snapshot_validation_metadata(
    validation: &radix_mir::MirValidationContext<'_>,
) -> FmirValidationMetadata {
    let mut closure_environments = validation
        .closure_environments
        .values()
        .cloned()
        .collect::<Vec<_>>();
    closure_environments.sort_by_key(|environment| environment.id.0);
    FmirValidationMetadata {
        struct_fields: validation
            .struct_fields
            .iter()
            .map(|(def, fields)| (*def, fields.iter().map(|(name, ty)| (*name, *ty)).collect()))
            .collect(),
        struct_field_order: validation
            .struct_field_order
            .iter()
            .map(|(def, order)| (*def, order.clone()))
            .collect(),
        variant_field_order: validation
            .variant_field_order
            .iter()
            .map(|(def, order)| (*def, order.clone()))
            .collect(),
        display_names: validation.display_names.clone().into_iter().collect(),
        display_tokens: validation.display_tokens.clone(),
        struct_type_params: validation
            .struct_type_params
            .iter()
            .map(|(def, params)| (*def, params.clone()))
            .collect(),
        struct_contracts: validation
            .struct_contracts
            .iter()
            .map(|(def, contracts)| (*def, contracts.iter().copied().collect()))
            .collect(),
        enum_type_params: validation
            .enum_type_params
            .iter()
            .map(|(def, params)| (*def, params.clone()))
            .collect(),
        optional_struct_fields: validation
            .optional_struct_fields
            .iter()
            .map(|(def, fields)| (*def, fields.iter().copied().collect()))
            .collect(),
        nullable_struct_fields: validation
            .nullable_struct_fields
            .iter()
            .map(|(def, fields)| (*def, fields.iter().copied().collect()))
            .collect(),
        opaque_structs: validation.opaque_structs.iter().copied().collect(),
        json_struct_field_keys: validation
            .json_struct_field_keys
            .iter()
            .map(|(def, keys)| {
                (
                    *def,
                    keys.iter()
                        .map(|(key, value)| (*key, value.clone()))
                        .collect(),
                )
            })
            .collect(),
        variant_parents: validation
            .variant_parents
            .iter()
            .map(|(variant, parent)| (*variant, *parent))
            .collect(),
        enum_variants: validation
            .enum_variants
            .iter()
            .map(|(enum_def, variants)| (*enum_def, variants.clone()))
            .collect(),
        variant_positions: validation
            .variant_positions
            .iter()
            .map(|(variant, position)| (*variant, *position))
            .collect(),
        variant_fields: validation
            .variant_fields
            .iter()
            .map(|(def, fields)| (*def, fields.iter().map(|(name, ty)| (*name, *ty)).collect()))
            .collect(),
        closure_environments,
        ad_routes: validation
            .ad_routes
            .iter()
            .map(|(route, def)| (route.clone(), *def))
            .collect(),
    }
}

/// Apply carried struct/variant metadata to a freshly built validation
/// context before re-validating the merged program.
pub(super) fn apply_validation_metadata(
    context: &mut radix_mir::MirValidationContext<'_>,
    metadata: &FmirValidationMetadata,
) {
    context.struct_fields = metadata
        .struct_fields
        .iter()
        .map(|(def, fields)| (*def, fields.iter().map(|(name, ty)| (*name, *ty)).collect()))
        .collect();
    context.struct_field_order = metadata
        .struct_field_order
        .iter()
        .map(|(def, order)| (*def, order.clone()))
        .collect();
    context.variant_field_order = metadata
        .variant_field_order
        .iter()
        .map(|(def, order)| (*def, order.clone()))
        .collect();
    context.display_names = metadata.display_names.clone().into_iter().collect();
    context.display_tokens = metadata.display_tokens.clone();
    context.struct_type_params = metadata
        .struct_type_params
        .iter()
        .map(|(def, params)| (*def, params.clone()))
        .collect();
    context.struct_contracts = metadata
        .struct_contracts
        .iter()
        .map(|(def, contracts)| (*def, contracts.iter().copied().collect()))
        .collect();
    context.enum_type_params = metadata
        .enum_type_params
        .iter()
        .map(|(def, params)| (*def, params.clone()))
        .collect();
    context.optional_struct_fields = metadata
        .optional_struct_fields
        .iter()
        .map(|(def, fields)| (*def, fields.iter().copied().collect()))
        .collect();
    context.nullable_struct_fields = metadata
        .nullable_struct_fields
        .iter()
        .map(|(def, fields)| (*def, fields.iter().copied().collect()))
        .collect();
    context.opaque_structs = metadata.opaque_structs.iter().copied().collect();
    context.json_struct_field_keys = metadata
        .json_struct_field_keys
        .iter()
        .map(|(def, keys)| {
            (
                *def,
                keys.iter()
                    .map(|(key, value)| (*key, value.clone()))
                    .collect(),
            )
        })
        .collect();
    context.variant_parents = metadata
        .variant_parents
        .iter()
        .map(|(variant, parent)| (*variant, *parent))
        .collect();
    context.enum_variants = metadata
        .enum_variants
        .iter()
        .map(|(enum_def, variants)| (*enum_def, variants.clone()))
        .collect();
    context.variant_positions = metadata
        .variant_positions
        .iter()
        .map(|(variant, position)| (*variant, *position))
        .collect();
    context.variant_fields = metadata
        .variant_fields
        .iter()
        .map(|(def, fields)| (*def, fields.iter().map(|(name, ty)| (*name, *ty)).collect()))
        .collect();
    context.closure_environments = metadata
        .closure_environments
        .iter()
        .map(|environment| (environment.id, environment.clone()))
        .collect();
    context.ad_routes = metadata
        .ad_routes
        .iter()
        .map(|(route, def)| (route.clone(), *def))
        .collect();
}

pub(super) fn fmir_package_image_from_lowered(
    prepared: &PreparedPackageMir<'_>,
    lowered: &LoweredMirUnit<'_>,
    diagnostic_path: PathBuf,
    format: FmirPackageImageFormat,
    seams: &MirSeams,
) -> Result<FmirPackageImage, Vec<Diagnostic>> {
    // The device-section construction is a faber-supplied seam (ownership-
    // freeze §5.1): without `device-runtime` faber's builder returns `None`.
    let device = (seams.device_section_builder)(prepared, lowered, &diagnostic_path)?;
    // Source identities ride the image when the package carries a device
    // payload (the A10 identity's source half): the source-format route has
    // the real package files on disk. The FHIR-loaded route reconstructs a
    // package from an envelope and never constructs a device payload, so its
    // (non-existent on disk) source paths are never read here.
    let source_hashes = if device.is_some() {
        prepared
            .source_paths
            .iter()
            .map(|path| fs::read(path).map(|bytes| fnv64_hex(&bytes)))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                vec![mir_diag(
                    &prepared.entry_path,
                    format!("could not read source identity: {error}"),
                )]
            })?
    } else {
        Vec::new()
    };
    let (types, interner) = snapshot_context(lowered);
    Ok(FmirPackageImage {
        diagnostic_path,
        format,
        entry_function: "run_entry".to_owned(),
        runtime_requirements: prepared.runtime_requirements.clone(),
        cli: prepared.fmir_text_cli.clone(),
        exit_code: prepared.cli_exit_code,
        types,
        interner,
        program: lowered.validated.program().clone(),
        device,
        source_hashes,
        validation: snapshot_validation_metadata(lowered.validated.validation()),
    })
}

pub(super) fn run_fmir_package_image<H: Host + ?Sized>(
    mut image: FmirPackageImage,
    host: &mut H,
    kernel_policy: RunnerKernelPolicy,
) -> Result<(), Vec<Diagnostic>> {
    check_fmir_runtime_requirements(&image)?;
    let types = TypeTable::from_snapshot(image.types).map_err(|error| {
        vec![
            mir_issue_diag(
                &image.diagnostic_path,
                "fmir_image_type_metadata_invalid",
                format!(
                    "could not load {} type metadata: {error}",
                    image.format.label()
                ),
            )
            .with_arg("format", image.format.label()),
        ]
    })?;
    let mut interner = radix_module::lexer::Interner::from_strings(image.interner);
    bind_fmir_text_runtime_cli(
        &mut image.program,
        image.cli.as_ref(),
        &image.entry_function,
        &mut interner,
        host,
        &image.diagnostic_path,
    )?;
    let mut validation = radix_mir::MirValidationContext::new(&types);
    validation.interner = radix_mir::MirInterner::Borrowed(&interner);
    apply_validation_metadata(&mut validation, &image.validation);
    let validated =
        radix_mir::ValidatedMir::new(image.program.clone(), validation).map_err(|errors| {
            errors
                .into_iter()
                .map(|error| mir_diag(&image.diagnostic_path, error.message))
                .collect::<Vec<_>>()
        })?;
    run_entry_with_process_exit(&validated, host, kernel_policy)
        .map_err(|errors| runner_diagnostics(&image.diagnostic_path, errors))?;
    if let Some(code) = image.exit_code {
        host.exit(code);
    }
    Ok(())
}

/// [`run_fmir_package_image`] with the opt-in diagnostic meter armed (RSM
/// CPU/FMIR stats outcome): pre-execution failures (runtime-requirement
/// check, type metadata, CLI binding, validation) remain the outer `Err`
/// without a summary; once the entry frame is pushed, runtime diagnostics
/// are retained in [`PackageRunStats::result`] beside the collected summary.
/// The declared exit code applies only to a successful started run, matching
/// the unmetered route.
pub(super) fn run_fmir_package_image_with_stats<H: Host + ?Sized>(
    mut image: FmirPackageImage,
    host: &mut H,
    kernel_policy: RunnerKernelPolicy,
) -> Result<PackageRunStats, Vec<Diagnostic>> {
    check_fmir_runtime_requirements(&image)?;
    let types = TypeTable::from_snapshot(image.types).map_err(|error| {
        vec![
            mir_issue_diag(
                &image.diagnostic_path,
                "fmir_image_type_metadata_invalid",
                format!(
                    "could not load {} type metadata: {error}",
                    image.format.label()
                ),
            )
            .with_arg("format", image.format.label()),
        ]
    })?;
    let mut interner = radix_module::lexer::Interner::from_strings(image.interner);
    bind_fmir_text_runtime_cli(
        &mut image.program,
        image.cli.as_ref(),
        &image.entry_function,
        &mut interner,
        host,
        &image.diagnostic_path,
    )?;
    let mut validation = radix_mir::MirValidationContext::new(&types);
    validation.interner = radix_mir::MirInterner::Borrowed(&interner);
    apply_validation_metadata(&mut validation, &image.validation);
    let validated =
        radix_mir::ValidatedMir::new(image.program.clone(), validation).map_err(|errors| {
            errors
                .into_iter()
                .map(|error| mir_diag(&image.diagnostic_path, error.message))
                .collect::<Vec<_>>()
        })?;
    let outcome = run_entry_with_stats_and_kernel_policy(&validated, host, kernel_policy)
        .map_err(|errors| runner_diagnostics(&image.diagnostic_path, errors))?;
    let result = outcome
        .result
        .map_err(|errors| runner_diagnostics(&image.diagnostic_path, errors));
    if result.is_ok() {
        // Ruling Q-T8: the entry tail lowered by the MIR entry pass evaluates
        // the exit expression, so a `numerus` return becomes the process
        // status. The serialized constant stays the fallback for images whose
        // entry returns vacuum (pre-existing `exit <literal>` artifacts).
        if let Some(radix_mir_runner::Value::Int(code)) = outcome.return_value {
            #[allow(clippy::cast_possible_truncation)]
            let exit_code = code as i32;
            host.exit(exit_code);
        } else if let Some(code) = image.exit_code {
            host.exit(code);
        }
    }
    Ok(PackageRunStats {
        stats: outcome.stats,
        result,
    })
}

impl FmirPackageImageFormat {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Source => "source-built FMIR",
            Self::FmirText => "fmir-text",
            Self::Fmir => "fmir",
        }
    }
}

/// Snapshot the validated type table and interner strings carried by FMIR
/// images (text, binary, and source-built formats share the snapshot shape).
pub(super) fn snapshot_context(lowered: &LoweredMirUnit<'_>) -> (TypeTableSnapshot, Vec<String>) {
    (
        lowered.validated.validation().types.snapshot(),
        lowered
            .validated
            .validation()
            .interner
            .as_interner()
            .map(|interner| interner.strings().to_vec())
            .unwrap_or_default(),
    )
}

/// Common FMIR image header section — the fields the text and binary image
/// formats share verbatim. Each packager fills in only the format-specific
/// program payload and encode step.
pub(super) struct FmirImageHeader {
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
    device: Option<FmirDeviceSection>,
}

pub(super) fn fmir_image_header(
    prepared: &PreparedPackageMir<'_>,
    lowered: &LoweredMirUnit<'_>,
    package_root: &Path,
    target: &str,
    seams: &MirSeams,
) -> Result<FmirImageHeader, Vec<Diagnostic>> {
    let mut sources = prepared
        .source_paths
        .iter()
        .map(|path| source_identity(path, package_root))
        .collect::<Result<Vec<_>, _>>()?;
    sources.sort_by(|left, right| left.file.cmp(&right.file));
    // The device-section construction is a faber-supplied seam (ownership-
    // freeze §5.1): without `device-runtime` faber's builder returns `None`.
    let device = (seams.device_section_builder)(prepared, lowered, &prepared.entry_path)?;
    let (types, interner) = snapshot_context(lowered);
    Ok(FmirImageHeader {
        version: PACKAGE_MIR_ARTIFACT_VERSION,
        target: target.to_owned(),
        package_root: ".".to_owned(),
        entry: relative_or_display(package_root, &prepared.entry_path),
        entry_function: "run_entry".to_owned(),
        toolchain: FmirTextToolchainSection {
            faber_cli_version: seams.toolchain_version.to_owned(),
        },
        runtime: FmirTextRuntimeSection {
            requirement: prepared.runtime_requirements.clone(),
        },
        sources: FmirTextSourcesSection { source: sources },
        cli: prepared.fmir_text_cli.clone(),
        exit_code: prepared.cli_exit_code,
        types: FmirTextTypesSection { table: types },
        interner,
        device,
    })
}

impl FmirImageHeader {
    fn into_text_image(self, json: String) -> FmirTextImageFile {
        let FmirImageHeader {
            version,
            target,
            package_root,
            entry,
            entry_function,
            toolchain,
            runtime,
            sources,
            cli,
            exit_code,
            types,
            interner,
            device,
        } = self;
        FmirTextImageFile {
            version,
            target,
            package_root,
            entry,
            entry_function,
            toolchain,
            runtime,
            sources,
            cli,
            exit_code,
            types,
            interner,
            program: FmirTextProgramSection { json },
            device,
        }
    }

    fn into_binary_image(self, program: MirProgram) -> FmirBinaryImageFile {
        let FmirImageHeader {
            version,
            target,
            package_root,
            entry,
            entry_function,
            toolchain,
            runtime,
            sources,
            cli,
            exit_code,
            types,
            interner,
            device,
        } = self;
        FmirBinaryImageFile {
            version,
            target,
            package_root,
            entry,
            entry_function,
            toolchain,
            runtime,
            sources,
            cli,
            exit_code,
            types,
            interner,
            program,
            device,
        }
    }
}

pub(super) fn package_fmir_text_image(
    prepared: &PreparedPackageMir<'_>,
    lowered: &LoweredMirUnit<'_>,
    package_root: &Path,
    seams: &MirSeams,
) -> Result<String, Vec<Diagnostic>> {
    let program_json =
        serde_json::to_string_pretty(&lowered.validated.program()).map_err(|error| {
            vec![mir_diag(
                &prepared.entry_path,
                format!("could not encode fmir-text program: {error}"),
            )]
        })?;
    let image = fmir_image_header(
        prepared,
        lowered,
        package_root,
        FMIR_TEXT_TARGET_NAME,
        seams,
    )?
    .into_text_image(program_json);
    encode_text_image(&image).map_err(|error| {
        vec![mir_diag(
            &prepared.entry_path,
            format!("could not encode fmir-text image: {error}"),
        )]
    })
}

pub(super) fn package_fmir_binary_image(
    prepared: &PreparedPackageMir<'_>,
    lowered: &LoweredMirUnit<'_>,
    package_root: &Path,
    seams: &MirSeams,
) -> Result<Vec<u8>, Vec<Diagnostic>> {
    let image = fmir_image_header(prepared, lowered, package_root, FMIR_TARGET_NAME, seams)?
        .into_binary_image(lowered.validated.program().clone());
    encode_binary_image(&image).map_err(|error| {
        vec![mir_diag(
            &prepared.entry_path,
            format!("could not encode fmir image: {error}"),
        )]
    })
}

/// Decoded FMIR image payload fields shared by the text and binary loaders;
/// only the program representation differs (JSON string vs decoded MIR).
pub(super) struct FmirImagePayload {
    entry_function: String,
    runtime_requirement: Vec<String>,
    interner: Vec<String>,
    cli: Option<FmirTextCliSection>,
    exit_code: Option<i32>,
    types: TypeTableSnapshot,
    program: FmirImageProgram,
    device: Option<FmirDeviceSection>,
    source_hashes: Vec<String>,
}

pub(super) enum FmirImageProgram {
    Text { json: String },
    Binary(MirProgram),
}

impl From<FmirTextImageFile> for FmirImagePayload {
    fn from(image: FmirTextImageFile) -> Self {
        Self {
            entry_function: image.entry_function,
            runtime_requirement: image.runtime.requirement,
            interner: image.interner,
            cli: image.cli,
            exit_code: image.exit_code,
            types: image.types.table,
            program: FmirImageProgram::Text {
                json: image.program.json,
            },
            device: image.device,
            source_hashes: image
                .sources
                .source
                .iter()
                .map(|identity| identity.hash.clone())
                .collect(),
        }
    }
}

impl From<FmirBinaryImageFile> for FmirImagePayload {
    fn from(image: FmirBinaryImageFile) -> Self {
        Self {
            entry_function: image.entry_function,
            runtime_requirement: image.runtime.requirement,
            interner: image.interner,
            cli: image.cli,
            exit_code: image.exit_code,
            types: image.types.table,
            program: FmirImageProgram::Binary(image.program),
            device: image.device,
            source_hashes: image
                .sources
                .source
                .iter()
                .map(|identity| identity.hash.clone())
                .collect(),
        }
    }
}

/// Load an FMIR image (text or binary) into an in-memory package image:
/// decode → map `FmirImageError` to diagnostics → decode the program payload
/// → assemble the package image. `decode` supplies the format-specific
/// decoder and payload conversion; `style` supplies the format-specific
/// diagnostic wording.
pub(super) fn load_fmir_image_with<D>(
    path: &Path,
    format: FmirPackageImageFormat,
    style: FmirImageErrorStyle,
    decode: D,
) -> Result<FmirPackageImage, Vec<Diagnostic>>
where
    D: FnOnce() -> Result<FmirImagePayload, FmirImageError>,
{
    let payload = decode().map_err(|error| fmir_image_decode_error(error, path, &style))?;
    let FmirImagePayload {
        entry_function,
        runtime_requirement,
        interner,
        cli,
        exit_code,
        types,
        program,
        device,
        source_hashes,
    } = payload;
    let program = match program {
        FmirImageProgram::Text { json } => serde_json::from_str(&json).map_err(|error| {
            vec![
                mir_diag(
                    path,
                    format!("could not decode fmir-text MIR program: {error}"),
                )
                .with_arg("detail", error.to_string()),
            ]
        })?,
        FmirImageProgram::Binary(program) => program,
    };
    Ok(FmirPackageImage {
        diagnostic_path: path.to_path_buf(),
        format,
        entry_function,
        runtime_requirements: runtime_requirement,
        interner,
        cli,
        exit_code,
        types,
        program,
        device,
        source_hashes,
        validation: FmirValidationMetadata::default(),
    })
}

/// FMIR image decode-error style: the text and binary loaders report
/// identical diagnostics except for the format label in message text and how
/// the parse arm is reported.
pub(super) struct FmirImageErrorStyle {
    /// Format label embedded in message text: `fmir-text` or `fmir`.
    label: &'static str,
    /// Parse-arm verb: `parse` (text) vs `decode` (binary).
    parse_verb: &'static str,
    /// Issue code for the parse arm (text reports an issue; binary reports a
    /// plain diagnostic).
    parse_issue: Option<&'static str>,
    /// Issue code for the unsupported-version arm.
    version_issue: &'static str,
}

pub(super) fn fmir_image_decode_error(
    error: FmirImageError,
    path: &Path,
    style: &FmirImageErrorStyle,
) -> Vec<Diagnostic> {
    match error {
        FmirImageError::Parse(detail) => match style.parse_issue {
            Some(issue) => vec![mir_issue_diag(
                path,
                issue,
                format!(
                    "could not {} {} image: {detail}",
                    style.parse_verb, style.label
                ),
            )
            .with_arg("detail", detail)],
            None => vec![mir_diag(
                path,
                format!(
                    "could not {} {} image: {detail}",
                    style.parse_verb, style.label
                ),
            )
            .with_arg("detail", detail)],
        },
        FmirImageError::VersionMismatch { actual, expected } => vec![mir_issue_diag(
            path,
            style.version_issue,
            format!(
                "unsupported {} image version {actual}; expected {expected}",
                style.label
            ),
        )
        .with_arg("actual", actual.to_string())
        .with_arg("expected", expected.to_string())],
        FmirImageError::TargetMismatch { found, expected } => vec![
            mir_diag(
                path,
                format!("{} image target must be `{expected}`", style.label),
            )
            .with_arg("found", found)
            .with_arg("expected", expected),
        ],
        FmirImageError::ToolchainMismatch { found, expected } => vec![mir_diag(
            path,
            format!(
                "unsupported {} image toolchain {found}; expected {expected}",
                style.label
            ),
        )
        .with_arg("found", found)
        .with_arg("expected", expected)],
        FmirImageError::ArtifactDigestMismatch {
            backend,
            field,
            stored,
            computed,
        } => vec![mir_diag(
            path,
            format!(
                "{} image device artifact `{backend}` digest `{field}` mismatch (stored {stored}, recomputed {computed})",
                style.label
            ),
        )
        .with_arg("backend", backend)
        .with_arg("field", field)
        .with_arg("stored", stored)
        .with_arg("computed", computed)],
        FmirImageError::CompilerInputProvenance { backend, detail } => vec![mir_diag(
            path,
            format!(
                "{} image device artifact `{backend}` provenance violation: {detail}",
                style.label
            ),
        )
        .with_arg("backend", backend)
        .with_arg("detail", detail)],
        FmirImageError::NonAdmittedTargetFeature { feature } => vec![mir_diag(
            path,
            format!(
                "{} image device artifact requires unknown target feature `{feature}`",
                style.label
            ),
        )
        .with_arg("feature", feature)],
        FmirImageError::TargetIdMismatch {
            backend,
            target,
            expected,
        } => vec![mir_diag(
            path,
            format!(
                "{} image device artifact `{backend}` declares target `{target}`; expected `{expected}`",
                style.label
            ),
        )
        .with_arg("backend", backend)
        .with_arg("target", target)
        .with_arg("expected", expected)],
        FmirImageError::AbsentArtifactForSelection { selection } => vec![mir_diag(
            path,
            format!(
                "{} image device selection `{selection}` names no declared artifact of that backend",
                style.label
            ),
        )
        .with_arg("selection", selection)],
        FmirImageError::ArtifactEncodingMismatch { backend, detail } => vec![mir_diag(
            path,
            format!(
                "{} image device artifact `{backend}` payload-encoding mismatch: {detail}",
                style.label
            ),
        )
        .with_arg("backend", backend)
        .with_arg("detail", detail)],
        FmirImageError::NonAdmittedArtifactBackend { backend } => vec![mir_diag(
            path,
            format!(
                "{} image device artifact backend `{backend}` is not a compiled-in leaf",
                style.label
            ),
        )
        .with_arg("backend", backend)],
        // DDCP3-U0: artifact admission and selection-resolution failure
        // classes mapped to focused diagnostics (exact-match clean break).
        FmirImageError::ArtifactAbiVersionMismatch {
            backend,
            actual,
            accepted,
        } => vec![mir_issue_diag(
            path,
            "fmir_image_artifact_abi_version_unsupported",
            format!(
                "{} image device artifact `{backend}` declares ABI version {actual}; the accepted version is {accepted} — exact-match clean break",
                style.label
            ),
        )
        .with_arg("backend", backend)
        .with_arg("actual", actual.to_string())
        .with_arg("accepted", accepted.to_string())],
        FmirImageError::ArtifactReflectionVersionMismatch {
            backend,
            actual,
            expected,
        } => vec![mir_issue_diag(
            path,
            "fmir_image_artifact_reflection_version_unsupported",
            format!(
                "unsupported artifact reflection version {actual} for backend `{backend}`; expected {expected}"
            ),
        )
        .with_arg("backend", backend)
        .with_arg("actual", actual.to_string())
        .with_arg("expected", expected.to_string())],
        FmirImageError::IncompatibleArtifactCombination { backend, detail } => vec![
            mir_issue_diag(
                path,
                "fmir_image_artifact_combination_incompatible",
                format!(
                    "{} image device artifact `{backend}` combination is incompatible: {detail}",
                    style.label
                ),
            )
            .with_arg("backend", backend)
            .with_arg("detail", detail),
        ],
        FmirImageError::DuplicateRequiredFeature { feature } => vec![mir_issue_diag(
            path,
            "fmir_image_artifact_duplicate_required_feature",
            format!(
                "{} image device artifact declares target feature `{feature}` more than once — duplicate required features fail admission",
                style.label
            ),
        )
        .with_arg("feature", feature)],
        FmirImageError::DuplicateEntrypoint { entry } => vec![mir_issue_diag(
            path,
            "fmir_image_artifact_duplicate_entrypoint",
            format!(
                "{} image device artifact declares entrypoint `{entry}` more than once — duplicate entrypoint identities fail admission",
                style.label
            ),
        )
        .with_arg("entry", entry)],
        FmirImageError::NonAdmittedSelectedBackend { backend } => vec![mir_issue_diag(
            path,
            "fmir_image_selected_backend_unsupported",
            format!(
                "{} image device selection `{backend}` names an unknown backend — selection resolution fails closed (DDCP3-U0 preserve-until-selection)",
                style.label
            ),
        )
        .with_arg("backend", backend)],
        FmirImageError::DeviceProgramVersionMismatch { actual, expected } => vec![
            mir_issue_diag(
                path,
                "fmir_image_device_program_version_unsupported",
                format!("unsupported device-program wire version {actual}; expected {expected}"),
            )
            .with_arg("actual", actual.to_string())
            .with_arg("expected", expected.to_string()),
        ],
        FmirImageError::WireProgramInvalid { detail } => vec![mir_issue_diag(
            path,
            "fmir_image_device_program_invalid",
            format!("invalid device program in image: {detail}"),
        )
        .with_arg("detail", detail)],
        FmirImageError::ExecutorPlanSectionVersionMismatch { actual, expected } => vec![
            mir_issue_diag(
                path,
                "fmir_image_executor_plan_section_version_unsupported",
                format!("unsupported executor-plan section version {actual}; expected {expected}"),
            )
            .with_arg("actual", actual.to_string())
            .with_arg("expected", expected.to_string()),
        ],
        FmirImageError::ExecutorPlanSectionInvalid { detail } => vec![mir_issue_diag(
            path,
            "fmir_image_executor_plan_section_invalid",
            format!("invalid executor-plan section in image: {detail}"),
        )
        .with_arg("detail", detail)],
        // MD2-W1: the distributed execution section is admitted fail-closed
        // at load; each failure class maps to a focused diagnostic.
        FmirImageError::DistributedSectionVersionMismatch { actual, expected } => vec![
            mir_issue_diag(
                path,
                "fmir_image_distributed_section_version_unsupported",
                format!(
                    "unsupported distributed-section version {actual}; expected {expected}"
                ),
            )
            .with_arg("actual", actual.to_string())
            .with_arg("expected", expected.to_string()),
        ],
        FmirImageError::DistributedSectionInvalid { detail } => vec![mir_issue_diag(
            path,
            "fmir_image_distributed_section_invalid",
            format!("invalid distributed section in image: {detail}"),
        )
        .with_arg("detail", detail)],
        FmirImageError::DistributedSectionEquivalenceMismatch { detail } => vec![mir_issue_diag(
            path,
            "fmir_image_distributed_section_equivalence_mismatch",
            format!("distributed plan mismatch in image: {detail}"),
        )
        .with_arg("detail", detail)],
        FmirImageError::DistributedSectionHashMismatch {
            expected,
            recomputed,
        } => vec![mir_issue_diag(
            path,
            "fmir_image_distributed_section_hash_mismatch",
            format!(
                "distributed section logical-plan hash {expected} does not match the delivered plan (recomputed {recomputed})"
            ),
        )
        .with_arg("expected", expected)
        .with_arg("recomputed", recomputed)],
        // GI4-2: the cadence/session section is admitted fail-closed with its
        // own version ratchet, independently of the device-program wire and
        // outer image versions.
        FmirImageError::SessionSectionVersionMismatch { actual, expected } => vec![
            mir_issue_diag(
                path,
                "fmir_image_session_section_version_unsupported",
                format!("unsupported session-section version {actual}; expected {expected}"),
            )
            .with_arg("actual", actual.to_string())
            .with_arg("expected", expected.to_string()),
        ],
        FmirImageError::SessionSectionInvalid { detail } => vec![mir_issue_diag(
            path,
            "fmir_image_session_section_invalid",
            format!("invalid session section in image: {detail}"),
        )
        .with_arg("detail", detail)],
    }
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn load_fmir_text_image(
    text: &str,
    path: &Path,
    toolchain_version: &str,
) -> Result<FmirPackageImage, Vec<Diagnostic>> {
    load_fmir_image_with(
        path,
        FmirPackageImageFormat::FmirText,
        FmirImageErrorStyle {
            label: "fmir-text",
            parse_verb: "parse",
            parse_issue: Some("fmir_text_image_parse_failed"),
            version_issue: "fmir_text_image_version_unsupported",
        },
        || decode_text_image(text, toolchain_version).map(FmirImagePayload::from),
    )
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn load_fmir_image(
    bytes: &[u8],
    path: &Path,
    toolchain_version: &str,
) -> Result<FmirPackageImage, Vec<Diagnostic>> {
    load_fmir_image_with(
        path,
        FmirPackageImageFormat::Fmir,
        FmirImageErrorStyle {
            label: "fmir",
            parse_verb: "decode",
            parse_issue: None,
            version_issue: "fmir_image_version_unsupported",
        },
        || decode_binary_image(bytes, toolchain_version).map(FmirImagePayload::from),
    )
}

#[cfg(test)]
#[path = "image_test.rs"]
mod tests;
