//! Rewrite program source identities and unmapped library sources.

use super::{
    DefId, HashSet, MirAggregate, MirAggregateFields, MirAggregateItem, MirCallee,
    MirClosureEnvironment, MirConstant, MirFunction, MirOperand, MirOptionChainLink, MirOptionOp,
    MirPlace, MirProgram, MirProjection, MirStatement, MirStatementKind, MirTerminator,
    MirTerminatorKind, MirValue, MirValueKind, Path, SourceRewrites,
};

pub(super) fn rewrite_templates_sources(
    templates: &mut [MirFunction],
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    if source_rewrites.is_empty() || templates.is_empty() {
        return;
    }
    for template in templates {
        rewrite_function_sources(template, unit_path, source_rewrites);
    }
}

pub(super) fn rewrite_generic_param_sources(
    generic_params: &mut [(DefId, Vec<radix_mir_lower::lower::MirGenericParam>)],
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    if source_rewrites.is_empty() {
        return;
    }
    for (source, _) in generic_params {
        if let Some(rewritten) = rewritten_source(unit_path, *source, source_rewrites) {
            *source = rewritten;
        }
    }
}

pub(super) fn rewrite_program_sources(
    program: &mut MirProgram,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    if source_rewrites.is_empty() {
        return;
    }
    for function in &mut program.functions {
        rewrite_function_sources(function, unit_path, source_rewrites);
    }
}

/// Rewrite one function's own source id, its statement operands, and its
/// terminators through the unit's source map.
fn rewrite_function_sources(
    function: &mut MirFunction,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    if let Some(source) = function.source
        && let Some(rewritten) = rewritten_source(unit_path, source, source_rewrites)
    {
        function.source = Some(rewritten);
    }
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            rewrite_statement_sources(statement, unit_path, source_rewrites);
        }
        rewrite_terminator_sources(&mut block.terminator, unit_path, source_rewrites);
    }
}

/// Rewrite the unit's closure-environment capture sources under the same map
/// as [`rewrite_program_sources`]. The closure body's
/// `MirProjection::ClosureCapture { source }` and the environment row's
/// `captures[].source` name the same definition; rewriting only the body
/// desyncs body and metadata ("closure capture projection is missing
/// metadata").
pub(super) fn rewrite_closure_environment_sources(
    environments: &mut [MirClosureEnvironment],
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    if source_rewrites.is_empty() {
        return;
    }
    for environment in environments {
        for capture in &mut environment.captures {
            if let Some(rewritten) = rewritten_source(unit_path, capture.source, source_rewrites) {
                capture.source = rewritten;
            }
        }
    }
}

pub(super) fn rewritten_source(
    unit_path: &Path,
    source: DefId,
    source_rewrites: &SourceRewrites,
) -> Option<DefId> {
    source_rewrites
        .get(&(unit_path.to_path_buf(), source))
        .copied()
}

/// Assign fresh synthetic sources to library functions whose source ids were
/// not known at link time — the auto-generated `__backward_callee_N`
/// companions for unannotated AIR callees. Separate library analyses reuse
/// their own companion-id allocators, so their raw sources would collide in
/// the merged program.
pub(super) fn extend_unmapped_library_sources(
    program: &MirProgram,
    library_path: &Path,
    source_rewrites: &mut SourceRewrites,
    next_synthetic: &mut u32,
) {
    let key = |source: DefId| (library_path.to_path_buf(), source);
    // Sources already rewritten to a synthetic id are the map's *values*, not
    // its keys; skip both so already-rewritten functions are never re-mapped.
    let synthetic_values: HashSet<DefId> = source_rewrites.values().copied().collect();
    let mut pending = Vec::new();
    for function in &program.functions {
        let Some(source) = function.source else {
            continue;
        };
        if source_rewrites.contains_key(&key(source)) || synthetic_values.contains(&source) {
            continue;
        }
        pending.push(source);
    }
    for source in pending {
        if source_rewrites.contains_key(&key(source)) {
            continue;
        }
        let synthetic = DefId(*next_synthetic);
        *next_synthetic += 1;
        source_rewrites.insert(key(source), synthetic);
    }
}

pub(super) fn rewrite_statement_sources(
    statement: &mut MirStatement,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match &mut statement.kind {
        MirStatementKind::Assign { value, .. } => {
            rewrite_value_sources(value, unit_path, source_rewrites);
        }
        MirStatementKind::Call { callee, args, .. } => {
            rewrite_callee_sources(callee, unit_path, source_rewrites);
            for arg in args {
                rewrite_operand_sources(arg, unit_path, source_rewrites);
            }
        }
        MirStatementKind::RuntimeCall { call, .. } => {
            for arg in &mut call.args {
                rewrite_operand_sources(arg, unit_path, source_rewrites);
            }
        }
        MirStatementKind::Construct { aggregate, .. } => {
            rewrite_aggregate_sources(aggregate, unit_path, source_rewrites);
        }
    }
}

