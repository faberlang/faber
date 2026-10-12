//! Tests for the `RunnerKernelPolicy::Dispatch` seam: a call to a compute-stage
//! function reaches the run's `KernelDispatcher` with evaluated arguments, a
//! `mut` argument is written back, and the dispatcher's value is the call's.

use super::kernel_dispatch::{self, DeviceToken, ElementRange, KernelDispatcher};
use super::*;
use crate::host::BufferHost;
use radix_lexer::Interner;
use radix_mir::{MirLocal, MirParam, MirParamMode};
use radix_types::{Primitive, TypeTable};

struct Recorder {
    calls: Vec<(MirFunctionId, Vec<Value>)>,
    result: Result<Option<Value>, RunnerError>,
}

impl KernelDispatcher for Recorder {
    fn dispatch(
        &mut self,
        function: MirFunctionId,
        args: &mut [Value],
        _span: Span,
    ) -> Result<Option<Value>, RunnerError> {
        self.calls.push((function, args.to_vec()));
        // Replace the scalar `mut` argument and mutate the shared lista
        // argument in place.
        args[0] = Value::Int(42);
        if let Value::Array(items) = &args[2] {
            items.borrow_mut().push(Value::Int(9));
        }
        self.result.clone()
    }
}

fn kernel_program(types: &TypeTable) -> MirProgram {
    let scalar = MirType::semantic(types.primitive(Primitive::Numerus));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let span = Span::default();
    let local = |id: u32| MirLocal {
        id: MirLocalId(id),
        name: None,
        ty: scalar,
        mutable: true,
        span,
    };
    let param = |id: u32, mode: MirParamMode| MirParam {
        local: MirLocalId(id),
        name: None,
        ty: scalar,
        mode,
        span,
    };
    let mut program = MirProgram::new();
    // f0: the caller, with three locals holding the call's arguments.
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![local(0), local(1), local(2)],
        temps: vec![],
        blocks: vec![],
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    });
    // f1: a compute-stage function with no blocks; executing it would fail,
    // so a passing call proves the body never ran.
    program.functions.push(MirFunction {
        id: MirFunctionId(1),
        source: None,
        name: None,
        params: vec![
            param(3, MirParamMode::MutRef),
            param(4, MirParamMode::Ref),
            param(5, MirParamMode::MutRef),
        ],
        locals: vec![local(3), local(4), local(5)],
        temps: vec![],
        blocks: vec![],
        return_ty: scalar,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: Some(radix_mir::MirKernelShaderStage::Compute),
        span,
    });
    program
}

fn place_args() -> Vec<MirOperand> {
    (0..3)
        .map(|id| MirOperand::Place(MirPlace::local(MirLocalId(id))))
        .collect()
}

#[test]
fn kernel_dispatch_reaches_the_dispatcher_and_writes_back() {
    let types = TypeTable::new();
    let interner = Interner::new();
    let program = kernel_program(&types);
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let recorder = Rc::new(RefCell::new(Recorder {
        calls: Vec::new(),
        result: Ok(Some(Value::Int(7))),
    }));
    let list = Rc::new(RefCell::new(vec![Value::Int(1)]));
    {
        let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
        runner.kernel_policy = RunnerKernelPolicy::Dispatch;
        runner.kernel_dispatcher = Some(recorder.clone() as kernel_dispatch::ResidencyHook);
        runner.push_frame(MirFunctionId(0), MirBlockId(0));
        let frame = runner.frames.last_mut().expect("caller frame");
        frame.seed_local(MirLocalId(0), Value::Int(3));
        frame.seed_local(MirLocalId(1), Value::Int(5));
        frame.seed_local(MirLocalId(2), Value::Array(list.clone()));

        let outcome = runner
            .invoke_call(
                &MirCallee::Function(MirFunctionId(1)),
                &place_args(),
                Span::default(),
            )
            .expect("a dispatched kernel call succeeds");
        // (3) the stub's returned value is the call's value.
        assert!(matches!(outcome, CallOutcome::Ok(Value::Int(7))));
        // (2) the `mut` scalar replaced by the stub is visible to the caller,
        // the `in` argument is untouched, and the in-place lista mutation shows.
        assert_eq!(
            runner
                .read_local(MirLocalId(0), Span::default())
                .expect("mut local"),
            Value::Int(42)
        );
        assert_eq!(
            runner
                .read_local(MirLocalId(1), Span::default())
                .expect("in local"),
            Value::Int(5)
        );
    }
    assert_eq!(list.borrow().len(), 2);
    // (1) the dispatcher saw the right function id and argument values.
    assert_eq!(recorder.borrow().calls.len(), 1);
    assert_eq!(recorder.borrow().calls[0].0, MirFunctionId(1));
    assert_eq!(recorder.borrow().calls[0].1[0], Value::Int(3));
    assert_eq!(recorder.borrow().calls[0].1[1], Value::Int(5));
}

