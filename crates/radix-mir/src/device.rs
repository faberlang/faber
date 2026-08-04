//! Shared MIR device roles and device-safe lowerability gate.
//!
//! TARGET: device-facing MIR consumers (`metal-text`, future `wgsl-text`,
//! NVVM/PTX, and LLVM's device-entry proof comments).
//!
//! WHY: device roles were first extracted inside `llvm_text`, while `metal_text`
//! carried a duplicate role context and local rejection policy. MIR GPU Stage 1
//! hoists the role context and adds one conservative pre-emission gate so
//! device emitters reject host-only MIR shapes before target-specific printing.

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
use crate::visit::MirVisitor;
use crate::{
    MirAggregate, MirAggregateKind, MirCollectionOp, MirFunction, MirFunctionId, MirIntrinsic,
    MirLayoutKind, MirLayoutTable, MirLocal, MirParam, MirPlace, MirProgram, MirProjection,
    MirRuntimeCall, MirScalarLayout, MirStatement, MirStatementKind, MirTemp, MirTensorLocusLayout,
    MirTensorStorageLayout, MirTerminator, MirTerminatorKind, MirType, MirValidationContext,
    MirValue, ValidatedMir,
};
use radix_hir::{DefId, HirFunction, HirItemKind, HirProgram};
use radix_types::{NumericWidth, Primitive, Type, TypeId, TypeTable};
use rustc_hash::FxHashMap;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirDeviceRole {
    Kernel,
    Device,
}

#[derive(Debug, Clone, Default)]
pub struct MirDeviceContext {
    by_function_id: FxHashMap<MirFunctionId, MirDeviceRole>,
    by_def_id: FxHashMap<DefId, MirDeviceRole>,
}

impl MirDeviceContext {
    #[must_use]
    pub fn from_hir_roles(roles_by_def: FxHashMap<DefId, MirDeviceRole>) -> Self {
        Self {
            by_def_id: roles_by_def,
            ..Self::default()
        }
    }

    pub fn attach_program(&mut self, program: &MirProgram) {
        for function in &program.functions {
            if let Some(def_id) = function.source {
                if let Some(role) = self.by_def_id.get(&def_id).copied() {
                    self.by_function_id.insert(function.id, role);
                }
            }
        }
    }

    #[must_use]
    pub fn role_for_function(&self, function: &MirFunction) -> Option<MirDeviceRole> {
        self.by_function_id.get(&function.id).copied().or_else(|| {
            function
                .source
                .and_then(|def_id| self.by_def_id.get(&def_id).copied())
        })
    }

    #[must_use]
    pub fn has_roles(&self) -> bool {
        !self.by_function_id.is_empty() || !self.by_def_id.is_empty()
    }
}

#[must_use]
pub fn device_roles_from_hir(hir: &HirProgram) -> FxHashMap<DefId, MirDeviceRole> {
    let mut roles_by_def = FxHashMap::default();
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(func) => {
                if let Some(role) = device_role_for_hir_function(func) {
                    roles_by_def.insert(item.def_id, role);
                }
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    if let Some(role) = device_role_for_hir_function(&method.func) {
                        roles_by_def.insert(method.def_id, role);
                    }
                }
            }
            _ => {}
        }
    }
    roles_by_def
}

/// The owned companion-relation carrier (S3-A2): primal → companion device
/// materialization facts carried from the driver onto the lowered MIR so the
/// device-program constructor can lower generated companions into device
/// kernels. Replaces the discarded `AirCompanionRelation` — companion names
/// are diagnostic identity only; this typed map is the routing surface.
#[derive(Debug, Clone, Default)]
pub struct MirCompanionMap {
    by_primal: FxHashMap<DefId, MirCompanionEntry>,
}

impl MirCompanionMap {
    /// Record one primal → companion materialization fact.
    pub fn insert(&mut self, entry: MirCompanionEntry) {
        self.by_primal.insert(entry.primal, entry);
    }

    /// The companion fact for a primal function, if carried.
    #[must_use]
    pub fn companion_for(&self, primal: DefId) -> Option<&MirCompanionEntry> {
        self.by_primal.get(&primal)
    }

    /// Iterate every carried companion fact (deterministic by primal `DefId`).
    pub fn iter(&self) -> impl Iterator<Item = &MirCompanionEntry> {
        let mut entries: Vec<&MirCompanionEntry> = self.by_primal.values().collect();
        entries.sort_by_key(|entry| entry.primal.0);
        entries.into_iter()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_primal.is_empty()
    }
}

