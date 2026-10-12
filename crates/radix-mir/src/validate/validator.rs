//! Validation core: the validated-program token, the driver entry point, and
//! the internal [`Validator`]/[`FunctionScope`] state passed between the
//! per-category check modules (`stmt`, `value`, `intrinsic`, `aggregate`,
//! `type_checks`).

use super::context::MirValidationContext;
use super::error::{MirFunctionSignature, MirParamSignature, MirValidationError};
use crate::device::{
    MirDeviceContext, MirDeviceFunctionMetadataMap, MirDeviceRole,
    reject_nucleum_fragment_survivors,
};
use crate::visit::MirVisitor;
use crate::{
    MirBlock, MirBlockId, MirFunction, MirFunctionId, MirFunctionIndex, MirGenericBindings,
    MirLocal, MirLocalId, MirParam, MirProgram, MirTemp, MirTempId, MirType, MirValueId,
};
use radix_hir::DefId;
use radix_lexer::Span;
use radix_types::FuncSig;
use rustc_hash::{FxHashMap, FxHashSet};

/// Validated MIR program proof — the type-level gate for all public MIR entry points.
///
/// `ValidatedMir` wraps a [`MirProgram`] together with the
/// [`MirValidationContext`] that proved it well-formed. It can only be
/// constructed by passing the program and context through
/// [`validate_program`]. Consuming [`ValidatedMir::into_parts`] forfeits the
/// proof token; [`ValidatedMir::patch_program`] restores one only after the
/// patched program passes validation again.
///
/// Callers that already hold a program and a known-good context can call
/// [`ValidatedMir::new`], which re-runs validation to produce the token.
///
/// Empty-context substitution is impossible: the constructor consumes the
/// context, so a bare `MirValidationContext::new(types)` can never produce
/// a `ValidatedMir` unless the program passes validation against it.
///
/// The token is immutable after construction. Callers that need to augment
/// the context (e.g. merging closure environments during package linking)
/// must clone the context, modify the clone, and construct a fresh token.
pub struct ValidatedMir<'a> {
    program: MirProgram,
    function_index: MirFunctionIndex,
    validation: MirValidationContext<'a>,
}

/// Failure from a consuming [`ValidatedMir::patch_program`] transition.
///
/// Neither variant retains the consumed program, validation context, or proof
/// token, so a rejected patch cannot expose mutated MIR as validated.
#[derive(Debug)]
pub enum PatchProgramError<E> {
    Patch(E),
    Validation(Vec<MirValidationError>),
}

impl<E> PatchProgramError<E> {
    /// Consume this error through caller-specific patch and validation
    /// adapters without exposing the consumed program, context, or token.
    pub fn fold<T>(
        self,
        patch: impl FnOnce(E) -> T,
        validation: impl FnOnce(Vec<MirValidationError>) -> T,
    ) -> T {
        match self {
            Self::Patch(error) => patch(error),
            Self::Validation(errors) => validation(errors),
        }
    }
}

impl std::fmt::Debug for ValidatedMir<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValidatedMir")
            .field("program", &self.program)
            .finish_non_exhaustive()
    }
}

impl Clone for ValidatedMir<'_> {
    fn clone(&self) -> Self {
        Self {
            program: self.program.clone(),
            function_index: self.function_index.clone(),
            validation: self.validation.clone(),
        }
    }
}

impl<'a> ValidatedMir<'a> {
    /// Build a validated-MIR token by running MIR validation.
    ///
    /// # Errors
    /// Returns [`Vec<MirValidationError>`] when the program does not satisfy
    /// all MIR well-formedness checks.
    pub fn new(
        program: MirProgram,
        mut context: MirValidationContext<'a>,
    ) -> Result<Self, Vec<MirValidationError>> {
        context.rebuild_variant_positions();
        validate_program(&program, &context)?;
        Ok(Self::from_validated(program, context))
    }

    /// Build a validated-MIR token and enforce the nucleum-fragment survivor
    /// invariant against `roles`.
    ///
    /// Ordinary `new` stays role-blind so fragment-free kernels do not
    /// false-positive. Tests and mutation proofs call this seam directly
    /// (no public skip-composition flag).
    ///
    /// # Errors
    /// Returns structural validation errors plus [`crate::NUCLEUM_FRAGMENT_SURVIVED`]
    /// when a fragment call or standalone fragment symbol remains.
    pub fn with_device_roles(
        program: MirProgram,
        context: MirValidationContext<'a>,
        roles: &FxHashMap<DefId, MirDeviceRole>,
    ) -> Result<Self, Vec<MirValidationError>> {
        Self::with_device_roles_and_metadata(
            program,
            context,
            roles,
            &MirDeviceFunctionMetadataMap::default(),
        )
    }

