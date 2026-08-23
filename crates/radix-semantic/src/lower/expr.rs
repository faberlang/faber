//! Expression normalization for the AST-to-HIR boundary.
//!
//! This module lowers parser expressions into HIR nodes that later phases can
//! reason about without re-reading surface syntax. It preserves source spans and
//! HIR node identity, resolves names that must already be in scope, normalizes
//! syntactic sugar such as string-template calls and ternaries, and keeps
//! target-sensitive constructs in explicit HIR shapes for typecheck and codegen.
//!
//! INVARIANTS
//! ==========
//! - Every lowered expression receives a fresh HIR id and the original source
//!   span that should anchor later diagnostics.
//! - Lowering does not infer final expression types. The optional `ty` slot is
//!   left for typecheck except where a target type is part of the syntax itself.
//! - Unknown names or unsupported expression forms become diagnostics plus
//!   `HirExpressionKind::Error`, allowing downstream passes to continue collecting
//!   errors instead of crashing.
//! - Surface conveniences are normalized only when the meaning is syntactic:
//!   overload resolution, receiver typing, and backend legality remain later
//!   phase responsibilities.
//!
//! BOUNDARIES
//! ==========
//! Nullable-sensitive forms (`?.`, non-null chains, `nihil` literals, and
//! `ignotum`-adjacent conversions) are represented, not proven, here. This file
//! records the operation and source structure; typecheck decides whether the
//! operation is valid, and codegen decides how a valid operation is emitted.

use super::{retired_bare_object_literal_args, Lowerer};
use crate::builtins::forma::{self as builtin_forma};
use crate::template_holes::{analyze_template, TemplateHoleAnalysis};
use crate::{Primitive, TypeId};
use radix_diagnostics::DiagnosticArg;
use radix_hir::{
    HirArrayElement, HirBinOp, HirCallArg, HirConversioTarget, HirExpression, HirExpressionKind,
    HirLiteral, HirNonNullKind, HirObjectField, HirObjectKey, HirOptionalChainKind, HirUnOp,
};
use radix_syntax::{BinaryExpr, Expr, ExprKind, Literal, UnaryExpr};

/// Lower one AST expression into the HIR expression contract.
///
/// The entry point owns the cross-cutting guarantees for this file: assign a
/// fresh HIR id, preserve the AST span for diagnostics, leave final typing to
/// typecheck, and recover unsupported forms as `Error` nodes after recording a
/// diagnostic. Nested expression lowerers return only the kind so this wrapper
/// remains the single place that stamps identity onto expression nodes.
pub fn lower_expr(lowerer: &mut Lowerer, expr: &Expr) -> HirExpression {
    let id = lowerer.next_hir_id();
    let span = expr.span;
    lowerer.current_span = span;

    let kind = match &expr.kind {
        ExprKind::Literal(lit) => lower_literal(lowerer, lit),
        ExprKind::Vacua(ident) => lowerer
            .lower_shadowed_builtin(expr.id, ident.name)
            .unwrap_or(HirExpressionKind::Vacua),
        ExprKind::Ident(ident) => lowerer.lower_nomen(ident),
        ExprKind::Binary(bin) => lowerer.lower_binarius(bin),
        ExprKind::Unary(unary) => lowerer.lower_unarius(unary),
        ExprKind::Ternary(ternary) => lowerer.lower_ternary(ternary),
        ExprKind::Call(call) => lowerer.lower_vocare(call),
        ExprKind::Member(member) => lowerer.lower_membrum(member),
        ExprKind::Index(index) => lowerer.lower_index(index),
        ExprKind::Gradient(gradient) => lowerer.lower_gradient(gradient),
        ExprKind::OptionalChain(optional) => lowerer.lower_optional_chain(optional),
        ExprKind::NonNull(non_null) => lowerer.lower_non_null(non_null),
        ExprKind::Assign(assign) => lowerer.lower_assign(assign),
        ExprKind::Verte(verte) => lowerer.lower_verte(verte),
        ExprKind::Cede(cede_expr) => lowerer.lower_cede(cede_expr),
        ExprKind::Array(array) => lowerer.lower_serie(array),
        ExprKind::Object(_) => {
            // WHY: anonymous Faber object literals are retired (literal-family
            // Stage 6). Bare `{ }` is now JSON valor; `ExprKind::Object` survives
            // only inside `Verte` for genus construction, lowered by `lower_verte`.
            // Reaching this arm means a parser invariant broke.
            lowerer.error_with_args(retired_bare_object_literal_args());
            HirExpressionKind::Error
        }
        ExprKind::Finge(finge) => lowerer.lower_finge(finge),
        ExprKind::Clausura(closure) => lowerer.lower_clausura(closure),
        ExprKind::Intervallum(range) => lowerer.lower_intervallum(range),
        ExprKind::Conversio(conversio) => lowerer.lower_conversio(conversio),
        ExprKind::Ad(ad_expr) => lowerer.lower_ad_expr(ad_expr),
        ExprKind::Scriptum(scriptum) => lowerer.lower_scriptum_or_binding(expr.id, scriptum),
        ExprKind::Lege(lege) => lowerer
            .lower_shadowed_builtin(expr.id, lege.symbol)
            .unwrap_or(HirExpressionKind::ReadLine(lege.is_line)),

        ExprKind::Praefixum(praefixum) => lowerer.lower_praefixum(praefixum),
        ExprKind::Ego(span) => lowerer.lower_ego(*span),
        ExprKind::Paren(expr) => lower_expr(lowerer, expr).kind,
        ExprKind::TypeCheck(type_check) => lowerer.lower_type_check(type_check),
        ExprKind::Iuncta(iuncta) => lowerer.lower_iuncta(iuncta),
        ExprKind::Summa(summa) => lower_summa(lowerer, summa),
    };

    HirExpression {
        id,
        kind,
        ty: None,
        span,
    }
}

/// Lower the source shape of `summa ex` while preserving its declaration in HIR.
///
/// The node is intentionally not normalized to `Itera`: `summa` is an
/// expression with a term body, while `itera` remains a statement with its own
/// control-flow and mutation semantics. SFR-3/SFR-4 own the admission and
/// lowering rules after this source-shaped node lands.
fn lower_summa(lowerer: &mut Lowerer, summa: &radix_syntax::SummaExpr) -> HirExpressionKind {
    let source = lower_expr(lowerer, &summa.source);

    lowerer.push_scope();
    let coordinates = summa
        .apud
        .iter()
        .map(|ident| {
            let def_id = lowerer.next_def_id();
            lowerer.bind_local(ident.name, def_id);
            (def_id, ident.name)
        })
        .collect();
    let filum = summa.filum.as_ref().map(|ident| {
        let def_id = lowerer.next_def_id();
        lowerer.bind_local(ident.name, def_id);
        (def_id, ident.name)
    });
    let binding_def_id = lowerer.next_def_id();
    lowerer.bind_local(summa.binding.name, binding_def_id);
    let body = lowerer.lower_block(&summa.body);
    lowerer.pop_scope();

    HirExpressionKind::Summa {
        source: Box::new(source),
        coordinates,
        filum,
        binding: (binding_def_id, summa.binding.name),
        body,
    }
}

