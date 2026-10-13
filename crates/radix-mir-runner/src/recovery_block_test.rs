//! Recovery-region execution for `⊥` conversion fallbacks.
//!
//! A side-effecting recovery lowers into a real CFG region the success path
//! skips: the region's tail stores the fallback into the conversion's
//! destination temp and joins the main flow. At the failure point the runner
//! executes the region through the normal statement executor; the orphan-call
//! reconstruction this replaces (BUG-AA50C86A) could only replay the root
//! call, so nested calls died on an uninitialized temp and non-call roots
//! ran eagerly on success.
// Test fixtures: exact float expectations and small, known-in-range numeric casts.
#![allow(clippy::trivially_copy_pass_by_ref)]

use super::*;
use crate::host::BufferHost;
use radix_lexer::{Interner, Symbol};
use radix_mir::{
    MirBinOp, MirBlock, MirCallee, MirConstant, MirConversion, MirConversionFlavor,
    MirDiagnosticKind, MirFunction, MirIntrinsic, MirLocal, MirParam, MirParamMode, MirPlace,
    MirProgram, MirRuntimeCall, MirStatement, MirStatementKind, MirTemp, MirTempId, MirTerminator,
    MirTerminatorKind, MirValidationContext, MirValue, MirValueId, MirValueKind,
};
use radix_types::{Primitive, TypeTable};

fn span() -> Span {
    Span::default()
}

fn int_ty(types: &TypeTable) -> radix_mir::MirType {
    radix_mir::MirType::semantic(types.primitive(Primitive::Numerus))
}

fn text_ty(types: &TypeTable) -> radix_mir::MirType {
    radix_mir::MirType::semantic(types.primitive(Primitive::Textus))
}

fn void_ty(types: &TypeTable) -> radix_mir::MirType {
    radix_mir::MirType::semantic(types.primitive(Primitive::Vacuum))
}

fn function(
    id: u32,
    params: Vec<MirParam>,
    locals: Vec<MirLocal>,
    temps: Vec<MirTemp>,
    blocks: Vec<MirBlock>,
    return_ty: radix_mir::MirType,
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
        error_ty: None,
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

fn temp(id: u32, ty: radix_mir::MirType) -> MirTemp {
    MirTemp {
        id: MirTempId(id),
        ty,
        span: span(),
    }
}

fn print(text: &Symbol, types: &TypeTable) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: None,
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Nota),
                args: vec![MirOperand::Constant(MirConstant::String(*text))],
                return_ty: void_ty(types),
            },
        },
        span: span(),
    }
}

fn assign(
    temp_id: MirTempId,
    kind: MirValueKind,
    ty: radix_mir::MirType,
    value_id: u32,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Assign {
            place: MirPlace::temp(temp_id),
            value: MirValue {
                id: MirValueId(value_id),
                kind,
                ty,
                span: span(),
            },
        },
        span: span(),
    }
}

fn call(
    destination: MirTempId,
    callee: u32,
    args: Vec<MirOperand>,
    _ty: radix_mir::MirType,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Call {
            destination: Some(MirPlace::temp(destination)),
            callee: MirCallee::Function(MirFunctionId(callee)),
            args,
        },
        span: span(),
    }
}

fn convert(
    destination: MirTempId,
    source: MirOperand,
    recovery: Option<MirOperand>,
    types: &TypeTable,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::temp(destination)),
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Convert(MirConversion {
                    store: None,
                    flavor: MirConversionFlavor::Runtime,
                    source_ty: text_ty(types),
                    target_ty: int_ty(types),
                    params: Vec::new(),
                    recovery,
                    struct_defaults: Vec::new(),
                }),
                args: vec![source],
                return_ty: int_ty(types),
            },
        },
        span: span(),
    }
}

fn goto(target: u32) -> MirTerminatorKind {
    MirTerminatorKind::Goto(MirBlockId(target))
}

