//! Device regions: the `fac machina` block (en `do device`), a checked
//! device-resident region.
//!
//! A region is defined as the surface of a private kernel-of-kernels function
//! called once under `fac omnia` (gpu-reset rule 12.7: a kernel body may be
//! split into sequential launches at whole-grid dependencies); it is not a
//! third legality universe. V0 checks the block in place and lowers it
//! exactly as `fac omnia`, so no device route changes:
//!
//! - everything between launches is kernel code: [`super::kernel_legality`]'s
//!   body walk runs over the block (host calls, host effects, host types,
//!   handlers, async forms and raw lane ids keep their existing names);
//! - [`ISSUE_MACHINA_NESTED`], [`ISSUE_EXIT_CROSSES_BOUNDARY`],
//!   [`ISSUE_LANE_LOOP_NOT_BUILT`], [`ISSUE_ALLOC_NOT_BUILT`] and
//!   [`ISSUE_DEVICE_READ_NOT_BUILT`] are the region's own rows;
//! - captures are classified by writes and every kernel call site is counted,
//!   for the `faber check --device` report.
//!
//! The written expansion (the function the block stands for) lives in
//! `docs/design/device-region.md`; V0 does not synthesize it.

use super::kernel_legality::{Env, Sink};
use super::kernel_legality_body::check_region_body;
use crate::{Primitive, SymbolKind, Type, TypeId, TypeTable};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{HirVisitor, walk_expr, walk_local, walk_stmt};
use radix_hir::{
    DefId, HirArgMarker, HirBlock, HirCallArg, HirExpression, HirExpressionKind, HirItemKind,
    HirModule, HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, Span, Symbol};

/// A `fac machina` block nested inside another one.
pub const ISSUE_MACHINA_NESTED: &str = "fac_machina_nested";
/// A `redde`, `rumpe` or `perge` whose target lies outside the block.
pub const ISSUE_EXIT_CROSSES_BOUNDARY: &str = "nucleum_block_exit_crosses_boundary";
/// A lane-generating form (`filum` loop, `reducta`, scan, tensor expression)
/// written directly in the block instead of in a kernel.
pub const ISSUE_LANE_LOOP_NOT_BUILT: &str = "nucleum_region_lane_loop_not_built";
/// A tensor constructed inside the block.
pub const ISSUE_ALLOC_NOT_BUILT: &str = "nucleum_region_alloc_not_built";
/// A condition, bound, scalar store or kernel scalar argument that reads a
/// value computed on the device (the one rule that makes "0 host syncs" true).
pub const ISSUE_DEVICE_READ_NOT_BUILT: &str = "nucleum_region_device_read_not_built";

/// The statements of one region. A `fac machina … dum c` region is its body
/// only: the synthesized loop test is host control and stays outside.
pub struct RegionBody<'a> {
    pub statements: &'a [HirStatement],
    pub tail: Option<&'a HirExpression>,
    pub span: Span,
}

/// The region a block stands for, when it carries the `machina` marker.
pub fn region_body(block: &HirBlock) -> Option<RegionBody<'_>> {
    let kind = block.breakable?;
    if !kind.is_device_region() {
        return None;
    }
    if kind.is_loop_marker() {
        let (body, _, _) = block.fac_dum_parts()?;
        return Some(RegionBody {
            statements: body,
            tail: None,
            span: block.span,
        });
    }
    Some(RegionBody {
        statements: &block.statements,
        tail: block.expr.as_deref(),
        span: block.span,
    })
}

/// The names of the enclosing function's parameters and locals seen before a
/// region (the region's possible captures).
pub type OuterNames = FxHashMap<DefId, Symbol>;

type RegionCallback<'f> = dyn FnMut(&RegionBody<'_>, u32, &OuterNames) + 'f;

/// Finds every region in a subtree and reports it with its nesting depth.
struct RegionFinder<'f> {
    depth: u32,
    names: OuterNames,
    on_region: &'f mut RegionCallback<'f>,
}

impl HirVisitor for RegionFinder<'_> {
    fn visit_def(&mut self, def_id: DefId, name: Symbol) {
        self.names.insert(def_id, name);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        let block = match &expr.kind {
            HirExpressionKind::Block(block) | HirExpressionKind::Loop(block) => Some(block),
            _ => None,
        };
        if let Some(body) = block.and_then(region_body) {
            (self.on_region)(&body, self.depth, &self.names);
            self.depth += 1;
            walk_expr(self, expr);
            self.depth -= 1;
        } else {
            walk_expr(self, expr);
        }
    }
}

