//! Package-MIR driver: prepare a package and lend it to a probe or lower it for a route.

use super::{
    AnalyzedProgram, BTreeMap, CliPackagePlan, CliPlanningMode, Config, DefId,
    DeviceManifestConfig, Diagnostic, HashMap, HirItemKind, LibraryInterfaceCache, LoweredMirUnit,
    MirSeams, ModuleId, PackageMirConsumer, PackageMirLinks, Path, PathBuf, PreparedPackageMir,
    TestFunctionIds, TestPackageScope, air_specialize, canonicalize_direct_definition_callees,
    collect_package_runtime_requirements, kernel_import_calls, local_namespace_call_targets,
    lower_package_units, mir_diag, mir_validation_diag, plan_cli_package,
    rewrite_unit_namespace_calls, validate_program,
};

struct PackageMirPreparation<'a> {
    config: &'a Config,
    argumenta: &'a [String],
    cli_mode: CliPlanningMode,
    consumer: PackageMirConsumer,
    loaded_links: Option<&'a BTreeMap<PathBuf, BTreeMap<String, PathBuf>>>,
    /// The cache the package analysis filled. Link and lowering reuse it when
    /// it analyzes libraries exactly as `library_cache_for_config(config)`
    /// would (same target, same locale loader); otherwise they start fresh.
    analysis_library_cache: Option<LibraryInterfaceCache>,
}

#[cfg(test)]
thread_local! {
    /// Test seam: when set, link and lowering ignore the analysis-stage
    /// library cache and build a fresh one, as before the cache was shared.
    pub(super) static FRESH_LIBRARY_CACHE_PER_STAGE: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// The library cache for link and lowering: the analysis-stage cache when it
/// is interchangeable with a fresh one, else a fresh one.
fn library_cache_for_preparation(
    config: &Config,
    seams: &MirSeams,
    analysis_cache: Option<LibraryInterfaceCache>,
) -> LibraryInterfaceCache {
    let fresh = (seams.library_cache_for_config)(config);
    #[cfg(test)]
    if FRESH_LIBRARY_CACHE_PER_STAGE.with(std::cell::Cell::get) {
        return fresh;
    }
    match analysis_cache {
        Some(cache) if cache.analyzes_like(&fresh) => cache,
        _ => fresh,
    }
}

/// Analyze, link, and validate a package as MIR, then lend the result to a target probe.
///
/// Package ownership remains inside radix-program so callers cannot retain
/// validation references or reconstruct library resolution independently.
/// Faber product seams enter through [`MirSeams`].
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn with_lowered_package_mir<R>(
    config: &Config,
    input: &Path,
    seams: &MirSeams,
    run: impl for<'a> FnOnce(&LoweredMirUnit<'a>) -> R,
) -> Result<R, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode_and_consumer(
        config,
        input,
        &[],
        CliPlanningMode::Parsed,
        PackageMirConsumer::ExternalTarget,
        seams,
        |_, lowered| Ok(run(lowered)),
    )
}

/// Analyze, link, and validate a package as MIR under the INTERPRETED
/// consumer (S5-U5c): the consumer that links library imports into the
/// merged program (the `faber run` path), so a test can probe the merged
/// MIR — and the device constructor — with a library-backed body such as the
/// gradus `train_step_*` surface. External-target probes ([`with_lowered_package_mir`])
/// keep the previous behavior: library calls stay provider-backed.
///
/// Test-facing probe: faber's device tests and the radix-program mir tests
/// consume it through the seams.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn with_interpreted_lowered_package_mir<R>(
    config: &Config,
    input: &Path,
    seams: &MirSeams,
    run: impl for<'a> FnOnce(&LoweredMirUnit<'a>) -> R,
) -> Result<R, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode_and_consumer(
        config,
        input,
        &[],
        CliPlanningMode::Parsed,
        PackageMirConsumer::Interpreted,
        seams,
        |_, lowered| Ok(run(lowered)),
    )
}

/// Analyze and lower a package for `check --lower`: plan the CLI surface
/// without supplying a runtime argument vector, using interpreted linking.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn with_check_lowered_package_mir<R>(
    config: &Config,
    input: &Path,
    seams: &MirSeams,
    run: impl for<'a> FnOnce(&LoweredMirUnit<'a>) -> R,
) -> Result<R, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode_and_consumer(
        config,
        input,
        &[],
        CliPlanningMode::Check,
        PackageMirConsumer::Interpreted,
        seams,
        |_, lowered| Ok(run(lowered)),
    )
}

