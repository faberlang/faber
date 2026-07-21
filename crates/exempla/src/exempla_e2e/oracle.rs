//! Shared Rust-oracle metadata for backend pairwise corpus checks.
//!
//! The Rust executable lane is the behavioral authority. This module owns the
//! path classifications and run contracts that backend harnesses may consume;
//! target harnesses must not grow independent copies of these tables.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpectedStdout {
    /// Compare against the sibling `.expected` file when one exists.
    SiblingFixture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RustOracleOutcome {
    RunSuccess {
        args: &'static [&'static str],
        stdout: ExpectedStdout,
        exit_code: i32,
    },
    DeclarationOnly {
        stdout: ExpectedStdout,
        exit_code: i32,
    },
    ExpectedRuntimeFailure {
        args: &'static [&'static str],
        stderr_contains: &'static str,
    },
    ExpectedNonzeroExit {
        args: &'static [&'static str],
        exit_code: i32,
        stdout: ExpectedStdout,
    },
    ExpectedCompileFailure {
        issue: &'static str,
    },
    ExplicitWrongLane {
        issue: &'static str,
    },
}

impl RustOracleOutcome {
    pub(crate) fn is_executable(self) -> bool {
        matches!(
            self,
            Self::RunSuccess { .. }
                | Self::DeclarationOnly { .. }
                | Self::ExpectedRuntimeFailure { .. }
                | Self::ExpectedNonzeroExit { .. }
        )
    }

    pub(crate) fn expected_compile_issue(self) -> Option<&'static str> {
        match self {
            Self::ExpectedCompileFailure { issue } | Self::ExplicitWrongLane { issue } => {
                Some(issue)
            }
            _ => None,
        }
    }

    pub(crate) fn run_args(self) -> &'static [&'static str] {
        match self {
            Self::RunSuccess { args, .. }
            | Self::ExpectedRuntimeFailure { args, .. }
            | Self::ExpectedNonzeroExit { args, .. } => args,
            Self::DeclarationOnly { .. }
            | Self::ExpectedCompileFailure { .. }
            | Self::ExplicitWrongLane { .. } => &[],
        }
    }
}

const EXPECTED_COMPILE_FAILURES: &[(&str, &str)] = &[
    ("gpu-core-types/atomic-element-reject.fab", "atomic_element"),
    (
        "gpu-core-types/atomic-operations.fab",
        "rust_target_atomic_unsupported",
    ),
    ("gpu-core-types/f16-bf16-reject.fab", "unknown_type"),
    (
        "gpu-core-types/f16-width.fab",
        "rust_target_fractus_f16_unsupported",
    ),
    (
        "gpu-core-types/matrix-tensor-reject.fab",
        "expression_type_mismatch",
    ),
    ("protecta/protecta.fab", "protecta_reserved"),
    ("rumpe/rumpe-top-level-error.fab", "break_outside_breakable"),
    ("sparsa/conversio-reject.fab", "expression_type_mismatch"),
    (
        "sparsa/non-numeric-reject.fab",
        "sparsa_element_non_numeric",
    ),
    (
        "stdlib-nativum/vector-pending-placeholder.fab",
        "unknown_method",
    ),
    (
        "tensor/arithmetic-reject.fab",
        "tensor_arithmetic_numeric_element_required",
    ),
    ("typi/sized-family-error.fab", "float_width_on_numerus"),
];

const WRONG_LANE_FAILURES: &[(&str, &str)] = &[
    ("air/air-lane.fab", "lane_requires_mir_backed_target"),
    (
        "gpu-core-types/matrix-register.fab",
        "rust_target_matrix_unsupported",
    ),
    (
        "script-kernel/aleator-uuid.fab",
        "kernel_import_script_mode_only",
    ),
    (
        "script-kernel/glob-import.fab",
        "kernel_import_script_mode_only",
    ),
    (
        "script-kernel/kernel-import.fab",
        "kernel_import_script_mode_only",
    ),
    (
        "script-kernel/processus-argumenta.fab",
        "kernel_import_script_mode_only",
    ),
    (
        "script-kernel/processus-exsequi.fab",
        "kernel_import_script_mode_only",
    ),
    (
        "script-kernel/solum-json.fab",
        "kernel_import_script_mode_only",
    ),
];

