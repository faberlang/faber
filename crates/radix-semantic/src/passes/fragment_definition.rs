//! Definition-level device-safety check for `@ nucleum fragment` bodies.
//!
//! Each fragment is checked once against the V1 device-safe effect subset.
//! The check enforces device-safety of the body, not purity: explicit writes
//! to caller-provided device arguments are allowed and recorded.

use std::sync::LazyLock;

use crate::{Resolver, SemanticError, SemanticErrorKind, SymbolKind, Type, TypeId, TypeTable};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::{
    DefId, HirBlock, HirCallArg, HirCape, HirExpression, HirExpressionKind, HirFunction,
    HirItemKind, HirLocal, HirModule, HirObjectField, HirObjectKey, HirStatement, HirStatementKind,
    NucleumRole,
};
use radix_lexer::{Interner, Span, Symbol};

const ISSUE_EFFECT: &str = "nucleum_fragment_effect";
const ISSUE_CONTEXT: &str = "nucleum_fragment_context";

/// One visible device effect at a recorded source span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEffect {
    pub kind: DeviceEffectKind,
    pub span: Span,
}

/// Visible effect kinds recorded on a fragment definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceEffectKind {
    ArgumentRead,
    ArgumentWrite,
    FragmentCall,
    DeviceIntrinsic,
}

/// Device-effect summary for one fragment definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FragmentEffectSummary {
    pub def_id: DefId,
    pub effects: Vec<DeviceEffect>,
}

/// Per-definition device-effect summaries for every fragment in a module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FragmentEffectTable {
    by_def: FxHashMap<DefId, FragmentEffectSummary>,
}

