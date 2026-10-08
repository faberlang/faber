//! `empty` (`vacua`) at a dense tensor, vector, or matrix type is the typed zero.
//!
//! Operator ruling 2026-10-08: `empty` is sugar for the default empty instance
//! of the declared type on the left; for a tensor, vector, or matrix that
//! instance is the zero, so `tf32[2, 3] blank ← empty` means exactly
//! `tf32[2, 3] blank ← tf32[2, 3] {}`.
//!
//! The expected type is only known here, so the checker rewrites the bare
//! `Vacua` node into the very HIR that lowering synthesizes for the typed
//! zero `Type {}` (a `crea` call on a pre-typed `vacua` receiver for a tensor;
//! a zero array converted to the register type for a vector or matrix). Every
//! later stage and every backend then follows the typed-zero path with no
//! second spelling of the same value.
//!
//! Left alone, by design: rank-0 tensors (`tensor<T, []>`), a shape that is
//! one whole unresolved hole, `sparsa`, and every non-dense collection.

use radix_hir::{HirArrayElement, HirCallArg, HirExpression, HirExpressionKind, HirLiteral};
use radix_lexer::Span;
use radix_types::{IndexExpr, IndexId, RuntimeExtentId, Type, TypeId, numeric_width};

use super::TypeChecker;

/// Issue for an `empty` whose dense shape has no buildable extents.
pub(super) const ISSUE_VACUA_REQUIRES_RESOLVED_EXTENTS: &str = "vacua_requires_resolved_extents";

/// What the rewrite decided for one `empty` expression.
pub(super) enum EmptyDense {
    /// Not a dense tensor, vector, or matrix slot: check the literal as before.
    NotDense,
    /// The node now holds the typed-zero HIR.
    Rewritten,
    /// The shape has no buildable extents; a diagnostic was recorded and the
    /// node stays a bare literal that types as the error type.
    Refused,
}

