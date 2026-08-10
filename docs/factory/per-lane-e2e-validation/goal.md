# Per-Lane E2E Validation — Feature-Isolated Backend Lanes + Release Without the Wait

**Status**: planned — pre-implementation; discovered 2026-08-10 (radix 0.81.0 / faber 1.6.0 release postmortem + feature-isolation trial)
**Created**: 2026-08-10
**Target repo**: `/Users/ianzepp/work/faberlang/faber`
**Factory artifact dir**: `docs/factory/per-lane-e2e-validation/`

## Summary

Restructure the exempla e2e harness so each backend lane builds and runs in
feature isolation (`cargo test -p exempla --no-default-features --features
hir-go` builds go-only radix and runs only the go lane), then run the lane
grid on a schedule on non-dev infrastructure (pharos) so "is main releasable?"
is a standing fact instead of a discovery at release time. The release then
becomes bump → commit → tag → push on a green main — minutes of local work,
not a day of verification on the developer's machine.

## Problem

- Releases happen ~daily at current velocity. The protocol mandates local
  verification (ladder, e2e, release-gate) before tagging, so every release
  parks the developer's machine for the duration of the slowest runs and
  contends with their dev builds for the shared cargo target dir.
- The e2e harnesses are deliberately excluded from the dev cycle because they
  are slow — so nothing runs them between releases. Code can drift red for
  days, and the first time the slow gates run is at tag time. This release
  surfaced exactly that: latent `↤` codegen bugs (swift/rust/go/ts), a forma
  round-trip non-idempotency, a stale factory README gate, and a
  pinned-sibling lockfile mismatch — all discovered post-commit / post-tag.
- GitHub Actions has become increasingly unreliable under agent-automation
  load; correctness validation should not depend on it. GitHub stays the
  distribution endpoint only.
- The pinned-sibling lockfile trap: `cargo update` locally resolves against
  local sibling checkouts; CI builds against the pinned siblings. When they
  differ, the `--locked` build fails in CI even though the local build is
  green. The rehearsal procedure exists but is manual and only run under
  release pressure.

## Goals

- The exempla crate gains a per-target feature pass-through to radix
  (`hir-go = ["radix/hir-go"]`, …), default = `full-targets` (today's full
  e2e unchanged), and each lane module is `#[cfg]`-gated on its feature so
  unfeatured lanes are not compiled.
- `cargo test -p exempla --no-default-features --features <lane> --test
  e2e_harness -- --ignored` builds a feature-limited radix and runs only that
  lane — a few minutes per lane instead of the full grid.
- A scheduled per-lane validation grid runs against `origin/main` on non-dev
  infrastructure (pharos) and reports a standing green/red for main.
- Late-found breakage is cheap to localize: a red lane names its commit via
  lane-scoped bisect.
- The release protocol becomes: release only on a green main; local release =
  bump → regen lock against pinned siblings (scripted) → commit → tag → push.

## Non-goals

- Making GitHub Actions the correctness gate (distribution endpoint only).
- Per-commit full e2e in the dev cycle (the e2e stays out of the inner loop;
  the lane grid replaces release-time discovery).
- Changing the exempla corpus data (`radix/corpus` stays shared across lanes).
- The `faber/corpus` package/format fixtures (unaffected by lane isolation).

## Ground Truth Researched

- Trial (2026-08-10): `cargo check -p radix --no-default-features --features
  hir-rust` passes clean in 2m07s cold (~17 crates vs full); `--features
  hir-go` passes (20s warm); `--tests` (radix crate test targets) passes —
  shared-crate tests do not reach into unfeatured backends.
- faber already depends on radix with `default-features = false` and passes
  target features through (`hir-rust = ["radix/hir-rust", "dep:faber-hir-rust"]`,
  faber/Cargo.toml). The pass-through pattern exists.
- The e2e harness (`faber/crates/exempla`) depends on radix WITHOUT
  `default-features = false` — any harness build pulls full radix. This is the
  one structural gap for per-lane builds.
- `mir-llvm` and `mir-fmir` features are empty no-ops: `radix-mir-llvm` and
  `radix-mir-stepper` are unconditional deps of the radix facade. Small
  builds still compile LLVM + FMIR + stepper + air.
- "Good breakage" found by the trial: `failable_facts_parts` in
  `radix/src/codegen/mod.rs` is dead code in a rust-only build — shared code
  only used by non-rust backends (leaf-ownership violation surfaced
  mechanically).
- Release incidents this cycle (evidence for the problem): doctest pass made
  stage 4 take 42 min (disabled across the ladder); `conversio-assign.fab`
  revealed latent codegen bugs on go/ts/rust/swift; the committed fixture
  deterministically fails the forma round-trip lane (now ledgered);
  `docs(factory)` goals landed after the last green run and the stale README
  gate failed CI post-tag; faber `--locked` build failed in CI because the
  local lock resolved against local faber-runtime `6d42d8d` ≠ pinned
  `10d48ea`.