/// `fb` prints "fb ran" and returns 5; `id` prints "id ran" and returns its
/// argument + 100 — the vrf-vrp witness functions.
fn witness_callees(types: &TypeTable, interner: &mut Interner) -> (MirFunction, MirFunction) {
    let fb_ran = interner.intern("fb ran");
    let id_ran = interner.intern("id ran");
    let fb = function(
        0,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![block(
            0,
            vec![print(&fb_ran, types)],
            MirTerminatorKind::Return(Some(MirOperand::Constant(MirConstant::Int(5)))),
        )],
        int_ty(types),
    );
    let id = function(
        1,
        vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: int_ty(types),
            mode: MirParamMode::Owned,
            span: span(),
        }],
        Vec::new(),
        vec![temp(0, int_ty(types))],
        vec![block(
            0,
            vec![
                print(&id_ran, types),
                assign(
                    MirTempId(0),
                    MirValueKind::Binary {
                        op: MirBinOp::Add,
                        lhs: MirOperand::Place(MirPlace::local(MirLocalId(0))),
                        rhs: MirOperand::Constant(MirConstant::Int(100)),
                    },
                    int_ty(types),
                    0,
                ),
            ],
            MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(0)))),
        )],
        int_ty(types),
    );
    (fb, id)
}

fn source_constant(interner: &mut Interner, text: &str) -> MirOperand {
    MirOperand::Constant(MirConstant::String(interner.intern(text)))
}

fn run_main(
    functions: Vec<MirFunction>,
    main_id: u32,
    types: &TypeTable,
    interner: &Interner,
) -> (Option<Value>, Vec<String>, Vec<(&'static str, String)>) {
    let context = MirValidationContext::new(types);
    let mut host = BufferHost::default();
    let program = MirProgram { functions };
    let mut runner = Runner::new(&program, &context, types, interner, &mut host);
    runner.push_frame(MirFunctionId(main_id), MirBlockId(0));
    runner.run_until_depth(0);
    let value = runner.return_value.clone();
    let errors = runner
        .errors
        .iter()
        .map(|error| (error.code, error.issue.clone()))
        .collect();
    let stdout = host.stdout_lines.clone();
    (value, stdout, errors)
}

/// Case 1 (BUG-AA50C86A): nested `⊥ id(fb())` runs the whole region on the
/// failure path — the runner must not replay only the root call (the old
/// orphan-call reconstruction died with `uninitialized temp` on `id`'s
/// argument) — and must not run it on the success path.
#[test]
fn recovery_region_runs_nested_call_fallback_only_on_failure() {
    let types = TypeTable::new();
    let mut interner = Interner::new();
    let (fb, id) = witness_callees(&types, &mut interner);

    let main = |source: MirOperand| {
        function(
            2,
            Vec::new(),
            vec![MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: int_ty(&types),
                mutable: true,
                span: span(),
            }],
            vec![
                temp(0, int_ty(&types)),
                temp(1, int_ty(&types)),
                temp(2, int_ty(&types)),
            ],
            vec![
                block(
                    0,
                    vec![convert(
                        MirTempId(2),
                        source,
                        Some(MirOperand::Temp(MirTempId(2))),
                        &types,
                    )],
                    goto(2),
                ),
                // Recovery region: %0 = fb(); %1 = id(%0); %2 = %1; goto join.
                block(
                    1,
                    vec![
                        call(MirTempId(0), 0, Vec::new(), int_ty(&types)),
                        call(
                            MirTempId(1),
                            1,
                            vec![MirOperand::Temp(MirTempId(0))],
                            int_ty(&types),
                        ),
                        assign(
                            MirTempId(2),
                            MirValueKind::Operand(MirOperand::Temp(MirTempId(1))),
                            int_ty(&types),
                            1,
                        ),
                    ],
                    goto(2),
                ),
                block(
                    2,
                    Vec::new(),
                    MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(2)))),
                ),
            ],
            int_ty(&types),
        )
    };

    let functions = vec![
        fb.clone(),
        id.clone(),
        main(source_constant(&mut interner, "42")),
    ];
    let (value, stdout, errors) = run_main(functions, 2, &types, &interner);
    assert!(errors.is_empty(), "success path must be clean: {errors:?}");
    assert_eq!(value, Some(Value::Int(42)));
    assert!(
        stdout.is_empty(),
        "fallback must not run on success: {stdout:?}"
    );

    let functions = vec![fb, id, main(source_constant(&mut interner, "zz"))];
    let (value, stdout, errors) = run_main(functions, 2, &types, &interner);
    assert!(
        errors.is_empty(),
        "failure path must run the nested fallback, not fail on an uninitialized temp: {errors:?}"
    );
    assert_eq!(value, Some(Value::Int(105)));
    assert_eq!(stdout, ["fb ran", "id ran"]);
}

