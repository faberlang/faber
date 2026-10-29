//! Authored presentation witness reconciliation.
//!
//! Split from the FHIR artifact serializer (`serialize.rs`) because the Faber
//! formatter and the Faber target re-emit through the same reconciliation and
//! must not need the `hir-fhir` artifact feature.

use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{HirVisitor, walk_expr, walk_item, walk_method, walk_pattern};
use radix_types::{AppliedArg, Type, TypeId, TypeTable};

// ── Authored presentation witness reconciliation ─────────────────────────

/// Reconcile authored type records with the finalized HIR type table before a
/// FHIR artifact is written or a canonical Faber re-emission is revalidated.
///
/// Lowering records the source form before type checking can add a reference
/// carrier or canonicalize an index expression. The HIR type at the same site
/// is authoritative for the wire witness; the form head, path, and authored
/// child order remain presentation data. Records whose owner is not present in
/// the finalized HIR are left untouched so the artifact writer never hides a
/// producer-side integrity defect.
#[doc(hidden)]
pub(crate) fn normalize_presentation_witnesses(
    hir: &radix_hir::HirModule,
    presentation: &mut radix_hir::HirPresentation,
    types: &TypeTable,
) {
    remap_struct_field_sites(hir, presentation);
    let registry = TypeSiteRegistry::build(hir, presentation);
    for authored in &mut presentation.authored_types {
        if let Some(expected) = registry.sites.get(&authored.site).copied() {
            normalize_type_form(&mut authored.form, expected, types);
        }
    }
    // An authored `_` hole at a position HIR never materializes (for example
    // the target of an inferred constructor `_ { … }`) carries no presentation
    // information: the resolved cell type is the HIR node's fact. Drop it here
    // so the artifact validator does not reject the hole's site as unknown. A
    // non-hole record with an unknown site is a producer-side integrity defect
    // and is left for the validator to fail closed.
    presentation.authored_types.retain(|authored| {
        registry.sites.contains_key(&authored.site) || !matches!(authored.form.head, radix_hir::HirTypeFormHead::Infer)
    });
}

/// The validator's field inventory names a struct field by its containing
/// struct and `Field(n)`, while lowering records the field definition as the
/// owner. Move only that producer-side spelling mismatch; enum and schema
/// field sites already use the validator's accepted owner shape.
fn remap_struct_field_sites(hir: &radix_hir::HirModule, presentation: &mut radix_hir::HirPresentation) {
    let mut authored_by_site = FxHashMap::default();
    for authored in &mut presentation.authored_types {
        // Preserve the first-match rule without changing serialized site order.
        authored_by_site
            .entry(authored.site.clone())
            .or_insert(authored);
    }
    for item in &hir.items {
        let radix_hir::HirItemKind::Struct(strukt) = &item.kind else {
            continue;
        };
        for (ordinal, field) in strukt.fields.iter().enumerate() {
            let old_site = radix_hir::HirTypeSite {
                owner: radix_hir::HirOwnerRef::Def(field.def_id),
                slot: radix_hir::HirTypeSlot::Field(u32::try_from(ordinal).unwrap_or(u32::MAX)),
            };
            if let Some(authored) = authored_by_site.get_mut(&old_site) {
                authored.site.owner = radix_hir::HirOwnerRef::Def(item.def_id);
            }
        }
    }
}

#[derive(Default)]
struct TypeSiteRegistry {
    sites: FxHashMap<radix_hir::HirTypeSite, TypeId>,
    current_expr: Option<radix_hir::HirId>,
    inferred_constructor_ids: FxHashSet<radix_hir::HirId>,
}

impl TypeSiteRegistry {
    fn build(hir: &radix_hir::HirModule, presentation: &radix_hir::HirPresentation) -> Self {
        let mut registry = Self::default();
        registry
            .inferred_constructor_ids
            .extend(presentation.inferred_constructor_ids.iter().copied());
        radix_hir::visit::walk_program(&mut registry, hir);
        for discard in &presentation.discard_binding_stmts {
            if let Some(ty) = discard.ty {
                register_site(
                    &mut registry.sites,
                    radix_hir::HirOwnerRef::Hir(discard.expr_id),
                    radix_hir::HirTypeSlot::DiscardBinding,
                    ty,
                );
            }
        }
        registry
    }
}

