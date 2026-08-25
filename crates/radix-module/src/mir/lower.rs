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
use crate::hir::visit::{walk_expr, walk_program, HirVisitor};
use crate::hir::{
    DefId, HirAnnotationValue, HirArrayElement, HirBinOp, HirBlock, HirCape, HirCasuArm, HirConst, HirConversioTarget,
    HirExpression, HirExpressionKind, HirExtremumOp, HirField, HirFunction, HirItem, HirItemKind, HirIteraMode,
    HirLiteral, HirLocal, HirMethod, HirNonNullKind, HirObjectField, HirObjectKey, HirOptionalChainKind, HirParamMode,
    HirPattern, HirRangeKind, HirScribeKind, HirStatement, HirStatementKind, HirStruct, HirUnOp,
};
use crate::lexer::{Interner, Span, Symbol};
use crate::mir::{
    dump_program, instantiate_existing_generic_type, type_contains_param, MirAggregate, MirAggregateFields,
    MirAggregateItem, MirAggregateKind, MirBinOp, MirBlock, MirBlockId, MirCallee, MirClosureCallee, MirClosureCapture,
    MirClosureEnvironment, MirClosureEnvironmentId, MirClosureValue, MirConstant, MirConversion, MirConversionFlavor,
    MirDeviceRole, MirDiagnosticKind, MirFilumCoordinate, MirFilumSumOp, MirFunction, MirFunctionId,
    MirGenericBindings, MirIntrinsic, MirKernelShaderStage, MirKeyValueOperand, MirLayoutTable,
    MirLocal as MirLocalDecl, MirLocalId, MirMonomorphization, MirNamedOperand, MirOperand, MirOptionChainLink,
    MirOptionOp, MirOptionUnwrapMode, MirParam, MirParamMode, MirPlace, MirPlaceBase, MirProgram, MirProjection,
    MirProvider, MirProviderKind, MirRuntimeCall, MirStatement, MirStatementKind, MirStructFieldDefaults,
    MirSwitchCase, MirTemp, MirTempId, MirTerminator, MirTerminatorKind, MirType, MirTypeCheckOp, MirUnOp,
    MirValidationContext, MirValue, MirValueId, MirValueKind, ValidatedMir,
};
use crate::semantic::{IndexExpr, NumericWidth, Primitive, Type, TypeId, TypeTable};
use radix_types::{MirCollectionOp, MirKernelBuiltin};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;

mod aggregate;
mod async_surface;
mod callable;
mod collection_higher_order;

mod context;
mod control;
mod expr;
#[cfg(test)]
#[path = "lower/first_match_test.rs"]
mod first_match_tests;
mod item;
mod monomorphize;
mod package_instantiate;
mod place;
mod runtime;
mod stmt;

use context::{function_param_map, struct_field_map, LoweringContextMaps};
use expr::HirExpressionLoweringVisitor;
use item::ItemLoweringPass;
pub use package_instantiate::instantiate_merged_generic_calls;

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

/// Lower a semantically analyzed HIR unit into a validated MIR unit.
///
/// The returned unit carries a program that has already passed MIR validation,
/// along with the validation token and lowering metadata. Lowering failures and
/// validation failures share the same error type so callers can treat the MIR
/// pipeline as one fail-closed developer command.
///
/// # Errors
/// Returns `MirError` values for unsupported HIR surfaces, missing semantic
/// types, sealed-block misuse, and post-lowering validation failures.
pub fn lower_analyzed_unit(
    unit: &mut AnalyzedModule,
    options: MirLoweringOptions,
) -> Result<LoweredMirUnit<'_>, Vec<MirError>> {
    lower_analyzed_unit_with_options(unit, options)
}

