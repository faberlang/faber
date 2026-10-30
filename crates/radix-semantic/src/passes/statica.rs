//! Module-level constants: compile-time evaluation and the top-level-binding
//! error (DECISIONS D5.7, D5.8, D5.9, D8.4, st1).
//!
//! A module-level constant (`const T NAME = expr`, la `fixum`) is immutable
//! and has no runtime initialization: its initializer must be evaluable at
//! compile time. (The `static`/`generis` declaration is retired: PARSE010
//! `static_decl_retired`; the lowering marker keeps the `statica` name.)
//! This pass evaluates every constant after typecheck and rewrites each
//! foldable scalar subexpression to the literal it evaluates to, so every
//! backend receives the constant through the ordinary top-level-constant
//! path and initialization order can never differ between targets.
//!
//! The evaluable surface is small and closed:
//! - literals;
//! - arithmetic, comparison, bit, and logical operators on `numerus`,
//!   `fractus`, and `bivalens` scalars, plus `textus` concatenation;
//! - references to other constants, evaluated in dependency order (a cycle is
//!   SEM007 `constant_cycle`);
//! - `module.NAME` references to exported constants of an imported file whose
//!   interface carries the folded value (ST2-1); an imported function call or
//!   an exported constant without a folded value stays SEM060;
//! - collection literals (`lista`, tuples, map construction) whose elements
//!   are constants. These stay structural; only their scalar leaves fold.
//!
//! Anything else is SEM060 `constant_initializer_not_constant`. Integer
//! arithmetic is checked (overflow and division by zero are compile errors),
//! matching the runner's exact integer semantics (d11-6 ruling 17a: every
//! intermediate lies in [-2^63, 2^64 - 1], the declared type bounds only the
//! constant itself); `/` floors and `%` takes the divisor's sign.
//! Decimal widths and modular words are not folded, so arithmetic on them
//! is not constant.
//!
//! Folding replaces `HirConst::value`; the authored initializer is kept on
//! [`HirStatic::authored`] so Faber re-emission prints the source, not the
//! folded value.
//!
//! A `praefixum { … }` initializer (D8.4) is not evaluated here: the driver
//! runs it on the MIR runner, substitutes the literal, and calls [`fold`]
//! again so constants that read it fold too.
//!
//! D5.7: a top-level `fixum`/`varia` store (`←`, `↤`, `↢`, `varia`) is SEM062
//! `top_level_binding`: module-level state is a compile-time constant
//! (`const T X = e`) or does not exist.

use crate::file_interface::InterfaceConstValue;
use crate::{Resolver, SemanticError, SemanticErrorKind, SymbolKind, Type, TypeTable};
use handroll::FxHashMap;
use radix_hir::visit::{HirVisitor, walk_expr, walk_program};
use radix_hir::{
    DefId, HirArrayElement, HirBinOp, HirExpression, HirExpressionKind, HirId, HirItemKind,
    HirLiteral, HirModule, HirObjectKey, HirUnOp,
};
use radix_lexer::{Interner, Span, Symbol};
use radix_types::{Primitive, TypeId};

const ISSUE_TOP_LEVEL_BINDING: &str = "top_level_binding";
const ISSUE_NOT_CONSTANT: &str = "constant_initializer_not_constant";
const ISSUE_CYCLE: &str = "constant_cycle";
const ISSUE_OVERFLOW: &str = "constant_arithmetic_overflow";
const ISSUE_DIVISION_BY_ZERO: &str = "constant_division_by_zero";

/// Reject every legacy top-level `fixum`/`varia` binding (D5.7 migration).
#[must_use]
pub fn reject_top_level_bindings(hir: &HirModule, interner: &Interner) -> Vec<SemanticError> {
    hir.items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if konst.statica.is_none() => Some(
                SemanticError::structured(SemanticErrorKind::TopLevelBindingForbidden, item.span)
                    .with_arg("issue", ISSUE_TOP_LEVEL_BINDING)
                    .with_arg("name", interner.resolve(konst.name)),
            ),
            _ => None,
        })
        .collect()
}

/// Evaluate every static, report non-constant initializers and cycles, and
/// rewrite foldable initializers to literals.
///
/// Module-local constants only: a `module.NAME` reference to an imported
/// constant needs the resolver, see [`fold_with_imports`].
pub fn fold(hir: &mut HirModule, types: &TypeTable, interner: &mut Interner) -> Vec<SemanticError> {
    fold_inner(hir, types, interner, None)
}

