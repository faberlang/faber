use super::{
    DeviceExecuteInvocation, DeviceExecuteInvocationMode, DeviceExecuteProgram,
    DeviceExecuteProtocol, DeviceExecuteReceipt, DeviceExecuteRequest, DeviceExecuteV2Request,
    DistributedExecuteRequest, DistributedPrepareReceipt, HostSpawnError, WeightFileRange,
    decode_device_inputs, encode_device_execute_invocation, encode_device_inputs,
    encode_weight_map, execute_device_at, execute_distributed_at, open_device_session_at,
    resolve_host_cli_at, resolve_host_cli_at_root, start_device_session_at,
    validate_protocol_invocation,
};
use std::collections::BTreeMap;
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

fn distributed_stub_cli() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("faber-host-macos-arm64");
    let script = r##"#!/bin/sh
case "$*" in
  *"device-execute --backend cuda --distributed-image "*" --bind-count 1")
    printf '%s\n' '{"physical_device_count":1,"physical_device_ids":["cuda:test"],"virtual_partition_count":8,"virtual_partition_ids":["partition:0"],"fixture_identity_class":"virtual","transport_class":"host_staged","hardware_isolation_claimed":false,"bind_shape":"8:1","communication_graph_edge_count":3,"snapshot_id":"snapshot:test","logical_distributed_plan_hash":"sha256:logical","bound_distributed_plan_hash":"sha256:bound","transaction_state":"prepared"}'
    exit 0
    ;;
  *) exit 64 ;;
esac
"##;
    std::fs::write(&path, script).expect("write distributed stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod distributed stub");
    }
    (dir, path)
}

fn n1_distributed_stub_cli() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("faber-host-macos-arm64");
    let script = r##"#!/bin/sh
case "$*" in
  *"device-execute --backend metal --distributed-image "*" --bind-count 1")
    printf '%s\n' '{"physical_device_count":1,"physical_device_ids":["metal:test"],"virtual_partition_count":1,"virtual_partition_ids":["implicit-local"],"fixture_identity_class":"virtual","transport_class":"none","hardware_isolation_claimed":false,"bind_shape":"1:1","communication_graph_edge_count":0,"snapshot_id":"snapshot:n1","logical_distributed_plan_hash":"sha256:logical-n1","bound_distributed_plan_hash":"sha256:bound-n1","transaction_state":"prepared"}'
    exit 0
    ;;
  *) exit 64 ;;
esac
"##;
    std::fs::write(&path, script).expect("write N=1 distributed stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod N=1 distributed stub");
    }
    (dir, path)
}

fn resident_stub_cli() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("faber-host-macos-arm64");
    let script = r##"#!/bin/sh
IFS= read -r command || exit 2
printf '%s\n' '{"operation":"load","lifecycle":{"prepares":1,"reuses":0,"resets":0,"releases":0,"module_reloads":0,"per_program_reallocs":0,"live_handles":3},"kernel_count":5219}'
steps=0
while IFS= read -r command; do
  case "$command" in
    *step*)
      steps=$((steps + 1))
      printf '%s\n' "{\"operation\":\"step\",\"lifecycle\":{\"prepares\":1,\"reuses\":$steps,\"resets\":0,\"releases\":0,\"module_reloads\":0,\"per_program_reallocs\":0,\"live_handles\":3},\"receipt\":{\"backend\":\"metal\",\"device_name\":\"fake\",\"launches\":1,\"launch_ids\":[1],\"launch_entries\":[\"addita\"],\"copy_ins\":1,\"outputs\":{\"3\":[4.0,6.0]},\"allocated_buffers\":[1,2,3],\"syncs\":1,\"transfers\":2,\"readbacks\":1,\"kernel_count\":5219,\"encode_us\":11,\"submit_us\":22,\"wait_us\":33,\"gpu_encode_submit_wait_us\":66,\"stage_timing\":{\"kernel_us\":7,\"transfer_us\":3,\"host_round_trip_us\":2}}}"
      ;;
    *reset*)
      printf '%s\n' '{"operation":"reset","lifecycle":{"prepares":1,"reuses":2,"resets":1,"releases":0,"module_reloads":0,"per_program_reallocs":0,"live_handles":3},"kernel_count":5219,"reset_cleared":0}'
      ;;
    *release*)
      printf '%s\n' '{"operation":"release","lifecycle":{"prepares":1,"reuses":2,"resets":1,"releases":1,"module_reloads":0,"per_program_reallocs":0,"live_handles":0},"kernel_count":5219}'
      exit 0
      ;;
    *) exit 2 ;;
  esac
