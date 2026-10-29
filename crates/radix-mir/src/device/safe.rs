//! Device-safe lowerability gate: rejects host-only MIR shapes before
//! target-specific emission.

mod dead_artifact;
mod dead_const_copy;

use dead_artifact::{FoldArtifactSlots, dead_fold_artifact_slots, is_fold_artifact_type};
use dead_const_copy::{
    DeadConstCopySlots, dead_non_kernel_scalar_const_slots, is_non_kernel_scalar_layout,
};

use super::roles::MirDeviceContext;
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
use crate::literal_indexed_access::{LiteralIndexedAccessPlan, MirSlot};
use crate::visit::MirVisitor;
use crate::{CapabilityGap, Lowerability};
use crate::{
    MirAggregate, MirAggregateKind, MirCollectionOp, MirConstant, MirDeviceViewElement,
    MirFunction, MirIntrinsic, MirLayoutKind, MirLayoutTable, MirLiftOp, MirLocal, MirOperand,
    MirParam, MirPlace, MirPlaceBase, MirProjection, MirRuntimeCall, MirScalarLayout, MirStatement,
    MirStatementKind, MirTemp, MirTensorLocusLayout, MirTensorStorageLayout, MirTerminator,
    MirTerminatorKind, MirType, MirValidationContext, MirValue, MirValueKind, ValidatedMir,
};
use crate::{MirLocalId, MirStructLayout, resolve_struct_layout};
use handroll::FxHashMap;
use radix_types::MirMathFunction;
use radix_types::{NumericWidth, Primitive, Type, TypeId, TypeTable};

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
/// MLC-4: fail-closed sparse admission row — same spelling as the ABI gate.
const SHAPE_KERNEL_SPARSE_STORAGE: &str = "kernel sparse tensor storage is not admitted";
const SHAPE_DEVICE_PLACE_PROJECTION: &str = "device place projection";
/// gpu-reset Q02: a narrow (`f16`, `i8`, `u8`) field value is loaded natively
/// and converted, never computed on at its own width (math happens in the
/// ether, the width applies at the store), so arithmetic on one is refused.
pub(crate) const SHAPE_KERNEL_NARROW_ARITHMETIC: &str =
    "kernel narrow scalar arithmetic (convert the f16/i8/u8 field value first)";
/// C4-6b: genus (struct) kernel-parameter rows.
const SHAPE_GENUS_PARAM: &str = "device genus parameter";
const SHAPE_GENUS_PARAM_MUTABLE: &str = "device genus parameter: mutable (read-only only)";
const SHAPE_GENUS_PARAM_WRITE: &str = "device genus parameter: field write";
const SHAPE_GENUS_PARAM_PROJECTION: &str =
    "device genus parameter: only a direct scalar or vector field read is admitted";
/// C4-6b follow-up: a genus (struct) LOCAL or temp of a lane-form kernel,
/// scalarized by the lane emitters (`<base>_f<field>`) into one target
/// variable per field. Its fields must each be a value type the lane form
/// admits; the parameter layout law does not apply to a scalarized value.
const SHAPE_GENUS_LOCAL_GENERIC: &str = "device genus local: generic";
const SHAPE_GENUS_LOCAL_OPAQUE: &str = "device genus local: opaque";
const SHAPE_GENUS_LOCAL_UNKNOWN_FIELDS: &str = "device genus local: unknown fields";
const SHAPE_GENUS_LOCAL_EMPTY: &str = "device genus local: empty genus";
const SHAPE_GENUS_LOCAL_NESTED: &str = "device genus local: nested genus field";
const SHAPE_ASYNC_GENERATOR_DEVICE_FN: &str = "async/generator device function";
const SHAPE_DEVICE_MULTI_BLOCK_CFG: &str = "device multi-block control flow";
/// gpu-reset C4-2e: a lane-form kernel (a Lane builtin parameter) whose CFG is
/// acyclic structures into a region tree, which the target lane emitters
/// consume instead of the single-block / shape-matcher forms above. A
/// lane-form CFG the structurizer rejects (an irreducible or multi-level loop, a switch, ...) keeps the
/// multi-block rejection.
fn is_structurizable_lane_form(function: &MirFunction) -> bool {
    function
        .params
        .iter()
        .any(|param| param.mode == crate::MirParamMode::Lane)
        && crate::structurize::structurize(function).is_ok()
}

