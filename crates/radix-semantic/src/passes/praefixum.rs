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
//! A violation is SEM064 `praefixum_forbidden` naming the effect and the call
//! chain from the static to the function that performs it.
//!
//! The same walk records which statics each initializer reads, directly or
//! through its callees. [`evaluation_order`] turns that into the order the
//! driver evaluates `praefixum` statics in; a cycle through a `praefixum`
//! static is SEM007 `constant_cycle`.
//!
//! LOCAL SITES
//! ===========
//! `fixum T x = praefixum { … }` inside a function body, method, closure or
//! the entry block is a *local* site (st1 R6/R7). It runs exactly like a
//! module constant (a free-standing synthetic function on the runner), so
//! its body must stand alone (st1 R8/R9):
//! - it may not read an enclosing parameter or local, including an earlier
//!   compile-time local: SEM064 `praefixum_captures_local`;
//! - in a generic function, method or genus it may not mention a type
//!   parameter of the enclosing declaration: SEM064 `praefixum_type_parameter`.
//!
//! A function that contains a local site is, for ordering, a reader of that
//! site: whoever calls the function waits for the site's evaluation.

use crate::{SemanticError, SemanticErrorKind, Type, TypeTable};
use radix_hir::visit::{HirVisitor, walk_expr, walk_local};
use radix_hir::{
    DefId, HirExpression, HirExpressionKind, HirFunction, HirItemKind, HirLocal, HirModule,
};
use radix_lexer::{Interner, Span, Symbol};
use radix_types::{AppliedArg, TypeId, TypeParamConstraint};
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;

const ISSUE_FORBIDDEN: &str = "praefixum_forbidden";
const ISSUE_CYCLE: &str = "constant_cycle";
const ISSUE_CAPTURES_LOCAL: &str = "praefixum_captures_local";
const ISSUE_TYPE_PARAMETER: &str = "praefixum_type_parameter";

/// Where a `praefixum { … }` initializer sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PraefixumSiteKind {
    /// A module constant: `fixum T X = praefixum { … }`.
    Constant,
    /// A genus field default (`generis`, `fixum` or `varia`).
    Field,
    /// A block-level constant inside a function, method, closure or the
    /// entry block. The driver keeps no authored copy: Faber re-emission
    /// never evaluates, so it prints the body from the HIR itself.
    Local,
}

/// One constant whose initializer is a `praefixum { … }` body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PraefixumSite {
    /// The constant's definition: the constant item, the field, or the local.
    pub def_id: DefId,
    pub name: Symbol,
    pub span: Span,
    pub kind: PraefixumSiteKind,
}

/// Every `praefixum` site in the module: module constants and genus field
/// defaults in item order, then local sites (innermost first inside one
/// body, so an enclosing body never runs ahead of a site it contains).
#[must_use]
pub fn sites(hir: &HirModule) -> Vec<PraefixumSite> {
    let scan = scan_locals(hir);
    all_sites(hir, &scan)
}

fn all_sites(hir: &HirModule, scan: &LocalScan) -> Vec<PraefixumSite> {
    let mut sites = item_sites(hir);
    sites.extend(scan.sites.iter().map(|local| local.site));
    sites
}

