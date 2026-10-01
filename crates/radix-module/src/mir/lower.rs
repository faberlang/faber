//! HIR-to-MIR lowering orchestration for the compiler-developer MIR surface.
//!
//! This module is the boundary between a fully analyzed HIR unit and the
//! execution-shaped MIR model used by MIR dumps, validation, and temporary MIR
//! probes. It deliberately depends on [`AnalyzedModule`] rather than raw HIR so
//! lowering can preserve semantic identity, resolved definitions, inferred
//! types, entry metadata, and the interner without rebuilding semantic context.
//!
//! ERROR STRATEGY
//! ==============
//! MIR lowering is fail-closed infrastructure. Unsupported HIR surfaces, missing
//! semantic types, sealed-block misuse, and post-lowering validation failures are
//! reported as [`MirError`] values; they are not guessed around in order to emit
//! partial MIR. This keeps developer-facing MIR inspection honest while the MIR
//! subset grows toward the full language.
//!
//! ORCHESTRATION
//! =============
//! [`MirLowerer`] owns whole-program order: collect context maps, lower
//! supported top-level items, synthesize the primitive entry function when
//! allowed, and hand the complete program to validation. [`FunctionBuilder`]
//! owns one function body at a time, including local bindings, temporaries,
//! open basic blocks, loop/handler stacks, and value numbering.
//!
//! INVARIANTS
//! ==========
//! - Every emitted block is finalized with exactly one terminator before the
//!   program reaches validation.
//! - MIR types are copied from semantic `TypeId`s; missing HIR type information
//!   is a lowering error, not a codegen fallback.
//! - Unsupported features remain explicit diagnostics so later MIR work can add
//!   support without inheriting silent behavior.

use crate::cli::{CliDefault, CliOperand, CliOption, CliType};
use crate::driver::AnalyzedModule;
use crate::hir::visit::{HirVisitor, walk_expr, walk_program};
use crate::hir::{
    DefId, HirAnnotationValue, HirArrayElement, HirBinOp, HirBlock, HirCape, HirCasuArm, HirConst, HirConversioTarget,
    HirExpression, HirExpressionKind, HirExtremumOp, HirField, HirFunction, HirItem, HirItemKind, HirIteraMode,
    HirLiteral, HirLocal, HirMethod, HirNonNullKind, HirObjectField, HirObjectKey, HirOptionalChainKind, HirParamMode,
    HirPattern, HirRangeKind, HirScribeKind, HirStatement, HirStatementKind, HirStruct, HirTypeParam, HirUnOp,
};
use crate::lexer::{Interner, Span, Symbol};
use crate::mir::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBinOp, MirBlock, MirBlockId, MirCallee,
    MirCaptureMode, MirClosureCallee, MirClosureCapture, MirClosureEnvironment, MirClosureEnvironmentId,
    MirClosureValue, MirConstant, MirConversion, MirConversionFlavor, MirDeviceRole, MirDiagnosticKind,
    MirFilumCoordinate, MirFilumSumOp, MirFunction, MirFunctionId, MirFunctionOrigin, MirFunctionVisibility,
    MirGenericBindings, MirIntrinsic, MirKernelShaderStage, MirKeyValueOperand, MirLayoutTable, MirLiftOp,
    MirLocal as MirLocalDecl, MirLocalId, MirMonomorphization, MirNamedOperand, MirOperand, MirOptionChainLink,
    MirOptionOp, MirOptionUnwrapMode, MirParam, MirParamMode, MirPlace, MirPlaceBase, MirProgram, MirProjection,
    MirProvider, MirProviderKind, MirRuntimeCall, MirStatement, MirStatementKind, MirStructFieldDefaults,
    MirSwitchCase, MirSyntheticFunctionIdentity, MirTemp, MirTempId, MirTerminator, MirTerminatorKind, MirType,
    MirTypeCheckOp, MirUnOp, MirValidationContext, MirValue, MirValueId, MirValueKind, ValidatedMir, dump_program,
    instantiate_existing_generic_type, type_contains_param,
};
use crate::semantic::{ConversionPair, IndexExpr, IndexId, NumericWidth, Primitive, Type, TypeId, TypeTable};
use radix_mir::device::{MirDeviceFunctionMetadata, MirDeviceFunctionMetadataMap, is_public_entry};
use radix_mir::validate::PatchProgramError;
use radix_types::{MirCollectionOp, MirKernelBuiltin, SemanticMutability};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

mod aggregate;
mod async_surface;
mod callable;
mod collection_higher_order;

mod context;
mod control;
mod conversio_route;
mod expr;
#[cfg(test)]
#[path = "lower/first_match_test.rs"]
mod first_match_tests;
mod fractus_potentia;
mod item;
mod monomorphize;
mod package_instantiate;
mod place;
mod runtime;
mod stmt;
mod tensor_lift;
mod union_dispatch;

use context::{LoweringContextMaps, function_param_map, struct_field_map};
use expr::HirExpressionLoweringVisitor;
use item::ItemLoweringPass;
pub use package_instantiate::{
    ImportedDeviceIndex, ImportedDeviceRegistration, ImportedDeviceRoute, PackageStructMetadata,
    instantiate_merged_generic_calls, instantiate_merged_generic_calls_with_devices,
    instantiate_merged_generic_calls_with_devices_and_metadata,
};

/// Developer-facing MIR lowering diagnostic.
///
/// MIR lowering errors describe gaps between the typed HIR surface and the MIR
/// subset currently implemented. They carry the primary span and, for
/// composition diagnostics, an optional secondary definition span. The caller
/// is a compiler inspection/probe surface, not the primary user diagnostic
/// renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirError {
    pub message: String,
    pub issue: String,
    pub span: Span,
    pub definition_span: Option<Span>,
}

impl MirError {
    fn unsupported(span: Span, what: impl Into<String>) -> Self {
        let what = what.into();
        Self {
            message: format!("unsupported MIR lowering: {what}"),
            issue: mir_lower_issue("unsupported_mir_lowering", &what),
            span,
            definition_span: None,
        }
    }

    fn missing_type(span: Span, what: impl Into<String>) -> Self {
        let what = what.into();
        Self {
            message: format!("missing type information for MIR lowering: {what}"),
            issue: mir_lower_issue("missing_type_information_for_mir_lowering", &what),
            span,
            definition_span: None,
        }
    }

    fn validation(span: Span, what: impl Into<String>) -> Self {
        let what = what.into();
        Self {
            message: format!("invalid MIR: {what}"),
            issue: mir_lower_issue("invalid_mir", &what),
            span,
            definition_span: None,
        }
    }

    fn missing_generic_air_template(span: Span, source: u32) -> Self {
        Self {
            message: format!("generic AIR definition #{source} has no analyzed template"),
            issue: "generic_air_template_missing".to_owned(),
            span,
            definition_span: None,
        }
    }
}

fn mir_lower_issue(prefix: &str, detail: &str) -> String {
    let mut issue = prefix.to_owned();
    let mut separated = false;
    for ch in detail.chars() {
        if ch.is_ascii_alphanumeric() {
            if !separated {
                issue.push('_');
            }
            issue.push(ch.to_ascii_lowercase());
            separated = true;
        } else if !issue.ends_with('_') {
            issue.push('_');
            separated = true;
        }
    }
    issue.trim_end_matches('_').to_owned()
}

/// MIR output plus the proving validated-MIR token and auxiliary lowering data.
pub struct LoweredMirUnit<'a> {
    /// Complete symbol table after lowering-added runtime and diagnostic symbols.
    pub interner: Interner,
    pub monomorphizations: Vec<MirMonomorphization>,
    pub closure_environments: Vec<MirClosureEnvironment>,
    /// Explicit role/origin/visibility facts for source-derived and source-less
    /// device functions. Consumers must use this carrier rather than infer
    /// private status from a missing source definition or compute stage alone.
    pub device_function_metadata: MirDeviceFunctionMetadataMap,
    /// Generic source bodies removed from [`Self::validated`] after unit
    /// monomorphization. Package linking keeps them so a later unit can
    /// instantiate a shape the defining unit never called.
    pub generic_templates: Vec<MirFunction>,
    /// Declaration-order generic parameters for [`Self::generic_templates`],
    /// keyed by the template's `function.source` (rewritten to a synthetic
    /// def at package link). `is_shape` is `magnitudo`; otherwise `typus`.
    pub generic_params: Vec<(DefId, Vec<MirGenericParam>)>,
    /// Validated MIR token and sole owner of the program that passed
    /// [`validate_program`] against the enclosed context.
    pub validated: ValidatedMir<'a>,
    /// Owned companion-relation carrier (S3-A2): primal → companion device
    /// materialization facts, populated by the driver's
    /// [`apply_air_backward_bundle`](crate::driver::apply_air_backward_bundle)
    /// instead of being discarded after validation. The faber device-program
    /// constructor consumes it to lower generated companions into kernels.
    pub companions: radix_mir::device::MirCompanionMap,
    /// Imported-device routes transferred from this unit's lowering context
    /// (KPC-WIRE done_when (c)), keyed by import-item `DefId`: the v2 device
    /// role, launch visibility, provider-route identity, and explicit body
    /// channel collected at the import items. Package linking joins each
    /// route to the merged synthetic template `DefId` and instantiates
    /// against the resulting index; the single-unit path leaves the map
    /// untouched.
    pub imported_device_routes: FxHashMap<DefId, Vec<ImportedDeviceRoute>>,
    /// Concrete imported-device instances registered for this unit by
    /// package-link instantiation (KPC-WIRE done_when (d)): one entry per
    /// [`MirMonomorphizationKey`], each proving the instance was admitted
    /// under its route's role + body channel rather than a role bit alone.
    /// Empty until package linking runs the device-aware instantiation.
    pub imported_device_registrations: Vec<ImportedDeviceRegistration>,
}

impl<'a> LoweredMirUnit<'a> {
    /// Consume this carrier, patch its unvalidated program, and return a
    /// replacement carrier only after validation succeeds.
    ///
    /// # Errors
    /// Returns only the patch or validation error; the consumed carrier and
    /// proof token are not recoverable on rejection.
    pub fn patch_program<E>(
        self,
        patch: impl FnOnce(&mut MirProgram) -> Result<(), E>,
    ) -> Result<Self, PatchProgramError<E>> {
        let Self {
            interner,
            monomorphizations,
            closure_environments,
            device_function_metadata,
            generic_templates,
            generic_params,
            validated,
            companions,
            imported_device_routes,
            imported_device_registrations,
        } = self;
        let validated = validated.patch_program(patch)?;
        Ok(Self {
            interner,
            monomorphizations,
            closure_environments,
            device_function_metadata,
            generic_templates,
            generic_params,
            validated,
            companions,
            imported_device_routes,
            imported_device_registrations,
        })
    }
}

impl std::fmt::Debug for LoweredMirUnit<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoweredMirUnit")
            .field("validated", &self.validated)
            .field("monomorphizations", &self.monomorphizations)
            .field("closure_environments", &self.closure_environments)
            .field("device_function_metadata", &self.device_function_metadata)
            .field("generic_templates", &self.generic_templates)
            .field("generic_params", &self.generic_params)
            .field("companions", &self.companions)
            .field("imported_device_registrations", &self.imported_device_registrations)
            .finish_non_exhaustive()
    }
}

/// One generic parameter in source declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirGenericParam {
    pub name: Symbol,
    pub is_shape: bool,
}

