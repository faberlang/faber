//! Per-instantiation nucleum-fragment checking after shape finalization.
//!
//! Every call from an `Entry` or `Fragment` is checked against the callee's
//! specialized signature. The role call-legality matrix is enforced here so
//! illegal cells never reach composition. Errors are call-site primary; the
//! callee definition is a secondary span.

use crate::diagnostics::DiagnosticArg;
use crate::hir::DefId;
use crate::lexer::Span;
use crate::mir::{
    type_contains_param, MirCallee, MirDeviceRole, MirFunction, MirFunctionId, MirKernelShaderStage, MirLayoutTable,
    MirMonomorphization, MirOperand, MirParamMode, MirPlace, MirPlaceBase, MirStatementKind, MirTerminatorKind,
    MirType,
};
use crate::semantic::{
    passes::fragment_definition::{DeviceEffectKind, FragmentEffectTable},
    Primitive, Type, TypeId, TypeTable,
};
use rustc_hash::FxHashMap;

pub const ISSUE_CONTEXT: &str = "NUCLEUM_FRAGMENT_CONTEXT";
pub const ISSUE_TYPE: &str = "NUCLEUM_FRAGMENT_TYPE";

/// One instantiation or matrix diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentInstantiationError {
    pub issue: &'static str,
    pub reason: &'static str,
    pub span: Span,
    pub definition_span: Option<Span>,
    pub args: Vec<DiagnosticArg>,
}

impl FragmentInstantiationError {
    #[must_use]
    pub fn message(&self) -> String {
        crate::diagnostics::semantic_message(
            crate::semantic::SemanticErrorKind::InvalidOperandTypes,
            &self.catalog_args(),
        )
        .unwrap_or_else(|| format!("{}: {}", self.issue, self.reason))
    }

    #[must_use]
    pub fn catalog_args(&self) -> Vec<DiagnosticArg> {
        let mut args = vec![
            DiagnosticArg::new("issue", self.issue),
            DiagnosticArg::new("reason", self.reason),
            DiagnosticArg::new("site", "call"),
        ];
        args.extend(self.args.iter().cloned());
        args
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoleKind {
    Entry,
    Fragment,
    Graphics,
    Ordinary,
}

impl RoleKind {
    fn caller_arg(self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::Fragment => "fragment",
            Self::Graphics | Self::Ordinary => "host",
        }
    }
}

/// Check every nucleum-relevant call against the specialized signature and matrix.
#[must_use]
pub fn check(
    functions: &[MirFunction],
    monomorphizations: &[MirMonomorphization],
    layouts: &MirLayoutTable,
    types: &TypeTable,
    roles: &FxHashMap<DefId, MirDeviceRole>,
    summaries: Option<&FragmentEffectTable>,
) -> Vec<FragmentInstantiationError> {
    let mut checker =
        InstantiationChecker { functions, monomorphizations, layouts, types, roles, summaries, errors: Vec::new() };
    checker.check_program();
    checker.errors
}

struct InstantiationChecker<'a> {
    functions: &'a [MirFunction],
    monomorphizations: &'a [MirMonomorphization],
    layouts: &'a MirLayoutTable,
    types: &'a TypeTable,
    roles: &'a FxHashMap<DefId, MirDeviceRole>,
    summaries: Option<&'a FragmentEffectTable>,
    errors: Vec<FragmentInstantiationError>,
}