/// Link an already-analyzed package's explicit test roots into one validated
/// MIR program.
///
/// This route deliberately does not select or require a package entry unit.
/// [`AnalyzedProgram::roots`] are the explicit test roots; `scope.remap_anchor`
/// is the named remapping/type-identity host. The local import closure of
/// those roots is taken from [`AnalyzedProgram::imports`]. Library imports
/// are linked through the normal interpreted consumer.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn with_linked_test_package_mir<R>(
    package: AnalyzedProgram,
    scope: &TestPackageScope,
    config: &Config,
    seams: &MirSeams,
    run: impl for<'a> FnOnce(&radix_mir::ValidatedMir<'a>, &TestFunctionIds) -> R,
) -> Result<R, Vec<Diagnostic>> {
    with_linked_test_package_lowered_mir(package, scope, config, seams, |lowered, ids| {
        run(&lowered.validated, ids)
    })
}

/// [`with_linked_test_package_mir`] handing the whole lowered unit to `run`:
/// a device session route (`faber test --device cpu`) builds its device image
/// from the unit's device-function metadata, not from the validated program
/// alone.
///
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn with_linked_test_package_lowered_mir<R>(
    package: AnalyzedProgram,
    scope: &TestPackageScope,
    config: &Config,
    seams: &MirSeams,
    run: impl for<'a> FnOnce(&LoweredMirUnit<'a>, &TestFunctionIds) -> R,
) -> Result<R, Vec<Diagnostic>> {
    if package
        .diagnostics
        .iter()
        .any(radix_module::Diagnostic::is_error)
    {
        return Err(package.diagnostics);
    }

    crate::qualify::require_contract_execution_targets(
        &package,
        radix_module::codegen::Target::MirStepper,
    )?;

    let mut library_cache = (seams.library_cache_for_config)(config);
    link_package_lowered_functions(package, scope, config, &mut library_cache, run)
}

/// Link the test roots of an already-analyzed package into one validated MIR
/// program, resolving library imports through the supplied cache. The core of
/// [`with_linked_test_package_mir`]: callers that already own a
/// [`LibraryInterfaceCache`] (and no [`MirSeams`]) enter here.
///
/// # Errors
/// Returns an error when scoping, linking, lowering, or validation fails.
pub(crate) fn link_package_functions<R>(
    package: AnalyzedProgram,
    scope: &TestPackageScope,
    config: &Config,
    library_cache: &mut LibraryInterfaceCache,
    run: impl for<'a> FnOnce(&radix_mir::ValidatedMir<'a>, &TestFunctionIds) -> R,
) -> Result<R, Vec<Diagnostic>> {
    link_package_lowered_functions(package, scope, config, library_cache, |lowered, ids| {
        run(&lowered.validated, ids)
    })
}

/// [`link_package_functions`] handing the whole lowered unit to `run`.
fn link_package_lowered_functions<R>(
    mut package: AnalyzedProgram,
    scope: &TestPackageScope,
    config: &Config,
    library_cache: &mut LibraryInterfaceCache,
    run: impl for<'a> FnOnce(&LoweredMirUnit<'a>, &TestFunctionIds) -> R,
) -> Result<R, Vec<Diagnostic>> {
    let library_resolver = package.library_resolver.clone();
    let anchor_id = apply_test_link_scope(&mut package, scope)?;
    let anchor_path = package
        .nodes
        .get(&anchor_id)
        .map(|node| node.path.clone())
        .ok_or_else(|| {
            vec![crate::package_diagnostic_error(
                "internal invariant: remap anchor is a program node",
            )]
        })?;
    let mut links = local_namespace_call_targets(
        &package,
        PackageMirConsumer::Interpreted,
        &library_resolver,
        library_cache,
        None,
    )?;
    kernel_import_calls::check_kernel_import_calls(
        &package,
        &links,
        &library_resolver,
        library_cache,
    )?;
    air_specialize::plan_package_instances(
        &mut package,
        &mut links,
        &library_resolver,
        library_cache,
    )?;
    reserve_test_scope_sources(&package, &anchor_path, &mut links);
    for unit in package.nodes.values_mut() {
        rewrite_unit_namespace_calls(
            unit,
            &links.calls,
            &links.data_member_targets,
            &links.namespaces,
            &links.method_targets,
        )?;
    }
    air_specialize::rewrite_package_instance_calls(&mut package, &links.air_calls);

    let cli_plan = CliPackagePlan::default();
    let mut lowered = lower_package_units(
        &mut package,
        &anchor_id,
        &links,
        &library_resolver,
        library_cache,
        &cli_plan,
        config.no_fuse,
        config.kernel_lowering,
    )?;
    lowered = canonicalize_direct_definition_callees(lowered, &anchor_path)?;
    validate_program(lowered.validated.program(), lowered.validated.validation()).map_err(
        |errors| {
            errors
                .into_iter()
                .map(|error| mir_validation_diag(&anchor_path, error))
                .collect::<Vec<_>>()
        },
    )?;
    let function_ids = linked_test_function_ids(&lowered, &links, scope, &anchor_path)?;
    Ok(run(&lowered, &function_ids))
}