#[derive(Default)]
pub struct MirLoweringOptions {
    pub cli_record_fields_by_local: Arc<FxHashMap<Symbol, Vec<MirRuntimeRecordField>>>,
    /// Descriptor-derived CLI record fields keyed by the `argumenta` record
    /// type (entry + `@ imperium` command surfaces), used by the shared
    /// default-path lowering when the caller supplied no parsed runtime
    /// fields for a CLI record local.
    pub cli_record_fields_by_type: Arc<FxHashMap<TypeId, Vec<MirRuntimeRecordField>>>,
    /// S8.2: build CLI-record entry/command records from runtime-filled temps
    /// (see [`FunctionBuilderContext::cli_adapter_active`]).
    pub cli_adapter_active: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirRuntimeRecordField {
    pub name: Symbol,
    pub value: MirRuntimeRecordValue,
}

impl MirRuntimeRecordField {
    #[must_use]
    pub fn operand(field: MirNamedOperand) -> Self {
        Self { name: field.name, value: MirRuntimeRecordValue::Operand(field.value) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirRuntimeRecordValue {
    Operand(MirOperand),
    Array { ty: MirType, items: Vec<MirOperand> },
}

/// Lower a semantically analyzed HIR unit into a validated MIR unit.
///
/// The unit is read immutably: every shape lowering synthesizes — the
/// `ensure_*` scanners below, CLI record field types, and monomorphization's
/// concrete shapes — goes into a lowering-owned extension table cloned from
/// the unit's arena once per call. Upstream phases own what they created, so
/// lowering never mutates the analyzed unit's `TypeTable` or `Interner`, and
/// the returned token carries its own table through
/// [`MirValidationContext::with_owned_types`].
///
/// The returned unit carries a program that has already passed MIR validation,
/// along with the validation token and lowering metadata. Lowering failures and
/// validation failures share the same error type so callers can treat the MIR
/// pipeline as one fail-closed developer command.
///
/// # Errors
/// Returns `MirError` values for unsupported HIR surfaces, missing semantic
/// types, sealed-block misuse, a corrupt type arena, and post-lowering
/// validation failures.
pub fn lower_analyzed_unit(
    unit: &AnalyzedModule,
    options: MirLoweringOptions,
) -> Result<LoweredMirUnit<'_>, Vec<MirError>> {
    // Lowering-owned extension arena: one clone of the analyzed table per
    // call; every synthesized shape below interns into this table, never
    // into the unit's.
    let mut types = TypeTable::from_snapshot(unit.types.snapshot()).map_err(|message| {
        vec![MirError {
            message: format!("MIR lowering could not clone the type arena: {message}"),
            issue: "mir_type_arena_clone_failed".to_owned(),
            span: Span::default(),
            definition_span: None,
        }]
    })?;
    // Same law for the interner: lowering-added runtime and diagnostic
    // symbols intern into this clone, so the analyzed unit's interner keeps
    // exactly the symbols upstream phases produced.
    let mut interner = unit.interner.clone();
    // The snapshot carries only the arena; the ordering-contract fact (D1.4)
    // rides along for monomorphization's bound dispatch.
    types.ordering_contract = unit.types.ordering_contract;
    ensure_map_iteration_array_types(&mut types);
    ensure_itera_coordinate_array_types(&unit.hir, &mut types);
    ensure_tensor_bracket_index_array_types(&unit.hir, &mut types);
    // The layout table is rebuilt after monomorphization's shape finalization
    // so it covers the concrete shapes/type graph the instances carry.
    prepare_mir_lowering_interner(&mut interner);
    // Merge the caller-supplied CLI record fields (parsed argv, adapter, or
    // package-MIR runtime records) with descriptor-derived fields keyed by the
    // `argumenta` record type: the shared default-path lowering fills every
    // CLI record from the analyzed `CliProgram` surface (option/operand
    // binding + typed defaults) so CLI programs lower through the same program
    // graph without caller input. Caller-supplied fields win per local.
    let cli_record_fields_by_type = cli_descriptor_record_fields_by_type(unit, &mut interner, &mut types);
    let empty_textus = interner.intern("");
    let diagnostics = DiagnosticSymbols {
        format_templates: [interner.intern("§"), interner.intern("§ §"), interner.intern("§ § §")],
        separator: interner.intern(" "),
        nihil: interner.intern("nihil"),
        lockstep_length_mismatch: interner.intern("itera lockstep sources must have equal length"),
        ordo_no_member: interner.intern("no ordo member has this value"),
        potentia_negative_exponent: interner.intern("numerus potentia failed: negative exponent"),
    };
    let mut lowerer = MirLowerer {
        unit,
        types: &types,
        interner: &interner,
        options: MirLoweringOptions { cli_record_fields_by_type, ..options },
        empty_textus,
        diagnostics,
        errors: Vec::new(),
        functions: Vec::new(),
        closure_environments: Vec::new(),
        device_function_metadata: MirDeviceFunctionMetadataMap::default(),
        struct_field_types: None,
    };
    lowerer.lower();

    let lowering_errors = std::mem::take(&mut lowerer.errors);
    if lowering_errors.is_empty() {
        let monomorphize::MonomorphizeOutput { monomorphizations, templates: generic_templates, generic_params } =
            lowerer.monomorphize();
        let monomorphization_errors = std::mem::take(&mut lowerer.errors);
        if !monomorphization_errors.is_empty() {
            return Err(monomorphization_errors);
        }
        let functions = std::mem::take(&mut lowerer.functions);
        let closure_environments = std::mem::take(&mut lowerer.closure_environments);
        let device_function_metadata = std::mem::take(&mut lowerer.device_function_metadata);
        let lowering_interner = interner.clone();
        drop(lowerer);
        let mut program = MirProgram { functions };
        // Shape substitution interns the concrete shapes/types each instance
        // carries into the lowering-owned arena before layouts and validation
        // read them back.
        let finalize_errors = monomorphize::finalize_monomorphized_shapes(
            &unit.hir,
            &mut types,
            &mut program.functions,
            &monomorphizations,
        );
        if !finalize_errors.is_empty() {
            return Err(finalize_errors);
        }
        // Full (un-gated) device roles: composition checks classify every
        // `@ nucleum` function, including private helpers that produce no
        // entry row. Entry gating happens on the driver's emit/dispatch path
        // via `device_entry_roles_from_hir`.
        let roles = crate::mir::device_roles_from_hir(&unit.hir);
        // DFV2-4: launch entries are the public `@ nucleum` sources — the
        // only device functions that stay program symbols when device code
        // also calls them; composed private helpers are pruned.
        let launch_entries = crate::mir::device_entry_roles_from_hir(&unit.hir, &interner)
            .into_iter()
            .filter(|(_, role)| *role == crate::mir::MirDeviceRole::Kernel)
            .map(|(source, _)| source)
            .collect::<FxHashSet<_>>();
        let layouts = MirLayoutTable::from_types(&types);
        let summaries =
            crate::semantic::passes::fragment_definition::check(&unit.hir, &unit.resolver, &types, &interner).summaries;
        let instantiation_errors = super::fragment_instantiation::check_with_metadata(
            &program.functions,
            &monomorphizations,
            &layouts,
            &types,
            &roles,
            Some(&summaries),
            &device_function_metadata,
        );
        if !instantiation_errors.is_empty() {
            return Err(instantiation_errors
                .into_iter()
                .map(|error| MirError {
                    message: error.message(),
                    issue: error.issue.to_owned(),
                    span: error.span,
                    definition_span: None,
                })
                .collect());
        }
        let composition_errors = super::fragment_composition::compose_with_metadata(
            &mut program.functions,
            &monomorphizations,
            &roles,
            &device_function_metadata,
            super::fragment_composition::active_budget(),
            Some(&interner),
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
        let mut maps = LoweringContextMaps::collect(unit);
        // KPC-WIRE done_when (c): the module-owned route map is transferred
        // exactly once instead of being dropped with the rest of the walk.
        let imported_device_routes = maps.take_imported_device_routes();
        let mut validation = maps.validation;
        align_function_signatures_with_mir_params(&mut validation, &program.functions);
        validation.layouts = MirLayoutTable::from_types(&types);
        validation.closure_environments = closure_environments
            .iter()
            .map(|environment| (environment.id, environment.clone()))
            .collect();
        // The escaping context resolves MIR symbols through the lowering-owned
        // interner clone — it carries the runtime/diagnostic symbols this
        // lowering interned (format templates, the lockstep panic text), which
        // the analyzed unit's interner no longer gains by mutation.
        validation.interner = radix_mir::MirInterner::Owned(std::sync::Arc::new(interner.clone()));
        // Ownership hand-off: the validation token takes the lowering-owned
        // arena, so the returned unit carries its own table and borrows
        // nothing from the lowering call itself.
        let validation = validation.with_owned_types(types);
        let validated = ValidatedMir::new(program, validation).map_err(|errors| {
            errors
                .into_iter()
                .map(|error| MirError::validation(error.span, error.message))
                .collect::<Vec<_>>()
        })?;
        Ok(LoweredMirUnit {
            interner: lowering_interner,
            monomorphizations,
            closure_environments,
            device_function_metadata,
            generic_templates,
            generic_params,
            validated,
            companions: radix_mir::device::MirCompanionMap::default(),
            imported_device_routes,
            imported_device_registrations: Vec::new(),
        })
    } else {
        Err(lowering_errors)
    }
}

/// Replace HIR reference wrappers in local callable signatures with the MIR
/// storage types. `MirParamMode` carries the borrow ABI separately, while call
/// argument validation must compare the caller's value place with the callee's
/// referent slot type.
fn align_function_signatures_with_mir_params(validation: &mut MirValidationContext<'_>, functions: &[MirFunction]) {
    for function in functions {
        let Some(source) = function.source else {
            continue;
        };
        let Some(signature) = validation.functions.get_mut(&source) else {
            continue;
        };
        for (expected, param) in signature.params.iter_mut().zip(&function.params) {
            expected.ty = param.ty;
        }
    }
}

/// Pre-intern symbols synthesized by MIR lowering before a target clones the
/// analyzed-unit interner for emission.
pub fn prepare_mir_lowering_interner(interner: &mut Interner) {
    for template in ["", "§", "§ §", "§ § §", " ", "nihil"] {
        interner.intern(template);
    }
}

/// Build descriptor-derived CLI record fields keyed by the `argumenta` record
/// type.
///
/// The shared default-path lowering constructs every `argumenta` record
/// (entry and `@ imperium` command functions) from the analyzed CLI descriptor
/// when the caller supplied no parsed runtime fields for that local: each
/// option/operand binding becomes a named record field carrying its typed
/// default (flags default to false, undefaulted value options to `nihil`,
/// rest operands to an empty array, required operands to a type-shaped
/// placeholder the runtime rebinds). Keying by record type keeps entry and
/// command surfaces that share a binding name (e.g. `cli/cli.fab` entry and
/// `greet` both bind `args`) from colliding.
fn cli_descriptor_record_fields_by_type(
    unit: &AnalyzedModule,
    interner: &mut Interner,
    types: &mut TypeTable,
) -> Arc<FxHashMap<TypeId, Vec<MirRuntimeRecordField>>> {
    let Some(cli) = unit.cli_program.as_ref() else {
        return Arc::new(FxHashMap::default());
    };
    let mut fields_by_type = FxHashMap::default();
    if let Some(entry_ty) = cli_entry_args_type(unit) {
        let fields = cli_descriptor_record_fields(
            interner,
            types,
            cli.global_options.iter().chain(cli.options.iter()),
            cli.global_operands.iter().chain(cli.operands.iter()),
        );
        fields_by_type.entry(entry_ty).or_insert(fields);
    }
    for command in &cli.commands {
        if command.args_binding.is_none() {
            continue;
        }
        let Some(command_ty) = cli_command_record_type(unit, command.function_symbol) else {
            continue;
        };
        let fields = cli_descriptor_record_fields(
            interner,
            types,
            cli.global_options.iter().chain(command.options.iter()),
            cli.global_operands.iter().chain(command.operands.iter()),
        );
        fields_by_type.entry(command_ty).or_insert(fields);
    }
    Arc::new(fields_by_type)
}

/// Record type of the root `incipit argumenta` local, read from the HIR entry
/// block's runtime-provided args local.
fn cli_entry_args_type(unit: &AnalyzedModule) -> Option<TypeId> {
    let name = unit.hir.entry_args_name?;
    let entry = unit.hir.entry.as_ref()?;
    entry.statements.iter().find_map(|statement| {
        let HirStatementKind::Local(local) = &statement.kind else {
            return None;
        };
        (local.name == name && local.runtime_provided)
            .then_some(local.ty)
            .flatten()
    })
}

/// Record type of a mounted command's `argumenta` param, read from the HIR
/// command function's CLI-args parameter.
fn cli_command_record_type(unit: &AnalyzedModule, function_symbol: Symbol) -> Option<TypeId> {
    unit.hir.items.iter().find_map(|item| {
        let HirItemKind::Function(function) = &item.kind else {
            return None;
        };
        (function.name == function_symbol)
            .then(|| function.cli_args.as_ref().map(|param| param.ty))
            .flatten()
    })
}

/// Build the shared-MIR `argumenta` record surface for one CLI entry point or
/// command from the normalized CLI descriptor: options first (in global-then-
/// local order), then operands, matching the HIR lowerer's entry and command
/// argument record types.
fn cli_descriptor_record_fields<'a>(
    interner: &mut Interner,
    types: &mut TypeTable,
    options: impl Iterator<Item = &'a CliOption>,
    operands: impl Iterator<Item = &'a CliOperand>,
) -> Vec<MirRuntimeRecordField> {
    let mut fields = Vec::new();
    for option in options {
        let value = if option.flag {
            MirRuntimeRecordValue::Operand(MirOperand::Constant(MirConstant::Bool(match &option.default {
                Some(CliDefault::Bool(value)) => *value,
                _ => false,
            })))
        } else if matches!(option.ty, CliType::ListaTextus | CliType::ListaNumerus) {
            if option.default.is_some() {
                // Defaulted list options are non-optional `array(element)`; no
                // literal list default exists, so the compile-time shape is the
                // empty array.
                let element = cli_record_base_type(types, &option.ty);
                MirRuntimeRecordValue::Array { ty: MirType::semantic(types.array(element)), items: Vec::new() }
            } else {
                MirRuntimeRecordValue::Operand(MirOperand::Constant(MirConstant::Nil))
            }
        } else if let Some(default) = &option.default {
            MirRuntimeRecordValue::Operand(
                cli_literal_constant(interner, default).unwrap_or_else(|| cli_type_zero_operand(interner, &option.ty)),
            )
        } else {
            // Undefaulted value options are `option(value)` in the record type.
            MirRuntimeRecordValue::Operand(MirOperand::Constant(MirConstant::Nil))
        };
        fields.push(MirRuntimeRecordField { name: option.binding_symbol, value });
    }
    for operand in operands {
        let value = if operand.rest || matches!(operand.ty, CliType::ListaTextus | CliType::ListaNumerus) {
            let element = cli_record_base_type(types, &operand.ty);
            MirRuntimeRecordValue::Array { ty: MirType::semantic(types.array(element)), items: Vec::new() }
        } else if let Some(default) = &operand.default {
            MirRuntimeRecordValue::Operand(
                cli_literal_constant(interner, default).unwrap_or_else(|| cli_type_zero_operand(interner, &operand.ty)),
            )
        } else {
            MirRuntimeRecordValue::Operand(cli_type_zero_operand(interner, &operand.ty))
        };
        fields.push(MirRuntimeRecordField { name: operand.binding_symbol, value });
    }
    fields
}

/// Map a CLI boundary type to the semantic primitive used as its record-field
/// base (mirroring `cli_value_type` in the HIR lowerer).
fn cli_record_base_type(types: &TypeTable, ty: &CliType) -> TypeId {
    match ty {
        CliType::Textus | CliType::Ignotum => types.primitive(Primitive::Textus),
        CliType::Numerus => types.primitive(Primitive::Numerus),
        CliType::Fractus => types.primitive(Primitive::Fractus),
        CliType::Bivalens => types.primitive(Primitive::Bivalens),
        CliType::Octeti => types.primitive(Primitive::Octeti),
        CliType::ListaTextus => types.primitive(Primitive::Textus),
        CliType::ListaNumerus => types.primitive(Primitive::Numerus),
    }
}

/// Literal `CliDefault` → MIR constant. Non-literal defaults return `None` so
/// callers fall back to a type-shaped placeholder.
fn cli_literal_constant(interner: &mut Interner, default: &CliDefault) -> Option<MirOperand> {
    let constant = match default {
        CliDefault::Text(value) => MirConstant::String(interner.intern(value)),
        CliDefault::Integer(value) => MirConstant::Int(*value),
        CliDefault::Float(value) => MirConstant::Float(*value),
        CliDefault::Bool(value) => MirConstant::Bool(*value),
        CliDefault::Nil => MirConstant::Nil,
        CliDefault::Expr(_) => return None,
    };
    Some(MirOperand::Constant(constant))
}

/// Type-shaped zero for a required CLI operand (or non-literal default) the
/// runtime rebinds from argv.
fn cli_type_zero_operand(interner: &mut Interner, ty: &CliType) -> MirOperand {
    let constant = match ty {
        CliType::Textus | CliType::Ignotum => MirConstant::String(interner.intern("")),
        CliType::Numerus => MirConstant::Int(0),
        CliType::Fractus => MirConstant::Float(0.0),
        CliType::Bivalens => MirConstant::Bool(false),
        CliType::Octeti => MirConstant::Octeti(Vec::new()),
        // List-shaped fields are always array aggregates in the record;
        // a zero constant is only reachable for a non-list fallback path.
        CliType::ListaTextus | CliType::ListaNumerus => MirConstant::Nil,
    };
    MirOperand::Constant(constant)
}

/// Lower and render a deterministic MIR dump for compiler inspection.
///
/// This helper exists so `radix mir` can expose one operation without letting
/// dump rendering bypass validation.
///
/// # Errors
/// Returns `MirError` values for unsupported HIR surfaces, missing semantic
/// types, sealed-block misuse, and post-lowering validation failures.
pub fn dump_analyzed_unit(unit: &AnalyzedModule) -> Result<String, Vec<MirError>> {
    lower_analyzed_unit(unit, MirLoweringOptions::default()).map(|lowered| dump_program(lowered.validated.program()))
}

fn ensure_map_iteration_array_types(types: &mut TypeTable) {
    let mut elements = FxHashSet::default();
    for index in 0..types.type_count() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        match types.get(TypeId(index as u32)) {
            Type::Map(key, value) => {
                elements.insert(*key);
                elements.insert(*value);
            }
            Type::Cursor(element) => {
                elements.insert(*element);
            }
            Type::AsyncCursor(element, _) => {
                elements.insert(*element);
            }
            _ => {}
        }
    }
    for element in elements {
        types.ensure_array(element);
    }
}

fn ensure_itera_coordinate_array_types(hir: &crate::hir::HirModule, types: &mut TypeTable) {
    struct IteraCoordinateCollector<'a> {
        types: &'a TypeTable,
        elements: FxHashSet<TypeId>,
    }

    impl HirVisitor for IteraCoordinateCollector<'_> {
        fn visit_expr(&mut self, expr: &HirExpression) {
            match &expr.kind {
                HirExpressionKind::Itera(_, coordinates, _, _, iter, _) if !coordinates.is_empty() => {
                    self.collect_shaped_element(iter);
                }
                // The sequential `summa ex` fold rides the shaped itera arm
                // (SFR-4), so its coordinate list needs the same array types.
                HirExpressionKind::Summa { source, coordinates, .. } if !coordinates.is_empty() => {
                    self.collect_shaped_element(source);
                }
                _ => {}
            }
            walk_expr(self, expr);
        }
    }

    impl IteraCoordinateCollector<'_> {
        fn collect_shaped_element(&mut self, iter: &HirExpression) {
            let mut iter_ty = iter.ty;
            while let Some(ty) = iter_ty {
                match self.types.get(ty) {
                    Type::Alias(_, inner) => iter_ty = Some(*inner),
                    Type::Tensor(element, _) | Type::Matrix(element, _) => {
                        self.elements.insert(*element);
                        break;
                    }
                    _ => break,
                }
            }
        }
    }

    let mut collector = IteraCoordinateCollector { types, elements: FxHashSet::default() };
    walk_program(&mut collector, hir);
    let elements = std::mem::take(&mut collector.elements);
    drop(collector);
    if !elements.is_empty() {
        let numerus = types.primitive(Primitive::Numerus);
        types.ensure_array(numerus);
        for element in elements {
            types.intern(Type::Option(element));
        }
    }
}

fn ensure_tensor_bracket_index_array_types(hir: &crate::hir::HirModule, types: &mut TypeTable) {
    struct TensorBracketIndexArrayCollector<'a> {
        types: &'a TypeTable,
        elements: FxHashSet<TypeId>,
    }

    impl HirVisitor for TensorBracketIndexArrayCollector<'_> {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if let HirExpressionKind::Index(object, index) = &expr.kind {
                let object_is_tensor = object
                    .ty
                    .is_some_and(|ty| crate::semantic::tensor_bracket::is_tensor_type(self.types, ty));
                let index_is_list = index
                    .ty
                    .is_some_and(|ty| crate::semantic::tensor_bracket::is_bracket_index_list_type(self.types, ty));
                if object_is_tensor
                    && !index_is_list
                    && let Some(index_ty) = index.ty
                {
                    self.elements.insert(index_ty);
                }
            }
            walk_expr(self, expr);
        }
    }

    let mut collector = TensorBracketIndexArrayCollector { types, elements: FxHashSet::default() };
    walk_program(&mut collector, hir);
    for element in collector.elements {
        types.ensure_array(element);
    }
}

#[derive(Clone, Copy)]
struct DiagnosticSymbols {
    format_templates: [Symbol; 3],
    separator: Symbol,
    /// The rendered null-value textus used by the scribe grouping when an
    /// option argument's payload is absent (`display_option` semantics).
    ///
    /// Keep this in lockstep with the Rust runtime carrier
    /// (`faber::display_option`), which owns the text. D23.2 splits the null
    /// *value* (`nulla`) from the null *type* (`nihil`); the value display must
    /// therefore move to `nulla` on both sides together — this string, the
    /// runtime carrier, and the corpus `.expected` oracles — in the D23.2
    /// sweep, never here alone.
    nihil: Symbol,
    /// Lockstep `itera ex` runtime admission message: the
    /// fail-closed equal-length guard's panic textus.
    lockstep_length_mismatch: Symbol,
    /// Failed `numerus ↦ Ordo` payload (D9.4): no member has the value.
    ordo_no_member: Symbol,
    /// Integer `potentia` with a negative exponent (ruling 28): the
    /// arithmetic trap's panic textus.
    potentia_negative_exponent: Symbol,
}

struct MirLowerer<'a> {
    unit: &'a AnalyzedModule,
    /// Lowering-owned extension arena (a per-call clone of `unit.types` that
    /// this lowering alone extends). Every type read during the walk goes
    /// through this table so synthesized shapes resolve like analyzed ones.
    types: &'a TypeTable,
    /// Lowering-owned interner clone: same law as [`Self::types`] for the
    /// runtime/diagnostic symbols lowering interns.
    interner: &'a Interner,
    options: MirLoweringOptions,
    empty_textus: Symbol,
    diagnostics: DiagnosticSymbols,
    errors: Vec<MirError>,
    functions: Vec<MirFunction>,
    closure_environments: Vec<MirClosureEnvironment>,
    device_function_metadata: MirDeviceFunctionMetadataMap,
    /// Struct field types collected during the body-lowering walk and retained
    /// for generic monomorphization, avoiding a second HIR traversal.
    struct_field_types: Option<Arc<FxHashMap<DefId, FxHashMap<Symbol, MirType>>>>,
}

/// MIR locals, CFG, and side-effect functions produced by one callable lowering pass.
struct CallableLoweringParts {
    params: Vec<MirParam>,
    locals: Vec<MirLocalDecl>,
    temps: Vec<MirTemp>,
    blocks: Vec<MirBlock>,
    errors: Vec<MirError>,
    synthetics: Vec<MirFunction>,
    closure_environments: Vec<MirClosureEnvironment>,
    device_function_metadata: MirDeviceFunctionMetadataMap,
}

struct CallableLoweringConfig<'input> {
    annotations: &'input [radix_hir::HirAnnotation],
    error_ty: Option<MirType>,
    parent_function_id: MirFunctionId,
    device_role: Option<MirDeviceRole>,
    target: CallableLoweringTarget<'input>,
}

enum CallableLoweringTarget<'input> {
    Function(&'input HirFunction),
    Method {
        struct_item: &'input HirItem,
        strukt: &'input HirStruct,
        method: &'input HirMethod,
        receiver_ty: TypeId,
    },
}

/// Per-function lowering inputs collected once per whole-unit pass.
pub(super) struct BodyLoweringContext<'maps, 'unit> {
    context_maps: &'maps LoweringContextMaps<'unit>,
    struct_fields: Arc<FxHashMap<DefId, Vec<&'unit HirField>>>,
    struct_field_types: Arc<FxHashMap<DefId, FxHashMap<crate::lexer::Symbol, MirType>>>,
    function_params: Arc<FxHashMap<DefId, Vec<context::CallableParamMeta<'unit>>>>,
    empty_textus: Symbol,
    diagnostics: DiagnosticSymbols,
}

impl<'a> MirLowerer<'a> {
    /// Run the whole-unit lowering sequence before MIR validation.
    ///
    /// Whole-unit maps are collected up front because function bodies need
    /// signatures, enum variant metadata, provider imports, and validation
    /// tables that may be declared later in source order.
    fn lower(&mut self) {
        let context_maps = LoweringContextMaps::collect(self.unit);
        let struct_fields = Arc::new(struct_field_map(self.unit));
        let struct_field_types = Arc::new(context_maps.validation.struct_fields.clone());
        self.struct_field_types = Some(Arc::clone(&struct_field_types));
        let function_params = Arc::new(function_param_map(self.unit));
        let body_ctx = BodyLoweringContext {
            context_maps: &context_maps,
            struct_fields,
            struct_field_types,
            function_params,
            empty_textus: self.empty_textus,
            diagnostics: self.diagnostics,
        };
        ItemLoweringPass::new(self, &body_ctx).lower_items();

        if let Some(entry) = &self.unit.hir.entry {
            self.lower_entry(entry, &body_ctx);
        }
    }

    /// Compute shader stage from HIR function flags and emit a diagnostic when
    /// conflicting annotations make the stage unresolvable.
    fn compute_shader_stage(&mut self, function: &HirFunction, span: Span) -> Option<MirKernelShaderStage> {
        let stage = shader_stage_for_function(function);
        if stage.is_none() && !annotations_exclusive(function) {
            self.errors.push(MirError::validation(
                span,
                "conflicting shader-stage annotations: at most one of @nucleum, @nucleum fragment, @vertex, or @fragment is allowed",
            ));
        }
        stage
    }

