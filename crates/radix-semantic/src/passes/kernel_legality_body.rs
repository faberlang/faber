//! Kernel body walk: the expression and call rules of kernel legality
//! (gpu-reset rule 12), and the host-side rule that only public kernels are
//! called from host code.
//!
//! Boundary: read-only HIR traversal driven by [`super::kernel_legality`];
//! every violation goes through the shared sink so a construct is reported
//! once per span.

use super::fragment_definition::struct_def_from_type;
use super::kernel_legality::{
    Env, ISSUE_ASYNC, ISSUE_HOST_CALL, ISSUE_HOST_EFFECT, ISSUE_HOST_TYPE,
    ISSUE_POSITIONAL_LANE_PARAM, ISSUE_PRIVATE_HOST_CALL, ISSUE_RANGED_INT_NOT_BUILT,
    ISSUE_RAW_LANE_ID, ISSUE_RAW_LANE_ID_PARAM, ISSUE_RECOVERY, ISSUE_SATURATING_FLOAT_NOT_BUILT,
    ISSUE_TYPED_ERROR_CHANNEL, ISSUE_UNBOUNDED_LIST_SIZE, ISSUE_WORKGROUP, Sink,
};
use super::kernel_legality_types::{
    FAMILY_ATOMIC, FAMILY_RANGED_INT, FAMILY_SATURATING_FLOAT, FAMILY_UNBOUNDED_LIST_SIZE,
    host_only_family,
};
use crate::file_interface::{InterfaceDeviceRole, InterfaceDeviceVisibility};
use crate::{
    NumericWidth, Primitive, ResolvedTensorEdgePolicy, SymbolKind, Type, TypeId, WarningKind,
};
use radix_hir::visit::{HirVisitor, walk_expr, walk_local};
use radix_hir::{
    DefId, HirBlock, HirCallArg, HirExpression, HirExpressionKind, HirFunction, HirLocal,
    NucleumRole,
};
use radix_lexer::{Span, Symbol};
use radix_types::MirKernelBuiltin;

/// Check one `@ nucleum` definition. Returns the kernels it calls, for the
/// recursion facts.
pub(super) fn check_kernel_function(
    env: &Env<'_>,
    sink: &mut Sink,
    function: &HirFunction,
    span: Span,
) -> Vec<DefId> {
    walk_kernel_body(env, sink, function, span, true)
}

/// Check one ordinary custom tensor-edge policy method as an exact device
/// candidate. It shares body/type/effect checks with kernels but has no kernel
/// positional lane ABI.
pub(super) fn check_policy_callback(
    env: &Env<'_>,
    sink: &mut Sink,
    function: &HirFunction,
    span: Span,
) -> Vec<DefId> {
    walk_kernel_body(env, sink, function, span, false)
}

/// Run the device-body rules over `function` and collect the kernels it calls.
/// Policy callbacks pass `check_lane_abi: false`: they share the body, type and
/// effect checks but not the kernel launch ABI.
fn walk_kernel_body(
    env: &Env<'_>,
    sink: &mut Sink,
    function: &HirFunction,
    span: Span,
    check_lane_abi: bool,
) -> Vec<DefId> {
    let mut walk = KernelWalk {
        env,
        sink,
        function_span: span,
        callees: Vec::new(),
        quiet: 0,
        check_lane_abi,
        region: false,
    };
    walk.check_signature(function);
    if let Some(body) = &function.body {
        walk.visit_block(body);
    }
    walk.callees
}

/// Check the body of a `fac machina` region under the kernel body rules. The
/// block is kernel code by definition, but it is not a function: no signature
/// or lane ABI applies, and the kernels it calls must be launchable.
pub(super) fn check_region_body(
    env: &Env<'_>,
    sink: &mut Sink,
    region: &super::device_region::RegionBody<'_>,
) {
    let mut walk = KernelWalk {
        env,
        sink,
        function_span: region.span,
        callees: Vec::new(),
        quiet: 0,
        check_lane_abi: false,
        region: true,
    };
    for statement in region.statements {
        walk.visit_stmt(statement);
    }
    if let Some(tail) = region.tail {
        walk.visit_expr(tail);
    }
}

