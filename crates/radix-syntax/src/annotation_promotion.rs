//! Sugar and braced views for promoted `@` annotation families.
//!
//! Owns `@ nondum`, `@ radix` (lane + typus), `@ verte`, and `@ protecta`
//! views from both braced and statement-form annotation kinds. Also owns the
//! `verte_rust_methodus` router for the Rust codegen boundary.

use crate::{
    Annotation, AnnotationField, AnnotationFieldValue, AnnotationKind, AnnotationStmt,
    BracedAnnotation, TypeExpr,
};
use radix_lexer::{Interner, Symbol, Token, TokenKind};

#[derive(Debug, Clone, Copy)]
pub enum AnnotationView<T> {
    Absent,
    Valid(T),
    Malformed(InvalidAnnotationView),
}

#[derive(Debug, Clone, Copy)]
pub struct InvalidAnnotationView {
    pub annotation: &'static str,
    pub issue: &'static str,
    pub span: radix_lexer::Span,
}

/// `@ nondum` field view.
pub struct NondumView {
    pub target: Option<Symbol>,
    pub ratio: Option<Symbol>,
    pub target_span: Option<radix_lexer::Span>,
    pub ratio_span: Option<radix_lexer::Span>,
}

/// Borrowed `@ radix { lane = STRING }` view.
pub struct RadixLaneView {
    pub lane: Symbol,
    pub span: radix_lexer::Span,
}

/// Borrowed `@ radix typus` braced/sugar-normalized view.
pub struct RadixTypusView<'a> {
    pub param: Symbol,
    pub allowed: Vec<&'a TypeExpr>,
    pub param_span: radix_lexer::Span,
}

/// Borrowed `@ verte` field view (rename or template spelling).
pub struct VerteView {
    pub target: Symbol,
    pub methodus: Option<Symbol>,
    /// Template sugar spelling when rename form does not apply.
    pub forma_spelling: Option<String>,
    pub target_span: radix_lexer::Span,
    pub payload_span: radix_lexer::Span,
}

pub fn braced_nondum_view(braced: &BracedAnnotation, interner: &Interner) -> Option<NondumView> {
    if !crate::braced_family_is(braced, interner, "nondum") {
        return None;
    }
    let target_field = find_field(braced, "target", interner);
    let ratio_field = find_field(braced, "ratio", interner);
    Some(NondumView {
        target: target_field.and_then(ident_symbol_from_field),
        ratio: ratio_field.and_then(string_from_field),
        target_span: target_field.map(|field| field.span),
        ratio_span: ratio_field.map(|field| field.span),
    })
}

#[must_use]
pub fn braced_radix_lane_view(
    braced: &BracedAnnotation,
    interner: &Interner,
) -> Option<RadixLaneView> {
    if !crate::braced_family_is(braced, interner, "radix") {
        return None;
    }
    let lane_field = find_field(braced, "lane", interner)?;
    let lane = string_from_field(lane_field)?;
    Some(RadixLaneView {
        lane,
        span: lane_field.span,
    })
}

#[must_use]
pub fn braced_radix_lane_view_strict(
    braced: &BracedAnnotation,
    interner: &Interner,
) -> AnnotationView<RadixLaneView> {
    if !crate::braced_family_is(braced, interner, "radix") {
        return AnnotationView::Absent;
    }
    let Some(lane_field) = find_field(braced, "lane", interner) else {
        return AnnotationView::Absent;
    };
    match string_from_field(lane_field) {
        Some(lane) => AnnotationView::Valid(RadixLaneView {
            lane,
            span: lane_field.span,
        }),
        None => AnnotationView::Malformed(InvalidAnnotationView {
            annotation: "@ radix lane",
            issue: "non_string_lane_value",
            span: lane_field.span,
        }),
    }
}

#[must_use]
pub fn braced_radix_typus_view<'a>(
    braced: &'a BracedAnnotation,
    interner: &Interner,
) -> Option<RadixTypusView<'a>> {
    if !crate::braced_family_is(braced, interner, "radix") {
        return None;
    }
    if find_field(braced, "lane", interner).is_some() {
        return None;
    }
    let param_field = find_field(braced, "param", interner)?;
    let param = ident_symbol_from_field(param_field)?;
    let allowed = braced
        .fields
        .iter()
        .filter(|field| interner.resolve(field.name.name) == "allowed")
        .filter_map(|field| type_from_field(field))
        .collect();
    Some(RadixTypusView {
        param,
        allowed,
        param_span: param_field.span,
    })
}

