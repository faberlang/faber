use super::*;

#[test]
fn llvm_host_gap_ledger_is_structurally_valid() {
    let ledger = parse_gap_ledger(GAP_LEDGER).expect("checked-in LLVM host gap ledger must parse");
    // Count pinned to the live-regenerated ledger (measured 2026-08-06@live-pairwise-s86;
    // was stale at 143). Update this pin when the ledger is next regenerated.
    assert_eq!(ledger.gap.len(), 33);
}

#[test]
fn llvm_host_comparison_preserves_trailing_newlines() {
    let oracle = RustOracleOutcome::RunSuccess {
        args: &[],
        stdout: super::super::oracle::ExpectedStdout::SiblingFixture,
        exit_code: 0,
    };
    let rust = ProcessOutcome {
        exit_code: Some(0),
        stdout: b"salve\n".to_vec(),
        stderr: vec![],
    };
    let llvm = LlvmOutcome::Ran(ProcessOutcome {
        exit_code: Some(0),
        stdout: b"salve".to_vec(),
        stderr: vec![],
    });
    assert_eq!(
        compare_pair(oracle, Some(&rust), Some(&llvm), None),
        Comparison::Mismatch {
            boundary: Boundary::Outcome,
            issue: "stdout_mismatch".to_owned()
        }
    );
}

#[test]
fn llvm_host_comparison_normalizes_crlf_only() {
    let oracle = RustOracleOutcome::RunSuccess {
        args: &[],
        stdout: super::super::oracle::ExpectedStdout::SiblingFixture,
        exit_code: 0,
    };
    let rust = ProcessOutcome {
        exit_code: Some(0),
        stdout: b"a\nb\n".to_vec(),
        stderr: vec![],
    };
    let llvm = LlvmOutcome::Ran(ProcessOutcome {
        exit_code: Some(0),
        stdout: b"a\r\nb\r\n".to_vec(),
        stderr: vec![],
    });
    assert_eq!(
        compare_pair(oracle, Some(&rust), Some(&llvm), None),
        Comparison::Pass
    );
}

#[test]
fn llvm_host_gap_ledger_rejects_duplicates_and_wrong_owners() {
    let source = r#"
schema_version = 1
measured = "today"
rust_executable_denominator = 1
gap_ceiling = 2
[[gap]]
path = "a.fab"
owner_stage = 4
boundary = "frontend"
issue = "x"
reason = "x"
first_seen = "today"
[[gap]]
path = "a.fab"
owner_stage = 3
boundary = "frontend"
issue = "x"
reason = "x"
first_seen = "today"
"#;
    let error = parse_gap_ledger(source).expect_err("invalid ledger must fail");
    assert!(error.contains("wrong owner") || error.contains("duplicate"));
}

#[test]
fn llvm_host_gap_ledger_limits_stage8_mir_to_cli_records() {
    let source = r#"
schema_version = 1
measured = "today"
rust_executable_denominator = 1
gap_ceiling = 1
[[gap]]
path = "a.fab"
owner_stage = 8
boundary = "mir"
issue = "mir_lowering_rejected"
reason = "x"
first_seen = "today"
"#;
    let error = parse_gap_ledger(source).expect_err("broad Stage 8 MIR owner must fail");
    assert!(error.contains("wrong owner"));
}

#[test]
fn llvm_host_async_solum_leget_reaches_native_link() {
    assert_reaches_native_link("ad/async-solum-leget.fab");
}

#[test]
fn llvm_host_async_solum_leget_uses_existing_route_poll_boundary() {
    let path = crate::paths::corpus_dir().join("ad/async-solum-leget.fab");
    let config = radix::Config::default().with_target(radix::codegen::Target::LlvmText);
    let llvm = faber_cli::package::with_lowered_package_mir(&config, &path, |lowered| {
        let interner = lowered
            .validated
            .validation()
            .interner
            .ok_or_else(|| "package MIR validation context has no interner".to_owned())?;
        radix::mir::emit_llvm_text_probe(&lowered.validated, interner)
            .map_err(|error| format!("{}:{}", error.category, error.shape))
    })
    .expect("async solum package analysis must succeed")
    .expect("async solum package LLVM emission must succeed");

    assert!(llvm.contains("declare i32 @poll(ptr, i32, i32)"), "{llvm}");
    assert!(
        llvm.contains("call i32 @poll(ptr null, i32 0, i32 0)"),
        "{llvm}"
    );
    assert!(llvm.contains("@__faber_rt_v1_solum_read_text"), "{llvm}");
    assert!(!llvm.contains("__faber_runtime_provider"), "{llvm}");
}

