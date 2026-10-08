//! Per-lane execution of a kernel's device-form MIR function (gpu-reset F1-1).
//!
//! The device lowering turns a kernel body into a scalar-per-lane function:
//! tensor parameters are indexed through one linear index (`_1[%2] = _0[%5]`)
//! and the lane index arrives as a builtin-input parameter
//! ([`MirKernelBuiltinInput`]). This entry runs that function once, for one
//! lane, on the ordinary runner: the builtin-input locals are seeded from the
//! lane, every other parameter from `args` in declaration order. Tensor
//! arguments are the runner's own shared tensors, so a `mut` tensor parameter
//! is written in place and no write-back step exists.
//!
//! Only the global-invocation builtins are fed. A workgroup, local-id or
//! workgroup-size builtin is refused by name; workgroup memory and barriers
//! have no runner form here (see the F1-1 spike notes).

use crate::host::Host;
use crate::{Runner, RunnerError, RunnerKernelPolicy, Value};
use radix_lexer::Span;
use radix_mir::MirBlockId;
use radix_mir::{MirFunctionId, MirKernelBuiltin, MirKernelBuiltinInput, ValidatedMir};

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
    let context = validated.validation();
    let program = validated.program();
    let fail = |message: String| vec![RunnerError::internal(Span::default(), message)];
    let interner = context
        .interner
        .as_interner()
        .ok_or_else(|| fail("runner requires interner context".to_owned()))?;
    let function = validated
        .function_by_id(function_id)
        .ok_or_else(|| fail(format!("no MIR function with id {}", function_id.0)))?;

    let mut seeds: Vec<(usize, Value)> = Vec::with_capacity(function.params.len());
    let mut remaining = args.iter();
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
            seeds.push((slot, Value::Int(i64::from(lane[axis]))));
        } else {
            let value = remaining
                .next()
                .ok_or_else(|| fail("too few arguments for the kernel's parameters".to_owned()))?;
            seeds.push((slot, value.clone()));
        }
    }
    if remaining.next().is_some() {
        return Err(fail(
            "too many arguments for the kernel's parameters".to_owned(),
        ));
    }

    let mut runner = Runner::new(program, context, context.types.as_table(), interner, host);
    runner.kernel_policy = RunnerKernelPolicy::Execute;
    let mut frame = runner.new_frame(function_id, MirBlockId(0));
    for (slot, value) in seeds {
        if let Some(cell) = frame.locals.get_mut(slot) {
            *cell = Some(value);
        }
    }
    runner.resume_frame(frame);
    runner.run_until_depth(0);
    runner.close_all_frames();
    if runner.errors.is_empty() {
        Ok(())
    } else {
        Err(std::mem::take(&mut runner.errors))
    }
}
