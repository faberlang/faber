//! Per-target kernel requirements and integer range proofs (F9 rulings 18, 27).
//!
//! This pass records portable-device admission facts beside the typed HIR. It
//! does not change Faber arithmetic or lower runtime checks. Exact integer
//! intermediates must fit the host's 64-bit integer intermediate range
//! (ruling 17); a `d64` intermediate uses the 128-bit carrier and traps beyond
//! it at run time (ruling 19(e)). The operator ruling of 2026-10-28 admits
//! `i64`, `u64`, `wrapping<u64>`, `saturating<u64>` and `d64` as kernel values,
//! so these widths are per-target capability facts in the report, never check
//! errors. An implicit trapping store that is not proven safe remains an
//! explicit option-F requirement.

use crate::{
    NumericWidth, Primitive, ResolvedTensorEdgePolicy, Resolver, SemanticError, SemanticErrorKind,
    SymbolKind, TensorEdgePolicyTable, Type, TypeId, TypeTable,
};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{HirVisitor, walk_expr};
use radix_hir::{
    DefId, HirAnnotation, HirArrayElement, HirBinOp, HirBlock, HirExpression, HirExpressionKind,
    HirFunction, HirId, HirIncDecOp, HirItemKind, HirLocal, HirStatement, HirStatementKind,
    HirUnOp, KernelTier,
};
use radix_lexer::{Interner, Span, Symbol};
use radix_target::Target;
use radix_types::{IndexExpr, IndexId, evaluate_index_dimension};
use serde::{Deserialize, Serialize};

/// The current device backends for which rule 18 defines portable admission.
/// Owned by [`Target`]; this alias keeps the module's report indexing surface.
pub const DEVICE_TARGETS: [Target; 3] = Target::DEVICE_TARGETS;

/// Full analysis result for the kernels declared in one source module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KernelRequirementsReport {
    pub kernels: Vec<KernelRequirement>,
    /// One row per `fac machina` (en `do device`) region of the module's host
    /// code. A local fact: the module artifact and the FHIR wire read
    /// `kernels` only.
    pub regions: Vec<DeviceRegionRequirement>,
}

/// A `fac machina` region's requirement rows and the facts `faber check
/// --device` prints for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRegionRequirement {
    /// The block's source span (the report names it `do device`).
    pub span: Span,
    pub target_reports: Vec<KernelTargetRequirement>,
    pub facts: super::device_region::RegionFacts,
    /// Host syncs the block performs. Zero for every region that checks: a
    /// device read at the top of the block is refused by name.
    pub host_syncs: u32,
}

/// One kernel's inferred requirements and admission verdict for every device
/// target. A kernel can be legal Faber while a particular device rejects it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelRequirement {
    pub def_id: DefId,
    pub name: String,
    /// The tier the author declared (`@ kernel relaxed` / `unsafe`). Under
    /// the safe switch analysis already read it as the default tier, so this
    /// is the tier the emitters will see.
    pub tier: KernelTier,
    pub target_reports: Vec<KernelTargetRequirement>,
}

/// Target-local facts used by reports and product admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelTargetRequirement {
    pub target: Target,
    pub admitted: bool,
    pub recursive: bool,
    /// Direct and transitive kernel callees. Their individual rows retain
    /// argument-dependent extent conditions without flattening foreign axes.
    pub callees: Vec<DefId>,
    pub widths: Vec<KernelWidthRequirement>,
    pub extent_conditions: Vec<KernelExtentCondition>,
    pub trapping_stores: Vec<KernelTrappingStore>,
    pub failures: Vec<KernelRequirementFailure>,
}

/// A numeric carrier used by the body or signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelWidthRequirement {
    pub span: Span,
    pub primitive: Primitive,
    pub width: NumericWidth,
    pub policy: KernelNumericPolicy,
}

/// Store policy attached to an inferred numeric width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelNumericPolicy {
    Exact,
    Wrapping,
    Saturating,
    PolicyFloat,
}

/// A value range which must fit the portable device integer envelope for the
/// kernel to be admitted on a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelExtentCondition {
    pub span: Span,
    pub range: KernelValueRange,
    pub minimum: i128,
    pub maximum: i128,
}

/// Inclusive bounds over exact values. Symbolic terms point into the same
/// type-table index arena as the module's tensor shapes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelValueRange {
    pub minimum: KernelRangeExpr,
    pub maximum: KernelRangeExpr,
}

/// Integer expression used by the launch requirement report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KernelRangeExpr {
    Constant(i128),
    Shape(IndexId),
    Add(Box<Self>, Box<Self>),
    Subtract(Box<Self>, Box<Self>),
    Multiply(Box<Self>, Box<Self>),
    FloorDivide(Box<Self>, Box<Self>),
    Minimum(Box<Self>, Box<Self>),
    Maximum(Box<Self>, Box<Self>),
    /// A kernel parameter named by its definition. The value is unknown at
    /// check time; the term only makes sense inside a launch-time condition.
    Parameter(DefId),
}

/// An implicit trapping store whose value is not statically proven to fit.
/// N6(c)'s device flag implements this requirement; this pass only records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelTrappingStore {
    pub span: Span,
    pub width: NumericWidth,
}

/// Why a device cannot admit the body under the portable profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KernelRequirementFailure {
    RecursiveCall,
    CalleeNotAdmitted {
        callee: DefId,
    },
    /// The target's emitter does not carry this integer or `d64` width, or an
    /// expression needs a lane of that width. A capability refusal by name
    /// (never a check error): 64-bit integers and `d64` are kernel values
    /// (ruling 2026-10-28). Metal carries them (`long` / `ulong` lanes, and
    /// `d64` in a 128-bit carrier); WGSL and AMD refuse them until their
    /// emitters emulate 64-bit lanes, and CUDA until its unit lands.
    UnsupportedIntegerWidth {
        span: Span,
        width: NumericWidth,
    },
    UnsupportedFloatWidth {
        span: Span,
        width: NumericWidth,
    },
    IntermediateOutOfRange {
        span: Span,
        range: KernelValueRange,
    },
    StoreOutOfRange {
        span: Span,
        range: KernelValueRange,
    },
    UnsupportedIntegerRange {
        span: Span,
    },
    DivideByZeroPossible {
        span: Span,
    },
    NegativeShiftCountPossible {
        span: Span,
    },
    SaturatingLoweringDeferred {
        span: Span,
        width: NumericWidth,
    },
    /// The store needs the GPU-reset option-F error flag, which is not part of
    /// K15. The requirement remains explicit and the device path stays closed.
    OptionFRequired {
        span: Span,
        width: NumericWidth,
    },
    /// A generic limes callback is unresolved until a concrete specialization.
    TensorEdgePolicyDeferred {
        span: Span,
    },
}

/// Issue: `f64` is not a kernel value.
pub const ISSUE_F64: &str = "nucleum_f64";
/// Issue: an exact intermediate is provably beyond the 64-bit integer range
/// or, for `d64`, a constant operation is provably beyond the 128-bit carrier.
pub const ISSUE_WIDE_INTERMEDIATE: &str = "nucleum_wide_intermediate";
/// Issue: an integer extent has no proof that it fits the envelope.
pub const ISSUE_EXTENT_UNPROVEN: &str = "nucleum_extent_unproven";
/// Issue: a store is neither provably in range nor a wrapping/saturating cell.
pub const ISSUE_TRAPPING_STORE: &str = "nucleum_trapping_store";

/// The language-rule subset of the requirement report as check-time
/// diagnostics. Every other failure kind stays a per-target capability fact:
/// the report still describes what each device refuses, while these rules are
/// refused on every path (`faber check`, `run`, and every emit target).
///
/// Each kernel's first target row supplies the rows; per-target widths,
/// wrapping policy, and recursion are not language rules and stay in the
/// report. Diagnostics are deduplicated by `(issue, span)`.
#[must_use]
pub fn language_errors(report: &KernelRequirementsReport) -> Vec<SemanticError> {
    let mut errors = Vec::new();
    let mut seen: FxHashSet<(&'static str, u32, u32)> = FxHashSet::default();
    let rows = report
        .kernels
        .iter()
        .map(|kernel| kernel.target_reports.first())
        .chain(
            report
                .regions
                .iter()
                .map(|region| region.target_reports.first()),
        );
    for row in rows {
        let Some(row) = row else {
            continue;
        };
        for failure in &row.failures {
            let Some(diagnostic) = language_failure(failure, &row.widths) else {
                continue;
            };
            if !seen.insert((diagnostic.issue, diagnostic.span.start, diagnostic.span.end)) {
                continue;
            }
            let mut error =
                SemanticError::structured(SemanticErrorKind::LoweringError, diagnostic.span)
                    .with_arg("issue", diagnostic.issue);
            for (name, value) in diagnostic.args {
                error = error.with_arg(name, value);
            }
            errors.push(error);
        }
    }
    errors
}

/// One language-rule diagnostic row: issue, span, and its named arguments.
struct LanguageDiagnostic {
    issue: &'static str,
    span: Span,
    args: Vec<(&'static str, String)>,
}

fn language_failure(
    failure: &KernelRequirementFailure,
    widths: &[KernelWidthRequirement],
) -> Option<LanguageDiagnostic> {
    match failure {
        KernelRequirementFailure::UnsupportedFloatWidth { span, width }
            if *width == NumericWidth::F64 =>
        {
            Some(LanguageDiagnostic {
                issue: ISSUE_F64,
                span: *span,
                args: vec![("width", width.faber_marker().to_string())],
            })
        }
        KernelRequirementFailure::IntermediateOutOfRange { span, range } => {
            Some(LanguageDiagnostic {
                issue: ISSUE_WIDE_INTERMEDIATE,
                span: *span,
                args: vec![("extent", extent_label(range))],
            })
        }
        KernelRequirementFailure::UnsupportedIntegerRange { span } => Some(LanguageDiagnostic {
            issue: ISSUE_EXTENT_UNPROVEN,
            span: *span,
            args: vec![("extent", "unknown".to_string())],
        }),
        KernelRequirementFailure::StoreOutOfRange { span, .. } => {
            store_width(*span, widths).map(|width| LanguageDiagnostic {
                issue: ISSUE_TRAPPING_STORE,
                span: *span,
                args: vec![("width", width.faber_marker().to_string())],
            })
        }
        KernelRequirementFailure::OptionFRequired { span, width } => Some(LanguageDiagnostic {
            issue: ISSUE_TRAPPING_STORE,
            span: *span,
            args: vec![("width", width.faber_marker().to_string())],
        }),
        KernelRequirementFailure::RecursiveCall
        | KernelRequirementFailure::CalleeNotAdmitted { .. }
        | KernelRequirementFailure::DivideByZeroPossible { .. }
        | KernelRequirementFailure::NegativeShiftCountPossible { .. }
        | KernelRequirementFailure::SaturatingLoweringDeferred { .. }
        | KernelRequirementFailure::TensorEdgePolicyDeferred { .. }
        | KernelRequirementFailure::UnsupportedIntegerWidth { .. }
        | KernelRequirementFailure::UnsupportedFloatWidth { .. } => None,
    }
}

/// The carrier width recorded for a store at the same span as its range
/// failure. `StoreOutOfRange` carries only the value range, so the width comes
/// from the row the store already recorded.
fn store_width(span: Span, widths: &[KernelWidthRequirement]) -> Option<NumericWidth> {
    widths
        .iter()
        .find(|row| row.span == span)
        .map(|row| row.width)
}

/// Render a value range as the diagnostic `extent`: exact endpoints print as
/// digits, a symbolic axis prints as `shape`, and anything else is `unknown`.
fn extent_label(range: &KernelValueRange) -> String {
    if let (Some(minimum), Some(maximum)) = (constant(&range.minimum), constant(&range.maximum)) {
        return format!("{minimum}..{maximum}");
    }
    if contains_shape(range) {
        return "shape".to_string();
    }
    "unknown".to_string()
}

/// The declared tier of every kernel function and kernel method of a module,
/// keyed by definition. A kernel with no tier word maps to
/// [`KernelTier::Default`].
#[must_use]
pub fn declared_kernel_tiers(
    hir: &radix_hir::HirModule,
    interner: &Interner,
) -> FxHashMap<DefId, KernelTier> {
    let mut tiers = FxHashMap::default();
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(function) if function.is_kernel => {
                tiers.insert(
                    item.def_id,
                    radix_hir::kernel_tier_of(&item.annotations, interner),
                );
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    if method.func.is_kernel {
                        tiers.insert(
                            method.def_id,
                            radix_hir::kernel_tier_of(&method.annotations, interner),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    tiers
}

impl KernelRequirementsReport {
    /// Record each kernel's declared tier on its row.
    pub fn apply_tiers(&mut self, tiers: &FxHashMap<DefId, KernelTier>) {
        for kernel in &mut self.kernels {
            kernel.tier = tiers.get(&kernel.def_id).copied().unwrap_or_default();
        }
    }

    #[must_use]
    pub fn kernel(&self, def_id: DefId) -> Option<&KernelRequirement> {
        self.kernels.iter().find(|kernel| kernel.def_id == def_id)
    }

    #[must_use]
    pub fn target(&self, def_id: DefId, target: Target) -> Option<&KernelTargetRequirement> {
        self.kernel(def_id)?
            .target_reports
            .iter()
            .find(|report| report.target == target)
    }
}

/// Infer kernel requirements from a typed module. `recursive_kernels` comes
/// from the existing legality pass, which already computes the complete local
/// kernel call graph and its strongly connected definitions.
#[must_use]
pub fn check(
    hir: &radix_hir::HirModule,
    resolver: &Resolver,
    types: &TypeTable,
    interner: &Interner,
    recursive_kernels: &FxHashSet<DefId>,
    kernel_calls: &FxHashMap<DefId, Vec<DefId>>,
    tensor_edge_policies: &TensorEdgePolicyTable,
) -> KernelRequirementsReport {
    let facts = TypeFacts::from_hir(hir);
    let mut callables: FxHashMap<DefId, (&HirFunction, Span)> = FxHashMap::default();
    let mut private_kernels: FxHashSet<DefId> = FxHashSet::default();
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(function) => {
                callables.insert(item.def_id, (function.as_ref(), item.span));
                if function.is_kernel && !is_public_kernel(&item.annotations, interner) {
                    private_kernels.insert(item.def_id);
                }
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    callables.insert(method.def_id, (&method.func, method.span));
                    if method.func.is_kernel && !is_public_kernel(&method.annotations, interner) {
                        private_kernels.insert(method.def_id);
                    }
                }
            }
            _ => {}
        }
    }
    let const_values: FxHashMap<DefId, i128> = hir
        .items
        .iter()
        .filter_map(|item| {
            let HirItemKind::Constant(konst) = &item.kind else {
                return None;
            };
            (konst.statica.is_some() && !konst.mutable)
                .then(|| crate::passes::const_int_value(&konst.value))
                .flatten()
                .map(|value| (item.def_id, value))
        })
        .collect();
    let mut kernels = Vec::new();
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(function) if function.is_kernel => {
                let mut kernel = analyze_kernel(
                    item.def_id,
                    function,
                    &facts,
                    &const_values,
                    resolver,
                    types,
                    interner,
                    tensor_edge_policies,
                    recursive_kernels.contains(&item.def_id),
                    None,
                    None,
                );
                analyze_limes_callbacks(
                    &mut kernel,
                    function,
                    tensor_edge_policies,
                    &callables,
                    types,
                    interner,
                    &facts,
                    resolver,
                    &const_values,
                    recursive_kernels,
                );
                kernels.push(kernel);
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    if method.func.is_kernel {
                        let mut kernel = analyze_kernel(
                            method.def_id,
                            &method.func,
                            &facts,
                            &const_values,
                            resolver,
                            types,
                            interner,
                            tensor_edge_policies,
                            recursive_kernels.contains(&method.def_id),
                            None,
                            None,
                        );
                        analyze_limes_callbacks(
                            &mut kernel,
                            &method.func,
                            tensor_edge_policies,
                            &callables,
                            types,
                            interner,
                            &facts,
                            resolver,
                            &const_values,
                            recursive_kernels,
                        );
                        kernels.push(kernel);
                    }
                }
            }
            _ => {}
        }
    }
    seed_constant_kernel_arguments(
        &mut kernels,
        hir,
        &facts,
        &callables,
        &private_kernels,
        &const_values,
        resolver,
        types,
        interner,
        tensor_edge_policies,
        recursive_kernels,
    );
    let callback_defs: FxHashSet<DefId> = tensor_edge_policies
        .calls
        .values()
        .filter_map(|policy| match policy {
            ResolvedTensorEdgePolicy::Custom {
                remap, transform, ..
            } => Some(std::iter::once(*remap).chain(transform.iter().copied())),
            ResolvedTensorEdgePolicy::Builtin(_) | ResolvedTensorEdgePolicy::Deferred { .. } => {
                None
            }
        })
        .flatten()
        .collect();
    propagate_callee_requirements(
        &mut kernels,
        kernel_calls,
        recursive_kernels,
        &callback_defs,
    );
    let mut regions = Vec::new();
    super::device_region::for_each_region(hir, &mut |body, depth, outer| {
        // A nested region is refused by the legality pass; the outer row
        // already covers its statements.
        if depth == 0 {
            regions.push(analyze_region(
                body,
                outer,
                &facts,
                &const_values,
                resolver,
                types,
                interner,
                tensor_edge_policies,
            ));
        }
    });
    let mut report = KernelRequirementsReport { kernels, regions };
    report.apply_tiers(&declared_kernel_tiers(hir, interner));
    report
}

/// Whether a kernel declaration carries the `publica` launch marker.
fn is_public_kernel(annotations: &[HirAnnotation], interner: &Interner) -> bool {
    annotations
        .iter()
        .any(|annotation| interner.resolve(annotation.family) == "publica")
}

/// The folded integer value of an imported `module.CONST` member, when the
/// exporter folded it.
fn imported_int_const(resolver: &Resolver, object: &HirExpression, member: Symbol) -> Option<i128> {
    let HirExpressionKind::Path(module) = &object.kind else {
        return None;
    };
    let namespace = resolver
        .get_symbol(*module)
        .filter(|namespace| namespace.kind == SymbolKind::Module)?;
    match resolver.namespace_file_const_value_canonical(namespace.name, member)? {
        crate::file_interface::InterfaceConstValue::Int(value) => Some(*value),
        _ => None,
    }
}

