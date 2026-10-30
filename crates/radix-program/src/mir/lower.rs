//! Lower package units (and libraries) into the merged package MIR program.

use radix_module::driver::AnalyzedModule;

use super::{
    AnalyzedProgram, AnalyzedProgramNode, BTreeMap, CliEntryRecords, CliPackagePlan,
    CliRecordTypeRewrite, DataMemberLink, DefId, Diagnostic, HashMap, HashSet, HirConst,
    HirExpression, HirExpressionKind, HirId, HirItem, HirItemKind, HirLiteral, HirObjectField,
    IndexExpr, InterfaceNominalKind, Interner, LibraryInterfaceCache, LibraryResolver,
    LoweredMirUnit, MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlock,
    MirBlockId, MirCallee, MirClosureCallee, MirClosureEnvironment, MirClosureEnvironmentId,
    MirClosureValue, MirConstant, MirFunction, MirFunctionId, MirIntrinsic, MirOperand,
    MirOptionChainLink, MirOptionOp, MirPlace, MirProgram, MirProjection, MirRuntimeCall,
    MirStatement, MirStatementKind, MirSwitchCase, MirTensorEdgePolicyMetadata,
    MirTensorEdgePolicySelector, MirTerminator, MirTerminatorKind, MirType, MirValue, MirValueKind,
    ModuleId, NominalImportContext, PackageMirLinks, Path, PathBuf, Primitive, Resolver,
    SourceRewrites, Symbol, Type, TypeId, TypeTable, build_nominal_remap, build_variant_remap,
    exported_nominal_names, extend_unmapped_library_sources, import_semantic_type_with_nominal,
    library_cached_analysis, library_cached_file_interface, lower_analyzed_unit, mir_diag,
    mir_error_diag, mir_issue_diag, mir_lowering_diag, mir_validation_diag, push_type_rewrite,
    record_shared_nominals, remap_function_text_symbols, remap_optional_symbol,
    remap_program_text_symbols, rewrite_analysis_namespace_calls,
    rewrite_closure_environment_sources, rewrite_generic_param_sources, rewrite_program_sources,
    rewrite_templates_sources, source_nominal_names, with_library_cached_analysis_mut,
};
use radix_mir::MirFunctionSignature;
use radix_mir::device::MirDeviceFunctionMetadataMap;
use radix_module::file_interface::FileInterface;
use radix_types::KernelLowering;

/// Struct/enum/function validation metadata remapped from a sibling or
/// library unit into the entry's `DefId` / `TypeId` / Symbol space.
///
/// Self-provider method bodies (gradus `t.get()`, `Tensor { dtype = … }`)
/// lower against the source unit's metadata. Nested sibling nominals such as
/// `dtype.DType` are often absent from the entry's imported identity table, so
/// the merged program would otherwise fail closed on missing field/parent
/// metadata. Union this onto the entry context before re-validation.
#[derive(Clone, Default)]
pub(super) struct RemappedValidation {
    struct_fields: HashMap<DefId, HashMap<Symbol, MirType>>,
    struct_field_order: HashMap<DefId, Vec<Symbol>>,
    variant_field_order: HashMap<DefId, Vec<Symbol>>,
    display_names: HashMap<DefId, String>,
    struct_type_params: HashMap<DefId, Vec<Symbol>>,
    /// Struct → the contracts its `implet` clause names, both sides in the
    /// destination's `DefId` space.
    struct_contracts: HashMap<DefId, HashSet<DefId>>,
    optional_struct_fields: HashMap<DefId, HashSet<Symbol>>,
    nullable_struct_fields: HashMap<DefId, HashSet<Symbol>>,
    variant_parents: HashMap<DefId, DefId>,
    enum_variants: HashMap<DefId, Vec<DefId>>,
    variant_fields: HashMap<DefId, HashMap<Symbol, MirType>>,
    functions: HashMap<DefId, MirFunctionSignature>,
    /// Functions lowered from genus methods (first parameter is the receiver).
    method_defs: HashSet<DefId>,
    /// Tier-1 `@ ad` routes of the linked units (route → handler `DefId`).
    ad_routes: HashMap<String, DefId>,
}

impl RemappedValidation {
    /// Merged struct metadata for the package generic instantiator: field
    /// types and generic param names let it type projected call arguments
    /// (`layer.ln1` on an applied nominal) while binding witnesses.
    pub(super) fn struct_metadata(&self) -> radix_mir_lower::lower::PackageStructMetadata<'_> {
        radix_mir_lower::lower::PackageStructMetadata {
            struct_fields: &self.struct_fields,
            struct_type_params: &self.struct_type_params,
        }
    }

    /// This metadata joined over the merged context's own struct facts (the
    /// entry unit's nominals are in the context, never in the remapped
    /// extras), so the package generic instantiator types a field projection
    /// of an entry-owned class the same as one of a linked unit's class.
    pub(super) fn joined_with_context(
        &self,
        context: &radix_mir::MirValidationContext<'_>,
    ) -> Self {
        let mut joined = Self::default();
        for (def, fields) in &context.struct_fields {
            joined
                .struct_fields
                .insert(*def, fields.iter().map(|(name, ty)| (*name, *ty)).collect());
        }
        for (def, params) in &context.struct_type_params {
            joined.struct_type_params.insert(*def, params.clone());
        }
        union_field_maps(&mut joined.struct_fields, &self.struct_fields);
        for (def, params) in &self.struct_type_params {
            joined
                .struct_type_params
                .entry(*def)
                .or_insert_with(|| params.clone());
        }
        joined
    }

    fn merge(&mut self, other: &Self) {
        union_field_maps(&mut self.struct_fields, &other.struct_fields);
        for (def, order) in &other.struct_field_order {
            self.struct_field_order
                .entry(*def)
                .or_insert_with(|| order.clone());
        }
        for (def, order) in &other.variant_field_order {
            self.variant_field_order
                .entry(*def)
                .or_insert_with(|| order.clone());
        }
        for (def, name) in &other.display_names {
            self.display_names
                .entry(*def)
                .or_insert_with(|| name.clone());
        }
        for (def, params) in &other.struct_type_params {
            self.struct_type_params
                .entry(*def)
                .or_insert_with(|| params.clone());
        }
        for (def, contracts) in &other.struct_contracts {
            self.struct_contracts
                .entry(*def)
                .or_default()
                .extend(contracts.iter().copied());
        }
        union_symbol_sets(
            &mut self.optional_struct_fields,
            &other.optional_struct_fields,
        );
        union_symbol_sets(
            &mut self.nullable_struct_fields,
            &other.nullable_struct_fields,
        );
        for (variant, parent) in &other.variant_parents {
            self.variant_parents.entry(*variant).or_insert(*parent);
        }
        for (enum_def, variants) in &other.enum_variants {
            self.enum_variants
                .entry(*enum_def)
                .or_insert_with(|| variants.clone());
        }
        union_field_maps(&mut self.variant_fields, &other.variant_fields);
        for (def, sig) in &other.functions {
            self.functions.entry(*def).or_insert_with(|| sig.clone());
        }
        self.method_defs.extend(other.method_defs.iter().copied());
        for (route, def) in &other.ad_routes {
            self.ad_routes.entry(route.clone()).or_insert(*def);
        }
    }

    fn apply(&self, context: &mut radix_mir::MirValidationContext<'_>) {
        for (def, fields) in &self.struct_fields {
            context
                .struct_fields
                .insert(*def, fields.iter().map(|(name, ty)| (*name, *ty)).collect());
        }
        for (def, order) in &self.struct_field_order {
            context.struct_field_order.insert(*def, order.clone());
        }
        for (def, order) in &self.variant_field_order {
            context.variant_field_order.insert(*def, order.clone());
        }
        for (def, name) in &self.display_names {
            context.display_names.insert(*def, name.clone());
        }
        for (def, params) in &self.struct_type_params {
            context
                .struct_type_params
                .entry(*def)
                .or_insert_with(|| params.clone());
        }
        for (def, contracts) in &self.struct_contracts {
            context
                .struct_contracts
                .entry(*def)
                .or_default()
                .extend(contracts.iter().copied());
        }
        for (def, fields) in &self.optional_struct_fields {
            context
                .optional_struct_fields
                .entry(*def)
                .or_default()
                .extend(fields.iter().copied());
        }
        for (def, fields) in &self.nullable_struct_fields {
            context
                .nullable_struct_fields
                .entry(*def)
                .or_default()
                .extend(fields.iter().copied());
        }
        for (variant, parent) in &self.variant_parents {
            context.variant_parents.insert(*variant, *parent);
        }
        for (enum_def, variants) in &self.enum_variants {
            context
                .enum_variants
                .entry(*enum_def)
                .or_insert_with(|| variants.clone());
        }
        for (def, fields) in &self.variant_fields {
            let slot = context.variant_fields.entry(*def).or_default();
            for (name, ty) in fields {
                slot.entry(*name).or_insert(*ty);
            }
        }
        for (def, sig) in &self.functions {
            context.functions.entry(*def).or_insert_with(|| sig.clone());
        }
        context.method_defs.extend(self.method_defs.iter().copied());
        for (route, def) in &self.ad_routes {
            context.ad_routes.entry(route.clone()).or_insert(*def);
        }
    }
}

fn union_field_maps(
    dest: &mut HashMap<DefId, HashMap<Symbol, MirType>>,
    src: &HashMap<DefId, HashMap<Symbol, MirType>>,
) {
    for (def, fields) in src {
        let slot = dest.entry(*def).or_default();
        for (name, ty) in fields {
            slot.entry(*name).or_insert(*ty);
        }
    }
}

fn union_symbol_sets(
    dest: &mut HashMap<DefId, HashSet<Symbol>>,
    src: &HashMap<DefId, HashSet<Symbol>>,
) {
    for (def, fields) in src {
        dest.entry(*def).or_default().extend(fields.iter().copied());
    }
}

fn complete_def_remap(
    remap: &mut HashMap<DefId, DefId>,
    source_defs: impl IntoIterator<Item = DefId>,
    next_synthetic: &mut u32,
) {
    for def in source_defs {
        remap.entry(def).or_insert_with(|| {
            let id = DefId(*next_synthetic);
            *next_synthetic += 1;
            id
        });
    }
}

fn source_nominal_defs(source: &radix_mir::MirValidationContext<'_>) -> Vec<DefId> {
    let mut defs: Vec<DefId> = source.struct_fields.keys().copied().collect();
    defs.extend(source.enum_variants.keys().copied());
    defs.extend(source.variant_parents.values().copied());
    defs.extend(program_nominal_defs_from_types(source.types.as_table()));
    defs.sort_by_key(|def| def.0);
    defs.dedup();
    defs
}

fn program_nominal_defs_from_types(types: &TypeTable) -> Vec<DefId> {
    (0..types.type_count())
        .filter_map(
            |index| match types.get(TypeId(u32::try_from(index).unwrap_or(u32::MAX))) {
                Type::Struct(def_id) | Type::Enum(def_id) | Type::Interface(def_id) => {
                    Some(*def_id)
                }
                _ => None,
            },
        )
        .collect()
}

fn program_aggregate_defs(program: &MirProgram) -> (Vec<DefId>, Vec<DefId>) {
    let mut structs = Vec::new();
    let mut variants = Vec::new();
    for function in &program.functions {
        collect_function_aggregate_defs(function, &mut structs, &mut variants);
    }
    (structs, variants)
}

fn lowered_aggregate_defs(lowered: &LoweredMirUnit<'_>) -> (Vec<DefId>, Vec<DefId>) {
    let (mut structs, mut variants) = program_aggregate_defs(lowered.validated.program());
    for function in &lowered.generic_templates {
        collect_function_aggregate_defs(function, &mut structs, &mut variants);
    }
    (structs, variants)
}

fn collect_function_aggregate_defs(
    function: &MirFunction,
    structs: &mut Vec<DefId>,
    variants: &mut Vec<DefId>,
) {
    for block in &function.blocks {
        for statement in &block.statements {
            collect_statement_aggregate_defs(statement, structs, variants);
        }
        collect_terminator_aggregate_defs(&block.terminator, variants);
    }
}

fn collect_statement_aggregate_defs(
    statement: &MirStatement,
    structs: &mut Vec<DefId>,
    variants: &mut Vec<DefId>,
) {
    match &statement.kind {
        MirStatementKind::Construct {
            destination,
            aggregate,
        } => {
            collect_place_variant_defs(destination, variants);
            match aggregate.kind {
                MirAggregateKind::Struct(def_id) => structs.push(def_id),
                MirAggregateKind::EnumVariant(def_id) => variants.push(def_id),
                _ => {}
            }
            collect_aggregate_operand_variant_defs(aggregate, variants);
        }
        MirStatementKind::Assign { place, value } => {
            collect_place_variant_defs(place, variants);
            collect_value_variant_defs(value, variants);
        }
        MirStatementKind::Call {
            destination, args, ..
        } => {
            if let Some(destination) = destination {
                collect_place_variant_defs(destination, variants);
            }
            for arg in args {
                collect_operand_variant_defs(arg, variants);
            }
        }
        MirStatementKind::RuntimeCall { destination, call } => {
            if let Some(destination) = destination {
                collect_place_variant_defs(destination, variants);
            }
            for arg in &call.args {
                collect_operand_variant_defs(arg, variants);
            }
        }
    }
}

fn collect_terminator_aggregate_defs(terminator: &MirTerminator, variants: &mut Vec<DefId>) {
    match &terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            collect_operand_variant_defs(operand, variants);
        }
        MirTerminatorKind::TryCall {
            destination,
            args,
            error_place,
            ..
        } => {
            if let Some(destination) = destination {
                collect_place_variant_defs(destination, variants);
            }
            for arg in args {
                collect_operand_variant_defs(arg, variants);
            }
            collect_place_variant_defs(error_place, variants);
        }
        MirTerminatorKind::Branch { condition, .. } => {
            collect_operand_variant_defs(condition, variants);
        }
        MirTerminatorKind::Switch { value, .. } => collect_operand_variant_defs(value, variants),
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

fn collect_value_variant_defs(value: &MirValue, variants: &mut Vec<DefId>) {
    match &value.kind {
        MirValueKind::Operand(operand) | MirValueKind::Unary { operand, .. } => {
            collect_operand_variant_defs(operand, variants);
        }
        MirValueKind::Closure(closure) => {
            collect_operand_variant_defs(&closure.environment, variants);
        }
        MirValueKind::TensorExtent { tensor, .. } => collect_operand_variant_defs(tensor, variants),
        MirValueKind::Binary { lhs, rhs, .. } => {
            collect_operand_variant_defs(lhs, variants);
            collect_operand_variant_defs(rhs, variants);
        }
        MirValueKind::Option(op) => match op {
            MirOptionOp::None => {}
            MirOptionOp::Some(operand)
            | MirOptionOp::IsNil(operand)
            | MirOptionOp::IsNotNil(operand) => collect_operand_variant_defs(operand, variants),
            MirOptionOp::Unwrap { value, .. } => collect_operand_variant_defs(value, variants),
            MirOptionOp::Coalesce { value, fallback } => {
                collect_operand_variant_defs(value, variants);
                collect_operand_variant_defs(fallback, variants);
            }
            MirOptionOp::Chain { base, link } => {
                collect_operand_variant_defs(base, variants);
                match link {
                    MirOptionChainLink::VariantField { variant, .. } => variants.push(*variant),
                    MirOptionChainLink::Index(operand) => {
                        collect_operand_variant_defs(operand, variants);
                    }
                    MirOptionChainLink::Call { args, .. } => {
                        for arg in args {
                            collect_operand_variant_defs(arg, variants);
                        }
                    }
                    MirOptionChainLink::Field(_) => {}
                }
            }
        },
    }
}

fn collect_aggregate_operand_variant_defs(aggregate: &MirAggregate, variants: &mut Vec<DefId>) {
    match &aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        collect_operand_variant_defs(operand, variants);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                collect_operand_variant_defs(&item.value, variants);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                collect_operand_variant_defs(&item.key, variants);
                collect_operand_variant_defs(&item.value, variants);
            }
        }
    }
}

fn collect_operand_variant_defs(operand: &MirOperand, variants: &mut Vec<DefId>) {
    if let MirOperand::Place(place) = operand {
        collect_place_variant_defs(place, variants);
    }
}

fn collect_place_variant_defs(place: &MirPlace, variants: &mut Vec<DefId>) {
    for projection in &place.projections {
        match projection {
            MirProjection::VariantField { variant, .. } => variants.push(*variant),
            MirProjection::Index(operand) | MirProjection::ProvenIndex(operand) => {
                collect_operand_variant_defs(operand, variants);
            }
            MirProjection::Field(_)
            | MirProjection::ClosureCapture { .. }
            | MirProjection::VectorLane(_)
            | MirProjection::MatrixCell { .. }
            | MirProjection::SharedCell => {}
        }
    }
}

fn source_variant_defs(source: &radix_mir::MirValidationContext<'_>) -> Vec<DefId> {
    let mut defs: Vec<DefId> = source.variant_parents.keys().copied().collect();
    defs.extend(source.variant_fields.keys().copied());
    defs.sort_by_key(|def| def.0);
    defs.dedup();
    defs
}

#[derive(Clone, Copy)]
enum MetadataDefKind {
    /// Structs, enums, and interfaces use the nominal remap.
    Nominal,
    /// Enum variants use the variant remap, even when their source `DefId`
    /// also appears in the nominal remap.
    Variant,
}

/// Remap metadata in its consumer's identity domain.
///
/// A source analysis can expose one `DefId` for both a nominal and a variant:
/// variant lowering falls back to the ordinary name lookup when the
/// parent-qualified variant entry is unavailable, so a variant whose spelling
/// matches a struct can reuse that struct's id. The remap tables are already
/// separate `(DefId, kind)` domains; preserve that distinction at each
/// metadata consumer instead of collapsing them with precedence.
fn remap_metadata_def(
    def: DefId,
    kind: MetadataDefKind,
    struct_remap: &HashMap<DefId, DefId>,
    variant_remap: &HashMap<DefId, DefId>,
    nominal_map: &HashMap<DefId, TypeId>,
    entry_types: &TypeTable,
) -> DefId {
    let remap = match kind {
        MetadataDefKind::Nominal => struct_remap,
        MetadataDefKind::Variant => variant_remap,
    };
    if let Some(mapped) = remap.get(&def) {
        return *mapped;
    }
    if matches!(kind, MetadataDefKind::Nominal)
        && let Some(ty) = nominal_map.get(&def)
    {
        match entry_types.get(*ty) {
            Type::Struct(mapped) | Type::Enum(mapped) | Type::Interface(mapped) => {
                return *mapped;
            }
            _ => {}
        }
    }
    def
}

fn remap_metadata_symbol(symbol: Symbol, source: &Interner, target: &mut Interner) -> Symbol {
    target.intern(source.resolve(symbol))
}

fn remap_metadata_type(ty: MirType, type_rewrites: &[(TypeId, TypeId)]) -> MirType {
    let mut remapped = ty;
    rewrite_type_id(&mut remapped, type_rewrites);
    remapped
}

fn collect_remapped_validation(
    source: &radix_mir::MirValidationContext<'_>,
    source_interner: &Interner,
    entry_interner: &mut Interner,
    type_rewrites: &[(TypeId, TypeId)],
    struct_remap: &HashMap<DefId, DefId>,
    variant_remap: &HashMap<DefId, DefId>,
    nominal_map: &HashMap<DefId, TypeId>,
    entry_types: &TypeTable,
    source_rewrites: &SourceRewrites,
    unit_path: &Path,
) -> RemappedValidation {
    let remap_nominal_def = |def: DefId| {
        remap_metadata_def(
            def,
            MetadataDefKind::Nominal,
            struct_remap,
            variant_remap,
            nominal_map,
            entry_types,
        )
    };
    let remap_variant_def = |def: DefId| {
        remap_metadata_def(
            def,
            MetadataDefKind::Variant,
            struct_remap,
            variant_remap,
            nominal_map,
            entry_types,
        )
    };
    let remap_function_def = |def: DefId| {
        source_rewrites
            .get(&(unit_path.to_path_buf(), def))
            .copied()
            .unwrap_or_else(|| remap_nominal_def(def))
    };
    let mut out = RemappedValidation::default();
    remap_struct_and_variant_defs(
        source,
        source_interner,
        entry_interner,
        type_rewrites,
        remap_nominal_def,
        remap_variant_def,
        &mut out,
    );
    for (def, sig) in &source.functions {
        let mut sig = sig.clone();
        rewrite_type_id(&mut sig.return_ty, type_rewrites);
        if let Some(error_ty) = &mut sig.error_ty {
            rewrite_type_id(error_ty, type_rewrites);
        }
        for param in &mut sig.params {
            rewrite_type_id(&mut param.ty, type_rewrites);
        }
        out.functions.entry(remap_function_def(*def)).or_insert(sig);
    }
    for def in &source.method_defs {
        out.method_defs.insert(remap_function_def(*def));
    }
    for (route, def) in &source.ad_routes {
        out.ad_routes
            .entry(route.clone())
            .or_insert_with(|| remap_function_def(*def));
    }
    out
}

