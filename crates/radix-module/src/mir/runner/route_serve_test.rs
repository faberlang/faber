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
fn partem_reads_the_requested_range() {
    let (_dir, path) = fixture(b"abcdefgh");
    let host = run_ok(
        "route-partem.fab",
        &program(&format!(
            "    fixum lista<valor> args ← [\"{path}\", 2, 3]\n    \
             fixum octeti bytes ← ad 'solum:partem' (args) ↦ octeti\n    \
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
            "    fixum lista<valor> args ← [\"{path}\", 0, {over}]\n    \
             nota ad 'solum:partem' (args) ↦ octeti"
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
            "    fixum octeti all ← ad 'solum:hauri' (\"{path}\") ↦ octeti\n    \
             nota all[0] ↦ u8\n    nota all[2] ↦ u8\n    \
             fixum lista<valor> args ← [\"{path}\", 3]\n    \
             fixum octeti capped ← ad 'solum:hauri' (args) ↦ octeti\n    \
             nota capped[1] ↦ u8"
        )),
    );
    assert_eq!(host.stdout_lines, [b'x'.to_string(), b'z'.to_string(), b'y'.to_string()]);

    let mut host = BufferHost::default();
    let errors = run_host(
        "route-hauri-ceiling.fab",
        &program(&format!(
            "    fixum lista<valor> args ← [\"{path}\", 2]\n    \
             nota ad 'solum:hauri' (args) ↦ octeti"
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

#[test]
fn dic_writes_stdout_and_mone_writes_stderr() {
    let host = run_ok(
        "route-console.fab",
        &program(
            "    fixum vacuum a ← ad 'consolum:dic' (\"salve\") ↦ vacuum\n    \
             fixum vacuum b ← ad 'consolum:mone' (\"cave\") ↦ vacuum\n    \
             nota \"fin\"",
        ),
    );
    assert_eq!(host.stdout_lines, ["salvefin"]);
    assert_eq!(host.stderr_lines, ["cave"]);
}

#[test]
fn exi_exits_with_the_code() {
    let mut host = BufferHost::default();
    let source = program("    fixum vacuum a ← ad 'processus:exi' (7) ↦ vacuum");
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