/// Exact integer value of a call argument used to seed an inlined private
/// kernel callee, including an imported `module.CONST` value folded from its
/// file interface. `resolver` supplies that imported value.
fn kernel_constant_argument(
    expr: &HirExpression,
    const_values: &FxHashMap<DefId, i128>,
    resolver: &Resolver,
) -> Option<i128> {
    match &expr.kind {
        HirExpressionKind::Literal(radix_hir::HirLiteral::Int(value)) => Some(i128::from(*value)),
        HirExpressionKind::Literal(radix_hir::HirLiteral::BigInt(_)) => {
            crate::passes::const_int_value(expr)
        }
        HirExpressionKind::Path(def_id) => const_values.get(def_id).copied(),
        HirExpressionKind::Field(object, member) => match &object.kind {
            HirExpressionKind::Path(module) => resolver
                .get_symbol(*module)
                .filter(|namespace| namespace.kind == SymbolKind::Module)
                .and_then(|namespace| {
                    resolver.namespace_file_const_value_canonical(namespace.name, *member)
                })
                .and_then(|value| match value {
                    crate::file_interface::InterfaceConstValue::Int(value) => Some(*value),
                    _ => None,
                }),
            _ => None,
        },
        HirExpressionKind::Conversio {
            source,
            implicit: Some(_),
            ..
        } => kernel_constant_argument(source, const_values, resolver),
        HirExpressionKind::Unary(HirUnOp::Neg, inner) => {
            kernel_constant_argument(inner, const_values, resolver)?.checked_neg()
        }
        HirExpressionKind::Binary(op, lhs, rhs) => {
            let lhs = kernel_constant_argument(lhs, const_values, resolver)?;
            let rhs = kernel_constant_argument(rhs, const_values, resolver)?;
            match op {
                HirBinOp::Add => lhs.checked_add(rhs),
                HirBinOp::Sub => lhs.checked_sub(rhs),
                HirBinOp::Mul => lhs.checked_mul(rhs),
                HirBinOp::Div => lhs.checked_div(rhs),
                HirBinOp::Mod => lhs.checked_rem(rhs),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Collects the constant integer argument tuple each private kernel callee is
/// reached with, so an inlined callee can be analyzed with concrete parameters.
struct KernelCallCollector<'a, 'hir> {
    callables: &'a FxHashMap<DefId, (&'hir HirFunction, Span)>,
    const_values: &'a FxHashMap<DefId, i128>,
    resolver: &'a Resolver,
    types: &'a TypeTable,
    sites: FxHashMap<DefId, Vec<Option<FxHashMap<DefId, i128>>>>,
}

impl HirVisitor for KernelCallCollector<'_, '_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Call(callee, _, args, _) = &expr.kind
            && let HirExpressionKind::Path(callee_def) = &callee.kind
            && let Some((function, _)) = self.callables.get(callee_def)
            && function.is_kernel
        {
            let mut seed = FxHashMap::default();
            let mut complete = true;
            for (position, param) in function.params.iter().enumerate() {
                if integer_slot(param.ty, self.types).is_none() {
                    continue;
                }
                let Some(value) = args.get(position).and_then(|arg| {
                    kernel_constant_argument(&arg.expr, self.const_values, self.resolver)
                }) else {
                    complete = false;
                    break;
                };
                seed.insert(param.def_id, value);
            }
            self.sites
                .entry(*callee_def)
                .or_default()
                .push(complete.then_some(seed));
        }
        walk_expr(self, expr);
    }
}

/// Rule 12.7 inlines private kernel helpers at their call sites, so a call
/// with constant arguments proves the callee's parameter ranges. A callee is
/// analyzed once per distinct constant argument tuple and those specializations
/// are merged into its conservative row; a call site with a non-constant
/// argument for an integer parameter keeps the conservative row instead.
#[allow(clippy::too_many_arguments)]
fn seed_constant_kernel_arguments(
    kernels: &mut [KernelRequirement],
    hir: &radix_hir::HirModule,
    facts: &TypeFacts,
    callables: &FxHashMap<DefId, (&HirFunction, Span)>,
    private_kernels: &FxHashSet<DefId>,
    const_values: &FxHashMap<DefId, i128>,
    resolver: &Resolver,
    types: &TypeTable,
    interner: &Interner,
    tensor_edge_policies: &TensorEdgePolicyTable,
    recursive_kernels: &FxHashSet<DefId>,
) {
    let mut collector = KernelCallCollector {
        callables,
        const_values,
        resolver,
        types,
        sites: FxHashMap::default(),
    };
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(function) if function.is_kernel => {
                collector.visit_function(function);
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    if method.func.is_kernel {
                        collector.visit_function(&method.func);
                    }
                }
            }
            _ => {}
        }
    }
    for (callee, sites) in collector.sites {
        if !private_kernels.contains(&callee) || recursive_kernels.contains(&callee) {
            continue;
        }
        if sites.iter().any(Option::is_none) {
            continue;
        }
        let mut tuples: Vec<&FxHashMap<DefId, i128>> = Vec::new();
        for tuple in sites.iter().flatten() {
            if !tuples.contains(&tuple) {
                tuples.push(tuple);
            }
        }
        if tuples.len() == 1 && tuples[0].is_empty() {
            continue;
        }
        let Some((function, _)) = callables.get(&callee).copied() else {
            continue;
        };
        let Some(conservative) = kernels
            .iter()
            .find(|kernel| kernel.def_id == callee)
            .cloned()
        else {
            continue;
        };
        let mut seeded_rows: Vec<KernelRequirement> = Vec::new();
        for tuple in tuples {
            let mut seeded = analyze_kernel(
                callee,
                function,
                facts,
                const_values,
                resolver,
                types,
                interner,
                tensor_edge_policies,
                recursive_kernels.contains(&callee),
                None,
                Some(tuple),
            );
            analyze_limes_callbacks(
                &mut seeded,
                function,
                tensor_edge_policies,
                callables,
                types,
                interner,
                facts,
                resolver,
                const_values,
                recursive_kernels,
            );
            seeded_rows.push(seeded);
        }
        let Some(row) = kernels.iter_mut().find(|kernel| kernel.def_id == callee) else {
            continue;
        };
        for conservative_report in &conservative.target_reports {
            let seeded_reports: Vec<KernelTargetRequirement> = seeded_rows
                .iter()
                .filter_map(|seeded| {
                    seeded
                        .target_reports
                        .iter()
                        .find(|report| report.target == conservative_report.target)
                        .cloned()
                })
                .collect();
            if let Some(target_report) = row
                .target_reports
                .iter_mut()
                .find(|report| report.target == conservative_report.target)
            {
                *target_report = merge_seeded_target_report(conservative_report, &seeded_reports);
            }
        }
    }
}

/// Merges the per-specialization reports of one private kernel callee that was
/// analyzed once per distinct constant argument tuple.
///
/// A failure survives only when every seeded call shape produced it: a failure
/// avoided by one tuple is an artifact of that tuple, not a property of the
/// callee. Requirements are the union of what any call shape needs, and the
/// conservative row keeps its identity and call graph, extended by any callee
/// only the specialized shapes reached.
pub(super) fn merge_seeded_target_report(
    conservative: &KernelTargetRequirement,
    seeded: &[KernelTargetRequirement],
) -> KernelTargetRequirement {
    if seeded.is_empty() {
        return conservative.clone();
    }
    let failures = dedupe(
        seeded
            .iter()
            .flat_map(|report| report.failures.iter())
            .filter(|failure| {
                seeded.iter().all(|report| {
                    report
                        .failures
                        .iter()
                        .any(|candidate| same_seeded_failure(failure, candidate))
                })
            })
            .cloned()
            .collect(),
    );
    let mut callees = conservative.callees.clone();
    for callee in seeded
        .iter()
        .flat_map(|report| report.callees.iter().copied())
    {
        if !callees.contains(&callee) {
            callees.push(callee);
        }
    }
    KernelTargetRequirement {
        target: conservative.target,
        admitted: failures.is_empty(),
        recursive: conservative.recursive,
        callees,
        widths: dedupe(
            seeded
                .iter()
                .flat_map(|report| report.widths.iter().cloned())
                .collect(),
        ),
        extent_conditions: dedupe(
            seeded
                .iter()
                .flat_map(|report| report.extent_conditions.iter().cloned())
                .collect(),
        ),
        trapping_stores: dedupe(
            seeded
                .iter()
                .flat_map(|report| report.trapping_stores.iter().cloned())
                .collect(),
        ),
        failures,
    }
}

/// Whether two seeded failures describe the same blocker. Range payloads can
/// differ between constant call-site specializations, but the blocker remains
/// real when every specialization exceeds the envelope at the same operation.
fn same_seeded_failure(lhs: &KernelRequirementFailure, rhs: &KernelRequirementFailure) -> bool {
    match (lhs, rhs) {
        (
            KernelRequirementFailure::IntermediateOutOfRange { span: lhs, .. },
            KernelRequirementFailure::IntermediateOutOfRange { span: rhs, .. },
        )
        | (
            KernelRequirementFailure::StoreOutOfRange { span: lhs, .. },
            KernelRequirementFailure::StoreOutOfRange { span: rhs, .. },
        ) => lhs == rhs,
        _ => lhs == rhs,
    }
}

fn analyze_kernel(
    def_id: DefId,
    function: &HirFunction,
    facts: &TypeFacts,
    const_values: &FxHashMap<DefId, i128>,
    resolver: &Resolver,
    types: &TypeTable,
    interner: &Interner,
    tensor_edge_policies: &TensorEdgePolicyTable,
    recursive: bool,
    coordinate_ranges: Option<&[AbstractRange]>,
    constant_params: Option<&FxHashMap<DefId, i128>>,
) -> KernelRequirement {
    let mut base = BodyFacts {
        types,
        facts,
        interner,
        tensor_edge_policies,
        const_values,
        resolver,
        ranges: FxHashMap::default(),
        parameter_types: FxHashMap::default(),
        parameter_aliases: FxHashMap::default(),
        range_only_bindings: FxHashSet::default(),
        coordinate_ranges: FxHashMap::default(),
        expression_vectors: FxHashMap::default(),
        has_signed_coordinate_input: coordinate_ranges.is_some(),
        failures: Vec::new(),
        wide_intermediates: Vec::new(),
        conditions: Vec::new(),
        trapping_stores: Vec::new(),
        widths: Vec::new(),
        return_type: function.ret_ty,
    };
    for param in &function.params {
        base.parameter_types.insert(param.def_id, param.ty);
    }
    for (position, param) in function.params.iter().enumerate() {
        if position == 0
            && let Some(coordinates) = coordinate_ranges
        {
            base.range_only_bindings.insert(param.def_id);
            base.coordinate_ranges
                .insert(param.def_id, coordinates.to_vec());
            base.widths.push(KernelWidthRequirement {
                span: param.span,
                primitive: Primitive::Numerus,
                width: NumericWidth::I32,
                policy: KernelNumericPolicy::Exact,
            });
            base.ranges.insert(param.def_id, AbstractRange::unknown());
        } else {
            base.record_type(param.ty, param.span);
            let range = match constant_params.and_then(|values| values.get(&param.def_id)) {
                Some(value) => AbstractRange::exact(*value),
                None => AbstractRange::from_type(param.ty, types),
            };
            base.ranges.insert(param.def_id, range);
        }
    }
    base.bind_size_values(function);
    if let Some(ret_ty) = function.ret_ty
        && !is_integer_value_type(Some(ret_ty), types)
    {
        base.record_type(
            ret_ty,
            function
                .body
                .as_ref()
                .map_or(Span::default(), |body| body.span),
        );
    }
    if let Some(body) = &function.body {
        base.visit_block_stateful(body);
    }
    let widths = dedupe(base.widths);
    let conditions = dedupe(base.conditions);
    let trapping_stores = dedupe(base.trapping_stores);
    let failures = dedupe(base.failures);
    let wide_intermediates = dedupe(base.wide_intermediates);

    KernelRequirement {
        def_id,
        name: interner.resolve(function.name).to_owned(),
        tier: KernelTier::Default,
        target_reports: target_rows(
            &failures,
            &wide_intermediates,
            &widths,
            &conditions,
            &trapping_stores,
            recursive,
        ),
    }
}

/// One requirement row per device target from the facts a body produced.
fn target_rows(
    failures: &[KernelRequirementFailure],
    wide_intermediates: &[(Span, KernelValueRange, bool)],
    widths: &[KernelWidthRequirement],
    conditions: &[KernelExtentCondition],
    trapping_stores: &[KernelTrappingStore],
    recursive: bool,
) -> Vec<KernelTargetRequirement> {
    DEVICE_TARGETS
        .iter()
        .copied()
        .map(|target| {
            let mut target_failures = failures.to_vec();
            if recursive {
                target_failures.push(KernelRequirementFailure::RecursiveCall);
            }
            // An exact intermediate beyond the portable 32-bit envelope needs
            // a 64-bit lane, and a 32-bit lane would silently wrap it. A
            // target whose emitter carries 64-bit integers computes one that
            // is its expression's own type natively (`long` / `ulong`); every
            // other intermediate, and every other target, refuses by name.
            for (span, range, native) in wide_intermediates {
                if *native && target.supports_wide_kernel_values() {
                    continue;
                }
                target_failures.push(KernelRequirementFailure::UnsupportedIntegerWidth {
                    span: *span,
                    width: wide_lane_width(range),
                });
            }
            for requirement in widths {
                if requirement.policy == KernelNumericPolicy::Saturating {
                    target_failures.push(KernelRequirementFailure::SaturatingLoweringDeferred {
                        span: requirement.span,
                        width: requirement.width,
                    });
                }
                if !supports_width(target, requirement) {
                    target_failures.push(width_failure(requirement));
                }
            }
            let failures = dedupe(target_failures);
            KernelTargetRequirement {
                target,
                admitted: failures.is_empty(),
                recursive,
                callees: Vec::new(),
                widths: widths.to_vec(),
                extent_conditions: conditions.to_vec(),
                trapping_stores: trapping_stores.to_vec(),
                failures,
            }
        })
        .collect()
}

/// Requirements of one `fac machina` region: the same body facts a kernel
/// body gets, over the block's statements, with no parameters.
fn analyze_region(
    region: &super::device_region::RegionBody<'_>,
    outer: &super::device_region::OuterNames,
    facts: &TypeFacts,
    const_values: &FxHashMap<DefId, i128>,
    resolver: &Resolver,
    types: &TypeTable,
    interner: &Interner,
    tensor_edge_policies: &TensorEdgePolicyTable,
) -> DeviceRegionRequirement {
    let mut base = BodyFacts {
        types,
        facts,
        interner,
        tensor_edge_policies,
        const_values,
        resolver,
        ranges: FxHashMap::default(),
        parameter_types: FxHashMap::default(),
        parameter_aliases: FxHashMap::default(),
        range_only_bindings: FxHashSet::default(),
        coordinate_ranges: FxHashMap::default(),
        expression_vectors: FxHashMap::default(),
        has_signed_coordinate_input: false,
        failures: Vec::new(),
        wide_intermediates: Vec::new(),
        conditions: Vec::new(),
        trapping_stores: Vec::new(),
        widths: Vec::new(),
        return_type: None,
    };
    let region_facts = super::device_region::region_facts(resolver, types, outer, interner, region);
    base.bind_captured_size_values(&region_facts.captured);
    for statement in region.statements {
        base.visit_statement_stateful(statement);
    }
    if let Some(tail) = region.tail {
        base.evaluate(tail);
    }
    let widths = dedupe(base.widths);
    let conditions = dedupe(base.conditions);
    let trapping_stores = dedupe(base.trapping_stores);
    let failures = dedupe(base.failures);
    let wide_intermediates = dedupe(base.wide_intermediates);
    DeviceRegionRequirement {
        span: region.span,
        target_reports: target_rows(
            &failures,
            &wide_intermediates,
            &widths,
            &conditions,
            &trapping_stores,
            false,
        ),
        facts: region_facts,
        host_syncs: 0,
    }
}

fn analyze_limes_callbacks(
    owner: &mut KernelRequirement,
    kernel_function: &HirFunction,
    policies: &TensorEdgePolicyTable,
    callables: &FxHashMap<DefId, (&HirFunction, Span)>,
    types: &TypeTable,
    interner: &Interner,
    facts: &TypeFacts,
    resolver: &Resolver,
    const_values: &FxHashMap<DefId, i128>,
    recursive_kernels: &FxHashSet<DefId>,
) {
    analyze_limes_callbacks_inner(
        owner,
        kernel_function,
        policies,
        callables,
        types,
        interner,
        facts,
        resolver,
        const_values,
        recursive_kernels,
        &mut FxHashSet::default(),
    );
}

fn analyze_limes_callbacks_inner(
    owner: &mut KernelRequirement,
    kernel_function: &HirFunction,
    policies: &TensorEdgePolicyTable,
    callables: &FxHashMap<DefId, (&HirFunction, Span)>,
    types: &TypeTable,
    interner: &Interner,
    facts: &TypeFacts,
    resolver: &Resolver,
    const_values: &FxHashMap<DefId, i128>,
    recursive_kernels: &FxHashSet<DefId>,
    active_callbacks: &mut FxHashSet<DefId>,
) {
    let mut collector = PolicyCallCollector {
        policies,
        expressions: FxHashMap::default(),
        needed_expressions: policies
            .calls
            .keys()
            .copied()
            .chain(
                policies
                    .limes_contexts
                    .values()
                    .map(|context| context.shift_expression),
            )
            .collect(),
        policy_calls: Vec::new(),
    };
    collector.visit_function(kernel_function);

    for call_id in collector.policy_calls {
        let Some(ResolvedTensorEdgePolicy::Custom {
            remap, transform, ..
        }) = policies.calls.get(&call_id)
        else {
            continue;
        };
        let Some(call_expr) = collector.expressions.get(&call_id) else {
            continue;
        };
        let coordinate_seed = policies.limes_contexts.get(&call_id).and_then(|context| {
            let shifts = collector.expressions.get(&context.shift_expression)?;
            callback_coordinate_ranges(context.source_shape, shifts, types)
        });
        let callback_defs = std::iter::once(*remap).chain(transform.iter().copied());
        for callback_def in callback_defs {
            if !active_callbacks.insert(callback_def) {
                add_callback_failure(owner, KernelRequirementFailure::RecursiveCall);
                continue;
            }
            let Some((callback, _)) = callables.get(&callback_def).copied() else {
                add_callback_failure(
                    owner,
                    KernelRequirementFailure::CalleeNotAdmitted {
                        callee: callback_def,
                    },
                );
                active_callbacks.remove(&callback_def);
                continue;
            };
            let coordinates = coordinate_seed
                .as_ref()
                .map(|(ranges, _)| ranges.as_slice());
            let mut callback_report = analyze_kernel(
                callback_def,
                callback,
                facts,
                const_values,
                resolver,
                types,
                interner,
                policies,
                recursive_kernels.contains(&callback_def),
                coordinates,
                None,
            );
            if let Some((_, conditions)) = &coordinate_seed {
                for report in &mut callback_report.target_reports {
                    report.extent_conditions.extend(conditions.iter().cloned());
                    report.extent_conditions =
                        dedupe(std::mem::take(&mut report.extent_conditions));
                }
            } else {
                add_callback_failure(
                    &mut callback_report,
                    KernelRequirementFailure::UnsupportedIntegerRange {
                        span: call_expr.span,
                    },
                );
            }
            analyze_limes_callbacks_inner(
                &mut callback_report,
                callback,
                policies,
                callables,
                types,
                interner,
                facts,
                resolver,
                const_values,
                recursive_kernels,
                active_callbacks,
            );

            for owner_report in &mut owner.target_reports {
                if let Some(callback_report) = callback_report
                    .target_reports
                    .iter()
                    .find(|report| report.target == owner_report.target)
                {
                    owner_report
                        .widths
                        .extend(callback_report.widths.iter().cloned());
                    owner_report
                        .extent_conditions
                        .extend(callback_report.extent_conditions.iter().cloned());
                    owner_report
                        .trapping_stores
                        .extend(callback_report.trapping_stores.iter().cloned());
                    owner_report
                        .failures
                        .extend(callback_report.failures.iter().cloned());
                } else {
                    owner_report
                        .failures
                        .push(KernelRequirementFailure::CalleeNotAdmitted {
                            callee: callback_def,
                        });
                }
                owner_report.widths = dedupe(std::mem::take(&mut owner_report.widths));
                owner_report.extent_conditions =
                    dedupe(std::mem::take(&mut owner_report.extent_conditions));
                owner_report.trapping_stores =
                    dedupe(std::mem::take(&mut owner_report.trapping_stores));
                owner_report.failures = dedupe(std::mem::take(&mut owner_report.failures));
                owner_report.admitted = owner_report.failures.is_empty();
            }
            active_callbacks.remove(&callback_def);
        }
    }
}

