use crate::MirScalarLayout;
pub(crate) const ERR_KERNEL_PARAM_RETURN_MISMATCH: &str =
    "kernel parameter and return types must match";
pub(crate) const ERR_INDEXED_VIEW_RETURNS_VACUUM: &str = "indexed view kernel must return vacuum";
pub(crate) const ERR_INDEXED_VIEW_REQUIRES_INPUT: &str =
    "indexed view kernel requires at least one input";
pub(crate) const ERR_KERNEL_BUILTIN_UNKNOWN_NAME: &str =
    "kernel builtin parameter must use a known builtin name";
pub(crate) const ERR_KERNEL_BUILTIN_DUPLICATE_NAME: &str =
    "kernel builtin parameter must not repeat a builtin name";
pub(crate) const ERR_KERNEL_WORKGROUP_NONZERO: &str = "kernel workgroup size must be nonzero";
pub(crate) const ERR_KERNEL_REFLECTION_SCALAR_ELEMENT: &str =
    "kernel reflection resource element must be scalar";
pub(crate) const ERR_KERNEL_BUILTIN_SCALAR_INPUT: &str = "kernel builtin input must be scalar";
// The return-buffer ABI is elementwise-only: every input and the return must
// agree on element type AND element count. The count arm is what a matmul
// author hits (input/return shapes differ), so the message names the equality
// law and points to the indexed-view ABI that carries the matmul contract.
pub(crate) const ERR_KERNEL_IO_ELEMENT_MISMATCH: &str = "kernel input and output resource element types and counts must match; the return-buffer ABI is elementwise-only — matmul and reduction kernels must use the indexed-view ABI";
pub(crate) const ERR_KERNEL_INDEX_U32: &str = "kernel index parameter must be u32";
pub(crate) const ERR_KERNEL_DEVICE_VIEW_ELEMENT: &str = "kernel device view element type";
pub(crate) const ERR_KERNEL_VECTOR_PARAM: &str = "kernel vector param";
/// Stable shape for rejected matrix kernel params/returns (Unit B ratchet).
pub(crate) const ERR_KERNEL_MATRIX_PARAM: &str = "kernel matrix param";
pub(crate) const ERR_KERNEL_AGGREGATE_LAYOUT: &str = "kernel aggregate layout";
pub(crate) const ERR_KERNEL_OPAQUE_LAYOUT: &str = "kernel opaque layout";
/// S3-A1: the result-tuple ABI requires a non-empty tuple return of device
/// views (the multi-output contract).
pub(crate) const ERR_KERNEL_RESULT_TUPLE_REQUIRED: &str =
    "result-tuple kernel requires a non-empty tuple return of device-view tensors";
/// S3-A1: the result-tuple ABI requires at least one input param.
pub(crate) const ERR_KERNEL_RESULT_TUPLE_REQUIRES_INPUT: &str =
    "result-tuple kernel requires at least one input parameter";
pub(crate) const ERR_KERNEL_TENSOR_STORAGE_LOCUS: &str =
    "kernel tensor storage/locus requires device handle";
/// MLC-4: fail-closed sparse admission row — layout facts exist, kernels do
/// not admit them.
pub(crate) const ERR_KERNEL_SPARSE_STORAGE: &str = "kernel sparse tensor storage is not admitted";
pub(crate) const ERR_KERNEL_VOID_SCALAR: &str = "void kernel scalar";
pub(crate) const ERR_KERNEL_NEVER_SCALAR: &str = "never kernel scalar";
pub(crate) const ERR_INDEXED_KERNEL_DEVICE_VIEW: &str =
    "indexed kernel resource must be device view";
pub(crate) const ERR_KERNEL_RESOURCE_BINDING_OVERFLOW: &str =
    "kernel resource binding index exceeds u32";
pub(crate) const ERR_KERNEL_TENSOR_EXTENT_OPERAND: &str =
    "kernel TensorExtent operand must be a tensor parameter";
pub(crate) const ERR_KERNEL_TENSOR_EXTENT_U32: &str =
    "kernel TensorExtent requires the u32 numeric type";
