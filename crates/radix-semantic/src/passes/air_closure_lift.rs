//! Capture-free closure lifting for AIR higher-order combines.
//!
//! AIR accepts named function definitions as higher-order operands.  This pass
//! gives an equivalent identity to the one closure shape that is safe to admit:
//! a closure with no free local/parameter references.  The closure body and
//! parameters move into a synthetic AIR-lane item, and the call-site closure is
//! replaced by a path to that item.  Reverse AD therefore sees the same `DefId`
//! call edge it already handles for named functions.
//!
//! Capture detection is deliberately structural.  A module item is not a
//! capture, while a path to a local or an enclosing callable parameter is.

use crate::function_facts::{CaptureFact, CaptureMode, CaptureMutations};
use crate::{RadixLane, RadixLaneMetadata, SemanticError, SemanticErrorKind, Type, TypeTable};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{HirVisitor, HirVisitorMut, walk_expr, walk_program};
use radix_hir::{
    DefId, HirBlock, HirExpression, HirExpressionKind, HirFunction, HirId, HirItem, HirItemKind,
    HirModule, HirParam,
};
use radix_lexer::{Interner, Span, Symbol};
use radix_syntax::CallablePosture;

/// Higher-order tensor methods whose first argument is a combine/callback.
///
/// The intrinsic registry owns whether a method exists on a given receiver.
/// This pass only owns the AIR callback-lifting seam after ARC-3 has admitted
/// these names to the AIR purity gate.
pub(crate) fn is_higher_order_method(name: &str) -> bool {
    matches!(name, "reducta" | "cumulata" | "mappata" | "filtrata")
}

/// Rewrite capture-free AIR higher-order closure operands into synthetic items.
///
/// # Errors
///
/// Returns one structured AIR diagnostic when a combine captures a local or
/// enclosing parameter.  The diagnostic carries the capture name and the
/// source span of its use; the rejected closure is replaced with an error node
/// so the purity walker does not add a less-specific duplicate `closure` error.
pub fn lift(
    hir: &mut HirModule,
    lanes: &mut RadixLaneMetadata,
    interner: &mut Interner,
    types: &TypeTable,
) -> Result<(), Vec<SemanticError>> {
    let index = DefinitionIndex::from_hir(hir);
    let mut visitor = LiftVisitor::new(index, lanes, interner, types);
    radix_hir::visit::walk_program_mut(&mut visitor, hir);
    hir.items.extend(visitor.pending);

    if visitor.errors.is_empty() {
        Ok(())
    } else {
        Err(visitor.errors)
    }
}

#[derive(Default)]
struct DefinitionIndex {
    module_defs: FxHashSet<DefId>,
    names: FxHashMap<DefId, Symbol>,
    used_defs: FxHashSet<DefId>,
    max_hir_id: u32,
}

impl DefinitionIndex {
    fn from_hir(hir: &HirModule) -> Self {
        let mut index = Self::default();
        for item in &hir.items {
            index.module_defs.insert(item.def_id);
        }

        let mut collector = IndexCollector { index: &mut index };
        walk_program(&mut collector, hir);
        index
    }
}

struct IndexCollector<'a> {
    index: &'a mut DefinitionIndex,
}

impl HirVisitor for IndexCollector<'_> {
    fn visit_def(&mut self, def_id: DefId, name: Symbol) {
        self.index.used_defs.insert(def_id);
        self.index.names.insert(def_id, name);
    }

    fn visit_item(&mut self, item: &HirItem) {
        self.index.max_hir_id = self.index.max_hir_id.max(item.id.0);
        radix_hir::visit::walk_item(self, item);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        self.index.max_hir_id = self.index.max_hir_id.max(expr.id.0);
        walk_expr(self, expr);
    }

    fn visit_stmt(&mut self, stmt: &radix_hir::HirStatement) {
        self.index.max_hir_id = self.index.max_hir_id.max(stmt.id.0);
        radix_hir::visit::walk_stmt(self, stmt);
    }
}

