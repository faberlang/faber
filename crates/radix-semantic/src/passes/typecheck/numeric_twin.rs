//! Numeric method twins are their operators (d11-6 rulings 17, 21; ss1).
//!
//! `x.added(y)` means `x + y` and `x.add(y)` means `x ← x + y`. The twins are
//! rewritten here, before the call is checked, into the ordinary HIR node the
//! operator or assignment would have produced. Every later stage then types
//! them by the range rule, applies the slot policy at the store, and lowers
//! them like any other operator; no stage keeps a second arithmetic path for
//! the method spellings.
//!
//! Scope: scalar `numerus` / `fractus` receivers and `wrapping` words (the
//! receiver families the registry's `Numeric` rows serve). Tensor, vector and
//! matrix receivers keep their own methods.

use radix_hir::{
    DefId, HirBinOp, HirBlock, HirExpression, HirExpressionKind, HirLocal, HirStatement,
    HirStatementKind, HirUnOp,
    visit::{HirVisitor, walk_expr},
};
use radix_intrinsics::{IntrinsicReceiver, lookup_method_for_type};
use radix_types::Type;

use super::TypeChecker;

fn place_temporary_name(ordinal: usize) -> String {
    format!("__place{ordinal}")
}

/// Finds the widest place an in-place method call could pin to temporaries.
struct PlaceWidth(usize);

impl HirVisitor for PlaceWidth {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::MethodCall(receiver, ..) = &expr.kind
            && is_place_chain(receiver)
        {
            self.0 = self.0.max(impure_parts(receiver));
        }
        walk_expr(self, expr);
    }
}

/// Intern the names the place temporaries use, before typecheck (which holds
/// the interner read-only). Each temporary of one place needs its own name, so
/// intern as many as the widest place in the module can need (an upper bound:
/// every method-call receiver that is a place chain is counted).
pub(crate) fn intern_place_temporaries(
    hir: &radix_hir::HirModule,
    interner: &mut radix_lexer::Interner,
) {
    let mut width = PlaceWidth(0);
    width.visit_program(hir);
    for ordinal in 0..width.0 {
        interner.intern(&place_temporary_name(ordinal));
    }
    // The coordinate names `t.sum(axis n)` binds when it expands to the
    // grouped reduction.
    for name in super::reduction::AXIS_REDUCTION_COORDINATES {
        interner.intern(name);
    }
}

/// What a numeric twin method means.
#[derive(Clone, Copy)]
enum Twin {
    /// `x.added(y)` is `x + y`.
    Binary(HirBinOp),
    /// `x.negated()` is `-x`.
    Unary(HirUnOp),
    /// `x.add(y)` is `x ← x + y`.
    InPlaceBinary(HirBinOp),
    /// `x.negate()` is `x ← -x`.
    InPlaceUnary(HirUnOp),
}

/// Canonical binary twins (`x.added(y)` is `x + y`).
const BINARY_TWINS: [(&str, HirBinOp); 14] = [
    ("addita", HirBinOp::Add),
    ("subtracta", HirBinOp::Sub),
    ("multiplicata", HirBinOp::Mul),
    ("divisa", HirBinOp::Div),
    ("modulata", HirBinOp::Mod),
    ("coniuncta", HirBinOp::BitAnd),
    ("disiuncta", HirBinOp::BitOr),
    ("alternata", HirBinOp::BitXor),
    ("sinistrata", HirBinOp::Shl),
    ("dextrata", HirBinOp::Shr),
    ("minor", HirBinOp::Lt),
    ("maior", HirBinOp::Gt),
    ("minime", HirBinOp::LtEq),
    ("maxime", HirBinOp::GtEq),
];

/// Canonical unary twins (`x.negated()` is `-x`).
const UNARY_TWINS: [(&str, HirUnOp); 2] = [
    ("negativa", HirUnOp::Neg),
    ("complementata", HirUnOp::BitNot),
];

/// Canonical in-place binary twins (`x.add(y)` is `x ← x + y`).
const IN_PLACE_BINARY_TWINS: [(&str, HirBinOp); 10] = [
    ("adde", HirBinOp::Add),
    ("subtrahe", HirBinOp::Sub),
    ("multiplica", HirBinOp::Mul),
    ("divida", HirBinOp::Div),
    ("modula", HirBinOp::Mod),
    ("coniunge", HirBinOp::BitAnd),
    ("disiunge", HirBinOp::BitOr),
    ("alterna", HirBinOp::BitXor),
    ("sinistra", HirBinOp::Shl),
    ("dextra", HirBinOp::Shr),
];

/// Canonical in-place unary twins (`x.negate()` is `x ← -x`).
const IN_PLACE_UNARY_TWINS: [(&str, HirUnOp); 2] =
    [("nega", HirUnOp::Neg), ("complementa", HirUnOp::BitNot)];

