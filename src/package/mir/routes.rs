//! Package-MIR run/build route API: route selection, image loading, and the stdio entry.

use super::*;

pub(crate) fn run_package_mir<H: Host + ?Sized>(
    config: &Config,
    input: &Path,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let argumenta = host
        .argumenta()
        .map_err(|e| vec![mir_diag(input, e.message)])?
        .to_vec();
    with_prepared_package_mir(config, input, &argumenta, |prepared, lowered| {
        let image = fmir_package_image_from_lowered(
            prepared,
            lowered,
            prepared.entry_path.clone(),
            FmirPackageImageFormat::Source,
        )?;
        run_fmir_package_image(image, host)
    })
}

/// Run a FHIR-loaded package in-process: reconstruct the package from the
/// envelope, lower to FMIR, and execute with the provided host — no Rust, no
/// source checkout. Local imports resolve from the envelope's link table.
pub(crate) fn run_package_mir_from_loaded<H: Host + ?Sized>(
    config: &Config,
    package: AnalyzedPackage,
    loaded_links: &BTreeMap<PathBuf, BTreeMap<String, PathBuf>>,
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
        |prepared, lowered| {
            let image = fmir_package_image_from_lowered(
                prepared,
                lowered,
                prepared.entry_path.clone(),
                FmirPackageImageFormat::Source,
            )?;
            run_fmir_package_image(image, host)
        },
    )
}

