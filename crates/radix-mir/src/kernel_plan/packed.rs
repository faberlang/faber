//! Packed-layout admission and per-layout dequant resolution (R-PACK-02A).
//!
//! `QuantizedMatMul` / `QuantizedGather` names were frozen by R-PACK-01A.
//! This module is the fail-closed policy that maps a GGML type id onto a
//! typed plan: F32 falls back to the existing dense recipes; the packed
//! §5 layouts resolve a [`PackedDequantResolution`]; anything else is
//! unrepresentable.
//!
//! `QuantizedGatherPlan` is an admission record, not a
//! [`super::CollectionKernelPlan`] variant — adding a variant would force
//! a wire change (unit stop condition).

use super::build::matmul_barrier_schedule;
use super::plan::{
    CollectionKernelPlan, F32_ELEMENT_BYTE_WIDTH, GatherPlan, MATMUL_SHARED_A_BUFFER_NAME,
    MATMUL_SHARED_B_BUFFER_NAME, MATMUL_TILE, MatMulPlan, MatMulShapes, MatMulSharedMemory,
    OobPaddingPolicy, PlanDim, QuantizedMatMulPlan, SharedMemoryLayout,
};
use crate::abi::PackedStorageLayout;
use crate::{
    MirCollectionOp, MirConstant, MirFunction, MirOperand, MirRuntimeCall, MirStatementKind,
};
use std::fmt;

/// In-kernel dequant class pinned to the §5 layout table (block vs
/// superblock vs native convert vs E8M0 scaled group). Never inferred
/// from a filename.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackedDequantKind {
    /// Plain f32 passthrough — existing `TiledMatMul` / `Gather`.
    PassthroughF32,
    /// Native 2-byte IEEE-754 binary16; in-kernel f32 convert.
    NativeF16Convert,
    /// Native 2-byte bfloat16; in-kernel f32 convert.
    NativeBf16Convert,
    /// Block dequant (`Q8_0`, `Q5_0`): f16 scale + packed ints, 32 elements.
    Block,
    /// Superblock dequant (`Q4_K`, `Q5_K`, `Q6_K`): 256 elements.
    Superblock,
    /// MXFP4 scaled group: one E8M0 scale over 32 packed 4-bit values.
    ///
    /// Not a native per-element convert and not `Block` (that class is
    /// f16 scale + packed ints). llama.cpp `dequantize_row_mxfp4`
    /// (`ggml-quants.c:569-587`) multiplies `kvalues_mxfp4[nibble]` by
    /// `GGML_E8M0_TO_FP32_HALF(e)`.
    ScaledE8M0Group,
}

/// Typed dequant facts resolved from an admitted [`PackedStorageLayout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedDequantResolution {
    /// Closed layout this resolution was derived from.
    pub layout: PackedStorageLayout,
    /// Block/superblock/convert class from the §5 table.
    pub class: PackedDequantKind,
    /// Logical elements per stored block.
    pub block_elements: u32,
    /// Packed bytes per stored block.
    pub block_bytes: u32,
}

/// Why packed admission refused a layout or operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackedAdmissionError {
    /// GGML type id is outside the closed §5 set.
    UnrepresentableLayout { ggml_type_id: u32 },
    /// Layout operand is not a const GGML type id.
    NonConstLayout,
}

/// Packed embedding-table gather admission (R-PACK-02A).
///
/// Not a [`CollectionKernelPlan`] variant — the carried plan stays
/// [`CollectionKernelPlan::Gather`] so the accepted wire is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuantizedGatherPlan {
    /// Table rows `V`.
    pub table_rows: u64,
    /// Table columns `D`.
    pub table_cols: u64,
    /// Id-vector length `T`.
    pub id_count: u64,
    /// Stored table layout (never `F32` — that stays [`GatherPlan`]).
    pub layout: PackedStorageLayout,
    /// In-kernel dequant facts for `layout`.
    pub dequant: PackedDequantResolution,
}

/// Gather admission result: F32 stays the existing recipe; packed tables
/// resolve [`QuantizedGatherPlan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmittedGather {
    /// Dense f32 table — existing [`GatherPlan`].
    Dense(GatherPlan),
    /// Packed table — row gather + in-kernel dequant.
    Quantized(QuantizedGatherPlan),
}

