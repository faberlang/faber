//! `sermo<O, R>` conversation typing (DECISIONS D6.11, D6.12; delivery U10).
//!
//! A route served by a Faber `@ ad` handler visible to this module (its own
//! handlers plus those of the files it imports) types its `ad` call site as
//! `sermo<O, R>`: `O` is the handler's one parameter (`nihil` when it takes
//! none) and `R` is its item type (the return type, the yielded type for a
//! cursor, `vacuum` for no return). Every other route — host routes and
//! handlers outside the import closure — keeps bare `sermo`, which means
//! `sermo<valor, valor>`.
//!
//! RULES
//! =====
//! - The opener must be assignable to `O`; supplying one to an `O = nihil`
//!   handler, or omitting it when `O ≠ nihil`, is an error.
//! - `↦ T` on `sermo<O, R>` requires `T = R` or `T = lista<R>`.
//! - `s.tuus<T>()` requires `T = R` and `s.meus<T>()` requires `T = O`; an
//!   omitted `T` is inferred.
//! - Handlers cannot read later frames (two-way handlers are deferred), so
//!   `meus.sink` toward a Faber handler is an error; `meus.fini()` stays legal.
//! - `sermo<O, R>` assigns to bare `sermo`; the reverse is an error.
//!
//! The type arguments are compile-time only. Finalization erases them
//! ([`crate::builtins::frame_types::erase_conversation_type`]), so the HIR
//! every backend reads carries the unchanged `sermo` carrier and frames still
//! carry `valor` on the wire.

use super::TypeChecker;
use crate::builtins::frame_types;
use crate::{Primitive, SemanticErrorKind, Type, TypeId};
use frame_types::SermoShape;
use radix_hir::{HirExpression, HirItemKind, HirModule};
use radix_lexer::Symbol;

pub(super) const ISSUE_AD_ROUTE_OPENER_TYPE_MISMATCH: &str = "ad_route_opener_type_mismatch";
pub(super) const ISSUE_AD_ROUTE_OPENER_MISSING: &str = "ad_route_opener_missing";
pub(super) const ISSUE_AD_ROUTE_OPENER_UNEXPECTED: &str = "ad_route_opener_unexpected";
pub(super) const ISSUE_SERMO_REPLY_TYPE_MISMATCH: &str = "sermo_reply_type_mismatch";
pub(super) const ISSUE_SERMO_VIEW_TYPE_MISMATCH: &str = "sermo_view_type_mismatch";
pub(super) const ISSUE_AD_HANDLER_SEND_CLOSED: &str = "ad_handler_send_closed";
pub(super) const ISSUE_SERMO_TYPED_FROM_UNTYPED: &str = "sermo_typed_from_untyped";
pub(super) const ISSUE_SERMO_TYPE_ARGUMENTS_MISMATCH: &str = "sermo_type_arguments_mismatch";

/// Conversation types of one tier-1 route, as the caller sees them.
#[derive(Debug, Clone, Copy)]
pub struct AdRouteTypes {
    /// What the caller sends: the handler parameter, or `nihil`.
    pub opener: TypeId,
    /// Each item frame back.
    pub reply: TypeId,
}

/// Direction of a `sermo` view method on a typed conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SermoView {
    Tuus,
    Meus,
}

