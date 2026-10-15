//! E3 runner parity for synthetic conversion-arm calls.
// Test fixtures: exact float expectations and small, known-in-range numeric casts.
#![allow(clippy::similar_names)]

use super::*;
use radix_mir::{MirFunction, MirLocal, MirParam, MirStatement, MirTemp, MirTerminator};
use radix_types::Type;

fn span() -> Span {
    Span::default()
}

fn function(
    id: u32,
    params: Vec<MirParam>,
    locals: Vec<MirLocal>,
    temps: Vec<MirTemp>,
    blocks: Vec<MirBlock>,
    return_ty: radix_mir::MirType,
    error_ty: Option<radix_mir::MirType>,
) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params,
        locals,
        temps,
        blocks,
        return_ty,
        error_ty,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: span(),
    }
}

fn block(id: u32, statements: Vec<MirStatement>, terminator: MirTerminatorKind) -> MirBlock {
    MirBlock {
        id: MirBlockId(id),
        statements,
        terminator: MirTerminator {
            kind: terminator,
            span: span(),
        },
        span: span(),
    }
}

#[test]
fn conversion_arm_call_runs_once_only_on_propagate_error_path() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{
        MirCallee, MirConstant, MirDiagnosticKind, MirIntrinsic, MirLocalId, MirOperand, MirPlace,
        MirRuntimeCall, MirStatementKind, MirTempId, MirTerminatorKind,
    };
    use radix_types::{Primitive, TypeTable};

    let mut types = TypeTable::new();
    let vacuum = radix_mir::MirType::semantic(types.primitive(Primitive::Vacuum));
    let source_def = radix_hir::DefId(10);
    let destination_def = radix_hir::DefId(20);
    let source_ty = types.intern(Type::Enum(source_def));
    let destination_ty = types.intern(Type::Enum(destination_def));
    let source_mir_ty = radix_mir::MirType::semantic(source_ty);
    let destination_mir_ty = radix_mir::MirType::semantic(destination_ty);
    let mut interner = Interner::new();
    let raw = interner.intern("raw-source-error");
    let converted = interner.intern("conversion-arm-ran");

    let source_failure = function(
        0,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![block(
            0,
            Vec::new(),
            MirTerminatorKind::ReturnError(MirOperand::Constant(MirConstant::String(raw))),
        )],
        vacuum,
        Some(source_mir_ty),
    );
    let source_success = function(
        1,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![block(0, Vec::new(), MirTerminatorKind::Return(None))],
        vacuum,
        Some(source_mir_ty),
    );
    let conversion_arm = function(
        2,
        vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: source_mir_ty,
            mode: MirParamMode::Owned,
            span: span(),
        }],
        vec![MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: source_mir_ty,
            mutable: false,
            span: span(),
        }],
        Vec::new(),
        vec![block(
            0,
            vec![MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: None,
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Nota),
                        args: vec![MirOperand::Constant(MirConstant::String(converted))],
                        return_ty: vacuum,
                    },
                },
                span: span(),
            }],
            MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(MirLocalId(0))))),
        )],
        destination_mir_ty,
        None,
    );

    let propagate = |id: u32, callee: MirFunctionId| {
        function(
            id,
            Vec::new(),
            Vec::new(),
            vec![
                MirTemp {
                    id: MirTempId(0),
                    ty: source_mir_ty,
                    span: span(),
                },
                MirTemp {
                    id: MirTempId(1),
                    ty: destination_mir_ty,
                    span: span(),
                },
            ],
            vec![
                block(
                    0,
                    Vec::new(),
                    MirTerminatorKind::TryCall {
                        destination: None,
                        callee: MirCallee::Function(callee),
                        args: Vec::new(),
                        ok_block: MirBlockId(1),
                        error_place: MirPlace::temp(MirTempId(0)),
                        error_block: MirBlockId(2),
                    },
                ),
                block(1, Vec::new(), MirTerminatorKind::Return(None)),
                block(
                    2,
                    vec![MirStatement {
                        kind: MirStatementKind::Call {
                            destination: Some(MirPlace::temp(MirTempId(1))),
                            callee: MirCallee::Function(MirFunctionId(2)),
                            args: vec![MirOperand::Temp(MirTempId(0))],
                        },
                        span: span(),
                    }],
                    MirTerminatorKind::ReturnError(MirOperand::Temp(MirTempId(1))),
                ),
            ],
            vacuum,
            Some(destination_mir_ty),
        )
    };

    let program = MirProgram {
        functions: vec![
            source_failure,
            source_success,
            conversion_arm,
            propagate(3, MirFunctionId(0)),
            propagate(4, MirFunctionId(1)),
        ],
    };
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);

    runner.push_frame(MirFunctionId(3), MirBlockId(0));
    runner.run_until_depth(0);
    assert!(
        runner.errors.is_empty(),
        "failure propagation should run: {:?}",
        runner.errors
    );
    assert_eq!(
        runner.return_error,
        Some(Value::Str("raw-source-error".to_owned()))
    );

    runner.push_frame(MirFunctionId(4), MirBlockId(0));
    runner.run_until_depth(0);
    assert!(
        runner.errors.is_empty(),
        "successful call should run: {:?}",
        runner.errors
    );
    assert_eq!(runner.return_value, Some(Value::Unit));
    assert_eq!(host.stdout_lines, ["conversion-arm-ran"]);
}

