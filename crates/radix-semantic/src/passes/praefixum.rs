//! `praefixum { … }` build-time initializers: purity and evaluation order
//! (DECISIONS D8.4–D8.6).
//!
//! A `praefixum` body runs on the MIR runner during the build (the driver owns
//! that step). Before it may run, this pass proves it is deterministic
//! computation: the body, and every Faber function it reaches through calls
//! and method calls (checked transitively), must not
//! - open an `ad` conversation (network, clock, randomness, files, async);
//! - await (`reddet`, `tacebit`, awaited bindings) or call an async function
//!   or async cursor;
//! - call `externa` (bodyless) code, which cannot run on the runner;
//! - read input (`lege`) or write output (`scribe`/`nota`).
//!
//! A violation is SEM062 `praefixum_forbidden` naming the effect and the call
//! chain from the static to the function that performs it.
//!
//! The same walk records which statics each initializer reads, directly or
//! through its callees. [`evaluation_order`] turns that into the order the
//! driver evaluates `praefixum` statics in; a cycle through a `praefixum`
//! static is SEM007 `static_cycle`.

use crate::{SemanticError, SemanticErrorKind, Type, TypeTable};
use radix_hir::visit::{HirVisitor, walk_expr, walk_local};
use radix_hir::{
    DefId, HirExpression, HirExpressionKind, HirFunction, HirItemKind, HirLocal, HirModule,
};
use radix_lexer::{Interner, Span, Symbol};
use rustc_hash::{FxHashMap, FxHashSet};

const ISSUE_FORBIDDEN: &str = "praefixum_forbidden";
const ISSUE_CYCLE: &str = "static_cycle";

/// One static whose initializer is a `praefixum { … }` body: a top-level
/// static (`generis T X = praefixum { … }`) or a genus static field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PraefixumSite {
    /// The static's definition: the constant item or the field.
    pub def_id: DefId,
    pub name: Symbol,
    pub span: Span,
}

