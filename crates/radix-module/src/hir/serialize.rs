//! HIR artifact serialization from analyzed units.
//!
//! Public constructor for creating a [`radix_hir_fhir::HirArtifact`]
//! from an analyzed unit. Serialization into `postcard` bytes and writing
//! to disk happen at the call site.

use crate::driver::AnalyzedModule;
use radix_hir::visit::{HirVisitor, walk_expr, walk_item, walk_method, walk_pattern};
use radix_hir_fhir::{
    CaptureFactWire, CaptureModeWire, CliCommandWire, CliDefaultWire, CliExitWire, CliModeWire, CliOperandWire,
    CliOptionWire, CliProgramWire, CliTypeWire, FunctionFactTableWire, FunctionFactWire, FunctionProvenanceTableWire,
    FunctionProvenanceWire, GpuBuiltinWire, HirArtifact, LastUseWire, RegionRelationshipWire, ResolvedUseKeyWire,
    ResolvedUseKindWire, ResolvedUseWire, ReturnedBorrowWire, SCHEMA_VERSION, SourceIdentity,
};
use radix_types::{AppliedArg, Type, TypeId, TypeTable};
use rustc_hash::FxHashMap;

// ── Entry point ───────────────────────────────────────────────────────────

/// Build a [`HirArtifact`] from a fully analyzed unit.
///
/// Populates all serializable fields: HIR program, type table snapshot,
/// interner table, resolver snapshot, CLI program (F18), function facts,
/// resolved uses, analysis stamp, and GPU builtins.
///
/// The `content_hash` should be a hex-encoded SHA-256 of the source bytes.
/// The `relative_path` should be a path relative to the project root.
pub fn from_analyzed(unit: &AnalyzedModule, content_hash: &str, relative_path: &str) -> HirArtifact {
    // HIR finalization may canonicalize a compound witness or add an access
    // marker to a local without rewriting the presentation sidecar. Reconcile
    // the sidecar at the artifact boundary so schema-v9 validation sees the
    // same finalized witnesses as HIR while retaining authored source shape.
    let mut presentation = unit.presentation.clone();
    normalize_presentation_witnesses(&unit.hir, &mut presentation, &unit.types);

    HirArtifact {
        schema_version: SCHEMA_VERSION,
        source_identity: SourceIdentity {
            content_hash: content_hash.to_string(),
            relative_path: relative_path.to_string(),
        },
        hir: unit.hir.clone(),
        presentation,
        cli_program: unit.cli_program.as_ref().map(cli_program_wire),
        types: unit.types.snapshot(),
        resolved_generic_calls: unit.types.resolved_generic_calls.clone(),
        interner: unit.interner.to_string_table(),
        resolver: unit.resolver.to_snapshot(),
        libraries: unit.libraries.clone(),
        function_facts: function_fact_table_to_wire(&unit.function_facts),
        resolved_uses: unit
            .resolved_uses
            .to_snapshot()
            .into_iter()
            .map(|u| resolved_use_to_wire(&u))
            .collect(),
        analysis_stamp: unit.analysis_stamp.get(),
        gpu_builtins: unit
            .gpu_builtins
            .iter()
            .map(|(def_id, builtin)| (*def_id, gpu_builtin_to_wire(*builtin)))
            .collect(),
        capability_provenance: provenance_table_to_wire(&unit.provenance),
    }
}

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
}

/// The validator's field inventory names a struct field by its containing
/// struct and `Field(n)`, while lowering records the field definition as the
/// owner. Move only that producer-side spelling mismatch; enum and schema
/// field sites already use the validator's accepted owner shape.
fn remap_struct_field_sites(hir: &radix_hir::HirModule, presentation: &mut radix_hir::HirPresentation) {
    for item in &hir.items {
        let radix_hir::HirItemKind::Struct(strukt) = &item.kind else {
            continue;
        };
        for (ordinal, field) in strukt.fields.iter().enumerate() {
            let old_site = radix_hir::HirTypeSite {
                owner: radix_hir::HirOwnerRef::Def(field.def_id),
                slot: radix_hir::HirTypeSlot::Field(ordinal as u32),
            };
            if let Some(authored) = presentation
                .authored_types
                .iter_mut()
                .find(|entry| entry.site == old_site)
            {
                authored.site.owner = radix_hir::HirOwnerRef::Def(item.def_id);
            }
        }
    }
}

#[derive(Default)]
struct TypeSiteRegistry {
    sites: FxHashMap<radix_hir::HirTypeSite, TypeId>,
    current_expr: Option<radix_hir::HirId>,
}

