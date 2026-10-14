use super::{Case, ChildError, Outcome, TargetExpectation, run_bounded, runner_run_args};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn case(args: &[&str]) -> Case {
    Case {
        id: "c".to_owned(),
        dir: PathBuf::from("/d"),
        input: PathBuf::from("/d/main.fab"),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        stdin: None,
        expect: None,
        target: TargetExpectation {
            outcome: Outcome::Pass,
            phase: None,
            failure_kind: None,
            args: None,
            diagnostic_contains: None,
            reason: None,
            expect: None,
            timeout_secs: None,
            known_fail_why: None,
            rejects: false,
        },
        timeout: Duration::from_secs(1),
    }
}

#[test]
fn runner_target_pins_the_cpu_device() {
    assert_eq!(
        runner_run_args(&case(&[])),
        ["run", "/d/main.fab", "--device", "cpu"]
    );
    assert_eq!(
        runner_run_args(&case(&["a", "b"])),
        ["run", "/d/main.fab", "--device", "cpu", "--", "a", "b"]
    );
}

#[cfg(unix)]
#[test]
fn run_bounded_times_out_when_a_descendant_holds_the_pipe_after_the_parent_exits() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pidfile = dir.path().join("pid");
    let mut command = Command::new("sh");
    command
        .args(["-c", "sleep 30 & echo $! > \"$1\"; exit 0", "sh"])
        .arg(&pidfile);
    let started = Instant::now();
    let result = run_bounded(&mut command, Stdio::null(), Duration::from_secs(1));
    let elapsed = started.elapsed();
    let Err(ChildError::Timeout(limit)) = result else {
        panic!("a descendant holding the pipe must hit the deadline, not return output");
    };
    assert_eq!(limit, Duration::from_secs(1));
    assert!(
        elapsed < Duration::from_secs(10),
        "run_bounded took {elapsed:?}"
    );
    let pid = fs::read_to_string(&pidfile)
        .expect("the case recorded its descendant pid")
        .trim()
        .to_owned();
    let alive = || {
        Command::new("sh")
            .args(["-c", "kill -0 \"$1\" 2>/dev/null", "sh", &pid])
            .output()
            .expect("pid probe")
            .status
            .success()
    };
    for _ in 0..40 {
        if !alive() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("descendant {pid} outlived the process-group kill");
}

#[cfg(unix)]
#[test]
fn run_bounded_returns_output_when_child_and_pipes_close_promptly() {
    let mut command = Command::new("sh");
    command.args(["-c", "echo out; echo err 1>&2"]);
    let Ok(output) = run_bounded(&mut command, Stdio::null(), Duration::from_secs(10)) else {
        panic!("a child whose pipes close promptly must return output");
    };
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("out"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("err"));
}
