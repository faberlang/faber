//! FMIR runtime-requirement checks and collection.

use super::*;

pub(super) fn check_fmir_runtime_requirements(
    image: &FmirPackageImage,
) -> Result<(), Vec<Diagnostic>> {
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

pub(super) fn is_known_fmir_runtime_requirement(requirement: &str) -> bool {
    is_known_host_requirement(requirement) || is_known_fmir_kernel_requirement(requirement)
}

pub(super) fn is_known_fmir_kernel_requirement(requirement: &str) -> bool {
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

pub(super) fn collect_package_runtime_requirements(
    lowered: &LoweredMirUnit<'_>,
    cli_plan: &CliPackagePlan,
) -> Vec<String> {
    let mut requirements = BTreeSet::new();
    let interner = lowered.validated.validation().interner;
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

pub(super) fn collect_runtime_call_requirement(
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
        | MirIntrinsic::GpuBuiltin(_)
        | MirIntrinsic::Gradient(_)
        | MirIntrinsic::TypeCheck(_) => {}
        MirIntrinsic::ReadLine => {
            requirements.insert("host:stdin".to_owned());
        }
        MirIntrinsic::CursorStream(_) => {}
    }
}

pub(super) fn collect_kernel_host_requirements(
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
        // TOML parse/serialize is in-process (no host capability).
        radix::kernel::KernelModule::Toml => {}
    }
}
