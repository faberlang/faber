// Sibling + root items: explicit `use super` lists carry the seams the mir/
// split routes through `use super::*` (wildcard imports are denied).
use super::{
    collection_op_contract, decompose_kernel_function, device_diag, is_transformer_recipe_op,
    kernel_plan_for_function, subchain_signature_for_emission_with_source, training_plan_facts,
    transformer_subchain_signature_for_emission_with_source, tuple_return_locals, BTreeMap,
    BTreeSet, Binding, BufferId, BufferIdentity, BufferLifetime, BufferRole, BufferVersion,
    CollectionKernelPlan, DependencyEdge, DeviceProgram, DeviceProgramLifetime, DeviceResource,
    DeviceSemantics, Diagnostic, HashMap, InitializationFact, InitializationPolicy, Interner,
    KernelLaunchPlan, KernelUnit, LaunchId, LaunchUnit, LosslessMirCompanionEntry, MirCollectionOp,
    MirFunction, MirFunctionId, MirIntrinsic, MirKernelResource, MirKernelResourceAccess,
    MirKernelResourceKind, MirKernelResourceRole, MirKernelShaderStage, MirKernelSignature,
    MirLocalId, MirStatementKind, MirTensorStorageLayout, MirType, ObservationCadence,
    ObservationFact, SemanticValue, SemanticValueId, SemanticValueOrigin, TypeTable, ValidatedMir,
    ValueBinding, ValueGeneration, VecDeque,
};

// ---------------------------------------------------------------------------
// Device-program constructor
// ---------------------------------------------------------------------------

/// The logical entry name for a kernel function.
///
/// The target-neutral entry is the Faber function name when the function
/// carries one (the S1-3 emitters name the Metal kernel entry after it; the
/// CUDA PTX symbol is separate and carried via [`CudaKernelIdentity`]).
fn kernel_entry_name(function: &MirFunction, interner: &Interner) -> String {
    function
        .name
        .map(|symbol| interner.resolve(symbol))
        .map(str::to_owned)
        .unwrap_or_else(|| format!("kernel{}", function.id.0))
}

/// Whether a device-resident function's body CONSTRUCTS tensor shapes
/// (`magnitudines()` shape lists and `crea` fills over a vacua seed) — the
/// exact surface the S5-U4 static-shape fold
/// ([`radix_mir::static_shape_fold::fold_static_shapes`]) rewrites to
/// constant dims + scalar-broadcast elementwise fills. A library-backed
/// `train_step` body (the gradus `train_step_2x2`/`train_step_4x4` surface)
/// is the first consumer: without the fold the shape ops carry no kernel
/// plan and the constructor fails the decomposition closed.
pub(super) fn function_has_shape_construction(function: &MirFunction) -> bool {
    function
        .blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .any(|statement| {
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                return false;
            };
            matches!(
                call.intrinsic,
                MirIntrinsic::Collection(
                    MirCollectionOp::TensorShape | MirCollectionOp::TensorCreate
                )
            )
        })
}

/// The transformer recipe op of a subchain body, when it carries exactly one
/// (S6-P1 statement scan — mirrors the plan pass's typed-fact scan and the
/// emitters' `transformer_recipe_op`: a body mixing distinct transformer
/// recipes cannot emit as one kernel and fails closed upstream at plan
/// resolution).
fn transformer_recipe_op(function: &MirFunction) -> Option<MirCollectionOp> {
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                continue;
            };
            let MirIntrinsic::Collection(op) = call.intrinsic else {
                continue;
            };
            if is_transformer_recipe_op(op) {
                return Some(op);
            }
        }
    }
    None
}

/// The param-name for a buffer slot, when the kernel function names the
/// source local (diagnostics + the `[device] inputs` manifest keys).
///
/// `source` is the ORIGINAL device-resident function the slot's value comes
/// from: for a decomposition subchain the synthetic function's params carry
/// no names, so the name resolves through the source function's params and
/// locals — never from the subchain slice (S5-U1b/U5).
fn buffer_slot_name(
    source: &MirFunction,
    interner: &Interner,
    resource: &MirKernelResource,
) -> String {
    if let Some(local) = resource.source_local {
        if let Some(name) = source
            .params
            .iter()
            .find(|param| param.local == local)
            .and_then(|param| param.name)
            .and_then(|symbol| safe_interner_name(interner, symbol))
        {
            return name;
        }
        if let Some(name) = source
            .locals
            .iter()
            .find(|entry| entry.id == local)
            .and_then(|entry| entry.name)
            .and_then(|symbol| safe_interner_name(interner, symbol))
        {
            return name;
        }
    }
    match resource.role {
        radix_mir::abi::MirKernelResourceRole::Input => {
            format!("input_{}", resource.binding)
        }
        radix_mir::abi::MirKernelResourceRole::Output => {
            format!("output_{}", resource.binding)
        }
    }
}

/// Resolve a symbol name safely: an uninterned synthetic symbol (the
/// reverse-AD residual/upstream placeholders of a generated companion)
/// falls back to `None` instead of indexing past the interner's string
/// table — the caller then uses the role-based fallback name (S3-A2).
fn safe_interner_name(interner: &Interner, symbol: radix::lexer::Symbol) -> Option<String> {
    if (symbol.0 as usize) < interner.strings().len() {
        Some(interner.resolve(symbol).to_owned())
    } else {
        None
    }
}

/// One unified buffer's program-level identity facts plus the carried
/// semantic-value fact it is minted from (F1).
///
/// The S2-5 cross-kernel wiring still unifies one kernel's output with
/// another kernel's input by the declared logical identity (name + shape) —
/// that wiring is the only carried fact connecting independent kernels — but
/// the unification is now guarded by [`unify_roles`]: two independent
/// producers of the same named output never alias (F1). The buffer's
/// semantic VALUE identity is minted from the carried MIR fact of the slot
/// that first references it (a MIR local, a host input, or a distinct
/// synthetic label) — never from the buffer id, binding position, or
/// declaration coincidence.
///
/// The resource-state axes (F5) ride here as **independent facts** gathered
/// from the slot access pattern across the whole program: `written` and
/// `consumed` feed the allocation-lifetime and initialization decisions in
/// pass 2 — no axis is derived from the role or from another axis.
struct ProgramBuffer {
    id: BufferId,
    name: String,
    element_ty: MirType,
    element_count: u64,
    role: BufferRole,
    /// Any kernel slot writes the buffer (Write/ReadWrite) — the independent
    /// fact behind the lifetime and initialization axes.
    written: bool,
    /// Any kernel slot reads the buffer (Read) — the independent fact that
    /// distinguishes a step-local intermediate from a final observation.
    consumed: bool,
    /// Any kernel slot accesses the buffer in place (ReadWrite).
    readwrite: bool,
    /// A trainable parameter of a `RepeatingStep` training program (S5-U5):
    /// `PerProgram` persistent state with HostProvided init, copied in exactly
    /// once at session creation (the step's once-init contract). The flag is
    /// the independent fact behind the lifetime and initialization axes — the
    /// access pattern alone would decide these differently (a readwrite
    /// intermediate is ZeroFill, a written+consumed buffer is `PerStep`).
    param: bool,
    /// A backward companion's gradient output slot (S5-U5): a per-step
    /// scratch value feeding the optimizer step — never an observation point
    /// and never persistent state, even when no step kernel consumes it (a
    /// frozen-input gradient like `gx`).
    gradient: bool,
    /// The carried MIR/value fact the buffer's semantic value identity is
    /// minted from (F1).
    origin: SemanticValueOrigin,
}