#[test]
fn kernel_dispatch_error_keeps_the_dispatcher_issue_code() {
    let types = TypeTable::new();
    let interner = Interner::new();
    let program = kernel_program(&types);
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let recorder = Rc::new(RefCell::new(Recorder {
        calls: Vec::new(),
        result: Err(RunnerError::internal_with_issue(
            Span::default(),
            "device lost",
            "stub_device_lost",
        )),
    }));
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.kernel_policy = RunnerKernelPolicy::Dispatch;
    runner.kernel_dispatcher = Some(recorder.clone() as kernel_dispatch::ResidencyHook);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let frame = runner.frames.last_mut().expect("caller frame");
    frame.seed_local(MirLocalId(0), Value::Int(3));
    frame.seed_local(MirLocalId(1), Value::Int(5));
    frame.seed_local(MirLocalId(2), Value::Array(Rc::default()));
    let Err(error) = runner.invoke_call(
        &MirCallee::Function(MirFunctionId(1)),
        &place_args(),
        Span::default(),
    ) else {
        panic!("a dispatcher error must stop the call");
    };
    assert_eq!(error.issue, "stub_device_lost");
}

/// A lane-form kernel carries a `Lane` parameter no caller supplies. Under the
/// `Refuse` policy (plain `faber run`, device `auto`) a call to it must be the
/// named refusal, never the arity mismatch the missing lane argument implies.
#[test]
fn refused_lane_form_kernel_call_is_the_named_refusal_not_an_arity_error() {
    let types = TypeTable::new();
    let interner = Interner::new();
    let mut program = kernel_program(&types);
    let scalar = MirType::semantic(types.primitive(Primitive::Numerus));
    let span = Span::default();
    program.functions[1].params.push(MirParam {
        local: MirLocalId(6),
        name: None,
        ty: scalar,
        mode: MirParamMode::Lane,
        span,
    });
    program.functions[1].locals.push(MirLocal {
        id: MirLocalId(6),
        name: None,
        ty: scalar,
        mutable: false,
        span,
    });
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    assert_eq!(runner.kernel_policy, RunnerKernelPolicy::Refuse);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let frame = runner.frames.last_mut().expect("caller frame");
    frame.seed_local(MirLocalId(0), Value::Int(3));
    frame.seed_local(MirLocalId(1), Value::Int(5));
    frame.seed_local(MirLocalId(2), Value::Array(Rc::default()));
    let Err(error) =
        runner.invoke_call(&MirCallee::Function(MirFunctionId(1)), &place_args(), span)
    else {
        panic!("a refused kernel call must fail");
    };
    assert_eq!(error.issue, RUNNER_KERNEL_REFUSED_ISSUE);
    // The whole diagnostic (message hint included) is the refusal constructor's
    // own value; no prose is re-spelled here.
    assert_eq!(error, kernel_refused(error.span));
}

/// Counts the residency callbacks a host write makes.
#[derive(Default)]
struct WriteRecorder {
    downloads: Vec<(DeviceToken, ElementRange)>,
    wrote: Vec<DeviceToken>,
}

impl KernelDispatcher for WriteRecorder {
    fn dispatch(
        &mut self,
        _function: MirFunctionId,
        _args: &mut [Value],
        _span: Span,
    ) -> Result<Option<Value>, RunnerError> {
        Ok(None)
    }

    fn make_host_current(
        &mut self,
        token: DeviceToken,
        range: ElementRange,
    ) -> Result<(), RunnerError> {
        self.downloads.push((token, range));
        Ok(())
    }

    fn host_wrote(&mut self, token: DeviceToken) {
        self.wrote.push(token);
    }
}

#[test]
fn before_write_downloads_then_marks_the_device_copy_stale_once() {
    let slot = kernel_dispatch::ResidencySlot::default();
    // No marker: a host write is host-only and touches no dispatcher.
    assert!(slot.before_write(4));
    let recorder = Rc::new(RefCell::new(WriteRecorder::default()));
    slot.mark(
        DeviceToken(7),
        recorder.clone() as kernel_dispatch::ResidencyHook,
        false,
    );
    assert!(slot.before_write(3));
    assert_eq!(recorder.borrow().downloads, vec![(DeviceToken(7), 0..3)]);
    assert_eq!(recorder.borrow().wrote, vec![DeviceToken(7)]);
    // The device copy is stale now: a second write neither downloads nor
    // re-marks it.
    assert!(slot.before_write(3));
    assert_eq!(recorder.borrow().downloads.len(), 1);
    assert_eq!(recorder.borrow().wrote.len(), 1);
}