impl PackedDequantResolution {
    /// Resolve the pinned dequant facts for an admitted layout.
    #[must_use]
    pub fn for_layout(layout: PackedStorageLayout) -> Self {
        let class = match layout {
            PackedStorageLayout::F32 => PackedDequantKind::PassthroughF32,
            PackedStorageLayout::F16 => PackedDequantKind::NativeF16Convert,
            PackedStorageLayout::Bf16 => PackedDequantKind::NativeBf16Convert,
            PackedStorageLayout::Q8_0 | PackedStorageLayout::Q5_0 => PackedDequantKind::Block,
            PackedStorageLayout::Q4_K | PackedStorageLayout::Q5_K | PackedStorageLayout::Q6_K => {
                PackedDequantKind::Superblock
            }
            PackedStorageLayout::MXFP4 => PackedDequantKind::ScaledE8M0Group,
        };
        Self {
            layout,
            class,
            block_elements: layout.block_elements(),
            block_bytes: layout.block_bytes(),
        }
    }

    /// Resolve from a GGML type id. Unknown ids fail closed.
    ///
    /// # Errors
    /// [`PackedAdmissionError::UnrepresentableLayout`] when `id` is not in
    /// the closed §5 set.
    pub fn from_ggml_type_id(id: u32) -> Result<Self, PackedAdmissionError> {
        let layout = packed_layout_from_ggml_type_id(id)?;
        Ok(Self::for_layout(layout))
    }
}

/// Closed-set GGML type id → layout. Unknown ids fail closed.
///
/// # Errors
/// [`PackedAdmissionError::UnrepresentableLayout`] when `id` is outside
/// {F32, F16, BF16, `Q8_0`, `Q4_K`, `Q5_K`, `Q6_K`, `Q5_0`, MXFP4}.
pub fn packed_layout_from_ggml_type_id(
    id: u32,
) -> Result<PackedStorageLayout, PackedAdmissionError> {
    PackedStorageLayout::from_ggml_type_id(id)
        .ok_or(PackedAdmissionError::UnrepresentableLayout { ggml_type_id: id })
}

/// Layout from a trailing const operand (GGML type id). Non-const and
/// unknown ids fail closed.
///
/// # Errors
/// [`PackedAdmissionError::NonConstLayout`] or
/// [`PackedAdmissionError::UnrepresentableLayout`].
pub fn packed_layout_from_operand(
    operand: &MirOperand,
) -> Result<PackedStorageLayout, PackedAdmissionError> {
    let id = match operand {
        MirOperand::Constant(MirConstant::Int(value)) => u32::try_from(*value).ok(),
        MirOperand::Constant(MirConstant::UInt(value)) => u32::try_from(*value).ok(),
        _ => None,
    };
    let Some(id) = id else {
        return Err(PackedAdmissionError::NonConstLayout);
    };
    packed_layout_from_ggml_type_id(id)
}

/// Admit a matmul over `layout`: F32 → [`CollectionKernelPlan::TiledMatMul`];
/// packed §5 layouts → [`CollectionKernelPlan::QuantizedMatMul`] with typed
/// dequant. The layout is already a closed enum — unrepresentable ids are
/// rejected by [`packed_layout_from_ggml_type_id`] first.
#[must_use]
pub fn admit_quantized_matmul(
    layout: PackedStorageLayout,
    shapes: MatMulShapes,
) -> CollectionKernelPlan {
    match layout {
        PackedStorageLayout::F32 => tiled_matmul_from_shapes(shapes),
        packed => CollectionKernelPlan::QuantizedMatMul(QuantizedMatMulPlan {
            m: shapes.m,
            k: shapes.k,
            n: shapes.n,
            layout: packed,
            tile: MATMUL_TILE,
            workgroup_x: MATMUL_TILE,
            workgroup_y: MATMUL_TILE,
        }),
    }
}

/// Admit a matmul from a GGML type id (the first-failing-oracle surface).
///
/// # Errors
/// [`PackedAdmissionError::UnrepresentableLayout`] when `id` is not admitted.
pub fn admit_quantized_matmul_from_ggml_type_id(
    id: u32,
    shapes: MatMulShapes,
) -> Result<CollectionKernelPlan, PackedAdmissionError> {
    let layout = packed_layout_from_ggml_type_id(id)?;
    Ok(admit_quantized_matmul(layout, shapes))
}

