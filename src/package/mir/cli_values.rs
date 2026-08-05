//! Package-MIR CLI record/value planning and supported-surface checks.

use super::*;

pub(super) fn plan_cli_command_records(
    unit: &mut AnalyzedPackageUnit,
    command: &CliCommand,
    argumenta: &[String],
    global_fields: Vec<MirRuntimeRecordField>,
    global_operands: &[PlannedCliOperand<'_>],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Option<CliRecordFieldsByLocal>> {
    let mut operands = global_operands.to_vec();
    operands.extend(planned_cli_operands(
        &unit.analysis.interner,
        command.operands.iter(),
    ));
    let options = planned_cli_options(&unit.analysis.interner, command.options.iter());
    let parsed = parse_cli_arguments(unit, &options, argumenta, diagnostics)?;
    let mut fields = global_fields;
    fields.extend(parsed.option_fields);
    fields.extend(cli_operand_record_fields(
        unit,
        &operands,
        &parsed.positionals,
        diagnostics,
    )?);
    let Some(args_binding) = &command.args_binding else {
        if !fields.is_empty() {
            diagnostics.push(unsupported_cli_diagnostic(
                &unit.path,
                "CLI argument records",
            ));
            return None;
        }
        return Some(None);
    };
    let args_symbol = unit.analysis.interner.intern(args_binding);
    Some(Some(HashMap::from([(args_symbol, fields)])))
}

pub(super) fn cli_operand_record_fields(
    unit: &mut AnalyzedPackageUnit,
    operands: &[PlannedCliOperand<'_>],
    argumenta: &[String],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Vec<MirRuntimeRecordField>> {
    let mut index = 0;
    let mut fields = Vec::new();
    for operand in operands {
        let value = if cli_operand_consumes_many(operand.operand) {
            let raw = argumenta[index..].iter().collect::<Vec<_>>();
            index = argumenta.len();
            cli_operand_list_value(unit, operand, raw, diagnostics)?
        } else if let Some(value) = argumenta.get(index) {
            index += 1;
            MirRuntimeRecordValue::Operand(cli_operand_value(unit, operand, value, diagnostics)?)
        } else if let Some(default) = &operand.operand.default {
            MirRuntimeRecordValue::Operand(cli_default_operand_value(
                unit,
                operand,
                default,
                diagnostics,
            )?)
        } else {
            push_cli_operand_missing_diagnostic(unit, operand, diagnostics);
            return None;
        };
        fields.push(MirRuntimeRecordField {
            name: unit.analysis.interner.intern(&operand.binding_name),
            value,
        });
    }
    if argumenta.get(index).is_some() {
        diagnostics.push(unsupported_cli_diagnostic(
            &unit.path,
            "CLI argument parsing",
        ));
        return None;
    }
    Some(fields)
}

pub(super) fn cli_operand_consumes_many(operand: &CliOperand) -> bool {
    operand.rest || matches!(operand.ty, CliType::ListaTextus | CliType::ListaNumerus)
}

pub(super) fn cli_operand_value(
    unit: &mut AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    value: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MirOperand> {
    let constant = match &operand.operand.ty {
        CliType::Textus | CliType::Ignotum => {
            let payload = textus_literal_payload(value);
            MirConstant::String(unit.analysis.interner.intern(&payload))
        }
        CliType::Numerus => match value.parse::<i64>() {
            Ok(value) => MirConstant::Int(value),
            Err(_) => {
                push_cli_operand_parse_diagnostic(unit, operand, "numerus", value, diagnostics);
                return None;
            }
        },
        CliType::Fractus => match value.parse::<f64>() {
            Ok(value) => MirConstant::Float(value),
            Err(_) => {
                push_cli_operand_parse_diagnostic(unit, operand, "fractus", value, diagnostics);
                return None;
            }
        },
        CliType::Bivalens => match value.parse::<bool>() {
            Ok(value) => MirConstant::Bool(value),
            Err(_) => {
                push_cli_operand_parse_diagnostic(unit, operand, "bivalens", value, diagnostics);
                return None;
            }
        },
        CliType::Octeti => MirConstant::Octeti(value.as_bytes().to_vec()),
        CliType::ListaTextus | CliType::ListaNumerus => return None,
    };
    Some(MirOperand::Constant(constant))
}

pub(super) fn cli_operand_list_value(
    unit: &mut AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    values: Vec<&String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MirRuntimeRecordValue> {
    let ty = cli_record_type(
        &mut unit.analysis.types,
        &operand.operand.ty,
        operand.operand.rest,
    )?;
    let items = values
        .into_iter()
        .map(|value| cli_operand_list_item_value(unit, operand, value, diagnostics))
        .collect::<Option<Vec<_>>>()?;
    Some(MirRuntimeRecordValue::Array {
        ty: MirType::semantic(ty),
        items,
    })
}

pub(super) fn cli_operand_list_item_value(
    unit: &mut AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    value: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MirOperand> {
    match &operand.operand.ty {
        CliType::Textus | CliType::Ignotum | CliType::ListaTextus => {
            let payload = textus_literal_payload(value);
            Some(MirOperand::Constant(MirConstant::String(
                unit.analysis.interner.intern(&payload),
            )))
        }
        CliType::Numerus | CliType::ListaNumerus => match value.parse::<i64>() {
            Ok(value) => Some(MirOperand::Constant(MirConstant::Int(value))),
            Err(_) => {
                push_cli_operand_parse_diagnostic(unit, operand, "numerus", value, diagnostics);
                None
            }
        },
        CliType::Fractus => match value.parse::<f64>() {
            Ok(value) => Some(MirOperand::Constant(MirConstant::Float(value))),
            Err(_) => {
                push_cli_operand_parse_diagnostic(unit, operand, "fractus", value, diagnostics);
                None
            }
        },
        CliType::Bivalens => match value.parse::<bool>() {
            Ok(value) => Some(MirOperand::Constant(MirConstant::Bool(value))),
            Err(_) => {
                push_cli_operand_parse_diagnostic(unit, operand, "bivalens", value, diagnostics);
                None
            }
        },
        CliType::Octeti => None,
    }
}

pub(super) fn cli_option_value(
    unit: &mut AnalyzedPackageUnit,
    option: &PlannedCliOption<'_>,
    value: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MirOperand> {
    let constant = match &option.option.ty {
        CliType::Textus | CliType::Ignotum => {
            let payload = textus_literal_payload(value);
            MirConstant::String(unit.analysis.interner.intern(&payload))
        }
        CliType::Numerus => match value.parse::<i64>() {
            Ok(value) => MirConstant::Int(value),
            Err(_) => {
                push_cli_option_parse_diagnostic(unit, option, "numerus", value, diagnostics);
                return None;
            }
        },
        CliType::Fractus => match value.parse::<f64>() {
            Ok(value) => MirConstant::Float(value),
            Err(_) => {
                push_cli_option_parse_diagnostic(unit, option, "fractus", value, diagnostics);
                return None;
            }
        },
        CliType::Bivalens => match value.parse::<bool>() {
            Ok(value) => MirConstant::Bool(value),
            Err(_) => {
                push_cli_option_parse_diagnostic(unit, option, "bivalens", value, diagnostics);
                return None;
            }
        },
        CliType::Octeti | CliType::ListaTextus | CliType::ListaNumerus => return None,
    };
    Some(MirOperand::Constant(constant))
}

pub(super) fn cli_default_operand_value(
    unit: &mut AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    default: &CliDefault,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<MirOperand> {
    cli_default_value(unit, &operand.operand.ty, default).or_else(|| {
        push_cli_operand_default_diagnostic(unit, operand, diagnostics);
        None
    })
}

pub(super) fn cli_default_value(
    unit: &mut AnalyzedPackageUnit,
    ty: &CliType,
    default: &CliDefault,
) -> Option<MirOperand> {
    let constant = match (ty, default) {
        (CliType::Textus | CliType::Ignotum, CliDefault::Text(value)) => {
            let payload = textus_literal_payload(value);
            MirConstant::String(unit.analysis.interner.intern(&payload))
        }
        (CliType::Numerus, CliDefault::Integer(value)) => MirConstant::Int(*value),
        (CliType::Fractus, CliDefault::Float(value)) => MirConstant::Float(*value),
        (CliType::Fractus, CliDefault::Integer(value)) => MirConstant::Float(*value as f64),
        (CliType::Bivalens, CliDefault::Bool(value)) => MirConstant::Bool(*value),
        _ => return None,
    };
    Some(MirOperand::Constant(constant))
}

pub(super) fn push_cli_operand_parse_diagnostic(
    unit: &AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    ty: &str,
    value: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name = operand.binding_name.as_str();
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR could not parse CLI operand `{name}` value `{value}` as {ty}; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string())
        .with_arg("issue", "package_mir_cli_surface_unsupported")
        .with_arg("surface", "CLI operand parse")
        .with_arg("operand", name),
    );
}

pub(super) fn push_cli_option_parse_diagnostic(
    unit: &AnalyzedPackageUnit,
    option: &PlannedCliOption<'_>,
    ty: &str,
    value: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name = option.binding_name.as_str();
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR could not parse CLI option `{name}` value `{value}` as {ty}; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string())
        .with_arg("issue", "package_mir_cli_surface_unsupported")
        .with_arg("surface", "CLI option parse")
        .with_arg("option", name),
    );
}

pub(super) fn push_cli_option_default_diagnostic(
    unit: &AnalyzedPackageUnit,
    option: &PlannedCliOption<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name = option.binding_name.as_str();
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR does not yet support CLI option `{name}` default value; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string())
        .with_arg("issue", "package_mir_cli_option_default_unsupported")
        .with_arg("option", name),
    );
}

pub(super) fn push_cli_operand_default_diagnostic(
    unit: &AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name = operand.binding_name.as_str();
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR does not yet support CLI operand `{name}` default value; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string())
        .with_arg("issue", "package_mir_cli_operand_default_unsupported")
        .with_arg("operand", name),
    );
}

