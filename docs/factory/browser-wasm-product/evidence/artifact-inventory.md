# Route-Map Artifact Inventory — Stage 0 U0

**Unit**: `bwp-s0-u0-artifact-inventory` (delivery-stage0.md §U0, lines 122–136)
**Campaign**: BROWSER-WASM-PRODUCT Stage 0 — baseline, ownership, boundary lock
**Status**: delivered (evidence captured 2026-08-09)
**Hand**: hand-2 (tugboat, task `3c0a5a6f`)

## Capture environment

| Item | Value |
| --- | --- |
| workspace | `/Users/ianzepp/work/faberlang` (faber, faber-web, hosts, triga, examples sibling repos) |
| faber tree HEAD at capture | `68c32df` (`docs(factory): browser-wasm Stage 0 U1 — TS browser behavior reference evidence`) — clean at capture |
| capture date | 2026-08-09 |
| toolchain | `rg`/`find`/`wc`/`node` only — **no cargo** |

This is the master inventory for Stage 0. Per delivery-stage0.md §4, U0 is the
only unit that writes the master inventory; U1–U6 cite these paths/counts.
Sibling repos are read-only; no product code was written.

## 1. Measurement method — exact validation command

Per-class counts below are measured by the exact command from the U0
`validation` field (never copied from campaign prose):

```sh
cd /Users/ianzepp/work/faberlang
rg --files examples/browser-app faber-web hosts/webgpu-browser/public triga/corpus \
  | grep -E '\.(ts|tsx|js|json|wgsl|wasm|wat)$'
```

Run twice (2026-08-09) — output identical, so the recount is idempotent.
`rg --files` honors `.gitignore`, so per-demo `public/`, `src/shaders/`, and
`dist/` generated copies are excluded from this count; they are enumerated
separately in §6.

### Measured per-class counts (exact command)

| Class | Count | Artifacts |
| --- | --- | --- |
| `.ts` | 4 | faber-web runtime ×2 + tests ×2 |
| `.tsx` | 0 | — |
| `.js` | 13 | hosts/webgpu-browser `public/src` ×11 + `vendor/three@0.180` ×2 |
| `.json` | 6 | faber-webgpu-product.json, `generated/` ×4, faber-web tsconfig.json |
| `.wgsl` | 6 | hosts `public/generated` ×3 + triga corpus demo assets ×3 |
| `.wasm` | 0 | — |
| `.wat` | 0 | — |
| **Total** | **29** | |

## 2. Artifact inventory

Each row carries the five named fields: **producer**, **consumer**, **owner**,
**runtime-required**, **disposition**. Disposition vocabulary is anchored to
the CAMPAIGN budgets: `keep` / `move` / `remove`, with the budget's specific
verbs (`shrink-to-thunks`, `remove-at-closeout`) where the CAMPAIGN names them.

### Class `.ts` — TypeScript (4 files, all authored)

| Artifact | Producer | Consumer | Owner | Runtime-required | Disposition |
| --- | --- | --- | --- | --- | --- |
| `faber-web/runtime/dom.ts` | faber-web (hand-authored scoped DOM runtime, WEB4) | generated TS product code — emitted `web-dom.js`/`web-shim-dom.js` compiled from this shim; controller mounts | faber-web | yes (compiled into every current TS-route ESM build) | **keep** — re-express as target-neutral Faber contracts + Wasm binding map (Stage 4); no required TS runtime per CAMPAIGN |
| `faber-web/runtime/canvas2d.ts` | faber-web (hand-authored canvas2d binding shim) | emitted `web-canvas2d.js` facade; `web:canvas2d` routes | faber-web | yes (compiled into TS-route builds that use `web:canvas2d`) | **keep** — same Stage 4 re-express as `runtime/dom.ts` |
| `faber-web/tests/contract-test.ts` | faber-web (authored contract harness) | node/`tsc` test run; verifies `bindings/ts.toml` route↔shim bijection | faber-web | no | **keep** — authored test evidence |
| `faber-web/tests/dom-runtime-test.ts` | faber-web (authored DOM runtime test) | node/`tsc` test run | faber-web | no | **keep** — authored test evidence |

