// Sibling + root items: explicit `use super` lists carry the seams the mir/
// split routes through `use super::*` (wildcard imports are denied).
use super::{
    device_diag, BTreeMap, BTreeSet, DefId, Diagnostic, Interner, MirCallee, MirConstant,
    MirFunction, MirFunctionId, MirKernelShaderStage, MirLocalId, MirNames, MirOperand, MirPlace,
    MirPlaceBase, MirProjection, MirStatementKind, MirTempId, MirTerminatorKind, MirValueKind,
    Type, ValidatedMir,
};

// ---------------------------------------------------------------------------
// Training-plan analysis (S5-U5): RepeatingStep + trainable-param identity
// ---------------------------------------------------------------------------

/// The default declared step count of a RepeatingStep device program (S5-U5):
/// the device section carries `RepeatingStep` + a declared step count; the
/// U5 default is 100 steps, and the constructor's step-count admission
/// validates it against the source loop bound when the entry loop carries a
/// constant bound. The manifest `[device] steps` channel (S5-U5b) overrides
/// the default, and the effective count rides the wire's
/// `RepeatingStep(count)` lifetime variant so an image-loaded route recovers
/// it.
pub(crate) const DEFAULT_TRAINING_STEPS: u32 = 100;

/// One trainable parameter of a RepeatingStep training program (S5-U5 +
/// `stage-5-param-identity.md`): the entry-loop local that is BOTH
///
/// - (a) a selected input of the device-resident backward companion
///   (backward selection — the gradient-to-primal identity), AND
/// - (b) the read source of the train_step update, whose updated tuple
///   element the loop re-binds into the same local (step write).
///
/// The materializer projects it to a PerProgram `InOut` buffer with
/// `HostProvided` init, and the train_step kernel's output slot carrying the
/// update aliases the SAME buffer (in-place update at the device level —
/// the next step's read source).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrainableParam {
    /// The training-step MIR function that updates this parameter.
    pub(crate) step_function: MirFunctionId,
    /// Parameter position in the train_step signature (the arg position in
    /// the entry loop's step call).
    pub(crate) param_position: u32,
    /// The train_step result-tuple output slot carrying the updated value
    /// (0-based; the slot aliases the parameter's buffer).
    pub(crate) update_output: u32,
    /// The entry-loop local the parameter lives in (semantic identity).
    pub(crate) entry_local: MirLocalId,
}

/// The faber-owned training-plan facts of a package (S5-U5): the repeating
/// step count, the trainable-param identity rows, and the gradient-flow
/// links. Derived from the entry loop's value graph — never from name/shape
/// coincidence (param-identity.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrainingPlanFacts {
    /// The effective declared step count of the RepeatingStep program.
    pub(crate) step_count: u32,
    /// The source loop bound when the entry loop's iteration count is a
    /// constant (the authority the declared count must agree with).
    pub(crate) source_loop_bound: Option<u32>,
    /// The trainable-param identity rows.
    pub(crate) trainable: Vec<TrainableParam>,
    /// The gradient-flow links: which companion result-tuple slot carries
    /// each trainable param's gradient and which train_step input position
    /// consumes it (the backward → train_step data-flow).
    pub(crate) gradients: Vec<GradientLink>,
}

/// One gradient-flow link (S5-U5): the companion result-tuple slot carrying a
/// trainable parameter's gradient (the `MirCompanionSelectedInput`'s
/// `gradient_slot`), and the train_step input position that consumes it. This
/// is what makes the backward → train_step data-flow explicit — the gradient
/// buffer written by the companion is the gradient buffer read by the step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GradientLink {
    /// The training-step MIR function consuming the gradient.
    pub(crate) step_function: MirFunctionId,
    /// The train_step input position (param position) holding the gradient.
    pub(crate) gradient_position: u32,
    /// The companion function producing the gradient.
    pub(crate) companion: DefId,
    /// The companion result-tuple slot carrying the gradient.
    pub(crate) gradient_slot: u32,
}