/// Check a `@ nucleum fragment` body for raw lane ids only; the fragment's
/// device-safety contract is owned by `fragment_definition`.
pub(super) fn check_fragment_lane_ids(env: &Env<'_>, sink: &mut Sink, function: &HirFunction) {
    if let Some(body) = &function.body {
        LaneIdScan { env, sink }.visit_block(body);
    }
}

/// Check a non-kernel function for calls to private kernels.
pub(super) fn check_host_function(env: &Env<'_>, sink: &mut Sink, function: &HirFunction) {
    if let Some(body) = &function.body {
        check_host_block(env, sink, body);
    }
}

/// Check host statements (a function body or the entry block).
pub(super) fn check_host_block(env: &Env<'_>, sink: &mut Sink, block: &HirBlock) {
    HostWalk { env, sink }.visit_block(block);
}

// ---------------------------------------------------------------------------
// Kernel bodies
// ---------------------------------------------------------------------------

struct KernelWalk<'a, 'e> {
    env: &'a Env<'e>,
    sink: &'a mut Sink,
    function_span: Span,
    callees: Vec<DefId>,
    /// Depth of subtrees whose value types are not checked: operands of a
    /// host operation (already reported) and error payloads (rule 12 point 4).
    quiet: u32,
    /// Callback candidates use device body rules but have no kernel launch
    /// ABI, so positional-lane and builtin-name checks do not apply.
    check_lane_abi: bool,
    /// The walk covers a `fac machina` region (host code that is checked as
    /// kernel code): a callee must be a launchable kernel.
    region: bool,
}

impl KernelWalk<'_, '_> {
    fn check_signature(&mut self, function: &HirFunction) {
        if function.is_async || function.posture.is_async() {
            self.sink.report(
                ISSUE_ASYNC,
                self.function_span,
                &[("construct", "async_function")],
            );
        }
        if function.is_generator || function.posture.is_stream() {
            self.sink.report(
                ISSUE_ASYNC,
                self.function_span,
                &[("construct", "generator_function")],
            );
        }
        for param in &function.params {
            self.check_type(param.ty, param.span, "parameter");
        }
        if let Some(ret) = function.ret_ty {
            self.check_type(ret, self.function_span, "return");
        }
        if let Some(err) = function.err_ty
            && !matches!(self.env.types.get(err), Type::Primitive(Primitive::Textus))
        {
            let kernel = self.env.interner.resolve(function.name);
            self.sink.report(
                ISSUE_TYPED_ERROR_CHANNEL,
                self.function_span,
                &[("kernel", kernel)],
            );
        }
        if self.check_lane_abi {
            self.check_lane_parameters(function);
        }
    }

