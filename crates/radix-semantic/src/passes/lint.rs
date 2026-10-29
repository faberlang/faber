//! Final semantic lint pass over typed HIR.
//!
//! Linting is the last non-codegen semantic pass. It runs after collection,
//! resolution, lowering, typechecking, and any target-enabled borrow and
//! exhaustiveness checks. Its job is to turn fully resolved HIR identity and
//! `TypeTable` facts into user-facing diagnostics for suspicious but often
//! recoverable code: unused declarations, unreachable statements, redundant
//! casts, and explicit `ignotum` annotations.
//!
//! The pass is target-configurable through `PassConfig`. When enabled, it
//! returns both warning-kind diagnostics and hard semantic errors through the
//! same vector because the semantic result owns final severity policy. In
//! particular, shadowing is intentionally a hard error here: resolved `DefId`s
//! make the program unambiguous to the compiler, but the source remains
//! misleading enough that the language rejects it.
//!
//! INVARIANTS
//! ==========
//! - Usage is tracked by `DefId`, never by spelling, so imports, functions, and
//!   locals follow the resolver's identity model.
//! - Scope shadowing checks use a lightweight lexical stack local to this pass;
//!   they do not mutate resolver scopes or type information.
//! - The `TypeTable` is read-only and used only where lint policy needs type
//!   facts, such as redundant `verte` casts or explicit `ignotum` annotations.
//! - Warnings produced here should not block compilation unless the surrounding
//!   diagnostic policy promotes warning-kind diagnostics.

use super::method_receiver_mutation::{self, MutatingMethods};
use crate::{
    Primitive, Resolver, SemanticError, SemanticErrorKind, SemanticParamMode, Type, TypeTable,
    error::WarningKind, locale::LocalePack,
};
use handroll::FxHashSet;
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{
    HirBinOp, HirBlock, HirExpression, HirExpressionKind, HirFunction, HirImport, HirItem,
    HirItemKind, HirLiteral, HirLocal, HirModule, HirParam, HirStatement, HirStatementKind,
};
use radix_intrinsics::{is_mutating_receiver, lookup_method_for_type_id};
use radix_lexer::{Interner, Span};
use radix_types::{IndexExpr, RuntimeExtentId};

const ISSUE_EMPTY_MARKER_SHADOWING: &str = "empty_marker_shadowing";
const ISSUE_EXPLICIT_IGNOTUM_ANNOTATION: &str = "explicit_ignotum_annotation";
const ISSUE_PREFER_GUILLEMET_FOR_QUOTE: &str = "prefer_guillemet_for_quote";
const ISSUE_PREFER_STRING_TEMPLATE: &str = "prefer_string_template";
const ISSUE_SHADOWED_VARIABLE: &str = "shadowed_variable";
const ISSUE_UNNECESSARY_CAST: &str = "unnecessary_cast";
const ISSUE_UNNECESSARY_VARIA: &str = "unnecessary_varia";
const ISSUE_UNREACHABLE_CODE: &str = "unreachable_code";
const ISSUE_UNUSED_BINDING: &str = "unused_binding";
const ISSUE_UNUSED_FUNCTION: &str = "unused_function";
const ISSUE_UNUSED_IMPORT: &str = "unused_import";
const ISSUE_UNUSED_VARIABLE: &str = "unused_variable";
const ISSUE_VALUE_LITERAL_SHADOWING: &str = "value_literal_shadowing";

/// Latin registry keys of the value-literal keywords a binding may shadow.
/// `nihil` is the transitional alias of the `nulla` null literal.
const VALUE_LITERAL_KEYS: [&str; 5] = ["verum", "falsum", "nulla", "nihil", "nonnumerus"];

