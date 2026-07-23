use super::common::{
    collect_exempla_files, format_diagnostics, is_expected_failure, normalize_newline,
    read_expected_stdout,
};
use super::types::E2eResult;
use radix::{codegen::Target, tool::compile_cli_path, Output};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Exempla that are expected to fail Swift emission (codegen not implemented
/// for these expression/statement shapes yet).
///
/// SC-002+ops/scribe: 54 exempla pass. The remaining exempla use features
/// deferred to SC-003 (collections), SC-004 (classes/structs), SC-005 (enums),
/// SC-006 (failable), SC-007 (optionals), SC-008 (stdlib), or are
/// intentionally rejected by Faber semantic analysis.
const SWIFT_EXPECTED_FAILURES: &[&str] = &[
    "abstractus/abstractus.fab",
    "ad/async-solum-leget.fab",
    "ad/async-tempus-dormiet.fab",
    "ad/sermo-conversio.fab",
    "ad/sermo-live-directional.fab",
    "ad/sermo-recovery.fab",
    "ad/sermo-tuus.fab",
    "ad/sermo-vacuum.fab",
    "ad/solum-lege-generic.fab",
    "adfirma/adfirma.fab",
    "adfirma/in-functione.fab",
    "annotation-sugar/cli-braced.fab",
    "annotation-sugar/optio-braced.fab",
    "argumenta/argumenta.fab",
    "assertio/nonnulla.fab",
    "aut/aut.fab",
    "binarius/binarius.fab",
    "cape/cape.fab",
    "cede/cede.fab",
    "ceteri/ceteri.fab",
    "clausa/clausa.fab",
    "clausura/clausura.fab",
    "cli/cli.fab",
    "conversio/bivalens.fab",
    "conversio/collectiones.fab",
    "conversio/conversio.fab",
    "conversio/fallibilis.fab",
    "conversio/instans-valor-carrier.fab",
    "conversio/instans.fab",
    "conversio/lista-tensor-shaped.fab",
    "conversio/numeric-bool.fab",
    "conversio/octeti.fab",
    "conversio/radix.fab",
    "conversio/rectangular-lista-literal-tensor.fab",
    "conversio/regex.fab",
    "conversio/tensor.fab",
    "conversio/valor-genus.fab",
    "conversio/valor-boxing.fab",
    "conversio/valor-scalaria.fab",
    "conversio/valor-tensor.fab",
    "conversio/verte-aggregate.fab",
    "cura/nidificatus.fab",
    "cursor/cursor.fab",
    "custodi/custodi.fab",
    "de/de.fab",
    "descriptio/descriptio.fab",
    "discerne/discerne.fab",
    "dum/conditio-complexa.fab",
    "dum/dum.fab",
    "dum/in-functione.fab",
    "elige/ceterum.fab",
    "elige/ergo-redde.fab",
    "elige/in-functione.fab",
    "est/est.fab",
    "et/et.fab",
    "ex/ex.fab",
    "exitus/exitus.fab",
    "fac/fac-cape.fab",
    "fac/fac-dum.fab",
    "falsum/falsum.fab",
    "finge/finge.fab",
    "fixum/fixum.fab",
    "fragilis/fragilis.fab",
    "functio/functio.fab",
    "functio/generic-call-type-args.fab",
    "functio/in-ex.fab",
    "functio/recursio.fab",
    "functio/sponte-vel.fab",
    "functio/typi-parametri.fab",
    "futura/futura.fab",
    "futurum/futurum.fab",
    "generic/generic.fab",
    "generic/genus.fab",
    "genus/creo.fab",
    "gpu-core-types/atomic-element-reject.fab",
    "gpu-core-types/atomic-operations.fab",
    "gpu-core-types/f16-bf16-reject.fab",
    "gpu-core-types/matrix-register.fab",
    "gpu-core-types/matrix-tensor-reject.fab",
    "iace/functio-fallibilis.fab",
    "iace/functio-propagans.fab",
    "iace/iace-si-guard.fab",
    "iace/iace.fab",
    "implendum/implendum.fab",
    "implet/implet.fab",
    "importa/auxilium.fab",
    "importa/importa.fab",
    "in/in.fab",
    "incipiet/incipiet.fab",
    "incipit/functionibus.fab",
    "instans/instans.fab",
    "integratio/arena-mixta.fab",
    "integratio/destructura-sparsa.fab",
    "integratio/discerne-insanum.fab",
    "integratio/fluxus-cede.fab",
    "integratio/minimum-smoke.fab",
    "inter/inter.fab",
    "intervallum/algebra.fab",
    "intervallum/conversio.fab",
    "intrinseca/copia-algebra.fab",
    "intrinseca/copia-fundamenta.fab",
    "intrinseca/fractus-approximata.fab",
    "intrinseca/fractus-comparatio.fab",
    "intrinseca/fractus-rotundatio.fab",
    "intrinseca/numeric-operator-methods.fab",
    "intrinseca/numerus-methodi.fab",
    "intrinseca/primitiva.fab",
    "intrinseca/textus-quaestiones.fab",
    "intrinseca/textus-transformationes.fab",
    "intrinseca/vacua-ascribere.fab",
    "itera/cursor-iteratio.fab",
    "itera/de.fab",
    "itera/in-functione.fab",
    "lege/lege.fab",
    "lista/lista.fab",
    "lista/methodi-accessus.fab",
    "lista/methodi-copiae.fab",
    "lista/methodi-functionales.fab",
    "lista/methodi-mutatio.fab",
    "literalia/ascii.fab",
    "literalia/block-string.fab",
    "literalia/boolean.fab",
    "literalia/forma.fab",
    "literalia/nihil.fab",
    "literalia/regex.fab",
    "literalia/textus.fab",
    "membrum/membrum.fab",
    "meta/requirit.fab",
    "meta/versio.fab",
    "mone/mone.fab",
    "mori/mori-si-guard.fab",
    "mori/mori.fab",
    "morphologia/morphologia.fab",
    "negativum/negativum.fab",
    "nexum/nexum.fab",
    "nihil/nihil.fab",
    "non/non.fab",
    "nonnihil/nonnihil.fab",
    "nonnulla/nonnulla.fab",
    "nota/gradus.fab",
    "nota/nota.fab",
    "nulla/nulla.fab",
    "numquam/numquam.fab",
    "octet/octet.fab",
    "octeti/octeti.fab",
    "octeti/unify.fab",
    "omitte/omitte.fab",
    "omnia/omnia.fab",
    "operandus/operandus.fab",
    "operatores/comparatio.fab",
    "operatores/control.fab",
    "operatores/function-types.fab",
    "operatores/logica.fab",
    "operatores/metadata.fab",
    "operatores/modular-word-sha-round.fab",
    "operatores/modular-word-u16.fab",
    "operatores/modular-word-u64-sha-round.fab",
    "operatores/modular-word-u64.fab",
    "operatores/modular-word-u8.fab",
    "operatores/modular-word.fab",
    "operatores/nonnull-chain.fab",
    "operatores/numeric-value-eq.fab",
    "operatores/numerus-overflow.fab",
    "operatores/optional-chain.fab",
    "optio/optio.fab",
    "optionalis/optionalis.fab",
    "ordo/ordo.fab",
    "perge/perge.fab",
    "positivum/positivum.fab",
    "postpara/postpara.fab",
    "postparabit/postparabit.fab",
    "prae/prae.fab",
    "praefixum/praefixum.fab",
    "praepara/praepara.fab",
    "praeparabit/praeparabit.fab",
    "proba/proba.fab",
    "probandum/probandum.fab",
    "redde/redde.fab",
    "repete/repete.fab",
    "rumpe/fac-dum-rumpe.fab",
    "rumpe/fac-si-rumpe.fab",
    "rumpe/rumpe-top-level-error.fab",
    "rumpe/rumpe.fab",
    "scriptum/scriptum.fab",
    "si/ergo-redde.fab",
    "sit/sit.fab",
    "solum-in/solum-in.fab",
    "solum/solum.fab",
    "sparge/sparge.fab",
    "sparsa/access.fab",
    "sparsa/conversio-reject.fab",
    "sparsa/conversio.fab",
    "sparsa/decl.fab",
    "sparsa/non-numeric-reject.fab",
    "sparsa/sparsa-codegen-smoke.fab",
    "sparsa/sugar.fab",
    "sub/sub.fab",
    "tabula/methodi-accessus.fab",
    "tabula/tabula.fab",
    "tacet/tacet.fab",
    "tag/tag.fab",
    "temporis/temporis.fab",
    "tensor-fragment/tiny-linear-device/src/main.fab",
    "tensor-fragment/tiny-linear/src/main.fab",
    "tensor-package/fmir-matmul/src/main.fab",
    "tensor/arithmetic-elementwise.fab",
    "tensor/arithmetic-matmul.fab",
    "tensor/arithmetic-reduction.fab",
    "tensor/arithmetic-reject.fab",
    "tensor/bracket-access.fab",
    "tensor/decl.fab",
    "tensor/index-width.fab",
    "tensor/method-errors.fab",
    "tensor/method-policy.fab",
    "tensor/placement-execution-v1.fab",
    "tensor/shape.fab",
    "tensor/textus.fab",
    "ternarius/ternarius.fab",
    "typi/sized-family-error.fab",
    "typi/sized-numerus.fab",
    "ubique/ubique.fab",
    "unarius/unarius.fab",
    "unio/unio.fab",
    "ut/ut.fab",
    "vacuum/vacuum.fab",
    "varia/typi-ligata.fab",
    "vector/builtins.fab",
    "vector/cross.fab",
    "vector/dot.fab",
    "vector/elementwise.fab",
    "vector/infer.fab",
    "vector/kernel.fab",
    "vector/swizzle.fab",
    "vel/vel.fab",
    "verum/verum.fab",
    "vocatio/vocatio.fab",
];

/// Exempla expected to fail with a specific Faber compile-error message.
const SWIFT_EXPECTED_COMPILE_FAILURES: &[(&str, &str)] = &[
    // JSON types are not supported for Swift.
    ("destructura/literal.fab", "swift_json_unsupported"),
    ("json/json.fab", "swift_json_unsupported"),
    // Protected/test fixtures with compiler-level gates.
    ("protecta/protecta.fab", "protecta_reserved"),
];

/// Baseline floor for accepted outcomes (passes + expected failures).
const EXPECTED_SWIFT_PASS_FLOOR: usize = 67;
const EXPECTED_SWIFT_ACCEPTED_OUTCOME_FLOOR: usize = 70;
const EXPECTED_SWIFT_EXPECTED_FAILURE_CEILING: usize = 260;

/// Compile a single Faber exemplum to Swift via the single-file emit path.
fn compile_swift_exemplum(file: &Path) -> Result<String, String> {
    let result = compile_cli_path(file, false, Target::Swift);
    match result.output {
        Some(Output::Swift(output)) => Ok(output.code),
        Some(_) => Err("compiler did not produce Swift output".to_owned()),
        None => {
            let diagnostics = format_diagnostics(&result);
            Err(diagnostics)
        }
    }
}

/// Returns whether `swiftc` is on PATH and responds to `--version`.
fn swift_available() -> bool {
    super::common::command_available("swiftc", &["--version"])
}

#[test]
#[ignore = "slow swift e2e; run: cargo test -p exempla --test e2e_harness exempla_swift_e2e -- --ignored --nocapture"]
fn exempla_swift_e2e() {
    if !swift_available() {
        eprintln!("swiftc not found on PATH; skipping Swift exempla end-to-end harness");
        return;
    }

    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);
    let total = exempla.len();
    let mut results: Vec<E2eResult> = Vec::with_capacity(total);
    let mut expected_count = 0usize;

    for (idx, file) in exempla.iter().enumerate() {
        let relative = file
            .strip_prefix(&exempla_dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| file.display().to_string());

        let expected = read_expected_stdout(file);
        if expected.is_some() {
            expected_count += 1;
        }

        let compiled = compile_swift_exemplum(file);

        let code = match compiled {
            Ok(code) => {
                if expected_compile_failure(file).is_some() {
                    eprintln!(
                        "[swift-e2e {idx:03}/{total}] {relative}  stale-expected-compile-fail"
                    );
                    results.push(E2eResult {
                        path: file.clone(),
                        passed: false,
                        reason: "expected compile failure now compiles".to_owned(),
                    });
                    continue;
                }
                code
            }
            Err(reason) => {
                if let Some(expected) = expected_compile_failure(file) {
                    let passed = reason.contains(expected);
                    let reason = if passed {
                        format!("expected compile failure: {expected}")
                    } else {
                        format!(
                            "expected compile failure containing `{expected}`, got: {reason}"
                        )
                    };
                    eprintln!(
                        "[swift-e2e {idx:03}/{total}] {relative}  expected-compile-fail"
                    );
                    results.push(E2eResult {
                        path: file.clone(),
                        passed,
                        reason,
                    });
                    continue;
                }
                eprintln!(
                    "[swift-e2e {idx:03}/{total}] {relative}  compile-fail"
                );
                results.push(E2eResult {
                    path: file.clone(),
                    passed: false,
                    reason,
                });
                continue;
            }
        };

        // Compile the emitted Swift with swiftc.
        let stem = file
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("exemplum");
        let swift_file = std::env::temp_dir().join(format!("swift_e2e_{idx:03}_{stem}.swift"));
        let binary = std::env::temp_dir().join(format!("swift_e2e_{idx:03}_{stem}"));

        if let Err(err) = fs::write(&swift_file, &code) {
            results.push(E2eResult {
                path: file.clone(),
                passed: false,
                reason: format!("cannot write Swift source: {err}"),
            });
            continue;
        }

        let compile = Command::new("swiftc")
            .arg(&swift_file)
            .arg("-o")
            .arg(&binary)
            .output();

        match compile {
            Ok(output) if output.status.success() => {
                // Run the compiled binary.
                let run = Command::new(&binary).output();
                match run {
                    Ok(output) => {
                        let stdout = normalize_newline(&String::from_utf8_lossy(&output.stdout));
                        if let Some(expected) = &expected {
                            if stdout != *expected {
                                results.push(E2eResult {
                                    path: file.clone(),
                                    passed: false,
                                    reason: format!(
                                        "stdout mismatch: expected `{expected}`, got `{stdout}`"
                                    ),
                                });
                            } else {
                                results.push(E2eResult {
                                    path: file.clone(),
                                    passed: true,
                                    reason: String::new(),
                                });
                            }
                        } else {
                            results.push(E2eResult {
                                path: file.clone(),
                                passed: true,
                                reason: String::new(),
                            });
                        }
                    }
                    Err(err) => {
                        results.push(E2eResult {
                            path: file.clone(),
                            passed: false,
                            reason: format!("cannot execute Swift binary: {err}"),
                        });
                    }
                }
                // Clean up binary.
                let _ = fs::remove_file(&binary);
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                results.push(E2eResult {
                    path: file.clone(),
                    passed: false,
                    reason: format!("swiftc compilation failed: {stderr}"),
                });
            }
            Err(err) => {
                results.push(E2eResult {
                    path: file.clone(),
                    passed: false,
                    reason: format!("cannot execute swiftc: {err}"),
                });
            }
        }

        // Clean up Swift source file.
        let _ = fs::remove_file(&swift_file);

        let label = if results.last().map_or(false, |r| r.passed) {
            "OK"
        } else {
            "FAIL"
        };
        eprintln!("[swift-e2e {idx:03}/{total}] {relative}  {label}");
    }

    // ---- Summary --------------------------------------------------------------
    let total = results.len();
    let accepted_count = results
        .iter()
        .filter(|r| r.passed || is_expected_failure(&r.path, SWIFT_EXPECTED_FAILURES))
        .count();
    let pass_count = results
        .iter()
        .filter(|r| r.passed && expected_compile_failure(&r.path).is_none())
        .count();
    let expected_compile_fail_count = results
        .iter()
        .filter(|r| r.passed && expected_compile_failure(&r.path).is_some())
        .count();
    let unaccepted_count = total.saturating_sub(accepted_count);
    let unexpected_failures = results
        .iter()
        .filter(|r| !r.passed && !is_expected_failure(&r.path, SWIFT_EXPECTED_FAILURES))
        .collect::<Vec<_>>();
    let unexpected_passes = results
        .iter()
        .filter(|r| r.passed && is_expected_failure(&r.path, SWIFT_EXPECTED_FAILURES))
        .collect::<Vec<_>>();

    eprintln!(
        "Swift e2e exempla: {accepted_count}/{total} accepted outcomes ({pass_count} run, {expected_compile_fail_count} expected compile failures)"
    );
    eprintln!("Expected-output checks enabled for {expected_count} exempla files");
    eprintln!(
        "Unaccepted failures: {unaccepted_count} total, {} unexpected",
        unexpected_failures.len()
    );

    for result in results.iter().filter(|r| !r.passed) {
        let label = if is_expected_failure(&result.path, SWIFT_EXPECTED_FAILURES) {
            "tracked"
        } else {
            "fail"
        };
        eprintln!("[{label}] {} :: {}", result.path.display(), result.reason);
    }

    assert!(
        pass_count >= EXPECTED_SWIFT_PASS_FLOOR,
        "Swift e2e pass count regressed: {pass_count}/{} below floor {EXPECTED_SWIFT_PASS_FLOOR}",
        total,
    );
    assert!(
        accepted_count >= EXPECTED_SWIFT_ACCEPTED_OUTCOME_FLOOR,
        "Swift e2e accepted outcomes regressed: {accepted_count}/{total} below floor {EXPECTED_SWIFT_ACCEPTED_OUTCOME_FLOOR}",
    );
    assert!(
        SWIFT_EXPECTED_FAILURES.len() <= EXPECTED_SWIFT_EXPECTED_FAILURE_CEILING,
        "Swift e2e expected-failure metadata grew: {} above ceiling {EXPECTED_SWIFT_EXPECTED_FAILURE_CEILING}",
        SWIFT_EXPECTED_FAILURES.len(),
    );
    assert!(
        unexpected_failures.is_empty(),
        "unexpected Swift e2e failures: {}",
        format_result_paths(&unexpected_failures)
    );
    assert!(
        unexpected_passes.is_empty(),
        "Swift e2e expected failures now pass and should be removed from metadata: {}",
        format_result_paths(&unexpected_passes)
    );
}

