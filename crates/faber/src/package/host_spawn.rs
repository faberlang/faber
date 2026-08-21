//! Spawn/connect client for the hosts `device-execute` CLI.
//!
//! No hosts Cargo, git, or registry dependency. The client writes the CLI
//! file surface (`--descriptor` JSON, `--module` raw bytes, `--inputs` JSON
//! for tiny per-step values, optional `--weights` GGUF + `--weight-map`)
//! and parses the receipt / HostError JSON the CLI prints.
//!
//! Error taxonomy (CLI owns the codes):
//! - exit 0 — receipt JSON
//! - exit 2 — HostError JSON on stdout; this client surfaces `code` /
//!   `message` verbatim (`E_INVALID_ARGS` vs `E_DEVICE_DESCRIPTOR` stay distinct)
//! - exit 64 / spawn failure / unreadable child output — `E_HOST_SPAWN`

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "device-runtime")]
use radix_mir_fmir::{
    FmirDeviceBackend, FmirDeviceSection, WireObservationCadence, WireProgramLifetime,
};
use serde::{Deserialize, Serialize};

/// Structured failure from the spawn client or the child CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSpawnError {
    /// Stable host / client code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

impl std::fmt::Display for HostSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// Files the hosts CLI consumes for one packed run.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceExecuteRequest {
    /// `--backend` spelling (`metal` / `cuda` / `auto`).
    pub backend: String,
    /// Descriptor JSON (no module image).
    pub descriptor_json: Vec<u8>,
    /// Raw module image (MSL or PTX).
    pub module_image: Vec<u8>,
    /// Inputs JSON (`{ "<buffer-id>": [f32, ...] }`) — tiny per-step values.
    pub inputs_json: Vec<u8>,
    /// GGUF file the child maps; not copied into the work dir.
    pub weights_path: Option<PathBuf>,
    /// `{ "<buffer-id>": { offset, len, elems } }` into `--weights`.
    pub weight_map_json: Option<Vec<u8>>,
}

/// One GGUF byte range the child copies into a native packed device region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeightFileRange {
    /// Byte offset into the GGUF file.
    pub offset: u64,
    /// Packed-byte length to copy.
    pub len: u64,
    /// Admitted packed-region width in f32 words (`ceil(len / 4)`).
    pub elems: u64,
}

/// Receipt timing split shared by one-shot and resident control steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StageTimingReceipt {
    pub kernel_us: u64,
    pub transfer_us: u64,
    pub host_round_trip_us: u64,
}

impl StageTimingReceipt {
    #[must_use]
    pub const fn total_us(self) -> u64 {
        self.kernel_us
            .saturating_add(self.transfer_us)
            .saturating_add(self.host_round_trip_us)
    }
}

/// Resident session lifecycle counters returned by the hosts control entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DeviceExecuteLifecycle {
    pub prepares: usize,
    pub reuses: usize,
    pub resets: usize,
    pub releases: usize,
    pub module_reloads: usize,
    pub per_program_reallocs: usize,
    pub live_handles: usize,
}

/// One response from the local load/step/reset/release stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceExecuteControlReceipt {
    pub operation: String,
    pub lifecycle: DeviceExecuteLifecycle,
    #[serde(default)]
    pub kernel_count: usize,
    #[serde(default)]
    pub reset_cleared: usize,
    #[serde(default)]
    pub receipt: Option<DeviceExecuteReceipt>,
}

/// Client-side handle for one long-lived hosts control owner.
///
/// The child process is intentionally reused for every operation. Dropping an
/// unreleased handle kills the child, but successful delivery requires an
/// explicit `release` so the receipt proves zero live handles.
pub struct DeviceExecuteSession {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    released: bool,
    _work: WorkDir,
}

