//! In-process MIR runner for Faber scripting.
//!
//! TARGET: faber run (single-file interpret path)
//! WHY: Wasm already executes MIR end-to-end; this module dispatches the same
//! intrinsics directly without emit/instantiate.

#![allow(clippy::match_same_arms, clippy::too_many_lines, clippy::map_unwrap_or)]

pub mod capability;
mod conversio;
mod host;
pub mod kernel;
mod runtime;
mod textus_literal;
pub mod value;

pub use capability::{classify_runner_lowerability, runner_dispatch_status, RunnerDispatchStatus};

pub use host::{BufferHost, Host, StdioHost};
pub use value::{MapKey, Value};

use radix_hir::DefId;
use radix_lexer::{Interner, Span, Symbol};
use radix_mir::{
    MirBinOp, MirBlock, MirBlockId, MirCallee, MirConstant, MirFunction, MirFunctionId, MirLocalId,
    MirOperand, MirParamMode, MirPlace, MirPlaceBase, MirProgram, MirStatementKind, MirTempId,
    MirTerminatorKind, MirUnOp, MirValidationContext, MirValue, MirValueId, MirValueKind,
    ValidatedMir,
};
use radix_types::{effective_width, NumericWidth, Primitive, Type, TypeTable};
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;
use std::rc::Rc;
use value::{
    add_values, compare_values, int_bitwise_binop, modular_word_bitnot, modular_word_bitwise_binop,
    modular_word_compare_values, modular_word_neg, modular_word_numeric_binop, numeric_binop,
    uint_width_checked_div, uint_width_checked_mul, uint_width_checked_neg, uint_width_checked_rem,
    uint_width_checked_sub, values_equal, width_checked_div, width_checked_mul, width_checked_neg,
    width_checked_rem, width_checked_sub, GradientStorage, TensorValue,
};

pub(crate) const RUNNER_UNSUPPORTED_PREFIX: &str = "faberscript unsupported";

/// Stable diagnostic code for unsupported runner operations (D-02-07).
pub(crate) const RUNNER_CODE_UNSUPPORTED: &str = "runner_unsupported";

/// Stable diagnostic code for internal runner failures (D-02-07).
pub(crate) const RUNNER_CODE_INTERNAL: &str = "runner_internal";

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

    fn is_conversion_failure(&self) -> bool {
        // WHY: both parse failures ("... conversion failed") and width-overflow
        // failures ("... conversion out of range", SA-002) are recoverable
        // `cape`-able conversion failures — parity with Rust codegen's fallible
        // `.parse::<W>()` path.
        self.message.ends_with(" conversion failed")
            || self.message.ends_with(" conversion out of range")
    }
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

#[derive(Debug, Clone)]
struct PlaceAlias {
    /// Frame containing the caller place. The caller remains on the stack for
    /// the lifetime of the callee frame, so this is a stable in-runner cell.
    frame: usize,
    place: MirPlace,
}

struct Frame {
    function: MirFunctionId,
    locals: FxHashMap<MirLocalId, Value>,
    /// `in` parameter locals that borrow a caller place instead of owning a
    /// cloned scalar value.
    aliases: FxHashMap<MirLocalId, PlaceAlias>,
    temps: FxHashMap<MirTempId, Value>,
    values: FxHashMap<MirValueId, Value>,
    block: MirBlockId,
    /// `true` while this frame runs a `@ cursor` generator whose `cede` yields
    /// collect into [`Runner::generator_yields`] rather than awaiting.
    is_generator: bool,
}

enum CallOutcome {
    Ok(Value),
    Err(Value),
}

enum StatementOutcome {
    Continue,
    FrameReturned,
}

