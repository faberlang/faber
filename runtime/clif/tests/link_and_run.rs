//! C1 proof: a C0 object linked with the runtime archive via `cc` runs and
//! prints the scalar result (goal `radix-mir-clif`, unit C1).
//!
//! `cargo build -p faber-host-clif` must run first: only `cargo build`
//! produces the staticlib (`cargo test` alone links the rlib). A missing
//! archive fails the test loudly rather than skipping.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn archive_path() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target = manifest
        .parent()
        .and_then(Path::parent)
        .expect("runtime/clif sits two levels below the workspace root")
        .join("target");
    for profile in ["debug", "release"] {
        let candidate = target.join(profile).join("libfaber_rt_clif.a");
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("libfaber_rt_clif.a not built; run `cargo build -p faber-host-clif` before the test");
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
        .arg(archive_path())
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
