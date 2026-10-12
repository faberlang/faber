//! Package-MIR run/build route API: route selection, image loading, and the stdio entry.

use radix_module::mir::KernelLowering;
use std::borrow::Cow;

use super::{
    AnalyzedProgram, BTreeMap, CliPlanningMode, Config, DeviceSelection, Diagnostic,
    FMIR_BIN_ARTIFACT_DIR, FMIR_BIN_ENTRYPOINT_FILE, FMIR_IMAGE_FILE, FMIR_TEXT_IMAGE_FILE,
    FmirPackageImage, FmirPackageImageFormat, Host, MirSeams, PACKAGE_MIR_MANIFEST_FILE,
    PackageFmirBinaryBundle, PackageFmirImage, PackageFmirTextImage, PackageMirArtifact, Path,
    PathBuf, RunnerKernelPolicy, RunnerStats, fmir_package_image_from_lowered, fs, load_fmir_image,
    load_fmir_text_image, mir_diag, mir_issue_diag, package_artifact_dir, package_artifact_root,
    package_fmir_binary_image, package_fmir_text_image, package_mir_manifest,
    run_fmir_package_image, run_fmir_package_image_with_stats, validate_package_mir_manifest,
    with_prepared_package_mir, with_prepared_package_mir_from_loaded,
    with_prepared_package_mir_with_cli_mode, write_package_image,
};

/// The started package run's diagnostic-statistics outcome (RSM route
/// transport): the meter summary plus the started run's status. Compile/load
/// failures happen before a runner starts and remain the route's `Err`
/// (diagnostics without a summary); runtime diagnostics are retained here so
/// the caller receives the artifact and the failure together.
#[derive(Debug)]
pub struct PackageRunStats {
    pub stats: RunnerStats,
    pub result: Result<(), Vec<Diagnostic>>,
}

/// The lowering config a selection runs under: `cpu` lowers kernels as
/// ordinary host functions, every other selection leaves the config alone.
fn config_for_selection(config: &Config, selection: DeviceSelection) -> Cow<'_, Config> {
    match selection.kernel_lowering() {
        KernelLowering::Device => Cow::Borrowed(config),
        KernelLowering::Host => Cow::Owned(config.clone().with_host_kernels()),
    }
}

/// Read the host's program arguments, mapping a host failure onto the route's
/// diagnostic vector.
fn host_argumenta<H: Host + ?Sized>(
    input: &Path,
    host: &mut H,
) -> Result<Vec<String>, Vec<Diagnostic>> {
    Ok(host
        .argumenta()
        .map_err(|e| vec![mir_diag(input, e.message)])?
        .to_vec())
}

