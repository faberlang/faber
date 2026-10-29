//! Remap program text symbols across interner tables.

use super::{
    Interner, MirAggregate, MirAggregateFields, MirAggregateItem, MirCallee, MirConstant,
    MirFunction, MirOperand, MirOptionChainLink, MirOptionOp, MirPlace, MirProgram, MirProjection,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind, MirValue,
    MirValueKind, Symbol,
};
use radix_mir::{MirFilumReduceOp, MirIntrinsic, MirShapeExpr};

/// Remap a name symbol from the lowering interner into the merged interner.
/// A `Symbol` always means text in its named interner (ARCH2-2), so the
/// re-intern is unconditional: generated identity tokens and positional
/// indices carry their own types and can never reach this walker.
pub(super) fn remap_optional_symbol(
    name: Option<Symbol>,
    source: &Interner,
    target: &mut Interner,
) -> Option<Symbol> {
    name.map(|symbol| target.intern(source.resolve(symbol)))
}

/// Remap a name symbol that is always present; see [`remap_optional_symbol`].
fn remap_symbol(name: Symbol, source: &Interner, target: &mut Interner) -> Symbol {
    target.intern(source.resolve(name))
}

pub(super) fn remap_function_text_symbols(
    function: &mut MirFunction,
    source: &Interner,
    target: &mut Interner,
) {
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

pub(super) fn remap_program_text_symbols(
    program: &mut MirProgram,
    source: &Interner,
    target: &mut Interner,
) {
    for function in &mut program.functions {
        remap_function_text_symbols(function, source, target);
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
            remap_operand_text_symbols(operand, source, target);
        }
        MirTerminatorKind::TryCall {
            destination,
            callee,
            args,
            ok_block: _,
            error_place,
            error_block: _,
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
        MirTerminatorKind::Branch {
            condition,
            then_block: _,
            else_block: _,
        } => {
            remap_operand_text_symbols(condition, source, target);
        }
        MirTerminatorKind::Switch {
            value,
            cases,
            default: _,
        } => {
            remap_operand_text_symbols(value, source, target);
            for case in cases {
                remap_constant_text_symbols(&mut case.value, source, target);
            }
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

pub(super) fn remap_value_text_symbols(
    value: &mut MirValue,
    source: &Interner,
    target: &mut Interner,
) {
    match &mut value.kind {
        MirValueKind::Operand(operand) => remap_operand_text_symbols(operand, source, target),
        MirValueKind::Closure(closure) => {
            remap_operand_text_symbols(&mut closure.environment, source, target);
        }
        MirValueKind::Unary { op: _, operand } => {
            remap_operand_text_symbols(operand, source, target);
        }
        MirValueKind::Binary { op: _, lhs, rhs } => {
            remap_operand_text_symbols(lhs, source, target);
            remap_operand_text_symbols(rhs, source, target);
        }
        MirValueKind::Option(op) => remap_option_text_symbols(op, source, target),
        MirValueKind::TensorExtent { tensor, .. } => {
            remap_operand_text_symbols(tensor, source, target);
        }
    }
}

pub(super) fn remap_operand_text_symbols(
    operand: &mut MirOperand,
    source: &Interner,
    target: &mut Interner,
) {
    match operand {
        MirOperand::Place(place) => remap_place_text_symbols(place, source, target),
        MirOperand::Constant(constant) => remap_constant_text_symbols(constant, source, target),
        MirOperand::Temp(_) | MirOperand::Value(_) => {}
    }
}

pub(super) fn remap_place_text_symbols(
    place: &mut MirPlace,
    source: &Interner,
    target: &mut Interner,
) {
    for projection in &mut place.projections {
        match projection {
            MirProjection::Field(field) | MirProjection::VariantField { variant: _, field } => {
                *field = remap_symbol(*field, source, target);
            }
            MirProjection::Index(operand) | MirProjection::ProvenIndex(operand) => {
                remap_operand_text_symbols(operand, source, target)
            }
            MirProjection::ClosureCapture {
                environment: _,
                source: _,
            }
            | MirProjection::VectorLane(_)
            | MirProjection::MatrixCell { row: _, column: _ }
            | MirProjection::SharedCell => {}
        }
    }
}

pub(super) fn remap_callee_text_symbols(
    callee: &mut MirCallee,
    source: &Interner,
    target: &mut Interner,
) {
    match callee {
        MirCallee::Closure(closure) => {
            remap_operand_text_symbols(&mut closure.environment, source, target);
        }
        MirCallee::Value(operand) => remap_operand_text_symbols(operand, source, target),
        MirCallee::Function(_)
        | MirCallee::Definition {
            source: _,
            type_args: _,
        } => {}
    }
}

pub(super) fn remap_runtime_call_text_symbols(
    call: &mut MirRuntimeCall,
    source: &Interner,
    target: &mut Interner,
) {
    // EXHAUSTIVE by design: every `MirIntrinsic` variant states whether its
    // payload carries interner symbols. Keep it wildcard-free so a new
    // intrinsic fails to compile here instead of silently resolving to
    // unrelated text in the merged interner.
    match &mut call.intrinsic {
        MirIntrinsic::Provider(provider) => {
            for part in &mut provider.module {
                *part = remap_symbol(*part, source, target);
            }
            provider.name = remap_symbol(provider.name, source, target);
        }
        MirIntrinsic::FormatString { template } | MirIntrinsic::FormatValue { spec: template } => {
            *template = remap_symbol(*template, source, target);
        }
        MirIntrinsic::Convert(conversion) => {
            for param in &mut conversion.params {
                *param = remap_symbol(*param, source, target);
            }
            if let Some(recovery) = &mut conversion.recovery {
                remap_operand_text_symbols(recovery, source, target);
            }
            for defaults in &mut conversion.struct_defaults {
                for field in &mut defaults.fields {
                    field.name = remap_symbol(field.name, source, target);
                    remap_operand_text_symbols(&mut field.value, source, target);
                }
            }
        }
        MirIntrinsic::FilumReduce(filum) => remap_filum_sum_text_symbols(filum, source, target),
        MirIntrinsic::TensorShapeCheck(check) => {
            for relation in &mut check.relations {
                remap_shape_expr_text_symbols(&mut relation.left, source, target);
                remap_shape_expr_text_symbols(&mut relation.right, source, target);
            }
        }
        MirIntrinsic::TensorShapeSize(expression) => {
            remap_shape_expr_text_symbols(expression, source, target);
        }
        // Ids, plans, and tags only — no interner symbols to re-intern.
        MirIntrinsic::Diagnostic(_)
        | MirIntrinsic::Assert
        | MirIntrinsic::Collection(_)
        | MirIntrinsic::Scan(_)
        | MirIntrinsic::Atomic(_)
        | MirIntrinsic::Panic
        | MirIntrinsic::Sermo(_)
        | MirIntrinsic::SermoOpen
        | MirIntrinsic::SermoSetOpener
        | MirIntrinsic::ReadLine
        | MirIntrinsic::ReadAll
        | MirIntrinsic::Cede
        | MirIntrinsic::DeviceBlock(_)
        | MirIntrinsic::GpuBuiltin(_)
        | MirIntrinsic::CursorStream(_)
        | MirIntrinsic::Gradient(_)
        | MirIntrinsic::TypeCheck(_)
        | MirIntrinsic::TensorLift(_)
        | MirIntrinsic::Pow
        | MirIntrinsic::VectorOp(_)
        | MirIntrinsic::Duplicate
        | MirIntrinsic::TensorLimes(_) => {}
    }
    for arg in &mut call.args {
        remap_operand_text_symbols(arg, source, target);
    }
}

fn remap_shape_expr_text_symbols(
    expression: &mut MirShapeExpr,
    source: &Interner,
    target: &mut Interner,
) {
    match expression {
        MirShapeExpr::TensorFieldAxis { fields, .. } => {
            for field in fields {
                *field = remap_symbol(*field, source, target);
            }
        }
        MirShapeExpr::Product(factors) => {
            for factor in factors {
                remap_shape_expr_text_symbols(factor, source, target);
            }
        }
        MirShapeExpr::Affine { base, .. } => remap_shape_expr_text_symbols(base, source, target),
        MirShapeExpr::Divide { numerator, .. } => {
            remap_shape_expr_text_symbols(numerator, source, target);
        }
        MirShapeExpr::Literal(_)
        | MirShapeExpr::ScalarArgument { .. }
        | MirShapeExpr::TensorAxis { .. } => {}
    }
}

/// Filum declaration facts carry the lane, term, and coordinate binder names
/// plus the captured term body — real MIR whose own statements, and any
/// intrinsic nested in them, hold lowering-interner symbols.
fn remap_filum_sum_text_symbols(
    filum: &mut MirFilumReduceOp,
    source: &Interner,
    target: &mut Interner,
) {
    filum.filum_binder = remap_optional_symbol(filum.filum_binder, source, target);
    filum.term_binder_name = remap_optional_symbol(filum.term_binder_name, source, target);
    for coordinate in &mut filum.coordinates {
        coordinate.name = remap_optional_symbol(coordinate.name, source, target);
    }
    for statement in &mut filum.term_body {
        remap_statement_text_symbols(statement, source, target);
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
                        remap_operand_text_symbols(operand, source, target);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                item.name = remap_symbol(item.name, source, target);
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

pub(super) fn remap_option_text_symbols(
    op: &mut MirOptionOp,
    source: &Interner,
    target: &mut Interner,
) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand) => remap_operand_text_symbols(operand, source, target),
        MirOptionOp::Unwrap { value, mode: _ } => remap_operand_text_symbols(value, source, target),
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
        MirOptionChainLink::Field(field)
        | MirOptionChainLink::VariantField { variant: _, field } => {
            *field = remap_symbol(*field, source, target);
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
            *symbol = remap_symbol(*symbol, source, target);
        }
        MirConstant::Regex { pattern, flags } => {
            *pattern = remap_symbol(*pattern, source, target);
            if let Some(flags) = flags {
                *flags = remap_symbol(*flags, source, target);
            }
        }
        MirConstant::Int(_)
        | MirConstant::UInt(_)
        | MirConstant::Float(_)
        | MirConstant::Bool(_)
        | MirConstant::Nil
        | MirConstant::Unit
        | MirConstant::Octeti(_)
        | MirConstant::Function(_)
        // Digits carry no symbols.
        | MirConstant::Magnus(_)
        | MirConstant::ShapeSize { shape: _, axis: _ } => {}
    }
}
