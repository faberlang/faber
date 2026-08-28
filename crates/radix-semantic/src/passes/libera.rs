//! Closure modifier contracts: `libera` capture freedom and the `nucleum`
//! kernel closure law.
//!
//! A closure marked `libera` (reader `free`) must not capture any local or
//! parameter of an enclosing function: its environment must be empty. Module
//! items, builtins, enum variants, and imported members are statically
//! resolved identities, not captures, and remain legal.
//!
//! A closure marked `nucleum` (reader `kernel`) is a local private device
//! region, not a first-class value. It carries the same capture freedom plus
//! two structural laws: the body admits no alternate exits, error/recovery
//! operations, host effects, or dynamic function values; and the closure
//! itself may only initialize an immutable local or serve as a direct call
//! callee (the same-expression IIFE) — a field, list/tuple element, return
//! value, argument, or assignment payload use is an escape. Enclosing
//! type/size parameters are not value bindings and stay legal by
//! construction, exactly as for `libera`.
//!
//! Unmarked closures are untouched — this pass fires only on markers. The
//! checks are structural over typed HIR and run on every lane; MIR
//! device-safe classification stays the final lowerability authority.

use crate::{Resolver, SemanticError, SemanticErrorKind};
use radix_hir::visit::{
    walk_block, walk_cape, walk_expr, walk_local, walk_pattern, walk_stmt, HirVisitor,
};
use radix_hir::{
    DefId, HirBlock, HirCape, HirExpression, HirExpressionKind, HirFunction, HirLocal, HirModule,
    HirParam, HirPattern, HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, Span, Symbol};
use radix_syntax::ClosureModifier;
use rustc_hash::{FxHashMap, FxHashSet};

/// Structured issue for a marked closure that captures an enclosing local/param.
pub const ISSUE_LIBERA_CAPTURE: &str = "libera_capture";
/// Structured issue for a kernel closure that captures an enclosing local/param.
pub const ISSUE_NUCLEUM_CAPTURE: &str = "nucleum_closure_capture";
/// Structured issue for a kernel-closure body or channel that violates the
/// device contract (alternate exit, error/recovery, host effect, dynamic value).
pub const ISSUE_NUCLEUM_EFFECT: &str = "nucleum_closure_effect";
/// Structured issue for a kernel closure value that escapes its legal
/// local-const / IIFE positions.
pub const ISSUE_NUCLEUM_ESCAPE: &str = "nucleum_closure_escape";

/// Reject `libera` closures that capture an enclosing function's locals or
/// parameters.
///
/// # Errors
///
/// Returns one structured [`SemanticError`] per captured definition, spanning
/// the capture use-site and naming the binding.
pub fn analyze(
    hir: &HirModule,
    resolver: &Resolver,
    interner: &Interner,
) -> Result<(), Vec<SemanticError>> {
    let mut checker = LiberaChecker {
        resolver,
        interner,
        diagnostics: Vec::new(),
        scopes: Vec::new(),
        names: FxHashMap::default(),
        kernel_bindings: FxHashSet::default(),
        position: KernelPosition::Other("expression"),
    };
    checker.visit_program(hir);
    if checker.diagnostics.is_empty() {
        Ok(())
    } else {
        Err(checker.diagnostics)
    }
}

struct LiberaChecker<'a> {
    resolver: &'a Resolver,
    interner: &'a Interner,
    diagnostics: Vec<SemanticError>,
    /// Binding scopes visible in the current function walk. The union of the
    /// stack is the set of enclosing locals/params a marked closure may not
    /// reference; module-level identities never enter the stack, so they are
    /// legal by construction.
    scopes: Vec<FxHashSet<DefId>>,
    /// Definition display names gathered from [`HirVisitor::visit_def`] so
    /// synthetic bindings (pattern/cape/itera) can be named in diagnostics.
    names: FxHashMap<DefId, Symbol>,
    /// Immutable locals of the current function initialized by a `nucleum`
    /// closure. Their only legal use is the direct call-callee position.
    kernel_bindings: FxHashSet<DefId>,
    /// Lexical position of the expression currently being visited; decides
    /// whether a kernel closure occurrence is a legal binding/IIFE site or an
    /// escape, and names the escape context fact.
    position: KernelPosition,
}