pub(super) fn with_prepared_package_mir<R>(
    config: &Config,
    input: &Path,
    argumenta: &[String],
    seams: &MirSeams,
    run: impl for<'a> FnOnce(&PreparedPackageMir<'a>, &LoweredMirUnit<'a>) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode(
        config,
        input,
        argumenta,
        CliPlanningMode::Parsed,
        seams,
        run,
    )
}

pub(super) fn with_prepared_package_mir_with_cli_mode<R>(
    config: &Config,
    input: &Path,
    argumenta: &[String],
    cli_mode: CliPlanningMode,
    seams: &MirSeams,
    run: impl for<'a> FnOnce(&PreparedPackageMir<'a>, &LoweredMirUnit<'a>) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    with_prepared_package_mir_with_cli_mode_and_consumer(
        config,
        input,
        argumenta,
        cli_mode,
        PackageMirConsumer::Interpreted,
        seams,
        run,
    )
}

pub(super) fn with_prepared_package_mir_with_cli_mode_and_consumer<R>(
    config: &Config,
    input: &Path,
    argumenta: &[String],
    cli_mode: CliPlanningMode,
    consumer: PackageMirConsumer,
    seams: &MirSeams,
    run: impl for<'a> FnOnce(&PreparedPackageMir<'a>, &LoweredMirUnit<'a>) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    // ONE-EFFECTIVE-TARGET SEAM (TARGETLANE001 executed-lane routing,
    // `radix/docs/factory/executed-lane-target-decision.md`): the interpreted
    // (executed) package resolves its analysis target to a MIR-backed target —
    // an explicit MIR-backed config target > the manifest `[build] target` >
    // the executed-lane default `fmir`. Air-lane packages stop tripping
    // `lane_requires_mir_backed_target` at analysis because the executed route
    // selects its own MIR-backed target, not because the radix-side guard
    // relaxed. The same effective config feeds analysis and the run (ABI
    // source of truth — analysis and execution cannot diverge). External-
    // target probes keep the caller's config unchanged (decision §5).
    let effective_config;
    let config = match consumer {
        PackageMirConsumer::Interpreted => {
            let target = (seams.executed_lane_target)(config, input).map_err(|diag| vec![*diag])?;
            effective_config = config.clone().with_target(target);
            &effective_config
        }
        PackageMirConsumer::ExternalTarget => config,
    };
    let (package, analysis_library_cache) =
        crate::analyze::analyze_package_retaining_library_cache(config, input, &seams.analysis)?;
    prepare_package_mir(
        package,
        PackageMirPreparation {
            config,
            argumenta,
            cli_mode,
            consumer,
            loaded_links: None,
            analysis_library_cache: Some(analysis_library_cache),
        },
        seams,
        || (seams.device_config_for_input)(input),
        run,
    )
}