fn add_callback_failure(report: &mut KernelRequirement, failure: KernelRequirementFailure) {
    for target in &mut report.target_reports {
        target.failures.push(failure.clone());
        target.failures = dedupe(std::mem::take(&mut target.failures));
        target.admitted = false;
    }
}

fn callback_coordinate_ranges(
    shape: IndexId,
    shift_expression: &HirExpression,
    types: &TypeTable,
) -> Option<(Vec<AbstractRange>, Vec<KernelExtentCondition>)> {
    let dimensions = match types.get_index(shape) {
        IndexExpr::Tuple(dimensions) => dimensions.clone(),
        IndexExpr::Unspecified | IndexExpr::Infer(_) => return None,
        _ => vec![shape],
    };
    let HirExpressionKind::Array(elements) = &shift_expression.kind else {
        return None;
    };
    if elements.len() != dimensions.len() {
        return None;
    }

    let mut ranges = Vec::with_capacity(dimensions.len());
    let mut conditions = Vec::new();
    for (dimension, element) in dimensions.into_iter().zip(elements) {
        let HirArrayElement::Expr(offset_expr) = element else {
            return None;
        };
        let offset = crate::passes::const_int_value(offset_expr)?;
        let coordinate = coordinate_range(dimension, types);
        let KernelValueRange { minimum, maximum } = coordinate.0?;
        let range = KernelValueRange {
            minimum: add(minimum, KernelRangeExpr::Constant(offset)),
            maximum: add(maximum, KernelRangeExpr::Constant(offset)),
        };
        let minimum_bounds = symbolic_bounds(&range.minimum);
        let maximum_bounds = symbolic_bounds(&range.maximum);
        let impossible = minimum_bounds
            .minimum
            .is_some_and(|minimum| minimum > 2_147_483_647)
            || maximum_bounds
                .minimum
                .is_some_and(|maximum| maximum > 2_147_483_647)
            || maximum_bounds
                .maximum
                .is_some_and(|maximum| maximum < DEVICE_INTEGER_MIN);
        let fits = minimum_bounds
            .minimum
            .is_some_and(|minimum| minimum >= DEVICE_INTEGER_MIN)
            && maximum_bounds
                .maximum
                .is_some_and(|maximum| maximum <= 2_147_483_647);
        if impossible
            || constant(&range.minimum)
                .is_some_and(|min| !(DEVICE_INTEGER_MIN..=2_147_483_647).contains(&min))
        {
            return None;
        }
        if !fits {
            if !contains_shape(&range)
                || minimum_bounds
                    .minimum
                    .is_none_or(|minimum| minimum < DEVICE_INTEGER_MIN)
                || maximum_bounds
                    .minimum
                    .is_none_or(|maximum| maximum > 2_147_483_647)
            {
                return None;
            }
            conditions.push(KernelExtentCondition {
                span: shift_expression.span,
                range: range.clone(),
                minimum: DEVICE_INTEGER_MIN,
                maximum: 2_147_483_647,
            });
        }
        ranges.push(AbstractRange(Some(range)));
    }
    Some((ranges, conditions))
}

struct PolicyCallCollector<'a> {
    policies: &'a TensorEdgePolicyTable,
    expressions: FxHashMap<HirId, HirExpression>,
    needed_expressions: FxHashSet<HirId>,
    policy_calls: Vec<HirId>,
}

impl HirVisitor for PolicyCallCollector<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if self.needed_expressions.contains(&expr.id) {
            self.expressions.insert(expr.id, expr.clone());
        }
        if self.policies.calls.contains_key(&expr.id) {
            self.policy_calls.push(expr.id);
        }
        walk_expr(self, expr);
    }
}

/// Index of a kernel report's target within [`DEVICE_TARGETS`].
fn device_target_index(target: Target) -> usize {
    DEVICE_TARGETS
        .iter()
        .position(|device| *device == target)
        .expect("all kernel reports use a device target")
}

fn supports_width(target: Target, requirement: &KernelWidthRequirement) -> bool {
    match requirement.primitive {
        Primitive::Numerus => {
            let narrow_integer = matches!(
                requirement.width,
                NumericWidth::I8
                    | NumericWidth::I16
                    | NumericWidth::I32
                    | NumericWidth::U8
                    | NumericWidth::U16
                    | NumericWidth::U32
            );
            let wide_integer = target.supports_wide_kernel_values()
                && matches!(requirement.width, NumericWidth::I64 | NumericWidth::U64);
            let integer_width = narrow_integer || wide_integer;
            match requirement.policy {
                KernelNumericPolicy::Exact => {
                    integer_width
                        || (target.supports_wide_kernel_values()
                            && requirement.width == NumericWidth::D64)
                }
                KernelNumericPolicy::Saturating => integer_width,
                KernelNumericPolicy::Wrapping => {
                    target.supports_wrapping_kernel_integers() && integer_width
                }
                KernelNumericPolicy::PolicyFloat => false,
            }
        }
        Primitive::Fractus => match requirement.width {
            NumericWidth::F32 => true,
            NumericWidth::F16 | NumericWidth::BF16 => target.is_device_target(),
            _ => false,
        },
        _ => false,
    }
}

/// The 64-bit lane an out-of-envelope intermediate needs: unsigned when the
/// range is non-negative and exceeds `i64`, signed otherwise.
fn wide_lane_width(range: &KernelValueRange) -> NumericWidth {
    let unsigned = constant(&range.minimum).is_some_and(|minimum| minimum >= 0)
        && constant(&range.maximum).is_some_and(|maximum| maximum > i128::from(i64::MAX));
    if unsigned {
        NumericWidth::U64
    } else {
        NumericWidth::I64
    }
}

fn width_failure(requirement: &KernelWidthRequirement) -> KernelRequirementFailure {
    if requirement.primitive == Primitive::Fractus {
        KernelRequirementFailure::UnsupportedFloatWidth {
            span: requirement.span,
            width: requirement.width,
        }
    } else {
        KernelRequirementFailure::UnsupportedIntegerWidth {
            span: requirement.span,
            width: requirement.width,
        }
    }
}

/// Copy target blockers through the complete local kernel call graph. Extent
/// requirements themselves stay attached to each callee's report row: a
/// caller's actual arguments determine how the callee's formal axes bind.
fn propagate_callee_requirements(
    kernels: &mut [KernelRequirement],
    calls: &FxHashMap<DefId, Vec<DefId>>,
    recursive_kernels: &FxHashSet<DefId>,
    callback_defs: &FxHashSet<DefId>,
) {
    let target_summaries: FxHashMap<(DefId, usize), KernelTargetRequirement> = kernels
        .iter()
        .flat_map(|kernel| {
            kernel.target_reports.iter().map(move |report| {
                (
                    (kernel.def_id, device_target_index(report.target)),
                    report.clone(),
                )
            })
        })
        .collect();
    let mut requirements = Vec::with_capacity(kernels.len());
    for kernel in kernels.iter() {
        let mut reached = Vec::new();
        let mut seen = FxHashSet::default();
        let mut stack = calls.get(&kernel.def_id).cloned().unwrap_or_default();
        while let Some(callee) = stack.pop() {
            if !seen.insert(callee) {
                continue;
            }
            reached.push(callee);
            stack.extend(calls.get(&callee).into_iter().flatten().copied());
        }
        let reaches_recursion = reached
            .iter()
            .any(|callee| recursive_kernels.contains(callee));
        requirements.push((reached, reaches_recursion));
    }

    for (kernel, (reached, reaches_recursion)) in kernels.iter_mut().zip(requirements) {
        for report in &mut kernel.target_reports {
            report.callees = reached
                .iter()
                .filter(|callee| !callback_defs.contains(callee))
                .copied()
                .collect();
            report.recursive |= reaches_recursion;
            if reaches_recursion
                && !report
                    .failures
                    .contains(&KernelRequirementFailure::RecursiveCall)
            {
                report
                    .failures
                    .push(KernelRequirementFailure::RecursiveCall);
            }
            let target_index = device_target_index(report.target);
            for callee in &reached {
                if let Some(callee_report) = target_summaries.get(&(*callee, target_index)) {
                    report.widths.extend(callee_report.widths.iter().cloned());
                    report
                        .extent_conditions
                        .extend(callee_report.extent_conditions.iter().cloned());
                    report
                        .trapping_stores
                        .extend(callee_report.trapping_stores.iter().cloned());
                    if !callee_report.admitted {
                        report
                            .failures
                            .push(KernelRequirementFailure::CalleeNotAdmitted { callee: *callee });
                    }
                } else if callback_defs.contains(callee) {
                    // The callback's context-specific report has already been
                    // folded into the owning kernel. Its kernel callees remain
                    // in `reached` and are summarized independently below.
                } else {
                    report
                        .failures
                        .push(KernelRequirementFailure::CalleeNotAdmitted { callee: *callee });
                }
            }
            report.widths = dedupe(std::mem::take(&mut report.widths));
            report.extent_conditions = dedupe(std::mem::take(&mut report.extent_conditions));
            report.trapping_stores = dedupe(std::mem::take(&mut report.trapping_stores));
            report.failures = dedupe(std::mem::take(&mut report.failures));
            report.admitted = report.failures.is_empty();
        }
    }
}

/// The portable 32-bit device envelope: tensor addressing, launch extents and
/// the lane-coordinate carrier. A target without 64-bit lanes also refuses an
/// exact integer intermediate beyond it.
const DEVICE_INTEGER_MIN: i128 = -2_147_483_648;
const DEVICE_INTEGER_MAX: i128 = 4_294_967_295;
/// The host's 64-bit integer intermediate range (ruling 17): every exact
/// integer intermediate must lie in it, on every target.
const WIDE_INTEGER_MIN: i128 = -9_223_372_036_854_775_808;
const WIDE_INTEGER_MAX: i128 = 18_446_744_073_709_551_615;
/// Scale of a stored `d64`: an `i64` mantissa at 10⁻⁸.
const D64_STORE_SCALE: u32 = 8;

#[derive(Debug, Clone, Copy)]
enum D64Constant {
    Value(i128, u32),
    Overflow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AbstractRange(Option<KernelValueRange>);

impl AbstractRange {
    fn unknown() -> Self {
        Self(None)
    }

    fn exact(value: i128) -> Self {
        Self(Some(KernelValueRange {
            minimum: KernelRangeExpr::Constant(value),
            maximum: KernelRangeExpr::Constant(value),
        }))
    }

    fn from_type(ty: TypeId, types: &TypeTable) -> Self {
        type_range(ty, types).map_or_else(Self::unknown, |range| Self(Some(range)))
    }
}

fn is_d64_type(ty: Option<TypeId>, types: &TypeTable) -> bool {
    ty.is_some_and(|ty| {
        matches!(
            types.get(numeric_scalar_type(ty, types)),
            Type::SizedNumeric(_, NumericWidth::D64)
        )
    })
}

fn decimal_constant(value: i128) -> D64Constant {
    value
        .checked_mul(10i128.pow(D64_STORE_SCALE))
        .map_or(D64Constant::Overflow, |value| {
            D64Constant::Value(value, D64_STORE_SCALE)
        })
}

fn scale_d64_constant(value: i128, from: u32, to: u32) -> Option<i128> {
    value.checked_mul(10i128.checked_pow(to.checked_sub(from)?)?)
}

fn normalize_d64_constant(mut value: i128, mut scale: u32) -> D64Constant {
    while scale > D64_STORE_SCALE && value % 10 == 0 {
        value /= 10;
        scale -= 1;
    }
    D64Constant::Value(value, scale)
}

struct BodyFacts<'a> {
    types: &'a TypeTable,
    facts: &'a TypeFacts,
    interner: &'a Interner,
    tensor_edge_policies: &'a TensorEdgePolicyTable,
    const_values: &'a FxHashMap<DefId, i128>,
    resolver: &'a Resolver,
    ranges: FxHashMap<DefId, AbstractRange>,
    parameter_types: FxHashMap<DefId, TypeId>,
    parameter_aliases: FxHashMap<DefId, DefId>,
    range_only_bindings: FxHashSet<DefId>,
    coordinate_ranges: FxHashMap<DefId, Vec<AbstractRange>>,
    expression_vectors: FxHashMap<HirId, Vec<AbstractRange>>,
    has_signed_coordinate_input: bool,
    failures: Vec<KernelRequirementFailure>,
    /// Exact intermediates inside the 64-bit range but beyond the portable
    /// 32-bit envelope. They need a 64-bit lane. The flag is whether the
    /// expression's own type is that lane (an `i64` or `u64` result holding a
    /// value of its own range): such an intermediate is computed natively by
    /// a target that carries 64-bit integers. Any other (a narrower type, or a
    /// wrapping word observed beyond its carrier) needs a widening that no
    /// emitter performs yet, so each target row refuses it by name.
    wide_intermediates: Vec<(Span, KernelValueRange, bool)>,
    conditions: Vec<KernelExtentCondition>,
    trapping_stores: Vec<KernelTrappingStore>,
    widths: Vec<KernelWidthRequirement>,
    return_type: Option<TypeId>,
}