pub(crate) const ERR_KERNEL_VECTOR_ELEMENT_COUNT: &str = "kernel vector element count";
pub(crate) const ERR_KERNEL_DEVICE_VIEW_ELEMENT_COUNT: &str = "kernel device view element count";
pub(crate) const ERR_KERNEL_REFLECTION_BYTE_OVERFLOW: &str =
    "kernel reflection resource byte length overflows u64";
pub(crate) const ERR_VERTEX_INPUT_DUPLICATE_LOCATION: &str =
    "vertex input locations must be unique";
pub(crate) const ERR_VERTEX_INPUT_UNSUPPORTED_FORMAT_CODE: &str =
    "unsupported Triga vertex input format code";
pub(crate) const ERR_VARYING_DUPLICATE_LOCATION: &str = "varying locations must be unique";
pub(crate) const ERR_FRAGMENT_OUTPUT_DUPLICATE_LOCATION: &str =
    "fragment output locations must be unique";
pub(crate) const ERR_VARYING_FRAGMENT_WITHOUT_VERTEX: &str =
    "fragment varying has no matching vertex output";
pub(crate) const ERR_VARYING_STAGE_MISMATCH: &str =
    "vertex and fragment varyings disagree on format or interpolation";
pub(crate) const ERR_GRAPHICS_PIPELINE_REQUIRES_VERTEX: &str =
    "graphics pipeline reflection requires vertex-stage reflection";
pub(crate) const ERR_GRAPHICS_PIPELINE_REQUIRES_FRAGMENT: &str =
    "graphics pipeline reflection requires fragment-stage reflection";
pub(crate) const ERR_GRAPHICS_PIPELINE_REQUIRES_VERTEX_INPUTS: &str =
    "graphics pipeline reflection requires vertex inputs";
pub(crate) const ERR_GRAPHICS_PIPELINE_VARYING_MISMATCH: &str =
    "graphics pipeline reflection requires compatible vertex/fragment varyings";
pub(crate) const ERR_GRAPHICS_PIPELINE_REQUIRES_COLOR_TARGETS: &str =
    "graphics pipeline reflection requires at least one color target";

pub(crate) fn kernel_atomic_param_error(element: MirScalarLayout) -> String {
    format!(
        "kernel atomic parameters are not supported for {}",
        element.spelling()
    )
}

pub(crate) fn kernel_atomic_element_count_error(element: MirScalarLayout) -> String {
    format!(
        "kernel atomic element counts are not supported for {}",
        element.spelling()
    )
}

pub(crate) fn kernel_atomic_reflection_error(element: MirScalarLayout) -> String {
    format!(
        "kernel atomic reflection resources are not supported for {}",
        element.spelling()
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirAbiError {
    pub message: String,
    pub issue: &'static str,
    pub detail: Option<String>,
}

impl MirAbiError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        let message = message.into();
        let issue = abi_issue(&message);
        Self {
            message,
            issue,
            detail: None,
        }
    }

    pub(crate) fn with_detail(
        message: impl Into<String>,
        issue: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            message: message.into(),
            issue,
            detail: Some(detail.into()),
        }
    }
}

