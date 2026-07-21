use super::{normalize_pairwise_output, rust_oracle, RustOracleOutcome};
use std::path::Path;

#[test]
fn classifies_each_outcome_family() {
    assert!(matches!(
        rust_oracle(Path::new("corpus/incipit/salve-munde.fab")),
        RustOracleOutcome::RunSuccess { .. }
    ));
    assert!(matches!(
        rust_oracle(Path::new("corpus/curata/curata.fab")),
        RustOracleOutcome::DeclarationOnly { .. }
    ));
    assert!(matches!(
        rust_oracle(Path::new("corpus/operatores/numerus-overflow.fab")),
        RustOracleOutcome::ExpectedRuntimeFailure { .. }
    ));
    assert!(matches!(
        rust_oracle(Path::new("corpus/exitus/exitus.fab")),
        RustOracleOutcome::ExpectedNonzeroExit { exit_code: 1, .. }
    ));
    assert!(matches!(
        rust_oracle(Path::new("corpus/protecta/protecta.fab")),
        RustOracleOutcome::ExpectedCompileFailure { .. }
    ));
    assert!(matches!(
        rust_oracle(Path::new("corpus/air/air-lane.fab")),
        RustOracleOutcome::ExplicitWrongLane { .. }
    ));
}

#[test]
fn normalization_contract_is_crlf_to_lf_only() {
    assert_eq!(normalize_pairwise_output("a\r\nb\r\n"), "a\nb\n");
}