impl BodyFacts<'_> {
    fn visit_block_stateful(&mut self, block: &HirBlock) {
        for stmt in &block.statements {
            self.visit_statement_stateful(stmt);
        }
        if let Some(expr) = &block.expr {
            self.evaluate(expr);
        }
    }

    fn visit_statement_stateful(&mut self, stmt: &HirStatement) {
        match &stmt.kind {
            HirStatementKind::Local(local) => self.visit_local_stateful(local),
            HirStatementKind::Expr(expr) => {
                self.evaluate(expr);
            }
            HirStatementKind::Redde(value) => {
                if let Some(value) = value {
                    let range = self.evaluate(value);
                    if let Some(return_type) = self.return_type {
                        self.check_store(stmt.span, return_type, range);
                    }
                }
            }
            HirStatementKind::ReddeFilum(value) => {
                if let Some(value) = value {
                    self.evaluate(value);
                }
            }
            HirStatementKind::IncDec(inc_dec) => {
                let target_range = self.evaluate(&inc_dec.target);
                let delta = match inc_dec.op {
                    HirIncDecOp::Inc => 1,
                    HirIncDecOp::Dec => -1,
                };
                let value = binary_range(
                    if delta > 0 {
                        HirBinOp::Add
                    } else {
                        HirBinOp::Sub
                    },
                    target_range,
                    AbstractRange::exact(1),
                );
                if let Some(ty) = inc_dec.target.ty {
                    self.check_value_intermediate(stmt.span, value.clone(), Some(ty));
                    let stored = self.check_store(stmt.span, ty, value);
                    if let HirExpressionKind::Path(def_id) = inc_dec.target.kind {
                        self.ranges.insert(def_id, stored);
                    }
                }
            }
            HirStatementKind::Custodi(custodi) => {
                for clause in &custodi.clauses {
                    let before = self.ranges.clone();
                    self.evaluate(&clause.cond);
                    self.ranges.clone_from(&before);
                    self.refine_condition(&clause.cond, true);
                    self.visit_block_stateful(&clause.body);
                    let entered = self.ranges.clone();
                    self.ranges = merge_ranges(before, entered);
                }
            }
            HirStatementKind::Rumpe | HirStatementKind::Perge | HirStatementKind::Tacet => {}
        }
    }

    fn visit_local_stateful(&mut self, local: &HirLocal) {
        let declared = local
            .ty
            .or_else(|| local.init.as_ref().and_then(|init| init.ty));
        let range_only_init = local.init.as_ref().is_some_and(|init| {
            self.range_only_expression(init)
                && declared.is_some_and(|ty| is_range_narrowable_type(ty, self.types))
        });
        if range_only_init {
            self.range_only_bindings.insert(local.def_id);
        }
        if let Some(ty) = declared.filter(|_| !range_only_init) {
            self.record_type(ty, local.init.as_ref().map_or(Span::default(), |e| e.span));
        }
        let value = local
            .init
            .as_ref()
            .map_or_else(AbstractRange::unknown, |init| self.evaluate(init));
        if let Some(init) = &local.init {
            let vector_ranges =
                self.expression_vectors
                    .get(&init.id)
                    .cloned()
                    .or_else(|| match init.kind {
                        HirExpressionKind::Path(def_id) => {
                            self.coordinate_ranges.get(&def_id).cloned()
                        }
                        _ => None,
                    });
            if let Some(vector_ranges) = vector_ranges {
                self.coordinate_ranges.insert(local.def_id, vector_ranges);
            }
        }
        if let Some(init) = &local.init
            && let HirExpressionKind::Path(source) = init.kind
            && let Some(parameter) = self.parameter_of(source)
        {
            self.parameter_aliases.insert(local.def_id, parameter);
        }
        let stored = declared.map_or(value.clone(), |ty| {
            self.check_store(
                local.init.as_ref().map_or(Span::default(), |e| e.span),
                ty,
                value,
            )
        });
        self.ranges.insert(local.def_id, stored);
    }

    fn evaluate(&mut self, expr: &HirExpression) -> AbstractRange {
        if let Some(ty) = expr.ty
            && !(self.range_only_expression(expr) && is_range_narrowable_type(ty, self.types))
            && !self.float_constant_only(expr, ty)
            && !self.range_proven_reduction(expr)
        {
            self.record_type(ty, expr.span);
        }
        match &expr.kind {
            HirExpressionKind::Literal(radix_hir::HirLiteral::Int(value)) => {
                let range = AbstractRange::exact(i128::from(*value));
                self.check_value_intermediate(expr.span, range.clone(), expr.ty);
                range
            }
            HirExpressionKind::Literal(radix_hir::HirLiteral::BigInt(_)) => {
                let range = crate::passes::const_int_value(expr)
                    .map_or_else(AbstractRange::unknown, AbstractRange::exact);
                self.check_value_intermediate(expr.span, range.clone(), expr.ty);
                range
            }
            HirExpressionKind::Path(def_id) => self.path_range(*def_id, expr.ty),
            HirExpressionKind::Index(base, index) => {
                let index_range = self.evaluate_index_coordinates(index);
                let index_range = self
                    .linear_tensor_index_range(base, index)
                    .unwrap_or(index_range);
                // Generated device address arithmetic is part of the kernel
                // requirement even when the source never stores the lane id.
                // In particular, an `apud` binder keeps its `int` source type
                // but contributes a symbolic [0, E) requirement for the u32
                // device index path.
                self.check_intermediate(index.span, index_range);
                if let HirExpressionKind::Path(def_id) = base.kind
                    && let Some(axes) = self.coordinate_ranges.get(&def_id)
                    && is_integer_value_type(expr.ty, self.types)
                {
                    if let Some(axis) = crate::passes::const_int_value(index)
                        .and_then(|axis| usize::try_from(axis).ok())
                    {
                        let range = axes
                            .get(axis)
                            .cloned()
                            .unwrap_or_else(AbstractRange::unknown);
                        self.check_intermediate(expr.span, range.clone());
                        return range;
                    }
                    let range = merge_abstract_ranges(axes);
                    self.check_intermediate(expr.span, range.clone());
                    return range;
                }
                self.evaluate(base);
                expr.ty.map_or_else(AbstractRange::unknown, |ty| {
                    AbstractRange::from_type(ty, self.types)
                })
            }
            HirExpressionKind::Unary(op, operand) => {
                let input = self.evaluate(operand);
                let modular_ty = expr
                    .ty
                    .filter(|ty| is_modular_unary(Some(*ty), *op, self.types));
                let output = if let Some(ty) = modular_ty {
                    AbstractRange::from_type(ty, self.types)
                } else {
                    unary_range(*op, input)
                };
                if is_integer_value_type(expr.ty, self.types) && modular_ty.is_none() {
                    self.check_value_intermediate(expr.span, output.clone(), expr.ty);
                }
                if *op == HirUnOp::Neg && is_d64_type(expr.ty, self.types) {
                    self.check_d64_intermediate(expr);
                }
                output
            }
            HirExpressionKind::Binary(op, lhs, rhs) => {
                if is_non_homomorphic_observer(*op) {
                    self.check_observed_modular_intermediates(lhs);
                    self.check_observed_modular_intermediates(rhs);
                    if *op == HirBinOp::Div
                        && let Some(carrier) = modular_carrier_bounds(expr.ty, self.types)
                    {
                        let exact = binary_range(*op, self.peek_range(lhs), self.peek_range(rhs));
                        self.check_value_intermediate_in(expr.span, exact, carrier, None);
                    }
                }
                let left = self.evaluate(lhs);
                let right = self.evaluate(rhs);
                let mut assumed_divisor_max = None;
                if is_integer_value_type(expr.ty, self.types) {
                    match op {
                        HirBinOp::Div | HirBinOp::Mod if !excludes_zero(&right) => {
                            let positive = self.positive_parameter_divisor(rhs);
                            if let Some((parameter, divisor_max)) = positive {
                                // The launch contract binds a positive size-class
                                // parameter to [1, DEVICE_INTEGER_MAX], so neither the
                                // quotient site nor the remainder site needs a zero
                                // proof of its own.
                                self.conditions.push(KernelExtentCondition {
                                    span: expr.span,
                                    range: KernelValueRange {
                                        minimum: KernelRangeExpr::Parameter(parameter),
                                        maximum: KernelRangeExpr::Parameter(parameter),
                                    },
                                    minimum: 1,
                                    maximum: DEVICE_INTEGER_MAX,
                                });
                                assumed_divisor_max = Some(divisor_max);
                            } else {
                                self.failures.push(
                                    KernelRequirementFailure::DivideByZeroPossible {
                                        span: expr.span,
                                    },
                                );
                            }
                        }
                        HirBinOp::Shl | HirBinOp::Shr if !proves_nonnegative(&right) => {
                            self.failures.push(
                                KernelRequirementFailure::NegativeShiftCountPossible {
                                    span: expr.span,
                                },
                            );
                        }
                        _ => {}
                    }
                }
                let modular_ty = expr
                    .ty
                    .filter(|ty| is_modular_binary(Some(*ty), *op, self.types));
                let output = if *op == HirBinOp::Coalesce {
                    // The payload of the left side, or the right side.
                    merge_abstract_ranges(&[left, right])
                } else if let Some(ty) = modular_ty {
                    AbstractRange::from_type(ty, self.types)
                } else if let Some(divisor_max) = assumed_divisor_max {
                    // The divisor is assumed to lie in [1, divisor_max]. Floor
                    // division by a positive divisor is monotone in both
                    // arguments, so the endpointwise fold over those divisor
                    // candidates is exact for the quotient; a floored remainder
                    // lies in [0, divisor_max - 1] for either numerator sign.
                    match (*op, left.0) {
                        (HirBinOp::Div, Some(lhs)) => {
                            let divisors = [
                                KernelRangeExpr::Constant(1),
                                KernelRangeExpr::Constant(divisor_max),
                            ];
                            let candidates = [
                                floor_divide(lhs.minimum.clone(), divisors[0].clone()),
                                floor_divide(lhs.minimum.clone(), divisors[1].clone()),
                                floor_divide(lhs.maximum.clone(), divisors[0].clone()),
                                floor_divide(lhs.maximum.clone(), divisors[1].clone()),
                            ];
                            candidates
                                .into_iter()
                                .collect::<Option<Vec<_>>>()
                                .and_then(|values| {
                                    let mut values = values.into_iter();
                                    let first = values.next()?;
                                    let (minimum_value, maximum_value) =
                                        values.fold((first.clone(), first), |(lo, hi), value| {
                                            (minimum(lo, value.clone()), maximum(hi, value))
                                        });
                                    Some(AbstractRange(Some(KernelValueRange {
                                        minimum: minimum_value,
                                        maximum: maximum_value,
                                    })))
                                })
                                .unwrap_or_else(AbstractRange::unknown)
                        }
                        (HirBinOp::Div, None) => AbstractRange::unknown(),
                        _ => AbstractRange(Some(KernelValueRange {
                            minimum: KernelRangeExpr::Constant(0),
                            maximum: KernelRangeExpr::Constant(divisor_max.saturating_sub(1)),
                        })),
                    }
                } else {
                    binary_range(*op, left, right)
                };
                if is_integer_value_operator(*op)
                    && is_integer_value_type(expr.ty, self.types)
                    && modular_ty.is_none()
                {
                    self.check_value_intermediate(expr.span, output.clone(), expr.ty);
                }
                if matches!(
                    op,
                    HirBinOp::Add | HirBinOp::Sub | HirBinOp::Mul | HirBinOp::Div | HirBinOp::Mod
                ) && is_d64_type(expr.ty, self.types)
                {
                    self.check_d64_intermediate(expr);
                }
                output
            }
            HirExpressionKind::Assign(lhs, rhs) => {
                let value = self.evaluate(rhs);
                // The store address of a bounded N-D tensor place is device
                // address arithmetic too (a lane-loop write `dst[[i, j]]`).
                if let HirExpressionKind::Index(base, index) = &lhs.kind
                    && matches!(index.kind, HirExpressionKind::Array(_))
                {
                    self.evaluate(index);
                    if let Some(address) = self.linear_tensor_index_range(base, index) {
                        self.check_intermediate(index.span, address);
                    }
                }
                let Some(slot) = lhs.ty else {
                    return value;
                };
                let stored = self.check_store(expr.span, slot, value);
                if let HirExpressionKind::Path(def_id) = lhs.kind {
                    self.ranges.insert(def_id, stored.clone());
                    if self.range_only_expression(rhs) && is_range_narrowable_type(slot, self.types)
                    {
                        self.range_only_bindings.insert(def_id);
                    } else {
                        self.range_only_bindings.remove(&def_id);
                    }
                    if let Some(vector_ranges) = self
                        .expression_vectors
                        .get(&rhs.id)
                        .cloned()
                        .or_else(|| match rhs.kind {
                            HirExpressionKind::Path(source) => {
                                self.coordinate_ranges.get(&source).cloned()
                            }
                            _ => None,
                        })
                    {
                        self.coordinate_ranges.insert(def_id, vector_ranges);
                    } else {
                        self.coordinate_ranges.remove(&def_id);
                    }
                }
                stored
            }
            HirExpressionKind::Conversio {
                source,
                target,
                implicit,
                ..
            } => {
                let value = self.evaluate(source);
                if implicit.is_some()
                    && let radix_hir::HirConversioTarget::Type(slot) = target
                {
                    return self.check_store(expr.span, *slot, value);
                }
                expr.ty
                    .map_or(value, |ty| AbstractRange::from_type(ty, self.types))
            }
            HirExpressionKind::Widen { operand, .. } => self.evaluate(operand),
            HirExpressionKind::Verte { source, target, .. } => {
                let value = self.evaluate(source);
                self.check_store(expr.span, *target, value)
            }
            HirExpressionKind::ConversioAssign { target, source, .. } => {
                let value = self.evaluate(source);
                let Some(slot) = target.ty else {
                    return value;
                };
                let stored = self.check_store(expr.span, slot, value);
                if let HirExpressionKind::Path(def_id) = target.kind {
                    self.ranges.insert(def_id, stored.clone());
                    if self.range_only_expression(source)
                        && is_range_narrowable_type(slot, self.types)
                    {
                        self.range_only_bindings.insert(def_id);
                    } else {
                        self.range_only_bindings.remove(&def_id);
                    }
                    if let Some(vector_ranges) = self
                        .expression_vectors
                        .get(&source.id)
                        .cloned()
                        .or_else(|| match source.kind {
                            HirExpressionKind::Path(source_def) => {
                                self.coordinate_ranges.get(&source_def).cloned()
                            }
                            _ => None,
                        })
                    {
                        self.coordinate_ranges.insert(def_id, vector_ranges);
                    } else {
                        self.coordinate_ranges.remove(&def_id);
                    }
                }
                stored
            }
            HirExpressionKind::Reduction {
                op,
                source,
                coordinates,
                group,
                binding,
                body,
                identity,
                ..
            } => self.evaluate_reduction(
                expr,
                *op,
                source,
                coordinates,
                group,
                binding,
                body,
                identity,
            ),
            HirExpressionKind::Scan {
                op,
                source,
                coordinates,
                group,
                filum: _,
                binding,
                body,
                ..
            } => self.evaluate_scan(expr, *op, source, coordinates, group, *binding, body),
            HirExpressionKind::Array(elements) => self.evaluate_array_elements(expr.id, elements),
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch: _,
                else_block,
            } => {
                self.evaluate(cond);
                let before = self.ranges.clone();
                self.ranges.clone_from(&before);
                self.refine_condition(cond, true);
                self.visit_block_stateful(then_block);
                let then_ranges = self.ranges.clone();
                self.ranges.clone_from(&before);
                self.refine_condition(cond, false);
                if let Some(else_block) = else_block {
                    self.visit_block_stateful(else_block);
                }
                let else_ranges = self.ranges.clone();
                self.ranges = merge_ranges(then_ranges, else_ranges);
                expr.ty.map_or_else(AbstractRange::unknown, |ty| {
                    AbstractRange::from_type(ty, self.types)
                })
            }
            HirExpressionKind::Dum(cond, body) => {
                self.evaluate(cond);
                let before = self.ranges.clone();
                self.refine_condition(cond, true);
                self.visit_block_stateful(body);
                let after = self.ranges.clone();
                self.ranges = merge_loop_ranges(before, after);
                expr.ty.map_or_else(AbstractRange::unknown, |ty| {
                    AbstractRange::from_type(ty, self.types)
                })
            }
            HirExpressionKind::Loop(body) => {
                let before = self.ranges.clone();
                self.visit_block_stateful(body);
                let after = self.ranges.clone();
                self.ranges = merge_loop_ranges(before, after);
                expr.ty.map_or_else(AbstractRange::unknown, |ty| {
                    AbstractRange::from_type(ty, self.types)
                })
            }
            HirExpressionKind::Itera {
                iter,
                coordinates,
                binding,
                body,
                ..
            } => {
                self.evaluate(iter);
                self.visit_tensor_loop(iter, coordinates, *binding, body, None);
                expr.ty.map_or_else(AbstractRange::unknown, |ty| {
                    AbstractRange::from_type(ty, self.types)
                })
            }
            HirExpressionKind::IteraLockstep {
                sources,
                coordinates,
                bindings,
                body,
                ..
            } => {
                for source in sources {
                    self.evaluate(source);
                }
                let Some(first) = sources.first() else {
                    return AbstractRange::unknown();
                };
                self.visit_tensor_loop_many(first, coordinates, bindings, body);
                expr.ty.map_or_else(AbstractRange::unknown, |ty| {
                    AbstractRange::from_type(ty, self.types)
                })
            }
            HirExpressionKind::IteraRange {
                ranges,
                bindings,
                body,
                ..
            } => {
                for range in ranges {
                    self.evaluate(range);
                }
                let before = self.ranges.clone();
                for (index, (def_id, _)) in bindings.iter().enumerate() {
                    let range = ranges
                        .get(index)
                        .map_or_else(AbstractRange::unknown, |range| self.range_binding(range));
                    self.check_value_intermediate(body.span, range.clone(), None);
                    self.ranges.insert(*def_id, range);
                    self.range_only_bindings.insert(*def_id);
                }
                self.visit_block_stateful(body);
                let after = self.ranges.clone();
                self.ranges = merge_loop_ranges(before, after);
                expr.ty.map_or_else(AbstractRange::unknown, |ty| {
                    AbstractRange::from_type(ty, self.types)
                })
            }
            HirExpressionKind::MethodCall(receiver, method, _, args, _) => {
                let receiver_range = self.evaluate(receiver);
                let argument_ranges: Vec<_> = args
                    .iter()
                    .map(|argument| self.evaluate(&argument.expr))
                    .collect();
                let method_name = self.interner.resolve(*method);
                if is_integer_value_type(receiver.ty, self.types)
                    && let Some(AbstractRange(Some(right))) = argument_ranges.first()
                    && let AbstractRange(Some(left)) = &receiver_range
                {
                    let range = match method_name {
                        "minimus" => Some(KernelValueRange {
                            minimum: minimum(left.minimum.clone(), right.minimum.clone()),
                            maximum: minimum(left.maximum.clone(), right.maximum.clone()),
                        }),
                        "maximus" => Some(KernelValueRange {
                            minimum: maximum(left.minimum.clone(), right.minimum.clone()),
                            maximum: maximum(left.maximum.clone(), right.maximum.clone()),
                        }),
                        _ => None,
                    };
                    if let Some(range) = range {
                        return AbstractRange(Some(range));
                    }
                }
                if matches!(
                    self.tensor_edge_policies.calls.get(&expr.id),
                    Some(ResolvedTensorEdgePolicy::Deferred { .. })
                ) {
                    self.failures
                        .push(KernelRequirementFailure::TensorEdgePolicyDeferred {
                            span: expr.span,
                        });
                }
                let tensor_element = receiver.ty.and_then(|ty| tensor_element(ty, self.types));
                if receiver.ty.is_some_and(|ty| is_tensor_type(ty, self.types))
                    && tensor_element
                        .is_some_and(|element| integer_slot(element, self.types).is_some())
                {
                    match method_name {
                        "divisa" => {
                            if !argument_ranges.first().is_some_and(excludes_zero) {
                                self.failures.push(
                                    KernelRequirementFailure::DivideByZeroPossible {
                                        span: expr.span,
                                    },
                                );
                            }
                        }
                        "addita" | "subtracta" | "multiplicata" | "nega" => {}
                        _ => {
                            return expr.ty.map_or_else(AbstractRange::unknown, |ty| {
                                AbstractRange::from_type(ty, self.types)
                            });
                        }
                    }
                    let result = match method_name {
                        "addita" => argument_ranges
                            .first()
                            .cloned()
                            .map(|right| binary_range(HirBinOp::Add, receiver_range, right)),
                        "subtracta" => argument_ranges
                            .first()
                            .cloned()
                            .map(|right| binary_range(HirBinOp::Sub, receiver_range, right)),
                        "multiplicata" => argument_ranges
                            .first()
                            .cloned()
                            .map(|right| binary_range(HirBinOp::Mul, receiver_range, right)),
                        "divisa" => argument_ranges
                            .first()
                            .cloned()
                            .map(|right| binary_range(HirBinOp::Div, receiver_range, right)),
                        "nega" => Some(unary_range(HirUnOp::Neg, receiver_range)),
                        _ => None,
                    }
                    .unwrap_or_else(AbstractRange::unknown);
                    let modular_ty = expr.ty.filter(|ty| is_modular_type(Some(*ty), self.types));
                    if modular_ty.is_none() || method_name == "divisa" {
                        self.check_value_intermediate(expr.span, result.clone(), expr.ty);
                    }
                    return if let Some(ty) = modular_ty {
                        AbstractRange::from_type(ty, self.types)
                    } else {
                        result
                    };
                }
                expr.ty.map_or_else(AbstractRange::unknown, |ty| {
                    AbstractRange::from_type(ty, self.types)
                })
            }
            HirExpressionKind::Field(object, member) => {
                let resolver = self.resolver;
                let imported = match &object.kind {
                    HirExpressionKind::Path(module) => resolver
                        .get_symbol(*module)
                        .filter(|namespace| namespace.kind == SymbolKind::Module)
                        .and_then(|namespace| {
                            resolver.namespace_file_const_value_canonical(namespace.name, *member)
                        })
                        .and_then(|value| match value {
                            crate::file_interface::InterfaceConstValue::Int(value) => Some(*value),
                            _ => None,
                        }),
                    _ => None,
                };
                if let Some(value) = imported {
                    self.evaluate(object);
                    return AbstractRange::exact(value);
                }
                walk_expr(self, expr);
                expr.ty.map_or_else(AbstractRange::unknown, |ty| {
                    AbstractRange::from_type(ty, self.types)
                })
            }
            _ => {
                walk_expr(self, expr);
                expr.ty.map_or_else(AbstractRange::unknown, |ty| {
                    AbstractRange::from_type(ty, self.types)
                })
            }
        }
    }

    /// A reduction. The term body runs with the coordinate binders in
    /// `[0, E)` exactly as a tensor loop does; the result range follows the
    /// operation, so a narrowing store of an index or a count is proven
    /// (the proof is the narrowing, F9 ruling 27) instead of read as a full
    /// 64-bit integer:
    ///
    /// - rank-1 `argmax` / `argmin`: `[0, E0 - 1]` (a symbolic `E0` becomes an
    ///   extent condition at the store, like a lane binder bound);
    /// - `count`: `[0, E0 * ... * Ek]`;
    /// - `all` / `any`: a bool, the type range;
    /// - every other operation (and a rank above one, a tuple, or a grouped
    ///   reduction): the result type's range, which proves nothing narrower.
    fn evaluate_reduction(
        &mut self,
        expr: &HirExpression,
        op: radix_hir::HirReductionOp,
        source: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        group: &Option<Box<HirExpression>>,
        binding: &Option<(DefId, Symbol)>,
        body: &Option<HirBlock>,
        identity: &Option<Box<HirExpression>>,
    ) -> AbstractRange {
        use radix_hir::HirReductionOp as Op;
        if let Some(identity) = identity {
            self.evaluate(identity);
        }
        if let (Some((binding, _)), Some(body)) = (binding, body) {
            // The term's `redde` returns the term, not the kernel result.
            let return_type = self.return_type.take();
            self.visit_tensor_loop(source, coordinates, *binding, body, group.as_deref());
            self.return_type = return_type;
        } else {
            self.evaluate(source);
            if let Some(group) = group {
                let before = self.ranges.clone();
                self.bind_tensor_coordinates(source, coordinates, expr.span);
                self.evaluate(group);
                self.ranges = before;
            }
        }
        let declared = expr.ty.map_or_else(AbstractRange::unknown, |ty| {
            AbstractRange::from_type(ty, self.types)
        });
        if group.is_some() {
            if op == Op::Count {
                let dims = source
                    .ty
                    .map_or_else(Vec::new, |ty| shape_dimensions(ty, self.types));
                let total =
                    dims.into_iter()
                        .try_fold(KernelRangeExpr::Constant(1), |total, dim| {
                            let extent = match evaluate_index_dimension(dim, self.types) {
                                Ok(Some(extent)) => KernelRangeExpr::Constant(i128::from(extent)),
                                Ok(None) => KernelRangeExpr::Shape(dim),
                                Err(_) => return None,
                            };
                            Some(multiply(total, extent))
                        });
                if let Some(element) = expr.ty.and_then(|ty| tensor_element(ty, self.types)) {
                    self.check_store(
                        expr.span,
                        element,
                        AbstractRange(total.map(|total| KernelValueRange {
                            minimum: KernelRangeExpr::Constant(0),
                            maximum: total,
                        })),
                    );
                }
            }
            return declared;
        }
        let dims = source
            .ty
            .map_or_else(Vec::new, |ty| shape_dimensions(ty, self.types));
        match op {
            Op::ArgMax | Op::ArgMin if dims.len() == 1 => coordinate_range(dims[0], self.types),
            Op::Count if !dims.is_empty() => {
                let mut total = KernelRangeExpr::Constant(1);
                for dim in dims {
                    let extent = match evaluate_index_dimension(dim, self.types) {
                        Ok(Some(extent)) => KernelRangeExpr::Constant(i128::from(extent)),
                        Ok(None) => KernelRangeExpr::Shape(dim),
                        Err(_) => return declared.clone(),
                    };
                    total = multiply(total, extent);
                }
                AbstractRange(Some(KernelValueRange {
                    minimum: KernelRangeExpr::Constant(0),
                    maximum: total,
                }))
            }
            _ => declared.clone(),
        }
    }

    fn evaluate_scan(
        &mut self,
        expr: &HirExpression,
        op: radix_hir::HirReductionOp,
        source: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        group: &Option<Box<HirExpression>>,
        binding: (DefId, Symbol),
        body: &HirBlock,
    ) -> AbstractRange {
        self.visit_tensor_loop(source, coordinates, binding.0, body, group.as_deref());
        let declared = expr.ty.map_or_else(AbstractRange::unknown, |ty| {
            AbstractRange::from_type(ty, self.types)
        });
        if op == radix_hir::HirReductionOp::Count {
            let dims = source
                .ty
                .map_or_else(Vec::new, |ty| shape_dimensions(ty, self.types));
            let total = dims
                .into_iter()
                .try_fold(KernelRangeExpr::Constant(1), |total, dim| {
                    let extent = match evaluate_index_dimension(dim, self.types) {
                        Ok(Some(extent)) => KernelRangeExpr::Constant(i128::from(extent)),
                        Ok(None) => KernelRangeExpr::Shape(dim),
                        Err(_) => return None,
                    };
                    Some(multiply(total, extent))
                });
            if let Some(element) = expr.ty.and_then(|ty| tensor_element(ty, self.types)) {
                self.check_store(
                    expr.span,
                    element,
                    AbstractRange(total.map(|total| KernelValueRange {
                        minimum: KernelRangeExpr::Constant(0),
                        maximum: total,
                    })),
                );
            }
        }
        declared
    }

    /// Evaluate an array literal's elements and record its per-axis ranges.
    fn evaluate_array_elements(
        &mut self,
        id: HirId,
        elements: &[HirArrayElement],
    ) -> AbstractRange {
        let mut axes = Vec::with_capacity(elements.len());
        let mut representable = true;
        for element in elements {
            match element {
                HirArrayElement::Expr(value) => axes.push(self.evaluate(value)),
                HirArrayElement::Spread(value) => {
                    self.evaluate(value);
                    representable = false;
                }
                HirArrayElement::Infer(_) => {
                    axes.push(AbstractRange::unknown());
                    representable = false;
                }
            }
        }
        if representable {
            self.expression_vectors.insert(id, axes);
        }
        AbstractRange::unknown()
    }

    /// Evaluate a tensor index. A coordinate vector (`t[[a, b, …]]`) is
    /// generated address arithmetic, not a stored kernel value, so its own
    /// element type places no value-width requirement on the kernel; the
    /// per-axis coordinates are still evaluated and the address range check
    /// decides whether the access is proven or needs a runtime guard.
    fn evaluate_index_coordinates(&mut self, index: &HirExpression) -> AbstractRange {
        match &index.kind {
            HirExpressionKind::Array(elements) => self.evaluate_array_elements(index.id, elements),
            _ => self.evaluate(index),
        }
    }

    /// Bound the exact row-major address arithmetic before its runtime guard.
    /// Signed coordinates may fault at runtime, but both ends of their linear
    /// address range must still fit the portable representation envelope.
    fn linear_tensor_index_range(
        &self,
        base: &HirExpression,
        index: &HirExpression,
    ) -> Option<AbstractRange> {
        if !matches!(index.kind, HirExpressionKind::Array(_)) {
            return None;
        }
        let axes = self.expression_vectors.get(&index.id)?;
        let mut base_ty = base.ty?;
        while let Type::Ref(_, target) = self.types.get(base_ty) {
            base_ty = *target;
        }
        let dims = shape_dimensions(base_ty, self.types);
        if axes.is_empty()
            || axes.len() != dims.len()
            || dims.iter().any(|dim| {
                matches!(
                    self.types.get_index(*dim),
                    IndexExpr::Unspecified | IndexExpr::Infer(_)
                )
            })
        {
            return None;
        }
        let mut minimum_index = KernelRangeExpr::Constant(0);
        let mut maximum_index = KernelRangeExpr::Constant(0);
        for (axis, range) in axes.iter().enumerate() {
            let range = range.0.as_ref()?;
            // A full unsigned scalar is not a signed device coordinate.
            // Keep its admission closed unless a binder/guard narrows it.
            if constant(&range.maximum).is_some_and(|maximum| maximum > i128::from(i32::MAX))
                || constant(&range.minimum).is_some_and(|minimum| minimum < i128::from(i32::MIN))
            {
                return None;
            }
            let stride = dims[axis + 1..]
                .iter()
                .fold(KernelRangeExpr::Constant(1), |acc, dim| {
                    let extent = match evaluate_index_dimension(*dim, self.types) {
                        Ok(Some(extent)) => KernelRangeExpr::Constant(i128::from(extent)),
                        _ => KernelRangeExpr::Shape(*dim),
                    };
                    multiply(acc, extent)
                });
            minimum_index = add(
                minimum_index,
                multiply(range.minimum.clone(), stride.clone()),
            );
            maximum_index = add(maximum_index, multiply(range.maximum.clone(), stride));
        }
        Some(AbstractRange(Some(KernelValueRange {
            minimum: minimum_index,
            maximum: maximum_index,
        })))
    }

    fn bind_tensor_coordinates(
        &mut self,
        iter: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        span: Span,
    ) {
        let dims = iter
            .ty
            .map_or_else(Vec::new, |ty| shape_dimensions(ty, self.types));
        for (axis, (def_id, _)) in coordinates.iter().enumerate() {
            let range = dims
                .get(axis)
                .copied()
                .map_or_else(AbstractRange::unknown, |dim| {
                    coordinate_range(dim, self.types)
                });
            if let Some(extent) = &range.0 {
                self.check_intermediate_with_bounds(
                    span,
                    AbstractRange(Some(KernelValueRange {
                        minimum: extent.minimum.clone(),
                        maximum: add(extent.maximum.clone(), KernelRangeExpr::Constant(1)),
                    })),
                    i128::from(i32::MIN),
                    i128::from(i32::MAX),
                );
            }
            self.ranges.insert(*def_id, range);
            self.range_only_bindings.insert(*def_id);
        }
    }

    fn visit_tensor_loop(
        &mut self,
        iter: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        binding: DefId,
        body: &HirBlock,
        group: Option<&HirExpression>,
    ) {
        let before = self.ranges.clone();
        // A single counted range lowers as Itera, not IteraRange. Its
        // binder has the interval's bounds, not a tensor element's range.
        if matches!(iter.kind, HirExpressionKind::Intervallum { .. }) {
            let range = self.range_binding(iter);
            self.check_value_intermediate(iter.span, range.clone(), None);
            self.ranges.insert(binding, range);
            self.range_only_bindings.insert(binding);
            self.visit_block_stateful(body);
            let after = self.ranges.clone();
            self.ranges = merge_loop_ranges(before, after);
            return;
        }
        self.bind_tensor_coordinates(iter, coordinates, body.span);
        if let Some(group) = group {
            self.evaluate(group);
        }
        if let Some(element) = iter.ty.and_then(|ty| tensor_element(ty, self.types)) {
            self.record_type(element, body.span);
            self.ranges
                .insert(binding, AbstractRange::from_type(element, self.types));
        } else {
            self.ranges.insert(binding, AbstractRange::unknown());
        }
        self.visit_block_stateful(body);
        let after = self.ranges.clone();
        self.ranges = merge_loop_ranges(before, after);
    }

    fn visit_tensor_loop_many(
        &mut self,
        first: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        bindings: &[(DefId, Symbol)],
        body: &HirBlock,
    ) {
        let before = self.ranges.clone();
        let dims = first
            .ty
            .map_or_else(Vec::new, |ty| shape_dimensions(ty, self.types));
        for (axis, (def_id, _)) in coordinates.iter().enumerate() {
            let range = dims
                .get(axis)
                .copied()
                .map_or_else(AbstractRange::unknown, |dim| {
                    coordinate_range(dim, self.types)
                });
            if let Some(extent) = &range.0 {
                self.check_intermediate_with_bounds(
                    body.span,
                    AbstractRange(Some(KernelValueRange {
                        minimum: extent.minimum.clone(),
                        maximum: add(extent.maximum.clone(), KernelRangeExpr::Constant(1)),
                    })),
                    i128::from(i32::MIN),
                    i128::from(i32::MAX),
                );
            }
            self.ranges.insert(*def_id, range);
            self.range_only_bindings.insert(*def_id);
        }
        for (def_id, _) in bindings {
            self.ranges.insert(*def_id, AbstractRange::unknown());
        }
        self.visit_block_stateful(body);
        let after = self.ranges.clone();
        self.ranges = merge_loop_ranges(before, after);
    }

    fn refine_condition(&mut self, cond: &HirExpression, truth: bool) {
        match &cond.kind {
            HirExpressionKind::Binary(HirBinOp::And, lhs, rhs) if truth => {
                self.refine_condition(lhs, true);
                self.refine_condition(rhs, true);
            }
            HirExpressionKind::Binary(HirBinOp::Or, lhs, rhs) if !truth => {
                self.refine_condition(lhs, false);
                self.refine_condition(rhs, false);
            }
            HirExpressionKind::Unary(HirUnOp::Not, inner) => self.refine_condition(inner, !truth),
            HirExpressionKind::Binary(op, lhs, rhs) => {
                self.refine_comparison(*op, lhs, rhs, truth);
            }
            _ => {}
        }
    }

    /// Homomorphic modular chains may flow to a wrapping store. A comparison,
    /// division, remainder, or right shift observes an exact value instead, so
    /// every modular arithmetic node in its operand tree must fit its carrier.
    fn check_observed_modular_intermediates(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Unary(op, operand) => {
                self.check_observed_modular_intermediates(operand);
                if matches!(op, HirUnOp::Neg | HirUnOp::BitNot)
                    && let Some(carrier) = modular_carrier_bounds(expr.ty, self.types)
                {
                    let exact = unary_range(*op, self.peek_range(operand));
                    self.check_value_intermediate_in(expr.span, exact, carrier, None);
                }
            }
            HirExpressionKind::Binary(op, lhs, rhs) => {
                self.check_observed_modular_intermediates(lhs);
                self.check_observed_modular_intermediates(rhs);
                if matches!(
                    *op,
                    HirBinOp::Add | HirBinOp::Sub | HirBinOp::Mul | HirBinOp::Shl | HirBinOp::Div
                ) && let Some(carrier) = modular_carrier_bounds(expr.ty, self.types)
                {
                    let exact = binary_range(*op, self.peek_range(lhs), self.peek_range(rhs));
                    self.check_value_intermediate_in(expr.span, exact, carrier, None);
                }
            }
            _ => {}
        }
    }

    fn range_binding(&self, expr: &HirExpression) -> AbstractRange {
        let HirExpressionKind::Intervallum {
            start,
            end,
            step,
            kind,
        } = &expr.kind
        else {
            return self.peek_range(expr);
        };
        if step
            .as_deref()
            .is_some_and(|step| crate::passes::const_int_value(step) != Some(1))
        {
            return AbstractRange::unknown();
        }
        let start = self.peek_range(start);
        let end = self.peek_range(end);
        let (Some(start), Some(end)) = (start.0, end.0) else {
            return AbstractRange::unknown();
        };
        let maximum = match kind {
            radix_hir::HirRangeKind::Exclusive => {
                subtract(end.maximum, KernelRangeExpr::Constant(1))
            }
            radix_hir::HirRangeKind::Inclusive => end.maximum,
        };
        AbstractRange(Some(KernelValueRange {
            minimum: start.minimum,
            maximum,
        }))
    }

    fn refine_comparison(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        truth: bool,
    ) {
        let (def_id, current, bound, normalized_op) =
            if let HirExpressionKind::Path(def_id) = lhs.kind {
                let current = self.ranges.get(&def_id).cloned().unwrap_or_else(|| {
                    lhs.ty.map_or_else(AbstractRange::unknown, |ty| {
                        AbstractRange::from_type(ty, self.types)
                    })
                });
                (def_id, current, self.peek_range(rhs), op)
            } else if let HirExpressionKind::Path(def_id) = rhs.kind {
                let current = self.ranges.get(&def_id).cloned().unwrap_or_else(|| {
                    rhs.ty.map_or_else(AbstractRange::unknown, |ty| {
                        AbstractRange::from_type(ty, self.types)
                    })
                });
                (
                    def_id,
                    current,
                    self.peek_range(lhs),
                    reverse_comparison(op),
                )
            } else {
                return;
            };
        let Some(current) = current.0 else {
            return;
        };
        let Some(bound) = bound.0 else {
            return;
        };
        let Some(next) = refine_interval(normalized_op, current, bound, truth) else {
            return;
        };
        self.ranges.insert(def_id, AbstractRange(Some(next)));
    }

    fn path_range(&self, def_id: DefId, ty: Option<TypeId>) -> AbstractRange {
        self.ranges
            .get(&def_id)
            .cloned()
            .or_else(|| {
                self.const_values
                    .get(&def_id)
                    .copied()
                    .map(AbstractRange::exact)
            })
            .or_else(|| ty.map(|ty| AbstractRange::from_type(ty, self.types)))
            .unwrap_or_else(AbstractRange::unknown)
    }

    fn peek_range(&self, expr: &HirExpression) -> AbstractRange {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.path_range(*def_id, expr.ty),
            HirExpressionKind::Literal(_) => crate::passes::const_int_value(expr)
                .map_or_else(AbstractRange::unknown, AbstractRange::exact),
            HirExpressionKind::Field(object, member) => {
                imported_int_const(self.resolver, object, *member).map_or_else(
                    || {
                        expr.ty.map_or_else(AbstractRange::unknown, |ty| {
                            AbstractRange::from_type(ty, self.types)
                        })
                    },
                    AbstractRange::exact,
                )
            }
            HirExpressionKind::Unary(op, operand) => unary_range(*op, self.peek_range(operand)),
            HirExpressionKind::Binary(op, lhs, rhs) => {
                binary_range(*op, self.peek_range(lhs), self.peek_range(rhs))
            }
            _ => expr.ty.map_or_else(AbstractRange::unknown, |ty| {
                AbstractRange::from_type(ty, self.types)
            }),
        }
    }

    /// The kernel parameter a path denotes, following `let` re-bindings of a
    /// parameter. `None` for any value the launch contract does not name.
    fn parameter_of(&self, def_id: DefId) -> Option<DefId> {
        if self.parameter_types.contains_key(&def_id) {
            return Some(def_id);
        }
        self.parameter_aliases.get(&def_id).copied()
    }

    /// A divisor proven positive by the launch contract: a size-class value
    /// *derived* from one kernel integer parameter — a `let T hf ← span`
    /// binding, optionally converted (`span ↦ int`) or scaled by a positive
    /// constant (`2 * hf`) — whose declared type has a finite positive maximum.
    /// A bare parameter reference is not a size-class derivation, so it stays
    /// an unproven divisor. Returns the parameter and its assumed maximum.
    fn positive_parameter_divisor(&self, expr: &HirExpression) -> Option<(DefId, i128)> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                let parameter = self.parameter_aliases.get(def_id).copied()?;
                // Use the parameter's own type, never a widened expression type:
                // `span ↦ int` stays bounded by `span`'s declared width.
                let declared = self.parameter_types.get(&parameter).copied()?;
                let range = type_range(declared, self.types)?;
                let maximum = constant(&range.maximum).filter(|maximum| *maximum > 0)?;
                Some((parameter, maximum))
            }
            HirExpressionKind::Conversio { source, .. }
            | HirExpressionKind::Widen {
                operand: source, ..
            } => self.positive_parameter_divisor(source),
            HirExpressionKind::Binary(HirBinOp::Mul, lhs, rhs) => {
                let scaled = |factor: &HirExpression, inner: &HirExpression| {
                    let factor =
                        crate::passes::const_int_value(factor).filter(|factor| *factor > 0)?;
                    let (parameter, maximum) = self.positive_parameter_divisor(inner)?;
                    Some((parameter, maximum.checked_mul(factor)?))
                };
                scaled(lhs, rhs).or_else(|| scaled(rhs, lhs))
            }
            _ => None,
        }
    }

    fn check_intermediate(&mut self, span: Span, range: AbstractRange) {
        self.check_intermediate_with_bounds(span, range, DEVICE_INTEGER_MIN, DEVICE_INTEGER_MAX);
    }

    /// An exact value intermediate: a result of integer arithmetic. It must lie
    /// in the host's 64-bit intermediate range on every target; one beyond the
    /// portable 32-bit envelope is also recorded, because it needs a 64-bit
    /// lane that no device emitter computes yet (each target row refuses it by
    /// name). Ranges that name launch extents or arrive with signed
    /// coordinates describe addressing, which stays on the 32-bit device
    /// envelope.
    fn check_value_intermediate(&mut self, span: Span, range: AbstractRange, ty: Option<TypeId>) {
        let own = ty.and_then(|ty| type_range(ty, self.types));
        self.check_value_intermediate_in(
            span,
            range,
            (DEVICE_INTEGER_MIN, DEVICE_INTEGER_MAX),
            own.as_ref(),
        );
    }

    /// [`Self::check_value_intermediate`] with the narrow-lane bounds outside
    /// of which a 32-bit lane would wrap.
    fn check_value_intermediate_in(
        &mut self,
        span: Span,
        range: AbstractRange,
        (narrow_min, narrow_max): (i128, i128),
        own: Option<&KernelValueRange>,
    ) {
        let Some(bounds) = range.0.as_ref() else {
            self.check_intermediate(span, range);
            return;
        };
        if self.has_signed_coordinate_input || contains_shape(bounds) {
            self.check_intermediate(span, range);
            return;
        }
        let minimum = symbolic_bounds(&bounds.minimum);
        let maximum = symbolic_bounds(&bounds.maximum);
        let out_of_range = constant(&bounds.minimum).is_some_and(|value| value < WIDE_INTEGER_MIN)
            || constant(&bounds.maximum).is_some_and(|value| value > WIDE_INTEGER_MAX)
            || minimum
                .minimum
                .is_some_and(|value| value > WIDE_INTEGER_MAX)
            || maximum
                .maximum
                .is_some_and(|value| value < WIDE_INTEGER_MIN);
        if out_of_range {
            if let Some(range) = range.0 {
                self.failures
                    .push(KernelRequirementFailure::IntermediateOutOfRange { span, range });
            }
            return;
        }
        let fits = |low: i128, high: i128| {
            minimum.minimum.is_some_and(|value| value >= low)
                && maximum.maximum.is_some_and(|value| value <= high)
        };
        if !fits(WIDE_INTEGER_MIN, WIDE_INTEGER_MAX) {
            self.failures
                .push(KernelRequirementFailure::UnsupportedIntegerRange { span });
        } else if !fits(narrow_min, narrow_max)
            && let Some(range) = range.0
        {
            let native = own.is_some_and(|own| {
                matches!(
                    (constant(&own.minimum), constant(&own.maximum)),
                    (Some(low), Some(high)) if fits(low, high)
                )
            });
            self.wide_intermediates.push((span, range, native));
        }
    }

    /// A `d64` intermediate is admitted unless compile-time constants prove
    /// that one of its carrier operations overflows. Bounds based on the full
    /// stored `i64` range are deliberately not a refusal: the Metal helpers
    /// propagate the carrier-overflow flag to the existing device fault slot.
    fn check_d64_intermediate(&mut self, expr: &HirExpression) {
        if !matches!(self.constant_d64(expr), Some(D64Constant::Overflow)) {
            return;
        }
        self.failures
            .push(KernelRequirementFailure::IntermediateOutOfRange {
                span: expr.span,
                range: KernelValueRange {
                    minimum: KernelRangeExpr::Constant(-i128::MAX),
                    maximum: KernelRangeExpr::Constant(i128::MAX),
                },
            });
    }

    /// Evaluate only the constant subset needed to prove a `d64` carrier
    /// overflow. `None` means the expression depends on a run-time value (or
    /// uses an operation not folded here); it must be checked by the device.
    fn constant_d64(&self, expr: &HirExpression) -> Option<D64Constant> {
        if !is_d64_type(expr.ty, self.types) {
            return crate::passes::const_int_value(expr).map(|value| D64Constant::Value(value, 0));
        }
        match &expr.kind {
            HirExpressionKind::Literal(radix_hir::HirLiteral::Int(value)) => {
                Some(decimal_constant(i128::from(*value)))
            }
            HirExpressionKind::Literal(radix_hir::HirLiteral::BigInt(_)) => {
                crate::passes::const_int_value(expr).map(decimal_constant)
            }
            HirExpressionKind::Literal(radix_hir::HirLiteral::FloatText { text, .. }) => {
                let (mantissa, exponent) = radix_types::numeric_width::decimal_text_to_scaled(
                    self.interner.resolve(*text),
                )?;
                let shift = exponent.checked_add(i32::try_from(D64_STORE_SCALE).ok()?)?;
                let scaled = if shift >= 0 {
                    mantissa.checked_mul(10i128.checked_pow(u32::try_from(shift).ok()?)?)?
                } else {
                    let divisor = 10i128.checked_pow(shift.unsigned_abs())?;
                    if mantissa % divisor != 0 {
                        return None;
                    }
                    mantissa / divisor
                };
                Some(D64Constant::Value(scaled, D64_STORE_SCALE))
            }
            HirExpressionKind::Unary(HirUnOp::Neg, operand) => match self.constant_d64(operand)? {
                D64Constant::Value(value, scale) => value
                    .checked_neg()
                    .map_or(Some(D64Constant::Overflow), |value| {
                        Some(D64Constant::Value(value, scale))
                    }),
                D64Constant::Overflow => Some(D64Constant::Overflow),
            },
            HirExpressionKind::Binary(op, lhs, rhs)
                if matches!(op, HirBinOp::Add | HirBinOp::Sub | HirBinOp::Mul) =>
            {
                let lhs = self.constant_d64(lhs)?;
                let rhs = self.constant_d64(rhs)?;
                let (D64Constant::Value(lhs, lhs_scale), D64Constant::Value(rhs, rhs_scale)) =
                    (lhs, rhs)
                else {
                    return Some(D64Constant::Overflow);
                };
                match op {
                    HirBinOp::Add | HirBinOp::Sub => {
                        let scale = lhs_scale.max(rhs_scale);
                        let Some(lhs) = scale_d64_constant(lhs, lhs_scale, scale) else {
                            return Some(D64Constant::Overflow);
                        };
                        let Some(rhs) = scale_d64_constant(rhs, rhs_scale, scale) else {
                            return Some(D64Constant::Overflow);
                        };
                        let value = if *op == HirBinOp::Add {
                            lhs.checked_add(rhs)
                        } else {
                            lhs.checked_sub(rhs)
                        };
                        Some(value.map_or(D64Constant::Overflow, |value| {
                            normalize_d64_constant(value, scale)
                        }))
                    }
                    HirBinOp::Mul => {
                        let Some(scale) = lhs_scale.checked_add(rhs_scale) else {
                            return Some(D64Constant::Overflow);
                        };
                        Some(lhs.checked_mul(rhs).map_or(D64Constant::Overflow, |value| {
                            normalize_d64_constant(value, scale)
                        }))
                    }
                    _ => None,
                }
            }
            HirExpressionKind::Conversio { source, .. } => {
                if is_d64_type(source.ty, self.types) {
                    self.constant_d64(source)
                } else {
                    crate::passes::const_int_value(source).map(decimal_constant)
                }
            }
            HirExpressionKind::Verte { source, .. } => self.constant_d64(source),
            _ => None,
        }
    }

    fn check_intermediate_with_bounds(
        &mut self,
        span: Span,
        range: AbstractRange,
        carrier_min: i128,
        carrier_max: i128,
    ) {
        let Some(range) = range.0 else {
            self.failures
                .push(KernelRequirementFailure::UnsupportedIntegerRange { span });
            return;
        };
        let minimum = symbolic_bounds(&range.minimum);
        let maximum = symbolic_bounds(&range.maximum);
        let allowed_min = carrier_min.max(DEVICE_INTEGER_MIN);
        let allowed_max = carrier_max.min(if self.has_signed_coordinate_input {
            2_147_483_647
        } else {
            DEVICE_INTEGER_MAX
        });
        let fixed_endpoint_out_of_range = constant(&range.minimum)
            .is_some_and(|minimum| minimum < allowed_min)
            || constant(&range.maximum).is_some_and(|maximum| maximum > allowed_max);
        let impossible = minimum.minimum.is_some_and(|minimum| minimum > allowed_max)
            || maximum.maximum.is_some_and(|maximum| maximum < allowed_min);
        if impossible || fixed_endpoint_out_of_range {
            self.failures
                .push(KernelRequirementFailure::IntermediateOutOfRange { span, range });
        } else if minimum
            .minimum
            .is_some_and(|minimum| minimum >= allowed_min)
            && maximum
                .maximum
                .is_some_and(|maximum| maximum <= allowed_max)
        {
            // Every possible symbolic value is contained in the device envelope.
        } else if contains_shape(&range) {
            self.conditions.push(KernelExtentCondition {
                span,
                range,
                minimum: allowed_min,
                maximum: allowed_max,
            });
        } else {
            self.failures
                .push(KernelRequirementFailure::UnsupportedIntegerRange { span });
        }
    }

    fn check_store(&mut self, span: Span, slot: TypeId, value: AbstractRange) -> AbstractRange {
        if is_numeric_container(slot, self.types) {
            return value;
        }
        let Some((width, slot_range, is_wrapping, is_saturating)) = integer_slot(slot, self.types)
        else {
            return value;
        };
        if is_wrapping || is_saturating {
            return AbstractRange(Some(slot_range));
        }
        let Some(value_range) = value.0.as_ref() else {
            self.trapping_stores
                .push(KernelTrappingStore { span, width });
            self.failures
                .push(KernelRequirementFailure::OptionFRequired { span, width });
            return AbstractRange(Some(slot_range));
        };
        let slot_min = constant(&slot_range.minimum).expect("integer slot has a constant minimum");
        let slot_max = constant(&slot_range.maximum).expect("integer slot has a constant maximum");
        let minimum = symbolic_bounds(&value_range.minimum);
        let maximum = symbolic_bounds(&value_range.maximum);
        let fixed_endpoint_out_of_range = constant(&value_range.minimum)
            .is_some_and(|minimum| minimum < slot_min)
            || constant(&value_range.maximum).is_some_and(|maximum| maximum > slot_max);
        let impossible = minimum.minimum.is_some_and(|minimum| minimum > slot_max)
            || maximum.maximum.is_some_and(|maximum| maximum < slot_min);
        let proven = minimum.minimum.is_some_and(|minimum| minimum >= slot_min)
            && maximum.maximum.is_some_and(|maximum| maximum <= slot_max);
        if impossible || fixed_endpoint_out_of_range {
            self.failures
                .push(KernelRequirementFailure::StoreOutOfRange {
                    span,
                    range: value_range.clone(),
                });
        } else if proven {
            // The full abstract interval fits the destination cell.
            return AbstractRange(Some(value_range.clone()));
        } else if contains_shape(value_range) {
            if !could_fit_between(minimum, maximum, slot_min, slot_max) {
                self.failures
                    .push(KernelRequirementFailure::StoreOutOfRange {
                        span,
                        range: value_range.clone(),
                    });
                return AbstractRange(Some(slot_range));
            }
            self.conditions.push(KernelExtentCondition {
                span,
                range: value_range.clone(),
                minimum: slot_min,
                maximum: slot_max,
            });
            return AbstractRange(Some(value_range.clone()));
        } else {
            self.trapping_stores
                .push(KernelTrappingStore { span, width });
            self.failures
                .push(KernelRequirementFailure::OptionFRequired { span, width });
        }
        AbstractRange(Some(slot_range))
    }

    /// Size-name reads. A `size` parameter named in a kernel body reads as an
    /// `i32` (the lane-coordinate carrier), and the launch contract owns its
    /// value: it is the symbolic extent of the tensor axis the parameter
    /// names, so arithmetic on it is bounded by a launch-time extent condition
    /// rather than by the carrier's full range. A size parameter no tensor
    /// parameter's shape names has no axis to read and keeps the carrier range.
    fn bind_size_values(&mut self, function: &HirFunction) {
        for type_param in &function.type_params {
            if type_param.kind != radix_syntax::GenericParamKind::Magnitudo {
                continue;
            }
            let axis = function
                .params
                .iter()
                .flat_map(|param| shape_dimensions(param.ty, self.types))
                .find(|axis| {
                    matches!(self.types.get_index(*axis), IndexExpr::Param(name) if *name == type_param.name)
                });
            let Some(axis) = axis else {
                continue;
            };
            let extent = KernelRangeExpr::Shape(axis);
            self.ranges.insert(
                type_param.def_id,
                AbstractRange(Some(KernelValueRange {
                    minimum: extent.clone(),
                    maximum: extent,
                })),
            );
        }
    }

    /// A captured `size` binding that a captured tensor's shape names reads as
    /// that axis's symbolic extent, as a `size` type parameter does in a
    /// kernel (the launch contract owns its value).
    fn bind_captured_size_values(&mut self, captured: &[(DefId, TypeId)]) {
        for (_, ty) in captured {
            for axis in shape_dimensions(*ty, self.types) {
                let IndexExpr::Runtime(radix_types::RuntimeExtentId::ScalarBinding(binding)) =
                    self.types.get_index(axis)
                else {
                    continue;
                };
                let extent = KernelRangeExpr::Shape(axis);
                self.ranges.insert(
                    *binding,
                    AbstractRange(Some(KernelValueRange {
                        minimum: extent.clone(),
                        maximum: extent,
                    })),
                );
                // The binding is a launch-time extent, not an i64 cell.
                self.range_only_bindings.insert(*binding);
            }
        }
    }

    fn record_type(&mut self, ty: TypeId, span: Span) {
        let mut seen_types = FxHashSet::default();
        let mut seen_defs = FxHashSet::default();
        self.record_type_inner(ty, span, &mut seen_types, &mut seen_defs);
    }

    fn record_type_inner(
        &mut self,
        ty: TypeId,
        span: Span,
        seen_types: &mut FxHashSet<TypeId>,
        seen_defs: &mut FxHashSet<DefId>,
    ) {
        if !seen_types.insert(ty) {
            return;
        }
        match self.types.get(ty) {
            Type::Primitive(Primitive::Numerus) => self.widths.push(KernelWidthRequirement {
                span,
                primitive: Primitive::Numerus,
                width: NumericWidth::I64,
                policy: KernelNumericPolicy::Exact,
            }),
            Type::Primitive(Primitive::Fractus) => self.widths.push(KernelWidthRequirement {
                span,
                primitive: Primitive::Fractus,
                width: NumericWidth::F64,
                policy: KernelNumericPolicy::Exact,
            }),
            Type::SizedNumeric(primitive, width) => self.widths.push(KernelWidthRequirement {
                span,
                primitive: *primitive,
                width: *width,
                policy: KernelNumericPolicy::Exact,
            }),
            Type::ModularWord(width) => self.widths.push(KernelWidthRequirement {
                span,
                primitive: Primitive::Numerus,
                width: *width,
                policy: KernelNumericPolicy::Wrapping,
            }),
            Type::SaturatingWord(width) => self.widths.push(KernelWidthRequirement {
                span,
                primitive: Primitive::Numerus,
                width: *width,
                policy: KernelNumericPolicy::Saturating,
            }),
            Type::PolicyFloat(_, width) => self.widths.push(KernelWidthRequirement {
                span,
                primitive: Primitive::Fractus,
                width: *width,
                policy: KernelNumericPolicy::PolicyFloat,
            }),
            Type::Tensor(element, _)
            | Type::Vector(element, _)
            | Type::Matrix(element, _)
            | Type::Sparsa(element, _)
            | Type::Array(element)
            | Type::BoundedArray { element, .. }
            | Type::Option(element)
            | Type::Ref(_, element)
            | Type::Alias(_, element) => {
                self.record_type_inner(*element, span, seen_types, seen_defs);
            }
            Type::Struct(def) | Type::Enum(def) => {
                if seen_defs.insert(*def)
                    && let Some(members) = self.facts.fields.get(def)
                {
                    for member in members {
                        self.record_type_inner(*member, span, seen_types, seen_defs);
                    }
                }
            }
            Type::Applied(base, args) => {
                self.record_type_inner(*base, span, seen_types, seen_defs);
                for arg in args {
                    if let radix_types::AppliedArg::Type(member) = arg {
                        self.record_type_inner(*member, span, seen_types, seen_defs);
                    }
                }
            }
            Type::Union(members) | Type::Intersection(members) | Type::Tuple(members) => {
                for member in members {
                    self.record_type_inner(*member, span, seen_types, seen_defs);
                }
            }
            Type::Record(fields) => {
                for member in fields.values() {
                    self.record_type_inner(*member, span, seen_types, seen_defs);
                }
            }
            _ => {}
        }
    }

    /// An untyped float constant (or arithmetic over only such constants) has
    /// no width of its own: it joins the width of the float it meets, so it
    /// places no width requirement on the kernel. Only an expression still
    /// typed as the untyped `fractus` qualifies; an explicit conversion to a
    /// sized float carries that float's type and is recorded as usual.
    ///
    /// A list literal over untyped floats (`[x, 2.0, 3.0] ↦ vf32[3]`) is the
    /// same case one level up: its elements carry their own requirements and
    /// the conversion target carries the vector's width, so the literal's
    /// own array-of-`fractus` type adds none.
    fn float_constant_only(&self, expr: &HirExpression, ty: TypeId) -> bool {
        match (&expr.kind, self.types.get(ty)) {
            (
                HirExpressionKind::Array(elements),
                Type::Array(element) | Type::BoundedArray { element, .. },
            ) => {
                matches!(
                    self.types.get(*element),
                    Type::Primitive(Primitive::Fractus)
                ) && elements
                    .iter()
                    .all(|element| matches!(element, HirArrayElement::Expr(_)))
            }
            (_, Type::Primitive(Primitive::Fractus)) => self.untyped_float_expression(expr),
            _ => false,
        }
    }

    fn untyped_float_expression(&self, expr: &HirExpression) -> bool {
        let untyped = |operand: &HirExpression| {
            operand
                .ty
                .is_some_and(|ty| matches!(self.types.get(ty), Type::Primitive(Primitive::Fractus)))
                && self.untyped_float_expression(operand)
        };
        match &expr.kind {
            HirExpressionKind::Literal(
                radix_hir::HirLiteral::Float(_) | radix_hir::HirLiteral::FloatText { .. },
            ) => true,
            HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, inner) => untyped(inner),
            HirExpressionKind::Binary(_, lhs, rhs) => untyped(lhs) && untyped(rhs),
            _ => false,
        }
    }

    /// A rank-1 `argmax` / `argmin` or a `count`: the result is an index or a
    /// tally whose range the reduction arm of [`Self::evaluate`] bounds by the
    /// source extent, so its `int` type is not a 64-bit device value; only the
    /// store that consumes it names a width.
    fn range_proven_reduction(&self, expr: &HirExpression) -> bool {
        use radix_hir::HirReductionOp as Op;
        let HirExpressionKind::Reduction {
            op,
            source,
            group: None,
            ..
        } = &expr.kind
        else {
            return false;
        };
        let rank = source
            .ty
            .map_or(0, |ty| shape_dimensions(ty, self.types).len());
        match op {
            Op::ArgMax | Op::ArgMin => rank == 1,
            Op::Count => rank > 0,
            _ => false,
        }
    }

    fn range_proven_scan(&self, expr: &HirExpression) -> bool {
        let HirExpressionKind::Scan { op, source, .. } = &expr.kind else {
            return false;
        };
        *op == radix_hir::HirReductionOp::Count
            && source
                .ty
                .is_some_and(|ty| !shape_dimensions(ty, self.types).is_empty())
    }

    fn range_only_expression(&self, expr: &HirExpression) -> bool {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.range_only_bindings.contains(def_id),
            HirExpressionKind::Literal(
                radix_hir::HirLiteral::Int(_) | radix_hir::HirLiteral::BigInt(_),
            ) => true,
            HirExpressionKind::Unary(_, inner)
            | HirExpressionKind::Conversio { source: inner, .. }
            | HirExpressionKind::Widen { operand: inner, .. }
            | HirExpressionKind::Verte { source: inner, .. } => self.range_only_expression(inner),
            HirExpressionKind::Binary(_, lhs, rhs) => {
                self.range_only_expression(lhs) && self.range_only_expression(rhs)
            }
            HirExpressionKind::Index(base, _) => matches!(
                base.kind,
                HirExpressionKind::Path(def_id) if self.coordinate_ranges.contains_key(&def_id)
            ),
            HirExpressionKind::Array(elements) => elements.iter().all(|element| match element {
                HirArrayElement::Expr(expr) => self.range_only_expression(expr),
                HirArrayElement::Spread(_) | HirArrayElement::Infer(_) => false,
            }),
            // Shape metadata passed to a tensor recipe is not an i64 value
            // cell. The receiver's extents carry its launch requirements.
            HirExpressionKind::MethodCall(receiver, method, _, args, _) => {
                args.is_empty()
                    && self.interner.resolve(*method) == "magnitudines"
                    && receiver.ty.is_some_and(|ty| is_tensor_type(ty, self.types))
            }
            HirExpressionKind::Reduction { .. } => self.range_proven_reduction(expr),
            HirExpressionKind::Scan { .. } => self.range_proven_scan(expr),
            // An imported module constant folds to its interface value: it is a
            // compile-time number, not an i64 cell on the device.
            HirExpressionKind::Field(object, member) => match &object.kind {
                HirExpressionKind::Path(module) => self
                    .resolver
                    .get_symbol(*module)
                    .filter(|namespace| namespace.kind == SymbolKind::Module)
                    .and_then(|namespace| {
                        self.resolver
                            .namespace_file_const_value_canonical(namespace.name, *member)
                    })
                    .is_some(),
                _ => false,
            },
            _ => false,
        }
    }
}