impl TypeSiteRegistry {
    fn build(hir: &radix_hir::HirModule, presentation: &radix_hir::HirPresentation) -> Self {
        let mut registry = Self::default();
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
            radix_hir::HirTypeSlot::Parameter(ordinal as u32),
            param.ty,
        );
    }
    if let Some(param) = &function.cli_args {
        register_site(
            sites,
            radix_hir::HirOwnerRef::Def(param.def_id),
            radix_hir::HirTypeSlot::Parameter(function.params.len() as u32),
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
                    radix_hir::HirTypeSlot::AnnotationField { annotation: annotation as u32, field },
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
        match &item.kind {
            radix_hir::HirItemKind::Function(function) => {
                register_function_sites(&mut self.sites, item.def_id, function);
                register_annotation_sites(&mut self.sites, owner.clone(), &function.annotations);
            }
            radix_hir::HirItemKind::Struct(strukt) => {
                register_annotation_sites(&mut self.sites, owner.clone(), &strukt.annotations);
                for (ordinal, field) in strukt.fields.iter().enumerate() {
                    register_site(
                        &mut self.sites,
                        owner.clone(),
                        radix_hir::HirTypeSlot::Field(ordinal as u32),
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
                register_annotation_sites(&mut self.sites, owner.clone(), &enumeration.annotations);
                for (ordinal, field) in enumeration.commune_fields.iter().enumerate() {
                    register_site(
                        &mut self.sites,
                        owner.clone(),
                        radix_hir::HirTypeSlot::Field(ordinal as u32),
                        field.ty,
                    );
                }
                for variant in &enumeration.variants {
                    for (ordinal, field) in variant.fields.iter().enumerate() {
                        register_site(
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
                        register_site(
                            &mut self.sites,
                            radix_hir::HirOwnerRef::Def(param.def_id),
                            radix_hir::HirTypeSlot::Parameter(ordinal as u32),
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
                        radix_hir::HirTypeSlot::Field(ordinal as u32),
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
        register_annotation_sites(
            &mut self.sites,
            radix_hir::HirOwnerRef::Def(method.def_id),
            &method.func.annotations,
        );
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
                )
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
                        radix_hir::HirTypeSlot::TupleTypeArgument(ordinal as u32),
                        *ty,
                    );
                }
            }
            radix_hir::HirExpressionKind::Clausura(params, ret, err, _, _) => {
                for (ordinal, param) in params.iter().enumerate() {
                    register_site(
                        &mut self.sites,
                        radix_hir::HirOwnerRef::Def(param.def_id),
                        radix_hir::HirTypeSlot::ClosureParameter(ordinal as u32),
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
        | Type::Intervallum(inner)
        | Type::BoundedArray { element: inner, .. }
        | Type::BoundedQueue { element: inner, .. }
        | Type::BoundedStack { element: inner, .. } => {
            normalize_first_type_arg(form, *inner, types);
        }
        Type::Tensor(inner, shape) => {
            normalize_first_type_arg(form, *inner, types);
            normalize_size_arg(form, *shape);
        }
        Type::Vector(inner, _) | Type::Matrix(inner, _) | Type::Sparsa(inner, _) => {
            normalize_first_type_arg(form, *inner, types)
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
            let members = fields.values().copied().collect::<Vec<_>>();
            normalize_type_args(form, &members, types);
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

fn normalize_type_form_arg(child: &mut radix_hir::HirTypeForm, expected: TypeId, types: &TypeTable) {
    if child.ty != expected {
        let original = child.clone();
        child.ty = expected;
        child.head = radix_hir::HirTypeFormHead::Semantic(radix_syntax::TypeFormLevel::LONG);
        child.args = vec![radix_hir::HirTypeFormArg::Type(Box::new(original))];
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

// ── Capability provenance wire conversions (SCR-04) ───────────────────────

fn returned_borrow_to_wire(p: crate::semantic::ReturnedBorrowProvenance) -> ReturnedBorrowWire {
    match p {
        crate::semantic::ReturnedBorrowProvenance::None => ReturnedBorrowWire::None,
        crate::semantic::ReturnedBorrowProvenance::Parameter(def_id) => ReturnedBorrowWire::Parameter(def_id),
        crate::semantic::ReturnedBorrowProvenance::Definition(def_id) => ReturnedBorrowWire::Definition(def_id),
    }
}

fn provenance_table_to_wire(table: &crate::semantic::FunctionProvenanceTable) -> FunctionProvenanceTableWire {
    FunctionProvenanceTableWire {
        functions: table
            .iter()
            .map(|row| FunctionProvenanceWire {
                function: row.function,
                returned_borrow: returned_borrow_to_wire(row.returned_borrow),
                required_regions: row
                    .required_regions
                    .iter()
                    .map(|rel| RegionRelationshipWire { from: rel.from, to: rel.to })
                    .collect(),
                last_uses: row
                    .last_uses
                    .iter()
                    .map(|fact| LastUseWire { borrower: fact.borrower, site: fact.site })
                    .collect(),
            })
            .collect(),
    }
}

// ── Wire-conversion helpers ───────────────────────────────────────────────

fn capture_fact_to_wire(fact: &crate::semantic::CaptureFact) -> CaptureFactWire {
    CaptureFactWire {
        def_id: fact.def_id,
        mode: match fact.mode {
            crate::semantic::CaptureMode::Read => CaptureModeWire::Read,
            crate::semantic::CaptureMode::Mutate => CaptureModeWire::Mutate,
            crate::semantic::CaptureMode::Consume => CaptureModeWire::Consume,
        },
    }
}

fn function_fact_to_wire(fact: &crate::semantic::FunctionFact) -> FunctionFactWire {
    FunctionFactWire {
        has_direct_failure: fact.direct_failure,
        may_fail: fact.may_fail,
        is_async: fact.is_async,
        is_generator: fact.is_generator,
        requires_await: fact.requires_await,
        call_edges: fact.call_edges.iter().copied().collect(),
        captures: fact.captures.iter().map(capture_fact_to_wire).collect(),
        param_modes: fact.param_modes.iter().map(|(k, v)| (*k, *v)).collect(),
        err_ty: fact.err_ty,
    }
}

fn function_fact_table_to_wire(ft: &crate::semantic::FunctionFactTable) -> FunctionFactTableWire {
    FunctionFactTableWire {
        entries: ft
            .iter()
            .map(|(def_id, fact)| (def_id, function_fact_to_wire(fact)))
            .collect(),
        entry: ft.entry.as_ref().map(function_fact_to_wire),
    }
}

fn resolved_use_to_wire(u: &crate::semantic::ResolvedUse) -> ResolvedUseWire {
    ResolvedUseWire {
        key: match &u.key {
            crate::semantic::ResolvedUseKey::Local(def_id) => ResolvedUseKeyWire::Local(*def_id),
            crate::semantic::ResolvedUseKey::Portable(s) => ResolvedUseKeyWire::Portable(s.clone()),
        },
        kind: match u.kind {
            crate::semantic::ResolvedUseKind::Path => ResolvedUseKindWire::Path,
            crate::semantic::ResolvedUseKind::Call => ResolvedUseKindWire::Call,
            crate::semantic::ResolvedUseKind::MethodCall => ResolvedUseKindWire::MethodCall,
            crate::semantic::ResolvedUseKind::TypeRef => ResolvedUseKindWire::TypeRef,
            crate::semantic::ResolvedUseKind::ImportBinding => ResolvedUseKindWire::ImportBinding,
        },
        enclosing: u.enclosing,
    }
}

fn gpu_builtin_to_wire(b: radix_types::MirKernelBuiltin) -> GpuBuiltinWire {
    match b {
        radix_types::MirKernelBuiltin::GlobalInvocationX => GpuBuiltinWire::GlobalInvocationX,
        radix_types::MirKernelBuiltin::GlobalInvocationY => GpuBuiltinWire::GlobalInvocationY,
        radix_types::MirKernelBuiltin::GlobalInvocationZ => GpuBuiltinWire::GlobalInvocationZ,
        radix_types::MirKernelBuiltin::GlobalInvocationId => GpuBuiltinWire::GlobalInvocationId,
        radix_types::MirKernelBuiltin::WorkgroupIdX => GpuBuiltinWire::WorkgroupIdX,
        radix_types::MirKernelBuiltin::WorkgroupIdY => GpuBuiltinWire::WorkgroupIdY,
        radix_types::MirKernelBuiltin::WorkgroupIdZ => GpuBuiltinWire::WorkgroupIdZ,
        radix_types::MirKernelBuiltin::WorkgroupId => GpuBuiltinWire::WorkgroupId,
        radix_types::MirKernelBuiltin::LocalInvocationIdX => GpuBuiltinWire::LocalInvocationIdX,
        radix_types::MirKernelBuiltin::LocalInvocationIdY => GpuBuiltinWire::LocalInvocationIdY,
        radix_types::MirKernelBuiltin::LocalInvocationIdZ => GpuBuiltinWire::LocalInvocationIdZ,
        radix_types::MirKernelBuiltin::LocalInvocationId => GpuBuiltinWire::LocalInvocationId,
        radix_types::MirKernelBuiltin::WorkgroupSizeX => GpuBuiltinWire::WorkgroupSizeX,
        radix_types::MirKernelBuiltin::WorkgroupSizeY => GpuBuiltinWire::WorkgroupSizeY,
        radix_types::MirKernelBuiltin::WorkgroupSizeZ => GpuBuiltinWire::WorkgroupSizeZ,
    }
}

// ── CLI wire conversions (F18 Shape B — forward mapping) ─────────────────

fn cli_type_wire(ty: &crate::cli::CliType) -> CliTypeWire {
    match ty {
        crate::cli::CliType::Textus => CliTypeWire::Textus,
        crate::cli::CliType::Numerus => CliTypeWire::Numerus,
        crate::cli::CliType::Fractus => CliTypeWire::Fractus,
        crate::cli::CliType::Bivalens => CliTypeWire::Bivalens,
        crate::cli::CliType::Octeti => CliTypeWire::Octeti,
        crate::cli::CliType::Ignotum => CliTypeWire::Ignotum,
        crate::cli::CliType::ListaTextus => CliTypeWire::ListaTextus,
        crate::cli::CliType::ListaNumerus => CliTypeWire::ListaNumerus,
    }
}

fn cli_default_wire(d: &crate::cli::CliDefault) -> CliDefaultWire {
    match d {
        crate::cli::CliDefault::Text(s) => CliDefaultWire::Text(s.clone()),
        crate::cli::CliDefault::Integer(i) => CliDefaultWire::Integer(*i),
        crate::cli::CliDefault::Float(f) => CliDefaultWire::Float(*f),
        crate::cli::CliDefault::Bool(b) => CliDefaultWire::Bool(*b),
        crate::cli::CliDefault::Nil => CliDefaultWire::Nil,
        crate::cli::CliDefault::Expr(s) => CliDefaultWire::Expr(s.clone()),
    }
}

fn cli_exit_wire(e: &crate::cli::CliExit) -> CliExitWire {
    match e {
        crate::cli::CliExit::Fixed(v) => CliExitWire::Fixed(*v),
        crate::cli::CliExit::Binding(s) => CliExitWire::Binding(s.clone()),
        crate::cli::CliExit::Field { object, field } => {
            CliExitWire::Field { object: object.clone(), field: field.clone() }
        }
        crate::cli::CliExit::Unsupported => CliExitWire::Unsupported,
    }
}

fn cli_option_wire(o: &crate::cli::CliOption) -> CliOptionWire {
    CliOptionWire {
        binding: o.binding.clone(),
        ty: cli_type_wire(&o.ty),
        short: o.short.clone(),
        long: o.long.clone(),
        description: o.description.clone(),
        global: o.global,
        default: o.default.as_ref().map(cli_default_wire),
        is_flag: o.flag,
    }
}

fn cli_operand_wire(o: &crate::cli::CliOperand) -> CliOperandWire {
    CliOperandWire {
        binding: o.binding.clone(),
        ty: cli_type_wire(&o.ty),
        is_rest: o.rest,
        description: o.description.clone(),
        global: o.global,
        default: o.default.as_ref().map(cli_default_wire),
    }
}

fn cli_command_wire(c: &crate::cli::CliCommand) -> CliCommandWire {
    CliCommandWire {
        path: c.path.clone(),
        module_path: c.module_path.clone(),
        function: c.function.clone(),
        function_symbol: c.function_symbol,
        args_binding: c.args_binding.clone(),
        aliases: c.aliases.clone(),
        description: c.description.clone(),
        options: c.options.iter().map(cli_option_wire).collect(),
        operands: c.operands.iter().map(cli_operand_wire).collect(),
    }
}

fn cli_mode_wire(m: &crate::cli::CliMode) -> CliModeWire {
    match m {
        crate::cli::CliMode::NotCli => CliModeWire::NotCli,
        crate::cli::CliMode::SingleCommand => CliModeWire::SingleCommand,
        crate::cli::CliMode::Subcommand => CliModeWire::Subcommand,
    }
}

fn cli_program_wire(p: &crate::cli::CliProgram) -> CliProgramWire {
    CliProgramWire {
        name: p.name.clone(),
        entry_args: p.entry_args.clone(),
        mode: cli_mode_wire(&p.mode),
        version: p.version.clone(),
        description: p.description.clone(),
        global_options: p.global_options.iter().map(cli_option_wire).collect(),
        global_operands: p.global_operands.iter().map(cli_operand_wire).collect(),
        options: p.options.iter().map(cli_option_wire).collect(),
        operands: p.operands.iter().map(cli_operand_wire).collect(),
        commands: p.commands.iter().map(cli_command_wire).collect(),
        exit: p.exit.as_ref().map(cli_exit_wire),
    }
}
