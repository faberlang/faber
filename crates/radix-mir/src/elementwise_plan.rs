//! Target-neutral elementwise expression plan for device MIR (OF-1).
//!
//! The plan is a typed side structure beside MIR device planning. It is built
//! from existing statement, classifier, type, layout, span, and read facts.
//! Classification is consulted from [`crate::device_program_plans`] /
//! `kernel_plan/`; this module never re-derives the recipe/elementwise sets
//! and never treats [`crate::kernel_plan::CollectionKernelPlan::Elementwise`]
//! as a silent fallback.

use crate::device_program_plans::{
    is_admitted_tensor_unary, is_elementwise_collection_op, is_recipe_op,
};
use crate::tensor_type::tensor_literal_dims;
use crate::{
    MirCollectionOp, MirConstant, MirFunction, MirLayoutKind, MirLocalId, MirOperand, MirPlaceBase,
    MirStatement, MirStatementKind, MirTempId, MirType, MirUnOp, MirValidationContext,
    MirValueKind, collection_ops::statement_collection_op,
};
use handroll::{FxHashMap, FxHashSet};
use radix_lexer::Span;
use radix_types::MirMathFunction;
use radix_types::Type;
use std::fmt;

/// One typed input of an [`ElementwisePlan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementwiseInput {
    /// Source local this slot reads.
    pub local: MirLocalId,
    /// Typed MIR type of the local.
    pub ty: MirType,
    /// Literal dims from the type table; empty for a scalar.
    pub shape: Vec<u64>,
    /// Per-element index mapping against the plan result shape.
    pub index: IndexMapping,
}

/// Typed destination of an [`ElementwisePlan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementwiseOutput {
    /// Destination local written by the composed expression.
    pub local: MirLocalId,
    /// Typed MIR type of the destination.
    pub ty: MirType,
    /// Literal result dims from the type table.
    pub shape: Vec<u64>,
}

/// Concrete result shape carried by the plan (type-table dims plus layout).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConcreteTensorShape {
    /// Literal dimension list (`[4]` or `[4, 8]`).
    pub dims: Vec<u64>,
    /// Element type of the tensor (or the scalar itself).
    pub element_ty: MirType,
    /// Layout fact from the validation table, when one exists.
    pub layout: Option<MirLayoutKind>,
}

/// How an input is indexed relative to the plan's result shape.
///
/// Rank-extension broadcast is an explicit mapping from typed dims. Equal
/// element counts are never treated as proof of the same indexing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexMapping {
    /// Operand shape equals the result shape; load at the result index.
    Identity,
    /// Lower-rank operand whose dims equal the result's trailing dims.
    RankExtension {
        /// Operand literal dims.
        operand_shape: Vec<u64>,
        /// Result literal dims.
        result_shape: Vec<u64>,
    },
    /// Scalar or one-element operand, broadcast to every result lane.
    BroadcastScalar,
}

/// Declared numerical policy for the composed expression.
///
/// Phase one preserves source order and forbids reassociation, FMA, and
/// implicit widening in the target-neutral plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumericalPolicy {
    /// Whether the plan may rewrite operation association.
    pub reassociation: ReassociationPolicy,
}

impl NumericalPolicy {
    /// Source-order policy: no reassociation or synthesized FMA/fast-math.
    pub const SOURCE_ORDER: Self = Self {
        reassociation: ReassociationPolicy::Forbidden,
    };
}

/// Reassociation permission for a composed expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReassociationPolicy {
    /// Source order is preserved; the plan does not reassociate.
    Forbidden,
}

/// Source-span fact for one composed operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpanFact {
    /// Span of the originating MIR statement.
    pub span: Span,
    /// Elementwise op at that span, when the statement is a collection op.
    pub op: Option<ElementwiseOp>,
}

/// Closed set of elementwise operations the plan may compose.
///
/// Collection-op membership is decided by [`is_elementwise_collection_op`];
/// `Relu`/`Gelu` name both their collection and tensor-unary spellings, and
/// `Tanh`/`Exp`/`Sqrt`/`F16Round` name the admitted tensor unaries
/// ([`is_admitted_tensor_unary`]) that have no collection spelling. The enum
/// only names an already-classified op; it never re-derives admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementwiseOp {
    /// [`MirCollectionOp::TensorAdd`].
    Add,
    /// [`MirCollectionOp::TensorSub`].
    Sub,
    /// [`MirCollectionOp::TensorMul`].
    Mul,
    /// [`MirCollectionOp::TensorDiv`].
    Div,
    /// [`MirCollectionOp::TensorNeg`].
    Neg,
    /// [`MirCollectionOp::TensorFill`].
    Fill,
    /// [`MirCollectionOp::TensorRelu`] or the admitted `MirUnOp::Relu`.
    Relu,
    /// [`MirCollectionOp::TensorGelu`] or the admitted `MirUnOp::Gelu`.
    Gelu,
    /// The admitted `MirUnOp::Tanh` assignment.
    Tanh,
    /// The admitted `MirUnOp::Exp` assignment.
    Exp,
    /// The admitted `MirUnOp::Sqrt` assignment.
    Sqrt,
    /// The admitted `MirUnOp::F16Round` assignment.
    F16Round,
}

