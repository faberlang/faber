//! Compiler-owned `@ json` genus contract validation.
//!
//! This is intentionally separate from generic `@ annotatio`: JSON genera are a
//! built-in data-model contract with stricter-than-`valor` field rules.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_hir::{HirAnnotation, HirAnnotationValue, HirExpressionKind, HirField, HirLiteral};

const ISSUE_JSON_ANNOTATION_UNSUPPORTED_SITE: &str = "json_annotation_unsupported_site";
const ISSUE_JSON_FIELD_ANNOTATION_OUTSIDE_JSON_GENUS: &str =
    "json_field_annotation_outside_json_genus";
const ISSUE_JSON_GENUS_FIELD_TYPE_UNSUPPORTED: &str = "json_genus_field_type_unsupported";
const ISSUE_JSON_GENUS_MAP_KEY_NOT_TEXTUS: &str = "json_genus_map_key_not_textus";
const ISSUE_JSON_GENUS_NESTED_REQUIRES_JSON: &str = "json_genus_nested_requires_json";
const ISSUE_JSON_FIELD_ANNOTATION_OPTION: &str = "json_field_annotation_option";
const ISSUE_JSON_FIELD_NOMEN_EMPTY: &str = "json_field_nomen_empty";
const ISSUE_JSON_GENUS_DUPLICATE_FIELD_NAME: &str = "json_genus_duplicate_field_name";

