//! Captured tensor elements and scalar returns from kernel calls, and the comparator that finds
//! the first divergence between two runs of one program (the C3 comparator).
//!
//! A run captures one [`KernelTraceRecord`] per source-level kernel call: a
//! call, from non-kernel code, to a compute-stage function. The reference run
//! (`RunnerKernelPolicy::Execute`) and a device run
//! (`RunnerKernelPolicy::Dispatch`) capture the same calls in the same order,
//! so [`compare_traces`] walks the two traces call by call and reports the
//! first kernel (by name and call number) and element whose values differ
//! beyond the identical / conforming rules of `numeric-model.md`
//! (*Conformance of a device result against the reference*).

use crate::value::{TensorValue, Value};
use crate::{RunnerError, kernel_dispatch};
use radix_lexer::Span;
use std::fmt;

/// Absolute float tolerance of a conforming kernel result (the repo kernel
/// tolerance policy, `numeric-model.md` row 17).
pub const FLOAT_ATOL: f64 = 1e-6;

/// Relative float tolerance of a conforming kernel result, measured against
/// the reference value.
pub const FLOAT_RTOL: f64 = 1e-6;

/// One element of a traced tensor.
#[derive(Debug, Clone, PartialEq)]
pub enum TraceElement {
    /// An `f32` or `f64` tensor element, represented as `f64` (an `f32` is
    /// widened exactly).
    Float(f64),
    /// A non-floating numeric value represented by its 64-bit pattern.
    Exact(u64),
    /// A value without a 64-bit numeric form, represented by rendered text.
    Text(String),
}

/// How a traced tensor took part in its kernel call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceRole {
    /// A tensor passed by value or read-only reference.
    Input,
    /// A `mut` tensor parameter: the kernel writes it.
    Output,
    /// The call's returned tensor.
    Return,
}

/// The summation partition of a float reduction whose result may differ from
/// the reference by the order of addition (`cpu-reordering-bound.md`): `p`
/// partials of at most `m` elements each over a source with absolute sum
/// `Σ|x|`. A trace carries it only when the capture knows the partition; the
/// comparator never infers one from an entry name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReductionExtent {
    /// Number of partial sums `p`.
    pub partials: u64,
    /// Most elements `m` in one partial.
    pub partial_len: u64,
    /// `Σ|x|` over the whole reduced source.
    pub sum_abs: f64,
    /// Unit roundoff `u` of the element type (`2^-24` for `f32`).
    pub unit_roundoff: f64,
}

impl ReductionExtent {
    /// The bound `((m − 1) + (p − 1)) · u · Σ|x|` on the difference between
    /// a reordered sum and the exactly rounded one.
    #[must_use]
    pub fn bound(&self) -> f64 {
        let terms = self.partial_len.saturating_sub(1) + self.partials.saturating_sub(1);
        terms as f64 * self.unit_roundoff * self.sum_abs
    }
}

/// One tensor captured from one kernel call, in backing-array order.
#[derive(Debug, Clone, PartialEq)]
pub struct TracedTensor {
    /// The tensor's slot: `arg<N>` for the call's `N`th argument (zero-based,
    /// as written at the call site), or `return` for the returned tensor.
    pub slot: String,
    /// Whether the kernel reads, writes or returns the tensor.
    pub role: TraceRole,
    /// The tensor's logical shape.
    pub shape: Vec<i64>,
    /// The captured elements in backing-array order.
    pub elements: Vec<TraceElement>,
    /// The reduction partition, when the capture carries one.
    pub reduction: Option<ReductionExtent>,
}

/// One kernel call captured during a run.
#[derive(Debug, Clone, PartialEq)]
pub struct KernelTraceRecord {
    /// One-based source-level kernel-call number in the run. A call the
    /// device expands into several launches is still one call.
    pub index: usize,
    /// MIR function identifier for the kernel.
    pub function: u32,
    /// Name identifying the kernel entry point.
    pub entry: String,
    /// How many physical launch stages the device ran for this call, when the
    /// device expanded it into stages. `None` on the reference route and for
    /// a call that is one launch.
    pub physical_stages: Option<u32>,
    /// Tensors captured for this call, in argument order, then the return.
    pub tensors: Vec<TracedTensor>,
    /// The call's scalar return (a number or a bool), when it returns one. A
    /// returned tensor is in `tensors`; a call that returns nothing, or a
    /// value with no numeric form, has no result here.
    pub result: Option<TraceElement>,
}