    /// Build a validated-MIR token with explicit source-less device metadata.
    ///
    /// The metadata is kept alongside the validation operation so role-aware
    /// checks classify synthetic private kernels without inferring from
    /// `source`, generated names, or shader stage.
    /// # Errors
    /// Returns an error when the input or persisted artifact is invalid.
    pub fn with_device_roles_and_metadata(
        program: MirProgram,
        mut context: MirValidationContext<'a>,
        roles: &FxHashMap<DefId, MirDeviceRole>,
        metadata: &MirDeviceFunctionMetadataMap,
    ) -> Result<Self, Vec<MirValidationError>> {
        context.rebuild_variant_positions();
        validate_program_with_device_roles_and_metadata(&program, &context, roles, metadata)?;
        Ok(Self::from_validated(program, context))
    }

    /// Assemble the proof token from a program that has just passed
    /// validation and the context that proved it.
    fn from_validated(program: MirProgram, context: MirValidationContext<'a>) -> Self {
        let function_index = program.function_index();
        Self {
            program,
            function_index,
            validation: context,
        }
    }

    /// The validated inner program.
    #[must_use]
    pub fn program(&self) -> &MirProgram {
        &self.program
    }

    /// Resolve a function through the immutable index captured with validation.
    #[must_use]
    pub fn function_by_id(&self, id: MirFunctionId) -> Option<&MirFunction> {
        self.program.indexed_function(&self.function_index, id)
    }

    /// The proving validation context consumed during construction.
    #[must_use]
    pub fn validation(&self) -> &MirValidationContext<'a> {
        &self.validation
    }

    /// Consume this proof token and return its now-unvalidated parts.
    ///
    /// Access to [`Self::program`] and [`Self::validation`] is forfeited until
    /// the caller constructs a new token by validating the resulting program.
    #[must_use]
    pub fn into_parts(self) -> (MirProgram, MirValidationContext<'a>) {
        (self.program, self.validation)
    }

    /// Consume this proof token, patch the unvalidated program, and validate
    /// it before returning a replacement token.
    ///
    /// # Errors
    /// Returns only the patch or validation error. The consumed program,
    /// context, and proof token are never returned on rejection.
    pub fn patch_program<E>(
        self,
        patch: impl FnOnce(&mut MirProgram) -> Result<(), E>,
    ) -> Result<Self, PatchProgramError<E>> {
        let (mut program, context) = self.into_parts();
        patch(&mut program).map_err(PatchProgramError::Patch)?;
        Self::new(program, context).map_err(PatchProgramError::Validation)
    }
}

/// Validate one MIR program and collect all structural/type-contract errors.
///
/// Validation is deliberately caller-supplied-context driven: MIR carries stable
/// references, while the context provides the semantic type table and signature
/// metadata needed to prove those references are coherent.
/// # Errors
/// Returns an error when validation or type resolution fails. See [`Vec<MirValidationError`].
pub fn validate_program(
    program: &MirProgram,
    context: &MirValidationContext<'_>,
) -> Result<(), Vec<MirValidationError>> {
    let mut validator = Validator {
        program,
        context,
        errors: Vec::new(),
        existing_type_index: std::cell::OnceCell::new(),
    };
    validator.validate_program();
    if validator.errors.is_empty() {
        Ok(())
    } else {
        Err(validator.errors)
    }
}

/// Validate structural MIR and reject leftover nucleum-fragment calls/symbols.
///
/// # Errors
/// Returns combined structural and [`crate::NUCLEUM_FRAGMENT_SURVIVED`] errors.
#[expect(
    clippy::implicit_hasher,
    reason = "FxHashMap is the crate-wide map type for device roles"
)]
pub fn validate_program_with_device_roles(
    program: &MirProgram,
    context: &MirValidationContext<'_>,
    roles: &FxHashMap<DefId, MirDeviceRole>,
) -> Result<(), Vec<MirValidationError>> {
    validate_program_with_device_roles_and_metadata(
        program,
        context,
        roles,
        &MirDeviceFunctionMetadataMap::default(),
    )
}