/// [`fold`], additionally resolving `module.NAME` references to imported
/// constants through the file interfaces installed in `resolver`.
pub fn fold_with_imports(
    hir: &mut HirModule,
    types: &TypeTable,
    interner: &mut Interner,
    resolver: &Resolver,
) -> Vec<SemanticError> {
    fold_inner(hir, types, interner, Some(resolver))
}

/// The value an exported constant's interface carries: the scalar or text its
/// (already folded) initializer evaluates to, conformed to the declared type.
/// `None` for anything that does not fold to a scalar or text.
#[must_use]
pub fn export_value(
    value: &HirExpression,
    declared: Option<TypeId>,
    types: &TypeTable,
    interner: &Interner,
) -> Option<InterfaceConstValue> {
    let mut evaluator = Evaluator {
        types,
        imports: None,
        inits: FxHashMap::default(),
        state: FxHashMap::default(),
    };
    let folded = evaluator
        .eval(value, interner)
        .and_then(|folded| evaluator.fit_slot(folded, declared, value.span))
        .ok()?;
    match folded {
        Value::Opaque => export_list(&mut evaluator, value, interner),
        scalar => scalar_value(scalar),
    }
}

fn scalar_value(value: Value) -> Option<InterfaceConstValue> {
    match value {
        Value::Int(v) => Some(InterfaceConstValue::Int(v)),
        Value::Float(v) => Some(InterfaceConstValue::Float(v)),
        Value::Bool(b) => Some(InterfaceConstValue::Bool(b)),
        Value::Text(text) => Some(InterfaceConstValue::Text(text)),
        Value::Opaque => None,
    }
}

/// A list literal whose every element folds to a scalar or text, as the
/// interface value of a list constant. Spreads, holes and non-scalar elements
/// leave the constant value-less.
fn export_list(
    evaluator: &mut Evaluator<'_>,
    value: &HirExpression,
    interner: &Interner,
) -> Option<InterfaceConstValue> {
    let HirExpressionKind::Array(elements) = &value.kind else {
        return None;
    };
    let mut items = Vec::with_capacity(elements.len());
    for element in elements {
        let HirArrayElement::Expr(inner) = element else {
            return None;
        };
        let folded = evaluator
            .eval(inner, interner)
            .and_then(|folded| evaluator.fit_slot(folded, inner.ty, inner.span))
            .ok()?;
        items.push(scalar_value(folded)?);
    }
    Some(InterfaceConstValue::List(items))
}

fn fold_inner(
    hir: &mut HirModule,
    types: &TypeTable,
    interner: &mut Interner,
    imports: Option<&Resolver>,
) -> Vec<SemanticError> {
    let mut inits = FxHashMap::default();
    let mut order = Vec::new();
    for item in &hir.items {
        if let HirItemKind::Constant(konst) = &item.kind
            && konst.statica.is_some()
        {
            inits.insert(item.def_id, (konst.name, konst.value.clone(), konst.ty));
            order.push(item.def_id);
        }
    }
    if order.is_empty() {
        return Vec::new();
    }

    let mut evaluator = Evaluator {
        types,
        imports,
        inits,
        state: FxHashMap::default(),
    };
    let mut errors = Vec::new();
    for def_id in &order {
        if let Err(fault) = evaluator.eval_static(*def_id, interner)
            && fault.issue != ISSUE_DEPENDENCY
            && fault.issue != ISSUE_DEFERRED
        {
            let name = evaluator.inits[def_id].0;
            let kind = if fault.issue == ISSUE_CYCLE {
                SemanticErrorKind::CircularDependency
            } else {
                SemanticErrorKind::StaticInitializerInvalid
            };
            errors.push(
                SemanticError::structured(kind, fault.span)
                    .with_arg("issue", fault.issue)
                    .with_arg("name", interner.resolve(name)),
            );
        }
    }
    if !errors.is_empty() {
        return errors;
    }

    let state = std::mem::take(&mut evaluator.state);
    let mut rewriter = Rewriter {
        evaluator: Evaluator {
            types,
            imports,
            inits: FxHashMap::default(),
            state,
        },
        next_id: max_hir_id(hir) + 1,
    };
    for item in &mut hir.items {
        let HirItemKind::Constant(konst) = &mut item.kind else {
            continue;
        };
        let Some(statica) = konst.statica.as_mut() else {
            continue;
        };
        let authored = konst.value.clone();
        if rewriter.rewrite(&mut konst.value, interner) {
            statica.authored = Some(Box::new(authored));
        }
    }
    Vec::new()
}

