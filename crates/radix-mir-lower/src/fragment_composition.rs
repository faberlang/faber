//! Device-call MIR composition (post-monomorphization, DFV2-4).
//!
//! Every call from device code (`@ nucleum` compute or its graphics-fragment
//! sugar) to a concrete device function — local or imported, entry or private
//! helper — is cloned into the caller: parameters bind to call-site values
//! exactly once, locals/blocks/values are alpha-renamed, the cloned return
//! path joins the caller's destination, and source spans stay with the cloned
//! nodes. Host callers are the launch boundary, never composition inputs.
//! Cycles are rejected before any clone. Expansion budgets fail closed —
//! there is no "emit a call instead" escape. A composed private compute
//! helper with no remaining reference is pruned so no standalone helper
//! symbol survives; fragment bodies stay for the package body sidecar.
//!
//! Documented budget defaults (starting anchors):
//! - depth 16
//! - `65_536` cloned MIR nodes per `Entry`
//! - `32_768` cloned MIR nodes per root call site
//! - `1_024` concrete instances per `Entry`
//! - 1 inline per call site

use handroll::{FxHashMap, FxHashSet};
use radix_diagnostics::DiagnosticArg;
use radix_hir::DefId;
use radix_lexer::{Interner, Span};
use radix_mir::device::MirDeviceFunctionMetadataMap;
use radix_mir::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirBlock, MirBlockId, MirCallee, MirConversion, MirDeviceRole,
    MirFilumReduceOp, MirFunction, MirFunctionId, MirFunctionOrigin, MirIntrinsic, MirKeyValueOperand, MirLocal,
    MirLocalId, MirMonomorphization, MirNamedOperand, MirOperand, MirOptionChainLink, MirOptionOp, MirParam,
    MirParamMode, MirPlace, MirPlaceBase, MirProjection, MirRuntimeCall, MirStatement, MirStatementKind, MirTemp,
    MirTempId, MirTerminator, MirTerminatorKind, MirType, MirValue, MirValueId, MirValueKind,
};
use radix_semantic::arg_value;
use std::cell::Cell;
use std::rc::Rc;

pub const ISSUE_CYCLE: &str = "nucleum_fragment_cycle";
pub const ISSUE_EXPANSION: &str = "nucleum_fragment_expansion";

/// Documented bounded defaults for the composition budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpansionBudget {
    pub max_depth: u32,
    pub max_cloned_nodes_per_entry: u32,
    pub max_cloned_nodes_per_root_callsite: u32,
    pub max_concrete_instances: u32,
    pub max_inlines_per_callsite: u32,
}

impl ExpansionBudget {
    pub const DOCUMENTED_DEFAULTS: Self = Self {
        max_depth: 16,
        max_cloned_nodes_per_entry: 65_536,
        max_cloned_nodes_per_root_callsite: 32_768,
        max_concrete_instances: 1_024,
        max_inlines_per_callsite: 1,
    };
}

// The budget override stays compiled in every configuration: the companion
// tests that drive it live in `radix-module` and run against this crate as a
// normal dependency, where a `#[cfg(test)]` copy would not exist.
thread_local! {
    static BUDGET_OVERRIDE: Cell<Option<ExpansionBudget>> = const { Cell::new(None) };
}

/// Run `f` with the composition budgets overridden to `budget`.
///
/// The override is thread-local and restored on return, so one test's budget
/// cannot leak into another.
pub fn with_budget_override<R>(budget: ExpansionBudget, f: impl FnOnce() -> R) -> R {
    struct Reset(Option<ExpansionBudget>);
    impl Drop for Reset {
        fn drop(&mut self) {
            BUDGET_OVERRIDE.with(|slot| slot.set(self.0));
        }
    }
    let previous = BUDGET_OVERRIDE.with(|slot| slot.replace(Some(budget)));
    let _reset = Reset(previous);
    f()
}

/// Active budget: the thread-local override when set, else the documented defaults.
#[must_use]
pub fn active_budget() -> ExpansionBudget {
    if let Some(budget) = BUDGET_OVERRIDE.with(Cell::get) {
        return budget;
    }
    ExpansionBudget::DOCUMENTED_DEFAULTS
}

/// One composition diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentCompositionError {
    pub issue: &'static str,
    pub reason: &'static str,
    pub span: Span,
    /// Secondary span for dual-site composition diagnostics.
    pub definition_span: Option<Span>,
    pub args: Vec<DiagnosticArg>,
}

impl FragmentCompositionError {
    #[must_use]
    pub fn message(&self) -> String {
        let rendered = match self.issue {
            ISSUE_CYCLE => nucleum_fragment_cycle_message(&self.args),
            ISSUE_EXPANSION => Some(nucleum_fragment_expansion_message(self.reason, &self.args)),
            _ => None,
        };
        rendered.unwrap_or_else(|| format!("{}: {}", self.issue, self.reason))
    }
}

/// Message prose for this module's issues lives here; the semantic catalog
/// carries only the code and help text for them.
fn nucleum_fragment_cycle_message(args: &[DiagnosticArg]) -> Option<String> {
    let cycle = arg_value(args, "cycle").or_else(|| arg_value(args, "chain"))?;
    Some(format!("nucleum fragment call cycle: {cycle}"))
}

fn nucleum_fragment_expansion_message(reason: &str, args: &[DiagnosticArg]) -> String {
    let kind = arg_value(args, "kind").or(Some(reason));
    let observed = arg_value(args, "observed");
    let limit = arg_value(args, "limit");
    let chain = arg_value(args, "chain");
    match (kind, observed, limit, chain) {
        (Some("survived"), _, _, _) => "nucleum fragment call survived composition".to_owned(),
        (Some("unresolved"), _, _, _) => "nucleum fragment call could not be resolved for composition".to_owned(),
        (Some("failable"), _, _, _) => "nucleum fragment call cannot use a failable terminator".to_owned(),
        (Some(kind), Some(observed), Some(limit), Some(chain)) => {
            format!("nucleum fragment expansion exceeds {kind} limit {limit} (observed {observed}) along {chain}")
        }
        (Some(kind), Some(observed), Some(limit), _) => {
            format!("nucleum fragment expansion exceeds {kind} limit {limit} (observed {observed})")
        }
        _ => "nucleum fragment expansion exceeds the documented budget".to_owned(),
    }
}

/// Compose every device call into its caller with the explicit metadata
/// carrier for source-less synthetic kernel closures (KCM-05). Returns
/// diagnostics; empty is success.
///
/// `launch_entries` are the source defs of launchable `@ nucleum @ publica`
/// entries: they stay program functions even when device code also calls
/// them. Every other private compute helper that was inlined at least once
/// and has no remaining reference is pruned after composition; fragment
/// bodies stay (the package body sidecar attaches from this program).
///
/// The carrier classifies a synthetic closure as device-side for
/// composition — a kernel caller's direct call composes, and a composed
/// closure with no remaining reference is pruned — without ever making it a
/// launch entry (`launch_entries` stays `DefId`-keyed and synthetic
/// functions carry no source).
pub fn compose_with_metadata(
    functions: &mut Vec<MirFunction>,
    monomorphizations: &[MirMonomorphization],
    roles: &FxHashMap<DefId, MirDeviceRole>,
    metadata: &MirDeviceFunctionMetadataMap,
    budget: ExpansionBudget,
    interner: Option<&Interner>,
    launch_entries: &FxHashSet<DefId>,
) -> Vec<FragmentCompositionError> {
    compose_keeping_template_callees(
        functions,
        monomorphizations,
        roles,
        metadata,
        budget,
        interner,
        launch_entries,
        &[],
    )
}

/// [`compose_with_metadata`] for a unit that exports generic templates.
///
/// A generic template is not in `functions` (it instantiates at the package
/// link), so its calls are invisible to the prune scan. A private helper the
/// template calls must survive the unit's composition, or the instantiated
/// kernel meets a call to a function the program no longer has. Every callee a
/// template names is therefore treated as referenced.
pub fn compose_keeping_template_callees(
    functions: &mut Vec<MirFunction>,
    monomorphizations: &[MirMonomorphization],
    roles: &FxHashMap<DefId, MirDeviceRole>,
    metadata: &MirDeviceFunctionMetadataMap,
    budget: ExpansionBudget,
    interner: Option<&Interner>,
    launch_entries: &FxHashSet<DefId>,
    templates: &[MirFunction],
) -> Vec<FragmentCompositionError> {
    let template_callees = {
        templates
            .iter()
            .flat_map(|template| Resolver::all_callees(template))
            .map(|site| site.callee)
            .collect::<Vec<_>>()
    };
    let mut pass = CompositionPass {
        functions,
        monomorphizations,
        roles,
        metadata,
        budget,
        interner,
        launch_entries,
        template_callees,
        inlined: FxHashSet::default(),
        errors: Vec::new(),
    };
    pass.run();
    pass.errors
}