/// Visit every region in the host code of `hir` (function and method bodies
/// that are not kernels, and the entry block).
pub fn for_each_region<'f>(hir: &HirModule, on_region: &'f mut RegionCallback<'f>) {
    let mut finder = RegionFinder {
        depth: 0,
        names: OuterNames::default(),
        on_region,
    };
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(function) if !function.is_kernel => {
                finder.names.clear();
                finder.visit_function(function);
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    if !method.func.is_kernel {
                        finder.names.clear();
                        finder.visit_function(&method.func);
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(entry) = &hir.entry {
        finder.names.clear();
        finder.visit_block(entry);
    }
}

/// Check every region of `hir` against the region rules.
pub(super) fn check_regions(env: &Env<'_>, sink: &mut Sink, hir: &HirModule) {
    for_each_region(hir, &mut |body, depth, _| {
        check_region(env, sink, body, depth);
    });
}

fn check_region(env: &Env<'_>, sink: &mut Sink, body: &RegionBody<'_>, depth: u32) {
    if depth > 0 {
        sink.report(ISSUE_MACHINA_NESTED, body.span, &[]);
        return;
    }
    check_region_body(env, sink, body);
    let mut exits = ExitScan {
        sink,
        loops: 0,
        closures: 0,
    };
    scan(&mut exits, body);
    let mut shape = ShapeScan {
        types: env.types,
        sink,
    };
    scan(&mut shape, body);
    let mut taint = TaintScan {
        env,
        sink,
        tainted: FxHashSet::default(),
    };
    scan(&mut taint, body);
}

fn scan<V: HirVisitor>(visitor: &mut V, body: &RegionBody<'_>) {
    for statement in body.statements {
        visitor.visit_stmt(statement);
    }
    if let Some(tail) = body.tail {
        visitor.visit_expr(tail);
    }
}

// ---------------------------------------------------------------------------
// Block exits
// ---------------------------------------------------------------------------

/// `return`, and `break` / `continue` aimed outside the block. A `break`
/// of a loop written inside the block is ordinary.
struct ExitScan<'a> {
    sink: &'a mut Sink,
    loops: u32,
    closures: u32,
}

impl HirVisitor for ExitScan<'_> {
    fn visit_stmt(&mut self, stmt: &HirStatement) {
        match &stmt.kind {
            HirStatementKind::Redde(_) if self.closures == 0 => {
                self.sink.report(
                    ISSUE_EXIT_CROSSES_BOUNDARY,
                    stmt.span,
                    &[("construct", "redde")],
                );
            }
            HirStatementKind::Rumpe if self.loops == 0 && self.closures == 0 => {
                self.sink.report(
                    ISSUE_EXIT_CROSSES_BOUNDARY,
                    stmt.span,
                    &[("construct", "rumpe")],
                );
            }
            HirStatementKind::Perge if self.loops == 0 && self.closures == 0 => {
                self.sink.report(
                    ISSUE_EXIT_CROSSES_BOUNDARY,
                    stmt.span,
                    &[("construct", "perge")],
                );
            }
            _ => {}
        }
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Loop(_)
            | HirExpressionKind::Dum(..)
            | HirExpressionKind::Itera { .. }
            | HirExpressionKind::IteraLockstep { .. }
            | HirExpressionKind::IteraRange { .. } => {
                self.loops += 1;
                walk_expr(self, expr);
                self.loops -= 1;
            }
            HirExpressionKind::Block(block)
                if block.breakable.is_some_and(|kind| kind.is_scope()) =>
            {
                // A plain `fac { }` is a breakable scope: `rumpe` leaves it.
                self.loops += 1;
                walk_expr(self, expr);
                self.loops -= 1;
            }
            HirExpressionKind::Clausura(..)
            | HirExpressionKind::Reduction { .. }
            | HirExpressionKind::Scan { .. }
            | HirExpressionKind::Sort { .. } => {
                self.closures += 1;
                walk_expr(self, expr);
                self.closures -= 1;
            }
            _ => walk_expr(self, expr),
        }
    }
}

// ---------------------------------------------------------------------------
// Direct lane loops and allocation
// ---------------------------------------------------------------------------

