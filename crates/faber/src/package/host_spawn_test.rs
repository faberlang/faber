use super::{
    execute_device_at, resolve_host_cli_at, DeviceExecuteReceipt, DeviceExecuteRequest,
    HostSpawnError,
};
use std::path::PathBuf;

fn stub_host_cli(stdout: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("faber-host-macos-arm64");
    let script = format!("#!/bin/sh\ncat <<'JSON'\n{stdout}\nJSON\nexit 2\n");
    std::fs::write(&path, script).expect("write stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    (dir, path)
}

fn spawn_via_stub_cli(request: DeviceExecuteRequest, stdout: &str) -> HostSpawnError {
    let (_dir, host) = stub_host_cli(stdout);
    execute_device_at(&host, &request).expect_err("cli error")
}

fn malformed_request() -> DeviceExecuteRequest {
    DeviceExecuteRequest {
        backend: "metal".to_owned(),
        descriptor_json: b"{not-json".to_vec(),
        module_image: b"// fake".to_vec(),
        inputs_json: b"{}".to_vec(),
    }
}

fn empty_kernels_request() -> DeviceExecuteRequest {
    DeviceExecuteRequest {
        backend: "metal".to_owned(),
        descriptor_json: br#"{
          "backend": "metal",
          "kernels": [],
          "launches": [{"id":1,"kernel_index":0}],
          "buffer_versions": [],
          "program_lifetime": "single-run",
          "data_flow": [],
          "roots": [1],
          "results": [],
          "end_of_run_results": []
        }"#
        .to_vec(),
        module_image: b"// fake".to_vec(),
        inputs_json: br#"{"1":[1.0,2.0],"2":[3.0,4.0]}"#.to_vec(),
    }
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
    // Hosts `device-execute` maps invalid descriptor JSON to exit 2 + this
    // HostError object (`cli_device_execute_malformed_descriptor_exits_2`).
    let stdout = r#"{
        "code": "E_INVALID_ARGS",
        "message": "device-execute descriptor JSON is invalid: expected value at line 1 column 2",
        "retryable": false
    }"#;
    let err = spawn_via_stub_cli(malformed_request(), stdout);
    assert_eq!(err.code, "E_INVALID_ARGS");
    assert!(err.message.contains("descriptor JSON"), "{err}");
}

#[test]
fn spawn_invalid_descriptor_fails_closed_via_cli() {
    // Hosts `device-execute` maps a structurally empty program to exit 2 +
    // this HostError object (`cli_device_execute_invalid_descriptor_fails_closed`).
    let stdout = r#"{
        "code": "E_DEVICE_DESCRIPTOR",
        "message": "device descriptor declares no kernels",
        "retryable": false
    }"#;
    let err = spawn_via_stub_cli(empty_kernels_request(), stdout);
    assert_eq!(err.code, "E_DEVICE_DESCRIPTOR");
    assert_ne!(err.code, "E_INVALID_ARGS");
}

#[test]
fn host_spawn_error_displays_code_and_message() {
    let err = HostSpawnError {
        code: "E_HOST_SPAWN".to_owned(),
        message: "missing binary".to_owned(),
    };
    assert_eq!(err.to_string(), "E_HOST_SPAWN: missing binary");
}
