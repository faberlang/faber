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
    let list<b8> raw ← |00 00 80 3f 00 00 00 40 00 00 40 40 00 00 80 40 00 00 a0 40 00 00 c0 40|
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

/// A bounded range of a real file becomes a tensor of fixed-layout records
///: `octeti ↦ tensor<Block, [n]> via Le` keeps the range's
/// bytes as the rows and a field read decodes in place. A range that is not a
/// whole number of rows is a catchable, named failure. Nothing here names a
/// file format; `Block` is any fixed-layout genus.
fn record_range_program(path: &str, offset: u32, length: u32) -> String {
    format!(
        r#"+++
locale = "en"
+++

@ public
class Block {{
    let f16 scale
    let list<i8, size 4> values
}}

fn read_blocks(tensor<Block, [2]> blocks, mut tensor<f32, [2]> out) → void {{
    for from blocks at [i] let b {{
        out[[i]] ← (b.scale ↦ f32) + (b.values[1] ↦ f32)
    }}
}}

main {{
    let list<value> args ← ["{path}", {offset}, {length}]
    let list<b8> raw ← call 'solum:partem' (args) ↦ list<b8>
    do {{
        let tensor<Block, [2]> blocks ← raw ↦ tensor<Block, [2]> via Le
        mut tensor<f32, [2]> out ← tf32[2] {{}}
        read_blocks(blocks, mut out)
        print out[[0]]
        print out[[1]]
    }}
    catch err {{
        print "caught"
    }}
}}
"#
    )
}

#[test]
fn real_file_range_loads_as_record_tensor_and_misaligned_range_is_catchable() {
    // Three bytes of lead, two 6-byte rows (f16 scale + four i8 slots), tail.
    let rows: [u8; 12] = [
        0x00, 0x3e, 0xfd, 0x04, 0x05, 0x06, // scale 1.5, values [-3, 4, 5, 6]
        0x00, 0x41, 0x0a, 0xec, 0x1e, 0xd8, // scale 2.5, values [10, -20, 30, -40]
    ];
    let mut file = vec![0xee_u8; 3];
    file.extend_from_slice(&rows);
    file.extend_from_slice(&[0xdd; 5]);
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("records.bin");
    std::fs::write(&path, &file).expect("write fixture");
    let path = path.to_string_lossy().into_owned();

    let mut host = BufferHost::default();
    run_source(
        &session(),
        "octeti-record-range.fab",
        &record_range_program(&path, 3, 12),
        &mut host,
    )
    .expect("a bounded file range loads as record rows");
    assert_eq!(host.stdout_lines, ["5.5", "-17.5"]);

    // Eleven bytes: not a whole number of 6-byte rows.
    let mut host = BufferHost::default();
    run_source(
        &session(),
        "octeti-record-range-short.fab",
        &record_range_program(&path, 3, 11),
        &mut host,
    )
    .expect("the failure is caught");
    assert_eq!(host.stdout_lines, ["caught"]);
}

/// A field store through a record tensor element reaches the tensor (need
/// d43ea3d1): a record row decodes to a fresh value on every read, so the
/// store writes the changed element back. The compound `↑`, a store inside a
/// kernel-shaped loop and an untouched neighbour row all agree.
const FIELD_WRITE_THROUGH: &str = r#"+++
locale = "en"
+++

class Cell {
    mut i16 n
    let list<i8, size 4> values
}

fn bump(mut tensor<Cell, [2]> cs) → void {
    for range 0‥2 let i {
        cs[[i]].n ← 9 + i
    }
}

main {
    let list<b8> raw ← [0x01, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x00, 0x05, 0x06, 0x07, 0x08]
    mut tensor<Cell, [2]> cells ← raw ↦ tensor<Cell, [2]> via Le
    print cells[[0]].n
    bump(mut cells)
    print cells[[0]].n
    print cells[[1]].n
    cells[[1]].n ↑
    print cells[[1]].n
    print cells[[1]].values[2]
}
"#;

#[test]
fn field_store_through_a_record_tensor_element_writes_back() {
    let mut host = BufferHost::default();
    run_source(&session(), "record-field-write.fab", FIELD_WRITE_THROUGH, &mut host)
        .expect("a field store through a record tensor element runs");
    assert_eq!(host.stdout_lines, ["1", "9", "10", "11", "7"]);
}
