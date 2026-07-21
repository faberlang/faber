//! Sexp/Racket exempla e2e harness: MIR lowering, sexp text emission, and racket checks.

use super::common::{
    collect_exempla_files, format_diagnostics, format_result_paths, is_expected_failure,
    make_temp_root, normalize_newline, racket_available, read_expected_stdout,
};
use super::types::E2eResult;
use radix::codegen::Target;
use radix::driver::Session;
use radix::{Config, Output};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Exempla that do not yet pass the MIR sexp validation end-to-end harness.
const SEXP_EXPECTED_FAILURES: &[&str] = &[
    "ad/sermo-conversio.fab",
    "ad/sermo-live-directional.fab",
    "ad/sermo-recovery.fab",
    "ad/sermo-tuus.fab",
    "ad/sermo-vacuum.fab",
    "ad/solum-lege-generic.fab",
    "annotation-sugar/cli-braced.fab",
    "annotation-sugar/optio-braced.fab",
    "annotation-sugar/radix-lane-braced.fab",
    "argumenta/argumenta.fab",
    "aut/aut.fab",
    "binarius/binarius.fab",
    "cede/cede.fab",
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
    "conversio/valor-boxing.fab",
    "conversio/valor-genus.fab",
    "conversio/valor-scalaria.fab",
    "conversio/valor-tensor.fab",
    "cursor/cursor.fab",
    "descriptio/descriptio.fab",
    "est/est.fab",
    "exitus/exitus.fab",
    "futura/futura.fab",
    "generic/genus.fab",
    "genus/creo.fab",
    "genus/methodi.fab",
    "importa/importa.fab",
    "in/in.fab",
    "incipiet/incipiet.fab",
    "instans/instans.fab",
    "integratio/arena-mixta.fab",
    "integratio/fluxus-cede.fab",
    "intervallum/algebra.fab",
    "intervallum/conversio.fab",
    "intrinseca/copia-algebra.fab",
    "intrinseca/copia-fundamenta.fab",
    "intrinseca/fractus-approximata.fab",
    "intrinseca/textus-quaestiones.fab",
    "intrinseca/textus-transformationes.fab",
    "intrinseca/vacua-ascribere.fab",
    "itera/cursor-iteratio.fab",
    "lege/lege.fab",
    "lista/lista.fab",
    "lista/methodi-copiae.fab",
    "lista/methodi-functionales.fab",
    "lista/methodi-mutatio.fab",
    "literalia/regex.fab",
    "meta/versio.fab",
    "mori/mori.fab",
    "morphologia/morphologia.fab",
    "numquam/numquam.fab",
    "gpu-core-types/atomic-element-reject.fab",
    "gpu-core-types/atomic-operations.fab",
    "gpu-core-types/f16-bf16-reject.fab",
    "gpu-core-types/matrix-register.fab",
    "gpu-core-types/matrix-tensor-reject.fab",
    "omitte/omitte.fab",
    "operandus/operandus.fab",
    "operatores/optional-chain.fab",
    "optio/optio.fab",
    "optionalis/optionalis.fab",
    "praefixum/praefixum.fab",
    "protecta/protecta.fab",
    "rumpe/rumpe-top-level-error.fab",
    "sparsa/access.fab",
    "sparsa/conversio-reject.fab",
    "sparsa/conversio.fab",
    "sparsa/decl.fab",
    "sparsa/non-numeric-reject.fab",
    "sparsa/sparsa-codegen-smoke.fab",
    "sparsa/sugar.fab",
    "tabula/methodi-accessus.fab",
    "tabula/tabula.fab",
    "tensor/arithmetic-elementwise.fab",
    "tensor/arithmetic-matmul.fab",
    "tensor/arithmetic-reduction.fab",
    "tensor/arithmetic-reject.fab",
    "tensor/bracket-access.fab",
    "tensor/index-width.fab",
    "tensor/method-errors.fab",
    "tensor/method-policy.fab",
    "tensor/shape.fab",
    "tensor/textus.fab",
    "tensor-package/fmir-matmul/src/main.fab",
    "typi/sized-family-error.fab",
    "ubique/ubique.fab",
    "vector/builtins.fab",
    "vel/vel.fab",
    "vocatio/vocatio.fab",
];

const EXPECTED_SEXP_EMITTED_FLOOR: usize = 190;
const EXPECTED_RACKET_COMPILED_FLOOR: usize = 185;
const EXPECTED_RACKET_RUN_FLOOR: usize = 185;