impl ProgramBuffer {
    /// The S2-5 wiring key: same declared logical name and same shape. The
    /// caller additionally guards on [`unify_roles`] before unifying, so a
    /// name/shape coincidence alone never aliases two unrelated values.
    fn matches(&self, name: &str, element_ty: MirType, element_count: u64) -> bool {
        self.name == name && self.element_ty == element_ty && self.element_count == element_count
    }
}

/// Whether a slot with program role `next` may join an existing buffer with
/// role `existing` under the S2-5 wiring (same logical name + shape).
///
/// The wiring is a data-flow continuation — a shared input, a
/// producer→consumer intermediate, or an in-place accumulation slot. Two
/// independent producers of the same named output NEVER unify (F1: two
/// unrelated same-name/same-shape values are distinct, never aliased).
fn unify_roles(existing: BufferRole, next: BufferRole) -> bool {
    match (existing, next) {
        (BufferRole::Input, BufferRole::Input) => true,
        (BufferRole::InOut, _) | (_, BufferRole::InOut) => true,
        (BufferRole::Output, BufferRole::Input) | (BufferRole::Input, BufferRole::Output) => true,
        (BufferRole::Output, BufferRole::Output) => false,
    }
}

/// The carried fact a buffer's semantic value identity is minted from (F1):
/// the MIR local the slot's value flows from when the slot names one (stable
/// under rename — the identity never follows the diagnostic name), a
/// host-provided input for an anonymous read slot, or a distinct synthetic
/// label for an anonymous writer slot (a tuple-output gradient).
fn value_origin(
    function: &MirFunction,
    resource: &MirKernelResource,
    buffer_id: BufferId,
) -> SemanticValueOrigin {
    if let Some(local) = resource.source_local {
        SemanticValueOrigin::MirLocal {
            function: function.id,
            local: local.0,
        }
    } else if resource.role == MirKernelResourceRole::Input {
        SemanticValueOrigin::HostInput
    } else {
        SemanticValueOrigin::Synthetic {
            label: format!("buffer-{}", buffer_id.0),
        }
    }
}

/// Merge an ABI-derived role onto a buffer's program-level role under one id.
fn merge_buffer_roles(previous: BufferRole, next: BufferRole) -> BufferRole {
    match (previous, next) {
        (BufferRole::Input, BufferRole::Input) => BufferRole::Input,
        (BufferRole::Output, BufferRole::Output) => BufferRole::Output,
        _ => BufferRole::InOut,
    }
}

/// Merge a slot's facts onto an existing unified buffer under one id: the
/// role merge (a trainable-parameter slot forces InOut), the independent
/// resource-state axes (`written` / `consumed` / `readwrite`), and the param
/// flag. Shared by the carried local-identity wiring (S5-U1) and the
/// name+shape wiring — the two joins must never drift apart.
fn merge_slot_facts(
    entry: &mut ProgramBuffer,
    role: BufferRole,
    resource: &MirKernelResource,
    is_param_input: bool,
) {
    entry.role = if is_param_input {
        BufferRole::InOut
    } else {
        merge_buffer_roles(entry.role, role)
    };
    entry.written |= matches!(
        resource.access,
        MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
    );
    entry.consumed |= resource.access == MirKernelResourceAccess::Read;
    entry.readwrite |= resource.access == MirKernelResourceAccess::ReadWrite;
    entry.param |= is_param_input;
}

/// Order the launch sequence to follow the carried producer/consumer
/// dependency graph (F3) — never kernel declaration order. Producers always
/// precede their consumers; the launch ids are carried facts and do not
/// change, only their position in the execution sequence does.
///
/// A `RepeatingStep` program's trainable-parameter edges are EXCLUDED: the
/// parameter is persistent state — the step writes the value the NEXT step
/// reads — so the write→read edge within one step's sequence would form a
/// cycle. The parameter's current value is the once-init'd (or previous-step)
/// state, not a producer inside this step.
///
/// # Errors
/// Fail-closed when the dependency graph contains a cycle (a launch sequence
/// that follows it would never terminate).
fn dependency_ordered_launches(
    program: &DeviceProgram,
    excluded_buffers: &[BufferId],
) -> Result<Vec<LaunchUnit>, Vec<Diagnostic>> {
    let edges = program
        .buffer_registry()
        .data_flow_pairs()
        .into_iter()
        .filter(|edge| !excluded_buffers.contains(&edge.buffer))
        .collect::<Vec<_>>();
    let mut indegree: HashMap<LaunchId, usize> = HashMap::new();
    let mut dependents: HashMap<LaunchId, Vec<LaunchId>> = HashMap::new();
    for launch in &program.launches {
        indegree.insert(launch.id, 0);
    }
    for edge in &edges {
        if edge.producer == edge.consumer {
            continue;
        }
        *indegree.entry(edge.consumer).or_insert(0) += 1;
        dependents
            .entry(edge.producer)
            .or_default()
            .push(edge.consumer);
    }
    // Kahn's algorithm seeded in declaration order: independent launches keep
    // their declaration order, and any launch that depends on another is
    // emitted only after every producer has run.
    let mut ready: VecDeque<LaunchId> = program
        .launches
        .iter()
        .filter(|launch| indegree.get(&launch.id).copied().unwrap_or(0) == 0)
        .map(|launch| launch.id)
        .collect();
    let mut ordered: Vec<LaunchId> = Vec::with_capacity(program.launches.len());
    while let Some(id) = ready.pop_front() {
        ordered.push(id);
        if let Some(consumers) = dependents.get(&id) {
            for &consumer in consumers {
                if let Some(count) = indegree.get_mut(&consumer) {
                    *count -= 1;
                    if *count == 0 {
                        ready.push_back(consumer);
                    }
                }
            }
        }
    }
    if ordered.len() != program.launches.len() {
        return Err(vec![device_diag(
            "launch order",
            "the carried producer/consumer dependency graph contains a cycle; the launch sequence cannot follow it",
        )]);
    }
    // Reorder the launch units by their position in the carried topological
    // order (launch ids are carried facts; only their sequence position
    // changes).
    let mut position: HashMap<LaunchId, usize> = HashMap::new();
    for (index, id) in ordered.iter().enumerate() {
        position.insert(*id, index);
    }
    let mut sorted: Vec<LaunchUnit> = program.launches.to_vec();
    sorted.sort_by_key(|launch| position.get(&launch.id).copied().unwrap_or(usize::MAX));
    Ok(sorted)
}