    /// Lower one HIR function item into one MIR function.
    ///
    /// Function lowering is intentionally type-strict: parameters, locals, the
    /// return type, and alternate-exit type must already be present on HIR. The
    /// builder may still emit local lowering errors, but this wrapper preserves
    /// source-order accounting for the function while the whole-unit pipeline
    /// decides whether validation is allowed to run.
    fn lower_function(&mut self, item: &HirItem, function: &HirFunction, body_ctx: &BodyLoweringContext<'_, '_>) {
        let Some(return_ty) = function.ret_ty else {
            self.errors
                .push(MirError::missing_type(item.span, "function return type"));
            return;
        };
        self.push_lowered_callable(
            function.name,
            item.def_id,
            item.span,
            return_ty,
            function,
            &item.annotations,
            CallableLoweringTarget::Function(function),
            body_ctx,
        );
    }

    /// Lower a genus method as an ordinary MIR function with an explicit
    /// receiver parameter bound to the struct definition used by `ego`.
    fn lower_method(
        &mut self,
        struct_item: &HirItem,
        strukt: &HirStruct,
        method: &HirMethod,
        body_ctx: &BodyLoweringContext<'_, '_>,
    ) {
        let Some(return_ty) = method.func.ret_ty else {
            self.errors
                .push(MirError::missing_type(method.span, "method return type"));
            return;
        };
        let Some(receiver_ty) = self.types.find_struct(struct_item.def_id) else {
            self.errors
                .push(MirError::missing_type(method.span, "method receiver type"));
            return;
        };
        self.push_lowered_callable(
            method.func.name,
            method.def_id,
            method.span,
            return_ty,
            &method.func,
            &method.annotations,
            CallableLoweringTarget::Method { struct_item, strukt, method, receiver_ty },
            body_ctx,
        );
    }

    /// Lower one callable body and push its `MirFunction` (plus any synthetic
    /// companion functions) into the program.
    ///
    /// Shared spine of [`Self::lower_function`] and [`Self::lower_method`].
    /// Preserves source-order error accounting: the caller's return-type guard
    /// fires first, then body lowering errors, then the shader-stage
    /// annotation check (which may append a validation error last).
    fn push_lowered_callable(
        &mut self,
        name: Symbol,
        def_id: DefId,
        span: Span,
        return_ty: TypeId,
        function: &HirFunction,
        annotations: &[radix_hir::HirAnnotation],
        target: CallableLoweringTarget<'_>,
        body_ctx: &BodyLoweringContext<'_, '_>,
    ) {
        let error_ty = function.err_ty.map(MirType::semantic);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; lowered function count is bounded by u32::MAX"
        )]
        let parent_function_id = MirFunctionId(self.functions.len() as u32);
        let CallableLoweringParts {
            params,
            locals,
            temps,
            blocks,
            errors,
            synthetics,
            closure_environments,
            device_function_metadata,
        } = self.lower_callable_body(
            body_ctx,
            CallableLoweringConfig {
                annotations,
                error_ty,
                parent_function_id,
                device_role: device_role_for_function(function),
                target,
            },
        );
        self.errors.extend(errors);
        self.closure_environments.extend(closure_environments);
        self.device_function_metadata
            .extend(device_function_metadata);

        let shader_stage = self.compute_shader_stage(function, span);
        // KCM-05: named device functions carry the same explicit role,
        // stage, and visibility facts as source-less kernel closures. The
        // device-program constructor is fail-closed when this row is absent,
        // so populate it at the source-function lowering boundary rather than
        // making package consumers reconstruct HIR visibility later.
        if let (Some(role), Some(shader_stage)) = (device_role_for_function(function), shader_stage) {
            self.device_function_metadata.insert(
                parent_function_id,
                MirDeviceFunctionMetadata {
                    identity: MirSyntheticFunctionIdentity { parent: parent_function_id, ordinal: 0 },
                    role,
                    shader_stage,
                    visibility: if is_public_entry(annotations, self.interner) {
                        MirFunctionVisibility::Public
                    } else {
                        MirFunctionVisibility::Private
                    },
                    origin: MirFunctionOrigin::Source,
                },
            );
        }
        self.push_function_with_synthetics(
            MirFunction {
                id: parent_function_id,
                source: Some(def_id),
                name: Some(name),
                params,
                locals,
                temps,
                blocks,
                return_ty: MirType::semantic(return_ty),
                error_ty,
                is_async: function.is_async,
                is_generator: function.is_generator,
                shader_stage,
                span,
            },
            synthetics,
        );
    }

    /// Fresh per-callable lowering context from the whole-unit maps, wired to
    /// this lowerer's CLI-record options. The returned context outlives the
    /// maps borrow (it owns its Arc-cloned tables), so the unit lifetime `'a`
    /// is named explicitly.
    fn builder_context(&self, body_ctx: &BodyLoweringContext<'_, 'a>) -> FunctionBuilderContext<'a> {
        body_ctx.context_maps.builder_context(
            self.interner,
            body_ctx.empty_textus,
            body_ctx.diagnostics,
            Arc::clone(&body_ctx.struct_fields),
            Arc::clone(&body_ctx.struct_field_types),
            Arc::clone(&body_ctx.function_params),
            Arc::clone(&self.options.cli_record_fields_by_local),
            Arc::clone(&self.options.cli_record_fields_by_type),
            self.options.cli_adapter_active,
        )
    }

    /// Lower one function or method body through a fresh [`FunctionBuilder`].
    ///
    /// Parameter binding differs between top-level functions and genus methods,
    /// so callers supply an `add_params` hook while this helper owns builder
    /// context construction, body visitation, and synthetic-function extraction.
    fn lower_callable_body(
        &mut self,
        body_ctx: &BodyLoweringContext<'_, '_>,
        config: CallableLoweringConfig<'_>,
    ) -> CallableLoweringParts {
        let context = self.builder_context(body_ctx);
        let shader_stage = match &config.target {
            CallableLoweringTarget::Function(function) => shader_stage_for_function(function),
            CallableLoweringTarget::Method { method, .. } => shader_stage_for_function(&method.func),
        };
        let mut builder = FunctionBuilder::for_function(
            self.types,
            config.error_ty,
            context,
            config.parent_function_id,
            config.device_role,
            shader_stage,
        );
        builder.size_params = match &config.target {
            CallableLoweringTarget::Function(function) => size_param_names(&function.type_params),
            CallableLoweringTarget::Method { method, .. } => size_param_names(&method.func.type_params),
        };
        builder.bound_methods = match &config.target {
            CallableLoweringTarget::Function(function) => self.bound_method_targets(&function.type_params),
            CallableLoweringTarget::Method { method, .. } => self.bound_method_targets(&method.func.type_params),
        };
        let (body, cli_args) = match config.target {
            CallableLoweringTarget::Function(function) => {
                for param in &function.params {
                    builder.add_param(param.def_id, param.name, param.ty, param.mode, param.span);
                }
                let explicit_lane = lane_binding_local(function, config.annotations, self.interner);
                if let Some((def_id, name, ty, span)) = explicit_lane {
                    builder.add_lane_param(def_id, name, ty, span);
                }
                if function.nucleum_fragment {
                    for (def_id, name, ty, span) in inherited_lane_binding_locals(
                        function,
                        &self.unit.resolver,
                        explicit_lane.map(|(def_id, ..)| def_id),
                    ) {
                        builder.add_inherited_lane_param(def_id, name, ty, span);
                    }
                }
                (function.body.as_ref(), function.cli_args.as_ref())
            }
            CallableLoweringTarget::Method { struct_item, strukt, method, receiver_ty } => {
                builder.add_param(struct_item.def_id, strukt.name, receiver_ty, HirParamMode::Owned, method.span);
                for param in &method.func.params {
                    builder.add_param(param.def_id, param.name, param.ty, param.mode, param.span);
                }
                if let Some((def_id, name, ty, span)) =
                    lane_binding_local(&method.func, config.annotations, self.interner)
                {
                    builder.add_lane_param(def_id, name, ty, span);
                }
                (method.func.body.as_ref(), None)
            }
        };

        let blocks = match body {
            Some(body) => builder.lower_body_with_cli_args(body, cli_args),
            None => Vec::new(),
        };
        let synthetics = builder.take_synthetic_functions();
        let closure_environments = builder.take_closure_environments();
        let device_function_metadata = builder.take_device_function_metadata();
        CallableLoweringParts {
            params: builder.params,
            locals: builder.locals,
            temps: builder.temps,
            blocks,
            errors: builder.errors,
            synthetics,
            closure_environments,
            device_function_metadata,
        }
    }

    fn push_function_with_synthetics(&mut self, function: MirFunction, synthetics: Vec<MirFunction>) {
        self.functions.push(function);
        for synthetic in synthetics {
            self.functions.push(synthetic);
        }
    }

    /// Lower the top-level entry block as a synthetic vacuum-returning function.
    ///
    /// Entry lowering reuses ordinary function-body MIR construction so the MIR
    /// stays target-neutral and unsupported entry contents fail through the same
    /// diagnostics as unsupported function contents.
    fn lower_entry(&mut self, entry: &HirBlock, body_ctx: &BodyLoweringContext<'_, '_>) {
        let vacuum = self.types.primitive(Primitive::Vacuum);
        let context = self.builder_context(body_ctx);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; lowered function count is bounded by u32::MAX"
        )]
        let parent_function_id = MirFunctionId(self.functions.len() as u32);
        let (locals, temps, blocks, errors, synthetics, closure_environments, device_function_metadata) = {
            let mut builder = FunctionBuilder::for_function(self.types, None, context, parent_function_id, None, None);
            builder.shared_bindings = radix_hir::capture::CaptureFacts::for_block(entry).shared;
            let entry_block = builder.fresh_block(entry.span);
            builder.switch_to(entry_block);
            let mut consts_lowered = true;
            for item in &self.unit.hir.items {
                if let HirItemKind::Constant(const_item) = &item.kind
                    && builder
                        .lower_const_binding(item.def_id, const_item, item.span)
                        .is_none()
                {
                    consts_lowered = false;
                    break;
                }
            }
            if consts_lowered {
                builder.visit_block(entry);
                builder.terminate_open_current(MirTerminatorKind::Return(None), entry.span);
            }
            let blocks = builder.finish_blocks();
            let synthetics = builder.take_synthetic_functions();
            let closure_environments = builder.take_closure_environments();
            let device_function_metadata = builder.take_device_function_metadata();
            (
                builder.locals,
                builder.temps,
                blocks,
                builder.errors,
                synthetics,
                closure_environments,
                device_function_metadata,
            )
        };
        self.errors.extend(errors);
        self.closure_environments.extend(closure_environments);
        self.device_function_metadata
            .extend(device_function_metadata);

        self.push_function_with_synthetics(
            MirFunction {
                id: parent_function_id,
                source: None,
                name: None,
                params: Vec::new(),
                locals,
                temps,
                blocks,
                return_ty: MirType::semantic(vacuum),
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: entry.span,
            },
            synthetics,
        );
    }
}

fn lane_binding_local(
    function: &HirFunction,
    annotations: &[radix_hir::HirAnnotation],
    interner: &Interner,
) -> Option<(DefId, Symbol, TypeId, Span)> {
    let lane = annotations.iter().find_map(|annotation| {
        if interner.resolve(annotation.family) != "nucleum" {
            return None;
        }
        annotation
            .fields
            .iter()
            .find_map(|field| (interner.resolve(field.name) == "lane").then_some(field.value.clone()))
    })?;
    let HirAnnotationValue::String(name) = lane else {
        return None;
    };
    let body = function.body.as_ref()?;
    body.statements.iter().find_map(|statement| {
        let HirStatementKind::Local(local) = &statement.kind else {
            return None;
        };
        (local.runtime_provided && local.name == name).then_some((local.def_id, local.name, local.ty?, statement.span))
    })
}

fn inherited_lane_binding_locals(
    function: &HirFunction,
    resolver: &crate::semantic::Resolver,
    explicit_lane: Option<DefId>,
) -> Vec<(DefId, Symbol, TypeId, Span)> {
    let Some(body) = function.body.as_ref() else {
        return Vec::new();
    };
    body.statements
        .iter()
        .filter_map(|statement| {
            let HirStatementKind::Local(local) = &statement.kind else {
                return None;
            };
            if !local.runtime_provided || Some(local.def_id) == explicit_lane {
                return None;
            }
            let symbol = resolver.get_symbol(local.def_id)?;
            (symbol.kind == crate::semantic::SymbolKind::Lane).then_some((
                local.def_id,
                local.name,
                local.ty?,
                statement.span,
            ))
        })
        .collect()
}

/// Annotation exclusivity check shared by device-role and shader-stage
/// assignment (CTO a7c616ed, extended NF-2: at most one of Entry/Fragment/
/// `is_vertex`/`is_fragment` may be set).
fn annotations_exclusive(function: &HirFunction) -> bool {
    [
        function.is_kernel,
        function.nucleum_fragment,
        function.is_vertex,
        function.is_fragment,
    ]
    .iter()
    .filter(|&&b| b)
    .count()
        <= 1
}

/// Map HIR function flags to MIR device roles.
///
/// `MirDeviceRole` classifies compute entry (`Kernel`) and the nucleum
/// composition role (`Fragment`). Graphics shader stages (`@ vertex`/
/// `@ fragment`) set `is_vertex`/`is_fragment` on `HirFunction`, but the
/// shader-stage identity seam is `MirKernelShaderStage`, not
/// `MirDeviceRole` (CTO a7c616ed: do not conflate execution stage with
/// compute helper role).
fn device_role_for_function(function: &HirFunction) -> Option<MirDeviceRole> {
    if !annotations_exclusive(function) {
        return None;
    }
    if function.is_kernel {
        Some(MirDeviceRole::Kernel)
    } else if function.nucleum_fragment {
        Some(MirDeviceRole::Fragment)
    } else {
        None
    }
}

/// Map HIR function flags to MIR shader stage identity.
///
/// `MirKernelShaderStage` carries execution-stage identity (Compute/
/// Vertex/Fragment). Assigned during HIR→MIR lowering so that lowered
/// `MirFunction`s carry the stage seam even before full body lowering
/// lands. Conflicting annotations fail-closed to `None`.
///
/// Residual: body lowering through `MirProgram` + WGSL emit + backend impact
/// is the next unit. Stage assignment is the identity seam, not the body path.
fn shader_stage_for_function(function: &HirFunction) -> Option<MirKernelShaderStage> {
    if !annotations_exclusive(function) {
        return None;
    }
    if function.is_kernel {
        Some(MirKernelShaderStage::Compute)
    } else if function.is_vertex {
        Some(MirKernelShaderStage::Vertex)
    } else if function.is_fragment {
        Some(MirKernelShaderStage::Fragment)
    } else {
        None
    }
}

/// Body-visible `magnitudo` parameters of one callable, def → name. A value
/// position naming one of these defs carries its compile-time size into MIR
/// (sgr C2a); `typus` parameters are types, never values.
impl MirLowerer<'_> {
    /// `(type parameter, method) → contract method` for one callable's
    /// `implet` bounds (D1.1). The first bound that declares a method wins,
    /// matching the typechecker's lookup order.
    fn bound_method_targets(&self, params: &[HirTypeParam]) -> FxHashMap<(Symbol, Symbol), DefId> {
        let mut targets = FxHashMap::default();
        for param in params {
            let crate::hir::HirTypeParamConstraint::Implet(bounds) = &param.constraint else {
                continue;
            };
            for bound in bounds {
                // D1.4: a call through the ordering bound targets the
                // contract itself; monomorphization picks the witness genus's
                // `compare` or the built-in compare. The contract may be
                // imported, so its method has no local definition.
                if self.unit.types.ordering_contract == Some(bound.contract)
                    && let Some(compare) = self
                        .unit
                        .interner
                        .lookup(crate::semantic::radix_contract::ORDERING_METHOD)
                {
                    targets
                        .entry((param.name, compare))
                        .or_insert(bound.contract);
                    continue;
                }
                let Some(interface) = self
                    .unit
                    .hir
                    .items
                    .iter()
                    .find_map(|item| match &item.kind {
                        HirItemKind::Interface(interface) if item.def_id == bound.contract => Some(interface),
                        _ => None,
                    })
                else {
                    continue;
                };
                for method in &interface.methods {
                    targets
                        .entry((param.name, method.name))
                        .or_insert(method.def_id);
                }
            }
        }
        targets
    }
}

fn size_param_names(params: &[HirTypeParam]) -> FxHashMap<DefId, Symbol> {
    params
        .iter()
        .filter(|param| !param.is_typus())
        .map(|param| (param.def_id, param.name))
        .collect()
}

/// Interned `IndexExpr::Param` node for one size parameter. Signature shapes
/// intern the node whenever the parameter appears in a type, so the ordinary
/// body-visible case resolves; a parameter with no node anywhere has no
/// symbolic carrier and the path fails closed.
fn symbolic_size_index(types: &TypeTable, name: Symbol) -> Option<IndexId> {
    (0..types.index_count()).find_map(|index| {
        #[expect(clippy::cast_possible_truncation, reason = "IndexId is a u32 arena index by design")]
        let id = IndexId(index as u32);
        matches!(types.get_index(id), IndexExpr::Param(found) if *found == name).then_some(id)
    })
}

#[derive(Debug, Clone, Copy)]
struct LocalBinding {
    local: MirLocalId,
    ty: MirType,
}

struct OpenBlock {
    id: MirBlockId,
    statements: Vec<MirStatement>,
    terminator: Option<MirTerminator>,
    span: Span,
}

#[derive(Debug, Clone, Copy)]
struct BreakableContext {
    perge_target: Option<MirBlockId>,
    /// Exit edge for `rumpe` (and breakable `fac` fall-through).
    rumpe_target: MirBlockId,
    /// Whether any `rumpe` actually targeted this scope's exit edge. Used to
    /// decide if the after-block stays live when the body's straight-line
    /// fall-through is dead (e.g. a body whose only effect is `rumpe`).
    rumpe_used: bool,
}

/// Active `summa ex` sequential fold (SFR-4): a valued `redde` inside the
/// term body adds into `accumulator` with `+` and continues the iteration
/// loop instead of returning from the enclosing function.
#[derive(Debug, Clone, Copy)]
struct SummaFoldContext {
    accumulator: FoldAccumulator,
    ty: MirType,
}

/// Where a fold keeps its running value. The sequential `summa` fold uses a
/// temp so every writer is an exact integer operation or copy and backends
/// keep the running total exact; the filum capture keeps a plain local.
#[derive(Debug, Clone, Copy)]
enum FoldAccumulator {
    Local(MirLocalId),
    Temp(MirTempId),
}

impl FoldAccumulator {
    fn place(self) -> MirPlace {
        match self {
            Self::Local(id) => MirPlace::local(id),
            Self::Temp(id) => MirPlace::temp(id),
        }
    }

    fn operand(self) -> MirOperand {
        match self {
            Self::Local(id) => MirOperand::Place(MirPlace::local(id)),
            Self::Temp(id) => MirOperand::Temp(id),
        }
    }
}

#[derive(Debug, Clone)]
struct HandlerContext {
    error_place: MirPlace,
    error_ty: MirType,
    error_block: MirBlockId,
    /// DFS push order of this capture boundary (shared counter with
    /// `TrapContext`), so mixed `fac { capta { … } }` nesting routes each
    /// alternate exit to the innermost boundary.
    depth: usize,
}

