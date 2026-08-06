//! Device-safe lowerability gate: rejects host-only MIR shapes before
//! target-specific emission.

use super::roles::MirDeviceContext;
use crate::capability::{CapabilityGap, Lowerability};
use crate::control::{
    is_structured_indexed_value_join_branch, is_structured_indexed_write_branch,
    is_structured_indexed_write_goto, is_structured_indexed_write_loop,
    is_structured_indexed_write_loop_break, is_structured_indexed_write_loop_continue,
    is_structured_indexed_write_switch, is_structured_nested_indexed_write_loop_branch,
    is_structured_nested_indexed_write_loop_break,
    is_structured_nested_indexed_write_loop_continue, is_structured_nested_scalar_loop_branch,
    is_structured_return_branch, is_structured_scalar_loop, is_structured_scalar_loop_break,
    is_structured_scalar_loop_continue, is_structured_scalar_return_goto,
    is_structured_scalar_switch, is_structured_value_join_branch,
};
use crate::kernel_decomposition::ResultAggregationSlots;
use crate::visit::MirVisitor;
use crate::{
    MirAggregate, MirAggregateKind, MirCollectionOp, MirFunction, MirIntrinsic, MirLayoutKind,
    MirLayoutTable, MirLocal, MirLocalId, MirOperand, MirParam, MirPlace, MirPlaceBase,
    MirProjection, MirRuntimeCall, MirScalarLayout, MirStatement, MirStatementKind, MirTemp,
    MirTempId, MirTensorLocusLayout, MirTensorStorageLayout, MirTerminator, MirTerminatorKind,
    MirType, MirValidationContext, MirValue, MirValueKind, ValidatedMir,
};
use radix_types::{NumericWidth, Primitive, Type, TypeId, TypeTable};
use rustc_hash::FxHashSet;

const SLUG_ABI: &str = "missing-abi-fact";
const SLUG_LAYOUT: &str = "missing-layout-fact";
const SLUG_POLICY: &str = "target-policy-rejection";
const DEVICE_VECTOR_MIN_WIDTH: u8 = 2;
const DEVICE_VECTOR_MAX_WIDTH: u8 = 16;
/// WGSL register matrices are `matCxR` with C,R in 2..=4; keep the shared device gate aligned.
const DEVICE_MATRIX_MIN_DIM: u8 = 2;
const DEVICE_MATRIX_MAX_DIM: u8 = 4;

const SHAPE_UNRESOLVED_TYPE: &str = "unresolved type";
const SHAPE_DEVICE_VECTOR_SURROGATE: &str = "device vector surrogate requires explicit vector fact";
const SHAPE_DEVICE_NIHIL_VALUE: &str = "device nihil value";
const SHAPE_KERNEL_AGGREGATE_LAYOUT: &str = "kernel aggregate layout";
const SHAPE_KERNEL_OPAQUE_LAYOUT: &str = "kernel opaque layout";
const SHAPE_KERNEL_HOST_TENSOR_STORAGE: &str = "kernel tensor storage/locus requires device handle";
const SHAPE_DEVICE_PLACE_PROJECTION: &str = "device place projection";
const SHAPE_ASYNC_GENERATOR_DEVICE_FN: &str = "async/generator device function";
const SHAPE_DEVICE_MULTI_BLOCK_CFG: &str = "device multi-block control flow";
const SHAPE_FAILABLE_DEVICE_FN: &str = "failable device function";
const SHAPE_DEVICE_FUNCTION_CALL: &str = "device function call";
const SHAPE_KERNEL_RUNTIME_CALL: &str = "kernel runtime call";
const SHAPE_KERNEL_AGGREGATE_CONSTRUCT: &str = "kernel aggregate construct";
const SHAPE_KERNEL_TENSOR_VIEW_OP: &str =
    "kernel tensor view operation requires explicit materialize";
const SHAPE_TRY_CALL: &str = "try_call";
const SHAPE_RETURN_ERROR_DEVICE_TERMINATOR: &str = "return_error device terminator";

#[must_use]
pub fn classify_device_safe(
    validated: &ValidatedMir<'_>,
    device: &MirDeviceContext,
) -> Lowerability {
    let mut classifier = DeviceSafeClassifier::new(validated.validation(), device);
    classifier.visit_program(validated.program());
    classifier.finish()
}

#[must_use]
pub fn classify_device_safe_for_target_entry(
    validated: &ValidatedMir<'_>,
    device: &MirDeviceContext,
) -> Lowerability {
    let mut classifier = DeviceSafeClassifier::new(validated.validation(), device);
    classifier.allow_host_owned_tensor_entry = true;
    classifier.visit_program(validated.program());
    classifier.finish()
}

/// The dead fold-artifact slots of one function (S5-W-A1): the local and
/// temp destinations of `Array`/`Tensor` constructs that no statement or
/// terminator reads — the static-shape fold's shape-list / vacua artifacts
/// the emitters skip (S5-U2c).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct FoldArtifactSlots {
    locals: FxHashSet<MirLocalId>,
    temps: FxHashSet<MirTempId>,
}

impl FoldArtifactSlots {
    /// Whether a place base is one of the dead fold-artifact slots.
    fn contains(&self, base: &MirPlaceBase) -> bool {
        match base {
            MirPlaceBase::Local(local) => self.locals.contains(local),
            MirPlaceBase::Temp(temp) => self.temps.contains(temp),
        }
    }
}

/// The dead fold-artifact slots of a function (S5-W-A1) — the S5-U2c
/// liveness scan the gate shares with the emitters.
///
/// A slot is collected when it is a fold-artifact slot shape
/// (`Array`/`Tensor` — the static-shape fold's shape-list / vacua types)
/// and DEAD: no statement or terminator reads it, and a local slot is not a
/// function param (a param is a side effect — always live). A temp slot
/// traces through the lowered body's forward copy (`Local(l) ← Temp(t)`):
/// the temp is dead exactly when the local it forwards into (or a direct
/// operand consumer) is never read — mirrors the Metal/CUDA
/// `destination_is_read` (S5-U2c). The forward-copy target local is
/// collected too (the fold keeps the untraceable `temp→local` forward, so
/// the shape-list local reaches the gate as a declaration even though no
/// statement reads it). The emitters skip exactly these slots; a live
/// `Array`/`Tensor` construct or slot stays fail-closed with the aggregate
/// diagnostics.
fn dead_fold_artifact_slots(function: &MirFunction, types: &TypeTable) -> FoldArtifactSlots {
    let read_locals = crate::read_analysis::function_read_locals(function);
    let mut dead = FoldArtifactSlots::default();
    for local in &function.locals {
        if is_fold_artifact_type(types, local.ty)
            && !function.params.iter().any(|param| param.local == local.id)
            && !read_locals.contains(&local.id)
        {
            dead.locals.insert(local.id);
        }
    }
    for temp in &function.temps {
        if !is_fold_artifact_type(types, temp.ty)
            || crate::read_analysis::temp_directly_read(function, temp.id)
        {
            continue;
        }
        match crate::read_analysis::temp_forward_local(function, temp.id) {
            None => {
                dead.temps.insert(temp.id);
            }
            Some(local)
                if !function.params.iter().any(|param| param.local == local)
                    && !read_locals.contains(&local) =>
            {
                dead.temps.insert(temp.id);
            }
            Some(_) => {}
        }
    }
    dead
}