/// Target-neutral per-element expression. Nodes carry provenance and the
/// source-order numerical policy; they do not encode target syntax.
#[derive(Debug, Clone, PartialEq)]
pub enum ElementwiseExpr {
    /// Load or broadcast a plan input.
    Input {
        /// Index into [`ElementwisePlan::inputs`].
        slot: usize,
        /// Index mapping for this use.
        index: IndexMapping,
    },
    /// Embedded scalar constant.
    Scalar(MirConstant),
    /// Unary elementwise op.
    Unary {
        /// Classified elementwise op.
        op: ElementwiseOp,
        /// Operand expression.
        operand: Box<ElementwiseExpr>,
        /// Originating statement span.
        provenance: SourceSpanFact,
        /// Numerical policy at this node (source order).
        numerical_policy: NumericalPolicy,
    },
    /// Binary elementwise op. Operand order is the MIR operand order.
    Binary {
        /// Classified elementwise op.
        op: ElementwiseOp,
        /// Left operand (MIR `args[0]`).
        lhs: Box<ElementwiseExpr>,
        /// Right operand (MIR `args[1]`).
        rhs: Box<ElementwiseExpr>,
        /// Originating statement span.
        provenance: SourceSpanFact,
        /// Numerical policy at this node (source order).
        numerical_policy: NumericalPolicy,
    },
}

/// Typed elementwise plan for one legal single-consumer device subchain.
#[derive(Debug, Clone, PartialEq)]
pub struct ElementwisePlan {
    /// External reads, in first-use order.
    pub inputs: Vec<ElementwiseInput>,
    /// Composed destination.
    pub output: ElementwiseOutput,
    /// Concrete result shape from type-table / layout facts.
    pub shape: ConcreteTensorShape,
    /// Per-element expression in source operation order.
    pub expression: ElementwiseExpr,
    /// Statement spans in source order.
    pub provenance: Vec<SourceSpanFact>,
    /// Chain-wide numerical policy (no reassociation).
    pub numerical_policy: NumericalPolicy,
    /// Every local the composed chain writes (the plan's intermediate
    /// producers plus the output), in ascending local-id order — the
    /// consumer's statement-coverage fact (PGC-R6 / OF-3). Pure-forward
    /// alias destinations belong here too (1A-bis F7).
    pub composed_locals: Vec<MirLocalId>,
    /// Every temp the composed chain writes (the plan's temp producers plus
    /// temp alias destinations), in ascending temp-id order — the temp half
    /// of the coverage fact, so Unit 3 never reconstructs it from Metal MIR
    /// (1A-bis F7).
    pub composed_temps: Vec<MirTempId>,
}

impl ElementwisePlan {
    /// Whether `local` is written by this plan (an intermediate producer
    /// or the composed output) — the emit-side coverage check.
    #[must_use]
    pub fn covers_local(&self, local: MirLocalId) -> bool {
        local == self.output.local || self.composed_locals.contains(&local)
    }

    /// Whether an unprojected place base slot is written by this plan —
    /// the place/base coverage accessor over locals and temps (1A-bis F7).
    #[must_use]
    pub fn covers_slot(&self, base: &MirPlaceBase) -> bool {
        match base {
            MirPlaceBase::Local(local) => self.covers_local(*local),
            MirPlaceBase::Temp(temp) => self.composed_temps.contains(temp),
        }
    }
}

