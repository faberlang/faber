//! The default engine of a plain `faber run` (no `--target`): the clif-jit
//! lane first, falling back to the runner on any compile-time decline
//! (operator ruling 2026-10-29).
//!
//! The boundary pinned here:
//!
//! * a program the Cranelift lowering accepts runs on clif-jit — proven with
//!   an AIR exemplar only that lane can execute (`genus/air-struct-literal`:
//!   the runner refuses it with `TARGETLANE001`);
//! * a program the lowering declines at compile time (the named capability
//!   refusals) falls back to the runner, byte-identical to
//!   `--target runner` on stdout, stderr and exit status, while the explicit
//!   `--target clif-jit` still refuses it;
//! * the fallback is decided only from the compile-time result: a program
//!   that traps on clif-jit owns its process there and is never re-run;
//! * device runs keep the runner as their host engine.

#![cfg(all(feature = "runner", feature = "mir-clif"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn faber() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_faber"));
    command.stdin(Stdio::null());
    command
}

struct Outcome {
    stdout: Vec<u8>,
    stderr: String,
    status: Option<i32>,
}

impl std::fmt::Debug for Outcome {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "status={:?} stdout={:?} stderr={:?}",
            self.status,
            String::from_utf8_lossy(&self.stdout),
            self.stderr
        )
    }
}