/// Run the target-enabled semantic lint suite.
///
/// Returns shadowing, unused declarations, and other lint findings as a list
/// of [`SemanticError`]s. Warning-kind diagnostics are non-fatal: they are
/// carried as [`SemanticErrorKind::Warning`] and callers decide fatality via
/// [`SemanticError::is_error`]. Shadowing remains a hard semantic error.
pub fn lint(
    hir: &HirModule,
    resolver: &Resolver,
    types: &TypeTable,
    interner: &Interner,
    locale_pack: Option<&LocalePack>,
) -> Vec<SemanticError> {
    let mut ctx = LintContext::new(resolver, types, interner, locale_pack);
    // A program bound to a target by a target-native import holds foreign
    // values as inferred `ignotum`: that is the design, not an annotation.
    ctx.target_bound = hir.items.iter().any(|item| {
        matches!(&item.kind, radix_hir::HirItemKind::Import(import) if import.target.is_some())
    });
    ctx.mutating_methods = method_receiver_mutation::collect(hir, types, interner);
    ctx.collect_items(hir);
    ctx.mark_type_extent_uses();
    ctx.check_program(hir);

    let mut errors = ctx.errors;

    // Convert warnings to findings (with warning kind). They are non-fatal by
    // kind; fatality is decided downstream via `SemanticError::is_error`.
    errors.extend(ctx.warnings.into_iter().map(
        |(kind, issue, span, suggestion): (WarningKind, &'static str, Span, Option<String>)| {
            let error = SemanticError::structured(SemanticErrorKind::Warning(kind), span)
                .with_arg("issue", issue);
            match suggestion {
                Some(text) => error.with_arg("suggestion", text),
                None => error,
            }
        },
    ));

    errors
}

/// Spellings the active reader surface claims as value-literal keywords.
///
/// Mirrors the parser's keyword claim: with a pack, only spellings the pack
/// declares for one of [`VALUE_LITERAL_KEYS`] claim; without one, the Latin
/// registry spellings do (the `nihil` alias is a type row there, so it is out).
fn value_literal_spellings(locale_pack: Option<&LocalePack>) -> FxHashSet<String> {
    let Some(pack) = locale_pack else {
        return ["verum", "falsum", "nulla", "nonnumerus"]
            .into_iter()
            .map(str::to_owned)
            .collect();
    };
    let mut spellings = FxHashSet::default();
    for key in VALUE_LITERAL_KEYS {
        let candidates = std::iter::once(pack.contextual_keyword_surface(key)).chain(
            pack.contextual_keyword_aliases(key)
                .iter()
                .map(String::as_str),
        );
        for candidate in candidates {
            if pack
                .keyword_latin_key(candidate)
                .is_some_and(|latin| VALUE_LITERAL_KEYS.contains(&latin))
            {
                spellings.insert(candidate.to_owned());
            }
        }
    }
    spellings
}

struct LintContext<'a> {
    /// Whether the module has a target-native import.
    target_bound: bool,
    resolver: &'a Resolver,
    /// Read-only semantic type facts for lints that need more than identity,
    /// such as redundant casts and explicit `ignotum` annotations.
    types: &'a TypeTable,
    interner: &'a Interner,
    warnings: Vec<(WarningKind, &'static str, Span, Option<String>)>,
    errors: Vec<SemanticError>,
    used: FxHashSet<radix_hir::DefId>,
    mutated: FxHashSet<radix_hir::DefId>,
    defs: Vec<(radix_hir::DefId, Span, WarningKind)>,
    mutable_locals: Vec<(radix_hir::DefId, Span)>,
    mutable_local_defs: FxHashSet<radix_hir::DefId>,
    /// Class methods whose body changes the receiver in place.
    mutating_methods: MutatingMethods,
    /// Per declared function: which parameter positions are `mut` (`in`).
    mut_param_positions: handroll::FxHashMap<radix_hir::DefId, Vec<bool>>,
    imports: Vec<(radix_hir::DefId, Span)>,
    functions: Vec<(radix_hir::DefId, Span)>,
    scope: Vec<handroll::FxHashMap<radix_lexer::Symbol, radix_hir::DefId>>,
    /// Declaration names by `DefId`, so suggestions can render path operands
    /// back to their source spellings.
    def_names: handroll::FxHashMap<radix_hir::DefId, radix_lexer::Symbol>,
    /// Active reader-locale spellings that claim the empty-collection marker.
    marker_spellings: FxHashSet<String>,
    /// Active reader-locale spellings that claim a value-literal keyword.
    value_literal_spellings: FxHashSet<String>,
    /// True while the visitor descends into the left operand of a `+`: that
    /// operand belongs to the enclosing `+` chain and must not evaluate the
    /// chain itself.
    inside_add_left_child: bool,
    in_loop: bool,
}