fn item_sites(hir: &HirModule) -> Vec<PraefixumSite> {
    let mut sites = Vec::new();
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Constant(konst) if is_praefixum(&konst.value) => {
                sites.push(PraefixumSite {
                    def_id: item.def_id,
                    name: konst.name,
                    span: konst.value.span,
                    kind: PraefixumSiteKind::Constant,
                });
            }
            HirItemKind::Struct(strukt) => {
                for field in &strukt.fields {
                    if let Some(init) = &field.init
                        && is_praefixum(init)
                    {
                        sites.push(PraefixumSite {
                            def_id: field.def_id,
                            name: field.name,
                            span: init.span,
                            kind: PraefixumSiteKind::Field,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    sites
}

// LOCAL SITES

/// One declaration body (function, method, entry, field default) and the
/// bindings it introduces: what a local site inside it could capture.
struct Root {
    bindings: FxHashMap<DefId, Symbol>,
    /// Type parameters of the enclosing declaration(s).
    type_params: Vec<Symbol>,
}

struct LocalSite {
    site: PraefixumSite,
    /// The `Praefixum` initializer expression (cloned out of the module).
    init: HirExpression,
    root: usize,
}

#[derive(Default)]
struct LocalScan {
    roots: Vec<Root>,
    sites: Vec<LocalSite>,
}

/// Collects the bindings of one root and its local sites, innermost first.
struct RootWalk {
    bindings: FxHashMap<DefId, Symbol>,
    found: Vec<(PraefixumSite, HirExpression)>,
}

impl HirVisitor for RootWalk {
    fn visit_def(&mut self, def_id: DefId, name: Symbol) {
        self.bindings.insert(def_id, name);
    }

    fn visit_local(&mut self, local: &HirLocal) {
        walk_local(self, local);
        if let Some(init) = &local.init
            && is_praefixum(init)
        {
            self.found.push((
                PraefixumSite {
                    def_id: local.def_id,
                    name: local.name,
                    span: init.span,
                    kind: PraefixumSiteKind::Local,
                },
                init.clone(),
            ));
        }
    }
}

impl LocalScan {
    fn add_root(&mut self, type_params: Vec<Symbol>, walk: impl FnOnce(&mut RootWalk)) {
        let mut root_walk = RootWalk {
            bindings: FxHashMap::default(),
            found: Vec::new(),
        };
        walk(&mut root_walk);
        if root_walk.found.is_empty() {
            return;
        }
        let root = self.roots.len();
        self.roots.push(Root {
            bindings: root_walk.bindings,
            type_params,
        });
        for (site, init) in root_walk.found {
            self.sites.push(LocalSite { site, init, root });
        }
    }
}

fn scan_locals(hir: &HirModule) -> LocalScan {
    let mut scan = LocalScan::default();
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(function) => {
                let params = function
                    .type_params
                    .iter()
                    .map(|param| param.name)
                    .collect();
                scan.add_root(params, |walk| walk.visit_function(function));
            }
            HirItemKind::Struct(strukt) => {
                let owner: Vec<Symbol> =
                    strukt.type_params.iter().map(|param| param.name).collect();
                for method in &strukt.methods {
                    let mut params = owner.clone();
                    params.extend(method.func.type_params.iter().map(|param| param.name));
                    scan.add_root(params, |walk| walk.visit_function(&method.func));
                }
                for field in &strukt.fields {
                    if let Some(init) = &field.init {
                        scan.add_root(owner.clone(), |walk| walk.visit_expr(init));
                    }
                }
            }
            HirItemKind::Constant(konst) => {
                scan.add_root(Vec::new(), |walk| walk.visit_expr(&konst.value));
            }
            _ => {}
        }
    }
    if let Some(entry) = &hir.entry {
        scan.add_root(Vec::new(), |walk| walk.visit_block(entry));
    }
    scan
}

fn is_praefixum(expr: &HirExpression) -> bool {
    matches!(expr.kind, HirExpressionKind::Praefixum(_))
}

/// Check every `praefixum` body for forbidden effects, static cycles,
/// captured locals and type-parameter dependence.
#[must_use]
pub fn check(hir: &HirModule, types: &TypeTable, interner: &Interner) -> Vec<SemanticError> {
    let scan = scan_locals(hir);
    let sites = all_sites(hir, &scan);
    if sites.is_empty() {
        return Vec::new();
    }
    let mut graph = Graph::new(hir, types, &scan);
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
    errors.extend(standalone_errors(hir, types, interner, &scan));
    if let Err(cycle) = graph.order(&sites) {
        errors.push(
            SemanticError::structured(SemanticErrorKind::CircularDependency, cycle.span)
                .with_arg("issue", ISSUE_CYCLE)
                .with_arg("name", interner.resolve(cycle.name)),
        );
    }
    errors
}

// STANDALONE BODIES (st1 R8, R9)

/// What a `praefixum` body mentions that ties it to its surroundings.
struct BodyWalk<'a> {
    types: &'a TypeTable,
    functions: &'a FxHashSet<DefId>,
    /// Type parameters of the enclosing declarations.
    type_params: &'a [Symbol],
    declared: FxHashSet<DefId>,
    paths: Vec<DefId>,
    param: Option<Symbol>,
}

impl BodyWalk<'_> {
    fn note_type(&mut self, ty: Option<TypeId>) {
        if self.param.is_none()
            && let Some(ty) = ty
        {
            self.param = type_param_in(self.types, ty, self.type_params, &mut FxHashSet::default());
        }
    }
}

impl HirVisitor for BodyWalk<'_> {
    fn visit_def(&mut self, def_id: DefId, _name: Symbol) {
        self.declared.insert(def_id);
    }

    fn visit_local(&mut self, local: &HirLocal) {
        self.note_type(local.ty);
        walk_local(self, local);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                self.paths.push(*def_id);
                // A named function's type carries the callee's own
                // parameters, not the enclosing declaration's.
                if !self.functions.contains(def_id) {
                    self.note_type(expr.ty);
                }
            }
            HirExpressionKind::Call(_, type_args, ..)
            | HirExpressionKind::MethodCall(_, _, type_args, ..) => {
                for ty in type_args {
                    self.note_type(Some(*ty));
                }
                self.note_type(expr.ty);
            }
            _ => self.note_type(expr.ty),
        }
        walk_expr(self, expr);
    }
}