fn register_site(
    sites: &mut FxHashMap<radix_hir::HirTypeSite, TypeId>,
    owner: radix_hir::HirOwnerRef,
    slot: radix_hir::HirTypeSlot,
    ty: TypeId,
) {
    sites
        .entry(radix_hir::HirTypeSite { owner, slot })
        .or_insert(ty);
}

fn register_function_sites(
    sites: &mut FxHashMap<radix_hir::HirTypeSite, TypeId>,
    owner: radix_hir::DefId,
    function: &radix_hir::HirFunction,
) {
    for (ordinal, param) in function.params.iter().enumerate() {
        register_site(
            sites,
            radix_hir::HirOwnerRef::Def(param.def_id),
            radix_hir::HirTypeSlot::Parameter(u32::try_from(ordinal).unwrap_or(u32::MAX)),
            param.ty,
        );
    }
    if let Some(param) = &function.cli_args {
        register_site(
            sites,
            radix_hir::HirOwnerRef::Def(param.def_id),
            radix_hir::HirTypeSlot::Parameter(u32::try_from(function.params.len()).unwrap_or(u32::MAX)),
            param.ty,
        );
    }
    if let Some(ty) = function.ret_ty {
        register_site(sites, radix_hir::HirOwnerRef::Def(owner), radix_hir::HirTypeSlot::Return, ty);
    }
    if let Some(ty) = function.err_ty {
        register_site(sites, radix_hir::HirOwnerRef::Def(owner), radix_hir::HirTypeSlot::Error, ty);
    }
}

fn register_annotation_sites(
    sites: &mut FxHashMap<radix_hir::HirTypeSite, TypeId>,
    owner: radix_hir::HirOwnerRef,
    annotations: &[radix_hir::HirAnnotation],
) {
    for (annotation, ann) in annotations.iter().enumerate() {
        let mut field = 0;
        for value in &ann.fields {
            if let radix_hir::HirAnnotationValue::Type(ty) = value.value {
                register_site(
                    sites,
                    owner.clone(),
                    radix_hir::HirTypeSlot::AnnotationField {
                        annotation: u32::try_from(annotation).unwrap_or(u32::MAX),
                        field,
                    },
                    ty,
                );
                field += 1;
            }
        }
    }
}

