//! Whole-unit lookup tables for HIR-to-MIR lowering.
//!
//! MIR builders lower one function body at a time, but body lowering frequently
//! needs facts that are declared elsewhere in the analyzed unit: callable
//! signatures for validation, alternate-exit types, enum variant ownership,
//! variant payload field names, struct fields for aggregate construction, and
//! provider-import identities. This module gathers those facts once from typed
//! HIR and packages them into the read-mostly context used by function lowering.
//!
//! DESIGN
//! ======
//! The maps are not a second semantic analysis pass. They preserve information
//! already established by collect, resolve, lower, and typecheck, then reshape it
//! into MIR-oriented lookup tables. Missing type information still fails later at
//! the lowering site; these maps should not synthesize types or invent fallback
//! signatures.
//!
//! VALIDATION HANDOFF
//! ==================
//! [`LoweringContextMaps`] also owns the [`MirValidationContext`] built from HIR
//! declarations. Validation receives this context after lowering so it can check
//! function calls, struct fields, and variant payloads against semantic source
//! truth rather than trusting the MIR producer.

use super::package_instantiate::ImportedDeviceRoute;
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering context
use super::*;
use radix_hir::visit::HirVisitor;
use radix_hir::{
    HirAnnotation, HirAnnotationValue, HirConst, HirEnum, HirExpression, HirExpressionKind, HirFunction, HirImport,
    HirItemKind, HirMethod, HirStruct, LibraryItem, LibraryItemKind, LibraryProvider,
};
use radix_mir::MirExternalFunctionIdentity;
use radix_mir::MirProviderKind;
use radix_mir::{MirFunctionSignature, MirLayoutTable, MirParamSignature};
use radix_semantic::SymbolKind;
use radix_semantic::analyzed_application::AnalyzedModuleApplicationView;
use radix_semantic::file_interface::InterfaceConstValue;
use radix_semantic::kernel::{
    is_kernel_glob_import_path, is_kernel_import_path, resolve_kernel_module, resolve_kernel_module_name,
};
use std::sync::Arc;

/// Callable parameter metadata preserved for MIR call lowering.
#[derive(Debug, Clone)]
pub(super) struct CallableParamMeta<'a> {
    /// Declared parameter type (`None` for the synthetic method receiver slot,
    /// which is never omitted at a call site).
    pub ty: Option<TypeId>,
    pub optional: bool,
    pub default: Option<&'a HirExpression>,
}

/// Frozen Arc-wrapped maps for cheap per-function sharing.
///
/// After `collect` populates the lowering maps, `freeze` wraps each map in an
/// `Arc` so that `builder_context` (called once per function body) clones only
/// the cheap atomic reference instead of the entire map.
struct FrozenMaps<'a> {
    /// A `⇥` kernel exists in the unit: handled blocks emit device markers.
    fault_delivery: bool,
    function_errors: Arc<FxHashMap<DefId, MirType>>,
    function_async: Arc<FxHashMap<DefId, bool>>,
    function_generator: Arc<FxHashMap<DefId, bool>>,
    variant_parents: Arc<FxHashMap<DefId, DefId>>,
    enum_variants: Arc<FxHashMap<DefId, Vec<DefId>>>,
    enum_variant_by_name: Arc<FxHashMap<(DefId, Symbol), DefId>>,
    enum_type_params: Arc<FxHashMap<DefId, Vec<Symbol>>>,
    variant_fields: Arc<FxHashMap<DefId, Vec<Symbol>>>,
    variant_field_tys: Arc<FxHashMap<DefId, FxHashMap<Symbol, MirType>>>,
    conversion_rows: Arc<FxHashMap<ConversionPair, DefId>>,
    top_level_consts: Arc<FxHashMap<DefId, &'a HirConst>>,
    /// Const-member (`fixum`) genus fields keyed by `(struct, field)`; a read through
    /// the type name inlines the field's folded initializer.
    static_fields: Arc<FxHashMap<(DefId, Symbol), &'a HirField>>,
    /// Const-member (`fixum`) fields of imported genus exports by `(struct, field)`:
    /// the interface's folded initializer, inlined by a `module.Tab.Z` read.
    imported_static_fields: Arc<FxHashMap<(DefId, Symbol), HirExpression>>,
    /// Folded scalar values of imported module constants by `(module, member)`:
    /// a single-file `module.NAME` read inlines the interface's value.
    imported_constants: Arc<FxHashMap<(DefId, Symbol), InterfaceConstValue>>,
    /// Producer-side defaults for imported class fields, keyed by importer-local type.
    imported_struct_defaults: Arc<FxHashMap<DefId, Vec<(Symbol, HirExpression)>>>,
    provider_imports: Arc<FxHashMap<DefId, ProviderImport>>,
    imported_type_members: Arc<FxHashMap<(DefId, Symbol), MirType>>,
    method_targets: Arc<FxHashMap<(DefId, Symbol), MethodTarget>>,
    creo_hooks: Arc<FxHashMap<DefId, DefId>>,
    interface_implementors: Arc<FxHashMap<DefId, Vec<(DefId, TypeId)>>>,
    imported_method_providers: Arc<FxHashMap<(DefId, Symbol), ImportedMethodProvider>>,
    gpu_builtins: Arc<FxHashMap<DefId, MirKernelBuiltin>>,
}

