//! TypeScript lane expected-outcome tables (tier floors + `TS_EXPECTED_OUTCOMES`, moved verbatim under per-lane ownership; no rows re-authored).
//!
//! Ts-lane expected-outcome surface (per-lane-e2e-validation EL-4). The
//! lane harness module consumes only this table via
//! `super::expectations::ts::…`; no other lane may absorb these rows.

// Floors are calibrated to the full language corpus; live asserts use
// `floor_for_corpus` so a small `radix/corpus` scaffold can pass while the
// tree is migrated.
// Floors ratchet upward only. Re-based to the measured 2026-07-31 baseline
// after clean-break `3e70afa10` (radix) removed 6 fully-passing corpus files
// (emitte, negativum, nonnihil, nonnulla, nulla, positivum): corpus denominator
// 310 -> 304, and every tier dropped by exactly 6. The `vector/builtins` seam
// fix (radix-codegen-ts GPU builtin DefId resolution) then raised emitted,
// typecheck-valid, and runnable by one each. The iterator/genus fixes raised
// the live typecheck and runnable counts to 275 and 273. Stage 4 ratchet
// (codex-gap): the TS4-1 modular-word emitter (radix 0ad139a55) and the TS4-2
// JSON-root FaberJson carrier (radix 3f1d3955) moved the six
// `operatores/modular-word*.fab` rows and the four JSON-root rows
// (`conversio/valor-{genus,tensor}.fab`, `json/json.fab`,
// `destructura/literal.fab`) off `TS_EXPECTED_OUTCOMES` and past the
// frontend-analyzed tier — measured emitted 289, typecheck-valid 285,
// runnable 283 (frontend analyzed unchanged at 288). Stage 4 ratchet 2
// (codex-gap TS4-3, radix fd5b8da9b): the failable/narrowing/control-flow
// emitter seams — cape bindings on textus/ignotum error values lower to the
// value itself, breakable `fac {}` bodies wrap in `while (true) {}`, bare
// `↦` conversions inside failable returns are try/caught into an
// `{ ok:false, error }` channel, and statement-only non-void bodies end in
// an explicit tail return of the type's natural zero — moved the five
// `conversio/fallibilis.fab`, `discerne/discerne.fab`, `fac/fac-cape.fab`,
// `rumpe/fac-dum-rumpe.fab`, `rumpe/fac-si-rumpe.fab` rows off
// `TS_EXPECTED_OUTCOMES` and past the typecheck-valid tier — measured
// typecheck-valid 290, runnable 288 (emitted unchanged at 289).
pub(crate) const EXPECTED_TS_FRONTEND_ANALYZED_FLOOR: usize = 288;
pub(crate) const EXPECTED_TS_EMITTED_FLOOR: usize = 289;
pub(crate) const EXPECTED_TS_TYPECHECK_VALID_FLOOR: usize = 290;
pub(crate) const EXPECTED_TS_RUNNABLE_FLOOR: usize = 288;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TsHighestTier {
    FrontendRejected,
    FrontendAnalyzed,
    TypeScriptEmitted,
    TypecheckValid,
    Runnable,
    RunPass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExpectedTsKind {
    CompileFail,
    DeclarationOnly,
    RuntimeFailure,
    RuntimeBehavior,
    BehaviorFailure,
    TrackedGap,
    SplitOut,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExpectedTsOutcome {
    pub(crate) path: &'static str,
    pub(crate) highest_tier: TsHighestTier,
    pub(crate) kind: ExpectedTsKind,
    pub(crate) bucket: &'static str,
    pub(crate) reason_contains: &'static str,
}

pub(crate) const TS_EXPECTED_OUTCOMES: &[ExpectedTsOutcome] = &[
    ExpectedTsOutcome {
        path: "ad/solum-lege-generic.fab",
        // Y: quarantined 2026-08-06; frontend now admits this split-out row,
        // but the TS host module binding is still absent.
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::SplitOut,
        bucket: "package HAL split-out",
        reason_contains: "Cannot find module 'norma:solum'",
    },
    ExpectedTsOutcome {
        path: "cli/cli.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "missing type/variant binding",
        reason_contains: "error TS2304: Cannot find name 'args'",
    },
    ExpectedTsOutcome {
        path: "gpu-core-types/atomic-element-reject.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "atomic_element",
    },
    ExpectedTsOutcome {
        path: "gpu-core-types/f16-bf16-reject.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "unknown_type",
    },
    ExpectedTsOutcome {
        path: "gpu-core-types/matrix-tensor-reject.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "expression_type_mismatch",
    },
    ExpectedTsOutcome {
        path: "importa/default-braced.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::SplitOut,
        bucket: "package HAL split-out",
        reason_contains:
            "Cannot find module 'norma:chorda' or its corresponding type declarations.",
    },
    ExpectedTsOutcome {
        path: "importa/default-minimal.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::SplitOut,
        bucket: "package HAL split-out",
        reason_contains:
            "Cannot find module 'norma:chorda' or its corresponding type declarations.",
    },
    ExpectedTsOutcome {
        path: "instans/instans.fab",
        // Y: quarantined 2026-08-06; frontend now admits this split-out row,
        // but TS module bindings for tempus/toml/valor are still absent.
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::SplitOut,
        bucket: "package HAL split-out",
        reason_contains: "Cannot find module 'norma:tempus'",
    },
    ExpectedTsOutcome {
        path: "lege/lege.fab",
        highest_tier: TsHighestTier::TypecheckValid,
        kind: ExpectedTsKind::RuntimeFailure,
        bucket: "runtime input provider gap",
        reason_contains: "ReferenceError: prompt is not defined",
    },
    ExpectedTsOutcome {
        path: "protecta/protecta.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "protecta_reserved",
    },
    ExpectedTsOutcome {
        path: "rumpe/rumpe-top-level-error.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "break_outside_breakable",
    },
    ExpectedTsOutcome {
        path: "sparsa/conversio-reject.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "sparsa_tensor_shape_mismatch",
    },
    ExpectedTsOutcome {
        path: "sparsa/non-numeric-reject.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "sparsa_element_non_numeric",
    },
    ExpectedTsOutcome {
        path: "tensor/arithmetic-reject.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "tensor_arithmetic_numeric_element_required",
    },
    ExpectedTsOutcome {
        path: "tensor/method-errors.fab",
        highest_tier: TsHighestTier::TypecheckValid,
        kind: ExpectedTsKind::RuntimeBehavior,
        bucket: "expected runtime error behavior",
        reason_contains: "tensor structa element count does not match shape",
    },
    ExpectedTsOutcome {
        path: "typi/sized-family-error.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "float_width_on_numerus",
    },
    ExpectedTsOutcome {
        path: "ad/async-solum-leget.fab",
        // Y: quarantined 2026-08-06; frontend now admits this split-out row,
        // but the TS host module binding is still absent.
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::SplitOut,
        bucket: "package HAL split-out",
        reason_contains: "Cannot find module 'norma:solum'",
    },
    ExpectedTsOutcome {
        path: "ad/async-tempus-dormiet.fab",
        // Y: quarantined 2026-08-06; frontend now admits this split-out row,
        // but the TS host module binding is still absent.
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::SplitOut,
        bucket: "package HAL split-out",
        reason_contains: "Cannot find module 'norma:tempus'",
    },
    ExpectedTsOutcome {
        path: "operatores/numerus-overflow.fab",
        // Y: quarantined 2026-08-06; emitted JS number behavior does not
        // preserve Rust/i64 overflow semantics.
        highest_tier: TsHighestTier::Runnable,
        kind: ExpectedTsKind::BehaviorFailure,
        bucket: "numeric overflow semantics",
        reason_contains: "stdout mismatch",
    },
    ExpectedTsOutcome {
        path: "praefixum/praefixum.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "expression_type_mismatch",
    },
    ExpectedTsOutcome {
        path: "tensor/placement-execution-v1.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "numeric_operands_required",
    },
];