impl HirVisitor for TypeSiteRegistry {
    fn visit_item(&mut self, item: &radix_hir::HirItem) {
        let owner = radix_hir::HirOwnerRef::Def(item.def_id);
        register_annotation_sites(&mut self.sites, owner.clone(), &item.annotations);
        match &item.kind {
            radix_hir::HirItemKind::Function(function) => {
                register_function_sites(&mut self.sites, item.def_id, function);
            }
            radix_hir::HirItemKind::Struct(strukt) => {
                for (ordinal, field) in strukt.fields.iter().enumerate() {
                    register_site(
                        &mut self.sites,
                        owner.clone(),
                        radix_hir::HirTypeSlot::Field(u32::try_from(ordinal).unwrap_or(u32::MAX)),
                        field.ty,
                    );
                    register_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Def(field.def_id),
                        radix_hir::HirTypeSlot::Declared,
                        field.ty,
                    );
                    register_annotation_sites(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Def(field.def_id),
                        &field.annotations,
                    );
                }
            }
            radix_hir::HirItemKind::Enum(enumeration) => {
                for (ordinal, field) in enumeration.commune_fields.iter().enumerate() {
                    register_site(
                        &mut self.sites,
                        owner.clone(),
                        radix_hir::HirTypeSlot::Field(u32::try_from(ordinal).unwrap_or(u32::MAX)),
                        field.ty,
                    );
                }
                for variant in &enumeration.variants {
                    for (ordinal, field) in variant.fields.iter().enumerate() {
                        register_site(
                            &mut self.sites,
                            radix_hir::HirOwnerRef::Def(variant.def_id),
                            radix_hir::HirTypeSlot::Field(u32::try_from(ordinal).unwrap_or(u32::MAX)),
                            field.ty,
                        );
                    }
                }
            }
            radix_hir::HirItemKind::Interface(interface) => {
                for method in &interface.methods {
                    for (ordinal, param) in method.params.iter().enumerate() {
                        register_site(
                            &mut self.sites,
                            radix_hir::HirOwnerRef::Def(param.def_id),
                            radix_hir::HirTypeSlot::Parameter(u32::try_from(ordinal).unwrap_or(u32::MAX)),
                            param.ty,
                        );
                    }
                }
            }
            radix_hir::HirItemKind::TypeAlias(alias) => {
                register_site(&mut self.sites, owner.clone(), radix_hir::HirTypeSlot::AliasBody, alias.ty);
            }
            radix_hir::HirItemKind::Schema(schema) => {
                for (ordinal, column) in schema.columns.iter().enumerate() {
                    register_site(
                        &mut self.sites,
                        owner.clone(),
                        radix_hir::HirTypeSlot::Field(u32::try_from(ordinal).unwrap_or(u32::MAX)),
                        column.ty,
                    );
                }
            }
            radix_hir::HirItemKind::Constant(constant) => {
                if let Some(ty) = constant.ty {
                    register_site(&mut self.sites, owner.clone(), radix_hir::HirTypeSlot::Declared, ty);
                }
            }
            radix_hir::HirItemKind::Import(_) => {}
        }
        walk_item(self, item);
    }

    fn visit_method(&mut self, method: &radix_hir::HirMethod) {
        register_function_sites(&mut self.sites, method.def_id, &method.func);
        register_annotation_sites(&mut self.sites, radix_hir::HirOwnerRef::Def(method.def_id), &method.annotations);
        walk_method(self, method);
    }

    fn visit_local(&mut self, local: &radix_hir::HirLocal) {
        if let Some(ty) = local.ty {
            register_site(
                &mut self.sites,
                radix_hir::HirOwnerRef::Def(local.def_id),
                radix_hir::HirTypeSlot::Declared,
                ty,
            );
        }
        radix_hir::visit::walk_local(self, local);
    }

    fn visit_expr(&mut self, expr: &radix_hir::HirExpression) {
        let previous = self.current_expr.replace(expr.id);
        match &expr.kind {
            radix_hir::HirExpressionKind::Verte { target, .. } => {
                register_site(
                    &mut self.sites,
                    radix_hir::HirOwnerRef::Hir(expr.id),
                    radix_hir::HirTypeSlot::AscriptionTarget,
                    *target,
                );
                register_site(
                    &mut self.sites,
                    radix_hir::HirOwnerRef::Hir(expr.id),
                    radix_hir::HirTypeSlot::TypedZero,
                    *target,
                );
                register_site(
                    &mut self.sites,
                    radix_hir::HirOwnerRef::Hir(expr.id),
                    radix_hir::HirTypeSlot::ConversionTarget,
                    *target,
                );
            }
            radix_hir::HirExpressionKind::Conversio { target: radix_hir::HirConversioTarget::Type(ty), .. } => {
                register_site(
                    &mut self.sites,
                    radix_hir::HirOwnerRef::Hir(expr.id),
                    radix_hir::HirTypeSlot::ConversionTarget,
                    *ty,
                );
                // An inferred constructor rewritten to a set-cell conversion
                // keeps the literal's node id and the lowering's authored `_`
                // record at the ascription slot. Register that site so the
                // presentation record survives reconciliation and the emitter
                // can print the hole.
                if self.inferred_constructor_ids.contains(&expr.id) {
                    register_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Hir(expr.id),
                        radix_hir::HirTypeSlot::AscriptionTarget,
                        *ty,
                    );
                }
            }
            radix_hir::HirExpressionKind::TypeCheck { ty, .. } => register_site(
                &mut self.sites,
                radix_hir::HirOwnerRef::Hir(expr.id),
                radix_hir::HirTypeSlot::TypeCheckTarget,
                *ty,
            ),
            radix_hir::HirExpressionKind::Tuple(_, Some(type_args)) => {
                for (ordinal, ty) in type_args.iter().enumerate() {
                    register_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Hir(expr.id),
                        radix_hir::HirTypeSlot::TupleTypeArgument(u32::try_from(ordinal).unwrap_or(u32::MAX)),
                        *ty,
                    );
                }
            }
            radix_hir::HirExpressionKind::Clausura(params, ret, err, _) => {
                for (ordinal, param) in params.iter().enumerate() {
                    register_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Def(param.def_id),
                        radix_hir::HirTypeSlot::ClosureParameter(u32::try_from(ordinal).unwrap_or(u32::MAX)),
                        param.ty,
                    );
                }
                if let Some(ty) = ret {
                    register_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Hir(expr.id),
                        radix_hir::HirTypeSlot::Return,
                        *ty,
                    );
                }
                if let Some(ty) = err {
                    register_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Hir(expr.id),
                        radix_hir::HirTypeSlot::Error,
                        *ty,
                    );
                }
            }
            radix_hir::HirExpressionKind::MethodCall(receiver, ..)
                if matches!(receiver.kind, radix_hir::HirExpressionKind::Vacua) =>
            {
                // A tensor typed zero `Type {}` lowers to a `crea` call on a
                // `vacua` receiver pre-typed with the claimed tensor type.
                if let Some(ty) = receiver.ty {
                    register_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Hir(expr.id),
                        radix_hir::HirTypeSlot::TypedZero,
                        ty,
                    );
                }
            }
            _ => {}
        }
        walk_expr(self, expr);
        self.current_expr = previous;
    }

    fn visit_pattern(&mut self, pattern: &radix_hir::HirPattern) {
        if let (Some(owner), radix_hir::HirPattern::TypeTest { ty, .. }) = (self.current_expr, pattern) {
            register_site(
                &mut self.sites,
                radix_hir::HirOwnerRef::Hir(owner),
                radix_hir::HirTypeSlot::PatternType,
                *ty,
            );
        }
        walk_pattern(self, pattern);
    }
}