fn abi_issue(message: &str) -> &'static str {
    if message == ERR_KERNEL_BUILTIN_DUPLICATE_NAME {
        "mir_kernel_builtin_duplicate_name"
    } else if message == crate::tensor_type::ZERO_LITERAL_DIM_MESSAGE {
        "mir_kernel_tensor_zero_extent"
    } else if message == crate::tensor_type::DIM_PRODUCT_OVERFLOW_MESSAGE {
        "mir_kernel_tensor_element_count_overflow"
    } else if message == ERR_KERNEL_WORKGROUP_NONZERO {
        "mir_kernel_workgroup_nonzero"
    } else if message == ERR_KERNEL_TENSOR_STORAGE_LOCUS {
        "mir_kernel_tensor_storage_locus_unsupported"
    } else if message == ERR_KERNEL_SPARSE_STORAGE {
        "mir_kernel_sparse_storage_unsupported"
    } else if message == ERR_KERNEL_REFLECTION_BYTE_OVERFLOW {
        "mir_kernel_reflection_byte_length_overflow"
    } else if message == ERR_INDEXED_KERNEL_DEVICE_VIEW {
        "mir_indexed_kernel_resource_not_device_view"
    } else if message == ERR_KERNEL_TENSOR_EXTENT_OPERAND {
        "mir_kernel_tensor_extent_operand"
    } else if message == ERR_KERNEL_TENSOR_EXTENT_U32 {
        "mir_kernel_tensor_extent_u32_missing"
    } else if message == ERR_KERNEL_RESOURCE_BINDING_OVERFLOW {
        "mir_kernel_resource_binding_overflow"
    } else if message == ERR_KERNEL_RESULT_TUPLE_REQUIRED {
        "mir_kernel_result_tuple_required"
    } else if message == ERR_KERNEL_RESULT_TUPLE_REQUIRES_INPUT {
        "mir_kernel_result_tuple_requires_input"
    } else if message.starts_with("kernel atomic parameters are not supported for ") {
        "mir_kernel_atomic_parameters_unsupported"
    } else if message.starts_with("kernel atomic reflection resources are not supported for ") {
        "mir_kernel_atomic_reflection_resources_unsupported"
    } else if message == ERR_KERNEL_MATRIX_PARAM {
        "mir_kernel_matrix_param_unsupported"
    } else if message == ERR_VERTEX_INPUT_DUPLICATE_LOCATION {
        "mir_vertex_input_duplicate_location"
    } else if message == ERR_VERTEX_INPUT_UNSUPPORTED_FORMAT_CODE {
        "mir_vertex_input_unsupported_format_code"
    } else if message == ERR_VARYING_DUPLICATE_LOCATION {
        "mir_varying_duplicate_location"
    } else if message == ERR_FRAGMENT_OUTPUT_DUPLICATE_LOCATION {
        "mir_fragment_output_duplicate_location"
    } else if message == ERR_VARYING_FRAGMENT_WITHOUT_VERTEX {
        "mir_varying_fragment_without_vertex"
    } else if message == ERR_VARYING_STAGE_MISMATCH {
        "mir_varying_stage_mismatch"
    } else if message == ERR_GRAPHICS_PIPELINE_REQUIRES_VERTEX {
        "mir_graphics_pipeline_requires_vertex"
    } else if message == ERR_GRAPHICS_PIPELINE_REQUIRES_FRAGMENT {
        "mir_graphics_pipeline_requires_fragment"
    } else if message == ERR_GRAPHICS_PIPELINE_REQUIRES_VERTEX_INPUTS {
        "mir_graphics_pipeline_requires_vertex_inputs"
    } else if message == ERR_GRAPHICS_PIPELINE_REQUIRES_COLOR_TARGETS {
        "mir_graphics_pipeline_requires_color_targets"
    } else if message == ERR_GRAPHICS_PIPELINE_VARYING_MISMATCH {
        "mir_graphics_pipeline_varying_mismatch"
    } else if message == ERR_KERNEL_IO_ELEMENT_MISMATCH {
        "mir_kernel_io_element_mismatch"
    } else if message.starts_with("kernel storage-buffer count ") {
        "mir_kernel_storage_buffer_limit"
    } else if message.starts_with("kernel device view rank ") {
        "mir_kernel_device_view_rank_unsupported"
    } else if message.starts_with("transpose dispatch contract requires a rank-2 tensor operand") {
        "mir_transpose_contract_rank_unsupported"
    } else if message.starts_with("packed kernel layout must be an admitted GGML type id") {
        "mir_packed_layout_ggml_type_id_unsupported"
    } else if message.starts_with("tensor-tensor add broadcast pair ")
        && message.contains("neither same-shape nor a rank extension")
    {
        "mir_broadcast_pair_not_rank_extension"
    } else {
        "mir_abi_error"
    }
}