/// The canonical binary twin method whose desugaring is `x op y`; the inverse
/// of the rewrite in [`desugar_numeric_twin`], used by Faber re-emission to
/// restore the authored method spelling.
#[must_use]
pub fn binary_twin_name(op: HirBinOp) -> Option<&'static str> {
    BINARY_TWINS
        .iter()
        .find_map(|(name, twin_op)| (*twin_op == op).then_some(*name))
}

/// The canonical unary twin method whose desugaring is `op x`.
#[must_use]
pub fn unary_twin_name(op: HirUnOp) -> Option<&'static str> {
    UNARY_TWINS
        .iter()
        .find_map(|(name, twin_op)| (*twin_op == op).then_some(*name))
}

/// The canonical in-place twin method whose desugaring is `x ← x op y`; the
/// inverse of the rewrite in [`desugar_numeric_twin`], used by Faber
/// re-emission to restore the authored method spelling.
#[must_use]
pub fn in_place_binary_twin_name(op: HirBinOp) -> Option<&'static str> {
    IN_PLACE_BINARY_TWINS
        .iter()
        .find_map(|(name, twin_op)| (*twin_op == op).then_some(*name))
}

/// The canonical in-place twin method whose desugaring is `x ← op x`.
#[must_use]
pub fn in_place_unary_twin_name(op: HirUnOp) -> Option<&'static str> {
    IN_PLACE_UNARY_TWINS
        .iter()
        .find_map(|(name, twin_op)| (*twin_op == op).then_some(*name))
}

/// The twin a canonical method name denotes, by argument count.
fn twin_of(canonical: &str, arity: usize) -> Option<Twin> {
    let binary = |op| (arity == 1).then_some(op);
    let unary = |op| (arity == 0).then_some(op);
    if let Some((_, op)) = BINARY_TWINS.iter().find(|(name, _)| *name == canonical) {
        return binary(*op).map(Twin::Binary);
    }
    if let Some((_, op)) = UNARY_TWINS.iter().find(|(name, _)| *name == canonical) {
        return unary(*op).map(Twin::Unary);
    }
    IN_PLACE_BINARY_TWINS
        .iter()
        .find(|(name, _)| *name == canonical)
        .and_then(|(_, op)| binary(*op).map(Twin::InPlaceBinary))
        .or_else(|| {
            IN_PLACE_UNARY_TWINS
                .iter()
                .find(|(name, _)| *name == canonical)
                .and_then(|(_, op)| unary(*op).map(Twin::InPlaceUnary))
        })
}

fn is_bitwise(op: HirBinOp) -> bool {
    matches!(
        op,
        HirBinOp::BitAnd | HirBinOp::BitOr | HirBinOp::BitXor | HirBinOp::Shl | HirBinOp::Shr
    )
}

/// A place whose evaluation has no effect, so reading it and writing it may
/// evaluate it twice.
fn is_pure_place(expr: &HirExpression) -> bool {
    match &expr.kind {
        HirExpressionKind::Path(_) | HirExpressionKind::Literal(_) => true,
        HirExpressionKind::Field(inner, _)
        | HirExpressionKind::Deref(inner)
        | HirExpressionKind::Unary(_, inner) => is_pure_place(inner),
        HirExpressionKind::Index(base, index) => is_pure_place(base) && is_pure_place(index),
        HirExpressionKind::Binary(_, lhs, rhs) => is_pure_place(lhs) && is_pure_place(rhs),
        _ => false,
    }
}

/// How many sub-expressions of a place chain carry effects (and so need a
/// temporary): impure indices and impure non-place bases.
fn impure_parts(place: &HirExpression) -> usize {
    let base_parts = |base: &HirExpression| {
        if is_place_chain(base) {
            impure_parts(base)
        } else {
            usize::from(!is_pure_place(base))
        }
    };
    match &place.kind {
        HirExpressionKind::Field(base, _) | HirExpressionKind::Deref(base) => base_parts(base),
        HirExpressionKind::Index(base, index) => {
            base_parts(base) + usize::from(!is_pure_place(index))
        }
        _ => 0,
    }
}

/// A path, field, deref or index chain: an expression that can be stored to.
fn is_place_chain(expr: &HirExpression) -> bool {
    matches!(
        expr.kind,
        HirExpressionKind::Path(_)
            | HirExpressionKind::Field(..)
            | HirExpressionKind::Deref(_)
            | HirExpressionKind::Index(..)
    )
}

