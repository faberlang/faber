//! A1-R1 proof: the real hosts serve the raw process-capability routes
//! (`solum:*`, `consolum:*`, `processus:exi`) through `Host::sermo_route`,
//! reusing the native kernel verbs; unknown routes stay unrouted.

use super::{BufferHost, RunSourceError, RunnerError, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn run_host(name: &str, source: &str, host: &mut BufferHost) -> Result<(), Vec<RunnerError>> {
    match run_source(
        &Session::new(Config::default().with_target(Target::HirRust)),
        name,
        source,
        host,
    ) {
        Ok(()) => Ok(()),
        Err(RunSourceError::Stepper(errors)) => Err(errors),
        Err(error) => panic!("{name} should reach the MIR runner: {error:?}"),
    }
}

fn run_ok(name: &str, source: &str) -> BufferHost {
    let mut host = BufferHost::default();
    run_host(name, source, &mut host)
        .unwrap_or_else(|errors| panic!("{name} should run on the MIR runner: {errors:?}"));
    host
}

fn program(body: &str) -> String {
    format!("+++\nlocale = \"la\"\n+++\nincipit {{\n{body}\n}}\n")
}

fn fixture(bytes: &[u8]) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("artifact.bin");
    std::fs::write(&path, bytes).expect("write fixture");
    let path = path.to_string_lossy().into_owned();
    (dir, path)
}

#[test]
fn mensura_prints_the_file_size() {
    let (_dir, path) = fixture(b"abcdefgh");
    let host = run_ok(
        "route-mensura.fab",
        &program(&format!("    nota ad 'solum:mensura' (\"{path}\") ↦ numerus")),
    );
    assert_eq!(host.stdout_lines, ["8"]);
}

#[test]
fn conde_writes_durably_and_leaves_no_temporary() {
    let (dir, target) = fixture(b"previous");
    let host = run_ok(
        "route-conde.fab",
        &program(&format!(
            "    sit vacuum gestum ← ad 'solum:conde' ([\"{target}\", \"custodia integra\"]) ↦ vacuum\n    \
             nota ad 'solum:mensura' (\"{target}\") ↦ numerus"
        )),
    );
    assert_eq!(host.stdout_lines, ["16"]);
    assert_eq!(std::fs::read(&target).expect("read the target"), b"custodia integra");
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .expect("read the target directory")
        .map(|entry| entry.expect("directory entry").file_name())
        .collect();
    assert_eq!(
        entries,
        vec![std::ffi::OsString::from("artifact.bin")],
        "the durable replace must leave only the target, no .faber-conde temporary"
    );
}

#[test]
fn partem_reads_the_requested_range() {
    let (_dir, path) = fixture(b"abcdefgh");
    let host = run_ok(
        "route-partem.fab",
        &program(&format!(
            "    sit lista<valor> args ← [\"{path}\", 2, 3]\n    \
             sit lista<b8> bytes ← ad 'solum:partem' (args) ↦ lista<b8>\n    \
             nota bytes[0] ↦ u8\n    nota bytes[2] ↦ u8"
        )),
    );
    assert_eq!(host.stdout_lines, [b'c'.to_string(), b'e'.to_string()]);
}

#[test]
fn partem_honours_the_range_cap() {
    let (_dir, path) = fixture(b"abcdefgh");
    let over = 512 * 1024 * 1024 + 1;
    let mut host = BufferHost::default();
    let errors = run_host(
        "route-partem-cap.fab",
        &program(&format!(
            "    sit lista<valor> args ← [\"{path}\", 0, {over}]\n    \
             nota ad 'solum:partem' (args) ↦ lista<b8>"
        )),
        &mut host,
    )
    .expect_err("an over-cap range must fail");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].issue, "runner_sermo_reply_conversion_failed");
}