/// Whole-unit maps shared by item and function lowering.
///
/// The lowering-only maps feed [`FunctionBuilderContext`]. The validation map is
/// carried beside them because it is built from the same HIR walk but consumed
/// only after a complete [`MirProgram`] exists.
pub struct LoweringContextMaps<'a> {
    interner: &'a Interner,
    fault_delivery: bool,
    function_errors: FxHashMap<DefId, MirType>,
    function_async: FxHashMap<DefId, bool>,
    function_generator: FxHashMap<DefId, bool>,
    variant_parents: FxHashMap<DefId, DefId>,
    /// Enum declaration → ordered variant defs (local + imported file-interface
    /// enums). Zero-init of a no-initializer enum-typed local picks the first
    /// variant; `variant_parents` only knows the reverse direction.
    enum_variants: FxHashMap<DefId, Vec<DefId>>,
    /// (enum declaration, variant name) → variant def. Compiler-synthesized
    /// arms (the `capta` boundary's `thrown { error }` wrap) resolve the
    /// builtin genus's variants by name, exactly as authored patterns do.
    enum_variant_by_name: FxHashMap<(DefId, Symbol), DefId>,
    enum_type_params: FxHashMap<DefId, Vec<Symbol>>,
    variant_fields: FxHashMap<DefId, Vec<Symbol>>,
    variant_field_tys: FxHashMap<DefId, FxHashMap<Symbol, MirType>>,
    /// Coherence-registry conversion rows keyed by ordered pair
    /// `(source → destination)` (CTR-06). Populated from the analyzed
    /// unit's resolver — the one semantic registry — not from HIR shape.
    conversion_rows: FxHashMap<ConversionPair, DefId>,
    sermo_def: Option<DefId>,
    meus_def: Option<DefId>,
    tuus_def: Option<DefId>,
    scrinium_def: Option<DefId>,
    top_level_consts: FxHashMap<DefId, &'a HirConst>,
    static_fields: FxHashMap<(DefId, Symbol), &'a HirField>,
    imported_static_fields: FxHashMap<(DefId, Symbol), HirExpression>,
    imported_constants: FxHashMap<(DefId, Symbol), InterfaceConstValue>,
    provider_imports: FxHashMap<DefId, ProviderImport>,
    imported_type_members: FxHashMap<(DefId, Symbol), MirType>,
    imported_method_providers: FxHashMap<(DefId, Symbol), ImportedMethodProvider>,
    /// Producer-side defaults for imported class fields, keyed by importer-local type.
    imported_struct_defaults: FxHashMap<DefId, Vec<(Symbol, HirExpression)>>,
    /// Package-resolved file-interface call targets keyed by import-item
    /// `DefId`: `verb → synthesized external-definition id`. Populated from
    /// `unit.package_import_identities` when package-aware callers supply the
    /// canonical identity facts (D-PA1); empty otherwise.
    package_file_interface_exports: FxHashMap<DefId, FxHashMap<Symbol, DefId>>,
    /// Resolved imported-device routes keyed by import-item `DefId` (DFV2-3):
    /// the v2 device role, launch visibility, provider-route identity, and
    /// explicit body channel carried through the consumer link. The concrete
    /// composition pass consumes these; a `Missing` body channel is carried,
    /// never silently admitted.
    imported_device_routes: FxHashMap<DefId, Vec<ImportedDeviceRoute>>,
    /// Current package product (package-aware compiles only).
    package_product: Option<String>,
    /// This file's canonical module segments (package-aware compiles only).
    package_module_segments: Vec<String>,
    method_targets: FxHashMap<(DefId, Symbol), MethodTarget>,
    /// Genus → its `creo` post-construction hook method. Typed construction
    /// calls the hook once the fields are initialized.
    creo_hooks: FxHashMap<DefId, DefId>,
    /// Contract → the local `genus` types whose `implet` clause names it, in
    /// declaration order. A method call through a contract-typed value lowers
    /// to a type-test chain over exactly these members.
    interface_implementors: FxHashMap<DefId, Vec<(DefId, TypeId)>>,
    gpu_builtins: Vec<(DefId, radix_mir::MirKernelBuiltin)>,
    /// Frozen Arc-wrapped maps populated after the HIR visitor phase.
    frozen: FrozenMaps<'a>,
    pub(super) validation: MirValidationContext<'a>,
}

impl<'a> LoweringContextMaps<'a> {
    /// Transfer the collected imported-device route map (KPC-WIRE `done_when`
    /// (c)): the consumer link's provider routes, handed to the lowered-unit
    /// carrier exactly once so package linking can join them to rewritten
    /// synthetic template sources. The map is left empty — a second read
    /// yields nothing, so a route is never admitted twice.
    pub(super) fn take_imported_device_routes(&mut self) -> FxHashMap<DefId, Vec<ImportedDeviceRoute>> {
        std::mem::take(&mut self.imported_device_routes)
    }