/// Ordered kernel-call records captured from one run.
pub type KernelTrace = Vec<KernelTraceRecord>;

/// Capture a tensor's backing array as traced elements.
///
/// A view's trace is its backing array, not its logical coordinates, because
/// the device dispatcher refuses a non-contiguous or non-zero-offset tensor
/// argument. A device-resident tensor is made host-current first (a download,
/// like any host read), so tracing a device run changes its transfer counts.
///
/// # Errors
///
/// Returns the residency error when a device download fails.
pub fn traced_tensor(
    slot: &str,
    role: TraceRole,
    tensor: &TensorValue,
) -> Result<TracedTensor, RunnerError> {
    let count = tensor.data.len();
    if !tensor.make_host_current(0..count) {
        return Err(tensor.host_read_error(|| {
            RunnerError::internal(Span::default(), "kernel trace: device download failed")
        }));
    }
    let elements = (0..count)
        .filter_map(|index| tensor.data.get(index))
        .map(trace_value)
        .collect();
    Ok(TracedTensor {
        slot: slot.to_owned(),
        role,
        shape: tensor.shape.clone(),
        elements,
        reduction: None,
    })
}

fn trace_value(value: Value) -> TraceElement {
    match value {
        Value::Float(value) => TraceElement::Float(value),
        Value::Int(value) => TraceElement::Exact(u64::from_le_bytes(value.to_le_bytes())),
        Value::UInt(value) => TraceElement::Exact(value),
        Value::Bool(value) => TraceElement::Exact(u64::from(value)),
        other => TraceElement::Text(format!("{other:?}")),
    }
}

/// Capture a kernel call's scalar return: a float, an integer or a bool. A
/// tensor return is traced as a tensor, and any other value (unit, text, a
/// composite) is not part of the trace.
#[must_use]
pub fn traced_result(value: &Value) -> Option<TraceElement> {
    match value {
        Value::Float(_) | Value::Int(_) | Value::UInt(_) | Value::Bool(_) => {
            Some(trace_value(value.clone()))
        }
        _ => None,
    }
}

/// Ask a dispatcher how many physical stages its last call ran; used by the
/// runner right after a dispatch.
pub(crate) fn last_call_stages(hook: &kernel_dispatch::ResidencyHook) -> Option<u32> {
    hook.try_borrow_mut()
        .ok()
        .and_then(|mut dispatcher| dispatcher.last_call_stages())
}

/// How two elements relate under the identical / conforming rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementMatch {
    /// Bit for bit equal.
    Identical,
    /// Floats that differ, but inside the tolerance (or the carried
    /// reduction bound). Never called identical.
    Conforming,
    /// Outside the rules: a defect.
    Differs,
}

/// Relate a device element to the reference element.
///
/// Floats are identical when their bits are equal and conforming when they
/// differ by at most `atol + rtol·|reference|` (or the carried reduction
/// bound, when larger); two NaNs conform. Every other element kind must be
/// bitwise equal. `reduction` is the bound of the tensor the element is from.
#[must_use]
pub fn match_elements(
    reference: &TraceElement,
    device: &TraceElement,
    reduction: Option<&ReductionExtent>,
) -> ElementMatch {
    match (reference, device) {
        (TraceElement::Float(want), TraceElement::Float(got)) => {
            if want.to_bits() == got.to_bits() {
                return ElementMatch::Identical;
            }
            if want.is_nan() && got.is_nan() {
                return ElementMatch::Conforming;
            }
            if !want.is_finite() || !got.is_finite() {
                return ElementMatch::Differs;
            }
            let tolerance = FLOAT_ATOL + FLOAT_RTOL * want.abs();
            let allowed = reduction.map_or(tolerance, |extent| tolerance.max(extent.bound()));
            if (want - got).abs() <= allowed {
                ElementMatch::Conforming
            } else {
                ElementMatch::Differs
            }
        }
        (TraceElement::Exact(want), TraceElement::Exact(got)) if want == got => {
            ElementMatch::Identical
        }
        (TraceElement::Text(want), TraceElement::Text(got)) if want == got => {
            ElementMatch::Identical
        }
        _ => ElementMatch::Differs,
    }
}

