//! C1/C3 proof: objects emitted from hand-built MIR link with the runtime
//! archive via `cc` and run (goal `radix-mir-clif`, units C1 and C3).
//!
//! The archive is `runtime/clif`'s staticlib. This crate cannot name it as a
//! dev-dependency (compiler crates never depend on `runtime/*`), and a plain
//! `cargo test` links only the rlib, so the test builds the archive itself with
//! the same `cargo` into the profile directory it is running from. A failed
//! build fails the test loudly rather than skipping.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use radix_lexer::{Interner, Span};
use radix_mir::*;
use radix_types::{NumericWidth, Primitive, Type, TypeTable};

/// The four tests in this executable run concurrently, and each one builds the
/// same archive into the same profile directory. Holding this lock for the
/// whole `cargo` invocation plus the archive check serializes them so only one
/// `cargo build` runs inside the test binary at a time.
static ARCHIVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Wait out a rebuild window opened by a `cargo` in another process sharing
/// this target directory: `cargo build` can exit zero while the `.a` is
/// momentarily absent. Poll with a bounded wait before the loud assert.
fn wait_for_archive(archive: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !archive.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
}

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
    // Hold the guard through both the build and the check below.
    let _guard = ARCHIVE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let built = build.output().expect("run cargo build -p faber-host-clif");
    assert!(
        built.status.success(),
        "cargo build -p faber-host-clif failed: {}{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr),
    );

    let archive = profile_dir.join("libfaber_rt_clif.a");
    wait_for_archive(&archive);
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
        &radix_mir_clif::StructMeta::default(),
    )
    .expect("object emission succeeds");

    let dir = std::env::temp_dir().join(format!("radix-mir-clif-{name}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let object_path = dir.join("program.o");
    std::fs::write(&object_path, &object.bytes).expect("write object");
    let executable = dir.join("program");

    // The platform dead-strip flag is load-bearing (design review):
    // without it the same link measured 13 MB. A Rust staticlib also needs
    // the system libraries on Linux.
    let platform_args: &[&str] = if cfg!(target_os = "macos") {
        &["-Wl,-dead_strip"]
    } else {
        &["-Wl,--gc-sections", "-lm", "-ldl", "-lpthread"]
    };
    let linked = Command::new("cc")
        .arg(&object_path)
        .arg(build_archive())
        .args(platform_args)
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

/// Intermediates are exact in `[-2^63, 2^64 - 1]`: `max + 1` prints, adding
/// `max` reaches the top of the range, the squared product goes through the
/// runtime, `/ 3` and `% 7` through the runtime
/// division, and a product past the range ends the run with the runner's
/// wording.
#[test]
fn exact_intermediates_pass_the_i64_carrier() {
    let types = TypeTable::new();
    let numerus = MirType::semantic(types.primitive(Primitive::Numerus));
    let vacuum = MirType::semantic(types.primitive(Primitive::Vacuum));
    let (_, table) = fixture(vec![], vec![]);
    let max = || MirOperand::Constant(MirConstant::Int(i64::MAX));
    let int = |value: i64| MirOperand::Constant(MirConstant::Int(value));
    let wide = |temp: u32| MirOperand::Temp(MirTempId(temp));
    let compute = |target: u32, id: u32, op: MirBinOp, lhs: MirOperand, rhs: MirOperand| {
        assign(
            MirPlace::temp(MirTempId(target)),
            id,
            numerus,
            binary(op, lhs, rhs),
        )
    };
    let tail = vec![
        compute(1, 10, MirBinOp::Add, max(), int(1)),
        nota(wide(1), vacuum),
        compute(2, 11, MirBinOp::Add, wide(1), max()),
        nota(wide(2), vacuum),
        compute(3, 12, MirBinOp::Div, wide(2), int(3)),
        nota(wide(3), vacuum),
        compute(4, 13, MirBinOp::Mod, wide(2), int(7)),
        nota(wide(4), vacuum),
        compute(5, 14, MirBinOp::Sub, wide(1), int(1)),
        nota(wide(5), vacuum),
        compute(6, 15, MirBinOp::Mul, wide(2), wide(2)),
        nota(wide(6), vacuum),
    ];
    let temps = (1..=6).map(|id| temp(id, numerus)).collect();
    let (program, _) = fixture(tail, temps);
    let run = build_and_run("exact-intermediates", &program, &table);
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    assert_eq!(
        stdout,
        "3628800\n1.5\ntrue\n9223372036854775808\n18446744073709551615\n6148914691236517205\n1\n9223372036854775807\n",
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("numerus overflow"),
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
}

/// A `d64` computes exactly until a store: a product keeps its sixteen fraction
/// digits, a quotient rounds half-even at the larger operand scale, and a
/// comparison with a whole number scales the integer.
#[test]
fn decimals_compute_exactly_and_print_shortest() {
    let (_, mut table) = fixture(vec![], vec![]);
    let d64 =
        MirType::semantic(table.intern(Type::SizedNumeric(Primitive::Numerus, NumericWidth::D64)));
    let bivalens = MirType::semantic(table.primitive(Primitive::Bivalens));
    let vacuum = MirType::semantic(table.primitive(Primitive::Vacuum));
    let carrier = |value: i64| MirOperand::Constant(MirConstant::Int(value));
    let wide = |temp: u32| MirOperand::Temp(MirTempId(temp));
    let tail = vec![
        // 4.2 and 0.1 as scale-8 carriers.
        assign(
            MirPlace::temp(MirTempId(1)),
            10,
            d64,
            MirValueKind::Operand(carrier(420_000_000)),
        ),
        assign(
            MirPlace::temp(MirTempId(2)),
            11,
            d64,
            MirValueKind::Operand(carrier(10_000_000)),
        ),
        assign(
            MirPlace::temp(MirTempId(3)),
            12,
            d64,
            binary(MirBinOp::Mul, wide(1), wide(2)),
        ),
        nota(wide(3), vacuum),
        assign(
            MirPlace::temp(MirTempId(4)),
            13,
            d64,
            binary(MirBinOp::Div, wide(1), wide(2)),
        ),
        nota(wide(4), vacuum),
        assign(
            MirPlace::temp(MirTempId(5)),
            14,
            d64,
            binary(MirBinOp::Sub, wide(2), wide(1)),
        ),
        nota(wide(5), vacuum),
        assign(
            MirPlace::temp(MirTempId(6)),
            15,
            bivalens,
            binary(MirBinOp::Gt, wide(1), carrier(4)),
        ),
        nota(wide(6), vacuum),
    ];
    let temps = vec![
        temp(1, d64),
        temp(2, d64),
        temp(3, d64),
        temp(4, d64),
        temp(5, d64),
        temp(6, bivalens),
    ];
    let (program, _) = fixture(tail, temps);
    let run = build_and_run("decimals", &program, &table);
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    assert_eq!(
        stdout,
        "3628800\n1.5\ntrue\n0.42\n42\n-4.1\ntrue\n",
        "stderr: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.status.code(), Some(0));
}