    /// Collect MIR lowering and validation maps from an analyzed unit.
    ///
    /// This walk intentionally follows HIR item structure instead of MIR output:
    /// declarations that never lower into executable MIR, such as structs,
    /// enums, and imports, still define facts required by function-body lowering
    /// and validation.
    pub(super) fn collect(unit: AnalyzedModuleApplicationView<'a>) -> Self {
        let mut maps = Self {
            interner: unit.interner,
            fault_delivery: false,
            function_errors: FxHashMap::default(),
            function_async: FxHashMap::default(),
            function_generator: FxHashMap::default(),
            variant_parents: FxHashMap::default(),
            enum_variants: FxHashMap::default(),
            enum_variant_by_name: FxHashMap::default(),
            enum_type_params: FxHashMap::default(),
            variant_fields: FxHashMap::default(),
            variant_field_tys: FxHashMap::default(),
            conversion_rows: FxHashMap::default(),
            sermo_def: None,
            meus_def: None,
            tuus_def: None,
            scrinium_def: None,
            top_level_consts: FxHashMap::default(),
            static_fields: FxHashMap::default(),
            imported_static_fields: FxHashMap::default(),
            imported_constants: FxHashMap::default(),
            provider_imports: FxHashMap::default(),
            imported_type_members: FxHashMap::default(),
            imported_method_providers: FxHashMap::default(),
            imported_struct_defaults: FxHashMap::default(),
            package_file_interface_exports: FxHashMap::default(),
            imported_device_routes: FxHashMap::default(),
            package_product: None,
            package_module_segments: Vec::new(),
            method_targets: FxHashMap::default(),
            creo_hooks: FxHashMap::default(),
            interface_implementors: FxHashMap::default(),
            gpu_builtins: unit.gpu_builtins.to_vec(),
            frozen: FrozenMaps {
                fault_delivery: false,
                function_errors: Arc::new(FxHashMap::default()),
                function_async: Arc::new(FxHashMap::default()),
                function_generator: Arc::new(FxHashMap::default()),
                variant_parents: Arc::new(FxHashMap::default()),
                enum_variants: Arc::new(FxHashMap::default()),
                enum_variant_by_name: Arc::new(FxHashMap::default()),
                enum_type_params: Arc::new(FxHashMap::default()),
                variant_fields: Arc::new(FxHashMap::default()),
                variant_field_tys: Arc::new(FxHashMap::default()),
                conversion_rows: Arc::new(FxHashMap::default()),
                top_level_consts: Arc::new(FxHashMap::default()),
                static_fields: Arc::new(FxHashMap::default()),
                imported_static_fields: Arc::new(FxHashMap::default()),
                imported_constants: Arc::new(FxHashMap::default()),
                imported_struct_defaults: Arc::new(FxHashMap::default()),
                provider_imports: Arc::new(FxHashMap::default()),
                imported_type_members: Arc::new(FxHashMap::default()),
                method_targets: Arc::new(FxHashMap::default()),
                creo_hooks: Arc::new(FxHashMap::default()),
                interface_implementors: Arc::new(FxHashMap::default()),
                imported_method_providers: Arc::new(FxHashMap::default()),
                gpu_builtins: Arc::new(FxHashMap::default()),
            },
            validation: MirValidationContext {
                // Interim borrow of the analyzed unit's arena; the lowering
                // entry point replaces it with the lowering-owned extension
                // table (via `with_owned_types`) before the context escapes.
                // Walk-time reads here (primitives, declared structs) resolve
                // identically in both tables.
                types: radix_mir::MirTypeTable::Borrowed(unit.types),
                // Placeholder: body lowering never reads `.layouts`; the real
                // table is built once at the lowering entry point and moved
                // into the escaping context via `validation_context`.
                layouts: MirLayoutTable::default(),
                interner: radix_mir::MirInterner::Borrowed(unit.interner),
                functions: FxHashMap::default(),
                grid_reduce_helper_indexes: FxHashMap::default(),
                method_defs: FxHashSet::default(),
                external_function_identities: FxHashMap::default(),
                package_product: None,
                struct_fields: FxHashMap::default(),
                struct_field_order: FxHashMap::default(),
                variant_field_order: FxHashMap::default(),
                display_names: FxHashMap::default(),
                display_tokens: radix_mir::MirDisplayTokens::default(),
                struct_type_params: FxHashMap::default(),
                struct_contracts: FxHashMap::default(),
                enum_type_params: FxHashMap::default(),
                optional_struct_fields: FxHashMap::default(),
                nullable_struct_fields: FxHashMap::default(),
                opaque_structs: FxHashSet::default(),
                json_struct_field_keys: FxHashMap::default(),
                variant_parents: FxHashMap::default(),
                enum_variants: FxHashMap::default(),
                variant_positions: FxHashMap::default(),
                variant_fields: FxHashMap::default(),
                scalar_value_domains: FxHashMap::default(),
                closure_environments: FxHashMap::default(),
                ad_routes: FxHashMap::default(),
            },
        };
        maps.record_package_import_identities(unit);
        maps.visit_program(unit.hir);
        maps.record_conversion_rows(unit);
        for item in unit.libraries.items.values() {
            maps.record_library_item_identity(item);
        }
        maps.record_imported_struct_items(unit);
        maps.record_imported_type_members(unit);
        maps.record_imported_constants(unit);
        maps.record_imported_enum_items(unit);
        for item in &unit.hir.items {
            match &item.kind {
                HirItemKind::Constant(konst) => {
                    maps.top_level_consts.insert(item.def_id, konst);
                }
                HirItemKind::Struct(strukt) => {
                    for field in strukt.fields.iter().filter(|field| field.is_static()) {
                        maps.static_fields.insert((item.def_id, field.name), field);
                    }
                }
                _ => {}
            }
        }
        // Freeze: wrap owned maps in Arc for cheap per-function sharing.
        // After freeze, builder_context reads from self.frozen, not the
        // (now-empty) owned fields.
        maps.frozen = FrozenMaps {
            fault_delivery: maps.fault_delivery,
            function_errors: Arc::new(core::mem::take(&mut maps.function_errors)),
            function_async: Arc::new(core::mem::take(&mut maps.function_async)),
            function_generator: Arc::new(core::mem::take(&mut maps.function_generator)),
            variant_parents: Arc::new(core::mem::take(&mut maps.variant_parents)),
            enum_variants: Arc::new(core::mem::take(&mut maps.enum_variants)),
            enum_variant_by_name: Arc::new(core::mem::take(&mut maps.enum_variant_by_name)),
            enum_type_params: Arc::new(core::mem::take(&mut maps.enum_type_params)),
            variant_fields: Arc::new(core::mem::take(&mut maps.variant_fields)),
            variant_field_tys: Arc::new(core::mem::take(&mut maps.variant_field_tys)),
            conversion_rows: Arc::new(core::mem::take(&mut maps.conversion_rows)),
            top_level_consts: Arc::new(core::mem::take(&mut maps.top_level_consts)),
            static_fields: Arc::new(core::mem::take(&mut maps.static_fields)),
            imported_static_fields: Arc::new(core::mem::take(&mut maps.imported_static_fields)),
            imported_constants: Arc::new(core::mem::take(&mut maps.imported_constants)),
            imported_struct_defaults: Arc::new(core::mem::take(&mut maps.imported_struct_defaults)),
            provider_imports: Arc::new(core::mem::take(&mut maps.provider_imports)),
            imported_type_members: Arc::new(core::mem::take(&mut maps.imported_type_members)),
            method_targets: Arc::new(core::mem::take(&mut maps.method_targets)),
            creo_hooks: Arc::new(core::mem::take(&mut maps.creo_hooks)),
            interface_implementors: Arc::new(core::mem::take(&mut maps.interface_implementors)),
            imported_method_providers: Arc::new(core::mem::take(&mut maps.imported_method_providers)),
            gpu_builtins: Arc::new(
                core::mem::take(&mut maps.gpu_builtins)
                    .into_iter()
                    .collect::<FxHashMap<DefId, MirKernelBuiltin>>(),
            ),
        };
        maps
    }

