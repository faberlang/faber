# Stage 0 Gate Closeout + Boundary Review — BROWSER-WASM-PRODUCT (U7)

**Unit**: `bwp-s0-u7-stage0-closeout` (delivery-stage0.md §5 U7, lines 243–253)
**Campaign**: BROWSER-WASM-PRODUCT Stage 0 — baseline, ownership, boundary lock
**Status**: delivered (closeout 2026-08-09)
**Hand**: hand-10 (tugboat, task `8888423f`)
**Predecessors**: U0 `42f9604` · U1 `68c32df` · U2 `82bc3b9` · U3 `840fa0a` · U4 `e5764f6` · U5 `3328ede` · U6 `fdfb76f` — **ALL LANDED** (verified on faber `main`; git log on `docs/factory/browser-wasm-product/`)
**Delivery audit**: `914e3adb` ADMITTED; residuals folded `042f26c`
**Read scope**: all U0–U6 evidence under `evidence/`; CAMPAIGN.md §Stage 0 gate; goal.md §Open Questions; delivery-stage0.md

This document is **D7 of the delivery spec** — the Stage 0 done-oracle: every
CAMPAIGN §Stage 0 gate bullet is satisfied by a checked-in artifact with a
named file, the boundary review records that no unresolved question changes the
Stage 1 (Wasm reachability) or Stage 2 (browser host admission) boundary, and
the Stage 0 → 1 handoff (Stage 1 shape list + Stage 2 input packet) is explicit.

---

## 1. Gate-checklist — CAMPAIGN §Stage 0 gate bullet → evidence

CAMPAIGN.md §Stage 0 gate (lines 216–233), all eight bullets, each mapped to
its evidence file and row. Verdict per row: **SATISFIED**.

