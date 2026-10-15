//! Which user-declared methods change their receiver in place.
//!
//! A class method mutates its receiver when its body writes through `ego`
//! (assignment, increment/decrement, or an in-place intrinsic on a place rooted
//! at `ego`) or calls another mutating method on such a place. Methods carry no
//! receiver marker in source, so the property is inferred from the body and
//! closed over sibling and cross-class calls with a fixpoint.
//!
//! The result feeds the immutable-binding rule in
//! [`super::definite_assignment`]: a mutating method call on an immutable
//! binding is rejected exactly like a field write on that binding, so both
//! forms obey one const/var rule.
//!
//! LIMITS
//! ======
//! - Only methods declared in the analyzed module are known; a call to an
//!   imported class's method is not classified.
//! - A write through a local alias of a field (`mut list<int> v ← ego.xs`) is
//!   not followed; the rule errs toward accepting.

use crate::{Type, TypeId, TypeTable};
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{
    DefId, HirExpression, HirExpressionKind, HirItemKind, HirModule, HirStatement, HirStatementKind,
};
use radix_intrinsics::{is_mutating_receiver, lookup_method_for_type_id};
use radix_lexer::{Interner, Symbol};
use rustc_hash::FxHashSet;

/// `(class definition, method name)` pairs whose body mutates the receiver.
pub(super) type MutatingMethods = FxHashSet<(DefId, Symbol)>;

/// Compute the mutating-method set for every class in `hir`.
pub(super) fn collect(hir: &HirModule, types: &TypeTable, interner: &Interner) -> MutatingMethods {
    let mut known = MutatingMethods::default();
    loop {
        let before = known.len();
        for item in &hir.items {
            let HirItemKind::Struct(strukt) = &item.kind else {
                continue;
            };
            for method in &strukt.methods {
                let key = (item.def_id, method.func.name);
                if known.contains(&key) {
                    continue;
                }
                let mut scan = SelfWriteScan {
                    self_def: item.def_id,
                    types,
                    interner,
                    known: &known,
                    found: false,
                };
                scan.visit_function(&method.func);
                if scan.found {
                    known.insert(key);
                }
            }
        }
        if known.len() == before {
            return known;
        }
    }
}

/// Class definition behind a receiver type, looking through access markers,
/// aliases, and generic application.
pub(super) fn class_def(types: &TypeTable, ty: TypeId) -> Option<DefId> {
    match types.get(ty) {
        Type::Struct(def) => Some(*def),
        Type::Ref(_, inner) | Type::Alias(_, inner) => class_def(types, *inner),
        Type::Applied(base, _) => class_def(types, *base),
        _ => None,
    }
}

/// Root binding of a place expression, looking through projections and `!`.
pub(super) fn place_root(expr: &HirExpression) -> Option<DefId> {
    match &expr.kind {
        HirExpressionKind::Path(def) => Some(*def),
        HirExpressionKind::Field(object, _)
        | HirExpressionKind::Index(object, _)
        | HirExpressionKind::Deref(object) => place_root(object),
        HirExpressionKind::NonNull(inner, _) => place_root(inner),
        _ => None,
    }
}

struct SelfWriteScan<'a> {
    self_def: DefId,
    types: &'a TypeTable,
    interner: &'a Interner,
    known: &'a MutatingMethods,
    found: bool,
}

impl SelfWriteScan<'_> {
    fn rooted_at_self(&self, place: &HirExpression) -> bool {
        place_root(place) == Some(self.self_def)
    }

    fn call_mutates(&self, receiver: &HirExpression, name: Symbol, arg_count: usize) -> bool {
        let Some(ty) = receiver.ty else {
            return false;
        };
        if class_def(self.types, ty).is_some_and(|def| self.known.contains(&(def, name))) {
            return true;
        }
        lookup_method_for_type_id(ty, self.types, self.interner.resolve(name), arg_count)
            .is_some_and(is_mutating_receiver)
    }
}

impl HirVisitor for SelfWriteScan<'_> {
    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if self.found {
            return;
        }
        if let HirStatementKind::IncDec(inc_dec) = &stmt.kind
            && self.rooted_at_self(&inc_dec.target)
        {
            self.found = true;
            return;
        }
        radix_hir::visit::walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        if self.found {
            return;
        }
        match &expr.kind {
            HirExpressionKind::Assign(target, _)
            | HirExpressionKind::ConversioAssign { target, .. }
                if self.rooted_at_self(target) =>
            {
                self.found = true;
                return;
            }
            HirExpressionKind::MethodCall(receiver, name, _, args, _)
                if self.rooted_at_self(receiver)
                    && self.call_mutates(receiver, *name, args.len()) =>
            {
                self.found = true;
                return;
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

/// The receiver of `value!.method()` without the non-null assertion.
pub(super) fn unwrap_non_null(expr: &HirExpression) -> &HirExpression {
    match &expr.kind {
        HirExpressionKind::NonNull(inner, _) => unwrap_non_null(inner),
        _ => expr,
    }
}