    /// Lane-id plumbing in the signature (gpu-reset rule 12.5). A parameter
    /// named after a thread-id builtin is the raw lane id under another
    /// spelling and is rejected. A positional trailing `u32` of any other name
    /// is still the compiler-bound lane (the `[inputs.., output, index]` ABI in
    /// `radix-mir`): it warns until `filum` replaces the positional form.
    fn check_lane_parameters(&mut self, function: &HirFunction) {
        for param in &function.params {
            let name = self.env.interner.resolve(param.name);
            if let Some(builtin) = MirKernelBuiltin::from_spelling(name) {
                self.sink.report(
                    ISSUE_RAW_LANE_ID_PARAM,
                    param.span,
                    &[("builtin", builtin.spelling()), ("parameter", name)],
                );
            }
        }
        let returns_value = function.ret_ty.is_some_and(|ty| {
            !matches!(self.env.types.get(ty), Type::Primitive(Primitive::Vacuum))
        });
        if returns_value || function.params.len() < 3 {
            return;
        }
        let Some(last) = function.params.last() else {
            return;
        };
        let name = self.env.interner.resolve(last.name);
        let is_u32 = matches!(
            self.env.types.get(last.ty),
            Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32)
        );
        if is_u32 && MirKernelBuiltin::from_spelling(name).is_none() {
            let kernel = self.env.interner.resolve(function.name);
            self.sink.warn(
                WarningKind::PositionalLaneParam,
                ISSUE_POSITIONAL_LANE_PARAM,
                last.span,
                &[("kernel", kernel), ("parameter", name)],
            );
        }
    }

    fn check_type(&mut self, ty: TypeId, span: Span, site: &str) {
        let Some(family) = host_only_family(ty, self.env.types, self.env.facts) else {
            return;
        };
        if family == FAMILY_ATOMIC {
            self.sink.report(
                ISSUE_WORKGROUP,
                span,
                &[("construct", family), ("site", site)],
            );
        } else if family == FAMILY_SATURATING_FLOAT {
            self.sink
                .report(ISSUE_SATURATING_FLOAT_NOT_BUILT, span, &[("site", site)]);
        } else if family == FAMILY_UNBOUNDED_LIST_SIZE {
            self.sink
                .report(ISSUE_UNBOUNDED_LIST_SIZE, span, &[("site", site)]);
        } else if family == FAMILY_RANGED_INT {
            self.sink
                .report(ISSUE_RANGED_INT_NOT_BUILT, span, &[("site", site)]);
        } else {
            self.sink
                .report(ISSUE_HOST_TYPE, span, &[("family", family), ("site", site)]);
        }
    }

    fn check_value(&mut self, expr: &HirExpression) {
        if self.quiet == 0
            && let Some(ty) = expr.ty
        {
            self.check_type(ty, expr.span, "expression");
        }
    }

    fn effect(&mut self, span: Span, operation: &str, reason: &str) {
        self.sink.report(
            ISSUE_HOST_EFFECT,
            span,
            &[("operation", operation), ("reason", reason)],
        );
    }

    fn asynchronous(&mut self, span: Span, construct: &str) {
        self.sink
            .report(ISSUE_ASYNC, span, &[("construct", construct)]);
    }

    fn recovery(&mut self, span: Span, construct: &str) {
        self.sink
            .report(ISSUE_RECOVERY, span, &[("construct", construct)]);
    }

    /// Walk the children of `expr` with value-type checks suppressed.
    fn walk_quiet(&mut self, expr: &HirExpression) {
        self.quiet += 1;
        walk_expr(self, expr);
        self.quiet -= 1;
    }

    fn check_call(&mut self, expr: &HirExpression, callee: &HirExpression) {
        let HirExpressionKind::Path(def_id) = &callee.kind else {
            let namespace_member = match &callee.kind {
                HirExpressionKind::Field(object, member) => {
                    self.check_namespace_call(expr.span, object, *member)
                }
                _ => false,
            };
            if !namespace_member {
                self.dynamic_call(expr.span, "expression");
            }
            return;
        };
        if let Some(builtin) = self.env.resolver.gpu_builtin(*def_id) {
            self.sink.report(
                ISSUE_RAW_LANE_ID,
                expr.span,
                &[("builtin", builtin.spelling())],
            );
            return;
        }
        if self.env.facts.kernels.contains_key(def_id) {
            self.callees.push(*def_id);
        } else if self.env.facts.host_functions.contains(def_id) {
            self.not_a_kernel(expr.span, *def_id);
        }
        // Anything else is a local binding (the closure question belongs to
        // the kernel-closure contract) or an import whose device role the
        // resolver does not carry.
    }

    /// A call through an imported namespace (`kernels.advance(...)`) is a
    /// `MethodCall` on the module path (a `Call(Field(Path(module), member))`
    /// callee is the same shape spelled as a field). The imported `@ nucleum`
    /// contract says whether the member is a kernel. Returns false when the
    /// receiver is not a namespace.
    fn check_namespace_call(&mut self, span: Span, object: &HirExpression, member: Symbol) -> bool {
        let HirExpressionKind::Path(module) = &object.kind else {
            return false;
        };
        let Some(namespace) = self
            .env
            .resolver
            .get_symbol(*module)
            .filter(|symbol| symbol.kind == SymbolKind::Module)
        else {
            return false;
        };
        let qualified = format!(
            "{}.{}",
            self.env.interner.resolve(namespace.name),
            self.env.interner.resolve(member)
        );
        let contract = self
            .env
            .resolver
            .namespace_file_device_contract_canonical(namespace.name, member);
        match contract {
            Some(contract)
                if contract.role == InterfaceDeviceRole::Kernel
                    || (contract.role == InterfaceDeviceRole::Fragment && !self.region) =>
            {
                if self.region && contract.visibility == InterfaceDeviceVisibility::Private {
                    self.sink
                        .report(ISSUE_PRIVATE_HOST_CALL, span, &[("callee", &qualified)]);
                }
            }
            _ => self.sink.report(
                ISSUE_HOST_CALL,
                span,
                &[("callee", &qualified), ("reason", "not_a_kernel")],
            ),
        }
        true
    }

    fn check_method_call(
        &mut self,
        expr: &HirExpression,
        receiver: &HirExpression,
        method: Symbol,
    ) {
        if self.check_namespace_call(expr.span, receiver, method) {
            return;
        }
        let Some(receiver_ty) = receiver.ty else {
            return;
        };
        if is_interface(receiver_ty, self.env) {
            self.dynamic_call(expr.span, "pactum");
            return;
        }
        let Some(struct_def) = struct_def_from_type(receiver_ty, self.env.types) else {
            return;
        };
        let Some(target) = self
            .env
            .facts
            .method_index
            .get(&(struct_def, method))
            .copied()
        else {
            return;
        };
        if self.env.facts.kernels.contains_key(&target) {
            self.callees.push(target);
        } else {
            self.not_a_kernel(expr.span, target);
        }
    }

    fn not_a_kernel(&mut self, span: Span, callee: DefId) {
        let name = callee_name(self.env, callee);
        self.sink.report(
            ISSUE_HOST_CALL,
            span,
            &[("callee", &name), ("reason", "not_a_kernel")],
        );
    }

    fn dynamic_call(&mut self, span: Span, callee: &str) {
        self.sink.report(
            ISSUE_HOST_CALL,
            span,
            &[("callee", callee), ("reason", "dynamic")],
        );
    }
}