/// Device call sites that remain in `functions`, using the explicit
/// synthetic-kernel metadata carrier.
///
/// Only device callers are scanned: a host caller's call to a device entry
/// is the launch/partition boundary, not a composition survivor.
#[must_use]
pub fn surviving_fragment_calls_with_metadata(
    functions: &[MirFunction],
    monomorphizations: &[MirMonomorphization],
    roles: &FxHashMap<DefId, MirDeviceRole>,
    metadata: &MirDeviceFunctionMetadataMap,
) -> Vec<(MirFunctionId, Span)> {
    let resolver = Resolver { functions, monomorphizations, roles, metadata };
    let mut survivors = Vec::new();
    for function in functions {
        if !matches!(
            role_of(function, roles, metadata),
            RoleKind::Entry | RoleKind::Fragment | RoleKind::SyntheticKernel
        ) {
            continue;
        }
        for site in resolver.device_sites(function) {
            survivors.push((function.id, site.span));
        }
    }
    survivors
}

/// Fail-closed gate: any remaining device call is a composition error.
#[must_use]
pub fn reject_surviving_fragment_calls_with_metadata(
    functions: &[MirFunction],
    monomorphizations: &[MirMonomorphization],
    roles: &FxHashMap<DefId, MirDeviceRole>,
    metadata: &MirDeviceFunctionMetadataMap,
) -> Vec<FragmentCompositionError> {
    surviving_fragment_calls_with_metadata(functions, monomorphizations, roles, metadata)
        .into_iter()
        .map(|(_, span)| {
            error(
                ISSUE_EXPANSION,
                "survived",
                span,
                vec![
                    DiagnosticArg::new("kind", "survived"),
                    DiagnosticArg::new("observed", "1"),
                    DiagnosticArg::new("limit", "0"),
                ],
            )
        })
        .collect()
}

struct CompositionPass<'a> {
    functions: &'a mut Vec<MirFunction>,
    monomorphizations: &'a [MirMonomorphization],
    roles: &'a FxHashMap<DefId, MirDeviceRole>,
    metadata: &'a MirDeviceFunctionMetadataMap,
    budget: ExpansionBudget,
    interner: Option<&'a Interner>,
    launch_entries: &'a FxHashSet<DefId>,
    /// Callees named by this unit's generic templates (never pruned).
    template_callees: Vec<MirCallee>,
    /// Functions inlined at least once during this pass (prune candidates).
    inlined: FxHashSet<MirFunctionId>,
    errors: Vec<FragmentCompositionError>,
}