#[test]
#[ignore = "known breakage: async tempus MIR lowering rejected (await/yield); see need 8ea13983"]
fn llvm_host_async_tempus_dormiet_reaches_native_link() {
    assert_reaches_native_link("ad/async-tempus-dormiet.fab");
}

fn assert_reaches_native_link(relative_path: &str) {
    let path = crate::paths::corpus_dir().join(relative_path);
    let session = radix::driver::Session::new(
        radix::Config::default().with_target(radix::codegen::Target::LlvmText),
    );
    let toolchain = crate::harness::llvm::detect_llvm_toolchain();
    assert!(
        toolchain.is_available(),
        "LLVM host parity requires llvm-as or opt"
    );

    let result = crate::harness::llvm::classify_llvm_exemplum(
        &session,
        &path,
        0,
        &super::super::common::make_temp_root(),
        &toolchain,
    );
    let probe = result
        .run_probe
        .unwrap_or_else(|| panic!("async solum did not reach native link: {}", result.reason));
    assert_ne!(
        probe.bucket,
        crate::harness::llvm_runtime::LlvmRunBucket::LinkFailed,
        "{}",
        probe.reason
    );
    assert_ne!(
        probe.bucket,
        crate::harness::llvm_runtime::LlvmRunBucket::ToolchainMissing,
        "{}",
        probe.reason
    );
}

#[test]
fn llvm_host_comparison_rejects_exit_code_mismatch() {
    let oracle = RustOracleOutcome::RunSuccess {
        args: &[],
        stdout: super::super::oracle::ExpectedStdout::SiblingFixture,
        exit_code: 0,
    };
    let rust = ProcessOutcome {
        exit_code: Some(0),
        stdout: b"ok\n".to_vec(),
        stderr: vec![],
    };
    let llvm = LlvmOutcome::Ran(ProcessOutcome {
        exit_code: Some(1),
        stdout: b"ok\n".to_vec(),
        stderr: vec![],
    });
    let comparison = compare_pair(oracle, Some(&rust), Some(&llvm), None);
    assert_eq!(
        comparison,
        Comparison::Mismatch {
            boundary: Boundary::Outcome,
            issue: "exit_code_mismatch".to_owned(),
        }
    );
}

#[test]
fn llvm_host_comparison_accepts_perfect_match() {
    let oracle = RustOracleOutcome::RunSuccess {
        args: &[],
        stdout: super::super::oracle::ExpectedStdout::SiblingFixture,
        exit_code: 0,
    };
    let rust = ProcessOutcome {
        exit_code: Some(0),
        stdout: b"salve\n".to_vec(),
        stderr: vec![],
    };
    let llvm = LlvmOutcome::Ran(ProcessOutcome {
        exit_code: Some(0),
        stdout: b"salve\n".to_vec(),
        stderr: vec![],
    });
    assert_eq!(
        compare_pair(oracle, Some(&rust), Some(&llvm), None),
        Comparison::Pass
    );
}

#[test]
fn llvm_host_comparison_ignores_stderr_for_run_success() {
    // The RunSuccess parity contract compares exit code and normalized stdout
    // only. stderr is not part of the success contract — it is checked only for
    // ExpectedRuntimeFailure oracles via `stderr_contains`. This test pins that
    // contract so a stderr-only divergence cannot be mistaken for a parity break.
    let oracle = RustOracleOutcome::RunSuccess {
        args: &[],
        stdout: super::super::oracle::ExpectedStdout::SiblingFixture,
        exit_code: 0,
    };
    let rust = ProcessOutcome {
        exit_code: Some(0),
        stdout: b"ok\n".to_vec(),
        stderr: vec![],
    };
    let llvm = LlvmOutcome::Ran(ProcessOutcome {
        exit_code: Some(0),
        stdout: b"ok\n".to_vec(),
        stderr: b"unexpected\n".to_vec(),
    });
    assert_eq!(
        compare_pair(oracle, Some(&rust), Some(&llvm), None),
        Comparison::Pass
    );
}