impl HirVisitor for BodyFacts<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        self.evaluate(expr);
    }

    fn visit_block(&mut self, block: &HirBlock) {
        self.visit_block_stateful(block);
    }

    fn visit_local(&mut self, local: &HirLocal) {
        self.visit_local_stateful(local);
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        self.visit_statement_stateful(stmt);
    }
}

fn type_range(ty: TypeId, types: &TypeTable) -> Option<KernelValueRange> {
    let ty = numeric_scalar_type(ty, types);
    let width = match types.get(ty) {
        Type::Primitive(Primitive::Numerus) => NumericWidth::I64,
        Type::SizedNumeric(Primitive::Numerus, width)
        | Type::ModularWord(width)
        | Type::SaturatingWord(width) => *width,
        Type::Alias(_, target) => return type_range(*target, types),
        _ => return None,
    };
    let (minimum, maximum) = width.integer_range()?;
    Some(KernelValueRange {
        minimum: KernelRangeExpr::Constant(minimum),
        maximum: KernelRangeExpr::Constant(maximum),
    })
}

fn integer_slot(
    ty: TypeId,
    types: &TypeTable,
) -> Option<(NumericWidth, KernelValueRange, bool, bool)> {
    let ty = numeric_scalar_type(ty, types);
    let (width, wrapping, saturating) = match types.get(ty) {
        Type::Primitive(Primitive::Numerus) => (NumericWidth::I64, false, false),
        Type::SizedNumeric(Primitive::Numerus, width) => (*width, false, false),
        Type::ModularWord(width) => (*width, true, false),
        Type::SaturatingWord(width) => (*width, false, true),
        Type::Alias(_, target) => return integer_slot(*target, types),
        _ => return None,
    };
    let (minimum, maximum) = width.integer_range()?;
    Some((
        width,
        KernelValueRange {
            minimum: KernelRangeExpr::Constant(minimum),
            maximum: KernelRangeExpr::Constant(maximum),
        },
        wrapping,
        saturating,
    ))
}

