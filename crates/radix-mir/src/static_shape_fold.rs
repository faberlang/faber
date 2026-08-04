//! Static-shape folding for the device path (S5-U4).
//!
//! The device path folds the Gradus dynamic-shape construction on
//! statically-shaped tensors so a device-resident training step lowers to
//! elementwise kernels with literal shape facts:
//!
//! - [`MirCollectionOp::TensorShape`] (`magnitudines()`) on a static tensor
//!   folds to a constant `lista<numerus>` construct of the literal dims;
//! - [`MirCollectionOp::TensorCreate`] (`crea(lr, shape)`) on a static
//!   destination folds to a scalar-broadcast [`MirCollectionOp::TensorFill`]
//!   whose receiver is the **shape-source tensor** (the tensor whose
//!   `magnitudines()` supplied the shape). The destination's literal tensor
//!   type carries the shape, so the fill is device-expressible and the
//!   materializer attaches it as a version-keyed buffer shape (new shape ⇒
//!   new version — [`crate::device_program::BufferVersion`]);
//! - [`MirCollectionOp::TensorFromFlat`] on a static destination keeps its
//!   data movement and records the folded shape fact.
//!
//! The fold is **fail-closed**: an unfoldable shape — a `magnitudines()` on a
//! non-static receiver, or a create/from-flat with a non-static destination —
//! is a typed [`ShapeFoldError`], never a silent host-side shape.
//!
//! The fold is **semantics-preserving**: `TensorShape(t)` becomes the constant
//! dims of `t`; `crea(lr, t.magnitudines())` becomes `TensorFill(t, lr)` (the
//! fill broadcasts the scalar over `t`'s shape, exactly the create's source
//! meaning); dead fold artifacts (folded shape lists and vacua seed templates
//! that only fed folded creates) are dropped, so the folded body carries only
//! device-expressible elementwise ops plus the multi-output tuple tail.
//!
//! # Device materialization admission
//!
//! [`admit_device_shape_materialization`] folds a device-resident function and
//! verifies the folded body still lowers through the shared plan surface
//! ([`crate::device_program_plans::kernel_plan_for_function`]) before
//! admitting it, returning the folded shape facts the constructor attaches to
//! output buffers.

use crate::abi::MirKernelSignature;
use crate::device_program_plans::kernel_plan_for_function;
use crate::figura::literal_index_i64s;
use crate::kernel_plan::CollectionKernelPlan;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirCollectionOp,
    MirConstant, MirFunction, MirIntrinsic, MirLocalId, MirOperand, MirPlace, MirPlaceBase,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTerminatorKind, MirType,
    MirValidationContext, MirValue, MirValueKind,
};
use radix_types::Type;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Stable category of a shape-fold failure (S5-U4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeFoldErrorKind {
    /// `magnitudines()` on a tensor with no literal shape (dynamic shape).
    UnfoldableMagnitudines,
    /// A shape-construction destination with no literal tensor shape.
    NonStaticShapeDestination,
    /// The folded body does not lower through the shared plan surface.
    NotDeviceMaterializable,
}

impl ShapeFoldErrorKind {
    /// Short diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::UnfoldableMagnitudines => "unfoldable magnitudines",
            Self::NonStaticShapeDestination => "non-static shape destination",
            Self::NotDeviceMaterializable => "not device materializable",
        }
    }
}

/// A typed fail-closed diagnostic for an unfoldable or non-device-expressible
/// shape (S5-U4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeFoldError {
    /// Stable failure category.
    pub kind: ShapeFoldErrorKind,
    /// Human-readable detail.
    pub message: String,
}

impl ShapeFoldError {
    fn new(kind: ShapeFoldErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ShapeFoldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.spelling(), self.message)
    }
}

impl std::error::Error for ShapeFoldError {}

/// Which shape construction produced a folded shape fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeFactSource {
    /// `magnitudines()` on a static tensor folded to its literal dims.
    Magnitudines,
    /// `crea(lr, shape)` folded to a scalar-broadcast fill.
    Crea,
    /// `TensorFromFlat` with a foldable shape.
    FromFlat,
}