impl CompositionPass<'_> {
    fn run(&mut self) {
        if !self.reject_cycles() {
            return;
        }
        let entry_ids = self
            .functions
            .iter()
            .filter(|function| self.is_entry(function))
            .map(|function| function.id)
            .collect::<Vec<_>>();
        for id in entry_ids {
            self.expand_function(id);
            if !self.errors.is_empty() {
                return;
            }
        }
        // Only device callers expand: a host caller's call to a device entry
        // is the launch boundary, not a composition input.
        let leftover = self
            .functions
            .iter()
            .filter(|function| self.is_device_callee(function) && self.resolver().has_device_call(function))
            .map(|function| function.id)
            .collect::<Vec<_>>();
        for id in leftover {
            self.expand_function(id);
            if !self.errors.is_empty() {
                return;
            }
        }
        self.errors
            .extend(reject_surviving_fragment_calls_with_metadata(
                self.functions,
                self.monomorphizations,
                self.roles,
                self.metadata,
            ));
        if self.errors.is_empty() {
            self.prune_composed_helpers();
        }
    }

    /// Prune private `@ nucleum` helpers that were composed into their
    /// callers and no longer have any call reference: a composed unit leaves
    /// no standalone compute-helper symbol in the program. Launch entries
    /// always stay; helpers still referenced by host code stay; fragment
    /// bodies stay (the package body sidecar attaches from the lowered
    /// program, and the graphics survivor scan owns their symbol proof).
    /// Composed synthetic kernel closures prune under the same law — they
    /// are private device helpers by carrier fact and can never be launch
    /// entries (`launch_entries` is `DefId`-keyed).
    fn prune_composed_helpers(&mut self) {
        if self.inlined.is_empty() {
            return;
        }
        let mut referenced_ids = FxHashSet::default();
        let mut referenced_sources = FxHashSet::default();
        for callee in &self.template_callees {
            match callee {
                MirCallee::Function(id) => {
                    referenced_ids.insert(*id);
                }
                MirCallee::Definition { source, .. } => {
                    referenced_sources.insert(*source);
                }
                MirCallee::Closure(_) | MirCallee::Value(_) => {}
            }
        }
        for function in self.functions.iter() {
            for site in Resolver::all_callees(function) {
                match site.callee {
                    MirCallee::Function(id) => {
                        referenced_ids.insert(id);
                    }
                    MirCallee::Definition { source, .. } => {
                        referenced_sources.insert(source);
                    }
                    MirCallee::Closure(_) | MirCallee::Value(_) => {}
                }
            }
        }
        self.functions.retain(|function| {
            if !self.inlined.contains(&function.id) {
                return true;
            }
            // Only device helpers prune: fragment bodies are the package body
            // sidecar's input and stay in the lowered program; synthetic
            // kernel closures prune like the private compute helpers they are.
            if !matches!(
                role_of(function, self.roles, self.metadata),
                RoleKind::Entry | RoleKind::SyntheticKernel
            ) {
                return true;
            }
            let referenced = referenced_ids.contains(&function.id)
                || function
                    .source
                    .is_some_and(|source| referenced_sources.contains(&source));
            let launchable = function
                .source
                .is_some_and(|source| self.launch_entries.contains(&source));
            referenced || launchable
        });
    }

    fn reject_cycles(&mut self) -> bool {
        let graph = self.fragment_graph();
        let mut visiting = FxHashSet::default();
        let mut visited = FxHashSet::default();
        let mut stack = Vec::new();
        let mut seen_cycles = FxHashSet::default();
        let nodes = graph.keys().copied().collect::<Vec<_>>();
        for start in nodes {
            self.dfs_cycles(start, &graph, &mut visiting, &mut visited, &mut stack, &mut seen_cycles);
        }
        self.errors.is_empty()
    }

    fn fragment_graph(&self) -> FxHashMap<MirFunctionId, Vec<MirFunctionId>> {
        let resolver = self.resolver();
        let mut graph = FxHashMap::default();
        for function in self.functions.iter() {
            if !self.is_device_callee(function) {
                continue;
            }
            let mut edges = Vec::new();
            for site in resolver.device_sites(function) {
                if let Some(callee) = resolver.resolve_function(&site.callee)
                    && self.is_device_callee(callee)
                    && !edges.contains(&callee.id)
                {
                    edges.push(callee.id);
                }
            }
            graph.insert(function.id, edges);
        }
        graph
    }

    fn dfs_cycles(
        &mut self,
        node: MirFunctionId,
        graph: &FxHashMap<MirFunctionId, Vec<MirFunctionId>>,
        visiting: &mut FxHashSet<MirFunctionId>,
        visited: &mut FxHashSet<MirFunctionId>,
        stack: &mut Vec<MirFunctionId>,
        seen: &mut FxHashSet<Vec<MirFunctionId>>,
    ) {
        if visited.contains(&node) || (!self.errors.is_empty() && visiting.is_empty()) {
            return;
        }
        if !visiting.insert(node) {
            if let Some(start) = stack.iter().position(|id| *id == node) {
                let mut cycle = stack[start..].to_vec();
                cycle.push(node);
                let key = normalize_cycle(&cycle);
                if seen.insert(key) {
                    let span = self
                        .function(node)
                        .map(|function| function.span)
                        .unwrap_or_default();
                    let chain = self.chain_labels(&cycle);
                    self.errors.push(error(
                        ISSUE_CYCLE,
                        "cycle",
                        span,
                        vec![DiagnosticArg::new("cycle", &chain), DiagnosticArg::new("chain", &chain)],
                    ));
                }
            }
            return;
        }
        stack.push(node);
        if let Some(edges) = graph.get(&node) {
            for next in edges {
                self.dfs_cycles(*next, graph, visiting, visited, stack, seen);
            }
        }
        stack.pop();
        visiting.remove(&node);
        visited.insert(node);
    }

    fn expand_function(&mut self, caller_id: MirFunctionId) {
        let mut account = Account::new();
        // Nesting is tracked per statement, not per block: sibling calls in
        // one block share their enclosing depth, and only the statements a
        // splice introduces sit one level deeper than the call they replace.
        let root = Rc::new(vec![caller_id]);
        let mut nesting: FxHashMap<MirBlockId, BlockNesting> = FxHashMap::default();
        if let Some(function) = self.function(caller_id) {
            for block in &function.blocks {
                nesting.insert(block.id, BlockNesting::root(block.statements.len(), &root));
            }
        }
        let mut steps = 0;
        loop {
            steps += 1;
            if steps > self.budget.max_concrete_instances.saturating_add(2) {
                let span = self
                    .function(caller_id)
                    .map(|function| function.span)
                    .unwrap_or_default();
                self.push_expansion("instances", span, steps, self.budget.max_concrete_instances, &[caller_id]);
                return;
            }
            let site = {
                let resolver = self.resolver();
                self.function(caller_id)
                    .and_then(|function| resolver.device_sites(function).into_iter().next())
            };
            let Some(site) = site else {
                return;
            };
            let Some(callee) = self.resolver().resolve_function(&site.callee).cloned() else {
                self.push_expansion("unresolved", site.span, 1, 0, &[caller_id]);
                return;
            };
            if !self.is_device_callee(&callee) {
                return;
            }
            if let Some(fragment_lane) = lane_param(&callee) {
                let caller_had_lane = self.function(caller_id).and_then(lane_param).is_some();
                let promoted = !caller_had_lane && fragment_lane.mode == MirParamMode::Lane;
                if promoted {
                    self.add_propagated_lane_param(caller_id, fragment_lane);
                }
                let caller_lane = self.function(caller_id).and_then(lane_param);
                let same_name = caller_lane
                    .and_then(|caller| caller.name)
                    .zip(fragment_lane.name)
                    .is_some_and(|(caller, fragment)| caller == fragment);
                let allowed = match caller_lane {
                    None => false,
                    Some(_) if fragment_lane.mode == MirParamMode::LaneInherited => same_name,
                    Some(bound) => promoted || (bound.mode == MirParamMode::LaneInherited && same_name),
                };
                if !allowed {
                    let mut diagnostic = error(
                        "nucleum_lane_fragment_clash",
                        "fragment lane binding conflicts with caller lane binding",
                        site.span,
                        vec![
                            DiagnosticArg::new(
                                "caller",
                                caller_lane
                                    .and_then(|param| param.name)
                                    .and_then(|name| self.interner.map(|interner| interner.resolve(name)))
                                    .unwrap_or("none"),
                            ),
                            DiagnosticArg::new(
                                "fragment",
                                fragment_lane
                                    .name
                                    .and_then(|name| self.interner.map(|interner| interner.resolve(name)))
                                    .unwrap_or("lane"),
                            ),
                        ],
                    );
                    diagnostic.definition_span = Some(fragment_lane.span);
                    self.errors.push(diagnostic);
                    return;
                }
            }
            if !Self::expand_step(self, caller_id, &site, &callee, &mut account, &mut nesting, &root) {
                return;
            }
        }
    }

    /// One expansion step: gate `site` against every budget, compose it, and
    /// record the splice. `false` means an expansion error was pushed and the
    /// caller must stop expanding.
    fn expand_step(
        pass: &mut CompositionPass<'_>,
        caller_id: MirFunctionId,
        site: &CallSite,
        callee: &MirFunction,
        account: &mut Account,
        nesting: &mut FxHashMap<MirBlockId, BlockNesting>,
        root: &Rc<Vec<MirFunctionId>>,
    ) -> bool {
        let enclosing = nesting
            .get(&site.block_id)
            .and_then(|block| block.site(site.stmt_index))
            .cloned()
            .unwrap_or_else(|| Nesting { depth: 0, chain: Rc::clone(root), root_site: 0 });
        let depth = enclosing.depth.saturating_add(1);
        let mut chain = enclosing.chain.as_ref().clone();
        chain.push(callee.id);
        if depth > pass.budget.max_depth {
            pass.push_expansion("depth", site.span, depth, pass.budget.max_depth, &chain);
            return false;
        }
        let cloned_nodes = count_nodes(callee) + saturating_u32(callee.params.len());
        if account.nodes.saturating_add(cloned_nodes) > pass.budget.max_cloned_nodes_per_entry {
            pass.push_expansion(
                "nodes",
                site.span,
                account.nodes.saturating_add(cloned_nodes),
                pass.budget.max_cloned_nodes_per_entry,
                &chain,
            );
            return false;
        }
        let root_site = if enclosing.depth == 0 {
            account.next_root = account.next_root.saturating_add(1);
            account.next_root
        } else {
            enclosing.root_site
        };
        let site_nodes = account
            .root_nodes
            .get(&root_site)
            .copied()
            .unwrap_or(0)
            .saturating_add(cloned_nodes);
        if site_nodes > pass.budget.max_cloned_nodes_per_root_callsite {
            pass.push_expansion(
                "callsite_nodes",
                site.span,
                site_nodes,
                pass.budget.max_cloned_nodes_per_root_callsite,
                &chain,
            );
            return false;
        }
        if account.instances.saturating_add(1) > pass.budget.max_concrete_instances {
            pass.push_expansion(
                "instances",
                site.span,
                account.instances.saturating_add(1),
                pass.budget.max_concrete_instances,
                &chain,
            );
            return false;
        }
        let site_key = (callee.id, site.block_id, site.stmt_index.map_or(u32::MAX, saturating_u32));
        let inlines = account
            .sites
            .get(&site_key)
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        if inlines > pass.budget.max_inlines_per_callsite {
            pass.push_expansion("callsite", site.span, inlines, pass.budget.max_inlines_per_callsite, &chain);
            return false;
        }
        // A failable call composes only for a callee with an error
        // channel (U9-6b: a throwing kernel); the instantiation check has
        // already refused any callee whose error is not a text and any
        // caller that cannot throw.
        if site.is_try_call && callee.error_ty.is_none() {
            pass.push_expansion("failable", site.span, 1, 0, &chain);
            return false;
        }
        let Some(new_blocks) = pass.inline_call(caller_id, site, callee) else {
            pass.push_expansion("survived", site.span, 1, 0, &chain);
            return false;
        };
        account.nodes = account.nodes.saturating_add(cloned_nodes);
        account.instances = account.instances.saturating_add(1);
        account.sites.insert(site_key, inlines);
        account.root_nodes.insert(root_site, site_nodes);
        pass.inlined.insert(callee.id);
        let spliced = Nesting { depth, chain: Rc::new(chain), root_site };
        if let Some(function) = pass.function(caller_id) {
            splice_nesting(nesting, function, site, &new_blocks, &spliced);
        }
        true
    }

    fn add_propagated_lane_param(&mut self, caller_id: MirFunctionId, fragment_lane: &MirParam) {
        let Some(index) = self
            .functions
            .iter()
            .position(|function| function.id == caller_id)
        else {
            return;
        };
        let entry = self.is_entry(&self.functions[index]);
        let mode = if entry {
            MirParamMode::Lane
        } else {
            MirParamMode::LaneInherited
        };
        let mut ids = IdGen::from_function(&self.functions[index]);
        let local = ids.next_local();
        let function = &mut self.functions[index];
        function.params.push(MirParam {
            local,
            name: fragment_lane.name,
            ty: fragment_lane.ty,
            mode,
            span: fragment_lane.span,
        });
        function.locals.push(MirLocal {
            id: local,
            name: fragment_lane.name,
            ty: fragment_lane.ty,
            mutable: false,
            span: fragment_lane.span,
        });
    }

    fn inline_call(&mut self, caller_id: MirFunctionId, site: &CallSite, callee: &MirFunction) -> Option<InlineBlocks> {
        let caller_index = self
            .functions
            .iter()
            .position(|function| function.id == caller_id)?;
        if callee.blocks.is_empty() {
            return None;
        }
        let caller_lane_local = self
            .function(caller_id)
            .and_then(lane_param)
            .map(|param| param.local);
        let target = &mut self.functions[caller_index];
        if site.block_index >= target.blocks.len() {
            return None;
        }
        let try_route = try_route_of(target, site);
        if site.is_try_call && try_route.is_none() {
            return None;
        }
        let stmt_index = match site.stmt_index {
            Some(index) => index,
            None if try_route.is_some() => target.blocks[site.block_index].statements.len(),
            None => return None,
        };
        if try_route.is_none() && stmt_index >= target.blocks[site.block_index].statements.len() {
            return None;
        }
        let mut ids = IdGen::from_function(target);
        let mut alpha = Alpha::default();
        let mut binds = Vec::new();
        // GEA3-A2b: an `in` (MutRef) parameter carries mutating ownership, so
        // its binding may not terminate at a fresh immutable clone — the
        // callee's writes must reach the caller's actual place. A bare caller
        // local aliases in place (the runner's MutRef place-alias contract);
        // any other place, e.g. an indexed `buf[i]`, binds a fresh *mutable*
        // local that writes back to the caller place once the callee body
        // finishes.
        let mut mut_writebacks: Vec<(MirPlace, MirLocalId, MirType)> = Vec::new();
        for (param, arg) in callee
            .params
            .iter()
            .filter(|param| !is_lane_param(param))
            .zip(site.args.iter())
        {
            let mut mut_target = None;
            if param.mode == MirParamMode::MutRef
                && let MirOperand::Place(place) = arg
            {
                if place.projections.is_empty()
                    && let MirPlaceBase::Local(caller_local) = place.base
                {
                    alpha.locals.insert(param.local, caller_local);
                    continue;
                }
                mut_target = Some((place.clone(), param.ty));
            }
            let new_local = ids.next_local();
            alpha.locals.insert(param.local, new_local);
            target.locals.push(MirLocal {
                id: new_local,
                name: param.name,
                ty: param.ty,
                mutable: mut_target.is_some(),
                span: param.span,
            });
            binds.push(MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace::local(new_local),
                    value: MirValue {
                        id: ids.next_value(),
                        kind: MirValueKind::Operand(arg.clone()),
                        ty: param.ty,
                        span: site.span,
                    },
                },
                span: site.span,
            });
            if let Some((place, ty)) = mut_target {
                mut_writebacks.push((place, new_local, ty));
            }
        }
        Self::rename_inline_locals(target, &mut alpha, &mut ids, callee, caller_lane_local);
        Self::emit_inline(target, site, callee, &mut alpha, &mut ids, binds, &mut_writebacks)
    }

    /// Alpha-rename the callee's lane binding, then every callee local and
    /// temp the argument binding pass did not already map.
    fn rename_inline_locals(
        target: &mut MirFunction,
        alpha: &mut Alpha,
        ids: &mut IdGen,
        callee: &MirFunction,
        caller_lane_local: Option<MirLocalId>,
    ) {
        if let Some(fragment_lane) = lane_param(callee)
            && let Some(caller_lane_local) = caller_lane_local
        {
            alpha.locals.insert(fragment_lane.local, caller_lane_local);
        }
        for local in &callee.locals {
            if alpha.locals.contains_key(&local.id) {
                continue;
            }
            let new_local = ids.next_local();
            alpha.locals.insert(local.id, new_local);
            target.locals.push(MirLocal {
                id: new_local,
                name: local.name,
                ty: local.ty,
                mutable: local.mutable,
                span: local.span,
            });
        }
        for temp in &callee.temps {
            let new_temp = ids.next_temp();
            alpha.temps.insert(temp.id, new_temp);
            target
                .temps
                .push(MirTemp { id: new_temp, ty: temp.ty, span: temp.span });
        }
    }

    /// Splice the alpha-renamed callee body into `target`'s call block,
    /// connecting the cloned return edges to the caller continuation.
    fn emit_inline(
        target: &mut MirFunction,
        site: &CallSite,
        callee: &MirFunction,
        alpha: &mut Alpha,
        ids: &mut IdGen,
        binds: Vec<MirStatement>,
        mut_writebacks: &[(MirPlace, MirLocalId, MirType)],
    ) -> Option<InlineBlocks> {
        let try_route = try_route_of(target, site);
        let stmt_index = match site.stmt_index {
            Some(index) => index,
            None if try_route.is_some() => target.blocks[site.block_index].statements.len(),
            None => return None,
        };
        let dest_ty = site
            .destination
            .as_ref()
            .and_then(|place| place_ty(target, place))
            .unwrap_or(callee.return_ty);
        let block = &mut target.blocks[site.block_index];
        if try_route.is_none() && stmt_index >= block.statements.len() {
            return None;
        }
        let mut tail = Vec::new();
        if try_route.is_none() {
            tail = block.statements.split_off(stmt_index);
            if tail.is_empty() || !matches!(tail[0].kind, MirStatementKind::Call { .. }) {
                block.statements.append(&mut tail);
                return None;
            }
            tail.remove(0);
        }
        // CDC-3: a single-block Return-terminated callee splices directly
        // into the caller block — arg binds, remapped statements, result
        // assign, then the caller's own tail and original terminator. No
        // entry Goto, no continue block, no fresh block ids, so the composed
        // caller keeps the single-block shape the device-safe gate admits.
        if try_route.is_none()
            && callee.blocks.len() == 1
            && matches!(callee.blocks[0].terminator.kind, MirTerminatorKind::Return(_))
        {
            let spliced = Self::direct_splice_statements(&callee.blocks[0], site, dest_ty, alpha, ids, mut_writebacks);
            block.statements.extend(binds);
            block.statements.extend(spliced);
            block.statements.extend(tail);
            return Some(InlineBlocks { cloned: Vec::new(), continue_id: None });
        }
        for block in &callee.blocks {
            alpha.blocks.insert(block.id, ids.next_block());
        }
        // A failable call continues in its own `ok` block; the callee's
        // throws leave through the caller's error edge.
        let continue_id = match &try_route {
            Some(route) => route.ok_block,
            None => ids.next_block(),
        };
        let cloned_entry = *alpha.blocks.get(&callee.blocks[0].id)?;
        let mut cloned = Vec::new();
        let mut cloned_ids = Vec::new();
        for block in &callee.blocks {
            let mapped = remap_and_connect(
                block,
                alpha,
                ids,
                site.destination.as_ref(),
                continue_id,
                dest_ty,
                mut_writebacks,
                try_route
                    .as_ref()
                    .map(|route| (route, callee.error_ty.unwrap_or(callee.return_ty))),
            );
            cloned_ids.push(mapped.id);
            cloned.push(mapped);
        }
        let old_term = std::mem::replace(
            &mut block.terminator,
            MirTerminator { kind: MirTerminatorKind::Goto(cloned_entry), span: site.span },
        );
        block.statements.extend(binds);
        if let Some(route) = &try_route {
            target.blocks.extend(cloned);
            if route.propagates {
                drop_unreferenced_block(target, route.error_block);
            }
            return Some(InlineBlocks { cloned: cloned_ids, continue_id: None });
        }
        target
            .blocks
            .push(MirBlock { id: continue_id, statements: tail, terminator: old_term, span: site.span });
        target.blocks.extend(cloned);
        Some(InlineBlocks { cloned: cloned_ids, continue_id: Some(continue_id) })
    }

    /// Statements for the CDC-3 direct splice: the remapped callee body,
    /// mut-output writebacks, and the result assign.
    fn direct_splice_statements(
        callee_block: &MirBlock,
        site: &CallSite,
        dest_ty: MirType,
        alpha: &mut Alpha,
        ids: &mut IdGen,
        mut_writebacks: &[(MirPlace, MirLocalId, MirType)],
    ) -> Vec<MirStatement> {
        let mut spliced: Vec<_> = callee_block
            .statements
            .iter()
            .map(|stmt| remap_stmt(stmt, alpha, ids))
            .collect();
        // GEA3-A2b: publish mut-output clones after the callee body and
        // before the result assign, so a destination overlapping a mut
        // place keeps call semantics (param writes precede the
        // return-value assign).
        for (place, local, ty) in mut_writebacks {
            spliced.push(writeback_stmt(place, *local, *ty, callee_block.terminator.span, ids));
        }
        if let (Some(dest), MirTerminatorKind::Return(Some(operand))) =
            (site.destination.as_ref(), &callee_block.terminator.kind)
        {
            spliced.push(MirStatement {
                kind: MirStatementKind::Assign {
                    place: dest.clone(),
                    value: MirValue {
                        id: ids.next_value(),
                        kind: MirValueKind::Operand(alpha.operand(operand, ids)),
                        ty: dest_ty,
                        span: callee_block.terminator.span,
                    },
                },
                span: callee_block.terminator.span,
            });
        }
        spliced
    }

    fn push_expansion(&mut self, reason: &'static str, span: Span, observed: u32, limit: u32, chain: &[MirFunctionId]) {
        // The innermost chain link is the callee that tripped the budget; its
        // definition is the secondary span. A one-link chain names only the
        // caller, which has no callee to point at.
        let definition_span = match chain {
            [_, .., callee] => self.function(*callee).map(|function| function.span),
            _ => None,
        };
        let chain = self.chain_labels(chain);
        let mut diagnostic = error(
            ISSUE_EXPANSION,
            reason,
            span,
            vec![
                DiagnosticArg::new("kind", reason),
                DiagnosticArg::new("observed", observed.to_string()),
                DiagnosticArg::new("limit", limit.to_string()),
                DiagnosticArg::new("chain", &chain),
            ],
        );
        diagnostic.definition_span = definition_span;
        self.errors.push(diagnostic);
    }

    fn chain_labels(&self, ids: &[MirFunctionId]) -> String {
        ids.iter()
            .map(|id| {
                self.function(*id)
                    .map_or_else(|| format!("fn#{}", id.0), |function| function_label(function, self.interner))
            })
            .collect::<Vec<_>>()
            .join(" → ")
    }

    fn resolver(&self) -> Resolver<'_> {
        Resolver {
            functions: self.functions,
            monomorphizations: self.monomorphizations,
            roles: self.roles,
            metadata: self.metadata,
        }
    }

    fn function(&self, id: MirFunctionId) -> Option<&MirFunction> {
        self.functions.iter().find(|function| function.id == id)
    }

    fn is_device_callee(&self, function: &MirFunction) -> bool {
        matches!(
            role_of(function, self.roles, self.metadata),
            RoleKind::Entry | RoleKind::Fragment | RoleKind::SyntheticKernel
        )
    }

    /// Expansion roots: launch entries plus synthetic kernel closures (their
    /// own device calls compose inside the region body).
    fn is_entry(&self, function: &MirFunction) -> bool {
        matches!(
            role_of(function, self.roles, self.metadata),
            RoleKind::Entry | RoleKind::SyntheticKernel
        )
    }
}

