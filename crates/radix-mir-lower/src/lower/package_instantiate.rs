//! Package-link instantiation of leftover generic `Definition` callees.
//!
//! Unit lowering keys instances by [`MirMonomorphizationKey`] `(source,
//! type_args, shape_args)` and then drops the generic template. Package
//! linking previously re-identified every instance by `function.source`
//! alone, so `_sgd_family<[2,2]>` and `_sgd_family<[8,8]>` collapsed and
//! the caller's arguments were checked against the first signature. This
//! pass keeps the templates, rebuilds the shape-discriminating index, and
//! instantiates any shape the defining unit never called.

use super::{
    MirError, MirGenericParam, MirTensorEdgePolicyMetadata, MirTensorEdgePolicyProvider, MirTensorEdgePolicySelector,
    MirTensorLimesTemplate,
};
use radix_hir::DefId;
use radix_lexer::{Interner, Span};
use radix_mir::device::MirDeviceFunctionMetadataMap;
use radix_mir::generic::{
    MirShapeBindings, bind_generic_type_witness_with_shapes, canonical_index, function_contains_shape_size,
    instantiate_existing_generic_type, instantiate_generic_type_with_shapes, substitute_shape_size_constants,
    substitute_tensor_from_flat_shape_size_constants,
};
use radix_mir::{
    MirBuiltinEdgePolicy, MirCallee, MirEdgePolicy, MirExternalFunctionIdentity, MirFunction, MirFunctionId,
    MirFunctionIndex, MirGenericBindings, MirIntrinsic, MirMonomorphization, MirMonomorphizationKey, MirOperand,
    MirPlace, MirPlaceBase, MirProgram, MirProjection, MirStatementKind, MirTerminatorKind, MirType,
    type_contains_param,
};
use radix_semantic::file_interface::{
    FileExportKind, FileInterface, InterfaceDeviceContract, InterfaceDeviceRole, InterfaceDeviceVisibility,
};
use radix_semantic::{IndexExpr, IndexId, Primitive, Type, TypeId, TypeTable};
use radix_types::RuntimeExtentId;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::HashMap;

const MAX_PACKAGE_MONOMORPHIZATIONS: usize = 64;
const UNRESOLVED_SHAPE_SIZE_SURVIVOR: &str = "unresolved shape-size constant survives monomorphization";
/// Named fail-closed reason for a signature-only imported device import
/// (DFV2-3 (e)): the role fact alone never admits the instance.
const IMPORTED_DEVICE_BODY_MISSING: &str = "imported_device_body_missing";
/// Named fail-closed reason for an imported device instantiation whose size
/// bindings cannot be resolved from the use site (SGR-U0): a device entry
/// admits only concrete instances, so an unbound shape parameter is a
/// classified rejection, never a silent provider fallback.
const IMPORTED_DEVICE_BINDINGS_UNRESOLVED: &str = "imported_device_bindings_unresolved";
/// Named fail-closed reason for a consumer-local role disagreeing with an
/// imported route's contract on one merged source (CDC-1): the composition
/// role map is one authority, so a contradictory join is a wiring defect,
/// never a silent override (same defect class as
/// `imported_device_entry_role_conflict`, at the composition seam).
const CONSUMER_DEVICE_ROLE_CONFLICT: &str = "consumer_device_role_conflict";

/// One resolved imported-device route (DFV2-3 transport): the provider-route
/// identity plus the v2 device contract carried from the exporting module's
/// file interface through the consumer link.
///
/// Role and body travel together by construction — the contract carries the
/// explicit body channel — so the composition unit (DFV2-4) can never observe
/// a role fact without its body availability.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedDeviceRoute {
    /// Canonical provider-route identity: product, module path, verb.
    pub identity: MirExternalFunctionIdentity,
    /// v2 device fact (role, launch visibility) + explicit body channel.
    pub contract: InterfaceDeviceContract,
}

impl ImportedDeviceRoute {
    /// Build the consumer-side routes for one imported module's file
    /// interface: every function export that carries a device contract.
    ///
    /// Non-device exports are not routes. A device export with a missing
    /// body channel stays a route (carrying [`InterfaceDeviceContract`]'s
    /// `Missing` state) so the consumer fails closed on it instead of
    /// silently losing the device fact.
    #[must_use]
    pub fn from_interface(product: &str, module_segments: &[String], interface: &FileInterface) -> Vec<Self> {
        interface
            .exports
            .values()
            .filter_map(|export| {
                let FileExportKind::Function(callable) = &export.kind else {
                    return None;
                };
                let contract = callable.device.clone()?;
                Some(Self {
                    identity: MirExternalFunctionIdentity::new(
                        product.to_owned(),
                        module_segments.to_vec(),
                        export.name.clone(),
                    ),
                    contract,
                })
            })
            .collect()
    }

    /// Stable display key for diagnostics: `product::module::verb`.
    #[must_use]
    pub fn display_key(&self) -> String {
        format!(
            "{}::{}::{}",
            self.identity.product,
            self.identity.module_path.join("::"),
            self.identity.name
        )
    }
}

/// Consumer-side index of imported device functions, keyed by the merged
/// program's template source `DefId` (DFV2-3 transport input).
///
/// Package linking rewrites each unit's generic-template sources to synthetic
/// defs; the linking layer joins those defs to routes here. DFV2-3 supplies
/// the index and the request semantics; DFV2-4 consumes the resolved routes.
#[derive(Debug, Clone, Default)]
pub struct ImportedDeviceIndex {
    by_source: FxHashMap<DefId, ImportedDeviceRoute>,
}

impl ImportedDeviceIndex {
    /// Index routes by their merged template source def.
    #[must_use]
    pub fn from_routes(routes: impl IntoIterator<Item = (DefId, ImportedDeviceRoute)>) -> Self {
        Self { by_source: routes.into_iter().collect() }
    }

    /// The route registered for one imported template source, if any.
    #[must_use]
    pub fn get(&self, source: DefId) -> Option<&ImportedDeviceRoute> {
        self.by_source.get(&source)
    }

    /// All indexed routes with their template sources (composition input).
    pub fn iter(&self) -> impl Iterator<Item = (&DefId, &ImportedDeviceRoute)> {
        self.by_source.iter()
    }
}

/// One concrete imported device instance registered for the consumer (DFV2-3
/// transport output).
///
/// Design §6: a concrete instantiation of a generic `@ nucleum` function
/// registers the instance as a device function of the compiling unit. The
/// concrete device-call composition (DFV2-4) consumes these as its input; a
/// `Canonical` body channel rides through unchanged.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedDeviceRegistration {
    /// Concrete instantiation key (source + type/shape args).
    pub key: MirMonomorphizationKey,
    /// Concrete instance function in the merged program.
    pub function: MirFunctionId,
    /// The imported-device route the instance was admitted under.
    pub route: ImportedDeviceRoute,
}

/// Merged struct metadata for typing projected operands during package
/// generic instantiation (`layer.ln1` on an applied nominal needs the field
/// type substituted by the applied size args, not the container type).
pub struct PackageStructMetadata<'a> {
    pub struct_fields: &'a HashMap<DefId, HashMap<radix_lexer::Symbol, MirType>>,
    pub struct_type_params: &'a HashMap<DefId, Vec<radix_lexer::Symbol>>,
}

/// Instantiate remaining generic `Definition` callees in a merged package
/// program, distinguishing instances by [`MirMonomorphizationKey`] shape args.
///
/// Existing instances (from the defining unit's own calls) are indexed by
/// binding the template against their concrete param types. Missing shapes
/// clone the kept template. New instances are shape-finalized before return.
///
/// # Errors
/// Returns [`MirError`] when a call site cannot bind, a template is missing,
/// substitution fails, or a shape-size scalar survives finalization.
pub fn instantiate_merged_generic_calls(
    program: &mut MirProgram,
    templates: &[MirFunction],
    generic_params: &[(DefId, Vec<MirGenericParam>)],
    types: &mut TypeTable,
    interner: &Interner,
    struct_metadata: Option<&PackageStructMetadata<'_>>,
) -> Result<Vec<MirMonomorphization>, Vec<MirError>> {
    instantiate_merged_generic_calls_with_devices(
        program,
        templates,
        generic_params,
        types,
        interner,
        struct_metadata,
        &ImportedDeviceIndex::from_routes([]),
        &HashMap::new(),
        &std::collections::HashSet::new(),
    )
    .map(|(monomorphizations, _)| monomorphizations)
}