/// One folded literal shape (S5-U4): the local whose value carries the
/// shape, the literal dims, and the flat device-view facts the materializer
/// attaches to a version-keyed buffer (new shape ⇒ new version).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldedShapeFact {
    /// The local whose value carries this shape.
    pub local: MirLocalId,
    /// Literal dims (rank = `dims.len()`).
    pub dims: Vec<i64>,
    /// Element type of the shape's tensor.
    pub element_ty: MirType,
    /// Flat element count (product of the literal dims).
    pub element_count: u64,
    /// Which shape construction folded.
    pub source: ShapeFactSource,
}

/// The folded shape facts of one function (S5-U4).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShapeFoldReport {
    /// The folded shape facts in statement order.
    pub facts: Vec<FoldedShapeFact>,
}

/// The result of folding one function: the folded function plus the folded
/// shape facts.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeFoldOutcome {
    /// The folded function (all shape ops folded; dead fold artifacts dropped).
    pub function: MirFunction,
    /// The folded shape facts.
    pub report: ShapeFoldReport,
}

/// The device-materialization admission decision for a shape-folded function
/// (S5-U4).
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeFoldedAdmission {
    /// The folded function (the body the constructor plans).
    pub function: MirFunction,
    /// The kernel plan the folded body resolves to (elementwise when no
    /// recipe op remains).
    pub plan: CollectionKernelPlan,
    /// The folded shape facts (attached as version-keyed buffer shapes).
    pub facts: Vec<FoldedShapeFact>,
}

/// A traced `magnitudines()` result: the local, its literal dims, and the
/// tensor local it was taken from (the fill receiver source).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ShapeSource {
    dims: Vec<i64>,
    source_receiver: Option<MirLocalId>,
}

/// Fold the shape-construction ops of one device-resident function (S5-U4).
///
/// `TensorShape` on a static tensor folds to a constant dims construct;
/// `TensorCreate` with a static destination folds to a scalar-broadcast
/// `TensorFill` whose receiver is the shape-source tensor; `TensorFromFlat`
/// with a static destination keeps its data movement and records a folded
/// shape fact. Folded shape lists and vacua seed templates that no longer
/// carry a live value are dropped from the body.
///
/// # Errors
/// Returns [`ShapeFoldError`] when a shape op cannot fold: a `magnitudines()`
/// whose receiver has no literal shape (`UnfoldableMagnitudines`) or a
/// shape-construction destination with no literal tensor shape
/// (`NonStaticShapeDestination`). Fail-closed: never a silent host-side shape.
pub fn fold_static_shapes(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<ShapeFoldOutcome, ShapeFoldError> {
    // Pass 1: resolve every shape op's literal shape and trace
    // `magnitudines()` results to their shape-source tensor. Any unfoldable
    // shape stops the fold with a typed diagnostic.
    let mut shape_sources: BTreeMap<MirLocalId, ShapeSource> = BTreeMap::new();
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::RuntimeCall { destination, call } = &statement.kind else {
                continue;
            };
            let Some(local) = destination_local(destination.as_ref()) else {
                continue;
            };
            match call.intrinsic {
                MirIntrinsic::Collection(MirCollectionOp::TensorShape) => {
                    let dims = shape_receiver_dims(call, function, validation)?;
                    shape_sources.insert(
                        local,
                        ShapeSource {
                            dims,
                            source_receiver: operand_local(call.args.first()),
                        },
                    );
                }
                MirIntrinsic::Collection(
                    MirCollectionOp::TensorCreate | MirCollectionOp::TensorFromFlat,
                ) => {
                    let dims =
                        tensor_literal_dims(call.return_ty, validation).ok_or_else(|| {
                            ShapeFoldError::new(
                                ShapeFoldErrorKind::NonStaticShapeDestination,
                                format!(
                                    "shape-construction destination local {} has no literal \
                                 tensor shape (dynamic shape is not device-expressible)",
                                    local.0
                                ),
                            )
                        })?;
                    shape_sources.insert(
                        local,
                        ShapeSource {
                            dims,
                            source_receiver: None,
                        },
                    );
                }
                _ => {}
            }
        }
    }

    // Pass 2: rewrite the statements, collecting the folded shape facts.
    let mut facts: Vec<FoldedShapeFact> = Vec::new();
    let mut blocks = Vec::with_capacity(function.blocks.len());
    for block in &function.blocks {
        let mut folded_block = block.clone();
        folded_block.statements = block
            .statements
            .iter()
            .map(|statement| {
                fold_statement(statement, &shape_sources, function, validation, &mut facts)
            })
            .collect::<Result<Vec<_>, _>>()?;
        blocks.push(folded_block);
    }

    // Pass 3: drop the fold artifacts that no longer carry a live value
    // (folded shape lists and vacua seed templates whose only consumers were
    // the folded creates). Reads resolve against the whole folded body, so a
    // shape list consumed outside the folded creates stays in the body (and
    // the device pipeline fails closed on it there).
    let mut folded = function.clone();
    folded.blocks = blocks;
    let read_set = local_read_set(&folded);
    for block in &mut folded.blocks {
        block.statements.retain(|statement| {
            if !is_fold_artifact(statement) {
                return true;
            }
            match statement_destination_local(statement) {
                // A fold artifact stays only while its value is still read
                // (a shape list consumed outside the folded creates, or a
                // vacua template still referenced by an unfolded create).
                None => true,
                Some(local) => read_set.contains(&local),
            }
        });
    }

    Ok(ShapeFoldOutcome {
        function: folded,
        report: ShapeFoldReport { facts },
    })
}