impl InstantiationChecker<'_> {
    fn check_program(&mut self) {
        for function in self.functions {
            let caller = self.role_of_function(function);
            for block in &function.blocks {
                for stmt in &block.statements {
                    if let MirStatementKind::Call { callee, args, destination } = &stmt.kind {
                        self.check_call(caller, callee, args, destination.as_ref(), false, stmt.span);
                    }
                }
                if let MirTerminatorKind::TryCall { callee, args, destination, .. } = &block.terminator.kind {
                    self.check_call(caller, callee, args, destination.as_ref(), true, block.terminator.span);
                }
            }
        }
    }

    fn check_call(
        &mut self,
        caller: RoleKind,
        callee: &MirCallee,
        args: &[MirOperand],
        destination: Option<&MirPlace>,
        failable: bool,
        span: Span,
    ) {
        let Some((callee_id, callee_role, definition_span)) = self.resolve_callee(callee) else {
            if matches!(caller, RoleKind::Entry | RoleKind::Fragment)
                && matches!(callee, MirCallee::Closure(_) | MirCallee::Value(_))
            {
                self.context(caller, "host_callable", span, None);
            }
            return;
        };
        match (caller, callee_role) {
            (RoleKind::Entry, RoleKind::Entry) => {
                self.context(caller, "nested_dispatch", span, Some(definition_span));
            }
            (RoleKind::Fragment, RoleKind::Entry) => {
                self.context(caller, "entry_callee", span, Some(definition_span));
            }
            (RoleKind::Entry | RoleKind::Fragment, RoleKind::Graphics) => {
                self.context(caller, "graphics_stage", span, Some(definition_span));
            }
            (RoleKind::Ordinary | RoleKind::Graphics, RoleKind::Fragment) => {
                self.context(caller, "host_callable", span, Some(definition_span));
            }
            (RoleKind::Entry | RoleKind::Fragment, RoleKind::Ordinary) => {
                self.context(caller, "host_callable", span, Some(definition_span));
            }
            (RoleKind::Entry | RoleKind::Fragment, RoleKind::Fragment) => {
                self.check_instantiation(caller, callee, callee_id, args, destination, failable, span);
            }
            _ => {}
        }
    }

    fn check_instantiation(
        &mut self,
        caller: RoleKind,
        callee: &MirCallee,
        callee_id: MirFunctionId,
        args: &[MirOperand],
        destination: Option<&MirPlace>,
        failable: bool,
        span: Span,
    ) {
        let Some(sig) = self.specialized_signature(callee_id) else {
            return;
        };
        let definition_span = Some(sig.span);
        if !self.bindings_solved(callee, &sig) {
            self.ty(caller, "bindings", span, definition_span, None, None);
            return;
        }
        if let Some((expected, actual)) = self.shape_mismatch(&sig, args) {
            self.ty(caller, "shape", span, definition_span, Some(&expected), Some(&actual));
            return;
        }
        if let Some((expected, actual)) = self.layout_mismatch(&sig, args) {
            self.ty(caller, "layout", span, definition_span, Some(&expected), Some(&actual));
            return;
        }
        if args.len() != sig.params.len() {
            self.ty(
                caller,
                "argument",
                span,
                definition_span,
                Some(&sig.params.len().to_string()),
                Some(&args.len().to_string()),
            );
            return;
        }
        for (param, arg) in sig.params.iter().zip(args.iter()) {
            if let Some(actual) = self.operand_ty(arg) {
                if !self.types_fit(param.ty, actual) {
                    let expected = self.type_label(param.ty);
                    let actual = self.type_label(actual);
                    self.ty(caller, "argument", span, definition_span, Some(&expected), Some(&actual));
                    return;
                }
            }
            if !self.mode_legal(param.mode, arg) {
                self.ty(
                    caller,
                    "mode",
                    span,
                    definition_span,
                    Some(mode_label(param.mode)),
                    Some(operand_mode_label(arg)),
                );
                return;
            }
        }
        if let Some(illegal) = self.target_illegal_type(&sig) {
            self.ty(caller, "target", span, definition_span, None, Some(&illegal));
            return;
        }
        if let Some(dest) = destination {
            if let Some(expected_ty) = self.place_ty(dest) {
                if !self.types_fit(expected_ty, sig.return_ty) {
                    let expected = self.type_label(expected_ty);
                    let actual = self.type_label(sig.return_ty);
                    self.ty(caller, "return", span, definition_span, Some(&expected), Some(&actual));
                    return;
                }
            }
        }
        if failable || !self.effect_fits(&sig) {
            self.ty(caller, "effect", span, definition_span, None, None);
        }
    }

    fn specialized_signature(&self, callee_id: MirFunctionId) -> Option<SpecializedSignature> {
        let function = self.function_by_id(callee_id)?;
        Some(SpecializedSignature {
            id: function.id,
            source: function.source,
            span: function.span,
            params: function
                .params
                .iter()
                .map(|param| SpecializedParam { ty: param.ty, mode: param.mode })
                .collect(),
            return_ty: function.return_ty,
            error_ty: function.error_ty,
            is_async: function.is_async,
            is_generator: function.is_generator,
        })
    }

    fn bindings_solved(&self, callee: &MirCallee, sig: &SpecializedSignature) -> bool {
        if signature_has_unsolved_params(self.types, sig) {
            return false;
        }
        match callee {
            MirCallee::Definition { source, type_args } => {
                if !type_args.is_empty() {
                    return self
                        .monomorphizations
                        .iter()
                        .any(|mono| mono.key.source == *source && mono.function == sig.id);
                }
                !self
                    .monomorphizations
                    .iter()
                    .any(|mono| mono.key.source == *source)
                    || self
                        .monomorphizations
                        .iter()
                        .any(|mono| mono.function == sig.id)
            }
            MirCallee::Function(_) => true,
            MirCallee::Closure(_) | MirCallee::Value(_) => false,
        }
    }

    fn shape_mismatch(&self, sig: &SpecializedSignature, args: &[MirOperand]) -> Option<(String, String)> {
        for (param, arg) in sig.params.iter().zip(args.iter()) {
            let actual = self.operand_ty(arg)?;
            if let Some((expected, got)) = self.tensor_shape_pair(param.ty, actual) {
                if expected != got {
                    return Some((format!("{expected:?}"), format!("{got:?}")));
                }
            }
        }
        None
    }

    fn layout_mismatch(&self, sig: &SpecializedSignature, args: &[MirOperand]) -> Option<(String, String)> {
        for (param, arg) in sig.params.iter().zip(args.iter()) {
            if !self.needs_device_layout(param.ty) {
                continue;
            }
            let actual = self.operand_ty(arg)?;
            let expected_layout = self.layouts.layout_for_type(param.ty).ok()?;
            let Ok(actual_layout) = self.layouts.layout_for_type(actual) else {
                return Some((format!("{:?}", expected_layout.kind), "missing".to_owned()));
            };
            if expected_layout.kind != actual_layout.kind {
                return Some((format!("{:?}", expected_layout.kind), format!("{:?}", actual_layout.kind)));
            }
        }
        None
    }

    fn target_illegal_type(&self, sig: &SpecializedSignature) -> Option<String> {
        for param in &sig.params {
            if !self.is_device_safe_type(param.ty) {
                return Some(self.type_label(param.ty));
            }
            if self.needs_device_layout(param.ty) && self.layouts.layout_for_type(param.ty).is_err() {
                return Some(self.type_label(param.ty));
            }
        }
        if !self.is_device_safe_type(sig.return_ty) {
            return Some(self.type_label(sig.return_ty));
        }
        None
    }

    fn effect_fits(&self, sig: &SpecializedSignature) -> bool {
        if sig.error_ty.is_some() || sig.is_async || sig.is_generator {
            return false;
        }
        let Some(summaries) = self.summaries else {
            return true;
        };
        let Some(source) = sig.source else {
            return true;
        };
        let Some(summary) = summaries.get(source) else {
            return true;
        };
        summary.effects.iter().all(|effect| {
            matches!(
                effect.kind,
                DeviceEffectKind::ArgumentRead
                    | DeviceEffectKind::ArgumentWrite
                    | DeviceEffectKind::FragmentCall
                    | DeviceEffectKind::DeviceIntrinsic
            )
        })
    }

    fn resolve_callee(&self, callee: &MirCallee) -> Option<(MirFunctionId, RoleKind, Span)> {
        let function = match callee {
            MirCallee::Function(id) => self.function_by_id(*id)?,
            MirCallee::Definition { source, type_args } => self
                .monomorphizations
                .iter()
                .find(|mono| mono.key.source == *source && (type_args.is_empty() || &mono.key.type_args == type_args))
                .and_then(|mono| self.function_by_id(mono.function))
                .or_else(|| {
                    self.functions
                        .iter()
                        .find(|function| function.source == Some(*source))
                })?,
            MirCallee::Closure(_) | MirCallee::Value(_) => return None,
        };
        Some((function.id, self.role_of_function(function), function.span))
    }

    fn role_of_function(&self, function: &MirFunction) -> RoleKind {
        if let Some(source) = function.source {
            match self.roles.get(&source) {
                Some(MirDeviceRole::Kernel) => return RoleKind::Entry,
                Some(MirDeviceRole::Fragment) => return RoleKind::Fragment,
                None => {}
            }
        }
        match function.shader_stage {
            Some(MirKernelShaderStage::Fragment | MirKernelShaderStage::Vertex) => RoleKind::Graphics,
            Some(MirKernelShaderStage::Compute) => RoleKind::Entry,
            None => RoleKind::Ordinary,
        }
    }

    fn function_by_id(&self, id: MirFunctionId) -> Option<&MirFunction> {
        self.functions.iter().find(|function| function.id == id)
    }

    fn operand_ty(&self, operand: &MirOperand) -> Option<MirType> {
        match operand {
            MirOperand::Place(place) => self.place_ty(place),
            MirOperand::Temp(id) => self
                .functions
                .iter()
                .find_map(|function| function.temp_ty(*id)),
            MirOperand::Value(id) => self.functions.iter().find_map(|function| {
                function.blocks.iter().find_map(|block| {
                    block.statements.iter().find_map(|stmt| match &stmt.kind {
                        MirStatementKind::Assign { value, .. } if value.id == *id => Some(value.ty),
                        _ => None,
                    })
                })
            }),
            MirOperand::Constant(_) => None,
        }
    }

    fn place_ty(&self, place: &MirPlace) -> Option<MirType> {
        match place.base {
            MirPlaceBase::Local(id) => self.functions.iter().find_map(|function| {
                function
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
                    })
            }),
            MirPlaceBase::Temp(id) => self
                .functions
                .iter()
                .find_map(|function| function.temp_ty(id)),
        }
    }

    fn types_fit(&self, expected: MirType, actual: MirType) -> bool {
        resolve_ty(self.types, expected.semantic_id()) == resolve_ty(self.types, actual.semantic_id())
    }

    fn tensor_shape_pair(
        &self,
        expected: MirType,
        actual: MirType,
    ) -> Option<(crate::semantic::IndexId, crate::semantic::IndexId)> {
        match (
            self.types
                .get(resolve_ty(self.types, expected.semantic_id())),
            self.types.get(resolve_ty(self.types, actual.semantic_id())),
        ) {
            (Type::Tensor(_, expected_shape), Type::Tensor(_, actual_shape))
            | (Type::Vector(_, expected_shape), Type::Vector(_, actual_shape))
            | (Type::Matrix(_, expected_shape), Type::Matrix(_, actual_shape))
            | (Type::Sparsa(_, expected_shape), Type::Sparsa(_, actual_shape)) => {
                Some((*expected_shape, *actual_shape))
            }
            _ => None,
        }
    }

    fn needs_device_layout(&self, ty: MirType) -> bool {
        matches!(
            self.types.get(resolve_ty(self.types, ty.semantic_id())),
            Type::Tensor(_, _) | Type::Vector(_, _) | Type::Matrix(_, _) | Type::Sparsa(_, _)
        )
    }

    fn is_device_safe_type(&self, ty: MirType) -> bool {
        is_device_safe_type(self.types, ty.semantic_id())
    }

    fn type_label(&self, ty: MirType) -> String {
        type_label(self.types, ty.semantic_id())
    }

    fn mode_legal(&self, expected: MirParamMode, arg: &MirOperand) -> bool {
        match expected {
            MirParamMode::Owned => true,
            MirParamMode::De | MirParamMode::In | MirParamMode::Ex => matches!(arg, MirOperand::Place(_)),
        }
    }

    fn context(&mut self, caller: RoleKind, reason: &'static str, span: Span, definition_span: Option<Span>) {
        self.errors.push(FragmentInstantiationError {
            issue: ISSUE_CONTEXT,
            reason,
            span,
            definition_span,
            args: vec![DiagnosticArg::new("caller", caller.caller_arg())],
        });
    }

    fn ty(
        &mut self,
        caller: RoleKind,
        reason: &'static str,
        span: Span,
        definition_span: Option<Span>,
        expected: Option<&str>,
        actual: Option<&str>,
    ) {
        let mut args = vec![DiagnosticArg::new("caller", caller.caller_arg())];
        if let Some(expected) = expected {
            args.push(DiagnosticArg::new("expected", expected));
        }
        if let Some(actual) = actual {
            args.push(DiagnosticArg::new("actual", actual));
        }
        self.errors
            .push(FragmentInstantiationError { issue: ISSUE_TYPE, reason, span, definition_span, args });
    }
}