/// Case 2: a non-call-root fallback (`⊥ (fb() + 1)`) is side-effecting too —
/// it must run only on the failure path, never eagerly on success (the old
/// lowering evaluated it on every path).
#[test]
fn recovery_region_runs_non_call_root_fallback_only_on_failure() {
    let types = TypeTable::new();
    let mut interner = Interner::new();
    let (fb, _) = witness_callees(&types, &mut interner);

    let main = |source: MirOperand| {
        function(
            2,
            Vec::new(),
            Vec::new(),
            vec![
                temp(0, int_ty(&types)),
                temp(1, int_ty(&types)),
                temp(2, int_ty(&types)),
            ],
            vec![
                block(
                    0,
                    vec![convert(
                        MirTempId(2),
                        source,
                        Some(MirOperand::Temp(MirTempId(2))),
                        &types,
                    )],
                    goto(2),
                ),
                // Recovery region: %0 = fb(); %1 = %0 + 1; %2 = %1; goto join.
                block(
                    1,
                    vec![
                        call(MirTempId(0), 0, Vec::new(), int_ty(&types)),
                        assign(
                            MirTempId(1),
                            MirValueKind::Binary {
                                op: MirBinOp::Add,
                                lhs: MirOperand::Temp(MirTempId(0)),
                                rhs: MirOperand::Constant(MirConstant::Int(1)),
                            },
                            int_ty(&types),
                            0,
                        ),
                        assign(
                            MirTempId(2),
                            MirValueKind::Operand(MirOperand::Temp(MirTempId(1))),
                            int_ty(&types),
                            1,
                        ),
                    ],
                    goto(2),
                ),
                block(
                    2,
                    Vec::new(),
                    MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(2)))),
                ),
            ],
            int_ty(&types),
        )
    };

    let functions = vec![fb.clone(), main(source_constant(&mut interner, "42"))];
    let (value, stdout, errors) = run_main(functions, 2, &types, &interner);
    assert!(errors.is_empty(), "success path must be clean: {errors:?}");
    assert_eq!(value, Some(Value::Int(42)));
    assert!(
        stdout.is_empty(),
        "fallback must not run on success: {stdout:?}"
    );

    let functions = vec![fb, main(source_constant(&mut interner, "zz"))];
    let (value, stdout, errors) = run_main(functions, 2, &types, &interner);
    assert!(errors.is_empty(), "failure path must be clean: {errors:?}");
    assert_eq!(value, Some(Value::Int(6)));
    assert_eq!(stdout, ["fb ran"]);
}

/// Case 2 (conditional root): `⊥ (c ✓ fb() ✗ 0)` spans a multi-block region
/// (branch, arms, tail). The runner follows the region's internal control
/// flow from its entry and stops at the main-reachable join.
#[test]
fn recovery_region_follows_internal_branches_and_stops_at_join() {
    let types = TypeTable::new();
    let mut interner = Interner::new();
    let (fb, _) = witness_callees(&types, &mut interner);

    let main = |source: MirOperand, condition: bool| {
        function(
            2,
            Vec::new(),
            Vec::new(),
            vec![
                temp(0, int_ty(&types)),
                temp(1, int_ty(&types)),
                temp(2, int_ty(&types)),
            ],
            vec![
                block(
                    0,
                    vec![convert(
                        MirTempId(2),
                        source,
                        Some(MirOperand::Temp(MirTempId(2))),
                        &types,
                    )],
                    goto(5),
                ),
                // Region entry: %c = const; branch bb2/bb3.
                block(
                    1,
                    vec![assign(
                        MirTempId(1),
                        MirValueKind::Operand(MirOperand::Constant(MirConstant::Bool(condition))),
                        radix_mir::MirType::semantic(types.primitive(Primitive::Bivalens)),
                        0,
                    )],
                    MirTerminatorKind::Branch {
                        condition: MirOperand::Temp(MirTempId(1)),
                        then_block: MirBlockId(2),
                        else_block: MirBlockId(3),
                    },
                ),
                block(
                    2,
                    vec![call(MirTempId(0), 0, Vec::new(), int_ty(&types))],
                    goto(4),
                ),
                block(
                    3,
                    vec![assign(
                        MirTempId(0),
                        MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(0))),
                        int_ty(&types),
                        2,
                    )],
                    goto(4),
                ),
                // Region tail: %2 = %0 (the taken arm's value); goto join.
                block(
                    4,
                    vec![assign(
                        MirTempId(2),
                        MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
                        int_ty(&types),
                        1,
                    )],
                    goto(5),
                ),
                block(
                    5,
                    Vec::new(),
                    MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(2)))),
                ),
            ],
            int_ty(&types),
        )
    };

    // Success parse: the fallback (and its call) must not run at all.
    let functions = vec![fb.clone(), main(source_constant(&mut interner, "42"), true)];
    let (value, stdout, errors) = run_main(functions, 2, &types, &interner);
    assert!(errors.is_empty(), "success path must be clean: {errors:?}");
    assert_eq!(value, Some(Value::Int(42)));
    assert!(
        stdout.is_empty(),
        "fallback must not run on success: {stdout:?}"
    );

    // Failed parse with a true condition: the then arm runs.
    let functions = vec![fb.clone(), main(source_constant(&mut interner, "zz"), true)];
    let (value, stdout, errors) = run_main(functions, 2, &types, &interner);
    assert!(errors.is_empty(), "failure path must be clean: {errors:?}");
    assert_eq!(value, Some(Value::Int(5)));
    assert_eq!(stdout, ["fb ran"]);

    // Failed parse with a false condition: the else arm runs, not the call.
    let functions = vec![fb, main(source_constant(&mut interner, "zz"), false)];
    let (value, stdout, errors) = run_main(functions, 2, &types, &interner);
    assert!(errors.is_empty(), "failure path must be clean: {errors:?}");
    assert_eq!(value, Some(Value::Int(0)));
    assert!(
        stdout.is_empty(),
        "the untaken arm must not run: {stdout:?}"
    );
}