pub(crate) struct Runner<'a, H: Host + ?Sized> {
    program: &'a MirProgram,
    context: &'a MirValidationContext<'a>,
    types: &'a TypeTable,
    interner: &'a Interner,
    host: &'a mut H,
    frames: Vec<Frame>,
    errors: Vec<RunnerError>,
    /// Remaining-capacity counter: `None` means unlimited. One unit is one
    /// MIR basic-block visit in `run_until_depth`.
    step_limit: Option<u64>,
    steps_taken: u64,
    return_value: Option<Value>,
    return_error: Option<Value>,
    /// Stack of yield buffers, one per active `CursorStream` materialization.
    /// `cede` inside a generator frame (`Frame::is_generator`) appends here; the
    /// matching `eval_cursor_stream` pops the buffer as the materialized `lista<T>`.
    generator_yields: Vec<Vec<Value>>,
    /// `DefId` → `MirFunctionId` index built once at construction (BO-M-6).
    def_to_function: FxHashMap<DefId, MirFunctionId>,
    /// Per-step block share: one clone at construct, `Rc` bump in the loop (BO-M-1).
    block_cache: FxHashMap<(MirFunctionId, MirBlockId), Rc<MirBlock>>,
    /// Opaque gradient-handle storage: handle id → accumulated gradient
    /// tensor (`MirIntrinsic::Gradient` dispatch). Clean-break internal
    /// contract mirroring the host LLVM gradient ABI; in-process only.
    gradient_handles: FxHashMap<u64, GradientStorage>,
    /// Next gradient handle id (monotonic, never reused).
    next_gradient_handle: u64,
    /// Decoded `textus` literals, keyed by interned source symbol.
    textus_literals: RefCell<FxHashMap<Symbol, String>>,
    /// Owned ASCII literals, keyed by interned source symbol.
    ascii_literals: RefCell<FxHashMap<Symbol, String>>,
    /// Side-effecting `⇥` recovery calls parked in unreachable orphan blocks.
    /// Looked up by `(function, dest temp)` so conversion failure is O(1).
    recovery_calls: FxHashMap<(MirFunctionId, MirTempId), (MirCallee, Vec<MirOperand>)>,
}

impl<'a, H: Host + ?Sized> Runner<'a, H> {
    /// Build a runner with O(1) lookup indexes for functions, defs, and blocks.
    pub(crate) fn new(
        program: &'a MirProgram,
        context: &'a MirValidationContext<'a>,
        types: &'a TypeTable,
        interner: &'a Interner,
        host: &'a mut H,
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
            program,
            context,
            types,
            interner,
            host,
            frames: Vec::new(),
            errors: Vec::new(),
            step_limit: None,
            steps_taken: 0,
            return_value: None,
            return_error: None,
            generator_yields: Vec::new(),
            def_to_function,
            block_cache,
            gradient_handles: FxHashMap::default(),
            next_gradient_handle: 0,
            textus_literals: RefCell::new(FxHashMap::default()),
            ascii_literals: RefCell::new(FxHashMap::default()),
            recovery_calls: index_orphan_recovery_calls(program),
        }
    }
}

/// Run the entry (`incipit`) function to completion.
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be executed.
pub fn run_entry<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    host: &mut H,
) -> Result<(), Vec<RunnerError>> {
    let context = validated.validation();
    let program = validated.program();
    let interner = context.interner.ok_or_else(|| {
        vec![RunnerError::internal(
            Span::default(),
            "runner requires interner context",
        )]
    })?;
    let names = radix_mir::names::MirNames::new(program, context.types, interner);
    let Some(entry) = names.explicit_entry_function() else {
        return Err(vec![RunnerError::internal(
            Span::default(),
            "no entry function in MIR program",
        )]);
    };
    run_function(validated, entry.id, host)
}

/// Run a specific MIR function to completion (used by proba case execution).
///
/// The function must take no parameters. Assertion failures and host aborts
/// surface through the host's `abort` / runner error paths.
///
/// # Errors
///
/// Returns a list of runner errors if the program cannot be executed.
pub fn run_function<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
) -> Result<(), Vec<RunnerError>> {
    run_function_with_step_budget(validated, function_id, host, None)
}

/// Run a specific MIR function with an optional deterministic step budget.
///
/// One step is one MIR basic-block visit. `None` is unlimited. Exhaustion
/// records `interpreter step budget exhausted after N steps` and stops.
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
    let context = validated.validation();
    let program = validated.program();
    let interner = context.interner.ok_or_else(|| {
        vec![RunnerError::internal(
            Span::default(),
            "runner requires interner context",
        )]
    })?;
    if program.function_by_id(function_id).is_none() {
        return Err(vec![RunnerError::internal(
            Span::default(),
            format!("no MIR function with id {}", function_id.0),
        )]);
    }
    if let Some((function, reason)) = program.functions.iter().find_map(|function| {
        function_unsupported_bounded_carrier(function, context.types)
            .map(|reason| (function, reason))
    }) {
        return Err(vec![RunnerError::unsupported(function.span, reason)]);
    }
    let mut runner = Runner::new(program, context, context.types, interner, host);
    runner.step_limit = step_budget;
    runner.push_frame(function_id, MirBlockId(0));
    runner.run_until_depth(0);
    if runner.errors.is_empty() {
        Ok(())
    } else {
        Err(runner.errors)
    }
}

