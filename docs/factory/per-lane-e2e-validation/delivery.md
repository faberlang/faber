# Delivery: Per-Lane E2E Validation — feature-isolated backend lanes + release without the wait

**Goal ref**: `faber/docs/factory/per-lane-e2e-validation/goal.md` (Status: planned — pre-implementation; discovered 2026-08-10, radix 0.81.0 / faber 1.6.0 release postmortem + feature-isolation trial)
**Status**: lowered 2026-08-10 by planner-4 — goal-check **READY** (consumer: delivery). Hand tasking **DEFERRED** by Mind order (second horizon, lower priority): these units are not tasked until the blocking radix set (union-variant-namespace, union-variant-first-class, nullable-narrowing, prefer-string-template, ascii-char-index, intrinsic-locale-translation, member-trivia-attachment) is tasked. This delivery is the durable plan; Mind files Hands at reconcile.
**Repos**: faber (primary; exempla harness, scripta, release docs), radix (facade features + codegen leaf-ownership rows for EL-2/EL-3 only), pharos (EL-5 host only).
**Entry gate**: goal-check READY; trial evidence on record (2026-08-10 feature-isolation trial: `cargo check -p radix --no-default-features --features hir-rust` clean in 2m07s cold; `--features hir-go` 20s warm; `--tests` passes).
**Planning-only**: no product code is written by this lowering; this document is the delivery spec implementing Hands run from. Mind commits at reconcile.

## Phase Intent

Restructure the exempla e2e harness so each backend lane builds and runs in feature isolation, then run the lane grid on a schedule on non-dev infrastructure (pharos) so "is main releasable?" is a standing fact instead of a discovery at release time — and rewrite the release protocol to tag only on a green main, turning local release into bump → commit → tag → push.

Non-goals (from goal doc): GitHub Actions as the correctness gate (distribution endpoint only); per-commit full e2e in the dev cycle; changing the exempla corpus data (`radix/corpus` stays shared); the `faber/corpus` package/format fixtures.

## Normalized Spec