fn remap_struct_and_variant_defs(
    source: &radix_mir::MirValidationContext<'_>,
    source_interner: &Interner,
    entry_interner: &mut Interner,
    type_rewrites: &[(TypeId, TypeId)],
    remap_nominal_def: impl Fn(DefId) -> DefId + Copy,
    remap_variant_def: impl Fn(DefId) -> DefId + Copy,
    out: &mut RemappedValidation,
) {
    for (def, fields) in &source.struct_fields {
        let dest = remap_nominal_def(*def);
        let slot = out.struct_fields.entry(dest).or_default();
        for (name, ty) in fields {
            slot.insert(
                remap_metadata_symbol(*name, source_interner, entry_interner),
                remap_metadata_type(*ty, type_rewrites),
            );
        }
    }
    for (def, order) in &source.struct_field_order {
        out.struct_field_order.insert(
            remap_nominal_def(*def),
            order
                .iter()
                .map(|name| remap_metadata_symbol(*name, source_interner, entry_interner))
                .collect(),
        );
    }
    for (def, order) in &source.variant_field_order {
        out.variant_field_order.insert(
            remap_variant_def(*def),
            order
                .iter()
                .map(|name| remap_metadata_symbol(*name, source_interner, entry_interner))
                .collect(),
        );
    }
    for (def, name) in &source.display_names {
        let dest = if source.variant_parents.contains_key(def) {
            remap_variant_def(*def)
        } else {
            remap_nominal_def(*def)
        };
        out.display_names.insert(dest, name.clone());
    }
    // Generic struct params must ride the same remap: field projections
    // through an `Applied` sibling/library struct (namespace-forwarded
    // `ns.load<…>(…)` results) instantiate against these bindings.
    for (def, params) in &source.struct_type_params {
        out.struct_type_params
            .entry(remap_nominal_def(*def))
            .or_insert_with(|| {
                params
                    .iter()
                    .map(|param| remap_metadata_symbol(*param, source_interner, entry_interner))
                    .collect()
            });
    }
    // A contract is a nominal like the struct that names it: both sides ride
    // the nominal remap so the conformance survives into the entry's space.
    for (def, contracts) in &source.struct_contracts {
        out.struct_contracts
            .entry(remap_nominal_def(*def))
            .or_default()
            .extend(
                contracts
                    .iter()
                    .map(|contract| remap_nominal_def(*contract)),
            );
    }
    for (def, fields) in &source.optional_struct_fields {
        let dest = remap_nominal_def(*def);
        out.optional_struct_fields.entry(dest).or_default().extend(
            fields
                .iter()
                .map(|name| remap_metadata_symbol(*name, source_interner, entry_interner)),
        );
    }
    for (def, fields) in &source.nullable_struct_fields {
        let dest = remap_nominal_def(*def);
        out.nullable_struct_fields.entry(dest).or_default().extend(
            fields
                .iter()
                .map(|name| remap_metadata_symbol(*name, source_interner, entry_interner)),
        );
    }
    for (variant, parent) in &source.variant_parents {
        out.variant_parents
            .entry(remap_variant_def(*variant))
            .or_insert(remap_nominal_def(*parent));
    }
    for (enum_def, variants) in &source.enum_variants {
        out.enum_variants
            .entry(remap_nominal_def(*enum_def))
            .or_insert_with(|| variants.iter().copied().map(remap_variant_def).collect());
    }
    for (def, fields) in &source.variant_fields {
        let dest = remap_variant_def(*def);
        let slot = out.variant_fields.entry(dest).or_default();
        for (name, ty) in fields {
            slot.insert(
                remap_metadata_symbol(*name, source_interner, entry_interner),
                remap_metadata_type(*ty, type_rewrites),
            );
        }
    }
}

#[cfg(test)]
#[path = "lower_test.rs"]
mod tests;

pub(super) fn lower_package_units<'a>(
    package: &'a mut AnalyzedProgram,
    entry_id: &ModuleId,
    links: &PackageMirLinks,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    cli_plan: &CliPackagePlan,
    no_fuse: bool,
    kernel_lowering: KernelLowering,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    struct PendingUnit<'a> {
        lowered: LoweredMirUnit<'a>,
        dispatch_function: Option<MirFunctionId>,
    }

    // S1 U3 reachability seed: the package's rewritten unit HIR references
    // linked library functions through package-MIR synthetic def ids
    // (`Path(synthetic)` namespace-call targets and const data-member
    // references). That set seeds the library-function reachability closure
    // below, so linked library functions are lowered only when reachable from
    // the entry's call graph (default = pruning, delivery §4.3).
    let mut reachable_seed = collect_package_synthetic_references(package);
    collect_tensor_edge_policy_callback_references(
        package,
        &links.method_targets,
        &mut reachable_seed,
    );

    let other_ids = package
        .nodes
        .keys()
        .filter(|id| *id != entry_id)
        .cloned()
        .collect::<Vec<_>>();
    let mut others = BTreeMap::new();
    for id in other_ids {
        if let Some(node) = package.nodes.remove(&id) {
            others.insert(id, node);
        }
    }
    let Some(entry) = package.nodes.get_mut(entry_id) else {
        return Err(vec![crate::package_diagnostic_error(
            "internal invariant: entry id selected from program nodes",
        )]);
    };
    // Linked const data members (sibling units + libraries) are transplanted
    // into the entry analysis before the entry lowers: the entry's rewritten
    // `Path(synthetic_def)` member references then materialize through the
    // entry's existing top-level-const seam.
    inject_package_data_members(
        &entry.path,
        &mut entry.analysis,
        others.values(),
        links,
        library_resolver,
        library_cache,
    )?;
    let entry_imported_function_params = imported_function_params_for_caller(
        &entry.path,
        &mut entry.analysis,
        others.values(),
        links,
        library_resolver,
        library_cache,
    )?;
    let entry_path = entry.path.clone();
    // The same rewrite also fires inside SIBLING units: a reader unit's
    // function body referencing another unit's exported const
    // (`k between keys.KEYS`) was rewritten to `Path(synthetic)`, but the
    // synthetic const item only exists in the entry analysis — the reader
    // then lowers "path that does not resolve to a local value". Transplant
    // the linked members into every sibling reader before it lowers, so the
    // rewritten path materializes through that unit's own top-level-const
    // seam (same data-member ABI as the entry).
    let data_member_reader_paths: HashSet<&PathBuf> = links
        .data_member_targets
        .keys()
        .map(|(caller_path, _, _)| caller_path)
        .filter(|caller_path| **caller_path != entry_path)
        .collect();
    let sibling_reader_ids: Vec<ModuleId> = others
        .iter()
        .filter(|(_, unit)| data_member_reader_paths.contains(&unit.path))
        .map(|(id, _)| id.clone())
        .collect();
    for id in sibling_reader_ids {
        let Some((_, mut reader)) = others.remove_entry(&id) else {
            return Err(vec![crate::package_diagnostic_error(
                "internal invariant: reader id from map",
            )]);
        };
        let injection = inject_package_data_members(
            &reader.path,
            &mut reader.analysis,
            others.values(),
            links,
            library_resolver,
            library_cache,
        );
        others.insert(id, reader);
        injection?;
    }
    // Cached library analyses sit outside `others`, but their namespace
    // const reads use the same synthetic paths. Inject while package source
    // analyses are still borrowable; pending sibling MIR retains their borrows.
    for library in &links.libraries {
        if !data_member_reader_paths.contains(&library.path) {
            continue;
        }
        let mut diagnostics = Vec::new();
        with_library_cached_analysis_mut(
            &library.import,
            library_resolver,
            library_cache,
            |analysis, cache| {
                if let Err(errors) = inject_package_data_members(
                    &library.path,
                    analysis,
                    std::iter::once(&*entry).chain(others.values()),
                    links,
                    library_resolver,
                    cache,
                ) {
                    diagnostics.extend(errors);
                }
                Ok(())
            },
        )
        .map_err(|diagnostic| vec![diagnostic])?;
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
    }
    // Transplant defaults into each sibling caller before lowering begins.
    // The lowered units retain borrows into their map nodes until package
    // merging, so keep those owners in `others` while doing this id-driven
    // remove/inspect/reinsert pass.
    let sibling_ids: Vec<ModuleId> = others.keys().cloned().collect();
    let mut sibling_imported_function_params = HashMap::new();
    for id in sibling_ids {
        let Some((_, mut unit)) = others.remove_entry(&id) else {
            continue;
        };
        let unit_path = unit.path.clone();
        let imported_function_params = imported_function_params_for_caller(
            &unit_path,
            &mut unit.analysis,
            std::iter::once(&*entry).chain(others.values()),
            links,
            library_resolver,
            library_cache,
        )?;
        others.insert(id, unit);
        sibling_imported_function_params.insert(unit_path, imported_function_params);
    }

    let mut pending = Vec::new();
    let mut extra_validation = RemappedValidation::default();
    let mut shared_nominals: HashMap<(InterfaceNominalKind, String), TypeId> = HashMap::new();
    // Imported-device routes joined to synthetic template sources (KPC-WIRE):
    // seeded from the linker's library join, extended by each lowered unit's
    // module-owned route map.
    let mut imported_device_routes = links.imported_device_routes.clone();
    // CDC-1: the consumer units' own device roles (full role map, private
    // helpers included) and public Kernel launch-entry sources, keyed by the
    // merged program's source DefIds. Collected at each unit's `lower_unit`
    // site while its analysis is freely borrowable; consumed by the
    // package-seam composition as the leaf-tail union with the routes.
    let mut consumer_device_roles: HashMap<DefId, radix_mir::MirDeviceRole> = HashMap::default();
    let mut consumer_launch_entries: HashSet<DefId> = HashSet::default();
    // Mutable copy of the link sources: the library-lowering pass extends it
    // with synthetic ids for auto-generated sub-companion sources.
    let mut source_rewrites = links.sources.clone();
    let mut next_synthetic = links.next_synthetic;

    for unit in others.values_mut() {
        let unit_path = unit.path.clone();
        let imported_function_params = sibling_imported_function_params
            .get(&unit_path)
            .cloned()
            .unwrap_or_else(|| std::sync::Arc::new(HashMap::new()));
        let source_exports = exported_nominal_names(&unit.file_interface);
        // S1 U2 nominal identity: build the canonical nominal remap while the
        // source analysis is freely borrowable (lowering mutates it below).
        let nominal_map = match build_nominal_remap(
            &unit.analysis.types,
            &unit.analysis.interner,
            &unit.analysis.resolver,
            unit.file_interface.identity.as_ref(),
            &source_exports,
            &entry.analysis,
            &shared_nominals,
        ) {
            Ok(map) => map,
            Err(message) => {
                return Err(vec![mir_issue_diag(
                    &unit_path,
                    "package_mir_nominal_identity_unknown",
                    message,
                )]);
            }
        };
        // S1 U2 VALUE members: variant definitions ride the same remap.
        let variant_remap = match build_variant_remap(
            &unit.analysis.interner,
            &unit.analysis.resolver,
            unit.file_interface.identity.as_ref(),
            &source_exports,
            &entry.analysis,
            &shared_nominals,
        ) {
            Ok(remap) => remap,
            Err(message) => {
                return Err(vec![mir_issue_diag(
                    &unit_path,
                    "package_mir_variant_identity_unknown",
                    message,
                )]);
            }
        };
        let source_names = source_nominal_names(
            &unit.analysis.types,
            &unit.analysis.interner,
            &unit.analysis.resolver,
        );
        // CDC-1: collect this sibling unit's own device facts before
        // lowering takes the mutable analysis borrow. Keys remap through the
        // same `source_rewrites` map `rewrite_program_sources` applies to
        // this unit's lowered program, so they join the merged program's
        // synthetic template sources.
        if kernel_lowering == KernelLowering::Device {
            collect_consumer_device_facts(
                &unit.analysis,
                &unit_path,
                Some(&source_rewrites),
                &mut consumer_device_roles,
                &mut consumer_launch_entries,
            );
        }
        source_rewrites.extend(imported_method_source_rewrites(
            &unit.analysis,
            &unit_path,
            &links.method_targets,
        ));
        let mut lowered = lower_unit(
            unit,
            &cli_plan.entry_records,
            no_fuse,
            kernel_lowering,
            &imported_function_params,
        )?;
        // Remap from the post-lowering table: lowering appends runtime and
        // diagnostic text (format templates, the lockstep length-mismatch
        // message, …) that the pre-lowering analysis interner never held
        // (B11).
        let source_interner = lowered.interner.clone();
        let lowered_source_types = TypeTable::from_snapshot(
            lowered.validated.validation().types.snapshot(),
        )
        .map_err(|message| {
            vec![mir_issue_diag(
                &unit_path,
                "package_mir_source_types_snapshot",
                message,
            )]
        })?;
        // KPC-WIRE (c): this unit's module-owned route map was transferred
        // into the lowered unit instead of being dropped; join it to the
        // linker's synthetic template sources for this caller now, while the
        // unit path is in hand.
        imported_device_routes.extend(joined_imported_device_routes(
            &lowered.imported_device_routes,
            links,
            &unit_path,
        ));
        lowered.imported_device_routes.clear();
        // Sibling struct/union literals constructed in the source analysis
        // carry the source `DefId` in aggregate kinds and interned types; the
        // merged program's metadata is keyed by the entry's canonical nominal
        // def, so leftover unions ride the same (kind, name) remap instead of
        // intern-colliding on source-local DefIds.
        let mut nominal_def_remap = build_nominal_def_remap(&nominal_map, &entry.analysis.types);
        let (program_structs, program_variants) = lowered_aggregate_defs(&lowered);
        complete_def_remap(
            &mut nominal_def_remap,
            source_nominal_defs(lowered.validated.validation())
                .into_iter()
                .chain(program_structs),
            &mut next_synthetic,
        );
        let mut variant_remap = variant_remap;
        complete_def_remap(
            &mut variant_remap,
            source_variant_defs(lowered.validated.validation())
                .into_iter()
                .chain(program_variants),
            &mut next_synthetic,
        );
        let mut def_remap = nominal_def_remap.clone();
        def_remap.extend(variant_remap.iter().map(|(key, dest)| (*key, *dest)));
        let param_symbols = super::intern_param_symbols(
            lowered.validated.validation().types.as_table(),
            &lowered.interner,
            &mut entry.analysis.interner,
        );
        let record_field_symbols = super::intern_record_field_symbols(
            lowered.validated.validation().types.as_table(),
            &lowered.interner,
            &mut entry.analysis.interner,
        );
        let source_to_entry_types = {
            let nominal = NominalImportContext {
                nominal_map: &nominal_map,
                def_remap: &def_remap,
                source_names: &source_names,
                shared_nominals: &shared_nominals,
                param_symbols: &param_symbols,
                record_field_symbols: &record_field_symbols,
            };
            import_lowered_semantic_types(
                lowered.validated.validation().types.as_table(),
                &mut entry.analysis.types,
                Some(&nominal),
            )
        };
        record_shared_nominals(
            &source_names,
            lowered.validated.validation().types.as_table(),
            &source_to_entry_types,
            &mut shared_nominals,
        );
        let validation = collect_remapped_validation(
            lowered.validated.validation(),
            &source_interner,
            &mut entry.analysis.interner,
            &source_to_entry_types,
            &nominal_def_remap,
            &variant_remap,
            &nominal_map,
            &entry.analysis.types,
            &source_rewrites,
            &unit_path,
        );
        lowered = remap_lowered_program_text_symbols(
            lowered,
            &source_interner,
            &mut entry.analysis.interner,
            &unit_path,
        )?;
        lowered = remap_templates_text_symbols(lowered, &mut entry.analysis.interner);
        lowered = rewrite_lowered_type_ids(
            lowered,
            &source_to_entry_types,
            &entry.analysis.types,
            &variant_remap,
            &nominal_def_remap,
            &validation,
            &unit_path,
        )?;
        if let Some(rewrite) = cli_plan
            .dispatch
            .as_ref()
            .filter(|dispatch| dispatch.unit_path == unit_path)
            .and_then(|dispatch| dispatch.record_type_rewrite.as_ref())
        {
            let dispatch_rewrites =
                imported_dispatch_type_rewrites(rewrite, &source_to_entry_types);
            lowered = rewrite_lowered_type_ids(
                lowered,
                &dispatch_rewrites,
                &entry.analysis.types,
                &variant_remap,
                &nominal_def_remap,
                &validation,
                &unit_path,
            )?;
        }
        lowered = rewrite_lowered_shape_size_indices(
            lowered,
            &lowered_source_types,
            &mut entry.analysis.types,
            &param_symbols,
            &record_field_symbols,
            &validation,
            &unit_path,
        )?;
        lowered = rewrite_lowered_variant_defs(lowered, &variant_remap, &unit_path);
        lowered = rewrite_lowered_struct_defs(lowered, &nominal_def_remap, &unit_path);
        lowered =
            rewrite_lowered_program_sources(lowered, &unit_path, &source_rewrites, &unit_path)?;
        lowered = rewrite_generic_identity(
            lowered,
            &unit_path,
            &source_rewrites,
            &nominal_def_remap,
            links.method_targets.get(&unit_path),
        );
        ensure_unique_definition_sources(lowered.validated.program(), &unit_path)?;
        extra_validation.merge(&validation);
        let dispatch_function = selected_cli_dispatch_function(
            cli_plan,
            &unit_path,
            lowered.validated.program(),
            &source_interner,
            &entry.analysis.interner,
        );
        pending.push(PendingUnit {
            lowered,
            dispatch_function,
        });
    }

    // Lower linked library modules before the entry lowers, so their symbols
    // and types can be remapped into the entry's tables without aliasing the
    // merged program. The lowered library programs are owned (functions +
    // closure environments); the validation token is rebuilt on the merged
    // program below.
    let mut library_parts: Vec<LibraryMirParts> = Vec::new();
    // Display tokens of the file that defines each generic template, keyed by
    // the template's source DefId (stable across id shifts and pruning).
    let mut template_tokens: HashMap<
        DefId,
        radix_module::runtime_contract::display_tokens::DisplayTokens,
    > = HashMap::new();
    for library in &links.libraries {
        let mut diagnostics = Vec::new();
        // Read the library's own file interface before its cache entry is
        // lent out below: while the entry is lent, a lookup of this very key
        // misses and would re-analyze the whole library.
        let cached_interface =
            library_cached_file_interface(&library.import, library_resolver, library_cache).ok();
        with_library_cached_analysis_mut(
            &library.import,
            library_resolver,
            library_cache,
            |analysis, cache| {
                if let Err(errors) = rewrite_analysis_namespace_calls(
                    &library.path,
                    analysis,
                    &links.calls,
                    &links.data_member_targets,
                    &links.namespaces,
                    &links.method_targets,
                ) {
                    diagnostics.extend(errors);
                    return Ok(());
                }
                // S1 U2 nominal identity: build the canonical nominal remap
                // while the library analysis is freely borrowable (the entry
                // registered the library's file interface under the same
                // module identity during analysis).
                let source_identity = cached_interface
                    .as_ref()
                    .and_then(|interface| interface.identity.clone());
                let source_exports = cached_interface
                    .as_ref()
                    .map(exported_nominal_names)
                    .unwrap_or_default();
                let nominal_map = match build_nominal_remap(
                    &analysis.types,
                    &analysis.interner,
                    &analysis.resolver,
                    source_identity.as_ref(),
                    &source_exports,
                    &entry.analysis,
                    &shared_nominals,
                ) {
                    Ok(map) => map,
                    Err(message) => {
                        diagnostics.push(mir_issue_diag(
                            &library.path,
                            "package_mir_nominal_identity_unknown",
                            message,
                        ));
                        return Ok(());
                    }
                };
                // S1 U2 VALUE members: variant definitions ride the same remap.
                let variant_remap = match build_variant_remap(
                    &analysis.interner,
                    &analysis.resolver,
                    source_identity.as_ref(),
                    &source_exports,
                    &entry.analysis,
                    &shared_nominals,
                ) {
                    Ok(remap) => remap,
                    Err(message) => {
                        diagnostics.push(mir_issue_diag(
                            &library.path,
                            "package_mir_variant_identity_unknown",
                            message,
                        ));
                        return Ok(());
                    }
                };
                let source_names =
                    source_nominal_names(&analysis.types, &analysis.interner, &analysis.resolver);
                let bundle = match super::air_backward::prepare(analysis) {
                    Ok(bundle) => bundle,
                    Err(message) => {
                        diagnostics.push(mir_lowering_diag(&library.path, message));
                        return Ok(());
                    }
                };
                let imported_function_params = match imported_function_params_for_caller(
                    &library.path,
                    analysis,
                    std::iter::empty::<&AnalyzedProgramNode>(),
                    links,
                    library_resolver,
                    cache,
                ) {
                    Ok(parameters) => parameters,
                    Err(errors) => {
                        diagnostics.extend(errors);
                        return Ok(());
                    }
                };
                let mut lowered = match lower_analyzed_unit(
                    analysis.application_view(),
                    radix_mir_lower::lower::MirLoweringOptions {
                        kernel_lowering,
                        imported_function_params: std::sync::Arc::new(
                            imported_function_params
                                .iter()
                                .map(|(def, params)| (*def, params.clone()))
                                .collect(),
                        ),
                        ..Default::default()
                    },
                ) {
                    Ok(lowered) => lowered,
                    Err(errors) => {
                        diagnostics.extend(
                            errors
                                .into_iter()
                                .map(|error| mir_error_diag(&library.path, error)),
                        );
                        return Ok(());
                    }
                };
                let lowered_source_types =
                    match TypeTable::from_snapshot(lowered.validated.validation().types.snapshot())
                    {
                        Ok(types) => types,
                        Err(message) => {
                            diagnostics.push(mir_lowering_diag(&library.path, message));
                            return Ok(());
                        }
                    };
                if let Some(bundle) = bundle {
                    lowered = match super::air_backward::apply(lowered, bundle, analysis, no_fuse) {
                        Ok(lowered) => lowered,
                        Err(message) => {
                            diagnostics.push(mir_lowering_diag(&library.path, message));
                            return Ok(());
                        }
                    };
                }
                let source_interner = lowered.interner.clone();
                let mut nominal_def_remap =
                    build_nominal_def_remap(&nominal_map, &entry.analysis.types);
                let (program_structs, program_variants) = lowered_aggregate_defs(&lowered);
                complete_def_remap(
                    &mut nominal_def_remap,
                    source_nominal_defs(lowered.validated.validation())
                        .into_iter()
                        .chain(program_structs),
                    &mut next_synthetic,
                );
                let mut variant_remap = variant_remap;
                complete_def_remap(
                    &mut variant_remap,
                    source_variant_defs(lowered.validated.validation())
                        .into_iter()
                        .chain(program_variants),
                    &mut next_synthetic,
                );
                let mut def_remap = nominal_def_remap.clone();
                def_remap.extend(variant_remap.iter().map(|(key, dest)| (*key, *dest)));
                let param_symbols = super::intern_param_symbols(
                    lowered.validated.validation().types.as_table(),
                    &lowered.interner,
                    &mut entry.analysis.interner,
                );
                let record_field_symbols = super::intern_record_field_symbols(
                    lowered.validated.validation().types.as_table(),
                    &lowered.interner,
                    &mut entry.analysis.interner,
                );
                let source_to_entry_types = {
                    let nominal = NominalImportContext {
                        nominal_map: &nominal_map,
                        def_remap: &def_remap,
                        source_names: &source_names,
                        shared_nominals: &shared_nominals,
                        param_symbols: &param_symbols,
                        record_field_symbols: &record_field_symbols,
                    };
                    import_lowered_semantic_types(
                        lowered.validated.validation().types.as_table(),
                        &mut entry.analysis.types,
                        Some(&nominal),
                    )
                };
                record_shared_nominals(
                    &source_names,
                    lowered.validated.validation().types.as_table(),
                    &source_to_entry_types,
                    &mut shared_nominals,
                );
                extend_unmapped_library_sources(
                    lowered.validated.program(),
                    &library.path,
                    &mut source_rewrites,
                    &mut next_synthetic,
                );
                let validation = collect_remapped_validation(
                    lowered.validated.validation(),
                    &source_interner,
                    &mut entry.analysis.interner,
                    &source_to_entry_types,
                    &nominal_def_remap,
                    &variant_remap,
                    &nominal_map,
                    &entry.analysis.types,
                    &source_rewrites,
                    &library.path,
                );
                lowered = match remap_lowered_program_text_symbols(
                    lowered,
                    &source_interner,
                    &mut entry.analysis.interner,
                    &library.path,
                ) {
                    Ok(lowered) => lowered,
                    Err(errors) => {
                        diagnostics.extend(errors);
                        return Ok(());
                    }
                };
                lowered = remap_templates_text_symbols(lowered, &mut entry.analysis.interner);
                lowered = match rewrite_lowered_type_ids(
                    lowered,
                    &source_to_entry_types,
                    &entry.analysis.types,
                    &variant_remap,
                    &nominal_def_remap,
                    &validation,
                    &library.path,
                ) {
                    Ok(lowered) => lowered,
                    Err(errors) => {
                        diagnostics.extend(errors);
                        return Ok(());
                    }
                };
                lowered = match rewrite_lowered_shape_size_indices(
                    lowered,
                    &lowered_source_types,
                    &mut entry.analysis.types,
                    &param_symbols,
                    &record_field_symbols,
                    &validation,
                    &library.path,
                ) {
                    Ok(lowered) => lowered,
                    Err(errors) => {
                        diagnostics.extend(errors);
                        return Ok(());
                    }
                };
                lowered = rewrite_lowered_variant_defs(lowered, &variant_remap, &library.path);
                lowered = rewrite_lowered_struct_defs(lowered, &nominal_def_remap, &library.path);
                lowered = match rewrite_lowered_program_sources(
                    lowered,
                    &library.path,
                    &source_rewrites,
                    &library.path,
                ) {
                    Ok(lowered) => lowered,
                    Err(errors) => {
                        diagnostics.extend(errors);
                        return Ok(());
                    }
                };
                lowered = rewrite_generic_identity(
                    lowered,
                    &library.path,
                    &source_rewrites,
                    &nominal_def_remap,
                    links.method_targets.get(&library.path),
                );
                if let Err(errors) =
                    ensure_unique_definition_sources(lowered.validated.program(), &library.path)
                {
                    diagnostics.extend(errors);
                    return Ok(());
                }
                extra_validation.merge(&validation);
                let (library_program, library_context) = lowered.validated.into_parts();
                record_template_display_tokens(
                    &mut template_tokens,
                    &lowered.generic_templates,
                    &lowered.display_tokens,
                );
                library_parts.push(LibraryMirParts {
                    program: library_program,
                    display_tokens: library_context.display_tokens.clone(),
                    environments: lowered.closure_environments,
                    templates: lowered.generic_templates,
                    generic_params: lowered.generic_params,
                    tensor_edge_policy_metadata: lowered.tensor_edge_policy_metadata,
                    device_function_metadata: lowered.device_function_metadata,
                });
                Ok(())
            },
        )
        .map_err(|diag| vec![diag])?;
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
    }

    // S1 U3 reachability (default = pruning): linked library functions that
    // are unreachable from the package's synthetic call graph are dropped
    // before they merge into the program (delivery §4.3).
    // A custom limes provider is a static dispatch edge rather than an
    // ordinary MIR call in the generic template. Keep its checked callback
    // bodies alive until package specialization replaces the template's
    // placeholder with concrete callback MIDs.
    for library in &library_parts {
        for provider in &library.tensor_edge_policy_metadata.providers {
            if let MirTensorEdgePolicySelector::Custom {
                remap, transform, ..
            } = provider.selector
            {
                reachable_seed.insert(remap);
                reachable_seed.extend(transform);
            }
        }
    }
    prune_unreachable_library_functions(&mut library_parts, &mut reachable_seed);
    let Some(entry) = package.nodes.get_mut(entry_id) else {
        return Err(vec![crate::package_diagnostic_error(
            "internal invariant: entry id selected from program nodes",
        )]);
    };

    // CDC-1: the entry unit's own device facts join the consumer-local
    // collection the same way (identity-keyed: the merged program never
    // source-rewrites the entry's own definitions).
    if kernel_lowering == KernelLowering::Device {
        collect_consumer_device_facts(
            &entry.analysis,
            &entry_path,
            None,
            &mut consumer_device_roles,
            &mut consumer_launch_entries,
        );
    }
    // The entry's own definitions are never source-rewritten, but a contract
    // dispatch arm calls an imported method by its consumer-local identity.
    let entry_method_rewrites: SourceRewrites =
        imported_method_source_rewrites(&entry.analysis, &entry_path, &links.method_targets)
            .into_iter()
            .collect();
    let mut merged = lower_unit(
        entry,
        &cli_plan.entry_records,
        no_fuse,
        kernel_lowering,
        &entry_imported_function_params,
    )?;
    if !entry_method_rewrites.is_empty() {
        merged = rewrite_lowered_program_sources(
            merged,
            &entry_path,
            &entry_method_rewrites,
            &entry_path,
        )?;
    }
    ensure_unique_definition_sources(merged.validated.program(), &entry_path)?;
    // Generic limes templates from future library units are already offered
    // to the incremental linker below. Make their portable policy sites
    // visible at the same time, so a template cannot be specialized with its
    // default Clamp placeholder before the defining library is appended.
    for library in &library_parts {
        merge_tensor_edge_policy_metadata(
            &mut merged.tensor_edge_policy_metadata,
            &library.tensor_edge_policy_metadata,
        );
    }
    for unit in &pending {
        merge_tensor_edge_policy_metadata(
            &mut merged.tensor_edge_policy_metadata,
            &unit.lowered.tensor_edge_policy_metadata,
        );
    }
    // KPC-WIRE (c): the entry's module-owned route map joins the same way as
    // the siblings'; the transferred map is consumed exactly once.
    imported_device_routes.extend(joined_imported_device_routes(
        &merged.imported_device_routes,
        links,
        &entry_path,
    ));
    merged.imported_device_routes.clear();
    let mut dispatch_function = selected_cli_dispatch_function(
        cli_plan,
        &entry_path,
        merged.validated.program(),
        &merged.interner,
        &merged.interner,
    );

    let mut generic_templates = std::mem::take(&mut merged.generic_templates);
    let mut generic_params = std::mem::take(&mut merged.generic_params);
    record_template_display_tokens(
        &mut template_tokens,
        &generic_templates,
        &merged.display_tokens,
    );
    // Host kernel lowering has no device routes: imported kernels are plain
    // functions, so the composition inputs stay empty.
    let imported_device_index = match kernel_lowering {
        KernelLowering::Device => {
            radix_mir_lower::lower::ImportedDeviceIndex::from_routes(imported_device_routes.clone())
        }
        KernelLowering::Host => radix_mir_lower::lower::ImportedDeviceIndex::default(),
    };
    let mut planning_program = merged.validated.program().clone();
    let mut planning_environments = merged.closure_environments.clone();
    for unit in &pending {
        let mut program = unit.lowered.validated.program().clone();
        let mut environments = unit.lowered.closure_environments.clone();
        append_shifted_parts(
            &mut planning_program,
            &mut planning_environments,
            &mut program,
            &mut environments,
            &entry_path,
        )?;
    }
    let mut future_library_templates = Vec::new();
    let mut future_library_generic_params = Vec::new();
    for library in &library_parts {
        let mut program = library.program.clone();
        let mut environments = library.environments.clone();
        let offset = append_shifted_parts(
            &mut planning_program,
            &mut planning_environments,
            &mut program,
            &mut environments,
            &entry_path,
        )?;
        let mut templates = library.templates.clone();
        // A template body that names a function of its own library (a
        // synthetic failure-conversion helper behind `⇥`) cannot be
        // instantiated before that library is appended: the instance would
        // call a function id the merged program does not hold yet, and the
        // merge validation rejects it. Such templates instantiate when their
        // library is appended, which sees the whole merged program including
        // the entry and sibling call sites.
        templates.retain(|template| {
            scan_mir_function_body_references(template)
                .functions
                .is_empty()
        });
        shift_templates(&mut templates, offset);
        future_library_templates.extend(templates);
        future_library_generic_params.extend(library.generic_params.clone());
    }
    for unit in pending {
        let PendingUnit {
            lowered,
            dispatch_function: unit_dispatch,
        } = unit;
        let offset = MirProgram::allocate_function_id(&merged.validated.program().functions)
            .map_err(|_| {
                vec![function_id_overflow_diag(
                    &entry_path,
                    "package MIR function id space is exhausted",
                )]
            })?
            .0;
        record_template_display_tokens(
            &mut template_tokens,
            &lowered.generic_templates,
            &lowered.display_tokens,
        );
        let mut available_templates = generic_templates.clone();
        available_templates.extend(future_library_templates.iter().cloned());
        let mut available_generic_params = generic_params.clone();
        available_generic_params.extend(future_library_generic_params.iter().cloned());
        let (next, mut incoming) = append_shifted_program(
            merged,
            lowered,
            &entry_path,
            &extra_validation,
            &available_templates,
            &available_generic_params,
            &imported_device_index,
            &consumer_device_roles,
            &consumer_launch_entries,
        )?;
        merged = next;
        shift_templates(&mut incoming.templates, offset);
        generic_templates.append(&mut incoming.templates);
        generic_params.append(&mut incoming.generic_params);
        if let Some(local_id) = unit_dispatch {
            dispatch_function = Some(checked_shifted_function_id(local_id, offset, &entry_path)?);
        }
        ensure_unique_definition_sources(merged.validated.program(), &entry_path)?;
    }
    for library in library_parts {
        let (next, mut library, offset) = append_shifted_library_parts(
            merged,
            library,
            &entry_path,
            &extra_validation,
            &generic_templates,
            &generic_params,
            &imported_device_index,
            &consumer_device_roles,
            &consumer_launch_entries,
        )?;
        merged = next;
        shift_templates(&mut library.templates, offset);
        generic_templates.append(&mut library.templates);
        generic_params.append(&mut library.generic_params);
        ensure_unique_definition_sources(merged.validated.program(), &entry_path)?;
    }
    merged = instantiate_package_generics(
        merged,
        generic_templates,
        generic_params,
        &entry_path,
        Some(&extra_validation),
        &imported_device_index,
        &consumer_device_roles,
        &consumer_launch_entries,
    )?;
    merged = rebuild_merged_validated(
        merged,
        &entry_path,
        Some(&extra_validation),
        &template_tokens,
    )?;

    if cli_plan.dispatch.is_some() {
        let Some(function) = dispatch_function else {
            return Err(vec![mir_diag(
                &entry_path,
                "package MIR could not find selected CLI command function",
            )]);
        };
        merged = install_cli_dispatch_entry(merged, function, &entry_path)?;
    }

    Ok(merged)
}