/// Fail-closed step-count admission (S5-U5 done-when): a declared repeating
/// step count must agree with the source loop bound when the entry loop
/// carries a constant bound — a mismatch fails construction closed (the
/// image's step count and the source loop's iteration count are the same
/// contract). When the source bound is absent the declared count applies.
///
/// # Errors
/// Fail-closed when both a declared count and a constant source bound exist
/// and disagree.
pub(crate) fn admit_step_count(declared: u32, source: Option<u32>) -> Result<u32, Vec<Diagnostic>> {
    if let Some(source) = source {
        if declared != source {
            return Err(vec![device_diag(
                "step count",
                format!(
                    "declared repeating step count {declared} contradicts the source training loop bound {source}; the device image's step count and the entry loop's iteration count must agree"
                ),
            )]);
        }
    }
    Ok(declared)
}

/// The package entry function (the lowered `incipit`), when the program has
/// one.
fn entry_function<'a>(
    validated: &'a ValidatedMir<'_>,
    interner: &'a Interner,
) -> Option<&'a MirFunction> {
    MirNames::new(validated.program(), validated.validation().types, interner).entry_function()
}

/// The local a plain place operand references, if any.
fn operand_local(operand: &MirOperand) -> Option<MirLocalId> {
    let MirOperand::Place(place) = operand else {
        return None;
    };
    let MirPlaceBase::Local(local) = place.base else {
        return None;
    };
    Some(local)
}

/// The constant loop bound of the entry function's training loop, when the
/// loop's iteration count is a literal: the largest positive integer constant
/// assigned to a local that a branch condition compares (the
/// `itera ab 0‥steps` bound when `steps` is a constant). Tracks one
/// local→local copy hop (the lowered `steps` copy). `None` when the loop
/// bound is not a compile-time constant.
fn source_loop_bound(entry: &MirFunction) -> Option<u32> {
    /// A local's initialization: a literal integer or a copy of another
    /// local (the lowered `steps` copy). First assignment wins (the prelude
    /// dominates the loop body's re-assignments).
    #[derive(Clone, Copy)]
    enum LocalDef {
        Const(i64),
        Copy(MirLocalId),
    }
    let mut defs: BTreeMap<MirLocalId, LocalDef> = BTreeMap::new();
    for block in &entry.blocks {
        for statement in &block.statements {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            let MirPlaceBase::Local(local) = place.base else {
                continue;
            };
            if !place.projections.is_empty() || defs.contains_key(&local) {
                continue;
            }
            match &value.kind {
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(n))) => {
                    defs.insert(local, LocalDef::Const(*n));
                }
                MirValueKind::Operand(MirOperand::Place(MirPlace {
                    base: MirPlaceBase::Local(other),
                    projections,
                })) if projections.is_empty() => {
                    defs.insert(local, LocalDef::Copy(*other));
                }
                _ => {}
            }
        }
    }
    fn resolve(local: MirLocalId, defs: &BTreeMap<MirLocalId, LocalDef>) -> Option<i64> {
        let mut seen: Vec<MirLocalId> = Vec::new();
        let mut current = local;
        loop {
            if seen.contains(&current) {
                return None;
            }
            seen.push(current);
            match defs.get(&current)? {
                LocalDef::Const(n) => return Some(*n),
                LocalDef::Copy(next) => current = *next,
            }
        }
    }

    // The branch conditions that compare two locals (the loop guard).
    let mut compared: Vec<MirLocalId> = Vec::new();
    for block in &entry.blocks {
        let MirTerminatorKind::Branch { condition, .. } = &block.terminator.kind else {
            continue;
        };
        let temp = match condition {
            MirOperand::Temp(temp) => *temp,
            MirOperand::Place(MirPlace {
                base: MirPlaceBase::Temp(temp),
                projections,
            }) if projections.is_empty() => *temp,
            _ => continue,
        };
        for statement in &block.statements {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            if place.base != MirPlaceBase::Temp(temp) {
                continue;
            }
            let MirValueKind::Binary { lhs, rhs, .. } = &value.kind else {
                continue;
            };
            for operand in [lhs, rhs] {
                if let Some(local) = operand_local(operand) {
                    compared.push(local);
                }
            }
        }
    }
    compared
        .iter()
        .filter_map(|local| resolve(*local, &defs).filter(|n| *n > 0))
        .map(|n| u32::try_from(n).unwrap_or(u32::MAX))
        .max()
}