/// Pure recovery operands stay eagerly lowered: the conversion's recovery
/// operand is a main-flow temp, and evaluation reads it without any region
/// walk (success path byte-identical to the pre-region lowering).
#[test]
fn pure_recovery_operand_evaluates_without_a_region() {
    let types = TypeTable::new();
    let mut interner = Interner::new();

    let main = |source: MirOperand| {
        function(
            0,
            Vec::new(),
            Vec::new(),
            vec![temp(0, int_ty(&types)), temp(1, int_ty(&types))],
            vec![
                block(
                    0,
                    vec![
                        assign(
                            MirTempId(0),
                            MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(7))),
                            int_ty(&types),
                            0,
                        ),
                        convert(
                            MirTempId(1),
                            source,
                            Some(MirOperand::Temp(MirTempId(0))),
                            &types,
                        ),
                    ],
                    goto(1),
                ),
                block(
                    1,
                    Vec::new(),
                    MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(1)))),
                ),
            ],
            int_ty(&types),
        )
    };

    let functions = vec![main(source_constant(&mut interner, "42"))];
    let (value, _, errors) = run_main(functions, 0, &types, &interner);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(value, Some(Value::Int(42)));

    let functions = vec![main(source_constant(&mut interner, "zz"))];
    let (value, _, errors) = run_main(functions, 0, &types, &interner);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(value, Some(Value::Int(7)));
}

/// A `convert` statement with an explicit target type; the shared `convert`
/// helper targets `numerus`.
fn convert_to(
    destination: MirTempId,
    source: MirOperand,
    target: radix_mir::MirType,
    recovery: Option<MirOperand>,
    types: &TypeTable,
) -> MirStatement {
    let mut statement = convert(destination, source, recovery, types);
    let MirStatementKind::RuntimeCall { call, .. } = &mut statement.kind else {
        unreachable!("convert builds a runtime call");
    };
    let MirIntrinsic::Convert(conversion) = &mut call.intrinsic else {
        unreachable!("convert builds a conversion");
    };
    conversion.target_ty = target;
    call.return_ty = target;
    statement
}

