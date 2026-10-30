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

use super::{Lowerer, issue_args};
use crate::builtins::forma::{self as builtin_forma};
use crate::template_holes::{TemplateHoleAnalysis, analyze_template};
use crate::{Primitive, SemanticErrorKind, TypeId, WarningKind};
use handroll::FxHashSet;
use radix_diagnostics::DiagnosticArg;
use radix_hir::{
    HirArrayElement, HirBinOp, HirCallArg, HirConversioTarget, HirExpression, HirExpressionKind,
    HirId, HirLiteral, HirNonNullKind, HirObjectField, HirObjectKey, HirOptionalChainKind, HirUnOp,
};
use radix_syntax::{
    BinaryExpr, Expr, ExprKind, GenericArgExpr, Literal, TypeExpr, TypeExprKind, UnaryExpr,
};

/// The authored form of a template application with named holes: the authored
/// template text and the labeled actuals as `(label, positional slot)`.
type TemplateAuthored = Option<(radix_lexer::Symbol, Vec<(radix_lexer::Symbol, u32)>)>;

/// The parser's private method name for the source-only `ᵀ` suffix (the same
/// spelling typecheck rewrites to the transpose intrinsic).
const GLYPH_TRANSPOSE_MARKER: &str = "__faber_glyph_transpose";
const ROUNDING_GLYPH_MARKER: &str = "__faber_glyph_round:";

/// Lower one AST expression into the HIR expression contract.
///
/// The entry point owns the cross-cutting guarantees for this file: assign a
/// fresh HIR id, preserve the AST span for diagnostics, leave final typing to
/// typecheck, and recover unsupported forms as `Error` nodes after recording a
/// diagnostic. Nested expression lowerers return only the kind so this wrapper
/// remains the single place that stamps identity onto expression nodes.
pub fn lower_expr(lowerer: &mut Lowerer, expr: &Expr) -> HirExpression {
    // Parentheses are not HIR nodes. Lower the inner expression under its own
    // identity so the presentation records keyed to it (an authored `∷`
    // ascription's `AscriptionTarget` site, for example) still name a node in
    // the finalized HIR. The authored paren extent stays the node's span.
    if let ExprKind::Paren(inner) = &expr.kind {
        let mut child = lower_expr(lowerer, inner);
        child.span = expr.span;
        return child;
    }
    let id = lowerer.next_hir_id();
    let span = expr.span;
    lowerer.current_span = span;

    let kind = match &expr.kind {
        ExprKind::Literal(lit) => lower_literal(lowerer, lit),
        ExprKind::Vacua(ident) => {
            lowerer.intern_typed_zero_crea_names();
            lowerer
                .lower_shadowed_builtin(expr.id, ident.name)
                .unwrap_or(HirExpressionKind::Vacua)
        }
        ExprKind::KeywordLiteral(keyword) => {
            match lowerer.lower_shadowed_builtin(expr.id, keyword.ident.name) {
                Some(shadowed) => shadowed,
                None => lower_literal(lowerer, &keyword.literal),
            }
        }
        ExprKind::Ident(ident) => lowerer.lower_nomen(ident),
        ExprKind::Binary(bin) => lowerer.lower_binarius(bin),
        ExprKind::Unary(unary) => lowerer.lower_unarius(unary, id),
        ExprKind::Ternary(ternary) => lowerer.lower_ternary(ternary),
        ExprKind::Call(call) => lowerer.lower_vocare(call, id),
        ExprKind::Member(member) => lowerer.lower_membrum(member, id),
        ExprKind::Index(index) => lowerer.lower_index(index),
        ExprKind::Gradient(gradient) => lowerer.lower_gradient(gradient),
        ExprKind::OptionalChain(optional) => lowerer.lower_optional_chain(optional),
        ExprKind::NonNull(non_null) => lowerer.lower_non_null(non_null),
        ExprKind::Assign(assign) => lowerer.lower_assign(assign),
        ExprKind::Verte(verte) => lowerer.lower_verte(verte, id),
        ExprKind::Cede(cede_expr) => lowerer.lower_cede(cede_expr),
        ExprKind::Array(array) => lowerer.lower_serie(array),
        ExprKind::Object(_) => {
            // WHY: bare `{ }` is a JSON valor; `ExprKind::Object` survives
            // only inside `Verte` for genus construction, lowered by `lower_verte`.
            // Reaching this arm means a parser invariant broke.
            lowerer.error_with_args(issue_args("unknown_lowering_error"));
            HirExpressionKind::Error
        }
        ExprKind::Finge(finge) => lowerer.lower_finge(finge, id),
        ExprKind::Clausura(closure) => lowerer.lower_clausura(closure, id),
        ExprKind::Intervallum(range) => lowerer.lower_intervallum(range),
        ExprKind::Conversio(conversio) => lowerer.lower_conversio(conversio),
        ExprKind::Ad(ad_expr) => lowerer.lower_ad_expr(ad_expr),
        ExprKind::Scriptum(scriptum) => lowerer.lower_scriptum_or_binding(expr.id, scriptum),
        ExprKind::Lege(lege) => lowerer
            .lower_shadowed_builtin(expr.id, lege.symbol)
            .unwrap_or(HirExpressionKind::ReadLine(lege.is_line)),

        // `praefixum` is legal only as the whole initializer of a `=`
        // constant (module level, block level, genus field default), which
        // lowers through `lower_compile_time_initializer` (D8.6, st1 R6).
        // Every other position (a runtime `←` store, an operand, an
        // argument) reaches here.
        ExprKind::Praefixum(_) => {
            lowerer.push_error(
                SemanticErrorKind::PraefixumInvalid,
                vec![DiagnosticArg::new("issue", "praefixum_outside_constant")],
                expr.span,
            );
            HirExpressionKind::Error
        }
        // The parser claims `insere "path"` in any expression position; it
        // is legal only inside a compile-time initializer tree (st1 R5).
        ExprKind::Insere(insere) => lowerer.lower_insere(insere, id, span),
        ExprKind::Ego(span) => lowerer.lower_ego(*span),
        // Parentheses carry no HIR node; the early return above lowers them.
        // Reaching this arm means that invariant was violated after id allocation.
        ExprKind::Paren(_) => {
            lowerer.error_with_args(issue_args("unknown_lowering_error"));
            HirExpressionKind::Error
        }
        ExprKind::TypeCheck(type_check) => lowerer.lower_type_check(type_check, id),
        ExprKind::Iuncta(iuncta) => lowerer.lower_iuncta(iuncta, id),
        ExprKind::Construct(ty) => lowerer.lower_construct(ty, id, expr.span),
        ExprKind::Reducta(reduction) => lower_reducta(lowerer, reduction),
        ExprKind::Scan(scan_expr) => lower_scan(lowerer, scan_expr),
        ExprKind::Sort(sort) => lower_sort(lowerer, sort),
        // `⇇` copy-into lowers to the typed HIR node; typecheck owns the
        // tensor-storage, exact-type and alias rules.
        ExprKind::CopyInto(copy) => lowerer.lower_copy_into(copy),
        // `⇜` sink-into lowers to the typed HIR node; typecheck owns the
        // contract conformance, the exact item type, and the statement shape.
        ExprKind::SinkInto(sink) => lowerer.lower_sink_into(sink),
        ExprKind::Trap(trap) => lower_trap(lowerer, trap),
        ExprKind::CallDefault(call_default) => lower_call_default(lowerer, call_default),
        // D2.1: `value ¶ "spec"` keeps the spec literal's text; typecheck
        // parses and validates it against the value's type (D2.2).
        ExprKind::Format(format) => HirExpressionKind::Format {
            value: Box::new(lower_expr(lowerer, &format.value)),
            spec: format.spec,
        },
    };

    HirExpression {
        id,
        kind,
        ty: None,
        span,
    }
}

/// Lower the source shape of `reducta via <Op>` while preserving its declaration in HIR.
///
/// The node is intentionally not normalized to `Itera`: a reduction is an
/// expression with a term body, while `itera` remains a statement with its own
/// control-flow and mutation semantics. K3 owns the admission and lowering
/// rules after this source-shaped node lands.
/// Lower owned reduction scopes without desugaring away their identity.
fn lower_reducta(
    lowerer: &mut Lowerer,
    reduction: &radix_syntax::ReductaExpr,
) -> HirExpressionKind {
    let source = Box::new(lower_expr(lowerer, &reduction.source));
    let identity = reduction
        .identity
        .as_ref()
        .map(|e| Box::new(lower_expr(lowerer, e)));
    lowerer.push_scope();
    let coordinates = reduction
        .apud
        .iter()
        .map(|ident| {
            let id = lowerer.next_def_id();
            lowerer.bind_local(ident.name, id);
            (id, ident.name)
        })
        .collect();
    let group = reduction
        .group
        .as_ref()
        .map(|e| Box::new(lower_expr(lowerer, e)));
    let filum = reduction.filum.as_ref().map(|ident| {
        let id = lowerer.next_def_id();
        lowerer.bind_local(ident.name, id);
        (id, ident.name)
    });
    let binding = reduction.binding.as_ref().map(|ident| {
        let id = lowerer.next_def_id();
        lowerer.bind_local(ident.name, id);
        (id, ident.name)
    });
    let body = reduction
        .body
        .as_ref()
        .map(|body| lowerer.lower_block(body));
    lowerer.pop_scope();
    let op = match reduction.op {
        radix_syntax::ReductaOp::Sum => radix_hir::HirReductionOp::Sum,
        radix_syntax::ReductaOp::Product => radix_hir::HirReductionOp::Product,
        radix_syntax::ReductaOp::Max => radix_hir::HirReductionOp::Max,
        radix_syntax::ReductaOp::Min => radix_hir::HirReductionOp::Min,
        radix_syntax::ReductaOp::ArgMax => radix_hir::HirReductionOp::ArgMax,
        radix_syntax::ReductaOp::ArgMin => radix_hir::HirReductionOp::ArgMin,
        radix_syntax::ReductaOp::All => radix_hir::HirReductionOp::All,
        radix_syntax::ReductaOp::Any => radix_hir::HirReductionOp::Any,
        radix_syntax::ReductaOp::Count => radix_hir::HirReductionOp::Count,
        radix_syntax::ReductaOp::First => radix_hir::HirReductionOp::First,
    };
    HirExpressionKind::Reduction {
        op,
        source,
        coordinates,
        group,
        filum,
        binding,
        body,
        identity,
        mutable: reduction.mutability == radix_syntax::AstMutability::Mutable,
    }
}