struct SpecializedSignature {
    id: MirFunctionId,
    source: Option<DefId>,
    span: Span,
    params: Vec<SpecializedParam>,
    return_ty: MirType,
    error_ty: Option<MirType>,
    is_async: bool,
    is_generator: bool,
}

struct SpecializedParam {
    ty: MirType,
    mode: MirParamMode,
}

fn signature_has_unsolved_params(types: &TypeTable, sig: &SpecializedSignature) -> bool {
    sig.params
        .iter()
        .any(|param| type_contains_param(types, param.ty.semantic_id()))
        || type_contains_param(types, sig.return_ty.semantic_id())
        || sig
            .error_ty
            .is_some_and(|ty| type_contains_param(types, ty.semantic_id()))
}

fn resolve_ty(types: &TypeTable, ty: TypeId) -> TypeId {
    match types.get(ty) {
        Type::Alias(_, inner) => resolve_ty(types, *inner),
        _ => ty,
    }
}

fn is_device_safe_type(types: &TypeTable, ty: TypeId) -> bool {
    match types.get(resolve_ty(types, ty)) {
        Type::Primitive(Primitive::Textus | Primitive::Ascii | Primitive::Octeti | Primitive::Ignotum) => false,
        Type::Primitive(_) | Type::SizedNumeric(_, _) | Type::ModularWord(_) | Type::SizedInstans(_) => true,
        Type::Tensor(_, _) | Type::Vector(_, _) | Type::Matrix(_, _) | Type::Sparsa(_, _) | Type::Atomic(_) => true,
        Type::Struct(_) => true,
        Type::Alias(_, inner) | Type::Applied(inner, _) => is_device_safe_type(types, *inner),
        Type::Param(_) | Type::Infer(_) => false,
        Type::Array(_)
        | Type::Map(_, _)
        | Type::Set(_)
        | Type::Record(_)
        | Type::Promissum(_)
        | Type::PromissumFailable(_, _)
        | Type::Cursor(_)
        | Type::AsyncCursor(_, _)
        | Type::Ref(_, _)
        | Type::Func(_)
        | Type::Option(_)
        | Type::Intervallum(_)
        | Type::Enum(_)
        | Type::Interface(_) => false,
        _ => false,
    }
}

