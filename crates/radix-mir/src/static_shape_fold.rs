//! Static-shape folding for the device path (S5-U4).
//!
//! The device path folds the Gradus dynamic-shape construction on
//! statically-shaped tensors so a device-resident training step lowers to
//! elementwise kernels with literal shape facts:
//!
//! - [`MirCollectionOp::TensorShape`] (`magnitudines()`) on a static tensor
//!   folds to a constant `lista<numerus>` construct of the literal dims;
//! - [`MirCollectionOp::TensorExpand`] on a static `TensorShape` witness
//!   uses the source tensor as a checked shape carrier; its consumed shape
//!   list is erased and the explicit expansion's index map remains live;
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
use crate::figura::i64s_from_literal_index;
use crate::kernel_plan::CollectionKernelPlan;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirCollectionOp,
    MirConstant, MirFunction, MirFunctionId, MirIntrinsic, MirLocalId, MirOperand, MirPlace,
    MirPlaceBase, MirRuntimeCall, MirStatement, MirStatementKind, MirTempId, MirType,
    MirValidationContext, MirValueKind, ValidatedMir,
};
use radix_types::Type;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Stable category of a shape-fold failure (S5-U4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeFoldKind {
    /// `magnitudines()` on a tensor with no literal shape (dynamic shape).
    UnfoldableMagnitudines,
    /// A shape-construction destination with no literal tensor shape.
    NonStaticShapeDestination,
    /// The folded body does not lower through the shared plan surface.
    NotDeviceMaterializable,
}

impl ShapeFoldKind {
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
    pub kind: ShapeFoldKind,
    /// Human-readable detail.
    pub message: String,
}

impl ShapeFoldError {
    fn new(kind: ShapeFoldKind, message: impl Into<String>) -> Self {
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
    let expansion_folded = crate::tensor_expansion::fold_expansion_witnesses(function, validation)?;
    let function = &expansion_folded;
    // The lowered body's call-result copies: a runtime call lands in a temp
    // and the next statement copies it into a local (`Local(l) ← Temp(t)`).
    // Shape construction is no exception (`shape_w ← weight.magnitudines()`,
    // `lr_fill ← seed.crea(...)`), so the fold traces a temp destination
    // through the copy to the local that carries the value.
    let mut temp_to_local: BTreeMap<MirTempId, MirLocalId> = BTreeMap::new();
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            let MirPlaceBase::Local(local) = place.base else {
                continue;
            };
            if !place.projections.is_empty() {
                continue;
            }
            let MirValueKind::Operand(MirOperand::Temp(temp)) = &value.kind else {
                continue;
            };
            temp_to_local.insert(*temp, local);
        }
    }

    // Pass 1: resolve every shape op's literal shape and trace
    // `magnitudines()` results to their shape-source tensor. Any unfoldable
    // shape stops the fold with a typed diagnostic.
    let shape_sources = resolve_shape_sources(function, validation, &temp_to_local)?;

    // Pass 2: rewrite the statements, collecting the folded shape facts.
    let mut facts: Vec<FoldedShapeFact> = Vec::new();
    let mut blocks = Vec::with_capacity(function.blocks.len());
    for block in &function.blocks {
        let mut folded_block = block.clone();
        folded_block.statements = block
            .statements
            .iter()
            .map(|statement| {
                fold_statement(
                    statement,
                    &shape_sources,
                    &temp_to_local,
                    function,
                    validation,
                    &mut facts,
                )
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
    let read_set = crate::read_analysis::function_read_locals(&folded);
    for block in &mut folded.blocks {
        block.statements.retain(|statement| {
            if !is_fold_artifact(statement) {
                return true;
            }
            match crate::read_analysis::statement_destination(statement) {
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

/// Resolve every shape op's literal shape and trace `magnitudines()` results
/// to their shape-source tensor. Any unfoldable shape is a typed diagnostic.
fn resolve_shape_sources(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    temp_to_local: &BTreeMap<MirTempId, MirLocalId>,
) -> Result<BTreeMap<MirLocalId, ShapeSource>, ShapeFoldError> {
    let mut shape_sources: BTreeMap<MirLocalId, ShapeSource> = BTreeMap::new();
    for block in &function.blocks {
        for statement in &block.statements {
            let Some(op) = crate::collection_ops::statement_collection_op(statement) else {
                continue;
            };
            let MirStatementKind::RuntimeCall { destination, call } = &statement.kind else {
                continue;
            };
            let Some(local) = destination_local_traced(destination.as_ref(), temp_to_local) else {
                continue;
            };
            match op {
                MirCollectionOp::TensorShape => {
                    let dims = shape_receiver_dims(call, function, validation)?;
                    shape_sources.insert(
                        local,
                        ShapeSource {
                            dims,
                            source_receiver: call
                                .args
                                .first()
                                .and_then(crate::read_analysis::operand_local),
                        },
                    );
                }
                MirCollectionOp::TensorCreate | MirCollectionOp::TensorFromFlat => {
                    let dims =
                        tensor_literal_dims(call.return_ty, validation).ok_or_else(|| {
                            ShapeFoldError::new(
                                ShapeFoldKind::NonStaticShapeDestination,
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
    Ok(shape_sources)
}

/// Fold every selected device-resident function in one validated MIR program.
///
/// The caller supplies the complete device-kernel identity set because some
/// device kernels are ordinary compute functions while others are generated
/// companions or training-step functions. Every selected function is visited
/// exactly once; functions without shape-construction operations pass through
/// unchanged. The returned token is revalidated whenever a body changes.
///
/// # Errors
/// Returns the first fail-closed [`ShapeFoldError`] from a selected function,
/// or when the resulting folded program no longer validates.
pub fn fold_validated_device_program<'a>(
    validated: &ValidatedMir<'a>,
    device_function_ids: &BTreeSet<MirFunctionId>,
) -> Result<ValidatedMir<'a>, ShapeFoldError> {
    let mut program = validated.program().clone();
    let mut changed = false;
    for function in &mut program.functions {
        if !device_function_ids.contains(&function.id) {
            continue;
        }
        let outcome = fold_static_shapes(function, validated.validation())?;
        changed |= outcome.function != *function;
        *function = outcome.function;
    }
    if !changed {
        return Ok(validated.clone());
    }
    ValidatedMir::new(program, validated.validation().clone()).map_err(|errors| {
        ShapeFoldError::new(
            ShapeFoldKind::NotDeviceMaterializable,
            format!(
                "device shape fold produced invalid MIR: {}",
                errors
                    .iter()
                    .map(|error| error.message.as_str())
                    .collect::<Vec<_>>()
                    .join(" | ")
            ),
        )
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
                ShapeFoldKind::NotDeviceMaterializable,
                format!("kernel signature after shape folding: {}", error.message),
            )
        })?;
    let plan =
        kernel_plan_for_function(&outcome.function, &signature, validation).map_err(|error| {
            ShapeFoldError::new(
                ShapeFoldKind::NotDeviceMaterializable,
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
    temp_to_local: &BTreeMap<MirTempId, MirLocalId>,
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
            let traced = destination_local_traced(destination.as_ref(), temp_to_local)
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
            if let Some(local) = destination_local_traced(destination.as_ref(), temp_to_local) {
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
            // The folded fill keeps the ORIGINAL destination (a temp is
            // fine — the lowered body's copy into the traced local follows);
            // the traced local is the value identity the fold fact names.
            let traced_local = destination_local_traced(destination.as_ref(), temp_to_local);
            let Some(local) = traced_local else {
                return Err(ShapeFoldError::new(
                    ShapeFoldKind::NonStaticShapeDestination,
                    "tensor_create destination is not a local; cannot fold to a fill kernel",
                ));
            };
            let dims = tensor_literal_dims(call.return_ty, validation).ok_or_else(|| {
                ShapeFoldError::new(
                    ShapeFoldKind::NonStaticShapeDestination,
                    format!(
                        "crea destination local {} has no literal tensor shape \
                         (dynamic shape is not device-expressible)",
                        local.0
                    ),
                )
            })?;
            let fill_value = call.args.get(1).cloned().ok_or_else(|| {
                ShapeFoldError::new(
                    ShapeFoldKind::NonStaticShapeDestination,
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
        MirCollectionOp::TensorFromFlat => fold_tensor_from_flat(
            statement,
            destination,
            call,
            temp_to_local,
            validation,
            facts,
        ),
        _ => Ok(statement.clone()),
    }
}

/// Fold a `TensorFromFlat` statement: keep its data movement and record the
/// folded shape fact for the destination local.
fn fold_tensor_from_flat(
    statement: &MirStatement,
    destination: &Option<MirPlace>,
    call: &MirRuntimeCall,
    temp_to_local: &BTreeMap<MirTempId, MirLocalId>,
    validation: &MirValidationContext<'_>,
    facts: &mut Vec<FoldedShapeFact>,
) -> Result<MirStatement, ShapeFoldError> {
    let Some(local) = destination_local_traced(destination.as_ref(), temp_to_local) else {
        return Err(ShapeFoldError::new(
            ShapeFoldKind::NonStaticShapeDestination,
            "tensor_from_flat destination is not a local; cannot fold its shape",
        ));
    };
    let dims = tensor_literal_dims(call.return_ty, validation).ok_or_else(|| {
        ShapeFoldError::new(
            ShapeFoldKind::NonStaticShapeDestination,
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

/// The MIR tensor type of a `magnitudines()` receiver, or a typed
/// `UnfoldableMagnitudines` diagnostic.
fn magnitudines_receiver_ty(
    call: &MirRuntimeCall,
    function: &MirFunction,
) -> Result<MirType, ShapeFoldError> {
    let Some(receiver) = call.args.first() else {
        return Err(ShapeFoldError::new(
            ShapeFoldKind::UnfoldableMagnitudines,
            "magnitudines() has no receiver operand",
        ));
    };
    let Some(ty) = crate::local_ty::operand_local_ty(function, receiver) else {
        return Err(ShapeFoldError::new(
            ShapeFoldKind::UnfoldableMagnitudines,
            "magnitudines() receiver has no resolvable MIR type",
        ));
    };
    Ok(ty)
}

/// The literal dims of a `TensorShape` receiver, or a typed
/// `UnfoldableMagnitudines` diagnostic.
fn shape_receiver_dims(
    call: &MirRuntimeCall,
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<Vec<i64>, ShapeFoldError> {
    let ty = magnitudines_receiver_ty(call, function)?;
    let Some(dims) = tensor_literal_dims(ty, validation) else {
        return Err(ShapeFoldError::new(
            ShapeFoldKind::UnfoldableMagnitudines,
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
    tensor_element_ty(magnitudines_receiver_ty(call, function)?, validation)
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
        .and_then(crate::read_analysis::operand_local)
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
    i64s_from_literal_index(*shape, validation.types.as_table())
}

/// The element type of a tensor type.
fn tensor_element_ty(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<MirType, ShapeFoldError> {
    match validation.types.get(ty.semantic_id()) {
        Type::Tensor(element, _) => Ok(MirType::semantic(*element)),
        _ => Err(ShapeFoldError::new(
            ShapeFoldKind::NonStaticShapeDestination,
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
                ShapeFoldKind::NonStaticShapeDestination,
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

/// The destination local of a place, traced through the lowered body's
/// temp→local copy: a runtime call's result lands in a temp and the next
/// statement copies it into a local (`Local(l) ← Temp(t)`). The fold treats
/// the copied local as the value's identity — shape construction is lowered
/// the same way, so a temp destination with a traced copy is equivalent to a
/// direct local destination.
fn destination_local_traced(
    destination: Option<&MirPlace>,
    temp_to_local: &BTreeMap<MirTempId, MirLocalId>,
) -> Option<MirLocalId> {
    if let Some(local) = destination.and_then(crate::read_analysis::place_local_base) {
        return Some(local);
    }
    let MirPlaceBase::Temp(temp) = destination?.base else {
        return None;
    };
    temp_to_local.get(&temp).copied()
}

#[cfg(test)]
#[path = "static_shape_fold_test.rs"]
mod tests;
