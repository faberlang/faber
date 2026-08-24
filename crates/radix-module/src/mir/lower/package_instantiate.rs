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
use crate::hir::DefId;
use crate::lexer::{Interner, Span};
use crate::mir::generic::{
    bind_generic_type_witness_with_shapes, canonical_index, function_contains_shape_size,
    instantiate_existing_generic_type, instantiate_generic_type_with_shapes, substitute_shape_size_constants,
    MirShapeBindings,
};
use crate::mir::{
    type_contains_param, MirCallee, MirFunction, MirFunctionId, MirGenericBindings, MirMonomorphization,
    MirMonomorphizationKey, MirOperand, MirPlace, MirPlaceBase, MirProgram, MirStatementKind, MirTerminatorKind,
    MirType,
};
use crate::semantic::{IndexExpr, Primitive, Type, TypeTable};
use rustc_hash::{FxHashMap, FxHashSet};

const MAX_PACKAGE_MONOMORPHIZATIONS: usize = 64;
const UNRESOLVED_SHAPE_SIZE_SURVIVOR: &str = "unresolved shape-size constant survives monomorphization";

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
) -> Result<Vec<MirMonomorphization>, Vec<MirError>> {
    if templates.is_empty() {
        return Ok(Vec::new());
    }
    let mut state = PackageInstantiate {
        types,
        interner,
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
    Ok(state.monomorphizations)
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
    templates: FxHashMap<DefId, MirFunction>,
    generic_params: FxHashMap<DefId, Vec<MirGenericParam>>,
    instance_index: FxHashMap<MirMonomorphizationKey, MirFunctionId>,
    monomorphizations: Vec<MirMonomorphization>,
    next_id: MirFunctionId,
    errors: Vec<MirError>,
    created: Vec<MirFunction>,
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
        if !self.bind_explicit_type_args(&params, &call.type_args, &mut type_bindings, &mut shape_bindings, call.span) {
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
                let Some(value) = self.magnitudo_literal_of(concrete) else {
                    self.errors.push(MirError::validation(
                        span,
                        "generic call type argument does not match parameter kind",
                    ));
                    return false;
                };
                let Some(index) = find_literal_index(self.types, value) else {
                    self.errors.push(MirError::validation(
                        span,
                        "generic call signature has unresolved type parameter",
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
        if let Some(existing) = self.instance_index.get(key).copied() {
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
        Some(id)
    }

    fn substitute_function(&mut self, function: &mut MirFunction, bindings: &MirGenericBindings, span: Span) -> bool {
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
                    }
                    MirStatementKind::Construct { aggregate, .. } => {
                        aggregate.ty = match self.substitute_ty(aggregate.ty, bindings, span) {
                            Some(ty) => ty,
                            None => return false,
                        };
                    }
                    MirStatementKind::Call { callee, .. } => {
                        if !self.substitute_callee(callee, bindings, span) {
                            return false;
                        }
                    }
                }
            }
            if let MirTerminatorKind::TryCall { callee, .. } = &mut block.terminator.kind {
                if !self.substitute_callee(callee, bindings, span) {
                    return false;
                }
            }
        }
        true
    }

    fn substitute_callee(&mut self, callee: &mut MirCallee, bindings: &MirGenericBindings, span: Span) -> bool {
        let MirCallee::Definition { type_args, .. } = callee else {
            return true;
        };
        for type_arg in type_args {
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
        match place.base {
            MirPlaceBase::Local(id) => scope.locals.get(&id).copied(),
            MirPlaceBase::Temp(id) => scope.temps.get(&id).copied(),
        }
        .or_else(|| {
            self.errors
                .push(MirError::validation(span, "undefined place in generic call"));
            None
        })
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