/// Typed reason a function is not a phase-one elementwise plan.
// `PartialEq` only: `MultiConsumer` names its slot with `MirPlaceBase`,
// which carries no `Eq` (matches! is the consumer-side pattern anyway).
#[derive(Debug, Clone, PartialEq)]
pub enum ElementwiseBarrierError {
    /// Recipe-bearing collection op (`is_recipe_op`); never elementwise.
    Recipe {
        /// The recipe collection op.
        op: MirCollectionOp,
        /// Statement span.
        span: Span,
    },
    /// Collection op outside the closed elementwise set and not a recipe.
    UnsupportedOp {
        /// The unplannable collection op.
        op: MirCollectionOp,
        /// Statement span.
        span: Span,
    },
    /// An intermediate is read by more than one live consumer.
    MultiConsumer {
        /// The shared intermediate slot (local or temp).
        slot: MirPlaceBase,
        /// Span of a consumer or the producer.
        span: Span,
    },
    /// A slot is written by more than one statement in the block — an SSA
    /// ambiguity: resolution would be order-dependent (reads between the
    /// writes see different versions), so it fails closed instead of
    /// resolving by map precedence.
    Redefinition {
        /// The redefined slot.
        slot: MirPlaceBase,
        /// Span of the second definition.
        span: Span,
    },
    /// Destination place carries a projection (alias/index/field).
    ProjectedDestination {
        /// Statement span.
        span: Span,
    },
    /// Multi-block or non-straight-line body.
    ControlFlow {
        /// Function span.
        span: Span,
    },
    /// Shape, layout, or broadcast mapping is not a typed fact.
    UnresolvedShape {
        /// Statement or operand span.
        span: Span,
    },
    /// Call or effect outside the closed elementwise set.
    Effect {
        /// Statement span.
        span: Span,
    },
    /// No elementwise collection statement feeds the result.
    NoCandidate {
        /// Function span.
        span: Span,
    },
}

impl fmt::Display for ElementwiseBarrierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recipe { op, .. } => {
                write!(f, "recipe op {op:?} is an elementwise fusion barrier")
            }
            Self::UnsupportedOp { op, .. } => {
                write!(f, "collection op {op:?} is not an elementwise candidate")
            }
            Self::MultiConsumer { slot, .. } => {
                write!(f, "intermediate slot {slot:?} has multiple live consumers")
            }
            Self::Redefinition { slot, .. } => {
                write!(f, "slot {slot:?} is written more than once in the block")
            }
            Self::ProjectedDestination { .. } => {
                write!(f, "destination place carries an unsupported projection")
            }
            Self::ControlFlow { .. } => {
                write!(
                    f,
                    "device function body is not a single straight-line block"
                )
            }
            Self::UnresolvedShape { .. } => {
                write!(f, "shape, layout, or broadcast mapping is unresolved")
            }
            Self::Effect { .. } => {
                write!(
                    f,
                    "statement is not a device-safe elementwise collection op"
                )
            }
            Self::NoCandidate { .. } => {
                write!(f, "function has no elementwise collection result to plan")
            }
        }
    }
}

impl std::error::Error for ElementwiseBarrierError {}

