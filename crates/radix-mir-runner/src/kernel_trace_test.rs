use super::kernel_dispatch::KernelDispatcher;
use super::kernel_trace::{
    Comparison, Divergence, DivergenceDetail, ElementMatch, KernelTraceRecord, ReductionExtent,
    TraceElement, TraceRole, TracedTensor, compare_traces, match_elements, traced_result,
};
use super::*;
use crate::dense::DenseStorage;
use crate::host::BufferHost;
use crate::value::{TensorValue, Value};
use radix_lexer::Interner;
use radix_mir::{MirBlock, MirLocal, MirParam, MirParamMode, MirTerminator};
use radix_types::{Primitive, TypeTable};

#[test]
fn f32_elements_are_float_and_exactly_widened() {
    let tensor = TensorValue::try_contiguous_typed(DenseStorage::F32(vec![1.5, 0.1]), vec![2])
        .expect("valid tensor shape");
    let traced = kernel_trace::traced_tensor("tensor:0", kernel_trace::TraceRole::Input, &tensor)
        .expect("host tensor traces");

    assert_eq!(traced.slot, "tensor:0");
    assert_eq!(traced.elements.len(), 2);
    assert_eq!(traced.elements[0], kernel_trace::TraceElement::Float(1.5));
    assert_eq!(
        traced.elements[1],
        kernel_trace::TraceElement::Float(f64::from(0.1_f32))
    );
}

#[test]
fn i64_elements_keep_their_twos_complement_bits() {
    let tensor = TensorValue::try_contiguous_typed(DenseStorage::I64(vec![-1, 7]), vec![2])
        .expect("valid tensor shape");
    let traced = kernel_trace::traced_tensor("tensor:0", kernel_trace::TraceRole::Input, &tensor)
        .expect("host tensor traces");

    assert_eq!(
        traced.elements,
        vec![
            kernel_trace::TraceElement::Exact(u64::MAX),
            kernel_trace::TraceElement::Exact(7)
        ]
    );
}

#[test]
fn bool_elements_are_exact_zero_or_one() {
    let tensor = TensorValue::try_contiguous_typed(DenseStorage::Bool(vec![true, false]), vec![2])
        .expect("valid tensor shape");
    let traced = kernel_trace::traced_tensor("tensor:0", kernel_trace::TraceRole::Input, &tensor)
        .expect("host tensor traces");

    assert_eq!(
        traced.elements,
        vec![
            kernel_trace::TraceElement::Exact(1),
            kernel_trace::TraceElement::Exact(0)
        ]
    );
}

#[test]
fn a_values_backing_records_floats_ints_and_text() {
    let tensor = TensorValue::try_contiguous(
        vec![
            Value::Float(2.0),
            Value::Int(-3),
            Value::Str("x".to_owned()),
        ],
        vec![3],
    )
    .expect("valid tensor shape");
    let traced = kernel_trace::traced_tensor("tensor:0", kernel_trace::TraceRole::Input, &tensor)
        .expect("host tensor traces");

    assert_eq!(traced.elements[0], kernel_trace::TraceElement::Float(2.0));
    assert_eq!(
        traced.elements[1],
        kernel_trace::TraceElement::Exact(u64::MAX - 2)
    );
    match &traced.elements[2] {
        kernel_trace::TraceElement::Text(text) => assert!(text.contains('x')),
        other => panic!("expected text trace element, got {other:?}"),
    }
}

#[test]
fn traced_tensor_records_shape_and_role() {
    let tensor = TensorValue::try_contiguous_typed(DenseStorage::F32(vec![1.0; 6]), vec![2, 3])
        .expect("valid tensor shape");
    let traced = kernel_trace::traced_tensor("arg1", TraceRole::Output, &tensor)
        .expect("host tensor traces");
    assert_eq!(traced.shape, vec![2, 3]);
    assert_eq!(traced.role, TraceRole::Output);
    assert_eq!(traced.elements.len(), 6);
}

// ---- the comparator ------------------------------------------------------

fn floats(values: &[f64]) -> Vec<TraceElement> {
    values.iter().copied().map(TraceElement::Float).collect()
}

