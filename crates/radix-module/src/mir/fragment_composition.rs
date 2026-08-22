//! Nucleum-fragment MIR composition (post-monomorphization).
//!
//! Every call to a `Fragment` is cloned into the caller: parameters bind to
//! call-site values exactly once, locals/blocks/values are alpha-renamed, the
//! cloned return path joins the caller's destination, and source spans stay
//! with the cloned nodes. Cycles are rejected before any clone. Expansion
//! budgets fail closed — there is no "emit a call instead" escape.
//!
//! Documented budget defaults (starting anchors; head-cto confirms on land):
//! - depth 16
//! - 65_536 cloned MIR nodes per `Entry`
//! - 1_024 concrete instances per `Entry`
//! - 1 inline per call site

use crate::diagnostics::DiagnosticArg;
use crate::hir::DefId;
use crate::lexer::{Interner, Span};
use crate::mir::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirBlock, MirBlockId, MirCallee, MirConversion, MirDeviceRole,
    MirFunction, MirFunctionId, MirIntrinsic, MirKeyValueOperand, MirLocal, MirLocalId, MirMonomorphization,
    MirNamedOperand, MirOperand, MirOptionChainLink, MirOptionOp, MirPlace, MirPlaceBase, MirProjection,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTemp, MirTempId, MirTerminator, MirTerminatorKind, MirType,
    MirValue, MirValueId, MirValueKind,
};
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::Cell;

pub const ISSUE_CYCLE: &str = "nucleum_fragment_cycle";
pub const ISSUE_EXPANSION: &str = "nucleum_fragment_expansion";

/// Documented bounded defaults for the composition budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpansionBudget {
    pub max_depth: u32,
    pub max_cloned_nodes_per_entry: u32,
    pub max_concrete_instances: u32,
    pub max_inlines_per_callsite: u32,
}

impl ExpansionBudget {
    pub const DOCUMENTED_DEFAULTS: Self = Self {
        max_depth: 16,
        max_cloned_nodes_per_entry: 65_536,
        max_concrete_instances: 1_024,
        max_inlines_per_callsite: 1,
    };
}

thread_local! {
    static BUDGET_OVERRIDE: Cell<Option<ExpansionBudget>> = const { Cell::new(None) };
}

/// Active budget: test override or the documented defaults.
#[must_use]
pub fn active_budget() -> ExpansionBudget {
    BUDGET_OVERRIDE
        .with(Cell::get)
        .unwrap_or(ExpansionBudget::DOCUMENTED_DEFAULTS)
}

/// One composition diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentCompositionError {
    pub issue: &'static str,
    pub reason: &'static str,
    pub span: Span,
    pub args: Vec<DiagnosticArg>,
}

impl FragmentCompositionError {
    #[must_use]
    pub fn message(&self) -> String {
        crate::diagnostics::semantic_message(
            crate::semantic::SemanticErrorKind::OperandTypesInvalid,
            &self.catalog_args(),
        )
        .unwrap_or_else(|| format!("{}: {}", self.issue, self.reason))
    }

    #[must_use]
    pub fn catalog_args(&self) -> Vec<DiagnosticArg> {
        let mut args = vec![
            DiagnosticArg::new("issue", self.issue),
            DiagnosticArg::new("reason", self.reason),
            DiagnosticArg::new("site", "compose"),
        ];
        args.extend(self.args.iter().cloned());
        args
    }
}

/// Compose every fragment call into its caller. Returns diagnostics; empty is success.
pub fn compose(
    functions: &mut [MirFunction],
    monomorphizations: &[MirMonomorphization],
    roles: &FxHashMap<DefId, MirDeviceRole>,
    budget: ExpansionBudget,
    interner: Option<&Interner>,
) -> Vec<FragmentCompositionError> {
    let mut pass = CompositionPass { functions, monomorphizations, roles, budget, interner, errors: Vec::new() };
    pass.run();
    pass.errors
}

/// Fragment call sites that remain in `functions`. The mutation-test gate.
#[must_use]
pub fn surviving_fragment_calls(
    functions: &[MirFunction],
    monomorphizations: &[MirMonomorphization],
    roles: &FxHashMap<DefId, MirDeviceRole>,
) -> Vec<(MirFunctionId, Span)> {
    let resolver = Resolver { functions, monomorphizations, roles };
    let mut survivors = Vec::new();
    for function in functions {
        for site in resolver.fragment_sites(function) {
            survivors.push((function.id, site.span));
        }
    }
    survivors
}

