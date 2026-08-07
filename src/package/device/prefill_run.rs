//! GI3-5 — the Q1-default prefill device-run driver (faber CLI device-run
//! path; `gi3-delivery.md` §GI3-5 Q1 default).
//!
//! This module is the **execution half** of the GI3 serial integration: it
//! constructs the prefill **wire program** from the admitted row facts,
//! repacks the 290 admitted weight tensors to the **declared f32
//! conversion** (GI3-2 repack plan — the GI2-1 dequant semantics with the
//! pinned comparator's f16 register rounding for quantized tensors, **never
//! presented as direct GGUF quantized execution**), assembles the prefill
//! **MIR program** (the `DeviceProgram` + `ValidatedMir` the emitters
//! consume), emits the Metal artifact, and executes a **`SingleRun`** prefill
//! on the device route through the existing `ProgramSession` — runtime
//! weight values supplied at session execution, **no wire-schema change**
//! (`WIRE_DEVICE_PROGRAM_VERSION` and the `FmirDeviceSection` stay
//! untouched).
//!
//! The GPU prompt-final logits are then compared against the committed GI2-3
//! logits golden under the Q2 thresholds ([`compare_gpu_logits`]) and the
//! result is recorded as the committed comparison record + S6 receipts
//! (`radix/docs/factory/gpu-inference-gguf/evidence/`).
//!
//! # Kernel graph
//!
//! The prefill program is assembled from the pinned-row facts
//! (SmolLM2-360M-Instruct Q4_K_M): embedding gather over the tied
//! `token_embd.weight` → 32 decoder layers (attn_norm → QKV matmuls → RoPE →
//! GQA causal attention → attn_output → residual → ffn_norm → SwiGLU FFN →
//! residual) → output_norm → tied-head projection → full-vocab logits
//! `[9, 49152]`. GQA (15/5 heads) is expressed on the frozen recipe surface
//! with **per-head kernel fan-out**: `attn_q`/`attn_output` are split into
//! the 15 query heads and `attn_k`/`attn_v` into the 5 KV heads, so each
//! query head's score row is scaled/softmaxed independently (the oracle
//! softmaxes each query head's scores) and the 15 head outputs fold back
//! into the `[9, 960]` context.
//!
//! **GI3-1 contract amendment (task 74264397):** RoPE rotates **per
//! position** (the rank-2 `[rows, dim/2]` cos/sin tables; the kernel reads
//! `table[row · (dim/2) + pair]` — each row at its own position), and the
//! device K/V path applies the comparator's **f16 register rounding**
//! (`kv_f16_round`) via the `MirUnOp::F16Round` unary — K after RoPE, V
//! directly; Q stays f32 (the oracle's application order).
//!
//! # Constraints carried in
//!
//! - No per-op host recomputation in the execution path (the oracle is
//!   comparison-only).
//! - No hidden llama.cpp execution (the golden is the committed fixture).
//! - The S6 prefill-regime fields are recorded separately on the receipt
//!   (shape class / representation / algorithm / workspace / evidence, CTO
//!   S6) plus the repack/conversion, module-prep, upload, and first-
//!   invocation/capture timing (CTO S11).
//! - The burgus Metal device run is the gated consumer step (env-gated).

use super::{
    admit_device_program_section, artifact_for_backend, descriptor_for_backend,
    device_section_for_program, inputs_by_buffer_id, BTreeMap, BufferId, BufferIdentity,
    BufferLifetime, BufferRole, BufferVersion, CollectionKernelPlan, DeviceBackend,
    DeviceProgram, DeviceProgramLifetime, DeviceResource, DeviceSectionBuild, DeviceSelection,
    Diagnostic, FmirDeviceSection, Interner, KernelLaunchPlan, LaunchId, LaunchUnit, MirFunction,
    MirFunctionId, MirKernelResourceAccess, MirTensorStorageLayout, MirType, ValidatedMir,
};
use faber::dequant::{dequant_tensor, OracleReceipt};
use faber::gguf::admit_file;
use faber::json::Json;
use faber::prefill::{
    compare_gpu_logits, ExecutableRegime, PrefillComparison, PrefillReceipt, PrefillRegimeFields,
    PROMPT_TOKENS,
};
use faber::tensor_view::TensorView;
use faber::valor::Valor;
use radix::lexer::Span;
use radix::mir::{
    MirBlock, MirBlockId, MirConstant, MirIntrinsic, MirLocal, MirLocalId, MirOperand, MirParam,
    MirParamMode, MirPlace, MirProgram, MirRuntimeCall, MirStatement, MirStatementKind,
    MirTerminator, MirTerminatorKind, MirUnOp, MirValue, MirValueId, MirValueKind,
    MirValidationContext,
};
use radix::semantic::{Primitive, Type, TypeTable};
use radix_mir::abi::{MirKernelResourceKind, MirKernelSignature};
use radix_mir::device_program::{Binding, KernelUnit, ObservationCadence, ResultBuffer};
use radix_mir::device_program_plans::{
    kernel_plan_for_function, transformer_shape_signature,
};
use radix_mir::device_semantics::{DependencyEdge, DeviceSemantics};
use radix_mir_fmir::schema::WireBufferRole;
use radix_types::{IndexExpr, NumericWidth};
use std::collections::BTreeMap as StdBTreeMap;
use std::path::Path;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Pinned row facts (frozen by the model contract v1.0.0 + decoder_ops)
// ---------------------------------------------------------------------------

/// The hidden width.
const HIDDEN_SIZE: u64 = 960;
/// The query head count.
const HEAD_COUNT: u64 = 15;
/// The KV head count.
const KV_HEAD_COUNT: u64 = 5;
/// The head dimension.
const HEAD_DIM: u64 = 64;
/// The FFN hidden width.
const FFN_SIZE: u64 = 2560;
/// The vocab size.
const VOCAB_SIZE: u64 = 49152;
/// The pinned prompt token count.
const PROMPT_TOKEN_COUNT: u64 = 9;
/// RMSNorm epsilon (the frozen 1e-5).
const RMS_EPS: f64 = 1e-5;
/// The attention score scale (1/sqrt(head_dim) = 0.125).
const ATTENTION_SCALE: f64 = 0.125;
/// The query heads per KV head (consecutive-triples grouping, FC7).
const QUERY_HEADS_PER_KV: u64 = HEAD_COUNT / KV_HEAD_COUNT;
/// The layer count.
const LAYER_COUNT: u64 = 32;

// ---------------------------------------------------------------------------
// Weight loading + the declared f32 repack (GI3-2)
// ---------------------------------------------------------------------------

/// The declared f32 conversion of one layer's weights.
///
/// Every tensor is dequantized through the GI2-1 dequant semantics and
/// f16-rounded per the pinned comparator's Metal register arithmetic (the
/// oracle applies `metal_f16_round_weights` to the embedding and ALL nine
/// per-layer tensors; only the F32 `output_norm` is unrounded). The
/// attention weights are **split per head** (`attn_q`/`attn_out` into the 15
/// query heads, `attn_k`/`attn_v` into the 5 KV heads) so the per-head
/// attention softmax is expressible on the recipe surface — the oracle
/// softmaxes each query head's score row independently.
struct LayerRepack {
    attn_norm: Vec<f32>,
    /// 15 query-head K-major slices of `attn_q` `[960, 960]`, each
    /// `[960, 64]` (the device matmul right operand — the oracle's `dense`
    /// K-major layout, see [`split_columns`]).
    attn_q_heads: Vec<Vec<f32>>,
    /// 5 KV-head K-major slices of `attn_k` `[960, 320]`, each `[960, 64]`.
    attn_k_heads: Vec<Vec<f32>>,
    /// 5 KV-head K-major slices of `attn_v` `[960, 320]`, each `[960, 64]`.
    attn_v_heads: Vec<Vec<f32>>,
    /// 15 query-head K-major slices of `attn_output` `[960, 960]`, each
    /// `[64, 960]` (see [`split_rows`]).
    attn_out_heads: Vec<Vec<f32>>,
    ffn_norm: Vec<f32>,
    ffn_gate: Vec<f32>,
    ffn_up: Vec<f32>,
    ffn_down: Vec<f32>,
}

/// The repacked prefill weight set for the admitted row.
struct PrefillWeights {
    /// `token_embd.weight` — `[49152, 960]` vocab-major rows (the tied
    /// embedding/output head).
    token_embd: Vec<f32>,
    layers: Vec<LayerRepack>,
    output_norm: Vec<f32>,
}

/// f16 round-trip of a single f32 (RN), matching `kv_f16_round`.
fn f16_round(value: f32) -> f32 {
    half_to_f32(faber::cpu_oracle::f32_to_f16_rn(value))
}

/// f16 bits -> f32 — the bit-exact decode (identical to the oracle's
/// `faber_runtime::dequant::half_to_f32`, so the declared-f32 repack's f16
/// rounding is byte-identical to the CPU oracle's `kv_f16_round`).
fn half_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) & 0x1;
    let exp = u32::from(bits >> 10) & 0x1f;
    let frac = u32::from(bits & 0x3ff);
    let bits32 = if exp == 0 {
        if frac == 0 {
            sign << 31 // ±0
        } else {
            // Subnormal: value = frac × 2^-24, frac ∈ [1, 1023]. Normalize
            // the leading bit: with p = bit_length(frac), the value is
            // (frac × 2^(1-p)) × 2^(p-25), so the f32 exponent field is
            // p - 25 + 127 = p + 102 and the mantissa field is
            // (frac × 2^(1-p) - 1) × 2^23 = frac × 2^(24-p) - 2^23.
            let p = 32 - frac.leading_zeros();
            (sign << 31) | ((p + 102) << 23) | ((frac << (24 - p)) - (1 << 23))
        }
    } else if exp == 0x1f {
        (sign << 31) | (0xff << 23) | (frac << 13) // ±inf / NaN
    } else {
        (sign << 31) | ((exp + 112) << 23) | (frac << 13) // 127 - 15 = 112
    };
    f32::from_bits(bits32)
}

/// Split a K-major `[in_dim, out_dim]` weight (row-major storage
/// `data[i + in_dim * j]` = the CPU oracle's `dense` layout — input `i`,
/// output `j`) into per-head column slices for the device matmul right
/// operand: head `h` gets the `[in_dim, head_dim]` slice
/// `slice[k * head_dim + n] = source[k + in_dim * (h * head_dim + n)]` —
/// output column `(h * head_dim + n)` read K-major, so the device matmul
/// `a · slice` reproduces the oracle's `q/k/v` head outputs exactly.
fn split_columns(source: &[f32], in_dim: usize, out_dim: usize, head_dim: usize) -> Vec<Vec<f32>> {
    let head_count = out_dim / head_dim;
    (0..head_count)
        .map(|head| {
            let mut slice = vec![0.0f32; in_dim * head_dim];
            for k in 0..in_dim {
                for n in 0..head_dim {
                    slice[k * head_dim + n] = source[k + in_dim * (head * head_dim + n)];
                }
            }
            slice
        })
        .collect()
}