#[must_use]
pub fn braced_radix_typus_view_strict<'a>(
    braced: &'a BracedAnnotation,
    interner: &Interner,
) -> AnnotationView<RadixTypusView<'a>> {
    if !crate::braced_family_is(braced, interner, "radix") {
        return AnnotationView::Absent;
    }
    if find_field(braced, "lane", interner).is_some() {
        return AnnotationView::Absent;
    }
    if find_field(braced, "param", interner).is_none()
        && find_field(braced, "allowed", interner).is_none()
    {
        return AnnotationView::Malformed(InvalidAnnotationView {
            annotation: "@ radix",
            issue: "missing_directive",
            span: braced.name.span,
        });
    }

    let Some(param_field) = find_field(braced, "param", interner) else {
        return AnnotationView::Malformed(InvalidAnnotationView {
            annotation: "@ radix typus",
            issue: "missing_type_param",
            span: braced.name.span,
        });
    };
    let Some(param) = ident_symbol_from_field(param_field) else {
        return AnnotationView::Malformed(InvalidAnnotationView {
            annotation: "@ radix typus",
            issue: "non_identifier_type_param",
            span: param_field.span,
        });
    };

    let allowed_fields = braced
        .fields
        .iter()
        .filter(|field| interner.resolve(field.name.name) == "allowed")
        .collect::<Vec<_>>();
    if allowed_fields.is_empty() {
        return AnnotationView::Malformed(InvalidAnnotationView {
            annotation: "@ radix typus",
            issue: "empty_radix_typus_domain",
            span: braced.name.span,
        });
    }

    let mut allowed = Vec::with_capacity(allowed_fields.len());
    for field in allowed_fields {
        let Some(ty) = type_from_field(field) else {
            return AnnotationView::Malformed(InvalidAnnotationView {
                annotation: "@ radix typus",
                issue: "non_type_allowed",
                span: field.span,
            });
        };
        allowed.push(ty);
    }

    AnnotationView::Valid(RadixTypusView {
        param,
        allowed,
        param_span: param_field.span,
    })
}

pub fn braced_verte_view(braced: &BracedAnnotation, interner: &Interner) -> Option<VerteView> {
    if !crate::braced_family_is(braced, interner, "verte") {
        return None;
    }
    let target_field = find_field(braced, "target", interner)?;
    let target = ident_symbol_from_field(target_field)?;
    let methodus = find_field(braced, "methodus", interner).and_then(string_from_field);
    let forma_spelling = find_field(braced, "forma", interner)
        .and_then(string_from_field)
        .map(|sym| interner.resolve(sym).to_owned());
    let payload_span = methodus
        .and_then(|_| find_field(braced, "methodus", interner).map(|field| field.span))
        .or_else(|| find_field(braced, "forma", interner).map(|field| field.span))
        .unwrap_or(target_field.span);
    Some(VerteView {
        target,
        methodus,
        forma_spelling,
        target_span: target_field.span,
        payload_span,
    })
}

#[must_use]
pub fn nondum_from_stmt(stmt: &AnnotationStmt, interner: &Interner) -> Option<NondumView> {
    if interner.resolve(stmt.name.name) != "nondum" {
        return None;
    }
    let mut target = None;
    let mut ratio = None;
    let mut target_span = None;
    let mut ratio_span = None;
    for arg in &stmt.args {
        match arg.kind {
            TokenKind::Ident(symbol) if target.is_none() => {
                target = Some(symbol);
                target_span = Some(arg.span);
            }
            TokenKind::String(symbol) if ratio.is_none() => {
                ratio = Some(symbol);
                ratio_span = Some(arg.span);
            }
            _ => {}
        }
    }
    Some(NondumView {
        target,
        ratio,
        target_span,
        ratio_span,
    })
}

#[must_use]
pub fn radix_lane_from_stmt(stmt: &AnnotationStmt, interner: &Interner) -> Option<RadixLaneView> {
    if interner.resolve(stmt.name.name) != "radix" {
        return None;
    }
    let directive = stmt.args.first()?;
    let TokenKind::Ident(symbol) = directive.kind else {
        return None;
    };
    if interner.resolve(symbol) != "lane" {
        return None;
    }
    let value = stmt.args.get(1)?;
    let TokenKind::String(lane) = value.kind else {
        return None;
    };
    if stmt.args.len() > 2 {
        return None;
    }
    Some(RadixLaneView {
        lane,
        span: value.span,
    })
}