struct Resolver<'a> {
    functions: &'a [MirFunction],
    monomorphizations: &'a [MirMonomorphization],
    roles: &'a FxHashMap<DefId, MirDeviceRole>,
    metadata: &'a MirDeviceFunctionMetadataMap,
}

impl Resolver<'_> {
    fn has_device_call(&self, function: &MirFunction) -> bool {
        !self.device_sites(function).is_empty()
    }

    /// Device call sites in one function: calls whose callee resolves to a
    /// device function (`@ nucleum` compute or fragment sugar, entry or
    /// helper). Collected regardless of the containing function's role; the
    /// pass only expands device callers, so host-boundary call sites stay
    /// untouched.
    fn device_sites(&self, function: &MirFunction) -> Vec<CallSite> {
        Self::all_callees(function)
            .into_iter()
            .filter(|site| self.is_device_callee(&site.callee))
            .collect()
    }

    /// Every call site in one function, device or not (prune reference scan).
    fn all_callees(function: &MirFunction) -> Vec<CallSite> {
        let mut sites = Vec::new();
        for (block_index, block) in function.blocks.iter().enumerate() {
            for (stmt_index, stmt) in block.statements.iter().enumerate() {
                if let MirStatementKind::Call { destination, callee, args } = &stmt.kind {
                    sites.push(CallSite {
                        block_index,
                        block_id: block.id,
                        stmt_index: Some(stmt_index),
                        is_try_call: false,
                        destination: destination.clone(),
                        callee: callee.clone(),
                        args: args.clone(),
                        span: stmt.span,
                    });
                }
            }
            if let MirTerminatorKind::TryCall { destination, callee, args, .. } = &block.terminator.kind {
                sites.push(CallSite {
                    block_index,
                    block_id: block.id,
                    stmt_index: None,
                    is_try_call: true,
                    destination: destination.clone(),
                    callee: callee.clone(),
                    args: args.clone(),
                    span: block.terminator.span,
                });
            }
        }
        sites
    }

    fn is_device_callee(&self, callee: &MirCallee) -> bool {
        let is_device = |function: &MirFunction| {
            matches!(
                role_of(function, self.roles, self.metadata),
                RoleKind::Entry | RoleKind::Fragment | RoleKind::SyntheticKernel
            )
        };
        match self.resolve_function(callee) {
            Some(function) => is_device(function),
            // A generic call whose type args match no instance of a device
            // source is still a device call: it stays a site so expansion
            // reports it `unresolved` instead of leaving it behind unchecked.
            None => matches!(callee, MirCallee::Definition { source, .. }
                if self.functions.iter().any(|function| function.source == Some(*source) && is_device(function))),
        }
    }

    fn resolve_function(&self, callee: &MirCallee) -> Option<&MirFunction> {
        match callee {
            MirCallee::Function(id) => self.functions.iter().find(|function| function.id == *id),
            MirCallee::Definition { source, type_args } => self
                .monomorphizations
                .iter()
                .find(|mono| mono.key.source == *source && (type_args.is_empty() || &mono.key.type_args == type_args))
                .and_then(|mono| {
                    self.functions
                        .iter()
                        .find(|function| function.id == mono.function)
                })
                .or_else(|| {
                    // Only a callee without explicit type args names its
                    // source plainly; explicit args that match no
                    // monomorphization are unresolved, never an arbitrary
                    // same-source function.
                    if !type_args.is_empty() {
                        return None;
                    }
                    self.functions
                        .iter()
                        .find(|function| function.source == Some(*source))
                }),
            MirCallee::Closure(_) | MirCallee::Value(_) => None,
        }
    }
}