/// Mint the carried semantic facts (F1–F6) of a materialized program, in
/// execution order.
///
/// - values + bindings: one semantic value per unified buffer, minted from
///   the carried origin facts (F1);
/// - generations: every logical write in the ordered launch sequence is an
///   explicit new generation; a read consumes the current generation (F2);
/// - roots + dependencies: the carried graph the host schedules from (F3);
///   a trainable parameter's edges are persistent-state facts — the write
///   feeds the NEXT step, never a producer inside this step — so they are
///   excluded (the same exclusion the launch ordering applies);
/// - relations: the lossless primal/companion rows from the carrier (F4);
/// - initializations + observations: the independent axis facts (F5/F6). A
///   trainable parameter is HostProvided (copied in exactly once at session
///   creation), never zero-filled despite its InOut access.
fn semantic_facts(
    program: &DeviceProgram,
    buffers: &[ProgramBuffer],
    companions: &radix_mir::device::MirCompanionMap,
) -> DeviceSemantics {
    let values: Vec<SemanticValue> = buffers
        .iter()
        .map(|buffer| SemanticValue {
            id: SemanticValueId(buffer.id.0),
            name: buffer.name.clone(),
            element_ty: buffer.element_ty,
            element_count: buffer.element_count,
            origin: buffer.origin.clone(),
        })
        .collect();
    let bindings: Vec<ValueBinding> = buffers
        .iter()
        .map(|buffer| ValueBinding {
            value: SemanticValueId(buffer.id.0),
            buffer: buffer.id,
        })
        .collect();

    // Generations in EXECUTION order: a later write is a new generation,
    // never another producer of the same one (F2). A read-only buffer's
    // reads consume its initial (host-provided) state generation 1.
    let mut produced: HashMap<BufferId, u32> =
        buffers.iter().map(|buffer| (buffer.id, 0)).collect();
    let mut generations: Vec<ValueGeneration> = Vec::new();
    for launch in &program.launches {
        let Some(kernel) = program.kernels.get(launch.kernel_index) else {
            continue;
        };
        for resource in &kernel.resources {
            let buffer = resource.buffer.id;
            match resource.access {
                MirKernelResourceAccess::Read => {}
                MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite => {
                    let next = produced.get(&buffer).copied().unwrap_or(0) + 1;
                    produced.insert(buffer, next);
                    generations.push(ValueGeneration {
                        value: SemanticValueId(buffer.0),
                        generation: next,
                        element_ty: resource.version.element_ty,
                        element_count: resource.version.element_count,
                        produced_by: launch.id,
                    });
                }
            }
        }
    }

    let dependencies: Vec<DependencyEdge> = program
        .buffer_registry()
        .data_flow_pairs()
        .into_iter()
        // Persistent trainable-parameter state: the step's write feeds the
        // NEXT step's read — excluded from this step's dependency graph.
        .filter(|pair| {
            !buffers
                .iter()
                .any(|buffer| buffer.param && buffer.id == pair.buffer)
        })
        .map(|pair| DependencyEdge {
            producer: pair.producer,
            consumer: pair.consumer,
            buffer: pair.buffer,
            version: pair.version,
        })
        .collect();
    let roots: Vec<LaunchId> = program
        .launches
        .iter()
        .filter(|launch| !dependencies.iter().any(|edge| edge.consumer == launch.id))
        .map(|launch| launch.id)
        .collect();
    let relations: Vec<LosslessMirCompanionEntry> = companions.iter().cloned().collect();
    let initializations: Vec<InitializationFact> = buffers
        .iter()
        .map(|buffer| {
            let policy = if buffer.param {
                // S5-U5 / param-identity.md: a trainable parameter's storage
                // is HostProvided — copied in exactly once at session
                // creation (the RepeatingStep once-init contract). Its InOut
                // access alone would decide ZeroFill; the param identity is
                // the independent fact.
                InitializationPolicy::HostProvided
            } else if buffer.readwrite {
                InitializationPolicy::ZeroFill
            } else if !buffer.written {
                InitializationPolicy::HostProvided
            } else {
                InitializationPolicy::KernelInitialized
            };
            InitializationFact {
                buffer: buffer.id,
                policy,
            }
        })
        .collect();
    let observations: Vec<ObservationFact> = program
        .results
        .iter()
        .map(|result| ObservationFact {
            buffer: result.buffer.id,
            version: result.version.version,
            at_launch: result.produced_by,
        })
        .collect();

    DeviceSemantics {
        values,
        bindings,
        generations,
        roots,
        dependencies,
        relations,
        initializations,
        observations,
    }
}

