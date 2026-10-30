//! Expression synthesis and expected-type propagation for HIR type checking.
//!
//! This file is the central dispatcher for expression typing after name
//! resolution and HIR lowering have already chosen expression forms and
//! `DefId`s. Most expressions synthesize a type from their children, while a
//! smaller set accepts an expected type from declarations, arguments, closures,
//! or control-flow joins. That expected context is advisory until the final
//! `unify` at the bottom of `check_expr_with_expected`; helpers may use it
//! earlier for constructs such as `vacua`, array literals, blocks, branches,
//! and closures where syntax alone cannot recover the intended shape.
//!
//! ERROR STRATEGY
//! ==============
//! The checker keeps walking after local failures. Invalid lowered expressions
//! and missing contextual type information return the shared error type so later
//! phases can still attach diagnostics to neighboring code instead of losing the
//! rest of the file. `ignotum` remains an explicit escape type handled by the
//! specialized helpers; it is not used here as a silent replacement for missing
//! collection or nullability information.
//!
//! BOUNDARY
//! ========
//! This pass records resolved expression types on HIR nodes. It does not choose
//! target-specific lowering policy, library translation, or codegen fallbacks.

use super::store::StoreSite;
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_hir::HirStoreKind;

const ISSUE_IACE_REQUIRES_ALTERNATE_EXIT: &str = "iace_requires_alternate_exit";
const ISSUE_IACE_VALUE_TYPE_MISMATCH: &str = "iace_value_type_mismatch";
const ISSUE_ASSERT_CONDITION_TYPE_MISMATCH: &str = "assert_condition_type_mismatch";
pub(super) const ISSUE_STREAM_AD_OPENER_TYPE_MISMATCH: &str = "stream_ad_opener_type_mismatch";
const ISSUE_EXPRESSION_TYPE_MISMATCH: &str = "expression_type_mismatch";
const ISSUE_INTEGER_QUOTIENT_TO_FLOAT: &str = "integer_quotient_to_float_requires_true_division";
const ISSUE_ITERA_AB_SOURCE_REQUIRES_RANGE: &str = "itera_ab_source_requires_range";
pub(super) const ISSUE_CURSOR_ITERATION_REQUIRES_EX: &str = "cursor_iteration_requires_ex";
const ISSUE_ITERA_APUD_REQUIRES_EX: &str = "itera_apud_requires_ex";
pub(super) const ISSUE_ITERA_APUD_ARITY_MISMATCH: &str = "itera_apud_arity_mismatch";
pub(super) const ISSUE_ITERA_APUD_REQUIRES_SHAPED_ITERABLE: &str =
    "itera_apud_requires_shaped_iterable";
/// D3.1: `itera ex … apud` over a source with no index surface (copia,
/// cursors and generators, text, sparsa).
const ISSUE_ITERA_APUD_REQUIRES_INDEXED_ITERABLE: &str = "itera_apud_requires_indexed_iterable";
/// Lockstep `itera ex` N-ary series laws. All are the same
/// fail-closed SEM family as the `apud` arity rule above.
pub(super) const ISSUE_ITERA_LOCKSTEP_SOURCE_BINDER_ARITY_MISMATCH: &str =
    "itera_lockstep_source_binder_arity_mismatch";
pub(super) const ISSUE_ITERA_LOCKSTEP_AT_COORDINATE_CARDINALITY: &str =
    "itera_lockstep_at_coordinate_cardinality";
pub(super) const ISSUE_ITERA_LOCKSTEP_SOURCE_REQUIRES_LISTA: &str =
    "itera_lockstep_source_requires_lista";
pub(super) const ISSUE_ITERA_LOCKSTEP_LENGTH_MISMATCH: &str = "itera_lockstep_length_mismatch";
const ISSUE_CEDE_REQUIRES_PROMISSUM: &str = "cede_requires_promissum";
const ISSUE_CEDE_REQUIRES_GENERATOR: &str = "cede_requires_generator";
const ISSUE_REDDET_REQUIRES_ASYNC_FUNCTION: &str = "reddet_requires_async_function";
const ISSUE_TACEBIT_REQUIRES_ASYNC_CONTEXT: &str = "tacebit_requires_async_context";
const ISSUE_INVALID_LOWERED_EXPRESSION: &str = "invalid_lowered_expression";
/// Phase 2A fail-closed fact: the dedicated gradient node exists in HIR, but the
/// operand-eligibility and selector rules land with tensor-glyphs 2B. No `∇`
/// expression may pass typecheck untyped or silently.
const ISSUE_GRADIENT_EXPRESSION_UNSUPPORTED: &str = "gradient_expression_unsupported";

