//! Generic forwarding and concrete discharge for tensor edge-policy selectors.
//!
//! `limes<P>` leaves an obligation on a generic function. Once every local
//! body has been checked, this pass follows ordinary generic call bindings,
//! forwards requirements through callers, and validates concrete witnesses
//! before codegen receives the provider catalog.

use super::{
    DefId, FxHashMap, FxHashSet, GenericWitness, HirExpression, HirExpressionKind, HirFunction,
    HirItem, HirItemKind, HirModule, ResolvedGenericBinding, Symbol, Type, TypeChecker, TypeId,
    TypeTable,
};
use radix_hir::visit::{HirVisitor, walk_expr, walk_function, walk_item, walk_method};
use radix_hir::{TensorEdgePolicyProviderImpl, TensorEdgePolicyRequirement};

const ISSUE_TENSOR_EDGE_POLICY_MARKER_NOT_RUNTIME: &str =
    "tensor_edge_policy_marker_not_runtime_value";
const ISSUE_TENSOR_LIMES_POLICY_INVALID: &str = "tensor_limes_policy_invalid";

/// Resolved generic call facts retained until all local policy requirements
/// have been collected. The authoritative witness representation remains the
/// existing `ResolvedGenericBinding`/`GenericWitness` pair.
#[derive(Debug, Clone)]
pub(super) struct GenericPolicyCall {
    pub caller: Option<DefId>,
    pub callee: Option<DefId>,
    pub span: radix_lexer::Span,
    pub bindings: Vec<ResolvedGenericBinding>,
}

#[derive(Clone, Copy)]
struct DirectPolicyFact {
    owner: Option<DefId>,
    span: radix_lexer::Span,
    policy_type: TypeId,
    element_type: TypeId,
    rank: usize,
    policy: crate::ResolvedTensorEdgePolicy,
}

struct PolicyUsageCollector<'a> {
    policies: &'a crate::TensorEdgePolicyTable,
    types: &'a TypeTable,
    owner: Option<DefId>,
    enclosing_type_params: Vec<Symbol>,
    function_type_params: FxHashMap<DefId, FxHashSet<Symbol>>,
    direct_facts: Vec<DirectPolicyFact>,
}

impl PolicyUsageCollector<'_> {
    fn record_function_type_params(&mut self, function: &HirFunction) {
        let Some(owner) = self.owner else {
            return;
        };
        let mut params = self
            .enclosing_type_params
            .iter()
            .copied()
            .collect::<FxHashSet<_>>();
        params.extend(
            function
                .type_params
                .iter()
                .filter(|param| param.kind == radix_syntax::GenericParamKind::Typus)
                .map(|param| param.name),
        );
        self.function_type_params.insert(owner, params);
    }

    fn record_limes_call(&mut self, expression: &HirExpression) {
        let Some(policy) = self.policies.calls.get(&expression.id).copied() else {
            return;
        };
        let (HirExpressionKind::Call(_, type_args, ..)
        | HirExpressionKind::MethodCall(_, _, type_args, ..)) = &expression.kind
        else {
            return;
        };
        let Some(&written_policy_type) = type_args.first() else {
            return;
        };
        let Some(result_type) = expression.ty else {
            return;
        };
        let mut result_type = result_type;
        while let Type::Alias(_, inner) = self.types.get(result_type) {
            result_type = *inner;
        }
        let Type::Tensor(element_type, shape) = self.types.get(result_type) else {
            return;
        };
        let Some(rank) = super::intrinsics::tensor_shape_rank_from_index(self.types, *shape) else {
            return;
        };
        self.direct_facts.push(DirectPolicyFact {
            owner: self.owner,
            span: expression.span,
            policy_type: match policy {
                crate::ResolvedTensorEdgePolicy::Deferred { policy_type } => policy_type,
                _ => written_policy_type,
            },
            element_type: *element_type,
            rank,
            policy,
        });
    }
}

impl HirVisitor for PolicyUsageCollector<'_> {
    fn visit_item(&mut self, item: &HirItem) {
        match &item.kind {
            HirItemKind::Function(_) => {
                let previous_owner = self.owner.replace(item.def_id);
                walk_item(self, item);
                self.owner = previous_owner;
            }
            HirItemKind::Struct(struct_item) => {
                let previous_type_params = std::mem::replace(
                    &mut self.enclosing_type_params,
                    struct_item
                        .type_params
                        .iter()
                        .filter(|param| param.kind == radix_syntax::GenericParamKind::Typus)
                        .map(|param| param.name)
                        .collect(),
                );
                walk_item(self, item);
                self.enclosing_type_params = previous_type_params;
            }
            _ => walk_item(self, item),
        }
    }

    fn visit_method(&mut self, method: &radix_hir::HirMethod) {
        let previous_owner = self.owner.replace(method.def_id);
        walk_method(self, method);
        self.owner = previous_owner;
    }

    fn visit_function(&mut self, function: &HirFunction) {
        self.record_function_type_params(function);
        walk_function(self, function);
    }

    fn visit_expr(&mut self, expression: &HirExpression) {
        self.record_limes_call(expression);
        walk_expr(self, expression);
    }
}

