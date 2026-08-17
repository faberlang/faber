//! Lower parsed `@` metadata into normalized [`HirAnnotation`] records.
//!
//! This module owns the annotation syntax-to-HIR normalization for built-in annotation
//! families (cli, imperium, optio, operandus, nondum, radix, verte) and braced/sugar forms.
//! It does not resolve annotation contracts, validate field types against contract schemas,
//! or decide annotation semantics — those belong to semantic analysis and later phases.

use super::Lowerer;
use crate::hir::{
    HirAnnotation, HirAnnotationField, HirAnnotationValue, HirAvailability, HirTypeParam, HirTypeParamConstraint,
};
use crate::lexer::{Interner, Span, Symbol};
use crate::semantic::{ContractPayloadValue, Primitive, ResolvedContractApplication, Type, TypeId};
use crate::syntax::{
    braced_cli_nomen, braced_imperium_nomen, braced_nondum_view, braced_operandus_view, braced_optio_view,
    braced_radix_lane_view, braced_radix_typus_view, braced_verte_view, nondum_from_stmt, radix_lane_from_stmt,
    verte_from_stmt, Annotation, AnnotationField, AnnotationFieldValue, AnnotationKind, AnnotationStmt,
    BracedAnnotation, CliAnnotation, ImperiumAnnotation, OperandusAnnotation, OptioAnnotation,
    RadixTypusDomainAnnotation,
};

fn lower_contract_payload(value: &ContractPayloadValue) -> HirAnnotationValue {
    match value {
        ContractPayloadValue::String(sym) => HirAnnotationValue::String(*sym),
        ContractPayloadValue::Int(n) => HirAnnotationValue::Int(*n),
        ContractPayloadValue::Float(n) => HirAnnotationValue::Float(*n),
        ContractPayloadValue::Bool(b) => HirAnnotationValue::Bool(*b),
        ContractPayloadValue::Nihil => HirAnnotationValue::Nihil,
    }
}

/// Try to parse a `nondum` annotation into an [`HirAvailability`] record.
///
/// Returns `None` when the annotation family is not `"nondum"`.
fn try_parse_nondum_annotation(annotation: &HirAnnotation, interner: &Interner) -> Option<HirAvailability> {
    if interner.resolve(annotation.family) != "nondum" {
        return None;
    }

    let mut target = None;
    let mut reason = None;
    for field in &annotation.fields {
        match interner.resolve(field.name) {
            "target" => {
                if let HirAnnotationValue::Symbol(symbol) = field.value {
                    target = Some(symbol);
                }
            }
            "ratio" => {
                if let HirAnnotationValue::String(symbol) = field.value {
                    reason = Some(symbol);
                }
            }
            _ => {}
        }
    }

    Some(HirAvailability::Nondum { target, reason })
}

