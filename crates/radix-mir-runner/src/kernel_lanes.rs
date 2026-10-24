//! Per-lane execution of a kernel's device-form MIR function (gpu-reset F1-1).
//!
//! The device lowering turns a kernel body into a scalar-per-lane function:
//! tensor parameters are indexed through one linear index (`_1[%2] = _0[%5]`)
//! and the lane index arrives as a builtin-input parameter
//! ([`MirKernelBuiltinInput`]). These entries run that function on the
//! ordinary runner, once per lane: the builtin-input locals are seeded from the
//! lane, every other parameter from `args` in declaration order. Tensor
//! arguments are the runner's own shared tensors, so a `mut` tensor parameter
//! is written in place and no write-back step exists.
//!
//! [`run_device_form_lanes`] runs a whole sequence of lanes on one runner, so
//! the runner's whole-program indexes are built once per call, not once per
//! lane. [`run_device_form_lane`] is the one-lane case.
//! [`run_device_form_lanes_prepared`] takes those indexes ready-made as a
//! [`RunnerPrepared`], for callers that run one program many times.
//! [`run_device_form_call_prepared`] is the whole-call form of a kernel with
//! no lane builtin: one run that returns the kernel's returned value.
//!
//! Only the global-invocation builtins are fed. A workgroup, local-id or
//! workgroup-size builtin is refused by name; workgroup memory and barriers
//! have no runner form here (see the F1-1 spike notes).

use crate::host::Host;
use crate::{Runner, RunnerError, RunnerKernelPolicy, RunnerPrepared, Value};
use radix_lexer::Span;
use radix_mir::{MirBlockId, MirLocalId, MirParamMode, MirPlaceBase};
use radix_mir::{MirFunctionId, MirKernelBuiltin, MirKernelBuiltinInput, ValidatedMir};

/// The lane a [`run_device_form_lanes`] call stopped at, and why.
///
/// `lane` is `None` when the call failed before any lane ran (an unknown
/// function, an argument count mismatch, or a builtin that is not fed).
/// `raised` is the value a lane threw when it left through the error channel
/// (its `errors` then hold one message naming the value); a caller that routes
/// kernel errors, such as a device fault channel, reads it from here.
#[derive(Debug)]
pub struct LaneRunFailure {
    pub lane: Option<[u32; 3]>,
    pub errors: Vec<RunnerError>,
    pub raised: Option<Value>,
}

/// Run `function_id`'s device form once for `lane` (`[x, y, z]` global
/// invocation id).
///
/// `builtin_inputs` names the parameters fed from `lane`
/// ([`MirKernelBuiltin::GlobalInvocationX`] / `Y` / `Z`). `args` supplies the
/// remaining parameters in declaration order.
///
/// # Errors
///
/// Returns runner errors when the function is unknown, the argument count does
/// not match the non-builtin parameters, a builtin other than the
/// global-invocation axes is named, or the body fails.
pub fn run_device_form_lane<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    args: &[Value],
    builtin_inputs: &[MirKernelBuiltinInput],
    lane: [u32; 3],
) -> Result<(), Vec<RunnerError>> {
    run_device_form_lanes(
        validated,
        function_id,
        host,
        args,
        builtin_inputs,
        std::iter::once(lane),
    )
    .map_err(|failure| failure.errors)
}

/// Run `function_id`'s device form once for every lane in `lanes`, in the
/// order given, on one runner.
///
/// Arguments and builtins are as for [`run_device_form_lane`]. Each lane starts
/// from a fresh frame, so no local survives from one lane to the next; tensor
/// arguments are shared, so a later lane observes an earlier lane's writes, as
/// the sequential meaning does.
///
/// # Errors
///
/// Stops at the first lane whose body fails (a runner error, or an exit
/// through the error channel) and returns that lane with its runner errors. A setup failure (unknown function, argument count mismatch,
/// a builtin that is not fed) returns before any lane runs, with no lane.
pub fn run_device_form_lanes<H: Host + ?Sized>(
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    args: &[Value],
    builtin_inputs: &[MirKernelBuiltinInput],
    lanes: impl IntoIterator<Item = [u32; 3]>,
) -> Result<(), LaneRunFailure> {
    let prepared = RunnerPrepared::new(
        validated.program(),
        validated.validation(),
        validated.validation().types.as_table(),
    );
    run_device_form_lanes_prepared(
        &prepared,
        validated,
        function_id,
        host,
        args,
        builtin_inputs,
        lanes,
    )
}