done
"##;
    std::fs::write(&path, script).expect("write resident stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod resident stub");
    }
    (dir, path)
}

fn pretty_error_resident_stub_cli() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("faber-host-macos-arm64");
    let script = r##"#!/bin/sh
IFS= read -r command || exit 2
cat <<'JSON'
{
  "code": "E_DEVICE_DESCRIPTOR",
  "message": "prepared session descriptor is invalid",
  "retryable": false
}
JSON
exit 2
"##;
    std::fs::write(&path, script).expect("write pretty error resident stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod pretty error resident stub");
    }
    (dir, path)
}

fn order_violation_stub_cli() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("faber-host-macos-arm64");
    let script = r##"#!/bin/sh
loaded=0
while IFS= read -r command; do
  case "$command" in
    *'"op":"load"'*)
      if [ "$loaded" -eq 1 ]; then
        printf '%s\n' '{"code":"E_INVALID_ARGS","message":"resident device session is already loaded"}'
      else
        loaded=1
        printf '%s\n' '{"operation":"load","lifecycle":{"prepares":1,"reuses":0,"resets":0,"releases":0,"module_reloads":0,"per_program_reallocs":0,"live_handles":1}}'
      fi
      ;;
    *'"op":"step"'*)
      if [ "$loaded" -eq 0 ]; then
        printf '%s\n' '{"code":"E_INVALID_ARGS","message":"resident step requires load first"}'
      else
        printf '%s\n' '{"operation":"step","lifecycle":{"prepares":1,"reuses":1,"resets":0,"releases":0,"module_reloads":0,"per_program_reallocs":0,"live_handles":1}}'
      fi
      ;;
    *'"op":"reset"'*)
      case "$command" in
        *'"inputs":'*)
          printf '%s\n' '{"code":"E_INVALID_ARGS","message":"resident reset does not accept inputs"}'
          ;;
        *)
          printf '%s\n' '{"operation":"reset","lifecycle":{"prepares":1,"reuses":0,"resets":1,"releases":0,"module_reloads":0,"per_program_reallocs":0,"live_handles":1}}'
          ;;
      esac
      ;;
    *'"op":"end"'*)
      printf '%s\n' '{"code":"E_INVALID_ARGS","message":"resident end requires release first"}'
      ;;
    *'"op":"release"'*)
      printf '%s\n' '{"operation":"release","lifecycle":{"prepares":1,"reuses":0,"resets":0,"releases":1,"module_reloads":0,"per_program_reallocs":0,"live_handles":0}}'
      exit 0
      ;;
    *)
      printf '%s\n' '{"code":"E_INVALID_ARGS","message":"unknown resident operation"}'
      ;;
  esac
done
"##;
    std::fs::write(&path, script).expect("write order violation stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod order violation stub");
    }
    (dir, path)
}

fn assert_order_error(error: HostSpawnError, message: &str) {
    assert_eq!(error.code, "E_INVALID_ARGS");
    assert_eq!(error.message, message);
}