fn is_integer_value_type(ty: Option<TypeId>, types: &TypeTable) -> bool {
    ty.is_some_and(|ty| integer_slot(ty, types).is_some())
}

fn is_numeric_container(ty: TypeId, types: &TypeTable) -> bool {
    match types.get(ty) {
        Type::Tensor(..)
        | Type::Vector(..)
        | Type::Matrix(..)
        | Type::Sparsa(..)
        | Type::Array(..)
        | Type::BoundedArray { .. } => true,
        Type::Alias(_, target) | Type::Ref(_, target) => is_numeric_container(*target, types),
        _ => false,
    }
}

fn is_range_narrowable_type(ty: TypeId, types: &TypeTable) -> bool {
    match types.get(ty) {
        Type::Primitive(Primitive::Numerus) => true,
        Type::Tensor(element, _)
        | Type::Vector(element, _)
        | Type::Matrix(element, _)
        | Type::Sparsa(element, _)
        | Type::Array(element)
        | Type::BoundedArray { element, .. }
        | Type::Ref(_, element)
        | Type::Alias(_, element) => is_range_narrowable_type(*element, types),
        _ => false,
    }
}

fn is_modular_unary(ty: Option<TypeId>, op: HirUnOp, types: &TypeTable) -> bool {
    ty.is_some_and(|ty| {
        matches!(
            (types.get(numeric_scalar_type(ty, types)), op),
            (Type::ModularWord(_), HirUnOp::Neg | HirUnOp::BitNot)
        )
    })
}