fn function_unsupported_bounded_carrier(
    function: &MirFunction,
    types: &TypeTable,
) -> Option<&'static str> {
    let label = |ty: radix_mir::MirType| match types.get(ty.semantic_id()) {
        Type::BoundedTextus { .. } => Some("bounded textus runner carrier"),
        Type::BoundedOcteti { .. } => Some("bounded octeti runner carrier"),
        _ => None,
    };
    function
        .params
        .iter()
        .find_map(|param| label(param.ty))
        .or_else(|| label(function.return_ty))
        .or_else(|| function.error_ty.and_then(label))
        .or_else(|| function.locals.iter().find_map(|local| label(local.ty)))
        .or_else(|| function.temps.iter().find_map(|temp| label(temp.ty)))
        .or_else(|| {
            function.blocks.iter().find_map(|block| {
                block
                    .statements
                    .iter()
                    .find_map(|statement| match &statement.kind {
                        MirStatementKind::Assign { value, .. } => label(value.ty),
                        MirStatementKind::RuntimeCall { call, .. } => label(call.return_ty),
                        MirStatementKind::Call { .. } | MirStatementKind::Construct { .. } => None,
                    })
            })
        })
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
fn f16_round_f64(v: f64) -> f64 {
    f64::from(f16_round_f32(v as f32))
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
        return if mantissa == 0 {
            f32::INFINITY
        } else {
            f32::NAN
        };
    }
    let value = (1.0f32 + mantissa as f32 / 1024.0f32) * 2f32.powi(exponent as i32 - 15);
    if sign == 1 {
        -value
    } else {
        value
    }
}