/// Split a K-major `[in_dim, out_dim]` weight into per-head ROW slices for
/// the device matmul right operand: head `h` gets the `[head_dim, out_dim]`
/// slice `slice[k * out_dim + d] = source[d * in_dim + h * head_dim + k]` —
/// input row `(h * head_dim + k)` read K-major (the attn_output per-head
/// projection weights).
fn split_rows(source: &[f32], in_dim: usize, out_dim: usize, head_dim: usize) -> Vec<Vec<f32>> {
    let head_count = in_dim / head_dim;
    (0..head_count)
        .map(|head| {
            let mut slice = vec![0.0f32; head_dim * out_dim];
            for k in 0..head_dim {
                for d in 0..out_dim {
                    slice[k * out_dim + d] = source[d * in_dim + head * head_dim + k];
                }
            }
            slice
        })
        .collect()
}

/// Transpose a K-major `[in_dim, out_dim]` weight (storage
/// `data[i + in_dim * j]` — input `i`, output `j`) to the device matmul
/// right operand's row-major layout `[in_dim, out_dim]`:
/// `out[k * out_dim + n] = source[k + in_dim * n]`. The FFN matmul weights
/// (`ffn_gate`/`ffn_up`/`ffn_down`) ride the device right operand directly
/// (no per-head split), so they need this single transpose to reproduce the
/// oracle's `dense` (which reads K-major) exactly.
fn transpose_kmajor(source: &[f32], in_dim: usize, out_dim: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; in_dim * out_dim];
    for k in 0..in_dim {
        for n in 0..out_dim {
            out[k * out_dim + n] = source[k + in_dim * n];
        }
    }
    out
}

impl PrefillWeights {
    /// Load the pinned model file and repack the 290 admitted tensors to the
    /// declared f32 conversion (GI3-2). Returns the repacked set plus the
    /// per-tensor oracle receipts (the GI2-1 materialization contract).
    fn load(path: &Path) -> Result<(Self, Vec<OracleReceipt>), Vec<Diagnostic>> {
        let bytes = std::fs::read(path).map_err(|error| {
            vec![Diagnostic::error(format!(
                "prefill device run: cannot read the admitted model file `{}`: {error}",
                path.display()
            ))]
        })?;
        let admission = admit_file(path).map_err(|error| {
            vec![Diagnostic::error(format!(
                "prefill device run: GGUF admission failed for `{}`: {error}",
                path.display()
            ))]
        })?;
        let view = TensorView::build(&admission, &bytes).map_err(|error| {
            vec![Diagnostic::error(format!(
                "prefill device run: tensor view build failed: {error}"
            ))]
        })?;
        if !view.coverage_ok() {
            return Err(vec![Diagnostic::error(
                "prefill device run: the admitted model is not fully covered by the tensor view",
            )
            .with_arg("issue", "E_PREFILL_COVERAGE")]);
        }

        let mut receipts = Vec::with_capacity(290);
        let mut tensor = |name: &str| -> Result<Vec<f32>, Vec<Diagnostic>> {
            let entry = view.tensor(name).ok_or_else(|| {
                vec![Diagnostic::error(format!(
                    "prefill device run: admitted tensor `{name}` not found in the model"
                ))
                .with_arg("issue", "E_PREFILL_TENSOR_MISSING")]
            })?;
            let values = dequant_tensor(&view, entry).map_err(|error| {
                vec![Diagnostic::error(format!(
                    "prefill device run: dequant of `{name}` failed: {error}"
                ))]
            })?;
            receipts.push(OracleReceipt::for_tensor(&view, entry).map_err(|error| {
                vec![Diagnostic::error(format!(
                    "prefill device run: oracle receipt for `{name}` failed: {error}"
                ))]
            })?);
            // The comparator's f16 register rounding (the pinned Metal
            // matmul path stores dequantized weights as f16) — the oracle
            // applies `metal_f16_round_weights` to the embedding and ALL
            // nine per-layer tensors (including the F32 attn_norm/ffn_norm;
            // the ONLY unrounded tensor is the F32 `output_norm`, which is
            // loaded separately below).
            Ok(values.into_iter().map(f16_round).collect())
        };
        // `output_norm.weight` is the ONE tensor the oracle does NOT
        // f16-round (`CpuOracle::build` loads it via plain dequant); its
        // receipt is appended after the rounding loop (the `tensor` closure
        // holds `&mut receipts`).
        let output_norm_entry = view.tensor("output_norm.weight").ok_or_else(|| {
            vec![Diagnostic::error(
                "prefill device run: admitted tensor `output_norm.weight` not found in the model",
            )]
        })?;
        let output_norm = dequant_tensor(&view, output_norm_entry).map_err(|error| {
            vec![Diagnostic::error(format!(
                "prefill device run: dequant of `output_norm.weight` failed: {error}"
            ))]
        })?;

        let token_embd = tensor("token_embd.weight")?;
        let mut layers = Vec::with_capacity(LAYER_COUNT as usize);
        for il in 0..32 {
            let name = |base: &str| format!("blk.{il}.{base}");
            let attn_q = tensor(&name("attn_q.weight"))?;
            let attn_k = tensor(&name("attn_k.weight"))?;
            let attn_v = tensor(&name("attn_v.weight"))?;
            let attn_output = tensor(&name("attn_output.weight"))?;
            // The FFN matmul weights ride the device right operand directly,
            // so they are transposed from the K-major storage to the matmul
            // row-major layout (the per-head attention weights are
            // transposed inside their splits).
            let ffn_gate = tensor(&name("ffn_gate.weight"))?;
            let ffn_up = tensor(&name("ffn_up.weight"))?;
            let ffn_down = tensor(&name("ffn_down.weight"))?;
            layers.push(LayerRepack {
                attn_norm: tensor(&name("attn_norm.weight"))?,
                attn_q_heads: split_columns(
                    &attn_q,
                    HIDDEN_SIZE as usize,
                    (HEAD_COUNT * HEAD_DIM) as usize,
                    HEAD_DIM as usize,
                ),
                attn_k_heads: split_columns(
                    &attn_k,
                    HIDDEN_SIZE as usize,
                    (KV_HEAD_COUNT * HEAD_DIM) as usize,
                    HEAD_DIM as usize,
                ),
                attn_v_heads: split_columns(
                    &attn_v,
                    HIDDEN_SIZE as usize,
                    (KV_HEAD_COUNT * HEAD_DIM) as usize,
                    HEAD_DIM as usize,
                ),
                attn_out_heads: split_rows(
                    &attn_output,
                    HIDDEN_SIZE as usize,
                    HIDDEN_SIZE as usize,
                    HEAD_DIM as usize,
                ),
                ffn_norm: tensor(&name("ffn_norm.weight"))?,
                ffn_gate: transpose_kmajor(&ffn_gate, HIDDEN_SIZE as usize, FFN_SIZE as usize),
                ffn_up: transpose_kmajor(&ffn_up, HIDDEN_SIZE as usize, FFN_SIZE as usize),
                ffn_down: transpose_kmajor(&ffn_down, FFN_SIZE as usize, HIDDEN_SIZE as usize),
            });
        }
        receipts.push(
            OracleReceipt::for_tensor(&view, output_norm_entry).map_err(|error| {
                vec![Diagnostic::error(format!(
                    "prefill device run: oracle receipt for `output_norm.weight` failed: {error}"
                ))]
            })?,
        );
        Ok((
            Self {
                token_embd,
                layers,
                output_norm,
            },
            receipts,
        ))
    }

    /// The declared-inputs map: every Input-role weight buffer name → its
    /// runtime f32 values (the Q1-default runtime value supply).
    fn declared_inputs(&self) -> BTreeMap<String, Vec<f32>> {
        let mut map = BTreeMap::new();
        map.insert("token_embd.weight".to_owned(), self.token_embd.clone());
        for (il, layer) in self.layers.iter().enumerate() {
            let name = |base: &str| format!("blk.{il}.{base}");
            map.insert(name("attn_norm.weight"), layer.attn_norm.clone());
            for (head, slice) in layer.attn_q_heads.iter().enumerate() {
                map.insert(
                    format!("blk.{il}.attn_q.h{head}.weight"),
                    slice.clone(),
                );
            }
            for (head, slice) in layer.attn_k_heads.iter().enumerate() {
                map.insert(
                    format!("blk.{il}.attn_k.g{head}.weight"),
                    slice.clone(),
                );
            }
            for (head, slice) in layer.attn_v_heads.iter().enumerate() {
                map.insert(
                    format!("blk.{il}.attn_v.g{head}.weight"),
                    slice.clone(),
                );
            }
            for (head, slice) in layer.attn_out_heads.iter().enumerate() {
                map.insert(
                    format!("blk.{il}.attn_output.h{head}.weight"),
                    slice.clone(),
                );
            }
            map.insert(name("ffn_norm.weight"), layer.ffn_norm.clone());
            map.insert(name("ffn_gate.weight"), layer.ffn_gate.clone());
            map.insert(name("ffn_up.weight"), layer.ffn_up.clone());
            map.insert(name("ffn_down.weight"), layer.ffn_down.clone());
        }
        map.insert("output_norm.weight".to_owned(), self.output_norm.clone());
        map
    }
}

// ---------------------------------------------------------------------------
// MIR function construction (the prefill MIR program)
// ---------------------------------------------------------------------------

/// The prompt-final tokens as the gather ids' f32 bit patterns (the wire
/// dtype surface pins f32; the gather device body reads the ids as
/// `numerus<u32>`, so the driver supplies the u32 bit patterns through the
/// f32 carrier — the same 4 bytes).
fn prompt_ids_values() -> Vec<f32> {
    PROMPT_TOKENS
        .iter()
        .map(|token| f32::from_bits(*token as u32))
        .collect()
}

fn span() -> Span {
    Span::default()
}

fn tensor_ty(types: &mut TypeTable, dims: &[u64]) -> MirType {
    let element = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let shape = if dims.len() == 1 {
        types.intern_index(IndexExpr::Literal(dims[0]))
    } else {
        let dims: Vec<_> = dims
            .iter()
            .map(|dim| types.intern_index(IndexExpr::Literal(*dim)))
            .collect();
        types.intern_index(IndexExpr::Tuple(dims))
    };
    MirType::semantic(types.intern(Type::Tensor(element, shape)))
}

fn tensor_param(local: u32, ty: MirType) -> MirParam {
    MirParam {
        local: MirLocalId(local),
        name: None,
        ty,
        mode: MirParamMode::Owned,
        span: span(),
    }
}

fn typed_local(local: u32, ty: MirType) -> MirLocal {
    MirLocal {
        id: MirLocalId(local),
        name: None,
        ty,
        mutable: false,
        span: span(),
    }
}