/// Receipt the hosts CLI prints on exit 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceExecuteReceipt {
    /// Selected backend spelling.
    pub backend: String,
    /// Selected-hardware name.
    pub device_name: String,
    /// Launches dispatched.
    pub launches: usize,
    /// Descriptor launch identities, in order.
    #[serde(default)]
    pub launch_ids: Vec<u32>,
    /// Kernel entries dispatched, in order.
    #[serde(default)]
    pub launch_entries: Vec<String>,
    /// Host→device copy-ins.
    #[serde(default)]
    pub copy_ins: usize,
    /// Temporary PerStep/ObservationPoint handles allocated by this step.
    #[serde(default)]
    pub pool_allocations: usize,
    /// Temporary handles reused from the session-scoped pool this step.
    #[serde(default)]
    pub pool_reuses: usize,
    /// Declared observation outputs (`buffer id` → f32 values).
    pub outputs: BTreeMap<String, Vec<f32>>,
    /// Allocated program-level buffer ids.
    #[serde(default)]
    pub allocated_buffers: Vec<u32>,
    /// Observed device syncs.
    pub syncs: usize,
    /// Observed transfers.
    #[serde(default)]
    pub transfers: usize,
    /// Device→host readbacks.
    pub readbacks: usize,
    /// Program-graph SHA-256 receipt.
    #[serde(default)]
    pub program_graph_hash: String,
    /// Child file-read wall (descriptor + module + inputs).
    #[serde(default)]
    pub file_read_us: u64,
    /// Descriptor JSON decode wall.
    #[serde(default)]
    pub descriptor_decode_us: u64,
    /// Inputs JSON decode wall (tiny per-step values only).
    #[serde(default)]
    pub json_decode_us: u64,
    /// Composite-host construction wall (Metal device admit).
    #[serde(default)]
    pub host_construct_us: u64,
    /// Session create wall (module compile + PerProgram alloc).
    #[serde(default)]
    pub session_create_us: u64,
    /// Module compile + pipeline-create wall inside session create.
    #[serde(default)]
    pub load_module_us: u64,
    /// PerProgram allocation wall inside session create.
    #[serde(default)]
    pub per_program_alloc_us: u64,
    /// Host→device copy-in wall (packed-weight upload on SingleRun).
    #[serde(default)]
    pub copy_in_us: u64,
    /// Kernel encode + submit + blocking wait (true GPU step time).
    #[serde(default)]
    pub gpu_encode_submit_wait_us: u64,
    /// Per-encoder GPU timestamps in launch order (µs). Empty when unsampled.
    #[serde(default)]
    pub launch_gpu_us: Vec<u64>,
    /// Per-encoder GPU start times in launch order (µs, relative to the
    /// first encoder start). Empty when unsampled.
    #[serde(default)]
    pub launch_gpu_start_us: Vec<u64>,
    /// Observation readback wall.
    #[serde(default)]
    pub readback_us: u64,
    /// Child wall from first file read through teardown (excludes dyld).
    #[serde(default)]
    pub cli_internal_us: u64,
    /// Parent temp-file write wall (descriptor + module + inputs JSON).
    #[serde(default)]
    pub parent_write_files_us: u64,
    /// Parent `Command::output` wall (process start through child exit).
    #[serde(default)]
    pub parent_child_wait_us: u64,
    /// Number of declared physical kernel bodies in the admitted descriptor.
    #[serde(default)]
    pub kernel_count: usize,
    /// Stage timing for this one-shot or resident step.
    #[serde(default)]
    pub stage_timing: StageTimingReceipt,
}

impl DeviceExecuteReceipt {
    /// Read one observation buffer.
    pub fn output(&self, buffer_id: u32) -> Result<&[f32], HostSpawnError> {
        self.outputs
            .get(&buffer_id.to_string())
            .map(Vec::as_slice)
            .ok_or_else(|| HostSpawnError {
                code: "E_DEVICE_DESCRIPTOR".to_owned(),
                message: format!("logits buffer {buffer_id} was not read back"),
            })
    }
}

/// Build the CLI request from a packed FMIR device section.
#[cfg(feature = "device-runtime")]
pub fn request_from_section(
    section: &FmirDeviceSection,
    backend: &str,
    module_image: &[u8],
    inputs: &BTreeMap<u32, Vec<f32>>,
    weights_path: Option<&Path>,
    weight_map: Option<&BTreeMap<u32, WeightFileRange>>,
) -> Result<DeviceExecuteRequest, HostSpawnError> {
    let descriptor_json = descriptor_json_from_section(section, backend, weight_map)?;
    let inputs_json = encode_device_inputs(inputs)?;
    let weight_map_json = match weight_map {
        Some(map) if !map.is_empty() => Some(encode_weight_map(map)?),
        _ => None,
    };
    let weights_path = match (weights_path, &weight_map_json) {
        (Some(path), Some(_)) => Some(path.to_path_buf()),
        (None, None) | (Some(_), None) => None,
        (None, Some(_)) => {
            return Err(HostSpawnError {
                code: "E_HOST_SPAWN".to_owned(),
                message: "weight-map present without a GGUF path".to_owned(),
            });
        }
    };
    Ok(DeviceExecuteRequest {
        backend: backend.to_owned(),
        descriptor_json,
        module_image: module_image.to_vec(),
        inputs_json,
        weights_path,
        weight_map_json,
    })
}

