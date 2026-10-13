//! Leaf-route opener conversion: a `lista<b8>` byte list must reach the leaf
//! as bytes whether it is the whole opener or one element of an argument list.

use super::*;
use crate::host::BufferHost;

fn lista(items: Vec<Value>) -> Value {
    Value::Array(Rc::new(RefCell::new(items)))
}

fn byte_list(bytes: &[u8]) -> Value {
    lista(bytes.iter().map(|b| Value::UInt(u64::from(*b))).collect())
}

fn serve(route: &str, opener: &Value) -> Result<Vec<Value>, Value> {
    let mut host = BufferHost::default();
    serve_route(&mut host, route, opener, Span::default()).expect("leaf route")
}

#[test]
fn consolum_funde_writes_a_whole_opener_byte_list() {
    assert_eq!(serve("consolum:funde", &byte_list(b"ok\n")), Ok(Vec::new()));
}

#[test]
fn consolum_funde_writes_a_signed_int_byte_list() {
    let opener = lista(b"ok\n".iter().map(|b| Value::Int(i64::from(*b))).collect());
    assert_eq!(serve("consolum:funde", &opener), Ok(Vec::new()));
}

#[test]
fn consolum_funde_writes_a_single_byte_list() {
    assert_eq!(serve("consolum:funde", &byte_list(b"\n")), Ok(Vec::new()));
}

#[test]
fn solum_funde_writes_a_nested_signed_int_byte_list() {
    // Reviewer repro: a `list<int>` payload must reach the write as bytes,
    // exactly like an explicit `bytes` value — not as a positional list.
    let path =
        std::env::temp_dir().join(format!("radix-routes-test-{}-int.bin", std::process::id()));
    let path_text = path.to_string_lossy().into_owned();
    let opener = lista(vec![
        Value::Str(path_text.clone()),
        lista(vec![Value::Int(0x6f), Value::Int(0x6b)]),
    ]);
    assert_eq!(serve("solum:funde", &opener), Ok(Vec::new()));
    let read = serve("solum:hauri", &Value::Str(path_text));
    std::fs::remove_file(&path).expect("remove scratch file");
    assert_eq!(read, Ok(vec![Value::Octeti(vec![0x6f, 0x6b])]));
}

#[test]
fn solum_funde_does_not_convert_a_non_byte_int_list() {
    // A list with an out-of-range element, or a non-integer element, is not
    // a byte list: it keeps the generic list path and the leaf refuses it —
    // nothing is written.
    let path = std::env::temp_dir().join(format!(
        "radix-routes-test-{}-nonbyte.bin",
        std::process::id()
    ));
    let path_text = path.to_string_lossy().into_owned();
    for data in [
        lista(vec![Value::Int(300), Value::Int(0x6b)]),
        lista(vec![
            Value::Int(0x6f),
            Value::Str("not an integer".to_owned()),
        ]),
    ] {
        let opener = lista(vec![Value::Str(path_text.clone()), data]);
        assert!(
            serve("solum:funde", &opener).is_err(),
            "non-byte list must be refused"
        );
    }
    assert!(
        !path.exists(),
        "a refused non-byte list must not write the file"
    );
}

#[test]
fn solum_funde_round_trips_a_nested_byte_list() {
    let path = std::env::temp_dir().join(format!("radix-routes-test-{}.bin", std::process::id()));
    let path_text = path.to_string_lossy().into_owned();
    let data = [0x68_u8, 0x69, 0x00, 0xff];
    let opener = lista(vec![Value::Str(path_text.clone()), byte_list(&data)]);
    assert_eq!(serve("solum:funde", &opener), Ok(Vec::new()));
    let read = serve("solum:hauri", &Value::Str(path_text));
    std::fs::remove_file(&path).expect("remove scratch file");
    assert_eq!(read, Ok(vec![Value::Octeti(data.to_vec())]));
}