/// The entry-loop's tuple trace: how tuple-return call results are unpacked
/// into locals (the lowered `iuncta` pattern-match).
#[derive(Debug, Clone, Default)]
struct TupleTrace {
    /// Local → the call-destination temp it was plain-copied from
    /// (`tuple ← Temp(t)`).
    plain_copy: BTreeMap<MirLocalId, MirTempId>,
    /// Local → the (call-destination temp, element index) it was extracted
    /// from (`element ← tuple[i]`, including through a plain copy hop).
    extract: BTreeMap<MirLocalId, (MirTempId, u32)>,
}

/// Build the tuple trace of one function's statements.
fn build_tuple_trace(entry: &MirFunction) -> TupleTrace {
    let mut trace = TupleTrace::default();
    for block in &entry.blocks {
        for statement in &block.statements {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            let MirPlaceBase::Local(local) = place.base else {
                continue;
            };
            if !place.projections.is_empty() {
                continue;
            }
            let (base_temp, index) = match &value.kind {
                MirValueKind::Operand(MirOperand::Temp(temp)) => (*temp, None),
                MirValueKind::Operand(MirOperand::Place(inner)) => {
                    let temp = match inner.base {
                        MirPlaceBase::Temp(temp) => temp,
                        // One hop through a plain copy: `Local(l)[i]` where
                        // `l` holds the call's tuple.
                        MirPlaceBase::Local(other) => {
                            let Some(temp) = trace.plain_copy.get(&other).copied() else {
                                continue;
                            };
                            temp
                        }
                    };
                    (temp, place_tuple_index(inner))
                }
                _ => continue,
            };
            match index {
                None => {
                    trace.plain_copy.insert(local, base_temp);
                }
                Some(index) => {
                    trace.extract.insert(local, (base_temp, index));
                }
            }
        }
    }
    trace
}

/// The tuple element index of a place's projections, when they are exactly
/// one constant index.
fn place_tuple_index(place: &MirPlace) -> Option<u32> {
    if place.projections.len() != 1 {
        return None;
    }
    let MirProjection::Index(MirOperand::Constant(MirConstant::Int(n))) = &place.projections[0]
    else {
        return None;
    };
    u32::try_from(*n).ok()
}

/// The tuple element an assign value extracts from a call-destination temp:
/// `Temp(t)[i]`, `Local(l)[i]` (where `l` was plain-copied from the call
/// temp), or a re-binding of an already-extracted local (`Place(Local(l))`
/// where `l ← tuple[i]`).
fn tuple_index_of(kind: &MirValueKind, trace: &TupleTrace) -> Option<(MirTempId, u32)> {
    let MirValueKind::Operand(MirOperand::Place(place)) = kind else {
        return None;
    };
    match place.base {
        MirPlaceBase::Temp(temp) => Some((temp, place_tuple_index(place)?)),
        MirPlaceBase::Local(local) => {
            if place.projections.is_empty() {
                // Re-binding a previously extracted element.
                return trace.extract.get(&local).copied();
            }
            // `Local(l)[i]`: `l` holds the call's tuple (plain copy).
            let temp = *trace.plain_copy.get(&local)?;
            Some((temp, place_tuple_index(place)?))
        }
    }
}

/// The entry-loop calls to device-resident backward companions: the
/// entry-loop local at each companion-selected primal parameter position,
/// and — through the tuple trace — the entry-loop local each companion
/// result-tuple slot's value is extracted into.
struct CompanionCalls {
    /// Selected-input locals: the entry-loop local passed at a selected
    /// primal parameter position.
    selected_locals: BTreeSet<MirLocalId>,
    /// (companion, result-tuple slot) → the entry-loop local holding that
    /// slot's value (the gradient extraction).
    tuple_locals: BTreeMap<(u32, u32), MirLocalId>,
    /// (entry-loop local) → (companion, gradient slot) for the selected
    /// inputs, keyed by the local passed at the primal parameter position.
    selected_gradient_slots: BTreeMap<MirLocalId, (u32, u32)>,
}

