//! Focused HIR-direct Rust vs MIR-stepper parity fixtures for campaign gates.

use super::common::{
    cargo_available, format_diagnostics, make_temp_root, normalize_newline, read_expected_stdout,
    shared_target_dir, write_rust_cargo_project,
};
use radix::driver::Session;
use radix::mir::{run_source, BufferHost, RunSourceError};
use radix::{Compiler, Config, Output};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct ParityFixture {
    row: &'static str,
    path: &'static str,
}

struct FailureParityFixture {
    row: &'static str,
    path: &'static str,
    stepper_issue: &'static str,
    rust_message: &'static str,
}

const STAGE1_FIXTURES: &[ParityFixture] = &[
    ParityFixture {
        row: "SYNC-001",
        path: "conversio/lista-tensor-shaped.fab",
    },
    ParityFixture {
        row: "SYNC-006",
        path: "tensor/bracket-access.fab",
    },
    ParityFixture {
        row: "SYNC-008",
        path: "conversio/verte-aggregate.fab",
    },
    ParityFixture {
        row: "SYNC-010",
        path: "conversio/instans.fab",
    },
];

const STAGE2A_FIXTURES: &[ParityFixture] = &[
    ParityFixture {
        row: "SYNC-013",
        path: "conversio/conversio.fab",
    },
    ParityFixture {
        row: "SYNC-013",
        path: "conversio/radix.fab",
    },
    ParityFixture {
        row: "SYNC-014",
        path: "conversio/numeric-bool.fab",
    },
    ParityFixture {
        row: "SYNC-014",
        path: "conversio/bivalens.fab",
    },
    ParityFixture {
        row: "SYNC-015",
        path: "conversio/octeti.fab",
    },
    ParityFixture {
        row: "SYNC-017",
        path: "conversio/instans-valor-carrier.fab",
    },
];

const STAGE2B_FIXTURES: &[ParityFixture] = &[ParityFixture {
    row: "SYNC-016",
    path: "conversio/regex.fab",
}];

const STAGE2C_SCALAR_FIXTURES: &[ParityFixture] = &[ParityFixture {
    row: "SYNC-018",
    path: "conversio/valor-scalaria.fab",
}];

const STAGE2C_AGGREGATE_FIXTURES: &[ParityFixture] = &[
    ParityFixture {
        row: "SYNC-019",
        path: "conversio/valor-genus.fab",
    },
    ParityFixture {
        row: "SYNC-019",
        path: "conversio/valor-tensor.fab",
    },
];

const STAGE2C_BOXING_FIXTURES: &[ParityFixture] = &[ParityFixture {
    row: "SYNC-020",
    path: "conversio/valor-boxing.fab",
}];

const STAGE3_COLLECTION_FIXTURES: &[ParityFixture] = &[ParityFixture {
    row: "SYNC-005",
    path: "tensor/method-policy.fab",
}];

const STAGE3_COLLECTION_FAILURE_FIXTURES: &[FailureParityFixture] = &[FailureParityFixture {
    row: "SYNC-005",
    path: "tensor/method-errors.fab",
    stepper_issue: "stepper_tensor_structa_element_count_does_not_match_shape",
    rust_message: "tensor structa element count does not match shape",
}];

const STAGE5_NUMERUS_FAILURE_FIXTURES: &[FailureParityFixture] = &[FailureParityFixture {
    row: "SYNC-007",
    path: "operatores/numerus-overflow.fab",
    stepper_issue: "stepper_numerus_overflow",
    rust_message: "numerus overflow",
}];

const MODULAR_WORD_FIXTURE: ParityFixture = ParityFixture {
    row: "MODULAR-001",
    path: "operatores/modular-word.fab",
};

const MODULAR_WORD_SHA_ROUND_FIXTURE: ParityFixture = ParityFixture {
    row: "MODULAR-SHA-001",
    path: "operatores/modular-word-sha-round.fab",
};

const MODULAR_WORD_WIDTH_FIXTURES: &[ParityFixture] = &[
    ParityFixture {
        row: "MODULAR-W8-001",
        path: "operatores/modular-word-u8.fab",
    },
    ParityFixture {
        row: "MODULAR-W16-001",
        path: "operatores/modular-word-u16.fab",
    },
    ParityFixture {
        row: "MODULAR-W64-001",
        path: "operatores/modular-word-u64.fab",
    },
    ParityFixture {
        row: "MODULAR-SHA64-001",
        path: "operatores/modular-word-u64-sha-round.fab",
    },
];