// EVALUATION

/// Marker issue: a static failed because a static it references failed.
/// The referenced static carries the diagnostic; this one stays quiet.
const ISSUE_DEPENDENCY: &str = "constant_dependency";

/// Marker issue: a `praefixum { … }` initializer (D8.5). The driver evaluates
/// it on the MIR runner after analysis, then folds again; statics that
/// depend on it stay quiet until then.
const ISSUE_DEFERRED: &str = "constant_deferred";

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Int(i128),
    Float(f64),
    Bool(bool),
    Text(String),
    /// A constant with no scalar fold: a collection literal of constants,
    /// `nihil`, a JSON/regex/ascii/octeti literal, or a decimal-width value.
    Opaque,
}

#[derive(Debug, Clone, Copy)]
struct Fault {
    issue: &'static str,
    span: Span,
}

impl Fault {
    fn at(issue: &'static str, span: Span) -> Self {
        Self { issue, span }
    }
}

enum State {
    InProgress,
    /// Evaluated; a failure keeps the static's own fault so it is reported
    /// even when a dependent evaluated it first.
    Done(Result<Value, Fault>),
}

/// The scalar class a node's type folds as, if any.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Class {
    Int(i128, i128),
    Float,
    Bool,
    Text,
}

struct Evaluator<'a> {
    types: &'a TypeTable,
    /// Resolver whose installed file interfaces supply imported constants.
    imports: Option<&'a Resolver>,
    inits: FxHashMap<DefId, (Symbol, HirExpression, Option<TypeId>)>,
    state: FxHashMap<DefId, State>,
}