/// Admit a device-resident function for device materialization: fold its
/// shape construction and verify the folded body lowers through the shared
/// plan surface (S5-U4).
///
/// Returns the folded function, the kernel plan it resolves to, and the
/// folded shape facts the constructor attaches to version-keyed output
/// buffers (new shape ⇒ new version).
///
/// # Errors
/// Returns [`ShapeFoldError`] when a shape op cannot fold
/// (`UnfoldableMagnitudines` / `NonStaticShapeDestination`) or the folded
/// body does not derive a kernel signature / plan (`NotDeviceMaterializable`;
/// the plan pass's typed diagnostic is preserved in the message).
pub fn admit_device_shape_materialization(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<ShapeFoldedAdmission, ShapeFoldError> {
    let outcome = fold_static_shapes(function, validation)?;
    let signature = MirKernelSignature::storage_buffer_kernel(&outcome.function, validation)
        .map_err(|error| {
            ShapeFoldError::new(
                ShapeFoldErrorKind::NotDeviceMaterializable,
                format!("kernel signature after shape folding: {}", error.message),
            )
        })?;
    let plan =
        kernel_plan_for_function(&outcome.function, &signature, validation).map_err(|error| {
            ShapeFoldError::new(
                ShapeFoldErrorKind::NotDeviceMaterializable,
                format!("kernel plan after shape folding: {}", error.message),
            )
        })?;
    Ok(ShapeFoldedAdmission {
        function: outcome.function,
        plan: plan.unwrap_or(CollectionKernelPlan::Elementwise),
        facts: outcome.report.facts,
    })
}

/// Rewrite one statement through the fold. Non-shape statements pass through
/// unchanged; `TensorShape` becomes a constant dims construct; `TensorCreate`
/// becomes a scalar-broadcast `TensorFill`; `TensorFromFlat` is kept (its
/// data movement) with a folded shape fact recorded.
fn fold_statement(
    statement: &MirStatement,
    shape_sources: &BTreeMap<MirLocalId, ShapeSource>,
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    facts: &mut Vec<FoldedShapeFact>,
) -> Result<MirStatement, ShapeFoldError> {
    let MirStatementKind::RuntimeCall { destination, call } = &statement.kind else {
        return Ok(statement.clone());
    };
    let MirIntrinsic::Collection(op) = call.intrinsic else {
        return Ok(statement.clone());
    };
    match op {
        MirCollectionOp::TensorShape => {
            let traced = destination_local(destination.as_ref())
                .and_then(|local| shape_sources.get(&local).map(|source| source.dims.clone()));
            let dims = match traced {
                Some(dims) => dims,
                // Temp destination (no traced source): resolve the receiver's
                // literal shape directly.
                None => shape_receiver_dims(call, function, validation)?,
            };
            let Some(place) = destination else {
                return Ok(statement.clone());
            };
            if let Some(local) = destination_local(destination.as_ref()) {
                facts.push(FoldedShapeFact {
                    local,
                    dims: dims.clone(),
                    element_ty: receiver_tensor_element_ty(call, function, validation)?,
                    element_count: element_count_for_dims(&dims)?,
                    source: ShapeFactSource::Magnitudines,
                });
            }
            Ok(MirStatement {
                kind: MirStatementKind::Construct {
                    destination: place.clone(),
                    aggregate: shape_lista_construct(call.return_ty, &dims),
                },
                span: statement.span,
            })
        }
        MirCollectionOp::TensorCreate => {
            let Some(local) = destination_local(destination.as_ref()) else {
                return Err(ShapeFoldError::new(
                    ShapeFoldErrorKind::NonStaticShapeDestination,
                    "tensor_create destination is not a local; cannot fold to a fill kernel",
                ));
            };
            let dims = tensor_literal_dims(call.return_ty, validation).ok_or_else(|| {
                ShapeFoldError::new(
                    ShapeFoldErrorKind::NonStaticShapeDestination,
                    format!(
                        "crea destination local {} has no literal tensor shape \
                         (dynamic shape is not device-expressible)",
                        local.0
                    ),
                )
            })?;
            let fill_value = call.args.get(1).cloned().ok_or_else(|| {
                ShapeFoldError::new(
                    ShapeFoldErrorKind::NonStaticShapeDestination,
                    format!("crea at local {} has no scalar fill operand", local.0),
                )
            })?;
            let receiver = fill_receiver_for_create(call, shape_sources, &dims);
            facts.push(FoldedShapeFact {
                local,
                dims: dims.clone(),
                element_ty: tensor_element_ty(call.return_ty, validation)?,
                element_count: element_count_for_dims(&dims)?,
                source: ShapeFactSource::Crea,
            });
            Ok(MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: destination.clone(),
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorFill),
                        args: vec![receiver, fill_value],
                        return_ty: call.return_ty,
                    },
                },
                span: statement.span,
            })
        }
        MirCollectionOp::TensorFromFlat => {
            let Some(local) = destination_local(destination.as_ref()) else {
                return Err(ShapeFoldError::new(
                    ShapeFoldErrorKind::NonStaticShapeDestination,
                    "tensor_from_flat destination is not a local; cannot fold its shape",
                ));
            };
            let dims = tensor_literal_dims(call.return_ty, validation).ok_or_else(|| {
                ShapeFoldError::new(
                    ShapeFoldErrorKind::NonStaticShapeDestination,
                    format!(
                        "tensor_from_flat destination local {} has no literal tensor shape \
                         (dynamic shape is not device-expressible)",
                        local.0
                    ),
                )
            })?;
            facts.push(FoldedShapeFact {
                local,
                dims: dims.clone(),
                element_ty: tensor_element_ty(call.return_ty, validation)?,
                element_count: element_count_for_dims(&dims)?,
                source: ShapeFactSource::FromFlat,
            });
            Ok(statement.clone())
        }
        _ => Ok(statement.clone()),
    }
}