impl<'a> LintContext<'a> {
    fn new(
        resolver: &'a Resolver,
        types: &'a TypeTable,
        interner: &'a Interner,
        locale_pack: Option<&LocalePack>,
    ) -> Self {
        let mut marker_spellings = FxHashSet::default();
        if let Some(pack) = locale_pack {
            marker_spellings.insert(pack.contextual_keyword_surface("vacua").to_owned());
            marker_spellings.extend(pack.contextual_keyword_aliases("vacua").iter().cloned());
        } else {
            marker_spellings.insert("vacua".to_owned());
        }
        let value_literal_spellings = value_literal_spellings(locale_pack);
        Self {
            target_bound: false,
            resolver,
            types,
            interner,
            warnings: Vec::new(),
            errors: Vec::new(),
            used: FxHashSet::default(),
            mutated: FxHashSet::default(),
            defs: Vec::new(),
            mutable_locals: Vec::new(),
            mutable_local_defs: FxHashSet::default(),
            mutating_methods: MutatingMethods::default(),
            mut_param_positions: handroll::FxHashMap::default(),
            imports: Vec::new(),
            functions: Vec::new(),
            scope: Vec::new(),
            def_names: handroll::FxHashMap::default(),
            marker_spellings,
            value_literal_spellings,
            inside_add_left_child: false,
            in_loop: false,
        }
    }

    fn collect_items(&mut self, hir: &HirModule) {
        for item in &hir.items {
            if let HirItemKind::Function(func) = &item.kind {
                self.mut_param_positions.insert(
                    item.def_id,
                    func.params
                        .iter()
                        .map(|param| param.mode == radix_hir::HirParamMode::In)
                        .collect(),
                );
            }
            match &item.kind {
                HirItemKind::Import(import) => self.collect_import(import, item.span),
                // A `@ ad` handler is used by its route, not by name.
                HirItemKind::Function(_)
                    if item
                        .annotations
                        .iter()
                        .any(|annotation| annotation.ad_route(self.interner).is_some()) => {}
                // WHY: Reader-locale `public`/`publica` spellings normalize to family `publica`,
                // as in `is_public_entry`; an export is a use this per-module lint cannot observe.
                HirItemKind::Function(_)
                    if item.annotations.iter().any(|annotation| {
                        self.interner.resolve(annotation.family) == "publica"
                    }) => {}
                // An `@ nondum` function is declared to be unavailable; the
                // semantic pass refuses every use, so "unused" is the point.
                HirItemKind::Function(_)
                    if item
                        .annotations
                        .iter()
                        .any(|annotation| self.interner.resolve(annotation.family) == "nondum") => {
                }
                HirItemKind::Function(func) if !item.def_id.is_hir_synthetic() => {
                    self.functions.push((
                        item.def_id,
                        func.body.as_ref().map_or(item.span, |b| b.span),
                    ));
                }
                _ => {}
            }
        }
    }

    /// `span` is the import statement's: HIR import members carry no span of
    /// their own, so each unused member's warning anchors at its statement.
    fn collect_import(&mut self, import: &HirImport, span: Span) {
        for item in &import.items {
            let binding = item.alias.unwrap_or(item.name);
            if self.resolver.namespace_used(binding) {
                self.used.insert(item.def_id);
            }
            self.imports.push((item.def_id, span));
        }
    }

