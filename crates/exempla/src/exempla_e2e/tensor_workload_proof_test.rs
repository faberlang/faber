use super::{
    tensor_workload_proof_rows, TensorWorkloadProofBucket, TensorWorkloadProofOwner,
    TensorWorkloadProofTier,
};
use crate::exempla_e2e::common::{command_available, make_temp_root};
use crate::exempla_e2e::gpu_workload::read_reference_fixture;
use radix::driver::Session;
use radix::Config;
use std::fs;
use std::process::Command;

#[test]
fn tensor_workload_proof_selects_rung0_matmul() {
    let rows = tensor_workload_proof_rows();
    // G-P-13 S4: proof table extended from 2 rows (rung 0 + 1) to 3 rows
    // (rung 2 added by the S2 handoff).
    assert_eq!(rows.len(), 3);

    let row = rows[0];
    assert_eq!(row.rung, 0);
    assert_eq!(row.exemplar_path, "gpu-workload/rung-0-matmul.fab");
    assert_eq!(row.reference_path, "gpu-workload/rung-0-matmul.ref.json");
    assert_eq!(
        row.expected_stdout_path,
        "gpu-workload/rung-0-matmul.expected"
    );
    assert_eq!(row.selected_operation, "rank-2 f32 matmul workload");
}

#[test]
fn tensor_workload_proof_records_current_stable_blocker() {
    let row = tensor_workload_proof_rows()[0];

    assert_eq!(row.tier, TensorWorkloadProofTier::DeviceStaged);
    assert_eq!(
        row.bucket,
        Some(TensorWorkloadProofBucket::LaunchContractFailed)
    );
    assert!(!row.output_checked);
    assert_eq!(
        row.blocker_owner,
        Some(TensorWorkloadProofOwner::CudaKernelEmitHostProvider)
    );
    assert!(row.blocker_issue.contains("host provider"));
    assert!(row.blocker_issue.contains("SermoOpen"));
    assert!(row.blocker_issue.contains("cuda:launch"));
    assert!(row.blocker_issue.contains("no real device executor"));
    assert!(row.blocker_issue.contains("launch contract step"));
}

#[test]
fn tensor_workload_proof_rung0_reference_fixture_is_valid() {
    // exemplar_path is examples-relative (`gpu-workload/rung-0-matmul.fab`).
    let path = crate::paths::gpu_workload_dir()
        .parent()
        .expect("examples home")
        .join(tensor_workload_proof_rows()[0].exemplar_path);

    let fixture = read_reference_fixture(&path, 0).expect("rung 0 reference fixture");

    assert_eq!(fixture.tolerance, 0.00001);
    assert_eq!(
        fixture.reference,
        serde_json::json!([58.0, 64.0, 139.0, 154.0])
    );
}

#[test]
fn tensor_workload_proof_cites_pinned_gpu_baseline() {
    let row = tensor_workload_proof_rows()[0];

    assert!(row
        .evidence
        .contains("gpu-workload-floor/baseline-ledger.md"));
    assert!(row.evidence.contains("Bucket Ownership"));
}

#[test]
fn tensor_workload_proof_selects_rung1_device_linear() {
    let row = tensor_workload_proof_rows()[1];

    assert_eq!(row.rung, 1);
    assert_eq!(row.tier, TensorWorkloadProofTier::OutputChecked);
    assert!(row.output_checked);
    assert_eq!(row.bucket, None);
    assert_eq!(row.blocker_owner, None);
    assert_eq!(
        row.exemplar_path,
        "tensor-fragment/tiny-linear-device/src/main.fab"
    );
    assert!(row
        .evidence
        .contains("tensor_workload_proof_rung1_device_linear_matches_stepper"));
    assert!(row.evidence.contains("w4-06b-gpu-proof.mjs"));
    assert!(row.evidence.contains("headless Chrome"));
}

#[test]
fn tensor_workload_proof_rung1_device_linear_matches_stepper() {
    let row = tensor_workload_proof_rows()[1];
    let path = crate::paths::package_corpus_dir().join(row.exemplar_path);
    let fixture = read_reference_fixture(&path, 1).expect("rung 1 reference fixture");
    assert_eq!(fixture.tolerance, 0.00001);
    assert_eq!(
        fixture.reference,
        serde_json::json!([9.1, 12.2, 18.1, 24.2, 27.1, 36.2, 36.1, 48.2])
    );
}

