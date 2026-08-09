# Triga Fixture Selection + Exact Browser Proof Commands (browser-wasm Stage 0, U6)

- **Unit**: `bwp-s0-u6-triga-fixture-selection`
- **Delivery**: `delivery-stage0.md` §5 U6 (delivery audit `914e3adb` ADMITTED; residuals folded `042f26c`; predecessor `042f26c`)
- **Campaign gate**: CAMPAIGN.md §Stage 0 — "`triga/corpus/webgl-geometries` is confirmed as the first vertical slice, followed by `webgl-geometry-terrain`, `webgl-animation-orbit`, and `webgl-animation-terrain`, with exact browser proof commands"
- **Author / date**: hand-8, 2026-08-09
- **Control-plane repo**: `faber` — evidence-only unit; zero product edits; sibling repos (triga, hosts, radix, faber-web) read-only
- **Sibling evidence consumed**: `artifact-inventory.md` (U0), `ts-browser-reference.md` (U1), `faber-wasm-package-baseline.md` (U2), `target-conflict-ledger.md` (U3), `host-js-allowlist.md` (U4), `async-abi-ledger.md` (U5)

## 1. Vertical-slice order (confirmed)

The CAMPAIGN §Stage 0 gate bullet is confirmed on live source. The selected
slice is the static-first, then animated/interactive progression:

| Order | Fixture (demo dir) | Campaign stage | Stage gate |
| --- | --- | --- | --- |
| 1 | `triga/corpus/webgl-geometries` | Stage 5 — Triga static WebGPU proof | first honest static scene |
| 2 | `triga/corpus/webgl-geometry-terrain` | Stage 5 — Triga static WebGPU proof | declared static geometry set |
| 3 | `triga/corpus/webgl-animation-orbit` | Stage 6 — animation/input/renderer policy | measured frames + input + pixels |
| 4 | `triga/corpus/webgl-animation-terrain` | Stage 6 — animation/input/renderer policy | measured frames + input + pixels |

`webgl-animation-water` (reference `webgl_gpgpu_water`) exists in the corpus
(`triga/corpus/README.md`) but is **excluded** from this selection: the
CAMPAIGN §Stage 0 gate names exactly the four fixtures above. It remains a
Stage 6 extension candidate, not part of the locked vertical slice.