/// The literal dims of a `TensorShape` receiver, or a typed
/// `UnfoldableMagnitudines` diagnostic.
fn shape_receiver_dims(
    call: &MirRuntimeCall,
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<Vec<i64>, ShapeFoldError> {
    let Some(receiver) = call.args.first() else {
        return Err(ShapeFoldError::new(
            ShapeFoldErrorKind::UnfoldableMagnitudines,
            "magnitudines() has no receiver operand",
        ));
    };
    let Some(ty) = operand_local_ty(receiver, function) else {
        return Err(ShapeFoldError::new(
            ShapeFoldErrorKind::UnfoldableMagnitudines,
            "magnitudines() receiver has no resolvable MIR type",
        ));
    };
    let Some(dims) = tensor_literal_dims(ty, validation) else {
        return Err(ShapeFoldError::new(
            ShapeFoldErrorKind::UnfoldableMagnitudines,
            "magnitudines() receiver has no literal tensor shape (dynamic shape; \
             a statically-shaped tensor is required for the device path)",
        ));
    };
    Ok(dims)
}

/// The element type of the tensor a `TensorShape` receiver holds.
fn receiver_tensor_element_ty(
    call: &MirRuntimeCall,
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<MirType, ShapeFoldError> {
    let Some(receiver) = call.args.first() else {
        return Err(ShapeFoldError::new(
            ShapeFoldErrorKind::UnfoldableMagnitudines,
            "magnitudines() has no receiver operand",
        ));
    };
    let Some(ty) = operand_local_ty(receiver, function) else {
        return Err(ShapeFoldError::new(
            ShapeFoldErrorKind::UnfoldableMagnitudines,
            "magnitudines() receiver has no resolvable MIR type",
        ));
    };
    tensor_element_ty(ty, validation)
}

/// The fill receiver for a folded `TensorCreate`: the shape-source tensor
/// (the tensor whose `magnitudines()` supplied the shape) when it agrees with
/// the destination's literal shape; otherwise the create's original receiver
/// (the vacua seed template).
fn fill_receiver_for_create(
    call: &MirRuntimeCall,
    shape_sources: &BTreeMap<MirLocalId, ShapeSource>,
    destination_dims: &[i64],
) -> MirOperand {
    let traced = call
        .args
        .get(2)
        .and_then(|operand| operand_local(Some(operand)))
        .and_then(|local| shape_sources.get(&local))
        .and_then(|source| {
            if source.dims == destination_dims {
                source
                    .source_receiver
                    .map(MirPlace::local)
                    .map(MirOperand::Place)
            } else {
                None
            }
        });
    traced.unwrap_or_else(|| call.args[0].clone())
}

/// A constant `lista<numerus>` construct of the literal dims (the folded
/// `magnitudines()` result).
fn shape_lista_construct(ty: MirType, dims: &[i64]) -> MirAggregate {
    MirAggregate {
        kind: MirAggregateKind::Array,
        ty,
        fields: MirAggregateFields::Ordered(
            dims.iter()
                .map(|&dim| MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(dim))))
                .collect(),
        ),
    }
}