pub(super) fn is_tensor(ty: TypeId, types: &TypeTable) -> bool {
    match types.get(ty) {
        Type::Tensor(..) => true,
        Type::Alias(_, target) | Type::Ref(_, target) => is_tensor(*target, types),
        _ => false,
    }
}

/// Lane-generating forms and tensor construction written directly in the block.
struct ShapeScan<'a> {
    types: &'a TypeTable,
    sink: &'a mut Sink,
}

impl ShapeScan<'_> {
    fn tensor_valued(&self, expr: &HirExpression) -> bool {
        expr.ty.is_some_and(|ty| is_tensor(ty, self.types))
    }
}

impl HirVisitor for ShapeScan<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Itera { filum: Some(_), .. } => self.sink.report(
                ISSUE_LANE_LOOP_NOT_BUILT,
                expr.span,
                &[("construct", "filum")],
            ),
            HirExpressionKind::Reduction { .. } => self.sink.report(
                ISSUE_LANE_LOOP_NOT_BUILT,
                expr.span,
                &[("construct", "reducta")],
            ),
            HirExpressionKind::Scan { .. } => self.sink.report(
                ISSUE_LANE_LOOP_NOT_BUILT,
                expr.span,
                &[("construct", "scan")],
            ),
            HirExpressionKind::Binary(..)
            | HirExpressionKind::Unary(..)
            | HirExpressionKind::Conversio { .. }
                if self.tensor_valued(expr) =>
            {
                self.sink.report(
                    ISSUE_LANE_LOOP_NOT_BUILT,
                    expr.span,
                    &[("construct", "tensor_expression")],
                );
            }
            HirExpressionKind::Array(_)
            | HirExpressionKind::Vacua
            | HirExpressionKind::Verte { .. }
            | HirExpressionKind::Struct(..)
                if self.tensor_valued(expr) =>
            {
                self.sink
                    .report(ISSUE_ALLOC_NOT_BUILT, expr.span, &[("construct", "tensor")]);
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

// ---------------------------------------------------------------------------
// Device reads (the host-sync rule)
// ---------------------------------------------------------------------------

/// Methods of a tensor that read its shape, not its device contents.
const SHAPE_METHODS: [&str; 4] = ["length", "longitudo", "rank", "shape"];

/// Does `expr` carry a value computed on the device?
struct TaintProbe<'a, 'e> {
    env: &'a Env<'e>,
    tainted: &'a FxHashSet<DefId>,
    found: bool,
}

impl TaintProbe<'_, '_> {
    /// Whether `callee` names a kernel (local, or imported through a
    /// namespace). A host function is already refused by name; its result is
    /// not device data.
    fn is_kernel_callee(&self, callee: &HirExpression) -> bool {
        match &callee.kind {
            HirExpressionKind::Path(def_id) => self.env.facts.kernels.contains_key(def_id),
            HirExpressionKind::Field(object, member) => self.is_namespace_kernel(object, *member),
            _ => false,
        }
    }

    fn is_namespace_kernel(&self, object: &HirExpression, member: Symbol) -> bool {
        let HirExpressionKind::Path(module) = &object.kind else {
            return false;
        };
        self.env
            .resolver
            .get_symbol(*module)
            .filter(|symbol| symbol.kind == SymbolKind::Module)
            .is_some_and(|namespace| {
                self.env
                    .resolver
                    .namespace_file_device_contract_canonical(namespace.name, member)
                    .is_some()
            })
    }
}

impl HirVisitor for TaintProbe<'_, '_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if self.found {
            return;
        }
        match &expr.kind {
            HirExpressionKind::Path(def_id) if self.tainted.contains(def_id) => {
                self.found = true;
            }
            HirExpressionKind::Index(base, _)
                if base.ty.is_some_and(|ty| is_tensor(ty, self.env.types)) =>
            {
                self.found = true;
            }
            HirExpressionKind::MethodCall(receiver, method, ..)
                if receiver.ty.is_some_and(|ty| is_tensor(ty, self.env.types))
                    && !SHAPE_METHODS.contains(&self.env.interner.resolve(*method)) =>
            {
                self.found = true;
            }
            HirExpressionKind::Call(callee, ..)
                if expr.ty.is_some_and(|ty| !is_void(ty, self.env.types))
                    && self.is_kernel_callee(callee) =>
            {
                self.found = true;
            }
            HirExpressionKind::MethodCall(receiver, member, ..)
                if expr.ty.is_some_and(|ty| !is_void(ty, self.env.types))
                    && self.is_namespace_kernel(receiver, *member) =>
            {
                self.found = true;
            }
            HirExpressionKind::Clausura(..) => {}
            _ => walk_expr(self, expr),
        }
    }
}

