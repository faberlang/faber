//! Opt-in diagnostic statistics for interpreted MIR execution.
//!
//! The collector deliberately lives beside the interpreter rather than in the
//! package or CLI layers.  It records execution facts only; serialization and
//! policy for consuming those facts belong to callers.

use radix_lexer::Interner;
use radix_mir::{
    MirDiagnosticKind, MirFunction, MirFunctionId, MirIntrinsic, MirSermoOp, MirValidationContext,
};
use rustc_hash::FxHashMap;
use std::time::{Duration, Instant};

/// Number of duration samples retained for each function or intrinsic.
pub const RESERVOIR_SIZE: usize = 256;

/// The duration reservoir is deliberately approximate, even when it has not
/// filled.  The CLI schema owns the serialized spelling of this fact.
pub const RESERVOIR_METHOD: &str = "reservoir";
pub const RESERVOIR_EXACT: bool = false;

/// One observed interpreted function, keyed by its source module and name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionStats {
    pub module: String,
    pub name: String,
    pub calls: u64,
    pub total_ns: u64,
    pub self_ns: u64,
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub max_ns: u64,
}

/// One observed compiler-runtime intrinsic.
///
/// Intrinsics intentionally do not expose a `p50_ns` or `max_ns` field in the
/// binding schema.  Their `p95_ns` is still derived from the same fixed
/// reservoir, while `self_ns == total_ns` is the settled leaf approximation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntrinsicStats {
    pub name: String,
    pub calls: u64,
    pub total_ns: u64,
    pub self_ns: u64,
    pub p95_ns: u64,
}

/// Snapshot of the runner's diagnostic observations.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunnerStats {
    pub functions: Vec<FunctionStats>,
    pub intrinsics: Vec<IntrinsicStats>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FunctionKey {
    module: String,
    name: String,
}

#[derive(Debug, Clone)]
struct FunctionAccumulator {
    calls: u64,
    total_ns: u64,
    self_ns: u64,
    durations: Reservoir,
}

impl FunctionAccumulator {
    fn new() -> Self {
        Self {
            calls: 0,
            total_ns: 0,
            self_ns: 0,
            durations: Reservoir::default(),
        }
    }

    /// Record one activation segment. A resumed generator segment adds time
    /// and a duration sample but not another call.
    fn record(&mut self, duration_ns: u64, self_ns: u64, counts_call: bool) {
        if counts_call {
            self.calls = self.calls.saturating_add(1);
        }
        self.total_ns = self.total_ns.saturating_add(duration_ns);
        self.self_ns = self.self_ns.saturating_add(self_ns);
        self.durations.push(duration_ns);
    }

    fn into_stats(self, key: FunctionKey) -> FunctionStats {
        let (p50_ns, p95_ns, max_ns) = self.durations.percentiles();
        FunctionStats {
            module: key.module,
            name: key.name,
            calls: self.calls,
            total_ns: self.total_ns,
            self_ns: self.self_ns,
            p50_ns,
            p95_ns,
            max_ns,
        }
    }
}

#[derive(Debug, Clone)]
struct IntrinsicAccumulator {
    calls: u64,
    total_ns: u64,
    durations: Reservoir,
}

impl IntrinsicAccumulator {
    fn new() -> Self {
        Self {
            calls: 0,
            total_ns: 0,
            durations: Reservoir::default(),
        }
    }

    fn record(&mut self, duration_ns: u64) {
        self.calls = self.calls.saturating_add(1);
        self.total_ns = self.total_ns.saturating_add(duration_ns);
        self.durations.push(duration_ns);
    }

    fn into_stats(self, name: String) -> IntrinsicStats {
        let (_, p95_ns, _) = self.durations.percentiles();
        IntrinsicStats {
            name,
            calls: self.calls,
            total_ns: self.total_ns,
            // Higher-order dispatch is intentionally a leaf approximation in
            // v1: nested function rows account for their own samples, while
            // the intrinsic row retains its inclusive dispatch time.
            self_ns: self.total_ns,
            p95_ns,
        }
    }
}

/// A deterministic fixed-size reservoir.
///
/// The replacement decision is the standard reservoir decision with a
/// deterministic `SplitMix64` draw derived from the observation number.  This
/// preserves the bounded-reservoir shape without depending on process-global
/// randomness, which keeps diagnostic artifacts reproducible for a fixed trace.
#[derive(Debug, Clone, Default)]
struct Reservoir {
    samples: Vec<u64>,
    seen: u64,
}

impl Reservoir {
    fn push(&mut self, duration_ns: u64) {
        self.seen = self.seen.saturating_add(1);
        if self.samples.len() < RESERVOIR_SIZE {
            self.samples.push(duration_ns);
            return;
        }

        // `seen` is the one-based observation count.  A standard reservoir
        // draw for observation n chooses an integer in [0, n), then replaces
        // that slot only when it falls in the retained range.
        let slot = splitmix64(self.seen) % self.seen;
        if slot < u64::try_from(RESERVOIR_SIZE).unwrap_or(u64::MAX) {
            let Ok(slot) = usize::try_from(slot) else {
                return;
            };
            self.samples[slot] = duration_ns;
        }
    }

