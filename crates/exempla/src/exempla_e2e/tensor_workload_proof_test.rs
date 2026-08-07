use super::{tensor_workload_proof_rows, TensorWorkloadProofTier};
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
fn tensor_workload_proof_rung0_is_output_checked_with_receipt() {
    let row = tensor_workload_proof_rows()[0];

    // U-06 ratchet (codex-gap Stage 2, task 8199e91d): the rung-0 row moved
    // off the CUDA launch-contract blocker onto OutputChecked, evidenced by
    // the U-05 device-execution receipt (radix a88fc4933) — never by a CPU or
    // staged-LLVM fallback.
    assert_eq!(row.tier, TensorWorkloadProofTier::OutputChecked);
    assert_eq!(row.bucket, None);
    assert!(row.output_checked);
    assert_eq!(row.blocker_owner, None);
    assert_eq!(row.blocker_issue, "");
    // The historical blocker (absent device executor; sermo_open collision
    // fixed in radix 663cbfe58) is resolved and preserved in the receipt.
    assert!(row.evidence.contains("a88fc4933"));
    assert!(row.evidence.contains("u05-rung0-matmul-evidence.md"));
    assert!(row.evidence.contains("dc-a100"));
    assert!(row.evidence.contains("worst delta 0"));
    assert!(row.evidence.contains("output-checked"));
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
fn tensor_workload_proof_cites_ledger_ratchet_and_receipt() {
    let row = tensor_workload_proof_rows()[0];

    // U-06: evidence anchors the ledger ratchet (baseline-ledger.md rung-0
    // floor 0→1) and the U-05 device-execution receipt — not a staged-LLVM or
    // CPU-fallback claim.
    assert!(row
        .evidence
        .contains("gpu-workload-floor/baseline-ledger.md"));
    assert!(row.evidence.contains("baseline-ledger.md"));
    assert!(row.evidence.contains("rung-0 floor 0→1"));
    assert!(row.evidence.contains("a88fc4933"));
    assert!(row.evidence.contains("u05-rung0-matmul-evidence.md"));
    assert!(row.evidence.contains("receipt.md"));
    // The stale launch-contract blocker phrasing is gone from the claim.
    assert!(!row.evidence.contains("LaunchContractFailed"));
    assert!(!row.evidence.contains("sermo_open"));
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
        .contains("tensor_workload_proof_rung1_device_gpu_chain_dispatch"));
    assert!(row.evidence.contains("w4-06b-gpu-proof.mjs"));
    assert!(row.evidence.contains("w4-06c-gpu-chain-proof.mjs"));
    assert!(row.evidence.contains("headless Chrome"));
    // U1 (G-P-12+S5): resolvable hosts anchors 275263e + e45a9e0; the stale
    // c11cd04c1 anchor is gone.
    assert!(row.evidence.contains("275263e"));
    assert!(row.evidence.contains("e45a9e0"));
    assert!(!row.evidence.contains("c11cd04c1"));
}