fn is_void(ty: TypeId, types: &TypeTable) -> bool {
    matches!(types.get(ty), Type::Primitive(Primitive::Vacuum))
}

/// Forward taint over the block in statement order: loop bodies are walked
/// twice so a late write reaches an early read.
struct TaintScan<'a, 'e> {
    env: &'a Env<'e>,
    sink: &'a mut Sink,
    tainted: FxHashSet<DefId>,
}

impl TaintScan<'_, '_> {
    fn is_tainted(&self, expr: &HirExpression) -> bool {
        let mut probe = TaintProbe {
            env: self.env,
            tainted: &self.tainted,
            found: false,
        };
        probe.visit_expr(expr);
        probe.found
    }

    fn scalar(&self, expr: &HirExpression) -> bool {
        expr.ty.is_none_or(|ty| !is_tensor(ty, self.env.types))
    }

    fn read(&mut self, span: Span, construct: &str) {
        self.sink.report(
            ISSUE_DEVICE_READ_NOT_BUILT,
            span,
            &[("construct", construct)],
        );
    }

    fn check_scalar(&mut self, expr: &HirExpression, construct: &str) {
        if self.scalar(expr) && self.is_tainted(expr) {
            self.read(expr.span, construct);
        }
    }

    fn taint_root(&mut self, target: &HirExpression) {
        match &target.kind {
            HirExpressionKind::Path(def_id) => {
                self.tainted.insert(*def_id);
            }
            HirExpressionKind::Field(object, _) | HirExpressionKind::Index(object, _) => {
                self.taint_root(object);
            }
            _ => {}
        }
    }

    fn loop_body(&mut self, body: &HirBlock) {
        self.visit_block(body);
        self.visit_block(body);
    }
}

impl HirVisitor for TaintScan<'_, '_> {
    fn visit_local(&mut self, local: &radix_hir::HirLocal) {
        walk_local(self, local);
        if let Some(init) = &local.init
            && self.is_tainted(init)
        {
            if self.scalar(init) {
                self.read(init.span, "scalar_store");
            }
            self.tainted.insert(local.def_id);
        }
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Si { cond, .. } => {
                self.check_scalar(cond, "condition");
                walk_expr(self, expr);
            }
            HirExpressionKind::Discerne { scrutinees, .. } => {
                for scrutinee in scrutinees {
                    self.check_scalar(scrutinee, "condition");
                }
                walk_expr(self, expr);
            }
            HirExpressionKind::Dum(cond, body) => {
                self.check_scalar(cond, "condition");
                self.visit_expr(cond);
                self.loop_body(body);
            }
            HirExpressionKind::Loop(body) => self.loop_body(body),
            HirExpressionKind::Itera {
                iter,
                body,
                binding,
                ..
            } => {
                if let HirExpressionKind::Intervallum {
                    start, end, step, ..
                } = &iter.kind
                {
                    for bound in [Some(start), Some(end), step.as_ref()]
                        .into_iter()
                        .flatten()
                    {
                        self.check_scalar(bound, "bound");
                    }
                }
                self.visit_expr(iter);
                if self.is_tainted(iter) {
                    self.tainted.insert(*binding);
                }
                self.loop_body(body);
            }
            HirExpressionKind::IteraRange { ranges, body, .. } => {
                for range in ranges {
                    if let HirExpressionKind::Intervallum {
                        start, end, step, ..
                    } = &range.kind
                    {
                        for bound in [Some(start), Some(end), step.as_ref()]
                            .into_iter()
                            .flatten()
                        {
                            self.check_scalar(bound, "bound");
                        }
                    }
                    self.visit_expr(range);
                }
                self.loop_body(body);
            }
            HirExpressionKind::Assign(target, value) => {
                walk_expr(self, expr);
                if self.is_tainted(value) {
                    if self.scalar(target) {
                        self.read(expr.span, "scalar_store");
                    }
                    self.taint_root(target);
                }
            }
            HirExpressionKind::ConversioAssign { target, source, .. } => {
                walk_expr(self, expr);
                if self.is_tainted(source) {
                    if self.scalar(target) {
                        self.read(expr.span, "scalar_store");
                    }
                    self.taint_root(target);
                }
            }
            HirExpressionKind::Call(_, _, args, _)
            | HirExpressionKind::MethodCall(_, _, _, args, _) => {
                for arg in args {
                    self.check_scalar(&arg.expr, "kernel_argument");
                }
                walk_expr(self, expr);
                // A scalar or genus handed over as `mut` is device state from
                // here on: the kernel may have written it.
                for arg in args {
                    if arg.marker == Some(HirArgMarker::Mut) && self.scalar(&arg.expr) {
                        self.taint_root(&arg.expr);
                    }
                }
            }
            _ => walk_expr(self, expr),
        }
    }
}

