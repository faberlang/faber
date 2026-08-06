//! Plan package CLI dispatch: root entry, argument parsing, and subcommands.

use super::*;

pub(super) fn plan_cli_package(
    package: &mut AnalyzedPackage,
    argumenta: &[String],
    mode: CliPlanningMode,
) -> Result<CliPackagePlan, Vec<Diagnostic>> {
    let mut plan = CliPackagePlan::default();
    let mut diagnostics = Vec::new();
    let Some(entry_index) = package.units.iter().position(|unit| unit.is_entry) else {
        return Ok(plan);
    };
    let Some(cli_program) = package.units[entry_index].analysis.cli_program.clone() else {
        return Ok(plan);
    };
    plan.uses_cli_runtime = true;

    match cli_program.mode {
        CliMode::SingleCommand => {
            if mode == CliPlanningMode::FmirTextRuntime {
                if let Some((records, cli_section, exit_code)) =
                    plan_fmir_text_runtime_cli_root_entry(
                        &mut package.units[entry_index],
                        &cli_program,
                        &mut diagnostics,
                    )
                {
                    plan.exit_code = exit_code;
                    plan.entry_records
                        .insert(package.units[entry_index].path.clone(), records);
                    plan.fmir_text_cli = Some(cli_section);
                }
            } else if let Some((records, exit_code)) = plan_cli_root_entry(
                &mut package.units[entry_index],
                &cli_program,
                argumenta,
                &mut diagnostics,
            ) {
                plan.exit_code = exit_code;
                plan.entry_records
                    .insert(package.units[entry_index].path.clone(), records);
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
            plan.dispatch = plan_cli_subcommand(
                package,
                &cli_program,
                argumenta,
                &mut plan.entry_records,
                &mut diagnostics,
            );
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
    unit: &mut AnalyzedPackageUnit,
    cli_program: &CliProgram,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(CliRecordFieldsByLocal, FmirTextCliSection, Option<i32>)> {
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
    let exit_code = package_mir_cli_exit_code(&cli_program.exit, None, &[], unit, diagnostics)?;
    let operands = cli_program
        .global_operands
        .iter()
        .chain(&cli_program.operands)
        .collect::<Vec<_>>();
    if operands.iter().any(|operand| {
        operand.rest
            || operand.default.is_some()
            || matches!(
                operand.ty,
                CliType::Octeti | CliType::ListaTextus | CliType::ListaNumerus
            )
    }) {
        diagnostics.push(unsupported_cli_diagnostic(
            &unit.path,
            "fmir-text runtime CLI operands beyond required scalar values",
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
        let ty = fmir_text_cli_value_type(&operand.operand.ty)?;
        fields.push(MirRuntimeRecordField {
            name: unit.analysis.interner.intern(&operand.binding_name),
            value: MirRuntimeRecordValue::Operand(fmir_text_cli_placeholder_value(unit, &ty)),
        });
        image_operands.push(FmirTextCliOperand {
            field: operand.binding_name,
            ty,
        });
    }
    let record = unit.analysis.interner.resolve(args_name).to_owned();
    Some((
        HashMap::from([(args_name, fields)]),
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
        CliType::Octeti | CliType::ListaTextus | CliType::ListaNumerus => None,
    }
}

pub(super) fn fmir_text_cli_placeholder_value(
    unit: &mut AnalyzedPackageUnit,
    ty: &FmirTextCliValueType,
) -> MirOperand {
    let constant = match ty {
        FmirTextCliValueType::Textus => {
            MirConstant::String(unit.analysis.interner.intern("__fmir_runtime_arg__"))
        }
        FmirTextCliValueType::Numerus => MirConstant::Int(0),
        FmirTextCliValueType::Fractus => MirConstant::Float(0.0),
        FmirTextCliValueType::Bivalens => MirConstant::Bool(false),
    };
    MirOperand::Constant(constant)
}

pub(super) fn plan_cli_root_entry(
    unit: &mut AnalyzedPackageUnit,
    cli_program: &CliProgram,
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(CliRecordFieldsByLocal, Option<i32>)> {
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
    let operands = cli_program
        .global_operands
        .iter()
        .chain(&cli_program.operands)
        .collect::<Vec<_>>();
    if has_unsupported_package_mir_operands(operands.iter().copied()) {
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
    let operands = planned_cli_operands(
        &unit.analysis.interner,
        cli_program
            .global_operands
            .iter()
            .chain(&cli_program.operands),
    );
    let mut fields = parsed.option_fields;
    fields.extend(cli_operand_record_fields(
        unit,
        &operands,
        &parsed.positionals,
        diagnostics,
    )?);
    let Some(args_name) = unit.analysis.hir.entry_args_name else {
        if !fields.is_empty() {
            diagnostics.push(unsupported_cli_diagnostic(
                &unit.path,
                "CLI argument records",
            ));
            return None;
        }
        let exit_code = package_mir_cli_exit_code(&cli_program.exit, None, &[], unit, diagnostics)?;
        return Some((HashMap::new(), exit_code));
    };
    let exit_code = package_mir_cli_exit_code(
        &cli_program.exit,
        Some(args_name),
        &fields,
        unit,
        diagnostics,
    )?;
    Some((HashMap::from([(args_name, fields)]), exit_code))
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
    unit: &mut AnalyzedPackageUnit,
    options: &[PlannedCliOption<'_>],
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ParsedCliArguments> {
    parse_cli_arguments_with_mode(unit, options, argumenta, diagnostics, false)
}

pub(super) fn parse_leading_cli_options(
    unit: &mut AnalyzedPackageUnit,
    options: &[PlannedCliOption<'_>],
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ParsedCliArguments> {
    parse_cli_arguments_with_mode(unit, options, argumenta, diagnostics, true)
}

pub(super) fn parse_cli_arguments_with_mode(
    unit: &mut AnalyzedPackageUnit,
    options: &[PlannedCliOption<'_>],
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
    stop_at_first_positional: bool,
) -> Option<ParsedCliArguments> {
    if options.is_empty() {
        return Some(ParsedCliArguments {
            option_fields: Vec::new(),
            positionals: argumenta.to_vec(),
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
            let raw = match inline_value {
                Some(value) => value,
                None => {
                    argument_index += 1;
                    let Some(value) = argumenta.get(argument_index) else {
                        push_cli_option_missing_value_diagnostic(unit, argument, diagnostics);
                        return None;
                    };
                    value.clone()
                }
            };
            option_fields[option_index].value =
                MirRuntimeRecordValue::Operand(cli_option_value(unit, option, &raw, diagnostics)?);
            argument_index += 1;
            continue;
        }
        if let Some(name) = argument.strip_prefix('-') {
            if !name.is_empty() {
                let Some(option_index) = options
                    .iter()
                    .position(|option| option.option.short.as_deref() == Some(name))
                else {
                    push_cli_option_match_diagnostic(unit, argument, diagnostics);
                    return None;
                };
                let option = &options[option_index];
                if option.option.flag {
                    option_fields[option_index].value = MirRuntimeRecordValue::Operand(
                        MirOperand::Constant(MirConstant::Bool(true)),
                    );
                    argument_index += 1;
                    continue;
                }
                argument_index += 1;
                let Some(raw) = argumenta.get(argument_index) else {
                    push_cli_option_missing_value_diagnostic(unit, argument, diagnostics);
                    return None;
                };
                option_fields[option_index].value = MirRuntimeRecordValue::Operand(
                    cli_option_value(unit, option, raw, diagnostics)?,
                );
                argument_index += 1;
                continue;
            }
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
    unit: &mut AnalyzedPackageUnit,
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
    unit: &AnalyzedPackageUnit,
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
    unit: &AnalyzedPackageUnit,
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
    package: &mut AnalyzedPackage,
    cli_program: &CliProgram,
    argumenta: &[String],
    entry_records: &mut CliEntryRecords,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CliDispatchPlan> {
    let entry_path = package.spec.entry.clone();
    let diagnostic_count = diagnostics.len();
    if cli_program
        .global_options
        .iter()
        .any(|option| !is_package_mir_supported_option(option))
    {
        diagnostics.push(unsupported_cli_diagnostic(
            &entry_path,
            "CLI global options beyond boolean flags and scalar values",
        ));
    }
    if !cli_program.options.is_empty() {
        diagnostics.push(unsupported_cli_diagnostic(
            &entry_path,
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
            &entry_path,
            "CLI command operands beyond supported positional values",
        ));
    }
    if !cli_program.operands.is_empty() {
        diagnostics.push(unsupported_cli_diagnostic(
            &entry_path,
            "root-local CLI operands in subcommand mode",
        ));
    }
    if cli_program.exit.is_some() {
        diagnostics.push(unsupported_cli_diagnostic(
            &entry_path,
            "CLI exit expressions",
        ));
    }
    if cli_program
        .commands
        .iter()
        .any(command_has_unsupported_options)
    {
        diagnostics.push(unsupported_cli_diagnostic(
            &entry_path,
            "CLI command options beyond boolean flags and scalar values",
        ));
    }
    if diagnostics.len() != diagnostic_count {
        return None;
    }

    let entry_index = package.units.iter().position(|unit| unit.is_entry)?;
    let global_options = planned_cli_options(
        &package.units[entry_index].analysis.interner,
        cli_program.global_options.iter(),
    );
    let global_operands = planned_cli_operands(
        &package.units[entry_index].analysis.interner,
        cli_program.global_operands.iter(),
    );
    let parsed_globals = parse_leading_cli_options(
        &mut package.units[entry_index],
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
    if let Some(args_name) = package.units[entry_index].analysis.hir.entry_args_name {
        let mut entry_fields = parsed_globals.option_fields.clone();
        let global_operand_args_len = cli_operand_consumed_len(&global_operands, command_args);
        entry_fields.extend(cli_operand_record_fields(
            &mut package.units[entry_index],
            &global_operands,
            &command_args[..global_operand_args_len],
            diagnostics,
        )?);
        if !entry_fields.is_empty() {
            entry_records
                .entry(package.units[entry_index].path.clone())
                .or_default()
                .insert(args_name, entry_fields);
        }
    }
    let command = command_match.command;
    let Some(unit_index) = command_unit_index(package, command) else {
        diagnostics.push(
            crate::package_diagnostic_error(format!(
                "package MIR could not resolve CLI command module for `{}`",
                command.path.join(" ")
            ))
            .with_file(entry_path.display().to_string()),
        );
        return None;
    };
    let target_command = command_in_unit(&package.units[unit_index], command)
        .cloned()
        .unwrap_or_else(|| command.clone());
    let mut record_type_rewrite = selected_command_record_type_rewrite(
        package,
        unit_index,
        &target_command,
        &global_options,
        &global_operands,
    );
    let unit = &mut package.units[unit_index];
    let unit_path = unit.path.clone();
    let global_fields = if unit_index == entry_index {
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
        add_cli_runtime_field_type_rewrites(package, unit_index, entry_index, rewrite, fields);
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
    package: &mut AnalyzedPackage,
    unit_index: usize,
    command: &CliCommand,
    global_options: &[PlannedCliOption<'_>],
    global_operands: &[PlannedCliOperand<'_>],
) -> Option<CliRecordTypeRewrite> {
    let entry_index = package.units.iter().position(|unit| unit.is_entry)?;
    if entry_index == unit_index {
        return None;
    }
    let from = command_cli_args_type(&package.units[unit_index], command)?;
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
            let name = package.units[unit_index]
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
            let name = package.units[unit_index]
                .analysis
                .interner
                .resolve(operand.binding_symbol)
                .to_owned();
            (name, operand.ty.clone(), operand.rest, false)
        }))
        .collect::<Vec<_>>();
    let entry = &mut package.units[entry_index];
    let mut fields = Vec::new();
    for (name, ty, rest, nullable) in field_names {
        let symbol = entry.analysis.interner.intern(&name);
        let mut ty = cli_record_type(&mut entry.analysis.types, &ty, rest)?;
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
    package: &mut AnalyzedPackage,
    unit_index: usize,
    entry_index: usize,
    rewrite: &mut CliRecordTypeRewrite,
    fields_by_local: &CliRecordFieldsByLocal,
) {
    if unit_index == entry_index {
        return;
    }
    let mut imported = HashMap::new();
    if unit_index < entry_index {
        let (before_entry, entry_and_after) = package.units.split_at_mut(entry_index);
        let source_types = &before_entry[unit_index].analysis.types;
        let target_types = &mut entry_and_after[0].analysis.types;
        add_runtime_field_type_rewrites(
            source_types,
            target_types,
            rewrite,
            fields_by_local,
            &mut imported,
        );
    } else {
        let (before_source, source_and_after) = package.units.split_at_mut(unit_index);
        let target_types = &mut before_source[entry_index].analysis.types;
        let source_types = &source_and_after[0].analysis.types;
        add_runtime_field_type_rewrites(
            source_types,
            target_types,
            rewrite,
            fields_by_local,
            &mut imported,
        );
    }
}

pub(super) fn add_runtime_field_type_rewrites(
    source_types: &TypeTable,
    target_types: &mut TypeTable,
    rewrite: &mut CliRecordTypeRewrite,
    fields_by_local: &CliRecordFieldsByLocal,
    imported: &mut HashMap<TypeId, TypeId>,
) {
    for fields in fields_by_local.values() {
        for field in fields {
            if let MirRuntimeRecordValue::Array { ty, .. } = &field.value {
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

pub(super) fn import_semantic_type(
    source: &TypeTable,
    target: &mut TypeTable,
    ty: TypeId,
    imported: &mut HashMap<TypeId, TypeId>,
) -> TypeId {
    if let Some(existing) = imported.get(&ty).copied() {
        return existing;
    }
    let imported_ty = match source.get(ty).clone() {
        Type::Primitive(primitive) => target.primitive(primitive),
        Type::Array(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target
                .find_array(inner)
                .unwrap_or_else(|| target.array(inner))
        }
        Type::Map(key, value) => {
            let key = import_semantic_type(source, target, key, imported);
            let value = import_semantic_type(source, target, value, imported);
            target.map(key, value)
        }
        Type::Record(fields) => {
            let fields = fields
                .into_iter()
                .map(|(name, field_ty)| {
                    (
                        name,
                        import_semantic_type(source, target, field_ty, imported),
                    )
                })
                .collect();
            target.intern(Type::Record(fields))
        }
        Type::Set(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.set(inner)
        }
        Type::Promissum(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.promissum(inner)
        }
        Type::PromissumFailable(success, alternate) => {
            let success = import_semantic_type(source, target, success, imported);
            let alternate = import_semantic_type(source, target, alternate, imported);
            target.promissum_failable(success, alternate)
        }
        Type::Cursor(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.cursor(inner)
        }
        Type::AsyncCursor(item, alternate) => {
            let item = import_semantic_type(source, target, item, imported);
            let alternate = import_semantic_type(source, target, alternate, imported);
            target.async_cursor(item, alternate)
        }
        Type::Tensor(inner, shape) => {
            let inner = import_semantic_type(source, target, inner, imported);
            let shape = import_index_expr(source, target, shape);
            target.tensor_with_shape(inner, shape)
        }
        Type::Vector(inner, width) => {
            let inner = import_semantic_type(source, target, inner, imported);
            let width = import_index_expr(source, target, width);
            target.vector_with_width(inner, width)
        }
        Type::Matrix(inner, shape) => {
            let inner = import_semantic_type(source, target, inner, imported);
            let shape = import_index_expr(source, target, shape);
            target.matrix_with_shape(inner, shape)
        }
        Type::Sparsa(inner, shape) => {
            let inner = import_semantic_type(source, target, inner, imported);
            let shape = import_index_expr(source, target, shape);
            target.sparsa_with_shape(inner, shape)
        }
        Type::Atomic(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.atomic(inner)
        }
        Type::Intervallum(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.intern(Type::Intervallum(inner))
        }
        Type::SizedNumeric(primitive, width) => target.sized_numeric(primitive, width),
        Type::ModularWord(width) => target.intern(Type::ModularWord(width)),
        Type::SizedInstans(precision) => target.intern(Type::SizedInstans(precision)),
        Type::Option(inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.option(inner)
        }
        Type::Ref(mutability, inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.reference(mutability, inner)
        }
        Type::Alias(def_id, inner) => {
            let inner = import_semantic_type(source, target, inner, imported);
            target.intern(Type::Alias(def_id, inner))
        }
        Type::Func(mut sig) => {
            for param in &mut sig.params {
                param.ty = import_semantic_type(source, target, param.ty, imported);
            }
            sig.ret = import_semantic_type(source, target, sig.ret, imported);
            if let Some(error_ty) = sig.err {
                sig.err = Some(import_semantic_type(source, target, error_ty, imported));
            }
            target.function(sig)
        }
        Type::Applied(base, args) => {
            let base = import_semantic_type(source, target, base, imported);
            let args = args
                .into_iter()
                .map(|arg| import_semantic_type(source, target, arg, imported))
                .collect();
            target.intern(Type::Applied(base, args))
        }
        Type::Union(members) => {
            let members = members
                .into_iter()
                .map(|member| import_semantic_type(source, target, member, imported))
                .collect();
            target.intern(Type::Union(members))
        }
        Type::Tuple(members) => {
            let members = members
                .into_iter()
                .map(|member| import_semantic_type(source, target, member, imported))
                .collect();
            target.intern(Type::Tuple(members))
        }
        other @ (Type::Struct(_)
        | Type::Enum(_)
        | Type::Interface(_)
        | Type::Param(_)
        | Type::Infer(_)
        | Type::InferUnion(_)
        | Type::Error) => target.intern(other),
    };
    imported.insert(ty, imported_ty);
    imported_ty
}

pub(super) fn import_index_expr(
    source: &TypeTable,
    target: &mut TypeTable,
    index: radix::semantic::IndexId,
) -> radix::semantic::IndexId {
    match source.get_index(index).clone() {
        IndexExpr::Tuple(items) => {
            let items = items
                .into_iter()
                .map(|item| import_index_expr(source, target, item))
                .collect();
            target.intern_index(IndexExpr::Tuple(items))
        }
        other => target.intern_index(other),
    }
}

pub(super) fn command_cli_args_type(
    unit: &AnalyzedPackageUnit,
    command: &CliCommand,
) -> Option<TypeId> {
    unit.analysis.hir.items.iter().find_map(|item| {
        let HirItemKind::Function(function) = &item.kind else {
            return None;
        };
        (function.name == command.function_symbol)
            .then(|| function.cli_args.as_ref().map(|param| param.ty))
            .flatten()
    })
}