/// Whether a slot type is a fold-artifact slot shape (`Array`/`Tensor` —
/// the static-shape fold's shape-list and vacua-seed types). Only these
/// types may be dead-skipped at the gate, mirroring the emitters' S5-U2c
/// skip surface; they are structurally non-scalar, satisfying the narrowed
/// "non-scalar only" dead-skip rule (radix-mir-metal 78e4440e7).
fn is_fold_artifact_type(types: &TypeTable, ty: MirType) -> bool {
    matches!(
        types.get(ty.semantic_id()),
        Type::Array(_) | Type::Tensor(_, _)
    )
}

/// The dead non-kernel-scalar const-copy slots of one function (need
/// a0184031): plain destinations of a DEAD const-copy Assign whose value is
/// a scalar the kernel surface cannot express — the `LayerNorm` lowering's
/// `i64` axis const (`Assign(Local(3) <- Constant(Int(axis)))` typed `i64`;
/// the recipe call carries the axis directly, so the const local is never
/// read).
///
/// The Metal emitter skips exactly this statement (26424928f Gap 1 —
/// "format/legality-only": the destination is never read, so no semantics
/// change) and the NVVM emitter emits it as a harmless native-i64 slot
/// store; the production CUDA artifact path runs this shared gate BEFORE the
/// emitter, so the gate must agree. The CTO council constraints
/// (5f1b1348) hold structurally:
///
/// - a slot is dead only when NO statement or terminator reads it (the read
///   set covers terminators — a value read only by a block's terminator is
///   still live);
/// - a slot is never a function param (a param is a side effect — always
///   live, and resource-visible), never a result-aggregation slot;
/// - the statement-level skip additionally requires an UNPROJECTED plain
///   slot (an output-projection / projected store is never skipped), a
///   pure `Constant` RHS (no runtime call, no expression — the emitters'
///   dead-skip admits only `MirOperand::Constant`), and a non-kernel-scalar
///   value — so a LIVE or merely-difficult (non-constant) i64 stays
///   fail-closed with the layout diagnostics.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct DeadConstCopySlots {
    locals: FxHashSet<MirLocalId>,
    temps: FxHashSet<MirTempId>,
}

impl DeadConstCopySlots {
    /// Whether a place base is one of the dead non-kernel-scalar const-copy
    /// slots.
    fn contains(&self, base: &MirPlaceBase) -> bool {
        match base {
            MirPlaceBase::Local(local) => self.locals.contains(local),
            MirPlaceBase::Temp(temp) => self.temps.contains(temp),
        }
    }
}

/// The dead non-kernel-scalar const-copy slots of one function (need
/// a0184031) — the liveness scan the gate shares with the emitters' skip.
///
/// A slot is collected when its declared type is a non-kernel scalar (a
/// `Scalar` layout the kernel surface cannot express — i64 etc.) and it is
/// DEAD: no statement or terminator reads it, and a local slot is not a
/// function param (a param is a side effect — always live). A temp slot
/// traces through the lowered body's forward copy (`Local(l) ← Temp(t)`)
/// exactly like the fold-artifact slots: the temp is dead when the local it
/// forwards into (or a direct operand consumer) is never read.
fn dead_non_kernel_scalar_const_slots(
    function: &MirFunction,
    layouts: &MirLayoutTable,
) -> DeadConstCopySlots {
    let read_locals = crate::read_analysis::function_read_locals(function);
    let mut dead = DeadConstCopySlots::default();
    for local in &function.locals {
        if is_non_kernel_scalar_layout(layouts, local.ty)
            && !function.params.iter().any(|param| param.local == local.id)
            && !read_locals.contains(&local.id)
        {
            dead.locals.insert(local.id);
        }
    }
    for temp in &function.temps {
        if !is_non_kernel_scalar_layout(layouts, temp.ty)
            || crate::read_analysis::temp_directly_read(function, temp.id)
        {
            continue;
        }
        match crate::read_analysis::temp_forward_local(function, temp.id) {
            None => {
                dead.temps.insert(temp.id);
            }
            Some(local)
                if !function.params.iter().any(|param| param.local == local)
                    && !read_locals.contains(&local) =>
            {
                dead.temps.insert(temp.id);
            }
            Some(_) => {}
        }
    }
    dead
}

/// Whether a type's layout is a scalar the kernel surface cannot express — a
/// `Scalar` layout that is NOT one of the kernel scalars (f32, bool, i32,
/// u32). A DEAD const-copy of such a value (the `LayerNorm` lowering's `i64`
/// axis const) is skipped as a format/legality-only drop (need,
/// mirroring the Metal emitter's `value_is_non_kernel_scalar`, 26424928f
/// Gap 1). A LIVE or merely-difficult (non-constant) value keeps the layout
/// diagnostics.
fn is_non_kernel_scalar_layout(layouts: &MirLayoutTable, ty: MirType) -> bool {
    matches!(
        layouts.layout_for_type(ty),
        Ok(layout)
            if matches!(
                layout.kind,
                MirLayoutKind::Scalar(scalar)
                    if !matches!(
                        scalar,
                        MirScalarLayout::F32
                            | MirScalarLayout::Bool
                            | MirScalarLayout::I32
                            | MirScalarLayout::U32
                    )
            )
    )
}

