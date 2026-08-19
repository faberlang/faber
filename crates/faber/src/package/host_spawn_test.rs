use super::{
    decode_device_inputs, encode_device_inputs, encode_weight_map, execute_device_at,
    resolve_host_cli_at, DeviceExecuteReceipt, DeviceExecuteRequest, HostSpawnError,
    WeightFileRange,
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
fn missing_explicit_host_binary_is_a_spawn_error() {
    let err = resolve_host_cli_at(
        Some(PathBuf::from("/no/such/faber-host-macos-arm64")),
        false,
    )
    .expect_err("missing binary");
    assert_eq!(err.code, "E_HOST_SPAWN");
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
