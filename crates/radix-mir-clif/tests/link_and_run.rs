//! C1/C3 proof: objects emitted from hand-built MIR link with the runtime
//! archive via `cc` and run (goal `radix-mir-clif`, units C1 and C3).
//!
//! The archive is `runtime/clif`'s staticlib. This crate cannot name it as a
//! dev-dependency (compiler crates never depend on `runtime/*`), and a plain
//! `cargo test` links only the rlib, so the test builds the archive itself with
//! the same `cargo` into the profile directory it is running from. A failed
//! build fails the test loudly rather than skipping.

use std::path::PathBuf;
use std::process::{Command, Output};

use radix_lexer::{Interner, Span};
use radix_mir::*;
use radix_types::{Primitive, TypeTable};

/// Build `faber-host-clif` into the profile directory this test executable
/// lives in (`<target>/<profile>/deps/<exe>`) and return the archive path.
fn build_archive() -> PathBuf {
    let exe = std::env::current_exe().expect("test executable path");
    let profile_dir = exe
        .parent()
        .and_then(|deps| deps.parent())
        .expect("test executable sits in <target>/<profile>/deps")
        .to_path_buf();
    let target_dir = profile_dir
        .parent()
        .expect("profile directory sits inside the target directory");
    let profile = profile_dir
        .file_name()
        .and_then(|name| name.to_str())
        .expect("profile directory name");

    let mut build = Command::new(env!("CARGO"));
    build
        .args(["build", "--locked", "-p", "faber-host-clif"])
        .arg("--target-dir")
        .arg(target_dir)
        .current_dir(env!("CARGO_MANIFEST_DIR"));
    if profile != "debug" {
        build.args(["--profile", profile]);
    }
    let built = build.output().expect("run cargo build -p faber-host-clif");
    assert!(
        built.status.success(),
        "cargo build -p faber-host-clif failed: {}{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr),
    );

    let archive = profile_dir.join("libfaber_rt_clif.a");
    assert!(
        archive.exists(),
        "cargo build did not produce {}",
        archive.display()
    );
    archive
}

fn span() -> Span {
    Span::default()
}

fn statement(kind: MirStatementKind) -> MirStatement {
    MirStatement { kind, span: span() }
}

fn terminator(kind: MirTerminatorKind) -> MirTerminator {
    MirTerminator { kind, span: span() }
}

fn block(id: u32, statements: Vec<MirStatement>, kind: MirTerminatorKind) -> MirBlock {
    MirBlock {
        id: MirBlockId(id),
        statements,
        terminator: terminator(kind),
        span: span(),
    }
}

fn temp(id: u32, ty: MirType) -> MirTemp {
    MirTemp {
        id: MirTempId(id),
        ty,
        span: span(),
    }
}

fn assign(place: MirPlace, id: u32, ty: MirType, kind: MirValueKind) -> MirStatement {
    statement(MirStatementKind::Assign {
        place,
        value: MirValue {
            id: MirValueId(id),
            kind,
            ty,
            span: span(),
        },
    })
}

fn binary(op: MirBinOp, lhs: MirOperand, rhs: MirOperand) -> MirValueKind {
    MirValueKind::Binary { op, lhs, rhs }
}

fn nota(operand: MirOperand, unit: MirType) -> MirStatement {
    statement(MirStatementKind::RuntimeCall {
        destination: None,
        call: MirRuntimeCall {
            intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Nota),
            args: vec![operand],
            return_ty: unit,
        },
    })
}