/// Active `capta { ... }` capture boundary (UT): failable calls and `iace`
/// inside the body lower to `TryCall` whose error block wraps the payload
/// into the trap carrier (the union-typed destination) instead of entering a
/// cape body or reaching `ReturnError`.
#[derive(Debug, Clone)]
struct TrapContext {
    /// Union-typed destination the wrapped result is assigned into.
    destination: MirPlace,
    /// Join block both arms converge on.
    join_block: MirBlockId,
    /// Whether any error wrap block already targets `join_block` (keeps the
    /// reachability decision honest when the body itself diverges).
    error_paths: bool,
    /// DFS push order (shared counter with `HandlerContext`).
    depth: usize,
}

#[derive(Debug, Clone)]
struct ProviderImport {
    kind: MirProviderKind,
    module: Vec<Symbol>,
    item: Symbol,
    /// Package-resolved file-interface call targets (`verb → synthesized
    /// external-definition id`) when the import resolves to a sibling
    /// user-code module under package identity context (D-PA1). `None` keeps
    /// the provider route (raw import spelling).
    file_interface_functions: Option<FxHashMap<Symbol, DefId>>,
}

#[derive(Debug, Clone, Copy)]
struct MethodTarget {
    def_id: DefId,
}

#[derive(Debug, Clone)]
struct ImportedMethodProvider {
    provider: ProviderImport,
    canonical_name: Symbol,
}

/// Immutable whole-unit facts needed while lowering one function body.
///
/// This context is intentionally cloned into each [`FunctionBuilder`] instead of
/// letting builders mutate global lowering state. Per-function lowering can then
/// accumulate local errors and CFG state while still resolving function errors,
/// aggregate fields, enum payload names, and provider imports consistently.
struct FunctionBuilderContext<'a> {
    interner: Option<&'a Interner>,
    empty_textus: Symbol,
    diagnostic_format_templates: [Symbol; 3],
    diagnostic_separator: Symbol,
    diagnostic_nihil: Symbol,
    /// Lockstep `itera ex` runtime admission message: the
    /// fail-closed equal-length guard's panic textus.
    diagnostic_lockstep_length_mismatch: Symbol,
    /// Failed `numerus ↦ Ordo` payload textus (D9.4).
    diagnostic_ordo_no_member: Symbol,
    diagnostic_potentia_negative_exponent: Symbol,
    function_errors: Arc<FxHashMap<DefId, MirType>>,
    function_async: Arc<FxHashMap<DefId, bool>>,
    function_generator: Arc<FxHashMap<DefId, bool>>,
    function_params: Arc<FxHashMap<DefId, Vec<context::CallableParamMeta<'a>>>>,
    structs: Arc<FxHashMap<DefId, Vec<&'a HirField>>>,
    /// Producer-side defaults for imported class fields.
    imported_struct_defaults: Arc<FxHashMap<DefId, Vec<(Symbol, HirExpression)>>>,
    /// Field-name → MirType metadata for EVERY struct the analysis knows
    /// (local + imported file-interface structs). Local structs are also in
    /// [`Self::structs`]; imported structs exist only here. Aggregate
    /// materialization falls back to this map for zero-init of imported
    /// struct-typed values (FMIR e2e-hardening CTO-1).
    struct_field_types: Arc<FxHashMap<DefId, FxHashMap<crate::lexer::Symbol, MirType>>>,
    variant_parents: Arc<FxHashMap<DefId, DefId>>,
    /// Enum declaration → ordered variant defs (local + imported file-interface
    /// enums). The no-initializer `varia` zero-init picks the first variant.
    enum_variants: Arc<FxHashMap<DefId, Vec<DefId>>>,
    enum_type_params: Arc<FxHashMap<DefId, Vec<Symbol>>>,
    variant_fields: Arc<FxHashMap<DefId, Vec<crate::lexer::Symbol>>>,
    variant_field_tys: Arc<FxHashMap<DefId, FxHashMap<crate::lexer::Symbol, MirType>>>,
    /// Registry conversion rows keyed by ordered pair `(source →
    /// destination)`: the coherence registry's user rows (CTR-06). The
    /// failable-call recovery path uses these to re-select the registered
    /// function when a package rewrite dropped the call-site conversion.
    conversion_rows: Arc<FxHashMap<ConversionPair, DefId>>,
    sermo_def: Option<DefId>,
    meus_def: Option<DefId>,
    tuus_def: Option<DefId>,
    scrinium_def: Option<DefId>,
    scrinium_data_field: Option<Symbol>,
    top_level_consts: Arc<FxHashMap<DefId, &'a HirConst>>,
    provider_imports: Arc<FxHashMap<DefId, ProviderImport>>,
    imported_type_members: Arc<FxHashMap<(DefId, Symbol), MirType>>,
    method_targets: Arc<FxHashMap<(DefId, Symbol), MethodTarget>>,
    interface_implementors: Arc<FxHashMap<DefId, Vec<(DefId, TypeId)>>>,
    imported_method_providers: Arc<FxHashMap<(DefId, Symbol), ImportedMethodProvider>>,
    gpu_builtins: Arc<FxHashMap<DefId, MirKernelBuiltin>>,
    cli_record_fields_by_local: Arc<FxHashMap<Symbol, Vec<MirRuntimeRecordField>>>,
    /// Descriptor-derived CLI record fields keyed by `argumenta` record type;
    /// the shared default-path fallback when the caller supplied no parsed
    /// runtime fields for a CLI record local.
    cli_record_fields_by_type: Arc<FxHashMap<TypeId, Vec<MirRuntimeRecordField>>>,
    /// S8.2: CLI-record locals/params are constructed from runtime-filled
    /// temps the emitted LLVM adapter seeds from the static descriptor parse
    /// table (the compile-time field values are only shape placeholders).
    cli_adapter_active: bool,
}

impl<'a> FunctionBuilderContext<'a> {
    /// Borrow-free snapshot of callee param metadata for call lowering.
    ///
    /// `lower_direct_call_args` must lower default expressions while mutably
    /// borrowing the builder, so it cannot hold an immutable borrow of
    /// `function_params` on `self`. Extract the small `(optional, default)`
    /// pairs first instead of cloning the full metadata vec.
    fn callable_param_lower_meta(&self, callee: DefId) -> Option<Vec<(bool, Option<&'a HirExpression>)>> {
        self.function_params.get(&callee).map(|params| {
            params
                .iter()
                .map(|param| (param.optional, param.default))
                .collect()
        })
    }

    #[cfg(test)]
    fn empty() -> Self {
        Self {
            interner: None,
            empty_textus: Symbol(0),
            diagnostic_format_templates: [Symbol(0); 3],
            diagnostic_separator: Symbol(0),
            diagnostic_nihil: Symbol(0),
            diagnostic_lockstep_length_mismatch: Symbol(0),
            diagnostic_ordo_no_member: Symbol(0),
            diagnostic_potentia_negative_exponent: Symbol(0),
            function_errors: Arc::new(FxHashMap::default()),
            function_async: Arc::new(FxHashMap::default()),
            function_generator: Arc::new(FxHashMap::default()),
            function_params: Arc::new(FxHashMap::default()),
            structs: Arc::new(FxHashMap::default()),
            imported_struct_defaults: Arc::new(FxHashMap::default()),
            struct_field_types: Arc::new(FxHashMap::default()),
            variant_parents: Arc::new(FxHashMap::default()),
            enum_variants: Arc::new(FxHashMap::default()),
            enum_type_params: Arc::new(FxHashMap::default()),
            variant_fields: Arc::new(FxHashMap::default()),
            variant_field_tys: Arc::new(FxHashMap::default()),
            conversion_rows: Arc::new(FxHashMap::default()),
            sermo_def: None,
            meus_def: None,
            tuus_def: None,
            scrinium_def: None,
            scrinium_data_field: None,
            top_level_consts: Arc::new(FxHashMap::default()),
            provider_imports: Arc::new(FxHashMap::default()),
            imported_type_members: Arc::new(FxHashMap::default()),
            method_targets: Arc::new(FxHashMap::default()),
            interface_implementors: Arc::new(FxHashMap::default()),
            imported_method_providers: Arc::new(FxHashMap::default()),
            gpu_builtins: Arc::new(FxHashMap::default()),
            cli_record_fields_by_local: Arc::new(FxHashMap::default()),
            cli_record_fields_by_type: Arc::new(FxHashMap::default()),
            cli_adapter_active: false,
        }
    }
}

/// Collect top-level constant paths before CFG lowering so each cached local
/// receives its assignment in the function entry block.
struct TopLevelConstCollector<'a> {
    top_level_consts: &'a FxHashMap<DefId, &'a HirConst>,
    found: Vec<DefId>,
    seen: FxHashSet<DefId>,
}

impl HirVisitor for TopLevelConstCollector<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Path(def_id) = expr.kind
            && self.top_level_consts.contains_key(&def_id)
            && self.seen.insert(def_id)
        {
            self.found.push(def_id);
        }
        walk_expr(self, expr);
    }
}

/// Stateful builder for one MIR function.
///
/// The builder translates typed HIR body structure into MIR locals, temporaries,
/// values, and basic blocks. Its mutable state models the current CFG frontier:
/// `current` is `None` after a terminator, `breakables` records `perge`/`rumpe`
/// targets, and `handlers` records active alternate-exit handlers. Any attempt
/// to keep emitting after a block is sealed becomes a lowering error.
struct FunctionBuilder<'a> {
    types: &'a TypeTable,
    error_ty: Option<MirType>,
    context: FunctionBuilderContext<'a>,
    bindings: FxHashMap<DefId, LocalBinding>,
    params: Vec<MirParam>,
    locals: Vec<MirLocalDecl>,
    temps: Vec<MirTemp>,
    blocks: Vec<OpenBlock>,
    current: Option<MirBlockId>,
    breakables: Vec<BreakableContext>,
    summa_fold: Option<SummaFoldContext>,
    /// Active `summa … filum` term capture (SFR-5): a valued `redde` inside
    /// the captured term body stores the term value into `accumulator` and
    /// exits the body — the statements land in the filum plan entry, not the
    /// kernel's inline flow.
    filum_capture: Option<SummaFoldContext>,
    handlers: Vec<HandlerContext>,
    /// Active `capta` trap boundaries (UT); see [`TrapContext`].
    traps: Vec<TrapContext>,
    /// Monotonic DFS order shared by handler and trap pushes, so
    /// `innermost_capture` picks the lexically innermost boundary.
    capture_depth: usize,
    materializing_consts: FxHashSet<DefId>,
    /// `object!` method receivers lowered inside the call being lowered:
    /// `(storage place, asserted payload temp, storage type)`. A receiver
    /// write lands on the payload temp, so a mutating call stores the payload
    /// back into the storage afterwards (`lower_method_call`).
    asserted_receivers: Vec<(MirPlace, MirPlace, MirType)>,
    errors: Vec<MirError>,
    next_value: u32,
    synthetic_functions: Vec<MirFunction>,
    closure_environments: Vec<MirClosureEnvironment>,
    device_function_metadata: MirDeviceFunctionMetadataMap,
    next_kernel_closure_ordinal: u32,
    closure_temp_callees: FxHashMap<MirTempId, MirClosureCallee>,
    closure_local_callees: FxHashMap<MirLocalId, MirClosureCallee>,
    kernel_temp_callees: FxHashMap<MirTempId, MirFunctionId>,
    kernel_local_callees: FxHashMap<MirLocalId, MirFunctionId>,
    captured_bindings: FxHashMap<DefId, MirPlace>,
    captured_binding_metadata: FxHashMap<DefId, (Symbol, MirType)>,
    /// `var` locals of this body that closures capture and that are rebound
    /// somewhere: their storage is a shared cell (operator ruling 2026-09-24,
    /// unmarked closures share the enclosing scope).
    shared_bindings: FxHashSet<DefId>,
    /// In-scope shared bindings → the place holding the cell itself (the
    /// declaring local, or a closure-environment capture slot). Paths read and
    /// write the content through `captured_bindings` (`cell.SharedCell`);
    /// nested closures capture the cell from here.
    shared_cells: FxHashMap<DefId, MirPlace>,
    /// Enclosing callable's `magnitudo` parameters, def → name. A body path
    /// to one of these defs lowers to the symbolic size operand, never a
    /// runtime local (sgr C2a).
    size_params: FxHashMap<DefId, Symbol>,
    /// Enclosing callable's contract-bound methods (D1.1), keyed by
    /// `(type parameter, method)`: the contract method a `T` receiver call
    /// targets until monomorphization picks the witness's own method.
    bound_methods: FxHashMap<(Symbol, Symbol), DefId>,
    parent_function_id: MirFunctionId,
    device_role: Option<MirDeviceRole>,
    shader_stage: Option<MirKernelShaderStage>,
    /// `copy` parameters (D-6): `(param local, owned duplicate local, type,
    /// span)`. The body binds the duplicate; entry emits the deep copy.
    copy_param_entries: Vec<(MirLocalId, MirLocalId, MirType, Span)>,
}

impl<'a> FunctionBuilder<'a> {
    #[cfg(test)]
    fn new(types: &'a TypeTable) -> Self {
        Self::for_function(types, None, FunctionBuilderContext::empty(), MirFunctionId(0), None, None)
    }

    fn for_function(
        types: &'a TypeTable,
        error_ty: Option<MirType>,
        context: FunctionBuilderContext<'a>,
        parent_function_id: MirFunctionId,
        device_role: Option<MirDeviceRole>,
        shader_stage: Option<MirKernelShaderStage>,
    ) -> Self {
        Self {
            types,
            error_ty,
            context,
            bindings: FxHashMap::default(),
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: Vec::new(),
            current: None,
            breakables: Vec::new(),
            summa_fold: None,
            filum_capture: None,
            handlers: Vec::new(),
            traps: Vec::new(),
            capture_depth: 0,
            materializing_consts: FxHashSet::default(),
            asserted_receivers: Vec::new(),
            errors: Vec::new(),
            next_value: 0,
            synthetic_functions: Vec::new(),
            closure_environments: Vec::new(),
            device_function_metadata: MirDeviceFunctionMetadataMap::default(),
            next_kernel_closure_ordinal: 0,
            closure_temp_callees: FxHashMap::default(),
            closure_local_callees: FxHashMap::default(),
            kernel_temp_callees: FxHashMap::default(),
            kernel_local_callees: FxHashMap::default(),
            captured_bindings: FxHashMap::default(),
            captured_binding_metadata: FxHashMap::default(),
            shared_bindings: FxHashSet::default(),
            shared_cells: FxHashMap::default(),
            size_params: FxHashMap::default(),
            bound_methods: FxHashMap::default(),
            parent_function_id,
            device_role,
            shader_stage,
            copy_param_entries: Vec::new(),
        }
    }

    fn is_device_lowering(&self) -> bool {
        self.device_role.is_some()
    }

    /// The innermost active `capta` trap when it is the innermost capture
    /// boundary (deeper than any active `cape` handler), marking that an
    /// error wrap block will target the trap join. `None` keeps the existing
    /// handler / propagation routing.
    fn take_innermost_trap(&mut self) -> Option<TrapContext> {
        let handler_depth = self.handlers.last().map_or(0, |handler| handler.depth);
        let trap = self.traps.last().cloned()?;
        if trap.depth > handler_depth {
            self.traps
                .last_mut()
                .expect("trap context present")
                .error_paths = true;
            Some(trap)
        } else {
            None
        }
    }

    fn add_param(&mut self, def_id: DefId, name: crate::lexer::Symbol, ty: TypeId, mode: HirParamMode, span: Span) {
        let local = self.next_local_id();
        // MIR carries access intent in `MirParamMode`; the local slot carries
        // the payload value type so MIR calls, returns, and places stay in the
        // storage domain. Every type-position access marker is a source-level
        // wrapper, including `own`/`copy` even though HIR collapses those to
        // value-owned mode.
        let (copy_param, storage_ty) = match self.normalized_type(ty) {
            Type::Ref(mutability, inner) => (*mutability == SemanticMutability::Copy, *inner),
            _ => (false, ty),
        };
        let mir_ty = MirType::semantic(storage_ty);
        // Keep the MIR ABI mode and local mutability paired. In particular,
        // `in` must remain `MirParamMode::MutRef` so call frames can preserve a
        // scalar mutable borrow instead of treating it as an owned value.
        let (mir_mode, mutable) = match mode {
            HirParamMode::Owned => (MirParamMode::Owned, false),
            HirParamMode::De => (MirParamMode::Ref, false),
            HirParamMode::In => (MirParamMode::MutRef, true),
            HirParamMode::Ex => (MirParamMode::Move, false),
        };
        self.params
            .push(MirParam { local, name: Some(name), ty: mir_ty, mode: mir_mode, span });
        self.locals
            .push(MirLocalDecl { id: local, name: Some(name), ty: mir_ty, mutable, span });
        if copy_param {
            // `copy` (D-6): the callee receives a fresh owned duplicate at
            // entry; the caller's value is never shared with the body.
            let duplicate = self.next_local_id();
            self.locals
                .push(MirLocalDecl { id: duplicate, name: Some(name), ty: mir_ty, mutable: true, span });
            self.copy_param_entries
                .push((local, duplicate, mir_ty, span));
            self.bindings
                .insert(def_id, LocalBinding { local: duplicate, ty: mir_ty });
            return;
        }
        self.bindings
            .insert(def_id, LocalBinding { local, ty: mir_ty });
    }

    /// Emit the entry deep copy for every `copy` parameter (D-6).
    fn emit_copy_param_entries(&mut self) {
        for (param, duplicate, ty, span) in std::mem::take(&mut self.copy_param_entries) {
            self.runtime_call_to_place(
                Some(MirPlace::local(duplicate)),
                MirIntrinsic::Duplicate,
                vec![MirOperand::Place(MirPlace::local(param))],
                ty,
                span,
            );
        }
    }

    fn add_lane_param(&mut self, def_id: DefId, name: crate::lexer::Symbol, ty: TypeId, span: Span) {
        self.add_lane_param_with_mode(def_id, name, ty, span, MirParamMode::Lane);
    }

    fn add_inherited_lane_param(&mut self, def_id: DefId, name: crate::lexer::Symbol, ty: TypeId, span: Span) {
        self.add_lane_param_with_mode(def_id, name, ty, span, MirParamMode::LaneInherited);
    }

    fn add_lane_param_with_mode(
        &mut self,
        def_id: DefId,
        name: crate::lexer::Symbol,
        ty: TypeId,
        span: Span,
        mode: MirParamMode,
    ) {
        let local = self.next_local_id();
        let mir_ty = MirType::semantic(ty);
        self.params
            .push(MirParam { local, name: Some(name), ty: mir_ty, mode, span });
        self.locals
            .push(MirLocalDecl { id: local, name: Some(name), ty: mir_ty, mutable: false, span });
        self.bindings
            .insert(def_id, LocalBinding { local, ty: mir_ty });
    }