fn malformed_request() -> DeviceExecuteRequest {
    DeviceExecuteRequest {
        backend: "metal".to_owned(),
        descriptor_json: b"{not-json".to_vec(),
        module_image: b"// fake".to_vec(),
        inputs_json: b"{}".to_vec(),
        weights_path: None,
        weight_map_json: None,
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
        weights_path: None,
        weight_map_json: None,
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
fn protocol_v2_invocation_encodes_token_and_position_without_inputs() {
    let invocation = DeviceExecuteInvocation {
        mode: DeviceExecuteInvocationMode::ScalarDecode,
        token: Some(42),
        position: 17,
        sequence_epoch: 3,
        prefix_before: 17,
        valid_len_after: 18,
        query_start: 17,
    };
    let bytes = encode_device_execute_invocation(&invocation).expect("encode v2 invocation");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(value["mode"], "scalar_decode");
    assert_eq!(value["token"], 42);
    assert_eq!(value["position"], 17);
    assert!(
        value.get("inputs").is_none(),
        "decode has no legacy inputs map"
    );
}

#[test]
fn protocol_v1_rejects_kv_invocation_before_spawn() {
    let invocation = DeviceExecuteInvocation {
        mode: DeviceExecuteInvocationMode::ScalarDecode,
        token: Some(7),
        position: 2,
        sequence_epoch: 1,
        prefix_before: 2,
        valid_len_after: 3,
        query_start: 2,
    };
    let error = validate_protocol_invocation(DeviceExecuteProtocol::V1, &invocation)
        .expect_err("v1 cannot carry KV execution");
    assert_eq!(error.code, "E_INVALID_ARGS");
    assert!(error.message.contains("protocol v1"), "{error}");
}

#[test]
fn protocol_v2_request_requires_both_programs_and_shared_identities() {
    let request = DeviceExecuteV2Request {
        backend: "metal".to_owned(),
        prefill: DeviceExecuteProgram {
            descriptor_json: b"prefill".to_vec(),
            module_image: b"prefill.msl".to_vec(),
        },
        decode: DeviceExecuteProgram {
            descriptor_json: b"decode".to_vec(),
            module_image: b"decode.msl".to_vec(),
        },
        prefill_inputs_json: br#"{"1":[1]}"#.to_vec(),
        model_identity: "smollm2".to_owned(),
        session_identity: "prompt-7".to_owned(),
        weights_path: None,
        weight_map_json: None,
    };
    request.validate().expect("complete paired request");

    let mut missing = request.clone();
    missing.decode.module_image.clear();
    let error = missing.validate().expect_err("decode program is mandatory");
    assert_eq!(error.code, "E_DEVICE_DESCRIPTOR");

    let mut unnamed = request;
    unnamed.session_identity.clear();
    let error = unnamed
        .validate()
        .expect_err("session identity is mandatory");
    assert_eq!(error.code, "E_INVALID_ARGS");
}

#[test]
fn v2_receipt_json_preserves_identities_and_lifecycle_counts() {
    let json = br#"{
        "operation":"load",
        "protocol":2,
        "model_identity":"smollm2",
        "session_identity":"prompt-7",
        "program_identities":{"prefill":"sha256:p","scalar_decode":"sha256:d"},
        "lifecycle":{"prepares":1,"reuses":4,"resets":1,"releases":0,"live_handles":9}
    }"#;
    let receipt: super::DeviceExecuteControlReceipt =
        serde_json::from_slice(json).expect("receipt");
    assert_eq!(receipt.protocol, 2);
    assert_eq!(receipt.model_identity.as_deref(), Some("smollm2"));
    assert_eq!(receipt.session_identity.as_deref(), Some("prompt-7"));
    assert_eq!(receipt.lifecycle.prepares, 1);
    assert_eq!(receipt.lifecycle.reuses, 4);
    assert_eq!(receipt.program_identities.len(), 2);
}

#[test]
fn missing_explicit_host_binary_is_a_spawn_error() {
    let err = resolve_host_cli_at(
        "metal",
        Some(PathBuf::from("/no/such/faber-host-macos-arm64")),
        false,
    )
    .expect_err("missing binary");
    assert_eq!(err.code, "E_HOST_SPAWN");
}

#[test]
fn cuda_without_override_fails_closed_with_backend_named_error() {
    let hosts = tempfile::tempdir().expect("tempdir");
    let err = resolve_host_cli_at_root("cuda", None, false, hosts.path())
        .expect_err("cuda has no product binary without an override");
    assert_eq!(err.code, "E_HOST_SPAWN");
    assert!(err.message.contains("cuda"), "{}", err.message);
    assert!(err.message.contains("FABER_HOST_CUDA"), "{}", err.message);
    assert!(!err.message.contains("Metal"), "{}", err.message);
}

#[test]
fn cuda_override_resolves_the_given_host_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let host = dir.path().join("cuda-host");
    std::fs::write(&host, b"stub").expect("write host stub");
    let resolved = resolve_host_cli_at("cuda", Some(host.clone()), false).expect("cuda host");
    assert_eq!(resolved, host);
}