    fn percentiles(&self) -> (u64, u64, u64) {
        if self.samples.is_empty() {
            return (0, 0, 0);
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        (
            nearest_rank(&sorted, 50),
            nearest_rank(&sorted, 95),
            *sorted.last().unwrap_or(&0),
        )
    }
}

fn nearest_rank(sorted: &[u64], percentile: u64) -> u64 {
    let percentile = usize::try_from(percentile).unwrap_or(usize::MAX);
    let rank = sorted.len().saturating_mul(percentile).saturating_add(99) / 100;
    let rank = rank.max(1);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

struct ActiveCall {
    function: MirFunctionId,
    counts_call: bool,
    started: Instant,
    child_ns: u64,
}

/// Internal opt-in collector owned by one [`Runner`](crate::Runner).
pub(crate) struct StatsCollector {
    function_names: FxHashMap<MirFunctionId, FunctionKey>,
    functions: FxHashMap<FunctionKey, FunctionAccumulator>,
    intrinsics: FxHashMap<String, IntrinsicAccumulator>,
    active_calls: Vec<ActiveCall>,
}

impl StatsCollector {
    pub(crate) fn new(
        program: &radix_mir::MirProgram,
        context: &MirValidationContext<'_>,
        interner: &Interner,
    ) -> Self {
        let names = radix_mir::names::MirNames::new(program, context.types.as_table(), interner);
        let function_names = program
            .functions
            .iter()
            .map(|function| {
                let key = function_key(function, &names, context);
                (function.id, key)
            })
            .collect();
        Self {
            function_names,
            functions: FxHashMap::default(),
            intrinsics: FxHashMap::default(),
            active_calls: Vec::new(),
        }
    }

    /// Open an activation segment; `counts_call` is `false` when a parked
    /// generator frame resumes (D6.6), so one generator call counts once.
    pub(crate) fn enter_function(&mut self, function: MirFunctionId, counts_call: bool) {
        self.active_calls.push(ActiveCall {
            function,
            counts_call,
            started: Instant::now(),
            child_ns: 0,
        });
    }

    pub(crate) fn leave_function(&mut self, function: MirFunctionId) {
        let Some(active) = self.active_calls.pop() else {
            return;
        };
        debug_assert_eq!(active.function, function);
        let elapsed_ns = duration_ns(active.started.elapsed());
        let self_ns = elapsed_ns.saturating_sub(active.child_ns);
        let key = self.key_for_function(function);
        self.record_function(key, elapsed_ns, self_ns, active.counts_call);
        if let Some(parent) = self.active_calls.last_mut() {
            parent.child_ns = parent.child_ns.saturating_add(elapsed_ns);
        }
    }

    /// Stable key for an observed function, falling back to the root module
    /// for a function absent from the program's name table.
    fn key_for_function(&self, function: MirFunctionId) -> FunctionKey {
        self.function_names
            .get(&function)
            .cloned()
            .unwrap_or_else(|| FunctionKey {
                module: "root".to_owned(),
                name: format!("f{}", function.0),
            })
    }

    fn record_function(
        &mut self,
        key: FunctionKey,
        duration_ns: u64,
        self_ns: u64,
        counts_call: bool,
    ) {
        self.functions
            .entry(key)
            .or_insert_with(FunctionAccumulator::new)
            .record(duration_ns, self_ns, counts_call);
    }

    pub(crate) fn begin_intrinsic() -> Instant {
        Instant::now()
    }

    pub(crate) fn record_intrinsic(&mut self, name: String, started: Instant) {
        self.intrinsics
            .entry(name)
            .or_insert_with(IntrinsicAccumulator::new)
            .record(duration_ns(started.elapsed()));
    }

    /// Record a synthetic duration for the focused collector tests.  Runtime
    /// production paths use `enter_function`/`leave_function`, so this helper
    /// does not alter the clock or execution boundary.
    #[cfg(test)]
    pub(crate) fn record_function_sample(
        &mut self,
        function: MirFunctionId,
        duration_ns: u64,
        self_ns: u64,
    ) {
        let key = self.key_for_function(function);
        self.record_function(key, duration_ns, self_ns, true);
    }

    #[cfg(test)]
    pub(crate) fn record_intrinsic_sample(&mut self, name: &str, duration_ns: u64) {
        self.intrinsics
            .entry(name.to_owned())
            .or_insert_with(IntrinsicAccumulator::new)
            .record(duration_ns);
    }

    pub(crate) fn finish(mut self) -> RunnerStats {
        // A normally completed run has an empty stack.  Close any frames left
        // by a runtime failure as a final defensive boundary, so every entry
        // still has a paired sample even if a future caller forgets runner
        // cleanup.
        while let Some(active) = self.active_calls.last() {
            let function = active.function;
            self.leave_function(function);
        }
        let mut functions = self
            .functions
            .into_iter()
            .map(|(key, value)| value.into_stats(key))
            .collect::<Vec<_>>();
        functions.sort_by(|left, right| {
            left.module
                .cmp(&right.module)
                .then_with(|| left.name.cmp(&right.name))
        });
        let mut intrinsics = self
            .intrinsics
            .into_iter()
            .map(|(name, value)| value.into_stats(name))
            .collect::<Vec<_>>();
        intrinsics.sort_by(|left, right| left.name.cmp(&right.name));
        RunnerStats {
            functions,
            intrinsics,
        }
    }
}

fn function_key(
    function: &MirFunction,
    names: &radix_mir::names::MirNames<'_>,
    context: &MirValidationContext<'_>,
) -> FunctionKey {
    if let Some(identity) = function
        .source
        .and_then(|source| context.external_function_identities.get(&source))
    {
        return FunctionKey {
            module: if identity.module_path.is_empty() {
                "root".to_owned()
            } else {
                identity.module_path.join("::")
            },
            name: identity.name.clone(),
        };
    }
    // `MirNames` owns the same O(F) spelling table used by emitters.  Source
    // functions without package identity and synthetic entry functions both
    // use its stable spelling, under the root module.
    FunctionKey {
        module: "root".to_owned(),
        name: names.function(function),
    }
}

fn duration_ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// Stable, human-readable intrinsic row name.
pub(crate) fn intrinsic_name(intrinsic: &MirIntrinsic) -> String {
    match intrinsic {
        MirIntrinsic::Diagnostic(kind) => format!("diagnostic.{}", diagnostic_name(*kind)),
        MirIntrinsic::Assert => "assert".to_owned(),
        MirIntrinsic::FormatString { .. } => "format_string".to_owned(),
        MirIntrinsic::FormatValue { .. } => "format_value".to_owned(),
        MirIntrinsic::Convert(_) => "convert".to_owned(),
        MirIntrinsic::Collection(op) => format!("collection.{}", debug_snake(op)),
        MirIntrinsic::Atomic(op) => format!("atomic.{}", op.spelling()),
        MirIntrinsic::Panic => "panic".to_owned(),
        MirIntrinsic::Provider(_) => "provider".to_owned(),
        MirIntrinsic::SermoOpen => "sermo_open".to_owned(),
        MirIntrinsic::SermoSetOpener => "sermo_set_opener".to_owned(),
        MirIntrinsic::Sermo(op) => format!("sermo.{}", sermo_name(op)),
        MirIntrinsic::ReadLine => "read_line".to_owned(),
        MirIntrinsic::ReadAll => "read_all".to_owned(),
        MirIntrinsic::Cede => "cede".to_owned(),
        MirIntrinsic::DeviceBlock(_) => "device_block".to_owned(),
        MirIntrinsic::GpuBuiltin(_) => "gpu_builtin".to_owned(),
        MirIntrinsic::CursorStream(_) => "cursor_stream".to_owned(),
        MirIntrinsic::Gradient(op) => format!("gradient.{}", debug_snake(op)),
        MirIntrinsic::TypeCheck(_) => "type_check".to_owned(),
        MirIntrinsic::TensorShapeCheck(_) => "tensor_shape_check".to_owned(),
        MirIntrinsic::TensorShapeSize(_) => "tensor_shape_size".to_owned(),
        MirIntrinsic::TensorLimes(_) => "tensor_limes".to_owned(),
        MirIntrinsic::FilumReduce(_) => "filum_sum".to_owned(),
        MirIntrinsic::Duplicate => "duplicate".to_owned(),
        MirIntrinsic::TensorLift(op) => format!("tensor_lift.{}", op.name()),
        MirIntrinsic::VectorOp(op) => format!("vector_op.{}", op.name()),
    }
}

fn diagnostic_name(kind: MirDiagnosticKind) -> &'static str {
    match kind {
        MirDiagnosticKind::Nota => "nota",
        MirDiagnosticKind::Vide => "vide",
        MirDiagnosticKind::Mone => "mone",
        MirDiagnosticKind::Scribe => "scribe",
    }
}

fn sermo_name(op: &MirSermoOp) -> &'static str {
    match op {
        MirSermoOp::ViewMeus => "view_meus",
        MirSermoOp::ViewTuus => "view_tuus",
        MirSermoOp::Send => "send",
        MirSermoOp::Flush => "flush",
        MirSermoOp::Finish => "finish",
        MirSermoOp::Close => "close",
        MirSermoOp::CloseFailed => "close_failed",
        MirSermoOp::CloseError => "close_error",
        MirSermoOp::Receive { .. } => "receive",
        MirSermoOp::Cursor { .. } => "cursor",
        MirSermoOp::DrainCursor => "drain_cursor",
    }
}

fn debug_snake<T: std::fmt::Debug>(value: &T) -> String {
    let raw = format!("{value:?}");
    let mut out = String::with_capacity(raw.len());
    for (index, ch) in raw.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch);
        }
    }
    out
}
