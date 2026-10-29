//! In-process MIR runner for Faber scripting.
//!
//! TARGET: faber run (single-file interpret path)
//! WHY: Wasm already executes MIR end-to-end; this module dispatches the same
//! intrinsics directly without emit/instantiate.

#![allow(clippy::too_many_lines)]

pub mod capability;
mod conversio;
mod cursor;
mod decimal;
pub mod dense;
mod device_block;
mod exact_int;
mod filum_grid;
#[cfg(test)]
#[path = "filum_grid_test.rs"]
mod filum_grid_tests;
mod host;
pub mod inf;
pub mod kernel_dispatch;
pub mod kernel_lanes;
pub mod kernel_trace;
#[cfg(test)]
mod kernel_trace_test;
#[cfg(test)]
mod regex_literal_raw_test;
mod regex_program;
mod routes;
mod run_tier;
mod runtime;
mod scan;
pub mod sermo;
#[cfg(test)]
mod shape_size_test;
pub(crate) mod shuffle;
pub mod stats;
pub mod store;
mod tensor_edge;
mod tensor_lift;
pub mod value;

pub use capability::{RunnerDispatchStatus, classify_runner_lowerability, runner_dispatch_status};
pub use run_tier::classify_run_tier;

pub use host::{BufferHost, Host, StdioHost};
pub use inf::{
    RUNNER_INF_BIT_CEILING, RUNNER_INF_MUL_WORK_CEILING, RUNNER_ISSUE_INF_SIZE_EXHAUSTED,
};
pub fn reset_execution_state() {
    faber_leaf::reset_execution_state();
}
pub use routes::serve_route;
pub use shuffle::configure_shuffle_seed;
pub use stats::{FunctionStats, IntrinsicStats, RunnerStats};
pub use value::{FieldOrder, MapKey, Value};

use handroll::{FxHashMap, FxHashSet};
use radix_hir::DefId;
use radix_lexer::{Interner, Span, Symbol};
use radix_mir::{
    MirBinOp, MirBlock, MirBlockId, MirCallee, MirCollectionOp, MirConstant, MirFunction,
    MirFunctionId, MirFunctionIndex, MirKernelShaderStage, MirLocalId, MirOperand, MirParamMode,
    MirPlace, MirPlaceBase, MirProgram, MirProjection, MirStatementKind, MirTempId,
    MirTerminatorKind, MirType, MirUnOp, MirValidationContext, MirValue, MirValueId, MirValueKind,
    ValidatedMir,
};
use radix_runtime_contract::display_tokens::DisplayTokens;
use radix_runtime_contract::trap::text as trap_text;
use radix_types::{IndexExpr, NumericWidth, Primitive, RuntimeExtentId, Type, TypeId, TypeTable};
use regex_program::RegexProgram;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;
use value::{
    GradientStorage, add_values, compare_values, decimal_numeric_binop, decimal_scale_factor,
    effective_width, int_bitwise_binop, modular_word_bitnot, modular_word_bitwise_binop,
    modular_word_compare_values, modular_word_neg, modular_word_numeric_binop,
    modular_zero_divisor, numeric_binop, round_float, values_equal,
};

pub(crate) const RUNNER_UNSUPPORTED_PREFIX: &str = "faberscript unsupported";

/// Stable diagnostic code for unsupported runner operations (D-02-07).
pub(crate) const RUNNER_CODE_UNSUPPORTED: &str = "runner_unsupported";

/// Stable diagnostic code for internal runner failures (D-02-07).
pub(crate) const RUNNER_CODE_INTERNAL: &str = "runner_internal";

/// Stable diagnostic code for recoverable conversion failures: malformed
/// source text or a value outside the target width (SA-002). The `cape`
/// classifier keys on this code, never on the rendered message.
pub(crate) const RUNNER_CODE_CONVERSION: &str = "runner_conversion";

/// Malformed input to the failable JSON parser follows its text alternate exit.
pub(crate) const RUNNER_JSON_PARSE_FAILED: &str = "runner_json_parse_failed";

/// A flat `lista` whose length does not fill the target tensor's shape
/// (`ys ↦ tensor<E, [..]>`, `tensor.construct`). It is a recoverable
/// conversion failure carrying the `expected` and `actual` element counts,
/// like every other `↦` failure: `⊥`, `fac … cape` and `⇥` see it.
pub(crate) const RUNNER_CONVERSION_LENGTH_MISMATCH: &str = "conversion_length_mismatch";
/// A statement of a conversion's recovery region left the frame (its failure
/// followed the alternate exit). The frame is already popped, so the
/// conversion statement reports `FrameReturned` instead of writing a result.
pub(crate) const RUNNER_RECOVERY_FRAME_RETURNED: &str = "runner_recovery_frame_returned";
/// Two live lists of an elementwise operator differ in length (hard error).
pub(crate) const RUNNER_LISTA_LENGTH_MISMATCH: &str = "runner_lista_elementwise_length_mismatch";
/// The same mismatch between two bounded lists is a recoverable runtime
/// failure (D6), like a push at capacity.
pub(crate) const RUNNER_BOUNDED_LISTA_LENGTH_MISMATCH: &str =
    "runner_bounded_lista_elementwise_length_mismatch";

/// Stable diagnostic identity for refusing execution of a device-annotated
/// function. The runner's execution policy keys on MIR shader-stage metadata,
/// not on source names or rendered diagnostic text.
pub const RUNNER_KERNEL_REFUSED_ISSUE: &str = "runner_kernel_refused";

/// Whether the runner executes functions carrying the compute shader stage
/// (`@ kernel`).
///
/// Every entry point runs under [`RunnerKernelPolicy::Refuse`] unless the
/// caller names [`RunnerKernelPolicy::Execute`], so a route that is not the
/// CPU device never runs a kernel by accident. `Execute` runs a kernel with
/// its sequential meaning: the same code a host function would run, one
/// thread, a `thread`/`filum` loop as the plain loop. The policy keys on the
/// MIR shader-stage metadata, never on a source name or on message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunnerKernelPolicy {
    /// A call to a compute-stage function stops the run with
    /// [`RUNNER_KERNEL_REFUSED_ISSUE`].
    #[default]
    Refuse,
    /// A compute-stage function runs like any other function.
    Execute,
    /// A call to a compute-stage function is handed to the run's
    /// [`kernel_dispatch::KernelDispatcher`] instead of being refused or
    /// executed: a pure call with synchronous semantics. The entry point that
    /// selects `Dispatch` supplies the dispatcher
    /// ([`run_function_with_kernel_dispatcher`]).
    Dispatch,
}

/// Stable diagnostic identity for a step-budget stop. The step count is a
/// message fact and never part of the identity: a caller that bounds an
/// execution keys on this, not on the rendered wording.
pub const RUNNER_ISSUE_STEP_BUDGET_EXHAUSTED: &str = "runner_interpreter_step_budget_exhausted";

/// Stable diagnostic identity for a wall-clock deadline stop.
pub const RUNNER_ISSUE_WALL_CLOCK_TIMEOUT: &str = "runner_wall_clock_timeout_exceeded";

/// Deepest nesting of Faber calls the runner executes. A call past it stops
/// the run with [`RUNNER_ISSUE_CALL_DEPTH_EXCEEDED`] instead of growing the
/// stack without bound. Native stack is grown on demand up to this depth, so
/// the limit does not depend on the host thread's stack size.
pub const MAX_CALL_DEPTH: usize = 10_000;

/// Stable diagnostic identity for a call nested deeper than
/// [`MAX_CALL_DEPTH`].
pub const RUNNER_ISSUE_CALL_DEPTH_EXCEEDED: &str = "runner_call_depth_exceeded";

/// Native stack left, in bytes, below which a call grows the stack.
const NATIVE_STACK_RED_ZONE: usize = 256 * 1024;

/// Size, in bytes, of each native stack segment a call grows by.
const NATIVE_STACK_SEGMENT: usize = 8 * 1024 * 1024;

/// Issue stem for the unsupported family; identity comes from the structured
/// code + feature arg, never from parsing the prose message.
const RUNNER_ISSUE_UNSUPPORTED_STEM: &str = "runner_faberscript_unsupported";

/// Runner diagnostic surfaced to callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerError {
    pub message: String,
    pub issue: String,
    pub span: Span,
    /// Structured diagnostic code (`runner_unsupported`, `runner_internal`).
    pub code: &'static str,
    /// Named identity facts (e.g. `feature`, `identity`, `detail`).
    pub args: Vec<(String, String)>,
}

#[cfg(test)]
thread_local! {
    /// Test seam: how many `RunnerError::internal` values this thread has built.
    pub(crate) static INTERNAL_ERRORS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Lowercase identifier fragment for a structured fact value.
fn runner_slug(value: &str) -> String {
    runner_issue(value).trim_start_matches("runner_").to_owned()
}

impl RunnerError {
    #[must_use]
    pub fn unsupported(span: Span, feature: &str) -> Self {
        // WHY: identity derives from the structured code + `feature` arg, not
        // from the prose message; non-renderer tests assert `issue`, not prose.
        let message = format!("{RUNNER_UNSUPPORTED_PREFIX}: {feature}");
        let issue = format!("{RUNNER_ISSUE_UNSUPPORTED_STEM}_{}", runner_slug(feature));
        Self {
            message,
            issue,
            span,
            code: RUNNER_CODE_UNSUPPORTED,
            args: vec![("feature".to_owned(), feature.to_owned())],
        }
    }

    pub fn internal(span: Span, message: impl Into<String>) -> Self {
        // WHY: identity derives from the structured `detail` arg, not from
        // parsing the rendered prose message field.
        #[cfg(test)]
        INTERNAL_ERRORS_BUILT.with(|built| built.set(built.get() + 1));
        let message = message.into();
        let issue = format!("runner_{}", runner_slug(&message));
        Self {
            issue,
            span,
            code: RUNNER_CODE_INTERNAL,
            args: vec![("detail".to_owned(), message.clone())],
            message,
        }
    }

    pub fn internal_with_issue(
        span: Span,
        message: impl Into<String>,
        issue: &'static str,
    ) -> Self {
        Self {
            message: message.into(),
            issue: issue.to_owned(),
            span,
            code: RUNNER_CODE_INTERNAL,
            args: Vec::new(),
        }
    }

    /// Recoverable conversion failure with a stable named issue, for a failure
    /// whose identity is a structured fact (the caller adds its `args`) rather
    /// than a slug of the message.
    pub(crate) fn conversion_with_issue(
        span: Span,
        message: impl Into<String>,
        issue: &'static str,
    ) -> Self {
        Self {
            message: message.into(),
            issue: issue.to_owned(),
            span,
            code: RUNNER_CODE_CONVERSION,
            args: Vec::new(),
        }
    }

    /// Recoverable conversion failure (`cape`-able): malformed source text or a
    /// value outside the target width (SA-002).
    ///
    /// Carries [`RUNNER_CODE_CONVERSION`] so the caller classifies it from the
    /// structured code instead of the message wording.
    pub(crate) fn conversion(span: Span, message: impl Into<String>) -> Self {
        let message = message.into();
        let issue = format!("runner_{}", runner_slug(&message));
        Self {
            args: vec![("detail".to_owned(), message.clone())],
            issue,
            span,
            code: RUNNER_CODE_CONVERSION,
            message,
        }
    }

    /// Value of the named structured fact, if the diagnostic carries it.
    #[must_use]
    pub fn arg(&self, name: &str) -> Option<&str> {
        self.args
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Process exit status of a run this error ends: the sysexits class of the
    /// failure (`radix_runtime_contract::trap`). A leaf failure takes the class
    /// of its `IoFault` variant (`fault_kind`; a leaf failure also carries the
    /// conversion code, so the variant is read first), any other conversion
    /// failure 65, and every other failure the program-logic class 70. The
    /// class never comes from the message wording.
    #[must_use]
    pub fn exit_status(&self) -> i32 {
        use radix_runtime_contract::trap::{TrapKind, exit_status, io_fault_kind};
        let kind = match self.arg("fault_kind") {
            Some(variant) => io_fault_kind(variant).unwrap_or(TrapKind::ProgramLogic),
            None if self.is_conversion_failure() => TrapKind::Conversion,
            None => TrapKind::ProgramLogic,
        };
        exit_status(kind)
    }

    fn is_conversion_failure(&self) -> bool {
        // WHY: parse failures ("... conversion failed") and width-overflow
        // failures ("... conversion out of range", SA-002) are both recoverable
        // `cape`-able conversion failures — parity with Rust codegen's fallible
        // `.parse::<W>()` path. The structured code carries that fact; the
        // message wording never classifies a diagnostic.
        self.code == RUNNER_CODE_CONVERSION
    }
}

fn kernel_refused(span: Span) -> RunnerError {
    RunnerError::internal_with_issue(
        span,
        "runner refuses execution of @ kernel function; this route does not run kernels, pass `--device cpu` to run them on the CPU",
        RUNNER_KERNEL_REFUSED_ISSUE,
    )
}

fn runner_issue(message: &str) -> String {
    let mut issue = String::from("runner");
    let mut separated = false;
    for ch in message.chars() {
        if ch.is_ascii_alphanumeric() {
            if !separated {
                issue.push('_');
            }
            issue.push(ch.to_ascii_lowercase());
            separated = true;
        } else if !issue.ends_with('_') {
            issue.push('_');
            separated = true;
        }
    }
    issue.trim_end_matches('_').to_string()
}

fn is_recoverable_shape_failure(error: &RunnerError) -> bool {
    matches!(
        error.issue.as_str(),
        "runner_tensor_shape_mismatch" | "runner_tensor_shape_invalid_extent"
    )
}

fn shape_guard_precedes_try_call(
    statement: &radix_mir::MirStatement,
    terminator: &MirTerminatorKind,
) -> bool {
    let MirTerminatorKind::TryCall { args, .. } = terminator else {
        return false;
    };
    let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
        return false;
    };
    matches!(
        &call.intrinsic,
        radix_mir::MirIntrinsic::TensorShapeCheck(_)
    ) && call.args.get(..args.len()) == Some(args.as_slice())
}

/// Whether the tensor-fill result is dropped: the statement posture of
/// `t.fill(v);`. The MIR lowering gives every value-returning call a temp
/// destination, so the posture is read off the temp's uses: a destination the
/// rest of the block never reads (and the terminator never consumes) is the
/// discarded fill, and the documented mutation statement applies. The scan is
/// conservative within the block: any later read keeps the bound fresh-result
/// posture. It is a same-block scan, not whole-function liveness: a read of the
/// temp from another block is not seen, and this treats the fill as dropped.
fn fill_result_is_dropped(
    statement: &radix_mir::MirStatement,
    block: &radix_mir::MirBlock,
    index: usize,
) -> bool {
    let MirStatementKind::RuntimeCall { destination, call } = &statement.kind else {
        return false;
    };
    if !matches!(
        &call.intrinsic,
        radix_mir::MirIntrinsic::Collection(MirCollectionOp::TensorFill)
    ) {
        return false;
    }
    let Some(place) = destination else {
        return true;
    };
    // A named local or projected destination is a real binding.
    let MirPlaceBase::Temp(dest) = place.base else {
        return false;
    };
    if !place.projections.is_empty() {
        return false;
    }
    let mut reads = TempReads {
        temp: dest,
        found: false,
    };
    for later in block.statements.iter().skip(index + 1) {
        radix_mir::visit::walk_stmt(&mut reads, later);
    }
    radix_mir::visit::walk_terminator(&mut reads, &block.terminator);
    !reads.found
}

/// Finds any read of one temp. It rides the exhaustive MIR visitor rather than
/// re-listing every operand-bearing node: the hand-rolled predecessor skipped
/// `Option`/`Closure` values, so a bound fill stored into a nullable binding
/// (`Some(temp)`) was misread as a dropped statement and mutated its source.
struct TempReads {
    temp: MirTempId,
    found: bool,
}

impl radix_mir::visit::MirVisitor for TempReads {
    fn visit_operand(&mut self, operand: &MirOperand) {
        let reads_temp = match operand {
            MirOperand::Temp(id) => *id == self.temp,
            MirOperand::Place(place) => {
                matches!(place.base, MirPlaceBase::Temp(id) if id == self.temp)
            }
            MirOperand::Value(_) | MirOperand::Constant(_) => false,
        };
        self.found |= reads_temp;
        radix_mir::visit::walk_operand(self, operand);
    }

    fn visit_place(&mut self, place: &MirPlace) {
        // A plain destination write is not a read, but a projected write
        // (`temp[i] ← x`) mutates the temp's own storage.
        let writes_through = !place.projections.is_empty()
            && matches!(place.base, MirPlaceBase::Temp(id) if id == self.temp);
        self.found |= writes_through;
        radix_mir::visit::walk_place(self, place);
    }
}

#[derive(Debug, Clone)]
struct PlaceAlias {
    /// Frame containing the caller place. The caller remains on the stack for
    /// the lifetime of the callee frame, so this is a stable in-runner cell.
    frame: usize,
    /// Only the base is kept: an alias is created for a projection-free
    /// caller place, so there are no projections to replay.
    base: MirPlaceBase,
}

struct Frame {
    function: MirFunctionId,
    /// Slot-indexed by `MirLocalId` (dense per function; `None` = never
    /// written). A hash map here measured -5.4% cycles to remove on A2.
    locals: Vec<Option<Value>>,
    /// `in` parameter locals that borrow a caller place instead of owning a
    /// cloned scalar value.
    aliases: FxHashMap<MirLocalId, PlaceAlias>,
    /// Pending `mut` write-backs for projected caller places, applied when
    /// this frame returns: a `mut` parameter writes back to whatever place
    /// the caller passed, projected or not (CTO ruling, need).
    /// Each entry is (caller frame index, caller place, callee local).
    mut_writebacks: Vec<(usize, MirPlace, MirLocalId)>,
    /// Slot-indexed by `MirTempId` (`None` = never written).
    temps: Vec<Option<Value>>,
    /// Statement results readable through `MirOperand::Value`. Filled by
    /// `Assign` only when `records_values` is set, and by the edge-callback
    /// argument slots.
    values: FxHashMap<MirValueId, Value>,
    /// `true` when the frame's function reads a `MirOperand::Value`; every
    /// other function skips the per-`Assign` value record.
    records_values: bool,
    block: MirBlockId,
    /// First statement of `block` to run: nonzero only for a generator frame
    /// resumed after the yielding statement it was parked on.
    resume_at: usize,
    /// `true` once this generator frame has been parked at a `cede`, so its
    /// next push resumes the same call rather than starting a new one.
    resumed: bool,
    /// `true` while this frame runs a `@ cursor` generator whose `cede` parks
    /// the frame and hands the item to [`Runner::yielded`] rather than awaiting.
    is_generator: bool,
    /// Handled blocks open in this frame, innermost last (`fac … cape`,
    /// `fac omnia … cape`). Maintained by the device-block markers; empty for
    /// every program without a `⇥` kernel.
    blocks: Vec<OpenBlock>,
}

/// One open handled block (U9-5): the delivery target of a queued kernel error.
struct OpenBlock {
    /// Unique per dynamic entry, increasing; the owner tag of its launches.
    id: u64,
    omnia: bool,
    handler: MirBlockId,
    error_place: MirPlace,
}

enum CallOutcome {
    Ok(Value),
    Err(Value),
}

#[cfg(test)]
impl Frame {
    /// Test-frame seeding: grow the slot storage to hold `id` (hand-built
    /// fixtures may not declare every written slot).
    fn seed_local(&mut self, id: MirLocalId, value: Value) {
        let index = id.0 as usize;
        if self.locals.len() <= index {
            self.locals.resize(index + 1, None);
        }
        self.locals[index] = Some(value);
    }
}

/// Write one slot of a frame's slot-indexed storage. An id outside the
/// function's declared layout is an internal error (validation guarantees the
/// layout covers every written id).
pub(crate) fn write_slot(
    slots: &mut [Option<Value>],
    index: usize,
    value: Value,
    span: Span,
    kind: &str,
) -> Result<(), RunnerError> {
    match slots.get_mut(index) {
        Some(slot) => {
            *slot = Some(value);
            Ok(())
        }
        None => Err(RunnerError::internal(
            span,
            format!("write to undeclared {kind} slot {index}"),
        )),
    }
}

/// Per-function slot layout: the declared static type of every local and temp
/// id, indexed by the id (locals also cover `in`-parameter slots), plus the
/// parameter local ids a frame pre-seeds. Built once at construction so the
/// hot paths index a `Vec` instead of scanning the function's declarations
/// (the linear scan was 8.5% of A2 samples).
pub(crate) struct FunctionSlots {
    locals: Vec<Option<TypeId>>,
    temps: Vec<Option<TypeId>>,
    param_locals: Vec<MirLocalId>,
    /// `(local, mode)` of every parameter in declaration order, shared so a
    /// call reads it with a refcount bump instead of rebuilding a `Vec`.
    params: Rc<[(MirLocalId, MirParamMode)]>,
    /// Locals whose declared type is a bare `f32` (through aliases): a float
    /// stored there rounds to binary32. A `PolicyFloat` cell is not flagged;
    /// it keeps the bare float and its own store check.
    f32_locals: Vec<bool>,
    /// Temps whose declared type is a bare `f32`, like `f32_locals`.
    f32_temps: Vec<bool>,
}

impl FunctionSlots {
    pub(crate) fn local_ty(&self, id: MirLocalId) -> Option<TypeId> {
        self.locals.get(id.0 as usize).copied().flatten()
    }

    pub(crate) fn temp_ty(&self, id: MirTempId) -> Option<TypeId> {
        self.temps.get(id.0 as usize).copied().flatten()
    }

    fn local_is_f32(&self, id: MirLocalId) -> bool {
        self.f32_locals.get(id.0 as usize).copied().unwrap_or(false)
    }

    fn temp_is_f32(&self, id: MirTempId) -> bool {
        self.f32_temps.get(id.0 as usize).copied().unwrap_or(false)
    }
}

/// `true` when `ty`, resolved through aliases, is the bare `f32` float type.
fn is_bare_f32(types: &TypeTable, mut ty: TypeId) -> bool {
    loop {
        match types.get(ty) {
            Type::Alias(_, inner) => ty = *inner,
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32) => return true,
            _ => return false,
        }
    }
}