pub(super) fn push_cli_operand_missing_diagnostic(
    unit: &AnalyzedPackageUnit,
    operand: &PlannedCliOperand<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name = operand.binding_name.as_str();
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR expected CLI operand `{name}` but no value was provided; use compiled package execution for this surface"
        ))
        .with_file(unit.path.display().to_string())
        .with_arg("issue", "package_mir_cli_surface_unsupported")
        .with_arg("surface", "CLI operand value")
        .with_arg("operand", name),
    );
}

pub(super) fn command_has_unsupported_options(command: &CliCommand) -> bool {
    command
        .options
        .iter()
        .any(|option| !is_package_mir_supported_option(option))
}

pub(super) struct MatchedCliCommand<'a> {
    pub(super) command: &'a CliCommand,
    pub(super) consumed: usize,
}

pub(super) fn matching_cli_command<'a>(
    commands: &'a [CliCommand],
    argumenta: &[String],
) -> Option<MatchedCliCommand<'a>> {
    let mut routes = commands
        .iter()
        .flat_map(|command| {
            command_routes(command)
                .into_iter()
                .map(move |route| (command, route))
        })
        .collect::<Vec<_>>();
    routes.sort_by_key(|(_, route)| std::cmp::Reverse(route.len()));
    routes
        .into_iter()
        .find(|(_, route)| cli_route_matches(route, argumenta))
        .map(|(command, route)| MatchedCliCommand {
            command,
            consumed: route.len(),
        })
}