#[test]
fn tensor_workload_proof_selects_rung2_device_relu() {
    let rows = tensor_workload_proof_rows();
    let row = rows[2];

    assert_eq!(row.rung, 2);
    // Honest tier per need fe38bb00 (wave-4 council item 10): the proof row
    // records rung 2 at the measured lower tier, NOT OutputChecked — no real
    // ReLU device dispatch evidence exists yet (no w4-06d-gpu-relu-proof.mjs,
    // no chain test). Fixture validation alone does not claim device output.
    assert_eq!(row.tier, TensorWorkloadProofTier::FrontendAnalyzed);
    assert_eq!(row.bucket, Some(TensorWorkloadProofBucket::MirLoweringFailed));
    assert!(!row.output_checked);
    assert_eq!(row.blocker_owner, None);
    assert_eq!(row.blocker_issue, "");
    assert_eq!(
        row.exemplar_path,
        "tensor-fragment/tiny-linear-device-relu/src/main.fab"
    );
    assert_eq!(
        row.reference_path,
        "tensor-fragment/tiny-linear-device-relu/src/main.ref.json"
    );
    assert_eq!(
        row.expected_stdout_path,
        "tensor-fragment/tiny-linear-device-relu/src/main.expected"
    );
    assert!(row.selected_operation.contains("ReLU"));
    assert!(row
        .evidence
        .contains("corpus/tensor-fragment/tiny-linear-device-relu"));
}

#[test]
fn tensor_workload_proof_rung2_device_relu_matches_stepper() {
    let row = tensor_workload_proof_rows()[2];
    let path = crate::paths::package_corpus_dir().join(row.exemplar_path);
    let fixture = read_reference_fixture(&path, 2).expect("rung 2 reference fixture");
    assert_eq!(fixture.tolerance, 0.00001);
    assert_eq!(
        fixture.reference,
        serde_json::json!([10.1, 0.0, 0.0, 24.2, 28.1, 0.0, 0.0, 48.2])
    );

    // Not-identity proof: 4 of 8 elements are 0.0 (were negative
    // pre-activation values), 4 are positive and unchanged — ReLU is active,
    // not identity.
    let values: Vec<f64> = fixture
        .reference
        .as_array()
        .expect("reference is array")
        .iter()
        .map(|v| v.as_f64().expect("f64"))
        .collect();
    let zero_count = values.iter().filter(|&&v| v == 0.0).count();
    let non_zero_count = values.iter().filter(|&&v| v != 0.0).count();
    assert_eq!(zero_count, 4, "expected exactly 4 zeroed negative pre-activations");
    assert_eq!(
        non_zero_count, 4,
        "expected exactly 4 unchanged positive pre-activations"
    );
}

#[test]
fn tensor_workload_proof_empty_rows_when_no_matching_exemplar_path() {
    let rows = tensor_workload_proof_rows();
    // We know the rows have specific exemplar_path entries; verify these are
    // well-formed for every row.
    for (i, row) in rows.iter().enumerate() {
        assert!(
            !row.exemplar_path.is_empty(),
            "row {} has empty exemplar_path",
            i
        );
        assert!(
            row.tier as u8 >= 0,
            "row {} has invalid tier {:?}",
            i,
            row.tier
        );
    }
}

#[test]
fn tensor_workload_proof_rung_indices_are_contiguous_from_zero() {
    let rows = tensor_workload_proof_rows();
    for (i, row) in rows.iter().enumerate() {
        assert_eq!(
            row.rung, i,
            "expected row {i} to have rung {i}, got rung {}",
            row.rung
        );
    }
}

