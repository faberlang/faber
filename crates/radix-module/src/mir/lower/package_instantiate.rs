//! Package-link instantiation of leftover generic `Definition` callees.
//!
//! Unit lowering keys instances by [`MirMonomorphizationKey`] `(source,
//! type_args, shape_args)` and then drops the generic template. Package
//! linking previously re-identified every instance by `function.source`
//! alone, so `_sgd_family<[2,2]>` and `_sgd_family<[8,8]>` collapsed and
//! the caller's arguments were checked against the first signature. This
//! pass keeps the templates, rebuilds the shape-discriminating index, and
//! instantiates any shape the defining unit never called.

use super::{MirError, MirGenericParam};
use crate::file_interface::{
    FileExportKind, FileInterface, InterfaceDeviceContract, InterfaceDeviceRole, InterfaceDeviceVisibility,
};
use crate::hir::DefId;
use crate::lexer::{Interner, Span};
use crate::mir::generic::{
    bind_generic_type_witness_with_shapes, canonical_index, function_contains_shape_size,
    instantiate_existing_generic_type, instantiate_generic_type_with_shapes, substitute_shape_size_constants,
    MirShapeBindings,
};
use crate::mir::{
    type_contains_param, MirCallee, MirExternalFunctionIdentity, MirFunction, MirFunctionId, MirGenericBindings,
    MirMonomorphization, MirMonomorphizationKey, MirOperand, MirPlace, MirPlaceBase, MirProgram, MirProjection,
    MirStatementKind, MirTerminatorKind, MirType,
};
use crate::semantic::{IndexExpr, Primitive, Type, TypeTable};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::HashMap;