impl FragmentEffectTable {
    #[must_use]
    pub fn get(&self, def_id: DefId) -> Option<&FragmentEffectSummary> {
        self.by_def.get(&def_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (DefId, &FragmentEffectSummary)> {
        self.by_def.iter().map(|(id, summary)| (*id, summary))
    }
}

/// Result of checking every fragment definition in a module.
#[derive(Debug, Clone)]
pub struct FragmentDefinitionReport {
    pub summaries: FragmentEffectTable,
    pub errors: Vec<SemanticError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CalleeKind {
    Fragment,
    Entry,
    Graphics,
    Ordinary,
}

/// Check every `@ nucleum fragment` definition against the device-safe subset.
#[must_use]
pub fn check(
    hir: &HirModule,
    resolver: &Resolver,
    types: &TypeTable,
    interner: &Interner,
) -> FragmentDefinitionReport {
    let mut checker = FragmentDefinitionChecker {
        resolver,
        types,
        interner,
        roles: collect_callee_kinds(hir),
        method_index: collect_struct_method_defs(hir),
        param_ids: FxHashSet::default(),
        local_ids: FxHashSet::default(),
        effects: Vec::new(),
        errors: Vec::new(),
        summaries: FragmentEffectTable::default(),
    };
    checker.check_program(hir);
    FragmentDefinitionReport {
        summaries: checker.summaries,
        errors: checker.errors,
    }
}

struct FragmentDefinitionChecker<'a> {
    resolver: &'a Resolver,
    types: &'a TypeTable,
    interner: &'a Interner,
    roles: FxHashMap<DefId, CalleeKind>,
    method_index: FxHashMap<(DefId, Symbol), DefId>,
    param_ids: FxHashSet<DefId>,
    local_ids: FxHashSet<DefId>,
    effects: Vec<DeviceEffect>,
    errors: Vec<SemanticError>,
    summaries: FragmentEffectTable,
}

impl FragmentDefinitionChecker<'_> {
    fn check_program(&mut self, hir: &HirModule) {
        for item in &hir.items {
            match &item.kind {
                HirItemKind::Function(function) => {
                    self.check_item(item.def_id, function, item.span);
                }
                HirItemKind::Struct(strukt) => {
                    for method in &strukt.methods {
                        self.check_item(method.def_id, &method.func, method.span);
                    }
                }
                _ => {}
            }
        }
    }

    fn check_item(&mut self, def_id: DefId, function: &HirFunction, span: Span) {
        if function.nucleum_role() != Some(NucleumRole::Fragment) {
            return;
        }
        self.param_ids = function.params.iter().map(|param| param.def_id).collect();
        self.local_ids.clear();
        self.effects.clear();
        self.check_fragment(function, span);
        self.summaries.by_def.insert(
            def_id,
            FragmentEffectSummary {
                def_id,
                effects: std::mem::take(&mut self.effects),
            },
        );
    }

    fn check_fragment(&mut self, function: &HirFunction, span: Span) {
        if function.err_ty.is_some() {
            self.effect(span, "alternate_exit");
        }
        if function.is_async || function.posture.is_async() {
            self.effect(span, "async_posture");
        }
        if function.is_generator || function.posture.is_stream() {
            self.effect(span, "generator_posture");
        }
        if function.cli_args.is_some() {
            self.effect(span, "host_runtime");
        }
        let Some(body) = &function.body else {
            self.effect(span, "missing_body");
            return;
        };
        self.check_block(body);
    }

    fn check_block(&mut self, block: &HirBlock) {
        for stmt in &block.statements {
            self.check_stmt(stmt);
        }
        if let Some(expr) = &block.expr {
            self.check_expr(expr);
        }
    }

    fn check_stmt(&mut self, stmt: &HirStatement) {
        match &stmt.kind {
            HirStatementKind::Local(local) => self.check_local(local),
            HirStatementKind::Redde(Some(expr))
            | HirStatementKind::ReddeFilum(Some(expr))
            | HirStatementKind::Expr(expr) => {
                self.check_expr(expr);
            }
            HirStatementKind::Redde(None)
            | HirStatementKind::ReddeFilum(None)
            | HirStatementKind::Rumpe
            | HirStatementKind::Perge
            | HirStatementKind::Tacet => {}
            HirStatementKind::IncDec(inc) => self.check_place(&inc.target, true),
            HirStatementKind::Custodi(custodi) => {
                for clause in &custodi.clauses {
                    self.check_expr(&clause.cond);
                    self.check_block(&clause.body);
                }
            }
        }
    }

    fn check_local(&mut self, local: &HirLocal) {
        self.local_ids.insert(local.def_id);
        if local.is_await {
            self.effect(local_span_or_init(local), "async_posture");
        }
        if let Some(init) = &local.init {
            self.check_expr(init);
        }
    }

    fn check_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.check_path_value(*def_id, expr.span),
            HirExpressionKind::Literal(_)
            | HirExpressionKind::Insere(_)
            | HirExpressionKind::Vacua
            | HirExpressionKind::Error => {}
            HirExpressionKind::Widen { operand, .. } | HirExpressionKind::Unary(_, operand) => {
                self.check_expr(operand);
            }
            HirExpressionKind::Binary(_, lhs, rhs) => {
                self.check_expr(lhs);
                self.check_expr(rhs);
            }
            HirExpressionKind::Call(callee, _, args, _) => self.check_call(callee, args, expr.span),
            // `target ⇇ value` writes existing tensor storage; fragments admit
            // no such effect yet, so classify it as one instead of admitting it
            // silently.
            HirExpressionKind::CopyInto { target, value } => {
                self.effect(expr.span, "dynamic_function");
                self.check_expr(target);
                self.check_expr(value);
            }
            // `sink ⇜ item` writes through the sink, an effect fragments admit
            // no more than `⇇`.
            HirExpressionKind::SinkInto { sink, item } => {
                self.effect(expr.span, "dynamic_function");
                self.check_expr(sink);
                self.check_expr(item);
            }
            HirExpressionKind::Gradient {
                call,
                selected_arguments,
            } => {
                self.effect(expr.span, "unapproved_effect");
                self.check_expr(call);
                for selector in selected_arguments {
                    self.check_expr(selector);
                }
            }
            HirExpressionKind::MethodCall(receiver, method, _, args, _) => {
                self.check_method_call(receiver, *method, args, expr.span);
            }
            HirExpressionKind::Field(object, _) => self.check_expr(object),
            HirExpressionKind::Index(object, index) => {
                self.check_expr(object);
                self.check_expr(index);
            }
            HirExpressionKind::OptionalChain(object, _) | HirExpressionKind::NonNull(object, _) => {
                self.effect(expr.span, "unapproved_effect");
                self.check_expr(object);
            }
            HirExpressionKind::Marked { expr: inner, .. } | HirExpressionKind::Praefixum(inner) => {
                self.check_expr(inner);
            }
            HirExpressionKind::Block(block) | HirExpressionKind::Loop(block) => {
                self.check_block(block);
            }
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => {
                self.check_expr(cond);
                if let Some(catch) = then_catch {
                    self.check_cape(catch);
                }
                self.check_block(then_block);
                if let Some(else_block) = else_block {
                    self.check_block(else_block);
                }
            }
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => {
                for scrutinee in scrutinees {
                    self.check_expr(scrutinee);
                }
                for arm in arms {
                    self.check_expr(&arm.body);
                }
            }
            HirExpressionKind::Dum(cond, block) => {
                self.check_expr(cond);
                self.check_block(block);
            }
            HirExpressionKind::Itera {
                iter: source,
                body: block,
                ..
            } => {
                self.check_expr(source);
                self.check_block(block);
            }
            HirExpressionKind::IteraLockstep { sources, body, .. } => {
                for source in sources {
                    self.check_expr(source);
                }
                self.check_block(body);
            }
            HirExpressionKind::IteraRange { ranges, body, .. } => {
                for range in ranges {
                    self.check_expr(range);
                }
                self.check_block(body);
            }
            HirExpressionKind::Reduction {
                source,
                group,
                body,
                identity,
                ..
            } => {
                self.effect(expr.span, "unapproved_effect");
                self.check_expr(source);
                if let Some(group) = group {
                    self.check_expr(group);
                }
                if let Some(body) = body {
                    self.check_block(body);
                }
                if let Some(identity) = identity {
                    self.check_expr(identity);
                }
            }
            HirExpressionKind::Scan {
                source,
                group,
                body,
                ..
            } => {
                self.effect(expr.span, "unapproved_effect");
                self.check_expr(source);
                if let Some(group) = group {
                    self.check_expr(group);
                }
                self.check_block(body);
            }
            HirExpressionKind::Sort { source, body, .. } => {
                self.effect(expr.span, "unapproved_effect");
                self.check_expr(source);
                if let Some(body) = body {
                    self.check_block(body);
                }
            }
            HirExpressionKind::Intervallum {
                start, end, step, ..
            } => {
                self.check_expr(start);
                self.check_expr(end);
                if let Some(step) = step {
                    self.check_expr(step);
                }
            }
            HirExpressionKind::Assign(lhs, rhs) => {
                self.check_place(lhs, true);
                self.check_expr(rhs);
            }
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
                ..
            } => {
                if recovery.is_some() {
                    self.effect(expr.span, "alternate_exit");
                }
                self.check_place(target, true);
                self.check_expr(source);
                if let Some(recovery) = recovery {
                    self.check_expr(recovery);
                }
            }
            HirExpressionKind::Array(elements) => {
                self.effect(expr.span, "host_allocation");
                for element in elements {
                    match element {
                        radix_hir::HirArrayElement::Expr(inner)
                        | radix_hir::HirArrayElement::Spread(inner) => {
                            self.check_expr(inner);
                        }
                        radix_hir::HirArrayElement::Infer(_) => {}
                    }
                }
            }
            HirExpressionKind::Struct(_, fields) => {
                for (_, value) in fields {
                    self.check_expr(value);
                }
            }
            HirExpressionKind::Tuple(elements, _) => {
                for element in elements {
                    self.check_expr(element);
                }
            }
            HirExpressionKind::Scriptum(_, elements) | HirExpressionKind::Scribe(_, elements) => {
                self.effect(expr.span, "host_io");
                for element in elements {
                    self.check_expr(element);
                }
            }
            HirExpressionKind::ReadLine(_) => self.effect(expr.span, "host_io"),
            HirExpressionKind::Adfirma(cond, message) => {
                self.effect(expr.span, "unapproved_effect");
                self.check_expr(cond);
                if let Some(message) = message {
                    self.check_expr(message);
                }
            }
            HirExpressionKind::Panic(message) => {
                self.effect(expr.span, "unapproved_effect");
                self.check_expr(message);
            }
            HirExpressionKind::Throw(value) => {
                self.effect(expr.span, "alternate_exit");
                self.check_expr(value);
            }
            HirExpressionKind::Handled { body, catch } => {
                self.effect(expr.span, "alternate_exit");
                self.check_block(body);
                self.check_cape(catch);
            }
            // UT: the capture boundary locally resolves the body's alternate
            // exits — the effect is the same class as a local handler.
            HirExpressionKind::Trap(body) => {
                self.effect(expr.span, "alternate_exit");
                self.check_block(body);
            }
            HirExpressionKind::CallDefault { body, default, .. } => {
                self.effect(expr.span, "alternate_exit");
                self.check_block(body);
                self.check_expr(default);
            }
            HirExpressionKind::Clausura(params, _, _, body) => {
                self.effect(expr.span, "dynamic_function");
                for param in params {
                    if let Some(default) = &param.default {
                        self.check_expr(default);
                    }
                }
                self.check_expr(body);
            }
            HirExpressionKind::Cede(value) | HirExpressionKind::Yield(value) => {
                self.effect(expr.span, "generator_posture");
                self.check_expr(value);
            }
            HirExpressionKind::Reddet(value) | HirExpressionKind::Tacebit(value) => {
                self.effect(expr.span, "async_posture");
                self.check_expr(value);
            }
            HirExpressionKind::Verte {
                source, entries, ..
            } => {
                self.check_expr(source);
                if let Some(entries) = entries {
                    for field in entries {
                        self.check_object_field(field);
                    }
                }
            }
            HirExpressionKind::Conversio {
                source,
                target,
                recovery,
                ..
            } => {
                if recovery.is_some() {
                    self.effect(expr.span, "alternate_exit");
                }
                self.check_expr(source);
                if let radix_hir::HirConversioTarget::Intervallum(operand) = target {
                    self.check_expr(operand);
                }
                if let Some(recovery) = recovery {
                    self.check_expr(recovery);
                }
            }
            HirExpressionKind::Ad { opener, .. } => {
                self.effect(expr.span, "host_runtime");
                if let Some(opener) = opener {
                    self.check_expr(opener);
                }
            }
            HirExpressionKind::Ref(_, operand) | HirExpressionKind::Deref(operand) => {
                self.effect(expr.span, "address_taking");
                self.check_expr(operand);
            }
            HirExpressionKind::TypeCheck { expr: inner, .. } => {
                self.effect(expr.span, "unapproved_effect");
                self.check_expr(inner);
            }
            // `¶` builds text, like `scriptum`: not a device-lane value.
            HirExpressionKind::Format { value, .. } => {
                self.effect(expr.span, "host_io");
                self.check_expr(value);
            }
        }
    }

    fn check_call(&mut self, callee: &HirExpression, args: &[HirCallArg], span: Span) {
        match &callee.kind {
            HirExpressionKind::Path(def_id) => self.classify_path_callee(*def_id, span),
            other => {
                if !matches!(other, HirExpressionKind::Error) {
                    self.effect(callee.span, "dynamic_function");
                }
                self.check_expr(callee);
            }
        }
        self.check_call_args(args);
    }

    fn check_method_call(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        args: &[HirCallArg],
        span: Span,
    ) {
        if let Some(def_id) =
            resolve_method_call_def(&self.method_index, receiver, method, self.types)
        {
            self.classify_path_callee(def_id, span);
        } else if is_allowed_device_method(self.interner.resolve(method))
            && self.is_device_receiver(receiver)
        {
            self.record(DeviceEffectKind::DeviceIntrinsic, span);
        } else {
            self.context(span, "host_callable");
        }
        self.check_expr(receiver);
        self.check_call_args(args);
    }

    fn classify_path_callee(&mut self, def_id: DefId, span: Span) {
        if self.resolver.gpu_builtin(def_id).is_some() {
            self.record(DeviceEffectKind::DeviceIntrinsic, span);
            return;
        }
        match self
            .roles
            .get(&def_id)
            .copied()
            .unwrap_or(CalleeKind::Ordinary)
        {
            CalleeKind::Fragment => self.record(DeviceEffectKind::FragmentCall, span),
            CalleeKind::Entry => self.context(span, "entry_callee"),
            CalleeKind::Graphics => self.context(span, "graphics_stage"),
            CalleeKind::Ordinary => self.context(span, "host_callable"),
        }
    }

    fn check_call_args(&mut self, args: &[HirCallArg]) {
        for arg in args {
            self.check_expr(&arg.expr);
        }
    }

    fn check_object_field(&mut self, field: &HirObjectField) {
        match &field.key {
            HirObjectKey::Ident(_) | HirObjectKey::String(_) => {}
            HirObjectKey::Computed(expr) | HirObjectKey::Spread(expr) => self.check_expr(expr),
        }
        if let Some(value) = &field.value {
            self.check_expr(value);
        }
    }

    fn check_cape(&mut self, cape: &HirCape) {
        self.effect(cape.span, "alternate_exit");
        self.check_block(&cape.body);
    }

    fn check_place(&mut self, expr: &HirExpression, writing: bool) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.check_path_place(*def_id, expr.span, writing),
            HirExpressionKind::Field(object, _) => self.check_place(object, writing),
            HirExpressionKind::Index(object, index) => {
                self.check_place(object, writing);
                self.check_expr(index);
            }
            _ => self.check_expr(expr),
        }
    }

    fn check_path_place(&mut self, def_id: DefId, span: Span, writing: bool) {
        if self.param_ids.contains(&def_id) {
            self.record(
                if writing {
                    DeviceEffectKind::ArgumentWrite
                } else {
                    DeviceEffectKind::ArgumentRead
                },
                span,
            );
            return;
        }
        if self.local_ids.contains(&def_id) {
            return;
        }
        if writing {
            self.effect(span, "hidden_storage");
            return;
        }
        if self
            .resolver
            .get_symbol(def_id)
            .is_some_and(|symbol| symbol.mutable)
        {
            self.effect(span, "captured_mut_host");
        } else {
            self.check_path_value(def_id, span);
        }
    }

    fn check_path_value(&mut self, def_id: DefId, span: Span) {
        if self.param_ids.contains(&def_id) {
            self.record(DeviceEffectKind::ArgumentRead, span);
            return;
        }
        if self.local_ids.contains(&def_id) {
            return;
        }
        if self.resolver.gpu_builtin(def_id).is_some() {
            self.effect(span, "dynamic_function");
            return;
        }
        if let Some(symbol) = self.resolver.get_symbol(def_id) {
            if matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method) {
                self.effect(span, "dynamic_function");
                return;
            }
            if symbol.mutable {
                self.effect(span, "captured_mut_host");
                return;
            }
            if let Some(ty) = symbol.ty
                && is_func_type(ty, self.types)
            {
                self.effect(span, "dynamic_function");
            }
            return;
        }
    }

    fn is_device_receiver(&self, receiver: &HirExpression) -> bool {
        receiver
            .ty
            .is_some_and(|ty| is_device_value_type(ty, self.types))
    }

    fn effect(&mut self, span: Span, reason: &'static str) {
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::OperandTypesInvalid, span)
                .with_arg("issue", ISSUE_EFFECT)
                .with_arg("reason", reason),
        );
    }

    fn context(&mut self, span: Span, reason: &'static str) {
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::OperandTypesInvalid, span)
                .with_arg("issue", ISSUE_CONTEXT)
                .with_arg("reason", reason)
                .with_arg("site", "definition"),
        );
    }

    fn record(&mut self, kind: DeviceEffectKind, span: Span) {
        self.effects.push(DeviceEffect { kind, span });
    }
}