    /// Build the immutable context used by one [`FunctionBuilder`].
    ///
    /// Function builders own their CFG and local state, but they should all see
    /// the same whole-unit declaration facts. Cloning the small maps here keeps
    /// body lowering isolated from cross-function mutation.
    pub(super) fn builder_context(
        &self,
        interner: &'a Interner,
        empty_textus: radix_lexer::Symbol,
        diagnostics: DiagnosticSymbols,
        structs: Arc<FxHashMap<DefId, Vec<&'a HirField>>>,
        struct_field_types: Arc<FxHashMap<DefId, FxHashMap<radix_lexer::Symbol, MirType>>>,
        function_params: Arc<FxHashMap<DefId, Vec<CallableParamMeta<'a>>>>,
        imported_function_params: Arc<FxHashMap<DefId, Vec<super::ImportedCallableParam>>>,
        cli_parsed_record_fields_by_type: Arc<FxHashMap<radix_types::TypeId, Vec<MirRuntimeRecordField>>>,
        cli_record_fields_by_type: Arc<FxHashMap<radix_types::TypeId, Vec<MirRuntimeRecordField>>>,
        cli_adapter_active: bool,
    ) -> FunctionBuilderContext<'a> {
        FunctionBuilderContext {
            interner: Some(interner),
            empty_textus,
            diagnostic_format_templates: diagnostics.format_templates,
            diagnostic_separator: diagnostics.separator,
            diagnostic_nihil: diagnostics.nihil,
            diagnostic_lockstep_length_mismatch: diagnostics.lockstep_length_mismatch,
            diagnostic_ordo_no_member: diagnostics.ordo_no_member,
            diagnostic_potentia_negative_exponent: diagnostics.potentia_negative_exponent,
            diagnostic_range_step_not_positive: diagnostics.range_step_not_positive,
            diagnostic_map_changed_size: diagnostics.map_changed_size,
            diagnostic_ponde_invalid_index: diagnostics.ponde_invalid_index,
            diagnostic_accipe_invalid_index: diagnostics.accipe_invalid_index,
            diagnostic_at_out_of_range: diagnostics.at_out_of_range,
            diagnostic_slice_out_of_range: diagnostics.slice_out_of_range,
            diagnostic_take_out_of_range: diagnostics.take_out_of_range,
            diagnostic_ranged_push_past_maximum: diagnostics.ranged_push_past_maximum,
            diagnostic_ranged_remove_below_minimum: diagnostics.ranged_remove_below_minimum,
            fault_delivery: self.frozen.fault_delivery,
            function_errors: Arc::clone(&self.frozen.function_errors),
            function_async: Arc::clone(&self.frozen.function_async),
            function_generator: Arc::clone(&self.frozen.function_generator),
            function_params,
            imported_function_params,
            tensor_edge_policies: Arc::new(radix_hir::TensorEdgePolicyTable::default()),
            pending_tensor_limes: Arc::new(std::sync::Mutex::new(Vec::new())),
            shape_calls: Arc::new(FxHashMap::default()),
            shape_functions: Arc::new(FxHashMap::default()),
            call_size_bindings: Arc::new(FxHashMap::default()),
            shape_stores: Arc::new(FxHashMap::default()),
            proven_indexes: Arc::new(FxHashSet::default()),
            function_size_bindings: Arc::new(FxHashMap::default()),
            structs,
            struct_field_types,
            imported_struct_defaults: Arc::clone(&self.frozen.imported_struct_defaults),
            variant_parents: Arc::clone(&self.frozen.variant_parents),
            enum_variants: Arc::clone(&self.frozen.enum_variants),
            enum_variant_by_name: Arc::clone(&self.frozen.enum_variant_by_name),
            enum_type_params: Arc::clone(&self.frozen.enum_type_params),
            variant_fields: Arc::clone(&self.frozen.variant_fields),
            variant_field_tys: Arc::clone(&self.frozen.variant_field_tys),
            conversion_rows: Arc::clone(&self.frozen.conversion_rows),
            sermo_def: self.sermo_def,
            meus_def: self.meus_def,
            tuus_def: self.tuus_def,
            scrinium_def: self.scrinium_def,
            top_level_consts: Arc::clone(&self.frozen.top_level_consts),
            static_fields: Arc::clone(&self.frozen.static_fields),
            imported_static_fields: Arc::clone(&self.frozen.imported_static_fields),
            imported_constants: Arc::clone(&self.frozen.imported_constants),
            provider_imports: Arc::clone(&self.frozen.provider_imports),
            imported_type_members: Arc::clone(&self.frozen.imported_type_members),
            method_targets: Arc::clone(&self.frozen.method_targets),
            creo_hooks: Arc::clone(&self.frozen.creo_hooks),
            interface_implementors: Arc::clone(&self.frozen.interface_implementors),
            imported_method_providers: Arc::clone(&self.frozen.imported_method_providers),
            gpu_builtins: Arc::clone(&self.frozen.gpu_builtins),
            cli_parsed_record_fields_by_type,
            cli_record_fields_by_type,
            cli_adapter_active,
            kernel_lowering: KernelLowering::Device,
        }
    }

    fn record_function_item(&mut self, def_id: DefId, function: &HirFunction) {
        self.function_async.insert(def_id, function.is_async);
        self.function_generator
            .insert(def_id, function.is_generator);
        if let Some(err_ty) = function.err_ty {
            self.function_errors
                .insert(def_id, MirType::semantic(err_ty));
            self.fault_delivery |= function.is_kernel;
        }
        if let Some(return_ty) = function.ret_ty {
            let return_ty = if function.is_async && function.is_generator {
                let alternate = function
                    .err_ty
                    .unwrap_or_else(|| self.validation.types.primitive(Primitive::Numquam));
                self.validation
                    .types
                    .find_equal(&Type::AsyncCursor(return_ty, alternate))
                    .unwrap_or(return_ty)
            } else {
                return_ty
            };
            self.validation.functions.insert(
                def_id,
                function_signature(
                    function
                        .params
                        .iter()
                        .map(|param| HirParamSignatureProxy { ty: param.ty, optional: param.optional }),
                    return_ty,
                    function.err_ty,
                ),
            );
        }
        // D-PA1: under package identity context, this file's own top-level
        // functions carry the canonical (product, module_segments, name)
        // identity so the sibling unit's emitted module defines each function
        // under the external symbol its consumers call (`__faber_external_…`).
        if let Some(product) = &self.package_product
            && !self.package_module_segments.is_empty()
        {
            self.validation.external_function_identities.insert(
                def_id,
                MirExternalFunctionIdentity::new(
                    product.clone(),
                    self.package_module_segments.clone(),
                    self.interner.resolve(function.name),
                ),
            );
        }
    }

    /// Tier-1 `@ ad` handler: route → handler `DefId` for the MIR runner's
    /// static table (A4/U9). Legality was proven by `passes::ad_routes`.
    fn record_ad_route(&mut self, item: &HirItem) {
        if let Some(route) = item
            .annotations
            .iter()
            .find_map(|annotation| annotation.ad_route(self.interner))
        {
            self.validation
                .ad_routes
                .insert(self.interner.resolve(route).to_owned(), item.def_id);
        }
    }

    fn record_struct_item(&mut self, def_id: DefId, annotations: &[radix_hir::HirAnnotation], strukt: &HirStruct) {
        self.validation
            .struct_type_params
            .insert(def_id, strukt.type_params.iter().map(|param| param.name).collect());
        match self.interner.resolve(strukt.name) {
            "sermo" => {
                self.sermo_def = Some(def_id);
                self.validation.opaque_structs.insert(def_id);
            }
            "meus" => {
                self.meus_def = Some(def_id);
                self.validation.opaque_structs.insert(def_id);
            }
            "tuus" => {
                self.tuus_def = Some(def_id);
                self.validation.opaque_structs.insert(def_id);
            }
            "scrinium" => {
                self.scrinium_def = Some(def_id);
            }
            _ => {}
        }
        let receiver_ty = self.validation.types.find_struct(def_id);
        let mut fields = FxHashMap::default();
        let mut optional_fields = FxHashSet::default();
        let mut nullable_fields = FxHashSet::default();
        let mut json_field_keys = FxHashMap::default();
        let is_json_struct = annotations_contain_family(annotations, self.interner, "json");
        if is_json_struct {
            for field in &strukt.fields {
                if !field.is_static() {
                    let key = json_annotation_nomen(&field.annotations, self.interner).unwrap_or(field.name);
                    json_field_keys.insert(field.name, self.interner.resolve(key).to_owned());
                }
            }
        }
        for field in &strukt.fields {
            if !field.is_static() {
                fields.insert(field.name, MirType::semantic(field.ty));
                if field.sponte || field.init.is_some() {
                    optional_fields.insert(field.name);
                }
                if field.sponte {
                    nullable_fields.insert(field.name);
                }
            }
        }
        self.validation
            .display_names
            .insert(def_id, self.interner.resolve(strukt.name).to_owned());
        self.validation.struct_field_order.insert(
            def_id,
            strukt
                .fields
                .iter()
                .filter(|field| !field.is_static())
                .map(|field| field.name)
                .collect(),
        );
        self.validation.struct_fields.insert(def_id, fields);
        self.validation
            .optional_struct_fields
            .insert(def_id, optional_fields);
        self.validation
            .nullable_struct_fields
            .insert(def_id, nullable_fields);
        if is_json_struct {
            self.validation
                .json_struct_field_keys
                .insert(def_id, json_field_keys);
        }

        if let Some(receiver_ty) = receiver_ty {
            for contract in &strukt.implements {
                self.validation
                    .struct_contracts
                    .entry(def_id)
                    .or_default()
                    .insert(contract.contract);
                self.interface_implementors
                    .entry(contract.contract)
                    .or_default()
                    .push((def_id, receiver_ty));
            }
            for method in &strukt.methods {
                self.record_method_item(def_id, receiver_ty, method);
            }
        }
    }

    fn record_imported_enum_items(&mut self, unit: AnalyzedModuleApplicationView<'_>) {
        let nominal_names = unit.resolver.imported_file_nominal_names();
        for (def, name) in &nominal_names {
            self.validation
                .display_names
                .insert(*def, self.interner.resolve(*name).to_owned());
        }
        for (enum_def, _, variants) in unit.resolver.imported_file_enums() {
            let enum_name = self
                .validation
                .display_names
                .get(&enum_def)
                .cloned()
                .unwrap_or_default();
            let variant_defs = variants
                .iter()
                .map(|variant| variant.def_id)
                .collect::<Vec<_>>();
            self.enum_variants.insert(enum_def, variant_defs.clone());
            self.validation.enum_variants.insert(enum_def, variant_defs);
            for variant in &variants {
                self.enum_variant_by_name
                    .insert((enum_def, variant.name), variant.def_id);
            }
            let commune = unit
                .resolver
                .commune_fields(enum_def)
                .iter()
                .map(|field| (field.name, field.ty))
                .collect::<Vec<_>>();
            for variant in &variants {
                self.variant_parents.insert(variant.def_id, enum_def);
                self.validation
                    .variant_parents
                    .insert(variant.def_id, enum_def);
                self.validation
                    .display_names
                    .insert(variant.def_id, format!("{enum_name}.{}", self.interner.resolve(variant.name)));
                let field_names = commune
                    .iter()
                    .map(|(name, _)| *name)
                    .chain(variant.fields.iter().map(|field| field.name))
                    .collect::<Vec<_>>();
                self.validation
                    .variant_field_order
                    .insert(variant.def_id, field_names.clone());
                self.variant_fields.insert(variant.def_id, field_names);
                let mut fields = FxHashMap::default();
                for (name, ty) in &commune {
                    fields.insert(*name, MirType::semantic(*ty));
                }
                for field in &variant.fields {
                    fields.insert(field.name, MirType::semantic(field.ty));
                }
                self.variant_field_tys
                    .insert(variant.def_id, fields.clone());
                self.validation
                    .variant_fields
                    .insert(variant.def_id, fields);
            }
        }
    }

    fn record_imported_struct_items(&mut self, unit: AnalyzedModuleApplicationView<'_>) {
        for (def_id, imported_fields, _, type_params) in unit.resolver.imported_file_structs() {
            let mut fields = FxHashMap::default();
            let mut optional_fields = FxHashSet::default();
            let mut nullable_fields = FxHashSet::default();
            let mut defaults = Vec::new();
            self.validation
                .struct_field_order
                .insert(def_id, imported_fields.iter().map(|field| field.name).collect());
            for field in imported_fields {
                fields.insert(field.name, MirType::semantic(field.ty));
                if let Some(init) = field.init {
                    if field.modifier == Some(radix_syntax::FieldModifier::Const) {
                        self.imported_static_fields
                            .insert((def_id, field.name), init.clone());
                    }
                    defaults.push((field.name, init));
                }
                if field.optional || !field.required {
                    optional_fields.insert(field.name);
                }
                if field.optional {
                    nullable_fields.insert(field.name);
                }
            }
            self.validation.struct_fields.insert(def_id, fields);
            // Imported generic structs must register their declaration params:
            // field projection through an `Applied` nominal instantiates
            // against these bindings (the own-item path in `record_struct_item`
            // only covers structs declared in this unit).
            self.validation
                .struct_type_params
                .insert(def_id, type_params);
            self.validation
                .optional_struct_fields
                .insert(def_id, optional_fields);
            self.validation
                .nullable_struct_fields
                .insert(def_id, nullable_fields);
            if !defaults.is_empty() {
                self.imported_struct_defaults.insert(def_id, defaults);
            }
        }

        self.record_imported_struct_contracts(unit);

        // A direct single-file MIR route has no sibling body to link for an
        // imported nominal method. Preserve the typed method surface as a
        // provider call instead of sending it to the generic unsupported-method
        // diagnostic. Package-aware callers rewrite these calls to linked
        // definitions before this fallback is reached.
        for item in &unit.hir.items {
            let HirItemKind::Import(import) = &item.kind else {
                continue;
            };
            for import_item in &import.items {
                let binding = import_item.alias.unwrap_or(import_item.name);
                let Some(provider) = self.provider_imports.get(&import_item.def_id).cloned() else {
                    continue;
                };
                for (def_id, _, methods, _) in unit.resolver.imported_file_structs_for_namespace(binding) {
                    for method in methods {
                        let target = ImportedMethodProvider { provider: provider.clone(), canonical_name: method.name };
                        self.imported_method_providers
                            .insert((def_id, method.name), target.clone());
                        self.imported_method_providers
                            .insert((def_id, method.surface_name), target);
                    }
                }
            }
        }
    }

    /// Register the declared conformance of imported structs the way
    /// `record_struct_item` does for the unit's own: the validator admits an
    /// imported struct into a slot typed by a contract its declaration names.
    ///
    /// A method call through the contract dispatches over its implementors, so
    /// an implementor joins that list only when its method targets resolve. A
    /// struct imported from a sibling source module under package identity
    /// context qualifies: each of its methods is registered by its
    /// consumer-local identity, which the package link rewrites to the linked
    /// definition. Every other imported implementor stays out, so a contract
    /// call keeps dispatching over the implementors it can actually reach.
    fn record_imported_struct_contracts(&mut self, unit: AnalyzedModuleApplicationView<'_>) {
        let mut contracts = unit.resolver.imported_struct_contracts();
        if contracts.is_empty() {
            return;
        }
        contracts.sort_by_key(|(def_id, _)| def_id.0);
        for (def_id, declared) in &contracts {
            self.validation
                .struct_contracts
                .entry(*def_id)
                .or_default()
                .extend(declared.iter().map(|contract| contract.contract));
        }
        let Some(package_identities) = unit.package_import_identities else {
            return;
        };
        for item in &unit.hir.items {
            let HirItemKind::Import(import) = &item.kind else {
                continue;
            };
            for import_item in &import.items {
                // The package-identity map is keyed by the binding this module
                // actually binds (`import_binding`: the alias when one is
                // spelled, else the imported name), and the resolver installs
                // the file interface under that same binding. Look the binding
                // up once and use it for both so an aliased import registers
                // its implementors like the plain spelling does.
                let binding = import_item.alias.unwrap_or(import_item.name);
                if !package_identities
                    .imports
                    .contains_key(self.interner.resolve(binding))
                {
                    continue;
                }
                for (def_id, _, methods, type_params) in unit.resolver.imported_file_structs_for_namespace(binding) {
                    let Some((_, declared)) = contracts.iter().find(|(def, _)| *def == def_id) else {
                        continue;
                    };
                    // Generic implementors are not lowered across modules.
                    if !type_params.is_empty() {
                        continue;
                    }
                    let Some(receiver_ty) = self.validation.types.find_struct(def_id) else {
                        continue;
                    };
                    for method in &methods {
                        self.record_imported_method_target(def_id, receiver_ty, method);
                    }
                    for contract in declared {
                        let implementors = self
                            .interface_implementors
                            .entry(contract.contract)
                            .or_default();
                        if !implementors.contains(&(def_id, receiver_ty)) {
                            implementors.push((def_id, receiver_ty));
                        }
                    }
                }
            }
        }
    }

    /// Register one imported struct method as a call target under its
    /// consumer-local identity, with the signature validation checks the call
    /// against (`record_method_item` does the same for a unit's own method).
    fn record_imported_method_target(
        &mut self,
        owner_def_id: DefId,
        receiver_ty: TypeId,
        method: &radix_semantic::scope::ImportedFileInterfaceMethod,
    ) {
        let target = MethodTarget { def_id: method.def_id };
        self.method_targets
            .insert((owner_def_id, method.name), target);
        self.method_targets
            .insert((owner_def_id, method.surface_name), target);
        if let Some(err_ty) = method.sig.err {
            self.function_errors
                .insert(method.def_id, MirType::semantic(err_ty));
        }
        self.validation.method_defs.insert(method.def_id);
        let params = std::iter::once(HirParamSignatureProxy { ty: receiver_ty, optional: false }).chain(
            method
                .sig
                .params
                .iter()
                .map(|param| HirParamSignatureProxy { ty: param.ty, optional: param.optional }),
        );
        self.validation
            .functions
            .insert(method.def_id, function_signature(params, method.sig.ret, method.sig.err));
    }

    /// Record imported nominal members so a qualified type test can recover its
    /// target even when the parser preserved `namespace.Type` as a value field.
    fn record_imported_type_members(&mut self, unit: AnalyzedModuleApplicationView<'_>) {
        for item in &unit.hir.items {
            let HirItemKind::Import(import) = &item.kind else {
                continue;
            };
            for import_item in &import.items {
                let namespace = import_item.alias.unwrap_or(import_item.name);
                for export in unit.resolver.imported_file_type_exports(namespace) {
                    let Some(ty) = unit.resolver.namespace_file_type(namespace, export.member) else {
                        continue;
                    };
                    self.imported_type_members
                        .insert((import_item.def_id, export.member), MirType::semantic(ty));
                }
            }
        }
    }

    /// Record folded imported module constants read as `module.NAME`.
    ///
    /// A direct single-file route has no sibling body to link, so the read
    /// inlines the value the file interface carries (the same value the
    /// module-constant fold uses). Package-aware callers rewrite these reads
    /// to linked definitions before lowering, so they never reach this map.
    fn record_imported_constants(&mut self, unit: AnalyzedModuleApplicationView<'_>) {
        struct Reads<'u> {
            unit: AnalyzedModuleApplicationView<'u>,
            found: FxHashMap<(DefId, Symbol), InterfaceConstValue>,
        }
        impl HirVisitor for Reads<'_> {
            fn visit_expr(&mut self, expr: &HirExpression) {
                if let HirExpressionKind::Field(object, member) = &expr.kind
                    && let HirExpressionKind::Path(module) = &object.kind
                    && let Some(namespace) = self.unit.resolver.get_symbol(*module)
                    && namespace.kind == SymbolKind::Module
                    && let Some(value) = self
                        .unit
                        .resolver
                        .namespace_file_const_value_canonical(namespace.name, *member)
                {
                    self.found.insert((*module, *member), value.clone());
                }
                radix_hir::visit::walk_expr(self, expr);
            }
        }
        let mut reads = Reads { unit, found: FxHashMap::default() };
        radix_hir::visit::walk_program(&mut reads, unit.hir);
        self.imported_constants = reads.found;
    }

    fn record_method_item(&mut self, owner_def_id: DefId, receiver_ty: TypeId, method: &HirMethod) {
        self.function_async
            .insert(method.def_id, method.func.is_async);
        self.function_generator
            .insert(method.def_id, method.func.is_generator);
        self.method_targets
            .insert((owner_def_id, method.func.name), MethodTarget { def_id: method.def_id });
        if self.interner.resolve(method.func.name) == "creo" {
            self.creo_hooks.insert(owner_def_id, method.def_id);
        }
        if let Some(err_ty) = method.func.err_ty {
            self.function_errors
                .insert(method.def_id, MirType::semantic(err_ty));
            self.fault_delivery |= method.func.is_kernel;
        }
        self.validation.method_defs.insert(method.def_id);
        if let Some(return_ty) = method.func.ret_ty {
            let params = std::iter::once(HirParamSignatureProxy { ty: receiver_ty, optional: false }).chain(
                method
                    .func
                    .params
                    .iter()
                    .map(|param| HirParamSignatureProxy { ty: param.ty, optional: param.optional }),
            );
            self.validation
                .functions
                .insert(method.def_id, function_signature(params, return_ty, method.func.err_ty));
        }
    }

    /// Collect the coherence registry's user rows into the pair → function
    /// table used by failable-call error recovery (CTR-06). The resolver's
    /// registry is the one authority — the destination enum no longer owns
    /// conversion shape. Builtin rows carry no callable and are skipped;
    /// first enrollment wins a pair, matching the registry's
    /// duplicate-rejecting insertion order.
    fn record_conversion_rows(&mut self, unit: AnalyzedModuleApplicationView<'_>) {
        for row in unit.resolver.conversion_rows() {
            if let Some(fn_def_id) = row.fn_def_id {
                self.conversion_rows.entry(row.pair).or_insert(fn_def_id);
            }
        }
    }

    fn record_enum_item(&mut self, enum_item: &HirEnum, enum_def_id: DefId) {
        let enum_type_params = enum_item
            .type_params
            .iter()
            .map(|param| param.name)
            .collect::<Vec<_>>();
        self.enum_type_params
            .insert(enum_def_id, enum_type_params.clone());
        self.validation
            .enum_type_params
            .insert(enum_def_id, enum_type_params);
        let variants = enum_item
            .variants
            .iter()
            .map(|variant| variant.def_id)
            .collect::<Vec<_>>();
        for (position, variant) in variants.iter().enumerate() {
            self.validation.variant_positions.insert(*variant, position);
        }
        self.enum_variants.insert(enum_def_id, variants.clone());
        self.validation.enum_variants.insert(enum_def_id, variants);
        for variant in &enum_item.variants {
            self.enum_variant_by_name
                .insert((enum_def_id, variant.name), variant.def_id);
        }
        for variant in &enum_item.variants {
            self.variant_parents.insert(variant.def_id, enum_def_id);
            self.validation
                .variant_parents
                .insert(variant.def_id, enum_def_id);
            // D2.8: every enum member prints `Enum.Member`, payload or not.
            self.validation.display_names.insert(
                variant.def_id,
                format!(
                    "{}.{}",
                    self.interner.resolve(enum_item.name),
                    self.interner.resolve(variant.name)
                ),
            );
            let field_names = enum_item
                .commune_fields
                .iter()
                .chain(variant.fields.iter())
                .map(|field| field.name)
                .collect::<Vec<_>>();
            self.validation
                .variant_field_order
                .insert(variant.def_id, field_names.clone());
            self.variant_fields.insert(variant.def_id, field_names);
            let mut fields = FxHashMap::default();
            for field in enum_item.commune_fields.iter().chain(variant.fields.iter()) {
                fields.insert(field.name, MirType::semantic(field.ty));
            }
            self.variant_field_tys
                .insert(variant.def_id, fields.clone());
            self.validation
                .variant_fields
                .insert(variant.def_id, fields);
        }
    }

    fn record_import_item(&mut self, import: &HirImport) {
        let path = self.interner.resolve(import.path);

        for import_item in &import.items {
            let kind = if is_kernel_glob_import_path(path) {
                let module_name = self.interner.resolve(import_item.name);
                resolve_kernel_module_name(module_name).map_or(MirProviderKind::Package, MirProviderKind::Kernel)
            } else if is_kernel_import_path(path) {
                resolve_kernel_module(path).map_or(MirProviderKind::Package, MirProviderKind::Kernel)
            } else {
                MirProviderKind::Package
            };
            self.provider_imports.insert(
                import_item.def_id,
                ProviderImport {
                    kind,
                    module: vec![import.path],
                    item: import_item.name,
                    file_interface_functions: self
                        .package_file_interface_exports
                        .get(&import_item.def_id)
                        .cloned(),
                },
            );
        }
    }

    /// Record package-local import identity facts (D-PA1).
    ///
    /// Package-aware callers supply the canonical `(product, module_path,
    /// item)` facts on the analyzed unit. This pass:
    ///
    /// 1. carries the current package product into the validation context so
    ///    emitters can classify user-module identities vs library identities;
    /// 2. for each local sibling import that resolves to a user-code file,
    ///    allocates a synthesized external-definition id per non-generic
    ///    function export (the verb), recording the verb's signature in
    ///    `validation.functions` and its canonical identity in
    ///    `external_function_identities` (the design's "signature-known
    ///    external definition keyed to the verb, not the binding alias").
    ///
    /// The synthesized ids are keys in the validation tables only; the MIR
    /// call lowers to `MirCallee::Definition { source: <verb id> }`, and the
    /// sibling unit's own compile records the SAME identity for its function
    /// definition, so both emitters agree on one symbol.
    fn record_package_import_identities(&mut self, unit: AnalyzedModuleApplicationView<'a>) {
        let Some(package_identities) = unit.package_import_identities else {
            return;
        };
        self.package_product = Some(package_identities.product.clone());
        self.package_module_segments
            .clone_from(&package_identities.module_segments);
        self.validation.package_product = Some(package_identities.product.clone());
        if package_identities.imports.is_empty() {
            return;
        }
        let mut next_synthetic = radix_types::PROVIDER_DEF_ID_BASE;
        for item in &unit.hir.items {
            let HirItemKind::Import(import) = &item.kind else {
                continue;
            };
            if !self.interner.resolve(import.path).starts_with("./") {
                continue;
            }
            for import_item in &import.items {
                // The package-identity map is keyed by the binding this module
                // actually binds (`import_binding`: the alias when one is
                // spelled, else the imported name), and the resolver installs
                // the file interface under that same binding. Look the
                // binding up once and use it for both, like
                // `record_imported_struct_contracts` does, so an aliased
                // import records its identities like the plain spelling
                // does.
                let binding = import_item.alias.unwrap_or(import_item.name);
                let Some(target_segments) = package_identities
                    .imports
                    .get(self.interner.resolve(binding))
                else {
                    continue;
                };
                let exports = unit.resolver.namespace_file_function_exports(binding);
                if exports.is_empty() {
                    continue;
                }
                let mut by_verb = FxHashMap::default();
                for (verb, sig) in exports {
                    if !sig.type_params.is_empty() {
                        // Generic file-interface exports keep the provider
                        // route; the identity table records concrete
                        // instantiations only.
                        continue;
                    }
                    let def_id = DefId(next_synthetic);
                    next_synthetic += 1;
                    self.validation.functions.insert(
                        def_id,
                        function_signature(
                            sig.params
                                .iter()
                                .map(|param| HirParamSignatureProxy { ty: param.ty, optional: param.optional }),
                            sig.ret,
                            sig.err,
                        ),
                    );
                    if let Some(error_ty) = sig.err {
                        // Imported package exports have synthetic definition ids
                        // which are absent from the local HIR function walk. Keep
                        // their interface error channel in the same lookup used
                        // by direct-call lowering, so they emit `TryCall` just
                        // like local failable definitions.
                        self.function_errors
                            .insert(def_id, MirType::semantic(error_ty));
                    }
                    self.validation.external_function_identities.insert(
                        def_id,
                        MirExternalFunctionIdentity::new(
                            package_identities.product.clone(),
                            target_segments.clone(),
                            self.interner.resolve(verb),
                        ),
                    );
                    by_verb.insert(verb, def_id);
                }
                if !by_verb.is_empty() {
                    self.package_file_interface_exports
                        .insert(import_item.def_id, by_verb);
                }
                // DFV2-3 provider route: carry the imported-device route —
                // role, launch visibility, body channel, provider-route
                // identity — into the consumer link for this import item.
                // Recorded at the import level so a shape-generic imported
                // device function (the spike-3 shape, skipped from the
                // concrete identity table above) keeps its device fact; the
                // route is consumed by the concrete composition pass, never
                // by a signature-only call. The interface is served from the
                // unit's own `PackageImportIdentities` — a fact about this
                // unit's imports, not a process-wide registry (S01-7).
                if let Some(interface) = package_identities
                    .device_interfaces
                    .get(self.interner.resolve(binding))
                {
                    self.fault_delivery |= ImportedDeviceRoute::interface_has_failable_kernel(interface);
                    let routes =
                        ImportedDeviceRoute::from_interface(&package_identities.product, target_segments, interface);
                    if !routes.is_empty() {
                        self.imported_device_routes
                            .insert(import_item.def_id, routes);
                    }
                }
            }
        }
    }

    fn record_library_item_identity(&mut self, item: &LibraryItem) {
        // Function exports carry the canonical external identity today. Const
        // data members get the same `MirExternalFunctionIdentity`-shaped key
        // (product + module path + export name) so package-aware consumers can
        // name a linked const the same way they name a linked verb (S1
        // data-member ABI). Other declaration kinds (structs/enums/interfaces/
        // type aliases) stay unrecorded here — their identity surface is the
        // nominal contract, not the value ABI.
        if !matches!(item.kind, LibraryItemKind::Function | LibraryItemKind::Const) {
            return;
        }
        let product = match &item.identity.provider {
            LibraryProvider::Builtin(name) | LibraryProvider::Package(name) => name.clone(),
        };
        self.validation.external_function_identities.insert(
            item.def_id,
            MirExternalFunctionIdentity::new(product, item.identity.module_path.clone(), item.exported_name.clone()),
        );
    }
}