fn normalize_type_form(form: &mut radix_hir::HirTypeForm, expected: TypeId, types: &TypeTable) {
    let recorded = form.ty;
    if let Type::Ref(_, inner) = types.get(expected)
        && recorded != expected
    {
        let mut child = form.clone();
        normalize_type_form(&mut child, *inner, types);
        form.ty = expected;
        form.head = radix_hir::HirTypeFormHead::Semantic(radix_syntax::TypeFormLevel::LONG);
        form.args = vec![radix_hir::HirTypeFormArg::Type(Box::new(child))];
        return;
    }

    form.ty = expected;
    match types.get(expected) {
        Type::Array(inner)
        | Type::Queue(inner)
        | Type::Stack(inner)
        | Type::Set(inner)
        | Type::Promissum(inner)
        | Type::Cursor(inner)
        | Type::Atomic(inner)
        | Type::Intervallum(inner, _)
        | Type::BoundedArray { element: inner, .. }
        | Type::RangedArray { element: inner, .. }
        | Type::BoundedQueue { element: inner, .. }
        | Type::BoundedStack { element: inner, .. }
        | Type::Vector(inner, _)
        | Type::Matrix(inner, _)
        | Type::Sparsa(inner, _) => {
            normalize_first_type_arg(form, *inner, types);
        }
        Type::Tensor(inner, shape) => {
            normalize_first_type_arg(form, *inner, types);
            normalize_size_arg(form, *shape);
        }
        Type::Map(key, value) | Type::PromissumFailable(key, value) | Type::AsyncCursor(key, value) => {
            normalize_type_args(form, &[*key, *value], types);
        }
        Type::Option(inner) => {
            if form
                .args
                .iter()
                .any(|arg| matches!(arg, radix_hir::HirTypeFormArg::Type(_)))
            {
                normalize_first_type_arg(form, *inner, types);
            }
        }
        Type::Ref(_, inner) => normalize_first_type_arg(form, *inner, types),
        Type::Tuple(members) => normalize_type_args(form, members, types),
        Type::Record(fields) => {
            // The witness map has no order; authored children are keyed by
            // their label, never zipped against map iteration order.
            for arg in &mut form.args {
                if let radix_hir::HirTypeFormArg::Type(child) = arg
                    && let Some(expected) = child.label.and_then(|label| fields.get(&label))
                {
                    normalize_type_form_arg(child, *expected, types);
                }
            }
        }
        Type::Applied(_, args) => {
            let members = args
                .iter()
                .filter_map(|arg| match arg {
                    AppliedArg::Type(ty) => Some(*ty),
                    AppliedArg::Size(_) => None,
                })
                .collect::<Vec<_>>();
            normalize_type_args(form, &members, types);
        }
        _ => {}
    }
}