/// Fail-closed gate: any remaining fragment call is a composition error.
#[must_use]
pub fn reject_surviving_fragment_calls(
    functions: &[MirFunction],
    monomorphizations: &[MirMonomorphization],
    roles: &FxHashMap<DefId, MirDeviceRole>,
) -> Vec<FragmentCompositionError> {
    surviving_fragment_calls(functions, monomorphizations, roles)
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
    functions: &'a mut [MirFunction],
    monomorphizations: &'a [MirMonomorphization],
    roles: &'a FxHashMap<DefId, MirDeviceRole>,
    budget: ExpansionBudget,
    interner: Option<&'a Interner>,
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
        let leftover = self
            .functions
            .iter()
            .filter(|function| self.resolver().has_fragment_call(function))
            .map(|function| function.id)
            .collect::<Vec<_>>();
        for id in leftover {
            self.expand_function(id);
            if !self.errors.is_empty() {
                return;
            }
        }
        self.errors.extend(reject_surviving_fragment_calls(
            self.functions,
            self.monomorphizations,
            self.roles,
        ));
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
            if !self.is_fragment(function) {
                continue;
            }
            let mut edges = Vec::new();
            for site in resolver.fragment_sites(function) {
                if let Some(callee) = resolver.resolve_function(&site.callee) {
                    if self.is_fragment(callee) && !edges.contains(&callee.id) {
                        edges.push(callee.id);
                    }
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
        let mut block_depth = FxHashMap::default();
        let mut block_chain = FxHashMap::default();
        if let Some(function) = self.function(caller_id) {
            let root = vec![caller_id];
            for block in &function.blocks {
                block_depth.insert(block.id, 0);
                block_chain.insert(block.id, root.clone());
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
                    .and_then(|function| resolver.fragment_sites(function).into_iter().next())
            };
            let Some(site) = site else {
                return;
            };
            let Some(callee) = self.resolver().resolve_function(&site.callee).cloned() else {
                self.push_expansion("unresolved", site.span, 1, 0, &[caller_id]);
                return;
            };
            if !self.is_fragment(&callee) {
                return;
            }
            let depth = block_depth
                .get(&site.block_id)
                .copied()
                .unwrap_or(0u32)
                .saturating_add(1);
            let mut chain = block_chain
                .get(&site.block_id)
                .cloned()
                .unwrap_or_else(|| vec![caller_id]);
            chain.push(callee.id);
            if depth > self.budget.max_depth {
                self.push_expansion("depth", site.span, depth, self.budget.max_depth, &chain);
                return;
            }
            let cloned_nodes = count_nodes(&callee) + callee.params.len() as u32;
            if account.nodes.saturating_add(cloned_nodes) > self.budget.max_cloned_nodes_per_entry {
                self.push_expansion(
                    "nodes",
                    site.span,
                    account.nodes.saturating_add(cloned_nodes),
                    self.budget.max_cloned_nodes_per_entry,
                    &chain,
                );
                return;
            }
            if account.instances.saturating_add(1) > self.budget.max_concrete_instances {
                self.push_expansion(
                    "instances",
                    site.span,
                    account.instances.saturating_add(1),
                    self.budget.max_concrete_instances,
                    &chain,
                );
                return;
            }
            let site_key = (
                caller_id,
                callee.id,
                site.block_id,
                site.stmt_index
                    .map(|index| index as u32)
                    .unwrap_or(u32::MAX),
            );
            let inlines = account
                .sites
                .get(&site_key)
                .copied()
                .unwrap_or(0)
                .saturating_add(1);
            if inlines > self.budget.max_inlines_per_callsite {
                self.push_expansion("callsite", site.span, inlines, self.budget.max_inlines_per_callsite, &chain);
                return;
            }
            if site.is_try_call {
                self.push_expansion("failable", site.span, 1, 0, &chain);
                return;
            }
            let new_blocks = match self.inline_call(caller_id, &site, &callee) {
                Some(blocks) => blocks,
                None => {
                    self.push_expansion("survived", site.span, 1, 0, &chain);
                    return;
                }
            };
            account.nodes = account.nodes.saturating_add(cloned_nodes);
            account.instances = account.instances.saturating_add(1);
            account.sites.insert(site_key, inlines);
            for block_id in new_blocks.cloned {
                block_depth.insert(block_id, depth);
                block_chain.insert(block_id, chain.clone());
            }
            block_depth.insert(new_blocks.continue_id, depth.saturating_sub(1));
            block_chain.insert(new_blocks.continue_id, chain[..chain.len() - 1].to_vec());
        }
    }

    fn inline_call(&mut self, caller_id: MirFunctionId, site: &CallSite, callee: &MirFunction) -> Option<InlineBlocks> {
        let caller_index = self
            .functions
            .iter()
            .position(|function| function.id == caller_id)?;
        if callee.blocks.is_empty() {
            return None;
        }
        let caller = &mut self.functions[caller_index];
        if site.block_index >= caller.blocks.len() {
            return None;
        }
        let stmt_index = site.stmt_index?;
        if stmt_index >= caller.blocks[site.block_index].statements.len() {
            return None;
        }
        let mut ids = IdGen::from_function(caller);
        let mut alpha = Alpha::default();
        let mut binds = Vec::new();
        for (param, arg) in callee.params.iter().zip(site.args.iter()) {
            let new_local = ids.next_local();
            alpha.locals.insert(param.local, new_local);
            caller.locals.push(MirLocal {
                id: new_local,
                name: param.name,
                ty: param.ty,
                mutable: false,
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
        }
        for local in &callee.locals {
            if alpha.locals.contains_key(&local.id) {
                continue;
            }
            let new_local = ids.next_local();
            alpha.locals.insert(local.id, new_local);
            caller.locals.push(MirLocal {
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
            caller
                .temps
                .push(MirTemp { id: new_temp, ty: temp.ty, span: temp.span });
        }
        for block in &callee.blocks {
            alpha.blocks.insert(block.id, ids.next_block());
        }
        let continue_id = ids.next_block();
        let cloned_entry = *alpha.blocks.get(&callee.blocks[0].id)?;
        let dest_ty = site
            .destination
            .as_ref()
            .and_then(|place| place_ty(caller, place))
            .unwrap_or(callee.return_ty);
        let mut cloned = Vec::new();
        let mut cloned_ids = Vec::new();
        for block in &callee.blocks {
            let mapped =
                remap_and_connect(block, &mut alpha, &mut ids, site.destination.as_ref(), continue_id, dest_ty);
            cloned_ids.push(mapped.id);
            cloned.push(mapped);
        }
        let block = &mut caller.blocks[site.block_index];
        if stmt_index >= block.statements.len() {
            return None;
        }
        let mut tail = block.statements.split_off(stmt_index);
        if tail.is_empty() || !matches!(tail[0].kind, MirStatementKind::Call { .. }) {
            block.statements.append(&mut tail);
            return None;
        }
        tail.remove(0);
        let old_term = std::mem::replace(
            &mut block.terminator,
            MirTerminator { kind: MirTerminatorKind::Goto(cloned_entry), span: site.span },
        );
        block.statements.extend(binds);
        caller
            .blocks
            .push(MirBlock { id: continue_id, statements: tail, terminator: old_term, span: site.span });
        caller.blocks.extend(cloned);
        Some(InlineBlocks { cloned: cloned_ids, continue_id })
    }

    fn push_expansion(&mut self, reason: &'static str, span: Span, observed: u32, limit: u32, chain: &[MirFunctionId]) {
        let chain = self.chain_labels(chain);
        self.errors.push(error(
            ISSUE_EXPANSION,
            reason,
            span,
            vec![
                DiagnosticArg::new("kind", reason),
                DiagnosticArg::new("observed", observed.to_string()),
                DiagnosticArg::new("limit", limit.to_string()),
                DiagnosticArg::new("chain", &chain),
            ],
        ));
    }

    fn chain_labels(&self, ids: &[MirFunctionId]) -> String {
        ids.iter()
            .map(|id| {
                self.function(*id)
                    .map(|function| function_label(function, self.interner))
                    .unwrap_or_else(|| format!("fn#{}", id.0))
            })
            .collect::<Vec<_>>()
            .join(" → ")
    }

    fn resolver(&self) -> Resolver<'_> {
        Resolver { functions: self.functions, monomorphizations: self.monomorphizations, roles: self.roles }
    }

    fn function(&self, id: MirFunctionId) -> Option<&MirFunction> {
        self.functions.iter().find(|function| function.id == id)
    }

    fn is_fragment(&self, function: &MirFunction) -> bool {
        role_of(function, self.roles) == RoleKind::Fragment
    }

    fn is_entry(&self, function: &MirFunction) -> bool {
        role_of(function, self.roles) == RoleKind::Entry
    }
}

struct Resolver<'a> {
    functions: &'a [MirFunction],
    monomorphizations: &'a [MirMonomorphization],
    roles: &'a FxHashMap<DefId, MirDeviceRole>,
}

impl Resolver<'_> {
    fn has_fragment_call(&self, function: &MirFunction) -> bool {
        !self.fragment_sites(function).is_empty()
    }

    fn fragment_sites(&self, function: &MirFunction) -> Vec<CallSite> {
        let mut sites = Vec::new();
        for (block_index, block) in function.blocks.iter().enumerate() {
            for (stmt_index, stmt) in block.statements.iter().enumerate() {
                if let MirStatementKind::Call { destination, callee, args } = &stmt.kind {
                    if self.is_fragment_callee(callee) {
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
            }
            if let MirTerminatorKind::TryCall { destination, callee, args, .. } = &block.terminator.kind {
                if self.is_fragment_callee(callee) {
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
        }
        sites
    }

    fn is_fragment_callee(&self, callee: &MirCallee) -> bool {
        self.resolve_function(callee)
            .is_some_and(|function| role_of(function, self.roles) == RoleKind::Fragment)
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
    cloned: Vec<MirBlockId>,
    continue_id: MirBlockId,
}

struct Account {
    nodes: u32,
    instances: u32,
    sites: FxHashMap<(MirFunctionId, MirFunctionId, MirBlockId, u32), u32>,
}

impl Account {
    fn new() -> Self {
        Self { nodes: 0, instances: 0, sites: FxHashMap::default() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoleKind {
    Entry,
    Fragment,
    Other,
}

fn role_of(function: &MirFunction, roles: &FxHashMap<DefId, MirDeviceRole>) -> RoleKind {
    if let Some(source) = function.source {
        match roles.get(&source) {
            Some(MirDeviceRole::Kernel) => return RoleKind::Entry,
            Some(MirDeviceRole::Fragment) => return RoleKind::Fragment,
            None => {}
        }
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
        .map(|(index, _)| index)
        .unwrap_or(0);
    let mut rotated = body[min_at..].to_vec();
    rotated.extend_from_slice(&body[..min_at]);
    if let Some(first) = rotated.first().copied() {
        rotated.push(first);
    }
    rotated
}

fn error(issue: &'static str, reason: &'static str, span: Span, args: Vec<DiagnosticArg>) -> FragmentCompositionError {
    FragmentCompositionError { issue, reason, span, args }
}

fn count_nodes(function: &MirFunction) -> u32 {
    let stmts = function
        .blocks
        .iter()
        .map(|block| block.statements.len() as u32 + 1)
        .sum::<u32>();
    stmts + function.locals.len() as u32 + function.temps.len() as u32 + function.blocks.len() as u32
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
        let mut gen = Self { local: 0, temp: 0, block: 0, value: 0 };
        for param in &function.params {
            gen.local = gen.local.max(param.local.0.saturating_add(1));
        }
        for local in &function.locals {
            gen.local = gen.local.max(local.id.0.saturating_add(1));
        }
        for temp in &function.temps {
            gen.temp = gen.temp.max(temp.id.0.saturating_add(1));
        }
        for block in &function.blocks {
            gen.block = gen.block.max(block.id.0.saturating_add(1));
            for stmt in &block.statements {
                if let MirStatementKind::Assign { value, .. } = &stmt.kind {
                    gen.value = gen.value.max(value.id.0.saturating_add(1));
                }
            }
        }
        gen
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
            other => other.clone(),
        }
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
            .map(|defaults| crate::mir::MirStructFieldDefaults {
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
                    .map(|case| crate::mir::MirSwitchCase {
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

fn rewrite_return_into(
    block: &mut MirBlock,
    destination: Option<&MirPlace>,
    continue_id: MirBlockId,
    dest_ty: MirType,
    ids: &mut IdGen,
) {
    match &block.terminator.kind {
        MirTerminatorKind::Return(operand) => {
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
        MirTerminatorKind::ReturnError(_) => {
            block.terminator = MirTerminator { kind: MirTerminatorKind::Unreachable, span: block.terminator.span };
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
) -> MirBlock {
    let mut mapped = remap_block(block, alpha, ids);
    rewrite_return_into(&mut mapped, destination, continue_id, dest_ty, ids);
    mapped
}

#[cfg(test)]
#[path = "fragment_composition_test.rs"]
mod tests;

#[cfg(test)]
#[path = "fragment_corpus_test.rs"]
mod corpus;