fn standalone_errors(
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
    scan: &LocalScan,
) -> Vec<SemanticError> {
    let functions: FxHashSet<DefId> = hir
        .items
        .iter()
        .flat_map(|item| match &item.kind {
            HirItemKind::Function(_) => vec![item.def_id],
            HirItemKind::Struct(strukt) => {
                strukt.methods.iter().map(|method| method.def_id).collect()
            }
            _ => Vec::new(),
        })
        .collect();
    let mut errors = Vec::new();
    let mut check_body = |site: &PraefixumSite,
                          init: &HirExpression,
                          root: Option<&Root>,
                          type_params: &[Symbol]| {
        let mut walk = BodyWalk {
            types,
            functions: &functions,
            type_params,
            declared: FxHashSet::default(),
            paths: Vec::new(),
            param: None,
        };
        walk.visit_expr(init);
        let name = interner.resolve(site.name);
        if let Some(root) = root
            && let Some(captured) = walk
                .paths
                .iter()
                .find(|def| root.bindings.contains_key(def) && !walk.declared.contains(def))
        {
            errors.push(
                SemanticError::structured(SemanticErrorKind::PraefixumInvalid, init.span)
                    .with_arg("issue", ISSUE_CAPTURES_LOCAL)
                    .with_arg("name", name)
                    .with_arg("captured", interner.resolve(root.bindings[captured])),
            );
        }
        if let Some(param) = walk.param {
            errors.push(
                SemanticError::structured(SemanticErrorKind::PraefixumInvalid, init.span)
                    .with_arg("issue", ISSUE_TYPE_PARAMETER)
                    .with_arg("name", name)
                    .with_arg("param", interner.resolve(param)),
            );
        }
    };
    for local in &scan.sites {
        let root = &scan.roots[local.root];
        check_body(&local.site, &local.init, Some(root), &root.type_params);
    }
    // A generic genus's field default runs once per genus, not per instance.
    for item in &hir.items {
        let HirItemKind::Struct(strukt) = &item.kind else {
            continue;
        };
        if strukt.type_params.is_empty() {
            continue;
        }
        let params: Vec<Symbol> = strukt.type_params.iter().map(|param| param.name).collect();
        for field in &strukt.fields {
            if let Some(init) = &field.init
                && is_praefixum(init)
            {
                let site = PraefixumSite {
                    def_id: field.def_id,
                    name: field.name,
                    span: init.span,
                    kind: PraefixumSiteKind::Field,
                };
                check_body(&site, init, None, &params);
            }
        }
    }
    errors
}