#[derive(Clone)]
struct CallSite {
    block_index: usize,
    block_id: MirBlockId,
    stmt_index: Option<usize>,
    is_try_call: bool,
    destination: Option<MirPlace>,
    callee: MirCallee,
    args: Vec<MirOperand>,
    span: Span,
}

struct InlineBlocks {
    /// Fresh block ids the splice added (cloned callee blocks).
    cloned: Vec<MirBlockId>,
    /// The block control continues in after a Goto-join splice; `None` when
    /// the splice added no blocks (the caller's own block continues in
    /// place).
    continue_id: Option<MirBlockId>,
}

/// Call-nesting context of one statement (or terminator): how many calls
/// deep the code sits, and the call chain that put it there.
#[derive(Clone)]
struct Nesting {
    depth: u32,
    chain: Rc<Vec<MirFunctionId>>,
    /// Root call site this code is charged to; 0 for the entry's own
    /// statements before any root site has been allocated.
    root_site: u32,
}

/// Per-block nesting, parallel to the block's statements plus its terminator.
struct BlockNesting {
    statements: Vec<Nesting>,
    terminator: Nesting,
}

impl BlockNesting {
    fn root(statements: usize, root: &Rc<Vec<MirFunctionId>>) -> Self {
        let at_root = Nesting { depth: 0, chain: Rc::clone(root), root_site: 0 };
        Self { statements: vec![at_root.clone(); statements], terminator: at_root }
    }

    /// The nesting of a call site: its statement, or the terminator for a
    /// `TryCall`.
    fn site(&self, stmt_index: Option<usize>) -> Option<&Nesting> {
        match stmt_index {
            Some(index) => self.statements.get(index),
            None => Some(&self.terminator),
        }
    }
}

