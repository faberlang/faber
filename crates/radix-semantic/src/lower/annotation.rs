//! Lower parsed `@` metadata into normalized [`HirAnnotation`] records.
//!
//! This module owns the annotation syntax-to-HIR normalization for built-in annotation
//! families (cli, imperium, optio, operandus, nondum, radix, verte) and braced/sugar forms.
//! It does not resolve annotation contracts, validate field types against contract schemas,
//! or decide annotation semantics — those belong to semantic analysis and later phases.

use super::Lowerer;
use crate::{ContractPayloadValue, Primitive, ResolvedContractApplication, Type, TypeId};
use radix_hir::{
    DefId, HirAnnotation, HirAnnotationField, HirAnnotationValue, HirAvailability, HirOwnerRef,
    HirTypeParam, HirTypeParamConstraint,
};
use radix_lexer::{Interner, Span, Symbol};
use radix_syntax::{
    Annotation, AnnotationField, AnnotationFieldValue, AnnotationKind, AnnotationStmt,
    BracedAnnotation, CliAnnotation, ImperiumAnnotation, NucleumAnnotation, OperandusAnnotation,
    OptioAnnotation, RadixTypusDomainAnnotation, TypeExpr, braced_cli_nomen, braced_imperium_nomen,
    braced_nondum_view, braced_operandus_view, braced_optio_view, braced_radix_lane_view,
    braced_radix_typus_view, braced_verte_view, nondum_from_stmt, radix_lane_from_stmt,
    verte_from_stmt,
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
#[must_use]
pub fn try_parse_nondum_annotation(
    annotation: &HirAnnotation,
    interner: &Interner,
) -> Option<HirAvailability> {
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
    pub(super) fn lower_item_annotations(
        &mut self,
        def_id: DefId,
        annotations: &[Annotation],
    ) -> Vec<HirAnnotation> {
        self.annotation_capture = Some(HirOwnerRef::Def(def_id));
        let lowered = self.lower_annotations(annotations);
        self.annotation_capture = None;
        lowered
    }

    pub(super) fn lower_annotations(&mut self, annotations: &[Annotation]) -> Vec<HirAnnotation> {
        annotations
            .iter()
            .enumerate()
            .map(|(ordinal, annotation)| {
                self.annotation_ordinal = Some(u32::try_from(ordinal).unwrap_or(u32::MAX));
                self.annotation_field_ordinal = 0;

                self.lower_annotation(annotation)
            })
            .collect()
    }

    /// Lower a type-valued annotation field, capturing its authored form when
    /// a carrying owner is in scope.
    pub(super) fn lower_annotation_type(&mut self, ty: &TypeExpr) -> TypeId {
        let field_ordinal = self.annotation_field_ordinal;
        self.annotation_field_ordinal += 1;
        match (self.annotation_capture.clone(), self.annotation_ordinal) {
            (Some(owner), Some(annotation)) => self.lower_authored_type(
                radix_hir::HirTypeSite {
                    owner,
                    slot: radix_hir::HirTypeSlot::AnnotationField {
                        annotation,
                        field: field_ordinal,
                    },
                },
                ty,
                false,
                false,
            ),
            _ => self.lower_type(ty),
        }
    }

    pub(super) fn lower_annotation(&mut self, annotation: &Annotation) -> HirAnnotation {
        if let Some(resolved) = self
            .annotation_contracts
            .and_then(|meta| meta.application_at(annotation.span))
        {
            return Self::lower_resolved_contract(resolved, annotation.span);
        }

        match &annotation.kind {
            AnnotationKind::Statement(stmt) => {
                self.lower_statement_annotation(stmt, annotation.span)
            }
            AnnotationKind::RadixTypusDomain(domain) => {
                self.lower_radix_typus_domain(domain, annotation.span)
            }
            AnnotationKind::RadixLane(lane) => {
                let radix_sym = self.interner.intern("radix");
                self.lower_radix_lane_fields(radix_sym, lane.lane, annotation.span)
            }
            AnnotationKind::Nucleum(nucleum) => {
                self.lower_nucleum_annotation(nucleum, annotation.span)
            }
            // Standalone `@ conversio` marker; the ordered pair is derived
            // and enrolled from the function signature by `lower_functio`.
            AnnotationKind::Conversio => self.unit_annotation("conversio", annotation.span),
            // `@ ad '<route>'`: family `ad`, one `route` string field
            // ([`radix_hir::HirAnnotation::ad_route`]); validation and route
            // collection belong to `passes::ad_routes`.
            AnnotationKind::Ad(ad) => {
                let mut lowered = HirAnnotation::with_fields(
                    self.intern_field_name("ad"),
                    vec![HirAnnotationField {
                        name: self.intern_field_name("route"),
                        value: HirAnnotationValue::String(ad.route),
                        span: ad.route_span,
                    }],
                    annotation.span,
                );
                lowered.brace_authored = false;
                lowered
            }
            AnnotationKind::Cli(cli) => self.lower_cli_annotation(cli, annotation.span),
            AnnotationKind::Imperium(imperium) => {
                self.lower_imperium_annotation(imperium, annotation.span)
            }
            AnnotationKind::Optio(optio) => self.lower_optio_annotation(optio, annotation.span),
            AnnotationKind::Operandus(operandus) => {
                self.lower_operandus_annotation(operandus, annotation.span)
            }
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

    fn lower_resolved_contract(
        resolved: &ResolvedContractApplication,
        span: Span,
    ) -> HirAnnotation {
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
            brace_authored: true,
            contract_id: Some(resolved.contract_id),
            span,
        }
    }

    fn lower_braced_annotation(&mut self, braced: &BracedAnnotation, span: Span) -> HirAnnotation {
        let family_owned = self.interner.resolve(braced.name.name).to_owned();

        if family_owned == "contract" {
            // The compiler-owned marker was validated before HIR lowering.
            // Keep its environment a constant metadata value, not an ordinary
            // executable expression in the annotation payload.
            let fields = braced
                .fields
                .iter()
                .filter_map(|field| {
                    let AnnotationFieldValue::Expr(expr) = &field.value else {
                        return None;
                    };
                    let radix_syntax::ExprKind::Literal(radix_syntax::Literal::String(value)) =
                        expr.kind
                    else {
                        return None;
                    };
                    Some(HirAnnotationField {
                        name: field.name.name,
                        value: HirAnnotationValue::String(value),
                        span: field.span,
                    })
                })
                .collect();
            return HirAnnotation::with_fields(braced.name.name, fields, span);
        }

        // Both families lower to the same `nomen` record, so one shared
        // builder covers `@ cli` and `@ imperium`; the family check inside
        // each view helper keeps them mutually exclusive.
        let nomen = braced_cli_nomen(braced, self.interner)
            .or_else(|| braced_imperium_nomen(braced, self.interner));
        if let Some(nomen) = nomen {
            return HirAnnotation {
                family: braced.name.name,
                fields: vec![HirAnnotationField {
                    name: self.intern_field_name("nomen"),
                    value: HirAnnotationValue::String(nomen),
                    span,
                }],
                raw_args: Vec::new(),
                brace_authored: true,
                contract_id: None,
                span,
            };
        }

        if let Some(view) = braced_optio_view(braced, self.interner) {
            return self.lower_braced_optio(braced.name.name, &view, span);
        }

        if let Some(view) = braced_operandus_view(braced, self.interner) {
            return self.lower_braced_operandus(braced.name.name, &view, span);
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
            // Authored empty braced record (`@ futura { }`) — keep the braces.
            return HirAnnotation::with_fields(braced.name.name, Vec::new(), span);
        }

        HirAnnotation {
            family: braced.name.name,
            fields: braced
                .fields
                .iter()
                .map(|field| self.lower_braced_field(field))
                .collect(),
            raw_args: Vec::new(),
            brace_authored: true,
            contract_id: None,
            span,
        }
    }

    fn lower_braced_optio(
        &mut self,
        family: Symbol,
        view: &radix_syntax::BracedOptioView<'_>,
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
                value: HirAnnotationValue::Type(self.lower_annotation_type(ty)),
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
        view: &radix_syntax::BracedOperandusView<'_>,
        span: Span,
    ) -> HirAnnotation {
        let mut fields = Vec::new();
        if view.is_rest {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("ceteri"),
                value: HirAnnotationValue::Bool(true),
                span,
            });
        }
        fields.push(HirAnnotationField {
            name: self.intern_field_name("typus"),
            value: HirAnnotationValue::Type(self.lower_annotation_type(view.ty)),
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

    fn lower_braced_field(&mut self, field: &AnnotationField) -> HirAnnotationField {
        let name = field.name.name;
        let value = match &field.value {
            AnnotationFieldValue::Expr(expr) => {
                HirAnnotationValue::Expr(Box::new(self.lower_expr(expr)))
            }
            AnnotationFieldValue::Type(ty) => {
                HirAnnotationValue::Type(self.lower_annotation_type(ty))
            }
        };
        HirAnnotationField {
            name,
            value,
            span: field.span,
        }
    }

    fn unit_annotation(&mut self, family: &str, span: Span) -> HirAnnotation {
        let mut annotation =
            HirAnnotation::with_fields(self.intern_field_name(family), Vec::new(), span);
        // Bare marker (`@ futura`); braced empty records route through the
        // braced-annotation path with the authored flag set.
        annotation.brace_authored = false;
        annotation
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
            return self.lower_verte_fields(
                family,
                view.target,
                view.methodus,
                view.forma_spelling.as_deref(),
                span,
            );
        }

        if matches!(
            family_name,
            "futura" | "cursor" | "nucleum" | "vertex" | "fragment" | "solum" | "omitte" | "metior"
        ) && stmt.args.is_empty()
        {
            let mut annotation = HirAnnotation::with_fields(family, Vec::new(), span);
            annotation.brace_authored = false;
            return annotation;
        }

        HirAnnotation {
            family,
            fields: Vec::new(),
            raw_args: stmt.args.clone(),
            brace_authored: true,
            contract_id: None,
            span,
        }
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

    fn lower_radix_lane_fields(
        &mut self,
        family: Symbol,
        lane: Symbol,
        span: Span,
    ) -> HirAnnotation {
        HirAnnotation {
            family,
            fields: vec![HirAnnotationField {
                name: self.intern_field_name("lane"),
                value: HirAnnotationValue::String(lane),
                span,
            }],
            raw_args: Vec::new(),
            brace_authored: true,
            contract_id: None,
            span,
        }
    }

    fn lower_braced_radix_typus(
        &mut self,
        family: Symbol,
        view: radix_syntax::RadixTypusView<'_>,
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
                value: HirAnnotationValue::Type(self.lower_annotation_type(ty)),
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

    fn lower_nucleum_annotation(
        &mut self,
        nucleum: &NucleumAnnotation,
        span: Span,
    ) -> HirAnnotation {
        let family = self.intern_field_name("nucleum");
        let mut fields = Vec::new();
        if nucleum.is_fragment {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("fragment"),
                value: HirAnnotationValue::Bool(true),
                span,
            });
        }
        // The tier is carried as its Latin keyword identity (`laxus`,
        // `intutus`); the default tier writes nothing.
        if let Some(identity) = nucleum.tier.identity() {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("tier"),
                value: HirAnnotationValue::Symbol(self.intern_field_name(identity)),
                span,
            });
        }
        // Authored brace shape: `@ nucleum { … }` keeps braces; the sugar
        // markers (`@ kernel`, `@ kernel fragment`) stay bare.
        let mut annotation = HirAnnotation::with_fields(family, fields, span);
        annotation.brace_authored = nucleum.braced;
        annotation
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
            brace_authored: true,
            contract_id: None,
            span,
        }
    }

    fn lower_imperium_annotation(
        &mut self,
        imperium: &ImperiumAnnotation,
        span: Span,
    ) -> HirAnnotation {
        HirAnnotation {
            family: self.intern_field_name("imperium"),
            fields: vec![HirAnnotationField {
                name: self.intern_field_name("nomen"),
                value: HirAnnotationValue::String(imperium.name),
                span,
            }],
            raw_args: Vec::new(),
            brace_authored: true,
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
                value: HirAnnotationValue::Type(self.lower_annotation_type(ty)),
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

    fn lower_operandus_annotation(
        &mut self,
        operandus: &OperandusAnnotation,
        span: Span,
    ) -> HirAnnotation {
        let mut fields = Vec::new();
        if operandus.is_rest {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("ceteri"),
                value: HirAnnotationValue::Bool(true),
                span,
            });
        }
        fields.push(HirAnnotationField {
            name: self.intern_field_name("typus"),
            value: HirAnnotationValue::Type(self.lower_annotation_type(&operandus.ty)),
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

    fn lower_radix_typus_domain(
        &mut self,
        domain: &RadixTypusDomainAnnotation,
        span: Span,
    ) -> HirAnnotation {
        let mut fields = vec![HirAnnotationField {
            name: self.intern_field_name("param"),
            value: HirAnnotationValue::Symbol(domain.param.name),
            span: domain.param.span,
        }];
        for ty in &domain.allowed {
            fields.push(HirAnnotationField {
                name: self.intern_field_name("allowed"),
                value: HirAnnotationValue::Type(self.lower_annotation_type(ty)),
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
            if type_params[param_index].kind != radix_syntax::GenericParamKind::Typus {
                self.current_span = param_field.span;
                self.error("radix_typus_non_type_param");
                continue;
            }

            let mut allowed = match &type_params[param_index].constraint {
                HirTypeParamConstraint::Any => Vec::new(),
                HirTypeParamConstraint::OneOf(existing) => existing.clone(),
                // A closed type list and a contract bound are two admission
                // rules for one parameter; D1.9 keeps them separate.
                HirTypeParamConstraint::Implet(_) => {
                    self.current_span = param_field.span;
                    self.error("radix_typus_bounded_param");
                    continue;
                }
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

    pub(super) fn lower_availability_from_annotations(
        &self,
        annotations: &[HirAnnotation],
    ) -> Vec<HirAvailability> {
        annotations
            .iter()
            .filter_map(|a| try_parse_nondum_annotation(a, self.interner))
            .collect()
    }

    pub(super) fn annotation_family_is(
        annotations: &[HirAnnotation],
        interner: &radix_lexer::Interner,
        family: &str,
    ) -> bool {
        annotations
            .iter()
            .any(|annotation| interner.resolve(annotation.family) == family)
    }

    /// Entry vs fragment bits for the nucleus surface. Both true is a conflict
    /// left visible so `device_role_for_hir_function` can fail closed.
    pub(super) fn nucleum_role_bits(
        annotations: &[HirAnnotation],
        interner: &radix_lexer::Interner,
    ) -> (bool, bool) {
        let mut entry = false;
        let mut fragment = false;
        for annotation in annotations {
            if interner.resolve(annotation.family) != "nucleum" {
                continue;
            }
            let is_fragment = annotation.fields.iter().any(|field| {
                interner.resolve(field.name) == "fragment"
                    && matches!(field.value, HirAnnotationValue::Bool(true))
            });
            if is_fragment {
                fragment = true;
            } else {
                entry = true;
            }
        }
        (entry, fragment)
    }

    fn intern_field_name(&mut self, name: &str) -> Symbol {
        self.interner.intern(name)
    }

    /// Whether an `allowed` domain member names a concrete type: no `ignotum`,
    /// generic parameter, or unresolved hole anywhere inside it. Exhaustive on
    /// purpose, so a new compound type cannot count as concrete by default.
    fn type_param_domain_member_is_concrete(&self, ty: TypeId) -> bool {
        match self.types.get(ty) {
            Type::Primitive(Primitive::Ignotum)
            | Type::Param(_)
            | Type::Infer(_)
            | Type::InferUnion(_)
            | Type::MarkerHole { .. }
            | Type::Error => false,
            Type::Array(inner)
            | Type::Queue(inner)
            | Type::Stack(inner)
            | Type::BoundedArray { element: inner, .. }
            | Type::RangedArray { element: inner, .. }
            | Type::BoundedQueue { element: inner, .. }
            | Type::BoundedStack { element: inner, .. }
            | Type::Option(inner)
            | Type::Set(inner)
            | Type::Promissum(inner)
            | Type::Cursor(inner)
            | Type::Atomic(inner)
            | Type::Intervallum(inner, _)
            | Type::Alias(_, inner)
            | Type::Ref(_, inner)
            | Type::Tensor(inner, _)
            | Type::Vector(inner, _)
            | Type::Matrix(inner, _)
            | Type::Sparsa(inner, _) => self.type_param_domain_member_is_concrete(*inner),
            Type::PromissumFailable(first, second)
            | Type::AsyncCursor(first, second)
            | Type::Map(first, second)
            | Type::Ledger(first, second) => {
                self.type_param_domain_member_is_concrete(*first)
                    && self.type_param_domain_member_is_concrete(*second)
            }
            Type::Record(fields) => fields
                .values()
                .all(|field| self.type_param_domain_member_is_concrete(*field)),
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
                    && args.iter().all(|arg| match arg {
                        radix_types::AppliedArg::Type(id) => {
                            self.type_param_domain_member_is_concrete(*id)
                        }
                        radix_types::AppliedArg::Size(_) => true,
                    })
            }
            Type::Union(members) | Type::Intersection(members) | Type::Tuple(members) => members
                .iter()
                .all(|member| self.type_param_domain_member_is_concrete(*member)),
            Type::Primitive(_)
            | Type::TensorEdgePolicy(_)
            | Type::BoundedTextus { .. }
            | Type::BoundedAscii { .. }
            | Type::BoundedOcteti { .. }
            | Type::SizedNumeric(..)
            | Type::Bits(_)
            | Type::ModularWord(_)
            | Type::SaturatingWord(_)
            | Type::RangedInt { .. }
            | Type::PolicyFloat(..)
            | Type::SizedInstans(_)
            | Type::Struct(_)
            | Type::Enum(_)
            | Type::Interface(_) => true,
        }
    }
}

#[cfg(test)]
#[path = "annotation_test.rs"]
mod annotation_test;