/// A scalar-truthiness `bivalens` conversion whose `⊥` fallback is a deferred
/// call region must run the region on the falsy path, not read the region's
/// destination temp before it was written (`uninitialized temp`).
#[test]
fn bivalens_truthiness_fallback_runs_deferred_call_region() {
    let types = TypeTable::new();
    let mut interner = Interner::new();
    let fb_ran = interner.intern("fb ran");
    let bool_ty = radix_mir::MirType::semantic(types.primitive(Primitive::Bivalens));
    let fb = function(
        0,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![block(
            0,
            vec![print(&fb_ran, &types)],
            MirTerminatorKind::Return(Some(MirOperand::Constant(MirConstant::Bool(true)))),
        )],
        bool_ty,
    );
    let main = function(
        1,
        Vec::new(),
        Vec::new(),
        vec![temp(0, bool_ty), temp(1, bool_ty)],
        vec![
            block(
                0,
                vec![convert_to(
                    MirTempId(1),
                    source_constant(&mut interner, ""),
                    bool_ty,
                    Some(MirOperand::Temp(MirTempId(1))),
                    &types,
                )],
                goto(2),
            ),
            // Recovery region: %0 = fb(); %1 = %0; goto join.
            block(
                1,
                vec![
                    call(MirTempId(0), 0, Vec::new(), bool_ty),
                    assign(
                        MirTempId(1),
                        MirValueKind::Operand(MirOperand::Temp(MirTempId(0))),
                        bool_ty,
                        0,
                    ),
                ],
                goto(2),
            ),
            block(
                2,
                Vec::new(),
                MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(1)))),
            ),
        ],
        bool_ty,
    );

    let (value, stdout, errors) = run_main(vec![fb, main], 1, &types, &interner);
    assert!(
        errors.is_empty(),
        "falsy path must run the deferred fallback: {errors:?}"
    );
    assert_eq!(value, Some(Value::Bool(true)));
    assert_eq!(stdout, ["fb ran"]);
}

/// A statement inside the recovery region that fails and leaves the frame
/// (the function has a `textus` alternate exit) ends the walk: the rest of the
/// region must not run against the caller's frame, and the failure reaches the
/// caller's error path.
#[test]
fn recovery_region_stops_when_a_region_statement_leaves_the_frame() {
    let types = TypeTable::new();
    let mut interner = Interner::new();
    let (fb, _) = witness_callees(&types, &mut interner);
    let text = text_ty(&types);

    // bad(): %2 = "zz" ↦ numerus ⊥ region; region: %3 = "zz" ↦ numerus (fails,
    // leaves the frame); %0 = fb(); %2 = %3 + %0; goto join.
    let mut bad = function(
        1,
        Vec::new(),
        Vec::new(),
        vec![
            temp(0, int_ty(&types)),
            temp(1, int_ty(&types)),
            temp(2, int_ty(&types)),
            temp(3, int_ty(&types)),
        ],
        vec![
            block(
                0,
                vec![convert(
                    MirTempId(2),
                    source_constant(&mut interner, "zz"),
                    Some(MirOperand::Temp(MirTempId(2))),
                    &types,
                )],
                goto(2),
            ),
            block(
                1,
                vec![
                    convert(
                        MirTempId(3),
                        source_constant(&mut interner, "zz"),
                        None,
                        &types,
                    ),
                    call(MirTempId(0), 0, Vec::new(), int_ty(&types)),
                    assign(
                        MirTempId(2),
                        MirValueKind::Binary {
                            op: MirBinOp::Add,
                            lhs: MirOperand::Temp(MirTempId(3)),
                            rhs: MirOperand::Temp(MirTempId(0)),
                        },
                        int_ty(&types),
                        0,
                    ),
                ],
                goto(2),
            ),
            block(
                2,
                Vec::new(),
                MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(2)))),
            ),
        ],
        int_ty(&types),
    );
    bad.error_ty = Some(text);

    // main(): try bad() — the error path returns 99, the ok path 1.
    let main = function(
        2,
        Vec::new(),
        Vec::new(),
        vec![temp(0, int_ty(&types)), temp(1, text)],
        vec![
            block(
                0,
                Vec::new(),
                MirTerminatorKind::TryCall {
                    destination: Some(MirPlace::temp(MirTempId(0))),
                    callee: MirCallee::Function(MirFunctionId(1)),
                    args: Vec::new(),
                    ok_block: MirBlockId(1),
                    error_place: MirPlace::temp(MirTempId(1)),
                    error_block: MirBlockId(2),
                },
            ),
            block(
                1,
                Vec::new(),
                MirTerminatorKind::Return(Some(MirOperand::Constant(MirConstant::Int(1)))),
            ),
            block(
                2,
                Vec::new(),
                MirTerminatorKind::Return(Some(MirOperand::Constant(MirConstant::Int(99)))),
            ),
        ],
        int_ty(&types),
    );

    let (value, stdout, errors) = run_main(vec![fb, bad, main], 2, &types, &interner);
    assert!(
        errors.is_empty(),
        "a leaving region statement must not keep running in the caller's frame: {errors:?}"
    );
    assert_eq!(value, Some(Value::Int(99)));
    assert!(
        stdout.is_empty(),
        "statements after the failing one must not run: {stdout:?}"
    );
}
