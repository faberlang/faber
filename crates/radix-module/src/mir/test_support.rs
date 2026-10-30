//! Shared compile helpers for MIR target-text probe tests.
//!
//! Probe tests call these helpers to compile Faber source through the driver and
//! extract emitted text or structured diagnostic facts. Target-specific binding
//! assertions stay in each `*_text_test.rs` module.

#[cfg(feature = "mir-metal")]
use crate::MetalTextOutput;
#[cfg(feature = "mir-wgsl")]
use crate::WgslTextOutput;
use crate::codegen::Target;
use crate::diagnostics::Diagnostic;
use crate::driver::ModuleCompileRequest;
use crate::driver::compile;
use crate::test_support::test_session_la;
use crate::{CompileResult, Output};

/// Format one diagnostic with code, issue, and named args (MIR probe error strings).
#[must_use]
pub fn diagnostic_fact(diagnostic: &Diagnostic) -> String {
    let args = diagnostic
        .args
        .iter()
        .map(|arg| format!("{}={}", arg.name, arg.value))
        .collect::<Vec<_>>()
        .join(",");
    format!("{:?}:{:?}:{args}", diagnostic.code, diagnostic.issue())
}

/// Join compile failure diagnostics for `Result<String, String>` error paths.
pub fn diagnostic_facts_message(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(diagnostic_fact)
        .collect::<Vec<_>>()
        .join(" | ")
}

#[must_use]
pub fn compile_target_result(target: Target, name: &str, source: &str) -> CompileResult {
    compile(&test_session_la(target), name, source, ModuleCompileRequest::default())
}

/// Compile source to the given target and return the emitted text.
///
/// # Errors
///
/// Returns joined diagnostic facts as a string when compilation fails.
pub fn compile_target_text(target: Target, name: &str, source: &str) -> Result<String, String> {
    let result = compile_target_result(target, name, source);
    match target_code(target, result.output) {
        Some(code) => Ok(code),
        None => Err(diagnostic_facts_message(&result.diagnostics)),
    }
}

/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
#[must_use]
pub fn compile_target_text_or_panic(target: Target, name: &str, source: &str) -> String {
    compile_target_text(target, name, source)
        .unwrap_or_else(|message| panic!("expected {target:?} text output, got diagnostics: {message}"))
}

/// # Panics
///
/// Panics when an assertion or unwrap path fails (test/helper utilities).
#[must_use]
pub fn compile_target_text_expect_failure(target: Target, name: &str, source: &str) -> String {
    let result = compile_target_result(target, name, source);
    assert!(!result.success(), "expected compile failure for {name}");
    diagnostic_facts_message(&result.diagnostics)
}

/// Compile source to WGSL text output.
///
/// # Errors
///
/// Returns joined diagnostic facts as a string when compilation fails.
#[cfg(feature = "mir-wgsl")]
pub fn compile_wgsl_text_output(source: &str) -> Result<WgslTextOutput, String> {
    compile_target_typed_output(Target::MirWgsl, "wgsl.fab", source)
}

/// Compile source to WGSL text.
///
/// # Errors
///
/// Returns joined diagnostic facts as a string when compilation fails.
#[cfg(feature = "mir-wgsl")]
pub fn compile_wgsl_text(source: &str) -> Result<String, String> {
    compile_wgsl_text_output(source).map(|output| output.code)
}

/// Compile source to Metal text output.
///
/// # Errors
///
/// Returns joined diagnostic facts as a string when compilation fails.
#[cfg(feature = "mir-metal")]
pub fn compile_metal_text_output(source: &str) -> Result<MetalTextOutput, String> {
    compile_target_typed_output(Target::MirMetal, "metal.fab", source)
}

/// Compile Metal text with the kernel safe switch enabled.
///
/// # Errors
///
/// Returns diagnostic facts when compilation does not produce Metal text output.
#[cfg(feature = "mir-metal")]
pub fn compile_metal_text_output_with_safe_kernels(source: &str) -> Result<MetalTextOutput, String> {
    let session = crate::driver::Session::new(
        crate::driver::Config::default()
            .with_target(Target::MirMetal)
            .with_dev_locale_root()
            .with_latin_locale()
            .with_safe_kernels(),
    );
    let result = compile(&session, "metal.fab", source, ModuleCompileRequest::default());
    match MetalTextOutput::from_output(result.output) {
        Some(output) => Ok(output),
        None => Err(diagnostic_facts_message(&result.diagnostics)),
    }
}

