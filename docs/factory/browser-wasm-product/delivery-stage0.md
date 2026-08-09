# Delivery Spec — BROWSER-WASM-PRODUCT Stage 0: Baseline, Ownership, And Boundary Lock

**Status**: planned (delivery lowering complete, 2026-08-09)
**Planner**: planner-1 (task f007aaf0)
**Campaign**: [CAMPAIGN.md](CAMPAIGN.md) · **Goal**: [goal.md](goal.md) ·
**Goal-check**: [goal-check.md](goal-check.md) — READY
**Control-plane repo**: `/Users/ianzepp/work/faberlang/faber`
**Mode**: **P3 delivery** — evidence/inventory tier, zero product code
**Posture**: discovery-first (CAMPAIGN §Stage 0)
**Consumes**: `radix/docs/factory/wasm-host-parity/` (Stage 1 baseline complete;
v1 host live) and `faber/docs/factory/direct-device-product-pipeline/`
(DDPP0 delivered; submission-region boundary locked in `ddpp0-contract.md`
§PreparedRegion).

## 1. Interpreted Unit

Stage 0 of the Browser Wasm Product campaign: **baseline, ownership, and
boundary lock** — inventory every generated and authored TypeScript,
JavaScript, JSON, WGSL, and Wasm artifact in the current browser route; measure
current behavior; reconcile stale Wasm product claims; select the first
controller and Triga static fixture; freeze the initial JavaScript allowlist.

This is a **measurement and decision slice**. It writes no product code, no
host code, no compiler code. Its artifacts are checked-in evidence + one
boundary review, all under `faber/docs/factory/browser-wasm-product/`. The
campaign remains the status authority; sibling repos (radix, hosts, faber-web,
triga, examples) are **read-only** this stage.

**Done-oracle (campaign gate, distilled):** every CAMPAIGN §Stage 0 gate bullet
is satisfied by a checked-in artifact with a named file; no unresolved question
changes the Stage 1 (Wasm reachability) or Stage 2 (browser host admission)
boundary.

## 2. Normalized Spec

Deliverables (each = one checked-in artifact under `evidence/`):

| # | Deliverable | Artifact |
| --- | --- | --- |
| D0 | Route-map artifact inventory (producer, consumer, owner, runtime-required, keep/move/remove) | `evidence/artifact-inventory.md` |
| D1 | Current TypeScript browser behavior recorded as reference evidence | `evidence/ts-browser-reference.md` |
| D2 | Faber wasm package baseline (WAT-persist / in-memory-wasm limitation; Stage 3 names durable binary module-set output) | `evidence/faber-wasm-package-baseline.md` |
| D3 | Target-capability conflict ledger with live authority | `evidence/target-conflict-ledger.md` |
| D4 | Host JavaScript file allowlist + line/byte baseline | `evidence/host-js-allowlist.md` |
| D5 | Async ABI ledger (operation ids, dispatcher export, status/payload, cancellation race, non-reentry, ordering, device loss, future-valued routes) | `evidence/async-abi-ledger.md` |
| D6 | Triga fixture selection + exact browser proof commands | `evidence/triga-fixture-selection.md` |
| D7 | Stage 0 gate closeout + boundary review | `stage0-closeout.md` |

## 3. Repo-Aware Baseline

Live tree state (verified 2026-08-09; all sibling repos clean, faber `main`
@ `23266e0`):

- **Faber wasm package path (live, feature `mir-wasm`):**
  `src/package/compile.rs` (CLI `Target::MirWasmBinary` dispatch),
  `src/package/artifact_plan.rs` (`plan_package(…, MirWasmBinary)` →
  supported, target `"wasm"`), `src/package/wasm.rs` (`build_package_wasm`:
  one module per unit; WAT written under `<pkg>/target/faber/wasm/`; `.wasm`
  bytes held in `manifest.entry_bytes`/`sibling_bytes`; `faber_external`
  cross-module symbols; `incipit` entry). Tests: `src/package/wasm_test.rs`.
  **No persisted `.wasm` binary, no Norma/library closure, no product recipe,
  no `faber.toml [build] target = "wasm"` row.**