fn collection_call(op: MirIntrinsic, args: &[MirOperand], dest: u32, return_ty: MirType) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::local(MirLocalId(dest))),
            call: MirRuntimeCall {
                intrinsic: op,
                args: args.to_vec(),
                return_ty,
            },
        },
        span: span(),
    }
}

fn place(local: u32) -> MirOperand {
    MirOperand::Place(MirPlace::local(MirLocalId(local)))
}

fn const_float(value: f64) -> MirOperand {
    MirOperand::Constant(MirConstant::Float(value))
}

fn const_int(value: i64) -> MirOperand {
    MirOperand::Constant(MirConstant::Int(value))
}

fn exp_statement(input: u32, dest: u32, ty: MirType) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::local(MirLocalId(dest)),
            value: MirValue {
                id: MirValueId(0),
                kind: MirValueKind::Unary {
                    op: MirUnOp::Exp,
                    operand: place(input),
                },
                ty,
                span: span(),
            },
        },
        span: span(),
    }
}

fn vacuum_return() -> MirTerminator {
    MirTerminator {
        kind: MirTerminatorKind::Return(None),
        span: span(),
    }
}

fn value_return(local: u32) -> MirTerminator {
    MirTerminator {
        kind: MirTerminatorKind::Return(Some(place(local))),
        span: span(),
    }
}

/// The embedding row-gather function: `out[T, 960] = table[ids, :]` (the
/// pinned `[49152, 960]` vocab-major token-embedding table).
fn gather_function(types: &mut TypeTable, id: u32, id_count: u64) -> MirFunction {
    let table_ty = tensor_ty(types, &[VOCAB_SIZE, HIDDEN_SIZE]);
    let ids_element = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let ids_ty = MirType::semantic(types.array(ids_element));
    let out_ty = tensor_ty(types, &[id_count, HIDDEN_SIZE]);
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: vec![tensor_param(0, table_ty), tensor_param(1, ids_ty)],
        locals: vec![
            typed_local(0, table_ty),
            typed_local(1, ids_ty),
            typed_local(2, out_ty),
        ],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![collection_call(
                MirIntrinsic::Collection(radix_mir::MirCollectionOp::TensorGather),
                &[place(0), place(1)],
                2,
                out_ty,
            )],
            terminator: value_return(2),
            span: span(),
        }],
        return_ty: out_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: span(),
    }
}

/// The RMSNorm function: `out = rms_norm(x, last_axis, eps, gamma)` — no
/// mean subtraction (the pinned row's norm, NOT LayerNormalization).
fn rms_norm_function(types: &mut TypeTable, id: u32, dims: [u64; 2]) -> MirFunction {
    let x_ty = tensor_ty(types, &dims);
    let gamma_ty = tensor_ty(types, &[dims[1]]);
    let u32_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    // `dims` is always the two-dimensional activation shape here, so the
    // norm axis is statically 1 — safe by construction (no fallible cast).
    let axis = (dims.len() - 1) as i64;
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: vec![
            tensor_param(0, x_ty),
            tensor_param(1, gamma_ty),
            tensor_param(2, x_ty),
            tensor_param(3, u32_ty),
        ],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![collection_call(
                MirIntrinsic::Collection(radix_mir::MirCollectionOp::TensorRmsNorm),
                &[place(0), const_int(axis), const_float(RMS_EPS), place(1)],
                2,
                x_ty,
            )],
            terminator: vacuum_return(),
            span: span(),
        }],
        return_ty: vacuum_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: span(),
    }
}

/// The RoPE function (llama-arch NORM consecutive-pair rotation): the
/// cos/sin tables are host-precomputed and are the kernel's two extra
/// inputs; `pos`/`dim` are the CCI-1 const operands. **Per-position mode**
/// (the GI3-1 contract amendment): the tables are rank-2 `[rows, dim/2]` and
/// the kernel rotates each row at its own position; the plan resolution
/// detects the mode from the table shapes.
fn rope_function(types: &mut TypeTable, id: u32, dims: [u64; 2], pos: i64, dim: i64) -> MirFunction {
    let x_ty = tensor_ty(types, &dims);
    let rows = dims[0];
    // `dim` is the pinned per-head width (`HEAD_DIM`, a compile-time const),
    // so `dim / 2` is a small non-negative value — safe by construction.
    let table_len = (dim / 2) as u64;
    let table_ty = tensor_ty(types, &[rows, table_len]);
    let u32_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: vec![
            tensor_param(0, x_ty),
            tensor_param(1, table_ty),
            tensor_param(2, table_ty),
            tensor_param(3, x_ty),
            tensor_param(4, u32_ty),
        ],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![collection_call(
                MirIntrinsic::Collection(radix_mir::MirCollectionOp::TensorRope),
                &[place(0), const_int(pos), const_int(dim)],
                3,
                x_ty,
            )],
            terminator: vacuum_return(),
            span: span(),
        }],
        return_ty: vacuum_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: span(),
    }
}

/// The causal masked-softmax function over the `[rows, cols]` score matrix.
fn causal_softmax_function(types: &mut TypeTable, id: u32, dims: [u64; 2]) -> MirFunction {
    let x_ty = tensor_ty(types, &dims);
    let u32_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: vec![
            tensor_param(0, x_ty),
            tensor_param(1, x_ty),
            tensor_param(2, u32_ty),
        ],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![collection_call(
                MirIntrinsic::Collection(radix_mir::MirCollectionOp::TensorCausalMaskedSoftmax),
                &[place(0)],
                1,
                x_ty,
            )],
            terminator: vacuum_return(),
            span: span(),
        }],
        return_ty: vacuum_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: span(),
    }
}

/// The matmul function: `out[M, N] = left[M, K] * right[K, N]`.
fn matmul_function(types: &mut TypeTable, id: u32, m: u64, k: u64, n: u64) -> MirFunction {
    let left_ty = tensor_ty(types, &[m, k]);
    let right_ty = tensor_ty(types, &[k, n]);
    let out_ty = tensor_ty(types, &[m, n]);
    let u32_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: vec![
            tensor_param(0, left_ty),
            tensor_param(1, right_ty),
            tensor_param(2, out_ty),
            tensor_param(3, u32_ty),
        ],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![collection_call(
                MirIntrinsic::Collection(radix_mir::MirCollectionOp::TensorMatMul),
                &[place(0), place(1)],
                2,
                out_ty,
            )],
            terminator: vacuum_return(),
            span: span(),
        }],
        return_ty: vacuum_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: span(),
    }
}

/// The transpose function: `out[N, M] = transpose(x[M, N])`.
fn transpose_function(types: &mut TypeTable, id: u32, dims: [u64; 2]) -> MirFunction {
    let x_ty = tensor_ty(types, &dims);
    let out_ty = tensor_ty(types, &[dims[1], dims[0]]);
    let u32_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    let vacuum_ty = MirType::semantic(types.primitive(Primitive::Vacuum));
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: vec![
            tensor_param(0, x_ty),
            tensor_param(1, out_ty),
            tensor_param(2, u32_ty),
        ],
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![collection_call(
                MirIntrinsic::Collection(radix_mir::MirCollectionOp::TensorTranspose),
                &[place(0)],
                1,
                out_ty,
            )],
            terminator: vacuum_return(),
            span: span(),
        }],
        return_ty: vacuum_ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: span(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementwiseUnary {
    Neg,
    Exp,
    /// The half-precision round trip (the comparator's `kv_f16_round` K/V
    /// register rounding): `f16_round(x)`.
    F16Round,
}

/// A single-input elementwise function (TensorNeg / the Exp unary / the
/// F16Round unary): `out = op(x)`.
fn elementwise_unary_function(
    types: &mut TypeTable,
    id: u32,
    dims: &[u64],
    op: ElementwiseUnary,
) -> MirFunction {
    let ty = tensor_ty(types, dims);
    let statement = match op {
        ElementwiseUnary::Neg => collection_call(
            MirIntrinsic::Collection(radix_mir::MirCollectionOp::TensorNeg),
            &[place(0)],
            1,
            ty,
        ),
        ElementwiseUnary::Exp => exp_statement(0, 1, ty),
        ElementwiseUnary::F16Round => MirStatement {
            kind: MirStatementKind::Assign {
                place: MirPlace::local(MirLocalId(1)),
                value: MirValue {
                    id: MirValueId(0),
                    kind: MirValueKind::Unary {
                        op: MirUnOp::F16Round,
                        operand: place(0),
                    },
                    ty,
                    span: span(),
                },
            },
            span: span(),
        },
    };
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: vec![tensor_param(0, ty)],
        locals: vec![typed_local(0, ty), typed_local(1, ty)],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![statement],
            terminator: value_return(1),
            span: span(),
        }],
        return_ty: ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: span(),
    }
}

/// A scalar-constant elementwise function (`1 + x`, `1 / x`, `x · c`).
///
/// The scalar is materialized as a `TensorFill` tensor first (the emitter's
/// `fill_one` broadcast precedent — the `1+e` / `1/a` composition shape), so
/// every binary elementwise op operates on two tensor operands and the MIR
/// validator's "tensor intrinsic receiver is not tensor" rule is satisfied.
fn elementwise_scalar_function(
    types: &mut TypeTable,
    id: u32,
    dims: &[u64],
    op: ElementwiseScalarOp,
    scalar: f64,
) -> MirFunction {
    let ty = tensor_ty(types, dims);
    // Local layout: 0 = x (param), 1 = the filled scalar tensor, 2 = result.
    let fill = fill_statement(1, 0, scalar, ty);
    let result = match op {
        ElementwiseScalarOp::Add => collection_call(
            MirIntrinsic::Collection(radix_mir::MirCollectionOp::TensorAdd),
            &[place(1), place(0)],
            2,
            ty,
        ),
        ElementwiseScalarOp::Div => collection_call(
            MirIntrinsic::Collection(radix_mir::MirCollectionOp::TensorDiv),
            &[place(1), place(0)],
            2,
            ty,
        ),
        ElementwiseScalarOp::Mul => collection_call(
            MirIntrinsic::Collection(radix_mir::MirCollectionOp::TensorMul),
            &[place(0), place(1)],
            2,
            ty,
        ),
    };
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: vec![tensor_param(0, ty)],
        locals: vec![typed_local(0, ty), typed_local(1, ty), typed_local(2, ty)],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![fill, result],
            terminator: value_return(2),
            span: span(),
        }],
        return_ty: ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: span(),
    }
}