/// Device-aware package-link instantiation (DFV2-3 transport + DFV2-4
/// composition, CDC-1 consumer-local union).
///
/// Beyond [`instantiate_merged_generic_calls`], every concrete instantiation
/// of an indexed imported device function is registered for the consumer
/// ([`ImportedDeviceRegistration`]), a signature-only imported device import
/// fails closed with the named `imported_device_body_missing` reason before
/// composition (the role bit alone never validates the call), and the merged
/// program's device calls — imported instance bodies calling other imported
/// device functions and consumer-local device callers calling imported
/// instances — compose here, between shape finalization and the caller's
/// `ValidatedMir` rebuild. The composition's role and launch-entry inputs are
/// the merged union of the imported routes and the consumer units' own device
/// roles and public Kernel launch sources (CDC-1, leaf-tail union design
/// ruling 2026-08-27), so consumer-local-only packages reach compose; a
/// consumer-local role that disagrees with a route contract on one source
/// fails closed with `consumer_device_role_conflict`.
///
/// # Errors
/// Returns [`MirError`] when a call site cannot bind, a template is missing,
/// substitution fails, a shape-size scalar survives finalization, an
/// imported device function has no usable body channel, the role union is
/// contradictory, or composition rejects a cycle or expansion budget.
#[allow(clippy::type_complexity)]
pub fn instantiate_merged_generic_calls_with_devices(
    program: &mut MirProgram,
    templates: &[MirFunction],
    generic_params: &[(DefId, Vec<MirGenericParam>)],
    types: &mut TypeTable,
    interner: &Interner,
    struct_metadata: Option<&PackageStructMetadata<'_>>,
    imported_devices: &ImportedDeviceIndex,
    consumer_roles: &HashMap<DefId, radix_mir::MirDeviceRole>,
    consumer_launch_entries: &std::collections::HashSet<DefId>,
) -> Result<(Vec<MirMonomorphization>, Vec<ImportedDeviceRegistration>), Vec<MirError>> {
    let mut device_function_metadata = MirDeviceFunctionMetadataMap::default();
    instantiate_merged_generic_calls_with_devices_and_metadata(
        program,
        templates,
        generic_params,
        types,
        interner,
        struct_metadata,
        &mut device_function_metadata,
        imported_devices,
        consumer_roles,
        consumer_launch_entries,
    )
}

/// Device-aware package-link instantiation with the lowered function metadata
/// carrier. Production package linking uses this entry point so source-derived
/// visibility and source-less synthetic closure identity survive the generic
/// instantiation/composition boundary.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
#[allow(clippy::type_complexity)]
pub fn instantiate_merged_generic_calls_with_devices_and_metadata(
    program: &mut MirProgram,
    templates: &[MirFunction],
    generic_params: &[(DefId, Vec<MirGenericParam>)],
    types: &mut TypeTable,
    interner: &Interner,
    struct_metadata: Option<&PackageStructMetadata<'_>>,
    device_function_metadata: &mut MirDeviceFunctionMetadataMap,
    imported_devices: &ImportedDeviceIndex,
    consumer_roles: &HashMap<DefId, radix_mir::MirDeviceRole>,
    consumer_launch_entries: &std::collections::HashSet<DefId>,
) -> Result<(Vec<MirMonomorphization>, Vec<ImportedDeviceRegistration>), Vec<MirError>> {
    instantiate_merged_generic_calls_with_tensor_edge_policy(
        program,
        templates,
        generic_params,
        types,
        interner,
        struct_metadata,
        device_function_metadata,
        imported_devices,
        consumer_roles,
        consumer_launch_entries,
        &MirTensorEdgePolicyMetadata::default(),
        true,
    )
}

/// Package-link instantiation with portable tensor edge-policy facts.
///
/// Early package append steps may defer a custom limes instance until its
/// provider module is linked. The final package pass sets
/// `require_tensor_edge_resolution` and fails closed if any concrete limes
/// site still has no provider or callback target.
#[allow(clippy::type_complexity)]
pub fn instantiate_merged_generic_calls_with_tensor_edge_policy(
    program: &mut MirProgram,
    templates: &[MirFunction],
    generic_params: &[(DefId, Vec<MirGenericParam>)],
    types: &mut TypeTable,
    interner: &Interner,
    struct_metadata: Option<&PackageStructMetadata<'_>>,
    device_function_metadata: &mut MirDeviceFunctionMetadataMap,
    imported_devices: &ImportedDeviceIndex,
    consumer_roles: &HashMap<DefId, radix_mir::MirDeviceRole>,
    consumer_launch_entries: &std::collections::HashSet<DefId>,
    tensor_edge_policy_metadata: &MirTensorEdgePolicyMetadata,
    require_tensor_edge_resolution: bool,
) -> Result<(Vec<MirMonomorphization>, Vec<ImportedDeviceRegistration>), Vec<MirError>> {
    if templates.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let next_id = MirProgram::allocate_function_id(&program.functions).map_err(|_| {
        vec![MirError::validation(
            Span::new(0, 0),
            "package MIR function id space is exhausted",
        )]
    })?;
    let mut state = PackageInstantiate {
        types,
        interner,
        struct_metadata,
        templates: templates
            .iter()
            .filter_map(|template| template.source.map(|source| (source, template.clone())))
            .collect(),
        generic_params: generic_params.iter().cloned().collect(),
        tensor_limes_templates: tensor_edge_policy_metadata
            .generic_limes
            .iter()
            .copied()
            .map(|site| ((site.source, site.destination), site))
            .collect(),
        tensor_edge_policy_providers: tensor_edge_policy_metadata.providers.clone(),
        function_ids_by_source: program
            .functions
            .iter()
            .filter_map(|function| function.source.map(|source| (source, function.id)))
            .collect(),
        require_tensor_edge_resolution,
        instance_index: FxHashMap::default(),
        monomorphizations: Vec::new(),
        next_id,
        errors: Vec::new(),
        created: Vec::new(),
        imported_devices: imported_devices.clone(),
        device_registrations: Vec::new(),
        registered_device_keys: FxHashSet::default(),
    };
    state.index_existing_instances(&program.functions);
    if !state.errors.is_empty() {
        return Err(state.errors);
    }
    state.resolve_existing_tensor_limes(&mut program.functions);
    if !state.errors.is_empty() {
        return Err(state.errors);
    }
    state.rewrite_program(program);
    if !state.errors.is_empty() {
        return Err(state.errors);
    }
    let created_ids: FxHashSet<MirFunctionId> = state.created.iter().map(|function| function.id).collect();
    program.functions.extend(std::mem::take(&mut state.created));
    state.finalize_new_instances(program, &created_ids);
    if !state.errors.is_empty() {
        return Err(state.errors);
    }
    // A generic device template's source row must follow every concrete
    // instance id. Without this copy, the fail-closed constructor would omit
    // an imported public kernel even though its route carried public
    // visibility. Synthetic closure rows remain keyed by their own lowered
    // function ids and are not inferred here. Every program function that
    // instantiates a template and still lacks a row receives it, not only
    // this pass's new instances: an instance created while a sibling unit
    // appends (the library's own rows not yet merged) is completed by the
    // later pass that sees the template's row.
    let template_metadata: FxHashMap<DefId, _> = templates
        .iter()
        .filter_map(|template| {
            let source = template.source?;
            let metadata = device_function_metadata.get(&template.id).copied()?;
            Some((source, metadata))
        })
        .collect();
    let created_metadata = program
        .functions
        .iter()
        .filter(|function| !device_function_metadata.contains_key(&function.id))
        .filter_map(|function| {
            let source = function.source?;
            let mut metadata = *template_metadata.get(&source)?;
            metadata.identity.parent = function.id;
            Some((function.id, metadata))
        })
        .collect::<Vec<_>>();
    device_function_metadata.extend(created_metadata);

    // DFV2-4 merged-pipeline composition: after shapes finalize, imported
    // device instances are device functions of this program (routes carry
    // role + launch visibility), so their device calls compose before the
    // caller rebuilds `ValidatedMir`. CDC-1: the consumer units' own device
    // roles and public Kernel launch sources join the same inputs — the
    // leaf-tail union — so a consumer-local device caller's imported-instance
    // call composes in this one pass, and consumer-local-only packages
    // (no imported routes) reach compose. Empty union composes nothing.
    let mut device_roles = consumer_roles
        .iter()
        .map(|(def, role)| (*def, *role))
        .collect::<FxHashMap<DefId, radix_mir::MirDeviceRole>>();
    for (source, route) in imported_devices.iter() {
        let role = mir_role(route.contract.role);
        if let Some(consumer) = device_roles.insert(*source, role)
            && consumer != role
        {
            return Err(vec![MirError {
                message: format!(
                    "consumer-local device role for def#{} conflicts with the imported route ({consumer:?} vs {role:?})",
                    source.0
                ),
                issue: CONSUMER_DEVICE_ROLE_CONFLICT.to_owned(),
                span: Span::default(),
                definition_span: None,
            }]);
        }
    }
    if !device_roles.is_empty() {
        let mut launch_entries = consumer_launch_entries
            .iter()
            .copied()
            .collect::<FxHashSet<DefId>>();
        launch_entries.extend(
            imported_devices
                .iter()
                .filter(|(_, route)| {
                    route.contract.visibility == InterfaceDeviceVisibility::Public
                        && mir_role(route.contract.role) == radix_mir::MirDeviceRole::Kernel
                })
                .map(|(source, _)| *source),
        );
        let composition_errors = crate::fragment_composition::compose_with_metadata(
            &mut program.functions,
            &state.monomorphizations,
            &device_roles,
            device_function_metadata,
            crate::fragment_composition::active_budget(),
            Some(interner),
            &launch_entries,
        );
        if !composition_errors.is_empty() {
            return Err(composition_errors
                .into_iter()
                .map(|error| MirError {
                    message: error.message(),
                    issue: error.issue.to_owned(),
                    span: error.span,
                    definition_span: error.definition_span,
                })
                .collect());
        }
    }
    Ok((state.monomorphizations, state.device_registrations))
}