fn f32_slot_flags(types: &TypeTable, slots: &[Option<TypeId>]) -> Vec<bool> {
    slots
        .iter()
        .map(|ty| ty.is_some_and(|ty| is_bare_f32(types, ty)))
        .collect()
}

fn slot_table_len(ids: impl Iterator<Item = u32>) -> usize {
    ids.map(|id| id as usize + 1).max().unwrap_or(0)
}

fn index_slot_types(
    program: &MirProgram,
    types: &TypeTable,
) -> FxHashMap<MirFunctionId, FunctionSlots> {
    let mut table = FxHashMap::default();
    for function in &program.functions {
        let mut locals = vec![
            None;
            slot_table_len(
                function
                    .locals
                    .iter()
                    .map(|local| local.id.0)
                    .chain(function.params.iter().map(|param| param.local.0))
            )
        ];
        // Params first: a declared local wins the slot, matching the
        // locals-then-params lookup order this table replaces.
        let mut param_locals = Vec::with_capacity(function.params.len());
        for param in &function.params {
            locals[param.local.0 as usize] = Some(param.ty.semantic_id());
            param_locals.push(param.local);
        }
        for local in &function.locals {
            locals[local.id.0 as usize] = Some(local.ty.semantic_id());
        }
        let mut temps = vec![None; slot_table_len(function.temps.iter().map(|temp| temp.id.0))];
        for temp in &function.temps {
            temps[temp.id.0 as usize] = Some(temp.ty.semantic_id());
        }
        let f32_locals = f32_slot_flags(types, &locals);
        let f32_temps = f32_slot_flags(types, &temps);
        table.insert(
            function.id,
            FunctionSlots {
                locals,
                temps,
                param_locals,
                params: function
                    .params
                    .iter()
                    .map(|param| (param.local, param.mode))
                    .collect(),
                f32_locals,
                f32_temps,
            },
        );
    }
    table
}

enum StatementOutcome {
    Continue,
    FrameReturned,
}

/// Result of a run that was admitted and started with statistics enabled.
///
/// Validation and route failures happen before a runner starts and are
/// returned as the outer `Err`, while runtime failures are retained alongside
/// the completed statistics snapshot so callers can emit diagnostics and the
/// artifact together.
#[derive(Debug, Clone, PartialEq)]
pub struct RunnerRunOutcome {
    pub result: Result<(), Vec<RunnerError>>,
    pub stats: RunnerStats,
    /// Conversation router slots (D8.3), read after every frame closed, so
    /// `live` counts conversations the last-reference net did not release.
    pub conversations: sermo::ConversationLedger,
    /// The entry's returned value, when the run completed a `Return` that
    /// carried one. `None` for a vacuum return, a `ReturnError` (the trap
    /// carries the failure instead), or a run that failed before returning.
    pub return_value: Option<Value>,
    /// The kernel calls captured by a traced run
    /// ([`run_function_with_kernel_trace`]); `None` when tracing was off.
    pub kernel_trace: Option<kernel_trace::KernelTrace>,
}

type RegexLiteralMap = FxHashMap<(Symbol, Option<Symbol>), Rc<RegexProgram>>;

/// The immutable whole-program indexes a [`Runner`] reads: the block cache,
/// function and definition indexes, value and slot types, and the field
/// display indexes.
///
/// Building them costs time proportional to the whole program (every block is
/// cloned into the cache). A caller that runs one program many times, such as
/// a device worker launching a kernel chunk after chunk, builds this once and
/// runs every runner over it (see
/// [`kernel_lanes::run_device_form_lanes_prepared`]).
///
/// A prepared value is valid only for the `(program, context, types)` it was
/// built from; it holds `Rc` blocks, so it is per thread.
pub struct RunnerPrepared {
    function_index: MirFunctionIndex,
    /// `DefId` → `MirFunctionId` index (BO-M-6).
    def_to_function: FxHashMap<DefId, MirFunctionId>,
    /// Per-step block share: one clone at construct, `Rc` bump in the loop (BO-M-1).
    block_cache: FxHashMap<(MirFunctionId, MirBlockId), Rc<MirBlock>>,
    /// Value-id static types per function (`MirStatementKind::Assign`), for
    /// display-width resolution at diagnostics.
    value_types: FxHashMap<(MirFunctionId, MirValueId), MirType>,
    /// Functions whose body reads a `MirOperand::Value`; only their frames
    /// record `Assign` results.
    value_reading_functions: FxHashSet<MirFunctionId>,
    /// Declared local/temp slot types per function (O(1) id lookups).
    slot_types: FxHashMap<MirFunctionId, FunctionSlots>,
    /// Declared non-f64 float widths of struct fields, for width-aware display.
    field_float_widths: value::FieldFloatWidths,
    /// Struct and variant fields holding a `littera`, so display quotes them
    /// as chars and every other ascii-carried text as text (pr4).
    field_char_set: value::FieldCharSet,
    field_bits_widths: value::FieldBitsWidths,
    /// Struct and variant fields whose declared type holds a tuple, so display
    /// prints them as `iuncta [..]` at their element widths.
    field_tuple_parts: value::FieldTupleParts,
}

impl RunnerPrepared {
    /// Index `program` once for any number of runners.
    #[must_use]
    pub fn new(
        program: &MirProgram,
        context: &MirValidationContext<'_>,
        types: &TypeTable,
    ) -> Self {
        let mut def_to_function = FxHashMap::default();
        let mut block_cache = FxHashMap::default();
        for function in &program.functions {
            if let Some(def_id) = function.source {
                def_to_function.insert(def_id, function.id);
            }
            for block in &function.blocks {
                block_cache.insert((function.id, block.id), Rc::new(block.clone()));
            }
        }
        Self {
            function_index: program.function_index(),
            def_to_function,
            block_cache,
            value_types: index_value_types(program),
            value_reading_functions: index_value_reading_functions(program),
            slot_types: index_slot_types(program, types),
            field_float_widths: index_field_float_widths(context, types),
            field_char_set: index_field_char_set(context, types),
            field_bits_widths: index_field_bits_widths(context, types),
            field_tuple_parts: index_field_tuple_parts(context, types),
        }
    }
}

/// A runner's indexes: built for it alone, or shared with other runners.
enum PreparedRef<'a> {
    Owned(Box<RunnerPrepared>),
    Borrowed(&'a RunnerPrepared),
}

impl std::ops::Deref for PreparedRef<'_> {
    type Target = RunnerPrepared;

    fn deref(&self) -> &RunnerPrepared {
        match self {
            Self::Owned(prepared) => prepared,
            Self::Borrowed(prepared) => prepared,
        }
    }
}

pub(crate) struct Runner<'a, H: Host + ?Sized> {
    program: &'a MirProgram,
    prepared: PreparedRef<'a>,
    context: &'a MirValidationContext<'a>,
    types: &'a TypeTable,
    interner: &'a Interner,
    host: &'a mut H,
    frames: Vec<Frame>,
    /// Emptied slot buffers of returned frames, reused by the next call.
    slot_pool: Vec<Vec<Option<Value>>>,
    /// Optional diagnostic meter. `None` is the unmetered fast path: no
    /// statistics clock reads, active-stack work, or row lookups occur.
    stats: Option<stats::StatsCollector>,
    errors: Vec<RunnerError>,
    /// Remaining-capacity counter: `None` means unlimited. One unit is one
    /// MIR basic-block visit in `run_until_depth`.
    step_limit: Option<u64>,
    steps_taken: u64,
    /// Cooperative wall-clock deadline for the test path. Checked every
    /// `DEADLINE_CHECK_INTERVAL` steps in `consume_step` so the common case
    /// pays no clock read; program execution (`*_unbounded`) leaves it `None`.
    deadline: Option<Instant>,
    return_value: Option<Value>,
    return_error: Option<Value>,
    /// Item handed over by `cede` in a generator frame (`Frame::is_generator`).
    /// While set, `run_until_depth` parks the generator frame after the
    /// yielding statement and stops; the pulling cursor takes the item.
    yielded: Option<Value>,
    /// Opaque gradient-handle storage: handle id → accumulated gradient
    /// tensor (`MirIntrinsic::Gradient` dispatch). Clean-break internal
    /// contract mirroring the host LLVM gradient ABI; in-process only.
    gradient_handles: FxHashMap<u64, GradientStorage>,
    /// Next gradient handle id (monotonic, never reused).
    next_gradient_handle: u64,
    /// Compiled regex literals, keyed by their interned pattern and flags.
    regex_literals: RefCell<RegexLiteralMap>,
    /// Decoded `textus` literals, keyed by interned source symbol.
    textus_literals: RefCell<FxHashMap<Symbol, String>>,
    /// Owned ASCII literals, keyed by interned source symbol.
    ascii_literals: RefCell<FxHashMap<Symbol, String>>,
    /// Whether a call to a compute-stage (`@ kernel`) function runs or stops
    /// the run. Defaults to [`RunnerKernelPolicy::Refuse`].
    kernel_policy: RunnerKernelPolicy,
    /// Receives compute-stage calls under [`RunnerKernelPolicy::Dispatch`].
    kernel_dispatcher: Option<kernel_dispatch::ResidencyHook>,
    /// The kernel calls captured so far, when the run is traced (the C3
    /// comparator). `None` leaves every call path untouched.
    kernel_trace: Option<kernel_trace::KernelTrace>,
    /// How many traced kernel bodies are executing: a kernel called from a
    /// kernel is part of the outer call and is not a record of its own.
    kernel_trace_depth: usize,
    /// Next open-block id (see [`OpenBlock::id`]).
    next_block_id: u64,
    /// A delivered kernel error whose owner block is open in a caller frame;
    /// it waits there for the caller's next delivery point.
    carried_fault: Option<kernel_dispatch::DeviceFault>,
    /// Last conversation id handed out by `SermoOpen` (§2.7 record).
    next_conversation_id: u64,
    /// Router slots held by open conversations (D8.3); each conversation
    /// gives its slot back at its release point.
    conversation_slots: Rc<sermo::ConversationSlots>,
}

impl<'a, H: Host + ?Sized> Runner<'a, H> {
    /// Build a runner with O(1) lookup indexes for functions, defs, and blocks.
    ///
    /// The indexes are built for this runner alone; callers that start many
    /// runners on one program build a [`RunnerPrepared`] once and use
    /// [`Runner::with_prepared`].
    pub(crate) fn new(
        program: &'a MirProgram,
        context: &'a MirValidationContext<'a>,
        types: &'a TypeTable,
        interner: &'a Interner,
        host: &'a mut H,
    ) -> Self {
        let prepared = PreparedRef::Owned(Box::new(RunnerPrepared::new(program, context, types)));
        Self::from_prepared(program, context, types, interner, host, prepared)
    }

    /// Build a runner over indexes prepared earlier for the same
    /// `(program, context, types)`.
    pub(crate) fn with_prepared(
        prepared: &'a RunnerPrepared,
        program: &'a MirProgram,
        context: &'a MirValidationContext<'a>,
        types: &'a TypeTable,
        interner: &'a Interner,
        host: &'a mut H,
    ) -> Self {
        Self::from_prepared(
            program,
            context,
            types,
            interner,
            host,
            PreparedRef::Borrowed(prepared),
        )
    }

    fn from_prepared(
        program: &'a MirProgram,
        context: &'a MirValidationContext<'a>,
        types: &'a TypeTable,
        interner: &'a Interner,
        host: &'a mut H,
        prepared: PreparedRef<'a>,
    ) -> Self {
        Self {
            prepared,
            program,
            context,
            types,
            interner,
            host,
            frames: Vec::new(),
            slot_pool: Vec::new(),
            stats: None,
            errors: Vec::new(),
            step_limit: None,
            steps_taken: 0,
            deadline: None,
            return_value: None,
            return_error: None,
            yielded: None,
            gradient_handles: FxHashMap::default(),
            next_gradient_handle: 0,
            regex_literals: RefCell::new(FxHashMap::default()),
            textus_literals: RefCell::new(FxHashMap::default()),
            ascii_literals: RefCell::new(FxHashMap::default()),
            kernel_policy: RunnerKernelPolicy::Refuse,
            kernel_dispatcher: None,
            kernel_trace: None,
            kernel_trace_depth: 0,
            next_block_id: 1,
            carried_fault: None,
            next_conversation_id: 0,
            conversation_slots: Rc::default(),
        }
    }

    /// The output tokens of the function now running: its entry in the image's
    /// display-token table, or Latin when it has none (library-linked
    /// functions carry no entry).
    fn display_tokens(&self) -> &DisplayTokens {
        self.frames
            .last()
            .and_then(|frame| self.context.display_tokens.for_function(frame.function))
            .unwrap_or_else(|| value::FieldOrder::latin_tokens())
    }

    /// Display metadata (field orders and declared float widths) for value
    /// rendering.
    pub(crate) fn field_order(&self) -> value::FieldOrder<'_> {
        value::FieldOrder {
            structs: &self.context.struct_field_order,
            variants: &self.context.variant_field_order,
            float_widths: &self.prepared.field_float_widths,
            char_fields: &self.prepared.field_char_set,
            tuple_fields: &self.prepared.field_tuple_parts,
            tuple: None,
            bits_fields: &self.prepared.field_bits_widths,
            bits: None,
            interval: false,
            char_text: false,
            names: &self.context.display_names,
            tokens: self.display_tokens(),
            dynamic: false,
        }
    }
}

/// The element type of a collection or option wrapper, which a value of that
/// type renders through.
fn wrapped_element(ty: TypeId, types: &TypeTable) -> Option<TypeId> {
    match types.get(ty) {
        Type::Map(_, elem)
        | Type::Option(elem)
        | Type::Queue(elem)
        | Type::Stack(elem)
        | Type::Vector(elem, _)
        | Type::Matrix(elem, _)
        | Type::Tensor(elem, _)
        | Type::Sparsa(elem, _)
        | Type::Array(elem)
        | Type::BoundedArray { element: elem, .. }
        | Type::RangedArray { element: elem, .. } => Some(*elem),
        _ => None,
    }
}

fn element_display_width(ty: TypeId, types: &TypeTable, depth: u8) -> Option<NumericWidth> {
    if depth > 8 {
        return None;
    }
    match types.get(ty) {
        Type::SizedNumeric(_, width) if width.is_float() || width.is_decimal() => Some(*width),
        Type::PolicyFloat(_, width) => Some(*width),
        _ => element_display_width(wrapped_element(ty, types)?, types, depth + 1),
    }
}

/// Display shape of `ty` when it is (or holds, through collections and
/// options) a tuple: each element's width and nested shape come from the
/// declared tuple type. An interval has its own type and gets no shape.
fn tuple_parts(ty: TypeId, types: &TypeTable, depth: u8) -> Option<value::TupleParts> {
    if depth > 8 {
        return None;
    }
    match types.get(ty) {
        Type::Tuple(elements) => Some(value::TupleParts(
            elements
                .iter()
                .map(|element| value::TuplePart {
                    width: element_display_width(*element, types, 0),
                    bits: holds_bits(*element, types, 0),
                    tuple: tuple_parts(*element, types, depth + 1),
                })
                .collect(),
        )),
        _ => tuple_parts(wrapped_element(ty, types)?, types, depth + 1),
    }
}

/// Whether `ty` is (or holds, through collections and options) a dynamic
/// `valor`/`json` carrier: such a value displays as a JSON-shaped object even
/// when the runner's representation is a typed `genus` value.
fn dynamic_display_type(ty: TypeId, types: &TypeTable, depth: u8) -> bool {
    if depth > 8 {
        return false;
    }
    match types.get(ty) {
        Type::Primitive(Primitive::Valor | Primitive::Json) => true,
        Type::Map(_, elem)
        | Type::Option(elem)
        | Type::Queue(elem)
        | Type::Stack(elem)
        | Type::Array(elem)
        | Type::BoundedArray { element: elem, .. }
        | Type::RangedArray { element: elem, .. } => dynamic_display_type(*elem, types, depth + 1),
        _ => false,
    }
}

/// Whether `ty` is (or holds, through collections and options) a `littera`:
/// such a value prints as a char literal inside a composite, where any other
/// ascii-carried text prints double-quoted.
fn holds_littera(ty: TypeId, types: &TypeTable, depth: u8) -> bool {
    if depth > 8 {
        return false;
    }
    match types.get(ty) {
        Type::Primitive(Primitive::Littera) => true,
        Type::Map(_, elem)
        | Type::Option(elem)
        | Type::Queue(elem)
        | Type::Stack(elem)
        | Type::Vector(elem, _)
        | Type::Matrix(elem, _)
        | Type::Tensor(elem, _)
        | Type::Sparsa(elem, _)
        | Type::Array(elem)
        | Type::BoundedArray { element: elem, .. }
        | Type::RangedArray { element: elem, .. } => holds_littera(*elem, types, depth + 1),
        _ => false,
    }
}

/// Whether `ty` is (or holds, through collections and options) an
/// `intervallum`: such a value carries its interval as a tuple-shaped
/// `Value::Tuple` and prints as its construction literal (`2‥9`).
fn holds_interval(ty: TypeId, types: &TypeTable, depth: u8) -> bool {
    if depth > 8 {
        return false;
    }
    match types.get(ty) {
        Type::Intervallum(..) => true,
        Type::Map(_, elem)
        | Type::Option(elem)
        | Type::Queue(elem)
        | Type::Stack(elem)
        | Type::Vector(elem, _)
        | Type::Matrix(elem, _)
        | Type::Tensor(elem, _)
        | Type::Sparsa(elem, _)
        | Type::Array(elem)
        | Type::BoundedArray { element: elem, .. }
        | Type::RangedArray { element: elem, .. } => holds_interval(*elem, types, depth + 1),
        _ => false,
    }
}

/// The raw-bits width of `ty` when it is (or holds, through collections and
/// options) a `bN`: such a value prints as hexadecimal at its full width.
fn holds_bits(ty: TypeId, types: &TypeTable, depth: u8) -> Option<radix_types::BitsWidth> {
    if depth > 8 {
        return None;
    }
    match types.get(ty) {
        Type::Bits(width) => Some(*width),
        _ => holds_bits(wrapped_element(ty, types)?, types, depth + 1),
    }
}

fn index_field_bits_widths(
    context: &MirValidationContext<'_>,
    types: &TypeTable,
) -> value::FieldBitsWidths {
    let mut widths = FxHashMap::default();
    let owners = context
        .struct_fields
        .iter()
        .chain(context.variant_fields.iter());
    for (def, fields) in owners {
        for (name, ty) in fields {
            if let Some(width) = holds_bits(ty.semantic_id(), types, 0) {
                widths.insert((*def, *name), width);
            }
        }
    }
    widths
}

fn index_field_char_set(
    context: &MirValidationContext<'_>,
    types: &TypeTable,
) -> value::FieldCharSet {
    let mut set = FxHashSet::default();
    let owners = context
        .struct_fields
        .iter()
        .chain(context.variant_fields.iter());
    for (def, fields) in owners {
        for (name, ty) in fields {
            if holds_littera(ty.semantic_id(), types, 0) {
                set.insert((*def, *name));
            }
        }
    }
    set
}

fn index_field_tuple_parts(
    context: &MirValidationContext<'_>,
    types: &TypeTable,
) -> value::FieldTupleParts {
    let mut parts = FxHashMap::default();
    let owners = context
        .struct_fields
        .iter()
        .chain(context.variant_fields.iter());
    for (def, fields) in owners {
        for (name, ty) in fields {
            if let Some(shape) = tuple_parts(ty.semantic_id(), types, 0) {
                parts.insert((*def, *name), shape);
            }
        }
    }
    parts
}

