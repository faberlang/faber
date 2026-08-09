//! FMIR image construction and loading (source-built, text, and binary formats).

use super::*;

/// Struct/variant validation metadata carried by in-memory package FMIR
/// images (codex-gap S1 U2 VALUE members).
///
/// The source-built run path re-validates the merged program inside
/// [`run_fmir_package_image`] with a fresh [`radix::mir::MirValidationContext`].
/// Enum-variant constructs require the merged variant surface (parent
/// metadata, variant lists, variant field types) to validate — an empty
/// context errors with "enum variant is missing parent metadata". The image
/// therefore carries the maps the merge proved; the decoded-artifact routes
/// (fmir / fmir-text wire formats) do not serialize this section and keep the
/// previous empty-metadata behavior for those images.
#[derive(Debug, Clone, Default)]
pub(super) struct FmirValidationMetadata {
    pub struct_fields: HashMap<DefId, HashMap<Symbol, MirType>>,
    pub struct_type_params: HashMap<DefId, Vec<Symbol>>,
    pub enum_type_params: HashMap<DefId, Vec<Symbol>>,
    pub optional_struct_fields: HashMap<DefId, HashSet<Symbol>>,
    pub nullable_struct_fields: HashMap<DefId, HashSet<Symbol>>,
    pub opaque_structs: HashSet<DefId>,
    pub json_struct_field_keys: HashMap<DefId, HashMap<Symbol, String>>,
    pub variant_parents: HashMap<DefId, DefId>,
    pub enum_variants: HashMap<DefId, Vec<DefId>>,
    pub variant_positions: HashMap<DefId, usize>,
    pub variant_fields: HashMap<DefId, HashMap<Symbol, MirType>>,
}

/// Snapshot the merged program's struct/variant validation surface from a
/// validated context (defs are stable across the merged type table).
fn snapshot_validation_metadata(
    validation: &radix::mir::MirValidationContext<'_>,
) -> FmirValidationMetadata {
    FmirValidationMetadata {
        struct_fields: validation
            .struct_fields
            .iter()
            .map(|(def, fields)| (*def, fields.iter().map(|(name, ty)| (*name, *ty)).collect()))
            .collect(),
        struct_type_params: validation
            .struct_type_params
            .iter()
            .map(|(def, params)| (*def, params.clone()))
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
            .map(|(def, keys)| (*def, keys.iter().map(|(key, value)| (*key, value.clone())).collect()))
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
    }
}

/// Apply carried struct/variant metadata to a freshly built validation
/// context before re-validating the merged program.
fn apply_validation_metadata(
    context: &mut radix::mir::MirValidationContext<'_>,
    metadata: &FmirValidationMetadata,
) {
    context.struct_fields = metadata
        .struct_fields
        .iter()
        .map(|(def, fields)| (*def, fields.iter().map(|(name, ty)| (*name, *ty)).collect()))
        .collect();
    context.struct_type_params = metadata
        .struct_type_params
        .iter()
        .map(|(def, params)| (*def, params.clone()))
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
        .map(|(def, keys)| (*def, keys.iter().map(|(key, value)| (*key, value.clone())).collect()))
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
}