fn companion_calls(
    entry: &MirFunction,
    companions: &radix_mir::device::MirCompanionMap,
) -> CompanionCalls {
    let trace = build_tuple_trace(entry);
    let mut selected_locals: BTreeSet<MirLocalId> = BTreeSet::new();
    let mut selected_gradient_slots: BTreeMap<MirLocalId, (u32, u32)> = BTreeMap::new();
    let mut tuple_locals: BTreeMap<(u32, u32), MirLocalId> = BTreeMap::new();
    for block in &entry.blocks {
        for statement in &block.statements {
            let MirStatementKind::Call {
                destination,
                callee,
                args,
            } = &statement.kind
            else {
                continue;
            };
            let MirCallee::Definition { source, .. } = callee else {
                continue;
            };
            let Some(companion) = companions
                .iter()
                .find(|candidate| candidate.device_resident && candidate.companion == *source)
            else {
                continue;
            };
            for selected_input in &companion.selected_inputs {
                if let Some(arg) = args.get(selected_input.position as usize) {
                    if let Some(local) = operand_local(arg) {
                        selected_locals.insert(local);
                        selected_gradient_slots
                            .insert(local, (source.0, selected_input.gradient_slot));
                    }
                }
            }
            let Some(destination) = destination else {
                continue;
            };
            let MirPlaceBase::Temp(temp) = destination.base else {
                continue;
            };
            if !destination.projections.is_empty() {
                continue;
            }
            // The gradient extractions from this companion call's result.
            for (local, (call_temp, index)) in &trace.extract {
                if *call_temp == temp {
                    tuple_locals.insert((source.0, *index), *local);
                }
            }
        }
    }
    CompanionCalls {
        selected_locals,
        tuple_locals,
        selected_gradient_slots,
    }
}

/// Whether a program function is a tuple-return function (the `iuncta` /
/// result-tuple shape a training step carries its updated parameters in).
fn is_tuple_return(function: &MirFunction, validated: &ValidatedMir<'_>) -> bool {
    matches!(
        validated.validation().types.get(function.return_ty.semantic_id()),
        Type::Tuple(elements) if !elements.is_empty()
    )
}

/// One entry-loop call to a package-local training-step function: the callee
/// function id, the destination temp holding the result tuple, and the
/// entry-loop locals at each argument position.
struct StepCall {
    function: MirFunctionId,
    destination: MirTempId,
    args: Vec<MirLocalId>,
}