/// What differed at the first divergence.
#[derive(Debug, Clone, PartialEq)]
pub enum DivergenceDetail {
    /// One trace has more kernel calls than the other.
    CallCount {
        /// Calls in the reference trace.
        reference: usize,
        /// Calls in the device trace.
        device: usize,
    },
    /// The same call number ran different kernels.
    Entry {
        /// The reference run's kernel.
        reference: String,
        /// The device run's kernel.
        device: String,
    },
    /// The call's tensors do not line up (count, slot, role, shape or length),
    /// or only one run returned a scalar.
    Layout(String),
    /// The call's scalar return differs beyond the identical / conforming
    /// rules.
    Return {
        /// The reference value.
        reference: TraceElement,
        /// The device value.
        device: TraceElement,
    },
    /// One element differs beyond the identical / conforming rules.
    Element {
        /// The tensor's slot.
        slot: String,
        /// The tensor's logical shape.
        shape: Vec<i64>,
        /// The backing-array index of the element.
        element: usize,
        /// The reference value.
        reference: TraceElement,
        /// The device value.
        device: TraceElement,
    },
}

/// The first place a device trace departs from the reference trace.
#[derive(Debug, Clone, PartialEq)]
pub struct Divergence {
    /// One-based source-level kernel-call number.
    pub call: usize,
    /// The device's physical stage count for that call, when it expanded it.
    pub physical_stages: Option<u32>,
    /// The kernel entry name (the reference's, or the one present).
    pub entry: String,
    /// What differed.
    pub detail: DivergenceDetail,
}

/// The outcome of comparing a device trace against a reference trace.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// Kernel calls compared (up to and including the divergent one).
    pub calls_compared: usize,
    /// Float elements that differed but conformed, before any divergence.
    pub conforming_elements: usize,
    /// The first divergence, or `None` when the traces agree.
    pub divergence: Option<Divergence>,
}

impl Comparison {
    /// Whether the traces agree bit for bit (no divergence, nothing merely
    /// conforming).
    #[must_use]
    pub fn is_identical(&self) -> bool {
        self.divergence.is_none() && self.conforming_elements == 0
    }

    /// One report line naming the verdict: `IDENTICAL`, `CONFORMING` or the
    /// first divergence.
    #[must_use]
    pub fn report(&self) -> String {
        match &self.divergence {
            Some(divergence) => format!("DIVERGENT: {divergence}"),
            None if self.conforming_elements == 0 => {
                format!(
                    "IDENTICAL: {} kernel calls, no divergence",
                    self.calls_compared
                )
            }
            None => format!(
                "CONFORMING: {} kernel calls, {} elements differ within tolerance, no divergence",
                self.calls_compared, self.conforming_elements
            ),
        }
    }
}

impl fmt::Display for TraceElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Float(value) => write!(f, "{value:?} (0x{:016x})", value.to_bits()),
            Self::Exact(bits) => write!(f, "{bits} (0x{bits:x})"),
            Self::Text(text) => f.write_str(text),
        }
    }
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "kernel call {} `{}`", self.call, self.entry)?;
        if let Some(stages) = self.physical_stages {
            write!(f, " ({stages} physical stages)")?;
        }
        match &self.detail {
            DivergenceDetail::CallCount { reference, device } => {
                write!(
                    f,
                    ": reference ran {reference} kernel calls, device ran {device}"
                )
            }
            DivergenceDetail::Entry { reference, device } => {
                write!(f, ": reference ran `{reference}`, device ran `{device}`")
            }
            DivergenceDetail::Layout(message) => write!(f, ": {message}"),
            DivergenceDetail::Return { reference, device } => {
                write!(f, ": return value: reference {reference}, device {device}")
            }
            DivergenceDetail::Element {
                slot,
                shape,
                element,
                reference,
                device,
            } => write!(
                f,
                ": slot {slot} element {element}{}: reference {reference}, device {device}",
                coordinates(shape, *element)
            ),
        }
    }
}

/// ` at [i, j]` for a row-major element index, or empty when the shape does
/// not cover the index (a view's backing is larger than its shape).
fn coordinates(shape: &[i64], element: usize) -> String {
    let mut remaining = element;
    let mut at = vec![0_usize; shape.len()];
    for (axis, extent) in shape.iter().enumerate().rev() {
        let Ok(extent) = usize::try_from(*extent) else {
            return String::new();
        };
        if extent == 0 {
            return String::new();
        }
        at[axis] = remaining % extent;
        remaining /= extent;
    }
    if remaining != 0 {
        return String::new();
    }
    let text = at
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!(" at [{text}]")
}