fn record(
    index: usize,
    entry: &str,
    shape: &[i64],
    elements: Vec<TraceElement>,
) -> KernelTraceRecord {
    KernelTraceRecord {
        index,
        function: 1,
        entry: entry.to_owned(),
        physical_stages: None,
        tensors: vec![TracedTensor {
            slot: "arg1".to_owned(),
            role: TraceRole::Output,
            shape: shape.to_vec(),
            elements,
            reduction: None,
        }],
        result: None,
    }
}

/// Six clean launches, then `decode_rope_q` at launch 6 (the gea3 shape).
fn gea3_trace(rope_q: Vec<TraceElement>) -> Vec<KernelTraceRecord> {
    let mut trace: Vec<_> = (1..=5)
        .map(|index| record(index, "decode_norm", &[1, 4], floats(&[1.0, 2.0, 3.0, 4.0])))
        .collect();
    trace.push(record(6, "decode_rope_q", &[1, 960], rope_q));
    trace
}

fn rope_values() -> Vec<f64> {
    (0..960).map(|i| f64::from(i) * 0.25 - 100.0).collect()
}

#[test]
fn the_gea3_exact_zero_launch_is_named_by_kernel_and_element() {
    let reference = gea3_trace(floats(&rope_values()));
    let mut zeroed = rope_values();
    zeroed.iter_mut().for_each(|value| *value = 0.0);
    // Element 0 of the reference is -100.0; the device returned exact zeros.
    let device = gea3_trace(floats(&zeroed));

    let comparison = compare_traces(&reference, &device);
    let divergence = comparison.divergence.expect("exact zeros must diverge");
    assert_eq!(divergence.call, 6);
    assert_eq!(divergence.entry, "decode_rope_q");
    assert_eq!(
        divergence.detail,
        DivergenceDetail::Element {
            slot: "arg1".to_owned(),
            shape: vec![1, 960],
            element: 0,
            reference: TraceElement::Float(-100.0),
            device: TraceElement::Float(0.0),
        }
    );
    let text = divergence.to_string();
    assert!(text.contains("kernel call 6 `decode_rope_q`"), "{text}");
    assert!(text.contains("element 0 at [0, 0]"), "{text}");
    assert!(text.contains("reference -100.0"), "{text}");
    assert!(text.contains("device 0.0"), "{text}");
}

#[test]
fn a_zero_on_the_first_nonzero_element_is_the_divergence_not_a_later_one() {
    let mut want = rope_values();
    want[0] = 0.0;
    want[1] = 0.0;
    want[2] = 7.5;
    want[3] = 8.5;
    let mut got = want.clone();
    got[2] = 0.0;
    got[3] = 0.0;
    let comparison = compare_traces(&gea3_trace(floats(&want)), &gea3_trace(floats(&got)));
    let Some(Divergence {
        detail: DivergenceDetail::Element { element, .. },
        ..
    }) = comparison.divergence
    else {
        panic!("expected an element divergence");
    };
    assert_eq!(element, 2);
}

#[test]
fn identical_traces_report_identical_with_no_divergence() {
    let trace = gea3_trace(floats(&rope_values()));
    let comparison = compare_traces(&trace, &trace.clone());
    assert!(comparison.is_identical());
    assert_eq!(comparison.calls_compared, 6);
    assert!(comparison.report().starts_with("IDENTICAL"));
}

#[test]
fn a_conforming_pair_reports_conforming_never_identical() {
    let want = rope_values();
    // One part in 1e7 relative: inside rtol 1e-6.
    let got: Vec<f64> = want.iter().map(|value| value * (1.0 + 1e-7)).collect();
    let comparison = compare_traces(&gea3_trace(floats(&want)), &gea3_trace(floats(&got)));
    assert!(
        comparison.divergence.is_none(),
        "{:?}",
        comparison.divergence
    );
    assert!(!comparison.is_identical());
    assert!(comparison.conforming_elements > 0);
    assert!(comparison.report().starts_with("CONFORMING"));
}