/// Prepare the package under the interpreted consumer, bind the host's
/// program arguments into the CLI entry record exactly as the image routes do,
/// and lend the lowered unit, the argument-bound validated program, and the
/// declared CLI exit code to `run`. This is the in-process device route's
/// operand-binding seam: the caller runs the entry itself and applies
/// `host.exit(code)` on success.
/// # Errors
/// Returns an error when the input is invalid or the arguments do not bind.
pub fn with_interpreted_bound_package_mir<R, H: Host + ?Sized>(
    config: &Config,
    input: &Path,
    host: &mut H,
    seams: &MirSeams,
    run: impl for<'a, 'v> FnOnce(
        &super::LoweredMirUnit<'a>,
        &radix_module::mir::ValidatedMir<'v>,
        Option<i32>,
        &mut H,
    ) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    let argumenta = host_argumenta(input, host)?;
    with_prepared_package_mir(config, input, &argumenta, seams, |prepared, lowered| {
        let (type_snapshot, interner_strings) = super::image::snapshot_context(lowered);
        let types = super::TypeTable::from_snapshot(type_snapshot).map_err(|error| {
            vec![mir_diag(
                &prepared.entry_path,
                format!("could not load type metadata: {error}"),
            )]
        })?;
        let mut interner = super::Interner::from_strings(interner_strings);
        let mut program = lowered.validated.program().clone();
        super::cli_bind::bind_fmir_text_runtime_cli(
            &mut program,
            prepared.fmir_text_cli.as_ref(),
            "run_entry",
            &mut interner,
            &*host,
            &prepared.entry_path,
        )?;
        let mut validation = radix_module::mir::MirValidationContext::new(&types);
        validation.interner = radix_module::mir::MirInterner::Borrowed(&interner);
        super::image::apply_validation_metadata(
            &mut validation,
            &super::image::snapshot_validation_metadata(lowered.validated.validation()),
        );
        let validated =
            radix_module::mir::ValidatedMir::new(program, validation).map_err(|errors| {
                errors
                    .into_iter()
                    .map(|error| mir_diag(&prepared.entry_path, error.message))
                    .collect::<Vec<_>>()
            })?;
        run(lowered, &validated, prepared.cli_exit_code, host)
    })
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn run_package_mir<H: Host + ?Sized>(
    config: &Config,
    input: &Path,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let argumenta = host_argumenta(input, host)?;
    with_prepared_package_mir(config, input, &argumenta, seams, |prepared, lowered| {
        let image = fmir_package_image_from_lowered(
            prepared,
            lowered,
            prepared.entry_path.clone(),
            FmirPackageImageFormat::Source,
            seams,
        )?;
        // The one host-construction policy (N1.1/N1.5, S1-6): the image's own
        // `[device]` section decides whether the route carries a device
        // program, and the resolved selection applies before launch — a
        // device-bearing image runs through the composite host's device route
        // (host driver); anything else runs the CPU/FMIR runner.
        run_loaded_fmir_image_route(image, host)
    })
}

/// [`run_package_mir`] with an explicit selection override (the CLI
/// `--device` flag, N1.1 precedence: CLI > image's declared selection >
/// `auto`).
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn run_package_mir_with_selection<H: Host + ?Sized>(
    config: &Config,
    input: &Path,
    selection: DeviceSelection,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let argumenta = host_argumenta(input, host)?;
    let config = config_for_selection(config, selection);
    with_prepared_package_mir(&config, input, &argumenta, seams, |prepared, lowered| {
        let image = fmir_package_image_from_lowered(
            prepared,
            lowered,
            prepared.entry_path.clone(),
            FmirPackageImageFormat::Source,
            seams,
        )?;
        run_loaded_fmir_image_route_with_selection(image, selection, host)
    })
}

/// [`run_package_mir_with_selection`] with the opt-in diagnostic meter (RSM
/// route transport): the CPU/FMIR leg collects the runner summary. The
/// single-source route is intentionally out of scope (refused by the faber CLI
/// layer, RSM-3).
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn run_package_mir_with_selection_and_stats<H: Host + ?Sized>(
    config: &Config,
    input: &Path,
    selection: DeviceSelection,
    seams: &MirSeams,
    host: &mut H,
) -> Result<PackageRunStats, Vec<Diagnostic>> {
    let argumenta = host_argumenta(input, host)?;
    let config = config_for_selection(config, selection);
    with_prepared_package_mir(&config, input, &argumenta, seams, |prepared, lowered| {
        let image = fmir_package_image_from_lowered(
            prepared,
            lowered,
            prepared.entry_path.clone(),
            FmirPackageImageFormat::Source,
            seams,
        )?;
        run_loaded_fmir_image_route_with_selection_and_stats(image, selection, host)
    })
}

/// Run a FHIR-loaded package in-process: reconstruct the package from the
/// envelope, lower to FMIR, and execute with the provided host — no Rust, no
/// source checkout. Local imports resolve from the envelope's link table.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn run_package_mir_from_loaded<H: Host + ?Sized>(
    config: &Config,
    package: AnalyzedProgram,
    loaded_links: &BTreeMap<PathBuf, BTreeMap<String, PathBuf>>,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let diagnostic_path = package.spec.entry.clone();
    let argumenta = host_argumenta(&diagnostic_path, host)?;
    with_prepared_package_mir_from_loaded(
        config,
        package,
        loaded_links,
        &argumenta,
        seams,
        |prepared, lowered| {
            let image = fmir_package_image_from_lowered(
                prepared,
                lowered,
                prepared.entry_path.clone(),
                FmirPackageImageFormat::Source,
                seams,
            )?;
            run_fmir_package_image(image, host, RunnerKernelPolicy::Refuse)
        },
    )
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn build_package_mir_artifact(
    config: &Config,
    input: &Path,
    argumenta: &[String],
    seams: &MirSeams,
) -> Result<PackageMirArtifact, Vec<Diagnostic>> {
    with_prepared_package_mir(config, input, argumenta, seams, |prepared, _| {
        let package_root = package_artifact_root(input)?;
        let artifact_root = package_artifact_dir(&package_root, &prepared.entry_path, "")?;
        let manifest_path = write_package_image(
            &artifact_root,
            &prepared.entry_path,
            PACKAGE_MIR_MANIFEST_FILE,
            package_mir_manifest(prepared, &package_root),
        )?;
        Ok(PackageMirArtifact {
            root: artifact_root,
            manifest_path,
            entry: prepared.entry_path.clone(),
        })
    })
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn build_package_fmir_text_image(
    config: &Config,
    input: &Path,
    argumenta: &[String],
    seams: &MirSeams,
) -> Result<PackageFmirTextImage, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode(
        config,
        input,
        argumenta,
        CliPlanningMode::FmirTextRuntime,
        seams,
        |prepared, lowered| {
            let package_root = package_artifact_root(input)?;
            let artifact_root = package_artifact_dir(&package_root, &prepared.entry_path, "")?;
            let image = package_fmir_text_image(prepared, lowered, &package_root, seams)?;
            let image_path = write_package_image(
                &artifact_root,
                &prepared.entry_path,
                FMIR_TEXT_IMAGE_FILE,
                image,
            )?;
            Ok(PackageFmirTextImage { image_path })
        },
    )
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn build_package_fmir_image(
    config: &Config,
    input: &Path,
    argumenta: &[String],
    seams: &MirSeams,
) -> Result<PackageFmirImage, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode(
        config,
        input,
        argumenta,
        CliPlanningMode::FmirTextRuntime,
        seams,
        |prepared, lowered| {
            let package_root = package_artifact_root(input)?;
            let artifact_root = package_artifact_dir(&package_root, &prepared.entry_path, "")?;
            let image = package_fmir_binary_image(prepared, lowered, &package_root, seams)?;
            let image_path =
                write_package_image(&artifact_root, &prepared.entry_path, FMIR_IMAGE_FILE, image)?;
            Ok(PackageFmirImage { image_path })
        },
    )
}

/// Build the FMIR binary image AND run it from the in-memory image (FMIR
/// e2e-hardening CTO-1): the source-built image carries the merged program's
/// struct/variant validation metadata (`FmirValidationMetadata`), which the
/// image-FILE round-trip drops — so running a library-backed package (struct
/// literals, method calls, field projections) from the file failed at run
/// with missing field metadata. The image file is still written so the
/// artifact surface is unchanged; the run consumes the in-memory image under
/// the same host-construction policy (CPU runner vs device route).
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn run_package_fmir_built_image<H: Host + ?Sized>(
    config: &Config,
    input: &Path,
    selection: DeviceSelection,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode(
        config,
        input,
        &[],
        CliPlanningMode::FmirTextRuntime,
        seams,
        |prepared, lowered| {
            let package_root = package_artifact_root(input)?;
            let artifact_root = package_artifact_dir(&package_root, &prepared.entry_path, "")?;
            let image = fmir_package_image_from_lowered(
                prepared,
                lowered,
                prepared.entry_path.clone(),
                FmirPackageImageFormat::Source,
                seams,
            )?;
            let encoded = package_fmir_binary_image(prepared, lowered, &package_root, seams)?;
            write_package_image(
                &artifact_root,
                &prepared.entry_path,
                FMIR_IMAGE_FILE,
                encoded,
            )?;
            run_loaded_fmir_image_route_with_selection(image, selection, host)
        },
    )
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn build_package_fmir_binary_bundle(
    config: &Config,
    input: &Path,
    argumenta: &[String],
    release: bool,
    seams: &MirSeams,
) -> Result<PackageFmirBinaryBundle, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode(
        config,
        input,
        argumenta,
        CliPlanningMode::FmirTextRuntime,
        seams,
        |prepared, lowered| {
            let package_root = package_artifact_root(input)?;
            let artifact_root =
                package_artifact_dir(&package_root, &prepared.entry_path, FMIR_BIN_ARTIFACT_DIR)?;
            let image = package_fmir_binary_image(prepared, lowered, &package_root, seams)?;
            let image_path = write_package_image(
                &artifact_root,
                &prepared.entry_path,
                FMIR_IMAGE_FILE,
                &image,
            )?;

            let entrypoint_path = artifact_root.join(FMIR_BIN_ENTRYPOINT_FILE);
            (seams.bin_runner_writer)(
                &artifact_root,
                &entrypoint_path,
                &prepared.entry_path,
                &image,
                release,
            )?;

            Ok(PackageFmirBinaryBundle {
                entrypoint_path,
                image_path,
            })
        },
    )
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn run_package_mir_artifact<H: Host + ?Sized>(
    config: &Config,
    artifact: &PackageMirArtifact,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let manifest = fs::read_to_string(&artifact.manifest_path).map_err(|error| {
        vec![mir_diag(
            &artifact.manifest_path,
            format!("could not read package MIR artifact manifest: {error}"),
        )]
    })?;
    validate_package_mir_manifest(&manifest, &artifact.manifest_path)?;
    run_package_mir(config, &artifact.entry, seams, host)
}

/// Read and decode an FMIR text image file under the route's toolchain
/// version.
fn load_fmir_text_image_from_file(
    image: &PackageFmirTextImage,
    seams: &MirSeams,
) -> Result<FmirPackageImage, Vec<Diagnostic>> {
    let image_text = fs::read_to_string(&image.image_path).map_err(|error| {
        vec![mir_diag(
            &image.image_path,
            format!("could not read fmir-text image: {error}"),
        )]
    })?;
    load_fmir_text_image(&image_text, &image.image_path, seams.toolchain_version)
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn run_package_fmir_text_image<H: Host + ?Sized>(
    image: &PackageFmirTextImage,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let loaded = load_fmir_text_image_from_file(image, seams)?;
    run_loaded_fmir_image_route(loaded, host)
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn run_package_fmir_text_image_with_selection<H: Host + ?Sized>(
    image: &PackageFmirTextImage,
    selection: DeviceSelection,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let loaded = load_fmir_text_image_from_file(image, seams)?;
    run_loaded_fmir_image_route_with_selection(loaded, selection, host)
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn run_package_fmir_image<H: Host + ?Sized>(
    image: &PackageFmirImage,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    run_fmir_image_path(&image.image_path, seams, host)
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn run_package_fmir_image_with_selection<H: Host + ?Sized>(
    image: &PackageFmirImage,
    selection: DeviceSelection,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    run_fmir_image_path_with_selection(&image.image_path, Some(selection), seams, host)
}

/// Admit a loaded image to the CPU/FMIR runner route, or fail closed.
///
/// The child-process device route is retired (gpu-reset C2 U7). Device
/// execution is the in-process session, which needs a lowered unit that a
/// loaded image does not carry, so this route never executes a device:
///
/// - `reference` and `cpu` run the image on the runner (kernels in host form), even when the
///   image carries a device section;
/// - a device-bearing image under any other selection fails with
///   `E_DEVICE_IMAGE_ROUTE_RETIRED`;
/// - an explicit GPU selection with no device section fails closed.
fn admit_runner_route(
    image: &FmirPackageImage,
    selection: DeviceSelection,
) -> Result<(), Vec<Diagnostic>> {
    if matches!(selection, DeviceSelection::Cpu | DeviceSelection::Reference) {
        return Ok(());
    }
    if image.device.is_some() {
        return Err(vec![mir_issue_diag(
            &image.diagnostic_path,
            "E_DEVICE_IMAGE_ROUTE_RETIRED",
            "this FMIR image carries a device section, and the child-process device route that ran such images is retired; device execution is the in-process device session, which needs the package source (`faber run <package>`), not a packaged image. Pass `--device cpu` to run the image's kernels on the host-form runner",
        )]);
    }
    if selection != DeviceSelection::Auto {
        return Err(vec![mir_diag(
            &image.diagnostic_path,
            format!(
                "explicit device selection `{}` cannot run on the CPU/FMIR runner route; device execution runs through the in-process device session (`faber run` of a package built with `device-runtime`), and CPU-only images never silently fall back",
                selection.spelling()
            ),
        )
        .with_arg("selection", selection.spelling())]);
    }
    Ok(())
}

/// **The runner route over a loaded image** (N1.1/N1.5): resolve the image's
/// selection and run it on the CPU/FMIR runner. A device-bearing image never
/// runs here (see [`admit_runner_route`]); an explicit GPU request never
/// silently falls back.
pub(super) fn run_loaded_fmir_image_route<H: Host + ?Sized>(
    image: FmirPackageImage,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let selection = image.route_selection()?;
    run_loaded_fmir_image_route_with_selection(image, selection, host)
}

/// The same policy with an explicit selection override (the image-runner
/// route's `--device` flag, N1.1 precedence: CLI > image's declared
/// selection > `auto`).
pub(super) fn run_loaded_fmir_image_route_with_selection<H: Host + ?Sized>(
    image: FmirPackageImage,
    selection: DeviceSelection,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    admit_runner_route(&image, selection)?;
    run_fmir_package_image(image, host, selection.kernel_policy())
}

/// [`run_loaded_fmir_image_route_with_selection`] with the opt-in diagnostic
/// meter (RSM route transport): the CPU/FMIR leg arms the collector. Stats do
/// not change the route decision; a device-bearing image or explicit GPU
/// selection fails closed exactly as it does without stats.
pub(super) fn run_loaded_fmir_image_route_with_selection_and_stats<H: Host + ?Sized>(
    image: FmirPackageImage,
    selection: DeviceSelection,
    host: &mut H,
) -> Result<PackageRunStats, Vec<Diagnostic>> {
    admit_runner_route(&image, selection)?;
    run_fmir_package_image_with_stats(image, host, selection.kernel_policy())
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn run_fmir_image_path<H: Host + ?Sized>(
    image_path: &Path,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    run_fmir_image_path_with_selection(image_path, None, seams, host)
}

/// Read and decode an FMIR image file under the route's toolchain version.
fn load_fmir_image_from_path(
    image_path: &Path,
    seams: &MirSeams,
) -> Result<FmirPackageImage, Vec<Diagnostic>> {
    let image_bytes = fs::read(image_path).map_err(|error| {
        vec![mir_diag(
            image_path,
            format!("could not read fmir image: {error}"),
        )]
    })?;
    load_fmir_image(&image_bytes, image_path, seams.toolchain_version)
}

/// Run an FMIR image file under the one host-construction policy, with an
/// optional selection override (the image-runner `--device` flag).
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn run_fmir_image_path_with_selection<H: Host + ?Sized>(
    image_path: &Path,
    selection_override: Option<DeviceSelection>,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let loaded = load_fmir_image_from_path(image_path, seams)?;
    let selection = match selection_override {
        Some(selection) => selection,
        None => loaded.route_selection()?,
    };
    run_loaded_fmir_image_route_with_selection(loaded, selection, host)
}

/// S1-5 route decision for a binary FMIR image (the image-runner route of the
/// one host-construction policy): whether the image carries a device program,
/// the selection request it records, and its declared backend artifacts
/// (N1.1 / N1.7).
pub struct FmirImageRouteDecision {
    /// Whether the image carries a `device` section (N1.7). Drives the
    /// route's `requires_device`.
    pub requires_device: bool,
    /// The selection request recorded in the image's `device` section
    /// (fallback `auto` when the image carries none).
    pub declared_selection: DeviceSelection,
    /// Declared backend artifacts (canonical bytes + `content_sha256`
    /// digest), empty for CPU-only images. Re-verified against their
    /// canonical decoded bytes at image admission (DDCP2-U3).
    pub declared_artifacts: Vec<radix_mir_fmir::FmirDeviceArtifact>,
}

/// Load a binary FMIR image for the host-factory route decision without
/// executing it (fail-before-launch: the image is decoded and admitted before
/// any launch). The caller runs the image separately.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn fmir_image_route_decision(
    image_path: &Path,
    seams: &MirSeams,
) -> Result<FmirImageRouteDecision, Vec<Diagnostic>> {
    let loaded = load_fmir_image_from_path(image_path, seams)?;
    let requires_device = loaded.device.is_some();
    let declared_selection = loaded.route_selection()?;
    let declared_artifacts = loaded
        .device
        .as_ref()
        .map(|device| device.artifacts.artifact.clone())
        .unwrap_or_default();
    Ok(FmirImageRouteDecision {
        requires_device,
        declared_selection,
        declared_artifacts,
    })
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn run_fmir_image_bytes_with_stdio(
    image_bytes: &[u8],
    diagnostic_path: &Path,
    argumenta: Vec<String>,
    seams: &MirSeams,
) -> Result<(), Vec<Diagnostic>> {
    let loaded = load_fmir_image(image_bytes, diagnostic_path, seams.toolchain_version)?;
    let selection = loaded.route_selection()?;
    admit_runner_route(&loaded, selection)?;
    let mut host = radix_module::mir::StdioHost::with_argumenta(argumenta);
    run_fmir_package_image(loaded, &mut host, selection.kernel_policy())
}
