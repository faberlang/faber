use super::gpu_workload::{
    compare_numeric_output, parse_numeric_output, read_reference_fixture, GpuReferenceFixture,
};
use std::fs;

#[test]
fn numeric_output_comparison_accepts_array_within_tolerance() {
    let fixture = GpuReferenceFixture {
        tolerance: 0.001,
        reference: serde_json::json!([1.0, 2.0, 3.0]),
    };

    compare_numeric_output("[1.0, 2.0005, 3.0]", &fixture).expect("within tolerance");
}

#[test]
fn numeric_output_comparison_rejects_length_mismatch() {
    let fixture = GpuReferenceFixture {
        tolerance: 0.001,
        reference: serde_json::json!([1.0, 2.0]),
    };

    let err = compare_numeric_output("[1.0]", &fixture).expect_err("length mismatch");
    assert!(err.contains("length mismatch"));
}

#[test]
fn numeric_output_comparison_rejects_tolerance_mismatch() {
    let fixture = GpuReferenceFixture {
        tolerance: 0.001,
        reference: serde_json::json!([1.0]),
    };

    let err = compare_numeric_output("[1.01]", &fixture).expect_err("numeric mismatch");
    assert!(err.contains("numeric output mismatch"));
}

#[test]
fn numeric_output_parser_accepts_line_output() {
    let values = parse_numeric_output("1.0\n2.5\n").expect("numeric lines");
    assert_eq!(values, vec![1.0, 2.5]);
}

#[test]
fn checked_in_expected_stdout_fixtures_match_numeric_references() {
    let workload_dir = crate::paths::gpu_workload_dir();

    for entry in fs::read_dir(workload_dir).expect("read gpu workload corpus") {
        let path = entry.expect("read gpu workload entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("fab") {
            continue;
        }
        let rung = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.strip_prefix("rung-"))
            .and_then(|rest| rest.split('-').next())
            .and_then(|digits| digits.parse::<usize>().ok())
            .expect("rung file name");

        read_reference_fixture(&path, rung)
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    }
}