fn index_field_float_widths(
    context: &MirValidationContext<'_>,
    types: &TypeTable,
) -> value::FieldFloatWidths {
    let mut widths = FxHashMap::default();
    let owners = context
        .struct_fields
        .iter()
        .chain(context.variant_fields.iter());
    for (def, fields) in owners {
        for (name, ty) in fields {
            if let Some(width) = element_display_width(ty.semantic_id(), types, 0)
                && width != NumericWidth::F64
            {
                widths.insert((*def, *name), width);
            }
        }
    }
    widths
}

/// Run the entry (`incipit`) function to completion.
///
/// Program execution (not a proba test): explicitly unbounded — no step
/// budget, no wall-clock deadline. See [`run_function_unbounded`].
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be executed.
pub fn run_entry<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    host: &mut H,
) -> Result<(), Vec<RunnerError>> {
    run_entry_with_kernel_policy(validated, host, RunnerKernelPolicy::Refuse)
}

/// [`run_entry`] under an explicit [`RunnerKernelPolicy`]: the CPU device
/// route passes [`RunnerKernelPolicy::Execute`]; every other route uses
/// [`run_entry`], which refuses kernels.
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be executed.
pub fn run_entry_with_kernel_policy<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    host: &mut H,
    kernel_policy: RunnerKernelPolicy,
) -> Result<(), Vec<RunnerError>> {
    let entry = explicit_entry_id(validated)?;
    run_function_with_kernel_policy(validated, entry, host, kernel_policy)
}

/// [`run_entry`] for a process host: after the body completes normally, the
/// expression is evaluated and becomes the process exit status; a trap or
/// unhandled failure still exits with the trap's code.
///
/// A trap aborts the frame before the tail `Return` runs, so no exit status is
/// applied for it — the trap's own code already reached the host. A successful
/// entry that returns a `numerus` hands its value to [`Host::exit`]; a vacuum
/// entry returns `Ok(())` without exiting.
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be executed or the
/// entry fails at run time.
pub fn run_entry_with_process_exit<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    host: &mut H,
    kernel_policy: RunnerKernelPolicy,
) -> Result<(), Vec<RunnerError>> {
    let entry = explicit_entry_id(validated)?;
    let outcome = run_function_with_explicit_limits_and_stats(
        validated,
        entry,
        host,
        None,
        None,
        false,
        kernel_policy,
    )?;
    outcome.result?;
    if let Some(Value::Int(code)) = outcome.return_value {
        // WHY: a process exit status is `i32`, while the Faber `numerus`
        // carrier is `i64`; the host truncates exactly as the trap path's
        // `exit_code_from_i32` does.
        #[allow(clippy::cast_possible_truncation)]
        let exit_code = code as i32;
        host.exit(exit_code);
    }
    Ok(())
}

/// Run a specific MIR function to completion, unbounded, under an explicit
/// [`RunnerKernelPolicy`].
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be executed.
pub fn run_function_with_kernel_policy<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    kernel_policy: RunnerKernelPolicy,
) -> Result<(), Vec<RunnerError>> {
    run_function_with_explicit_limits(validated, function_id, host, None, None, kernel_policy)
}

/// Run the entry (`incipit`) function under explicit interpreter limits.
///
/// The bounded counterpart to [`run_entry`]: in-process classifiers that
/// execute corpus programs (the coverage matrix) use this so a program with
/// unbounded work stops at a named limit instead of stalling the caller.
/// `step_budget: None` applies [`DEFAULT_TEST_STEP_BUDGET`]; a deadline is
/// checked cooperatively every [`DEADLINE_CHECK_INTERVAL`] steps.
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be executed, the step
/// budget is exhausted ([`RUNNER_ISSUE_STEP_BUDGET_EXHAUSTED`]), or the
/// wall-clock deadline passes ([`RUNNER_ISSUE_WALL_CLOCK_TIMEOUT`]).
pub fn run_entry_with_limits<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    host: &mut H,
    step_budget: Option<u64>,
    deadline: Option<Instant>,
) -> Result<(), Vec<RunnerError>> {
    let entry = explicit_entry_id(validated)?;
    run_function_with_limits(validated, entry, host, step_budget, deadline)
}

/// Resolve the explicit entry function's id, or the refusal for a program that
/// has none. Both entry runners share this so their pre-execution errors agree.
fn explicit_entry_id(validated: &ValidatedMir<'_>) -> Result<MirFunctionId, Vec<RunnerError>> {
    let context = validated.validation();
    let program = validated.program();
    let interner = context.interner.as_interner().ok_or_else(|| {
        vec![RunnerError::internal(
            Span::default(),
            "runner requires interner context",
        )]
    })?;
    let names = radix_mir::names::MirNames::new(program, context.types.as_table(), interner);
    names
        .explicit_entry_function()
        .map(|entry| entry.id)
        .ok_or_else(|| {
            vec![RunnerError::internal(
                Span::default(),
                radix_mir::names::NO_ENTRY_FUNCTION,
            )]
        })
}

/// Run the explicit entry function with the diagnostic meter armed.
///
/// The outer error is reserved for failures before execution starts.  Once the
/// entry frame is pushed, runtime errors are returned in
/// [`RunnerRunOutcome::result`] together with the collected snapshot.
///
/// # Errors
///
/// Returns pre-execution runner errors when the validation context has no
/// interner or the MIR has no explicit entry function.
pub fn run_entry_with_stats<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    host: &mut H,
) -> Result<RunnerRunOutcome, Vec<RunnerError>> {
    run_entry_with_stats_and_kernel_policy(validated, host, RunnerKernelPolicy::Refuse)
}

/// [`run_entry_with_stats`] under an explicit [`RunnerKernelPolicy`].
///
/// # Errors
///
/// Returns pre-execution runner errors when the validation context has no
/// interner or the MIR has no explicit entry function.
pub fn run_entry_with_stats_and_kernel_policy<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    host: &mut H,
    kernel_policy: RunnerKernelPolicy,
) -> Result<RunnerRunOutcome, Vec<RunnerError>> {
    let entry = explicit_entry_id(validated)?;
    run_function_with_explicit_limits_and_stats(
        validated,
        entry,
        host,
        None,
        None,
        true,
        kernel_policy,
    )
}

/// Run a specific MIR function with the diagnostic meter armed.
///
/// The outer error is reserved for failures before execution starts.  Once the
/// function frame is pushed, runtime errors are returned in
/// [`RunnerRunOutcome::result`] together with the collected snapshot.
///
/// # Errors
///
/// Returns pre-execution runner errors when the validation context is missing
/// an interner, the function id is unknown, or the MIR is refused before start.
pub fn run_function_with_stats<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
) -> Result<RunnerRunOutcome, Vec<RunnerError>> {
    run_function_with_explicit_limits_and_stats(
        validated,
        function_id,
        host,
        None,
        None,
        true,
        RunnerKernelPolicy::Refuse,
    )
}

/// Run a specific MIR function with the diagnostic meter armed and explicit
/// interpreter limits.  This is the stats-capable counterpart to
/// [`run_function_with_limits`].
///
/// # Errors
///
/// Returns pre-execution runner errors when the validation context is missing
/// an interner, the function id is unknown, or the MIR is refused before start.
pub fn run_function_with_stats_and_limits<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    step_budget: Option<u64>,
    deadline: Option<Instant>,
) -> Result<RunnerRunOutcome, Vec<RunnerError>> {
    run_function_with_stats_and_limits_and_kernel_policy(
        validated,
        function_id,
        host,
        step_budget,
        deadline,
        RunnerKernelPolicy::Refuse,
    )
}

/// [`run_function_with_stats_and_limits`] under an explicit
/// [`RunnerKernelPolicy`]: the proba CPU oracle route passes
/// [`RunnerKernelPolicy::Execute`].
///
/// # Errors
///
/// Returns pre-execution runner errors when the validation context is missing
/// an interner, the function id is unknown, or the MIR is refused before start.
pub fn run_function_with_stats_and_limits_and_kernel_policy<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    step_budget: Option<u64>,
    deadline: Option<Instant>,
    kernel_policy: RunnerKernelPolicy,
) -> Result<RunnerRunOutcome, Vec<RunnerError>> {
    run_function_with_explicit_limits_and_stats(
        validated,
        function_id,
        host,
        Some(step_budget.unwrap_or(DEFAULT_TEST_STEP_BUDGET)),
        deadline,
        true,
        kernel_policy,
    )
}

/// Run a specific MIR function with the diagnostic meter armed and a step
/// budget.  `None` applies [`DEFAULT_TEST_STEP_BUDGET`].
///
/// # Errors
///
/// Returns pre-execution runner errors when the validation context is missing
/// an interner, the function id is unknown, or the MIR is refused before start.
pub fn run_function_with_stats_and_step_budget<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    step_budget: Option<u64>,
) -> Result<RunnerRunOutcome, Vec<RunnerError>> {
    run_function_with_stats_and_limits(validated, function_id, host, step_budget, None)
}

/// Run a specific MIR function to completion.
///
/// Program execution: explicitly unbounded (see [`run_function_unbounded`]).
/// The proba test path uses [`run_function_with_limits`] instead, which
/// applies a default step budget and an optional wall-clock deadline.
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be executed.
pub fn run_function<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
) -> Result<(), Vec<RunnerError>> {
    run_function_unbounded(validated, function_id, host)
}

/// Run a specific MIR function with NO step budget and NO deadline.
///
/// The explicit-unbounded variant reserved for program execution
/// (`faber run`, FMIR image runner, coverage). Test paths must not call this:
/// they inherit [`DEFAULT_TEST_STEP_BUDGET`] through
/// [`run_function_with_limits`] instead.
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be executed.
pub fn run_function_unbounded<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
) -> Result<(), Vec<RunnerError>> {
    run_function_with_explicit_limits(
        validated,
        function_id,
        host,
        None,
        None,
        RunnerKernelPolicy::Refuse,
    )
}

/// Default step budget applied to test executions when the caller passes no
/// explicit budget (`None`). One step is one MIR basic-block visit. Generous:
/// real tests never trip it; only unbounded work (infinite loops, runaway
/// recursion) does. Tunable later via a max-steps-style flag.
pub const DEFAULT_TEST_STEP_BUDGET: u64 = 100_000_000;

/// Run a specific MIR function with an optional deterministic step budget.
///
/// One step is one MIR basic-block visit. `None` applies
/// [`DEFAULT_TEST_STEP_BUDGET`] — test paths inherit the default guard.
/// Exhaustion records `interpreter step budget exhausted after N steps` and
/// stops. No wall-clock deadline is applied; use [`run_function_with_limits`].
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be executed or the
/// step budget is exhausted.
pub fn run_function_with_step_budget<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    step_budget: Option<u64>,
) -> Result<(), Vec<RunnerError>> {
    run_function_with_limits(validated, function_id, host, step_budget, None)
}

/// Run a specific MIR function with the test-path limits: an optional step
/// budget (`None` applies [`DEFAULT_TEST_STEP_BUDGET`]) and an optional
/// wall-clock deadline (checked cooperatively every
/// [`DEADLINE_CHECK_INTERVAL`] steps).
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be executed, the
/// step budget is exhausted, or the wall-clock deadline passes.
pub fn run_function_with_limits<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    step_budget: Option<u64>,
    deadline: Option<Instant>,
) -> Result<(), Vec<RunnerError>> {
    run_function_with_limits_and_kernel_policy(
        validated,
        function_id,
        host,
        step_budget,
        deadline,
        RunnerKernelPolicy::Refuse,
    )
}

/// [`run_function_with_limits`] under an explicit [`RunnerKernelPolicy`]: the
/// proba CPU oracle route passes [`RunnerKernelPolicy::Execute`].
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be executed, the
/// step budget is exhausted, or the wall-clock deadline passes.
pub fn run_function_with_limits_and_kernel_policy<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    step_budget: Option<u64>,
    deadline: Option<Instant>,
    kernel_policy: RunnerKernelPolicy,
) -> Result<(), Vec<RunnerError>> {
    run_function_with_explicit_limits(
        validated,
        function_id,
        host,
        Some(step_budget.unwrap_or(DEFAULT_TEST_STEP_BUDGET)),
        deadline,
        kernel_policy,
    )
}

/// Shared execution core. `step_limit: None` here means truly unlimited
/// (callers translate their own `None` into the default beforehand).
fn run_function_with_explicit_limits<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    step_limit: Option<u64>,
    deadline: Option<Instant>,
    kernel_policy: RunnerKernelPolicy,
) -> Result<(), Vec<RunnerError>> {
    let outcome = run_function_with_explicit_limits_and_stats(
        validated,
        function_id,
        host,
        step_limit,
        deadline,
        false,
        kernel_policy,
    )?;
    outcome.result
}

/// Run a specific MIR function under [`RunnerKernelPolicy::Dispatch`]: every
/// call to a compute-stage function is handed to `dispatcher`.
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be executed.
pub fn run_function_with_kernel_dispatcher<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    dispatcher: &kernel_dispatch::ResidencyHook,
) -> Result<(), Vec<RunnerError>> {
    run_function_dispatching(
        validated,
        function_id,
        host,
        None,
        None,
        false,
        RunnerKernelPolicy::Dispatch,
        Some(Rc::clone(dispatcher)),
        false,
    )?
    .result
}

/// [`run_function_with_kernel_dispatcher`] with the opt-in diagnostic meter
/// armed: the outcome carries the host-side [`RunnerStats`] beside the result.
/// Kernel dispatches are not measured; the meter observes host functions and
/// intrinsics only.
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be started.
pub fn run_function_with_kernel_dispatcher_and_stats<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    dispatcher: &kernel_dispatch::ResidencyHook,
) -> Result<RunnerRunOutcome, Vec<RunnerError>> {
    run_function_dispatching(
        validated,
        function_id,
        host,
        None,
        None,
        true,
        RunnerKernelPolicy::Dispatch,
        Some(Rc::clone(dispatcher)),
        false,
    )
}

/// [`run_function_with_kernel_dispatcher`] under the test-path limits of
/// [`run_function_with_limits`] (`None` step budget applies
/// [`DEFAULT_TEST_STEP_BUDGET`]; an optional wall-clock deadline), with the
/// diagnostic meter armed when `collect_stats` is set. The proba route of a
/// device session (`faber test --device cpu`) enters here.
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be started.
pub fn run_function_with_limits_and_kernel_dispatcher<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    step_budget: Option<u64>,
    deadline: Option<Instant>,
    collect_stats: bool,
    dispatcher: &kernel_dispatch::ResidencyHook,
) -> Result<RunnerRunOutcome, Vec<RunnerError>> {
    run_function_dispatching(
        validated,
        function_id,
        host,
        Some(step_budget.unwrap_or(DEFAULT_TEST_STEP_BUDGET)),
        deadline,
        collect_stats,
        RunnerKernelPolicy::Dispatch,
        Some(Rc::clone(dispatcher)),
        false,
    )
}

fn run_function_with_explicit_limits_and_stats<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    step_limit: Option<u64>,
    deadline: Option<Instant>,
    collect_stats: bool,
    kernel_policy: RunnerKernelPolicy,
) -> Result<RunnerRunOutcome, Vec<RunnerError>> {
    run_function_dispatching(
        validated,
        function_id,
        host,
        step_limit,
        deadline,
        collect_stats,
        kernel_policy,
        None,
        false,
    )
}

/// Run a specific MIR function with the kernel trace armed (the C3
/// comparator): the outcome's [`RunnerRunOutcome::kernel_trace`] holds one
/// record per source-level kernel call, in call order.
///
/// `kernel_policy` is [`RunnerKernelPolicy::Execute`] for the reference run
/// or [`RunnerKernelPolicy::Dispatch`] (with `dispatcher`) for a device run.
/// A call is recorded after it completes: a kernel that fails, or leaves
/// through its error channel, is not recorded, and the records before it
/// stay. A device-resident tensor is downloaded to be recorded, so tracing a
/// device run changes its transfer counts; an ordinary run is untouched.
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be started.
pub fn run_function_with_kernel_trace<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    kernel_policy: RunnerKernelPolicy,
    dispatcher: Option<&kernel_dispatch::ResidencyHook>,
) -> Result<RunnerRunOutcome, Vec<RunnerError>> {
    run_function_dispatching(
        validated,
        function_id,
        host,
        None,
        None,
        false,
        kernel_policy,
        dispatcher.map(Rc::clone),
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_function_dispatching<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    step_limit: Option<u64>,
    deadline: Option<Instant>,
    collect_stats: bool,
    kernel_policy: RunnerKernelPolicy,
    dispatcher: Option<kernel_dispatch::ResidencyHook>,
    trace_kernels: bool,
) -> Result<RunnerRunOutcome, Vec<RunnerError>> {
    let context = validated.validation();
    let program = validated.program();
    let interner = context.interner.as_interner().ok_or_else(|| {
        vec![RunnerError::internal(
            Span::default(),
            "runner requires interner context",
        )]
    })?;
    let Some(function) = validated.function_by_id(function_id) else {
        return Err(vec![RunnerError::internal(
            Span::default(),
            format!("no MIR function with id {}", function_id.0),
        )]);
    };
    if kernel_policy == RunnerKernelPolicy::Refuse
        && function.shader_stage == Some(MirKernelShaderStage::Compute)
    {
        return Err(vec![kernel_refused(function.span)]);
    }
    let mut runner = Runner::new(program, context, context.types.as_table(), interner, host);
    runner.step_limit = step_limit;
    runner.deadline = deadline;
    runner.kernel_policy = kernel_policy;
    runner.kernel_dispatcher = dispatcher;
    if trace_kernels {
        runner.kernel_trace = Some(Vec::new());
    }
    if collect_stats {
        runner.stats = Some(stats::StatsCollector::new(program, context, interner));
    }
    runner.push_frame(function_id, MirBlockId(0));
    runner.run_until_depth(0);
    runner.close_all_frames();
    // Program end is a delivery point: complete device work and surface a
    // deferred fault after any error already recorded (first in program order).
    // A fault a host read already surfaced is also kept as the dispatcher's
    // deferred error; it is the same fault, reported once.
    if let Err(error) = runner.finish_dispatcher()
        && !runner.errors.contains(&error)
    {
        runner.errors.push(error);
    }
    let result = if runner.errors.is_empty() {
        Ok(())
    } else {
        Err(std::mem::take(&mut runner.errors))
    };
    let stats = runner.take_stats().unwrap_or_default();
    let conversations = runner.conversation_slots.ledger();
    let return_value = runner.return_value.take();
    Ok(RunnerRunOutcome {
        result,
        stats,
        conversations,
        return_value,
        kernel_trace: runner.kernel_trace.take(),
    })
}

/// Why a build-time evaluation ([`evaluate_function`]) produced no value.
#[derive(Debug)]
pub enum EvaluationFailure {
    /// Runner errors: an unsupported construct, an internal failure, or the
    /// exhausted step budget.
    Errors(Vec<RunnerError>),
    /// The function ended on its `⇥` channel with this error value.
    Raised(Value),
    /// The function ended without a return value.
    NoValue,
    /// The step budget ran out (an endless or runaway computation).
    StepLimit,
}

/// Run a specific MIR function under a step budget and return its value.
///
/// This is the build-time evaluator for `praefixum` statics (DECISIONS
/// D8.5): the caller supplies a fixed budget so an endless loop fails the
/// build. A `mori` reaches the host's `abort`, which the caller traps.
///
/// # Errors
///
/// Returns [`EvaluationFailure`] when execution fails, raises on its error
/// channel, or returns nothing.
pub fn evaluate_function<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    step_budget: u64,
) -> Result<Value, EvaluationFailure> {
    let context = validated.validation();
    let program = validated.program();
    let interner = context.interner.as_interner().ok_or_else(|| {
        EvaluationFailure::Errors(vec![RunnerError::internal(
            Span::default(),
            "runner requires interner context",
        )])
    })?;
    if validated.function_by_id(function_id).is_none() {
        return Err(EvaluationFailure::Errors(vec![RunnerError::internal(
            Span::default(),
            format!("no MIR function with id {}", function_id.0),
        )]));
    }
    let mut runner = Runner::new(program, context, context.types.as_table(), interner, host);
    runner.step_limit = Some(step_budget);
    runner.push_frame(function_id, MirBlockId(0));
    runner.run_until_depth(0);
    runner.close_all_frames();
    if runner
        .errors
        .iter()
        .any(|error| error.issue == RUNNER_ISSUE_STEP_BUDGET_EXHAUSTED)
    {
        // Completing on the last allowed block is valid; only an attempted
        // visit beyond the budget records this issue.
        return Err(EvaluationFailure::StepLimit);
    }
    if !runner.errors.is_empty() {
        return Err(EvaluationFailure::Errors(std::mem::take(
            &mut runner.errors,
        )));
    }
    if let Some(raised) = runner.return_error.take() {
        return Err(EvaluationFailure::Raised(raised));
    }
    match runner.return_value.take() {
        Some(Value::Unit) | None => Err(EvaluationFailure::NoValue),
        Some(value) => Ok(value),
    }
}