    /// A scalar binding named in a type's extent (`tf32[size (n - 1)]`) is read
    /// when the shape is evaluated, but the name lives in the type, not in a
    /// `Path` expression. Every such extent is interned as a runtime index
    /// term, so reading those terms is the use.
    fn mark_type_extent_uses(&mut self) {
        for raw in 0..self.types.index_count() {
            let id = radix_types::IndexId(u32::try_from(raw).expect("index arena fits u32"));
            if let IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(def_id)) =
                self.types.get_index(id)
            {
                self.used.insert(*def_id);
            }
        }
    }

    fn check_program(&mut self, hir: &HirModule) {
        for item in &hir.items {
            self.check_item(item);
        }

        if let Some(entry) = &hir.entry {
            self.check_block(entry, false);
        }

        for (def_id, span, kind) in &self.defs {
            if !self.used.contains(def_id) {
                let issue = match kind {
                    WarningKind::UnusedVariable => ISSUE_UNUSED_VARIABLE,
                    _ => ISSUE_UNUSED_BINDING,
                };
                self.warnings.push((*kind, issue, *span, None));
            }
        }

        for (def_id, span) in &self.mutable_locals {
            if self.used.contains(def_id) && !self.mutated.contains(def_id) {
                self.warnings.push((
                    WarningKind::UnnecessaryVaria,
                    ISSUE_UNNECESSARY_VARIA,
                    *span,
                    None,
                ));
            }
        }

        for (def_id, span) in &self.imports {
            if !self.used.contains(def_id) {
                self.warnings
                    .push((WarningKind::UnusedImport, ISSUE_UNUSED_IMPORT, *span, None));
            }
        }

        for (def_id, span) in &self.functions {
            if !self.used.contains(def_id) {
                self.warnings.push((
                    WarningKind::UnusedFunction,
                    ISSUE_UNUSED_FUNCTION,
                    *span,
                    None,
                ));
            }
        }
    }

    fn check_item(&mut self, item: &HirItem) {
        match &item.kind {
            HirItemKind::Function(func) => {
                // em1: a function named like the marker does not shadow it, so
                // the declaration no longer warns; params, locals and consts do.
                self.check_function(func);
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    self.check_function(&method.func);
                }
            }
            HirItemKind::Constant(const_item) => {
                self.warn_on_shadowed_keyword(const_item.name, item.span);
                self.check_expr(&const_item.value, false);
            }
            _ => {}
        }
    }

    fn check_function(&mut self, func: &HirFunction) {
        self.push_scope();
        for param in &func.params {
            self.declare_binding(param.name, param.def_id, param.span, func.body.is_some());
            self.warn_on_explicit_ignotum(param.ty, param.span);
        }
        if let Some(body) = &func.body {
            self.check_block(body, false);
        }
        self.pop_scope();
    }

    fn check_block(&mut self, block: &HirBlock, in_loop: bool) {
        self.push_scope();
        self.check_statements(&block.statements, in_loop);
        if let Some(expr) = &block.expr {
            self.check_expr(expr, in_loop);
        }
        self.pop_scope();
    }

    /// Check a statement run, warning on every statement after the first
    /// terminator. Returns the terminator's index, if any.
    fn check_statements(&mut self, statements: &[HirStatement], in_loop: bool) -> Option<usize> {
        let mut terminator = None;
        for (index, stmt) in statements.iter().enumerate() {
            if terminator.is_some() {
                self.warnings.push((
                    WarningKind::UnreachableCode,
                    ISSUE_UNREACHABLE_CODE,
                    stmt.span,
                    None,
                ));
                continue;
            }
            self.check_stmt(stmt, in_loop);
            if matches!(stmt.kind, HirStatementKind::Redde(_))
                || (in_loop
                    && matches!(stmt.kind, HirStatementKind::Rumpe | HirStatementKind::Perge))
            {
                terminator = Some(index);
            }
        }
        terminator
    }

    /// Ruling R3: `perge` in a `fac … dum` body lands on the synthesized
    /// `si non c { rumpe }` test. A terminated body leaves the test reachable
    /// when any `perge` before the terminator targets this loop.
    fn check_fac_dum(&mut self, body: &[HirStatement], test: &HirStatement) {
        self.push_scope();
        let terminator = self.check_statements(body, true);
        let test_reachable = terminator.is_none_or(|index| {
            let mut perge = PergeFinder::default();
            body[..=index]
                .iter()
                .for_each(|stmt| perge.visit_stmt(stmt));
            perge.found
        });
        if test_reachable {
            self.check_stmt(test, true);
        } else {
            self.warnings.push((
                WarningKind::UnreachableCode,
                ISSUE_UNREACHABLE_CODE,
                test.span,
                None,
            ));
        }
        self.pop_scope();
    }

    fn check_stmt(&mut self, stmt: &HirStatement, in_loop: bool) {
        match &stmt.kind {
            HirStatementKind::Local(local) => self.check_local(local, stmt.span),
            HirStatementKind::Expr(expr) => self.check_expr(expr, in_loop),
            HirStatementKind::Redde(value) | HirStatementKind::ReddeFilum(value) => {
                if let Some(expr) = value {
                    self.check_expr(expr, in_loop);
                }
            }
            HirStatementKind::Rumpe | HirStatementKind::Perge | HirStatementKind::Tacet => {}
            HirStatementKind::IncDec(inc_dec) => {
                self.mark_place_mutated(&inc_dec.target);
                self.check_expr(&inc_dec.target, in_loop);
            }
            HirStatementKind::Custodi(custodi) => {
                for clause in &custodi.clauses {
                    self.check_expr(&clause.cond, in_loop);
                    self.check_block(&clause.body, in_loop);
                }
            }
        }
    }

    fn check_local(&mut self, local: &HirLocal, span: Span) {
        self.declare_binding(local.name, local.def_id, span, true);
        if let Some(ty) = local.ty {
            self.warn_on_explicit_ignotum(ty, span);
        }
        if local.mutable {
            // A `ref` local is writable only as a name (it may be re-pointed),
            // so a ref local that is never re-pointed is not an unneeded `mut`.
            let ref_local = local.ty.is_some_and(|ty| self.types.is_ref_view(ty));
            if !ref_local {
                self.mutable_locals.push((local.def_id, span));
            }
            self.mutable_local_defs.insert(local.def_id);
        }
        if let Some(init) = &local.init {
            self.check_expr(init, false);
        }
    }

    fn check_expr(&mut self, expr: &HirExpression, in_loop: bool) {
        let previous = self.in_loop;
        self.in_loop = in_loop;
        self.visit_expr(expr);
        self.in_loop = previous;
    }

    fn warn_on_shadowed_keyword(&mut self, name: radix_lexer::Symbol, span: Span) {
        let Some(spelling) = self.interner.strings().get(name.0 as usize) else {
            return;
        };
        if self.marker_spellings.contains(spelling) {
            self.warnings.push((
                WarningKind::EmptyMarkerShadowing,
                ISSUE_EMPTY_MARKER_SHADOWING,
                span,
                None,
            ));
        } else if self.value_literal_spellings.contains(spelling) {
            self.errors.push(
                SemanticError::structured(
                    SemanticErrorKind::Warning(WarningKind::EmptyMarkerShadowing),
                    span,
                )
                .with_arg("issue", ISSUE_VALUE_LITERAL_SHADOWING)
                .with_arg("keyword", spelling.clone()),
            );
        }
    }

    fn check_shadowing(&mut self, name: radix_lexer::Symbol, def_id: radix_hir::DefId, span: Span) {
        for scope in self.scope.iter().rev() {
            if let Some(existing) = scope.get(&name) {
                if existing != &def_id {
                    self.errors.push(
                        SemanticError::structured(SemanticErrorKind::ShadowedVariable, span)
                            .with_arg("issue", ISSUE_SHADOWED_VARIABLE),
                    );
                }
                break;
            }
        }
    }

    fn insert_name(&mut self, name: radix_lexer::Symbol, def_id: radix_hir::DefId) {
        if let Some(scope) = self.scope.last_mut() {
            scope.insert(name, def_id);
        }
    }

    /// Record a source binding for shadowing, `DefId` spelling, unused
    /// tracking, and scope insertion — the preamble shared by parameters and
    /// locals. `record_unused` is false for a signature-only parameter, which
    /// has no body that could read it.
    fn declare_binding(
        &mut self,
        name: radix_lexer::Symbol,
        def_id: radix_hir::DefId,
        span: Span,
        record_unused: bool,
    ) {
        self.warn_on_shadowed_keyword(name, span);
        self.def_names.insert(def_id, name);
        if record_unused {
            self.defs.push((def_id, span, WarningKind::UnusedVariable));
        }
        self.check_shadowing(name, def_id, span);
        self.insert_name(name, def_id);
    }

    fn push_scope(&mut self) {
        self.scope.push(handroll::FxHashMap::default());
    }

    fn pop_scope(&mut self) {
        self.scope.pop();
    }

    fn warn_on_explicit_ignotum(&mut self, ty: crate::TypeId, span: Span) {
        if !self.target_bound && matches!(self.types.get(ty), Type::Primitive(Primitive::Ignotum)) {
            self.warnings.push((
                WarningKind::ExplicitIgnotumAnnotation,
                ISSUE_EXPLICIT_IGNOTUM_ANNOTATION,
                span,
                None,
            ));
        }
    }

    /// A `mut` (`in`) parameter is the in-place-mutation mode: the callee may
    /// rewrite the argument's contents and the argument must be mutable, so an
    /// argument place passed to one is mutated like an assignment target. A
    /// `ref`/owned/`ex` position reads (or moves) and does not count.
    fn mark_mut_param_args(&mut self, callee: &HirExpression, args: &[radix_hir::HirCallArg]) {
        let modes: Vec<bool> = match (callee.ty.map(|ty| self.types.get(ty)), &callee.kind) {
            (Some(Type::Func(sig)), _) => sig
                .params
                .iter()
                .map(|param| param.mode == SemanticParamMode::MutRef)
                .collect(),
            (_, HirExpressionKind::Path(def_id)) => self
                .mut_param_positions
                .get(def_id)
                .cloned()
                .unwrap_or_default(),
            _ => return,
        };
        for (arg, is_mut) in args.iter().zip(modes) {
            if is_mut {
                self.mark_place_mutated(&arg.expr);
            }
        }
    }

    fn mark_place_mutated(&mut self, expr: &HirExpression) {
        if let Some(def_id) = root_place_def_id(expr) {
            self.mutated.insert(def_id);
        }
    }

    /// WARN013 means "never modified". A mutating call is a write to its
    /// receiver binding, so it counts: a mutating intrinsic
    /// (`is_mutating_receiver`: `n.adde(1)`, `xs.appende(3)`, `xs.ordina()`) and
    /// a user method whose body changes `ego` both need a `varia` binding, and
    /// the checker rejects either on an immutable one.
    fn mark_receiver_reassigned_if_needed(
        &mut self,
        receiver: &HirExpression,
        method: radix_lexer::Symbol,
        arg_count: usize,
    ) {
        let Some(def_id) = root_place_def_id(receiver) else {
            return;
        };
        if !self.mutable_local_defs.contains(&def_id) {
            return;
        }

        // Keyed on the same registry predicate typecheck rejects with, so the
        // lint never suggests a `fixum` the typechecker would refuse. The
        // lookup peels access markers: a `varia T b ← copy a` binding carries
        // the `copy` wrapper on its type, and its in-place `ordina` still
        // mutates `b`.
        let method_name = self.interner.resolve(method);
        let reassigns = receiver
            .ty
            .and_then(|ty| lookup_method_for_type_id(ty, self.types, method_name, arg_count))
            .is_some_and(is_mutating_receiver);
        let user_mutator = receiver
            .ty
            .and_then(|ty| method_receiver_mutation::class_def(self.types, ty))
            .is_some_and(|class| self.mutating_methods.contains(&(class, method)));
        if reassigns || user_mutator {
            self.mutated.insert(def_id);
        }
    }

    /// Flag a string-typed `+` chain of 3+ operands that contains at least one
    /// string-literal operand (threshold B). The outermost node of the chain
    /// evaluates it once; left-child nodes of a `+` defer to their enclosing
    /// node via `is_add_left_child`.
    fn check_string_concat_chain(&mut self, expr: &HirExpression, is_add_left_child: bool) {
        if is_add_left_child {
            return;
        }
        let Some(ty) = expr.ty else {
            return;
        };
        if !matches!(self.types.get(ty), Type::Primitive(Primitive::Textus)) {
            return;
        }
        let mut operands = Vec::new();
        add_chain_operands(expr, &mut operands);
        if operands.len() < 3 {
            return;
        }
        if !operands.iter().any(|op| is_string_literal(op)) {
            return;
        }
        let suggestion = self.render_template_suggestion(&operands);
        self.warnings.push((
            WarningKind::PreferStringTemplate,
            ISSUE_PREFER_STRING_TEMPLATE,
            expr.span,
            Some(suggestion),
        ));
    }

    /// Render the `"…§…"(args)` template rewrite of a chain, folding literal
    /// text into the template body and turning every other operand into a `§`
    /// hole.
    fn render_template_suggestion(&self, operands: &[&HirExpression]) -> String {
        let mut body = String::new();
        let mut args: Vec<String> = Vec::new();
        for operand in operands {
            if let HirExpressionKind::Literal(
                HirLiteral::String(symbol) | HirLiteral::Ascii(symbol),
            ) = &operand.kind
            {
                let text = self.interner.resolve(*symbol);
                if text.contains('"') || text.contains('§') || text.contains('\\') {
                    // Literal text would corrupt the template rendering;
                    // degrade the segment to a hole.
                    body.push('§');
                    args.push(self.render_operand(operand));
                } else {
                    body.push_str(text);
                }
            } else {
                body.push('§');
                args.push(self.render_operand(operand));
            }
        }
        if args.is_empty() {
            format!("\"{body}\"")
        } else {
            format!("\"{body}\"({})", args.join(", "))
        }
    }

    /// WARN034: a `"…"` literal whose raw payload escapes a quote (`\"`)
    /// reads better as a guillemet block, which needs no escape for `"` or
    /// `'`. HIR cannot distinguish `"…"` from `«…»`; a guillemet literal
    /// that still escapes a quote warns too.
    fn check_guillemet_quote(&mut self, symbol: radix_lexer::Symbol, span: Span) {
        if !has_escaped_quote(self.interner.resolve(symbol)) {
            return;
        }
        self.warnings.push((
            WarningKind::PreferGuillemetQuote,
            ISSUE_PREFER_GUILLEMET_FOR_QUOTE,
            span,
            None,
        ));
    }

    /// Render one operand back to approximate source text for a suggestion.
    fn render_operand(&self, expr: &HirExpression) -> String {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.def_names.get(def_id).map_or_else(
                || "_".to_owned(),
                |symbol| self.interner.resolve(*symbol).to_owned(),
            ),
            HirExpressionKind::Field(base, name) => {
                format!(
                    "{}.{}",
                    self.render_operand(base),
                    self.interner.resolve(*name)
                )
            }
            HirExpressionKind::Literal(HirLiteral::String(symbol) | HirLiteral::Ascii(symbol)) => {
                format!("\"{}\"", self.interner.resolve(*symbol))
            }
            _ => "_".to_owned(),
        }
    }
}