pub(crate) fn build_package_mir_artifact(
    config: &Config,
    input: &Path,
    argumenta: &[String],
) -> Result<PackageMirArtifact, Vec<Diagnostic>> {
    with_prepared_package_mir(config, input, argumenta, |prepared, _| {
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
            let artifact_root = package_artifact_dir(&package_root, &prepared.entry_path, "")?;
            let image = package_fmir_text_image(prepared, lowered, &package_root)?;
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
            let artifact_root = package_artifact_dir(&package_root, &prepared.entry_path, "")?;
            let image = package_fmir_binary_image(prepared, lowered, &package_root)?;
            let image_path =
                write_package_image(&artifact_root, &prepared.entry_path, FMIR_IMAGE_FILE, image)?;
            Ok(PackageFmirImage { image_path })
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
            let artifact_root =
                package_artifact_dir(&package_root, &prepared.entry_path, FMIR_BIN_ARTIFACT_DIR)?;
            let image = package_fmir_binary_image(prepared, lowered, &package_root)?;
            let image_path = write_package_image(
                &artifact_root,
                &prepared.entry_path,
                FMIR_IMAGE_FILE,
                &image,
            )?;

            let entrypoint_path = artifact_root.join(FMIR_BIN_ENTRYPOINT_FILE);
            write_fmir_bin_runner(
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
    run_loaded_fmir_image_route(loaded, host)
}

pub(crate) fn run_package_fmir_text_image_with_selection<H: Host + ?Sized>(
    image: &PackageFmirTextImage,
    selection: DeviceSelection,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let image_text = fs::read_to_string(&image.image_path).map_err(|error| {
        vec![mir_diag(
            &image.image_path,
            format!("could not read fmir-text image: {error}"),
        )]
    })?;
    let loaded = load_fmir_text_image(&image_text, &image.image_path)?;
    run_loaded_fmir_image_route_with_selection(loaded, selection, host)
}

pub(crate) fn run_package_fmir_image<H: Host + ?Sized>(
    image: &PackageFmirImage,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    run_fmir_image_path(&image.image_path, host)
}

pub(crate) fn run_package_fmir_image_with_selection<H: Host + ?Sized>(
    image: &PackageFmirImage,
    selection: DeviceSelection,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    run_fmir_image_path_with_selection(&image.image_path, Some(selection), host)
}

/// **The one host-construction policy on the image-runner routes** (N1.1/
/// N1.5, S1-6): resolve the loaded image's selection against the machine's
/// admitted backends; a device-bearing image that resolves runs through the
/// composite host's device route (S1-6 launch seam), anything else runs the
/// CPU/FMIR stepper. Fail-closed diagnostics; an explicit GPU request never
/// silently falls back.
pub(super) fn run_loaded_fmir_image_route<H: Host + ?Sized>(
    image: FmirPackageImage,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let selection = image.route_selection()?;
    run_loaded_fmir_image_route_with_selection(image, selection, host)
}

/// The same policy with an explicit selection override (the image-runner
/// route's `--backend` flag, N1.1 precedence: CLI > image's declared
/// selection > `auto`).
pub(super) fn run_loaded_fmir_image_route_with_selection<H: Host + ?Sized>(
    image: FmirPackageImage,
    selection: DeviceSelection,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let requires_device = image.device.is_some();
    // DDPP1-U2 (C2 feature isolation): the device route is compiled only under
    // `device-runtime`; without it an explicit backend request or a
    // device-bearing image fails closed (never silently runs CPU).
    #[cfg(feature = "device-runtime")]
    {
        match super::super::host_factory::resolve_backend_selection(
            selection,
            requires_device,
            &super::super::host_factory::admitted_backends(),
        ) {
            Err(diagnostic) => Err(vec![diagnostic]),
            Ok(None) => run_fmir_package_image(image, host),
            Ok(Some(backend)) => {
                let device = image.device.as_ref().ok_or_else(|| {
                    vec![super::super::host_factory::missing_device_descriptor(
                        backend,
                    )]
                })?;
                super::super::device::execute_device_route(device, backend, &image.source_hashes)
            }
        }
    }
    #[cfg(not(feature = "device-runtime"))]
    {
        if selection != DeviceSelection::Auto || requires_device {
            return Err(vec![mir_diag(
                &image.diagnostic_path,
                "device section or explicit backend selection present, but this faber build compiles no `device-runtime`; rebuild with the `device-runtime` feature to execute device routes",
            )]);
        }
        run_fmir_package_image(image, host)
    }
}

pub(crate) fn run_fmir_image_path<H: Host + ?Sized>(
    image_path: &Path,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    run_fmir_image_path_with_selection(image_path, None, host)
}

/// Run an FMIR image file under the one host-construction policy, with an
/// optional selection override (the image-runner `--backend` flag).
pub(crate) fn run_fmir_image_path_with_selection<H: Host + ?Sized>(
    image_path: &Path,
    selection_override: Option<DeviceSelection>,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let image_bytes = fs::read(image_path).map_err(|error| {
        vec![mir_diag(
            image_path,
            format!("could not read fmir image: {error}"),
        )]
    })?;
    let loaded = load_fmir_image(&image_bytes, image_path)?;
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
pub(crate) struct FmirImageRouteDecision {
    /// Whether the image carries a `device` section (N1.7). Drives the
    /// route's `requires_device`.
    pub(crate) requires_device: bool,
    /// The selection request recorded in the image's `device` section
    /// (fallback `auto` when the image carries none).
    pub(crate) declared_selection: DeviceSelection,
    /// Declared backend artifacts (canonical bytes + `content_sha256`
    /// digest), empty for CPU-only images. Re-verified against their
    /// canonical decoded bytes at image admission (DDCP2-U3).
    pub(crate) declared_artifacts: Vec<radix_mir_fmir::FmirDeviceArtifact>,
}

/// Load a binary FMIR image for the host-factory route decision without
/// executing it (fail-before-launch: the image is decoded and admitted before
/// any launch). The caller runs the image separately.
pub(crate) fn fmir_image_route_decision(
    image_path: &Path,
) -> Result<FmirImageRouteDecision, Vec<Diagnostic>> {
    let image_bytes = fs::read(image_path).map_err(|error| {
        vec![mir_diag(
            image_path,
            format!("could not read fmir image: {error}"),
        )]
    })?;
    let loaded = load_fmir_image(&image_bytes, image_path)?;
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
) -> Result<(), Vec<Diagnostic>> {
    let loaded = load_fmir_image(image_bytes, diagnostic_path)?;
    let requires_device = loaded.device.is_some();
    let selection = loaded.route_selection()?;
    // DDPP1-U2 (C2 feature isolation): the composite-host device route is
    // compiled only under `device-runtime`; without it an explicit backend
    // request or a device-bearing image fails closed (never silently runs
    // CPU). With the runtime, the one host-construction policy applies (N1.1/
    // N1.5): `auto` + a payload-less image keeps the CPU route unchanged; an
    // explicit request that cannot be served fails closed and never silently
    // falls back to CPU.
    #[cfg(feature = "device-runtime")]
    {
        match super::super::host_factory::resolve_backend_selection(
            selection,
            requires_device,
            &super::super::host_factory::admitted_backends(),
        ) {
            Err(diagnostic) => Err(vec![diagnostic]),
            Ok(None) => {
                let mut host = radix::mir::StdioHost::with_argumenta(argumenta);
                run_fmir_package_image(loaded, &mut host)
            }
            Ok(Some(backend)) => {
                // Device route: the image carries a device program and the
                // backend is admitted. Run it through the composite host's
                // device route (S1-6 launch seam); fail-before-launch applies
                // inside the descriptor validation.
                let device = loaded.device.as_ref().ok_or_else(|| {
                    vec![super::super::host_factory::missing_device_descriptor(
                        backend,
                    )]
                })?;
                super::super::device::execute_device_route(device, backend, &loaded.source_hashes)
            }
        }
    }
    #[cfg(not(feature = "device-runtime"))]
    {
        if selection != DeviceSelection::Auto || requires_device {
            return Err(vec![mir_diag(
                diagnostic_path,
                "device section or explicit backend selection present, but this faber build compiles no `device-runtime`; rebuild with the `device-runtime` feature to execute device routes",
            )]);
        }
        let mut host = radix::mir::StdioHost::with_argumenta(argumenta);
        run_fmir_package_image(loaded, &mut host)
    }
}