/// Invoke `faber-host-macos-arm64 device-execute` as a child process.
#[allow(dead_code)] // live entry for device-runtime; lib tests use `execute_device_at`
pub fn execute_device(
    request: &DeviceExecuteRequest,
    build_if_missing: bool,
) -> Result<DeviceExecuteReceipt, HostSpawnError> {
    execute_device_at(&resolve_host_cli(build_if_missing)?, request)
}

/// Start one long-lived hosts control owner and perform its explicit `load`.
#[allow(dead_code)] // live from the dense device-runtime route
pub fn open_device_session(
    request: &DeviceExecuteRequest,
    build_if_missing: bool,
) -> Result<(DeviceExecuteSession, DeviceExecuteControlReceipt), HostSpawnError> {
    open_device_session_at(&resolve_host_cli(build_if_missing)?, request)
}

fn open_device_session_at(
    host: &Path,
    request: &DeviceExecuteRequest,
) -> Result<(DeviceExecuteSession, DeviceExecuteControlReceipt), HostSpawnError> {
    let mut session = start_device_session_at(host, request)?;
    let loaded = session.control("load", None)?;
    Ok((session, loaded))
}

/// Start a resident control owner without sending its first operation.
///
/// Keeping process startup separate from the initial `load` gives the client
/// tests a way to exercise the host's fail-closed operation ordering without
/// changing the production open-session contract.
fn start_device_session_at(
    host: &Path,
    request: &DeviceExecuteRequest,
) -> Result<DeviceExecuteSession, HostSpawnError> {
    let work = WorkDir::create()?;
    let descriptor_path = work.path.join("descriptor.json");
    let module_path = work.path.join("module.bin");
    let inputs_path = work.path.join("inputs.json");
    let weight_map_path = work.path.join("weight-map.json");
    write_file(&descriptor_path, &request.descriptor_json)?;
    write_file(&module_path, &request.module_image)?;
    write_file(&inputs_path, &request.inputs_json)?;
    if let Some(map) = &request.weight_map_json {
        write_file(&weight_map_path, map)?;
    }

    let mut command = Command::new(host);
    command
        .arg("device-execute")
        .arg("--control")
        .args(["--backend", &request.backend])
        .args(["--descriptor", &path_text(&descriptor_path)?])
        .args(["--module", &path_text(&module_path)?])
        .args(["--inputs", &path_text(&inputs_path)?])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let (Some(weights), Some(_)) = (&request.weights_path, &request.weight_map_json) {
        command.args([
            "--weights",
            &path_text(weights)?,
            "--weight-map",
            &path_text(&weight_map_path)?,
        ]);
    }
    let mut child = command.spawn().map_err(|error| HostSpawnError {
        code: "E_HOST_SPAWN".to_owned(),
        message: format!("failed to spawn resident device-execute: {error}"),
    })?;
    let stdin = child.stdin.take().ok_or_else(|| HostSpawnError {
        code: "E_HOST_SPAWN".to_owned(),
        message: "resident device-execute stdin was not piped".to_owned(),
    })?;
    let stdout = child.stdout.take().ok_or_else(|| HostSpawnError {
        code: "E_HOST_SPAWN".to_owned(),
        message: "resident device-execute stdout was not piped".to_owned(),
    })?;
    Ok(DeviceExecuteSession {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        released: false,
        _work: work,
    })
}

impl DeviceExecuteSession {
    /// Execute one repeated invocation through the loaded session.
    pub fn step(
        &mut self,
        inputs: &BTreeMap<u32, Vec<f32>>,
    ) -> Result<DeviceExecuteControlReceipt, HostSpawnError> {
        self.control("step", Some(inputs))
    }

    /// Reset resident state while retaining once-loaded weights.
    pub fn reset(&mut self) -> Result<DeviceExecuteControlReceipt, HostSpawnError> {
        self.control("reset", None)
    }

    /// Explicitly release the resident session and assert zero live handles.
    pub fn release(&mut self) -> Result<DeviceExecuteControlReceipt, HostSpawnError> {
        let receipt = self.control("release", None)?;
        self.released = true;
        Ok(receipt)
    }

