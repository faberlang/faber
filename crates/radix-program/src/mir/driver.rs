//! Package-MIR driver: prepare a package and lend it to a probe or lower it for a route.

use super::{
    AnalyzedProgram, BTreeMap, CliPackagePlan, CliPlanningMode, Config, DefId,
    DeviceManifestConfig, Diagnostic, HashMap, HirItemKind, LoweredMirUnit, MirIntrinsic,
    MirProviderKind, MirRuntimeCall, MirSeams, MirStatementKind, ModuleId, PackageMirConsumer,
    PackageMirLinks, Path, PathBuf, PreparedPackageMir, TestFunctionIds, TestPackageScope,
    air_specialize, canonicalize_direct_definition_callees, collect_package_runtime_requirements,
    local_namespace_call_targets, lower_package_units, mir_diag, mir_lowering_diag,
    plan_cli_package, rewrite_unit_namespace_calls, validate_program,
};

struct PackageMirPreparation<'a> {
    config: &'a Config,
    argumenta: &'a [String],
    cli_mode: CliPlanningMode,
    consumer: PackageMirConsumer,
    loaded_links: Option<&'a BTreeMap<PathBuf, BTreeMap<String, PathBuf>>>,
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
/// # Panics
/// Panics when required internal state is absent.
pub fn with_linked_test_package_mir<R>(
    mut package: AnalyzedProgram,
    scope: &TestPackageScope,
    config: &Config,
    seams: &MirSeams,
    run: impl for<'a> FnOnce(&radix_module::mir::ValidatedMir<'a>, &TestFunctionIds) -> R,
) -> Result<R, Vec<Diagnostic>> {
    if package
        .diagnostics
        .iter()
        .any(radix_module::Diagnostic::is_error)
    {
        return Err(package.diagnostics);
    }

    let library_resolver = package.library_resolver.clone();
    let mut library_cache = (seams.library_cache_for_config)(config);
    let anchor_id = apply_test_link_scope(&mut package, scope)?;
    let anchor_path = package
        .nodes
        .get(&anchor_id)
        .map(|node| node.path.clone())
        .expect("remap anchor is a program node");
    let mut links = local_namespace_call_targets(
        &package,
        PackageMirConsumer::Interpreted,
        &library_resolver,
        &mut library_cache,
        None,
    )?;
    air_specialize::plan_package_instances(&mut package, &mut links)?;
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
        &mut library_cache,
        &cli_plan,
        config.no_fuse,
    )?;
    canonicalize_direct_definition_callees(&mut lowered, &anchor_path)?;
    validate_program(&lowered.program, lowered.validated.validation()).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| mir_lowering_diag(&anchor_path, error.message))
            .collect::<Vec<_>>()
    })?;
    bridge_norma_providers_to_kernel(&mut lowered, &anchor_path)?;
    let function_ids = linked_test_function_ids(&lowered, &links, scope, &anchor_path)?;
    Ok(run(&lowered.validated, &function_ids))
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
            effective_config = config
                .clone()
                .with_target((seams.executed_lane_target)(config, input));
            &effective_config
        }
        PackageMirConsumer::ExternalTarget => config,
    };
    let package = crate::analyze::analyze_package(config, input, &seams.analysis)?;
    prepare_package_mir(
        package,
        PackageMirPreparation {
            config,
            argumenta,
            cli_mode,
            consumer,
            loaded_links: None,
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
    } = preparation;
    if package
        .diagnostics
        .iter()
        .any(radix_module::Diagnostic::is_error)
    {
        return Err(package.diagnostics);
    }
    let cli_plan = plan_cli_package(&mut package, argumenta, cli_mode)?;
    let library_resolver = package.library_resolver.clone();
    let mut library_cache = (seams.library_cache_for_config)(config);
    let mut links = local_namespace_call_targets(
        &package,
        consumer,
        &library_resolver,
        &mut library_cache,
        loaded_links,
    )?;
    air_specialize::plan_package_instances(&mut package, &mut links)?;
    let entry_id = select_entry_node(&package)?;
    let entry_path = package
        .nodes
        .get(&entry_id)
        .map(|node| node.path.clone())
        .expect("entry id is a program node");
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
    )?;
    // The radix-air lowering emits every call as a `Definition` edge; the
    // boundary-ABI lane only admits direct-call shapes, so unambiguous
    // single-target definition edges are canonicalized to `Function` callees
    // before the final validation gate.
    canonicalize_direct_definition_callees(&mut lowered, &entry_path)?;
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
    let device_config = device_config()?;
    let prepared = PreparedPackageMir {
        entry_path: entry_path.clone(),
        source_paths,
        runtime_requirements,
        cli_exit_code: cli_plan.exit_code,
        fmir_text_cli: cli_plan.fmir_text_cli.clone(),
        device_inputs: device_config.inputs,
        device_backend: device_config.backend,
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
            .program
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

/// Whether an import path string (`norma:solum`) names a kernel-manifest
/// module the interpreted-package bridge can satisfy. Shared by the
/// library-import allowlist (identity-based) and the namespace-link pass
/// (path-based) so the two rejection sites agree.
pub(super) fn is_bridged_norma_import_path(path: &str) -> bool {
    path.strip_prefix("norma:")
        .and_then(radix_module::kernel::resolve_kernel_module_name)
        .is_some()
}

/// Bridge interpreted `norma:<kernel-manifest-module>` providers to the
/// in-process runner kernel.
///
/// Post-validation transform (see
/// `docs/archived/faber-script-runtime/stage1b-package-host-bridge.md`). For
/// each `Package` provider whose module resolves to a kernel-manifest module,
/// rewrite `kind` to `Kernel(module)` when the called verb is in the manifest
/// subset; otherwise fail closed with an actionable diagnostic. Compiled
/// package execution is unaffected (it never runs this path).
///
/// RETIRE: delete this pass once core-stdlib Stage 8 routes `norma:*` over
/// `ad` to the Rust frame runtime in the runner.
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
                let Some(module) = radix_module::kernel::resolve_kernel_module_name(norma_module)
                else {
                    // Non-manifest `norma:*` stays as Package; the runner's
                    // `host.provider()` reports it unsupported.
                    continue;
                };
                let verb = interner.resolve(provider.name);
                if radix_module::kernel::kernel_module_supports_verb(module, verb) {
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
