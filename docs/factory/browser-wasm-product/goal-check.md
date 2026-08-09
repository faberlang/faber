# Goal Check — Browser Wasm Product

## Goal Check Summary

- Artifacts reviewed: `faber/docs/factory/browser-wasm-product/goal.md` and
  `CAMPAIGN.md`, plus the consumed predecessor campaigns
  `radix/docs/factory/wasm-host-parity/` and
  `faber/docs/factory/direct-device-product-pipeline/`.
- Evaluator mode: **independent cold pass** against the live tree; the
  self-pass verdict already recorded in `goal.md` §Goal Check was treated as
  evidence, not authority.
- Intended next consumer: Stage 0 delivery planning (this folder).
- Handoff bar: mid-tier implementation model with no material architectural
  choice left to invent for the selected stage.
- Verdict: **READY** for Stage 0 delivery planning.

## Reasoning

The goal's central claim — that a package-to-core-Wasm path already exists and
only needs a browser product recipe, a generic host, target-neutral web
contracts, and acceptance proofs — was re-verified live. `faber build --target
wasm --package .` builds a real package module set (entry + sibling units),
the `faber-host-wasm` host links and runs that set, the current browser route
is genuinely TypeScript/`tsc`-bound, and both consumed predecessor campaigns
match their described state. No stop-condition trigger and no stale claim
material to the starting path was found. Two target-capability claims are
stale in the live tooling (see Stage 0 obligations); both are exactly what
Stage 0 must record, so they do not block this goal.

## Key Points

- **Live probe (2026-08-09):** `faber build --target wasm --package .`
  succeeds on a two-unit package. Output: one WAT module per unit under
  `<package>/target/faber/wasm/`, entry exports `incipit`, imports close on
  `faber_rt_v1` + `faber_external`, sibling exports the canonical
  `__faber_external_*` symbol. No `.wasm` file is persisted; binary bytes are
  retained in the build manifest only. This confirms the CAMPAIGN "persists
  WAT, retains wasm bytes in memory" baseline and `faber build --target wasm
  --package .` as the compiler path.
- **Host module-set linking is real:** `hosts/wasm` (`faber-host-wasm`,
  wasmtime 45) exposes `WasmRtV1Host::run_package`; `hosts/wasm/tests/
  package_run_test.rs` proves entry + sibling instantiation, `faber_external`
  cross-module resolution, and typed missing-import/entry/trap/link outcomes.
  `faber/src/package/wasm_test.rs` proves the package builder + host round
  trip end to end.
- **The TS browser route is exactly as described:** `[build] target = "ts"` +
  `[product] kind = "browser-app"` with `controllers_json`, and
  `faber/src/package/product/ts_render.rs` invokes `tsc --project` and renders
  `tsconfig`. `examples/browser-app/faber.toml` and
  `triga/corpus/webgl-geometries/faber.toml` are live instances.
- **faber-web is TS-oriented as claimed:** `bindings/ts.toml` maps
  `web:dom.*` / `web:canvas2d.*` routes to `runtime/dom.ts` /
  `runtime/canvas2d.ts` shims; `src/dom.fab` holds the contracts;
  `tests/contract-test.ts` / `dom-runtime-test.ts` are a TS harness. The README
  records the `dom.fetch_text` async codegen gap — relevant to the campaign's
  async law.
- **Browser WebGPU proof exists in JS:** `hosts/webgpu-browser` owns
  `public/src/backend/webgpu-runtime.js` + engine/, checked-in generated
  `kernel.wgsl`/`reflection.json`/`graphics.*`, and `scripta/
  webgpu-browser-proof {generate|check|serve}` (serve at
  http://127.0.0.1:8787/). `hosts/webgpu-browser/public/src/` is the
  engine-JS source of truth; demos copy from it via their `tests/run.sh`
  (`HOST_DIR=$WORKSPACE/hosts/webgpu-browser`). `triga/corpus/_host/`
  contains only a superseded-pointer `README.md` (DS-S2 extraction), not live
  engine JS.
  Three.js is present as `vendor/three@0.180` (presentation chrome per README).
- **Wasm Host Parity matches its campaign state:** active; Stage 1 baseline
  complete (live ledger `baseline-gap-ledger.toml`: 308 rows — 160 parity,
  132 gap, 13 contract-reject, 3 n/a — regenerated 2026-08-06/07; the
  2026-08-05 baseline of 307 rows / 30 parity / 264 gap / 13 contract-reject
  is recorded historically in `stage-1-baseline-status.md`). "Bounded MIR
  subset" in the CAMPAIGN Current
  Tracks table is accurate; the browser controller + Triga shape gaps are the
  real Stage 1 work. `hosts/wasm` is the v1 host owner; no browser packaging
  there.
- **Direct Device Product Pipeline matches:** DDPP0 delivered 2026-08-08;
  DDPP1 waits on DDCP2. The submission-region boundary the browser campaign
  imports is already locked contract (§PreparedRegion), not a new invention.
- **No stop-condition trigger:** no HIR-direct/browser-compiler proposal, no
  JS-behind-Wasm, no second ABI, no TS fallback in the accepted path, no
  reflection-by-WGSL-parsing, no Workers/WASI/wasm64/threads scope.
- **Factory machinery is clean:** `faber/docs/factory/README.md` lists
  `browser-wasm-product | planned`; `./scripta/check-factory-goal-status`
  reports "no drift flagged (20 goals scanned)".

## Stage 0 Obligations (named conflicts to lock, not blockers)

These are live discrepancies the goal already directs Stage 0 to record. They
are confirmed present in the tree and must appear in the Stage 0 inventory:

1. **`wasm` capability row is stale.** `faber targets` and
   `radix/crates/radix/src/tool/commands/targets.rs`
   (`faber_target_capabilities`) report `wasm … run=no package=no` with note
   "not faber run/package", and `radix/docs/design/
   target-capability-matrix.md` mirrors it — but `faber/src/package/compile.rs`
   (CLI package path), `artifact_plan.rs`
   (`plan_package(Target::MirWasmBinary)` → supported), and `wasm.rs` +
   `wasm_test.rs` prove package-mode wasm build and host `run_package` are
   live. The row must be reconciled on the `package` flag (and `run` clarified
   as host-crate-only today).
2. **`faber.toml [build] target = "wasm"` is rejected.** `manifest_build_target`
   (`src/package/manifest.rs`) accepts `rust|fhir|ts|typescript|scena|fmir-*|
   llvm-host` only. The CLI `--target wasm` override works, but the manifest
   row (needed by the Stage 3 product recipe) is absent.
3. **Wasm deferral in the capability matrix** ("reopen when browser-Wasm
   execution is a specified deliverable") is precisely the reopen this
   campaign performs; Stage 0 records the supersession.

## Blocking Gaps

None. The goal is implementation-handoff-ready for the selected stage.

## Recommended Next Step

Proceed to `delivery` for Stage 0 (see `delivery-stage0.md` in this folder).
Stage 0 is evidence-only: inventory, reference evidence, baseline, conflicts,
allowlist, async ledger, and fixture selection with exact proof commands.