struct DeviceSafeClassifier<'a> {
    types: &'a TypeTable,
    layouts: &'a MirLayoutTable,
    device: &'a MirDeviceContext,
    in_device_function: bool,
    allows_structured_return_branch: bool,
    allows_structured_indexed_write_branch: bool,
    allows_structured_indexed_write_goto: bool,
    allows_structured_indexed_write_loop: bool,
    allows_structured_indexed_write_loop_break: bool,
    allows_structured_indexed_write_loop_continue: bool,
    allows_structured_indexed_write_switch: bool,
    allows_structured_indexed_value_join_branch: bool,
    allows_structured_value_join_branch: bool,
    allows_structured_scalar_switch: bool,
    allows_structured_scalar_loop: bool,
    allows_structured_scalar_loop_break: bool,
    allows_structured_scalar_loop_continue: bool,
    allows_structured_scalar_return_goto: bool,
    allows_structured_nested_scalar_loop_branch: bool,
    allows_structured_nested_indexed_write_loop_branch: bool,
    allows_structured_nested_indexed_write_loop_break: bool,
    allows_structured_nested_indexed_write_loop_continue: bool,
    allow_host_owned_tensor_entry: bool,
    /// S5A-U4: the output-set tuple aggregation slots of the function under
    /// visit — the ONLY tuple-typed local/temp/value the gate admits. The
    /// fact comes from the decomposition (`kernel_decomposition.rs`): the
    /// function's own result-tuple return aggregation, plus the kept source
    /// result-tuple local a decomposed multi-output subchain synthetic
    /// declares. An unrelated tuple aggregate — live or not — stays
    /// fail-closed with the aggregate diagnostics.
    result_aggregation_slots: ResultAggregationSlots,
    /// S5-W-A1: the dead fold-artifact slots of the function under visit —
    /// the destinations of `Array`/`Tensor` constructs (the static-shape
    /// fold's shape-list / vacua artifacts) that no statement or terminator
    /// reads. Both emitters skip exactly these constructs (S5-U2c), so the
    /// gate admits them: the statement, its aggregate type, and the
    /// artifact's own local/temp declarations are not device values. A live
    /// `Array`/`Tensor` construct stays fail-closed with the aggregate
    /// diagnostics.
    dead_fold_artifact_slots: FoldArtifactSlots,
    /// a0184031: the dead non-kernel-scalar const-copy slots of the function
    /// under visit — plain destinations of a DEAD const-copy Assign whose
    /// value is a scalar the kernel surface cannot express (the `LayerNorm`
    /// lowering's `i64` axis const). The Metal emitter skips exactly these
    /// statements (26424928f Gap 1) and the NVVM emitter emits them as
    /// harmless native-i64 slot stores, so the gate admits the local/temp
    /// declarations and the const-copy statement. A LIVE or merely-difficult
    /// (non-constant) non-kernel scalar stays fail-closed with the layout
    /// diagnostics.
    dead_const_copy_slots: DeadConstCopySlots,
    gaps: Vec<CapabilityGap>,
}

impl<'a> DeviceSafeClassifier<'a> {
    fn new(validation: &'a MirValidationContext<'a>, device: &'a MirDeviceContext) -> Self {
        Self {
            types: validation.types,
            layouts: &validation.layouts,
            device,
            in_device_function: false,
            allows_structured_return_branch: false,
            allows_structured_indexed_write_branch: false,
            allows_structured_indexed_write_goto: false,
            allows_structured_indexed_write_loop: false,
            allows_structured_indexed_write_loop_break: false,
            allows_structured_indexed_write_loop_continue: false,
            allows_structured_indexed_write_switch: false,
            allows_structured_indexed_value_join_branch: false,
            allows_structured_value_join_branch: false,
            allows_structured_scalar_switch: false,
            allows_structured_scalar_loop: false,
            allows_structured_scalar_loop_break: false,
            allows_structured_scalar_loop_continue: false,
            allows_structured_scalar_return_goto: false,
            allows_structured_nested_scalar_loop_branch: false,
            allows_structured_nested_indexed_write_loop_branch: false,
            allows_structured_nested_indexed_write_loop_break: false,
            allows_structured_nested_indexed_write_loop_continue: false,
            allow_host_owned_tensor_entry: false,
            result_aggregation_slots: ResultAggregationSlots::default(),
            dead_fold_artifact_slots: FoldArtifactSlots::default(),
            dead_const_copy_slots: DeadConstCopySlots::default(),
            gaps: Vec::new(),
        }
    }

    fn finish(self) -> Lowerability {
        if self.gaps.is_empty() {
            Lowerability::Capable
        } else {
            Lowerability::Rejected(self.gaps)
        }
    }

    fn record_layout(&mut self, shape: impl Into<String>) {
        self.gaps.push(CapabilityGap::MissingLayoutFact {
            shape: shape.into(),
            slug: SLUG_LAYOUT,
        });
    }

    fn record_abi(&mut self, shape: impl Into<String>) {
        self.gaps.push(CapabilityGap::MissingAbiFact {
            shape: shape.into(),
            slug: SLUG_ABI,
        });
    }

    fn record_policy(&mut self, shape: impl Into<String>) {
        self.gaps.push(CapabilityGap::TargetPolicyRejection {
            shape: shape.into(),
            slug: SLUG_POLICY,
        });
    }

    fn check_device_type(&mut self, ty: MirType) {
        let semantic = self.types.get(ty.semantic_id()).clone();
        match semantic {
            Type::Primitive(Primitive::Bivalens | Primitive::Vacuum | Primitive::Numquam)
            | Type::SizedNumeric(Primitive::Fractus, _) => self.check_device_layout(ty),
            Type::Alias(_, _) => self.check_device_layout(ty),
            Type::Param(_) | Type::Infer(_) | Type::Error => {
                self.record_layout(SHAPE_UNRESOLVED_TYPE);
            }
            Type::Primitive(
                Primitive::Textus
                | Primitive::Ascii
                | Primitive::Octeti
                | Primitive::Regex
                | Primitive::Json
                | Primitive::Valor
                | Primitive::Ignotum
                | Primitive::Instans,
            )
            | Type::SizedInstans(_) => {
                self.record_policy(format!("device host value type {semantic:?}"));
            }
            Type::Primitive(Primitive::Numerus | Primitive::Fractus)
            | Type::SizedNumeric(Primitive::Numerus, _) => {
                self.check_device_layout(ty);
            }
            Type::ModularWord(_) => self.check_device_layout(ty),
            Type::SizedNumeric(_, _) => {
                self.record_policy(format!("device host value type {semantic:?}"));
            }
            Type::Array(inner) if self.is_vector_surrogate_element(inner) => {
                self.record_policy(SHAPE_DEVICE_VECTOR_SURROGATE);
            }
            Type::Array(_)
            | Type::Tuple(_)
            | Type::Map(_, _)
            | Type::Record(_)
            | Type::Set(_)
            | Type::Promissum(_)
            | Type::PromissumFailable(_, _)
            | Type::AsyncCursor(_, _)
            | Type::Cursor(_)
            | Type::Sparsa(_, _)
            | Type::Intervallum(_)
            | Type::Option(_)
            | Type::Ref(_, _)
            | Type::Struct(_)
            | Type::Enum(_)
            | Type::Interface(_)
            | Type::Func(_)
            | Type::Applied(_, _)
            | Type::Union(_) => {
                self.record_policy(format!("device aggregate value type {semantic:?}"));
            }
            Type::Atomic(_) => self.check_device_layout(ty),
            Type::Tensor(_, _) => self.check_device_layout(ty),
            Type::Vector(_, _) => self.check_device_layout(ty),
            Type::Matrix(_, _) => self.check_device_layout(ty),
            Type::Primitive(Primitive::Nihil) => self.record_policy(SHAPE_DEVICE_NIHIL_VALUE),
        }
    }