struct ParityHarness {
    corpus: PathBuf,
    temp_root: PathBuf,
    shared_target: PathBuf,
    session: Session,
    compiler: Compiler,
}

impl ParityHarness {
    fn new() -> Option<Self> {
        if !cargo_available() {
            eprintln!("cargo not found on PATH; skipping HIR/MIR parity fixture test");
            return None;
        }
        let temp_root = make_temp_root();
        Some(Self {
            corpus: crate::paths::corpus_dir(),
            shared_target: shared_target_dir(&temp_root),
            temp_root,
            session: Session::new(Config::default()),
            compiler: Compiler::new(Config::default()),
        })
    }
}

enum CargoRunMode {
    ExpectSuccess,
    ExpectFailure,
}

#[test]
fn hir_mir_stage1_parity_fixtures_match_stepper_and_rust() {
    assert_parity_fixtures("stage1_parity", STAGE1_FIXTURES);
}

#[test]
fn hir_mir_stage2a_scalar_conversio_parity_fixtures_match_stepper_and_rust() {
    assert_parity_fixtures("stage2a_parity", STAGE2A_FIXTURES);
}

#[test]
fn hir_mir_stage2b_regex_conversio_parity_fixtures_match_stepper_and_rust() {
    assert_parity_fixtures("stage2b_parity", STAGE2B_FIXTURES);
}

#[test]
fn hir_mir_stage2c_valor_scalar_parity_fixtures_match_stepper_and_rust() {
    assert_parity_fixtures("stage2c_scalar_parity", STAGE2C_SCALAR_FIXTURES);
}

#[test]
fn hir_mir_stage2c_valor_aggregate_parity_fixtures_match_stepper_and_rust() {
    assert_parity_fixtures("stage2c_aggregate_parity", STAGE2C_AGGREGATE_FIXTURES);
}

#[test]
fn hir_mir_stage2c_valor_boxing_parity_fixtures_match_stepper_and_rust() {
    assert_parity_fixtures("stage2c_boxing_parity", STAGE2C_BOXING_FIXTURES);
}

#[test]
fn hir_mir_stage3_collection_method_parity_fixtures_match_stepper_and_rust() {
    assert_parity_fixtures("stage3_collection_parity", STAGE3_COLLECTION_FIXTURES);
    assert_failure_parity_fixtures(
        "stage3_collection_failure_parity",
        STAGE3_COLLECTION_FAILURE_FIXTURES,
    );
}

#[test]
fn hir_mir_stage5_numerus_overflow_parity_fixture_matches_stepper_and_rust() {
    assert_failure_parity_fixtures(
        "stage5_numerus_failure_parity",
        STAGE5_NUMERUS_FAILURE_FIXTURES,
    );
}

#[test]
fn modular_word_target_parity() {
    let Some(harness) = ParityHarness::new() else {
        return;
    };
    let fixture = &MODULAR_WORD_FIXTURE;
    let file = harness.corpus.join(fixture.path);
    let expected = read_expected_stdout(&file).expect("modular word fixture has expected output");
    let stepper = run_stepper_stdout(&harness.session, &file).expect("modular word stepper route");
    let code = compile_rust_code(&harness.compiler, &file).expect("modular word Rust route");
    let project_dir = harness.temp_root.join("modular-word-target-parity");
    let package_name = "modular_word_target_parity";
    let debug = run_cargo_project(
        &project_dir,
        package_name,
        &code,
        &harness.shared_target,
        CargoRunMode::ExpectSuccess,
        false,
    )
    .expect("modular word debug Rust route");
    let release = run_cargo_project(
        &project_dir,
        package_name,
        &code,
        &harness.shared_target,
        CargoRunMode::ExpectSuccess,
        true,
    )
    .expect("modular word release Rust route");

    assert_eq!(stepper, expected, "modular word stepper output drifted");
    assert_eq!(debug, expected, "modular word debug Rust output drifted");
    assert_eq!(
        release, expected,
        "modular word release Rust output drifted"
    );
    assert_eq!(debug, release, "modular word debug/release output drifted");
}