struct LiftVisitor<'a> {
    module_defs: FxHashSet<DefId>,
    names: FxHashMap<DefId, Symbol>,
    used_defs: FxHashSet<DefId>,
    next_def_id: u32,
    next_hir_id: u32,
    lanes: &'a mut RadixLaneMetadata,
    interner: &'a mut Interner,
    types: &'a TypeTable,
    in_air_function: bool,
    pending: Vec<HirItem>,
    errors: Vec<SemanticError>,
}

impl<'a> LiftVisitor<'a> {
    fn new(
        index: DefinitionIndex,
        lanes: &'a mut RadixLaneMetadata,
        interner: &'a mut Interner,
        types: &'a TypeTable,
    ) -> Self {
        Self {
            module_defs: index.module_defs,
            names: index.names,
            used_defs: index.used_defs,
            next_def_id: radix_types::CLOSURE_LIFT_DEF_ID_BASE,
            next_hir_id: index.max_hir_id.saturating_add(1),
            lanes,
            interner,
            types,
            in_air_function: false,
            pending: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn fresh_def_id(&mut self) -> DefId {
        loop {
            let candidate = DefId(self.next_def_id);
            self.next_def_id = self.next_def_id.saturating_add(1);
            if candidate != DefId::INVALID
                && !candidate.is_companion()
                && self.used_defs.insert(candidate)
            {
                return candidate;
            }
        }
    }

    fn fresh_hir_id(&mut self) -> HirId {
        let id = HirId(self.next_hir_id);
        self.next_hir_id = self.next_hir_id.saturating_add(1);
        id
    }

    fn lift_callback(&mut self, closure: HirExpression) -> Result<HirExpression, ()> {
        let HirExpression {
            id,
            kind: HirExpressionKind::Clausura(params, explicit_ret, explicit_err, mut body),
            ty: closure_ty,
            span,
        } = closure
        else {
            return Err(());
        };

        let captures = collect_captures(&body, &params, &self.module_defs);
        if !captures.is_empty() {
            self.errors.push(self.capture_error(span, &captures));
            return Err(());
        }

        // Nested higher-order combines get the same treatment while the
        // enclosing closure still has its AIR-function context.
        self.visit_expr_mut(body.as_mut());

        let return_ty = explicit_ret
            .or_else(|| closure_signature(closure_ty, self.types).map(|sig| sig.ret))
            .or(body.ty);
        let err_ty = explicit_err
            .or_else(|| closure_signature(closure_ty, self.types).and_then(|sig| sig.err));
        let def_id = self.fresh_def_id();
        let name = self.interner.intern(&format!("__air_closure_{}", def_id.0));
        let body_span = body.span;
        let body_statement = radix_hir::HirStatement {
            id: self.fresh_hir_id(),
            kind: radix_hir::HirStatementKind::Redde(Some(*body)),
            span: body_span,
        };
        let function = HirFunction {
            name,
            type_params: Vec::new(),
            params,
            cli_args: None,
            ret_ty: return_ty,
            err_ty,
            body: Some(HirBlock::simple(vec![body_statement], None, body_span)),
            posture: CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            test: None,
        };
        let item_id = self.fresh_hir_id();
        self.pending.push(HirItem {
            annotations: Vec::new(),
            id: item_id,
            def_id,
            kind: HirItemKind::Function(Box::new(function)),
            span,
        });
        self.lanes.insert(def_id, RadixLane::Air);

        Ok(HirExpression {
            id,
            kind: HirExpressionKind::Path(def_id),
            ty: closure_ty,
            span,
        })
    }

    fn capture_error(&self, closure_span: Span, captures: &[CaptureUse]) -> SemanticError {
        let names = captures
            .iter()
            .map(|capture| self.capture_name(capture.fact.def_id))
            .collect::<Vec<_>>();
        let spans = captures
            .iter()
            .map(|capture| format!("{:?}", capture.span))
            .collect::<Vec<_>>();
        SemanticError::structured(SemanticErrorKind::AirPurityViolation, closure_span)
            .with_arg("issue", "closure_capture")
            .with_arg("lane", "air")
            .with_arg("capture", names.join(", "))
            .with_arg("capture_name", names.join(", "))
            .with_arg("captures", names.join(", "))
            .with_arg("capture_span", spans.join(", "))
            .with_arg("capture_spans", spans.join(", "))
    }

    fn capture_name(&self, def_id: DefId) -> String {
        self.names.get(&def_id).map_or_else(
            || format!("DefId({})", def_id.0),
            |symbol| self.interner.resolve(*symbol).to_owned(),
        )
    }
}

impl HirVisitorMut for LiftVisitor<'_> {
    fn visit_item_mut(&mut self, item: &mut HirItem) {
        let previous = self.in_air_function;
        self.in_air_function = self.lanes.lane_for(item.def_id) == Some(RadixLane::Air);
        match &mut item.kind {
            HirItemKind::Function(function) => self.visit_function_mut(function),
            _ => radix_hir::visit::walk_item_mut(self, item),
        }
        self.in_air_function = previous;
    }

    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        if self.in_air_function
            && let HirExpressionKind::MethodCall(_, method, _, args, _) = &mut expr.kind
        {
            let method_name = self.interner.resolve(*method);
            if is_higher_order_method(method_name)
                && let Some(combine) = args.first_mut()
                && matches!(combine.expr.kind, HirExpressionKind::Clausura(..))
            {
                let placeholder = HirExpression {
                    id: combine.expr.id,
                    kind: HirExpressionKind::Error,
                    ty: combine.expr.ty,
                    span: combine.expr.span,
                };
                let closure = std::mem::replace(&mut combine.expr, placeholder);
                if let Ok(path) = self.lift_callback(closure) {
                    combine.expr = path;
                }
            }
        }
        radix_hir::visit::walk_expr_mut(self, expr);
    }
}