/// Gaussian Error Linear Unit using the tanh approximation:
/// 0.5 * x * (1 + tanh(√(2/π) * (x + 0.044715 * x³)))
fn gelu_f64(x: f64) -> f64 {
    let alpha = (2.0 / std::f64::consts::PI).sqrt();
    let beta = 0.044_715;
    let cube = x * x * x;
    0.5 * x * (1.0 + (alpha * (x + beta * cube)).tanh())
}

/// The half-precision round trip (the comparator's `kv_f16_round` register
/// rounding): round f32 to f16 (round-to-nearest-even) and widen back.
fn f16_round_f32(x: f32) -> f32 {
    half_to_f32(f32_to_f16_rn(x))
}

/// The half-precision round trip applied to the runner's f64 value carrier
/// (round through f32/f16 and widen back to f64 — the IEEE conversion is
/// inherently a narrowing cast).
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub(crate) fn f16_round_f64(v: f64) -> f64 {
    f64::from(f16_round_f32(v as f32))
}

/// Round the runner's f64 carrier to bfloat16 using round-to-nearest-even.
pub(crate) fn bf16_round_f64(v: f64) -> f64 {
    let bits = (v as f32).to_bits();
    let rounded = if v.is_nan() {
        (bits >> 16) | 0x0040
    } else {
        bits.wrapping_add(0x7fff + ((bits >> 16) & 1)) >> 16
    };
    f64::from(f32::from_bits(rounded << 16))
}

/// f32 -> f16 round-to-nearest-even (the public faber `f32_to_f16_rn`
/// algorithm; the IEEE bit-layout shifts/truncations are intentional).
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::unnecessary_cast
)]
fn f32_to_f16_rn(x: f32) -> u16 {
    if x.is_nan() {
        return 0x7e00;
    }
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x7f_ffff;
    if exponent == 0xff {
        // Infinity or NaN.
        return if mantissa == 0 { sign | 0x7c00 } else { 0x7e00 };
    }
    let biased = exponent - 127 + 15;
    if biased >= 0x1f {
        // Overflow to infinity (RN).
        return sign | 0x7c00;
    }
    if biased <= 0 {
        // Subnormal/zero: round the f32 mantissa into f16 subnormal bits.
        if biased < -10 {
            return sign;
        }
        let shift = 14 - biased;
        let half = mantissa | 0x80_0000;
        let rounded = half + (1 << (shift - 1)) - 1 + ((half >> shift) & 1);
        return sign | ((rounded >> shift) as u16);
    }
    let round_bits = mantissa & 0x1fff;
    let mut half_mantissa = (mantissa >> 13) as u16;
    if round_bits > 0x1000 || (round_bits == 0x1000 && (half_mantissa & 1) == 1) {
        half_mantissa += 1;
        if half_mantissa == 0x400 {
            return sign | ((biased as u16 + 1) << 10);
        }
    }
    sign | ((biased as u16) << 10) | half_mantissa
}

/// f16 bits -> f32 (the standard sign/exponent/mantissa decode; the IEEE
/// bit-layout casts are intentional).
#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::unnecessary_cast
)]
fn half_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) & 1) as u32;
    let exponent = ((bits >> 10) & 0x1f) as u32;
    let mantissa = (bits & 0x3ff) as u32;
    if exponent == 0 {
        if mantissa == 0 {
            return if sign == 1 { -0.0 } else { 0.0 };
        }
        // Subnormal.
        return (mantissa as f32) * 2f32.powi(-24) * if sign == 1 { -1.0 } else { 1.0 };
    }
    if exponent == 0x1f {
        // Infinity/NaN decode must honor the sign bit so Bits round trips
        // return the original ±Inf/NaN bits.
        let magnitude = if mantissa == 0 {
            f32::INFINITY
        } else {
            f32::NAN
        };
        return if sign == 1 { -magnitude } else { magnitude };
    }
    let value = (1.0f32 + mantissa as f32 / 1024.0f32) * 2f32.powi(exponent as i32 - 15);
    if sign == 1 { -value } else { value }
}