No `.tsx` files exist in the scan roots (measured: 0). The **generated** TS
(`dist/faber-ts/*.ts` in the browser-app fixture) is gitignored build output —
enumerated in §6, disposition remove-at-closeout.

### Class `.js` — JavaScript (13 files)

#### Host lanes — `hosts/webgpu-browser/public/src/` (11 files, all authored)

| Artifact | Producer | Consumer | Owner | Runtime-required | Disposition |
| --- | --- | --- | --- | --- | --- |
| `src/backend/webgpu-runtime.js` (2 699 lines, 94.5 KiB) | hosts/webgpu-browser (authored; moved from corpus `_host` via DS-S2) | `src/engine/engine.js`; demo `public/` sync copies via `tests/run.sh` | hosts/webgpu-browser | yes (device, buffer, pipeline, encoder, queue operations) | **shrink-to-thunks** — WebGPU engine lanes; raw capability calls stay, per-app/renderer policy moves to Wasm |
| `src/engine/engine.js` (1 096 lines, 36.4 KiB) | hosts/webgpu-browser (authored; renderer/session facade) | `src/product/bootstrap.js` `initEngine`; demo pages | hosts/webgpu-browser | yes (session creation, scene mount) | **shrink-to-thunks** — renderer/session policy → Wasm (CAMPAIGN: policy in Wasm) |
| `src/engine/frame-scheduler.js` (205 lines) | hosts/webgpu-browser (authored; rAF loop + readback phases) | demo pages via `initEngine` | hosts/webgpu-browser | yes (animation-frame loop) | **shrink-to-thunks** — loop-policy decisions → Wasm; frame-callback transport is an allowed thunk |
| `src/engine/resource-manager.js` (262 lines) | hosts/webgpu-browser (authored; GPU resource residency) | `src/engine/engine.js` | hosts/webgpu-browser | yes (buffer/mesh residency) | **shrink-to-thunks** — resource-lifetime policy → Wasm; raw create/upload stays |
| `src/engine/scene-extractor.js` (235 lines) | hosts/webgpu-browser (authored; parses reflection + DOM scene facts into render items) | `src/engine/engine.js` | hosts/webgpu-browser | yes (scene geometry extraction) | **shrink-to-thunks / relocate** — source-semantics recovery is NOT allowed in JS; extraction moves to Wasm (U4 calls this reference JS policy to shrink/relocate) |
| `src/contract/capability-admission.js` (183 lines) | hosts/webgpu-browser (authored; fail-closed device capability admission) | `src/engine/engine.js` before draw | hosts/webgpu-browser | yes (typed `CapabilityAdmissionError` gate) | **keep** — allowed thunk: fail-closed contract/admission check |
| `src/contract/artifact-admission.js` (726 lines, 28.2 KiB) | hosts/webgpu-browser (authored; artifact fetch + kernel/pipeline load + reflection consumption) | `src/app.js`, `src/engine/engine.js` | hosts/webgpu-browser | yes (fetches declared WGSL/reflection/draw artifacts) | **keep** — allowed thunk: declared asset fetch + module loading; consumes the kept reflection facts |
| `src/presentation/canvas.js` (83 lines) | hosts/webgpu-browser (authored; canvas surface lookup/sizing/context) | `src/product/bootstrap.js` | hosts/webgpu-browser | yes (canvas/WebGPU context) | **keep** — allowed thunk: raw DOM/canvas/WebGPU ops |
| `src/presentation/debug-overlay.js` (77 lines) | hosts/webgpu-browser (authored; `.triga-facts` DOM facts publishing) | Faber controllers via DOM facts contract | hosts/webgpu-browser | yes (facts channel) | **keep** — allowed thunk: raw DOM transport |
| `src/product/bootstrap.js` (120 lines, 4.3 KiB) | hosts/webgpu-browser (authored; session bootstrap + DOM bridge) | demo pages (imports `initEngine`) | hosts/webgpu-browser | yes (page entry) | **keep** — per-app loader/adapter within the ≤16 KiB unminified budget; session policy → Wasm |
| `src/app.js` (252 lines, 8.2 KiB) | hosts/webgpu-browser (authored; proof page entry — compute + graphics proofs via three.js) | `public/index.html` | hosts/webgpu-browser | yes (current proof page) | **remove-at-closeout** — reference proof harness; app-level policy, replaced by the Wasm route at the accepted gate |