/// Construct the packaged device section for a prepared package, when the
/// package declares a `[device]` surface (S1-6): the device-program
/// constructor scans the lowered MIR for `@ nucleum` compute kernels, emits
/// the Metal MSL + CUDA PTX artifacts (S1-3 emitters), and assembles the
/// canonical payload + selection + runtime requirements. `None` for packages
/// without a device declaration (no device payload; the CPU route is
/// unchanged).
pub(super) fn package_device_section(
    prepared: &PreparedPackageMir<'_>,
    lowered: &LoweredMirUnit<'_>,
    diagnostic_path: &Path,
) -> Result<Option<FmirDeviceSection>, Vec<Diagnostic>> {
    if !prepared.device_declared {
        return Ok(None);
    }
    // S5-U5b: the manifest `[device] steps` channel (or the portable default)
    // is the declared repeating step count the constructor admits against the
    // source loop bound and the wire carries.
    let declared_steps = prepared
        .device_steps
        .unwrap_or(super::super::device::DEFAULT_TRAINING_STEPS);
    let Some((program, semantics, step_count)) = super::super::device::device_program_for_lowered(
        &lowered.validated,
        &lowered.interner,
        &lowered.companions,
        declared_steps,
    )?
    else {
        // The selected entry lowers to no compute kernel: no device payload.
        // An explicit GPU request for this package fails closed at run time
        // (N1.1: "package has no device program"); `auto` keeps the CPU
        // route. (A `faber script` capture runner in the same package —
        // e.g. a CPU oracle — legitimately has no kernel.)
        return Ok(None);
    };
    // S5-U5b: the `[device] steps` channel applies only to a RepeatingStep
    // training program. A declared count on a device program the constructor
    // materialized as SingleRun is a contradiction — fail closed, never a
    // silently dropped count.
    if prepared.device_steps.is_some()
        && program.lifetime != radix_mir::device_program::DeviceProgramLifetime::RepeatingStep
    {
        return Err(vec![crate::package_diagnostic_error(
            "faber.toml device.steps applies only to a training-loop package; this package's device program is not a RepeatingStep training program",
        )
        .with_file(diagnostic_path.display().to_string())
        .with_arg("issue", "package_device_steps_not_repeating")]);
    }
    // Fail closed when a declared input is missing for an input buffer. The
    // TWO exemptions (S3-A5 + S5-U1): the companion's reverse-AD upstream
    // seed — a synthetic 1-element input whose source param carries no
    // source name — is provisioned with 1.0; and a DECOMPOSED kernel's
    // data-flow input that another kernel in the program produces (a forward
    // intermediate) needs no external value. The materializer provisions
    // the seed; the program unifies the intermediates.
    let mut device_inputs = prepared.device_inputs.clone();
    let mut upstream_seeds: Vec<(String, Vec<f32>)> = Vec::new();
    // Buffers any kernel writes: produced-by-the-program intermediates (and
    // the InOut trainable params, which stay declared inputs with initial
    // values from the manifest).
    let produced: std::collections::BTreeSet<String> = program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .filter(|resource| {
            matches!(
                resource.buffer.role,
                radix_mir::device_program::BufferRole::Output
            )
        })
        .map(|resource| resource.buffer.name.clone())
        .collect();
    for kernel in &program.kernels {
        let Some(function) = lowered
            .validated
            .program()
            .functions
            .iter()
            .find(|function| function.id == kernel.function)
        else {
            continue;
        };
        // The reverse-AD upstream seed: the companion's anonymous scalar
        // param (no source name, or a synthetic uninternable symbol) is
        // provisioned with 1.0. The seed is detected from the FUNCTION's
        // params — a decomposed subchain kernel's Input resources do not
        // positionally align with the whole-function signature's Input
        // resources (the subchain carries a subset of the function's reads
        // in its own binding order), so the old signature↔kernel positional
        // zip mispaired the seed against a named param and never provisioned
        // it (the D-5 fallout `mlp_backward__1 input_4` failure).
        let has_anonymous_param = function.params.iter().any(|param| {
            param.name.is_none()
                || param
                    .name
                    .is_some_and(|symbol| (symbol.0 as usize) >= lowered.interner.strings().len())
        });
        // The seed scan runs for EVERY kernel — a decomposed companion's
        // whole-function signature does not derive (the S5-U1 skip below),
        // but its subchain kernels still carry the 1-element seed (a
        // tensor-return companion's f32 upstream). Gating the scan on the
        // signature left the seed unprovisioned and the RepeatingStep
        // once-init check failed at run time (the bert-tiny `input_4`
        // gap): a 1-element Input buffer the program does not produce and
        // the manifest does not declare is the reverse-AD seed.
        if has_anonymous_param {
            for program_name in kernel.resources.iter().filter(|resource| {
                resource.buffer.role == radix_mir::device_program::BufferRole::Input
            }) {
                if program_name.version.element_count == 1
                    && !device_inputs.contains_key(&program_name.buffer.name)
                    && !produced.contains(&program_name.buffer.name)
                {
                    upstream_seeds.push((program_name.buffer.name.clone(), vec![1.0]));
                }
            }
        }
        // A decomposed kernel (S5-U1) whose source function mixes recipes —
        // e.g. the scalar-return lane, whose whole signature legitimately
        // fails the return-buffer equality law — has no whole-function
        // signature to derive; its subchain signatures were already
        // validated by the constructor, so skip it.
        let Ok(_signature) = radix_mir::abi::MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
            function, lowered.validated.validation(), &lowered.interner,
        ) else {
            continue;
        };
        for program_name in kernel
            .resources
            .iter()
            .filter(|resource| resource.buffer.role == radix_mir::device_program::BufferRole::Input)
        {
            // A companion's 1-element input (the reverse-AD seed) is
            // provisioned above (or declared in the manifest) — never a
            // missing-input failure.
            if has_anonymous_param && program_name.version.element_count == 1 {
                continue;
            }
            if !device_inputs.contains_key(&program_name.buffer.name) {
                // A produced-by-the-program data-flow input (S5-U1 forward
                // intermediate) needs no external value — skip, don't fail.
                if produced.contains(&program_name.buffer.name) {
                    continue;
                }
                return Err(vec![mir_issue_diag(
                    diagnostic_path,
                    "package_device_input_missing",
                    format!(
                        "[device] inputs has no value for kernel `{}` input buffer `{}`",
                        kernel.entry, program_name.buffer.name
                    ),
                )
                .with_arg("buffer", program_name.buffer.name.clone())]);
            }
        }
    }
    for (name, values) in upstream_seeds {
        device_inputs.insert(name, values);
    }
    let selection = prepared
        .device_backend
        .unwrap_or(faber::device::DeviceSelection::Auto);
    let section = super::super::device::device_section_for_program(
        &program,
        &semantics,
        &lowered.validated,
        &lowered.interner,
        super::super::device::DeviceSectionBuild {
            selection,
            inputs: &device_inputs,
            ptx_target: S1_6_PTX_TARGET,
            repeating_steps: step_count,
        },
    )?;
    Ok(Some(section))
}