    fn check_device_layout(&mut self, ty: MirType) {
        match self.layouts.layout_for_type(ty) {
            Ok(layout) => match layout.kind {
                MirLayoutKind::Scalar(
                    MirScalarLayout::F32
                    | MirScalarLayout::I32
                    | MirScalarLayout::U32
                    | MirScalarLayout::Bool,
                )
                | MirLayoutKind::Void
                | MirLayoutKind::Never => {}
                MirLayoutKind::Scalar(scalar) => {
                    self.record_layout(format!("kernel scalar layout {}", scalar.spelling()));
                }
                MirLayoutKind::Aggregate(_) => {
                    self.record_layout(SHAPE_KERNEL_AGGREGATE_LAYOUT);
                }
                MirLayoutKind::DeviceView(view) => {
                    if self.allow_host_owned_tensor_entry
                        && view.storage == MirTensorStorageLayout::HostOwned
                        && view.locus == MirTensorLocusLayout::Host
                    {
                    } else if view.storage != MirTensorStorageLayout::DeviceHandle
                        || view.locus != MirTensorLocusLayout::Device
                    {
                        self.record_policy(SHAPE_KERNEL_HOST_TENSOR_STORAGE);
                    } else if !view.row_major || !view.contiguous || view.offset_elements != 0 {
                        self.record_layout("kernel dense tensor view layout");
                    } else if view.rank != 1 {
                        self.record_layout(format!("kernel device view rank {}", view.rank));
                    } else if !matches!(
                        view.element,
                        MirScalarLayout::F32 | MirScalarLayout::I32 | MirScalarLayout::U32
                    ) {
                        self.record_layout(format!(
                            "kernel device view element {}",
                            view.element.spelling()
                        ));
                    }
                }
                MirLayoutKind::DeviceVector(vector) => {
                    if vector.width < DEVICE_VECTOR_MIN_WIDTH
                        || vector.width > DEVICE_VECTOR_MAX_WIDTH
                    {
                        self.record_layout(format!("kernel vector width {}", vector.width));
                    } else if !is_device_vector_element(vector.element) {
                        self.record_layout(format!(
                            "kernel vector element {}",
                            vector.element.spelling()
                        ));
                    }
                }
                MirLayoutKind::DeviceMatrix(matrix) => {
                    if matrix.rows < DEVICE_MATRIX_MIN_DIM
                        || matrix.rows > DEVICE_MATRIX_MAX_DIM
                        || matrix.columns < DEVICE_MATRIX_MIN_DIM
                        || matrix.columns > DEVICE_MATRIX_MAX_DIM
                    {
                        self.record_layout(format!(
                            "kernel matrix {}x{}",
                            matrix.rows, matrix.columns
                        ));
                    } else if !is_device_matrix_element(matrix.element) {
                        self.record_layout(format!(
                            "kernel matrix element {}",
                            matrix.element.spelling()
                        ));
                    }
                }
                MirLayoutKind::Atomic(atomic) => {
                    self.record_policy(format!(
                        "kernel atomic value type {}",
                        atomic.element.spelling()
                    ));
                }
                MirLayoutKind::PackedNumeric(packed) => {
                    self.record_policy(packed.unsupported_execution_issue());
                }
                MirLayoutKind::OpaqueHandle => {
                    self.record_layout(SHAPE_KERNEL_OPAQUE_LAYOUT);
                }
            },
            Err(error) => self.record_layout(error.message),
        }
    }

    fn is_vector_surrogate_element(&self, inner: TypeId) -> bool {
        matches!(
            self.types.get(inner),
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
        )
    }

    fn check_place(&mut self, place: &MirPlace) {
        if !place.projections.iter().all(|projection| {
            matches!(
                projection,
                MirProjection::Index(_)
                    | MirProjection::VectorLane(_)
                    | MirProjection::MatrixCell { .. }
            )
        }) {
            self.record_policy(SHAPE_DEVICE_PLACE_PROJECTION);
        }
    }

    /// Whether a place base is one of the function's output-set tuple
    /// aggregation slots (S5A-U4): the ONLY tuple-typed local/temp/value the
    /// gate admits. The fact is the decomposition's explicit result-
    /// aggregation record (`kernel_decomposition.rs`), not a source-wide
    /// `DefId` allowance — an unrelated tuple aggregate inside a
    /// tuple-returning source stays fail-closed.
    fn is_result_aggregation_slot(&self, base: &MirPlaceBase) -> bool {
        self.result_aggregation_slots.contains(base)
    }

    /// Whether the statement is a DEAD non-kernel-scalar const-copy Assign
    /// (need — mirroring the Metal emitter-side skip, 26424928f Gap
    /// 1): the CTO council constraints (5f1b1348) all hold structurally —
    ///
    /// 1. the destination is unread by statements AND terminators (the
    ///    precomputed dead slot set uses `function_read_locals`, which
    ///    counts terminator reads);
    /// 2. the destination is an UNPROJECTED plain slot (a projected store —
    ///    including the kernel `output[i] = <result>` write — is never
    ///    skipped), is not a function parameter (params are side effects and
    ///    resource-visible), and is not a result-aggregation slot;
    /// 3. the RHS is a pure `MirOperand::Constant` copy — no runtime call,
    ///    no expression, no side effect (the emitters' dead-skip admits only
    ///    constants);
    /// 4. the value is a scalar the kernel surface cannot express, so the
    ///    statement is exactly the one every affected emitter discards
    ///    before target-specific type emission (Metal skips it; NVVM emits a
    ///    harmless native-i64 slot store);
    /// 5. a LIVE or merely-difficult (non-constant) non-kernel scalar is not
    ///    a dead const-copy slot and keeps the layout diagnostics.
    fn is_dead_non_kernel_scalar_const_copy(&self, place: &MirPlace, value: &MirValue) -> bool {
        place.projections.is_empty()
            && !self.is_result_aggregation_slot(&place.base)
            && self.dead_const_copy_slots.contains(&place.base)
            && matches!(&value.kind, MirValueKind::Operand(MirOperand::Constant(_)))
            && is_non_kernel_scalar_layout(self.layouts, value.ty)
    }
}