// ---------------------------------------------------------------------------
// Bindings and launches (the `faber check --device` report)
// ---------------------------------------------------------------------------

/// One name the block shares with the code around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionBinding {
    pub name: String,
    /// The binding's type as source text; a tensor extent is the name of the
    /// `size` binding that carries it.
    pub ty: String,
}

/// Facts the report prints for one region.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegionFacts {
    /// Captures only read.
    pub inputs: Vec<RegionBinding>,
    /// Captures the block writes (a `mut` argument, a store, a copy-into).
    pub in_outs: Vec<RegionBinding>,
    /// Kernel call sites in the body of the outermost loop, always reached.
    pub loop_launches: u32,
    /// Kernel call sites in that body that sit under a guard.
    pub guarded_launches: u32,
    /// Kernel call sites outside any loop.
    pub other_launches: u32,
    /// Every capture with its type, in first-use order.
    pub captured: Vec<(DefId, TypeId)>,
}

/// Classify the block's captures by writes and count its kernel call sites.
pub(super) fn region_facts(
    resolver: &crate::Resolver,
    types: &TypeTable,
    outer: &OuterNames,
    interner: &Interner,
    body: &RegionBody<'_>,
) -> RegionFacts {
    let mut scan = FactsScan {
        resolver,
        outer,
        declared: FxHashSet::default(),
        captured: FxHashMap::default(),
        written: FxHashSet::default(),
        order: Vec::new(),
        loops: 0,
        guards: 0,
        facts: RegionFacts::default(),
    };
    scan_facts(&mut scan, body);
    scan.facts.captured = scan
        .order
        .iter()
        .filter_map(|def_id| scan.captured.get(def_id).map(|ty| (*def_id, *ty)))
        .collect();
    for def_id in scan.order {
        let (Some(name), Some(ty)) = (outer.get(&def_id), scan.captured.get(&def_id).copied())
        else {
            continue;
        };
        let binding = RegionBinding {
            name: interner.resolve(*name).to_owned(),
            ty: describe_type(ty, types, interner, outer, resolver),
        };
        if scan.written.contains(&def_id) {
            scan.facts.in_outs.push(binding);
        } else {
            scan.facts.inputs.push(binding);
        }
    }
    scan.facts
}

fn scan_facts(scan: &mut FactsScan<'_>, body: &RegionBody<'_>) {
    scan.scan_body(body);
}

struct FactsScan<'a> {
    resolver: &'a crate::Resolver,
    outer: &'a OuterNames,
    declared: FxHashSet<DefId>,
    captured: FxHashMap<DefId, TypeId>,
    written: FxHashSet<DefId>,
    order: Vec<DefId>,
    loops: u32,
    guards: u32,
    facts: RegionFacts,
}

impl FactsScan<'_> {
    fn scan_body(&mut self, body: &RegionBody<'_>) {
        scan(self, body);
    }

    fn capture(&mut self, def_id: DefId, ty: Option<TypeId>) {
        if self.declared.contains(&def_id) || self.captured.contains_key(&def_id) {
            return;
        }
        if !self.outer.contains_key(&def_id) {
            return;
        }
        if let Some(ty) = ty {
            self.captured.insert(def_id, ty);
            self.order.push(def_id);
        }
    }

    fn write(&mut self, target: &HirExpression) {
        match &target.kind {
            HirExpressionKind::Path(def_id) => {
                self.written.insert(*def_id);
            }
            HirExpressionKind::Field(object, _) | HirExpressionKind::Index(object, _) => {
                self.write(object);
            }
            _ => {}
        }
    }

    fn call_site(&mut self) {
        if self.loops == 0 {
            self.facts.other_launches += 1;
        } else if self.loops == 1 {
            if self.guards == 0 {
                self.facts.loop_launches += 1;
            } else {
                self.facts.guarded_launches += 1;
            }
        }
    }

    fn marked_writes(&mut self, args: &[HirCallArg]) {
        for arg in args {
            if matches!(arg.marker, Some(HirArgMarker::Mut | HirArgMarker::Own)) {
                self.write(&arg.expr);
            }
        }
    }
}