/// Compile source to Metal text.
///
/// # Errors
///
/// Returns joined diagnostic facts as a string when compilation fails.
#[cfg(feature = "mir-metal")]
pub fn compile_metal_text(source: &str) -> Result<String, String> {
    compile_metal_text_output(source).map(|output| output.code)
}

#[cfg(feature = "mir-llvm")]
#[must_use]
pub fn compile_llvm_text(source: &str) -> String {
    compile_target_text_or_panic(Target::MirLlvm, "llvm.fab", source)
}

#[cfg(feature = "mir-wasm")]
#[must_use]
pub fn compile_wasm_text(source: &str) -> String {
    compile_target_text_or_panic(Target::MirWasm, "wasm.fab", source)
}

#[cfg(feature = "mir-wasm")]
#[must_use]
pub fn compile_wasm_text_error(source: &str) -> String {
    compile_target_text_expect_failure(Target::MirWasm, "wasm.fab", source)
}

#[cfg(any(feature = "mir-wgsl", feature = "mir-metal"))]
fn compile_target_typed_output<T>(target: Target, name: &str, source: &str) -> Result<T, String>
where
    T: TargetTextOutput,
{
    let result = compile_target_result(target, name, source);
    match T::from_output(result.output) {
        Some(output) => Ok(output),
        None => Err(diagnostic_facts_message(&result.diagnostics)),
    }
}

fn target_code(target: Target, output: Option<Output>) -> Option<String> {
    match (target, output?) {
        #[cfg(feature = "mir-wgsl")]
        (Target::MirWgsl, Output::WgslText(output)) => Some(output.code),
        #[cfg(feature = "mir-metal")]
        (Target::MirMetal, Output::MetalText(output)) => Some(output.code),
        #[cfg(feature = "mir-wasm")]
        (Target::MirWasm, Output::WasmText(output)) => Some(output.code),
        #[cfg(feature = "mir-llvm")]
        (Target::MirLlvm, Output::LlvmText(output)) => Some(output.code),
        _ => None,
    }
}

#[cfg(any(feature = "mir-wgsl", feature = "mir-metal"))]
trait TargetTextOutput: Sized {
    fn from_output(output: Option<Output>) -> Option<Self>;
}

#[cfg(feature = "mir-wgsl")]
impl TargetTextOutput for WgslTextOutput {
    fn from_output(output: Option<Output>) -> Option<Self> {
        match output? {
            Output::WgslText(output) => Some(output),
            _ => None,
        }
    }
}

#[cfg(feature = "mir-metal")]
impl TargetTextOutput for MetalTextOutput {
    fn from_output(output: Option<Output>) -> Option<Self> {
        match output? {
            Output::MetalText(output) => Some(output),
            _ => None,
        }
    }
}

// ============================================================================
// Shared ABI type helpers (used by abi_test, device_test, etc.)
// ============================================================================

use crate::hir::DefId;
use crate::mir::{
    MirBlock, MirBlockId, MirDeviceViewLayout, MirFunction, MirFunctionId, MirIntrinsic, MirKernelBuiltin,
    MirKernelDispatchSize, MirKernelLaunchPlan, MirKernelReflection, MirKernelResourceReflection, MirKernelShaderStage,
    MirKernelSignature, MirKernelWorkgroupCount, MirLayoutKind, MirLocalId, MirOperand, MirParam, MirParamMode,
    MirPlace, MirPlaceBase, MirProjection, MirRuntimeCall, MirScalarLayout, MirStatement, MirStatementKind,
    MirTerminator, MirTerminatorKind, MirType, MirValidationContext, MirWorkgroupSize,
};
use crate::semantic::{IndexExpr, NumericWidth, Primitive, TypeTable};
use crate::test_support::test_span;

// --- Scalar type constructors ---

#[must_use]
pub fn f32_type(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

#[must_use]
pub fn fractus_type(types: &TypeTable) -> MirType {
    MirType::semantic(types.primitive(Primitive::Fractus))
}

#[must_use]
pub fn u32_type(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32))
}

#[must_use]
pub fn atomic_i32_type(types: &mut TypeTable) -> MirType {
    let element = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    MirType::semantic(types.atomic(element))
}

#[must_use]
pub fn rank1_tensor_type(types: &mut TypeTable, primitive: Primitive, width: NumericWidth) -> MirType {
    let element = types.sized_numeric(primitive, width);
    let shape = types.intern_index(IndexExpr::Literal(4));
    MirType::semantic(types.tensor_with_shape(element, shape))
}