impl MirVisitor for DeviceSafeClassifier<'_> {
    fn visit_function(&mut self, function: &MirFunction) {
        let was_in_device_function = self.in_device_function;
        let previous_allows_structured_return_branch = self.allows_structured_return_branch;
        let previous_allows_structured_indexed_write_branch =
            self.allows_structured_indexed_write_branch;
        let previous_allows_structured_indexed_write_goto =
            self.allows_structured_indexed_write_goto;
        let previous_allows_structured_indexed_write_loop =
            self.allows_structured_indexed_write_loop;
        let previous_allows_structured_indexed_write_loop_break =
            self.allows_structured_indexed_write_loop_break;
        let previous_allows_structured_indexed_write_loop_continue =
            self.allows_structured_indexed_write_loop_continue;
        let previous_allows_structured_indexed_write_switch =
            self.allows_structured_indexed_write_switch;
        let previous_allows_structured_indexed_value_join_branch =
            self.allows_structured_indexed_value_join_branch;
        let previous_allows_structured_value_join_branch = self.allows_structured_value_join_branch;
        let previous_allows_structured_scalar_switch = self.allows_structured_scalar_switch;
        let previous_allows_structured_scalar_loop = self.allows_structured_scalar_loop;
        let previous_allows_structured_scalar_loop_break = self.allows_structured_scalar_loop_break;
        let previous_allows_structured_scalar_loop_continue =
            self.allows_structured_scalar_loop_continue;
        let previous_allows_structured_scalar_return_goto =
            self.allows_structured_scalar_return_goto;
        let previous_allows_structured_nested_scalar_loop_branch =
            self.allows_structured_nested_scalar_loop_branch;
        let previous_allows_structured_nested_indexed_write_loop_branch =
            self.allows_structured_nested_indexed_write_loop_branch;
        let previous_allows_structured_nested_indexed_write_loop_break =
            self.allows_structured_nested_indexed_write_loop_break;
        let previous_allows_structured_nested_indexed_write_loop_continue =
            self.allows_structured_nested_indexed_write_loop_continue;
        self.in_device_function = self.device.role_for_function(function).is_some();
        self.allows_structured_return_branch = false;
        self.allows_structured_indexed_write_branch = false;
        self.allows_structured_indexed_write_goto = false;
        self.allows_structured_indexed_write_loop = false;
        self.allows_structured_indexed_write_loop_break = false;
        self.allows_structured_indexed_write_loop_continue = false;
        self.allows_structured_indexed_write_switch = false;
        self.allows_structured_indexed_value_join_branch = false;
        self.allows_structured_value_join_branch = false;
        self.allows_structured_scalar_switch = false;
        self.allows_structured_scalar_loop = false;
        self.allows_structured_scalar_loop_break = false;
        self.allows_structured_scalar_loop_continue = false;
        self.allows_structured_scalar_return_goto = false;
        self.allows_structured_nested_scalar_loop_branch = false;
        self.allows_structured_nested_indexed_write_loop_branch = false;
        self.allows_structured_nested_indexed_write_loop_break = false;
        self.allows_structured_nested_indexed_write_loop_continue = false;

        if self.in_device_function {
            let allows_structured_return_branch = is_structured_return_branch(function);
            let allows_structured_indexed_write_branch =
                is_structured_indexed_write_branch(function);
            let allows_structured_indexed_write_goto = is_structured_indexed_write_goto(function);
            let allows_structured_indexed_write_loop = is_structured_indexed_write_loop(function);
            let allows_structured_indexed_write_loop_break =
                is_structured_indexed_write_loop_break(function);
            let allows_structured_indexed_write_loop_continue =
                is_structured_indexed_write_loop_continue(function);
            let allows_structured_indexed_write_switch =
                is_structured_indexed_write_switch(function);
            let allows_structured_indexed_value_join_branch =
                is_structured_indexed_value_join_branch(function);
            let allows_structured_value_join_branch = is_structured_value_join_branch(function);
            let allows_structured_scalar_switch = is_structured_scalar_switch(function);
            let allows_structured_scalar_loop = is_structured_scalar_loop(function);
            let allows_structured_scalar_loop_break = is_structured_scalar_loop_break(function);
            let allows_structured_scalar_loop_continue =
                is_structured_scalar_loop_continue(function);
            let allows_structured_scalar_return_goto = is_structured_scalar_return_goto(function);
            let allows_structured_nested_scalar_loop_branch =
                is_structured_nested_scalar_loop_branch(function);
            let allows_structured_nested_indexed_write_loop_branch =
                is_structured_nested_indexed_write_loop_branch(function);
            let allows_structured_nested_indexed_write_loop_break =
                is_structured_nested_indexed_write_loop_break(function);
            let allows_structured_nested_indexed_write_loop_continue =
                is_structured_nested_indexed_write_loop_continue(function);
            self.allows_structured_return_branch = allows_structured_return_branch;
            self.allows_structured_indexed_write_branch = allows_structured_indexed_write_branch;
            self.allows_structured_indexed_write_goto = allows_structured_indexed_write_goto;
            self.allows_structured_indexed_write_loop = allows_structured_indexed_write_loop;
            self.allows_structured_indexed_write_loop_break =
                allows_structured_indexed_write_loop_break;
            self.allows_structured_indexed_write_loop_continue =
                allows_structured_indexed_write_loop_continue;
            self.allows_structured_indexed_write_switch = allows_structured_indexed_write_switch;
            self.allows_structured_indexed_value_join_branch =
                allows_structured_indexed_value_join_branch;
            self.allows_structured_value_join_branch = allows_structured_value_join_branch;
            self.allows_structured_scalar_switch = allows_structured_scalar_switch;
            self.allows_structured_scalar_loop = allows_structured_scalar_loop;
            self.allows_structured_scalar_loop_break = allows_structured_scalar_loop_break;
            self.allows_structured_scalar_loop_continue = allows_structured_scalar_loop_continue;
            self.allows_structured_scalar_return_goto = allows_structured_scalar_return_goto;
            self.allows_structured_nested_scalar_loop_branch =
                allows_structured_nested_scalar_loop_branch;
            self.allows_structured_nested_indexed_write_loop_branch =
                allows_structured_nested_indexed_write_loop_branch;
            self.allows_structured_nested_indexed_write_loop_break =
                allows_structured_nested_indexed_write_loop_break;
            self.allows_structured_nested_indexed_write_loop_continue =
                allows_structured_nested_indexed_write_loop_continue;
            if function.is_async || function.is_generator {
                self.record_policy(SHAPE_ASYNC_GENERATOR_DEVICE_FN);
            }
            if function.blocks.len() != 1
                && !allows_structured_return_branch
                && !allows_structured_indexed_write_branch
                && !allows_structured_indexed_write_goto
                && !allows_structured_indexed_write_loop
                && !allows_structured_indexed_write_loop_break
                && !allows_structured_indexed_write_loop_continue
                && !allows_structured_indexed_write_switch
                && !allows_structured_indexed_value_join_branch
                && !allows_structured_value_join_branch
                && !allows_structured_scalar_switch
                && !allows_structured_scalar_loop
                && !allows_structured_scalar_loop_break
                && !allows_structured_scalar_loop_continue
                && !allows_structured_scalar_return_goto
                && !allows_structured_nested_scalar_loop_branch
                && !allows_structured_nested_indexed_write_loop_branch
                && !allows_structured_nested_indexed_write_loop_break
                && !allows_structured_nested_indexed_write_loop_continue
            {
                self.record_policy(SHAPE_DEVICE_MULTI_BLOCK_CFG);
            }
            if function.error_ty.is_some() {
                self.record_abi(SHAPE_FAILABLE_DEVICE_FN);
            }
            // S3-A1: a result-tuple return is the multi-output kernel contract —
            // the tuple elements are DISTINCT device output resources (the ABI
            // lowers them via `multi_output_return_kernel`). Classify each
            // element instead of the aggregate tuple, which would otherwise
            // record a "device aggregate value type" gap on a valid contract.
            match self.types.get(function.return_ty.semantic_id()) {
                Type::Tuple(elements) if !elements.is_empty() => {
                    for element in elements {
                        self.check_device_type(MirType::semantic(*element));
                    }
                }
                _ => self.check_device_type(function.return_ty),
            }
            if let Some(error_ty) = function.error_ty {
                self.check_device_type(error_ty);
            }
            // S5-W-A1: the dead fold-artifact slots (shape-list / vacua
            // `Array`/`Tensor` slots nothing reads) are precomputed so the
            // local, temp, and construct checks admit them exactly as the
            // emitters' S5-U2c skip does.
            self.dead_fold_artifact_slots = dead_fold_artifact_slots(function, self.types);
            // a0184031: the dead non-kernel-scalar const-copy slots (i64
            // axis-const locals/temps nothing reads) are precomputed so the
            // local/temp declarations and the const-copy statement checks
            // admit them exactly as the emitters' dead-skip does (Metal
            // 26424928f Gap 1; NVVM emits the harmless native-i64 store).
            self.dead_const_copy_slots = dead_non_kernel_scalar_const_slots(function, self.layouts);
            // S5A-U4: the output-set tuple aggregation slots of this function
            // (its own result-tuple return aggregation plus a decomposed
            // multi-output source's kept result-tuple local) — the ONLY
            // tuple-typed slots the gate admits. Computed per function from
            // the decomposition's explicit fact; no source-wide `DefId`
            // allowance.
            self.result_aggregation_slots =
                crate::kernel_decomposition::multi_output_result_aggregation_slots(
                    function, self.types,
                );
            crate::visit::walk_function(self, function);
        }

        self.dead_fold_artifact_slots = FoldArtifactSlots::default();
        self.dead_const_copy_slots = DeadConstCopySlots::default();
        self.result_aggregation_slots = ResultAggregationSlots::default();
        self.in_device_function = was_in_device_function;
        self.allows_structured_return_branch = previous_allows_structured_return_branch;
        self.allows_structured_indexed_write_branch =
            previous_allows_structured_indexed_write_branch;
        self.allows_structured_indexed_write_goto = previous_allows_structured_indexed_write_goto;
        self.allows_structured_indexed_write_loop = previous_allows_structured_indexed_write_loop;
        self.allows_structured_indexed_write_loop_break =
            previous_allows_structured_indexed_write_loop_break;
        self.allows_structured_indexed_write_loop_continue =
            previous_allows_structured_indexed_write_loop_continue;
        self.allows_structured_indexed_write_switch =
            previous_allows_structured_indexed_write_switch;
        self.allows_structured_indexed_value_join_branch =
            previous_allows_structured_indexed_value_join_branch;
        self.allows_structured_value_join_branch = previous_allows_structured_value_join_branch;
        self.allows_structured_scalar_switch = previous_allows_structured_scalar_switch;
        self.allows_structured_scalar_loop = previous_allows_structured_scalar_loop;
        self.allows_structured_scalar_loop_break = previous_allows_structured_scalar_loop_break;
        self.allows_structured_scalar_loop_continue =
            previous_allows_structured_scalar_loop_continue;
        self.allows_structured_scalar_return_goto = previous_allows_structured_scalar_return_goto;
        self.allows_structured_nested_scalar_loop_branch =
            previous_allows_structured_nested_scalar_loop_branch;
        self.allows_structured_nested_indexed_write_loop_branch =
            previous_allows_structured_nested_indexed_write_loop_branch;
        self.allows_structured_nested_indexed_write_loop_break =
            previous_allows_structured_nested_indexed_write_loop_break;
        self.allows_structured_nested_indexed_write_loop_continue =
            previous_allows_structured_nested_indexed_write_loop_continue;
    }

    fn visit_param(&mut self, param: &MirParam) {
        if self.in_device_function {
            self.check_device_type(param.ty);
        }
    }

    fn visit_local(&mut self, local: &MirLocal) {
        if self.in_device_function
            && !self
                .result_aggregation_slots
                .contains(&MirPlaceBase::Local(local.id))
            && !self.dead_fold_artifact_slots.locals.contains(&local.id)
            && !self.dead_const_copy_slots.locals.contains(&local.id)
        {
            self.check_device_type(local.ty);
        }
    }

    fn visit_temp(&mut self, temp: &MirTemp) {
        if self.in_device_function
            && !self
                .result_aggregation_slots
                .contains(&MirPlaceBase::Temp(temp.id))
            && !self.dead_fold_artifact_slots.temps.contains(&temp.id)
            && !self.dead_const_copy_slots.temps.contains(&temp.id)
        {
            self.check_device_type(temp.ty);
        }
    }

    fn visit_stmt(&mut self, stmt: &MirStatement) {
        if !self.in_device_function {
            return;
        }
        match &stmt.kind {
            MirStatementKind::Assign { place, value } => {
                // S5-W-A1: a dead fold-artifact store — a shape-list/vacua
                // value written into a dead fold-artifact slot (the fold's
                // temp→local forward, `Local(l) ← Temp(t)`, whose local
                // nothing reads). The emitters skip the dead store (S5-U2c),
                // so the gate walks only the destination place, not the
                // artifact value (whose Array/Tensor type would otherwise
                // record the aggregate diagnostic).
                if is_fold_artifact_type(self.types, value.ty)
                    && self.dead_fold_artifact_slots.contains(&place.base)
                {
                    self.visit_place(place);
                } else if self.is_dead_non_kernel_scalar_const_copy(place, value) {
                    // a0184031: a DEAD const-copy whose value is a scalar the
                    // kernel surface cannot express (the LayerNorm lowering's
                    // i64 axis const — the recipe call carries the axis
                    // directly, so the const local is never read) fails the
                    // layout gate before any use. The Metal emitter skips
                    // exactly this statement (26424928f Gap 1) and the NVVM
                    // emitter emits it as a harmless native-i64 slot store,
                    // so the gate walks only the destination place, not the
                    // non-kernel-scalar value. The skip is format/legality-
                    // only: the CTO council constraints (5f1b1348) are all
                    // proven structurally (see `is_dead_non_kernel_scalar_
                    // const_copy`) — a LIVE or merely-difficult i64 stays
                    // fail-closed with the layout diagnostics.
                    self.visit_place(place);
                } else {
                    crate::visit::walk_stmt(self, stmt);
                }
            }
            MirStatementKind::Call { .. } => self.record_policy(SHAPE_DEVICE_FUNCTION_CALL),
            MirStatementKind::RuntimeCall { call, .. }
                if matches!(call.intrinsic, MirIntrinsic::GpuBuiltin(_)) =>
            {
                crate::visit::walk_stmt(self, stmt);
            }
            MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::Atomic(op),
                        ..
                    },
                ..
            } => self.record_policy(format!("kernel atomic runtime call {}", op.spelling())),
            MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorSlice),
                        ..
                    },
                ..
            } => self.record_policy(SHAPE_KERNEL_TENSOR_VIEW_OP),
            MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic:
                            MirIntrinsic::Collection(MirCollectionOp::Sum | MirCollectionOp::TensorMean),
                        ..
                    },
                ..
            } => {
                crate::visit::walk_stmt(self, stmt);
            }
            MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorMatMul),
                        ..
                    },
                ..
            } => {
                crate::visit::walk_stmt(self, stmt);
            }
            // S5-U2: rank-2 `TensorTranspose` is a recipe op — the plan pass
            // admits it (`CollectionKernelPlan::Transpose`, kernel_plan.rs),
            // the ABI resolves its dispatch contract (workgroup 64, dispatch
            // M·N), and the Metal emitter lowers the column-major remap
            // body. The device-safe gate admits the statement the same way
            // it admits the other recipe ops (the operand layouts are still
            // walked and checked).
            MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::TensorTranspose),
                        ..
                    },
                ..
            } => {
                crate::visit::walk_stmt(self, stmt);
            }
            // S6-P2: the transformer recipe ops (`TensorSumAxis` /
            // `TensorSoftmax` / `TensorLayerNorm`) are device-materialization
            // admissions — the S6-P1 plan surface resolves each to a single
            // `CollectionKernelPlan` recipe (`AxisReduction` / `RowSoftmax`
            // / `LayerNormalization`, kernel_plan/build.rs
            // `from_transformer_op`; the authoritative closed set is
            // `is_transformer_recipe_op`, device_program_plans.rs) and the
            // emitters lower the recipe bodies (the S6-P2 Metal arms). The
            // gate admits the statement the same way it admits the other
            // recipe ops; the operand layouts are still walked and checked.
            MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic:
                            MirIntrinsic::Collection(
                                MirCollectionOp::TensorSumAxis
                                | MirCollectionOp::TensorSoftmax
                                | MirCollectionOp::TensorLayerNorm,
                            ),
                        ..
                    },
                ..
            } => {
                crate::visit::walk_stmt(self, stmt);
            }
            MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic:
                            MirIntrinsic::Collection(
                                MirCollectionOp::TensorAdd
                                | MirCollectionOp::TensorSub
                                | MirCollectionOp::TensorMul
                                | MirCollectionOp::TensorDiv
                                | MirCollectionOp::TensorNeg
                                | MirCollectionOp::TensorFill
                                | MirCollectionOp::TensorRelu
                                | MirCollectionOp::TensorGelu,
                            ),
                        ..
                    },
                ..
            } => {
                // S3-A5: the elementwise tensor set is device-safe — the
                // Metal emitter lowers each op to a per-element tensor
                // expression (the companion's missing mul/div/fill arms, plus
                // the pointwise family; TensorAdd/TensorRelu predate S3-A5).
                // The WGSL emitter keeps its per-op fail-closed emit gate.
                crate::visit::walk_stmt(self, stmt);
            }
            MirStatementKind::RuntimeCall { .. } => self.record_policy(SHAPE_KERNEL_RUNTIME_CALL),
            MirStatementKind::Construct {
                aggregate,
                destination,
            } => {
                // S3-A1: a Tuple construct is the multi-output result
                // aggregation — the ABI lowers it to DISTINCT output
                // resources; the container is not a device value.
                //
                // S5-W-A1: a DEAD fold-artifact Array/Tensor construct —
                // the static-shape fold's shape-list (`Array`) / vacua
                // (`Tensor`) artifacts, written to a destination nothing
                // reads — is skipped by both emitters (S5-U2c) and must
                // pass the gate; a LIVE Array/Tensor construct is a true
                // unplannable kernel aggregate and stays fail-closed with
                // the aggregate diagnostics (the statement and its
                // aggregate type are both skipped from classification).
                let dead_fold_artifact = self.dead_fold_artifact_slots.contains(&destination.base)
                    && matches!(
                        aggregate.kind,
                        MirAggregateKind::Array | MirAggregateKind::Tensor
                    );
                if !dead_fold_artifact
                    && !matches!(
                        aggregate.kind,
                        MirAggregateKind::Vector
                            | MirAggregateKind::Matrix
                            | MirAggregateKind::Tuple
                    )
                {
                    self.record_policy(SHAPE_KERNEL_AGGREGATE_CONSTRUCT);
                }
                if dead_fold_artifact {
                    // The destination place is still walked (a write to a
                    // plain slot); the aggregate — and its non-scalar type —
                    // is not classified, matching the emitters' whole-
                    // construct skip.
                    self.visit_place(destination);
                } else {
                    crate::visit::walk_stmt(self, stmt);
                }
            }
        }
    }

    fn visit_terminator(&mut self, terminator: &MirTerminator) {
        if !self.in_device_function {
            return;
        }
        match &terminator.kind {
            MirTerminatorKind::Return(_) => crate::visit::walk_terminator(self, terminator),
            MirTerminatorKind::Branch { .. }
                if self.allows_structured_return_branch
                    || self.allows_structured_indexed_write_branch
                    || self.allows_structured_indexed_value_join_branch
                    || self.allows_structured_value_join_branch =>
            {
                crate::visit::walk_terminator(self, terminator);
            }
            MirTerminatorKind::Goto(_)
                if self.allows_structured_indexed_write_branch
                    || self.allows_structured_return_branch
                    || self.allows_structured_indexed_write_goto
                    || self.allows_structured_indexed_value_join_branch
                    || self.allows_structured_value_join_branch =>
            {
                crate::visit::walk_terminator(self, terminator);
            }
            MirTerminatorKind::Switch { .. } if self.allows_structured_scalar_switch => {
                crate::visit::walk_terminator(self, terminator);
            }
            MirTerminatorKind::Switch { .. } if self.allows_structured_indexed_write_switch => {
                crate::visit::walk_terminator(self, terminator);
            }
            MirTerminatorKind::Goto(_) if self.allows_structured_indexed_write_switch => {
                crate::visit::walk_terminator(self, terminator);
            }
            MirTerminatorKind::Goto(_) if self.allows_structured_scalar_switch => {
                crate::visit::walk_terminator(self, terminator);
            }
            MirTerminatorKind::Goto(_) if self.allows_structured_scalar_return_goto => {
                crate::visit::walk_terminator(self, terminator);
            }
            MirTerminatorKind::Goto(_) | MirTerminatorKind::Branch { .. }
                if self.allows_structured_scalar_loop
                    || self.allows_structured_indexed_write_loop
                    || self.allows_structured_scalar_loop_break
                    || self.allows_structured_indexed_write_loop_break
                    || self.allows_structured_scalar_loop_continue
                    || self.allows_structured_indexed_write_loop_continue
                    || self.allows_structured_nested_scalar_loop_branch
                    || self.allows_structured_nested_indexed_write_loop_branch
                    || self.allows_structured_nested_indexed_write_loop_break
                    || self.allows_structured_nested_indexed_write_loop_continue =>
            {
                crate::visit::walk_terminator(self, terminator);
            }
            MirTerminatorKind::TryCall { .. } => self.record_abi(SHAPE_TRY_CALL),
            MirTerminatorKind::ReturnError(_) => {
                self.record_abi(SHAPE_RETURN_ERROR_DEVICE_TERMINATOR);
            }
            MirTerminatorKind::Unreachable if self.allows_structured_scalar_switch => {
                crate::visit::walk_terminator(self, terminator);
            }
            MirTerminatorKind::Goto(_)
            | MirTerminatorKind::Branch { .. }
            | MirTerminatorKind::Switch { .. }
            | MirTerminatorKind::Unreachable => {
                self.record_policy(format!("device {} terminator", terminator.kind.kind_name()));
                crate::visit::walk_terminator(self, terminator);
            }
        }
    }

    fn visit_value(&mut self, value: &MirValue) {
        if self.in_device_function {
            // S5A-U4: a tuple-typed value is admitted only when its operand
            // base is one of the decomposition's output-set tuple aggregation
            // slots (the explicit result aggregation the ABI flattens). A
            // tuple-typed value whose base is an unrelated aggregate — or a
            // tuple computed by an expression — stays fail-closed.
            let is_result_aggregation = match &value.kind {
                MirValueKind::Operand(MirOperand::Place(place)) => {
                    self.is_result_aggregation_slot(&place.base)
                }
                MirValueKind::Operand(MirOperand::Temp(temp)) => {
                    self.result_aggregation_slots.temps.contains(temp)
                }
                _ => false,
            };
            // S6-P2: an admitted TENSOR unary assign (`MirUnOp::Exp` /
            // `Sqrt` — the softmax exp and the LayerNorm sqrt lanes, plus
            // the pre-existing Tanh/Relu/Gelu) skips the value-type check
            // the same way the elementwise tensor ops do: the P1 seam
            // (`is_admitted_tensor_unary`, device_program_plans.rs) is
            // exactly the set the elementwise emitters dispatch per-element,
            // and the unary's tensor operand layouts are still walked and
            // checked below. A unary on a non-tensor value, or an op outside
            // the admitted set, stays fail-closed with the value-type
            // diagnostics.
            let admitted_tensor_unary = is_admitted_tensor_unary_value(self.types, value);
            if !is_result_aggregation && !admitted_tensor_unary {
                self.check_device_type(value.ty);
            }
            crate::visit::walk_value(self, value);
        }
    }

    fn visit_place(&mut self, place: &MirPlace) {
        if self.in_device_function {
            self.check_place(place);
            crate::visit::walk_place(self, place);
        }
    }

    fn visit_aggregate(&mut self, aggregate: &MirAggregate) {
        if self.in_device_function {
            // S3-A1: a Tuple aggregate is the multi-output result contract —
            // its element types (checked through the field walk) are the
            // DISTINCT device output resources; the tuple container itself is
            // not a device value. Other aggregates (record/map/… containers)
            // stay reject-as-aggregate.
            if !matches!(aggregate.kind, MirAggregateKind::Tuple) {
                self.check_device_type(aggregate.ty);
            }
            crate::visit::walk_aggregate(self, aggregate);
        }
    }
}