/// D1.11: `∷` states only what the compiler can prove, so a float-to-integer
/// ascription is a compile error (`SEM016`) and the runner's `Cast` arm carries
/// no float path: an ascription cast of a float is an internal error, never a
/// saturating conversion. `bivalens ∷ numerus` still casts to 0/1.
#[test]
fn numerus_ascription_cast_has_no_float_path() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{MirConstant, MirConversion, MirConversionFlavor, MirOperand, MirProgram};
    use radix_types::{NumericWidth, Primitive, TypeTable};

    let mut types = TypeTable::new();
    let fractus = types.primitive(Primitive::Fractus);
    let bivalens = types.primitive(Primitive::Bivalens);
    let numerus = types.primitive(Primitive::Numerus);
    let i8_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::I8);

    let program = MirProgram {
        functions: vec![function(
            0,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![block(0, Vec::new(), MirTerminatorKind::Return(None))],
            radix_mir::MirType::semantic(numerus),
            None,
        )],
    };
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));

    let cast = |source, target| MirConversion {
        store: None,
        flavor: MirConversionFlavor::Cast,
        source_ty: radix_mir::MirType::semantic(source),
        target_ty: radix_mir::MirType::semantic(target),
        params: Vec::new(),
        recovery: None,
        struct_defaults: Vec::new(),
    };
    let float_cast = cast(fractus, i8_ty);
    assert!(
        runner
            .eval_convert(
                &float_cast,
                &[MirOperand::Constant(MirConstant::Float(300.0))],
                span(),
            )
            .is_err(),
        "a float ascription cast must not saturate"
    );
    let bool_cast = cast(bivalens, numerus);
    assert_eq!(
        runner
            .eval_convert(
                &bool_cast,
                &[MirOperand::Constant(MirConstant::Bool(true))],
                span(),
            )
            .expect("bool to numerus cast"),
        Value::Int(1)
    );
}

