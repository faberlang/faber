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
    if argumenta.len() != cli.root.operand.len() {
        return Err(vec![mir_diag(
            path,
            format!(
                "fmir-text image expected {} runtime argument(s), got {}",
                cli.root.operand.len(),
                argumenta.len()
            ),
        )]);
    }
    let mut bindings = Vec::new();
    for (operand, raw) in cli.root.operand.iter().zip(argumenta.iter()) {
        let name = interner.intern(&operand.field);
        let value = fmir_text_runtime_cli_operand(&operand.ty, raw, interner, path)?;
        bindings.push(MirNamedOperand { name, value });
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
    };
    Ok(MirOperand::Constant(constant))
}

pub(super) fn patch_fmir_text_cli_record(
    program: &mut MirProgram,
    cli: &FmirTextCliSection,
    entry_function: &str,
    interner: &Interner,
    bindings: &[MirNamedOperand],
) -> bool {
    if cli.root.record.is_empty() {
        return false;
    }
    for function in &mut program.functions {
        if !fmir_text_function_matches_entry(function, entry_function, interner) {
            continue;
        }
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
                if patch_fmir_text_cli_record_fields(fields, bindings) {
                    return true;
                }
            }
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
    bindings: &[MirNamedOperand],
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
        .map(|binding| binding.name)
        .collect::<HashSet<_>>();
    if field_names != binding_names {
        return false;
    }
    for binding in bindings {
        if let Some(field) = fields.iter_mut().find(|field| field.name == binding.name) {
            field.value = binding.value.clone();
        }
    }
    true
}