impl HirVisitor for KernelWalk<'_, '_> {
    fn visit_local(&mut self, local: &HirLocal) {
        if local.is_await {
            let span = local
                .init
                .as_ref()
                .map_or(self.function_span, |init| init.span);
            self.asynchronous(span, "await");
        }
        if let Some(ty) = local.ty
            && self.quiet == 0
        {
            let span = local
                .init
                .as_ref()
                .map_or(self.function_span, |init| init.span);
            self.check_type(ty, span, "local");
        }
        walk_local(self, local);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        if let Some(ResolvedTensorEdgePolicy::Custom {
            remap, transform, ..
        }) = self.env.tensor_edge_policies.calls.get(&expr.id)
        {
            self.callees.push(*remap);
            self.callees.extend(transform.iter().copied());
        }
        match &expr.kind {
            HirExpressionKind::Scribe(..) => {
                self.effect(expr.span, "scribe", "console");
                self.walk_quiet(expr);
            }
            HirExpressionKind::Scriptum(..) => {
                self.effect(expr.span, "scriptum", "text_construction");
                self.walk_quiet(expr);
            }
            HirExpressionKind::Format { .. } => {
                self.effect(expr.span, "format", "text_construction");
                self.walk_quiet(expr);
            }
            HirExpressionKind::ReadLine(_) => self.effect(expr.span, "read_line", "console"),
            HirExpressionKind::Ad { .. } => {
                self.effect(expr.span, "ad", "conversation");
                self.walk_quiet(expr);
            }
            HirExpressionKind::Cede(_) => self.asynchronous(expr.span, "cede"),
            HirExpressionKind::Yield(_) => self.asynchronous(expr.span, "yield"),
            HirExpressionKind::Reddet(_) | HirExpressionKind::Tacebit(_) => {
                self.asynchronous(expr.span, "promise_result");
            }
            HirExpressionKind::Handled { .. } => {
                self.recovery(expr.span, "cape");
                walk_expr(self, expr);
            }
            HirExpressionKind::Trap(_) => {
                self.recovery(expr.span, "capta");
                walk_expr(self, expr);
            }
            // `call ⊥ default` and `n ↦ T ⊥ default` select a value and
            // never unwind (compare plus select on a device); they install
            // no handler (operator ruling 2026-10-08, KCONVDEF).
            HirExpressionKind::CallDefault { .. } => walk_expr(self, expr),
            HirExpressionKind::Throw(_)
            | HirExpressionKind::Panic(_)
            | HirExpressionKind::Adfirma(..) => self.walk_quiet(expr),
            HirExpressionKind::Call(callee, _, args, _) => {
                self.check_call(expr, callee);
                self.walk_call_parts(callee, args);
            }
            HirExpressionKind::MethodCall(receiver, method, _, args, _) => {
                self.check_method_call(expr, receiver, *method);
                self.visit_expr(receiver);
                self.walk_args(args);
            }
            // Only an array literal allocates. A text literal is a
            // compile-time constant (a swizzle selector, an error message);
            // where it becomes a value its declared type is checked.
            HirExpressionKind::Array(_) => {
                self.check_value(expr);
                walk_expr(self, expr);
            }
            _ => walk_expr(self, expr),
        }
    }
}