    fn control(
        &mut self,
        operation: &str,
        inputs: Option<&BTreeMap<u32, Vec<f32>>>,
    ) -> Result<DeviceExecuteControlReceipt, HostSpawnError> {
        if self.released {
            return Err(HostSpawnError {
                code: "E_INVALID_ARGS".to_owned(),
                message: "resident device session is already released".to_owned(),
            });
        }
        let mut command = serde_json::Map::new();
        command.insert(
            "op".to_owned(),
            serde_json::Value::String(operation.to_owned()),
        );
        if let Some(inputs) = inputs {
            let encoded = encode_device_inputs(inputs)?;
            let values = serde_json::from_slice(&encoded).map_err(|error| HostSpawnError {
                code: "E_DEVICE_DESCRIPTOR".to_owned(),
                message: format!("failed to encode control inputs: {error}"),
            })?;
            command.insert("inputs".to_owned(), values);
        }
        serde_json::to_writer(&mut self.stdin, &command).map_err(|error| HostSpawnError {
            code: "E_HOST_SPAWN".to_owned(),
            message: format!("failed to write resident control request: {error}"),
        })?;
        self.stdin
            .write_all(b"\n")
            .map_err(|error| HostSpawnError {
                code: "E_HOST_SPAWN".to_owned(),
                message: format!("failed to write resident control request: {error}"),
            })?;
        self.stdin.flush().map_err(|error| HostSpawnError {
            code: "E_HOST_SPAWN".to_owned(),
            message: format!("failed to flush resident control request: {error}"),
        })?;
        // Success responses are compact NDJSON, while the CLI's terminal
        // HostError path uses the shared pretty JSON printer. Accumulate one
        // complete JSON value instead of treating the first pretty-printed
        // line (`{`) as a response. A second value in the same frame still
        // fails closed because serde_json rejects trailing input.
        let mut response = String::new();
        loop {
            let mut line = String::new();
            let read = self
                .stdout
                .read_line(&mut line)
                .map_err(|error| HostSpawnError {
                    code: "E_HOST_SPAWN".to_owned(),
                    message: format!("failed to read resident control receipt: {error}"),
                })?;
            if read == 0 {
                let status = self.child.try_wait().ok().flatten();
                return Err(HostSpawnError {
                    code: "E_HOST_SPAWN".to_owned(),
                    message: format!(
                        "resident device-execute exited before {operation} receipt: {status:?}"
                    ),
                });
            }
            response.push_str(&line);

            match serde_json::from_str::<WireHostError>(&response) {
                Ok(error) if !error.code.is_empty() => {
                    return Err(HostSpawnError {
                        code: error.code,
                        message: error.message,
                    });
                }
                Ok(_) | Err(_) => {}
            }

            match serde_json::from_str::<DeviceExecuteControlReceipt>(&response) {
                Ok(receipt) => return Ok(receipt),
                Err(error) if error.is_eof() => continue,
                Err(error) => {
                    return Err(HostSpawnError {
                        code: "E_HOST_SPAWN".to_owned(),
                        message: format!("resident control receipt JSON is invalid: {error}"),
                    });
                }
            }
        }
    }
}