fn function(
    id: u32,
    params: Vec<MirParam>,
    locals: Vec<MirLocal>,
    temps: Vec<MirTemp>,
    blocks: Vec<MirBlock>,
    return_ty: MirType,
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

/// `fact(n)` recursive, plus an `incipit` that prints `fact(10)`, a float and
/// a bool. Function 1 is the entry; `tail` appends statements to it.
fn fixture(tail: Vec<MirStatement>, extra_temps: Vec<MirTemp>) -> (MirProgram, TypeTable) {
    let types = TypeTable::new();
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let fractus = MirType::semantic(types.primitive(Primitive::Fractus));
    let bivalens = MirType::semantic(types.primitive(Primitive::Bivalens));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let n = MirLocalId(0);
    let place_n = || MirOperand::Place(MirPlace::local(n));
    let fact = function(
        0,
        vec![MirParam {
            local: n,
            name: None,
            ty: numerus,
            mode: MirParamMode::Owned,
            span: span(),
        }],
        vec![MirLocal {
            id: n,
            name: None,
            ty: numerus,
            mutable: false,
            span: span(),
        }],
        vec![
            temp(0, bivalens),
            temp(1, numerus),
            temp(2, numerus),
            temp(3, numerus),
        ],
        vec![
            block(
                0,
                vec![assign(
                    MirPlace::temp(MirTempId(0)),
                    0,
                    bivalens,
                    binary(
                        MirBinOp::LtEq,
                        place_n(),
                        MirOperand::Constant(MirConstant::Int(1)),
                    ),
                )],
                MirTerminatorKind::Branch {
                    condition: MirOperand::Temp(MirTempId(0)),
                    then_block: MirBlockId(1),
                    else_block: MirBlockId(2),
                },
            ),
            block(
                1,
                vec![],
                MirTerminatorKind::Return(Some(MirOperand::Constant(MirConstant::Int(1)))),
            ),
            block(
                2,
                vec![
                    assign(
                        MirPlace::temp(MirTempId(1)),
                        1,
                        numerus,
                        binary(
                            MirBinOp::Sub,
                            place_n(),
                            MirOperand::Constant(MirConstant::Int(1)),
                        ),
                    ),
                    statement(MirStatementKind::Call {
                        destination: Some(MirPlace::temp(MirTempId(2))),
                        callee: MirCallee::Function(MirFunctionId(0)),
                        args: vec![MirOperand::Temp(MirTempId(1))],
                    }),
                    assign(
                        MirPlace::temp(MirTempId(3)),
                        2,
                        numerus,
                        binary(MirBinOp::Mul, place_n(), MirOperand::Temp(MirTempId(2))),
                    ),
                ],
                MirTerminatorKind::Return(Some(MirOperand::Temp(MirTempId(3)))),
            ),
        ],
        numerus,
    );
    let mut statements = vec![
        statement(MirStatementKind::Call {
            destination: Some(MirPlace::temp(MirTempId(0))),
            callee: MirCallee::Function(MirFunctionId(0)),
            args: vec![MirOperand::Constant(MirConstant::Int(10))],
        }),
        nota(MirOperand::Temp(MirTempId(0)), vacuum),
        nota(MirOperand::Constant(MirConstant::Float(1.5)), vacuum),
        nota(MirOperand::Constant(MirConstant::Bool(true)), vacuum),
    ];
    statements.extend(tail);
    let mut temps = vec![temp(0, numerus)];
    temps.extend(extra_temps);
    let incipit = function(
        1,
        vec![],
        vec![],
        temps,
        vec![block(0, statements, MirTerminatorKind::Return(None))],
        vacuum,
    );
    let _ = fractus;
    (
        MirProgram {
            functions: vec![fact, incipit],
        },
        types,
    )
}

fn build_and_run(name: &str, program: &MirProgram, types: &TypeTable) -> Output {
    let interner = Interner::new();
    let object = radix_mir_clif::emit_program(
        program,
        types,
        &interner,
        Some(MirFunctionId(1)),
        ["true", "false", "none", "tuple"].map(str::to_owned),
    )
    .expect("object emission succeeds");

    let dir = std::env::temp_dir().join(format!("radix-mir-clif-{name}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let object_path = dir.join("program.o");
    std::fs::write(&object_path, &object.bytes).expect("write object");
    let executable = dir.join("program");

    // The platform dead-strip flag is load-bearing (design review):
    // without it the same link measured 13 MB.
    let linked = Command::new("cc")
        .arg(&object_path)
        .arg(build_archive())
        .arg("-Wl,-dead_strip")
        .arg("-o")
        .arg(&executable)
        .output()
        .expect("run cc");
    assert!(
        linked.status.success(),
        "cc link failed: {}{}",
        String::from_utf8_lossy(&linked.stdout),
        String::from_utf8_lossy(&linked.stderr),
    );
    Command::new(&executable)
        .output()
        .expect("run linked program")
}

#[test]
fn recursive_function_links_and_prints_scalars() {
    let (program, types) = fixture(vec![], vec![]);
    let run = build_and_run("fact", &program, &types);
    assert!(run.status.success(), "exit {:?}", run.status.code());
    assert_eq!(String::from_utf8_lossy(&run.stdout), "3628800\n1.5\ntrue\n");
}

#[test]
fn division_by_zero_exits_one_with_a_named_trap() {
    let types = TypeTable::new();
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let (program, _) = fixture(
        vec![assign(
            MirPlace::temp(MirTempId(1)),
            9,
            numerus,
            binary(
                MirBinOp::Div,
                MirOperand::Constant(MirConstant::Int(7)),
                MirOperand::Temp(MirTempId(2)),
            ),
        )],
        vec![temp(1, numerus), temp(2, numerus)],
    );
    // Rebuild against the fixture's own table so the type ids agree.
    let (program, table) = {
        let (_, table) = fixture(vec![], vec![]);
        (program, table)
    };
    let run = build_and_run("divzero", &program, &table);
    assert_eq!(run.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "3628800\n1.5\ntrue\n",
        "output before the trap is flushed"
    );
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("numerus division failed"),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
}
