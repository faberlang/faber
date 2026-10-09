//! U9-5: queued kernel-error delivery for handled blocks, against a fake
//! dispatcher that injects recoverable faults for chosen launches.

use super::kernel_dispatch::{self, DeviceFault, DeviceToken, ElementRange, KernelDispatcher};
use super::*;
use crate::host::BufferHost;
use crate::value::TensorValue;
use radix_lexer::Interner;
use radix_mir::{
    MirBinOp, MirBlock, MirCallee, MirConstant, MirDeviceBlockOp, MirFunction, MirIntrinsic,
    MirLocal, MirPlace, MirProgram, MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator,
    MirTerminatorKind, MirValidationContext, MirValue, MirValueId, MirValueKind,
};
use radix_types::{Primitive, TypeTable};

#[derive(Default)]
struct Fake {
    launches: u32,
    fault_launches: Vec<u32>,
    /// Surface the launch's fault at once, as a host read would.
    surface_at: Option<u32>,
    next_owner: Option<u64>,
    pending: Vec<DeviceFault>,
    stash: Option<DeviceFault>,
    flushes: u32,
    delivered: Vec<String>,
    /// Hand back a device-resident output whose host read surfaces the fault.
    resident_output: bool,
    me: std::rc::Weak<RefCell<Fake>>,
}

impl Fake {
    fn surface(&mut self) {
        if self.stash.is_none() && !self.pending.is_empty() {
            self.stash = Some(self.pending.remove(0));
        }
        self.pending.clear(); // one error per flush; later ones are dropped
    }
}

impl KernelDispatcher for Fake {
    fn dispatch(
        &mut self,
        _f: MirFunctionId,
        _args: &mut [Value],
        _span: Span,
    ) -> Result<Option<Value>, RunnerError> {
        self.launches += 1;
        if self.fault_launches.contains(&self.launches) {
            self.pending.push(DeviceFault {
                text: format!("fault{}", self.launches),
                owner: self.next_owner,
            });
        }
        if self.surface_at == Some(self.launches) {
            self.surface();
        }
        if self.resident_output
            && let Some(me) = self.me.upgrade()
        {
            let output =
                TensorValue::try_contiguous(vec![Value::Int(41)], vec![1]).expect("tensor");
            output.mark_device_resident(
                DeviceToken(1),
                me as kernel_dispatch::ResidencyHook,
                false,
            );
            return Ok(Some(Value::Tensor(Rc::new(RefCell::new(output)))));
        }
        Ok(None)
    }
    fn make_host_current(
        &mut self,
        _token: DeviceToken,
        _range: ElementRange,
    ) -> Result<(), RunnerError> {
        self.surface();
        Ok(())
    }
    fn set_launch_owner(&mut self, owner: Option<u64>) {
        self.next_owner = owner;
    }
    fn flush(&mut self) -> Result<(), RunnerError> {
        self.flushes += 1;
        self.surface();
        Ok(())
    }
    fn deliver_faults(&mut self) -> Result<Option<DeviceFault>, RunnerError> {
        let fault = self.stash.take();
        if let Some(fault) = &fault {
            self.delivered.push(fault.text.clone());
        }
        Ok(fault)
    }
}

const T: u32 = 0; // trace
const E: u32 = 1; // error text
const C: u32 = 2; // counter
const TMP: u32 = 3;
const COND: u32 = 4;
const E2: u32 = 5;
const OUT: u32 = 6; // a launch's device-resident output
const READ: u32 = 7; // the host read of OUT

fn span() -> Span {
    Span::default()
}

fn local(id: u32, ty: MirType) -> MirLocal {
    MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable: true,
        span: span(),
    }
}

fn stmt(kind: MirStatementKind) -> MirStatement {
    MirStatement { kind, span: span() }
}

fn val(kind: MirValueKind, ty: MirType) -> MirValue {
    MirValue {
        id: MirValueId(0),
        kind,
        ty,
        span: span(),
    }
}

