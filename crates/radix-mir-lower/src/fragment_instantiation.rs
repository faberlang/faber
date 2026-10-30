//! Per-instantiation nucleum-fragment checking after shape finalization.
//!
//! Every call from an `Entry` or `Fragment` is checked against the callee's
//! specialized signature. Under the v2 device contract (DFV2-4) a device
//! caller may call any concrete device function — entry or helper — so the
//! remaining matrix rows reject only non-device callees (host callables,
//! graphics stages) and host callers of fragments. Errors are call-site
//! primary; the callee definition is a secondary span.

use handroll::FxHashMap;
use radix_diagnostics::DiagnosticArg;
use radix_hir::DefId;
use radix_lexer::Span;
use radix_mir::device::MirDeviceFunctionMetadataMap;
use radix_mir::{
    MirCallee, MirDeviceRole, MirFunction, MirFunctionId, MirFunctionIndex, MirFunctionOrigin, MirKernelShaderStage,
    MirLayoutTable, MirMonomorphization, MirOperand, MirParamMode, MirPlace, MirPlaceBase, MirStatementKind,
    MirTerminatorKind, MirType, MirValueId, type_contains_param,
};
use radix_semantic::{
    Primitive, Type, TypeId, TypeTable, arg_value,
    passes::fragment_definition::{DeviceEffectKind, FragmentEffectTable},
};

pub const ISSUE_CONTEXT: &str = "nucleum_fragment_context";
pub const ISSUE_TYPE: &str = "nucleum_fragment_type";
/// A kernel that inlines a throwing (`⇥ text`) kernel must itself declare
/// `⇥ text`: the callee's constant throws become throw sites of the caller.
pub const ISSUE_THROW_CALLER: &str = "nucleum_fragment_throw_caller";

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
        let rendered = match self.issue {
            ISSUE_CONTEXT => Some(nucleum_fragment_context_message(self.reason, &self.args)),
            ISSUE_TYPE => nucleum_fragment_type_message(self.reason, &self.args),
            ISSUE_THROW_CALLER => {
                Some("@ nucleum caller of a throwing kernel must declare the same `⇥ text` error channel".to_owned())
            }
            _ => None,
        };
        rendered.unwrap_or_else(|| format!("{}: {}", self.issue, self.reason))
    }
}

/// Message prose for this module's issues lives here; the semantic catalog
/// carries only the code and help text for them.
fn nucleum_fragment_context_message(reason: &str, args: &[DiagnosticArg]) -> String {
    let caller = arg_value(args, "caller");
    match (caller, reason) {
        (_, "entry_callee") => "@ nucleum fragment cannot call an @ nucleum entry",
        (_, "nested_dispatch") => "@ nucleum cannot call another @ nucleum entry",
        (Some("entry"), "graphics_stage") => "@ nucleum cannot call a graphics @ fragment or @ vertex",
        (_, "graphics_stage") => "@ nucleum fragment cannot call a graphics @ fragment or @ vertex",
        (Some("host"), "host_callable") => "host code cannot call an @ nucleum fragment",
        (Some("entry"), "host_callable") => "@ nucleum cannot call a host function or other non-device-safe callable",
        (_, "host_callable") => "@ nucleum fragment cannot call a host function or other non-device-safe callable",
        _ => "@ nucleum fragment call is not legal in this context",
    }
    .to_owned()
}