fn normalize_first_type_arg(form: &mut radix_hir::HirTypeForm, expected: TypeId, types: &TypeTable) {
    if let Some(radix_hir::HirTypeFormArg::Type(child)) = form
        .args
        .iter_mut()
        .find(|arg| matches!(arg, radix_hir::HirTypeFormArg::Type(_)))
    {
        normalize_type_form_arg(child, expected, types);
    }
}

/// Whether `ty` is a named witness the artifact validator spells as a
/// constructor head of its own rather than as a canonical form.
///
/// A `Semantic` head over one of these carries a named type inside a canonical
/// form, which the validator rejects as `semantic_named_constructor`.
fn is_named_witness(types: &TypeTable, ty: TypeId) -> bool {
    matches!(
        types.get(ty),
        Type::Struct(_) | Type::Enum(_) | Type::Interface(_) | Type::Alias(_, _) | Type::Applied(_, _)
    )
}

fn normalize_type_form_arg(child: &mut radix_hir::HirTypeForm, expected: TypeId, types: &TypeTable) {
    if child.ty != expected {
        let named_function_alias = matches!(
            (&child.head, types.get(expected)),
            (radix_hir::HirTypeFormHead::Named { .. }, Type::Func(_))
        );
        if is_named_witness(types, expected) || named_function_alias {
            // A named witness is its own constructor head: the recorded child
            // already names the type, so pairing it with the finalized witness
            // needs no canonical form. A function alias has the same rule: its
            // signature is canonical in FHIR, but the alias head is preserved
            // and the validator checks its transparent body against that exact
            // signature.
            child.ty = expected;
        } else {
            let original = child.clone();
            child.ty = expected;
            child.head = radix_hir::HirTypeFormHead::Semantic(radix_syntax::TypeFormLevel::LONG);
            child.args = vec![radix_hir::HirTypeFormArg::Type(Box::new(original))];
        }
    }
    normalize_type_form(child, expected, types);
}

fn normalize_type_args(form: &mut radix_hir::HirTypeForm, expected: &[TypeId], types: &TypeTable) {
    for (child, expected) in form
        .args
        .iter_mut()
        .filter_map(|arg| match arg {
            radix_hir::HirTypeFormArg::Type(child) => Some(child),
            _ => None,
        })
        .zip(expected.iter().copied())
    {
        normalize_type_form_arg(child, expected, types);
    }
}

fn normalize_size_arg(form: &mut radix_hir::HirTypeForm, expected: radix_types::IndexId) {
    if let Some(radix_hir::HirTypeFormArg::Size(index)) = form
        .args
        .iter_mut()
        .find(|arg| matches!(arg, radix_hir::HirTypeFormArg::Size(_)))
    {
        *index = expected;
    }
}