#[test]
fn modular_word_sha_round_target_parity() {
    assert_parity_fixtures(
        "modular_word_sha_round_parity",
        &[MODULAR_WORD_SHA_ROUND_FIXTURE],
    );
}

#[test]
fn modular_word_widths_target_parity() {
    assert_parity_fixtures("modular_word_widths_parity", MODULAR_WORD_WIDTH_FIXTURES);
}

fn assert_parity_fixtures(package_prefix: &str, fixtures: &[ParityFixture]) {
    let Some(harness) = ParityHarness::new() else {
        return;
    };

    for (idx, fixture) in fixtures.iter().enumerate() {
        let file = harness.corpus.join(fixture.path);
        let expected = read_expected_stdout(&file)
            .unwrap_or_else(|| panic!("{} must have a sibling .expected file", fixture.path));
        let stepper = run_stepper_stdout(&harness.session, &file).unwrap_or_else(|reason| {
            panic!("{} {} stepper failed: {reason}", fixture.row, fixture.path)
        });
        let rust = run_rust_stdout(
            &harness.compiler,
            &file,
            &harness.temp_root,
            &harness.shared_target,
            package_prefix,
            idx,
            fixture,
        )
        .unwrap_or_else(|reason| {
            panic!(
                "{} {} Rust route failed: {reason}",
                fixture.row, fixture.path
            )
        });

        assert_eq!(
            stepper, expected,
            "{} {} stepper stdout changed",
            fixture.row, fixture.path
        );
        assert_eq!(
            rust, expected,
            "{} {} Rust stdout changed",
            fixture.row, fixture.path
        );
        assert_eq!(
            stepper, rust,
            "{} {} route stdout drifted",
            fixture.row, fixture.path
        );
    }
}

fn assert_failure_parity_fixtures(package_prefix: &str, fixtures: &[FailureParityFixture]) {
    let Some(harness) = ParityHarness::new() else {
        return;
    };

    for (idx, fixture) in fixtures.iter().enumerate() {
        let file = harness.corpus.join(fixture.path);
        let stepper = run_stepper_failure(&harness.session, &file).unwrap_or_else(|reason| {
            panic!(
                "{} {} stepper failure probe did not fail as expected: {reason}",
                fixture.row, fixture.path
            )
        });
        let rust = run_rust_failure(
            &harness.compiler,
            &file,
            &harness.temp_root,
            &harness.shared_target,
            package_prefix,
            idx,
            fixture,
        )
        .unwrap_or_else(|reason| {
            panic!(
                "{} {} Rust failure probe did not fail as expected: {reason}",
                fixture.row, fixture.path
            )
        });

        assert!(
            stepper.contains(fixture.stepper_issue),
            "{} {} stepper failure issue drifted: {stepper}",
            fixture.row,
            fixture.path
        );
        assert!(
            rust.contains(fixture.rust_message),
            "{} {} Rust failure message drifted: {rust}",
            fixture.row,
            fixture.path
        );
    }
}

fn run_stepper_stdout(session: &Session, file: &Path) -> Result<String, String> {
    let source =
        fs::read_to_string(file).map_err(|err| format!("cannot read {}: {err}", file.display()))?;
    let mut host = BufferHost::default();
    run_source(session, &file.display().to_string(), &source, &mut host)
        .map_err(format_run_source_error)?;
    if !host.stderr_lines.is_empty() {
        return Err(format!(
            "unexpected stepper stderr: {}",
            host.stderr_lines.join("\n")
        ));
    }
    Ok(normalize_newline(&host.stdout_lines.join("\n")))
}

fn run_stepper_failure(session: &Session, file: &Path) -> Result<String, String> {
    let source =
        fs::read_to_string(file).map_err(|err| format!("cannot read {}: {err}", file.display()))?;
    let mut host = BufferHost::default();
    match run_source(session, &file.display().to_string(), &source, &mut host) {
        Ok(()) => Err("stepper unexpectedly succeeded".to_owned()),
        Err(error) => Ok(format_run_source_error(error)),
    }
}

fn compile_rust_code(compiler: &Compiler, file: &Path) -> Result<String, String> {
    let result = compiler.compile(file);
    match result.output {
        Some(Output::Rust(output)) => Ok(output.code),
        Some(_) => Err("compiler did not produce Rust output".to_owned()),
        None => Err(format!("compile failed: {}", format_diagnostics(&result))),
    }
}

