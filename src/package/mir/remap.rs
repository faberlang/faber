//! Remap program text symbols across interner tables.

use super::*;

/// Remap a name symbol from the lowering interner into the merged interner.
/// A symbol outside the source interner's string table is a GENERATED
/// identity token (the HIR-lowering synthetic range), not a text name —
/// it stays as-is (the value carries no text meaning either interner).
pub(super) fn remap_optional_symbol(
    name: Option<Symbol>,
    source: &Interner,
    target: &mut Interner,
) -> Option<Symbol> {
    name.map(|symbol| {
        if (symbol.0 as usize) < source.strings().len() {
            target.intern(source.resolve(symbol))
        } else {
            symbol
        }
    })
}

pub(super) fn remap_program_text_symbols(program: &mut MirProgram, source: &Interner, target: &mut Interner) {
    for function in &mut program.functions {
        // Function names are symbols from the lowering interner; they must be
        // remapped too or `MirNames` (built against the merged interner)
        // resolves a foreign symbol value.
        function.name = remap_optional_symbol(function.name, source, target);
        // Param and local names are the same kind of lowering-interner
        // symbols: without remapping they resolve to UNRELATED strings in the
        // merged interner (the entry's symbol space), garbling the
        // device-constructor's buffer names — the library-backed train_step
        // slots then alias the entry's locals (`vacua`/`strue` instead of
        // `weight`/`bias`). S5-U5c: remap them so the merged library body
        // keeps its declared names.
        for param in &mut function.params {
            param.name = remap_optional_symbol(param.name, source, target);
        }
        for local in &mut function.locals {
            local.name = remap_optional_symbol(local.name, source, target);
        }
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                remap_statement_text_symbols(statement, source, target);
            }
            remap_terminator_text_symbols(&mut block.terminator, source, target);
        }
    }
}

pub(super) fn remap_statement_text_symbols(
    statement: &mut MirStatement,
    source: &Interner,
    target: &mut Interner,
) {
    match &mut statement.kind {
        MirStatementKind::Assign { place, value } => {
            remap_place_text_symbols(place, source, target);
            remap_value_text_symbols(value, source, target);
        }
        MirStatementKind::Call {
            destination,
            callee,
            args,
        } => {
            if let Some(destination) = destination {
                remap_place_text_symbols(destination, source, target);
            }
            remap_callee_text_symbols(callee, source, target);
            for arg in args {
                remap_operand_text_symbols(arg, source, target);
            }
        }
        MirStatementKind::RuntimeCall { destination, call } => {
            if let Some(destination) = destination {
                remap_place_text_symbols(destination, source, target);
            }
            remap_runtime_call_text_symbols(call, source, target);
        }
        MirStatementKind::Construct {
            destination,
            aggregate,
        } => {
            remap_place_text_symbols(destination, source, target);
            remap_aggregate_text_symbols(aggregate, source, target);
        }
    }
}