/// MIR output plus the proving validated-MIR token and auxiliary lowering data.
pub struct LoweredMirUnit<'a> {
    /// Unvalidated program copy for backward-compat mutation (driver post-lower patches).
    /// Prefer [`Self::validated`] for any path that feeds MIR entry points.
    pub program: MirProgram,
    /// Complete symbol table after lowering-added runtime and diagnostic symbols.
    pub interner: Interner,
    pub monomorphizations: Vec<MirMonomorphization>,
    pub closure_environments: Vec<MirClosureEnvironment>,
    /// Generic source bodies removed from [`Self::program`] after unit
    /// monomorphization. Package linking keeps them so a later unit can
    /// instantiate a shape the defining unit never called.
    pub generic_templates: Vec<MirFunction>,
    /// Declaration-order generic parameters for [`Self::generic_templates`],
    /// keyed by the template's `function.source` (rewritten to a synthetic
    /// def at package link). `is_shape` is `magnitudo`; otherwise `typus`.
    pub generic_params: Vec<(DefId, Vec<MirGenericParam>)>,
    /// Validated MIR token: proof that `program` (or a clone of it) passed
    /// [`validate_program`] against the enclosed context.
    pub validated: ValidatedMir<'a>,
    /// Owned companion-relation carrier (S3-A2): primal → companion device
    /// materialization facts, populated by the driver's
    /// [`apply_air_backward_bundle`](crate::driver::apply_air_backward_bundle)
    /// instead of being discarded after validation. The faber device-program
    /// constructor consumes it to lower generated companions into kernels.
    pub companions: radix_mir::device::MirCompanionMap,
}

impl std::fmt::Debug for LoweredMirUnit<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoweredMirUnit")
            .field("program", &self.program)
            .field("monomorphizations", &self.monomorphizations)
            .field("closure_environments", &self.closure_environments)
            .field("generic_templates", &self.generic_templates)
            .field("generic_params", &self.generic_params)
            .field("companions", &self.companions)
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

