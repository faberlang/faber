use super::common::{
    collect_exempla_files, format_forma_diagnostics, format_result_paths, is_expected_failure,
};
use super::types::E2eResult;
use radix::driver::{Config, Session};
use radix::forma::compile_canonical;
use std::fs;

const FABER_ROUNDTRIP_EXPECTED_FAILURES: &[&str] = &[
    // Family F — CLI gate (Rust-only CLI lowering)
    "cli/cli.fab",
    // Triage Stage 0 — operatores surface (upstream)
    "operatores/logica.fab",
    // Family G — upstream typecheck (first compile fails; fix in semantic, not forma)
    "chorda/angustat.fab",
    "chorda/discidit.fab",
    "chorda/retine.fab",
    "conversio/valor-genus.fab",
    "instans/instans.fab",
    "stdlib-nativum/chorda.fab",
    "stdlib-nativum/csv-chorda.fab",
    "stdlib-nativum/mathesis-operators.fab",
    "stdlib-nativum/mathesis.fab",
    "stdlib-nativum/retorta.fab",
    "stdlib-nativum/tempus-civil.fab",
    "stdlib-nativum/tensor-applicata.fab",
    "stdlib-nativum/tensor-bridge.fab",
    "stdlib-nativum/vector-pending-placeholder.fab",
    "gpu-core-types/atomic-element-reject.fab",
    "gpu-core-types/f16-bf16-reject.fab",
    "gpu-core-types/matrix-tensor-reject.fab",
    "sparsa/conversio-reject.fab",
    "sparsa/non-numeric-reject.fab",
    "tensor/arithmetic-reject.fab",
    "typi/sized-family-error.fab",
    // Deliberate error exempla (invalid programs)
    "protecta/protecta.fab",
    "rumpe/rumpe-top-level-error.fab",
    "si/ergo-redde.fab",
    // Family C residual — forma template canonical round-trip (author idempotent)
    "literalia/forma.fab",
    // Family D residual — conversio/tensor emit gap
    "conversio/tensor.fab",
];

#[test]
#[ignore = "slow faber roundtrip e2e; run: cargo test -p exempla --test e2e_harness exempla_faber_roundtrip_e2e -- --ignored --nocapture"]
fn exempla_faber_roundtrip_e2e() {
    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);

    let session = Session::new(Config::default());
    let mut results = Vec::with_capacity(exempla.len());

    for file in &exempla {
        let source = match fs::read_to_string(file) {
            Ok(source) => source,
            Err(err) => {
                results.push(E2eResult {
                    path: file.clone(),
                    passed: false,
                    reason: format!("cannot read source: {err}"),
                });
                continue;
            }
        };

        let first = compile_canonical(&session, &file.display().to_string(), &source);
        let Some(first_output) = first.output else {
            results.push(E2eResult {
                path: file.clone(),
                passed: false,
                reason: format!(
                    "first faber compile failed: {}",
                    format_forma_diagnostics(&first)
                ),
            });
            continue;
        };

        let second = compile_canonical(&session, &file.display().to_string(), &first_output.code);
        let Some(second_output) = second.output else {
            results.push(E2eResult {
                path: file.clone(),
                passed: false,
                reason: format!(
                    "second faber compile failed: {}",
                    format_forma_diagnostics(&second)
                ),
            });
            continue;
        };

        if first_output.code != second_output.code {
            results.push(E2eResult {
                path: file.clone(),
                passed: false,
                reason: "faber emit did not stabilize after one round-trip".to_owned(),
            });
            continue;
        }

        if source.trim().is_empty() {
            results.push(E2eResult {
                path: file.clone(),
                passed: false,
                reason: "source file was unexpectedly empty".to_owned(),
            });
            continue;
        }

        results.push(E2eResult {
            path: file.clone(),
            passed: true,
            reason: String::new(),
        });
    }

    let pass_count = results.iter().filter(|r| r.passed).count();
    eprintln!(
        "Faber roundtrip exempla: {pass_count}/{} exempla files stabilize",
        results.len()
    );

    for fail in results.iter().filter(|r| !r.passed) {
        eprintln!("[fail] {} :: {}", fail.path.display(), fail.reason);
    }

    let salve_ok = results
        .iter()
        .find(|r| r.path.ends_with("salve-munde.fab"))
        .map(|r| r.passed)
        .unwrap_or(false);
    assert!(
        salve_ok,
        "salve-munde.fab should stabilize through Faber round-trip"
    );

    let unexpected_failures = results
        .iter()
        .filter(|r| !r.passed && !is_expected_failure(&r.path, FABER_ROUNDTRIP_EXPECTED_FAILURES))
        .collect::<Vec<_>>();
    let unexpected_passes = results
        .iter()
        .filter(|r| r.passed && is_expected_failure(&r.path, FABER_ROUNDTRIP_EXPECTED_FAILURES))
        .collect::<Vec<_>>();

    assert!(
        unexpected_failures.is_empty(),
        "unexpected Faber roundtrip failures: {}",
        format_result_paths(&unexpected_failures)
    );
    assert!(
        unexpected_passes.is_empty(),
        "Faber roundtrip expected failures now pass and should be removed from metadata: {}",
        format_result_paths(&unexpected_passes)
    );
}