/// Map the interface device role onto the MIR role of the same meaning.
fn mir_role(role: InterfaceDeviceRole) -> radix_mir::MirDeviceRole {
    match role {
        InterfaceDeviceRole::Kernel => radix_mir::MirDeviceRole::Kernel,
        InterfaceDeviceRole::Fragment => radix_mir::MirDeviceRole::Fragment,
    }
}

pub struct PackageInstantiate<'a> {
    pub types: &'a mut TypeTable,
    pub interner: &'a Interner,
    pub struct_metadata: Option<&'a PackageStructMetadata<'a>>,
    pub templates: FxHashMap<DefId, MirFunction>,
    pub generic_params: FxHashMap<DefId, Vec<MirGenericParam>>,
    pub tensor_limes_templates: FxHashMap<(DefId, radix_mir::MirTempId), MirTensorLimesTemplate>,
    pub tensor_edge_policy_providers: Vec<MirTensorEdgePolicyProvider>,
    pub function_ids_by_source: FxHashMap<DefId, MirFunctionId>,
    pub require_tensor_edge_resolution: bool,
    pub instance_index: FxHashMap<MirMonomorphizationKey, MirFunctionId>,
    pub monomorphizations: Vec<MirMonomorphization>,
    pub next_id: MirFunctionId,
    pub errors: Vec<MirError>,
    pub created: Vec<MirFunction>,
    /// Imported device functions indexed by template source (DFV2-3).
    pub imported_devices: ImportedDeviceIndex,
    /// Concrete imported device instances registered for the consumer.
    pub device_registrations: Vec<ImportedDeviceRegistration>,
    /// Registration dedupe guard: one registration per monomorphization key.
    pub registered_device_keys: FxHashSet<MirMonomorphizationKey>,
}

#[derive(Clone)]
struct FunctionTypeScope {
    locals: FxHashMap<radix_mir::MirLocalId, MirType>,
    temps: FxHashMap<radix_mir::MirTempId, MirType>,
    values: FxHashMap<radix_mir::MirValueId, MirType>,
}

impl FunctionTypeScope {
    fn from_function(function: &MirFunction) -> Self {
        Self {
            locals: function
                .locals
                .iter()
                .map(|local| (local.id, local.ty))
                .collect(),
            temps: function
                .temps
                .iter()
                .map(|temp| (temp.id, temp.ty))
                .collect(),
            values: FxHashMap::default(),
        }
    }
}

struct PendingCall {
    function: MirFunctionId,
    block_index: usize,
    statement_index: Option<usize>,
    destination: Option<MirPlace>,
    error_place: Option<MirPlace>,
    source: DefId,
    type_args: Vec<MirType>,
    args: Vec<MirOperand>,
    span: Span,
    scope: FunctionTypeScope,
}

/// A-133: outcome of an explicit size witness lookup in package linking.
/// `Found` carries the resolved index; `MissingReported` means the lookup
/// already pushed the more specific diagnostic, so the caller must not repeat
/// it; `MissingUnreported` leaves the kind-mismatch diagnostic to the caller,
/// so one failed argument reports exactly once.
enum ExplicitShapeWitness {
    Found(radix_semantic::IndexId),
    MissingReported,
    MissingUnreported,
}