/// The NVPTX target the S1-6 device images compile for.
///
/// `sm_90` is the highest arch the pinned pharos build-time compiler
/// (Ubuntu clang 18.1.3, N1.10) supports — `sm_120` (the RTX 5070's native
/// arch) is rejected by that clang. PTX is loaded via `cuModuleLoadData` and
/// JIT-compiled by the driver at module load (N1.3 §3.1), so the `.target`
/// is a minimum-arch declaration: `sm_90` PTX runs on sm_120 exactly as the
/// G4/G5 pharos receipt's default-target PTX did.
pub(super) const S1_6_PTX_TARGET: &str = "sm_90";

pub(super) fn fmir_package_image_from_lowered(
    prepared: &PreparedPackageMir<'_>,
    lowered: &LoweredMirUnit<'_>,
    diagnostic_path: PathBuf,
    format: FmirPackageImageFormat,
) -> Result<FmirPackageImage, Vec<Diagnostic>> {
    let device = package_device_section(prepared, lowered, &diagnostic_path)?;
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
        program: lowered.program.clone(),
        device,
        source_hashes,
        validation: snapshot_validation_metadata(lowered.validated.validation()),
    })
}

pub(super) fn run_fmir_package_image<H: Host + ?Sized>(
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
    apply_validation_metadata(&mut validation, &image.validation);
    let validated =
        radix::mir::ValidatedMir::new(image.program.clone(), validation).map_err(|errors| {
            errors
                .into_iter()
                .map(|error| mir_diag(&image.diagnostic_path, error.message))
                .collect::<Vec<_>>()
        })?;
    run_entry(&validated, host)
        .map_err(|errors| stepper_diagnostics(&image.diagnostic_path, errors))?;
    if let Some(code) = image.exit_code {
        host.exit(code);
    }
    Ok(())
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
) -> Result<FmirImageHeader, Vec<Diagnostic>> {
    let mut sources = prepared
        .source_paths
        .iter()
        .map(|path| source_identity(path, package_root))
        .collect::<Result<Vec<_>, _>>()?;
    sources.sort_by(|left, right| left.file.cmp(&right.file));
    let device = package_device_section(prepared, lowered, &prepared.entry_path)?;
    let (types, interner) = snapshot_context(lowered);
    Ok(FmirImageHeader {
        version: PACKAGE_MIR_ARTIFACT_VERSION,
        target: target.to_owned(),
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
) -> Result<String, Vec<Diagnostic>> {
    let program_json = serde_json::to_string_pretty(&lowered.program).map_err(|error| {
        vec![mir_diag(
            &prepared.entry_path,
            format!("could not encode fmir-text program: {error}"),
        )]
    })?;
    let image = fmir_image_header(prepared, lowered, package_root, FMIR_TEXT_TARGET_NAME)?
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
) -> Result<Vec<u8>, Vec<Diagnostic>> {
    let image = fmir_image_header(prepared, lowered, package_root, FMIR_TARGET_NAME)?
        .into_binary_image(lowered.program.clone());
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
            vec![mir_diag(
                path,
                format!("could not decode fmir-text MIR program: {error}"),
            )]
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
            )],
            None => vec![mir_diag(
                path,
                format!(
                    "could not {} {} image: {detail}",
                    style.parse_verb, style.label
                ),
            )],
        },
        FmirImageError::UnsupportedVersion { actual, expected } => vec![mir_issue_diag(
            path,
            style.version_issue,
            format!(
                "unsupported {} image version {actual}; expected {expected}",
                style.label
            ),
        )
        .with_arg("actual", actual.to_string())
        .with_arg("expected", expected.to_string())],
        FmirImageError::WrongTarget { found: _, expected } => vec![mir_diag(
            path,
            format!("{} image target must be `{expected}`", style.label),
        )],
        FmirImageError::UnsupportedToolchain { found, expected } => vec![mir_diag(
            path,
            format!(
                "unsupported {} image toolchain {found}; expected {expected}",
                style.label
            ),
        )],
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
        )],
        FmirImageError::CompilerInputProvenance { backend, detail } => vec![mir_diag(
            path,
            format!(
                "{} image device artifact `{backend}` provenance violation: {detail}",
                style.label
            ),
        )],
        FmirImageError::UnsupportedTargetFeature { feature } => vec![mir_diag(
            path,
            format!(
                "{} image device artifact requires unknown target feature `{feature}`",
                style.label
            ),
        )],
        FmirImageError::IncompatibleTargetId {
            backend,
            target,
            expected,
        } => vec![mir_diag(
            path,
            format!(
                "{} image device artifact `{backend}` declares target `{target}`; expected `{expected}`",
                style.label
            ),
        )],
        FmirImageError::MissingArtifactForSelection { selection } => vec![mir_diag(
            path,
            format!(
                "{} image device selection `{selection}` names no declared artifact of that backend",
                style.label
            ),
        )],
        FmirImageError::ArtifactEncodingMismatch { backend, detail } => vec![mir_diag(
            path,
            format!(
                "{} image device artifact `{backend}` payload-encoding mismatch: {detail}",
                style.label
            ),
        )],
        FmirImageError::UnsupportedArtifactBackend { backend } => vec![mir_diag(
            path,
            format!(
                "{} image device artifact backend `{backend}` is not a compiled-in leaf",
                style.label
            ),
        )],
        FmirImageError::UnsupportedDeviceProgramVersion { actual, expected } => vec![
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
        )],
        // MD2-W1: the distributed execution section is admitted fail-closed
        // at load; each failure class maps to a focused diagnostic.
        FmirImageError::UnsupportedDistributedSectionVersion { actual, expected } => vec![
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
        )],
        FmirImageError::DistributedSectionEquivalenceMismatch { detail } => vec![mir_issue_diag(
            path,
            "fmir_image_distributed_section_equivalence_mismatch",
            format!("distributed plan mismatch in image: {detail}"),
        )],
        FmirImageError::DistributedSectionHashMismatch {
            expected,
            recomputed,
        } => vec![mir_issue_diag(
            path,
            "fmir_image_distributed_section_hash_mismatch",
            format!(
                "distributed section logical-plan hash {expected} does not match the delivered plan (recomputed {recomputed})"
            ),
        )],
        // GI4-2: the cadence/session section is admitted fail-closed with its
        // own version ratchet, independently of the device-program wire and
        // outer image versions.
        FmirImageError::UnsupportedSessionSectionVersion { actual, expected } => vec![
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
        )],
    }
}

pub(super) fn load_fmir_text_image(
    text: &str,
    path: &Path,
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
        || decode_text_image(text, PACKAGE_MIR_TOOLCHAIN_VERSION).map(FmirImagePayload::from),
    )
}

pub(super) fn load_fmir_image(
    bytes: &[u8],
    path: &Path,
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
        || decode_binary_image(bytes, PACKAGE_MIR_TOOLCHAIN_VERSION).map(FmirImagePayload::from),
    )
}
