//! Script stepper exempla e2e harness: analyze, lower, interpret, stdout compare.

use super::common::{
    collect_exempla_files, format_diagnostic_messages, format_result_paths, normalize_newline,
    read_expected_stdout,
};
use super::types::E2eResult;
use radix::driver::Session;
use radix::mir::{run_source, BufferHost, RunSourceError};
use radix::Config;
use std::fs;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ScriptFailureBucket {
    FrontendNegative,
    PackageOnly,
    NormaImport,
    CliProgram,
    MirBackedTargetOnly,
    CapabilityStream,
    NoEntryReference,
    UnsupportedMir,
}

impl ScriptFailureBucket {
    pub(super) const ALL: [Self; 8] = [
        Self::FrontendNegative,
        Self::PackageOnly,
        Self::NormaImport,
        Self::CliProgram,
        Self::MirBackedTargetOnly,
        Self::CapabilityStream,
        Self::NoEntryReference,
        Self::UnsupportedMir,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::FrontendNegative => "frontend-negative",
            Self::PackageOnly => "package-only",
            Self::NormaImport => "norma-import",
            Self::CliProgram => "cli-program",
            Self::MirBackedTargetOnly => "mir-backed-target-only",
            Self::CapabilityStream => "capability-stream",
            Self::NoEntryReference => "no-entry-reference",
            Self::UnsupportedMir => "unsupported-mir",
        }
    }

    fn explanation(self) -> &'static str {
        match self {
            Self::FrontendNegative => "intentional invalid-program or diagnostic exemplar",
            Self::PackageOnly => "requires package/import behavior outside in-memory script mode",
            Self::NormaImport => "imports norma:*; script mode uses faber:* kernel imports",
            Self::CliProgram => "depends on CLI-specific entry or operand lowering",
            Self::MirBackedTargetOnly => {
                "requires a MIR-backed emit target, not script/default Rust analysis"
            }
            Self::CapabilityStream => {
                "depends on ad, SermoOpen, cursor, or async stream capability"
            }
            Self::NoEntryReference => "reference or declaration exemplar without runnable incipit",
            Self::UnsupportedMir => "real MIR lowering or stepper debt",
        }
    }
}

pub(super) struct ExpectedScriptFailure {
    path: &'static str,
    bucket: ScriptFailureBucket,
}

