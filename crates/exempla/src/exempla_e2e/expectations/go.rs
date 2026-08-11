//! Go lane expected-failure tables (moved verbatim from the go harness under per-lane ownership; no rows re-authored).
//!
//! Go-lane expected-outcome surface (per-lane-e2e-validation EL-4). The
//! lane harness module consumes only this table via
//! `super::expectations::go::…`; no other lane may absorb these rows.

pub(crate) const GO_EXPECTED_FAILURES: &[&str] = &[
    // Provider-gated async routes + generic solum:lege (norma Go host shims
    // are faber/provider work, not the Go emitter; codex-gap Stage 3 U3
    // residual).
    "ad/async-solum-leget.fab",
    "ad/async-tempus-dormiet.fab",
    "ad/solum-lege-generic.fab",
    // Y: quarantined 2026-08-06 during full Radix e2e. Kernel proof rows are
    // not yet valid Go host executables.
    "cuda/addita-proof.fab",
    "cuda/matmul-proof.fab",
    "cuda/summa-proof.fab",
    "gpu-core-types/matrix-register.fab",
    // Y: quarantined 2026-08-06 during full Radix e2e. Go package/import and
    // generated syntax issues need codegen triage outside this validation pass.
    "importa/importa.fab",
    // Norma-gated (tempus/toml/valor host shims are faber/provider work); the
    // U2 JSON/valor carriers do not lift the norma import resolution.
    "instans/instans.fab",
    // The JSON literal carrier (U2) makes the fixture compile, but the
    // norma:json wire encode/decode host surface (`json.pange`/`solve`/
    // `tempta`) is not materialized on Go — provider-gated like instans.
    "json/json.fab",
    "vector/builtins.fab",
    "vector/cross.fab",
    "vector/decl.fab",
    "vector/dot.fab",
    "vector/elementwise.fab",
    "vector/infer.fab",
    "vector/sugar.fab",
    "vector/swizzle.fab",
];
pub(crate) const GO_EXPECTED_RUNTIME_FAILURES: &[(&str, &str)] = &[
    ("operatores/numerus-overflow.fab", "panic: numerus overflow"),
    (
        "tensor/method-errors.fab",
        "panic: tensor structa element count does not match shape",
    ),
];

pub(crate) const GO_EXPECTED_COMPILE_FAILURES: &[(&str, &str)] = &[
    (
        "annotation-sugar/cli-braced.fab",
        "go_cli_options_unsupported",
    ),
    (
        "annotation-sugar/optio-braced.fab",
        "go_cli_options_unsupported",
    ),
    ("cli/cli.fab", "go_cli_subcommand_unsupported"),
    ("gpu-core-types/atomic-element-reject.fab", "atomic_element"),
    (
        "gpu-core-types/atomic-operations.fab",
        "go_atomic_types_unsupported",
    ),
    ("gpu-core-types/f16-bf16-reject.fab", "unknown_type"),
    ("gpu-core-types/f16-width.fab", "go_type_unsupported"),
    // Async stream posture on Go is fail-closed until a channel carrier lands.
    (
        "itera/cursor-iteratio.fab",
        "go_target_async_stream_unsupported",
    ),
    (
        "gpu-core-types/matrix-tensor-reject.fab",
        "expression_type_mismatch",
    ),
    (
        "operatores/modular-word-sha-round.fab",
        "modular_word_target_unsupported",
    ),
    (
        "operatores/modular-word-u16.fab",
        "modular_word_target_unsupported",
    ),
    (
        "operatores/modular-word-u64-sha-round.fab",
        "modular_word_target_unsupported",
    ),
    (
        "operatores/modular-word-u64.fab",
        "modular_word_target_unsupported",
    ),
    (
        "operatores/modular-word-u8.fab",
        "modular_word_target_unsupported",
    ),
    (
        "operatores/modular-word.fab",
        "modular_word_target_unsupported",
    ),
    ("optio/optio.fab", "go_cli_options_unsupported"),
    // praefixum tabula block annotation still SEM010.
    ("praefixum/praefixum.fab", "expression_type_mismatch"),
    ("protecta/protecta.fab", "protecta_reserved"),
    ("rumpe/rumpe-top-level-error.fab", "break_outside_breakable"),
    ("sparsa/conversio-reject.fab", "expression_type_mismatch"),
    (
        "sparsa/non-numeric-reject.fab",
        "sparsa_element_non_numeric",
    ),
    ("tensor/arithmetic-reject.fab", "expression_type_mismatch"),
    // Placement spine kernel arithmetic not SEM-green yet.
    (
        "tensor/placement-execution-v1.fab",
        "numeric_operands_required",
    ),
    ("typi/sized-family-error.fab", "float_width_on_numerus"),
    ("ubique/ubique.fab", "go_cli_options_unsupported"),
];
pub(crate) const GO_DECLARATION_ONLY_FIXTURES: &[&str] = &[
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
    // Scalar return demos define helpers only (no incipit / main).
    "scalar/return-bool.fab",
    "scalar/return-integer.fab",
    "scalar/return-string.fab",
    "scalaria/scalaria.fab",
    "solum-in/solum-in.fab",
    "solum/solum.fab",
    "sponte/sponte.fab",
    "tag/tag.fab",
    "temporis/temporis.fab",
    "vector/kernel.fab",
];
// Floors ratchet upward only; do not lower to absorb drift.
// 2026-08-01 (GC-001 U0 baseline freeze, Mind-accepted need 3bbb5db2): re-pinned
// from 253/310 to the measured live baseline 247/304. The 6-gap is corpus shrink
// (radix clean-break 3e70afa10 removed 6 run-pass corpus files: emitte,
// negativum, nonnihil, nonnulla, nulla, positivum), not a regression — ledgers
// byte-identical since pin 27a6459, 0 unexpected failures. Triage in
// radix/docs/factory/go-canonical/ledger.md.
//
// 2026-08-07: target-safe bindings and tensor/sparse conversions raised the
// live signed corpus to 251 runnable cases.
//
// 2026-08-07 (codex-gap Stage 3 Go ratchet): U1/U2/U3 carriers moved twelve
// expected-failure rows and two go_json_unsupported compile-failure rows off
// the ledgers, each cited to its proof commit:
//  - U1 (radix 18e88216c): intervallum/{algebra,conversio}.fab,
//    tensor/bracket-access.fab, type-hole-union/type-hole-union.fab.
//  - U2 (radix f30b3c5f6): conversio/{collectiones,valor-boxing,valor-tensor,
//    valor-genus}.fab + the map[string]any JSON object-root carrier
//    (destructura/literal.fab compiles and runs).
//  - U3 (radix ba23d6e09): ad/sermo-{conversio,live-directional,recovery,tuus,
//    vacuum}.fab via the in-process radixSermo frame shim.
// json/json.fab compiles under the U2 carrier but stays a tracked failure:
// the norma:json wire encode/decode host surface (pange/solve/tempta) is not
// materialized on Go (provider-gated, like instans/instans.fab).
// Measured live after the ratchet: pass 265, accepted 308.
pub(crate) const EXPECTED_GO_PASS_FLOOR: usize = 265;
pub(crate) const EXPECTED_GO_ACCEPTED_OUTCOME_FLOOR: usize = 308;
// WHY: Remaining expected failures are tracked Go lowering gaps with
// per-path reopen contracts in docs/factory/go-e2e-failures-matrix/baseline.md.
pub(crate) const EXPECTED_GO_EXPECTED_FAILURE_CEILING: usize = 51;