impl HirVisitor for FactsScan<'_> {
    fn visit_def(&mut self, def_id: DefId, _name: radix_lexer::Symbol) {
        self.declared.insert(def_id);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.capture(*def_id, expr.ty),
            HirExpressionKind::Assign(target, _)
            | HirExpressionKind::ConversioAssign { target, .. }
            | HirExpressionKind::CopyInto { target, .. } => {
                self.write(target);
                walk_expr(self, expr);
            }
            HirExpressionKind::Call(_, _, args, _) => {
                self.marked_writes(args);
                self.call_site();
                walk_expr(self, expr);
            }
            HirExpressionKind::MethodCall(receiver, _, _, args, _) => {
                self.marked_writes(args);
                // `module.kernel(...)` is a launch; a method on a value is not.
                if matches!(
                    &receiver.kind,
                    HirExpressionKind::Path(def_id)
                        if self.resolver.get_symbol(*def_id).is_some_and(|symbol| symbol.kind == SymbolKind::Module)
                ) {
                    self.call_site();
                }
                walk_expr(self, expr);
            }
            HirExpressionKind::Loop(_)
            | HirExpressionKind::Dum(..)
            | HirExpressionKind::Itera { .. }
            | HirExpressionKind::IteraLockstep { .. }
            | HirExpressionKind::IteraRange { .. } => {
                self.loops += 1;
                walk_expr(self, expr);
                self.loops -= 1;
            }
            HirExpressionKind::Si { .. } | HirExpressionKind::Discerne { .. } => {
                self.guards += 1;
                walk_expr(self, expr);
                self.guards -= 1;
            }
            _ => walk_expr(self, expr),
        }
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if let HirStatementKind::IncDec(inc_dec) = &stmt.kind {
            self.write(&inc_dec.target);
        }
        walk_stmt(self, stmt);
    }
}

/// A type as source text. A tensor extent that a captured `size` binding
/// carries prints as that binding's name.
fn describe_type(
    ty: TypeId,
    types: &TypeTable,
    interner: &Interner,
    outer: &OuterNames,
    resolver: &crate::Resolver,
) -> String {
    use radix_types::{IndexExpr, RuntimeExtentId};
    let name_of = |def_id: DefId| {
        resolver.get_symbol(def_id).map_or_else(
            || format!("Def{}", def_id.0),
            |symbol| interner.resolve(symbol.name).to_owned(),
        )
    };
    let dimension = |index| match types.get_index(index) {
        IndexExpr::Literal(value) => value.to_string(),
        IndexExpr::Param(name) => interner.resolve(*name).to_owned(),
        IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(binding)) => outer
            .get(binding)
            .map_or_else(|| "_".to_owned(), |name| interner.resolve(*name).to_owned()),
        _ => "_".to_owned(),
    };
    match types.get(ty) {
        Type::Primitive(Primitive::Numerus) => "int".to_owned(),
        Type::Primitive(Primitive::Fractus) => "float".to_owned(),
        Type::Primitive(Primitive::Bivalens) => "bool".to_owned(),
        Type::Primitive(Primitive::Textus) => "string".to_owned(),
        Type::Alias(_, target) | Type::Ref(_, target) => {
            describe_type(*target, types, interner, outer, resolver)
        }
        Type::Tensor(element, shape) => {
            let dimensions = match types.get_index(*shape) {
                IndexExpr::Tuple(dimensions) => dimensions
                    .iter()
                    .map(|index| dimension(*index))
                    .collect::<Vec<_>>()
                    .join(", "),
                _ => dimension(*shape),
            };
            format!(
                "tensor<{}, [{dimensions}]>",
                describe_type(*element, types, interner, outer, resolver)
            )
        }
        _ => crate::format_faber_type(ty, types, interner, &name_of),
    }
}