/// Derive the training-plan facts from the entry loop (S5-U5).
///
/// The identity rule (`stage-5-param-identity.md`): a trainable parameter is
/// an entry-loop local that (a) is a selected input of a device-resident
/// backward companion AND (b) is the read source of a train_step call whose
/// updated result-tuple element the loop re-binds into the same local. The
/// intersection of the two facts decides the trainable-param rows — never a
/// name/shape coincidence. `None` when the package has no training loop (no
/// device-resident companion called from the entry, or no step functions).
///
/// # Errors
/// Fail-closed when a training loop's step function is not a package-local
/// MIR function (e.g. a provider/library-backed `train_step`), when the
/// train_step result tuple cannot be traced, or when a trainable param's
/// write path does not read the param it claims to update.
pub(crate) fn training_plan_facts(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    companions: &radix_mir::device::MirCompanionMap,
    declared_steps: Option<u32>,
) -> Result<Option<TrainingPlanFacts>, Vec<Diagnostic>> {
    let program = validated.program();
    let Some(entry) = entry_function(validated, interner) else {
        return Ok(None);
    };
    // (a) backward selection: the entry-loop locals passed to a device-
    // resident companion at its selected-input positions.
    let companion_calls = companion_calls(entry, companions);
    if companion_calls.selected_locals.is_empty() {
        return Ok(None);
    }

    // Step calls: direct calls to package-local functions (Definition callee
    // whose source is a program function) that are neither compute kernels
    // nor companions and carry a tuple return — the optimizer step shape.
    let mut step_calls: Vec<StepCall> = Vec::new();
    for block in &entry.blocks {
        for statement in &block.statements {
            let MirStatementKind::Call {
                destination,
                callee,
                args,
            } = &statement.kind
            else {
                continue;
            };
            let MirCallee::Definition { source, .. } = callee else {
                continue;
            };
            let Some(step_function) = program
                .functions
                .iter()
                .find(|function| function.source == Some(*source))
            else {
                continue;
            };
            if step_function.shader_stage == Some(MirKernelShaderStage::Compute) {
                continue;
            }
            if companions
                .iter()
                .any(|companion| companion.companion == *source)
            {
                continue;
            }
            if !is_tuple_return(step_function, validated) {
                continue;
            }
            let Some(destination) = destination else {
                continue;
            };
            let MirPlaceBase::Temp(temp) = destination.base else {
                continue;
            };
            if !destination.projections.is_empty() {
                continue;
            }
            step_calls.push(StepCall {
                function: step_function.id,
                destination: temp,
                args: args.iter().filter_map(operand_local).collect(),
            });
        }
    }
    let step_call_dests: BTreeMap<MirTempId, MirFunctionId> = step_calls
        .iter()
        .map(|call| (call.destination, call.function))
        .collect();

    // The loop's re-bindings: which result-tuple index re-binds which
    // entry-loop local (the def-use chain through the extraction temp).
    // key: (step function, tuple index) → entry local.
    let trace = build_tuple_trace(entry);
    let mut tuple_to_entry: BTreeMap<(MirFunctionId, u32), MirLocalId> = BTreeMap::new();
    for block in &entry.blocks {
        for statement in &block.statements {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            let MirPlaceBase::Local(local) = place.base else {
                continue;
            };
            if !place.projections.is_empty() {
                continue;
            }
            let Some(index) = tuple_index_of(&value.kind, &trace) else {
                continue;
            };
            if let Some(step_function) = step_call_dests.get(&index.0) {
                tuple_to_entry.insert((*step_function, index.1), local);
            }
        }
    }
    // The identity rule (b): a trainable parameter is the PRE-UPDATE
    // entry-loop local — a local the loop re-binds FROM the step result AND
    // passes INTO a step call. Extraction intermediates (the tuple element
    // copied into a fresh local before the re-bind) are not parameters.
    let step_arg_locals: BTreeSet<MirLocalId> = step_calls
        .iter()
        .flat_map(|call| call.args.iter().copied())
        .collect();
    tuple_to_entry.retain(|_, local| step_arg_locals.contains(local));

    // Trainable params: step-call args whose entry-local is BOTH companion-
    // selected (a) AND re-bound from a step result tuple element (b).
    let mut trainable: Vec<TrainableParam> = Vec::new();
    for call in &step_calls {
        for (position, entry_local) in call.args.iter().enumerate() {
            if !companion_calls.selected_locals.contains(entry_local) {
                continue;
            }
            let Some(update_output) = tuple_to_entry
                .iter()
                .find(|((function, _), local)| {
                    *function == call.function && **local == *entry_local
                })
                .map(|((_, index), _)| *index)
            else {
                continue;
            };
            trainable.push(TrainableParam {
                step_function: call.function,
                param_position: u32::try_from(position).unwrap_or(u32::MAX),
                update_output,
                entry_local: *entry_local,
            });
        }
    }
    if trainable.is_empty() {
        return Ok(None);
    }
    trainable.sort_by_key(|param| (param.step_function.0, param.param_position));
    trainable.dedup();

    // Gradient-flow links: for each trainable param, the companion
    // result-tuple slot carrying its gradient (the selected input's
    // `gradient_slot`), and the train_step input position consuming the
    // extracted gradient local. The backward → train_step edge is explicit
    // only through this graph connection — never a name/shape coincidence.
    let mut gradients: Vec<GradientLink> = Vec::new();
    for param in &trainable {
        let Some((companion, gradient_slot)) = companion_calls
            .selected_gradient_slots
            .get(&param.entry_local)
            .copied()
        else {
            continue;
        };
        let Some(gradient_local) = companion_calls
            .tuple_locals
            .get(&(companion, gradient_slot))
            .copied()
        else {
            continue;
        };
        for call in step_calls
            .iter()
            .filter(|call| call.function == param.step_function)
        {
            if let Some((position, _)) = call
                .args
                .iter()
                .enumerate()
                .find(|(_, local)| **local == gradient_local)
            {
                gradients.push(GradientLink {
                    step_function: param.step_function,
                    gradient_position: u32::try_from(position).unwrap_or(u32::MAX),
                    companion: DefId(companion),
                    gradient_slot,
                });
                break;
            }
        }
    }
    gradients.sort_by_key(|link| (link.step_function.0, link.gradient_position));
    gradients.dedup();

    // Fail-closed step-write verification (param-identity.md): every trainable
    // param's update output must be a local whose defining statement reads
    // the param local it claims to update (param' = param − lr·grad). A
    // train_step that writes a value without reading the param cannot
    // establish in-place identity.
    for param in &trainable {
        let step_function = program
            .functions
            .iter()
            .find(|function| function.id == param.step_function)
            .ok_or_else(|| {
                vec![device_diag(
                    "training step",
                    format!(
                        "train_step function DefId({}) is missing from the lowered MIR",
                        param.step_function.0
                    ),
                )]
            })?;
        let param_local = step_function
            .params
            .get(param.param_position as usize)
            .map(|param| param.local)
            .ok_or_else(|| {
                vec![device_diag(
                    "training step",
                    format!(
                        "train_step DefId({}) has no parameter at position {}",
                        param.step_function.0, param.param_position
                    ),
                )]
            })?;
        let update_local = tuple_return_locals(step_function)
            .and_then(|locals| locals.get(param.update_output as usize).copied())
            .ok_or_else(|| {
                vec![device_diag(
                    "training step",
                    format!(
                        "train_step DefId({}) result tuple has no element at slot {}",
                        param.step_function.0, param.update_output
                    ),
                )]
            })?;
        if !statement_defining_reads(step_function, update_local, param_local) {
            return Err(vec![device_diag(
                "param identity",
                format!(
                    "train_step DefId({}) writes update local l{} at tuple slot {} without reading the parameter local l{} it claims to update; in-place param identity requires the write path to read the pre-update parameter (param' = param − lr·grad)",
                    param.step_function.0,
                    update_local.0,
                    param.update_output,
                    param_local.0
                ),
            )]);
        }
    }

    let source_loop_bound = source_loop_bound(entry);
    let step_count = admit_step_count(
        declared_steps.unwrap_or(DEFAULT_TRAINING_STEPS),
        source_loop_bound,
    )?;
    Ok(Some(TrainingPlanFacts {
        step_count,
        source_loop_bound,
        trainable,
        gradients,
    }))
}