/// Admit a row-gather over `layout`: F32 → existing [`GatherPlan`]; packed
/// §5 layouts → [`QuantizedGatherPlan`].
#[must_use]
pub fn admit_quantized_gather(
    layout: PackedStorageLayout,
    table_rows: u64,
    table_cols: u64,
    id_count: u64,
) -> AdmittedGather {
    match layout {
        PackedStorageLayout::F32 => AdmittedGather::Dense(GatherPlan {
            table_rows: PlanDim::Literal(table_rows),
            table_cols: PlanDim::Literal(table_cols),
            id_count: PlanDim::Literal(id_count),
        }),
        packed => AdmittedGather::Quantized(QuantizedGatherPlan {
            table_rows,
            table_cols,
            id_count,
            layout: packed,
            dequant: PackedDequantResolution::for_layout(packed),
        }),
    }
}

/// Admit a gather from a GGML type id.
///
/// # Errors
/// [`PackedAdmissionError::UnrepresentableLayout`] when `id` is not admitted.
pub fn admit_quantized_gather_from_ggml_type_id(
    id: u32,
    table_rows: u64,
    table_cols: u64,
    id_count: u64,
) -> Result<AdmittedGather, PackedAdmissionError> {
    let layout = packed_layout_from_ggml_type_id(id)?;
    Ok(admit_quantized_gather(
        layout, table_rows, table_cols, id_count,
    ))
}

/// Confirm a resolved subchain plan matches packed admission (R-PACK-02A
/// decomposition gate). Fail-closed on an unrepresentable gather/matmul
/// layout const; F32 quantized-matmul must already have fallen back to
/// [`CollectionKernelPlan::TiledMatMul`].
///
/// # Errors
/// [`PackedAdmissionError`] when a layout const is unrepresentable or a
/// `QuantizedMatMul` plan still carries F32.
pub fn confirm_packed_admission(
    function: &MirFunction,
    plan: &CollectionKernelPlan,
) -> Result<(), PackedAdmissionError> {
    if let Some(operand) = trailing_layout_operand(function, MirCollectionOp::TensorQuantizedMatMul)
    {
        let layout = packed_layout_from_operand(operand)?;
        match (layout, plan) {
            (PackedStorageLayout::F32, CollectionKernelPlan::TiledMatMul(_)) => {}
            (packed, CollectionKernelPlan::QuantizedMatMul(quantized))
                if packed != PackedStorageLayout::F32 && quantized.layout == packed => {}
            _ => {
                return Err(PackedAdmissionError::UnrepresentableLayout {
                    ggml_type_id: layout.ggml_type_id(),
                });
            }
        }
    }
    if let Some(operand) = trailing_layout_operand(function, MirCollectionOp::TensorGather) {
        let layout = packed_layout_from_operand(operand)?;
        // `GatherPlan` carries no layout field, so the gate is plan-shape only.
        if !matches!(plan, CollectionKernelPlan::Gather(_)) {
            return Err(PackedAdmissionError::UnrepresentableLayout {
                ggml_type_id: layout.ggml_type_id(),
            });
        }
    }
    Ok(())
}

/// Optional trailing GGML type-id const on `op` (the packed-layout fact).
pub(crate) fn trailing_layout_operand(
    function: &MirFunction,
    op: MirCollectionOp,
) -> Option<&MirOperand> {
    for block in &function.blocks {
        for statement in &block.statements {
            if crate::collection_ops::statement_collection_op(statement) != Some(op) {
                continue;
            }
            let MirStatementKind::RuntimeCall {
                call: MirRuntimeCall { args, .. },
                ..
            } = &statement.kind
            else {
                continue;
            };
            // Quantized matmul: [left, right, layout]. Gather: [table, ids, layout?].
            let index = match op {
                MirCollectionOp::TensorQuantizedMatMul | MirCollectionOp::TensorGather => 2,
                _ => return None,
            };
            return args.get(index);
        }
    }
    None
}

