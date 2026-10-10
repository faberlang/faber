//! Pass-2 typing of the build-time file embed `insere "path"` (D8.10, st1 R5).
//!
//! An embed has no type of its own: the slot it fills decides. A `textus`
//! slot takes the file as text, an `octeti` slot takes the raw bytes, and
//! anything else (including a slot still awaiting inference, or no slot at
//! all) is a named compile error. The `insere_fold` pass later replaces the
//! typed node with the file's literal.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_lexer::Span;

const ISSUE_INSERE_TYPE_REQUIRED: &str = "insere_type_required";
const ISSUE_INSERE_TYPE_INVALID: &str = "insere_type_invalid";

/// Type `insere "path"` from the slot's expected type.
pub(super) fn check_insere(
    checker: &mut TypeChecker<'_>,
    path: Symbol,
    expected: Option<TypeId>,
    span: Span,
) -> TypeId {
    let resolved = expected.map(|ty| checker.resolve_inferred_type(ty));
    let (issue, recovered) = match resolved {
        None => (ISSUE_INSERE_TYPE_REQUIRED, checker.error_type),
        Some(ty) => match checker.types.get(ty) {
            Type::Primitive(Primitive::Textus | Primitive::Octeti) => return ty,
            // `lista<b8>`: the bytes of a file (ruling B9).
            Type::Array(element)
                if matches!(
                    checker.types.get(*element),
                    Type::Bits(radix_types::BitsWidth::B8)
                ) =>
            {
                return ty;
            }
            // An erroring slot already reported its own fault.
            Type::Error => return checker.error_type,
            Type::Infer(_) => (ISSUE_INSERE_TYPE_REQUIRED, checker.error_type),
            // The slot is concrete but cannot hold a file: recover as the
            // slot's own type so the store check adds no second error.
            _ => (ISSUE_INSERE_TYPE_INVALID, ty),
        },
    };
    let path = checker
        .interner
        .map(|interner| interner.resolve(path).to_owned())
        .unwrap_or_default();
    checker.error_issue_with_args(
        SemanticErrorKind::InsereInvalid,
        issue,
        span,
        &[("path", path)],
    );
    recovered
}