#### Vendored — `hosts/webgpu-browser/public/vendor/three@0.180/` (2 files)

| Artifact | Producer | Consumer | Owner | Runtime-required | Disposition |
| --- | --- | --- | --- | --- | --- |
| `vendor/three@0.180/three.webgpu.js` | three.js upstream (vendored, v0.180) | `src/app.js` (presentation chrome only — never supplies binding/launch facts) | hosts/webgpu-browser (vendored copy) | yes (current proof presentation) | **remove-at-closeout** — product budget: zero Three.js runtime dependency |
| `vendor/three@0.180/three.core.js` | three.js upstream (vendored, v0.180) | `src/app.js` (via three.webgpu) | hosts/webgpu-browser (vendored copy) | yes (current proof presentation) | **remove-at-closeout** — same as `three.webgpu.js` |

### Class `.json` — JSON (6 files)

| Artifact | Producer | Consumer | Owner | Runtime-required | Disposition |
| --- | --- | --- | --- | --- | --- |
| `hosts/webgpu-browser/public/faber-webgpu-product.json` | hosts/webgpu-browser (authored product manifest for the WebGPU proof) | `./scripta/webgpu-browser-proof check\|serve`, humans, `product-boundary-check.mjs` | hosts/webgpu-browser | no (proof/build metadata; not fetched by the page) | **keep** — authored proof manifest; stable metadata with one owner |
| `hosts/webgpu-browser/public/generated/reflection.json` (16.5 KiB) | radix `emit --reflection -t wgsl-text` on `hosts/webgpu-browser/fixtures/add-one.fab` (via `webgpu-browser-proof generate`) | `src/contract/artifact-admission.js` `loadFaberKernel` — binding/layout/dispatch truth | radix (producer); hosts (staging) | yes (runtime binding truth) | **keep** — reflection is an owned compiler build fact; runtime serving is removed at Stage 7/8 closeout, facts stay |
| `hosts/webgpu-browser/public/generated/graphics-reflection.json` (22.0 KiB) | radix `emit --reflection -t wgsl-text` on `triga/exempla/triga-hello-voxel-shaders.fab` | `src/contract/artifact-admission.js` `loadFaberGraphicsPipeline` | radix (producer); hosts (staging) | yes (runtime binding truth for graphics pipeline) | **keep** — same reflection-facts rule as `reflection.json` |
| `hosts/webgpu-browser/public/generated/triga-lit-reflection.json` (4.4 KiB) | hosts (placeholder copy of the pre-radix lit-shader reflection; radix regeneration pending per `corpus/_host/README.md`); animation demos derive theirs via `tests/adapt-graphics-reflection.mjs` | `src/engine/engine.js` graphics pipeline; demo pages | radix/hosts (until radix regeneration lands) | yes (runtime binding truth for lit shader) | **keep** — reflection facts; regenerate from radix before Stage 5 proof |
| `hosts/webgpu-browser/public/generated/draw.json` (103 B) | `hosts/scripta/generate-graphics-payloads.py` (called by `webgpu-browser-proof generate`) | `src/engine/engine.js` draw submission | hosts (generated; radix submission-region producer later) | yes (current draw descriptor) | **remove-at-closeout** — draw descriptors become compiled submission-region descriptors from Wasm (CAMPAIGN submission boundary); zero runtime draw.json |
| `faber-web/tsconfig.json` | faber-web (authored TS project config for runtime shims + tests) | `tsc` (contract-test compile; WEB3 TS/ESM toolchain) | faber-web | no (build/tooling only) | **keep** — authored dev config; distinct from the *generated* `tsconfig.faber-browser.json` staged per build (see §6) |

### Class `.wgsl` — WGSL (6 files)