#[test]
fn backend_host_rows_do_not_cross_binary_names() {
    let cuda_only = tempfile::tempdir().expect("cuda tempdir");
    std::fs::create_dir_all(cuda_only.path().join("target/debug")).expect("target dir");
    std::fs::write(
        cuda_only.path().join("target/debug/faber-host-cuda"),
        b"stub",
    )
    .expect("write cuda row");
    let err = resolve_host_cli_at_root("metal", None, false, cuda_only.path())
        .expect_err("metal must not use the cuda row");
    assert!(
        err.message.contains("FABER_HOST_MACOS_ARM64"),
        "{}",
        err.message
    );

    let metal_only = tempfile::tempdir().expect("metal tempdir");
    std::fs::create_dir_all(metal_only.path().join("target/debug")).expect("target dir");
    std::fs::write(
        metal_only
            .path()
            .join("target/debug/faber-host-macos-arm64"),
        b"stub",
    )
    .expect("write metal row");
    let err = resolve_host_cli_at_root("cuda", None, false, metal_only.path())
        .expect_err("cuda must not use the metal row");
    assert!(err.message.contains("FABER_HOST_CUDA"), "{}", err.message);
}

#[test]
fn distributed_spawn_passes_image_and_bind_count_and_decodes_receipt() {
    let (_dir, host) = distributed_stub_cli();
    let request = DistributedExecuteRequest {
        backend: "cuda".to_owned(),
        image: b"fmir-distributed".to_vec(),
        bind_count: 1,
    };
    let receipt: DistributedPrepareReceipt =
        execute_distributed_at(&host, &request).expect("distributed CLI receipt");
    assert_eq!(receipt.bind_shape, "8:1");
    assert_eq!(receipt.transaction_state, "prepared");
    assert!(!receipt.hardware_isolation_claimed);
}