/// G-SPINE-10 S4: headless Chrome WebGPU chain dispatch proof (task 9151ae7d).
///
/// Compiles the tiny-linear-device exemplar through the faber pipeline,
/// extracts the G-SPINE-10 [`KernelChainDescriptor`] from compiler output,
/// spawns the Node.js Puppeteer proof script
/// (`triga/scripta/w4-06c-gpu-chain-proof.mjs`), which dispatches the chain
/// through `dispatchChainFromDescriptor` (hosts `735df10`) in headless Chrome
/// WebGPU (SwiftShader), reads back the output, and asserts a bit-identical
/// match to the stepper reference `[9.1, 12.2, ..., 36.1, 48.2]` within f32
/// tolerance `0.00001`.
///
/// Green gate: PARKED on the hand-1 radix-mir-wgsl `Collection(TensorAdd)`
/// emission task (filed from need bccc236b; ABI premise corrections in need
/// 4e156be2, task 9151ae7d). The compiler currently rejects the exemplar's
/// kernel before emitting any descriptor, so this test fails at the compile
/// step with the exact diagnostic until that task lands. The test is
/// deliberately NOT `#[ignore]` — once the emitter gap closes it must run
/// for real in CI (no Wave-3 G-P-12 ignored-integration-test pattern).
#[test]
fn tensor_workload_proof_rung1_device_gpu_chain_dispatch() {
    // ── 1. Compile the exemplar through the faber pipeline ────────────────
    let row = tensor_workload_proof_rows()[1];
    let path = crate::paths::package_corpus_dir().join(row.exemplar_path);
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));

    let session = Session::new(Config::default());
    let mut analysis =
        radix::driver::analyze_source(&session, &path.display().to_string(), &source)
            .expect("frontend analysis of tiny-linear-device");
    let interner = analysis.interner.clone();
    let device_roles = radix::mir::device_roles_from_hir(&analysis.hir);
    let mir = radix::mir::lower_analyzed_unit_with_context(&mut analysis)
        .expect("MIR lowering of tiny-linear-device");
    let chain = radix::mir::emit_chain_descriptor(&device_roles, &mir.validated, &interner).expect(
        "emit_chain_descriptor for tiny-linear-device — green gate parked on hand-1 \
         TensorAdd emission (need bccc236b / 4e156be2); once that lands this must pass",
    );
    assert!(
        !chain.chain.is_empty(),
        "chain descriptor must contain at least one kernel"
    );

    // ── 2. Serialize the chain descriptor to JSON ─────────────────────────
    let descriptor_json =
        serde_json::to_string(&chain).expect("serialize chain descriptor to JSON");

    // ── 3. Build input data keyed by the descriptor's storage-buffer
    //       @binding namespace. dispatchChainFromDescriptor resolves each
    //       bind-group entry via resources.buffers.get(bufDecl.binding).
    //       Output buffers are provided zero-initialized so the dispatch can
    //       bind them. tiny-linear-device inputs: x (12 f32 / 48 B),
    //       w (6 f32 / 24 B), b (8 f32 / 32 B); output y (8 f32 / 32 B). ─────
    const X_DATA: &[f64] = &[1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0, 4.0, 4.0, 4.0];
    const W_DATA: &[f64] = &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    const B_DATA: &[f64] = &[0.1, 0.2, 0.1, 0.2, 0.1, 0.2, 0.1, 0.2];

    let mut input = serde_json::Map::new();
    for kernel in &chain.chain {
        for (buffer_index, decl) in kernel.storage_buffers.iter().enumerate() {
            let is_output = kernel.output_bindings.contains(&(buffer_index as u32));
            let payload: Vec<f64> = if is_output {
                vec![0.0; (decl.size / 4) as usize]
            } else {
                match decl.size {
                    48 => X_DATA.to_vec(),
                    24 => W_DATA.to_vec(),
                    32 => B_DATA.to_vec(),
                    other => panic!(
                        "unexpected storage buffer size {other} in kernel {}",
                        kernel.entry_point
                    ),
                }
            };
            input.insert(decl.binding.to_string(), serde_json::json!(payload));
        }
    }
    let input_json = serde_json::Value::Object(input).to_string();

    // ── 4. Expected values from the stepper reference fixture ─────────────
    let fixture = read_reference_fixture(&path, 1).expect("rung 1 reference fixture");
    let expected_json = fixture.reference.to_string();

    // ── 5. Write artifacts to the managed temp root ───────────────────────
    let temp_root = make_temp_root();
    let descriptor_file = temp_root.join("chain-descriptor.json");
    fs::write(&descriptor_file, &descriptor_json).expect("write chain descriptor JSON");

    // ── 6. Spawn the Node proof script ────────────────────────────────────
    if !command_available("node", &["--version"]) {
        eprintln!("SKIP: node not on PATH — chain dispatch proof cannot run (recorded condition)");
        return;
    }
    let script = triga_scripta_dir().join("w4-06c-gpu-chain-proof.mjs");
    if !script.is_file() {
        eprintln!(
            "SKIP: proof script {} not present — chain dispatch proof cannot run (recorded condition)",
            script.display()
        );
        return;
    }

    let output = Command::new("node")
        .arg(&script)
        .arg("--descriptor")
        .arg(&descriptor_file)
        .arg("--input")
        .arg(&input_json)
        .arg("--expected")
        .arg(&expected_json)
        .arg("--tolerance")
        .arg("0.00001")
        .output()
        .expect("spawn node chain proof script");

    assert!(
        output.status.success(),
        "chain dispatch proof failed (exit {:?}):\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Resolve the `triga/scripta` directory containing the Puppeteer proof
/// scripts (`FABER_TRIGA_SCRIPTA` env override; sibling `triga/scripta`).
fn triga_scripta_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("FABER_TRIGA_SCRIPTA") {
        return std::path::PathBuf::from(dir);
    }
    if let Some(home) = crate::paths::faberlang_home() {
        let dir = home.join("triga").join("scripta");
        if dir.is_dir() {
            return dir;
        }
    }
    // crates/exempla → faber → faberlang container → triga/scripta
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../triga/scripta")
}