/// The `TensorFill` scalar-materialization statement: `dest = fill(x, value)`
/// — the emitter's `fill_one` broadcast precedent.
fn fill_statement(dest: u32, shape_local: u32, value: f64, ty: MirType) -> MirStatement {
    collection_call(
        MirIntrinsic::Collection(radix_mir::MirCollectionOp::TensorFill),
        &[place(shape_local), const_float(value)],
        dest,
        ty,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ElementwiseScalarOp {
    /// `1 + x`.
    Add,
    /// `1 / x`.
    Div,
    /// `x · c`.
    Mul,
}

/// A binary elementwise function (`a + b` / `a * b`).
fn elementwise_binary_function(
    types: &mut TypeTable,
    id: u32,
    dims: &[u64],
    op: radix_mir::MirCollectionOp,
) -> MirFunction {
    let ty = tensor_ty(types, dims);
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: vec![tensor_param(0, ty), tensor_param(1, ty)],
        locals: vec![typed_local(0, ty), typed_local(1, ty), typed_local(2, ty)],
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![collection_call(
                MirIntrinsic::Collection(op),
                &[place(0), place(1)],
                2,
                ty,
            )],
            terminator: value_return(2),
            span: span(),
        }],
        return_ty: ty,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: span(),
    }
}

// ---------------------------------------------------------------------------
// Device-program assembly (two-phase: functions, then kernels)
// ---------------------------------------------------------------------------

/// The prefill MIR program: the functions (owned) + the TypeTable the
/// caller validates against.
struct PrefillMirProgram {
    functions: MirProgram,
    types: TypeTable,
}

/// One kernel slot's buffer facts (id/name/role/lifetime).
#[derive(Debug, Clone)]
struct BufferFacts {
    id: u32,
    name: String,
    role: BufferRole,
    lifetime: BufferLifetime,
}

impl BufferFacts {
    fn input(id: u32, name: &str) -> Self {
        Self {
            id,
            name: name.to_owned(),
            role: BufferRole::Input,
            lifetime: BufferLifetime::PerProgram,
        }
    }

    fn activation(id: u32, name: &str) -> Self {
        Self {
            id,
            name: name.to_owned(),
            role: BufferRole::InOut,
            lifetime: BufferLifetime::PerStep,
        }
    }

    fn identity(&self) -> BufferIdentity {
        BufferIdentity {
            id: BufferId(self.id),
            name: self.name.clone(),
            role: self.role,
            storage: MirTensorStorageLayout::DeviceHandle,
            lifetime: self.lifetime,
        }
    }
}

/// The kernel assembler: builds the kernels/launches/results from the MIR
/// program (immutable borrows) — the two phases never borrow-conflict.
struct KernelAssembler {
    kernels: Vec<KernelUnit>,
    launches: Vec<LaunchUnit>,
    results: Vec<ResultBuffer>,
    next_buffer: u32,
    next_launch: u32,
    /// buffer name → facts (for the weight/prompt input buffers).
    known: BTreeMap<String, BufferFacts>,
}

impl KernelAssembler {
    fn new() -> Self {
        Self {
            kernels: Vec::new(),
            launches: Vec::new(),
            results: Vec::new(),
            next_buffer: 1,
            next_launch: 1,
            known: BTreeMap::new(),
        }
    }

    /// Register a weight/prompt input buffer by name.
    fn register_input(&mut self, name: &str) -> u32 {
        if let Some(existing) = self.known.get(name) {
            return existing.id;
        }
        let id = self.next_buffer;
        self.next_buffer += 1;
        self.known
            .insert(name.to_owned(), BufferFacts::input(id, name));
        id
    }

    /// Look up a registered input buffer's facts.
    ///
    /// Static invariant (safe by construction): every name queried here is
    /// registered by `build_prefill_program` (or `rope_table_buffers`) before
    /// kernel-graph assembly reaches it, so the lookup always succeeds.
    fn input_facts(&self, name: &str) -> BufferFacts {
        self.known
            .get(name)
            .cloned()
            .expect("registered input buffer")
    }

    fn fresh_activation(&mut self, name: &str) -> BufferFacts {
        let id = self.next_buffer;
        self.next_buffer += 1;
        BufferFacts::activation(id, name)
    }

    fn push_kernel(
        &mut self,
        function: MirFunctionId,
        entry: String,
        plan: CollectionKernelPlan,
        resources: Vec<DeviceResource>,
        launch: KernelLaunchPlan,
    ) -> LaunchId {
        let kernel_index = self.kernels.len();
        self.kernels.push(KernelUnit {
            function,
            entry,
            plan,
            resources,
            launch,
        });
        let id = LaunchId(self.next_launch);
        self.next_launch += 1;
        self.launches.push(LaunchUnit {
            id,
            kernel_index,
        });
        id
    }

    /// Map a signature's storage buffers onto program resources with the
    /// caller's buffer facts (in signature order).
    fn resources_from_signature(
        &self,
        signature: &MirKernelSignature,
        facts: &[BufferFacts],
    ) -> Vec<DeviceResource> {
        signature
            .resources()
            .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
            .zip(facts.iter())
            .map(|(resource, facts)| DeviceResource {
                buffer: facts.identity(),
                version: BufferVersion {
                    version: 1,
                    element_ty: resource.element_ty,
                    element_count: resource.element_count,
                    // The carried reduced-resource projection (council-8
                    // CB-3) rides the wire version from the signature
                    // resource — never reconstructed from element count.
                    reduced_projection: resource.reduced_projection,
                },
                binding: Binding {
                    group: resource.group,
                    binding: resource.binding,
                },
                access: resource.access,
            })
            .collect()
    }
}

/// The assembled prefill program (owned; the caller validates the MIR).
struct PrefillProgramArtifact {
    program: DeviceProgram,
    semantics: DeviceSemantics,
    mir: PrefillMirProgram,
    logits_buffer_id: u32,
}

