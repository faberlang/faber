//! Package CLI planning (root entry, argument parsing, subcommands) and the
//! semantic-type import surface used when rehoming types between analysis
//! `TypeTable`s (source → target with canonical nominal identity).

use super::cli_values::plan_cli_command_records_check;
use super::{
    AnalyzedProgram, AnalyzedProgramNode, BTreeSet, CliCommand, CliDefault, CliDispatchPlan,
    CliEntryRecords, CliMode, CliOperand, CliOption, CliPackagePlan, CliPlanningMode, CliProgram,
    CliRecordFieldsByType, CliRecordTypeRewrite, CliType, DefId, Diagnostic, FmirTextCliOperand,
    FmirTextCliRootSection, FmirTextCliSection, FmirTextCliValueType, HashMap, HirItemKind,
    HirStatementKind, IndexExpr, InterfaceLibraryIdentity, InterfaceNominalKind, Interner,
    MirConstant, MirOperand, MirRuntimeRecordField, MirRuntimeRecordValue, MirType,
    MissingOperandPolicy, ModuleId, PlannedCliOperand, PlannedCliOption, Resolver, Symbol,
    SymbolKind, Type, TypeId, TypeTable, cli_default_value, cli_operand_consumes_many,
    cli_operand_record_fields, cli_option_is_nullable, cli_option_value, cli_record_type,
    command_has_unsupported_options, command_in_unit, command_unit_id,
    has_unsupported_package_mir_operands, is_package_mir_supported_option, matching_cli_command,
    package_mir_cli_exit_code, plan_cli_command_records, push_cli_option_default_diagnostic,
    unsupported_cli_diagnostic,
};
use radix_module::driver::AnalyzedModule;
use radix_module::file_interface::{FileExportKind, FileInterface};

#[cfg(test)]
#[path = "cli_plan_test.rs"]
mod tests;

/// Names of the nominal exports (struct/enum/interface) declared by a source
/// unit's own file interface — the identity-bearing nominals whose canonical
/// home-module key is the source module's own identity.
pub(super) fn exported_nominal_names(interface: &FileInterface) -> BTreeSet<String> {
    interface
        .exports
        .values()
        .filter_map(|export| match &export.kind {
            FileExportKind::Struct(_) | FileExportKind::Enum(_) | FileExportKind::Interface(_) => {
                Some(export.name.clone())
            }
            FileExportKind::Function(_)
            | FileExportKind::TypeAlias(_)
            | FileExportKind::Const(..) => None,
        })
        .collect()
}

fn program_entry_id(package: &AnalyzedProgram) -> Option<ModuleId> {
    package.entry.clone().or_else(|| {
        package
            .nodes
            .iter()
            .find(|(_, node)| node.is_entry)
            .map(|(id, _)| id.clone())
    })
}