fn lower_scan(lowerer: &mut Lowerer, scan: &radix_syntax::ScanExpr) -> HirExpressionKind {
    let source = Box::new(lower_expr(lowerer, &scan.source));
    lowerer.push_scope();
    let coordinates = scan
        .apud
        .iter()
        .map(|ident| {
            let id = lowerer.next_def_id();
            lowerer.bind_local(ident.name, id);
            (id, ident.name)
        })
        .collect();
    let group = scan
        .group
        .as_ref()
        .map(|expr| Box::new(lower_expr(lowerer, expr)));
    let filum_id = lowerer.next_def_id();
    lowerer.bind_local(scan.filum.name, filum_id);
    let binding_id = lowerer.next_def_id();
    lowerer.bind_local(scan.binding.name, binding_id);
    let body = lowerer.lower_block(&scan.body);
    lowerer.pop_scope();

    let op = match scan.op {
        radix_syntax::ReductaOp::Sum => radix_hir::HirReductionOp::Sum,
        radix_syntax::ReductaOp::Product => radix_hir::HirReductionOp::Product,
        radix_syntax::ReductaOp::Max => radix_hir::HirReductionOp::Max,
        radix_syntax::ReductaOp::Min => radix_hir::HirReductionOp::Min,
        radix_syntax::ReductaOp::All => radix_hir::HirReductionOp::All,
        radix_syntax::ReductaOp::Any => radix_hir::HirReductionOp::Any,
        radix_syntax::ReductaOp::Count => radix_hir::HirReductionOp::Count,
        radix_syntax::ReductaOp::ArgMax
        | radix_syntax::ReductaOp::ArgMin
        | radix_syntax::ReductaOp::First => {
            unreachable!("scan parser admits only value-prefix operators")
        }
    };
    let total = match scan.total {
        radix_syntax::ScanTotal::Inclusive => radix_hir::HirScanTotal::Inclusive,
        radix_syntax::ScanTotal::Exclusive => radix_hir::HirScanTotal::Exclusive,
    };
    let order = match scan.order {
        radix_syntax::SortDirection::Ascending => radix_hir::HirSortDirection::Ascending,
        radix_syntax::SortDirection::Descending => radix_hir::HirSortDirection::Descending,
    };
    HirExpressionKind::Scan {
        op,
        total,
        source,
        coordinates,
        group,
        group_domain: None,
        order,
        filum: (filum_id, scan.filum.name),
        binding: (binding_id, scan.binding.name),
        body,
    }
}

/// Lower the source shape of `ordinata via <Direction>` while preserving its
/// declaration in HIR.
///
/// The optional key body binds its element name in its own scope, as the
/// reduction body does. Typecheck owns the source and key rules.
fn lower_sort(lowerer: &mut Lowerer, sort: &radix_syntax::SortExpr) -> HirExpressionKind {
    let source = Box::new(lower_expr(lowerer, &sort.source));
    lowerer.push_scope();
    let binding = sort.binding.as_ref().map(|ident| {
        let id = lowerer.next_def_id();
        lowerer.bind_local(ident.name, id);
        (id, ident.name)
    });
    let body = sort.body.as_ref().map(|body| lowerer.lower_block(body));
    lowerer.pop_scope();
    HirExpressionKind::Sort {
        direction: match sort.direction {
            radix_syntax::SortDirection::Ascending => radix_hir::HirSortDirection::Ascending,
            radix_syntax::SortDirection::Descending => radix_hir::HirSortDirection::Descending,
        },
        source_mode: match sort.source_mode {
            radix_syntax::SortSourceMode::Values => radix_hir::HirSortSource::Values,
            radix_syntax::SortSourceMode::Keys => radix_hir::HirSortSource::Keys,
        },
        source,
        binding,
        body,
        mutable: sort.mutability == radix_syntax::AstMutability::Mutable,
    }
}

/// Lower the `capta { ... }` capture boundary (UT).
///
/// The body is an ordinary block with its own scope; its trailing expression
/// statement is lifted into the block's tail slot as the success value. No
/// `cape` clause attaches and there is no early-success exit (open row 1).
/// The boundary's result is the builtin genus `trapped<R, T>` (f9d8a611), so
/// lowering marks the genus applied — the injected HIR enum item then ships
/// with the module exactly as a written `trapped<...>` type position would.
fn lower_trap(lowerer: &mut Lowerer, trap: &radix_syntax::TrapExpr) -> HirExpressionKind {
    lowerer.current_span = trap.span;
    lowerer.resolver.mark_builtin_trapped_applied();
    let mut body = lowerer.lower_block(&trap.body);
    lift_trailing_expression_stmt(lowerer, &mut body);
    HirExpressionKind::Trap(Box::new(body))
}

/// Lower `call ⊥ default` (D17.7) through the trap machinery: the call
/// becomes the trailing expression of a trap body, and the default stays a
/// separate operand evaluated only on failure.
fn lower_call_default(
    lowerer: &mut Lowerer,
    call_default: &radix_syntax::CallDefaultExpr,
) -> HirExpressionKind {
    let call = lower_expr(lowerer, &call_default.call);
    let span = call.span;
    let body = radix_hir::HirBlock::simple(Vec::new(), Some(Box::new(call)), span);
    HirExpressionKind::CallDefault {
        body: Box::new(body),
        default: Box::new(lower_expr(lowerer, &call_default.default)),
        err_ty: None,
    }
}

/// Lower a literal
fn lower_literal(lowerer: &mut Lowerer, lit: &Literal) -> HirExpressionKind {
    match lit {
        Literal::Forma(template) => lowerer.lower_forma_capture(*template, Vec::new()),
        _ => match super::literal::literal_to_hir(lit, lowerer.interner) {
            Ok(literal) => HirExpressionKind::Literal(literal),
            Err(message) => {
                lowerer.error_from_message(message);
                HirExpressionKind::Error
            }
        },
    }
}