fn annotations_contain_family(annotations: &[HirAnnotation], interner: &Interner, family: &str) -> bool {
    annotations
        .iter()
        .any(|annotation| interner.resolve(annotation.family) == family)
}

fn json_annotation_nomen(annotations: &[HirAnnotation], interner: &Interner) -> Option<Symbol> {
    let annotation = annotations
        .iter()
        .find(|annotation| interner.resolve(annotation.family) == "json")?;
    let field = annotation.fields.first()?;
    match &field.value {
        HirAnnotationValue::String(symbol) => Some(*symbol),
        HirAnnotationValue::Expr(expr) => match &expr.kind {
            HirExpressionKind::Literal(radix_hir::HirLiteral::String(symbol)) => Some(*symbol),
            _ => None,
        },
        _ => None,
    }
}

struct HirParamSignatureProxy {
    ty: TypeId,
    optional: bool,
}

fn function_signature(
    params: impl Iterator<Item = HirParamSignatureProxy>,
    return_ty: TypeId,
    error_ty: Option<TypeId>,
) -> MirFunctionSignature {
    MirFunctionSignature {
        params: params
            .map(|param| MirParamSignature { ty: MirType::semantic(param.ty), optional: param.optional })
            .collect(),
        return_ty: MirType::semantic(return_ty),
        error_ty: error_ty.map(MirType::semantic),
    }
}