pub(super) fn rewrite_terminator_sources(
    terminator: &mut MirTerminator,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match &mut terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            rewrite_operand_sources(operand, unit_path, source_rewrites);
        }
        MirTerminatorKind::TryCall {
            callee,
            args,
            error_place,
            ..
        } => {
            rewrite_callee_sources(callee, unit_path, source_rewrites);
            for arg in args {
                rewrite_operand_sources(arg, unit_path, source_rewrites);
            }
            rewrite_place_sources(error_place, unit_path, source_rewrites);
        }
        MirTerminatorKind::Branch { condition, .. } => {
            rewrite_operand_sources(condition, unit_path, source_rewrites);
        }
        MirTerminatorKind::Switch { value, cases, .. } => {
            rewrite_operand_sources(value, unit_path, source_rewrites);
            for case in cases {
                rewrite_constant_sources(&mut case.value, unit_path, source_rewrites);
            }
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

pub(super) fn rewrite_value_sources(
    value: &mut MirValue,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match &mut value.kind {
        MirValueKind::Operand(operand) | MirValueKind::Unary { operand, .. } => {
            rewrite_operand_sources(operand, unit_path, source_rewrites);
        }
        MirValueKind::Closure(closure) => {
            rewrite_operand_sources(&mut closure.environment, unit_path, source_rewrites);
        }
        MirValueKind::Binary { lhs, rhs, .. } => {
            rewrite_operand_sources(lhs, unit_path, source_rewrites);
            rewrite_operand_sources(rhs, unit_path, source_rewrites);
        }
        MirValueKind::Option(op) => rewrite_option_sources(op, unit_path, source_rewrites),
        MirValueKind::TensorExtent { tensor, .. } => {
            rewrite_operand_sources(tensor, unit_path, source_rewrites);
        }
    }
}

pub(super) fn rewrite_operand_sources(
    operand: &mut MirOperand,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match operand {
        MirOperand::Place(place) => rewrite_place_sources(place, unit_path, source_rewrites),
        MirOperand::Constant(constant) => {
            rewrite_constant_sources(constant, unit_path, source_rewrites);
        }
        MirOperand::Temp(_) | MirOperand::Value(_) => {}
    }
}

pub(super) fn rewrite_place_sources(
    place: &mut MirPlace,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    for projection in &mut place.projections {
        match projection {
            // Variant DefIds are not function sources: they ride the nominal
            // variant remap (`rewrite_lowered_variant_defs`), never the
            // function-source rewrite — a colliding source id in the rewrite
            // map would corrupt the variant identity with a synthetic id.
            MirProjection::VariantField { .. }
            | MirProjection::Field(_)
            | MirProjection::VectorLane(_)
            | MirProjection::MatrixCell { .. }
            | MirProjection::SharedCell => {}
            MirProjection::ClosureCapture { source, .. } => {
                if let Some(rewritten) = rewritten_source(unit_path, *source, source_rewrites) {
                    *source = rewritten;
                }
            }
            MirProjection::Index(operand) | MirProjection::ProvenIndex(operand) => {
                rewrite_operand_sources(operand, unit_path, source_rewrites);
            }
        }
    }
}

pub(super) fn rewrite_constant_sources(
    _constant: &mut MirConstant,
    _unit_path: &Path,
    _source_rewrites: &SourceRewrites,
) {
}

pub(super) fn rewrite_callee_sources(
    callee: &mut MirCallee,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match callee {
        MirCallee::Definition { source, .. } => {
            if let Some(rewritten) = rewritten_source(unit_path, *source, source_rewrites) {
                *source = rewritten;
            }
        }
        MirCallee::Value(operand) => rewrite_operand_sources(operand, unit_path, source_rewrites),
        MirCallee::Function(_) | MirCallee::Closure(_) => {}
    }
}

pub(super) fn rewrite_aggregate_sources(
    aggregate: &mut MirAggregate,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match &mut aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        rewrite_operand_sources(operand, unit_path, source_rewrites);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                rewrite_operand_sources(&mut item.value, unit_path, source_rewrites);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                rewrite_operand_sources(&mut item.key, unit_path, source_rewrites);
                rewrite_operand_sources(&mut item.value, unit_path, source_rewrites);
            }
        }
    }
}

pub(super) fn rewrite_option_sources(
    op: &mut MirOptionOp,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand)
        | MirOptionOp::Unwrap { value: operand, .. } => {
            rewrite_operand_sources(operand, unit_path, source_rewrites);
        }
        MirOptionOp::Coalesce { value, fallback } => {
            rewrite_operand_sources(value, unit_path, source_rewrites);
            rewrite_operand_sources(fallback, unit_path, source_rewrites);
        }
        MirOptionOp::Chain { base, link } => {
            rewrite_operand_sources(base, unit_path, source_rewrites);
            rewrite_option_chain_sources(link, unit_path, source_rewrites);
        }
        MirOptionOp::None => {}
    }
}

pub(super) fn rewrite_option_chain_sources(
    link: &mut MirOptionChainLink,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
) {
    match link {
        // Variant DefIds are not function sources (see
        // `rewrite_place_sources`); the option-chain variant field stays
        // identity-mapped like a plain field.
        MirOptionChainLink::VariantField { .. } | MirOptionChainLink::Field(_) => {}
        MirOptionChainLink::Index(operand) => {
            rewrite_operand_sources(operand, unit_path, source_rewrites);
        }
        MirOptionChainLink::Call { callee, args } => {
            rewrite_callee_sources(callee, unit_path, source_rewrites);
            for arg in args {
                rewrite_operand_sources(arg, unit_path, source_rewrites);
            }
        }
    }
}