fn type_label(types: &TypeTable, ty: TypeId) -> String {
    match types.get(resolve_ty(types, ty)) {
        Type::Primitive(prim) => format!("{prim:?}"),
        Type::SizedNumeric(prim, width) => format!("{prim:?}<{width:?}>"),
        Type::Tensor(elem, shape) => format!("tensor<{}, {shape:?}>", type_label(types, *elem)),
        Type::Vector(elem, shape) => format!("vector<{}, {shape:?}>", type_label(types, *elem)),
        Type::Matrix(elem, shape) => format!("matrix<{}, {shape:?}>", type_label(types, *elem)),
        Type::Param(name) => format!("param#{name:?}"),
        Type::Infer(var) => format!("infer#{var:?}"),
        other => format!("{other:?}"),
    }
}

fn mode_label(mode: MirParamMode) -> &'static str {
    match mode {
        MirParamMode::Owned => "owned",
        MirParamMode::De => "de",
        MirParamMode::In => "in",
        MirParamMode::Ex => "ex",
    }
}

fn operand_mode_label(arg: &MirOperand) -> &'static str {
    match arg {
        MirOperand::Place(_) => "place",
        MirOperand::Temp(_) | MirOperand::Value(_) | MirOperand::Constant(_) => "value",
    }
}

#[cfg(test)]
#[path = "fragment_instantiation_test.rs"]
mod tests;