/// Where an expression sits relative to the kernel-closure escape law.
#[derive(Clone, Copy)]
enum KernelPosition {
    /// Inside a local initializer (past the direct binding site, which
    /// `visit_local` owns). Only names the escape context for kernel values.
    LocalInit,
    /// Direct callee of a call — the IIFE / local kernel-call site.
    CallCallee,
    /// Any other position, carrying the structured `context` fact.
    Other(&'static str),
}

impl KernelPosition {
    /// Context fact reported for an escaping kernel value at this position.
    fn context(self) -> &'static str {
        match self {
            Self::LocalInit => "local_binding",
            Self::CallCallee => "call_callee",
            Self::Other(context) => context,
        }
    }
}

impl LiberaChecker<'_> {
    fn enclosing_union(&self) -> FxHashSet<DefId> {
        let mut all = FxHashSet::default();
        for scope in &self.scopes {
            all.extend(scope.iter().copied());
        }
        all
    }

    fn capture_name(&self, def_id: DefId) -> String {
        self.names
            .get(&def_id)
            .or_else(|| self.resolver.get_symbol(def_id).map(|symbol| &symbol.name))
            .map_or_else(
                || format!("DefId({})", def_id.0),
                |symbol| self.interner.resolve(*symbol).to_owned(),
            )
    }

    fn check_libera_closure(&mut self, params: &[HirParam], body: &HirExpression) {
        let enclosing = self.enclosing_union();
        let mut probe = CaptureProbe {
            enclosing,
            scopes: vec![params.iter().map(|param| param.def_id).collect()],
            captures: Vec::new(),
        };
        probe.visit_expr(body);
        for (def_id, span) in probe.captures {
            let name = self.capture_name(def_id);
            self.diagnostics.push(
                SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
                    .with_arg("issue", ISSUE_LIBERA_CAPTURE)
                    .with_arg("capture", name.clone())
                    .with_arg("capture_name", name),
            );
        }
    }

    /// Full `nucleum` contract for a closure at a legal position: enclosing
    /// captures and body forms. The signature `⇥` channel and signature type
    /// admission are owned by the typecheck closure path; the MIR device-safe
    /// classification stays the final lowerability authority.
    fn check_nucleum_closure(&mut self, params: &[HirParam], body: &HirExpression) {
        let enclosing = self.enclosing_union();
        let mut probe = CaptureProbe {
            enclosing,
            scopes: vec![params.iter().map(|param| param.def_id).collect()],
            captures: Vec::new(),
        };
        probe.visit_expr(body);
        for (def_id, span) in probe.captures {
            let name = self.capture_name(def_id);
            self.diagnostics.push(
                SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
                    .with_arg("issue", ISSUE_NUCLEUM_CAPTURE)
                    .with_arg("capture", name.clone())
                    .with_arg("capture_name", name),
            );
        }
        let mut forms = KernelBodyProbe {
            diagnostics: Vec::new(),
        };
        forms.visit_expr(body);
        self.diagnostics.extend(forms.diagnostics);
    }

    fn escape_error(&mut self, span: Span, context: &str) {
        self.diagnostics.push(
            SemanticError::structured(SemanticErrorKind::LoweringError, span)
                .with_arg("issue", ISSUE_NUCLEUM_ESCAPE)
                .with_arg("context", context),
        );
    }

    fn visit_in(&mut self, position: KernelPosition, expr: &HirExpression) {
        let saved = self.position;
        self.position = position;
        self.visit_expr(expr);
        self.position = saved;
    }
}