#[test]
#[ignore = "slow sexp e2e; run: cargo test -p exempla --test e2e_harness exempla_sexp_e2e -- --ignored --nocapture"]
fn exempla_sexp_e2e() {
    if !racket_available() {
        eprintln!("racket not found on PATH; skipping sexp exempla end-to-end harness");
        return;
    }

    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);
    let session = Session::new(Config::default().with_target(Target::Sexp));
    let temp_root = make_temp_root();
    let mut results = Vec::with_capacity(exempla.len());
    let mut emitted = 0usize;
    let mut compiled = 0usize;
    let mut ran = 0usize;

    for (idx, file) in exempla.iter().enumerate() {
        let expected = read_expected_stdout(file);
        let result = compile_sexp_exemplum(&session, file);
        let output = match result {
            Ok(output) => output,
            Err(reason) => {
                results.push(E2eResult {
                    path: file.clone(),
                    passed: false,
                    reason,
                });
                continue;
            }
        };
        emitted += 1;

        let stem = file
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("exemplum");
        let rkt_file = temp_root.join(format!("{idx:03}-{stem}.rkt"));
        if let Err(err) = fs::write(&rkt_file, &output) {
            results.push(E2eResult {
                path: file.clone(),
                passed: false,
                reason: format!("cannot write racket output: {err}"),
            });
            continue;
        }

        let compile = Command::new("racket").arg("-t").arg(&rkt_file).output();
        let compile = match compile {
            Ok(compile) => compile,
            Err(err) => {
                results.push(E2eResult {
                    path: file.clone(),
                    passed: false,
                    reason: format!("cannot execute racket -t: {err}"),
                });
                continue;
            }
        };
        if !compile.status.success() {
            let stderr = String::from_utf8_lossy(&compile.stderr).trim().to_owned();
            results.push(E2eResult {
                path: file.clone(),
                passed: false,
                reason: format!("racket -t failed: {stderr}"),
            });
            continue;
        }
        compiled += 1;

        let run = Command::new("racket").arg(&rkt_file).output();
        let run = match run {
            Ok(run) => run,
            Err(err) => {
                results.push(E2eResult {
                    path: file.clone(),
                    passed: false,
                    reason: format!("cannot execute racket: {err}"),
                });
                continue;
            }
        };
        if !run.status.success() {
            let stderr = String::from_utf8_lossy(&run.stderr).trim().to_owned();
            results.push(E2eResult {
                path: file.clone(),
                passed: false,
                reason: format!("racket run failed: {stderr}"),
            });
            continue;
        }
        ran += 1;

        if let Some(expected) = expected {
            let stdout = normalize_newline(&String::from_utf8_lossy(&run.stdout));
            if stdout != expected {
                results.push(E2eResult {
                    path: file.clone(),
                    passed: false,
                    reason: format!("stdout mismatch: expected `{expected}`, got `{stdout}`"),
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

    let pass_count = results.iter().filter(|r| r.passed).count();
    eprintln!(
        "Sexp e2e exempla: {pass_count}/{} exempla files pass end-to-end",
        results.len()
    );
    eprintln!("  sexp emitted: {emitted}/{}", exempla.len());
    eprintln!("  racket compiled (-t): {compiled}/{}", exempla.len());
    eprintln!("  racket run: {ran}/{}", exempla.len());
    eprintln!(
        "  floors: emitted>={EXPECTED_SEXP_EMITTED_FLOOR}, compiled>={EXPECTED_RACKET_COMPILED_FLOOR}, run>={EXPECTED_RACKET_RUN_FLOOR}"
    );

    for fail in results.iter().filter(|r| !r.passed) {
        eprintln!("[fail] {} :: {}", fail.path.display(), fail.reason);
    }

    assert!(
        emitted >= EXPECTED_SEXP_EMITTED_FLOOR,
        "sexp emitted floor regression: {emitted}"
    );
    assert!(
        compiled >= EXPECTED_RACKET_COMPILED_FLOOR,
        "racket compile floor regression: {compiled}"
    );
    assert!(
        ran >= EXPECTED_RACKET_RUN_FLOOR,
        "racket run floor regression: {ran}"
    );

    let unexpected_failures = results
        .iter()
        .filter(|r| !r.passed && !is_expected_failure(&r.path, SEXP_EXPECTED_FAILURES))
        .collect::<Vec<_>>();
    let unexpected_passes = results
        .iter()
        .filter(|r| r.passed && is_expected_failure(&r.path, SEXP_EXPECTED_FAILURES))
        .collect::<Vec<_>>();

    assert!(
        unexpected_failures.is_empty(),
        "unexpected sexp e2e failures: {}",
        format_result_paths(&unexpected_failures)
    );
    assert!(
        unexpected_passes.is_empty(),
        "sexp e2e expected failures now pass and should be removed from metadata: {}",
        format_result_paths(&unexpected_passes)
    );
}

fn compile_sexp_exemplum(session: &Session, file: &Path) -> Result<String, String> {
    let result = radix::Compiler::new(session.config.clone()).compile(file);
    match result.output {
        Some(Output::Sexp(output)) => Ok(output.code),
        Some(_) => Err("compiler did not produce sexp output".to_owned()),
        None => {
            let diagnostics = format_diagnostics(&result);
            Err(format!("compile failed: {diagnostics}"))
        }
    }
}