impl TypeChecker<'_> {
    /// Record active type parameters that flow into runtime value types.
    ///
    /// A builtin edge-policy marker may bind only a parameter whose uses are
    /// compile-time policy selection and generic forwarding. The recursive
    /// walk makes `Holder<P>` and nested callable/value shapes count as runtime
    /// uses just like a direct `P` slot.
    pub(super) fn record_tensor_edge_policy_runtime_type_use(&mut self, ty: TypeId) {
        let Some(owner) = self.current_function_def else {
            return;
        };
        let params = self
            .current_generic_param_kinds
            .iter()
            .filter(|(_, kind)| **kind == radix_syntax::GenericParamKind::Typus)
            .map(|(symbol, _)| *symbol)
            .collect::<Vec<_>>();
        let used = params
            .into_iter()
            .filter(|symbol| {
                type_contains_param(self.types, ty, *symbol, &mut FxHashSet::default())
            })
            .map(|symbol| (owner, symbol))
            .collect::<Vec<_>>();
        self.tensor_edge_policy_runtime_params.extend(used);
    }

    /// Whether a formal type parameter is used only by a resolved `limes`
    /// obligation and generic forwarding. Callers use this after body
    /// checking, when requirements and runtime-position uses are both known.
    pub fn tensor_edge_policy_parameter_is_required(
        &self,
        function: DefId,
        symbol: Symbol,
    ) -> bool {
        !self
            .tensor_edge_policy_runtime_params
            .contains(&(function, symbol))
            && self
                .tensor_edge_policies
                .function_requirements
                .get(&function)
                .or_else(|| self.tensor_edge_policy_imported_requirements.get(&function))
                .is_some_and(|requirements| {
                    requirements.iter().any(|requirement| {
                        type_param_symbol(self.types, requirement.policy_type) == Some(symbol)
                    })
                })
    }

    /// Complete local generic policy summaries and concrete provider facts.
    pub(super) fn discharge_tensor_edge_policy_obligations(&mut self, hir: &HirModule) {
        let (direct_facts, function_type_params) = {
            let mut collector = PolicyUsageCollector {
                policies: &self.tensor_edge_policies,
                types: self.types,
                owner: None,
                enclosing_type_params: Vec::new(),
                function_type_params: FxHashMap::default(),
                direct_facts: Vec::new(),
            };
            collector.visit_program(hir);
            (collector.direct_facts, collector.function_type_params)
        };

        for fact in direct_facts {
            match fact.policy {
                crate::ResolvedTensorEdgePolicy::Deferred { .. } => {
                    if let Some(owner) = fact.owner {
                        let policy_param = type_param_symbol(self.types, fact.policy_type);
                        if policy_param.is_some_and(|symbol| {
                            self.tensor_edge_policy_runtime_params
                                .contains(&(owner, symbol))
                        }) {
                            self.invalid_operand_types(
                                ISSUE_TENSOR_LIMES_POLICY_INVALID,
                                fact.span,
                            );
                        } else {
                            push_requirement(
                                &mut self.tensor_edge_policies,
                                owner,
                                TensorEdgePolicyRequirement {
                                    policy_type: fact.policy_type,
                                    element_type: fact.element_type,
                                    rank: fact.rank,
                                },
                            );
                        }
                    }
                }
                policy => push_provider(
                    &mut self.tensor_edge_policies,
                    TensorEdgePolicyProviderImpl {
                        policy_type: fact.policy_type,
                        element_type: fact.element_type,
                        rank: fact.rank,
                        policy,
                    },
                ),
            }
        }

        self.propagate_tensor_edge_policy_requirements(&function_type_params);
        self.discharge_concrete_tensor_edge_policy_calls(&function_type_params);
    }

    fn propagate_tensor_edge_policy_requirements(
        &mut self,
        function_type_params: &FxHashMap<DefId, FxHashSet<Symbol>>,
    ) {
        let mut diagnosed_runtime_forwarders = FxHashSet::default();
        let calls = self.tensor_edge_generic_calls.clone();
        loop {
            let mut requirements = self.tensor_edge_policies.function_requirements.clone();
            requirements.extend(self.tensor_edge_policy_imported_requirements.clone());
            let mut forwarded = Vec::new();
            let mut runtime_forward_spans = Vec::new();
            for call in &calls {
                let (Some(caller), Some(callee_def)) = (call.caller, call.callee) else {
                    continue;
                };
                let Some(caller_params) = function_type_params.get(&caller) else {
                    continue;
                };
                let Some(callee_requirements) = requirements.get(&callee_def) else {
                    continue;
                };
                for requirement in callee_requirements {
                    let Some(policy_param) = type_param_symbol(self.types, requirement.policy_type)
                    else {
                        continue;
                    };
                    let Some(GenericWitness::Typus(actual_policy)) = call
                        .bindings
                        .iter()
                        .find(|binding| binding.name == policy_param)
                        .map(|binding| binding.witness)
                    else {
                        continue;
                    };
                    let actual_policy = self.resolve_inferred_type(actual_policy);
                    let Some(caller_policy_param) = type_param_symbol(self.types, actual_policy)
                    else {
                        continue;
                    };
                    if !caller_params.contains(&caller_policy_param) {
                        continue;
                    }
                    if self
                        .tensor_edge_policy_runtime_params
                        .contains(&(caller, caller_policy_param))
                        && diagnosed_runtime_forwarders.insert((caller, caller_policy_param))
                    {
                        runtime_forward_spans.push(call.span);
                    }
                    let mut substitutions = type_substitutions(&call.bindings);
                    let element_type = self.substitute_type_params(
                        requirement.element_type,
                        &mut substitutions,
                        None,
                    );
                    forwarded.push((
                        caller,
                        TensorEdgePolicyRequirement {
                            policy_type: actual_policy,
                            element_type,
                            rank: requirement.rank,
                        },
                    ));
                }
            }

            for span in runtime_forward_spans {
                self.invalid_operand_types(ISSUE_TENSOR_LIMES_POLICY_INVALID, span);
            }
            let mut changed = false;
            for (function, requirement) in forwarded {
                changed |= push_requirement(&mut self.tensor_edge_policies, function, requirement);
            }
            if !changed {
                break;
            }
        }
    }

    fn discharge_concrete_tensor_edge_policy_calls(
        &mut self,
        function_type_params: &FxHashMap<DefId, FxHashSet<Symbol>>,
    ) {
        let mut requirements = self.tensor_edge_policies.function_requirements.clone();
        requirements.extend(self.tensor_edge_policy_imported_requirements.clone());
        let mut invalid_marker_bindings = FxHashSet::default();
        let calls = self.tensor_edge_generic_calls.clone();

        for (call_index, call) in calls.iter().enumerate() {
            for binding in &call.bindings {
                let GenericWitness::Typus(actual) = binding.witness else {
                    continue;
                };
                let actual = self.resolve_inferred_type(actual);
                if !self.types.contains_tensor_edge_policy(actual) {
                    continue;
                }
                let Some(callee) = call.callee else {
                    self.invalid_operand_types(
                        ISSUE_TENSOR_EDGE_POLICY_MARKER_NOT_RUNTIME,
                        call.span,
                    );
                    invalid_marker_bindings.insert((call_index, binding.name));
                    continue;
                };
                if !self.tensor_edge_policy_parameter_is_required(callee, binding.name)
                    || self.types.tensor_edge_policy_kind(actual).is_none()
                {
                    self.invalid_operand_types(
                        ISSUE_TENSOR_EDGE_POLICY_MARKER_NOT_RUNTIME,
                        call.span,
                    );
                    invalid_marker_bindings.insert((call_index, binding.name));
                }
            }
        }

        for (call_index, call) in calls.iter().enumerate() {
            let Some(callee) = call.callee else {
                continue;
            };
            let Some(callee_requirements) = requirements.get(&callee) else {
                continue;
            };
            for requirement in callee_requirements {
                let Some(policy_param) = type_param_symbol(self.types, requirement.policy_type)
                else {
                    continue;
                };
                let Some(GenericWitness::Typus(actual_policy)) = call
                    .bindings
                    .iter()
                    .find(|binding| binding.name == policy_param)
                    .map(|binding| binding.witness)
                else {
                    continue;
                };
                if invalid_marker_bindings.contains(&(call_index, policy_param)) {
                    continue;
                }
                let actual_policy = self.resolve_inferred_type(actual_policy);
                if let Some(symbol) = type_param_symbol(self.types, actual_policy)
                    && call
                        .caller
                        .and_then(|caller| function_type_params.get(&caller))
                        .is_some_and(|params| params.contains(&symbol))
                {
                    continue;
                }

                let mut substitutions = type_substitutions(&call.bindings);
                let element_type =
                    self.substitute_type_params(requirement.element_type, &mut substitutions, None);
                let dispatch =
                    if let Some(policy) = self.types.tensor_edge_policy_kind(actual_policy) {
                        Some(crate::ResolvedTensorEdgePolicy::Builtin(policy))
                    } else {
                        super::intrinsics::check_custom_tensor_edge_policy(
                            self,
                            actual_policy,
                            element_type,
                            requirement.rank,
                        )
                    };
                let Some(policy) = dispatch else {
                    self.invalid_operand_types(ISSUE_TENSOR_LIMES_POLICY_INVALID, call.span);
                    continue;
                };
                push_provider(
                    &mut self.tensor_edge_policies,
                    TensorEdgePolicyProviderImpl {
                        policy_type: actual_policy,
                        element_type,
                        rank: requirement.rank,
                        policy,
                    },
                );
            }
        }
    }
}