#[test]
fn float_matching_follows_the_tolerance_rules() {
    let f = TraceElement::Float;
    assert_eq!(
        match_elements(&f(1.0), &f(1.0), None),
        ElementMatch::Identical
    );
    // atol covers values near zero.
    assert_eq!(
        match_elements(&f(0.0), &f(5e-7), None),
        ElementMatch::Conforming
    );
    assert_eq!(
        match_elements(&f(0.0), &f(2e-6), None),
        ElementMatch::Differs
    );
    // rtol scales with the reference.
    assert_eq!(
        match_elements(&f(1000.0), &f(1000.0005), None),
        ElementMatch::Conforming
    );
    assert_eq!(
        match_elements(&f(1000.0), &f(1000.01), None),
        ElementMatch::Differs
    );
    // Both NaN conform; NaN against a number and a lost infinity do not.
    assert_eq!(
        match_elements(&f(f64::NAN), &f(-f64::NAN), None),
        ElementMatch::Conforming
    );
    assert_eq!(
        match_elements(&f(1.0), &f(f64::NAN), None),
        ElementMatch::Differs
    );
    assert_eq!(
        match_elements(&f(f64::INFINITY), &f(f64::MAX), None),
        ElementMatch::Differs
    );
    assert_eq!(
        match_elements(&f(f64::INFINITY), &f(f64::INFINITY), None),
        ElementMatch::Identical
    );
}

#[test]
fn exact_elements_must_match_bit_for_bit() {
    let exact = TraceElement::Exact;
    assert_eq!(
        match_elements(&exact(7), &exact(7), None),
        ElementMatch::Identical
    );
    // A changed integer is a defect however close it is.
    assert_eq!(
        match_elements(&exact(7), &exact(8), None),
        ElementMatch::Differs
    );
    // Kinds never mix.
    assert_eq!(
        match_elements(&exact(0), &TraceElement::Float(0.0), None),
        ElementMatch::Differs
    );
}

#[test]
fn a_carried_reduction_bound_widens_only_that_tensor() {
    let extent = ReductionExtent {
        partials: 2,
        partial_len: 256,
        sum_abs: 1.0e4,
        unit_roundoff: 2.0_f64.powi(-24),
    };
    // bound = (255 + 1) * 2^-24 * 1e4 ~= 0.1526
    let want = TraceElement::Float(5000.0);
    let got = TraceElement::Float(5000.1);
    assert_eq!(match_elements(&want, &got, None), ElementMatch::Differs);
    assert_eq!(
        match_elements(&want, &got, Some(&extent)),
        ElementMatch::Conforming
    );
    assert_eq!(
        match_elements(&want, &TraceElement::Float(5000.2), Some(&extent)),
        ElementMatch::Differs
    );
}

#[test]
fn a_missing_or_extra_call_and_a_changed_entry_are_named() {
    let full = gea3_trace(floats(&rope_values()));
    let short = full[..4].to_vec();
    let comparison = compare_traces(&full, &short);
    let divergence = comparison.divergence.expect("a missing call diverges");
    assert_eq!(divergence.call, 5);
    assert_eq!(
        divergence.detail,
        DivergenceDetail::CallCount {
            reference: 6,
            device: 4
        }
    );

    let mut renamed = full.clone();
    renamed[2].entry = "decode_other".to_owned();
    let divergence = compare_traces(&full, &renamed)
        .divergence
        .expect("a changed entry diverges");
    assert_eq!(divergence.call, 3);
    assert!(matches!(divergence.detail, DivergenceDetail::Entry { .. }));
}

#[test]
fn a_shape_mismatch_is_a_layout_divergence() {
    let reference = gea3_trace(floats(&rope_values()));
    let mut device = reference.clone();
    device[5].tensors[0].shape = vec![960, 1];
    let divergence = compare_traces(&reference, &device)
        .divergence
        .expect("a shape change diverges");
    assert_eq!(divergence.call, 6);
    assert!(matches!(divergence.detail, DivergenceDetail::Layout(_)));
}

#[test]
fn the_physical_stage_count_rides_the_report() {
    let reference = gea3_trace(floats(&rope_values()));
    let mut device = gea3_trace(floats(&vec![0.0; 960]));
    device[5].physical_stages = Some(2);
    let divergence = compare_traces(&reference, &device)
        .divergence
        .expect("diverges");
    assert_eq!(divergence.physical_stages, Some(2));
    assert!(divergence.to_string().contains("(2 physical stages)"));
}