#[test]
fn tensor_workload_proof_selects_rung2_device_relu() {
    let rows = tensor_workload_proof_rows();
    let row = rows[2];

    assert_eq!(row.rung, 2);
    // Honest tier per D-W6-B2 (wave-4 council item 10): rung 2 is NOW
    // OutputChecked — the B1-follow-up emitter fix (radix 05a47f864: a
    // workgroupBarrier() between the fused matmul-add store and the relu pass
    // in the TensorRelu emitter arm) closed the intra-workgroup readback race
    // that previously read back all zeros (DeviceResultMismatch, radix
    // d495c2cff). The live real-device dispatch test
    // `tensor_workload_proof_rung2_device_gpu_chain_dispatch` is non-ignored
    // and is the promotion's evidence gate.
    assert_eq!(row.tier, TensorWorkloadProofTier::OutputChecked);
    assert_eq!(row.bucket, None);
    assert!(row.output_checked);
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
    // Evidence anchors cite the live dispatch + the barrier fix: the
    // non-ignored rung-2 dispatch test, the w4-06d proof script, the
    // workgroupBarrier, the radix fix hash, the f32 tolerance, and the exact
    // readback (with exactly 4 zeroed elements — ReLU active, not identity).
    assert!(row
        .evidence
        .contains("tensor_workload_proof_rung2_device_gpu_chain_dispatch"));
    assert!(row.evidence.contains("w4-06d-gpu-relu-proof.mjs"));
    assert!(row.evidence.contains("workgroupBarrier"));
    assert!(row.evidence.contains("05a47f864"));
    assert!(row.evidence.contains("0.00001"));
    assert!(row
        .evidence
        .contains("[10.1, 0, 0, 24.2, 28.1, 0, 0, 48.2]"));
    assert!(row.evidence.contains("exit 0"));
    // The blocked/mismatch state is gone: the row is promoted, and the old
    // "kernel runtime call" staging rejection is still not claimed.
    assert!(!row.evidence.contains("DeviceResultMismatch"));
    assert!(!row.evidence.contains("kernel runtime call"));
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
    assert_eq!(
        zero_count, 4,
        "expected exactly 4 zeroed negative pre-activations"
    );
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
        // Tier validity is enforced by the type itself:
        // `TensorWorkloadProofTier` is a closed fieldless enum, so every value
        // is a valid tier — the previous `row.tier as u8 >= 0` bound was
        // vacuous (always true for unsigned) and tripped `unused_comparisons`.
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

/// G-P-12 + G-SPINE-10 S5 (this delivery): LIVE headless Chrome WebGPU chain
/// dispatch proof — the rung-1 `OutputChecked` evidence rests on real device
/// execution, not fixture reading alone.
///
/// Compiles the tiny-linear-device exemplar through the faber pipeline,
/// extracts the G-SPINE-10 [`KernelChainDescriptor`] from compiler output,
/// spawns the Node.js Puppeteer proof script
/// (`triga/scripta/w4-06c-gpu-chain-proof.mjs`), which dispatches the chain
/// through `dispatchChainFromDescriptor` (hosts `735df10`) in headless Chrome
/// WebGPU (SwiftShader), reads back the output, and asserts a bit-identical
/// match to the stepper reference `[9.1, 12.2, ..., 36.1, 48.2]` within f32
/// tolerance `0.00001` (fixture discipline — not the proof page's looser
/// `0.001`).
///
/// Fixture validation is a sub-step of this same test: the stepper reference
/// fixture (`main.ref.json`) is still read and asserted so the reference
/// contract cannot drift. There is no separate fixture-only test claiming
/// rung-1 device proof (spec U2 done-when (d)).
///
/// Skip discipline (spec Q4): the test skips with a RECORDED CONDITION only
/// when the headless Chrome/WebGPU environment is unavailable — detected via
/// the proof script's deterministic exit code 2 (environment error: missing
/// Chrome, missing puppeteer deps). Exit 0 is a pass, exit 1 is a proof
/// failure and fails the test. The script is a committed triga deliverable;
/// its absence is a repo defect and fails loudly, it is not a skip.
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
    let chain = radix::mir::emit_chain_descriptor(&device_roles, &mir.validated, &interner)
        .expect("emit_chain_descriptor for tiny-linear-device");
    assert!(
        !chain.chain.is_empty(),
        "chain descriptor must contain at least one kernel"
    );

    // ── 2. Fixture validation sub-step (stepper reference contract) ───────
    let fixture = read_reference_fixture(&path, 1).expect("rung 1 reference fixture");
    assert_eq!(fixture.tolerance, 0.00001);
    assert_eq!(
        fixture.reference,
        serde_json::json!([9.1, 12.2, 18.1, 24.2, 27.1, 36.2, 36.1, 48.2])
    );

    // ── 3. Serialize the chain descriptor to JSON ─────────────────────────
    let descriptor_json =
        serde_json::to_string(&chain).expect("serialize chain descriptor to JSON");

    // ── 4. Build input data keyed by the descriptor's storage-buffer
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

    // ── 5. Write artifacts to the managed temp root ───────────────────────
    let temp_root = make_temp_root();
    let descriptor_file = temp_root.join("chain-descriptor.json");
    fs::write(&descriptor_file, &descriptor_json).expect("write chain descriptor JSON");
    // The expected reference is passed as the ref.json fixture path
    // (spec §3.5 validation shape: `--expected <main.ref.json>`).
    let expected_ref_path = crate::paths::package_corpus_dir().join(row.reference_path);
    assert!(
        expected_ref_path.is_file(),
        "expected reference fixture missing: {}",
        expected_ref_path.display()
    );

    // ── 6. Spawn the Node proof script ────────────────────────────────────
    if !command_available("node", &["--version"]) {
        eprintln!(
            "SKIP: node not on PATH — headless Chrome WebGPU environment unavailable \
             (recorded condition, not a pass)"
        );
        return;
    }
    let script = triga_scripta_dir().join("w4-06c-gpu-chain-proof.mjs");
    assert!(
        script.is_file(),
        "proof script {} must be present (committed triga deliverable); refusing to skip a repo defect",
        script.display()
    );

    let output = Command::new("node")
        .arg(&script)
        .arg("--descriptor")
        .arg(&descriptor_file)
        .arg("--input")
        .arg(&input_json)
        .arg("--expected")
        .arg(&expected_ref_path)
        .arg("--tolerance")
        .arg("0.00001")
        .output()
        .expect("spawn node chain proof script");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    match output.status.code() {
        // Exit 0 — the chain dispatched on headless Chrome WebGPU and the
        // readback matched the stepper reference within 0.00001.
        Some(0) => {}
        // Exit 2 — deterministic environment error from the proof script
        // (missing Chrome for Testing / puppeteer deps). Recorded condition,
        // NOT a pass; the proof did not execute.
        Some(2) => {
            eprintln!(
                "SKIP: headless Chrome WebGPU unavailable (proof script exit 2, recorded \
                 condition, not a pass):\nstderr:\n{stderr}"
            );
            return;
        }
        code => panic!(
            "chain dispatch proof failed (exit {code:?}):\nstdout:\n{stdout}\nstderr:\n{stderr}"
        ),
    }
}