/// Flatten a `+` chain into its operand list in source order.
fn add_chain_operands<'x>(expr: &'x HirExpression, operands: &mut Vec<&'x HirExpression>) {
    match &expr.kind {
        HirExpressionKind::Binary(HirBinOp::Add, lhs, rhs) => {
            add_chain_operands(lhs, operands);
            operands.push(rhs);
        }
        _ => operands.push(expr),
    }
}

/// Whether a raw textus payload contains a `\"` escape. A backslash consumes
/// the character that follows it, so `\\` stays a bare backslash and the quote
/// after it is unescaped.
fn has_escaped_quote(raw: &str) -> bool {
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            continue;
        }
        if chars.next() == Some('"') {
            return true;
        }
    }
    false
}

/// Whether an operand is a string or ascii literal (threshold-B trigger and
/// suggestion-folding candidate).
fn is_string_literal(expr: &HirExpression) -> bool {
    matches!(
        &expr.kind,
        HirExpressionKind::Literal(HirLiteral::String(_) | HirLiteral::Ascii(_))
    )
}

impl HirVisitor for LintContext<'_> {
    fn visit_param(&mut self, param: &HirParam) {
        self.warn_on_shadowed_keyword(param.name, param.span);
        if let Some(default) = &param.default {
            self.visit_expr(default);
        }
    }

    fn visit_block(&mut self, block: &HirBlock) {
        self.check_block(block, self.in_loop);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                self.used.insert(*def_id);
            }
            // D9.3: an explicit `↦` through a registered `@ conversio` pair
            // calls that function.
            HirExpressionKind::Conversio {
                route: Some(radix_hir::HirConversioRoute::Function { def_id, .. }),
                ..
            } => {
                self.used.insert(*def_id);
                walk_expr(self, expr);
            }
            HirExpressionKind::Loop(block) => match block.fac_dum_parts() {
                Some((body, test, _)) => self.check_fac_dum(body, test),
                None => self.check_block(block, true),
            },
            HirExpressionKind::Dum(cond, block) => {
                self.visit_expr(cond);
                self.check_block(block, true);
            }
            HirExpressionKind::Itera {
                iter, body: block, ..
            } => {
                self.visit_expr(iter);
                self.check_block(block, true);
            }
            HirExpressionKind::Verte {
                source,
                target,
                entries,
            } => {
                if entries.is_none()
                    && let Some(inner_ty) = source.ty
                    && self.types.equals(inner_ty, *target)
                {
                    self.warnings.push((
                        WarningKind::UnnecessaryCast,
                        ISSUE_UNNECESSARY_CAST,
                        expr.span,
                        None,
                    ));
                }
                walk_expr(self, expr);
            }
            // `⇇` writes the existing storage its target names exactly like
            // ordinary assignment: for the `var` lint that is a mutation of
            // the target place.
            HirExpressionKind::Assign(target, value)
            | HirExpressionKind::CopyInto { target, value } => {
                self.mark_place_mutated(target);
                self.visit_expr(target);
                self.visit_expr(value);
            }
            // `sink ⇜ item` writes through the sink: a mutation of its place.
            HirExpressionKind::SinkInto { sink, item } => {
                self.mark_place_mutated(sink);
                self.visit_expr(sink);
                self.visit_expr(item);
            }
            HirExpressionKind::Literal(HirLiteral::String(symbol)) => {
                self.check_guillemet_quote(*symbol, expr.span);
                walk_expr(self, expr);
            }
            // String-typed `+` chains: the outermost node of a left-spine run
            // evaluates the whole chain once; nested nodes defer to it.
            HirExpressionKind::Binary(HirBinOp::Add, lhs, rhs) => {
                let was_add_left_child = self.inside_add_left_child;
                self.inside_add_left_child = false;
                self.check_string_concat_chain(expr, was_add_left_child);
                self.inside_add_left_child = true;
                self.visit_expr(lhs);
                self.inside_add_left_child = false;
                self.visit_expr(rhs);
                self.inside_add_left_child = was_add_left_child;
            }
            // `↤` mutates its target place exactly like ordinary assignment:
            // lint's mutation tracking must see the store.
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
                ..
            } => {
                self.mark_place_mutated(target);
                self.visit_expr(target);
                self.visit_expr(source);
                if let Some(recovery) = recovery {
                    self.visit_expr(recovery);
                }
            }
            HirExpressionKind::MethodCall(receiver, method, _, args, _) => {
                self.mark_receiver_reassigned_if_needed(receiver, *method, args.len());
                self.visit_expr(receiver);
                for arg in args {
                    self.visit_expr(&arg.expr);
                }
            }
            HirExpressionKind::Call(callee, _, args, _) => {
                self.mark_mut_param_args(callee, args);
                walk_expr(self, expr);
            }
            HirExpressionKind::Adfirma(cond, message) => {
                self.visit_expr(cond);
                if let Some(message) = message {
                    self.visit_expr(message);
                }
            }
            _ => walk_expr(self, expr),
        }
    }
}

/// Finds a `perge` that continues the enclosing loop: nested loops and
/// closures own their own `perge`, so the search stops at them.
#[derive(Default)]
struct PergeFinder {
    found: bool,
}

impl HirVisitor for PergeFinder {
    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if matches!(stmt.kind, HirStatementKind::Perge) {
            self.found = true;
        }
        radix_hir::visit::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        if !matches!(
            expr.kind,
            HirExpressionKind::Loop(_)
                | HirExpressionKind::Dum(..)
                | HirExpressionKind::Itera { .. }
                | HirExpressionKind::IteraLockstep { .. }
                | HirExpressionKind::IteraRange { .. }
                | HirExpressionKind::Clausura(..)
        ) {
            walk_expr(self, expr);
        }
    }
}

fn root_place_def_id(expr: &HirExpression) -> Option<radix_hir::DefId> {
    match &expr.kind {
        HirExpressionKind::Path(def_id) => Some(*def_id),
        HirExpressionKind::Field(base, _)
        | HirExpressionKind::Deref(base)
        | HirExpressionKind::Index(base, _)
        | HirExpressionKind::OptionalChain(base, _)
        | HirExpressionKind::NonNull(base, _) => root_place_def_id(base),
        _ => None,
    }
}