| # | CAMPAIGN §Stage 0 gate bullet (verbatim) | Evidence file | Evidence row / section |
| --- | --- | --- | --- |
| 1 | "one checked-in inventory names artifact, producer, consumer, owner, whether it is runtime-required, and intended keep/move/remove disposition" | `evidence/artifact-inventory.md` (U0) | §1 measured per-class counts; §2 class tables — every artifact row carries the five named fields (producer, consumer, owner, runtime-required, disposition); §3 disposition-consistency table vs CAMPAIGN budgets; §4 product-block cross-check; §6 gitignored generated copies |
| 2 | "current TypeScript browser behavior is recorded as reference evidence" | `evidence/ts-browser-reference.md` (U1) | §(a) `faber build --package .` transcript + `dist/` layout on `examples/browser-app`; §(b) `tsc --project` invocation (source `ts_render.rs:217–245`, captured via shadow wrapper); §(c) `controllers.json` fields (version/controllers; selector/module/export rows); §(d) `triga/corpus/webgl-geometries/tests/run.sh` outcome (check ok, build fails at tsc gate — reference red); §(e) `fetch_text` TS async gap + WEB5 fixture scope with source paths |
| 3 | "the Faber baseline records that the current package builder persists `.wat` while retaining `.wasm` bytes in memory, and Stage 3 names the durable binary module-set and manifest output that replaces this limitation" | `evidence/faber-wasm-package-baseline.md` (U2) | §(a) live CLI probe — two `.wat`, zero `.wasm`, in-memory `entry_bytes`/`sibling_bytes`, reserved `binary_path`; §(b) `manifest_build_target` no-`"wasm"` arm; §(c) builder + host round-trip test coverage; §(d) gap list item 1 ("No persisted binary module set") + **"Stage 3 durable output"** paragraph naming the deterministic binary module-set + manifest as the replacement |
| 4 | "every target capability conflict is named with live authority" | `evidence/target-conflict-ledger.md` (U3) | §1 Rows 1–4, each with live authority (file:line + observed command) and proposed reconciliation owner: (1) `faber targets`/radix `wasm` row `run=no package=no` vs live package-wasm build; (2) `manifest_build_target` rejects `[build] target = "wasm"` while CLI `--target wasm` works; (3) `target-capability-matrix.md` wasm deferral (reopen event = this campaign); (4) `faber-web/README.md` "HIR → TypeScript emit" architecture law supersession. §2 validation transcript captured |
| 5 | "the exact host JavaScript file allowlist and line/byte baseline are recorded" | `evidence/host-js-allowlist.md` (U4) | §2 measurement command + 11-file table (path / bytes / lines / budget class / rationale); per-class subtotals (shared host 208 184 B / 5 938 lines; allowed-thunk set 44 467 B / 1 189 lines); §3 the exact initial allowlist (5 files) frozen as the Stage 0 hard gate; §5 generated copies; §6 TS shims (same budget, TS side) |
| 6 | "the async ABI ledger names operation identifiers, dispatcher export, status and payload records, cancellation race behavior, non-reentry rule, ordering, device-loss delivery, and which future-valued routes are admitted or deferred" | `evidence/async-abi-ledger.md` (U5) | §2 rows R1–R11: R1 operation-id allocation · R2 dispatcher export (provisional `__faber_rt_v1_async_dispatch`) · R3 status codes (completed/failed/cancelled/device-lost) · R4 payload record (status-first `(i32 status, payload…)`) · R5 queue-after-import-returns · R6 serialized non-reentrant delivery · R7 exactly-one-terminal-result · R8 best-effort cancellation + late-result discard · R9 browser delivery order preserved, no fabricated total order · R10 device-loss + unsolicited events · R11 future-valued routes (fetch_text deferred/not-admitted); §3 traceability table maps every row to its CAMPAIGN §Async host law bullet |
| 7 | "`triga/corpus/webgl-geometries` is confirmed as the first vertical slice, followed by `webgl-geometry-terrain`, `webgl-animation-orbit`, and `webgl-animation-terrain`, with exact browser proof commands" | `evidence/triga-fixture-selection.md` (U6) | §1 vertical-slice order table (order 1–4, campaign stage, stage gate; `webgl-animation-water` explicitly excluded); §2 shared build/serve surface (`tests/run.sh` entry, serve routes, `faber.lock` TS deps); §3.1–3.4 per-fixture rows (demo dir, three.js reference, what it pressures, Faber sources, build/serve commands, static host admission, evidence oracle, reachability risk); §4 evidence oracle (non-background pixel readback / frame progress / input response / console diagnostics) — recorded as auditor/operator gates, not Hand claims |
| 8 | "no unresolved question changes the Stage 1 compiler or Stage 2 host boundary" | `stage0-closeout.md` (U7, this file) | §2 boundary review (no boundary-changing open question); §3 open questions all non-boundary-changing, each with owner + default; §4 Stage 1 shape list; §5 Stage 2 input packet |

**Done-oracle verdict:** every §Stage 0 gate bullet maps to a checked-in
artifact with a named file and row. No bullet is satisfied by campaign prose
alone; each citation is the U0–U6 evidence captured 2026-08-09. Stage 0 is
**complete** as an evidence slice.

---

## 2. Boundary review

### 2.1 No unresolved question changes the Stage 1/2 boundary (gate bullet 8)

The four open questions recorded in §3 are each owned by a later stage or the
operator and each has a default. Re-checked against the two boundaries they
must not move:

- **Stage 1 boundary (MIR → core Wasm reachability, radix-owned).** Nothing
  in the U0–U6 evidence admits a new compiler target, an HIR-direct Wasm
  backend, or a source-replay/TS-fallback path. The Stage 1 shape list (§4)
  is exactly the "expected families … Stage 0 evidence decides the exact set"
  of CAMPAIGN §Stage 1, decided by the U6 fixture selection and the U5 closed
  `__faber_rt_v1_*` surface. The `[provisional]` async-dispatcher spelling
  (open question Q2) is a Stage 2 naming detail over an already-locked
  contract shape (U5 R1–R11), not a compiler boundary change.
