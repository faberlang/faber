//! Target-neutral elementwise expression plan for device MIR (OF-1).
//!
//! The plan is a typed side structure beside MIR device planning. It is built
//! from existing statement, classifier, type, layout, span, and read facts.
//! Classification is consulted from [`crate::device_program_plans`] /
//! `kernel_plan/`; this module never re-derives the recipe/elementwise sets
//! and never treats [`crate::kernel_plan::CollectionKernelPlan::Elementwise`]
//! as a silent fallback.

use crate::device_program_plans::{is_elementwise_collection_op, is_recipe_op};
use crate::tensor_type::tensor_literal_dims;
use crate::{
    collection_ops::statement_collection_op, MirCollectionOp, MirConstant, MirFunction,
    MirLayoutKind, MirLocalId, MirOperand, MirPlace, MirStatement, MirStatementKind, MirType,
    MirValidationContext,
};
use radix_lexer::Span;
use radix_types::Type;
use rustc_hash::{FxHashMap, FxHashSet};
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
/// Membership is decided by [`is_elementwise_collection_op`]; this enum only
/// names an already-classified op.
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
    /// [`MirCollectionOp::TensorRelu`].
    Relu,
    /// [`MirCollectionOp::TensorGelu`].
    Gelu,
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
    /// consumer's statement-coverage fact (PGC-R6 / OF-3).
    pub composed_locals: Vec<MirLocalId>,
}

impl ElementwisePlan {
    /// Whether `local` is written by this plan (an intermediate producer
    /// or the composed output) — the emit-side coverage check.
    #[must_use]
    pub fn covers_local(&self, local: MirLocalId) -> bool {
        local == self.output.local || self.composed_locals.contains(&local)
    }
}

/// Typed reason a function is not a phase-one elementwise plan.
#[derive(Debug, Clone, PartialEq, Eq)]
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
        /// The shared intermediate.
        local: MirLocalId,
        /// Span of a consumer or the producer.
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
            Self::MultiConsumer { local, .. } => {
                write!(f, "intermediate {local:?} has multiple live consumers")
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
    let consumer_counts = consumer_counts(function);
    let mut producers = FxHashMap::default();
    for (index, statement) in block.statements.iter().enumerate() {
        let dest_place = statement_destination_place(statement);
        if dest_place.is_some_and(|place| !place.projections.is_empty()) {
            return Err(ElementwiseBarrierError::ProjectedDestination {
                span: statement.span,
            });
        }
        let Some(op) = statement_collection_op(statement) else {
            if let Some(local) = crate::read_analysis::statement_destination(statement) {
                if consumer_counts.get(&local).copied().unwrap_or(0) > 0 {
                    return Err(ElementwiseBarrierError::Effect {
                        span: statement.span,
                    });
                }
            }
            continue;
        };
        let dest = crate::read_analysis::statement_destination(statement);
        if dest.is_some_and(|local| {
            !crate::read_analysis::place_is_read(function, &MirPlace::local(local))
        }) {
            continue;
        }
        let classified = classify_collection_op(op, statement.span)?;
        let Some(local) = dest else {
            return Err(ElementwiseBarrierError::UnresolvedShape {
                span: statement.span,
            });
        };
        let count = consumer_counts.get(&local).copied().unwrap_or(0);
        if count > 1 {
            return Err(ElementwiseBarrierError::MultiConsumer {
                local,
                span: statement.span,
            });
        }
        let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
            return Err(ElementwiseBarrierError::Effect {
                span: statement.span,
            });
        };
        producers.insert(
            local,
            Producer {
                index,
                op: classified,
                args: call.args.clone(),
                dest_ty: call.return_ty,
                span: statement.span,
            },
        );
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
    let Some(output_producer) = producers.get(&output_local) else {
        return Err(ElementwiseBarrierError::NoCandidate {
            span: function.span,
        });
    };
    let output_ty = output_producer.dest_ty;
    let result_shape = concrete_shape(output_ty, validation, function.span)?;
    let mut builder = PlanBuilder {
        function,
        validation,
        producers,
        result_shape: result_shape.dims.clone(),
        inputs: Vec::new(),
        input_slots: FxHashMap::default(),
        used: FxHashSet::default(),
        visiting: FxHashSet::default(),
    };
    let expression = builder.expr_for_local(output_local, function.span)?;
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
    let mut composed_locals: Vec<MirLocalId> = builder.producers.keys().copied().collect();
    composed_locals.sort_unstable();
    composed_locals.dedup();
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
        _ => None,
    }
}

fn statement_destination_place(statement: &MirStatement) -> Option<&MirPlace> {
    match &statement.kind {
        MirStatementKind::Assign { place, .. }
        | MirStatementKind::Construct {
            destination: place, ..
        } => Some(place),
        MirStatementKind::RuntimeCall { destination, .. }
        | MirStatementKind::Call { destination, .. } => destination.as_ref(),
    }
}

fn consumer_counts(function: &MirFunction) -> FxHashMap<MirLocalId, usize> {
    let mut counts = FxHashMap::default();
    for block in &function.blocks {
        for statement in &block.statements {
            for local in crate::read_analysis::statement_read_locals(statement) {
                *counts.entry(local).or_insert(0) += 1;
            }
        }
        for local in crate::read_analysis::terminator_read_locals(&block.terminator.kind) {
            *counts.entry(local).or_insert(0) += 1;
        }
    }
    counts
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
    dest_ty: MirType,
    span: Span,
}

struct PlanBuilder<'a, 'b> {
    function: &'a MirFunction,
    validation: &'a MirValidationContext<'b>,
    producers: FxHashMap<MirLocalId, Producer>,
    result_shape: Vec<u64>,
    inputs: Vec<ElementwiseInput>,
    input_slots: FxHashMap<MirLocalId, usize>,
    used: FxHashSet<usize>,
    visiting: FxHashSet<MirLocalId>,
}

impl PlanBuilder<'_, '_> {
    fn expr_for_local(
        &mut self,
        local: MirLocalId,
        span: Span,
    ) -> Result<ElementwiseExpr, ElementwiseBarrierError> {
        if !self.visiting.insert(local) {
            return Err(ElementwiseBarrierError::UnresolvedShape { span });
        }
        let expr = if self.producers.contains_key(&local) {
            self.expr_for_producer(local)?
        } else {
            self.intern_input(local, span)?
        };
        self.visiting.remove(&local);
        Ok(expr)
    }

    fn expr_for_producer(
        &mut self,
        local: MirLocalId,
    ) -> Result<ElementwiseExpr, ElementwiseBarrierError> {
        let Some(producer) = self.producers.get(&local) else {
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
                let Some(local) = crate::read_analysis::place_local_base(place) else {
                    return Err(ElementwiseBarrierError::UnresolvedShape { span });
                };
                self.expr_for_local(local, span)
            }
            MirOperand::Temp(_) | MirOperand::Value(_) => {
                Err(ElementwiseBarrierError::UnresolvedShape { span })
            }
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