/// Transplant linked const data members into a target unit's analysis before
/// that unit lowers (S1 data-member ABI). Entry, sibling, and library readers
/// use the same bindings for const reads rewritten to synthetic paths.
///
/// Each linked const is re-interned into the target interner and its type is
/// imported into the target type table, then injected as a synthetic
/// `HirItemKind::Constant` item under the member's synthetic def id. The
/// target lowering context then records it in `top_level_consts`, so rewritten
/// `Path(synthetic_def)` member references materialize through the existing
/// top-level-const seam.
fn inject_package_data_members<'a>(
    target_path: &Path,
    target: &mut AnalyzedModule,
    others: impl IntoIterator<Item = &'a AnalyzedProgramNode>,
    links: &PackageMirLinks,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<(), Vec<Diagnostic>> {
    let others = others.into_iter().collect::<Vec<_>>();
    let mut diagnostics = Vec::new();
    // Const-to-const references inside linked initializers (`ELEMENTS ← N *
    // K`) rewrite through the linked-const entries of `links.sources`. The
    // map keeps only synthetic ids that belong to a transplanted const data
    // member — `links.sources` also rewrites linked functions, and a const
    // initializer must not silently reference one of those.
    let const_members: HashSet<DefId> = links
        .data_members
        .iter()
        .map(|member| member.synthetic)
        .collect();
    let mut const_path_rewrites: HashMap<&PathBuf, HashMap<DefId, DefId>> = HashMap::new();
    for ((source_path, def_id), synthetic) in &links.sources {
        if const_members.contains(synthetic) {
            const_path_rewrites
                .entry(source_path)
                .or_default()
                .insert(*def_id, *synthetic);
        }
    }
    let no_rewrites = HashMap::new();
    for member in &links.data_members {
        if member.source_path == target_path {
            // Same-unit reads already use the target's original const
            // definitions rather than rewritten namespace member paths.
            continue;
        }
        // The source analysis is borrowed from the sibling nodes (disjoint
        // from `target`) or the library cache; only the interner needs cloning
        // (the target borrow must not conflict with the package-unit borrow).
        let source = others
            .iter()
            .copied()
            .find(|unit| unit.path == member.source_path)
            .map(|unit| {
                (
                    unit.analysis.interner.clone(),
                    &unit.analysis.types,
                    &unit.analysis.resolver,
                    unit.file_interface.clone(),
                )
            })
            .or_else(|| {
                let library = links
                    .libraries
                    .iter()
                    .find(|library| library.path == member.source_path)?;
                let interface =
                    library_cached_file_interface(&library.import, library_resolver, library_cache)
                        .ok()?;
                let analysis =
                    library_cached_analysis(&library.import, library_resolver, library_cache)
                        .ok()?;
                Some((
                    analysis.interner.clone(),
                    &analysis.types,
                    &analysis.resolver,
                    interface,
                ))
            });
        let Some((source_interner, source_types, source_resolver, source_interface)) = source
        else {
            diagnostics.push(mir_issue_diag(
                target_path,
                "package_mir_data_member_unsupported",
                format!(
                    "package MIR could not find const data member source unit `{}`",
                    member.source_path.display()
                ),
            ));
            continue;
        };
        let item = match transplant_data_member(
            member,
            &source_interner,
            source_types,
            source_resolver,
            &source_interface,
            const_path_rewrites
                .get(&member.source_path)
                .unwrap_or(&no_rewrites),
            target,
        ) {
            Ok(item) => item,
            Err(message) => {
                diagnostics.push(mir_issue_diag(
                    target_path,
                    "package_mir_data_member_unsupported",
                    message,
                ));
                continue;
            }
        };
        target.hir.items.push(item);
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

/// Collect the optional/default parameter metadata of every function linked
/// into this caller's namespace imports, with each param's default expression
/// transplanted into the caller analysis.
///
/// Package-linked callees carry synthetic definition ids that are absent from
/// the caller's own parameter map. `lower_direct_call_args` fills omitted
/// arguments from [`MirLoweringOptions::imported_function_params`], so the
/// entry (and, in a later unit, sibling/library callers) must receive the
/// linked callees' metadata here. The default expressions originate in the
/// source unit's interner and type table, so they go through the same
/// transplant seam as linked const initializers.
fn imported_function_params_for_caller<'a>(
    caller_path: &Path,
    caller: &mut AnalyzedModule,
    others: impl IntoIterator<Item = &'a AnalyzedProgramNode>,
    links: &PackageMirLinks,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<
    std::sync::Arc<HashMap<DefId, Vec<radix_mir_lower::lower::ImportedCallableParam>>>,
    Vec<Diagnostic>,
> {
    let others = others.into_iter().collect::<Vec<_>>();
    // Never walk the std `HashMap` directly: each transplanted default interns
    // symbols/types into the caller, so the visit order fixes the emitted ids.
    let mut targets = links
        .calls
        .iter()
        .filter(|((path, _, _), _)| path == caller_path)
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return Ok(std::sync::Arc::new(HashMap::new()));
    }
    targets.sort_by(|((_, lhs_def, lhs_name), _), ((_, rhs_def, rhs_name), _)| {
        (lhs_def.0, lhs_name).cmp(&(rhs_def.0, rhs_name))
    });
    let const_members: HashSet<DefId> = links
        .data_members
        .iter()
        .map(|member| member.synthetic)
        .collect();
    let mut const_path_rewrites: HashMap<&PathBuf, HashMap<DefId, DefId>> = HashMap::new();
    for ((source_path, def_id), synthetic) in &links.sources {
        if const_members.contains(synthetic) {
            const_path_rewrites
                .entry(source_path)
                .or_default()
                .insert(*def_id, *synthetic);
        }
    }
    let no_rewrites = HashMap::new();
    let mut parameters = HashMap::new();
    let mut diagnostics = Vec::new();
    for (_, synthetic) in targets {
        let Some(imported) = links.imported_function_params.get(synthetic) else {
            continue;
        };
        // The source analysis is borrowed from the sibling nodes (disjoint
        // from `caller`) or the library cache; only the interner needs cloning
        // (the caller borrow must not conflict with the package-unit borrow).
        let source = others
            .iter()
            .copied()
            .find(|unit| unit.path == imported.source_path)
            .map(|unit| {
                (
                    unit.analysis.interner.clone(),
                    &unit.analysis.types,
                    &unit.analysis.resolver,
                    unit.file_interface.clone(),
                )
            })
            .or_else(|| {
                let library = links
                    .libraries
                    .iter()
                    .find(|library| library.path == imported.source_path)?;
                let interface =
                    library_cached_file_interface(&library.import, library_resolver, library_cache)
                        .ok()?;
                let analysis =
                    library_cached_analysis(&library.import, library_resolver, library_cache)
                        .ok()?;
                Some((
                    analysis.interner.clone(),
                    &analysis.types,
                    &analysis.resolver,
                    interface,
                ))
            });
        let Some((source_interner, source_types, source_resolver, source_interface)) = source
        else {
            diagnostics.push(mir_issue_diag(
                caller_path,
                "package_mir_imported_default_unsupported",
                format!(
                    "package MIR could not find callable source unit `{}`",
                    imported.source_path.display()
                ),
            ));
            continue;
        };
        let source_exports = exported_nominal_names(&source_interface);
        let nominal_map = match build_nominal_remap(
            source_types,
            &source_interner,
            source_resolver,
            source_interface.identity.as_ref(),
            &source_exports,
            caller,
            &HashMap::new(),
        ) {
            Ok(map) => map,
            Err(message) => {
                diagnostics.push(mir_issue_diag(
                    caller_path,
                    "package_mir_imported_default_unsupported",
                    message,
                ));
                continue;
            }
        };
        let empty_def_remap = HashMap::new();
        let empty_source_names = HashMap::new();
        let empty_shared_nominals = HashMap::new();
        let param_symbols =
            super::intern_param_symbols(source_types, &source_interner, &mut caller.interner);
        let record_field_symbols = super::intern_record_field_symbols(
            source_types,
            &source_interner,
            &mut caller.interner,
        );
        let nominal = NominalImportContext {
            nominal_map: &nominal_map,
            def_remap: &empty_def_remap,
            source_names: &empty_source_names,
            shared_nominals: &empty_shared_nominals,
            param_symbols: &param_symbols,
            record_field_symbols: &record_field_symbols,
        };
        let mut imported_types = HashMap::new();
        let mut params = Vec::with_capacity(imported.params.len());
        let mut failed = false;
        for param in &imported.params {
            let default = match &param.default {
                Some(default) => match transplant_expr(
                    default,
                    &source_interner,
                    source_types,
                    &mut caller.interner,
                    &mut caller.types,
                    &mut imported_types,
                    Some(&nominal),
                    const_path_rewrites
                        .get(&imported.source_path)
                        .unwrap_or(&no_rewrites),
                ) {
                    Ok(expression) => Some(expression),
                    Err(message) => {
                        diagnostics.push(mir_issue_diag(
                            caller_path,
                            "package_mir_imported_default_unsupported",
                            message,
                        ));
                        failed = true;
                        break;
                    }
                },
                None => None,
            };
            params.push(radix_mir_lower::lower::ImportedCallableParam {
                optional: param.optional,
                default,
            });
        }
        if !failed {
            parameters.insert(*synthetic, params);
        }
    }
    if diagnostics.is_empty() {
        Ok(std::sync::Arc::new(parameters))
    } else {
        Err(diagnostics)
    }
}

/// Build the synthetic `HirItemKind::Constant` item for one linked const,
/// re-interning the const name + initializer symbols into the target interner
/// and importing the const (and initializer) types into the target type table.
fn transplant_data_member(
    member: &DataMemberLink,
    source_interner: &Interner,
    source_types: &TypeTable,
    source_resolver: &Resolver,
    source_interface: &FileInterface,
    const_path_rewrites: &HashMap<DefId, DefId>,
    target: &mut AnalyzedModule,
) -> Result<HirItem, String> {
    let mut imported = HashMap::new();
    let source_exports = exported_nominal_names(source_interface);
    // S1 U2 nominal identity: build the canonical nominal remap so const
    // types/initializers referencing library nominals unify with the target's
    // semantic types (the linked source module's identity is the key).
    let nominal_map = build_nominal_remap(
        source_types,
        source_interner,
        source_resolver,
        source_interface.identity.as_ref(),
        &source_exports,
        target,
        &HashMap::new(),
    )?;
    let empty_def_remap = HashMap::new();
    let empty_source_names = HashMap::new();
    let empty_shared_nominals = HashMap::new();
    let param_symbols =
        super::intern_param_symbols(source_types, source_interner, &mut target.interner);
    let record_field_symbols =
        super::intern_record_field_symbols(source_types, source_interner, &mut target.interner);
    let nominal = NominalImportContext {
        nominal_map: &nominal_map,
        def_remap: &empty_def_remap,
        source_names: &empty_source_names,
        shared_nominals: &empty_shared_nominals,
        param_symbols: &param_symbols,
        record_field_symbols: &record_field_symbols,
    };
    let konst = HirConst {
        name: remap_symbol(member.konst.name, source_interner, &mut target.interner),
        ty: member.konst.ty.map(|ty| {
            import_semantic_type_with_nominal(
                source_types,
                &mut target.types,
                ty,
                &mut imported,
                Some(&nominal),
            )
        }),
        value: transplant_expr(
            &member.konst.value,
            source_interner,
            source_types,
            &mut target.interner,
            &mut target.types,
            &mut imported,
            Some(&nominal),
            const_path_rewrites,
        )?,
        mutable: member.konst.mutable,
        is_await: member.konst.is_await,
        // The authored form belongs to the source module's re-emission only.
        statica: member
            .konst
            .statica
            .as_ref()
            .map(|_| radix_module::hir::HirStatic { authored: None }),
    };
    Ok(HirItem {
        annotations: Vec::new(),
        // The synthetic def id doubles as a unique item/expression id; no
        // unit-local HIR id range collides with the package-MIR synthetic range.
        id: HirId(member.synthetic.0),
        def_id: member.synthetic,
        kind: HirItemKind::Constant(konst),
        span: member.konst.value.span,
    })
}

/// Re-intern one text symbol into the target interner. Symbols outside the
/// source string table are compiler-generated identity tokens and stay as-is.
fn remap_symbol(symbol: Symbol, source: &Interner, target: &mut Interner) -> Symbol {
    remap_optional_symbol(Some(symbol), source, target).unwrap_or(symbol)
}

/// Transplant a const initializer expression into the entry analysis:
/// re-intern text symbols and import every semantic type. Expression shapes
/// that reference cross-analysis definitions (calls, fields, struct literals,
/// paths to non-linked definitions, …) fail closed — those are the
/// nominal/value-member surfaces U2 carries, not the U1 const-value ABI.
/// The one admitted cross-definition shape is a `Path` to another linked
/// const data member, which rewrites to that member's synthetic def id.
fn transplant_expr(
    expr: &HirExpression,
    source_interner: &Interner,
    source_types: &TypeTable,
    entry_interner: &mut Interner,
    entry_types: &mut TypeTable,
    imported: &mut HashMap<TypeId, TypeId>,
    nominal: Option<&NominalImportContext<'_>>,
    const_path_rewrites: &HashMap<DefId, DefId>,
) -> Result<HirExpression, String> {
    let ty = expr.ty.map(|ty| {
        import_semantic_type_with_nominal(source_types, entry_types, ty, imported, nominal)
    });
    let kind = match &expr.kind {
        HirExpressionKind::Literal(literal) => {
            HirExpressionKind::Literal(transplant_literal(literal, source_interner, entry_interner))
        }
        HirExpressionKind::Vacua => HirExpressionKind::Vacua,
        HirExpressionKind::Binary(op, lhs, rhs) => HirExpressionKind::Binary(
            *op,
            Box::new(transplant_expr(
                lhs,
                source_interner,
                source_types,
                entry_interner,
                entry_types,
                imported,
                nominal,
                const_path_rewrites,
            )?),
            Box::new(transplant_expr(
                rhs,
                source_interner,
                source_types,
                entry_interner,
                entry_types,
                imported,
                nominal,
                const_path_rewrites,
            )?),
        ),
        HirExpressionKind::Unary(op, inner) => HirExpressionKind::Unary(
            *op,
            Box::new(transplant_expr(
                inner,
                source_interner,
                source_types,
                entry_interner,
                entry_types,
                imported,
                nominal,
                const_path_rewrites,
            )?),
        ),
        HirExpressionKind::Array(elements) => {
            let mut transplanted = Vec::with_capacity(elements.len());
            for element in elements {
                transplanted.push(match element {
                    radix_module::hir::HirArrayElement::Expr(element) => {
                        radix_module::hir::HirArrayElement::Expr(transplant_expr(
                            element,
                            source_interner,
                            source_types,
                            entry_interner,
                            entry_types,
                            imported,
                            nominal,
                            const_path_rewrites,
                        )?)
                    }
                    radix_module::hir::HirArrayElement::Spread(element) => {
                        radix_module::hir::HirArrayElement::Spread(transplant_expr(
                            element,
                            source_interner,
                            source_types,
                            entry_interner,
                            entry_types,
                            imported,
                            nominal,
                            const_path_rewrites,
                        )?)
                    }
                    radix_module::hir::HirArrayElement::Infer(span) => {
                        radix_module::hir::HirArrayElement::Infer(*span)
                    }
                });
            }
            HirExpressionKind::Array(transplanted)
        }
        HirExpressionKind::Tuple(items, type_args) => {
            let mut transplanted = Vec::with_capacity(items.len());
            for item in items {
                transplanted.push(transplant_expr(
                    item,
                    source_interner,
                    source_types,
                    entry_interner,
                    entry_types,
                    imported,
                    nominal,
                    const_path_rewrites,
                )?);
            }
            let type_args = type_args.as_ref().map(|args| {
                args.iter()
                    .map(|ty| {
                        import_semantic_type_with_nominal(
                            source_types,
                            entry_types,
                            *ty,
                            imported,
                            nominal,
                        )
                    })
                    .collect()
            });
            HirExpressionKind::Tuple(transplanted, type_args)
        }
        HirExpressionKind::Scriptum(template, items) => {
            let mut transplanted = Vec::with_capacity(items.len());
            for item in items {
                transplanted.push(transplant_expr(
                    item,
                    source_interner,
                    source_types,
                    entry_interner,
                    entry_types,
                    imported,
                    nominal,
                    const_path_rewrites,
                )?);
            }
            HirExpressionKind::Scriptum(
                remap_symbol(*template, source_interner, entry_interner),
                transplanted,
            )
        }
        HirExpressionKind::Scribe(kind, items) => {
            let mut transplanted = Vec::with_capacity(items.len());
            for item in items {
                transplanted.push(transplant_expr(
                    item,
                    source_interner,
                    source_types,
                    entry_interner,
                    entry_types,
                    imported,
                    nominal,
                    const_path_rewrites,
                )?);
            }
            HirExpressionKind::Scribe(*kind, transplanted)
        }
        // Static type ascription (`∷`): the source expression and the target
        // type both travel. Const data members across library boundaries use
        // this shape for typed literals (`sit f32 GELU_ALPHA ← (0.79… ∷
        // f32)`), so the transplant must carry it through the entry's
        // top-level-const seam instead of failing closed.
        HirExpressionKind::Verte {
            source,
            target,
            entries,
        } => {
            let source = Box::new(transplant_expr(
                source,
                source_interner,
                source_types,
                entry_interner,
                entry_types,
                imported,
                nominal,
                const_path_rewrites,
            )?);
            let target = import_semantic_type_with_nominal(
                source_types,
                entry_types,
                *target,
                imported,
                nominal,
            );
            let entries = match entries {
                Some(fields) => {
                    let mut transplanted = Vec::with_capacity(fields.len());
                    for field in fields {
                        let key = match &field.key {
                            radix_module::hir::HirObjectKey::Ident(symbol) => {
                                radix_module::hir::HirObjectKey::Ident(remap_symbol(
                                    *symbol,
                                    source_interner,
                                    entry_interner,
                                ))
                            }
                            radix_module::hir::HirObjectKey::String(symbol) => {
                                radix_module::hir::HirObjectKey::String(remap_symbol(
                                    *symbol,
                                    source_interner,
                                    entry_interner,
                                ))
                            }
                            radix_module::hir::HirObjectKey::Computed(expr) => {
                                radix_module::hir::HirObjectKey::Computed(transplant_expr(
                                    expr,
                                    source_interner,
                                    source_types,
                                    entry_interner,
                                    entry_types,
                                    imported,
                                    nominal,
                                    const_path_rewrites,
                                )?)
                            }
                            radix_module::hir::HirObjectKey::Spread(expr) => {
                                radix_module::hir::HirObjectKey::Spread(transplant_expr(
                                    expr,
                                    source_interner,
                                    source_types,
                                    entry_interner,
                                    entry_types,
                                    imported,
                                    nominal,
                                    const_path_rewrites,
                                )?)
                            }
                        };
                        let value = field
                            .value
                            .as_ref()
                            .map(|value| {
                                transplant_expr(
                                    value,
                                    source_interner,
                                    source_types,
                                    entry_interner,
                                    entry_types,
                                    imported,
                                    nominal,
                                    const_path_rewrites,
                                )
                            })
                            .transpose()?;
                        transplanted.push(HirObjectField { key, value });
                    }
                    Some(transplanted)
                }
                None => None,
            };
            HirExpressionKind::Verte {
                source,
                target,
                entries,
            }
        }
        // Runtime conversion (`↦ target`): module-level collection consts
        // convert their literal at initialization (`[…] ↦ set<string>` in
        // gradus gguf.fab). The source expression, the conversion target
        // (type or clamp range), the codegen hint symbols, and the optional
        // inline recovery all travel; cross-analysis definitions inside them
        // still fail closed through the recursive transplant below.
        HirExpressionKind::Conversio {
            route,
            source,
            target,
            params,
            recovery,
            implicit,
        } => {
            // A routed conversion (ordo row, registered `@ conversio`)
            // names definitions of the source analysis; fail closed rather
            // than transplant them unmapped.
            if route.is_some() {
                return Err(
                    "package MIR cannot transplant a const data member initializer with an ordo or registered conversio row"
                        .to_string(),
                );
            }
            let source = Box::new(transplant_expr(
                source,
                source_interner,
                source_types,
                entry_interner,
                entry_types,
                imported,
                nominal,
                const_path_rewrites,
            )?);
            let target = match target {
                radix_module::hir::HirConversioTarget::Type(ty) => {
                    radix_module::hir::HirConversioTarget::Type(import_semantic_type_with_nominal(
                        source_types,
                        entry_types,
                        *ty,
                        imported,
                        nominal,
                    ))
                }
                radix_module::hir::HirConversioTarget::Intervallum(range) => {
                    radix_module::hir::HirConversioTarget::Intervallum(Box::new(transplant_expr(
                        range,
                        source_interner,
                        source_types,
                        entry_interner,
                        entry_types,
                        imported,
                        nominal,
                        const_path_rewrites,
                    )?))
                }
            };
            let params = params
                .iter()
                .map(|param| remap_symbol(*param, source_interner, entry_interner))
                .collect();
            let recovery = match recovery {
                Some(recovery) => Some(Box::new(transplant_expr(
                    recovery,
                    source_interner,
                    source_types,
                    entry_interner,
                    entry_types,
                    imported,
                    nominal,
                    const_path_rewrites,
                )?)),
                None => None,
            };
            let implicit = implicit
                .clone()
                .map(|site| radix_module::hir::HirStoreSite {
                    slot: site
                        .slot
                        .map(|slot| remap_symbol(slot, source_interner, entry_interner)),
                    ..site
                });
            HirExpressionKind::Conversio {
                route: None,
                source,
                target,
                params,
                recovery,
                implicit,
            }
        }
        // Const-to-const reference: the initializer names another linked
        // let data member of the same source unit (e.g. `ELEMENTS ← N *
        // K` in gradus kernel.fab). Rewriting to that member's synthetic def
        // id materializes the reference through the entry's top-level-const
        // seam, exactly like the entry's own rewritten member references.
        // Paths without a linked const target — functions, non-linked
        // definitions, anything else the source analysis resolves — fail
        // closed below.
        HirExpressionKind::Path(def_id) => match const_path_rewrites.get(def_id) {
            Some(synthetic) => HirExpressionKind::Path(*synthetic),
            None => {
                return Err(
                    "package MIR cannot transplant const data member initializer Path reference without a linked const data member target"
                        .to_string(),
                );
            }
        },
        _ => {
            return Err(
                "package MIR cannot transplant const data member initializer with unsupported expression shape"
                    .to_string(),
            );
        }
    };
    Ok(HirExpression {
        id: expr.id,
        kind,
        ty,
        span: expr.span,
    })
}

fn transplant_literal(
    literal: &HirLiteral,
    source_interner: &Interner,
    entry_interner: &mut Interner,
) -> HirLiteral {
    match literal {
        HirLiteral::String(symbol) => {
            HirLiteral::String(remap_symbol(*symbol, source_interner, entry_interner))
        }
        HirLiteral::Ascii(symbol) => {
            HirLiteral::Ascii(remap_symbol(*symbol, source_interner, entry_interner))
        }
        HirLiteral::Octeti(symbol) => {
            HirLiteral::Octeti(remap_symbol(*symbol, source_interner, entry_interner))
        }
        HirLiteral::Regex(pattern, flags) => HirLiteral::Regex(
            remap_symbol(*pattern, source_interner, entry_interner),
            flags
                .as_ref()
                .map(|flags| remap_symbol(*flags, source_interner, entry_interner)),
        ),
        HirLiteral::Int(value) => HirLiteral::Int(*value),
        // inf track U3: the canonical decimal digits carry no symbol to remap.
        HirLiteral::BigInt(digits) => HirLiteral::BigInt(digits.clone()),
        // decimal-widths D1 successor: the digit text crosses
        // the boundary remapped, so the entry unit's MIR lowering still
        // derives the exact scaled carrier; the f64 field alone cannot.
        HirLiteral::Float(value) => HirLiteral::Float(*value),
        HirLiteral::FloatText { value, text } => HirLiteral::FloatText {
            value: *value,
            text: remap_symbol(*text, source_interner, entry_interner),
        },
        HirLiteral::JsonValor(value) => HirLiteral::JsonValor(value.clone()),
        HirLiteral::Bool(value) => HirLiteral::Bool(*value),
        HirLiteral::Nil => HirLiteral::Nil,
    }
}

pub(super) fn selected_cli_dispatch_function(
    cli_plan: &CliPackagePlan,
    unit_path: &Path,
    program: &MirProgram,
    dispatch_interner: &Interner,
    program_interner: &Interner,
) -> Option<MirFunctionId> {
    let dispatch = cli_plan.dispatch.as_ref()?;
    if dispatch.unit_path != unit_path {
        return None;
    }
    // Compare by resolved text: the mounted-command symbol is interned in the
    // command unit's analysis, while lowered program names are re-interned
    // into the entry interner during package symbol remapping.
    let name_text = dispatch_interner.resolve(dispatch.function);
    program
        .functions
        .iter()
        .find(|candidate| {
            candidate
                .name
                .is_some_and(|name| program_interner.resolve(name) == name_text)
        })
        .map(|candidate| candidate.id)
}

pub(super) fn import_lowered_semantic_types(
    source: &TypeTable,
    target: &mut TypeTable,
    nominal: Option<&NominalImportContext<'_>>,
) -> Vec<(TypeId, TypeId)> {
    let mut imported = HashMap::new();
    let mut rewrites = Vec::new();
    for index in 0..source.type_count() {
        let source_ty = TypeId(u32::try_from(index).unwrap_or(u32::MAX));
        let target_ty =
            import_semantic_type_with_nominal(source, target, source_ty, &mut imported, nominal);
        push_type_rewrite(&mut rewrites, source_ty, target_ty);
    }
    rewrites
}

pub(super) fn imported_dispatch_type_rewrites(
    rewrite: &CliRecordTypeRewrite,
    imported: &[(TypeId, TypeId)],
) -> Vec<(TypeId, TypeId)> {
    rewrite
        .types
        .iter()
        .filter_map(|(from, to)| {
            let imported_from = imported
                .iter()
                .find_map(|(source, target)| (*source == *from).then_some(*target))
                .unwrap_or(*from);
            (imported_from != *to).then_some((imported_from, *to))
        })
        .collect()
}

fn validation_diagnostics(
    diagnostic_path: &Path,
    errors: Vec<radix_mir::MirValidationError>,
) -> Vec<Diagnostic> {
    errors
        .into_iter()
        .map(|error| mir_validation_diag(diagnostic_path, error))
        .collect()
}

fn rewrite_validation_type_ids(
    context: &mut radix_mir::MirValidationContext<'_>,
    rewrites: &[(TypeId, TypeId)],
) {
    for signature in context.functions.values_mut() {
        for param in &mut signature.params {
            rewrite_type_id(&mut param.ty, rewrites);
        }
        rewrite_type_id(&mut signature.return_ty, rewrites);
        if let Some(error_ty) = &mut signature.error_ty {
            rewrite_type_id(error_ty, rewrites);
        }
    }
    for fields in context.struct_fields.values_mut() {
        for ty in fields.values_mut() {
            rewrite_type_id(ty, rewrites);
        }
    }
    for fields in context.variant_fields.values_mut() {
        for ty in fields.values_mut() {
            rewrite_type_id(ty, rewrites);
        }
    }
    for environment in context.closure_environments.values_mut() {
        rewrite_type_id(&mut environment.value_ty, rewrites);
        for capture in &mut environment.captures {
            rewrite_type_id(&mut capture.ty, rewrites);
        }
    }
    context.scalar_value_domains = std::mem::take(&mut context.scalar_value_domains)
        .into_iter()
        .map(|(mut ty, domain)| {
            if let Some((_, rewritten)) = rewrites.iter().find(|(source, _)| *source == ty) {
                ty = *rewritten;
            }
            (ty, domain)
        })
        .collect();
}

/// Move validation-context `DefId` keys through the same variant/nominal remap
/// as the program's `EnumVariant` aggregates.
///
/// Each unit allocates user `DefId`s from `USER_DEF_ID_BASE`. After the program
/// rewrite, a source-local `DTypeError.UnknownName` can still occupy the same
/// numeric id as the consumer's remapped `DType.I32`. `remapped.apply` then
/// `or_insert`s commune fields onto that slot, so a unit-variant construction
/// fails as "named aggregate is missing required field". Rewrite the keys
/// first so leftover source-local metadata cannot alias a remapped dest.
fn rewrite_validation_def_ids(
    context: &mut radix_mir::MirValidationContext<'_>,
    variant_remap: &HashMap<DefId, DefId>,
    struct_remap: &HashMap<DefId, DefId>,
) {
    let remap_variant = |id: DefId| variant_remap.get(&id).copied().unwrap_or(id);
    let remap_nominal = |id: DefId| struct_remap.get(&id).copied().unwrap_or(id);

    for (def, fields) in std::mem::take(&mut context.variant_fields) {
        let slot = context
            .variant_fields
            .entry(remap_variant(def))
            .or_default();
        for (name, ty) in fields {
            slot.entry(name).or_insert(ty);
        }
    }

    for (def, order) in std::mem::take(&mut context.variant_field_order) {
        context
            .variant_field_order
            .entry(remap_variant(def))
            .or_insert(order);
    }

    for (def, name) in std::mem::take(&mut context.display_names) {
        let dest = variant_remap
            .get(&def)
            .or_else(|| struct_remap.get(&def))
            .copied()
            .unwrap_or(def);
        context.display_names.entry(dest).or_insert(name);
    }

    for (variant, parent) in std::mem::take(&mut context.variant_parents) {
        context
            .variant_parents
            .entry(remap_variant(variant))
            .or_insert(remap_nominal(parent));
    }

    for (enum_def, variants) in std::mem::take(&mut context.enum_variants) {
        context
            .enum_variants
            .entry(remap_nominal(enum_def))
            .or_insert_with(|| variants.into_iter().map(remap_variant).collect());
    }
    context.rebuild_variant_positions();
}

/// Re-key the validation context's closure-environment registry onto the
/// unit's current environment vec. Every rewrite pass rebuilds `ValidatedMir`
/// from the rewritten program plus these live environments, so the stale
/// pre-rewrite registry carried in through `into_parts` must be replaced
/// before re-validation.
fn sync_context_closure_environments(
    context: &mut radix_mir::MirValidationContext<'_>,
    environments: &[MirClosureEnvironment],
) {
    context.closure_environments = environments
        .iter()
        .map(|environment| (environment.id, environment.clone()))
        .collect();
}

pub(super) fn rewrite_lowered_type_ids<'a>(
    mut lowered: LoweredMirUnit<'a>,
    rewrites: &[(TypeId, TypeId)],
    target_types: &TypeTable,
    variant_remap: &HashMap<DefId, DefId>,
    struct_remap: &HashMap<DefId, DefId>,
    remapped: &RemappedValidation,
    diagnostic_path: &Path,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    let (mut program, context) = lowered.validated.into_parts();
    for function in &mut program.functions {
        rewrite_function_type_ids(function, rewrites);
        rewrite_function_variant_defs(function, variant_remap);
        rewrite_function_struct_defs(function, struct_remap);
    }
    for function in &mut lowered.generic_templates {
        rewrite_function_type_ids(function, rewrites);
        rewrite_function_variant_defs(function, variant_remap);
        rewrite_function_struct_defs(function, struct_remap);
    }
    for environment in &mut lowered.closure_environments {
        rewrite_type_id(&mut environment.value_ty, rewrites);
        for capture in &mut environment.captures {
            rewrite_type_id(&mut capture.ty, rewrites);
        }
    }
    let target_snapshot = TypeTable::from_snapshot(target_types.snapshot())
        .map_err(|message| vec![mir_lowering_diag(diagnostic_path, message)])?;
    let target_snapshot: &'static TypeTable = Box::leak(Box::new(target_snapshot));
    let mut context = context.with_types(target_snapshot);
    rewrite_validation_type_ids(&mut context, rewrites);
    rewrite_validation_def_ids(&mut context, variant_remap, struct_remap);
    remapped.apply(&mut context);
    sync_context_closure_environments(&mut context, &lowered.closure_environments);
    lowered.validated = radix_mir::ValidatedMir::new(program, context)
        .map_err(|errors| validation_diagnostics(diagnostic_path, errors))?;
    Ok(lowered)
}