/// The literal dims of a tensor type, or `None` for a dynamic shape.
fn tensor_literal_dims(ty: MirType, validation: &MirValidationContext<'_>) -> Option<Vec<i64>> {
    let Type::Tensor(_, shape) = validation.types.get(ty.semantic_id()) else {
        return None;
    };
    literal_index_i64s(*shape, validation.types)
}

/// The element type of a tensor type.
fn tensor_element_ty(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<MirType, ShapeFoldError> {
    match validation.types.get(ty.semantic_id()) {
        Type::Tensor(element, _) => Ok(MirType::semantic(*element)),
        _ => Err(ShapeFoldError::new(
            ShapeFoldErrorKind::NonStaticShapeDestination,
            "shape destination type is not a tensor",
        )),
    }
}

/// The flat element count of a literal dims list (product of the dims).
fn element_count_for_dims(dims: &[i64]) -> Result<u64, ShapeFoldError> {
    let mut product: u64 = 1;
    for &dim in dims {
        let count = u64::try_from(dim).map_err(|_| {
            ShapeFoldError::new(
                ShapeFoldErrorKind::NonStaticShapeDestination,
                format!(
                    "shape dimension {dim} is negative or too large for a device element count"
                ),
            )
        })?;
        product = product.saturating_mul(count);
    }
    Ok(product)
}

/// Whether a statement is a fold artifact eligible for dead-artifact removal:
/// a folded shape-lista construct or a vacua seed template construct.
fn is_fold_artifact(statement: &MirStatement) -> bool {
    let MirStatementKind::Construct { aggregate, .. } = &statement.kind else {
        return false;
    };
    matches!(
        aggregate.kind,
        MirAggregateKind::Array | MirAggregateKind::Tensor
    )
}

/// Every local read anywhere in the function (statements and terminators,
/// across all blocks). Destinations are writes, never reads.
fn local_read_set(function: &MirFunction) -> BTreeSet<MirLocalId> {
    let mut read = BTreeSet::new();
    for block in &function.blocks {
        for statement in &block.statements {
            statement_read_locals(statement, &mut read);
        }
        terminator_read_locals(&block.terminator.kind, &mut read);
    }
    read
}

/// Collect the locals a statement reads into `out`.
fn statement_read_locals(statement: &MirStatement, out: &mut BTreeSet<MirLocalId>) {
    match &statement.kind {
        MirStatementKind::Assign { value, .. } => value_read_locals(value, out),
        MirStatementKind::RuntimeCall { call, .. } => operand_read_locals(&call.args, out),
        MirStatementKind::Call { args, .. } => operand_read_locals(args, out),
        MirStatementKind::Construct { aggregate, .. } => aggregate_read_locals(aggregate, out),
    }
}

/// Collect the locals a terminator reads into `out`.
fn terminator_read_locals(terminator: &MirTerminatorKind, out: &mut BTreeSet<MirLocalId>) {
    match terminator {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            operand_read_local(operand, out);
        }
        MirTerminatorKind::Branch { condition, .. }
        | MirTerminatorKind::Switch {
            value: condition, ..
        } => operand_read_local(condition, out),
        MirTerminatorKind::TryCall { args, .. } => operand_read_locals(args, out),
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

/// Collect the locals an operand slice reads into `out`.
fn operand_read_locals(operands: &[MirOperand], out: &mut BTreeSet<MirLocalId>) {
    for operand in operands {
        operand_read_local(operand, out);
    }
}

/// Collect the local a single operand reads into `out`, when it is a local
/// place.
fn operand_read_local(operand: &MirOperand, out: &mut BTreeSet<MirLocalId>) {
    if let Some(local) = operand_local(Some(operand)) {
        out.insert(local);
    }
}

/// Collect the locals a value reads into `out`.
fn value_read_locals(value: &MirValue, out: &mut BTreeSet<MirLocalId>) {
    match &value.kind {
        MirValueKind::Operand(operand) | MirValueKind::Unary { operand, .. } => {
            operand_read_local(operand, out);
        }
        MirValueKind::Binary { lhs, rhs, .. } => {
            operand_read_local(lhs, out);
            operand_read_local(rhs, out);
        }
        MirValueKind::Option(op) => option_read_locals(op, out),
        MirValueKind::Closure(closure) => operand_read_local(&closure.environment, out),
    }
}

/// Collect the locals an option operation reads into `out`.
fn option_read_locals(op: &crate::MirOptionOp, out: &mut BTreeSet<MirLocalId>) {
    match op {
        crate::MirOptionOp::Some(operand)
        | crate::MirOptionOp::IsNil(operand)
        | crate::MirOptionOp::IsNotNil(operand) => operand_read_local(operand, out),
        crate::MirOptionOp::Unwrap { value, .. } => operand_read_local(value, out),
        crate::MirOptionOp::Coalesce { value, fallback } => {
            operand_read_local(value, out);
            operand_read_local(fallback, out);
        }
        crate::MirOptionOp::Chain { base, .. } => operand_read_local(base, out),
        crate::MirOptionOp::None => {}
    }
}

/// Collect the locals an aggregate construct reads into `out` (its field
/// operands; the destination is a write).
fn aggregate_read_locals(aggregate: &MirAggregate, out: &mut BTreeSet<MirLocalId>) {
    match &aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                let (MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand)) = item;
                operand_read_local(operand, out);
            }
        }
        MirAggregateFields::Named(fields) => {
            for field in fields {
                operand_read_local(&field.value, out);
            }
        }
        MirAggregateFields::Keyed(fields) => {
            for field in fields {
                operand_read_local(&field.key, out);
                operand_read_local(&field.value, out);
            }
        }
    }
}