fn lower_analyzed_unit_with_options(
    unit: &mut AnalyzedModule,
    options: MirLoweringOptions,
) -> Result<LoweredMirUnit<'_>, Vec<MirError>> {
    ensure_map_iteration_array_types(&mut unit.types);
    ensure_itera_coordinate_array_types(&unit.hir, &mut unit.types);
    ensure_tensor_bracket_index_array_types(&unit.hir, &mut unit.types);
    // The layout table is rebuilt after monomorphization's shape finalization
    // so it covers the concrete shapes/type graph the instances carry.
    prepare_mir_lowering_interner(&mut unit.interner);
    // Merge the caller-supplied CLI record fields (parsed argv, adapter, or
    // package-MIR runtime records) with descriptor-derived fields keyed by the
    // `argumenta` record type: the shared default-path lowering fills every
    // CLI record from the analyzed `CliProgram` surface (option/operand
    // binding + typed defaults) so CLI programs lower through the same program
    // graph without caller input. Caller-supplied fields win per local.
    let cli_record_fields_by_type = cli_descriptor_record_fields_by_type(unit);
    let empty_textus = unit.interner.intern("");
    let diagnostics = DiagnosticSymbols {
        format_templates: [
            unit.interner.intern("§"),
            unit.interner.intern("§ §"),
            unit.interner.intern("§ § §"),
        ],
        separator: unit.interner.intern(" "),
        nihil: unit.interner.intern("nihil"),
    };
    let mut lowerer = MirLowerer {
        unit,
        options: MirLoweringOptions { cli_record_fields_by_type, ..options },
        empty_textus,
        diagnostics,
        errors: Vec::new(),
        functions: Vec::new(),
        closure_environments: Vec::new(),
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
        let interner = lowerer.unit.interner.clone();
        drop(lowerer);
        let mut program = MirProgram { functions };
        // Shape substitution interns concrete shapes/types into the type
        // arena, which requires `&mut unit.types` — unavailable while the
        // lowerer's immutable unit borrow is alive. Finalize runs now.
        let finalize_errors =
            monomorphize::finalize_monomorphized_shapes(unit, &mut program.functions, &monomorphizations);
        if !finalize_errors.is_empty() {
            return Err(finalize_errors);
        }
        let roles = crate::mir::device_roles_from_hir(&unit.hir);
        let layouts = MirLayoutTable::from_types(&unit.types);
        let summaries =
            crate::semantic::passes::fragment_definition::check(&unit.hir, &unit.resolver, &unit.types, &unit.interner)
                .summaries;
        let instantiation_errors = super::fragment_instantiation::check(
            &program.functions,
            &monomorphizations,
            &layouts,
            &unit.types,
            &roles,
            Some(&summaries),
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
        let composition_errors = super::fragment_composition::compose(
            &mut program.functions,
            &monomorphizations,
            &roles,
            super::fragment_composition::active_budget(),
            Some(&unit.interner),
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
        let mut validation = LoweringContextMaps::collect(unit).validation;
        align_function_signatures_with_mir_params(&mut validation, &program.functions);
        validation.layouts = MirLayoutTable::from_types(&unit.types);
        validation.closure_environments = closure_environments
            .iter()
            .map(|environment| (environment.id, environment.clone()))
            .collect();
        let validated = ValidatedMir::new(program.clone(), validation).map_err(|errors| {
            errors
                .into_iter()
                .map(|error| MirError::validation(error.span, error.message))
                .collect::<Vec<_>>()
        })?;
        Ok(LoweredMirUnit {
            program,
            interner,
            monomorphizations,
            closure_environments,
            generic_templates,
            generic_params,
            validated,
            companions: radix_mir::device::MirCompanionMap::default(),
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
    unit: &mut AnalyzedModule,
) -> Arc<FxHashMap<TypeId, Vec<MirRuntimeRecordField>>> {
    let Some(cli) = unit.cli_program.as_ref() else {
        return Arc::new(FxHashMap::default());
    };
    let mut fields_by_type = FxHashMap::default();
    if let Some(entry_ty) = cli_entry_args_type(unit) {
        let fields = cli_descriptor_record_fields(
            &mut unit.interner,
            &mut unit.types,
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
            &mut unit.interner,
            &mut unit.types,
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
pub fn dump_analyzed_unit(unit: &mut AnalyzedModule) -> Result<String, Vec<MirError>> {
    lower_analyzed_unit(unit, MirLoweringOptions::default()).map(|lowered| dump_program(&lowered.program))
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
                if object_is_tensor && !index_is_list {
                    if let Some(index_ty) = index.ty {
                        self.elements.insert(index_ty);
                    }
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
    /// The rendered `nihil` textus used by the scribe grouping when an option
    /// argument's payload is absent (`display_option` semantics).
    nihil: Symbol,
}

struct MirLowerer<'a> {
    unit: &'a AnalyzedModule,
    options: MirLoweringOptions,
    empty_textus: Symbol,
    diagnostics: DiagnosticSymbols,
    errors: Vec<MirError>,
    functions: Vec<MirFunction>,
    closure_environments: Vec<MirClosureEnvironment>,
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
}

struct CallableLoweringConfig<'input> {
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
        let Some(receiver_ty) = self.unit.types.find_struct(struct_item.def_id) else {
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
        target: CallableLoweringTarget<'_>,
        body_ctx: &BodyLoweringContext<'_, '_>,
    ) {
        let error_ty = function.err_ty.map(MirType::semantic);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; lowered function count is bounded by u32::MAX"
        )]
        let parent_function_id = MirFunctionId(self.functions.len() as u32);
        let CallableLoweringParts { params, locals, temps, blocks, errors, synthetics, closure_environments } = self
            .lower_callable_body(
                body_ctx,
                CallableLoweringConfig {
                    error_ty,
                    parent_function_id,
                    device_role: device_role_for_function(function),
                    target,
                },
            );
        self.errors.extend(errors);
        self.closure_environments.extend(closure_environments);

        let shader_stage = self.compute_shader_stage(function, span);
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
            &self.unit.interner,
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
            &self.unit.types,
            config.error_ty,
            context,
            config.parent_function_id,
            config.device_role,
            shader_stage,
        );
        let (body, cli_args) = match config.target {
            CallableLoweringTarget::Function(function) => {
                for param in &function.params {
                    builder.add_param(param.def_id, param.name, param.ty, param.mode, param.span);
                }
                let explicit_lane = lane_binding_local(function, &self.unit.interner);
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
                if let Some((def_id, name, ty, span)) = lane_binding_local(&method.func, &self.unit.interner) {
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
        CallableLoweringParts {
            params: builder.params,
            locals: builder.locals,
            temps: builder.temps,
            blocks,
            errors: builder.errors,
            synthetics,
            closure_environments,
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
        let vacuum = self.unit.types.primitive(Primitive::Vacuum);
        let context = self.builder_context(body_ctx);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; lowered function count is bounded by u32::MAX"
        )]
        let parent_function_id = MirFunctionId(self.functions.len() as u32);
        let (locals, temps, blocks, errors, synthetics, closure_environments) = {
            let mut builder =
                FunctionBuilder::for_function(&self.unit.types, None, context, parent_function_id, None, None);
            let entry_block = builder.fresh_block(entry.span);
            builder.switch_to(entry_block);
            let mut consts_lowered = true;
            for item in &self.unit.hir.items {
                if let HirItemKind::Constant(const_item) = &item.kind {
                    if builder
                        .lower_const_binding(item.def_id, const_item, item.span)
                        .is_none()
                    {
                        consts_lowered = false;
                        break;
                    }
                }
            }
            if consts_lowered {
                builder.visit_block(entry);
                builder.terminate_open_current(MirTerminatorKind::Return(None), entry.span);
            }
            let blocks = builder.finish_blocks();
            let synthetics = builder.take_synthetic_functions();
            let closure_environments = builder.take_closure_environments();
            (
                builder.locals,
                builder.temps,
                blocks,
                builder.errors,
                synthetics,
                closure_environments,
            )
        };
        self.errors.extend(errors);
        self.closure_environments.extend(closure_environments);

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

fn lane_binding_local(function: &HirFunction, interner: &Interner) -> Option<(DefId, Symbol, TypeId, Span)> {
    let lane = function.annotations.iter().find_map(|annotation| {
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
    accumulator: MirLocalId,
    ty: MirType,
}

#[derive(Debug, Clone)]
struct HandlerContext {
    error_place: MirPlace,
    error_ty: MirType,
    error_block: MirBlockId,
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
    sermo_def: Option<DefId>,
    meus_def: Option<DefId>,
    tuus_def: Option<DefId>,
    scrinium_def: Option<DefId>,
    scrinium_data_field: Option<Symbol>,
    top_level_consts: Arc<FxHashMap<DefId, &'a HirConst>>,
    provider_imports: Arc<FxHashMap<DefId, ProviderImport>>,
    method_targets: Arc<FxHashMap<(DefId, Symbol), MethodTarget>>,
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
            sermo_def: None,
            meus_def: None,
            tuus_def: None,
            scrinium_def: None,
            scrinium_data_field: None,
            top_level_consts: Arc::new(FxHashMap::default()),
            provider_imports: Arc::new(FxHashMap::default()),
            method_targets: Arc::new(FxHashMap::default()),
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
        if let HirExpressionKind::Path(def_id) = expr.kind {
            if self.top_level_consts.contains_key(&def_id) && self.seen.insert(def_id) {
                self.found.push(def_id);
            }
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
    materializing_consts: FxHashSet<DefId>,
    errors: Vec<MirError>,
    next_value: u32,
    synthetic_functions: Vec<MirFunction>,
    closure_environments: Vec<MirClosureEnvironment>,
    closure_temp_callees: FxHashMap<MirTempId, MirClosureCallee>,
    closure_local_callees: FxHashMap<MirLocalId, MirClosureCallee>,
    captured_bindings: FxHashMap<DefId, MirPlace>,
    captured_binding_metadata: FxHashMap<DefId, (Symbol, MirType)>,
    parent_function_id: MirFunctionId,
    device_role: Option<MirDeviceRole>,
    shader_stage: Option<MirKernelShaderStage>,
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
            materializing_consts: FxHashSet::default(),
            errors: Vec::new(),
            next_value: 0,
            synthetic_functions: Vec::new(),
            closure_environments: Vec::new(),
            closure_temp_callees: FxHashMap::default(),
            closure_local_callees: FxHashMap::default(),
            captured_bindings: FxHashMap::default(),
            captured_binding_metadata: FxHashMap::default(),
            parent_function_id,
            device_role,
            shader_stage,
        }
    }

    fn is_device_lowering(&self) -> bool {
        self.device_role.is_some()
    }

    fn add_param(&mut self, def_id: DefId, name: crate::lexer::Symbol, ty: TypeId, mode: HirParamMode, span: Span) {
        let local = self.next_local_id();
        // MIR carries access intent in `MirParamMode`; the local slot carries
        // the payload value type so MIR calls, returns, and places stay in the
        // storage domain. Every type-position access marker is a source-level
        // wrapper, including `own`/`copy` even though HIR collapses those to
        // value-owned mode.
        let storage_ty = match self.normalized_type(ty) {
            Type::Ref(_, inner) => *inner,
            _ => ty,
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
        self.bindings
            .insert(def_id, LocalBinding { local, ty: mir_ty });
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
    /// The item list is pre-built in [`lower_analyzed_unit_with_options`].
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
        let entry = self.fresh_block(body.span);
        self.switch_to(entry);
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

        let mir_ty = MirType::semantic(ty);
        let id = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id, name: Some(local.name), ty: mir_ty, mutable: local.mutable, span });

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
    ///      (`lower_conversio`) — `⇥` recovery runs only on conversion failure,
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
        let converted = self.lower_conversio(source, ty.semantic_id(), &[], recovery, expr)?;
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
        self.assign(place, value, ty, span);
    }

    fn lower_assignment_place_with_fallback(
        &mut self,
        expr: &HirExpression,
        fallback_ty: MirType,
    ) -> Option<(MirPlace, MirType)> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
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
            self.errors
                .push(MirError::unsupported(expr.span, "path that does not resolve to a local value"));
            return None;
        };

        Some(MirOperand::Place(MirPlace::local(binding.local)))
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
                // Upper-half u64 magnitude may not be sign-encoded into
                // `Int(i64)`; semantic analysis already rejected it for signed
                // targets, so reaching here is a contract violation.
                self.errors.push(MirError::unsupported(
                    span,
                    "unsigned integer literal above i64::MAX requires an unsigned target",
                ));
                None
            }
            HirLiteral::Float(value) => {
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
                Type::ModularWord(NumericWidth::U64) => return true,
                _ => return false,
            }
        }
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
        let Some(op) = mir_un_op(op) else {
            self.errors
                .push(MirError::unsupported(expr.span, "unary operator without a MIR primitive"));
            return None;
        };
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
        if matches!(op, HirBinOp::And | HirBinOp::Or) {
            return self.lower_logical_short_circuit(op, lhs, rhs, expr);
        }
        if matches!(op, HirBinOp::Is | HirBinOp::IsNot) {
            if let Some(predicate) = self.lower_is_predicate(op, lhs, rhs, expr) {
                return Some(predicate);
            }
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
        // Numeric tensor/vector/matrix `+`/`-` take the live `addita`/`subtrahe`
        // method paths (`TensorAdd` / `TensorSub`), never primitive scalar ops.
        if matches!(op, HirBinOp::Add | HirBinOp::Sub) && self.is_addita_plus_operands(lhs, rhs) {
            return self.lower_glyph_product(op, lhs, rhs, expr);
        }
        if matches!(op, HirBinOp::FuzzyEq | HirBinOp::FuzzyNotEq) {
            let lhs = self.lower_expr_value(lhs)?;
            let rhs = self.lower_expr_value(rhs)?;
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
        let Some(op) = mir_bin_op(op) else {
            self.errors
                .push(MirError::unsupported(expr.span, "binary operator without a MIR primitive"));
            return None;
        };
        let lhs = self.lower_expr_value(lhs)?;
        let rhs = self.lower_expr_value(rhs)?;
        let ty = self.expr_ty(expr)?;

        Some(self.assign_temp(MirValueKind::Binary { op, lhs, rhs }, ty, expr.span))
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
        let op = if matches!(op, HirBinOp::Is) {
            MirUnOp::IsNil
        } else {
            MirUnOp::IsNotNil
        };
        self.assign_temp(MirValueKind::Unary { op, operand }, result_ty, span)
    }

    fn lower_coalesce(&mut self, lhs: &HirExpression, rhs: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        let value = self.lower_expr_value(lhs)?;
        let fallback = self.lower_expr_value(rhs)?;
        let ty = self.expr_ty(expr)?;
        Some(self.assign_temp(MirValueKind::Option(MirOptionOp::Coalesce { value, fallback }), ty, expr.span))
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
        if let Some(closure) = self.closure_callee_for_operand(value) {
            self.closure_local_callees.insert(local, closure);
        } else {
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

    // consumes args into iterators
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
        HirExpressionKind::Si { .. } => "si before control-flow MIR lowering",
        HirExpressionKind::Discerne { .. } => "discerne before switch MIR lowering",
        HirExpressionKind::Loop(_) => "loop before control-flow MIR lowering",
        HirExpressionKind::Dum(_, _) => "dum before control-flow MIR lowering",
        HirExpressionKind::Itera(_, _, _, _, _, _) => "itera before iterator MIR lowering",
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
        HirExpressionKind::ReadLine(_) => "lege before input MIR lowering",
        HirExpressionKind::Praefixum(_) => "praefixum before compile-time MIR lowering",
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