fn outcome(command: &mut Command) -> Outcome {
    let output = command.output().expect("spawn faber");
    Outcome {
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status.code(),
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A program only the clif lane runs: the runner's MIR-backed lane
/// restriction refuses it, so a successful plain run proves the default
/// engine compiled and executed it in memory.
fn air_exemplar() -> PathBuf {
    root().join("corpus/genus/air-struct-literal.fab")
}

#[test]
fn plain_run_of_an_accepted_program_uses_clif_jit() {
    let source = air_exemplar();
    let plain = outcome(faber().arg("run").arg(&source));
    let jit = outcome(faber().args(["run", "--target", "clif-jit"]).arg(&source));
    let runner = outcome(faber().args(["run", "--target", "runner"]).arg(&source));
    assert_eq!(jit.status, Some(0), "{:?}", jit.stderr);
    assert!(!jit.stdout.is_empty(), "the exemplar prints its answer");
    // The plain route is the JIT lane, byte for byte.
    assert_eq!(plain.status, jit.status);
    assert_eq!(plain.stdout, jit.stdout, "stdout");
    assert_eq!(plain.stderr, jit.stderr, "stderr");
    // ... and not the runner, which refuses this program by name.
    assert_eq!(runner.status, Some(65), "{:?}", runner.stderr);
    assert!(runner.stderr.contains("TARGETLANE001"));
    assert_eq!(plain.stdout, b"42\n");
}

/// A program the Cranelift lowering declines at compile time: the plain run
/// executes on the runner and is byte-identical to `--target runner`, while
/// the explicit `--target clif-jit` keeps refusing it by name. Fixtures are
/// the regression set of `clif_jit_parity_regression_test.rs`: one corpus
/// exemplar and two small sources.
#[test]
fn plain_run_of_a_refused_program_falls_back_to_the_runner() {
    let dir = tempfile::tempdir().expect("scratch");
    let mut sources = vec![(
        "corpus/call-defaults".to_owned(),
        root().join("corpus/sponte/call-defaults.fab"),
    )];
    for (name, text) in [
        (
            "omitted",
            "fn pag(int p optional coalesce 1) → int { return p }\nmain {\n    print \"before\"\n    print pag()\n}\n",
        ),
        (
            "mut_union",
            "class Circ { mut int r = 1 }\nclass Sq { mut int s = 10 }\ntype Figura = Circ ∪ Sq\nfn bump(mut Figura u) → void {}\nmain {\n    print \"before\"\n    mut Circ c ← Circ {}\n    bump(mut c)\n}\n",
        ),
    ] {
        let path = dir.path().join(format!("{name}.fab"));
        fs::write(&path, text).expect("fixture");
        sources.push((name.to_owned(), path));
    }
    for (name, source) in sources {
        let plain = outcome(faber().arg("run").arg(&source));
        let runner = outcome(faber().args(["run", "--target", "runner"]).arg(&source));
        let jit = outcome(faber().args(["run", "--target", "clif-jit"]).arg(&source));
        // The fallback is the runner route, byte for byte.
        assert_eq!(plain.status, runner.status, "{name}: exit status");
        assert_eq!(plain.stdout, runner.stdout, "{name}: stdout");
        assert_eq!(plain.stderr, runner.stderr, "{name}: stderr");
        assert_eq!(
            plain.status,
            Some(0),
            "{name}: the runner runs it: {plain:?}"
        );
        // The explicit target never falls back.
        assert_eq!(jit.status, Some(65), "{name}: refusal status");
        assert!(jit.stdout.is_empty(), "{name}: refusal precedes execution");
        assert!(jit.stderr.contains("CODEGEN001"), "{name}: {}", jit.stderr);
    }
}

/// A trap on clif-jit owns its process: the program's output appears once and
/// the exit status is the trap's own, never a second run on the runner.
#[test]
fn a_program_that_traps_on_clif_jit_is_not_rerun() {
    let dir = tempfile::tempdir().expect("scratch");
    let source = dir.path().join("trap.fab");
    fs::write(
        &source,
        "main {\n    let list<int> xs ← [10, 20, 30]\n    mut int n ← 9\n    print \"before\"\n    print xs[n]\n}\n",
    )
    .expect("fixture");
    let plain = outcome(faber().arg("run").arg(&source));
    let jit = outcome(faber().args(["run", "--target", "clif-jit"]).arg(&source));
    assert_eq!(plain.status, Some(70), "{plain:?}");
    assert_eq!(plain.stdout, b"before\n", "the trap output appears once");
    assert_eq!(plain.status, jit.status);
    assert_eq!(plain.stdout, jit.stdout);
    let trap_line = |stderr: &str| {
        stderr
            .lines()
            .find(|line| line.starts_with("error:"))
            .map(str::to_owned)
    };
    assert_eq!(
        trap_line(&plain.stderr).as_deref(),
        Some("error: array index out of bounds")
    );
    assert_eq!(trap_line(&plain.stderr), trap_line(&jit.stderr));
}

/// Device runs keep their current host engine: a `--device reference` run
/// goes through the interpreter whatever the default engine is, so its
/// streams match `--target runner --device reference` exactly (no
/// compile-lane warnings appear).
#[test]
fn device_runs_keep_the_runner_as_their_host_engine() {
    let dir = tempfile::tempdir().expect("scratch");
    let source = dir.path().join("trap.fab");
    fs::write(
        &source,
        "main {\n    let list<int> xs ← [10, 20, 30]\n    mut int n ← 9\n    print \"before\"\n    print xs[n]\n}\n",
    )
    .expect("fixture");
    let plain = outcome(faber().args(["run", "--device", "reference"]).arg(&source));
    let runner = outcome(
        faber()
            .args(["run", "--target", "runner", "--device", "reference"])
            .arg(&source),
    );
    assert_eq!(plain.status, Some(70));
    assert_eq!(plain.status, runner.status);
    assert_eq!(plain.stdout, runner.stdout);
    assert_eq!(plain.stderr, runner.stderr);
    assert!(
        !plain.stderr.contains("warning["),
        "the reference route prints no compile-lane warnings: {}",
        plain.stderr
    );
}

/// A kernel-declaring input keeps the `auto` device route (the in-process
/// device session, whose host lane is the runner): the clif lane would run
/// the kernel-less exemplar to a silent success, so any nonzero plain run
/// proves the default engine did not take it. On a Metal host the device
/// session answers (`PKG001` no entry function); on a host with no admitted
/// device `auto` fails closed — both nonzero, never the JIT's silent 0.
#[test]
fn kernel_declaring_inputs_keep_the_device_route() {
    let source = root().join("corpus/cuda/addita-proof.fab");
    let plain = outcome(faber().arg("run").arg(&source));
    let jit = outcome(faber().args(["run", "--target", "clif-jit"]).arg(&source));
    assert_eq!(jit.status, Some(0), "{:?}", jit.stderr);
    assert!(jit.stdout.is_empty());
    assert_ne!(plain.status, Some(0), "{plain:?}");
}

/// Two input shapes whose semantics only the interpreted route serves today
/// keep the runner under the default engine: an `@ cli` program's option
/// values bind through package CLI planning (the clif lanes would run them
/// with defaults), and a lone `.fab` file that imports a sibling module links
/// through the package route (the single-file clif compile resolves no file
/// imports). Pinned end to end by `run_integration_test.rs`
/// (`run_cli_root_*_option_executes_without_cargo_or_rust_emit`,
/// `run_import_cycle_failure_does_not_emit_rust`); this is the engine-side
/// statement of the same contracts.
#[test]
fn cli_options_and_file_imports_bind_on_the_interpreted_route() {
    let dir = tempfile::tempdir().expect("scratch");
    let cli = dir.path().join("tool.fab");
    fs::write(
        &cli,
        "@ cli \"tool\"\n@ option verbose long \"verbose\" type bool\nmain args args {\n  print args.verbose\n}\n",
    )
    .expect("fixture");
    let plain = outcome(faber().arg("run").arg(&cli).args(["--", "--verbose"]));
    assert_eq!(plain.status, Some(0), "{plain:?}");
    assert_eq!(plain.stdout, b"true\n", "the option value must bind");

    let imported = dir.path().join("main.fab");
    fs::write(
        &imported,
        "import from \"./jobs\" jobs\n\nmain {\n    print jobs.answer()\n}\n",
    )
    .expect("entry");
    fs::write(
        dir.path().join("jobs.fab"),
        "@ public\nfn answer() → int { return 42 }\n",
    )
    .expect("sibling");
    let plain = outcome(faber().arg("run").arg(&imported));
    assert_eq!(plain.status, Some(0), "{plain:?}");
    assert_eq!(plain.stdout, b"42\n", "the sibling import must link");
}

/// The engine note is a `--debug` (stderr-only) line: stdout stays the
/// program's own on both sides of the decision.
#[test]
fn the_engine_note_lives_behind_debug_on_stderr() {
    let dir = tempfile::tempdir().expect("scratch");
    let accepted = dir.path().join("hello.fab");
    fs::write(&accepted, "main {\n    print \"hello\"\n}\n").expect("fixture");
    let noted = outcome(faber().args(["run", "--debug"]).arg(&accepted));
    assert_eq!(noted.status, Some(0));
    assert_eq!(noted.stdout, b"hello\n");
    assert!(
        noted.stderr.contains("debug: engine: clif-jit (default)"),
        "{}",
        noted.stderr
    );

    let refused = dir.path().join("omitted.fab");
    fs::write(
        &refused,
        "fn pag(int p optional coalesce 1) → int { return p }\nmain {\n    print pag()\n}\n",
    )
    .expect("fixture");
    let noted = outcome(faber().args(["run", "--debug"]).arg(&refused));
    assert_eq!(noted.status, Some(0));
    assert_eq!(noted.stdout, b"1\n");
    assert!(
        noted
            .stderr
            .contains("debug: engine: runner (clif-jit declined at compile time)"),
        "{}",
        noted.stderr
    );
}