/// A single-lane kernel (no `thread lane` loop, one `mut` tensor — the shared
/// ABI law [`crate::MirKernelSignature::is_single_lane_form`]) is emitted by
/// the same lane emitters as one sequential lane, so its control flow and
/// scalar conversions take the lane-form admissions. Its sequential body
/// routinely re-joins branch tails inside a loop, which the Metal lane
/// emitter structurizes by duplicating the tails, so the CFG is judged by
/// that same structurizer. The genus admissions stay lane-form only.
fn is_structurizable_single_lane(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> bool {
    crate::MirKernelSignature::is_single_lane_form(function, validation)
        && crate::structurize::structurize_duplicating_tails(function).is_ok()
}

/// Functions a whole-grid reduction or scan runs inside its lane region: the
/// term helper and each grouped-key helper. They inherit lane-form admissions
/// and structured-CFG emission without receiving their own Lane parameter.
fn reduction_term_functions(
    program: &crate::MirProgram,
) -> handroll::FxHashSet<crate::MirFunctionId> {
    let mut terms = handroll::FxHashSet::default();
    for function in &program.functions {
        for block in &function.blocks {
            for statement in &block.statements {
                let MirStatementKind::RuntimeCall {
                    call:
                        MirRuntimeCall {
                            intrinsic: MirIntrinsic::FilumReduce(filum),
                            args,
                            ..
                        },
                    ..
                } = &statement.kind
                else {
                    continue;
                };
                let Some(group_operands) = group_operand_count(filum) else {
                    continue;
                };
                let Some(group_start) = args.len().checked_sub(group_operands) else {
                    continue;
                };
                if filum.term_captures.is_some()
                    && let Some(position) = group_start.checked_sub(1)
                    && let Some(MirOperand::Constant(MirConstant::Function(term))) =
                        args.get(position)
                {
                    terms.insert(*term);
                }
                if let Some(group) = &filum.group {
                    let mut position = group_start;
                    for key in &group.keys {
                        let Some(function_position) = position.checked_add(key.captures as usize)
                        else {
                            continue;
                        };
                        if let Some(MirOperand::Constant(MirConstant::Function(helper))) =
                            args.get(function_position)
                        {
                            terms.insert(*helper);
                        }
                        position = match function_position.checked_add(1) {
                            Some(position) => position,
                            None => continue,
                        };
                    }
                }
            }
            for statement in &block.statements {
                let MirStatementKind::RuntimeCall {
                    call:
                        MirRuntimeCall {
                            intrinsic: MirIntrinsic::Scan(scan),
                            args,
                            ..
                        },
                    ..
                } = &statement.kind
                else {
                    continue;
                };
                let group_operands = scan.group.as_ref().map_or(Some(0), |group| {
                    group.axes.iter().try_fold(0_usize, |count, axis| {
                        count.checked_add(usize::try_from(axis.captures).ok()?.checked_add(1)?)
                    })
                });
                let Some(group_operands) = group_operands else {
                    continue;
                };
                let Some(group_start) = args.len().checked_sub(group_operands) else {
                    continue;
                };
                let Ok(term_captures) = usize::try_from(scan.term_captures) else {
                    continue;
                };
                let Some(term_position) = 1_usize.checked_add(term_captures) else {
                    continue;
                };
                if term_position < group_start
                    && let Some(MirOperand::Constant(MirConstant::Function(term))) =
                        args.get(term_position)
                {
                    terms.insert(*term);
                }
                if let Some(group) = &scan.group {
                    let mut position = group_start;
                    for axis in &group.axes {
                        let Ok(captures) = usize::try_from(axis.captures) else {
                            continue;
                        };
                        let Some(function_position) = position.checked_add(captures) else {
                            continue;
                        };
                        if let Some(MirOperand::Constant(MirConstant::Function(helper))) =
                            args.get(function_position)
                        {
                            terms.insert(*helper);
                        }
                        position = match function_position.checked_add(1) {
                            Some(position) => position,
                            None => continue,
                        };
                    }
                }
            }
        }
    }
    terms
}

fn group_operand_count(filum: &crate::MirFilumReduceOp) -> Option<usize> {
    filum.group.as_ref().map_or(Some(0), |group| {
        group.keys.iter().try_fold(0_usize, |count, key| {
            count.checked_add(usize::try_from(key.captures).ok()?.checked_add(1)?)
        })
    })
}

const SHAPE_FAILABLE_DEVICE_FN: &str = "failable device function";
const SHAPE_DEVICE_FUNCTION_CALL: &str = "device function call";
const SHAPE_KERNEL_RUNTIME_CALL: &str = "kernel runtime call";
const SHAPE_KERNEL_AGGREGATE_CONSTRUCT: &str = "kernel aggregate construct";
const SHAPE_KERNEL_TENSOR_VIEW_OP: &str =
    "kernel tensor view operation requires explicit materialize";
const SHAPE_TRY_CALL: &str = "try_call";
const SHAPE_RETURN_ERROR_DEVICE_TERMINATOR: &str = "return_error device terminator";

/// EXEC02-PC1: name the collection op that hit the wildcard runtime-call
/// reject (e.g. `TensorBatchedAttention`). Runtime conversions are outside
/// the kernel subset and are named `conversio`; other non-collection
/// intrinsics keep the bare shape where their stable diagnostics identify
/// the statement.
fn runtime_call_op_name(intrinsic: &MirIntrinsic) -> String {
    match intrinsic {
        MirIntrinsic::Collection(op) => format!("{op:?}"),
        MirIntrinsic::Convert(_) => "conversio".to_owned(),
        _ => String::new(),
    }
}

#[must_use]
pub fn classify_device_safe(
    validated: &ValidatedMir<'_>,
    device: &MirDeviceContext,
) -> Lowerability {
    let mut classifier = DeviceSafeClassifier::new(validated.validation(), device);
    classifier.reduction_terms = reduction_term_functions(validated.program());
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
    classifier.reduction_terms = reduction_term_functions(validated.program());
    classifier.visit_program(validated.program());
    classifier.finish()
}

struct DeviceSafeClassifier<'a> {
    validation: &'a MirValidationContext<'a>,
    types: &'a TypeTable,
    layouts: &'a MirLayoutTable,
    device: &'a MirDeviceContext,
    in_device_function: bool,
    /// The function under check is a structurizable lane-form kernel, whose
    /// `int` coordinate binders are admitted as 64-bit locals (C4-2e).
    in_lane_form_function: bool,
    /// The function under check is a structurizable single-lane kernel: its
    /// control flow and scalar conversions are admitted like a lane form's.
    in_single_lane_function: bool,
    /// The reduction term functions of the program (see
    /// [`reduction_term_functions`]).
    reduction_terms: handroll::FxHashSet<crate::MirFunctionId>,
    /// C4-6b: the genus (struct) PARAMETERS of the lane-form function under
    /// visit whose layout resolves, keyed by parameter local. A read of one
    /// field (`p.f`) is the only admitted use.
    genus_params: FxHashMap<MirLocalId, MirStructLayout>,
    /// The lane-form function's record-element device-view PARAMETERS
    /// (`tensor<Block, [n]>`) whose element record layout resolves, keyed by
    /// parameter local. Indexing the view and reading fields of the element
    /// (`blocks[i].f`, `blocks[i].f[j]`, `blocks[i].f.lane`) is the only
    /// admitted use.
    record_view_params: FxHashMap<MirLocalId, MirStructLayout>,
    /// The narrow (`f16`, `i8`, `u8`) locals and temps of the lane-form
    /// function under visit: values that may be moved and converted but never
    /// computed on (see [`DeviceSafeClassifier::check_narrow_arithmetic`]).
    narrow_locals: handroll::FxHashSet<MirLocalId>,
    narrow_temps: handroll::FxHashSet<crate::MirTempId>,
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
    /// Literal-coordinate tensor accesses whose compile-time index carriers,
    /// optional get carriers, and complete output coverage were proven by the
    /// shared MIR analysis.
    literal_indexed_access: Option<LiteralIndexedAccessPlan>,
    /// U9-6: the slots holding the constant text of each `ReturnError` of the
    /// function under visit, when EVERY `ReturnError` is a constant text throw
    /// of a text-channel function (`Some`); the throw terminators and their
    /// text slots are then fault sites, not host values. `None` keeps the
    /// failable shapes closed.
    throw_slots: Option<Vec<MirPlaceBase>>,
    gaps: Vec<CapabilityGap>,
}