fn is_modular_binary(ty: Option<TypeId>, op: HirBinOp, types: &TypeTable) -> bool {
    if !ty.is_some_and(|ty| {
        matches!(
            types.get(numeric_scalar_type(ty, types)),
            Type::ModularWord(_)
        )
    }) {
        return false;
    }
    matches!(
        op,
        HirBinOp::Add
            | HirBinOp::Sub
            | HirBinOp::Mul
            | HirBinOp::Div
            | HirBinOp::Mod
            | HirBinOp::BitAnd
            | HirBinOp::BitOr
            | HirBinOp::BitXor
            | HirBinOp::Shl
            | HirBinOp::Shr
    )
}

fn is_modular_type(ty: Option<TypeId>, types: &TypeTable) -> bool {
    ty.is_some_and(|ty| {
        matches!(
            types.get(numeric_scalar_type(ty, types)),
            Type::ModularWord(_)
        )
    })
}

/// The narrow-lane carrier of a wrapping word whose exact intermediates are
/// checked when an observer reads them. A wrapping word reduces once, at the
/// store; the exact value in between lives in the host's 64-bit intermediate
/// range (ruling 17), so an intermediate outside the word's own carrier needs
/// a 64-bit lane. A 64-bit word has no narrower carrier.
fn modular_carrier_bounds(ty: Option<TypeId>, types: &TypeTable) -> Option<(i128, i128)> {
    let ty = numeric_scalar_type(ty?, types);
    match types.get(ty) {
        Type::ModularWord(NumericWidth::I32) => Some((i128::from(i32::MIN), i128::from(i32::MAX))),
        Type::ModularWord(NumericWidth::U32) => Some((0, i128::from(u32::MAX))),
        Type::ModularWord(NumericWidth::I64 | NumericWidth::U64) => {
            Some((WIDE_INTEGER_MIN, WIDE_INTEGER_MAX))
        }
        _ => None,
    }
}

fn is_tensor_type(ty: TypeId, types: &TypeTable) -> bool {
    match types.get(ty) {
        Type::Tensor(..) => true,
        Type::Alias(_, target) | Type::Ref(_, target) => is_tensor_type(*target, types),
        _ => false,
    }
}

/// Numeric operators on tensor-like values execute elementwise. Project the
/// container type to its element before deriving the lane range, storage
/// policy, or modular arithmetic family. Keeping this projection in one place
/// makes a `TensorDiv` divisor use the range of its elements rather than
/// treating the whole tensor as an untyped value.
fn numeric_scalar_type(ty: TypeId, types: &TypeTable) -> TypeId {
    match types.get(ty) {
        Type::Alias(_, target) => numeric_scalar_type(*target, types),
        Type::Tensor(element, _)
        | Type::Vector(element, _)
        | Type::Matrix(element, _)
        | Type::Sparsa(element, _)
        | Type::Ref(_, element) => numeric_scalar_type(*element, types),
        _ => ty,
    }
}

fn is_non_homomorphic_observer(op: HirBinOp) -> bool {
    matches!(
        op,
        HirBinOp::Eq
            | HirBinOp::NotEq
            | HirBinOp::ApproxEq
            | HirBinOp::ApproxNotEq
            | HirBinOp::FuzzyEq
            | HirBinOp::FuzzyNotEq
            | HirBinOp::StrictEq
            | HirBinOp::StrictNotEq
            | HirBinOp::Lt
            | HirBinOp::LtEq
            | HirBinOp::Gt
            | HirBinOp::GtEq
            | HirBinOp::InRange
            | HirBinOp::Between
            | HirBinOp::NotInRange
            | HirBinOp::NotBetween
            | HirBinOp::Div
            | HirBinOp::Mod
            | HirBinOp::Shr
    )
}

fn is_integer_value_operator(op: HirBinOp) -> bool {
    matches!(
        op,
        HirBinOp::Add
            | HirBinOp::Sub
            | HirBinOp::Mul
            | HirBinOp::Div
            | HirBinOp::Mod
            | HirBinOp::BitAnd
            | HirBinOp::BitOr
            | HirBinOp::BitXor
            | HirBinOp::Shl
            | HirBinOp::Shr
    )
}

fn unary_range(op: HirUnOp, value: AbstractRange) -> AbstractRange {
    let Some(value) = value.0 else {
        return AbstractRange::unknown();
    };
    match op {
        HirUnOp::Neg => AbstractRange(Some(KernelValueRange {
            minimum: negate(value.maximum),
            maximum: negate(value.minimum),
        })),
        HirUnOp::BitNot => AbstractRange(Some(KernelValueRange {
            minimum: subtract(negate(value.maximum), KernelRangeExpr::Constant(1)),
            maximum: subtract(negate(value.minimum), KernelRangeExpr::Constant(1)),
        })),
        HirUnOp::Not => AbstractRange::unknown(),
    }
}

fn binary_range(op: HirBinOp, lhs: AbstractRange, rhs: AbstractRange) -> AbstractRange {
    let (Some(lhs), Some(rhs)) = (lhs.0, rhs.0) else {
        return AbstractRange::unknown();
    };
    let result = match op {
        HirBinOp::Add => KernelValueRange {
            minimum: add(lhs.minimum, rhs.minimum),
            maximum: add(lhs.maximum, rhs.maximum),
        },
        HirBinOp::Sub => KernelValueRange {
            minimum: subtract(lhs.minimum, rhs.maximum),
            maximum: subtract(lhs.maximum, rhs.minimum),
        },
        HirBinOp::Mul => {
            let values = [
                multiply(lhs.minimum.clone(), rhs.minimum.clone()),
                multiply(lhs.minimum, rhs.maximum.clone()),
                multiply(lhs.maximum.clone(), rhs.minimum),
                multiply(lhs.maximum, rhs.maximum),
            ];
            let [first, rest @ ..] = &values;
            KernelValueRange {
                minimum: rest.iter().cloned().fold(first.clone(), minimum),
                maximum: rest.iter().cloned().fold(first.clone(), maximum),
            }
        }
        HirBinOp::Div | HirBinOp::Shr => {
            let divisors = if op == HirBinOp::Shr {
                let Some(shift) =
                    constant(&rhs.minimum).filter(|shift| *shift >= 0 && *shift < 127)
                else {
                    return AbstractRange::unknown();
                };
                if constant(&rhs.maximum) != Some(shift) {
                    return AbstractRange::unknown();
                }
                let factor = KernelRangeExpr::Constant(1_i128 << shift as u32);
                [factor.clone(), factor.clone()]
            } else {
                let (Some(divisor_min), Some(divisor_max)) =
                    (constant(&rhs.minimum), constant(&rhs.maximum))
                else {
                    return AbstractRange::unknown();
                };
                if divisor_min <= 0 && divisor_max >= 0 {
                    return AbstractRange::unknown();
                }
                [
                    KernelRangeExpr::Constant(divisor_min),
                    KernelRangeExpr::Constant(divisor_max),
                ]
            };
            let values = [
                floor_divide(lhs.minimum.clone(), divisors[0].clone()),
                floor_divide(lhs.minimum, divisors[1].clone()),
                floor_divide(lhs.maximum.clone(), divisors[0].clone()),
                floor_divide(lhs.maximum, divisors[1].clone()),
            ];
            let Some(values) = values.into_iter().collect::<Option<Vec<_>>>() else {
                return AbstractRange::unknown();
            };
            let mut values = values.into_iter();
            let Some(first) = values.next() else {
                return AbstractRange::unknown();
            };
            let (minimum_value, maximum_value) = values
                .fold((first.clone(), first), |(lo, hi), value| {
                    (minimum(lo, value.clone()), maximum(hi, value))
                });
            KernelValueRange {
                minimum: minimum_value,
                maximum: maximum_value,
            }
        }
        HirBinOp::Mod => {
            // A non-negative numerator over a symbolic extent divisor (a
            // captured `size` binding, a tensor axis) is bounded by the
            // extent: a floored remainder by `d > 0` lies in `[0, d - 1]`.
            // A zero divisor is a separate failure; it never yields a value.
            if (constant(&rhs.minimum).is_none() || constant(&rhs.maximum).is_none())
                && symbolic_bounds(&lhs.minimum)
                    .minimum
                    .is_some_and(|m| m >= 0)
                && symbolic_bounds(&rhs.minimum)
                    .minimum
                    .is_some_and(|m| m >= 0)
                && contains_shape(&rhs)
            {
                return AbstractRange(Some(KernelValueRange {
                    minimum: KernelRangeExpr::Constant(0),
                    maximum: subtract(rhs.maximum, KernelRangeExpr::Constant(1)),
                }));
            }
            let Some(divisor_min) = constant(&rhs.minimum) else {
                return AbstractRange::unknown();
            };
            let Some(divisor_max) = constant(&rhs.maximum) else {
                return AbstractRange::unknown();
            };
            if divisor_min <= 0 && divisor_max >= 0 {
                return AbstractRange::unknown();
            }
            let Some(bound) = divisor_min
                .checked_abs()
                .zip(divisor_max.checked_abs())
                .map(|(lo, hi)| lo.max(hi).saturating_sub(1))
            else {
                return AbstractRange::unknown();
            };
            if divisor_min > 0 {
                KernelValueRange {
                    minimum: KernelRangeExpr::Constant(0),
                    maximum: KernelRangeExpr::Constant(bound),
                }
            } else {
                KernelValueRange {
                    minimum: KernelRangeExpr::Constant(-bound),
                    maximum: KernelRangeExpr::Constant(0),
                }
            }
        }
        HirBinOp::Shl => {
            let Some(shift) = constant(&rhs.minimum).filter(|shift| *shift >= 0 && *shift < 127)
            else {
                return AbstractRange::unknown();
            };
            if constant(&rhs.maximum) != Some(shift) {
                return AbstractRange::unknown();
            }
            let factor = KernelRangeExpr::Constant(1_i128 << shift as u32);
            let values = [
                multiply(lhs.minimum.clone(), factor.clone()),
                multiply(lhs.maximum.clone(), factor),
            ];
            let [first, rest @ ..] = &values;
            KernelValueRange {
                minimum: rest.iter().cloned().fold(first.clone(), minimum),
                maximum: rest.iter().cloned().fold(first.clone(), maximum),
            }
        }
        HirBinOp::BitAnd => {
            let Some(mask) = constant(&rhs.maximum).filter(|mask| *mask >= 0) else {
                return AbstractRange::unknown();
            };
            KernelValueRange {
                minimum: KernelRangeExpr::Constant(0),
                maximum: KernelRangeExpr::Constant(mask),
            }
        }
        HirBinOp::BitOr | HirBinOp::BitXor if lhs == rhs => lhs,
        _ => return AbstractRange::unknown(),
    };
    AbstractRange(Some(result))
}

fn add(lhs: KernelRangeExpr, rhs: KernelRangeExpr) -> KernelRangeExpr {
    if let (Some(lhs), Some(rhs)) = (constant(&lhs), constant(&rhs))
        && let Some(value) = lhs.checked_add(rhs)
    {
        return KernelRangeExpr::Constant(value);
    }
    match (lhs, rhs) {
        (value, KernelRangeExpr::Constant(0)) | (KernelRangeExpr::Constant(0), value) => value,
        (lhs, rhs) => KernelRangeExpr::Add(Box::new(lhs), Box::new(rhs)),
    }
}