/// Rehome compiler-owned shape-size indices when a sibling or library MIR
/// unit is merged into the entry type table. Unlike a tensor type, a
/// `MirConstant::ShapeSize` stores an `IndexId` directly, so the ordinary
/// `TypeId` rewrite cannot move it across the package boundary. Re-intern the
/// full index expression and its parameter symbols in the entry namespace.
fn rewrite_lowered_shape_size_indices<'a>(
    mut lowered: LoweredMirUnit<'a>,
    source_types: &TypeTable,
    target_types: &mut TypeTable,
    param_symbols: &HashMap<Symbol, Symbol>,
    record_field_symbols: &HashMap<Symbol, Symbol>,
    remapped: &RemappedValidation,
    diagnostic_path: &Path,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    let (mut program, context) = lowered.validated.into_parts();
    let mut rewrite_function = |function: &mut MirFunction| {
        let completed = radix_mir::generic::rewrite_function_constants(function, &mut |constant| {
            let MirConstant::ShapeSize { shape, .. } = constant else {
                return true;
            };
            *shape = import_shape_size_index(
                source_types,
                target_types,
                *shape,
                param_symbols,
                record_field_symbols,
            );
            true
        });
        // The callback never aborts the walk.
        debug_assert!(completed);
    };
    for function in &mut program.functions {
        rewrite_function(function);
    }
    for function in &mut lowered.generic_templates {
        rewrite_function(function);
    }
    for (_, params) in &mut lowered.generic_params {
        for param in params {
            if let Some(witness) = param.runtime_witness.as_mut() {
                *witness = import_shape_size_index(
                    source_types,
                    target_types,
                    *witness,
                    param_symbols,
                    record_field_symbols,
                );
            }
        }
    }
    let target_snapshot = TypeTable::from_snapshot(target_types.snapshot())
        .map_err(|message| vec![mir_lowering_diag(diagnostic_path, message)])?;
    let target_snapshot: &'static TypeTable = Box::leak(Box::new(target_snapshot));
    let mut context = context.with_types(target_snapshot);
    remapped.apply(&mut context);
    lowered.validated = radix_mir::ValidatedMir::new(program, context)
        .map_err(|errors| validation_diagnostics(diagnostic_path, errors))?;
    Ok(lowered)
}