pub(super) fn plan_cli_package(
    package: &mut AnalyzedProgram,
    argumenta: &[String],
    mode: CliPlanningMode,
) -> Result<CliPackagePlan, Vec<Diagnostic>> {
    let mut plan = CliPackagePlan::default();
    let mut diagnostics = Vec::new();
    let Some(entry_id) = program_entry_id(package) else {
        return Ok(plan);
    };
    let Some(cli_program) = package
        .nodes
        .get(&entry_id)
        .and_then(|node| node.analysis.cli_program.clone())
    else {
        return Ok(plan);
    };
    plan.uses_cli_runtime = true;

    match cli_program.mode {
        CliMode::SingleCommand => {
            // The entry node was found above through `cli_program`.
            let Some(entry_node) = package.nodes.get_mut(&entry_id) else {
                return Ok(plan);
            };
            if mode == CliPlanningMode::FmirTextRuntime {
                if let Some((records, cli_section, exit_code)) =
                    plan_fmir_text_runtime_cli_root_entry(
                        entry_node,
                        &cli_program,
                        &mut diagnostics,
                    )
                {
                    plan.exit_code = exit_code;
                    plan.entry_records.insert(entry_node.path.clone(), records);
                    plan.fmir_text_cli = Some(cli_section);
                }
            } else {
                let planned = if mode == CliPlanningMode::Check {
                    plan_cli_root_entry_check(entry_node, &cli_program, &mut diagnostics)
                } else {
                    plan_cli_root_entry(entry_node, &cli_program, argumenta, &mut diagnostics)
                };
                if let Some((records, exit_code)) = planned {
                    plan.exit_code = exit_code;
                    plan.entry_records.insert(entry_node.path.clone(), records);
                }
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
            if mode == CliPlanningMode::Check {
                if let Some(entry_records) =
                    plan_cli_subcommand_check(package, &cli_program, &mut diagnostics)
                {
                    plan.entry_records = entry_records;
                }
            } else {
                plan.dispatch = plan_cli_subcommand(
                    package,
                    &cli_program,
                    argumenta,
                    &mut plan.entry_records,
                    &mut diagnostics,
                );
            }
        }
        CliMode::NotCli => {}
    }

    if diagnostics.is_empty() {
        Ok(plan)
    } else {
        Err(diagnostics)
    }
}

pub(super) fn plan_fmir_text_runtime_cli_root_entry(
    unit: &mut AnalyzedProgramNode,
    cli_program: &CliProgram,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(CliRecordFieldsByType, FmirTextCliSection, Option<i32>)> {
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
    let exit_code =
        package_mir_cli_exit_code(cli_program.exit.as_ref(), None, &[], unit, diagnostics);
    let operands = cli_program
        .global_operands
        .iter()
        .chain(&cli_program.operands)
        .collect::<Vec<_>>();
    if operands.iter().enumerate().any(|(index, operand)| {
        !is_fmir_text_runtime_supported_operand(operand, index + 1 == operands.len())
    }) {
        diagnostics.push(unsupported_cli_diagnostic(
            &unit.path,
            "fmir-text runtime CLI operands beyond required scalars and a final rest textus list",
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
        let ty = fmir_text_runtime_cli_value_type(operand.operand)?;
        fields.push(MirRuntimeRecordField {
            name: unit.analysis.interner.intern(&operand.binding_name),
            value: fmir_text_cli_placeholder_record_value(unit, operand.operand, &ty)?,
        });
        image_operands.push(FmirTextCliOperand {
            field: operand.binding_name,
            ty,
        });
    }
    let record = unit.analysis.interner.resolve(args_name).to_owned();
    let fields_by_type = if let Some(record_ty) = entry_cli_args_type(unit) {
        HashMap::from([(record_ty, fields)])
    } else {
        if !fields.is_empty() {
            diagnostics.push(unsupported_cli_diagnostic(
                &unit.path,
                "CLI argument records",
            ));
            return None;
        }
        HashMap::new()
    };
    Some((
        fields_by_type,
        FmirTextCliSection {
            root: FmirTextCliRootSection {
                record,
                operand: image_operands,
            },
        },
        exit_code,
    ))
}

pub(super) fn fmir_text_cli_value_type(ty: &CliType) -> Option<FmirTextCliValueType> {
    match ty {
        CliType::Textus | CliType::Ignotum => Some(FmirTextCliValueType::Textus),
        CliType::Numerus => Some(FmirTextCliValueType::Numerus),
        CliType::Fractus => Some(FmirTextCliValueType::Fractus),
        CliType::Bivalens => Some(FmirTextCliValueType::Bivalens),
        CliType::ListaTextus => Some(FmirTextCliValueType::ListaTextus),
        CliType::Octeti | CliType::ListaNumerus => None,
    }
}

/// Image-runtime operand type: a final rest `textus` list is `ListaTextus`,
/// not a scalar `Textus` placeholder.
pub(super) fn fmir_text_runtime_cli_value_type(
    operand: &radix_module::cli::CliOperand,
) -> Option<FmirTextCliValueType> {
    if operand.rest {
        return matches!(
            operand.ty,
            CliType::Textus | CliType::Ignotum | CliType::ListaTextus
        )
        .then_some(FmirTextCliValueType::ListaTextus);
    }
    fmir_text_cli_value_type(&operand.ty)
}

pub(super) fn is_fmir_text_runtime_supported_operand(
    operand: &radix_module::cli::CliOperand,
    is_final: bool,
) -> bool {
    if operand.default.is_some() {
        return false;
    }
    if operand.rest {
        return is_final
            && matches!(
                operand.ty,
                CliType::Textus | CliType::Ignotum | CliType::ListaTextus
            );
    }
    if matches!(operand.ty, CliType::ListaTextus) {
        return is_final;
    }
    matches!(
        operand.ty,
        CliType::Textus
            | CliType::Ignotum
            | CliType::Numerus
            | CliType::Fractus
            | CliType::Bivalens
    )
}

pub(super) fn fmir_text_cli_placeholder_value(
    unit: &mut AnalyzedProgramNode,
    ty: &FmirTextCliValueType,
) -> MirOperand {
    let constant = match ty {
        FmirTextCliValueType::Textus | FmirTextCliValueType::ListaTextus => {
            MirConstant::String(unit.analysis.interner.intern("__fmir_runtime_arg__"))
        }
        FmirTextCliValueType::Numerus => MirConstant::Int(0),
        FmirTextCliValueType::Fractus => MirConstant::Float(0.0),
        FmirTextCliValueType::Bivalens => MirConstant::Bool(false),
    };
    MirOperand::Constant(constant)
}

pub(super) fn fmir_text_cli_placeholder_record_value(
    unit: &mut AnalyzedProgramNode,
    operand: &radix_module::cli::CliOperand,
    ty: &FmirTextCliValueType,
) -> Option<MirRuntimeRecordValue> {
    if *ty == FmirTextCliValueType::ListaTextus {
        let list_ty = cli_record_type(&mut unit.analysis.types, &operand.ty, operand.rest);
        return Some(MirRuntimeRecordValue::Array {
            ty: MirType::semantic(list_ty),
            items: Vec::new(),
        });
    }
    Some(MirRuntimeRecordValue::Operand(
        fmir_text_cli_placeholder_value(unit, ty),
    ))
}

pub(super) fn plan_cli_root_entry(
    unit: &mut AnalyzedProgramNode,
    cli_program: &CliProgram,
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(CliRecordFieldsByType, Option<i32>)> {
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
    let declared_operands = cli_program
        .global_operands
        .iter()
        .chain(&cli_program.operands)
        .collect::<Vec<_>>();
    if has_unsupported_package_mir_operands(declared_operands.iter().copied()) {
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
    let operands = planned_cli_operands(&unit.analysis.interner, declared_operands.into_iter());
    let mut fields = parsed.option_fields;
    fields.extend(cli_operand_record_fields(
        unit,
        &operands,
        &parsed.positionals,
        diagnostics,
        MissingOperandPolicy::Reject,
    )?);
    finish_cli_root_entry(unit, cli_program, fields, diagnostics)
}

/// `faber check --lower` planning for a single-command root entry.
///
/// The argv is a runtime question, so this path keeps every surface rejection
/// of [`plan_cli_root_entry`] but never parses arguments: options take their
/// declared defaults and operands take their `vel` default or a typed
/// placeholder for the value the runner will supply.
pub(super) fn plan_cli_root_entry_check(
    unit: &mut AnalyzedProgramNode,
    cli_program: &CliProgram,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(CliRecordFieldsByType, Option<i32>)> {
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
    let declared_operands = cli_program
        .global_operands
        .iter()
        .chain(&cli_program.operands)
        .collect::<Vec<_>>();
    if has_unsupported_package_mir_operands(declared_operands.iter().copied()) {
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
    let mut fields = options
        .iter()
        .map(|option| cli_option_default_field(unit, option, diagnostics))
        .collect::<Option<Vec<_>>>()?;
    let operands = planned_cli_operands(&unit.analysis.interner, declared_operands.into_iter());
    fields.extend(cli_operand_record_fields(
        unit,
        &operands,
        &[],
        diagnostics,
        MissingOperandPolicy::Check,
    )?);
    finish_cli_root_entry(unit, cli_program, fields, diagnostics)
}

fn finish_cli_root_entry(
    unit: &mut AnalyzedProgramNode,
    cli_program: &CliProgram,
    fields: Vec<MirRuntimeRecordField>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(CliRecordFieldsByType, Option<i32>)> {
    let args_name = unit.analysis.hir.entry_args_name;
    let Some(record_ty) = entry_cli_args_type(unit) else {
        if !fields.is_empty() {
            diagnostics.push(unsupported_cli_diagnostic(
                &unit.path,
                "CLI argument records",
            ));
            return None;
        }
        let exit_code =
            package_mir_cli_exit_code(cli_program.exit.as_ref(), None, &[], unit, diagnostics);
        return Some((HashMap::new(), exit_code));
    };
    let exit_code = package_mir_cli_exit_code(
        cli_program.exit.as_ref(),
        args_name,
        &fields,
        unit,
        diagnostics,
    );
    Some((HashMap::from([(record_ty, fields)]), exit_code))
}

pub(super) struct ParsedCliArguments {
    pub(super) option_fields: Vec<MirRuntimeRecordField>,
    pub(super) positionals: Vec<String>,
    pub(super) consumed: usize,
}

pub(super) fn planned_cli_options<'a>(
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

pub(super) fn planned_cli_operands<'a>(
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

pub(super) fn parse_cli_arguments(
    unit: &mut AnalyzedProgramNode,
    options: &[PlannedCliOption<'_>],
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ParsedCliArguments> {
    parse_cli_arguments_with_mode(unit, options, argumenta, diagnostics, false)
}

pub(super) fn parse_leading_cli_options(
    unit: &mut AnalyzedProgramNode,
    options: &[PlannedCliOption<'_>],
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ParsedCliArguments> {
    parse_cli_arguments_with_mode(unit, options, argumenta, diagnostics, true)
}

pub(super) fn parse_cli_arguments_with_mode(
    unit: &mut AnalyzedProgramNode,
    options: &[PlannedCliOption<'_>],
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
    stop_at_first_positional: bool,
) -> Option<ParsedCliArguments> {
    if options.is_empty() {
        // `--` ends option parsing even when none are declared. Leading mode
        // stops at the first argument, so only a leading `--` is consumed.
        let mut positionals = argumenta.to_vec();
        if let Some(end) = argumenta.iter().position(|argument| argument == "--")
            && (!stop_at_first_positional || end == 0)
        {
            positionals.remove(end);
        }
        return Some(ParsedCliArguments {
            option_fields: Vec::new(),
            positionals,
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
        // WHY: the generated Rust parser and `runtime/llvm` both read a bare
        // `--` as end-of-options, so a dash-leading operand stays passable.
        if argument == "--" {
            positionals.extend(argumenta[argument_index + 1..].iter().cloned());
            return Some(ParsedCliArguments {
                option_fields,
                positionals,
                consumed: if stop_at_first_positional {
                    argument_index
                } else {
                    argumenta.len()
                },
            });
        }
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
            let raw = if let Some(value) = inline_value {
                value
            } else {
                argument_index += 1;
                let Some(value) = argumenta.get(argument_index) else {
                    push_cli_option_missing_value_diagnostic(unit, argument, diagnostics);
                    return None;
                };
                value.clone()
            };
            option_fields[option_index].value = cli_option_value(unit, option, &raw, diagnostics)?;
            argument_index += 1;
            continue;
        }
        if let Some(name) = argument.strip_prefix('-')
            && !name.is_empty()
        {
            let Some(option_index) = options
                .iter()
                .position(|option| option.option.short.as_deref() == Some(name))
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
            argument_index += 1;
            let Some(raw) = argumenta.get(argument_index) else {
                push_cli_option_missing_value_diagnostic(unit, argument, diagnostics);
                return None;
            };
            option_fields[option_index].value = cli_option_value(unit, option, raw, diagnostics)?;
            argument_index += 1;
            continue;
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

pub(super) fn cli_option_default_field(
    unit: &mut AnalyzedProgramNode,
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

pub(super) fn push_cli_option_match_diagnostic(
    unit: &AnalyzedProgramNode,
    option: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR could not match CLI option `{option}`; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string())
        .with_arg("issue", "package_mir_cli_surface_unsupported")
        .with_arg("surface", "CLI option match"),
    );
}

pub(super) fn push_cli_option_missing_value_diagnostic(
    unit: &AnalyzedProgramNode,
    option: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR expected a value for CLI option `{option}`; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string())
        .with_arg("issue", "package_mir_cli_surface_unsupported")
        .with_arg("surface", "CLI option value"),
    );
}

pub(super) fn plan_cli_subcommand(
    package: &mut AnalyzedProgram,
    cli_program: &CliProgram,
    argumenta: &[String],
    entry_records: &mut CliEntryRecords,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CliDispatchPlan> {
    let entry_path = package.spec.entry.clone();
    if !subcommand_cli_surface_supported(cli_program, &entry_path, diagnostics) {
        return None;
    }

    let entry_id = program_entry_id(package)?;
    let global_options = planned_cli_options(
        &package.nodes.get(&entry_id)?.analysis.interner,
        cli_program.global_options.iter(),
    );
    let global_operands = planned_cli_operands(
        &package.nodes.get(&entry_id)?.analysis.interner,
        cli_program.global_operands.iter(),
    );
    let parsed_globals = parse_leading_cli_options(
        package.nodes.get_mut(&entry_id)?,
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
    let entry_args_ty = package.nodes.get(&entry_id).and_then(entry_cli_args_type);
    if let Some(args_ty) = entry_args_ty {
        let mut entry_fields = parsed_globals.option_fields.clone();
        let global_operand_args_len = cli_operand_consumed_len(&global_operands, command_args);
        entry_fields.extend(cli_operand_record_fields(
            package.nodes.get_mut(&entry_id)?,
            &global_operands,
            &command_args[..global_operand_args_len],
            diagnostics,
            MissingOperandPolicy::Reject,
        )?);
        if !entry_fields.is_empty() {
            entry_records
                .entry(package.nodes.get(&entry_id)?.path.clone())
                .or_default()
                .insert(args_ty, entry_fields);
        }
    }
    let command = command_match.command;
    let Some(unit_id) = command_unit_id(package, command) else {
        diagnostics.push(
            crate::package_diagnostic_error(format!(
                "package MIR could not resolve CLI command module for `{}`",
                command.path.join(" ")
            ))
            .with_file(entry_path.display().to_string()),
        );
        return None;
    };
    let target_command = command_in_unit(package.nodes.get(&unit_id)?, command)
        .cloned()
        .unwrap_or_else(|| command.clone());
    let mut record_type_rewrite = selected_command_record_type_rewrite(
        package,
        &unit_id,
        &target_command,
        &global_options,
        &global_operands,
    );
    let unit = package.nodes.get_mut(&unit_id)?;
    let unit_path = unit.path.clone();
    let global_fields = if unit_id == entry_id {
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
        add_cli_runtime_field_type_rewrites(package, &unit_id, &entry_id, rewrite, fields);
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

/// `faber check --lower` planning for a subcommand root entry.
///
/// The argv is a runtime question, so this path keeps every surface rejection
/// of [`plan_cli_subcommand`] but never parses arguments or matches a command:
/// global options take their declared defaults, operands take their `vel`
/// default or a typed placeholder, and each command that declares an args
/// binding plans its own record. No dispatch entry is installed; the root body
/// is required to be empty, so nothing needs one.
pub(super) fn plan_cli_subcommand_check(
    package: &mut AnalyzedProgram,
    cli_program: &CliProgram,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CliEntryRecords> {
    let entry_path = package.spec.entry.clone();
    if !subcommand_cli_surface_supported(cli_program, &entry_path, diagnostics) {
        return None;
    }

    let entry_id = program_entry_id(package)?;
    let global_options = planned_cli_options(
        &package.nodes.get(&entry_id)?.analysis.interner,
        cli_program.global_options.iter(),
    );
    let global_operands = planned_cli_operands(
        &package.nodes.get(&entry_id)?.analysis.interner,
        cli_program.global_operands.iter(),
    );
    let mut entry_records = CliEntryRecords::new();

    let entry_unit = package.nodes.get_mut(&entry_id)?;
    let mut entry_fields = global_options
        .iter()
        .map(|option| cli_option_default_field(entry_unit, option, diagnostics))
        .collect::<Option<Vec<_>>>()?;
    entry_fields.extend(cli_operand_record_fields(
        entry_unit,
        &global_operands,
        &[],
        diagnostics,
        MissingOperandPolicy::Check,
    )?);
    if let Some(record_ty) = entry_cli_args_type(entry_unit)
        && !entry_fields.is_empty()
    {
        entry_records
            .entry(entry_unit.path.clone())
            .or_default()
            .insert(record_ty, entry_fields);
    }

    for command in &cli_program.commands {
        // A command with no args binding takes no argument record; planning the
        // global fields for it would trip the `CLI argument records` rejection.
        if command.args_binding.is_none() {
            continue;
        }
        let Some(unit_id) = command_unit_id(package, command) else {
            diagnostics.push(
                crate::package_diagnostic_error(format!(
                    "package MIR could not resolve CLI command module for `{}`",
                    command.path.join(" ")
                ))
                .with_file(entry_path.display().to_string()),
            );
            return None;
        };
        let target_command = command_in_unit(package.nodes.get(&unit_id)?, command)
            .cloned()
            .unwrap_or_else(|| command.clone());
        let unit = package.nodes.get_mut(&unit_id)?;
        let unit_path = unit.path.clone();
        let record_fields = plan_cli_command_records_check(
            unit,
            &target_command,
            &global_options,
            &global_operands,
            diagnostics,
        )?;
        if let Some(fields) = record_fields {
            entry_records.entry(unit_path).or_default().extend(fields);
        }
    }

    Some(entry_records)
}

/// Validate the subcommand CLI surface this planner can dispatch, pushing one
/// unsupported-surface diagnostic for each part it cannot. Returns `false`
/// when any diagnostic was pushed.
fn subcommand_cli_surface_supported(
    cli_program: &CliProgram,
    entry_path: &std::path::Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let diagnostic_count = diagnostics.len();
    if cli_program
        .global_options
        .iter()
        .any(|option| !is_package_mir_supported_option(option))
    {
        diagnostics.push(unsupported_cli_diagnostic(
            entry_path,
            "CLI global options beyond boolean flags and scalar values",
        ));
    }
    if !cli_program.options.is_empty() {
        diagnostics.push(unsupported_cli_diagnostic(
            entry_path,
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
            entry_path,
            "CLI command operands beyond supported positional values",
        ));
    }
    if !cli_program.operands.is_empty() {
        diagnostics.push(unsupported_cli_diagnostic(
            entry_path,
            "root-local CLI operands in subcommand mode",
        ));
    }
    if cli_program.exit.is_some() {
        diagnostics.push(unsupported_cli_diagnostic(
            entry_path,
            "CLI exit expressions",
        ));
    }
    if cli_program
        .commands
        .iter()
        .any(command_has_unsupported_options)
    {
        diagnostics.push(unsupported_cli_diagnostic(
            entry_path,
            "CLI command options beyond boolean flags and scalar values",
        ));
    }
    diagnostics.len() == diagnostic_count
}

pub(super) fn cli_operand_consumed_len(
    operands: &[PlannedCliOperand<'_>],
    argumenta: &[String],
) -> usize {
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

pub(super) fn selected_command_record_type_rewrite(
    package: &mut AnalyzedProgram,
    unit_id: &ModuleId,
    command: &CliCommand,
    global_options: &[PlannedCliOption<'_>],
    global_operands: &[PlannedCliOperand<'_>],
) -> Option<CliRecordTypeRewrite> {
    let entry_id = program_entry_id(package)?;
    if &entry_id == unit_id {
        return None;
    }
    let unit = package.nodes.get(unit_id)?;
    let from = command_cli_args_type(unit, command)?;
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
            let name = unit
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
            let name = unit
                .analysis
                .interner
                .resolve(operand.binding_symbol)
                .to_owned();
            (name, operand.ty.clone(), operand.rest, false)
        }))
        .collect::<Vec<_>>();
    let entry = package.nodes.get_mut(&entry_id)?;
    let mut fields = Vec::new();
    for (name, ty, rest, nullable) in field_names {
        let symbol = entry.analysis.interner.intern(&name);
        let mut ty = cli_record_type(&mut entry.analysis.types, &ty, rest);
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

pub(super) fn add_cli_runtime_field_type_rewrites(
    package: &mut AnalyzedProgram,
    unit_id: &ModuleId,
    entry_id: &ModuleId,
    rewrite: &mut CliRecordTypeRewrite,
    fields_by_type: &CliRecordFieldsByType,
) {
    if unit_id == entry_id {
        return;
    }
    // Temporarily move the unit out so its type table can be borrowed
    // immutably alongside the entry's mutable type table; it is restored below
    // and on the not-found early return.
    let Some(source) = package.nodes.remove(unit_id) else {
        return;
    };
    let Some(entry) = package.nodes.get_mut(entry_id) else {
        package.nodes.insert(unit_id.clone(), source);
        return;
    };
    let mut imported = HashMap::new();
    add_runtime_field_type_rewrites(
        &source.analysis.types,
        &mut entry.analysis.types,
        rewrite,
        fields_by_type,
        &mut imported,
    );
    package.nodes.insert(unit_id.clone(), source);
}

pub(super) fn add_runtime_field_type_rewrites(
    source_types: &TypeTable,
    target_types: &mut TypeTable,
    rewrite: &mut CliRecordTypeRewrite,
    fields_by_type: &CliRecordFieldsByType,
    imported: &mut HashMap<TypeId, TypeId>,
) {
    // Importing interns into the target type table: walk the std `HashMap`
    // in type-id order, never in its per-process seeded order.
    let mut locals: Vec<_> = fields_by_type.iter().collect();
    locals.sort_by_key(|(ty, _)| ty.0);
    for (_, fields) in locals {
        for field in fields {
            if let MirRuntimeRecordValue::Array { ty, .. }
            | MirRuntimeRecordValue::Rows { ty, .. } = &field.value
            {
                let source = ty.semantic_id();
                let target = import_semantic_type(source_types, target_types, source, imported);
                push_type_rewrite(&mut rewrite.types, source, target);
            }
        }
    }
}

pub(super) fn push_type_rewrite(
    rewrites: &mut Vec<(TypeId, TypeId)>,
    source: TypeId,
    target: TypeId,
) {
    if source == target || rewrites.iter().any(|(existing, _)| *existing == source) {
        return;
    }
    rewrites.push((source, target));
}

pub(crate) fn import_semantic_type(
    source: &TypeTable,
    target: &mut TypeTable,
    ty: TypeId,
    imported: &mut HashMap<TypeId, TypeId>,
) -> TypeId {
    import_semantic_type_with_nominal(source, target, ty, imported, None)
}

/// Context for mapping source-analysis nominal `DefId`s through the canonical
/// module-qualified nominal-identity key into the entry analysis (codex-gap
/// S1 U2 nominal identity).
///
/// The same nominal referenced from a library analysis (`Type::Struct(lib
/// def)`) and from the consumer analysis (`Type::Struct(consumer def)`) must
/// unify to ONE semantic type in the merged package MIR program. `nominal_map`
/// is the prebuilt source nominal def → unified entry `TypeId` mapping,
/// computed from the source resolver + module identity against the entry's
/// canonical imported-nominal table before lowering mutates the source.
pub(super) struct NominalImportContext<'a> {
    /// Source nominal `DefId` → unified entry `TypeId`.
    pub nominal_map: &'a HashMap<DefId, TypeId>,
    /// Source struct/enum/variant `DefId` → merged dest, so interned types
    /// carry the same `DefId` as remapped aggregates and projections.
    pub def_remap: &'a HashMap<DefId, DefId>,
    /// Source nominal `DefId` → (kind, export name). Intern falls back to
    /// `shared_nominals` so leftover unions unify by name instead of minting
    /// a colliding `Type::Enum(source-local DefId)`.
    pub source_names: &'a HashMap<DefId, (InterfaceNominalKind, String)>,
    /// Previously interned sibling/library nominals keyed by (kind, name).
    pub shared_nominals: &'a HashMap<(InterfaceNominalKind, String), TypeId>,
    /// Source param symbol → merged-interner symbol. Param symbols are
    /// interner-local; without the by-name re-intern a generic size literal
    /// (`Param("2")`) lands as the merged interner's unrelated symbol and
    /// forwarded type args on merged generic calls garble.
    pub param_symbols: &'a HashMap<Symbol, Symbol>,
    /// Source record field-name symbol → merged-interner symbol. Record
    /// field identities live in the type arena (`Type::Record`), not in a
    /// validation metadata map, so the import re-interns them by name —
    /// after package text remapping the merged program's record aggregates
    /// and field projections carry merged-interner symbols, and a record
    /// imported with source-local symbols would fail re-validation with
    /// unknown/missing field diagnostics.
    pub record_field_symbols: &'a HashMap<Symbol, Symbol>,
}

/// Pre-intern every `Type::Param` and `IndexExpr::Param` symbol of a source
/// type table into the merged interner. A `Symbol` always means text in its
/// named interner (ARCH2-2), so the re-intern is unconditional.
pub(super) fn intern_param_symbols(
    source_types: &TypeTable,
    source_interner: &Interner,
    target_interner: &mut Interner,
) -> HashMap<Symbol, Symbol> {
    let mut map = HashMap::new();
    let mut intern_by_name =
        |symbol: Symbol| -> Symbol { target_interner.intern(source_interner.resolve(symbol)) };
    for index in 0..source_types.type_count() {
        let type_index = u32::try_from(index).unwrap_or(u32::MAX);
        if let Type::Param(symbol) = source_types.get(TypeId(type_index)) {
            let remapped = intern_by_name(*symbol);
            map.insert(*symbol, remapped);
        }
    }
    // Shape dims (`tensor<f32, [D, Q]>`) are index params, a separate symbol
    // population from type params; remap both by name.
    for index in 0..source_types.index_count() {
        let index_id = u32::try_from(index).unwrap_or(u32::MAX);
        if let IndexExpr::Param(symbol) =
            source_types.get_index(radix_module::semantic::IndexId(index_id))
        {
            let remapped = intern_by_name(*symbol);
            map.insert(*symbol, remapped);
        }
    }
    map
}

/// Pre-intern every `Type::Record` field-name symbol of a source type table
/// into the merged interner; the import's `Type::Record` arm then re-interns
/// field names through this map so imported records validate against the
/// text-remapped merged program.
pub(super) fn intern_record_field_symbols(
    source_types: &TypeTable,
    source_interner: &Interner,
    target_interner: &mut Interner,
) -> HashMap<Symbol, Symbol> {
    let mut map = HashMap::new();
    for index in 0..source_types.type_count() {
        let type_index = u32::try_from(index).unwrap_or(u32::MAX);
        let Type::Record(fields) = source_types.get(TypeId(type_index)) else {
            continue;
        };
        for name in fields.keys() {
            let remapped = target_interner.intern(source_interner.resolve(*name));
            map.insert(*name, remapped);
        }
    }
    map
}

/// Build the canonical nominal remap for one source unit (sibling or linked
/// library) into the entry analysis.
///
/// Every nominal type id in the source type table is resolved to its canonical
/// key (kind + home-module identity + export name) and looked up in the
/// entry's canonical imported-nominal table. The home-module identity is the
/// nominal's own module identity (mirror of the snapshot side); for the source
/// unit's own exported nominals that is the source module identity.
///
/// Fail-closed (council-11 `correct_before_next_phase)`: an identity-bearing
/// nominal of a module the entry analysis knows must never silently keep its
/// source-local identity — a canonical-table miss for such a nominal returns
/// an unknown-identity error. Modules the entry never imported, identity-less
/// legacy sources, and private source-local nominals keep the legacy
/// source-local fallback (the def is interned as-is).
pub(super) fn build_nominal_remap(
    source_types: &TypeTable,
    source_interner: &Interner,
    source_resolver: &Resolver,
    source_identity: Option<&InterfaceLibraryIdentity>,
    source_exports: &BTreeSet<String>,
    entry: &AnalyzedModule,
    shared_nominals: &HashMap<(InterfaceNominalKind, String), TypeId>,
) -> Result<HashMap<DefId, TypeId>, String> {
    let mut map = HashMap::new();
    let mut target_interner = entry.interner.clone();
    for index in 0..source_types.type_count() {
        let type_index = u32::try_from(index).unwrap_or(u32::MAX);
        let kind = match source_types.get(TypeId(type_index)) {
            Type::Struct(def_id) => Some((*def_id, InterfaceNominalKind::Struct)),
            Type::Enum(def_id) => Some((*def_id, InterfaceNominalKind::Enum)),
            Type::Interface(def_id) => Some((*def_id, InterfaceNominalKind::Interface)),
            _ => None,
        };
        let Some((def_id, kind)) = kind else {
            continue;
        };
        if map.contains_key(&def_id) {
            continue;
        }
        let Some(symbol) = source_resolver.get_symbol(def_id) else {
            continue;
        };
        let name_text = source_interner.resolve(symbol.name);
        let name = target_interner.intern(name_text);
        let home_identity = source_resolver.nominal_module_identity(def_id).or_else(|| {
            source_exports
                .contains(name_text)
                .then(|| source_identity.cloned())
                .flatten()
        });
        if let Some(entry_ty) = entry
            .resolver
            .imported_nominal_type_by_identity(kind, home_identity.as_ref(), name)
            .or_else(|| fallback_entry_nominal_type(entry, kind, name, name_text))
            .or_else(|| shared_nominals.get(&(kind, name_text.to_owned())).copied())
        {
            map.insert(def_id, entry_ty);
        } else if home_identity
            .as_ref()
            .is_some_and(|identity| entry.resolver.canonical_identity_known(identity))
        {
            return Err(format!(
                "identity-bearing nominal `{name_text}` (kind {kind:?}) from module `{}` is not known to the entry analysis; it must not retain a source-local identity",
                home_identity
                    .as_ref()
                    .map(InterfaceLibraryIdentity::display_key)
                    .unwrap_or_default(),
            ));
        }
    }
    Ok(map)
}

/// Build the canonical enum-variant remap for one source unit into the entry
/// analysis (codex-gap S1 U2 VALUE members): source variant `DefId` → consumer
/// variant `DefId`.
///
/// Each source variant resolves through its parent enum (source resolver
/// `variant_parents`) to the canonical enum key (Enum + home-module identity +
/// enum export name), which maps to the consumer enum `DefId`; the consumer
/// variant then resolves by name through `variant_child`.
///
/// Fail-closed (council-11 `correct_before_next_phase)`: an identity-bearing
/// variant of an enum whose module the entry analysis knows must never
/// silently retain its source-local identity — an unresolved consumer variant
/// returns an unknown-identity error. Enums of modules the entry never
/// imported, identity-less legacy sources, and private enums keep their
/// source-local defs, as before.
pub(super) fn build_variant_remap(
    source_interner: &Interner,
    source_resolver: &Resolver,
    source_identity: Option<&InterfaceLibraryIdentity>,
    source_exports: &BTreeSet<String>,
    entry: &AnalyzedModule,
    shared_nominals: &HashMap<(InterfaceNominalKind, String), TypeId>,
) -> Result<HashMap<DefId, DefId>, String> {
    let mut map = HashMap::new();
    let mut target_interner = entry.interner.clone();
    for (variant_def, enum_def) in source_resolver.variant_parents_all() {
        let Some(variant_symbol) = source_resolver.get_symbol(variant_def) else {
            continue;
        };
        let Some(enum_symbol) = source_resolver.get_symbol(enum_def) else {
            continue;
        };
        let enum_name_text = source_interner.resolve(enum_symbol.name);
        let variant_name_text = source_interner.resolve(variant_symbol.name);
        let enum_name = target_interner.intern(enum_name_text);
        let enum_home = source_resolver
            .nominal_module_identity(enum_def)
            .or_else(|| {
                source_exports
                    .contains(enum_name_text)
                    .then(|| source_identity.cloned())
                    .flatten()
            });
        let Some(entry_enum_def) = entry
            .resolver
            .imported_nominal_def_id(InterfaceNominalKind::Enum, enum_home.as_ref(), enum_name)
            .or_else(|| {
                fallback_entry_nominal_type(
                    entry,
                    InterfaceNominalKind::Enum,
                    enum_name,
                    enum_name_text,
                )
                .or_else(|| {
                    shared_nominals
                        .get(&(InterfaceNominalKind::Enum, enum_name_text.to_owned()))
                        .copied()
                })
                .and_then(|ty| match entry.types.get(ty) {
                    Type::Enum(def) => Some(*def),
                    _ => None,
                })
            })
        else {
            if enum_home
                .as_ref()
                .is_some_and(|identity| entry.resolver.canonical_identity_known(identity))
            {
                return Err(format!(
                    "identity-bearing enum `{enum_name_text}` (module `{}`) is not known to the entry analysis; variant `{variant_name_text}` must not retain a source-local identity",
                    enum_home
                        .as_ref()
                        .map(InterfaceLibraryIdentity::display_key)
                        .unwrap_or_default(),
                ));
            }
            continue;
        };
        let variant_name = target_interner.intern(variant_name_text);
        if let Some(entry_variant_def) = entry.resolver.variant_child(entry_enum_def, variant_name)
        {
            map.insert(variant_def, entry_variant_def);
        } else if enum_home
            .as_ref()
            .is_some_and(|identity| entry.resolver.canonical_identity_known(identity))
        {
            return Err(format!(
                "identity-bearing enum `{enum_name_text}` variant `{variant_name_text}` is not resolvable in the consumer analysis; it must not retain a source-local identity",
            ));
        }
    }
    Ok(map)
}

/// Nested sibling/library nominals (gradus `dtype.DType` on `tensor.Tensor`)
/// often live on the consumer only as a field/error type, not as a directly
/// imported module identity. Fall back to the unique name-keyed imported
/// nominal, then to a unique same-kind type-table entry with that name.
fn fallback_entry_nominal_type(
    entry: &AnalyzedModule,
    kind: InterfaceNominalKind,
    name: Symbol,
    name_text: &str,
) -> Option<TypeId> {
    if !entry
        .resolver
        .imported_nominal_type_is_ambiguous(kind, name)
        && let Some(ty) = entry.resolver.imported_nominal_type(kind, name)
    {
        return Some(ty);
    }
    let expected = match kind {
        InterfaceNominalKind::Struct => SymbolKind::Struct,
        InterfaceNominalKind::Enum => SymbolKind::Enum,
        InterfaceNominalKind::Interface => SymbolKind::Interface,
    };
    let mut found = None;
    for index in 0..entry.types.type_count() {
        let ty = TypeId(u32::try_from(index).unwrap_or(u32::MAX));
        let def_id = match (kind, entry.types.get(ty)) {
            (InterfaceNominalKind::Struct, Type::Struct(def_id))
            | (InterfaceNominalKind::Enum, Type::Enum(def_id))
            | (InterfaceNominalKind::Interface, Type::Interface(def_id)) => *def_id,
            _ => continue,
        };
        let Some(symbol) = entry.resolver.get_symbol(def_id) else {
            continue;
        };
        if symbol.kind != expected {
            continue;
        }
        if entry.interner.resolve(symbol.name) != name_text {
            continue;
        }
        if found.is_some_and(|existing| existing != ty) {
            return None;
        }
        found = Some(ty);
    }
    found
}

/// Remember interned sibling/library nominals by (kind, name) so a later unit
/// that imports the same nested type (self-provider `dtype.DType` from
/// `tensor.fab`) unifies onto the first intern instead of minting a second
/// `Enum(DefId)`.
pub(super) fn record_shared_nominals(
    source_names: &HashMap<DefId, (InterfaceNominalKind, String)>,
    source_types: &TypeTable,
    type_rewrites: &[(TypeId, TypeId)],
    shared: &mut HashMap<(InterfaceNominalKind, String), TypeId>,
) {
    for index in 0..source_types.type_count() {
        let source_ty = TypeId(u32::try_from(index).unwrap_or(u32::MAX));
        let def_id = match source_types.get(source_ty) {
            Type::Struct(def_id) | Type::Enum(def_id) | Type::Interface(def_id) => *def_id,
            _ => continue,
        };
        let Some((kind, name)) = source_names.get(&def_id) else {
            continue;
        };
        let Some((_, entry_ty)) = type_rewrites.iter().find(|(from, _)| *from == source_ty) else {
            continue;
        };
        shared.entry((*kind, name.clone())).or_insert(*entry_ty);
    }
}

pub(super) fn source_nominal_names(
    source_types: &TypeTable,
    source_interner: &Interner,
    source_resolver: &Resolver,
) -> HashMap<DefId, (InterfaceNominalKind, String)> {
    let mut names = HashMap::new();
    for index in 0..source_types.type_count() {
        let ty = TypeId(u32::try_from(index).unwrap_or(u32::MAX));
        let (kind, def_id) = match source_types.get(ty) {
            Type::Struct(def_id) => (InterfaceNominalKind::Struct, *def_id),
            Type::Enum(def_id) => (InterfaceNominalKind::Enum, *def_id),
            Type::Interface(def_id) => (InterfaceNominalKind::Interface, *def_id),
            _ => continue,
        };
        let Some(symbol) = source_resolver.get_symbol(def_id) else {
            continue;
        };
        names
            .entry(def_id)
            .or_insert_with(|| (kind, source_interner.resolve(symbol.name).to_owned()));
    }
    names
}

pub(super) fn import_semantic_type_with_nominal(
    source: &TypeTable,
    target: &mut TypeTable,
    ty: TypeId,
    imported: &mut HashMap<TypeId, TypeId>,
    nominal: Option<&NominalImportContext<'_>>,
) -> TypeId {
    if let Some(existing) = imported.get(&ty).copied() {
        return existing;
    }
    let imported_ty = match source.get(ty).clone() {
        Type::Primitive(primitive) => target.primitive(primitive),
        Type::TensorEdgePolicy(policy) => target.tensor_edge_policy(policy),
        Type::Array(inner) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            target.ensure_array(inner)
        }
        Type::BoundedArray { element, capacity } => {
            let element =
                import_semantic_type_with_nominal(source, target, element, imported, nominal);
            let capacity = import_index_expr_with_nominal(source, target, capacity, nominal);
            target.bounded_array(element, capacity)
        }
        Type::RangedArray { element, min, max } => {
            let element =
                import_semantic_type_with_nominal(source, target, element, imported, nominal);
            target.ranged_array(element, min, max)
        }
        Type::BoundedQueue { element, capacity } => {
            let element =
                import_semantic_type_with_nominal(source, target, element, imported, nominal);
            let capacity = import_index_expr_with_nominal(source, target, capacity, nominal);
            target.bounded_queue(element, capacity)
        }
        Type::BoundedStack { element, capacity } => {
            let element =
                import_semantic_type_with_nominal(source, target, element, imported, nominal);
            let capacity = import_index_expr_with_nominal(source, target, capacity, nominal);
            target.bounded_stack(element, capacity)
        }
        Type::BoundedTextus { capacity } => {
            let capacity = import_index_expr_with_nominal(source, target, capacity, nominal);
            target.bounded_textus(capacity)
        }
        Type::BoundedAscii { capacity } => {
            let capacity = import_index_expr_with_nominal(source, target, capacity, nominal);
            target.bounded_ascii(capacity)
        }
        Type::BoundedOcteti { capacity } => {
            let capacity = import_index_expr_with_nominal(source, target, capacity, nominal);
            target.bounded_octeti(capacity)
        }
        Type::Map(key, value) => {
            let key = import_semantic_type_with_nominal(source, target, key, imported, nominal);
            let value = import_semantic_type_with_nominal(source, target, value, imported, nominal);
            target.map(key, value)
        }
        Type::Ledger(key, value) => {
            let key = import_semantic_type_with_nominal(source, target, key, imported, nominal);
            let value = import_semantic_type_with_nominal(source, target, value, imported, nominal);
            target.ledger(key, value)
        }
        Type::Record(fields) => {
            let fields = fields
                .into_iter()
                .map(|(name, field_ty)| {
                    // Record field names are interner-local: re-intern through
                    // the prebuilt symbol map so the imported record's field
                    // identities match the text-remapped merged program (the
                    // `param_symbols` law, applied to record fields).
                    let name = nominal
                        .and_then(|context| context.record_field_symbols.get(&name).copied())
                        .unwrap_or(name);
                    (
                        name,
                        import_semantic_type_with_nominal(
                            source, target, field_ty, imported, nominal,
                        ),
                    )
                })
                .collect();
            target.intern(Type::Record(fields))
        }
        Type::Set(inner) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            target.set(inner)
        }
        Type::Promissum(inner) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            target.promissum(inner)
        }
        Type::PromissumFailable(success, alternate) => {
            let success =
                import_semantic_type_with_nominal(source, target, success, imported, nominal);
            let alternate =
                import_semantic_type_with_nominal(source, target, alternate, imported, nominal);
            target.promissum_failable(success, alternate)
        }
        Type::Cursor(inner) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            target.cursor(inner)
        }
        Type::Queue(inner) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            target.queue(inner)
        }
        Type::Stack(inner) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            target.stack(inner)
        }
        Type::AsyncCursor(item, alternate) => {
            let item = import_semantic_type_with_nominal(source, target, item, imported, nominal);
            let alternate =
                import_semantic_type_with_nominal(source, target, alternate, imported, nominal);
            target.async_cursor(item, alternate)
        }
        Type::Tensor(inner, shape) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            let shape = import_index_expr_with_nominal(source, target, shape, nominal);
            target.tensor_with_shape(inner, shape)
        }
        Type::Vector(inner, width) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            let width = import_index_expr_with_nominal(source, target, width, nominal);
            target.vector_with_width(inner, width)
        }
        Type::Matrix(inner, shape) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            let shape = import_index_expr_with_nominal(source, target, shape, nominal);
            target.matrix_with_shape(inner, shape)
        }
        Type::Sparsa(inner, shape) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            let shape = import_index_expr_with_nominal(source, target, shape, nominal);
            target.sparsa_with_shape(inner, shape)
        }
        Type::Atomic(inner) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            target.atomic(inner)
        }
        Type::Intervallum(inner, _) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            target.intervallum(inner)
        }
        Type::SizedNumeric(primitive, width) => target.sized_numeric(primitive, width),
        Type::ModularWord(width) => target.intern(Type::ModularWord(width)),
        Type::RangedInt {
            policy,
            width,
            lo,
            hi,
        } => target.ranged_int(policy, width, lo, hi),
        Type::Bits(width) => target.bits(width),
        Type::SaturatingWord(width) => target.intern(Type::SaturatingWord(width)),
        Type::PolicyFloat(policy, width) => target.intern(Type::PolicyFloat(policy, width)),
        Type::SizedInstans(precision) => target.sized_instans(precision),
        Type::Option(inner) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            target.option(inner)
        }
        Type::Ref(mutability, inner) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            target.reference(mutability, inner)
        }
        Type::Alias(def_id, inner) => {
            let inner = import_semantic_type_with_nominal(source, target, inner, imported, nominal);
            target.intern(Type::Alias(def_id, inner))
        }
        Type::Func(mut sig) => {
            for param in &mut sig.params {
                param.ty =
                    import_semantic_type_with_nominal(source, target, param.ty, imported, nominal);
            }
            sig.ret = import_semantic_type_with_nominal(source, target, sig.ret, imported, nominal);
            if let Some(error_ty) = sig.err {
                sig.err = Some(import_semantic_type_with_nominal(
                    source, target, error_ty, imported, nominal,
                ));
            }
            target.function(sig)
        }
        Type::Applied(base, args) => {
            let base = import_semantic_type_with_nominal(source, target, base, imported, nominal);
            let args = args
                .into_iter()
                .map(|arg| match arg {
                    radix_types::AppliedArg::Type(id) => radix_types::AppliedArg::Type(
                        import_semantic_type_with_nominal(source, target, id, imported, nominal),
                    ),
                    radix_types::AppliedArg::Size(id) => radix_types::AppliedArg::Size(
                        import_index_expr_with_nominal(source, target, id, nominal),
                    ),
                })
                .collect();
            target.intern(Type::Applied(base, args))
        }
        Type::Union(members) => {
            let members = members
                .into_iter()
                .map(|member| {
                    import_semantic_type_with_nominal(source, target, member, imported, nominal)
                })
                .collect();
            target.intern(Type::Union(members))
        }
        Type::Intersection(members) => {
            let members = members
                .into_iter()
                .map(|member| {
                    import_semantic_type_with_nominal(source, target, member, imported, nominal)
                })
                .collect();
            target.intern(Type::Intersection(members))
        }
        Type::Tuple(members) => {
            let members = members
                .into_iter()
                .map(|member| {
                    import_semantic_type_with_nominal(source, target, member, imported, nominal)
                })
                .collect();
            target.tuple(members)
        }
        other @ (Type::Struct(_) | Type::Enum(_) | Type::Interface(_)) => {
            import_nominal_by_canonical_key(&other, nominal).unwrap_or_else(|| {
                let remap = |def: DefId| {
                    nominal
                        .and_then(|ctx| ctx.def_remap.get(&def).copied())
                        .unwrap_or(def)
                };
                match other {
                    Type::Struct(def) => target.intern(Type::Struct(remap(def))),
                    Type::Enum(def) => target.intern(Type::Enum(remap(def))),
                    Type::Interface(def) => target.intern(Type::Interface(remap(def))),
                    // The outer pattern admits only Struct/Enum/Interface.
                    other => target.intern(other),
                }
            })
        }
        // Param symbols are interner-local: a generic size literal interned
        // as `Param("2")` in the source unit would land as the merged
        // interner's unrelated symbol 2 without a by-name re-intern, garbling
        // forwarded type args on merged generic calls.
        Type::Param(symbol) => {
            let remapped = nominal
                .and_then(|ctx| ctx.param_symbols.get(&symbol).copied())
                .unwrap_or(symbol);
            target.intern(Type::Param(remapped))
        }
        other @ (Type::Infer(_) | Type::InferUnion(_) | Type::MarkerHole { .. } | Type::Error) => {
            target.intern(other)
        }
    };
    imported.insert(ty, imported_ty);
    imported_ty
}

/// Map one source-analysis nominal through the canonical nominal-identity
/// remap into the entry type table (codex-gap S1 U2).
///
/// Returns `None` when the source def has no canonical entry mapping — the
/// caller falls back to interning through `def_remap` so leftover unions
/// and structs do not keep colliding source-local `DefId`s.
fn import_nominal_by_canonical_key(
    ty: &Type,
    nominal: Option<&NominalImportContext<'_>>,
) -> Option<TypeId> {
    let ctx = nominal?;
    let def_id = match ty {
        Type::Struct(def_id) | Type::Enum(def_id) | Type::Interface(def_id) => *def_id,
        _ => return None,
    };
    if let Some(mapped) = ctx.nominal_map.get(&def_id).copied() {
        return Some(mapped);
    }
    let (kind, name) = ctx.source_names.get(&def_id)?;
    ctx.shared_nominals.get(&(*kind, name.clone())).copied()
}

pub(crate) fn import_index_expr(
    source: &TypeTable,
    target: &mut TypeTable,
    index: radix_module::semantic::IndexId,
) -> radix_module::semantic::IndexId {
    import_index_expr_with_nominal(source, target, index, None)
}

/// Index-param symbols are interner-local exactly like [`Type::Param`]
/// symbols: a shape `tensor<f32, [D, Q]>` imported with the source unit's
/// raw symbol ids aliases unrelated merged strings (garbage dims after
/// instantiation). Re-intern `Param` dims by name through the import
/// context's precomputed map.
pub(super) fn import_index_expr_with_nominal(
    source: &TypeTable,
    target: &mut TypeTable,
    index: radix_module::semantic::IndexId,
    nominal: Option<&NominalImportContext<'_>>,
) -> radix_module::semantic::IndexId {
    match source.get_index(index).clone() {
        IndexExpr::Tuple(items) => {
            let items = items
                .into_iter()
                .map(|item| import_index_expr_with_nominal(source, target, item, nominal))
                .collect();
            target.intern_index(IndexExpr::Tuple(items))
        }
        IndexExpr::Param(symbol) => {
            let remapped = nominal
                .and_then(|ctx| ctx.param_symbols.get(&symbol).copied())
                .unwrap_or(symbol);
            target.intern_index(IndexExpr::Param(remapped))
        }
        other => target.intern_index(other),
    }
}

/// Record type of the root `incipit argumenta` local, read from the HIR entry
/// block's runtime-provided args local — the same identity the entry record
/// aggregate lowers with.
pub(super) fn entry_cli_args_type(unit: &AnalyzedProgramNode) -> Option<TypeId> {
    let name = unit.analysis.hir.entry_args_name?;
    let entry = unit.analysis.hir.entry.as_ref()?;
    entry.statements.iter().find_map(|statement| {
        let HirStatementKind::Local(local) = &statement.kind else {
            return None;
        };
        (local.name == name && local.runtime_provided)
            .then_some(local.ty)
            .flatten()
    })
}

pub(super) fn command_cli_args_type(
    unit: &AnalyzedProgramNode,
    command: &CliCommand,
) -> Option<TypeId> {
    unit.analysis.hir.items.iter().find_map(|item| {
        let HirItemKind::Function(function) = &item.kind else {
            return None;
        };
        if function.name != command.function_symbol {
            return None;
        }
        function.cli_args.as_ref().map(|param| param.ty)
    })
}