fn local_span_or_init(local: &HirLocal) -> Span {
    local
        .init
        .as_ref()
        .map_or(Span::default(), |init| init.span)
}

fn collect_callee_kinds(hir: &HirModule) -> FxHashMap<DefId, CalleeKind> {
    let mut roles = FxHashMap::default();
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(function) => {
                roles.insert(item.def_id, callee_kind(function));
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    roles.insert(method.def_id, callee_kind(&method.func));
                }
            }
            _ => {}
        }
    }
    roles
}

fn callee_kind(function: &HirFunction) -> CalleeKind {
    match function.nucleum_role() {
        Some(NucleumRole::Fragment) => CalleeKind::Fragment,
        Some(NucleumRole::Entry) => CalleeKind::Entry,
        None if function.is_vertex || function.is_fragment => CalleeKind::Graphics,
        None => CalleeKind::Ordinary,
    }
}

fn collect_struct_method_defs(hir: &HirModule) -> FxHashMap<(DefId, Symbol), DefId> {
    let mut index = FxHashMap::default();
    for item in &hir.items {
        let HirItemKind::Struct(strukt) = &item.kind else {
            continue;
        };
        for method in &strukt.methods {
            index.insert((item.def_id, method.func.name), method.def_id);
        }
    }
    index
}

pub(crate) fn resolve_method_call_def(
    method_index: &FxHashMap<(DefId, Symbol), DefId>,
    receiver: &HirExpression,
    method: Symbol,
    types: &TypeTable,
) -> Option<DefId> {
    let receiver_ty = receiver.ty?;
    let struct_def = struct_def_from_type(receiver_ty, types)?;
    method_index.get(&(struct_def, method)).copied()
}

