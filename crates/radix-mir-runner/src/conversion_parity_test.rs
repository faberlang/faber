//! E3 runner parity for synthetic conversion-arm calls.

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

/// `↦` (Runtime) and `∷` (Cast) both cast with Rust `as` semantics at the
/// DECLARED width, matching the `as W` codegen emits for either glyph
/// (`300.0 ↦ numerus<i8>` and `300.0 ∷ numerus<i8>` are both `300.0 as i8`).
///
/// Pins the values want reported as silently wrong: the runner used to
/// saturate at the width-agnostic i64 carrier, so `300.0 ↦ numerus<i8>` printed
/// 300, `-1.9 ↦ numerus<u64>` printed -1, and a `∷` ascription executed the raw
/// float. FORK-2 (mail 2fb79900 / memo) keeps casts saturating while
/// integer arithmetic errors.
#[test]
fn numerus_casts_saturate_at_the_declared_width() {
    use crate::host::BufferHost;
    use radix_lexer::Interner;
    use radix_mir::{MirConstant, MirConversion, MirConversionFlavor, MirOperand, MirProgram};
    use radix_types::{NumericWidth, Primitive, TypeTable};

    let mut types = TypeTable::new();
    let fractus = types.primitive(Primitive::Fractus);
    let numerus = types.primitive(Primitive::Numerus);
    let i8_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::I8);
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

    let conversion =
        |flavor: MirConversionFlavor, source: radix_types::TypeId, target| MirConversion {
            store: None,
            flavor,
            source_ty: radix_mir::MirType::semantic(source),
            target_ty: radix_mir::MirType::semantic(target),
            params: Vec::new(),
            recovery: None,
            struct_defaults: Vec::new(),
        };
    let cast = |runner: &mut Runner<'_, BufferHost>, conversion: &MirConversion, value: f64| {
        runner
            .eval_convert(
                conversion,
                &[MirOperand::Constant(MirConstant::Float(value))],
                span(),
            )
            .expect("float to numerus cast")
    };

    // Sized signed target: saturate at the width, not at i64.
    let i8_cast = conversion(MirConversionFlavor::Cast, fractus, i8_ty);
    assert_eq!(cast(&mut runner, &i8_cast, 300.0), Value::Int(127));
    assert_eq!(cast(&mut runner, &i8_cast, -300.0), Value::Int(-128));
    let i8_widen = conversion(MirConversionFlavor::Runtime, fractus, i8_ty);
    assert_eq!(cast(&mut runner, &i8_widen, 300.0), Value::Int(127));

    // Unsigned target: negative floats saturate at zero, and the carrier is
    // unsigned so `u64::MAX` stays exact.
    let u64_cast = conversion(MirConversionFlavor::Cast, fractus, u64_ty);
    assert_eq!(cast(&mut runner, &u64_cast, -1.9), Value::UInt(0));
    assert_eq!(
        cast(&mut runner, &u64_cast, 18446744073709551615.0),
        Value::UInt(u64::MAX)
    );
    let u64_widen = conversion(MirConversionFlavor::Runtime, fractus, u64_ty);
    assert_eq!(cast(&mut runner, &u64_widen, -1.9), Value::UInt(0));

    // Bare numerus keeps the width-agnostic carrier, and NaN casts to zero.
    let i64_cast = conversion(MirConversionFlavor::Cast, fractus, numerus);
    assert_eq!(cast(&mut runner, &i64_cast, 1e300), Value::Int(i64::MAX));
    assert_eq!(cast(&mut runner, &i64_cast, f64::NAN), Value::Int(0));
}