impl<'a> DeviceSafeClassifier<'a> {
    fn new(validation: &'a MirValidationContext<'a>, device: &'a MirDeviceContext) -> Self {
        Self {
            validation,
            types: validation.types.as_table(),
            layouts: &validation.layouts,
            device,
            in_device_function: false,
            in_lane_form_function: false,
            in_single_lane_function: false,
            reduction_terms: handroll::FxHashSet::default(),
            genus_params: FxHashMap::default(),
            record_view_params: FxHashMap::default(),
            narrow_locals: handroll::FxHashSet::default(),
            narrow_temps: handroll::FxHashSet::default(),
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
            literal_indexed_access: None,
            throw_slots: None,
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

    /// Whether `call` is a `Convert` between two numeric scalar types with no
    /// store site and no recovery operand.
    fn is_scalar_extremum(&self, call: &MirRuntimeCall) -> bool {
        matches!(
            call.intrinsic,
            MirIntrinsic::Collection(MirCollectionOp::Minimum | MirCollectionOp::Maximum)
        ) && call.args.len() == 2
            && matches!(
                self.types.get(call.return_ty.semantic_id()),
                Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                    | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
            )
    }

    /// The implicit store of a `d64` value into a `d64` cell (kernel-wide
    /// types ruling 2026-10-28): the rounding, range check and fault the lane
    /// emitters lower.
    fn is_d64_store_conversion(&self, call: &MirRuntimeCall) -> bool {
        let MirIntrinsic::Convert(conversion) = &call.intrinsic else {
            return false;
        };
        conversion.recovery.is_none()
            && conversion.params.is_empty()
            && call.args.len() == 1
            && crate::is_d64_type(conversion.source_ty, self.types)
            && crate::is_d64_type(conversion.target_ty, self.types)
    }

    fn is_plain_scalar_conversion(&self, call: &MirRuntimeCall) -> bool {
        let MirIntrinsic::Convert(conversion) = &call.intrinsic else {
            return false;
        };
        let is_scalar = |ty: MirType| {
            matches!(
                self.types.get(ty.semantic_id()),
                Type::Primitive(Primitive::Numerus | Primitive::Fractus)
                    | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
            )
        };
        conversion.store.is_none()
            && conversion.recovery.is_none()
            && conversion.params.is_empty()
            && call.args.len() == 1
            && is_scalar(conversion.source_ty)
            && is_scalar(conversion.target_ty)
    }

    fn check_device_type(&mut self, ty: MirType) {
        let semantic = self.types.get(ty.semantic_id()).clone();
        match semantic {
            Type::Primitive(Primitive::Bivalens | Primitive::Vacuum | Primitive::Numquam)
            | Type::SizedNumeric(Primitive::Fractus, _)
            | Type::Alias(_, _)
            | Type::ModularWord(_)
            | Type::Bits(_)
            | Type::Atomic(_)
            | Type::Tensor(_, _)
            | Type::Vector(_, _)
            | Type::Matrix(_, _) => self.check_device_layout(ty),
            Type::Param(_)
            | Type::Infer(_)
            | Type::InferUnion(_)
            | Type::MarkerHole { .. }
            | Type::Error => {
                self.record_layout(SHAPE_UNRESOLVED_TYPE);
            }
            Type::TensorEdgePolicy(_) => {
                self.record_policy("tensor edge-policy marker is not a runtime value type");
            }
            Type::Primitive(Primitive::Numerus | Primitive::Fractus)
            | Type::SizedNumeric(Primitive::Numerus, _) => {
                self.check_device_layout(ty);
            }
            // D11.1: no device clamp lowering yet — fail closed.
            // N7E-1: the policy-float cell has no device lowering; D6 splits
            // `trapping` (a language rule) from `saturating` (temporary) in N7E-2.
            Type::Primitive(
                Primitive::Textus
                | Primitive::Ascii
                | Primitive::Littera
                | Primitive::Octeti
                | Primitive::Regex
                | Primitive::Match
                | Primitive::Json
                | Primitive::Valor
                | Primitive::Ignotum
                | Primitive::Instans,
            )
            | Type::SizedInstans(_)
            | Type::SaturatingWord(_)
            | Type::RangedInt { .. }
            | Type::PolicyFloat(_, _)
            | Type::SizedNumeric(_, _) => {
                self.record_policy(format!("device host value type {semantic:?}"));
            }
            Type::Array(inner) if self.is_vector_surrogate_element(inner) => {
                self.record_policy(SHAPE_DEVICE_VECTOR_SURROGATE);
            }
            // gpu-reset C4-6b follow-up: a genus LOCAL or temp in a lane-form
            // kernel is scalarized field-by-field by the lane emitters; only
            // its fields need value-type admission (no memory layout).
            Type::Struct(_) if self.in_lane_form_function => {
                self.check_genus_local_type(ty);
            }
            Type::Array(_)
            | Type::Tuple(_)
            | Type::Map(_, _)
            | Type::Ledger(_, _)
            | Type::Record(_)
            | Type::Set(_)
            | Type::Promissum(_)
            | Type::PromissumFailable(_, _)
            | Type::AsyncCursor(_, _)
            | Type::Cursor(_)
            | Type::Queue(_)
            | Type::Stack(_)
            | Type::Sparsa(_, _)
            | Type::Intervallum(..)
            | Type::Option(_)
            | Type::Ref(_, _)
            | Type::Enum(_)
            | Type::Interface(_)
            | Type::Func(_)
            | Type::Applied(_, _)
            | Type::Union(_)
            | Type::Intersection(_)
            | Type::BoundedArray { .. }
            | Type::RangedArray { .. }
            | Type::BoundedQueue { .. }
            | Type::BoundedStack { .. }
            | Type::BoundedTextus { .. }
            | Type::BoundedAscii { .. }
            | Type::BoundedOcteti { .. }
            | Type::Struct(_) => {
                self.record_policy(format!("device aggregate value type {semantic:?}"));
            }
            Type::Primitive(Primitive::Nihil) => self.record_policy(SHAPE_DEVICE_NIHIL_VALUE),
        }
    }

    fn check_device_layout(&mut self, ty: MirType) {
        match self.layouts.layout_for_type(ty) {
            Ok(layout) => match layout.kind {
                MirLayoutKind::Scalar(
                    MirScalarLayout::Bf16
                    | MirScalarLayout::F32
                    | MirScalarLayout::I32
                    | MirScalarLayout::U32
                    | MirScalarLayout::Bool,
                )
                | MirLayoutKind::Void
                | MirLayoutKind::Never => {}
                MirLayoutKind::Scalar(
                    MirScalarLayout::I64 | MirScalarLayout::U64 | MirScalarLayout::D64,
                ) if self.in_lane_form_function || self.in_single_lane_function => {}
                // gpu-reset Q02: a narrow field read in place. A lane-form
                // kernel that reads a record-element view (or a genus
                // parameter) loads an `f16`, `i8` or `u8` field at its own
                // width, natively (MSL `half`, `char`, `uchar`), and widens
                // it where the source converts; nothing is emulated. The
                // allowance is tied to such a kernel so no other kernel gains
                // a narrow value type, and a target without the native load
                // refuses the kernel by name in its own emitter.
                MirLayoutKind::Scalar(
                    MirScalarLayout::F16 | MirScalarLayout::I8 | MirScalarLayout::U8,
                ) if self.in_lane_form_function
                    && (!self.record_view_params.is_empty() || !self.genus_params.is_empty()) => {}
                // Every other scalar is rejected by name — including `Magnus`
                // (the unbounded `inf`, ruling 39(d)): "kernel scalar layout magnus".
                MirLayoutKind::Scalar(scalar) => {
                    self.record_layout(format!("kernel scalar layout {}", scalar.spelling()));
                }
                MirLayoutKind::Aggregate(_) => {
                    self.record_layout(SHAPE_KERNEL_AGGREGATE_LAYOUT);
                }
                MirLayoutKind::DeviceView(view) => self.check_device_view(view),
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

    fn check_device_view(&mut self, view: crate::MirDeviceViewLayout) {
        // MLC-4: sparse storage is rejected by name, fail-closed,
        // before the host-owned entry and generic storage rows.
        if view.storage == MirTensorStorageLayout::Sparse {
            self.record_policy(SHAPE_KERNEL_SPARSE_STORAGE);
            return;
        }
        let host_owned_entry = self.allow_host_owned_tensor_entry
            && view.storage == MirTensorStorageLayout::HostOwned
            && view.locus == MirTensorLocusLayout::Host;
        if host_owned_entry {
            return;
        }
        if view.storage != MirTensorStorageLayout::DeviceHandle
            || view.locus != MirTensorLocusLayout::Device
        {
            self.record_policy(SHAPE_KERNEL_HOST_TENSOR_STORAGE);
        } else if !view.row_major || !view.contiguous || view.offset_elements != 0 {
            self.record_layout("kernel dense tensor view layout");
        } else if view.rank != 1 {
            self.record_layout(format!("kernel device view rank {}", view.rank));
        } else if let Some(element) = view.element.scalar() {
            if !matches!(
                element,
                MirScalarLayout::Bf16
                    | MirScalarLayout::F32
                    | MirScalarLayout::I32
                    | MirScalarLayout::U32
                    | MirScalarLayout::I64
                    | MirScalarLayout::U64
                    | MirScalarLayout::D64
                    | MirScalarLayout::U8
            ) {
                self.record_layout(format!("kernel device view element {}", element.spelling()));
            }
        } else if let MirDeviceViewElement::Vector(vector) = view.element
            && (!is_device_view_vector(vector.width) || !is_device_vector_element(vector.element))
        {
            self.record_layout(format!(
                "kernel vector-element device view width {} element {}",
                vector.width,
                vector.element.spelling()
            ));
        } else if let MirDeviceViewElement::Record(record_ty) = view.element
            && let Err(refusal) =
                resolve_struct_layout(MirType::semantic(record_ty), self.validation)
        {
            // WHY: a resolved record element is admitted here; a refused one
            // is reported by the layout resolver's own reason.
            self.record_layout(format!("kernel device view element: {}", refusal.reason()));
        }
    }

    fn is_narrow_ty(&self, ty: MirType) -> bool {
        matches!(
            self.layouts.layout_for_type(ty).map(|layout| layout.kind),
            Ok(MirLayoutKind::Scalar(
                MirScalarLayout::F16 | MirScalarLayout::I8 | MirScalarLayout::U8
            ))
        )
    }

    /// Whether an operand reads a narrow value: a narrow local or temp, or a
    /// field read of a record-element view or genus parameter whose field
    /// scalar is narrow.
    fn is_narrow_operand(&self, operand: &MirOperand) -> bool {
        match operand {
            MirOperand::Temp(temp) => self.narrow_temps.contains(temp),
            MirOperand::Place(place) if place.projections.is_empty() => match place.base {
                MirPlaceBase::Local(local) => self.narrow_locals.contains(&local),
                MirPlaceBase::Temp(temp) => self.narrow_temps.contains(&temp),
            },
            MirOperand::Place(place) => {
                let layout = self
                    .record_view_base(place)
                    .or_else(|| self.genus_param_base(place));
                let field = place
                    .projections
                    .iter()
                    .find_map(|projection| match projection {
                        MirProjection::Field(name) => Some(*name),
                        _ => None,
                    });
                layout.zip(field).is_some_and(|(layout, name)| {
                    layout.fields.iter().any(|candidate| {
                        candidate.name == name
                            && matches!(
                                candidate.element,
                                MirDeviceViewElement::Scalar(
                                    MirScalarLayout::F16
                                        | MirScalarLayout::I8
                                        | MirScalarLayout::U8
                                )
                            )
                    })
                })
            }
            MirOperand::Value(_) | MirOperand::Constant(_) => false,
        }
    }

    /// Arithmetic on a narrow value is refused: the kernel loads and converts
    /// it (`half` widening to f32 is exact, `char` to an integer is exact),
    /// and does not compute at the narrow width.
    fn check_narrow_arithmetic(&mut self, value: &MirValue) {
        let narrow = match &value.kind {
            MirValueKind::Unary { operand, .. } => self.is_narrow_operand(operand),
            MirValueKind::Binary { lhs, rhs, .. } => {
                self.is_narrow_operand(lhs) || self.is_narrow_operand(rhs)
            }
            _ => return,
        };
        if narrow || self.is_narrow_ty(value.ty) {
            self.record_policy(SHAPE_KERNEL_NARROW_ARITHMETIC);
        }
    }

    fn is_vector_surrogate_element(&self, inner: TypeId) -> bool {
        matches!(
            self.types.get(inner),
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
        )
    }

    fn is_genus_type(&self, ty: MirType) -> bool {
        matches!(self.types.get(ty.semantic_id()), Type::Struct(_))
    }

    /// The lane-form function's genus parameters whose layout resolves.
    fn resolved_genus_params(
        &self,
        function: &MirFunction,
    ) -> FxHashMap<MirLocalId, MirStructLayout> {
        if !self.in_lane_form_function {
            return FxHashMap::default();
        }
        function
            .params
            .iter()
            .filter(|param| param.mode != crate::MirParamMode::MutRef)
            .filter(|param| self.is_genus_type(param.ty))
            .filter_map(|param| {
                resolve_struct_layout(param.ty, self.validation)
                    .ok()
                    .map(|layout| (param.local, layout))
            })
            .collect()
    }

    /// The lane-form function's record-element device-view parameters whose
    /// element record layout resolves.
    fn resolved_record_view_params(
        &self,
        function: &MirFunction,
    ) -> FxHashMap<MirLocalId, MirStructLayout> {
        if !self.in_lane_form_function {
            return FxHashMap::default();
        }
        function
            .params
            .iter()
            .filter(|param| param.mode != crate::MirParamMode::MutRef)
            .filter_map(|param| {
                let layout = self.layouts.layout_for_type(param.ty).ok()?;
                let MirLayoutKind::DeviceView(view) = layout.kind else {
                    return None;
                };
                let MirDeviceViewElement::Record(record_ty) = view.element else {
                    return None;
                };
                resolve_struct_layout(MirType::semantic(record_ty), self.validation)
                    .ok()
                    .map(|layout| (param.local, layout))
            })
            .collect()
    }

    /// Whether the place base is one of the function's record-element device
    /// view parameters.
    fn record_view_base(&self, place: &MirPlace) -> Option<&MirStructLayout> {
        match place.base {
            MirPlaceBase::Local(local) => self.record_view_params.get(&local),
            MirPlaceBase::Temp(_) => None,
        }
    }

    /// C4-6b: a genus parameter is admitted by value and read-only when its
    /// layout resolves; a refusal surfaces by the resolver's name.
    fn check_genus_param(&mut self, param: &MirParam) {
        if param.mode == crate::MirParamMode::MutRef {
            self.record_policy(SHAPE_GENUS_PARAM_MUTABLE);
        } else if let Err(refusal) = resolve_struct_layout(param.ty, self.validation) {
            self.record_layout(format!("{SHAPE_GENUS_PARAM}: {}", refusal.reason()));
        }
    }

    /// C4-6b follow-up: a genus (struct) LOCAL or temp of a lane-form kernel
    /// is scalarized by the lane emitters into one target variable per field
    /// (`<base>_f<field>`), so it needs no memory layout and is admitted when
    /// every declared field type is itself a value type the lane form admits
    /// — `resolve_struct_layout` is not used here because a scalarized value
    /// has no layout and A2's `Geo` (`vector<f32, size 3>` fields) would be refused
    /// by the parameter layout law.
    fn check_genus_local_type(&mut self, ty: MirType) {
        let Type::Struct(def_id) = self.types.get(ty.semantic_id()).clone() else {
            return;
        };
        let validation = self.validation;
        if validation
            .struct_type_params
            .get(&def_id)
            .is_some_and(|params| !params.is_empty())
        {
            self.record_policy(SHAPE_GENUS_LOCAL_GENERIC);
        }
        if validation.opaque_structs.contains(&def_id) {
            self.record_policy(SHAPE_GENUS_LOCAL_OPAQUE);
        }
        let Some(order) = validation.struct_field_order.get(&def_id) else {
            self.record_policy(SHAPE_GENUS_LOCAL_UNKNOWN_FIELDS);
            return;
        };
        if order.is_empty() {
            self.record_policy(SHAPE_GENUS_LOCAL_EMPTY);
            return;
        }
        let Some(fields) = validation.struct_fields.get(&def_id) else {
            self.record_policy(SHAPE_GENUS_LOCAL_UNKNOWN_FIELDS);
            return;
        };
        for name in order {
            let Some(field_ty) = fields.get(name).copied() else {
                self.record_policy(SHAPE_GENUS_LOCAL_UNKNOWN_FIELDS);
                continue;
            };
            if matches!(self.types.get(field_ty.semantic_id()), Type::Struct(_)) {
                self.record_policy(SHAPE_GENUS_LOCAL_NESTED);
            } else {
                self.check_device_type(field_ty);
            }
        }
    }

    /// A write whose destination is rooted at a genus parameter is refused.
    fn check_genus_write(&mut self, stmt: &MirStatement) {
        let destination = match &stmt.kind {
            MirStatementKind::Assign { place, .. } => Some(place),
            MirStatementKind::Call { destination, .. }
            | MirStatementKind::RuntimeCall { destination, .. } => destination.as_ref(),
            MirStatementKind::Construct { destination, .. } => Some(destination),
        };
        if destination.is_some_and(|place| self.genus_param_base(place).is_some()) {
            self.record_policy(SHAPE_GENUS_PARAM_WRITE);
        }
    }

    fn genus_param_base(&self, place: &MirPlace) -> Option<&MirStructLayout> {
        match place.base {
            MirPlaceBase::Local(local) => self.genus_params.get(&local),
            MirPlaceBase::Temp(_) => None,
        }
    }

    fn check_place(&mut self, place: &MirPlace) {
        if let Some(layout) = self.genus_param_base(place) {
            let admitted = matches!(
                place.projections.as_slice(),
                [MirProjection::Field(name)] if layout.fields.iter().any(|field| field.name == *name)
            );
            if !admitted {
                self.record_policy(SHAPE_GENUS_PARAM_PROJECTION);
            }
            return;
        }
        // A record-element device view: the only admitted projections are one
        // index into the view followed by a read of one declared field
        // (`blocks[i].f`), a fixed-array element of that field
        // (`blocks[i].f[j]`), or a lane of that field (`blocks[i].f.lane`).
        if let Some(layout) = self.record_view_base(place) {
            let field_read =
                |name: &radix_lexer::Symbol| layout.fields.iter().any(|field| field.name == *name);
            let admitted = match place.projections.as_slice() {
                [
                    MirProjection::Index(_) | MirProjection::ProvenIndex(_),
                    MirProjection::Field(name),
                ]
                | [
                    MirProjection::Index(_) | MirProjection::ProvenIndex(_),
                    MirProjection::Field(name),
                    MirProjection::Index(_)
                    | MirProjection::ProvenIndex(_)
                    | MirProjection::VectorLane(_),
                ] => field_read(name),
                _ => false,
            };
            if !admitted {
                self.record_policy(SHAPE_DEVICE_PLACE_PROJECTION);
            }
            return;
        }
        // C4-6b follow-up: a scalarized genus local's field place (and a lane
        // of a vector field) is exactly what the lane emitters' operand
        // lowering handles (`<base>_f<field>[.lane]`); no other projection
        // shape is admitted.
        if self.in_lane_form_function
            && matches!(
                place.projections.as_slice(),
                [MirProjection::Field(_)] | [MirProjection::Field(_), MirProjection::VectorLane(_)]
            )
        {
            return;
        }
        if !place.projections.iter().all(|projection| {
            matches!(
                projection,
                MirProjection::Index(_)
                    | MirProjection::ProvenIndex(_)
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

    /// U9-6: whether `base` holds the constant text of a throw site.
    fn is_throw_slot(&self, base: &MirPlaceBase) -> bool {
        self.throw_slots
            .as_ref()
            .is_some_and(|slots| slots.contains(base))
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
    /// WHY no per-function save/restore: `crate::visit::walk_function`
    /// visits only the current function's params, locals, temps and blocks,
    /// so this method never re-enters. `classify_structured_allowances` and
    /// `classify_device_function_slots` overwrite every per-function field
    /// before the walk, and every reader is guarded by
    /// `in_device_function`.
    fn visit_function(&mut self, function: &MirFunction) {
        self.in_device_function = self.device.role_for_function(function).is_some();

        if self.in_device_function {
            classify_structured_allowances(self, function);
            classify_device_function_slots(self, function);
            crate::visit::walk_function(self, function);
        }
    }

    fn visit_param(&mut self, param: &MirParam) {
        if self.in_device_function {
            if self.is_genus_type(param.ty) && self.in_lane_form_function {
                self.check_genus_param(param);
            } else {
                self.check_device_type(param.ty);
            }
        }
    }

    fn visit_local(&mut self, local: &MirLocal) {
        if self.in_device_function
            && !self.genus_params.contains_key(&local.id)
            && !self
                .result_aggregation_slots
                .contains(&MirPlaceBase::Local(local.id))
            && !self.dead_fold_artifact_slots.locals.contains(&local.id)
            && !self.dead_const_copy_slots.locals.contains(&local.id)
            && !self.is_throw_slot(&MirPlaceBase::Local(local.id))
            && !self.literal_indexed_access.as_ref().is_some_and(|plan| {
                plan.is_index_slot(&MirPlace::local(local.id))
                    || plan.is_get_slot(MirSlot::Local(local.id))
            })
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
            && !self.is_throw_slot(&MirPlaceBase::Temp(temp.id))
            && !self.literal_indexed_access.as_ref().is_some_and(|plan| {
                plan.is_index_slot(&MirPlace::temp(temp.id))
                    || plan.is_get_slot(MirSlot::Temp(temp.id))
            })
        {
            self.check_device_type(temp.ty);
        }
    }

    fn visit_stmt(&mut self, stmt: &MirStatement) {
        if !self.in_device_function {
            return;
        }
        self.check_genus_write(stmt);
        match &stmt.kind {
            MirStatementKind::Assign { place, value } => {
                classify_assign_statement(self, stmt, place, value);
            }
            MirStatementKind::Call { .. } => self.record_policy(SHAPE_DEVICE_FUNCTION_CALL),
            MirStatementKind::RuntimeCall { call, .. }
                if matches!(call.intrinsic, MirIntrinsic::GpuBuiltin(_)) =>
            {
                crate::visit::walk_stmt(self, stmt);
            }
            MirStatementKind::RuntimeCall {
                destination,
                call:
                    MirRuntimeCall {
                        intrinsic:
                            MirIntrinsic::Collection(
                                MirCollectionOp::TensorGet | MirCollectionOp::TensorSet,
                            ),
                        args,
                        ..
                    },
            } if self.literal_indexed_access.is_some() => {
                classify_literal_indexed_access(self, destination, args);
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
            // S3-A5: the elementwise tensor set is device-safe — the
            // Metal emitter lowers each op to a per-element tensor
            // expression (the companion's missing mul/div/fill arms, plus
            // the pointwise family; TensorAdd/TensorRelu predate S3-A5).
            // The WGSL emitter keeps its per-op fail-closed emit gate.
            //
            // S5-U2: rank-2 `TensorTranspose` is a recipe op — the plan pass
            // admits it (`CollectionKernelPlan::Transpose`, kernel_plan.rs),
            // the ABI resolves its dispatch contract (workgroup 64, dispatch
            // M·N), and the Metal emitter lowers the column-major remap
            // body. The device-safe gate admits the statement the same way
            // it admits the other recipe ops (the operand layouts are still
            // walked and checked).
            //
            // S6-P2: the transformer recipe ops (`TensorSumAxis` /
            // `TensorSoftmax` / `TensorLayerNorm`) are device-materialization
            // admissions — the S6-P1 plan surface resolves each to a single
            // `CollectionKernelPlan` recipe (`AxisReduction` / `RowSoftmax`
            // / `LayerNormalization`, kernel_plan/build.rs
            // `from_transformer_op`; the authoritative closed set is
            // `is_transformer_recipe_op`, device_program_plans.rs) and the
            // emitters lower the recipe bodies (the S6-P2 Metal arms). The
            // GI3-1 inference recipes (`TensorRmsNorm` / `TensorRope` /
            // `TensorGather` / `TensorCausalMaskedSoftmax`) follow the same
            // `from_transformer_op`-sibling admission path (gi3-contract.md
            // §1 — `RmsNormalization` / `Rope` / `Gather` /
            // `CausalMaskedSoftmax`, all members of the same
            // `is_transformer_recipe_op` closed set) and the GI3-4 NVVM /
            // GI3-3 Metal emitter arms lower the recipe bodies. The gate
            // admits the statements the same way it admits the other
            // recipe ops; the operand layouts are still walked and checked.
            //
            // SFR-7: a `FilumSum` declaration is the author-stated recipe
            // (SFR-5 plan entry) — the emitters lower it in place (the
            // Metal native simdgroup arm, the NVVM warp collective). The
            // gate admits the statement the same way it admits the other
            // recipe ops; the operand layouts are still walked and checked.
            MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic:
                            MirIntrinsic::Collection(
                                MirCollectionOp::Sum
                                    | MirCollectionOp::TensorMean
                                    | MirCollectionOp::TensorMax
                                    | MirCollectionOp::TensorMin
                                    | MirCollectionOp::TensorMatMul
                                    | MirCollectionOp::TensorQuantizedMatMul
                                    // EXEC02-PS3: `TensorSsmConv1d` is a recipe
                                    // op (`CollectionKernelPlan::SsmConv1d`) with
                                    // an NVVM body emitter; other targets keep
                                    // their own per-op fail-closed emit gates.
                                    | MirCollectionOp::TensorSsmConv1d
                                    // EXEC02-PC1: `TensorSsmScan` and
                                    // `TensorGroupedMatMul` follow the same
                                    // recipe admission (`nvvm/ssm_scan.rs` /
                                    // `nvvm/grouped_matmul.rs` body emitters);
                                    // other targets keep their own per-op
                                    // fail-closed emit gates. Ops with no NVVM
                                    // body (e.g. a malformed attention shape)
                                    // stay fail-closed in their NVVM emitter.
                                    | MirCollectionOp::TensorSsmScan
                                    | MirCollectionOp::TensorGroupedMatMul
                                    // EXEC02-PC1 split: batched attention now has
                                    // a real NVVM body (`nvvm/batched_attention.rs`)
                                    // and follows the same target-local admission.
                                    | MirCollectionOp::TensorBatchedAttention
                                    | MirCollectionOp::TensorTranspose
                                    | MirCollectionOp::TensorSumAxis
                                    | MirCollectionOp::TensorSoftmax
                                    | MirCollectionOp::TensorLayerNorm
                                    | MirCollectionOp::TensorRmsNorm
                                    | MirCollectionOp::TensorRope
                                    | MirCollectionOp::TensorRopeHalf
                                    | MirCollectionOp::TensorGather
                                    | MirCollectionOp::TensorCausalMaskedSoftmax
                                    | MirCollectionOp::TensorAdd
                                    | MirCollectionOp::TensorSub
                                    | MirCollectionOp::TensorMul
                                    | MirCollectionOp::TensorDiv
                                    | MirCollectionOp::TensorNeg
                                    | MirCollectionOp::TensorFill
                                    | MirCollectionOp::TensorExpand
                                    | MirCollectionOp::TensorRelu
                                    | MirCollectionOp::TensorGelu
                                    // The math functions the Metal elementwise
                                    // lane lowers per element (`t.sqrt()` /
                                    // `t.exp()`); the other `MirMathFunction`
                                    // members keep the named reject below.
                                    | MirCollectionOp::TensorMath(MirMathFunction::Sqrt)
                                    | MirCollectionOp::TensorMath(MirMathFunction::Exp),
                            )
                            | MirIntrinsic::FilumReduce(_)
                            | MirIntrinsic::Scan(_)
                            // gpu-reset rule 15 K4 / need item 4: the
                            // pointwise tensor lifts (add/sub/mul/neg) are
                            // device-safe elementwise expressions — targets that
                            // lower the `Collection` pointwise set lower these
                            // through the same tensor-expr model. Every other
                            // `MirLiftOp` keeps the wildcard rejection below, and
                            // a target that has no lift lowering still fails
                            // closed in its own emit gate.
                            | MirIntrinsic::TensorLift(
                                MirLiftOp::Add
                                | MirLiftOp::Sub
                                | MirLiftOp::Mul
                                | MirLiftOp::Neg
                                // The float division lifts lower through the
                                // same tensor-expr model (the TensorDiv arm's
                                // semantics); the comparison and Mod lifts
                                // keep the wildcard rejection below.
                                | MirLiftOp::Div
                                | MirLiftOp::TrueDiv,
                            ),
                        ..
                    },
                ..
            } => {
                crate::visit::walk_stmt(self, stmt);
            }
            // A lane-form kernel's plain numeric scalar conversion (no
            // implicit-store site, no recovery) is a per-lane cast the lane
            // emitters lower (`lane conversion`); the operand layouts are
            // still walked and checked. Every other conversion stays a named
            // reject below, and a lane emitter that has no cast for the
            // pair fails closed in its own emit gate.
            MirStatementKind::RuntimeCall { call, .. }
                if (self.in_lane_form_function || self.in_single_lane_function)
                    && (self.is_plain_scalar_conversion(call)
                        || self.is_d64_store_conversion(call)) =>
            {
                crate::visit::walk_stmt(self, stmt);
            }
            // A lane-form kernel's scalar `⤓` / `⤒` (and the clamps a shift
            // view resolves to) is a per-lane `min` / `max` the lane emitters
            // lower; the operand layouts are still walked and checked.
            MirStatementKind::RuntimeCall { call, .. }
                if (self.in_lane_form_function || self.in_single_lane_function)
                    && self.is_scalar_extremum(call) =>
            {
                crate::visit::walk_stmt(self, stmt);
            }
            // EXEC02-PC1: the wildcard runtime-call reject names the
            // offending collection op — the first CUDA dense run died
            // unnamed; a diagnostic must not be vaguer than the fact.
            MirStatementKind::RuntimeCall { call, .. } => {
                reject_runtime_call(self, call);
            }
            MirStatementKind::Construct {
                aggregate,
                destination,
            } => {
                classify_construct_statement(self, stmt, aggregate, destination);
            }
        }
    }

    fn visit_terminator(&mut self, terminator: &MirTerminator) {
        if !self.in_device_function {
            return;
        }
        match &terminator.kind {
            MirTerminatorKind::Return(_) | MirTerminatorKind::ReturnThread => {
                crate::visit::walk_terminator(self, terminator);
            }
            // C4-2e: a structurizable lane-form kernel's acyclic Goto/Branch
            // edges are expressed by its region tree.
            //
            // A structurizable lane-form kernel has no reachable `Unreachable`
            // terminator (the structurizer refuses one), so any it carries
            // sits on a dead block the region tree never visits.
            MirTerminatorKind::Goto(_)
            | MirTerminatorKind::Branch { .. }
            | MirTerminatorKind::Unreachable
                if self.in_lane_form_function || self.in_single_lane_function =>
            {
                crate::visit::walk_terminator(self, terminator);
            }
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
            MirTerminatorKind::Switch { .. } | MirTerminatorKind::Unreachable
                if self.allows_structured_scalar_switch =>
            {
                crate::visit::walk_terminator(self, terminator);
            }
            MirTerminatorKind::Switch { .. } | MirTerminatorKind::Goto(_)
                if self.allows_structured_indexed_write_switch =>
            {
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
            MirTerminatorKind::ReturnError(_) if self.throw_slots.is_some() => {}
            MirTerminatorKind::ReturnError(_) => {
                self.record_abi(SHAPE_RETURN_ERROR_DEVICE_TERMINATOR);
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
            let is_literal_get_alias = match &value.kind {
                MirValueKind::Operand(operand) => {
                    MirSlot::from_operand(operand).is_some_and(|slot| {
                        self.literal_indexed_access
                            .as_ref()
                            .is_some_and(|plan| plan.is_get_slot(slot))
                    })
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
            if !is_result_aggregation && !is_literal_get_alias && !admitted_tensor_unary {
                self.check_device_type(value.ty);
            }
            self.check_narrow_arithmetic(value);
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

fn classify_assign_statement(
    classifier: &mut DeviceSafeClassifier<'_>,
    stmt: &MirStatement,
    place: &MirPlace,
    value: &MirValue,
) {
    // S5-W-A1: a dead fold-artifact store — a shape-list/vacua
    // value written into a dead fold-artifact slot (the fold's
    // temp→local forward, `Local(l) ← Temp(t)`, whose local
    // nothing reads). The emitters skip the dead store (S5-U2c),
    // so the gate walks only the destination place, not the
    // artifact value (whose Array/Tensor type would otherwise
    // record the aggregate diagnostic).
    if is_fold_artifact_type(classifier.types, value.ty)
        && classifier.dead_fold_artifact_slots.contains(&place.base)
    {
        classifier.visit_place(place);
    } else if place.projections.is_empty() && classifier.is_throw_slot(&place.base) {
        // U9-6: the constant text of a throw site; the device never
        // materializes it, the fault site names it.
        classifier.visit_place(place);
    } else if classifier.is_dead_non_kernel_scalar_const_copy(place, value) {
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
        classifier.visit_place(place);
    } else {
        crate::visit::walk_stmt(classifier, stmt);
    }
}

fn classify_construct_statement(
    classifier: &mut DeviceSafeClassifier<'_>,
    stmt: &MirStatement,
    aggregate: &MirAggregate,
    destination: &MirPlace,
) {
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
    let literal_index = classifier
        .literal_indexed_access
        .as_ref()
        .is_some_and(|plan| plan.is_index_slot(destination));
    if literal_index {
        classifier.visit_place(destination);
        return;
    }
    let dead_fold_artifact = classifier
        .dead_fold_artifact_slots
        .contains(&destination.base)
        && matches!(
            aggregate.kind,
            MirAggregateKind::Array | MirAggregateKind::Tensor
        );
    // C4-6b follow-up: a lane-form genus local construct is the scalarized
    // per-field initialization the lane emitters emit; the aggregate's type
    // is still classified through `visit_aggregate` -> `check_device_type`.
    let lane_form_struct =
        matches!(aggregate.kind, MirAggregateKind::Struct(_)) && classifier.in_lane_form_function;
    if !dead_fold_artifact
        && !lane_form_struct
        && !matches!(
            aggregate.kind,
            MirAggregateKind::Vector | MirAggregateKind::Matrix | MirAggregateKind::Tuple
        )
    {
        classifier.record_policy(SHAPE_KERNEL_AGGREGATE_CONSTRUCT);
    }
    if dead_fold_artifact {
        // The destination place is still walked (a write to a
        // plain slot); the aggregate — and its non-scalar type —
        // is not classified, matching the emitters' whole-
        // construct skip.
        classifier.visit_place(destination);
    } else {
        crate::visit::walk_stmt(classifier, stmt);
    }
}

fn classify_literal_indexed_access(
    classifier: &mut DeviceSafeClassifier<'_>,
    destination: &Option<MirPlace>,
    args: &[MirOperand],
) {
    if let Some(destination) = destination {
        classifier.visit_place(destination);
    }
    if let Some(receiver) = args.first() {
        classifier.visit_operand(receiver);
    }
    if args.len() == 3 {
        classifier.visit_operand(&args[2]);
    }
}

fn reject_runtime_call(classifier: &mut DeviceSafeClassifier<'_>, call: &MirRuntimeCall) {
    let op_name = runtime_call_op_name(&call.intrinsic);
    let shape = if op_name.is_empty() {
        SHAPE_KERNEL_RUNTIME_CALL.to_owned()
    } else {
        format!("{SHAPE_KERNEL_RUNTIME_CALL} {op_name}")
    };
    classifier.record_policy(shape);
}

fn classify_structured_allowances(
    classifier: &mut DeviceSafeClassifier<'_>,
    function: &MirFunction,
) {
    classifier.in_single_lane_function =
        is_structurizable_single_lane(function, classifier.validation);
    classifier.in_lane_form_function = is_structurizable_lane_form(function)
        || (classifier.reduction_terms.contains(&function.id)
            && crate::structurize::structurize_duplicating_tails(function).is_ok());
    classifier.genus_params = classifier.resolved_genus_params(function);
    classifier.record_view_params = classifier.resolved_record_view_params(function);
    classifier.narrow_locals = function
        .locals
        .iter()
        .filter(|local| classifier.is_narrow_ty(local.ty))
        .map(|local| local.id)
        .collect();
    classifier.narrow_temps = function
        .temps
        .iter()
        .filter(|temp| classifier.is_narrow_ty(temp.ty))
        .map(|temp| temp.id)
        .collect();
    let allows_structured_return_branch = is_structured_return_branch(function);
    let allows_structured_indexed_write_branch = is_structured_indexed_write_branch(function);
    let allows_structured_indexed_write_goto = is_structured_indexed_write_goto(function);
    let allows_structured_indexed_write_loop = is_structured_indexed_write_loop(function);
    let allows_structured_indexed_write_loop_break =
        is_structured_indexed_write_loop_break(function);
    let allows_structured_indexed_write_loop_continue =
        is_structured_indexed_write_loop_continue(function);
    let allows_structured_indexed_write_switch = is_structured_indexed_write_switch(function);
    let allows_structured_indexed_value_join_branch =
        is_structured_indexed_value_join_branch(function);
    let allows_structured_value_join_branch = is_structured_value_join_branch(function);
    let allows_structured_scalar_switch = is_structured_scalar_switch(function);
    let allows_structured_scalar_loop = is_structured_scalar_loop(function);
    let allows_structured_scalar_loop_break = is_structured_scalar_loop_break(function);
    let allows_structured_scalar_loop_continue = is_structured_scalar_loop_continue(function);
    let allows_structured_scalar_return_goto = is_structured_scalar_return_goto(function);
    let allows_structured_nested_scalar_loop_branch =
        is_structured_nested_scalar_loop_branch(function);
    let allows_structured_nested_indexed_write_loop_branch =
        is_structured_nested_indexed_write_loop_branch(function);
    let allows_structured_nested_indexed_write_loop_break =
        is_structured_nested_indexed_write_loop_break(function);
    let allows_structured_nested_indexed_write_loop_continue =
        is_structured_nested_indexed_write_loop_continue(function);
    classifier.allows_structured_return_branch = allows_structured_return_branch;
    classifier.allows_structured_indexed_write_branch = allows_structured_indexed_write_branch;
    classifier.allows_structured_indexed_write_goto = allows_structured_indexed_write_goto;
    classifier.allows_structured_indexed_write_loop = allows_structured_indexed_write_loop;
    classifier.allows_structured_indexed_write_loop_break =
        allows_structured_indexed_write_loop_break;
    classifier.allows_structured_indexed_write_loop_continue =
        allows_structured_indexed_write_loop_continue;
    classifier.allows_structured_indexed_write_switch = allows_structured_indexed_write_switch;
    classifier.allows_structured_indexed_value_join_branch =
        allows_structured_indexed_value_join_branch;
    classifier.allows_structured_value_join_branch = allows_structured_value_join_branch;
    classifier.allows_structured_scalar_switch = allows_structured_scalar_switch;
    classifier.allows_structured_scalar_loop = allows_structured_scalar_loop;
    classifier.allows_structured_scalar_loop_break = allows_structured_scalar_loop_break;
    classifier.allows_structured_scalar_loop_continue = allows_structured_scalar_loop_continue;
    classifier.allows_structured_scalar_return_goto = allows_structured_scalar_return_goto;
    classifier.allows_structured_nested_scalar_loop_branch =
        allows_structured_nested_scalar_loop_branch;
    classifier.allows_structured_nested_indexed_write_loop_branch =
        allows_structured_nested_indexed_write_loop_branch;
    classifier.allows_structured_nested_indexed_write_loop_break =
        allows_structured_nested_indexed_write_loop_break;
    classifier.allows_structured_nested_indexed_write_loop_continue =
        allows_structured_nested_indexed_write_loop_continue;
    if function.is_async || function.is_generator {
        classifier.record_policy(SHAPE_ASYNC_GENERATOR_DEVICE_FN);
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
        && !classifier.in_lane_form_function
        && !classifier.in_single_lane_function
    {
        classifier.record_policy(SHAPE_DEVICE_MULTI_BLOCK_CFG);
    }
}

fn classify_device_function_slots(
    classifier: &mut DeviceSafeClassifier<'_>,
    function: &MirFunction,
) {
    classifier.throw_slots = crate::abi::throw_sites(function, classifier.types).map(|sites| {
        sites
            .into_iter()
            .filter_map(|site| site.slot)
            .collect::<Vec<_>>()
    });
    if function.error_ty.is_some() && classifier.throw_slots.is_none() {
        classifier.record_abi(SHAPE_FAILABLE_DEVICE_FN);
    }
    // S3-A1: a result-tuple return is the multi-output kernel contract —
    // the tuple elements are DISTINCT device output resources (the ABI
    // lowers them via `multi_output_return_kernel`). Classify each
    // element instead of the aggregate tuple, which would otherwise
    // record a "device aggregate value type" gap on a valid contract.
    match classifier.types.get(function.return_ty.semantic_id()) {
        Type::Tuple(elements) if !elements.is_empty() => {
            for element in elements {
                classifier.check_device_type(MirType::semantic(*element));
            }
        }
        // A genus RETURN is never a scalarizable local: the lane emitters
        // scalarize locals, not returns, so it keeps the aggregate refusal
        // even in lane form.
        Type::Struct(_) => classifier.record_policy(format!(
            "device aggregate value type {:?}",
            classifier.types.get(function.return_ty.semantic_id())
        )),
        _ => classifier.check_device_type(function.return_ty),
    }
    if let Some(error_ty) = function.error_ty
        && classifier.throw_slots.is_none()
    {
        classifier.check_device_type(error_ty);
    }
    // S5-W-A1: the dead fold-artifact slots (shape-list / vacua
    // `Array`/`Tensor` slots nothing reads) are precomputed so the
    // local, temp, and construct checks admit them exactly as the
    // emitters' S5-U2c skip does.
    classifier.dead_fold_artifact_slots = dead_fold_artifact_slots(function, classifier.types);
    // a0184031: the dead non-kernel-scalar const-copy slots (i64
    // axis-const locals/temps nothing reads) are precomputed so the
    // local/temp declarations and the const-copy statement checks
    // admit them exactly as the emitters' dead-skip does (Metal
    // 26424928f Gap 1; NVVM emits the harmless native-i64 store).
    classifier.dead_const_copy_slots =
        dead_non_kernel_scalar_const_slots(function, classifier.layouts);
    classifier.literal_indexed_access =
        LiteralIndexedAccessPlan::for_function(function, classifier.types);
    // S5A-U4: the output-set tuple aggregation slots of this function
    // (its own result-tuple return aggregation plus a decomposed
    // multi-output source's kept result-tuple local) — the ONLY
    // tuple-typed slots the gate admits. Computed per function from
    // the decomposition's explicit fact; no source-wide `DefId`
    // allowance.
    classifier.result_aggregation_slots =
        crate::kernel_decomposition::multi_output_result_aggregation_slots(
            function,
            classifier.types,
        );
}

/// Packed vector widths a device view admits: the widths whose size equals
/// their alignment (2 and 4). Width 3 stays fail-closed (operator ruling #3).
fn is_device_view_vector(width: u8) -> bool {
    matches!(width, 2 | 4)
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