fn is_device_vector_element(element: MirScalarLayout) -> bool {
    matches!(
        element,
        MirScalarLayout::Bool
            | MirScalarLayout::I8
            | MirScalarLayout::I16
            | MirScalarLayout::I32
            | MirScalarLayout::U8
            | MirScalarLayout::U16
            | MirScalarLayout::U32
            | MirScalarLayout::F32
    )
}

/// Whether a value is an admitted TENSOR unary assign (S6-P2): a
/// `MirValueKind::Unary` whose op is in the P1-admitted unary set
/// (`is_admitted_tensor_unary`, `device_program_plans.rs` — `Exp`/`Sqrt` are
/// the S6-P1 transformer additions, the softmax exp and the `LayerNorm` sqrt
/// lanes) applied to a TENSOR-typed result. The tensor restriction mirrors
/// the elementwise emitters' tensor-expr surface: a scalar (or otherwise
/// non-tensor) unary is not admitted by this seam and keeps its value-type
/// diagnostics.
fn is_admitted_tensor_unary_value(types: &TypeTable, value: &MirValue) -> bool {
    let MirValueKind::Unary { op, .. } = &value.kind else {
        return false;
    };
    crate::device_program_plans::is_admitted_tensor_unary(*op)
        && matches!(types.get(value.ty.semantic_id()), Type::Tensor(_, _))
}

/// WGSL `matCxR` is f32/f16 only; Stage1 admits f32 register matrices.
fn is_device_matrix_element(element: MirScalarLayout) -> bool {
    matches!(element, MirScalarLayout::F32)
}