impl Drop for DeviceExecuteSession {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

/// Invoke a resolved host binary. Tests pass a stub CLI here so the decode
/// contract does not depend on a prebuilt sibling hosts artifact.
fn execute_device_at(
    host: &Path,
    request: &DeviceExecuteRequest,
) -> Result<DeviceExecuteReceipt, HostSpawnError> {
    use std::time::Instant;

    let work = WorkDir::create()?;
    let descriptor_path = work.path.join("descriptor.json");
    let module_path = work.path.join("module.bin");
    let inputs_path = work.path.join("inputs.json");
    let weight_map_path = work.path.join("weight-map.json");
    let write_started = Instant::now();
    write_file(&descriptor_path, &request.descriptor_json)?;
    write_file(&module_path, &request.module_image)?;
    write_file(&inputs_path, &request.inputs_json)?;
    if let Some(map) = &request.weight_map_json {
        write_file(&weight_map_path, map)?;
    }
    let parent_write_files_us = elapsed_us(write_started);

    let child_started = Instant::now();
    let mut command = Command::new(host);
    command.args([
        "device-execute",
        "--backend",
        &request.backend,
        "--descriptor",
        &path_text(&descriptor_path)?,
        "--module",
        &path_text(&module_path)?,
        "--inputs",
        &path_text(&inputs_path)?,
    ]);
    if let (Some(weights), Some(_)) = (&request.weights_path, &request.weight_map_json) {
        command.args([
            "--weights",
            &path_text(weights)?,
            "--weight-map",
            &path_text(&weight_map_path)?,
        ]);
    }
    let output = command.output().map_err(|error| HostSpawnError {
        code: "E_HOST_SPAWN".to_owned(),
        message: format!("failed to spawn {}: {error}", host.display()),
    })?;
    let parent_child_wait_us = elapsed_us(child_started);

    let mut receipt =
        decode_device_execute_output(output.status.code(), &output.stdout, &output.stderr)?;
    receipt.parent_write_files_us = parent_write_files_us;
    receipt.parent_child_wait_us = parent_child_wait_us;
    Ok(receipt)
}

fn elapsed_us(start: std::time::Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}

/// Decode one `device-execute` child response.
///
/// A parseable HostError JSON on stdout is returned as-is. The client does
/// not rewrite CLI codes into `E_HOST_SPAWN`.
fn decode_device_execute_output(
    status: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<DeviceExecuteReceipt, HostSpawnError> {
    if status == Some(0) {
        return serde_json::from_slice(stdout).map_err(|error| HostSpawnError {
            code: "E_HOST_SPAWN".to_owned(),
            message: format!("device-execute receipt JSON is invalid: {error}"),
        });
    }
    if let Ok(parsed) = serde_json::from_slice::<WireHostError>(stdout) {
        if !parsed.code.is_empty() {
            return Err(HostSpawnError {
                code: parsed.code,
                message: parsed.message,
            });
        }
    }
    Err(HostSpawnError {
        code: "E_HOST_SPAWN".to_owned(),
        message: format!(
            "device-execute exited {:?}: {}",
            status,
            String::from_utf8_lossy(stderr)
        ),
    })
}

/// Locate the hosts CLI. Never a Cargo dependency — the binary is a child.
#[allow(dead_code)] // called by `execute_device`
pub fn resolve_host_cli(build_if_missing: bool) -> Result<PathBuf, HostSpawnError> {
    resolve_host_cli_at(
        std::env::var_os("FABER_HOST_MACOS_ARM64").map(PathBuf::from),
        build_if_missing,
    )
}

/// Resolve with an explicit override (tests + `FABER_HOST_MACOS_ARM64`).
pub fn resolve_host_cli_at(
    explicit: Option<PathBuf>,
    build_if_missing: bool,
) -> Result<PathBuf, HostSpawnError> {
    if let Some(path) = explicit {
        if path.is_file() {
            return Ok(path);
        }
        return Err(HostSpawnError {
            code: "E_HOST_SPAWN".to_owned(),
            message: format!("FABER_HOST_MACOS_ARM64 is not a file: {}", path.display()),
        });
    }
    let hosts = sibling_hosts_dir()?;
    for candidate in [
        hosts.join("target/release/faber-host-macos-arm64"),
        hosts.join("target/debug/faber-host-macos-arm64"),
    ] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    if build_if_missing {
        let status = Command::new("cargo")
            .args([
                "build",
                "--release",
                "-p",
                "faber-host-macos-arm64",
                "--manifest-path",
            ])
            .arg(hosts.join("Cargo.toml"))
            .status()
            .map_err(|error| HostSpawnError {
                code: "E_HOST_SPAWN".to_owned(),
                message: format!("failed to build faber-host-macos-arm64: {error}"),
            })?;
        let built = hosts.join("target/release/faber-host-macos-arm64");
        if status.success() && built.is_file() {
            return Ok(built);
        }
        return Err(HostSpawnError {
            code: "E_HOST_SPAWN".to_owned(),
            message: format!(
                "cargo build -p faber-host-macos-arm64 failed ({status}); missing {}",
                built.display()
            ),
        });
    }
    Err(HostSpawnError {
        code: "E_HOST_SPAWN".to_owned(),
        message: format!(
            "faber-host-macos-arm64 not found under {}; set FABER_HOST_MACOS_ARM64 or build the hosts crate",
            hosts.display()
        ),
    })
}

#[derive(Deserialize)]
struct WireHostError {
    code: String,
    message: String,
}

#[cfg(feature = "device-runtime")]
#[derive(Serialize)]
struct WireDescriptor {
    backend: String,
    kernels: Vec<WireKernel>,
    launches: Vec<WireLaunch>,
    buffer_versions: Vec<WireBufferVersion>,
    program_lifetime: String,
    data_flow: Vec<WireDataFlow>,
    roots: Vec<u32>,
    results: Vec<WireResult>,
    end_of_run_results: Vec<WireEndOfRun>,
}

#[cfg(feature = "device-runtime")]
#[derive(Serialize)]
struct WireKernel {
    entry: String,
    buffers: Vec<WireBuffer>,
    grid: [u32; 3],
    block: [u32; 3],
}

#[cfg(feature = "device-runtime")]
#[derive(Serialize)]
struct WireBuffer {
    buffer_id: u32,
    buffer_name: String,
    semantic_value: u32,
    role: String,
    lifetime: String,
    initialization: String,
    binding: u32,
    element_ty: String,
    element_count: u64,
    version: u32,
}

#[cfg(feature = "device-runtime")]
#[derive(Serialize)]
struct WireLaunch {
    id: u32,
    kernel_index: u32,
}

#[cfg(feature = "device-runtime")]
#[derive(Serialize)]
struct WireBufferVersion {
    buffer_id: u32,
    version: u32,
    element_ty: String,
    element_count: u64,
}

#[cfg(feature = "device-runtime")]
#[derive(Serialize)]
struct WireDataFlow {
    buffer_id: u32,
    version: u32,
    producer: u32,
    consumer: u32,
}

#[cfg(feature = "device-runtime")]
#[derive(Serialize)]
struct WireResult {
    buffer_id: u32,
    version: u32,
    produced_by: u32,
    at_launch: u32,
}

#[cfg(feature = "device-runtime")]
#[derive(Serialize)]
struct WireEndOfRun {
    buffer_id: u32,
    version: u32,
}

#[cfg(feature = "device-runtime")]
fn packed_region_elems(
    weight_map: Option<&BTreeMap<u32, WeightFileRange>>,
    buffer_id: u32,
) -> Option<u64> {
    weight_map.and_then(|map| map.get(&buffer_id).map(|range| range.elems))
}

#[cfg(feature = "device-runtime")]
fn descriptor_json_from_section(
    section: &FmirDeviceSection,
    backend: &str,
    weight_map: Option<&BTreeMap<u32, WeightFileRange>>,
) -> Result<Vec<u8>, HostSpawnError> {
    let wire = &section.device_program.program;
    let mut kernels = Vec::with_capacity(wire.kernels.len());
    let mut buffer_versions = Vec::new();
    for kernel in &wire.kernels {
        let entry = kernel_entry(section, backend, &kernel.entry);
        let mut buffers = Vec::with_capacity(kernel.resources.len());
        for resource in &kernel.resources {
            let element_count = packed_region_elems(weight_map, resource.buffer.id)
                .unwrap_or(resource.version.element_count);
            add_buffer_version(
                &mut buffer_versions,
                resource.buffer.id,
                resource.version.version,
                &resource.version.element_ty,
                element_count,
            )?;
            buffers.push(WireBuffer {
                buffer_id: resource.buffer.id,
                buffer_name: resource.buffer.name.clone(),
                semantic_value: resource.buffer.semantic_value,
                role: resource.buffer.role.spelling().to_owned(),
                lifetime: resource.buffer.lifetime.spelling().to_owned(),
                initialization: resource.initialization.spelling().to_owned(),
                binding: resource.binding.binding,
                element_ty: resource.version.element_ty.clone(),
                element_count,
                version: resource.version.version,
            });
        }
        kernels.push(WireKernel {
            entry,
            buffers,
            grid: [
                u32::try_from(kernel.launch.workgroup_count.x).unwrap_or(u32::MAX),
                u32::try_from(kernel.launch.workgroup_count.y).unwrap_or(u32::MAX),
                u32::try_from(kernel.launch.workgroup_count.z).unwrap_or(u32::MAX),
            ],
            block: [
                kernel.launch.workgroup.x,
                kernel.launch.workgroup.y,
                kernel.launch.workgroup.z,
            ],
        });
    }
    for result in &wire.results {
        add_buffer_version(
            &mut buffer_versions,
            result.buffer.id,
            result.version.version,
            &result.version.element_ty,
            result.version.element_count,
        )?;
    }
    let mut results = Vec::new();
    let mut end_of_run_results = Vec::new();
    for result in &wire.results {
        if result.observation.cadence == WireObservationCadence::EndOfRun {
            end_of_run_results.push(WireEndOfRun {
                buffer_id: result.buffer.id,
                version: result.version.version,
            });
        } else {
            results.push(WireResult {
                buffer_id: result.buffer.id,
                version: result.version.version,
                produced_by: result.produced_by,
                at_launch: result.observation.at_launch,
            });
        }
    }
    let descriptor = WireDescriptor {
        backend: backend.to_owned(),
        kernels,
        launches: wire
            .launches
            .iter()
            .map(|launch| WireLaunch {
                id: launch.id,
                kernel_index: launch.kernel_index,
            })
            .collect(),
        buffer_versions,
        program_lifetime: match wire.lifetime {
            WireProgramLifetime::SingleRun => "single-run".to_owned(),
            WireProgramLifetime::RepeatingStep(_) => "repeating-step".to_owned(),
        },
        data_flow: wire
            .dependencies
            .iter()
            .map(|edge| WireDataFlow {
                buffer_id: edge.buffer,
                version: edge.version,
                producer: edge.producer,
                consumer: edge.consumer,
            })
            .collect(),
        roots: wire.roots.clone(),
        results,
        end_of_run_results,
    };
    serde_json::to_vec_pretty(&descriptor).map_err(|error| HostSpawnError {
        code: "E_DEVICE_DESCRIPTOR".to_owned(),
        message: format!("failed to encode device-execute descriptor: {error}"),
    })
}

#[cfg(feature = "device-runtime")]
fn kernel_entry(section: &FmirDeviceSection, backend: &str, logical: &str) -> String {
    if backend != "cuda" {
        return logical.to_owned();
    }
    section
        .artifacts
        .artifact
        .iter()
        .find(|artifact| artifact.backend == FmirDeviceBackend::Cuda)
        .and_then(|artifact| {
            artifact
                .entrypoints
                .iter()
                .find(|identity| identity.entry == logical)
                .map(|identity| identity.symbol.clone())
        })
        .unwrap_or_else(|| logical.to_owned())
}

#[cfg(feature = "device-runtime")]
fn add_buffer_version(
    versions: &mut Vec<WireBufferVersion>,
    buffer_id: u32,
    version: u32,
    element_ty: &str,
    element_count: u64,
) -> Result<(), HostSpawnError> {
    if let Some(existing) = versions
        .iter()
        .find(|existing| existing.buffer_id == buffer_id && existing.version == version)
    {
        if existing.element_ty != element_ty || existing.element_count != element_count {
            return Err(HostSpawnError {
                code: "E_DEVICE_DESCRIPTOR".to_owned(),
                message: format!(
                    "buffer {buffer_id} version {version} carries conflicting shape facts"
                ),
            });
        }
        return Ok(());
    }
    versions.push(WireBufferVersion {
        buffer_id,
        version,
        element_ty: element_ty.to_owned(),
        element_count,
    });
    Ok(())
}

/// Encode `{ "<buffer-id>": [f32, ...] }` for the hosts CLI `--inputs` file.
///
/// Finite values are JSON numbers (`f32` → `f64` is injective). NaN
/// payloads are `"0x"` + 8 hex bits so packed GGUF words survive the
/// child-process file; `"Infinity"` / `"-Infinity"` stay strings.
pub fn encode_device_inputs(inputs: &BTreeMap<u32, Vec<f32>>) -> Result<Vec<u8>, HostSpawnError> {
    let wire: BTreeMap<String, Vec<serde_json::Value>> = inputs
        .iter()
        .map(|(id, values)| {
            (
                id.to_string(),
                values.iter().copied().map(f32_to_json).collect(),
            )
        })
        .collect();
    serde_json::to_vec_pretty(&wire).map_err(|error| HostSpawnError {
        code: "E_DEVICE_DESCRIPTOR".to_owned(),
        message: format!("failed to encode device-execute inputs: {error}"),
    })
}

pub(crate) fn encode_weight_map(
    map: &BTreeMap<u32, WeightFileRange>,
) -> Result<Vec<u8>, HostSpawnError> {
    let wire: BTreeMap<String, WeightFileRange> = map
        .iter()
        .map(|(id, range)| (id.to_string(), *range))
        .collect();
    serde_json::to_vec(&wire).map_err(|error| HostSpawnError {
        code: "E_DEVICE_DESCRIPTOR".to_owned(),
        message: format!("failed to encode device-execute weight-map: {error}"),
    })
}

/// Decode the hosts CLI `--inputs` JSON. Mirrors
/// `faber_host_macos_arm64::device_execute::inputs_from_json`.
pub fn decode_device_inputs(bytes: &[u8]) -> Result<BTreeMap<u32, Vec<f32>>, HostSpawnError> {
    let wire: BTreeMap<String, Vec<serde_json::Value>> =
        serde_json::from_slice(bytes).map_err(|error| HostSpawnError {
            code: "E_DEVICE_DESCRIPTOR".to_owned(),
            message: format!("device-execute inputs JSON is invalid: {error}"),
        })?;
    let mut inputs = BTreeMap::new();
    for (key, values) in wire {
        let id = key.parse::<u32>().map_err(|_| HostSpawnError {
            code: "E_DEVICE_DESCRIPTOR".to_owned(),
            message: format!("device-execute inputs key `{key}` is not a buffer id"),
        })?;
        let mut parsed = Vec::with_capacity(values.len());
        for (index, value) in values.into_iter().enumerate() {
            parsed.push(f32_from_json(&value).map_err(|detail| HostSpawnError {
                code: "E_DEVICE_DESCRIPTOR".to_owned(),
                message: format!("device-execute inputs[{id}][{index}] is invalid: {detail}"),
            })?);
        }
        inputs.insert(id, parsed);
    }
    Ok(inputs)
}

fn f32_to_json(value: f32) -> serde_json::Value {
    if value.is_nan() {
        serde_json::Value::String(hex_f32_bits(value))
    } else if value == f32::INFINITY {
        serde_json::Value::String("Infinity".to_owned())
    } else if value == f32::NEG_INFINITY {
        serde_json::Value::String("-Infinity".to_owned())
    } else {
        serde_json::Number::from_f64(f64::from(value))
            .map(serde_json::Value::Number)
            .unwrap_or_else(|| serde_json::Value::String(hex_f32_bits(value)))
    }
}

fn f32_from_json(value: &serde_json::Value) -> Result<f32, String> {
    match value {
        serde_json::Value::Number(number) => number
            .as_f64()
            .map(|wide| wide as f32)
            .ok_or_else(|| "number is not finite".to_owned()),
        serde_json::Value::String(spelling) => parse_f32_string(spelling),
        serde_json::Value::Null => Ok(f32::NAN),
        other => Err(format!("expected number or non-finite string, got {other}")),
    }
}

fn hex_f32_bits(value: f32) -> String {
    format!("0x{:08x}", value.to_bits())
}

fn parse_f32_string(spelling: &str) -> Result<f32, String> {
    match spelling {
        "NaN" => Ok(f32::NAN),
        "Infinity" => Ok(f32::INFINITY),
        "-Infinity" => Ok(f32::NEG_INFINITY),
        hex if hex.len() == 10 && hex.as_bytes()[..2].eq_ignore_ascii_case(b"0x") => {
            u32::from_str_radix(&hex[2..], 16)
                .map(f32::from_bits)
                .map_err(|_| format!("f32 bit string `{spelling}` is not hex"))
        }
        other => Err(format!("unknown f32 spelling `{other}`")),
    }
}

fn sibling_hosts_dir() -> Result<PathBuf, HostSpawnError> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Packet `…/worktrees/<lane>/radix/crates/faber` → packet/hosts.
    // Radix-only packets have no hosts member; walk up to the container
    // `faberlang/hosts` checkout so spawn can still build the CLI.
    let mut dir = manifest.clone();
    for _ in 0..6 {
        dir.pop();
        let hosts = dir.join("hosts");
        if hosts.join("Cargo.toml").is_file() {
            return Ok(hosts);
        }
    }
    Err(HostSpawnError {
        code: "E_HOST_SPAWN".to_owned(),
        message: format!(
            "sibling hosts crate not found from {} (expected packet or workspace layout)",
            manifest.display()
        ),
    })
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), HostSpawnError> {
    fs::write(path, bytes).map_err(|error| HostSpawnError {
        code: "E_HOST_SPAWN".to_owned(),
        message: format!("failed to write {}: {error}", path.display()),
    })
}

fn path_text(path: &Path) -> Result<String, HostSpawnError> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| HostSpawnError {
            code: "E_HOST_SPAWN".to_owned(),
            message: format!("non-UTF8 path {}", path.display()),
        })
}

struct WorkDir {
    path: PathBuf,
}

impl WorkDir {
    fn create() -> Result<Self, HostSpawnError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "faber-device-execute-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).map_err(|error| HostSpawnError {
            code: "E_HOST_SPAWN".to_owned(),
            message: format!("failed to create {}: {error}", path.display()),
        })?;
        Ok(Self { path })
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
#[path = "host_spawn_test.rs"]
mod tests;