- **Stage 2 boundary (browser core-Wasm host admission, `hosts/webgpu-browser`).**
  The Stage 2 input packet (§5) is closed: U4 allowlist + U5 async ledger + v1
  runtime contract + module-set linking contract. The host remains inside the
  Stage 0 allowlist, owns no app/controller/Triga policy, and the async
  contract is the single typed dispatcher per U5. The binding/reflection
  encoding question (Q1) is decided *before Stage 3*, after Stage 0 measured
  consumers (U0/U4); it does not alter what the Stage 2 host loads or how it
  reports, so the Stage 2 admission slice is unchanged.

### 2.2 Known-red surfaces (U6-recorded) — stage ownership statement

Two live-tree red surfaces were captured honestly in U6 evidence. Neither
changes the Stage 1 compiler or Stage 2 host boundary; each is owned by
**Stage 3/8 product work** (or a reference-surface owner feeding those stages):

1. **TS-route `tsc` build break on `webgl-geometries`** — the current
   TypeScript browser-app recipe generates `camera_controls.ts` with
   undeclared `Vec3`/`Object3D` names, so `tsc --strict` rejects the build
   (`evidence/triga-fixture-selection.md` §6.1; `evidence/ts-browser-reference.md`
   §(d)). **Owned by the Stage 3/8 product recipe work**: the Wasm browser-app
   recipe is the replacement path (CAMPAIGN §Stage 3), and the TypeScript
   recipe is retired at the Stage 8 clean break. The red is baseline evidence
   of the drift Stage 0 measures; it does not alter the fixture selection
   (fixture stays the first vertical slice) and does not touch Stage 1/2.
2. **`radix emit` PARSE030 on stale `triga-hello-voxel-shaders.fab`** — the
   hosts static-admission command `./scripta/webgpu-browser-proof check` fails
   at graphics regeneration because the exempla uses `→ void` / `main { }`
   against a grammar that reserves `vacuum`; `faber check` accepts the same
   file (`evidence/triga-fixture-selection.md` §6.2). **Owned by triga
   exempla upkeep / hosts check-fixture selection** (U6 §6.3 residual 2), which
   feeds the Stage 2 static-admission surface and the Stage 5 Triga proof;
   repair is a reference-surface/product-stage change, not a Stage 1/2
   boundary changer. The companion emit-vs-check parse-surface divergence
   (U6 §6.3 residual 3) is carried as a Stage 1 note (§4) because the wasm
   target uses the faber/faber-wasm path — it does not block Stage 1.

No other red surfaces were recorded in U0–U6; the one blocked faber-side cargo
test in U2 (faber cannot compile against radix `Target::MirAmd` until
`artifact_plan.rs`/`postprocess.rs` matches land) is a **build-environment
residual**, not a Stage 0/1/2 boundary question, and is tracked in
`evidence/faber-wasm-package-baseline.md` §Cross-references and residuals.

---

## 3. Open questions (non-blocking; none change the Stage 1/2 boundary)

Each question is carried from goal.md §Open Questions / delivery-stage0.md §9
/ CAMPAIGN §Open Decisions, with its owner and default. Only questions that
cannot change the Stage 1 compiler or Stage 2 host boundary are listed.

| # | Question | Owner | Default | Boundary check |
| --- | --- | --- | --- | --- |
| Q1 | Stable binding/reflection table encoding: wasm data exports vs custom section vs compact sidecar | Stage 3 delivery (measured consumers at Stage 0 — U0 §2/U4 §2; decided before Stage 3 per goal.md §Open Questions) | Keep current generated JSON as reference evidence; no new encoding chosen in Stage 0; reflection facts stay owned compiler build facts | Chosen after Stage 2 admission; does not change what Stage 1 lowers or Stage 2 loads |
| Q2 | Async dispatcher export spelling (`__faber_rt_v1_async_dispatch` provisional) | Stage 2 host contract (recorded in U5 R2 with explicit `[provisional]` marker) | The U5 R1–R11 shape is locked; only the export *name* is provisional, following the `__faber_rt_v1_*` convention | Naming detail over a locked contract shape; no Stage 1/2 semantic change |
| Q3 | Whether Stage 6 measurements justify a coarser render-command batch surface | Stage 6 (measurement decision, per CAMPAIGN §Open Decisions) | No — submission-region boundary as locked in DDPP0 §PreparedRegion; batch surface added only from measured boundary traffic | Post-Stage-5 measurement; Stage 2 admission uses the submission-region boundary as-is |
| Q4 | Faber version and migration window at Stage 8 | Operator (release checkpoint per goal.md §Release Posture) | Defer-release; operator decision at the Stage 8 checkpoint after product acceptance | Release-only; no effect on Stage 1/2 architecture |