/// Returns `(tensor_type, element_type)` for rank-1 f32 tensor.
#[must_use]
pub fn rank1_tf32_type(types: &mut TypeTable) -> (MirType, MirType) {
    let element = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape = types.intern_index(IndexExpr::Literal(4));
    let tensor = types.tensor_with_shape(element, shape);
    (MirType::semantic(tensor), MirType::semantic(element))
}

// --- Place helpers ---

/// Build the `local[index]` indexed place used by MIR shape tests.
#[must_use]
pub fn indexed_local_place(local: MirLocalId, index: MirLocalId) -> MirPlace {
    MirPlace {
        base: MirPlaceBase::Local(local),
        projections: vec![MirProjection::Index(MirOperand::Place(MirPlace::local(index)))],
    }
}

// --- Validation context builder ---

/// Build a `MirValidationContext` with device-tensor layouts for every type.
#[must_use]
pub fn validation_with_device_tensor_layouts(types: &TypeTable) -> MirValidationContext<'_> {
    let mut validation = MirValidationContext::new(types);
    for index in 0..types.type_count() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        let ty = crate::semantic::TypeId(index as u32);
        let Ok(layout) = validation.layouts.layout_for_type(MirType::semantic(ty)) else {
            continue;
        };
        let MirLayoutKind::DeviceView(view) = layout.kind else {
            continue;
        };
        let mut handle = MirDeviceViewLayout::device_handle_row_major(MirScalarLayout::F32, view.rank);
        handle.element = view.element;
        validation
            .layouts
            .set_type_layout(ty, MirLayoutKind::DeviceView(handle));
    }
    validation
}

// --- Kernel function factories ---

#[must_use]
pub fn indexed_kernel_function(tensor_ty: MirType, index_ty: MirType, return_ty: MirType) -> MirFunction {
    MirFunction {
        id: MirFunctionId(8),
        source: Some(DefId(10)),
        name: None,
        params: vec![
            MirParam { local: MirLocalId(0), name: None, ty: tensor_ty, mode: MirParamMode::Owned, span: test_span() },
            MirParam { local: MirLocalId(1), name: None, ty: tensor_ty, mode: MirParamMode::Owned, span: test_span() },
            MirParam { local: MirLocalId(2), name: None, ty: index_ty, mode: MirParamMode::Owned, span: test_span() },
        ],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
            span: test_span(),
        }],
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

#[must_use]
pub fn multi_input_indexed_kernel_function(tensor_ty: MirType, index_ty: MirType, return_ty: MirType) -> MirFunction {
    MirFunction {
        id: MirFunctionId(9),
        source: Some(DefId(11)),
        name: None,
        params: vec![
            MirParam { local: MirLocalId(0), name: None, ty: tensor_ty, mode: MirParamMode::Owned, span: test_span() },
            MirParam { local: MirLocalId(1), name: None, ty: tensor_ty, mode: MirParamMode::Owned, span: test_span() },
            MirParam { local: MirLocalId(2), name: None, ty: tensor_ty, mode: MirParamMode::Owned, span: test_span() },
            MirParam { local: MirLocalId(3), name: None, ty: index_ty, mode: MirParamMode::Owned, span: test_span() },
        ],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: test_span() },
            span: test_span(),
        }],
        return_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

#[must_use]
pub fn kernel_function(ty: MirType) -> MirFunction {
    MirFunction {
        id: MirFunctionId(7),
        source: Some(DefId(9)),
        name: None,
        params: vec![MirParam { local: MirLocalId(3), name: None, ty, mode: MirParamMode::Owned, span: test_span() }],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(3))))),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: test_span(),
    }
}

// --- Statement helpers ---

#[must_use]
pub fn gpu_builtin_statement(builtin: MirKernelBuiltin, return_ty: MirType) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: None,
            call: MirRuntimeCall { intrinsic: MirIntrinsic::GpuBuiltin(builtin), args: Vec::new(), return_ty },
        },
        span: test_span(),
    }
}

// --- Setup helpers ---