impl Evaluator<'_> {
    fn eval_static(&mut self, def_id: DefId, interner: &Interner) -> Result<Value, Fault> {
        let init = match self.state.get(&def_id) {
            Some(State::Done(Ok(value))) => return Ok(value.clone()),
            Some(State::Done(Err(fault))) => return Err(*fault),
            Some(State::InProgress) => return Err(Fault::at(ISSUE_CYCLE, Span::new(0, 0))),
            None => self.inits[&def_id].1.clone(),
        };
        let declared = self.inits.get(&def_id).and_then(|entry| entry.2);
        self.state.insert(def_id, State::InProgress);
        let result = self
            .eval(&init, interner)
            .and_then(|value| self.fit_slot(value, declared, init.span));
        self.state.insert(def_id, State::Done(result.clone()));
        result
    }

    /// The static's own store: the exact value must fit the declared type.
    /// Intermediates are only bound by the 64-bit range (ruling 17a).
    fn fit_slot(&self, value: Value, declared: Option<TypeId>, span: Span) -> Result<Value, Fault> {
        match (&value, self.class(declared)) {
            (Value::Int(v), Some(Class::Int(lo, hi))) if *v < lo || *v > hi => {
                Err(Fault::at(ISSUE_OVERFLOW, span))
            }
            _ => Ok(value),
        }
    }

    fn class(&self, ty: Option<TypeId>) -> Option<Class> {
        let mut ty = ty?;
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                Type::Primitive(Primitive::Numerus) => {
                    return Some(Class::Int(i128::from(i64::MIN), i128::from(i64::MAX)));
                }
                Type::SizedNumeric(Primitive::Numerus, width) => {
                    // Deliberate: `inf` has no range and no constant class yet.
                    let (lo, hi) = width.integer_range()?;
                    return Some(Class::Int(lo, hi));
                }
                Type::Primitive(Primitive::Fractus) => return Some(Class::Float),
                Type::SizedNumeric(Primitive::Fractus, width) if width.is_float() => {
                    return Some(Class::Float);
                }
                Type::Primitive(Primitive::Bivalens) => return Some(Class::Bool),
                Type::Primitive(Primitive::Textus) => return Some(Class::Text),
                _ => return None,
            }
        }
    }

    fn eval(&mut self, expr: &HirExpression, interner: &Interner) -> Result<Value, Fault> {
        let value = match &expr.kind {
            HirExpressionKind::Literal(literal) => self.eval_literal(expr, literal, interner),
            HirExpressionKind::Path(def_id) => {
                if !self.state.contains_key(def_id) && !self.inits.contains_key(def_id) {
                    return Err(Fault::at(ISSUE_NOT_CONSTANT, expr.span));
                }
                self.eval_static(*def_id, interner)
                    .map_err(|fault| match fault.issue {
                        ISSUE_CYCLE => Fault::at(ISSUE_CYCLE, expr.span),
                        _ => Fault::at(ISSUE_DEPENDENCY, expr.span),
                    })?
            }
            HirExpressionKind::Field(object, member) => self
                .imported_value(object, *member)
                .ok_or(Fault::at(ISSUE_NOT_CONSTANT, expr.span))?,
            HirExpressionKind::Praefixum(_) => return Err(Fault::at(ISSUE_DEFERRED, expr.span)),
            // An implicit store node (a narrowing into the static's slot):
            // the exact source value must fit the slot's type.
            HirExpressionKind::Conversio {
                source,
                target: radix_hir::HirConversioTarget::Type(target),
                implicit: Some(_),
                ..
            } => {
                let value = self.eval(source, interner)?;
                self.fit_slot(value, Some(*target), expr.span)?
            }
            // inf-wn: the widening is lossless; the `inf` node has no constant
            // class, so `fit` leaves the exact operand value opaque.
            HirExpressionKind::Widen { operand, .. } => self.eval(operand, interner)?,
            HirExpressionKind::Unary(op, operand) => {
                let operand = self.eval(operand, interner)?;
                unary(*op, operand).ok_or(Fault::at(ISSUE_NOT_CONSTANT, expr.span))?
            }
            HirExpressionKind::Binary(op, lhs, rhs) => {
                let lhs = self.eval(lhs, interner)?;
                let rhs = self.eval(rhs, interner)?;
                binary(*op, lhs, rhs, expr.span)?
            }
            HirExpressionKind::Array(elements) => {
                for element in elements {
                    match element {
                        HirArrayElement::Expr(inner) => {
                            self.eval(inner, interner)?;
                        }
                        HirArrayElement::Spread(inner) => {
                            return Err(Fault::at(ISSUE_NOT_CONSTANT, inner.span));
                        }
                        HirArrayElement::Infer(span) => {
                            return Err(Fault::at(ISSUE_NOT_CONSTANT, *span));
                        }
                    }
                }
                Value::Opaque
            }
            HirExpressionKind::Tuple(elements, ..) => {
                for inner in elements {
                    self.eval(inner, interner)?;
                }
                Value::Opaque
            }
            HirExpressionKind::Verte {
                source, entries, ..
            } => {
                self.eval(source, interner)?;
                for field in entries.iter().flatten() {
                    match (&field.key, &field.value) {
                        (HirObjectKey::Ident(_) | HirObjectKey::String(_), Some(value)) => {
                            self.eval(value, interner)?;
                        }
                        _ => return Err(Fault::at(ISSUE_NOT_CONSTANT, expr.span)),
                    }
                }
                Value::Opaque
            }
            _ => return Err(Fault::at(ISSUE_NOT_CONSTANT, expr.span)),
        };
        self.fit(value, expr)
    }

    /// The folded value of `module.member` when `module` is an imported file
    /// namespace whose interface carries one for that constant.
    fn imported_value(&self, object: &HirExpression, member: Symbol) -> Option<Value> {
        let resolver = self.imports?;
        let HirExpressionKind::Path(module) = &object.kind else {
            return None;
        };
        let namespace = resolver.get_symbol(*module)?;
        if namespace.kind != SymbolKind::Module {
            return None;
        }
        Some(
            match resolver.namespace_file_const_value_canonical(namespace.name, member)? {
                InterfaceConstValue::Int(v) => Value::Int(*v),
                InterfaceConstValue::Float(v) => Value::Float(*v),
                InterfaceConstValue::Bool(b) => Value::Bool(*b),
                InterfaceConstValue::Text(text) => Value::Text(text.clone()),
                InterfaceConstValue::List(_) => Value::Opaque,
            },
        )
    }

    fn eval_literal(
        &self,
        expr: &HirExpression,
        literal: &HirLiteral,
        interner: &Interner,
    ) -> Value {
        let class = self.class(expr.ty);
        match literal {
            HirLiteral::Int(v) => match class {
                Some(Class::Float) => Value::Float(radix_types::Magnus::from_u64(*v).to_f64()),
                Some(Class::Int(..)) => Value::Int(i128::from(*v)),
                _ => Value::Opaque,
            },
            HirLiteral::Float(v) | HirLiteral::FloatText { value: v, .. } => match class {
                Some(Class::Float) => Value::Float(*v),
                _ => Value::Opaque,
            },
            HirLiteral::Bool(b) => Value::Bool(*b),
            HirLiteral::String(sym) => Value::Text(interner.resolve(*sym).to_owned()),
            _ => Value::Opaque,
        }
    }

    /// Conform a computed value to the node's own type: widen integers
    /// to floats where the node is `fractus`, and range-check integers.
    fn fit(&self, value: Value, expr: &HirExpression) -> Result<Value, Fault> {
        match (value, self.class(expr.ty)) {
            (Value::Int(v), Some(Class::Int(..))) => {
                // d11-6 ruling 17a: an intermediate is exact and lies in the
                // single 64-bit range whatever its node type says; the
                // static's declared type is checked by `fit_slot`.
                if v < i128::from(i64::MIN) || v > i128::from(u64::MAX) {
                    Err(Fault::at(ISSUE_OVERFLOW, expr.span))
                } else {
                    Ok(Value::Int(v))
                }
            }
            (Value::Int(v), Some(Class::Float)) => {
                Ok(Value::Float(radix_types::Magnus::from_i128(v).to_f64()))
            }
            (Value::Float(v), Some(Class::Float)) if !v.is_finite() => {
                Err(Fault::at(ISSUE_OVERFLOW, expr.span))
            }
            // A numeric value on a node whose type does not fold (a decimal
            // width, a modular word) is not evaluated further.
            (Value::Int(_) | Value::Float(_), None) => Ok(Value::Opaque),
            (value, _) => Ok(value),
        }
    }
}