/// D1.11 / D11.8: `fractus ↦ numerus<W>` truncates toward zero but is a check:
/// a truncation outside `W`, NaN, or an infinity fails (or takes the `⊥`
/// default) instead of saturating.
#[test]
fn float_to_numerus_runtime_conversion_is_a_check() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{MirConstant, MirConversion, MirConversionFlavor, MirOperand, MirProgram};
    use radix_types::{NumericWidth, Primitive, TypeTable};

    let mut types = TypeTable::new();
    let fractus = types.primitive(Primitive::Fractus);
    let numerus = types.primitive(Primitive::Numerus);
    let u8_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let u64_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U64);

    let program = MirProgram {
        functions: vec![function(
            0,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![block(0, Vec::new(), MirTerminatorKind::Return(None))],
            radix_mir::MirType::semantic(numerus),
            None,
        )],
    };
    let interner = Interner::new();
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));

    let conversion = |target, recovery| MirConversion {
        store: None,
        flavor: MirConversionFlavor::Runtime,
        source_ty: radix_mir::MirType::semantic(fractus),
        target_ty: radix_mir::MirType::semantic(target),
        params: Vec::new(),
        recovery,
        struct_defaults: Vec::new(),
    };
    let run = |runner: &mut Runner<'_, BufferHost>, conversion: &MirConversion, value: f64| {
        runner.eval_convert(
            conversion,
            &[MirOperand::Constant(MirConstant::Float(value))],
            span(),
        )
    };

    let to_u8 = conversion(u8_ty, None);
    let to_i64 = conversion(numerus, None);
    let to_u64 = conversion(u64_ty, None);
    // The fraction truncates toward zero.
    assert_eq!(run(&mut runner, &to_u8, 3.7).unwrap(), Value::UInt(3));
    assert_eq!(run(&mut runner, &to_i64, -3.7).unwrap(), Value::Int(-3));
    assert_eq!(run(&mut runner, &to_u8, -0.9).unwrap(), Value::UInt(0));
    assert_eq!(run(&mut runner, &to_u8, 255.9).unwrap(), Value::UInt(255));
    assert_eq!(
        run(&mut runner, &to_u64, 9.3e18).unwrap(),
        Value::UInt(9_300_000_000_000_000_000)
    );
    // Out of range, negative into unsigned, NaN and infinities fail.
    for bad in [
        300.0,
        -1.0,
        1e30,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        assert!(run(&mut runner, &to_u8, bad).is_err(), "{bad} into u8");
    }
    assert!(run(&mut runner, &to_i64, 1e30).is_err());
    assert!(run(&mut runner, &to_i64, f64::NAN).is_err());
    assert!(run(&mut runner, &to_u64, 1.9e19).is_err());
    // `⊥` supplies the default when the check fails.
    let recovering = conversion(u8_ty, Some(MirOperand::Constant(MirConstant::Int(7))));
    assert_eq!(run(&mut runner, &recovering, 300.0).unwrap(), Value::Int(7));
    assert_eq!(
        run(&mut runner, &recovering, f64::NAN).unwrap(),
        Value::Int(7)
    );
    assert_eq!(run(&mut runner, &recovering, 3.7).unwrap(), Value::UInt(3));
}

/// Run `lista<numerus> ↦ lista<numerus, capacity>` over `items`, with an
/// optional `⊥` recovery constant.
fn run_bounded_lista_conversio(
    capacity: u64,
    items: &[i64],
    recovery: Option<radix_mir::MirOperand>,
) -> Result<Value, RunnerError> {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{MirConversion, MirConversionFlavor, MirProgram, MirValidationContext};
    use radix_types::{IndexExpr, Primitive, TypeTable};
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut types = TypeTable::new();
    let element = types.primitive(Primitive::Numerus);
    let source_ty = types.intern(Type::Array(element));
    let capacity = types.intern_index(IndexExpr::Literal(capacity));
    let target_ty = types.intern(Type::BoundedArray { element, capacity });
    let interner = Interner::new();
    let program = MirProgram {
        functions: vec![function(
            0,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            radix_mir::MirType::semantic(target_ty),
            None,
        )],
    };
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let conversion = MirConversion {
        store: None,
        flavor: MirConversionFlavor::Runtime,
        source_ty: radix_mir::MirType::semantic(source_ty),
        target_ty: radix_mir::MirType::semantic(target_ty),
        params: Vec::new(),
        recovery,
        struct_defaults: Vec::new(),
    };
    let source = Value::Array(Rc::new(RefCell::new(
        items.iter().copied().map(Value::Int).collect(),
    )));
    runner.eval_runtime_convert(&conversion, source, span())
}

fn int_array(items: &[i64]) -> Value {
    Value::Array(std::rc::Rc::new(std::cell::RefCell::new(
        items.iter().copied().map(Value::Int).collect(),
    )))
}

/// A list shorter than the capacity converts to the same elements.
#[test]
fn bounded_lista_conversio_fits() {
    let result = run_bounded_lista_conversio(4, &[1, 2, 3], None).expect("fits capacity 4");
    assert_eq!(result, int_array(&[1, 2, 3]));
}