#[test]
fn hauri_returns_the_bytes_and_honours_the_ceiling() {
    let (_dir, path) = fixture(b"xyz");
    let host = run_ok(
        "route-hauri.fab",
        &program(&format!(
            "    sit lista<b8> all ← ad 'solum:hauri' (\"{path}\") ↦ lista<b8>\n    \
             nota all[0] ↦ u8\n    nota all[2] ↦ u8\n    \
             sit lista<valor> args ← [\"{path}\", 3]\n    \
             sit lista<b8> capped ← ad 'solum:hauri' (args) ↦ lista<b8>\n    \
             nota capped[1] ↦ u8"
        )),
    );
    assert_eq!(host.stdout_lines, [b'x'.to_string(), b'z'.to_string(), b'y'.to_string()]);

    let mut host = BufferHost::default();
    let errors = run_host(
        "route-hauri-ceiling.fab",
        &program(&format!(
            "    sit lista<valor> args ← [\"{path}\", 2]\n    \
             nota ad 'solum:hauri' (args) ↦ lista<b8>"
        )),
        &mut host,
    )
    .expect_err("a file over the ceiling must fail");
    assert_eq!(errors[0].issue, "runner_sermo_reply_conversion_failed");
}

#[test]
fn digestio_is_gone_with_the_solum_kernel() {
    let (_dir, path) = fixture(b"abc");
    let mut host = BufferHost::default();
    let errors = run_host(
        "route-digestio.fab",
        &program(&format!("    nota ad 'solum:digestio' (\"{path}\") ↦ textus")),
        &mut host,
    )
    .expect_err("solum:digestio was runner-only and is deleted (R1)");
    assert_eq!(errors[0].code, "runner_unsupported");
}

/// Marks the re-executed test binary that runs the console program itself.
const CONSOLE_CHILD: &str = "RADIX_ROUTE_SERVE_CONSOLE_CHILD";

// WHY: the stdio leaf writes `consolum:dic` / `consolum:mone` straight to the
// process's own fd 1 / fd 2, not into the `BufferHost` line buffers, so the
// streams are only observable from outside the process. The test re-executes
// its own binary and reads the child's real stdout and stderr.
#[test]
fn dic_writes_stdout_and_mone_writes_stderr() {
    if std::env::var_os(CONSOLE_CHILD).is_some() {
        let host = run_ok(
            "route-console.fab",
            &program(
                "    sit vacuum a ← ad 'consolum:dic' (\"salve\") ↦ vacuum\n    \
                 sit vacuum b ← ad 'consolum:mone' (\"cave\") ↦ vacuum\n    \
                 nota \"fin\"",
            ),
        );
        assert_eq!(host.stdout_lines, ["fin"]);
        assert!(host.stderr_lines.is_empty(), "{:?}", host.stderr_lines);
        return;
    }
    let module = module_path!()
        .split_once("::")
        .map_or(module_path!(), |(_, path)| path);
    let output = std::process::Command::new(std::env::current_exe().expect("test binary path"))
        .args([
            "--exact",
            &format!("{module}::dic_writes_stdout_and_mone_writes_stderr"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CONSOLE_CHILD, "1")
        .output()
        .expect("re-run the test binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "child failed:\n{stdout}\n{stderr}");
    assert!(stdout.contains("salve"), "dic must write stdout: {stdout:?}");
    assert!(!stdout.contains("cave"), "mone must not write stdout: {stdout:?}");
    assert!(stderr.contains("cave\n"), "mone must write a stderr line: {stderr:?}");
    assert!(!stderr.contains("salve"), "dic must not write stderr: {stderr:?}");
}

#[test]
fn exi_exits_with_the_code() {
    let mut host = BufferHost::default();
    let source = program("    sit vacuum a ← ad 'processus:exi' (7) ↦ vacuum");
    let panic = catch_unwind(AssertUnwindSafe(|| {
        let _ = run_host("route-exi.fab", &source, &mut host);
    }))
    .expect_err("exi must reach Host::exit");
    let message = panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
        .unwrap_or_default();
    assert_eq!(message, "runner exit: 7");
}

#[test]
fn unknown_route_stays_unrouted() {
    let mut host = BufferHost::default();
    let errors = run_host(
        "route-unknown.fab",
        &program("    nota ad 'nusquam:via' (\"x\") ↦ textus"),
        &mut host,
    )
    .expect_err("an unknown route must fail closed");
    assert_eq!(errors[0].code, "runner_unsupported");
    assert_eq!(
        errors[0].args,
        [(
            "feature".to_owned(),
            "sermo route nusquam:via before runner provider runtime".to_owned()
        )]
    );
}
