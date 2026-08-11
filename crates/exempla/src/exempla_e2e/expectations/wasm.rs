//! Wasm lane expected-outcome tables (per-exemplum expected tier floors + aggregate tier floors, moved verbatim under per-lane ownership).
//!
//! Wasm-lane expected-outcome surface (per-lane-e2e-validation EL-4). The
//! lane harness module consumes only this table via
//! `super::expectations::wasm::…`; no other lane may absorb these rows.

/// Wasm exempla e2e tiers aligned with the Rust-parity contract (A–D).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum WasmTier {
    SourceReadable,
    FrontendAnalyzed,
    MirLowered,
    /// Tier A — Wasm bytes emitted in-tree.
    WasmEmitted,
    /// Tier B — external `wasm-tools validate` accepts the module.
    CompileValid,
    /// Tier C — external stub host runs `incipit` without trap.
    Runnable,
    /// Tier D — captured output matches sibling `*.expected` when present.
    OutputChecked,
}

// Aggregate tier floors (minimum ratchets; U6-F raised tiers A–D by the
// measured delta of the cursor-stream promotion: cede/cede.fab +
// cursor/cursor.fab reach output-checked through the product-runner boost,
// +2 on tiers A/B/C/D. Frontend/MIR are unchanged — the rows already counted
// there; no floor rises by reclassification without its focused proof).
pub(crate) const EXPECTED_FRONTEND_ANALYZED_FLOOR: usize = 210;
pub(crate) const EXPECTED_MIR_LOWERED_FLOOR: usize = 194;
pub(crate) const EXPECTED_WASM_TIER_A_EMITTED_FLOOR: usize = 186;
pub(crate) const EXPECTED_WASM_TIER_B_COMPILE_VALID_FLOOR: usize = 182;
pub(crate) const EXPECTED_WASM_TIER_C_RUNNABLE_FLOOR: usize = 159;
pub(crate) const EXPECTED_WASM_TIER_D_OUTPUT_CHECKED_FLOOR: usize = 14;