/// Lift a value-block's trailing expression statement into the block's
/// tail-expression slot.
///
/// Authored blocks lower with `expr: None` (block values are ordinarily
/// `redde`-driven), but some expression bodies are bare trailing expressions:
/// the `ubi` predicate (`ubi x { x < 0.0 }`, AFM-3) and the `capta` trap body
/// (its final expression is the success value, UT). The last expression
/// statement of the block becomes the block value; everything before it is
/// ordinary sequential preamble. The source statement's presentation owner
/// follows the expression into that slot; leaving it on the removed statement
/// would orphan its comments even though the value itself survives.
fn lift_trailing_expression_stmt(lowerer: &mut Lowerer, body: &mut radix_hir::HirBlock) {
    if body.expr.is_some() {
        return;
    }
    let Some(trailing) = body.statements.pop() else {
        return;
    };
    let expr = match trailing.kind {
        radix_hir::HirStatementKind::Expr(expr) => expr,
        other => {
            body.statements.push(radix_hir::HirStatement {
                id: trailing.id,
                span: trailing.span,
                kind: other,
            });
            return;
        }
    };
    let statement_owner = radix_hir::HirOwnerRef::Hir(trailing.id);
    if let Some(owner) = lowerer
        .presentation
        .owners
        .iter_mut()
        .rev()
        .find(|owner| owner.semantic.as_ref() == Some(&statement_owner))
    {
        owner.semantic = Some(radix_hir::HirOwnerRef::Hir(expr.id));
    }
    body.expr = Some(Box::new(expr));
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
        // Size-type Z1 force: a `magnitudo` param names a read-only int
        // value. Its DefId lowers as a path; MIR resolves it through
        // size_param_sources (sgr C2a) and converts to numerus.
        if let Some((def_id, radix_syntax::GenericParamKind::Magnitudo)) =
            self.lexical_type_param(ident.name)
        {
            return HirExpressionKind::Path(def_id);
        }
        if let Some(def_id) = self.resolver.lookup(ident.name)
            && self
                .resolver
                .get_symbol(def_id)
                .is_some_and(|symbol| symbol.kind == crate::SymbolKind::IndexParam)
        {
            return HirExpressionKind::Path(def_id);
        }
        match self.resolver.lookup_variant_bare(ident.name) {
            crate::BareVariantLookup::Single(def_id) => HirExpressionKind::Path(def_id),
            // Ambiguous and absent fail identically: the bare name does not
            // resolve to one variant path, and neither case may silently pick.
            crate::BareVariantLookup::Ambiguous { .. } | crate::BareVariantLookup::None => {
                self.current_span = ident.span;
                self.error("undefined_name");
                HirExpressionKind::Error
            }
        }
    }

    /// Intern the names the typed-zero `crea` rewrite needs.
    ///
    /// Typecheck may rewrite a bare `vacua` literal or an inferred-constructor
    /// hole into the typed-zero `crea` call (`empty_dense`) and cannot intern
    /// there, so the names it needs are interned here, where the interner is
    /// writable.
    fn intern_typed_zero_crea_names(&mut self) {
        self.intrinsic_surface_symbol("crea");
        self.interner.intern("crea");
    }

    /// Single-segment variant policy (uvf-u3): a resolver binding wins, then
    /// the per-union namespace resolves an unambiguous bare variant name. An
    /// ambiguous or unowned bare name yields nothing rather than a silent
    /// pick; the caller reports the miss.
    fn lookup_variant_bare_single(&self, name: radix_lexer::Symbol) -> Option<radix_hir::DefId> {
        self.resolver
            .lookup(name)
            .or_else(|| match self.resolver.lookup_variant_bare(name) {
                crate::BareVariantLookup::Single(def_id) => Some(def_id),
                crate::BareVariantLookup::Ambiguous { .. } | crate::BareVariantLookup::None => None,
            })
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
            radix_syntax::BinOp::TrueDiv => HirBinOp::TrueDiv,
            radix_syntax::BinOp::Mod => HirBinOp::Mod,
            radix_syntax::BinOp::Dot => HirBinOp::Dot,
            radix_syntax::BinOp::Cross => HirBinOp::Cross,
            radix_syntax::BinOp::TensorProduct => HirBinOp::TensorProduct,
            radix_syntax::BinOp::Hadamard => HirBinOp::Hadamard,
            radix_syntax::BinOp::HadamardDiv => HirBinOp::HadamardDiv,
            radix_syntax::BinOp::Eq => HirBinOp::Eq,
            radix_syntax::BinOp::NotEq => HirBinOp::NotEq,
            radix_syntax::BinOp::ApproxEq => HirBinOp::ApproxEq,
            radix_syntax::BinOp::ApproxNotEq => HirBinOp::ApproxNotEq,
            radix_syntax::BinOp::FuzzyEq => HirBinOp::FuzzyEq,
            radix_syntax::BinOp::FuzzyNotEq => HirBinOp::FuzzyNotEq,
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
            // WHY: placeholders; the checker retargets `Between` / `NotBetween` to the range forms by right-operand type.
            radix_syntax::BinOp::Between | radix_syntax::BinOp::Member => HirBinOp::Between,
            radix_syntax::BinOp::NotMember => HirBinOp::NotBetween,
            radix_syntax::BinOp::Maximum => {
                return self.lower_extremum_method_call("maximus", lhs, rhs);
            }
            radix_syntax::BinOp::Minimum => {
                return self.lower_extremum_method_call("minimus", lhs, rhs);
            }
        };

        HirExpressionKind::Binary(op, Box::new(lhs), Box::new(rhs))
    }

    /// Infix extrema are syntax sugar for the canonical method twins. Keep
    /// the HIR shape identical to an explicit `.maximus(n)` / `.minimus(n)`
    /// call so the existing typecheck, registry lookup, and MIR lowering
    /// paths remain the sole semantic owners.
    fn lower_extremum_method_call(
        &mut self,
        canonical: &str,
        lhs: HirExpression,
        rhs: HirExpression,
    ) -> HirExpressionKind {
        let surface = self
            .locale_pack
            .map_or(canonical, |pack| pack.intrinsic_surface(canonical));
        let name = self.interner.intern(surface);
        HirExpressionKind::MethodCall(
            Box::new(lhs),
            name,
            Vec::new(),
            vec![HirCallArg {
                name: None,
                spread: false,
                marker: None,
                span: rhs.span,
                expr: rhs,
            }],
            None,
        )
    }

    /// Lower unary expression (unarius)
    fn lower_unarius(&mut self, unary: &UnaryExpr, id: HirId) -> HirExpressionKind {
        let operand = lower_expr(self, &unary.operand);

        let op = match unary.op {
            radix_syntax::UnOp::Neg => HirUnOp::Neg,
            radix_syntax::UnOp::Not => HirUnOp::Not,
            radix_syntax::UnOp::BitNot => HirUnOp::BitNot,
            // The rounding delimiters are sugar for the canonical rounding
            // method twins (`pavimentum`, `tectum`, `rotunda`). The private
            // marker name lets typecheck reject a tensor operand by name and
            // then rewrite to the active pack's intrinsic surface.
            // The node id is recorded with the glyph ids so Faber re-emission
            // prints the authored delimiters instead of the method twin.
            radix_syntax::UnOp::Floor => {
                self.presentation.glyph_ids.push(id);
                return self.lower_rounding_glyph("pavimentum", operand);
            }
            radix_syntax::UnOp::Ceil => {
                self.presentation.glyph_ids.push(id);
                return self.lower_rounding_glyph("tectum", operand);
            }
            radix_syntax::UnOp::Nearest => {
                self.presentation.glyph_ids.push(id);
                return self.lower_rounding_glyph("rotunda", operand);
            }
            // The radical glyphs are sugar for the canonical method twins
            // (`radix`, `radix_cubica`); the ordinary intrinsic checker owns
            // legality, so an integer operand inherits the `.sqrt()` error.
            // The node id is recorded with the glyph ids so Faber re-emission
            // prints the authored glyph instead of the method twin.
            radix_syntax::UnOp::RadicalSqrt => {
                self.presentation.glyph_ids.push(id);
                return self.lower_radical_glyph("radix", operand);
            }
            radix_syntax::UnOp::RadicalCbrt => {
                self.presentation.glyph_ids.push(id);
                return self.lower_radical_glyph("radix_cubica", operand);
            }
            // `∜ x` is `√ √ x`: `.sqrt().sqrt()`. The outer id is recorded in
            // its own set: authored `∜ x` and authored `√ x.sqrt()` lower to
            // the same nested shape, and only the set distinguishes them.
            radix_syntax::UnOp::RadicalFourth => {
                self.presentation.radical_quarta_ids.push(id);
                let span = operand.span;
                let squared = HirExpression {
                    id: self.next_hir_id(),
                    kind: self.lower_radical_glyph("radix", operand),
                    ty: None,
                    span,
                };
                return self.lower_radical_glyph("radix", squared);
            }
        };

        HirExpressionKind::Unary(op, Box::new(operand))
    }

    /// Lower a rounding glyph to a marker `MethodCall` whose name carries the
    /// canonical twin (`__faber_glyph_round:<canonical>`); typecheck rewrites it.
    fn lower_rounding_glyph(
        &mut self,
        canonical: &str,
        operand: HirExpression,
    ) -> HirExpressionKind {
        // Intern the pack surface so typecheck's lookup finds it.
        self.intrinsic_surface_symbol(canonical);
        let marker = self
            .interner
            .intern(&format!("{ROUNDING_GLYPH_MARKER}{canonical}"));
        HirExpressionKind::MethodCall(Box::new(operand), marker, Vec::new(), Vec::new(), None)
    }

    /// Lower a radical glyph to its canonical method twin: `√ x` is `x.sqrt()`,
    /// `∛ x` is `x.cbrt()`, and `∜ x` is `x.sqrt().sqrt()`. The HIR shape is the
    /// authored method call, so typecheck and codegen stay the sole owners of
    /// legality (an integer operand is the `.sqrt()` error).
    fn lower_radical_glyph(
        &mut self,
        canonical: &str,
        operand: HirExpression,
    ) -> HirExpressionKind {
        let name = self.intrinsic_surface_symbol(canonical);
        HirExpressionKind::MethodCall(Box::new(operand), name, Vec::new(), Vec::new(), None)
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
    ///
    /// `id` is the call expression's own `HirId`; a glyph transpose (`xᵀ`,
    /// parsed as a call of the private marker method) records it as the
    /// presentation anchor so Faber re-emission restores the glyph.
    fn lower_vocare(
        &mut self,
        call: &radix_syntax::CallExpr,
        id: radix_hir::HirId,
    ) -> HirExpressionKind {
        let type_args: Vec<TypeId> = call
            .type_args
            .iter()
            .map(|arg| lower_call_generic_arg(self, arg))
            .collect();
        match &call.callee.kind {
            ExprKind::Literal(Literal::String(template)) => {
                if self.reject_template_call_type_args(&type_args) {
                    return HirExpressionKind::Error;
                }
                match self.lower_template_application(*template, &call.args) {
                    Some((erased, args, authored)) => {
                        self.record_template_holes(id, authored);
                        HirExpressionKind::Scriptum(erased, args)
                    }
                    None => HirExpressionKind::Error,
                }
            }
            ExprKind::Literal(Literal::Forma(template)) => {
                if self.reject_template_call_type_args(&type_args) {
                    return HirExpressionKind::Error;
                }
                match self.lower_template_application(*template, &call.args) {
                    Some((erased, args, authored)) => {
                        self.record_template_holes(id, authored);
                        self.lower_forma_capture(erased, args)
                    }
                    None => HirExpressionKind::Error,
                }
            }
            // `base!.method(args)` is a method call on the asserted payload:
            // the assertion becomes the receiver, so every later phase sees
            // the same `MethodCall` shape as a call on a plain binding.
            ExprKind::NonNull(radix_syntax::NonNullExpr {
                object,
                chain: radix_syntax::NonNullKind::Member(method),
            }) => {
                let base = lower_expr(self, object);
                let span = base.span;
                let recv = HirExpression {
                    id: self.next_hir_id(),
                    kind: HirExpressionKind::NonNull(Box::new(base), HirNonNullKind::Value),
                    ty: None,
                    span,
                };
                let args = self.lower_call_args(&call.args);
                HirExpressionKind::MethodCall(Box::new(recv), method.name, type_args, args, None)
            }
            ExprKind::Member(member) => {
                let (recv, name) = self.flatten_dotted_method_callee(member);
                let name = self.canonicalize_namespace_member(&recv, name);
                if self.interner.resolve(name) == GLYPH_TRANSPOSE_MARKER {
                    self.presentation.glyph_ids.push(id);
                }
                let args = self.lower_call_args(&call.args);
                HirExpressionKind::MethodCall(Box::new(recv), name, type_args, args, None)
            }
            _ => {
                let callee = lower_expr(self, &call.callee);
                let args = self.lower_call_args(&call.args);
                HirExpressionKind::Call(Box::new(callee), type_args, args, None)
            }
        }
    }

    /// Type-expression constructor (size-type Z2): the claimed tensor type
    /// mints a zero-filled value. Synthesizes the same HIR shape as the
    /// corpus surface `seed.create(0.0, [extents])` — a `create` method call
    /// on a vacua receiver pre-typed with the claimed type — so typing, MIR
    /// lowering, and the runtime create path all reuse the existing
    /// machinery. The receiver's pre-lowered `ty` is what
    /// `check_method_call` reads instead of re-checking the vacua literal.
    fn lower_construct(
        &mut self,
        ty: &radix_syntax::TypeExpr,
        owner: HirId,
        span: radix_lexer::Span,
    ) -> HirExpressionKind {
        let claimed = self.lower_authored_type(
            radix_hir::HirTypeSite {
                owner: radix_hir::HirOwnerRef::Hir(owner),
                slot: radix_hir::HirTypeSlot::TypedZero,
            },
            ty,
            false,
            false,
        );
        match &ty.kind {
            radix_syntax::TypeExprKind::Tensor(..)
            | radix_syntax::TypeExprKind::Vector(..)
            | radix_syntax::TypeExprKind::Matrix(..) => self.construct_zero(ty, claimed, span).kind,
            _ => {
                self.error_with_args_at(
                    vec![DiagnosticArg::new(
                        "issue",
                        "constructor_requires_extent_type",
                    )],
                    span,
                );
                HirExpressionKind::Error
            }
        }
    }

    /// The zero value of a type expression: a scalar zero, a zero vector or
    /// matrix (a literal array of zeros converted to the claimed type), or a
    /// tensor created on the zero of its element.
    fn construct_zero(
        &mut self,
        ty: &radix_syntax::TypeExpr,
        claimed: radix_types::TypeId,
        span: radix_lexer::Span,
    ) -> HirExpression {
        let hir = |this: &mut Self, kind| HirExpression {
            id: this.next_hir_id(),
            kind,
            ty: None,
            span,
        };
        match (&ty.kind, self.types.get(claimed).clone()) {
            (radix_syntax::TypeExprKind::Vector(_, width), crate::Type::Vector(element, _)) => {
                // The short sugar `vf32[4]` parses its width as a one-element
                // bracket shape; the long form `vector<f32, size 4>` as the bare
                // literal. Both spell the same width.
                let width = match width {
                    radix_syntax::FiguraExpr::Tuple(dims) if dims.len() == 1 => &dims[0],
                    other => other,
                };
                let lanes = self.construct_literal_extent(width, span);
                let zero = self.construct_scalar_zero(element, span);
                let array = HirExpressionKind::Array(
                    (0..lanes)
                        .map(|_| HirArrayElement::Expr(zero.clone()))
                        .collect(),
                );
                let source = hir(self, array);
                hir(
                    self,
                    HirExpressionKind::Verte {
                        source: Box::new(source),
                        target: claimed,
                        entries: None,
                    },
                )
            }
            (radix_syntax::TypeExprKind::Matrix(_, shape), crate::Type::Matrix(element, _)) => {
                let (rows, cols) = match shape {
                    radix_syntax::FiguraExpr::Tuple(dims) if dims.len() == 2 => (
                        self.construct_literal_extent(&dims[0], span),
                        self.construct_literal_extent(&dims[1], span),
                    ),
                    _ => (0, 0),
                };
                let zero = self.construct_scalar_zero(element, span);
                let row = HirExpressionKind::Array(
                    (0..cols)
                        .map(|_| HirArrayElement::Expr(zero.clone()))
                        .collect(),
                );
                let mut row_exprs = Vec::new();
                for _ in 0..rows {
                    row_exprs.push(HirArrayElement::Expr(hir(self, row.clone())));
                }
                let source = hir(self, HirExpressionKind::Array(row_exprs));
                hir(
                    self,
                    HirExpressionKind::Verte {
                        source: Box::new(source),
                        target: claimed,
                        entries: None,
                    },
                )
            }
            (
                radix_syntax::TypeExprKind::Tensor(element_ty, figura),
                crate::Type::Tensor(element, _),
            ) => {
                let fill = self.construct_zero(element_ty, element, span);
                let dims: &[radix_syntax::FiguraExpr] = match figura {
                    radix_syntax::FiguraExpr::Tuple(dims) => dims,
                    single => std::slice::from_ref(single),
                };
                let extents = dims
                    .iter()
                    .map(|dim| self.construct_extent_expr(dim, span))
                    .collect::<Vec<_>>();
                let extents_array = hir(
                    self,
                    HirExpressionKind::Array(
                        extents.into_iter().map(HirArrayElement::Expr).collect(),
                    ),
                );
                let receiver = HirExpression {
                    id: self.next_hir_id(),
                    kind: HirExpressionKind::Vacua,
                    ty: Some(claimed),
                    span,
                };
                let kind = HirExpressionKind::MethodCall(
                    Box::new(receiver),
                    self.intrinsic_surface_symbol("crea"),
                    Vec::new(),
                    vec![
                        HirCallArg {
                            name: None,
                            spread: false,
                            marker: None,
                            expr: fill,
                            span,
                        },
                        HirCallArg {
                            name: None,
                            spread: false,
                            marker: None,
                            expr: extents_array,
                            span,
                        },
                    ],
                    None,
                );
                hir(self, kind)
            }
            _ => self.construct_scalar_zero(claimed, span),
        }
    }

    fn construct_scalar_zero(
        &mut self,
        element: radix_types::TypeId,
        span: radix_lexer::Span,
    ) -> HirExpression {
        let kind = if radix_types::numeric_width::is_tensor_float_element(self.types, element) {
            HirExpressionKind::Literal(HirLiteral::Float(0.0))
        } else {
            HirExpressionKind::Literal(HirLiteral::Int(0))
        };
        HirExpression {
            id: self.next_hir_id(),
            kind,
            ty: None,
            span,
        }
    }

    /// A vector width or matrix extent that must be a literal: the zero is
    /// materialised as an array literal of that many zeros.
    fn construct_literal_extent(
        &mut self,
        dim: &radix_syntax::FiguraExpr,
        span: radix_lexer::Span,
    ) -> usize {
        match dim {
            radix_syntax::FiguraExpr::Literal(value) => *value as usize,
            radix_syntax::FiguraExpr::Lift(extent) => match &**extent {
                radix_syntax::LiftExtent::Literal(value) => *value as usize,
                radix_syntax::LiftExtent::Paren(term) => self.construct_literal_extent(term, span),
                _ => self.construct_literal_extent_refusal(span),
            },
            _ => self.construct_literal_extent_refusal(span),
        }
    }

    fn construct_literal_extent_refusal(&mut self, span: radix_lexer::Span) -> usize {
        self.error_with_args_at(
            vec![DiagnosticArg::new(
                "issue",
                "constructor_register_extent_requires_literal",
            )],
            span,
        );
        0
    }

    /// One constructor extent term as a value expression. Extents admit
    /// literals, size names, and spelled size lifts; a `_` hole belongs in
    /// the annotation, and affine extent terms are a named residual.
    fn construct_extent_expr(
        &mut self,
        dim: &radix_syntax::FiguraExpr,
        span: radix_lexer::Span,
    ) -> HirExpression {
        let kind = match dim {
            radix_syntax::FiguraExpr::Literal(value) => {
                HirExpressionKind::Literal(HirLiteral::Int(*value))
            }
            radix_syntax::FiguraExpr::Var(ident) => self.lower_nomen(ident),
            radix_syntax::FiguraExpr::Lift(extent) => match &**extent {
                radix_syntax::LiftExtent::Literal(value) => {
                    HirExpressionKind::Literal(HirLiteral::Int(*value))
                }
                radix_syntax::LiftExtent::Name(ident) => self.lower_nomen(ident),
                radix_syntax::LiftExtent::Negative { value, .. } => {
                    self.error_with_args_at(
                        vec![
                            DiagnosticArg::new("issue", "size_lift_negative"),
                            DiagnosticArg::new("value", format!("-{value}")),
                        ],
                        span,
                    );
                    HirExpressionKind::Error
                }
                radix_syntax::LiftExtent::Paren(term) => {
                    return self.construct_extent_expr(term, span);
                }
                radix_syntax::LiftExtent::Shape(_) => {
                    self.error_with_args_at(
                        vec![DiagnosticArg::new(
                            "issue",
                            "constructor_affine_extent_unsupported",
                        )],
                        span,
                    );
                    HirExpressionKind::Error
                }
            },
            radix_syntax::FiguraExpr::Infer => {
                self.error_with_args_at(
                    vec![DiagnosticArg::new(
                        "issue",
                        "constructor_extent_requires_term",
                    )],
                    span,
                );
                HirExpressionKind::Error
            }
            radix_syntax::FiguraExpr::Add(_, _)
            | radix_syntax::FiguraExpr::Subtract(_, _)
            | radix_syntax::FiguraExpr::Divide(_, _)
            | radix_syntax::FiguraExpr::Tuple(_) => {
                self.error_with_args_at(
                    vec![DiagnosticArg::new(
                        "issue",
                        "constructor_affine_extent_unsupported",
                    )],
                    span,
                );
                HirExpressionKind::Error
            }
        };
        HirExpression {
            id: self.next_hir_id(),
            kind,
            ty: None,
            span,
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
                    marker: if arg.axis {
                        Some(radix_hir::HirArgMarker::Axis)
                    } else {
                        arg.marker.map(|mode| match mode {
                            radix_syntax::TypeMode::De => radix_hir::HirArgMarker::Ref,
                            radix_syntax::TypeMode::In => radix_hir::HirArgMarker::Mut,
                            radix_syntax::TypeMode::Own => radix_hir::HirArgMarker::Own,
                            radix_syntax::TypeMode::Copy => radix_hir::HirArgMarker::Copy,
                        })
                    },
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
    ) -> Option<(radix_lexer::Symbol, Vec<HirExpression>, TemplateAuthored)> {
        let call_span = self.current_span;
        let text = self.interner.resolve(template).to_owned();
        let analysis = analyze_template(&text);
        let lowered: Vec<(Option<radix_lexer::Symbol>, HirExpression)> = args
            .iter()
            .map(|arg| (arg.label, lower_expr(self, &arg.value)))
            .collect();

        if let Some(reads) = analysis.plain_numbered_mix() {
            self.push_error(
                SemanticErrorKind::Warning(WarningKind::MixedTemplateHoles),
                vec![
                    DiagnosticArg::new("issue", "mixed_plain_numbered_template_holes"),
                    DiagnosticArg::new("reads", reads),
                ],
                call_span,
            );
        }

        let mut ok = true;
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
            let labeled_actual_names: Vec<String> = lowered
                .iter()
                .filter_map(|(label, _)| {
                    label.map(|symbol| self.interner.resolve(symbol).to_owned())
                })
                .collect();
            let filled_labels: FxHashSet<&str> =
                labeled_actual_names.iter().map(String::as_str).collect();
            for name in &labeled_actual_names {
                if analysis.label_position(name).is_none() {
                    self.error_with_args_at(
                        vec![
                            DiagnosticArg::new("issue", "unknown_template_hole_label"),
                            DiagnosticArg::new("label", name.clone()),
                        ],
                        call_span,
                    );
                    ok = false;
                }
            }
            for name in analysis.label_positions.keys() {
                let filled = filled_labels.contains(name.as_str());
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

        // A numbered hole `§N` addresses argument N, so repeated numbers share
        // one value. The template needs `max + 1` actuals, each position must be
        // read, and a supplied value must not go unread. A gap in the numbering
        // is not an arity defect, even when the argument count happens to
        // match: name the first unread position instead of the count.
        let (required_slots, distinct_slots) = analysis.value_slots();
        if distinct_slots < required_slots
            && let Some(index) = analysis.first_unread_slot()
        {
            self.error_with_args_at(
                vec![
                    DiagnosticArg::new("issue", "template_hole_index_gap"),
                    DiagnosticArg::new("index", index.to_string()),
                    DiagnosticArg::new("actual", lowered.len().to_string()),
                ],
                call_span,
            );
            ok = false;
        } else if lowered.len() != required_slots {
            self.error_with_args_at(
                vec![
                    DiagnosticArg::new("issue", "template_application_arity_mismatch"),
                    DiagnosticArg::new("actual", lowered.len().to_string()),
                    DiagnosticArg::new("expected", required_slots.to_string()),
                ],
                call_span,
            );
            ok = false;
        }

        if !ok {
            return None;
        }

        // Template text is a string payload: keep source bytes (no NFKC).
        let erased = self.interner.intern_raw(&analysis.erased);
        // Authored form for re-emission: only a template with named holes
        // differs from its erased text, and labeled actuals are listed in
        // authored order with the positional slot each one fills.
        let labeled_actuals: Vec<(radix_lexer::Symbol, u32)> = lowered
            .iter()
            .filter_map(|(label, _)| {
                let label = (*label)?;
                let position = analysis.label_position(self.interner.resolve(label))?;
                Some((label, u32::try_from(position).ok()?))
            })
            .collect();
        let authored = (analysis.erased != text).then_some((template, labeled_actuals));
        let positional = if has_labeled {
            self.reorder_labeled_actuals(&analysis, lowered)?
        } else {
            lowered.into_iter().map(|(_, expr)| expr).collect()
        };
        Some((erased, positional, authored))
    }

    /// Record the authored template text and actual labels of the application
    /// `id` when its holes were named, so Faber re-emission restores them.
    fn record_template_holes(&mut self, id: radix_hir::HirId, authored: TemplateAuthored) {
        if let Some((template, labeled_actuals)) = authored {
            self.presentation
                .template_holes
                .push(radix_hir::HirTemplateHoles {
                    expr: id,
                    template,
                    labeled_actuals,
                });
        }
    }

    /// Place labeled actuals into the positional vector the backends already consume.
    fn reorder_labeled_actuals(
        &self,
        analysis: &TemplateHoleAnalysis,
        lowered: Vec<(Option<radix_lexer::Symbol>, HirExpression)>,
    ) -> Option<Vec<HirExpression>> {
        let slot_count = analysis
            .holes
            .iter()
            .map(|hole| hole.position)
            .max()
            .map_or(0, |position| position.saturating_add(1));
        let mut slots: Vec<Option<HirExpression>> = (0..slot_count).map(|_| None).collect();
        for (label, expr) in lowered {
            let name = self.interner.resolve(label?);
            let position = analysis.label_position(name)?;
            slots[position] = Some(expr);
        }
        slots.into_iter().collect()
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
        if let radix_syntax::ExprKind::Ident(base) = &object.kind
            && let Some(module) = segment_names.first().copied()
        {
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
    /// member expression. `id` is the member expression's own `HirId`, which a
    /// by-label tuple read records as its presentation anchor.
    fn lower_membrum(
        &mut self,
        member: &radix_syntax::MemberExpr,
        id: radix_hir::HirId,
    ) -> HirExpressionKind {
        // WHY: postfix member chains are parsed iteratively and may be much
        // deeper than the native stack can safely lower recursively. Flatten
        // only this left spine, then rebuild HIR from the base outward.
        let mut chain = Vec::new();
        let mut cursor = member;
        loop {
            chain.push(cursor);
            let radix_syntax::ExprKind::Member(previous) = &cursor.object.kind else {
                break;
            };
            cursor = previous;
        }
        let mut object = lower_expr(self, &cursor.object);
        for (index, item) in chain.iter().rev().enumerate() {
            let item_id = if index + 1 == chain.len() {
                id
            } else {
                self.next_hir_id()
            };
            let span = object.span.merge(item.member.span);
            let kind = self.lower_member_from_object(item, item_id, object);
            object = radix_hir::HirExpression {
                id: item_id,
                kind,
                ty: None,
                span,
            };
        }
        object.kind
    }

    fn lower_member_from_object(
        &mut self,
        member: &radix_syntax::MemberExpr,
        id: radix_hir::HirId,
        object: radix_hir::HirExpression,
    ) -> HirExpressionKind {
        if let radix_syntax::ExprKind::Ident(base) = &member.object.kind
            && let Some(base_def) = self.lookup_name(base.name)
            && let Some(base_symbol) = self.resolver.get_symbol(base_def)
            && base_symbol.kind == crate::SymbolKind::Enum
        {
            if let Some(variant_def) = self.resolver.variant_child(base_def, member.member.name) {
                self.record_variant_path(variant_def, &[base.name, member.member.name]);
                return HirExpressionKind::Path(variant_def);
            }
            self.error("unknown_variant");
            return HirExpressionKind::Error;
        }
        // Imported namespace variant values (`module.Type.VARIANT`, uvf-u4):
        // the object is a two-segment path whose penultimate segment names a
        // type export of the module. When the terminal segment is one of that
        // type's variants, lower the whole path to the variant definition —
        // mirroring the same-module enum shorthand above and the qualified
        // `finge`/pattern-path resolution. The intermediate `module.Type` has
        // no expression role of its own; only the full variant path is a
        // first-class value reference.
        if let radix_syntax::ExprKind::Member(obj) = &member.object.kind
            && let radix_syntax::ExprKind::Ident(base) = &obj.object.kind
            && let Some(base_def) = self.lookup_name(base.name)
            && let Some(base_symbol) = self.resolver.get_symbol(base_def)
            && base_symbol.kind == crate::SymbolKind::Module
        {
            if let Some((variant_def, _)) = self.resolver.namespace_file_type_variant(
                base.name,
                obj.member.name,
                member.member.name,
            ) {
                self.record_variant_path(
                    variant_def,
                    &[base.name, obj.member.name, member.member.name],
                );
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
                && self.resolver.imported_nominal_has_variants(type_def)
            {
                self.error("unknown_variant");
                return HirExpressionKind::Error;
            }
        }

        // `module.Tab.Z` where `Tab` is an imported genus: the object `module.Tab`
        // has no expression role of its own, so the read lowers against the
        // imported genus's definition exactly like the same-module `Tab.Z`;
        // typecheck decides from the field's modifier whether `Z` is static.
        if let radix_syntax::ExprKind::Member(obj) = &member.object.kind
            && let radix_syntax::ExprKind::Ident(base) = &obj.object.kind
            && let Some(base_def) = self.lookup_name(base.name)
            && self
                .resolver
                .get_symbol(base_def)
                .is_some_and(|symbol| symbol.kind == crate::SymbolKind::Module)
            && let Some(type_def) = self
                .resolver
                .namespace_file_struct_def(base.name, obj.member.name)
        {
            let object = radix_hir::HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Path(type_def),
                ty: None,
                span: member.object.span,
            };
            return HirExpressionKind::Field(Box::new(object), member.member.name);
        }

        let field = self.canonicalize_namespace_member(&object, member.member.name);
        if let Some(kind) = self.lower_tuple_label_member(&object, field, member.member.span, id) {
            return kind;
        }

        HirExpressionKind::Field(Box::new(object), field)
    }

    /// Rewrite `p.gx` on a labeled `iuncta` binding to the same constant-index
    /// node as `p[0]`. Unknown labels emit `unknown_tuple_element_label`.
    ///
    /// The authored label is identity-erased by the rewrite, so it is recorded
    /// against the rewritten node's `HirId`; emission re-sugars exactly those
    /// ids back to `.gx`.
    fn lower_tuple_label_member(
        &mut self,
        object: &radix_hir::HirExpression,
        label: radix_lexer::Symbol,
        member_span: radix_lexer::Span,
        id: radix_hir::HirId,
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
            self.presentation
                .tuple_label_accesses
                .push(radix_hir::HirTupleLabelAccess { expr_id: id, label });
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

    /// Lower the `⇇` copy-into: `target ⇇ value`.
    fn lower_copy_into(&mut self, copy: &radix_syntax::CopyIntoExpr) -> HirExpressionKind {
        let target = lower_expr(self, &copy.target);
        let value = lower_expr(self, &copy.value);
        HirExpressionKind::CopyInto {
            target: Box::new(target),
            value: Box::new(value),
        }
    }

    /// `sink ⇜ item`: both operands lower normally; the statement's typing and
    /// access rules live in typecheck and the statement passes.
    fn lower_sink_into(&mut self, sink: &radix_syntax::SinkIntoExpr) -> HirExpressionKind {
        let target = lower_expr(self, &sink.sink);
        let item = lower_expr(self, &sink.item);
        HirExpressionKind::SinkInto {
            sink: Box::new(target),
            item: Box::new(item),
        }
    }

    fn lower_assign(&mut self, assign: &radix_syntax::AssignExpr) -> HirExpressionKind {
        let target = lower_expr(self, &assign.target);
        let mut source = lower_expr(self, &assign.value);

        match assign.op {
            radix_syntax::AssignOp::Assign => {
                if assign.value_marker == Some(radix_syntax::TypeMode::Copy) {
                    let span = source.span;
                    let id = self.next_hir_id();
                    let ty = None;
                    source = radix_hir::HirExpression {
                        id,
                        kind: HirExpressionKind::Marked {
                            marker: radix_hir::HirArgMarker::Copy,
                            expr: Box::new(source),
                        },
                        ty,
                        span,
                    };
                }
                HirExpressionKind::Assign(Box::new(target), Box::new(source))
            }
            // `↤` (U+21A4) is conversion-directed assignment: the operator is
            // preserved through HIR (never erased into `Assign`) until the
            // destination type resolves in typecheck, and the `⊥` inline
            // recovery travels with the node. The right side must go through
            // the same conversion registry as an explicit `↦ TargetType`.
            radix_syntax::AssignOp::ConversioAssign => {
                let recovery = assign
                    .recovery
                    .as_ref()
                    .map(|fallback| Box::new(lower_expr(self, fallback)));
                // The cell type is not known until typecheck, so only the
                // hint's name is validated here; whether the pairing takes a
                // hint is the checker's rule (`check_conversio`).
                let mut params = Vec::new();
                if let Some(via) = &assign.via {
                    let hint = self.interner.resolve(via.name).to_owned();
                    let issue = if hint == "Bits" {
                        Some("conversio_via_bits_retired")
                    } else if crate::numeric_width::is_float_rounding_hint(&hint)
                        || crate::numeric_width::is_conversio_radix_hint(&hint)
                    {
                        None
                    } else {
                        Some("conversio_via_unknown_hint")
                    };
                    self.apply_via_hint_verdict(issue, hint, via, &mut params);
                }
                HirExpressionKind::ConversioAssign {
                    target: Box::new(target),
                    source: Box::new(source),
                    recovery,
                    params,
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
    fn lower_verte(&mut self, verte: &radix_syntax::VerteExpr, owner: HirId) -> HirExpressionKind {
        if matches!(verte.ty.kind, radix_syntax::TypeExprKind::Infer) {
            self.intern_typed_zero_crea_names();
        }
        let previous_bare_generic_construction = self.allow_bare_generic_construction;
        self.allow_bare_generic_construction = matches!(
            &verte.expr.kind,
            radix_syntax::ExprKind::Object(_) | radix_syntax::ExprKind::Finge(_)
        );
        let target = self.lower_authored_type(
            radix_hir::HirTypeSite {
                owner: radix_hir::HirOwnerRef::Hir(owner),
                slot: radix_hir::HirTypeSlot::AscriptionTarget,
            },
            &verte.ty,
            false,
            false,
        );
        self.allow_bare_generic_construction = previous_bare_generic_construction;
        // Operator ruling 2026-10-14: a bare `{ field = value }` literal leaves
        // its type slot out, so its inferred target is a hole WITH a default —
        // the field-shaped record. The stated `record<*>` head is the same
        // whole-field-list hole and solves from the shape the same way; an
        // explicit `_ { … }` hole (form `LONG`) is unmarked and keeps failing
        // closed without an expected type.
        let record_defaults_from_shape =
            matches!(verte.ty.kind, radix_syntax::TypeExprKind::FieldListHole)
                || (matches!(verte.ty.kind, radix_syntax::TypeExprKind::Infer)
                    && verte.ty.form == radix_syntax::TypeFormLevel::BARE_LITERAL);
        if record_defaults_from_shape && let crate::Type::Infer(var) = self.types.get(target) {
            let var = *var;
            self.types.mark_default_record_hole(var);
        }
        // WHY: `{} ∷ Genus` is the same value as `Genus {}` (constructor =
        // type + shape; the bare literal takes the ascribed cell type), so the
        // empty bare JSON object literal ascribed to a genus lowers as a
        // construction with zero entries rather than a JSON source.
        let empty_json_object_for_genus = matches!(
            &verte.expr.kind,
            radix_syntax::ExprKind::Literal(radix_syntax::Literal::JsonValor(
                radix_syntax::JsonValue::Object(members)
            )) if members.is_empty()
        ) && self.ascription_target_is_genus(target);
        if empty_json_object_for_genus {
            let placeholder = HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Tuple(Vec::new(), None),
                ty: None,
                span: self.current_span,
            };
            HirExpressionKind::Verte {
                source: Box::new(placeholder),
                target,
                entries: Some(Vec::new()),
            }
        } else if let Some(object) = match &verte.expr.kind {
            radix_syntax::ExprKind::Object(object) => Some(object),
            // WHY: `{ f = v } ∷ Genus` is `Genus { f = v }` (constructor =
            // type + shape), so the ascribed field literal takes the genus
            // instead of nesting a hole construction under an ascription
            // whose own entries would be missing in MIR.
            _ => Self::ascribed_field_hole(&verte.expr)
                .filter(|_| self.ascription_target_is_genus(target)),
        } {
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

    /// The field list of a bare field literal `{ f = v }` or `_ { f = v }`:
    /// the parser's hole constructor, an `Infer`-typed `Verte` over an object.
    fn ascribed_field_hole(expr: &radix_syntax::Expr) -> Option<&radix_syntax::ObjectExpr> {
        let radix_syntax::ExprKind::Verte(hole) = &expr.kind else {
            return None;
        };
        if !matches!(hole.ty.kind, radix_syntax::TypeExprKind::Infer) {
            return None;
        }
        match &hole.expr.kind {
            radix_syntax::ExprKind::Object(object) => Some(object),
            _ => None,
        }
    }

    /// Whether an ascription target resolved to a genus (bare or applied).
    fn ascription_target_is_genus(&self, target: TypeId) -> bool {
        match self.types.get(target) {
            crate::Type::Struct(_) => true,
            crate::Type::Applied(base, _) => {
                matches!(self.types.get(*base), crate::Type::Struct(_))
            }
            _ => false,
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
                radix_syntax::ArrayElement::Infer(span) => {
                    elements.push(HirArrayElement::Infer(*span));
                }
            }
        }

        HirExpressionKind::Array(elements)
    }

    /// Lower iuncta tuple construction: `iuncta<A, B> [a, b]`.
    ///
    /// Type arguments from the source syntax are lowered and preserved so
    /// typecheck can use them when no contextual expected tuple is available.
    fn lower_iuncta(
        &mut self,
        iuncta: &radix_syntax::IunctaExpr,
        owner: HirId,
    ) -> HirExpressionKind {
        let mut elements = Vec::new();
        for expr in &iuncta.elements {
            elements.push(lower_expr(self, expr));
        }
        let type_args: Vec<TypeId> = iuncta
            .type_args
            .iter()
            .enumerate()
            .map(|(ordinal, ta)| {
                self.lower_authored_type(
                    radix_hir::HirTypeSite {
                        owner: radix_hir::HirOwnerRef::Hir(owner),
                        slot: radix_hir::HirTypeSlot::TupleTypeArgument(ordinal as u32),
                    },
                    ta,
                    false,
                    false,
                )
            })
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
    fn lower_finge(&mut self, finge: &radix_syntax::FingeExpr, owner: HirId) -> HirExpressionKind {
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
            let symbols = variant_path_symbols(&finge.variant);
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
                let symbols = variant_path_symbols(&finge.variant);
                match self.resolver.qualified_variant_path(&symbols) {
                    crate::QualifiedVariantPathLookup::Variant(def_id) => Some(def_id),
                    crate::QualifiedVariantPathLookup::UnknownVariant => None,
                    crate::QualifiedVariantPathLookup::NotQualified => {
                        self.lookup_variant_bare_single(last.name)
                    }
                }
            }
        } else {
            self.lookup_variant_bare_single(last.name)
        };
        let Some(variant_def_id) = resolved else {
            self.current_span = last.span;
            self.error("unknown_finge_variant");
            return HirExpressionKind::Error;
        };
        if finge.variant.len() >= 2 {
            let path = variant_path_symbols(&finge.variant);
            self.record_variant_path(variant_def_id, &path);
        }

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
                marker: None,
                expr: lower_expr(self, &field.value),
                span: field.span,
            })
            .collect();

        let call = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Call(Box::new(callee), Vec::new(), args, None),
            ty: None,
            span: self.current_span,
        };

        if let Some(cast) = &finge.cast {
            // The cast is a full type annotation; lower it through the normal
            // type path so a qualified name (`∷ pkg.Exitus`) resolves the
            // imported union identity. Diagnostics for a non-type or unknown
            // cast surface as the standard type-lowering errors.
            let previous_bare_generic_construction = self.allow_bare_generic_construction;
            self.allow_bare_generic_construction = true;
            let cast_ty = self.lower_authored_type(
                radix_hir::HirTypeSite {
                    owner: radix_hir::HirOwnerRef::Hir(owner),
                    slot: radix_hir::HirTypeSlot::ConversionTarget,
                },
                cast,
                false,
                false,
            );
            self.allow_bare_generic_construction = previous_bare_generic_construction;
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
    fn lower_clausura(
        &mut self,
        closure: &radix_syntax::ClausuraExpr,
        owner: HirId,
    ) -> HirExpressionKind {
        let mut params = Vec::new();
        self.push_scope();
        for (ordinal, param) in closure.params.iter().enumerate() {
            let def_id = self.next_def_id();
            let ty = self.lower_authored_type(
                radix_hir::HirTypeSite {
                    owner: radix_hir::HirOwnerRef::Def(def_id),
                    slot: radix_hir::HirTypeSlot::ClosureParameter(ordinal as u32),
                },
                &param.ty,
                true,
                false,
            );
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

        let ret = closure.ret.as_ref().map(|ret| {
            self.lower_authored_type(
                radix_hir::HirTypeSite {
                    owner: radix_hir::HirOwnerRef::Hir(owner),
                    slot: radix_hir::HirTypeSlot::Return,
                },
                ret,
                false,
                true,
            )
        });
        let err = closure.err.as_ref().map(|err| {
            self.lower_authored_type(
                radix_hir::HirTypeSite {
                    owner: radix_hir::HirOwnerRef::Hir(owner),
                    slot: radix_hir::HirTypeSlot::Error,
                },
                err,
                false,
                true,
            )
        });
        HirExpressionKind::Clausura(params, ret, err, Box::new(body))
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
        if let Some(via) = &conversio.via {
            self.lower_via_hint(via, &target, &mut params);
        }
        let recovery = conversio
            .recovery
            .as_ref()
            .map(|fb| Box::new(lower_expr(self, fb)));
        HirExpressionKind::Conversio {
            route: None,
            source: Box::new(source),
            target,
            params,
            recovery,
            implicit: None,
        }
    }

    /// Fold a `via HINT` clause (D11.9) into the conversion's hint slot.
    ///
    /// The clause lands in `params` and routes through the conversion rows
    /// keyed by that hint symbol. A clause carrying an unknown hint or sitting
    /// on a target that takes none is rejected (SEM016) and contributes no
    /// hint.
    fn lower_via_hint(
        &mut self,
        via: &radix_syntax::Ident,
        target: &HirConversioTarget,
        params: &mut Vec<radix_lexer::Symbol>,
    ) {
        let hint = self.interner.resolve(via.name).to_owned();
        // FABER-2-SUNSET: delete with the retired-word diagnostics.
        // Need (ruling 1dce2442, C3): `via Bits` retired entirely;
        // a bit pattern is a `bN` value and `↦ bN` reads or writes it.
        let issue = if hint == "Bits" {
            Some("conversio_via_bits_retired")
        } else if crate::numeric_width::is_float_rounding_hint(&hint) {
            // Need: a rounding hint belongs to an integer-family
            // target; the source (float or decimal) is the checker's rule.
            (!via_target_takes_rounding_hint(self, target)).then_some("conversio_via_unknown_hint")
        } else if !crate::numeric_width::is_conversio_radix_hint(&hint) {
            Some("conversio_via_unknown_hint")
        } else if !via_target_takes_hint(self, target) {
            Some("conversio_via_target_takes_no_hint")
        } else {
            None
        };
        self.apply_via_hint_verdict(issue, hint, via, params);
    }

    /// Apply a `via`-hint verdict: a rejected hint reports against its own
    /// span and contributes nothing; an accepted hint travels in `params`.
    fn apply_via_hint_verdict(
        &mut self,
        issue: Option<&'static str>,
        hint: String,
        via: &radix_syntax::Ident,
        params: &mut Vec<radix_lexer::Symbol>,
    ) {
        match issue {
            Some(issue) => self.push_error(
                SemanticErrorKind::ConversionInvalid,
                vec![
                    DiagnosticArg::new("issue", issue),
                    DiagnosticArg::new("hint", hint),
                ],
                via.span,
            ),
            None => params.push(via.name),
        }
    }

    /// Lower `expr est <type>` / `expr non est <type>` into a `TypeCheck` HIR node.
    /// The target type is resolved here; typecheck and codegen decide whether
    /// it maps to a Valor variant tag check.
    fn lower_type_check(
        &mut self,
        tc: &radix_syntax::TypeCheckExpr,
        owner: HirId,
    ) -> HirExpressionKind {
        let expr = lower_expr(self, &tc.expr);
        let ty = self.lower_est_target_type(tc, owner);
        HirExpressionKind::TypeCheck {
            positive: tc.positive,
            expr: Box::new(expr),
            ty,
        }
    }

    /// Lower the right-hand side of an `est` test.
    ///
    /// The right-hand side is always a type (DECISIONS D23.1). An enum-variant
    /// name is the one target the type namespace does not carry: `r est M`
    /// targets the variant's member type, exactly as the prior value-shaped
    /// promotion did. Everything else is ordinary authored-type lowering.
    fn lower_est_target_type(&mut self, tc: &radix_syntax::TypeCheckExpr, owner: HirId) -> TypeId {
        if let Some(variant_def) = crate::passes::resolve::est_target_variant(self.resolver, &tc.ty)
        {
            return self.types.intern(crate::Type::Struct(variant_def));
        }
        self.lower_authored_type(
            radix_hir::HirTypeSite {
                owner: radix_hir::HirOwnerRef::Hir(owner),
                slot: radix_hir::HirTypeSlot::TypeCheckTarget,
            },
            &tc.ty,
            false,
            false,
        )
    }

    /// Lower backtick template capture into builtin `forma` genus construction.
    ///
    /// Backtick application captures template text and parameters; it never routes
    /// through `scriptum` rendering.
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
    /// a name load through the lowerer's own scope table. Resolver `DefIds` for
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
            marker: None,
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
                marker: None,
                expr,
                span: arg.span,
            });
        }
        HirExpressionKind::Call(Box::new(callee), Vec::new(), args, None)
    }

    /// Lower a static's `praefixum { … }` body to `Praefixum(Block)`; the
    /// driver evaluates it on the MIR runner (D8.5).
    pub(super) fn lower_praefixum(
        &mut self,
        praefixum: &radix_syntax::PraefixumExpr,
        span: radix_lexer::Span,
    ) -> HirExpressionKind {
        let inner = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Block(self.lower_block(&praefixum.body)),
            ty: None,
            span,
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

/// The authored variant path as bare symbols, for the qualified lookup and
/// for the presentation path recorded against the resolved variant.
fn variant_path_symbols(variant: &[radix_syntax::Ident]) -> Vec<radix_lexer::Symbol> {
    variant.iter().map(|ident| ident.name).collect()
}

fn lower_call_generic_arg(lowerer: &mut Lowerer<'_>, arg: &GenericArgExpr) -> TypeId {
    // The `size` marker is required on every explicit size argument, as the
    // `axis` marker is on an axis argument: a natural, a bare shape, a
    // dimension term, or the bare name of a caller size param says what it
    // is only by position, so it is refused with the fix named.
    if let Some(span) = call_arg_unmarked_size_span(lowerer, arg) {
        lowerer.push_error(
            crate::SemanticErrorKind::ModeMismatch,
            vec![DiagnosticArg::new(
                "issue",
                crate::ISSUE_SIZE_MARKER_REQUIRED,
            )],
            span,
        );
        return lowerer.types.intern(crate::Type::Error);
    }
    if let GenericArgExpr::Size(radix_syntax::FiguraExpr::Lift(lift)) = arg
        && let radix_syntax::LiftExtent::Shape(dims) = lift.as_ref()
    {
        return lower_call_shape_arg(lowerer, dims);
    }
    // A bare name spelling a scalar value is the invisible int to size
    // crossing; the spelled `size n` lift is the did-you-mean.
    if let Some(name) = call_arg_bare_name(arg)
        && matches!(arg, GenericArgExpr::Type(_))
        && call_arg_names_scalar_value(lowerer, name)
    {
        lowerer.error("size_slot_bare_value_name");
        return lowerer.types.intern(crate::Type::Error);
    }
    let spelled_runtime_lift = match arg {
        GenericArgExpr::Size(radix_syntax::FiguraExpr::Lift(lift)) => match lift.as_ref() {
            radix_syntax::LiftExtent::Name(ident) => Some(ident.name),
            _ => None,
        },
        _ => None,
    };
    match lowerer.lower_applied_arg(arg) {
        radix_types::AppliedArg::Type(id) => id,
        radix_types::AppliedArg::Size(id) => {
            match (lowerer.types.get_index(id).clone(), spelled_runtime_lift) {
                (crate::IndexExpr::Literal(value), _) => call_size_arg(lowerer, &value.to_string()),
                (crate::IndexExpr::Param(symbol), _) => {
                    let name = lowerer.interner.resolve(symbol).to_owned();
                    call_size_arg(lowerer, &name)
                }
                (
                    crate::IndexExpr::Runtime(radix_types::RuntimeExtentId::ScalarBinding(_)),
                    Some(lift),
                ) => {
                    let name = lowerer.interner.resolve(lift).to_owned();
                    call_size_arg(lowerer, &name)
                }
                (
                    crate::IndexExpr::Runtime(_)
                    | crate::IndexExpr::Affine { .. }
                    | crate::IndexExpr::Divide { .. }
                    | crate::IndexExpr::Infer(_)
                    | crate::IndexExpr::Unspecified
                    | crate::IndexExpr::Tuple(_),
                    _,
                ) => {
                    lowerer.error("generic_call_size_shape_unsupported");
                    lowerer.types.intern(crate::Type::Error)
                }
            }
        }
    }
}

/// One marked explicit size argument as the `Type::Param` it travels as: the
/// size marker's spelling prefix, then the operand (a natural, a shape of
/// naturals, or a name). A plain `Param` in the same list is a type.
fn call_size_arg(lowerer: &mut Lowerer<'_>, operand: &str) -> TypeId {
    let symbol = lowerer
        .interner
        .intern(&radix_types::spell_explicit_size(operand));
    lowerer.types.intern(crate::Type::Param(symbol))
}

/// Span of an explicit call argument that is a size spelled without the
/// `size` marker, or `None` when the argument is a type or carries the
/// marker. A size can only be a natural, a bracketed shape, a dimension
/// term, or the name of a caller `size` parameter; none of them is a type, so
/// the callee does not need to be known to refuse the bare spelling.
fn call_arg_unmarked_size_span(
    lowerer: &Lowerer<'_>,
    arg: &GenericArgExpr,
) -> Option<radix_lexer::Span> {
    match arg {
        GenericArgExpr::Size(
            radix_syntax::FiguraExpr::Lift(_) | radix_syntax::FiguraExpr::Infer,
        )
        | GenericArgExpr::Range(_) => None,
        GenericArgExpr::Size(_) => Some(lowerer.current_span),
        GenericArgExpr::Type(ty) => {
            let name = call_arg_bare_name(arg)?;
            let is_natural =
                radix_types::parse_natural_capacity(lowerer.interner.resolve(name)).is_some();
            (is_natural || call_arg_names_caller_size_param(lowerer, name)).then_some(ty.span)
        }
        GenericArgExpr::Name(ident) => {
            call_arg_names_caller_size_param(lowerer, ident.name).then_some(ident.span)
        }
    }
}

/// A whole compile-time shape after the marker (`size [4]`, `size [2, 3]`).
/// It travels as one marked `Type::Param` (`magnitudo [2, 3]`), the same
/// carrier a natural size uses. Every dimension must be a natural literal; a `_` hole,
/// a name, a run-time lift or a nested term is refused by name.
fn lower_call_shape_arg(lowerer: &mut Lowerer<'_>, dims: &[radix_syntax::FiguraExpr]) -> TypeId {
    let mut naturals = Vec::with_capacity(dims.len());
    for dim in dims {
        let radix_syntax::FiguraExpr::Literal(value) = dim else {
            lowerer.error("size_shape_dimension_unsupported");
            return lowerer.types.intern(crate::Type::Error);
        };
        naturals.push(*value);
    }
    call_size_arg(lowerer, &radix_types::spell_natural_shape(&naturals))
}

/// Symbol of a bare-name call argument (`sink<T>(x)` parses the name as a
/// nested `Type` at call sites; generic heads keep `Name`).
fn call_arg_bare_name(arg: &GenericArgExpr) -> Option<radix_lexer::Symbol> {
    match arg {
        GenericArgExpr::Type(radix_syntax::TypeExpr {
            kind: radix_syntax::TypeExprKind::Named(ident, nested),
            ..
        }) if nested.is_empty() => Some(ident.name),
        GenericArgExpr::Name(ident) => Some(ident.name),
        _ => None,
    }
}

/// True when `name` denotes a caller-owned compile-time size — a lexical
/// `magnitudo` param or a resolved index param. Typus params keep the type
/// path so wrong-kind rejection stays intact.
fn call_arg_names_scalar_value(lowerer: &Lowerer<'_>, name: radix_lexer::Symbol) -> bool {
    // HIR-local and parameter bindings live in the lowerer's scope stack, not
    // the resolver (mirrors the figura Var arm's runtime-extent lookup);
    // module-scope values keep the resolver path.
    lowerer
        .local_scopes
        .iter()
        .rev()
        .any(|scope| scope.contains_key(&name))
        || lowerer
            .resolver
            .lookup(name)
            .and_then(|def_id| lowerer.resolver.get_symbol(def_id))
            .is_some_and(|symbol| {
                matches!(
                    symbol.kind,
                    crate::SymbolKind::Local | crate::SymbolKind::Param
                )
            })
}

fn call_arg_names_caller_size_param(lowerer: &Lowerer<'_>, name: radix_lexer::Symbol) -> bool {
    if matches!(
        lowerer.lexical_generic_param_kind(name),
        Some(radix_syntax::GenericParamKind::Magnitudo)
    ) {
        return true;
    }
    lowerer
        .resolver
        .lookup(name)
        .and_then(|def_id| lowerer.resolver.get_symbol(def_id))
        .is_some_and(|symbol| symbol.kind == crate::SymbolKind::IndexParam)
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
            let Some(type_params) = generic_type_exprs(target_params) else {
                lowerer.error("generic_size_argument_in_type_position");
                return lowerer.types.intern(crate::Type::Error);
            };
            let mut messages = Vec::new();
            let ty_id = crate::instans_precision::lower_sized_instans(
                &type_params,
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
                Some(Primitive::Ascii | Primitive::Octeti | Primitive::Textus)
            ) && !target_params.is_empty() =>
        {
            // Bounded ascii/octeti/textus are real conversion targets, not
            // radix hints. Preserve capacity so typecheck and Rust emit can
            // enforce D7 (`len ≤ N`, spare slots unread).
            lowerer.lower_type(ty)
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
                form: radix_syntax::TypeFormLevel::LONG,
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

fn generic_type_expr(arg: &GenericArgExpr) -> Option<TypeExpr> {
    match arg {
        GenericArgExpr::Type(ty) => Some(ty.clone()),
        GenericArgExpr::Name(name) => Some(TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Named(name.clone(), Vec::new()),
            span: name.span,
            form: radix_syntax::TypeFormLevel::LONG,
            iuncta_label: None,
        }),
        GenericArgExpr::Size(_) | GenericArgExpr::Range(_) => None,
    }
}

fn generic_type_exprs(args: &[GenericArgExpr]) -> Option<Vec<TypeExpr>> {
    args.iter().map(generic_type_expr).collect()
}

/// Whether a `↦` target can carry a convert hint: the numeric families, the
/// ascii format pack, `littera` (`Code`), the register windows (`vector<T, size N>`
/// and `octeti<N>` take `Be`/`Le`), or a generic parameter that a caller
/// instantiates. Typecheck still decides which (source, target, hint) rows
/// exist; this only rejects targets no row can ever carry a hint for.
fn via_target_takes_hint(lowerer: &Lowerer<'_>, target: &HirConversioTarget) -> bool {
    let HirConversioTarget::Type(ty) = target else {
        return false;
    };
    let mut ty = *ty;
    loop {
        match lowerer.types.get(ty) {
            crate::Type::Option(inner) | crate::Type::Ref(_, inner) => ty = *inner,
            // `Octeti` takes `Be`/`Le` for the `inf` encode row (ruling 40).
            crate::Type::Primitive(
                Primitive::Numerus
                | Primitive::Fractus
                | Primitive::Ascii
                | Primitive::Littera
                | Primitive::Octeti,
            )
            | crate::Type::SizedNumeric(..)
            | crate::Type::ModularWord(_)
            | crate::Type::SaturatingWord(_)
            | crate::Type::PolicyFloat(..)
            | crate::Type::BoundedAscii { .. }
            | crate::Type::BoundedOcteti { .. }
            | crate::Type::Vector(..)
            // `octeti ↦ tensor<E, [shape]> via Be|Le` (byte reinterpretation).
            | crate::Type::Tensor(..)
            | crate::Type::Param(_)
            | crate::Type::Alias(..)
            // `bN` conversions: the hint is refused by name in
            // the checker when the width stays the same.
            | crate::Type::Bits(_)
            | crate::Type::Error => return true,
            // `lista<bN> ↔ lista<number>` takes `via Le|Be` exactly when the
            // width changes (B5); the checker owns that rule, since only it
            // sees the source type. Any other numeric list refuses there too.
            crate::Type::Array(elem) => {
                return matches!(
                    lowerer.types.get(*elem),
                    crate::Type::Bits(_)
                        | crate::Type::SizedNumeric(..)
                        | crate::Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                );
            }
            _ => return false,
        }
    }
}

/// Whether a `↦` target can carry a rounding hint (`via Floor|Ceil|HalfEven|
/// Trunc`, need): an integer-family target (`numerus`, sized
/// integers, `inf`, `wrapping<W>`, `saturating<W>`). An alias or generic
/// parameter is left to the checker, which sees the resolved pairing.
fn via_target_takes_rounding_hint(lowerer: &Lowerer<'_>, target: &HirConversioTarget) -> bool {
    let HirConversioTarget::Type(ty) = target else {
        return false;
    };
    match lowerer.types.get(*ty) {
        crate::Type::Primitive(Primitive::Numerus)
        | crate::Type::ModularWord(_)
        | crate::Type::SaturatingWord(_)
        | crate::Type::Alias(..)
        | crate::Type::Param(_)
        | crate::Type::Error => true,
        crate::Type::SizedNumeric(Primitive::Numerus, width) => !width.is_decimal(),
        _ => false,
    }
}

/// Extract a user-written conversion hint from a type expression.
///
/// Only bare named syntax contributes a hint. More complex type expressions are
/// still lowered as target types, but they are not collapsed into a single
/// symbol because that would lose the structure typecheck needs later.
fn conversio_hint_symbol(arg: &GenericArgExpr) -> Option<radix_lexer::Symbol> {
    match arg {
        GenericArgExpr::Name(ident) => Some(ident.name),
        GenericArgExpr::Type(ty) => match &ty.kind {
            TypeExprKind::Named(ident, _) => Some(ident.name),
            _ => None,
        },
        GenericArgExpr::Size(_) | GenericArgExpr::Range(_) => None,
    }
}
