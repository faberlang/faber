//! Desugar helpers for canonical braced `@` metadata into structured AST views.
//!
//! Owns field extraction for `@ cli`, `@ imperium`, `@ optio`, and `@ operandus`
//! annotation families. Does not own promoted statement-form views (see
//! `annotation_promotion.rs`).

use crate::{
    AnnotationField, AnnotationFieldValue, BracedAnnotation, Expr, ExprKind, Ident, Literal,
    TypeExpr,
};
use radix_lexer::{Interner, Span};

/// Return whether `braced` names the given annotation family.
#[must_use]
pub fn braced_family_is(braced: &BracedAnnotation, interner: &Interner, family: &str) -> bool {
    interner.resolve(braced.name.name) == family
}

/// Result of strict braced annotation extraction.
///
/// Compiler-owned consumers use this where a known family with malformed fields
/// must fail closed instead of becoming indistinguishable from an absent family.
#[derive(Debug, Clone, Copy)]
pub enum BracedView<T> {
    Absent,
    Valid(T),
    Malformed(BracedAnnotationError),
}

#[derive(Debug, Clone, Copy)]
pub struct BracedAnnotationError {
    pub annotation: &'static str,
    pub field: &'static str,
    pub expected: &'static str,
    pub span: Span,
}

/// Borrowed `@ cli` field view (`nomen = STRING`).
pub fn braced_cli_nomen(
    braced: &BracedAnnotation,
    interner: &Interner,
) -> Option<radix_lexer::Symbol> {
    if !braced_family_is(braced, interner, "cli") {
        return None;
    }
    find_field(braced, "nomen", interner).and_then(string_from_field)
}

#[must_use]
pub fn braced_cli_nomen_strict(
    braced: &BracedAnnotation,
    interner: &Interner,
) -> BracedView<radix_lexer::Symbol> {
    string_field_strict(braced, interner, "cli", "@ cli", "nomen")
}

/// Borrowed `@ imperium` field view.
pub fn braced_imperium_nomen(
    braced: &BracedAnnotation,
    interner: &Interner,
) -> Option<radix_lexer::Symbol> {
    if !braced_family_is(braced, interner, "imperium") {
        return None;
    }
    find_field(braced, "nomen", interner).and_then(string_from_field)
}

#[must_use]
pub fn braced_imperium_nomen_strict(
    braced: &BracedAnnotation,
    interner: &Interner,
) -> BracedView<radix_lexer::Symbol> {
    string_field_strict(braced, interner, "imperium", "@ imperium", "nomen")
}

/// Borrowed `@ optio` field view for CLI and HIR consumers.
pub struct BracedOptioView<'a> {
    pub binding: &'a Ident,
    pub ty: Option<&'a TypeExpr>,
    pub short: Option<radix_lexer::Symbol>,
    pub long: Option<radix_lexer::Symbol>,
    pub description: Option<radix_lexer::Symbol>,
    pub global: bool,
    pub default: Option<&'a Expr>,
}

pub fn braced_optio_view<'a>(
    braced: &'a BracedAnnotation,
    interner: &Interner,
) -> Option<BracedOptioView<'a>> {
    if !braced_family_is(braced, interner, "optio") {
        return None;
    }

    let binding_field = find_field(braced, "binding", interner)?;
    let binding = ident_from_field(binding_field)?;

    Some(BracedOptioView {
        binding,
        ty: find_field(braced, "typus", interner).and_then(type_from_field),
        short: find_field(braced, "brevis", interner).and_then(string_from_field),
        long: find_field(braced, "longum", interner).and_then(string_from_field),
        description: find_field(braced, "descriptio", interner).and_then(string_from_field),
        global: find_field(braced, "ubique", interner)
            .is_some_and(|field| bool_from_field(field).unwrap_or(true)),
        default: find_field(braced, "vel", interner).and_then(expr_from_field),
    })
}