/// The first of `params` that `ty` mentions, looking through every
/// structural shape (aliases, containers, function signatures, applications).
fn type_param_in(
    types: &TypeTable,
    ty: TypeId,
    params: &[Symbol],
    seen: &mut FxHashSet<TypeId>,
) -> Option<Symbol> {
    if params.is_empty() || !seen.insert(ty) {
        return None;
    }
    let children: Vec<TypeId> = match types.get(ty) {
        Type::Param(name) => return params.contains(name).then_some(*name),
        Type::Array(inner)
        | Type::Option(inner)
        | Type::Ref(_, inner)
        | Type::Promissum(inner)
        | Type::Cursor(inner)
        | Type::Atomic(inner)
        | Type::Intervallum(inner)
        | Type::Set(inner)
        | Type::Queue(inner)
        | Type::Stack(inner)
        | Type::Alias(_, inner)
        | Type::Tensor(inner, _)
        | Type::Vector(inner, _)
        | Type::Matrix(inner, _)
        | Type::Sparsa(inner, _) => vec![*inner],
        Type::BoundedArray { element, .. }
        | Type::BoundedQueue { element, .. }
        | Type::BoundedStack { element, .. } => vec![*element],
        Type::Map(key, value)
        | Type::PromissumFailable(key, value)
        | Type::AsyncCursor(key, value) => vec![*key, *value],
        Type::Func(sig) => {
            let mut kids: Vec<TypeId> = sig.params.iter().map(|param| param.ty).collect();
            kids.push(sig.ret);
            kids.extend(sig.err);
            for param in &sig.type_params {
                if let TypeParamConstraint::OneOf(members) = &param.constraint {
                    kids.extend(members.iter().copied());
                }
            }
            kids
        }
        Type::Applied(base, args) => {
            let mut kids = vec![*base];
            kids.extend(args.iter().filter_map(|arg| match arg {
                AppliedArg::Type(id) => Some(*id),
                AppliedArg::Size(_) => None,
            }));
            kids
        }
        Type::Union(members) | Type::Intersection(members) | Type::Tuple(members) => {
            members.clone()
        }
        Type::Record(fields) => fields.values().copied().collect(),
        _ => return None,
    };
    children
        .into_iter()
        .find_map(|child| type_param_in(types, child, params, seen))
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
    let scan = scan_locals(hir);
    let sites = all_sites(hir, &scan);
    Graph::new(hir, types, &scan).order(&sites)
}

/// Every Faber function a static's initializer reaches through calls and
/// method calls, transitively: what the build-time evaluation must lower.
#[must_use]
pub fn reached_functions(
    hir: &HirModule,
    types: &TypeTable,
    static_def: DefId,
) -> FxHashSet<DefId> {
    let mut graph = Graph::new(hir, types, &scan_locals(hir));
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
    /// Static definitions (top-level constants, genus fields, local
    /// `praefixum` constants) → initializer.
    statics: FxHashMap<DefId, (Symbol, Cow<'a, HirExpression>)>,
    facts: FxHashMap<DefId, Facts>,
    violations: FxHashMap<DefId, Option<Violation>>,
}

impl<'a> Graph<'a> {
    fn new(hir: &'a HirModule, types: &'a TypeTable, scan: &LocalScan) -> Self {
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
                        .insert(item.def_id, (konst.name, Cow::Borrowed(&konst.value)));
                }
                HirItemKind::Struct(strukt) => {
                    for method in &strukt.methods {
                        graph.functions.insert(method.def_id, &method.func);
                        graph
                            .methods
                            .insert((item.def_id, method.func.name), method.def_id);
                    }
                    for field in &strukt.fields {
                        if let Some(init) = &field.init {
                            graph
                                .statics
                                .insert(field.def_id, (field.name, Cow::Borrowed(init)));
                        }
                    }
                }
                _ => {}
            }
        }
        for local in &scan.sites {
            graph.statics.insert(
                local.site.def_id,
                (local.site.name, Cow::Owned(local.init.clone())),
            );
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
            } else if let Some((_, init)) = collector.graph.statics.get(&def_id) {
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
        if local.init.as_ref().is_some_and(is_praefixum) {
            // A local `praefixum` is its own site. The body that contains it
            // reads its substituted value: callers wait for the site.
            self.facts.statics.push(local.def_id);
            return;
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