impl<H: Host + ?Sized> Runner<'_, H> {
    fn push_frame(&mut self, function_id: MirFunctionId, entry_block: MirBlockId) {
        let frame = self.new_frame(function_id, entry_block);
        self.resume_frame(frame);
    }

    /// Push a built (or parked generator) frame onto the runner stack.
    fn resume_frame(&mut self, frame: Frame) {
        if let Some(stats) = self.stats.as_mut() {
            stats.enter_function(frame.function, !frame.resumed);
        }
        self.frames.push(frame);
    }

    fn new_frame(&self, function_id: MirFunctionId, entry_block: MirBlockId) -> Frame {
        self.new_frame_from(function_id, entry_block, Vec::new(), Vec::new())
    }

    /// [`Self::new_frame`] over recycled (empty) slot buffers, so a call
    /// reuses the allocations of a frame that already returned.
    fn new_frame_from(
        &self,
        function_id: MirFunctionId,
        entry_block: MirBlockId,
        mut locals: Vec<Option<Value>>,
        mut temps: Vec<Option<Value>>,
    ) -> Frame {
        if let Some(slots) = self.prepared.slot_types.get(&function_id) {
            locals.resize(slots.locals.len(), None);
            for param in slots.param_locals.iter().copied() {
                if let Some(slot) = locals.get_mut(param.0 as usize) {
                    *slot = Some(Value::Unit);
                }
            }
            temps.resize(slots.temps.len(), None);
        }
        Frame {
            function: function_id,
            locals,
            aliases: FxHashMap::default(),
            mut_writebacks: Vec::new(),
            temps,
            values: FxHashMap::default(),
            records_values: self.prepared.value_reading_functions.contains(&function_id),
            block: entry_block,
            resume_at: 0,
            resumed: false,
            is_generator: false,
            blocks: Vec::new(),
        }
    }

    fn pop_frame(&mut self) -> Option<Frame> {
        let frame = self.frames.pop();
        if let Some(frame) = frame.as_ref()
            && let Some(stats) = self.stats.as_mut()
        {
            stats.leave_function(frame.function);
        }
        frame
    }

    /// Keep a returned frame's slot buffers for the next call.
    fn recycle_frame(&mut self, frame: Frame) {
        const SLOT_POOL_LIMIT: usize = 64;
        if self.slot_pool.len() + 2 > SLOT_POOL_LIMIT {
            return;
        }
        let Frame {
            mut locals,
            mut temps,
            ..
        } = frame;
        locals.clear();
        temps.clear();
        self.slot_pool.push(locals);
        self.slot_pool.push(temps);
    }

    fn close_all_frames(&mut self) {
        while self.pop_frame().is_some() {}
    }

    fn take_stats(&mut self) -> Option<RunnerStats> {
        self.stats.take().map(stats::StatsCollector::finish)
    }

    fn function_by_id(&self, id: MirFunctionId, span: Span) -> Result<&MirFunction, RunnerError> {
        // Use the immutable function-id index captured at runner construction.
        self.program
            .indexed_function(&self.prepared.function_index, id)
            .ok_or_else(|| RunnerError::internal(span, format!("missing function f{}", id.0)))
    }

    /// Refuse execution of a function carrying the device `@ kernel`
    /// annotation unless the run's [`RunnerKernelPolicy`] is `Execute`. The
    /// annotation survives HIR→MIR as the compute shader stage; no
    /// source-name or prose matching belongs in this policy gate.
    fn refuse_kernel_execution(
        &self,
        function_id: MirFunctionId,
        span: Span,
    ) -> Result<(), RunnerError> {
        if self.kernel_policy != RunnerKernelPolicy::Refuse {
            return Ok(());
        }
        let function = self.function_by_id(function_id, span)?;
        if function.shader_stage == Some(MirKernelShaderStage::Compute) {
            return Err(kernel_refused(span));
        }
        Ok(())
    }

    /// Whether a call to `function_id` goes to the kernel dispatcher: the run
    /// is under [`RunnerKernelPolicy::Dispatch`] and either the callee carries
    /// the compute shader stage or the run's dispatcher claims it.
    ///
    /// A dispatcher may carry a device kernel for a plain function (no shader
    /// stage): the device image built it, so the dispatcher owns the fact and
    /// the stage alone never decides.
    fn dispatches_kernel(
        &self,
        function_id: MirFunctionId,
        span: Span,
    ) -> Result<bool, RunnerError> {
        if self.kernel_policy != RunnerKernelPolicy::Dispatch {
            return Ok(false);
        }
        let function = self.function_by_id(function_id, span)?;
        if function.shader_stage == Some(MirKernelShaderStage::Compute) {
            return Ok(true);
        }
        let Some(dispatcher) = self.kernel_dispatcher.as_ref() else {
            return Ok(false);
        };
        let hook = dispatcher.clone();
        let mut dispatcher = hook.try_borrow_mut().map_err(|_| {
            RunnerError::internal(
                span,
                "kernel dispatcher is busy during the dispatch claim probe",
            )
        })?;
        Ok(dispatcher.claims_dispatch(function_id))
    }

    /// Delivery point (the `ad` boundary): complete device work and surface
    /// the first deferred fault. No-op without a dispatcher.
    pub(crate) fn flush_dispatcher(&mut self) -> Result<(), RunnerError> {
        let Some(dispatcher) = self.kernel_dispatcher.as_ref() else {
            return Ok(());
        };
        dispatcher
            .try_borrow_mut()
            .map_err(|_| {
                RunnerError::internal(Span::default(), "kernel dispatcher is busy during a flush")
            })?
            .flush()
    }

    /// Delivery point (program end): flush, then release the device.
    fn finish_dispatcher(&mut self) -> Result<(), RunnerError> {
        let Some(dispatcher) = self.kernel_dispatcher.as_ref() else {
            return Ok(());
        };
        let mut dispatcher = dispatcher.try_borrow_mut().map_err(|_| {
            RunnerError::internal(Span::default(), "kernel dispatcher is busy during finish")
        })?;
        let finished = dispatcher.finish();
        // A recoverable error still queued at program end has no handler.
        let leftover = dispatcher.deliver_faults();
        finished?;
        if let Some(fault) = self.carried_fault.take().or(leftover?) {
            return Err(device_block::unhandled_fault_error(&fault, Span::default()));
        }
        Ok(())
    }

    /// Hand a kernel call to the dispatcher; the returned value is the call's
    /// value (`Unit` when the dispatcher returns none). `args` is mutable so
    /// the caller can write `mut` arguments back.
    fn dispatch_kernel_call(
        &mut self,
        function_id: MirFunctionId,
        args: &mut [Value],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let owner = self.launch_owner();
        let Some(dispatcher) = self.kernel_dispatcher.as_ref() else {
            return Err(RunnerError::internal(
                span,
                "kernel dispatch policy has no dispatcher",
            ));
        };
        // A re-entrant host access during dispatch is an error, not a panic.
        let mut dispatcher = dispatcher.try_borrow_mut().map_err(|_| {
            RunnerError::internal(span, "kernel dispatcher is busy during a dispatch")
        })?;
        dispatcher.set_launch_owner(owner);
        Ok(dispatcher
            .dispatch(function_id, args, span)?
            .unwrap_or(Value::Unit))
    }

    fn set_active_block(&mut self, block: MirBlockId, span: Span) {
        match self.frames.last_mut() {
            Some(frame) => frame.block = block,
            None => self
                .errors
                .push(RunnerError::internal(span, "no active frame")),
        }
    }

    /// Interval (in steps) between cooperative wall-clock deadline checks in
    /// [`Runner::consume_step`] — amortizes the clock read over many steps.
    const DEADLINE_CHECK_INTERVAL: u64 = 1024;

    /// Count one interpreter step (one MIR basic-block visit). Returns `false`
    /// when the step budget is exhausted or the wall-clock deadline passes; an
    /// error is recorded. The deadline is checked every
    /// [`DEADLINE_CHECK_INTERVAL`] steps so the common step pays no clock cost.
    fn consume_step(&mut self, span: Span) -> bool {
        if self.deadline.is_some()
            && self
                .steps_taken
                .is_multiple_of(Self::DEADLINE_CHECK_INTERVAL)
            && let Some(deadline) = self.deadline
            && Instant::now() >= deadline
        {
            self.errors.push(RunnerError::internal_with_issue(
                span,
                "wall-clock timeout exceeded",
                RUNNER_ISSUE_WALL_CLOCK_TIMEOUT,
            ));
            return false;
        }
        if let Some(limit) = self.step_limit
            && self.steps_taken >= limit
        {
            self.errors.push(RunnerError::internal_with_issue(
                span,
                format!("interpreter step budget exhausted after {limit} steps"),
                RUNNER_ISSUE_STEP_BUDGET_EXHAUSTED,
            ));
            return false;
        }
        self.steps_taken += 1;
        true
    }

    /// Record an error and drop the active frame. `run_until_depth` observes
    /// the error list and stops; the frame drop keeps the depth guard exact.
    fn fail(&mut self, error: RunnerError) {
        self.errors.push(error);
        self.pop_frame();
    }

    fn run_until_depth(&mut self, min_depth: usize) {
        while self.frames.len() > min_depth {
            if !self.consume_step(Span::default()) {
                break;
            }
            let Some(frame) = self.frames.last_mut() else {
                break;
            };
            let (function_id, block_id) = (frame.function, frame.block);
            let resume_at = std::mem::take(&mut frame.resume_at);
            let block_data = match self.block_snapshot(function_id, block_id) {
                Ok(data) => data,
                Err(error) => {
                    self.fail(error);
                    continue;
                }
            };

            let mut frame_returned = false;
            let mut suspended = false;
            let mut try_call_shape_failure = None;
            for (index, statement) in block_data.statements.iter().enumerate().skip(resume_at) {
                let shape_guard_for_try_call = index + 1 == block_data.statements.len()
                    && shape_guard_precedes_try_call(statement, &block_data.terminator.kind);
                let fill_result_dropped = fill_result_is_dropped(statement, &block_data, index);
                let result = self.execute_statement_inner(
                    statement,
                    shape_guard_for_try_call,
                    fill_result_dropped,
                );
                match result {
                    Ok(StatementOutcome::Continue) => {
                        if self.yielded.is_some() {
                            // D6.6: park the generator after its `cede`.
                            if let Some(frame) = self.frames.last_mut() {
                                frame.resume_at = index + 1;
                                frame.resumed = true;
                            }
                            suspended = true;
                            break;
                        }
                    }
                    Ok(StatementOutcome::FrameReturned) => {
                        frame_returned = true;
                        break;
                    }
                    Err(error)
                        if shape_guard_for_try_call && is_recoverable_shape_failure(&error) =>
                    {
                        try_call_shape_failure = Some(error);
                    }
                    Err(error) => {
                        self.fail(error);
                        break;
                    }
                }
            }
            if !self.errors.is_empty() || suspended {
                break;
            }
            if frame_returned {
                continue;
            }

            let terminator = &block_data.terminator;
            match &terminator.kind {
                MirTerminatorKind::Return(operand) => {
                    let value = match operand {
                        Some(operand) => match self.eval_operand(operand, terminator.span) {
                            Ok(value) => value,
                            Err(error) => {
                                self.fail(error);
                                continue;
                            }
                        },
                        None => Value::Unit,
                    };
                    self.return_error = None;
                    self.return_value = Some(value);
                    let returned = self.pop_frame();
                    if let Some(mut frame) = returned {
                        for (caller, place, local) in std::mem::take(&mut frame.mut_writebacks) {
                            let Some(value) = frame.locals.get(local.0 as usize).cloned().flatten()
                            else {
                                self.fail(RunnerError::internal(
                                    terminator.span,
                                    "mut write-back parameter cell is uninitialized",
                                ));
                                break;
                            };
                            if let Err(error) =
                                self.write_place_at(caller, &place, value, terminator.span)
                            {
                                self.fail(error);
                                break;
                            }
                        }
                        self.recycle_frame(frame);
                    }
                }
                MirTerminatorKind::ReturnThread => {
                    self.fail(RunnerError::unsupported(
                        terminator.span,
                        "return_thread terminator is not executable before lane lowering",
                    ));
                }
                MirTerminatorKind::ReturnError(operand) => {
                    let value = match self.eval_operand(operand, terminator.span) {
                        Ok(value) => value,
                        Err(error) => {
                            self.fail(error);
                            continue;
                        }
                    };
                    self.return_value = None;
                    self.return_error = Some(value);
                    self.pop_frame();
                }
                MirTerminatorKind::TryCall {
                    destination,
                    callee,
                    args,
                    ok_block,
                    error_place,
                    error_block,
                } => {
                    let call_result = if let Some(error) = try_call_shape_failure.take() {
                        match self.shape_failure_try_call_route_is_textus_compatible(
                            callee,
                            error_place,
                            terminator.span,
                        ) {
                            Ok(true) => Ok(CallOutcome::Err(Value::Str(error.message))),
                            Ok(false) => {
                                self.fail(RunnerError::internal(
                                    terminator.span,
                                    "recoverable runtime shape failure requires a textus-compatible TryCall error place",
                                ));
                                continue;
                            }
                            Err(route_error) => {
                                self.fail(route_error);
                                continue;
                            }
                        }
                    } else {
                        self.invoke_call(callee, args, terminator.span)
                    };
                    match call_result {
                        Ok(CallOutcome::Ok(value)) => {
                            if let Some(destination) = destination
                                && let Err(error) =
                                    self.write_place(destination, value, terminator.span)
                            {
                                self.fail(error);
                                continue;
                            }
                            self.set_active_block(*ok_block, terminator.span);
                        }
                        Ok(CallOutcome::Err(error_value)) => {
                            if let Err(error) =
                                self.write_place(error_place, error_value, terminator.span)
                            {
                                self.fail(error);
                                continue;
                            }
                            self.set_active_block(*error_block, terminator.span);
                        }
                        Err(error) => {
                            self.fail(error);
                        }
                    }
                }
                MirTerminatorKind::Goto(target) => {
                    self.set_active_block(*target, terminator.span);
                }
                MirTerminatorKind::Branch {
                    condition,
                    then_block,
                    else_block,
                } => match self.eval_operand(condition, terminator.span) {
                    Ok(cond) => match cond.as_bool(terminator.span) {
                        Ok(taken) => {
                            self.set_active_block(
                                if taken { *then_block } else { *else_block },
                                terminator.span,
                            );
                        }
                        Err(error) => {
                            self.fail(error);
                        }
                    },
                    Err(error) => {
                        self.fail(error);
                    }
                },
                MirTerminatorKind::Switch {
                    value,
                    cases,
                    default,
                } => match self.eval_operand(value, terminator.span) {
                    Ok(switch_value) => {
                        let target = cases
                            .iter()
                            .find(|case| {
                                constant_equals_value(&case.value, &switch_value, self.interner)
                            })
                            .map_or(*default, |case| case.target);
                        self.set_active_block(target, terminator.span);
                    }
                    Err(error) => {
                        self.fail(error);
                    }
                },
                MirTerminatorKind::Unreachable => {
                    self.fail(RunnerError::unsupported(
                        terminator.span,
                        &format!("terminator {:?}", terminator.kind),
                    ));
                }
            }
        }
    }

    fn block_snapshot(
        &self,
        function_id: MirFunctionId,
        block_id: MirBlockId,
    ) -> Result<Rc<MirBlock>, RunnerError> {
        // BO-M-1: share a pre-cloned block via Rc instead of deep-cloning per step.
        self.prepared
            .block_cache
            .get(&(function_id, block_id))
            .cloned()
            .ok_or_else(|| {
                RunnerError::internal(Span::default(), format!("missing block b{}", block_id.0))
            })
    }

    #[cfg(test)]
    fn execute_statement(
        &mut self,
        statement: &radix_mir::MirStatement,
    ) -> Result<StatementOutcome, RunnerError> {
        // Standalone statement execution has no surrounding block: the
        // posture scan runs against an empty block, so a temp destination
        // counts as dropped (nothing can read it later).
        let block = radix_mir::MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: radix_mir::MirTerminator {
                kind: MirTerminatorKind::Unreachable,
                span: Span::default(),
            },
            span: Span::default(),
        };
        self.execute_statement_inner(
            statement,
            false,
            fill_result_is_dropped(statement, &block, 0),
        )
    }

    fn execute_statement_inner(
        &mut self,
        statement: &radix_mir::MirStatement,
        shape_failure_belongs_to_try_call: bool,
        fill_result_dropped: bool,
    ) -> Result<StatementOutcome, RunnerError> {
        match &statement.kind {
            MirStatementKind::Assign { place, value } => {
                let evaluated = self.eval_value(value, statement.span)?;
                if matches!(
                    self.deliver_at_host_read(statement.span)?,
                    StatementOutcome::FrameReturned
                ) {
                    return Ok(StatementOutcome::FrameReturned);
                }
                self.write_place(place, evaluated, statement.span)?;
                Ok(StatementOutcome::Continue)
            }
            MirStatementKind::RuntimeCall { destination, call } => {
                if let radix_mir::MirIntrinsic::DeviceBlock(op) = &call.intrinsic {
                    return self.execute_device_block(*op, call, statement.span);
                }
                // Keep this boundary around both production dispatch paths:
                // `Convert` is handled directly here, while every other
                // intrinsic goes through `eval_runtime_call`.  In particular,
                // do not instrument only that method's unreachable Convert
                // arm; conversion recovery must still be counted.
                let intrinsic_name = self
                    .stats
                    .as_ref()
                    .map(|_| stats::intrinsic_name(&call.intrinsic));
                let intrinsic_started = self
                    .stats
                    .as_ref()
                    .map(|_| stats::StatsCollector::begin_intrinsic());
                let result = match &call.intrinsic {
                    radix_mir::MirIntrinsic::Convert(conversion) => {
                        match self.eval_convert(conversion, &call.args, statement.span) {
                            Ok(value) => Ok(Some(value)),
                            Err(error) if error.issue == RUNNER_RECOVERY_FRAME_RETURNED => Ok(None),
                            Err(error) if error.is_conversion_failure() => {
                                self.propagate_runtime_failure(error).map(|_| None)
                            }
                            Err(error) => Err(error),
                        }
                    }
                    // WHY: the fill statement (dropped result) is the
                    // in-place mutation posture — the bound form builds a
                    // fresh tensor instead of touching the source. Mirrors
                    // the HIR→Rust emit lane's split at the same posture
                    // boundary.
                    radix_mir::MirIntrinsic::Collection(MirCollectionOp::TensorFill)
                        if fill_result_dropped =>
                    {
                        self.tensor_fill_in_place(&call.args, statement.span)
                            .map(|_| Some(Value::Unit))
                    }
                    _ => match self.eval_runtime_call(call, statement.span) {
                        Ok(value) => Ok(Some(value)),
                        Err(error)
                            if shape_failure_belongs_to_try_call
                                && is_recoverable_shape_failure(&error) =>
                        {
                            Err(error)
                        }
                        Err(error)
                            if matches!(
                                error.issue.as_str(),
                                "runner_collection_push_capacity"
                                    | RUNNER_JSON_PARSE_FAILED
                                    | RUNNER_CONVERSION_LENGTH_MISMATCH
                                    | "runner_tensor_shape_mismatch"
                                    | "runner_tensor_shape_invalid_extent"
                            ) =>
                        {
                            self.propagate_runtime_failure(error).map(|_| None)
                        }
                        Err(error) => Err(error),
                    },
                };
                if let (Some(name), Some(started)) = (intrinsic_name, intrinsic_started)
                    && let Some(stats) = self.stats.as_mut()
                {
                    stats.record_intrinsic(name, started);
                }
                let Some(result) = result? else {
                    return Ok(StatementOutcome::FrameReturned);
                };
                if matches!(
                    self.deliver_at_host_read(statement.span)?,
                    StatementOutcome::FrameReturned
                ) {
                    return Ok(StatementOutcome::FrameReturned);
                }
                if let Some(destination) = destination {
                    self.write_place(destination, result, statement.span)?;
                }
                Ok(StatementOutcome::Continue)
            }
            MirStatementKind::Call {
                destination,
                callee,
                args,
            } => match self.invoke_call(callee, args, statement.span)? {
                CallOutcome::Ok(result) => {
                    if let Some(destination) = destination {
                        self.write_place(destination, result, statement.span)?;
                    }
                    Ok(StatementOutcome::Continue)
                }
                CallOutcome::Err(_) => Err(RunnerError::internal(
                    statement.span,
                    "uncaught alternate return from call",
                )),
            },
            MirStatementKind::Construct {
                destination,
                aggregate,
            } => {
                let value = self.eval_construct(aggregate, statement.span)?;
                self.write_place(destination, value, statement.span)?;
                Ok(StatementOutcome::Continue)
            }
        }
    }

    fn propagate_runtime_failure(
        &mut self,
        error: RunnerError,
    ) -> Result<StatementOutcome, RunnerError> {
        let Some(frame) = self.frames.last() else {
            return Err(error);
        };
        let function = self.function_by_id(frame.function, error.span)?;
        let Some(error_ty) = function.error_ty else {
            return Err(error);
        };
        // A leaf failure carries `fault_kind`: its payload is the two-entry
        // fault record, which also crosses into the dynamic `valor` carrier.
        // Every other failure keeps the text payload and its textus/ignotum
        // alternate exit.
        let fault = fault_record_args(&error);
        let accepts_payload = if fault.is_some() {
            matches!(
                self.types.get(error_ty.semantic_id()),
                Type::Primitive(Primitive::Textus | Primitive::Ignotum | Primitive::Valor)
            )
        } else {
            matches!(
                self.types.get(error_ty.semantic_id()),
                Type::Primitive(Primitive::Textus | Primitive::Ignotum)
            )
        };
        if !accepts_payload {
            return Err(RunnerError::internal(
                error.span,
                "recoverable runtime failure requires textus-compatible alternate-exit type",
            ));
        }

        // The channel decides the payload shape: only an ignotum/valor
        // alternate exit carries the fault record map; a textus channel
        // carries the failure text, like every other failure.
        let primitive = match self.types.get(error_ty.semantic_id()) {
            Type::Primitive(primitive) => *primitive,
            _ => Primitive::Textus,
        };

        self.return_value = None;
        self.return_error = Some(failure_channel_value(&primitive, fault, error.message));
        self.pop_frame();
        Ok(StatementOutcome::FrameReturned)
    }

    fn shape_failure_try_call_route_is_textus_compatible(
        &mut self,
        callee: &MirCallee,
        error_place: &MirPlace,
        span: Span,
    ) -> Result<bool, RunnerError> {
        let callee_id = match callee {
            MirCallee::Value(operand) => match self.eval_operand(operand, span)? {
                Value::Function(id) | Value::Closure(id, _, _) => id,
                _ => {
                    return Err(RunnerError::internal(
                        span,
                        "call callee is not a function value",
                    ));
                }
            },
            _ => self.resolve_callee(callee, span)?,
        };
        let Some(callee_error_ty) = self.function_by_id(callee_id, span)?.error_ty else {
            return Ok(false);
        };
        if !matches!(
            self.types.get(callee_error_ty.semantic_id()),
            Type::Primitive(Primitive::Textus | Primitive::Ignotum)
        ) || !error_place.projections.is_empty()
        {
            return Ok(false);
        }

        let Some(frame) = self.frames.last() else {
            return Err(RunnerError::internal(span, "no active frame"));
        };
        let error_place_ty = match &error_place.base {
            MirPlaceBase::Local(id) => self
                .prepared
                .slot_types
                .get(&frame.function)
                .and_then(|slots| slots.local_ty(*id)),
            MirPlaceBase::Temp(id) => self
                .prepared
                .slot_types
                .get(&frame.function)
                .and_then(|slots| slots.temp_ty(*id)),
        };
        Ok(error_place_ty.is_some_and(|ty| {
            matches!(
                self.types.get(ty),
                Type::Primitive(Primitive::Textus | Primitive::Ignotum)
            )
        }))
    }

    fn invoke_call(
        &mut self,
        callee: &MirCallee,
        args: &[MirOperand],
        span: Span,
    ) -> Result<CallOutcome, RunnerError> {
        // d32b3558-3.4: a stepped generator walk settles through a TryCall
        // whose callee operand still holds the cursor value (the temp is typed
        // as the materialized lista but carries the lazy carrier). The settle
        // is not a function call: it hands back the error the cursor raised
        // (if any), or succeeds with unit so the walk ends normally.
        if let MirCallee::Value(operand) = callee
            && let Value::Cursor(cursor) = self.eval_operand(operand, span)?
        {
            return Ok(match cursor.take_raised() {
                Some(raised) => CallOutcome::Err(raised),
                None => CallOutcome::Ok(Value::Unit),
            });
        }
        let (func_id, closure_environment) = match callee {
            MirCallee::Closure(closure) => {
                let Some(environment) = self
                    .context
                    .closure_environments
                    .get(&closure.environment_id)
                else {
                    return Err(RunnerError::internal(
                        span,
                        "closure callee metadata is missing",
                    ));
                };
                if environment.function != closure.function {
                    return Err(RunnerError::internal(
                        span,
                        "closure callee function mismatch",
                    ));
                }
                let value = self.eval_operand(&closure.environment, span)?;
                (
                    closure.function,
                    Some((environment.environment_local, value)),
                )
            }
            MirCallee::Value(operand) => match self.eval_operand(operand, span)? {
                Value::Function(id) => (id, None),
                Value::Closure(function, environment_id, value) => {
                    let Some(environment) = self.context.closure_environments.get(&environment_id)
                    else {
                        return Err(RunnerError::internal(
                            span,
                            "closure value metadata is missing",
                        ));
                    };
                    if environment.function != function {
                        return Err(RunnerError::internal(
                            span,
                            "closure value function mismatch",
                        ));
                    }
                    (function, Some((environment.environment_local, *value)))
                }
                _ => {
                    return Err(RunnerError::internal(
                        span,
                        "call callee is not a function value",
                    ));
                }
            },
            _ => (self.resolve_callee(callee, span)?, None),
        };
        self.refuse_kernel_execution(func_id, span)?;
        let dispatched = self.dispatches_kernel(func_id, span)?;
        let kernel_scope = self.kernel_trace_scope(func_id, span)?;
        let traced = kernel_scope && self.kernel_trace_depth == 0;
        // BO-M-3/052: avoid cloning the entire MirFunction; collect param locals only.
        let param_modes: Rc<[(MirLocalId, MirParamMode)]> = match self
            .prepared
            .slot_types
            .get(&func_id)
            .filter(|_| !dispatched)
        {
            Some(slots) => Rc::clone(&slots.params),
            None => self
                .function_by_id(func_id, span)?
                .params
                .iter()
                .map(|param| (param.local, param.mode))
                // The lane id is a builtin of the device launch, never a call
                // argument: a dispatched lane-form kernel is called without it.
                .filter(|(_, mode)| !(dispatched && *mode == MirParamMode::Lane))
                .collect(),
        };
        let arg_values = args
            .iter()
            .map(|arg| self.eval_operand(arg, span))
            .collect::<Result<Vec<_>, _>>()?;
        if param_modes.len() != arg_values.len() {
            return Err(RunnerError::internal(span, "call argument count mismatch"));
        }
        for ((_, mode), operand) in param_modes.iter().zip(args) {
            if *mode == MirParamMode::MutRef && !matches!(operand, MirOperand::Place(_)) {
                return Err(RunnerError::internal(
                    span,
                    "mutable parameter requires a place argument",
                ));
            }
        }
        let trace_roles = traced.then(|| {
            param_modes
                .iter()
                .map(|(_, mode)| {
                    if *mode == MirParamMode::MutRef {
                        kernel_trace::TraceRole::Output
                    } else {
                        kernel_trace::TraceRole::Input
                    }
                })
                .collect::<Vec<_>>()
        });
        if dispatched {
            let mut arg_values = arg_values;
            let owner = self.launch_owner();
            let value = self.dispatch_kernel_call(func_id, &mut arg_values, span)?;
            if let Some(roles) = &trace_roles {
                let stages = self
                    .kernel_dispatcher
                    .as_ref()
                    .and_then(kernel_trace::last_call_stages);
                self.record_kernel_call(func_id, roles, &arg_values, &value, stages, span)?;
            }
            // `mut` write-back through the existing param-mode path: the
            // dispatcher's final argument value lands in the caller's place.
            for (((_, mode), operand), arg) in param_modes.iter().zip(args).zip(arg_values) {
                if *mode == MirParamMode::MutRef
                    && let MirOperand::Place(place) = operand
                {
                    self.write_place(place, arg, span)?;
                }
            }
            // U9-5: a `⇥` kernel under an omnia block is queued (its error
            // arrives at a delivery point); anywhere else the call waits.
            if self.function_by_id(func_id, span)?.error_ty.is_some() && !self.top_block_is_omnia()
            {
                self.flush_dispatcher()?;
                if let Some(fault) = self.take_dispatcher_fault(span)? {
                    if fault.owner == owner {
                        return Ok(CallOutcome::Err(Value::Str(fault.text)));
                    }
                    // An earlier launch of an outer block failed first: it
                    // unwinds to that block at the next delivery point.
                    self.carried_fault = Some(fault);
                }
            }
            return Ok(CallOutcome::Ok(value));
        }

        let depth = self.frames.len();
        if depth >= MAX_CALL_DEPTH {
            return Err(RunnerError::internal_with_issue(
                span,
                format!("call depth limit of {MAX_CALL_DEPTH} exceeded"),
                RUNNER_ISSUE_CALL_DEPTH_EXCEEDED,
            ));
        }
        self.return_value = None;
        self.return_error = None;
        let locals_buf = self.slot_pool.pop().unwrap_or_default();
        let temps_buf = self.slot_pool.pop().unwrap_or_default();
        let frame = self.new_frame_from(func_id, MirBlockId(0), locals_buf, temps_buf);
        self.resume_frame(frame);
        // The executed kernel's tensors are shared (`Rc`), so the arguments
        // evaluated here still see the body's writes after it returns.
        let trace_args = trace_roles.is_some().then(|| arg_values.clone());
        let callee_slots = self.prepared.slot_types.get(&func_id);
        let mut mut_writebacks: Vec<(usize, MirPlace, MirLocalId)> = Vec::new();
        if let Some(frame) = self.frames.last_mut() {
            if let Some((local, value)) = closure_environment {
                write_slot(&mut frame.locals, local.0 as usize, value, span, "local")?;
            }
            for ((local, mode), (arg, operand)) in param_modes
                .iter()
                .copied()
                .zip(arg_values.into_iter().zip(args))
            {
                // `in` is a borrow contract, not a scalar copy. Keep a
                // direct place alias while the caller frame is live; all
                // other modes retain the existing value transfer behavior.
                if mode == MirParamMode::MutRef
                    && depth > 0
                    && let MirOperand::Place(place) = operand
                    && place.projections.is_empty()
                {
                    frame.aliases.insert(
                        local,
                        PlaceAlias {
                            frame: depth - 1,
                            base: place.base.clone(),
                        },
                    );
                } else {
                    // Binding is a store into the parameter cell: a float
                    // bound to a bare `f32` parameter rounds to binary32.
                    let arg = match arg {
                        Value::Float(v)
                            if callee_slots.is_some_and(|slots| slots.local_is_f32(local)) =>
                        {
                            Value::Float(f64::from(v as f32))
                        }
                        other => other,
                    };
                    write_slot(&mut frame.locals, local.0 as usize, arg, span, "local")?;
                    // A projected `mut` place cannot take the alias branch
                    // (the alias carries the base only), so the value was
                    // copied in; schedule the write-back at return instead
                    // of silently dropping the callee's rebind.
                    if mode == MirParamMode::MutRef
                        && depth > 0
                        && let MirOperand::Place(place) = operand
                    {
                        mut_writebacks.push((depth - 1, place.clone(), local));
                    }
                }
            }
            frame.mut_writebacks = mut_writebacks;
        }
        if kernel_scope {
            self.kernel_trace_depth += 1;
        }
        stacker::maybe_grow(NATIVE_STACK_RED_ZONE, NATIVE_STACK_SEGMENT, || {
            self.run_until_depth(depth);
        });
        if kernel_scope {
            self.kernel_trace_depth -= 1;
        }
        if let Some(error) = self.errors.pop() {
            while self.frames.len() > depth {
                self.pop_frame();
            }
            return Err(error);
        }
        while self.frames.len() > depth {
            self.pop_frame();
        }
        if let Some(error_value) = self.return_error.take() {
            return Ok(CallOutcome::Err(error_value));
        }
        let returned = self.return_value.take().unwrap_or(Value::Unit);
        if let (Some(roles), Some(args)) = (&trace_roles, &trace_args) {
            self.record_kernel_call(func_id, roles, args, &returned, None, span)?;
        }
        Ok(CallOutcome::Ok(returned))
    }

    /// Whether a call to `function_id` is a compute-stage kernel call of a
    /// traced run. Both routes key on the shader stage alone, so the reference
    /// and the device capture the same calls.
    fn kernel_trace_scope(
        &self,
        function_id: MirFunctionId,
        span: Span,
    ) -> Result<bool, RunnerError> {
        if self.kernel_trace.is_none() {
            return Ok(false);
        }
        Ok(self.function_by_id(function_id, span)?.shader_stage
            == Some(MirKernelShaderStage::Compute))
    }

    /// Append the record of one completed kernel call: every tensor argument
    /// (`arg<N>`, as written at the call site), a returned tensor (`return`),
    /// and a returned scalar.
    fn record_kernel_call(
        &mut self,
        function_id: MirFunctionId,
        roles: &[kernel_trace::TraceRole],
        args: &[Value],
        returned: &Value,
        physical_stages: Option<u32>,
        span: Span,
    ) -> Result<(), RunnerError> {
        let mut tensors = Vec::new();
        for (position, (arg, role)) in args.iter().zip(roles).enumerate() {
            if let Value::Tensor(tensor) = arg {
                tensors.push(kernel_trace::traced_tensor(
                    &format!("arg{position}"),
                    *role,
                    &tensor.borrow(),
                )?);
            }
        }
        if let Value::Tensor(tensor) = returned {
            tensors.push(kernel_trace::traced_tensor(
                "return",
                kernel_trace::TraceRole::Return,
                &tensor.borrow(),
            )?);
        }
        let entry = radix_mir::host_partition::kernel_entry_name(
            self.function_by_id(function_id, span)?,
            self.interner,
        );
        if let Some(trace) = self.kernel_trace.as_mut() {
            trace.push(kernel_trace::KernelTraceRecord {
                index: trace.len() + 1,
                function: function_id.0,
                entry,
                physical_stages,
                tensors,
                result: kernel_trace::traced_result(returned),
            });
        }
        Ok(())
    }

    fn resolve_callee(
        &mut self,
        callee: &MirCallee,
        span: Span,
    ) -> Result<MirFunctionId, RunnerError> {
        match callee {
            MirCallee::Function(id) => Ok(*id),
            MirCallee::Definition { source: def_id, .. } => self.resolve_definition(*def_id, span),
            MirCallee::Closure(closure) => Ok(closure.function),
            MirCallee::Value(operand) => match self.eval_operand(operand, span)? {
                Value::Function(id) => Ok(id),
                _ => Err(RunnerError::internal(
                    span,
                    "call callee is not a function value",
                )),
            },
        }
    }

    fn resolve_definition(&self, def_id: DefId, span: Span) -> Result<MirFunctionId, RunnerError> {
        // BO-M-6: O(1) DefId → MirFunctionId via construction-time index.
        self.prepared
            .def_to_function
            .get(&def_id)
            .copied()
            .ok_or_else(|| {
                RunnerError::internal(span, format!("missing MIR function for def#{def_id:?}"))
            })
    }

    fn eval_value(&mut self, value: &MirValue, span: Span) -> Result<Value, RunnerError> {
        let computed = match &value.kind {
            MirValueKind::Closure(closure) => Value::Closure(
                closure.function,
                closure.environment_id,
                Box::new(self.eval_operand(&closure.environment, span)?),
            ),
            MirValueKind::Operand(operand) => self.eval_operand(operand, span)?,
            MirValueKind::Unary { op, operand } => self.eval_unop(*op, operand, value.ty, span)?,
            MirValueKind::Binary { op, lhs, rhs } => {
                self.eval_binop(*op, lhs, rhs, value.ty, span)?
            }
            MirValueKind::Option(op) => self.eval_option_value(op, span)?,
            MirValueKind::TensorExtent { tensor, axis } => {
                self.eval_tensor_extent(tensor, *axis, span)?
            }
        };
        let computed = self.normalize_decimal_value(computed, value.ty.semantic_id(), span)?;
        // WHY: Value::Float is an f64 carrier. Declared f32 (and other
        // non-f64 float) width has to land at this store, not at `≡`.
        let computed = self.apply_declared_float_width(computed, value.ty.semantic_id());
        if let Some(frame) = self.frames.last_mut()
            && frame.records_values
        {
            frame.values.insert(value.id, computed.clone());
        }
        Ok(computed)
    }

    /// `TensorExtent`: the carrier's extent on `axis`, read from its shape.
    fn eval_tensor_extent(
        &mut self,
        tensor: &MirOperand,
        axis: u32,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Value::Tensor(tensor) = self.eval_operand(tensor, span)? else {
            return Err(RunnerError::internal(
                span,
                "tensor extent operand is not a tensor",
            ));
        };
        let extent = tensor.borrow().shape.get(axis as usize).copied();
        extent.map(Value::Int).ok_or_else(|| {
            RunnerError::internal(span, "tensor extent axis is out of range for the carrier")
        })
    }

    fn eval_operand(&mut self, operand: &MirOperand, span: Span) -> Result<Value, RunnerError> {
        match operand {
            MirOperand::Constant(constant) => self.eval_constant(constant, span),
            MirOperand::Place(place) => self.read_place(place, span),
            MirOperand::Temp(temp) => self.read_temp(*temp, span),
            MirOperand::Value(value_id) => self.read_value(*value_id, span),
        }
    }

    /// Declared display width for a diagnostic operand (scalar or numeric
    /// collection element): the float width or `d64` decimal width its value
    /// renders at. Struct-field and element projections resolve through the
    /// field's declared type.
    fn operand_float_width(&self, operand: &MirOperand) -> Option<NumericWidth> {
        Self::element_display_width(self.operand_static_type(operand)?, self.types, 0)
    }

    /// Display shape of a diagnostic operand whose static type is (or holds) a
    /// tuple; `None` for an interval and every non-tuple.
    fn operand_tuple_parts(&self, operand: &MirOperand) -> Option<value::TupleParts> {
        tuple_parts(self.operand_static_type(operand)?, self.types, 0)
    }

    /// Whether a diagnostic operand's static type is (or holds) a dynamic
    /// `valor`/`json` carrier, which displays as a JSON-shaped object.
    fn operand_is_dynamic(&self, operand: &MirOperand) -> bool {
        self.operand_static_type(operand)
            .is_some_and(|ty| dynamic_display_type(ty, self.types, 0))
    }

    /// Raw-bits width of a diagnostic operand whose static type is (or holds)
    /// a `bN`, so it prints as hexadecimal at the full width.
    fn operand_bits_width(&self, operand: &MirOperand) -> Option<radix_types::BitsWidth> {
        holds_bits(self.operand_static_type(operand)?, self.types, 0)
    }

    /// Whether a diagnostic operand's static type is (or holds) a `littera`,
    /// so its ascii-carried elements print as chars rather than text.
    fn operand_holds_littera(&self, operand: &MirOperand) -> bool {
        self.operand_static_type(operand)
            .is_some_and(|ty| holds_littera(ty, self.types, 0))
    }

    /// Whether a diagnostic operand's static type is (or holds) an
    /// `intervallum`, so its tuple-shaped carrier renders as the interval
    /// construction literal rather than a debug tuple.
    fn operand_holds_interval(&self, operand: &MirOperand) -> bool {
        self.operand_static_type(operand)
            .is_some_and(|ty| holds_interval(ty, self.types, 0))
    }

    /// Static semantic type of an operand, resolving field and element
    /// projections through declared types.
    fn operand_static_type(&self, operand: &MirOperand) -> Option<TypeId> {
        let function = || {
            self.program
                .indexed_function(&self.prepared.function_index, self.frames.last()?.function)
        };
        // WHY: the prepared slot table answers temps and non-parameter locals
        // in O(1); a linear scan per operand dominated device-form lane runs.
        // A parameter local keeps the declared-locals-only lookup, which the
        // table (params fill unset local slots) does not reproduce.
        let slots = || self.prepared.slot_types.get(&self.frames.last()?.function);
        let ty = match operand {
            MirOperand::Temp(id) => slots()?.temp_ty(*id)?,
            MirOperand::Place(place) => {
                let base = match place.base {
                    MirPlaceBase::Temp(id) => slots()?.temp_ty(id),
                    MirPlaceBase::Local(id) if slots()?.param_locals.contains(&id) => function()?
                        .locals
                        .iter()
                        .find(|l| l.id == id)
                        .map(|l| l.ty.semantic_id()),
                    MirPlaceBase::Local(id) => slots()?.local_ty(id),
                }?;
                let mut ty = base;
                for projection in &place.projections {
                    ty = match projection {
                        MirProjection::Field(field) => {
                            let Type::Struct(def) = self.types.get(ty) else {
                                return None;
                            };
                            self.context
                                .struct_fields
                                .get(def)?
                                .get(field)?
                                .semantic_id()
                        }
                        // An element of a tuple is its declared part.
                        MirProjection::Index(MirOperand::Constant(MirConstant::Int(position)))
                        | MirProjection::ProvenIndex(MirOperand::Constant(MirConstant::Int(
                            position,
                        ))) if matches!(self.types.get(ty), Type::Tuple(_)) => {
                            let Type::Tuple(parts) = self.types.get(ty) else {
                                return None;
                            };
                            *parts.get(usize::try_from(*position).ok()?)?
                        }
                        // An element of a numeric collection renders at the
                        // collection's element width.
                        MirProjection::Index(_)
                        | MirProjection::ProvenIndex(_)
                        | MirProjection::VectorLane(_)
                        | MirProjection::MatrixCell { .. } => ty,
                        _ => return None,
                    };
                }
                return Some(ty);
            }
            MirOperand::Value(id) => self
                .prepared
                .value_types
                .get(&(self.frames.last()?.function, *id))
                .map(|ty| ty.semantic_id())?,
            MirOperand::Constant(_) => return None,
        };
        Some(ty)
    }

    fn element_display_width(ty: TypeId, types: &TypeTable, depth: u8) -> Option<NumericWidth> {
        element_display_width(ty, types, depth)
    }

    /// Apply declared float width to a newly stored scalar.
    ///
    /// Collection values stay untouched here so a list copy keeps its `Rc`.
    /// List/vector/matrix *elements* round in `eval_construct`.
    fn apply_declared_float_width(&self, value: Value, ty: TypeId) -> Value {
        let Value::Float(v) = value else {
            return value;
        };
        match effective_width(self.types, ty) {
            Some(width) if width.is_float() => Value::Float(round_float(v, width)),
            _ => Value::Float(v),
        }
    }

    fn apply_element_display_width(&self, items: Vec<Value>, ty: TypeId) -> Vec<Value> {
        match Self::element_display_width(ty, self.types, 0) {
            Some(width) if width.is_float() => items
                .into_iter()
                .map(|item| match item {
                    Value::Float(v) => Value::Float(round_float(v, width)),
                    other => other,
                })
                .collect(),
            _ => items,
        }
    }

    /// CTO-3 — the fallback runs ONLY on conversion failure. A side-effecting
    /// `⊥` default is lowered into the conversion's recovery region: real CFG
    /// blocks the success path skips, whose tail stores the fallback into the
    /// conversion's destination temp. At the failure point this executes the
    /// region through the normal statement executor — following its internal
    /// control flow, stopping at the main-reachable join — and reads the
    /// destination. Pure recovery operands evaluate normally.
    fn eval_recovery(&mut self, recovery: &MirOperand, span: Span) -> Result<Value, RunnerError> {
        let MirOperand::Temp(destination) = recovery else {
            return self.eval_operand(recovery, span);
        };
        let Some(frame) = self.frames.last() else {
            return Err(RunnerError::internal(
                span,
                "recovery evaluation outside a frame",
            ));
        };
        let function_id = frame.function;
        if frame
            .temps
            .get(destination.0 as usize)
            .is_some_and(Option::is_some)
        {
            // A temp the main flow already computed is a pure recovery operand.
            return self.eval_operand(recovery, span);
        }
        let function = self.function_by_id(function_id, span)?;
        let main_reachable = reachable_block_ids(function);
        let Some(entry) = recovery_region_entry(function, *destination, &main_reachable) else {
            return Err(RunnerError::internal(
                span,
                "conversion recovery region missing",
            ));
        };
        let mut current = entry;
        loop {
            let block = self.block_snapshot(function_id, current)?;
            for statement in &block.statements {
                // WHY: a region statement that fails through the alternate exit
                // has already popped this frame; the rest of the region must
                // not run against the caller's frame.
                if matches!(
                    self.execute_statement_inner(statement, false, false)?,
                    StatementOutcome::FrameReturned
                ) {
                    return Err(RunnerError::internal_with_issue(
                        span,
                        "conversion recovery region left the frame",
                        RUNNER_RECOVERY_FRAME_RETURNED,
                    ));
                }
            }
            let next = match &block.terminator.kind {
                MirTerminatorKind::Goto(target) => {
                    (!main_reachable.contains(target)).then_some(*target)
                }
                MirTerminatorKind::Branch {
                    condition,
                    then_block,
                    else_block,
                } => {
                    let condition = self.eval_operand(condition, span)?;
                    let taken = condition.as_bool(span)?;
                    Some(if taken { *then_block } else { *else_block })
                }
                _ => None,
            };
            let Some(next) = next else {
                break;
            };
            current = next;
        }
        self.eval_operand(recovery, span)
    }

    #[allow(clippy::unnecessary_wraps)]
    fn eval_constant(&self, constant: &MirConstant, span: Span) -> Result<Value, RunnerError> {
        Ok(match constant {
            MirConstant::Int(value) => Value::Int(*value),
            MirConstant::UInt(value) => Value::UInt(*value),
            MirConstant::Float(value) => Value::Float(*value),
            MirConstant::Bool(value) => Value::Bool(*value),
            MirConstant::String(symbol) => Value::Str(self.interned_textus(*symbol)),
            MirConstant::Ascii(symbol) => {
                let text = self.interned_ascii(*symbol);
                match text.as_bytes() {
                    [b] => Value::Ascii(*b),
                    _ => Value::Str(text),
                }
            }
            MirConstant::Nil => Value::Nil,
            MirConstant::Unit => Value::Unit,
            MirConstant::Octeti(bytes) => Value::Octeti(bytes.clone()),
            MirConstant::Function(id) => Value::Function(*id),
            // An exact `inf` literal (canonical decimal digits); read back
            // through the canonical carrier.
            MirConstant::Magnus(digits) => match handroll::Magnus::parse_decimal(digits) {
                Ok(value) => inf::from_magnus(value),
                Err(_) => {
                    return Err(RunnerError::internal(span, "malformed bignum constant"));
                }
            },
            MirConstant::ShapeSize { shape, axis } => {
                #[allow(clippy::cast_precision_loss)] // shape sizes are far below 2^53
                {
                    Value::Float(self.runtime_shape_size(*shape, *axis, span)? as f64)
                }
            }
            MirConstant::Regex { pattern, flags } => {
                // WHY: the pattern symbol is the raw regex body; slash bodies
                // receive no Faber backslash processing. A `textus ↦ regex`
                // conversion decodes the textus constant through
                // `MirConstant::String` on its own path.
                let key = (*pattern, *flags);
                let cached = self.regex_literals.borrow().get(&key).cloned();
                let program = if let Some(program) = cached {
                    program
                } else {
                    let pattern_text = regex_pattern(
                        self.interner.resolve(*pattern),
                        flags.map(|flags| self.interner.resolve(flags)),
                    );
                    let program = RegexProgram::compile(&pattern_text).map_err(|payload| {
                        RunnerError::internal(span, format!("constant regex is invalid: {payload}"))
                    })?;
                    let program = Rc::new(program);
                    self.regex_literals
                        .borrow_mut()
                        .insert(key, Rc::clone(&program));
                    program
                };
                Value::Regex(program)
            }
        })
    }

    fn runtime_shape_size(
        &self,
        shape: radix_types::IndexId,
        axis: Option<i64>,
        span: Span,
    ) -> Result<u64, RunnerError> {
        if let Some(axis) = axis {
            let IndexExpr::Tuple(dimensions) = self.types.get_index(shape) else {
                if axis == 0 {
                    return self.runtime_index_value(shape, span);
                }
                return Err(RunnerError::internal(
                    span,
                    "runtime shape-size axis is outside a scalar shape",
                ));
            };
            let axis = usize::try_from(axis)
                .map_err(|_| RunnerError::internal(span, "runtime shape-size axis is negative"))?;
            let dimension = dimensions.get(axis).copied().ok_or_else(|| {
                RunnerError::internal(span, "runtime shape-size axis is out of range")
            })?;
            return self.runtime_index_value(dimension, span);
        }

        match self.types.get_index(shape) {
            IndexExpr::Tuple(dimensions) => dimensions.iter().try_fold(1_u64, |product, dim| {
                let extent = self.runtime_index_value(*dim, span)?;
                product.checked_mul(extent).ok_or_else(|| {
                    RunnerError::internal(span, "runtime tensor element count overflow")
                })
            }),
            _ => self.runtime_index_value(shape, span),
        }
    }

    fn runtime_index_value(
        &self,
        index: radix_types::IndexId,
        span: Span,
    ) -> Result<u64, RunnerError> {
        match self.types.get_index(index) {
            IndexExpr::Literal(value) => Ok(*value),
            IndexExpr::Runtime(source) => self.runtime_extent_value(source, span),
            IndexExpr::Affine { base, offset } => {
                let base = i128::from(self.runtime_index_value(*base, span)?);
                let adjusted = base.checked_add(*offset).ok_or_else(|| {
                    RunnerError::internal(span, "runtime shape affine extent overflow")
                })?;
                u64::try_from(adjusted).map_err(|_| {
                    RunnerError::internal(
                        span,
                        "runtime shape affine extent is negative or too large",
                    )
                })
            }
            IndexExpr::Divide { numerator, divisor } => {
                if *divisor == 0 {
                    return Err(RunnerError::internal(
                        span,
                        "runtime shape extent divides by zero",
                    ));
                }
                let numerator = self.runtime_index_value(*numerator, span)?;
                if numerator % divisor != 0 {
                    return Err(RunnerError::internal(
                        span,
                        "runtime shape extent division is not exact",
                    ));
                }
                Ok(numerator / divisor)
            }
            IndexExpr::Param(_) | IndexExpr::Infer(_) | IndexExpr::Unspecified => Err(
                RunnerError::internal(span, "runtime shape-size index remains unresolved"),
            ),
            IndexExpr::Tuple(_) => Err(RunnerError::internal(
                span,
                "runtime shape-size expected one dimension, found a tuple",
            )),
        }
    }

    fn runtime_extent_value(
        &self,
        source: &RuntimeExtentId,
        span: Span,
    ) -> Result<u64, RunnerError> {
        match source {
            RuntimeExtentId::ScalarParameter { parameter } => {
                let value = self.function_parameter_value(*parameter, span)?;
                match value {
                    Value::UInt(value) => Ok(value),
                    Value::Int(value) => u64::try_from(value).map_err(|_| {
                        RunnerError::internal(span, "runtime scalar extent is negative")
                    }),
                    _ => Err(RunnerError::internal(
                        span,
                        "runtime scalar extent parameter is not an integer",
                    )),
                }
            }
            RuntimeExtentId::FunctionParameterAxis { parameter, axis } => {
                let value = self.function_parameter_value(*parameter, span)?;
                Self::tensor_axis_extent(value, *axis, span)
            }
            RuntimeExtentId::FunctionParameterFieldAxis {
                parameter,
                fields,
                axis,
            } => {
                let mut value = self.function_parameter_value(*parameter, span)?;
                for field in fields {
                    value = Self::runtime_field_value(value, *field, span)?;
                }
                Self::tensor_axis_extent(value, *axis, span)
            }
            RuntimeExtentId::ScalarBinding(_)
            | RuntimeExtentId::TensorBindingAxis { .. }
            | RuntimeExtentId::TensorFieldAxis { .. }
            | RuntimeExtentId::ScalarExpression { .. }
            | RuntimeExtentId::ExpressionAxis { .. }
            | RuntimeExtentId::ExpressionFieldAxis { .. }
            | RuntimeExtentId::CallArgumentAxis { .. }
            | RuntimeExtentId::CallArgumentFieldAxis { .. } => Err(RunnerError::internal(
                span,
                "runtime shape-size source was not lowered to a function parameter",
            )),
        }
    }

    fn function_parameter_value(&self, parameter: u32, span: Span) -> Result<Value, RunnerError> {
        let frame_index =
            self.frames.len().checked_sub(1).ok_or_else(|| {
                RunnerError::internal(span, "runtime shape-size has no active frame")
            })?;
        let frame = self.frames.get(frame_index).ok_or_else(|| {
            RunnerError::internal(span, "runtime shape-size frame is unavailable")
        })?;
        let function = self.function_by_id(frame.function, span)?;
        let parameter = usize::try_from(parameter)
            .ok()
            .and_then(|ordinal| function.params.get(ordinal))
            .ok_or_else(|| {
                RunnerError::internal(span, "runtime shape-size parameter is out of range")
            })?;
        self.read_local_at(frame_index, parameter.local, span)
    }

    fn runtime_field_value(value: Value, field: Symbol, span: Span) -> Result<Value, RunnerError> {
        let (Value::Record(fields, _) | Value::Struct(_, fields) | Value::EnumVariant(_, fields)) =
            value
        else {
            return Err(RunnerError::internal(
                span,
                "runtime shape field source is not a record",
            ));
        };
        fields
            .borrow()
            .get(&field)
            .cloned()
            .ok_or_else(|| RunnerError::internal(span, "runtime shape field source is missing"))
    }

    fn tensor_axis_extent(value: Value, axis: u32, span: Span) -> Result<u64, RunnerError> {
        let Value::Tensor(tensor) = value else {
            return Err(RunnerError::internal(
                span,
                "runtime shape axis source is not a tensor",
            ));
        };
        let axis = usize::try_from(axis)
            .map_err(|_| RunnerError::internal(span, "runtime shape axis does not fit usize"))?;
        let extent = tensor.borrow().shape.get(axis).copied().ok_or_else(|| {
            RunnerError::internal(span, "runtime tensor shape axis is out of range")
        })?;
        u64::try_from(extent)
            .map_err(|_| RunnerError::internal(span, "runtime tensor shape extent is negative"))
    }

    fn interned_textus(&self, symbol: Symbol) -> String {
        let mut cache = self.textus_literals.borrow_mut();
        if let Some(cached) = cache.get(&symbol) {
            return cached.clone();
        }
        let decoded =
            radix_lexer::textus_literal::decode_textus_literal(self.interner.resolve(symbol));
        cache.insert(symbol, decoded.clone());
        decoded
    }

    fn interned_ascii(&self, symbol: Symbol) -> String {
        let mut cache = self.ascii_literals.borrow_mut();
        if let Some(cached) = cache.get(&symbol) {
            return cached.clone();
        }
        let owned = decode_ascii_payload(self.interner.resolve(symbol));
        cache.insert(symbol, owned.clone());
        owned
    }

    fn read_place(&mut self, place: &MirPlace, span: Span) -> Result<Value, RunnerError> {
        let frame = self
            .frames
            .len()
            .checked_sub(1)
            .ok_or_else(|| RunnerError::internal(span, "no active frame"))?;
        self.read_place_at(frame, place, span)
    }

    /// [`Self::read_place`] against an explicit frame: write-backs and other
    /// cross-frame helpers resolve places in the caller, not the top frame.
    fn read_place_at(
        &mut self,
        frame_index: usize,
        place: &MirPlace,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let mut value = self.read_base_place(frame_index, &place.base, span)?;
        for projection in &place.projections {
            value = self.project(value, projection, span)?;
        }
        Ok(value)
    }

    fn write_place(
        &mut self,
        place: &MirPlace,
        value: Value,
        span: Span,
    ) -> Result<(), RunnerError> {
        let frame = self
            .frames
            .len()
            .checked_sub(1)
            .ok_or_else(|| RunnerError::internal(span, "no active frame"))?;
        self.write_place_at(frame, place, value, span)
    }

    /// [`Self::write_place`] against an explicit frame. Caller aliases and
    /// the store-side f32 rounding live in [`Self::write_base_place`], so a
    /// write-back into a caller local that is itself a `mut` alias keeps
    /// resolving and narrowing like any other store.
    fn write_place_at(
        &mut self,
        frame_index: usize,
        place: &MirPlace,
        value: Value,
        span: Span,
    ) -> Result<(), RunnerError> {
        if place.projections.is_empty() {
            return self.write_base_place(frame_index, &place.base, value, span);
        }

        let parent = MirPlace {
            base: place.base.clone(),
            projections: place.projections[..place.projections.len() - 1].to_vec(),
        };
        // `is_empty()` returned above, so the last projection is present.
        let last = &place.projections[place.projections.len() - 1];
        let mut current = self.read_place_at(frame_index, &parent, span)?;
        current = self.project_write(current, last, value, span)?;
        self.write_place_at(frame_index, &parent, current, span)
    }

    fn write_base_place(
        &mut self,
        frame_index: usize,
        base: &MirPlaceBase,
        value: Value,
        span: Span,
    ) -> Result<(), RunnerError> {
        if let MirPlaceBase::Local(id) = base {
            let alias = self
                .frames
                .get(frame_index)
                .and_then(|frame| frame.aliases.get(id))
                .cloned();
            if let Some(alias) = alias {
                return self.write_base_place(alias.frame, &alias.base, value, span);
            }
        }
        let frame = self
            .frames
            .get_mut(frame_index)
            .ok_or_else(|| RunnerError::internal(span, "no active frame"))?;
        // A value has no width until it is stored: a float stored into a bare
        // `f32` cell rounds to the nearest binary32 (ties to even).
        let narrows_to_f32 = matches!(value, Value::Float(_))
            && self
                .prepared
                .slot_types
                .get(&frame.function)
                .is_some_and(|slots| match base {
                    MirPlaceBase::Local(id) => slots.local_is_f32(*id),
                    MirPlaceBase::Temp(id) => slots.temp_is_f32(*id),
                });
        let value = match value {
            Value::Float(v) if narrows_to_f32 => Value::Float(f64::from(v as f32)),
            other => other,
        };
        match base {
            MirPlaceBase::Local(id) => {
                write_slot(&mut frame.locals, id.0 as usize, value, span, "local")?;
            }
            MirPlaceBase::Temp(id) => {
                write_slot(&mut frame.temps, id.0 as usize, value, span, "temp")?;
            }
        }
        Ok(())
    }

    /// An element read the checker proved in range (`ProvenIndex`, need
    /// 5d3a8afd): `0 <= index` and `index <` the list's live length or the
    /// tensor's extent. The list read builds no bounds fault and a dense
    /// tensor read skips the logical-extent comparison; a proof that turns out
    /// wrong is a compiler bug (a Rust panic for a list, the storage's own
    /// miss for a tensor), never a silent wrong read. Text, windows, maps,
    /// cursors, octeti and vector lanes keep the checked read: the scalar
    /// lookup of a text has no separate length test to omit.
    pub(crate) fn collection_index_proven(
        receiver: Value,
        index: &Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Some(at) = crate::value::proven_position(index) else {
            return Self::collection_index(receiver, index, span);
        };
        match receiver {
            Value::Array(cell) => Ok(cell.borrow()[at].clone()),
            Value::Tensor(tensor) => tensor
                .borrow()
                .value_at_proven(at)
                .ok_or_else(|| RunnerError::internal(span, "tensor index out of bounds")),
            other => Self::collection_index(other, index, span),
        }
    }

    fn project_write(
        &mut self,
        value: Value,
        projection: &radix_mir::MirProjection,
        assigned: Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match projection {
            // A proven list element write: the checker showed
            // `0 <= index < live length`, so the slot is written without the
            // negative and past-the-end tests. A wrong proof is a compiler
            // bug and panics in the slice index, never a silent wrong write.
            // Every other receiver keeps the checked write below (a tensor
            // store has no separate extent test to omit).
            radix_mir::MirProjection::ProvenIndex(index) if matches!(value, Value::Array(_)) => {
                let index = self.eval_operand(index, span)?;
                let Some(at) = value::proven_position(&index) else {
                    return Err(RunnerError::internal(span, trap_text::NEGATIVE_INDEX));
                };
                let Value::Array(cell) = value else {
                    return Err(RunnerError::internal(span, "index write on non-array"));
                };
                cell.borrow_mut()[at] = assigned;
                Ok(Value::Array(cell))
            }
            radix_mir::MirProjection::Index(index)
            | radix_mir::MirProjection::ProvenIndex(index) => {
                let index = self.eval_operand(index, span)?;
                match value {
                    Value::Array(cell) => {
                        let idx = index.as_index(span)?;
                        {
                            let mut borrow = cell.borrow_mut();
                            let slot = borrow.get_mut(idx).ok_or_else(|| {
                                RunnerError::internal(span, trap_text::LIST_INDEX_OUT_OF_BOUNDS)
                            })?;
                            *slot = assigned;
                        }
                        Ok(Value::Array(cell))
                    }
                    Value::Vector(lanes) => {
                        let idx = index.as_index(span)?;
                        {
                            let mut borrow = lanes.borrow_mut();
                            let slot = borrow.get_mut(idx).ok_or_else(|| {
                                RunnerError::internal(span, trap_text::LIST_INDEX_OUT_OF_BOUNDS)
                            })?;
                            *slot = assigned;
                        }
                        Ok(Value::Vector(lanes))
                    }
                    Value::Map(cell) => {
                        let key = index.to_key(span)?;
                        cell.borrow_mut().insert(key, assigned);
                        Ok(Value::Map(cell))
                    }
                    Value::Ledger(cell) => {
                        let key = index.to_key(span)?;
                        cell.borrow_mut().insert(key, assigned);
                        Ok(Value::Ledger(cell))
                    }
                    // A device-form kernel stores through a tensor place with one scalar
                    // linear index (`_1[%2] = v`); the tensor handle is shared, so the
                    // write lands in the caller's storage.
                    Value::Tensor(cell) => {
                        let idx = index.as_index(span)?;
                        cell.borrow().set_value_at(idx, assigned).ok_or_else(|| {
                            RunnerError::internal(span, "tensor index out of bounds")
                        })?;
                        Ok(Value::Tensor(cell))
                    }
                    _ => Err(RunnerError::internal(
                        span,
                        "index write on non-array or non-map",
                    )),
                }
            }
            radix_mir::MirProjection::Field(field) => match value {
                Value::Record(cell, order) => {
                    cell.borrow_mut().insert(*field, assigned);
                    Ok(Value::Record(cell, order))
                }
                Value::Struct(def, cell) => {
                    cell.borrow_mut().insert(*field, assigned);
                    Ok(Value::Struct(def, cell))
                }
                _ => Err(RunnerError::internal(
                    span,
                    "field write on non-record aggregate",
                )),
            },
            radix_mir::MirProjection::VariantField { variant, field } => match value {
                Value::EnumVariant(def, cell) if def == *variant => {
                    cell.borrow_mut().insert(*field, assigned);
                    Ok(Value::EnumVariant(def, cell))
                }
                _ => Err(RunnerError::internal(span, "variant field write mismatch")),
            },
            radix_mir::MirProjection::VectorLane(lane) => match value {
                Value::Vector(lanes) => {
                    {
                        let mut borrow = lanes.borrow_mut();
                        let Some(slot) = borrow.get_mut(usize::from(*lane)) else {
                            return Err(RunnerError::internal(
                                span,
                                "vector lane write out of bounds",
                            ));
                        };
                        *slot = assigned;
                    }
                    Ok(Value::Vector(lanes))
                }
                _ => Err(RunnerError::internal(span, "vector lane write base")),
            },
            radix_mir::MirProjection::MatrixCell { row, column } => match value {
                Value::Matrix(matrix) => {
                    {
                        let mut borrow = matrix.borrow_mut();
                        let Some(slot) = borrow.cell_mut(*row, *column) else {
                            return Err(RunnerError::internal(
                                span,
                                "matrix cell write out of bounds",
                            ));
                        };
                        *slot = assigned;
                    }
                    Ok(Value::Matrix(matrix))
                }
                _ => Err(RunnerError::internal(span, "matrix cell write base")),
            },
            radix_mir::MirProjection::SharedCell => match value {
                Value::SharedCell(cell) => {
                    *cell.borrow_mut() = assigned;
                    Ok(Value::SharedCell(cell))
                }
                _ => Err(RunnerError::internal(
                    span,
                    "shared cell write base is not a cell",
                )),
            },
            // Store through the environment's shared map. This also keeps a
            // re-stored `SharedCell` capture unchanged while making value-slot
            // writes visible to subsequent reads of the closure environment.
            radix_mir::MirProjection::ClosureCapture {
                environment,
                source,
            } => {
                let Value::ClosureEnvironment(value_environment, cell) = value else {
                    return Err(RunnerError::internal(
                        span,
                        "closure capture projection base is not an environment",
                    ));
                };
                if value_environment != *environment {
                    return Err(RunnerError::internal(
                        span,
                        "closure capture environment mismatch",
                    ));
                }
                let Some(metadata) = self.context.closure_environments.get(environment) else {
                    return Err(RunnerError::internal(
                        span,
                        "closure capture metadata is missing",
                    ));
                };
                let Some(name) = metadata
                    .captures
                    .iter()
                    .find(|capture| capture.source == *source)
                    .map(|capture| capture.name)
                else {
                    return Err(RunnerError::internal(
                        span,
                        "closure capture source is missing",
                    ));
                };
                cell.borrow_mut().insert(name, assigned);
                Ok(Value::ClosureEnvironment(value_environment, cell))
            }
        }
    }

    // Test-only accessor: production reads go through `read_place`, which
    // resolves the current frame uniformly.
    #[cfg(test)]
    fn read_local(&self, id: MirLocalId, span: Span) -> Result<Value, RunnerError> {
        let frame = self
            .frames
            .len()
            .checked_sub(1)
            .ok_or_else(|| RunnerError::internal(span, "no active frame"))?;
        self.read_local_at(frame, id, span)
    }

    fn read_local_at(
        &self,
        frame_index: usize,
        id: MirLocalId,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let alias = self
            .frames
            .get(frame_index)
            .and_then(|frame| frame.aliases.get(&id))
            .cloned();
        if let Some(alias) = alias {
            return self.read_base_place(alias.frame, &alias.base, span);
        }
        self.frames
            .get(frame_index)
            .and_then(|frame| frame.locals.get(id.0 as usize))
            .and_then(|slot| slot.as_ref())
            .cloned()
            .ok_or_else(|| RunnerError::internal(span, format!("uninitialized local l{}", id.0)))
    }

    fn read_base_place(
        &self,
        frame_index: usize,
        base: &MirPlaceBase,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match base {
            MirPlaceBase::Local(id) => self.read_local_at(frame_index, *id, span),
            MirPlaceBase::Temp(id) => self
                .frames
                .get(frame_index)
                .and_then(|frame| frame.temps.get(id.0 as usize))
                .and_then(|slot| slot.as_ref())
                .cloned()
                .ok_or_else(|| {
                    RunnerError::internal(span, format!("uninitialized temp t{}", id.0))
                }),
        }
    }

    fn read_temp(&self, id: MirTempId, span: Span) -> Result<Value, RunnerError> {
        self.frames
            .last()
            .and_then(|frame| frame.temps.get(id.0 as usize))
            .and_then(|slot| slot.as_ref())
            .cloned()
            .ok_or_else(|| RunnerError::internal(span, format!("uninitialized temp t{}", id.0)))
    }

    fn read_value(&self, id: MirValueId, span: Span) -> Result<Value, RunnerError> {
        self.frames
            .last()
            .and_then(|frame| frame.values.get(&id))
            .cloned()
            .ok_or_else(|| RunnerError::internal(span, format!("uninitialized value v{}", id.0)))
    }

    fn resolve_int_width(&self, ty: radix_mir::MirType) -> NumericWidth {
        // The result `MirType` carries the semantic `numerus<W>` / `fractus<W>`
        // width. For integer ops this is the overflow boundary; for floats it
        // is unused (f32/f64 fidelity is a separate gap). Fall back to i64 only
        // if the type is missing or non-numeric — a defensive no-op.
        effective_width(self.types, ty.semantic_id()).unwrap_or(NumericWidth::I64)
    }

    /// The raw-bits width (`b8 b16 b32 b64`) when `ty` is a bits type
    ///; bit operations on it act at exactly that width.
    fn bits_width(&self, ty: radix_mir::MirType) -> Option<radix_types::BitsWidth> {
        match self.types.get(ty.semantic_id()) {
            Type::Bits(width) => Some(*width),
            _ => None,
        }
    }

    fn modular_word_width(&self, ty: radix_mir::MirType) -> Option<NumericWidth> {
        match self.types.get(ty.semantic_id()) {
            Type::ModularWord(width) => Some(*width),
            _ => None,
        }
    }

    /// Width of a modular word operand, following type aliases. A bitwise
    /// intermediate can lose its word result type even though the other
    /// multiplication operand remains a `wrapping<W>` value.
    fn operand_modular_word_width(&self, operand: &MirOperand) -> Option<NumericWidth> {
        let mut ty = self.operand_static_type(operand)?;
        loop {
            match self.types.get(ty) {
                Type::ModularWord(width) => return Some(*width),
                Type::Alias(_, inner) => ty = *inner,
                _ => return None,
            }
        }
    }

    /// Whether an operand's *static* type is a `wrapping<W>` word, following
    /// type aliases to their underlying word. Range-based typing can give
    /// arithmetic on such operands a plain integer result, so the result type
    /// alone cannot decide whether `/` is modular.
    fn operand_is_modular_word(&self, operand: &MirOperand) -> bool {
        let Some(mut ty) = self.operand_static_type(operand) else {
            return false;
        };
        loop {
            match self.types.get(ty) {
                Type::ModularWord(_) => return true,
                Type::Alias(_, inner) => ty = *inner,
                _ => return false,
            }
        }
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "sqrt/exp/ln promote Int/UInt operands to f64 for computation, deliberately accepting f64 mantissa precision for large integer magnitudes"
    )]
    fn eval_unop(
        &mut self,
        op: MirUnOp,
        operand: &MirOperand,
        result_ty: radix_mir::MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let width = self.resolve_int_width(result_ty);
        let value = self.eval_operand(operand, span)?;
        let value = if width.is_decimal() {
            self.normalize_decimal_value(value, result_ty.semantic_id(), span)?
        } else {
            value
        };
        let modular = self.modular_word_width(result_ty);
        match op {
            MirUnOp::Neg if matches!(value, Value::Dec(..)) => decimal::neg(&value, span),
            MirUnOp::Neg => {
                if let Some(width) = modular {
                    modular_word_neg(value, width, span)
                } else {
                    match exact_int::exact_neg(&value, width, span) {
                        Some(result) => result,
                        None => match value {
                            Value::Float(v) => Ok(Value::Float(-v)),
                            _ => Err(RunnerError::internal(span, "negation type mismatch")),
                        },
                    }
                }
            }
            MirUnOp::Not => value.as_bool(span).map(|value| Value::Bool(!value)),
            MirUnOp::BitNot if let Some(bits) = self.bits_width(result_ty) => {
                value::bits_not(&value, bits, span)
            }
            MirUnOp::BitNot => {
                if let Some(width) = modular {
                    modular_word_bitnot(value, width, span)
                } else {
                    exact_int::exact_bitnot(&value, width, span).unwrap_or_else(|| {
                        Err(RunnerError::internal(
                            span,
                            "bitnot requires numerus operand",
                        ))
                    })
                }
            }
            MirUnOp::IsNil => Ok(Value::Bool(matches!(value, Value::Nil))),
            MirUnOp::IsNotNil => Ok(Value::Bool(!matches!(value, Value::Nil))),
            MirUnOp::Sqrt => match value {
                Value::Float(v) if v >= 0.0 => Ok(Value::Float(v.sqrt())),
                Value::Float(_) => Err(RunnerError::internal(span, "sqrt of negative float")),
                Value::Int(v) if v >= 0 => Ok(Value::Float((v as f64).sqrt())),
                Value::UInt(v) => Ok(Value::Float((v as f64).sqrt())),
                Value::Tensor(tensor) => self.map_tensor_unary(&tensor, span, &mut |v| match v {
                    Value::Float(f) if *f >= 0.0 => Ok(Value::Float(f.sqrt())),
                    Value::Float(_) => {
                        Err(RunnerError::internal(span, "tensor sqrt of negative float"))
                    }
                    Value::Int(i) if *i >= 0 => Ok(Value::Float((*i as f64).sqrt())),
                    Value::UInt(u) => Ok(Value::Float((*u as f64).sqrt())),
                    _ => Err(RunnerError::internal(
                        span,
                        "tensor sqrt element type mismatch",
                    )),
                }),
                _ => Err(RunnerError::internal(
                    span,
                    "sqrt requires non-negative numeric",
                )),
            },
            MirUnOp::Relu => match value {
                Value::Float(v) => Ok(Value::Float(v.max(0.0))),
                Value::Int(v) => Ok(Value::Int(v.max(0))),
                _ => Err(RunnerError::internal(span, "relu requires numeric operand")),
            },
            MirUnOp::Gelu => match value {
                Value::Float(v) => Ok(Value::Float(gelu_f64(v))),
                Value::Tensor(tensor) => self.map_tensor_unary(&tensor, span, &mut |v| match v {
                    Value::Float(f) => Ok(Value::Float(gelu_f64(*f))),
                    _ => Err(RunnerError::internal(
                        span,
                        "tensor gelu element type mismatch",
                    )),
                }),
                _ => Err(RunnerError::internal(span, "gelu requires float operand")),
            },
            MirUnOp::Tanh => match value {
                Value::Float(v) => Ok(Value::Float(v.tanh())),
                Value::Tensor(tensor) => self.map_tensor_unary(&tensor, span, &mut |v| match v {
                    Value::Float(f) => Ok(Value::Float(f.tanh())),
                    _ => Err(RunnerError::internal(
                        span,
                        "tensor tanh element type mismatch",
                    )),
                }),
                _ => Err(RunnerError::internal(span, "tanh requires float operand")),
            },
            MirUnOp::Exp => {
                let result = match value {
                    Value::Float(v) => v.exp(),
                    Value::Int(v) => (v as f64).exp(),
                    Value::UInt(v) => (v as f64).exp(),
                    Value::Tensor(tensor) => {
                        return self.map_tensor_unary(&tensor, span, &mut |v| match v {
                            Value::Float(f) => {
                                let r = f.exp();
                                if r.is_finite() {
                                    Ok(Value::Float(r))
                                } else {
                                    Err(RunnerError::internal(span, "tensor exp overflow"))
                                }
                            }
                            _ => Err(RunnerError::internal(
                                span,
                                "tensor exp element type mismatch",
                            )),
                        });
                    }
                    _ => return Err(RunnerError::internal(span, "exp requires numeric operand")),
                };
                if result.is_finite() {
                    Ok(Value::Float(result))
                } else {
                    Err(RunnerError::internal(span, "exp overflow"))
                }
            }
            MirUnOp::Log => {
                let result = match value {
                    Value::Float(v) if v > 0.0 => v.ln(),
                    Value::Int(v) if v > 0 => (v as f64).ln(),
                    Value::UInt(v) if v > 0 => (v as f64).ln(),
                    Value::Float(_) | Value::Int(_) | Value::UInt(_) => {
                        return Err(RunnerError::internal(span, "log requires positive input"));
                    }
                    _ => return Err(RunnerError::internal(span, "log requires numeric operand")),
                };
                if result.is_finite() {
                    Ok(Value::Float(result))
                } else {
                    Err(RunnerError::internal(
                        span,
                        "log produced non-finite result",
                    ))
                }
            }
            MirUnOp::F16Round => {
                // The half-precision round trip (the comparator's
                // `kv_f16_round` register rounding): f32 -> f16 (RN) -> f32.
                match value {
                    Value::Float(v) => Ok(Value::Float(f16_round_f64(v))),
                    Value::Int(v) => Ok(Value::Float(f16_round_f64(v as f64))),
                    Value::UInt(v) => Ok(Value::Float(f16_round_f64(v as f64))),
                    Value::Tensor(tensor) => {
                        self.map_tensor_unary(&tensor, span, &mut |element| match element {
                            Value::Float(v) => Ok(Value::Float(f16_round_f64(*v))),
                            _ => Err(RunnerError::internal(
                                span,
                                "tensor f16-round element type mismatch",
                            )),
                        })
                    }
                    _ => Err(RunnerError::internal(
                        span,
                        "f16-round requires numeric operand",
                    )),
                }
            }
        }
    }

    /// A decimal operand as `(carrier, scale digits)`: an exact intermediate
    /// carries its own scale, a stored carrier is scale 8 when the operand's
    /// static type is decimal.
    fn decimal_side(&self, operand: &MirOperand, value: &Value) -> Option<(i128, u32)> {
        match value {
            Value::Dec(carrier, scale) => Some((*carrier, *scale)),
            Value::Int(carrier) => self
                .operand_decimal_width(operand)
                .and_then(NumericWidth::decimal_scale)
                .map(|scale| (i128::from(*carrier), scale)),
            _ => None,
        }
    }

    /// Exact comparison of a decimal with a float, an integer or another
    /// decimal (ruling 22). Two stored carriers compare as plain integers, so
    /// that pair returns `None` and takes the ordinary path.
    fn decimal_comparison(
        &self,
        op: MirBinOp,
        lhs: &MirOperand,
        rhs: &MirOperand,
        left: &Value,
        right: &Value,
    ) -> Option<bool> {
        if !matches!(
            op,
            MirBinOp::Lt
                | MirBinOp::Gt
                | MirBinOp::LtEq
                | MirBinOp::GtEq
                | MirBinOp::Eq
                | MirBinOp::NotEq
        ) {
            return None;
        }
        let as_i128 = |value: &Value| match value {
            Value::Int(v) => Some(i128::from(*v)),
            Value::UInt(v) => Some(i128::from(*v)),
            _ => None,
        };
        let (l, r) = (self.decimal_side(lhs, left), self.decimal_side(rhs, right));
        let ordering = match (l, r) {
            (Some(a), Some(b)) => {
                if !matches!(left, Value::Dec(..)) && !matches!(right, Value::Dec(..)) {
                    return None;
                }
                Some(decimal::compare(a, b))
            }
            (Some((scaled, scale)), None) => match right {
                Value::Float(f) => exact_int::cmp_scaled_float(scaled, decimal::pow10(scale)?, *f),
                // An `inf` beyond the 64-bit carriers compares exactly (ruling 37).
                Value::Magnus(big) => {
                    Some(inf::cmp_magnus_scaled(big, scaled, decimal::pow10(scale)?).reverse())
                }
                other => Some(decimal::compare((scaled, scale), (as_i128(other)?, 0))),
            },
            (None, Some((scaled, scale))) => match left {
                Value::Float(f) => exact_int::cmp_scaled_float(scaled, decimal::pow10(scale)?, *f)
                    .map(std::cmp::Ordering::reverse),
                Value::Magnus(big) => {
                    Some(inf::cmp_magnus_scaled(big, scaled, decimal::pow10(scale)?))
                }
                other => Some(decimal::compare((as_i128(other)?, 0), (scaled, scale))),
            },
            (None, None) => return None,
        };
        // A NaN float compares unequal to everything and orders nowhere.
        Some(match op {
            MirBinOp::Lt => ordering.is_some_and(std::cmp::Ordering::is_lt),
            MirBinOp::Gt => ordering.is_some_and(std::cmp::Ordering::is_gt),
            MirBinOp::LtEq => ordering.is_some_and(std::cmp::Ordering::is_le),
            MirBinOp::GtEq => ordering.is_some_and(std::cmp::Ordering::is_ge),
            MirBinOp::Eq => ordering.is_some_and(std::cmp::Ordering::is_eq),
            _ => ordering.is_none_or(std::cmp::Ordering::is_ne),
        })
    }

    fn eval_binop(
        &mut self,
        op: MirBinOp,
        lhs: &MirOperand,
        rhs: &MirOperand,
        result_ty: radix_mir::MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let width = self.resolve_int_width(result_ty);
        let left = self.eval_operand(lhs, span)?;
        let right = self.eval_operand(rhs, span)?;
        let (left, right) = if width.is_decimal() {
            (
                self.normalize_decimal_value(left, result_ty.semantic_id(), span)?,
                self.normalize_decimal_value(right, result_ty.semantic_id(), span)?,
            )
        } else {
            (left, right)
        };
        if let Some(compared) = self.decimal_comparison(op, lhs, rhs, &left, &right) {
            return Ok(Value::Bool(compared));
        }
        if let Some(bits) = self.bits_width(result_ty)
            && matches!(
                op,
                MirBinOp::BitAnd
                    | MirBinOp::BitOr
                    | MirBinOp::BitXor
                    | MirBinOp::Shl
                    | MirBinOp::Shr
            )
        {
            return value::bits_binop(&left, &right, op, bits, span);
        }
        let modular = self.modular_word_width(result_ty);
        // Range-based typing can erase the `wrapping<u64>` result type from a
        // bitwise intermediate. When that value feeds multiplication, keep
        // the word's modular product instead of trapping before the wrapping
        // destination store.
        if modular.is_none()
            && op == MirBinOp::Mul
            && let Some(NumericWidth::U64) = self
                .operand_modular_word_width(lhs)
                .or_else(|| self.operand_modular_word_width(rhs))
        {
            return modular_word_numeric_binop(left, right, op, NumericWidth::U64, span);
        }
        if let Some(modular) = modular {
            return match op {
                MirBinOp::Add | MirBinOp::Sub | MirBinOp::Mul | MirBinOp::Div | MirBinOp::Mod => {
                    modular_word_numeric_binop(left, right, op, modular, span)
                }
                MirBinOp::Eq => Ok(Value::Bool(values_equal(&left, &right))),
                MirBinOp::NotEq => Ok(Value::Bool(!values_equal(&left, &right))),
                MirBinOp::Lt | MirBinOp::Gt | MirBinOp::LtEq | MirBinOp::GtEq => {
                    modular_word_compare_values(left, right, op, span)
                }
                MirBinOp::BitAnd
                | MirBinOp::BitOr
                | MirBinOp::BitXor
                | MirBinOp::Shl
                | MirBinOp::Shr => modular_word_bitwise_binop(left, right, op, modular, span),
                _ => Err(RunnerError::internal(span, "invalid modulus operator")),
            };
        }
        // A modular-origin `/` or `%` that range-based typing gave a plain
        // integer result still traps a zero divisor as a modular failure, before
        // the checked `numerus` dispatch below reports its own division issue.
        // A nonzero divisor runs the exact integer math and reduces at the
        // store, exactly as any other wrapping intermediate does.
        let modular_origin = self.operand_is_modular_word(lhs) || self.operand_is_modular_word(rhs);
        if let Some(error) = modular_zero_divisor(op, &right, modular_origin, span) {
            return Err(error);
        }
        match op {
            MirBinOp::Add | MirBinOp::Sub | MirBinOp::Mul | MirBinOp::Div | MirBinOp::Mod
                if width.is_decimal() =>
            {
                decimal_numeric_binop(left, right, op, width, span)
            }
            MirBinOp::Add => add_values(left, right, width, span),
            MirBinOp::Sub => numeric_binop(left, right, op, |a, b| a - b, width, span),
            MirBinOp::Mul => numeric_binop(left, right, op, |a, b| a * b, width, span),
            MirBinOp::Div => numeric_binop(left, right, op, |a, b| a / b, width, span),
            MirBinOp::Mod => {
                numeric_binop(left, right, op, exact_int::float_floor_rem, width, span)
            }
            MirBinOp::Eq => Ok(Value::Bool(values_equal(&left, &right))),
            MirBinOp::NotEq => Ok(Value::Bool(!values_equal(&left, &right))),
            MirBinOp::Lt | MirBinOp::Gt | MirBinOp::LtEq | MirBinOp::GtEq => {
                compare_values(left, right, op, span)
            }
            // WHY: MIR-level And/Or is eager. Short-circuiting is lowered to
            // Branch terminators before reaching the runner, so both operands
            // are already side-effect-free values by this point.
            MirBinOp::And => Ok(Value::Bool(left.as_bool(span)? && right.as_bool(span)?)),
            MirBinOp::Or => Ok(Value::Bool(left.as_bool(span)? || right.as_bool(span)?)),
            MirBinOp::Coalesce => {
                if matches!(left, Value::Nil) {
                    Ok(right)
                } else {
                    Ok(left)
                }
            }
            MirBinOp::BitAnd
            | MirBinOp::BitOr
            | MirBinOp::BitXor
            | MirBinOp::Shl
            | MirBinOp::Shr => int_bitwise_binop(left, right, op, width, span),
        }
    }
}