pub(super) fn remap_terminator_text_symbols(
    terminator: &mut MirTerminator,
    source: &Interner,
    target: &mut Interner,
) {
    match &mut terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            remap_operand_text_symbols(operand, source, target)
        }
        MirTerminatorKind::TryCall {
            destination,
            callee,
            args,
            error_place,
            ..
        } => {
            if let Some(destination) = destination {
                remap_place_text_symbols(destination, source, target);
            }
            remap_callee_text_symbols(callee, source, target);
            for arg in args {
                remap_operand_text_symbols(arg, source, target);
            }
            remap_place_text_symbols(error_place, source, target);
        }
        MirTerminatorKind::Branch { condition, .. } => {
            remap_operand_text_symbols(condition, source, target)
        }
        MirTerminatorKind::Switch { value, cases, .. } => {
            remap_operand_text_symbols(value, source, target);
            for case in cases {
                remap_constant_text_symbols(&mut case.value, source, target);
            }
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

pub(super) fn remap_value_text_symbols(value: &mut MirValue, source: &Interner, target: &mut Interner) {
    match &mut value.kind {
        MirValueKind::Operand(operand) => remap_operand_text_symbols(operand, source, target),
        MirValueKind::Closure(closure) => {
            remap_operand_text_symbols(&mut closure.environment, source, target)
        }
        MirValueKind::Unary { operand, .. } => remap_operand_text_symbols(operand, source, target),
        MirValueKind::Binary { lhs, rhs, .. } => {
            remap_operand_text_symbols(lhs, source, target);
            remap_operand_text_symbols(rhs, source, target);
        }
        MirValueKind::Option(op) => remap_option_text_symbols(op, source, target),
    }
}

pub(super) fn remap_operand_text_symbols(operand: &mut MirOperand, source: &Interner, target: &mut Interner) {
    match operand {
        MirOperand::Place(place) => remap_place_text_symbols(place, source, target),
        MirOperand::Constant(constant) => remap_constant_text_symbols(constant, source, target),
        MirOperand::Temp(_) | MirOperand::Value(_) => {}
    }
}

pub(super) fn remap_place_text_symbols(place: &mut MirPlace, source: &Interner, target: &mut Interner) {
    for projection in &mut place.projections {
        match projection {
            MirProjection::Field(field) => {
                *field = target.intern(source.resolve(*field));
            }
            MirProjection::VariantField { field, .. } => {
                *field = target.intern(source.resolve(*field));
            }
            MirProjection::Index(operand) => remap_operand_text_symbols(operand, source, target),
            MirProjection::ClosureCapture { .. }
            | MirProjection::VectorLane(_)
            | MirProjection::MatrixCell { .. } => {}
        }
    }
}

pub(super) fn remap_callee_text_symbols(callee: &mut MirCallee, source: &Interner, target: &mut Interner) {
    match callee {
        MirCallee::Closure(closure) => {
            remap_operand_text_symbols(&mut closure.environment, source, target)
        }
        MirCallee::Value(operand) => remap_operand_text_symbols(operand, source, target),
        MirCallee::Function(_) | MirCallee::Definition { .. } => {}
    }
}

pub(super) fn remap_runtime_call_text_symbols(
    call: &mut MirRuntimeCall,
    source: &Interner,
    target: &mut Interner,
) {
    if let radix::mir::MirIntrinsic::FormatString { template } = &mut call.intrinsic {
        *template = target.intern(source.resolve(*template));
    }
    if let radix::mir::MirIntrinsic::Convert(conversion) = &mut call.intrinsic {
        if let Some(recovery) = &mut conversion.recovery {
            remap_operand_text_symbols(recovery, source, target);
        }
        for defaults in &mut conversion.struct_defaults {
            for field in &mut defaults.fields {
                field.name = target.intern(source.resolve(field.name));
                remap_operand_text_symbols(&mut field.value, source, target);
            }
        }
    }
    for arg in &mut call.args {
        remap_operand_text_symbols(arg, source, target);
    }
}

pub(super) fn remap_aggregate_text_symbols(
    aggregate: &mut MirAggregate,
    source: &Interner,
    target: &mut Interner,
) {
    match &mut aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        remap_operand_text_symbols(operand, source, target)
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                item.name = target.intern(source.resolve(item.name));
                remap_operand_text_symbols(&mut item.value, source, target);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                remap_operand_text_symbols(&mut item.key, source, target);
                remap_operand_text_symbols(&mut item.value, source, target);
            }
        }
    }
}

pub(super) fn remap_option_text_symbols(op: &mut MirOptionOp, source: &Interner, target: &mut Interner) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand) => remap_operand_text_symbols(operand, source, target),
        MirOptionOp::Unwrap { value, .. } => remap_operand_text_symbols(value, source, target),
        MirOptionOp::Coalesce { value, fallback } => {
            remap_operand_text_symbols(value, source, target);
            remap_operand_text_symbols(fallback, source, target);
        }
        MirOptionOp::Chain { base, link } => {
            remap_operand_text_symbols(base, source, target);
            remap_option_chain_text_symbols(link, source, target);
        }
        MirOptionOp::None => {}
    }
}

pub(super) fn remap_option_chain_text_symbols(
    link: &mut MirOptionChainLink,
    source: &Interner,
    target: &mut Interner,
) {
    match link {
        MirOptionChainLink::Index(operand) => remap_operand_text_symbols(operand, source, target),
        MirOptionChainLink::Call { callee, args } => {
            remap_callee_text_symbols(callee, source, target);
            for arg in args {
                remap_operand_text_symbols(arg, source, target);
            }
        }
        MirOptionChainLink::Field(field) => {
            *field = target.intern(source.resolve(*field));
        }
        MirOptionChainLink::VariantField { field, .. } => {
            *field = target.intern(source.resolve(*field));
        }
    }
}

pub(super) fn remap_constant_text_symbols(
    constant: &mut MirConstant,
    source: &Interner,
    target: &mut Interner,
) {
    match constant {
        MirConstant::String(symbol) | MirConstant::Ascii(symbol) => {
            *symbol = target.intern(source.resolve(*symbol));
        }
        MirConstant::Regex { pattern, flags } => {
            *pattern = target.intern(source.resolve(*pattern));
            if let Some(flags) = flags {
                *flags = target.intern(source.resolve(*flags));
            }
        }
        MirConstant::Int(_)
        | MirConstant::UInt(_)
        | MirConstant::Float(_)
        | MirConstant::Bool(_)
        | MirConstant::Nil
        | MirConstant::Unit
        | MirConstant::Octeti(_)
        | MirConstant::Function(_) => {}
    }
}