/// Build a typed elementwise plan from a device function's existing facts.
///
/// Classification uses [`is_recipe_op`] / [`is_elementwise_collection_op`]
/// (the `kernel_plan/` recipe-authority boundary). Shapes come from the
/// type table; layout is consulted from the validation layout table; reads
/// come from [`crate::read_analysis`]. The expression preserves MIR operand
/// order and never reassociates.
///
/// # Errors
///
/// Returns a typed [`ElementwiseBarrierError`] when the body is not a phase-one
/// single-consumer elementwise chain.
#[must_use = "the plan or barrier must be acted on"]
pub fn plan_elementwise(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<ElementwisePlan, ElementwiseBarrierError> {
    if function.blocks.len() != 1 {
        return Err(ElementwiseBarrierError::ControlFlow {
            span: function.span,
        });
    }
    let block = &function.blocks[0];
    let consumer_counts = slot_consumer_counts(function);
    let mut producers: FxHashMap<PlanValue, Producer> = FxHashMap::default();
    let mut forwards: FxHashMap<PlanValue, PureForward> = FxHashMap::default();
    for (index, statement) in block.statements.iter().enumerate() {
        let dest_place = crate::read_analysis::statement_destination_place(statement);
        if dest_place.is_some_and(|place| !place.projections.is_empty()) {
            return Err(ElementwiseBarrierError::ProjectedDestination {
                span: statement.span,
            });
        }
        let dest_slot = dest_place
            .and_then(crate::read_analysis::place_slot_base)
            .map(PlanValue::from);
        match statement_role(statement) {
            StatementRole::Collection(op) => {
                if !dest_slot
                    .is_some_and(|slot| destination_slot_is_live(function, slot, &consumer_counts))
                {
                    continue;
                }
                let classified = classify_collection_op(op, statement.span)?;
                let Some(slot) = dest_slot else {
                    return Err(ElementwiseBarrierError::UnresolvedShape {
                        span: statement.span,
                    });
                };
                enforce_single_consumer(slot, &consumer_counts, statement.span)?;
                enforce_single_definition(slot, &producers, &forwards, statement.span)?;
                let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                    return Err(ElementwiseBarrierError::Effect {
                        span: statement.span,
                    });
                };
                producers.insert(
                    slot,
                    Producer {
                        index,
                        op: classified,
                        args: call.args.clone(),
                        span: statement.span,
                    },
                );
            }
            StatementRole::TensorUnary(un_op) => {
                let Some(slot) = dest_slot else {
                    return Err(ElementwiseBarrierError::UnresolvedShape {
                        span: statement.span,
                    });
                };
                if !destination_slot_is_live(function, slot, &consumer_counts) {
                    continue;
                }
                // The admission authority already spoke; this names the op
                // (1A-bis F3). A divergence between admission and naming
                // stays fail-closed.
                let Some(classified) = named_tensor_unary(un_op) else {
                    return Err(ElementwiseBarrierError::Effect {
                        span: statement.span,
                    });
                };
                enforce_single_consumer(slot, &consumer_counts, statement.span)?;
                enforce_single_definition(slot, &producers, &forwards, statement.span)?;
                let MirStatementKind::Assign { value, .. } = &statement.kind else {
                    return Err(ElementwiseBarrierError::Effect {
                        span: statement.span,
                    });
                };
                let MirValueKind::Unary { operand, .. } = &value.kind else {
                    return Err(ElementwiseBarrierError::Effect {
                        span: statement.span,
                    });
                };
                producers.insert(
                    slot,
                    Producer {
                        index,
                        op: classified,
                        args: vec![operand.clone()],
                        span: statement.span,
                    },
                );
            }
            StatementRole::PureForward(source_base) => {
                let Some(slot) = dest_slot else {
                    return Err(ElementwiseBarrierError::UnresolvedShape {
                        span: statement.span,
                    });
                };
                if !destination_slot_is_live(function, slot, &consumer_counts) {
                    continue;
                }
                let MirStatementKind::Assign { value, .. } = &statement.kind else {
                    return Err(ElementwiseBarrierError::Effect {
                        span: statement.span,
                    });
                };
                // A pure forward is an alias only when its value, source
                // slot, and destination slot types are all present and
                // semantically equal (1A-bis F4); anything else fails closed.
                if !forward_types_equal(
                    function,
                    validation,
                    value.ty,
                    PlanValue::from(source_base.clone()),
                    slot,
                ) {
                    return Err(ElementwiseBarrierError::Effect {
                        span: statement.span,
                    });
                }
                enforce_single_consumer(slot, &consumer_counts, statement.span)?;
                enforce_single_definition(slot, &producers, &forwards, statement.span)?;
                forwards.insert(
                    slot,
                    PureForward {
                        source: PlanValue::from(source_base),
                    },
                );
            }
            StatementRole::Other => {
                if let Some(slot) = dest_slot
                    && consumer_counts.get(&slot).copied().unwrap_or(0) > 0
                {
                    return Err(ElementwiseBarrierError::Effect {
                        span: statement.span,
                    });
                }
            }
        }
    }
    let output_local = crate::read_analysis::terminator_return_local(&block.terminator.kind)
        .or_else(|| {
            block
                .statements
                .iter()
                .rev()
                .find_map(crate::read_analysis::statement_destination)
        })
        .ok_or(ElementwiseBarrierError::NoCandidate {
            span: function.span,
        })?;
    // The expression root stays ABI-local (1A-bis F5): plan from the
    // returned local, following pure forwards to their producer. A
    // dangling temp is never invented as an input here — an unresolved
    // root is a typed barrier.
    let mut root = PlanValue::Local(output_local);
    let mut walked = FxHashSet::default();
    while let Some(forward) = forwards.get(&root) {
        if !walked.insert(root) {
            return Err(ElementwiseBarrierError::UnresolvedShape {
                span: function.span,
            });
        }
        root = forward.source;
    }
    let Some(_) = producers.get(&root) else {
        return Err(ElementwiseBarrierError::NoCandidate {
            span: function.span,
        });
    };
    // The plan output type is the destination local's declared type — the
    // producer's return type can be an alias spelling of it.
    let output_ty = crate::local_ty::local_ty(function, output_local).ok_or(
        ElementwiseBarrierError::UnresolvedShape {
            span: function.span,
        },
    )?;
    let result_shape = concrete_shape(output_ty, validation, function.span)?;
    let mut builder = PlanBuilder {
        function,
        validation,
        producers,
        forwards,
        result_shape: result_shape.dims.clone(),
        inputs: Vec::new(),
        input_slots: FxHashMap::default(),
        used: FxHashSet::default(),
        visiting: FxHashSet::default(),
    };
    let expression = builder.expr_for_slot(root, function.span)?;
    let mut used: Vec<usize> = builder.used.into_iter().collect();
    used.sort_unstable();
    let provenance = used
        .into_iter()
        .filter_map(|index| {
            builder
                .producers
                .values()
                .find(|producer| producer.index == index)
                .map(|producer| SourceSpanFact {
                    span: producer.span,
                    op: Some(producer.op),
                })
        })
        .collect();
    let mut composed_locals: Vec<MirLocalId> = builder
        .producers
        .keys()
        .chain(builder.forwards.keys())
        .filter_map(|value| match value {
            PlanValue::Local(local) => Some(*local),
            PlanValue::Temp(_) => None,
        })
        .collect();
    composed_locals.sort_unstable();
    composed_locals.dedup();
    let mut composed_temps: Vec<MirTempId> = builder
        .producers
        .keys()
        .chain(builder.forwards.keys())
        .filter_map(|value| match value {
            PlanValue::Temp(temp) => Some(*temp),
            PlanValue::Local(_) => None,
        })
        .collect();
    composed_temps.sort_unstable();
    composed_temps.dedup();
    Ok(ElementwisePlan {
        inputs: builder.inputs,
        output: ElementwiseOutput {
            local: output_local,
            ty: output_ty,
            shape: result_shape.dims.clone(),
        },
        shape: result_shape,
        expression,
        provenance,
        numerical_policy: NumericalPolicy::SOURCE_ORDER,
        composed_locals,
        composed_temps,
    })
}

