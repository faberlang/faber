//! Bind the fmir-text runtime CLI section into the lowered program.

use super::*;

pub(super) fn bind_fmir_text_runtime_cli<H: Host + ?Sized>(
    program: &mut MirProgram,
    cli: Option<&FmirTextCliSection>,
    entry_function: &str,
    interner: &mut Interner,
    host: &H,
    path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let Some(cli) = cli else {
        return Ok(());
    };
    if cli.root.operand.is_empty() {
        return Ok(());
    }
    let argumenta = host
        .argumenta()
        .map_err(|e| vec![mir_diag(path, e.message)])?;
    let list_count = cli
        .root
        .operand
        .iter()
        .filter(|operand| matches!(operand.ty, FmirTextCliValueType::ListaTextus))
        .count();
    if list_count > 1 {
        return Err(vec![mir_diag(
            path,
            "fmir-text image supports at most one rest textus list operand",
        )]);
    }
    let scalar_count = cli.root.operand.len() - list_count;
    if list_count == 0 {
        if argumenta.len() != scalar_count {
            return Err(vec![mir_diag(
                path,
                format!(
                    "fmir-text image expected {} runtime argument(s), got {}",
                    scalar_count,
                    argumenta.len()
                ),
            )]);
        }
    } else if argumenta.len() < scalar_count {
        return Err(vec![mir_diag(
            path,
            format!(
                "fmir-text image expected at least {scalar_count} runtime argument(s), got {}",
                argumenta.len()
            ),
        )]);
    }
    let mut bindings = Vec::new();
    let mut argument_index = 0;
    for operand in &cli.root.operand {
        let name = interner.intern(&operand.field);
        if matches!(operand.ty, FmirTextCliValueType::ListaTextus) {
            let items = argumenta[argument_index..]
                .iter()
                .map(|raw| MirOperand::Constant(MirConstant::String(interner.intern(raw))))
                .collect();
            argument_index = argumenta.len();
            bindings.push(FmirTextCliBinding::List { name, items });
            continue;
        }
        let raw = &argumenta[argument_index];
        argument_index += 1;
        let value = fmir_text_runtime_cli_operand(&operand.ty, raw, interner, path)?;
        bindings.push(FmirTextCliBinding::Scalar(MirNamedOperand { name, value }));
    }
    if patch_fmir_text_cli_record(program, cli, entry_function, interner, &bindings) {
        Ok(())
    } else {
        Err(vec![mir_diag(
            path,
            "fmir-text image could not bind runtime CLI record",
        )])
    }
}

pub(super) fn fmir_text_runtime_cli_operand(
    ty: &FmirTextCliValueType,
    raw: &str,
    interner: &mut Interner,
    path: &Path,
) -> Result<MirOperand, Vec<Diagnostic>> {
    let constant = match ty {
        FmirTextCliValueType::Textus => MirConstant::String(interner.intern(raw)),
        FmirTextCliValueType::Numerus => MirConstant::Int(raw.parse::<i64>().map_err(|_| {
            vec![mir_diag(
                path,
                format!("fmir-text runtime argument `{raw}` is not numerus"),
            )]
        })?),
        FmirTextCliValueType::Fractus => MirConstant::Float(raw.parse::<f64>().map_err(|_| {
            vec![mir_diag(
                path,
                format!("fmir-text runtime argument `{raw}` is not fractus"),
            )]
        })?),
        FmirTextCliValueType::Bivalens => MirConstant::Bool(raw.parse::<bool>().map_err(|_| {
            vec![mir_diag(
                path,
                format!("fmir-text runtime argument `{raw}` is not bivalens"),
            )]
        })?),
        FmirTextCliValueType::ListaTextus => {
            return Err(vec![mir_diag(
                path,
                "fmir-text runtime list operand must bind remaining arguments, not a scalar",
            )]);
        }
    };
    Ok(MirOperand::Constant(constant))
}

