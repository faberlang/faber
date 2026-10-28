//! Kernel legality: the host-free subset of `@ nucleum` bodies (gpu-reset
//! rule 12), decided at check time.
//!
//! Boundary: typed-HIR pass after typecheck. It answers *language legality*
//! only — whether a kernel's meaning needs a host runtime (I/O, conversations,
//! async, heap-backed values, calls resolvable only at run time) — and the
//! call legality between kernels and host code. Per-target capability
//! (portable-profile widths, per-target verdicts, recursion gating) is a
//! separate requirement inference, and the MIR device-safe gate stays the
//! lowerability authority for shapes device emission cannot yet express.
//!
//! `@ nucleum fragment` bodies keep their own definition check
//! ([`super::fragment_definition`]); they are callable kernels here but are not
//! re-walked.

use super::kernel_legality_body::{
    check_fragment_lane_ids, check_host_block, check_host_function, check_kernel_function,
    check_policy_callback,
};
use crate::{
    ResolvedTensorEdgePolicy, Resolver, SemanticError, SemanticErrorKind, TensorEdgePolicyTable,
    TypeId, TypeTable, WarningKind,
};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::{DefId, HirAnnotation, HirFunction, HirItemKind, HirModule, NucleumRole};
use radix_lexer::{Interner, Span, Symbol};

/// A kernel body performs a host operation (I/O, console, conversation).
pub const ISSUE_HOST_EFFECT: &str = "nucleum_host_effect";
/// A kernel is async, a generator, awaits, or yields.
pub const ISSUE_ASYNC: &str = "nucleum_async";
/// A kernel names a value type that needs the host (heap text, unbounded
/// collections, dynamic values, host handles).
pub const ISSUE_HOST_TYPE: &str = "nucleum_host_type";
/// A kernel names a `saturating<float>` cell: the language admits it, but its
/// device lowering is not built (N7e D6; removed when device lowering lands).
pub const ISSUE_SATURATING_FLOAT_NOT_BUILT: &str = "nucleum_saturating_float_not_built";
/// A kernel names a ranged integer cell: the language admits it on the host,
/// but a kernel applies no range yet (need, series 4/5).
pub const ISSUE_RANGED_INT_NOT_BUILT: &str = "nucleum_ranged_policy_not_built";
/// A kernel names a list with no upper size (`list<T, size 1…∞>`): a kernel
/// unrolls every slot up to the maximum, so it needs a finite constant one.
pub const ISSUE_UNBOUNDED_LIST_SIZE: &str = "nucleum_unbounded_list_size";
/// A kernel uses compiler-internal workgroup state (atomics, barriers).
pub const ISSUE_WORKGROUP: &str = "nucleum_workgroup";
/// A kernel calls something that is not a kernel or an approved intrinsic.
pub const ISSUE_HOST_CALL: &str = "nucleum_host_call";
/// Host code calls a private kernel (only public kernels are launchable).
pub const ISSUE_PRIVATE_HOST_CALL: &str = "nucleum_private_host_call";
/// A kernel body installs a local handler.
pub const ISSUE_RECOVERY: &str = "nucleum_recovery";
/// A kernel declares a typed error channel `⇥ E` (any `E` other than text).
/// A kernel may declare `⇥ text` and nothing else (gpu-reset rules 9, 12.4).
pub const ISSUE_TYPED_ERROR_CHANNEL: &str = "nucleum_typed_error_channel";
/// User source calls a thread-id builtin (`global_invocation_id()` and its
/// siblings). They are raw lane ids, a compiler-internal lowering surface
/// (gpu-reset rule 12.5); the parallel pattern is a `filum` loop.
pub const ISSUE_RAW_LANE_ID: &str = "nucleum_raw_lane_id";
/// A kernel parameter is named after a thread-id builtin. The name alone
/// selects the builtin in the device ABI, so it is the raw lane id under
/// another spelling (gpu-reset rule 12.5).
pub const ISSUE_RAW_LANE_ID_PARAM: &str = "nucleum_raw_lane_id_param";
/// Warning issue: a kernel's trailing `u32` parameter is the compiler-bound
/// lane id, whatever its name. The positional lane ABI retires in favor of
/// `filum`.
pub const ISSUE_POSITIONAL_LANE_PARAM: &str = "positional_lane_param";