/// Setup a simple f32 scalar kernel with default (1,1,1) dimensions.
///
/// # Panics
///
/// Panics when the default kernel function does not form a storage-buffer
/// kernel signature or does not yield kernel reflection (test/helper
/// utilities).
#[must_use]
pub fn setup_simple_kernel_reflection_launch_plan() -> (TypeTable, MirType, MirKernelReflection, MirKernelLaunchPlan) {
    let mut types = TypeTable::new();
    let ty = f32_type(&mut types);
    let validation = validation_with_device_tensor_layouts(&types);
    let function = kernel_function(ty);
    let signature = MirKernelSignature::storage_buffer_kernel(&function, &validation).expect("kernel signature");
    let reflection =
        MirKernelReflection::from_signature("add_one", &signature, &validation).expect("kernel reflection");
    let launch = reflection.launch_plan();
    (types, ty, reflection, launch)
}

/// Setup a sparse-group (groups 2 and 0) reflection with default (1,1,1) dimensions.
#[must_use]
pub fn setup_sparse_group_launch_plan() -> (TypeTable, MirKernelLaunchPlan) {
    let mut types = TypeTable::new();
    let ty = f32_type(&mut types);
    let reflection = MirKernelReflection {
        fault_sites: Vec::new(),
        lane_range_binding: None,
        slab_origin_bindings: Vec::new(),
        function: MirFunctionId(12),
        provenance: crate::mir::MirKernelProvenance::MirFunction(MirFunctionId(12)),
        entry_name: "sparse_groups".to_owned(),
        shader_stage: MirKernelShaderStage::Compute,
        varyings: Vec::new(),
        fragment_outputs: Vec::new(),
        resources: sparse_group_resources(ty),
        bound_resources: Vec::new(),
        vertex_inputs: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize { x: 1, y: 1, z: 1 },
        workgroup_count: MirKernelWorkgroupCount { x: 1, y: 1, z: 1 },
    };
    let launch = reflection.launch_plan();
    (types, launch)
}

/// The two sparse-group resource rows (input group 2, output group 0) shared by
/// the launch-plan setup helpers.
fn sparse_group_resources(element_ty: MirType) -> Vec<MirKernelResourceReflection> {
    vec![
        MirKernelResourceReflection {
            group: 2,
            binding: 0,
            kind: crate::mir::MirKernelResourceKind::StorageBuffer,
            role: crate::mir::MirKernelResourceRole::Input,
            access: crate::mir::MirKernelResourceAccess::Read,
            element_ty,
            element_layout: MirScalarLayout::F32,
            element_byte_width: 4,
            element_count: 1,
            shape: None,
            buffer_byte_len: 4,
            source_local: None,
            source_name: None,
            stage_io_role: crate::mir::ShaderStageIoRole::Other,
        },
        MirKernelResourceReflection {
            group: 0,
            binding: 0,
            kind: crate::mir::MirKernelResourceKind::StorageBuffer,
            role: crate::mir::MirKernelResourceRole::Output,
            access: crate::mir::MirKernelResourceAccess::Write,
            element_ty,
            element_layout: MirScalarLayout::F32,
            element_byte_width: 4,
            element_count: 1,
            shape: None,
            buffer_byte_len: 4,
            source_local: None,
            source_name: None,
            stage_io_role: crate::mir::ShaderStageIoRole::Other,
        },
    ]
}

// --- WebGPU probe setup (non-unit dispatch) ---

/// Setup a sparse reflection with dispatch (3,2,1) for WebGPU adapter tests.
#[must_use]
pub fn setup_webgpu_probe_reflection() -> (TypeTable, MirType, MirKernelReflection) {
    let mut types = TypeTable::new();
    let ty = f32_type(&mut types);
    let reflection = MirKernelReflection {
        fault_sites: Vec::new(),
        lane_range_binding: None,
        slab_origin_bindings: Vec::new(),
        function: MirFunctionId(12),
        provenance: crate::mir::MirKernelProvenance::MirFunction(MirFunctionId(12)),
        entry_name: "sparse_groups".to_owned(),
        shader_stage: MirKernelShaderStage::Compute,
        varyings: Vec::new(),
        fragment_outputs: Vec::new(),
        resources: sparse_group_resources(ty),
        bound_resources: Vec::new(),
        vertex_inputs: Vec::new(),
        builtin_inputs: Vec::new(),
        called_builtins: Vec::new(),
        workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
        dispatch_size: MirKernelDispatchSize { x: 3, y: 2, z: 1 },
        workgroup_count: MirKernelWorkgroupCount { x: 3, y: 2, z: 1 },
    };
    (types, ty, reflection)
}