fn place(id: u32) -> MirPlace {
    MirPlace::local(MirLocalId(id))
}

fn num() -> MirType {
    MirType::semantic(TypeTable::new().primitive(Primitive::Numerus))
}

fn assign_const(ty: MirType, dest: u32, n: i64) -> MirStatement {
    stmt(MirStatementKind::Assign {
        place: place(dest),
        value: val(
            MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(n))),
            ty,
        ),
    })
}

fn binary(ty: MirType, dest: u32, op: MirBinOp, lhs: u32, rhs: i64) -> MirStatement {
    stmt(MirStatementKind::Assign {
        place: place(dest),
        value: val(
            MirValueKind::Binary {
                op,
                lhs: MirOperand::Place(place(lhs)),
                rhs: MirOperand::Constant(MirConstant::Int(rhs)),
            },
            ty,
        ),
    })
}

fn marker(op: MirDeviceBlockOp, error: Option<u32>) -> MirStatement {
    let vacuum = MirType::semantic(TypeTable::new().primitive(Primitive::Vacuum));
    stmt(MirStatementKind::RuntimeCall {
        destination: None,
        call: MirRuntimeCall {
            intrinsic: MirIntrinsic::DeviceBlock(op),
            args: error
                .map(|id| MirOperand::Place(place(id)))
                .into_iter()
                .collect(),
            return_ty: vacuum,
        },
    })
}

fn enter(omnia: bool, handler: u32, error: u32) -> MirStatement {
    marker(
        MirDeviceBlockOp::Enter {
            omnia,
            handler: MirBlockId(handler),
        },
        Some(error),
    )
}

fn exit() -> MirStatement {
    marker(MirDeviceBlockOp::Exit { error_edge: false }, None)
}

fn exit_err() -> MirStatement {
    marker(MirDeviceBlockOp::Exit { error_edge: true }, None)
}

fn deliver(sync: bool) -> MirStatement {
    marker(MirDeviceBlockOp::Deliver { sync }, None)
}

/// `t = t * 10 + digit`
fn trace(digit: i64) -> Vec<MirStatement> {
    let n = num();
    vec![
        binary(n, TMP, MirBinOp::Mul, T, 10),
        binary(n, T, MirBinOp::Add, TMP, digit),
    ]
}

fn block(
    id: u32,
    mut statements: Vec<MirStatement>,
    extra: Vec<Vec<MirStatement>>,
    kind: MirTerminatorKind,
) -> MirBlock {
    for more in extra {
        statements.extend(more);
    }
    MirBlock {
        id: MirBlockId(id),
        statements,
        terminator: MirTerminator { kind, span: span() },
        span: span(),
    }
}

fn goto(id: u32) -> MirTerminatorKind {
    MirTerminatorKind::Goto(MirBlockId(id))
}

fn ret(id: u32) -> MirTerminatorKind {
    MirTerminatorKind::Return(Some(MirOperand::Place(place(id))))
}

/// `callee` is f1 (the `⇥` kernel) unless given.
fn try_call(ok: u32, err_place: u32, err: u32) -> MirTerminatorKind {
    try_call_fn(1, ok, err_place, err)
}

fn try_call_fn(callee: u32, ok: u32, err_place: u32, err: u32) -> MirTerminatorKind {
    MirTerminatorKind::TryCall {
        destination: None,
        callee: MirCallee::Function(MirFunctionId(callee)),
        args: vec![],
        ok_block: MirBlockId(ok),
        error_place: place(err_place),
        error_block: MirBlockId(err),
    }
}