pub(super) fn command_routes(command: &CliCommand) -> Vec<Vec<&str>> {
    std::iter::once(command.path.iter().map(String::as_str).collect::<Vec<_>>())
        .chain(command.aliases.iter().map(|alias| alias_path(alias)))
        .collect()
}

pub(super) fn alias_path(alias: &str) -> Vec<&str> {
    alias.split('/').filter(|part| !part.is_empty()).collect()
}

pub(super) fn cli_route_matches(route: &[&str], argumenta: &[String]) -> bool {
    argumenta.len() >= route.len()
        && route
            .iter()
            .enumerate()
            .all(|(index, part)| argumenta[index] == *part)
}

pub(super) fn command_unit_index(package: &AnalyzedPackage, command: &CliCommand) -> Option<usize> {
    if command.module_path.is_some() {
        return package
            .units
            .iter()
            .enumerate()
            .find(|unit| {
                !unit.1.is_entry
                    && unit.1.analysis.cli_program.as_ref().is_some_and(|program| {
                        program.commands.iter().any(|candidate| {
                            candidate.path == command.path && candidate.function == command.function
                        })
                    })
            })
            .map(|(index, _)| index);
    }
    package.units.iter().position(|unit| unit.is_entry)
}

pub(super) fn command_in_unit<'a>(
    unit: &'a AnalyzedPackageUnit,
    command: &CliCommand,
) -> Option<&'a CliCommand> {
    unit.analysis
        .cli_program
        .as_ref()?
        .commands
        .iter()
        .find(|candidate| candidate.path == command.path && candidate.function == command.function)
}

pub(super) fn has_unsupported_package_mir_operands<'a>(
    operands: impl IntoIterator<Item = &'a CliOperand>,
) -> bool {
    let operands = operands.into_iter().collect::<Vec<_>>();
    operands.iter().enumerate().any(|(index, operand)| {
        !is_package_mir_supported_operand(operand, index + 1 == operands.len())
    })
}

pub(super) fn is_package_mir_supported_operand(operand: &CliOperand, is_final: bool) -> bool {
    if !is_package_mir_supported_operand_default(operand) {
        return false;
    }
    if operand.rest {
        return is_final && !matches!(operand.ty, CliType::Octeti);
    }
    if matches!(operand.ty, CliType::ListaTextus | CliType::ListaNumerus) {
        return is_final;
    }
    true
}