/// D-W6-B2 (this delivery): LIVE headless Chrome WebGPU relu-chain dispatch
/// proof — the rung-2 `OutputChecked` evidence rests on real device
/// execution, not fixture reading alone (wave-4 council item 10).
///
/// Compiles the tiny-linear-device-relu exemplar through the faber pipeline,
/// extracts the G-SPINE-10 [`KernelChainDescriptor`] from compiler output,
/// spawns the Node.js Puppeteer proof script
/// (`triga/scripta/w4-06d-gpu-relu-proof.mjs`), which dispatches the
/// compiler-emitted rung-2 relu chain (matmul → add → relu, fused into one
/// kernel by D-W6-B1's emit_chain_descriptor) through `dispatchChainFromDescriptor`
/// (hosts) in headless Chrome WebGPU (SwiftShader), reads back the output, and
/// asserts a match to the stepper reference `[10.1, 0.0, 0.0, 24.2, 28.1, 0.0,
/// 0.0, 48.2]` within f32 tolerance `0.00001` — including the explicit
/// not-identity assertion that exactly 4 of 8 readback elements are 0.0
/// (ReLU zeroes the negative pre-activations; ReLU is provably not identity).
///
/// Fixture validation is a sub-step of this same test: the stepper reference
/// fixture (`main.ref.json`) is still read and asserted so the reference
/// contract cannot drift.
///
/// Skip discipline (spec §5 U2): the test skips with a RECORDED CONDITION only
/// when the headless Chrome/WebGPU environment is unavailable — detected via
/// the proof script's deterministic exit code 2 (environment error: missing
/// Chrome, missing puppeteer deps). Exit 0 is a pass, exit 1 is a proof
/// failure and fails the test. The script is a committed triga deliverable;
/// its absence is a repo defect and fails loudly, it is not a skip.
///
/// # Promotion gate (task 42aed477)
///
/// This test is NON-ignored: the B1-follow-up emitter fix (radix 05a47f864)
/// emits a `workgroupBarrier()` between the fused matmul-add store and the
/// relu pass in the `TensorRelu` emitter arm (`crates/radix-mir-wgsl/src/
/// lib.rs`), closing the intra-workgroup readback race that previously read
/// back all zeros (DeviceResultMismatch, radix d495c2cff). This live dispatch
/// is the rung-2 `OutputChecked` evidence gate; a regression of the barrier
/// ordering fails loudly here (exit 1) and in the wgsl_text ordering test.
#[test]
fn tensor_workload_proof_rung2_device_gpu_chain_dispatch() {
    // ── 1. Compile the exemplar through the faber pipeline ────────────────
    let row = tensor_workload_proof_rows()[2];
    let path = crate::paths::package_corpus_dir().join(row.exemplar_path);
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));

    let session = Session::new(Config::default());
    let mut analysis =
        radix::driver::analyze_source(&session, &path.display().to_string(), &source)
            .expect("frontend analysis of tiny-linear-device-relu");
    let interner = analysis.interner.clone();
    let device_roles = radix::mir::device_roles_from_hir(&analysis.hir);
    let mir = radix::mir::lower_analyzed_unit_with_context(&mut analysis)
        .expect("MIR lowering of tiny-linear-device-relu");
    let chain = radix::mir::emit_chain_descriptor(&device_roles, &mir.validated, &interner)
        .expect("emit_chain_descriptor for tiny-linear-device-relu");
    assert!(
        !chain.chain.is_empty(),
        "chain descriptor must contain at least one kernel"
    );

    // ── 2. Fixture validation sub-step (stepper reference contract) ───────
    let fixture = read_reference_fixture(&path, 2).expect("rung 2 reference fixture");
    assert_eq!(fixture.tolerance, 0.00001);
    assert_eq!(
        fixture.reference,
        serde_json::json!([10.1, 0.0, 0.0, 24.2, 28.1, 0.0, 0.0, 48.2])
    );

    // ── 3. Serialize the chain descriptor to JSON ─────────────────────────
    let descriptor_json =
        serde_json::to_string(&chain).expect("serialize chain descriptor to JSON");

    // ── 4. Build input data keyed by the descriptor's storage-buffer
    //       @binding namespace. dispatchChainFromDescriptor resolves each
    //       bind-group entry via resources.buffers.get(bufDecl.binding).
    //       Output buffers are provided zero-initialized so the dispatch can
    //       bind them. tiny-linear-device-relu inputs: x (12 f32 / 48 B),
    //       w (6 f32 / 24 B), b (8 f32 / 32 B); output y (8 f32 / 32 B).
    //       The signed x/w data produce mixed-sign pre-activations — ReLU
    //       zeroes 4 of 8, proving it is not identity. ─────────────────────
    const X_DATA: &[f64] = &[
        1.0, -2.0, 3.0, -4.0, 5.0, -6.0, 7.0, -8.0, 9.0, -10.0, 11.0, -12.0,
    ];
    const W_DATA: &[f64] = &[1.0, -2.0, 3.0, -4.0, 5.0, -6.0];
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

    // ── 5. Write artifacts to the managed temp root ───────────────────────
    let temp_root = make_temp_root();
    let descriptor_file = temp_root.join("chain-descriptor.json");
    fs::write(&descriptor_file, &descriptor_json).expect("write chain descriptor JSON");
    // The expected reference is passed as the ref.json fixture path
    // (spec §7 validation shape: `--expected <main.ref.json>`).
    let expected_ref_path = crate::paths::package_corpus_dir().join(row.reference_path);
    assert!(
        expected_ref_path.is_file(),
        "expected reference fixture missing: {}",
        expected_ref_path.display()
    );

    // ── 6. Spawn the Node proof script ────────────────────────────────────
    if !command_available("node", &["--version"]) {
        eprintln!(
            "SKIP: node not on PATH — headless Chrome WebGPU environment unavailable \
             (recorded condition, not a pass)"
        );
        return;
    }
    let script = triga_scripta_dir().join("w4-06d-gpu-relu-proof.mjs");
    assert!(
        script.is_file(),
        "proof script {} must be present (committed triga deliverable); refusing to skip a repo defect",
        script.display()
    );

    let output = Command::new("node")
        .arg(&script)
        .arg("--descriptor")
        .arg(&descriptor_file)
        .arg("--input")
        .arg(&input_json)
        .arg("--expected")
        .arg(&expected_ref_path)
        .arg("--tolerance")
        .arg("0.00001")
        .output()
        .expect("spawn node relu proof script");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    match output.status.code() {
        // Exit 0 — the relu chain dispatched on headless Chrome WebGPU, the
        // readback matched the stepper reference within 0.00001, and the
        // not-identity assertion (exactly 4 zeroed elements) held.
        Some(0) => {}
        // Exit 2 — deterministic environment error from the proof script
        // (missing Chrome for Testing / puppeteer deps). Recorded condition,
        // NOT a pass; the proof did not execute.
        Some(2) => {
            eprintln!(
                "SKIP: headless Chrome WebGPU unavailable (proof script exit 2, recorded \
                 condition, not a pass):\nstderr:\n{stderr}"
            );
            return;
        }
        code => panic!(
            "relu chain dispatch proof failed (exit {code:?}):\nstdout:\n{stdout}\nstderr:\n{stderr}"
        ),
    }
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