pub(crate) const WASM_EXPECTED_TIER_FLOORS: &[(&str, WasmTier)] = &[
    ("abstractus/abstractus.fab", WasmTier::CompileValid),
    ("ad/sermo-conversio.fab", WasmTier::FrontendAnalyzed),
    ("ad/sermo-live-directional.fab", WasmTier::FrontendAnalyzed),
    ("ad/sermo-tuus.fab", WasmTier::FrontendAnalyzed),
    ("ad/solum-lege-generic.fab", WasmTier::SourceReadable),
    ("adfirma/adfirma.fab", WasmTier::CompileValid),
    ("adfirma/in-functione.fab", WasmTier::CompileValid),
    (
        "annotation-sugar/radix-lane-braced.fab",
        WasmTier::SourceReadable,
    ),
    ("ante/ante.fab", WasmTier::CompileValid),
    ("assertio/nonnulla.fab", WasmTier::Runnable),
    ("assignatio/assignatio.fab", WasmTier::CompileValid),
    ("aut/aut.fab", WasmTier::CompileValid),
    ("binarius/binarius.fab", WasmTier::CompileValid),
    // Y: promoted U6-F — cursor-stream materialization landed end-to-end
    // (U6-A emitter on the closed-set v1 row __faber_rt_v1_cursor_stream;
    // U6-B product host binds materialization + the cede yield channel). The
    // e2e lane's product-runner boost runs the emitted module and matches the
    // sibling .expected oracle ([1, 2]). Recipe: fixture
    // radix/corpus/cede/cede.fab -> e2e wasm lane -> OutputChecked (product
    // boost, stdout match) -> source radix-mir-wasm/src/calls.rs +
    // hosts/wasm/src/imports.rs -> done oracle [1, 2].
    ("cede/cede.fab", WasmTier::OutputChecked),
    ("ceteri/ceteri.fab", WasmTier::CompileValid),
    ("clausa/clausa.fab", WasmTier::Runnable),
    ("clausura/clausura.fab", WasmTier::Runnable),
    ("cli/cli.fab", WasmTier::FrontendAnalyzed),
    ("conversio/conversio.fab", WasmTier::MirLowered),
    ("conversio/octeti.fab", WasmTier::FrontendAnalyzed),
    // Y: promoted U6-F — cursor-stream materialization landed end-to-end
    // (U6-A emitter on the closed-set v1 row __faber_rt_v1_cursor_stream;
    // U6-B product host binds materialization + the cede yield channel). The
    // e2e lane's product-runner boost runs the emitted module and matches the
    // sibling .expected oracle ([1, 2]). Recipe: fixture
    // radix/corpus/cursor/cursor.fab -> e2e wasm lane -> OutputChecked
    // (product boost, stdout match) -> source radix-mir-wasm/src/calls.rs +
    // hosts/wasm/src/imports.rs -> done oracle [1, 2].
    ("cursor/cursor.fab", WasmTier::OutputChecked),
    ("cura/cura.fab", WasmTier::FrontendAnalyzed),
    ("cura/nidificatus.fab", WasmTier::FrontendAnalyzed),
    ("custodi/custodi.fab", WasmTier::CompileValid),
    ("destructura/lista.fab", WasmTier::CompileValid),
    ("destructura/literal.fab", WasmTier::FrontendAnalyzed),
    ("directiva/directiva.fab", WasmTier::Runnable),
    ("discretio/discretio.fab", WasmTier::CompileValid),
    ("dum/conditio-complexa.fab", WasmTier::CompileValid),
    ("dum/dum.fab", WasmTier::CompileValid),
    ("dum/in-functione.fab", WasmTier::CompileValid),
    ("ego/ego.fab", WasmTier::CompileValid),
    ("elige/ceterum.fab", WasmTier::CompileValid),
    ("elige/elige.fab", WasmTier::CompileValid),
    ("elige/ergo-redde.fab", WasmTier::CompileValid),
    ("elige/in-functione.fab", WasmTier::CompileValid),
    ("est/est.fab", WasmTier::MirLowered),
    ("et/et.fab", WasmTier::CompileValid),
    ("fac/fac-cape.fab", WasmTier::FrontendAnalyzed),
    ("fac/fac-dum.fab", WasmTier::FrontendAnalyzed),
    ("finge/finge.fab", WasmTier::CompileValid),
    ("fixum/fixum.fab", WasmTier::CompileValid),
    ("functio/functio.fab", WasmTier::CompileValid),
    ("functio/in-ex.fab", WasmTier::Runnable),
    ("functio/recursio.fab", WasmTier::CompileValid),
    ("functio/sponte-vel.fab", WasmTier::Runnable),
    ("functio/typi-parametri.fab", WasmTier::CompileValid),
    ("futura/futura.fab", WasmTier::Runnable),
    ("generic/generic.fab", WasmTier::FrontendAnalyzed),
    ("generic/genus.fab", WasmTier::FrontendAnalyzed),
    ("genus/creo.fab", WasmTier::CompileValid),
    ("genus/genus.fab", WasmTier::CompileValid),
    ("genus/literal.fab", WasmTier::CompileValid),
    ("genus/methodi.fab", WasmTier::CompileValid),
    (
        "gpu-core-types/atomic-element-reject.fab",
        WasmTier::SourceReadable,
    ),
    (
        "gpu-core-types/f16-bf16-reject.fab",
        WasmTier::SourceReadable,
    ),
    (
        "gpu-core-types/matrix-tensor-reject.fab",
        WasmTier::SourceReadable,
    ),
    ("iace/functio-fallibilis.fab", WasmTier::MirLowered),
    ("iace/iace.fab", WasmTier::Runnable),
    // Y: promoted U6-F — standalone-runnable package helper (measured
    // output-checked in the wasm-host-parity ledger; the product host W11/W12
    // text surface renders `Salve, auxilium!`). The package role
    // (importa:auxilium:saluta) is Faber-owned linking; the package link+run
    // proof lands on the carrier-typed importa-wasm fixture (U6-D/E).
    ("importa/auxilium.fab", WasmTier::OutputChecked),
    // Y: reconciled U6-F — the single-module probe keeps its fail-closed D-PA4
    // diagnostic (mir_wasm_rejected_provider_capability, `auxilium:saluta`)
    // for same-package cross-module identities; the package-aware lane landed
    // (U6-C emit, U6-D product path, U6-E host resolution) and proves the
    // package link+run on the carrier-typed importa-wasm fixture. Floor stays
    // at the measured MirLowered tier of this probe lane.
    ("importa/importa.fab", WasmTier::MirLowered),
    ("implet/implet.fab", WasmTier::CompileValid),
    ("incipiet/incipiet.fab", WasmTier::Runnable),
    ("incipit/functionibus.fab", WasmTier::CompileValid),
    ("incipit/incipit.fab", WasmTier::CompileValid),
    ("incipit/salve-munde.fab", WasmTier::CompileValid),
    ("instans/instans.fab", WasmTier::SourceReadable),
    ("integratio/destructura-sparsa.fab", WasmTier::Runnable),
    ("integratio/minimum-smoke.fab", WasmTier::Runnable),
    ("intrinseca/copia-algebra.fab", WasmTier::Runnable),
    ("intrinseca/copia-fundamenta.fab", WasmTier::Runnable),
    ("intrinseca/fractus-comparatio.fab", WasmTier::Runnable),
    ("intrinseca/fractus-rotundatio.fab", WasmTier::Runnable),
    ("intrinseca/numerus-methodi.fab", WasmTier::Runnable),
    ("intrinseca/textus-quaestiones.fab", WasmTier::Runnable),
    ("intrinseca/textus-transformationes.fab", WasmTier::Runnable),
    ("intrinseca/vacua-ascribere.fab", WasmTier::FrontendAnalyzed),
    ("itera/de.fab", WasmTier::Runnable),
    ("itera/ex.fab", WasmTier::CompileValid),
    ("itera/in-functione.fab", WasmTier::CompileValid),
    ("itera/intervallum-gradus.fab", WasmTier::CompileValid),
    ("itera/intervallum.fab", WasmTier::CompileValid),
    ("itera/nidificatus.fab", WasmTier::CompileValid),
    ("lege/lege.fab", WasmTier::SourceReadable),
    ("lista/lista.fab", WasmTier::CompileValid),
    ("literalia/regex.fab", WasmTier::Runnable),
    ("membrum/membrum.fab", WasmTier::WasmEmitted),
    ("mone/mone.fab", WasmTier::CompileValid),
    ("mori/mori.fab", WasmTier::CompileValid),
    ("morphologia/morphologia.fab", WasmTier::FrontendAnalyzed),
    ("nexum/nexum.fab", WasmTier::CompileValid),
    ("nonnulla/nonnulla.fab", WasmTier::Runnable),
    ("nota/gradus.fab", WasmTier::CompileValid),
    ("nota/nota.fab", WasmTier::CompileValid),
    ("octet/octet.fab", WasmTier::MirLowered),
    ("octeti/octeti.fab", WasmTier::OutputChecked),
    ("octeti/unify.fab", WasmTier::MirLowered),
    ("operatores/nonnull-chain.fab", WasmTier::Runnable),
    ("operatores/optional-chain.fab", WasmTier::Runnable),
    ("optio/optio.fab", WasmTier::FrontendAnalyzed),
    ("implendum/implendum.fab", WasmTier::CompileValid),
    ("per/per.fab", WasmTier::CompileValid),
    ("perge/perge.fab", WasmTier::CompileValid),
    // Y: quarantined 2026-08-06; fixture regressed to frontend diagnostics before Wasm lowering.
    ("praefixum/praefixum.fab", WasmTier::SourceReadable),
    ("privata/privata.fab", WasmTier::CompileValid),
    ("probandum/probandum.fab", WasmTier::CompileValid),
    ("promissum/promissum.fab", WasmTier::Runnable),
    ("protecta/protecta.fab", WasmTier::SourceReadable),
    ("publica/publica.fab", WasmTier::CompileValid),
    ("redde/redde.fab", WasmTier::CompileValid),
    ("rumpe/fac-dum-rumpe.fab", WasmTier::Runnable),
    ("rumpe/fac-si-rumpe.fab", WasmTier::Runnable),
    ("rumpe/rumpe-top-level-error.fab", WasmTier::SourceReadable),
    ("rumpe/rumpe.fab", WasmTier::CompileValid),
    ("scriptum/scriptum.fab", WasmTier::CompileValid),
    ("si/ergo.fab", WasmTier::CompileValid),
    ("si/nidificatus.fab", WasmTier::CompileValid),
    ("si/secus.fab", WasmTier::CompileValid),
    ("si/si.fab", WasmTier::CompileValid),
    ("si/sin.fab", WasmTier::CompileValid),
    ("sparge/sparge.fab", WasmTier::Runnable),
    ("sparsa/conversio-reject.fab", WasmTier::SourceReadable),
    ("sparsa/non-numeric-reject.fab", WasmTier::SourceReadable),
    ("stdlib-nativum/chorda.fab", WasmTier::SourceReadable),
    ("stdlib-nativum/retorta.fab", WasmTier::SourceReadable),
    ("sub/sub.fab", WasmTier::CompileValid),
    ("tabula/methodi-accessus.fab", WasmTier::FrontendAnalyzed),
    ("tabula/tabula.fab", WasmTier::FrontendAnalyzed),
    ("tacet/tacet.fab", WasmTier::FrontendAnalyzed),
    ("tensor/arithmetic-reject.fab", WasmTier::SourceReadable),
    // Y: quarantined 2026-08-06; fixture regressed to frontend diagnostics before Wasm lowering.
    (
        "tensor/placement-execution-v1.fab",
        WasmTier::SourceReadable,
    ),
    ("typi/sized-family-error.fab", WasmTier::SourceReadable),
    ("typus/typus.fab", WasmTier::CompileValid),
    ("unarius/unarius.fab", WasmTier::CompileValid),
    ("unio/unio.fab", WasmTier::Runnable),
    ("usque/usque.fab", WasmTier::CompileValid),
    ("varia/typi-ligata.fab", WasmTier::CompileValid),
    ("varia/varia.fab", WasmTier::CompileValid),
    ("vel/vel.fab", WasmTier::Runnable),
    ("vide/vide.fab", WasmTier::CompileValid),
];