/// Every `praefixum` static in the module, in source order.
#[must_use]
pub fn sites(hir: &HirModule) -> Vec<PraefixumSite> {
    let mut sites = Vec::new();
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Constant(konst) if is_praefixum(&konst.value) => {
                sites.push(PraefixumSite {
                    def_id: item.def_id,
                    name: konst.name,
                    span: konst.value.span,
                });
            }
            HirItemKind::Struct(strukt) => {
                for field in &strukt.fields {
                    if let Some(init) = &field.init
                        && field.is_static()
                        && is_praefixum(init)
                    {
                        sites.push(PraefixumSite {
                            def_id: field.def_id,
                            name: field.name,
                            span: init.span,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    sites
}

fn is_praefixum(expr: &HirExpression) -> bool {
    matches!(expr.kind, HirExpressionKind::Praefixum(_))
}

/// Check every `praefixum` body for forbidden effects and static cycles.
#[must_use]
pub fn check(hir: &HirModule, types: &TypeTable, interner: &Interner) -> Vec<SemanticError> {
    let sites = sites(hir);
    if sites.is_empty() {
        return Vec::new();
    }
    let mut graph = Graph::new(hir, types);
    let mut errors = Vec::new();
    for site in &sites {
        if let Some(violation) = graph.site_violation(site.def_id) {
            let mut chain = vec![interner.resolve(site.name).to_owned()];
            chain.extend(
                violation
                    .chain
                    .iter()
                    .map(|name| interner.resolve(*name).to_owned()),
            );
            errors.push(
                SemanticError::structured(SemanticErrorKind::PraefixumInvalid, violation.span)
                    .with_arg("issue", ISSUE_FORBIDDEN)
                    .with_arg("name", interner.resolve(site.name))
                    .with_arg("effect", violation.effect)
                    .with_arg("chain", chain.join(" → ")),
            );
        }
    }
    if let Err(cycle) = graph.order(&sites) {
        errors.push(
            SemanticError::structured(SemanticErrorKind::CircularDependency, cycle.span)
                .with_arg("issue", ISSUE_CYCLE)
                .with_arg("name", interner.resolve(cycle.name)),
        );
    }
    errors
}

/// A `praefixum` static that depends on itself.
#[derive(Debug, Clone, Copy)]
pub struct StaticCycle {
    pub name: Symbol,
    pub span: Span,
}

/// The `praefixum` statics in an order where every static a body reads
/// (directly, through another static's initializer, or through a callee) is
/// evaluated before it.
///
/// # Errors
///
/// Returns the first static found on a dependency cycle.
pub fn evaluation_order(
    hir: &HirModule,
    types: &TypeTable,
) -> Result<Vec<PraefixumSite>, StaticCycle> {
    let sites = sites(hir);
    Graph::new(hir, types).order(&sites)
}

/// Every Faber function a static's initializer reaches through calls and
/// method calls, transitively: what the build-time evaluation must lower.
#[must_use]
pub fn reached_functions(
    hir: &HirModule,
    types: &TypeTable,
    static_def: DefId,
) -> FxHashSet<DefId> {
    let mut graph = Graph::new(hir, types);
    let mut reached = FxHashSet::default();
    let mut stack = vec![static_def];
    while let Some(next) = stack.pop() {
        for (callee, _) in graph.facts(next).callees.clone() {
            if reached.insert(callee) {
                stack.push(callee);
            }
        }
    }
    reached
}

// GRAPH

/// A forbidden effect reached from a body.
#[derive(Debug, Clone)]
struct Violation {
    effect: &'static str,
    span: Span,
    /// Function names from the body down to the one performing the effect.
    chain: Vec<Symbol>,
}

/// What one body (a function, a static initializer) does directly.
#[derive(Default)]
struct Facts {
    effect: Option<(&'static str, Span)>,
    callees: Vec<(DefId, Span)>,
    statics: Vec<DefId>,
}

struct Graph<'a> {
    types: &'a TypeTable,
    functions: FxHashMap<DefId, &'a HirFunction>,
    methods: FxHashMap<(DefId, Symbol), DefId>,
    /// Static definitions (top-level constants, static fields) → initializer.
    statics: FxHashMap<DefId, (Symbol, &'a HirExpression)>,
    facts: FxHashMap<DefId, Facts>,
    violations: FxHashMap<DefId, Option<Violation>>,
}

impl<'a> Graph<'a> {
    fn new(hir: &'a HirModule, types: &'a TypeTable) -> Self {
        let mut graph = Self {
            types,
            functions: FxHashMap::default(),
            methods: FxHashMap::default(),
            statics: FxHashMap::default(),
            facts: FxHashMap::default(),
            violations: FxHashMap::default(),
        };
        for item in &hir.items {
            match &item.kind {
                HirItemKind::Function(function) => {
                    graph.functions.insert(item.def_id, function);
                }
                HirItemKind::Constant(konst) => {
                    graph
                        .statics
                        .insert(item.def_id, (konst.name, &konst.value));
                }
                HirItemKind::Struct(strukt) => {
                    for method in &strukt.methods {
                        graph.functions.insert(method.def_id, &method.func);
                        graph
                            .methods
                            .insert((item.def_id, method.func.name), method.def_id);
                    }
                    for field in &strukt.fields {
                        if let Some(init) = &field.init
                            && field.is_static()
                        {
                            graph.statics.insert(field.def_id, (field.name, init));
                        }
                    }
                }
                _ => {}
            }
        }
        graph
    }

    /// Direct facts of a function body or static initializer.
    fn facts(&mut self, def_id: DefId) -> &Facts {
        if !self.facts.contains_key(&def_id) {
            let mut collector = Collector {
                graph: self,
                facts: Facts::default(),
            };
            if let Some(function) = collector.graph.functions.get(&def_id).copied() {
                collector.function(function);
            } else if let Some((_, init)) = collector.graph.statics.get(&def_id).copied() {
                collector.visit_expr(init);
            }
            let facts = collector.facts;
            self.facts.insert(def_id, facts);
        }
        &self.facts[&def_id]
    }

    /// The first forbidden effect a static's initializer reaches.
    fn site_violation(&mut self, def_id: DefId) -> Option<Violation> {
        self.violation(def_id, &mut FxHashSet::default())
    }

    fn violation(&mut self, def_id: DefId, visiting: &mut FxHashSet<DefId>) -> Option<Violation> {
        if let Some(known) = self.violations.get(&def_id) {
            return known.clone();
        }
        if !visiting.insert(def_id) {
            // Recursion: the cycle's own members report their effects.
            return None;
        }
        let (effect, callees) = {
            let facts = self.facts(def_id);
            (facts.effect, facts.callees.clone())
        };
        let mut found = effect.map(|(effect, span)| Violation {
            effect,
            span,
            chain: Vec::new(),
        });
        if found.is_none() {
            for (callee, span) in callees {
                if let Some(mut inner) = self.violation(callee, visiting) {
                    let name = self.functions[&callee].name;
                    inner.chain.insert(0, name);
                    // Report at the call inside this body.
                    inner.span = span;
                    found = Some(inner);
                    break;
                }
            }
        }
        visiting.remove(&def_id);
        self.violations.insert(def_id, found.clone());
        found
    }

    /// Statics reachable from `def_id`'s body through callees (not through
    /// other statics' initializers).
    fn static_reads(&mut self, def_id: DefId) -> FxHashSet<DefId> {
        let mut reads = FxHashSet::default();
        let mut seen = FxHashSet::default();
        let mut stack = vec![def_id];
        while let Some(next) = stack.pop() {
            if !seen.insert(next) {
                continue;
            }
            let facts = self.facts(next);
            reads.extend(facts.statics.iter().copied());
            stack.extend(facts.callees.iter().map(|(callee, _)| *callee));
        }
        reads
    }

    fn order(&mut self, sites: &[PraefixumSite]) -> Result<Vec<PraefixumSite>, StaticCycle> {
        let by_def: FxHashMap<DefId, PraefixumSite> =
            sites.iter().map(|site| (site.def_id, *site)).collect();
        let mut state: FxHashMap<DefId, bool> = FxHashMap::default();
        let mut order = Vec::new();
        for site in sites {
            self.visit_static(site.def_id, &by_def, &mut state, &mut order)
                .map_err(|()| StaticCycle {
                    name: site.name,
                    span: site.span,
                })?;
        }
        Ok(order)
    }

    /// Depth-first post-order over static dependencies; `state` is `false`
    /// while a static is in progress and `true` once done.
    fn visit_static(
        &mut self,
        def_id: DefId,
        sites: &FxHashMap<DefId, PraefixumSite>,
        state: &mut FxHashMap<DefId, bool>,
        order: &mut Vec<PraefixumSite>,
    ) -> Result<(), ()> {
        match state.get(&def_id) {
            Some(true) => return Ok(()),
            Some(false) => return Err(()),
            None => {}
        }
        state.insert(def_id, false);
        let mut reads: Vec<DefId> = self.static_reads(def_id).into_iter().collect();
        reads.sort_by_key(|def| def.0);
        for read in reads {
            if self.statics.contains_key(&read) {
                self.visit_static(read, sites, state, order)?;
            }
        }
        state.insert(def_id, true);
        if let Some(site) = sites.get(&def_id) {
            order.push(*site);
        }
        Ok(())
    }
}

struct Collector<'g, 'a> {
    graph: &'g Graph<'a>,
    facts: Facts,
}

impl Collector<'_, '_> {
    fn function(&mut self, function: &HirFunction) {
        if function.is_async {
            self.effect("async", Span::default());
        }
        match &function.body {
            Some(body) => self.visit_block(body),
            None => self.effect("externa", Span::default()),
        }
    }

    fn effect(&mut self, effect: &'static str, span: Span) {
        if self.facts.effect.is_none() {
            self.facts.effect = Some((effect, span));
        }
    }

    fn call(&mut self, def_id: DefId, span: Span) {
        if let Some(function) = self.graph.functions.get(&def_id) {
            if function.is_async {
                self.effect("async", span);
            } else if function.body.is_none() {
                self.effect("externa", span);
            } else {
                self.facts.callees.push((def_id, span));
            }
        }
    }

    fn method(&mut self, receiver: &HirExpression, name: Symbol, span: Span) {
        let Some(mut ty) = receiver.ty else {
            return;
        };
        loop {
            match self.graph.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                Type::Struct(owner) => {
                    if let Some(method) = self.graph.methods.get(&(*owner, name)).copied() {
                        self.call(method, span);
                    }
                    return;
                }
                _ => return,
            }
        }
    }
}

impl HirVisitor for Collector<'_, '_> {
    fn visit_local(&mut self, local: &HirLocal) {
        if local.is_await {
            let span = local
                .init
                .as_ref()
                .map_or(Span::default(), |init| init.span);
            self.effect("await", span);
        }
        walk_local(self, local);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Ad { .. } => self.effect("ad", expr.span),
            HirExpressionKind::Reddet(_)
            | HirExpressionKind::Tacebit(_)
            | HirExpressionKind::Cede(_) => self.effect("await", expr.span),
            HirExpressionKind::ReadLine(_) => self.effect("lege", expr.span),
            HirExpressionKind::Scribe(..) => self.effect("scribe", expr.span),
            HirExpressionKind::Path(def_id) => {
                if self.graph.statics.contains_key(def_id) {
                    self.facts.statics.push(*def_id);
                } else if self.graph.functions.contains_key(def_id) {
                    // A named function used as a value may be called later.
                    self.call(*def_id, expr.span);
                }
            }
            HirExpressionKind::MethodCall(receiver, name, ..) => {
                self.method(receiver, *name, expr.span);
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}