pub fn braced_optio_view_strict<'a>(
    braced: &'a BracedAnnotation,
    interner: &Interner,
) -> BracedView<BracedOptioView<'a>> {
    if !braced_family_is(braced, interner, "optio") {
        return BracedView::Absent;
    }

    let binding_field = match required_field(braced, interner, "@ optio", "binding", "identifier") {
        Ok(field) => field,
        Err(error) => return BracedView::Malformed(error),
    };
    let Some(binding) = ident_from_field(binding_field) else {
        return malformed("@ optio", "binding", "identifier", binding_field.span);
    };

    let ty = match optional_type_field(braced, interner, "@ optio", "typus") {
        Ok(ty) => ty,
        Err(error) => return BracedView::Malformed(error),
    };
    let short = match optional_string_field(braced, interner, "@ optio", "brevis") {
        Ok(short) => short,
        Err(error) => return BracedView::Malformed(error),
    };
    let long = match optional_string_field(braced, interner, "@ optio", "longum") {
        Ok(long) => long,
        Err(error) => return BracedView::Malformed(error),
    };
    let description = match optional_string_field(braced, interner, "@ optio", "descriptio") {
        Ok(description) => description,
        Err(error) => return BracedView::Malformed(error),
    };
    let global = match optional_bool_field(braced, interner, "@ optio", "ubique") {
        Ok(global) => global.unwrap_or(false),
        Err(error) => return BracedView::Malformed(error),
    };

    BracedView::Valid(BracedOptioView {
        binding,
        ty,
        short,
        long,
        description,
        global,
        default: find_field(braced, "vel", interner).and_then(expr_from_field),
    })
}

/// Borrowed `@ operandus` field view.
pub struct BracedOperandusView<'a> {
    pub is_rest: bool,
    pub ty: &'a TypeExpr,
    pub binding: &'a Ident,
    pub description: Option<radix_lexer::Symbol>,
    pub global: bool,
    pub default: Option<&'a Expr>,
}

pub fn braced_operandus_view<'a>(
    braced: &'a BracedAnnotation,
    interner: &Interner,
) -> Option<BracedOperandusView<'a>> {
    if !braced_family_is(braced, interner, "operandus") {
        return None;
    }

    let ty = find_field(braced, "typus", interner).and_then(type_from_field)?;
    let binding_field = find_field(braced, "binding", interner)?;
    let binding = ident_from_field(binding_field)?;

    Some(BracedOperandusView {
        is_rest: find_field(braced, "ceteri", interner)
            .is_some_and(|field| bool_from_field(field).unwrap_or(true)),
        ty,
        binding,
        description: find_field(braced, "descriptio", interner).and_then(string_from_field),
        global: find_field(braced, "ubique", interner)
            .is_some_and(|field| bool_from_field(field).unwrap_or(true)),
        default: find_field(braced, "vel", interner).and_then(expr_from_field),
    })
}

pub fn braced_operandus_view_strict<'a>(
    braced: &'a BracedAnnotation,
    interner: &Interner,
) -> BracedView<BracedOperandusView<'a>> {
    if !braced_family_is(braced, interner, "operandus") {
        return BracedView::Absent;
    }

    let ty_field = match required_field(braced, interner, "@ operandus", "typus", "type") {
        Ok(field) => field,
        Err(error) => return BracedView::Malformed(error),
    };
    let Some(ty) = type_from_field(ty_field) else {
        return malformed("@ operandus", "typus", "type", ty_field.span);
    };

    let binding_field =
        match required_field(braced, interner, "@ operandus", "binding", "identifier") {
            Ok(field) => field,
            Err(error) => return BracedView::Malformed(error),
        };
    let Some(binding) = ident_from_field(binding_field) else {
        return malformed("@ operandus", "binding", "identifier", binding_field.span);
    };

    let description = match optional_string_field(braced, interner, "@ operandus", "descriptio") {
        Ok(description) => description,
        Err(error) => return BracedView::Malformed(error),
    };
    let is_rest = match optional_bool_field(braced, interner, "@ operandus", "ceteri") {
        Ok(rest) => rest.unwrap_or(false),
        Err(error) => return BracedView::Malformed(error),
    };
    let global = match optional_bool_field(braced, interner, "@ operandus", "ubique") {
        Ok(global) => global.unwrap_or(false),
        Err(error) => return BracedView::Malformed(error),
    };

    BracedView::Valid(BracedOperandusView {
        is_rest,
        ty,
        binding,
        description,
        global,
        default: find_field(braced, "vel", interner).and_then(expr_from_field),
    })
}

