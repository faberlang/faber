use super::super::common::make_temp_root;
use super::super::wasm_external::{run_wasm_entry_with_stub_host, WasmRunBucket};
use super::{behavior_matches, expected_wasm_behavior, WASM_BEHAVIOR_FIXTURES};
use radix::codegen::Target;
use radix::driver::Session;
use radix::Config;
use std::fs;

#[test]
#[ignore = "requires external radix-wasm-stub-host; run: cargo test -p exempla --lib wasm_behavior_fixtures_match_stub_host_diag_traces -- --ignored --nocapture"]
fn wasm_behavior_fixtures_match_stub_host_diag_traces() {
    let exempla_dir = crate::paths::corpus_dir();
    let session = Session::new(Config::default().with_target(Target::Wasm));
    let temp_root = make_temp_root();

    for fixture in WASM_BEHAVIOR_FIXTURES {
        let path = exempla_dir.join(fixture.exemplum);
        let source = fs::read_to_string(&path).unwrap_or_else(|err| {
            panic!("cannot read {}: {err}", path.display());
        });
        let analysis =
            radix::driver::analyze_source(&session, &path.display().to_string(), &source).unwrap();
        let mut analysis = analysis;
        let interner = analysis.interner.clone();
        let mir = radix::mir::lower_analyzed_unit_with_context(&mut analysis).unwrap();
        let bytes = radix::mir::emit_wasm_binary_probe_with_context(
            &mir.program,
            &mir.validation,
            &interner,
        )
        .unwrap();
        let wasm_file = temp_root.join(format!("{}.wasm", fixture.exemplum.replace('/', "_")));
        fs::write(&wasm_file, bytes).unwrap();
        let run_probe = run_wasm_entry_with_stub_host(&wasm_file);
        assert_eq!(
            run_probe.bucket,
            WasmRunBucket::Runnable,
            "{}: {}",
            fixture.exemplum,
            run_probe.reason
        );
        assert!(
            behavior_matches(fixture.expected_diag, &run_probe.diag_events),
            "{}: expected {:?}, got {:?}",
            fixture.exemplum,
            fixture.expected_diag,
            run_probe.diag_events
        );
        assert_eq!(
            expected_wasm_behavior(fixture.exemplum),
            Some(fixture.expected_diag)
        );
    }
}