Authority: `triga/corpus/README.md` (per-fixture reference + "What it
pressures" table) and `delivery-stage0.md` §5 U6 done_when.

## 2. Shared build/serve surface (all four fixtures)

Each demo is a self-contained `browser-app` package. `tests/run.sh` is the
single build entry (asset sync → `faber check` → `faber build --package .` →
dist contract greps). The corpus server is the default serve route; each demo
keeps a focused per-demo server for debugging.

- **FABER_BIN resolution** (each `tests/run.sh`): `${FABER:-}` → `$WORKSPACE/faber/target/debug/faber` → `${HOME}/.cache/faberlang-target/faber/debug/faber`; `WORKSPACE` = `$APP_DIR/../../..` (the faberlang container root).
- **Host asset sync**: `tests/run.sh` copies `hosts/webgpu-browser/public/src/{product,contract,engine,backend,presentation}` into the demo's generated `public/src/`, and the shader artifacts (`triga-lit.wgsl` + `triga-lit-reflection.json`) into `public/` and the `[product.shaders]` source dir. Single source of truth: `hosts/webgpu-browser` (corpus `_host/README.md` superseded-pointer note, DS-S2 extraction).
- **faber.lock**: regenerated per build; `web` and `triga` path deps with `target_language = "ts"`, `target_triple = "browser"` (current TS route).
- **Build assertion surface** (webgl-geometries / webgl-geometry-terrain): `dist/faber-esm/faber-browser.js`, `dist/controllers.json` + selector grep, `dist/public/src/product/bootstrap.js`, `dist/public/src/engine/engine.js`, `dist/public/src/backend/webgpu-runtime.js`, `dist/public/triga-lit.wgsl` + `triga-lit-reflection.json`; stale flat host names absent; node greps (`initEngine`, `requestAnimationFrame`, `.triga-canvas`, `.triga-facts`, `renderGreyboxSceneFrame`, `mountControllers`).
- **Build assertion surface** (webgl-animation-orbit / webgl-animation-terrain): same files plus WGSL entry-name greps (`triga_orbit_vertex`/`triga_orbit_fragment`, `triga_terrain_vertex`/`triga_terrain_fragment`), reflection contract (`schema_version == 1`, `target == "wgsl-text"`, single 36-byte interleaved vertex input, one vertex-buffer layout descriptor), and source greps (`dom.on_frame`, `data-animation-seconds`, `data-transform-payload` / `data-terrain-features`, shader `fbm`/`ridge_fbm`/`smoothstep`/`valley_fog`). These two demos adapt `hosts` `graphics-reflection.json` via their own `adapt-graphics-reflection.mjs` because the host generator publishes `graphics.*` names while the engine fetches `triga-lit.*`.

### 2.1 Current `[build]` / `[product]` block (identical across all four)

The current product route is the **TypeScript browser-app recipe** (`emit = "typescript"`, `tsc`-checked). This is the block Stage 3 replaces with the ordinary Wasm product recipe (no `tsc`, no generated TS); the `wasm` target row is absent from `manifest_build_target` today (see `faber-wasm-package-baseline.md` U2 and `target-conflict-ledger.md` U3).

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
```

## 3. Per-fixture rows

### 3.1 `webgl-geometries` — first static vertical slice

| Field | Value |
| --- | --- |
| **Demo dir** | `triga/corpus/webgl-geometries/` (package `triga-corpus-geometries`; entry `main.fab`) |
| **three.js reference** | `webgl_geometries` (three.js example; vendor pin `hosts/webgpu-browser/vendor/three@0.180` is presentation chrome only) |
| **What it pressures** | Every `triga:primitives` generator (plane, box, circle, sphere, cylinder, cone, torus — `triga:primitives/basic`), per-mesh colors, `BufferGeometry` → interleaved host payload (pos3+normal3+color3, 9 f32 per vertex), record shapes (`CorpusMesh`, `CorpusGeometryManifest`), `list<f32>`/`list<int<u32>>` payloads, option carriers (`∪ null`), `while` loops, `BufferAttribute.float32_values()` cross-module accessor |
| **Faber sources** | `src/shapes.fab`, `src/scene.fab`, `src/camera_controls.fab`, `src/main.fab` |
| **Current build block** | §2.1 (`target = "ts"`, `kind = "browser-app"`, `emit = "typescript"`, `out = "dist"`) |
| **Build command** | `cd triga/corpus/webgl-geometries && FABER_BIN=/Users/ianzepp/work/faberlang/faber/target/debug/faber ./tests/run.sh` |
| **Serve command** | `cd triga/corpus && ./serve.sh` → `http://127.0.0.1:8780/webgl-geometries/` (corpus server; per-demo `./serve.sh` port 8771 → `http://127.0.0.1:8771/pages/index.html`) |
| **Static host admission** | `cd hosts && ./scripta/webgpu-browser-proof check` |
| **Evidence oracle** | §4 static proof: fresh build, module provenance, device setup, upload, **non-background pixel readback**, expected geometry cases (8 mesh instances on a 4×2 grid), clean console diagnostics |
| **wasm-host-parity reachability risk** | §5 row 1: records (`CorpusMesh`/`CorpusGeometryManifest`), `list`/option carriers, `triga:primitives` generators, `math.vector3`, `float32_values` cross-module enum access (known library gap, corpus README §Library gaps) |

### 3.2 `webgl-geometry-terrain`

| Field | Value |
| --- | --- |
| **Demo dir** | `triga/corpus/webgl-geometry-terrain/` (package `triga-corpus-terrain`) |
| **three.js reference** | `webgl_geometry_terrain` |
| **What it pressures** | Procedural mesh generation at scale (48² heightfield, ~4.6k triangles), value noise (`_value_noise`), central-difference normals, elevation color ramp via `triga:math.color_interpolata`, water plane, `int<u32>` index arithmetic and casts, nested-loop `list` building |
| **Faber sources** | `src/terrain.fab`, `src/scene.fab`, `src/camera_controls.fab`, `src/main.fab` |
| **Current build block** | §2.1 |
| **Build command** | `cd triga/corpus/webgl-geometry-terrain && FABER_BIN=/Users/ianzepp/work/faberlang/faber/target/debug/faber ./tests/run.sh` |
| **Serve command** | `cd triga/corpus && ./serve.sh` → `http://127.0.0.1:8780/webgl-geometry-terrain/` (per-demo port 8772) |
| **Static host admission** | `cd hosts && ./scripta/webgpu-browser-proof check` |
| **Evidence oracle** | §4 static proof: non-background pixel readback, expected geometry cases (terrain mesh + water plane; heightfield vertex/index counts), clean console |
| **wasm-host-parity reachability risk** | §5 row 2: `int<u32>` casts (`conversio` family), nested-loop list mutation at scale, `functio` composition, `color_interpolata` |

### 3.3 `webgl-animation-orbit`

| Field | Value |
| --- | --- |
| **Demo dir** | `triga/corpus/webgl-animation-orbit/` (package `triga-corpus-animation-orbit`) |
| **three.js reference** | `webgl_animation_multiple` + `webgl_geometry_terrain` |
| **What it pressures** | Delta-driven frame updates (`dom.on_frame` / `dom.FrameState`), automatic camera travel, rotating model transforms (`math.Euler.ad_quaternionem` → `math.Quaternion`, `math.matrix4_composita`), view-projection (`math.matrix4_perspectiva`, `math.matrix4_conspectus`), live transform readback (`math.TransformPayload`, 32-f32 / 128-byte model-then-VP payload), option-carrier error handling |
| **Faber sources** | `src/main.fab` (single module) + `assets/orbit.wgsl` (per-vertex color, `triga_orbit_vertex`/`triga_orbit_fragment`) |
| **Current build block** | §2.1 |
| **Build command** | `cd triga/corpus/webgl-animation-orbit && FABER_BIN=/Users/ianzepp/work/faberlang/faber/target/debug/faber ./tests/run.sh` |
| **Serve command** | `cd triga/corpus && ./serve.sh` → `http://127.0.0.1:8780/webgl-animation-orbit/` (per-demo port 8773) |
| **Static host admission** | `cd hosts && ./scripta/webgpu-browser-proof check` |
| **Evidence oracle** | §4 animated proof: **frame progress across measured frames**, **input response**, non-background pixel preservation, no app/renderer policy in JS |
| **wasm-host-parity reachability risk** | §5 row 3: matrices/vectors (`math.Matrix4`, `Euler`, `Quaternion`, `TransformPayload`), animation-frame callback exports, option carriers |

### 3.4 `webgl-animation-terrain`

| Field | Value |
| --- | --- |
| **Demo dir** | `triga/corpus/webgl-animation-terrain/` (package `triga-corpus-animation-terrain`) |
| **three.js reference** | `webgl_geometry_terrain` + `webgpu_tsl_procedural_terrain` |
| **What it pressures** | Animated heightmap terrain with fBM, ridged noise, terrace shaping, slope-aware detail, lighting, and valley fog — shader owns height/normal/color/fog (`assets/terrain.wgsl`); `dom.on_frame` frame updates; animated model + camera transforms (`math.matrix4_composita`, `math.matrix4_conspectus`); live transform readback; per-frame terrain feature metadata (`data-terrain-features`) |
| **Faber sources** | `src/main.fab` (single module) + `assets/terrain.wgsl` (`fbm`, `ridge_fbm`, `smoothstep`, `valley_fog`, `triga_terrain_vertex`/`triga_terrain_fragment`) |
| **Current build block** | §2.1 |
| **Build command** | `cd triga/corpus/webgl-animation-terrain && FABER_BIN=/Users/ianzepp/work/faberlang/faber/target/debug/faber ./tests/run.sh` |
| **Serve command** | `cd triga/corpus && ./serve.sh` → `http://127.0.0.1:8780/webgl-animation-terrain/` (per-demo port 8775) |
| **Static host admission** | `cd hosts && ./scripta/webgpu-browser-proof check` |
| **Evidence oracle** | §4 animated proof: frame progress, input response, non-background pixels, console; `data-terrain-features` reflects shader feature set |
| **wasm-host-parity reachability risk** | §5 row 4: shader program (WGSL assets — preserve WGSL; consume compiler-owned reflection facts per hello-voxel, do not duplicate), matrices/vectors, animation-frame callbacks |

## 4. Evidence oracle (per CAMPAIGN; auditor/operator-gated)

CAMPAIGN §Browser proof requires output/readback, animation progress,
interaction, and console diagnostics — "rather than a successful process exit
alone." The oracle families each Stage 5/6 proof must satisfy:

| Oracle family | Static proof (Stage 5, fixtures 1–2) | Animated proof (Stage 6, fixtures 3–4) |
| --- | --- | --- |
| **Non-background pixel readback** | non-background pixels across the declared geometry set after a fresh build | non-background pixels preserved across measured frames |
| **Frame progress** | n/a (static) | frames advance across a measured interval (e.g. `submittedFrameCount`-style counter / `data-frame` attribute moves) |
| **Input response** | n/a | declared input (arrows orbit, `W`/`S` dolly, `A`/`D` pan, `Q`/`E` view-axis pan) changes the observed state |
| **Console diagnostics** | clean console (no errors/traps; typed non-success kinds on unsupported environments, never a silent hang) | same, plus no app/renderer policy in JavaScript |

Per CAMPAIGN §Development Posture and delivery-stage0.md §6: **physical
browser runs, pixel readback, and frame-progress evidence are auditor /
operator gates. They are recorded as such here — they are NOT Hand claims.**
The Hand evidence for this unit is: source shape inventory, build/static
transcripts (below), and the exact commands + oracle each proof must satisfy.

Consumed, not duplicated: `triga/docs/factory/hello-voxel/goal-00-contract-map.md`
locks the first-draw contract (position/color/index/transform buffers; vertex
`float32x3` stride 12; `TransformPayload` = 32 f32 / 128 bytes, group 0
binding 0 read-only storage; vertex WGSL entry emits builtin position + color
varying). The corpus fixtures consume those same Triga source facts
(`triga/src/geometry.fab`, `triga/src/math.fab`); the browser host consumes
emitted reflection and must not parse WGSL or infer field names.

## 5. wasm-host-parity reachability risk (Stage 1 candidates)

Source: `radix/docs/factory/wasm-host-parity/baseline-gap-ledger.toml`
(generated by `crates/exempla/src/exempla_e2e/wasm_ledger.rs`; live ledger,
regenerated 2026-08-06/07). **Live counts (verified 2026-08-09 by
`grep '^outcome = ' baseline-gap-ledger.toml | sort | uniq -c`): 308 rows —
160 parity / 132 gap / 13 contract-reject / 3 n/a.** The ledger rows are
corpus-relative radix-corpus paths; the fixture shapes below are Triga-corpus
sources that exercise the same shape families, so the ledger families are the
reachability evidence for those shapes.

### 5.1 Ledger row-family splits (shape families the fixtures exercise)

| Ledger family | Rows | parity | gap | contract-reject | Notes |
| --- | --- | --- | --- | --- | --- |
| `operatores/*` | 17 | 6 | 11 | 0 | arithmetic/operator surface (payload math) |
| `conversio/*` | 18 | 7 | 11 | 0 | casts (`int<u32>`, f32) |
| `intrinseca/*` | 11 | 3 | 8 | 0 | builtins/accessors |
| `vector/*` | 9 | 4 | 5 | 0 | `builtins/cross/dot/elementwise/swizzle` gap — `wasm_emission_unsupported`, owner `wasm-encoding` |
| `tensor/*` | 12 | 9 | 1 | 2 | matrix/aggregate carriers (9 parity already) |
| `gpu-core-types/*` | 6 | 0 | 1 | 5 | `matrix-register` gap `wasm_emission_unsupported`; rejects owned by `frontend` |
| `lista/*` | 5 | 4 | 1 | 0 | list methods |
| `literalia/*` | 7 | 6 | 1 | 0 | literals |
| `tabula/*` | 2 | 1 | 1 | 0 | record/struct access |
| `functio/*` | 6 | 5 | 1 | 0 | function shapes |
| `scalar/*` | 3 | 3 | 0 | 0 | scalar carriers fully at parity |
| **LIVE LEDGER TOTAL** | **308** | **160** | **132** | **13** | + 3 n/a |

Gap rows carry `blocker = "wasm_emission_unsupported"` and `owner =
"wasm-encoding"` (or `wasm-host` for `output_mismatch`); contract-reject rows
carry `owner = "frontend"` with a named language-contract reason.

### 5.2 Per-fixture reachability risk

1. **webgl-geometries** — records + list/option carriers are the Stage 1 load:
   `tabula/*` (1 gap), `lista/*` (1 gap), `intrinseca/*` (8 gap — includes
   accessor shapes such as `float32_values`, the known cross-module enum-scope
   gap documented in `triga/corpus/README.md` §Library gaps). `vector3`
   construction maps onto `vector/*` (5 gap). Mid-risk: the interleaved 9-f32
   payload projection needs list/loop/append parity.
2. **webgl-geometry-terrain** — same record/vector base plus `int<u32>` index
   arithmetic and casts (`conversio/*` 11 gap), nested-loop mesh building, and
   `functio` composition (`functio/*` 1 gap). High risk: large `list` mutation
   loops at ~4.6k-triangle scale.
3. **webgl-animation-orbit** — matrices/vectors dominate: `tensor/*` (1 gap,
   2 contract-reject — e.g. `matrix-register` under `gpu-core-types` is a
   `wasm_emission_unsupported` gap), `vector/*` (5 gap). The `dom.on_frame`
   → `FrameState` callback export and `TransformPayload` (32-f32 model+VP)
   are Stage 1 callback-export / aggregate-carrier candidates. Option-carrier
   error handling (`∪ null` + `fac`/`cape`) is part of the Stage 1 option/error
   carrier family.
4. **webgl-animation-terrain** — same matrix/vector + callback risk as orbit,
   plus the shader program: WGSL stays the shader artifact (CAMPAIGN Decisions
   Locked), and per-fixture shaders are authored corpus assets (`assets/terrain.wgsl`)
   adapted into the host naming, not compiler fixtures. Stage 1 needs the
   matrix/vector and frame-callback carriers; shader-pipeline reflection is
   hello-voxel-owned (Goal 00 contract map), consumed, not duplicated here.

## 6. Validation (this unit, run 2026-08-09)

Declared validation from delivery-stage0.md §5 U6. Both commands were run
exactly as declared with the prebuilt `faber/target/debug/faber` binary (no
cargo invoked by this Hand; `webgpu-browser-proof check` runs its own
`cargo run -p radix` regeneration internally as the script's declared
behavior). **Both currently fail on the live tree for pre-existing reasons
independent of this evidence unit.** Transcripts below are captured verbatim.

### 6.1 `triga/corpus/webgl-geometries/tests/run.sh` — FAILED at build (tsc)

Command:
```sh
cd triga/corpus/webgl-geometries && \
  FABER_BIN=/Users/ianzepp/work/faberlang/faber/target/debug/faber ./tests/run.sh
```

Result: the four `faber check` gates pass (warnings only); the browser product
build fails at the TypeScript check (`tsc`) step of the current TS recipe.

```text
checking src/shapes.fab          → ok: …/src/shapes.fab   (LOCALE002 warnings only)
checking src/camera_controls.fab → ok: …/src/camera_controls.fab
checking src/scene.fab           → ok: …/src/scene.fab
checking src/main.fab            → ok: …/src/main.fab
building browser package
error: browser product TypeScript check failed:
  .dist.faber.tmp-*/faber-ts/camera_controls.ts(23,14): error TS2304: Cannot find name 'Vec3'.
  …(110,16), (114,22), (115,16): error TS2304: Cannot find name 'Vec3'.
  …(116,109): error TS2552: Cannot find name 'Object3D'. Did you mean 'Object'?
```

Interpretation: the live TS-route product build of the first selected fixture
generates `camera_controls.ts` referencing `Vec3` / `Object3D` type names with
no in-scope declaration. `Vec3` appears nowhere in the current TS surface
(`rg 'Vec3' triga/src faber-web` → no hits; the generated `dist/faber-ts/`
tree of a prior successful `webgl-animation-orbit` build also contains no
`Vec3`). `Object3D` lives in `triga/src/graph/object.fab` and `PerspectiveCamera`
in `triga/src/graph/camera.fab`, both `target_language = "ts"` deps of the
fixture. The faber binary (built 2026-08-09 17:53) postdates every triga
`src/` change (latest `2c2d01d` 2026-08-09 12:51, a polish/docs commit; last
code-affecting `src/` commit `dee26f5` 2026-08-09 02:00), so the emission is
the live binary's current behavior:
the TS route fails to type-check this fixture on the current tree. **This is
baseline evidence that the current TS browser-app recipe does not build the
first selected vertical slice** — it is exactly the drift Stage 0 measures,
and a residual for the TS-route owner (see §6.3). It does not change the
fixture selection.

### 6.2 `hosts/scripta/webgpu-browser-proof check` — FAILED at graphics regeneration

Command:
```sh
cd hosts && ./scripta/webgpu-browser-proof check
```

Result: compute artifact regeneration succeeds (`kernel.wgsl`, `reflection.json`
for `fixtures/add-one.fab`, warnings only); the graphics fixture regeneration
fails because current `radix emit` rejects
`triga/exempla/triga-hello-voxel-shaders.fab`.

```text
Running `target/debug/radix emit -t wgsl-text …/add-one.fab …`      → ok (WARN003 only)
Running `target/debug/radix emit --reflection -t wgsl-text …/add-one.fab …` → ok
Running `target/debug/radix emit -t wgsl-text triga/exempla/triga-hello-voxel-shaders.fab …`
error[PARSE030:expected_expression]: triga-hello-voxel-shaders.fab:194
  |  fn hello_voxel_vertex() → void {
error[PARSE030:expected_expression]: …:249   fn hello_voxel_fragment() → void {
error[PARSE030:expected_expression]: …:600   const list<int<u32>> indices ← […]
error[PARSE030:expected_expression]: …:908   const data.BufferGeometry cube ← data.BufferGeometry {…}
compilation failed
```

Interpretation: `faber check` accepts the same exempla file (`ok:`), so the
`emit` pipeline's parse surface is narrower than the checker's. The exempla
uses `→ void` and a `main { }` block, which the current grammar reserves for
`→ vacuum` (radix `EBNF.md` row: `vacuum` = "void"); the corpus fixtures
consistently use `→ vacuum` and pass `faber check`. The exempla is stale
relative to the current grammar on the `emit` path. This is a pre-existing
live-tree red on the hosts static-admission command, independent of this unit
— recorded honestly; repair is out of scope (no triga/radix/hosts edits).

### 6.3 Residuals (filed for routing; not fixed here)

1. **TS browser-app route cannot build `webgl-geometries` (and likely the
   other corpus fixtures) on the live tree** — `camera_controls.ts` generated
   with undefined `Vec3`/`Object3D` references → `tsc` fails. Owner: TS
   browser-app recipe / Stage 0 measurement (reconcile with `ts-browser-reference.md`
   U1 evidence if U1 also observed this). Default: keep the fixture as the
   selected slice; Stage 3 wasm recipe is the replacement path.
2. **`./scripta/webgpu-browser-proof check` is red on the live tree** —
   `triga/exempla/triga-hello-voxel-shaders.fab` (graphics fixture) rejected
   by `radix emit` (PARSE030; `→ void`/`main { }` vs current grammar).
   Owner: triga exempla upkeep or hosts check fixture selection; default:
   align the exempla to `→ vacuum` in a triga-owned change.
3. **`radix emit` rejects `fn`-declaration files generally** (reproduced on
   `webgl-geometries/src/shapes.fab` at PARSE030) while `faber check` accepts
   them — pipeline parse-surface divergence to note for Stage 1 (wasm target
   uses the faber/faber-wasm path, but the divergence should be reconciled).
   Owner: radix emit / parser surface.

## 7. Stage 1/2 handoff inputs (this unit's contribution)

- **Stage 1 shape list (refined by fixture selection)**: records/structs
  (`CorpusMesh`, `OrbitCamera`), `list<f32>` / `list<int<u32>>` carriers,
  option carriers (`∪ null`), matrices/vectors (`Vector3`, `Matrix4`, `Euler`,
  `Quaternion`, `TransformPayload`), casts (`int<u32>`), list append/iteration,
  animation-frame callback exports (`dom.on_frame` → `FrameState`).
- **Stage 2 admission input**: unchanged — U4 host allowlist + U5 async
  ledger + v1 runtime contract (per delivery §10).