/// The return-tuple element locals of a tuple-return function, in tuple
/// order (from the aggregate construction that produces the returned tuple).
fn tuple_return_locals(function: &MirFunction) -> Option<Vec<MirLocalId>> {
    let block = function.blocks.first()?;
    let MirTerminatorKind::Return(Some(operand)) = &block.terminator.kind else {
        return None;
    };
    // The returned value: a plain temp or a plain local place.
    let returned = match operand {
        MirOperand::Temp(temp) => MirPlace::temp(*temp),
        MirOperand::Place(place) if place.projections.is_empty() => place.clone(),
        _ => return None,
    };
    for statement in &block.statements {
        let MirStatementKind::Construct {
            destination,
            aggregate,
        } = &statement.kind
        else {
            continue;
        };
        if destination != &returned {
            continue;
        }
        let locals: Vec<MirLocalId> = aggregate
            .fields
            .operands()
            .iter()
            .filter_map(|operand| operand_local(operand))
            .collect();
        if locals.is_empty() {
            return None;
        }
        return Some(locals);
    }
    None
}

/// Whether the statement that defines `destination_local` reads `source`
/// (the step-write verification: the update local's defining statement reads
/// the pre-update parameter). Traces one temp hop: the defining copy
/// (`nw ← t`) is traced to the producing runtime call (`t ← w − gw`), which
/// reads the parameter.
fn statement_defining_reads(
    function: &MirFunction,
    destination_local: MirLocalId,
    source: MirLocalId,
) -> bool {
    for block in &function.blocks {
        for statement in &block.statements {
            let writes = match &statement.kind {
                MirStatementKind::Assign { place, .. }
                | MirStatementKind::Construct {
                    destination: place, ..
                } => {
                    matches!(
                        place.base,
                        MirPlaceBase::Local(local) if local == destination_local
                    ) && place.projections.is_empty()
                }
                MirStatementKind::RuntimeCall { destination, .. }
                | MirStatementKind::Call { destination, .. } => {
                    destination.as_ref().is_some_and(|place| {
                        matches!(
                            place.base,
                            MirPlaceBase::Local(local) if local == destination_local
                        ) && place.projections.is_empty()
                    })
                }
            };
            if !writes {
                continue;
            }
            if statement_reads_local(statement, source) {
                return true;
            }
            // One temp hop: the defining value is a temp whose producing
            // statement reads `source`.
            if let Some(temp) = statement_value_temp(statement) {
                if function
                    .blocks
                    .iter()
                    .flat_map(|block| block.statements.iter())
                    .any(|producer| {
                        statement_dest_temp(producer) == Some(temp)
                            && statement_reads_local(producer, source)
                    })
                {
                    return true;
                }
            }
            return false;
        }
    }
    false
}