#[must_use]
pub fn verte_from_stmt(stmt: &AnnotationStmt, interner: &Interner) -> Option<VerteView> {
    if interner.resolve(stmt.name.name) != "verte" {
        return None;
    }
    let target_token = stmt.args.first()?;
    let TokenKind::Ident(target) = target_token.kind else {
        return None;
    };
    let payload = stmt.args.get(1)?;
    match payload.kind {
        TokenKind::String(methodus) => Some(VerteView {
            target,
            methodus: Some(methodus),
            forma_spelling: None,
            target_span: target_token.span,
            payload_span: payload.span,
        }),
        _ if stmt.args.len() > 1 => Some(VerteView {
            target,
            methodus: None,
            forma_spelling: Some(token_args_to_source(&stmt.args[1..], interner)),
            target_span: target_token.span,
            payload_span: payload.span,
        }),
        _ => None,
    }
}

#[must_use]
pub fn annotation_has_protecta(annotation: &Annotation, interner: &Interner) -> bool {
    match &annotation.kind {
        AnnotationKind::Protecta => true,
        AnnotationKind::Statement(stmt) => {
            interner.resolve(stmt.name.name) == "protecta" && stmt.args.is_empty()
        }
        AnnotationKind::Braced(braced) => {
            crate::braced_family_is(braced, interner, "protecta") && braced.fields.is_empty()
        }
        _ => false,
    }
}

#[must_use]
pub fn verte_rust_methodus(annotation: &Annotation, interner: &Interner) -> Option<String> {
    match &annotation.kind {
        AnnotationKind::Statement(stmt) => verte_from_stmt(stmt, interner).and_then(|view| {
            if interner.resolve(view.target) != "rs" {
                return None;
            }
            view.methodus.map(|sym| interner.resolve(sym).to_owned())
        }),
        AnnotationKind::Braced(braced) => braced_verte_view(braced, interner).and_then(|view| {
            if interner.resolve(view.target) != "rs" {
                return None;
            }
            view.methodus.map(|sym| interner.resolve(sym).to_owned())
        }),
        _ => None,
    }
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

fn string_from_field(field: &AnnotationField) -> Option<Symbol> {
    let expr = expr_from_field(field)?;
    match &expr.kind {
        crate::ExprKind::Literal(crate::Literal::String(sym)) => Some(*sym),
        _ => None,
    }
}

fn ident_symbol_from_field(field: &AnnotationField) -> Option<Symbol> {
    let expr = expr_from_field(field)?;
    match &expr.kind {
        crate::ExprKind::Ident(ident) => Some(ident.name),
        _ => None,
    }
}

fn type_from_field(field: &AnnotationField) -> Option<&TypeExpr> {
    match &field.value {
        AnnotationFieldValue::Type(ty) => Some(ty),
        AnnotationFieldValue::Expr(_) => None,
    }
}

fn expr_from_field(field: &AnnotationField) -> Option<&crate::Expr> {
    match &field.value {
        AnnotationFieldValue::Expr(expr) => Some(expr),
        AnnotationFieldValue::Type(_) => None,
    }
}

fn token_args_to_source(args: &[Token], interner: &Interner) -> String {
    let mut out = String::new();
    for (index, token) in args.iter().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        out.push_str(&token_to_source(token, interner));
    }
    out
}

fn token_to_source(token: &Token, interner: &Interner) -> String {
    match &token.kind {
        TokenKind::String(sym) | TokenKind::AsciiString(sym) | TokenKind::BacktickString(sym) => {
            format!("\"{}\"", interner.resolve(*sym))
        }
        TokenKind::Ident(sym) | TokenKind::Underscore(sym) => interner.resolve(*sym).to_owned(),
        TokenKind::Integer(value) => value.to_string(),
        TokenKind::Float(value) => value.to_string(),
        TokenKind::Verum => "verum".to_owned(),
        TokenKind::Falsum => "falsum".to_owned(),
        TokenKind::Nihil => "nihil".to_owned(),
        TokenKind::LParen => "(".to_owned(),
        TokenKind::RParen => ")".to_owned(),
        TokenKind::Comma => ",".to_owned(),
        TokenKind::Arrow => "→".to_owned(),
        TokenKind::ExitArrow => "⇥".to_owned(),
        other => format!("{other:?}"),
    }
}