Locked decisions and defaults (folded from the goal's Open Questions; defaults recorded so Hands never choose):

1. **Lane → feature mapping mirrors the faber `[features]` pass-through pattern.** Exempla gains `hir-rust = ["radix/hir-rust"]`, `hir-go = ["radix/hir-go"]`, `hir-ts = ["radix/hir-ts"]`, `hir-swift = ["radix/hir-swift"]`, `mir-sexp = ["radix/mir-sexp"]`, `mir-metal = ["radix/mir-metal"]`, `mir-wasm = ["radix/mir-wasm"]`, `mir-llvm = ["radix/mir-llvm"]`, `mir-fmir = ["radix/mir-fmir"]`; `default = ["full-targets"]` and `full-targets` lists them all — today's full e2e unchanged (additive, never reduced-coverage).
2. **The no-backend build is itself a lane.** Goal Q1 (roundtrip lane mapping): `roundtrip`, `mir` (stepper), and `oracle` use no radix leaf feature — they map to the `--no-default-features` bare exempla build ("frontend + forma + stepper" minimal lane). This also covers the `mir-llvm`/`mir-fmir` gating validation after EL-2. Recorded default; Mind may override at reconcile.
3. **Lane modules are the compile-time gate.** Each backend lane module is `#[cfg(feature = "<lane>")]`-gated in both `src/exempla_e2e/mod.rs` (lib test tree) and `tests/e2e_harness/mod.rs` (integration binary) plus the lane module files; shared helpers (`common`, `types`, `oracle`, `parity`, `paths`, `postprocess`) stay ungated. "Feature absent" never masquerades as a test failure.
4. **Phase-2 gating spans two repos, not one.** Ground truth: `radix-mir-llvm` and `radix-mir-stepper` are unconditional deps of the radix facade; `radix-mir-fmir` is an unconditional dep of **faber** (`faber/Cargo.toml` line 26), with radix's `mir-fmir = []` a no-op. Gating therefore: radix `mir-llvm = ["dep:radix-mir-llvm"]` (and `mir-amd` implies `dep:radix-mir-llvm` + `radix-mir-llvm/mir-amd`); faber `radix-mir-fmir` → `optional = true` + `mir-fmir = ["radix/mir-fmir", "dep:radix-mir-fmir"]` + cfg-gate the faber FMIR consumers. Verified: `radix-mir-stepper/src` does not reference `radix_mir_llvm`, so the goal Q4 concern (llvm unconditional presence load-bearing for the stepper) has no known coupling — EL-2 proves it and reverts+escalates if the facade re-export surface disagrees.
5. **`failable_facts_parts` is gated/moved, not deleted.** Leaf-ownership row: the function moves to the leaf that uses it or is `#[cfg]`-gated to the non-rust backends; a rust-only build must compile without it and without dead-code warnings. (Goal's path `radix/src/codegen/mod.rs` is a drift — actual file: `radix/crates/radix/src/codegen/mod.rs`.)
6. **Per-lane expectation ledgers already partially exist** (`GO_EXPECTED_FAILURES`, `FABER_ROUNDTRIP_EXPECTED_FAILURES`). EL-4 formalizes the pattern across lanes and adds the diff-derived lane-selection helper; it does not re-author existing ledgers.
7. **Grid posture first iteration: Tier 1 report-only** (goal Q2 default). The grid reports standing green/red with receipts; it does not gate merges until the signal is trusted (stop condition 3). Status location default (goal Q3): a workspace file `faber/docs/factory/per-lane-e2e-validation/grid-status.md` written by the pharos runner; Mind may redirect to a Vivi memo at reconcile.
8. **Validation discipline (workspace law, faber side).** Every unit validates exact-crate scoped: `cargo test -p <crate> --lib` (cheap surface) or `./scripta/test` from `faber/` (default stage 1 = hygiene + `cargo test -p faber --lib`). The goal's named per-lane commands (`cargo test -p exempla --no-default-features --features <lane> --test e2e_harness -- --ignored`, `cargo check -p radix --no-default-features --features <lane>`) are exact-crate single-package runs with no forbidden flag (`--workspace`/`--all`/`--all-targets`/`--all-features`) and are the unit done-oracles, run once each, never in a loop. **Forbidden in unit validation**: bare `cargo check/build/test` (no `-p`), `--workspace`, `--all`, `--all-targets`, `--all-features`, `faber/scripta/release-gate`. Full e2e (default features) and `radix/scripta/test --stage 1-3` closeout runs are goal-level and auditor/operator-owned at the gate.
9. **Serialization flag (dispatch).** Every unit builds a radix-* path dep (exempla → radix facade, or edits the facade itself). Mind serializes all faber/radix cargo-touching units against other fleet Hands (shared target-dir lock); EL-5 runs on pharos (non-dev infra, no dev lock).

## Repo-Aware Baseline

Verified 2026-08-10 against faber HEAD `9ddd932` (main) and radix HEAD `8b5f69055`:

- **The one structural gap is confirmed.** `faber/crates/exempla/Cargo.toml`: `radix = { path = "../../../radix/crates/radix" }` with **no** `default-features = false` — any harness build pulls full radix. `faber/Cargo.toml` already passes features through (`hir-rust = ["radix/hir-rust", "dep:faber-hir-rust"]`, …; `default = ["full-targets"]`), the pattern to mirror.
- **Lane modules**: `src/exempla_e2e/mod.rs` (lib test tree, ~30 modules) and `tests/e2e_harness/mod.rs` (integration binary; go/ts/wasm*/rust/rust_canonical/swift/sexp/llvm*/roundtrip/mir/oracle). Lane tests are `#[ignore]`-marked with run filters `exempla_<lane>_e2e` (e.g. `go.rs:194`). Expected-failure tables per lane already exist for go and roundtrip.
- **Radix facade features** (`radix/crates/radix/Cargo.toml`): `hir-*` dep-gated (`dep:radix-…`); `mir-fmir = []` and `mir-llvm = []` are no-ops; `radix-mir-llvm` (line 50) and `radix-mir-stepper` (line 54) unconditional; `mir-amd = ["radix-mir-llvm/mir-amd"]` (line 118). No `cfg(feature = "mir-fmir")` in radix `src/`.
- **Leaf-ownership target**: `radix/crates/radix/src/codegen/mod.rs` contains `failable_facts_parts` (dead in a rust-only build — surfaced mechanically by the trial).
- **Corpus**: shared at `radix/corpus` (per goal non-goal: unchanged by this delivery).
- **Release surface**: `faber/docs/release/worktree-rehearsal-procedure.md` + `v1.6.0-rc.1-sibling-pins.md` exist; `faber/scripta/regen-lock` (+ `regen-lock-test.py`) already exists — the one-command pinned-sibling rehearsal extends this script rather than inventing a new tool. Incident lessons captured in `9ddd932` (docs(release)).
- **Foreign dirt (never touch, Class B)**: faber working tree carries untracked `dist/faber-v1.6.0-*` release tarballs — not delivery files. Before EL-2 is tasked, Mind confirms `faber/src/package/mir/*` has no in-flight FMIR e2e-hardening edits (hand-1 touched `{driver,link,lower}.rs` + `lane_test.rs` during the faber-script-e2e-hardening horizon; the FMIR gate lands on those files).

**Authority order**: live source/tests → goal doc → this delivery spec → sibling delivery conventions (ddpp1, ngab7).

## Stage Graph (unit graph)

```text
EL-1 exempla feature pass-through + cfg gates (rust proof lane)   [Wave A; goal root]
  ├─ EL-2 gate radix-mir-llvm + radix-mir-fmir behind dep:        [Wave B; radix+faber; serialized]
  ├─ EL-3 failable_facts_parts leaf ownership                      [Wave B; radix codegen; serialized]
  └─ EL-4 lane-scoped expectations + diff-derived lane selection   [Wave C]
        └─ EL-5 nightly per-lane grid on pharos                    [Wave C; needs EL-1, wants EL-4]
              └─ EL-6 release protocol rewrite + one-command rehearsal [Wave D; needs EL-5 dry-run]
```

Dependency edges: `EL-1 → EL-2`, `EL-1 → EL-3`, `EL-1 → EL-4`, `EL-4 → EL-5` (recommended — diff-derived selection trims grid cost; EL-5 can ship full-grid without it), `EL-5 → EL-6`. **Waves**: A = EL-1; B = EL-2 + EL-3 (disjoint write scopes, serialized at dispatch on the shared radix build lock — never parallel in one Hand burst); C = EL-4 + EL-5; D = EL-6 (goal gate: grid dry-run must land before the protocol rewrite). No unit's write scope overlaps another's.

## Implementation Work (units)

### EL-1 — Exempla lane feature pass-through + cfg gates (rust proof lane)

| Field | Content |
| --- | --- |
| **outcome** | The exempla crate gains per-target features mirroring faber's radix pass-through; `default = ["full-targets"]` unchanged; every backend lane module is `#[cfg]`-gated on its feature in `src/exempla_e2e/mod.rs` and `tests/e2e_harness/mod.rs` so unfeatured lanes are not compiled; `cargo test -p exempla --no-default-features --features hir-rust --test e2e_harness -- --ignored` builds feature-limited radix and runs only the rust lane green. |
| **write_scope** | `faber/crates/exempla/Cargo.toml` (features + `default-features = false` on the radix dep); `faber/crates/exempla/src/exempla_e2e/mod.rs` + lane module files (cfg attrs); `faber/crates/exempla/tests/e2e_harness/mod.rs`; `faber/docs/factory/per-lane-e2e-validation/ledger.md` (unit row). |
| **read_scope** | `faber/Cargo.toml` `[features]` (pass-through pattern to mirror); `radix/crates/radix/Cargo.toml` `[features]` (target feature names); `radix/corpus` (read-only; unchanged); goal doc §Reference Packet. |
| **done_when** | (a) the per-lane rust command above exits 0 running only the rust lane; (b) `grep -rn 'cfg(feature' src/exempla_e2e/mod.rs tests/e2e_harness/mod.rs` shows every backend lane module gated (go, ts, wasm*, rust, rust_canonical, swift, sexp, llvm*, mir, roundtrip per decision 2); shared helpers ungated; (c) `cargo test -p exempla --lib` (default features) still green — additive, not reduced coverage; (d) `git diff --check` clean. |
| **validation** | the exact per-lane rust command above (one run); `cargo test -p exempla --lib` (default surface); `./scripta/test` from `faber/` (stage 1) at unit closeout; `git diff --check`. No `--all-targets`/`--all-features`/workspace runs. |
| **depends_on** | none (goal root). **Parallel children**: none within the unit (`Cargo.toml` + `mod.rs` coherence). |
| **non_goals** | No gating of radix facade deps (EL-2); no `failable_facts_parts` move (EL-3); no expectation-ledger rework (EL-4); no pharos grid (EL-5); no release-protocol edits (EL-6); no corpus changes. |
| **risk** | medium — cfg-gating breadth (~30 module declarations) makes it easy to miss a backend reference; the stop-condition guard is mechanical: an unfeatured lane must not compile, verifiable by grep + the lane run. |
| **est_work_tokens** | 10k–16k. **est_basis**: `ladder-script-rewrite` (ledger 1015s / 136 calls — closest analog for broad cross-file wiring across a harness). |
| **tool_latency** | medium (feature-limited exempla build + one lane e2e run, a few minutes). |
| **dispatch note** | builds radix as a path dep → Mind serializes against fleet Hands on the shared cargo lock; no `--workspace`. |

### EL-2 — Gate `radix-mir-llvm` + `radix-mir-fmir` behind `dep:` features (real isolation)

| Field | Content |
| --- | --- |
| **outcome** | A rust-only build compiles neither `radix-mir-llvm` nor `radix-mir-fmir`: radix `mir-llvm = ["dep:radix-mir-llvm"]` (and `mir-amd` implies `dep:radix-mir-llvm` + `radix-mir-llvm/mir-amd`); faber `radix-mir-fmir` becomes `optional = true` behind `mir-fmir = ["radix/mir-fmir", "dep:radix-mir-fmir"]` with the faber FMIR consumers cfg-gated. `full-targets` builds (radix and faber) are unchanged and green; stepper behavior unaffected. |
| **write_scope** | `radix/crates/radix/Cargo.toml` (features + deps); `faber/Cargo.toml` (`radix-mir-fmir` optional + feature wiring); `faber/src/**` FMIR consumer cfg-gates (`src/package/mir/*` route/selection surfaces per landed state — hunk-disjoint from any in-flight FMIR work, confirmed at dispatch); `faber/docs/factory/per-lane-e2e-validation/ledger.md`. |
| **read_scope** | `radix/crates/radix/src/` (facade re-export surface: does shared code reference llvm/fmir types?); `radix/crates/radix-mir-stepper/` (coupling proof — verified no `radix_mir_llvm` references today); `faber/src/package/mir/` (FMIR consumers). |
| **done_when** | (a) `cargo check -p radix --no-default-features --features hir-rust` exits 0 and the build graph contains no `radix-mir-llvm`/`radix-mir-fmir` compile (grep `cargo tree -p radix --no-default-features --features hir-rust` or equivalent); (b) `cargo check -p faber --no-default-features --features hir-rust` exits 0 with no `radix-mir-fmir` in the graph; (c) `cargo test -p radix --lib` (shared behavior incl. stepper) green; (d) default/full-targets builds unchanged — `cargo test -p exempla --lib` from faber green; (e) `git diff --check` clean. |
| **validation** | the two exact-crate feature-limited commands above (one run each); `cargo test -p radix --lib`; `cargo test -p exempla --lib`; `./scripta/test` from `faber/` (stage 1) at closeout. If the facade re-export surface breaks (goal Q4 answered "load-bearing"), revert the gate and escalate with the offending reference — never force the gate through. |
| **depends_on** | EL-1 (isolation is the prerequisite that makes the gate provable). **Parallel children**: none within the unit (Cargo.toml coherence); may sit beside EL-3 (disjoint files) but dispatch-serialized on the shared radix lock. |
| **non_goals** | No gating of `radix-mir-stepper` or `radix-mir` (the goal names only llvm + fmir leaves); no `failable_facts_parts` work (EL-3); no change to `full-targets` membership; no runtime-unreachable-path claims — exclusion is compile-level. |
| **risk** | medium — if the stepper or any unconditional facade code consumes llvm/fmir types through a non-obvious path, gating breaks; done_when (c) + revert-escalate rule contain it. Dispatch hazard: FMIR consumer files were hand-1 WIP during the faber-script-e2e-hardening horizon — Mind confirms clean before tasking. |
| **est_work_tokens** | 6k–10k. **est_basis**: `crate-dep-swap` (ledger 483s / 43 calls — dep + feature wiring with verification, scaled up for the two-repo span). |
| **tool_latency** | medium (feature-limited `cargo check -p radix` cold ~2m; warm 20s). |
| **dispatch note** | edits radix facade + faber → Mind serializes; both are hot shared crates. |

### EL-3 — `failable_facts_parts` leaf ownership (dead-code in rust-only build)

| Field | Content |
| --- | --- |
| **outcome** | `failable_facts_parts` (dead in a rust-only build) is moved to the leaf that uses it or `#[cfg]`-gated to the non-rust backends; `cargo check -p radix --no-default-features --features hir-rust` compiles clean with no dead-code finding for it; shared behavior preserved. |
| **write_scope** | `radix/crates/radix/src/codegen/mod.rs` (+ codegen leaf modules only if the move target lives there); focused tests for the moved behavior; `faber/docs/factory/per-lane-e2e-validation/ledger.md`. |
| **read_scope** | `radix/crates/radix/src/` (callers of `failable_facts_parts` across backends — establish the leaf owner before moving). |
| **done_when** | (a) `cargo check -p radix --no-default-features --features hir-rust` shows no dead-code warning for `failable_facts_parts`; (b) `cargo test -p radix --lib` green (shared behavior); (c) `cargo check -p radix` (full-targets) green — no behavior change in backend builds; (d) `git diff --check` clean. |
| **validation** | the two exact-crate radix commands above (one run each); `grep -n 'failable_facts_parts' radix/crates/radix/src/` to show the leaf-owned single site (or cfg-gate); `./scripta/test` from `faber/` at closeout. |
| **depends_on** | EL-1 (the rust-only build is what surfaces the dead code mechanically). **Parallel children**: none within the unit; dispatch-serialized beside EL-2 on the shared radix lock. |
| **non_goals** | No behavior change to error-fact formatting/semantics; no broader codegen refactor; no touching `radix/corpus`. |
| **risk** | low-medium — function move with multiple backend callers; caller-grep before move and shared tests contain it. |
| **est_work_tokens** | 3k–6k. **est_basis**: `crate-dep-swap` (ledger 483s / 43 calls — impl + small test surface). |
| **tool_latency** | low-medium (two exact-crate radix checks). |
| **dispatch note** | edits radix codegen → Mind serializes. |

### EL-4 — Lane-scoped exemplar expectations + diff-derived lane selection

| Field | Content |
| --- | --- |
| **outcome** | Every lane's expected-failure ledger is scoped to that lane (formalizing the existing `GO_EXPECTED_FAILURES`/`FABER_ROUNDTRIP_EXPECTED_FAILURES` pattern across go/ts/wasm/rust/swift/sexp/llvm/mir/roundtrip); a lane-selection helper maps a changed-crate path set to the required lanes (a packet touching `radix-hir-go` gates on the go lane only); no lane absorbs another lane's expected failure. |
| **write_scope** | `faber/crates/exempla/src/exempla_e2e/**` (per-lane expectation tables + the lane-selection helper, e.g. `lane_selection.rs`); `radix/corpus` **read-only**; `faber/docs/factory/per-lane-e2e-validation/ledger.md`. |
| **read_scope** | `radix/crates/*/Cargo.toml` (crate-name → lane mapping for diff derivation); `faber/crates/exempla/tests/e2e_harness/` (existing lane test filters). |
| **done_when** | (a) each backend lane owns its expected-failure table and the rust lane's table contains exactly the rust lane's rows; (b) the lane-selection helper has unit tests proving: `radix-hir-go` change → {go}, `radix-mir-llvm` change → {llvm, metal if metal rides llvm}, exempla-only change → {all}; (c) dry-run of selection on a sample diff prints the expected lane set; (d) `cargo test -p exempla --lib` green; (e) `git diff --check` clean. |
| **validation** | `cargo test -p exempla --lib` (helper + expectation tests); selection dry-run on a synthetic diff; grep that no lane table references another lane's exemplar set. No lane e2e run required in-loop (kept for closeout). |
| **depends_on** | EL-1 (cfg-gated lanes must exist). **Parallel children**: none (single-crate coherence). |
| **non_goals** | No re-authoring of existing expected-failure rows beyond moving them to per-lane ownership; no grid infrastructure (EL-5); no release-protocol edits (EL-6); no corpus data changes. |
| **risk** | medium — expectation ratchets can hide real failures (goal stop conditions 1/3); done_when (a)/(c) and the grep guard keep coverage honest. |
| **est_work_tokens** | 6k–10k. **est_basis**: `ladder-script-rewrite` (ledger 1015s / 136 calls — cross-file restructure + new helper). |
| **tool_latency** | low (lib tests + dry-runs). |
| **dispatch note** | `cargo test -p exempla --lib` builds radix → Mind serializes. |

### EL-5 — Nightly per-lane lane grid on pharos

| Field | Content |
| --- | --- |
| **outcome** | A scheduled grid on pharos runs each lane against `origin/main` HEAD using the EL-1 per-lane commands, writes a per-lane green/red receipt and a standing status file (default `faber/docs/factory/per-lane-e2e-validation/grid-status.md`, decision 7); a lane-scoped bisect script localizes a red lane to its commit in ~7 lane-only runs. |
| **write_scope** | new grid runner + bisect scripts (under `faber/scripta/lane-grid*` or a named subdir of `faber/docs/factory/per-lane-e2e-validation/`); pharos host config (cron/systemd unit + runner checkout + status writer); the standing status file; `faber/docs/factory/per-lane-e2e-validation/ledger.md`. |
| **read_scope** | `faber/scripta/test`, `regen-lock` (scripta conventions to reuse); `radix/scripta/test` (ladder stages referenced by the runner's receipt format); pharos `/etc` service config patterns. |
| **done_when** | (a) grid dry-run on pharos runs every lane against a known-commit HEAD and produces one green/red receipt per lane; (b) the standing status file is written and readable with receipts; (c) bisect script localizes a deliberately-injected break (e.g. a stale expected-failure row) to the right commit in ≤7 lane-only runs; (d) no dev-machine cargo lock is touched by the grid (runs on pharos only); (e) runner scripts pass `python3 -m py_compile` / shellcheck-style hygiene and the scripta test convention. |
| **validation** | the pharos dry-run (single run, non-dev infra — no Cargo discipline impact on the dev machine); script hygiene checks; `./scripta/test` from `faber/` for any scripta-adjacent unit tests. The dry-run is auditor-visible evidence for the EL-6 gate. |
| **depends_on** | EL-1 (required — per-lane commands must exist); EL-4 (recommended — diff-derived selection trims grid cost; full-grid mode ships without it). **Parallel children**: runner-script authoring vs pharos host config are sequential (host config consumes the runner); no parallel split. |
| **non_goals** | No merge-gating (decision 7 — Tier 1 report-only first iteration); no per-commit triggering; no GitHub Actions correctness gate; no change to `radix/corpus`. |
| **risk** | medium — signal trust (a false green is worse than no signal, goal stop condition 3); pharos access/config drift. Contains: dry-run receipts + the standing status file, and the EL-6 gate requires the dry-run evidence. |
| **est_work_tokens** | 10k–16k. **est_basis**: `pilot` (no ledger class fits pharos-infra + runner scripting). |
| **tool_latency** | high (pharos dry-run grid = the full lane matrix, minutes per lane on non-dev infra). |
| **dispatch note** | runs on pharos (non-dev infra) — no shared-cargo-lock contention; Mind assigns a Hand with `sysadmin`/`pharos` grounding. |

### EL-6 — Release protocol rewrite + one-command pinned-sibling rehearsal

| Field | Content |
| --- | --- |
| **outcome** | The release protocol says: release only on a green main (grid or CI re-run on the exact commit); local release = bump → commit → tag → push; the pinned-sibling lock rehearsal is one command (extends `faber/scripta/regen-lock` per `worktree-rehearsal-procedure.md` + `v1.6.0-rc.1-sibling-pins.md`). |
| **write_scope** | `faber/docs/release/release-runbook.md` + `faber/docs/release/worktree-rehearsal-procedure.md` (protocol rewrite); `faber/scripta/regen-lock` (+ `regen-lock-test.py`) for the one-command rehearsal; `faber/AGENTS.md` + `radix/AGENTS.md` release-protocol sections (tag-only-on-green wording); `faber/docs/factory/per-lane-e2e-validation/ledger.md`. |
| **read_scope** | `faber/docs/release/*` (existing runbook, rehearsal procedure, sibling-pins notes, policy); `radix/AGENTS.md` + `faber/AGENTS.md` (current release protocols); goal doc §Release Posture. |
| **done_when** | (a) release-runbook states: tag only on a main tip the lane grid (or CI re-run on the exact commit) declared green; local release = bump → commit → tag → push; (b) the pinned-sibling rehearsal is one command, documented in the runbook, with a scripta test (extend `regen-lock-test.py`); (c) `faber/AGENTS.md` + `radix/AGENTS.md` protocol sections updated to match; (d) `./scripta/test` from `faber/` green (includes the scripta tests); (e) `git diff --check` clean. |
| **validation** | `./scripta/test` from `faber/` (stage 1 + scripta unit tests — `regen-lock-test.py`); the rehearsal command dry-run on a scratch worktree (never a real release; `release-gate` remains **forbidden**); grep the runbook for the tag-only-on-green sentence. EL-5 dry-run evidence is the gate input, not a validation command here. |
| **depends_on** | EL-5 (goal validation: "Nightly-grid dry run on pharos before the protocol rewrite lands"). **Parallel children**: none (doc + script coherence). |
| **non_goals** | No actual release execution; no `release-gate` run (forbidden); no version bumps; no change to the lane-grid mechanics (EL-5). |
| **risk** | low-medium — doc+script only; the rehearsal one-command must not mutate the dev checkout's lock (worktree-scoped, tested). |
| **est_work_tokens** | 6k–10k. **est_basis**: `ladder-script-rewrite` (ledger 1015s / 136 calls — cross-file docs + scripta rewrite analog). |
| **tool_latency** | low (doc + scripta tests). |
| **dispatch note** | `regen-lock` touches Cargo.lock on a worktree → short cargo lock; Mind serializes. |

## Checkpoints And Gates

- **Wave A gate (EL-1)**: rust lane runs isolated and green; default e2e unchanged (additive proof). Next: Mind may parallel-file EL-2/EL-3/EL-4.
- **Wave B gate (EL-2 + EL-3)**: rust-only radix build compiles neither llvm/fmir leaves nor dead `failable_facts_parts`; full-targets green; stepper unaffected. Radix write units are the only ones with a second planner's repo in scope — Mind confirms no overlap with the blocking radix set before tasking (the blocking goals are frontend/HIR-surface features; these rows are facade features + codegen cleanup — disjoint, but verify at dispatch).
- **Wave C gate (EL-4 + EL-5)**: per-lane expectations honest (no cross-lane absorption); pharos dry-run receipts exist and the status file is trustworthy (stop condition 3 is a hard gate).
- **Wave D gate (EL-6)**: protocol docs updated; one-command rehearsal tested; tag-only-on-green in `AGENTS.md`. **Goal closeout (auditor/operator-owned, not unit validation)**: `radix/scripta/test --stage 1-3` once, and the full e2e (`cargo test -p exempla` default features) once — exactly one run each per Cargo discipline. Release posture: **defer-release** — this goal makes future releases fast; it is not itself a release surface (goal §Release Posture).

## Validation Summary

| Unit | Primary validation (exact-crate only) | Latency |
| --- | --- | --- |
| EL-1 | `cargo test -p exempla --no-default-features --features hir-rust --test e2e_harness -- --ignored` (one run) + `cargo test -p exempla --lib` + `./scripta/test` | medium |
| EL-2 | `cargo check -p radix --no-default-features --features hir-rust` + `cargo check -p faber --no-default-features --features hir-rust` (one run each) + `cargo test -p radix --lib` + `cargo test -p exempla --lib` | medium |
| EL-3 | `cargo check -p radix --no-default-features --features hir-rust` (no dead `failable_facts_parts`) + `cargo test -p radix --lib` | low-medium |
| EL-4 | `cargo test -p exempla --lib` + lane-selection dry-run | low |
| EL-5 | pharos grid dry-run (per-lane commands, non-dev infra) + script hygiene | high |
| EL-6 | `./scripta/test` from `faber/` + rehearsal dry-run on scratch worktree | low |

All units: `git diff --check` clean; no bare/`--workspace`/`--all`/`--all-targets`/`--all-features`/`release-gate` invocations; no verification loops (one run per declared validation).

## Open Questions For Mind

1. **Q1 default recorded**: roundtrip/mir/oracle lanes map to the no-backend minimal build (decision 2). Confirm at reconcile — this also determines whether `--no-default-features` exempla builds are a supported lane shape or only an internal validation path.
2. **Q2 default recorded**: grid is Tier 1 report-only first iteration (decision 7). Confirm before EL-5 lands; the goal's stop condition 3 gates any Tier 0 escalation.
3. **Q3 default recorded**: status file at `faber/docs/factory/per-lane-e2e-validation/grid-status.md`. Redirect to Vivi memo if preferred at reconcile.
4. **Q4**: mir-llvm unconditional presence — no `radix_mir_llvm` reference found in `radix-mir-stepper`; EL-2 proves the facade re-export surface and reverts+escalates on any load-bearing reference. Flagged as EL-2's named risk, not a blocking decision.
5. **Dispatch coordination (new)**: EL-2 lands on `faber/src/package/mir/*` FMIR consumers — Mind confirms no in-flight FMIR e2e-hardening edits before tasking (hand-1 horizon overlap).
6. **Priority posture**: whole delivery is lower-priority/second horizon; Mind keeps all units untasked until the blocking radix set is tasked. No unit in this spec races a radix planner's write scope (facade features + codegen cleanup vs frontend/HIR-surface features — disjoint, but verify at each dispatch).
7. **Grid gating cadence**: EL-5's first iteration is nightly per goal; confirm the cadence at reconcile (nightly vs per-push).
