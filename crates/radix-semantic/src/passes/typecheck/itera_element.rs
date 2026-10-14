//! `itera ex` / `itera de` element typing (D19.4).
//!
//! Every iterable source binds its real element type; there is no silent
//! `numerus` fallback. `copia<T>` binds `T` (unspecified order, D19.2).
//! Text binds `littera`, one Unicode scalar, on every backend (D10.1/D10.2 —
//! [`TypeChecker::text_element_type`]); bytes bind `octetus` (`numerus<u8>`),
//! the same row as `octeti[i]` ([`TypeChecker::byte_element_type`]). A source with no iteration surface — a user `genus`, a scalar, a
//! generic parameter — is `itera_source_not_iterable`: per D19.5 a user type
//! that should be iterable exposes a method returning a cursor.
//!
//! `itera ab` (range bounds) and the `apud` arity/mode rules are checked by
//! the caller before this dispatch.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_hir::HirIteraMode;
use radix_lexer::Span;

use super::expr::ISSUE_CURSOR_ITERATION_REQUIRES_EX;

pub(super) const ISSUE_ITERA_SOURCE_NOT_ITERABLE: &str = "itera_source_not_iterable";

impl TypeChecker<'_> {
    /// Element row of text iteration and text cursors (D10.1): `littera`.
    /// `ascii[i]` binds the same row (D10.3); `textus[i]` keeps its
    /// `ascii<1>` row until operator fork F5 rules on its index unit.
    pub fn text_element_type(&mut self) -> TypeId {
        self.littera_type()
    }

    /// Element row of an `octeti` scalar index and of byte iteration:
    /// `octetus ≡ numerus<u8>` (D10.4).
    pub fn byte_element_type(&mut self) -> TypeId {
        self.types
            .sized_numeric(Primitive::Numerus, NumericWidth::U8)
    }

    /// Binding type for an `itera ex` / `itera de` over `dispatch_ty` (already
    /// peeled of alias and access markers). `itera ab` never reaches here.
    pub(super) fn itera_element_type(
        &mut self,
        dispatch_ty: TypeId,
        mode: HirIteraMode,
        span: Span,
    ) -> TypeId {
        let ex = matches!(mode, HirIteraMode::Ex);
        match self.types.get(dispatch_ty).clone() {
            // Recovery: the source already carries a diagnostic.
            Type::Error => self.error_type,
            Type::Array(inner) if ex => inner,
            Type::Map(_, value) | Type::Ledger(_, value) if ex => value,
            Type::Map(key, _) | Type::Ledger(key, _) => key,
            Type::Cursor(inner) | Type::AsyncCursor(inner, _) if ex => inner,
            Type::Cursor(_) | Type::AsyncCursor(_, _) => {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_CURSOR_ITERATION_REQUIRES_EX,
                    span,
                );
                self.error_type
            }
            // Value iteration over element-carrying collections. Queue FIFO /
            // stack top-first order is a MIR law; `copia` order is
            // unspecified (D19.2). Tensor, matrix, vector and sparse sources
            // bind their element; the `apud` arity rule (checked by the
            // caller) owns shape admission, and MIR stays the decline site
            // for shapes it cannot walk.
            Type::Queue(inner)
            | Type::BoundedQueue { element: inner, .. }
            | Type::Stack(inner)
            | Type::BoundedStack { element: inner, .. }
            | Type::BoundedArray { element: inner, .. }
            | Type::Tensor(inner, _)
            | Type::Matrix(inner, _)
            | Type::Vector(inner, _)
            | Type::Sparsa(inner, _)
                if ex =>
            {
                inner
            }
            // MIR walks a `copia` through a `lista<T>` snapshot; intern the row
            // for that lowering.
            Type::Set(inner) if ex => {
                self.types.array(inner);
                inner
            }
            Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. } if ex => {
                self.byte_element_type()
            }
            Type::Primitive(Primitive::Textus | Primitive::Ascii)
            | Type::BoundedTextus { .. }
            | Type::BoundedAscii { .. }
                if ex =>
            {
                let element = self.text_element_type();
                // MIR walks unbounded text through the cord cursor drained to
                // a list; intern both rows for that lowering.
                self.types.cursor(element);
                self.types.array(element);
                element
            }
            // `de` over any other iterable binds its index; MIR admits only
            // the lista/tabula index surfaces and declines the rest.
            ref kind if matches!(mode, HirIteraMode::De) && is_iterable_kind(kind) => {
                self.numerus_type()
            }
            _ => {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_ITERA_SOURCE_NOT_ITERABLE,
                    span,
                );
                self.error_type
            }
        }
    }
}

/// Whether `kind` has an iteration surface at all (independent of mode).
fn is_iterable_kind(kind: &Type) -> bool {
    matches!(
        kind,
        Type::Array(_)
            | Type::BoundedArray { .. }
            | Type::Queue(_)
            | Type::BoundedQueue { .. }
            | Type::Stack(_)
            | Type::BoundedStack { .. }
            | Type::Set(_)
            | Type::Map(_, _)
            | Type::Ledger(_, _)
            | Type::Tensor(_, _)
            | Type::Matrix(_, _)
            | Type::Vector(_, _)
            | Type::Sparsa(_, _)
            | Type::Primitive(Primitive::Textus | Primitive::Ascii | Primitive::Octeti)
            | Type::BoundedTextus { .. }
            | Type::BoundedAscii { .. }
            | Type::BoundedOcteti { .. }
    )
}