fn expected_compile_failure(path: &Path) -> Option<&'static str> {
    SWIFT_EXPECTED_COMPILE_FAILURES
        .iter()
        .find_map(|(expected_path, expected_message)| {
            path.ends_with(expected_path).then_some(*expected_message)
        })
}

fn format_result_paths(results: &[&E2eResult]) -> String {
    results
        .iter()
        .map(|r| r.path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn swift_expected_failure_metadata_ceiling_is_ratcheted() {
    assert!(
        SWIFT_EXPECTED_FAILURES.len() <= EXPECTED_SWIFT_EXPECTED_FAILURE_CEILING,
        "Swift e2e expected-failure metadata grew: {} above ceiling {EXPECTED_SWIFT_EXPECTED_FAILURE_CEILING}",
        SWIFT_EXPECTED_FAILURES.len(),
    );
}

#[test]
fn swift_expected_failure_ledgers_are_disjoint() {
    for (compile_failure, _) in SWIFT_EXPECTED_COMPILE_FAILURES {
        assert!(
            !SWIFT_EXPECTED_FAILURES.contains(compile_failure),
            "{compile_failure} is listed as both an expected failure and an expected compile failure",
        );
    }
}

#[test]
fn swift_expected_failure_ledgers_reference_current_corpus() {
    let corpus = crate::paths::corpus_dir();
    let missing = SWIFT_EXPECTED_FAILURES
        .iter()
        .copied()
        .chain(SWIFT_EXPECTED_COMPILE_FAILURES.iter().map(|(path, _)| *path))
        .filter(|path| !corpus.join(path).is_file())
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "Swift expected-failure metadata references paths outside the public corpus: {}",
        missing.join(", ")
    );
}