---

## 4. Stage 0 → 1 handoff — Stage 1 concrete shape list

Stage 1 (Browser-minimum MIR-to-Wasm reachability) lowers the shape families
CAMPAIGN §Stage 1 names — package calls, records and lists, option/error
carriers, matrices or vectors, callable identity, callback exports — with the
exact set now **decided by the U6 fixture selection and U5 closed surface**:

| # | Shape family (CAMPAIGN §Stage 1) | Concrete carriers (refined by U6) | Reachability evidence (U6 §5; U2/U5) |
| --- | --- | --- | --- |
| S1 | Package calls | Cross-module package calls: sibling import (`importa ex "./auxilium"`), canonical `__faber_external_*` sibling export, `incipit` entry; multi-unit module set linking | **Already live** (U2 §(a)/(c): two-unit probe + `WasmRtV1Host::run_package` round trip, 41→42); no new shape needed, only the broader package graphs Stage 1 proofs exercise |
| S2 | Records and lists | Records/structs `CorpusMesh`, `CorpusGeometryManifest`, `OrbitCamera`, `FrameState`; `list<f32>` / `list<int<u32>>` carriers; list append/iteration at scale (interleaved 9-f32 vertex payload projection; nested-loop mesh building) | `tabula/*` 1 gap, `lista/*` 1 gap, `intrinseca/*` 8 gap (incl. `float32_values` cross-module enum-scope accessor — U6 §5.2 row 1); high-risk: large `list` mutation loops at ~4.6k-triangle scale (U6 §5.2 row 2) |
| S3 | Option/error carriers | `∪ null` option carriers; `fac { } cape err { }` error handling; `⇥`/`*_or` recovery (`FetchResponse ⇥ textus`); P10 `ReturnError` carrier | U5 R3/R4/R8 grounding (`_or` recovery family, `lib.rs:199-210`; status-first model `lib.rs:505-514`); U6 §5.2 rows 3–4 (option-carrier error handling in orbit/terrain) |
| S4 | Matrices and vectors | `Vector3`, `Matrix4`, `Euler`, `Quaternion`, `TransformPayload` (32 f32 / 128-byte model-then-VP); matrix/vector math ops; `int<u32>` casts (`conversio` family) | `vector/*` 5 gap, `tensor/*` 1 gap + 2 contract-reject, `gpu-core-types/*` `matrix-register` gap `wasm_emission_unsupported` (U6 §5.1); `conversio/*` 11 gap (U6 §5.2 row 2) |
| S5 | Callable identity | `functio` composition; callable identity of cross-module accessors (`float32_values` enum-scope identity — the known library gap, `triga/corpus/README.md` §Library gaps) | `functio/*` 1 gap, `intrinseca/*` 8 gap (U6 §5.1/5.2) |
| S6 | Callback exports | Animation-frame callback exports `dom.on_frame → FrameState`; browser lifecycle callbacks (Stage 2 forwards callbacks); callback identity as first-class export | U6 §5.2 rows 3–4 (orbit/terrain frame callbacks + `TransformPayload` are Stage 1 callback-export / aggregate-carrier candidates) |

**Stage 1 notes (carried from U6):**

- **Emit-vs-check parse-surface divergence** (U6 §6.3 residual 3): `radix emit`
  rejects `fn`-declaration files (PARSE030) while `faber check` accepts them.
  The wasm target uses the faber/faber-wasm path, so this does not block Stage
  1, but the divergence should be reconciled as a separate radix-emit surface
  item.