impl PackageInstantiate<'_> {
    fn index_existing_instances(&mut self, functions: &[MirFunction]) {
        for function in functions {
            let Some(source) = function.source else {
                continue;
            };
            if !self.templates.contains_key(&source) {
                continue;
            }
            if super::monomorphize::function_contains_param(self.types, function) {
                continue;
            }
            let Some(key) = self.key_from_instance(function) else {
                continue;
            };
            self.instance_index.insert(key.clone(), function.id);
            self.monomorphizations
                .push(MirMonomorphization { key, function: function.id });
        }
    }

    fn resolve_existing_tensor_limes(&mut self, functions: &mut [MirFunction]) {
        for function in functions {
            let Some(source) = function.source else {
                continue;
            };
            if !self
                .tensor_limes_templates
                .keys()
                .any(|(template, _)| *template == source)
                || super::monomorphize::function_contains_param(self.types, function)
            {
                continue;
            }
            let Some(key) = self.key_from_instance(function) else {
                continue;
            };
            let span = function.span;
            if !self.resolve_tensor_limes_for_instance(&key, function, span) {
                if self.errors.is_empty() {
                    self.errors.push(MirError::validation(
                        span,
                        "concrete MIR function contains a deferred tensor limes policy before package linking completed",
                    ));
                }
                return;
            }
        }
    }

    fn key_from_instance(&mut self, function: &MirFunction) -> Option<MirMonomorphizationKey> {
        let source = function.source?;
        let template = self.templates.get(&source)?.clone();
        let params = self.generic_params.get(&source)?.clone();
        let mut type_bindings = MirGenericBindings::default();
        let mut shape_bindings = MirShapeBindings::default();
        for (expected, actual) in template.params.iter().zip(function.params.iter()) {
            if !bind_generic_type_witness_with_shapes(
                self.types,
                expected.ty,
                actual.ty,
                &mut type_bindings,
                &mut shape_bindings,
            ) {
                return None;
            }
        }
        // The return type only adds bindings when it witnesses them; a param
        // left unbound is rejected by the `?` lookups below, so a failed
        // return-type bind needs no separate exit.
        bind_generic_type_witness_with_shapes(
            self.types,
            template.return_ty,
            function.return_ty,
            &mut type_bindings,
            &mut shape_bindings,
        );
        let mut type_args = Vec::new();
        let mut shape_args = Vec::new();
        for param in &params {
            if param.is_shape {
                let bound = shape_bindings.get(&param.name).copied()?;
                let concrete =
                    super::monomorphize::resolve_parent_shape_binding(self.types, bound, &MirShapeBindings::default())?;
                shape_args.push(canonical_index(self.types, concrete));
            } else {
                let bound = type_bindings.get(&param.name).copied()?;
                type_args.push(bound);
            }
        }
        Some(MirMonomorphizationKey { source, type_args, shape_args })
    }

    fn rewrite_program(&mut self, program: &mut MirProgram) {
        // One index per scope: `program.functions` is frozen for the whole
        // loop; newly created instances ride `self.created` until the caller
        // appends them after this pass.
        let function_index = program.function_index();
        let mut worklist: Vec<MirFunctionId> = program
            .functions
            .iter()
            .map(|function| function.id)
            .collect();
        let mut seen = FxHashSet::default();
        while let Some(function_id) = worklist.pop() {
            if !seen.insert(function_id) {
                continue;
            }
            if !self.errors.is_empty() {
                return;
            }
            let calls = {
                let Some(function) = program
                    .indexed_function(&function_index, function_id)
                    .cloned()
                    .or_else(|| {
                        self.created
                            .iter()
                            .find(|function| function.id == function_id)
                            .cloned()
                    })
                else {
                    continue;
                };
                if super::monomorphize::function_contains_param(self.types, &function) {
                    continue;
                }
                self.collect_calls(&function)
            };
            for call in calls {
                if let Some(target) = self.instantiate_call(&call) {
                    self.apply_rewrite(program, &function_index, &call, target);
                    if self.created.iter().any(|function| function.id == target) {
                        worklist.push(target);
                    }
                }
            }
        }
    }

    fn collect_calls(&self, function: &MirFunction) -> Vec<PendingCall> {
        let mut calls = Vec::new();
        let mut scope = FunctionTypeScope::from_function(function);
        for (block_index, block) in function.blocks.iter().enumerate() {
            for (statement_index, statement) in block.statements.iter().enumerate() {
                if let MirStatementKind::Assign { value, .. } = &statement.kind {
                    scope.values.insert(value.id, value.ty);
                }
                if let MirStatementKind::Call { destination, callee: MirCallee::Definition { source, type_args }, args } =
                    &statement.kind
                    && self.templates.contains_key(source)
                {
                    calls.push(PendingCall {
                        function: function.id,
                        block_index,
                        statement_index: Some(statement_index),
                        destination: destination.clone(),
                        error_place: None,
                        source: *source,
                        type_args: type_args.clone(),
                        args: args.clone(),
                        span: statement.span,
                        scope: scope.clone(),
                    });
                }
            }
            if let MirTerminatorKind::TryCall {
                destination,
                callee: MirCallee::Definition { source, type_args },
                args,
                error_place,
                ..
            } = &block.terminator.kind
                && self.templates.contains_key(source)
            {
                calls.push(PendingCall {
                    function: function.id,
                    block_index,
                    statement_index: None,
                    destination: destination.clone(),
                    error_place: Some(error_place.clone()),
                    source: *source,
                    type_args: type_args.clone(),
                    args: args.clone(),
                    span: block.terminator.span,
                    scope: scope.clone(),
                });
            }
        }
        calls
    }

    fn instantiate_call(&mut self, call: &PendingCall) -> Option<MirFunctionId> {
        let template = self.templates.get(&call.source)?.clone();
        let params = self.generic_params.get(&call.source)?.clone();
        let parent_shapes = self.instance_shape_bindings(call.function);
        let mut type_bindings = MirGenericBindings::default();
        let mut shape_bindings = MirShapeBindings::default();
        if !self.bind_explicit_type_args(
            &params,
            &call.type_args,
            &mut type_bindings,
            &mut shape_bindings,
            &parent_shapes,
            call.span,
        ) {
            return None;
        }
        // Concrete witnesses first. A Path(synthetic) namespace callee is
        // untyped, so some argument temps still carry the generic export
        // shape; the ascribed destination is the concrete [8,8]/[8] fact.
        if let Some(destination) = &call.destination
            && let Some(actual) = self.place_ty(destination, &call.scope, call.span)
            && !type_contains_param(self.types, actual.semantic_id())
            && !bind_generic_type_witness_with_shapes(
                self.types,
                template.return_ty,
                actual,
                &mut type_bindings,
                &mut shape_bindings,
            )
        {
            self.errors
                .push(MirError::validation(call.span, "destination type mismatch"));
            return None;
        }
        for (index, arg) in call.args.iter().enumerate() {
            let Some(expected) = template.params.get(index) else {
                continue;
            };
            let Some(actual) = self.operand_ty(arg, &call.scope, call.span) else {
                continue;
            };
            if type_contains_param(self.types, actual.semantic_id()) {
                continue;
            }
            if !bind_generic_type_witness_with_shapes(
                self.types,
                expected.ty,
                actual,
                &mut type_bindings,
                &mut shape_bindings,
            ) {
                let expected_ty = self.format_ty(expected.ty);
                let actual_ty = self.format_ty(actual);
                self.errors.push(MirError {
                    message: format!(
                        "invalid MIR: call argument type mismatch (arg {index}: expected {expected_ty}, got {actual_ty})"
                    ),
                    issue: "invalid_mir_call_argument_type_mismatch".to_owned(),
                    span: call.span,
                    definition_span: None,
                });
                return None;
            }
        }
        // The catch place is the only witness for an alternate-exit type
        // parameter that no argument or destination mentions (`⇥ E`).
        if let (Some(error_ty), Some(error_place)) = (template.error_ty, &call.error_place)
            && type_contains_param(self.types, error_ty.semantic_id())
            && let Some(actual) = self.place_ty(error_place, &call.scope, call.span)
            && !type_contains_param(self.types, actual.semantic_id())
            && !bind_generic_type_witness_with_shapes(
                self.types,
                error_ty,
                actual,
                &mut type_bindings,
                &mut shape_bindings,
            )
        {
            self.errors
                .push(MirError::validation(call.span, "error destination type mismatch"));
            return None;
        }
        let key =
            self.key_from_bindings(call.source, &params, &type_bindings, &shape_bindings, &parent_shapes, call.span)?;
        self.ensure_instance(&template, &key, &type_bindings, call.span)
    }

    fn bind_explicit_type_args(
        &mut self,
        params: &[MirGenericParam],
        explicit: &[MirType],
        type_bindings: &mut MirGenericBindings,
        shape_bindings: &mut MirShapeBindings,
        parent_shapes: &MirShapeBindings,
        span: Span,
    ) -> bool {
        if explicit.is_empty() {
            return true;
        }
        let type_params: Vec<_> = params.iter().filter(|param| !param.is_shape).collect();
        let shape_params: Vec<_> = params.iter().filter(|param| param.is_shape).collect();
        if type_params.is_empty() && !shape_params.is_empty() {
            if explicit.len() != shape_params.len() {
                self.errors.push(MirError::validation(
                    span,
                    format!(
                        "generic call supplied {} type arguments for {} parameters",
                        explicit.len(),
                        shape_params.len()
                    ),
                ));
                return false;
            }
            for (param, concrete) in shape_params.iter().zip(explicit.iter().copied()) {
                // Size-only generics: NATURAL typeArguments bind magnitudo
                // params; a caller-owned symbolic `Param` resolves through
                // the calling instance's shape bindings (GSF-1 forwarding).
                let index = match self.explicit_shape_witness(concrete, parent_shapes, span) {
                    ExplicitShapeWitness::Found(index) => index,
                    ExplicitShapeWitness::MissingReported => return false,
                    ExplicitShapeWitness::MissingUnreported => {
                        self.errors.push(MirError::validation(
                            span,
                            "generic call type argument does not match parameter kind",
                        ));
                        return false;
                    }
                };
                shape_bindings.insert(param.name, canonical_index(self.types, index));
            }
            return true;
        }
        if explicit.len() != type_params.len() {
            self.errors.push(MirError::validation(
                span,
                format!(
                    "generic call supplied {} type arguments for {} parameters",
                    explicit.len(),
                    type_params.len()
                ),
            ));
            return false;
        }
        for (param, concrete) in type_params.iter().zip(explicit.iter().copied()) {
            type_bindings.insert(param.name, concrete);
        }
        true
    }

    /// Index witness of an explicit magnitudo argument: a NATURAL spelling
    /// maps to its literal index; a caller-owned symbolic `Param` resolves
    /// through the calling instance's shape bindings (GSF-1 forwarding —
    /// package instances are concretized at creation, so the parent map is
    /// authoritative at bind time). Anything else fails closed so wrong-kind
    /// rejection is preserved.
    fn explicit_shape_witness(
        &mut self,
        concrete: MirType,
        parent_shapes: &MirShapeBindings,
        span: Span,
    ) -> ExplicitShapeWitness {
        let Type::Param(sym) = self.types.get(concrete.semantic_id()) else {
            return ExplicitShapeWitness::MissingUnreported;
        };
        let sym = *sym;
        if let Some(value) = radix_types::parse_natural_capacity(self.interner.resolve(sym)) {
            return match find_literal_index(self.types, value) {
                Some(index) => ExplicitShapeWitness::Found(index),
                None => {
                    self.errors.push(MirError::validation(
                        span,
                        "generic call signature has unresolved type parameter",
                    ));
                    ExplicitShapeWitness::MissingReported
                }
            };
        }
        match parent_shapes.get(&sym).copied() {
            Some(index) => ExplicitShapeWitness::Found(index),
            None => {
                self.errors.push(MirError::validation(
                    span,
                    "generic call type argument does not match parameter kind",
                ));
                ExplicitShapeWitness::MissingReported
            }
        }
    }

    pub fn key_from_bindings(
        &mut self,
        source: DefId,
        params: &[MirGenericParam],
        type_bindings: &MirGenericBindings,
        shape_bindings: &MirShapeBindings,
        parent_shapes: &MirShapeBindings,
        span: Span,
    ) -> Option<MirMonomorphizationKey> {
        // SGR-U0 fail-closed classification: an imported device route whose
        // bindings do not resolve carries the stable issue identity, not the
        // generic unresolved-parameter prose alone.
        let issue = if self.imported_devices.get(source).is_some() {
            IMPORTED_DEVICE_BINDINGS_UNRESOLVED.to_owned()
        } else {
            String::new()
        };
        let unresolved = |state: &mut Self| {
            state.errors.push(MirError {
                message: "generic call signature has unresolved type parameter".to_owned(),
                issue: issue.clone(),
                span,
                definition_span: None,
            });
        };
        let mut type_args = Vec::new();
        let mut shape_args = Vec::new();
        for param in params {
            if param.is_shape {
                let Some(bound) = shape_bindings.get(&param.name).copied() else {
                    unresolved(self);
                    return None;
                };
                let contains_runtime =
                    super::index_contains_runtime_extent(self.types, bound, &mut FxHashSet::default());
                let concrete = if contains_runtime {
                    if runtime_extents_are_caller_formal_axes(self.types, bound, &mut FxHashSet::default()) {
                        // A nested size-generic call: package linking pre-resolves
                        // forwarded size args through the calling instance's shape
                        // bindings, so the binding already names the caller's own
                        // formal tensor axes. Key by it — per-caller instances whose
                        // signatures match the caller's argument types — mirroring
                        // the same-unit monomorphizer (GSF-1), which keys nested
                        // calls by the parent-resolved extent. Collapsing to the
                        // callee's formal witness here would validate every caller
                        // whose argument sits at a different parameter slot as a
                        // type mismatch.
                        Some(bound)
                    } else {
                        param.runtime_witness
                    }
                } else {
                    super::monomorphize::resolve_parent_shape_binding(self.types, bound, parent_shapes)
                };
                let Some(concrete) = concrete else {
                    if contains_runtime {
                        self.errors.push(MirError {
                            message: "runtime generic shape has no stable formal carrier witness".to_owned(),
                            issue: issue.clone(),
                            span,
                            definition_span: None,
                        });
                        return None;
                    }
                    unresolved(self);
                    return None;
                };
                shape_args.push(canonical_index(self.types, concrete));
            } else {
                let Some(bound) = type_bindings.get(&param.name).copied() else {
                    unresolved(self);
                    return None;
                };
                type_args.push(bound);
            }
        }
        Some(MirMonomorphizationKey { source, type_args, shape_args })
    }

    fn instance_shape_bindings(&self, function: MirFunctionId) -> MirShapeBindings {
        let Some(monomorphization) = self
            .monomorphizations
            .iter()
            .find(|monomorphization| monomorphization.function == function)
        else {
            return MirShapeBindings::default();
        };
        let Some(params) = self.generic_params.get(&monomorphization.key.source) else {
            return MirShapeBindings::default();
        };
        params
            .iter()
            .filter(|param| param.is_shape)
            .zip(monomorphization.key.shape_args.iter().copied())
            .map(|(param, shape)| (param.name, shape))
            .collect()
    }

    /// Concrete shape bindings for one monomorphization key: shape params in
    /// declaration order paired with the key's shape args.
    fn shape_bindings_from_key(&self, key: &MirMonomorphizationKey) -> MirShapeBindings {
        let mut shape_bindings = MirShapeBindings::default();
        let Some(params) = self.generic_params.get(&key.source) else {
            return shape_bindings;
        };
        let mut shape_args = key.shape_args.iter();
        for param in params {
            if param.is_shape
                && let Some(concrete) = shape_args.next().copied()
            {
                shape_bindings.insert(param.name, concrete);
            }
        }
        shape_bindings
    }

    fn ensure_instance(
        &mut self,
        template: &MirFunction,
        key: &MirMonomorphizationKey,
        type_bindings: &MirGenericBindings,
        span: Span,
    ) -> Option<MirFunctionId> {
        // DFV2-3 fail-closed gate: an imported device function with no usable
        // body channel (signature-only, or an incompatible artifact) is never
        // instantiated — the role bit alone does not validate the call.
        if let Some(route) = self.imported_devices.get(key.source)
            && !route.contract.is_composable()
        {
            self.errors.push(MirError {
                    message: format!(
                        "signature-only device import `{}` cannot be instantiated: no source body and no canonical body artifact",
                        route.display_key()
                    ),
                    issue: IMPORTED_DEVICE_BODY_MISSING.to_owned(),
                    span,
                    definition_span: None,
                });
            return None;
        }
        if let Some(existing) = self.instance_index.get(key).copied() {
            self.register_device_instance(key, existing);
            return Some(existing);
        }
        if self.created.len() >= MAX_PACKAGE_MONOMORPHIZATIONS {
            self.errors.push(MirError::validation(
                span,
                format!("generic monomorphization limit exceeded ({MAX_PACKAGE_MONOMORPHIZATIONS})"),
            ));
            return None;
        }
        let id = self.next_id;
        self.next_id = MirFunctionId(self.next_id.0 + 1);
        let mut instance = template.clone();
        instance.id = id;
        if !self.substitute_function(&mut instance, type_bindings, span) {
            return None;
        }
        if !self.resolve_tensor_limes_for_instance(key, &mut instance, span) {
            return None;
        }
        let shape_bindings = self.shape_bindings_from_key(key);
        if !finalize_function(self.types, &mut instance, type_bindings, &shape_bindings, &mut self.errors) {
            return None;
        }
        if super::monomorphize::function_contains_param(self.types, &instance) {
            self.errors.push(MirError::validation(
                instance.span,
                format!("function f{} contains unresolved generic type parameter", instance.id.0),
            ));
            return None;
        }
        self.instance_index.insert(key.clone(), id);
        self.monomorphizations
            .push(MirMonomorphization { key: key.clone(), function: id });
        self.created.push(instance);
        self.register_device_instance(key, id);
        Some(id)
    }

    fn resolve_tensor_limes_for_instance(
        &mut self,
        key: &MirMonomorphizationKey,
        function: &mut MirFunction,
        span: Span,
    ) -> bool {
        let sites = self
            .tensor_limes_templates
            .iter()
            .filter(|((source, _), _)| *source == key.source)
            .map(|(_, site)| *site)
            .collect::<Vec<_>>();
        for site in sites {
            let Some(actual_policy) = key.type_args.get(site.policy_type_argument).copied() else {
                self.errors.push(MirError::validation(
                    span,
                    "generic tensor limes policy argument is missing from its specialization key",
                ));
                return false;
            };
            let Some((element, rank)) = function
                .blocks
                .iter()
                .flat_map(|block| &block.statements)
                .find_map(|statement| match &statement.kind {
                    MirStatementKind::RuntimeCall { destination: Some(place), call }
                        if place.base == MirPlaceBase::Temp(site.destination)
                            && place.projections.is_empty()
                            && matches!(&call.intrinsic, MirIntrinsic::TensorLimes(_)) =>
                    {
                        tensor_type_element_and_rank(self.types, call.return_ty.semantic_id())
                    }
                    _ => None,
                })
            else {
                self.errors.push(MirError::validation(
                    span,
                    "generic tensor limes template call is missing after specialization",
                ));
                return false;
            };
            if rank != site.rank {
                self.errors.push(MirError::validation(
                    span,
                    "generic tensor limes tensor rank changed during specialization",
                ));
                return false;
            }
            let candidates = self
                .tensor_edge_policy_providers
                .iter()
                .filter(|provider| {
                    provider.rank == site.rank
                        && tensor_edge_provider_matches(
                            self.types,
                            self.interner,
                            actual_policy.semantic_id(),
                            element,
                            provider,
                        )
                })
                .cloned()
                .collect::<Vec<_>>();
            let [provider] = candidates.as_slice() else {
                if !self.require_tensor_edge_resolution && candidates.is_empty() {
                    return false;
                }
                self.errors.push(MirError::validation(
                    span,
                    "generic tensor limes specialization has no unique checked portable policy provider",
                ));
                return false;
            };
            let dispatch = match &provider.selector {
                MirTensorEdgePolicySelector::Builtin(policy) => MirEdgePolicy::Builtin(*policy),
                MirTensorEdgePolicySelector::Custom { policy_def, remap, transform } => {
                    let Some(remap) = self.function_ids_by_source.get(remap).copied() else {
                        if !self.require_tensor_edge_resolution {
                            return false;
                        }
                        self.errors.push(MirError::validation(
                            span,
                            "generic tensor limes remap callback is not linked into the package",
                        ));
                        return false;
                    };
                    let transform = match transform {
                        Some(definition) => match self.function_ids_by_source.get(definition).copied() {
                            Some(function) => Some(function),
                            None if !self.require_tensor_edge_resolution => return false,
                            None => {
                                self.errors.push(MirError::validation(
                                    span,
                                    "generic tensor limes transform callback is not linked into the package",
                                ));
                                return false;
                            }
                        },
                        None => None,
                    };
                    MirEdgePolicy::Custom { policy_def: *policy_def, remap, transform }
                }
            };
            let Some(statement) = function
                .blocks
                .iter_mut()
                .flat_map(|block| &mut block.statements)
                .find(|statement| {
                    matches!(
                        &statement.kind,
                        MirStatementKind::RuntimeCall {
                            destination: Some(place),
                            call,
                        } if place.base == MirPlaceBase::Temp(site.destination)
                            && place.projections.is_empty()
                            && matches!(&call.intrinsic, MirIntrinsic::TensorLimes(_))
                    )
                })
            else {
                self.errors.push(MirError::validation(
                    span,
                    "generic tensor limes template call changed during specialization",
                ));
                return false;
            };
            if let MirStatementKind::RuntimeCall { call, .. } = &mut statement.kind {
                call.intrinsic = MirIntrinsic::TensorLimes(dispatch);
            }
        }
        true
    }

    /// Register one concrete imported device instance for the consumer
    /// (DFV2-3 (b)). No-op for local (non-imported) templates; one
    /// registration per monomorphization key.
    fn register_device_instance(&mut self, key: &MirMonomorphizationKey, function: MirFunctionId) {
        let Some(route) = self.imported_devices.get(key.source).cloned() else {
            return;
        };
        if !self.registered_device_keys.insert(key.clone()) {
            return;
        }
        self.device_registrations
            .push(ImportedDeviceRegistration { key: key.clone(), function, route });
    }

    pub fn substitute_function(
        &mut self,
        function: &mut MirFunction,
        bindings: &MirGenericBindings,
        span: Span,
    ) -> bool {
        // Shape params of the generic being instantiated (GSF-1): forwarded
        // size witnesses in nested callee type args stay symbolic through
        // typus substitution.
        let shape_params: FxHashSet<radix_lexer::Symbol> = function
            .source
            .and_then(|source| self.generic_params.get(&source))
            .map(|params| {
                params
                    .iter()
                    .filter(|param| param.is_shape)
                    .map(|param| param.name)
                    .collect()
            })
            .unwrap_or_default();
        for param in &mut function.params {
            param.ty = match self.substitute_ty(param.ty, bindings, span) {
                Some(ty) => ty,
                None => return false,
            };
        }
        for local in &mut function.locals {
            local.ty = match self.substitute_ty(local.ty, bindings, span) {
                Some(ty) => ty,
                None => return false,
            };
        }
        for temp in &mut function.temps {
            temp.ty = match self.substitute_ty(temp.ty, bindings, span) {
                Some(ty) => ty,
                None => return false,
            };
        }
        function.return_ty = match self.substitute_ty(function.return_ty, bindings, span) {
            Some(ty) => ty,
            None => return false,
        };
        if let Some(error_ty) = function.error_ty {
            function.error_ty = match self.substitute_ty(error_ty, bindings, span) {
                Some(ty) => Some(ty),
                None => return false,
            };
        }
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                match &mut statement.kind {
                    MirStatementKind::Assign { value, .. } => {
                        value.ty = match self.substitute_ty(value.ty, bindings, span) {
                            Some(ty) => ty,
                            None => return false,
                        };
                    }
                    MirStatementKind::RuntimeCall { call, .. } => {
                        call.return_ty = match self.substitute_ty(call.return_ty, bindings, span) {
                            Some(ty) => ty,
                            None => return false,
                        };
                        // Conversions carry their own source/target payload
                        // types; leaving them pre-substitution fails the
                        // validator against the substituted operand (same
                        // rule as the unit monomorphizer).
                        if let radix_mir::MirIntrinsic::Convert(conversion) = &mut call.intrinsic {
                            conversion.source_ty = match self.substitute_ty(conversion.source_ty, bindings, span) {
                                Some(ty) => ty,
                                None => return false,
                            };
                            conversion.target_ty = match self.substitute_ty(conversion.target_ty, bindings, span) {
                                Some(ty) => ty,
                                None => return false,
                            };
                        }
                    }
                    MirStatementKind::Construct { aggregate, .. } => {
                        aggregate.ty = match self.substitute_ty(aggregate.ty, bindings, span) {
                            Some(ty) => ty,
                            None => return false,
                        };
                    }
                    MirStatementKind::Call { callee, .. } => {
                        if !self.substitute_callee(callee, bindings, &shape_params, span) {
                            return false;
                        }
                    }
                }
            }
            if let MirTerminatorKind::TryCall { callee, .. } = &mut block.terminator.kind
                && !self.substitute_callee(callee, bindings, &shape_params, span)
            {
                return false;
            }
        }
        true
    }

    fn substitute_callee(
        &mut self,
        callee: &mut MirCallee,
        bindings: &MirGenericBindings,
        shape_params: &FxHashSet<radix_lexer::Symbol>,
        span: Span,
    ) -> bool {
        let MirCallee::Definition { type_args, .. } = callee else {
            return true;
        };
        for type_arg in type_args {
            // GSF-1: a forwarded size witness stays a symbolic `Param` in
            // callee type args — the package rewrite resolves it through the
            // calling instance's shape bindings. Substituting it here would
            // reject the caller-owned magnitudo symbol as an unresolved type
            // parameter.
            if let Type::Param(sym) = self.types.get(type_arg.semantic_id())
                && shape_params.contains(sym)
                && !bindings.contains_key(sym)
            {
                continue;
            }
            *type_arg = match self.substitute_ty(*type_arg, bindings, span) {
                Some(ty) => ty,
                None => return false,
            };
        }
        true
    }

    fn substitute_ty(&mut self, ty: MirType, bindings: &MirGenericBindings, span: Span) -> Option<MirType> {
        if !type_contains_param(self.types, ty.semantic_id()) {
            return Some(ty);
        }
        match instantiate_existing_generic_type(self.types, ty, bindings, span) {
            Ok(ty) => Some(ty),
            Err(error) => {
                self.errors
                    .push(MirError::validation(error.span, error.message));
                None
            }
        }
    }

    fn apply_rewrite(
        &mut self,
        program: &mut MirProgram,
        function_index: &MirFunctionIndex,
        call: &PendingCall,
        target: MirFunctionId,
    ) {
        if let Some(function) = program.indexed_function_mut(function_index, call.function) {
            rewrite_call_callee(function, call, target);
            return;
        }
        if let Some(function) = self
            .created
            .iter_mut()
            .find(|function| function.id == call.function)
        {
            rewrite_call_callee(function, call, target);
        }
    }

    fn operand_ty(&mut self, operand: &MirOperand, scope: &FunctionTypeScope, span: Span) -> Option<MirType> {
        match operand {
            MirOperand::Place(place) => self.place_ty(place, scope, span),
            MirOperand::Temp(id) => scope.temps.get(id).copied(),
            MirOperand::Value(id) => scope.values.get(id).copied(),
            MirOperand::Constant(constant) => self.constant_ty(constant),
        }
    }

    fn place_ty(&mut self, place: &MirPlace, scope: &FunctionTypeScope, span: Span) -> Option<MirType> {
        let mut ty = match place.base {
            MirPlaceBase::Local(id) => scope.locals.get(&id).copied(),
            MirPlaceBase::Temp(id) => scope.temps.get(&id).copied(),
        }
        .or_else(|| {
            self.errors
                .push(MirError::validation(span, "undefined place in generic call"));
            None
        })?;
        for projection in &place.projections {
            ty = self.projection_ty(ty, projection, span)?;
        }
        Some(ty)
    }

    /// Field projections resolve through merged struct metadata (an applied
    /// nominal substitutes its size args into the field type). Other
    /// projection kinds carry no recoverable type in this pass; the caller
    /// skips witness binding from them.
    fn projection_ty(&mut self, base_ty: MirType, projection: &MirProjection, span: Span) -> Option<MirType> {
        match projection {
            MirProjection::Field(field) => self.field_projection_ty(base_ty, *field, span),
            _ => None,
        }
    }

    fn field_projection_ty(&mut self, base_ty: MirType, field: radix_lexer::Symbol, span: Span) -> Option<MirType> {
        let metadata = self.struct_metadata?;
        match self.types.get(base_ty.semantic_id()) {
            Type::Struct(def_id) => metadata
                .struct_fields
                .get(def_id)
                .and_then(|fields| fields.get(&field))
                .copied(),
            Type::Applied(base, args) => {
                let Type::Struct(def_id) = self.types.get(*base) else {
                    return None;
                };
                let field_ty = metadata
                    .struct_fields
                    .get(def_id)
                    .and_then(|fields| fields.get(&field))
                    .copied()?;
                let Some(params) = metadata.struct_type_params.get(def_id) else {
                    return Some(field_ty);
                };
                let mut type_bindings = MirGenericBindings::default();
                let mut shape_bindings = MirShapeBindings::default();
                for (param, arg) in params.iter().zip(args.iter()) {
                    match arg {
                        radix_types::AppliedArg::Type(witness) => {
                            type_bindings.insert(*param, MirType::semantic(*witness));
                        }
                        radix_types::AppliedArg::Size(index) => {
                            shape_bindings.insert(*param, *index);
                        }
                    }
                }
                instantiate_generic_type_with_shapes(self.types, field_ty, &type_bindings, &shape_bindings, span)
                    .map_err(|error| {
                        self.errors
                            .push(MirError::validation(error.span, error.message));
                    })
                    .ok()
            }
            Type::Alias(_, target) => self.field_projection_ty(MirType::semantic(*target), field, span),
            _ => None,
        }
    }

    fn constant_ty(&self, constant: &radix_mir::MirConstant) -> Option<MirType> {
        use radix_mir::MirConstant;
        let primitive = match constant {
            MirConstant::Int(_) => Primitive::Numerus,
            // An unsigned constant is the sized `u64`, as the lowerer and the
            // single-unit monomorphizer type it; plain `numerus` is `i64`.
            MirConstant::UInt(_) => {
                return Some(MirType::semantic(
                    self.types
                        .find_sized_numeric(Primitive::Numerus, radix_semantic::NumericWidth::U64)
                        .unwrap_or_else(|| self.types.primitive(Primitive::Numerus)),
                ));
            }
            // An exact bignum constant is the unbounded integer.
            MirConstant::Magnus(_) => {
                return Some(MirType::semantic(
                    self.types
                        .find_sized_numeric(Primitive::Numerus, radix_semantic::NumericWidth::Inf)
                        .unwrap_or_else(|| self.types.primitive(Primitive::Numerus)),
                ));
            }
            MirConstant::Float(_) | MirConstant::ShapeSize { .. } => Primitive::Fractus,
            MirConstant::Bool(_) => Primitive::Bivalens,
            MirConstant::Unit => Primitive::Vacuum,
            MirConstant::String(_) => Primitive::Textus,
            MirConstant::Ascii(_) => Primitive::Ascii,
            MirConstant::Nil => Primitive::Nihil,
            MirConstant::Octeti(_) => Primitive::Octeti,
            MirConstant::Regex { .. } => Primitive::Regex,
            MirConstant::Function(_) => return None,
        };
        Some(MirType::semantic(self.types.primitive(primitive)))
    }

    fn format_ty(&self, ty: MirType) -> String {
        radix_semantic::format_faber_type(ty.semantic_id(), self.types, self.interner, &|def_id| {
            format!("def{}", def_id.0)
        })
    }

    fn finalize_new_instances(&mut self, program: &mut MirProgram, new_ids: &FxHashSet<MirFunctionId>) {
        let function_index = program.function_index();
        let to_finalize: Vec<(MirFunctionId, MirMonomorphizationKey)> = self
            .monomorphizations
            .iter()
            .filter(|monomorphization| new_ids.contains(&monomorphization.function))
            .map(|monomorphization| (monomorphization.function, monomorphization.key.clone()))
            .collect();
        for (function_id, key) in to_finalize {
            let shape_bindings = self.shape_bindings_from_key(&key);
            let Some(function) = program.indexed_function_mut(&function_index, function_id) else {
                continue;
            };
            let type_bindings = MirGenericBindings::default();
            if !finalize_function(self.types, function, &type_bindings, &shape_bindings, &mut self.errors) {
                continue;
            }
            if super::monomorphize::function_contains_param(self.types, function) {
                self.errors.push(MirError::validation(
                    function.span,
                    format!("function f{} contains unresolved generic type parameter", function.id.0),
                ));
            }
        }
        for function in &mut program.functions {
            if new_ids.contains(&function.id) && function_contains_shape_size(function) {
                self.errors
                    .push(MirError::validation(function.span, UNRESOLVED_SHAPE_SIZE_SURVIVOR));
            }
        }
    }
}

