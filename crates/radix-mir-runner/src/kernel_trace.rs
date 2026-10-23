//! Captured tensor elements from one kernel call, for exact run comparisons.

use crate::value::{TensorValue, Value};

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

/// One tensor captured from one kernel call, in backing-array order.
#[derive(Debug, Clone, PartialEq)]
pub struct TracedTensor {
    /// The tensor argument's slot identifier.
    pub slot: String,
    /// The captured elements in backing-array order.
    pub elements: Vec<TraceElement>,
}

/// One kernel call captured during a run.
#[derive(Debug, Clone, PartialEq)]
pub struct KernelTraceRecord {
    /// Position of this call in the run.
    pub index: usize,
    /// MIR function identifier for the kernel.
    pub function: u32,
    /// Name identifying the kernel entry point.
    pub entry: String,
    /// Tensors captured for this call.
    pub tensors: Vec<TracedTensor>,
}

/// Ordered kernel-call records captured from one run.
pub type KernelTrace = Vec<KernelTraceRecord>;

/// Capture a tensor's backing array as traced elements.
///
/// A view's trace is its backing array, not its logical coordinates, because
/// the device dispatcher refuses a non-contiguous or non-zero-offset tensor
/// argument.
#[must_use]
pub fn traced_tensor(slot: &str, tensor: &TensorValue) -> TracedTensor {
    let elements = (0..tensor.data.len())
        .filter_map(|index| tensor.data.get(index))
        .map(trace_value)
        .collect();
    TracedTensor {
        slot: slot.to_owned(),
        elements,
    }
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