/// Build the full prefill MIR program + device program for the pinned row.
fn build_prefill_program() -> Result<PrefillProgramArtifact, Vec<Diagnostic>> {
    let mut types = TypeTable::new();
    let mut functions: Vec<MirFunction> = Vec::new();
    let mut next_function = 0u32;
    let intern = |types: &mut TypeTable,
                      functions: &mut Vec<MirFunction>,
                      next: &mut u32,
                      build: fn(&mut TypeTable, u32) -> MirFunction|
     -> MirFunctionId {
        let function = build(types, *next);
        *next += 1;
        let id = function.id;
        functions.push(function);
        id
    };

    // ---- Phase 1: intern every distinct function. ----
    let gather_fn = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        gather_function(types, id, PROMPT_TOKEN_COUNT)
    });
    let rms_norm_fn = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        rms_norm_function(types, id, [PROMPT_TOKEN_COUNT, HIDDEN_SIZE])
    });
    let rope_fn = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        // Per-position rotation over the 64-wide per-head rows (the
        // per-head attention fan-out; see `rope_tables`).
        rope_function(types, id, [PROMPT_TOKEN_COUNT, HEAD_DIM], 8, HEAD_DIM as i64)
    });
    let causal_fn = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        causal_softmax_function(types, id, [PROMPT_TOKEN_COUNT, PROMPT_TOKEN_COUNT])
    });
    let transpose_k_fn = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        transpose_function(types, id, [PROMPT_TOKEN_COUNT, HEAD_DIM])
    });
    let transpose_embd_fn =
        intern(&mut types, &mut functions, &mut next_function, |types, id| {
            transpose_function(types, id, [VOCAB_SIZE, HIDDEN_SIZE])
        });
    let mm_qkv = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        matmul_function(types, id, PROMPT_TOKEN_COUNT, HIDDEN_SIZE, HEAD_DIM)
    });
    let mm_scores = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        matmul_function(types, id, PROMPT_TOKEN_COUNT, HEAD_DIM, PROMPT_TOKEN_COUNT)
    });
    let mm_ctx = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        matmul_function(types, id, PROMPT_TOKEN_COUNT, PROMPT_TOKEN_COUNT, HEAD_DIM)
    });
    let mm_out = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        matmul_function(types, id, PROMPT_TOKEN_COUNT, HEAD_DIM, HIDDEN_SIZE)
    });
    let mm_2560 = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        matmul_function(types, id, PROMPT_TOKEN_COUNT, HIDDEN_SIZE, FFN_SIZE)
    });
    let mm_down = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        matmul_function(types, id, PROMPT_TOKEN_COUNT, FFN_SIZE, HIDDEN_SIZE)
    });
    let mm_tied = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        matmul_function(types, id, PROMPT_TOKEN_COUNT, HIDDEN_SIZE, VOCAB_SIZE)
    });
    let swiglu_neg = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        elementwise_unary_function(types, id, &[PROMPT_TOKEN_COUNT, FFN_SIZE], ElementwiseUnary::Neg)
    });
    let swiglu_exp = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        elementwise_unary_function(types, id, &[PROMPT_TOKEN_COUNT, FFN_SIZE], ElementwiseUnary::Exp)
    });
    // The device K/V f16 rounding unary (the GI3-1 amendment — the
    // comparator's `kv_f16_round` register rounding, applied post-RoPE to K
    // and directly to V; Q stays f32 per the oracle).
    let f16_round_fn = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        elementwise_unary_function(types, id, &[PROMPT_TOKEN_COUNT, HEAD_DIM], ElementwiseUnary::F16Round)
    });
    let swiglu_add1 = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        elementwise_scalar_function(
            types,
            id,
            &[PROMPT_TOKEN_COUNT, FFN_SIZE],
            ElementwiseScalarOp::Add,
            1.0,
        )
    });
    let swiglu_div = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        elementwise_scalar_function(
            types,
            id,
            &[PROMPT_TOKEN_COUNT, FFN_SIZE],
            ElementwiseScalarOp::Div,
            1.0,
        )
    });
    let swiglu_silu = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        elementwise_binary_function(
            types,
            id,
            &[PROMPT_TOKEN_COUNT, FFN_SIZE],
            radix_mir::MirCollectionOp::TensorMul,
        )
    });
    let swiglu_up = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        elementwise_binary_function(
            types,
            id,
            &[PROMPT_TOKEN_COUNT, FFN_SIZE],
            radix_mir::MirCollectionOp::TensorMul,
        )
    });
    let scale_scores = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        elementwise_scalar_function(
            types,
            id,
            &[PROMPT_TOKEN_COUNT, PROMPT_TOKEN_COUNT],
            ElementwiseScalarOp::Mul,
            ATTENTION_SCALE,
        )
    });
    let residual_add = intern(&mut types, &mut functions, &mut next_function, |types, id| {
        elementwise_binary_function(
            types,
            id,
            &[PROMPT_TOKEN_COUNT, HIDDEN_SIZE],
            radix_mir::MirCollectionOp::TensorAdd,
        )
    });

    let mir = PrefillMirProgram {
        functions: MirProgram { functions },
        types,
    };
    let validation = MirValidationContext::new(&mir.types);
    let validated = ValidatedMir::new(mir.functions.clone(), validation.clone())
        .map_err(|errors| {
            errors
                .into_iter()
                .map(|error| {
                    Diagnostic::error(format!(
                        "prefill device program: constructed MIR failed validation: {}",
                        error.message
                    ))
                })
                .collect::<Vec<_>>()
        })?;
    let _ = &validated;

    // ---- Phase 2: assemble the kernel graph. ----
    let mut assembler = KernelAssembler::new();
    assembler.register_input("token_embd.weight");
    assembler.register_input("prompt_tokens");
    for il in 0..LAYER_COUNT as usize {
        assembler.register_input(&format!("blk.{il}.attn_norm.weight"));
        for hq in 0..HEAD_COUNT as usize {
            assembler.register_input(&format!("blk.{il}.attn_q.h{hq}.weight"));
            assembler.register_input(&format!("blk.{il}.attn_output.h{hq}.weight"));
        }
        for g in 0..KV_HEAD_COUNT as usize {
            assembler.register_input(&format!("blk.{il}.attn_k.g{g}.weight"));
            assembler.register_input(&format!("blk.{il}.attn_v.g{g}.weight"));
        }
        assembler.register_input(&format!("blk.{il}.ffn_norm.weight"));
        assembler.register_input(&format!("blk.{il}.ffn_gate.weight"));
        assembler.register_input(&format!("blk.{il}.ffn_up.weight"));
        assembler.register_input(&format!("blk.{il}.ffn_down.weight"));
    }
    assembler.register_input("output_norm.weight");

    // The kernel graph follows the CPU oracle's forward path exactly: the
    // per-head attention fan-out (each query head's score row is scaled and
    // softmaxed independently — the oracle's `attention_gqa` softmaxes per
    // query head — then the 15 head outputs fold back into the `[9, 960]`
    // context).
    let mut h = push_gather_kernel(&mut assembler, &mir, &validation, gather_fn)?;
    for il in 0..LAYER_COUNT as usize {
        let entry = |base: &str| format!("prefill_blk_{il}_{base}");
        let attn_norm_w = assembler.input_facts(&format!("blk.{il}.attn_norm.weight"));
        let a = push_rms_norm_kernel(&mut assembler, &mir, &validation, rms_norm_fn, h.clone(), attn_norm_w, &format!("prefill.blk{il}.a"), &entry("attn_norm"))?;
        // Per-query-head Q projections + per-position RoPE (Q stays f32).
        let mut q_rot = Vec::with_capacity(HEAD_COUNT as usize);
        for hq in 0..HEAD_COUNT as usize {
            let attn_q_w = assembler.input_facts(&format!("blk.{il}.attn_q.h{hq}.weight"));
            let q = push_matmul_kernel(&mut assembler, &mir, &validation, mm_qkv, a.clone(), attn_q_w, &format!("prefill.blk{il}.q{hq}"), &entry(&format!("attn_q_h{hq}")))?;
            q_rot.push(push_rope_kernel(&mut assembler, &mir, &validation, rope_fn, q, &format!("prefill.blk{il}.qr{hq}"), &entry(&format!("rope_q_h{hq}")))?);
        }
        // Per-KV-head K/V projections; K gets RoPE then the f16 register
        // rounding (the GI3-1 amendment — the comparator's `kv_f16_round`,
        // K after RoPE, V directly) then transposes; V gets the rounding
        // directly.
        let mut k_t = Vec::with_capacity(KV_HEAD_COUNT as usize);
        let mut v_h16 = Vec::with_capacity(KV_HEAD_COUNT as usize);
        for g in 0..KV_HEAD_COUNT as usize {
            let attn_k_w = assembler.input_facts(&format!("blk.{il}.attn_k.g{g}.weight"));
            let kk = push_matmul_kernel(&mut assembler, &mir, &validation, mm_qkv, a.clone(), attn_k_w, &format!("prefill.blk{il}.k{g}"), &entry(&format!("attn_k_g{g}")))?;
            let kr = push_rope_kernel(&mut assembler, &mir, &validation, rope_fn, kk, &format!("prefill.blk{il}.kr{g}"), &entry(&format!("rope_k_g{g}")))?;
            let kr_h16 = push_elementwise_scalar_kernel(&mut assembler, &mir, &validation, f16_round_fn, kr, &format!("prefill.blk{il}.kr{g}_h16"), &entry(&format!("kv_f16_round_k_g{g}")))?;
            k_t.push(push_transpose_kernel(&mut assembler, &mir, &validation, transpose_k_fn, kr_h16, &format!("prefill.blk{il}.kt{g}"), &entry(&format!("transpose_k_g{g}")))?);
            let attn_v_w = assembler.input_facts(&format!("blk.{il}.attn_v.g{g}.weight"));
            let vv = push_matmul_kernel(&mut assembler, &mir, &validation, mm_qkv, a.clone(), attn_v_w, &format!("prefill.blk{il}.v{g}"), &entry(&format!("attn_v_g{g}")))?;
            v_h16.push(push_elementwise_scalar_kernel(&mut assembler, &mir, &validation, f16_round_fn, vv, &format!("prefill.blk{il}.v{g}_h16"), &entry(&format!("kv_f16_round_v_g{g}")))?);
        }
        // Per-query-head causal attention + output projection; fold the 15
        // head outputs into the `[9, 960]` context.
        let mut o = None;
        for (hq, qr) in q_rot.iter().enumerate() {
            let g = hq / QUERY_HEADS_PER_KV as usize;
            let scores = push_matmul_kernel(&mut assembler, &mir, &validation, mm_scores, qr.clone(), k_t[g].clone(), &format!("prefill.blk{il}.scores{hq}"), &entry(&format!("scores_h{hq}")))?;
            let scaled = push_elementwise_scalar_kernel(&mut assembler, &mir, &validation, scale_scores, scores, &format!("prefill.blk{il}.scaled{hq}"), &entry(&format!("scale_scores_h{hq}")))?;
            let probs = push_causal_softmax_kernel(&mut assembler, &mir, &validation, causal_fn, scaled, &format!("prefill.blk{il}.probs{hq}"), &entry(&format!("causal_softmax_h{hq}")))?;
            let ctx = push_matmul_kernel(&mut assembler, &mir, &validation, mm_ctx, probs, v_h16[g].clone(), &format!("prefill.blk{il}.ctx{hq}"), &entry(&format!("context_h{hq}")))?;
            let attn_out_w = assembler.input_facts(&format!("blk.{il}.attn_output.h{hq}.weight"));
            let oh = push_matmul_kernel(&mut assembler, &mir, &validation, mm_out, ctx, attn_out_w, &format!("prefill.blk{il}.o{hq}"), &entry(&format!("attn_output_h{hq}")))?;
            o = Some(match o {
                None => oh,
                Some(acc) => push_elementwise_binary_kernel(&mut assembler, &mir, &validation, residual_add, acc, oh, &format!("prefill.blk{il}.o"), &entry(&format!("sum_o_h{hq}")))?,
            });
        }
        // Static invariant (safe by construction): `HEAD_COUNT` is the pinned
        // const 15 > 0, so the query-head fold loop always yields `Some`.
        let o = o.expect("at least one query head");
        let h2 = push_elementwise_binary_kernel(&mut assembler, &mir, &validation, residual_add, h, o, &format!("prefill.blk{il}.h2"), &entry("residual_attn"))?;
        let ffn_norm_w = assembler.input_facts(&format!("blk.{il}.ffn_norm.weight"));
        let f = push_rms_norm_kernel(&mut assembler, &mir, &validation, rms_norm_fn, h2.clone(), ffn_norm_w, &format!("prefill.blk{il}.f"), &entry("ffn_norm"))?;
        let ffn_gate_w = assembler.input_facts(&format!("blk.{il}.ffn_gate.weight"));
        let gate = push_matmul_kernel(&mut assembler, &mir, &validation, mm_2560, f.clone(), ffn_gate_w, &format!("prefill.blk{il}.gate"), &entry("ffn_gate"))?;
        let ffn_up_w = assembler.input_facts(&format!("blk.{il}.ffn_up.weight"));
        let up = push_matmul_kernel(&mut assembler, &mir, &validation, mm_2560, f, ffn_up_w, &format!("prefill.blk{il}.up"), &entry("ffn_up"))?;
        let hh = push_swiglu_kernels(
            &mut assembler, &mir, &validation,
            swiglu_neg, swiglu_exp, swiglu_add1, swiglu_div, swiglu_silu, swiglu_up,
            gate, up,
            &format!("prefill.blk{il}.hh"), &entry,
        )?;
        let ffn_down_w = assembler.input_facts(&format!("blk.{il}.ffn_down.weight"));
        let down = push_matmul_kernel(&mut assembler, &mir, &validation, mm_down, hh, ffn_down_w, &format!("prefill.blk{il}.down"), &entry("ffn_down"))?;
        h = push_elementwise_binary_kernel(&mut assembler, &mir, &validation, residual_add, h2, down, &format!("prefill.blk{il}.h"), &entry("residual_ffn"))?;
    }

    let output_norm_w = assembler.input_facts("output_norm.weight");
    let hn = push_rms_norm_kernel(&mut assembler, &mir, &validation, rms_norm_fn, h, output_norm_w, "prefill.hn", "prefill_output_norm")?;
    let embd = assembler.input_facts("token_embd.weight");
    let embd_t = push_transpose_kernel(&mut assembler, &mir, &validation, transpose_embd_fn, embd, "prefill.token_embd_t", "prefill_transpose_token_embd")?;
    let logits = push_logits_kernel(&mut assembler, &mir, &validation, mm_tied, hn, embd_t)?;

    let program = DeviceProgram {
        kernels: assembler.kernels,
        launches: assembler.launches,
        lifetime: DeviceProgramLifetime::SingleRun,
        results: assembler.results,
    };
    let semantics = build_semantics(&program);
    Ok(PrefillProgramArtifact {
        program,
        semantics,
        mir,
        logits_buffer_id: logits.id,
    })
}

// ---------------------------------------------------------------------------
// Per-recipe kernel pushers
// ---------------------------------------------------------------------------

/// The signature a transformer-recipe kernel is emitted under: the whole ABI
/// when the ABI can express the function (rms-norm/rope/causal — the S6-P2
/// seam), the transformer shape signature for the gather (the ids vector is
/// an array the tensor-only ABI cannot express).
#[derive(Debug, Clone)]
enum KernelSignature {
    Whole(MirKernelSignature),
    Transformer(MirKernelSignature),
}

impl KernelSignature {
    fn resources(&self) -> &MirKernelSignature {
        match self {
            Self::Whole(signature) | Self::Transformer(signature) => signature,
        }
    }
}

