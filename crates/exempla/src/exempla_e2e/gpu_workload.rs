//! GPU workload floor harness: rung classification and output-checked ratchets.
//!
//! This is a measurement harness, not a CUDA implementation. It classifies the
//! workload rungs through frontend analysis, MIR lowering, and device IR
//! staging. Device execution is proven on the RunPod verification lane and
//! recorded as a receipt: a rung with a recorded CUDA-route device-execution
//! proof (launch → sync → readback → numeric oracle against its `*.ref.json`)
//! counts toward its output-checked floor. Rung 0 (`rung-0-matmul`) closed that
//! path 2026-08-07 on the re-verified U-05 receipt (radix 7f520f067, run
//! u05-rerun-nonce1; nonce-bound adoption); rungs 1–4 stay pinned at zero until
//! their producer tracks supply launch/run capability.

use super::common::{
    collect_exempla_files, command_available, format_ceiling_line, format_diagnostic_messages,
    format_tier_line, make_temp_root, read_expected_stdout,
};
use radix::codegen::Target;
use radix::driver::Session;
use radix::Config;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub(super) struct GpuReferenceFixture {
    pub(super) tolerance: f64,
    pub(super) reference: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum GpuWorkloadTier {
    SourceReadable,
    FrontendAnalyzed,
    MirLowered,
    DeviceStaged,
    KernelLaunchable,
    Runnable,
    OutputChecked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GpuWorkloadBucket {
    FrontendFailed,
    MirLoweringFailed,
    DeviceStagingFailed,
    LaunchContractFailed,
    RunFailed,
    NumericMismatch,
    ReferenceMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceVerifier {
    LlvmAs,
    Opt,
}

#[derive(Debug, Clone)]
struct GpuWorkloadToolchain {
    verifier: Option<DeviceVerifier>,
    verifier_version: Option<String>,
    ptxas_available: bool,
}

// Rung-0 output-checked floor: 0 → 1 via the re-verified U-05 device-execution
// receipt (radix 7f520f067, run u05-rerun-nonce1, 2026-08-07) — rung-0-matmul
// launch → sync → readback → numeric oracle on the RunPod dc-a100 lane; see
// CUDA_ROUTE_DEVICE_EXECUTION_PROOFS.
const EXPECTED_RUNG_0_OUTPUT_CHECKED_FLOOR: usize = 1;
const EXPECTED_RUNG_1_OUTPUT_CHECKED_FLOOR: usize = 0;
const EXPECTED_RUNG_2_OUTPUT_CHECKED_FLOOR: usize = 0;
const EXPECTED_RUNG_3_OUTPUT_CHECKED_FLOOR: usize = 0;
const EXPECTED_RUNG_4_OUTPUT_CHECKED_FLOOR: usize = 0;
/// Maximum rungs that may stop at explicit unsupported/launch-contract gaps.
///
/// WHY: first remeasurement (2026-07-09) moved rungs 0–4 from MirLoweringFailed
/// to DeviceStagingFailed — `lower_ad_expr` produces SermoOpen which the LLVM
/// text probe cannot verify because no host provider exists. Rung 0–2 route to
/// the cuda-kernel-emit host provider skeleton; rung 3 routes to the AIR autodiff
/// producer gate; rung 4 routes to the placement producer gate. This is counted
/// debt and must ratchet down as those producer gates land. 2026-08-07: rung 0
/// closed via the U-05 device-execution receipt, so the measured unsupported-gap
/// count is now at most 4; the ceiling constant stays 5 as the cap.
const EXPECTED_GPU_UNSUPPORTED_DIAGNOSTIC_CEILING: usize = 5;

/// Recorded CUDA-route device-execution proofs (receipt-gated ratchets).
///
/// The harness measures local staging only; device execution is proven on the
/// RunPod verification lane and recorded as a receipt. A rung listed here
/// carries an output-checked device-execution claim (launch → sync → readback
/// → numeric oracle) on the CUDA route and counts toward its output-checked
/// floor. WebGPU sibling-route proofs (rungs 1–2) are NOT listed here — the
/// route boundary law never moves CUDA-route floors from sibling-lane evidence.
const CUDA_ROUTE_DEVICE_EXECUTION_PROOFS: &[(usize, &str)] = &[(
    0,
    "rung-0-matmul runpod-gated closure PASS (U-05, re-verified u05-rerun-nonce1, \
     radix 7f520f067): dc-a100 A100-SXM4-80GB CC 8.0, pod_provenance adopted + \
     create_nonce e6acc08452049526da87ea0fea4fc15a, launch grid 1,1,1 block 8,8,1, \
     readback [58, 64, 139, 154] vs pinned oracle within 1e-5 (worst delta 0), \
     teardown provider-confirmed, zero orphan pods; evidence radix docs/factory/\
     codex-gap-campaign/u05-rerun-nonce-bound-evidence.md + trials receipt \
     ~/work/ianzepp/trials/runpod-gpu-verification/u05-rerun-nonce1/receipt.md \
     (original a88fc4933 landing held pending the nonce-bound re-verification; \
     this re-anchor is the accepted citation)",
)];

fn cuda_route_device_execution_proof(rung: usize) -> Option<&'static str> {
    CUDA_ROUTE_DEVICE_EXECUTION_PROOFS
        .iter()
        .find(|(rung_idx, _)| *rung_idx == rung)
        .map(|(_, proof)| *proof)
}

#[derive(Debug)]
struct GpuWorkloadResult {
    path: PathBuf,
    rung: usize,
    tier: GpuWorkloadTier,
    bucket: Option<GpuWorkloadBucket>,
    reason: String,
}

#[test]
#[ignore = "slow gpu workload e2e; run: cargo test -p exempla --lib exempla_gpu_workload_e2e -- --ignored --nocapture"]
fn exempla_gpu_workload_e2e() {
    let workload_dir = crate::paths::gpu_workload_dir();
    let workloads = collect_exempla_files(&workload_dir);
    assert!(
        !workloads.is_empty(),
        "GPU workload harness found no workload rungs"
    );

    let session = Session::new(Config::default().with_target(Target::MirLlvm));
    let temp_root = make_temp_root();
    let toolchain = detect_gpu_workload_toolchain();
    let mut results = Vec::with_capacity(workloads.len());

    for (idx, file) in workloads.iter().enumerate() {
        results.push(classify_gpu_workload(
            &session, file, idx, &temp_root, &toolchain,
        ));
    }

    print_gpu_workload_report(&results, &toolchain);
    assert_gpu_workload_gates(&results);
}

fn detect_gpu_workload_toolchain() -> GpuWorkloadToolchain {
    let verifier = if command_available("llvm-as", &["--version"]) {
        Some(DeviceVerifier::LlvmAs)
    } else if command_available("opt", &["--version"]) {
        Some(DeviceVerifier::Opt)
    } else {
        None
    };
    let verifier_version = verifier.map(device_verifier_version);

    GpuWorkloadToolchain {
        verifier,
        verifier_version,
        ptxas_available: command_available("ptxas", &["--version"]),
    }
}

fn device_verifier_version(verifier: DeviceVerifier) -> String {
    let output = match verifier {
        DeviceVerifier::LlvmAs => Command::new("llvm-as").arg("--version").output(),
        DeviceVerifier::Opt => Command::new("opt").arg("--version").output(),
    };
    let Ok(output) = output else {
        return "version unavailable".to_owned();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .unwrap_or("version unavailable")
        .to_owned()
}

fn classify_gpu_workload(
    session: &Session,
    file: &Path,
    idx: usize,
    temp_root: &Path,
    toolchain: &GpuWorkloadToolchain,
) -> GpuWorkloadResult {
    let rung = rung_index(file);
    let source = match fs::read_to_string(file) {
        Ok(source) => source,
        Err(err) => {
            return gpu_workload_result(
                file,
                rung,
                GpuWorkloadTier::SourceReadable,
                Some(GpuWorkloadBucket::FrontendFailed),
                format!("cannot read source: {err}"),
            );
        }
    };

    if let Err(reason) = read_reference_fixture(file, rung) {
        return gpu_workload_result(
            file,
            rung,
            GpuWorkloadTier::SourceReadable,
            Some(GpuWorkloadBucket::ReferenceMissing),
            reason,
        );
    }

    let mut analysis =
        match radix::driver::analyze_source(session, &file.display().to_string(), &source) {
            Ok(analysis) => analysis,
            Err(diagnostics) => {
                return gpu_workload_result(
                    file,
                    rung,
                    GpuWorkloadTier::SourceReadable,
                    Some(GpuWorkloadBucket::FrontendFailed),
                    format!(
                        "frontend failed: {}",
                        format_diagnostic_messages(&diagnostics)
                    ),
                );
            }
        };

    let interner = analysis.interner.clone();
    let device_roles = radix::mir::device_roles_from_hir(&analysis.hir);
    let mir = match radix::mir::lower_analyzed_unit_with_context(&mut analysis) {
        Ok(mir) => mir,
        Err(errors) => {
            return gpu_workload_result(
                file,
                rung,
                GpuWorkloadTier::FrontendAnalyzed,
                Some(GpuWorkloadBucket::MirLoweringFailed),
                format!(
                    "MIR lowering failed: {}",
                    errors
                        .iter()
                        .map(|error| error.issue.clone())
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
            );
        }
    };

    let llvm = match radix::mir::emit_llvm_text_probe_with_device_roles(
        &device_roles,
        &mir.validated,
        &interner,
    ) {
        Ok(llvm) => llvm,
        Err(error) => {
            return gpu_workload_result(
                file,
                rung,
                GpuWorkloadTier::MirLowered,
                Some(GpuWorkloadBucket::DeviceStagingFailed),
                format!("device staging failed: {error}"),
            );
        }
    };

    let stem = file
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("gpu-workload");
    let llvm_file = temp_root.join(format!("{idx:03}-{stem}.ll"));
    if let Err(err) = fs::write(&llvm_file, &llvm) {
        return gpu_workload_result(
            file,
            rung,
            GpuWorkloadTier::MirLowered,
            Some(GpuWorkloadBucket::DeviceStagingFailed),
            format!("cannot write device LLVM output: {err}"),
        );
    }

    if let Some(verifier) = toolchain.verifier {
        if let Err(reason) = verify_device_llvm(verifier, &llvm_file) {
            return gpu_workload_result(
                file,
                rung,
                GpuWorkloadTier::MirLowered,
                Some(GpuWorkloadBucket::DeviceStagingFailed),
                format!(
                    "device LLVM text emitted to {}; verifier failed: {reason}",
                    llvm_file.display()
                ),
            );
        }
    }

    let verify_note = match toolchain.verifier {
        Some(verifier) => format!("verified with {}", verifier.command()),
        None => "verifier unavailable; retained emitted device IR".to_owned(),
    };

    if let Some(proof) = cuda_route_device_execution_proof(rung) {
        return gpu_workload_result(
            file,
            rung,
            GpuWorkloadTier::OutputChecked,
            None,
            format!(
                "device LLVM text staged at {}; {verify_note}; CUDA-route \
                 device-execution proof recorded: {proof}",
                llvm_file.display()
            ),
        );
    }

    gpu_workload_result(
        file,
        rung,
        GpuWorkloadTier::DeviceStaged,
        Some(GpuWorkloadBucket::LaunchContractFailed),
        format!(
            "device LLVM text staged at {}; {verify_note}; CUDA launch provider/runner absent",
            llvm_file.display()
        ),
    )
}

fn gpu_workload_result(
    file: &Path,
    rung: usize,
    tier: GpuWorkloadTier,
    bucket: Option<GpuWorkloadBucket>,
    reason: String,
) -> GpuWorkloadResult {
    GpuWorkloadResult {
        path: file.to_path_buf(),
        rung,
        tier,
        bucket,
        reason,
    }
}

fn rung_index(path: &Path) -> usize {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.strip_prefix("rung-"))
        .and_then(|rest| rest.split('-').next())
        .and_then(|digits| digits.parse::<usize>().ok())
        .unwrap_or(usize::MAX)
}

pub(super) fn read_reference_fixture(
    path: &Path,
    rung: usize,
) -> Result<GpuReferenceFixture, String> {
    let expected =
        read_expected_stdout(path).ok_or_else(|| "missing .expected stdout fixture".to_owned())?;
    let content = fs::read_to_string(path.with_extension("ref.json"))
        .map_err(|err| format!("cannot read .ref.json fixture: {err}"))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|err| format!("invalid .ref.json fixture: {err}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| ".ref.json fixture must be an object".to_owned())?;

    let actual_rung = object
        .get("rung")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| ".ref.json fixture missing numeric `rung`".to_owned())?;
    if actual_rung != rung as u64 {
        return Err(format!(
            ".ref.json rung mismatch: file name says {rung}, fixture says {actual_rung}"
        ));
    }

    let tolerance = object
        .get("tolerance")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| ".ref.json fixture missing numeric `tolerance`".to_owned())?;
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err(".ref.json fixture tolerance must be finite and non-negative".to_owned());
    }

    let reference = object
        .get("reference")
        .ok_or_else(|| ".ref.json fixture missing `reference`".to_owned())?;
    if !is_nonempty_numeric_value(reference) {
        return Err(".ref.json `reference` must be a non-empty array or object".to_owned());
    }

    let output_reference = object
        .get("output_reference")
        .ok_or_else(|| ".ref.json fixture missing `output_reference`".to_owned())?;
    if !is_nonempty_numeric_value(output_reference) {
        return Err(".ref.json `output_reference` must be a non-empty array or object".to_owned());
    }

    let source = object
        .get("source")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim();
    if source.is_empty() {
        return Err(".ref.json fixture missing non-empty `source`".to_owned());
    }

    let fixture = GpuReferenceFixture {
        tolerance,
        reference: reference.clone(),
    };
    let output_fixture = GpuReferenceFixture {
        tolerance,
        reference: output_reference.clone(),
    };
    compare_numeric_output(&expected, &output_fixture).map_err(|reason| {
        format!(".expected stdout fixture does not match .ref.json output_reference: {reason}")
    })?;
    Ok(fixture)
}

fn is_nonempty_numeric_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(items) => !items.is_empty(),
        serde_json::Value::Object(fields) => !fields.is_empty(),
        _ => false,
    }
}