    /// Inject synthetic `MirLocalDecl`s (name, type, mutable) for a
    /// `@vertex` function, and create `DefId`→local bindings so HIR paths
    /// in the function body (e.g. `position`, `out_position`) resolve to
    /// these injected MIR locals.
    ///
    /// The item list is pre-built in [`lower_analyzed_unit`].
    #[cfg(test)]
    fn add_test_binding(&mut self, def_id: DefId, ty: MirType, mutable: bool) -> MirLocalId {
        let local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: local, name: None, ty, mutable, span: Span::default() });
        self.bindings.insert(def_id, LocalBinding { local, ty });
        local
    }

    fn lower_body(&mut self, body: &HirBlock) -> Vec<MirBlock> {
        self.lower_body_with_cli_args(body, None)
    }

    fn lower_body_with_cli_args(&mut self, body: &HirBlock, cli_args: Option<&crate::hir::HirParam>) -> Vec<MirBlock> {
        self.shared_bindings = radix_hir::capture::CaptureFacts::for_block(body).shared;
        let entry = self.fresh_block(body.span);
        self.switch_to(entry);
        self.emit_copy_param_entries();
        if let Some(cli_args) = cli_args {
            self.add_runtime_record_local(cli_args);
        }
        self.materialize_referenced_top_level_consts(body);
        self.visit_block(body);
        self.terminate_open_current(MirTerminatorKind::Return(None), body.span);
        self.finish_blocks()
    }

    /// Materialize each top-level constant used by this body before lowering
    /// any control flow. A lazy first-use binding is not safe when that first
    /// path sits inside a branch or loop that can be skipped: later uses share
    /// the cached local but would then read it before its assignment.
    fn materialize_referenced_top_level_consts(&mut self, body: &HirBlock) {
        let mut collector = TopLevelConstCollector {
            top_level_consts: &self.context.top_level_consts,
            found: Vec::new(),
            seen: FxHashSet::default(),
        };
        collector.visit_block(body);

        for def_id in collector.found {
            let Some(konst) = self.context.top_level_consts.get(&def_id).copied() else {
                continue;
            };
            self.lower_top_level_const_binding(def_id, konst, body.span);
        }
    }

    fn add_runtime_record_local(&mut self, param: &crate::hir::HirParam) -> Option<()> {
        let mir_ty = MirType::semantic(param.ty);
        let id = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id, name: Some(param.name), ty: mir_ty, mutable: true, span: param.span });
        if matches!(self.types.get(param.ty), Type::Record(_)) {
            self.lower_cli_record_initializer(id, param.name, param.ty, mir_ty, param.span);
        } else {
            self.errors
                .push(MirError::unsupported(param.span, "non-record CLI argument binding"));
        }
        self.bindings
            .insert(param.def_id, LocalBinding { local: id, ty: mir_ty });
        Some(())
    }

    /// S8.2: one runtime-filled temp per record field, in the record type's
    /// field order, so the emitted CLI adapter can seed the record construct
    /// from the static descriptor parse table.
    fn runtime_adapter_record_fields(&mut self, record_ty: TypeId, span: Span) -> Vec<MirNamedOperand> {
        let fields = match self.types.get(record_ty) {
            Type::Record(fields) => fields.clone(),
            _ => return Vec::new(),
        };
        fields
            .into_iter()
            .map(|(name, field_ty)| {
                let ty = MirType::semantic(field_ty);
                let temp = self.push_temp(ty, span);
                MirNamedOperand { name, value: MirOperand::Temp(temp) }
            })
            .collect()
    }

    fn runtime_record_fields(&mut self, fields: Vec<MirRuntimeRecordField>, span: Span) -> Vec<MirNamedOperand> {
        fields
            .into_iter()
            .map(|field| {
                let value = match field.value {
                    MirRuntimeRecordValue::Operand(value) => value,
                    MirRuntimeRecordValue::Array { ty, items } => self.construct_temp(
                        MirAggregateKind::Array,
                        MirAggregateFields::Ordered(items.into_iter().map(MirAggregateItem::Operand).collect()),
                        ty,
                        span,
                    ),
                };
                MirNamedOperand { name: field.name, value }
            })
            .collect()
    }

    /// Descriptor-derived CLI record fields for a record type (shared
    /// default-path fallback). Returns an empty surface for non-CLI record
    /// locals, matching the pre-fallback behavior.
    fn cli_record_fields_for_type(&mut self, ty: TypeId, span: Span) -> Vec<MirNamedOperand> {
        let Some(fields) = self.context.cli_record_fields_by_type.get(&ty).cloned() else {
            return Vec::new();
        };
        self.runtime_record_fields(fields, span)
    }

    /// Lower a CLI-bound record (param or runtime-provided local) into a
    /// constructed record aggregate assigned to `id`. Field sources: S8.2
    /// runtime-filled adapter temps, parsed runtime fields for this local, or
    /// descriptor-derived fields keyed by the record type.
    fn lower_cli_record_initializer(&mut self, id: MirLocalId, name: Symbol, ty: TypeId, mir_ty: MirType, span: Span) {
        let fields = if self.context.cli_adapter_active {
            // S8.2: the record reads runtime-filled temps; the emitted adapter
            // seeds them from the typed value table.
            self.runtime_adapter_record_fields(ty, span)
        } else if let Some(fields) = self.context.cli_record_fields_by_local.get(&name).cloned() {
            self.runtime_record_fields(fields, span)
        } else {
            self.cli_record_fields_for_type(ty, span)
        };
        let value = self.construct_temp(MirAggregateKind::Record, MirAggregateFields::Named(fields), mir_ty, span);
        self.assign(MirPlace::local(id), value, mir_ty, span);
    }

    fn lower_local(&mut self, local: &HirLocal, span: Span) -> Option<()> {
        if local.runtime_provided && self.bindings.contains_key(&local.def_id) {
            return Some(());
        }

        let Some(ty) = local.ty else {
            self.errors
                .push(MirError::missing_type(span, "local declaration"));
            return None;
        };

        // A `copy` bind elaborates to an exclusive owned value (D-6): the
        // local stores the payload type, like a `copy` parameter.
        let copy_bind = self.types.is_copy_view(ty);
        let mir_ty = if copy_bind {
            MirType::semantic(self.types.peel_access_marker(ty).1)
        } else {
            MirType::semantic(ty)
        };
        let id = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id, name: Some(local.name), ty: mir_ty, mutable: local.mutable, span });

        if local.mutable && self.shared_bindings.contains(&local.def_id) {
            return self.lower_shared_local(local, id, ty, mir_ty, span);
        }

        if let Some(init) = &local.init {
            if let HirExpressionKind::ConversioAssign { source, recovery, .. } = &init.kind {
                // Typed `↤` initializer (`fixum numerus n ↤ "42"`): the node's
                // target is this binding, so the destination is the local we
                // just declared. Convert the source to the declared type and
                // store once — no self-reference to resolve.
                self.lower_conversio_assign_to_destination(
                    source,
                    recovery.as_deref(),
                    MirPlace::local(id),
                    mir_ty,
                    span,
                    init,
                )?;
            } else if let Some(site) = &local.await_store {
                self.lower_await_store(init, MirPlace::local(id), mir_ty, site.clone(), span)?;
            } else if copy_bind {
                // `copy` bind (D-6): deep-duplicate the initializer; the
                // source stays live and shares nothing with the binding.
                let source = self.lower_expr_value(init)?;
                self.runtime_call_to_place(
                    Some(MirPlace::local(id)),
                    MirIntrinsic::Duplicate,
                    vec![source],
                    mir_ty,
                    span,
                );
            } else {
                self.lower_expr_to_destination(init, MirPlace::local(id), mir_ty)?;
            }
        } else if local.runtime_provided && matches!(self.types.get(ty), Type::Record(_)) {
            self.lower_cli_record_initializer(id, local.name, ty, mir_ty, span);
        } else if local.runtime_provided {
            // Runtime-provided vertex shader locals (position, out_position)
            // are bound by the runtime, not by an initializer or zero-init.
        } else if local.mutable {
            // M-007: `varia` without initializer receives a type zero at the decl site.
            let zero = self.type_zero_operand(ty, span)?;
            self.assign(MirPlace::local(id), zero, mir_ty, span);
        }
        // `fixum`/`sit` deferred-init (`!mutable`, no init`) stays unassigned until `←`.
        self.bindings
            .insert(local.def_id, LocalBinding { local: id, ty: mir_ty });
        Some(())
    }

    /// Declare a captured-and-rebound `var` as a fresh shared cell.
    ///
    /// The initializer (or type zero) lowers into a staging temp, then one
    /// `SharedCell` construct stores the cell in the declared local. The
    /// construct runs at every execution of the declaration, so a loop body
    /// gets a fresh cell per iteration. Every path to the binding then goes
    /// through `local.SharedCell`, and closures capture the cell itself.
    fn lower_shared_local(
        &mut self,
        local: &HirLocal,
        id: MirLocalId,
        ty: TypeId,
        mir_ty: MirType,
        span: Span,
    ) -> Option<()> {
        let staging = self.push_temp(mir_ty, span);
        let staging_place = MirPlace::temp(staging);
        if let Some(init) = &local.init {
            if let HirExpressionKind::ConversioAssign { source, recovery, .. } = &init.kind {
                self.lower_conversio_assign_to_destination(
                    source,
                    recovery.as_deref(),
                    staging_place.clone(),
                    mir_ty,
                    span,
                    init,
                )?;
            } else if let Some(site) = &local.await_store {
                self.lower_await_store(init, staging_place.clone(), mir_ty, site.clone(), span)?;
            } else {
                self.lower_expr_to_destination(init, staging_place.clone(), mir_ty)?;
            }
        } else {
            let zero = self.type_zero_operand(ty, span)?;
            self.assign(staging_place.clone(), zero, mir_ty, span);
        }
        self.append_stmt(MirStatement {
            kind: MirStatementKind::Construct {
                destination: MirPlace::local(id),
                aggregate: MirAggregate {
                    kind: MirAggregateKind::SharedCell,
                    ty: mir_ty,
                    fields: MirAggregateFields::Ordered(vec![MirAggregateItem::Operand(MirOperand::Place(
                        staging_place,
                    ))]),
                },
            },
            span,
        });
        let cell = MirPlace::local(id);
        let mut content = cell.clone();
        content.projections.push(MirProjection::SharedCell);
        self.captured_bindings.insert(local.def_id, content);
        self.captured_binding_metadata
            .insert(local.def_id, (local.name, mir_ty));
        self.shared_cells.insert(local.def_id, cell);
        self.bindings
            .insert(local.def_id, LocalBinding { local: id, ty: mir_ty });
        Some(())
    }

    fn lower_const_binding(&mut self, item_def_id: DefId, konst: &HirConst, span: Span) -> Option<()> {
        let Some(ty) = konst.ty else {
            self.errors
                .push(MirError::missing_type(span, "top-level const declaration"));
            return None;
        };

        let mir_ty = MirType::semantic(ty);
        let id = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id, name: Some(konst.name), ty: mir_ty, mutable: false, span });
        if let HirExpressionKind::ConversioAssign { source, recovery, .. } = &konst.value.kind {
            // Typed `↤` initializer: the destination is the const binding just
            // declared, so convert the source to its declared type and store once.
            self.lower_conversio_assign_to_destination(
                source,
                recovery.as_deref(),
                MirPlace::local(id),
                mir_ty,
                span,
                &konst.value,
            )?;
        } else {
            self.lower_expr_to_destination(&konst.value, MirPlace::local(id), mir_ty)?;
        }
        self.bindings
            .insert(item_def_id, LocalBinding { local: id, ty: mir_ty });
        Some(())
    }

    fn lower_top_level_const_binding(
        &mut self,
        item_def_id: DefId,
        konst: &HirConst,
        span: Span,
    ) -> Option<LocalBinding> {
        if let Some(binding) = self.bindings.get(&item_def_id).copied() {
            return Some(binding);
        }
        if !self.materializing_consts.insert(item_def_id) {
            self.errors
                .push(MirError::unsupported(span, "recursive top-level const before MIR lowering"));
            return None;
        }

        let Some(ty) = konst.ty else {
            self.errors
                .push(MirError::missing_type(span, "top-level const declaration"));
            self.materializing_consts.remove(&item_def_id);
            return None;
        };

        let mir_ty = MirType::semantic(ty);
        let id = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id, name: Some(konst.name), ty: mir_ty, mutable: false, span });
        let lowered = self.lower_expr_to_destination(&konst.value, MirPlace::local(id), mir_ty);
        self.materializing_consts.remove(&item_def_id);
        lowered?;
        let binding = LocalBinding { local: id, ty: mir_ty };
        self.bindings.insert(item_def_id, binding);
        Some(binding)
    }

    fn lower_assignment_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        let HirExpressionKind::Assign(lhs, rhs) = &expr.kind else {
            return self.lower_expr_value(expr);
        };

        if let Some(result) = self.lower_tensor_bracket_set(lhs, rhs, expr.span) {
            return Some(result);
        }
        if let Some(result) = self.lower_union_field_assign(lhs, rhs, expr.span) {
            return result;
        }

        let fallback_ty = self.expr_ty(rhs)?;
        let (place, ty) = self.lower_assignment_place_with_fallback(lhs, fallback_ty)?;
        self.lower_expr_to_destination(rhs, place.clone(), ty)?;
        Some(MirOperand::Place(place))
    }

    /// Lower conversion-directed assignment (`target ↤ source`, U+21A4).
    ///
    /// Eval order matches ordinary `←` exactly, with the conversion inserted
    /// between source evaluation and the store:
    ///   1. destination place evaluated once (receiver/index once),
    ///   2. source evaluated once,
    ///   3. converted once through the shared `↦` conversion owner
    ///      (`lower_conversio`) — `⊥` default runs only on conversion failure,
    ///   4. stored exactly once.
    ///
    /// The result value denotes the stored value (the destination place), the
    /// same contract as ordinary assignment — no `↤`-specific exception.
    /// There is no codegen-only rewrite that duplicates any evaluation.
    fn lower_conversio_assign_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        let HirExpressionKind::ConversioAssign { target, source, recovery } = &expr.kind else {
            return self.lower_expr_value(expr);
        };
        let fallback_ty = self.expr_ty(source)?;
        let (place, ty) = self.lower_assignment_place_with_fallback(target, fallback_ty)?;
        self.lower_conversio_assign_to_destination(source, recovery.as_deref(), place.clone(), ty, expr.span, expr)?;
        Some(MirOperand::Place(place))
    }

    /// Lower the conversion half of `↤` directly into a known destination.
    ///
    /// Used by typed `fixum`/`varia` initializers (and constants) where the
    /// `↤` node's target IS the binding being declared, so there is no
    /// self-reference to resolve: source evaluated once, converted once through
    /// the shared `↦` conversion owner, stored exactly once.
    fn lower_conversio_assign_to_destination(
        &mut self,
        source: &HirExpression,
        recovery: Option<&HirExpression>,
        destination: MirPlace,
        ty: MirType,
        span: Span,
        expr: &HirExpression,
    ) -> Option<()> {
        let converted = self.lower_conversio(source, ty.semantic_id(), &[], recovery, None, expr)?;
        self.assign(destination, converted, ty, span);
        Some(())
    }

    fn lower_tensor_bracket_set(&mut self, lhs: &HirExpression, rhs: &HirExpression, span: Span) -> Option<MirOperand> {
        if self.is_device_lowering() {
            return None;
        }
        let HirExpressionKind::Index(object, index) = &lhs.kind else {
            return None;
        };
        if !object
            .ty
            .is_some_and(|ty| crate::semantic::tensor_bracket::is_tensor_type(self.types, ty))
        {
            return None;
        }

        let receiver = self.lower_expr_value(object)?;
        let index = self.lower_tensor_index_list_operand(index)?;
        let value = self.lower_expr_value(rhs)?;
        let vacuum = MirType::semantic(self.types.primitive(Primitive::Vacuum));
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorSet),
            vec![receiver, index, value],
            vacuum,
            span,
        ))
    }

    fn lower_inc_dec_stmt(&mut self, span: Span, inc_dec: &crate::hir::HirIncDec) {
        let Some(fallback_ty) = self.expr_ty(&inc_dec.target) else {
            return;
        };
        let Some((place, ty)) = self.lower_assignment_place_with_fallback(&inc_dec.target, fallback_ty) else {
            return;
        };
        let bin_op = match inc_dec.op {
            crate::hir::HirIncDecOp::Inc => MirBinOp::Add,
            crate::hir::HirIncDecOp::Dec => MirBinOp::Sub,
        };
        let lhs = MirOperand::Place(place.clone());
        let rhs = MirOperand::Constant(MirConstant::Int(1));
        let value = self.assign_temp(MirValueKind::Binary { op: bin_op, lhs, rhs }, ty, span);
        // `place ↑` is a store (d11-6 row 4): the exact successor lands in the
        // slot under the slot's policy, and a failure has the store identity.
        let value = if !self.is_device_lowering() && self.is_integer_store_slot(ty) {
            let slot = match &inc_dec.target.kind {
                HirExpressionKind::Path(_) | HirExpressionKind::Field(..) => place
                    .projections
                    .iter()
                    .rev()
                    .find_map(|projection| match projection {
                        MirProjection::Field(name) => Some(*name),
                        _ => None,
                    })
                    .or_else(|| match place.base {
                        MirPlaceBase::Local(id) => self
                            .locals
                            .iter()
                            .find(|local| local.id == id)
                            .and_then(|local| local.name),
                        MirPlaceBase::Temp(_) => None,
                    }),
                _ => None,
            };
            let site = radix_hir::HirStoreSite {
                kind: radix_hir::HirStoreKind::Assignment,
                slot,
                inferred_from: None,
                inferred_text: None,
            };
            self.integer_store_convert(value, ty, site, span)
        } else {
            value
        };
        self.assign(place, value, ty, span);
    }

    /// Whether `ty` is an integer slot whose policy a store applies: a
    /// trapping `numerus`, a `wrapping` or a `saturating` word.
    fn is_integer_store_slot(&self, ty: MirType) -> bool {
        match self.normalized_type(self.types.peel_access_marker(ty.semantic_id()).1) {
            // Every integer slot takes the node: at the cap the exact successor
            // (`i64::MAX + 1`) is a value the carrier holds, so only the store
            // check keeps it out of an `i64` slot (ruling 17).
            Type::ModularWord(_) | Type::SaturatingWord(_) | Type::Primitive(Primitive::Numerus) => true,
            // Deliberate: a store into `inf` is total (nothing to fit), so it
            // takes no store node; `integer_range()` is `None` for `inf`.
            Type::SizedNumeric(Primitive::Numerus, width) => width.integer_range().is_some(),
            _ => false,
        }
    }

    /// A checked `Convert` of an integer value into its own slot type: the
    /// store node for a position whose HIR carries no `Conversio` (`x ↑`).
    fn integer_store_convert(
        &mut self,
        value: MirOperand,
        slot_ty: MirType,
        site: radix_hir::HirStoreSite,
        span: Span,
    ) -> MirOperand {
        self.runtime_call_value(
            MirIntrinsic::Convert(MirConversion {
                store: Some(site),
                flavor: MirConversionFlavor::Runtime,
                source_ty: slot_ty,
                target_ty: slot_ty,
                params: Vec::new(),
                recovery: None,
                struct_defaults: Vec::new(),
            }),
            vec![value],
            slot_ty,
            span,
        )
    }

    fn lower_assignment_place_with_fallback(
        &mut self,
        expr: &HirExpression,
        fallback_ty: MirType,
    ) -> Option<(MirPlace, MirType)> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                if let Some(place) = self.captured_bindings.get(def_id).cloned() {
                    let Some((_, ty)) = self.captured_binding_metadata.get(def_id).copied() else {
                        self.errors.push(MirError::unsupported(
                            expr.span,
                            "assignment target capture is missing metadata",
                        ));
                        return None;
                    };
                    return Some((place, ty));
                }
                let Some(binding) = self.bindings.get(def_id).copied() else {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "assignment target that does not resolve to a local",
                    ));
                    return None;
                };

                Some((MirPlace::local(binding.local), binding.ty))
            }
            HirExpressionKind::Field(object, name) => {
                let (mut place, _) = self.lower_assignment_place_with_fallback(object, fallback_ty)?;
                place.projections.push(MirProjection::Field(*name));
                let ty = expr.ty.map_or(fallback_ty, MirType::semantic);
                Some((place, ty))
            }
            HirExpressionKind::Index(object, index) => {
                let (mut place, _) = self.lower_assignment_place_with_fallback(object, fallback_ty)?;
                let index = self.lower_expr_value(index)?;
                place.projections.push(MirProjection::Index(index));
                let ty = expr.ty.map_or(fallback_ty, MirType::semantic);
                Some((place, ty))
            }
            _ => {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "assignment target that is not an addressable place",
                ));
                None
            }
        }
    }

    fn lower_transfer_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        let operand = self.lower_expr_value(expr)?;
        match operand {
            MirOperand::Constant(_) | MirOperand::Value(_) => {
                let ty = self.expr_ty(expr)?;
                Some(self.assign_temp(MirValueKind::Operand(operand), ty, expr.span))
            }
            MirOperand::Place(_) | MirOperand::Temp(_) => Some(operand),
        }
    }

    fn lower_return_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_transfer_expr(expr)
    }

    fn lower_path(&mut self, def_id: DefId, expr: &HirExpression) -> Option<MirOperand> {
        if let Some(place) = self.captured_bindings.get(&def_id).cloned() {
            let storage_ty = self
                .captured_binding_metadata
                .get(&def_id)
                .map(|(_, ty)| *ty);
            let place = self.adapt_narrowed_place(place, storage_ty, expr)?;
            return Some(MirOperand::Place(place));
        }
        let Some(binding) = self.bindings.get(&def_id).copied() else {
            if self.context.variant_parents.contains_key(&def_id) {
                let Some(field_names) = self.context.variant_fields.get(&def_id) else {
                    self.errors
                        .push(MirError::unsupported(expr.span, "enum variant path is missing field metadata"));
                    return None;
                };
                if !field_names.is_empty() {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "payload enum variant path without constructor call",
                    ));
                    return None;
                }
                let ty = self.expr_ty(expr)?;
                return Some(self.construct_temp(
                    MirAggregateKind::EnumVariant(def_id),
                    MirAggregateFields::Ordered(Vec::new()),
                    ty,
                    expr.span,
                ));
            }
            if let Some(konst) = self.context.top_level_consts.get(&def_id).copied() {
                let binding = self.lower_top_level_const_binding(def_id, konst, expr.span)?;
                return Some(MirOperand::Place(MirPlace::local(binding.local)));
            }
            // sgr C2a: a body-visible `magnitudo` parameter names its
            // compile-time size, not a runtime value. Carry it as the
            // symbolic shape-size operand on the parameter's own interned
            // node — shape bindings at instantiation fold it to the concrete
            // size; AIR never sees it unsubstituted.
            if let Some(name) = self.size_params.get(&def_id).copied()
                && let Some(shape) = symbolic_size_index(self.types, name)
            {
                return Some(MirOperand::Constant(MirConstant::ShapeSize { shape, axis: None }));
            }
            self.errors
                .push(MirError::unsupported(expr.span, "path that does not resolve to a local value"));
            return None;
        };

        let place = self.adapt_narrowed_place(MirPlace::local(binding.local), Some(binding.ty), expr)?;
        Some(MirOperand::Place(place))
    }

    fn lower_literal(&mut self, literal: &HirLiteral, ty: Option<TypeId>, span: Span) -> Option<MirOperand> {
        match literal {
            HirLiteral::Int(value) => {
                // Type-aware carrier: unsigned targets (`numerus<uW>` unsigned,
                // `modulus<uW>`) carry `UInt` even for small magnitudes, so
                // literals agree with conversion and arithmetic results on
                // equality, collection keys, and switch matching.
                if self.literal_ty_is_unsigned(ty) {
                    return Some(MirOperand::Constant(MirConstant::UInt(*value)));
                }
                if let Ok(value) = i64::try_from(*value) {
                    return Some(MirOperand::Constant(MirConstant::Int(value)));
                }
                // An `inf` slot holds the upper half of u64 exactly as `UInt`.
                if self.literal_ty_is_unbounded(ty) {
                    return Some(MirOperand::Constant(MirConstant::UInt(*value)));
                }
                // Upper-half u64 magnitude may not be sign-encoded into
                // `Int(i64)`; semantic analysis already rejected it for signed
                // targets, so reaching here is a contract violation.
                self.errors.push(MirError::unsupported(
                    span,
                    "unsigned integer literal above i64::MAX requires an unsigned target",
                ));
                None
            }
            // A literal beyond `u64` types only as `inf` (ruling 34): an exact
            // bignum constant, never truncated.
            HirLiteral::BigInt(digits) => Some(MirOperand::Constant(inf_literal_constant(digits))),
            // decimal-widths D1 successor: a decimal-typed
            // text literal lowers to its exact scaled carrier through the one
            // shared text→carrier conversion; the f64 field never intermediates.
            HirLiteral::FloatText { value, text } if let Some(width) = self.decimal_literal_width(ty) => {
                let Some(interner) = self.context.interner else {
                    // Without the interner the digit text is unreadable; keep
                    // the historical float constant instead of guessing.
                    return Some(MirOperand::Constant(MirConstant::Float(*value)));
                };
                match radix_codegen_shared::decimal::decimal_text_to_carrier(interner.resolve(*text), width) {
                    Some(carrier) => Some(MirOperand::Constant(MirConstant::Int(carrier))),
                    None => {
                        self.errors.push(MirError::unsupported(
                            span,
                            "decimal literal carrier is outside the declared width",
                        ));
                        None
                    }
                }
            }
            // Non-decimal text literals keep the historical float constant.
            HirLiteral::Float(value) | HirLiteral::FloatText { value, .. } => {
                let operand = MirOperand::Constant(MirConstant::Float(*value));
                match self.sized_fractus_literal_ty(ty) {
                    Some(ty) => Some(self.assign_temp(MirValueKind::Operand(operand), ty, span)),
                    None => Some(operand),
                }
            }
            HirLiteral::String(value) => Some(MirOperand::Constant(MirConstant::String(*value))),
            HirLiteral::Ascii(value) => Some(MirOperand::Constant(MirConstant::Ascii(*value))),
            HirLiteral::Bool(value) => Some(MirOperand::Constant(MirConstant::Bool(*value))),
            HirLiteral::Nil => Some(MirOperand::Constant(MirConstant::Nil)),
            HirLiteral::Octeti(sym) => self.lower_octeti_literal(*sym, ty, span),
            HirLiteral::Regex(pattern, flags) => {
                Some(MirOperand::Constant(MirConstant::Regex { pattern: *pattern, flags: *flags }))
            }
            HirLiteral::JsonValor(value) => self.lower_json_valor_literal(value, ty, span),
        }
    }

    fn sized_fractus_literal_ty(&self, ty: Option<TypeId>) -> Option<MirType> {
        let mut ty = ty?;
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                Type::SizedNumeric(Primitive::Fractus, _) => return Some(MirType::semantic(ty)),
                _ => return None,
            }
        }
    }

    /// The decimal width of a literal's target type, if any.
    ///
    /// Decimal widths ride `Numerus` (the scaled-integer carrier), not
    /// `Fractus`; mirrors the rust backend's decimal-literal detection.
    fn decimal_literal_width(&self, ty: Option<TypeId>) -> Option<NumericWidth> {
        let mut ty = ty?;
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal() => return Some(*width),
                _ => return None,
            }
        }
    }

    /// Whether an integer literal carries `UInt`. Only u64-width targets do:
    /// every narrower value fits the signed `Int` carrier, matching
    /// `modular_word_result` and the runner conversion boundaries, so
    /// literals, conversions, and arithmetic results stay carrier-consistent
    /// per width. Fails closed: a missing type is not unsigned. (Semantic
    /// analysis types every literal and rejects upper-half magnitudes for
    /// signed targets, so `None` here only arises for small literals that
    /// still fit the signed carrier.)
    fn literal_ty_is_unsigned(&self, ty: Option<TypeId>) -> bool {
        let Some(mut ty) = ty else { return false };
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::U64) => return true,
                Type::ModularWord(NumericWidth::U64) | Type::SaturatingWord(NumericWidth::U64) => return true,
                _ => return false,
            }
        }
    }

    /// Whether a literal's type is the unbounded `inf` (an `inf` slot holds
    /// any integer literal exactly, ruling 34).
    fn literal_ty_is_unbounded(&self, ty: Option<TypeId>) -> bool {
        ty.is_some_and(|ty| radix_types::numeric_width::is_unbounded_integer_type(self.types, ty))
    }

    fn lower_octeti_literal(&mut self, sym: Symbol, ty: Option<TypeId>, span: Span) -> Option<MirOperand> {
        if ty.is_none() {
            self.errors
                .push(MirError::missing_type(span, "octeti literal"));
            return None;
        }
        let Some(interner) = self.context.interner else {
            return Some(MirOperand::Constant(MirConstant::Octeti(Vec::new())));
        };
        let raw = interner.resolve(sym);
        let Ok(bytes) = crate::octeti_literal::decode_octeti_hex(raw) else {
            self.errors
                .push(MirError::unsupported(span, "octeti literal failed decode before MIR lowering"));
            return None;
        };
        Some(MirOperand::Constant(MirConstant::Octeti(bytes)))
    }

    fn lower_unary(&mut self, op: HirUnOp, operand: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        if let Some(lift) = self.tensor_lift_unary_op(op, operand) {
            return self.lower_tensor_lift(lift, &[operand], expr);
        }
        let Some(mir_op) = mir_un_op(op) else {
            self.errors
                .push(MirError::unsupported(expr.span, "unary operator without a MIR primitive"));
            return None;
        };
        // `-9223372036854775808`: the magnitude 2^63 has no `Int(i64)` form,
        // but its negation is exactly `i64::MIN` (d11-6 ruling 12: an untyped
        // constant is an exact value), so the negated literal is one constant.
        if op == HirUnOp::Neg
            && let HirExpressionKind::Literal(HirLiteral::Int(magnitude)) = &operand.kind
            && *magnitude == i64::MIN.unsigned_abs()
            && !self.literal_ty_is_unsigned(operand.ty)
        {
            return Some(MirOperand::Constant(MirConstant::Int(i64::MIN)));
        }
        let op = mir_op;
        let operand = self.lower_expr_value(operand)?;
        let ty = self.expr_ty(expr)?;

        Some(self.assign_temp(MirValueKind::Unary { op, operand }, ty, expr.span))
    }

    /// Lowers a runtime variant check (`est <type>`) to MIR.
    ///
    /// MIR does not model valor variant discrimination directly; per the existing
    /// `air/to_mir.rs` rule, `est` collapses to an equality test at this level.
    /// Lower `expr est <type>` / `expr non est <type>` into a runner-dispatched
    /// variant-tag check.
    ///
    /// The target type travels with the intrinsic so the runtime can check the
    /// operand's actual variant tag instead of approximating it as a nullity
    /// gate. Device/application lanes keep failing closed on the intrinsic
    /// (only the MIR runner dispatches `RuntimeTypeCheck`).
    fn lower_type_check(
        &mut self,
        positive: bool,
        ty: TypeId,
        expr: &HirExpression,
        outer: &HirExpression,
    ) -> Option<MirOperand> {
        let operand = self.lower_expr_value(expr)?;
        let result_ty = self.expr_ty(outer)?;
        // A type test against the null type is the null-presence probe: route
        // it back to the unary `is_nil` / `is_not_nil` lowering that the
        // legacy `est nihil` value form used, rather than the runner-dispatched
        // variant-tag intrinsic (DECISIONS D23.1).
        if matches!(self.types.get(ty), Type::Primitive(Primitive::Nihil)) {
            return Some(self.lower_nil_test(positive, operand, result_ty, outer.span));
        }
        Some(self.runtime_call_value(
            MirIntrinsic::TypeCheck(MirTypeCheckOp { positive, ty: MirType::semantic(ty) }),
            vec![operand],
            result_ty,
            outer.span,
        ))
    }

    fn lower_binary(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        // K4: a scalar operator over tensors is one elementwise lift; both
        // operands are always evaluated, so it precedes the short-circuit.
        if let Some(lift) = self.tensor_lift_binary_op(op, lhs, rhs) {
            return self.lower_tensor_lift(lift, &[lhs, rhs], expr);
        }
        if matches!(op, HirBinOp::And | HirBinOp::Or) {
            return self.lower_logical_short_circuit(op, lhs, rhs, expr);
        }
        if matches!(op, HirBinOp::Is | HirBinOp::IsNot)
            && let Some(predicate) = self.lower_is_predicate(op, lhs, rhs, expr)
        {
            return Some(predicate);
        }
        if matches!(op, HirBinOp::Between) {
            let collection = self.lower_expr_value(rhs)?;
            let value = self.lower_expr_value(lhs)?;
            let ty = self.expr_ty(expr)?;
            return Some(self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Contains),
                vec![collection, value],
                ty,
                expr.span,
            ));
        }
        // TENSOR-GLYPHS-3A: glyph products dispatch by operand family before
        // the primitive `mir_bin_op` fallback (which fails them closed).
        if op.is_glyph_product() {
            return self.lower_glyph_product(op, lhs, rhs, expr);
        }
        if op == HirBinOp::TrueDiv {
            return self.lower_true_division(lhs, rhs, expr);
        }
        // Numeric tensor/vector/matrix `+`/`-` take the live `addita`/`subtrahe`
        // method paths (`TensorAdd` / `TensorSub`), never primitive scalar ops.
        if matches!(op, HirBinOp::Add | HirBinOp::Sub) && self.is_addita_plus_operands(lhs, rhs) {
            return self.lower_glyph_product(op, lhs, rhs, expr);
        }
        if matches!(op, HirBinOp::FuzzyEq | HirBinOp::FuzzyNotEq) {
            // GT-1: fuzzy equality joins decimal operands at the d64 scale
            // like every other comparison tier; raw scaled carriers would
            // compare a d64 carrier against a peer at another scale.
            let (lhs, rhs) = self.lower_decimal_join_operands(lhs, rhs)?;
            let ty = self.expr_ty(expr)?;
            return Some(self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Approximate),
                vec![
                    lhs,
                    rhs,
                    MirOperand::Constant(MirConstant::Float(1e-9)),
                    MirOperand::Constant(MirConstant::Float(0.0)),
                    MirOperand::Constant(MirConstant::Bool(matches!(op, HirBinOp::FuzzyNotEq))),
                ],
                ty,
                expr.span,
            ));
        }
        // A1/C5 numeric collection laws have a dedicated MIR collection-op
        // seam. Dynamic pairs reach the runner for the hard live-length check;
        // literal-bounded lista pairs may still expand to indexed MIR cells,
        // while queue/stack capacities never stand in for their live lengths.
        if matches!(op, HirBinOp::Add | HirBinOp::Sub | HirBinOp::Mul | HirBinOp::Div)
            && self.is_lista_or_queue_stack_arithmetic_operand(lhs, rhs)
        {
            return self.lower_lista_elementwise(op, lhs, rhs, expr);
        }
        if matches!(op, HirBinOp::Lt | HirBinOp::Gt | HirBinOp::LtEq | HirBinOp::GtEq)
            && lhs
                .ty
                .is_some_and(|ty| matches!(self.types.get(self.types.peel_access_marker(ty).1), Type::Tuple(_)))
        {
            return self.lower_tuple_order(op, lhs, rhs, expr);
        }
        let Some(mir_op) = mir_bin_op(op) else {
            self.errors
                .push(MirError::unsupported(expr.span, "binary operator without a MIR primitive"));
            return None;
        };
        // GT-1: the decimal d64-scale join is the shared comparison join —
        // ordering (mixed-ordering-join) and the ≅/≈ value-equality tiers both
        // widen decimal operands at the d64 carrier before the bool op.
        let (lhs, rhs) = if matches!(
            op,
            HirBinOp::Lt
                | HirBinOp::Gt
                | HirBinOp::LtEq
                | HirBinOp::GtEq
                | HirBinOp::Eq
                | HirBinOp::NotEq
                | HirBinOp::StrictEq
                | HirBinOp::StrictNotEq
                | HirBinOp::ApproxEq
                | HirBinOp::ApproxNotEq
        ) {
            self.lower_decimal_join_operands(lhs, rhs)?
        } else {
            (self.lower_expr_value(lhs)?, self.lower_expr_value(rhs)?)
        };
        let ty = self.expr_ty(expr)?;

        Some(self.assign_temp(MirValueKind::Binary { op: mir_op, lhs, rhs }, ty, expr.span))
    }

    /// `÷` (d11-6 ruling 15): integer operands convert to `f64` and divide;
    /// float and decimal operands lower exactly like `/`. An integer zero
    /// divisor traps (ruling 19c) through a discarded integer `1 / divisor`,
    /// which fails with the runtime's standard division error.
    fn lower_true_division(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let lhs_ty = self.expr_ty(lhs)?;
        let rhs_ty = self.expr_ty(rhs)?;
        let is_integer = |this: &Self, ty: MirType| {
            let semantic = this.types.peel_access_marker(ty.semantic_id()).1;
            // `wrapping` and `saturating` words are integers too (ruling 15).
            matches!(
                this.normalized_type(semantic),
                Type::Primitive(Primitive::Numerus)
                    | Type::SizedNumeric(Primitive::Numerus, _)
                    | Type::ModularWord(_)
                    | Type::SaturatingWord(_)
            ) && !matches!(
                this.normalized_type(semantic),
                Type::SizedNumeric(_, width) if width.is_decimal()
            )
        };
        if !(is_integer(self, lhs_ty) && is_integer(self, rhs_ty)) {
            let (lhs, rhs) = self.lower_decimal_join_operands(lhs, rhs)?;
            return Some(self.assign_temp(MirValueKind::Binary { op: MirBinOp::Div, lhs, rhs }, ty, expr.span));
        }
        let lhs_value = self.lower_expr_value(lhs)?;
        let rhs_value = self.lower_expr_value(rhs)?;
        let one = MirOperand::Constant(MirConstant::Int(1));
        self.binary_temp(MirBinOp::Div, one, rhs_value.clone(), rhs_ty, expr.span);
        // An untyped constant operand joins the typed operand's range
        // computation (ruling 12), so its HIR type can be a word type while
        // the MIR constant is a plain integer: convert from the constant's
        // own MIR type, which is what the validator checks the operand against.
        let convert = |this: &mut Self, value: MirOperand, source_ty: MirType| {
            let source_ty = match &value {
                MirOperand::Constant(MirConstant::Int(_)) => {
                    MirType::semantic(this.types.primitive(Primitive::Numerus))
                }
                MirOperand::Constant(MirConstant::UInt(_)) => this
                    .types
                    .find_sized_numeric(Primitive::Numerus, NumericWidth::U64)
                    .map_or_else(
                        || MirType::semantic(this.types.primitive(Primitive::Numerus)),
                        MirType::semantic,
                    ),
                _ => source_ty,
            };
            this.runtime_call_value(
                MirIntrinsic::Convert(MirConversion {
                    store: None,
                    flavor: MirConversionFlavor::Runtime,
                    source_ty,
                    target_ty: ty,
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                vec![value],
                ty,
                expr.span,
            )
        };
        let lhs_float = convert(self, lhs_value, lhs_ty);
        let rhs_float = convert(self, rhs_value, rhs_ty);
        Some(self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Div, lhs: lhs_float, rhs: rhs_float },
            ty,
            expr.span,
        ))
    }

    /// Tuple `≺ ≻ ≤ ≥` (D1.4): lexicographic three-way compare, then the
    /// glyph against zero. Elements must be built-in orderables; a genus
    /// element would need its own `compare` call per position, which MIR
    /// does not expand yet, so it fails closed here.
    fn lower_tuple_order(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let tuple_ty = lhs.ty?;
        if radix_types::ordering::builtin_ordering_kind(self.types, tuple_ty).is_none() {
            self.errors.push(MirError::unsupported(
                expr.span,
                "tuple ordering over a genus element is not lowered on MIR",
            ));
            return None;
        }
        let lhs_value = self.lower_expr_value(lhs)?;
        let rhs_value = self.lower_expr_value(rhs)?;
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let ordering = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Compare),
            vec![lhs_value, rhs_value],
            numerus,
            expr.span,
        );
        let mir_op = mir_bin_op(op)?;
        let ty = self.expr_ty(expr)?;
        Some(self.assign_temp(
            MirValueKind::Binary { op: mir_op, lhs: ordering, rhs: MirOperand::Constant(MirConstant::Int(0)) },
            ty,
            expr.span,
        ))
    }

    fn lower_logical_short_circuit(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let result = self.push_temp(ty, expr.span);
        let result_place = MirPlace::temp(result);
        let lhs_value = self.lower_expr_value(lhs)?;

        let rhs_id = self.fresh_block(rhs.span);
        let settled_id = self.fresh_block(expr.span);
        let join_id = self.fresh_block(expr.span);
        let (then_block, else_block, settled_value) = match op {
            HirBinOp::And => (rhs_id, settled_id, false),
            HirBinOp::Or => (settled_id, rhs_id, true),
            _ => return None,
        };

        self.terminate_current(
            MirTerminatorKind::Branch { condition: lhs_value, then_block, else_block },
            expr.span,
        );

        self.switch_to(rhs_id);
        let rhs_value = self.lower_expr_value(rhs)?;
        self.assign(result_place.clone(), rhs_value, ty, rhs.span);
        let rhs_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), expr.span);

        self.switch_to(settled_id);
        self.assign(
            result_place.clone(),
            MirOperand::Constant(MirConstant::Bool(settled_value)),
            ty,
            expr.span,
        );
        let settled_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), expr.span);

        if rhs_reaches || settled_reaches {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, expr.span);
        }

        Some(MirOperand::Temp(result))
    }

    /// Placeholder operand for an omitted `sponte` parameter without `vel`.
    pub(super) fn omitted_optional_operand(&self, _span: Span) -> MirOperand {
        MirOperand::Constant(MirConstant::Nil)
    }

    fn type_zero_struct_operand(&mut self, def_id: DefId, mir_ty: MirType, span: Span) -> Option<MirOperand> {
        let fields = self.materialize_struct_fields(def_id, &[], aggregate::StructFieldFill::AllZero, span)?;
        Some(self.construct_temp(
            MirAggregateKind::Struct(def_id),
            MirAggregateFields::Named(fields),
            mir_ty,
            span,
        ))
    }

    /// M-007 zero of a no-initializer enum-typed local: the first variant in
    /// declaration order with a fully zeroed payload. A `varia` placeholder is
    /// assigned before any read, so any variant is sound; the first is the
    /// deterministic "zero" (mirrors C enum zero-init picking enumerator 0).
    /// Fails closed with a specific diagnostic when the enum has no variants or
    /// the chosen variant's payload metadata is missing.
    fn type_zero_enum_operand(&mut self, def_id: DefId, mir_ty: MirType, span: Span) -> Option<MirOperand> {
        let variants = self
            .context
            .enum_variants
            .get(&def_id)
            .cloned()
            .unwrap_or_default();
        let Some(&first) = variants.first() else {
            self.errors.push(MirError::unsupported(
                span,
                "enum type zero-init before MIR lowering completed: enum has no variants",
            ));
            return None;
        };
        let Some(field_names) = self.context.variant_fields.get(&first).cloned() else {
            self.errors.push(MirError::unsupported(
                span,
                "enum type zero-init before MIR lowering completed: variant payload metadata missing",
            ));
            return None;
        };
        let mut fields = Vec::with_capacity(field_names.len());
        for name in field_names {
            let Some(field_ty) = self
                .context
                .variant_field_tys
                .get(&first)
                .and_then(|tys| tys.get(&name))
                .copied()
            else {
                self.errors.push(MirError::unsupported(
                    span,
                    "enum type zero-init before MIR lowering completed: variant payload type metadata missing",
                ));
                return None;
            };
            let value = self.type_zero_operand(field_ty.semantic_id(), span)?;
            fields.push(MirNamedOperand { name, value });
        }
        Some(self.construct_temp(
            MirAggregateKind::EnumVariant(first),
            MirAggregateFields::Named(fields),
            mir_ty,
            span,
        ))
    }

    /// Type-shaped zero/default used at MIR lowering sites for voluntary slots.
    pub(super) fn type_zero_operand(&mut self, ty: TypeId, span: Span) -> Option<MirOperand> {
        let mir_ty = MirType::semantic(ty);
        match self.normalized_type(ty) {
            Type::Primitive(Primitive::Fractus) => Some(MirOperand::Constant(MirConstant::Float(0.0))),
            Type::Primitive(Primitive::Bivalens) => Some(MirOperand::Constant(MirConstant::Bool(false))),
            Type::Primitive(Primitive::Nihil | Primitive::Ignotum) => Some(MirOperand::Constant(MirConstant::Nil)),
            Type::Primitive(Primitive::Vacuum) => Some(MirOperand::Constant(MirConstant::Unit)),
            Type::Primitive(Primitive::Numerus) => Some(MirOperand::Constant(MirConstant::Int(0))),
            // SizedNumeric zeros are wrapped in a typed temp: a bare constant
            // carries the primitive `numerus`/`fractus` type, which the MIR
            // validator rejects when assigned to a sized field/local (e.g.
            // `f32` or `numerus<u8>`) without narrowing. The temp carries the
            // sized type, mirroring how the literal path types field values.
            Type::SizedNumeric(Primitive::Numerus, _) => {
                Some(self.assign_temp(MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(0))), mir_ty, span))
            }
            Type::SizedNumeric(Primitive::Fractus, _) => Some(self.assign_temp(
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(0.0))),
                mir_ty,
                span,
            )),
            Type::Primitive(Primitive::Textus | Primitive::Ascii) => {
                Some(MirOperand::Constant(MirConstant::String(self.context.empty_textus)))
            }
            Type::Primitive(Primitive::Octeti) => Some(MirOperand::Constant(MirConstant::Octeti(Vec::new()))),
            Type::Primitive(Primitive::Regex) => {
                self.errors.push(MirError::unsupported(
                    span,
                    "regex type zero-init before MIR lowering completed",
                ));
                None
            }
            Type::Option(_) => Some(MirOperand::Constant(MirConstant::Nil)),
            Type::Union(members) => {
                let nihil = self.types.primitive(Primitive::Nihil);
                if members.contains(&nihil) {
                    Some(MirOperand::Constant(MirConstant::Nil))
                } else {
                    // A Nihil-less value union takes the first member's zero,
                    // wrapped in a typed temp of the union type: a bare
                    // constant carries its member primitive type, which the
                    // validator rejects against the union destination.
                    let Some(&first) = members.first() else {
                        self.errors.push(MirError::unsupported(
                            span,
                            "union type zero-init before MIR lowering completed: union has no members",
                        ));
                        return None;
                    };
                    let member_zero = self.type_zero_operand(first, span)?;
                    Some(self.assign_temp(MirValueKind::Operand(member_zero), mir_ty, span))
                }
            }
            Type::Array(_) => Some(self.construct_temp(
                MirAggregateKind::Array,
                MirAggregateFields::Ordered(Vec::new()),
                mir_ty,
                span,
            )),
            Type::Map(_, _) => {
                Some(self.construct_temp(MirAggregateKind::Map, MirAggregateFields::Keyed(Vec::new()), mir_ty, span))
            }
            Type::Set(_) => {
                Some(self.construct_temp(MirAggregateKind::Set, MirAggregateFields::Ordered(Vec::new()), mir_ty, span))
            }
            Type::Sparsa(_, _) => Some(self.construct_temp(
                MirAggregateKind::SparseTensor,
                MirAggregateFields::Ordered(Vec::new()),
                mir_ty,
                span,
            )),
            Type::Struct(def_id) => self.type_zero_struct_operand(*def_id, mir_ty, span),
            Type::Enum(def_id) => self.type_zero_enum_operand(*def_id, mir_ty, span),
            _ => {
                self.errors
                    .push(MirError::unsupported(span, "type zero-init before MIR lowering completed"));
                None
            }
        }
    }

    fn lower_is_predicate(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let result_ty = self.expr_ty(expr)?;
        if let HirExpressionKind::Field(object, member) = &rhs.kind
            && let HirExpressionKind::Path(import_def) = &object.kind
            && let Some(target_ty) = self
                .context
                .imported_type_members
                .get(&(*import_def, *member))
                .copied()
        {
            let operand = self.lower_expr_value(lhs)?;
            return Some(self.runtime_call_value(
                MirIntrinsic::TypeCheck(MirTypeCheckOp { positive: matches!(op, HirBinOp::Is), ty: target_ty }),
                vec![operand],
                result_ty,
                expr.span,
            ));
        }
        if matches!(rhs.kind, HirExpressionKind::Literal(HirLiteral::Nil)) {
            let operand = self.lower_expr_value(lhs)?;
            return Some(self.lower_nil_comparison(op, operand, result_ty, expr.span));
        }
        if matches!(lhs.kind, HirExpressionKind::Literal(HirLiteral::Nil)) {
            let operand = self.lower_expr_value(rhs)?;
            return Some(self.lower_nil_comparison(op, operand, result_ty, expr.span));
        }
        None
    }

    /// `est`/`non est` against a literal `nihil` collapses to a nil-check
    /// unary op (`est` → `IsNil`, `non est` → `IsNotNil`), matching the
    /// `air/to_mir.rs` rule that variant discrimination reduces to equality
    /// at this level.
    fn lower_nil_comparison(
        &mut self,
        op: HirBinOp,
        operand: MirOperand,
        result_ty: MirType,
        span: Span,
    ) -> MirOperand {
        self.lower_nil_test(matches!(op, HirBinOp::Is), operand, result_ty, span)
    }

    /// `est nihil` → `IsNil`, `non est nihil` → `IsNotNil`.
    fn lower_nil_test(&mut self, positive: bool, operand: MirOperand, result_ty: MirType, span: Span) -> MirOperand {
        let op = if positive { MirUnOp::IsNil } else { MirUnOp::IsNotNil };
        self.assign_temp(MirValueKind::Unary { op, operand }, result_ty, span)
    }

    /// `vel` short-circuits (operator ruling 2026-09-24): the fallback is
    /// evaluated only when the left operand is `nihil`. An effect-free
    /// fallback (literal, path) keeps the single `option coalesce` op, since
    /// eager evaluation is unobservable; any other fallback lowers behind a
    /// nil branch, mirroring `lower_logical_short_circuit`.
    fn lower_coalesce(&mut self, lhs: &HirExpression, rhs: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        // K4: `vel` over a tensor fills each `nihil` element; both operands are evaluated.
        if let Some(lift) = self.tensor_lift_binary_op(HirBinOp::Coalesce, lhs, rhs) {
            return self.lower_tensor_lift(lift, &[lhs, rhs], expr);
        }
        if rhs.is_effect_free_operand() {
            let value = self.lower_expr_value(lhs)?;
            let fallback = self.lower_expr_value(rhs)?;
            let ty = self.expr_ty(expr)?;
            return Some(self.assign_temp(
                MirValueKind::Option(MirOptionOp::Coalesce { value, fallback }),
                ty,
                expr.span,
            ));
        }
        self.lower_coalesce_short_circuit(lhs, rhs, expr)
    }

    fn lower_coalesce_short_circuit(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let result = self.push_temp(ty, expr.span);
        let result_place = MirPlace::temp(result);
        let value = self.lower_expr_value(lhs)?;
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let is_nil = self.assign_temp(
            MirValueKind::Unary { op: MirUnOp::IsNil, operand: value.clone() },
            bivalens,
            lhs.span,
        );

        let fallback_id = self.fresh_block(rhs.span);
        let present_id = self.fresh_block(lhs.span);
        let join_id = self.fresh_block(expr.span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: is_nil, then_block: fallback_id, else_block: present_id },
            expr.span,
        );

        self.switch_to(fallback_id);
        let fallback = self.lower_expr_value(rhs)?;
        self.assign(result_place.clone(), fallback, ty, rhs.span);
        let fallback_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), expr.span);

        self.switch_to(present_id);
        // WHY: a nullable result (`T ∪ nihil vel T ∪ nihil`) keeps the option
        // carrier; a non-nullable result takes the proven-present payload.
        let result_nullable = expr.ty.is_some_and(|ty| {
            self.types.nullable_inner(ty).is_some()
                || self
                    .types
                    .equals(ty, self.types.primitive(Primitive::Nihil))
        });
        let lhs_nullable = lhs
            .ty
            .is_some_and(|ty| self.types.nullable_inner(ty).is_some());
        let present = if lhs_nullable && !result_nullable {
            self.assign_temp(
                MirValueKind::Option(MirOptionOp::Unwrap { value, mode: MirOptionUnwrapMode::Assume }),
                ty,
                lhs.span,
            )
        } else {
            value
        };
        self.assign(result_place, present, ty, lhs.span);
        let present_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), expr.span);

        if fallback_reaches || present_reaches {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, expr.span);
        }
        Some(MirOperand::Temp(result))
    }

    fn assign(&mut self, place: MirPlace, operand: MirOperand, ty: MirType, span: Span) {
        let value = self.new_value(MirValueKind::Operand(operand), ty, span);
        self.append_stmt(MirStatement { kind: MirStatementKind::Assign { place, value }, span });
    }

    fn assign_temp(&mut self, kind: MirValueKind, ty: MirType, span: Span) -> MirOperand {
        let temp = self.push_temp(ty, span);
        let value = self.new_value(kind, ty, span);
        self.append_stmt(MirStatement { kind: MirStatementKind::Assign { place: MirPlace::temp(temp), value }, span });
        MirOperand::Temp(temp)
    }

    fn record_closure_destination(&mut self, destination: &MirPlace, value: &MirOperand) {
        if !destination.projections.is_empty() {
            return;
        }
        let MirPlaceBase::Local(local) = destination.base else {
            return;
        };
        if let Some(function) = self.kernel_callee_for_operand(value) {
            self.kernel_local_callees.insert(local, function);
            self.closure_local_callees.remove(&local);
        } else if let Some(closure) = self.closure_callee_for_operand(value) {
            self.kernel_local_callees.remove(&local);
            self.closure_local_callees.insert(local, closure);
        } else {
            self.kernel_local_callees.remove(&local);
            self.closure_local_callees.remove(&local);
        }
    }

    fn construct_temp(
        &mut self,
        kind: MirAggregateKind,
        fields: MirAggregateFields,
        ty: MirType,
        span: Span,
    ) -> MirOperand {
        let temp = self.push_temp(ty, span);
        self.append_stmt(MirStatement {
            kind: MirStatementKind::Construct {
                destination: MirPlace::temp(temp),
                aggregate: MirAggregate { kind, ty, fields },
            },
            span,
        });
        MirOperand::Temp(temp)
    }

    fn variant_payload(&mut self, variant: DefId, args: Vec<MirOperand>, span: Span) -> MirAggregateFields {
        let Some(field_names) = self.context.variant_fields.get(&variant) else {
            self.errors.push(MirError::unsupported(
                span,
                "variant payload field metadata missing at MIR lowering",
            ));
            return MirAggregateFields::Ordered(args.into_iter().map(MirAggregateItem::Operand).collect());
        };
        if field_names.len() != args.len() {
            self.errors.push(MirError::unsupported(
                span,
                format!(
                    "variant payload arity mismatch: expected {} fields {}, got {} argument{}",
                    field_names.len(),
                    field_names
                        .iter()
                        .map(|s| format!("{s:?}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    args.len(),
                    if args.len() == 1 { "" } else { "s" },
                ),
            ));
            return MirAggregateFields::Ordered(args.into_iter().map(MirAggregateItem::Operand).collect());
        }
        MirAggregateFields::Named(
            field_names
                .iter()
                .copied()
                .zip(args)
                .map(|(name, value)| MirNamedOperand { name, value })
                .collect(),
        )
    }

    fn new_value(&mut self, kind: MirValueKind, ty: MirType, span: Span) -> MirValue {
        let id = MirValueId(self.next_value);
        self.next_value += 1;
        MirValue { id, kind, ty, span }
    }

    fn push_temp(&mut self, ty: MirType, span: Span) -> MirTempId {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirTempId is u32 by design; lowered temp count is bounded by u32::MAX"
        )]
        let id = MirTempId(self.temps.len() as u32);
        self.temps.push(MirTemp { id, ty, span });
        id
    }

    fn fresh_block(&mut self, span: Span) -> MirBlockId {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirBlockId is u32 by design; lowered block count is bounded by u32::MAX"
        )]
        let id = MirBlockId(self.blocks.len() as u32);
        self.blocks
            .push(OpenBlock { id, statements: Vec::new(), terminator: None, span });
        id
    }

    fn switch_to(&mut self, block: MirBlockId) {
        if self.block(block).terminator.is_some() {
            self.current = None;
            return;
        }
        self.current = Some(block);
    }

    fn current_is_open(&self) -> bool {
        self.current
            .is_some_and(|id| self.block(id).terminator.is_none())
    }

    fn append_stmt(&mut self, stmt: MirStatement) {
        let Some(current) = self.current else {
            self.errors
                .push(MirError::unsupported(stmt.span, "statement after a MIR terminator"));
            return;
        };

        let block = self.block_mut(current);
        if block.terminator.is_some() {
            self.errors
                .push(MirError::unsupported(stmt.span, "statement after a MIR terminator"));
            self.current = None;
            return;
        }
        block.statements.push(stmt);
    }

    fn terminate_current(&mut self, kind: MirTerminatorKind, span: Span) -> bool {
        let Some(current) = self.current else {
            self.errors.push(MirError::unsupported(
                span,
                "terminator emitted after current MIR block was sealed",
            ));
            return false;
        };

        let block = self.block_mut(current);
        if block.terminator.is_some() {
            self.errors
                .push(MirError::unsupported(span, "duplicate MIR block terminator"));
            self.current = None;
            return false;
        }

        block.terminator = Some(MirTerminator { kind, span });
        self.current = None;
        true
    }

    fn terminate_open_current(&mut self, kind: MirTerminatorKind, span: Span) -> bool {
        if !self.current_is_open() {
            return false;
        }
        self.terminate_current(kind, span)
    }

    fn seal_unreachable(&mut self, block: MirBlockId, span: Span) {
        let open = self.block(block).terminator.is_none();
        if open {
            self.block_mut(block).terminator = Some(MirTerminator { kind: MirTerminatorKind::Unreachable, span });
        }
        if self.current == Some(block) {
            self.current = None;
        }
    }

    fn finish_blocks(&mut self) -> Vec<MirBlock> {
        for index in 0..self.blocks.len() {
            if self.blocks[index].terminator.is_none() {
                let span = self.blocks[index].span;
                self.blocks[index].terminator = Some(MirTerminator { kind: MirTerminatorKind::Unreachable, span });
            }
        }

        self.blocks
            .drain(..)
            .map(|block| MirBlock {
                id: block.id,
                statements: block.statements,
                terminator: block
                    .terminator
                    .unwrap_or(MirTerminator { kind: MirTerminatorKind::Unreachable, span: block.span }),
                span: block.span,
            })
            .collect()
    }

    fn block(&self, id: MirBlockId) -> &OpenBlock {
        &self.blocks[id.0 as usize]
    }

    fn block_mut(&mut self, id: MirBlockId) -> &mut OpenBlock {
        &mut self.blocks[id.0 as usize]
    }

    fn next_local_id(&self) -> MirLocalId {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirLocalId is u32 by design; lowered local count is bounded by u32::MAX"
        )]
        MirLocalId(self.locals.len() as u32)
    }

    fn expr_ty(&mut self, expr: &HirExpression) -> Option<MirType> {
        let Some(ty) = expr.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "expression"));
            return None;
        };
        Some(MirType::semantic(ty))
    }

    fn is_vacuum(&self, ty: MirType) -> bool {
        ty.semantic_id() == self.types.primitive(Primitive::Vacuum)
    }

    fn is_addita_plus_operands(&self, lhs: &HirExpression, rhs: &HirExpression) -> bool {
        let (Some(lhs_ty), Some(rhs_ty)) = (lhs.ty, rhs.ty) else {
            return false;
        };
        matches!(
            (self.normalized_type(lhs_ty), self.normalized_type(rhs_ty)),
            (Type::Tensor(_, _), Type::Tensor(_, _))
                | (Type::Vector(_, _), Type::Vector(_, _))
                | (Type::Matrix(_, _), Type::Matrix(_, _))
        )
    }

    /// Join decimal comparison operands at the d64 scaled carrier before the
    /// bool-valued MIR binary op. Shared by ordering and the ≅/≈ equality
    /// tiers (GT-1). The result type of a comparison is bivalens, so runner
    /// normalization keyed only from that result would otherwise miss the
    /// decimal operand widths entirely.
    fn lower_decimal_join_operands(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
    ) -> Option<(MirOperand, MirOperand)> {
        let Some((lhs_ty, rhs_ty, target_ty)) = self.decimal_join_target(lhs, rhs) else {
            let (l, r) = (self.lower_expr_value(lhs)?, self.lower_expr_value(rhs)?);
            // A decimal beside a float compares by exact value (ruling 22); the
            // runner reads the decimal's scale from the operand's static type,
            // so a constant operand is materialized into a typed temp.
            let l = self.typed_decimal_operand(lhs, l)?;
            let r = self.typed_decimal_operand(rhs, r)?;
            return Some((l, r));
        };
        let (lhs_expr, rhs_expr) = (lhs, rhs);
        let lhs = self.lower_join_operand(lhs_expr, lhs_ty, target_ty)?;
        let rhs = self.lower_join_operand(rhs_expr, rhs_ty, target_ty)?;
        // An operand already at the join type is read as it is; type it too.
        let lhs = self.typed_decimal_operand(lhs_expr, lhs)?;
        let rhs = self.typed_decimal_operand(rhs_expr, rhs)?;
        Some((lhs, rhs))
    }

    fn typed_decimal_operand(&mut self, expr: &HirExpression, operand: MirOperand) -> Option<MirOperand> {
        // A projected place (`xs[0]`, `p.x`, `t[0]`) carries no type of its own
        // either, so it is read into a typed temp like a constant.
        let untyped = match &operand {
            MirOperand::Constant(_) => true,
            MirOperand::Place(place) => !place.projections.is_empty(),
            _ => false,
        };
        if !untyped {
            return Some(operand);
        }
        let ty = self.expr_ty(expr)?;
        let decimal = matches!(
            self.normalized_type(self.types.peel_access_marker(ty.semantic_id()).1),
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal()
        );
        Some(if decimal {
            self.assign_temp(MirValueKind::Operand(operand), ty, expr.span)
        } else {
            operand
        })
    }

    fn decimal_join_target(&mut self, lhs: &HirExpression, rhs: &HirExpression) -> Option<(MirType, MirType, TypeId)> {
        let lhs_ty = self.expr_ty(lhs)?;
        let rhs_ty = self.expr_ty(rhs)?;
        let lhs_semantic = self.types.peel_access_marker(lhs_ty.semantic_id()).1;
        let rhs_semantic = self.types.peel_access_marker(rhs_ty.semantic_id()).1;
        let lhs_width = match self.normalized_type(lhs_semantic) {
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal() => Some(*width),
            _ => None,
        };
        let rhs_width = match self.normalized_type(rhs_semantic) {
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal() => Some(*width),
            _ => None,
        };
        if lhs_width.is_none() && rhs_width.is_none() {
            return None;
        }
        // Ruling 22: a decimal beside a non-constant integer compares by exact
        // value; joining the integer at `d64` would trap for a large one. The
        // runner reads both static types (a constant integer still joins).
        let plain_integer = |expr: &HirExpression, semantic: TypeId| {
            let ty = self.normalized_type(semantic);
            // Ruling 37: `inf` beside a decimal is an exact cross-family
            // comparison like any plain integer — constant or not (joining it
            // at `d64` would trap for a large value).
            if matches!(ty, Type::SizedNumeric(Primitive::Numerus, width) if width.is_unbounded()) {
                return true;
            }
            crate::semantic::passes::const_int_value(expr).is_none()
                && match ty {
                    Type::Primitive(Primitive::Numerus) => true,
                    Type::SizedNumeric(Primitive::Numerus, width) => width.integer_range().is_some(),
                    _ => false,
                }
        };
        if (lhs_width.is_some() && rhs_width.is_none() && plain_integer(rhs, rhs_semantic))
            || (rhs_width.is_some() && lhs_width.is_none() && plain_integer(lhs, lhs_semantic))
        {
            return None;
        }
        if matches!(
            self.normalized_type(lhs_semantic),
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _)
        ) || matches!(
            self.normalized_type(rhs_semantic),
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _)
        ) {
            return None;
        }
        // The semantic ordering gate interns d64 as the lossless join. Reuse
        // that existing TypeId because FunctionBuilder borrows the shared
        // TypeTable immutably; the fallback keeps direct-HIR tests usable.
        let d64 = (0..self.types.type_count()).find_map(|index| {
            let index = u32::try_from(index).ok()?;
            let ty = TypeId(index);
            matches!(
                self.normalized_type(ty),
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::D64)
            )
            .then_some(ty)
        });
        let target = d64.unwrap_or(match (lhs_width, rhs_width) {
            (Some(NumericWidth::D64), _) => lhs_semantic,
            (_, Some(NumericWidth::D64)) => rhs_semantic,
            (Some(_), _) => lhs_semantic,
            (_, Some(_)) => rhs_semantic,
            (None, None) => lhs_semantic,
        });
        Some((lhs_ty, rhs_ty, target))
    }

    fn lower_join_operand(
        &mut self,
        expr: &HirExpression,
        source_ty: MirType,
        target_ty: TypeId,
    ) -> Option<MirOperand> {
        let value = self.lower_expr_value(expr)?;
        if self.types.equals(source_ty.semantic_id(), target_ty) {
            return Some(value);
        }
        let target_ty = MirType::semantic(target_ty);
        Some(self.runtime_call_value(
            MirIntrinsic::Convert(MirConversion {
                store: None,
                flavor: MirConversionFlavor::Runtime,
                source_ty,
                target_ty,
                params: Vec::new(),
                recovery: None,
                struct_defaults: Vec::new(),
            }),
            vec![value],
            target_ty,
            expr.span,
        ))
    }

    fn is_lista_or_queue_stack_arithmetic_operand(&self, lhs: &HirExpression, rhs: &HirExpression) -> bool {
        [lhs.ty, rhs.ty].into_iter().flatten().any(|ty| {
            matches!(
                self.normalized_type(ty),
                Type::Array(_)
                    | Type::BoundedArray { .. }
                    | Type::Queue(_)
                    | Type::BoundedQueue { .. }
                    | Type::Stack(_)
                    | Type::BoundedStack { .. }
            )
        })
    }

    fn normalized_type(&self, mut ty: TypeId) -> &Type {
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                other => return other,
            }
        }
    }

    /// Carrier under `ref`/`mut`/`own`/`copy` wrappers.
    ///
    /// Typecheck peels access markers before method lookup. MIR method
    /// resolution must too: a `mut list.get` or `ref Struct.method()` otherwise
    /// fail-closes as "method call before runtime/provider MIR lowering".
    fn method_receiver_payload_id(&self, ty: TypeId) -> TypeId {
        self.types.peel_access_marker(ty).1
    }

    fn method_receiver_struct_def(&self, ty: TypeId) -> Option<DefId> {
        let payload = self.method_receiver_payload_id(ty);
        match self.types.get(payload) {
            Type::Struct(def_id) => Some(*def_id),
            Type::Applied(base, _) => match self.types.get(*base) {
                Type::Struct(def_id) => Some(*def_id),
                _ => None,
            },
            _ => None,
        }
    }
}

