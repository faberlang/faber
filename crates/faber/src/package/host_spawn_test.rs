use super::{
    execute_device, resolve_host_cli_at, DeviceExecuteReceipt, DeviceExecuteRequest, HostSpawnError,
};
use serde_json::Value;
use std::path::PathBuf;

fn add_descriptor_json() -> Vec<u8> {
    br#"{
      "backend": "metal",
      "kernels": [{
        "entry": "addita",
        "buffers": [
          {"buffer_id":1,"buffer_name":"a","semantic_value":1,"role":"input","lifetime":"per-program","initialization":"host-provided","binding":0,"element_ty":"f32","element_count":2,"version":1},
          {"buffer_id":2,"buffer_name":"b","semantic_value":2,"role":"input","lifetime":"per-program","initialization":"host-provided","binding":1,"element_ty":"f32","element_count":2,"version":1},
          {"buffer_id":3,"buffer_name":"out","semantic_value":3,"role":"output","lifetime":"observation-point","initialization":"kernel-initialized","binding":2,"element_ty":"f32","element_count":2,"version":1}
        ],
        "grid": [1,1,1],
        "block": [2,1,1]
      }],
      "launches": [{"id":1,"kernel_index":0}],
      "buffer_versions": [
        {"buffer_id":1,"version":1,"element_ty":"f32","element_count":2},
        {"buffer_id":2,"version":1,"element_ty":"f32","element_count":2},
        {"buffer_id":3,"version":1,"element_ty":"f32","element_count":2}
      ],
      "program_lifetime": "single-run",
      "data_flow": [],
      "roots": [1],
      "results": [{"buffer_id":3,"version":1,"produced_by":1,"at_launch":1}],
      "end_of_run_results": []
    }"#
    .to_vec()
}

fn add_inputs_json() -> Vec<u8> {
    br#"{"1":[1.0,2.0],"2":[3.0,4.0]}"#.to_vec()
}

#[test]
fn receipt_json_parses_string_keyed_outputs() {
    let json = br#"{
        "backend": "metal",
        "device_name": "fake",
        "launches": 1,
        "outputs": {"3": [4.0, 6.0]},
        "syncs": 2,
        "readbacks": 1
    }"#;
    let receipt: DeviceExecuteReceipt = serde_json::from_slice(json).expect("receipt");
    assert_eq!(receipt.output(3).expect("out"), &[4.0, 6.0]);
    assert_eq!(receipt.launches, 1);
}

#[test]
fn missing_explicit_host_binary_is_a_spawn_error() {
    let err = resolve_host_cli_at(
        Some(PathBuf::from("/no/such/faber-host-macos-arm64")),
        false,
    )
    .expect_err("missing binary");
    assert_eq!(err.code, "E_HOST_SPAWN");
    assert!(err.message.contains("FABER_HOST_MACOS_ARM64"), "{err}");
}

#[test]
fn spawn_malformed_descriptor_returns_structured_cli_error() {
    let request = DeviceExecuteRequest {
        backend: "metal".to_owned(),
        descriptor_json: b"{not-json".to_vec(),
        module_image: b"// fake".to_vec(),
        inputs_json: b"{}".to_vec(),
    };
    let err = execute_device(&request, false).expect_err("malformed");
    assert_eq!(err.code, "E_INVALID_ARGS");
    assert!(err.message.contains("descriptor JSON"), "{err}");
}

#[test]
fn spawn_invalid_descriptor_fails_closed_via_cli() {
    let mut descriptor: Value = serde_json::from_slice(&add_descriptor_json()).expect("fixture");
    descriptor["kernels"] = Value::Array(Vec::new());
    let request = DeviceExecuteRequest {
        backend: "metal".to_owned(),
        descriptor_json: serde_json::to_vec(&descriptor).expect("encode"),
        module_image: b"// fake".to_vec(),
        inputs_json: add_inputs_json(),
    };
    let err = execute_device(&request, false).expect_err("invalid descriptor");
    assert_eq!(err.code, "E_DEVICE_DESCRIPTOR");
}

#[test]
fn host_spawn_error_displays_code_and_message() {
    let err = HostSpawnError {
        code: "E_HOST_SPAWN".to_owned(),
        message: "missing binary".to_owned(),
    };
    assert_eq!(err.to_string(), "E_HOST_SPAWN: missing binary");
}