/// Update the per-statement nesting after `site` was inlined. Only the
/// statements the splice introduced take `spliced`; the caller's own tail and
/// terminator keep the nesting they already had.
fn splice_nesting(
    nesting: &mut FxHashMap<MirBlockId, BlockNesting>,
    caller: &MirFunction,
    site: &CallSite,
    new_blocks: &InlineBlocks,
    spliced: &Nesting,
) {
    let Some(block) = caller.blocks.get(site.block_index) else {
        return;
    };
    let Some(entry) = nesting.get_mut(&site.block_id) else {
        return;
    };
    let Some(stmt_index) = site.stmt_index else {
        // Failable call: the block's `TryCall` became a jump into the cloned
        // callee blocks; only the argument binds were appended to its head.
        let binds = block
            .statements
            .len()
            .saturating_sub(entry.statements.len());
        entry.statements.extend(vec![spliced.clone(); binds]);
        entry.terminator = spliced.clone();
        nest_cloned_blocks(nesting, caller, &new_blocks.cloned, spliced);
        return;
    };
    let inserted = |kept: usize| vec![spliced.clone(); block.statements.len().saturating_sub(kept)];
    match new_blocks.continue_id {
        // No-new-blocks splice: binds and the callee body replace the call
        // statement in place; the caller's tail follows unchanged.
        None => {
            let tail = entry.statements.split_off(stmt_index + 1);
            entry.statements.truncate(stmt_index);
            let kept = stmt_index + tail.len();
            entry.statements.extend(inserted(kept));
            entry.statements.extend(tail);
        }
        // Goto-join splice: the block keeps its head and the argument binds,
        // the caller's tail and terminator move to the continue block, and
        // the cloned callee blocks sit at the spliced nesting.
        Some(continue_id) => {
            let mut tail = entry.statements.split_off(stmt_index);
            tail.remove(0);
            entry.statements.truncate(stmt_index);
            entry.statements.extend(inserted(stmt_index));
            let old_terminator = std::mem::replace(&mut entry.terminator, spliced.clone());
            nesting.insert(continue_id, BlockNesting { statements: tail, terminator: old_terminator });
            nest_cloned_blocks(nesting, caller, &new_blocks.cloned, spliced);
        }
    }
}

/// Register the cloned callee blocks at the spliced nesting: every statement
/// and the terminator sit one call deeper than the site they came from.
fn nest_cloned_blocks(
    nesting: &mut FxHashMap<MirBlockId, BlockNesting>,
    caller: &MirFunction,
    cloned: &[MirBlockId],
    spliced: &Nesting,
) {
    for cloned_id in cloned {
        let len = caller
            .blocks
            .iter()
            .find(|block| block.id == *cloned_id)
            .map_or(0, |block| block.statements.len());
        nesting.insert(
            *cloned_id,
            BlockNesting { statements: vec![spliced.clone(); len], terminator: spliced.clone() },
        );
    }
}

struct Account {
    nodes: u32,
    instances: u32,
    /// Per-caller expansion steps; the key omits the caller id because one
    /// `Account` never spans two expansion roots.
    sites: FxHashMap<(MirFunctionId, MirBlockId, u32), u32>,
    /// Next root call-site id; ids stay stable for the life of the account so
    /// nested sites can charge back to the site that pulled them in.
    next_root: u32,
    /// Cloned nodes charged to each root call site and everything it pulled in.
    root_nodes: FxHashMap<u32, u32>,
}

