//! Width and shape relations used by typed match patterns.
//!
//! A width pattern is a runtime predicate over a family carrier. It is not a
//! type-test operation: `lista<T, N>` means the value is a `lista<T>` whose live
//! length is at most `N`, while `lista<T>` is the family catch-all. The same
//! relation covers bounded textus/ascii/octeti and literal tensor figures.

use crate::{IndexExpr, IndexId, NumericWidth, Primitive, Type, TypeId, TypeTable};

/// Runtime family discriminant for a width or shape pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidthFamily {
    Lista(TypeId),
    Textus,
    Ascii,
    Octeti,
    Tensor(TypeId),
    Vector(TypeId),
    Matrix(TypeId),
    /// Numerus width rows are check-only until the decimal D4 emission gate.
    Numerus(Option<NumericWidth>),
}

/// A family pattern and its optional bound/shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidthPattern {
    pub family: WidthFamily,
    /// `None` is the unbounded family form. For tensor families it also means
    /// an unspecified runtime shape.
    pub bound: Option<IndexId>,
}

/// Classify a semantic type as a family width/shape pattern.
#[must_use]
pub fn width_pattern(types: &TypeTable, ty: TypeId) -> Option<WidthPattern> {
    classify(types, ty)
}

/// Whether a type-pattern target is a valid predicate over the expected
/// scrutinee type. A bounded target may narrow an unbounded or differently
/// bounded value of the same family; exact element types remain required.
#[must_use]
pub fn width_pattern_applies(types: &TypeTable, expected: TypeId, pattern: TypeId) -> bool {
    if let Type::Union(members) = types.get(expected) {
        return members
            .iter()
            .copied()
            .any(|member| width_pattern_applies(types, member, pattern));
    }
    if let Type::Option(inner) = types.get(expected) {
        return width_pattern_applies(types, *inner, pattern);
    }
    let Some(expected) = classify(types, expected) else {
        return false;
    };
    let Some(pattern) = classify(types, pattern) else {
        return false;
    };
    same_family(types, expected.family, pattern.family)
}

/// Whether `pattern` covers all values represented by `value`.
#[must_use]
pub fn width_pattern_covers(types: &TypeTable, pattern: TypeId, value: TypeId) -> bool {
    if let Type::Union(members) = types.get(value) {
        return members
            .iter()
            .copied()
            .all(|member| width_pattern_covers(types, pattern, member));
    }
    if let Type::Option(inner) = types.get(value) {
        return width_pattern_covers(types, pattern, *inner);
    }
    let Some(pattern) = classify(types, pattern) else {
        return false;
    };
    let Some(value) = classify(types, value) else {
        return false;
    };
    if !same_family(types, pattern.family, value.family) {
        return false;
    }
    if let (WidthFamily::Numerus(pattern_width), WidthFamily::Numerus(value_width)) =
        (pattern.family, value.family)
    {
        return match (pattern_width, value_width) {
            (None, _) => true,
            (Some(pattern), Some(value)) => pattern == value,
            (Some(_), None) => false,
        };
    }

    match pattern.bound {
        None => true,
        Some(pattern_bound) => match value.bound {
            None => false,
            Some(value_bound) => index_covers(types, pattern_bound, value_bound),
        },
    }
}

/// Return the literal length bound carried by a list or scalar-buffer pattern.
/// Tensor and symbolic bounds deliberately return `None`: emitters must fail
/// closed instead of inventing arithmetic or shape runtime APIs.
#[must_use]
pub fn literal_length_guard(types: &TypeTable, pattern: TypeId) -> Option<u64> {
    let description = classify(types, pattern)?;
    if !matches!(
        description.family,
        WidthFamily::Lista(_) | WidthFamily::Textus | WidthFamily::Ascii | WidthFamily::Octeti
    ) {
        return None;
    }
    let bound = description.bound?;
    match types.get_index(bound) {
        IndexExpr::Literal(value) => Some(*value),
        _ => None,
    }
}