/// [`run_device_form_lanes`] over whole-program indexes built earlier.
///
/// `prepared` must have been built from `validated`'s own program, validation
/// context and type table ([`RunnerPrepared::new`]); the same value serves any
/// number of calls on that program.
///
/// # Errors
///
/// As for [`run_device_form_lanes`].
pub fn run_device_form_lanes_prepared<H: Host + ?Sized>(
    prepared: &RunnerPrepared,
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    args: &[Value],
    builtin_inputs: &[MirKernelBuiltinInput],
    lanes: impl IntoIterator<Item = [u32; 3]>,
) -> Result<(), LaneRunFailure> {
    // Tensor arguments are shared, so a lane's element writes reach the
    // caller's tensors; a rebinding of a `mut` parameter stays in this copy.
    let mut args = args.to_vec();
    run_lanes(
        prepared,
        validated,
        function_id,
        host,
        &mut args,
        builtin_inputs,
        lanes,
        |_| {},
    )
}

/// Run `function_id`'s device form once, as lane `[0, 0, 0]`, and return
/// the value it returns (`None` for a function that returns nothing).
///
/// This is the whole-call form of a kernel with no lane builtin: the body is
/// sequential, so one call is the whole launch, and a returned result (a
/// fresh tensor, a scalar or a tuple of them) is the call's output.
/// `prepared` is as for [`run_device_form_lanes_prepared`].
///
/// A `mut` parameter is a place of the caller, as in an ordinary call: a body
/// that rebinds it (`o ⇇ a · b` lowers to `o = matmul(a, b)`) leaves the new
/// value in its `args` slot, also when the body throws.
///
/// # Errors
///
/// As for [`run_device_form_lanes`].
pub fn run_device_form_call_prepared<H: Host + ?Sized>(
    prepared: &RunnerPrepared,
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    args: &mut [Value],
    builtin_inputs: &[MirKernelBuiltinInput],
) -> Result<Option<Value>, LaneRunFailure> {
    let mut returned = None;
    run_lanes(
        prepared,
        validated,
        function_id,
        host,
        args,
        builtin_inputs,
        std::iter::once([0, 0, 0]),
        |value| returned = value,
    )?;
    Ok(returned)
}