pub(super) fn is_package_mir_supported_operand_default(operand: &CliOperand) -> bool {
    operand.default.as_ref().is_none_or(|default| {
        is_package_mir_scalar_default(&operand.ty, default)
            && !matches!(
                operand.ty,
                CliType::Octeti | CliType::ListaTextus | CliType::ListaNumerus
            )
    })
}

pub(super) fn is_package_mir_scalar_default(ty: &CliType, default: &CliDefault) -> bool {
    matches!(
        (ty, default),
        (CliType::Textus | CliType::Ignotum, CliDefault::Text(_))
            | (CliType::Numerus, CliDefault::Integer(_))
            | (
                CliType::Fractus,
                CliDefault::Float(_) | CliDefault::Integer(_)
            )
            | (CliType::Bivalens, CliDefault::Bool(_))
    )
}

pub(super) fn is_package_mir_supported_option(option: &CliOption) -> bool {
    if option.flag {
        return matches!(&option.ty, CliType::Bivalens);
    }
    is_package_mir_scalar_option(option)
}

pub(super) fn cli_option_is_nullable(option: &CliOption) -> bool {
    option.default.is_none() && !option.flag
}

pub(super) fn is_package_mir_scalar_option(option: &CliOption) -> bool {
    matches!(
        &option.ty,
        CliType::Textus
            | CliType::Ignotum
            | CliType::Numerus
            | CliType::Fractus
            | CliType::Bivalens
    ) && option
        .default
        .as_ref()
        .is_none_or(|default| is_package_mir_scalar_default(&option.ty, default))
}

pub(super) fn cli_record_type(
    types: &mut radix::semantic::TypeTable,
    ty: &CliType,
    rest: bool,
) -> Option<TypeId> {
    let base = match ty {
        CliType::Textus | CliType::Ignotum => Primitive::Textus,
        CliType::Numerus => Primitive::Numerus,
        CliType::Fractus => Primitive::Fractus,
        CliType::Bivalens => Primitive::Bivalens,
        CliType::Octeti => Primitive::Octeti,
        CliType::ListaTextus => {
            let textus = types.primitive(Primitive::Textus);
            return Some(types.array(textus));
        }
        CliType::ListaNumerus => {
            let numerus = types.primitive(Primitive::Numerus);
            return Some(types.array(numerus));
        }
    };
    let base = types.primitive(base);
    if rest {
        Some(types.array(base))
    } else {
        Some(base)
    }
}

pub(super) fn textus_literal_payload(text: &str) -> String {
    let mut escaped = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

pub(super) fn unsupported_cli_diagnostic(path: &Path, surface: &str) -> Diagnostic {
    crate::package_diagnostic_error(format!(
        "package MIR does not yet support {surface}; use compiled package execution for this surface"
    ))
    .with_file(path.display().to_string())
    .with_arg("issue", "package_mir_cli_surface_unsupported")
    .with_arg("surface", surface)
}

pub(super) fn package_mir_cli_exit_code(
    exit: &Option<CliExit>,
    args_name: Option<Symbol>,
    fields: &[MirRuntimeRecordField],
    unit: &AnalyzedPackageUnit,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Option<i32>> {
    let Some(exit) = exit else {
        return Some(None);
    };
    match exit {
        CliExit::Fixed(code) => match i32::try_from(*code) {
            Ok(code) => Some(Some(code)),
            Err(_) => {
                diagnostics.push(unsupported_cli_diagnostic(
                    &unit.path,
                    "CLI exit codes outside i32 range",
                ));
                None
            }
        },
        CliExit::Field { object, field } => {
            let args_name = args_name
                .map(|symbol| unit.analysis.interner.resolve(symbol))
                .unwrap_or("");
            if object != args_name {
                diagnostics.push(unsupported_cli_diagnostic(
                    &unit.path,
                    "CLI dynamic exit expressions",
                ));
                return None;
            }
            let value = fields.iter().find_map(|record_field| {
                let name = unit.analysis.interner.resolve(record_field.name);
                (name == field).then(|| package_mir_runtime_record_i32(&record_field.value))?
            });
            match value {
                Some(code) => Some(Some(code)),
                None => {
                    diagnostics.push(unsupported_cli_diagnostic(
                        &unit.path,
                        "CLI dynamic exit expressions",
                    ));
                    None
                }
            }
        }
        CliExit::Binding(_) | CliExit::Unsupported => {
            diagnostics.push(unsupported_cli_diagnostic(
                &unit.path,
                "CLI dynamic exit expressions",
            ));
            None
        }
    }
}

pub(super) fn package_mir_runtime_record_i32(value: &MirRuntimeRecordValue) -> Option<i32> {
    let MirRuntimeRecordValue::Operand(MirOperand::Constant(MirConstant::Int(value))) = value
    else {
        return None;
    };
    i32::try_from(*value).ok()
}