fn import_shape_size_index(
    source_types: &TypeTable,
    target_types: &mut TypeTable,
    index: radix_module::semantic::IndexId,
    param_symbols: &HashMap<Symbol, Symbol>,
    record_field_symbols: &HashMap<Symbol, Symbol>,
) -> radix_module::semantic::IndexId {
    match source_types.get_index(index).clone() {
        IndexExpr::Tuple(items) => {
            let items = items
                .into_iter()
                .map(|item| {
                    import_shape_size_index(
                        source_types,
                        target_types,
                        item,
                        param_symbols,
                        record_field_symbols,
                    )
                })
                .collect();
            target_types.intern_index(IndexExpr::Tuple(items))
        }
        IndexExpr::Affine { base, offset } => {
            let base = import_shape_size_index(
                source_types,
                target_types,
                base,
                param_symbols,
                record_field_symbols,
            );
            target_types.intern_index(IndexExpr::Affine { base, offset })
        }
        IndexExpr::Divide { numerator, divisor } => {
            let numerator = import_shape_size_index(
                source_types,
                target_types,
                numerator,
                param_symbols,
                record_field_symbols,
            );
            target_types.intern_index(IndexExpr::Divide { numerator, divisor })
        }
        IndexExpr::Param(symbol) => target_types.intern_index(IndexExpr::Param(
            param_symbols.get(&symbol).copied().unwrap_or(symbol),
        )),
        IndexExpr::Runtime(mut source) => {
            use radix_types::RuntimeExtentId;
            let fields = match &mut source {
                RuntimeExtentId::TensorFieldAxis { fields, .. }
                | RuntimeExtentId::ExpressionFieldAxis { fields, .. }
                | RuntimeExtentId::CallArgumentFieldAxis { fields, .. }
                | RuntimeExtentId::FunctionParameterFieldAxis { fields, .. } => Some(fields),
                _ => None,
            };
            if let Some(fields) = fields {
                for field in fields {
                    *field = record_field_symbols.get(field).copied().unwrap_or(*field);
                }
            }
            target_types.intern_index(IndexExpr::Runtime(source))
        }
        other => target_types.intern_index(other),
    }
}

fn remap_lowered_program_text_symbols<'a>(
    mut lowered: LoweredMirUnit<'a>,
    source: &Interner,
    target: &mut Interner,
    diagnostic_path: &Path,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    let (mut program, mut context) = lowered.validated.into_parts();
    remap_program_text_symbols(&mut program, source, target);
    remap_closure_environment_symbols(&mut lowered.closure_environments, source, target);
    for fields in context.struct_fields.values_mut() {
        *fields = std::mem::take(fields)
            .into_iter()
            .map(|(name, ty)| (remap_metadata_symbol(name, source, target), ty))
            .collect();
    }
    for order in context.struct_field_order.values_mut() {
        for name in order {
            *name = remap_metadata_symbol(*name, source, target);
        }
    }
    for order in context.variant_field_order.values_mut() {
        for name in order {
            *name = remap_metadata_symbol(*name, source, target);
        }
    }
    for params in context.struct_type_params.values_mut() {
        for param in params {
            *param = remap_metadata_symbol(*param, source, target);
        }
    }
    for params in context.enum_type_params.values_mut() {
        for param in params {
            *param = remap_metadata_symbol(*param, source, target);
        }
    }
    for fields in context.optional_struct_fields.values_mut() {
        *fields = std::mem::take(fields)
            .into_iter()
            .map(|name| remap_metadata_symbol(name, source, target))
            .collect();
    }
    for fields in context.nullable_struct_fields.values_mut() {
        *fields = std::mem::take(fields)
            .into_iter()
            .map(|name| remap_metadata_symbol(name, source, target))
            .collect();
    }
    for fields in context.json_struct_field_keys.values_mut() {
        *fields = std::mem::take(fields)
            .into_iter()
            .map(|(name, key)| (remap_metadata_symbol(name, source, target), key))
            .collect();
    }
    for fields in context.variant_fields.values_mut() {
        *fields = std::mem::take(fields)
            .into_iter()
            .map(|(name, ty)| (remap_metadata_symbol(name, source, target), ty))
            .collect();
    }
    // Record field identities are the one field population that lives in the
    // TYPE ARENA (`Type::Record`), not in the metadata maps above: remap their
    // name symbols through the same source→target text mapping so the
    // re-validation below resolves the record aggregates and `args.<field>`
    // projections this remap just re-interned (mounted-command CLI records —
    // the hazard `selected_cli_dispatch_function` documents for command
    // symbols). The snapshot rebuild keeps every `TypeId` slot-in-place, so
    // the program's still-source type ids resolve unchanged.
    let has_records = (0..context.types.type_count()).any(|index| {
        matches!(
            context
                .types
                .get(TypeId(u32::try_from(index).unwrap_or(u32::MAX))),
            Type::Record(_)
        )
    });
    if has_records {
        let mut snapshot = context.types.snapshot();
        snapshot.remap_record_field_symbols(|symbol| remap_symbol(symbol, source, target));
        let remapped_types = TypeTable::from_snapshot(snapshot)
            .map_err(|message| vec![mir_lowering_diag(diagnostic_path, message)])?;
        context = context.with_owned_types(remapped_types);
    }
    context.interner = radix_mir::MirInterner::Owned(std::sync::Arc::new(target.clone()));
    sync_context_closure_environments(&mut context, &lowered.closure_environments);
    lowered.validated = radix_mir::ValidatedMir::new(program, context)
        .map_err(|errors| validation_diagnostics(diagnostic_path, errors))?;
    Ok(lowered)
}

/// Consumer-local identities of imported sibling struct methods, each paired
/// with the linked definition the call rewrite resolves the same
/// `(struct, method)` to.
///
/// A method call through a contract-typed value lowers one dispatch arm per
/// implementor, against the identity the importing analysis allocated for the
/// method. The HIR call rewrite never sees those arms (the receiver is a
/// contract, not a struct), so the link names the linked definition here and
/// the unit's source rewrite applies it to the lowered program.
fn imported_method_source_rewrites(
    analysis: &radix_module::driver::AnalyzedModule,
    unit_path: &Path,
    method_targets: &HashMap<PathBuf, HashMap<(DefId, String), DefId>>,
) -> Vec<((PathBuf, DefId), DefId)> {
    let Some(targets) = method_targets.get(unit_path) else {
        return Vec::new();
    };
    analysis
        .resolver
        .imported_file_method_identities()
        .into_iter()
        .filter_map(|(method_def, owner_def, _, _, name)| {
            let linked = targets.get(&(owner_def, analysis.interner.resolve(name).to_owned()))?;
            Some(((unit_path.to_path_buf(), method_def), *linked))
        })
        .collect()
}

fn rewrite_lowered_program_sources<'a>(
    mut lowered: LoweredMirUnit<'a>,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
    diagnostic_path: &Path,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    let (mut program, mut context) = lowered.validated.into_parts();
    rewrite_program_sources(&mut program, unit_path, source_rewrites);
    rewrite_closure_environment_sources(
        &mut lowered.closure_environments,
        unit_path,
        source_rewrites,
    );
    let rewrite = |def: DefId| {
        source_rewrites
            .get(&(unit_path.to_path_buf(), def))
            .copied()
            .unwrap_or(def)
    };
    context.functions = std::mem::take(&mut context.functions)
        .into_iter()
        .map(|(def, signature)| (rewrite(def), signature))
        .collect();
    context.method_defs = std::mem::take(&mut context.method_defs)
        .into_iter()
        .map(rewrite)
        .collect();
    context.external_function_identities =
        std::mem::take(&mut context.external_function_identities)
            .into_iter()
            .map(|(def, identity)| (rewrite(def), identity))
            .collect();
    sync_context_closure_environments(&mut context, &lowered.closure_environments);
    lowered.validated = radix_mir::ValidatedMir::new(program, context)
        .map_err(|errors| validation_diagnostics(diagnostic_path, errors))?;
    Ok(lowered)
}

fn remap_templates_text_symbols<'a>(
    mut lowered: LoweredMirUnit<'a>,
    target: &mut Interner,
) -> LoweredMirUnit<'a> {
    let source = lowered.interner.clone();
    for template in &mut lowered.generic_templates {
        remap_function_text_symbols(template, &source, target);
    }
    // Generic param names must land in the merged interner too: template
    // shape params re-intern by name on type import, so shape-binding keys
    // (param names) and template shape symbols have to agree on one table.
    for (_, params) in &mut lowered.generic_params {
        for param in params {
            param.name = remap_symbol(param.name, &source, target);
        }
    }
    lowered
}

fn rewrite_generic_identity<'a>(
    mut lowered: LoweredMirUnit<'a>,
    unit_path: &Path,
    source_rewrites: &SourceRewrites,
    nominal_rewrites: &HashMap<DefId, DefId>,
    method_targets: Option<&HashMap<(DefId, String), DefId>>,
) -> LoweredMirUnit<'a> {
    rewrite_templates_sources(&mut lowered.generic_templates, unit_path, source_rewrites);
    rewrite_generic_param_sources(&mut lowered.generic_params, unit_path, source_rewrites);
    let rewrite_def = |source: DefId| {
        source_rewrites
            .get(&(unit_path.to_path_buf(), source))
            .copied()
            .unwrap_or(source)
    };
    for site in &mut lowered.tensor_edge_policy_metadata.generic_limes {
        site.source = rewrite_def(site.source);
    }
    for provider in &mut lowered.tensor_edge_policy_metadata.providers {
        if let MirTensorEdgePolicySelector::Custom {
            policy_def,
            remap,
            transform,
        } = &mut provider.selector
        {
            *policy_def = nominal_rewrites
                .get(policy_def)
                .copied()
                .unwrap_or(*policy_def);
            let rewrite_callback = |method: &str, callback: DefId| {
                method_targets
                    .and_then(|targets| targets.get(&(*policy_def, method.to_owned())).copied())
                    .or_else(|| {
                        source_rewrites
                            .get(&(unit_path.to_path_buf(), callback))
                            .copied()
                    })
                    .unwrap_or(callback)
            };
            *remap = rewrite_callback("remap", *remap);
            *transform = transform.map(|callback| rewrite_callback("transform", callback));
        }
        if let Some(policy_type) = &mut provider.policy_type {
            rewrite_tensor_edge_type_identity(policy_type, nominal_rewrites);
        }
        if let Some(element_type) = &mut provider.element_type {
            rewrite_tensor_edge_type_identity(element_type, nominal_rewrites);
        }
    }
    lowered
}

fn rewrite_tensor_edge_type_identity(
    identity: &mut radix_mir_lower::lower::MirTensorEdgeTypeIdentity,
    rewrites: &HashMap<DefId, DefId>,
) {
    use radix_mir_lower::lower::{MirTensorEdgeAppliedArg, MirTensorEdgeTypeIdentity};

    match identity {
        MirTensorEdgeTypeIdentity::Struct(definition)
        | MirTensorEdgeTypeIdentity::Enum(definition)
        | MirTensorEdgeTypeIdentity::Interface(definition) => {
            *definition = rewrites.get(definition).copied().unwrap_or(*definition);
        }
        MirTensorEdgeTypeIdentity::Applied(base, args) => {
            rewrite_tensor_edge_type_identity(base, rewrites);
            for argument in args {
                if let MirTensorEdgeAppliedArg::Type(ty) = argument {
                    rewrite_tensor_edge_type_identity(ty, rewrites);
                }
            }
        }
        MirTensorEdgeTypeIdentity::Array(inner)
        | MirTensorEdgeTypeIdentity::Option(inner)
        | MirTensorEdgeTypeIdentity::Ref(_, inner)
        | MirTensorEdgeTypeIdentity::Tensor(inner, _)
        | MirTensorEdgeTypeIdentity::Vector(inner, _)
        | MirTensorEdgeTypeIdentity::Matrix(inner, _)
        | MirTensorEdgeTypeIdentity::Sparsa(inner, _) => {
            rewrite_tensor_edge_type_identity(inner, rewrites);
        }
        MirTensorEdgeTypeIdentity::Tuple(items)
        | MirTensorEdgeTypeIdentity::Union(items)
        | MirTensorEdgeTypeIdentity::Intersection(items) => {
            for item in items {
                rewrite_tensor_edge_type_identity(item, rewrites);
            }
        }
        MirTensorEdgeTypeIdentity::Record(fields) => {
            for (_, field_ty) in fields {
                rewrite_tensor_edge_type_identity(field_ty, rewrites);
            }
        }
        MirTensorEdgeTypeIdentity::Primitive(_)
        | MirTensorEdgeTypeIdentity::BuiltinPolicy(_)
        | MirTensorEdgeTypeIdentity::SizedNumeric(_, _)
        | MirTensorEdgeTypeIdentity::ModularWord(_)
        | MirTensorEdgeTypeIdentity::SaturatingWord(_)
        | MirTensorEdgeTypeIdentity::PolicyFloat(_, _)
        | MirTensorEdgeTypeIdentity::SizedInstans(_) => {}
    }
}

/// Collect one package unit's own device facts for the package-seam
/// composition (CDC-1): the full device-role map (`@ nucleum` entries and
/// private helpers alike — the same derivation the single-unit compose
/// consumes) plus the public Kernel launch-entry sources. Keys are remapped
/// through the same `source_rewrites` map `rewrite_program_sources` applies
/// to that unit's lowered program; sibling units pass the map, the entry
/// unit passes `None` (its definitions are identity-keyed in the merged
/// program). One `DefId` should never join two facts; if a rewrite collision
/// ever produced one, the last write is inert because the merged program's
/// own `ensure_unique_definition_sources` guard rejects the underlying
/// duplicate-source collision first.
fn collect_consumer_device_facts(
    analysis: &radix_module::driver::AnalyzedModule,
    unit_path: &Path,
    source_rewrites: Option<&SourceRewrites>,
    device_roles: &mut HashMap<DefId, radix_mir::MirDeviceRole>,
    launch_entries: &mut HashSet<DefId>,
) {
    let remap = |def: DefId| -> DefId {
        source_rewrites
            .and_then(|rewrites| rewrites.get(&(unit_path.to_path_buf(), def)).copied())
            .unwrap_or(def)
    };
    for (def, role) in radix_mir::device_roles_from_hir(&analysis.hir) {
        device_roles.insert(remap(def), role);
    }
    for (def, role) in radix_mir::device_entry_roles_from_hir(&analysis.hir, &analysis.interner) {
        if role == radix_mir::MirDeviceRole::Kernel {
            launch_entries.insert(remap(def));
        }
    }
}

/// Join one unit's transferred imported-device route map to the merged
/// synthetic template `DefId`s (KPC-WIRE `done_when` (c)).
///
/// The module-side map is keyed by import-item `DefId`; the linker's call
/// table keys the same import item plus verb to the synthetic source the
/// package rewrite gives that function's generic template. A route with no
/// linked target joins nothing — it never silently admits an instance.
fn joined_imported_device_routes<'routes>(
    unit_routes: impl IntoIterator<
        Item = (
            &'routes DefId,
            &'routes Vec<radix_mir_lower::lower::ImportedDeviceRoute>,
        ),
    >,
    links: &PackageMirLinks,
    caller_path: &Path,
) -> Vec<(DefId, radix_mir_lower::lower::ImportedDeviceRoute)> {
    let mut joined = Vec::new();
    for (import_def_id, routes) in unit_routes {
        for route in routes {
            let Some(synthetic) = links.calls.get(&(
                caller_path.to_path_buf(),
                *import_def_id,
                route.identity.name.clone(),
            )) else {
                continue;
            };
            joined.push((*synthetic, route.clone()));
        }
    }
    joined
}

