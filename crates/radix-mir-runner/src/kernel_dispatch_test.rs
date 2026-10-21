//! Tests for the `RunnerKernelPolicy::Dispatch` seam: a call to a compute-stage
//! function reaches the run's `KernelDispatcher` with evaluated arguments, a
//! `mut` argument is written back, and the dispatcher's value is the call's.

use super::kernel_dispatch::{self, DeviceToken, ElementRange, KernelDispatcher};
use super::*;
use crate::host::BufferHost;
use radix_lexer::Interner;
use radix_mir::{MirLocal, MirParam, MirParamMode, MirStatement, MirTerminator};
use radix_types::{Primitive, TypeTable};

struct Recorder {
    calls: Vec<(MirFunctionId, Vec<Value>)>,
    result: Result<Option<Value>, RunnerError>,
    /// The only function this dispatcher claims as a device kernel.
    claimed: Option<MirFunctionId>,
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
        if let Some(first) = args.first_mut() {
            *first = Value::Int(42);
        }
        if let Some(Value::Array(items)) = args.get(2) {
            items.borrow_mut().push(Value::Int(9));
        }
        self.result.clone()
    }

    fn claims_dispatch(&mut self, function: MirFunctionId) -> bool {
        self.claimed == Some(function)
    }
}

fn kernel_program(types: &mut TypeTable) -> MirProgram {
    let scalar = MirType::semantic(types.primitive(Primitive::Numerus));
    let scalar_id = scalar.semantic_id();
    let tuple = MirType::semantic(types.tuple(vec![scalar_id, scalar_id]));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let span = Span::default();
    // Element 0 of the local the dispatched companion's tuple binds to.
    let tuple_element = MirPlace {
        base: MirPlaceBase::Local(MirLocalId(3)),
        projections: vec![MirProjection::Index(MirOperand::Constant(
            MirConstant::Int(0),
        ))],
    };
    let typed_local = |id: u32, ty: MirType| MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable: true,
        span,
    };
    let local = |id: u32| typed_local(id, scalar);
    let param = |id: u32, mode: MirParamMode| MirParam {
        local: MirLocalId(id),
        name: None,
        ty: scalar,
        mode,
        span,
    };
    let mut program = MirProgram::new();
    // f0: the caller, with three locals holding the call's arguments and one
    // tuple local holding the companion's returned pair.
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![local(0), local(1), local(2), typed_local(3, tuple)],
        temps: vec![],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                // The dispatched companion's tuple return binds to the
                // destination local, exactly as a host call's value would.
                kind: MirStatementKind::Call {
                    destination: Some(MirPlace::local(MirLocalId(3))),
                    callee: MirCallee::Function(MirFunctionId(2)),
                    args: vec![
                        MirOperand::Place(MirPlace::local(MirLocalId(0))),
                        MirOperand::Place(MirPlace::local(MirLocalId(1))),
                    ],
                },
                span,
            }],
            // The entry returns element 0 of the bound tuple.
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(tuple_element))),
                span,
            },
            span,
        }],
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
    // f2: the device-resident companion — compute-stage, two scalar
    // parameters, and a tuple return. It has no blocks either: a passing call
    // proves only that the dispatcher ran and its tuple reached the caller.
    program.functions.push(MirFunction {
        id: MirFunctionId(2),
        source: None,
        name: None,
        params: vec![param(6, MirParamMode::Owned), param(7, MirParamMode::Owned)],
        locals: vec![local(6), local(7)],
        temps: vec![],
        blocks: vec![],
        return_ty: tuple,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: Some(radix_mir::MirKernelShaderStage::Compute),
        span,
    });
    program
}

/// Seed the caller's two argument locals.
fn seed_callsite(runner: &mut Runner<'_, BufferHost>) {
    let frame = runner.frames.last_mut().expect("caller frame");
    frame.seed_local(MirLocalId(0), Value::Int(3));
    frame.seed_local(MirLocalId(1), Value::Int(5));
}

fn place_args() -> Vec<MirOperand> {
    (0..3)
        .map(|id| MirOperand::Place(MirPlace::local(MirLocalId(id))))
        .collect()
}