/// The destination local of a statement, when it writes a local place.
fn statement_destination_local(statement: &MirStatement) -> Option<MirLocalId> {
    let destination: Option<&MirPlace> = match &statement.kind {
        MirStatementKind::Assign { place, .. }
        | MirStatementKind::Construct {
            destination: place, ..
        } => Some(place),
        MirStatementKind::RuntimeCall { destination, .. }
        | MirStatementKind::Call {
            destination,
            callee: _,
            args: _,
        } => destination.as_ref(),
    };
    destination.and_then(|place| match place.base {
        MirPlaceBase::Local(local) => Some(local),
        MirPlaceBase::Temp(_) => None,
    })
}

/// The local base of a place operand, when it is a local place.
fn operand_local(operand: Option<&MirOperand>) -> Option<MirLocalId> {
    let MirOperand::Place(place) = operand? else {
        return None;
    };
    match place.base {
        MirPlaceBase::Local(local) => Some(local),
        MirPlaceBase::Temp(_) => None,
    }
}

/// The destination local of a place, when it is a local place.
fn destination_local(destination: Option<&MirPlace>) -> Option<MirLocalId> {
    let place = destination?;
    match place.base {
        MirPlaceBase::Local(local) => Some(local),
        MirPlaceBase::Temp(_) => None,
    }
}

/// The MIR type of a local (params first, then locals).
fn operand_local_ty(operand: &MirOperand, function: &MirFunction) -> Option<MirType> {
    let local = operand_local(Some(operand))?;
    function
        .params
        .iter()
        .find(|param| param.local == local)
        .map(|param| param.ty)
        .or_else(|| {
            function
                .locals
                .iter()
                .find(|entry| entry.id == local)
                .map(|entry| entry.ty)
        })
}

#[cfg(test)]
#[path = "static_shape_fold_test.rs"]
mod tests;
