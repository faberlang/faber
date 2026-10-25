//! Precision markers for `instans<ms>`, `instans<us>`, and `instans<ns>`.
//!
//! Bare `instans` is second precision (Unix epoch default). Named markers are
//! reserved spellings, not user-defined type parameters.
//!
//! EDGE: runtime `faber::InstansPraecisio` also includes `Secunda`; this module
//! models only the explicit `instans<N>` type-parameter spellings.

use crate::{InferVar, MarkerFamily, Primitive, Type, TypeId, TypeTable};
use serde::{Deserialize, Serialize};

/// Declared precision for sized `instans<N>` spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstansPrecision {
    Millis,
    Micros,
    Nanos,
}

/// Rejection reason for an `instans<N>` precision argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstansPrecisionError {
    /// The marker is not `ms`, `us`, or `ns`.
    UnknownMarker,
    /// The argument list holds more than one marker.
    TooManyParams,
}

impl InstansPrecisionError {
    /// Diagnostic text for this rejection.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::UnknownMarker => "instans accepts only ms, us, or ns as a precision marker",
            Self::TooManyParams => "instans accepts at most one precision marker",
        }
    }
}

/// Precision rank for bare `instans` (seconds) and sized `instans<N>` markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstansPrecisionRank {
    Seconds,
    Millis,
    Micros,
    Nanos,
}

impl InstansPrecisionRank {
    fn rank(self) -> u8 {
        match self {
            Self::Seconds => 0,
            Self::Millis => 1,
            Self::Micros => 2,
            Self::Nanos => 3,
        }
    }

    /// Coarse → fine (gain precision) is lossless and assignable.
    #[must_use]
    pub fn is_widening_assignable(from: Self, to: Self) -> bool {
        from.rank() <= to.rank()
    }
}

impl InstansPrecision {
    /// Rank of an explicit marker; never [`InstansPrecisionRank::Seconds`].
    #[must_use]
    pub fn rank(self) -> InstansPrecisionRank {
        match self {
            Self::Millis => InstansPrecisionRank::Millis,
            Self::Micros => InstansPrecisionRank::Micros,
            Self::Nanos => InstansPrecisionRank::Nanos,
        }
    }

    /// Parse a precision marker spelling.
    ///
    /// # Errors
    ///
    /// Returns an error if the marker is not `ms`, `us`, or `ns`.
    pub fn from_marker(marker: &str) -> Result<Self, InstansPrecisionError> {
        match marker {
            "ms" => Ok(Self::Millis),
            "us" => Ok(Self::Micros),
            "ns" => Ok(Self::Nanos),
            _ => Err(InstansPrecisionError::UnknownMarker),
        }
    }

    /// Source spelling of this marker.
    #[must_use]
    pub fn faber_marker(self) -> &'static str {
        match self {
            Self::Millis => "ms",
            Self::Micros => "us",
            Self::Nanos => "ns",
        }
    }
}

/// Lower `instans` with optional precision type arguments.
///
/// Zero arguments lower to bare [`Primitive::Instans`]; one marker or infer
/// hole lowers to a sized carrier. A rejected argument is reported through
/// `report` and lowers to [`Type::Error`].
pub fn lower_sized_instans(
    params: &[radix_syntax::TypeExpr],
    interner: &radix_lexer::Interner,
    types: &mut TypeTable,
    report: &mut dyn FnMut(&str),
) -> TypeId {
    let param = match params {
        [] => return types.primitive(Primitive::Instans),
        [param] => param,
        _ => {
            report(InstansPrecisionError::TooManyParams.message());
            return types.intern(Type::Error);
        }
    };
    if matches!(param.kind, radix_syntax::TypeExprKind::Infer) {
        return types.intern(Type::MarkerHole {
            family: MarkerFamily::Instans,
            var: InferVar(param.span.start),
        });
    }
    let Some(marker) = precision_marker_from_type_expr(param, interner) else {
        report("precision marker must be a bare spelling such as ms or us");
        return types.intern(Type::Error);
    };
    match InstansPrecision::from_marker(marker) {
        Ok(precision) => types.sized_instans(precision),
        Err(err) => {
            report(err.message());
            types.intern(Type::Error)
        }
    }
}

/// Bare spelling of a named marker argument; `None` for nested or non-named
/// arguments.
fn precision_marker_from_type_expr<'a>(
    ty: &radix_syntax::TypeExpr,
    interner: &'a radix_lexer::Interner,
) -> Option<&'a str> {
    match &ty.kind {
        radix_syntax::TypeExprKind::Named(ident, params) if params.is_empty() => {
            Some(interner.resolve(ident.name))
        }
        _ => None,
    }
}

/// Effective precision for bare or sized `instans` types.
///
/// Alias layers are transparent; every other carrier yields `None`.
#[must_use]
pub fn effective_instans_precision(types: &TypeTable, ty: TypeId) -> Option<InstansPrecisionRank> {
    match types.get(ty) {
        Type::Primitive(Primitive::Instans) => Some(InstansPrecisionRank::Seconds),
        Type::SizedInstans(precision) => Some(precision.rank()),
        Type::Alias(_, inner) => effective_instans_precision(types, *inner),
        _ => None,
    }
}

/// Whether two `instans` carriers are assignable via equality or coarse→fine widening.
#[must_use]
pub fn is_instans_assignable(types: &TypeTable, from: TypeId, to: TypeId) -> bool {
    match (
        effective_instans_precision(types, from),
        effective_instans_precision(types, to),
    ) {
        (Some(from_rank), Some(to_rank)) => {
            InstansPrecisionRank::is_widening_assignable(from_rank, to_rank)
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "instans_precision_test.rs"]
mod tests;