impl TypeChecker<'_> {
    /// Rewrite an `empty` literal at a dense slot into the typed zero.
    pub(super) fn desugar_empty_dense(
        &mut self,
        expr: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> EmptyDense {
        let Some(expected) = expected else {
            return EmptyDense::NotDense;
        };
        let resolved = self
            .types
            .peel_access_marker(self.resolve_inferred_type(expected))
            .1;
        if self.is_infer(resolved) {
            return EmptyDense::NotDense;
        }
        let resolved = self.vacua_collection_member(resolved).unwrap_or(resolved);
        let span = expr.span;
        let kind = match self.types.get(resolved).clone() {
            Type::Tensor(element, shape) => self.empty_tensor_zero(resolved, element, shape, span),
            Type::Vector(element, width) => {
                self.empty_register_zero(resolved, element, &[width], span)
            }
            Type::Matrix(element, shape) => {
                let dims = self.tuple_dims(shape);
                match dims {
                    Some(dims) if dims.len() == 2 => {
                        self.empty_register_zero(resolved, element, &dims, span)
                    }
                    _ => Err(()),
                }
            }
            _ => return EmptyDense::NotDense,
        };
        match kind {
            Ok(Some(kind)) => {
                expr.kind = kind;
                self.empty_dense_ids.push(expr.id);
                EmptyDense::Rewritten
            }
            Ok(None) => EmptyDense::NotDense,
            Err(()) => {
                self.invalid_operand_types(ISSUE_VACUA_REQUIRES_RESOLVED_EXTENTS, span);
                EmptyDense::Refused
            }
        }
    }

    /// The dimension terms of a tuple shape, or `None` for a non-tuple shape.
    fn tuple_dims(&mut self, shape: IndexId) -> Option<Vec<IndexId>> {
        let shape = self.resolve_index(shape);
        match self.types.get_index(shape) {
            IndexExpr::Tuple(dims) => Some(dims.clone()),
            _ => None,
        }
    }

    /// A tensor zero: `crea` on a receiver pre-typed with the tensor.
    ///
    /// `Ok(None)` leaves rank-0 and whole-shape holes to the existing path.
    fn empty_tensor_zero(
        &mut self,
        claimed: TypeId,
        element: TypeId,
        shape: IndexId,
        span: Span,
    ) -> Result<Option<HirExpressionKind>, ()> {
        let Some(dims) = self.tuple_dims(shape) else {
            return Ok(None);
        };
        if dims.is_empty() {
            return Ok(None);
        }
        let mut extents = Vec::with_capacity(dims.len());
        for dim in dims {
            let extent = self.extent_value(dim, span)?;
            extents.push(HirArrayElement::Expr(extent));
        }
        let fill = self.zero_of(element, span)?;
        let extents = self.synthesized_expr(HirExpressionKind::Array(extents), span);
        let receiver = HirExpression {
            id: self.fresh_hir_id(),
            kind: HirExpressionKind::Vacua,
            ty: Some(claimed),
            span,
        };
        let name = self.crea_symbol().ok_or(())?;
        let arg = |expr| HirCallArg {
            name: None,
            spread: false,
            marker: None,
            expr,
            span,
        };
        Ok(Some(HirExpressionKind::MethodCall(
            Box::new(receiver),
            name,
            Vec::new(),
            vec![arg(fill), arg(extents)],
            None,
        )))
    }

    /// A vector or matrix zero: a zero array converted to the register type.
    /// Register extents must be literal, exactly as for the typed zero.
    fn empty_register_zero(
        &mut self,
        claimed: TypeId,
        element: TypeId,
        dims: &[IndexId],
        span: Span,
    ) -> Result<Option<HirExpressionKind>, ()> {
        let mut lengths = Vec::with_capacity(dims.len());
        for dim in dims {
            lengths.push(self.literal_extent(*dim).ok_or(())?);
        }
        let zero = self.zero_of(element, span)?;
        let source = match lengths.as_slice() {
            [lanes] => self.zero_row(&zero, *lanes, span),
            [rows, cols] => {
                let mut row_exprs = Vec::new();
                for _ in 0..*rows {
                    let row = self.zero_row(&zero, *cols, span);
                    row_exprs.push(HirArrayElement::Expr(self.synthesized_expr(row, span)));
                }
                HirExpressionKind::Array(row_exprs)
            }
            _ => return Err(()),
        };
        let source = self.synthesized_expr(source, span);
        Ok(Some(HirExpressionKind::Verte {
            source: Box::new(source),
            target: claimed,
            entries: None,
        }))
    }

    fn zero_row(&mut self, zero: &HirExpression, lanes: usize, span: Span) -> HirExpressionKind {
        let mut row = Vec::with_capacity(lanes);
        for _ in 0..lanes {
            let mut lane = zero.clone();
            lane.id = self.fresh_hir_id();
            lane.span = span;
            row.push(HirArrayElement::Expr(lane));
        }
        HirExpressionKind::Array(row)
    }

    /// The zero of an element type: a zero register for a vector or matrix
    /// element, else the scalar zero literal of its numeric family.
    fn zero_of(&mut self, element: TypeId, span: Span) -> Result<HirExpression, ()> {
        let resolved = self.resolve_inferred_type(element);
        let kind = match self.types.get(resolved).clone() {
            Type::Vector(inner, width) => {
                self.empty_register_zero(resolved, inner, &[width], span)?
            }
            Type::Matrix(inner, shape) => {
                let dims = self.tuple_dims(shape).ok_or(())?;
                self.empty_register_zero(resolved, inner, &dims, span)?
            }
            _ if numeric_width::is_tensor_float_element(self.types, resolved) => {
                Some(HirExpressionKind::Literal(HirLiteral::Float(0.0)))
            }
            _ => Some(HirExpressionKind::Literal(HirLiteral::Int(0))),
        };
        let kind = kind.ok_or(())?;
        Ok(self.synthesized_expr(kind, span))
    }

    /// One tensor extent as a value expression: a literal, a declared size
    /// binding, or a forwarded size parameter. Anything else has no value to
    /// build the zero from (a `_` hole, an affine term, a runtime axis).
    fn extent_value(&mut self, dim: IndexId, span: Span) -> Result<HirExpression, ()> {
        let dim = self.resolve_index(dim);
        let kind = match self.types.get_index(dim).clone() {
            IndexExpr::Literal(value) => HirExpressionKind::Literal(HirLiteral::Int(value)),
            IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(def)) => HirExpressionKind::Path(def),
            IndexExpr::Param(symbol) => {
                let def = self
                    .size_param_def(symbol)
                    .or_else(|| self.hir_value_binding_def(symbol))
                    .ok_or(())?;
                HirExpressionKind::Path(def)
            }
            _ => return Err(()),
        };
        Ok(self.synthesized_expr(kind, span))
    }

    /// The declaration of the enclosing function's `size` parameter named `symbol`.
    fn size_param_def(&self, symbol: radix_lexer::Symbol) -> Option<radix_hir::DefId> {
        self.current_size_param_by_name.get(&symbol).copied()
    }

    /// A register extent that is a compile-time natural.
    fn literal_extent(&mut self, dim: IndexId) -> Option<usize> {
        let dim = self.resolve_index(dim);
        match radix_types::evaluate_index_dimension(dim, self.types) {
            Ok(Some(value)) => usize::try_from(value).ok(),
            _ => None,
        }
    }

    /// The `crea` method symbol the typed-zero constructor uses.
    ///
    /// Typecheck recognizes the synthesized call by the pack's surface
    /// spelling and rewrites it to the canonical one; the checker cannot
    /// intern, so it takes whichever spelling the interner already holds.
    fn crea_symbol(&self) -> Option<radix_lexer::Symbol> {
        let interner = self.interner?;
        let surface = self
            .locale_pack
            .map_or("crea", |pack| pack.intrinsic_surface("crea"));
        interner.lookup(surface).or_else(|| interner.lookup("crea"))
    }

    fn synthesized_expr(&mut self, kind: HirExpressionKind, span: Span) -> HirExpression {
        HirExpression {
            id: self.fresh_hir_id(),
            kind,
            ty: None,
            span,
        }
    }
}