/// Exempla that do not yet pass the MIR script stepper harness.
///
/// The bucket is part of the ratchet: non-script surfaces are explicit, while
/// `unsupported-mir` remains implementation debt that future phases should shrink.
const SCRIPT_EXPECTED_FAILURES: &[ExpectedScriptFailure] = &[
    expected(
        "ad/sermo-recovery.fab",
        ScriptFailureBucket::CapabilityStream,
    ),
    expected("ad/sermo-vacuum.fab", ScriptFailureBucket::CapabilityStream),
    expected("ad/async-solum-leget.fab", ScriptFailureBucket::NormaImport),
    expected(
        "ad/async-tempus-dormiet.fab",
        ScriptFailureBucket::NormaImport,
    ),
    expected(
        "ad/solum-lege-generic.fab",
        ScriptFailureBucket::NormaImport,
    ),
    expected("air/air-lane.fab", ScriptFailureBucket::MirBackedTargetOnly),
    // WHY: annotation-sugar braced markers are non-runnable reference/CLI/lane
    // exempla; bucketed so the script harness reflects ground truth. These were
    // added by the annotation-sugar campaign without harness entries.
    expected(
        "annotation-sugar/cli-braced.fab",
        ScriptFailureBucket::CliProgram,
    ),
    expected(
        "annotation-sugar/optio-braced.fab",
        ScriptFailureBucket::CliProgram,
    ),
    expected("argumenta/argumenta.fab", ScriptFailureBucket::CliProgram),
    expected("chorda/angustat.fab", ScriptFailureBucket::NormaImport),
    expected("chorda/diducta.fab", ScriptFailureBucket::NormaImport),
    expected("chorda/discidit.fab", ScriptFailureBucket::NormaImport),
    expected("chorda/retine.fab", ScriptFailureBucket::NormaImport),
    expected("cli/cli.fab", ScriptFailureBucket::CliProgram),
    expected("curata/curata.fab", ScriptFailureBucket::NoEntryReference),
    expected("descriptio/descriptio.fab", ScriptFailureBucket::CliProgram),
    expected("errata/errata.fab", ScriptFailureBucket::NoEntryReference),
    expected("est/est.fab", ScriptFailureBucket::UnsupportedMir),
    expected(
        "sparsa/conversio-reject.fab",
        ScriptFailureBucket::FrontendNegative,
    ),
    expected(
        "sparsa/non-numeric-reject.fab",
        ScriptFailureBucket::FrontendNegative,
    ),
    expected("sparsa/access.fab", ScriptFailureBucket::UnsupportedMir),
    expected("sparsa/conversio.fab", ScriptFailureBucket::UnsupportedMir),
    expected("sparsa/decl.fab", ScriptFailureBucket::UnsupportedMir),
    expected("sparsa/sugar.fab", ScriptFailureBucket::UnsupportedMir),
    expected(
        "tensor/arithmetic-reject.fab",
        ScriptFailureBucket::FrontendNegative,
    ),
    expected(
        "tensor/method-errors.fab",
        ScriptFailureBucket::FrontendNegative,
    ),
    expected("exitus/exitus.fab", ScriptFailureBucket::CliProgram),
    expected(
        "fragilis/fragilis.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected("futurum/futurum.fab", ScriptFailureBucket::NoEntryReference),
    expected(
        "gpu-core-types/atomic-element-reject.fab",
        ScriptFailureBucket::FrontendNegative,
    ),
    expected(
        "gpu-core-types/atomic-operations.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected(
        "gpu-core-types/f16-bf16-reject.fab",
        ScriptFailureBucket::FrontendNegative,
    ),
    expected(
        "gpu-core-types/f16-width.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected(
        "gpu-core-types/matrix-register.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected(
        "gpu-core-types/matrix-tensor-reject.fab",
        ScriptFailureBucket::FrontendNegative,
    ),
    expected(
        "operatores/numerus-overflow.fab",
        ScriptFailureBucket::UnsupportedMir,
    ),
    expected(
        "script-kernel/glob-import.fab",
        ScriptFailureBucket::UnsupportedMir,
    ),
    expected(
        "script-kernel/solum-json.fab",
        ScriptFailureBucket::UnsupportedMir,
    ),
    expected(
        "immutata/immutata.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected("importa/importa.fab", ScriptFailureBucket::PackageOnly),
    expected("instans/instans.fab", ScriptFailureBucket::NormaImport),
    expected("json/json.fab", ScriptFailureBucket::NormaImport),
    // removed: Stage 4 implemented Cede/CursorStream; itera/cursor-iteratio now runs.
    expected("meta/versio.fab", ScriptFailureBucket::CliProgram),
    expected("meta/requirit.fab", ScriptFailureBucket::NoEntryReference),
    expected("numquam/numquam.fab", ScriptFailureBucket::NoEntryReference),
    expected("omitte/omitte.fab", ScriptFailureBucket::NoEntryReference),
    expected("operandus/operandus.fab", ScriptFailureBucket::CliProgram),
    expected("optio/optio.fab", ScriptFailureBucket::CliProgram),
    expected(
        "optiones/optiones.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected(
        "postpara/postpara.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected(
        "postparabit/postparabit.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected("prae/prae.fab", ScriptFailureBucket::NoEntryReference),
    expected(
        "praepara/praepara.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected(
        "praeparabit/praeparabit.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected("proba/proba.fab", ScriptFailureBucket::NoEntryReference),
    expected(
        "probandum/probandum.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected(
        "protecta/protecta.fab",
        ScriptFailureBucket::FrontendNegative,
    ),
    expected("repete/repete.fab", ScriptFailureBucket::NoEntryReference),
    expected(
        "rumpe/rumpe-top-level-error.fab",
        ScriptFailureBucket::FrontendNegative,
    ),
    expected(
        "scalaria/scalaria.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected("solum/solum.fab", ScriptFailureBucket::NoEntryReference),
    expected(
        "solum-in/solum-in.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected("sponte/sponte.fab", ScriptFailureBucket::NoEntryReference),
    expected(
        "stdlib-nativum/caelum-terminus.fab",
        ScriptFailureBucket::NormaImport,
    ),
    expected(
        "stdlib-nativum/chorda.fab",
        ScriptFailureBucket::NormaImport,
    ),
    expected(
        "stdlib-nativum/csv-chorda.fab",
        ScriptFailureBucket::NormaImport,
    ),
    expected(
        "stdlib-nativum/mathesis-operators.fab",
        ScriptFailureBucket::NormaImport,
    ),
    expected(
        "stdlib-nativum/mathesis.fab",
        ScriptFailureBucket::NormaImport,
    ),
    expected(
        "stdlib-nativum/retorta.fab",
        ScriptFailureBucket::NormaImport,
    ),
    expected(
        "stdlib-nativum/tempus-civil.fab",
        ScriptFailureBucket::NormaImport,
    ),
    expected(
        "stdlib-nativum/tensor-applicata.fab",
        ScriptFailureBucket::NormaImport,
    ),
    expected(
        "stdlib-nativum/tensor-bridge.fab",
        ScriptFailureBucket::NormaImport,
    ),
    expected(
        "stdlib-nativum/vector-pending-placeholder.fab",
        ScriptFailureBucket::FrontendNegative,
    ),
    expected(
        "stdlib-nativum/toml-exige-claves.fab",
        ScriptFailureBucket::NormaImport,
    ),
    expected(
        "stdlib-nativum/toml-navigatio.fab",
        ScriptFailureBucket::NormaImport,
    ),
    expected("tag/tag.fab", ScriptFailureBucket::NoEntryReference),
    expected(
        "temporis/temporis.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected(
        "typi/sized-family-error.fab",
        ScriptFailureBucket::FrontendNegative,
    ),
    expected("ubique/ubique.fab", ScriptFailureBucket::CliProgram),
    expected("vector/builtins.fab", ScriptFailureBucket::NoEntryReference),
    expected("vector/cross.fab", ScriptFailureBucket::NoEntryReference),
    expected("vector/decl.fab", ScriptFailureBucket::NoEntryReference),
    expected("vector/dot.fab", ScriptFailureBucket::NoEntryReference),
    expected(
        "vector/elementwise.fab",
        ScriptFailureBucket::NoEntryReference,
    ),
    expected("vector/infer.fab", ScriptFailureBucket::NoEntryReference),
    expected("vector/kernel.fab", ScriptFailureBucket::NoEntryReference),
    expected("vector/sugar.fab", ScriptFailureBucket::NoEntryReference),
    expected("vector/swizzle.fab", ScriptFailureBucket::NoEntryReference),
];

const EXPECTED_SCRIPT_RUN_FLOOR: usize = 215;
const EXPECTED_SCRIPT_OUTPUT_CHECKED_FLOOR: usize = 60;

#[test]
#[ignore = "slow script e2e; run: cargo test -p exempla --lib exempla_script_e2e -- --ignored --nocapture"]
fn exempla_script_e2e() {
    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);
    assert!(
        !exempla.is_empty(),
        "script e2e harness found no exempla files"
    );

    let session = Session::new(Config::default());
    let mut results = Vec::with_capacity(exempla.len());
    let mut ran = 0usize;
    let mut output_checked = 0usize;

    for file in &exempla {
        let expected = read_expected_stdout(file);
        let result = run_script_exemplum(&session, file);
        match result {
            Ok(stdout) => {
                ran += 1;
                if let Some(expected) = expected {
                    output_checked += 1;
                    if stdout != expected {
                        results.push(E2eResult {
                            path: file.clone(),
                            passed: false,
                            reason: format!(
                                "stdout mismatch: expected `{expected}`, got `{stdout}`"
                            ),
                        });
                        continue;
                    }
                }
                results.push(E2eResult {
                    path: file.clone(),
                    passed: true,
                    reason: String::new(),
                });
            }
            Err(reason) => {
                results.push(E2eResult {
                    path: file.clone(),
                    passed: false,
                    reason,
                });
            }
        }
    }

    let pass_count = results.iter().filter(|r| r.passed).count();
    eprintln!(
        "Script e2e exempla: {pass_count}/{} exempla files pass end-to-end",
        results.len()
    );
    eprintln!("  stepper ran: {ran}/{}", exempla.len());
    eprintln!("  output checked: {output_checked}/{}", exempla.len());
    eprintln!("  floors: run>={EXPECTED_SCRIPT_RUN_FLOOR}, output_checked>={EXPECTED_SCRIPT_OUTPUT_CHECKED_FLOOR}");
    eprintln!("  expected failure buckets:");
    for bucket in ScriptFailureBucket::ALL {
        let count = count_expected_failure_bucket(bucket);
        if count > 0 {
            eprintln!("    {}: {count} ({})", bucket.label(), bucket.explanation());
        }
    }

    for fail in results.iter().filter(|r| !r.passed) {
        eprintln!("[fail] {} :: {}", fail.path.display(), fail.reason);
    }

    assert!(
        ran >= EXPECTED_SCRIPT_RUN_FLOOR,
        "script run floor regression: {ran}"
    );
    assert!(
        output_checked >= EXPECTED_SCRIPT_OUTPUT_CHECKED_FLOOR,
        "script output-checked floor regression: {output_checked}"
    );

    let unexpected_failures = results
        .iter()
        .filter(|r| !r.passed && expected_script_failure_bucket(&r.path).is_none())
        .collect::<Vec<_>>();
    let unexpected_passes = results
        .iter()
        .filter(|r| r.passed && expected_script_failure_bucket(&r.path).is_some())
        .collect::<Vec<_>>();

    assert!(
        unexpected_failures.is_empty(),
        "unexpected script e2e failures: {}",
        format_result_paths(&unexpected_failures)
    );
    assert!(
        unexpected_passes.is_empty(),
        "script e2e classified expected failures now pass and should be removed from SCRIPT_EXPECTED_FAILURES: {}",
        format_result_paths(&unexpected_passes)
    );
}

const fn expected(path: &'static str, bucket: ScriptFailureBucket) -> ExpectedScriptFailure {
    ExpectedScriptFailure { path, bucket }
}

pub(super) fn expected_script_failure_bucket(path: &Path) -> Option<ScriptFailureBucket> {
    SCRIPT_EXPECTED_FAILURES
        .iter()
        .find_map(|expected| path.ends_with(expected.path).then_some(expected.bucket))
}

pub(super) fn count_expected_failure_bucket(bucket: ScriptFailureBucket) -> usize {
    SCRIPT_EXPECTED_FAILURES
        .iter()
        .filter(|expected| expected.bucket == bucket)
        .count()
}

fn run_script_exemplum(session: &Session, file: &Path) -> Result<String, String> {
    let source = fs::read_to_string(file).map_err(|err| format!("cannot read source: {err}"))?;
    let path_label = file.display().to_string();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut host = BufferHost::default();
        let outcome = run_source(session, &path_label, &source, &mut host);
        (outcome, host)
    }));

    let (outcome, host) = match run {
        Ok(pair) => pair,
        Err(_) => return Err("stepper aborted".to_owned()),
    };

    if let Err(error) = outcome {
        return Err(match error {
            RunSourceError::Frontend(diagnostics) => {
                format!(
                    "frontend failed: {}",
                    format_diagnostic_messages(&diagnostics)
                )
            }
            RunSourceError::Mir(errors) => {
                format!(
                    "MIR lowering failed: {}",
                    errors
                        .into_iter()
                        .map(|error| error.issue)
                        .collect::<Vec<_>>()
                        .join(" | ")
                )
            }
            RunSourceError::Stepper(errors) => {
                format!(
                    "stepper failed: {}",
                    errors
                        .into_iter()
                        .map(|error| error.issue)
                        .collect::<Vec<_>>()
                        .join(" | ")
                )
            }
        });
    }
    Ok(normalize_newline(&host.stdout_lines.join("\n")))
}