/// The shared lane loop: `on_return` receives each lane's returned value.
///
/// Each lane runs as a call from a host frame: frame 0 is a frame of the same
/// function holding the `mut` arguments, and every `mut` parameter of the lane
/// frame aliases its cell there, as an ordinary call aliases the caller's
/// place. After the lane, the host cells hold the final `mut` values; they are
/// copied back into `args`, so a later lane and the caller see them.
#[allow(clippy::too_many_arguments)]
fn run_lanes<H: Host + ?Sized>(
    prepared: &RunnerPrepared,
    validated: &ValidatedMir<'_>,
    function_id: MirFunctionId,
    host: &mut H,
    args: &mut [Value],
    builtin_inputs: &[MirKernelBuiltinInput],
    lanes: impl IntoIterator<Item = [u32; 3]>,
    mut on_return: impl FnMut(Option<Value>),
) -> Result<(), LaneRunFailure> {
    let context = validated.validation();
    let program = validated.program();
    let fail = |message: String| LaneRunFailure {
        lane: None,
        errors: vec![RunnerError::internal(Span::default(), message)],
        raised: None,
    };
    let interner = context
        .interner
        .as_interner()
        .ok_or_else(|| fail("runner requires interner context".to_owned()))?;
    let function = validated
        .function_by_id(function_id)
        .ok_or_else(|| fail(format!("no MIR function with id {}", function_id.0)))?;

    // The argument seeds name each parameter's slot, its argument and whether
    // it is a `mut` place; the lane seeds name the slot and the axis each lane
    // fills.
    let mut arg_seeds: Vec<(usize, usize, bool)> = Vec::with_capacity(function.params.len());
    let mut lane_seeds: Vec<(usize, usize)> = Vec::with_capacity(builtin_inputs.len());
    let mut remaining = 0..args.len();
    for param in &function.params {
        let slot = param.local.0 as usize;
        if let Some(input) = builtin_inputs
            .iter()
            .find(|input| input.local == param.local)
        {
            let axis = match input.builtin {
                MirKernelBuiltin::GlobalInvocationX => 0,
                MirKernelBuiltin::GlobalInvocationY => 1,
                MirKernelBuiltin::GlobalInvocationZ => 2,
                other => {
                    return Err(fail(format!(
                        "device-form lane run does not feed builtin {other:?}"
                    )));
                }
            };
            lane_seeds.push((slot, axis));
        } else {
            let index = remaining
                .next()
                .ok_or_else(|| fail("too few arguments for the kernel's parameters".to_owned()))?;
            arg_seeds.push((slot, index, param.mode == MirParamMode::MutRef));
        }
    }
    if remaining.next().is_some() {
        return Err(fail(
            "too many arguments for the kernel's parameters".to_owned(),
        ));
    }

    let mut runner = Runner::with_prepared(
        prepared,
        program,
        context,
        context.types.as_table(),
        interner,
        host,
    );
    runner.kernel_policy = RunnerKernelPolicy::Execute;
    for lane in lanes {
        let mut caller = runner.new_frame(function_id, MirBlockId(0));
        let mut frame = runner.new_frame(function_id, MirBlockId(0));
        for &(slot, index, place) in &arg_seeds {
            let value = args[index].clone();
            let cell = if place {
                let local = MirLocalId(u32::try_from(slot).unwrap_or(u32::MAX));
                frame.aliases.insert(
                    local,
                    crate::PlaceAlias {
                        frame: 0,
                        base: MirPlaceBase::Local(local),
                    },
                );
                caller.locals.get_mut(slot)
            } else {
                frame.locals.get_mut(slot)
            };
            if let Some(cell) = cell {
                *cell = Some(value);
            }
        }
        for &(slot, axis) in &lane_seeds {
            if let Some(cell) = frame.locals.get_mut(slot) {
                *cell = Some(Value::Int(i64::from(lane[axis])));
            }
        }
        runner.resume_frame(caller);
        runner.resume_frame(frame);
        runner.run_until_depth(1);
        if let Some(caller) = runner.frames.first_mut() {
            for &(slot, index, place) in &arg_seeds {
                if place && let Some(Some(value)) = caller.locals.get_mut(slot).map(Option::take) {
                    args[index] = value;
                }
            }
        }
        runner.close_all_frames();
        let returned = runner.return_value.take();
        let raised = runner.return_error.take();
        runner.yielded = None;
        if !runner.errors.is_empty() {
            return Err(LaneRunFailure {
                lane: Some(lane),
                errors: std::mem::take(&mut runner.errors),
                raised: None,
            });
        }
        // A lane that exits through the error channel failed: dropping the
        // raised value would let a partial write pass as a result. The lanes
        // before it, and this lane's writes before the throw, stay written,
        // as the sequential meaning leaves them.
        if let Some(raised) = raised {
            return Err(LaneRunFailure {
                lane: Some(lane),
                errors: vec![RunnerError::internal(
                    Span::default(),
                    format!("the kernel raised an error value: {raised:?}"),
                )],
                raised: Some(raised),
            });
        }
        on_return(returned);
    }
    Ok(())
}
