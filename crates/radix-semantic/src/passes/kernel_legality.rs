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

use super::kernel_legality_body::{check_host_block, check_host_function, check_kernel_function};
use crate::{Resolver, SemanticError, SemanticErrorKind, TypeId, TypeTable};
use radix_hir::{DefId, HirAnnotation, HirFunction, HirItemKind, HirModule, NucleumRole};
use radix_lexer::{Interner, Span, Symbol};
use rustc_hash::{FxHashMap, FxHashSet};

/// A kernel body performs a host operation (I/O, console, conversation).
pub const ISSUE_HOST_EFFECT: &str = "nucleum_host_effect";
/// A kernel is async, a generator, awaits, or yields.
pub const ISSUE_ASYNC: &str = "nucleum_async";
/// A kernel names a value type that needs the host (heap text, unbounded
/// collections, dynamic values, host handles).
pub const ISSUE_HOST_TYPE: &str = "nucleum_host_type";
/// A kernel uses compiler-internal workgroup state (atomics, barriers).
pub const ISSUE_WORKGROUP: &str = "nucleum_workgroup";
/// A kernel calls something that is not a kernel or an approved intrinsic.
pub const ISSUE_HOST_CALL: &str = "nucleum_host_call";
/// Host code calls a private kernel (only public kernels are launchable).
pub const ISSUE_PRIVATE_HOST_CALL: &str = "nucleum_private_host_call";
/// A kernel body installs a local handler.
pub const ISSUE_RECOVERY: &str = "nucleum_recovery";

/// Outcome of the legality pass.
#[derive(Debug, Clone, Default)]
pub struct KernelLegalityReport {
    /// One structured diagnostic per violation.
    pub errors: Vec<SemanticError>,
    /// Kernels on a call cycle (directly or mutually recursive). Recursion is
    /// language-legal; whether a target may run it is a capability fact for
    /// the requirement inference, which reads this set.
    pub recursive_kernels: FxHashSet<DefId>,
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
}

/// Diagnostic sink that reports each violation once per span.
#[derive(Debug, Default)]
pub(super) struct Sink {
    pub errors: Vec<SemanticError>,
    seen: FxHashSet<(&'static str, u32, u32, String)>,
}

impl Sink {
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
) -> KernelLegalityReport {
    let facts = collect_facts(hir, interner);
    let env = Env {
        facts: &facts,
        resolver,
        types,
        interner,
    };
    let mut sink = Sink::default();
    let mut edges: FxHashMap<DefId, Vec<DefId>> = FxHashMap::default();

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
                }
            }
            _ => {}
        }
    }
    if let Some(entry) = &hir.entry {
        check_host_block(&env, &mut sink, entry);
    }

    KernelLegalityReport {
        errors: sink.errors,
        recursive_kernels: kernels_on_cycles(&edges),
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
        // Graphics-stage fragments and composition helpers own their check.
        Some(NucleumRole::Fragment) => {}
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