## Reference Packet

Before editing, inspect:

- `faber/crates/exempla/Cargo.toml` + `tests/e2e_harness.rs` +
  `src/exempla_e2e/mod.rs`: the harness feature pass-through and cfg-gate
  points.
- `faber/Cargo.toml` `[features]`: the existing radix pass-through pattern to
  mirror.
- `radix/crates/radix/Cargo.toml` `[features]` + `[dependencies]`: the
  per-target gates and the unconditional `radix-mir-llvm` /
  `radix-mir-stepper` deps.
- `radix/src/codegen/mod.rs` (`failable_facts_parts`): the leaf-ownership
  cleanup target.
- `faber/docs/release/worktree-rehearsal-procedure.md` +
  `v1.6.0-rc.1-sibling-pins.md`: the pinned-sibling lock rehearsal to script.
- `radix/scripta/test` (ladder) + `faber/AGENTS.md` / `radix/AGENTS.md`
  (release protocols): the protocol rewrite targets.

## Constraints And Invariants

- The full e2e (`default` features on the exempla crate) must stay green —
  lane isolation is additive, never a reduced-coverage build.
- Lane modules are the compile-time gate: an unfeatured lane must not compile,
  so "feature absent" never masquerades as a test failure.
- The corpus data stays shared; lanes differ in backend, not exemplars.
- A release tag points only at a main tip the lane grid (or a CI re-run on
  the exact commit) declared green.
- Cargo-lock contention on the shared dev machine is out of scope here; the
  lane grid moves release validation off the dev machine regardless.

## Supporting Skills

- `faber`: exempla harness + release protocol grounding.
- `housekeeping` Step 1 (mechanical): the per-packet fast-gate consumer.
- `sysadmin` / `pharos`: the nightly lane-grid host.

## Implementation Shape

- Phase 1 (proof lane): exempla Cargo.toml feature pass-through + cfg-gates in
  `mod.rs`; verify the rust lane end-to-end in feature isolation.
- Phase 2: gate `radix-mir-llvm` and `radix-mir-fmir` behind `dep:` features
  (real isolation; small builds stop compiling those leaves).
- Phase 3: move or `#[cfg]`-gate `failable_facts_parts` (leaf ownership).
- Phase 4: lane-scoped exemplar expectations + diff-derived lane selection
  (a packet touching `radix-hir-go` gates on the go lane only).
- Phase 5: nightly per-lane grid on pharos, lane→machine/toolchain manifest,
  status reporting into the workspace, lane-scoped bisect script.
- Phase 6: release protocol rewrite (tag-only-on-green; scripted
  pinned-sibling rehearsal; local release = bump → commit → tag → push).

## Release Posture

Decision: defer release — this goal makes future releases fast; it is not
itself a release surface. Its phases land as normal commits.

## Exit Strategy

Decision: not included.

## Acceptance Criteria

- `cargo test -p exempla --no-default-features --features hir-rust --test
  e2e_harness -- --ignored` builds feature-limited radix and passes the rust
  lane; the go/ts/wasm/llvm modules are not compiled.
- The same holds for each lane (go, ts, wasm, llvm, metal, roundtrip, mir).
- `cargo test -p exempla` with default features is unchanged and green.
- A nightly grid run on pharos reports main green/red per lane with receipts.
- A red lane localizes to a commit via the bisect script in ~7 lane-only runs.
- The release protocol doc says: release only on a green main; the local
  release is bump → commit → tag → push; the pinned-sibling rehearsal is one
  command.

## Validation

- Per-phase: the narrow lane command above + the existing ladder stages 1–3.
- Closeout: `./scripta/test --stage 1-3` in radix and the full e2e with
  default features once.
- Nightly-grid dry run on pharos before the protocol rewrite lands.

## Open Questions

- Does the roundtrip lane map to "frontend + forma" (no leaf feature) and run
  in every lane build or as its own minimal lane?
- Should the lane grid gate merges to main (Tier 0 enforcement) or only report
  (Tier 1 backstop) in the first iteration?
- Where does the grid status live (workspace file, Vivi memo, pharos badge)?
- Is `mir-llvm`'s unconditional presence load-bearing for the stepper, or is
  the `dep:` gate safe to add?

## Stop Conditions

- Stop if lane isolation silently reduces coverage (an unfeatured lane
  passing while its feature build is red).
- Stop if the full e2e (default features) regresses.
- Stop if the grid's green/red signal is not trustworthy within one nightly
  cycle (a false green is worse than no signal).