/// Collect callable parameter metadata for MIR call lowering.
pub(super) fn function_param_map(
    unit: AnalyzedModuleApplicationView<'_>,
) -> FxHashMap<DefId, Vec<CallableParamMeta<'_>>> {
    let mut params = FxHashMap::default();
    for item in &unit.hir.items {
        match &item.kind {
            HirItemKind::Function(function) => {
                params.insert(
                    item.def_id,
                    function
                        .params
                        .iter()
                        .map(|param| CallableParamMeta {
                            ty: Some(param.ty),
                            optional: param.optional,
                            default: param.default.as_ref(),
                        })
                        .collect(),
                );
            }
            HirItemKind::Struct(strukt) => {
                if unit.types.find_struct(item.def_id).is_none() {
                    continue;
                }
                for method in &strukt.methods {
                    params.insert(
                        method.def_id,
                        std::iter::once(CallableParamMeta { ty: None, optional: false, default: None })
                            .chain(method.func.params.iter().map(|param| CallableParamMeta {
                                ty: Some(param.ty),
                                optional: param.optional,
                                default: param.default.as_ref(),
                            }))
                            .collect(),
                    );
                }
            }
            _ => {}
        }
    }
    params
}

/// Collect struct field nodes for aggregate lowering.
///
/// Unlike validation's type-only field table, aggregate lowering needs borrowed
/// HIR field nodes so default initializers and field ordering remain available
/// when a constructor is lowered.
pub(super) fn struct_field_map(unit: AnalyzedModuleApplicationView<'_>) -> FxHashMap<DefId, Vec<&HirField>> {
    // WHY: this map intentionally keeps borrowed field nodes for default
    // initializer lowering. `HirVisitor::visit_item` cannot store those
    // borrows with the unit lifetime, so this remains an explicit unit scan.
    let mut structs = FxHashMap::default();
    for item in &unit.hir.items {
        if let HirItemKind::Struct(strukt) = &item.kind {
            let fields = strukt
                .fields
                .iter()
                .filter(|field| !field.is_static())
                .collect();
            structs.insert(item.def_id, fields);
        }
    }
    structs
}

impl HirVisitor for LoweringContextMaps<'_> {
    fn visit_item(&mut self, item: &HirItem) {
        match &item.kind {
            HirItemKind::Function(function) => {
                self.record_function_item(item.def_id, function);
                self.record_ad_route(item);
            }
            HirItemKind::Struct(strukt) => {
                self.record_struct_item(item.def_id, &item.annotations, strukt);
            }
            HirItemKind::Enum(enum_item) => self.record_enum_item(enum_item, item.def_id),
            HirItemKind::Import(import) => self.record_import_item(import),
            HirItemKind::Interface(_)
            | HirItemKind::TypeAlias(_)
            | HirItemKind::Schema(_)
            | HirItemKind::Constant(_) => {}
        }
    }
}

#[cfg(test)]
#[path = "context_test.rs"]
mod context_tests;