fn run_rust_stdout(
    compiler: &Compiler,
    file: &Path,
    temp_root: &Path,
    shared_target: &Path,
    package_prefix: &str,
    idx: usize,
    fixture: &ParityFixture,
) -> Result<String, String> {
    let code = compile_rust_code(compiler, file)?;
    let project_dir = temp_root.join(format!("{idx:03}-{}", fixture.row.to_ascii_lowercase()));
    let package_name = format!("{package_prefix}_{idx:03}");
    run_cargo_project(
        &project_dir,
        &package_name,
        &code,
        shared_target,
        CargoRunMode::ExpectSuccess,
        false,
    )
}

fn run_rust_failure(
    compiler: &Compiler,
    file: &Path,
    temp_root: &Path,
    shared_target: &Path,
    package_prefix: &str,
    idx: usize,
    fixture: &FailureParityFixture,
) -> Result<String, String> {
    let code = compile_rust_code(compiler, file)?;
    let project_dir = temp_root.join(format!(
        "{idx:03}-{}-failure",
        fixture.row.to_ascii_lowercase()
    ));
    let package_name = format!("{package_prefix}_{idx:03}");
    run_cargo_project(
        &project_dir,
        &package_name,
        &code,
        shared_target,
        CargoRunMode::ExpectFailure,
        true,
    )
}

fn run_cargo_project(
    project_dir: &Path,
    package_name: &str,
    code: &str,
    shared_target: &Path,
    mode: CargoRunMode,
    release: bool,
) -> Result<String, String> {
    let manifest = write_rust_cargo_project(project_dir, package_name, code);
    let mut command = Command::new("cargo");
    command
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(&manifest)
        .env("CARGO_TARGET_DIR", shared_target);
    set_warning_quiet_rustflags(&mut command);
    if release {
        command.arg("--release");
    }
    let run = command
        .output()
        .map_err(|err| format!("cannot run generated Rust binary: {err}"))?;

    match mode {
        CargoRunMode::ExpectSuccess => {
            if !run.status.success() {
                return Err(format!(
                    "generated Rust binary exited {:?}\nstdout:\n{}\nstderr:\n{}",
                    run.status.code(),
                    String::from_utf8_lossy(&run.stdout),
                    String::from_utf8_lossy(&run.stderr)
                ));
            }
            let stderr = String::from_utf8_lossy(&run.stderr);
            if !stderr.trim().is_empty() {
                return Err(format!("unexpected Rust stderr: {}", stderr.trim()));
            }
            Ok(normalize_newline(&String::from_utf8_lossy(&run.stdout)))
        }
        CargoRunMode::ExpectFailure => {
            if run.status.success() {
                return Err("generated Rust binary unexpectedly succeeded".to_owned());
            }
            Ok(format!(
                "stdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(&run.stderr)
            ))
        }
    }
}

fn set_warning_quiet_rustflags(command: &mut Command) {
    // The parity harness compares Faber runtime stdout/stderr behavior. Cargo
    // warning text from generated Rust fixtures is compile-time noise and is
    // covered by separate generated-code warning/canonicality checks.
    let rustflags = std::env::var("RUSTFLAGS").unwrap_or_default();
    let rustflags = if rustflags
        .split_whitespace()
        .any(|flag| flag == "-Awarnings")
    {
        rustflags
    } else if rustflags.is_empty() {
        "-Awarnings".to_owned()
    } else {
        format!("{rustflags} -Awarnings")
    };
    command.env("RUSTFLAGS", rustflags);
}

fn format_run_source_error(error: RunSourceError) -> String {
    match error {
        RunSourceError::Frontend(diagnostics) => diagnostics
            .iter()
            .map(|diagnostic| format!("{:?}:{:?}", diagnostic.code, diagnostic.issue()))
            .collect::<Vec<_>>()
            .join(" | "),
        RunSourceError::Mir(errors) => errors
            .iter()
            .map(|error| error.issue.as_str())
            .collect::<Vec<_>>()
            .join(" | "),
        RunSourceError::Stepper(errors) => errors
            .iter()
            .map(|error| error.issue.as_str())
            .collect::<Vec<_>>()
            .join(" | "),
    }
}