| Artifact | Producer | Consumer | Owner | Runtime-required | Disposition |
| --- | --- | --- | --- | --- | --- |
| `hosts/webgpu-browser/public/generated/kernel.wgsl` (475 B) | radix `emit -t wgsl-text` on `fixtures/add-one.fab` | `src/contract/artifact-admission.js` → WebGPU compute dispatch | radix (producer); hosts (staging) | yes (compute kernel) | **keep** — WGSL remains the shader artifact |
| `hosts/webgpu-browser/public/generated/graphics.wgsl` (1.6 KiB) | radix `emit -t wgsl-text` on `triga/exempla/triga-hello-voxel-shaders.fab` | `loadFaberGraphicsPipeline` → vertex/fragment graphics | radix (producer); hosts (staging) | yes (graphics shader) | **keep** — WGSL remains the shader artifact |
| `hosts/webgpu-browser/public/generated/triga-lit.wgsl` (5.0 KiB) | hosts (placeholder copy of the pre-radix lit shader; radix regeneration pending) | demo `public/triga-lit.wgsl` runtime fetch; `src/shaders/test-data/kernel.wgsl` build input | radix/triga (shader owner); hosts (staging) | yes (runtime shader) | **keep** — WGSL remains the shader artifact; regenerate from radix before Stage 5 |
| `triga/corpus/webgl-animation-orbit/assets/orbit.wgsl` (2.1 KiB) | triga corpus (authored demo shader) | demo `tests/run.sh` → `public/triga-lit.wgsl` + `src/shaders/test-data/kernel.wgsl` | triga | yes (via sync copy — the served name is `public/triga-lit.wgsl`) | **keep** — authored WGSL shader; demo-owned adapter |
| `triga/corpus/webgl-animation-terrain/assets/terrain.wgsl` (5.4 KiB) | triga corpus (authored procedural-terrain shader) | demo `tests/run.sh` sync (same as orbit) | triga | yes (via sync copy) | **keep** — authored WGSL shader |
| `triga/corpus/webgl-animation-water/assets/water.wgsl` (2.4 KiB) | triga corpus (authored flowing-water shader) | demo `tests/run.sh` sync (same as orbit) | triga | yes (via sync copy) | **keep** — authored WGSL shader |

### Class `.wasm` / `.wat` — Wasm (0 files)

Measured: **0** `.wasm` and **0** `.wat` files in the scan roots. This is a
true zero for the current browser route: no persisted binary module set exists.
The faber wasm package builder writes `.wat` under `<pkg>/target/faber/wasm/`
(faber package target dirs — outside the scan roots and gitignored) and
retains `.wasm` bytes only in memory (`manifest.entry_bytes` /
`sibling_bytes`). Stage 3 names the durable binary module-set + manifest output
that replaces this limitation; Stage 1/2 build toward it. Disposition for the
class: **n/a today** — the Wasm module set is the *target* artifact of the
campaign, not a present route artifact.

## 3. Disposition consistency vs. CAMPAIGN budgets

| CAMPAIGN budget rule | Inventory disposition |
| --- | --- |
| "zero runtime `controllers.json`, `draw.json`, or reflection JSON at closeout" | `draw.json` → **remove-at-closeout**; reflection JSON serving → removed at closeout while reflection **facts** stay (keep); `controllers.json` (generated, §6) → **remove-at-closeout** |
| "WGSL remains the shader artifact" | all 6 `.wgsl` → **keep** |
| "Preserve WGSL; expose reflection as owned build facts" | WGSL + `reflection.json`/`graphics-reflection.json`/`triga-lit-reflection.json` → **keep** (facts), served JSON removed at closeout |
| "shrink to generic raw capability thunks" (browser WebGPU host track) | `webgpu-runtime.js` + `engine/*` lanes → **shrink-to-thunks**; `contract/*`, `presentation/*`, `product/bootstrap.js` → **keep** (allowed thunks) |
| "zero Three.js runtime dependency" | `vendor/three@0.180/*` → **remove-at-closeout** |
| "no generated TS, no generated app JS, no TS configuration in output" | `dist/` generated `.ts`/`.js`/`json`/`tsconfig.faber-browser.json` (§6) → **remove-at-closeout** |
| "zero private per-demo renderer/runtime copies" | per-demo `public/` sync copies (§6) → **remove-at-closeout** (single source stays in `hosts/webgpu-browser`) |
| JS boundary budget: app/controller/Triga policy, renderer policy, shader/layout selection, source-semantics recovery **not allowed** | `src/app.js` → remove-at-closeout (reference proof); `scene-extractor.js` → shrink/relocate (source-semantics recovery); `engine.js`/`frame-scheduler.js`/`resource-manager.js` → shrink-to-thunks |