fn merge_imported_device_registrations(
    target: &mut Vec<radix_mir_lower::lower::ImportedDeviceRegistration>,
    incoming: Vec<radix_mir_lower::lower::ImportedDeviceRegistration>,
) {
    for registration in incoming {
        if target
            .iter()
            .any(|existing| existing.key == registration.key)
        {
            continue;
        }
        target.push(registration);
    }
}

fn merge_tensor_edge_policy_metadata(
    target: &mut MirTensorEdgePolicyMetadata,
    incoming: &MirTensorEdgePolicyMetadata,
) {
    for provider in &incoming.providers {
        if !target.providers.contains(provider) {
            target.providers.push(provider.clone());
        }
    }
    for site in &incoming.generic_limes {
        if !target.generic_limes.contains(site) {
            target.generic_limes.push(*site);
        }
    }
}

fn shift_templates(templates: &mut [MirFunction], offset: u32) {
    if offset == 0 {
        return;
    }
    for template in templates {
        shift_function_id(&mut template.id, offset);
        for block in &mut template.blocks {
            for statement in &mut block.statements {
                shift_statement_ids(statement, offset);
            }
            shift_terminator_ids(&mut block.terminator, offset);
        }
    }
}

#[allow(clippy::type_complexity)]
fn instantiate_package_generics<'a>(
    mut merged: LoweredMirUnit<'a>,
    templates: Vec<MirFunction>,
    generic_params: Vec<(DefId, Vec<radix_mir_lower::lower::MirGenericParam>)>,
    diagnostic_path: &Path,
    extra_validation: Option<&RemappedValidation>,
    imported_devices: &radix_mir_lower::lower::ImportedDeviceIndex,
    consumer_device_roles: &HashMap<DefId, radix_mir::MirDeviceRole>,
    consumer_launch_entries: &HashSet<DefId>,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    if templates.is_empty() {
        merged.generic_templates = templates;
        merged.generic_params = generic_params;
        return Ok(merged);
    }
    let mut types =
        radix_types::TypeTable::from_snapshot(merged.validated.validation().types.snapshot())
            .map_err(|message| vec![mir_lowering_diag(diagnostic_path, message)])?;
    // KPC-WIRE done_when (b): the production package path invokes the
    // device-aware helper with the joined imported-device index — never the
    // empty-index wrapper — so imported generic device calls register their
    // concrete instances and signature-only routes fail closed. CDC-1: the
    // consumer units' own device roles and launch entries ride the same call
    // as the leaf-tail union (no second composition pass, no dual authority).
    let (mut program, prior_context) = merged.validated.into_parts();
    let joined_validation =
        extra_validation.map(|validation| validation.joined_with_context(&prior_context));
    let (extra, registrations) =
        radix_mir_lower::lower::instantiate_merged_generic_calls_with_tensor_edge_policy(
            &mut program,
            &templates,
            &generic_params,
            &mut types,
            &merged.interner,
            joined_validation
                .as_ref()
                .map(|validation| validation.struct_metadata())
                .as_ref(),
            &mut merged.device_function_metadata,
            imported_devices,
            consumer_device_roles,
            consumer_launch_entries,
            &merged.tensor_edge_policy_metadata,
            true,
        )
        .map_err(|errors| {
            errors
                .into_iter()
                // Preserve the named fail-closed issue (KPC-WIRE done_when
                // (e)): `imported_device_body_missing` and its siblings are
                // machine-readable identities, not message prose.
                .map(|error| mir_error_diag(diagnostic_path, error))
                .collect::<Vec<_>>()
        })?;
    merged.monomorphizations.extend(extra);
    merge_imported_device_registrations(&mut merged.imported_device_registrations, registrations);
    merged.generic_templates = templates;
    merged.generic_params = generic_params;
    let types: &'static radix_types::TypeTable = Box::leak(Box::new(types));
    let mut context = prior_context.with_types(types);
    sync_context_closure_environments(&mut context, &merged.closure_environments);
    context.layouts = radix_mir::MirLayoutTable::from_types(types);
    if let Some(extra) = extra_validation {
        extra.apply(&mut context);
    }
    merged.validated = radix_mir::ValidatedMir::new(program, context)
        .map_err(|errors| validation_diagnostics(diagnostic_path, errors))?;
    Ok(merged)
}

fn rewrite_function_type_ids(function: &mut MirFunction, rewrites: &[(TypeId, TypeId)]) {
    rewrite_type_id(&mut function.return_ty, rewrites);
    if let Some(error_ty) = &mut function.error_ty {
        rewrite_type_id(error_ty, rewrites);
    }
    for param in &mut function.params {
        rewrite_type_id(&mut param.ty, rewrites);
    }
    for local in &mut function.locals {
        rewrite_type_id(&mut local.ty, rewrites);
    }
    for temp in &mut function.temps {
        rewrite_type_id(&mut temp.ty, rewrites);
    }
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            rewrite_statement_type_id(statement, rewrites);
        }
        // Failable generic callees carry the same imported-unit type
        // args as direct calls; rewrite them on the try edge too.
        if let MirTerminatorKind::TryCall { callee, .. } = &mut block.terminator.kind
            && let MirCallee::Definition { type_args, .. } = callee
        {
            for type_arg in type_args {
                rewrite_type_id(type_arg, rewrites);
            }
        }
    }
}

pub(super) fn rewrite_type_id(ty: &mut radix_mir::MirType, rewrites: &[(TypeId, TypeId)]) {
    if let Some((_, to)) = rewrites.iter().find(|(from, _)| ty.semantic_id() == *from) {
        *ty = radix_mir::MirType::semantic(*to);
    }
}

pub(super) fn rewrite_statement_type_id(
    statement: &mut MirStatement,
    rewrites: &[(TypeId, TypeId)],
) {
    match &mut statement.kind {
        MirStatementKind::Assign { value, .. } => rewrite_type_id(&mut value.ty, rewrites),
        MirStatementKind::Call { callee, .. } => {
            // A generic callee's explicit type args are semantic types from
            // the imported unit's table; unrewritten ids alias unrelated
            // entry types after the merge (size literals become arbitrary
            // shapes and monomorphization rejects the kind).
            if let MirCallee::Definition { type_args, .. } = callee {
                for type_arg in type_args {
                    rewrite_type_id(type_arg, rewrites);
                }
            }
        }
        MirStatementKind::RuntimeCall { call, .. } => {
            rewrite_type_id(&mut call.return_ty, rewrites);
            match &mut call.intrinsic {
                MirIntrinsic::Convert(conversion) => {
                    // A conversion carries both source and target types outside
                    // the ordinary call return type. Imported units get their
                    // semantic type ids rewritten into the merged entry table;
                    // leaving either side behind makes the validator compare a
                    // remapped operand against a source-local conversion type.
                    rewrite_type_id(&mut conversion.source_ty, rewrites);
                    rewrite_type_id(&mut conversion.target_ty, rewrites);
                }
                MirIntrinsic::TypeCheck(check) => {
                    // The checked type is separate from the bool return type.
                    // A source-local id can name a different carrier after linking.
                    rewrite_type_id(&mut check.ty, rewrites);
                }
                _ => {}
            }
        }
        MirStatementKind::Construct { aggregate, .. } => {
            rewrite_type_id(&mut aggregate.ty, rewrites);
        }
    }
}

/// Rewrite a lowered source program's enum-variant definitions to the
/// consumer analysis's variant identities (codex-gap S1 U2 VALUE members).
///
/// A library unit constructs and projects its variants under its own variant
/// `DefId`s; the merged program's variant metadata and the runner's variant
/// tags are keyed by the *consumer* variant defs, so every
/// `EnumVariant`/`VariantField` reference in the lowered source program must
/// ride the nominal remap. Variant defs without a consumer mapping keep their
/// source-local value (entry-local types referenced from a sibling).
pub(super) fn rewrite_lowered_variant_defs<'a>(
    lowered: LoweredMirUnit<'a>,
    _variant_remap: &HashMap<DefId, DefId>,
    _diagnostic_path: &Path,
) -> LoweredMirUnit<'a> {
    // Program and proof-context variant identities move atomically with the
    // preceding type-table transition. Keep this named owned step so callers
    // cannot reintroduce a mutable carrier endpoint.
    lowered
}

fn rewrite_function_variant_defs(
    function: &mut MirFunction,
    variant_remap: &HashMap<DefId, DefId>,
) {
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            rewrite_statement_variant_defs(statement, variant_remap);
        }
        rewrite_terminator_variant_defs(&mut block.terminator, variant_remap);
    }
}

/// Project the source nominal-identity remap onto struct/union/interface
/// defs: every nominal the map sends to an entry-side `Type::{Struct,Enum,
/// Interface}(def)` becomes a `(source DefId → entry DefId)` rewrite, so
/// leftover unions intern under the merged identity instead of colliding on
/// source-local `DefIds` (cache.fab `KvDtype` / `VLayout` / `LayerStructure`).
fn build_nominal_def_remap(
    nominal_map: &HashMap<DefId, TypeId>,
    entry_types: &TypeTable,
) -> HashMap<DefId, DefId> {
    nominal_map
        .iter()
        .filter_map(|(source_def, entry_ty)| match entry_types.get(*entry_ty) {
            Type::Struct(entry_def) | Type::Enum(entry_def) | Type::Interface(entry_def) => {
                Some((*source_def, *entry_def))
            }
            _ => None,
        })
        .collect()
}

/// Rewrite `MirAggregateKind::Struct` defs in a lowered program through the
/// nominal struct-def remap (FMIR e2e-hardening CTO-1).
fn rewrite_lowered_struct_defs<'a>(
    lowered: LoweredMirUnit<'a>,
    _struct_remap: &HashMap<DefId, DefId>,
    _diagnostic_path: &Path,
) -> LoweredMirUnit<'a> {
    // Struct identities are likewise part of the atomic type-table rewrite.
    lowered
}

fn rewrite_function_struct_defs(function: &mut MirFunction, struct_remap: &HashMap<DefId, DefId>) {
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            match &mut statement.kind {
                MirStatementKind::Construct { aggregate, .. } => {
                    if let MirAggregateKind::Struct(def_id) = &mut aggregate.kind
                        && let Some(rewritten) = struct_remap.get(def_id)
                    {
                        aggregate.kind = MirAggregateKind::Struct(*rewritten);
                    }
                }
                MirStatementKind::RuntimeCall { call, .. } => {
                    if let MirIntrinsic::TensorLimes(radix_mir::MirEdgePolicy::Custom {
                        policy_def,
                        ..
                    }) = &mut call.intrinsic
                        && let Some(rewritten) = struct_remap.get(policy_def)
                    {
                        *policy_def = *rewritten;
                    }
                }
                MirStatementKind::Assign { .. } | MirStatementKind::Call { .. } => {}
            }
        }
    }
}

fn rewrite_statement_variant_defs(
    statement: &mut MirStatement,
    variant_remap: &HashMap<DefId, DefId>,
) {
    match &mut statement.kind {
        MirStatementKind::Assign { value, .. } => {
            rewrite_value_variant_defs(value, variant_remap);
        }
        MirStatementKind::Call {
            callee: _, args, ..
        } => {
            for arg in args {
                rewrite_operand_variant_defs(arg, variant_remap);
            }
        }
        MirStatementKind::RuntimeCall { call, .. } => {
            for arg in &mut call.args {
                rewrite_operand_variant_defs(arg, variant_remap);
            }
        }
        MirStatementKind::Construct { aggregate, .. } => {
            if let MirAggregateKind::EnumVariant(def_id) = &mut aggregate.kind
                && let Some(rewritten) = variant_remap.get(def_id)
            {
                aggregate.kind = MirAggregateKind::EnumVariant(*rewritten);
            }
            rewrite_aggregate_variant_defs(aggregate, variant_remap);
        }
    }
}

fn rewrite_terminator_variant_defs(
    terminator: &mut MirTerminator,
    variant_remap: &HashMap<DefId, DefId>,
) {
    match &mut terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            rewrite_operand_variant_defs(operand, variant_remap);
        }
        MirTerminatorKind::TryCall {
            callee: _, args, ..
        } => {
            for arg in args {
                rewrite_operand_variant_defs(arg, variant_remap);
            }
        }
        MirTerminatorKind::Branch { condition, .. } => {
            rewrite_operand_variant_defs(condition, variant_remap);
        }
        MirTerminatorKind::Switch { value, .. } => {
            rewrite_operand_variant_defs(value, variant_remap);
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

fn rewrite_value_variant_defs(value: &mut MirValue, variant_remap: &HashMap<DefId, DefId>) {
    match &mut value.kind {
        MirValueKind::Operand(operand) => rewrite_operand_variant_defs(operand, variant_remap),
        MirValueKind::Closure(closure) => {
            rewrite_operand_variant_defs(&mut closure.environment, variant_remap);
        }
        MirValueKind::Unary { operand, .. } => {
            rewrite_operand_variant_defs(operand, variant_remap);
        }
        MirValueKind::Binary { lhs, rhs, .. } => {
            rewrite_operand_variant_defs(lhs, variant_remap);
            rewrite_operand_variant_defs(rhs, variant_remap);
        }
        MirValueKind::Option(op) => rewrite_option_variant_defs(op, variant_remap),
        MirValueKind::TensorExtent { tensor, .. } => {
            rewrite_operand_variant_defs(tensor, variant_remap);
        }
    }
}

fn rewrite_operand_variant_defs(operand: &mut MirOperand, variant_remap: &HashMap<DefId, DefId>) {
    if let MirOperand::Place(place) = operand {
        rewrite_place_variant_defs(place, variant_remap);
    }
}

fn rewrite_place_variant_defs(place: &mut MirPlace, variant_remap: &HashMap<DefId, DefId>) {
    for projection in &mut place.projections {
        match projection {
            MirProjection::VariantField { variant, .. } => {
                if let Some(rewritten) = variant_remap.get(variant) {
                    *variant = *rewritten;
                }
            }
            MirProjection::Index(operand) | MirProjection::ProvenIndex(operand) => {
                rewrite_operand_variant_defs(operand, variant_remap);
            }
            MirProjection::Field(_)
            | MirProjection::ClosureCapture { .. }
            | MirProjection::VectorLane(_)
            | MirProjection::MatrixCell { .. }
            | MirProjection::SharedCell => {}
        }
    }
}

fn rewrite_aggregate_variant_defs(
    aggregate: &mut MirAggregate,
    variant_remap: &HashMap<DefId, DefId>,
) {
    match &mut aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        rewrite_operand_variant_defs(operand, variant_remap);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                rewrite_operand_variant_defs(&mut item.value, variant_remap);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                rewrite_operand_variant_defs(&mut item.key, variant_remap);
                rewrite_operand_variant_defs(&mut item.value, variant_remap);
            }
        }
    }
}

fn rewrite_option_variant_defs(op: &mut MirOptionOp, variant_remap: &HashMap<DefId, DefId>) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand)
        | MirOptionOp::Unwrap { value: operand, .. } => {
            rewrite_operand_variant_defs(operand, variant_remap);
        }
        MirOptionOp::Coalesce { value, fallback } => {
            rewrite_operand_variant_defs(value, variant_remap);
            rewrite_operand_variant_defs(fallback, variant_remap);
        }
        MirOptionOp::Chain { base, link } => {
            rewrite_operand_variant_defs(base, variant_remap);
            rewrite_option_chain_variant_defs(link, variant_remap);
        }
        MirOptionOp::None => {}
    }
}

fn rewrite_option_chain_variant_defs(
    link: &mut MirOptionChainLink,
    variant_remap: &HashMap<DefId, DefId>,
) {
    match link {
        MirOptionChainLink::VariantField { variant, .. } => {
            if let Some(rewritten) = variant_remap.get(variant) {
                *variant = *rewritten;
            }
        }
        MirOptionChainLink::Index(operand) => {
            rewrite_operand_variant_defs(operand, variant_remap);
        }
        MirOptionChainLink::Call { callee: _, args } => {
            for arg in args {
                rewrite_operand_variant_defs(arg, variant_remap);
            }
        }
        MirOptionChainLink::Field(_) => {}
    }
}

pub(super) fn install_cli_dispatch_entry<'a>(
    lowered: LoweredMirUnit<'a>,
    command: MirFunctionId,
    entry_path: &Path,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    let types = lowered.validated.validation().types.clone();
    lowered
        .patch_program(|program| {
            let referenced = radix_mir::names::referenced_functions(program);
            let Some(entry_index) = program.functions.iter().position(|function| {
                is_explicit_entry_function(function, &types) && !referenced.contains(&function.id)
            }) else {
                return Err(vec![mir_diag(
                    entry_path,
                    "package MIR could not find root CLI entry function",
                )]);
            };
            let span = program.functions[entry_index].span;
            program.functions[entry_index].locals.clear();
            program.functions[entry_index].temps.clear();
            program.functions[entry_index].blocks = vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![MirStatement {
                    kind: MirStatementKind::Call {
                        destination: None,
                        callee: MirCallee::Function(command),
                        args: Vec::new(),
                    },
                    span,
                }],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span,
                },
                span,
            }];
            Ok(())
        })
        .map_err(|error| {
            error.fold(
                |diagnostics| diagnostics,
                |errors| validation_diagnostics(entry_path, errors),
            )
        })
}

pub(super) fn is_explicit_entry_function(
    function: &MirFunction,
    types: &radix_module::semantic::TypeTable,
) -> bool {
    function.source.is_none()
        && function.name.is_none()
        && function.params.is_empty()
        && matches!(
            types.get(function.return_ty.semantic_id()),
            Type::Primitive(Primitive::Vacuum)
        )
}

pub(super) fn lower_unit<'a>(
    unit: &'a mut AnalyzedProgramNode,
    cli_entry_records: &CliEntryRecords,
    no_fuse: bool,
    kernel_lowering: KernelLowering,
    imported_function_params: &std::sync::Arc<
        HashMap<DefId, Vec<radix_mir_lower::lower::ImportedCallableParam>>,
    >,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    let bundle = super::air_backward::prepare(&mut unit.analysis)
        .map_err(|message| vec![mir_lowering_diag(&unit.path, message)])?;

    let result = if unit.analysis.cli_program.is_some() {
        let fields = cli_entry_records
            .get(&unit.path)
            .cloned()
            .unwrap_or_default();
        lower_analyzed_unit(
            unit.analysis.application_view(),
            radix_mir_lower::lower::MirLoweringOptions {
                cli_parsed_record_fields_by_type: std::sync::Arc::new(fields.into_iter().collect()),
                kernel_lowering,
                imported_function_params: std::sync::Arc::new(
                    imported_function_params
                        .iter()
                        .map(|(def, params)| (*def, params.clone()))
                        .collect(),
                ),
                ..Default::default()
            },
        )
    } else {
        lower_analyzed_unit(
            unit.analysis.application_view(),
            radix_mir_lower::lower::MirLoweringOptions {
                kernel_lowering,
                imported_function_params: std::sync::Arc::new(
                    imported_function_params
                        .iter()
                        .map(|(def, params)| (*def, params.clone()))
                        .collect(),
                ),
                ..Default::default()
            },
        )
    };
    let mut lowered = result.map_err(|errors| {
        errors
            .into_iter()
            .map(|error| mir_error_diag(&unit.path, error))
            .collect::<Vec<Diagnostic>>()
    })?;

    if let Some(bundle) = bundle {
        lowered = super::air_backward::apply(lowered, bundle, &unit.analysis, no_fuse)
            .map_err(|message| vec![mir_lowering_diag(&unit.path, message)])?;
    }

    Ok(lowered)
}

/// Merge one appended unit's synthetic-kernel metadata into the merged map,
/// shifting keys and parent identities by the unit's id offset (KCM-05).
///
/// A row whose key or parent id would overflow the merged id space fails
/// closed with the shared function-id overflow diagnostic — a shifted row
/// that cannot be represented never silently joins the merged program.
///
/// # Errors
///
/// Returns the function-id overflow diagnostic when a shifted key or parent
/// id exceeds `u32::MAX`.
fn merge_shifted_device_function_metadata(
    merged: &mut MirDeviceFunctionMetadataMap,
    incoming: &MirDeviceFunctionMetadataMap,
    offset: u32,
    path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    if offset == 0 {
        merged.extend(incoming.iter().map(|(key, row)| (*key, *row)));
        return Ok(());
    }
    let shift = |id: MirFunctionId| -> Result<MirFunctionId, Vec<Diagnostic>> {
        checked_shifted_function_id(id, offset, path)
    };
    for (key, row) in incoming {
        let key = shift(*key)?;
        let mut row = *row;
        row.identity.parent = shift(row.identity.parent)?;
        merged.insert(key, row);
    }
    Ok(())
}