/// The MIR constant of an `inf` literal (canonical decimal digits, ruling
/// 34): the 64-bit constants when the value fits them, so arithmetic and
/// equality see one spelling, and an exact `Magnus` constant beyond.
pub(super) fn inf_literal_constant(digits: &str) -> MirConstant {
    match handroll::Magnus::parse_decimal(digits) {
        Ok(value) => {
            if let Some(v) = value.to_i64() {
                MirConstant::Int(v)
            } else if let Some(v) = value.to_u64() {
                MirConstant::UInt(v)
            } else {
                MirConstant::Magnus(value.to_string())
            }
        }
        Err(_) => MirConstant::Magnus(digits.to_owned()),
    }
}

fn mir_un_op(op: HirUnOp) -> Option<MirUnOp> {
    match op {
        HirUnOp::Neg => Some(MirUnOp::Neg),
        HirUnOp::Not => Some(MirUnOp::Not),
        HirUnOp::BitNot => Some(MirUnOp::BitNot),
    }
}

fn mir_bin_op(op: HirBinOp) -> Option<MirBinOp> {
    match op {
        HirBinOp::Add => Some(MirBinOp::Add),
        HirBinOp::Sub => Some(MirBinOp::Sub),
        HirBinOp::Mul => Some(MirBinOp::Mul),
        HirBinOp::Div => Some(MirBinOp::Div),
        HirBinOp::Mod => Some(MirBinOp::Mod),
        HirBinOp::Eq | HirBinOp::StrictEq | HirBinOp::ApproxEq => Some(MirBinOp::Eq),
        HirBinOp::FuzzyEq | HirBinOp::FuzzyNotEq => None,
        HirBinOp::NotEq | HirBinOp::StrictNotEq | HirBinOp::ApproxNotEq => Some(MirBinOp::NotEq),
        HirBinOp::Lt => Some(MirBinOp::Lt),
        HirBinOp::Gt => Some(MirBinOp::Gt),
        HirBinOp::LtEq => Some(MirBinOp::LtEq),
        HirBinOp::GtEq => Some(MirBinOp::GtEq),
        HirBinOp::And => Some(MirBinOp::And),
        HirBinOp::Or => Some(MirBinOp::Or),
        HirBinOp::Coalesce => Some(MirBinOp::Coalesce),
        HirBinOp::BitAnd => Some(MirBinOp::BitAnd),
        HirBinOp::BitOr => Some(MirBinOp::BitOr),
        HirBinOp::BitXor => Some(MirBinOp::BitXor),
        HirBinOp::Shl => Some(MirBinOp::Shl),
        HirBinOp::Shr => Some(MirBinOp::Shr),
        HirBinOp::Is => Some(MirBinOp::Eq),
        HirBinOp::IsNot => Some(MirBinOp::NotEq),
        HirBinOp::InRange | HirBinOp::Between => None,
        // Glyph products are intercepted by `lower_binary` before this point;
        // keep the explicit fail-closed arm for any future caller.
        HirBinOp::Dot | HirBinOp::Cross | HirBinOp::TensorProduct | HirBinOp::Hadamard | HirBinOp::HadamardDiv => None,
        // `÷` (d11-6 ruling 15) has no MIR primitive yet; fail closed.
        HirBinOp::TrueDiv => None,
    }
}