/// Outcome of the legality pass.
#[derive(Debug, Clone, Default)]
pub struct KernelLegalityReport {
    /// One structured diagnostic per violation.
    pub errors: Vec<SemanticError>,
    /// Non-fatal findings (the positional lane parameter).
    pub warnings: Vec<SemanticError>,
    /// Kernels on a call cycle (directly or mutually recursive). Recursion is
    /// language-legal; whether a target may run it is a capability fact for
    /// the requirement inference, which reads this set.
    pub recursive_kernels: FxHashSet<DefId>,
    /// Direct device-body calls collected by the same legality walk. This
    /// includes exact custom `limes` callbacks as special roots; ordinary
    /// unmarked callees are still rejected by the body walk.
    pub kernel_calls: FxHashMap<DefId, Vec<DefId>>,
}

/// What the pass knows about one kernel declared in this module.
#[derive(Debug, Clone, Copy)]
pub(super) struct KernelFact {
    pub role: NucleumRole,
    pub public: bool,
}

/// Module-level facts shared by the body walk and the type classifier.
#[derive(Debug, Default)]
pub(super) struct ModuleFacts {
    pub kernels: FxHashMap<DefId, KernelFact>,
    pub host_functions: FxHashSet<DefId>,
    pub names: FxHashMap<DefId, Symbol>,
    pub method_index: FxHashMap<(DefId, Symbol), DefId>,
    pub struct_fields: FxHashMap<DefId, Vec<TypeId>>,
    pub enum_fields: FxHashMap<DefId, Vec<TypeId>>,
}

/// Read-only compiler state a walk needs.
pub(super) struct Env<'a> {
    pub facts: &'a ModuleFacts,
    pub resolver: &'a Resolver,
    pub types: &'a TypeTable,
    pub interner: &'a Interner,
    pub tensor_edge_policies: &'a TensorEdgePolicyTable,
}

/// Diagnostic sink that reports each violation once per span.
#[derive(Debug, Default)]
pub(super) struct Sink {
    pub errors: Vec<SemanticError>,
    pub warnings: Vec<SemanticError>,
    seen: FxHashSet<(&'static str, u32, u32, String)>,
}

impl Sink {
    /// Record a non-fatal finding of warning `kind` with structured `issue`.
    pub fn warn(
        &mut self,
        kind: WarningKind,
        issue: &'static str,
        span: Span,
        args: &[(&'static str, &str)],
    ) {
        let mut warning = SemanticError::structured(SemanticErrorKind::Warning(kind), span)
            .with_arg("issue", issue);
        for (name, value) in args {
            warning = warning.with_arg(name, *value);
        }
        self.warnings.push(warning);
    }

    pub fn report(&mut self, issue: &'static str, span: Span, args: &[(&'static str, &str)]) {
        let key = (
            issue,
            span.start,
            span.end,
            args.iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join(";"),
        );
        if !self.seen.insert(key) {
            return;
        }
        let mut error = SemanticError::structured(SemanticErrorKind::LoweringError, span)
            .with_arg("issue", issue);
        for (name, value) in args {
            error = error.with_arg(name, *value);
        }
        self.errors.push(error);
    }
}

/// Check every kernel definition, and every host call site of a kernel, in
/// one module.
#[must_use]
pub fn check(
    hir: &HirModule,
    resolver: &Resolver,
    types: &TypeTable,
    interner: &Interner,
    tensor_edge_policies: &TensorEdgePolicyTable,
) -> KernelLegalityReport {
    let facts = collect_facts(hir, interner);
    let env = Env {
        facts: &facts,
        resolver,
        types,
        interner,
        tensor_edge_policies,
    };
    let mut sink = Sink::default();
    let mut edges: FxHashMap<DefId, Vec<DefId>> = FxHashMap::default();
    // Built in the same walk: the callback pass below needs every callable
    // body, not only the kernels checked here.
    let mut callables: FxHashMap<DefId, (&HirFunction, Span)> = FxHashMap::default();

    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(function) => {
                check_function(
                    &env,
                    &mut sink,
                    &mut edges,
                    item.def_id,
                    function,
                    item.span,
                );
                callables.insert(item.def_id, (function.as_ref(), item.span));
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    check_function(
                        &env,
                        &mut sink,
                        &mut edges,
                        method.def_id,
                        &method.func,
                        method.span,
                    );
                    callables.insert(method.def_id, (&method.func, method.span));
                }
            }
            _ => {}
        }
    }

    // Custom policy methods are ordinary zero-field methods in source, not
    // `@ nucleum` functions. Admit only exact DefIds retained by the limes
    // sidecar and check those bodies under the same host-free device rules.
    let callback_defs: FxHashSet<DefId> = tensor_edge_policies
        .calls
        .values()
        .filter_map(|policy| match policy {
            ResolvedTensorEdgePolicy::Custom {
                remap, transform, ..
            } => Some(std::iter::once(*remap).chain(transform.iter().copied())),
            ResolvedTensorEdgePolicy::Builtin(_) | ResolvedTensorEdgePolicy::Deferred { .. } => {
                None
            }
        })
        .flatten()
        .collect();
    let mut callback_queue: Vec<DefId> = edges
        .values()
        .flatten()
        .filter(|callee| callback_defs.contains(callee))
        .copied()
        .collect();
    let mut checked_callbacks = FxHashSet::default();
    while let Some(callback) = callback_queue.pop() {
        if !checked_callbacks.insert(callback) {
            continue;
        }
        let Some((function, span)) = callables.get(&callback).copied() else {
            sink.report("nucleum_policy_callback_missing", Span::default(), &[]);
            continue;
        };
        let callees = check_policy_callback(&env, &mut sink, function, span);
        callback_queue.extend(
            callees
                .iter()
                .filter(|callee| callback_defs.contains(callee))
                .copied(),
        );
        edges.insert(callback, callees);
    }
    if let Some(entry) = &hir.entry {
        check_host_block(&env, &mut sink, entry);
    }
    super::device_region::check_regions(&env, &mut sink, hir);

    let recursive_kernels = kernels_on_cycles(&edges);
    KernelLegalityReport {
        errors: sink.errors,
        warnings: sink.warnings,
        recursive_kernels,
        kernel_calls: edges,
    }
}

