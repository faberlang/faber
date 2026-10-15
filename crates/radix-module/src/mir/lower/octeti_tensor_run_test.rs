//! Run pins for the bulk byte decode with a `lista<b8>` source (need
//! 6a13d711, octeti retirement slice 2d): a little-endian decode of a byte
//! list value, and the clause-free 8-bit element row. Also the run-time
//! extent form: the target shape names `size` locals whose
//! values are known only at run time.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, run_source};

const LE_DECODE: &str = r#"+++
locale = "en"
+++
main {
    let list<b8> raw ← [0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40]
    let tensor<f32, [2]> v ← raw ↦ tensor<f32, [2]> via Le
    print v[[0]]
    print v[[1]]
    let tensor<u8, [8]> b ← raw ↦ tensor<u8, [8]>
    print b[[3]]
}
"#;

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

#[test]
fn byte_list_le_decode_runs() {
    let mut host = BufferHost::default();
    run_source(&session(), "octeti-tensor-le.fab", LE_DECODE, &mut host).expect("lista<b8> decodes into a tensor");
    assert_eq!(host.stdout_lines, ["1.0", "2.0", "63"]);
}

/// `octeti ↦ tensor<f32, [2, size n]> via Le` with `n` bound at run time: the
/// shape travels as a second operand, the byte length is checked against it,
/// and a wrong length is a recoverable failure.
const RUNTIME_EXTENT: &str = r#"+++
locale = "en"
+++
fn runtime_extent(int value) → int {
    return value
}

main {
    let bytes raw ← |00 00 80 3f 00 00 00 40 00 00 40 40 00 00 80 40 00 00 a0 40 00 00 c0 40|
    let int n ← runtime_extent(3)
    let tensor<f32, [2, size n]> v ← raw ↦ tensor<f32, [2, size n]> via Le
    print v[[0, 0]]
    print v[[1, 2]]
    print v.shape()[1]

    do {
        let int m ← runtime_extent(2)
        let tensor<f32, [2, size m]> bad ← raw ↦ tensor<f32, [2, size m]> via Le
        print bad[[0, 0]]
    }
    catch err {
        print "caught"
    }
}
"#;

#[test]
fn runtime_extent_decode_runs_and_length_mismatch_is_catchable() {
    let mut host = BufferHost::default();
    run_source(&session(), "octeti-tensor-runtime-extent.fab", RUNTIME_EXTENT, &mut host)
        .expect("a run-time extent target decodes");
    assert_eq!(host.stdout_lines, ["1.0", "6.0", "3", "caught"]);
}