fn unsupported_expr_kind_name(kind: &HirExpressionKind) -> &'static str {
    match kind {
        HirExpressionKind::MethodCall(..) => "method calls before receiver-aware MIR lowering",
        HirExpressionKind::Field(_, _) => "field access before aggregate place MIR lowering",
        HirExpressionKind::Index(_, _) => "index access before aggregate place MIR lowering",
        HirExpressionKind::OptionalChain(_, _) => "optional chains before nullable control-flow MIR lowering",
        HirExpressionKind::NonNull(_, _) => "non-null assertions before nullable control-flow MIR lowering",
        HirExpressionKind::Gradient { .. } => {
            "gradient expressions before tensor-glyphs 2B backward-companion MIR lowering"
        }
        HirExpressionKind::Block(_) => "block expressions before nested-block MIR lowering",
        // UT: trap lowers through `visit_trap_expr`; kept exhaustive here for
        // any future caller.
        HirExpressionKind::Trap(_) => "capta trap before capture-boundary MIR lowering",
        HirExpressionKind::CallDefault { .. } => "call default before capture-boundary MIR lowering",
        HirExpressionKind::Si { .. } => "si before control-flow MIR lowering",
        HirExpressionKind::Discerne { .. } => "discerne before switch MIR lowering",
        HirExpressionKind::Loop(_) => "loop before control-flow MIR lowering",
        HirExpressionKind::Dum(_, _) => "dum before control-flow MIR lowering",
        HirExpressionKind::Itera(_, _, _, _, _, _) => "itera before iterator MIR lowering",
        HirExpressionKind::IteraLockstep { .. } => {
            "lockstep itera before aligned-walk MIR lowering"
        }
        // Unreachable from the lowering dispatcher (the NR-3 range product
        // lowers through `visit_itera_range_expr`); kept so this naming
        // match stays exhaustive for any future caller.
        HirExpressionKind::IteraRange { .. } => {
            "range-product itera before ordered-walk MIR lowering"
        }
        HirExpressionKind::Summa { .. } => "summa before term-fold MIR lowering",
        HirExpressionKind::Extremum { .. } => "max/min before extrema MIR lowering",
        // Unreachable from the lowering dispatcher (first-match lowers through
        // `collection_higher_order`); kept so this naming match stays exhaustive.
        HirExpressionKind::FirstMatch { .. } => "primus_quem before first-match MIR lowering",
        HirExpressionKind::Intervallum { .. } => "range expressions before range MIR lowering",
        HirExpressionKind::Array(_) => "array literals before aggregate MIR lowering",
        HirExpressionKind::Struct(_, _) => "struct literals before aggregate MIR lowering",
        HirExpressionKind::Tuple(..) => "tuple literals before aggregate MIR lowering",
        HirExpressionKind::Scribe(kind, _) => scribe_kind_name(*kind),
        HirExpressionKind::Scriptum(_, _) => "scriptum templates before format intrinsic MIR lowering",
        HirExpressionKind::Format { .. } => "¶ format before format intrinsic MIR lowering",
        HirExpressionKind::ReadLine(_) => "lege before input MIR lowering",
        HirExpressionKind::Praefixum(_) => "praefixum before compile-time MIR lowering",
        HirExpressionKind::Insere(_) => "insere before the post-typecheck embed fold",
        HirExpressionKind::Widen { .. } => "bounded-to-inf widening before conversion MIR lowering",
        HirExpressionKind::Adfirma(_, _) => "adfirma before assert intrinsic MIR lowering",
        HirExpressionKind::Panic(_) => "mori fatal flow",
        HirExpressionKind::Throw(_) => "iace error-flow",
        HirExpressionKind::Handled { .. } => "structured cape before local-handler MIR lowering",
        HirExpressionKind::Clausura(_, _, _, _, _) => "closure expression",
        HirExpressionKind::Cede(_) => "legacy context-dependent cede carrier",
        HirExpressionKind::Verte { .. } => "verte before conversion MIR lowering",
        HirExpressionKind::Conversio { .. } => "conversio before runtime conversion MIR lowering",
        HirExpressionKind::ConversioAssign { .. } => {
            "conversio-assign before conversion-directed assignment MIR lowering"
        }
        HirExpressionKind::Ad { .. } => "ad expression before stream conversation MIR lowering",
        HirExpressionKind::Ref(_, _) => "references before borrow-aware MIR lowering",
        HirExpressionKind::Deref(_) => "dereferences before borrow-aware MIR lowering",
        HirExpressionKind::Error => "error expressions",
        HirExpressionKind::Path(_)
        | HirExpressionKind::Literal(_)
        | HirExpressionKind::Vacua
        | HirExpressionKind::Binary(_, _, _)
        | HirExpressionKind::Unary(_, _)
        | HirExpressionKind::Call(..)
        | HirExpressionKind::Assign(_, _)
        // Await/yield expression forms lower through their own dispatch arms
        // (`async_surface.rs`); they are unreachable here.
        | HirExpressionKind::Reddet(_)
        | HirExpressionKind::Tacebit(_)
        | HirExpressionKind::Yield(_)
        // Transfer lowers through its own visitor arm (sink-once/per-payload
        // calls); it is unreachable on the unsupported path.
        | HirExpressionKind::Transfer { .. }
        | HirExpressionKind::TypeCheck { .. } => "primitive expression",
    }
}

fn scribe_kind_name(kind: HirScribeKind) -> &'static str {
    match kind {
        HirScribeKind::Nota => "nota before print/runtime intrinsic MIR lowering",
        HirScribeKind::Vide => "vide before print/runtime intrinsic MIR lowering",
        HirScribeKind::Mone => "mone before print/runtime intrinsic MIR lowering",
        HirScribeKind::Scribe => "scribe before print/runtime intrinsic MIR lowering",
    }
}

#[cfg(test)]
#[path = "lower_test.rs"]
mod tests;

#[cfg(test)]
#[path = "lower_glyph_test.rs"]
mod glyph_tests;

#[cfg(test)]
#[path = "optional_field_unwrap_test.rs"]
mod optional_field_unwrap_tests;

#[cfg(test)]
#[path = "own_textus_test.rs"]
mod own_textus_tests;