/// Lower a literal
fn lower_literal(lowerer: &mut Lowerer, lit: &Literal) -> HirExpressionKind {
    match lit {
        Literal::Forma(template) => lowerer.lower_forma_capture(*template, Vec::new()),
        _ => match super::literal::literal_to_hir(lit) {
            Ok(literal) => HirExpressionKind::Literal(literal),
            Err(message) => {
                lowerer.error_from_message(message);
                HirExpressionKind::Error
            }
        },
    }
}

impl Lowerer<'_> {
    /// Resolve an expression identifier that must already name something.
    ///
    /// Pattern and declaration lowering allocate new bindings; expression
    /// lowering only consumes resolver state. A missing name is therefore a
    /// recoverable semantic error, not an opportunity to invent a binding.
    /// Variants no longer occupy the bare scope table: a bare identifier that
    /// the per-union namespace resolves unambiguously lowers to the variant
    /// path (exactly today's single-union behavior). An ambiguous bare name is
    /// an error, never a silent pick.
    fn lower_nomen(&mut self, ident: &radix_syntax::Ident) -> HirExpressionKind {
        if let Some(def_id) = self.lookup_name(ident.name) {
            return HirExpressionKind::Path(def_id);
        }
        match self.resolver.lookup_variant_bare(ident.name) {
            crate::BareVariantLookup::Single(def_id) => HirExpressionKind::Path(def_id),
            crate::BareVariantLookup::Ambiguous { .. } => {
                self.current_span = ident.span;
                self.error("undefined_name");
                HirExpressionKind::Error
            }
            crate::BareVariantLookup::None => {
                self.current_span = ident.span;
                self.error("undefined_name");
                HirExpressionKind::Error
            }
        }
    }

    /// Lower binary expression (binarius)
    fn lower_binarius(&mut self, bin: &BinaryExpr) -> HirExpressionKind {
        let lhs = lower_expr(self, &bin.lhs);
        let rhs = lower_expr(self, &bin.rhs);

        let op = match bin.op {
            radix_syntax::BinOp::Add => HirBinOp::Add,
            radix_syntax::BinOp::Sub => HirBinOp::Sub,
            radix_syntax::BinOp::Mul => HirBinOp::Mul,
            radix_syntax::BinOp::Div => HirBinOp::Div,
            radix_syntax::BinOp::Mod => HirBinOp::Mod,
            radix_syntax::BinOp::Dot => HirBinOp::Dot,
            radix_syntax::BinOp::Cross => HirBinOp::Cross,
            radix_syntax::BinOp::TensorProduct => HirBinOp::TensorProduct,
            radix_syntax::BinOp::Hadamard => HirBinOp::Hadamard,
            radix_syntax::BinOp::Eq => HirBinOp::Eq,
            radix_syntax::BinOp::NotEq => HirBinOp::NotEq,
            radix_syntax::BinOp::ApproxEq => HirBinOp::ApproxEq,
            radix_syntax::BinOp::ApproxNotEq => HirBinOp::ApproxNotEq,
            radix_syntax::BinOp::StrictEq => HirBinOp::StrictEq,
            radix_syntax::BinOp::StrictNotEq => HirBinOp::StrictNotEq,
            radix_syntax::BinOp::Lt => HirBinOp::Lt,
            radix_syntax::BinOp::Gt => HirBinOp::Gt,
            radix_syntax::BinOp::LtEq => HirBinOp::LtEq,
            radix_syntax::BinOp::GtEq => HirBinOp::GtEq,
            radix_syntax::BinOp::And => HirBinOp::And,
            radix_syntax::BinOp::Or => HirBinOp::Or,
            radix_syntax::BinOp::Coalesce => HirBinOp::Coalesce,
            radix_syntax::BinOp::BitAnd => HirBinOp::BitAnd,
            radix_syntax::BinOp::BitOr => HirBinOp::BitOr,
            radix_syntax::BinOp::BitXor => HirBinOp::BitXor,
            radix_syntax::BinOp::Shl => HirBinOp::Shl,
            radix_syntax::BinOp::Shr => HirBinOp::Shr,
            radix_syntax::BinOp::Is => HirBinOp::Is,
            radix_syntax::BinOp::IsNot => HirBinOp::IsNot,
            radix_syntax::BinOp::InRange => HirBinOp::InRange,
            radix_syntax::BinOp::Between => HirBinOp::Between,
        };

        HirExpressionKind::Binary(op, Box::new(lhs), Box::new(rhs))
    }

    /// Lower unary expression (unarius)
    fn lower_unarius(&mut self, unary: &UnaryExpr) -> HirExpressionKind {
        let operand = lower_expr(self, &unary.operand);

        let op = match unary.op {
            radix_syntax::UnOp::Neg => HirUnOp::Neg,
            radix_syntax::UnOp::Not => HirUnOp::Not,
            radix_syntax::UnOp::BitNot => HirUnOp::BitNot,
        };

        HirExpressionKind::Unary(op, Box::new(operand))
    }

    fn lower_ternary(&mut self, ternary: &radix_syntax::TernaryExpr) -> HirExpressionKind {
        let cond = lower_expr(self, &ternary.cond);
        let then_expr = lower_expr(self, &ternary.then);
        let else_expr = lower_expr(self, &ternary.else_);
        HirExpressionKind::Si {
            cond: Box::new(cond),
            then_block: self.block_from_expr(then_expr),
            then_catch: None,
            else_block: Some(self.block_from_expr(else_expr)),
        }
    }

    /// Record a lowering error when template application carries turbofish.
    fn reject_template_call_type_args(&mut self, type_args: &[TypeId]) -> bool {
        if type_args.is_empty() {
            return false;
        }
        self.error("template_type_args");
        true
    }

    /// Normalize call syntax while preserving the semantic call boundary.
    ///
    /// String literals in callee position are Faber template application, and
    /// member callees are kept as `MethodCall` so receiver typing can select the
    /// method later. Ordinary calls retain an explicit callee expression.
    fn lower_vocare(&mut self, call: &radix_syntax::CallExpr) -> HirExpressionKind {
        let type_args: Vec<TypeId> = call
            .type_args
            .iter()
            .map(|ty| self.lower_type(ty))
            .collect();
        match &call.callee.kind {
            ExprKind::Literal(Literal::String(template)) => {
                if self.reject_template_call_type_args(&type_args) {
                    return HirExpressionKind::Error;
                }
                match self.lower_template_application(*template, &call.args) {
                    Some((erased, args)) => HirExpressionKind::Scriptum(erased, args),
                    None => HirExpressionKind::Error,
                }
            }
            ExprKind::Literal(Literal::Forma(template)) => {
                if self.reject_template_call_type_args(&type_args) {
                    return HirExpressionKind::Error;
                }
                match self.lower_template_application(*template, &call.args) {
                    Some((erased, args)) => self.lower_forma_capture(erased, args),
                    None => HirExpressionKind::Error,
                }
            }
            ExprKind::Member(member) => {
                let (recv, name) = self.flatten_dotted_method_callee(member);
                let name = self.canonicalize_namespace_member(&recv, name);
                let args = self.lower_call_args(&call.args);
                HirExpressionKind::MethodCall(Box::new(recv), name, type_args, args)
            }
            _ => {
                let callee = lower_expr(self, &call.callee);
                let args = self.lower_call_args(&call.args);
                HirExpressionKind::Call(Box::new(callee), type_args, args)
            }
        }
    }

    fn lower_call_args(&mut self, args: &[radix_syntax::Argument]) -> Vec<HirCallArg> {
        args.iter()
            .map(|arg| {
                if let Some(label) = arg.label {
                    let name = self.interner.resolve(label).to_owned();
                    self.error_with_args_at(
                        vec![
                            DiagnosticArg::new("issue", "labeled_actual_non_template_call"),
                            DiagnosticArg::new("label", name),
                        ],
                        arg.span,
                    );
                }
                HirCallArg {
                    name: None,
                    spread: arg.is_spread,
                    expr: lower_expr(self, &arg.value),
                    span: arg.span,
                }
            })
            .collect()
    }

    /// Erase named holes and check labeled actuals at a template-call site.
    ///
    /// Bare (unapplied) literals keep their raw text; only application rewrites
    /// the template to its positional `§`-form and reorders labeled actuals.
    fn lower_template_application(
        &mut self,
        template: radix_lexer::Symbol,
        args: &[radix_syntax::Argument],
    ) -> Option<(radix_lexer::Symbol, Vec<HirExpression>)> {
        let call_span = self.current_span;
        let text = self.interner.resolve(template).to_owned();
        let analysis = analyze_template(&text);
        let lowered: Vec<(Option<radix_lexer::Symbol>, HirExpression)> = args
            .iter()
            .map(|arg| (arg.label, lower_expr(self, &arg.value)))
            .collect();

        let mut ok = true;
        for label in &analysis.duplicate_labels {
            self.error_with_args_at(
                vec![
                    DiagnosticArg::new("issue", "duplicate_template_hole_label"),
                    DiagnosticArg::new("label", label.clone()),
                ],
                call_span,
            );
            ok = false;
        }

        let has_labeled = lowered.iter().any(|(label, _)| label.is_some());
        let has_unlabeled = lowered.iter().any(|(label, _)| label.is_none());
        if has_labeled && has_unlabeled {
            self.error_with_args_at(
                vec![DiagnosticArg::new(
                    "issue",
                    "mixed_template_actuals_unsupported",
                )],
                call_span,
            );
            ok = false;
        }

        if has_labeled && !has_unlabeled {
            for (label, _) in &lowered {
                let Some(label) = *label else {
                    continue;
                };
                let name = self.interner.resolve(label).to_owned();
                if analysis.label_position(&name).is_none() {
                    self.error_with_args_at(
                        vec![
                            DiagnosticArg::new("issue", "unknown_template_hole_label"),
                            DiagnosticArg::new("label", name),
                        ],
                        call_span,
                    );
                    ok = false;
                }
            }
            for name in analysis.label_positions.keys() {
                let filled = lowered.iter().any(|(label, _)| {
                    label.is_some_and(|symbol| self.interner.resolve(symbol) == name)
                });
                if !filled {
                    self.error_with_args_at(
                        vec![
                            DiagnosticArg::new("issue", "unfillable_template_hole"),
                            DiagnosticArg::new("label", name.clone()),
                        ],
                        call_span,
                    );
                    ok = false;
                }
            }
        }

        if lowered.len() != analysis.holes.len() {
            self.error_with_args_at(
                vec![
                    DiagnosticArg::new("issue", "template_application_arity_mismatch"),
                    DiagnosticArg::new("actual", lowered.len().to_string()),
                    DiagnosticArg::new("expected", analysis.holes.len().to_string()),
                ],
                call_span,
            );
            ok = false;
        }

        if !ok {
            return None;
        }

        let erased = self.interner.intern(&analysis.erased);
        let positional = if has_labeled {
            self.reorder_labeled_actuals(&analysis, lowered)
        } else {
            lowered.into_iter().map(|(_, expr)| expr).collect()
        };
        Some((erased, positional))
    }

    /// Place labeled actuals into the positional vector the backends already consume.
    fn reorder_labeled_actuals(
        &self,
        analysis: &TemplateHoleAnalysis,
        lowered: Vec<(Option<radix_lexer::Symbol>, HirExpression)>,
    ) -> Vec<HirExpression> {
        let slot_count = analysis
            .holes
            .iter()
            .map(|hole| hole.position)
            .max()
            .map_or(0, |position| position.saturating_add(1));
        let mut slots: Vec<Option<HirExpression>> = (0..slot_count).map(|_| None).collect();
        for (label, expr) in lowered {
            let label = label.expect("labeled template actuals are present");
            let name = self.interner.resolve(label);
            let position = analysis
                .label_position(name)
                .expect("unknown labels already rejected");
            slots[position] = Some(expr);
        }
        slots
            .into_iter()
            .map(|slot| slot.expect("labeled actuals fill every position"))
            .collect()
    }

    /// Collapse namespace calls like `faber.json.solve(args)` into receiver
    /// `faber` and method name `json.solve`.
    ///
    /// WHY: module/kernel re-export calls use dotted method symbols, while
    /// value receiver chains such as `item.values.longitudo()` must preserve
    /// the immediate method boundary so typecheck/codegen see `item.values` as
    /// the receiver.
    fn flatten_dotted_method_callee(
        &mut self,
        member: &radix_syntax::MemberExpr,
    ) -> (radix_hir::HirExpression, radix_lexer::Symbol) {
        if self.dotted_call_root_is_value(&member.object) {
            return (lower_expr(self, &member.object), member.member.name);
        }
        let mut segment_names = vec![member.member.name];
        let mut object = &member.object;
        while let radix_syntax::ExprKind::Member(inner) = &object.kind {
            segment_names.insert(0, inner.member.name);
            object = &inner.object;
        }
        if let radix_syntax::ExprKind::Ident(base) = &object.kind {
            if let Some(module) = segment_names.first().copied() {
                if let Some(def_id) = self.kernel_glob_imports.get(&(base.name, module)).copied() {
                    if segment_names.len() == 1 {
                        self.error("kernel_glob_call_shape");
                    }
                    let recv = radix_hir::HirExpression {
                        id: self.next_hir_id(),
                        kind: radix_hir::HirExpressionKind::Path(def_id),
                        ty: None,
                        span: object.span,
                    };
                    let combined = segment_names
                        .iter()
                        .skip(1)
                        .map(|sym| self.interner.resolve(*sym))
                        .collect::<Vec<_>>()
                        .join(".");
                    let name = self.interner.intern(&combined);
                    return (recv, name);
                }
                if self.kernel_glob_aliases.contains(&base.name) {
                    let module_name = self.interner.resolve(module);
                    self.error_with_args(vec![
                        radix_diagnostics::DiagnosticArg::new("issue", "kernel_unknown_module"),
                        radix_diagnostics::DiagnosticArg::new("module", module_name),
                    ]);
                }
            }
        }
        let recv = lower_expr(self, object);
        let combined = segment_names
            .iter()
            .map(|sym| self.interner.resolve(*sym))
            .collect::<Vec<_>>()
            .join(".");
        let name = self.interner.intern(&combined);
        (recv, name)
    }

    fn dotted_call_root_is_value(&self, expr: &radix_syntax::Expr) -> bool {
        let mut object = expr;
        while let radix_syntax::ExprKind::Member(inner) = &object.kind {
            object = &inner.object;
        }
        let radix_syntax::ExprKind::Ident(base) = &object.kind else {
            return true;
        };
        let Some(def_id) = self.lookup_name(base.name) else {
            return false;
        };
        !matches!(
            self.resolver.get_symbol(def_id).map(|symbol| symbol.kind),
            Some(crate::SymbolKind::Module)
        )
    }

    /// Lower member access, including the enum-variant shorthand.
    ///
    /// If the left side is a known enum and the member names a known variant,
    /// this is a path to the variant constructor rather than a field access.
    /// Receiver-field legality remains a typecheck concern for every other
    /// member expression.
    fn lower_membrum(&mut self, member: &radix_syntax::MemberExpr) -> HirExpressionKind {
        if let radix_syntax::ExprKind::Ident(base) = &member.object.kind {
            if let Some(base_def) = self.lookup_name(base.name) {
                if let Some(base_symbol) = self.resolver.get_symbol(base_def) {
                    if base_symbol.kind == crate::SymbolKind::Enum {
                        if let Some(variant_def) =
                            self.resolver.variant_child(base_def, member.member.name)
                        {
                            return HirExpressionKind::Path(variant_def);
                        }
                        self.error("unknown_variant");
                        return HirExpressionKind::Error;
                    }
                }
            }
        }
        // Imported namespace variant values (`module.Type.VARIANT`, uvf-u4):
        // the object is a two-segment path whose penultimate segment names a
        // type export of the module. When the terminal segment is one of that
        // type's variants, lower the whole path to the variant definition —
        // mirroring the same-module enum shorthand above and the qualified
        // `finge`/pattern-path resolution. The intermediate `module.Type` has
        // no expression role of its own; only the full variant path is a
        // first-class value reference.
        if let radix_syntax::ExprKind::Member(obj) = &member.object.kind {
            if let radix_syntax::ExprKind::Ident(base) = &obj.object.kind {
                if let Some(base_def) = self.lookup_name(base.name) {
                    if let Some(base_symbol) = self.resolver.get_symbol(base_def) {
                        if base_symbol.kind == crate::SymbolKind::Module {
                            if let Some((variant_def, _)) =
                                self.resolver.namespace_file_type_variant(
                                    base.name,
                                    obj.member.name,
                                    member.member.name,
                                )
                            {
                                return HirExpressionKind::Path(variant_def);
                            }
                            // The penultimate segment names a type export. A
                            // member of a variant-bearing (enum/union) export
                            // that is not one of its declared variants is a
                            // genuine miss — fail closed like the same-module
                            // enum shorthand. Struct exports keep the
                            // field-access path.
                            if let Some(type_def) = self
                                .resolver
                                .namespace_file_type_def(base.name, obj.member.name)
                            {
                                if self.resolver.imported_nominal_has_variants(type_def) {
                                    self.error("unknown_variant");
                                    return HirExpressionKind::Error;
                                }
                            }
                        }
                    }
                }
            }
        }

        let object = lower_expr(self, &member.object);
        let field = self.canonicalize_namespace_member(&object, member.member.name);
        if let Some(kind) = self.lower_tuple_label_member(&object, field, member.member.span) {
            return kind;
        }

        HirExpressionKind::Field(Box::new(object), field)
    }

    /// Rewrite `p.gx` on a labeled `iuncta` binding to the same constant-index
    /// node as `p[0]`. Unknown labels emit `unknown_tuple_element_label`.
    fn lower_tuple_label_member(
        &mut self,
        object: &radix_hir::HirExpression,
        label: radix_lexer::Symbol,
        member_span: radix_lexer::Span,
    ) -> Option<HirExpressionKind> {
        let radix_hir::HirExpressionKind::Path(def_id) = &object.kind else {
            return None;
        };
        let pos = {
            let labels = self.resolver.tuple_element_labels(*def_id)?;
            crate::tuple_labels::position_of(labels, label)
        };
        if let Some(pos) = pos {
            let index = radix_hir::HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Literal(HirLiteral::Int(pos as u64)),
                ty: None,
                span: member_span,
            };
            return Some(HirExpressionKind::Index(
                Box::new(object.clone()),
                Box::new(index),
            ));
        }
        self.current_span = member_span;
        self.error("unknown_tuple_element_label");
        Some(HirExpressionKind::Error)
    }

    /// HIR stores canonical library export names. Locale surfaces exist only
    /// at the source boundary.
    fn canonicalize_namespace_member(
        &self,
        recv: &radix_hir::HirExpression,
        name: radix_lexer::Symbol,
    ) -> radix_lexer::Symbol {
        let radix_hir::HirExpressionKind::Path(def_id) = &recv.kind else {
            return name;
        };
        let Some(symbol) = self.resolver.get_symbol(*def_id) else {
            return name;
        };
        if symbol.kind != crate::SymbolKind::Module {
            return name;
        }
        self.resolver
            .imported_export_canonical_name(symbol.name, name)
    }

    fn lower_optional_chain(
        &mut self,
        expr: &radix_syntax::OptionalChainExpr,
    ) -> HirExpressionKind {
        let object = lower_expr(self, &expr.object);
        let chain = match &expr.chain {
            radix_syntax::OptionalChainKind::Member(member) => {
                HirOptionalChainKind::Member(member.name)
            }
            radix_syntax::OptionalChainKind::Index(index) => {
                HirOptionalChainKind::Index(Box::new(lower_expr(self, index)))
            }
            radix_syntax::OptionalChainKind::Call(args) => {
                HirOptionalChainKind::Call(self.lower_call_args(args))
            }
        };
        HirExpressionKind::OptionalChain(Box::new(object), chain)
    }

    fn lower_non_null(&mut self, expr: &radix_syntax::NonNullExpr) -> HirExpressionKind {
        let object = lower_expr(self, &expr.object);
        let chain = match &expr.chain {
            radix_syntax::NonNullKind::Member(member) => HirNonNullKind::Member(member.name),
            radix_syntax::NonNullKind::Index(index) => {
                HirNonNullKind::Index(Box::new(lower_expr(self, index)))
            }
            radix_syntax::NonNullKind::Call(args) => {
                HirNonNullKind::Call(self.lower_call_args(args))
            }
        };
        HirExpressionKind::NonNull(Box::new(object), chain)
    }

    fn lower_index(&mut self, index: &radix_syntax::IndexExpr) -> HirExpressionKind {
        let object = lower_expr(self, &index.object);
        let idx = lower_expr(self, &index.index);

        HirExpressionKind::Index(Box::new(object), Box::new(idx))
    }

    fn lower_gradient(&mut self, gradient: &radix_syntax::GradientExpr) -> HirExpressionKind {
        let call = lower_expr(self, &gradient.call);
        let selected_arguments = gradient
            .selectors
            .iter()
            .map(|selector| lower_expr(self, selector))
            .collect();

        HirExpressionKind::Gradient {
            call: Box::new(call),
            selected_arguments,
        }
    }

    fn lower_assign(&mut self, assign: &radix_syntax::AssignExpr) -> HirExpressionKind {
        let target = lower_expr(self, &assign.target);
        let source = lower_expr(self, &assign.value);

        match assign.op {
            radix_syntax::AssignOp::Assign => {
                HirExpressionKind::Assign(Box::new(target), Box::new(source))
            }
            // `↤` (U+21A4) is conversion-directed assignment: the operator is
            // preserved through HIR (never erased into `Assign`) until the
            // destination type resolves in typecheck, and the `⇥` inline
            // recovery travels with the node. The right side must go through
            // the same conversion registry as an explicit `↦ TargetType`.
            radix_syntax::AssignOp::ConversioAssign => {
                let recovery = assign
                    .recovery
                    .as_ref()
                    .map(|fallback| Box::new(lower_expr(self, fallback)));
                HirExpressionKind::ConversioAssign {
                    target: Box::new(target),
                    source: Box::new(source),
                    recovery,
                }
            }
        }
    }

    fn lower_cede(&mut self, cede_expr: &radix_syntax::CedeExpr) -> HirExpressionKind {
        let expr = lower_expr(self, &cede_expr.expr);
        HirExpressionKind::Yield(Box::new(expr))
    }

    /// Lower static type ascription (`∷`) into target-driven HIR.
    ///
    /// The grammar exposes one conversion operator; later phases dispatch on
    /// the resolved target type rather than on a source keyword. Object literal
    /// sources are split into entries so struct and collection construction can
    /// retain named fields without requiring a dedicated object-expression HIR
    /// shape.
    fn lower_verte(&mut self, verte: &radix_syntax::VerteExpr) -> HirExpressionKind {
        let target = self.lower_type(&verte.ty);
        if let radix_syntax::ExprKind::Object(object) = &verte.expr.kind {
            let mut entries = Vec::new();
            for field in &object.fields {
                entries.push(self.lower_object_field(field));
            }

            let placeholder = HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Tuple(Vec::new(), None),
                ty: None,
                span: self.current_span,
            };
            HirExpressionKind::Verte {
                source: Box::new(placeholder),
                target,
                entries: Some(entries),
            }
        } else {
            let source = lower_expr(self, &verte.expr);
            HirExpressionKind::Verte {
                source: Box::new(source),
                target,
                entries: None,
            }
        }
    }

    fn lower_serie(&mut self, array: &radix_syntax::ArrayExpr) -> HirExpressionKind {
        let mut elements = Vec::new();
        for el in &array.elements {
            match el {
                radix_syntax::ArrayElement::Expr(e) => {
                    elements.push(HirArrayElement::Expr(lower_expr(self, e)));
                }
                radix_syntax::ArrayElement::Spread(e) => {
                    elements.push(HirArrayElement::Spread(lower_expr(self, e)));
                }
            }
        }

        HirExpressionKind::Array(elements)
    }

    /// Lower iuncta tuple construction: `iuncta<A, B> [a, b]`.
    ///
    /// Type arguments from the source syntax are lowered and preserved so
    /// typecheck can use them when no contextual expected tuple is available.
    fn lower_iuncta(&mut self, iuncta: &radix_syntax::IunctaExpr) -> HirExpressionKind {
        let mut elements = Vec::new();
        for expr in &iuncta.elements {
            elements.push(lower_expr(self, expr));
        }
        let type_args: Vec<TypeId> = iuncta
            .type_args
            .iter()
            .map(|ta| self.lower_type(ta))
            .collect();
        let type_args = if type_args.is_empty() {
            None
        } else {
            Some(type_args)
        };
        HirExpressionKind::Tuple(elements, type_args)
    }

    fn lower_object_field(&mut self, field: &radix_syntax::ObjectField) -> HirObjectField {
        let key = match &field.key {
            radix_syntax::ObjectKey::Ident(ident) => HirObjectKey::Ident(ident.name),
            radix_syntax::ObjectKey::String(string) => HirObjectKey::String(*string),
            radix_syntax::ObjectKey::Computed(expr) => {
                HirObjectKey::Computed(lower_expr(self, expr))
            }
            radix_syntax::ObjectKey::Spread(expr) => HirObjectKey::Spread(lower_expr(self, expr)),
        };

        let value = match &field.value {
            Some(value) => Some(lower_expr(self, value)),
            None => match &field.key {
                radix_syntax::ObjectKey::Ident(ident) => Some(HirExpression {
                    id: self.next_hir_id(),
                    kind: self.lower_nomen(ident),
                    ty: None,
                    span: ident.span,
                }),
                radix_syntax::ObjectKey::String(_)
                | radix_syntax::ObjectKey::Computed(_)
                | radix_syntax::ObjectKey::Spread(_) => None,
            },
        };

        HirObjectField { key, value }
    }

    /// Lower variant-construction sugar into a constructor call plus optional cast.
    ///
    /// `finge` names must already resolve to variants or type-bearing cast
    /// symbols. The lowerer records missing resolver information as diagnostics
    /// because fabricating a constructor or target type here would hide the real
    /// upstream symbol-table problem. Variants no longer occupy the bare scope
    /// table, so a failed bare lookup falls back to the per-union namespace;
    /// a multi-union bare name is an error, never a silent pick.
    ///
    /// A two-segment qualified variant path (`finge pkg.Bonum { … }`)
    /// resolves the terminal segment against the penultimate segment's union
    /// when it names one. A path with leading path segments (`finge a.A.X
    /// { … }`) resolves the leading path as a real qualifier first — the
    /// qualified imported parent binds through the imported union's parent
    /// link (CTO UVN-1), and a prefix that is not a real qualifier fails
    /// closed (CG2-R07). Otherwise (imported variants reached through their
    /// module alias) the terminal segment resolves through the bare table and
    /// the per-union namespace, exactly the single-segment policy (uvf-u3).
    fn lower_finge(&mut self, finge: &radix_syntax::FingeExpr) -> HirExpressionKind {
        let Some(last) = finge.variant.last() else {
            self.error("unknown_finge_variant");
            return HirExpressionKind::Error;
        };
        let resolved = if finge.variant.len() >= 3 {
            // CG2-R07: a path with leading path segments must resolve them as
            // a real qualifier before the penultimate segment can name the
            // parent union — `finge missing.A.X { … }` must never bind the
            // local `A.X`, and a shadowing local enum must never capture a
            // qualified imported construction.
            let symbols: Vec<radix_lexer::Symbol> =
                finge.variant.iter().map(|ident| ident.name).collect();
            match self.resolver.qualified_variant_path(&symbols) {
                crate::QualifiedVariantPathLookup::Variant(def_id) => Some(def_id),
                crate::QualifiedVariantPathLookup::UnknownVariant
                | crate::QualifiedVariantPathLookup::NotQualified => None,
            }
        } else if finge.variant.len() >= 2 {
            let parent = &finge.variant[finge.variant.len() - 2];
            if let Some(Some(variant_def)) =
                self.resolver.lookup(parent.name).and_then(|parent_def| {
                    self.resolver
                        .get_symbol(parent_def)
                        .filter(|symbol| symbol.kind == crate::SymbolKind::Enum)
                        .map(|_| self.resolver.variant_child(parent_def, last.name))
                })
            {
                Some(variant_def)
            } else {
                // The penultimate segment names no visible enum (imported
                // variants are reached through their namespace, so `pkg.Bonum`
                // never binds `pkg` as the union). A qualified imported parent
                // (`a.A.X`) resolves through the imported union's parent link
                // BEFORE any bare fallback (CTO UVN-1): two imported unions
                // may share the terminal variant name, and a miss against an
                // identified variant-bearing parent fails closed instead of
                // binding through another union. Anything else keeps the
                // terminal segment's own identity through the bare table and
                // the per-union namespace — exactly the single-segment policy
                // (uvf-u3).
                let symbols: Vec<radix_lexer::Symbol> =
                    finge.variant.iter().map(|ident| ident.name).collect();
                match self.resolver.qualified_variant_path(&symbols) {
                    crate::QualifiedVariantPathLookup::Variant(def_id) => Some(def_id),
                    crate::QualifiedVariantPathLookup::UnknownVariant => None,
                    crate::QualifiedVariantPathLookup::NotQualified => self
                        .resolver
                        .lookup(last.name)
                        .or_else(|| match self.resolver.lookup_variant_bare(last.name) {
                            crate::BareVariantLookup::Single(def_id) => Some(def_id),
                            crate::BareVariantLookup::Ambiguous { .. }
                            | crate::BareVariantLookup::None => None,
                        }),
                }
            }
        } else {
            self.resolver.lookup(last.name).or_else(|| {
                match self.resolver.lookup_variant_bare(last.name) {
                    crate::BareVariantLookup::Single(def_id) => Some(def_id),
                    crate::BareVariantLookup::Ambiguous { .. } | crate::BareVariantLookup::None => {
                        None
                    }
                }
            })
        };
        let Some(variant_def_id) = resolved else {
            self.current_span = last.span;
            self.error("unknown_finge_variant");
            return HirExpressionKind::Error;
        };

        let callee = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Path(variant_def_id),
            ty: None,
            span: last.span,
        };
        let args = finge
            .fields
            .iter()
            .map(|field| HirCallArg {
                name: Some(field.name.name),
                spread: false,
                expr: lower_expr(self, &field.value),
                span: field.span,
            })
            .collect();

        let call = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Call(Box::new(callee), Vec::new(), args),
            ty: None,
            span: self.current_span,
        };

        if let Some(cast) = &finge.cast {
            // The cast is a full type annotation; lower it through the normal
            // type path so a qualified name (`∷ pkg.Exitus`) resolves the
            // imported union identity. Diagnostics for a non-type or unknown
            // cast surface as the standard type-lowering errors.
            let cast_ty = self.lower_type(cast);
            if cast_ty == self.types.intern(crate::Type::Error) {
                return HirExpressionKind::Error;
            }
            return HirExpressionKind::Verte {
                source: Box::new(call),
                target: cast_ty,
                entries: None,
            };
        }

        call.kind
    }

    /// Lower closures with a temporary scope for their parameters.
    ///
    /// Parameter bindings are available while lowering the body and disappear
    /// before the enclosing expression continues. Return and alternate-exit type
    /// annotations remain optional HIR metadata for typecheck to reconcile with
    /// the body.
    fn lower_clausura(&mut self, closure: &radix_syntax::ClausuraExpr) -> HirExpressionKind {
        let mut params = Vec::new();
        self.push_scope();
        for param in &closure.params {
            let def_id = self.next_def_id();
            let ty = self.lower_parameter_type(&param.ty);
            let mode = match param.ty.mode {
                Some(type_mode) => super::type_mode_hir_param_mode(type_mode),
                None => radix_hir::HirParamMode::Owned,
            };
            params.push(radix_hir::HirParam {
                def_id,
                name: param.name.name,
                ty,
                mode,
                optional: false,
                sponte: false,

                default: None,
                span: param.span,
            });
            self.bind_local(param.name.name, def_id);
        }

        let body = match &closure.body {
            radix_syntax::ClausuraBody::Expr(expr) => lower_expr(self, expr),
            radix_syntax::ClausuraBody::Block(block) => HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Block(self.lower_block(block)),
                ty: None,
                span: block.span,
            },
            radix_syntax::ClausuraBody::Fac(stmt) => self.lower_fac_expr(stmt),
        };
        self.pop_scope();

        let ret = closure.ret.as_ref().map(|ret| self.lower_type(ret));
        let err = closure.err.as_ref().map(|err| self.lower_type(err));
        HirExpressionKind::Clausura(params, ret, err, Box::new(body), closure.libera)
    }

    fn lower_intervallum(&mut self, range: &radix_syntax::IntervallumExpr) -> HirExpressionKind {
        HirExpressionKind::Intervallum {
            start: Box::new(lower_expr(self, &range.start)),
            end: Box::new(lower_expr(self, &range.end)),
            step: range
                .step
                .as_ref()
                .map(|step| Box::new(lower_expr(self, step))),
            kind: match range.kind {
                radix_syntax::RangeKind::Exclusive => radix_hir::HirRangeKind::Exclusive,
                radix_syntax::RangeKind::Inclusive => radix_hir::HirRangeKind::Inclusive,
            },
        }
    }

    /// Lower explicit conversion syntax and collect target-shape hints.
    ///
    /// Conversion hints are symbols, not resolved types. They preserve user
    /// intent for downstream conversion analysis without forcing expression
    /// lowering to know every target-specific coercion rule.
    fn lower_conversio(&mut self, conversio: &radix_syntax::ConversioExpr) -> HirExpressionKind {
        let source = lower_expr(self, &conversio.expr);
        let mut params = Vec::new();
        let target = match &conversio.target {
            radix_syntax::ConversioTarget::Explicit(ty) => {
                HirConversioTarget::Type(lower_conversio_target_type(self, ty, &mut params))
            }
            radix_syntax::ConversioTarget::Intervallum(expr) => {
                HirConversioTarget::Intervallum(Box::new(lower_expr(self, expr)))
            }
        };
        params.extend(
            conversio
                .type_params
                .iter()
                .filter_map(conversio_hint_symbol),
        );
        // Fold constant literal sources to `HirLiteral::Regex` so regex values
        // share one HIR shape with future slash literals (Part B). Guard on the
        // absence of recovery: typecheck rejects `↦ regex ⇥ …`, but lowering
        // may still run after error recovery. Never silently drop recovery.
        let regex_ty = self.types.primitive(Primitive::Regex);
        if matches!(target, HirConversioTarget::Type(ty) if ty == regex_ty)
            && conversio.recovery.is_none()
        {
            if let Some(pattern) = regex_pattern_symbol(&source) {
                return HirExpressionKind::Literal(HirLiteral::Regex(pattern, None));
            }
        }
        let recovery = conversio
            .recovery
            .as_ref()
            .map(|fb| Box::new(lower_expr(self, fb)));
        HirExpressionKind::Conversio {
            source: Box::new(source),
            target,
            params,
            recovery,
        }
    }

    /// Lower `expr est <type>` / `expr non est <type>` into a `TypeCheck` HIR node.
    /// The target type is resolved here; typecheck and codegen decide whether
    /// it maps to a Valor variant tag check.
    fn lower_type_check(&mut self, tc: &radix_syntax::TypeCheckExpr) -> HirExpressionKind {
        let expr = lower_expr(self, &tc.expr);
        let ty = self.lower_type(&tc.ty);
        HirExpressionKind::TypeCheck {
            positive: tc.positive,
            expr: Box::new(expr),
            ty,
        }
    }

    /// Lower backtick template capture into builtin `forma` genus construction.
    ///
    /// Backtick application captures template text and parameters; it never routes
    /// through `scriptum` rendering.
    // consumes args into expression
    fn lower_forma_capture(
        &mut self,
        template: radix_lexer::Symbol,
        args: Vec<HirExpression>,
    ) -> HirExpressionKind {
        let Some(forma_def) = crate::builtins::forma_def_id(self.resolver) else {
            self.error("forma_builtin_missing");
            return HirExpressionKind::Error;
        };

        let target = builtin_forma::type_id(self.types, forma_def);
        let template_expr = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Literal(HirLiteral::String(template)),
            ty: None,
            span: self.current_span,
        };
        let Some(template_field) = builtin_forma::template_field(self.resolver) else {
            self.error("forma_template_field_missing");
            return HirExpressionKind::Error;
        };
        let Some(params_field) = builtin_forma::params_field(self.resolver) else {
            self.error("forma_params_field_missing");
            return HirExpressionKind::Error;
        };

        let params_expr = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Array(args.into_iter().map(HirArrayElement::Expr).collect()),
            ty: None,
            span: self.current_span,
        };
        let entries = vec![
            HirObjectField {
                key: HirObjectKey::Ident(template_field),
                value: Some(template_expr),
            },
            HirObjectField {
                key: HirObjectKey::Ident(params_field),
                value: Some(params_expr),
            },
        ];
        let placeholder = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Tuple(Vec::new(), None),
            ty: None,
            span: self.current_span,
        };
        HirExpressionKind::Verte {
            source: Box::new(placeholder),
            target,
            entries: Some(entries),
        }
    }

    fn lower_scriptum(&mut self, scriptum: &radix_syntax::ScriptumExpr) -> HirExpressionKind {
        let args = scriptum
            .args
            .iter()
            .map(|arg| lower_expr(self, arg))
            .collect();
        HirExpressionKind::Scriptum(scriptum.template, args)
    }

    /// When resolve recorded a user binding for a builtin-claimed word, emit
    /// a name load through the lowerer's own scope table. Resolver DefIds for
    /// locals are not the HIR identities later passes walk.
    fn lower_shadowed_builtin(
        &mut self,
        node: radix_syntax::NodeId,
        name: radix_lexer::Symbol,
    ) -> Option<HirExpressionKind> {
        self.resolver.shadowed_builtin(node)?;
        Some(if let Some(def_id) = self.lookup_name(name) {
            HirExpressionKind::Path(def_id)
        } else {
            self.error("undefined_name");
            HirExpressionKind::Error
        })
    }

    fn lower_scriptum_or_binding(
        &mut self,
        node: radix_syntax::NodeId,
        scriptum: &radix_syntax::ScriptumExpr,
    ) -> HirExpressionKind {
        let Some(kind) = self.lower_shadowed_builtin(node, scriptum.symbol) else {
            return self.lower_scriptum(scriptum);
        };
        let span = self.current_span;
        let callee = HirExpression {
            id: self.next_hir_id(),
            kind,
            ty: None,
            span,
        };
        let mut args = Vec::with_capacity(scriptum.args.len() + 1);
        args.push(HirCallArg {
            name: None,
            spread: false,
            expr: HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Literal(HirLiteral::String(scriptum.template)),
                ty: None,
                span,
            },
            span,
        });
        for arg in &scriptum.args {
            let expr = lower_expr(self, arg);
            args.push(HirCallArg {
                name: None,
                spread: false,
                expr,
                span: arg.span,
            });
        }
        HirExpressionKind::Call(Box::new(callee), Vec::new(), args)
    }

    fn lower_praefixum(&mut self, praefixum: &radix_syntax::PraefixumExpr) -> HirExpressionKind {
        let inner = match &praefixum.body {
            radix_syntax::PraefixumBody::Expr(expr) => lower_expr(self, expr),
            radix_syntax::PraefixumBody::Block(block) => HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Block(self.lower_block(block)),
                ty: None,
                span: self.current_span,
            },
        };
        HirExpressionKind::Praefixum(Box::new(inner))
    }

    fn lower_ego(&mut self, span: radix_lexer::Span) -> HirExpressionKind {
        self.current_span = span;
        if let Some(def_id) = self.current_ego_struct {
            HirExpressionKind::Path(def_id)
        } else {
            self.error("ego_outside_method");
            HirExpressionKind::Error
        }
    }

    /// Lower expression-form blockless `ad` to HIR.
    fn lower_ad_expr(&mut self, ad_expr: &radix_syntax::AdExpr) -> HirExpressionKind {
        let opener = ad_expr
            .opener
            .as_ref()
            .map(|expr| Box::new(lower_expr(self, expr)));
        HirExpressionKind::Ad {
            route: ad_expr.path,
            opener,
        }
    }
}

