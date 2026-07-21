use super::script::{
    count_expected_failure_bucket, expected_script_failure_bucket, ScriptFailureBucket,
};
use std::path::Path;

#[test]
fn script_expected_failure_lookup_returns_bucket() {
    let path = Path::new("/tmp/corpus/cli/cli.fab");
    assert_eq!(
        expected_script_failure_bucket(path),
        Some(ScriptFailureBucket::CliProgram)
    );
}

#[test]
fn script_expected_failure_lookup_rejects_unclassified_path() {
    let path = Path::new("/tmp/corpus/conversio/tensor.fab");
    assert_eq!(expected_script_failure_bucket(path), None);
}

#[test]
fn script_expected_failure_bucket_counts_are_non_empty_for_current_non_debt_taxonomy() {
    for bucket in ScriptFailureBucket::ALL {
        if matches!(bucket, ScriptFailureBucket::UnsupportedMir) {
            continue;
        }
        assert!(
            count_expected_failure_bucket(bucket) > 0,
            "bucket {} should classify at least one current expected failure",
            bucket.label()
        );
    }
}