fn unary(op: HirUnOp, value: Value) -> Option<Value> {
    match (op, value) {
        (HirUnOp::Neg, Value::Int(v)) => Some(Value::Int(-v)),
        (HirUnOp::Neg, Value::Float(v)) => Some(Value::Float(-v)),
        (HirUnOp::Not, Value::Bool(b)) => Some(Value::Bool(!b)),
        // `¬x` is `-x - 1` (ruling 9), which is `!x` on two's complement.
        (HirUnOp::BitNot, Value::Int(v)) => Some(Value::Int(!v)),
        _ => None,
    }
}

fn binary(op: HirBinOp, lhs: Value, rhs: Value, span: Span) -> Result<Value, Fault> {
    let not_constant = Fault::at(ISSUE_NOT_CONSTANT, span);
    match (lhs, rhs) {
        (Value::Int(a), Value::Int(b)) => int_binary(op, a, b, span),
        (Value::Float(a), Value::Float(b)) => float_binary(op, a, b).ok_or(not_constant),
        (Value::Int(a), Value::Float(b)) => {
            float_binary(op, radix_types::Magnus::from_i128(a).to_f64(), b).ok_or(not_constant)
        }
        (Value::Float(a), Value::Int(b)) => {
            float_binary(op, a, radix_types::Magnus::from_i128(b).to_f64()).ok_or(not_constant)
        }
        (Value::Bool(a), Value::Bool(b)) => match op {
            HirBinOp::And => Ok(Value::Bool(a && b)),
            HirBinOp::Or => Ok(Value::Bool(a || b)),
            HirBinOp::Eq | HirBinOp::StrictEq => Ok(Value::Bool(a == b)),
            HirBinOp::NotEq | HirBinOp::StrictNotEq => Ok(Value::Bool(a != b)),
            _ => Err(not_constant),
        },
        (Value::Text(a), Value::Text(b)) => match op {
            HirBinOp::Add => Ok(Value::Text(a + &b)),
            HirBinOp::Eq | HirBinOp::StrictEq => Ok(Value::Bool(a == b)),
            HirBinOp::NotEq | HirBinOp::StrictNotEq => Ok(Value::Bool(a != b)),
            _ => Err(not_constant),
        },
        _ => Err(not_constant),
    }
}