const MAX_PACKAGE_MONOMORPHIZATIONS: usize = 64;
const UNRESOLVED_SHAPE_SIZE_SURVIVOR: &str = "unresolved shape-size constant survives monomorphization";
/// Named fail-closed reason for a signature-only imported device import
/// (DFV2-3 (e)): the role fact alone never admits the instance.
const IMPORTED_DEVICE_BODY_MISSING: &str = "imported_device_body_missing";
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
    pub struct_fields: &'a HashMap<DefId, HashMap<crate::lexer::Symbol, MirType>>,
    pub struct_type_params: &'a HashMap<DefId, Vec<crate::lexer::Symbol>>,
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
    consumer_roles: &HashMap<DefId, crate::mir::MirDeviceRole>,
    consumer_launch_entries: &std::collections::HashSet<DefId>,
) -> Result<(Vec<MirMonomorphization>, Vec<ImportedDeviceRegistration>), Vec<MirError>> {
    if templates.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut state = PackageInstantiate {
        types,
        interner,
        struct_metadata,
        templates: templates
            .iter()
            .filter_map(|template| template.source.map(|source| (source, template.clone())))
            .collect(),
        generic_params: generic_params.iter().cloned().collect(),
        instance_index: FxHashMap::default(),
        monomorphizations: Vec::new(),
        next_id: next_function_id(&program.functions),
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
        .collect::<FxHashMap<DefId, crate::mir::MirDeviceRole>>();
    for (source, route) in imported_devices.iter() {
        let role = mir_role(route.contract.role);
        if let Some(consumer) = device_roles.insert(*source, role) {
            if consumer != role {
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
                        && mir_role(route.contract.role) == crate::mir::MirDeviceRole::Kernel
                })
                .map(|(source, _)| *source),
        );
        let composition_errors = crate::mir::fragment_composition::compose(
            &mut program.functions,
            &state.monomorphizations,
            &device_roles,
            crate::mir::fragment_composition::active_budget(),
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
fn mir_role(role: InterfaceDeviceRole) -> crate::mir::MirDeviceRole {
    match role {
        InterfaceDeviceRole::Kernel => crate::mir::MirDeviceRole::Kernel,
        InterfaceDeviceRole::Fragment => crate::mir::MirDeviceRole::Fragment,
    }
}

fn next_function_id(functions: &[MirFunction]) -> MirFunctionId {
    functions
        .iter()
        .map(|function| function.id.0)
        .max()
        .map_or(MirFunctionId(0), |id| MirFunctionId(id + 1))
}

struct PackageInstantiate<'a> {
    types: &'a mut TypeTable,
    interner: &'a Interner,
    struct_metadata: Option<&'a PackageStructMetadata<'a>>,
    templates: FxHashMap<DefId, MirFunction>,
    generic_params: FxHashMap<DefId, Vec<MirGenericParam>>,
    instance_index: FxHashMap<MirMonomorphizationKey, MirFunctionId>,
    monomorphizations: Vec<MirMonomorphization>,
    next_id: MirFunctionId,
    errors: Vec<MirError>,
    created: Vec<MirFunction>,
    /// Imported device functions indexed by template source (DFV2-3).
    imported_devices: ImportedDeviceIndex,
    /// Concrete imported device instances registered for the consumer.
    device_registrations: Vec<ImportedDeviceRegistration>,
    /// Registration dedupe guard: one registration per monomorphization key.
    registered_device_keys: FxHashSet<MirMonomorphizationKey>,
}

#[derive(Clone)]
struct FunctionTypeScope {
    locals: FxHashMap<crate::mir::MirLocalId, MirType>,
    temps: FxHashMap<crate::mir::MirTempId, MirType>,
    values: FxHashMap<crate::mir::MirValueId, MirType>,
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
    source: DefId,
    type_args: Vec<MirType>,
    args: Vec<MirOperand>,
    span: Span,
    scope: FunctionTypeScope,
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
        let _ = bind_generic_type_witness_with_shapes(
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
                let Some(function) = super::monomorphize::function_by_id(&program.functions, function_id)
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
                    self.apply_rewrite(program, &call, target);
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
                if let MirStatementKind::Call {
                    destination,
                    callee: MirCallee::Definition { source, type_args },
                    args,
                } = &statement.kind
                {
                    if self.templates.contains_key(source) {
                        calls.push(PendingCall {
                            function: function.id,
                            block_index,
                            statement_index: Some(statement_index),
                            destination: destination.clone(),
                            source: *source,
                            type_args: type_args.clone(),
                            args: args.clone(),
                            span: statement.span,
                            scope: scope.clone(),
                        });
                    }
                }
            }
            if let MirTerminatorKind::TryCall {
                destination,
                callee: MirCallee::Definition { source, type_args },
                args,
                ..
            } = &block.terminator.kind
            {
                if self.templates.contains_key(source) {
                    calls.push(PendingCall {
                        function: function.id,
                        block_index,
                        statement_index: None,
                        destination: destination.clone(),
                        source: *source,
                        type_args: type_args.clone(),
                        args: args.clone(),
                        span: block.terminator.span,
                        scope: scope.clone(),
                    });
                }
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
        if let Some(destination) = &call.destination {
            if let Some(actual) = self.place_ty(destination, &call.scope, call.span) {
                if !type_contains_param(self.types, actual.semantic_id())
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
            }
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
                let Some(index) = self.explicit_shape_witness(concrete, parent_shapes, span) else {
                    self.errors.push(MirError::validation(
                        span,
                        "generic call type argument does not match parameter kind",
                    ));
                    return false;
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

    fn magnitudo_literal_of(&self, ty: MirType) -> Option<u64> {
        let Type::Param(sym) = self.types.get(ty.semantic_id()) else {
            return None;
        };
        radix_types::parse_natural_capacity(self.interner.resolve(*sym))
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
    ) -> Option<crate::semantic::IndexId> {
        let Type::Param(sym) = self.types.get(concrete.semantic_id()) else {
            return None;
        };
        let sym = *sym;
        if let Some(value) = self.magnitudo_literal_of(concrete) {
            let index = find_literal_index(self.types, value);
            if index.is_none() {
                self.errors.push(MirError::validation(
                    span,
                    "generic call signature has unresolved type parameter",
                ));
            }
            return index;
        }
        let index = parent_shapes.get(&sym).copied();
        if index.is_none() {
            self.errors.push(MirError::validation(
                span,
                "generic call type argument does not match parameter kind",
            ));
        }
        index
    }

    fn key_from_bindings(
        &mut self,
        source: DefId,
        params: &[MirGenericParam],
        type_bindings: &MirGenericBindings,
        shape_bindings: &MirShapeBindings,
        parent_shapes: &MirShapeBindings,
        span: Span,
    ) -> Option<MirMonomorphizationKey> {
        let mut type_args = Vec::new();
        let mut shape_args = Vec::new();
        for param in params {
            if param.is_shape {
                let Some(bound) = shape_bindings.get(&param.name).copied() else {
                    self.errors.push(MirError::validation(
                        span,
                        "generic call signature has unresolved type parameter",
                    ));
                    return None;
                };
                let Some(concrete) =
                    super::monomorphize::resolve_parent_shape_binding(self.types, bound, parent_shapes)
                else {
                    self.errors.push(MirError::validation(
                        span,
                        "generic call signature has unresolved type parameter",
                    ));
                    return None;
                };
                shape_args.push(canonical_index(self.types, concrete));
            } else {
                let Some(bound) = type_bindings.get(&param.name).copied() else {
                    self.errors.push(MirError::validation(
                        span,
                        "generic call signature has unresolved type parameter",
                    ));
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
            if param.is_shape {
                if let Some(concrete) = shape_args.next().copied() {
                    shape_bindings.insert(param.name, concrete);
                }
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
        if let Some(route) = self.imported_devices.get(key.source) {
            if !route.contract.is_composable() {
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

    fn substitute_function(&mut self, function: &mut MirFunction, bindings: &MirGenericBindings, span: Span) -> bool {
        // Shape params of the generic being instantiated (GSF-1): forwarded
        // size witnesses in nested callee type args stay symbolic through
        // typus substitution.
        let shape_params: FxHashSet<crate::lexer::Symbol> = function
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
                        if let crate::mir::MirIntrinsic::Convert(conversion) = &mut call.intrinsic {
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
            if let MirTerminatorKind::TryCall { callee, .. } = &mut block.terminator.kind {
                if !self.substitute_callee(callee, bindings, &shape_params, span) {
                    return false;
                }
            }
        }
        true
    }

    fn substitute_callee(
        &mut self,
        callee: &mut MirCallee,
        bindings: &MirGenericBindings,
        shape_params: &FxHashSet<crate::lexer::Symbol>,
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
            if let Type::Param(sym) = self.types.get(type_arg.semantic_id()) {
                if shape_params.contains(sym) && !bindings.contains_key(sym) {
                    continue;
                }
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

    fn apply_rewrite(&mut self, program: &mut MirProgram, call: &PendingCall, target: MirFunctionId) {
        if let Some(function) = super::monomorphize::function_by_id_mut(&mut program.functions, call.function) {
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
            MirOperand::Constant(constant) => self.constant_ty(constant, span),
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

    fn field_projection_ty(&mut self, base_ty: MirType, field: crate::lexer::Symbol, span: Span) -> Option<MirType> {
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

    fn constant_ty(&self, constant: &crate::mir::MirConstant, _span: Span) -> Option<MirType> {
        use crate::mir::MirConstant;
        let primitive = match constant {
            MirConstant::Int(_) | MirConstant::UInt(_) => Primitive::Numerus,
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
        crate::semantic::format_faber_type(ty.semantic_id(), self.types, self.interner, &|def_id| {
            format!("def{}", def_id.0)
        })
    }

    fn finalize_new_instances(&mut self, program: &mut MirProgram, new_ids: &FxHashSet<MirFunctionId>) {
        let to_finalize: Vec<(MirFunctionId, MirMonomorphizationKey)> = self
            .monomorphizations
            .iter()
            .filter(|monomorphization| new_ids.contains(&monomorphization.function))
            .map(|monomorphization| (monomorphization.function, monomorphization.key.clone()))
            .collect();
        for (function_id, key) in to_finalize {
            let shape_bindings = self.shape_bindings_from_key(&key);
            let Some(function) = super::monomorphize::function_by_id_mut(&mut program.functions, function_id) else {
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

fn rewrite_call_callee(function: &mut MirFunction, call: &PendingCall, target: MirFunctionId) {
    let Some(block) = function.blocks.get_mut(call.block_index) else {
        return;
    };
    if let Some(statement_index) = call.statement_index {
        if let Some(MirStatementKind::Call { callee, .. }) = block
            .statements
            .get_mut(statement_index)
            .map(|statement| &mut statement.kind)
        {
            *callee = MirCallee::Function(target);
        }
        return;
    }
    if let MirTerminatorKind::TryCall { callee, .. } = &mut block.terminator.kind {
        *callee = MirCallee::Function(target);
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
                    if let crate::mir::MirIntrinsic::Convert(conversion) = &mut call.intrinsic {
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
        if let MirTerminatorKind::TryCall { callee, .. } = &mut block.terminator.kind {
            if !finalize_callee(types, callee, type_bindings, shape_bindings, span, errors) {
                return false;
            }
        }
    }
    let _ = substitute_shape_size_constants(types, function, shape_bindings);
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
        if let Type::Param(sym) = types.get(type_arg.semantic_id()) {
            if shape_bindings.contains_key(sym) && !type_bindings.contains_key(sym) {
                continue;
            }
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

#[cfg(test)]
#[path = "package_instantiate_test.rs"]
mod tests;

fn find_literal_index(types: &TypeTable, value: u64) -> Option<crate::semantic::IndexId> {
    (0..types.index_count()).find_map(|index| {
        #[expect(clippy::cast_possible_truncation, reason = "IndexId is a u32 arena index by design")]
        let id = crate::semantic::IndexId(index as u32);
        matches!(types.get_index(id), IndexExpr::Literal(found) if *found == value).then_some(id)
    })
}
