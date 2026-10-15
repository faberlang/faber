//! C1 proof: a C0 object linked with the runtime archive via `cc` runs and
//! prints the scalar result (goal `radix-mir-clif`, unit C1).
//!
//! The archive is `runtime/clif`'s staticlib. This crate cannot name it as a
//! dev-dependency (compiler crates never depend on `runtime/*`), and a plain
//! `cargo test` links only the rlib, so the test builds the archive itself with
//! the same `cargo` into the profile directory it is running from. A failed
//! build fails the test loudly rather than skipping.

use std::path::PathBuf;
use std::process::Command;

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

#[test]
fn links_with_cc_and_prints_incipit_result() {
    let object = radix_mir_clif::emit_entry_object(42).expect("object emission succeeds");

    let dir = std::env::temp_dir().join("radix-mir-clif-c1");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let object_path = dir.join("faber_clif_incipit.o");
    std::fs::write(&object_path, &object).expect("write object");
    let program = dir.join("clif_hello");

    // The platform dead-strip flag is load-bearing (design review):
    // without it the same link measured 13 MB.
    let mut link = Command::new("cc");
    link.arg(&object_path)
        .arg(build_archive())
        .arg("-Wl,-dead_strip")
        .arg("-o")
        .arg(&program);
    let linked = link.output().expect("run cc");
    assert!(
        linked.status.success(),
        "cc link failed: {}{}",
        String::from_utf8_lossy(&linked.stdout),
        String::from_utf8_lossy(&linked.stderr),
    );

    let run = Command::new(&program).output().expect("run linked program");
    assert!(
        run.status.success(),
        "program exited {:#}: {}",
        run.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&run.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "42\n",
        "stdout must be the scalar result with a newline"
    );
}