## 4. Product-block cross-check (validation requirement)

Cross-checked the inventory against the two named product blocks.

### `examples/browser-app/faber.toml`

```toml
[build]
target = "ts"
kind = "bin"

[product]
kind = "browser-app"
emit = "typescript"
out = "dist"
templates = "pages"
styles = "styles"
public = "public"
controllers_json = "controllers.json"
```

- `controllers_json = "controllers.json"` → confirms the **generated**
  `controllers.json` class (dist-only, gitignored; §6) is the runtime
  controller manifest → remove-at-closeout.
- `emit = "typescript"`, `out = "dist"` → confirms generated `.ts`/`.js`/json
  under `dist/` are faber product-recipe output (`ts_render.rs`,
  `assets.rs`), not authored artifacts.
- No `[product.shaders]` block → browser-app has no local shader source (unlike
  the triga demos); `[build] target = "ts"` confirms the current TS route.

### `triga/corpus/webgl-geometries/faber.toml`

```toml
[build]
target = "ts"
kind = "bin"

[product]
kind = "browser-app"
emit = "typescript"
out = "dist"
templates = "pages"
styles = "styles"
controllers_json = "controllers.json"

[product.shaders]
source = "src/shaders/test-data"

[dependencies]
web = "0.1.0"
triga = "0.1.0"
```

- `[product.shaders] source = "src/shaders/test-data"` → confirms the demo
  build consumes `src/shaders/test-data/kernel.wgsl` + `reflection.json`
  (gitignored copies of `triga-lit.*`; §6) as the faber shader build input.