fn find_field<'a>(
    braced: &'a BracedAnnotation,
    name: &str,
    interner: &Interner,
) -> Option<&'a AnnotationField> {
    braced
        .fields
        .iter()
        .find(|field| interner.resolve(field.name.name) == name)
}

fn required_field<'a>(
    braced: &'a BracedAnnotation,
    interner: &Interner,
    annotation: &'static str,
    field: &'static str,
    expected: &'static str,
) -> Result<&'a AnnotationField, BracedAnnotationError> {
    find_field(braced, field, interner).ok_or(BracedAnnotationError {
        annotation,
        field,
        expected,
        span: braced.name.span,
    })
}

fn string_field_strict(
    braced: &BracedAnnotation,
    interner: &Interner,
    family: &str,
    annotation: &'static str,
    field: &'static str,
) -> BracedView<radix_lexer::Symbol> {
    if !braced_family_is(braced, interner, family) {
        return BracedView::Absent;
    }
    let field_value = match required_field(braced, interner, annotation, field, "string literal") {
        Ok(field_value) => field_value,
        Err(error) => return BracedView::Malformed(error),
    };
    match string_from_field(field_value) {
        Some(value) => BracedView::Valid(value),
        None => malformed(annotation, field, "string literal", field_value.span),
    }
}

fn optional_string_field(
    braced: &BracedAnnotation,
    interner: &Interner,
    annotation: &'static str,
    field: &'static str,
) -> Result<Option<radix_lexer::Symbol>, BracedAnnotationError> {
    let Some(field_value) = find_field(braced, field, interner) else {
        return Ok(None);
    };
    string_from_field(field_value)
        .map(Some)
        .ok_or(BracedAnnotationError {
            annotation,
            field,
            expected: "string literal",
            span: field_value.span,
        })
}

fn optional_bool_field(
    braced: &BracedAnnotation,
    interner: &Interner,
    annotation: &'static str,
    field: &'static str,
) -> Result<Option<bool>, BracedAnnotationError> {
    let Some(field_value) = find_field(braced, field, interner) else {
        return Ok(None);
    };
    bool_from_field(field_value)
        .map(Some)
        .ok_or(BracedAnnotationError {
            annotation,
            field,
            expected: "boolean literal",
            span: field_value.span,
        })
}

fn optional_type_field<'a>(
    braced: &'a BracedAnnotation,
    interner: &Interner,
    annotation: &'static str,
    field: &'static str,
) -> Result<Option<&'a TypeExpr>, BracedAnnotationError> {
    let Some(field_value) = find_field(braced, field, interner) else {
        return Ok(None);
    };
    type_from_field(field_value)
        .map(Some)
        .ok_or(BracedAnnotationError {
            annotation,
            field,
            expected: "type",
            span: field_value.span,
        })
}

fn malformed<T>(
    annotation: &'static str,
    field: &'static str,
    expected: &'static str,
    span: Span,
) -> BracedView<T> {
    BracedView::Malformed(BracedAnnotationError {
        annotation,
        field,
        expected,
        span,
    })
}

fn expr_from_field(field: &AnnotationField) -> Option<&Expr> {
    match &field.value {
        AnnotationFieldValue::Expr(expr) => Some(expr),
        AnnotationFieldValue::Type(_) => None,
    }
}

fn type_from_field(field: &AnnotationField) -> Option<&TypeExpr> {
    match &field.value {
        AnnotationFieldValue::Type(ty) => Some(ty),
        AnnotationFieldValue::Expr(_) => None,
    }
}

fn string_from_field(field: &AnnotationField) -> Option<radix_lexer::Symbol> {
    let expr = expr_from_field(field)?;
    match &expr.kind {
        ExprKind::Literal(Literal::String(sym)) => Some(*sym),
        _ => None,
    }
}

fn ident_from_field(field: &AnnotationField) -> Option<&Ident> {
    let expr = expr_from_field(field)?;
    match &expr.kind {
        ExprKind::Ident(ident) => Some(ident),
        _ => None,
    }
}

fn bool_from_field(field: &AnnotationField) -> Option<bool> {
    let expr = expr_from_field(field)?;
    match expr.kind.literal() {
        Some(Literal::Bool(value)) => Some(*value),
        _ => None,
    }
}