impl Lowerer<'_> {
    pub(super) fn lower_annotations(&mut self, annotations: &[Annotation]) -> Vec<HirAnnotation> {
        annotations
            .iter()
            .map(|annotation| self.lower_annotation(annotation))
            .collect()
    }

    pub(super) fn lower_annotation(&mut self, annotation: &Annotation) -> HirAnnotation {
        if let Some(resolved) = self
            .annotation_contracts
            .and_then(|meta| meta.application_at(annotation.span))
        {
            return self.lower_resolved_contract(resolved, annotation.span);
        }

        match &annotation.kind {
            AnnotationKind::Statement(stmt) => self.lower_statement_annotation(stmt, annotation.span),
            AnnotationKind::RadixTypusDomain(domain) => self.lower_radix_typus_domain(domain, annotation.span),
            AnnotationKind::RadixLane(lane) => {
                let radix_sym = self.interner.intern("radix");
                self.lower_radix_lane_fields(radix_sym, lane.lane, annotation.span)
            }
            AnnotationKind::Cli(cli) => self.lower_cli_annotation(cli, annotation.span),
            AnnotationKind::Imperium(imperium) => self.lower_imperium_annotation(imperium, annotation.span),
            AnnotationKind::Optio(optio) => self.lower_optio_annotation(optio, annotation.span),
            AnnotationKind::Operandus(operandus) => self.lower_operandus_annotation(operandus, annotation.span),
            AnnotationKind::Futura => self.unit_annotation("futura", annotation.span),
            AnnotationKind::Cursor => self.unit_annotation("cursor", annotation.span),
            AnnotationKind::Tag => self.unit_annotation("tag", annotation.span),
            AnnotationKind::Solum => self.unit_annotation("solum", annotation.span),
            AnnotationKind::Omitte => self.unit_annotation("omitte", annotation.span),
            AnnotationKind::Metior => self.unit_annotation("metior", annotation.span),
            AnnotationKind::Publica => self.unit_annotation("publica", annotation.span),
            AnnotationKind::Protecta => self.unit_annotation("protecta", annotation.span),
            AnnotationKind::Privata => self.unit_annotation("privata", annotation.span),
            AnnotationKind::Interna => self.unit_annotation("interna", annotation.span),
            AnnotationKind::Braced(braced) => self.lower_braced_annotation(braced, annotation.span),
        }
    }

    fn lower_resolved_contract(&self, resolved: &ResolvedContractApplication, span: Span) -> HirAnnotation {
        let fields = resolved
            .fields
            .iter()
            .map(|field| HirAnnotationField {
                name: field.name,
                value: lower_contract_payload(&field.value),
                span: field.span,
            })
            .collect();

        HirAnnotation {
            family: resolved.family,
            fields,
            raw_args: Vec::new(),
            contract_id: Some(resolved.contract_id),
            span,
        }
    }

    fn lower_braced_annotation(&mut self, braced: &BracedAnnotation, span: Span) -> HirAnnotation {
        let family_owned = self.interner.resolve(braced.name.name).to_owned();

        if let Some(nomen) = braced_cli_nomen(braced, self.interner) {
            return HirAnnotation {
                family: braced.name.name,
                fields: vec![HirAnnotationField {
                    name: self.intern_field_name("nomen"),
                    value: HirAnnotationValue::String(nomen),
                    span,
                }],
                raw_args: Vec::new(),
                contract_id: None,
                span,
            };
        }

        if let Some(nomen) = braced_imperium_nomen(braced, self.interner) {
            return HirAnnotation {
                family: braced.name.name,
                fields: vec![HirAnnotationField {
                    name: self.intern_field_name("nomen"),
                    value: HirAnnotationValue::String(nomen),
                    span,
                }],
                raw_args: Vec::new(),
                contract_id: None,
                span,
            };
        }

        if let Some(view) = braced_optio_view(braced, self.interner) {
            return self.lower_braced_optio(braced.name.name, view, span);
        }

        if let Some(view) = braced_operandus_view(braced, self.interner) {
            return self.lower_braced_operandus(braced.name.name, view, span);
        }

        if let Some(view) = braced_nondum_view(braced, self.interner) {
            return self.lower_nondum_fields(braced.name.name, view.target, view.ratio, span);
        }

        if let Some(view) = braced_radix_lane_view(braced, self.interner) {
            return self.lower_radix_lane_fields(braced.name.name, view.lane, span);
        }

        if let Some(view) = braced_radix_typus_view(braced, self.interner) {
            return self.lower_braced_radix_typus(braced.name.name, view, span);
        }

        if let Some(view) = braced_verte_view(braced, self.interner) {
            return self.lower_verte_fields(
                braced.name.name,
                view.target,
                view.methodus,
                view.forma_spelling.as_deref(),
                span,
            );
        }

        if matches!(
            family_owned.as_str(),
            "futura"
                | "cursor"
                | "tag"
                | "solum"
                | "omitte"
                | "metior"
                | "publica"
                | "protecta"
                | "privata"
                | "interna"
        ) && braced.fields.is_empty()
        {
            return self.unit_annotation(&family_owned, span);
        }

        HirAnnotation {
            family: braced.name.name,
            fields: braced
                .fields
                .iter()
                .filter_map(|field| self.lower_braced_field(field))
                .collect(),
            raw_args: Vec::new(),
            contract_id: None,
            span,
        }
    }

    fn lower_braced_optio(
        &mut self,
        family: Symbol,
        view: crate::syntax::BracedOptioView<'_>,
        span: Span,
    ) -> HirAnnotation {
        let mut fields = vec![HirAnnotationField {
            name: self.intern_field_name("binding"),
            value: HirAnnotationValue::Symbol(view.binding.name),
            span: view.binding.span,
        }];

        if let Some(short) = view.short {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("brevis"),
                value: HirAnnotationValue::String(short),
                span,
            });
        }
        if let Some(long) = view.long {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("longum"),
                value: HirAnnotationValue::String(long),
                span,
            });
        }
        if let Some(ty) = view.ty {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("typus"),
                value: HirAnnotationValue::Type(self.lower_type(ty)),
                span: ty.span,
            });
        }
        if let Some(description) = view.description {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("descriptio"),
                value: HirAnnotationValue::String(description),
                span,
            });
        }
        if view.global {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("ubique"),
                value: HirAnnotationValue::Bool(true),
                span,
            });
        }
        if let Some(default) = view.default {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("vel"),
                value: HirAnnotationValue::Expr(Box::new(self.lower_expr(default))),
                span: default.span,
            });
        }

        HirAnnotation::with_fields(family, fields, span)
    }

    fn lower_braced_operandus(
        &mut self,
        family: Symbol,
        view: crate::syntax::BracedOperandusView<'_>,
        span: Span,
    ) -> HirAnnotation {
        let mut fields = Vec::new();
        if view.rest {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("ceteri"),
                value: HirAnnotationValue::Bool(true),
                span,
            });
        }
        fields.push(HirAnnotationField {
            name: self.intern_field_name("typus"),
            value: HirAnnotationValue::Type(self.lower_type(view.ty)),
            span: view.ty.span,
        });
        fields.push(HirAnnotationField {
            name: self.intern_field_name("binding"),
            value: HirAnnotationValue::Symbol(view.binding.name),
            span: view.binding.span,
        });
        if let Some(description) = view.description {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("descriptio"),
                value: HirAnnotationValue::String(description),
                span,
            });
        }
        if view.global {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("ubique"),
                value: HirAnnotationValue::Bool(true),
                span,
            });
        }
        if let Some(default) = view.default {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("vel"),
                value: HirAnnotationValue::Expr(Box::new(self.lower_expr(default))),
                span: default.span,
            });
        }

        HirAnnotation::with_fields(family, fields, span)
    }

    fn lower_braced_field(&mut self, field: &AnnotationField) -> Option<HirAnnotationField> {
        let name = field.name.name;
        let value = match &field.value {
            AnnotationFieldValue::Expr(expr) => HirAnnotationValue::Expr(Box::new(self.lower_expr(expr))),
            AnnotationFieldValue::Type(ty) => HirAnnotationValue::Type(self.lower_type(ty)),
        };
        Some(HirAnnotationField { name, value, span: field.span })
    }

    fn unit_annotation(&mut self, family: &str, span: Span) -> HirAnnotation {
        HirAnnotation::with_fields(self.intern_field_name(family), Vec::new(), span)
    }

    fn lower_statement_annotation(&mut self, stmt: &AnnotationStmt, span: Span) -> HirAnnotation {
        let family = stmt.name.name;
        let family_name = self.interner.resolve(family);

        if let Some(view) = nondum_from_stmt(stmt, self.interner) {
            return self.lower_nondum_fields(family, view.target, view.ratio, span);
        }

        if let Some(view) = radix_lane_from_stmt(stmt, self.interner) {
            return self.lower_radix_lane_fields(family, view.lane, span);
        }

        if let Some(view) = verte_from_stmt(stmt, self.interner) {
            return self.lower_verte_fields(family, view.target, view.methodus, view.forma_spelling.as_deref(), span);
        }

        if matches!(
            family_name,
            "futura" | "cursor" | "nucleum" | "machina" | "vertex" | "fragment" | "solum" | "omitte" | "metior"
        ) && stmt.args.is_empty()
        {
            return HirAnnotation::with_fields(family, Vec::new(), span);
        }

        HirAnnotation { family, fields: Vec::new(), raw_args: stmt.args.clone(), contract_id: None, span }
    }

    fn lower_nondum_fields(
        &mut self,
        family: Symbol,
        target: Option<Symbol>,
        ratio: Option<Symbol>,
        span: Span,
    ) -> HirAnnotation {
        let mut fields = Vec::new();
        if let Some(target) = target {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("target"),
                value: HirAnnotationValue::Symbol(target),
                span,
            });
        }
        if let Some(ratio) = ratio {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("ratio"),
                value: HirAnnotationValue::String(ratio),
                span,
            });
        }
        HirAnnotation::with_fields(family, fields, span)
    }

    fn lower_radix_lane_fields(&mut self, family: Symbol, lane: Symbol, span: Span) -> HirAnnotation {
        HirAnnotation {
            family,
            fields: vec![HirAnnotationField {
                name: self.intern_field_name("lane"),
                value: HirAnnotationValue::String(lane),
                span,
            }],
            raw_args: Vec::new(),
            contract_id: None,
            span,
        }
    }

    fn lower_braced_radix_typus(
        &mut self,
        family: Symbol,
        view: crate::syntax::RadixTypusView<'_>,
        span: Span,
    ) -> HirAnnotation {
        let mut fields = vec![HirAnnotationField {
            name: self.intern_field_name("param"),
            value: HirAnnotationValue::Symbol(view.param),
            span: view.param_span,
        }];
        for ty in view.allowed {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("allowed"),
                value: HirAnnotationValue::Type(self.lower_type(ty)),
                span: ty.span,
            });
        }
        HirAnnotation::with_fields(family, fields, span)
    }

    fn lower_verte_fields(
        &mut self,
        family: Symbol,
        target: Symbol,
        methodus: Option<Symbol>,
        forma_spelling: Option<&str>,
        span: Span,
    ) -> HirAnnotation {
        let mut fields = vec![HirAnnotationField {
            name: self.intern_field_name("target"),
            value: HirAnnotationValue::Symbol(target),
            span,
        }];
        if let Some(methodus) = methodus {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("methodus"),
                value: HirAnnotationValue::String(methodus),
                span,
            });
        } else if let Some(forma_spelling) = forma_spelling {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("forma"),
                value: HirAnnotationValue::String(self.interner.intern(forma_spelling)),
                span,
            });
        }
        HirAnnotation::with_fields(family, fields, span)
    }

    fn lower_cli_annotation(&mut self, cli: &CliAnnotation, span: Span) -> HirAnnotation {
        HirAnnotation {
            family: self.intern_field_name("cli"),
            fields: vec![HirAnnotationField {
                name: self.intern_field_name("nomen"),
                value: HirAnnotationValue::String(cli.name),
                span,
            }],
            raw_args: Vec::new(),
            contract_id: None,
            span,
        }
    }

    fn lower_imperium_annotation(&mut self, imperium: &ImperiumAnnotation, span: Span) -> HirAnnotation {
        HirAnnotation {
            family: self.intern_field_name("imperium"),
            fields: vec![HirAnnotationField {
                name: self.intern_field_name("nomen"),
                value: HirAnnotationValue::String(imperium.name),
                span,
            }],
            raw_args: Vec::new(),
            contract_id: None,
            span,
        }
    }

    fn lower_optio_annotation(&mut self, optio: &OptioAnnotation, span: Span) -> HirAnnotation {
        let mut fields = vec![HirAnnotationField {
            name: self.intern_field_name("binding"),
            value: HirAnnotationValue::Symbol(optio.binding.name),
            span: optio.binding.span,
        }];

        if let Some(short) = optio.short {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("brevis"),
                value: HirAnnotationValue::String(short),
                span,
            });
        }
        if let Some(long) = optio.long {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("longum"),
                value: HirAnnotationValue::String(long),
                span,
            });
        }
        if let Some(ty) = &optio.ty {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("typus"),
                value: HirAnnotationValue::Type(self.lower_type(ty)),
                span: ty.span,
            });
        }
        if let Some(description) = optio.description {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("descriptio"),
                value: HirAnnotationValue::String(description),
                span,
            });
        }
        if optio.global {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("ubique"),
                value: HirAnnotationValue::Bool(true),
                span,
            });
        }
        if let Some(default) = &optio.default {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("vel"),
                value: HirAnnotationValue::Expr(Box::new(self.lower_expr(default))),
                span: default.span,
            });
        }

        HirAnnotation::with_fields(self.intern_field_name("optio"), fields, span)
    }

    fn lower_operandus_annotation(&mut self, operandus: &OperandusAnnotation, span: Span) -> HirAnnotation {
        let mut fields = Vec::new();
        if operandus.rest {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("ceteri"),
                value: HirAnnotationValue::Bool(true),
                span,
            });
        }
        fields.push(HirAnnotationField {
            name: self.intern_field_name("typus"),
            value: HirAnnotationValue::Type(self.lower_type(&operandus.ty)),
            span: operandus.ty.span,
        });
        fields.push(HirAnnotationField {
            name: self.intern_field_name("binding"),
            value: HirAnnotationValue::Symbol(operandus.binding.name),
            span: operandus.binding.span,
        });
        if let Some(description) = operandus.description {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("descriptio"),
                value: HirAnnotationValue::String(description),
                span,
            });
        }
        if operandus.global {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("ubique"),
                value: HirAnnotationValue::Bool(true),
                span,
            });
        }
        if let Some(default) = &operandus.default {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("vel"),
                value: HirAnnotationValue::Expr(Box::new(self.lower_expr(default))),
                span: default.span,
            });
        }

        HirAnnotation::with_fields(self.intern_field_name("operandus"), fields, span)
    }

    fn lower_radix_typus_domain(&mut self, domain: &RadixTypusDomainAnnotation, span: Span) -> HirAnnotation {
        let mut fields = vec![HirAnnotationField {
            name: self.intern_field_name("param"),
            value: HirAnnotationValue::Symbol(domain.param.name),
            span: domain.param.span,
        }];
        for ty in &domain.allowed {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("allowed"),
                value: HirAnnotationValue::Type(self.lower_type(ty)),
                span: ty.span,
            });
        }

        HirAnnotation::with_fields(self.intern_field_name("radix"), fields, span)
    }

    pub(super) fn apply_type_param_constraints_from_annotations(
        &mut self,
        type_params: &mut [HirTypeParam],
        annotations: &[HirAnnotation],
    ) {
        for annotation in annotations {
            if self.interner.resolve(annotation.family) != "radix" {
                continue;
            }

            let Some(param_field) = annotation
                .fields
                .iter()
                .find(|field| self.interner.resolve(field.name) == "param")
            else {
                continue;
            };
            let HirAnnotationValue::Symbol(param_name) = param_field.value else {
                continue;
            };

            let Some(param_index) = type_params
                .iter()
                .position(|param| param.name == param_name)
            else {
                self.current_span = param_field.span;
                self.error("radix_typus_unknown_generic");
                continue;
            };
            if type_params[param_index].kind != crate::syntax::GenericParamKind::Typus {
                self.current_span = param_field.span;
                self.error("radix_typus_non_type_param");
                continue;
            }

            let mut allowed = match &type_params[param_index].constraint {
                HirTypeParamConstraint::Any => Vec::new(),
                HirTypeParamConstraint::OneOf(existing) => existing.clone(),
            };
            for field in &annotation.fields {
                if self.interner.resolve(field.name) != "allowed" {
                    continue;
                }
                let HirAnnotationValue::Type(ty) = field.value else {
                    continue;
                };
                if !self.type_param_domain_member_is_concrete(ty) {
                    self.current_span = field.span;
                    self.error("radix_typus_allowed_type_not_concrete");
                    continue;
                }
                allowed.push(ty);
            }
            // BO-S-4: collect → sort → adjacent dedup (O(n log n + n·equals))
            // instead of O(n²) pairwise is_distinct_union_member scans.
            //
            // The sort key must cluster `equals`-true members (union_member_sort_key):
            // a `typus` alias and the type it resolves to are distinct arena
            // entries whose raw TypeId ordinals need not be adjacent — an
            // unrelated member listed between them survives dedup_by(equals),
            // so the constraint carries a redundant member and
            // type_param_constraints_equal (union_members_equal length guard)
            // no longer equates the alias spelling with the primitive spelling.
            allowed.sort_by_key(|id| (self.types.union_member_sort_key(*id).0, id.0));
            allowed.dedup_by(|a, b| self.types.equals(*a, *b));
            type_params[param_index].constraint = HirTypeParamConstraint::OneOf(allowed);
        }
    }

    pub(super) fn lower_availability_from_annotations(&self, annotations: &[HirAnnotation]) -> Vec<HirAvailability> {
        annotations
            .iter()
            .filter_map(|a| try_parse_nondum_annotation(a, self.interner))
            .collect()
    }

    pub(super) fn annotation_family_is(
        annotations: &[HirAnnotation],
        interner: &crate::lexer::Interner,
        family: &str,
    ) -> bool {
        annotations
            .iter()
            .any(|annotation| interner.resolve(annotation.family) == family)
    }

    fn intern_field_name(&mut self, name: &str) -> Symbol {
        self.interner.intern(name)
    }

    fn type_param_domain_member_is_concrete(&self, ty: TypeId) -> bool {
        match self.types.get(ty) {
            Type::Primitive(Primitive::Ignotum) | Type::Param(_) | Type::Infer(_) | Type::Error => false,
            Type::Array(inner)
            | Type::Option(inner)
            | Type::Set(inner)
            | Type::Promissum(inner)
            | Type::Cursor(inner)
            | Type::Intervallum(inner)
            | Type::Alias(_, inner) => self.type_param_domain_member_is_concrete(*inner),
            Type::PromissumFailable(success, alternate) | Type::AsyncCursor(success, alternate) => {
                self.type_param_domain_member_is_concrete(*success)
                    && self.type_param_domain_member_is_concrete(*alternate)
            }
            Type::Map(key, value) => {
                self.type_param_domain_member_is_concrete(*key) && self.type_param_domain_member_is_concrete(*value)
            }
            Type::Tensor(elem, _) => self.type_param_domain_member_is_concrete(*elem),
            Type::Ref(_, inner) => self.type_param_domain_member_is_concrete(*inner),
            Type::Func(sig) => {
                sig.type_params.is_empty()
                    && sig
                        .params
                        .iter()
                        .all(|param| self.type_param_domain_member_is_concrete(param.ty))
                    && self.type_param_domain_member_is_concrete(sig.ret)
                    && sig
                        .err
                        .is_none_or(|err| self.type_param_domain_member_is_concrete(err))
            }
            Type::Applied(base, args) => {
                self.type_param_domain_member_is_concrete(*base)
                    && args
                        .iter()
                        .all(|arg| self.type_param_domain_member_is_concrete(*arg))
            }
            Type::Union(types) => types
                .iter()
                .all(|member| self.type_param_domain_member_is_concrete(*member)),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::{Interner, Span};

    #[test]
    fn test_try_parse_nondum_annotation_positive() {
        let mut interner = Interner::new();
        let family = interner.intern("nondum");
        let name_target = interner.intern("target");
        let name_ratio = interner.intern("ratio");
        let target_sym = interner.intern("rs");
        let reason_str = interner.intern("not yet implemented");

        let annotation = HirAnnotation::with_fields(
            family,
            vec![
                HirAnnotationField {
                    name: name_target,
                    value: HirAnnotationValue::Symbol(target_sym),
                    span: Span { start: 0, end: 0 },
                },
                HirAnnotationField {
                    name: name_ratio,
                    value: HirAnnotationValue::String(reason_str),
                    span: Span { start: 0, end: 0 },
                },
            ],
            Span { start: 0, end: 0 },
        );

        let result = try_parse_nondum_annotation(&annotation, &interner);
        assert!(result.is_some());
        match result {
            Some(HirAvailability::Nondum { target, reason }) => {
                assert_eq!(target, Some(target_sym));
                assert_eq!(reason, Some(reason_str));
            }
            _ => panic!("expected Nondum variant"),
        }
    }

    #[test]
    fn test_try_parse_nondum_annotation_wrong_family_returns_none() {
        let mut interner = Interner::new();
        let family = interner.intern("optio");

        let annotation = HirAnnotation::with_fields(family, vec![], Span { start: 0, end: 0 });

        let result = try_parse_nondum_annotation(&annotation, &interner);
        assert!(result.is_none());
    }
}