/// Human-readable plan-or-barrier summary for one device function
/// (design §12: fusion facts must be discoverable without parsing target
/// text). `"elementwise plan: …"` for a legal chain, `"elementwise
/// barrier: <typed reason>"` otherwise.
#[must_use]
pub fn elementwise_plan_summary(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> String {
    match plan_elementwise(function, validation) {
        Ok(plan) => format!(
            "elementwise plan: {} ops, {} inputs, output local {}",
            plan.provenance.len(),
            plan.inputs.len(),
            plan.output.local.0
        ),
        Err(error) => format!("elementwise barrier: {error}"),
    }
}

/// Shared `kernel_plan/` classification: recipe, closed elementwise set, or
/// unplannable. Never uses `CollectionKernelPlan::from_contract` returning
/// `None` as an elementwise signal.
fn classify_collection_op(
    op: MirCollectionOp,
    span: Span,
) -> Result<ElementwiseOp, ElementwiseBarrierError> {
    if is_recipe_op(op) {
        return Err(ElementwiseBarrierError::Recipe { op, span });
    }
    if is_elementwise_collection_op(op) {
        return named_elementwise_op(op).ok_or(ElementwiseBarrierError::UnsupportedOp { op, span });
    }
    Err(ElementwiseBarrierError::UnsupportedOp { op, span })
}

/// Name an already-classified elementwise collection op.
fn named_elementwise_op(op: MirCollectionOp) -> Option<ElementwiseOp> {
    match op {
        MirCollectionOp::TensorAdd => Some(ElementwiseOp::Add),
        MirCollectionOp::TensorSub => Some(ElementwiseOp::Sub),
        MirCollectionOp::TensorMul => Some(ElementwiseOp::Mul),
        MirCollectionOp::TensorDiv => Some(ElementwiseOp::Div),
        MirCollectionOp::TensorNeg => Some(ElementwiseOp::Neg),
        MirCollectionOp::TensorFill => Some(ElementwiseOp::Fill),
        MirCollectionOp::TensorRelu => Some(ElementwiseOp::Relu),
        MirCollectionOp::TensorGelu => Some(ElementwiseOp::Gelu),
        // The math functions the elementwise emitters lower per element: the
        // plan names they share with the admitted tensor unaries (the
        // `ElementwiseOp` variants already exist for the `MirUnOp` seam).
        MirCollectionOp::TensorMath(MirMathFunction::Sqrt) => Some(ElementwiseOp::Sqrt),
        MirCollectionOp::TensorMath(MirMathFunction::Exp) => Some(ElementwiseOp::Exp),
        _ => None,
    }
}

/// One planner value space over local and temp slots (1A-bis F1): every
/// producer, alias, consumer-count, recursion, cycle, and coverage fact is
/// keyed by this. `ElementwiseInput`/`ElementwiseOutput` stay local-based
/// ABI facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PlanValue {
    Local(MirLocalId),
    Temp(MirTempId),
}