/// Walk `reference` and `device` in launch order and report the first kernel
/// call and element whose values differ beyond the identical / conforming
/// rules.
///
/// A float element that differs but conforms is counted, never called
/// identical; a changed integer, bit pattern or text, or a float outside the
/// tolerance, is the divergence.
#[must_use]
pub fn compare_traces(reference: &[KernelTraceRecord], device: &[KernelTraceRecord]) -> Comparison {
    let mut conforming_elements = 0;
    for (position, (want, got)) in reference.iter().zip(device).enumerate() {
        let call = position + 1;
        let diverge = |detail| {
            Some(Divergence {
                call,
                physical_stages: got.physical_stages,
                entry: want.entry.clone(),
                detail,
            })
        };
        let divergence = if want.entry == got.entry {
            compare_record(want, got, &mut conforming_elements).and_then(diverge)
        } else {
            diverge(DivergenceDetail::Entry {
                reference: want.entry.clone(),
                device: got.entry.clone(),
            })
        };
        if divergence.is_some() {
            return Comparison {
                calls_compared: call,
                conforming_elements,
                divergence,
            };
        }
    }
    let compared = reference.len().min(device.len());
    let divergence = (reference.len() != device.len()).then(|| {
        let extra = if reference.len() > device.len() {
            &reference[compared]
        } else {
            &device[compared]
        };
        Divergence {
            call: compared + 1,
            physical_stages: device
                .get(compared)
                .and_then(|record| record.physical_stages),
            entry: extra.entry.clone(),
            detail: DivergenceDetail::CallCount {
                reference: reference.len(),
                device: device.len(),
            },
        }
    });
    Comparison {
        calls_compared: if divergence.is_some() {
            compared + 1
        } else {
            compared
        },
        conforming_elements,
        divergence,
    }
}

fn compare_record(
    want: &KernelTraceRecord,
    got: &KernelTraceRecord,
    conforming: &mut usize,
) -> Option<DivergenceDetail> {
    if want.tensors.len() != got.tensors.len() {
        return Some(DivergenceDetail::Layout(format!(
            "reference traced {} tensors, device traced {}",
            want.tensors.len(),
            got.tensors.len()
        )));
    }
    for (want, got) in want.tensors.iter().zip(&got.tensors) {
        if want.slot != got.slot || want.role != got.role {
            return Some(DivergenceDetail::Layout(format!(
                "slot {} ({:?}) against slot {} ({:?})",
                want.slot, want.role, got.slot, got.role
            )));
        }
        if want.shape != got.shape || want.elements.len() != got.elements.len() {
            return Some(DivergenceDetail::Layout(format!(
                "slot {}: reference shape {:?} with {} elements, device shape {:?} with {}",
                want.slot,
                want.shape,
                want.elements.len(),
                got.shape,
                got.elements.len()
            )));
        }
        for (element, (r, d)) in want.elements.iter().zip(&got.elements).enumerate() {
            match match_elements(r, d, want.reduction.as_ref()) {
                ElementMatch::Identical => {}
                ElementMatch::Conforming => *conforming += 1,
                ElementMatch::Differs => {
                    return Some(DivergenceDetail::Element {
                        slot: want.slot.clone(),
                        shape: want.shape.clone(),
                        element,
                        reference: r.clone(),
                        device: d.clone(),
                    });
                }
            }
        }
    }
    compare_result(want, got, conforming)
}

/// Compare the scalar returns of one call: a float is identical, conforming
/// or divergent by the same rules as a tensor element; an integer, bool or
/// bit pattern must match exactly.
fn compare_result(
    want: &KernelTraceRecord,
    got: &KernelTraceRecord,
    conforming: &mut usize,
) -> Option<DivergenceDetail> {
    match (&want.result, &got.result) {
        (None, None) => None,
        (Some(reference), Some(device)) => match match_elements(reference, device, None) {
            ElementMatch::Identical => None,
            ElementMatch::Conforming => {
                *conforming += 1;
                None
            }
            ElementMatch::Differs => Some(DivergenceDetail::Return {
                reference: reference.clone(),
                device: device.clone(),
            }),
        },
        (Some(_), None) => Some(DivergenceDetail::Layout(
            "reference returned a scalar, device returned none".to_owned(),
        )),
        (None, Some(_)) => Some(DivergenceDetail::Layout(
            "reference returned no scalar, device returned one".to_owned(),
        )),
    }
}