#[test]
fn n1_distributed_spawn_preserves_zero_communication_edge_receipt() {
    let (_dir, host) = n1_distributed_stub_cli();
    let request = DistributedExecuteRequest {
        backend: "metal".to_owned(),
        image: b"fmir-one-partition".to_vec(),
        bind_count: 1,
    };
    let receipt: DistributedPrepareReceipt =
        execute_distributed_at(&host, &request).expect("N=1 CLI receipt");
    assert_eq!(receipt.physical_device_count, 1);
    assert_eq!(receipt.virtual_partition_count, 1);
    assert_eq!(receipt.bind_shape, "1:1");
    assert_eq!(receipt.communication_graph_edge_count, 0);
    assert!(!receipt.hardware_isolation_claimed);
    assert_eq!(receipt.transaction_state, "prepared");
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

#[test]
fn resident_control_client_keeps_one_child_across_load_steps_reset_release() {
    let (_dir, host) = resident_stub_cli();
    let request = malformed_request();
    let (mut session, load) = open_device_session_at(&host, &request).expect("load");
    assert_eq!(load.operation, "load");
    assert_eq!(load.kernel_count, 5219);
    assert_eq!(load.lifecycle.prepares, 1);
    assert_eq!(load.lifecycle.live_handles, 3);

    let inputs = BTreeMap::from([(2, vec![1.0, 2.0])]);
    let step1 = session.step(&inputs).expect("step 1");
    let step2 = session.step(&inputs).expect("step 2");
    for (index, step) in [step1, step2].into_iter().enumerate() {
        assert_eq!(step.operation, "step");
        assert_eq!(step.lifecycle.reuses, index + 1);
        assert_eq!(step.lifecycle.module_reloads, 0);
        assert_eq!(step.lifecycle.per_program_reallocs, 0);
        let receipt = step.receipt.expect("step receipt");
        assert_eq!(receipt.kernel_count, 5219);
        assert_eq!(receipt.encode_us, 11);
        assert_eq!(receipt.submit_us, 22);
        assert_eq!(receipt.wait_us, 33);
        assert_eq!(receipt.gpu_encode_submit_wait_us, 66);
        assert_eq!(receipt.stage_timing.total_us(), 12);
    }
    let reset = session.reset().expect("reset");
    assert_eq!(reset.lifecycle.resets, 1);
    assert_eq!(reset.reset_cleared, 0);
    let release = session.release().expect("release");
    assert_eq!(release.lifecycle.releases, 1);
    assert_eq!(release.lifecycle.live_handles, 0);
    assert!(session.release().is_err(), "a released owner refuses reuse");
}

#[test]
fn resident_control_client_accepts_pretty_terminal_host_error() {
    let (_dir, host) = pretty_error_resident_stub_cli();
    let request = malformed_request();
    let error = match open_device_session_at(&host, &request) {
        Ok(_) => panic!("pretty terminal HostError must be surfaced structurally"),
        Err(error) => error,
    };
    assert_eq!(error.code, "E_DEVICE_DESCRIPTOR");
    assert_eq!(error.message, "prepared session descriptor is invalid");
}

#[test]
fn resident_step_before_load_returns_structured_order_error() {
    let (_dir, host) = order_violation_stub_cli();
    let request = malformed_request();
    let mut session = start_device_session_at(&host, &request).expect("start session");
    let inputs = BTreeMap::from([(2, vec![1.0, 2.0])]);
    let error = session.step(&inputs).expect_err("step before load");
    assert_order_error(error, "resident step requires load first");
}

#[test]
fn resident_double_load_returns_structured_order_error() {
    let (_dir, host) = order_violation_stub_cli();
    let request = malformed_request();
    let mut session = start_device_session_at(&host, &request).expect("start session");
    session.control("load", None).expect("first load");
    let error = session.control("load", None).expect_err("double load");
    assert_order_error(error, "resident device session is already loaded");
}

#[test]
fn resident_reset_with_inputs_returns_structured_order_error() {
    let (_dir, host) = order_violation_stub_cli();
    let request = malformed_request();
    let mut session = start_device_session_at(&host, &request).expect("start session");
    session.control("load", None).expect("load");
    let inputs = BTreeMap::from([(2, vec![1.0, 2.0])]);
    let error = session
        .control("reset", Some(&inputs))
        .expect_err("reset with inputs");
    assert_order_error(error, "resident reset does not accept inputs");
}

#[test]
fn resident_end_without_release_returns_structured_order_error() {
    let (_dir, host) = order_violation_stub_cli();
    let request = malformed_request();
    let mut session = start_device_session_at(&host, &request).expect("start session");
    session.control("load", None).expect("load");
    let error = session
        .control("end", None)
        .expect_err("end without release");
    assert_order_error(error, "resident end requires release first");
}

fn assert_bit_exact(inputs: &BTreeMap<u32, Vec<f32>>) {
    let json = encode_device_inputs(inputs).expect("encode");
    let decoded = decode_device_inputs(&json).expect("decode");
    assert_eq!(decoded.len(), inputs.len());
    for (id, expected) in inputs {
        let got = decoded.get(id).unwrap_or_else(|| panic!("missing {id}"));
        assert_eq!(got.len(), expected.len(), "buffer {id} length");
        for (index, (observed, want)) in got.iter().zip(expected).enumerate() {
            assert_eq!(
                observed.to_bits(),
                want.to_bits(),
                "inputs[{id}][{index}] bits diverged"
            );
        }
    }
}

/// First spawn-wire divergence: packed GGUF word `0xff810000` (SmolLM2
/// file offset 1_774_948) encoded as `"NaN"` and decoded as `0x7fc00000`.
#[test]
fn spawn_inputs_preserve_smollm2_first_nan_payload() {
    const FIRST: u32 = 0xff81_0000;
    let inputs = BTreeMap::from([(1, vec![f32::from_bits(FIRST)])]);
    let json = encode_device_inputs(&inputs).expect("encode");
    let text = String::from_utf8(json.clone()).expect("utf8");
    assert!(text.contains("\"0xff810000\""), "{text}");
    assert!(!text.contains("\"NaN\""), "{text}");
    assert_bit_exact(&inputs);
}

#[test]
fn spawn_inputs_round_trip_smollm2_prefill_tokens_and_payloads() {
    let tokens: Vec<f32> = [504u32, 2365, 6354, 16438, 27003, 690, 260, 23790, 2767]
        .into_iter()
        .map(f32::from_bits)
        .collect();
    let payloads = vec![
        f32::from_bits(0xff81_0000),
        f32::from_bits(0x7fc0_0001),
        f32::from_bits(0x7f80_0001),
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        -0.0,
        1.0e-5,
        1.5,
    ];
    let inputs = BTreeMap::from([(1, tokens), (2, payloads)]);
    assert_bit_exact(&inputs);
}

#[test]
fn spawn_inputs_accept_legacy_nan_string() {
    let decoded = decode_device_inputs(br#"{"1":["NaN"]}"#).expect("decode");
    assert!(decoded[&1][0].is_nan());
}

#[test]
fn weight_map_json_names_byte_ranges_not_payloads() {
    let map = BTreeMap::from([(
        7,
        WeightFileRange {
            offset: 1_774_948,
            len: 12,
            elems: 4,
        },
    )]);
    let json = encode_weight_map(&map).expect("encode");
    let text = String::from_utf8(json).expect("utf8");
    assert!(text.contains("\"7\""), "{text}");
    assert!(text.contains("1774948"), "{text}");
    assert!(!text.contains("0xff810000"), "{text}");
}

/// Round-trip a real SmolLM2 packed prefix when the pinned GGUF is present.
#[test]
fn spawn_inputs_round_trip_smollm2_packed_prefix() {
    const PATH: &str = "~/ai/models/SmolLM2-360M-Instruct-Q4_K_M.gguf";
    const FIRST_NAN_OFFSET: usize = 1_774_948;
    let Ok(bytes) = std::fs::read(PATH) else {
        return;
    };
    assert!(
        bytes.len() > FIRST_NAN_OFFSET + 4,
        "pinned SmolLM2 GGUF is shorter than the named NaN offset"
    );
    let word = u32::from_le_bytes(
        bytes[FIRST_NAN_OFFSET..FIRST_NAN_OFFSET + 4]
            .try_into()
            .expect("4 bytes"),
    );
    assert_eq!(word, 0xff81_0000, "first packed NaN word moved");
    let start = FIRST_NAN_OFFSET.saturating_sub(64);
    let end = (FIRST_NAN_OFFSET + 256).min(bytes.len());
    let mut padded = bytes[start..end].to_vec();
    while padded.len() % 4 != 0 {
        padded.push(0);
    }
    let values: Vec<f32> = padded
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    assert_bit_exact(&BTreeMap::from([(7, values)]));
}