/// Construct the common device program for a lowered package.
///
/// Scans the validated package MIR for `@ nucleum` compute kernels
/// (`shader_stage == Compute`) and composes one ordered [`DeviceProgram`]
/// whose kernel units carry the typed plan, typed storage-buffer resources,
/// and derived launch plans, plus the frozen [`DeviceSemantics`] that rides
/// alongside it (Stage 3R F1–F7). Every field is a program fact from the ABI
/// signature and the shared plan pass — never inferred from emitted text
/// (A3). A package with no compute kernels yields `None` (no device payload).
///
/// **Faithful materialization (U4):**
/// - semantic VALUE identity (F1) is minted from carried MIR facts —
///   [`SemanticValueOrigin::MirLocal`] from the slot's source local (stable
///   under rename), `HostInput` for anonymous host-provided reads, or a
///   distinct `Synthetic` label for anonymous writer slots. Two unrelated
///   same-name/same-shape values never alias (the unification guard
///   [`unify_roles`] rejects a second independent producer of a named
///   output);
/// - every logical write advances an explicit value generation (F2) in the
///   ordered launch sequence;
/// - the launch SEQUENCE follows the carried producer/consumer dependency
///   graph (F3), never kernel declaration order;
/// - results name declared observation points only (F6): the program's
///   Output-role buffers. Writable intermediates and persistent state are
///   never results merely because they are writable;
/// - the lossless primal/companion relation (F4) rides from the owned
///   [`radix_mir::device::MirCompanionMap`] carrier into the semantics.
///
/// **Companion path (S3-A2, THE SPINE):** the constructor also selects the
/// generated companions of device-resident primals through the owned
/// [`radix_mir::device::MirCompanionMap`] carrier — NOT only
/// `shader_stage == Compute`. Each carried companion's tuple gradient return
/// lowers through the multi-output ABI (S3-A1) into distinct output
/// resources (N gradient outputs bind to N distinct slots), its kernel is
/// ordered AFTER the forward kernels, and its buffers join the same S2-5
/// unification (the companion reads the primal's device-resident buffers by
/// identity). Placement is decided here (A5): a companion of a
/// device-resident primal joins the same `DeviceProgram`; generated AIR
/// stays pure (the purity ledger is untouched).
///
/// # Errors
/// Fail-closed [`Diagnostic`]s when a kernel's ABI signature or plan cannot
/// be derived, a storage buffer has no coherent program role, a carried
/// companion is missing from the lowered MIR, the launch sequence cannot
/// follow the carried dependency graph, or the resulting program + carried
/// semantics fail [`DeviceProgram::validate_with_semantics`].
pub(crate) fn device_program_for_lowered(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    companions: &radix_mir::device::MirCompanionMap,
    declared_steps: u32,
) -> Result<Option<(DeviceProgram, DeviceSemantics, u32)>, Vec<Diagnostic>> {
    let program = validated.program();
    let training = training_plan_facts(validated, interner, companions, Some(declared_steps))?;

    let kernel_functions: Vec<&MirFunction> = program
        .functions
        .iter()
        .filter(|function| function.shader_stage == Some(MirKernelShaderStage::Compute))
        .collect();
    if kernel_functions.is_empty() {
        return Ok(None);
    }

    /// One kernel's build: signature/plan facts plus its resource slots
    /// (buffer ids resolved by the unification pass).
    struct KernelBuild {
        function: MirFunctionId,
        entry: String,
        plan: CollectionKernelPlan,
        launch: KernelLaunchPlan,
        resources: Vec<ResourceBuild>,
    }
    /// One storage-buffer slot: binding facts + the unified buffer id.
    struct ResourceBuild {
        group: u32,
        binding: u32,
        access: MirKernelResourceAccess,
        buffer_id: BufferId,
        element_ty: MirType,
        element_count: u64,
    }

    // Pass 1: derive signatures/plans and unify cross-kernel buffer identity.
    // One kernel builder serves the forward kernels, the S3-A2 companion
    // kernels, and the S5-U5 train_step kernels — every kernel participates
    // in the same S2-5 unification, so a companion reads its primal's
    // device-resident buffers by identity and a train_step updates the
    // parameter buffers in place.
    let mut buffers: Vec<ProgramBuffer> = Vec::new();
    let mut next_buffer_id = 1u32;
    let mut builds: Vec<KernelBuild> = Vec::new();
    // (step function, param local) → (param position, update output slot)
    // for the trainable params of the package's training loop.
    let mut param_updates: BTreeMap<(MirFunctionId, MirLocalId), (u32, u32)> = BTreeMap::new();
    if let Some(training) = &training {
        for param in &training.trainable {
            if let Some(function) = program
                .functions
                .iter()
                .find(|function| function.id == param.step_function)
            {
                if let Some(param_local) = function
                    .params
                    .get(param.param_position as usize)
                    .map(|param| param.local)
                {
                    param_updates.insert(
                        (param.step_function, param_local),
                        (param.param_position, param.update_output),
                    );
                }
            }
        }
    }
    // Gradient-flow links (S5-U5): the train_step input position consuming
    // each trainable param's gradient, and the companion result-tuple slot
    // producing it. `gradient_buffers` is filled while the companion kernels
    // are built (the companion precedes the train_step) and consulted while
    // the train_step kernels are built — the backward → train_step edge.
    let mut gradient_links: BTreeMap<(MirFunctionId, u32), (u32, u32)> = BTreeMap::new();
    let mut companion_gradient_slots: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    if let Some(training) = &training {
        for link in &training.gradients {
            gradient_links.insert(
                (link.step_function, link.gradient_position),
                (link.companion.0, link.gradient_slot),
            );
        }
    }
    // EVERY selected-input gradient slot of a device-resident companion is a
    // per-step scratch buffer — including a frozen input's gradient (no step
    // consumes it, but it is still a gradient, never an observation point).
    for entry in companions.iter() {
        if !entry.device_resident {
            continue;
        }
        for selected in &entry.selected_inputs {
            companion_gradient_slots
                .entry(entry.companion.0)
                .or_default()
                .insert(selected.gradient_slot);
        }
    }
    let mut gradient_buffers: BTreeMap<(u32, u32), BufferId> = BTreeMap::new();
    let mut build_kernel = |function: &MirFunction| -> Result<(), Vec<Diagnostic>> {
        // The entry name: the function's logical entry (the source name),
        // with a `__N` suffix when a multi-recipe body decomposes into
        // several subchain kernels (distinct Metal entries / CUDA logical
        // entries per kernel).
        //
        // S5-U4 static-shape fold: a device-resident body that CONSTRUCTS
        // shapes (`magnitudines()` / `crea`) folds to constant dims +
        // scalar-broadcast fills BEFORE any plan/signature derivation —
        // the shape ops carry no kernel plan and would otherwise fail the
        // decomposition closed. The first consumer is the library-backed
        // `train_step` body (gradus `train_step_2x2`/`train_step_4x4`) the
        // S5-U5 training-plan path materializes; the fold is a pure rewrite
        // (same function id, params, and semantic types), so bodies without
        // shape construction pass through unchanged.
        let folded: Option<MirFunction>;
        let kernel_source: &MirFunction = if function_has_shape_construction(function) {
            let outcome =
                radix_mir::static_shape_fold::fold_static_shapes(function, validated.validation())
                    .map_err(|error| vec![device_diag("shape fold", error.message)])?;
            folded = Some(outcome.function);
            folded.as_ref().ok_or_else(|| {
                vec![device_diag(
                    "shape fold",
                    "folded function missing after fold admission",
                )]
            })?
        } else {
            function
        };
        let base_entry = kernel_entry_name(kernel_source, interner);
        // The source function's return-tuple field locals (in full-tuple
        // order) — the discriminator for the S5-U5 update-output aliasing:
        // an output slot aliases a trainable parameter only when it IS the
        // full result-tuple slot for that parameter's update. The source
        // function (not the folded kernel_source) is the authoritative
        // return shape; the decomposition's subchain output slots carry the
        // same source local ids (the fold is a pure statement rewrite).
        let return_tuple_locals = tuple_return_locals(function);
        // The decomposed path records each subchain output slot's buffer id
        // (subchain index, output-slot index) while the subchains are built,
        // so the D-6 fill can resolve the companion's FULL result-tuple
        // field producers after all subchains are emitted (a subchain's
        // local output-slot index is the forward-save set, never the
        // full-tuple index the gradient slots index).
        let mut subchain_output_buffer_ids: BTreeMap<(usize, u32), BufferId> = BTreeMap::new();
        let mut emit_subchain = |synthetic: &MirFunction,
                                 signature: &MirKernelSignature,
                                 plan: CollectionKernelPlan,
                                 entry: String,
                                 param_updates: &BTreeMap<
            (MirFunctionId, MirLocalId),
            (u32, u32),
        >,
                                 subchain_index: Option<usize>,
                                 return_tuple_locals: &[MirLocalId]|
         -> Result<(), Vec<Diagnostic>> {
            let mut resources: Vec<ResourceBuild> = Vec::new();
            // The trainable param local → its registered buffer id, filled
            // as the signature's input slots are processed (inputs precede
            // outputs in the ABI), so the update output slots can alias it.
            let mut param_buffer_by_local: BTreeMap<MirLocalId, BufferId> = BTreeMap::new();
            for resource in signature
                .resources()
                .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
            {
                let role =
                    BufferRole::from_abi_role(resource.role, resource.access).ok_or_else(|| {
                        vec![device_diag(
                            "buffer role",
                            format!(
                            "storage buffer binding {} has no coherent program role ({:?} {:?})",
                            resource.binding, resource.role, resource.access
                        ),
                        )]
                    })?;
                // A trainable parameter's read slot: the buffer is persistent
                // InOut state with HostProvided init (param-identity.md) —
                // the program role is InOut regardless of the slot's ABI role.
                let is_param_input = resource.role == MirKernelResourceRole::Input
                    && resource.source_local.is_some_and(|local| {
                        param_updates.contains_key(&(kernel_source.id, local))
                    });
                // The output-slot tuple index (the ABI's first output is
                // element 0, each extra output follows).
                let output_index = if resource.role == MirKernelResourceRole::Output {
                    Some(output_slot_index(signature, resource))
                } else {
                    None
                };
                // An update output slot: the slot's tuple index decides which
                // parameter it updates (the loop re-binds that parameter's
                // entry-local from this tuple element). The alias applies ONLY
                // when the output slot IS the full result-tuple slot for that
                // parameter's update — an intermediate subchain's local output
                // slot (the scaled-grad forward-save set, or an elementwise
                // body's pre sub-slice) never carries a full-tuple index and
                // must never be written into the parameter buffer: a
                // decomposed train_step's pre subchain (fills + scaled-grad
                // muls, reading the wq/bq fill receivers) would otherwise
                // write its scaled-grad `swq` INTO `wq`, and the next
                // subchain's `swq` read would mint a fresh PerProgram input
                // (the bert-tiny once-init failure).
                let alias_param = match output_index {
                    Some(output_index) => {
                        let full_tuple_slot = resource.source_local.is_some_and(|local| {
                            return_tuple_locals.get(output_index as usize).copied() == Some(local)
                        });
                        if full_tuple_slot {
                            param_updates
                                .iter()
                                .find(|((_, _), (_, update_output))| *update_output == output_index)
                                .and_then(|((_, param_local), _)| {
                                    param_buffer_by_local.get(param_local).copied()
                                })
                        } else {
                            None
                        }
                    }
                    None => None,
                };
                // A gradient input slot of a train_step: the gradient buffer
                // the companion's result-tuple slot writes — the explicit
                // backward → train_step data-flow connection (the gradient
                // identity rides the companion relation, never a name).
                let gradient_alias = if resource.role == MirKernelResourceRole::Input {
                    resource.source_local.and_then(|local| {
                        let position = kernel_source
                            .params
                            .iter()
                            .position(|param| param.local == local);
                        position.and_then(|position| {
                            gradient_links
                                .get(&(
                                    kernel_source.id,
                                    u32::try_from(position).unwrap_or(u32::MAX),
                                ))
                                .and_then(|(companion, slot)| {
                                    gradient_buffers.get(&(*companion, *slot)).copied()
                                })
                        })
                    })
                } else {
                    None
                };
                let name = buffer_slot_name(kernel_source, interner, resource);
                // S2-5 wiring: the same logical buffer appears at this kernel
                // too. The role guard ([`unify_roles`]) keeps a name/shape
                // coincidence from aliasing two unrelated values — a second
                // independent producer of a named output mints a distinct buffer
                // and a distinct semantic value (F1).
                let buffer_id = if let Some(alias) = alias_param {
                    alias
                } else if let Some(alias) = gradient_alias {
                    // The gradient input consumes the companion's gradient
                    // output buffer — the buffer becomes an InOut per-step
                    // intermediate (written by the companion, read by the
                    // step), never an observation point.
                    if let Some(entry) = buffers.iter_mut().find(|entry| entry.id == alias) {
                        entry.role = merge_buffer_roles(entry.role, BufferRole::InOut);
                        entry.consumed = true;
                    }
                    alias
                } else if let Some(entry) = buffers.iter_mut().find(|entry| {
                    // S5-U1 forward-save identity: when the slot carries a
                    // source local, the (function, local) origin fact joins
                    // the producing and consuming decomposition subchains
                    // (they share the source function's local tables) even
                    // when the role-based fallback names differ (`input_N`
                    // vs `output_M`). The role guard ([`unify_roles`]) keeps
                    // the join honest exactly as it guards the name+shape
                    // wiring — two independent writers of the same local
                    // never alias.
                    let same_local_identity = resource.source_local.is_some_and(|local| {
                        matches!(
                            entry.origin,
                            SemanticValueOrigin::MirLocal {
                                function: origin_function,
                                local: origin_local,
                            } if origin_function == kernel_source.id && origin_local == local.0
                        )
                    });
                    same_local_identity && unify_roles(entry.role, role)
                }) {
                    // A trainable parameter's program role is InOut regardless
                    // of the merged slot roles (the param identity is a
                    // carried fact, never derived from one slot's ABI role).
                    merge_slot_facts(entry, role, resource, is_param_input);
                    entry.id
                } else if let Some(entry) = buffers.iter_mut().find(|entry| {
                    // A2-3 (D-5): the name+shape wiring is a data-flow
                    // continuation, never a second independent writer. A
                    // consumer slot joins any same-name/same-shape buffer; a
                    // producer slot joins only a buffer with NO writer yet
                    // (the reverse-declared producer→consumer chain, whose
                    // initial state is host-provided — e.g. the collige/
                    // recollige `medius` chain). Once a buffer has a writer, a
                    // producer must join by semantic identity (the branch
                    // above), a param alias, or a gradient alias; a name+shape
                    // producer join would alias two independent values whenever
                    // fallback names (`output_N` / `input_N`) and shapes
                    // coincide (the square MLP makes every shape equal) — the
                    // false second writer records write-after-write producers
                    // and backward launch edges (the D-4 launch-order cycle).
                    entry.matches(&name, resource.element_ty, resource.element_count)
                        && unify_roles(entry.role, role)
                        && (resource.access == MirKernelResourceAccess::Read || !entry.written)
                }) {
                    // A trainable parameter's program role is InOut regardless
                    // of the merged slot roles (the param identity is a
                    // carried fact, never derived from one slot's ABI role).
                    merge_slot_facts(entry, role, resource, is_param_input);
                    entry.id
                } else {
                    let id = BufferId(next_buffer_id);
                    next_buffer_id += 1;
                    buffers.push(ProgramBuffer {
                        id,
                        name,
                        element_ty: resource.element_ty,
                        element_count: resource.element_count,
                        role: if is_param_input {
                            BufferRole::InOut
                        } else {
                            role
                        },
                        written: matches!(
                            resource.access,
                            MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                        ),
                        consumed: resource.access == MirKernelResourceAccess::Read,
                        readwrite: resource.access == MirKernelResourceAccess::ReadWrite,
                        param: is_param_input,
                        gradient: false,
                        origin: value_origin(kernel_source, resource, id),
                    });
                    id
                };
                if is_param_input {
                    if let Some(local) = resource.source_local {
                        param_buffer_by_local.insert(local, buffer_id);
                    }
                }
                // Record the companion's gradient output buffers as they are
                // minted (the train_step kernels built afterwards alias them);
                // a companion gradient slot is a per-step scratch value,
                // never an observation point.
                //
                // Whole-function path (`subchain_index` is None): the
                // kernel's output tuple IS the companion's full result tuple,
                // so the output-slot index is the full result-tuple index the
                // gradient slots index. Decomposition path: a subchain's
                // local output-slot index is NOT the full-tuple index (it is
                // the forward-save set), so the gradient slots are marked
                // after ALL subchains from the decomposition's return-tuple
                // field bindings (D-6) — here only the produced buffer id is
                // recorded for that fill.
                if let (Some(source), Some(output_index)) = (kernel_source.source, output_index) {
                    if subchain_index.is_none()
                        && companion_gradient_slots
                            .get(&source.0)
                            .is_some_and(|slots| slots.contains(&output_index))
                    {
                        if let Some(entry) = buffers.iter_mut().find(|entry| entry.id == buffer_id)
                        {
                            entry.gradient = true;
                        }
                        gradient_buffers.insert((source.0, output_index), buffer_id);
                    }
                    if let Some(subchain_index) = subchain_index {
                        subchain_output_buffer_ids
                            .insert((subchain_index, output_index), buffer_id);
                    }
                }
                resources.push(ResourceBuild {
                    group: resource.group,
                    binding: resource.binding,
                    access: resource.access,
                    buffer_id,
                    element_ty: resource.element_ty,
                    element_count: resource.element_count,
                });
            }
            // The launch binds buffers in binding order (inputs first, output
            // last), matching the emitted kernel's buffer indices / param
            // order.
            resources.sort_by_key(|resource| (resource.binding, resource.group));
            builds.push(KernelBuild {
                function: kernel_source.id,
                entry,
                plan,
                launch: KernelLaunchPlan::from_signature_and_function(signature, synthetic),
                resources,
            });
            Ok(())
        };

        // Whole-function path: a single-recipe or elementwise-only body
        // carries ONE kernel from its full ABI signature — the S2-4/S3-A1
        // behavior unchanged. A multi-recipe body (the S5-U1 decomposition
        // contract) falls through to the subchain path.
        let whole_signature =
            MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
                kernel_source,
                validated.validation(),
                interner,
            );
        if let Ok(whole_signature) = &whole_signature {
            if let Ok(Some(plan)) =
                kernel_plan_for_function(kernel_source, whole_signature, validated.validation())
            {
                return emit_subchain(
                    kernel_source,
                    whole_signature,
                    plan,
                    base_entry,
                    &param_updates,
                    None,
                    return_tuple_locals.as_deref().unwrap_or(&[]),
                );
            }
        }

        // Decomposition path (S5-U1/U5): split the body at recipe boundaries
        // into subchain kernels, each carrying exactly one recipe (or an
        // elementwise-only body), with the shared contract-shaped emission
        // signature for recipe subchains and the full ABI synthesis for
        // elementwise-only subchains.
        let decomposition = decompose_kernel_function(kernel_source, validated.validation())
            .map_err(|error| vec![device_diag("decomposition", error.message)])?;
        // D-2b-2: pre-intern the subchain output-tuple return types (D-2b-1)
        // in the table the subchain derivation uses — `subchain_function`
        // holds only `&TypeTable` and fails closed when a multi-output
        // subchain's tuple type is missing. A restored superset table (the
        // tuple element types come from the source function's locals, shared
        // with the original table) resolves every other typed fact
        // identically.
        let mut subchain_types = TypeTable::from_snapshot(validated.validation().types.snapshot())
            .map_err(|error| vec![device_diag("subchain types", error)])?;
        decomposition.intern_output_tuple_types(kernel_source, &mut subchain_types);
        let subchain_validation = validated.validation().with_types(&subchain_types);
        for (subchain_index, subchain) in decomposition.subchains.iter().enumerate() {
            let synthetic = decomposition.subchain_function(
                kernel_source,
                subchain_index,
                subchain_validation.types,
            );
            let contract = collection_op_contract(&synthetic, &subchain_validation)
                .map_err(|error| vec![device_diag("plan", error.message)])?;
            let signature = match &contract {
                Some(contract) => subchain_signature_for_emission_with_source(
                    &synthetic,
                    contract,
                    &subchain_validation,
                    &subchain.outputs,
                    Some(kernel_source),
                )
                .map_err(|error| vec![device_diag("signature", error.message)])?,
                None => {
                    // S6-P1: a transformer-recipe subchain (`TensorSumAxis` /
                    // `TensorSoftmax` / `TensorLayerNorm`) has no
                    // `CollectionOpContract` variant — the ABI's generic
                    // return-buffer kernel is elementwise-only and cannot
                    // carry a genuinely size-changing reduction output (an
                    // axis reduction's `[M,N] → [N]` fails the return-buffer
                    // element-count law) or an affine-pair LayerNorm — so the
                    // signature dispatch goes through the P1 seam
                    // (`transformer_subchain_signature_for_emission`, the
                    // transformer-shaped recipe resources plus the subchain's
                    // data-flow inputs, e.g. the LN affine tensors).
                    // Mirrors the emitters' consumption of the same seam
                    // (Metal S6-P2, NVVM S6-P3). Elementwise-only subchains
                    // keep the full ABI synthesis. The SOURCE function rides
                    // both seam calls so a TensorSumAxis-produced data-flow
                    // input binds at the reduced count (the widened d8518267
                    // seam repair) — the same count all consumers derive.
                    if let Some(op) = transformer_recipe_op(&synthetic) {
                        transformer_subchain_signature_for_emission_with_source(
                            &synthetic,
                            op,
                            &subchain_validation,
                            &subchain.outputs,
                            Some(kernel_source),
                        )
                        .map_err(|error| vec![device_diag("signature", error.message)])?
                    } else {
                        MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
                            &synthetic,
                            &subchain_validation,
                            interner,
                        )
                        .map_err(|error| vec![device_diag("signature", error.message)])?
                    }
                }
            };
            let entry = if decomposition.subchains.len() == 1 {
                base_entry.clone()
            } else {
                format!("{base_entry}__{subchain_index}")
            };
            emit_subchain(
                &synthetic,
                &signature,
                subchain.plan.clone(),
                entry,
                &param_updates,
                Some(subchain_index),
                return_tuple_locals.as_deref().unwrap_or(&[]),
            )?;
        }
        // D-6 gradient-slot → buffer mapping: the companion's gradient slots
        // index the FULL result-tuple (a selected input's param position),
        // which the LAST subchain's return-tuple Construct binds to locals.
        // The train_step gradient reads alias the producing subchains'
        // gradient buffers (backward → train_step device-to-device edge); the
        // return-tuple local itself is never a kernel output (the ABI
        // flattens tuple returns into per-field device-view buffers). The
        // whole-function path already marked its output slots above (its
        // output tuple IS the full result tuple); this fill is the
        // decomposition path's marking.
        if let Some(source) = kernel_source.source {
            if companion_gradient_slots.contains_key(&source.0) {
                if let Some(fields) = decomposition.return_tuple_fields(kernel_source) {
                    for (slot, field_local) in fields.iter().enumerate() {
                        let slot = u32::try_from(slot).unwrap_or(u32::MAX);
                        if !companion_gradient_slots[&source.0].contains(&slot) {
                            continue;
                        }
                        // The decomposition exposes every return-tuple field
                        // local as a subchain output (D-6: the last subchain
                        // ALSO exposes its written fields), so every gradient
                        // slot resolves to a distinct kernel buffer. A local
                        // that is somehow not exposed (a written local never
                        // returned and never read by a later subchain) can
                        // never be a gradient slot — the lookup skips it.
                        let Some((producer_index, output_position)) =
                            decomposition.producer_position(*field_local)
                        else {
                            continue;
                        };
                        let output_position = u32::try_from(output_position).unwrap_or(u32::MAX);
                        let Some(&buffer_id) =
                            subchain_output_buffer_ids.get(&(producer_index, output_position))
                        else {
                            continue;
                        };
                        if let Some(entry) = buffers.iter_mut().find(|entry| entry.id == buffer_id)
                        {
                            entry.gradient = true;
                        }
                        gradient_buffers.insert((source.0, slot), buffer_id);
                    }
                }
            }
        }
        Ok(())
    };

    for function in &kernel_functions {
        build_kernel(function)?;
    }

    // S3-A2 companion path: the generated companions of device-resident
    // primals, ordered AFTER the forward kernels. Each companion's tuple
    // gradient return lowers through the multi-output ABI into distinct
    // output resources; a carried companion missing from the lowered MIR
    // fails construction closed (relation facts are the typed routing
    // surface — never a name heuristic).
    for entry in companions.iter() {
        if !entry.device_resident {
            continue;
        }
        let companion = validated
            .program()
            .functions
            .iter()
            .find(|function| function.source == Some(entry.companion))
            .ok_or_else(|| {
                vec![device_diag(
                    "companion",
                    format!(
                        "companion DefId({}) of device-resident primal DefId({}) is missing from the lowered MIR",
                        entry.companion.0, entry.primal.0
                    ),
                )]
            })?;
        build_kernel(companion)?;
    }

    // S5-U5 train_step path: the optimizer-step functions the entry loop
    // calls, materialized after the forward + companion kernels. Their
    // trainable parameters are PerProgram InOut HostProvided buffers; their
    // update output slots alias those parameter buffers (in-place updates).
    if let Some(training) = &training {
        for function in program.functions.iter() {
            if training
                .trainable
                .iter()
                .any(|param| param.step_function == function.id)
            {
                build_kernel(function)?;
            }
        }
    }

    // S5A-U1: the declared per-step observation of a RepeatingStep training
    // program — the forward kernel's RETURN output (the loss the training
    // loop measures). Identified from the training facts, never a shape scan:
    // the forward function's decomposition's LAST subchain (or the
    // whole-function kernel) emits the function's return value as its PRIMARY
    // output slot — the first write slot in binding order (the
    // contract-shaped `signature.output`, which binds before the D-2b-1
    // extra outputs). The scalar-loss fixtures select the same buffer the
    // pre-cadence scalarity rule did — but the fact is now DECLARED, not
    // inferred from element counts.
    let per_step_observation: Option<BufferId> = if training.is_some() {
        let forward_functions: BTreeSet<MirFunctionId> = kernel_functions
            .iter()
            .map(|function| function.id)
            .collect();
        builds
            .iter()
            .rev()
            .find(|build| forward_functions.contains(&build.function))
            .and_then(|build| {
                build
                    .resources
                    .iter()
                    .find(|resource| {
                        matches!(
                            resource.access,
                            MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                        )
                    })
                    .map(|resource| resource.buffer_id)
            })
    } else {
        None
    };

    // S5A-U1: the declared END-OF-RUN gradient set — the companion gradient
    // buffers the training plan's gradient-flow links feed into a train_step
    // (the backward → train_step data-flow, a carried training fact). A
    // frozen-input gradient no step consumes is per-step scratch — declared
    // never, observed never.
    let train_step_gradients: BTreeSet<BufferId> = training
        .as_ref()
        .map(|training| {
            training
                .gradients
                .iter()
                .filter_map(|link| {
                    gradient_buffers
                        .get(&(link.companion.0, link.gradient_slot))
                        .copied()
                })
                .collect()
        })
        .unwrap_or_default();

    // Pass 2: materialize the program with the merged identity facts. Every
    // reference to a unified id carries the same name/role/lifetime so the
    // schema's cross-reference consistency checks pass. The launch ids are
    // assigned in declaration order; the launch SEQUENCE is re-ordered
    // afterwards to follow the carried dependency graph (F3) — declaration
    // order is never an execution authority.
    let lifetime = if training.is_some() {
        DeviceProgramLifetime::RepeatingStep
    } else {
        DeviceProgramLifetime::SingleRun
    };
    let mut program = DeviceProgram::new(lifetime);
    for build in builds {
        let kernel_index = program.kernels.len();
        let mut resources: Vec<DeviceResource> = Vec::with_capacity(build.resources.len());
        for slot in build.resources {
            // Static invariant (safe by construction): the unification pass
            // registered every buffer referenced by a slot in `builds`, so the
            // id lookup always succeeds.
            let entry = buffers
                .iter()
                .find(|entry| entry.id == slot.buffer_id)
                .expect("every slot buffer was registered by the unification pass");
            let identity = BufferIdentity {
                id: entry.id,
                name: entry.name.clone(),
                role: entry.role,
                storage: MirTensorStorageLayout::DeviceHandle,
                // Independent allocation-lifetime axis (F5): decided from the
                // buffer's aggregate access facts (written / consumed) and the
                // trainable-parameter flag, never derived from the role. The
                // host receives these facts through the payload; it never
                // re-derives a lifetime from slot role alone.
                lifetime: unified_lifetime(entry, training.is_some(), per_step_observation),
            };
            resources.push(DeviceResource {
                buffer: identity,
                version: BufferVersion {
                    version: 1,
                    element_ty: slot.element_ty,
                    element_count: slot.element_count,
                },
                binding: Binding {
                    group: slot.group,
                    binding: slot.binding,
                },
                access: slot.access,
            });
        }

        let launch_id = LaunchId(u32::try_from(program.launches.len()).unwrap_or(u32::MAX) + 1);
        program.kernels.push(KernelUnit {
            function: build.function,
            entry: build.entry.clone(),
            plan: build.plan,
            resources: resources.clone(),
            launch: build.launch,
        });
        program.launches.push(LaunchUnit {
            id: launch_id,
            kernel_index,
        });
    }

    // The launch sequence follows the carried dependency graph (F3), never
    // kernel declaration order. A RepeatingStep program's trainable-param
    // edges are persistent-state facts (the step writes the value the NEXT
    // step reads) — they are excluded so the single-step sequence stays
    // acyclic.
    let param_buffer_ids: Vec<BufferId> = buffers
        .iter()
        .filter(|buffer| buffer.param)
        .map(|buffer| buffer.id)
        .collect();
    program.launches = dependency_ordered_launches(&program, &param_buffer_ids)?;

    // S5A-U1: declare the observation-eligible results with their cadence.
    // The cadence is the constructor's DECLARED fact from the training
    // program's structure — never derived from buffer shapes or names:
    //   - a RepeatingStep training program declares the forward's return
    //     output (the loss) PerStep and the final forward / final gradients /
    //     final params EndOfRun;
    //   - a SingleRun program declares every written-not-consumed final a
    //     per-execution observation (PerStep).
    // Each result names the launch that produces its observed version (the
    // last write in execution order) as both its producing launch and its
    // observation boundary. The route and the host consume THIS declared set
    // — there is no derived end-of-run set anywhere downstream.
    program.results = declared_result_rows(
        &program,
        &buffers,
        training.is_some(),
        per_step_observation,
        &train_step_gradients,
    );

    // Pass 3: mint the carried semantic facts (F1–F6) and validate the
    // program together with them fail-closed.
    let semantics = semantic_facts(&program, &buffers, companions);
    program
        .validate_with_semantics(&semantics)
        .map_err(|error| {
            vec![device_diag(
                "validation",
                format!("constructed device program is inconsistent: {error}"),
            )]
        })?;
    // The effective declared step count (S5-U5b): the admitted training-plan
    // count when the program is a RepeatingStep training program; otherwise
    // the caller's declared value (ignored — the wire only carries the count
    // for RepeatingStep).
    let effective_steps = training
        .as_ref()
        .map(|facts| facts.step_count)
        .unwrap_or(declared_steps);
    Ok(Some((program, semantics, effective_steps)))
}