impl HirVisitor for LiberaChecker<'_> {
    fn visit_program(&mut self, program: &HirModule) {
        for item in &program.items {
            self.visit_item(item);
        }
        for annotation in &program.entry_annotations {
            self.visit_annotation(annotation);
        }
        if let Some(entry) = &program.entry {
            // The entry body is a function-shaped scope with no parameters.
            let saved = std::mem::take(&mut self.scopes);
            let saved_bindings = std::mem::take(&mut self.kernel_bindings);
            let saved_position = self.position;
            self.scopes = vec![FxHashSet::default()];
            self.position = KernelPosition::Other("expression");
            self.visit_block(entry);
            self.position = saved_position;
            self.kernel_bindings = saved_bindings;
            self.scopes = saved;
        }
    }

    fn visit_function(&mut self, function: &HirFunction) {
        let saved = std::mem::take(&mut self.scopes);
        let saved_bindings = std::mem::take(&mut self.kernel_bindings);
        let saved_position = self.position;
        let mut params = FxHashSet::default();
        for param in &function.params {
            params.insert(param.def_id);
            self.visit_def(param.def_id, param.name);
        }
        if let Some(cli) = &function.cli_args {
            params.insert(cli.def_id);
            self.visit_def(cli.def_id, cli.name);
        }
        self.scopes = vec![params];
        self.position = KernelPosition::Other("expression");
        if let Some(body) = &function.body {
            self.visit_block(body);
        }
        self.position = saved_position;
        self.kernel_bindings = saved_bindings;
        self.scopes = saved;
    }

    fn visit_block(&mut self, block: &HirBlock) {
        self.scopes.push(FxHashSet::default());
        walk_block(self, block);
        self.scopes.pop();
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if let HirStatementKind::Redde(Some(value)) = &stmt.kind {
            self.visit_in(KernelPosition::Other("return"), value);
            return;
        }
        walk_stmt(self, stmt);
    }

    fn visit_local(&mut self, local: &HirLocal) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(local.def_id);
        }
        self.visit_def(local.def_id, local.name);
        if let Some(init) = &local.init {
            // A direct `nucleum` initializer is the local-const binding site:
            // immutable locals keep the kernel value callable in this function,
            // mutable locals escape it. The closure's own body belongs to
            // `check_nucleum_closure`, not to the enclosing walk.
            if matches!(
                init.kind,
                HirExpressionKind::Clausura(.., ClosureModifier::Nucleum)
            ) {
                if local.mutable {
                    self.escape_error(init.span, "varia_binding");
                } else {
                    self.kernel_bindings.insert(local.def_id);
                    if let HirExpressionKind::Clausura(params, _, _, body, _) = &init.kind {
                        self.check_nucleum_closure(params, body);
                    }
                }
                return;
            }
            self.visit_in(KernelPosition::LocalInit, init);
        }
    }

    fn visit_param(&mut self, param: &HirParam) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(param.def_id);
        }
        self.visit_def(param.def_id, param.name);
        if let Some(default) = &param.default {
            self.visit_in(KernelPosition::Other("ordinary_argument"), default);
        }
    }

    fn visit_pattern(&mut self, pattern: &HirPattern) {
        if let HirPattern::Binding(def_id, _) | HirPattern::Alias(def_id, _, _) = pattern {
            if let Some(scope) = self.scopes.last_mut() {
                scope.insert(*def_id);
            }
        }
        walk_pattern(self, pattern);
    }

    fn visit_cape(&mut self, cape: &HirCape) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(cape.binding_def_id);
        }
        walk_cape(self, cape);
    }

    fn visit_object_field(&mut self, field: &radix_hir::HirObjectField) {
        match &field.key {
            radix_hir::HirObjectKey::Ident(_) | radix_hir::HirObjectKey::String(_) => {}
            radix_hir::HirObjectKey::Computed(expr) | radix_hir::HirObjectKey::Spread(expr) => {
                self.visit_in(KernelPosition::Other("field"), expr);
            }
        }
        if let Some(value) = &field.value {
            self.visit_in(KernelPosition::Other("field"), value);
        }
    }

    fn visit_def(&mut self, def_id: DefId, name: Symbol) {
        self.names.entry(def_id).or_insert(name);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Clausura(params, _, err, body, modifier) => match modifier {
                ClosureModifier::Libera => {
                    self.check_libera_closure(params, body);
                    for param in params {
                        self.visit_def(param.def_id, param.name);
                        if let Some(scope) = self.scopes.last_mut() {
                            scope.insert(param.def_id);
                        }
                    }
                    self.visit_expr(body);
                }
                ClosureModifier::Nucleum => match self.position {
                    KernelPosition::CallCallee => {
                        self.check_nucleum_closure(params, body);
                    }
                    KernelPosition::LocalInit | KernelPosition::Other(_) => {
                        let context = self.position.context();
                        self.escape_error(expr.span, context);
                    }
                },
                ClosureModifier::None => walk_expr(self, expr),
            },
            HirExpressionKind::Path(def_id) => {
                if self.kernel_bindings.contains(def_id)
                    && !matches!(self.position, KernelPosition::CallCallee)
                {
                    let context = self.position.context();
                    self.escape_error(expr.span, context);
                }
            }
            HirExpressionKind::Call(callee, _, args, _) => {
                let direct_callee = matches!(
                    &callee.kind,
                    HirExpressionKind::Clausura(.., ClosureModifier::Nucleum)
                ) || matches!(&callee.kind, HirExpressionKind::Path(def_id)
                    if self.kernel_bindings.contains(def_id));
                if direct_callee {
                    self.visit_in(KernelPosition::CallCallee, callee);
                } else {
                    self.visit_in(KernelPosition::Other("ordinary_argument"), callee);
                }
                for arg in args {
                    self.visit_in(KernelPosition::Other("ordinary_argument"), &arg.expr);
                }
            }
            HirExpressionKind::MethodCall(receiver, _, _, args, _) => {
                self.visit_in(KernelPosition::Other("receiver"), receiver);
                for arg in args {
                    self.visit_in(KernelPosition::Other("ordinary_argument"), &arg.expr);
                }
            }
            HirExpressionKind::Struct(_, fields) => {
                for (_, value) in fields {
                    self.visit_in(KernelPosition::Other("field"), value);
                }
            }
            HirExpressionKind::Array(elements) => {
                for element in elements {
                    let expr = match element {
                        radix_hir::HirArrayElement::Expr(expr)
                        | radix_hir::HirArrayElement::Spread(expr) => expr,
                    };
                    self.visit_in(KernelPosition::Other("list"), expr);
                }
            }
            HirExpressionKind::Tuple(items, _) => {
                for item in items {
                    self.visit_in(KernelPosition::Other("tuple"), item);
                }
            }
            HirExpressionKind::Assign(target, value) => {
                self.visit_in(KernelPosition::Other("assignment"), target);
                self.visit_in(KernelPosition::Other("assignment"), value);
            }
            HirExpressionKind::ConversioAssign { source, .. } => {
                self.visit_in(KernelPosition::Other("assignment"), source);
            }
            HirExpressionKind::Itera(_, coordinates, def_id, name, iter, block) => {
                for (coordinate, coordinate_name) in coordinates {
                    self.visit_def(*coordinate, *coordinate_name);
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.insert(*coordinate);
                    }
                }
                self.visit_def(*def_id, *name);
                if let Some(scope) = self.scopes.last_mut() {
                    scope.insert(*def_id);
                }
                self.visit_expr(iter);
                self.visit_block(block);
            }
            _ => walk_expr(self, expr),
        }
    }
}