/// Validate MIR with HIR roles plus explicit source-less device metadata.
///
/// The separate metadata map keeps existing hand-built MIR fixtures source
/// compatible while making synthetic kernel role facts visible at validation.
///
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
#[expect(
    clippy::implicit_hasher,
    reason = "FxHashMap is the crate-wide map type for device roles"
)]
pub fn validate_program_with_device_roles_and_metadata(
    program: &MirProgram,
    context: &MirValidationContext<'_>,
    roles: &FxHashMap<DefId, MirDeviceRole>,
    metadata: &MirDeviceFunctionMetadataMap,
) -> Result<(), Vec<MirValidationError>> {
    let mut errors = match validate_program(program, context) {
        Ok(()) => Vec::new(),
        Err(errors) => errors,
    };
    let mut device = MirDeviceContext::from_hir_roles_with_metadata(roles.clone(), metadata);
    device.attach_program_with_metadata(program, metadata);
    errors.extend(reject_nucleum_fragment_survivors(program, &device));
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub(crate) struct Validator<'a, 'ctx> {
    pub(crate) program: &'a MirProgram,
    pub(crate) context: &'ctx MirValidationContext<'ctx>,
    pub(crate) errors: Vec<MirValidationError>,
    /// Lazily built once per validation run: the context's type table is
    /// borrowed immutably for the whole walk, so the index never goes stale.
    pub(crate) existing_type_index: std::cell::OnceCell<crate::generic::ExistingTypeIndex>,
}

pub(crate) struct FunctionScope<'prog, 'map> {
    pub(crate) function: &'prog MirFunction,
    /// Shared program-wide index — one map for all functions (PRP-053).
    pub(crate) functions_by_id: &'map FxHashMap<MirFunctionId, &'prog MirFunction>,
    pub(crate) locals: FxHashMap<MirLocalId, MirType>,
    pub(crate) temps: FxHashMap<MirTempId, MirType>,
    pub(crate) blocks: FxHashSet<MirBlockId>,
    /// Values are statement-local products in block order; clearing them per
    /// block prevents accidental cross-block SSA assumptions in this early MIR.
    pub(crate) values: FxHashMap<MirValueId, MirType>,
}

pub(crate) struct DuplicateFunctionFinder {
    ids: FxHashSet<MirFunctionId>,
    duplicates: Vec<(MirFunctionId, Span)>,
}

impl DuplicateFunctionFinder {
    pub(crate) fn find(program: &MirProgram) -> Vec<(MirFunctionId, Span)> {
        let mut finder = Self {
            ids: FxHashSet::default(),
            duplicates: Vec::new(),
        };
        finder.visit_program(program);
        finder.duplicates
    }
}

impl MirVisitor for DuplicateFunctionFinder {
    fn visit_function(&mut self, function: &MirFunction) {
        if !self.ids.insert(function.id) {
            self.duplicates.push((function.id, function.span));
        }
    }
}

pub(crate) struct FunctionShape {
    pub(crate) locals: FxHashMap<MirLocalId, MirType>,
    pub(crate) temps: FxHashMap<MirTempId, MirType>,
    pub(crate) blocks: FxHashSet<MirBlockId>,
    pub(crate) param_locals: FxHashSet<MirLocalId>,
    pub(crate) declared_locals: FxHashSet<MirLocalId>,
    pub(crate) duplicate_param_locals: Vec<(MirLocalId, Span)>,
    pub(crate) duplicate_locals: Vec<(MirLocalId, Span)>,
    pub(crate) conflicting_locals: Vec<(MirLocalId, Span)>,
    pub(crate) duplicate_temps: Vec<(MirTempId, Span)>,
    pub(crate) duplicate_blocks: Vec<(MirBlockId, Span)>,
}

impl FunctionShape {
    pub(crate) fn collect(function: &MirFunction) -> Self {
        let mut shape = Self {
            locals: FxHashMap::default(),
            temps: FxHashMap::default(),
            blocks: FxHashSet::default(),
            param_locals: FxHashSet::default(),
            declared_locals: FxHashSet::default(),
            duplicate_param_locals: Vec::new(),
            duplicate_locals: Vec::new(),
            conflicting_locals: Vec::new(),
            duplicate_temps: Vec::new(),
            duplicate_blocks: Vec::new(),
        };
        shape.visit_function(function);
        shape
    }
}

impl MirVisitor for FunctionShape {
    fn visit_param(&mut self, param: &MirParam) {
        if !self.param_locals.insert(param.local) {
            self.duplicate_param_locals.push((param.local, param.span));
            return;
        }
        self.locals.insert(param.local, param.ty);
    }

    fn visit_local(&mut self, local: &MirLocal) {
        if !self.declared_locals.insert(local.id) {
            self.duplicate_locals.push((local.id, local.span));
        }
        match self.locals.insert(local.id, local.ty) {
            Some(existing) if existing.semantic_id() != local.ty.semantic_id() => {
                self.conflicting_locals.push((local.id, local.span));
            }
            _ => {}
        }
    }

    fn visit_temp(&mut self, temp: &MirTemp) {
        if self.temps.insert(temp.id, temp.ty).is_some() {
            self.duplicate_temps.push((temp.id, temp.span));
        }
    }

    fn visit_block(&mut self, block: &MirBlock) {
        if !self.blocks.insert(block.id) {
            self.duplicate_blocks.push((block.id, block.span));
        }
    }
}

pub(crate) fn signature_from_function(function: &MirFunction) -> MirFunctionSignature {
    MirFunctionSignature {
        params: function
            .params
            .iter()
            // The lane id is a launch builtin, not a call argument.
            .filter(|param| param.mode != crate::MirParamMode::Lane)
            .map(|param| MirParamSignature {
                ty: param.ty,
                optional: false,
            })
            .collect(),
        return_ty: function.return_ty,
        error_ty: function.error_ty,
    }
}

pub(crate) fn signature_from_func_sig(sig: &FuncSig) -> MirFunctionSignature {
    MirFunctionSignature {
        params: sig
            .params
            .iter()
            .map(|param| MirParamSignature {
                ty: MirType::semantic(param.ty),
                optional: param.optional,
            })
            .collect(),
        return_ty: MirType::semantic(sig.ret),
        error_ty: sig.err.map(MirType::semantic),
    }
}

pub(crate) type GenericTypeBindings = MirGenericBindings;
