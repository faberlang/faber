# Host JavaScript Allowlist + Byte Baseline — Stage 0 U4

**Unit**: `bwp-s0-u4-host-js-allowlist` (delivery-stage0.md §U4, lines 198–210)
**Campaign**: BROWSER-WASM-PRODUCT Stage 0 — baseline, ownership, boundary lock
**Status**: delivered (evidence captured 2026-08-09)
**Hand**: hand-4 (tugboat, task `ecc712cb`)

## Capture environment

| Item | Value |
| --- | --- |
| workspace | `/Users/ianzepp/work/faberlang` (faber, faber-web, hosts, triga sibling repos) |
| faber tree HEAD at capture | `042f26c` (`docs(factory): fold auditor-5 P2 residuals into browser-wasm Stage 0 docs`) — the declared predecessor of this unit |
| capture date | 2026-08-09 |
| toolchain | `wc`/`rg`/`ls` only — **no cargo** |

This is the exact initial JavaScript allowlist for the browser route: every
authored host JS/TS file measured (`wc -c -l`) and classified against the
CAMPAIGN §JavaScript Boundary Budget. Per CAMPAIGN: "The semantic allowlist,
not an arbitrary total-size promise, is its first hard gate." This document is
**D4 of the delivery spec** and the **Stage 2 admission input** (delivery §10).

## 1. Engine-JS source of truth

The authoritative engine JavaScript lives in
`hosts/webgpu-browser/public/src/` — the shared host repo lane.

`triga/corpus/_host/public/` **does not exist** and has never held engine JS.
`triga/corpus/_host/` contains only a superseded-pointer `README.md`
(1 126 B; one-commit fork `806aa21`, 2026-07-31, extracted via DS-S2 decision
(b) — engine-runtime home). Its text: "This directory previously held the
shared greybox WebGPU host assets … has been **extracted into the sibling host
repo**"; "Engine JS: `$WORKSPACE/hosts/webgpu-browser/public/src/`"; "The demos
copy from the hosts repo via `tests/run.sh`
(`HOST_DIR="$WORKSPACE/hosts/webgpu-browser"`)".

So the measured tree below is the only authored host JS surface. Per-demo
`public/` copies (generated, §5) are copies of it, not a second source.

## 2. Measurement method — exact validation command

```sh
cd /Users/ianzepp/work/faberlang
wc -c -l hosts/webgpu-browser/public/src/**/*.js
```

`wc -c` = bytes, `wc -l` = lines (column order as printed: lines, bytes). No
`triga/corpus/_host/public/**/*.js` term — that directory does not exist (see
the `_host/README.md` pointer note above).

### Measured — all 11 authored host JS files (shared host)

| # | Path | Bytes | Lines | Budget class | Rationale vs CAMPAIGN §JavaScript Boundary Budget |
| --- | --- | --- | --- | --- | --- |
| 1 | `src/backend/webgpu-runtime.js` | 94 527 | 2 699 | **not-allowed today** — shrink-to-thunks | WebGPU engine lanes: raw device/buffer/pipeline/encoder/queue ops are an allowed thunk family, but as written it embeds renderer policy (render-pass encoding, draw submission, resource management, `renderPass.drawIndexed` orchestration at runtime.js:1503–1550) that must move to Wasm. CAMPAIGN: "render-pass selection, draw ordering, or resource lifetime policy" not allowed; draw/dispatch selection arrives as compiled submission-region descriptors. |
| 2 | `src/engine/engine.js` | 36 431 | 1 096 | **not-allowed today** — shrink-to-thunks | Renderer/session facade: session creation, scene mount, render-pass selection, `renderGreyboxSceneFrame` (engine.js:272) — reference JS renderer policy. CAMPAIGN: policy in Wasm; JS keeps only raw capability thunks + lifecycle callbacks. |
| 3 | `src/engine/frame-scheduler.js` | 6 940 | 205 | **not-allowed today** — shrink-to-thunks | rAF callback *transport* is an allowed thunk (translate animation-frame callbacks into versioned Wasm calls), but loop-policy decisions (frame pacing, readback phases) are application/control policy → Wasm. |
| 4 | `src/engine/resource-manager.js` | 8 958 | 262 | **not-allowed today** — shrink-to-thunks | Resource-lifetime policy is explicitly not allowed in JS; only raw create/upload/buffer-copy thunks may survive. |
| 5 | `src/engine/scene-extractor.js` | 8 685 | 235 | **not-allowed today** — shrink-to-thunks / relocate | Source-semantics recovery: parses reflection + DOM scene facts into render items — "parsing Faber output to recover compiler facts" is not allowed. Extraction moves to Wasm; the whole lane must shrink or relocate. |
| 6 | `src/contract/artifact-admission.js` | 28 209 | 726 | **allowed-thunk** — keep | Fetches the declared WGSL/reflection/draw artifacts and loads modules/pipelines: the CAMPAIGN allowed "fetch and instantiate the declared Wasm module set" + "declared asset fetch". Consumes the kept reflection facts (U0: reflection is an owned compiler build fact). |
| 7 | `src/contract/capability-admission.js` | 6 982 | 183 | **allowed-thunk** — keep | Fail-closed device-capability admission gate (typed `CapabilityAdmissionError`); matches the CAMPAIGN "fail closed" posture and the allowed "report … device loss and browser errors through the declared contract". |
| 8 | `src/presentation/canvas.js` | 2 364 | 83 | **allowed-thunk** — keep | Raw DOM/canvas/WebGPU surface lookup, sizing, context: "raw DOM, canvas, WebGPU adapter/device … operations". |
| 9 | `src/presentation/debug-overlay.js` | 2 608 | 77 | **allowed-thunk** — keep | Raw DOM transport (`.triga-facts` facts channel to controllers): DOM callbacks translate through the declared contract. |
| 10 | `src/product/bootstrap.js` | 4 304 | 120 | **allowed-thunk** — keep | Per-application loader/adapter + page entry + DOM bridge; 4.3 KiB unminified — inside the ≤ 16 KiB per-app loader/adapter product budget. Session *policy* moves to Wasm, but the file as written is the loader/adapter class. |
| 11 | `src/app.js` | 8 176 | 252 | **not-allowed** — remove-at-closeout | Reference proof harness (compute + graphics proofs via three.js): app-level policy with no place in the shared host. CAMPAIGN track: browser WebGPU host shrinks to generic raw capability thunks; the Wasm route replaces this page at the accepted gate. |
| **Total (shared host)** | **208 184** | **5 938** | | 5 allowed-thunk (keep) + 5 not-allowed shrink/relocate + 1 not-allowed remove | |