/// Kernel body-form walk over one `nucleum` closure body. The rejected forms
/// are HIR shapes: alternate exits and error/recovery operations, host
/// effects, and dynamic function values. Shapes not rejected here stay
/// subject to the MIR device-safe classification.
struct KernelBodyProbe {
    diagnostics: Vec<SemanticError>,
}

impl KernelBodyProbe {
    fn effect(&mut self, span: Span, reason: &str, operation: &str) {
        self.diagnostics.push(
            SemanticError::structured(SemanticErrorKind::LoweringError, span)
                .with_arg("issue", ISSUE_NUCLEUM_EFFECT)
                .with_arg("reason", reason)
                .with_arg("operation", operation),
        );
    }
}

impl HirVisitor for KernelBodyProbe {
    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Throw(_) => self.effect(expr.span, "throw", "iace"),
            HirExpressionKind::Handled { .. } => self.effect(expr.span, "cape", "cape"),
            HirExpressionKind::Trap(_) => self.effect(expr.span, "recovery", "capta"),
            HirExpressionKind::Scribe(..) => self.effect(expr.span, "host_effect", "scribe"),
            HirExpressionKind::Scriptum(..) => self.effect(expr.span, "host_effect", "scriptum"),
            HirExpressionKind::ReadLine(_) => self.effect(expr.span, "host_effect", "read_line"),
            HirExpressionKind::Panic(_) => self.effect(expr.span, "host_effect", "panic"),
            HirExpressionKind::Adfirma(..) => self.effect(expr.span, "host_effect", "adfirma"),
            HirExpressionKind::Clausura(..) => {
                self.effect(expr.span, "dynamic_value", "clausura");
            }
            HirExpressionKind::ConversioAssign {
                recovery: Some(_), ..
            } => self.effect(expr.span, "alternate_exit", "conversio_assign_recovery"),
            HirExpressionKind::Conversio {
                recovery: Some(_), ..
            } => self.effect(expr.span, "alternate_exit", "conversio_recovery"),
            _ => walk_expr(self, expr),
        }
    }
}