/// The output-slot tuple index of a multi-output ABI output resource: the
/// first output (`signature.output`) is tuple element 0, each
/// `extra_outputs` element follows.
fn output_slot_index(signature: &MirKernelSignature, output: &MirKernelResource) -> u32 {
    if signature.output.binding == output.binding {
        return 0;
    }
    for (index, extra) in signature.extra_outputs.iter().enumerate() {
        if extra.binding == output.binding {
            return u32::try_from(index + 1).unwrap_or(u32::MAX);
        }
    }
    u32::MAX
}

/// The independent allocation-lifetime axis (F5) of a unified buffer,
/// decided from its aggregate access facts and the trainable-parameter flag —
/// never from the role:
///
/// - a trainable parameter is persistent device-resident state (per-program);
/// - a buffer no kernel ever writes is host-provided persistent state
///   (per-program);
/// - a kernel-written buffer another kernel consumes is a step-local
///   intermediate (per-step);
/// - a kernel-written final is read back at an observation point.
///
/// G2/G3 (U8 repair) + S5A-U1: a `RepeatingStep` training program's per-step
/// observation is ONLY the DECLARED loss buffer (`per_step_observation`) —
/// the forward's return output. Every OTHER written-not-consumed final (the
/// forward/activation tensors the decomposition re-exposes) is a step-local
/// final (`PerStep`), declared an end-of-run observation rather than a
/// per-step readback. `SingleRun` programs keep the ordinary rule (all finals
/// are observation points).
fn unified_lifetime(
    entry: &ProgramBuffer,
    training_program: bool,
    per_step_observation: Option<BufferId>,
) -> BufferLifetime {
    // S5-U5 (param-identity.md): a trainable parameter accumulates updates
    // across steps in ONE buffer — allocated once (per-program), initialized
    // HostProvided at session creation, updated in place at every step.
    if entry.param {
        return BufferLifetime::PerProgram;
    }
    // G4 (F5): an in-place ReadWrite slot is PERSISTENT writable state — an
    // accumulation/optimizer-state buffer — not a per-step intermediate.
    // Its storage is allocated once (per-program), zero-filled at allocation
    // (the readwrite→ZeroFill initialization axis), and updated in place at
    // every generation: the accumulator/optimizer-state lifecycle G4
    // requires. No axis is derived from the role; the access facts decide.
    if entry.readwrite {
        return BufferLifetime::PerProgram;
    }
    // A companion gradient slot is per-step scratch (S5-U5): it feeds the
    // optimizer step within one step and recycles at the step boundary — even
    // a frozen-input gradient with no step consumer is never an observation.
    if entry.gradient {
        return BufferLifetime::PerStep;
    }
    if !entry.written {
        BufferLifetime::PerProgram
    } else if entry.consumed {
        BufferLifetime::PerStep
    } else if Some(entry.id) == per_step_observation {
        // The declared per-step observation — the loss — is the only
        // written-not-consumed final read back within each step.
        BufferLifetime::ObservationPoint
    } else if training_program {
        // A RepeatingStep program's OTHER written-not-consumed finals (the
        // forward tensors, including the decomposition's re-exposed
        // duplicates) are step-local — never per-step observations, never
        // per-step readbacks.
        BufferLifetime::PerStep
    } else {
        BufferLifetime::ObservationPoint
    }
}