/// Shared package-MIR preparation pipeline for the source-analysis and
/// loaded-package drivers: diagnostics gate → CLI plan → namespace-link
/// resolution → entry selection → unit rewriting → lowering → validation →
/// runtime-requirement collection → device config (the source route reads the
/// manifest through the caller-selected seam; the loaded route supplies fixed
/// empty values) → a `PreparedPackageMir`, then `run`.
fn prepare_package_mir<R>(
    mut package: AnalyzedProgram,
    preparation: PackageMirPreparation<'_>,
    seams: &MirSeams,
    device_config: impl FnOnce() -> Result<DeviceManifestConfig, Vec<Diagnostic>>,
    run: impl for<'a> FnOnce(&PreparedPackageMir<'a>, &LoweredMirUnit<'a>) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    let PackageMirPreparation {
        config,
        argumenta,
        cli_mode,
        consumer,
        loaded_links,
        analysis_library_cache,
    } = preparation;
    if package
        .diagnostics
        .iter()
        .any(radix_module::Diagnostic::is_error)
    {
        return Err(package.diagnostics);
    }
    if cli_mode != CliPlanningMode::Check && consumer == PackageMirConsumer::Interpreted {
        // The external-target route lends MIR to inspection probes. Actual
        // external artifact builders enforce their target at the owning emit
        // seam; inspection retains the caller's existing CLI-planning rules.
        crate::qualify::require_contract_execution_targets(
            &package,
            radix_module::codegen::Target::MirStepper,
        )?;
    }
    // The package may arrive in the neutral FHIR form: analyzed under the
    // implicit portable target of a manifest, or loaded from artifacts. MIR
    // lowering needs literals, so qualify it for a MIR target first (the
    // runner when the analysis target is the neutral one). A package analyzed
    // for a MIR target is unchanged by this.
    let qualify_config = if radix_module::driver::keeps_authored_form(config.target) {
        config
            .clone()
            .with_target(radix_module::codegen::Target::MirStepper)
    } else {
        config.clone()
    };
    crate::qualify::qualify_program(&mut package, &qualify_config)?;
    let cli_plan = plan_cli_package(&mut package, argumenta, cli_mode)?;
    let library_resolver = package.library_resolver.clone();
    let mut library_cache = library_cache_for_preparation(config, seams, analysis_library_cache);
    let mut links = local_namespace_call_targets(
        &package,
        consumer,
        &library_resolver,
        &mut library_cache,
        loaded_links,
    )?;
    kernel_import_calls::check_kernel_import_calls(
        &package,
        &links,
        &library_resolver,
        &mut library_cache,
    )?;
    air_specialize::plan_package_instances(
        &mut package,
        &mut links,
        &library_resolver,
        &mut library_cache,
    )?;
    let entry_id = select_entry_node(&package)?;
    let entry_path = package
        .nodes
        .get(&entry_id)
        .map(|node| node.path.clone())
        .ok_or_else(|| {
            vec![crate::package_diagnostic_error(
                "internal invariant: entry id is a program node",
            )]
        })?;
    let source_paths = package
        .nodes
        .values()
        .map(|unit| unit.path.clone())
        .collect();
    for unit in package.nodes.values_mut() {
        rewrite_unit_namespace_calls(
            unit,
            &links.calls,
            &links.data_member_targets,
            &links.namespaces,
            &links.method_targets,
        )?;
    }
    air_specialize::rewrite_package_instance_calls(&mut package, &links.air_calls);

    let mut lowered = lower_package_units(
        &mut package,
        &entry_id,
        &links,
        &library_resolver,
        &mut library_cache,
        &cli_plan,
        config.no_fuse,
        config.kernel_lowering,
    )?;
    // The radix-air lowering emits every call as a `Definition` edge; the
    // boundary-ABI lane only admits direct-call shapes, so unambiguous
    // single-target definition edges are canonicalized to `Function` callees
    // before the final validation gate.
    lowered = canonicalize_direct_definition_callees(lowered, &entry_path)?;
    validate_program(lowered.validated.program(), lowered.validated.validation()).map_err(
        |errors| {
            errors
                .into_iter()
                .map(|error| mir_validation_diag(&entry_path, error))
                .collect::<Vec<_>>()
        },
    )?;
    let runtime_requirements = collect_package_runtime_requirements(&lowered, &cli_plan);
    let device_config = device_config()?;
    let prepared = PreparedPackageMir {
        entry_path: entry_path.clone(),
        source_paths,
        runtime_requirements,
        cli_exit_code: cli_plan.exit_code,
        fmir_text_cli: cli_plan.fmir_text_cli.clone(),
        device_inputs: device_config.inputs,
        device_selection: device_config.selection,
        device_steps: device_config.steps,
        device_declared: device_config.declared,
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
    package: AnalyzedProgram,
    loaded_links: &BTreeMap<PathBuf, BTreeMap<String, PathBuf>>,
    argumenta: &[String],
    seams: &MirSeams,
    run: impl for<'a> FnOnce(&PreparedPackageMir<'a>, &LoweredMirUnit<'a>) -> Result<R, Vec<Diagnostic>>,
) -> Result<R, Vec<Diagnostic>> {
    // The FHIR-loaded route reconstructs the package from an envelope; no
    // filesystem manifest is consulted here, so no device payload is
    // constructed on this route (N1.1: source routes reject explicit GPU
    // requests; `auto` keeps the CPU route).
    prepare_package_mir(
        package,
        PackageMirPreparation {
            config,
            argumenta,
            cli_mode: CliPlanningMode::Parsed,
            consumer: PackageMirConsumer::Interpreted,
            loaded_links: Some(loaded_links),
            analysis_library_cache: None,
        },
        seams,
        || Ok(DeviceManifestConfig::default()),
        run,
    )
}

