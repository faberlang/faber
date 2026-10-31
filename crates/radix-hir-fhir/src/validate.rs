//! Fail-closed referential-integrity validation for the FHIR artifact.
//!
//! Walks every arena-backed index in the artifact — `Symbol`, `DefId`,
//! `HirId`, `TypeId`, `IndexId`, presentation anchors — and rejects any
//! reference that does not resolve before codegen can touch it (U2.3, CSO E8).

use crate::artifact::HirArtifact;
use crate::error::HirArtifactError;
use handroll::{FxHashMap, FxHashSet};

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

// Slot ordinals count HIR children (params, fields, annotations); a live HIR
// node never approaches u32::MAX slots, so the usize->u32 casts cannot truncate.
#[allow(clippy::cast_possible_truncation)]
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

#[allow(clippy::cast_possible_truncation)]
fn register_annotation_type_sites(
    sites: &mut FxHashMap<radix_hir::HirTypeSite, radix_types::TypeId>,
    owner: &radix_hir::HirOwnerRef,
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
    inferred_constructor_ids: FxHashSet<radix_hir::HirId>,
}

impl TypeSiteCollector {
    fn new() -> Self {
        Self {
            sites: FxHashMap::default(),
            current_expr: None,
            inferred_constructor_ids: FxHashSet::default(),
        }
    }
}