pub(super) enum FmirTextCliBinding {
    Scalar(MirNamedOperand),
    List {
        name: radix::lexer::Symbol,
        items: Vec<MirOperand>,
    },
}

impl FmirTextCliBinding {
    fn name(&self) -> radix::lexer::Symbol {
        match self {
            Self::Scalar(field) => field.name,
            Self::List { name, .. } => *name,
        }
    }
}

pub(super) fn patch_fmir_text_cli_record(
    program: &mut MirProgram,
    cli: &FmirTextCliSection,
    entry_function: &str,
    interner: &Interner,
    bindings: &[FmirTextCliBinding],
) -> bool {
    if cli.root.record.is_empty() {
        return false;
    }
    for function in &mut program.functions {
        if !fmir_text_function_matches_entry(function, entry_function, interner) {
            continue;
        }
        let mut list_patches = Vec::new();
        let mut found_record = false;
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                let MirStatementKind::Construct { aggregate, .. } = &mut statement.kind else {
                    continue;
                };
                if !matches!(aggregate.kind, MirAggregateKind::Record) {
                    continue;
                }
                let MirAggregateFields::Named(fields) = &mut aggregate.fields else {
                    continue;
                };
                if patch_fmir_text_cli_record_fields(fields, bindings, &mut list_patches) {
                    found_record = true;
                    break;
                }
            }
            if found_record {
                break;
            }
        }
        if found_record {
            return apply_fmir_text_cli_list_patches(function, list_patches);
        }
    }
    false
}

pub(super) fn fmir_text_function_matches_entry(
    function: &MirFunction,
    entry_function: &str,
    interner: &Interner,
) -> bool {
    if entry_function == "run_entry"
        && function.source.is_none()
        && function.name.is_none()
        && function.params.is_empty()
    {
        return true;
    }
    function
        .name
        .map(|name| interner.resolve(name) == entry_function)
        .unwrap_or(false)
}

pub(super) fn patch_fmir_text_cli_record_fields(
    fields: &mut [MirNamedOperand],
    bindings: &[FmirTextCliBinding],
    list_patches: &mut Vec<(MirOperand, Vec<MirOperand>)>,
) -> bool {
    if fields.len() != bindings.len() {
        return false;
    }
    let field_names = fields
        .iter()
        .map(|field| field.name)
        .collect::<HashSet<_>>();
    let binding_names = bindings
        .iter()
        .map(FmirTextCliBinding::name)
        .collect::<HashSet<_>>();
    if field_names != binding_names {
        return false;
    }
    for binding in bindings {
        let Some(field) = fields.iter_mut().find(|field| field.name == binding.name()) else {
            return false;
        };
        match binding {
            FmirTextCliBinding::Scalar(operand) => {
                field.value = operand.value.clone();
            }
            FmirTextCliBinding::List { items, .. } => {
                list_patches.push((field.value.clone(), items.clone()));
            }
        }
    }
    true
}

pub(super) fn apply_fmir_text_cli_list_patches(
    function: &mut MirFunction,
    mut patches: Vec<(MirOperand, Vec<MirOperand>)>,
) -> bool {
    if patches.is_empty() {
        return true;
    }
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            let MirStatementKind::Construct {
                destination,
                aggregate,
            } = &mut statement.kind
            else {
                continue;
            };
            if !matches!(aggregate.kind, MirAggregateKind::Array) {
                continue;
            }
            let MirAggregateFields::Ordered(items) = &mut aggregate.fields else {
                continue;
            };
            let Some(patch_index) = patches.iter().position(|(operand, _)| {
                matches!(
                    operand,
                    MirOperand::Temp(temp) if destination == &MirPlace::temp(*temp)
                )
            }) else {
                continue;
            };
            let (_, new_items) = patches.swap_remove(patch_index);
            *items = new_items
                .into_iter()
                .map(MirAggregateItem::Operand)
                .collect();
        }
    }
    patches.is_empty()
}