impl<H: Host + ?Sized> Runner<'_, H> {
    fn push_frame(&mut self, function_id: MirFunctionId, entry_block: MirBlockId) {
        let locals = match self.program.function_by_id(function_id) {
            Some(function) => {
                let mut locals = FxHashMap::default();
                for param in &function.params {
                    locals.insert(param.local, Value::Unit);
                }
                locals
            }
            None => FxHashMap::default(),
        };
        self.frames.push(Frame {
            function: function_id,
            locals,
            aliases: FxHashMap::default(),
            temps: FxHashMap::default(),
            values: FxHashMap::default(),
            block: entry_block,
            is_generator: false,
        });
    }

    fn function_by_id(&self, id: MirFunctionId, span: Span) -> Result<&MirFunction, RunnerError> {
        // Prefer dense O(1) via MirProgram::function_by_id (BO-M-3).
        self.program
            .function_by_id(id)
            .ok_or_else(|| RunnerError::internal(span, format!("missing function f{}", id.0)))
    }

    fn set_active_block(&mut self, block: MirBlockId, span: Span) {
        match self.frames.last_mut() {
            Some(frame) => frame.block = block,
            None => self
                .errors
                .push(RunnerError::internal(span, "no active frame")),
        }
    }

    /// Count one interpreter step (one MIR basic-block visit). Returns `false`
    /// when the optional step budget is exhausted; an error is recorded.
    fn consume_step(&mut self, span: Span) -> bool {
        let Some(limit) = self.step_limit else {
            return true;
        };
        if self.steps_taken >= limit {
            self.errors.push(RunnerError::internal(
                span,
                format!("interpreter step budget exhausted after {limit} steps"),
            ));
            return false;
        }
        self.steps_taken += 1;
        true
    }

    fn run_until_depth(&mut self, min_depth: usize) {
        while self.frames.len() > min_depth {
            if !self.consume_step(Span::default()) {
                break;
            }
            let Some(frame) = self.frames.last() else {
                break;
            };
            let (function_id, block_id) = (frame.function, frame.block);
            let block_data = match self.block_snapshot(function_id, block_id) {
                Ok(data) => data,
                Err(error) => {
                    self.errors.push(error);
                    self.frames.pop();
                    continue;
                }
            };

            let mut frame_returned = false;
            for statement in &block_data.statements {
                match self.execute_statement(statement) {
                    Ok(StatementOutcome::Continue) => {}
                    Ok(StatementOutcome::FrameReturned) => {
                        frame_returned = true;
                        break;
                    }
                    Err(error) => {
                        self.errors.push(error);
                        self.frames.pop();
                        break;
                    }
                }
            }
            if !self.errors.is_empty() {
                break;
            }
            if frame_returned {
                continue;
            }

            let terminator = block_data.terminator.clone();
            match terminator.kind {
                MirTerminatorKind::Return(operand) => {
                    let value = match operand {
                        Some(operand) => match self.eval_operand(&operand, terminator.span) {
                            Ok(value) => value,
                            Err(error) => {
                                self.errors.push(error);
                                self.frames.pop();
                                continue;
                            }
                        },
                        None => Value::Unit,
                    };
                    self.return_error = None;
                    self.return_value = Some(value);
                    self.frames.pop();
                }
                MirTerminatorKind::ReturnError(operand) => {
                    let value = match self.eval_operand(&operand, terminator.span) {
                        Ok(value) => value,
                        Err(error) => {
                            self.errors.push(error);
                            self.frames.pop();
                            continue;
                        }
                    };
                    self.return_value = None;
                    self.return_error = Some(value);
                    self.frames.pop();
                }
                MirTerminatorKind::TryCall {
                    destination,
                    callee,
                    args,
                    ok_block,
                    error_place,
                    error_block,
                } => match self.invoke_call(&callee, &args, terminator.span) {
                    Ok(CallOutcome::Ok(value)) => {
                        if let Some(destination) = destination {
                            if let Err(error) =
                                self.write_place(&destination, value, terminator.span)
                            {
                                self.errors.push(error);
                                self.frames.pop();
                                continue;
                            }
                        }
                        self.set_active_block(ok_block, terminator.span);
                    }
                    Ok(CallOutcome::Err(error_value)) => {
                        if let Err(error) =
                            self.write_place(&error_place, error_value, terminator.span)
                        {
                            self.errors.push(error);
                            self.frames.pop();
                            continue;
                        }
                        self.set_active_block(error_block, terminator.span);
                    }
                    Err(error) => {
                        self.errors.push(error);
                        self.frames.pop();
                    }
                },
                MirTerminatorKind::Goto(target) => {
                    self.set_active_block(target, terminator.span);
                }
                MirTerminatorKind::Branch {
                    condition,
                    then_block,
                    else_block,
                } => match self.eval_operand(&condition, terminator.span) {
                    Ok(cond) => match cond.as_bool(terminator.span) {
                        Ok(taken) => {
                            self.set_active_block(
                                if taken { then_block } else { else_block },
                                terminator.span,
                            );
                        }
                        Err(error) => {
                            self.errors.push(error);
                            self.frames.pop();
                        }
                    },
                    Err(error) => {
                        self.errors.push(error);
                        self.frames.pop();
                    }
                },
                MirTerminatorKind::Switch {
                    value,
                    cases,
                    default,
                } => match self.eval_operand(&value, terminator.span) {
                    Ok(switch_value) => {
                        let target = cases
                            .iter()
                            .find(|case| {
                                constant_equals_value(&case.value, &switch_value, self.interner)
                            })
                            .map_or(default, |case| case.target);
                        self.set_active_block(target, terminator.span);
                    }
                    Err(error) => {
                        self.errors.push(error);
                        self.frames.pop();
                    }
                },
                MirTerminatorKind::Unreachable => {
                    self.errors.push(RunnerError::unsupported(
                        terminator.span,
                        &format!("terminator {:?}", terminator.kind),
                    ));
                    self.frames.pop();
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
        self.block_cache
            .get(&(function_id, block_id))
            .cloned()
            .ok_or_else(|| {
                RunnerError::internal(Span::default(), format!("missing block b{}", block_id.0))
            })
    }

    fn execute_statement(
        &mut self,
        statement: &radix_mir::MirStatement,
    ) -> Result<StatementOutcome, RunnerError> {
        match &statement.kind {
            MirStatementKind::Assign { place, value } => {
                let evaluated = self.eval_value(value, statement.span)?;
                self.write_place(place, evaluated, statement.span)?;
                Ok(StatementOutcome::Continue)
            }
            MirStatementKind::RuntimeCall { destination, call } => {
                let result = match &call.intrinsic {
                    radix_mir::MirIntrinsic::Convert(conversion) => {
                        match self.eval_convert(conversion, &call.args, statement.span) {
                            Ok(value) => value,
                            Err(error) if error.is_conversion_failure() => {
                                return self.propagate_conversion_failure(error);
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    _ => self.eval_runtime_call(call, statement.span)?,
                };
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

    fn propagate_conversion_failure(
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
        if !matches!(
            self.types.get(error_ty.semantic_id()),
            Type::Primitive(Primitive::Textus | Primitive::Ignotum)
        ) {
            return Err(RunnerError::internal(
                error.span,
                "recoverable runtime conversio requires textus-compatible alternate-exit type",
            ));
        }

        self.return_value = None;
        self.return_error = Some(Value::Str(error.message));
        self.frames.pop();
        Ok(StatementOutcome::FrameReturned)
    }

    fn invoke_call(
        &mut self,
        callee: &MirCallee,
        args: &[MirOperand],
        span: Span,
    ) -> Result<CallOutcome, RunnerError> {
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
        // BO-M-3/052: avoid cloning the entire MirFunction; collect param locals only.
        let param_modes = self
            .function_by_id(func_id, span)?
            .params
            .iter()
            .map(|param| (param.local, param.mode))
            .collect::<Vec<_>>();
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

        let depth = self.frames.len();
        self.return_value = None;
        self.return_error = None;
        self.push_frame(func_id, MirBlockId(0));
        if let Some(frame) = self.frames.last_mut() {
            if let Some((local, value)) = closure_environment {
                frame.locals.insert(local, value);
            }
            for ((local, mode), (arg, operand)) in param_modes
                .into_iter()
                .zip(arg_values.into_iter().zip(args))
            {
                // `in` is a borrow contract, not a scalar copy. Keep a
                // direct place alias while the caller frame is live; all
                // other modes retain the existing value transfer behavior.
                if mode == MirParamMode::MutRef
                    && depth > 0
                    && matches!(operand, MirOperand::Place(place) if place.projections.is_empty())
                {
                    let MirOperand::Place(place) = operand else {
                        unreachable!("place checked above");
                    };
                    frame.aliases.insert(
                        local,
                        PlaceAlias {
                            frame: depth - 1,
                            place: place.clone(),
                        },
                    );
                } else {
                    frame.locals.insert(local, arg);
                }
            }
        }
        self.run_until_depth(depth);
        if let Some(error) = self.errors.pop() {
            while self.frames.len() > depth {
                self.frames.pop();
            }
            return Err(error);
        }
        while self.frames.len() > depth {
            self.frames.pop();
        }
        if let Some(error_value) = self.return_error.take() {
            return Ok(CallOutcome::Err(error_value));
        }
        Ok(CallOutcome::Ok(
            self.return_value.take().unwrap_or(Value::Unit),
        ))
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
        self.def_to_function.get(&def_id).copied().ok_or_else(|| {
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
        };
        if let Some(frame) = self.frames.last_mut() {
            frame.values.insert(value.id, computed.clone());
        }
        Ok(computed)
    }

    fn eval_operand(&mut self, operand: &MirOperand, span: Span) -> Result<Value, RunnerError> {
        match operand {
            MirOperand::Constant(constant) => self.eval_constant(constant, span),
            MirOperand::Place(place) => self.read_place(place, span),
            MirOperand::Temp(temp) => self.read_temp(*temp, span),
            MirOperand::Value(value_id) => self.read_value(*value_id, span),
        }
    }

    /// CTO-3 — the fallback runs ONLY on conversion failure. A side-effecting
    /// `⇥` recovery CALL is lowered by the materializer into an ORPHAN block
    /// (dead code to the main flow); at the conversion-failure recovery point
    /// this invokes it LAZILY, so its side effects run only here — never on a
    /// successful conversion (fallback-not-run-on-success). Pure recovery
    /// operands evaluate normally.
    fn eval_recovery(&mut self, recovery: &MirOperand, span: Span) -> Result<Value, RunnerError> {
        let MirOperand::Temp(temp_id) = recovery else {
            return self.eval_operand(recovery, span);
        };
        let function_id = self
            .frames
            .last()
            .map(|frame| frame.function)
            .ok_or_else(|| RunnerError::internal(span, "recovery evaluation outside a frame"))?;
        let deferred = self.recovery_calls.get(&(function_id, *temp_id)).cloned();
        if let Some((callee, args)) = deferred {
            return match self.invoke_call(&callee, &args, span)? {
                CallOutcome::Ok(value) => Ok(value),
                CallOutcome::Err(_) => Err(RunnerError::internal(
                    span,
                    "deferred recovery call returned an error",
                )),
            };
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
            // A symbolic shape-size scalar has no concrete value until
            // Phase-4 substitution; evaluating it would be a silent guess.
            MirConstant::ShapeSize { .. } => {
                return Err(RunnerError::internal(
                    span,
                    "symbolic shape-size constant is not concretely evaluable (Phase-4 substitution)",
                ));
            }
            MirConstant::Regex { pattern, flags } => {
                // WHY: the pattern symbol is the raw string-literal source; a
                // `textus ↦ regex` conversion must see the decoded textus value
                // (mirrors `MirConstant::String`), so `\\d+` is `\d+`.
                let pattern = regex_pattern(
                    &crate::textus_literal::decode_textus_literal(self.interner.resolve(*pattern)),
                    flags.map(|flags| self.interner.resolve(flags)),
                );
                Value::Regex(runtime_regex_pattern(&pattern))
            }
        })
    }

    fn interned_textus(&self, symbol: Symbol) -> String {
        let mut cache = self.textus_literals.borrow_mut();
        if let Some(cached) = cache.get(&symbol) {
            return cached.clone();
        }
        let decoded = crate::textus_literal::decode_textus_literal(self.interner.resolve(symbol));
        cache.insert(symbol, decoded.clone());
        decoded
    }

    fn interned_ascii(&self, symbol: Symbol) -> String {
        let mut cache = self.ascii_literals.borrow_mut();
        if let Some(cached) = cache.get(&symbol) {
            return cached.clone();
        }
        let owned = self.interner.resolve(symbol).to_owned();
        cache.insert(symbol, owned.clone());
        owned
    }

    fn read_place(&mut self, place: &MirPlace, span: Span) -> Result<Value, RunnerError> {
        let mut value = match place.base {
            MirPlaceBase::Local(id) => self.read_local(id, span)?,
            MirPlaceBase::Temp(id) => self.read_temp(id, span)?,
        };
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
        if place.projections.is_empty() {
            let frame = self
                .frames
                .len()
                .checked_sub(1)
                .ok_or_else(|| RunnerError::internal(span, "no active frame"))?;
            return self.write_base_place(frame, &place.base, value, span);
        }

        let parent = MirPlace {
            base: place.base.clone(),
            projections: place.projections[..place.projections.len() - 1].to_vec(),
        };
        let Some(last) = place.projections.last() else {
            return Err(RunnerError::internal(span, "empty projection path"));
        };
        let mut current = self.read_place(&parent, span)?;
        current = self.project_write(current, last, value, span)?;
        self.write_place(&parent, current, span)
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
                return self.write_base_place(alias.frame, &alias.place.base, value, span);
            }
        }
        let frame = self
            .frames
            .get_mut(frame_index)
            .ok_or_else(|| RunnerError::internal(span, "no active frame"))?;
        match base {
            MirPlaceBase::Local(id) => {
                frame.locals.insert(*id, value);
            }
            MirPlaceBase::Temp(id) => {
                frame.temps.insert(*id, value);
            }
        }
        Ok(())
    }

    fn project_write(
        &mut self,
        value: Value,
        projection: &radix_mir::MirProjection,
        assigned: Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match projection {
            radix_mir::MirProjection::Index(index) => {
                let index = self.eval_operand(index, span)?;
                match value {
                    Value::Array(cell) => {
                        let idx = index.as_index(span)?;
                        {
                            let mut borrow = cell.borrow_mut();
                            let slot = borrow.get_mut(idx).ok_or_else(|| {
                                RunnerError::internal(span, "array index out of bounds")
                            })?;
                            *slot = assigned;
                        }
                        Ok(Value::Array(cell))
                    }
                    Value::Map(cell) => {
                        let key = index.to_key(span)?;
                        cell.borrow_mut().insert(key, assigned);
                        Ok(Value::Map(cell))
                    }
                    _ => Err(RunnerError::internal(
                        span,
                        "index write on non-array or non-map",
                    )),
                }
            }
            radix_mir::MirProjection::Field(field) => match value {
                Value::Record(cell) => {
                    cell.borrow_mut().insert(*field, assigned);
                    Ok(Value::Record(cell))
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
            radix_mir::MirProjection::ClosureCapture { .. } => Err(RunnerError::unsupported(
                span,
                "closure capture projection write",
            )),
        }
    }

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
            return self.read_base_place(alias.frame, &alias.place.base, span);
        }
        self.frames
            .get(frame_index)
            .and_then(|frame| frame.locals.get(&id))
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
                .and_then(|frame| frame.temps.get(id))
                .cloned()
                .ok_or_else(|| {
                    RunnerError::internal(span, format!("uninitialized temp t{}", id.0))
                }),
        }
    }

    fn read_temp(&self, id: MirTempId, span: Span) -> Result<Value, RunnerError> {
        self.frames
            .last()
            .and_then(|frame| frame.temps.get(&id))
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

    fn modular_word_width(&self, ty: radix_mir::MirType) -> Option<NumericWidth> {
        match self.types.get(ty.semantic_id()) {
            Type::ModularWord(width) => Some(*width),
            _ => None,
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
        let value = self.eval_operand(operand, span)?;
        let width = self.resolve_int_width(result_ty);
        let modular = self.modular_word_width(result_ty);
        match op {
            MirUnOp::Neg => {
                if let Some(width) = modular {
                    modular_word_neg(value, width, span)
                } else {
                    match value {
                        Value::Int(v) => width_checked_neg(v, width, span).map(Value::Int),
                        Value::UInt(v) => uint_width_checked_neg(v, width, span).map(Value::UInt),
                        Value::Float(v) => Ok(Value::Float(-v)),
                        _ => Err(RunnerError::internal(span, "negation type mismatch")),
                    }
                }
            }
            MirUnOp::Not => value.as_bool(span).map(|value| Value::Bool(!value)),
            MirUnOp::BitNot => {
                if let Some(width) = modular {
                    modular_word_bitnot(value, width, span)
                } else {
                    match value {
                        Value::Int(v) => Ok(Value::Int(!v)),
                        Value::UInt(v) => Ok(Value::UInt(!v)),
                        _ => Err(RunnerError::internal(
                            span,
                            "bitnot requires numerus operand",
                        )),
                    }
                }
            }
            MirUnOp::IsNil => Ok(Value::Bool(matches!(value, Value::Nil))),
            MirUnOp::IsNotNil => Ok(Value::Bool(!matches!(value, Value::Nil))),
            MirUnOp::Sqrt => match value {
                Value::Float(v) if v >= 0.0 => Ok(Value::Float(v.sqrt())),
                Value::Float(_) => Err(RunnerError::internal(span, "sqrt of negative float")),
                Value::Int(v) if v >= 0 => Ok(Value::Float((v as f64).sqrt())),
                Value::UInt(v) => Ok(Value::Float((v as f64).sqrt())),
                Value::Tensor(cell) => {
                    let borrow = cell.borrow();
                    let data = borrow
                        .data
                        .iter()
                        .map(|v| match v {
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
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(Value::Tensor(Rc::new(RefCell::new(TensorValue {
                        data,
                        shape: borrow.shape.clone(),
                    }))))
                }
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
                Value::Tensor(cell) => {
                    let borrow = cell.borrow();
                    let data = borrow
                        .data
                        .iter()
                        .map(|v| match v {
                            Value::Float(f) => Ok(Value::Float(gelu_f64(*f))),
                            _ => Err(RunnerError::internal(
                                span,
                                "tensor gelu element type mismatch",
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(Value::Tensor(Rc::new(RefCell::new(TensorValue {
                        data,
                        shape: borrow.shape.clone(),
                    }))))
                }
                _ => Err(RunnerError::internal(span, "gelu requires float operand")),
            },
            MirUnOp::Tanh => match value {
                Value::Float(v) => Ok(Value::Float(v.tanh())),
                Value::Tensor(cell) => {
                    let borrow = cell.borrow();
                    let data = borrow
                        .data
                        .iter()
                        .map(|v| match v {
                            Value::Float(f) => Ok(Value::Float(f.tanh())),
                            _ => Err(RunnerError::internal(
                                span,
                                "tensor tanh element type mismatch",
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(Value::Tensor(Rc::new(RefCell::new(TensorValue {
                        data,
                        shape: borrow.shape.clone(),
                    }))))
                }
                _ => Err(RunnerError::internal(span, "tanh requires float operand")),
            },
            MirUnOp::Exp => {
                let result = match value {
                    Value::Float(v) => v.exp(),
                    Value::Int(v) => (v as f64).exp(),
                    Value::UInt(v) => (v as f64).exp(),
                    Value::Tensor(cell) => {
                        let borrow = cell.borrow();
                        let data = borrow
                            .data
                            .iter()
                            .map(|v| match v {
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
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        return Ok(Value::Tensor(Rc::new(RefCell::new(TensorValue {
                            data,
                            shape: borrow.shape.clone(),
                        }))));
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
                let is_valid = match value {
                    Value::Float(v) => v > 0.0,
                    Value::Int(v) => v > 0,
                    Value::UInt(v) => v > 0,
                    _ => return Err(RunnerError::internal(span, "log requires numeric operand")),
                };
                if !is_valid {
                    return Err(RunnerError::internal(span, "log requires positive input"));
                }
                let result = match value {
                    Value::Float(v) => v.ln(),
                    Value::Int(v) => (v as f64).ln(),
                    Value::UInt(v) => (v as f64).ln(),
                    _ => unreachable!(),
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
                    Value::Tensor(cell) => {
                        let borrow = cell.borrow();
                        let data = borrow
                            .data
                            .iter()
                            .map(|element| match element {
                                Value::Float(v) => Ok(Value::Float(f16_round_f64(*v))),
                                _ => Err(RunnerError::internal(
                                    span,
                                    "tensor f16-round element type mismatch",
                                )),
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok(Value::Tensor(Rc::new(RefCell::new(TensorValue {
                            data,
                            shape: borrow.shape.clone(),
                        }))))
                    }
                    _ => Err(RunnerError::internal(
                        span,
                        "f16-round requires numeric operand",
                    )),
                }
            }
        }
    }

    fn eval_binop(
        &mut self,
        op: MirBinOp,
        lhs: &MirOperand,
        rhs: &MirOperand,
        result_ty: radix_mir::MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let left = self.eval_operand(lhs, span)?;
        let right = self.eval_operand(rhs, span)?;
        let width = self.resolve_int_width(result_ty);
        if let Some(modular) = self.modular_word_width(result_ty) {
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
        match op {
            MirBinOp::Add => add_values(left, right, width, span),
            MirBinOp::Sub => numeric_binop(
                left,
                right,
                width_checked_sub,
                uint_width_checked_sub,
                |a, b| a - b,
                width,
                span,
            ),
            MirBinOp::Mul => numeric_binop(
                left,
                right,
                width_checked_mul,
                uint_width_checked_mul,
                |a, b| a * b,
                width,
                span,
            ),
            MirBinOp::Div => numeric_binop(
                left,
                right,
                width_checked_div,
                uint_width_checked_div,
                |a, b| a / b,
                width,
                span,
            ),
            MirBinOp::Mod => numeric_binop(
                left,
                right,
                width_checked_rem,
                uint_width_checked_rem,
                |a, b| a % b,
                width,
                span,
            ),
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
            | MirBinOp::Shr => int_bitwise_binop(left, right, op, span),
        }
    }
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
        (MirConstant::String(symbol), Value::Str(right)) => {
            crate::textus_literal::decode_textus_literal(interner.resolve(*symbol)) == *right
        }
        (MirConstant::Ascii(symbol), Value::Str(right)) => interner.resolve(*symbol) == right,
        (MirConstant::Ascii(symbol), Value::Ascii(right)) => {
            interner.resolve(*symbol).as_bytes() == [*right]
        }
        (MirConstant::Regex { pattern, flags }, Value::Regex(right)) => {
            regex_pattern(
                &crate::textus_literal::decode_textus_literal(interner.resolve(*pattern)),
                flags.map(|flags| interner.resolve(flags)),
            ) == *right
        }
        (MirConstant::Nil, Value::Nil) => true,
        (MirConstant::Unit, Value::Unit) => true,
        _ => false,
    }
}

fn index_orphan_recovery_calls(
    program: &MirProgram,
) -> FxHashMap<(MirFunctionId, MirTempId), (MirCallee, Vec<MirOperand>)> {
    let mut map = FxHashMap::default();
    for function in &program.functions {
        let reachable = reachable_block_ids(function);
        for block in &function.blocks {
            if reachable.contains(&block.id) {
                continue;
            }
            for statement in &block.statements {
                let MirStatementKind::Call {
                    destination: Some(place),
                    callee,
                    args,
                } = &statement.kind
                else {
                    continue;
                };
                let MirPlaceBase::Temp(temp_id) = place.base else {
                    continue;
                };
                map.entry((function.id, temp_id))
                    .or_insert_with(|| (callee.clone(), args.clone()));
            }
        }
    }
    map
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

pub(crate) fn runtime_regex_pattern(pattern: &str) -> String {
    // The runtime's `faber::Regex` is a compile-time literal carrier today:
    // `Regex::new(pattern).pattern()` returns the input unchanged. Mirror that
    // identity here without the runtime dependency.
    pattern.to_owned()
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod lib_test;

#[cfg(test)]
#[path = "octeti_index_test.rs"]
mod octeti_index_test;

#[cfg(test)]
#[path = "octeti_endian_test.rs"]
mod octeti_endian_test;

#[cfg(test)]
#[path = "ascii_format_pack_test.rs"]
mod ascii_format_pack_test;

#[cfg(test)]
#[path = "octeti_lista_test.rs"]
mod octeti_lista_test;

#[cfg(test)]
#[path = "ascii1_index_test.rs"]
mod ascii1_index_test;

#[cfg(test)]
#[path = "ascii_cursor_test.rs"]
mod ascii_cursor_test;

#[cfg(test)]
#[path = "capability_test.rs"]
mod capability_test;

#[cfg(test)]
#[path = "kernel_test.rs"]
mod kernel_test;

#[cfg(test)]
#[path = "value_test.rs"]
mod value_test;