/// One primal → companion device-materialization fact (S3-A2).
#[derive(Debug, Clone)]
pub struct MirCompanionEntry {
    /// The differentiable primal function.
    pub primal: DefId,
    /// The generated companion function (its MIR function's `source`).
    pub companion: DefId,
    /// Derivative kind of the companion.
    pub derivative: MirCompanionDerivativeKind,
    /// The primal is an explicitly device-resident forward kernel (A5): the
    /// driver records this from the primal's HIR device role, and the
    /// materializer joins the companion to the same `DeviceProgram`.
    pub device_resident: bool,
}

/// The derivative kind a companion computes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirCompanionDerivativeKind {
    /// Reverse-mode vector-Jacobian product.
    ReverseModeVjp,
}

impl MirCompanionDerivativeKind {
    /// Stable diagnostic spelling.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::ReverseModeVjp => "reverse-mode-vjp",
        }
    }
}

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
    /// S3-A1: whether the function under visit returns a non-empty tuple —
    /// the multi-output kernel contract. Tuple values (the result
    /// aggregation) are then classified per element instead of as aggregate.
    multi_output_result: bool,
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
            multi_output_result: false,
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
                    self.multi_output_result = true;
                    for element in elements {
                        self.check_device_type(MirType::semantic(*element));
                    }
                }
                _ => self.check_device_type(function.return_ty),
            }
            if let Some(error_ty) = function.error_ty {
                self.check_device_type(error_ty);
            }
            crate::visit::walk_function(self, function);
        }

        self.multi_output_result = false;
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
            && !(self.multi_output_result && is_result_tuple_type(self.types, local.ty))
        {
            self.check_device_type(local.ty);
        }
    }

    fn visit_temp(&mut self, temp: &MirTemp) {
        if self.in_device_function {
            self.check_device_type(temp.ty);
        }
    }

    fn visit_stmt(&mut self, stmt: &MirStatement) {
        if !self.in_device_function {
            return;
        }
        match &stmt.kind {
            MirStatementKind::Assign { .. } => crate::visit::walk_stmt(self, stmt),
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
            MirStatementKind::RuntimeCall {
                call:
                    MirRuntimeCall {
                        intrinsic:
                            MirIntrinsic::Collection(
                                op @ (MirCollectionOp::TensorAdd
                                | MirCollectionOp::TensorSub
                                | MirCollectionOp::TensorMul
                                | MirCollectionOp::TensorDiv
                                | MirCollectionOp::TensorNeg
                                | MirCollectionOp::TensorFill
                                | MirCollectionOp::TensorRelu
                                | MirCollectionOp::TensorGelu),
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
                let _ = op;
                crate::visit::walk_stmt(self, stmt);
            }
            MirStatementKind::RuntimeCall { .. } => self.record_policy(SHAPE_KERNEL_RUNTIME_CALL),
            MirStatementKind::Construct { aggregate, .. } => {
                // S3-A1: a Tuple construct is the multi-output result
                // aggregation — the ABI lowers it to DISTINCT output
                // resources; the container is not a device value.
                if !matches!(
                    aggregate.kind,
                    MirAggregateKind::Vector | MirAggregateKind::Matrix | MirAggregateKind::Tuple
                ) {
                    self.record_policy(SHAPE_KERNEL_AGGREGATE_CONSTRUCT);
                }
                crate::visit::walk_stmt(self, stmt);
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
            if !(self.multi_output_result && is_result_tuple_type(self.types, value.ty)) {
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

/// Whether a type is a non-empty tuple (the S3-A1 result-tuple contract).
fn is_result_tuple_type(types: &TypeTable, ty: MirType) -> bool {
    matches!(
        types.get(ty.semantic_id()),
        Type::Tuple(elements) if !elements.is_empty()
    )
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

/// WGSL `matCxR` is f32/f16 only; Stage1 admits f32 register matrices.
fn is_device_matrix_element(element: MirScalarLayout) -> bool {
    matches!(element, MirScalarLayout::F32)
}

#[must_use]
pub fn device_role_for_hir_function(func: &HirFunction) -> Option<MirDeviceRole> {
    // Fail-closed annotation exclusivity (CTO a7c616ed): one function must
    // not simultaneously carry compute and graphics annotations.
    let conflicting = [
        func.is_kernel,
        func.is_device,
        func.is_vertex,
        func.is_fragment,
    ]
    .iter()
    .filter(|&&b| b)
    .count();
    if conflicting > 1 {
        return None;
    }
    if func.is_kernel {
        Some(MirDeviceRole::Kernel)
    } else if func.is_device {
        Some(MirDeviceRole::Device)
    } else {
        None
    }
}