fn program(types: &TypeTable, main_blocks: Vec<MirBlock>, extra: Vec<MirFunction>) -> MirProgram {
    let n = MirType::semantic(types.primitive(Primitive::Numerus));
    let text = MirType::semantic(types.primitive(Primitive::Textus));
    let b = MirType::semantic(types.primitive(Primitive::Bivalens));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let locals = vec![
        local(T, n),
        local(E, text),
        local(C, n),
        local(TMP, n),
        local(COND, b),
        local(E2, text),
        local(OUT, n),
        local(READ, n),
    ];
    let function = |id: u32, blocks: Vec<MirBlock>, error_ty, stage| MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: vec![],
        locals: locals.clone(),
        temps: vec![],
        blocks,
        return_ty: if id == 0 { n } else { vacuum },
        error_ty,
        is_async: false,
        is_generator: false,
        shader_stage: stage,
        span: span(),
    };
    let mut program = MirProgram::new();
    program.functions.push(function(0, main_blocks, None, None));
    // f1: a `⇥ textus` compute kernel; its body never runs under dispatch.
    program.functions.push(function(
        1,
        vec![],
        Some(text),
        Some(radix_mir::MirKernelShaderStage::Compute),
    ));
    program.functions.extend(extra);
    program
}

struct Outcome {
    result: Option<Value>,
    errors: Vec<RunnerError>,
    fake: Rc<RefCell<Fake>>,
}

fn run(
    blocks: Vec<MirBlock>,
    extra: impl FnOnce(&TypeTable) -> Vec<MirFunction>,
    fake: Fake,
) -> Outcome {
    let types = TypeTable::new();
    let interner = Interner::new();
    let extra = extra(&types);
    let program = program(&types, blocks, extra);
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let fake = Rc::new(RefCell::new(fake));
    fake.borrow_mut().me = Rc::downgrade(&fake);
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.kernel_policy = RunnerKernelPolicy::Dispatch;
    runner.kernel_dispatcher = Some(fake.clone() as kernel_dispatch::ResidencyHook);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner.run_until_depth(0);
    Outcome {
        result: runner.return_value.take(),
        errors: std::mem::take(&mut runner.errors),
        fake,
    }
}

fn faulting(launches: &[u32]) -> Fake {
    Fake {
        fault_launches: launches.to_vec(),
        ..Fake::default()
    }
}

fn int(outcome: &Outcome) -> i64 {
    assert!(
        outcome.errors.is_empty(),
        "run errors: {:?}",
        outcome.errors
    );
    match &outcome.result {
        Some(Value::Int(n)) => *n,
        other => panic!("unexpected result {other:?}"),
    }
}

/// omnia { k() ; t = 5 } cape { t = 7 }
fn omnia_one_call() -> Vec<MirBlock> {
    let n = num();
    vec![
        block(
            0,
            vec![assign_const(n, T, 0), enter(true, 3, E)],
            vec![],
            goto(1),
        ),
        block(1, vec![], vec![], try_call(2, E, 3)),
        block(2, vec![], vec![trace(5), vec![exit()]], goto(4)),
        block(3, vec![exit_err()], vec![trace(7)], goto(4)),
        block(4, vec![], vec![], ret(T)),
    ]
}

#[test]
fn omnia_call_returns_at_once_and_the_error_arrives_at_block_exit() {
    let outcome = run(omnia_one_call(), |_| vec![], faulting(&[1]));
    // The body ran to its end (5), then the exit delivered into the handler (7).
    assert_eq!(int(&outcome), 57);
    let fake = outcome.fake.borrow();
    assert_eq!(fake.delivered, ["fault1"]);
    assert_eq!(fake.flushes, 2, "one exit flush, one handler-exit flush");
}

#[test]
fn omnia_without_a_fault_runs_the_body_and_flushes_once_at_exit() {
    let outcome = run(omnia_one_call(), |_| vec![], Fake::default());
    assert_eq!(int(&outcome), 5);
    assert_eq!(outcome.fake.borrow().flushes, 1);
}