- **Portable host (live):** `hosts/wasm` crate `faber-host-wasm`
  (`WasmRtV1Host::run_package`, wasmtime 45); tests
  `hosts/wasm/tests/package_run_test.rs`; depends on `radix-host-abi`
  (`__faber_rt_v1_*` closed surface). Native-only today — no browser host.
- **Browser product recipe (TS route):** `src/package/product/ts_render.rs`
  invokes `tsc --project` and renders tsconfig; `assets.rs`; manifest fields
  `controllers_json`/`assets_manifest` in `src/package/manifest.rs`
  (`ManifestProduct`). Live fixtures: `examples/browser-app/faber.toml`,
  `triga/corpus/*/faber.toml`.
- **faber-web (TS bindings):** `src/dom.fab`, `src/canvas2d.fab`, `src/web.fab`
  contracts; `bindings/ts.toml` (`[shim]`/`[functions]` → `runtime/dom.ts`,
  `runtime/canvas2d.ts`); `tests/contract-test.ts`, `tests/dom-runtime-test.ts`;
  `tsconfig.json`. README records the `fetch_text` async codegen gap.
- **Browser WebGPU proof (JS route):** `hosts/webgpu-browser` — `public/src/
  {backend,engine,contract,product,presentation}/*.js`, checked-in
  `public/generated/{kernel.wgsl,reflection.json,graphics.wgsl,
  graphics-reflection.json,graphics-*.bin,draw.json}`, `vendor/three@0.180`,
  `scripta/webgpu-browser-proof {generate|check|serve}` (serve
  http://127.0.0.1:8787/). Triga corpus: engine JS is **not** stored under
  `triga/corpus/_host/` — that directory holds only a superseded-pointer
  `README.md` (DS-S2 extraction, commit 24376cc); the engine-JS source of
  truth is `hosts/webgpu-browser/public/src/`, synced into each demo's
  `public/` via the demo's `tests/run.sh` (`HOST_DIR=$WORKSPACE/
  hosts/webgpu-browser`). Plus `triga/corpus/serve.sh`
  (http://127.0.0.1:8780/).
- **Predecessors:** wasm-host-parity active — Stage 1 oracle baseline complete
  (`baseline-gap-ledger.toml` live ledger, 308 rows: 160 parity / 132 gap /
  13 contract-reject / 3 n/a — regenerated 2026-08-06/07 in commits
  1b5fc0c59/24881453a/38002fae7/5614335ae; `stage-1-baseline-status.md`
  records the earlier 2026-08-05 baseline of 307 rows: 30 parity / 264 gap /
  13 contract-reject, and radix/corpus has since grown to 309 `.fab`),
  Stages 2–8 planned; DDPP DDPP0 delivered 2026-08-08 (submission-region
  boundary locked), DDPP1 waits on DDCP2.
- **Known stale claims (must be recorded in D3):** `faber targets` / radix
  `tool/commands/targets.rs` `wasm` row `run=no package=no` vs live package
  wasm build; `manifest_build_target` rejects `[build] target = "wasm"`;
  `target-capability-matrix.md` §Browser Application Product Packaging wasm
  deferral.

## 4. Stage Graph

All units are **evidence-only**. Write scope is strictly
`faber/docs/factory/browser-wasm-product/` (new `evidence/` subdir + two root
artifacts). Sibling repos are read-only.

```text
U0 (inventory) ─┬─ U1 (ts reference) ───────────┐
                ├─ U2 (wasm baseline) ─┬────────┤
                ├─ U3 (conflict ledger) ─┘ (a) ─┤
                ├─ U4 (host allowlist) ─────────┤
                ├─ U5 (async ledger) ───────────┤
                └─ U6 (triga selection) ────────┴─ U7 (closeout + boundary review)

(a) U3 depends_on U2 (probe evidence); soft edge — see Serialization boundary.
```

- **Serialization boundary:** U0 is the only unit with a write to the master
  inventory; all later units cite its paths/counts. U1–U6 are otherwise
  mutually independent (disjoint write files) and may run in parallel lanes
  after U0. **One soft ordering edge:** the U3 unit table sets
  `depends_on: U0, U2` because its done_when row 2 cites U2's live CLI probe
  as authority; the edge is soft — U3 may re-run the prebuilt-binary probe
  itself (`./target/debug/faber targets`, no cargo) and then has no ordering
  constraint, so U3 can dispatch alongside U2.
  U7 is strictly last. **Zero cross-repo writes**, so no repo-level
  serialization beyond the Cargo lock rule below.
- **Cargo discipline (tugboat):** every cargo invocation is `-p`-scoped, run
  one at a time (shared workspace lock). No bare `cargo build/test/check`, no
  `--workspace`, no `--all`. Stage 0 units touch no code, so cargo commands
  are *verification-only* of existing behavior; where a unit's validation is
  purely `rg`/`find`/node/manifest greps, prefer those and skip cargo entirely.

## 5. Implementation Work — Ordered Unit Graph

### U0 — Route-map artifact inventory

| Field | Value |
|---|---|
| `id` | `bwp-s0-u0-artifact-inventory` |
| `outcome` | One checked-in inventory (`evidence/artifact-inventory.md`) names every generated and authored `.ts`/`.tsx`, `.js`, `.json`, `.wgsl`, `.wasm`, and `.wat` artifact in the current browser route with: producer, consumer, owner, runtime-required (yes/no), and intended disposition (keep / move / remove). |
| `write_scope` | `faber/docs/factory/browser-wasm-product/evidence/artifact-inventory.md` (new) |
| `read_scope` | `examples/browser-app/`, `examples/web-canvas2d-smoke/`, `faber-web/` (`src/`, `runtime/`, `bindings/`, `tests/`, `tsconfig.json`), `hosts/webgpu-browser/public/` (incl. `generated/`, `vendor/`), `triga/corpus/webgl-geometries/` + `webgl-geometry-terrain/` + `webgl-animation-orbit/` + `webgl-animation-terrain/` + `webgl-animation-water/` (incl. `_host/README.md` superseded-pointer note — no live engine JS there; `faber.lock`, `faber.toml`), `faber/src/package/product/` (recipe emitters: `ts_render.rs`, `assets.rs`) |
| `done_when` | Every artifact class from the CAMPAIGN §Stage 0 gate ("generated and authored TypeScript, JavaScript, JSON, WGSL, and Wasm") has a row; each row has all five named fields; per-class counts are measured by the exact commands in `validation` (never copied from campaign prose); the inventory's disposition column is consistent with the CAMPAIGN JS boundary budget and product budget (e.g. `controllers.json`, `draw.json`, reflection JSON → remove-at-closeout; `webgpu-runtime.js` engine lanes → shrink-to-thunks; WGSL + reflection → keep). |
| `validation` | `rg --files examples/browser-app faber-web hosts/webgpu-browser/public triga/corpus | grep -E '\.(ts|tsx|js|json|wgsl|wasm|wat)$'` (note: no cargo needed); recount commands re-run idempotent; inventory table cross-checked against `triga/corpus/webgl-geometries/faber.toml` + `examples/browser-app/faber.toml` product blocks. |
| `est_work_tokens` | 4 000 – 7 000 |
| `tool_latency` | rg/find + node greps < 15 s; no cargo |
| `depends_on` | None |
| `non_goals` | No edits in any sibling repo; no product decisions beyond disposition tags (keep/move/remove); no re-deriving counts from other campaign docs. |

### U1 — TypeScript browser behavior reference evidence

| Field | Value |
|---|---|
| `id` | `bwp-s0-u1-ts-browser-reference` |
| `outcome` | `evidence/ts-browser-reference.md` records current TS browser behavior as reference evidence: the `tsc`-invoking build transcript, the `controllers.json` shape, the `dist/` layout, the serve route, and the WEB5 fixture evidence pointer — plus the `dom.fetch_text` async gap note from `faber-web/README.md`. |
| `write_scope` | `faber/docs/factory/browser-wasm-product/evidence/ts-browser-reference.md` (new); transcript capture under the same file (inline code blocks) |
| `read_scope` | `examples/browser-app/`, `examples/web-canvas2d-smoke/`, `triga/corpus/webgl-geometries/`, `faber-web/README.md`, `radix/docs/design/target-capability-matrix.md` §Browser Application Product Packaging, `radix/docs/factory/faber-hir-v1/browser-application-delivery.md` §WEB6 |
| `done_when` | Reference doc shows: (a) `faber build --package .` on `examples/browser-app` succeeds and produces `dist/` with `faber-esm/` + `controllers.json` (browser-app fixture, requires `tsc` on PATH); (b) the transcript records the `tsc --project` invocation (from `ts_render.rs` behavior); (c) `controllers.json` fields captured (selector/mount facts); (d) `triga/corpus/webgl-geometries/tests/run.sh` outcome recorded (asset sync → check → build → contract greps) and its `faber.lock` (`web` + `triga` as `target_language = "ts"` path deps) quoted; (e) the `fetch_text` TS async gap + WEB5 fixture scope quoted with source paths. |
| `validation` | From `faber/`: `target/debug/faber build --package .` in a fixture dir via the fixture's own `tests/run.sh` (`FABER_BIN` env) — transcript-only, no cargo; `node`/grep checks on `dist/` mirror `tests/run.sh` assertions (`dist/controllers.json`, `dist/faber-esm/faber-browser.js`, `dist/public/src/product/bootstrap.js`). Physical browser WebGPU observation is **not** this unit (see U6/auditor gates). |
| `est_work_tokens` | 3 000 – 5 000 |
| `tool_latency` | `faber build` on a fixture 5–60 s (prebuilt binary); `tests/run.sh` 1–3 min; no cargo |
| `depends_on` | U0 (inventory paths/counts to cite) |
| `non_goals` | No product behavior change; no tsc bypass; no fixing the `fetch_text` gap (record only). |

### U2 — Faber wasm package baseline

| Field | Value |
|---|---|
| `id` | `bwp-s0-u2-faber-wasm-baseline` |
| `outcome` | `evidence/faber-wasm-package-baseline.md` records the live package-to-wasm state: one module per unit; WAT persisted under `<pkg>/target/faber/wasm/`; binary `.wasm` retained in-memory only (`manifest.entry_bytes` / `sibling_bytes`); closed `faber_rt_v1` + `faber_external` imports; `incipit` entry; host round-trip via `WasmRtV1Host::run_package`; **absent**: persisted binary module set, Norma/library closure, browser product recipe, `faber.toml [build] target = "wasm"` row. Names the Stage 3 durable output (deterministic binary module set + manifest) as the replacement per CAMPAIGN §Stage 0 gate. |
| `write_scope` | `faber/docs/factory/browser-wasm-product/evidence/faber-wasm-package-baseline.md` (new) |
| `read_scope` | `faber/src/package/{wasm.rs,wasm_test.rs,compile.rs,artifact_plan.rs,manifest.rs}`, `hosts/wasm/`, `radix/crates/radix-host-abi/src/lib.rs` |
| `done_when` | Baseline doc cites: (a) the live CLI probe output `faber build --target wasm --package .` (two-unit package → two `.wat` files, `incipit` export, canonical `__faber_external_*` sibling export); (b) `manifest_build_target`'s allowed-target list quoted (no `"wasm"`); (c) `wasm_test.rs` + `package_run_test.rs` coverage summary (builder + host round trip); (d) exact gap list matching the CAMPAIGN "locked source-library closure is incomplete" row. |
| `validation` | `cargo test -p faber package::wasm::tests` (faber crate; single-crate, no workspace flag). For the CLI probe: `faber target/debug/faber build --target wasm --package .` on a fresh two-unit package in `/tmp` (prebuilt binary, no cargo). Host side only if needed: `cargo test -p faber-host-wasm` (from `hosts/`). Run one cargo command at a time. |
| `est_work_tokens` | 2 500 – 4 500 |
| `tool_latency` | CLI probe ~30–60 s; `cargo test -p faber package::wasm::tests` 1–10 min (large crate, lock contention); `cargo test -p faber-host-wasm` 30 s–3 min |
| `depends_on` | U0 |
| `non_goals` | No emitter/compiler edits; no persisting `.wasm` files; no product recipe work. |

### U3 — Target capability conflict ledger

| Field | Value |
|---|---|
| `id` | `bwp-s0-u3-target-conflict-ledger` |
| `outcome` | `evidence/target-conflict-ledger.md` names every target-capability conflict with live authority (file:line + observed command), per CAMPAIGN §Stage 0 gate ("every target capability conflict is named with live authority"). |
| `write_scope` | `faber/docs/factory/browser-wasm-product/evidence/target-conflict-ledger.md` (new) |
| `read_scope` | `faber/src/commands/targets.rs`, `faber/src/package/{compile.rs,artifact_plan.rs,wasm.rs,manifest.rs}`, `radix/crates/radix/src/tool/commands/targets.rs`, `radix/docs/design/target-capability-matrix.md` |
| `done_when` | At minimum these rows, each with live authority + proposed reconciliation owner: (1) `faber targets` / radix `tool/commands/targets.rs` `wasm` row `run=no package=no` + note "not faber run/package" vs live `compile.rs`/`artifact_plan.rs` package wasm build (authority: `radix/crates/radix/src/tool/commands/targets.rs` `MirWasmBinary` capability row; `faber/src/package/artifact_plan_test.rs` `plan_package_is_supported_and_no_longer_rejected`); (2) `manifest_build_target` rejects `[build] target = "wasm"` while CLI `--target wasm` works (authority: `faber/src/package/manifest.rs` `manifest_build_target`, live CLI probe); (3) `target-capability-matrix.md` §Browser Application Product Packaging wasm deferral (reopen event = this campaign) + §CLI capability surface `wasm` row `run=no package=no`; (4) `faber-web/README.md` architecture law ("browser apps use … HIR → TypeScript emit") vs this campaign's Wasm product goal (supersession recorded, not a code conflict). |
| `validation` | `./target/debug/faber targets` (prebuilt binary) captured in the doc; `rg -n "MirWasmBinary" radix/crates/radix/src/tool/commands/targets.rs faber/src/package/manifest.rs` for line authorities; no cargo. |
| `est_work_tokens` | 3 000 – 5 000 |
| `tool_latency` | rg + `faber targets` < 15 s; no cargo |
| `depends_on` | U0, U2 (probe evidence) |
| `non_goals` | No editing the capability tables/docs (that is Stage 3/8 product work; Stage 0 names owners only). |

### U4 — Host JavaScript allowlist + byte baseline

| Field | Value |
|---|---|
| `id` | `bwp-s0-u4-host-js-allowlist` |
| `outcome` | `evidence/host-js-allowlist.md` enumerates every authored JS/TS host file in the browser route, measures line + byte totals per file, classifies each against the CAMPAIGN semantic budget (allowed thunk: module loading / handle map / callback transport / raw DOM+WebGPU calls / declared asset fetch; **not allowed**: app/controller/Triga policy, renderer policy, shader/layout selection, source-semantics recovery), and records the exact initial allowlist as the Stage 0 hard gate (per CAMPAIGN "The semantic allowlist, not an arbitrary total-size promise, is its first hard gate"). |
| `write_scope` | `faber/docs/factory/browser-wasm-product/evidence/host-js-allowlist.md` (new) |
| `read_scope` | `hosts/webgpu-browser/public/src/` (all `*.js` incl. `backend/webgpu-runtime.js`, `engine/`, `contract/`, `product/`, `presentation/` — the engine-JS source of truth), `triga/corpus/_host/README.md` (superseded-pointer note only; engine JS no longer lives under `_host/` — DS-S2 extraction), `triga/corpus/*/public/` (generated copies, noted as generated), `faber-web/runtime/*.ts` (shims — TS side of the same budget) |
| `done_when` | Every authored host file has a row: path, bytes, lines (measured via `wc -c`/`wc -l`), budget class (allowed-thunk / not-allowed / generated-copy), and rationale against the CAMPAIGN §JavaScript Boundary Budget. Per-file byte totals plus the shared-host total are recorded (this is the "current bytes and modules" Stage 0 baseline the CAMPAIGN requires). The doc calls out that today's `engine/` + `scene-extractor.js` + `renderGreyboxSceneFrame` etc. are *reference JS policy* that the allowlist must shrink or relocate. |
| `validation` | `wc -c -l hosts/webgpu-browser/public/src/**/*.js` (engine-JS source of truth; no `triga/corpus/_host/public/**/*.js` term — that directory does not exist, see `_host/README.md` pointer note) (and explicit file list); `rg --files …` cross-check against inventory (U0); no cargo. |
| `est_work_tokens` | 2 500 – 4 500 |
| `tool_latency` | wc/rg < 15 s; no cargo |
| `depends_on` | U0 |
| `non_goals` | No host edits; no writing new host JS; no deleting reference JS (clean break happens at Stage 8). |

### U5 — Async ABI ledger

| Field | Value |
|---|---|
| `id` | `bwp-s0-u5-async-abi-ledger` |
| `outcome` | `evidence/async-abi-ledger.md` records the async ABI contract the browser host will expose — per CAMPAIGN §Stage 0 gate: operation identifiers (opaque `i32`), dispatcher export name, status + payload record layout, cancellation race behavior, non-reentry rule, ordering rule, device-loss delivery, and the admitted/deferred list of future-valued routes (with `fetch_text` explicitly dispositioned against the `faber-web` TS gap). Grounded in the existing `__faber_rt_v1_*` closed surface (no new ABI dialect). |
| `write_scope` | `faber/docs/factory/browser-wasm-product/evidence/async-abi-ledger.md` (new) |
| `read_scope` | `radix/crates/radix-host-abi/src/lib.rs` (symbol surface), CAMPAIGN.md §Async host law (the agreed law to name), `faber-web/README.md` (`fetch_text` gap), `hosts/webgpu-browser/public/src/` (current JS async behavior as reference) |
| `done_when` | Ledger has named rows for: operation-id allocation rule; dispatcher export (e.g. `faber_rt_v1_async_dispatch` — name is provisional, marked as such); status codes (completed/failed/cancelled/device-lost); payload record shape; queue-after-import-returns + serialized non-reentrant delivery; exactly-one-terminal-result; best-effort cancellation with late-result discard; browser delivery order preserved (no fabricated total order); device-loss + unsolicited events through the same typed path; and a future-valued route table where `fetch_text` = **deferred/admitted-only-if-contract** (default: not admitted — fails closed), matching the CAMPAIGN. Every law cites its CAMPAIGN §Async host law bullet. |
| `validation` | Cross-check: each ledger row maps to one CAMPAIGN §Async host law bullet (traceability table in the doc); `rg -n "fetch_text|fetchText" faber-web radix --glob '!**/target/**'` to confirm current status; no cargo. |
| `est_work_tokens` | 3 000 – 5 000 |
| `tool_latency` | rg < 15 s; no cargo |
| `depends_on` | U0 |
| `non_goals` | No ABI code/constant edits (radix-host-abi stays untouched; the ledger records names for Stage 2 contract work); no resolving the TS async gap. |

### U6 — Triga fixture selection + exact proof commands

| Field | Value |
|---|---|
| `id` | `bwp-s0-u6-triga-fixture-selection` |
| `outcome` | `evidence/triga-fixture-selection.md` confirms the vertical-slice order and records the exact browser proof commands with their evidence oracle: `triga/corpus/webgl-geometries` (first static slice) → `webgl-geometry-terrain` → `webgl-animation-orbit` → `webgl-animation-terrain` (per CAMPAIGN §Stage 0 gate), including the observed-pixels / frame-progress oracle each proof must satisfy. |
| `write_scope` | `faber/docs/factory/browser-wasm-product/evidence/triga-fixture-selection.md` (new) |
| `read_scope` | `triga/corpus/` (`README.md`, `serve.sh`, `serve.mjs`, each demo's `faber.toml` + `tests/run.sh`), `hosts/scripta/webgpu-browser-proof`, `hosts/webgpu-browser/README.md`, `triga/docs/factory/hello-voxel/` (sibling graphics/WebGPU contracts — Goal 00 complete, Goal 02 planned; consume its accepted reflection/fragment facts as build inputs, do not duplicate) |
| `done_when` | Selection doc names for each fixture: demo dir, its three.js reference, what it pressures, current `[build] target`/`[product]` block, exact build+serve command (`cd triga/corpus/webgl-geometries && ./tests/run.sh`; `cd triga/corpus && ./serve.sh` → http://127.0.0.1:8780/webgl-geometries/; static host admission `cd hosts && ./scripta/webgpu-browser-proof check`), the evidence oracle per CAMPAIGN (non-background pixel readback, frame progress, input response, console diagnostics — physical browser runs are **auditor/operator gates**, recorded as such, not Hand claims), and the known wasm-host-parity reachability risk for each fixture's shapes (camera_controls.fab, scene.fab, shapes.fab, shaders, matrices/vectors — Stage 1 candidates; cite `baseline-gap-ledger.toml` row families). |
| `validation` | `cd triga/corpus/webgl-geometries && FABER_BIN=/Users/ianzepp/work/faberlang/faber/target/debug/faber ./tests/run.sh` (prebuilt binary; transcript captured; **no cargo**); `cd hosts && ./scripta/webgpu-browser-proof check` (static admission; regenerates to temp + node checks); browser serve + pixel evidence deferred to the auditor gate. |
| `est_work_tokens` | 2 000 – 4 000 |
| `tool_latency` | `tests/run.sh` 1–3 min; `webgpu-browser-proof check` ~1–2 min; no cargo |
| `depends_on` | U0 |
| `non_goals` | No triga/radix/hosts edits; no new demo fixtures; no physical-browser evidence claims (that is the Stage 5/auditor surface). |

### U7 — Stage 0 gate closeout + boundary review

| Field | Value |
|---|---|
| `id` | `bwp-s0-u7-stage0-closeout` |
| `outcome` | `stage0-closeout.md` re-checks every CAMPAIGN §Stage 0 gate bullet against the U1–U6 evidence, records the boundary review (no unresolved question changes the Stage 1 compiler or Stage 2 host boundary), and emits the Stage 0→1 handoff: the concrete shape list for Stage 1 (package calls, records/lists, option/error carriers, matrices/vectors, callable identity, callback exports — refined by the fixture selection) and the Stage 2 admission input (host allowlist + async ledger). |
| `write_scope` | `faber/docs/factory/browser-wasm-product/stage0-closeout.md` (new) |
| `read_scope` | All U0–U6 evidence files; CAMPAIGN.md §Stage 0 gate; goal.md §Open Questions |
| `done_when` | A gate-checklist table maps each §Stage 0 gate bullet → evidence file + row; open-question section lists only questions that cannot change Stage 1/2 boundaries (each with a named owner + default); Stage 1 shape list and Stage 2 input packet are explicit. |
| `validation` | `./scripta/check-factory-goal-status` (faber; no cargo) — must report no drift; `git diff --check`; the goal-status audit remains clean. |
| `est_work_tokens` | 2 000 – 4 000 |
| `tool_latency` | `check-factory-goal-status` < 10 s; no cargo |
| `depends_on` | U1, U2, U3, U4, U5, U6 |
| `non_goals` | No Stage 1/2 implementation; no product code; no README regeneration unless the status line changes (it does not — Stage 0 stays `planned`). |

## 6. Checkpoints And Gates

- **Batching / Split Decision:** discovery-first, one coherent evidence slice.
  No split: every unit is read-only evidence under one control-plane folder.
  Split (into owner-repo deliveries) happens at Stage 1+ per the CAMPAIGN.
- **Stage 0 gate (all bullets):** satisfied by U0–U7 artifacts, checked at U7.
- **Release posture:** `not-applicable` / `defer-release`. Stage 0 is evidence
  only; no tags, pushes, deployment, or publication. The Stage 8 release
  checkpoint stays an operator gate (goal.md §Release Posture).
- **Auditor/operator boundaries:** physical browser WebGPU runs, pixel
  readback, frame-progress evidence, and the release decision are auditor /
  operator gates. Hands record transcripts and static admission only.
- **Foreign dirt:** none present in any sibling tree as of 2026-08-09 (all
  `git status --short` clean). This delivery writes only under
  `faber/docs/factory/browser-wasm-product/`. If dirt appears on a read path
  mid-stage, classify (A/B/C) and report — never normalize foreign WIP.

## 7. Validation

Per-unit validation is in the unit tables. Aggregate closeout (U7):

```bash
cd /Users/ianzepp/work/faberlang/faber
./scripta/check-factory-goal-status      # no drift (no cargo)
git diff --check
```

Exact cargo commands allowed anywhere in this stage (single-crate only, run
one at a time):

```bash
cargo test -p faber package::wasm::tests          # U2 — package wasm builder + host round trip
cargo test -p faber-host-wasm                      # U2 — host module-set linking (run from hosts/)
cargo test -p exempla --test e2e_harness wasm_ledger   # U6 read-only cross-check (radix workspace; timeout 300 per wasm-host-parity)
```

No `cargo build`, no bare `cargo test`, no `--workspace`/`--all`, no parallel
cargo invocations. Everything else uses the prebuilt `faber/target/debug/faber`
binary and `rg`/`find`/`wc`/`node`.

## 8. Companion Skill Plan

- `faber` skill: verify language/compiler/host claims against live source
  (used throughout U0–U6).
- `campaign` skill: this delivery is the §Stage 0 lowering; keep the campaign
  as the status authority (no per-repo campaign duplication).
- `goal-check` skill: already consumed — see [goal-check.md](goal-check.md).
- `delivery`/`factory`: Stage 1+ lowers through per-owner goal-forge →
  goal-check → delivery → factory, per CAMPAIGN §Lowers to.
- `zombie-docs`: repair the stale `wasm` capability row / capability-matrix
  claims at the owner-appropriate stage (named in U3; repair itself is out of
  scope for Stage 0).

## 9. Open Questions

Non-blocking (each with owner + default, none can change the Stage 1/2
boundary):

1. **Stable binding/reflection table encoding** (wasm data exports vs custom
   section vs compact sidecar) — measured at Stage 0 (U0/U4), decided before
   Stage 3. Default: keep current generated JSON as reference evidence; no
   new encoding chosen in Stage 0.
2. **Async dispatcher export spelling** (`faber_rt_v1_async_dispatch`
   provisional) — owned by Stage 2 host contract, recorded in U5 with an
   explicit provisional marker.
3. **Coarser render-command batching** — Stage 6 measurement decision;
   default: submission-region boundary as locked in DDPP0 §PreparedRegion.
4. **Faber version / migration window** at Stage 8 — operator decision; not
   started here.

## 10. Stage Handoff Notes

- **Stage 0 is evidence-only** — no Hand product edits anywhere.
- **Stage 1 input:** concrete shape list (from U7) + wasm-host-parity Stage 1
  ledger rows for the selected fixtures' shapes.
- **Stage 2 input:** U4 host allowlist + U5 async ledger + the v1 runtime
  contract (`radix-host-abi`) + module-set linking contract (`hosts/wasm`).
- **Closeout rule (tugboat):** one `./scripta/test --stage 1-3`-equivalent
  closeout is not required for a docs-only stage; U7's `check-factory-goal-status`
  + `git diff --check` is the declared validation, run exactly once.