/// Derive the signature + plan for one kernel from its function's typed
/// facts. `transformer_op` is `Some` for the GI3 transformer recipes (no
/// `CollectionOpContract` variant — resolved via the
/// `from_transformer_op`-sibling admission path); `gather` uses the
/// transformer shape signature for the resources too.
#[allow(clippy::too_many_arguments)]
fn kernel_signature_and_plan(
    mir: &PrefillMirProgram,
    validation: &MirValidationContext<'_>,
    function: MirFunctionId,
    transformer_op: Option<radix_mir::MirCollectionOp>,
    gather_seam: bool,
) -> Result<(KernelSignature, CollectionKernelPlan), Vec<Diagnostic>> {
    let function_ref = mir
        .functions
        .functions
        .iter()
        .find(|candidate| candidate.id == function)
        .ok_or_else(|| vec![Diagnostic::error("prefill function disappeared")])?;
    if let Some(op) = transformer_op {
        let shape = transformer_shape_signature(function_ref, op, validation).map_err(|error| {
            vec![Diagnostic::error(format!(
                "prefill device program: transformer shape signature failed: {error}"
            ))]
        })?;
        let plan = CollectionKernelPlan::from_transformer_op(&shape, function_ref, validation, op)
            .ok_or_else(|| {
                vec![Diagnostic::error(format!(
                    "prefill device program: transformer recipe {op:?} has no resolvable plan"
                ))]
            })?;
        if gather_seam {
            return Ok((KernelSignature::Transformer(shape), plan));
        }
        // The whole ABI signature is the resource/launch authority when the
        // ABI can express the function (the rms-norm/rope/causal seam).
        let whole =
            MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
                function_ref,
                validation,
                &Interner::new(),
            )
            .map_err(|error| {
                vec![Diagnostic::error(format!(
                    "prefill device program: whole ABI signature failed: {}",
                    error.message
                ))]
            })?;
        return Ok((KernelSignature::Whole(whole), plan));
    }
    // Recipe-op kernels (matmul / transpose) and elementwise-only bodies use
    // the whole ABI signature + the shared plan pass.
    let whole = MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
        function_ref,
        validation,
        &Interner::new(),
    )
    .map_err(|error| {
        vec![Diagnostic::error(format!(
            "prefill device program: whole ABI signature failed: {}",
            error.message
        ))]
    })?;
    let plan = kernel_plan_for_function(function_ref, &whole, validation)
        .map_err(|error| {
            vec![Diagnostic::error(format!(
                "prefill device program: plan derivation failed: {error}"
            ))]
        })?
        .unwrap_or(CollectionKernelPlan::Elementwise);
    Ok((KernelSignature::Whole(whole), plan))
}

#[allow(clippy::too_many_arguments)]
fn push_kernel(
    assembler: &mut KernelAssembler,
    mir: &PrefillMirProgram,
    validation: &MirValidationContext<'_>,
    function: MirFunctionId,
    entry: &str,
    facts: &[BufferFacts],
    transformer_op: Option<radix_mir::MirCollectionOp>,
    gather_seam: bool,
) -> Result<BufferFacts, Vec<Diagnostic>> {
    let (signature, plan) = kernel_signature_and_plan(mir, validation, function, transformer_op, gather_seam)?;
    let signature_ref = signature.resources();
    let resources = assembler.resources_from_signature(signature_ref, facts);
    let function_ref = mir
        .functions
        .functions
        .iter()
        .find(|candidate| candidate.id == function)
        .ok_or_else(|| vec![Diagnostic::error("prefill function disappeared")])?;
    let launch = KernelLaunchPlan::from_signature_and_function(signature_ref, function_ref);
    assembler.push_kernel(function, entry.to_owned(), plan, resources, launch);
    // The kernel's output buffer is the last fact; every caller appends a
    // fresh output buffer, but an internal facts-list mismatch is worth a
    // fail-closed diagnostic rather than a panic.
    facts.last().cloned().ok_or_else(|| {
        vec![Diagnostic::error(format!(
            "prefill device program: kernel `{entry}` has no output buffer in its facts"
        ))]
    })
}