#[test]
fn first_error_wins_and_later_ones_are_dropped() {
    let n = num();
    let blocks = vec![
        block(
            0,
            vec![assign_const(n, T, 0), enter(true, 4, E)],
            vec![],
            goto(1),
        ),
        block(1, vec![], vec![], try_call(2, E, 4)),
        block(2, vec![], vec![], try_call(3, E, 4)),
        block(3, vec![], vec![trace(5), vec![exit()]], goto(5)),
        block(4, vec![exit_err()], vec![trace(7)], goto(5)),
        block(5, vec![], vec![], ret(T)),
    ];
    let outcome = run(blocks, |_| vec![], faulting(&[1, 2]));
    assert_eq!(int(&outcome), 57);
    assert_eq!(outcome.fake.borrow().delivered, ["fault1"]);
}

#[test]
fn a_deliver_marker_takes_an_error_a_host_read_surfaced() {
    let n = num();
    let blocks = vec![
        block(
            0,
            vec![assign_const(n, T, 0), enter(true, 3, E)],
            vec![],
            goto(1),
        ),
        block(1, vec![], vec![], try_call(2, E, 3)),
        block(
            2,
            vec![deliver(false)],
            vec![trace(5), vec![exit()]],
            goto(4),
        ),
        block(3, vec![exit_err()], vec![trace(7)], goto(4)),
        block(4, vec![], vec![], ret(T)),
    ];
    let fake = Fake {
        fault_launches: vec![1],
        surface_at: Some(1),
        ..Fake::default()
    };
    let outcome = run(blocks, |_| vec![], fake);
    // The marker routed the error before the next statement ran: no 5.
    assert_eq!(int(&outcome), 7);
}

/// outer omnia { k() ; inner omnia { k() } cape { t = 2 } ; t = 1 } cape { t = 3 }
fn nested() -> Vec<MirBlock> {
    let n = num();
    vec![
        block(
            0,
            vec![assign_const(n, T, 0), enter(true, 6, E)],
            vec![],
            goto(1),
        ),
        block(1, vec![], vec![], try_call(2, E, 6)),
        block(2, vec![enter(true, 5, E2)], vec![], goto(3)),
        block(3, vec![], vec![], try_call(4, E2, 5)),
        block(4, vec![exit()], vec![trace(1), vec![exit()]], goto(7)),
        block(5, vec![exit_err()], vec![trace(2)], goto(7)),
        block(6, vec![exit_err()], vec![trace(3)], goto(7)),
        block(7, vec![], vec![], ret(T)),
    ]
}

#[test]
fn an_outer_launch_error_unwinds_the_inner_block() {
    let outcome = run(nested(), |_| vec![], faulting(&[1]));
    assert_eq!(
        int(&outcome),
        3,
        "outer handler only; the inner handler never ran"
    );
}

#[test]
fn an_inner_launch_error_goes_to_the_inner_handler() {
    let outcome = run(nested(), |_| vec![], faulting(&[2]));
    assert_eq!(int(&outcome), 2);
}

#[test]
fn plain_fac_waits_at_the_call() {
    let n = num();
    let blocks = vec![
        block(
            0,
            vec![assign_const(n, T, 0), enter(false, 3, E)],
            vec![],
            goto(1),
        ),
        block(1, vec![], vec![], try_call(2, E, 3)),
        block(2, vec![], vec![trace(5), vec![exit()]], goto(4)),
        block(3, vec![exit_err()], vec![trace(7)], goto(4)),
        block(4, vec![], vec![], ret(T)),
    ];
    let outcome = run(blocks, |_| vec![], faulting(&[1]));
    assert_eq!(
        int(&outcome),
        7,
        "the handler ran at the call, not after the body"
    );
    assert_eq!(outcome.fake.borrow().flushes, 1);
}