fn push_requirement(
    policies: &mut crate::TensorEdgePolicyTable,
    function: DefId,
    requirement: TensorEdgePolicyRequirement,
) -> bool {
    let rows = policies.function_requirements.entry(function).or_default();
    if rows.contains(&requirement) {
        false
    } else {
        rows.push(requirement);
        true
    }
}

fn push_provider(
    policies: &mut crate::TensorEdgePolicyTable,
    provider: TensorEdgePolicyProviderImpl,
) {
    if !policies.provider_impls.contains(&provider) {
        policies.provider_impls.push(provider);
    }
}

fn type_substitutions(bindings: &[ResolvedGenericBinding]) -> FxHashMap<Symbol, TypeId> {
    bindings
        .iter()
        .filter_map(|binding| match binding.witness {
            GenericWitness::Typus(ty) => Some((binding.name, ty)),
            GenericWitness::Magnitudo(_) => None,
        })
        .collect()
}

fn type_param_symbol(types: &TypeTable, mut ty: TypeId) -> Option<Symbol> {
    let mut seen = FxHashSet::default();
    while let Type::Alias(_, inner) = types.get(ty) {
        if !seen.insert(ty) {
            return None;
        }
        ty = *inner;
    }
    match types.get(ty) {
        Type::Param(symbol) => Some(*symbol),
        _ => None,
    }
}