/// A list exactly at the capacity converts.
#[test]
fn bounded_lista_conversio_exact() {
    let result = run_bounded_lista_conversio(3, &[1, 2, 3], None).expect("fits capacity 3");
    assert_eq!(result, int_array(&[1, 2, 3]));
}

/// A list over the capacity takes the `⊥` recovery value.
#[test]
fn bounded_lista_conversio_overflow_takes_recovery() {
    use radix_mir::{MirConstant, MirOperand};

    let recovery = MirOperand::Constant(MirConstant::Int(7));
    let result = run_bounded_lista_conversio(2, &[1, 2, 3], Some(recovery))
        .expect("overflow recovers with the ⊥ value");
    assert_eq!(result, Value::Int(7));
}

/// A list over the capacity with no `⊥` fails the conversion; it is a
/// conversion failure, not an unsupported target.
#[test]
fn bounded_lista_conversio_overflow_without_recovery_errs() {
    let error = run_bounded_lista_conversio(2, &[1, 2, 3], None)
        .expect_err("overflow without recovery fails the conversion");
    assert_ne!(
        error.issue,
        "runner_faberscript_unsupported_runtime_conversio_to_bounded_lista"
    );
    assert_eq!(error.code, crate::RUNNER_CODE_CONVERSION);
}

/// Run `ascii<8> ↦ ascii<capacity>` over `text`, with an optional `⊥`
/// recovery constant.
fn run_bounded_ascii_resize(
    capacity: u64,
    text: &str,
    recovery: Option<radix_mir::MirOperand>,
) -> Result<Value, RunnerError> {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{MirConversion, MirConversionFlavor, MirProgram, MirValidationContext};
    use radix_types::{IndexExpr, TypeTable};

    let mut types = TypeTable::new();
    let source_capacity = types.intern_index(IndexExpr::Literal(8));
    let source_ty = types.intern(Type::BoundedAscii {
        capacity: source_capacity,
    });
    let target_capacity = types.intern_index(IndexExpr::Literal(capacity));
    let target_ty = types.intern(Type::BoundedAscii {
        capacity: target_capacity,
    });
    let interner = Interner::new();
    let program = MirProgram {
        functions: vec![function(
            0,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            radix_mir::MirType::semantic(target_ty),
            None,
        )],
    };
    let context = MirValidationContext::new(&types);
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    let conversion = MirConversion {
        store: None,
        flavor: MirConversionFlavor::Runtime,
        source_ty: radix_mir::MirType::semantic(source_ty),
        target_ty: radix_mir::MirType::semantic(target_ty),
        params: Vec::new(),
        recovery,
        struct_defaults: Vec::new(),
    };
    runner.eval_runtime_convert(&conversion, Value::Str(text.to_string()), span())
}

/// A bounded ascii grows into a larger capacity with the same text.
#[test]
fn bounded_ascii_resize_grows() {
    let result = run_bounded_ascii_resize(8, "ab", None).expect("fits capacity 8");
    assert_eq!(result, Value::Str("ab".to_string()));
}

/// A source over the target capacity takes the `⊥` recovery value.
#[test]
fn bounded_ascii_resize_over_capacity_recovers() {
    use radix_mir::{MirConstant, MirOperand};

    let recovery = MirOperand::Constant(MirConstant::Int(7));
    let result = run_bounded_ascii_resize(4, "abcdef", Some(recovery))
        .expect("overflow recovers with the ⊥ value");
    assert_eq!(result, Value::Int(7));
}

/// A source over the target capacity with no `⊥` fails the conversion; it is
/// a conversion failure, not an unsupported target.
#[test]
fn bounded_ascii_resize_over_capacity_fails_without_recovery() {
    let error = run_bounded_ascii_resize(4, "abcdef", None)
        .expect_err("overflow without recovery fails the conversion");
    assert_eq!(error.code, crate::RUNNER_CODE_CONVERSION);
    assert_ne!(
        error.issue,
        "runner_faberscript_unsupported_runtime_conversio_to_bounded_ascii"
    );
}
