use super::common::{
    collect_exempla_files, format_forma_diagnostics, format_result_paths, is_expected_failure,
};
use super::types::E2eResult;
use radix::driver::{Config, Session};
use radix::forma::compile_canonical;
use std::fs;

// Live radix/corpus debt (2026-07-30): first-compile rejects, second-compile
// forma/semantic gaps, and non-idempotent forma emits. Paths must fail; when a
// row starts passing, remove it (do not leave stale expected entries).
const FABER_ROUNDTRIP_EXPECTED_FAILURES: &[&str] = &[
    // First-compile: intentional reject / frontend SEM policy
    "gpu-core-types/atomic-element-reject.fab",
    "gpu-core-types/f16-bf16-reject.fab",
    "gpu-core-types/matrix-tensor-reject.fab",
    "json/json.fab",
    "praefixum/praefixum.fab",
    "protecta/protecta.fab",
    "rumpe/rumpe-top-level-error.fab",
    "sparsa/conversio-reject.fab",
    "sparsa/non-numeric-reject.fab",
    "tensor/arithmetic-reject.fab",
    "tensor/placement-execution-v1.fab",
    "typi/sized-family-error.fab",
    // Second-compile: forma emit re-check fails (name_not_value / parse / types)
    "abstractus/abstractus.fab",
    "cli/cli.fab",
    "conversio/collectiones.fab",
    "discerne/discerne.fab",
    "est/est.fab",
    "membrum/membrum.fab",
    "nihil/nihil.fab",
    "si/ergo-redde.fab",
    "vector/builtins.fab",
    // Non-idempotent forma emit (fenced-doc / forma surface)
    // Y: quarantined 2026-08-06; full Radix e2e exposed additional forma roundtrip instability.
    "adfirma/in-functione.fab",
    "ante/ante.fab",
    "assertio/nonnulla.fab",
    "assignatio/assignatio.fab",
    "aut/aut.fab",
    "cede/cede.fab",
    "ceteri/ceteri.fab",
    "fient/fient.fab",
    "fiet/fiet.fab",
    "figendum/figendum.fab",
    "fiunt/fiunt.fab",
    "literalia/forma.fab",
    // Y: quarantined 2026-08-06; full Radix e2e exposed additional forma roundtrip instability.
    "nota/gradus.fab",
    "nota/nota.fab",
    "octeti/octeti.fab",
    "omnia/omnia.fab",
    "optionalis/optionalis.fab",
    "ordo/ordo.fab",
    "per/per.fab",
    "perge/perge.fab",
    "reddet/reddet.fab",
    "tacebit/tacebit.fab",
    "variandum/variandum.fab",
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