pub(crate) fn struct_def_from_type(ty: TypeId, types: &TypeTable) -> Option<DefId> {
    match types.get(ty) {
        Type::Struct(def) => Some(*def),
        Type::Ref(_, inner) | Type::Applied(inner, _) | Type::Alias(_, inner) => {
            struct_def_from_type(*inner, types)
        }
        _ => None,
    }
}

fn is_func_type(ty: TypeId, types: &TypeTable) -> bool {
    match types.get(ty) {
        Type::Func(_) => true,
        Type::Alias(_, inner)
        | Type::Ref(_, inner)
        | Type::Applied(inner, _)
        | Type::Option(inner) => is_func_type(*inner, types),
        _ => false,
    }
}

pub(crate) fn is_device_value_type(ty: TypeId, types: &TypeTable) -> bool {
    match types.get(ty) {
        Type::Tensor(_, _) | Type::Vector(_, _) | Type::Matrix(_, _) | Type::Sparsa(_, _) => true,
        Type::Alias(_, inner) | Type::Ref(_, inner) | Type::Applied(inner, _) => {
            is_device_value_type(*inner, types)
        }
        _ => false,
    }
}

/// Tensor methods the device-fragment subset withholds from the AIR-lane
/// vocabulary: the reduction/arg-reduction family and the higher-order combine
/// family. This is the only hand-typed list here; the device set is derived
/// from it.
pub(crate) const DEVICE_EXCLUDED: &[&str] = &[
    "maxima",
    "minima",
    "argmaxima",
    "argminima",
    "reducta",
    "cumulata",
    "mappata",
    "filtrata",
];

/// Derived device-fragment admission set: every AIR-lane tensor intrinsic
/// except [`DEVICE_EXCLUDED`]. Computed once from the shared tensor row set, so
/// there is no second literal list to drift.
static ALLOWED_DEVICE_METHODS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    super::air_tensor_intrinsics::allowed_tensor_intrinsics()
        .iter()
        .copied()
        .filter(|method| !DEVICE_EXCLUDED.contains(method))
        .collect()
});

/// The device-fragment method admission set. Test-support surface: the
/// radix-module bijection ratchet derives its direction-3 candidates from it.
#[doc(hidden)]
#[must_use]
pub fn allowed_device_methods() -> &'static [&'static str] {
    ALLOWED_DEVICE_METHODS.as_slice()
}

pub(crate) fn is_allowed_device_method(method: &str) -> bool {
    ALLOWED_DEVICE_METHODS.contains(&method)
}