/// The `kind`/`path` facts a leaf-failure diagnostic carries, when it has
/// them. The path defaults to empty so a record is always two entries.
fn fault_record_args(error: &RunnerError) -> Option<(String, String)> {
    let kind = error.arg("fault_kind")?;
    Some((
        kind.to_owned(),
        error.arg("fault_path").unwrap_or_default().to_owned(),
    ))
}

/// Shape the alternate-exit payload for a runtime failure.
///
/// Only an ignotum or valor channel receives the two-entry fault record;
/// a textus channel receives the failure text itself, matching the payload
/// every non-leaf failure keeps.
fn failure_channel_value(
    channel: &Primitive,
    fault: Option<(String, String)>,
    message: String,
) -> Value {
    match (fault, channel) {
        (Some((kind, path)), Primitive::Ignotum | Primitive::Valor) => {
            routes::fault_record_value(&kind, &path)
        }
        _ => Value::Str(message),
    }
}

/// Decode a lexer `ascii` literal payload through the one escape table.
///
/// A checked program carries only payloads the typechecker already accepted,
/// so a malformed payload cannot occur here; keep the raw text if it does.
fn decode_ascii_payload(raw: &str) -> String {
    radix_lexer::ascii_literal::decode_ascii_literal(raw).unwrap_or_else(|_| raw.to_owned())
}