#[test]
fn kernel_dispatch_reaches_the_dispatcher_and_writes_back() {
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let program = kernel_program(&mut types);
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let recorder = Rc::new(RefCell::new(Recorder {
        calls: Vec::new(),
        result: Ok(Some(Value::Int(7))),
        claimed: None,
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
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let program = kernel_program(&mut types);
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let recorder = Rc::new(RefCell::new(Recorder {
        calls: Vec::new(),
        claimed: None,
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
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let mut program = kernel_program(&mut types);
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

/// GT12 done_when 1, runner half: a device-resident companion's tuple return
/// binds at the call site, so the projection that reads element 0 sees the
/// dispatcher's value. Under `Refuse` the same call stays explicit.
#[test]
fn dispatched_companion_tuple_binds_at_call_site() {
    let mut types = TypeTable::new();
    let program = kernel_program(&mut types);
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let recorder = Rc::new(RefCell::new(Recorder {
        calls: Vec::new(),
        claimed: None,
        result: Ok(Some(Value::Tuple(vec![
            Value::Float(1.5),
            Value::Float(2.5),
        ]))),
    }));
    {
        let mut host = BufferHost::default();
        let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
        runner.kernel_policy = RunnerKernelPolicy::Dispatch;
        runner.kernel_dispatcher = Some(recorder.clone() as kernel_dispatch::ResidencyHook);
        runner.push_frame(MirFunctionId(0), MirBlockId(0));
        seed_callsite(&mut runner);
        runner.run_until_depth(0);
        assert!(
            runner.errors.is_empty(),
            "dispatch run failed: {:?}",
            runner.errors
        );
        // (b) the dispatched tuple bound at the call site: the entry's
        // projection of element 0 is what the run returned.
        assert_eq!(runner.return_value.take(), Some(Value::Float(1.5)));
    }
    // (a) the dispatcher saw the companion's id and its evaluated arguments.
    assert_eq!(recorder.borrow().calls.len(), 1);
    assert_eq!(recorder.borrow().calls[0].0, MirFunctionId(2));
    assert_eq!(
        recorder.borrow().calls[0].1,
        vec![Value::Int(3), Value::Int(5)]
    );

    // Reference-only execution stays explicit: under `Refuse` the same call is
    // the named refusal, never a silent host interpretation of the device body.
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    assert_eq!(runner.kernel_policy, RunnerKernelPolicy::Refuse);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    seed_callsite(&mut runner);
    runner.run_until_depth(0);
    let error = runner
        .errors
        .first()
        .expect("refused call must fail")
        .clone();
    assert_eq!(error.issue, RUNNER_KERNEL_REFUSED_ISSUE);
    // The whole diagnostic is the refusal constructor's own value; no prose is
    // re-spelled here.
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

/// The plain, stage-less callee a device image may nonetheless carry as a
/// kernel unit (the generic device-update invocation seam).
const PLAIN_CALLEE: MirFunctionId = MirFunctionId(3);
/// What the plain callee's host body computes.
const PLAIN_BODY_VALUE: i64 = 11;
/// What a dispatcher that owns the call returns instead.
const DISPATCHED_VALUE: i64 = 7;

/// `f0`: `let v ← f3(); v` — the entry calls a plain function with no
/// arguments. `f3`: a plain function with a one-block body that assigns a
/// constant to a local and returns it. Only the caller's value and the
/// dispatch records tell a host execution from a dispatched one.
fn plain_callee_program(types: &mut TypeTable) -> MirProgram {
    let scalar = MirType::semantic(types.primitive(Primitive::Numerus));
    let span = Span::default();
    let local = |id: u32| MirLocal {
        id: MirLocalId(id),
        name: None,
        ty: scalar,
        mutable: true,
        span,
    };
    let call_entry = MirStatement {
        kind: MirStatementKind::Call {
            destination: Some(MirPlace::local(MirLocalId(0))),
            callee: MirCallee::Function(PLAIN_CALLEE),
            args: vec![],
        },
        span,
    };
    let mut program = MirProgram::new();
    program.functions.push(MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![],
        locals: vec![local(0)],
        temps: vec![],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![call_entry],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(0),
                )))),
                span,
            },
            span,
        }],
        return_ty: scalar,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    });
    program.functions.push(MirFunction {
        id: PLAIN_CALLEE,
        source: None,
        name: None,
        params: vec![],
        locals: vec![local(0)],
        temps: vec![],
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace::local(MirLocalId(0)),
                    value: MirValue {
                        id: MirValueId(0),
                        kind: MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(
                            PLAIN_BODY_VALUE,
                        ))),
                        ty: scalar,
                        span,
                    },
                },
                span,
            }],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
                    MirLocalId(0),
                )))),
                span,
            },
            span,
        }],
        return_ty: scalar,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    });
    program
}

/// Run the plain-callee entry under `Dispatch` with a dispatcher that claims
/// `claimed` (if any), and return the run's return value plus the recorded
/// dispatch calls.
fn run_plain_callee(claimed: Option<MirFunctionId>) -> (Option<Value>, Vec<MirFunctionId>) {
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let program = plain_callee_program(&mut types);
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let recorder = Rc::new(RefCell::new(Recorder {
        calls: Vec::new(),
        result: Ok(Some(Value::Int(DISPATCHED_VALUE))),
        claimed,
    }));
    let return_value = {
        let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
        runner.kernel_policy = RunnerKernelPolicy::Dispatch;
        runner.kernel_dispatcher = Some(recorder.clone() as kernel_dispatch::ResidencyHook);
        runner.push_frame(MirFunctionId(0), MirBlockId(0));
        runner.run_until_depth(0);
        assert!(
            runner.errors.is_empty(),
            "plain callee run failed: {:?}",
            runner.errors
        );
        runner.return_value.take()
    };
    let callees = recorder
        .borrow()
        .calls
        .iter()
        .map(|(function, _)| *function)
        .collect();
    (return_value, callees)
}

/// The dispatcher owns the fact that a device kernel exists for a plain
/// function: a claim routes the call to it even though the callee carries no
/// shader stage, and the dispatcher's value is the call's value.
#[test]
fn claims_dispatch_routes_a_claimed_plain_function_to_the_dispatcher() {
    let (return_value, callees) = run_plain_callee(Some(PLAIN_CALLEE));
    assert_eq!(return_value, Some(Value::Int(DISPATCHED_VALUE)));
    assert_eq!(callees, vec![PLAIN_CALLEE]);
}

/// With no claim the same plain function executes on the host: its own body
/// computes the call's value and nothing reaches the dispatcher.
#[test]
fn claims_dispatch_leaves_an_unclaimed_plain_function_local() {
    let (return_value, callees) = run_plain_callee(None);
    assert_eq!(return_value, Some(Value::Int(PLAIN_BODY_VALUE)));
    assert!(callees.is_empty(), "no dispatch record for a plain call");
}