const RUNTIME_FAILURES: &[(&str, &str)] = &[
    (
        "instans/instans.fab",
        "norma:toml.solve is deferred pending Stage 2 dispatch",
    ),
    ("operatores/numerus-overflow.fab", "numerus overflow"),
    (
        "tensor/method-errors.fab",
        "tensor structa element count does not match shape",
    ),
];

const DECLARATION_ONLY_FIXTURES: &[&str] = &[
    "curata/curata.fab",
    "errata/errata.fab",
    "fragilis/fragilis.fab",
    "futurum/futurum.fab",
    "immutata/immutata.fab",
    "meta/requirit.fab",
    "numquam/numquam.fab",
    "omitte/omitte.fab",
    "optiones/optiones.fab",
    "postpara/postpara.fab",
    "postparabit/postparabit.fab",
    "prae/prae.fab",
    "praepara/praepara.fab",
    "praeparabit/praeparabit.fab",
    "proba/proba.fab",
    "probandum/probandum.fab",
    "repete/repete.fab",
    "scalaria/scalaria.fab",
    "solum-in/solum-in.fab",
    "solum/solum.fab",
    "sponte/sponte.fab",
    "tag/tag.fab",
    "temporis/temporis.fab",
    "vector/builtins.fab",
    "vector/cross.fab",
    "vector/decl.fab",
    "vector/dot.fab",
    "vector/elementwise.fab",
    "vector/infer.fab",
    "vector/kernel.fab",
    "vector/swizzle.fab",
    "vector/sugar.fab",
];

pub(crate) fn rust_oracle(path: &Path) -> RustOracleOutcome {
    if let Some(issue) = matching_value(path, WRONG_LANE_FAILURES) {
        return RustOracleOutcome::ExplicitWrongLane { issue };
    }
    if let Some(issue) = matching_value(path, EXPECTED_COMPILE_FAILURES) {
        return RustOracleOutcome::ExpectedCompileFailure { issue };
    }
    if let Some(stderr_contains) = matching_value(path, RUNTIME_FAILURES) {
        return RustOracleOutcome::ExpectedRuntimeFailure {
            args: &[],
            stderr_contains,
        };
    }
    if path.ends_with("exitus/exitus.fab") {
        return RustOracleOutcome::ExpectedNonzeroExit {
            args: &[],
            exit_code: 1,
            stdout: ExpectedStdout::SiblingFixture,
        };
    }
    if DECLARATION_ONLY_FIXTURES
        .iter()
        .any(|expected| path.ends_with(expected))
    {
        return RustOracleOutcome::DeclarationOnly {
            stdout: ExpectedStdout::SiblingFixture,
            exit_code: 0,
        };
    }

    let args = if path.ends_with("cli/cli.fab") {
        &["greet", "Marcus"][..]
    } else if path.ends_with("operandus/operandus.fab") {
        &["input.txt", "extra.txt"][..]
    } else {
        &[]
    };
    RustOracleOutcome::RunSuccess {
        args,
        stdout: ExpectedStdout::SiblingFixture,
        exit_code: 0,
    }
}

fn matching_value(path: &Path, entries: &'static [(&str, &str)]) -> Option<&'static str> {
    entries
        .iter()
        .find_map(|(expected_path, value)| path.ends_with(expected_path).then_some(*value))
}

#[cfg(test)]
#[path = "oracle_test.rs"]
mod tests;

/// Pairwise output normalization deliberately preserves trailing newlines.
pub(crate) fn normalize_pairwise_output(text: &str) -> String {
    text.replace("\r\n", "\n")
}