fn subtract(lhs: KernelRangeExpr, rhs: KernelRangeExpr) -> KernelRangeExpr {
    if let (Some(lhs), Some(rhs)) = (constant(&lhs), constant(&rhs))
        && let Some(value) = lhs.checked_sub(rhs)
    {
        return KernelRangeExpr::Constant(value);
    }
    match (lhs, rhs) {
        (value, KernelRangeExpr::Constant(0)) => value,
        (lhs, rhs) => KernelRangeExpr::Subtract(Box::new(lhs), Box::new(rhs)),
    }
}

fn negate(value: KernelRangeExpr) -> KernelRangeExpr {
    match constant(&value).and_then(i128::checked_neg) {
        Some(value) => KernelRangeExpr::Constant(value),
        None => subtract(KernelRangeExpr::Constant(0), value),
    }
}

fn multiply(lhs: KernelRangeExpr, rhs: KernelRangeExpr) -> KernelRangeExpr {
    if let (Some(lhs), Some(rhs)) = (constant(&lhs), constant(&rhs))
        && let Some(value) = lhs.checked_mul(rhs)
    {
        return KernelRangeExpr::Constant(value);
    }
    match (&lhs, &rhs) {
        (_, KernelRangeExpr::Constant(0)) | (KernelRangeExpr::Constant(0), _) => {
            KernelRangeExpr::Constant(0)
        }
        (_, KernelRangeExpr::Constant(1)) => lhs,
        (KernelRangeExpr::Constant(1), _) => rhs,
        _ => KernelRangeExpr::Multiply(Box::new(lhs), Box::new(rhs)),
    }
}

fn floor_divide(lhs: KernelRangeExpr, rhs: KernelRangeExpr) -> Option<KernelRangeExpr> {
    if let (Some(lhs), Some(rhs)) = (constant(&lhs), constant(&rhs)) {
        if rhs == 0 {
            return None;
        }
        return lhs.checked_div_euclid(rhs).map(KernelRangeExpr::Constant);
    }
    Some(KernelRangeExpr::FloorDivide(Box::new(lhs), Box::new(rhs)))
}

fn minimum(lhs: KernelRangeExpr, rhs: KernelRangeExpr) -> KernelRangeExpr {
    match (constant(&lhs), constant(&rhs)) {
        (Some(lhs), Some(rhs)) => KernelRangeExpr::Constant(lhs.min(rhs)),
        _ if lhs == rhs => lhs,
        _ => KernelRangeExpr::Minimum(Box::new(lhs), Box::new(rhs)),
    }
}

fn maximum(lhs: KernelRangeExpr, rhs: KernelRangeExpr) -> KernelRangeExpr {
    match (constant(&lhs), constant(&rhs)) {
        (Some(lhs), Some(rhs)) => KernelRangeExpr::Constant(lhs.max(rhs)),
        _ if lhs == rhs => lhs,
        _ => KernelRangeExpr::Maximum(Box::new(lhs), Box::new(rhs)),
    }
}

fn constant(expr: &KernelRangeExpr) -> Option<i128> {
    match expr {
        KernelRangeExpr::Constant(value) => Some(*value),
        KernelRangeExpr::Shape(_) | KernelRangeExpr::Parameter(_) => None,
        KernelRangeExpr::Add(lhs, rhs) => constant(lhs)?.checked_add(constant(rhs)?),
        KernelRangeExpr::Subtract(lhs, rhs) => constant(lhs)?.checked_sub(constant(rhs)?),
        KernelRangeExpr::Multiply(lhs, rhs) => constant(lhs)?.checked_mul(constant(rhs)?),
        KernelRangeExpr::FloorDivide(lhs, rhs) => {
            let rhs = constant(rhs)?;
            (rhs != 0)
                .then(|| constant(lhs)?.checked_div_euclid(rhs))
                .flatten()
        }
        KernelRangeExpr::Minimum(lhs, rhs) => Some(constant(lhs)?.min(constant(rhs)?)),
        KernelRangeExpr::Maximum(lhs, rhs) => Some(constant(lhs)?.max(constant(rhs)?)),
    }
}

fn contains_shape(range: &KernelValueRange) -> bool {
    fn visit(expr: &KernelRangeExpr) -> bool {
        match expr {
            KernelRangeExpr::Shape(_) | KernelRangeExpr::Parameter(_) => true,
            KernelRangeExpr::Constant(_) => false,
            KernelRangeExpr::Add(lhs, rhs)
            | KernelRangeExpr::Subtract(lhs, rhs)
            | KernelRangeExpr::Multiply(lhs, rhs)
            | KernelRangeExpr::FloorDivide(lhs, rhs)
            | KernelRangeExpr::Minimum(lhs, rhs)
            | KernelRangeExpr::Maximum(lhs, rhs) => visit(lhs) || visit(rhs),
        }
    }
    visit(&range.minimum) || visit(&range.maximum)
}

fn merge_ranges(
    before: FxHashMap<DefId, AbstractRange>,
    after: FxHashMap<DefId, AbstractRange>,
) -> FxHashMap<DefId, AbstractRange> {
    let mut merged = before.clone();
    for (def_id, next) in after {
        let Some(previous) = before.get(&def_id).cloned() else {
            merged.insert(def_id, next);
            continue;
        };
        let (Some(previous), Some(next)) = (previous.0, next.0) else {
            merged.insert(def_id, AbstractRange::unknown());
            continue;
        };
        merged.insert(
            def_id,
            AbstractRange(Some(KernelValueRange {
                minimum: minimum(previous.minimum, next.minimum),
                maximum: maximum(previous.maximum, next.maximum),
            })),
        );
    }
    merged
}

fn merge_abstract_ranges(ranges: &[AbstractRange]) -> AbstractRange {
    let Some(mut ranges) = ranges
        .iter()
        .map(|range| range.0.clone())
        .collect::<Option<Vec<_>>>()
    else {
        return AbstractRange::unknown();
    };
    let Some(mut merged) = ranges.pop() else {
        return AbstractRange::unknown();
    };
    for range in ranges {
        merged.minimum = minimum(merged.minimum, range.minimum);
        merged.maximum = maximum(merged.maximum, range.maximum);
    }
    AbstractRange(Some(merged))
}

/// A loop may execute any number of times. Keep an incoming fact only when
/// the body leaves it unchanged; one-iteration propagation is not a proof for
/// a backedge.
fn merge_loop_ranges(
    before: FxHashMap<DefId, AbstractRange>,
    after: FxHashMap<DefId, AbstractRange>,
) -> FxHashMap<DefId, AbstractRange> {
    before
        .into_iter()
        .map(|(def_id, original)| {
            let next = after.get(&def_id);
            let merged = if next == Some(&original) {
                original
            } else {
                AbstractRange::unknown()
            };
            (def_id, merged)
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct SymbolicBounds {
    minimum: Option<i128>,
    maximum: Option<i128>,
}

fn symbolic_bounds(expr: &KernelRangeExpr) -> SymbolicBounds {
    if let Some(value) = constant(expr) {
        return SymbolicBounds {
            minimum: Some(value),
            maximum: Some(value),
        };
    }
    match expr {
        KernelRangeExpr::Constant(value) => SymbolicBounds {
            minimum: Some(*value),
            maximum: Some(*value),
        },
        KernelRangeExpr::Shape(_) => SymbolicBounds {
            minimum: Some(0),
            maximum: None,
        },
        KernelRangeExpr::Parameter(_) => SymbolicBounds {
            minimum: None,
            maximum: None,
        },
        KernelRangeExpr::Add(lhs, rhs) => {
            let lhs = symbolic_bounds(lhs);
            let rhs = symbolic_bounds(rhs);
            SymbolicBounds {
                minimum: checked_binary(lhs.minimum, rhs.minimum, i128::checked_add),
                maximum: checked_binary(lhs.maximum, rhs.maximum, i128::checked_add),
            }
        }
        KernelRangeExpr::Subtract(lhs, rhs) => {
            let lhs = symbolic_bounds(lhs);
            let rhs = symbolic_bounds(rhs);
            SymbolicBounds {
                minimum: checked_binary(lhs.minimum, rhs.maximum, i128::checked_sub),
                maximum: checked_binary(lhs.maximum, rhs.minimum, i128::checked_sub),
            }
        }
        KernelRangeExpr::Multiply(lhs, rhs) => {
            let lhs_constant = constant(lhs);
            let rhs_constant = constant(rhs);
            match (lhs_constant, rhs_constant) {
                (Some(value), None) => scale_bounds(symbolic_bounds(rhs), value),
                (None, Some(value)) => scale_bounds(symbolic_bounds(lhs), value),
                _ => {
                    let lhs = symbolic_bounds(lhs);
                    let rhs = symbolic_bounds(rhs);
                    let values = [
                        checked_binary(lhs.minimum, rhs.minimum, i128::checked_mul),
                        checked_binary(lhs.minimum, rhs.maximum, i128::checked_mul),
                        checked_binary(lhs.maximum, rhs.minimum, i128::checked_mul),
                        checked_binary(lhs.maximum, rhs.maximum, i128::checked_mul),
                    ];
                    let known: Vec<i128> = values.into_iter().flatten().collect();
                    if known.len() == 4 {
                        bounds_of_values(&known)
                    } else {
                        SymbolicBounds {
                            minimum: None,
                            maximum: None,
                        }
                    }
                }
            }
        }
        KernelRangeExpr::FloorDivide(lhs, rhs) => {
            let Some(divisor) = constant(rhs).filter(|divisor| *divisor != 0) else {
                return SymbolicBounds {
                    minimum: None,
                    maximum: None,
                };
            };
            let numerator = symbolic_bounds(lhs);
            if divisor > 0 {
                SymbolicBounds {
                    minimum: numerator
                        .minimum
                        .and_then(|value| value.checked_div_euclid(divisor)),
                    maximum: numerator
                        .maximum
                        .and_then(|value| value.checked_div_euclid(divisor)),
                }
            } else {
                SymbolicBounds {
                    minimum: numerator
                        .maximum
                        .and_then(|value| value.checked_div_euclid(divisor)),
                    maximum: numerator
                        .minimum
                        .and_then(|value| value.checked_div_euclid(divisor)),
                }
            }
        }
        KernelRangeExpr::Minimum(lhs, rhs) => {
            let lhs = symbolic_bounds(lhs);
            let rhs = symbolic_bounds(rhs);
            SymbolicBounds {
                minimum: checked_min_lower(lhs.minimum, rhs.minimum),
                maximum: checked_min_upper(lhs.maximum, rhs.maximum),
            }
        }
        KernelRangeExpr::Maximum(lhs, rhs) => {
            let lhs = symbolic_bounds(lhs);
            let rhs = symbolic_bounds(rhs);
            SymbolicBounds {
                minimum: checked_max_lower(lhs.minimum, rhs.minimum),
                maximum: checked_max_upper(lhs.maximum, rhs.maximum),
            }
        }
    }
}

fn checked_binary(
    lhs: Option<i128>,
    rhs: Option<i128>,
    operation: fn(i128, i128) -> Option<i128>,
) -> Option<i128> {
    operation(lhs?, rhs?)
}

fn scale_bounds(bounds: SymbolicBounds, scale: i128) -> SymbolicBounds {
    if scale >= 0 {
        SymbolicBounds {
            minimum: bounds.minimum.and_then(|value| value.checked_mul(scale)),
            maximum: bounds.maximum.and_then(|value| value.checked_mul(scale)),
        }
    } else {
        SymbolicBounds {
            minimum: bounds.maximum.and_then(|value| value.checked_mul(scale)),
            maximum: bounds.minimum.and_then(|value| value.checked_mul(scale)),
        }
    }
}

fn bounds_of_values(values: &[i128]) -> SymbolicBounds {
    let (minimum, maximum) = values
        .iter()
        .copied()
        .fold((i128::MAX, i128::MIN), |(minimum, maximum), value| {
            (minimum.min(value), maximum.max(value))
        });
    SymbolicBounds {
        minimum: Some(minimum),
        maximum: Some(maximum),
    }
}

fn checked_min_lower(lhs: Option<i128>, rhs: Option<i128>) -> Option<i128> {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => Some(lhs.min(rhs)),
        _ => None,
    }
}

fn checked_min_upper(lhs: Option<i128>, rhs: Option<i128>) -> Option<i128> {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => Some(lhs.min(rhs)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn checked_max_lower(lhs: Option<i128>, rhs: Option<i128>) -> Option<i128> {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => Some(lhs.max(rhs)),
        (Some(lhs), None) | (None, Some(lhs)) => Some(lhs),
        (None, None) => None,
    }
}

fn checked_max_upper(lhs: Option<i128>, rhs: Option<i128>) -> Option<i128> {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => Some(lhs.max(rhs)),
        _ => None,
    }
}

fn could_fit_between(
    minimum: SymbolicBounds,
    maximum: SymbolicBounds,
    slot_min: i128,
    slot_max: i128,
) -> bool {
    minimum.minimum.is_none_or(|minimum| minimum <= slot_max)
        && maximum.maximum.is_none_or(|maximum| maximum >= slot_min)
}

fn excludes_zero(range: &AbstractRange) -> bool {
    let Some(range) = &range.0 else {
        return false;
    };
    let minimum = symbolic_bounds(&range.minimum);
    let maximum = symbolic_bounds(&range.maximum);
    minimum.minimum.is_some_and(|minimum| minimum > 0)
        || maximum.maximum.is_some_and(|maximum| maximum < 0)
}

fn proves_nonnegative(range: &AbstractRange) -> bool {
    range
        .0
        .as_ref()
        .and_then(|range| symbolic_bounds(&range.minimum).minimum)
        .is_some_and(|minimum| minimum >= 0)
}

fn reverse_comparison(op: HirBinOp) -> HirBinOp {
    match op {
        HirBinOp::Lt => HirBinOp::Gt,
        HirBinOp::Gt => HirBinOp::Lt,
        HirBinOp::LtEq => HirBinOp::GtEq,
        HirBinOp::GtEq => HirBinOp::LtEq,
        other => other,
    }
}

fn refine_interval(
    op: HirBinOp,
    current: KernelValueRange,
    bound: KernelValueRange,
    truth: bool,
) -> Option<KernelValueRange> {
    let (Some(bound_min), Some(bound_max)) = (constant(&bound.minimum), constant(&bound.maximum))
    else {
        return None;
    };
    if bound_min != bound_max {
        return None;
    }
    let bound = bound_min;
    let range = match (op, truth) {
        (HirBinOp::Lt, true) | (HirBinOp::GtEq, false) => KernelValueRange {
            minimum: current.minimum,
            maximum: minimum(
                current.maximum,
                KernelRangeExpr::Constant(bound.checked_sub(1)?),
            ),
        },
        (HirBinOp::Lt, false) | (HirBinOp::GtEq, true) => KernelValueRange {
            minimum: maximum(current.minimum, KernelRangeExpr::Constant(bound)),
            maximum: current.maximum,
        },
        (HirBinOp::LtEq, true) | (HirBinOp::Gt, false) => KernelValueRange {
            minimum: current.minimum,
            maximum: minimum(current.maximum, KernelRangeExpr::Constant(bound)),
        },
        (HirBinOp::LtEq, false) | (HirBinOp::Gt, true) => KernelValueRange {
            minimum: maximum(
                current.minimum,
                KernelRangeExpr::Constant(bound.checked_add(1)?),
            ),
            maximum: current.maximum,
        },
        _ => return None,
    };
    if constant(&range.minimum)
        .zip(constant(&range.maximum))
        .is_some_and(|(min, max)| min > max)
    {
        None
    } else {
        Some(range)
    }
}

fn shape_dimensions(ty: TypeId, types: &TypeTable) -> Vec<IndexId> {
    let shape = match types.get(ty) {
        Type::Tensor(_, shape)
        | Type::Vector(_, shape)
        | Type::Matrix(_, shape)
        | Type::Sparsa(_, shape) => *shape,
        Type::Alias(_, target) | Type::Ref(_, target) => {
            return shape_dimensions(*target, types);
        }
        _ => return Vec::new(),
    };
    match types.get_index(shape) {
        IndexExpr::Tuple(dimensions) => dimensions.clone(),
        IndexExpr::Unspecified => Vec::new(),
        _ => vec![shape],
    }
}

fn coordinate_range(dimension: IndexId, types: &TypeTable) -> AbstractRange {
    match evaluate_index_dimension(dimension, types) {
        Ok(Some(extent)) if extent > 0 => AbstractRange(Some(KernelValueRange {
            minimum: KernelRangeExpr::Constant(0),
            maximum: KernelRangeExpr::Constant(i128::from(extent - 1)),
        })),
        Ok(Some(0)) | Err(_) => AbstractRange::unknown(),
        _ => AbstractRange(Some(KernelValueRange {
            minimum: KernelRangeExpr::Constant(0),
            maximum: subtract(
                KernelRangeExpr::Shape(dimension),
                KernelRangeExpr::Constant(1),
            ),
        })),
    }
}

fn tensor_element(ty: TypeId, types: &TypeTable) -> Option<TypeId> {
    match types.get(ty) {
        Type::Tensor(element, _)
        | Type::Vector(element, _)
        | Type::Matrix(element, _)
        | Type::Sparsa(element, _)
        | Type::Array(element)
        | Type::BoundedArray { element, .. } => Some(*element),
        Type::Ref(_, target) | Type::Alias(_, target) => tensor_element(*target, types),
        _ => None,
    }
}

fn dedupe<T: Clone + PartialEq>(values: Vec<T>) -> Vec<T> {
    values.into_iter().fold(Vec::new(), |mut out, value| {
        if !out.contains(&value) {
            out.push(value);
        }
        out
    })
}

#[derive(Debug, Default)]
struct TypeFacts {
    fields: FxHashMap<DefId, Vec<TypeId>>,
}

impl TypeFacts {
    fn from_hir(hir: &radix_hir::HirModule) -> Self {
        let mut facts = Self::default();
        for item in &hir.items {
            match &item.kind {
                HirItemKind::Struct(strukt) => {
                    facts.fields.insert(
                        item.def_id,
                        strukt.fields.iter().map(|field| field.ty).collect(),
                    );
                }
                HirItemKind::Enum(enumeration) => {
                    facts.fields.insert(
                        item.def_id,
                        enumeration
                            .commune_fields
                            .iter()
                            .chain(
                                enumeration
                                    .variants
                                    .iter()
                                    .flat_map(|variant| variant.fields.iter()),
                            )
                            .map(|field| field.ty)
                            .collect(),
                    );
                }
                _ => {}
            }
        }
        facts
    }
}
