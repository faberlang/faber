//! Package-MIR driver: prepare a package and lend it to a probe or lower it for a route.

use super::*;

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

/// Analyze, link, and validate a package as MIR under the INTERPRETED
/// consumer (S5-U5c): the consumer that links library imports into the
/// merged program (the `faber run` path), so a test can probe the merged
/// MIR — and the device constructor — with a library-backed body such as the
/// gradus `train_step_*` surface. External-target probes ([`with_lowered_package_mir`])
/// keep the previous behavior: library calls stay provider-backed.
#[cfg(test)]
pub(crate) fn with_interpreted_lowered_package_mir<R>(
    config: &Config,
    input: &Path,
    run: impl for<'a> FnOnce(&LoweredMirUnit<'a>) -> R,
) -> Result<R, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode_and_consumer(
        config,
        input,
        &[],
        CliPlanningMode::Parsed,
        PackageMirConsumer::Interpreted,
        |_, lowered| Ok(run(lowered)),
    )
}

pub(super) fn with_prepared_package_mir<R>(
    config: &Config,
    input: &Path,
    argumenta: &[String],
    run: impl for<'a> FnOnce(&PreparedPackageMir<'a>, &LoweredMirUnit<'a>) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode(config, input, argumenta, CliPlanningMode::Parsed, run)
}

pub(super) fn with_prepared_package_mir_with_cli_mode<R>(
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

pub(super) fn with_prepared_package_mir_with_cli_mode_and_consumer<R>(
    config: &Config,
    input: &Path,
    argumenta: &[String],
    cli_mode: CliPlanningMode,
    consumer: PackageMirConsumer,
    run: impl for<'a> FnOnce(&PreparedPackageMir<'a>, &LoweredMirUnit<'a>) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    let package = analyze_package(config, input)?;
    prepare_package_mir(
        config,
        package,
        argumenta,
        cli_mode,
        consumer,
        None,
        || manifest_device_config(input),
        run,
    )
}

/// Shared package-MIR preparation pipeline for the source-analysis and
/// loaded-package drivers: diagnostics gate → CLI plan → namespace-link
/// resolution → entry selection → unit rewriting → lowering → validation →
/// runtime-requirement collection → device config (the source route reads the
/// manifest; the loaded route supplies fixed empty values) → a
/// `PreparedPackageMir`, then `run`.
pub(super) fn prepare_package_mir<R>(
    config: &Config,
    mut package: AnalyzedPackage,
    argumenta: &[String],
    cli_mode: CliPlanningMode,
    consumer: PackageMirConsumer,
    loaded_links: Option<&BTreeMap<PathBuf, BTreeMap<String, PathBuf>>>,
    device_config: impl FnOnce() -> Result<
        (
            BTreeMap<String, Vec<f32>>,
            Option<DeviceSelection>,
            Option<u32>,
            bool,
        ),
        Vec<Diagnostic>,
    >,
    run: impl for<'a> FnOnce(&PreparedPackageMir<'a>, &LoweredMirUnit<'a>) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    if package
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.is_error())
    {
        return Err(package.diagnostics);
    }
    let cli_plan = plan_cli_package(&mut package, argumenta, cli_mode)?;
    let library_resolver = library_resolver_from_config(config);
    let mut library_cache = LibraryInterfaceCache::with_config(config);
    let links = local_namespace_call_targets(
        &package,
        consumer,
        &library_resolver,
        &mut library_cache,
        loaded_links,
    )?;
    let entry_index = select_entry_unit(&package)?;
    let entry_path = package.units[entry_index].path.clone();
    let source_paths = package.units.iter().map(|unit| unit.path.clone()).collect();
    for unit in &mut package.units {
        rewrite_unit_namespace_calls(unit, &links.calls, &links.namespaces)?;
    }

    let mut lowered = lower_package_units(
        &mut package,
        entry_index,
        &links,
        &library_resolver,
        &mut library_cache,
        &cli_plan,
        config.no_fuse,
    )?;
    validate_program(&lowered.program, lowered.validated.validation()).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| mir_lowering_diag(&entry_path, error.message))
            .collect::<Vec<_>>()
    })?;
    if consumer == PackageMirConsumer::Interpreted {
        bridge_norma_providers_to_kernel(&mut lowered, &entry_path)?;
    }
    let runtime_requirements = collect_package_runtime_requirements(&lowered, &cli_plan);
    let (device_inputs, device_backend, device_steps, device_declared) = device_config()?;
    let prepared = PreparedPackageMir {
        entry_path: entry_path.clone(),
        source_paths,
        runtime_requirements,
        cli_exit_code: cli_plan.exit_code,
        fmir_text_cli: cli_plan.fmir_text_cli.clone(),
        device_inputs,
        device_backend,
        device_steps,
        device_declared,
        _marker: std::marker::PhantomData,
    };
    run(&prepared, &lowered)
}

/// Loaded-package MIR driver: prepare a package reconstructed from a FHIR
/// envelope for lowering/run. Mirrors [`prepare_package_mir`] but skips
/// `analyze_package` (the package arrives already reconstructed) and resolves
/// local imports from the envelope's explicit link table instead of the
/// filesystem.
pub(super) fn with_prepared_package_mir_from_loaded<R>(
    config: &Config,
    package: AnalyzedPackage,
    loaded_links: &BTreeMap<PathBuf, BTreeMap<String, PathBuf>>,
    argumenta: &[String],
    run: impl for<'a> FnOnce(&PreparedPackageMir<'a>, &LoweredMirUnit<'a>) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    // The FHIR-loaded route reconstructs the package from an envelope; no
    // filesystem manifest is consulted here, so no device payload is
    // constructed on this route (N1.1: source routes reject explicit GPU
    // requests; `auto` keeps the CPU route).
    prepare_package_mir(
        config,
        package,
        argumenta,
        CliPlanningMode::Parsed,
        PackageMirConsumer::Interpreted,
        Some(loaded_links),
        || Ok((BTreeMap::new(), None, None, false)),
        run,
    )
}

pub(super) fn select_entry_unit(package: &AnalyzedPackage) -> Result<usize, Vec<Diagnostic>> {
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

/// Whether an import path string (`norma:solum`) names a kernel-manifest
/// module the interpreted-package bridge can satisfy. Shared by the
/// library-import allowlist (identity-based) and the namespace-link pass
/// (path-based) so the two rejection sites agree.
pub(super) fn is_bridged_norma_import_path(path: &str) -> bool {
    path.strip_prefix("norma:")
        .and_then(radix::kernel::resolve_kernel_module_name)
        .is_some()
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
pub(super) fn bridge_norma_providers_to_kernel(
    lowered: &mut LoweredMirUnit,
    entry_path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let Some(interner) = lowered.validated.validation().interner else {
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
                    // Non-manifest `norma:*` stays as Package; the stepper's
                    // `host.provider()` reports it unsupported.
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

