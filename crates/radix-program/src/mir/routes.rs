//! Package-MIR run/build route API: route selection, image loading, and the stdio entry.

use super::*;

pub fn run_package_mir<H: Host + ?Sized>(
    config: &Config,
    input: &Path,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let argumenta = host
        .argumenta()
        .map_err(|e| vec![mir_diag(input, e.message)])?
        .to_vec();
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
        run_loaded_fmir_image_route(image, seams, host)
    })
}

/// [`run_package_mir`] with an explicit selection override (the CLI
/// `--backend` flag, N1.1 precedence: CLI > image's declared selection >
/// `auto`).
pub fn run_package_mir_with_selection<H: Host + ?Sized>(
    config: &Config,
    input: &Path,
    selection: DeviceSelection,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let argumenta = host
        .argumenta()
        .map_err(|e| vec![mir_diag(input, e.message)])?
        .to_vec();
    with_prepared_package_mir(config, input, &argumenta, seams, |prepared, lowered| {
        let image = fmir_package_image_from_lowered(
            prepared,
            lowered,
            prepared.entry_path.clone(),
            FmirPackageImageFormat::Source,
            seams,
        )?;
        run_loaded_fmir_image_route_with_selection(image, selection, seams, host)
    })
}

/// Run a FHIR-loaded package in-process: reconstruct the package from the
/// envelope, lower to FMIR, and execute with the provided host — no Rust, no
/// source checkout. Local imports resolve from the envelope's link table.
pub fn run_package_mir_from_loaded<H: Host + ?Sized>(
    config: &Config,
    package: AnalyzedProgram,
    loaded_links: &BTreeMap<PathBuf, BTreeMap<String, PathBuf>>,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let diagnostic_path = package.spec.entry.clone();
    let argumenta = host
        .argumenta()
        .map_err(|e| vec![mir_diag(&diagnostic_path, e.message)])?
        .to_vec();
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
            run_fmir_package_image(image, host)
        },
    )
}

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
            run_loaded_fmir_image_route_with_selection(image, selection, seams, host)
        },
    )
}

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

pub fn run_package_fmir_text_image<H: Host + ?Sized>(
    image: &PackageFmirTextImage,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let image_text = fs::read_to_string(&image.image_path).map_err(|error| {
        vec![mir_diag(
            &image.image_path,
            format!("could not read fmir-text image: {error}"),
        )]
    })?;
    let loaded = load_fmir_text_image(&image_text, &image.image_path, seams.toolchain_version)?;
    run_loaded_fmir_image_route(loaded, seams, host)
}

pub fn run_package_fmir_text_image_with_selection<H: Host + ?Sized>(
    image: &PackageFmirTextImage,
    selection: DeviceSelection,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let image_text = fs::read_to_string(&image.image_path).map_err(|error| {
        vec![mir_diag(
            &image.image_path,
            format!("could not read fmir-text image: {error}"),
        )]
    })?;
    let loaded = load_fmir_text_image(&image_text, &image.image_path, seams.toolchain_version)?;
    run_loaded_fmir_image_route_with_selection(loaded, selection, seams, host)
}

pub fn run_package_fmir_image<H: Host + ?Sized>(
    image: &PackageFmirImage,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    run_fmir_image_path(&image.image_path, seams, host)
}

pub fn run_package_fmir_image_with_selection<H: Host + ?Sized>(
    image: &PackageFmirImage,
    selection: DeviceSelection,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    run_fmir_image_path_with_selection(&image.image_path, Some(selection), seams, host)
}

/// **The one host-construction policy on the image-runner routes** (N1.1/
/// N1.5, S1-6): resolve the loaded image's selection against the machine's
/// admitted backends; a device-bearing image that resolves runs through the
/// composite host's device route (S1-6 launch seam), anything else runs the
/// CPU/FMIR runner. Fail-closed diagnostics; an explicit GPU request never
/// silently falls back. The policy body is faber's [`MirSeams::host_route`].
pub(super) fn run_loaded_fmir_image_route<H: Host + ?Sized>(
    image: FmirPackageImage,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let selection = image.route_selection()?;
    run_loaded_fmir_image_route_with_selection(image, selection, seams, host)
}

/// The same policy with an explicit selection override (the image-runner
/// route's `--backend` flag, N1.1 precedence: CLI > image's declared
/// selection > `auto`).
pub(super) fn run_loaded_fmir_image_route_with_selection<H: Host + ?Sized>(
    image: FmirPackageImage,
    selection: DeviceSelection,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    // The one host-construction policy (faber-supplied seam): the faber side
    // resolves the selection against the machine's admitted backends, runs a
    // device-bearing image through the composite host's device route, and
    // fails closed on unsupported requests. `Ok(Some(()))` means the device
    // route already executed; `Ok(None)` runs the CPU/FMIR runner with the
    // caller's host.
    if (seams.host_route)(&image, selection)?.is_some() {
        return Ok(());
    }
    run_fmir_package_image(image, host)
}

pub fn run_fmir_image_path<H: Host + ?Sized>(
    image_path: &Path,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    run_fmir_image_path_with_selection(image_path, None, seams, host)
}

/// Run an FMIR image file under the one host-construction policy, with an
/// optional selection override (the image-runner `--backend` flag).
pub fn run_fmir_image_path_with_selection<H: Host + ?Sized>(
    image_path: &Path,
    selection_override: Option<DeviceSelection>,
    seams: &MirSeams,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let image_bytes = fs::read(image_path).map_err(|error| {
        vec![mir_diag(
            image_path,
            format!("could not read fmir image: {error}"),
        )]
    })?;
    let loaded = load_fmir_image(&image_bytes, image_path, seams.toolchain_version)?;
    let selection = match selection_override {
        Some(selection) => selection,
        None => loaded.route_selection()?,
    };
    run_loaded_fmir_image_route_with_selection(loaded, selection, seams, host)
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
pub fn fmir_image_route_decision(
    image_path: &Path,
    seams: &MirSeams,
) -> Result<FmirImageRouteDecision, Vec<Diagnostic>> {
    let image_bytes = fs::read(image_path).map_err(|error| {
        vec![mir_diag(
            image_path,
            format!("could not read fmir image: {error}"),
        )]
    })?;
    let loaded = load_fmir_image(&image_bytes, image_path, seams.toolchain_version)?;
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

pub fn run_fmir_image_bytes_with_stdio(
    image_bytes: &[u8],
    diagnostic_path: &Path,
    argumenta: Vec<String>,
    seams: &MirSeams,
) -> Result<(), Vec<Diagnostic>> {
    let loaded = load_fmir_image(image_bytes, diagnostic_path, seams.toolchain_version)?;
    let selection = loaded.route_selection()?;
    // The one host-construction policy (faber-supplied seam): the faber side
    // resolves the selection against the machine's admitted backends, runs a
    // device-bearing image through the composite host's device route, and
    // fails closed on unsupported requests. `Ok(Some(()))` means the device
    // route already executed; `Ok(None)` runs the CPU/FMIR runner with the
    // stdio host.
    if (seams.host_route)(&loaded, selection)?.is_some() {
        return Ok(());
    }
    let mut host = radix_module::mir::StdioHost::with_argumenta(argumenta);
    run_fmir_package_image(loaded, &mut host)
}
