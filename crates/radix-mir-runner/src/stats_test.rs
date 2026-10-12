//! Focused proofs for the opt-in runner meter.

use super::*;
use crate::stats::{RESERVOIR_EXACT, RESERVOIR_METHOD, RESERVOIR_SIZE, StatsCollector};
use radix_hir::DefId;
use radix_lexer::Interner;
use radix_mir::{
    MirBlock, MirConstant, MirConversion, MirConversionFlavor, MirCursorStream, MirFunction,
    MirIntrinsic, MirOperand, MirProgram, MirRuntimeCall, MirStatement, MirStatementKind,
    MirTerminator, MirTerminatorKind, MirType, MirValidationContext,
};
use radix_types::{Primitive, TypeTable};

fn function(
    id: MirFunctionId,
    source: Option<DefId>,
    name: Option<Symbol>,
    return_ty: MirType,
    error_ty: Option<MirType>,
    blocks: Vec<MirBlock>,
) -> MirFunction {
    MirFunction {
        id,
        source,
        name,
        params: vec![],
        locals: vec![],
        temps: vec![],
        blocks,
        return_ty,
        error_ty,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

fn empty_block() -> MirBlock {
    MirBlock {
        id: MirBlockId(0),
        statements: vec![],
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span: Span::default(),
        },
        span: Span::default(),
    }
}

#[test]
fn exact_samples_and_intrinsic_leaf_rows_are_snapshot_sorted() {
    let types = TypeTable::new();
    let vacuum = types.primitive(Primitive::Vacuum);
    let mut interner = Interner::new();
    let alpha = interner.intern("alpha");
    let zeta = interner.intern("zeta");
    let mut context = MirValidationContext::new(&types);
    context.external_function_identities.insert(
        DefId(10),
        radix_mir::MirExternalFunctionIdentity::new(
            "fixture",
            vec!["pkg".to_owned(), "alpha".to_owned()],
            "zeta",
        ),
    );
    context.external_function_identities.insert(
        DefId(11),
        radix_mir::MirExternalFunctionIdentity::new(
            "fixture",
            vec!["pkg".to_owned(), "beta".to_owned()],
            "alpha",
        ),
    );
    let mut program = MirProgram::new();
    program.functions.push(function(
        MirFunctionId(0),
        Some(DefId(10)),
        Some(zeta),
        MirType::semantic(vacuum),
        None,
        vec![empty_block()],
    ));
    program.functions.push(function(
        MirFunctionId(1),
        Some(DefId(11)),
        Some(alpha),
        MirType::semantic(vacuum),
        None,
        vec![empty_block()],
    ));

    let mut collector = StatsCollector::new(&program, &context, &interner);
    collector.record_function_sample(MirFunctionId(0), 10, 10);
    collector.record_function_sample(MirFunctionId(1), 20, 20);
    collector.record_intrinsic_sample("convert", 7);
    let stats = collector.finish();

    assert_eq!(
        stats
            .functions
            .iter()
            .map(|row| (row.module.as_str(), row.name.as_str()))
            .collect::<Vec<_>>(),
        vec![("pkg::alpha", "zeta"), ("pkg::beta", "alpha")]
    );
    assert_eq!(stats.functions[0].calls, 1);
    assert_eq!(stats.functions[0].total_ns, 10);
    assert_eq!(stats.functions[0].self_ns, 10);
    assert_eq!(stats.intrinsics[0].calls, 1);
    assert_eq!(stats.intrinsics[0].total_ns, 7);
    assert_eq!(stats.intrinsics[0].self_ns, stats.intrinsics[0].total_ns);
    assert_eq!(stats.intrinsics[0].p95_ns, 7);
    assert_eq!(RESERVOIR_METHOD, "reservoir");
    assert_eq!(RESERVOIR_SIZE, 256);
    const {
        assert!(!RESERVOIR_EXACT);
    }
}

#[test]
fn reservoir_retains_small_runs_and_is_bounded_deterministic_after_capacity() {
    let types = TypeTable::new();
    let vacuum = types.primitive(Primitive::Vacuum);
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(function(
        MirFunctionId(0),
        None,
        None,
        MirType::semantic(vacuum),
        None,
        vec![empty_block()],
    ));

    let mut small = StatsCollector::new(&program, &context, &interner);
    for duration in 1..=RESERVOIR_SIZE as u64 {
        small.record_function_sample(MirFunctionId(0), duration, duration);
    }
    let small = small.finish();
    let row = &small.functions[0];
    assert_eq!(row.calls, 256);
    assert_eq!(row.total_ns, 256 * 257 / 2);
    assert_eq!(row.self_ns, row.total_ns);
    assert_eq!((row.p50_ns, row.p95_ns, row.max_ns), (128, 244, 256));

    let mut first = StatsCollector::new(&program, &context, &interner);
    let mut second = StatsCollector::new(&program, &context, &interner);
    for duration in 1..=512 {
        first.record_function_sample(MirFunctionId(0), duration, duration);
        second.record_function_sample(MirFunctionId(0), duration, duration);
    }
    let first = first.finish();
    let second = second.finish();
    assert_eq!(first, second);
    let row = &first.functions[0];
    assert_eq!(row.calls, 512);
    assert_eq!(row.total_ns, 512 * 513 / 2);
    assert_eq!(row.self_ns, row.total_ns);
    assert!(row.max_ns <= 512);
}

#[test]
fn active_call_stack_subtracts_inclusive_child_duration() {
    let types = TypeTable::new();
    let vacuum = types.primitive(Primitive::Vacuum);
    let mut interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(function(
        MirFunctionId(0),
        None,
        Some(interner.intern("outer")),
        MirType::semantic(vacuum),
        None,
        vec![empty_block()],
    ));
    program.functions.push(function(
        MirFunctionId(1),
        None,
        Some(interner.intern("inner")),
        MirType::semantic(vacuum),
        None,
        vec![empty_block()],
    ));

    let mut collector = StatsCollector::new(&program, &context, &interner);
    collector.enter_function(MirFunctionId(0), true);
    collector.enter_function(MirFunctionId(1), true);
    collector.leave_function(MirFunctionId(1));
    collector.leave_function(MirFunctionId(0));
    let stats = collector.finish();
    let outer = stats
        .functions
        .iter()
        .find(|row| row.name == "outer")
        .expect("outer row");
    let inner = stats
        .functions
        .iter()
        .find(|row| row.name == "inner")
        .expect("inner row");
    assert_eq!(outer.calls, 1);
    assert_eq!(inner.calls, 1);
    assert_eq!(outer.total_ns, outer.self_ns + inner.total_ns);
}

#[test]
fn direct_conversion_runtime_call_counts_success_and_recovery() {
    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);
    let mut interner = Interner::new();
    let bad = interner.intern("not-a-number");
    let fallback = interner.intern("9");
    let context = MirValidationContext::new(&types);
    let conversion = |recovery| MirConversion {
        store: None,
        flavor: MirConversionFlavor::Runtime,
        source_ty: MirType::semantic(textus),
        target_ty: MirType::semantic(numerus),
        params: vec![],
        recovery,
        struct_defaults: vec![],
    };
    let mut program = MirProgram::new();
    program.functions.push(function(
        MirFunctionId(0),
        None,
        Some(interner.intern("convert_fixture")),
        MirType::semantic(vacuum),
        Some(MirType::semantic(textus)),
        vec![empty_block()],
    ));

    let statement = |conversion| MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: None,
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Convert(conversion),
                args: vec![MirOperand::Constant(MirConstant::String(bad))],
                return_ty: MirType::semantic(numerus),
            },
        },
        span: Span::default(),
    };

    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.stats = Some(StatsCollector::new(&program, &context, &interner));
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    assert!(matches!(
        runner.execute_statement(&statement(conversion(None))),
        Ok(StatementOutcome::FrameReturned)
    ));
    let failed = runner.take_stats().expect("stats");
    let convert = failed
        .intrinsics
        .iter()
        .find(|row| row.name == "convert")
        .expect("conversion row");
    assert_eq!(convert.calls, 1);
    assert_eq!(convert.self_ns, convert.total_ns);
    assert_eq!(failed.functions[0].calls, 1);

    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.stats = Some(StatsCollector::new(&program, &context, &interner));
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let recovery = Some(MirOperand::Constant(MirConstant::String(fallback)));
    assert!(matches!(
        runner.execute_statement(&statement(conversion(recovery))),
        Ok(StatementOutcome::Continue)
    ));
    runner.pop_frame();
    let recovered = runner.take_stats().expect("stats");
    assert_eq!(recovered.intrinsics[0].name, "convert");
    assert_eq!(recovered.intrinsics[0].calls, 1);
}