/// Whether the pattern is an unbounded family catch-all.
#[must_use]
pub fn is_family_catchall(types: &TypeTable, pattern: TypeId) -> bool {
    width_pattern(types, pattern).is_some_and(|pattern| {
        pattern.bound.is_none() && !matches!(pattern.family, WidthFamily::Numerus(Some(_)))
    })
}

fn classify(types: &TypeTable, ty: TypeId) -> Option<WidthPattern> {
    match types.get(ty) {
        Type::Alias(_, inner) | Type::Ref(_, inner) | Type::Applied(inner, _) => {
            classify(types, *inner)
        }
        Type::Array(element) => Some(WidthPattern {
            family: WidthFamily::Lista(*element),
            bound: None,
        }),
        Type::BoundedArray { element, capacity } => Some(WidthPattern {
            family: WidthFamily::Lista(*element),
            bound: Some(*capacity),
        }),
        Type::Primitive(Primitive::Textus) => Some(WidthPattern {
            family: WidthFamily::Textus,
            bound: None,
        }),
        Type::BoundedTextus { capacity } => Some(WidthPattern {
            family: WidthFamily::Textus,
            bound: Some(*capacity),
        }),
        Type::Primitive(Primitive::Ascii) => Some(WidthPattern {
            family: WidthFamily::Ascii,
            bound: None,
        }),
        Type::BoundedAscii { capacity } => Some(WidthPattern {
            family: WidthFamily::Ascii,
            bound: Some(*capacity),
        }),
        Type::Primitive(Primitive::Octeti) => Some(WidthPattern {
            family: WidthFamily::Octeti,
            bound: None,
        }),
        Type::BoundedOcteti { capacity } => Some(WidthPattern {
            family: WidthFamily::Octeti,
            bound: Some(*capacity),
        }),
        Type::Primitive(Primitive::Numerus) => Some(WidthPattern {
            family: WidthFamily::Numerus(None),
            bound: None,
        }),
        Type::SizedNumeric(Primitive::Numerus, width) => Some(WidthPattern {
            family: WidthFamily::Numerus(Some(*width)),
            bound: None,
        }),
        Type::Tensor(element, shape) => Some(WidthPattern {
            family: WidthFamily::Tensor(*element),
            bound: shape_is_runtime_unspecified(types, *shape).then_some(*shape),
        }),
        Type::Vector(element, width) => Some(WidthPattern {
            family: WidthFamily::Vector(*element),
            bound: shape_is_runtime_unspecified(types, *width).then_some(*width),
        }),
        Type::Matrix(element, shape) => Some(WidthPattern {
            family: WidthFamily::Matrix(*element),
            bound: shape_is_runtime_unspecified(types, *shape).then_some(*shape),
        }),
        _ => None,
    }
}

fn shape_is_runtime_unspecified(types: &TypeTable, shape: IndexId) -> bool {
    !matches!(types.get_index(shape), IndexExpr::Unspecified)
}

fn same_family(types: &TypeTable, left: WidthFamily, right: WidthFamily) -> bool {
    match (left, right) {
        (WidthFamily::Lista(left), WidthFamily::Lista(right))
        | (WidthFamily::Tensor(left), WidthFamily::Tensor(right))
        | (WidthFamily::Vector(left), WidthFamily::Vector(right))
        | (WidthFamily::Matrix(left), WidthFamily::Matrix(right)) => types.equals(left, right),
        (WidthFamily::Textus, WidthFamily::Textus)
        | (WidthFamily::Ascii, WidthFamily::Ascii)
        | (WidthFamily::Octeti, WidthFamily::Octeti)
        | (WidthFamily::Numerus(_), WidthFamily::Numerus(_)) => true,
        _ => false,
    }
}

fn index_covers(types: &TypeTable, pattern: IndexId, value: IndexId) -> bool {
    match (types.get_index(pattern), types.get_index(value)) {
        (IndexExpr::Literal(pattern), IndexExpr::Literal(value)) => pattern >= value,
        (IndexExpr::Param(pattern), IndexExpr::Param(value)) => pattern == value,
        (IndexExpr::Infer(_) | IndexExpr::Unspecified, _) => true,
        _ => types.index_equals(pattern, value),
    }
}
