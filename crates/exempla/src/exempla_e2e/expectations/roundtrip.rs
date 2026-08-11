//! Roundtrip lane expected-failure table (moved verbatim from the faber roundtrip harness under per-lane ownership; no rows re-authored).
//!
//! Roundtrip-lane expected-outcome surface (per-lane-e2e-validation EL-4). The
//! lane harness module consumes only this table via
//! `super::expectations::roundtrip::…`; no other lane may absorb these rows.

// Live radix/corpus debt (2026-07-30): first-compile rejects, second-compile
// forma/semantic gaps, and non-idempotent forma emits. Paths must fail; when a
// row starts passing, remove it (do not leave stale expected entries).
pub(crate) const FABER_ROUNDTRIP_EXPECTED_FAILURES: &[&str] = &[
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
    // Y: quarantined 2026-08-10; the `↤` chain fixture's canonical emit does
    // not stabilize after one round-trip (forma surface, not codegen).
    "assignatio/conversio-assign.fab",
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
