//! Base-lane `libera` capture-free closure contract.
//!
//! A closure marked `libera` (reader `free`) must not capture any local or
//! parameter of an enclosing function: its environment must be empty. Module
//! items, builtins, enum variants, and imported members are statically
//! resolved identities, not captures, and remain legal. Unmarked closures are
//! untouched — this pass fires only on the marker.
//!
//! The check is structural over typed HIR and runs on every lane; AIR
//! consumption of the marker is a separate unit (`air-reducta-cumulata` 4).

use crate::{Resolver, SemanticError, SemanticErrorKind};
use radix_hir::visit::{
    walk_block, walk_cape, walk_expr, walk_local, walk_pattern, HirVisitor,
};
use radix_hir::{
    DefId, HirBlock, HirCape, HirExpression, HirExpressionKind, HirFunction, HirLocal, HirModule,
    HirParam, HirPattern,
};
use radix_lexer::{Interner, Span, Symbol};
use rustc_hash::{FxHashMap, FxHashSet};

/// Structured issue for a marked closure that captures an enclosing local/param.
pub const ISSUE_LIBERA_CAPTURE: &str = "libera_capture";

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
            self.scopes = vec![FxHashSet::default()];
            self.visit_block(entry);
            self.scopes = saved;
        }
    }

    fn visit_function(&mut self, function: &HirFunction) {
        let saved = std::mem::take(&mut self.scopes);
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
        if let Some(body) = &function.body {
            self.visit_block(body);
        }
        self.scopes = saved;
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
        self.visit_def(param.def_id, param.name);
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

    fn visit_def(&mut self, def_id: DefId, name: Symbol) {
        self.names.entry(def_id).or_insert(name);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Clausura(params, _, _, body, libera) => {
                if *libera {
                    self.check_libera_closure(params, body);
                }
                for param in params {
                    self.visit_def(param.def_id, param.name);
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.insert(param.def_id);
                    }
                }
                self.visit_expr(body);
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