pub(super) fn compare_numeric_output(
    captured_stdout: &str,
    fixture: &GpuReferenceFixture,
) -> Result<(), String> {
    let captured = parse_numeric_output(captured_stdout)?;
    let mut expected = Vec::new();
    collect_reference_numbers(&fixture.reference, &mut expected)?;
    if captured.len() != expected.len() {
        return Err(format!(
            "numeric output length mismatch: got {}, want {}",
            captured.len(),
            expected.len()
        ));
    }

    for (idx, (got, want)) in captured.iter().zip(expected.iter()).enumerate() {
        let delta = (got - want).abs();
        if delta > fixture.tolerance {
            return Err(format!(
                "numeric output mismatch at {idx}: got {got}, want {want}, tolerance {}",
                fixture.tolerance
            ));
        }
    }
    Ok(())
}

pub(super) fn parse_numeric_output(stdout: &str) -> Result<Vec<f64>, String> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err("captured stdout is empty".to_owned());
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let mut numbers = Vec::new();
        collect_reference_numbers(&value, &mut numbers)?;
        if !numbers.is_empty() {
            return Ok(numbers);
        }
    }

    let mut numbers = Vec::new();
    for (idx, line) in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .enumerate()
    {
        let value = line
            .parse::<f64>()
            .map_err(|err| format!("cannot parse numeric stdout line {idx}: {err}"))?;
        numbers.push(value);
    }
    if numbers.is_empty() {
        Err("captured stdout contains no numeric values".to_owned())
    } else {
        Ok(numbers)
    }
}