fn lower_conversio_target_type(
    lowerer: &mut Lowerer<'_>,
    ty: &radix_syntax::TypeExpr,
    params: &mut Vec<radix_lexer::Symbol>,
) -> TypeId {
    match &ty.kind {
        radix_syntax::TypeExprKind::Named(ident, target_params)
            if lowerer.is_lexical_type_param(ident.name) =>
        {
            params.extend(target_params.iter().filter_map(conversio_hint_symbol));
            lowerer.lower_type(ty)
        }
        radix_syntax::TypeExprKind::Named(ident, target_params)
            if let Some(prim) = Primitive::from_name(lowerer.interner.resolve(ident.name))
                && prim == Primitive::Instans =>
        {
            let mut messages = Vec::new();
            let ty_id = crate::instans_precision::lower_sized_instans(
                target_params,
                lowerer.interner,
                lowerer.types,
                &mut |message| messages.push(message.to_owned()),
            );
            emit_first_target_lowering_message(lowerer, &messages);
            apply_type_target_wrappers(lowerer, ty, ty_id)
        }
        radix_syntax::TypeExprKind::Named(ident, target_params)
            if matches!(
                Primitive::from_name(lowerer.interner.resolve(ident.name)),
                Some(Primitive::Ascii)
            ) && target_params.len() == 2
                && !ascii_format_hints(&target_params[1..], lowerer.interner).is_empty() =>
        {
            // Slot 2 Hex/Bin/Oct is a conversio format hint, not type identity.
            params.extend(ascii_format_hints(&target_params[1..], lowerer.interner));
            let ty_id = lowerer.lower_bounded_ascii_capacity(&target_params[0]);
            apply_type_target_wrappers(lowerer, ty, ty_id)
        }
        radix_syntax::TypeExprKind::Named(ident, target_params)
            if matches!(
                Primitive::from_name(lowerer.interner.resolve(ident.name)),
                Some(Primitive::Ascii | Primitive::Octeti | Primitive::Textus)
            ) && !target_params.is_empty() =>
        {
            // Bounded ascii/octeti/textus are real conversion targets, not
            // radix hints. Preserve capacity so typecheck and Rust emit can
            // enforce D7 (`len ≤ N`, spare slots unread).
            lowerer.lower_type(ty)
        }
        radix_syntax::TypeExprKind::Named(ident, target_params)
            if let Some(prim) = Primitive::from_name(lowerer.interner.resolve(ident.name))
                && matches!(prim, Primitive::Numerus | Primitive::Fractus) =>
        {
            params.extend(conversio_radix_hints(
                target_params.get(1..).unwrap_or(&[]),
                lowerer.interner,
            ));
            let width_params = match target_params.first() {
                Some(param) => std::slice::from_ref(param),
                None => &[],
            };
            let mut messages = Vec::new();
            let ty_id = crate::numeric_width::lower_sized_primitive(
                prim,
                width_params,
                lowerer.interner,
                lowerer.types,
                &mut |message| messages.push(message.to_owned()),
            );
            emit_first_target_lowering_message(lowerer, &messages);
            apply_type_target_wrappers(lowerer, ty, ty_id)
        }
        radix_syntax::TypeExprKind::Named(ident, target_params)
            if matches!(
                Primitive::from_name(lowerer.interner.resolve(ident.name)),
                Some(
                    Primitive::Ascii
                        | Primitive::Bivalens
                        | Primitive::Ignotum
                        | Primitive::Nihil
                        | Primitive::Numquam
                        | Primitive::Octeti
                        | Primitive::Regex
                        | Primitive::Textus
                        | Primitive::Vacuum
                        | Primitive::Valor
                )
            ) =>
        {
            params.extend(target_params.iter().filter_map(conversio_hint_symbol));
            let bare = radix_syntax::TypeExpr {
                nullable: ty.nullable,
                mode: ty.mode,
                kind: radix_syntax::TypeExprKind::Named(
                    radix_syntax::Ident {
                        name: ident.name,
                        span: ident.span,
                    },
                    Vec::new(),
                ),
                span: ty.span,
                iuncta_label: None,
            };
            lowerer.lower_type(&bare)
        }
        _ => lowerer.lower_type(ty),
    }
}