/// Transitive free-variable probe over one marked closure subtree.
///
/// `scopes` tracks bindings introduced inside the closure (parameters, body
/// locals, pattern/cape/itera bindings, nested closure parameters). A path is
/// a capture when it resolves to an enclosing binding, is not bound inside the
/// closure, and has not been reported yet.
struct CaptureProbe {
    enclosing: FxHashSet<DefId>,
    scopes: Vec<FxHashSet<DefId>>,
    captures: Vec<(DefId, Span)>,
}

impl CaptureProbe {
    fn is_bound(&self, def_id: DefId) -> bool {
        self.scopes.iter().any(|scope| scope.contains(&def_id))
    }
}

impl HirVisitor for CaptureProbe {
    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                if !self.is_bound(*def_id)
                    && self.enclosing.contains(def_id)
                    && self.captures.iter().all(|(captured, _)| captured != def_id)
                {
                    self.captures.push((*def_id, expr.span));
                }
            }
            HirExpressionKind::Itera(_, coordinates, def_id, _, iter, block) => {
                if let Some(scope) = self.scopes.last_mut() {
                    for (coordinate, _) in coordinates {
                        scope.insert(*coordinate);
                    }
                    scope.insert(*def_id);
                }
                self.visit_expr(iter);
                self.visit_block(block);
            }
            _ => walk_expr(self, expr),
        }
    }

    fn visit_block(&mut self, block: &HirBlock) {
        self.scopes.push(FxHashSet::default());
        walk_block(self, block);
        self.scopes.pop();
    }

    fn visit_local(&mut self, local: &HirLocal) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(local.def_id);
        }
        walk_local(self, local);
    }

    fn visit_param(&mut self, param: &HirParam) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(param.def_id);
        }
        if let Some(default) = &param.default {
            self.visit_expr(default);
        }
    }

    fn visit_pattern(&mut self, pattern: &HirPattern) {
        if let HirPattern::Binding(def_id, _) | HirPattern::Alias(def_id, _, _) = pattern {
            if let Some(scope) = self.scopes.last_mut() {
                scope.insert(*def_id);
            }
        }
        walk_pattern(self, pattern);
    }

    fn visit_cape(&mut self, cape: &HirCape) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(cape.binding_def_id);
        }
        walk_cape(self, cape);
    }
}