impl Account {
    fn new() -> Self {
        Self { nodes: 0, instances: 0, sites: FxHashMap::default(), next_root: 0, root_nodes: FxHashMap::default() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoleKind {
    Entry,
    Fragment,
    /// A source-less synthetic kernel closure: device-side for composition
    /// (its direct call composes into kernel callers, and it composes its
    /// own device calls), never a launch entry. Classified only through the
    /// explicit metadata carrier — never from `source: None`, a name, or the
    /// compute stage.
    SyntheticKernel,
    Other,
}

fn role_of(
    function: &MirFunction,
    roles: &FxHashMap<DefId, MirDeviceRole>,
    metadata: &MirDeviceFunctionMetadataMap,
) -> RoleKind {
    if let Some(source) = function.source {
        match roles.get(&source) {
            Some(MirDeviceRole::Kernel) => return RoleKind::Entry,
            Some(MirDeviceRole::Fragment) => return RoleKind::Fragment,
            None => {}
        }
    }
    if metadata
        .get(&function.id)
        .is_some_and(|metadata| metadata.origin == MirFunctionOrigin::SyntheticKernelClosure)
    {
        return RoleKind::SyntheticKernel;
    }
    RoleKind::Other
}

fn function_label(function: &MirFunction, interner: Option<&Interner>) -> String {
    if let (Some(interner), Some(name)) = (interner, function.name) {
        let resolved = interner.resolve(name);
        if !resolved.is_empty() {
            return resolved.to_owned();
        }
    }
    match function.source {
        Some(def) => format!("#{}", def.0),
        None => format!("fn#{}", function.id.0),
    }
}

fn normalize_cycle(cycle: &[MirFunctionId]) -> Vec<MirFunctionId> {
    if cycle.len() < 2 {
        return cycle.to_vec();
    }
    let body = &cycle[..cycle.len() - 1];
    let min_at = body
        .iter()
        .enumerate()
        .min_by_key(|(_, id)| id.0)
        .map_or(0, |(index, _)| index);
    let mut rotated = body[min_at..].to_vec();
    rotated.extend_from_slice(&body[..min_at]);
    if let Some(first) = rotated.first().copied() {
        rotated.push(first);
    }
    rotated
}

fn is_lane_param(param: &MirParam) -> bool {
    matches!(param.mode, MirParamMode::Lane | MirParamMode::LaneInherited)
}

fn lane_param(function: &MirFunction) -> Option<&MirParam> {
    function.params.iter().find(|param| is_lane_param(param))
}

fn error(issue: &'static str, reason: &'static str, span: Span, args: Vec<DiagnosticArg>) -> FragmentCompositionError {
    FragmentCompositionError { issue, reason, span, definition_span: None, args }
}

/// `usize` → `u32` for budget accounting: a value past `u32::MAX` counts as
/// the ceiling rather than wrapping.
fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn count_nodes(function: &MirFunction) -> u32 {
    let stmts = function
        .blocks
        .iter()
        .map(|block| saturating_u32(block.statements.len()) + 1)
        .sum::<u32>();
    stmts
        + saturating_u32(function.locals.len())
        + saturating_u32(function.temps.len())
        + saturating_u32(function.blocks.len())
}

fn place_ty(function: &MirFunction, place: &MirPlace) -> Option<MirType> {
    match place.base {
        MirPlaceBase::Local(id) => function
            .params
            .iter()
            .find(|param| param.local == id)
            .map(|param| param.ty)
            .or_else(|| {
                function
                    .locals
                    .iter()
                    .find(|local| local.id == id)
                    .map(|local| local.ty)
            }),
        MirPlaceBase::Temp(id) => function.temp_ty(id),
    }
}

#[derive(Default)]
struct Alpha {
    locals: FxHashMap<MirLocalId, MirLocalId>,
    temps: FxHashMap<MirTempId, MirTempId>,
    blocks: FxHashMap<MirBlockId, MirBlockId>,
    values: FxHashMap<MirValueId, MirValueId>,
}

struct IdGen {
    local: u32,
    temp: u32,
    block: u32,
    value: u32,
}

impl IdGen {
    fn from_function(function: &MirFunction) -> Self {
        let mut r#gen = Self { local: 0, temp: 0, block: 0, value: 0 };
        for param in &function.params {
            r#gen.local = r#gen.local.max(param.local.0.saturating_add(1));
        }
        for local in &function.locals {
            r#gen.local = r#gen.local.max(local.id.0.saturating_add(1));
        }
        for temp in &function.temps {
            r#gen.temp = r#gen.temp.max(temp.id.0.saturating_add(1));
        }
        for block in &function.blocks {
            r#gen.block = r#gen.block.max(block.id.0.saturating_add(1));
            for stmt in &block.statements {
                if let MirStatementKind::Assign { value, .. } = &stmt.kind {
                    r#gen.value = r#gen.value.max(value.id.0.saturating_add(1));
                }
            }
        }
        r#gen
    }

    fn next_local(&mut self) -> MirLocalId {
        let id = MirLocalId(self.local);
        self.local += 1;
        id
    }

    fn next_temp(&mut self) -> MirTempId {
        let id = MirTempId(self.temp);
        self.temp += 1;
        id
    }

    fn next_block(&mut self) -> MirBlockId {
        let id = MirBlockId(self.block);
        self.block += 1;
        id
    }

    fn next_value(&mut self) -> MirValueId {
        let id = MirValueId(self.value);
        self.value += 1;
        id
    }
}

impl Alpha {
    fn local(&self, id: MirLocalId) -> MirLocalId {
        self.locals.get(&id).copied().unwrap_or(id)
    }

    fn temp(&self, id: MirTempId) -> MirTempId {
        self.temps.get(&id).copied().unwrap_or(id)
    }

    fn block(&self, id: MirBlockId) -> MirBlockId {
        self.blocks.get(&id).copied().unwrap_or(id)
    }

    fn value(&mut self, id: MirValueId, ids: &mut IdGen) -> MirValueId {
        *self.values.entry(id).or_insert_with(|| ids.next_value())
    }

    fn operand(&mut self, operand: &MirOperand, ids: &mut IdGen) -> MirOperand {
        match operand {
            MirOperand::Place(place) => MirOperand::Place(self.place(place, ids)),
            MirOperand::Temp(id) => MirOperand::Temp(self.temp(*id)),
            MirOperand::Value(id) => MirOperand::Value(self.value(*id, ids)),
            MirOperand::Constant(constant) => MirOperand::Constant(constant.clone()),
        }
    }

    fn place(&mut self, place: &MirPlace, ids: &mut IdGen) -> MirPlace {
        MirPlace {
            base: match place.base {
                MirPlaceBase::Local(id) => MirPlaceBase::Local(self.local(id)),
                MirPlaceBase::Temp(id) => MirPlaceBase::Temp(self.temp(id)),
            },
            projections: place
                .projections
                .iter()
                .map(|projection| match projection {
                    MirProjection::Index(operand) => MirProjection::Index(self.operand(operand, ids)),
                    MirProjection::ProvenIndex(operand) => MirProjection::ProvenIndex(self.operand(operand, ids)),
                    other => other.clone(),
                })
                .collect(),
        }
    }

    fn opt_place(&mut self, place: Option<&MirPlace>, ids: &mut IdGen) -> Option<MirPlace> {
        place.map(|place| self.place(place, ids))
    }

    fn callee(&mut self, callee: &MirCallee, ids: &mut IdGen) -> MirCallee {
        match callee {
            MirCallee::Function(id) => MirCallee::Function(*id),
            MirCallee::Definition { source, type_args } => {
                MirCallee::Definition { source: *source, type_args: type_args.clone() }
            }
            MirCallee::Closure(closure) => {
                let mut closure = closure.clone();
                closure.environment = self.operand(&closure.environment, ids);
                MirCallee::Closure(closure)
            }
            MirCallee::Value(operand) => MirCallee::Value(self.operand(operand, ids)),
        }
    }

    fn value_node(&mut self, value: &MirValue, ids: &mut IdGen) -> MirValue {
        MirValue {
            id: self.value(value.id, ids),
            kind: match &value.kind {
                MirValueKind::Operand(operand) => MirValueKind::Operand(self.operand(operand, ids)),
                MirValueKind::Closure(closure) => {
                    let mut closure = closure.clone();
                    closure.environment = self.operand(&closure.environment, ids);
                    MirValueKind::Closure(closure)
                }
                MirValueKind::Unary { op, operand } => {
                    MirValueKind::Unary { op: *op, operand: self.operand(operand, ids) }
                }
                MirValueKind::Binary { op, lhs, rhs } => {
                    MirValueKind::Binary { op: *op, lhs: self.operand(lhs, ids), rhs: self.operand(rhs, ids) }
                }
                MirValueKind::Option(op) => MirValueKind::Option(self.option_op(op, ids)),
                MirValueKind::TensorExtent { tensor, axis } => {
                    MirValueKind::TensorExtent { tensor: self.operand(tensor, ids), axis: *axis }
                }
            },
            ty: value.ty,
            span: value.span,
        }
    }

    fn option_op(&mut self, op: &MirOptionOp, ids: &mut IdGen) -> MirOptionOp {
        match op {
            MirOptionOp::None => MirOptionOp::None,
            MirOptionOp::Some(operand) => MirOptionOp::Some(self.operand(operand, ids)),
            MirOptionOp::IsNil(operand) => MirOptionOp::IsNil(self.operand(operand, ids)),
            MirOptionOp::IsNotNil(operand) => MirOptionOp::IsNotNil(self.operand(operand, ids)),
            MirOptionOp::Unwrap { value, mode } => MirOptionOp::Unwrap { value: self.operand(value, ids), mode: *mode },
            MirOptionOp::Coalesce { value, fallback } => {
                MirOptionOp::Coalesce { value: self.operand(value, ids), fallback: self.operand(fallback, ids) }
            }
            MirOptionOp::Chain { base, link } => {
                MirOptionOp::Chain { base: self.operand(base, ids), link: self.link(link, ids) }
            }
        }
    }

    fn link(&mut self, link: &MirOptionChainLink, ids: &mut IdGen) -> MirOptionChainLink {
        match link {
            MirOptionChainLink::Index(operand) => MirOptionChainLink::Index(self.operand(operand, ids)),
            MirOptionChainLink::Call { callee, args } => MirOptionChainLink::Call {
                callee: self.callee(callee, ids),
                args: args.iter().map(|arg| self.operand(arg, ids)).collect(),
            },
            other => other.clone(),
        }
    }

    fn aggregate(&mut self, aggregate: &MirAggregate, ids: &mut IdGen) -> MirAggregate {
        MirAggregate {
            kind: aggregate.kind.clone(),
            ty: aggregate.ty,
            fields: match &aggregate.fields {
                MirAggregateFields::Ordered(items) => MirAggregateFields::Ordered(
                    items
                        .iter()
                        .map(|item| match item {
                            MirAggregateItem::Operand(operand) => MirAggregateItem::Operand(self.operand(operand, ids)),
                            MirAggregateItem::Spread(operand) => MirAggregateItem::Spread(self.operand(operand, ids)),
                        })
                        .collect(),
                ),
                MirAggregateFields::Named(items) => MirAggregateFields::Named(
                    items
                        .iter()
                        .map(|item| MirNamedOperand { name: item.name, value: self.operand(&item.value, ids) })
                        .collect(),
                ),
                MirAggregateFields::Keyed(items) => MirAggregateFields::Keyed(
                    items
                        .iter()
                        .map(|item| MirKeyValueOperand {
                            key: self.operand(&item.key, ids),
                            value: self.operand(&item.value, ids),
                        })
                        .collect(),
                ),
            },
        }
    }

    fn runtime(&mut self, call: &MirRuntimeCall, ids: &mut IdGen) -> MirRuntimeCall {
        MirRuntimeCall {
            intrinsic: self.intrinsic(&call.intrinsic, ids),
            args: call.args.iter().map(|arg| self.operand(arg, ids)).collect(),
            return_ty: call.return_ty,
        }
    }

    fn intrinsic(&mut self, intrinsic: &MirIntrinsic, ids: &mut IdGen) -> MirIntrinsic {
        match intrinsic {
            MirIntrinsic::Convert(conversion) => MirIntrinsic::Convert(self.conversion(conversion, ids)),
            MirIntrinsic::FilumReduce(filum) => MirIntrinsic::FilumReduce(self.filum_reduce(filum, ids)),
            other => other.clone(),
        }
    }

    /// A declared `filum` carries its own binder locals and the captured term
    /// body, all in the enclosing function's id space, so cloning the node into
    /// a caller renames them with the rest of the callee. The term-function
    /// form (`term_captures` set) names the term function's own parameter
    /// locals instead, which the clone does not touch.
    fn filum_reduce(&mut self, filum: &MirFilumReduceOp, ids: &mut IdGen) -> MirFilumReduceOp {
        let mut filum = filum.clone();
        if filum.term_captures.is_some() {
            return filum;
        }
        filum.term_binder = self.local(filum.term_binder);
        filum.term_result = self.local(filum.term_result);
        for coordinate in &mut filum.coordinates {
            coordinate.local = self.local(coordinate.local);
        }
        filum.term_body = filum
            .term_body
            .iter()
            .map(|stmt| remap_stmt(stmt, self, ids))
            .collect();
        filum
    }

    fn conversion(&mut self, conversion: &MirConversion, ids: &mut IdGen) -> MirConversion {
        let mut conversion = conversion.clone();
        conversion.recovery = conversion
            .recovery
            .as_ref()
            .map(|operand| self.operand(operand, ids));
        conversion.struct_defaults = conversion
            .struct_defaults
            .iter()
            .map(|defaults| radix_mir::MirStructFieldDefaults {
                def_id: defaults.def_id,
                fields: defaults
                    .fields
                    .iter()
                    .map(|field| MirNamedOperand { name: field.name, value: self.operand(&field.value, ids) })
                    .collect(),
            })
            .collect();
        conversion
    }
}

fn remap_block(block: &MirBlock, alpha: &mut Alpha, ids: &mut IdGen) -> MirBlock {
    MirBlock {
        id: alpha.block(block.id),
        statements: block
            .statements
            .iter()
            .map(|stmt| remap_stmt(stmt, alpha, ids))
            .collect(),
        terminator: remap_terminator(&block.terminator, alpha, ids),
        span: block.span,
    }
}

fn remap_stmt(stmt: &MirStatement, alpha: &mut Alpha, ids: &mut IdGen) -> MirStatement {
    MirStatement {
        kind: match &stmt.kind {
            MirStatementKind::Assign { place, value } => {
                MirStatementKind::Assign { place: alpha.place(place, ids), value: alpha.value_node(value, ids) }
            }
            MirStatementKind::Call { destination, callee, args } => MirStatementKind::Call {
                destination: alpha.opt_place(destination.as_ref(), ids),
                callee: alpha.callee(callee, ids),
                args: args.iter().map(|arg| alpha.operand(arg, ids)).collect(),
            },
            MirStatementKind::RuntimeCall { destination, call } => MirStatementKind::RuntimeCall {
                destination: alpha.opt_place(destination.as_ref(), ids),
                call: alpha.runtime(call, ids),
            },
            MirStatementKind::Construct { destination, aggregate } => MirStatementKind::Construct {
                destination: alpha.place(destination, ids),
                aggregate: alpha.aggregate(aggregate, ids),
            },
        },
        span: stmt.span,
    }
}

fn remap_terminator(terminator: &MirTerminator, alpha: &mut Alpha, ids: &mut IdGen) -> MirTerminator {
    MirTerminator {
        kind: match &terminator.kind {
            MirTerminatorKind::Return(operand) => {
                MirTerminatorKind::Return(operand.as_ref().map(|operand| alpha.operand(operand, ids)))
            }
            MirTerminatorKind::ReturnThread => MirTerminatorKind::ReturnThread,
            MirTerminatorKind::ReturnError(operand) => MirTerminatorKind::ReturnError(alpha.operand(operand, ids)),
            MirTerminatorKind::TryCall { destination, callee, args, ok_block, error_place, error_block } => {
                MirTerminatorKind::TryCall {
                    destination: alpha.opt_place(destination.as_ref(), ids),
                    callee: alpha.callee(callee, ids),
                    args: args.iter().map(|arg| alpha.operand(arg, ids)).collect(),
                    ok_block: alpha.block(*ok_block),
                    error_place: alpha.place(error_place, ids),
                    error_block: alpha.block(*error_block),
                }
            }
            MirTerminatorKind::Goto(block) => MirTerminatorKind::Goto(alpha.block(*block)),
            MirTerminatorKind::Branch { condition, then_block, else_block } => MirTerminatorKind::Branch {
                condition: alpha.operand(condition, ids),
                then_block: alpha.block(*then_block),
                else_block: alpha.block(*else_block),
            },
            MirTerminatorKind::Switch { value, cases, default } => MirTerminatorKind::Switch {
                value: alpha.operand(value, ids),
                cases: cases
                    .iter()
                    .map(|case| radix_mir::MirSwitchCase {
                        value: case.value.clone(),
                        target: alpha.block(case.target),
                    })
                    .collect(),
                default: alpha.block(*default),
            },
            MirTerminatorKind::Unreachable => MirTerminatorKind::Unreachable,
        },
        span: terminator.span,
    }
}

fn writeback_stmt(place: &MirPlace, local: MirLocalId, ty: MirType, span: Span, ids: &mut IdGen) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place: place.clone(),
            value: MirValue {
                id: ids.next_value(),
                kind: MirValueKind::Operand(MirOperand::Place(MirPlace::local(local))),
                ty,
                span,
            },
        },
        span,
    }
}