fn constant_equals_value(constant: &MirConstant, value: &Value, interner: &Interner) -> bool {
    match (constant, value) {
        (MirConstant::Int(left), Value::Int(right)) => left == right,
        (MirConstant::UInt(left), Value::UInt(right)) => left == right,
        // Carriers can legitimately mix (e.g. a signed literal case against an
        // unsigned switch subject); compare non-negative integers by value.
        (MirConstant::Int(left), Value::UInt(right)) => {
            u64::try_from(*left).is_ok_and(|left| left == *right)
        }
        (MirConstant::UInt(left), Value::Int(right)) => {
            u64::try_from(*right).is_ok_and(|right| *left == right)
        }
        #[allow(clippy::float_cmp)]
        (MirConstant::Float(left), Value::Float(right)) => left == right,
        (MirConstant::Bool(left), Value::Bool(right)) => left == right,
        // A switch case on an `inf` subject beyond the 64-bit carriers
        // (canonical `Magnus` values never equal an `Int`/`UInt` constant).
        (MirConstant::Magnus(digits), Value::Magnus(right)) => {
            handroll::Magnus::parse_decimal(digits).is_ok_and(|left| left == **right)
        }
        (MirConstant::String(symbol), Value::Str(right)) => {
            radix_lexer::textus_literal::decode_textus_literal(interner.resolve(*symbol)) == *right
        }
        (MirConstant::Ascii(symbol), Value::Str(right)) => {
            decode_ascii_payload(interner.resolve(*symbol)) == *right
        }
        (MirConstant::Ascii(symbol), Value::Ascii(right)) => {
            decode_ascii_payload(interner.resolve(*symbol)).as_bytes() == [*right]
        }
        (MirConstant::Regex { pattern, flags }, Value::Regex(right)) => {
            regex_pattern(
                interner.resolve(*pattern),
                flags.map(|flags| interner.resolve(flags)),
            ) == right.pattern()
        }
        (MirConstant::Nil, Value::Nil) | (MirConstant::Unit, Value::Unit) => true,
        _ => false,
    }
}

