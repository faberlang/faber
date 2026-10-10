//! Run pins for the bulk byte decode with a `lista<b8>` source (need
//! 6a13d711, octeti retirement slice 2d): a little-endian decode of a byte
//! list value, and the clause-free 8-bit element row.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, run_source};

const LE_DECODE: &str = r#"+++
locale = "en"
+++
main {
    const list<b8> raw ← [0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40]
    const tensor<f32, [2]> v ← raw ↦ tensor<f32, [2]> via Le
    print v[[0]]
    print v[[1]]
    const tensor<u8, [8]> b ← raw ↦ tensor<u8, [8]>
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