impl From<MirPlaceBase> for PlanValue {
    fn from(base: MirPlaceBase) -> Self {
        match base {
            MirPlaceBase::Local(local) => Self::Local(local),
            MirPlaceBase::Temp(temp) => Self::Temp(temp),
        }
    }
}

impl From<PlanValue> for MirPlaceBase {
    fn from(value: PlanValue) -> Self {
        match value {
            PlanValue::Local(local) => Self::Local(local),
            PlanValue::Temp(temp) => Self::Temp(temp),
        }
    }
}

/// A proven pure slot-to-slot forward (`dest ← source`): an alias that
/// contributes no expression node and no provenance operation (1A-bis F4).
struct PureForward {
    source: PlanValue,
}

/// The planner role of one statement (1A-bis F3/F4): a collection runtime
/// call, an admitted tensor unary assignment, a pure slot-to-slot copy
/// candidate, or anything else.
enum StatementRole {
    Collection(MirCollectionOp),
    TensorUnary(MirUnOp),
    PureForward(MirPlaceBase),
    Other,
}

fn statement_role(statement: &MirStatement) -> StatementRole {
    if let Some(op) = statement_collection_op(statement) {
        return StatementRole::Collection(op);
    }
    let MirStatementKind::Assign { value, .. } = &statement.kind else {
        return StatementRole::Other;
    };
    match &value.kind {
        MirValueKind::Unary { op, .. } if is_admitted_tensor_unary(*op) => {
            StatementRole::TensorUnary(*op)
        }
        MirValueKind::Operand(operand) => match operand {
            MirOperand::Temp(temp) => StatementRole::PureForward(MirPlaceBase::Temp(*temp)),
            MirOperand::Place(place) => match crate::read_analysis::place_slot_base(place) {
                Some(base) => StatementRole::PureForward(base),
                None => StatementRole::Other,
            },
            _ => StatementRole::Other,
        },
        _ => StatementRole::Other,
    }
}

/// Name an admitted tensor unary in the target-neutral op set.
fn named_tensor_unary(op: MirUnOp) -> Option<ElementwiseOp> {
    match op {
        MirUnOp::Tanh => Some(ElementwiseOp::Tanh),
        MirUnOp::Relu => Some(ElementwiseOp::Relu),
        MirUnOp::Gelu => Some(ElementwiseOp::Gelu),
        MirUnOp::Exp => Some(ElementwiseOp::Exp),
        MirUnOp::Sqrt => Some(ElementwiseOp::Sqrt),
        MirUnOp::F16Round => Some(ElementwiseOp::F16Round),
        _ => None,
    }
}

/// Consumer counts over the planner's slot space (locals AND temps,
/// including terminator reads and pure forwards — 1A-bis F6).
fn slot_consumer_counts(function: &MirFunction) -> FxHashMap<PlanValue, usize> {
    let mut counts = FxHashMap::default();
    for block in &function.blocks {
        for statement in &block.statements {
            for base in crate::read_analysis::statement_read_slots(statement) {
                *counts.entry(PlanValue::from(base)).or_insert(0) += 1;
            }
        }
        for base in crate::read_analysis::terminator_read_slots(&block.terminator.kind) {
            *counts.entry(PlanValue::from(base)).or_insert(0) += 1;
        }
    }
    counts
}

/// Whether a destination slot is live: a local destination that is a
/// function param is a side-effect write (the `place_is_read` rule);
/// otherwise the slot needs at least one read.
fn destination_slot_is_live(
    function: &MirFunction,
    slot: PlanValue,
    consumer_counts: &FxHashMap<PlanValue, usize>,
) -> bool {
    let is_read = consumer_counts.get(&slot).copied().unwrap_or(0) > 0;
    match slot {
        PlanValue::Local(local) => {
            is_read || function.params.iter().any(|param| param.local == local)
        }
        PlanValue::Temp(_) => is_read,
    }
}

/// The single-consumer law over slots (1A-bis F6): a produced slot read by
/// a forward AND another statement cannot fuse. Re-reading an external
/// input is legal; the law only binds slots the plan writes, which is why
/// this check runs per producer/alias destination.
fn enforce_single_consumer(
    slot: PlanValue,
    consumer_counts: &FxHashMap<PlanValue, usize>,
    span: Span,
) -> Result<(), ElementwiseBarrierError> {
    if consumer_counts.get(&slot).copied().unwrap_or(0) > 1 {
        return Err(ElementwiseBarrierError::MultiConsumer {
            slot: MirPlaceBase::from(slot),
            span,
        });
    }
    Ok(())
}