fn floor_div(a: i128, b: i128) -> Option<i128> {
    let quotient = a.checked_div(b)?;
    Some(if a % b != 0 && ((a < 0) != (b < 0)) {
        quotient - 1
    } else {
        quotient
    })
}

fn int_binary(op: HirBinOp, a: i128, b: i128, span: Span) -> Result<Value, Fault> {
    let overflow = Fault::at(ISSUE_OVERFLOW, span);
    // Floor division (ruling 14): `%` takes the divisor's sign.
    let divide = |remainder: bool| {
        if b == 0 {
            return Err(Fault::at(ISSUE_DIVISION_BY_ZERO, span));
        }
        let quotient = floor_div(a, b).ok_or(overflow)?;
        Ok(Value::Int(if remainder {
            a - b * quotient
        } else {
            quotient
        }))
    };
    // Shifts are pure math (ruling 10): `a * 2^n` and `floor(a / 2^n)`, with
    // no count masking; a negative count is an error.
    let shift_left = || {
        // The count is validated before the operand, as the runner does
        // (exact_int.rs): `0 ⇐ (0 - 1)` is an error, not 0, reported as
        // `constant_arithmetic_overflow` (there is no separate negative-shift issue).
        if b < 0 {
            return Err(overflow);
        }
        if a == 0 {
            return Ok(Value::Int(0));
        }
        u32::try_from(b)
            .ok()
            .filter(|n| *n <= 64)
            .and_then(|n| a.checked_mul(1_i128 << n))
            .map(Value::Int)
            .ok_or(overflow)
    };
    let shift_right = || match u32::try_from(b) {
        Ok(n) => Ok(Value::Int(if n >= 127 {
            if a < 0 { -1 } else { 0 }
        } else {
            a >> n
        })),
        Err(_) if b > 0 => Ok(Value::Int(if a < 0 { -1 } else { 0 })),
        Err(_) => Err(overflow),
    };
    match op {
        HirBinOp::Add => a.checked_add(b).map(Value::Int).ok_or(overflow),
        HirBinOp::Sub => a.checked_sub(b).map(Value::Int).ok_or(overflow),
        HirBinOp::Mul => a.checked_mul(b).map(Value::Int).ok_or(overflow),
        HirBinOp::Div => divide(false),
        HirBinOp::Mod => divide(true),
        HirBinOp::BitAnd => Ok(Value::Int(a & b)),
        HirBinOp::BitOr => Ok(Value::Int(a | b)),
        HirBinOp::BitXor => Ok(Value::Int(a ^ b)),
        HirBinOp::Shl => shift_left(),
        HirBinOp::Shr => shift_right(),
        HirBinOp::Eq | HirBinOp::StrictEq => Ok(Value::Bool(a == b)),
        HirBinOp::NotEq | HirBinOp::StrictNotEq => Ok(Value::Bool(a != b)),
        HirBinOp::Lt => Ok(Value::Bool(a < b)),
        HirBinOp::Gt => Ok(Value::Bool(a > b)),
        HirBinOp::LtEq => Ok(Value::Bool(a <= b)),
        HirBinOp::GtEq => Ok(Value::Bool(a >= b)),
        _ => Err(Fault::at(ISSUE_NOT_CONSTANT, span)),
    }
}

fn float_binary(op: HirBinOp, a: f64, b: f64) -> Option<Value> {
    Some(match op {
        HirBinOp::Add => Value::Float(a + b),
        HirBinOp::Sub => Value::Float(a - b),
        HirBinOp::Mul => Value::Float(a * b),
        HirBinOp::Div => Value::Float(a / b),
        HirBinOp::Lt => Value::Bool(a < b),
        HirBinOp::Gt => Value::Bool(a > b),
        HirBinOp::LtEq => Value::Bool(a <= b),
        HirBinOp::GtEq => Value::Bool(a >= b),
        _ => return None,
    })
}

// REWRITE

struct Rewriter<'a> {
    evaluator: Evaluator<'a>,
    next_id: u32,
}