#[test]
fn an_empty_pair_is_identical() {
    let comparison: Comparison = compare_traces(&[], &[]);
    assert!(comparison.is_identical());
    assert_eq!(comparison.calls_compared, 0);
}

// ---- capture at the call boundary ----------------------------------------

struct Capture {
    program: MirProgram,
    types: TypeTable,
    interner: Interner,
}

/// f0: a caller. f1 `decode_rope_q`: a compute-stage kernel `(in t, mut t)`
/// that returns at once. f2: a compute-stage kernel with no blocks, which
/// fails when executed.
fn capture_program() -> Capture {
    let types = TypeTable::new();
    let mut interner = Interner::new();
    let name = interner.intern("decode_rope_q");
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
    let function = |id: u32, name, blocks, params: Vec<MirParam>, stage| MirFunction {
        id: MirFunctionId(id),
        source: None,
        name,
        locals: params.iter().map(|p| local(p.local.0)).collect(),
        params,
        temps: vec![],
        blocks,
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: stage,
        span,
    };
    let returns = vec![MirBlock {
        id: MirBlockId(0),
        statements: vec![],
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span,
        },
        span,
    }];
    let compute = Some(radix_mir::MirKernelShaderStage::Compute);
    let mut program = MirProgram::new();
    program
        .functions
        .push(function(0, None, returns.clone(), vec![], None));
    program.functions.push(function(
        1,
        Some(name),
        returns,
        vec![param(1, MirParamMode::Ref), param(2, MirParamMode::MutRef)],
        compute,
    ));
    program.functions.push(function(
        2,
        None,
        vec![],
        vec![param(1, MirParamMode::Ref), param(2, MirParamMode::MutRef)],
        compute,
    ));
    // f3: a compute-stage kernel `(in t, mut t)` that returns the scalar
    // 2.5 (the shape of a mean kernel).
    let mut scalar_return = function(
        3,
        None,
        vec![MirBlock {
            id: MirBlockId(0),
            statements: vec![],
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(Some(MirOperand::Constant(MirConstant::Float(
                    2.5,
                )))),
                span,
            },
            span,
        }],
        vec![param(1, MirParamMode::Ref), param(2, MirParamMode::MutRef)],
        compute,
    );
    scalar_return.return_ty = MirType::semantic(types.primitive(Primitive::Fractus));
    program.functions.push(scalar_return);
    Capture {
        program,
        types,
        interner,
    }
}

fn tensor_value(values: Vec<f32>, shape: Vec<i64>) -> Value {
    Value::Tensor(Rc::new(RefCell::new(
        TensorValue::try_contiguous_typed(DenseStorage::F32(values), shape)
            .expect("valid tensor shape"),
    )))
}

fn call_args() -> Vec<MirOperand> {
    (0..2)
        .map(|id| MirOperand::Place(MirPlace::local(MirLocalId(id))))
        .collect()
}

fn seed(runner: &mut Runner<'_, BufferHost>) {
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let frame = runner.frames.last_mut().expect("caller frame");
    frame.seed_local(MirLocalId(0), tensor_value(vec![1.0, 2.0], vec![1, 2]));
    frame.seed_local(MirLocalId(1), tensor_value(vec![0.0, 0.0], vec![1, 2]));
}

#[test]
fn an_executed_kernel_call_is_recorded_after_it_completes() {
    let fixture = capture_program();
    let context = MirValidationContext::new(&fixture.types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(
        &fixture.program,
        &context,
        &fixture.types,
        &fixture.interner,
        &mut host,
    );
    runner.kernel_policy = RunnerKernelPolicy::Execute;
    runner.kernel_trace = Some(Vec::new());
    seed(&mut runner);
    for _ in 0..2 {
        runner
            .invoke_call(
                &MirCallee::Function(MirFunctionId(1)),
                &call_args(),
                Span::default(),
            )
            .expect("the kernel runs");
    }
    let trace = runner.kernel_trace.take().expect("trace armed");
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[1].index, 2);
    assert_eq!(trace[0].entry, "decode_rope_q");
    assert_eq!(trace[0].function, 1);
    assert_eq!(trace[0].physical_stages, None);
    let slots: Vec<_> = trace[0]
        .tensors
        .iter()
        .map(|tensor| (tensor.slot.as_str(), tensor.role, tensor.shape.clone()))
        .collect();
    assert_eq!(
        slots,
        vec![
            ("arg0", TraceRole::Input, vec![1, 2]),
            ("arg1", TraceRole::Output, vec![1, 2]),
        ]
    );
    assert_eq!(trace[0].tensors[0].elements, floats(&[1.0, 2.0]));
}