### Per-class subtotals (shared host)

| Class | Files | Bytes | Lines | Share of host bytes |
| --- | --- | --- | --- | --- |
| allowed-thunk (keep) | 5 (`artifact-admission`, `capability-admission`, `canvas`, `debug-overlay`, `bootstrap`) | 44 467 | 1 189 | 21.4 % |
| not-allowed today — shrink-to-thunks / relocate | 5 (`webgpu-runtime`, `engine`, `frame-scheduler`, `resource-manager`, `scene-extractor`) | 155 541 | 4 497 | 74.7 % |
| not-allowed — remove-at-closeout | 1 (`app.js`) | 8 176 | 252 | 3.9 % |
| **Shared host total** | **11** | **208 184** | **5 938** | 100 % |

The 11-module / 208 184-byte (203.3 KiB) shared generic browser host above is
the "current bytes and modules" baseline the CAMPAIGN requires Stage 0 to
record. The semantic allowlist that survives is the 5-file allowed-thunk set
(44 467 B / 1 189 lines); the other 6 files are reference JS policy that the
allowlist must shrink (to generic thunks) or relocate (to Wasm), or remove at
closeout. Later stages may **reduce** the allowlist; expanding it requires an
explicit campaign decision.

## 3. The Stage 0 hard gate — exact initial allowlist

The exact initial allowlist = the allowed-thunk rows above, frozen as the
Stage 0 gate (CAMPAIGN: "freeze the initial JavaScript allowlist"; delivery
U4: "the exact initial allowlist = the Stage 0 hard gate"):

```text
hosts/webgpu-browser/public/src/contract/artifact-admission.js    28 209 B  726 lines
hosts/webgpu-browser/public/src/contract/capability-admission.js   6 982 B  183 lines
hosts/webgpu-browser/public/src/presentation/canvas.js             2 364 B   83 lines
hosts/webgpu-browser/public/src/presentation/debug-overlay.js      2 608 B   77 lines
hosts/webgpu-browser/public/src/product/bootstrap.js               4 304 B  120 lines
```

Keep: module/artifact loading + declared asset fetch + fail-closed admission
(`contract/*`); raw DOM/canvas/WebGPU surface + facts transport
(`presentation/*`); per-app loader/adapter within the 16 KiB product budget
(`product/bootstrap.js`). Everything else currently in the shared host is
**not-allowed reference JS policy** that the allowlist must shrink or relocate.

## 4. Reference-policy callout (must shrink or relocate)

Today's `engine/` lanes plus `scene-extractor.js` and
`renderGreyboxSceneFrame` are reference JS policy — precisely the classes the
CAMPAIGN forbids in JavaScript:

- `src/engine/*` (5 files, 155 541 B / 4 497 lines): renderer/session policy,
  render-pass selection, draw ordering, frame/loop policy, resource-lifetime
  policy — all "not allowed" bullets in the budget; the engine-JS track must
  shrink to generic raw capability thunks (Stage 2) with policy moving to Wasm
  (Stages 5–6).
- `renderGreyboxSceneFrame` — `src/engine/engine.js:272` (scene render-pipeline
  selection, clear color, draw orchestration): reference renderer policy;
  becomes a compiled submission-region-descriptor consumer, not a JS scene
  renderer.
- `src/engine/scene-extractor.js` (235 lines): source-semantics recovery
  (parses reflection + DOM scene facts into render items) — explicitly not
  allowed; extraction relocates to Wasm.