impl Rewriter<'_> {
    /// Rewrite `expr` in place; returns whether anything changed.
    fn rewrite(&mut self, expr: &mut HirExpression, interner: &mut Interner) -> bool {
        let Ok(value) = self.evaluator.eval(expr, interner) else {
            return false;
        };
        if value != Value::Opaque {
            // `i64::MIN` has no literal form the backends accept (its
            // magnitude is above `i64::MAX`), so its expression stays.
            if is_literal_form(expr) || value == Value::Int(i128::from(i64::MIN)) {
                return false;
            }
            *expr = self.materialize(value, expr, interner);
            return true;
        }
        let mut changed = false;
        match &mut expr.kind {
            HirExpressionKind::Array(elements) => {
                for element in elements {
                    if let HirArrayElement::Expr(inner) = element {
                        changed |= self.rewrite(inner, interner);
                    }
                }
            }
            HirExpressionKind::Tuple(elements, ..) => {
                for inner in elements {
                    changed |= self.rewrite(inner, interner);
                }
            }
            HirExpressionKind::Verte {
                source, entries, ..
            } => {
                changed |= self.rewrite(source, interner);
                for field in entries.iter_mut().flatten() {
                    if let Some(value) = &mut field.value {
                        changed |= self.rewrite(value, interner);
                    }
                }
            }
            _ => {}
        }
        changed
    }

    fn fresh_id(&mut self) -> HirId {
        let id = HirId(self.next_id);
        self.next_id += 1;
        id
    }

    fn materialize(
        &mut self,
        value: Value,
        original: &HirExpression,
        interner: &mut Interner,
    ) -> HirExpression {
        let literal = |kind: HirLiteral, id: HirId| HirExpression {
            id,
            kind: HirExpressionKind::Literal(kind),
            ty: original.ty,
            span: original.span,
        };
        let negated = |inner: HirExpression| HirExpression {
            id: original.id,
            kind: HirExpressionKind::Unary(HirUnOp::Neg, Box::new(inner)),
            ty: original.ty,
            span: original.span,
        };
        match value {
            Value::Int(v) => {
                let magnitude = u64::try_from(v.unsigned_abs()).unwrap_or(u64::MAX);
                if v < 0 {
                    let inner = literal(HirLiteral::Int(magnitude), self.fresh_id());
                    negated(inner)
                } else {
                    literal(HirLiteral::Int(magnitude), original.id)
                }
            }
            Value::Float(v) => {
                if v.is_sign_negative() {
                    let inner = literal(HirLiteral::Float(-v), self.fresh_id());
                    negated(inner)
                } else {
                    literal(HirLiteral::Float(v), original.id)
                }
            }
            Value::Bool(b) => literal(HirLiteral::Bool(b), original.id),
            Value::Text(text) => literal(HirLiteral::String(interner.intern(&text)), original.id),
            Value::Opaque => original.clone(),
        }
    }
}

/// Whether `expr` is already the literal form a fold would produce.
fn is_literal_form(expr: &HirExpression) -> bool {
    match &expr.kind {
        HirExpressionKind::Literal(_) => true,
        HirExpressionKind::Unary(HirUnOp::Neg, inner) => matches!(
            inner.kind,
            HirExpressionKind::Literal(HirLiteral::Int(_) | HirLiteral::Float(_))
        ),
        _ => false,
    }
}

/// The largest non-companion definition id bound anywhere in the module.
pub fn max_def_id(hir: &HirModule) -> u32 {
    struct MaxDef(u32);
    impl HirVisitor for MaxDef {
        fn visit_def(&mut self, def_id: radix_hir::DefId, _name: radix_lexer::Symbol) {
            if !def_id.is_companion() {
                self.0 = self.0.max(def_id.0);
            }
        }
    }
    let mut max = MaxDef(0);
    walk_program(&mut max, hir);
    max.0
}

pub fn max_hir_id(hir: &HirModule) -> u32 {
    struct MaxId(u32);
    impl HirVisitor for MaxId {
        fn visit_item(&mut self, item: &radix_hir::HirItem) {
            self.0 = self.0.max(item.id.0);
            radix_hir::visit::walk_item(self, item);
        }
        fn visit_expr(&mut self, expr: &HirExpression) {
            self.0 = self.0.max(expr.id.0);
            walk_expr(self, expr);
        }
        fn visit_stmt(&mut self, stmt: &radix_hir::HirStatement) {
            self.0 = self.0.max(stmt.id.0);
            radix_hir::visit::walk_stmt(self, stmt);
        }
    }
    let mut max = MaxId(0);
    walk_program(&mut max, hir);
    max.0
}