#[test]
fn direct_cursor_stream_path_records_generator_frame_and_attribution() {
    let types = TypeTable::new();
    let vacuum = types.primitive(Primitive::Vacuum);
    let numerus = types.primitive(Primitive::Numerus);
    let mut interner = Interner::new();
    let generator_name = interner.intern("numbers");
    let context = MirValidationContext::new(&types);
    let generator_block = MirBlock {
        id: MirBlockId(0),
        statements: vec![MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::Cede,
                    args: vec![MirOperand::Constant(MirConstant::Int(7))],
                    return_ty: MirType::semantic(vacuum),
                },
            },
            span: Span::default(),
        }],
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span: Span::default(),
        },
        span: Span::default(),
    };
    let mut program = MirProgram::new();
    program.functions.push(function(
        MirFunctionId(0),
        None,
        Some(interner.intern("outer")),
        MirType::semantic(vacuum),
        None,
        vec![empty_block()],
    ));
    program.functions.push(function(
        MirFunctionId(1),
        Some(DefId(77)),
        Some(generator_name),
        MirType::semantic(numerus),
        None,
        vec![generator_block],
    ));

    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.stats = Some(StatsCollector::new(&program, &context, &interner));
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let value = runner
        .eval_cursor_stream(
            &MirCursorStream {
                callee: DefId(77),
                stepped: false,
            },
            &[],
            Span::default(),
        )
        .expect("cursor stream");
    assert!(matches!(value, Value::Array(_)));
    runner.pop_frame();
    let stats = runner.take_stats().expect("stats");
    let generator = stats
        .functions
        .iter()
        .find(|row| row.name == "numbers")
        .expect("generator row");
    assert_eq!(generator.calls, 1);
    assert!(generator.total_ns >= generator.self_ns);
    assert!(
        stats
            .intrinsics
            .iter()
            .any(|row| row.name == "cede" && row.calls == 1)
    );
}

#[test]
fn unmetered_runner_keeps_stats_state_absent() {
    let types = TypeTable::new();
    let vacuum = types.primitive(Primitive::Vacuum);
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(function(
        MirFunctionId(0),
        None,
        None,
        MirType::semantic(vacuum),
        None,
        vec![empty_block()],
    ));
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    assert!(runner.stats.is_none());
    runner.pop_frame();
    assert!(runner.stats.is_none());
}

#[test]
fn a_function_absent_from_the_name_table_falls_back_to_its_root_slot() {
    let types = TypeTable::new();
    let vacuum = types.primitive(Primitive::Vacuum);
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut program = MirProgram::new();
    program.functions.push(function(
        MirFunctionId(0),
        None,
        None,
        MirType::semantic(vacuum),
        None,
        vec![empty_block()],
    ));

    let mut collector = StatsCollector::new(&program, &context, &interner);
    collector.record_function_sample(MirFunctionId(7), 5, 5);
    let stats = collector.finish();
    assert_eq!(stats.functions.len(), 1);
    assert_eq!(stats.functions[0].module, "root");
    assert_eq!(stats.functions[0].name, "f7");
    assert_eq!(stats.functions[0].calls, 1);
}