/// The single-definition (SSA) invariant over the planner's slot space: a
/// slot written twice makes expression resolution order-dependent (reads
/// between the writes see different versions), so a second definition
/// fails closed instead of resolving by map precedence (review P1).
/// Dead definitions never reach this check — a skipped write has no reads
/// anywhere, so it cannot interleave with the live one.
fn enforce_single_definition(
    slot: PlanValue,
    producers: &FxHashMap<PlanValue, Producer>,
    forwards: &FxHashMap<PlanValue, PureForward>,
    span: Span,
) -> Result<(), ElementwiseBarrierError> {
    if producers.contains_key(&slot) || forwards.contains_key(&slot) {
        return Err(ElementwiseBarrierError::Redefinition {
            slot: MirPlaceBase::from(slot),
            span,
        });
    }
    Ok(())
}

/// The MIR type of a planner slot (params first for locals; the temps
/// table for temps).
fn slot_ty(function: &MirFunction, value: PlanValue) -> Option<MirType> {
    match value {
        PlanValue::Local(local) => crate::local_ty::local_ty(function, local),
        PlanValue::Temp(temp) => function
            .temps
            .iter()
            .find(|candidate| candidate.id == temp)
            .map(|candidate| candidate.ty),
    }
}

/// Whether a pure forward's value, source slot, and destination slot types
/// are all present and semantically equal under `TypeTable::equals` (the
/// alias proof; aliases peel, so a `fused_result` temp forwards into a
/// `fused` local — 1A-bis F4).
fn forward_types_equal(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    value_ty: MirType,
    source: PlanValue,
    dest: PlanValue,
) -> bool {
    let (Some(source_ty), Some(dest_ty)) = (slot_ty(function, source), slot_ty(function, dest))
    else {
        return false;
    };
    let (value_id, source_id, dest_id) = (
        value_ty.semantic_id(),
        source_ty.semantic_id(),
        dest_ty.semantic_id(),
    );
    validation.types.equals(value_id, source_id)
        && validation.types.equals(source_id, dest_id)
        && validation.types.equals(value_id, dest_id)
}

fn concrete_shape(
    ty: MirType,
    validation: &MirValidationContext<'_>,
    span: Span,
) -> Result<ConcreteTensorShape, ElementwiseBarrierError> {
    let dims = literal_dims(ty, validation, span)?;
    let element_ty = match validation.types.get(ty.semantic_id()) {
        Type::Tensor(element, _) => MirType::semantic(*element),
        _ => ty,
    };
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .ok()
        .map(|row| row.kind);
    Ok(ConcreteTensorShape {
        dims,
        element_ty,
        layout,
    })
}

fn literal_dims(
    ty: MirType,
    validation: &MirValidationContext<'_>,
    span: Span,
) -> Result<Vec<u64>, ElementwiseBarrierError> {
    match validation.types.get(ty.semantic_id()) {
        Type::Tensor(_, _) => tensor_literal_dims(ty, validation)
            .map_err(|_| ElementwiseBarrierError::UnresolvedShape { span }),
        _ => Ok(Vec::new()),
    }
}

fn index_mapping(operand_shape: &[u64], result_shape: &[u64]) -> Option<IndexMapping> {
    if operand_shape == result_shape {
        return Some(IndexMapping::Identity);
    }
    if operand_shape.is_empty() || operand_shape == [1] {
        return Some(IndexMapping::BroadcastScalar);
    }
    if result_shape.len() == operand_shape.len() + 1 {
        let trailing = &result_shape[1..];
        if trailing == operand_shape {
            return Some(IndexMapping::RankExtension {
                operand_shape: operand_shape.to_vec(),
                result_shape: result_shape.to_vec(),
            });
        }
    }
    None
}

struct Producer {
    index: usize,
    op: ElementwiseOp,
    args: Vec<MirOperand>,
    span: Span,
}

struct PlanBuilder<'a, 'b> {
    function: &'a MirFunction,
    validation: &'a MirValidationContext<'b>,
    producers: FxHashMap<PlanValue, Producer>,
    forwards: FxHashMap<PlanValue, PureForward>,
    result_shape: Vec<u64>,
    inputs: Vec<ElementwiseInput>,
    input_slots: FxHashMap<MirLocalId, usize>,
    used: FxHashSet<usize>,
    visiting: FxHashSet<PlanValue>,
}