fn rewrite_return_into(
    block: &mut MirBlock,
    destination: Option<&MirPlace>,
    continue_id: MirBlockId,
    dest_ty: MirType,
    ids: &mut IdGen,
    mut_writebacks: &[(MirPlace, MirLocalId, MirType)],
    error_route: Option<(&TryRoute, MirType)>,
) {
    match &block.terminator.kind {
        MirTerminatorKind::Return(operand) => {
            // GEA3-A2b: publish mut-output clones at the return edge, before
            // the result assign, so param writes precede the return-value
            // assign exactly as the uncomposed call ordered them.
            for (place, local, ty) in mut_writebacks {
                block
                    .statements
                    .push(writeback_stmt(place, *local, *ty, block.terminator.span, ids));
            }
            if let (Some(dest), Some(operand)) = (destination, operand.clone()) {
                block.statements.push(MirStatement {
                    kind: MirStatementKind::Assign {
                        place: dest.clone(),
                        value: MirValue {
                            id: ids.next_value(),
                            kind: MirValueKind::Operand(operand),
                            ty: dest_ty,
                            span: block.terminator.span,
                        },
                    },
                    span: block.terminator.span,
                });
            }
            block.terminator =
                MirTerminator { kind: MirTerminatorKind::Goto(continue_id), span: block.terminator.span };
        }
        MirTerminatorKind::ReturnError(operand) => {
            let span = block.terminator.span;
            let Some((route, error_ty)) = error_route else {
                block.terminator = MirTerminator { kind: MirTerminatorKind::Unreachable, span };
                return;
            };
            // The caller returns the error unchanged: the clone keeps its
            // own `ReturnError`, so a constant text stays a constant throw.
            if route.propagates {
                return;
            }
            let operand = operand.clone();
            block.statements.push(MirStatement {
                kind: MirStatementKind::Assign {
                    place: route.error_place.clone(),
                    value: MirValue { id: ids.next_value(), kind: MirValueKind::Operand(operand), ty: error_ty, span },
                },
                span,
            });
            block.terminator = MirTerminator { kind: MirTerminatorKind::Goto(route.error_block), span };
        }
        _ => {}
    }
}

fn remap_and_connect(
    block: &MirBlock,
    alpha: &mut Alpha,
    ids: &mut IdGen,
    destination: Option<&MirPlace>,
    continue_id: MirBlockId,
    dest_ty: MirType,
    mut_writebacks: &[(MirPlace, MirLocalId, MirType)],
    error_route: Option<(&TryRoute, MirType)>,
) -> MirBlock {
    let mut mapped = remap_block(block, alpha, ids);
    rewrite_return_into(&mut mapped, destination, continue_id, dest_ty, ids, mut_writebacks, error_route);
    mapped
}

/// Where a failable call site continues: its `ok` block and its error edge.
struct TryRoute {
    ok_block: MirBlockId,
    error_place: MirPlace,
    error_block: MirBlockId,
    /// The error block only returns the error place, so a callee throw can
    /// leave the caller directly.
    propagates: bool,
}

/// Remove `target` when no terminator reaches it any more (the error block of
/// a failable call whose callee throws now leave the caller directly).
fn drop_unreferenced_block(function: &mut MirFunction, target: MirBlockId) {
    let referenced = function.blocks.iter().any(|block| {
        let mut hit = false;
        visit_successors(&block.terminator.kind, |successor| hit |= successor == target);
        hit
    });
    if !referenced {
        function.blocks.retain(|block| block.id != target);
    }
}

fn visit_successors(kind: &MirTerminatorKind, mut visit: impl FnMut(MirBlockId)) {
    match kind {
        MirTerminatorKind::Goto(block) => visit(*block),
        MirTerminatorKind::Branch { then_block, else_block, .. } => {
            visit(*then_block);
            visit(*else_block);
        }
        MirTerminatorKind::Switch { cases, default, .. } => {
            for case in cases {
                visit(case.target);
            }
            visit(*default);
        }
        MirTerminatorKind::TryCall { ok_block, error_block, .. } => {
            visit(*ok_block);
            visit(*error_block);
        }
        MirTerminatorKind::Return(_)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::ReturnError(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

/// Whether `operand` reads exactly `place` (a temp operand names its temp slot).
fn operand_is_place(operand: &MirOperand, place: &MirPlace) -> bool {
    match operand {
        MirOperand::Place(read) => read == place,
        MirOperand::Temp(temp) => place.projections.is_empty() && place.base == MirPlaceBase::Temp(*temp),
        _ => false,
    }
}

fn try_route_of(caller: &MirFunction, site: &CallSite) -> Option<TryRoute> {
    if !site.is_try_call {
        return None;
    }
    let MirTerminatorKind::TryCall { ok_block, error_place, error_block, .. } =
        &caller.blocks.get(site.block_index)?.terminator.kind
    else {
        return None;
    };
    let propagates = caller
        .blocks
        .iter()
        .find(|block| block.id == *error_block)
        .is_some_and(|block| {
            block.statements.is_empty()
                && matches!(
                    &block.terminator.kind,
                    MirTerminatorKind::ReturnError(operand) if operand_is_place(operand, error_place)
                )
        });
    Some(TryRoute { ok_block: *ok_block, error_place: error_place.clone(), error_block: *error_block, propagates })
}