/// The launch that produces a buffer's observed version: the LAST write
/// (Write/ReadWrite) in execution order — the version the final readback
/// observes (a trainable param's in-place train_step update, a companion's
/// gradient write, a forward subchain's final write).
fn producing_launch(program: &DeviceProgram, buffer: BufferId) -> Option<LaunchId> {
    let mut produced: Option<LaunchId> = None;
    for launch in &program.launches {
        let Some(kernel) = program.kernels.get(launch.kernel_index) else {
            continue;
        };
        if kernel.resources.iter().any(|resource| {
            resource.buffer.id == buffer
                && matches!(
                    resource.access,
                    MirKernelResourceAccess::Write | MirKernelResourceAccess::ReadWrite
                )
        }) {
            produced = Some(launch.id);
        }
    }
    produced
}

/// Declare the observation-eligible results of a materialized program with
/// their observation cadence (S5A-U1). The cadence is a DECLARED fact from
/// the training program's structure — never derived from buffer shapes or
/// names:
///
/// - a `RepeatingStep` training program declares the forward's return output
///   (the loss) `PerStep` and the final forward / final gradients / final
///   params `EndOfRun` — the gradients are the training plan's gradient-flow
///   buffers (the companion slots a train_step consumes), never a frozen
///   input's gradient;
/// - a `SingleRun` program declares every declared per-execution observation
///   point (an `ObservationPoint`-lifetime final) `PerStep`; per-step
///   scratch (a companion gradient slot) is never a result.
///
/// Each result names the launch that produces its observed version (the last
/// write in execution order) as both its producing launch and its
/// observation boundary (F6). The route and the host consume exactly this
/// declared set — no downstream derivation exists.
fn declared_result_rows(
    program: &DeviceProgram,
    buffers: &[ProgramBuffer],
    training_program: bool,
    per_step_observation: Option<BufferId>,
    train_step_gradients: &BTreeSet<BufferId>,
) -> Vec<radix_mir::device_program::ResultBuffer> {
    let mut rows: Vec<radix_mir::device_program::ResultBuffer> = Vec::new();
    for buffer in buffers {
        let Some(produced_by) = producing_launch(program, buffer.id) else {
            continue;
        };
        let lifetime = unified_lifetime(buffer, training_program, per_step_observation);
        let cadence = if training_program {
            if Some(buffer.id) == per_step_observation {
                // The declared per-step observation — the loss.
                ObservationCadence::PerStep
            } else if buffer.param
                || train_step_gradients.contains(&buffer.id)
                || (buffer.role == BufferRole::Output && buffer.written && !buffer.consumed)
            {
                // The final forward / final gradients / final params.
                ObservationCadence::EndOfRun
            } else {
                continue;
            }
        } else if lifetime == BufferLifetime::ObservationPoint {
            // SingleRun: every declared per-execution observation point (the
            // written-not-consumed finals). Per-step scratch (a companion
            // gradient slot) is never a result.
            ObservationCadence::PerStep
        } else {
            continue;
        };
        rows.push(radix_mir::device_program::ResultBuffer {
            buffer: BufferIdentity {
                id: buffer.id,
                name: buffer.name.clone(),
                role: buffer.role,
                storage: MirTensorStorageLayout::DeviceHandle,
                lifetime,
            },
            version: BufferVersion {
                version: 1,
                element_ty: buffer.element_ty,
                element_count: buffer.element_count,
            },
            role: buffer.role,
            produced_by,
            cadence,
        });
    }
    rows
}