fn type_contains_param(
    types: &TypeTable,
    ty: TypeId,
    symbol: Symbol,
    seen: &mut FxHashSet<TypeId>,
) -> bool {
    if !seen.insert(ty) {
        return false;
    }
    let contains =
        |child, seen: &mut FxHashSet<TypeId>| type_contains_param(types, child, symbol, seen);
    match types.get(ty) {
        Type::Param(found) => *found == symbol,
        Type::Array(inner)
        | Type::Queue(inner)
        | Type::Stack(inner)
        | Type::Set(inner)
        | Type::Promissum(inner)
        | Type::Cursor(inner)
        | Type::Atomic(inner)
        | Type::Intervallum(inner, _)
        | Type::Option(inner)
        | Type::Ref(_, inner)
        | Type::Alias(_, inner) => contains(*inner, seen),
        Type::BoundedArray { element, .. }
        | Type::RangedArray { element, .. }
        | Type::BoundedQueue { element, .. }
        | Type::BoundedStack { element, .. }
        | Type::Tensor(element, _)
        | Type::Vector(element, _)
        | Type::Matrix(element, _)
        | Type::Sparsa(element, _) => contains(*element, seen),
        Type::Map(key, value)
        | Type::Ledger(key, value)
        | Type::PromissumFailable(key, value)
        | Type::AsyncCursor(key, value) => contains(*key, seen) || contains(*value, seen),
        Type::Record(fields) => fields.values().any(|field| contains(*field, seen)),
        Type::Func(signature) => {
            signature
                .params
                .iter()
                .any(|param| contains(param.ty, seen))
                || contains(signature.ret, seen)
                || signature.err.is_some_and(|error| contains(error, seen))
        }
        Type::Applied(base, args) => {
            contains(*base, seen)
                || args.iter().any(|arg| match arg {
                    radix_types::AppliedArg::Type(inner) => contains(*inner, seen),
                    radix_types::AppliedArg::Size(_) => false,
                })
        }
        Type::Union(members) | Type::Intersection(members) | Type::Tuple(members) => {
            members.iter().any(|member| contains(*member, seen))
        }
        Type::Primitive(_)
        | Type::TensorEdgePolicy(_)
        | Type::BoundedTextus { .. }
        | Type::BoundedAscii { .. }
        | Type::BoundedOcteti { .. }
        | Type::SizedNumeric(_, _)
        | Type::Bits(_)
        | Type::ModularWord(_)
        | Type::SaturatingWord(_)
        | Type::RangedInt { .. }
        | Type::PolicyFloat(_, _)
        | Type::SizedInstans(_)
        | Type::Struct(_)
        | Type::Enum(_)
        | Type::Interface(_)
        | Type::Infer(_)
        | Type::InferUnion(_)
        | Type::MarkerHole { .. }
        | Type::Error => false,
    }
}