fn tensor_type_element_and_rank(types: &TypeTable, mut ty: TypeId) -> Option<(TypeId, usize)> {
    let mut seen = FxHashSet::default();
    while let Type::Alias(_, inner) = types.get(ty) {
        if !seen.insert(ty) {
            return None;
        }
        ty = *inner;
    }
    let Type::Tensor(element, shape) = types.get(ty) else {
        return None;
    };
    let rank = match types.get_index(*shape) {
        IndexExpr::Tuple(dimensions) => dimensions.len(),
        IndexExpr::Literal(_)
        | IndexExpr::Param(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Affine { .. }
        | IndexExpr::Divide { .. } => 1,
        IndexExpr::Infer(_) | IndexExpr::Unspecified => return None,
    };
    Some((*element, rank))
}

/// Whether every runtime extent in an index expression names a *formal*
/// function-parameter axis (the caller's own parameter slot), as opposed to a
/// caller-local binding extent. Nested size-generic calls resolved through the
/// calling instance's shape bindings land here; top-level calls with
/// runtime-shaped arguments (whose extents are local bindings guarded by a
/// `TensorShapeCheck` at the call site) do not.
fn runtime_extents_are_caller_formal_axes(types: &TypeTable, index: IndexId, seen: &mut FxHashSet<IndexId>) -> bool {
    if !seen.insert(index) {
        return true;
    }
    match types.get_index(index) {
        IndexExpr::Runtime(
            RuntimeExtentId::FunctionParameterAxis { .. } | RuntimeExtentId::FunctionParameterFieldAxis { .. },
        ) => true,
        IndexExpr::Runtime(_) => false,
        IndexExpr::Affine { base, .. } => runtime_extents_are_caller_formal_axes(types, *base, seen),
        IndexExpr::Divide { numerator, .. } => runtime_extents_are_caller_formal_axes(types, *numerator, seen),
        IndexExpr::Tuple(dimensions) => dimensions
            .iter()
            .all(|dimension| runtime_extents_are_caller_formal_axes(types, *dimension, seen)),
        IndexExpr::Literal(_) | IndexExpr::Param(_) | IndexExpr::Infer(_) | IndexExpr::Unspecified => true,
    }
}

fn tensor_edge_provider_matches(
    types: &TypeTable,
    interner: &Interner,
    mut actual_policy: TypeId,
    element_type: TypeId,
    provider: &MirTensorEdgePolicyProvider,
) -> bool {
    let mut seen = FxHashSet::default();
    while let Type::Alias(_, inner) = types.get(actual_policy) {
        if !seen.insert(actual_policy) {
            return false;
        }
        actual_policy = *inner;
    }
    match &provider.selector {
        MirTensorEdgePolicySelector::Builtin(policy) => {
            types.tensor_edge_policy_kind(actual_policy)
                == Some(match *policy {
                    MirBuiltinEdgePolicy::Clamp => radix_types::BuiltinTensorEdgePolicy::Clamp,
                    MirBuiltinEdgePolicy::Reflect => radix_types::BuiltinTensorEdgePolicy::Reflect,
                    MirBuiltinEdgePolicy::Wrap => radix_types::BuiltinTensorEdgePolicy::Wrap,
                })
        }
        MirTensorEdgePolicySelector::Custom { .. } => {
            super::portable_tensor_edge_type_identity(types, interner, actual_policy)
                .as_ref()
                .zip(provider.policy_type.as_ref())
                .is_some_and(|(actual, expected)| actual == expected)
                && super::portable_tensor_edge_type_identity(types, interner, element_type)
                    .as_ref()
                    .zip(provider.element_type.as_ref())
                    .is_some_and(|(actual, expected)| actual == expected)
        }
    }
}

fn rewrite_call_callee(function: &mut MirFunction, call: &PendingCall, target: MirFunctionId) {
    let Some(block) = function.blocks.get_mut(call.block_index) else {
        return;
    };
    if let Some(statement_index) = call.statement_index {
        if let Some(statement) = block.statements.get_mut(statement_index)
            && let MirStatementKind::Call { callee, .. } = &mut statement.kind
        {
            *callee = MirCallee::Function(target);
        }
        return;
    }
    if let MirTerminatorKind::TryCall { callee, .. } = &mut block.terminator.kind {
        *callee = MirCallee::Function(target);
    }
}

/// Repair AIR-generated `ShapeSize` indices that cross a package type-table
/// merge. Ordinary type ids are remapped at the package seam, but a
/// `ShapeSize` carries an `IndexId` directly and therefore cannot be repaired
/// by the type-id rewrite. The reverse-AD mean producer emits the divisor as
/// the statement immediately after its `TensorMean`; that call's tensor
/// operand has the authoritative, already-finalized result shape.
fn repair_mean_shape_size_constants(types: &TypeTable, function: &mut MirFunction) {
    let snapshot = function.clone();
    for block in &mut function.blocks {
        for index in 1..block.statements.len() {
            let Some(mean_shape) = (|| {
                let MirStatementKind::RuntimeCall { call, .. } = &block.statements[index - 1].kind else {
                    return None;
                };
                if !matches!(
                    call.intrinsic,
                    radix_mir::MirIntrinsic::Collection(radix_mir::MirCollectionOp::TensorMean)
                ) {
                    return None;
                }
                tensor_shape_for_operand(types, &snapshot, call.args.first()?)
            })() else {
                continue;
            };
            if block.statements[index].span != block.statements[index - 1].span {
                continue;
            }
            let MirStatementKind::Assign { value, .. } = &mut block.statements[index].kind else {
                continue;
            };
            let radix_mir::MirValueKind::Operand(MirOperand::Constant(radix_mir::MirConstant::ShapeSize {
                shape, ..
            })) = &mut value.kind
            else {
                continue;
            };
            *shape = mean_shape;
        }
    }
}

fn tensor_shape_for_operand(
    types: &TypeTable,
    function: &MirFunction,
    operand: &MirOperand,
) -> Option<radix_semantic::IndexId> {
    let ty = match operand {
        MirOperand::Place(place) if place.projections.is_empty() => match place.base {
            MirPlaceBase::Local(id) => function
                .params
                .iter()
                .find(|param| param.local == id)
                .map(|param| param.ty)
                .or_else(|| {
                    function
                        .locals
                        .iter()
                        .find(|local| local.id == id)
                        .map(|local| local.ty)
                }),
            MirPlaceBase::Temp(id) => function
                .temps
                .iter()
                .find(|temp| temp.id == id)
                .map(|temp| temp.ty),
        },
        MirOperand::Temp(id) => function
            .temps
            .iter()
            .find(|temp| temp.id == *id)
            .map(|temp| temp.ty),
        MirOperand::Value(_) | MirOperand::Constant(_) | MirOperand::Place(_) => None,
    }?;
    let mut current = ty.semantic_id();
    let mut seen = FxHashSet::default();
    loop {
        if !seen.insert(current) {
            return None;
        }
        match types.get(current) {
            Type::Tensor(_, shape) => return Some(*shape),
            Type::Alias(_, target) => current = *target,
            _ => return None,
        }
    }
}

fn finalize_function(
    types: &mut TypeTable,
    function: &mut MirFunction,
    type_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    errors: &mut Vec<MirError>,
) -> bool {
    let span = function.span;
    for param in &mut function.params {
        param.ty = match finalize_ty(types, param.ty, type_bindings, shape_bindings, span, errors) {
            Some(ty) => ty,
            None => return false,
        };
    }
    for local in &mut function.locals {
        local.ty = match finalize_ty(types, local.ty, type_bindings, shape_bindings, span, errors) {
            Some(ty) => ty,
            None => return false,
        };
    }
    for temp in &mut function.temps {
        temp.ty = match finalize_ty(types, temp.ty, type_bindings, shape_bindings, span, errors) {
            Some(ty) => ty,
            None => return false,
        };
    }
    function.return_ty = match finalize_ty(types, function.return_ty, type_bindings, shape_bindings, span, errors) {
        Some(ty) => ty,
        None => return false,
    };
    if let Some(error_ty) = function.error_ty {
        function.error_ty = match finalize_ty(types, error_ty, type_bindings, shape_bindings, span, errors) {
            Some(ty) => Some(ty),
            None => return false,
        };
    }
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            match &mut statement.kind {
                MirStatementKind::Assign { value, .. } => {
                    value.ty = match finalize_ty(types, value.ty, type_bindings, shape_bindings, span, errors) {
                        Some(ty) => ty,
                        None => return false,
                    };
                }
                MirStatementKind::RuntimeCall { call, .. } => {
                    call.return_ty =
                        match finalize_ty(types, call.return_ty, type_bindings, shape_bindings, span, errors) {
                            Some(ty) => ty,
                            None => return false,
                        };
                    // Conversions carry their own source/target payload
                    // types; finalize them with the same shape bindings as
                    // the operands (unit `finalize_statement_shapes` parity).
                    if let radix_mir::MirIntrinsic::Convert(conversion) = &mut call.intrinsic {
                        conversion.source_ty =
                            match finalize_ty(types, conversion.source_ty, type_bindings, shape_bindings, span, errors)
                            {
                                Some(ty) => ty,
                                None => return false,
                            };
                        conversion.target_ty =
                            match finalize_ty(types, conversion.target_ty, type_bindings, shape_bindings, span, errors)
                            {
                                Some(ty) => ty,
                                None => return false,
                            };
                    }
                }
                MirStatementKind::Construct { aggregate, .. } => {
                    aggregate.ty = match finalize_ty(types, aggregate.ty, type_bindings, shape_bindings, span, errors) {
                        Some(ty) => ty,
                        None => return false,
                    };
                }
                MirStatementKind::Call { callee, .. } => {
                    if !finalize_callee(types, callee, type_bindings, shape_bindings, span, errors) {
                        return false;
                    }
                }
            }
        }
        if let MirTerminatorKind::TryCall { callee, .. } = &mut block.terminator.kind
            && !finalize_callee(types, callee, type_bindings, shape_bindings, span, errors)
        {
            return false;
        }
    }
    repair_mean_shape_size_constants(types, function);
    let _ = substitute_tensor_from_flat_shape_size_constants(types, function, shape_bindings);
    let _ = substitute_shape_size_constants(types, function, shape_bindings);
    super::monomorphize::fold_static_shape_checks(types, function);
    super::expand_deferred_vector_ops(types, function, errors);
    true
}