/// Value-id static types from `Assign` statements, keyed per function, for
/// display-width resolution at diagnostics.
fn index_value_types(program: &MirProgram) -> FxHashMap<(MirFunctionId, MirValueId), MirType> {
    let mut map = FxHashMap::default();
    for function in &program.functions {
        for block in &function.blocks {
            for statement in &block.statements {
                if let MirStatementKind::Assign { value, .. } = &statement.kind {
                    map.insert((function.id, value.id), value.ty);
                }
            }
        }
    }
    map
}

/// Functions that read a `MirOperand::Value` anywhere in their body.
fn index_value_reading_functions(program: &MirProgram) -> FxHashSet<MirFunctionId> {
    struct ValueReaders {
        current: Option<MirFunctionId>,
        readers: FxHashSet<MirFunctionId>,
    }
    impl radix_mir::visit::MirVisitor for ValueReaders {
        fn visit_function(&mut self, function: &MirFunction) {
            self.current = Some(function.id);
            radix_mir::visit::walk_function(self, function);
        }

        fn visit_operand(&mut self, operand: &MirOperand) {
            if let (MirOperand::Value(_), Some(function)) = (operand, self.current) {
                self.readers.insert(function);
            }
            radix_mir::visit::walk_operand(self, operand);
        }
    }
    let mut visitor = ValueReaders {
        current: None,
        readers: FxHashSet::default(),
    };
    radix_mir::visit::walk_program(&mut visitor, program);
    visitor.readers
}

/// Whether a statement writes `temp` through a plain temp place.
fn writes_temp(statement: &radix_mir::MirStatement, temp: MirTempId) -> bool {
    let (MirStatementKind::Assign { place, .. }
    | MirStatementKind::RuntimeCall {
        destination: Some(place),
        ..
    }
    | MirStatementKind::Call {
        destination: Some(place),
        ..
    }
    | MirStatementKind::Construct {
        destination: place, ..
    }) = &statement.kind
    else {
        return false;
    };
    matches!(&place.base, MirPlaceBase::Temp(id) if *id == temp) && place.projections.is_empty()
}

/// The entry block of a conversion's recovery region. The region tail is the
/// block outside the main-reachable set that writes the conversion's
/// destination temp; the entry is that region's root — the member of the
/// non-main-reachable set with no predecessor inside it.
fn recovery_region_entry(
    function: &MirFunction,
    destination: MirTempId,
    main_reachable: &FxHashSet<MirBlockId>,
) -> Option<MirBlockId> {
    let region_blocks: Vec<&MirBlock> = function
        .blocks
        .iter()
        .filter(|block| !main_reachable.contains(&block.id))
        .collect();
    let tail = region_blocks.iter().copied().find(|block| {
        block
            .statements
            .iter()
            .any(|statement| writes_temp(statement, destination))
    })?;
    let has_region_predecessor = |id: MirBlockId| {
        region_blocks
            .iter()
            .any(|block| terminator_successors(&block.terminator.kind).contains(&id))
    };
    // Backward walk over region-internal predecessors to the root.
    let mut frontier = vec![tail.id];
    let mut seen = FxHashSet::default();
    let mut entry = tail.id;
    while let Some(id) = frontier.pop() {
        if !seen.insert(id) {
            continue;
        }
        if !has_region_predecessor(id) {
            entry = id;
            break;
        }
        for block in &region_blocks {
            if terminator_successors(&block.terminator.kind).contains(&id) {
                frontier.push(block.id);
            }
        }
    }
    Some(entry)
}

fn reachable_block_ids(function: &MirFunction) -> FxHashSet<MirBlockId> {
    let mut reachable = FxHashSet::default();
    let mut stack = vec![MirBlockId(0)];
    reachable.insert(MirBlockId(0));
    while let Some(id) = stack.pop() {
        let Some(block) = function.block_by_id(id) else {
            continue;
        };
        for succ in terminator_successors(&block.terminator.kind) {
            if reachable.insert(succ) {
                stack.push(succ);
            }
        }
    }
    reachable
}

fn terminator_successors(kind: &MirTerminatorKind) -> Vec<MirBlockId> {
    match kind {
        MirTerminatorKind::Return(_)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::ReturnError(_)
        | MirTerminatorKind::Unreachable => Vec::new(),
        MirTerminatorKind::Goto(target) => vec![*target],
        MirTerminatorKind::Branch {
            then_block,
            else_block,
            ..
        } => vec![*then_block, *else_block],
        MirTerminatorKind::Switch { cases, default, .. } => {
            let mut targets = Vec::with_capacity(cases.len() + 1);
            targets.push(*default);
            targets.extend(cases.iter().map(|case| case.target));
            targets
        }
        MirTerminatorKind::TryCall {
            ok_block,
            error_block,
            ..
        } => vec![*ok_block, *error_block],
    }
}

fn regex_pattern(pattern: &str, flags: Option<&str>) -> String {
    let Some(flags) = flags else {
        return pattern.to_owned();
    };
    let mapped = flags
        .chars()
        .filter(|flag| matches!(flag, 'i' | 'm' | 's' | 'u' | 'U' | 'x'))
        .collect::<String>();
    if mapped.is_empty() {
        pattern.to_owned()
    } else {
        format!("(?{mapped}){pattern}")
    }
}

#[cfg(test)]
#[path = "entry_exit_test.rs"]
mod entry_exit_tests;

#[cfg(test)]
#[path = "lib_test.rs"]
mod lib_test;

#[cfg(test)]
#[path = "copy_into_test.rs"]
mod copy_into_test;

#[cfg(test)]
#[path = "exit_status_test.rs"]
mod exit_status_test;

#[cfg(test)]
#[path = "tensor_fill_test.rs"]
mod tensor_fill_test;

#[cfg(test)]
#[path = "tensor_extent_test.rs"]
mod tensor_extent_test;

#[cfg(test)]
#[path = "step_budget_test.rs"]
mod step_budget_test;

#[cfg(test)]
#[path = "kernel_lanes_test.rs"]
mod kernel_lanes_test;

#[cfg(test)]
#[path = "tensor_math_test.rs"]
mod tensor_math_test;

#[cfg(test)]
#[path = "conversion_parity_test.rs"]
mod conversion_parity_test;

#[cfg(test)]
#[path = "device_block_test.rs"]
mod device_block_test;

#[cfg(test)]
#[path = "recovery_block_test.rs"]
mod recovery_block_test;

#[cfg(test)]
#[path = "octeti_index_test.rs"]
mod octeti_index_test;

#[cfg(test)]
#[path = "octeti_endian_test.rs"]
mod octeti_endian_test;

#[cfg(test)]
#[path = "octeti_tensor_test.rs"]
mod octeti_tensor_test;

#[cfg(test)]
#[path = "ascii_format_pack_test.rs"]
mod ascii_format_pack_test;

#[cfg(test)]
#[path = "bounded_textus_test.rs"]
mod bounded_textus_test;

#[cfg(test)]
#[path = "octeti_lista_test.rs"]
mod octeti_lista_test;

#[cfg(test)]
mod octeti_valor_test;

#[cfg(test)]
mod valor_numerus_width_test;

#[cfg(test)]
#[path = "vector_lista_test.rs"]
mod vector_lista_test;

#[cfg(test)]
#[path = "tabula_lista_test.rs"]
mod tabula_lista_test;

#[cfg(test)]
#[path = "ascii1_index_test.rs"]
mod ascii1_index_test;

#[cfg(test)]
#[path = "ascii_cursor_test.rs"]
mod ascii_cursor_test;

#[cfg(test)]
#[path = "textus_index_test.rs"]
mod textus_index_test;

#[cfg(test)]
#[path = "proven_index_test.rs"]
mod proven_index_test;
#[cfg(test)]
#[path = "textus_bracket_test.rs"]
mod textus_bracket_test;

#[cfg(test)]
#[path = "collection_index_plain_test.rs"]
mod collection_index_plain_test;

#[cfg(test)]
#[path = "record_element_test.rs"]
mod record_element_test;

#[cfg(test)]
#[path = "tensor_get_direct_test.rs"]
mod tensor_get_direct_test;

#[cfg(test)]
#[path = "matrix_get_set_test.rs"]
mod matrix_get_set_test;

#[cfg(test)]
#[path = "capability_test.rs"]
mod capability_test;

#[cfg(test)]
#[path = "kernel_dispatch_test.rs"]
mod kernel_dispatch_test;

#[cfg(test)]
#[path = "value_test.rs"]
mod value_test;

#[cfg(test)]
#[path = "decimal_test.rs"]
mod decimal_test;

#[cfg(test)]
#[path = "decimal_signum_test.rs"]
mod decimal_signum_test;

#[cfg(test)]
#[path = "policy_float_test.rs"]
mod policy_float_test;

#[cfg(test)]
#[path = "f32_store_test.rs"]
mod f32_store_test;

#[cfg(test)]
#[path = "host_read_all_test.rs"]
mod host_read_all_test;

#[cfg(test)]
#[path = "narrowing_conversio_test.rs"]
mod narrowing_conversio_test;

#[cfg(test)]
#[path = "conversion_flavor_test.rs"]
mod conversion_flavor_test;

#[cfg(test)]
#[path = "ascii_conversio_test.rs"]
mod ascii_conversio_test;

#[cfg(test)]
#[path = "inf_test.rs"]
mod inf_test;

#[cfg(test)]
#[path = "decimal_shared_agreement_test.rs"]
mod decimal_shared_agreement_test;

#[cfg(test)]
#[path = "extremum_test.rs"]
mod extremum_test;

#[cfg(test)]
#[path = "arg_extremum_test.rs"]
mod arg_extremum_test;

#[cfg(test)]
#[path = "queue_stack_test.rs"]
mod queue_stack_test;

#[cfg(test)]
#[path = "tensor_from_flat_failure_test.rs"]
mod tensor_from_flat_failure_test;

#[cfg(test)]
#[path = "set_none_key_test.rs"]
mod set_none_key_test;

#[cfg(test)]
#[path = "tensor_count_test.rs"]
mod tensor_count_test;

#[cfg(test)]
#[path = "stats_test.rs"]
mod stats_test;

#[cfg(test)]
#[path = "format_value_test.rs"]
mod format_value_test;

#[cfg(test)]
#[path = "scribe_scriptum_parity_test.rs"]
mod scribe_scriptum_parity_test;

#[cfg(test)]
#[path = "shuffle_test.rs"]
mod shuffle_test;

#[cfg(test)]
#[path = "value_json_order_test.rs"]
mod value_json_order_test;

#[cfg(test)]
mod exact_int_test;

#[cfg(test)]
mod store_boundary_test;

#[cfg(test)]
mod ranged_store_test;

#[cfg(test)]
#[path = "remainder_floor_test.rs"]
mod remainder_floor_test;
#[cfg(test)]
#[path = "tensor_lift_test.rs"]
mod tensor_lift_test;

#[cfg(test)]
#[path = "vector_lane_select_test.rs"]
mod vector_lane_select_test;

#[cfg(test)]
#[path = "residency_test.rs"]
mod residency_test;

#[cfg(test)]
#[path = "dense_test.rs"]
mod dense_test;

#[cfg(test)]
#[path = "value_dense_test.rs"]
mod value_dense_test;

#[cfg(test)]
#[path = "softmax_rank_test.rs"]
mod softmax_rank_test;

#[cfg(test)]
#[path = "causal_softmax_test.rs"]
mod causal_softmax_test;

#[cfg(test)]
#[path = "sermo_frame_test.rs"]
mod sermo_frame_test;

#[cfg(test)]
#[path = "display_tokens_test.rs"]
mod display_tokens_test;

#[cfg(test)]
#[path = "round_half_even_test.rs"]
mod round_half_even_test;

#[cfg(test)]
#[path = "stepped_interval_test.rs"]
mod stepped_interval_test;