- `web` + `triga` deps → both resolve as `target_language = "ts"`,
  `target_triple = "browser"` path deps (verified in both `faber.lock` files,
  regenerated by each demo's `tests/run.sh`), confirming the TS-route library
  surface behind `faber-web` and triga.
- The other four triga corpus demos carry the same `[product.shaders]` +
  `web`/`triga` block (verified `webgl-geometry-terrain`,
  `webgl-animation-orbit`, `webgl-animation-terrain`, `webgl-animation-water`).

## 5. Read-scope coverage notes

- `examples/web-canvas2d-smoke/` — read scope, but contains no `.ts/.tsx/.js/
  .json/.wgsl/.wasm/.wat` files (only `.fab`, `.html`, `.css`, `.mjs`) and is
  not in the validation command's root list; contributes 0 artifacts.
- `faber/src/package/product/` recipe emitters — cited as the **producer** of
  the generated TS-route artifacts (`ts_render.rs` renders the browser entry,
  `controllers.json`, tsconfig; `assets.rs` plans `assets.json`/product
  assets). Not artifacts themselves (Rust).
- `hosts/webgpu-browser/public/src/*.mjs` (18 authored proof/check scripts,
  e.g. `app-matmul.mjs`, `product-boundary-check.mjs`, the `*-check.mjs`
  family) and `generated/*.bin` (graphics payloads) are **excluded by the exact
  extension pattern** (`.mjs` ≠ `.js`; `.bin` is not a class). They remain
  reference proof code; the `.mjs` scripts are out of the counted classes by
  design and are not part of the JS boundary count.

## 6. Gitignored generated copies (present on disk, excluded from §1 counts)

`rg --files` honors `.gitignore`, so these per-build generated copies are not
in the exact-command count. They are part of the current on-disk route and are
listed for completeness with their own measured counts (`find`, 2026-08-09).

### Per-demo sync copies — `triga/corpus/<demo>/public/` (5 demos, each 10 `.js` + 1 `.wgsl` + 1 `.json`)

Producer: each demo's `tests/run.sh` copies `hosts/webgpu-browser/public/src/`
lanes (minus `app.js`) + `generated/triga-lit.wgsl`/`triga-lit-reflection.json`
into the demo `public/`. Owner: triga corpus (generated copies; single source
of truth = hosts). Runtime-required: yes (pages import these at
`../public/src/product/bootstrap.js`). Disposition: **remove-at-closeout** —
product budget: zero private per-demo renderer/runtime copies; the shared host
owns one copy.

Measured: 5 demos × (10 `.js` + 1 `.wgsl` + 1 `.json`) = **60 files**
(`public/src/backend/webgpu-runtime.js`, `…/contract/{artifact,capability}-admission.js`,
`…/engine/{engine,frame-scheduler,resource-manager,scene-extractor}.js`,
`…/presentation/{canvas,debug-overlay}.js`, `…/product/bootstrap.js`,
`public/triga-lit.wgsl`, `public/triga-lit-reflection.json` per demo).

### Per-demo shader build inputs — `triga/corpus/<demo>/src/shaders/test-data/` (5 demos × 2)

Producer: each demo's `tests/run.sh` copies the demo shader into
`kernel.wgsl` + `reflection.json` (the `[product.shaders]` build-input names).
Owner: triga corpus (generated build inputs). Runtime-required: yes at build
time (consumed by `faber build --package .` shader contract); served name is
the `public/` copy. Disposition: **keep** as build inputs while the TS-route
shader contract exists; the canonical owned artifact is the radix-generated
WGSL/reflection (Stage 3+ removes the local-copy need).

Measured: 5 demos × 2 = **10 files**.

### Generated product output — `examples/browser-app/dist/` (gitignored)

Producer: faber product recipe (`ts_render.rs` + `assets.rs` + `tsc`) on
`examples/browser-app`. Owner: faber product packaging (generated output).
Runtime-required: yes (this is what the current TS route serves).
Disposition: **remove-at-closeout** — generated TS, generated app JS, and
runtime JSON/TS config are all zero in promoted distributions per the product
budget; the WEB5 fixture remains as reference evidence only.

Measured (current build): **19 files**

- `dist/controllers.json` (runtime controller manifest) → remove-at-closeout
- `dist/assets.json`, `dist/product.json` (product/assets metadata) →
  remove-at-closeout (stable facts become build metadata)
- `dist/faber-esm/*.js` (7: `faber-browser.js`, `main.js`, `web-dom.js`,
  `web-shim-dom.js`, `web-canvas2d.js`, `web-shim-canvas2d.js`, `web-web.js`)
  → remove-at-closeout (generated app JS)
- `dist/faber-ts/*.ts` (7) + `dist/faber-ts/faber-web.d.ts` (1) →
  remove-at-closeout (generated TS)
- `dist/tsconfig.faber-browser.json` (generated TS config) →
  remove-at-closeout (no TypeScript configuration in output)

Same `dist/` output classes exist for the five triga demos after a successful
build (currently one demo fails the `tsc` gate — see U1 evidence §(d)); their
`dist/` dirs are likewise gitignored and dispositioned identically.

## 7. Validation commands run (this unit)

```sh
cd /Users/ianzepp/work/faberlang
rg --files examples/browser-app faber-web hosts/webgpu-browser/public triga/corpus \
  | grep -E '\.(ts|tsx|js|json|wgsl|wasm|wat)$'      # -> 29 files (run twice, identical)

# per-class recount (idempotent)
for ext in ts tsx js json wgsl wasm wat; do
  rg --files examples/browser-app faber-web hosts/webgpu-browser/public triga/corpus \
    | grep -E "\.$ext$" | wc -l
done   # -> 4 0 13 6 6 0 0

# gitignored generated-copy enumeration (supplementary, find-based)
find triga/corpus/webgl-*/public -name '*.js' | wc -l      # -> 50
find triga/corpus/webgl-*/public -name '*.wgsl' | wc -l    # -> 5
find triga/corpus/webgl-*/public -name '*.json' | wc -l    # -> 5
find triga/corpus/webgl-*/src/shaders -name '*.wgsl' | wc -l  # -> 5
find triga/corpus/webgl-*/src/shaders -name '*.json' | wc -l  # -> 5
find examples/browser-app/dist -name '*.js' | wc -l        # -> 7
find examples/browser-app/dist -name '*.ts' | wc -l        # -> 8 (7 .ts + faber-web.d.ts, matches suffix)
find examples/browser-app/dist -name '*.json' | wc -l      # -> 4
```

No cargo was invoked; no sibling-repo file was modified. Generated outputs
(`public/`, `src/shaders/`, `dist/`) existed on disk at capture and are
gitignored in every affected repo.