/// A pending package unit and a linked library carry the same owned parts
/// (program, closure environments, templates, device/display metadata), so
/// the merge steps — id shift, metadata join, prior-template instantiation,
/// context rebuild — exist once, in `append_shifted_library_parts`. This
/// adapter only disassembles the unit into those parts; the caller's
/// `shift_templates` offset matches the one the shared path computes.
fn append_shifted_program<'merged>(
    merged: LoweredMirUnit<'merged>,
    incoming: LoweredMirUnit<'_>,
    diagnostic_path: &Path,
    extra: &RemappedValidation,
    prior_templates: &[MirFunction],
    prior_generic_params: &[(DefId, Vec<radix_mir_lower::lower::MirGenericParam>)],
    imported_devices: &radix_mir_lower::lower::ImportedDeviceIndex,
    consumer_device_roles: &HashMap<DefId, radix_mir::MirDeviceRole>,
    consumer_launch_entries: &HashSet<DefId>,
) -> Result<(LoweredMirUnit<'merged>, LibraryMirParts), Vec<Diagnostic>> {
    let (program, incoming_context) = incoming.validated.into_parts();
    let parts = LibraryMirParts {
        program,
        environments: incoming.closure_environments,
        templates: incoming.generic_templates,
        generic_params: incoming.generic_params,
        tensor_edge_policy_metadata: incoming.tensor_edge_policy_metadata,
        device_function_metadata: incoming.device_function_metadata,
        display_tokens: incoming_context.display_tokens,
    };
    let (merged, parts, _) = append_shifted_library_parts(
        merged,
        parts,
        diagnostic_path,
        extra,
        prior_templates,
        prior_generic_params,
        imported_devices,
        consumer_device_roles,
        consumer_launch_entries,
    )?;
    Ok((merged, parts))
}

fn append_shifted_library_parts<'a>(
    mut merged: LoweredMirUnit<'a>,
    mut library: LibraryMirParts,
    diagnostic_path: &Path,
    extra: &RemappedValidation,
    prior_templates: &[MirFunction],
    prior_generic_params: &[(DefId, Vec<radix_mir_lower::lower::MirGenericParam>)],
    imported_devices: &radix_mir_lower::lower::ImportedDeviceIndex,
    consumer_device_roles: &HashMap<DefId, radix_mir::MirDeviceRole>,
    consumer_launch_entries: &HashSet<DefId>,
) -> Result<(LoweredMirUnit<'a>, LibraryMirParts, u32), Vec<Diagnostic>> {
    let (mut merged_program, mut context) = merged.validated.into_parts();
    let offset = append_shifted_parts(
        &mut merged_program,
        &mut merged.closure_environments,
        &mut library.program,
        &mut library.environments,
        diagnostic_path,
    )?;
    merge_shifted_device_function_metadata(
        &mut merged.device_function_metadata,
        &library.device_function_metadata,
        offset,
        diagnostic_path,
    )?;
    context
        .display_tokens
        .absorb(&library.display_tokens, |id| MirFunctionId(id.0 + offset));
    merge_tensor_edge_policy_metadata(
        &mut merged.tensor_edge_policy_metadata,
        &library.tensor_edge_policy_metadata,
    );
    if !prior_templates.is_empty() || !library.templates.is_empty() {
        let mut current_templates = library.templates.clone();
        shift_templates(&mut current_templates, offset);
        let mut templates = prior_templates.to_vec();
        templates.extend(current_templates);
        let mut generic_params = prior_generic_params.to_vec();
        generic_params.extend(library.generic_params.clone());
        let mut types = TypeTable::from_snapshot(context.types.snapshot())
            .map_err(|message| vec![mir_lowering_diag(diagnostic_path, message)])?;
        let (monomorphizations, registrations) =
            radix_mir_lower::lower::instantiate_merged_generic_calls_with_tensor_edge_policy(
                &mut merged_program,
                &templates,
                &generic_params,
                &mut types,
                &merged.interner,
                Some(extra.joined_with_context(&context).struct_metadata()).as_ref(),
                &mut merged.device_function_metadata,
                imported_devices,
                consumer_device_roles,
                consumer_launch_entries,
                &merged.tensor_edge_policy_metadata,
                false,
            )
            .map_err(|errors| {
                errors
                    .into_iter()
                    .map(|error| mir_error_diag(diagnostic_path, error))
                    .collect::<Vec<_>>()
            })?;
        merged.monomorphizations.extend(monomorphizations);
        merge_imported_device_registrations(
            &mut merged.imported_device_registrations,
            registrations,
        );
        let types: &'static TypeTable = Box::leak(Box::new(types));
        context = context.with_types(types);
    }
    sync_context_closure_environments(&mut context, &merged.closure_environments);
    extra.apply(&mut context);
    merged.validated = radix_mir::ValidatedMir::new(merged_program, context)
        .map_err(|errors| validation_diagnostics(diagnostic_path, errors))?;
    Ok((merged, library, offset))
}

/// Append owned program parts (functions + closure environments) into the
/// merged program, shifting function and closure ids past the merged prefix.
pub(super) fn append_shifted_parts(
    merged_program: &mut MirProgram,
    merged_closure_environments: &mut Vec<MirClosureEnvironment>,
    program: &mut MirProgram,
    closure_environments: &mut Vec<MirClosureEnvironment>,
    diagnostic_path: &Path,
) -> Result<u32, Vec<Diagnostic>> {
    let offset = MirProgram::allocate_function_id(&merged_program.functions)
        .map_err(|_| {
            vec![function_id_overflow_diag(
                diagnostic_path,
                "package MIR function id space is exhausted",
            )]
        })?
        .0;
    ensure_shift_fits(program, closure_environments, offset, diagnostic_path)?;
    shift_program_ids(program, closure_environments, offset);
    // Closure environments are collected on the Vec and folded into a fresh
    // ValidatedMir token after all merges complete (rebuild_merged_validated).
    merged_closure_environments.append(closure_environments);
    merged_program.functions.append(&mut program.functions);
    Ok(offset)
}

/// Closure-environment capture names are lowering-interner symbols like
/// function/param/local names (`remap_program_text_symbols`); the merged
/// program resolves names through the entry interner, so the environment
/// metadata must ride the same remap or the construct-vs-metadata match
/// compares symbols from unrelated interners.
fn remap_closure_environment_symbols(
    environments: &mut [MirClosureEnvironment],
    source: &Interner,
    target: &mut Interner,
) {
    for environment in environments {
        for capture in &mut environment.captures {
            capture.name = remap_symbol(capture.name, source, target);
        }
    }
}

fn ensure_shift_fits(
    program: &MirProgram,
    closure_environments: &[MirClosureEnvironment],
    offset: u32,
    path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let max_function_id = program.functions.iter().map(|function| function.id.0).max();
    let max_environment_id = closure_environments
        .iter()
        .map(|environment| environment.id.0)
        .max();
    if max_function_id.is_some_and(|id| id.checked_add(offset).is_none())
        || max_environment_id.is_some_and(|id| id.checked_add(offset).is_none())
    {
        return Err(vec![function_id_overflow_diag(
            path,
            "package MIR append would overflow an id",
        )]);
    }
    Ok(())
}

pub(super) fn checked_shifted_function_id(
    id: MirFunctionId,
    offset: u32,
    path: &Path,
) -> Result<MirFunctionId, Vec<Diagnostic>> {
    id.0.checked_add(offset).map(MirFunctionId).ok_or_else(|| {
        vec![function_id_overflow_diag(
            path,
            "package MIR CLI dispatch function id would overflow",
        )]
    })
}

fn function_id_overflow_diag(path: &Path, message: &str) -> Diagnostic {
    mir_issue_diag(path, "package_mir_function_id_overflow", message)
}

/// Remember the defining file's tokens for each printing generic template of
/// a unit. The tokens are the unit's own file words, never a sibling's.
fn record_template_display_tokens(
    template_tokens: &mut HashMap<
        DefId,
        radix_module::runtime_contract::display_tokens::DisplayTokens,
    >,
    templates: &[MirFunction],
    tokens: &radix_module::runtime_contract::display_tokens::DisplayTokens,
) {
    for template in templates
        .iter()
        .filter(|template| radix_mir_lower::lower::holds_print_site(template))
    {
        if let Some(source) = template.source {
            template_tokens
                .entry(source)
                .or_insert_with(|| tokens.clone());
        }
    }
}

/// Give every monomorphization clone that has no tokens yet the tokens of the
/// file defining its template. A clone already in the table (a same-file
/// clone recorded at lowering) keeps its own entry; a clone whose template
/// has no recorded tokens stays without an id.
fn assign_clone_display_tokens(
    table: &mut radix_mir::MirDisplayTokens,
    monomorphizations: &[radix_mir::MirMonomorphization],
    template_tokens: &HashMap<DefId, radix_module::runtime_contract::display_tokens::DisplayTokens>,
) {
    for clone in monomorphizations {
        if table.by_function.contains_key(&clone.function) {
            continue;
        }
        if let Some(tokens) = template_tokens.get(&clone.key.source) {
            table.assign(clone.function, tokens);
        }
    }
}

/// Rebuild the validated-MIR token after package merging shifts IDs and
/// appends `functions/closure_environments`. The prior token proved the
/// pre-merge program; the merged program needs its own proof.
pub(super) fn rebuild_merged_validated<'a>(
    mut merged: LoweredMirUnit<'a>,
    diagnostic_path: &Path,
    extra: Option<&RemappedValidation>,
    template_tokens: &HashMap<DefId, radix_module::runtime_contract::display_tokens::DisplayTokens>,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    let (program, mut context) = merged.validated.into_parts();
    assign_clone_display_tokens(
        &mut context.display_tokens,
        &merged.monomorphizations,
        template_tokens,
    );
    sync_context_closure_environments(&mut context, &merged.closure_environments);
    if let Some(extra) = extra {
        extra.apply(&mut context);
    }
    merged.validated = radix_mir::ValidatedMir::new(program, context)
        .map_err(|errors| validation_diagnostics(diagnostic_path, errors))?;
    Ok(merged)
}

/// `function.source` is the HIR declaration identity, not the MIR instance
/// identity. EBNF `funcDecl` is one `IDENTIFIER` plus optional
/// `genericParams`; `MirMonomorphizationKey` then distinguishes instances by
/// `(source, type_args, shape_args)`. `radix_mir::names` already emits
/// specialized names when several functions share a source. Reject only when
/// two different declarations (name or span) claim one source.
pub(super) fn ensure_unique_definition_sources(
    program: &MirProgram,
    path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let mut seen = HashMap::new();
    for function in &program.functions {
        let Some(source) = function.source else {
            continue;
        };
        let identity = (function.name, function.span);
        if let Some(existing) = seen.insert(source, identity)
            && existing != identity
        {
            return Err(vec![
                mir_issue_diag(
                    path,
                    "package_mir_duplicate_function_source",
                    format!(
                        "package MIR link found duplicate function source def#{}",
                        source.0
                    ),
                )
                .with_arg("def_id", source.0.to_string()),
            ]);
        }
    }
    Ok(())
}

/// Canonicalize direct `Definition` callees into `Function` callees in the
/// merged program.
///
/// The radix-air lowering emits `MirCallee::Definition { source, type_args }`
/// for every call edge. The boundary-ABI lane (DDCP3-U5) admits exactly the
/// direct-call shapes — `Function`, function-constant `Value`, and `Closure`
/// — and rejects `Definition` callees as an unsupported callsite shape before
/// emission. A definition edge with no type arguments that resolves to
/// exactly one function in the merged program IS a direct call; rewriting it
/// to `MirCallee::Function` keeps the boundary lane's single emitted form
/// while preserving call identity. Generic (typed) or ambiguous edges stay
/// `Definition` and are still rejected by the gate when they cross a
/// boundary.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn canonicalize_direct_definition_callees<'a>(
    lowered: LoweredMirUnit<'a>,
    diagnostic_path: &Path,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    crate::mir_canonicalize::canonicalize_direct_definition_callees(lowered, diagnostic_path)
}

pub(super) fn shift_program_ids(
    program: &mut MirProgram,
    closure_environments: &mut [MirClosureEnvironment],
    offset: u32,
) {
    if offset == 0 {
        return;
    }
    for function in &mut program.functions {
        shift_function_id(&mut function.id, offset);
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                shift_statement_ids(statement, offset);
            }
            shift_terminator_ids(&mut block.terminator, offset);
        }
    }
    for environment in closure_environments {
        shift_environment_id(&mut environment.id, offset);
        shift_function_id(&mut environment.function, offset);
        for capture in &mut environment.captures {
            shift_place_ids(&mut capture.source_place, offset);
        }
    }
}

pub(super) fn shift_statement_ids(statement: &mut MirStatement, offset: u32) {
    match &mut statement.kind {
        MirStatementKind::Assign { place, value } => {
            shift_place_ids(place, offset);
            shift_value_ids(value, offset);
        }
        MirStatementKind::Call {
            destination,
            callee,
            args,
        } => {
            if let Some(destination) = destination {
                shift_place_ids(destination, offset);
            }
            shift_callee_ids(callee, offset);
            for arg in args {
                shift_operand_ids(arg, offset);
            }
        }
        MirStatementKind::RuntimeCall { destination, call } => {
            if let Some(destination) = destination {
                shift_place_ids(destination, offset);
            }
            shift_runtime_call_ids(call, offset);
        }
        MirStatementKind::Construct {
            destination,
            aggregate,
        } => {
            shift_place_ids(destination, offset);
            shift_aggregate_ids(aggregate, offset);
        }
    }
}

pub(super) fn shift_terminator_ids(terminator: &mut MirTerminator, offset: u32) {
    match &mut terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            shift_operand_ids(operand, offset);
        }
        MirTerminatorKind::TryCall {
            destination,
            callee,
            args,
            error_place,
            ..
        } => {
            if let Some(destination) = destination {
                shift_place_ids(destination, offset);
            }
            shift_callee_ids(callee, offset);
            for arg in args {
                shift_operand_ids(arg, offset);
            }
            shift_place_ids(error_place, offset);
        }
        MirTerminatorKind::Branch { condition, .. } => shift_operand_ids(condition, offset),
        MirTerminatorKind::Switch { value, cases, .. } => {
            shift_operand_ids(value, offset);
            for case in cases {
                shift_switch_case_ids(case, offset);
            }
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

pub(super) fn shift_value_ids(value: &mut MirValue, offset: u32) {
    match &mut value.kind {
        MirValueKind::Operand(operand) | MirValueKind::Unary { operand, .. } => {
            shift_operand_ids(operand, offset);
        }
        MirValueKind::Closure(closure) => shift_closure_value_ids(closure, offset),
        MirValueKind::Binary { lhs, rhs, .. } => {
            shift_operand_ids(lhs, offset);
            shift_operand_ids(rhs, offset);
        }
        MirValueKind::Option(op) => shift_option_ids(op, offset),
        MirValueKind::TensorExtent { tensor, .. } => shift_operand_ids(tensor, offset),
    }
}

pub(super) fn shift_operand_ids(operand: &mut MirOperand, offset: u32) {
    match operand {
        MirOperand::Place(place) => shift_place_ids(place, offset),
        MirOperand::Constant(constant) => shift_constant_ids(constant, offset),
        MirOperand::Temp(_) | MirOperand::Value(_) => {}
    }
}

pub(super) fn shift_place_ids(place: &mut MirPlace, offset: u32) {
    for projection in &mut place.projections {
        match projection {
            MirProjection::ClosureCapture { environment, .. } => {
                shift_environment_id(environment, offset);
            }
            MirProjection::Index(operand) | MirProjection::ProvenIndex(operand) => {
                shift_operand_ids(operand, offset);
            }
            MirProjection::Field(_)
            | MirProjection::VariantField { .. }
            | MirProjection::VectorLane(_)
            | MirProjection::MatrixCell { .. }
            | MirProjection::SharedCell => {}
        }
    }
}

pub(super) fn shift_constant_ids(constant: &mut MirConstant, offset: u32) {
    if let MirConstant::Function(id) = constant {
        shift_function_id(id, offset);
    }
}

pub(super) fn shift_callee_ids(callee: &mut MirCallee, offset: u32) {
    match callee {
        MirCallee::Function(id) => shift_function_id(id, offset),
        MirCallee::Closure(closure) => shift_closure_callee_ids(closure, offset),
        MirCallee::Value(operand) => shift_operand_ids(operand, offset),
        MirCallee::Definition { .. } => {}
    }
}

pub(super) fn shift_runtime_call_ids(call: &mut MirRuntimeCall, offset: u32) {
    for arg in &mut call.args {
        shift_operand_ids(arg, offset);
    }
    if let MirIntrinsic::TensorLimes(radix_mir::MirEdgePolicy::Custom {
        remap, transform, ..
    }) = &mut call.intrinsic
    {
        shift_function_id(remap, offset);
        if let Some(transform) = transform {
            shift_function_id(transform, offset);
        }
    }
}

pub(super) fn shift_aggregate_ids(aggregate: &mut MirAggregate, offset: u32) {
    match &mut aggregate.kind {
        // The merged environment registry is re-keyed by the same offset
        // (`shift_program_ids`); a shifted unit's `construct closure_env`
        // kind id must follow it or the construct names a pre-merge key.
        MirAggregateKind::ClosureEnvironment(id) => shift_environment_id(id, offset),
        // Struct/variant DefIds are nominal identities riding the nominal
        // remap, never the function/env merge offset.
        MirAggregateKind::Struct(_)
        | MirAggregateKind::EnumVariant(_)
        | MirAggregateKind::Tuple
        | MirAggregateKind::Array
        | MirAggregateKind::Record
        | MirAggregateKind::Map
        | MirAggregateKind::Set
        | MirAggregateKind::Tensor
        | MirAggregateKind::SparseTensor
        | MirAggregateKind::Interval { .. }
        | MirAggregateKind::Vector
        | MirAggregateKind::Matrix
        | MirAggregateKind::SharedCell => {}
    }
    match &mut aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        shift_operand_ids(operand, offset);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                shift_operand_ids(&mut item.value, offset);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                shift_operand_ids(&mut item.key, offset);
                shift_operand_ids(&mut item.value, offset);
            }
        }
    }
}

pub(super) fn shift_switch_case_ids(case: &mut MirSwitchCase, offset: u32) {
    shift_constant_ids(&mut case.value, offset);
}

pub(super) fn shift_option_ids(op: &mut MirOptionOp, offset: u32) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand)
        | MirOptionOp::Unwrap { value: operand, .. } => shift_operand_ids(operand, offset),
        MirOptionOp::Coalesce { value, fallback } => {
            shift_operand_ids(value, offset);
            shift_operand_ids(fallback, offset);
        }
        MirOptionOp::Chain { base, link } => {
            shift_operand_ids(base, offset);
            shift_option_chain_link_ids(link, offset);
        }
        MirOptionOp::None => {}
    }
}

pub(super) fn shift_option_chain_link_ids(link: &mut MirOptionChainLink, offset: u32) {
    match link {
        MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => {}
        MirOptionChainLink::Index(operand) => shift_operand_ids(operand, offset),
        MirOptionChainLink::Call { callee, args } => {
            shift_callee_ids(callee, offset);
            for arg in args {
                shift_operand_ids(arg, offset);
            }
        }
    }
}

pub(super) fn shift_closure_value_ids(closure: &mut MirClosureValue, offset: u32) {
    shift_function_id(&mut closure.function, offset);
    shift_environment_id(&mut closure.environment_id, offset);
    shift_operand_ids(&mut closure.environment, offset);
}

pub(super) fn shift_closure_callee_ids(closure: &mut MirClosureCallee, offset: u32) {
    shift_function_id(&mut closure.function, offset);
    shift_environment_id(&mut closure.environment_id, offset);
    shift_operand_ids(&mut closure.environment, offset);
}

pub(super) fn shift_function_id(id: &mut MirFunctionId, offset: u32) {
    id.0 += offset;
}

pub(super) fn shift_environment_id(id: &mut MirClosureEnvironmentId, offset: u32) {
    id.0 += offset;
}

// ── S1 U3 reachability: linked library functions are lowered only when
//    reachable from the package's synthetic call graph (default = pruning) ──

/// Collect every package-MIR synthetic def id referenced by the package's
/// rewritten unit HIR. The linker installed these `Path(synthetic)` targets
/// for used namespace calls (library exports) and const data members, so the
/// reference set is exactly the entry's call-graph seeds into the linked
/// libraries.
fn collect_package_synthetic_references(package: &AnalyzedProgram) -> HashSet<DefId> {
    let mut references = HashSet::new();
    for unit in package.nodes.values() {
        let mut collector = SyntheticReferenceCollector::default();
        radix_module::hir::visit::walk_program(&mut collector, &unit.analysis.hir);
        references.extend(collector.references);
    }
    references
}

/// Collect static custom edge-policy callbacks as package reachability edges.
/// A generic `limes<P>()` template has only a Deferred policy at this stage,
/// so the ordinary MIR reference scan cannot see the callback that will be
/// installed after cross-unit specialization. Resolve imported methods through
/// the caller's canonical nominal map, matching the later sidecar rewrite.
fn collect_tensor_edge_policy_callback_references(
    package: &AnalyzedProgram,
    method_targets: &HashMap<PathBuf, HashMap<(DefId, String), DefId>>,
    references: &mut HashSet<DefId>,
) {
    for unit in package.nodes.values() {
        let methods = method_targets.get(&unit.path);
        for provider in &unit.analysis.tensor_edge_policies.provider_impls {
            let radix_module::semantic::ResolvedTensorEdgePolicy::Custom {
                policy_def,
                remap,
                transform,
            } = provider.policy
            else {
                continue;
            };
            let callback = |name: &str, source: DefId| {
                methods
                    .and_then(|targets| targets.get(&(policy_def, name.to_owned())).copied())
                    .unwrap_or(source)
            };
            references.insert(callback("remap", remap));
            if let Some(transform) = transform {
                references.insert(callback("transform", transform));
            }
        }
    }
}