impl TypeChecker<'_> {
    /// Record the conversation types of every route this module can see:
    /// imported handlers first, then this module's own (which win).
    pub(super) fn collect_ad_route_types(&mut self, hir: &HirModule) {
        for route in self.resolver.imported_ad_routes() {
            self.ad_routes.insert(
                route.route,
                AdRouteTypes {
                    opener: route.opener,
                    reply: route.reply,
                },
            );
        }
        let Some(interner) = self.interner else {
            return;
        };
        let nihil = self.types.primitive(Primitive::Nihil);
        let vacuum = self.types.primitive(Primitive::Vacuum);
        for item in &hir.items {
            let HirItemKind::Function(func) = &item.kind else {
                continue;
            };
            let Some(route) = item
                .annotations
                .iter()
                .find_map(|annotation| annotation.ad_route(interner))
            else {
                continue;
            };
            let opener = func.params.first().map_or(nihil, |param| param.ty);
            let reply = func.ret_ty.unwrap_or(vacuum);
            self.ad_routes.insert(route, AdRouteTypes { opener, reply });
        }
    }

    /// Type `ad '<route>' (opener)`.
    pub(super) fn check_ad_expr(
        &mut self,
        route: Symbol,
        opener: Option<&mut HirExpression>,
        span: radix_lexer::Span,
    ) -> TypeId {
        let Some(sermo_def) = frame_types::sermo_def_id(self.resolver) else {
            if let Some(opener) = opener {
                self.check_expr(opener);
            }
            self.missing_type_annotation("sermo_type_not_registered", span);
            return self.types.primitive(Primitive::Ignotum);
        };
        let Some(route_types) = self.ad_routes.get(&route).copied() else {
            if let Some(opener) = opener {
                let valor = self.types.primitive(Primitive::Valor);
                let opener_ty = self.check_expr_with_expected(opener, Some(valor));
                self.unify_assign_issue(
                    opener_ty,
                    valor,
                    opener.span,
                    super::expr::ISSUE_STREAM_AD_OPENER_TYPE_MISMATCH,
                );
            }
            return frame_types::sermo_type_id(self.types, sermo_def);
        };
        let takes_opener = !matches!(
            self.types.get(route_types.opener),
            Type::Primitive(Primitive::Nihil)
        );
        match (opener, takes_opener) {
            (Some(opener), true) => {
                let opener_ty = self.check_expr_with_expected(opener, Some(route_types.opener));
                self.unify_assign_issue(
                    opener_ty,
                    route_types.opener,
                    opener.span,
                    ISSUE_AD_ROUTE_OPENER_TYPE_MISMATCH,
                );
            }
            (Some(opener), false) => {
                self.check_expr(opener);
                self.ad_route_error(ISSUE_AD_ROUTE_OPENER_UNEXPECTED, route, opener.span);
            }
            (None, true) => self.ad_route_error(ISSUE_AD_ROUTE_OPENER_MISSING, route, span),
            (None, false) => {}
        }
        frame_types::sermo_typed_type_id(
            self.types,
            sermo_def,
            route_types.opener,
            route_types.reply,
        )
    }

    fn ad_route_error(&mut self, issue: &'static str, route: Symbol, span: radix_lexer::Span) {
        let mut error = crate::SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
            .with_arg("issue", issue);
        if let Some(interner) = self.interner {
            error = error.with_arg("route", interner.resolve(route));
        }
        self.errors.push(error);
    }

    /// Whether `ty` is the typecheck-only closed `meus<T, marker>` view (two
    /// applied args) that a `meus` toward a Faber handler yields.
    pub(super) fn is_closed_meus(&self, ty: TypeId) -> bool {
        let Some(meus_def) = frame_types::meus_def_id(self.resolver) else {
            return false;
        };
        matches!(
            self.types.get(self.resolve_inferred_type(ty)),
            Type::Applied(base, args)
                if args.len() == 2
                    && matches!(self.types.get(*base), Type::Struct(def) if *def == meus_def)
        )
    }

    /// Write an inferred `s.tuus()` / `s.meus()` payload onto the HIR call,
    /// so backends that read the call's type arguments see the same `T` a
    /// written `s.tuus<T>()` would carry.
    pub(super) fn record_inferred_sermo_view_type_arg(
        &mut self,
        receiver: &HirExpression,
        name: Symbol,
        type_args: &mut Vec<TypeId>,
        view_ty: TypeId,
    ) {
        if !type_args.is_empty() {
            return;
        }
        let Some(interner) = self.interner else {
            return;
        };
        if !matches!(interner.resolve(name), "tuus" | "meus") {
            return;
        }
        let Some(receiver_ty) = receiver.ty else {
            return;
        };
        if !matches!(
            self.sermo_shape(receiver_ty),
            Some(SermoShape::Typed { .. })
        ) {
            return;
        }
        if let Type::Applied(_, args) = self.types.get(self.resolve_inferred_type(view_ty))
            && let Some(radix_types::AppliedArg::Type(payload)) = args.first()
        {
            type_args.push(*payload);
        }
    }

    /// Bare or typed conversation shape of `ty`, if it is a `sermo`.
    pub(super) fn sermo_shape(&self, ty: TypeId) -> Option<SermoShape> {
        let sermo_def = frame_types::sermo_def_id(self.resolver)?;
        frame_types::sermo_shape(self.types, sermo_def, self.resolve_inferred_type(ty))
    }

    /// `sermo<O, R> ↦ T`: `T` must be `R` or `lista<R>`. `None` for a bare
    /// conversation, which keeps the materializer policy alone.
    pub(super) fn check_typed_sermo_materialize(
        &mut self,
        source_ty: TypeId,
        target: TypeId,
        span: radix_lexer::Span,
    ) -> Option<bool> {
        let Some(SermoShape::Typed { reply, .. }) = self.sermo_shape(source_ty) else {
            return None;
        };
        let target = self.resolve_inferred_type(target);
        let reply = self.resolve_inferred_type(reply);
        let matches = self.types.equals(target, reply)
            || matches!(self.types.get(target), Type::Array(element) if self.types.equals(*element, reply));
        if !matches {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_SERMO_REPLY_TYPE_MISMATCH,
                span,
            );
        }
        Some(matches)
    }

    /// Payload type of `s.tuus<T>()` / `s.meus<T>()` on a typed conversation:
    /// the explicit `T` must be the conversation's `R` / `O`, and an omitted
    /// `T` is inferred. `None` for a bare conversation.
    pub(super) fn typed_sermo_view_payload(
        &mut self,
        receiver_ty: TypeId,
        view: SermoView,
        explicit_type_args: &[TypeId],
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let Some(SermoShape::Typed { opener, reply }) = self.sermo_shape(receiver_ty) else {
            return None;
        };
        let expected = match view {
            SermoView::Tuus => reply,
            SermoView::Meus => opener,
        };
        match explicit_type_args {
            [] => Some(expected),
            [written] => {
                let written_resolved = self.resolve_inferred_type(*written);
                let expected_resolved = self.resolve_inferred_type(expected);
                if !self.types.equals(written_resolved, expected_resolved) {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_SERMO_VIEW_TYPE_MISMATCH,
                        span,
                    );
                    return Some(self.error_type);
                }
                Some(expected)
            }
            _ => None,
        }
    }

    /// Named errors for conversation assignability (§2.3): bare `sermo` does
    /// not flow into `sermo<O, R>`, and two typed conversations must agree.
    /// Typed-to-bare falls through to ordinary struct acceptance. Returns
    /// the error type when a diagnostic was emitted.
    pub(super) fn sermo_assign_precheck(
        &mut self,
        from: TypeId,
        to: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let Some(SermoShape::Typed {
            opener: to_opener,
            reply: to_reply,
        }) = self.sermo_shape(to)
        else {
            return None;
        };
        match self.sermo_shape(from)? {
            SermoShape::Bare => {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_SERMO_TYPED_FROM_UNTYPED,
                    span,
                );
                Some(self.error_type)
            }
            SermoShape::Typed { opener, reply } => {
                let agree = self.types.equals(
                    self.resolve_inferred_type(opener),
                    self.resolve_inferred_type(to_opener),
                ) && self.types.equals(
                    self.resolve_inferred_type(reply),
                    self.resolve_inferred_type(to_reply),
                );
                if agree {
                    return None;
                }
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_SERMO_TYPE_ARGUMENTS_MISMATCH,
                    span,
                );
                Some(self.error_type)
            }
        }
    }
}