/// The embedding gather kernel.
fn push_gather_kernel(
    assembler: &mut KernelAssembler,
    mir: &PrefillMirProgram,
    validation: &MirValidationContext<'_>,
    function: MirFunctionId,
) -> Result<BufferFacts, Vec<Diagnostic>> {
    let table = assembler.input_facts("token_embd.weight");
    let ids = assembler.input_facts("prompt_tokens");
    let out = assembler.fresh_activation("prefill.h");
    let facts = vec![table, ids, out];
    push_kernel(
        assembler,
        mir,
        validation,
        function,
        "prefill_gather",
        &facts,
        Some(radix_mir::MirCollectionOp::TensorGather),
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn push_rms_norm_kernel(
    assembler: &mut KernelAssembler,
    mir: &PrefillMirProgram,
    validation: &MirValidationContext<'_>,
    function: MirFunctionId,
    input: BufferFacts,
    gamma: BufferFacts,
    out_name: &str,
    entry: &str,
) -> Result<BufferFacts, Vec<Diagnostic>> {
    let out = assembler.fresh_activation(out_name);
    let facts = vec![input, gamma, out];
    push_kernel(
        assembler,
        mir,
        validation,
        function,
        entry,
        &facts,
        Some(radix_mir::MirCollectionOp::TensorRmsNorm),
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn push_matmul_kernel(
    assembler: &mut KernelAssembler,
    mir: &PrefillMirProgram,
    validation: &MirValidationContext<'_>,
    function: MirFunctionId,
    left: BufferFacts,
    right: BufferFacts,
    out_name: &str,
    entry: &str,
) -> Result<BufferFacts, Vec<Diagnostic>> {
    let out = assembler.fresh_activation(out_name);
    let facts = vec![left, right, out];
    push_kernel(
        assembler,
        mir,
        validation,
        function,
        entry,
        &facts,
        None,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn push_rope_kernel(
    assembler: &mut KernelAssembler,
    mir: &PrefillMirProgram,
    validation: &MirValidationContext<'_>,
    function: MirFunctionId,
    input: BufferFacts,
    out_name: &str,
    entry: &str,
) -> Result<BufferFacts, Vec<Diagnostic>> {
    let out = assembler.fresh_activation(out_name);
    let (cos, sin) = rope_table_buffers(assembler);
    let facts = vec![input, cos, sin, out];
    push_kernel(
        assembler,
        mir,
        validation,
        function,
        entry,
        &facts,
        Some(radix_mir::MirCollectionOp::TensorRope),
        false,
    )
}

/// The RoPE cos/sin table buffers (PerProgram inputs): the rank-2
/// `[rows, dim/2]` per-position tables, in order `[cos, sin]` — the
/// host-precomputed angle tables each row rotates at (see [`rope_tables`]).
fn rope_table_buffers(assembler: &mut KernelAssembler) -> (BufferFacts, BufferFacts) {
    let _ = assembler.register_input("prefill.rope.cos");
    let _ = assembler.register_input("prefill.rope.sin");
    (
        assembler.input_facts("prefill.rope.cos"),
        assembler.input_facts("prefill.rope.sin"),
    )
}

#[allow(clippy::too_many_arguments)]
fn push_transpose_kernel(
    assembler: &mut KernelAssembler,
    mir: &PrefillMirProgram,
    validation: &MirValidationContext<'_>,
    function: MirFunctionId,
    input: BufferFacts,
    out_name: &str,
    entry: &str,
) -> Result<BufferFacts, Vec<Diagnostic>> {
    let out = assembler.fresh_activation(out_name);
    let facts = vec![input, out];
    push_kernel(
        assembler,
        mir,
        validation,
        function,
        entry,
        &facts,
        None,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn push_causal_softmax_kernel(
    assembler: &mut KernelAssembler,
    mir: &PrefillMirProgram,
    validation: &MirValidationContext<'_>,
    function: MirFunctionId,
    input: BufferFacts,
    out_name: &str,
    entry: &str,
) -> Result<BufferFacts, Vec<Diagnostic>> {
    let out = assembler.fresh_activation(out_name);
    let facts = vec![input, out];
    push_kernel(
        assembler,
        mir,
        validation,
        function,
        entry,
        &facts,
        Some(radix_mir::MirCollectionOp::TensorCausalMaskedSoftmax),
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn push_elementwise_scalar_kernel(
    assembler: &mut KernelAssembler,
    mir: &PrefillMirProgram,
    validation: &MirValidationContext<'_>,
    function: MirFunctionId,
    input: BufferFacts,
    out_name: &str,
    entry: &str,
) -> Result<BufferFacts, Vec<Diagnostic>> {
    let out = assembler.fresh_activation(out_name);
    let facts = vec![input, out];
    push_kernel(
        assembler,
        mir,
        validation,
        function,
        entry,
        &facts,
        None,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn push_elementwise_binary_kernel(
    assembler: &mut KernelAssembler,
    mir: &PrefillMirProgram,
    validation: &MirValidationContext<'_>,
    function: MirFunctionId,
    left: BufferFacts,
    right: BufferFacts,
    out_name: &str,
    entry: &str,
) -> Result<BufferFacts, Vec<Diagnostic>> {
    let out = assembler.fresh_activation(out_name);
    let facts = vec![left, right, out];
    push_kernel(
        assembler,
        mir,
        validation,
        function,
        entry,
        &facts,
        None,
        false,
    )
}

/// The tied-head projection kernel: `logits[9, 49152] = hn[9, 960] ·
/// token_embd_t[960, 49152]`. The logits buffer is the declared
/// ObservationPoint result — read back at the final launch's completion
/// boundary (the prompt-final full-vocab logits observation).
#[allow(clippy::too_many_arguments)]
fn push_logits_kernel(
    assembler: &mut KernelAssembler,
    mir: &PrefillMirProgram,
    validation: &MirValidationContext<'_>,
    function: MirFunctionId,
    left: BufferFacts,
    right: BufferFacts,
) -> Result<BufferFacts, Vec<Diagnostic>> {
    let logits_id = assembler.next_buffer;
    assembler.next_buffer += 1;
    let out = BufferFacts {
        id: logits_id,
        name: "prefill.logits".to_owned(),
        role: BufferRole::Output,
        lifetime: BufferLifetime::ObservationPoint,
    };
    let facts = vec![left, right, out.clone()];
    let (signature, plan) = kernel_signature_and_plan(mir, validation, function, None, false)?;
    let signature_ref = signature.resources();
    let resources = assembler.resources_from_signature(signature_ref, &facts);
    let function_ref = mir
        .functions
        .functions
        .iter()
        .find(|candidate| candidate.id == function)
        .ok_or_else(|| vec![Diagnostic::error("prefill function disappeared")])?;
    let launch = KernelLaunchPlan::from_signature_and_function(signature_ref, function_ref);
    let produced_by =
        assembler.push_kernel(function, "prefill_tied_head".to_owned(), plan, resources, launch);
    assembler.results.push(ResultBuffer {
        buffer: out.identity(),
        version: BufferVersion {
            version: 1,
            element_ty: signature_ref.output.element_ty,
            element_count: signature_ref.output.element_count,
            // The carried reduced-resource projection (council-8 CB-3)
            // rides the result version from the output resource.
            reduced_projection: signature_ref.output.reduced_projection,
        },
        role: BufferRole::Output,
        produced_by,
        cadence: ObservationCadence::PerStep,
    });
    Ok(out)
}

/// The SwiGLU composition kernels: `silu(gate)·up` where
/// `silu(gate) = gate/(1+exp(−gate))` — the GI3-1 verified elementwise
/// composition (no dedicated activation recipe).
#[allow(clippy::too_many_arguments)]
fn push_swiglu_kernels(
    assembler: &mut KernelAssembler,
    mir: &PrefillMirProgram,
    validation: &MirValidationContext<'_>,
    neg_fn: MirFunctionId,
    exp_fn: MirFunctionId,
    add1_fn: MirFunctionId,
    div_fn: MirFunctionId,
    silu_fn: MirFunctionId,
    up_fn: MirFunctionId,
    gate: BufferFacts,
    up: BufferFacts,
    out_name: &str,
    entry: &dyn Fn(&str) -> String,
) -> Result<BufferFacts, Vec<Diagnostic>> {
    let neg = push_elementwise_scalar_kernel(assembler, mir, validation, neg_fn, gate.clone(), &format!("{out_name}.neg"), &entry("swiglu_neg"))?;
    let exp = push_elementwise_scalar_kernel(assembler, mir, validation, exp_fn, neg, &format!("{out_name}.exp"), &entry("swiglu_exp"))?;
    let add1 = push_elementwise_scalar_kernel(assembler, mir, validation, add1_fn, exp, &format!("{out_name}.add1"), &entry("swiglu_add1"))?;
    let div = push_elementwise_scalar_kernel(assembler, mir, validation, div_fn, add1, &format!("{out_name}.div"), &entry("swiglu_div"))?;
    let silu = push_elementwise_binary_kernel(assembler, mir, validation, silu_fn, gate, div, &format!("{out_name}.silu"), &entry("swiglu_silu"))?;
    push_elementwise_binary_kernel(assembler, mir, validation, up_fn, silu, up, out_name, &entry("swiglu_up"))
}

// ---------------------------------------------------------------------------
// Semantics + section assembly
// ---------------------------------------------------------------------------

/// Build the carried semantic facts (F1–F6) of the prefill program: every
/// launch is scheduled from the chained activation flows; the weights/prompt
/// are read-only inputs (their reads consume the initial host-provided state
/// — generation 1 via the empty value table). The initialization axis
/// defaults to `KernelInitialized` (the weights are Input-role buffers copied
/// per execution).
fn build_semantics(program: &DeviceProgram) -> DeviceSemantics {
    // Dependencies: per buffer version, the producing launch → every later
    // consuming launch (a chained graph — no cross-buffer fan-in here beyond
    // the two-input kernels, which each produce one output).
    let mut edges: Vec<(LaunchId, LaunchId, u32, u32)> = Vec::new();
    for (index, launch) in program.launches.iter().enumerate() {
        let kernel = &program.kernels[launch.kernel_index];
        for resource in &kernel.resources {
            if resource.access == MirKernelResourceAccess::Read {
                // Find the latest earlier launch that writes this buffer
                // version.
                if let Some(producer) = program
                    .launches
                    .iter()
                    .enumerate()
                    .take(index)
                    .rev()
                    .find(|(_, candidate)| {
                        let candidate_kernel = &program.kernels[candidate.kernel_index];
                        candidate_kernel.resources.iter().any(|slot| {
                            slot.buffer.id == resource.buffer.id
                                && slot.version.version == resource.version.version
                                && slot.access != MirKernelResourceAccess::Read
                        })
                    })
                {
                    edges.push((
                        producer.1.id,
                        launch.id,
                        resource.buffer.id.0,
                        resource.version.version,
                    ));
                }
            }
        }
    }
    let dependencies: Vec<DependencyEdge> = edges
        .into_iter()
        .map(|(producer, consumer, buffer, version)| DependencyEdge {
            producer,
            consumer,
            buffer: radix_mir::device_program::BufferId(buffer),
            version,
        })
        .collect();
    let roots: Vec<LaunchId> = program
        .launches
        .iter()
        .filter(|launch| !dependencies.iter().any(|edge| edge.consumer == launch.id))
        .map(|launch| launch.id)
        .collect();
    DeviceSemantics {
        values: Vec::new(),
        bindings: Vec::new(),
        generations: Vec::new(),
        initializations: Vec::new(),
        observations: Vec::new(),
        roots,
        dependencies,
        relations: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Section assembly + the device run
// ---------------------------------------------------------------------------

/// The host-precomputed RoPE angle tables for **per-position** rotation over
/// `dim`-wide rows (the per-head 64-wide activations). The tables are
/// `[rows, dim/2]`: row `i` carries the angles for position `positions[i]`,
/// `theta[p] = pos · freq_base^(−2p/dim)` — the same angles the CPU oracle's
/// `rope_all_heads` applies per head.
fn rope_tables(positions: &[u64], dim: u64, freq_base: f64) -> (Vec<f32>, Vec<f32>) {
    let pairs = (dim / 2) as usize;
    let mut cos = Vec::with_capacity(positions.len() * pairs);
    let mut sin = Vec::with_capacity(positions.len() * pairs);
    for &pos in positions {
        for pair in 0..pairs {
            let theta = pos as f64 * freq_base.powf(-(2.0 * pair as f64) / dim as f64);
            cos.push(theta.cos() as f32);
            sin.push(theta.sin() as f32);
        }
    }
    (cos, sin)
}

/// Assemble the prefill wire section from the repacked weights: the MIR
/// program is validated, the Metal artifact is emitted, and the runtime
/// weight values ride the section's declared inputs (never an FMIR-image
/// literal — the section exists only in memory for this run).
fn build_prefill_section(
    weights: &PrefillWeights,
    backend: DeviceBackend,
) -> Result<(FmirDeviceSection, u32), Vec<Diagnostic>> {
    let artifact = build_prefill_program()?;
    let validation = MirValidationContext::new(&artifact.mir.types);
    let validated = ValidatedMir::new(artifact.mir.functions.clone(), validation.clone())
        .map_err(|errors| {
            errors
                .into_iter()
                .map(|error| {
                    Diagnostic::error(format!(
                        "prefill device program: constructed MIR failed validation: {}",
                        error.message
                    ))
                })
                .collect::<Vec<_>>()
        })?;
    let mut inputs = weights.declared_inputs();
    inputs.insert("prompt_tokens".to_owned(), prompt_ids_values());
    let (cos, sin) = rope_tables(&[0, 1, 2, 3, 4, 5, 6, 7, 8], HEAD_DIM, 100_000.0);
    inputs.insert("prefill.rope.cos".to_owned(), cos);
    inputs.insert("prefill.rope.sin".to_owned(), sin);
    let selection = match backend {
        DeviceBackend::Metal => DeviceSelection::Metal,
        DeviceBackend::Cuda => DeviceSelection::Cuda,
    };
    let section = device_section_for_program(
        &artifact.program,
        &artifact.semantics,
        &validated,
        &Interner::new(),
        DeviceSectionBuild {
            selection,
            inputs: &inputs,
            ptx_target: "sm_90",
            repeating_steps: 0,
        },
    )?;
    Ok((section, artifact.logits_buffer_id))
}

/// Execute the prefill `SingleRun` session on the device route and return
/// the execution receipt (the logits observation + the S6/S11 facts).
fn execute_prefill_session(
    device: &FmirDeviceSection,
    backend: DeviceBackend,
) -> Result<faber_host_macos_arm64::composite_host::DeviceExecutionReceipt, Vec<Diagnostic>> {
    admit_device_program_section(&device.device_program)?;
    let artifact = artifact_for_backend(&device.artifacts.artifact, backend).ok_or_else(|| {
        vec![super::super::host_factory::missing_backend_artifact(backend)]
    })?;
    let descriptor = descriptor_for_backend(device, backend, artifact.blob.as_bytes())?;
    let selection = match backend {
        DeviceBackend::Metal => DeviceSelection::Metal,
        DeviceBackend::Cuda => DeviceSelection::Cuda,
    };
    let mut host = super::super::host_factory::construct_composite_host(selection, true)
        .map_err(|diagnostic| vec![diagnostic])?;
    let inputs = inputs_by_buffer_id(device);
    let mut session = super::super::host_factory::create_program_session(&mut host, &descriptor)
        .map_err(|diagnostic| vec![diagnostic])?;
    let receipt = session
        .execute(&inputs)
        .map_err(|error| vec![super::super::host_factory::host_error_diagnostic(&error)])?;
    session
        .teardown()
        .map_err(|error| vec![super::super::host_factory::host_error_diagnostic(&error)])?;
    Ok(receipt)
}

/// Load the committed GI2-3 logits golden's raw prompt-final logits
/// (`testdata/gi2-3-logits-golden/logits-pos0.json` `raw_logits.f32_le_hex`).
fn load_golden_logits(golden_dir: &Path) -> Result<Vec<f32>, Vec<Diagnostic>> {
    let path = golden_dir.join("logits-pos0.json");
    let wire = std::fs::read_to_string(&path).map_err(|error| {
        vec![Diagnostic::error(format!(
            "prefill device run: cannot read the GI2-3 golden `{}`: {error}",
            path.display()
        ))]
    })?;
    let root = Json::parse(&wire)
        .map_err(|error| {
            vec![Diagnostic::error(format!(
                "prefill device run: the GI2-3 golden does not parse: {error}"
            ))]
        })?
        .as_valor()
        .clone();
    let raw = field(&root, "raw_logits")?;
    let raw = field(raw, "f32_le_hex")?;
    hex_f32s(text(raw)?)
}

/// The typed JSON-object field accessor. The golden is a user-supplied
/// fixture, so a non-object value or a missing key is a fail-closed
/// diagnostic, never a panic.
fn field<'a>(value: &'a Valor, key: &str) -> Result<&'a Valor, Vec<Diagnostic>> {
    let Valor::Tabula(fields) = value else {
        return Err(vec![Diagnostic::error(format!(
            "prefill device run: expected a JSON object field {key:?}"
        ))]);
    };
    fields.get(key).ok_or_else(|| {
        vec![Diagnostic::error(format!(
            "prefill device run: missing JSON field {key:?}"
        ))]
    })
}

fn text(value: &Valor) -> Result<&str, Vec<Diagnostic>> {
    let Valor::Textus(s) = value else {
        return Err(vec![Diagnostic::error(
            "prefill device run: expected a JSON string",
        )]);
    };
    Ok(s)
}

/// Parse the golden's `f32_le_hex` byte stream into f32 values. Malformed
/// hex (odd length, non-hex digits, or a byte count that is not a multiple
/// of 4) is a fail-closed diagnostic, never a panic.
fn hex_f32s(hex: &str) -> Result<Vec<f32>, Vec<Diagnostic>> {
    if hex.len() % 2 != 0 {
        return Err(vec![Diagnostic::error(
            "prefill device run: the golden f32_le_hex byte stream has an odd length",
        )]);
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16).map_err(|error| {
                Diagnostic::error(format!(
                    "prefill device run: malformed golden f32_le_hex byte stream: {error}"
                ))
            })
        })
        .collect::<Result<Vec<u8>, Diagnostic>>()
        .map_err(|error| vec![error])?;
    if bytes.len() % 4 != 0 {
        return Err(vec![Diagnostic::error(format!(
            "prefill device run: the golden f32_le_hex byte stream is {} bytes, expected a multiple of 4",
            bytes.len()
        ))]);
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

/// The S6 shape-class label of the executed prefill.
fn shape_class() -> String {
    format!("prefill-{PROMPT_TOKEN_COUNT}t")
}

/// The S6 representation + algorithm facts of the declared f32 repack.
fn representation() -> String {
    "declared f32 conversion (GI2-1 dequant semantics; quantized tensors f16-rounded per the pinned comparator's Metal register arithmetic; K-major weights transposed to the device matmul row-major layout — attention weights split per head (q/o into the 15 query heads, k/v into the 5 KV heads) so the per-head attention softmax is expressible on the recipe surface; never direct GGUF quantized execution)".to_owned()
}

fn algorithm() -> String {
    "Gather / RmsNormalization / Rope (per-position, periodic per-head tables) / CausalMaskedSoftmax / declared-f32 TiledMatMul / SiLU elementwise composition / device K/V f16 rounding (kv_f16_round)".to_owned()
}

/// The workspace facts of the executed program.
fn workspace(program: &radix_mir_fmir::schema::WireDeviceProgram) -> String {
    let input_buffers = program
        .kernels
        .iter()
        .flat_map(|kernel| kernel.resources.iter())
        .filter(|resource| resource.buffer.role == WireBufferRole::Input)
        .count();
    format!(
        "{} kernels, {} launches, {} input-role slots (logits observation [9, 49152])",
        program.kernels.len(),
        program.launches.len(),
        input_buffers
    )
}

/// The full Q1-default prefill device run: repack → build → execute →
/// compare → record.
///
/// `evidence_dir` receives the committed comparison record + receipt. The
/// model and the golden are pinned paths/env overrides.
///
/// # Errors
/// Fail-closed diagnostics; never a silent CPU fallback and never a hidden
/// comparison substitute.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_prefill_device_route(
    model_path: &Path,
    golden_dir: &Path,
    evidence_dir: &Path,
    backend: DeviceBackend,
) -> Result<PrefillRunOutcome, Vec<Diagnostic>> {
    let repack_start = Instant::now();
    let (weights, receipts) = PrefillWeights::load(model_path)?;
    let repack_conversion_us = repack_start.elapsed().as_micros() as u64;

    let build_start = Instant::now();
    let (section, logits_buffer_id) = build_prefill_section(&weights, backend)?;
    let module_prep_us = build_start.elapsed().as_micros() as u64;

    let run_start = Instant::now();
    let host_receipt = execute_prefill_session(&section, backend)?;
    let first_invocation_us = run_start.elapsed().as_micros() as u64;

    // The prompt-final logits: the observed [9, 49152] row 8 (position 0 =
    // prompt end).
    let observed = host_receipt.outputs.get(&logits_buffer_id).ok_or_else(|| {
        vec![Diagnostic::error(format!(
            "prefill device run: the logits observation buffer (id {logits_buffer_id}) was not read back"
        ))]
    })?;
    if observed.len() != (PROMPT_TOKEN_COUNT * VOCAB_SIZE) as usize {
        return Err(vec![Diagnostic::error(format!(
            "prefill device run: the logits observation has {} elements, expected {}",
            observed.len(),
            PROMPT_TOKEN_COUNT * VOCAB_SIZE
        ))]);
    }
    let row = VOCAB_SIZE as usize;
    let prompt_final_logits = &observed[8 * row..9 * row];

    let golden = load_golden_logits(golden_dir)?;
    let comparison = compare_gpu_logits(prompt_final_logits, &golden).map_err(|error| {
        vec![Diagnostic::error(format!(
            "prefill device run: the Q2 comparison failed closed: {error}"
        ))]
    })?;

    // The receipt: S6 regime fields + execution facts + repack/timing.
    let receipt = PrefillReceipt {
        regime: ExecutableRegime::Prefill,
        transfers: u32::try_from(host_receipt.transfers).unwrap_or(u32::MAX),
        allocations: u32::try_from(host_receipt.allocated_buffers.len()).unwrap_or(u32::MAX),
        launches: u32::try_from(host_receipt.launches).unwrap_or(u32::MAX),
        syncs: u32::try_from(host_receipt.syncs).unwrap_or(u32::MAX),
        observations: u32::try_from(host_receipt.readbacks).unwrap_or(u32::MAX),
        regime_fields: PrefillRegimeFields {
            shape_class: shape_class(),
            representation: representation(),
            algorithm: algorithm(),
            workspace: workspace(&section.device_program.program),
            evidence: String::new(),
        },
        repack_conversion_us,
        module_prep_us,
        persistent_upload_us: 0,
        first_invocation_us,
        capture_us: 0,
    };

    let outcome = PrefillRunOutcome {
        comparison,
        receipt,
        receipts: receipts.len(),
        backend: backend.spelling().to_string(),
    };
    let mut evidence = evidence_dir.to_path_buf();
    evidence.push("gi3-prefill-comparison.json");
    let record = comparison_record_json(&outcome, model_path)?;
    std::fs::write(&evidence, record).map_err(|error| {
        vec![Diagnostic::error(format!(
            "prefill device run: cannot write the comparison record `{}`: {error}",
            evidence.display()
        ))]
    })?;
    Ok(outcome)
}

/// The committed comparison record (schema `gi3-prefill-comparison-v1`).
fn comparison_record_json(
    outcome: &PrefillRunOutcome,
    model_path: &Path,
) -> Result<String, Vec<Diagnostic>> {
    let mut root = StdBTreeMap::new();
    root.insert("schema".to_owned(), Valor::from("gi3-prefill-comparison-v1"));
    root.insert(
        "device_run".to_owned(),
        Valor::from("completed"),
    );
    root.insert(
        "backend".to_owned(),
        Valor::from(outcome.backend.clone()),
    );
    root.insert(
        "model_path".to_owned(),
        Valor::from(model_path.display().to_string()),
    );
    let mut q2 = StdBTreeMap::new();
    q2.insert("schema".to_owned(), Valor::from("faber-runtime/src/prefill.rs compare_gpu_logits"));
    q2.insert("top1_matches".to_owned(), Valor::from(outcome.comparison.top1_matches));
    q2.insert("gpu_top1".to_owned(), Valor::from(outcome.comparison.gpu_top1));
    q2.insert("golden_top1".to_owned(), Valor::from(outcome.comparison.golden_top1));
    q2.insert("max_delta".to_owned(), Valor::from(outcome.comparison.max_delta as f64));
    q2.insert("numeric_matches".to_owned(), Valor::from(outcome.comparison.numeric_matches));
    q2.insert("all_finite".to_owned(), Valor::from(outcome.comparison.all_finite));
    q2.insert("divergence".to_owned(), Valor::from(outcome.comparison.divergence_field()));
    q2.insert("ok".to_owned(), Valor::from(outcome.comparison.ok));
    root.insert("q2".to_owned(), Valor::Tabula(q2));
    let mut s6 = StdBTreeMap::new();
    s6.insert("shape_class".to_owned(), Valor::from(outcome.receipt.regime_fields.shape_class.clone()));
    s6.insert("representation".to_owned(), Valor::from(outcome.receipt.regime_fields.representation.clone()));
    s6.insert("algorithm".to_owned(), Valor::from(outcome.receipt.regime_fields.algorithm.clone()));
    s6.insert("workspace".to_owned(), Valor::from(outcome.receipt.regime_fields.workspace.clone()));
    root.insert("s6_regime".to_owned(), Valor::Tabula(s6));
    root.insert(
        "transfers".to_owned(),
        Valor::from(outcome.receipt.transfers as i64),
    );
    root.insert(
        "allocations".to_owned(),
        Valor::from(outcome.receipt.allocations as i64),
    );
    root.insert(
        "launches".to_owned(),
        Valor::from(outcome.receipt.launches as i64),
    );
    root.insert(
        "syncs".to_owned(),
        Valor::from(outcome.receipt.syncs as i64),
    );
    root.insert(
        "observations".to_owned(),
        Valor::from(outcome.receipt.observations as i64),
    );
    root.insert(
        "repack_conversion_us".to_owned(),
        Valor::from(outcome.receipt.repack_conversion_us as i64),
    );
    root.insert(
        "module_prep_us".to_owned(),
        Valor::from(outcome.receipt.module_prep_us as i64),
    );
    root.insert(
        "first_invocation_us".to_owned(),
        Valor::from(outcome.receipt.first_invocation_us as i64),
    );
    let json = Json::from_object(root).map_err(|error| {
        vec![Diagnostic::error(format!(
            "prefill device run: cannot serialize the comparison record: {error}"
        ))]
    })?;
    Ok(format!("{}\n", json.to_wire()))
}

/// The outcome of a prefill device run.
pub(crate) struct PrefillRunOutcome {
    pub(crate) comparison: PrefillComparison,
    pub(crate) receipt: PrefillReceipt,
    pub(crate) receipts: usize,
    pub(crate) backend: String,
}

impl std::fmt::Display for PrefillRunOutcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(formatter, "prefill device run ({})", self.backend)?;
        writeln!(
            formatter,
            "  kernels launched: {}; transfers {}; allocations {}; syncs {}; observations {}",
            self.receipt.launches,
            self.receipt.transfers,
            self.receipt.allocations,
            self.receipt.syncs,
            self.receipt.observations
        )?;
        writeln!(
            formatter,
            "  repack {:.3}s; module prep {:.3}s; first invocation {:.3}s",
            self.receipt.repack_conversion_us as f64 / 1e6,
            self.receipt.module_prep_us as f64 / 1e6,
            self.receipt.first_invocation_us as f64 / 1e6
        )?;
        writeln!(
            formatter,
            "  Q2 vs the GI2-3 golden: top-1 {} (GPU) vs {} (golden) {}; numeric row {}; finite {}; max delta {:.3e}",
            self.comparison.gpu_top1,
            self.comparison.golden_top1,
            if self.comparison.top1_matches { "MATCH" } else { "MISMATCH" },
            if self.comparison.numeric_matches { "PASS" } else { "FAIL" },
            if self.comparison.all_finite { "PASS" } else { "FAIL" },
            self.comparison.max_delta
        )?;
        writeln!(formatter, "  divergence: {}", self.comparison.divergence_field())?;
        writeln!(formatter, "  repacked tensors: {} (declared f32 conversion)", self.receipts)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "prefill_run_test.rs"]
mod tests;