#[derive(Default)]
struct SyntheticReferenceCollector {
    references: HashSet<DefId>,
}

impl radix_module::hir::visit::HirVisitor for SyntheticReferenceCollector {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Path(def_id) = &expr.kind
            && def_id.is_package_mir_synthetic()
        {
            self.references.insert(*def_id);
        }
        radix_module::hir::visit::walk_expr(self, expr);
    }
}

/// Drop linked-library functions unreachable from the package's synthetic
/// call graph (S1 U3 reachability, default = pruning).
///
/// The reachable set is a global fixpoint over the linked library programs:
/// the package units' synthetic references seed the closure, each library's
/// BFS follows intra-program function edges (direct callees, closure bodies,
/// function constants) and `MirCallee::Definition { source }` edges (calls
/// into sibling libraries), and newly discovered definition sources extend
/// the seed until no library gains a function. Closure environments
/// referenced by kept functions are kept; unreferenced ones are dropped
/// alongside their (pruned) functions.
struct LibraryMirParts {
    program: MirProgram,
    environments: Vec<MirClosureEnvironment>,
    templates: Vec<MirFunction>,
    generic_params: Vec<(DefId, Vec<radix_mir_lower::lower::MirGenericParam>)>,
    tensor_edge_policy_metadata: MirTensorEdgePolicyMetadata,
    /// Synthetic-kernel metadata carried from the lowered unit; merged with
    /// the same id shift as the program (KCM-05).
    device_function_metadata: MirDeviceFunctionMetadataMap,
    /// The unit's display-token table, keyed by the unit's own function ids
    /// (absorbed with the merge shift like the program itself).
    display_tokens: radix_mir::MirDisplayTokens,
}

fn prune_unreachable_library_functions(
    library_parts: &mut [LibraryMirParts],
    reachable_seed: &mut HashSet<DefId>,
) {
    if library_parts.is_empty() {
        return;
    }
    loop {
        let mut new_seed = HashSet::new();
        let mut keep_sets = Vec::with_capacity(library_parts.len());
        for library in library_parts.iter() {
            let (reachable, definitions) = reachable_library_function_set(
                &library.program,
                &library.templates,
                reachable_seed,
            );
            keep_sets.push(reachable);
            new_seed.extend(definitions);
        }
        let grew = new_seed
            .iter()
            .any(|definition| !reachable_seed.contains(definition));
        reachable_seed.extend(new_seed);
        if grew {
            continue;
        }
        // Final keep sets (computed against the settled seed): filter each
        // library program to its reachable functions and keep only the
        // closure environments those functions reference.
        for (library, keep) in library_parts.iter_mut().zip(keep_sets.iter()) {
            let mut referenced_environments = HashSet::new();
            for function_id in keep.iter().copied() {
                referenced_environments.extend(
                    scan_mir_function_references(&library.program, function_id).environments,
                );
            }
            // Rows keyed by a pruned program function go with it. A key that
            // names no program function at all is a generic template's row
            // (templates have no `program.functions` entry) and stays.
            let program_ids: HashSet<MirFunctionId> = library
                .program
                .functions
                .iter()
                .map(|function| function.id)
                .collect();
            library
                .device_function_metadata
                .retain(|key, _| keep.contains(key) || !program_ids.contains(key));
            library
                .display_tokens
                .by_function
                .retain(|function, _| keep.contains(function));
            library
                .program
                .functions
                .retain(|function| keep.contains(&function.id));
            library
                .environments
                .retain(|environment| referenced_environments.contains(&environment.id));
            // The reachability pruning leaves function-id gaps; the merge
            // shift assumes each appended program's ids are zero-based and
            // contiguous, so re-index the kept functions before merging.
            reindex_function_ids(
                &mut library.program,
                &mut library.templates,
                &mut library.environments,
                &mut library.device_function_metadata,
                &mut library.display_tokens,
            );
            library.templates.retain(|template| {
                template
                    .source
                    .is_some_and(|source| reachable_seed.contains(&source))
            });
            library.generic_params.retain(|(source, _)| {
                library
                    .templates
                    .iter()
                    .any(|template| template.source == Some(*source))
            });
        }
        break;
    }
}

/// Compact a pruned library program's function ids to a contiguous 0..K
/// range and remap every function-id reference.
///
/// `prune_unreachable_library_functions` removes unreachable functions,
/// leaving id gaps. `append_shifted_parts` later shifts an appended program
/// by the current merged length and assumes the appended program's ids were
/// zero-based and contiguous — with gaps, two appended programs can land on
/// the same shifted id (duplicate MIR function ids in the merged program).
/// Block/local/temp ids stay per-function-scoped and untouched here; the
/// merge's uniform shift handles them.
///
/// The library's synthetic-kernel `device_function_metadata` rows are keyed
/// (and parented) by function id, so surviving rows move to their functions'
/// new ids through the same map. The caller has already dropped the rows of
/// pruned functions; a row whose key is not a program function (a generic
/// template's row) keeps its key.
///
/// `templates` are the unit's extracted generic templates. Their own ids are
/// outside the program (the template is not a program function), so they are
/// not remapped; their `MirCallee::Function` references must be, because a
/// template's callee can name a surviving synthetic helper whose program id
/// just moved. Leaving those stale lets a template call the compacted id of
/// an unrelated function — or, after the merge shift, the mono-instance id
/// itself.
fn reindex_function_ids(
    program: &mut MirProgram,
    templates: &mut [MirFunction],
    environments: &mut [MirClosureEnvironment],
    device_function_metadata: &mut MirDeviceFunctionMetadataMap,
    display_tokens: &mut radix_mir::MirDisplayTokens,
) {
    let mut remap = HashMap::new();
    for (function, index) in program.functions.iter().zip(0_u32..) {
        remap.insert(function.id, MirFunctionId(index));
    }
    remap_device_function_metadata(device_function_metadata, &remap);
    display_tokens.by_function = std::mem::take(&mut display_tokens.by_function)
        .into_iter()
        .map(|(function, id)| (remap.get(&function).copied().unwrap_or(function), id))
        .collect();
    let contiguous = program
        .functions
        .iter()
        .zip(0_u32..)
        .all(|(function, index)| function.id.0 == index);
    if contiguous {
        return;
    }
    for function in &mut program.functions {
        function.id = remap[&function.id];
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                remap_statement_function_ids(statement, &remap);
            }
            remap_terminator_function_ids(&mut block.terminator, &remap);
        }
    }
    for template in templates {
        for block in &mut template.blocks {
            for statement in &mut block.statements {
                remap_statement_function_ids(statement, &remap);
            }
            remap_terminator_function_ids(&mut block.terminator, &remap);
        }
    }
    for environment in environments {
        environment.function = remap[&environment.function];
    }
}

/// Re-key device metadata rows through an old-to-new function-id map. A row
/// keyed outside the map (a generic template) keeps its key; a kept row's
/// parent follows the map too (a parent outside the map keeps its id).
fn remap_device_function_metadata(
    metadata: &mut MirDeviceFunctionMetadataMap,
    remap: &HashMap<MirFunctionId, MirFunctionId>,
) {
    let rows = std::mem::take(metadata);
    for (key, mut row) in rows {
        remap_function_id(&mut row.identity.parent, remap);
        let mut key = key;
        remap_function_id(&mut key, remap);
        metadata.insert(key, row);
    }
}

fn remap_function_id(id: &mut MirFunctionId, remap: &HashMap<MirFunctionId, MirFunctionId>) {
    if let Some(next) = remap.get(id) {
        *id = *next;
    }
}

fn remap_constant_function_ids(
    constant: &mut MirConstant,
    remap: &HashMap<MirFunctionId, MirFunctionId>,
) {
    if let MirConstant::Function(id) = constant {
        remap_function_id(id, remap);
    }
}

fn remap_callee_function_ids(
    callee: &mut MirCallee,
    remap: &HashMap<MirFunctionId, MirFunctionId>,
) {
    match callee {
        MirCallee::Function(id) => remap_function_id(id, remap),
        MirCallee::Closure(closure) => remap_function_id(&mut closure.function, remap),
        MirCallee::Value(operand) => remap_operand_function_ids(operand, remap),
        MirCallee::Definition { .. } => {}
    }
}

fn remap_operand_function_ids(
    operand: &mut MirOperand,
    remap: &HashMap<MirFunctionId, MirFunctionId>,
) {
    if let MirOperand::Constant(constant) = operand {
        remap_constant_function_ids(constant, remap);
    }
}

fn remap_value_function_ids(value: &mut MirValue, remap: &HashMap<MirFunctionId, MirFunctionId>) {
    match &mut value.kind {
        MirValueKind::Operand(operand) | MirValueKind::Unary { operand, .. } => {
            remap_operand_function_ids(operand, remap);
        }
        MirValueKind::Closure(closure) => remap_function_id(&mut closure.function, remap),
        MirValueKind::Binary { lhs, rhs, .. } => {
            remap_operand_function_ids(lhs, remap);
            remap_operand_function_ids(rhs, remap);
        }
        MirValueKind::Option(op) => remap_option_function_ids(op, remap),
        MirValueKind::TensorExtent { tensor, .. } => remap_operand_function_ids(tensor, remap),
    }
}

fn remap_option_function_ids(op: &mut MirOptionOp, remap: &HashMap<MirFunctionId, MirFunctionId>) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand)
        | MirOptionOp::Unwrap { value: operand, .. } => remap_operand_function_ids(operand, remap),
        MirOptionOp::Coalesce { value, fallback } => {
            remap_operand_function_ids(value, remap);
            remap_operand_function_ids(fallback, remap);
        }
        MirOptionOp::Chain { base, link } => {
            remap_operand_function_ids(base, remap);
            remap_option_chain_link_function_ids(link, remap);
        }
        MirOptionOp::None => {}
    }
}

fn remap_option_chain_link_function_ids(
    link: &mut MirOptionChainLink,
    remap: &HashMap<MirFunctionId, MirFunctionId>,
) {
    match link {
        MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => {}
        MirOptionChainLink::Index(operand) => remap_operand_function_ids(operand, remap),
        MirOptionChainLink::Call { callee, args } => {
            remap_callee_function_ids(callee, remap);
            for arg in args {
                remap_operand_function_ids(arg, remap);
            }
        }
    }
}

fn remap_aggregate_function_ids(
    aggregate: &mut MirAggregate,
    remap: &HashMap<MirFunctionId, MirFunctionId>,
) {
    match &mut aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        remap_operand_function_ids(operand, remap);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                remap_operand_function_ids(&mut item.value, remap);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                remap_operand_function_ids(&mut item.key, remap);
                remap_operand_function_ids(&mut item.value, remap);
            }
        }
    }
}

fn remap_switch_case_function_ids(
    case: &mut MirSwitchCase,
    remap: &HashMap<MirFunctionId, MirFunctionId>,
) {
    remap_constant_function_ids(&mut case.value, remap);
}

fn remap_statement_function_ids(
    statement: &mut MirStatement,
    remap: &HashMap<MirFunctionId, MirFunctionId>,
) {
    match &mut statement.kind {
        MirStatementKind::Assign { value, .. } => remap_value_function_ids(value, remap),
        MirStatementKind::Call { callee, args, .. } => {
            remap_callee_function_ids(callee, remap);
            for arg in args {
                remap_operand_function_ids(arg, remap);
            }
        }
        MirStatementKind::RuntimeCall { call, .. } => {
            for arg in &mut call.args {
                remap_operand_function_ids(arg, remap);
            }
            if let MirIntrinsic::TensorLimes(radix_mir::MirEdgePolicy::Custom {
                remap: callback,
                transform,
                ..
            }) = &mut call.intrinsic
            {
                remap_function_id(callback, remap);
                if let Some(transform) = transform {
                    remap_function_id(transform, remap);
                }
            }
        }
        MirStatementKind::Construct { aggregate, .. } => {
            remap_aggregate_function_ids(aggregate, remap);
        }
    }
}

fn remap_terminator_function_ids(
    terminator: &mut MirTerminator,
    remap: &HashMap<MirFunctionId, MirFunctionId>,
) {
    match &mut terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            remap_operand_function_ids(operand, remap);
        }
        MirTerminatorKind::TryCall { callee, args, .. } => {
            remap_callee_function_ids(callee, remap);
            for arg in args {
                remap_operand_function_ids(arg, remap);
            }
        }
        MirTerminatorKind::Branch { condition, .. } => remap_operand_function_ids(condition, remap),
        MirTerminatorKind::Switch { value, cases, .. } => {
            remap_operand_function_ids(value, remap);
            for case in cases {
                remap_switch_case_function_ids(case, remap);
            }
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

/// Compute the reachable function set of one lowered library program from the
/// current synthetic-def seed, plus the `MirCallee::Definition` sources those
/// functions reference (cross-library edges for the fixpoint).
fn reachable_library_function_set(
    program: &MirProgram,
    templates: &[MirFunction],
    reachable_seed: &HashSet<DefId>,
) -> (HashSet<MirFunctionId>, Vec<DefId>) {
    let mut work: Vec<MirFunctionId> = Vec::new();
    for function in &program.functions {
        if let Some(source) = function.source
            && reachable_seed.contains(&source)
        {
            work.push(function.id);
        }
    }
    let mut reachable = HashSet::new();
    let mut definitions = Vec::new();
    let mut seen_definitions = HashSet::new();
    // A generic template has no row in `program.functions`, so its body is
    // invisible to the function walk below. A reachable template still calls
    // sibling-library functions (`Definition` edges) and local helpers
    // (`Function` edges), and those must stay reachable too.
    for template in templates {
        if template
            .source
            .is_some_and(|source| reachable_seed.contains(&source))
        {
            let scan = scan_mir_function_body_references(template);
            work.extend(scan.functions);
            for definition in scan.definitions {
                if seen_definitions.insert(definition) {
                    definitions.push(definition);
                }
            }
        }
    }
    while let Some(function_id) = work.pop() {
        if !reachable.insert(function_id) {
            continue;
        }
        let scan = scan_mir_function_references(program, function_id);
        work.extend(scan.functions);
        for definition in scan.definitions {
            if seen_definitions.insert(definition) {
                definitions.push(definition);
            }
        }
    }
    (reachable, definitions)
}

/// Function, definition, and closure-environment references made by one MIR
/// function body (S1 U3 reachability edges).
#[derive(Default)]
struct MirReferenceScan {
    functions: Vec<MirFunctionId>,
    definitions: Vec<DefId>,
    environments: Vec<MirClosureEnvironmentId>,
}

fn scan_mir_function_references(
    program: &MirProgram,
    function_id: MirFunctionId,
) -> MirReferenceScan {
    let Some(function) = program
        .functions
        .iter()
        .find(|function| function.id == function_id)
    else {
        return MirReferenceScan::default();
    };
    scan_mir_function_body_references(function)
}

fn scan_mir_function_body_references(function: &MirFunction) -> MirReferenceScan {
    let mut scan = MirReferenceScan::default();
    for block in &function.blocks {
        for statement in &block.statements {
            scan_statement_references(statement, &mut scan);
        }
        scan_terminator_references(&block.terminator, &mut scan);
    }
    scan
}

fn scan_statement_references(statement: &MirStatement, scan: &mut MirReferenceScan) {
    match &statement.kind {
        MirStatementKind::Assign { value, .. } => scan_value_references(value, scan),
        MirStatementKind::Call { callee, args, .. } => {
            scan_callee_references(callee, scan);
            for arg in args {
                scan_operand_references(arg, scan);
            }
        }
        MirStatementKind::RuntimeCall { call, .. } => {
            for arg in &call.args {
                scan_operand_references(arg, scan);
            }
            if let MirIntrinsic::TensorLimes(radix_mir::MirEdgePolicy::Custom {
                remap,
                transform,
                ..
            }) = &call.intrinsic
            {
                scan.functions.push(*remap);
                scan.functions.extend(*transform);
            }
        }
        MirStatementKind::Construct { aggregate, .. } => {
            scan_aggregate_references(aggregate, scan);
        }
    }
}

fn scan_terminator_references(terminator: &MirTerminator, scan: &mut MirReferenceScan) {
    match &terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            scan_operand_references(operand, scan);
        }
        MirTerminatorKind::TryCall { callee, args, .. } => {
            scan_callee_references(callee, scan);
            for arg in args {
                scan_operand_references(arg, scan);
            }
        }
        MirTerminatorKind::Branch { condition, .. } => {
            scan_operand_references(condition, scan);
        }
        MirTerminatorKind::Switch { value, cases, .. } => {
            scan_operand_references(value, scan);
            for case in cases {
                scan_constant_references(&case.value, scan);
            }
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

fn scan_value_references(value: &MirValue, scan: &mut MirReferenceScan) {
    match &value.kind {
        MirValueKind::Operand(operand) | MirValueKind::Unary { operand, .. } => {
            scan_operand_references(operand, scan);
        }
        MirValueKind::Closure(closure) => {
            scan.functions.push(closure.function);
            scan.environments.push(closure.environment_id);
            scan_operand_references(&closure.environment, scan);
        }
        MirValueKind::Binary { lhs, rhs, .. } => {
            scan_operand_references(lhs, scan);
            scan_operand_references(rhs, scan);
        }
        MirValueKind::Option(op) => scan_option_references(op, scan),
        MirValueKind::TensorExtent { tensor, .. } => scan_operand_references(tensor, scan),
    }
}

fn scan_operand_references(operand: &MirOperand, scan: &mut MirReferenceScan) {
    match operand {
        MirOperand::Place(place) => scan_place_references(place, scan),
        MirOperand::Constant(constant) => scan_constant_references(constant, scan),
        MirOperand::Temp(_) | MirOperand::Value(_) => {}
    }
}

fn scan_place_references(place: &MirPlace, scan: &mut MirReferenceScan) {
    for projection in &place.projections {
        match projection {
            MirProjection::ClosureCapture { environment, .. } => {
                scan.environments.push(*environment);
            }
            MirProjection::Index(operand) | MirProjection::ProvenIndex(operand) => {
                scan_operand_references(operand, scan);
            }
            MirProjection::Field(_)
            | MirProjection::VariantField { .. }
            | MirProjection::VectorLane(_)
            | MirProjection::MatrixCell { .. }
            | MirProjection::SharedCell => {}
        }
    }
}

fn scan_constant_references(constant: &MirConstant, scan: &mut MirReferenceScan) {
    if let MirConstant::Function(function_id) = constant {
        scan.functions.push(*function_id);
    }
}

fn scan_callee_references(callee: &MirCallee, scan: &mut MirReferenceScan) {
    match callee {
        MirCallee::Function(function_id) => scan.functions.push(*function_id),
        MirCallee::Closure(closure) => {
            scan.functions.push(closure.function);
            scan.environments.push(closure.environment_id);
            scan_operand_references(&closure.environment, scan);
        }
        MirCallee::Value(operand) => scan_operand_references(operand, scan),
        MirCallee::Definition { source, .. } => scan.definitions.push(*source),
    }
}

fn scan_aggregate_references(aggregate: &MirAggregate, scan: &mut MirReferenceScan) {
    if let MirAggregateKind::ClosureEnvironment(environment_id) = &aggregate.kind {
        scan.environments.push(*environment_id);
    }
    match &aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        scan_operand_references(operand, scan);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                scan_operand_references(&item.value, scan);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                scan_operand_references(&item.key, scan);
                scan_operand_references(&item.value, scan);
            }
        }
    }
}

fn scan_option_references(op: &MirOptionOp, scan: &mut MirReferenceScan) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand)
        | MirOptionOp::Unwrap { value: operand, .. } => scan_operand_references(operand, scan),
        MirOptionOp::Coalesce { value, fallback } => {
            scan_operand_references(value, scan);
            scan_operand_references(fallback, scan);
        }
        MirOptionOp::Chain { base, link } => {
            scan_operand_references(base, scan);
            scan_option_chain_references(link, scan);
        }
        MirOptionOp::None => {}
    }
}

fn scan_option_chain_references(link: &MirOptionChainLink, scan: &mut MirReferenceScan) {
    match link {
        MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => {}
        MirOptionChainLink::Index(operand) => scan_operand_references(operand, scan),
        MirOptionChainLink::Call { callee, args } => {
            scan_callee_references(callee, scan);
            for arg in args {
                scan_operand_references(arg, scan);
            }
        }
    }
}

#[cfg(test)]
#[path = "prune_device_metadata_test.rs"]
mod prune_device_metadata_tests;

#[cfg(test)]
#[path = "metadata_merge_test.rs"]
mod metadata_merge_tests;

#[cfg(test)]
#[path = "display_tokens_clone_test.rs"]
mod display_tokens_clone_tests;