impl TypeChecker<'_> {
    /// The canonical Latin twin spelling of a method name under the active
    /// reader pack, or `None` when the spelling is not a declared surface.
    fn canonical_twin_name(&self, surface: &str) -> Option<String> {
        match self.locale_pack {
            Some(pack) if pack.metadata.id != "la" => pack
                .intrinsic_canonical(IntrinsicReceiver::Numeric, surface)
                .or_else(|| pack.intrinsic_canonical(IntrinsicReceiver::Fractus, surface))
                .map(str::to_owned),
            _ => Some(surface.to_owned()),
        }
    }

    /// Rewrite a numeric twin method call into the operator or assignment it
    /// means. Leaves every other call (and every rejected twin, which the
    /// intrinsic checker then reports) untouched. Returns whether the call was
    /// an in-place twin: those yield no value (`vacuum`), whatever the
    /// desugared assignment is typed as.
    pub(super) fn desugar_numeric_twin(&mut self, expr: &mut HirExpression) -> bool {
        let Some(twin) = self.numeric_twin_of(expr) else {
            return false;
        };
        let span = expr.span;
        let kind = std::mem::replace(&mut expr.kind, HirExpressionKind::Vacua);
        let HirExpressionKind::MethodCall(mut receiver, _, _, mut args, _) = kind else {
            // `numeric_twin_of` only accepts method calls; restore and decline.
            expr.kind = kind;
            return false;
        };
        let mut argument = || Box::new(args.remove(0).expr);
        expr.kind = match twin {
            Twin::Binary(op) => HirExpressionKind::Binary(op, receiver, argument()),
            Twin::Unary(op) => HirExpressionKind::Unary(op, receiver),
            Twin::InPlaceBinary(op) => {
                let mut hoisted = Vec::new();
                self.hoist_place_effects(&mut receiver, &mut hoisted);
                let read = Box::new(self.clone_place(&receiver));
                let value = self.synthesized(HirExpressionKind::Binary(op, read, argument()), span);
                self.store_after(hoisted, receiver, value, span)
            }
            Twin::InPlaceUnary(op) => {
                let mut hoisted = Vec::new();
                self.hoist_place_effects(&mut receiver, &mut hoisted);
                let read = Box::new(self.clone_place(&receiver));
                let value = self.synthesized(HirExpressionKind::Unary(op, read), span);
                self.store_after(hoisted, receiver, value, span)
            }
        };
        let in_place = matches!(twin, Twin::InPlaceBinary(_) | Twin::InPlaceUnary(_));
        // The rewrite keeps the call's node id in every spelling: a value
        // twin is the operator node itself, a bare in-place twin is the
        // `Assign`, and an effect-hoisted one is the wrapping `Block`. All
        // are recorded so Faber re-emission restores the authored spelling.
        if !in_place
            || matches!(
                expr.kind,
                HirExpressionKind::Assign(..) | HirExpressionKind::Block(..)
            )
        {
            self.numeric_twin_ids.push(expr.id);
        }
        in_place
    }

    /// `place ← value`, preceded by the temporaries that pin the place's
    /// effects so the read and the store see one evaluation of them.
    fn store_after(
        &mut self,
        hoisted: Vec<HirStatement>,
        place: Box<HirExpression>,
        value: HirExpression,
        span: radix_lexer::Span,
    ) -> HirExpressionKind {
        let assign = HirExpressionKind::Assign(place, Box::new(value));
        if hoisted.is_empty() {
            return assign;
        }
        let assign = self.synthesized(assign, span);
        let mut statements = hoisted;
        statements.push(HirStatement {
            id: self.fresh_hir_id(),
            kind: HirStatementKind::Expr(assign),
            span,
        });
        HirExpressionKind::Block(HirBlock::simple(statements, None, span))
    }

    /// Bind every effectful sub-expression of `place` (index expressions and
    /// call receivers) to a fresh local, replacing it with a read of that
    /// local, so the place can be read and stored without evaluating it twice.
    fn hoist_place_effects(&mut self, place: &mut HirExpression, hoisted: &mut Vec<HirStatement>) {
        match &mut place.kind {
            HirExpressionKind::Field(base, _) | HirExpressionKind::Deref(base) => {
                self.hoist_place_base(base, hoisted);
            }
            HirExpressionKind::Index(base, index) => {
                self.hoist_place_base(base, hoisted);
                if !is_pure_place(index) {
                    self.hoist_into_local(index, false, hoisted);
                }
            }
            _ => {}
        }
    }

    fn hoist_place_base(&mut self, base: &mut HirExpression, hoisted: &mut Vec<HirStatement>) {
        if is_place_chain(base) {
            self.hoist_place_effects(base, hoisted);
        } else if !is_pure_place(base) {
            self.hoist_into_local(base, true, hoisted);
        }
    }

    fn hoist_into_local(
        &mut self,
        expr: &mut HirExpression,
        mutable: bool,
        hoisted: &mut Vec<HirStatement>,
    ) {
        let name = self
            .interner
            .and_then(|interner| interner.lookup(&place_temporary_name(hoisted.len())))
            .unwrap_or(radix_lexer::Symbol(0));
        let def_id = DefId(self.next_synthetic_def);
        self.next_synthetic_def += 1;
        let span = expr.span;
        let read = self.synthesized(HirExpressionKind::Path(def_id), span);
        let init = std::mem::replace(expr, read);
        hoisted.push(HirStatement {
            id: self.fresh_hir_id(),
            kind: HirStatementKind::Local(HirLocal {
                def_id,
                name,
                ty: None,
                init: Some(init),
                mutable,
                runtime_provided: false,
                await_store: None,
                size_decl: false,
                size_static: false,
                is_await: false,
            }),
            span,
        });
    }

    /// Decide whether `expr` is a numeric twin call on a scalar numeric
    /// receiver that may be desugared.
    fn numeric_twin_of(&mut self, expr: &mut HirExpression) -> Option<Twin> {
        let HirExpressionKind::MethodCall(receiver, name, type_args, args, conversion) =
            &mut expr.kind
        else {
            return None;
        };
        if !type_args.is_empty()
            || conversion.is_some()
            || args.iter().any(|arg| arg.spread || arg.name.is_some())
        {
            return None;
        }
        let surface = self.interner?.resolve(*name).to_owned();
        let canonical = self.canonical_twin_name(&surface)?;
        let mut twin = twin_of(&canonical, args.len())?;

        let errors_before = self.errors.len();
        let receiver_ty = self.check_expr(receiver);
        // The receiver is checked exactly once, whether the probe declines or
        // the twin is rewritten (see `twin_probed_receiver`).
        self.twin_probed_receiver = Some((receiver.id, receiver_ty));
        if self.errors.len() != errors_before {
            return None;
        }
        let carrier = self.value_carrier_type(receiver_ty);
        let (_, payload) = self.types.peel_access_marker(carrier);
        let receiver_type = self.types.get(payload).clone();
        let modular = matches!(receiver_type, Type::ModularWord(_));
        // WHY: on a modular word `subtrahe` is the value-returning wrapping
        // twin of `subtracta`, not the in-place mutation.
        if modular && canonical == "subtrahe" {
            twin = Twin::Binary(HirBinOp::Sub);
        }
        let served = lookup_method_for_type(&receiver_type, &canonical, args.len())
            .is_some_and(|entry| matches!(entry.receiver, IntrinsicReceiver::Numeric));
        if !served {
            return None;
        }
        // Bitwise twins on floats and in-place twins on immutable bindings or
        // ref views keep the intrinsic checker's own diagnostics.
        if matches!(twin, Twin::Binary(op) | Twin::InPlaceBinary(op) if is_bitwise(op))
            && self.is_fractus(receiver_ty)
        {
            return None;
        }
        if matches!(
            twin,
            Twin::Unary(HirUnOp::BitNot) | Twin::InPlaceUnary(HirUnOp::BitNot)
        ) && self.is_fractus(receiver_ty)
        {
            return None;
        }
        if matches!(twin, Twin::InPlaceBinary(_) | Twin::InPlaceUnary(_)) {
            if self.expr_is_ref_view(receiver)
                || (matches!(receiver.kind, HirExpressionKind::Path(_))
                    && !self.path_binding_is_mutable(receiver))
            {
                return None;
            }
            // A call or other value is no place to store into; the intrinsic
            // checker reports it.
            if !is_place_chain(receiver) {
                return None;
            }
        }
        Some(twin)
    }

    /// A synthesized expression with a fresh identity.
    pub(super) fn synthesized(
        &mut self,
        kind: HirExpressionKind,
        span: radix_lexer::Span,
    ) -> HirExpression {
        HirExpression {
            id: self.fresh_hir_id(),
            kind,
            ty: None,
            span,
        }
    }

    /// A copy of a pure place with fresh expression identities.
    fn clone_place(&mut self, place: &HirExpression) -> HirExpression {
        let mut copy = place.clone();
        self.renumber(&mut copy);
        copy
    }

    fn renumber(&mut self, expr: &mut HirExpression) {
        expr.id = self.fresh_hir_id();
        match &mut expr.kind {
            HirExpressionKind::Field(inner, _)
            | HirExpressionKind::Deref(inner)
            | HirExpressionKind::Unary(_, inner) => self.renumber(inner),
            HirExpressionKind::Index(lhs, rhs) | HirExpressionKind::Binary(_, lhs, rhs) => {
                self.renumber(lhs);
                self.renumber(rhs);
            }
            _ => {}
        }
    }
}