/// Rank (shape length) of a tensor shape when statically known.
///
/// `Literal` and `Param` shapes are rank-1 (`[N]` parses as a one-element
/// `Tuple`); compound shapes contribute their dimension count. Inference
/// holes and the pre-grammar unspecified bridge carry no static rank.
pub(super) fn tensor_shape_rank(types: &TypeTable, shape: crate::IndexId) -> Option<u64> {
    match types.get_index(shape) {
        IndexExpr::Literal(_)
        | IndexExpr::Param(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Affine { .. }
        | IndexExpr::Divide { .. } => Some(1),
        IndexExpr::Tuple(dims) => Some(dims.len() as u64),
        IndexExpr::Infer(_) | IndexExpr::Unspecified => None,
    }
}

/// Shared `apud` arity law (AC-5; reused by `reducta via` admission, SFR-3):
/// the coordinate count must equal the iterable's rank — tensor shape length
/// or matrix 2 — never a partial bind. A rank >= 2 iterable without an `apud`
/// clause is arity 0 != rank. Non-shaped iterables reject coordinates
/// fail-closed; coordinate-free iteration over a non-shaped iterable is left
/// to the owning construct's own iterable surface.
pub(super) enum ApudArity {
    /// Coordinate-free rank-1 shaped iterable, or exact coordinate/rank match.
    Admitted,
    /// Non-shaped iterable carrying `apud` coordinates (fail closed).
    RequiresShaped,
    /// Coordinate count does not equal the iterable's rank.
    Mismatch { expected: u64, actual: usize },
}

pub(super) fn apud_arity(
    types: &TypeTable,
    dispatch_ty: TypeId,
    has_coordinates: bool,
    coordinate_count: usize,
) -> ApudArity {
    let rank = match types.get(dispatch_ty) {
        Type::Tensor(_, shape) => tensor_shape_rank(types, *shape),
        Type::Matrix(_, _) => Some(2),
        _ => None,
    };
    match rank {
        Some(rank) if !has_coordinates && rank == 1 => ApudArity::Admitted,
        Some(rank) if rank == coordinate_count as u64 => ApudArity::Admitted,
        Some(rank) => ApudArity::Mismatch {
            expected: rank,
            actual: coordinate_count,
        },
        None if has_coordinates => ApudArity::RequiresShaped,
        None => ApudArity::Admitted,
    }
}

/// `itera ex … apud` admission (D3.1): the coordinate types when the names
/// match the iterable's index surface.
enum IteraApud {
    /// One type per `apud` name (empty when there are none).
    Admitted(Vec<TypeId>),
    /// The iterable has no index surface for `apud` names.
    RequiresIndexed,
    /// Name count does not equal the iterable's rank.
    Mismatch { expected: u64, actual: usize },
}

/// Index types of an indexed iterable: `lista` is rank 1 over `numerus`;
/// `tabula` is rank 1 over its key, or rank N over the parts of an
/// `iuncta` key (D3.2/D3.3). Tensors and matrices keep the shared
/// [`apud_arity`] law with `numerus` coordinates.
fn itera_apud_coordinates(
    types: &TypeTable,
    dispatch_ty: TypeId,
    numerus: TypeId,
    has_coordinates: bool,
    coordinate_count: usize,
) -> IteraApud {
    let index_types = match types.get(dispatch_ty) {
        Type::Array(_) => vec![numerus],
        Type::Map(key, _) | Type::Ledger(key, _) => {
            let mut peeled = *key;
            while let Type::Alias(_, inner) = types.get(peeled) {
                peeled = *inner;
            }
            match types.get(peeled) {
                Type::Tuple(parts) => parts.clone(),
                _ => vec![*key],
            }
        }
        _ => {
            return match apud_arity(types, dispatch_ty, has_coordinates, coordinate_count) {
                ApudArity::Admitted => IteraApud::Admitted(vec![numerus; coordinate_count]),
                ApudArity::RequiresShaped => IteraApud::RequiresIndexed,
                ApudArity::Mismatch { expected, actual } => {
                    IteraApud::Mismatch { expected, actual }
                }
            };
        }
    };
    // Coordinate-free value iteration over a lista or tabula stays legal.
    if !has_coordinates || index_types.len() == coordinate_count {
        return IteraApud::Admitted(if has_coordinates {
            index_types
        } else {
            Vec::new()
        });
    }
    IteraApud::Mismatch {
        expected: index_types.len() as u64,
        actual: coordinate_count,
    }
}

impl TypeChecker<'_> {
    /// Ruling 25 (air2): inside an AIR function the variable of an `itera ab`
    /// loop whose bounds are both constants lies between them, so it takes the
    /// smallest integer type holding the hull of the two bounds (ruling 12).
    /// The hull is sound for exclusive or inclusive ranges, either direction
    /// and any step, because a loop never leaves the interval it walks.
    fn constant_range_loop_type(&mut self, iter: &HirExpression, bound: TypeId) -> TypeId {
        let HirExpressionKind::Intervallum { start, end, .. } = &iter.kind else {
            return bound;
        };
        let (Some(a), Some(b)) = (
            super::integer_range::const_int_value(start),
            super::integer_range::const_int_value(end),
        ) else {
            return bound;
        };
        if !self.current_air_function || bound != self.numerus_type() {
            return bound;
        }
        self.width_type(radix_types::numeric_range::smallest_width((
            a.min(b),
            a.max(b),
        )))
    }

    /// Ruling 25 (air2): inside an AIR function an `apud` coordinate over a
    /// tensor with constant dimensions is exactly `0 .. dim - 1`, so it takes
    /// the smallest integer type holding that range (ruling 12) instead of
    /// `numerus`. This is what lets bounded index arithmetic be proven.
    fn narrow_apud_coordinates(&mut self, dispatch_ty: TypeId, coordinate_types: &mut [TypeId]) {
        let Type::Tensor(_, shape) = self.types.get(dispatch_ty) else {
            return;
        };
        let dims: Vec<u64> = match self.types.get_index(*shape) {
            IndexExpr::Literal(n) => vec![*n],
            IndexExpr::Tuple(parts) => {
                let mut dims = Vec::with_capacity(parts.len());
                for part in parts {
                    let IndexExpr::Literal(n) = self.types.get_index(*part) else {
                        return;
                    };
                    dims.push(*n);
                }
                dims
            }
            _ => return,
        };
        if dims.len() != coordinate_types.len() {
            return;
        }
        for (slot, dim) in coordinate_types.iter_mut().zip(dims) {
            let Some(last) = dim.checked_sub(1).map(i128::from) else {
                continue;
            };
            *slot = self.width_type(radix_types::numeric_range::smallest_width((0, last)));
        }
    }

    /// d11-6 ruling 20: an integer constant that typechecked into a float slot
    /// becomes a float constant, so lowering and codegen never see an integer
    /// literal carrying a `fractus` type.
    fn retype_integer_constant_as_float(&self, lit: &mut HirLiteral, ty: TypeId) {
        if !matches!(
            self.types.get(ty),
            Type::Primitive(Primitive::Fractus)
                | Type::SizedNumeric(Primitive::Fractus, _)
                | Type::PolicyFloat(_, _)
        ) {
            return;
        }
        match lit {
            HirLiteral::Int(value) => *lit = HirLiteral::Float(*value as f64),
            // A big literal in a float slot (ruling 34) rounds to nearest;
            // `literal_type` already proved it inside the float's range.
            HirLiteral::BigInt(digits) => {
                if let Some(value) = radix_types::big_literal::parse_big_literal(digits) {
                    *lit = HirLiteral::Float(value.to_f64());
                }
            }
            _ => {}
        }
    }

    /// Whether `expr` is an inferred-constructor hole: `_ { … }` (a `Verte`
    /// whose target is the unresolved hole, with fields or constant entries)
    /// or a bare bracket literal `[ … ]`. The bare literal is sugar for the
    /// hole form (operator ruling 2026-10-10), so it solves from a typed cell
    /// and raises the multi-member-union error exactly as `_ [ … ]` does.
    fn is_inferred_constructor_hole(&self, expr: &HirExpression) -> bool {
        if matches!(expr.kind, HirExpressionKind::Array(_)) {
            return true;
        }
        let HirExpressionKind::Verte {
            source,
            target,
            entries,
        } = &expr.kind
        else {
            return false;
        };
        self.is_infer(self.resolve_inferred_type(*target))
            && (entries.is_some()
                || matches!(
                    &source.kind,
                    HirExpressionKind::Literal(
                        HirLiteral::JsonValor(radix_syntax::JsonValue::Object(_))
                            | HirLiteral::Regex(..)
                    ) | HirExpressionKind::Array(_)
                ))
    }

    /// Operator ruling 2026-10-14: the anonymous object IS `record`. A bare
    /// `{ field = value }` literal leaves its type slot unsolved, and the
    /// lowering marks that hole with [`TypeTable::mark_default_record_hole`];
    /// this builds the `record` the literal's own fields shape and writes it
    /// back into `*target`. Returns `None` — with nothing checked — unless the
    /// target is that marked hole and every entry is a named key with an
    /// explicit value, so `_ { … }` (no default) and `{}` fall through to the
    /// caller's fail-closed no-expected-type error. A field whose value stays
    /// an unresolved hole, or a repeated key, reports the same named error.
    fn check_default_record_literal(
        &mut self,
        target: &mut TypeId,
        entries: &mut [HirObjectField],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        if entries.is_empty()
            || !self
                .types
                .is_default_record_hole(self.resolve_inferred_type(*target))
        {
            return None;
        }
        // Shape gate before any checking so a `None` return leaves no side
        // effects: plain named keys only, each with an explicit value.
        let mut named = Vec::with_capacity(entries.len());
        for entry in entries.iter_mut() {
            let name = match &entry.key {
                HirObjectKey::Ident(name) | HirObjectKey::String(name) => *name,
                HirObjectKey::Computed(_) | HirObjectKey::Spread(_) => return None,
            };
            let value = entry.value.as_mut()?;
            named.push((name, value));
        }
        let mut fields = Vec::with_capacity(named.len());
        let mut seen = Vec::with_capacity(named.len());
        for (name, value) in named {
            let checked = self.check_expr(value);
            let value_ty = self.resolve_inferred_type(checked);
            if self.is_infer(value_ty) {
                self.invalid_conversion(
                    super::convert::ISSUE_INFERRED_CONSTRUCTOR_NO_EXPECTED_TYPE,
                    span,
                );
                return Some(self.error_type);
            }
            if seen.contains(&name) {
                self.invalid_conversion(
                    super::convert::ISSUE_INFERRED_CONSTRUCTOR_NO_EXPECTED_TYPE,
                    value.span,
                );
                return Some(self.error_type);
            }
            seen.push(name);
            fields.push((name, value_ty));
        }
        let record_ty = self.types.record(fields);
        *target = record_ty;
        Some(record_ty)
    }

    /// The type an inferred constructor builds when the slot is a union.
    /// `T ∪ none` builds `T` (operator ruling 2026-10-09): the hole behaves as
    /// if the slot were `T` alone. Several non-none members are ambiguous and
    /// raise a named error; a union of only `none` has no type to build, so it
    /// is the no-expected-type error. A slot that is not a union is unchanged.
    fn inferred_constructor_expected(
        &mut self,
        expr: &HirExpression,
        expected: Option<TypeId>,
    ) -> Result<Option<TypeId>, TypeId> {
        let Some(slot) = expected else {
            return Ok(None);
        };
        if !self.is_inferred_constructor_hole(expr) {
            return Ok(expected);
        }
        let ty = self.resolve_inferred_type(slot);
        let mut members = Vec::new();
        if !self.collect_non_none_members(ty, &mut members) {
            return Ok(expected);
        }
        match members.as_slice() {
            [] => Ok(None),
            [only] => Ok(Some(*only)),
            _ => {
                self.invalid_conversion(
                    super::convert::ISSUE_INFERRED_CONSTRUCTOR_AMBIGUOUS_UNION,
                    expr.span,
                );
                // The named error stands alone: the node takes the slot's type.
                Err(slot)
            }
        }
    }

    /// Flattens `ty` through aliases, `Option` and nested unions into its
    /// distinct non-none members. Returns false when `ty` is not a union.
    fn collect_non_none_members(&self, ty: TypeId, out: &mut Vec<TypeId>) -> bool {
        let nihil = self.types.primitive(Primitive::Nihil);
        match self.types.get(ty) {
            Type::Alias(_, inner) => self.collect_non_none_members(*inner, out),
            Type::Option(inner) => {
                self.collect_leaf(*inner, nihil, out);
                true
            }
            Type::Union(members) => {
                for member in members {
                    self.collect_leaf(*member, nihil, out);
                }
                true
            }
            _ => self.types.equals(ty, nihil),
        }
    }

    fn collect_leaf(&self, ty: TypeId, nihil: TypeId, out: &mut Vec<TypeId>) {
        let mut nested = Vec::new();
        if self.collect_non_none_members(ty, &mut nested) {
            for member in nested {
                if !out.iter().any(|seen| self.types.equals(*seen, member)) {
                    out.push(member);
                }
            }
        } else if !self.types.equals(ty, nihil)
            && !out.iter().any(|seen| self.types.equals(*seen, ty))
        {
            out.push(ty);
        }
    }

    /// An empty bare `{}` or `[]` with no typed cell is the double hole
    /// (operator ruling 2026-10-10): an empty literal has no contents, so it
    /// has no default to fall back on. A typed cell keeps the literal's
    /// ordinary construction (json/map entries, list element type, class
    /// defaults). Returns `Some(error_type)` when the object is settled here;
    /// an empty array stays a list so the finalize pass reports its element
    /// hole as before.
    fn reject_empty_literal_without_cell(
        &mut self,
        expr: &HirExpression,
        expected: Option<TypeId>,
    ) -> Option<TypeId> {
        let empty_object = matches!(
            &expr.kind,
            HirExpressionKind::Literal(HirLiteral::JsonValor(radix_syntax::JsonValue::Object(
                members
            ))) if members.is_empty()
        );
        let empty_array = matches!(
            &expr.kind,
            HirExpressionKind::Array(elements) if elements.is_empty()
        );
        if !empty_object && !empty_array {
            return None;
        }
        if expected.is_some_and(|ty| !self.is_infer(self.resolve_inferred_type(ty))) {
            return None;
        }
        self.invalid_conversion(
            super::convert::ISSUE_INFERRED_CONSTRUCTOR_NO_EXPECTED_TYPE,
            expr.span,
        );
        // `{}` has no later backstop diagnostic, so settle it on the error
        // type. `[]` keeps its element hole for the existing finalize error.
        empty_object.then_some(self.error_type)
    }

    /// `_ { … }` content shape. The hole before braces builds the expected
    /// type, and the contents must match what that type takes: ENTRIES
    /// (`"key": value`) build a map or a `json` value exactly as the bare
    /// `{ … }` literal does, FIELDS (`name = value`) build a genus. A map
    /// target with no contents is the empty bare literal. The wrong shape is
    /// a named error. Returns `Some(type)` when the node is settled here.
    fn desugar_inferred_constructor_shape(
        &mut self,
        expr: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> Option<TypeId> {
        let HirExpressionKind::Verte {
            source,
            target,
            entries,
        } = &expr.kind
        else {
            return None;
        };
        if !self.is_infer(self.resolve_inferred_type(*target)) {
            return None;
        }
        let entry_form = entries.is_none()
            && matches!(
                &source.kind,
                HirExpressionKind::Literal(HirLiteral::JsonValor(radix_syntax::JsonValue::Object(
                    _
                )))
            );
        let field_form = entries.is_some();
        if !entry_form && !field_form {
            return None;
        }
        // The authored hole: re-emission prints `_ { … }` for this node id.
        self.inferred_constructor_ids.push(expr.id);
        let no_fields = entries.as_ref().is_some_and(Vec::is_empty);
        let span = expr.span;
        let concrete = expected
            .map(|ty| self.resolve_inferred_type(ty))
            .filter(|ty| !self.is_infer(*ty));
        let Some(concrete) = concrete else {
            if entry_form {
                self.invalid_conversion(
                    super::convert::ISSUE_INFERRED_CONSTRUCTOR_NO_EXPECTED_TYPE,
                    span,
                );
                return Some(self.error_type);
            }
            return None;
        };
        let is_map = matches!(self.types.get(concrete), Type::Map(..) | Type::Ledger(..));
        let json = self.json_type();
        if entry_form {
            if !(is_map || concrete == json) {
                self.invalid_conversion(
                    super::convert::ISSUE_INFERRED_CONSTRUCTOR_ENTRIES_NEED_MAP,
                    span,
                );
                // The named error stands alone: the node takes the slot's type.
                return Some(concrete);
            }
            let placeholder = HirExpressionKind::Tuple(Vec::new(), None);
            if let HirExpressionKind::Verte { source, .. } =
                std::mem::replace(&mut expr.kind, placeholder)
            {
                expr.kind = source.kind;
            }
            return None;
        }
        // `_ {}` at a dense tensor, vector, or matrix slot is the typed zero
        // `Type {}`: the very rewrite `empty` (`vacua`) takes at that slot.
        if no_fields
            && matches!(
                self.types.get(concrete),
                Type::Tensor(..) | Type::Vector(..) | Type::Matrix(..)
            )
        {
            expr.kind = HirExpressionKind::Vacua;
            expr.ty = None;
            return match self.desugar_empty_dense(expr, Some(concrete)) {
                super::empty_dense::EmptyDense::Refused => Some(concrete),
                _ => None,
            };
        }
        if is_map {
            if !no_fields {
                self.invalid_conversion(
                    super::convert::ISSUE_INFERRED_CONSTRUCTOR_FIELDS_FOR_MAP,
                    span,
                );
                // The named error stands alone: the node takes the slot's type.
                return Some(concrete);
            }
            expr.kind = HirExpressionKind::Literal(HirLiteral::JsonValor(
                radix_syntax::JsonValue::Object(Vec::new()),
            ));
        }
        None
    }

    /// `_ [ … ]` and a bare `[ … ]` against a tuple or set cell. The hole
    /// form (`Verte` with an `Infer` target over an array) builds the expected
    /// bracket type: a tuple cell makes the tuple, a set cell makes the
    /// `array ↦ set` conversion, any other cell gets the plain array literal
    /// (and its ordinary checking), and no expected type is a named error. A
    /// bare array literal takes a tuple or set cell's type too (the literal is
    /// a hole with a default; no cell leaves it a list). Returns `Some(type)`
    /// when the node is settled here.
    fn desugar_bracket_literal_shape(
        &mut self,
        expr: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> Option<TypeId> {
        let hole = match &expr.kind {
            HirExpressionKind::Verte {
                source,
                target,
                entries: None,
            } => {
                matches!(source.kind, HirExpressionKind::Array(_))
                    && self.is_infer(self.resolve_inferred_type(*target))
            }
            _ => false,
        };
        let bare = matches!(expr.kind, HirExpressionKind::Array(_));
        if !hole && !bare {
            return None;
        }
        let concrete = expected
            .map(|ty| self.resolve_inferred_type(ty))
            .filter(|ty| !self.is_infer(*ty));
        if hole && concrete.is_none() {
            self.invalid_conversion(
                super::convert::ISSUE_INFERRED_CONSTRUCTOR_NO_EXPECTED_TYPE,
                expr.span,
            );
            return Some(self.error_type);
        }
        // A set cell takes the bracket literal as the source of the same
        // `array ↦ set` conversion the authored `↦ set<T>` checks. The cells'
        // elements are checked against the set's element type below; a spread
        // element is not a plain element list, so it keeps the ordinary rule.
        let set_target = concrete.and_then(|ty| match self.types.get(ty) {
            Type::Set(elem) => Some((ty, *elem)),
            _ => None,
        });
        let all_expr = |elements: &[HirArrayElement]| {
            elements
                .iter()
                .all(|element| matches!(element, HirArrayElement::Expr(_)))
        };
        let set_literal = set_target.is_some()
            && match &expr.kind {
                HirExpressionKind::Verte { source, .. } => match &source.kind {
                    HirExpressionKind::Array(elements) => all_expr(elements),
                    _ => false,
                },
                HirExpressionKind::Array(elements) => all_expr(elements),
                _ => false,
            };
        let is_tuple = concrete.is_some_and(|ty| matches!(self.types.get(ty), Type::Tuple(_)));
        if hole || is_tuple || set_literal {
            // The node is rewritten to an `Array`, `Tuple`, or conversion and
            // loses its authored shape; re-emission keys on this id to print
            // `_ [ … ]` (the hole's type form survives under the node id), the
            // bare `[ … ]` the author wrote, or the authored set literal.
            self.inferred_constructor_ids.push(expr.id);
        }
        if hole {
            let placeholder = HirExpressionKind::Tuple(Vec::new(), None);
            if let HirExpressionKind::Verte { source, .. } =
                std::mem::replace(&mut expr.kind, placeholder)
            {
                expr.kind = source.kind;
            }
        }
        if is_tuple
            && let HirExpressionKind::Array(elements) = &mut expr.kind
            && elements
                .iter()
                .all(|element| matches!(element, HirArrayElement::Expr(_)))
        {
            let items = std::mem::take(elements)
                .into_iter()
                .filter_map(|element| match element {
                    HirArrayElement::Expr(item) => Some(item),
                    _ => None,
                })
                .collect();
            expr.kind = HirExpressionKind::Tuple(items, None);
        }
        if set_literal
            && let Some((set_ty, elem_ty)) = set_target
            && matches!(expr.kind, HirExpressionKind::Array(_))
        {
            // The array stays the conversion source, checked against the
            // set's element type so each item is an element store and a wrong
            // element is the ordinary element mismatch. The conversion keeps
            // the literal's node id (the hole's authored form is keyed to it)
            // and the fresh source id is its checked array child.
            let mut source = HirExpression {
                id: self.fresh_hir_id(),
                kind: std::mem::replace(&mut expr.kind, HirExpressionKind::Error),
                ty: None,
                span: expr.span,
            };
            let array_ty = self.types.array(elem_ty);
            self.check_expr_with_expected(&mut source, Some(array_ty));
            expr.kind = HirExpressionKind::Conversio {
                source: Box::new(source),
                target: HirConversioTarget::Type(set_ty),
                params: Vec::new(),
                recovery: None,
                route: None,
                implicit: None,
            };
            expr.ty = Some(set_ty);
            return Some(set_ty);
        }
        None
    }

    /// `_ /a/` against a typed cell: the hole form (`Verte` with an `Infer`
    /// target over a regex literal) unwraps to the literal so the ordinary
    /// regex check runs against the expected type. With no concrete expected
    /// type the hole is the named no-expected-type error.
    fn desugar_regex_hole(
        &mut self,
        expr: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> Option<TypeId> {
        let hole = match &expr.kind {
            HirExpressionKind::Verte {
                source,
                target,
                entries: None,
            } => {
                matches!(
                    source.kind,
                    HirExpressionKind::Literal(HirLiteral::Regex(..))
                ) && self.is_infer(self.resolve_inferred_type(*target))
            }
            _ => false,
        };
        if !hole {
            return None;
        }
        let concrete = expected
            .map(|ty| self.resolve_inferred_type(ty))
            .filter(|ty| !self.is_infer(*ty));
        if concrete.is_none() {
            self.invalid_conversion(
                super::convert::ISSUE_INFERRED_CONSTRUCTOR_NO_EXPECTED_TYPE,
                expr.span,
            );
            return Some(self.error_type);
        }
        let placeholder = HirExpressionKind::Tuple(Vec::new(), None);
        if let HirExpressionKind::Verte { source, .. } =
            std::mem::replace(&mut expr.kind, placeholder)
        {
            expr.kind = source.kind;
        }
        None
    }

    /// Synthesizes an expression type, optionally using an expected type as
    /// context for shape-sensitive expressions.
    ///
    /// The expected type is not a target annotation and does not bypass local
    /// checking. It gives ambiguous forms enough information to type themselves,
    /// then the synthesized result is unified with that expectation before the
    /// node's final `expr.ty` is recorded. Recovery paths return `error_type`
    /// after emitting diagnostics so callers can continue checking sibling
    /// expressions with a stable sentinel.
    pub fn check_expr_with_expected(
        &mut self,
        expr: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> TypeId {
        // The numeric-twin probe below checked this receiver already. Checking
        // is not idempotent (an intrinsic call writes its canonical method
        // symbol back, which a second pass rejects as a wrong-locale spelling),
        // so the probed type stands in for the second check.
        if let Some((probed, ty)) = self.twin_probed_receiver
            && probed == expr.id
        {
            self.twin_probed_receiver = None;
            return ty;
        }
        let saved_pending_conversion = self.pending_conversion.take();
        let store = self.pending_store.take();
        // Need: `t.sum(axis n)` / `t.mean(axis n)` expand to the
        // grouped reduction before the call is checked.
        if let super::reduction::AxisReduction::Refused = self.desugar_axis_reduction(expr) {
            self.pending_conversion = saved_pending_conversion;
            return self.error_type;
        }
        // Need: `t.expande([...], axis n)` marks the new dimension of
        // a broadcast-back; invalid target rank and axis are named errors.
        if let super::expanded_axis::ExpandedAxis::Refused = self.desugar_expanded_axis(expr) {
            self.pending_conversion = saved_pending_conversion;
            return self.error_type;
        }
        // SOFTAXIS: `t.softmax(axis n)` may name the last axis
        // explicitly; the marker is cleared before the call is checked.
        if let super::reduction::SoftmaxAxis::Refused = self.desugar_softmax_axis(expr) {
            self.pending_conversion = saved_pending_conversion;
            return self.error_type;
        }
        // ss1: a numeric method twin means its operator; rewrite it first.
        let in_place_twin = self.desugar_numeric_twin(expr);
        // Ruling 2026-10-08: `empty` at a dense tensor, vector, or matrix slot
        // is the typed zero of that slot; rewrite it before it is checked.
        let empty_refused = matches!(expr.kind, HirExpressionKind::Vacua)
            && expr.ty.is_none()
            && matches!(
                self.desugar_empty_dense(expr, expected),
                super::empty_dense::EmptyDense::Refused
            );
        if let (Some(_), Some(slot)) = (&store, expected) {
            self.mark_word_ring(expr, slot);
        }
        let expected = match self.inferred_constructor_expected(expr, expected) {
            Ok(expected) => expected,
            Err(slot) => {
                self.pending_conversion = saved_pending_conversion;
                return slot;
            }
        };
        if let Some(settled) = self.reject_empty_literal_without_cell(expr, expected) {
            self.pending_conversion = saved_pending_conversion;
            return settled;
        }
        if let Some(settled) = self.desugar_inferred_constructor_shape(expr, expected) {
            self.pending_conversion = saved_pending_conversion;
            return settled;
        }
        if let Some(settled) = self.desugar_bracket_literal_shape(expr, expected) {
            self.pending_conversion = saved_pending_conversion;
            return settled;
        }
        if let Some(settled) = self.desugar_regex_hole(expr, expected) {
            self.pending_conversion = saved_pending_conversion;
            return settled;
        }
        let ty = match &mut expr.kind {
            HirExpressionKind::Path(def_id) => self.check_path(*def_id, expr.span),
            HirExpressionKind::Literal(lit) => {
                let ty = self.literal_type(lit, expr.span, expected);
                self.retype_integer_constant_as_float(lit, ty);
                ty
            }
            // The refusal is already named; typing the node as its slot keeps
            // the follow-on store checks from piling mismatches on top of it.
            HirExpressionKind::Vacua if empty_refused => expected.unwrap_or(self.error_type),
            HirExpressionKind::Vacua => self.check_vacua(expected, expr.span),
            HirExpressionKind::Binary(op, lhs, rhs)
                if matches!(*op, HirBinOp::Between | HirBinOp::NotBetween) =>
            {
                // The unified membership operator: the right operand's type
                // picks range or collection, and the node records the pick.
                self.check_membership(op, lhs, rhs)
            }
            HirExpressionKind::Binary(op, lhs, rhs) => self.check_binary(*op, lhs, rhs, expected),
            HirExpressionKind::Unary(op, operand) => self.check_unary(*op, operand, expected),
            HirExpressionKind::Call(callee, type_args, args, conversion) => {
                let previous_shape_call = self.current_shape_call.replace(expr.id);
                let ty = self.check_call(callee, type_args, args, expected);
                self.current_shape_call = previous_shape_call;
                *conversion = self.pending_conversion.take();
                // A call may write any mutable source it can reach (need
                // 5d3a8afd, closed rule): arguments were evaluated first.
                self.kill_mutable_source_ranges();
                ty
            }
            HirExpressionKind::Gradient { .. } => {
                // Fail closed until tensor-glyphs 2B lands the operand
                // eligibility rule, selector resolution, and result carrier.
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_GRADIENT_EXPRESSION_UNSUPPORTED,
                    expr.span,
                );
                self.error_type
            }
            HirExpressionKind::CopyInto { target, value } => {
                let ty = self.check_copy_into(target, value, expr.span);
                self.kill_mutable_source_ranges();
                ty
            }
            HirExpressionKind::SinkInto { sink, item } => {
                let ty = self.check_sink_into(sink, item, expr.span);
                self.kill_mutable_source_ranges();
                ty
            }
            HirExpressionKind::Format { value, spec } => {
                super::format::check_format_expr(self, value, *spec, expr.span)
            }
            HirExpressionKind::MethodCall(receiver, name, type_args, args, conversion) => {
                let previous_shape_call = self.current_shape_call.replace(expr.id);
                let ty = self.check_method_call(receiver, name, type_args, args, expected);
                self.current_shape_call = previous_shape_call;
                *conversion = self.pending_conversion.take();
                self.record_inferred_sermo_view_type_arg(receiver, *name, type_args, ty);
                // Mutating intrinsic, closure call or ordinary method: all
                // kill (need, closed rule). The exemption is a
                // registry intrinsic that only reads (`longitudo`, `apud`,
                // `sectio`, ...): it cannot write any list.
                if !self.is_read_only_intrinsic_call(receiver, *name, args.len()) {
                    self.kill_mutable_source_ranges();
                }
                ty
            }
            HirExpressionKind::Field(object, name) => self.check_field(object, *name),
            HirExpressionKind::Index(object, index) => {
                let ty = self.check_index(object, index);
                self.record_index_proof(expr.id, object, index);
                ty
            }
            HirExpressionKind::OptionalChain(object, chain) => {
                self.check_optional_chain(object, chain, expr.span)
            }
            HirExpressionKind::NonNull(object, chain) => {
                self.check_non_null(object, chain, expr.span)
            }
            HirExpressionKind::Block(block) => self.check_block(block, expected),
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => self.check_if(
                cond,
                then_block,
                then_catch.as_deref_mut(),
                else_block.as_mut(),
                expected,
            ),
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => self.check_match(scrutinees, arms, expected),
            HirExpressionKind::Loop(block) => {
                self.check_loop_block(block);
                self.vacuum_type()
            }
            HirExpressionKind::Dum(cond, block) => {
                self.check_condition(cond);
                self.check_dum_block(cond, block);
                self.vacuum_type()
            }
            HirExpressionKind::Itera {
                mode,
                coordinates: coordinate_bindings,
                binding,
                binding_name,
                iter,
                body: block,
                ..
            } => self.check_itera_expr(
                mode,
                coordinate_bindings,
                (*binding, *binding_name),
                iter,
                block,
            ),
            HirExpressionKind::IteraLockstep {
                sources,
                coordinates,
                bindings,
                body,
                ..
            } => super::lockstep::check_lockstep_expr(
                self,
                sources,
                coordinates,
                bindings,
                body,
                expr.span,
            ),
            HirExpressionKind::IteraRange {
                ranges,
                bindings,
                body,
                ..
            } => super::range::check_range_product_expr(self, ranges, bindings, body, expr.span),
            HirExpressionKind::Reduction {
                op,
                source,
                coordinates,
                group,
                filum,
                binding,
                body,
                identity,
                ..
            } => {
                // An axis expansion supplies its own bucket shape.
                let expected = self.axis_reduction_results.remove(&expr.id).or(expected);
                super::reduction::check_reduction_expr(
                    self,
                    *op,
                    source,
                    coordinates,
                    group.as_mut(),
                    *filum,
                    *binding,
                    body.as_mut(),
                    identity.as_mut(),
                    expected,
                    expr.span,
                )
            }
            HirExpressionKind::Scan {
                op,
                total,
                source,
                coordinates,
                group,
                group_domain,
                filum,
                binding,
                body,
                ..
            } => super::scan::check_scan_expr(
                self,
                *op,
                *total,
                source,
                coordinates,
                group.as_mut(),
                group_domain,
                *filum,
                *binding,
                body,
                expr.span,
            ),
            HirExpressionKind::Intervallum {
                start, end, step, ..
            } => self.check_intervallum_expr(start, end, step, expr.span),
            HirExpressionKind::Assign(target, value) => self.check_assign(target, value),
            HirExpressionKind::Marked {
                marker,
                expr: inner,
            } => {
                // Value-side `copy` (`b ← copy c`): the value has the type of
                // its operand; a scalar carrier takes no access marker, the
                // same refusal the binding-side form gets at the type.
                let inner_span = inner.span;
                let ty = self.check_expr_with_expected(inner, expected);
                if matches!(marker, radix_hir::HirArgMarker::Copy) && ty != self.error_type {
                    let resolved = self.resolve_inferred_type(ty);
                    if self.types.is_scalar_for_access_marker(resolved) {
                        self.error_issue_with_args(
                            SemanticErrorKind::LoweringError,
                            crate::ISSUE_SCALAR_ACCESS_MARKER,
                            inner_span,
                            &[],
                        );
                    } else {
                        // Ruling 7b25889f R5: a deep duplicate must never
                        // silently share a member with identity.
                        let (_, payload) = self.types.peel_access_marker(resolved);
                        self.refuse_copy_of_nonduplicable(payload, inner_span);
                    }
                }
                ty
            }
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
                params,
            } => self.check_conversio_assign(
                target,
                source,
                recovery.as_deref_mut(),
                params,
                expr.span,
            ),
            HirExpressionKind::Array(elements) => self.check_array(elements, expr.span, expected),
            HirExpressionKind::Struct(def_id, fields) => self.check_struct_literal(*def_id, fields),
            HirExpressionKind::Verte {
                source,
                target,
                entries,
            } => {
                // `_ /pattern/`: the inferred regex hole takes its type from
                // the expected slot and is a named error without one (BUG-01,
                // need). The explicit `regex /pattern/` form stays a
                // self-typed literal and never reaches this hole shape.
                if entries.is_none()
                    && self.is_infer(self.resolve_inferred_type(*target))
                    && matches!(
                        &source.kind,
                        HirExpressionKind::Literal(HirLiteral::Regex(..))
                    )
                    && expected.is_none_or(|ty| self.is_infer(self.resolve_inferred_type(ty)))
                {
                    self.invalid_conversion(
                        super::convert::ISSUE_INFERRED_CONSTRUCTOR_NO_EXPECTED_TYPE,
                        expr.span,
                    );
                    return self.error_type;
                }
                let default_record =
                    if entries.is_some() && self.is_infer(self.resolve_inferred_type(*target)) {
                        match expected.map(|ty| self.resolve_inferred_type(ty)) {
                            Some(ty) if !self.is_infer(ty) => {
                                *target = ty;
                                None
                            }
                            _ => {
                                // Bare `{ field = value }` with no typed cell, or
                                // the explicit `record<*>` field-list hole, builds
                                // a record from its own entries. Keep the result in
                                // the ordinary expression flow below so its type
                                // is written onto the HIR node before MIR lowering.
                                let Some(record_ty) = entries.as_deref_mut().and_then(|fields| {
                                    self.check_default_record_literal(target, fields, expr.span)
                                }) else {
                                    self.invalid_conversion(
                                        super::convert::ISSUE_INFERRED_CONSTRUCTOR_NO_EXPECTED_TYPE,
                                        expr.span,
                                    );
                                    return self.error_type;
                                };
                                Some(record_ty)
                            }
                        }
                    } else {
                        None
                    };
                if let Some(record_ty) = default_record {
                    record_ty
                } else {
                    let checked =
                        self.check_verte(source, *target, entries.as_mut(), expected, expr.span);
                    // WHY: the authored target of `list<_>[1, 2]` keeps the
                    // unsolved hole id while the element checks bind it in the
                    // substitution table. Lowering reads this field, not the
                    // expression type finalization rewrites, so write the solved
                    // canonical shape back. The memoized cache may hold the
                    // pre-solve shape, so drop it first.
                    if self.verte_type_has_inference_hole(*target) {
                        self.canonical_cache.clear();
                        *target = self.canonicalize_type(*target);
                    }
                    checked
                }
            }
            // An implicit store node is the checker's own output; re-checking
            // it re-derives the slot type and never re-runs `↦` admission.
            HirExpressionKind::Conversio {
                source,
                target: HirConversioTarget::Type(slot),
                implicit: Some(_),
                ..
            } => {
                self.check_expr(source);
                *slot
            }
            HirExpressionKind::Conversio {
                route,
                source,
                target,
                params,
                recovery,
                ..
            } => self.check_conversio(
                source,
                target,
                params,
                recovery.as_deref_mut(),
                route,
                expr.span,
            ),
            HirExpressionKind::Tuple(items, type_args) => {
                self.check_tuple(items, type_args.as_ref(), expected)
            }
            HirExpressionKind::Scribe(_, items) => {
                for item in items {
                    self.check_expr(item);
                }
                self.vacuum_type()
            }
            HirExpressionKind::Scriptum(_template, args) => {
                for arg in args {
                    self.check_expr(arg);
                }
                self.textus_type()
            }
            HirExpressionKind::ReadLine(_) => {
                let textus = self.textus_type();
                self.types.option(textus)
            }
            HirExpressionKind::Adfirma(cond, message) => {
                let cond_ty = self.check_expr(cond);
                let bool_ty = self.bool_type();
                self.unify_assign_issue(
                    cond_ty,
                    bool_ty,
                    cond.span,
                    ISSUE_ASSERT_CONDITION_TYPE_MISMATCH,
                );
                if let Some(message) = message {
                    self.check_expr(message);
                }
                self.vacuum_type()
            }
            HirExpressionKind::Panic(value) => {
                self.check_expr(value);
                self.vacuum_type()
            }
            HirExpressionKind::Throw(value) => self.check_throw(value, expr.span),
            HirExpressionKind::Handled { body, catch } => {
                self.check_handled_block(body, catch, expected)
            }
            HirExpressionKind::Trap(body) => self.check_trap(body, expected, expr.span),
            HirExpressionKind::CallDefault {
                body,
                default,
                err_ty,
            } => self.check_call_default(body, default, err_ty, expected, expr.span),
            HirExpressionKind::Clausura(params, ret, err, body) => {
                self.check_closure(params, ret.as_mut(), err.as_mut(), body, expected)
            }
            HirExpressionKind::Cede(inner) => {
                self.check_expr(inner);
                self.error_issue(
                    SemanticErrorKind::LoweringError,
                    ISSUE_INVALID_LOWERED_EXPRESSION,
                    expr.span,
                );
                self.error_type
            }
            HirExpressionKind::Reddet(inner) => {
                // `reddet future` — await a promissum<T> and produce T.
                if !self.current_await_return_context {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_REDDET_REQUIRES_ASYNC_FUNCTION,
                        expr.span,
                    );
                }
                let inner_ty = self.check_expr(inner);
                self.await_success_type(inner_ty, inner.span, ISSUE_CEDE_REQUIRES_PROMISSUM)
            }
            HirExpressionKind::Tacebit(inner) => {
                // `tacebit future` — await to completion, discard success T.
                if !self.current_await_context {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_TACEBIT_REQUIRES_ASYNC_CONTEXT,
                        expr.span,
                    );
                }
                let inner_ty = self.check_expr(inner);
                self.await_success_type(inner_ty, inner.span, ISSUE_CEDE_REQUIRES_PROMISSUM);
                self.vacuum_type()
            }
            HirExpressionKind::Yield(inner) => {
                // `cede value` — yield one item. It is no longer an await form.
                if !self.current_generator_function {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_CEDE_REQUIRES_GENERATOR,
                        inner.span,
                    );
                    return self.error_type;
                }
                // The declared element type is a slot: `cede e` is a store (row 10).
                match self.current_return {
                    Some(element) => {
                        let site = StoreSite::new(HirStoreKind::Yield);
                        let ty = self.check_store_by_type(inner, element, site);
                        self.unify_expected(ty, element, inner.span);
                        ty
                    }
                    None => self.check_expr(inner),
                }
            }
            HirExpressionKind::Ref(kind, inner) => {
                let inner_ty = self.check_expr(inner);
                let mutability = match kind {
                    radix_hir::HirRefKind::Shared => crate::SemanticMutability::Immutable,
                    radix_hir::HirRefKind::Mutable => crate::SemanticMutability::Mutable,
                };
                self.types.reference(mutability, inner_ty)
            }
            HirExpressionKind::Deref(inner) => self.check_deref(inner, expr.span),
            HirExpressionKind::Ad { route, opener } => {
                self.check_ad_expr(*route, opener.as_deref_mut(), expr.span)
            }
            HirExpressionKind::TypeCheck {
                positive: _,
                expr,
                ty,
            } => {
                let operand_ty = self.check_expr(expr);
                self.warn_dead_type_test(operand_ty, *ty, expr.span);
                self.bool_type()
            }
            HirExpressionKind::Widen { operand, to, .. } => {
                // Typecheck inserts this node itself; a re-check keeps it.
                self.check_expr(operand);
                *to
            }
            HirExpressionKind::Praefixum(inner) => self.check_praefixum(inner, expected),
            HirExpressionKind::Insere(path) => {
                super::insere::check_insere(self, *path, expected, expr.span)
            }
            HirExpressionKind::Sort {
                source_mode,
                source,
                binding,
                body,
                ..
            } => super::sort_expr::check_sort_expr(
                self,
                *source_mode,
                source,
                *binding,
                body.as_mut(),
                expr.span,
            ),
            HirExpressionKind::Error => {
                if self.errored_exprs.insert(expr.id) {
                    self.error_issue(
                        SemanticErrorKind::LoweringError,
                        ISSUE_INVALID_LOWERED_EXPRESSION,
                        expr.span,
                    );
                }
                self.error_type
            }
        };

        // ss1a: an imperative twin mutates in place and yields no value, even
        // though its desugared assignment is typed as the target.
        let ty = if in_place_twin && ty != self.error_type {
            self.vacuum_type()
        } else {
            ty
        };
        self.last_synthesized_type = Some(ty);
        let synthesized = ty;
        let ty = match (expected, store) {
            // Ruling 16: an integer quotient cannot land in a float slot; the
            // help points at `÷`, which divides integers into an `f64`.
            (Some(expected), _)
                if matches!(expr.kind, HirExpressionKind::Binary(HirBinOp::Div, ..))
                    && self.is_integer_into_float(ty, expected) =>
            {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_INTEGER_QUOTIENT_TO_FLOAT,
                    expr.span,
                );
                self.error_type
            }
            // Need: an optional value stored into a declared cell
            // that cannot hold `none` is a named error, not a silent unwrap.
            (Some(expected), Some(site))
                if matches!(
                    site.kind,
                    HirStoreKind::Declaration
                        | HirStoreKind::Assignment
                        | HirStoreKind::Argument
                        | HirStoreKind::Return
                        | HirStoreKind::Yield
                        | HirStoreKind::Throw
                        | HirStoreKind::Field
                        | HirStoreKind::Element
                        | HirStoreKind::TuplePosition
                ) && !site.optional_slot
                    && self.reject_optional_into_plain_cell(ty, expected, expr.span) =>
            {
                // The named error stands alone: the node takes the slot's type.
                expected
            }
            (Some(expected), Some(site)) => self.coerce_store(expr, ty, expected, site),
            (Some(expected), None) => self.unify_expected(ty, expected, expr.span),
            (None, _) => ty,
        };
        self.pending_conversion = saved_pending_conversion;
        // inf-wn: a bounded integer that landed in an `inf` slot keeps its own
        // type under an explicit widening node instead of being repainted.
        if let Some(slot) = expected
            && ty != self.error_type
            && let Some(widened) = self.widen_into_inf(expr, synthesized, slot)
        {
            return widened;
        }
        let resolved = self.resolve_inferred_type(ty);
        // `vacua` or a tuple literal landing in an optional / union slot is the
        // slot's single collection / tuple member; the store widens it.
        let resolved = match expr.kind {
            HirExpressionKind::Vacua => self.vacua_collection_member(resolved).unwrap_or(resolved),
            HirExpressionKind::Tuple(..) => self
                .single_slot_member(resolved, |ty| matches!(ty, Type::Tuple(_)))
                .unwrap_or(resolved),
            _ => resolved,
        };
        if let HirExpressionKind::Path(binding) = &expr.kind {
            if self.tensor_edge_policies.shifted_bindings.contains(binding) {
                self.tensor_edge_policies
                    .shifted_expressions
                    .insert(expr.id);
            }
            if let Some(context) = self
                .tensor_edge_policies
                .shifted_view_bindings
                .get(binding)
                .copied()
            {
                self.tensor_edge_policies
                    .shifted_view_expressions
                    .insert(expr.id, context);
            }
        }
        expr.ty = Some(resolved);
        self.record_tensor_edge_policy_runtime_type_use(resolved);
        ty
    }

    /// Typecheck a thrown value against the enclosing error channel. The
    /// throw expression itself is `vacuum`; no channel is a named error.
    fn check_throw(&mut self, value: &mut HirExpression, span: radix_lexer::Span) -> TypeId {
        // A declared `⇥ T` channel is a slot: the thrown value is a
        // store into `T` (row 11). Local and trap sinks accumulate.
        let value_ty = match self.current_error {
            Some(ErrorSink::Function(slot)) => {
                self.check_store_by_type(value, slot, StoreSite::new(HirStoreKind::Throw))
            }
            _ => self.check_expr(value),
        };
        if let Some(err_ty) = self.current_error.map(|sink| match sink {
            ErrorSink::Function(ty) | ErrorSink::Local(ty) => ty,
            // UT: the throw payload accumulates into the trap hole —
            // same funnel, no channel-specific typing.
            ErrorSink::Trap(var) => self.trap_sink_type(var),
        }) {
            self.unify_assign_issue(value_ty, err_ty, value.span, ISSUE_IACE_VALUE_TYPE_MISMATCH);
        } else {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_IACE_REQUIRES_ALTERNATE_EXIT,
                span,
            );
        }
        self.vacuum_type()
    }

    /// Typecheck an `itera` loop: the source and element types, the
    /// `apud`-versus-rank arity law and mode rule, AIR coordinate narrowing,
    /// the binder scope, and the loop body. The loop itself is `vacuum`.
    fn check_itera_expr(
        &mut self,
        mode: &radix_hir::HirIteraMode,
        coordinate_bindings: &[(DefId, Symbol)],
        (binding, binding_name): (DefId, Symbol),
        iter: &mut Box<HirExpression>,
        block: &mut HirBlock,
    ) -> TypeId {
        // An `ordo` name in `itera ex` source position names the enum's own
        // member series, which the ordinary expression checker
        // rejects as a type-kind name. Use the enum type as the source type;
        // every other expression still checks normally.
        let enum_source_ty = if matches!(mode, radix_hir::HirIteraMode::Ab) {
            None
        } else {
            self.enum_iteration_source_type(iter)
        };
        let iter_ty = if matches!(mode, radix_hir::HirIteraMode::Ab) {
            self.check_expr_allowing_direct_intervallum_step(iter)
        } else if let Some(ty) = enum_source_ty {
            iter.ty = Some(ty);
            ty
        } else {
            self.check_expr(iter)
        };
        // A collection field read through an immutable `ref` root
        // is a read-only view of the carrier collection. Use that
        // carrier for iterable-shape dispatch; field projections
        // retain their `Ref` type for access and borrow checks.
        let dispatch_ty = if matches!(mode, radix_hir::HirIteraMode::Ab) {
            iter_ty
        } else {
            self.types
                .peel_access_marker(self.resolve_inferred_type(iter_ty))
                .1
        };
        let has_coordinates = !coordinate_bindings.is_empty();

        // Mode rule: `apud` coordinates require `ex` iteration in v1.
        if has_coordinates && !matches!(mode, radix_hir::HirIteraMode::Ex) {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_ITERA_APUD_REQUIRES_EX,
                iter.span,
            );
        }

        // Arity rule (D3.1): the `apud` names are exactly what goes in
        // the index brackets to get the element back, so their count
        // equals the iterable's rank — tensor shape length, matrix 2,
        // lista 1, tabula 1 or its tuple-key arity — never a partial
        // bind. A rank >= 2 tensor without an `apud` clause is arity
        // 0 != rank. Unindexed iterables (copia, cursors, text, …)
        // reject `apud` fail-closed.
        let numerus = self.numerus_type();
        let mut coordinate_types = vec![numerus; coordinate_bindings.len()];
        if matches!(mode, radix_hir::HirIteraMode::Ex) {
            match itera_apud_coordinates(
                self.types,
                dispatch_ty,
                numerus,
                has_coordinates,
                coordinate_bindings.len(),
            ) {
                IteraApud::Admitted(types) => coordinate_types = types,
                IteraApud::RequiresIndexed => self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_ITERA_APUD_REQUIRES_INDEXED_ITERABLE,
                    iter.span,
                ),
                IteraApud::Mismatch { expected, actual } => self.errors.push(
                    SemanticError::structured(SemanticErrorKind::TypeMismatch, iter.span)
                        .with_arg("issue", ISSUE_ITERA_APUD_ARITY_MISMATCH)
                        .with_arg("expected", expected.to_string())
                        .with_arg("actual", actual.to_string()),
                ),
            }
        }

        if self.current_air_function {
            self.narrow_apud_coordinates(dispatch_ty, &mut coordinate_types);
        } else if self.current_nucleum_entry
            && matches!(
                self.types.get(dispatch_ty),
                Type::Tensor(..) | Type::Matrix(..)
            )
        {
            // Device coordinate cells use the portable signed carrier. Launch
            // requirements still prove that each extent fits that carrier.
            let coordinate = self.width_type(radix_types::NumericWidth::I32);
            self.types.array(coordinate);
            coordinate_types.fill(coordinate);
        }

        let elem_ty = if matches!(mode, radix_hir::HirIteraMode::Ab) {
            if let Some(bound) = self.intervallum_bound_type(iter_ty) {
                self.constant_range_loop_type(iter, bound)
            } else {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_ITERA_AB_SOURCE_REQUIRES_RANGE,
                    iter.span,
                );
                self.numerus_type()
            }
        } else {
            self.itera_element_type(dispatch_ty, *mode, iter.span)
        };
        self.push_scope();
        for ((def_id, _name), ty) in coordinate_bindings.iter().zip(coordinate_types) {
            self.insert_binding(*def_id, ty);
        }
        if matches!(mode, radix_hir::HirIteraMode::Ex) {
            super::tensor_index::record_coordinate_binder_extents(
                self,
                dispatch_ty,
                coordinate_bindings,
            );
        }
        self.insert_binding(binding, elem_ty);
        // Interval facts for the binders, seeded before the loop-entry policy
        // runs so a body that may write a mutable source drops them (need
        // 5d3a8afd, slice 3).
        match mode {
            radix_hir::HirIteraMode::Ab => {
                self.seed_range_loop_binder(binding, binding_name, iter);
            }
            radix_hir::HirIteraMode::Ex if has_coordinates => {
                self.seed_coordinates(dispatch_ty, Some(iter), coordinate_bindings);
            }
            _ => {}
        }
        self.check_loop_block_over(std::iter::once(&**iter), block);
        self.release_range_binders(
            std::iter::once(binding).chain(coordinate_bindings.iter().map(|(def_id, _)| *def_id)),
        );
        self.pop_scope();
        self.vacuum_type()
    }

    /// Enum type of an `itera ex` / `itera de` source that is exactly a bare
    /// `ordo` name, or `None` for every other expression.
    ///
    /// `Color` in `itera ex Color` names the enum's member series, not a
    /// value; the ordinary expression checker would reject the type-kind name
    /// before element dispatch could type the members. The caller uses this to
    /// keep the source type and let [`TypeChecker::itera_element_type`] decide
    /// the mode (members for `ex`, refusal for `de`).
    fn enum_iteration_source_type(&mut self, iter: &HirExpression) -> Option<TypeId> {
        let HirExpressionKind::Path(def_id) = &iter.kind else {
            return None;
        };
        let symbol = self.resolver.get_symbol(*def_id)?;
        (symbol.kind == crate::SymbolKind::Enum).then(|| self.types.intern(Type::Enum(*def_id)))
    }

    /// Unify a synthesized expression type with the expected type.
    pub(super) fn unify_expected(
        &mut self,
        ty: TypeId,
        expected: TypeId,
        span: radix_lexer::Span,
    ) -> TypeId {
        let src_marked = self
            .types
            .peel_access_marker(self.resolve_inferred_type(ty))
            .0
            .is_some();
        let dest_marked = self
            .types
            .peel_access_marker(self.resolve_inferred_type(expected))
            .0
            .is_some();
        if src_marked || dest_marked {
            // Keep the synthesized type so a `ref` Path retains its fact;
            // payload compatibility is checked separately.
            self.unify_carrier_assign(ty, expected, span, ISSUE_EXPRESSION_TYPE_MISMATCH);
            ty
        } else {
            self.unify_assign_issue(ty, expected, span, ISSUE_EXPRESSION_TYPE_MISMATCH)
        }
    }

    /// The single member of an optional or union slot type that `admits`, or
    /// `None` when the slot is not optional/union or has zero or several such
    /// members (several supply none, like the single-numeric-member rule of a
    /// store). A literal that lands in `T ∪ none` is the `T` member; the
    /// surrounding store widens it into the slot (opt-lit).
    fn single_slot_member(&self, slot: TypeId, admits: fn(&Type) -> bool) -> Option<TypeId> {
        let members: Vec<TypeId> = match self.types.get(slot) {
            Type::Option(inner) => vec![*inner],
            Type::Union(members) => members.clone(),
            _ => return None,
        };
        let mut found = members.into_iter().filter_map(|member| {
            let member = self
                .types
                .peel_access_marker(self.resolve_inferred_type(member))
                .1;
            let member = self.single_slot_member(member, admits).unwrap_or(member);
            admits(self.types.get(member)).then_some(member)
        });
        let only = found.next()?;
        found.next().is_none().then_some(only)
    }

    /// The collection member an empty literal (`vacua`) lands in.
    pub(super) fn vacua_collection_member(&self, slot: TypeId) -> Option<TypeId> {
        self.single_slot_member(slot, |ty| {
            matches!(
                ty,
                Type::Array(_)
                    | Type::BoundedArray { .. }
                    | Type::RangedArray { .. }
                    | Type::Queue(_)
                    | Type::BoundedQueue { .. }
                    | Type::Stack(_)
                    | Type::BoundedStack { .. }
                    | Type::Map(_, _)
                    | Type::Ledger(_, _)
                    | Type::Set(_)
                    | Type::Tensor(..)
                    | Type::Sparsa(..)
                    | Type::Vector(..)
                    | Type::Matrix(..)
            )
        })
    }

    /// Checks the empty collection literal.
    ///
    /// `vacua` is deliberately expected-context driven: without an explicit
    /// declared collection type the checker cannot know whether it is a list,
    /// map, or set, and codegen must not guess that shape later.
    fn check_vacua(&mut self, expected: Option<TypeId>, span: radix_lexer::Span) -> TypeId {
        let Some(expected) = expected else {
            self.missing_type_annotation("vacua_requires_declared_type", span);
            return self.error_type;
        };

        let resolved = self
            .types
            .peel_access_marker(self.resolve_inferred_type(expected))
            .1;
        if self.is_infer(resolved) {
            self.missing_type_annotation("vacua_requires_declared_type", span);
            return self.error_type;
        }
        // An optional / union slot (`list<T> ∪ none`) hands the literal its
        // single collection member, as for a list literal (opt-lit).
        let resolved = self.vacua_collection_member(resolved).unwrap_or(resolved);

        // A length range never admits the empty list: its low end is at
        // least one (`list<T, size 0…N>` is the bounded list), so `empty` in such a
        // slot is a statically known length outside the range.
        if matches!(self.types.get(resolved), Type::RangedArray { .. }) {
            return self.check_ranged_literal_length(resolved, 0, span);
        }

        let shape = match self.types.get(resolved) {
            Type::Array(_)
            | Type::BoundedArray { .. }
            | Type::RangedArray { .. }
            | Type::Queue(_)
            | Type::BoundedQueue { .. }
            | Type::Stack(_)
            | Type::BoundedStack { .. }
            | Type::Map(_, _)
            | Type::Ledger(_, _)
            | Type::Set(_) => {
                return resolved;
            }
            Type::Tensor(_, shape) | Type::Sparsa(_, shape) => *shape,
            _ => {
                self.invalid_operand_types("vacua_requires_collection_type", span);
                return self.error_type;
            }
        };
        if self.is_unresolved_index_hole(shape) {
            let rank0 = self.types.rank0_shape();
            self.constrain_index_equal(shape, rank0, span);
        }
        resolved
    }

    /// Synthesizes an expression type without contextual expectations.
    ///
    /// Callers should prefer `check_expr_with_expected` when a declaration,
    /// function parameter, branch join, or enclosing aggregate already supplies
    /// a meaningful target shape.
    pub fn check_expr(&mut self, expr: &mut HirExpression) -> TypeId {
        self.check_expr_with_expected(expr, None)
    }
}