fn check_function(
    env: &Env<'_>,
    sink: &mut Sink,
    edges: &mut FxHashMap<DefId, Vec<DefId>>,
    def_id: DefId,
    function: &HirFunction,
    span: Span,
) {
    match function.nucleum_role() {
        Some(NucleumRole::Entry) => {
            let callees = check_kernel_function(env, sink, function, span);
            edges.insert(def_id, callees);
        }
        // Composition helpers own their definition check, but a raw lane id
        // is illegal in every user-written device body.
        Some(NucleumRole::Fragment) => check_fragment_lane_ids(env, sink, function),
        None => check_host_function(env, sink, function),
    }
}

fn collect_facts(hir: &HirModule, interner: &Interner) -> ModuleFacts {
    let mut facts = ModuleFacts::default();
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(function) => {
                record_callable(
                    &mut facts,
                    item.def_id,
                    function,
                    &item.annotations,
                    interner,
                );
            }
            HirItemKind::Struct(strukt) => {
                facts.struct_fields.insert(
                    item.def_id,
                    strukt.fields.iter().map(|field| field.ty).collect(),
                );
                for method in &strukt.methods {
                    facts
                        .method_index
                        .insert((item.def_id, method.func.name), method.def_id);
                    record_callable(
                        &mut facts,
                        method.def_id,
                        &method.func,
                        &method.annotations,
                        interner,
                    );
                }
            }
            HirItemKind::Enum(enumeration) => {
                let fields = enumeration
                    .commune_fields
                    .iter()
                    .chain(
                        enumeration
                            .variants
                            .iter()
                            .flat_map(|variant| variant.fields.iter()),
                    )
                    .map(|field| field.ty)
                    .collect();
                facts.enum_fields.insert(item.def_id, fields);
            }
            _ => {}
        }
    }
    facts
}

fn record_callable(
    facts: &mut ModuleFacts,
    def_id: DefId,
    function: &HirFunction,
    annotations: &[HirAnnotation],
    interner: &Interner,
) {
    facts.names.insert(def_id, function.name);
    match function.nucleum_role() {
        Some(role) => {
            let public = annotations
                .iter()
                .any(|annotation| interner.resolve(annotation.family) == "publica");
            facts.kernels.insert(def_id, KernelFact { role, public });
        }
        None => {
            facts.host_functions.insert(def_id);
        }
    }
}

/// Kernels that can reach themselves through kernel-to-kernel calls.
fn kernels_on_cycles(edges: &FxHashMap<DefId, Vec<DefId>>) -> FxHashSet<DefId> {
    edges
        .keys()
        .copied()
        .filter(|start| reaches(edges, *start, *start))
        .collect()
}

fn reaches(edges: &FxHashMap<DefId, Vec<DefId>>, from: DefId, target: DefId) -> bool {
    let mut seen = FxHashSet::default();
    let mut stack: Vec<DefId> = edges.get(&from).cloned().unwrap_or_default();
    while let Some(next) = stack.pop() {
        if next == target {
            return true;
        }
        if seen.insert(next) {
            stack.extend(edges.get(&next).into_iter().flatten().copied());
        }
    }
    false
}
