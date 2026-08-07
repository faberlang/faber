use super::script::{
    count_expected_failure_bucket, expected_script_failure_bucket, ScriptFailureBucket,
};
use std::path::Path;

#[test]
fn script_expected_failure_lookup_rejects_former_cli_program_path() {
    // d0621e6 (quarantine late e2e baseline gaps) removed every `cli-program`
    // row from SCRIPT_EXPECTED_FAILURES, so the former cli/cli.fab row no
    // longer classifies to a bucket.
    let path = Path::new("/tmp/corpus/cli/cli.fab");
    assert_eq!(expected_script_failure_bucket(path), None);
}

#[test]
fn script_expected_failure_lookup_rejects_unclassified_path() {
    let path = Path::new("/tmp/corpus/conversio/tensor.fab");
    assert_eq!(expected_script_failure_bucket(path), None);
}

#[test]
fn script_expected_failure_bucket_counts_are_non_empty_for_current_non_debt_taxonomy() {
    for bucket in ScriptFailureBucket::ALL {
        // `unsupported-mir` stays implementation debt that may be empty; the
        // `cli-program` and `capability-stream` buckets are empty because
        // d0621e6 removed every CliProgram/CapabilityStream row (quarantine
        // late e2e baseline gaps) — asserted explicitly below.
        if matches!(
            bucket,
            ScriptFailureBucket::UnsupportedMir
                | ScriptFailureBucket::CliProgram
                | ScriptFailureBucket::CapabilityStream
        ) {
            continue;
        }
        assert!(
            count_expected_failure_bucket(bucket) > 0,
            "bucket {} should classify at least one current expected failure",
            bucket.label()
        );
    }

    // Post-removal state: d0621e6 removed the CliProgram and CapabilityStream
    // rows (quarantine late e2e baseline gaps), so both buckets are empty.
    assert_eq!(
        count_expected_failure_bucket(ScriptFailureBucket::CliProgram),
        0,
        "cli-program bucket should be empty since d0621e6 removed the rows"
    );
    assert_eq!(
        count_expected_failure_bucket(ScriptFailureBucket::CapabilityStream),
        0,
        "capability-stream bucket should be empty since d0621e6 removed the rows"
    );
}