#[test]
fn omnia_dum_ends_the_loop_on_a_delivered_error() {
    let n = num();
    let b = MirType::semantic(TypeTable::new().primitive(Primitive::Bivalens));
    let blocks = |_: ()| {
        vec![
            block(
                0,
                vec![
                    assign_const(n, T, 0),
                    assign_const(n, C, 0),
                    enter(true, 5, E),
                ],
                vec![],
                goto(1),
            ),
            block(
                1,
                vec![binary(n, C, MirBinOp::Add, C, 1)],
                vec![],
                try_call(2, E, 5),
            ),
            block(
                2,
                vec![deliver(true), binary(b, COND, MirBinOp::Lt, C, 3)],
                vec![],
                MirTerminatorKind::Branch {
                    condition: MirOperand::Place(place(COND)),
                    then_block: MirBlockId(1),
                    else_block: MirBlockId(3),
                },
            ),
            block(3, vec![exit()], vec![trace(1)], goto(6)),
            block(5, vec![exit_err()], vec![trace(7)], goto(6)),
            block(
                6,
                vec![
                    binary(n, TMP, MirBinOp::Mul, C, 100),
                    binary(n, T, MirBinOp::Add, TMP, 0),
                ],
                vec![],
                ret(T),
            ),
        ]
    };
    // The result is the iteration count times 100.
    let ended = run(blocks(()), |_| vec![], faulting(&[2]));
    assert_eq!(int(&ended), 200, "the error at iteration 2 ended the loop");
    let full = run(blocks(()), |_| vec![], Fake::default());
    assert_eq!(int(&full), 300);
}

#[test]
fn an_omnia_block_inside_a_loop_continues_past_a_delivered_error() {
    let n = num();
    let b = MirType::semantic(TypeTable::new().primitive(Primitive::Bivalens));
    let blocks = vec![
        block(
            0,
            vec![assign_const(n, T, 0), assign_const(n, C, 0)],
            vec![],
            goto(1),
        ),
        block(
            1,
            vec![binary(b, COND, MirBinOp::Lt, C, 3)],
            vec![],
            MirTerminatorKind::Branch {
                condition: MirOperand::Place(place(COND)),
                then_block: MirBlockId(2),
                else_block: MirBlockId(6),
            },
        ),
        block(
            2,
            vec![binary(n, C, MirBinOp::Add, C, 1), enter(true, 5, E)],
            vec![],
            goto(3),
        ),
        block(3, vec![], vec![], try_call(4, E, 5)),
        block(4, vec![exit()], vec![], goto(1)),
        block(5, vec![exit_err()], vec![trace(7)], goto(1)),
        block(6, vec![], vec![], ret(T)),
    ];
    let outcome = run(blocks, |_| vec![], faulting(&[2]));
    // All three iterations ran (c == 3) and the handler ran once (t == 7).
    assert_eq!(int(&outcome), 7);
    assert_eq!(outcome.fake.borrow().launches, 3);
}

#[test]
fn no_open_block_propagates_through_a_textus_function_and_neither_is_fatal() {
    let n = num();
    // f2: `⇥ textus` caller of the kernel with no block of its own.
    let propagate = |types: &TypeTable| {
        let text = MirType::semantic(types.primitive(Primitive::Textus));
        let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
        let locals = vec![local(T, n), local(E, text)];
        vec![MirFunction {
            id: MirFunctionId(2),
            source: None,
            name: None,
            params: vec![],
            locals,
            temps: vec![],
            blocks: vec![
                block(0, vec![], vec![], try_call(1, E, 2)),
                block(1, vec![], vec![], MirTerminatorKind::Return(None)),
                block(
                    2,
                    vec![],
                    vec![],
                    MirTerminatorKind::ReturnError(MirOperand::Place(place(E))),
                ),
            ],
            return_ty: vacuum,
            error_ty: Some(text),
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: span(),
        }]
    };
    let blocks = vec![
        block(
            0,
            vec![assign_const(n, T, 0)],
            vec![],
            try_call_fn(2, 1, E, 2),
        ),
        block(1, vec![], vec![trace(5)], goto(3)),
        block(2, vec![], vec![trace(7)], goto(3)),
        block(3, vec![], vec![], ret(T)),
    ];
    let outcome = run(blocks, propagate, faulting(&[1]));
    assert_eq!(int(&outcome), 7);

    // Neither an open block nor a `⇥` function: the error cannot be handled.
    let bare = vec![
        block(
            0,
            vec![assign_const(n, T, 0)],
            vec![],
            MirTerminatorKind::Goto(MirBlockId(1)),
        ),
        block(
            1,
            vec![stmt(MirStatementKind::Call {
                destination: None,
                callee: MirCallee::Function(MirFunctionId(1)),
                args: vec![],
            })],
            vec![],
            ret(T),
        ),
    ];
    let fatal = run(bare, |_| vec![], faulting(&[1]));
    assert!(
        !fatal.errors.is_empty(),
        "an unhandled kernel error is fatal"
    );
}