impl TypeChecker<'_> {
    pub fn annotations_contain_json(&self, annotations: &[HirAnnotation]) -> bool {
        annotations
            .iter()
            .any(|annotation| self.annotation_is_json(annotation))
    }

    pub fn reject_json_annotations_on_site(
        &mut self,
        annotations: &[HirAnnotation],
        site: &'static str,
    ) {
        for annotation in annotations {
            if self.annotation_is_json(annotation) {
                self.errors.push(
                    SemanticError::structured(
                        SemanticErrorKind::AnnotationInvalid,
                        annotation.span,
                    )
                    .with_arg("issue", ISSUE_JSON_ANNOTATION_UNSUPPORTED_SITE)
                    .with_arg("site", site),
                );
            }
        }
    }

    pub fn check_json_struct_contract(&mut self, def_id: DefId, strukt: &HirStruct) {
        let is_json = self.json_structs.contains(&def_id);
        let mut output_names = FxHashMap::default();

        for field in &strukt.fields {
            let json_field = self.json_field_nomen(field);
            if !is_json && json_field.is_some() {
                self.error_issue(
                    SemanticErrorKind::AnnotationInvalid,
                    ISSUE_JSON_FIELD_ANNOTATION_OUTSIDE_JSON_GENUS,
                    field.span,
                );
                continue;
            }

            if !is_json {
                continue;
            }

            if field.is_static() {
                self.error_issue(
                    SemanticErrorKind::AnnotationInvalid,
                    ISSUE_JSON_GENUS_FIELD_TYPE_UNSUPPORTED,
                    field.span,
                );
                continue;
            }

            self.check_json_safe_type(field.ty, field.span);
            let output_name = match json_field {
                Some(name) => name,
                None => field.name,
            };
            if let Some(previous) = output_names.insert(output_name, field.span) {
                let name = self.resolve_symbol_for_diag(output_name);
                self.errors.push(
                    SemanticError::structured(SemanticErrorKind::AnnotationInvalid, field.span)
                        .with_arg("issue", ISSUE_JSON_GENUS_DUPLICATE_FIELD_NAME)
                        .with_arg("name", name)
                        .with_arg("previous_span", format!("{previous:?}")),
                );
            }
        }
    }

    fn check_json_safe_type(&mut self, ty: TypeId, span: radix_lexer::Span) {
        let resolved = self.resolve_inferred_type(ty);
        match self.types.get(resolved).clone() {
            Type::Primitive(
                Primitive::Nihil
                | Primitive::Bivalens
                | Primitive::Numerus
                | Primitive::Fractus
                | Primitive::Textus
                | Primitive::Ascii
                | Primitive::Instans,
            )
            | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
            | Type::SizedInstans(_)
            | Type::Error
            | Type::Infer(_) => {}
            Type::Array(inner) | Type::Option(inner) | Type::Alias(_, inner) => {
                self.check_json_safe_type(inner, span);
            }
            Type::Map(key, value) => {
                if !matches!(
                    self.types.get(self.resolve_inferred_type(key)),
                    Type::Primitive(Primitive::Textus)
                ) {
                    self.error_issue(
                        SemanticErrorKind::AnnotationInvalid,
                        ISSUE_JSON_GENUS_MAP_KEY_NOT_TEXTUS,
                        span,
                    );
                }
                self.check_json_safe_type(value, span);
            }
            Type::Union(members) => self.check_json_union(members, span),
            Type::Struct(nested) if self.json_structs.contains(&nested) => {}
            Type::Struct(_) => {
                self.error_issue(
                    SemanticErrorKind::AnnotationInvalid,
                    ISSUE_JSON_GENUS_NESTED_REQUIRES_JSON,
                    span,
                );
            }
            _ => {
                self.error_issue(
                    SemanticErrorKind::AnnotationInvalid,
                    ISSUE_JSON_GENUS_FIELD_TYPE_UNSUPPORTED,
                    span,
                );
            }
        }
    }

    // consumes Vec by iteration
    fn check_json_union(&mut self, members: Vec<TypeId>, span: radix_lexer::Span) {
        let nihil = self.types.primitive(Primitive::Nihil);
        let mut non_nil = Vec::new();
        for member in members {
            let resolved = self.resolve_inferred_type(member);
            if self.types.equals(resolved, nihil) {
                continue;
            }
            non_nil.push(resolved);
        }
        if non_nil.len() != 1 {
            self.error_issue(
                SemanticErrorKind::AnnotationInvalid,
                ISSUE_JSON_GENUS_FIELD_TYPE_UNSUPPORTED,
                span,
            );
            return;
        }
        self.check_json_safe_type(non_nil[0], span);
    }

    fn json_field_nomen(&mut self, field: &HirField) -> Option<Symbol> {
        let annotation = field
            .annotations
            .iter()
            .find(|annotation| self.annotation_is_json(annotation))?;
        if annotation.fields.len() != 1 {
            self.error_issue(
                SemanticErrorKind::AnnotationInvalid,
                ISSUE_JSON_FIELD_ANNOTATION_OPTION,
                annotation.span,
            );
            return None;
        }
        let option = &annotation.fields[0];
        if self.resolve_symbol_for_diag(option.name) != "nomen" {
            self.error_issue(
                SemanticErrorKind::AnnotationInvalid,
                ISSUE_JSON_FIELD_ANNOTATION_OPTION,
                option.span,
            );
            return None;
        }
        let Some(name) = Self::annotation_string_value(&option.value) else {
            self.error_issue(
                SemanticErrorKind::AnnotationInvalid,
                ISSUE_JSON_FIELD_ANNOTATION_OPTION,
                option.span,
            );
            return None;
        };
        if self.resolve_symbol_for_diag(name).is_empty() {
            self.error_issue(
                SemanticErrorKind::AnnotationInvalid,
                ISSUE_JSON_FIELD_NOMEN_EMPTY,
                option.span,
            );
            return None;
        }
        Some(name)
    }

    fn annotation_string_value(value: &HirAnnotationValue) -> Option<Symbol> {
        match value {
            HirAnnotationValue::String(symbol) => Some(*symbol),
            HirAnnotationValue::Expr(expr) => match &expr.kind {
                HirExpressionKind::Literal(HirLiteral::String(symbol)) => Some(*symbol),
                _ => None,
            },
            _ => None,
        }
    }

    fn annotation_is_json(&self, annotation: &HirAnnotation) -> bool {
        self.interner
            .is_some_and(|interner| interner.resolve(annotation.family) == "json")
    }

    fn resolve_symbol_for_diag(&self, symbol: Symbol) -> String {
        self.interner.map_or_else(
            || format!("sym#{}", symbol.0),
            |interner| interner.resolve(symbol).to_owned(),
        )
    }
}