- **Contract-reject rows** carry `owner = "frontend"` with named language
  reasons; gap rows carry `blocker = "wasm_emission_unsupported"`,
  `owner = "wasm-encoding"` (or `wasm-host` for `output_mismatch`) — the Stage
  1 delivery must route each admitted shape against these ledger rows
  (308-row ledger, `radix/docs/factory/wasm-host-parity/baseline-gap-ledger.toml`).
- **Shared MIR is single-writer**; wasm-encoding stays radix-owned (CAMPAIGN
  §Stage 1 Overlap).

---

## 5. Stage 2 admission input packet

Stage 2 (Browser core-Wasm host admission, owner `hosts/webgpu-browser`)
consumes the following closed inputs, per delivery-stage0.md §10:

1. **Host JS allowlist** — `evidence/host-js-allowlist.md` (U4): the exact
   initial allowlist (5 files, 44 467 B / 1 189 lines: `contract/artifact-admission.js`,
   `contract/capability-admission.js`, `presentation/canvas.js`,
   `presentation/debug-overlay.js`, `product/bootstrap.js`), frozen as the
   Stage 0 hard gate. Host code in Stage 2 must stay inside this allowlist and
   contain no app policy. The 6 non-allowed reference-policy lanes
   (`webgpu-runtime.js`, `engine/*`, `scene-extractor.js`) and `app.js` are
   shrink/relocate/remove targets — Stage 2 must not grow them.
2. **Async ABI ledger** — `evidence/async-abi-ledger.md` (U5): the R1–R11
   async contract (op-id allocation, provisional dispatcher export
   `__faber_rt_v1_async_dispatch`, four terminal status codes, status-first
   payload record, queue-after-import-returns, serialized non-reentrant
   delivery, exactly-one-terminal-result, best-effort cancellation with
   late-result discard, browser delivery order preserved, device-loss through
   the same typed path, `fetch_text` deferred/not-admitted). Stage 2 admits
   the dispatcher name (Q2) and implements the contract over the closed
   surface.
3. **v1 runtime contract** — `radix/crates/radix-host-abi/src/lib.rs`
   (`__faber_rt_v1_*` closed `SYMBOL_ROWS`, `ABI_SURFACE_NAME = "faber-rt-v1"`,
   `ABI_VERSION = 1`; status-first multi-value model; P6/P7/P10 recovery and
   genus tables). No new ABI dialect; unknown imports reject before entry.
4. **Module-set linking contract** — `hosts/wasm` (`WasmRtV1Host::run_package`,
   `faber_external` import → `__faber_{F}` sibling-export resolution,
   `incipit` entry, dependency-first instantiation; `package_run_test.rs` 10
   tests). The browser host reuses this contract; it does not duplicate native
   lifecycle work (CAMPAIGN §Scope Routing; stop condition: no duplication of
   Wasm Host Parity's package linking).

**Known-red caveat for the Stage 2 static-admission command:** the hosts
`./scripta/webgpu-browser-proof check` static admission is red today on the
PARSE030 graphics-fixture surface (§2.2 red 2); Stage 2 admission must not
treat that reference-surface red as a host-contract failure — the admission
gate (instantiate module set, invoke entry, forward one callback, reject
unknown imports, report traps, one async fixture proving the U5 contract) is
assessed against the real generated module set, with the PARSE030 repair owned
by triga exempla upkeep / hosts fixture selection.

---

## 6. Validation (this unit)

Declared validation from delivery-stage0.md §5 U7 / §7 — run exactly once
after the last product edit (no cargo):

```sh
cd /Users/ianzepp/work/faberlang/faber
./scripta/check-factory-goal-status    # no drift; the goal-status audit remains clean
git diff --check                       # no whitespace errors
```

**Result:** `check-factory-goal-status` reported no drift (exit 0); `git diff
--check` clean. Stage 0 remains `planned` in goal.md / CAMPAIGN.md — no README
regeneration was needed or run (delivery U7 non-goal). No cargo was invoked.

Write scope honored: only `stage0-closeout.md` is added by this unit. Foreign
WIP (`src/cli/*`, `src/commands/format*`, `faber/crates/exempla/*` and other
hands' in-flight work) was not touched.