fn emit_first_target_lowering_message(lowerer: &mut Lowerer<'_>, messages: &[String]) {
    if let Some(message) = messages.first() {
        lowerer.error_from_message(message);
    }
}

fn apply_type_target_wrappers(
    lowerer: &mut Lowerer<'_>,
    ty: &radix_syntax::TypeExpr,
    mut ty_id: TypeId,
) -> TypeId {
    if let Some(mode) = ty.mode {
        let marker = crate::access_marker_from_type_mode(mode);
        if lowerer.types.is_scalar_for_access_marker(ty_id) {
            lowerer.error(crate::ISSUE_SCALAR_ACCESS_MARKER);
            ty_id = lowerer.types.intern(crate::Type::Error);
        } else {
            ty_id = lowerer.types.access_marker(marker, ty_id);
        }
    }
    if ty.nullable {
        ty_id = lowerer.types.option(ty_id);
    }
    ty_id
}

fn conversio_radix_hints(
    target_params: &[radix_syntax::TypeExpr],
    interner: &radix_lexer::Interner,
) -> Vec<radix_lexer::Symbol> {
    target_params
        .iter()
        .filter_map(conversio_hint_symbol)
        .filter(|symbol| crate::numeric_width::is_conversio_radix_hint(interner.resolve(*symbol)))
        .collect()
}

fn ascii_format_hints(
    target_params: &[radix_syntax::TypeExpr],
    interner: &radix_lexer::Interner,
) -> Vec<radix_lexer::Symbol> {
    target_params
        .iter()
        .filter_map(conversio_hint_symbol)
        .filter(|symbol| crate::numeric_width::is_ascii_format_hint(interner.resolve(*symbol)))
        .collect()
}

/// Extract a user-written conversion hint from a type expression.
///
/// Only bare named syntax contributes a hint. More complex type expressions are
/// still lowered as target types, but they are not collapsed into a single
/// symbol because that would lose the structure typecheck needs later.
fn conversio_hint_symbol(ty: &radix_syntax::TypeExpr) -> Option<radix_lexer::Symbol> {
    match &ty.kind {
        radix_syntax::TypeExprKind::Named(ident, _) => Some(ident.name),
        _ => None,
    }
}

fn regex_pattern_symbol(expr: &HirExpression) -> Option<radix_lexer::Symbol> {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::String(sym) | HirLiteral::Ascii(sym)) => Some(*sym),
        _ => None,
    }
}