pub(super) fn tiled_matmul_from_shapes(shapes: MatMulShapes) -> CollectionKernelPlan {
    let tile = MATMUL_TILE;
    let tile_slots = tile * tile;
    CollectionKernelPlan::TiledMatMul(MatMulPlan {
        dims: crate::kernel_plan::MatMulDims::Literal {
            m: shapes.m,
            k: shapes.k,
            n: shapes.n,
        },
        m: shapes.m,
        k: shapes.k,
        n: shapes.n,
        left_layout: shapes.left_layout,
        right_layout: shapes.right_layout,
        tile,
        workgroup_x: tile,
        workgroup_y: tile,
        shared_memory: MatMulSharedMemory {
            shared_a: SharedMemoryLayout {
                element_byte_width: F32_ELEMENT_BYTE_WIDTH,
                slot_count: tile_slots,
                buffer_name: MATMUL_SHARED_A_BUFFER_NAME.to_owned(),
            },
            shared_b: SharedMemoryLayout {
                element_byte_width: F32_ELEMENT_BYTE_WIDTH,
                slot_count: tile_slots,
                buffer_name: MATMUL_SHARED_B_BUFFER_NAME.to_owned(),
            },
        },
        barriers: matmul_barrier_schedule(),
        oob_padding: OobPaddingPolicy::ZeroFill,
        batch: 1,
        right_batch: 1,
    })
}

impl fmt::Display for PackedAdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnrepresentableLayout { ggml_type_id } => write!(
                f,
                "packed kernel layout GGML type id {ggml_type_id} is unrepresentable \
                 (admitted: F32, F16, BF16, Q8_0, Q4_K, Q5_K, Q6_K, Q5_0, MXFP4)"
            ),
            Self::NonConstLayout => write!(
                f,
                "packed kernel layout must be a const GGML type id \
                 (F32, F16, BF16, Q8_0, Q4_K, Q5_K, Q6_K, Q5_0, MXFP4)"
            ),
        }
    }
}

impl std::error::Error for PackedAdmissionError {}

/// Why a packed block dequant / probe GEMM refused its input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackedDequantError {
    /// Packed block length is not the layout's `block_bytes`.
    BlockBytesMismatch { expected: u32, actual: usize },
    /// This unit has no in-kernel dequant formula for `layout`.
    UnrepresentableLayout { layout: PackedStorageLayout },
}

impl fmt::Display for PackedDequantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlockBytesMismatch { expected, actual } => write!(
                f,
                "packed dequant block is {actual} bytes; expected {expected}"
            ),
            Self::UnrepresentableLayout { layout } => write!(
                f,
                "packed dequant has no body for {layout:?} (admitted layouts land per R-PACK-02 unit)"
            ),
        }
    }
}

impl std::error::Error for PackedDequantError {}

/// IEEE-754 binary16 → binary32 (`ggml_compute_fp16_to_fp32`). Every
/// finite half is exact in f32; the mapping is a pure bit expansion.
#[must_use]
pub fn ggml_half_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) & 0x1;
    let exp = u32::from(bits >> 10) & 0x1f;
    let frac = u32::from(bits & 0x3ff);
    let bits32 = if exp == 0 {
        if frac == 0 {
            sign << 31
        } else {
            let p = 32 - frac.leading_zeros();
            (sign << 31) | ((p + 102) << 23) | ((frac << (24 - p)) - (1 << 23))
        }
    } else if exp == 0x1f {
        (sign << 31) | (0xff << 23) | (frac << 13)
    } else {
        (sign << 31) | ((exp + 112) << 23) | (frac << 13)
    };
    f32::from_bits(bits32)
}

/// In-kernel `Q8_0` block dequant (R-PACK-02B): `y[j] = i8(qs[j]) * f16(d)`.
///
/// # Errors
/// [`PackedDequantError::BlockBytesMismatch`] when `block` is not 34 bytes.
pub fn dequant_q8_0_block(block: &[u8]) -> Result<[f32; 32], PackedDequantError> {
    if block.len() != PackedStorageLayout::Q8_0.block_bytes() as usize {
        return Err(PackedDequantError::BlockBytesMismatch {
            expected: PackedStorageLayout::Q8_0.block_bytes(),
            actual: block.len(),
        });
    }
    let d = ggml_half_to_f32(u16::from_le_bytes([block[0], block[1]]));
    let mut y = [0.0f32; 32];
    for (j, slot) in y.iter_mut().enumerate() {
        *slot = f32::from(block[2 + j].cast_signed()) * d;
    }
    Ok(y)
}

