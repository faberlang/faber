//! Compiler-owned contract marker and the experimental field-free EVM profile.
//!
//! The marker retains normal HIR annotations. Profile restrictions are explicit
//! refusals, so admitting the marker does not promise object or storage support.

use crate::{Primitive, SemanticError, SemanticErrorKind, Type, TypeId, TypeTable};
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{HirExpression, HirExpressionKind, HirItem, HirItemKind, HirModule, HirParamMode};
use radix_lexer::{Interner, Span};
use radix_syntax::{
    Annotation, AnnotationFieldValue, AnnotationKind, ClassMemberKind, ExprKind, Literal, Program,
    StmtKind,
};

/// Whether an annotation names the locale-invariant compiler contract family.
#[must_use]
pub fn is_contract_annotation(annotation: &Annotation, interner: &Interner) -> bool {
    match &annotation.kind {
        AnnotationKind::Braced(value) => interner.resolve(value.name.name) == "contract",
        AnnotationKind::Statement(value) => interner.resolve(value.name.name) == "contract",
        _ => false,
    }
}

fn error(span: Span, issue: &str, reason: &str) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::AnnotationInvalid, span)
        .with_arg("issue", issue)
        .with_arg("reason", reason)
}

/// Validate the compiler-owned marker independently of user annotation schemas.
#[must_use]
pub fn validate_syntax(program: &Program, interner: &Interner) -> Vec<SemanticError> {
    let mut errors = Vec::new();
    let mut classes = 0;
    for stmt in &program.statements {
        for annotation in &stmt.annotations {
            if !is_contract_annotation(annotation, interner) {
                continue;
            }
            if !matches!(stmt.kind, StmtKind::Class(_)) {
                errors.push(error(
                    annotation.span,
                    "contract_annotation_invalid",
                    "class_required",
                ));
                continue;
            }
            classes += 1;
            if classes > 1 {
                errors.push(error(
                    annotation.span,
                    "evm_contract_unsupported",
                    "multiple_contracts",
                ));
            }
            let AnnotationKind::Braced(braced) = &annotation.kind else {
                errors.push(error(
                    annotation.span,
                    "contract_annotation_invalid",
                    "target_payload_required",
                ));
                continue;
            };
            let valid = braced.fields.len() == 1
                && interner.resolve(braced.fields[0].name.name) == "target"
                && matches!(&braced.fields[0].value,
                    AnnotationFieldValue::Expr(expr)
                    if matches!(&expr.kind, ExprKind::Literal(Literal::String(value))
                        if interner.resolve(*value) == "evm"));
            if !valid {
                errors.push(error(
                    annotation.span,
                    "contract_annotation_invalid",
                    "evm_target_payload_required",
                ));
            }
        }
        // Methods and fields cannot select a contract environment.
        let annotations: Vec<_> = match &stmt.kind {
            StmtKind::Func(func) => func.annotations.iter().collect(),
            StmtKind::Class(class) => class
                .members
                .iter()
                .flat_map(|member| {
                    member.annotations.iter().chain(match &member.kind {
                        ClassMemberKind::Method(method) => method.annotations.iter(),
                        ClassMemberKind::Field(_) => [].iter(),
                    })
                })
                .collect(),
            _ => Vec::new(),
        };
        for annotation in annotations {
            if is_contract_annotation(annotation, interner) {
                errors.push(error(
                    annotation.span,
                    "contract_annotation_invalid",
                    "class_required",
                ));
            }
        }
    }
    errors
}

/// Exact selected class identity, retained through the ordinary HIR pipeline.
#[must_use]
pub fn selected<'a>(hir: &'a HirModule, interner: &Interner) -> Option<&'a HirItem> {
    hir.items.iter().find(|item| {
        item.annotations.iter().any(|annotation| {
            interner.resolve(annotation.family) == "contract" && annotation.contract_id.is_none()
        })
    })
}

/// Refuse unsupported profile shapes after signature types have been resolved.
#[must_use]
pub fn validate_hir(hir: &HirModule, types: &TypeTable, interner: &Interner) -> Vec<SemanticError> {
    let Some(item) = selected(hir, interner) else {
        return Vec::new();
    };
    let HirItemKind::Struct(class) = &item.kind else {
        return Vec::new();
    };
    let mut errors = Vec::new();
    let mut refuse = |span, reason| errors.push(error(span, "evm_contract_unsupported", reason));
    if !class.fields.is_empty() {
        refuse(item.span, "fields");
    }
    if !class.type_params.is_empty() {
        refuse(item.span, "generic_class");
    }
    if !class.implements.is_empty() {
        refuse(item.span, "interface_conformance");
    }
    let mut names = std::collections::BTreeSet::new();
    let mut entries = 0;
    for method in &class.methods {
        let function = &method.func;
        if !names.insert(interner.resolve(function.name)) {
            refuse(method.span, "overloaded_method");
        }
        if !function.type_params.is_empty() {
            refuse(method.span, "generic_method");
        }
        if function.is_async || function.is_generator {
            refuse(method.span, "async_or_generator");
        }
        if function.err_ty.is_some() {
            refuse(method.span, "error_channel");
        }
        if function.body.is_none() {
            refuse(method.span, "method_body_or_receiver");
        }
        if function.params.iter().any(|param| {
            !scalar(types, param.ty)
                || param.optional
                || param.default.is_some()
                || param.mode != HirParamMode::Owned
        }) {
            refuse(method.span, "parameter_shape");
        }
        if function.ret_ty.is_none_or(|ty| {
            !scalar(types, ty) && !matches!(types.get(ty), Type::Primitive(Primitive::Vacuum))
        }) {
            refuse(method.span, "return_shape");
        }
        if method
            .annotations
            .iter()
            .any(|annotation| interner.resolve(annotation.family) == "publica")
        {
            entries += 1;
        }
    }
    if entries == 0 {
        refuse(item.span, "no_public_entries");
    }
    let mut constructions = Constructions {
        class: item.def_id,
        types,
        errors: &mut errors,
    };
    constructions.visit_program(hir);
    errors
}

struct Constructions<'a> {
    class: radix_hir::DefId,
    types: &'a TypeTable,
    errors: &'a mut Vec<SemanticError>,
}

impl HirVisitor for Constructions<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        let constructs = match &expr.kind {
            HirExpressionKind::Struct(class, _) => *class == self.class,
            HirExpressionKind::Verte {
                target,
                entries: Some(_),
                ..
            } => {
                matches!(self.types.get(*target), Type::Struct(class) if *class == self.class)
            }
            _ => false,
        };
        if constructs {
            self.errors
                .push(error(expr.span, "evm_contract_unsupported", "construction"));
        }
        walk_expr(self, expr);
    }
}

fn scalar(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(ty) {
        Type::Primitive(Primitive::Numerus | Primitive::Bivalens) => true,
        Type::SizedNumeric(Primitive::Numerus, width)
        | Type::ModularWord(width)
        | Type::SaturatingWord(width) => matches!(
            width,
            radix_types::NumericWidth::I8
                | radix_types::NumericWidth::I16
                | radix_types::NumericWidth::I32
                | radix_types::NumericWidth::I64
                | radix_types::NumericWidth::U8
                | radix_types::NumericWidth::U16
                | radix_types::NumericWidth::U32
                | radix_types::NumericWidth::U64
        ),
        Type::Alias(_, inner) => scalar(types, *inner),
        _ => false,
    }
}