impl KernelWalk<'_, '_> {
    fn walk_call_parts(&mut self, callee: &HirExpression, args: &[HirCallArg]) {
        if !matches!(callee.kind, HirExpressionKind::Path(_)) {
            self.visit_expr(callee);
        }
        self.walk_args(args);
    }

    fn walk_args(&mut self, args: &[HirCallArg]) {
        for arg in args {
            self.visit_expr(&arg.expr);
        }
    }
}

/// Reports every thread-id builtin call in a body.
struct LaneIdScan<'a, 'e> {
    env: &'a Env<'e>,
    sink: &'a mut Sink,
}

impl HirVisitor for LaneIdScan<'_, '_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Call(callee, ..) = &expr.kind
            && let HirExpressionKind::Path(def_id) = &callee.kind
            && let Some(builtin) = self.env.resolver.gpu_builtin(*def_id)
        {
            self.sink.report(
                ISSUE_RAW_LANE_ID,
                expr.span,
                &[("builtin", builtin.spelling())],
            );
        }
        walk_expr(self, expr);
    }
}

fn is_interface(ty: TypeId, env: &Env<'_>) -> bool {
    match env.types.get(ty) {
        Type::Interface(_) => true,
        Type::Ref(_, inner) | Type::Alias(_, inner) => is_interface(*inner, env),
        _ => false,
    }
}

fn callee_name(env: &Env<'_>, callee: DefId) -> String {
    env.facts
        .names
        .get(&callee)
        .map(|name| env.interner.resolve(*name).to_owned())
        .or_else(|| {
            env.resolver
                .get_symbol(callee)
                .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
                .map(|symbol| env.interner.resolve(symbol.name).to_owned())
        })
        .unwrap_or_else(|| "callee".to_owned())
}

// ---------------------------------------------------------------------------
// Host code calling kernels
// ---------------------------------------------------------------------------

struct HostWalk<'a, 'e> {
    env: &'a Env<'e>,
    sink: &'a mut Sink,
}

impl HostWalk<'_, '_> {
    fn check_target(&mut self, span: Span, target: DefId) {
        let Some(fact) = self.env.facts.kernels.get(&target) else {
            return;
        };
        if fact.role == NucleumRole::Entry && !fact.public {
            let name = callee_name(self.env, target);
            self.sink
                .report(ISSUE_PRIVATE_HOST_CALL, span, &[("callee", &name)]);
        }
    }
}

impl HirVisitor for HostWalk<'_, '_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Call(callee, ..) => {
                if let HirExpressionKind::Path(def_id) = &callee.kind {
                    self.check_target(expr.span, *def_id);
                }
            }
            HirExpressionKind::MethodCall(receiver, method, ..) => {
                let target = receiver
                    .ty
                    .and_then(|ty| struct_def_from_type(ty, self.env.types))
                    .and_then(|def| self.env.facts.method_index.get(&(def, *method)).copied());
                if let Some(target) = target {
                    self.check_target(expr.span, target);
                }
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}