/// `get_scale_min_k4` (llama.cpp `ggml-quants.c`): 6-bit scale + 6-bit min
/// from `scales[12]`.
fn get_scale_min_k4(index: usize, scales: &[u8]) -> (u8, u8) {
    if index < 4 {
        (scales[index] & 63, scales[index + 4] & 63)
    } else {
        (
            (scales[index + 4] & 0x0f) | ((scales[index - 4] >> 6) << 4),
            (scales[index + 4] >> 4) | ((scales[index] >> 6) << 4),
        )
    }
}

/// In-kernel `Q4_K` superblock dequant (R-PACK-02D): `d`/`dmin` halves +
/// `scales[12]` + `qs[128]` → 256 f32 elements.
///
/// # Errors
/// [`PackedDequantError::BlockBytesMismatch`] when `block` is not 144 bytes.
pub fn dequant_q4_k_block(block: &[u8]) -> Result<[f32; 256], PackedDequantError> {
    if block.len() != PackedStorageLayout::Q4_K.block_bytes() as usize {
        return Err(PackedDequantError::BlockBytesMismatch {
            expected: PackedStorageLayout::Q4_K.block_bytes(),
            actual: block.len(),
        });
    }
    let d = ggml_half_to_f32(u16::from_le_bytes([block[0], block[1]]));
    let min = ggml_half_to_f32(u16::from_le_bytes([block[2], block[3]]));
    let scales = &block[4..16];
    let qs = &block[16..144];
    let mut y = [0.0f32; 256];
    let mut is = 0usize;
    let mut qoff = 0usize;
    for chunk in [0usize, 64, 128, 192] {
        let (sc, m) = get_scale_min_k4(is, scales);
        let d1 = d * f32::from(sc);
        let m1 = min * f32::from(m);
        let (sc, m) = get_scale_min_k4(is + 1, scales);
        let d2 = d * f32::from(sc);
        let m2 = min * f32::from(m);
        for l in 0..32usize {
            y[chunk + l] = d1 * f32::from(qs[qoff + l] & 0x0f) - m1;
            y[chunk + 32 + l] = d2 * f32::from(qs[qoff + l] >> 4) - m2;
        }
        qoff += 32;
        is += 2;
    }
    Ok(y)
}

/// Dequant one packed block for an admitted layout. Unsupported layouts
/// fail closed.
///
/// # Errors
/// [`PackedDequantError`] when the block length is wrong or the layout
/// has no body yet.
pub fn dequant_packed_block(
    layout: PackedStorageLayout,
    block: &[u8],
) -> Result<Vec<f32>, PackedDequantError> {
    match layout {
        PackedStorageLayout::Q8_0 => Ok(dequant_q8_0_block(block)?.to_vec()),
        PackedStorageLayout::Q4_K => Ok(dequant_q4_k_block(block)?.to_vec()),
        PackedStorageLayout::Q5_K => Ok(super::packed_dequant::dequant_q5_k(block)?.to_vec()),
        PackedStorageLayout::Q6_K => Ok(super::packed_dequant::dequant_q6_k(block)?.to_vec()),
        other => Err(PackedDequantError::UnrepresentableLayout { layout: other }),
    }
}

/// Dense f32 GEMM `C[M,N] = A[M,K] · B[N,K]` used by the packed-kernel
/// probes. The right operand is already dequanted **per block** — this is
/// not a whole-model F32 materialization. The right operand is the
/// container's `[out][in]` row-major weight layout (out-row `col` holds the
/// `k` input weights — GEA2-U5g layout truth).
#[must_use]
pub fn gemm_f32(left: &[f32], m: usize, k: usize, right: &[f32], n: usize) -> Vec<f32> {
    debug_assert_eq!(left.len(), m * k);
    debug_assert_eq!(right.len(), k * n);
    let mut out = vec![0.0f32; m * n];
    for row in 0..m {
        for col in 0..n {
            let mut acc = 0.0f32;
            for inner in 0..k {
                acc += left[row * k + inner] * right[col * k + inner];
            }
            out[row * n + col] = acc;
        }
    }
    out
}