#[test]
fn an_untraced_run_records_nothing_and_behaves_as_before() {
    let fixture = capture_program();
    let context = MirValidationContext::new(&fixture.types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(
        &fixture.program,
        &context,
        &fixture.types,
        &fixture.interner,
        &mut host,
    );
    runner.kernel_policy = RunnerKernelPolicy::Execute;
    seed(&mut runner);
    let outcome = runner
        .invoke_call(
            &MirCallee::Function(MirFunctionId(1)),
            &call_args(),
            Span::default(),
        )
        .expect("the kernel runs");
    assert!(matches!(outcome, CallOutcome::Ok(Value::Unit)));
    assert!(runner.kernel_trace.is_none());
}

#[test]
fn a_failed_kernel_call_is_not_recorded_and_earlier_records_stay() {
    let fixture = capture_program();
    let context = MirValidationContext::new(&fixture.types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(
        &fixture.program,
        &context,
        &fixture.types,
        &fixture.interner,
        &mut host,
    );
    runner.kernel_policy = RunnerKernelPolicy::Execute;
    runner.kernel_trace = Some(Vec::new());
    seed(&mut runner);
    runner
        .invoke_call(
            &MirCallee::Function(MirFunctionId(1)),
            &call_args(),
            Span::default(),
        )
        .expect("the first kernel runs");
    let failed = runner.invoke_call(
        &MirCallee::Function(MirFunctionId(2)),
        &call_args(),
        Span::default(),
    );
    assert!(failed.is_err(), "a kernel with no blocks fails");
    let trace = runner.kernel_trace.take().expect("trace armed");
    assert_eq!(trace.len(), 1, "only the completed call is recorded");
    assert_eq!(runner.kernel_trace_depth, 0, "the nesting counter unwinds");
}

/// Writes 3.0 into element 1 of its `mut` tensor and reports two stages.
struct Writer;

impl KernelDispatcher for Writer {
    fn dispatch(
        &mut self,
        _function: MirFunctionId,
        args: &mut [Value],
        _span: Span,
    ) -> Result<Option<Value>, RunnerError> {
        let Some(Value::Tensor(out)) = args.get(1) else {
            panic!("expected a tensor output");
        };
        out.borrow()
            .set_value_at(1, Value::Float(3.0))
            .expect("write the output");
        Ok(None)
    }

    fn last_call_stages(&mut self) -> Option<u32> {
        Some(2)
    }
}

#[test]
fn a_dispatched_kernel_call_records_the_outputs_and_the_stage_count() {
    let fixture = capture_program();
    let context = MirValidationContext::new(&fixture.types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(
        &fixture.program,
        &context,
        &fixture.types,
        &fixture.interner,
        &mut host,
    );
    runner.kernel_policy = RunnerKernelPolicy::Dispatch;
    runner.kernel_dispatcher =
        Some(Rc::new(RefCell::new(Writer)) as kernel_dispatch::ResidencyHook);
    runner.kernel_trace = Some(Vec::new());
    seed(&mut runner);
    runner
        .invoke_call(
            &MirCallee::Function(MirFunctionId(1)),
            &call_args(),
            Span::default(),
        )
        .expect("the dispatched kernel runs");
    let trace = runner.kernel_trace.take().expect("trace armed");
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0].physical_stages, Some(2));
    assert_eq!(trace[0].tensors[1].elements, floats(&[0.0, 3.0]));
}

// ---- scalar kernel returns -------------------------------------------------

#[test]
fn an_executed_kernel_call_records_its_scalar_return() {
    let fixture = capture_program();
    let context = MirValidationContext::new(&fixture.types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(
        &fixture.program,
        &context,
        &fixture.types,
        &fixture.interner,
        &mut host,
    );
    runner.kernel_policy = RunnerKernelPolicy::Execute;
    runner.kernel_trace = Some(Vec::new());
    seed(&mut runner);
    for function in [1, 3] {
        runner
            .invoke_call(
                &MirCallee::Function(MirFunctionId(function)),
                &call_args(),
                Span::default(),
            )
            .expect("the kernel runs");
    }
    let trace = runner.kernel_trace.take().expect("trace armed");
    assert_eq!(trace.len(), 2);
    assert_eq!(
        trace[0].result, None,
        "a call returning nothing has no result"
    );
    assert_eq!(trace[1].result, Some(TraceElement::Float(2.5)));
    // The scalar return does not appear as a tensor.
    assert_eq!(trace[1].tensors.len(), 2);
}

#[test]
fn traced_result_keeps_numbers_and_bools_and_nothing_else() {
    assert_eq!(
        traced_result(&Value::Float(0.5)),
        Some(TraceElement::Float(0.5))
    );
    assert_eq!(
        traced_result(&Value::Int(-1)),
        Some(TraceElement::Exact(u64::MAX))
    );
    assert_eq!(traced_result(&Value::UInt(9)), Some(TraceElement::Exact(9)));
    assert_eq!(
        traced_result(&Value::Bool(true)),
        Some(TraceElement::Exact(1))
    );
    assert_eq!(traced_result(&Value::Unit), None);
    assert_eq!(traced_result(&Value::Str("x".to_owned())), None);
}

fn with_result(result: Option<TraceElement>) -> Vec<KernelTraceRecord> {
    let mut call = record(1, "mean_lin_32", &[1], floats(&[1.0]));
    call.result = result;
    vec![call]
}

#[test]
fn a_changed_exact_scalar_return_is_a_divergence_naming_the_return() {
    // The kernel-fused-mean-extents class: every tensor is identical, only the
    // scalar the kernel returned is wrong.
    let reference = with_result(Some(TraceElement::Exact(7)));
    let device = with_result(Some(TraceElement::Exact(8)));
    let comparison = compare_traces(&reference, &device);
    assert_eq!(
        comparison.divergence,
        Some(Divergence {
            call: 1,
            physical_stages: None,
            entry: "mean_lin_32".to_owned(),
            detail: DivergenceDetail::Return {
                reference: TraceElement::Exact(7),
                device: TraceElement::Exact(8),
            },
        })
    );
    assert!(
        comparison.report().contains("return value"),
        "{}",
        comparison.report()
    );
    assert!(!comparison.is_identical());
}

#[test]
fn a_scalar_float_return_is_identical_conforming_or_divergent() {
    let want = 0.024_414_063_f64;

    let identical = compare_traces(
        &with_result(Some(TraceElement::Float(want))),
        &with_result(Some(TraceElement::Float(want))),
    );
    assert!(identical.is_identical(), "{}", identical.report());

    // Inside atol + rtol * |reference|: conforming, never called identical.
    let conforming = compare_traces(
        &with_result(Some(TraceElement::Float(want))),
        &with_result(Some(TraceElement::Float(want + 5e-7))),
    );
    assert!(conforming.divergence.is_none(), "{}", conforming.report());
    assert_eq!(conforming.conforming_elements, 1);
    assert!(!conforming.is_identical());
    assert!(conforming.report().starts_with("CONFORMING"));

    // The sign error the Metal reduction bug produced: -0.16015625 against
    // 0.024414063.
    let divergent = compare_traces(
        &with_result(Some(TraceElement::Float(want))),
        &with_result(Some(TraceElement::Float(-0.160_156_25))),
    );
    assert!(
        matches!(
            divergent.divergence.as_ref().map(|d| &d.detail),
            Some(DivergenceDetail::Return { .. })
        ),
        "{}",
        divergent.report()
    );
}

#[test]
fn a_scalar_return_on_one_side_only_is_a_layout_divergence() {
    let comparison = compare_traces(
        &with_result(Some(TraceElement::Float(1.0))),
        &with_result(None),
    );
    assert!(
        matches!(
            comparison.divergence.as_ref().map(|d| &d.detail),
            Some(DivergenceDetail::Layout(_))
        ),
        "{}",
        comparison.report()
    );
    let comparison = compare_traces(
        &with_result(None),
        &with_result(Some(TraceElement::Float(1.0))),
    );
    assert!(comparison.divergence.is_some());
}