fn collect_reference_numbers(value: &serde_json::Value, out: &mut Vec<f64>) -> Result<(), String> {
    match value {
        serde_json::Value::Number(number) => {
            let value = number
                .as_f64()
                .ok_or_else(|| "reference number is not representable as f64".to_owned())?;
            out.push(value);
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_reference_numbers(item, out)?;
            }
        }
        serde_json::Value::Object(fields) => {
            let mut keys = fields.keys().collect::<Vec<_>>();
            keys.sort();
            for key in keys {
                collect_reference_numbers(&fields[key], out)?;
            }
        }
        _ => return Err("reference values must be numeric, arrays, or objects".to_owned()),
    }
    Ok(())
}

fn verify_device_llvm(verifier: DeviceVerifier, llvm_file: &Path) -> Result<(), String> {
    let output = match verifier {
        DeviceVerifier::LlvmAs => Command::new("llvm-as")
            .arg("-o")
            .arg(if cfg!(windows) { "NUL" } else { "/dev/null" })
            .arg(llvm_file)
            .output(),
        DeviceVerifier::Opt => Command::new("opt")
            .arg("-disable-output")
            .arg(llvm_file)
            .output(),
    };

    let output = output.map_err(|err| format!("cannot execute device verifier: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn print_gpu_workload_report(results: &[GpuWorkloadResult], toolchain: &GpuWorkloadToolchain) {
    let total = results.len();
    eprintln!("GPU workload floor toolchain:");
    eprintln!(
        "  device verifier: {}",
        match toolchain.verifier {
            Some(verifier) => match &toolchain.verifier_version {
                Some(version) => format!("{} ({version})", verifier.command()),
                None => verifier.command().to_owned(),
            },
            None => "unavailable (llvm-as/opt not found)".to_owned(),
        }
    );
    eprintln!(
        "  ptxas: {}",
        if toolchain.ptxas_available {
            "available"
        } else {
            "unavailable"
        }
    );
    eprintln!("GPU workload rungs:");
    eprintln!(
        "{}",
        format_tier_line(
            "frontend analyzed",
            count_gpu_tier(results, GpuWorkloadTier::FrontendAnalyzed),
            total,
            total,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "MIR lowered",
            count_gpu_tier(results, GpuWorkloadTier::MirLowered),
            total,
            0,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "device staged",
            count_gpu_tier(results, GpuWorkloadTier::DeviceStaged),
            total,
            0,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "kernel launchable",
            count_gpu_tier(results, GpuWorkloadTier::KernelLaunchable),
            total,
            0,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "runnable",
            count_gpu_tier(results, GpuWorkloadTier::Runnable),
            total,
            0,
        )
    );
    print_rung_floor(results, 0, EXPECTED_RUNG_0_OUTPUT_CHECKED_FLOOR);
    print_rung_floor(results, 1, EXPECTED_RUNG_1_OUTPUT_CHECKED_FLOOR);
    print_rung_floor(results, 2, EXPECTED_RUNG_2_OUTPUT_CHECKED_FLOOR);
    print_rung_floor(results, 3, EXPECTED_RUNG_3_OUTPUT_CHECKED_FLOOR);
    print_rung_floor(results, 4, EXPECTED_RUNG_4_OUTPUT_CHECKED_FLOOR);
    eprintln!(
        "{}",
        format_ceiling_line(
            "unsupported producer-gap bucket",
            count_unsupported_producer_gap(results),
            EXPECTED_GPU_UNSUPPORTED_DIAGNOSTIC_CEILING,
        )
    );
    for bucket in [
        GpuWorkloadBucket::FrontendFailed,
        GpuWorkloadBucket::MirLoweringFailed,
        GpuWorkloadBucket::DeviceStagingFailed,
        GpuWorkloadBucket::LaunchContractFailed,
        GpuWorkloadBucket::RunFailed,
        GpuWorkloadBucket::NumericMismatch,
        GpuWorkloadBucket::ReferenceMissing,
    ] {
        eprintln!("  {bucket:?}: {}", count_gpu_bucket(results, bucket));
    }

    for result in results
        .iter()
        .filter(|result| result.tier < GpuWorkloadTier::OutputChecked)
    {
        eprintln!(
            "[gpu-workload:rung-{}:{:?}] {} :: {}",
            result.rung,
            result.tier,
            result.path.display(),
            result.reason
        );
    }
}

fn print_rung_floor(results: &[GpuWorkloadResult], rung: usize, floor: usize) {
    eprintln!(
        "{}",
        format_tier_line(
            &format!("rung {rung} output-checked"),
            count_rung_output_checked(results, rung),
            count_rung(results, rung),
            floor,
        )
    );
}

fn count_gpu_tier(results: &[GpuWorkloadResult], tier: GpuWorkloadTier) -> usize {
    results.iter().filter(|result| result.tier >= tier).count()
}

fn count_gpu_bucket(results: &[GpuWorkloadResult], bucket: GpuWorkloadBucket) -> usize {
    results
        .iter()
        .filter(|result| result.bucket == Some(bucket))
        .count()
}

fn count_rung(results: &[GpuWorkloadResult], rung: usize) -> usize {
    results.iter().filter(|result| result.rung == rung).count()
}

fn count_rung_output_checked(results: &[GpuWorkloadResult], rung: usize) -> usize {
    results
        .iter()
        .filter(|result| result.rung == rung && result.tier >= GpuWorkloadTier::OutputChecked)
        .count()
}

fn assert_gpu_workload_gates(results: &[GpuWorkloadResult]) {
    let total = results.len();
    let frontend = count_gpu_tier(results, GpuWorkloadTier::FrontendAnalyzed);
    let unsupported = count_unsupported_producer_gap(results);

    let mut regressions = Vec::new();
    if frontend < total {
        regressions.push(format!(
            "frontend analyzed expected all {total} rungs, got {frontend}"
        ));
    }
    for (rung, floor) in [
        (0, EXPECTED_RUNG_0_OUTPUT_CHECKED_FLOOR),
        (1, EXPECTED_RUNG_1_OUTPUT_CHECKED_FLOOR),
        (2, EXPECTED_RUNG_2_OUTPUT_CHECKED_FLOOR),
        (3, EXPECTED_RUNG_3_OUTPUT_CHECKED_FLOOR),
        (4, EXPECTED_RUNG_4_OUTPUT_CHECKED_FLOOR),
    ] {
        let actual = count_rung_output_checked(results, rung);
        if actual < floor {
            regressions.push(format!(
                "rung {rung} output-checked expected at least {floor}, got {actual}"
            ));
        }
    }
    if unsupported > EXPECTED_GPU_UNSUPPORTED_DIAGNOSTIC_CEILING {
        regressions.push(format!(
            "unsupported producer-gap bucket expected at most {}, got {unsupported}",
            EXPECTED_GPU_UNSUPPORTED_DIAGNOSTIC_CEILING
        ));
    }

    assert!(
        regressions.is_empty(),
        "unexpected GPU workload floor regressions:\n{}",
        regressions.join("\n")
    );
}

fn count_unsupported_producer_gap(results: &[GpuWorkloadResult]) -> usize {
    count_gpu_bucket(results, GpuWorkloadBucket::MirLoweringFailed)
        + count_gpu_bucket(results, GpuWorkloadBucket::DeviceStagingFailed)
        + count_gpu_bucket(results, GpuWorkloadBucket::LaunchContractFailed)
}

impl DeviceVerifier {
    fn command(self) -> &'static str {
        match self {
            DeviceVerifier::LlvmAs => "llvm-as",
            DeviceVerifier::Opt => "opt -disable-output",
        }
    }
}