- `src/backend/webgpu-runtime.js` (94 527 B — the single largest host file):
  raw capability ops stay as thunks; embedded render-pass/draw/resource
  orchestration moves out.

Clean break at Stage 8 retires the removed surface (`app.js`, three.js vendor,
per-demo copies); **no host JS is deleted in Stage 0** (unit non-goal).

## 5. Generated copies — same route, not authored, not in the shared-host count

| Surface | Files | Producer | Disposition |
| --- | --- | --- | --- |
| `triga/corpus/<demo>/public/src/…` (5 demos × 10 `.js` = 50 files) | 50 | each demo's `tests/run.sh` syncs `hosts/webgpu-browser/public/src/` lanes (minus `app.js`) into the demo `public/` (`HOST_DIR=$WORKSPACE/hosts/webgpu-browser`) | **remove-at-closeout** — product budget: zero private per-demo renderer/runtime copies; one shared host copy |
| `hosts/webgpu-browser/public/src/*.mjs` (18 scripts, 275 268 B / 7 329 lines) | 18 | authored proof/check scripts (`*-check.mjs`, `product-boundary-check.mjs`, …) | **excluded by the exact extension pattern** (`.mjs` ≠ `.js`, per U0 inventory §5) — reference proof code, not part of the JS boundary count |

Verified 2026-08-09: each of the five triga corpus demos
(`webgl-geometries`, `webgl-geometry-terrain`, `webgl-animation-orbit`,
`webgl-animation-terrain`, `webgl-animation-water`) has 10 `.js` files under
its gitignored `public/`. These are generated copies of §2 rows; the single
source of truth stays `hosts/webgpu-browser/public/src/`.

## 6. TS side of the same budget — faber-web runtime shims

The CAMPAIGN product budget targets zero TS runtime in promoted
distributions; today the browser route compiles these authored shims into
every ESM build (U0 inventory §2 `.ts` rows):

| Path | Bytes | Lines | Budget class | Rationale |
| --- | --- | --- | --- | --- |
| `faber-web/runtime/dom.ts` | 13 014 | 412 | keep (re-express Stage 4) | Scoped DOM runtime shim compiled into `web-dom.js`/`web-shim-dom.js`; re-expressed as target-neutral Faber contracts + Wasm binding map (CAMPAIGN: no required TypeScript runtime). |
| `faber-web/runtime/canvas2d.ts` | 5 969 | 218 | keep (re-express Stage 4) | Canvas2d binding shim compiled into `web-canvas2d.js`; same Stage 4 re-express as `dom.ts`. |
| **Total (TS shims)** | **18 983** | **630** | | |

TS tests (`faber-web/tests/*.ts`) are authored test evidence, not runtime;
generated TS under `dist/faber-ts/` is build output (U0 §6, remove-at-closeout).

## 7. Validation commands run (this unit)

```sh
cd /Users/ianzepp/work/faberlang
wc -c -l hosts/webgpu-browser/public/src/**/*.js      # 11 files, 208 184 B / 5 938 lines total
wc -c -l faber-web/runtime/*.ts                        # 2 shims, 18 983 B / 630 lines
rg --files hosts/webgpu-browser/public/src | grep -E '\.(js|ts)$'   # 11 .js, 0 .ts — matches the tree above
ls triga/corpus/_host/                                  # README.md only — no public/ (pointer note confirmed)
find triga/corpus/<demo>/public -name '*.js' | wc -l    # 10 per demo × 5 demos = 50 generated copies
```

Cross-check vs the U0 inventory (`evidence/artifact-inventory.md`): U0 §2
Class `.js` counts the same 11 `public/src` files + 2 vendored
`vendor/three@0.180/*` (U0: remove-at-closeout — zero Three.js runtime
dependency; not measured here because the shared-host count is the authored
`public/src` surface); per-file line counts match U0's rows
(`webgpu-runtime.js` 2 699, `engine.js` 1 096, `artifact-admission.js` 726,
`scene-extractor.js` 235, `frame-scheduler.js` 205, `resource-manager.js`
262, `bootstrap.js` 120, `app.js` 252, `capability-admission.js` 183,
`canvas.js` 83, `debug-overlay.js` 77). U0 §6's 50 per-demo `.js` copies and
the 18 `.mjs` exclusion both agree with §5 above. U0 disposition vocabulary
(`shrink-to-thunks`, `remove-at-closeout`, `keep`) is preserved here; this
unit adds the measured byte/line baseline and the budget-class per row.

## 8. Caveats and non-goals

- **No host edits, no new host JS, no deletion of reference JS** in Stage 0
  (unit non-goal; clean break is Stage 8, operator-gated).
- Physical-browser WebGPU observation is **not** this unit — it is the
  auditor/operator surface (delivery §6).
- Byte totals are `wc -c` bytes (unminified, as checked in); the ≤ 16 KiB
  product-budget row applies to the per-app loader/adapter
  (`bootstrap.js`, 4 304 B), not to the shared host as a whole — the
  CAMPAIGN measures the shared generic browser host separately.
- `hosts` repo shows an unrelated dirty `Cargo.lock` at capture; it is
  outside this unit's read/write scope and was not touched.