fn nucleum_fragment_type_message(reason: &str, args: &[DiagnosticArg]) -> Option<String> {
    let expected = arg_value(args, "expected");
    let actual = arg_value(args, "actual");
    Some(match (reason, expected, actual) {
        ("bindings", _, _) => "@ nucleum fragment call has unsolved generic or magnitudo bindings".to_owned(),
        ("shape", Some(expected), Some(actual)) => {
            format!("@ nucleum fragment call shape {actual} does not match specialized shape {expected}")
        }
        ("shape", _, _) => "@ nucleum fragment call does not satisfy the specialized shape constraints".to_owned(),
        ("layout", Some(expected), Some(actual)) => {
            format!("@ nucleum fragment call layout {actual} does not match specialized layout {expected}")
        }
        ("layout", _, _) => "@ nucleum fragment call does not satisfy the specialized layout constraints".to_owned(),
        ("argument", Some(expected), Some(actual)) => {
            format!("@ nucleum fragment argument has type {actual}, expected {expected}")
        }
        ("argument", _, _) => "@ nucleum fragment argument does not match the specialized signature".to_owned(),
        ("mode", Some(expected), Some(actual)) => {
            format!("@ nucleum fragment argument mode {actual} is not valid for {expected}")
        }
        ("mode", _, _) => "@ nucleum fragment argument mode does not match the specialized signature".to_owned(),
        ("target", _, _) => {
            "@ nucleum fragment specialized types are not legal for the current device target".to_owned()
        }
        ("return", Some(expected), Some(actual)) => {
            format!("@ nucleum fragment returns {actual}, which does not fit caller type {expected}")
        }
        ("return", _, _) => "@ nucleum fragment return does not fit the caller".to_owned(),
        ("effect", _, _) => "@ nucleum fragment specialized effect summary does not fit the caller".to_owned(),
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoleKind {
    Entry,
    Fragment,
    /// A source-less synthetic kernel closure: device-side for call checking
    /// (kernel callers and fragments compose it; host callers launch it),
    /// never a launch entry. Classified only through the explicit metadata
    /// carrier — never from `source: None`, a name, or the compute stage.
    SyntheticKernel,
    Graphics,
    Ordinary,
}

impl RoleKind {
    fn caller_arg(self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::Fragment => "fragment",
            Self::SyntheticKernel => "synthetic",
            Self::Graphics | Self::Ordinary => "host",
        }
    }
}

/// Check every nucleum-relevant call against the specialized signature and
/// matrix, with the explicit metadata carrier for source-less synthetic
/// kernel closures (KCM-05). Programs lowered without kernel closures pass
/// no rows and classify exactly as before.
#[must_use]
pub fn check_with_metadata(
    functions: &[MirFunction],
    monomorphizations: &[MirMonomorphization],
    layouts: &MirLayoutTable,
    types: &TypeTable,
    roles: &FxHashMap<DefId, MirDeviceRole>,
    summaries: Option<&FragmentEffectTable>,
    metadata: &MirDeviceFunctionMetadataMap,
) -> Vec<FragmentInstantiationError> {
    let mut functions_by_source = FxHashMap::default();
    let mut value_types = FxHashMap::default();
    for function in functions {
        if let Some(source) = function.source {
            functions_by_source.entry(source).or_insert(function);
        }
        for block in &function.blocks {
            for statement in &block.statements {
                if let MirStatementKind::Assign { value, .. } = &statement.kind {
                    value_types
                        .entry((function.id, value.id))
                        .or_insert(value.ty);
                }
            }
        }
    }
    let mut checker = InstantiationChecker {
        functions,
        function_index: MirFunctionIndex::for_functions(functions),
        functions_by_source,
        value_types,
        monomorphizations,
        layouts,
        types,
        roles,
        metadata,
        summaries,
        errors: Vec::new(),
    };
    checker.check_program();
    checker.errors
}

struct InstantiationChecker<'a> {
    functions: &'a [MirFunction],
    function_index: MirFunctionIndex,
    functions_by_source: FxHashMap<DefId, &'a MirFunction>,
    value_types: FxHashMap<(MirFunctionId, MirValueId), MirType>,
    monomorphizations: &'a [MirMonomorphization],
    layouts: &'a MirLayoutTable,
    types: &'a TypeTable,
    roles: &'a FxHashMap<DefId, MirDeviceRole>,
    metadata: &'a MirDeviceFunctionMetadataMap,
    summaries: Option<&'a FragmentEffectTable>,
    errors: Vec<FragmentInstantiationError>,
}

impl<'a> InstantiationChecker<'a> {
    fn check_program(&mut self) {
        for function in self.functions {
            let caller = self.role_of_function(function);
            for block in &function.blocks {
                for stmt in &block.statements {
                    if let MirStatementKind::Call { callee, args, destination } = &stmt.kind {
                        self.check_call(caller, function, callee, args, destination.as_ref(), false, stmt.span);
                    }
                }
                if let MirTerminatorKind::TryCall { callee, args, destination, .. } = &block.terminator.kind {
                    self.check_call(
                        caller,
                        function,
                        callee,
                        args,
                        destination.as_ref(),
                        true,
                        block.terminator.span,
                    );
                }
            }
        }
    }

    fn check_call(
        &mut self,
        caller: RoleKind,
        caller_fn: &'a MirFunction,
        target: &MirCallee,
        args: &[MirOperand],
        destination: Option<&MirPlace>,
        failable: bool,
        span: Span,
    ) {
        let Some((callee_id, callee_role, definition_span)) = self.resolve_callee(target) else {
            if matches!(caller, RoleKind::Entry | RoleKind::Fragment | RoleKind::SyntheticKernel)
                && matches!(target, MirCallee::Closure(_) | MirCallee::Value(_))
            {
                self.context(caller, "host_callable", span, None);
            }
            return;
        };
        match (caller, callee_role) {
            // DFV2-4 (v2 design §1): device code may call device code —
            // entry or helper — and the call composes. Role-based call
            // legality is gone; the former `nested_dispatch` reject is now
            // the ordinary typed instantiation check.
            // KCM-05: a synthetic kernel closure is device-side for its
            // callers — a kernel caller or fragment composes it, it composes
            // entries/helpers itself, and a host caller's direct call is the
            // launch boundary (unmatched, so legal).
            (RoleKind::Entry, RoleKind::Entry)
            | (RoleKind::Entry | RoleKind::Fragment | RoleKind::SyntheticKernel, RoleKind::Fragment)
            | (RoleKind::Entry | RoleKind::Fragment, RoleKind::SyntheticKernel)
            | (RoleKind::SyntheticKernel, RoleKind::Entry | RoleKind::SyntheticKernel) => {
                self.check_instantiation(caller, caller_fn, target, callee_id, args, destination, failable, span);
            }
            (RoleKind::Fragment, RoleKind::Entry) => {
                self.context(caller, "entry_callee", span, Some(definition_span));
            }
            (RoleKind::Entry | RoleKind::Fragment | RoleKind::SyntheticKernel, RoleKind::Graphics) => {
                self.context(caller, "graphics_stage", span, Some(definition_span));
            }
            (RoleKind::Ordinary | RoleKind::Graphics, RoleKind::Fragment)
            | (RoleKind::Entry | RoleKind::Fragment | RoleKind::SyntheticKernel, RoleKind::Ordinary) => {
                self.context(caller, "host_callable", span, Some(definition_span));
            }
            _ => {}
        }
    }

    fn check_instantiation(
        &mut self,
        caller: RoleKind,
        caller_fn: &'a MirFunction,
        target: &MirCallee,
        callee_id: MirFunctionId,
        args: &[MirOperand],
        destination: Option<&MirPlace>,
        failable: bool,
        span: Span,
    ) {
        let Some(sig) = self.function_by_id(callee_id) else {
            return;
        };
        let definition_span = Some(sig.span);
        if !self.bindings_solved(target, sig) {
            self.ty(caller, "bindings", span, definition_span, None, None);
            return;
        }
        if let Some((expected, actual)) = self.shape_mismatch(caller_fn, sig, args) {
            self.ty(caller, "shape", span, definition_span, Some(&expected), Some(&actual));
            return;
        }
        if let Some((expected, actual)) = self.layout_mismatch(caller_fn, sig, args) {
            self.ty(caller, "layout", span, definition_span, Some(&expected), Some(&actual));
            return;
        }
        let callable_params = sig
            .params
            .iter()
            .filter(|param| is_callable_param(param))
            .collect::<Vec<_>>();
        if args.len() != callable_params.len() {
            self.ty(
                caller,
                "argument",
                span,
                definition_span,
                Some(&callable_params.len().to_string()),
                Some(&args.len().to_string()),
            );
            return;
        }
        for (param, arg) in callable_params.iter().zip(args.iter()) {
            if let Some(actual) = self.operand_ty(caller_fn, arg)
                && !self.types_fit(param.ty, actual)
            {
                let expected = self.type_label(param.ty);
                let actual = self.type_label(actual);
                self.ty(caller, "argument", span, definition_span, Some(&expected), Some(&actual));
                return;
            }
            if !Self::mode_legal(param.mode, arg) {
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
        if let Some(illegal) = self.target_illegal_type(sig) {
            self.ty(caller, "target", span, definition_span, None, Some(&illegal));
            return;
        }
        if let Some(dest) = destination
            && let Some(expected_ty) = Self::place_ty(caller_fn, dest)
            && !self.types_fit(expected_ty, sig.return_ty)
        {
            let expected = self.type_label(expected_ty);
            let actual = self.type_label(sig.return_ty);
            self.ty(caller, "return", span, definition_span, Some(&expected), Some(&actual));
            return;
        }
        if failable && throws_text(self.types, sig) {
            // U9-6b: a throwing kernel composes into a caller that can
            // itself throw its text; its constant throws become the
            // caller's throw sites.
            if !throws_text(self.types, caller_fn) {
                self.errors.push(FragmentInstantiationError {
                    issue: ISSUE_THROW_CALLER,
                    reason: "caller_not_failable",
                    span,
                    definition_span,
                    args: vec![DiagnosticArg::new("caller", caller.caller_arg())],
                });
            } else if !self.effect_fits(sig, true) {
                self.ty(caller, "effect", span, definition_span, None, None);
            }
            return;
        }
        if failable || !self.effect_fits(sig, false) {
            self.ty(caller, "effect", span, definition_span, None, None);
        }
    }

    fn bindings_solved(&self, callee: &MirCallee, sig: &MirFunction) -> bool {
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

    fn shape_mismatch(
        &self,
        caller_fn: &MirFunction,
        sig: &MirFunction,
        args: &[MirOperand],
    ) -> Option<(String, String)> {
        for (param, arg) in sig
            .params
            .iter()
            .filter(|param| is_callable_param(param))
            .zip(args.iter())
        {
            let Some(actual) = self.operand_ty(caller_fn, arg) else {
                continue;
            };
            if let Some((expected, got)) = self.tensor_shape_pair(param.ty, actual)
                && !self.types.index_equals(expected, got)
            {
                return Some((format!("{expected:?}"), format!("{got:?}")));
            }
        }
        None
    }

    fn layout_mismatch(
        &self,
        caller_fn: &MirFunction,
        sig: &MirFunction,
        args: &[MirOperand],
    ) -> Option<(String, String)> {
        for (param, arg) in sig
            .params
            .iter()
            .filter(|param| is_callable_param(param))
            .zip(args.iter())
        {
            if !self.needs_device_layout(param.ty) {
                continue;
            }
            let Some(actual) = self.operand_ty(caller_fn, arg) else {
                continue;
            };
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

    fn target_illegal_type(&self, sig: &MirFunction) -> Option<String> {
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

    fn effect_fits(&self, sig: &MirFunction, allow_text_throw: bool) -> bool {
        if (sig.error_ty.is_some() && !(allow_text_throw && throws_text(self.types, sig)))
            || sig.is_async
            || sig.is_generator
        {
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
                .or_else(|| self.functions_by_source.get(source).copied())?,
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
        // The explicit metadata carrier classifies source-less synthetic
        // kernel closures; without it a source-less compute-stage function
        // has no proven device identity and is never admitted as an entry
        // (fail-closed selection — KCM-05).
        if self
            .metadata
            .get(&function.id)
            .is_some_and(|metadata| metadata.origin == MirFunctionOrigin::SyntheticKernelClosure)
        {
            return RoleKind::SyntheticKernel;
        }
        match function.shader_stage {
            Some(MirKernelShaderStage::Fragment | MirKernelShaderStage::Vertex) => RoleKind::Graphics,
            Some(MirKernelShaderStage::Compute) | None => RoleKind::Ordinary,
        }
    }

    fn function_by_id(&self, id: MirFunctionId) -> Option<&'a MirFunction> {
        self.function_index.function(self.functions, id)
    }

    fn operand_ty(&self, function: &MirFunction, operand: &MirOperand) -> Option<MirType> {
        match operand {
            MirOperand::Place(place) => Self::place_ty(function, place),
            MirOperand::Temp(id) => function.temp_ty(*id),
            MirOperand::Value(id) => self.value_types.get(&(function.id, *id)).copied(),
            MirOperand::Constant(_) => None,
        }
    }

    /// Type of a projection-free place. A projected place (`v[0]`, `p.axis`,
    /// `t[i]`) has the type of its projection result, not of its base; this
    /// checker carries no field-type or element-type table, so it answers
    /// `None` (skip the check) and leaves projected places to MIR validation,
    /// which types every projection (`validate_place`) and checks call
    /// arguments against the callee signature.
    fn place_ty(function: &MirFunction, place: &MirPlace) -> Option<MirType> {
        if !place.projections.is_empty() {
            return None;
        }
        match place.base {
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
            MirPlaceBase::Temp(id) => function.temp_ty(id),
        }
    }

    fn types_fit(&self, expected: MirType, actual: MirType) -> bool {
        self.types.equals(
            resolve_ty(self.types, expected.semantic_id()),
            resolve_ty(self.types, actual.semantic_id()),
        )
    }

    fn tensor_shape_pair(
        &self,
        expected: MirType,
        actual: MirType,
    ) -> Option<(radix_semantic::IndexId, radix_semantic::IndexId)> {
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

    fn mode_legal(expected: MirParamMode, arg: &MirOperand) -> bool {
        match expected {
            MirParamMode::Owned | MirParamMode::Lane | MirParamMode::LaneInherited => true,
            MirParamMode::Ref | MirParamMode::MutRef | MirParamMode::Move => matches!(arg, MirOperand::Place(_)),
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

/// Whether the call argument list covers `param`. Lane bindings are filled by
/// the enclosing iteration, not by call arguments, so shape, layout, and
/// argument-count checks must all skip them identically.
fn is_callable_param(param: &radix_mir::MirParam) -> bool {
    !matches!(param.mode, MirParamMode::Lane | MirParamMode::LaneInherited)
}

/// Whether `function` declares the text error channel (`⇥ text`), the only
/// error a kernel can carry.
fn throws_text(types: &TypeTable, function: &MirFunction) -> bool {
    function
        .error_ty
        .is_some_and(|ty| matches!(types.get(ty.semantic_id()), Type::Primitive(Primitive::Textus)))
}

fn signature_has_unsolved_params(types: &TypeTable, sig: &MirFunction) -> bool {
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
        // These primitives are the host-side carriers the device cannot hold;
        // every other primitive is a fixed-width numeric.
        Type::Primitive(Primitive::Textus | Primitive::Ascii | Primitive::Octeti | Primitive::Ignotum) => false,
        Type::Primitive(_)
        | Type::SizedNumeric(_, _)
        | Type::ModularWord(_)
        | Type::SizedInstans(_)
        | Type::Tensor(_, _)
        | Type::Vector(_, _)
        | Type::Matrix(_, _)
        | Type::Sparsa(_, _)
        | Type::Atomic(_)
        | Type::Struct(_) => true,
        Type::Applied(inner, _) => is_device_safe_type(types, *inner),
        // WHY: fail closed — a type not enumerated here (param, infer, ref,
        // cursor, promissum, …) is rejected on the device target.
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
        MirParamMode::Ref => "de",
        MirParamMode::MutRef => "in",
        MirParamMode::Move => "ex",
        MirParamMode::Lane | MirParamMode::LaneInherited => "lane",
    }
}

fn operand_mode_label(arg: &MirOperand) -> &'static str {
    match arg {
        MirOperand::Place(_) => "place",
        MirOperand::Temp(_) | MirOperand::Value(_) | MirOperand::Constant(_) => "value",
    }
}