impl PlanBuilder<'_, '_> {
    /// The expression for one planner slot: its producer's op, a pure
    /// forward's source (the alias contributes no node), an external read
    /// for an unresolved LOCAL, or a typed barrier for a dangling temp
    /// (1A-bis F5: temps are never invented as external inputs).
    fn expr_for_slot(
        &mut self,
        value: PlanValue,
        span: Span,
    ) -> Result<ElementwiseExpr, ElementwiseBarrierError> {
        if !self.visiting.insert(value) {
            return Err(ElementwiseBarrierError::UnresolvedShape { span });
        }
        let expr = if self.producers.contains_key(&value) {
            self.expr_for_producer(value)?
        } else if let Some(forward) = self.forwards.get(&value) {
            let source = forward.source;
            self.expr_for_slot(source, span)?
        } else {
            match value {
                PlanValue::Local(local) => self.intern_input(local, span)?,
                PlanValue::Temp(_) => {
                    return Err(ElementwiseBarrierError::UnresolvedShape { span });
                }
            }
        };
        self.visiting.remove(&value);
        Ok(expr)
    }

    fn expr_for_producer(
        &mut self,
        value: PlanValue,
    ) -> Result<ElementwiseExpr, ElementwiseBarrierError> {
        let Some(producer) = self.producers.get(&value) else {
            return Err(ElementwiseBarrierError::NoCandidate {
                span: self.function.span,
            });
        };
        let index = producer.index;
        let op = producer.op;
        let args = producer.args.clone();
        let producer_span = producer.span;
        self.used.insert(index);
        let provenance = SourceSpanFact {
            span: producer_span,
            op: Some(op),
        };
        let policy = NumericalPolicy::SOURCE_ORDER;
        match op {
            ElementwiseOp::Add | ElementwiseOp::Sub | ElementwiseOp::Mul | ElementwiseOp::Div => {
                let lhs = args
                    .first()
                    .ok_or(ElementwiseBarrierError::UnresolvedShape {
                        span: producer_span,
                    })?;
                let rhs = args
                    .get(1)
                    .ok_or(ElementwiseBarrierError::UnresolvedShape {
                        span: producer_span,
                    })?;
                let lhs = self.expr_for_operand(lhs, producer_span)?;
                let rhs = self.expr_for_operand(rhs, producer_span)?;
                Ok(ElementwiseExpr::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    provenance,
                    numerical_policy: policy,
                })
            }
            ElementwiseOp::Neg
            | ElementwiseOp::Tanh
            | ElementwiseOp::Exp
            | ElementwiseOp::Sqrt
            | ElementwiseOp::F16Round
            | ElementwiseOp::Relu
            | ElementwiseOp::Gelu
            | ElementwiseOp::Fill => {
                let operand = if op == ElementwiseOp::Fill {
                    args.get(1)
                } else {
                    args.first()
                }
                .ok_or(ElementwiseBarrierError::UnresolvedShape {
                    span: producer_span,
                })?;
                let operand = self.expr_for_operand(operand, producer_span)?;
                Ok(ElementwiseExpr::Unary {
                    op,
                    operand: Box::new(operand),
                    provenance,
                    numerical_policy: policy,
                })
            }
        }
    }

    fn expr_for_operand(
        &mut self,
        operand: &MirOperand,
        span: Span,
    ) -> Result<ElementwiseExpr, ElementwiseBarrierError> {
        match operand {
            MirOperand::Constant(constant) => Ok(ElementwiseExpr::Scalar(constant.clone())),
            MirOperand::Place(place) => {
                if !place.projections.is_empty() {
                    return Err(ElementwiseBarrierError::ProjectedDestination { span });
                }
                self.expr_for_slot(PlanValue::from(place.base.clone()), span)
            }
            MirOperand::Temp(temp) => self.expr_for_slot(PlanValue::Temp(*temp), span),
            MirOperand::Value(_) => Err(ElementwiseBarrierError::UnresolvedShape { span }),
        }
    }

    fn intern_input(
        &mut self,
        local: MirLocalId,
        span: Span,
    ) -> Result<ElementwiseExpr, ElementwiseBarrierError> {
        if let Some(&slot) = self.input_slots.get(&local) {
            return Ok(ElementwiseExpr::Input {
                slot,
                index: self.inputs[slot].index.clone(),
            });
        }
        let ty = crate::local_ty::local_ty(self.function, local)
            .ok_or(ElementwiseBarrierError::UnresolvedShape { span })?;
        let shape = literal_dims(ty, self.validation, span)?;
        let index = index_mapping(&shape, &self.result_shape)
            .ok_or(ElementwiseBarrierError::UnresolvedShape { span })?;
        let slot = self.inputs.len();
        self.inputs.push(ElementwiseInput {
            local,
            ty,
            shape,
            index: index.clone(),
        });
        self.input_slots.insert(local, slot);
        Ok(ElementwiseExpr::Input { slot, index })
    }
}

#[cfg(test)]
#[path = "elementwise_plan_test.rs"]
mod tests;