#[test]
fn deliver_markers_do_no_device_work() {
    let n = num();
    let blocks = vec![
        block(
            0,
            vec![assign_const(n, T, 0), enter(true, 6, E)],
            vec![],
            goto(1),
        ),
        block(1, vec![], vec![], try_call(2, E, 6)),
        block(2, vec![deliver(false)], vec![], try_call(3, E, 6)),
        block(3, vec![deliver(false)], vec![], try_call(4, E, 6)),
        block(4, vec![deliver(false), exit()], vec![trace(5)], goto(5)),
        block(5, vec![], vec![], ret(T)),
        block(6, vec![exit_err()], vec![trace(7)], goto(5)),
    ];
    let outcome = run(blocks, |_| vec![], Fake::default());
    assert_eq!(int(&outcome), 5);
    let fake = outcome.fake.borrow();
    assert_eq!(
        (fake.launches, fake.flushes),
        (3, 1),
        "three launches, one flush at the block exit"
    );
}

/// omnia { out = k() ; read = out[0] ; t = 5 } cape { t = 7 }
fn omnia_host_read() -> Vec<MirBlock> {
    let n = num();
    let launch = MirTerminatorKind::TryCall {
        destination: Some(place(OUT)),
        callee: MirCallee::Function(MirFunctionId(1)),
        args: vec![],
        ok_block: MirBlockId(2),
        error_place: place(E),
        error_block: MirBlockId(3),
    };
    let read = stmt(MirStatementKind::Assign {
        place: place(READ),
        value: val(
            MirValueKind::Operand(MirOperand::Place(MirPlace {
                base: radix_mir::MirPlaceBase::Local(MirLocalId(OUT)),
                projections: vec![radix_mir::MirProjection::Index(MirOperand::Constant(
                    MirConstant::Int(0),
                ))],
            })),
            n,
        ),
    });
    vec![
        block(
            0,
            vec![assign_const(n, T, 0), enter(true, 3, E)],
            vec![],
            goto(1),
        ),
        block(1, vec![], vec![], launch),
        block(2, vec![read], vec![trace(5), vec![exit()]], goto(4)),
        block(3, vec![exit_err()], vec![trace(7)], goto(4)),
        block(4, vec![], vec![], ret(T)),
    ]
}

#[test]
fn a_host_read_of_a_faulted_output_delivers_before_the_read_completes() {
    let fake = Fake {
        fault_launches: vec![1],
        resident_output: true,
        ..Fake::default()
    };
    let outcome = run(omnia_host_read(), |_| vec![], fake);
    // The read surfaced the error; the handler ran at once, so the statement
    // after the read (the 5) never did.
    assert_eq!(int(&outcome), 7);
    assert_eq!(outcome.fake.borrow().delivered, ["fault1"]);
}

#[test]
fn a_host_read_without_a_fault_completes_normally() {
    let fake = Fake {
        resident_output: true,
        ..Fake::default()
    };
    let outcome = run(omnia_host_read(), |_| vec![], fake);
    assert_eq!(int(&outcome), 5);
    assert!(outcome.fake.borrow().delivered.is_empty());
}