/// Restrict the program to the local import closure of [`AnalyzedProgram::roots`]
/// and resolve the named remapping host. Uses the recorded import graph —
/// no path-scan reconstruction and no first-root inference. The remapping
/// host is always retained, even when it is not itself a test root.
fn apply_test_link_scope(
    package: &mut AnalyzedProgram,
    scope: &TestPackageScope,
) -> Result<ModuleId, Vec<Diagnostic>> {
    if package.roots.is_empty() {
        return Err(vec![
            crate::package_diagnostic_error(
                "package MIR test link requires at least one test root",
            )
            .with_file(package.spec.entry.display().to_string()),
        ]);
    }
    let Some((anchor_id, _)) = package.node_by_path(&scope.remap_anchor) else {
        return Err(vec![
            crate::package_diagnostic_error(
                "package MIR test link remap anchor is not an analyzed program node",
            )
            .with_file(scope.remap_anchor.display().to_string()),
        ]);
    };
    let anchor_id = anchor_id.clone();
    let mut included = package.imports.local_closure(package.roots.iter());
    included.insert(anchor_id.clone());
    package.nodes.retain(|id, _| included.contains(id));
    if !package.nodes.contains_key(&anchor_id) {
        return Err(vec![
            crate::package_diagnostic_error(
                "package MIR test link lost its named remapping anchor",
            )
            .with_file(scope.remap_anchor.display().to_string()),
        ]);
    }
    Ok(anchor_id)
}

fn reserve_test_scope_sources(
    package: &AnalyzedProgram,
    anchor_path: &Path,
    links: &mut PackageMirLinks,
) {
    for unit in package.nodes.values() {
        if unit.path == anchor_path {
            continue;
        }
        let mut definitions = Vec::new();
        for item in &unit.analysis.hir.items {
            match &item.kind {
                HirItemKind::Function(_) => definitions.push(item.def_id),
                HirItemKind::Struct(strukt) => {
                    definitions.extend(strukt.methods.iter().map(|method| method.def_id));
                }
                _ => {}
            }
        }
        for source in definitions {
            if links.sources.contains_key(&(unit.path.clone(), source)) {
                continue;
            }
            let synthetic = DefId(links.next_synthetic);
            links.next_synthetic += 1;
            links.sources.insert((unit.path.clone(), source), synthetic);
        }
    }
}

fn linked_test_function_ids(
    lowered: &LoweredMirUnit<'_>,
    links: &PackageMirLinks,
    scope: &TestPackageScope,
    diagnostic_path: &Path,
) -> Result<TestFunctionIds, Vec<Diagnostic>> {
    let mut ids = HashMap::new();
    for (path, original) in &scope.functions {
        let source = links
            .sources
            .get(&(path.clone(), *original))
            .copied()
            .unwrap_or(*original);
        let matches = lowered
            .validated
            .program()
            .functions
            .iter()
            .filter(|function| function.source == Some(source))
            .map(|function| function.id)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [function_id] => {
                ids.insert((path.clone(), *original), *function_id);
            }
            [] => {
                return Err(vec![mir_diag(
                    diagnostic_path,
                    format!(
                        "package MIR test function `{}` def#{} has no linked MIR function",
                        path.display(),
                        original.0
                    ),
                )]);
            }
            _ => {
                return Err(vec![mir_diag(
                    diagnostic_path,
                    format!(
                        "package MIR test function `{}` def#{} has multiple linked MIR functions",
                        path.display(),
                        original.0
                    ),
                )]);
            }
        }
    }
    Ok(ids)
}

pub(super) fn select_entry_node(package: &AnalyzedProgram) -> Result<ModuleId, Vec<Diagnostic>> {
    match package.entry.clone() {
        Some(id) if package.nodes.get(&id).is_some_and(|node| node.is_entry) => Ok(id),
        Some(_) | None => {
            let entries = package
                .nodes
                .iter()
                .filter(|(_, node)| node.is_entry)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            match entries.as_slice() {
                [id] => Ok(id.clone()),
                [] => Err(vec![
                    crate::package_diagnostic_error(
                        "package MIR run requires exactly one entry unit",
                    )
                    .with_file(package.spec.entry.display().to_string()),
                ]),
                _ => Err(vec![
                    crate::package_diagnostic_error("package MIR run found multiple entry units")
                        .with_file(package.spec.entry.display().to_string()),
                ]),
            }
        }
    }
}