/// The temp a statement's assigned value is a plain copy of, if any.
fn statement_value_temp(statement: &radix_mir::MirStatement) -> Option<MirTempId> {
    let MirStatementKind::Assign { value, .. } = &statement.kind else {
        return None;
    };
    match &value.kind {
        MirValueKind::Operand(MirOperand::Temp(temp)) => Some(*temp),
        MirValueKind::Operand(MirOperand::Place(place))
            if place.projections.is_empty() && matches!(place.base, MirPlaceBase::Temp(_)) =>
        {
            let MirPlaceBase::Temp(temp) = place.base else {
                return None;
            };
            Some(temp)
        }
        _ => None,
    }
}

/// The temp a statement's destination names, if any.
fn statement_dest_temp(statement: &radix_mir::MirStatement) -> Option<MirTempId> {
    let destination = match &statement.kind {
        MirStatementKind::Assign { place, .. }
        | MirStatementKind::Construct {
            destination: place, ..
        } => Some(place),
        MirStatementKind::RuntimeCall { destination, .. }
        | MirStatementKind::Call { destination, .. } => destination.as_ref(),
    };
    destination.and_then(|place| match place.base {
        MirPlaceBase::Temp(temp) if place.projections.is_empty() => Some(temp),
        _ => None,
    })
}

/// Whether a statement reads the given local in any operand/value position.
fn statement_reads_local(statement: &radix_mir::MirStatement, local: MirLocalId) -> bool {
    match &statement.kind {
        MirStatementKind::Assign { value, .. } => match &value.kind {
            MirValueKind::Operand(operand) | MirValueKind::Unary { operand, .. } => {
                operand_local(operand) == Some(local)
            }
            MirValueKind::Binary { lhs, rhs, .. } => {
                operand_local(lhs) == Some(local) || operand_local(rhs) == Some(local)
            }
            _ => false,
        },
        MirStatementKind::RuntimeCall { call, .. } => call
            .args
            .iter()
            .any(|operand| operand_local(operand) == Some(local)),
        MirStatementKind::Call { args, .. } => args
            .iter()
            .any(|operand| operand_local(operand) == Some(local)),
        MirStatementKind::Construct { aggregate, .. } => aggregate
            .fields
            .operands()
            .iter()
            .any(|operand| operand_local(operand) == Some(local)),
    }
}