fn finalize_callee(
    types: &mut TypeTable,
    callee: &mut MirCallee,
    type_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    span: Span,
    errors: &mut Vec<MirError>,
) -> bool {
    let MirCallee::Definition { type_args, .. } = callee else {
        return true;
    };
    for type_arg in type_args {
        // GSF-1: a forwarded size witness is a symbolic `Param` owned by the
        // caller's shape bindings — the package rewrite resolves it against
        // the calling instance, so finalization must leave it symbolic
        // rather than reject it as an unresolved type parameter.
        if let Type::Param(sym) = types.get(type_arg.semantic_id())
            && shape_bindings.contains_key(sym)
            && !type_bindings.contains_key(sym)
        {
            continue;
        }
        *type_arg = match finalize_ty(types, *type_arg, type_bindings, shape_bindings, span, errors) {
            Some(ty) => ty,
            None => return false,
        };
    }
    true
}

fn finalize_ty(
    types: &mut TypeTable,
    ty: MirType,
    type_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    span: Span,
    errors: &mut Vec<MirError>,
) -> Option<MirType> {
    match instantiate_generic_type_with_shapes(types, ty, type_bindings, shape_bindings, span) {
        Ok(ty) => Some(ty),
        Err(error) => {
            errors.push(MirError::validation(error.span, error.message));
            None
        }
    }
}

fn find_literal_index(types: &TypeTable, value: u64) -> Option<radix_semantic::IndexId> {
    (0..types.index_count()).find_map(|index| {
        #[expect(clippy::cast_possible_truncation, reason = "IndexId is a u32 arena index by design")]
        let id = radix_semantic::IndexId(index as u32);
        matches!(types.get_index(id), IndexExpr::Literal(found) if *found == value).then_some(id)
    })
}
