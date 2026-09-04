//! Fail-closed referential-integrity validation for the FHIR artifact.
//!
//! Walks every arena-backed index in the artifact — `Symbol`, `DefId`,
//! `HirId`, `TypeId`, `IndexId`, presentation anchors — and rejects any
//! reference that does not resolve before codegen can touch it (U2.3, CSO E8).

use crate::artifact::HirArtifact;
use crate::error::HirArtifactError;
use rustc_hash::{FxHashMap, FxHashSet};

// ---------------------------------------------------------------------------
// Authored type-site inventory and form validation (ATF-5)
// ---------------------------------------------------------------------------

fn register_type_site(
    sites: &mut FxHashMap<radix_hir::HirTypeSite, radix_types::TypeId>,
    owner: radix_hir::HirOwnerRef,
    slot: radix_hir::HirTypeSlot,
    ty: radix_types::TypeId,
) {
    sites
        .entry(radix_hir::HirTypeSite { owner, slot })
        .or_insert(ty);
}

fn register_function_type_sites(
    sites: &mut FxHashMap<radix_hir::HirTypeSite, radix_types::TypeId>,
    owner: radix_types::DefId,
    function: &radix_hir::HirFunction,
) {
    for (ordinal, param) in function.params.iter().enumerate() {
        register_type_site(
            sites,
            radix_hir::HirOwnerRef::Def(param.def_id),
            radix_hir::HirTypeSlot::Parameter(ordinal as u32),
            param.ty,
        );
    }
    if let Some(param) = &function.cli_args {
        register_type_site(
            sites,
            radix_hir::HirOwnerRef::Def(param.def_id),
            radix_hir::HirTypeSlot::Parameter(function.params.len() as u32),
            param.ty,
        );
    }
    if let Some(ty) = function.ret_ty {
        register_type_site(
            sites,
            radix_hir::HirOwnerRef::Def(owner),
            radix_hir::HirTypeSlot::Return,
            ty,
        );
    }
    if let Some(ty) = function.err_ty {
        register_type_site(
            sites,
            radix_hir::HirOwnerRef::Def(owner),
            radix_hir::HirTypeSlot::Error,
            ty,
        );
    }
}

fn register_annotation_type_sites(
    sites: &mut FxHashMap<radix_hir::HirTypeSite, radix_types::TypeId>,
    owner: radix_hir::HirOwnerRef,
    annotations: &[radix_hir::HirAnnotation],
) {
    for (annotation, ann) in annotations.iter().enumerate() {
        let mut type_field = 0;
        for value in &ann.fields {
            if let radix_hir::HirAnnotationValue::Type(ty) = &value.value {
                register_type_site(
                    sites,
                    owner.clone(),
                    radix_hir::HirTypeSlot::AnnotationField {
                        annotation: annotation as u32,
                        field: type_field,
                    },
                    *ty,
                );
                type_field += 1;
            }
        }
    }
}

struct TypeSiteCollector {
    sites: FxHashMap<radix_hir::HirTypeSite, radix_types::TypeId>,
    current_expr: Option<radix_hir::HirId>,
}

impl TypeSiteCollector {
    fn new() -> Self {
        Self {
            sites: FxHashMap::default(),
            current_expr: None,
        }
    }
}

impl radix_hir::visit::HirVisitor for TypeSiteCollector {
    fn visit_item(&mut self, item: &radix_hir::HirItem) {
        let owner = radix_hir::HirOwnerRef::Def(item.def_id);
        match &item.kind {
            radix_hir::HirItemKind::Function(function) => {
                register_function_type_sites(&mut self.sites, item.def_id, function);
                register_annotation_type_sites(
                    &mut self.sites,
                    owner.clone(),
                    &function.annotations,
                );
            }
            radix_hir::HirItemKind::Struct(strukt) => {
                register_annotation_type_sites(&mut self.sites, owner.clone(), &strukt.annotations);
                for (ordinal, field) in strukt.fields.iter().enumerate() {
                    register_type_site(
                        &mut self.sites,
                        owner.clone(),
                        radix_hir::HirTypeSlot::Field(ordinal as u32),
                        field.ty,
                    );
                    register_type_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Def(field.def_id),
                        radix_hir::HirTypeSlot::Declared,
                        field.ty,
                    );
                    register_annotation_type_sites(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Def(field.def_id),
                        &field.annotations,
                    );
                }
            }
            radix_hir::HirItemKind::Enum(enum_item) => {
                register_annotation_type_sites(
                    &mut self.sites,
                    owner.clone(),
                    &enum_item.annotations,
                );
                for (ordinal, field) in enum_item.commune_fields.iter().enumerate() {
                    register_type_site(
                        &mut self.sites,
                        owner.clone(),
                        radix_hir::HirTypeSlot::Field(ordinal as u32),
                        field.ty,
                    );
                }
                for variant in &enum_item.variants {
                    for (ordinal, field) in variant.fields.iter().enumerate() {
                        register_type_site(
                            &mut self.sites,
                            radix_hir::HirOwnerRef::Def(variant.def_id),
                            radix_hir::HirTypeSlot::Field(ordinal as u32),
                            field.ty,
                        );
                    }
                }
            }
            radix_hir::HirItemKind::Interface(interface) => {
                for method in &interface.methods {
                    for (ordinal, param) in method.params.iter().enumerate() {
                        register_type_site(
                            &mut self.sites,
                            radix_hir::HirOwnerRef::Def(param.def_id),
                            radix_hir::HirTypeSlot::Parameter(ordinal as u32),
                            param.ty,
                        );
                    }
                }
            }
            radix_hir::HirItemKind::TypeAlias(alias) => {
                register_type_site(
                    &mut self.sites,
                    owner.clone(),
                    radix_hir::HirTypeSlot::AliasBody,
                    alias.ty,
                );
            }
            radix_hir::HirItemKind::Schema(schema) => {
                for (ordinal, column) in schema.columns.iter().enumerate() {
                    register_type_site(
                        &mut self.sites,
                        owner.clone(),
                        radix_hir::HirTypeSlot::Field(ordinal as u32),
                        column.ty,
                    );
                }
            }
            radix_hir::HirItemKind::Constant(constant) => {
                if let Some(ty) = constant.ty {
                    register_type_site(
                        &mut self.sites,
                        owner.clone(),
                        radix_hir::HirTypeSlot::Declared,
                        ty,
                    );
                }
            }
            radix_hir::HirItemKind::Import(_) => {}
        }
        radix_hir::visit::walk_item(self, item);
    }

    fn visit_method(&mut self, method: &radix_hir::HirMethod) {
        register_function_type_sites(&mut self.sites, method.def_id, &method.func);
        register_annotation_type_sites(
            &mut self.sites,
            radix_hir::HirOwnerRef::Def(method.def_id),
            &method.func.annotations,
        );
        radix_hir::visit::walk_method(self, method);
    }

    fn visit_local(&mut self, local: &radix_hir::HirLocal) {
        if let Some(ty) = local.ty {
            register_type_site(
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
                register_type_site(
                    &mut self.sites,
                    radix_hir::HirOwnerRef::Hir(expr.id),
                    radix_hir::HirTypeSlot::AscriptionTarget,
                    *target,
                );
                // Finge casts currently lower to the same `Verte` carrier as
                // static ascription; the wire has no discriminator to tell
                // those two source slots apart, so both are checkable sites.
                register_type_site(
                    &mut self.sites,
                    radix_hir::HirOwnerRef::Hir(expr.id),
                    radix_hir::HirTypeSlot::ConversionTarget,
                    *target,
                );
            }
            radix_hir::HirExpressionKind::Conversio {
                target: radix_hir::HirConversioTarget::Type(ty),
                ..
            } => register_type_site(
                &mut self.sites,
                radix_hir::HirOwnerRef::Hir(expr.id),
                radix_hir::HirTypeSlot::ConversionTarget,
                *ty,
            ),
            radix_hir::HirExpressionKind::TypeCheck { ty, .. } => register_type_site(
                &mut self.sites,
                radix_hir::HirOwnerRef::Hir(expr.id),
                radix_hir::HirTypeSlot::TypeCheckTarget,
                *ty,
            ),
            radix_hir::HirExpressionKind::Tuple(_, Some(type_args)) => {
                for (ordinal, ty) in type_args.iter().enumerate() {
                    register_type_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Hir(expr.id),
                        radix_hir::HirTypeSlot::TupleTypeArgument(ordinal as u32),
                        *ty,
                    );
                }
            }
            radix_hir::HirExpressionKind::Clausura(params, ret, err, _, _) => {
                for (ordinal, param) in params.iter().enumerate() {
                    register_type_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Def(param.def_id),
                        radix_hir::HirTypeSlot::ClosureParameter(ordinal as u32),
                        param.ty,
                    );
                }
                if let Some(ty) = ret {
                    register_type_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Hir(expr.id),
                        radix_hir::HirTypeSlot::Return,
                        *ty,
                    );
                }
                if let Some(ty) = err {
                    register_type_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Hir(expr.id),
                        radix_hir::HirTypeSlot::Error,
                        *ty,
                    );
                }
            }
            _ => {}
        }
        radix_hir::visit::walk_expr(self, expr);
        self.current_expr = previous;
    }

    fn visit_pattern(&mut self, pattern: &radix_hir::HirPattern) {
        if let (Some(owner), radix_hir::HirPattern::TypeTest { ty, .. }) =
            (self.current_expr, pattern)
        {
            register_type_site(
                &mut self.sites,
                radix_hir::HirOwnerRef::Hir(owner),
                radix_hir::HirTypeSlot::PatternType,
                *ty,
            );
        }
        radix_hir::visit::walk_pattern(self, pattern);
    }
}

fn collect_type_site_registry(
    artifact: &HirArtifact,
) -> FxHashMap<radix_hir::HirTypeSite, radix_types::TypeId> {
    let mut collector = TypeSiteCollector::new();
    radix_hir::visit::walk_program(&mut collector, &artifact.hir);
    for discard in &artifact.presentation.discard_binding_stmts {
        if let Some(ty) = discard.ty {
            register_type_site(
                &mut collector.sites,
                radix_hir::HirOwnerRef::Hir(discard.expr_id),
                radix_hir::HirTypeSlot::DiscardBinding,
                ty,
            );
        }
    }
    collector.sites
}

fn collect_named_type_defs(artifact: &HirArtifact) -> FxHashSet<radix_types::DefId> {
    let mut defs = FxHashSet::default();
    for item in &artifact.hir.items {
        if matches!(
            &item.kind,
            radix_hir::HirItemKind::Struct(_)
                | radix_hir::HirItemKind::Enum(_)
                | radix_hir::HirItemKind::Interface(_)
                | radix_hir::HirItemKind::TypeAlias(_)
        ) {
            defs.insert(item.def_id);
        }
    }
    for (_, interface) in &artifact.resolver.file_interfaces {
        for (_, export) in &interface.exports {
            match export {
                crate::artifact::FileExportSnapshot::Struct { def_id, .. }
                | crate::artifact::FileExportSnapshot::Type {
                    def_id: Some(def_id),
                    ..
                } => {
                    defs.insert(*def_id);
                }
                crate::artifact::FileExportSnapshot::Function(_)
                | crate::artifact::FileExportSnapshot::Type { def_id: None, .. } => {}
            }
        }
    }
    defs
}

fn type_site_anchor(site: &radix_hir::HirTypeSite) -> u32 {
    match site.owner {
        radix_hir::HirOwnerRef::Hir(id) => id.0,
        radix_hir::HirOwnerRef::Def(def_id) => def_id.0,
    }
}