impl radix_hir::visit::HirVisitor for TypeSiteCollector {
    // Slot ordinals count HIR children; never near u32::MAX in a live node.
    #[allow(clippy::cast_possible_truncation)]
    fn visit_item(&mut self, item: &radix_hir::HirItem) {
        let owner = radix_hir::HirOwnerRef::Def(item.def_id);
        register_annotation_type_sites(&mut self.sites, &owner, &item.annotations);
        match &item.kind {
            radix_hir::HirItemKind::Function(function) => {
                register_function_type_sites(&mut self.sites, item.def_id, function);
            }
            radix_hir::HirItemKind::Struct(strukt) => {
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
                        &radix_hir::HirOwnerRef::Def(field.def_id),
                        &field.annotations,
                    );
                }
            }
            radix_hir::HirItemKind::Enum(enum_item) => {
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
            &radix_hir::HirOwnerRef::Def(method.def_id),
            &method.annotations,
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

    // Slot ordinals count HIR children; never near u32::MAX in a live node.
    #[allow(clippy::cast_possible_truncation)]
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
                register_type_site(
                    &mut self.sites,
                    radix_hir::HirOwnerRef::Hir(expr.id),
                    radix_hir::HirTypeSlot::TypedZero,
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
            } => {
                register_type_site(
                    &mut self.sites,
                    radix_hir::HirOwnerRef::Hir(expr.id),
                    radix_hir::HirTypeSlot::ConversionTarget,
                    *ty,
                );
                // An inferred constructor rewritten to a set-cell conversion
                // keeps the literal's node id and the lowering's authored `_`
                // record at the ascription slot. Register that site so the
                // presentation record is recognized here the same way the
                // serialize registry keeps it on write.
                if self.inferred_constructor_ids.contains(&expr.id) {
                    register_type_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Hir(expr.id),
                        radix_hir::HirTypeSlot::AscriptionTarget,
                        *ty,
                    );
                }
            }
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
            radix_hir::HirExpressionKind::Clausura(params, ret, err, _) => {
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
            radix_hir::HirExpressionKind::MethodCall(receiver, ..)
                if matches!(receiver.kind, radix_hir::HirExpressionKind::Vacua) =>
            {
                // A tensor typed zero `Type {}` lowers to a `crea` call on a
                // `vacua` receiver pre-typed with the claimed tensor type.
                if let Some(ty) = receiver.ty {
                    register_type_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Hir(expr.id),
                        radix_hir::HirTypeSlot::TypedZero,
                        ty,
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
    collector.inferred_constructor_ids.extend(
        artifact
            .presentation
            .inferred_constructor_ids
            .iter()
            .copied(),
    );
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

/// Every typus parameter each type-alias item declares, keyed by the item's
/// `DefId`.
///
/// A generic alias body is parametric in its declared typus parameters only:
/// `magnitudo` size parameters are compile-time facts that never appear as a
/// `Type::Param` in the recorded body, so collecting them would invite
/// bindings no structural match can consume.
fn collect_generic_alias_params(
    artifact: &HirArtifact,
) -> FxHashMap<radix_types::DefId, Vec<radix_lexer::Symbol>> {
    let mut params = FxHashMap::default();
    for item in &artifact.hir.items {
        if let radix_hir::HirItemKind::TypeAlias(alias) = &item.kind {
            params.insert(
                item.def_id,
                alias
                    .type_params
                    .iter()
                    .filter(|param| param.is_typus())
                    .map(|param| param.name)
                    .collect(),
            );
        }
    }
    params
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
    alias_params: &FxHashMap<radix_types::DefId, Vec<radix_lexer::Symbol>>,
    conversation: Option<radix_types::DefId>,
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
        alias_params,
        conversation,
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

/// Whether `element` is the u8 byte element the `octeti` word names.
///
/// The producer interns the byte-list element either as the raw 8-bit word
/// (`b8`, the seeded `octeti_list`) or as the sized unsigned numeric
/// (`numerus<u8>`, the octeti type-identity bridge), so both spell the same
/// u8 element; a wider width or another primitive is a different array.
fn is_octeti_byte_element(
    types: &radix_types::TypeTableSnapshot,
    element: radix_types::TypeId,
) -> bool {
    matches!(
        types.types().get(element.0 as usize),
        Some(
            radix_types::Type::Bits(radix_types::BitsWidth::B8)
                | radix_types::Type::SizedNumeric(
                    radix_types::Primitive::Numerus,
                    radix_types::NumericWidth::U8,
                ),
        )
    )
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
    alias_params: &FxHashMap<radix_types::DefId, Vec<radix_lexer::Symbol>>,
    conversation: Option<radix_types::DefId>,
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

    let (witness_def, applied_args) = match named_witness(types, form.ty) {
        Some(witness) => witness,
        None => {
            // A generic alias records both its parametric body and the
            // instantiated alias wrapper at the applied site. The generic
            // fallback binds the authored arguments to the declared
            // parameters, so it runs first; the exact-body witness applies
            // only to an alias with no typus parameters, because its
            // instantiated wrapper carries no argument witnesses to check.
            if let Some(applied) =
                transparent_generic_alias_witness(types, alias_params, path_def, form)
            {
                (path_def, applied)
            } else if alias_params.get(&path_def).is_none_or(Vec::is_empty)
                && transparent_alias_body_witness(types, path_def, form.ty)
            {
                (path_def, Vec::new())
            } else {
                return Err(type_presentation_issue("named_witness_not_named", site));
            }
        }
    };
    if witness_def != path_def {
        return Err(type_presentation_issue(
            "named_witness_definition_mismatch",
            site,
        ));
    }
    if applied_args.len() != form.args.len() {
        // The conversation builtin finalizes `sermo<O, R>` to its bare carrier
        // struct, so its witness carries no argument list even though the
        // author spelled two type arguments. Exactly the compiler-designated
        // constructor pairs that way, and only with two authored `Type`
        // children; every other named type keeps the strict arity rule.
        if !is_conversation_bare_carrier(types, conversation, path_def, &applied_args, form) {
            return Err(type_presentation_issue("named_argument_arity", site));
        }
        for arg in &form.args {
            let radix_hir::HirTypeFormArg::Type(child) = arg else {
                return Err(type_presentation_issue("conversation_argument_kind", site));
            };
            validate_type_form(
                child,
                site,
                root_slot,
                types,
                named_defs,
                alias_params,
                conversation,
                check_symbol,
                check_def,
                check_type,
                check_index,
            )?;
        }
        return Ok(());
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
                        alias_params,
                        conversation,
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
                radix_hir::HirTypeFormArg::Size(_) | radix_hir::HirTypeFormArg::Range(_) => {
                    return Err(type_presentation_issue("named_argument_kind", site));
                }
            },
            radix_types::AppliedArg::Size(expected) => {
                // A size-generic genus applied to a size (`Box<3>`) binds a
                // magnitude parameter; pair the authored `Size` with the
                // semantic witness exactly as the tensor-family shape
                // argument does. Any other authored kind still fails closed
                // through `size_argument_kind`.
                validate_size_arg(arg, expected, site, check_index)?;
            }
        }
    }
    Ok(())
}

/// Whether a `Named` form is the designated conversation constructor's bare
/// carrier pairing.
///
/// `sermo<O, R>` is finalized to its bare `Type::Struct` carrier, so the site
/// witness holds no `Applied` argument list while the author spelled the
/// conversation's two type arguments. Only the compiler-owned constructor
/// recorded on the artifact takes that shape; any other definition, any other
/// witness shape, and any other authored arity keeps the strict rule.
fn is_conversation_bare_carrier(
    types: &radix_types::TypeTableSnapshot,
    conversation: Option<radix_types::DefId>,
    path_def: radix_types::DefId,
    applied_args: &[radix_types::AppliedArg],
    form: &radix_hir::HirTypeForm,
) -> bool {
    conversation == Some(path_def)
        && applied_args.is_empty()
        && form.args.len() == 2
        && matches!(
            types.types().get(form.ty.0 as usize),
            Some(radix_types::Type::Struct(def)) if *def == path_def
        )
}

/// Validate the artifact's recorded conversation-constructor identity.
///
/// The metadata is the compiler's own `sermo` definition
/// (`Resolver::builtin_sermo_def_id`), so its `DefId` must resolve against the
/// artifact and the type table must carry that definition's bare struct — the
/// carrier shape the authored-form validator pairs an authored `sermo<O, R>`
/// with. A stale or unrelated identity fails closed.
fn validate_conversation_constructor(
    artifact: &HirArtifact,
    check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
) -> Result<(), HirArtifactError> {
    let Some(def_id) = artifact.conversation_constructor else {
        return Ok(());
    };
    check_def(def_id)?;
    let carrier = artifact
        .types
        .types()
        .iter()
        .any(|ty| matches!(ty, radix_types::Type::Struct(def) if *def == def_id));
    if !carrier {
        return Err(HirArtifactError::InvalidPresentation {
            issue: "conversation_constructor_not_carrier",
            anchor: def_id.0,
        });
    }
    Ok(())
}

/// Whether the artifact's type table records `path_def` as an alias whose
/// resolved body is exactly `body`.
///
/// A transparent type alias annotation is finalized at its HIR site to the
/// aliased body, so the authored `Named` head names the alias while the site
/// witness carries no `Type::Alias` wrapper. The recorded alias-to-body fact
/// is the witness for that body; absent it the form still fails closed.
fn transparent_alias_body_witness(
    types: &radix_types::TypeTableSnapshot,
    path_def: radix_types::DefId,
    body: radix_types::TypeId,
) -> bool {
    types.types().iter().any(|ty| {
        let radix_types::Type::Alias(def, alias_body) = ty else {
            return false;
        };
        *def == path_def && alias_body_matches(types, *alias_body, body)
    })
}

/// Whether a recorded alias body names `target`, directly or through a chain of
/// further transparent aliases.
///
/// `type B = A` records `Alias(B, Alias(A, int))` while the site it annotates is
/// finalized to `int`, so the chain — not just the immediate body — is the
/// alias-to-body fact. The body can also nest that chain inside a container
/// (`type C = list<B>` records `Array(Alias(B, …))` while the site carries
/// `Array(int)`), so the comparison strips aliases at every level. Any shape
/// this does not pair exactly fails closed.
fn alias_body_matches(
    types: &radix_types::TypeTableSnapshot,
    body: radix_types::TypeId,
    target: radix_types::TypeId,
) -> bool {
    let body = strip_transparent_aliases(types, body);
    let target = strip_transparent_aliases(types, target);
    if body == target {
        return true;
    }
    let pair = |left: radix_types::TypeId, right: radix_types::TypeId| {
        alias_body_matches(types, left, right)
    };
    match (
        types.types().get(body.0 as usize),
        types.types().get(target.0 as usize),
    ) {
        (Some(radix_types::Type::Array(left)), Some(radix_types::Type::Array(right)))
        | (Some(radix_types::Type::Queue(left)), Some(radix_types::Type::Queue(right)))
        | (Some(radix_types::Type::Stack(left)), Some(radix_types::Type::Stack(right)))
        | (Some(radix_types::Type::Set(left)), Some(radix_types::Type::Set(right)))
        | (Some(radix_types::Type::Promissum(left)), Some(radix_types::Type::Promissum(right)))
        | (Some(radix_types::Type::Cursor(left)), Some(radix_types::Type::Cursor(right)))
        | (Some(radix_types::Type::Atomic(left)), Some(radix_types::Type::Atomic(right)))
        | (Some(radix_types::Type::Option(left)), Some(radix_types::Type::Option(right)))
        | (Some(radix_types::Type::Ref(_, left)), Some(radix_types::Type::Ref(_, right))) => {
            pair(*left, *right)
        }
        (
            Some(radix_types::Type::Map(left_key, left_value)),
            Some(radix_types::Type::Map(right_key, right_value)),
        )
        | (
            Some(radix_types::Type::PromissumFailable(left_key, left_value)),
            Some(radix_types::Type::PromissumFailable(right_key, right_value)),
        )
        | (
            Some(radix_types::Type::AsyncCursor(left_key, left_value)),
            Some(radix_types::Type::AsyncCursor(right_key, right_value)),
        ) => pair(*left_key, *right_key) && pair(*left_value, *right_value),
        (Some(radix_types::Type::Tuple(left)), Some(radix_types::Type::Tuple(right))) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| pair(*left, *right))
        }
        (Some(radix_types::Type::Func(left)), Some(radix_types::Type::Func(right))) => {
            left.type_params == right.type_params
                && left.params.len() == right.params.len()
                && left.params.iter().zip(&right.params).all(|(left, right)| {
                    left.mode == right.mode
                        && left.optional == right.optional
                        && left.sponte == right.sponte
                        && pair(left.ty, right.ty)
                })
                && pair(left.ret, right.ret)
                && match (left.err, right.err) {
                    (Some(left), Some(right)) => pair(left, right),
                    (None, None) => true,
                    _ => false,
                }
                && left.is_async == right.is_async
                && left.is_generator == right.is_generator
        }
        _ => false,
    }
}

/// Follow a chain of transparent aliases to the type it ends at.
fn strip_transparent_aliases(
    types: &radix_types::TypeTableSnapshot,
    ty: radix_types::TypeId,
) -> radix_types::TypeId {
    let mut current = ty;
    let mut seen = FxHashSet::default();
    while seen.insert(current) {
        match types.types().get(current.0 as usize) {
            Some(radix_types::Type::Alias(_, inner)) => current = *inner,
            _ => return current,
        }
    }
    current
}

/// Whether a generic alias body structurally matches an authored site witness.
///
/// A generic alias body is parametric: the recorded `Type::Alias(def, body)`
/// keeps its declared parameters (`map<K, V>`) while the authored site is
/// finalized to the substitution (`map<string, int>`), so an exact-body
/// comparison can never agree. Matching therefore binds each authored argument
/// to the alias parameter that names it — `bindings[i]` pairs the i-th
/// parameter name with that argument's witness — and compares the two shapes
/// structurally. A parameter the bindings do not name, a mismatched variant, or
/// a shape outside the recursive arms below fails closed.
fn generic_alias_body_matches(
    types: &radix_types::TypeTableSnapshot,
    bindings: &[(radix_lexer::Symbol, radix_types::TypeId)],
    body: radix_types::TypeId,
    witness: radix_types::TypeId,
) -> bool {
    let (Some(body_ty), Some(witness_ty)) = (
        types.types().get(body.0 as usize),
        types.types().get(witness.0 as usize),
    ) else {
        return false;
    };
    let recurse = |body: radix_types::TypeId, witness: radix_types::TypeId| {
        generic_alias_body_matches(types, bindings, body, witness)
    };
    let same_variant = std::mem::discriminant(body_ty) == std::mem::discriminant(witness_ty);
    match (body_ty, witness_ty) {
        // A parameter binds by name against the authored argument witness, so a
        // permuted body (`map<V, K>`) still matches its own substitution.
        (radix_types::Type::Param(name), _) => bindings
            .iter()
            .any(|(param, arg)| param == name && *arg == witness),
        (
            radix_types::Type::Array(body_child)
            | radix_types::Type::Queue(body_child)
            | radix_types::Type::Stack(body_child)
            | radix_types::Type::Set(body_child)
            | radix_types::Type::Promissum(body_child)
            | radix_types::Type::Cursor(body_child)
            | radix_types::Type::Atomic(body_child)
            | radix_types::Type::Option(body_child),
            radix_types::Type::Array(witness_child)
            | radix_types::Type::Queue(witness_child)
            | radix_types::Type::Stack(witness_child)
            | radix_types::Type::Set(witness_child)
            | radix_types::Type::Promissum(witness_child)
            | radix_types::Type::Cursor(witness_child)
            | radix_types::Type::Atomic(witness_child)
            | radix_types::Type::Option(witness_child),
        ) => same_variant && recurse(*body_child, *witness_child),
        (
            radix_types::Type::Intervallum(body_child, body_step),
            radix_types::Type::Intervallum(witness_child, witness_step),
        ) => body_step == witness_step && recurse(*body_child, *witness_child),
        (
            radix_types::Type::Ref(body_mutability, body_child),
            radix_types::Type::Ref(witness_mutability, witness_child),
        ) => body_mutability == witness_mutability && recurse(*body_child, *witness_child),
        (
            radix_types::Type::RangedArray {
                element: body_child,
                min: body_min,
                max: body_max,
            },
            radix_types::Type::RangedArray {
                element: witness_child,
                min: witness_min,
                max: witness_max,
            },
        ) => {
            body_min == witness_min
                && body_max == witness_max
                && recurse(*body_child, *witness_child)
        }
        (
            radix_types::Type::BoundedArray {
                element: body_child,
                capacity: body_capacity,
            }
            | radix_types::Type::BoundedQueue {
                element: body_child,
                capacity: body_capacity,
            }
            | radix_types::Type::BoundedStack {
                element: body_child,
                capacity: body_capacity,
            },
            radix_types::Type::BoundedArray {
                element: witness_child,
                capacity: witness_capacity,
            }
            | radix_types::Type::BoundedQueue {
                element: witness_child,
                capacity: witness_capacity,
            }
            | radix_types::Type::BoundedStack {
                element: witness_child,
                capacity: witness_capacity,
            },
        ) => {
            same_variant
                && body_capacity == witness_capacity
                && recurse(*body_child, *witness_child)
        }
        (
            radix_types::Type::Tensor(body_child, body_shape)
            | radix_types::Type::Vector(body_child, body_shape)
            | radix_types::Type::Matrix(body_child, body_shape)
            | radix_types::Type::Sparsa(body_child, body_shape),
            radix_types::Type::Tensor(witness_child, witness_shape)
            | radix_types::Type::Vector(witness_child, witness_shape)
            | radix_types::Type::Matrix(witness_child, witness_shape)
            | radix_types::Type::Sparsa(witness_child, witness_shape),
        ) => same_variant && body_shape == witness_shape && recurse(*body_child, *witness_child),
        (
            radix_types::Type::Map(body_key, body_value)
            | radix_types::Type::Ledger(body_key, body_value)
            | radix_types::Type::PromissumFailable(body_key, body_value)
            | radix_types::Type::AsyncCursor(body_key, body_value),
            radix_types::Type::Map(witness_key, witness_value)
            | radix_types::Type::Ledger(witness_key, witness_value)
            | radix_types::Type::PromissumFailable(witness_key, witness_value)
            | radix_types::Type::AsyncCursor(witness_key, witness_value),
        ) => {
            same_variant && recurse(*body_key, *witness_key) && recurse(*body_value, *witness_value)
        }
        (
            radix_types::Type::Union(body_members)
            | radix_types::Type::Tuple(body_members)
            | radix_types::Type::Intersection(body_members),
            radix_types::Type::Union(witness_members)
            | radix_types::Type::Tuple(witness_members)
            | radix_types::Type::Intersection(witness_members),
        ) => {
            same_variant
                && body_members.len() == witness_members.len()
                && body_members
                    .iter()
                    .zip(witness_members)
                    .all(|(body, witness)| recurse(*body, *witness))
        }
        (
            radix_types::Type::Alias(body_def, body_child),
            radix_types::Type::Alias(witness_def, witness_child),
        ) => body_def == witness_def && recurse(*body_child, *witness_child),
        (
            radix_types::Type::Applied(body_base, body_args),
            radix_types::Type::Applied(witness_base, witness_args),
        ) => {
            recurse(*body_base, *witness_base)
                && body_args.len() == witness_args.len()
                && body_args
                    .iter()
                    .zip(witness_args)
                    .all(|(body, witness)| match (body, witness) {
                        (
                            radix_types::AppliedArg::Type(body),
                            radix_types::AppliedArg::Type(witness),
                        ) => recurse(*body, *witness),
                        (
                            radix_types::AppliedArg::Size(body),
                            radix_types::AppliedArg::Size(witness),
                        ) => body == witness,
                        (radix_types::AppliedArg::Type(_), radix_types::AppliedArg::Size(_))
                        | (radix_types::AppliedArg::Size(_), radix_types::AppliedArg::Type(_)) => {
                            false
                        }
                    })
        }
        (radix_types::Type::Record(body_fields), radix_types::Type::Record(witness_fields)) => {
            body_fields.len() == witness_fields.len()
                && body_fields.iter().all(|(key, body_field)| {
                    witness_fields
                        .get(key)
                        .is_some_and(|witness_field| recurse(*body_field, *witness_field))
                })
        }
        // Fail closed: primitives and nominals compare by identity, and an
        // unhandled variant pair never counts as a structural match.
        (body_ty, witness_ty) => body_ty == witness_ty,
    }
}

/// The applied-argument witness for a generic alias form, when it resolves.
///
/// A generic alias site is finalized to the substitution (`pairs<int, str>`)
/// while the recorded body keeps its declared parameters (`pairs<K, V>`), so
/// the authored arguments cannot be checked against the body directly. This
/// helper binds each typus parameter to the authored argument that names it
/// and accepts only when the body structurally matches the site witness under
/// those bindings (see [`generic_alias_body_matches`]); the returned arguments
/// are the witnesses the matched site itself carries. An alias the artifact
/// never declared, a body the table lost, an arity mismatch, or any
/// non-type argument fails closed.
fn transparent_generic_alias_witness(
    types: &radix_types::TypeTableSnapshot,
    alias_params: &FxHashMap<radix_types::DefId, Vec<radix_lexer::Symbol>>,
    path_def: radix_types::DefId,
    form: &radix_hir::HirTypeForm,
) -> Option<Vec<radix_types::AppliedArg>> {
    let params = alias_params.get(&path_def)?;
    let body = types.types().iter().find_map(|ty| match ty {
        radix_types::Type::Alias(def, alias_body) if *def == path_def => Some(*alias_body),
        _ => None,
    })?;
    if form.args.len() != params.len() {
        return None;
    }
    let children: Vec<&radix_hir::HirTypeForm> = form
        .args
        .iter()
        .map(|arg| match arg {
            radix_hir::HirTypeFormArg::Type(child) => Some(child.as_ref()),
            radix_hir::HirTypeFormArg::Name(_)
            | radix_hir::HirTypeFormArg::Size(_)
            | radix_hir::HirTypeFormArg::Range(_) => None,
        })
        .collect::<Option<Vec<&radix_hir::HirTypeForm>>>()?;
    let bindings: Vec<(radix_lexer::Symbol, radix_types::TypeId)> = params
        .iter()
        .copied()
        .zip(children.iter().map(|child| child.ty))
        .collect();
    generic_alias_body_matches(types, &bindings, body, form.ty).then(|| {
        children
            .iter()
            .map(|child| radix_types::AppliedArg::Type(child.ty))
            .collect()
    })
}

fn named_witness(
    types: &radix_types::TypeTableSnapshot,
    ty: radix_types::TypeId,
) -> Option<(radix_types::DefId, Vec<radix_types::AppliedArg>)> {
    let mut current = ty;
    let mut seen = FxHashSet::default();
    let mut outer_args = None;
    while seen.insert(current) {
        match types.types().get(current.0 as usize)? {
            radix_types::Type::Struct(def_id)
            | radix_types::Type::Enum(def_id)
            | radix_types::Type::Interface(def_id)
            | radix_types::Type::Alias(def_id, _) => {
                return Some((*def_id, outer_args.unwrap_or_default()));
            }
            radix_types::Type::Applied(base, args) => {
                outer_args.get_or_insert_with(|| args.clone());
                current = *base;
            }
            radix_types::Type::Ref(_, inner) => current = *inner,
            _ => return None,
        }
    }
    None
}

fn range_endpoint(
    bound: &radix_hir::HirRangeBound,
) -> Option<radix_types::numeric_width::RangeEnd> {
    match bound {
        radix_hir::HirRangeBound::Literal(value) => value
            .parse::<i128>()
            .ok()
            .map(radix_types::numeric_width::RangeEnd::Value),
        radix_hir::HirRangeBound::Unbounded { negative: true } => {
            Some(radix_types::numeric_width::RangeEnd::NegInf)
        }
        radix_hir::HirRangeBound::Unbounded { negative: false } => {
            Some(radix_types::numeric_width::RangeEnd::PosInf)
        }
        radix_hir::HirRangeBound::Name { .. } => None,
    }
}

fn validate_policy_range(
    form: &radix_hir::HirTypeForm,
    ty: &radix_types::Type,
    site: &radix_hir::HirTypeSite,
    check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
) -> Result<(), HirArtifactError> {
    let [radix_hir::HirTypeFormArg::Range(range)] = form.args.as_slice() else {
        return Err(type_presentation_issue("policy_range_arity", site));
    };
    for bound in [&range.lo, &range.hi] {
        if let radix_hir::HirRangeBound::Name { name, .. } = bound {
            check_symbol(*name)?;
        }
    }
    let (Some(lo), Some(hi)) = (range_endpoint(&range.lo), range_endpoint(&range.hi)) else {
        return Err(type_presentation_issue("policy_range_not_constant", site));
    };
    if matches!(
        ty,
        radix_types::Type::SizedNumeric(radix_types::Primitive::Numerus, width)
            if width.is_unbounded()
    ) {
        return if matches!(lo, radix_types::numeric_width::RangeEnd::NegInf)
            && matches!(hi, radix_types::numeric_width::RangeEnd::PosInf)
        {
            Ok(())
        } else {
            Err(type_presentation_issue("policy_range_witness", site))
        };
    }
    let (width, bounds) = match ty {
        radix_types::Type::RangedInt { width, lo, hi, .. } => (*width, (*lo, *hi)),
        radix_types::Type::SizedNumeric(radix_types::Primitive::Numerus, width)
        | radix_types::Type::ModularWord(width)
        | radix_types::Type::SaturatingWord(width) => (
            *width,
            width
                .integer_range()
                .ok_or_else(|| type_presentation_issue("policy_range_witness", site))?,
        ),
        _ => return Err(type_presentation_issue("policy_range_witness", site)),
    };
    let resolved = radix_types::resolve_ranged_int(width, lo, hi, range.inclusive)
        .map_err(|_| type_presentation_issue("policy_range_witness", site))?;
    if resolved != bounds {
        return Err(type_presentation_issue("policy_range_witness", site));
    }
    Ok(())
}

/// The authored `size lo‥hi` range of a list form, checked
/// against the resolved witness: the endpoints are constants and resolve to the
/// witness's length bounds. The authored range is a presentation record; the
/// witness stays the authority.
fn validate_list_size_range(
    range: &radix_hir::HirRangeArgument,
    ty: &radix_types::Type,
    types: &radix_types::TypeTableSnapshot,
    site: &radix_hir::HirTypeSite,
) -> Result<(), HirArtifactError> {
    let (Some(lo), Some(hi)) = (range_endpoint(&range.lo), range_endpoint(&range.hi)) else {
        return Err(type_presentation_issue("list_range_not_constant", site));
    };
    let resolved = radix_types::numeric_width::resolve_ranged_list(lo, hi, range.inclusive)
        .map_err(|_| type_presentation_issue("list_range_witness", site))?;
    let witness = match ty {
        radix_types::Type::Array(_) => (0, None),
        radix_types::Type::RangedArray { min, max, .. } => (*min, *max),
        radix_types::Type::BoundedArray { capacity, .. }
        | radix_types::Type::BoundedOcteti { capacity } => {
            match types.indices().get(capacity.0 as usize) {
                Some(radix_types::IndexExpr::Literal(max)) => (0, Some(*max)),
                _ => return Err(type_presentation_issue("list_range_witness", site)),
            }
        }
        _ => return Err(type_presentation_issue("list_range_witness", site)),
    };
    if resolved != witness {
        return Err(type_presentation_issue("list_range_witness", site));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_type_form(
    form: &radix_hir::HirTypeForm,
    site: &radix_hir::HirTypeSite,
    root_slot: &radix_hir::HirTypeSlot,
    types: &radix_types::TypeTableSnapshot,
    named_defs: &FxHashSet<radix_types::DefId>,
    alias_params: &FxHashMap<radix_types::DefId, Vec<radix_lexer::Symbol>>,
    conversation: Option<radix_types::DefId>,
    check_symbol: &dyn Fn(radix_lexer::Symbol) -> Result<(), HirArtifactError>,
    check_def: &dyn Fn(radix_types::DefId) -> Result<(), HirArtifactError>,
    check_type: &dyn Fn(radix_types::TypeId) -> Result<(), HirArtifactError>,
    check_index: &dyn Fn(radix_types::IndexId) -> Result<(), HirArtifactError>,
) -> Result<(), HirArtifactError> {
    check_type(form.ty)?;
    match &form.head {
        radix_hir::HirTypeFormHead::Infer | radix_hir::HirTypeFormHead::UnionHole => {
            // A `_` hole is legal at a binding cell. An inferred constructor
            // or typed-constructor ascription (`_ { … }`, `{ … } ∷ T`) also
            // records its authored hole at the expression's materialized
            // `AscriptionTarget` slot, and a closure parameter written `_ m`
            // records its hole at the parameter's `ClosureParameter` slot; in
            // both cases the root-witness check has already pinned that slot's
            // type to the HIR node's resolved type, so the hole carries no
            // further presentation fact there. Other slots still fail closed.
            let legal = match root_slot {
                radix_hir::HirTypeSlot::Declared | radix_hir::HirTypeSlot::DiscardBinding => true,
                radix_hir::HirTypeSlot::AscriptionTarget
                | radix_hir::HirTypeSlot::ClosureParameter(_) => {
                    matches!(form.head, radix_hir::HirTypeFormHead::Infer)
                }
                _ => false,
            };
            if !legal {
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
                alias_params,
                conversation,
                *def_id,
                path,
                check_symbol,
                check_def,
                check_type,
                check_index,
                root_slot,
            );
        }
        radix_hir::HirTypeFormHead::ImportedAlias { path } => {
            // An imported alias is carried as its resolved body, so the
            // witness is the whole presentation: the authored path and no
            // authored arguments.
            if path.is_empty() {
                return Err(type_presentation_issue("empty_imported_alias_path", site));
            }
            for symbol in path {
                check_symbol(*symbol)?;
            }
            if !form.args.is_empty() {
                return Err(type_presentation_issue("imported_alias_arguments", site));
            }
            return Ok(());
        }
        radix_hir::HirTypeFormHead::Semantic(level) => {
            let ty = types
                .types()
                .get(form.ty.0 as usize)
                .ok_or_else(|| type_presentation_issue("type_witness_missing", site))?;
            match ty {
                radix_types::Type::SizedNumeric(prim, width) => {
                    // Level 2 is the explicit policy word (`trapping<u8>`);
                    // a retained full-width range is the sole permitted arg.
                    let policy_ok = level.0 != 2 || *prim == radix_types::Primitive::Numerus;
                    let inf_word_ok = !matches!(level.0, 3 | 4)
                        || (*prim == radix_types::Primitive::Numerus && width.is_unbounded());
                    let range_arg =
                        matches!(form.args.as_slice(), [radix_hir::HirTypeFormArg::Range(_)]);
                    if level.0 > 4
                        || !policy_ok
                        || !inf_word_ok
                        || (!form.args.is_empty()
                            && !(range_arg
                                && matches!(level.0, 2..=4)
                                && *prim == radix_types::Primitive::Numerus))
                    {
                        return Err(type_presentation_issue("numeric_level_or_arity", site));
                    }
                    if range_arg {
                        validate_policy_range(form, ty, site, check_symbol)?;
                    }
                }
                radix_types::Type::PolicyFloat(_, _) => {
                    // Float policy cells do not admit ranged integer arguments.
                    if !matches!(level.0, 0 | 2) || !form.args.is_empty() {
                        return Err(type_presentation_issue("semantic_level_or_arity", site));
                    }
                }
                radix_types::Type::RangedInt { .. } => {
                    if !matches!(level.0, 0 | 2)
                        || (!form.args.is_empty()
                            && !matches!(
                                form.args.as_slice(),
                                [radix_hir::HirTypeFormArg::Range(_)]
                            ))
                    {
                        return Err(type_presentation_issue("semantic_level_or_arity", site));
                    }
                    if !form.args.is_empty() {
                        validate_policy_range(form, ty, site, check_symbol)?;
                    }
                }
                radix_types::Type::Bits(_) => {
                    // A bare width word (`b8`) is authored at SHORT level; the
                    // long form remains legal for a spelled-out head.
                    if !matches!(level.0, 0 | 1) || !form.args.is_empty() {
                        return Err(type_presentation_issue("semantic_level_or_arity", site));
                    }
                }
                radix_types::Type::Primitive(_)
                | radix_types::Type::TensorEdgePolicy(_)
                | radix_types::Type::SizedInstans(_)
                | radix_types::Type::BoundedTextus { .. }
                | radix_types::Type::BoundedAscii { .. } => {
                    if level.0 != 0 || !form.args.is_empty() {
                        return Err(type_presentation_issue("semantic_level_or_arity", site));
                    }
                }
                radix_types::Type::ModularWord(width)
                | radix_types::Type::SaturatingWord(width) => {
                    if level.0 != 0
                        || (!form.args.is_empty()
                            && !matches!(
                                form.args.as_slice(),
                                [radix_hir::HirTypeFormArg::Range(_)]
                            ))
                    {
                        return Err(type_presentation_issue("semantic_level_or_arity", site));
                    }
                    if !form.args.is_empty() {
                        validate_policy_range(form, ty, site, check_symbol)?;
                    }
                    // Ensure these policy carriers have an integer width.
                    if width.integer_range().is_none() {
                        return Err(type_presentation_issue("policy_range_witness", site));
                    }
                }
                radix_types::Type::BoundedOcteti { .. } => {
                    // The authored `list<b8, size N>`: the bounded byte buffer is
                    // authored as a list head over one raw-byte (`b8`) element
                    // argument; the capacity lives in the root witness, as for
                    // every bounded list.
                    // `size 0…8` keeps its authored range beside the element.
                    let authored_range = if let [_, radix_hir::HirTypeFormArg::Range(range)] =
                        form.args.as_slice()
                    {
                        Some(range)
                    } else {
                        None
                    };
                    if level.0 != 0 || (authored_range.is_none() && form.args.len() != 1) {
                        return Err(type_presentation_issue("bounded_octeti_shape", site));
                    }
                    if let Some(range) = authored_range {
                        validate_list_size_range(range, ty, types, site)?;
                    }
                    let radix_hir::HirTypeFormArg::Type(child) = &form.args[0] else {
                        return Err(type_presentation_issue("type_argument_kind", site));
                    };
                    if !matches!(
                        types.types().get(child.ty.0 as usize),
                        Some(radix_types::Type::Bits(radix_types::BitsWidth::B8))
                    ) {
                        return Err(type_presentation_issue("bounded_octeti_element", site));
                    }
                    validate_type_arg(
                        &form.args[0],
                        child.ty,
                        site,
                        root_slot,
                        types,
                        named_defs,
                        alias_params,
                        conversation,
                        check_symbol,
                        check_def,
                        check_type,
                        check_index,
                    )?;
                }
                radix_types::Type::Array(element) => {
                    // `octeti` is authored as the zero-argument primitive word
                    // while its semantic witness is the byte-list array
                    // `lista<b8>`. The u8 element is the octeti/list identity,
                    // not a missing authored argument; every other element
                    // type and every other level still needs the one argument.
                    if form.args.is_empty()
                        && level.0 == 0
                        && is_octeti_byte_element(types, *element)
                    {
                        return Ok(());
                    }
                    // `size 0…∞` keeps the authored range beside the element.
                    if let [_, radix_hir::HirTypeFormArg::Range(range)] = form.args.as_slice() {
                        validate_list_size_range(range, ty, types, site)?;
                    } else if form.args.len() != 1 {
                        return Err(type_presentation_issue("array_argument_arity", site));
                    }
                    validate_type_arg(
                        &form.args[0],
                        *element,
                        site,
                        root_slot,
                        types,
                        named_defs,
                        alias_params,
                        conversation,
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
                radix_types::Type::BoundedArray { element, .. }
                | radix_types::Type::RangedArray { element, .. } => {
                    if let [_, radix_hir::HirTypeFormArg::Range(range)] = form.args.as_slice() {
                        validate_list_size_range(range, ty, types, site)?;
                    } else if form.args.len() != 1 {
                        return Err(type_presentation_issue("bounded_array_shape", site));
                    }
                    if level.0 != 0 {
                        return Err(type_presentation_issue("bounded_array_shape", site));
                    }
                    validate_type_arg(
                        &form.args[0],
                        *element,
                        site,
                        root_slot,
                        types,
                        named_defs,
                        alias_params,
                        conversation,
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
                        alias_params,
                        conversation,
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
                radix_types::Type::Map(key, value) | radix_types::Type::Ledger(key, value) => {
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
                            alias_params,
                            conversation,
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
                            alias_params,
                            conversation,
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
                            alias_params,
                            conversation,
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
                | radix_types::Type::Intervallum(element, _)
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
                                alias_params,
                                conversation,
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
                            alias_params,
                            conversation,
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
                        alias_params,
                        conversation,
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
                        alias_params,
                        conversation,
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
                        alias_params,
                        conversation,
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
                            alias_params,
                            conversation,
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
                        alias_params,
                        conversation,
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
                        alias_params,
                        conversation,
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
    let alias_params = collect_generic_alias_params(artifact);
    let conversation = artifact.conversation_constructor;
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
            &alias_params,
            conversation,
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
    let mut registry =
        collect_def_registry_for_parts(&artifact.hir, &artifact.libraries, &artifact.resolver);
    collect_carried_provider_defs(&artifact.provider_units, &mut registry);
    registry
}

/// Count the definition sites owned by provider-carried units into the
/// consumer's registry. A consumer's library-call signatures legitimately
/// name nominal defs owned by a provider (for example `fault.IoFault` in the
/// `err_ty` of an imported `fs` function), and those defs are absent from the
/// consumer's own arenas and resolver snapshot. Without them the referential
/// walk rejects valid consumer tables with a false `DanglingIndex`.
fn collect_carried_provider_defs(
    units: &std::collections::BTreeMap<String, crate::artifact::ProviderCarriageEntryWire>,
    registry: &mut FxHashSet<radix_types::DefId>,
) {
    use crate::artifact::ProviderCarriageEntryWire;

    for entry in units.values() {
        let ProviderCarriageEntryWire::Unit(provider) = entry else {
            continue;
        };
        registry.extend(collect_def_registry_for_parts(
            &provider.hir,
            &radix_hir::LibraryRegistry::default(),
            &provider.resolver,
        ));
        collect_carried_provider_defs(&provider.nested, registry);
    }
}

fn collect_def_registry_for_parts(
    hir: &radix_hir::HirModule,
    libraries: &radix_hir::LibraryRegistry,
    resolver: &crate::artifact::ResolverSnapshot,
) -> FxHashSet<radix_types::DefId> {
    let mut registry = FxHashSet::default();

    // Walk HIR items for definition-site DefIds.
    collect_item_defs(&hir.items, &mut registry);

    // Entry block (if present).
    if let Some(ref entry) = hir.entry {
        collect_block_defs(entry, &mut registry);
    }

    // Annotations on entry.
    for ann in &hir.entry_annotations {
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
    for def_id in libraries.bindings.keys() {
        registry.insert(*def_id);
    }
    for binding in libraries.bindings.values() {
        registry.insert(binding.local_def_id);
    }
    for def_id in libraries.items.keys() {
        registry.insert(*def_id);
    }
    for item in libraries.items.values() {
        registry.insert(item.def_id);
    }
    for def_id in libraries.exports.keys() {
        registry.insert(*def_id);
    }
    for (pair, _) in libraries.reexports.keys() {
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
    for (_, iface) in &resolver.file_interfaces {
        for (_, export) in &iface.exports {
            match export {
                crate::artifact::FileExportSnapshot::Struct {
                    def_id, methods, ..
                } => {
                    registry.insert(*def_id);
                    for method in methods {
                        registry.insert(method.def_id);
                    }
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

fn collect_item_defs(items: &[radix_hir::HirItem], registry: &mut FxHashSet<radix_types::DefId>) {
    use radix_hir::HirItemKind;

    for item in items {
        registry.insert(item.def_id);
        for ann in &item.annotations {
            collect_annotation_defs(ann, registry);
        }
        match &item.kind {
            HirItemKind::Function(func) => {
                collect_function_defs(func, registry);
            }
            HirItemKind::Struct(s) => {
                for tp in &s.type_params {
                    registry.insert(tp.def_id);
                }
                for field in &s.fields {
                    registry.insert(field.def_id);
                    for ann in &field.annotations {
                        collect_annotation_defs(ann, registry);
                    }
                    if let Some(ref init) = field.init {
                        collect_expr_defs(init, registry);
                    }
                }
                for method in &s.methods {
                    registry.insert(method.def_id);
                    for ann in &method.annotations {
                        collect_annotation_defs(ann, registry);
                    }
                    collect_function_defs(&method.func, registry);
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
                // A required signature is a definition site on the wire: its
                // def_id is the identity member trivia and a type-parameter
                // bound call target. Struct methods register above; without
                // this row the validator rejects those keys as dangling.
                for method in &iface.methods {
                    registry.insert(method.def_id);
                    for param in &method.params {
                        collect_param_defs(param, registry);
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
                collect_expr_defs(&c.value, registry);
                // A `comptime { … }` initializer keeps its authored body in
                // `statica.authored` after folding replaced `value`; that body
                // is still on the wire, so its defs must be registered too.
                if let Some(statica) = &c.statica
                    && let Some(authored) = &statica.authored
                {
                    collect_expr_defs(authored, registry);
                }
            }
            HirItemKind::Import(imp) => {
                for ii in &imp.items {
                    registry.insert(ii.def_id);
                }
            }
        }
    }
}

#[derive(Default)]
struct AwaitBindingCollector {
    defs: FxHashSet<radix_types::DefId>,
}

impl radix_hir::visit::HirVisitor for AwaitBindingCollector {
    fn visit_item(&mut self, item: &radix_hir::HirItem) {
        if let radix_hir::HirItemKind::Constant(constant) = &item.kind
            && constant.is_await
        {
            self.defs.insert(item.def_id);
        }
        radix_hir::visit::walk_item(self, item);
    }

    fn visit_local(&mut self, local: &radix_hir::HirLocal) {
        if local.is_await {
            self.defs.insert(local.def_id);
        }
        radix_hir::visit::walk_local(self, local);
    }
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
        HirStatementKind::Expr(expr)
        | HirStatementKind::Redde(Some(expr))
        | HirStatementKind::ReddeFilum(Some(expr)) => {
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
        | HirStatementKind::Redde(None)
        | HirStatementKind::ReddeFilum(None) => {}
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
        | HirExpressionKind::Insere(_)
        | HirExpressionKind::Vacua
        | HirExpressionKind::ReadLine(_)
        | HirExpressionKind::Error => {}
        HirExpressionKind::Marked { expr, .. } => collect_expr_defs(expr, registry),
        HirExpressionKind::Reduction {
            source,
            coordinates,
            group,
            filum,
            binding,
            body,
            identity,
            ..
        } => {
            collect_expr_defs(source, registry);
            for (id, _) in coordinates {
                registry.insert(*id);
            }
            if let Some((id, _)) = filum {
                registry.insert(*id);
            }
            if let Some((id, _)) = binding {
                registry.insert(*id);
            }
            if let Some(group) = group {
                collect_expr_defs(group, registry);
            }
            if let Some(body) = body {
                collect_block_defs(body, registry);
            }
            if let Some(identity) = identity {
                collect_expr_defs(identity, registry);
            }
        }
        HirExpressionKind::Scan {
            source,
            coordinates,
            group,
            filum,
            binding,
            body,
            ..
        } => {
            collect_expr_defs(source, registry);
            for (id, _) in coordinates {
                registry.insert(*id);
            }
            registry.insert(filum.0);
            registry.insert(binding.0);
            if let Some(group) = group {
                collect_expr_defs(group, registry);
            }
            collect_block_defs(body, registry);
        }
        HirExpressionKind::Sort {
            source,
            binding,
            body,
            ..
        } => {
            collect_expr_defs(source, registry);
            if let Some((id, _)) = binding {
                registry.insert(*id);
            }
            if let Some(body) = body {
                collect_block_defs(body, registry);
            }
        }
        HirExpressionKind::Praefixum(inner)
        | HirExpressionKind::Format { value: inner, .. }
        | HirExpressionKind::Unary(_, inner)
        | HirExpressionKind::Ref(_, inner)
        | HirExpressionKind::Deref(inner) => {
            collect_expr_defs(inner, registry);
        }
        HirExpressionKind::CopyInto { target, value } => {
            collect_expr_defs(target, registry);
            collect_expr_defs(value, registry);
        }
        HirExpressionKind::SinkInto { sink, item } => {
            collect_expr_defs(sink, registry);
            collect_expr_defs(item, registry);
        }
        HirExpressionKind::Binary(_, lhs, rhs) | HirExpressionKind::Assign(lhs, rhs) => {
            collect_expr_defs(lhs, registry);
            collect_expr_defs(rhs, registry);
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
                radix_hir::HirNonNullKind::Value | radix_hir::HirNonNullKind::Member(_) => {}
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
                collect_expr_defs(&arm.body, registry);
            }
        }
        HirExpressionKind::Loop(body) => collect_block_defs(body, registry),
        HirExpressionKind::Dum(cond, body) => {
            collect_expr_defs(cond, registry);
            collect_block_defs(body, registry);
        }
        HirExpressionKind::Itera {
            coordinates,
            binding: def_id,
            iter: iterable,
            body,
            ..
        } => {
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
            ..
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
            ..
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
        HirExpressionKind::ConversioAssign {
            target,
            source,
            recovery,
            ..
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
                    radix_hir::HirArrayElement::Infer(_) => {}
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
        HirExpressionKind::Scribe(_, args) | HirExpressionKind::Scriptum(_, args) => {
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
        HirExpressionKind::Panic(expr)
        | HirExpressionKind::Throw(expr)
        | HirExpressionKind::TypeCheck { expr, .. } => {
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
        HirExpressionKind::CallDefault { body, default, .. } => {
            collect_block_defs(body, registry);
            collect_expr_defs(default, registry);
        }
        HirExpressionKind::Clausura(params, _, _, body) => {
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
        HirExpressionKind::Widen { operand, .. } => collect_expr_defs(operand, registry),
        HirExpressionKind::Ad { opener, .. } => {
            if let Some(o) = opener {
                collect_expr_defs(o, registry);
            }
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
        radix_hir::HirPattern::Or(patterns) => {
            for pattern in patterns {
                collect_pattern_defs(pattern, registry);
            }
        }
        radix_hir::HirPattern::Literal(_)
        | radix_hir::HirPattern::NegatedLiteral(_)
        | radix_hir::HirPattern::Wildcard => {}
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
    collect_hir_registry_for_hir(&artifact.hir)
}

fn collect_hir_registry_for_hir(hir: &radix_hir::HirModule) -> FxHashSet<radix_hir::HirId> {
    use radix_hir::{HirExpressionKind, HirStatementKind};

    fn collect_block(block: &radix_hir::HirBlock, registry: &mut FxHashSet<radix_hir::HirId>) {
        for stmt in &block.statements {
            registry.insert(stmt.id);
            match &stmt.kind {
                HirStatementKind::Expr(expr)
                | HirStatementKind::Redde(Some(expr))
                | HirStatementKind::ReddeFilum(Some(expr)) => {
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
                | HirStatementKind::Redde(None)
                | HirStatementKind::ReddeFilum(None) => {}
            }
        }
        if let Some(ref expr) = block.expr {
            collect_expr(expr, registry);
        }
    }

    fn collect_expr(expr: &radix_hir::HirExpression, registry: &mut FxHashSet<radix_hir::HirId>) {
        registry.insert(expr.id);
        match &expr.kind {
            HirExpressionKind::Reduction {
                source,
                group,
                body,
                identity,
                ..
            } => {
                collect_expr(source, registry);
                if let Some(group) = group {
                    collect_expr(group, registry);
                }
                if let Some(body) = body {
                    collect_block(body, registry);
                }
                if let Some(identity) = identity {
                    collect_expr(identity, registry);
                }
            }
            HirExpressionKind::Scan {
                source,
                group,
                body,
                ..
            } => {
                collect_expr(source, registry);
                if let Some(group) = group {
                    collect_expr(group, registry);
                }
                collect_block(body, registry);
            }
            HirExpressionKind::Sort { source, body, .. } => {
                collect_expr(source, registry);
                if let Some(body) = body {
                    collect_block(body, registry);
                }
            }
            HirExpressionKind::Binary(_, lhs, rhs) | HirExpressionKind::Assign(lhs, rhs) => {
                collect_expr(lhs, registry);
                collect_expr(rhs, registry);
            }
            HirExpressionKind::Unary(_, inner)
            | HirExpressionKind::Ref(_, inner)
            | HirExpressionKind::Deref(inner)
            | HirExpressionKind::Praefixum(inner)
            | HirExpressionKind::Format { value: inner, .. } => {
                collect_expr(inner, registry);
            }
            HirExpressionKind::Call(callee, _, args, _) => {
                collect_expr(callee, registry);
                for arg in args {
                    collect_expr(&arg.expr, registry);
                }
            }
            HirExpressionKind::CopyInto { target, value } => {
                collect_expr(target, registry);
                collect_expr(value, registry);
            }
            HirExpressionKind::SinkInto { sink, item } => {
                collect_expr(sink, registry);
                collect_expr(item, registry);
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
            HirExpressionKind::Marked { expr, .. }
            | HirExpressionKind::TypeCheck { expr, .. }
            | HirExpressionKind::Panic(expr)
            | HirExpressionKind::Throw(expr)
            | HirExpressionKind::Cede(expr)
            | HirExpressionKind::Reddet(expr)
            | HirExpressionKind::Tacebit(expr)
            | HirExpressionKind::Yield(expr) => {
                collect_expr(expr, registry);
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
                    radix_hir::HirNonNullKind::Value | radix_hir::HirNonNullKind::Member(_) => {}
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
                    collect_expr(&arm.body, registry);
                }
            }
            HirExpressionKind::Loop(body) => collect_block(body, registry),
            HirExpressionKind::Dum(cond, body) => {
                collect_expr(cond, registry);
                collect_block(body, registry);
            }
            HirExpressionKind::Itera {
                iter: iterable,
                body,
                ..
            } => {
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
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
                ..
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
                        radix_hir::HirArrayElement::Infer(_) => {}
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
            HirExpressionKind::Handled { body, catch } => {
                collect_block(body, registry);
                collect_block(&catch.body, registry);
            }
            HirExpressionKind::Trap(body) => {
                collect_block(body, registry);
            }
            HirExpressionKind::CallDefault { body, default, .. } => {
                collect_block(body, registry);
                collect_expr(default, registry);
            }
            HirExpressionKind::Clausura(params, _, _, body) => {
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
            HirExpressionKind::Widen { operand, .. } => collect_expr(operand, registry),
            HirExpressionKind::Ad { opener, .. } => {
                if let Some(o) = opener {
                    collect_expr(o, registry);
                }
            }
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
            | HirExpressionKind::Insere(_)
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

    let mut registry = FxHashSet::default();

    // Collect from all items and entry.
    for item in &hir.items {
        registry.insert(item.id);
        for ann in &item.annotations {
            collect_annotation(ann, &mut registry);
        }
        match &item.kind {
            radix_hir::HirItemKind::Function(func) => {
                for param in &func.params {
                    if let Some(ref init) = param.default {
                        collect_expr(init, &mut registry);
                    }
                }
                if let Some(cli_args) = &func.cli_args
                    && let Some(default) = &cli_args.default
                {
                    collect_expr(default, &mut registry);
                }
                if let Some(ref body) = func.body {
                    collect_block(body, &mut registry);
                }
            }
            radix_hir::HirItemKind::Struct(s) => {
                for field in &s.fields {
                    for ann in &field.annotations {
                        collect_annotation(ann, &mut registry);
                    }
                    if let Some(ref init) = field.init {
                        collect_expr(init, &mut registry);
                    }
                }
                for method in &s.methods {
                    for ann in &method.annotations {
                        collect_annotation(ann, &mut registry);
                    }
                    for param in &method.func.params {
                        if let Some(ref init) = param.default {
                            collect_expr(init, &mut registry);
                        }
                    }
                    if let Some(cli_args) = &method.func.cli_args
                        && let Some(default) = &cli_args.default
                    {
                        collect_expr(default, &mut registry);
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
                // The authored `comptime { … }` body survives folding in
                // `statica.authored`; walk it so its HirIds stay resolvable.
                if let Some(statica) = &c.statica
                    && let Some(authored) = &statica.authored
                {
                    collect_expr(authored, &mut registry);
                }
            }
            _ => {}
        }
    }
    if let Some(ref entry) = hir.entry {
        collect_block(entry, &mut registry);
    }
    // Entry annotations may contain expressions.
    for ann in &hir.entry_annotations {
        collect_annotation(ann, &mut registry);
    }

    registry
}

/// Validate arena chains before any presentation helper follows them. Nominal
/// recursion terminates at Struct/Enum; these transparent edges must terminate.
fn validate_type_chains(types: &radix_types::TypeTableSnapshot) -> Result<(), HirArtifactError> {
    let mut done = vec![false; types.type_count()];
    for start in 0..done.len() {
        let mut path = Vec::new();
        let mut visiting = FxHashSet::default();
        let mut current = start;
        while current < done.len() && !done[current] {
            if !visiting.insert(current) {
                return Err(HirArtifactError::MalformedSnapshot {
                    kind: "types",
                    detail: format!("cyclic type chain at TypeId({current})"),
                });
            }
            path.push(current);
            current = match &types.types()[current] {
                radix_types::Type::Applied(base, _)
                | radix_types::Type::Ref(_, base)
                | radix_types::Type::Alias(_, base) => base.0 as usize,
                _ => break,
            };
        }
        for index in path {
            done[index] = true;
        }
    }
    Ok(())
}

fn validate_interner(rows: &[String]) -> Result<(), HirArtifactError> {
    let mut seen = FxHashSet::default();
    for row in rows {
        if !seen.insert(row) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "interner",
                detail: "duplicate string row".to_owned(),
            });
        }
    }
    Ok(())
}

struct IntegrityChecks<'a> {
    def_registry: &'a FxHashSet<radix_types::DefId>,
    hir_registry: &'a FxHashSet<radix_hir::HirId>,
    type_count: usize,
    interner_len: usize,
    index_count: usize,
}

impl IntegrityChecks<'_> {
    fn check_symbol(&self, sym: radix_lexer::Symbol) -> Result<(), HirArtifactError> {
        let idx = sym.0 as usize;
        if idx >= self.interner_len {
            return Err(HirArtifactError::DanglingIndex {
                kind: "Symbol",
                index: sym.0,
                arena_size: self.interner_len,
            });
        }
        Ok(())
    }

    fn check_def(&self, def_id: radix_types::DefId) -> Result<(), HirArtifactError> {
        if def_id.is_builtin() {
            return Ok(());
        }
        if !self.def_registry.contains(&def_id) {
            return Err(HirArtifactError::DanglingIndex {
                kind: "DefId",
                index: def_id.0,
                arena_size: self.def_registry.len(),
            });
        }
        Ok(())
    }

    /// A backward companion identity is synthesized in the companion `DefId`
    /// family and names a function the reverse-AD transform generates later,
    /// so it is declared nowhere in the artifact: it must be in that family,
    /// not in the definition registry.
    fn check_companion_def(&self, def_id: radix_types::DefId) -> Result<(), HirArtifactError> {
        if def_id.is_companion() {
            return Ok(());
        }
        Err(HirArtifactError::DanglingIndex {
            kind: "DefId",
            index: def_id.0,
            arena_size: self.def_registry.len(),
        })
    }

    fn check_hir(&self, hir_id: radix_hir::HirId) -> Result<(), HirArtifactError> {
        if !self.hir_registry.contains(&hir_id) {
            return Err(HirArtifactError::DanglingIndex {
                kind: "HirId",
                index: hir_id.0,
                arena_size: self.hir_registry.len(),
            });
        }
        Ok(())
    }

    fn check_type(&self, ty: radix_types::TypeId) -> Result<(), HirArtifactError> {
        let idx = ty.0 as usize;
        if idx >= self.type_count {
            return Err(HirArtifactError::DanglingIndex {
                kind: "TypeId",
                index: ty.0,
                arena_size: self.type_count,
            });
        }
        Ok(())
    }

    fn check_index(&self, index: radix_types::IndexId) -> Result<(), HirArtifactError> {
        let idx = index.0 as usize;
        if idx >= self.index_count {
            return Err(HirArtifactError::DanglingIndex {
                kind: "IndexId",
                index: index.0,
                arena_size: self.index_count,
            });
        }
        Ok(())
    }
}

fn validate_resolved_tensor_edge_policy(
    policy: &crate::artifact::ResolvedTensorEdgePolicyWire,
    checks: &IntegrityChecks<'_>,
) -> Result<(), HirArtifactError> {
    match policy {
        crate::artifact::ResolvedTensorEdgePolicyWire::Builtin(_) => Ok(()),
        crate::artifact::ResolvedTensorEdgePolicyWire::Custom {
            policy_def,
            remap,
            transform,
        } => {
            checks.check_def(*policy_def)?;
            checks.check_def(*remap)?;
            if let Some(transform) = transform {
                checks.check_def(*transform)?;
            }
            Ok(())
        }
        crate::artifact::ResolvedTensorEdgePolicyWire::Deferred { policy_type } => {
            checks.check_type(*policy_type)
        }
    }
}

fn validate_tensor_shift_context(
    context: crate::artifact::TensorShiftContextWire,
    checks: &IntegrityChecks<'_>,
) -> Result<(), HirArtifactError> {
    checks.check_index(context.source_shape)?;
    checks.check_hir(context.shift_expression)
}

/// Every `ordo` order row must name a comparison site and member variants that
/// exist in the artifact's arenas, and each site may carry at most one row set
/// (the live table is keyed by the left-operand id, so a duplicate would
/// silently overwrite).
fn validate_ordo_order_rows(
    table: &crate::artifact::TensorEdgePolicyTableWire,
    checks: &IntegrityChecks<'_>,
) -> Result<(), HirArtifactError> {
    let mut sites = FxHashSet::default();
    for comparison in &table.ordo_order {
        checks.check_hir(comparison.lhs)?;
        if !sites.insert(comparison.lhs) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "tensor edge policies",
                detail: format!("duplicate ordo order row for HirId({})", comparison.lhs.0),
            });
        }
        for (variant, _) in &comparison.rows {
            checks.check_def(*variant)?;
        }
    }
    Ok(())
}

fn validate_tensor_edge_policy_ids(
    table: &crate::artifact::TensorEdgePolicyTableWire,
    checks: &IntegrityChecks<'_>,
) -> Result<(), HirArtifactError> {
    for row in &table.calls {
        checks.check_hir(row.call)?;
        validate_resolved_tensor_edge_policy(&row.policy, checks)?;
    }
    for expression in &table.shifted_expressions {
        checks.check_hir(*expression)?;
    }
    for (expression, context) in &table.shifted_view_expressions {
        checks.check_hir(*expression)?;
        validate_tensor_shift_context(*context, checks)?;
    }
    for binding in &table.shifted_bindings {
        checks.check_def(*binding)?;
    }
    for (binding, context) in &table.shifted_view_bindings {
        checks.check_def(*binding)?;
        validate_tensor_shift_context(*context, checks)?;
    }
    for (call, context) in &table.limes_contexts {
        checks.check_hir(*call)?;
        validate_tensor_shift_context(*context, checks)?;
    }
    for row in &table.function_requirements {
        checks.check_def(row.function)?;
        for requirement in &row.requirements {
            checks.check_type(requirement.policy_type)?;
            checks.check_type(requirement.element_type)?;
        }
    }
    for provider in &table.provider_impls {
        checks.check_type(provider.policy_type)?;
        checks.check_type(provider.element_type)?;
        validate_resolved_tensor_edge_policy(&provider.policy, checks)?;
    }
    validate_ordo_order_rows(table, checks)
}

fn validate_shape_obligation_table(
    table: &crate::artifact::ShapeObligationTableWire,
    checks: &IntegrityChecks<'_>,
) -> Result<(), HirArtifactError> {
    let mut seen = FxHashSet::default();
    for row in &table.calls {
        if !seen.insert(row.call) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "shape obligations",
                detail: "duplicate calls row".to_owned(),
            });
        }
        checks.check_hir(row.call)?;
        for relation in &row.relations {
            checks.check_index(relation.left)?;
            checks.check_index(relation.right)?;
            if relation.boundary != crate::artifact::ShapeRelationBoundaryWire::Call {
                return Err(HirArtifactError::MalformedSnapshot {
                    kind: "shape obligations",
                    detail: "call row contains a non-call relation".to_owned(),
                });
            }
        }
    }
    let mut seen = FxHashSet::default();
    for row in &table.call_size_bindings {
        if !seen.insert(row.call) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "shape obligations",
                detail: "duplicate call_size_bindings row".to_owned(),
            });
        }
        checks.check_hir(row.call)?;
        for binding in &row.bindings {
            checks.check_index(binding.witness)?;
        }
    }
    let mut seen = FxHashSet::default();
    for row in &table.functions {
        if !seen.insert(row.function) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "shape obligations",
                detail: "duplicate functions row".to_owned(),
            });
        }
        checks.check_def(row.function)?;
        for relation in &row.relations {
            checks.check_index(relation.left)?;
            checks.check_index(relation.right)?;
            if matches!(
                relation.boundary,
                crate::artifact::ShapeRelationBoundaryWire::Call
                    | crate::artifact::ShapeRelationBoundaryWire::Store
            ) {
                return Err(HirArtifactError::MalformedSnapshot {
                    kind: "shape obligations",
                    detail: "function row contains a call or store relation".to_owned(),
                });
            }
        }
    }
    let mut seen = FxHashSet::default();
    for row in &table.stores {
        if !seen.insert(row.initializer) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "shape obligations",
                detail: "duplicate stores row".to_owned(),
            });
        }
        checks.check_hir(row.initializer)?;
        for relation in &row.relations {
            checks.check_index(relation.left)?;
            checks.check_index(relation.right)?;
            if relation.boundary != crate::artifact::ShapeRelationBoundaryWire::Store {
                return Err(HirArtifactError::MalformedSnapshot {
                    kind: "shape obligations",
                    detail: "store row contains a non-store relation".to_owned(),
                });
            }
        }
    }
    let mut seen = FxHashSet::default();
    for row in &table.function_size_bindings {
        if !seen.insert(row.function) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "shape obligations",
                detail: "duplicate function_size_bindings row".to_owned(),
            });
        }
        checks.check_def(row.function)?;
        for binding in &row.bindings {
            checks.check_index(binding.witness)?;
        }
    }
    for access in &table.proven_indexes {
        checks.check_hir(*access)?;
    }
    Ok(())
}

fn validate_kernel_value_range(
    range: &crate::artifact::KernelValueRangeWire,
    checks: &IntegrityChecks<'_>,
) -> Result<(), HirArtifactError> {
    fn validate_expression(
        expression: &crate::artifact::KernelRangeExprWire,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        use crate::artifact::KernelRangeExprWire as Range;
        match expression {
            Range::Shape(index) => checks.check_index(*index),
            Range::Add(lhs, rhs)
            | Range::Subtract(lhs, rhs)
            | Range::Multiply(lhs, rhs)
            | Range::FloorDivide(lhs, rhs)
            | Range::Minimum(lhs, rhs)
            | Range::Maximum(lhs, rhs) => {
                validate_expression(lhs, checks)?;
                validate_expression(rhs, checks)
            }
            // Constants are literals and a parameter term names a definition
            // rather than an arena index; neither is referential to check.
            Range::Constant(_) | Range::Parameter(_) => Ok(()),
        }
    }

    validate_expression(&range.minimum, checks)?;
    validate_expression(&range.maximum, checks)
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
    use radix_hir::{HirExpressionKind, HirItemKind, HirStatementKind};

    // A `Typus` default names a concrete type arena slot; `Magnitudo` defaults
    // are literals and carry no reference.
    fn validate_type_param_default(
        default: Option<&radix_types::SemanticGenericParamDefault>,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        if let Some(radix_types::SemanticGenericParamDefault::Typus(ty)) = default {
            checks.check_type(*ty)?;
        }
        Ok(())
    }

    // Shared by every HIR owner carrying type params (functions, aliases,
    // structs, enums, interfaces); each owner's constraint arms are the same
    // fail-closed walk over defs, symbols, types, and typed defaults.
    fn validate_type_param(
        tp: &radix_hir::HirTypeParam,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        checks.check_def(tp.def_id)?;
        checks.check_symbol(tp.name)?;
        match &tp.constraint {
            radix_hir::HirTypeParamConstraint::Any => {}
            radix_hir::HirTypeParamConstraint::OneOf(tys) => {
                for ty in tys {
                    checks.check_type(*ty)?;
                }
            }
            radix_hir::HirTypeParamConstraint::Implet(bounds) => {
                for bound in bounds {
                    checks.check_def(bound.contract)?;
                    for ty in &bound.args {
                        checks.check_type(*ty)?;
                    }
                }
            }
        }
        validate_type_param_default(tp.default.as_ref(), checks)
    }

    // Shared by item type params and by `Type::Func` / resolver function
    // signatures; every constraint arm is the same fail-closed walk over
    // witness types and contract bounds.
    fn validate_generic_param_constraint(
        constraint: &radix_types::TypeParamConstraint,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        match constraint {
            radix_types::TypeParamConstraint::Any => {}
            radix_types::TypeParamConstraint::OneOf(tys) => {
                for ty in tys {
                    checks.check_type(*ty)?;
                }
            }
            radix_types::TypeParamConstraint::Implet(bounds) => {
                for bound in bounds {
                    checks.check_def(bound.contract)?;
                    for ty in &bound.args {
                        checks.check_type(*ty)?;
                    }
                }
            }
        }
        Ok(())
    }

    // Recursive walkers for the HIR tree.
    fn validate_items(
        items: &[radix_hir::HirItem],
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        for item in items {
            checks.check_hir(item.id)?;
            checks.check_def(item.def_id)?;
            for ann in &item.annotations {
                validate_annotation(ann, checks)?;
            }
            validate_item_kind(&item.kind, checks)?;
        }
        Ok(())
    }

    fn validate_function(
        func: &radix_hir::HirFunction,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        checks.check_symbol(func.name)?;
        for tp in &func.type_params {
            validate_type_param(tp, checks)?;
        }
        for param in &func.params {
            checks.check_def(param.def_id)?;
            checks.check_symbol(param.name)?;
            checks.check_type(param.ty)?;
            if let Some(ref init) = param.default {
                validate_expr(init, checks)?;
            }
        }
        if let Some(ref cli_args) = func.cli_args {
            checks.check_def(cli_args.def_id)?;
            checks.check_symbol(cli_args.name)?;
            checks.check_type(cli_args.ty)?;
            if let Some(ref init) = cli_args.default {
                validate_expr(init, checks)?;
            }
        }
        if let Some(ty) = func.ret_ty {
            checks.check_type(ty)?;
        }
        if let Some(ty) = func.err_ty {
            checks.check_type(ty)?;
        }
        if let Some(ref body) = func.body {
            validate_block(body, checks)?;
        }
        if let Some(ref test_meta) = func.test {
            checks.check_symbol(test_meta.name)?;
            for sym in &test_meta.suite_path {
                checks.check_symbol(*sym)?;
            }
            for m in &test_meta.modifiers {
                match m {
                    radix_hir::HirTestModifier::Omitte(s)
                    | radix_hir::HirTestModifier::Futurum(s)
                    | radix_hir::HirTestModifier::Tag(s)
                    | radix_hir::HirTestModifier::SolumIn(s) => checks.check_symbol(*s)?,
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn validate_item_kind(
        kind: &HirItemKind,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        match kind {
            HirItemKind::Function(func) => {
                validate_function(func, checks)?;
            }
            HirItemKind::Struct(s) => {
                validate_struct_item(s, checks)?;
            }
            HirItemKind::Enum(e) => {
                validate_enum_item(e, checks)?;
            }
            HirItemKind::Interface(iface) => {
                validate_interface_item(iface, checks)?;
            }
            HirItemKind::TypeAlias(a) => {
                checks.check_symbol(a.name)?;
                for tp in &a.type_params {
                    validate_type_param(tp, checks)?;
                }
                checks.check_type(a.ty)?;
            }
            HirItemKind::Schema(schema) => {
                checks.check_symbol(schema.name)?;
                for col in &schema.columns {
                    checks.check_symbol(col.name)?;
                    checks.check_symbol(col.source)?;
                    checks.check_type(col.ty)?;
                }
            }
            HirItemKind::Constant(c) => {
                checks.check_symbol(c.name)?;
                if let Some(ty) = c.ty {
                    checks.check_type(ty)?;
                }
                validate_expr(&c.value, checks)?;
            }
            HirItemKind::Import(imp) => {
                checks.check_symbol(imp.path)?;
                for item in &imp.items {
                    checks.check_def(item.def_id)?;
                    checks.check_symbol(item.name)?;
                    if let Some(a) = item.alias {
                        checks.check_symbol(a)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_struct_item(
        item: &radix_hir::HirStruct,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        checks.check_symbol(item.name)?;
        for tp in &item.type_params {
            validate_type_param(tp, checks)?;
        }
        for field in &item.fields {
            checks.check_def(field.def_id)?;
            checks.check_symbol(field.name)?;
            checks.check_type(field.ty)?;
            for ann in &field.annotations {
                validate_annotation(ann, checks)?;
            }
            if let Some(ref init) = field.init {
                validate_expr(init, checks)?;
            }
        }
        for method in &item.methods {
            checks.check_def(method.def_id)?;
            for ann in &method.annotations {
                validate_annotation(ann, checks)?;
            }
            validate_function(&method.func, checks)?;
        }
        for contract in &item.implements {
            checks.check_def(contract.contract)?;
            for arg in &contract.args {
                checks.check_type(*arg)?;
            }
        }
        Ok(())
    }

    fn validate_enum_item(
        item: &radix_hir::HirEnum,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        checks.check_symbol(item.name)?;
        for tp in &item.type_params {
            validate_type_param(tp, checks)?;
        }
        for field in &item.commune_fields {
            checks.check_symbol(field.name)?;
            checks.check_type(field.ty)?;
        }
        for variant in &item.variants {
            checks.check_def(variant.def_id)?;
            checks.check_symbol(variant.name)?;
            for field in &variant.fields {
                checks.check_symbol(field.name)?;
                checks.check_type(field.ty)?;
            }
        }
        Ok(())
    }

    fn validate_interface_item(
        item: &radix_hir::HirInterface,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        checks.check_symbol(item.name)?;
        for tp in &item.type_params {
            validate_type_param(tp, checks)?;
        }
        for method in &item.methods {
            for a in &method.availability {
                let radix_hir::HirAvailability::Nondum { target, reason } = a;
                if let Some(t) = target {
                    checks.check_symbol(*t)?;
                }
                if let Some(r) = reason {
                    checks.check_symbol(*r)?;
                }
            }
            checks.check_symbol(method.name)?;
            for param in &method.params {
                checks.check_def(param.def_id)?;
                checks.check_symbol(param.name)?;
                checks.check_type(param.ty)?;
                if let Some(ref init) = param.default {
                    validate_expr(init, checks)?;
                }
            }
            if let Some(ty) = method.ret_ty {
                checks.check_type(ty)?;
            }
            if let Some(ty) = method.err_ty {
                checks.check_type(ty)?;
            }
        }
        Ok(())
    }

    fn validate_block(
        block: &radix_hir::HirBlock,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        for stmt in &block.statements {
            checks.check_hir(stmt.id)?;
            match &stmt.kind {
                HirStatementKind::Local(local) => {
                    checks.check_def(local.def_id)?;
                    checks.check_symbol(local.name)?;
                    if let Some(site) = &local.await_store
                        && let Some(slot) = site.slot
                    {
                        checks.check_symbol(slot)?;
                    }
                    if let Some(ty) = local.ty {
                        checks.check_type(ty)?;
                    }
                    if let Some(ref init) = local.init {
                        validate_expr(init, checks)?;
                    }
                }
                HirStatementKind::Expr(expr)
                | HirStatementKind::Redde(Some(expr))
                | HirStatementKind::ReddeFilum(Some(expr)) => {
                    validate_expr(expr, checks)?;
                }
                HirStatementKind::IncDec(inc) => {
                    validate_expr(&inc.target, checks)?;
                }
                HirStatementKind::Custodi(cust) => {
                    for clause in &cust.clauses {
                        validate_expr(&clause.cond, checks)?;
                        validate_block(&clause.body, checks)?;
                    }
                }
                HirStatementKind::Rumpe
                | HirStatementKind::Perge
                | HirStatementKind::Tacet
                | HirStatementKind::Redde(None)
                | HirStatementKind::ReddeFilum(None) => {}
            }
        }
        if let Some(ref expr) = block.expr {
            validate_expr(expr, checks)?;
        }
        Ok(())
    }

    fn validate_expr(
        expr: &radix_hir::HirExpression,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        checks.check_hir(expr.id)?;
        if let Some(ty) = expr.ty {
            checks.check_type(ty)?;
        }
        match &expr.kind {
            HirExpressionKind::Reduction {
                source,
                coordinates,
                group,
                filum,
                binding,
                body,
                identity,
                ..
            } => {
                validate_expr(source, checks)?;
                for (id, name) in coordinates {
                    checks.check_def(*id)?;
                    checks.check_symbol(*name)?;
                }
                if let Some((id, name)) = filum {
                    checks.check_def(*id)?;
                    checks.check_symbol(*name)?;
                }
                if let Some((id, name)) = binding {
                    checks.check_def(*id)?;
                    checks.check_symbol(*name)?;
                }
                if let Some(group) = group {
                    validate_expr(group, checks)?;
                }
                if let Some(body) = body {
                    validate_block(body, checks)?;
                }
                if let Some(identity) = identity {
                    validate_expr(identity, checks)?;
                }
            }
            HirExpressionKind::Scan {
                source,
                coordinates,
                group,
                filum,
                binding,
                body,
                ..
            } => {
                validate_expr(source, checks)?;
                for (id, name) in coordinates {
                    checks.check_def(*id)?;
                    checks.check_symbol(*name)?;
                }
                checks.check_def(filum.0)?;
                checks.check_symbol(filum.1)?;
                checks.check_def(binding.0)?;
                checks.check_symbol(binding.1)?;
                if let Some(group) = group {
                    validate_expr(group, checks)?;
                }
                validate_block(body, checks)?;
            }
            HirExpressionKind::Sort {
                source,
                binding,
                body,
                ..
            } => {
                validate_expr(source, checks)?;
                if let Some((id, name)) = binding {
                    checks.check_def(*id)?;
                    checks.check_symbol(*name)?;
                }
                if let Some(body) = body {
                    validate_block(body, checks)?;
                }
            }
            HirExpressionKind::Path(def_id) => checks.check_def(*def_id)?,
            HirExpressionKind::Literal(lit) => match lit {
                radix_hir::HirLiteral::String(s)
                | radix_hir::HirLiteral::Ascii(s)
                | radix_hir::HirLiteral::Octeti(s)
                | radix_hir::HirLiteral::Regex(s, None) => checks.check_symbol(*s)?,
                radix_hir::HirLiteral::Regex(s, Some(f)) => {
                    checks.check_symbol(*s)?;
                    checks.check_symbol(*f)?;
                }
                _ => {}
            },
            HirExpressionKind::Binary(_, lhs, rhs) | HirExpressionKind::Assign(lhs, rhs) => {
                validate_expr(lhs, checks)?;
                validate_expr(rhs, checks)?;
            }
            HirExpressionKind::Unary(_, inner)
            | HirExpressionKind::Cede(inner)
            | HirExpressionKind::Reddet(inner)
            | HirExpressionKind::Tacebit(inner)
            | HirExpressionKind::Yield(inner)
            | HirExpressionKind::Ref(_, inner)
            | HirExpressionKind::Deref(inner)
            | HirExpressionKind::Praefixum(inner) => {
                validate_expr(inner, checks)?;
            }
            HirExpressionKind::Call(callee, type_args, args, _) => {
                validate_expr(callee, checks)?;
                for ty in type_args {
                    checks.check_type(*ty)?;
                }
                for arg in args {
                    if let Some(n) = arg.name {
                        checks.check_symbol(n)?;
                    }
                    validate_expr(&arg.expr, checks)?;
                }
            }
            HirExpressionKind::CopyInto { target, value } => {
                validate_expr(target, checks)?;
                validate_expr(value, checks)?;
            }
            HirExpressionKind::SinkInto { sink, item } => {
                validate_expr(sink, checks)?;
                validate_expr(item, checks)?;
            }
            HirExpressionKind::Gradient {
                call,
                selected_arguments,
            } => {
                validate_expr(call, checks)?;
                for selector in selected_arguments {
                    validate_expr(selector, checks)?;
                }
            }
            HirExpressionKind::MethodCall(receiver, method, type_args, args, _) => {
                validate_expr(receiver, checks)?;
                checks.check_symbol(*method)?;
                for ty in type_args {
                    checks.check_type(*ty)?;
                }
                for arg in args {
                    if let Some(n) = arg.name {
                        checks.check_symbol(n)?;
                    }
                    validate_expr(&arg.expr, checks)?;
                }
            }
            HirExpressionKind::Field(target, field) => {
                validate_expr(target, checks)?;
                checks.check_symbol(*field)?;
            }
            HirExpressionKind::Index(target, index) => {
                validate_expr(target, checks)?;
                validate_expr(index, checks)?;
            }
            HirExpressionKind::OptionalChain(base, chain) => {
                validate_expr(base, checks)?;
                match chain {
                    radix_hir::HirOptionalChainKind::Member(s) => checks.check_symbol(*s)?,
                    radix_hir::HirOptionalChainKind::Index(e) => {
                        validate_expr(e, checks)?;
                    }
                    radix_hir::HirOptionalChainKind::Call(args) => {
                        for arg in args {
                            if let Some(n) = arg.name {
                                checks.check_symbol(n)?;
                            }
                            validate_expr(&arg.expr, checks)?;
                        }
                    }
                }
            }
            HirExpressionKind::Marked { expr, .. } => validate_expr(expr, checks)?,
            HirExpressionKind::NonNull(base, chain) => {
                validate_expr(base, checks)?;
                match chain {
                    radix_hir::HirNonNullKind::Value => {}
                    radix_hir::HirNonNullKind::Member(s) => checks.check_symbol(*s)?,
                    radix_hir::HirNonNullKind::Index(e) => {
                        validate_expr(e, checks)?;
                    }
                    radix_hir::HirNonNullKind::Call(args) => {
                        for arg in args {
                            if let Some(n) = arg.name {
                                checks.check_symbol(n)?;
                            }
                            validate_expr(&arg.expr, checks)?;
                        }
                    }
                }
            }
            HirExpressionKind::Block(block) => {
                validate_block(block, checks)?;
            }
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => {
                validate_expr(cond, checks)?;
                validate_block(then_block, checks)?;
                if let Some(cape) = then_catch {
                    checks.check_def(cape.binding_def_id)?;
                    checks.check_symbol(cape.binding_name)?;
                    if let Some(ty) = cape.binding_ty {
                        checks.check_type(ty)?;
                    }
                    validate_block(&cape.body, checks)?;
                }
                if let Some(else_block) = else_block {
                    validate_block(else_block, checks)?;
                }
            }
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => {
                for s in scrutinees {
                    validate_expr(s, checks)?;
                }
                for arm in arms {
                    for pat in &arm.patterns {
                        validate_pattern(pat, checks)?;
                    }
                    validate_expr(&arm.body, checks)?;
                }
            }
            HirExpressionKind::Loop(body) => {
                validate_block(body, checks)?;
            }
            HirExpressionKind::Dum(cond, body) => {
                validate_expr(cond, checks)?;
                validate_block(body, checks)?;
            }
            HirExpressionKind::Itera {
                coordinates,
                binding: def_id,
                binding_name: sym,
                iter: iterable,
                body,
                ..
            } => {
                for (coordinate_def_id, coordinate_name) in coordinates {
                    checks.check_def(*coordinate_def_id)?;
                    checks.check_symbol(*coordinate_name)?;
                }
                checks.check_def(*def_id)?;
                checks.check_symbol(*sym)?;
                validate_expr(iterable, checks)?;
                validate_block(body, checks)?;
            }
            HirExpressionKind::IteraLockstep {
                sources,
                coordinates,
                bindings,
                body,
                ..
            } => {
                for (coordinate_def_id, coordinate_name) in coordinates {
                    checks.check_def(*coordinate_def_id)?;
                    checks.check_symbol(*coordinate_name)?;
                }
                for (binding_def_id, binding_name) in bindings {
                    checks.check_def(*binding_def_id)?;
                    checks.check_symbol(*binding_name)?;
                }
                for source in sources {
                    validate_expr(source, checks)?;
                }
                validate_block(body, checks)?;
            }
            // NR-5 (nary-range-walk): validate product binder defs/symbols
            // and ranges/body; dangling binder indices fail closed through
            // the shared DanglingIndex path.
            HirExpressionKind::IteraRange {
                ranges,
                bindings,
                body,
                ..
            } => {
                for (binding_def_id, binding_name) in bindings {
                    checks.check_def(*binding_def_id)?;
                    checks.check_symbol(*binding_name)?;
                }
                for range in ranges {
                    validate_expr(range, checks)?;
                }
                validate_block(body, checks)?;
            }
            HirExpressionKind::Intervallum {
                start, end, step, ..
            } => {
                validate_expr(start, checks)?;
                validate_expr(end, checks)?;
                if let Some(s) = step {
                    validate_expr(s, checks)?;
                }
            }
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
                ..
            } => {
                validate_expr(target, checks)?;
                validate_expr(source, checks)?;
                if let Some(recovery) = recovery {
                    validate_expr(recovery, checks)?;
                }
            }
            HirExpressionKind::Array(elements) => {
                for elem in elements {
                    match elem {
                        radix_hir::HirArrayElement::Expr(e)
                        | radix_hir::HirArrayElement::Spread(e) => {
                            validate_expr(e, checks)?;
                        }
                        radix_hir::HirArrayElement::Infer(_) => {}
                    }
                }
            }
            HirExpressionKind::Struct(def_id, fields) => {
                checks.check_def(*def_id)?;
                for (sym, e) in fields {
                    checks.check_symbol(*sym)?;
                    validate_expr(e, checks)?;
                }
            }
            HirExpressionKind::Tuple(elements, type_args) => {
                for e in elements {
                    validate_expr(e, checks)?;
                }
                if let Some(tas) = type_args {
                    for ty in tas {
                        checks.check_type(*ty)?;
                    }
                }
            }
            HirExpressionKind::Scribe(_, args) => {
                for a in args {
                    validate_expr(a, checks)?;
                }
            }
            HirExpressionKind::Scriptum(template, args) => {
                checks.check_symbol(*template)?;
                for a in args {
                    validate_expr(a, checks)?;
                }
            }
            HirExpressionKind::Adfirma(expr, msg) => {
                validate_expr(expr, checks)?;
                if let Some(m) = msg {
                    validate_expr(m, checks)?;
                }
            }
            HirExpressionKind::Panic(expr) | HirExpressionKind::Throw(expr) => {
                validate_expr(expr, checks)?;
            }
            HirExpressionKind::Handled { body, catch } => {
                validate_block(body, checks)?;
                checks.check_def(catch.binding_def_id)?;
                checks.check_symbol(catch.binding_name)?;
                if let Some(ty) = catch.binding_ty {
                    checks.check_type(ty)?;
                }
                validate_block(&catch.body, checks)?;
            }
            HirExpressionKind::Trap(body) => {
                validate_block(body, checks)?;
            }
            HirExpressionKind::CallDefault {
                body,
                default,
                err_ty,
            } => {
                validate_block(body, checks)?;
                validate_expr(default, checks)?;
                if let Some(ty) = err_ty {
                    checks.check_type(*ty)?;
                }
            }
            HirExpressionKind::Clausura(params, ret_ty, err_ty, body) => {
                for param in params {
                    checks.check_def(param.def_id)?;
                    checks.check_symbol(param.name)?;
                    checks.check_type(param.ty)?;
                    if let Some(ref init) = param.default {
                        validate_expr(init, checks)?;
                    }
                }
                if let Some(ty) = ret_ty {
                    checks.check_type(*ty)?;
                }
                if let Some(ty) = err_ty {
                    checks.check_type(*ty)?;
                }
                validate_expr(body, checks)?;
            }
            HirExpressionKind::Verte {
                source,
                target,
                entries,
            } => {
                validate_expr(source, checks)?;
                checks.check_type(*target)?;
                if let Some(entries) = entries {
                    for entry in entries {
                        match &entry.key {
                            radix_hir::HirObjectKey::Ident(s)
                            | radix_hir::HirObjectKey::String(s) => checks.check_symbol(*s)?,
                            radix_hir::HirObjectKey::Computed(e)
                            | radix_hir::HirObjectKey::Spread(e) => {
                                validate_expr(e, checks)?;
                            }
                        }
                        if let Some(ref v) = entry.value {
                            validate_expr(v, checks)?;
                        }
                    }
                }
            }
            HirExpressionKind::Conversio {
                route,
                source,
                target,
                params,
                recovery,
                implicit,
            } => {
                match route {
                    Some(radix_hir::HirConversioRoute::Function { def_id, .. }) => {
                        checks.check_def(*def_id)?;
                    }
                    Some(radix_hir::HirConversioRoute::Ordo { members, .. }) => {
                        for member in members {
                            checks.check_def(member.variant)?;
                            checks.check_symbol(member.name)?;
                        }
                    }
                    None => {}
                }
                if let Some(site) = implicit
                    && let Some(slot) = site.slot
                {
                    checks.check_symbol(slot)?;
                }
                validate_expr(source, checks)?;
                match target {
                    radix_hir::HirConversioTarget::Type(ty) => checks.check_type(*ty)?,
                    radix_hir::HirConversioTarget::Intervallum(e) => {
                        validate_expr(e, checks)?;
                    }
                }
                for sym in params {
                    checks.check_symbol(*sym)?;
                }
                if let Some(r) = recovery {
                    validate_expr(r, checks)?;
                }
            }
            HirExpressionKind::Widen { operand, from, to } => {
                validate_expr(operand, checks)?;
                checks.check_type(*from)?;
                checks.check_type(*to)?;
            }
            HirExpressionKind::Ad { route, opener } => {
                checks.check_symbol(*route)?;
                if let Some(o) = opener {
                    validate_expr(o, checks)?;
                }
            }
            HirExpressionKind::TypeCheck { expr, ty, .. } => {
                validate_expr(expr, checks)?;
                checks.check_type(*ty)?;
            }
            HirExpressionKind::Format { value, spec } => {
                checks.check_symbol(*spec)?;
                validate_expr(value, checks)?;
            }
            HirExpressionKind::Insere(path) => {
                checks.check_symbol(*path)?;
            }
            HirExpressionKind::Vacua
            | HirExpressionKind::ReadLine(_)
            | HirExpressionKind::Error => {}
        }
        Ok(())
    }

    fn validate_pattern(
        pat: &radix_hir::HirPattern,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        match pat {
            radix_hir::HirPattern::Binding(def_id, sym) => {
                checks.check_def(*def_id)?;
                checks.check_symbol(*sym)?;
            }
            radix_hir::HirPattern::Alias(def_id, sym, inner) => {
                checks.check_def(*def_id)?;
                checks.check_symbol(*sym)?;
                validate_pattern(inner, checks)?;
            }
            radix_hir::HirPattern::Variant(def_id, subpatterns) => {
                checks.check_def(*def_id)?;
                for sp in subpatterns {
                    validate_pattern(sp, checks)?;
                }
            }
            radix_hir::HirPattern::Literal(lit) | radix_hir::HirPattern::NegatedLiteral(lit) => {
                match lit {
                    radix_hir::HirLiteral::String(s)
                    | radix_hir::HirLiteral::Ascii(s)
                    | radix_hir::HirLiteral::Octeti(s)
                    | radix_hir::HirLiteral::Regex(s, None) => checks.check_symbol(*s)?,
                    radix_hir::HirLiteral::Regex(s, Some(f)) => {
                        checks.check_symbol(*s)?;
                        checks.check_symbol(*f)?;
                    }
                    _ => {}
                }
            }
            radix_hir::HirPattern::TypeTest { binding, .. } => {
                if let Some((def_id, sym)) = binding {
                    checks.check_def(*def_id)?;
                    checks.check_symbol(*sym)?;
                }
            }
            radix_hir::HirPattern::Or(patterns) => {
                for pattern in patterns {
                    validate_pattern(pattern, checks)?;
                }
            }
            radix_hir::HirPattern::Wildcard => {}
        }
        Ok(())
    }

    fn validate_annotation(
        ann: &radix_hir::HirAnnotation,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        checks.check_symbol(ann.family)?;
        if let Some(def_id) = ann.contract_id {
            checks.check_def(def_id)?;
        }
        for field in &ann.fields {
            checks.check_symbol(field.name)?;
            match &field.value {
                radix_hir::HirAnnotationValue::Expr(e) => {
                    validate_expr(e, checks)?;
                }
                radix_hir::HirAnnotationValue::Type(ty) => checks.check_type(*ty)?,
                radix_hir::HirAnnotationValue::Symbol(s)
                | radix_hir::HirAnnotationValue::String(s) => checks.check_symbol(*s)?,
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
        checks: &IntegrityChecks<'_>,
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
                Some(radix_hir::HirOwnerRef::Hir(id)) => checks.check_hir(*id)?,
                Some(radix_hir::HirOwnerRef::Def(def_id)) => checks.check_def(*def_id)?,
                None => {}
            }
        }

        let mut await_bindings = AwaitBindingCollector::default();
        radix_hir::visit::walk_program(&mut await_bindings, &artifact.hir);
        let mut await_assignment_defs = FxHashSet::default();
        for def_id in &presentation.await_assignment_bindings {
            checks.check_def(*def_id)?;
            if !await_bindings.defs.contains(def_id) {
                return Err(HirArtifactError::InvalidPresentation {
                    issue: "await_assignment_binding_not_awaited",
                    anchor: def_id.0,
                });
            }
            if !await_assignment_defs.insert(*def_id) {
                return Err(HirArtifactError::InvalidPresentation {
                    issue: "duplicate_await_assignment_binding",
                    anchor: def_id.0,
                });
            }
        }
        for def_id in &presentation.compile_time_locals {
            checks.check_def(*def_id)?;
        }
        for binding in &presentation.function_modifier_bindings {
            checks.check_def(binding.local)?;
        }
        validate_presentation_attachments(presentation, &owner_ids, checks)?;

        validate_presentation_structural_ends(presentation)?;

        let check_symbol = |sym| checks.check_symbol(sym);
        let check_hir = |hir_id| checks.check_hir(hir_id);
        let check_def = |def_id| checks.check_def(def_id);
        let check_type = |ty| checks.check_type(ty);
        let check_index = |index| checks.check_index(index);
        validate_authored_type_records(
            artifact,
            presentation,
            &check_symbol,
            &check_hir,
            &check_def,
            &check_type,
            &check_index,
        )?;
        validate_conversation_constructor(artifact, &check_def)?;

        // Each by-label `iuncta` read annotates exactly one rewritten index
        // node. A dangling or duplicated id is a producer defect: emission
        // would otherwise silently keep the numeric spelling.
        let mut label_access_ids: FxHashSet<radix_hir::HirId> = FxHashSet::default();
        for access in &presentation.tuple_label_accesses {
            checks.check_symbol(access.label)?;
            checks.check_hir(access.expr_id)?;
            if !label_access_ids.insert(access.expr_id) {
                return Err(HirArtifactError::InvalidPresentation {
                    issue: "duplicate_tuple_label_access",
                    anchor: access.expr_id.0,
                });
            }
        }

        // Each template application records its authored holes at most once.
        let mut template_hole_ids: FxHashSet<radix_hir::HirId> = FxHashSet::default();
        for holes in &presentation.template_holes {
            checks.check_symbol(holes.template)?;
            checks.check_hir(holes.expr)?;
            for (label, _) in &holes.labeled_actuals {
                checks.check_symbol(*label)?;
            }
            if !template_hole_ids.insert(holes.expr) {
                return Err(HirArtifactError::InvalidPresentation {
                    issue: "duplicate_template_holes",
                    anchor: holes.expr.0,
                });
            }
        }

        Ok(())
    }

    // ── Type arena walk helper ──────────────────────────────────────────

    fn validate_type(
        ty: &radix_types::Type,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        match ty {
            radix_types::Type::Primitive(_)
            | radix_types::Type::TensorEdgePolicy(_)
            | radix_types::Type::SizedNumeric(_, _)
            | radix_types::Type::ModularWord(_)
            | radix_types::Type::Bits(_)
            | radix_types::Type::SaturatingWord(_)
            | radix_types::Type::RangedInt { .. }
            | radix_types::Type::PolicyFloat(_, _)
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
            | radix_types::Type::Intervallum(t, _)
            | radix_types::Type::Option(t)
            | radix_types::Type::Ref(_, t) => {
                checks.check_type(*t)?;
            }
            radix_types::Type::PromissumFailable(t, e) | radix_types::Type::AsyncCursor(t, e) => {
                checks.check_type(*t)?;
                checks.check_type(*e)?;
            }
            radix_types::Type::Map(k, v) | radix_types::Type::Ledger(k, v) => {
                checks.check_type(*k)?;
                checks.check_type(*v)?;
            }
            radix_types::Type::Record(fields) => {
                for (&sym, &tid) in fields {
                    checks.check_symbol(sym)?;
                    checks.check_type(tid)?;
                }
            }
            radix_types::Type::RangedArray { element, .. } => {
                checks.check_type(*element)?;
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
                checks.check_type(*t)?;
                checks.check_index(*i)?;
            }
            radix_types::Type::BoundedTextus { capacity }
            | radix_types::Type::BoundedAscii { capacity }
            | radix_types::Type::BoundedOcteti { capacity } => {
                checks.check_index(*capacity)?;
            }
            radix_types::Type::Struct(d)
            | radix_types::Type::Enum(d)
            | radix_types::Type::Interface(d) => {
                checks.check_def(*d)?;
            }
            radix_types::Type::Alias(d, t) => {
                checks.check_def(*d)?;
                checks.check_type(*t)?;
            }
            radix_types::Type::Func(sig) => {
                checks.check_type(sig.ret)?;
                if let Some(et) = sig.err {
                    checks.check_type(et)?;
                }
                for param in &sig.type_params {
                    validate_generic_param_constraint(&param.constraint, checks)?;
                    if let Some(radix_types::SemanticGenericParamDefault::Typus(ty)) = param.default
                    {
                        checks.check_type(ty)?;
                    }
                    checks.check_symbol(param.name)?;
                }
                for pt in &sig.params {
                    checks.check_type(pt.ty)?;
                }
            }
            radix_types::Type::Param(name) => {
                checks.check_symbol(*name)?;
            }
            radix_types::Type::Applied(base, args) => {
                checks.check_type(*base)?;
                for arg in args {
                    match arg {
                        radix_types::AppliedArg::Type(id) => checks.check_type(*id)?,
                        radix_types::AppliedArg::Size(id) => checks.check_index(*id)?,
                    }
                }
            }
            radix_types::Type::Union(members)
            | radix_types::Type::Intersection(members)
            | radix_types::Type::Tuple(members) => {
                for m in members {
                    checks.check_type(*m)?;
                }
            }
        }
        Ok(())
    }

    // ── Resolver snapshot walk helper ───────────────────────────────────

    /// Field initializers cross the interface as definition-free expressions
    /// (literals, operators, aggregates, `scriptum`, `verte`). Their `HirId`s
    /// belong to the exporting unit, so only the arena references they carry
    /// are checked, and any other form is refused.
    fn validate_field_init(
        expr: &radix_hir::HirExpression,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        use radix_hir::{HirArrayElement, HirLiteral, HirObjectKey};
        if let Some(ty) = expr.ty {
            checks.check_type(ty)?;
        }
        match &expr.kind {
            HirExpressionKind::Literal(literal) => match literal {
                HirLiteral::String(sym) | HirLiteral::Ascii(sym) | HirLiteral::Octeti(sym) => {
                    checks.check_symbol(*sym)
                }
                HirLiteral::FloatText { text, .. } => checks.check_symbol(*text),
                HirLiteral::Regex(pattern, flags) => {
                    checks.check_symbol(*pattern)?;
                    flags.map_or(Ok(()), |flags| checks.check_symbol(flags))
                }
                _ => Ok(()),
            },
            HirExpressionKind::Vacua => Ok(()),
            HirExpressionKind::Unary(_, inner) => validate_field_init(inner, checks),
            HirExpressionKind::Binary(_, left, right) => {
                validate_field_init(left, checks)?;
                validate_field_init(right, checks)
            }
            HirExpressionKind::Array(items) => {
                for item in items {
                    match item {
                        HirArrayElement::Expr(inner) | HirArrayElement::Spread(inner) => {
                            validate_field_init(inner, checks)?;
                        }
                        HirArrayElement::Infer(_) => return Err(unsupported_field_init()),
                    }
                }
                Ok(())
            }
            HirExpressionKind::Tuple(items, arguments) => {
                for item in items {
                    validate_field_init(item, checks)?;
                }
                for argument in arguments.iter().flatten() {
                    checks.check_type(*argument)?;
                }
                Ok(())
            }
            HirExpressionKind::Scriptum(template, items) => {
                checks.check_symbol(*template)?;
                for item in items {
                    validate_field_init(item, checks)?;
                }
                Ok(())
            }
            HirExpressionKind::Verte {
                source,
                target,
                entries,
            } => {
                checks.check_type(*target)?;
                validate_field_init(source, checks)?;
                for entry in entries.iter().flatten() {
                    match &entry.key {
                        HirObjectKey::Ident(name) | HirObjectKey::String(name) => {
                            checks.check_symbol(*name)?;
                        }
                        HirObjectKey::Computed(inner) | HirObjectKey::Spread(inner) => {
                            validate_field_init(inner, checks)?;
                        }
                    }
                    if let Some(value) = &entry.value {
                        validate_field_init(value, checks)?;
                    }
                }
                Ok(())
            }
            _ => Err(unsupported_field_init()),
        }
    }

    fn unsupported_field_init() -> HirArtifactError {
        HirArtifactError::MalformedSnapshot {
            kind: "file interface field initializer",
            detail: "expression form is outside the definition-free initializer surface".to_owned(),
        }
    }

    fn validate_file_interface_snapshot(
        iface: &crate::artifact::FileInterfaceSnapshot,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        use crate::artifact::FileExportSnapshot;

        for (sym, export) in &iface.exports {
            checks.check_symbol(*sym)?;
            match export {
                FileExportSnapshot::Function(sig) => {
                    checks.check_type(sig.ret)?;
                    if let Some(et) = sig.err {
                        checks.check_type(et)?;
                    }
                    for param in &sig.type_params {
                        checks.check_symbol(param.name)?;
                        validate_generic_param_constraint(&param.constraint, checks)?;
                        if let Some(radix_types::SemanticGenericParamDefault::Typus(ty)) =
                            param.default
                        {
                            checks.check_type(ty)?;
                        }
                    }
                    for pt in &sig.params {
                        checks.check_type(pt.ty)?;
                    }
                }
                FileExportSnapshot::Type { ty, def_id } => {
                    checks.check_type(*ty)?;
                    if let Some(d) = def_id {
                        checks.check_def(*d)?;
                    }
                }
                FileExportSnapshot::Struct {
                    def_id,
                    ty,
                    fields,
                    methods,
                    implements,
                    ..
                } => {
                    checks.check_def(*def_id)?;
                    checks.check_type(*ty)?;
                    for field in fields {
                        checks.check_symbol(field.name)?;
                        checks.check_type(field.ty)?;
                        if let Some(init) = &field.init {
                            validate_field_init(init, checks)?;
                        }
                    }
                    for contract in implements {
                        for arg in &contract.args {
                            checks.check_type(*arg)?;
                        }
                    }
                    for method in methods {
                        checks.check_def(method.def_id)?;
                        checks.check_symbol(method.name)?;
                        checks.check_symbol(method.surface_name)?;
                        checks.check_type(method.sig.ret)?;
                        if let Some(error_ty) = method.sig.err {
                            checks.check_type(error_ty)?;
                        }
                        for param in &method.sig.type_params {
                            checks.check_symbol(param.name)?;
                            validate_generic_param_constraint(&param.constraint, checks)?;
                            if let Some(radix_types::SemanticGenericParamDefault::Typus(ty)) =
                                param.default
                            {
                                checks.check_type(ty)?;
                            }
                        }
                        for param in &method.sig.params {
                            checks.check_type(param.ty)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_nested_providers(
        units: &std::collections::BTreeMap<String, crate::artifact::ProviderCarriageEntryWire>,
    ) -> Result<(), HirArtifactError> {
        use crate::artifact::ProviderCarriageEntryWire;

        for entry in units.values() {
            let ProviderCarriageEntryWire::Unit(provider) = entry else {
                continue;
            };
            let provider = provider.as_ref();
            let libraries = radix_hir::LibraryRegistry::default();
            let def_registry =
                collect_def_registry_for_parts(&provider.hir, &libraries, &provider.resolver);
            let hir_registry = collect_hir_registry_for_hir(&provider.hir);
            let checks = IntegrityChecks {
                def_registry: &def_registry,
                hir_registry: &hir_registry,
                type_count: provider.types.type_count(),
                interner_len: provider.interner.len(),
                index_count: provider.types.indices().len(),
            };
            validate_interner(&provider.interner)?;
            validate_type_chains(&provider.types)?;
            validate_items(&provider.hir.items, &checks)?;
            if let Some(entry) = &provider.hir.entry {
                validate_block(entry, &checks)?;
            }
            for annotation in &provider.hir.entry_annotations {
                validate_annotation(annotation, &checks)?;
            }
            if let Some(name) = provider.hir.entry_args_name {
                checks.check_symbol(name)?;
            }
            for ty in provider.types.types() {
                validate_type(ty, &checks)?;
            }
            validate_indices(&provider.types, &checks)?;
            validate_resolver(&provider.resolver, &checks)?;
            validate_generic_calls(&provider.resolved_generic_calls, &checks)?;
            validate_tensor_edge_policy_ids(&provider.tensor_edge_policies, &checks)?;
            validate_shape_obligation_table(&provider.shape_obligations, &checks)?;
            validate_nested_providers(&provider.nested)?;
        }
        Ok(())
    }

    fn validate_resolver(
        resolver: &crate::artifact::ResolverSnapshot,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        for (sym, _) in &resolver.namespace_exports {
            checks.check_symbol(*sym)?;
        }
        for (sym, iface) in &resolver.file_interfaces {
            checks.check_symbol(*sym)?;
            validate_file_interface_snapshot(iface, checks)?;
        }
        for sym in &resolver.used_namespaces {
            checks.check_symbol(*sym)?;
        }
        for (def_id, sym) in &resolver.module_bindings {
            checks.check_def(*def_id)?;
            checks.check_symbol(*sym)?;
        }
        for ((_, sym), ty) in &resolver.imported_nominal_types {
            checks.check_symbol(*sym)?;
            checks.check_type(*ty)?;
        }
        for row in &resolver.conversion_rows {
            if let Some(fn_def_id) = row.fn_def_id {
                checks.check_def(fn_def_id)?;
            }
            checks.check_type(row.source)?;
            checks.check_type(row.destination)?;
        }

        Ok(())
    }

    fn validate_indices(
        types: &radix_types::TypeTableSnapshot,
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        for index in types.indices() {
            match index {
                radix_types::IndexExpr::Tuple(children) => {
                    for child in children {
                        checks.check_index(*child)?;
                    }
                }
                radix_types::IndexExpr::Param(symbol) => checks.check_symbol(*symbol)?,
                radix_types::IndexExpr::Affine { base, .. } => checks.check_index(*base)?,
                radix_types::IndexExpr::Divide { numerator, .. } => {
                    checks.check_index(*numerator)?;
                }
                radix_types::IndexExpr::Runtime(source) => match source {
                    radix_types::RuntimeExtentId::ScalarBinding(binding)
                    | radix_types::RuntimeExtentId::TensorBindingAxis { binding, .. } => {
                        checks.check_def(*binding)?;
                    }
                    radix_types::RuntimeExtentId::TensorFieldAxis {
                        binding, fields, ..
                    } => {
                        checks.check_def(*binding)?;
                        for field in fields {
                            checks.check_symbol(*field)?;
                        }
                    }
                    radix_types::RuntimeExtentId::FunctionParameterFieldAxis { fields, .. }
                    | radix_types::RuntimeExtentId::CallArgumentFieldAxis { fields, .. } => {
                        for field in fields {
                            checks.check_symbol(*field)?;
                        }
                    }
                    radix_types::RuntimeExtentId::ExpressionFieldAxis {
                        expression,
                        fields,
                        ..
                    } => {
                        checks.check_hir(radix_hir::HirId(*expression))?;
                        for field in fields {
                            checks.check_symbol(*field)?;
                        }
                    }
                    radix_types::RuntimeExtentId::ScalarExpression { expression }
                    | radix_types::RuntimeExtentId::ExpressionAxis { expression, .. } => {
                        checks.check_hir(radix_hir::HirId(*expression))?;
                    }
                    radix_types::RuntimeExtentId::ScalarParameter { .. }
                    | radix_types::RuntimeExtentId::CallArgumentAxis { .. }
                    | radix_types::RuntimeExtentId::FunctionParameterAxis { .. } => {}
                },
                radix_types::IndexExpr::Literal(_)
                | radix_types::IndexExpr::Infer(_)
                | radix_types::IndexExpr::Unspecified => {}
            }
        }

        Ok(())
    }

    fn validate_generic_calls(
        calls: &[radix_types::ResolvedGenericCall],
        checks: &IntegrityChecks<'_>,
    ) -> Result<(), HirArtifactError> {
        for row in calls {
            for binding in &row.bindings {
                checks.check_symbol(binding.name)?;
                match binding.witness {
                    radix_types::GenericWitness::Typus(ty) => checks.check_type(ty)?,
                    radix_types::GenericWitness::Magnitudo(xi) => checks.check_index(xi)?,
                }
            }
        }

        Ok(())
    }

    // ――― DefId registry (S2: collect every definition-site DefId) ―――――――
    let def_registry = collect_def_registry(artifact);

    // HirId registry: collect every HirId-bearing node in the tree (items +
    // statements + expressions) into a membership-test set. HirIds are
    // allocated by a monotonic counter in the lowerer, but error recovery
    // discards partially-constructed nodes, leaving gaps in the ID space.
    // A count-based bound would false-positive on those gaps; only a
    // membership test is correct.
    let hir_registry = collect_hir_registry(artifact);

    let checks = IntegrityChecks {
        def_registry: &def_registry,
        hir_registry: &hir_registry,
        type_count,
        interner_len,
        index_count: artifact.types.indices().len(),
    };

    // ── Top-level validation ───────────────────────────────────────────

    validate_interner(&artifact.interner)?;
    validate_type_chains(&artifact.types)?;

    // 1. HirModule items and entry.
    validate_items(&artifact.hir.items, &checks)?;
    if let Some(ref entry) = artifact.hir.entry {
        validate_block(entry, &checks)?;
    }
    for ann in &artifact.hir.entry_annotations {
        validate_annotation(ann, &checks)?;
    }
    if let Some(sym) = artifact.hir.entry_args_name {
        checks.check_symbol(sym)?;
    }

    // 2. gpu_builtins: DefId keys.
    for (def_id, _) in &artifact.gpu_builtins {
        checks.check_def(*def_id)?;
    }

    // 3. function_facts: DefId keys and internal index-bearing fields.
    for (def_id, fact) in &artifact.function_facts.entries {
        checks.check_def(*def_id)?;
        for edge in &fact.call_edges {
            checks.check_def(*edge)?;
        }
        for capture in &fact.captures {
            checks.check_def(capture.def_id)?;
        }
        for (pd, _mode) in &fact.param_modes {
            checks.check_def(*pd)?;
        }
        if let Some(ty) = fact.err_ty {
            checks.check_type(ty)?;
        }
    }
    if let Some(ref entry_fact) = artifact.function_facts.entry {
        for edge in &entry_fact.call_edges {
            checks.check_def(*edge)?;
        }
        for capture in &entry_fact.captures {
            checks.check_def(capture.def_id)?;
        }
        for (pd, _mode) in &entry_fact.param_modes {
            checks.check_def(*pd)?;
        }
        if let Some(ty) = entry_fact.err_ty {
            checks.check_type(ty)?;
        }
    }

    // 4. resolved_uses: DefId keys (Local) and enclosing DefIds.
    for use_wire in &artifact.resolved_uses {
        if let crate::artifact::ResolvedUseKeyWire::Local(def_id) = use_wire.key {
            checks.check_def(def_id)?;
        }
        if let Some(enclosing) = use_wire.enclosing {
            checks.check_def(enclosing)?;
        }
        if let Some(local) = use_wire.local {
            checks.check_def(local)?;
        }
    }

    // 5. libraries: deep walk.
    for def_id in artifact.libraries.bindings.keys() {
        checks.check_def(*def_id)?;
    }
    for binding in artifact.libraries.bindings.values() {
        checks.check_def(binding.local_def_id)?;
    }
    for def_id in artifact.libraries.items.keys() {
        checks.check_def(*def_id)?;
    }
    for item in artifact.libraries.items.values() {
        checks.check_def(item.def_id)?;
    }
    for def_id in artifact.libraries.exports.keys() {
        checks.check_def(*def_id)?;
    }
    for (pair, _) in artifact.libraries.reexports.keys() {
        checks.check_def(*pair)?;
    }

    // 6. cli_program: the option and operand binding symbols key the CLI
    // argument record, so they must name interner entries.
    if let Some(program) = &artifact.cli_program {
        let groups = std::iter::once((&program.global_options, &program.global_operands))
            .chain(std::iter::once((&program.options, &program.operands)))
            .chain(
                program
                    .commands
                    .iter()
                    .map(|command| (&command.options, &command.operands)),
            );
        for (options, operands) in groups {
            for option in options {
                checks.check_symbol(option.binding_symbol)?;
            }
            for operand in operands {
                checks.check_symbol(operand.binding_symbol)?;
            }
        }
    }

    // 7. resolver snapshot: deep walk. E3 conversion rows are validated
    // here rather than silently dropped at the artifact boundary.
    validate_resolver(&artifact.resolver, &checks)?;

    // 8. Type arena walk: validate TypeId/DefId/Symbol/IndexId
    //    cross-references inside the type-table snapshot.
    for ty in artifact.types.types() {
        validate_type(ty, &checks)?;
    }
    validate_indices(&artifact.types, &checks)?;

    // 8a. Runtime shape obligations (schema v31): each site and every
    //     relation/binding index must resolve before lowering consumes it.
    validate_shape_obligation_table(&artifact.shape_obligations, &checks)?;
    {
        let roles = &artifact.contract_roles;
        for def_id in [roles.ordering, roles.closing, roles.sinking]
            .into_iter()
            .flatten()
        {
            checks.check_def(def_id)?;
        }
        for def_id in roles
            .ordering_bindings
            .iter()
            .chain(&roles.sinking_bindings)
        {
            checks.check_def(*def_id)?;
        }
    }

    // 8b. Per-target kernel requirements (schema v32) retain the semantic
    //     admission contract across FHIR loading. Every callable and symbolic
    //     extent referenced by the report must belong to this artifact.
    let mut requirement_kernels = FxHashSet::default();
    for kernel in &artifact.kernel_requirements.kernels {
        checks.check_def(kernel.def_id)?;
        if !requirement_kernels.insert(kernel.def_id) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "kernel requirements",
                detail: format!("duplicate kernel row for DefId({})", kernel.def_id.0),
            });
        }
        let mut targets = FxHashSet::default();
        for report in &kernel.target_reports {
            if !targets.insert(report.target) {
                return Err(HirArtifactError::MalformedSnapshot {
                    kind: "kernel requirements",
                    detail: format!("duplicate target row for DefId({})", kernel.def_id.0),
                });
            }
            for callee in &report.callees {
                checks.check_def(*callee)?;
            }
            for condition in &report.extent_conditions {
                validate_kernel_value_range(&condition.range, &checks)?;
            }
            for failure in &report.failures {
                match failure {
                    crate::artifact::KernelRequirementFailureWire::CalleeNotAdmitted { callee } => {
                        checks.check_def(*callee)?;
                    }
                    crate::artifact::KernelRequirementFailureWire::IntermediateOutOfRange {
                        range,
                        ..
                    }
                    | crate::artifact::KernelRequirementFailureWire::StoreOutOfRange {
                        range,
                        ..
                    } => validate_kernel_value_range(range, &checks)?,
                    crate::artifact::KernelRequirementFailureWire::RecursiveCall
                    | crate::artifact::KernelRequirementFailureWire::UnsupportedIntegerWidth {
                        ..
                    }
                    | crate::artifact::KernelRequirementFailureWire::UnsupportedFloatWidth {
                        ..
                    }
                    | crate::artifact::KernelRequirementFailureWire::UnsupportedIntegerRange {
                        ..
                    }
                    | crate::artifact::KernelRequirementFailureWire::DivideByZeroPossible {
                        ..
                    }
                    | crate::artifact::KernelRequirementFailureWire::NegativeShiftCountPossible {
                        ..
                    }
                    | crate::artifact::KernelRequirementFailureWire::TensorEdgePolicyDeferred {
                        ..
                    }
                    | crate::artifact::KernelRequirementFailureWire::SaturatingLoweringDeferred {
                        ..
                    }
                    | crate::artifact::KernelRequirementFailureWire::OptionFRequired { .. } => {}
                }
            }
        }
    }

    // 8c. Tensor edge-policy metadata (schema v33) retains every checked
    //     dispatch site, shifted-view provenance edge, generic obligation,
    //     and concrete provider callback in the same HIR/type arenas.
    let edge_policies = &artifact.tensor_edge_policies;
    let mut policy_calls = FxHashSet::default();
    for row in &edge_policies.calls {
        checks.check_hir(row.call)?;
        if !policy_calls.insert(row.call) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "tensor edge policies",
                detail: format!("duplicate call row for HirId({})", row.call.0),
            });
        }
        validate_resolved_tensor_edge_policy(&row.policy, &checks)?;
        if !edge_policies
            .limes_contexts
            .iter()
            .any(|(call, _)| *call == row.call)
        {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "tensor edge policies",
                detail: format!("call row for HirId({}) has no limes context", row.call.0),
            });
        }
    }
    let mut shifted_expressions = FxHashSet::default();
    for expression in &edge_policies.shifted_expressions {
        checks.check_hir(*expression)?;
        if !shifted_expressions.insert(*expression) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "tensor edge policies",
                detail: format!("duplicate shifted expression HirId({})", expression.0),
            });
        }
        if !edge_policies
            .shifted_view_expressions
            .iter()
            .any(|(id, _)| id == expression)
        {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "tensor edge policies",
                detail: format!(
                    "shifted expression HirId({}) has no provenance",
                    expression.0
                ),
            });
        }
    }
    let mut shifted_view_expressions = FxHashSet::default();
    for (expression, context) in &edge_policies.shifted_view_expressions {
        checks.check_hir(*expression)?;
        validate_tensor_shift_context(*context, &checks)?;
        if !shifted_view_expressions.insert(*expression) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "tensor edge policies",
                detail: format!("duplicate shifted-view expression HirId({})", expression.0),
            });
        }
    }
    let mut shifted_bindings = FxHashSet::default();
    for binding in &edge_policies.shifted_bindings {
        checks.check_def(*binding)?;
        if !shifted_bindings.insert(*binding) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "tensor edge policies",
                detail: format!("duplicate shifted binding DefId({})", binding.0),
            });
        }
        if !edge_policies
            .shifted_view_bindings
            .iter()
            .any(|(id, _)| id == binding)
        {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "tensor edge policies",
                detail: format!("shifted binding DefId({}) has no provenance", binding.0),
            });
        }
    }
    let mut shifted_view_bindings = FxHashSet::default();
    for (binding, context) in &edge_policies.shifted_view_bindings {
        checks.check_def(*binding)?;
        validate_tensor_shift_context(*context, &checks)?;
        if !shifted_view_bindings.insert(*binding) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "tensor edge policies",
                detail: format!("duplicate shifted-view binding DefId({})", binding.0),
            });
        }
    }
    let mut limes_contexts = FxHashSet::default();
    for (call, context) in &edge_policies.limes_contexts {
        checks.check_hir(*call)?;
        validate_tensor_shift_context(*context, &checks)?;
        if !limes_contexts.insert(*call) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "tensor edge policies",
                detail: format!("duplicate limes context HirId({})", call.0),
            });
        }
    }
    let mut functions = FxHashSet::default();
    for row in &edge_policies.function_requirements {
        checks.check_def(row.function)?;
        if !functions.insert(row.function) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "tensor edge policies",
                detail: format!(
                    "duplicate function requirement row for DefId({})",
                    row.function.0
                ),
            });
        }
        for requirement in &row.requirements {
            checks.check_type(requirement.policy_type)?;
            checks.check_type(requirement.element_type)?;
        }
    }
    let mut providers = FxHashSet::default();
    for provider in &edge_policies.provider_impls {
        checks.check_type(provider.policy_type)?;
        checks.check_type(provider.element_type)?;
        if !providers.insert((provider.policy_type, provider.element_type, provider.rank)) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "tensor edge policies",
                detail: "duplicate concrete provider identity".to_owned(),
            });
        }
        validate_resolved_tensor_edge_policy(&provider.policy, &checks)?;
        if matches!(
            provider.policy,
            crate::artifact::ResolvedTensorEdgePolicyWire::Deferred { .. }
        ) {
            return Err(HirArtifactError::MalformedSnapshot {
                kind: "tensor edge policies",
                detail: "concrete provider catalog contains a deferred policy".to_owned(),
            });
        }
    }
    validate_ordo_order_rows(edge_policies, &checks)?;
    validate_nested_providers(&artifact.provider_units)?;

    // 9. Presentation sidecar (anchors from v2, authored types from v9): every
    //    attachment anchor must name a registered owner, owner anchors must be
    //    unique, each owner may
    //    carry at most one attachment, every comment Symbol must resolve
    //    in the interner table, and every HirOwnerRef back-reference must
    //    name an in-tree node or known def. The entry-start anchor, when
    //    present, must name a registered owner.
    validate_presentation(artifact, &artifact.presentation, &checks)?;

    // 10. Capability provenance (SCR-04, schema v8): every identity key —
    //     function rows, provenance/relationship DefIds, and last-use
    //     borrower/site pairs — must resolve against the HIR arenas.
    for row in &artifact.capability_provenance.functions {
        checks.check_def(row.function)?;
        match row.returned_borrow {
            crate::artifact::ReturnedBorrowWire::None => {}
            crate::artifact::ReturnedBorrowWire::Parameter(def_id)
            | crate::artifact::ReturnedBorrowWire::Definition(def_id) => {
                checks.check_def(def_id)?;
            }
        }
        for relationship in &row.required_regions {
            checks.check_def(relationship.from)?;
            checks.check_def(relationship.to)?;
        }
        for last_use in &row.last_uses {
            checks.check_def(last_use.borrower)?;
            checks.check_hir(last_use.site)?;
        }
    }

    // 11. Resolved generic bindings (schema v10): each binding name and
    //     kind-tagged witness must resolve against the interner and arenas.
    validate_generic_calls(&artifact.resolved_generic_calls, &checks)?;

    // 12. Compiler lane metadata (schema v11): a rehydrated AIR unit must
    // reference only declarations and symbols present in the same artifact.
    for entry in &artifact.radix_lanes {
        checks.check_def(entry.def_id)?;
        if let Some(backward) = &entry.backward {
            checks.check_symbol(backward.companion_name)?;
            checks.check_companion_def(backward.companion_def_id)?;
        }
    }

    Ok(())
}

fn validate_presentation_attachments(
    presentation: &radix_hir::HirPresentation,
    owner_ids: &FxHashSet<u32>,
    checks: &IntegrityChecks<'_>,
) -> Result<(), HirArtifactError> {
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
                checks.check_symbol(*text)?;
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

    // The `regio` declaration (D7.7): interned name and doc lines, and an
    // anchor that names a registered owner.
    if let Some(region) = presentation.region.as_ref() {
        checks.check_symbol(region.name)?;
        for line in &region.doc {
            checks.check_symbol(*line)?;
        }
        if let Some(anchor) = region.anchor.as_ref()
            && !owner_ids.contains(&anchor.0)
        {
            return Err(HirArtifactError::DanglingIndex {
                kind: "HirSourceAnchorId",
                index: anchor.0,
                arena_size: owner_ids.len(),
            });
        }
    }

    Ok(())
}

fn validate_presentation_structural_ends(
    presentation: &radix_hir::HirPresentation,
) -> Result<(), HirArtifactError> {
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

    Ok(())
}

#[cfg(test)]
#[path = "validate_test.rs"]
mod tests;