struct CaptureCollector<'a> {
    module_defs: &'a FxHashSet<DefId>,
    bound: FxHashSet<DefId>,
    captures: Vec<CaptureUse>,
    mutations: CaptureMutations,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureUse {
    fact: CaptureFact,
    span: Span,
}

fn collect_captures(
    body: &HirExpression,
    params: &[HirParam],
    module_defs: &FxHashSet<DefId>,
) -> Vec<CaptureUse> {
    let mut collector = CaptureCollector {
        module_defs,
        bound: params.iter().map(|param| param.def_id).collect(),
        captures: Vec::new(),
        mutations: CaptureMutations::default(),
    };
    collector.visit_expr(body);
    let CaptureCollector {
        mutations,
        captures,
        ..
    } = collector;
    let mut captures = captures;
    // Derive each capture's mode from the writes and moves the body performs:
    // the recorded fact stays truthful even though every capture is declined.
    for capture in &mut captures {
        capture.fact.mode = mutations.mode(capture.fact.def_id);
    }
    captures
}

impl HirVisitor for CaptureCollector<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        self.mutations.note_expr(expr);
        match &expr.kind {
            HirExpressionKind::Path(def_id)
                if !self.bound.contains(def_id)
                    && !self.module_defs.contains(def_id)
                    && self
                        .captures
                        .iter()
                        .all(|capture| capture.fact.def_id != *def_id) =>
            {
                self.captures.push(CaptureUse {
                    fact: CaptureFact {
                        def_id: *def_id,
                        mode: CaptureMode::Read,
                    },
                    span: expr.span,
                });
                walk_expr(self, expr);
            }
            HirExpressionKind::Clausura(params, _, _, body) => {
                let mut nested_bound = self.bound.clone();
                nested_bound.extend(params.iter().map(|param| param.def_id));
                let previous = std::mem::replace(&mut self.bound, nested_bound);
                self.visit_expr(body);
                self.bound = previous;
            }
            _ => walk_expr(self, expr),
        }
    }

    fn visit_stmt(&mut self, stmt: &radix_hir::HirStatement) {
        self.mutations.note_stmt(stmt);
        if let radix_hir::HirStatementKind::Local(local) = &stmt.kind {
            self.bound.insert(local.def_id);
        }
        radix_hir::visit::walk_stmt(self, stmt);
    }
}

fn closure_signature(
    ty: Option<radix_types::TypeId>,
    types: &TypeTable,
) -> Option<&radix_types::FuncSig> {
    let ty = ty?;
    match types.get(ty) {
        Type::Func(signature) => Some(signature),
        Type::Alias(_, inner) => closure_signature(Some(*inner), types),
        _ => None,
    }
}