fn type_presentation_issue(issue: &'static str, site: &radix_hir::HirTypeSite) -> HirArtifactError {
    HirArtifactError::InvalidPresentation {
        issue,
        anchor: type_site_anchor(site),
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_type_arg(
    arg: &radix_hir::HirTypeFormArg,
    expected: radix_types::TypeId,
    site: &radix_hir::HirTypeSite,
    root_slot: &radix_hir::HirTypeSlot,
    types: &radix_types::TypeTableSnapshot,
    named_defs: &FxHashSet<radix_types::DefId>,
    check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
    check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
    check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
    check_index: &dyn Fn(radix_types::IndexId) -> Result<(), HirArtifactError>,
) -> Result<(), HirArtifactError> {
    let radix_hir::HirTypeFormArg::Type(child) = arg else {
        return Err(type_presentation_issue("type_argument_kind", site));
    };
    if child.ty != expected {
        return Err(type_presentation_issue("nested_witness_mismatch", site));
    }
    validate_type_form(
        child,
        site,
        root_slot,
        types,
        named_defs,
        check_symbol,
        check_def,
        check_type,
        check_index,
    )
}

fn validate_size_arg(
    arg: &radix_hir::HirTypeFormArg,
    expected: radix_types::IndexId,
    site: &radix_hir::HirTypeSite,
    check_index: &dyn Fn(radix_types::IndexId) -> Result<(), HirArtifactError>,
) -> Result<(), HirArtifactError> {
    let radix_hir::HirTypeFormArg::Size(index) = arg else {
        return Err(type_presentation_issue("size_argument_kind", site));
    };
    check_index(*index)?;
    if *index != expected {
        return Err(type_presentation_issue("size_witness_mismatch", site));
    }
    Ok(())
}

fn validate_short_numeric_child(
    arg: &radix_hir::HirTypeFormArg,
    site: &radix_hir::HirTypeSite,
    types: &radix_types::TypeTableSnapshot,
) -> Result<(), HirArtifactError> {
    let radix_hir::HirTypeFormArg::Type(child) = arg else {
        return Err(type_presentation_issue("short_child_kind", site));
    };
    if !matches!(
        types.types().get(child.ty.0 as usize),
        Some(radix_types::Type::SizedNumeric(_, _))
    ) || !matches!(
        child.head,
        radix_hir::HirTypeFormHead::Semantic(level) if level.0 == 1
    ) || !child.args.is_empty()
    {
        return Err(type_presentation_issue("short_width_child", site));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_named_form(
    form: &radix_hir::HirTypeForm,
    site: &radix_hir::HirTypeSite,
    types: &radix_types::TypeTableSnapshot,
    named_defs: &FxHashSet<radix_types::DefId>,
    path_def: radix_types::DefId,
    path: &[radix_lexer::Symbol],
    check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
    check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
    check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
    check_index: &dyn Fn(radix_types::IndexId) -> Result<(), HirArtifactError>,
    root_slot: &radix_hir::HirTypeSlot,
) -> Result<(), HirArtifactError> {
    check_def(path_def)?;
    if !named_defs.contains(&path_def) {
        return Err(type_presentation_issue("named_def_not_type", site));
    }
    if path.is_empty() {
        return Err(type_presentation_issue("empty_named_path", site));
    }
    for symbol in path {
        check_symbol(*symbol)?;
    }

    let (witness_def, applied_args) = named_witness(types, form.ty)
        .ok_or_else(|| type_presentation_issue("named_witness_not_named", site))?;
    if witness_def != path_def {
        return Err(type_presentation_issue(
            "named_witness_definition_mismatch",
            site,
        ));
    }
    if applied_args.len() != form.args.len() {
        return Err(type_presentation_issue("named_argument_arity", site));
    }
    for (arg, expected) in form.args.iter().zip(applied_args) {
        match expected {
            radix_types::AppliedArg::Type(expected) => match arg {
                radix_hir::HirTypeFormArg::Type(child) => {
                    if child.ty != expected {
                        return Err(type_presentation_issue("named_argument_witness", site));
                    }
                    validate_type_form(
                        child,
                        site,
                        root_slot,
                        types,
                        named_defs,
                        check_symbol,
                        check_def,
                        check_type,
                        check_index,
                    )?;
                }
                radix_hir::HirTypeFormArg::Name(symbol) if matches!(types.types().get(expected.0 as usize), Some(radix_types::Type::Param(name)) if *name == *symbol) =>
                {
                    check_symbol(*symbol)?;
                }
                radix_hir::HirTypeFormArg::Name(_) => {
                    return Err(type_presentation_issue("named_name_argument_witness", site));
                }
                radix_hir::HirTypeFormArg::Size(_) => {
                    return Err(type_presentation_issue("named_argument_kind", site));
                }
            },
            radix_types::AppliedArg::Size(_) => {
                return Err(type_presentation_issue("named_size_argument", site));
            }
        }
    }
    Ok(())
}

fn named_witness(
    types: &radix_types::TypeTableSnapshot,
    ty: radix_types::TypeId,
) -> Option<(radix_types::DefId, Vec<radix_types::AppliedArg>)> {
    match types.types().get(ty.0 as usize)? {
        radix_types::Type::Struct(def_id)
        | radix_types::Type::Enum(def_id)
        | radix_types::Type::Interface(def_id)
        | radix_types::Type::Alias(def_id, _) => Some((*def_id, Vec::new())),
        radix_types::Type::Applied(base, args) => {
            let (def_id, _) = named_witness(types, *base)?;
            Some((def_id, args.clone()))
        }
        radix_types::Type::Ref(_, inner) => named_witness(types, *inner),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_type_form(
    form: &radix_hir::HirTypeForm,
    site: &radix_hir::HirTypeSite,
    root_slot: &radix_hir::HirTypeSlot,
    types: &radix_types::TypeTableSnapshot,
    named_defs: &FxHashSet<radix_types::DefId>,
    check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
    check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
    check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
    check_index: &dyn Fn(radix_types::IndexId) -> Result<(), HirArtifactError>,
) -> Result<(), HirArtifactError> {
    check_type(form.ty)?;
    match &form.head {
        radix_hir::HirTypeFormHead::Infer | radix_hir::HirTypeFormHead::UnionHole => {
            if !matches!(
                root_slot,
                radix_hir::HirTypeSlot::Declared | radix_hir::HirTypeSlot::DiscardBinding
            ) {
                return Err(type_presentation_issue("hole_in_illegal_slot", site));
            }
            if !form.args.is_empty() {
                return Err(type_presentation_issue("hole_arguments", site));
            }
            return Ok(());
        }
        radix_hir::HirTypeFormHead::Named { def_id, path } => {
            return validate_named_form(
                form,
                site,
                types,
                named_defs,
                *def_id,
                path,
                check_symbol,
                check_def,
                check_type,
                check_index,
                root_slot,
            );
        }
        radix_hir::HirTypeFormHead::Semantic(level) => {
            let ty = types
                .types()
                .get(form.ty.0 as usize)
                .ok_or_else(|| type_presentation_issue("type_witness_missing", site))?;
            match ty {
                radix_types::Type::SizedNumeric(_, _) => {
                    if level.0 > 1 || !form.args.is_empty() {
                        return Err(type_presentation_issue("numeric_level_or_arity", site));
                    }
                }
                radix_types::Type::Primitive(_)
                | radix_types::Type::ModularWord(_)
                | radix_types::Type::SizedInstans(_)
                | radix_types::Type::BoundedTextus { .. }
                | radix_types::Type::BoundedAscii { .. }
                | radix_types::Type::BoundedOcteti { .. } => {
                    if level.0 != 0 || !form.args.is_empty() {
                        return Err(type_presentation_issue("semantic_level_or_arity", site));
                    }
                }
                radix_types::Type::Array(element) => {
                    if form.args.len() != 1 {
                        return Err(type_presentation_issue("array_argument_arity", site));
                    }
                    validate_type_arg(
                        &form.args[0],
                        *element,
                        site,
                        root_slot,
                        types,
                        named_defs,
                        check_symbol,
                        check_def,
                        check_type,
                        check_index,
                    )?;
                    if level.0 == 1 {
                        validate_short_numeric_child(&form.args[0], site, types)?;
                    } else if level.0 != 0 {
                        return Err(type_presentation_issue("array_level", site));
                    }
                }
                radix_types::Type::BoundedArray { element, .. } => {
                    if level.0 != 0 || form.args.len() != 1 {
                        return Err(type_presentation_issue("bounded_array_shape", site));
                    }
                    validate_type_arg(
                        &form.args[0],
                        *element,
                        site,
                        root_slot,
                        types,
                        named_defs,
                        check_symbol,
                        check_def,
                        check_type,
                        check_index,
                    )?;
                }
                radix_types::Type::Tensor(element, shape)
                | radix_types::Type::Vector(element, shape)
                | radix_types::Type::Matrix(element, shape)
                | radix_types::Type::Sparsa(element, shape) => {
                    let has_explicit_shape = matches!(ty, radix_types::Type::Tensor(..));
                    let expected_arity = if has_explicit_shape { 2 } else { 1 };
                    if form.args.len() != expected_arity || level.0 > 1 {
                        return Err(type_presentation_issue("aggregate_level_or_arity", site));
                    }
                    validate_type_arg(
                        &form.args[0],
                        *element,
                        site,
                        root_slot,
                        types,
                        named_defs,
                        check_symbol,
                        check_def,
                        check_type,
                        check_index,
                    )?;
                    if has_explicit_shape {
                        validate_size_arg(&form.args[1], *shape, site, check_index)?;
                    }
                    if level.0 == 1 {
                        validate_short_numeric_child(&form.args[0], site, types)?;
                    }
                    match ty {
                        radix_types::Type::Vector(_, index)
                            if matches!(
                                types.indices().get(index.0 as usize),
                                Some(radix_types::IndexExpr::Tuple(dims)) if dims.len() != 1
                            ) =>
                        {
                            return Err(type_presentation_issue("vector_shape", site));
                        }
                        radix_types::Type::Matrix(_, index)
                            if !matches!(
                                types.indices().get(index.0 as usize),
                                Some(radix_types::IndexExpr::Tuple(dims)) if dims.len() == 2
                            ) =>
                        {
                            return Err(type_presentation_issue("matrix_shape", site));
                        }
                        _ => {}
                    }
                }
                radix_types::Type::Map(key, value) => {
                    if level.0 != 0 || (!form.args.is_empty() && form.args.len() != 2) {
                        return Err(type_presentation_issue("map_shape", site));
                    }
                    if form.args.len() == 2 {
                        validate_type_arg(
                            &form.args[0],
                            *key,
                            site,
                            root_slot,
                            types,
                            named_defs,
                            check_symbol,
                            check_def,
                            check_type,
                            check_index,
                        )?;
                        validate_type_arg(
                            &form.args[1],
                            *value,
                            site,
                            root_slot,
                            types,
                            named_defs,
                            check_symbol,
                            check_def,
                            check_type,
                            check_index,
                        )?;
                    }
                }
                radix_types::Type::Record(fields) => {
                    if level.0 != 0 || form.args.len() != fields.len() {
                        return Err(type_presentation_issue("record_shape", site));
                    }
                    let mut expected = fields.values().copied().collect::<Vec<_>>();
                    for arg in &form.args {
                        let radix_hir::HirTypeFormArg::Type(child) = arg else {
                            return Err(type_presentation_issue("record_argument_kind", site));
                        };
                        let Some(position) = expected.iter().position(|ty| *ty == child.ty) else {
                            return Err(type_presentation_issue("record_witness_mismatch", site));
                        };
                        expected.swap_remove(position);
                        validate_type_form(
                            child,
                            site,
                            root_slot,
                            types,
                            named_defs,
                            check_symbol,
                            check_def,
                            check_type,
                            check_index,
                        )?;
                    }
                }
                radix_types::Type::Promissum(element)
                | radix_types::Type::Cursor(element)
                | radix_types::Type::Set(element)
                | radix_types::Type::Atomic(element)
                | radix_types::Type::Intervallum(element)
                | radix_types::Type::Option(element) => {
                    if level.0 != 0 {
                        return Err(type_presentation_issue("semantic_level", site));
                    }
                    if matches!(ty, radix_types::Type::Option(_)) {
                        match form.args.as_slice() {
                            [] => {}
                            [arg] => validate_type_arg(
                                arg,
                                *element,
                                site,
                                root_slot,
                                types,
                                named_defs,
                                check_symbol,
                                check_def,
                                check_type,
                                check_index,
                            )?,
                            _ => return Err(type_presentation_issue("option_shape", site)),
                        }
                    } else {
                        if form.args.len() != 1 {
                            return Err(type_presentation_issue("unary_argument_arity", site));
                        }
                        validate_type_arg(
                            &form.args[0],
                            *element,
                            site,
                            root_slot,
                            types,
                            named_defs,
                            check_symbol,
                            check_def,
                            check_type,
                            check_index,
                        )?;
                    }
                }
                radix_types::Type::PromissumFailable(success, alternate)
                | radix_types::Type::AsyncCursor(success, alternate) => {
                    if level.0 != 0 || form.args.len() != 2 {
                        return Err(type_presentation_issue("binary_argument_arity", site));
                    }
                    validate_type_arg(
                        &form.args[0],
                        *success,
                        site,
                        root_slot,
                        types,
                        named_defs,
                        check_symbol,
                        check_def,
                        check_type,
                        check_index,
                    )?;
                    validate_type_arg(
                        &form.args[1],
                        *alternate,
                        site,
                        root_slot,
                        types,
                        named_defs,
                        check_symbol,
                        check_def,
                        check_type,
                        check_index,
                    )?;
                }
                radix_types::Type::Ref(_, inner) => {
                    if level.0 != 0 || form.args.len() != 1 {
                        return Err(type_presentation_issue("reference_shape", site));
                    }
                    validate_type_arg(
                        &form.args[0],
                        *inner,
                        site,
                        root_slot,
                        types,
                        named_defs,
                        check_symbol,
                        check_def,
                        check_type,
                        check_index,
                    )?;
                }
                radix_types::Type::Union(_)
                | radix_types::Type::Intersection(_)
                | radix_types::Type::Func(_) => {
                    if level.0 != 0 || !form.args.is_empty() {
                        return Err(type_presentation_issue("canonical_shape", site));
                    }
                }
                radix_types::Type::Tuple(members) => {
                    if level.0 != 0 || form.args.len() != members.len() {
                        return Err(type_presentation_issue("tuple_shape", site));
                    }
                    for (arg, expected) in form.args.iter().zip(members) {
                        validate_type_arg(
                            arg,
                            *expected,
                            site,
                            root_slot,
                            types,
                            named_defs,
                            check_symbol,
                            check_def,
                            check_type,
                            check_index,
                        )?;
                    }
                }
                radix_types::Type::Param(_)
                | radix_types::Type::Infer(_)
                | radix_types::Type::InferUnion(_)
                | radix_types::Type::MarkerHole { .. }
                | radix_types::Type::Error => {
                    if level.0 != 0 || !form.args.is_empty() {
                        return Err(type_presentation_issue("opaque_shape", site));
                    }
                }
                radix_types::Type::Struct(_)
                | radix_types::Type::Enum(_)
                | radix_types::Type::Interface(_)
                | radix_types::Type::Alias(_, _)
                | radix_types::Type::Applied(_, _) => {
                    return Err(type_presentation_issue("semantic_named_constructor", site));
                }
                radix_types::Type::Queue(element) | radix_types::Type::Stack(element) => {
                    if level.0 != 0 || form.args.len() != 1 {
                        return Err(type_presentation_issue("queue_stack_shape", site));
                    }
                    validate_type_arg(
                        &form.args[0],
                        *element,
                        site,
                        root_slot,
                        types,
                        named_defs,
                        check_symbol,
                        check_def,
                        check_type,
                        check_index,
                    )?;
                }
                radix_types::Type::BoundedQueue { element, .. }
                | radix_types::Type::BoundedStack { element, .. } => {
                    if level.0 != 0 || form.args.len() != 1 {
                        return Err(type_presentation_issue("bounded_queue_stack_shape", site));
                    }
                    validate_type_arg(
                        &form.args[0],
                        *element,
                        site,
                        root_slot,
                        types,
                        named_defs,
                        check_symbol,
                        check_def,
                        check_type,
                        check_index,
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn validate_authored_type_records(
    artifact: &HirArtifact,
    presentation: &radix_hir::HirPresentation,
    check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
    check_hir: &dyn Fn(radix_hir::HirId) -> Result<(), HirArtifactError>,
    check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
    check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
    check_index: &dyn Fn(radix_types::IndexId) -> Result<(), HirArtifactError>,
) -> Result<(), HirArtifactError> {
    let sites = collect_type_site_registry(artifact);
    let named_defs = collect_named_type_defs(artifact);
    let mut seen = FxHashSet::default();
    for authored in &presentation.authored_types {
        if !seen.insert(authored.site.clone()) {
            return Err(type_presentation_issue(
                "duplicate_type_site",
                &authored.site,
            ));
        }
        match authored.site.owner {
            radix_hir::HirOwnerRef::Hir(id) => check_hir(id)?,
            radix_hir::HirOwnerRef::Def(def_id) => check_def(def_id)?,
        }
        let Some(expected) = sites.get(&authored.site).copied() else {
            return Err(type_presentation_issue("unknown_type_slot", &authored.site));
        };
        check_type(authored.form.ty)?;
        if authored.form.ty != expected {
            return Err(type_presentation_issue(
                "root_witness_mismatch",
                &authored.site,
            ));
        }
        validate_type_form(
            &authored.form,
            &authored.site,
            &authored.site.slot,
            &artifact.types,
            &named_defs,
            check_symbol,
            check_def,
            check_type,
            check_index,
        )?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Referential integrity validation (U2.3)
// ---------------------------------------------------------------------------

/// Collect every definition-site `DefId` from the artifact into a registry
/// set. The registry is used to membership-test `DefId` references: a `DefId`
/// is valid iff it is a builtin (0 < d.0 < `USER_DEF_ID_BASE`) or it
/// appears in this set. Per prep S2.
fn collect_def_registry(artifact: &HirArtifact) -> FxHashSet<radix_types::DefId> {
    use radix_hir::HirItemKind;

    let mut registry = FxHashSet::default();

    // Walk HIR items for definition-site DefIds.
    for item in &artifact.hir.items {
        registry.insert(item.def_id);
        match &item.kind {
            HirItemKind::Function(func) => {
                collect_function_defs(func, &mut registry);
            }
            HirItemKind::Struct(s) => {
                for tp in &s.type_params {
                    registry.insert(tp.def_id);
                }
                for ann in &s.annotations {
                    collect_annotation_defs(ann, &mut registry);
                }
                for field in &s.fields {
                    registry.insert(field.def_id);
                    for ann in &field.annotations {
                        collect_annotation_defs(ann, &mut registry);
                    }
                    if let Some(ref init) = field.init {
                        collect_expr_defs(init, &mut registry);
                    }
                }
                for method in &s.methods {
                    registry.insert(method.def_id);
                    collect_function_defs(&method.func, &mut registry);
                }
            }
            HirItemKind::Enum(e) => {
                for tp in &e.type_params {
                    registry.insert(tp.def_id);
                }
                for variant in &e.variants {
                    registry.insert(variant.def_id);
                }
            }
            HirItemKind::Interface(iface) => {
                for tp in &iface.type_params {
                    registry.insert(tp.def_id);
                }
                for method in &iface.methods {
                    for param in &method.params {
                        collect_param_defs(param, &mut registry);
                    }
                }
            }
            HirItemKind::TypeAlias(a) => {
                for tp in &a.type_params {
                    registry.insert(tp.def_id);
                }
            }
            HirItemKind::Schema(_schema) => {
                // The item def_id is registered above; schema columns carry
                // symbols and type ids, never def ids of their own.
            }
            HirItemKind::Constant(c) => {
                collect_expr_defs(&c.value, &mut registry);
            }
            HirItemKind::Import(imp) => {
                for ii in &imp.items {
                    registry.insert(ii.def_id);
                }
            }
        }
    }

    // Entry block (if present).
    if let Some(ref entry) = artifact.hir.entry {
        collect_block_defs(entry, &mut registry);
    }

    // Annotations on entry.
    for ann in &artifact.hir.entry_annotations {
        if let Some(def_id) = ann.contract_id {
            registry.insert(def_id);
        }
        for field in &ann.fields {
            if let radix_hir::HirAnnotationValue::Expr(ref e) = field.value {
                collect_expr_defs(e, &mut registry);
            }
        }
    }

    // LibraryRegistry keys (bindings, items, exports, reexports).
    for def_id in artifact.libraries.bindings.keys() {
        registry.insert(*def_id);
    }
    for binding in artifact.libraries.bindings.values() {
        registry.insert(binding.local_def_id);
    }
    for def_id in artifact.libraries.items.keys() {
        registry.insert(*def_id);
    }
    for item in artifact.libraries.items.values() {
        registry.insert(item.def_id);
    }
    for def_id in artifact.libraries.exports.keys() {
        registry.insert(*def_id);
    }
    for (pair, _) in artifact.libraries.reexports.keys() {
        registry.insert(*pair);
    }
    // Registry conversion rows name ordinary in-tree functions; their def
    // sites come from the HIR walk above, so a row naming any other def
    // fails the membership check below instead of self-blessing here.

    // Imported-nominal def sites from the resolver snapshot. The resolver
    // allocates imported nominal types (structs, enums, interfaces) as fresh
    // unit-local DefIds and carries each one as a file-interface export
    // identity (`Struct` exports and `Type` exports with `Some(def_id)`); the
    // definition site lives only in the resolver snapshot on the wire (the
    // resolver's runtime `symbols` map is excluded per CTM E.2). Without
    // collecting these def sites, a valid artifact whose resolver snapshot
    // references an imported nominal def fails the membership check with a
    // false `DanglingIndex` (U2.3, S2). The type arena references the same
    // defs as `Type::Struct`/`Enum`/`Interface`, so the type-arena walk stays
    // consistent; a type-table-only DefId (no export identity anywhere) still
    // fails closed.
    for (_, iface) in &artifact.resolver.file_interfaces {
        for (_, export) in &iface.exports {
            match export {
                crate::artifact::FileExportSnapshot::Struct { def_id, .. } => {
                    registry.insert(*def_id);
                }
                crate::artifact::FileExportSnapshot::Type {
                    def_id: Some(d), ..
                } => {
                    registry.insert(*d);
                }
                crate::artifact::FileExportSnapshot::Type { def_id: None, .. }
                | crate::artifact::FileExportSnapshot::Function(_) => {}
            }
        }
    }

    registry
}

/// Register a function's own def sites and any nested sites in defaults,
/// annotations, or the body. Parameter defaults can introduce closures,
/// `itera` bindings, and `cape` bindings; skipping them false-rejects a
/// valid artifact at `check_def`.
fn collect_function_defs(
    func: &radix_hir::HirFunction,
    registry: &mut FxHashSet<radix_types::DefId>,
) {
    for tp in &func.type_params {
        registry.insert(tp.def_id);
    }
    for param in &func.params {
        collect_param_defs(param, registry);
    }
    if let Some(ref cli) = func.cli_args {
        collect_param_defs(cli, registry);
    }
    for ann in &func.annotations {
        collect_annotation_defs(ann, registry);
    }
    if let Some(ref body) = func.body {
        collect_block_defs(body, registry);
    }
}

fn collect_param_defs(param: &radix_hir::HirParam, registry: &mut FxHashSet<radix_types::DefId>) {
    registry.insert(param.def_id);
    if let Some(ref init) = param.default {
        collect_expr_defs(init, registry);
    }
}

/// Collect definition-site `DefIds` from a HIR block, recursing into
/// statements and expressions.
fn collect_block_defs(block: &radix_hir::HirBlock, registry: &mut FxHashSet<radix_types::DefId>) {
    use radix_hir::HirStatementKind;

    for stmt in &block.statements {
        if let HirStatementKind::Local(local) = &stmt.kind {
            registry.insert(local.def_id);
        }
        collect_stmt_expr_defs(&stmt.kind, registry);
    }
    if let Some(ref expr) = block.expr {
        collect_expr_defs(expr, registry);
    }
}

/// Collect definition-site `DefIds` from expression trees reachable from a
/// statement (local init, Expr stmt, Redde, `IncDec`, Custodi).
fn collect_stmt_expr_defs(
    kind: &radix_hir::HirStatementKind,
    registry: &mut FxHashSet<radix_types::DefId>,
) {
    use radix_hir::HirStatementKind;
    match kind {
        HirStatementKind::Local(local) => {
            if let Some(ref init) = local.init {
                collect_expr_defs(init, registry);
            }
        }
        HirStatementKind::Expr(expr) | HirStatementKind::Redde(Some(expr)) => {
            collect_expr_defs(expr, registry);
        }
        HirStatementKind::IncDec(inc) => {
            collect_expr_defs(&inc.target, registry);
        }
        HirStatementKind::Custodi(cust) => {
            for clause in &cust.clauses {
                collect_expr_defs(&clause.cond, registry);
                collect_block_defs(&clause.body, registry);
            }
        }
        HirStatementKind::Rumpe
        | HirStatementKind::Perge
        | HirStatementKind::Tacet
        | HirStatementKind::Redde(None) => {}
    }
}

/// Collect definition-site `DefIds` from annotation expression payloads.
///
/// The validator walks item-annotation subtrees (expression fields, contract
/// references); the registry must register definition sites in the same
/// regions or a valid artifact whose annotation references them would fail
/// the membership check with a false `DanglingIndex`.
fn collect_annotation_defs(
    ann: &radix_hir::HirAnnotation,
    registry: &mut FxHashSet<radix_types::DefId>,
) {
    for field in &ann.fields {
        if let radix_hir::HirAnnotationValue::Expr(ref e) = field.value {
            collect_expr_defs(e, registry);
        }
    }
}

/// Collect definition-site `DefIds` from a HIR expression tree.
fn collect_expr_defs(
    expr: &radix_hir::HirExpression,
    registry: &mut FxHashSet<radix_types::DefId>,
) {
    use radix_hir::HirExpressionKind;

    match &expr.kind {
        HirExpressionKind::Path(_)
        | HirExpressionKind::Literal(_)
        | HirExpressionKind::Vacua
        | HirExpressionKind::ReadLine(_)
        | HirExpressionKind::Error => {}
        HirExpressionKind::Summa {
            source,
            coordinates,
            filum,
            binding,
            body,
        } => {
            collect_expr_defs(source, registry);
            for (def_id, _) in coordinates {
                registry.insert(*def_id);
            }
            if let Some((def_id, _)) = filum {
                registry.insert(*def_id);
            }
            registry.insert(binding.0);
            collect_block_defs(body, registry);
        }
        HirExpressionKind::Extremum {
            source,
            coordinates,
            coalesce,
            ..
        } => {
            collect_expr_defs(source, registry);
            for (def_id, _) in coordinates {
                registry.insert(*def_id);
            }
            if let Some(identity) = coalesce {
                collect_expr_defs(identity, registry);
            }
        }
        HirExpressionKind::FirstMatch {
            source,
            coordinates,
            binding,
            body,
        } => {
            collect_expr_defs(source, registry);
            for (def_id, _) in coordinates {
                registry.insert(*def_id);
            }
            registry.insert(binding.0);
            collect_block_defs(body, registry);
        }
        HirExpressionKind::Praefixum(inner) => {
            collect_expr_defs(inner, registry);
        }
        HirExpressionKind::Transfer { sink, payloads } => {
            collect_expr_defs(sink, registry);
            for payload in payloads {
                collect_expr_defs(payload, registry);
            }
        }
        HirExpressionKind::Binary(_, lhs, rhs) => {
            collect_expr_defs(lhs, registry);
            collect_expr_defs(rhs, registry);
        }
        HirExpressionKind::Unary(_, inner)
        | HirExpressionKind::Ref(_, inner)
        | HirExpressionKind::Deref(inner) => {
            collect_expr_defs(inner, registry);
        }
        HirExpressionKind::Call(callee, _, args, _) => {
            collect_expr_defs(callee, registry);
            for arg in args {
                collect_expr_defs(&arg.expr, registry);
            }
        }
        HirExpressionKind::Gradient {
            call,
            selected_arguments,
        } => {
            collect_expr_defs(call, registry);
            for selector in selected_arguments {
                collect_expr_defs(selector, registry);
            }
        }
        HirExpressionKind::MethodCall(receiver, _, _, args, _) => {
            collect_expr_defs(receiver, registry);
            for arg in args {
                collect_expr_defs(&arg.expr, registry);
            }
        }
        HirExpressionKind::Field(target, _) => {
            collect_expr_defs(target, registry);
        }
        HirExpressionKind::Index(target, index) => {
            collect_expr_defs(target, registry);
            collect_expr_defs(index, registry);
        }
        HirExpressionKind::OptionalChain(base, chain) => {
            collect_expr_defs(base, registry);
            match chain {
                radix_hir::HirOptionalChainKind::Member(_) => {}
                radix_hir::HirOptionalChainKind::Index(e) => collect_expr_defs(e, registry),
                radix_hir::HirOptionalChainKind::Call(args) => {
                    for arg in args {
                        collect_expr_defs(&arg.expr, registry);
                    }
                }
            }
        }
        HirExpressionKind::NonNull(base, chain) => {
            collect_expr_defs(base, registry);
            match chain {
                radix_hir::HirNonNullKind::Member(_) => {}
                radix_hir::HirNonNullKind::Index(e) => collect_expr_defs(e, registry),
                radix_hir::HirNonNullKind::Call(args) => {
                    for arg in args {
                        collect_expr_defs(&arg.expr, registry);
                    }
                }
            }
        }
        HirExpressionKind::Block(block) => collect_block_defs(block, registry),
        HirExpressionKind::Si {
            cond,
            then_block,
            then_catch,
            else_block,
        } => {
            collect_expr_defs(cond, registry);
            collect_block_defs(then_block, registry);
            if let Some(cape) = then_catch {
                registry.insert(cape.binding_def_id);
                collect_block_defs(&cape.body, registry);
            }
            if let Some(else_block) = else_block {
                collect_block_defs(else_block, registry);
            }
        }
        HirExpressionKind::Discerne {
            scrutinees, arms, ..
        } => {
            for s in scrutinees {
                collect_expr_defs(s, registry);
            }
            for arm in arms {
                for pat in &arm.patterns {
                    collect_pattern_defs(pat, registry);
                }
                if let Some(ref guard) = arm.guard {
                    collect_expr_defs(guard, registry);
                }
                collect_expr_defs(&arm.body, registry);
            }
        }
        HirExpressionKind::Loop(body) => collect_block_defs(body, registry),
        HirExpressionKind::Dum(cond, body) => {
            collect_expr_defs(cond, registry);
            collect_block_defs(body, registry);
        }
        HirExpressionKind::Itera(_, coordinates, def_id, _, iterable, body) => {
            for (coordinate_def_id, _) in coordinates {
                registry.insert(*coordinate_def_id);
            }
            registry.insert(*def_id);
            collect_expr_defs(iterable, registry);
            collect_block_defs(body, registry);
        }
        HirExpressionKind::IteraLockstep {
            sources,
            coordinates,
            bindings,
            body,
        } => {
            for (coordinate_def_id, _) in coordinates {
                registry.insert(*coordinate_def_id);
            }
            for (binding_def_id, _) in bindings {
                registry.insert(*binding_def_id);
            }
            for source in sources {
                collect_expr_defs(source, registry);
            }
            collect_block_defs(body, registry);
        }
        // NR-5 (nary-range-walk): the product registers its binder defs —
        // they are definition sites, so in-body binder paths resolve — then
        // walks ranges and body.
        HirExpressionKind::IteraRange {
            ranges,
            bindings,
            body,
        } => {
            for (binding_def_id, _) in bindings {
                registry.insert(*binding_def_id);
            }
            for range in ranges {
                collect_expr_defs(range, registry);
            }
            collect_block_defs(body, registry);
        }
        HirExpressionKind::Intervallum {
            start, end, step, ..
        } => {
            collect_expr_defs(start, registry);
            collect_expr_defs(end, registry);
            if let Some(s) = step {
                collect_expr_defs(s, registry);
            }
        }
        HirExpressionKind::Assign(lhs, rhs) => {
            collect_expr_defs(lhs, registry);
            collect_expr_defs(rhs, registry);
        }
        HirExpressionKind::ConversioAssign {
            target,
            source,
            recovery,
        } => {
            collect_expr_defs(target, registry);
            collect_expr_defs(source, registry);
            if let Some(recovery) = recovery {
                collect_expr_defs(recovery, registry);
            }
        }
        HirExpressionKind::Array(elements) => {
            for elem in elements {
                match elem {
                    radix_hir::HirArrayElement::Expr(e) | radix_hir::HirArrayElement::Spread(e) => {
                        collect_expr_defs(e, registry);
                    }
                }
            }
        }
        HirExpressionKind::Struct(_, fields) => {
            for (_, e) in fields {
                collect_expr_defs(e, registry);
            }
        }
        HirExpressionKind::Tuple(elements, _) => {
            for e in elements {
                collect_expr_defs(e, registry);
            }
        }
        HirExpressionKind::Scribe(_, args) => {
            for a in args {
                collect_expr_defs(a, registry);
            }
        }
        HirExpressionKind::Scriptum(_, args) => {
            for a in args {
                collect_expr_defs(a, registry);
            }
        }
        HirExpressionKind::Adfirma(expr, msg) => {
            collect_expr_defs(expr, registry);
            if let Some(m) = msg {
                collect_expr_defs(m, registry);
            }
        }
        HirExpressionKind::Panic(expr) | HirExpressionKind::Throw(expr) => {
            collect_expr_defs(expr, registry);
        }
        HirExpressionKind::Handled { body, catch } => {
            collect_block_defs(body, registry);
            registry.insert(catch.binding_def_id);
            collect_block_defs(&catch.body, registry);
        }
        HirExpressionKind::Trap(body) => {
            collect_block_defs(body, registry);
        }
        HirExpressionKind::Clausura(params, _, _, body, _) => {
            for param in params {
                collect_param_defs(param, registry);
            }
            collect_expr_defs(body, registry);
        }
        HirExpressionKind::Cede(inner)
        | HirExpressionKind::Reddet(inner)
        | HirExpressionKind::Tacebit(inner)
        | HirExpressionKind::Yield(inner) => collect_expr_defs(inner, registry),
        HirExpressionKind::Verte {
            source, entries, ..
        } => {
            collect_expr_defs(source, registry);
            if let Some(entries) = entries {
                for entry in entries {
                    if let radix_hir::HirObjectKey::Computed(ref e)
                    | radix_hir::HirObjectKey::Spread(ref e) = entry.key
                    {
                        collect_expr_defs(e, registry);
                    }
                    if let Some(ref v) = entry.value {
                        collect_expr_defs(v, registry);
                    }
                }
            }
        }
        HirExpressionKind::Conversio {
            source,
            target,
            recovery,
            ..
        } => {
            collect_expr_defs(source, registry);
            if let radix_hir::HirConversioTarget::Intervallum(e) = target {
                collect_expr_defs(e, registry);
            }
            if let Some(r) = recovery {
                collect_expr_defs(r, registry);
            }
        }
        HirExpressionKind::Ad { opener, .. } => {
            if let Some(o) = opener {
                collect_expr_defs(o, registry);
            }
        }
        HirExpressionKind::TypeCheck { expr, .. } => {
            collect_expr_defs(expr, registry);
        }
    }
}

/// Collect definition-site `DefIds` from a HIR pattern.
fn collect_pattern_defs(pat: &radix_hir::HirPattern, registry: &mut FxHashSet<radix_types::DefId>) {
    match pat {
        radix_hir::HirPattern::Binding(def_id, _)
        | radix_hir::HirPattern::Alias(def_id, _, _)
        | radix_hir::HirPattern::Variant(def_id, _) => {
            registry.insert(*def_id);
        }
        radix_hir::HirPattern::TypeTest { binding, .. } => {
            if let Some((def_id, _)) = binding {
                registry.insert(*def_id);
            }
        }
        radix_hir::HirPattern::Literal(_) | radix_hir::HirPattern::Wildcard => {}
    }
    if let radix_hir::HirPattern::Alias(_, _, inner) = pat {
        collect_pattern_defs(inner, registry);
    }
    if let radix_hir::HirPattern::Variant(_, sub_patterns) = pat {
        for sp in sub_patterns {
            collect_pattern_defs(sp, registry);
        }
    }
}

/// Collect all `HirIds` from the HIR tree into a set for membership-test
/// validation. This accounts for nested statements and expressions that
/// have their own `HirIds` beyond the top-level items.
fn collect_hir_registry(artifact: &HirArtifact) -> FxHashSet<radix_hir::HirId> {
    use radix_hir::{HirExpressionKind, HirStatementKind};

    let mut registry = FxHashSet::default();

    fn collect_block(block: &radix_hir::HirBlock, registry: &mut FxHashSet<radix_hir::HirId>) {
        for stmt in &block.statements {
            registry.insert(stmt.id);
            match &stmt.kind {
                HirStatementKind::Expr(expr) | HirStatementKind::Redde(Some(expr)) => {
                    collect_expr(expr, registry);
                }
                HirStatementKind::IncDec(inc) => collect_expr(&inc.target, registry),
                HirStatementKind::Local(local) => {
                    if let Some(ref init) = local.init {
                        collect_expr(init, registry);
                    }
                }
                HirStatementKind::Custodi(cust) => {
                    for clause in &cust.clauses {
                        collect_expr(&clause.cond, registry);
                        collect_block(&clause.body, registry);
                    }
                }
                HirStatementKind::Rumpe
                | HirStatementKind::Perge
                | HirStatementKind::Tacet
                | HirStatementKind::Redde(None) => {}
            }
        }
        if let Some(ref expr) = block.expr {
            collect_expr(expr, registry);
        }
    }

    fn collect_expr(expr: &radix_hir::HirExpression, registry: &mut FxHashSet<radix_hir::HirId>) {
        registry.insert(expr.id);
        match &expr.kind {
            HirExpressionKind::Summa { source, body, .. } => {
                collect_expr(source, registry);
                collect_block(body, registry);
            }
            HirExpressionKind::Extremum {
                source, coalesce, ..
            } => {
                collect_expr(source, registry);
                if let Some(identity) = coalesce {
                    collect_expr(identity, registry);
                }
            }
            HirExpressionKind::FirstMatch { source, body, .. } => {
                collect_expr(source, registry);
                collect_block(body, registry);
            }
            HirExpressionKind::Binary(_, lhs, rhs) => {
                collect_expr(lhs, registry);
                collect_expr(rhs, registry);
            }
            HirExpressionKind::Unary(_, inner)
            | HirExpressionKind::Ref(_, inner)
            | HirExpressionKind::Deref(inner) => {
                collect_expr(inner, registry);
            }
            HirExpressionKind::Call(callee, _, args, _) => {
                collect_expr(callee, registry);
                for arg in args {
                    collect_expr(&arg.expr, registry);
                }
            }
            HirExpressionKind::Transfer { sink, payloads } => {
                collect_expr(sink, registry);
                for payload in payloads {
                    collect_expr(payload, registry);
                }
            }
            HirExpressionKind::Gradient {
                call,
                selected_arguments,
            } => {
                collect_expr(call, registry);
                for selector in selected_arguments {
                    collect_expr(selector, registry);
                }
            }
            HirExpressionKind::MethodCall(receiver, _, _, args, _) => {
                collect_expr(receiver, registry);
                for arg in args {
                    collect_expr(&arg.expr, registry);
                }
            }
            HirExpressionKind::Field(target, _) => {
                collect_expr(target, registry);
            }
            HirExpressionKind::Index(target, index) => {
                collect_expr(target, registry);
                collect_expr(index, registry);
            }
            HirExpressionKind::OptionalChain(base, chain) => {
                collect_expr(base, registry);
                match chain {
                    radix_hir::HirOptionalChainKind::Call(args) => {
                        for arg in args {
                            collect_expr(&arg.expr, registry);
                        }
                    }
                    radix_hir::HirOptionalChainKind::Index(e) => collect_expr(e, registry),
                    radix_hir::HirOptionalChainKind::Member(_) => {}
                }
            }
            HirExpressionKind::NonNull(base, chain) => {
                collect_expr(base, registry);
                match chain {
                    radix_hir::HirNonNullKind::Call(args) => {
                        for arg in args {
                            collect_expr(&arg.expr, registry);
                        }
                    }
                    radix_hir::HirNonNullKind::Index(e) => collect_expr(e, registry),
                    radix_hir::HirNonNullKind::Member(_) => {}
                }
            }
            HirExpressionKind::Block(block) => collect_block(block, registry),
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => {
                collect_expr(cond, registry);
                collect_block(then_block, registry);
                if let Some(cape) = then_catch {
                    collect_block(&cape.body, registry);
                }
                if let Some(else_block) = else_block {
                    collect_block(else_block, registry);
                }
            }
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => {
                for s in scrutinees {
                    collect_expr(s, registry);
                }
                for arm in arms {
                    if let Some(ref guard) = arm.guard {
                        collect_expr(guard, registry);
                    }
                    collect_expr(&arm.body, registry);
                }
            }
            HirExpressionKind::Loop(body) => collect_block(body, registry),
            HirExpressionKind::Dum(cond, body) => {
                collect_expr(cond, registry);
                collect_block(body, registry);
            }
            HirExpressionKind::Itera(_, _, _, _, iterable, body) => {
                collect_expr(iterable, registry);
                collect_block(body, registry);
            }
            HirExpressionKind::IteraLockstep { sources, body, .. } => {
                for source in sources {
                    collect_expr(source, registry);
                }
                collect_block(body, registry);
            }
            // NR-5 (nary-range-walk): collect the product's range and body
            // HirIds into the membership registry.
            HirExpressionKind::IteraRange { ranges, body, .. } => {
                for range in ranges {
                    collect_expr(range, registry);
                }
                collect_block(body, registry);
            }
            HirExpressionKind::Assign(lhs, rhs) => {
                collect_expr(lhs, registry);
                collect_expr(rhs, registry);
            }
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
            } => {
                collect_expr(target, registry);
                collect_expr(source, registry);
                if let Some(recovery) = recovery {
                    collect_expr(recovery, registry);
                }
            }
            HirExpressionKind::Array(elements) => {
                for elem in elements {
                    match elem {
                        radix_hir::HirArrayElement::Expr(e)
                        | radix_hir::HirArrayElement::Spread(e) => {
                            collect_expr(e, registry);
                        }
                    }
                }
            }
            HirExpressionKind::Struct(_, fields) => {
                for (_, e) in fields {
                    collect_expr(e, registry);
                }
            }
            HirExpressionKind::Tuple(elements, _) => {
                for e in elements {
                    collect_expr(e, registry);
                }
            }
            HirExpressionKind::Scribe(_, args) | HirExpressionKind::Scriptum(_, args) => {
                for a in args {
                    collect_expr(a, registry);
                }
            }
            HirExpressionKind::Adfirma(expr, msg) => {
                collect_expr(expr, registry);
                if let Some(m) = msg {
                    collect_expr(m, registry);
                }
            }
            HirExpressionKind::Panic(expr)
            | HirExpressionKind::Throw(expr)
            | HirExpressionKind::Cede(expr)
            | HirExpressionKind::Reddet(expr)
            | HirExpressionKind::Tacebit(expr)
            | HirExpressionKind::Yield(expr) => {
                collect_expr(expr, registry);
            }
            HirExpressionKind::Handled { body, catch } => {
                collect_block(body, registry);
                collect_block(&catch.body, registry);
            }
            HirExpressionKind::Trap(body) => {
                collect_block(body, registry);
            }
            HirExpressionKind::Clausura(params, _, _, body, _) => {
                for param in params {
                    if let Some(ref init) = param.default {
                        collect_expr(init, registry);
                    }
                }
                collect_expr(body, registry);
            }
            HirExpressionKind::Verte {
                source, entries, ..
            } => {
                collect_expr(source, registry);
                if let Some(entries) = entries {
                    for entry in entries {
                        if let radix_hir::HirObjectKey::Computed(ref e)
                        | radix_hir::HirObjectKey::Spread(ref e) = entry.key
                        {
                            collect_expr(e, registry);
                        }
                        if let Some(ref v) = entry.value {
                            collect_expr(v, registry);
                        }
                    }
                }
            }
            HirExpressionKind::Conversio {
                source,
                target,
                recovery,
                ..
            } => {
                collect_expr(source, registry);
                if let radix_hir::HirConversioTarget::Intervallum(e) = target {
                    collect_expr(e, registry);
                }
                if let Some(r) = recovery {
                    collect_expr(r, registry);
                }
            }
            HirExpressionKind::Ad { opener, .. } => {
                if let Some(o) = opener {
                    collect_expr(o, registry);
                }
            }
            HirExpressionKind::TypeCheck { expr, .. } => collect_expr(expr, registry),
            HirExpressionKind::Praefixum(inner) => collect_expr(inner, registry),
            HirExpressionKind::Intervallum {
                start, end, step, ..
            } => {
                collect_expr(start, registry);
                collect_expr(end, registry);
                if let Some(s) = step {
                    collect_expr(s, registry);
                }
            }
            HirExpressionKind::Path(_)
            | HirExpressionKind::Literal(_)
            | HirExpressionKind::Vacua
            | HirExpressionKind::ReadLine(_)
            | HirExpressionKind::Error => {}
        }
    }

    fn collect_annotation(
        ann: &radix_hir::HirAnnotation,
        registry: &mut FxHashSet<radix_hir::HirId>,
    ) {
        for field in &ann.fields {
            if let radix_hir::HirAnnotationValue::Expr(ref e) = field.value {
                collect_expr(e, registry);
            }
        }
    }

    // Collect from all items and entry.
    for item in &artifact.hir.items {
        registry.insert(item.id);
        match &item.kind {
            radix_hir::HirItemKind::Function(func) => {
                for ann in &func.annotations {
                    collect_annotation(ann, &mut registry);
                }
                for param in &func.params {
                    if let Some(ref init) = param.default {
                        collect_expr(init, &mut registry);
                    }
                }
                if let Some(ref body) = func.body {
                    collect_block(body, &mut registry);
                }
            }
            radix_hir::HirItemKind::Struct(s) => {
                for ann in &s.annotations {
                    collect_annotation(ann, &mut registry);
                }
                for field in &s.fields {
                    for ann in &field.annotations {
                        collect_annotation(ann, &mut registry);
                    }
                    if let Some(ref init) = field.init {
                        collect_expr(init, &mut registry);
                    }
                }
                for method in &s.methods {
                    for ann in &method.func.annotations {
                        collect_annotation(ann, &mut registry);
                    }
                    for param in &method.func.params {
                        if let Some(ref init) = param.default {
                            collect_expr(init, &mut registry);
                        }
                    }
                    if let Some(ref body) = method.func.body {
                        collect_block(body, &mut registry);
                    }
                }
            }
            radix_hir::HirItemKind::Interface(iface) => {
                for method in &iface.methods {
                    for param in &method.params {
                        if let Some(ref init) = param.default {
                            collect_expr(init, &mut registry);
                        }
                    }
                }
            }
            radix_hir::HirItemKind::Constant(c) => {
                collect_expr(&c.value, &mut registry);
            }
            _ => {}
        }
    }
    if let Some(ref entry) = artifact.hir.entry {
        collect_block(entry, &mut registry);
    }
    // Entry annotations may contain expressions.
    for ann in &artifact.hir.entry_annotations {
        collect_annotation(ann, &mut registry);
    }

    registry
}

/// Validate every arena index in the deserialized artifact against its target
/// arena bounds. Returns `Err(DanglingIndex)` on the first out-of-bounds index
/// (fail-fast per CSO §F). Runs after arena reconstruction and before
/// `Ok(AnalyzedModule)` return so no codegen touches corrupt data (CSO E8).
pub(crate) fn validate_referential_integrity(
    artifact: &HirArtifact,
    type_count: usize,
    interner_len: usize,
) -> Result<(), HirArtifactError> {
    // ――― DefId registry (S2: collect every definition-site DefId) ―――――――
    let def_registry = collect_def_registry(artifact);

    // HirId registry: collect every HirId-bearing node in the tree (items +
    // statements + expressions) into a membership-test set. HirIds are
    // allocated by a monotonic counter in the lowerer, but error recovery
    // discards partially-constructed nodes, leaving gaps in the ID space.
    // A count-based bound would false-positive on those gaps; only a
    // membership test is correct.
    let hir_registry = collect_hir_registry(artifact);

    // Convenience closures that return Err on first violation.
    let check_symbol = |sym: radix_lexer::Symbol| -> Result<(), HirArtifactError> {
        let idx = sym.0 as usize;
        if idx >= interner_len {
            return Err(HirArtifactError::DanglingIndex {
                kind: "Symbol",
                index: sym.0,
                arena_size: interner_len,
            });
        }
        Ok(())
    };
    let check_def = |def_id: radix_types::DefId| -> Result<(), HirArtifactError> {
        // Builtin range [1, USER_DEF_ID_BASE) is always valid.
        if def_id.0 > 0 && def_id.0 < radix_types::USER_DEF_ID_BASE {
            return Ok(());
        }
        if !def_registry.contains(&def_id) {
            return Err(HirArtifactError::DanglingIndex {
                kind: "DefId",
                index: def_id.0,
                arena_size: def_registry.len(),
            });
        }
        Ok(())
    };
    let check_hir = |hir_id: radix_hir::HirId| -> Result<(), HirArtifactError> {
        if !hir_registry.contains(&hir_id) {
            return Err(HirArtifactError::DanglingIndex {
                kind: "HirId",
                index: hir_id.0,
                arena_size: hir_registry.len(),
            });
        }
        Ok(())
    };
    let check_type = |ty: radix_types::TypeId| -> Result<(), HirArtifactError> {
        let idx = ty.0 as usize;
        if idx >= type_count {
            return Err(HirArtifactError::DanglingIndex {
                kind: "TypeId",
                index: ty.0,
                arena_size: type_count,
            });
        }
        Ok(())
    };

    use radix_hir::{HirExpressionKind, HirItemKind, HirStatementKind};

    // A `Typus` default names a concrete type arena slot; `Magnitudo` defaults
    // are literals and carry no reference.
    fn validate_type_param_default(
        default: &Option<radix_types::SemanticGenericParamDefault>,
        check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
    ) -> Result<(), HirArtifactError> {
        if let Some(radix_types::SemanticGenericParamDefault::Typus(ty)) = default {
            check_type(*ty)?;
        }
        Ok(())
    }

    // Recursive walkers for the HIR tree.
    fn validate_items(
        items: &[radix_hir::HirItem],
        check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
        check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
        check_hir: &dyn Fn(radix_hir::HirId) -> Result<(), HirArtifactError>,
        check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
    ) -> Result<(), HirArtifactError> {
        for item in items {
            check_hir(item.id)?;
            check_def(item.def_id)?;
            validate_item_kind(&item.kind, check_symbol, check_def, check_hir, check_type)?;
        }
        Ok(())
    }

    fn validate_function(
        func: &radix_hir::HirFunction,
        check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
        check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
        check_hir: &dyn Fn(radix_hir::HirId) -> Result<(), HirArtifactError>,
        check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
    ) -> Result<(), HirArtifactError> {
        check_symbol(func.name)?;
        for tp in &func.type_params {
            check_def(tp.def_id)?;
            check_symbol(tp.name)?;
            if let radix_hir::HirTypeParamConstraint::OneOf(tys) = &tp.constraint {
                for ty in tys {
                    check_type(*ty)?;
                }
            }
            validate_type_param_default(&tp.default, check_type)?;
        }
        for param in &func.params {
            check_def(param.def_id)?;
            check_symbol(param.name)?;
            check_type(param.ty)?;
            if let Some(ref init) = param.default {
                validate_expr(init, check_symbol, check_def, check_hir, check_type)?;
            }
        }
        if let Some(ref cli_args) = func.cli_args {
            check_def(cli_args.def_id)?;
            check_symbol(cli_args.name)?;
            check_type(cli_args.ty)?;
        }
        if let Some(ty) = func.ret_ty {
            check_type(ty)?;
        }
        if let Some(ty) = func.err_ty {
            check_type(ty)?;
        }
        if let Some(ref body) = func.body {
            validate_block(body, check_symbol, check_def, check_hir, check_type)?;
        }
        if let Some(ref test_meta) = func.test {
            check_symbol(test_meta.name)?;
            for sym in &test_meta.suite_path {
                check_symbol(*sym)?;
            }
            for m in &test_meta.modifiers {
                match m {
                    radix_hir::HirTestModifier::Omitte(s)
                    | radix_hir::HirTestModifier::Futurum(s)
                    | radix_hir::HirTestModifier::Tag(s)
                    | radix_hir::HirTestModifier::SolumIn(s) => check_symbol(*s)?,
                    _ => {}
                }
            }
        }
        for ann in &func.annotations {
            validate_annotation(ann, check_symbol, check_def, check_hir, check_type)?;
        }
        Ok(())
    }

    fn validate_item_kind(
        kind: &HirItemKind,
        check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
        check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
        check_hir: &dyn Fn(radix_hir::HirId) -> Result<(), HirArtifactError>,
        check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
    ) -> Result<(), HirArtifactError> {
        match kind {
            HirItemKind::Function(func) => {
                validate_function(func, check_symbol, check_def, check_hir, check_type)?;
            }
            HirItemKind::Struct(s) => {
                check_symbol(s.name)?;
                for tp in &s.type_params {
                    check_def(tp.def_id)?;
                    check_symbol(tp.name)?;
                    if let radix_hir::HirTypeParamConstraint::OneOf(tys) = &tp.constraint {
                        for ty in tys {
                            check_type(*ty)?;
                        }
                    }
                    validate_type_param_default(&tp.default, check_type)?;
                }
                for ann in &s.annotations {
                    validate_annotation(ann, check_symbol, check_def, check_hir, check_type)?;
                }
                for field in &s.fields {
                    check_def(field.def_id)?;
                    check_symbol(field.name)?;
                    check_type(field.ty)?;
                    for ann in &field.annotations {
                        validate_annotation(ann, check_symbol, check_def, check_hir, check_type)?;
                    }
                    if let Some(ref init) = field.init {
                        validate_expr(init, check_symbol, check_def, check_hir, check_type)?;
                    }
                }
                for method in &s.methods {
                    check_def(method.def_id)?;
                    validate_function(
                        &method.func,
                        check_symbol,
                        check_def,
                        check_hir,
                        check_type,
                    )?;
                }
                if let Some(d) = s.extends {
                    check_def(d)?;
                }
                for d in &s.implements {
                    check_def(*d)?;
                }
            }
            HirItemKind::Enum(e) => {
                check_symbol(e.name)?;
                for tp in &e.type_params {
                    check_def(tp.def_id)?;
                    check_symbol(tp.name)?;
                    if let radix_hir::HirTypeParamConstraint::OneOf(tys) = &tp.constraint {
                        for ty in tys {
                            check_type(*ty)?;
                        }
                    }
                    validate_type_param_default(&tp.default, check_type)?;
                }
                for field in &e.commune_fields {
                    check_symbol(field.name)?;
                    check_type(field.ty)?;
                }
                for variant in &e.variants {
                    check_def(variant.def_id)?;
                    check_symbol(variant.name)?;
                    for field in &variant.fields {
                        check_symbol(field.name)?;
                        check_type(field.ty)?;
                    }
                }
            }
            HirItemKind::Interface(iface) => {
                check_symbol(iface.name)?;
                for tp in &iface.type_params {
                    check_def(tp.def_id)?;
                    check_symbol(tp.name)?;
                    if let radix_hir::HirTypeParamConstraint::OneOf(tys) = &tp.constraint {
                        for ty in tys {
                            check_type(*ty)?;
                        }
                    }
                    validate_type_param_default(&tp.default, check_type)?;
                }
                for method in &iface.methods {
                    for a in &method.availability {
                        let radix_hir::HirAvailability::Nondum { target, reason } = a;
                        if let Some(t) = target {
                            check_symbol(*t)?;
                        }
                        if let Some(r) = reason {
                            check_symbol(*r)?;
                        }
                    }
                    check_symbol(method.name)?;
                    for param in &method.params {
                        check_def(param.def_id)?;
                        check_symbol(param.name)?;
                        check_type(param.ty)?;
                        if let Some(ref init) = param.default {
                            validate_expr(init, check_symbol, check_def, check_hir, check_type)?;
                        }
                    }
                    if let Some(ty) = method.ret_ty {
                        check_type(ty)?;
                    }
                    if let Some(ty) = method.err_ty {
                        check_type(ty)?;
                    }
                }
            }
            HirItemKind::TypeAlias(a) => {
                check_symbol(a.name)?;
                for tp in &a.type_params {
                    check_def(tp.def_id)?;
                    check_symbol(tp.name)?;
                    if let radix_hir::HirTypeParamConstraint::OneOf(tys) = &tp.constraint {
                        for ty in tys {
                            check_type(*ty)?;
                        }
                    }
                    validate_type_param_default(&tp.default, check_type)?;
                }
                check_type(a.ty)?;
            }
            HirItemKind::Schema(schema) => {
                check_symbol(schema.name)?;
                for col in &schema.columns {
                    check_symbol(col.name)?;
                    check_symbol(col.source)?;
                    check_type(col.ty)?;
                }
            }
            HirItemKind::Constant(c) => {
                check_symbol(c.name)?;
                if let Some(ty) = c.ty {
                    check_type(ty)?;
                }
                validate_expr(&c.value, check_symbol, check_def, check_hir, check_type)?;
            }
            HirItemKind::Import(imp) => {
                check_symbol(imp.path)?;
                for item in &imp.items {
                    check_def(item.def_id)?;
                    check_symbol(item.name)?;
                    if let Some(a) = item.alias {
                        check_symbol(a)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_block(
        block: &radix_hir::HirBlock,
        check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
        check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
        check_hir: &dyn Fn(radix_hir::HirId) -> Result<(), HirArtifactError>,
        check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
    ) -> Result<(), HirArtifactError> {
        for stmt in &block.statements {
            check_hir(stmt.id)?;
            match &stmt.kind {
                HirStatementKind::Local(local) => {
                    check_def(local.def_id)?;
                    check_symbol(local.name)?;
                    if let Some(ty) = local.ty {
                        check_type(ty)?;
                    }
                    if let Some(ref init) = local.init {
                        validate_expr(init, check_symbol, check_def, check_hir, check_type)?;
                    }
                }
                HirStatementKind::Expr(expr) => {
                    validate_expr(expr, check_symbol, check_def, check_hir, check_type)?;
                }
                HirStatementKind::Redde(Some(expr)) => {
                    validate_expr(expr, check_symbol, check_def, check_hir, check_type)?;
                }
                HirStatementKind::IncDec(inc) => {
                    validate_expr(&inc.target, check_symbol, check_def, check_hir, check_type)?;
                }
                HirStatementKind::Custodi(cust) => {
                    for clause in &cust.clauses {
                        validate_expr(
                            &clause.cond,
                            check_symbol,
                            check_def,
                            check_hir,
                            check_type,
                        )?;
                        validate_block(
                            &clause.body,
                            check_symbol,
                            check_def,
                            check_hir,
                            check_type,
                        )?;
                    }
                }
                HirStatementKind::Rumpe
                | HirStatementKind::Perge
                | HirStatementKind::Tacet
                | HirStatementKind::Redde(None) => {}
            }
        }
        if let Some(ref expr) = block.expr {
            validate_expr(expr, check_symbol, check_def, check_hir, check_type)?;
        }
        Ok(())
    }

    fn validate_expr(
        expr: &radix_hir::HirExpression,
        check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
        check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
        check_hir: &dyn Fn(radix_hir::HirId) -> Result<(), HirArtifactError>,
        check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
    ) -> Result<(), HirArtifactError> {
        check_hir(expr.id)?;
        if let Some(ty) = expr.ty {
            check_type(ty)?;
        }
        match &expr.kind {
            HirExpressionKind::Summa {
                source,
                coordinates,
                filum,
                binding,
                body,
            } => {
                validate_expr(source, check_symbol, check_def, check_hir, check_type)?;
                for (def_id, name) in coordinates {
                    check_def(*def_id)?;
                    check_symbol(*name)?;
                }
                if let Some((def_id, name)) = filum {
                    check_def(*def_id)?;
                    check_symbol(*name)?;
                }
                check_def(binding.0)?;
                check_symbol(binding.1)?;
                validate_block(body, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Extremum {
                source,
                coordinates,
                coalesce,
                ..
            } => {
                validate_expr(source, check_symbol, check_def, check_hir, check_type)?;
                for (def_id, name) in coordinates {
                    check_def(*def_id)?;
                    check_symbol(*name)?;
                }
                if let Some(identity) = coalesce {
                    validate_expr(identity, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::FirstMatch {
                source,
                coordinates,
                binding,
                body,
            } => {
                validate_expr(source, check_symbol, check_def, check_hir, check_type)?;
                for (def_id, name) in coordinates {
                    check_def(*def_id)?;
                    check_symbol(*name)?;
                }
                check_def(binding.0)?;
                check_symbol(binding.1)?;
                validate_block(body, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Path(def_id) => check_def(*def_id)?,
            HirExpressionKind::Literal(lit) => match lit {
                radix_hir::HirLiteral::String(s)
                | radix_hir::HirLiteral::Ascii(s)
                | radix_hir::HirLiteral::Octeti(s) => check_symbol(*s)?,
                radix_hir::HirLiteral::Regex(s, Some(f)) => {
                    check_symbol(*s)?;
                    check_symbol(*f)?;
                }
                radix_hir::HirLiteral::Regex(s, None) => check_symbol(*s)?,
                _ => {}
            },
            HirExpressionKind::Binary(_, lhs, rhs) => {
                validate_expr(lhs, check_symbol, check_def, check_hir, check_type)?;
                validate_expr(rhs, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Unary(_, inner) => {
                validate_expr(inner, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Call(callee, type_args, args, _) => {
                validate_expr(callee, check_symbol, check_def, check_hir, check_type)?;
                for ty in type_args {
                    check_type(*ty)?;
                }
                for arg in args {
                    if let Some(n) = arg.name {
                        check_symbol(n)?;
                    }
                    validate_expr(&arg.expr, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::Transfer { sink, payloads } => {
                validate_expr(sink, check_symbol, check_def, check_hir, check_type)?;
                for payload in payloads {
                    validate_expr(payload, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::Gradient {
                call,
                selected_arguments,
            } => {
                validate_expr(call, check_symbol, check_def, check_hir, check_type)?;
                for selector in selected_arguments {
                    validate_expr(selector, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::MethodCall(receiver, method, type_args, args, _) => {
                validate_expr(receiver, check_symbol, check_def, check_hir, check_type)?;
                check_symbol(*method)?;
                for ty in type_args {
                    check_type(*ty)?;
                }
                for arg in args {
                    if let Some(n) = arg.name {
                        check_symbol(n)?;
                    }
                    validate_expr(&arg.expr, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::Field(target, field) => {
                validate_expr(target, check_symbol, check_def, check_hir, check_type)?;
                check_symbol(*field)?;
            }
            HirExpressionKind::Index(target, index) => {
                validate_expr(target, check_symbol, check_def, check_hir, check_type)?;
                validate_expr(index, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::OptionalChain(base, chain) => {
                validate_expr(base, check_symbol, check_def, check_hir, check_type)?;
                match chain {
                    radix_hir::HirOptionalChainKind::Member(s) => check_symbol(*s)?,
                    radix_hir::HirOptionalChainKind::Index(e) => {
                        validate_expr(e, check_symbol, check_def, check_hir, check_type)?;
                    }
                    radix_hir::HirOptionalChainKind::Call(args) => {
                        for arg in args {
                            if let Some(n) = arg.name {
                                check_symbol(n)?;
                            }
                            validate_expr(
                                &arg.expr,
                                check_symbol,
                                check_def,
                                check_hir,
                                check_type,
                            )?;
                        }
                    }
                }
            }
            HirExpressionKind::NonNull(base, chain) => {
                validate_expr(base, check_symbol, check_def, check_hir, check_type)?;
                match chain {
                    radix_hir::HirNonNullKind::Member(s) => check_symbol(*s)?,
                    radix_hir::HirNonNullKind::Index(e) => {
                        validate_expr(e, check_symbol, check_def, check_hir, check_type)?;
                    }
                    radix_hir::HirNonNullKind::Call(args) => {
                        for arg in args {
                            if let Some(n) = arg.name {
                                check_symbol(n)?;
                            }
                            validate_expr(
                                &arg.expr,
                                check_symbol,
                                check_def,
                                check_hir,
                                check_type,
                            )?;
                        }
                    }
                }
            }
            HirExpressionKind::Block(block) => {
                validate_block(block, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => {
                validate_expr(cond, check_symbol, check_def, check_hir, check_type)?;
                validate_block(then_block, check_symbol, check_def, check_hir, check_type)?;
                if let Some(cape) = then_catch {
                    check_def(cape.binding_def_id)?;
                    check_symbol(cape.binding_name)?;
                    if let Some(ty) = cape.binding_ty {
                        check_type(ty)?;
                    }
                    validate_block(&cape.body, check_symbol, check_def, check_hir, check_type)?;
                }
                if let Some(else_block) = else_block {
                    validate_block(else_block, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => {
                for s in scrutinees {
                    validate_expr(s, check_symbol, check_def, check_hir, check_type)?;
                }
                for arm in arms {
                    for pat in &arm.patterns {
                        validate_pattern(pat, check_symbol, check_def)?;
                    }
                    if let Some(ref guard) = arm.guard {
                        validate_expr(guard, check_symbol, check_def, check_hir, check_type)?;
                    }
                    validate_expr(&arm.body, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::Loop(body) => {
                validate_block(body, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Dum(cond, body) => {
                validate_expr(cond, check_symbol, check_def, check_hir, check_type)?;
                validate_block(body, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Itera(_, coordinates, def_id, sym, iterable, body) => {
                for (coordinate_def_id, coordinate_name) in coordinates {
                    check_def(*coordinate_def_id)?;
                    check_symbol(*coordinate_name)?;
                }
                check_def(*def_id)?;
                check_symbol(*sym)?;
                validate_expr(iterable, check_symbol, check_def, check_hir, check_type)?;
                validate_block(body, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::IteraLockstep {
                sources,
                coordinates,
                bindings,
                body,
            } => {
                for (coordinate_def_id, coordinate_name) in coordinates {
                    check_def(*coordinate_def_id)?;
                    check_symbol(*coordinate_name)?;
                }
                for (binding_def_id, binding_name) in bindings {
                    check_def(*binding_def_id)?;
                    check_symbol(*binding_name)?;
                }
                for source in sources {
                    validate_expr(source, check_symbol, check_def, check_hir, check_type)?;
                }
                validate_block(body, check_symbol, check_def, check_hir, check_type)?;
            }
            // NR-5 (nary-range-walk): validate product binder defs/symbols
            // and ranges/body; dangling binder indices fail closed through
            // the shared DanglingIndex path.
            HirExpressionKind::IteraRange {
                ranges,
                bindings,
                body,
            } => {
                for (binding_def_id, binding_name) in bindings {
                    check_def(*binding_def_id)?;
                    check_symbol(*binding_name)?;
                }
                for range in ranges {
                    validate_expr(range, check_symbol, check_def, check_hir, check_type)?;
                }
                validate_block(body, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Intervallum {
                start, end, step, ..
            } => {
                validate_expr(start, check_symbol, check_def, check_hir, check_type)?;
                validate_expr(end, check_symbol, check_def, check_hir, check_type)?;
                if let Some(s) = step {
                    validate_expr(s, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::Assign(lhs, rhs) => {
                validate_expr(lhs, check_symbol, check_def, check_hir, check_type)?;
                validate_expr(rhs, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
            } => {
                validate_expr(target, check_symbol, check_def, check_hir, check_type)?;
                validate_expr(source, check_symbol, check_def, check_hir, check_type)?;
                if let Some(recovery) = recovery {
                    validate_expr(recovery, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::Array(elements) => {
                for elem in elements {
                    match elem {
                        radix_hir::HirArrayElement::Expr(e)
                        | radix_hir::HirArrayElement::Spread(e) => {
                            validate_expr(e, check_symbol, check_def, check_hir, check_type)?;
                        }
                    }
                }
            }
            HirExpressionKind::Struct(def_id, fields) => {
                check_def(*def_id)?;
                for (sym, e) in fields {
                    check_symbol(*sym)?;
                    validate_expr(e, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::Tuple(elements, type_args) => {
                for e in elements {
                    validate_expr(e, check_symbol, check_def, check_hir, check_type)?;
                }
                if let Some(tas) = type_args {
                    for ty in tas {
                        check_type(*ty)?;
                    }
                }
            }
            HirExpressionKind::Scribe(_, args) => {
                for a in args {
                    validate_expr(a, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::Scriptum(template, args) => {
                check_symbol(*template)?;
                for a in args {
                    validate_expr(a, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::Adfirma(expr, msg) => {
                validate_expr(expr, check_symbol, check_def, check_hir, check_type)?;
                if let Some(m) = msg {
                    validate_expr(m, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::Panic(expr) | HirExpressionKind::Throw(expr) => {
                validate_expr(expr, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Handled { body, catch } => {
                validate_block(body, check_symbol, check_def, check_hir, check_type)?;
                check_def(catch.binding_def_id)?;
                check_symbol(catch.binding_name)?;
                if let Some(ty) = catch.binding_ty {
                    check_type(ty)?;
                }
                validate_block(&catch.body, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Trap(body) => {
                validate_block(body, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Clausura(params, ret_ty, err_ty, body, _) => {
                for param in params {
                    check_def(param.def_id)?;
                    check_symbol(param.name)?;
                    check_type(param.ty)?;
                    if let Some(ref init) = param.default {
                        validate_expr(init, check_symbol, check_def, check_hir, check_type)?;
                    }
                }
                if let Some(ty) = ret_ty {
                    check_type(*ty)?;
                }
                if let Some(ty) = err_ty {
                    check_type(*ty)?;
                }
                validate_expr(body, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Cede(inner)
            | HirExpressionKind::Reddet(inner)
            | HirExpressionKind::Tacebit(inner)
            | HirExpressionKind::Yield(inner) => {
                validate_expr(inner, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Verte {
                source,
                target,
                entries,
            } => {
                validate_expr(source, check_symbol, check_def, check_hir, check_type)?;
                check_type(*target)?;
                if let Some(entries) = entries {
                    for entry in entries {
                        match &entry.key {
                            radix_hir::HirObjectKey::Ident(s)
                            | radix_hir::HirObjectKey::String(s) => check_symbol(*s)?,
                            radix_hir::HirObjectKey::Computed(e)
                            | radix_hir::HirObjectKey::Spread(e) => {
                                validate_expr(e, check_symbol, check_def, check_hir, check_type)?;
                            }
                        }
                        if let Some(ref v) = entry.value {
                            validate_expr(v, check_symbol, check_def, check_hir, check_type)?;
                        }
                    }
                }
            }
            HirExpressionKind::Conversio {
                source,
                target,
                params,
                recovery,
            } => {
                validate_expr(source, check_symbol, check_def, check_hir, check_type)?;
                match target {
                    radix_hir::HirConversioTarget::Type(ty) => check_type(*ty)?,
                    radix_hir::HirConversioTarget::Intervallum(e) => {
                        validate_expr(e, check_symbol, check_def, check_hir, check_type)?;
                    }
                }
                for sym in params {
                    check_symbol(*sym)?;
                }
                if let Some(r) = recovery {
                    validate_expr(r, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::Ad { route, opener } => {
                check_symbol(*route)?;
                if let Some(o) = opener {
                    validate_expr(o, check_symbol, check_def, check_hir, check_type)?;
                }
            }
            HirExpressionKind::Ref(_, inner) | HirExpressionKind::Deref(inner) => {
                validate_expr(inner, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::TypeCheck { expr, ty, .. } => {
                validate_expr(expr, check_symbol, check_def, check_hir, check_type)?;
                check_type(*ty)?;
            }
            HirExpressionKind::Praefixum(inner) => {
                validate_expr(inner, check_symbol, check_def, check_hir, check_type)?;
            }
            HirExpressionKind::Vacua
            | HirExpressionKind::ReadLine(_)
            | HirExpressionKind::Error => {}
        }
        Ok(())
    }

    fn validate_pattern(
        pat: &radix_hir::HirPattern,
        check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
        check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
    ) -> Result<(), HirArtifactError> {
        match pat {
            radix_hir::HirPattern::Binding(def_id, sym) => {
                check_def(*def_id)?;
                check_symbol(*sym)?;
            }
            radix_hir::HirPattern::Alias(def_id, sym, inner) => {
                check_def(*def_id)?;
                check_symbol(*sym)?;
                validate_pattern(inner, check_symbol, check_def)?;
            }
            radix_hir::HirPattern::Variant(def_id, subpatterns) => {
                check_def(*def_id)?;
                for sp in subpatterns {
                    validate_pattern(sp, check_symbol, check_def)?;
                }
            }
            radix_hir::HirPattern::Literal(lit) => match lit {
                radix_hir::HirLiteral::String(s)
                | radix_hir::HirLiteral::Ascii(s)
                | radix_hir::HirLiteral::Octeti(s) => check_symbol(*s)?,
                radix_hir::HirLiteral::Regex(s, Some(f)) => {
                    check_symbol(*s)?;
                    check_symbol(*f)?;
                }
                radix_hir::HirLiteral::Regex(s, None) => check_symbol(*s)?,
                _ => {}
            },
            radix_hir::HirPattern::TypeTest { binding, .. } => {
                if let Some((def_id, sym)) = binding {
                    check_def(*def_id)?;
                    check_symbol(*sym)?;
                }
            }
            radix_hir::HirPattern::Wildcard => {}
        }
        Ok(())
    }

    fn validate_annotation(
        ann: &radix_hir::HirAnnotation,
        check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
        check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
        check_hir: &dyn Fn(radix_hir::HirId) -> Result<(), HirArtifactError>,
        check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
    ) -> Result<(), HirArtifactError> {
        check_symbol(ann.family)?;
        if let Some(def_id) = ann.contract_id {
            check_def(def_id)?;
        }
        for field in &ann.fields {
            check_symbol(field.name)?;
            match &field.value {
                radix_hir::HirAnnotationValue::Expr(e) => {
                    validate_expr(e, check_symbol, check_def, check_hir, check_type)?;
                }
                radix_hir::HirAnnotationValue::Type(ty) => check_type(*ty)?,
                radix_hir::HirAnnotationValue::Symbol(s)
                | radix_hir::HirAnnotationValue::String(s) => check_symbol(*s)?,
                _ => {}
            }
        }
        Ok(())
    }

    // ── Presentation sidecar walk helper (schema v2/v9) ─────────────────
    //
    // Presentation anchors are a dedicated namespace (HirSourceAnchorId),
    // never overloaded with HirId. The sidecar must be internally coherent:
    // every attachment names a registered owner, owner anchors are unique,
    // each owner carries at most one attachment, every stored comment
    // Symbol resolves in the interner, and every semantic back-reference
    // (HirOwnerRef) names a node that exists in the HIR tree or def
    // registry. Dangling or duplicated anchors and dangling comment
    // symbols fail closed (CSO E8, F7).

    fn validate_presentation(
        artifact: &HirArtifact,
        presentation: &radix_hir::HirPresentation,
        check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
        check_hir: &dyn Fn(radix_hir::HirId) -> Result<(), HirArtifactError>,
        check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
        check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
        check_index: &dyn Fn(radix_types::IndexId) -> Result<(), HirArtifactError>,
    ) -> Result<(), HirArtifactError> {
        // Owner anchor ids must be unique. Each owner's semantic back-ref,
        // when present, must resolve to an in-tree node (HirId) or a known
        // definition (DefId); a dangling back-ref is a sidecar corruption.
        let mut owner_ids: FxHashSet<u32> = FxHashSet::default();
        for owner in &presentation.owners {
            if !owner_ids.insert(owner.id.0) {
                return Err(HirArtifactError::InvalidPresentation {
                    issue: "duplicate_owner_anchor",
                    anchor: owner.id.0,
                });
            }
            match &owner.semantic {
                Some(radix_hir::HirOwnerRef::Hir(id)) => check_hir(*id)?,
                Some(radix_hir::HirOwnerRef::Def(def_id)) => check_def(*def_id)?,
                None => {}
            }
        }

        // Every attachment anchor must name a registered owner, each owner
        // may carry at most one attachment, and comment Symbols must resolve
        // in the interner table.
        let mut attached_anchors: FxHashSet<u32> = FxHashSet::default();
        for attachment in &presentation.attachments {
            if !owner_ids.contains(&attachment.anchor.0) {
                return Err(HirArtifactError::DanglingIndex {
                    kind: "HirSourceAnchorId",
                    index: attachment.anchor.0,
                    arena_size: owner_ids.len(),
                });
            }
            if !attached_anchors.insert(attachment.anchor.0) {
                return Err(HirArtifactError::InvalidPresentation {
                    issue: "duplicate_attachment_owner",
                    anchor: attachment.anchor.0,
                });
            }
            for trivia in &attachment.leading {
                if let radix_hir::HirTrivia::CommentLine { text, .. } = trivia {
                    check_symbol(*text)?;
                }
            }
        }

        // The entry-start anchor, when present, must name a registered owner.
        if let Some(anchor) = presentation.entry_start_anchor.as_ref()
            && !owner_ids.contains(&anchor.0)
        {
            return Err(HirArtifactError::DanglingIndex {
                kind: "HirSourceAnchorId",
                index: anchor.0,
                arena_size: owner_ids.len(),
            });
        }

        // Structural ends (block ends + program end) must be anchor-consistent
        // with the owners registry. Lowering allocates every anchor from one
        // arena, so end ids are unique and never shared with a non-end owner;
        // an end that carries trivia is registered under its own kind, and an
        // owner claiming a structural-end kind must be backed by the matching
        // end record. Trivia-free ends legitimately have no owner record
        // (owners are registered only when trivia exists), so an end id absent
        // from the registry is legal.
        let mut end_ids: FxHashSet<u32> = FxHashSet::default();
        for end in &presentation.block_ends {
            if !end_ids.insert(end.id.0) {
                return Err(HirArtifactError::InvalidPresentation {
                    issue: "duplicate_block_end_anchor",
                    anchor: end.id.0,
                });
            }
        }
        if !end_ids.insert(presentation.program_end.id.0) {
            return Err(HirArtifactError::InvalidPresentation {
                issue: "program_end_collides_with_block_end",
                anchor: presentation.program_end.id.0,
            });
        }
        for owner in &presentation.owners {
            match owner.kind {
                radix_hir::HirTriviaOwnerKind::BlockEnd => {
                    if !end_ids.contains(&owner.id.0) {
                        return Err(HirArtifactError::InvalidPresentation {
                            issue: "block_end_owner_without_end",
                            anchor: owner.id.0,
                        });
                    }
                }
                radix_hir::HirTriviaOwnerKind::ProgramEnd => {
                    if owner.id != presentation.program_end.id {
                        return Err(HirArtifactError::InvalidPresentation {
                            issue: "program_end_owner_mismatch",
                            anchor: owner.id.0,
                        });
                    }
                }
                _ => {
                    if end_ids.contains(&owner.id.0) {
                        return Err(HirArtifactError::InvalidPresentation {
                            issue: "end_anchor_reused_as_owner",
                            anchor: owner.id.0,
                        });
                    }
                }
            }
        }

        validate_authored_type_records(
            artifact,
            presentation,
            check_symbol,
            check_hir,
            check_def,
            check_type,
            check_index,
        )?;

        Ok(())
    }

    // ── Type arena walk helper ──────────────────────────────────────────

    fn validate_type(
        ty: &radix_types::Type,
        check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
        check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
        check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
        check_index: &dyn Fn(radix_types::IndexId) -> Result<(), HirArtifactError>,
    ) -> Result<(), HirArtifactError> {
        match ty {
            radix_types::Type::Primitive(_)
            | radix_types::Type::SizedNumeric(_, _)
            | radix_types::Type::ModularWord(_)
            | radix_types::Type::SizedInstans(_)
            | radix_types::Type::Infer(_)
            | radix_types::Type::InferUnion(_)
            | radix_types::Type::MarkerHole { .. }
            | radix_types::Type::Error => {}
            radix_types::Type::Array(t)
            | radix_types::Type::Set(t)
            | radix_types::Type::Promissum(t)
            | radix_types::Type::Cursor(t)
            | radix_types::Type::Queue(t)
            | radix_types::Type::Stack(t)
            | radix_types::Type::Atomic(t)
            | radix_types::Type::Intervallum(t)
            | radix_types::Type::Option(t) => {
                check_type(*t)?;
            }
            radix_types::Type::PromissumFailable(t, e) => {
                check_type(*t)?;
                check_type(*e)?;
            }
            radix_types::Type::AsyncCursor(t, e) => {
                check_type(*t)?;
                check_type(*e)?;
            }
            radix_types::Type::Map(k, v) => {
                check_type(*k)?;
                check_type(*v)?;
            }
            radix_types::Type::Record(fields) => {
                for (&sym, &tid) in fields {
                    check_symbol(sym)?;
                    check_type(tid)?;
                }
            }
            radix_types::Type::Tensor(t, i)
            | radix_types::Type::Vector(t, i)
            | radix_types::Type::Matrix(t, i)
            | radix_types::Type::Sparsa(t, i)
            | radix_types::Type::BoundedArray {
                element: t,
                capacity: i,
            }
            | radix_types::Type::BoundedQueue {
                element: t,
                capacity: i,
            }
            | radix_types::Type::BoundedStack {
                element: t,
                capacity: i,
            } => {
                check_type(*t)?;
                check_index(*i)?;
            }
            radix_types::Type::BoundedTextus { capacity }
            | radix_types::Type::BoundedAscii { capacity }
            | radix_types::Type::BoundedOcteti { capacity } => {
                check_index(*capacity)?;
            }
            radix_types::Type::Ref(_, t) => {
                check_type(*t)?;
            }
            radix_types::Type::Struct(d)
            | radix_types::Type::Enum(d)
            | radix_types::Type::Interface(d) => {
                check_def(*d)?;
            }
            radix_types::Type::Alias(d, t) => {
                check_def(*d)?;
                check_type(*t)?;
            }
            radix_types::Type::Func(sig) => {
                check_type(sig.ret)?;
                if let Some(et) = sig.err {
                    check_type(et)?;
                }
                for param in &sig.type_params {
                    if let radix_types::TypeParamConstraint::OneOf(tys) = &param.constraint {
                        for ty in tys {
                            check_type(*ty)?;
                        }
                    }
                    if let Some(radix_types::SemanticGenericParamDefault::Typus(ty)) = param.default
                    {
                        check_type(ty)?;
                    }
                    check_symbol(param.name)?;
                }
                for pt in &sig.params {
                    check_type(pt.ty)?;
                }
            }
            radix_types::Type::Param(name) => {
                check_symbol(*name)?;
            }
            radix_types::Type::Applied(base, args) => {
                check_type(*base)?;
                for arg in args {
                    match arg {
                        radix_types::AppliedArg::Type(id) => check_type(*id)?,
                        radix_types::AppliedArg::Size(id) => check_index(*id)?,
                    }
                }
            }
            radix_types::Type::Union(members)
            | radix_types::Type::Intersection(members)
            | radix_types::Type::Tuple(members) => {
                for m in members {
                    check_type(*m)?;
                }
            }
        }
        Ok(())
    }

    // ── Resolver snapshot walk helper ───────────────────────────────────

    fn validate_file_interface_snapshot(
        iface: &crate::artifact::FileInterfaceSnapshot,
        check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
        check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
        check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
    ) -> Result<(), HirArtifactError> {
        use crate::artifact::FileExportSnapshot;

        for (sym, export) in &iface.exports {
            check_symbol(*sym)?;
            match export {
                FileExportSnapshot::Function(sig) => {
                    check_type(sig.ret)?;
                    if let Some(et) = sig.err {
                        check_type(et)?;
                    }
                    for param in &sig.type_params {
                        if let radix_types::TypeParamConstraint::OneOf(tys) = &param.constraint {
                            for ty in tys {
                                check_type(*ty)?;
                            }
                        }
                        if let Some(radix_types::SemanticGenericParamDefault::Typus(ty)) =
                            param.default
                        {
                            check_type(ty)?;
                        }
                    }
                    for pt in &sig.params {
                        check_type(pt.ty)?;
                    }
                }
                FileExportSnapshot::Type { ty, def_id } => {
                    check_type(*ty)?;
                    if let Some(d) = def_id {
                        check_def(*d)?;
                    }
                }
                FileExportSnapshot::Struct { def_id, ty, fields } => {
                    check_def(*def_id)?;
                    check_type(*ty)?;
                    for field in fields {
                        check_symbol(field.name)?;
                        check_type(field.ty)?;
                    }
                }
            }
        }
        Ok(())
    }

    // ── Top-level validation ───────────────────────────────────────────

    // 1. HirModule items and entry.
    validate_items(
        &artifact.hir.items,
        &check_symbol,
        &check_def,
        &check_hir,
        &check_type,
    )?;
    if let Some(ref entry) = artifact.hir.entry {
        validate_block(entry, &check_symbol, &check_def, &check_hir, &check_type)?;
    }
    for ann in &artifact.hir.entry_annotations {
        validate_annotation(ann, &check_symbol, &check_def, &check_hir, &check_type)?;
    }
    if let Some(sym) = artifact.hir.entry_args_name {
        check_symbol(sym)?;
    }

    // 2. gpu_builtins: DefId keys.
    for (def_id, _) in &artifact.gpu_builtins {
        check_def(*def_id)?;
    }

    // 3. function_facts: DefId keys and internal index-bearing fields.
    for (def_id, fact) in &artifact.function_facts.entries {
        check_def(*def_id)?;
        for edge in &fact.call_edges {
            check_def(*edge)?;
        }
        for capture in &fact.captures {
            check_def(capture.def_id)?;
        }
        for (pd, _mode) in &fact.param_modes {
            check_def(*pd)?;
        }
        if let Some(ty) = fact.err_ty {
            check_type(ty)?;
        }
    }
    if let Some(ref entry_fact) = artifact.function_facts.entry {
        for edge in &entry_fact.call_edges {
            check_def(*edge)?;
        }
        for capture in &entry_fact.captures {
            check_def(capture.def_id)?;
        }
        for (pd, _mode) in &entry_fact.param_modes {
            check_def(*pd)?;
        }
        if let Some(ty) = entry_fact.err_ty {
            check_type(ty)?;
        }
    }

    // 4. resolved_uses: DefId keys (Local) and enclosing DefIds.
    for use_wire in &artifact.resolved_uses {
        if let crate::artifact::ResolvedUseKeyWire::Local(def_id) = use_wire.key {
            check_def(def_id)?;
        }
        if let Some(enclosing) = use_wire.enclosing {
            check_def(enclosing)?;
        }
    }

    // 5. libraries: deep walk.
    for def_id in artifact.libraries.bindings.keys() {
        check_def(*def_id)?;
    }
    for binding in artifact.libraries.bindings.values() {
        check_def(binding.local_def_id)?;
    }
    for def_id in artifact.libraries.items.keys() {
        check_def(*def_id)?;
    }
    for item in artifact.libraries.items.values() {
        check_def(item.def_id)?;
    }
    for def_id in artifact.libraries.exports.keys() {
        check_def(*def_id)?;
    }
    for (pair, _) in artifact.libraries.reexports.keys() {
        check_def(*pair)?;
    }

    // 6. cli_program: resolved-use-like fields only (no Symbol-bearing fields in CliProgramWire).

    // 7. resolver snapshot: deep walk. E3 conversion rows are validated
    // here rather than silently dropped at the artifact boundary.
    for (sym, _) in &artifact.resolver.namespace_exports {
        check_symbol(*sym)?;
    }
    for (sym, iface) in &artifact.resolver.file_interfaces {
        check_symbol(*sym)?;
        validate_file_interface_snapshot(iface, &check_symbol, &check_def, &check_type)?;
    }
    for sym in &artifact.resolver.used_namespaces {
        check_symbol(*sym)?;
    }
    for ((_, sym), ty) in &artifact.resolver.imported_nominal_types {
        check_symbol(*sym)?;
        check_type(*ty)?;
    }
    for row in &artifact.resolver.conversion_rows {
        if let Some(fn_def_id) = row.fn_def_id {
            check_def(fn_def_id)?;
        }
        check_type(row.source)?;
        check_type(row.destination)?;
    }

    // 8. Type arena walk: validate TypeId/DefId/Symbol/IndexId
    //    cross-references inside the type-table snapshot.
    let check_index = |xi: radix_types::IndexId| -> Result<(), HirArtifactError> {
        let idx_count = artifact.types.indices().len();
        let i = xi.0 as usize;
        if i >= idx_count {
            return Err(HirArtifactError::DanglingIndex {
                kind: "IndexId",
                index: xi.0,
                arena_size: idx_count,
            });
        }
        Ok(())
    };
    for ty in artifact.types.types() {
        validate_type(ty, &check_def, &check_symbol, &check_type, &check_index)?;
    }
    for index in artifact.types.indices() {
        if let radix_types::IndexExpr::Tuple(children) = index {
            for child in children {
                check_index(*child)?;
            }
        }
        if let radix_types::IndexExpr::Param(symbol) = index {
            check_symbol(*symbol)?;
        }
    }

    // 9. Presentation sidecar (anchors from v2, authored types from v9): every
    //    attachment anchor must name a registered owner, owner anchors must be
    //    unique, each owner may
    //    carry at most one attachment, every comment Symbol must resolve
    //    in the interner table, and every HirOwnerRef back-reference must
    //    name an in-tree node or known def. The entry-start anchor, when
    //    present, must name a registered owner.
    validate_presentation(
        artifact,
        &artifact.presentation,
        &check_symbol,
        &check_hir,
        &check_def,
        &check_type,
        &check_index,
    )?;

    // 10. Capability provenance (SCR-04, schema v8): every identity key —
    //     function rows, provenance/relationship DefIds, and last-use
    //     borrower/site pairs — must resolve against the HIR arenas.
    for row in &artifact.capability_provenance.functions {
        check_def(row.function)?;
        match row.returned_borrow {
            crate::artifact::ReturnedBorrowWire::None => {}
            crate::artifact::ReturnedBorrowWire::Parameter(def_id)
            | crate::artifact::ReturnedBorrowWire::Definition(def_id) => check_def(def_id)?,
        }
        for relationship in &row.required_regions {
            check_def(relationship.from)?;
            check_def(relationship.to)?;
        }
        for last_use in &row.last_uses {
            check_def(last_use.borrower)?;
            check_hir(last_use.site)?;
        }
    }

    // 11. Resolved generic bindings (schema v10): each binding name and
    //     kind-tagged witness must resolve against the interner and arenas.
    for row in &artifact.resolved_generic_calls {
        for binding in &row.bindings {
            check_symbol(binding.name)?;
            match binding.witness {
                radix_types::GenericWitness::Typus(ty) => check_type(ty)?,
                radix_types::GenericWitness::Magnitudo(xi) => check_index(xi)?,
            }
        }
    }

    // 12. Compiler lane metadata (schema v11): a rehydrated AIR unit must
    // reference only declarations and symbols present in the same artifact.
    for entry in &artifact.radix_lanes {
        check_def(entry.def_id)?;
        if let Some(backward) = entry.backward {
            check_symbol(backward.companion_name)?;
            check_def(backward.companion_def_id)?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "validate_test.rs"]
mod tests;
