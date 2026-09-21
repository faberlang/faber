use crate::{
    MirCollectionOp, MirFunction, MirKernelAxis, MirKernelBuiltin, MirLayoutKind, MirLocalId,
    MirScalarLayout, MirType, MirValidationContext, shader_contract::ShaderStageIoRole,
};
use radix_types::{IndexExpr, IndexId, Primitive, Type};
use serde::{Deserialize, Serialize};

use super::{
    ERR_INDEXED_KERNEL_DEVICE_VIEW, ERR_KERNEL_AGGREGATE_LAYOUT, ERR_KERNEL_BUILTIN_SCALAR_INPUT,
    ERR_KERNEL_DEVICE_VIEW_ELEMENT, ERR_KERNEL_DEVICE_VIEW_ELEMENT_COUNT, ERR_KERNEL_INDEX_U32,
    ERR_KERNEL_IO_ELEMENT_MISMATCH, ERR_KERNEL_MATRIX_PARAM, ERR_KERNEL_NEVER_SCALAR,
    ERR_KERNEL_OPAQUE_LAYOUT, ERR_KERNEL_REFLECTION_BYTE_OVERFLOW,
    ERR_KERNEL_REFLECTION_SCALAR_ELEMENT, ERR_KERNEL_SPARSE_STORAGE,
    ERR_KERNEL_TENSOR_STORAGE_LOCUS, ERR_KERNEL_VECTOR_ELEMENT_COUNT, ERR_KERNEL_VECTOR_PARAM,
    ERR_KERNEL_VOID_SCALAR, ERR_KERNEL_WORKGROUP_NONZERO, MirAbiError,
    kernel_atomic_element_count_error, kernel_atomic_param_error, kernel_atomic_reflection_error,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirKernelResourceReflection {
    pub group: u32,
    pub binding: u32,
    pub kind: MirKernelResourceKind,
    pub role: MirKernelResourceRole,
    pub access: MirKernelResourceAccess,
    pub element_ty: MirType,
    pub element_layout: MirScalarLayout,
    pub element_byte_width: u16,
    pub element_count: u64,
    /// The declaring kernel parameter's concrete shape vector
    /// (declaration-order literal dims), stamped by the producers that can
    /// resolve the parameter; `None` for rows with no declared tensor
    /// parameter. Carried beside `element_count` because the flattened
    /// count cannot distinguish `[4,4]` from `[16]`.
    pub shape: Option<Vec<u64>>,
    pub buffer_byte_len: u64,
    pub source_local: Option<MirLocalId>,
    pub source_name: Option<String>,
    /// Explicit stage-I/O role (see [`crate::shader_contract::ShaderStageIoRole`]).
    /// Carried so transform detection and other role-driven emission can key
    /// on the declared contract role instead of source-name spelling.
    pub stage_io_role: ShaderStageIoRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirKernelResource {
    pub group: u32,
    pub binding: u32,
    pub kind: MirKernelResourceKind,
    pub role: MirKernelResourceRole,
    pub access: MirKernelResourceAccess,
    pub element_ty: MirType,
    pub element_count: u64,
    pub source_local: Option<MirLocalId>,
    pub source_name: Option<String>,
    /// The producer-defined reduced-buffer projection (council-8 CB-3): the
    /// axis-reduction fact that maps a keep-dims flat element into this
    /// resource's REDUCED buffer. `Some` only on a resource that binds a
    /// reduced-count buffer produced by an `AxisReduction` recipe — carried
    /// here (and on the wire [`crate::device_program::BufferVersion`]) so
    /// consumers never reconstruct the projection from element count (which
    /// is ambiguous for equal dims — a `[4,4]` axis-0 and axis-1 reduction
    /// both produce 4 elements but map differently). `None` for every other
    /// resource.
    pub reduced_projection: Option<ReducedProjection>,
}

/// The producer-defined linear projection of a reduced buffer
/// (council-8 CB-3): `axis_extent` is the reduced-axis dimension
/// (`dims[axis]`, the per-row reduction width) and `inner_stride` is the
/// product of the dims after the axis (`1` when the axis is last). These two
/// facts distinguish every reduction mapping — for `[4,4]`, axis 0 carries
/// `{axis_extent: 4, inner_stride: 4}` (column reduce) and axis 1 carries
/// `{axis_extent: 4, inner_stride: 1}` (row reduce), even though both
/// reduced buffers hold 4 elements. A consumer reads a keep-dims flat
/// element `o` at the reduced-buffer flat index
/// `(o / (axis_extent · inner_stride)) · inner_stride + (o % inner_stride)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReducedProjection {
    /// The reduced-axis extent (`dims[axis]`).
    pub axis_extent: u64,
    /// The product of the dims after the axis (`1` when the axis is last).
    pub inner_stride: u64,
}

impl ReducedProjection {
    /// The reduced-buffer flat index for a keep-dims element at flat index
    /// `flat` (council-8 CB-3): the general prefix/suffix flatten. Axis-last
    /// (`inner_stride == 1`) simplifies to `flat / axis_extent`.
    #[must_use]
    pub fn reduced_index(self, flat: u64) -> u64 {
        let width = self.axis_extent.saturating_mul(self.inner_stride);
        (flat / width).saturating_mul(self.inner_stride) + (flat % self.inner_stride)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirKernelBuiltinInput {
    pub local: MirLocalId,
    pub source_name: Option<String>,
    pub builtin: MirKernelBuiltin,
    pub ty: MirType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirKernelBuiltinInputReflection {
    pub source_local: MirLocalId,
    pub source_name: Option<String>,
    pub builtin: MirKernelBuiltin,
    pub axis: MirKernelAxis,
    pub ty: MirType,
    pub layout: MirScalarLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirKernelResourceKind {
    StorageBuffer,
    /// Runtime-extent count channel (U2, L3/L5).
    ///
    /// A host-supplied `u32` element count that device guards read instead of
    /// the static dispatch shape. The extent is an explicit
    /// [`MirKernelResource`] appended to `extra_inputs` — no parallel field
    /// (L3) — so its binding falls out of the positional allocator and
    /// `resources()` / `launch_plan()` / `emit_chain_descriptor` enumerate it
    /// for free (L4: chain-descriptor JSON shape unchanged).
    ///
    /// NVVM forward obligation (L7): future NVVM/PTX device guards must
    /// consume this channel (read the extent binding, with the shared
    /// `oob_guard_limit` static axis as fallback) — they must not reintroduce
    /// a static-shape guard.
    RuntimeExtent,
}

impl MirKernelResourceKind {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::StorageBuffer => "storage-buffer",
            Self::RuntimeExtent => "runtime-extent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirKernelResourceRole {
    Input,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirKernelResourceAccess {
    Read,
    Write,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirKernelShaderAccess {
    Read,
    ReadWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirKernelBufferBindingKind {
    ReadOnlyStorage,
    Storage,
}

impl MirKernelShaderAccess {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::ReadWrite => "read_write",
        }
    }
}

impl MirKernelBufferBindingKind {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::ReadOnlyStorage => "read-only-storage",
            Self::Storage => "storage",
        }
    }
}

pub(crate) const DEFAULT_KERNEL_RESOURCE_GROUP: u32 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirWorkgroupSize {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirKernelDispatchSize {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirKernelWorkgroupCount {
    pub x: u64,
    pub y: u64,
    pub z: u64,
}

/// Collection-op-aware workgroup count for a dispatch grid under the
/// effective workgroup (D-A1, delivery D-W6-A1).
///
/// The single shared semantics every reflection consumer applies after
/// [`MirKernelReflection::from_signature`] via
/// [`MirKernelReflection::apply_effective_workgroup`]:
/// - **matmul** (`TensorMatMul`, `TensorQuantizedMatMul`,
///   `TensorGroupedMatMul`): `dispatch_size` is the tile grid (one
///   workgroup per output tile) — the count is `dispatch_size` verbatim;
///   dividing by the tile under-launches (PRP-004). A packed QMM that
///   took the generic ceil path wrote only the first `tile` rows / a
///   `1/tile` strip of columns (vocab-1 all-zero last-row logits).
/// - **everything else** (reduction, elementwise, non-collection):
///   `ceil(dispatch_size / effective)` per dimension.
///
/// Semantics shared with the former target-local
/// `kernel_dispatch_workgroups` / `apply_effective_workgroup` pair.
#[must_use]
pub fn collection_op_workgroup_count(
    function: &MirFunction,
    dispatch_size: MirKernelDispatchSize,
    effective: MirWorkgroupSize,
) -> MirKernelWorkgroupCount {
    if kernel_has_collection_op(function, |op| {
        matches!(
            op,
            MirCollectionOp::TensorMatMul
                | MirCollectionOp::TensorQuantizedMatMul
                | MirCollectionOp::TensorGroupedMatMul
        )
    }) {
        return MirKernelWorkgroupCount {
            x: dispatch_size.x,
            y: dispatch_size.y,
            z: dispatch_size.z,
        };
    }
    MirKernelWorkgroupCount {
        x: effective_ceil_dimension(dispatch_size.x, effective.x),
        y: effective_ceil_dimension(dispatch_size.y, effective.y),
        z: effective_ceil_dimension(dispatch_size.z, effective.z),
    }
}

/// True when the kernel body contains a collection op matching `predicate`.
///
/// Single statement scan shared by [`collection_op_contract`],
/// [`collection_op_workgroup_count`], and the target emitters, so the
/// collection-op semantic classification stays in one place.
pub(crate) fn kernel_has_collection_op(
    function: &MirFunction,
    mut predicate: impl FnMut(MirCollectionOp) -> bool,
) -> bool {
    for block in &function.blocks {
        for statement in &block.statements {
            let Some(op) = crate::collection_ops::statement_collection_op(statement) else {
                continue;
            };
            if predicate(op) {
                return true;
            }
        }
    }
    false
}

/// `ceil(dispatch / workgroup)` with a defensively nonzero workgroup lane
/// count (mirrors the target chain-descriptor guard; the effective workgroup
/// is nonzero for every real consumer).
fn effective_ceil_dimension(dispatch: u64, workgroup: u32) -> u64 {
    dispatch.div_ceil(u64::from(workgroup.max(1)))
}

pub(crate) fn ceil_div_dimension(dispatch: u64, workgroup: u32) -> Result<u64, MirAbiError> {
    if workgroup == 0 {
        return Err(MirAbiError::new(ERR_KERNEL_WORKGROUP_NONZERO));
    }
    Ok(effective_ceil_dimension(dispatch, workgroup))
}

pub(crate) fn reflect_kernel_resource(
    resource: &MirKernelResource,
    validation: &MirValidationContext<'_>,
) -> Result<MirKernelResourceReflection, MirAbiError> {
    let layout = validation
        .layouts
        .layout_for_type(resource.element_ty)
        .map_err(|error| MirAbiError::new(error.message))?;
    let element_layout = match layout.kind {
        MirLayoutKind::Scalar(element_layout) => element_layout,
        MirLayoutKind::Atomic(atomic) => {
            return Err(MirAbiError::new(kernel_atomic_reflection_error(
                atomic.element,
            )));
        }
        _ => return Err(MirAbiError::new(ERR_KERNEL_REFLECTION_SCALAR_ELEMENT)),
    };
    let element_byte_width = kernel_element_byte_width(element_layout)?;
    let buffer_byte_len = kernel_buffer_byte_len(element_byte_width, resource.element_count)?;
    Ok(MirKernelResourceReflection {
        group: resource.group,
        binding: resource.binding,
        kind: resource.kind,
        role: resource.role,
        access: resource.access,
        element_ty: resource.element_ty,
        element_layout,
        element_byte_width,
        element_count: resource.element_count,
        shape: None,
        buffer_byte_len,
        source_local: resource.source_local,
        source_name: resource.source_name.clone(),
        stage_io_role: ShaderStageIoRole::Other,
    })
}

pub(crate) fn kernel_buffer_byte_len(
    element_byte_width: u16,
    element_count: u64,
) -> Result<u64, MirAbiError> {
    u64::from(element_byte_width)
        .checked_mul(element_count)
        .ok_or_else(|| MirAbiError::new(ERR_KERNEL_REFLECTION_BYTE_OVERFLOW))
}

/// The concrete shape vector of one reflected resource's declaring kernel
/// parameter: the declaration-order literal dims of the parameter's tensor
/// type, resolved from the type table. `None` when the slot declares no
/// tensor parameter (a synthetic buffer, a scalar, the runtime-extent count
/// channel) — a flattened `element_count` cannot reconstruct rank or dims,
/// so nothing is inferred here (the wire-shape ruling: no source-text
/// parsing, no static shape table).
#[must_use]
pub fn kernel_param_shape(
    source_local: Option<MirLocalId>,
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Option<Vec<u64>> {
    let local = source_local?;
    let param = function.params.iter().find(|param| param.local == local)?;
    tensor_param_shape_dims(param.ty, validation)
}

fn tensor_param_shape_dims(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Option<Vec<u64>> {
    match validation.types.get(ty.semantic_id()) {
        Type::Tensor(_, shape) => tensor_shape_dims(*shape, validation),
        Type::Alias(_, target) => {
            tensor_param_shape_dims(MirType::semantic(*target), validation)
        }
        _ => None,
    }
}

fn tensor_shape_dims(shape: IndexId, validation: &MirValidationContext<'_>) -> Option<Vec<u64>> {
    match validation.types.get_index(shape) {
        IndexExpr::Literal(count) => Some(vec![*count]),
        IndexExpr::Tuple(dims) => dims
            .iter()
            .map(|dim| match validation.types.get_index(*dim) {
                IndexExpr::Literal(count) => Some(*count),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

pub(crate) fn reflect_kernel_builtin_input(
    input: &MirKernelBuiltinInput,
    validation: &MirValidationContext<'_>,
) -> Result<MirKernelBuiltinInputReflection, MirAbiError> {
    let layout = validation
        .layouts
        .layout_for_type(input.ty)
        .map_err(|error| MirAbiError::new(error.message))?;
    let MirLayoutKind::Scalar(scalar) = layout.kind else {
        return Err(MirAbiError::new(ERR_KERNEL_BUILTIN_SCALAR_INPUT));
    };
    Ok(MirKernelBuiltinInputReflection {
        source_local: input.local,
        source_name: input.source_name.clone(),
        builtin: input.builtin,
        axis: input.builtin.axis(),
        ty: input.ty,
        layout: scalar,
    })
}

pub(crate) fn require_same_element_type(
    input: MirType,
    output: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    if validation
        .types
        .equals(input.semantic_id(), output.semantic_id())
    {
        Ok(())
    } else {
        Err(MirAbiError::new(ERR_KERNEL_IO_ELEMENT_MISMATCH))
    }
}

pub(crate) fn is_vacuum(ty: MirType, validation: &MirValidationContext<'_>) -> bool {
    matches!(
        validation.types.get(ty.semantic_id()),
        Type::Primitive(Primitive::Vacuum)
    )
}

/// The tuple-element count of a kernel **result-tuple** return (S3-A1), or
/// `None` when the return type is not a non-empty tuple. The tuple elements
/// are the multi-output contract's output resources. `pub(crate)` so the
/// decomposition seam's D-2b-1 tuple-return tests can assert the ABI
/// recognizes the synthetic's tuple form.
pub(crate) fn result_tuple_count(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Option<usize> {
    match validation.types.get(function.return_ty.semantic_id()) {
        Type::Tuple(elements) if !elements.is_empty() => Some(elements.len()),
        _ => None,
    }
}

pub(crate) fn require_u32_index(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .map_err(|error| MirAbiError::new(error.message))?;
    if layout.kind == MirLayoutKind::Scalar(MirScalarLayout::U32) {
        Ok(())
    } else {
        Err(MirAbiError::new(ERR_KERNEL_INDEX_U32))
    }
}

pub(crate) fn buffer_element_type(
    ty: MirType,
    validation: &MirValidationContext<'_>,
    allow_host_owned_tensor_entry: bool,
) -> Result<MirType, MirAbiError> {
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .map_err(|error| MirAbiError::new(error.message))?;
    match layout.kind {
        MirLayoutKind::Scalar(scalar) if crate::tensor_type::is_kernel_element_layout(scalar) => {
            Ok(ty)
        }
        MirLayoutKind::Scalar(scalar) => Err(MirAbiError::with_detail(
            format!("kernel scalar layout {}", scalar.spelling()),
            "mir_kernel_scalar_layout_unsupported",
            scalar.spelling(),
        )),
        MirLayoutKind::DeviceView(view) => {
            require_kernel_device_view(view, allow_host_owned_tensor_entry)?;
            tensor_element_type(ty, validation)
                .ok_or_else(|| MirAbiError::new(ERR_KERNEL_DEVICE_VIEW_ELEMENT))
        }
        MirLayoutKind::DeviceVector(_) => Err(MirAbiError::new(ERR_KERNEL_VECTOR_PARAM)),
        MirLayoutKind::DeviceMatrix(_) => Err(MirAbiError::new(ERR_KERNEL_MATRIX_PARAM)),
        MirLayoutKind::Atomic(atomic) => {
            Err(MirAbiError::new(kernel_atomic_param_error(atomic.element)))
        }
        MirLayoutKind::PackedNumeric(packed) => {
            Err(MirAbiError::new(packed.unsupported_execution_issue()))
        }
        MirLayoutKind::Aggregate(_) => Err(MirAbiError::new(ERR_KERNEL_AGGREGATE_LAYOUT)),
        MirLayoutKind::OpaqueHandle => Err(MirAbiError::new(ERR_KERNEL_OPAQUE_LAYOUT)),
        MirLayoutKind::Void => Err(MirAbiError::new(ERR_KERNEL_VOID_SCALAR)),
        MirLayoutKind::Never => Err(MirAbiError::new(ERR_KERNEL_NEVER_SCALAR)),
    }
}

pub(crate) fn device_view_element_type(
    ty: MirType,
    validation: &MirValidationContext<'_>,
    allow_host_owned_tensor_entry: bool,
) -> Result<MirType, MirAbiError> {
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .map_err(|error| MirAbiError::new(error.message))?;
    match layout.kind {
        MirLayoutKind::DeviceView(view) => {
            require_kernel_device_view(view, allow_host_owned_tensor_entry)?;
            tensor_element_type(ty, validation)
                .ok_or_else(|| MirAbiError::new(ERR_KERNEL_DEVICE_VIEW_ELEMENT))
        }
        _ => Err(MirAbiError::new(ERR_INDEXED_KERNEL_DEVICE_VIEW)),
    }
}

pub(crate) fn require_kernel_device_view(
    view: crate::MirDeviceViewLayout,
    allow_host_owned_tensor_entry: bool,
) -> Result<(), MirAbiError> {
    // MLC-4: sparse storage is rejected by name, before the generic
    // storage/locus row — fail-closed with a stable issue.
    if view.storage == crate::MirTensorStorageLayout::Sparse {
        return Err(MirAbiError::with_detail(
            ERR_KERNEL_SPARSE_STORAGE,
            "mir_kernel_sparse_storage_unsupported",
            "sparsa",
        ));
    }
    let host_owned_entry = allow_host_owned_tensor_entry
        && view.storage == crate::MirTensorStorageLayout::HostOwned
        && view.locus == crate::MirTensorLocusLayout::Host;
    let device_entry = view.storage == crate::MirTensorStorageLayout::DeviceHandle
        && view.locus == crate::MirTensorLocusLayout::Device;
    if !host_owned_entry && !device_entry {
        return Err(MirAbiError::new(ERR_KERNEL_TENSOR_STORAGE_LOCUS));
    }
    if !view.row_major || !view.contiguous || view.offset_elements != 0 {
        return Err(MirAbiError::new("kernel device view layout"));
    }
    // GEA2-U4b: the frozen interleaved RoPE table is a contiguous rank-3
    // device view. U1 head-axis: the batched matmul route's rank-4 Q
    // operand (`[kv_heads, q_per_kv, query_seq, head_dim]` — the settled
    // GEA3 layout) extends the cap to 4; rank 5+ stays fail-closed.
    if view.rank < 1 || view.rank > 4 {
        return Err(MirAbiError::new(format!(
            "kernel device view rank {}",
            view.rank
        )));
    }
    if !crate::tensor_type::is_kernel_element_layout(view.element) {
        return Err(MirAbiError::with_detail(
            format!("kernel device view element {}", view.element.spelling()),
            "mir_kernel_device_view_element_unsupported",
            view.element.spelling(),
        ));
    }
    Ok(())
}

pub(crate) fn buffer_element_count(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<u64, MirAbiError> {
    let layout = validation
        .layouts
        .layout_for_type(ty)
        .map_err(|error| MirAbiError::new(error.message))?;
    match layout.kind {
        MirLayoutKind::Scalar(scalar) if crate::tensor_type::is_kernel_element_layout(scalar) => {
            Ok(1)
        }
        MirLayoutKind::Scalar(scalar) => Err(MirAbiError::new(format!(
            "kernel scalar layout {}",
            scalar.spelling()
        ))),
        MirLayoutKind::DeviceView(_) => device_view_element_count(ty, validation),
        MirLayoutKind::DeviceVector(_) => Err(MirAbiError::new(ERR_KERNEL_VECTOR_ELEMENT_COUNT)),
        MirLayoutKind::DeviceMatrix(_) => Err(MirAbiError::new(ERR_KERNEL_MATRIX_PARAM)),
        MirLayoutKind::Atomic(atomic) => Err(MirAbiError::new(kernel_atomic_element_count_error(
            atomic.element,
        ))),
        MirLayoutKind::PackedNumeric(packed) => {
            Err(MirAbiError::new(packed.unsupported_execution_issue()))
        }
        MirLayoutKind::Aggregate(_) => Err(MirAbiError::new(ERR_KERNEL_AGGREGATE_LAYOUT)),
        MirLayoutKind::OpaqueHandle => Err(MirAbiError::new(ERR_KERNEL_OPAQUE_LAYOUT)),
        MirLayoutKind::Void => Err(MirAbiError::new(ERR_KERNEL_VOID_SCALAR)),
        MirLayoutKind::Never => Err(MirAbiError::new(ERR_KERNEL_NEVER_SCALAR)),
    }
}

/// Fail closed with a single stable shape when any kernel param or return is a
/// register matrix (storage-buffer ABI does not map `DeviceMatrix` IO).
///
/// Unit B ratchet: shared by all storage-buffer and `@nucleum` emit lanes so
/// matrix kernel IO is never fail-open on a staging target.
/// # Errors
/// Returns an error when validation or type resolution fails. See [`MirAbiError`].
pub fn reject_kernel_matrix_abi(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<(), MirAbiError> {
    for param in &function.params {
        if layout_is_device_matrix(param.ty, validation) {
            return Err(MirAbiError::new(ERR_KERNEL_MATRIX_PARAM));
        }
    }
    if layout_is_device_matrix(function.return_ty, validation) {
        return Err(MirAbiError::new(ERR_KERNEL_MATRIX_PARAM));
    }
    Ok(())
}

pub(crate) fn layout_is_device_matrix(ty: MirType, validation: &MirValidationContext<'_>) -> bool {
    matches!(
        validation
            .layouts
            .layout_for_type(ty)
            .map(|layout| layout.kind),
        Ok(MirLayoutKind::DeviceMatrix(_))
    )
}

pub(crate) fn device_view_element_count(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<u64, MirAbiError> {
    match validation.types.get(ty.semantic_id()) {
        Type::Tensor(_, shape) => device_view_element_count_from_shape(*shape, validation),
        Type::Alias(_, target) => device_view_element_count(MirType::semantic(*target), validation),
        _ => Err(MirAbiError::new(ERR_KERNEL_DEVICE_VIEW_ELEMENT_COUNT)),
    }
}

pub(crate) fn device_view_element_count_from_shape(
    shape: IndexId,
    validation: &MirValidationContext<'_>,
) -> Result<u64, MirAbiError> {
    match validation.types.get_index(shape) {
        IndexExpr::Literal(count) => Ok(*count),
        IndexExpr::Tuple(dims) => {
            let mut product: u64 = 1;
            for &dim in dims {
                match validation.types.get_index(dim) {
                    IndexExpr::Literal(count) => product = product.saturating_mul(*count),
                    _ => {
                        return Err(MirAbiError::new(
                            "kernel device view element count must be literal dimensions",
                        ));
                    }
                }
            }
            Ok(product)
        }
        _ => Err(MirAbiError::new(
            "kernel device view element count must be literal dimensions",
        )),
    }
}

pub(crate) fn tensor_element_type(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Option<MirType> {
    match validation.types.get(ty.semantic_id()) {
        Type::Tensor(element, _) => Some(MirType::semantic(*element)),
        Type::Alias(_, target) => tensor_element_type(MirType::semantic(*target), validation),
        _ => None,
    }
}

pub(crate) fn kernel_element_byte_width(layout: MirScalarLayout) -> Result<u16, MirAbiError> {
    if !crate::tensor_type::is_kernel_element_layout(layout) {
        return Err(MirAbiError::new(format!(
            "kernel scalar layout {}",
            layout.spelling()
        )));
    }
    let size_bits = layout.size_bits();
    if !size_bits.is_multiple_of(8) {
        return Err(MirAbiError::new(format!(
            "kernel scalar layout {} is not byte-sized",
            layout.spelling()
        )));
    }
    Ok(size_bits / 8)
}
