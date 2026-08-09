# Delivery Spec — BROWSER-WASM-PRODUCT Stage 1: Browser-Minimum MIR-to-Wasm Reachability (compiler surface; shape list S1–S6)

**Status**: planned (delivery lowering complete, 2026-08-09)
**Planner**: planner-1 (task `f914c781`)
**Campaign**: [CAMPAIGN.md](CAMPAIGN.md) · **Goal**: [goal.md](goal.md) ·
**Predecessor**: [stage0-closeout.md](stage0-closeout.md) (U7 `1ece368` — Stage 0 gate closeout + boundary review)
**Control-plane repo**: `/Users/ianzepp/work/faberlang/faber`
**Mode**: **P3 delivery** — compiler-surface shape lowering through the ordinary `faber build --target wasm --package .` route. Zero browser recipe, zero browser host, zero radix product edits.
**Posture**: split-on-boundary; batch homogeneous Wasm carrier families (CAMPAIGN §Stage 1)
**Consumes**: wasm-host-parity Stage 1 baseline (`radix/docs/factory/wasm-host-parity/baseline-gap-ledger.toml`, 308 rows), [stage0-closeout.md](stage0-closeout.md) §4 shape list (S1–S6), [triga-fixture-selection.md](evidence/triga-fixture-selection.md) (U6), [faber-wasm-package-baseline.md](evidence/faber-wasm-package-baseline.md) (U2), [async-abi-ledger.md](evidence/async-abi-ledger.md) (U5).

---

## 1. P1 Goal (forge)

### 1.1 Intent

Make the six shape families named in stage0-closeout §4 — **package calls
(S1), records and lists (S2), option/error carriers (S3), matrices and
vectors (S4), callable identity (S5), callback exports (S6)** — lower
through validated MIR to valid core Wasm via the faber/faber-wasm package
route, per CAMPAIGN §Stage 1 ("make the selected controller and static Triga
source shapes lower through validated MIR to valid core Wasm").

This is the **compiler/emitter surface** stage. It proves each shape family
carries through the module set; it does not build the browser distribution
(Stage 3), admit the browser host (Stage 2), rework `faber-web` contracts
(Stage 4), or run Triga acceptance proofs (Stage 5/6).

### 1.2 Outcome

Two shape-probe packages under `faber/corpus/browser-wasm/` — one carrying the
selected **controller** shapes (`probe-controller`), one carrying the selected
**static Triga geometry** shapes (`probe-geometry`) — emit and validate as
deterministic module sets through `faber build --target wasm --package .`
(build twice → identical `.wat` set and export surface), **or** each remaining
shape failure is an explicit target-contract rejection with one earliest owner
recorded in the stage evidence (CAMPAIGN §Stage 1 gate). Missing compiler
primitives are recorded as owning-repo blocking deliveries (radix) — never
worked around in faber glue, probe source, or demo-side workarounds.

### 1.3 Stage-1 gate reading (decision, recorded)

CAMPAIGN §Stage 1 gate: *"both selected packages emit and validate as a
deterministic module set, or each remaining failure is an explicit
target-contract rejection with one earliest owner. No source replay or TS
fallback participates."*

Reading used here (cited, not invented): the "selected packages" are the two
**shape-probe packages** carrying the selected controller and static Triga
shapes. The real demo packages (`triga/corpus/webgl-geometries` etc.) cannot
yet assemble through the wasm route because the locked source-library closure
is incomplete (U2 gap #2 — `plan_wasm_artifacts` emits package units only, no
`GeneratedLibrary`/lock-index handling). That closure is **Stage 3** product
work (CAMPAIGN §Stage 3 "close locked source-library dependencies through the
Wasm target"); it is not a Stage 1 compiler-surface gap. The Stage 1 gate is
satisfied when the two probe module sets emit + validate deterministically and
every S1–S6 shape is either probe-proven live or an explicit contract
rejection with one earliest owner.

### 1.4 Boundaries

| Boundary | Holder |
| --- | --- |
| Compiler primitives missing for a shape (emission, naming, MIR facts) | Radix shared MIR / radix-mir-wasm (`wasm-encoding`) — **recorded as owning-repo blocking deliveries R2–R4 (§5)**, routed by Mind to radix's own delivery process; never implemented in this faber delivery |
| `@ nucleum` GPU-register lanes (`vector/*` cross/dot/elementwise/swizzle/builtins, `gpu-core-types/matrix-register`) | wasm-host-parity promotion packet **P12** (device-lane disposition, owner GPU session) — contract-rejection with earliest owner, **not** a Stage 1 primitive gap (§5 R1) |
| Shape-proof fixtures | `faber/corpus/browser-wasm/` (faber package corpus; precedent `corpus/importa-wasm/`) |
| Shape evidence | `faber/docs/factory/browser-wasm-product/stage1-evidence/` |
| Module-set contract notes for the browser host | Recorded in stage evidence; consumed by Stage 2 (CAMPAIGN §Stage 2 / stage0-closeout §5) |
| Native-host parity rows (`output_mismatch`, `runtime_import_unresolved` at `invoked`+) | wasm-host-parity native-host concerns, **not** Stage 1 emit+validate blockers; recorded as notes for the browser host import table (Stage 2) |

### 1.5 Non-goals

- No browser-app product recipe, no `[build] target = "wasm"` manifest row,
  no persisted `.wasm` publication, no serve/dist UX (all **Stage 3**).
- No browser host, no module loader, no import resolution, no callback
  trampoline implementation (all **Stage 2**).
- No `faber-web` contract/target-neutral rework (**Stage 4**).
- No triga demo edits, no physical-browser proof, no pixel/frame evidence
  (auditor/operator gates at **Stage 5/6**).
- No `@ nucleum` register-typed source in the probes (routes to P12, §5 R1).
- No radix product edits, no shared-MIR vocabulary additions from this
  delivery (MIR Blocker Promotion law applies — proposals belong to the
  wasm-host-parity campaign or a routed radix delivery).
- No TS-route `tsc` break repair (Stage 3/8 product work per stage0-closeout
  §2.2 red 1); no repair of the `radix emit` PARSE030 divergence (separate
  radix-emit surface item per stage0-closeout §4 note).
- No edits to `evidence/*` or `stage0-closeout.md` (auditor-2 is reading them
  in parallel), no CAMPAIGN.md/goal.md status changes (Stage 1 stays
  `planned`; the gate close is recorded in `stage1-closeout.md`).

### 1.6 Decision owners

| Decision | Owner | Default |
| --- | --- | --- |
| Gate reading (probe packages = "both selected packages") | Mind accepts this spec | recorded §1.3; only changed by explicit operator/campaign amendment |
| Routing of R2/R3/R4 to radix deliveries | Mind | routed to radix's own goal-forge/delivery (wasm-host-parity campaign home) |
| `@ nucleum` register-lane disposition | GPU session (P12 recheck trigger) | device-lane disposition stays open |
| Probe manifest shape (`[paths]`-only vs manifestless) | U0 (live verification) | fallback to U2's manifestless default-entry probe precedent |

---

## 2. P2 Done-when + validation (check)

### 2.1 Done-when (stage level)

1. **Both probe packages** (`probe-controller`, `probe-geometry`) emit through
   `faber build --target wasm --package .` and validate (WAT parses to binary
   via the builder's `wat::parse_str`, `faber/src/package/wasm.rs`) as
   **deterministic module sets**: a repeat build produces an identical `.wat`
   set (shasum) and identical export/import surface.
2. **Every S1–S6 shape family is in exactly one disposition**, recorded in the
   stage evidence: (a) probe-proven live (emits + validates), or (b) explicit
   target-contract rejection with **one earliest owner** named (R1–R4 or a
   routed wasm-host-parity row).
3. **No source replay or TS fallback** participates: probe transcripts show
   `--target wasm`, no `tsc`, no `.ts`/`.tsx`/`tsconfig` artifacts under the
   probe packages.
4. **Radix-owned primitive gaps are recorded as owning-repo blocking
   deliveries** (R2–R4 §5) with the exact ledger rows/fixtures — not worked
   around in faber glue, probe source, or demo-side accessor copies.
5. **Stage evidence + closeout**: `stage1-evidence/s1…s6-*.md` + U0 scaffold
   doc + `stage1-closeout.md` checked in; closeout re-checks the CAMPAIGN
   §Stage 1 gate bullet and emits the Stage 1→2 module-set contract notes and
   Stage 1→3 handoff.
6. **Zero foreign dirt**: only the declared write paths change; `git diff
   --check` clean; `./scripta/check-factory-goal-status` reports no drift.

### 2.2 Validation (stage level; exact commands)

```sh
# Faber control-plane (no cargo)
cd /Users/ianzepp/work/faberlang/faber
./scripta/check-factory-goal-status      # no drift; no cargo
git diff --check                         # no whitespace errors

# Aggregate probe (prebuilt binary, no cargo; per-unit probes are the required closeout)
FABER_BIN=${FABER:-$PWD/target/debug/faber} ./corpus/browser-wasm/run-probes.sh
```

Per-unit required closeout (every U1–U6):

```sh
cd faber/corpus/browser-wasm/probe-<lane>
"$FABER_BIN" build --target wasm --package .        # transcript captured; EXIT=0
"$FABER_BIN" build --target wasm --package .        # second run
shasum target/faber/wasm/*.wat                      # identical across runs
# export/import-surface greps per unit (see unit tables)
```

Scoped cargo (only where a unit adds exempla-harness regression; run one
command, never in parallel, never with workspace flags):

```sh
cargo nextest run -p exempla --test e2e_harness wasm_shapes   # single crate, single test binary
```

Rationale for `-p` scoping: the package-wasm surface lives in the `faber`
crate (`src/package/wasm.rs`) and the exempla harness in the `exempla` crate
(`crates/exempla/src/exempla_e2e/`); those are the narrowest crates for the
faber-side surface. No bare `cargo`, no `--workspace`, no `--all`/`--all-targets`
flags. Prebuilt-binary probes (zero cargo) are the default proof everywhere.

### 2.3 Factual claims requiring audit (audit-needed)

| Claim | Authority to re-check | Risk |
| --- | --- | --- |
| faber `target/debug/faber` on `main` (1ece368 tip) builds the wasm package route live | `faber/src/package/wasm.rs`; U2 probe transcript | medium — binary may be stale; U0 re-verifies |
| MirAmd arms landed (faber compiles against current radix main) | `faber/src/package/artifact_plan.rs:404`, `src/postprocess.rs:111/140` | low |
| Records/structs emit as aggregate-handle carriers (`Type::Record/Struct/Enum → WasmValue::AggregateHandle`) | `radix/crates/radix-mir-wasm/src/operand.rs` `scalar_ty` | low |
| List constructs emit through v1 rows (`array_new`/`array_push`) | `radix/crates/radix-mir-wasm/src/collection.rs` | low |
| `vector/*` + `gpu-core-types/matrix-register` gaps are the P12 `@ nucleum` register lanes (owner GPU session, required tier `validated`) | `radix/docs/factory/wasm-host-parity/promotion-packet-p12-device-vector-matrix-lanes.md`; `baseline-gap-ledger.toml` rows | medium — this routing decides whether S4 needs a radix delivery at all |
| `conversio/*` wasm-encoding cast-emission rows (7) are the `int<u32>` cast family | `baseline-gap-ledger.toml` rows (owners/blockers) | medium |
| `float32_values` cross-module enum-scope access is a known language gap | `triga/corpus/README.md` §Library gaps; `triga/src/geometry/attribute.fab` | medium — decides R3 |
| `lista/methodi-copiae` `runtime_import_unresolved` (owner cpu-abi) is a host import-table item, not an emission blocker | `baseline-gap-ledger.toml`; `wasm_ledger.rs` tier→blocker mapping | low |
| U2 manifest asymmetry (CLI `--target wasm` works; `[build] target = "wasm"` rejects) | `faber/src/package/manifest.rs`; U2 evidence | low — probes use CLI flag only |

---

## 3. Repo-aware baseline (live tree, verified 2026-08-09)

- **Faber wasm package route (live, feature `mir-wasm`):**
  `src/package/compile.rs` (`Target::MirWasmBinary` dispatch),
  `src/package/artifact_plan.rs`, `src/package/wasm.rs` (`build_package_wasm`:
  one module per unit; WAT written under `<pkg>/target/faber/wasm/`; `.wasm`
  bytes in-memory in `entry_bytes`/`sibling_bytes`; `faber_external`
  cross-module imports; `incipit` entry; deterministic `module_file_name` =
  zero-padded index + product + segments). Tests: `src/package/wasm_test.rs`
  (builder + `WasmRtV1Host::run_package` round trip). **No `[build] target =
  "wasm"` manifest arm** (U2 §(b)) — probes use the CLI `--target wasm` flag.
- **MirAmd residual resolved**: `Target::MirAmd` arms landed in faber
  (`artifact_plan.rs:404`, `postprocess.rs:111/140`); faber git tree clean at
  capture; the U2 build-blocker no longer applies.
- **Portable host (read-only reference):** `hosts/wasm` (`WasmRtV1Host::run_package`,
  `faber_external` → `__faber_{F}` sibling-export resolution, dependency-first
  instantiation); `package_run_test.rs` 10 tests. Native-only today; the
  browser host is Stage 2.
- **wasm-host-parity ledger (authority for shape routing):**
  `radix/docs/factory/wasm-host-parity/baseline-gap-ledger.toml` — 308 rows
  (160 parity / 132 gap / 13 contract-reject / 3 n/a). Fixture families:
  `operatores/*` 11 gap, `conversio/*` 11 gap (7 owner `wasm-encoding` +
  4 `output_mismatch`), `intrinseca/*` 8 gap, `vector/*` 5 gap (all
  `declaration-only`, owner `wasm-encoding`, `wasm_emission_unsupported` →
  **P12 register lanes**), `gpu-core-types/*` 1 gap (`matrix-register`,
  same P12 family) + 5 contract-reject, `lista/*` 1 gap (`methodi-copiae`,
  owner `cpu-abi`, `runtime_import_unresolved`), `literalia/*` 1 gap,
  `tabula/*` 1 gap (`output_mismatch` — native host parity), `functio/*`
  1 gap (`sponte-vel`, `output_mismatch`). Emission-level reachability is
  already proven for records, lists, options, functio at the
  `mir→validated`/`invoked` tiers.
- **Shape probes home**: `faber/corpus/` package corpus (precedent
  `importa-wasm/` — two-unit sibling-import probe, carrier-typed call, WAT
  export surface captured in U2 §(a)). `corpus/browser-wasm/` does not exist
  yet.
- **Known deviations to record, not fix**: textus handle cannot cross module
  instances (separate linear memories; U6-E note in
  `crates/exempla/src/exempla_e2e/wasm_package.rs`) — scalar carriers cross as
  i64; the `emit`-vs-`check` PARSE030 divergence (stage0-closeout §4 note) —
  wasm path uses faber/faber-wasm.

---

## 4. Shape routing table (S1–S6 → disposition)

Source: stage0-closeout §4 table (shape families + concrete carriers + reachability
evidence). Disposition keys: **probe** = faber probe package proves it (unit),
**R2–R4** = radix-owned blocking delivery recorded §5, **P12** = contract
rejection, earliest owner GPU session.

| # | Shape family (closeout §4) | Concrete carriers | Ledger/reachability evidence | Disposition | Unit |
| --- | --- | --- | --- | --- | --- |
| S1 | Package calls | Sibling `importa ex "./auxilium"`, canonical `__faber_external_*` sibling export, `incipit` entry, multi-unit module set linking | **Already live** — U2 §(a)/(c) two-unit probe + `WasmRtV1Host::run_package` round trip | **probe** — broader package graph proof (≥3 units), carrier-typed cross-module calls | U1 |
| S2 | Records and lists | `CorpusMesh`/`CorpusGeometryManifest`-shaped records; `list<f32>`/`list<int<u32>>` append/iteration at scale (interleaved 9-f32 vertex projection; nested-loop mesh build) | `tabula/*` 1 gap `output_mismatch` (host parity, not emission); `lista/*` 1 gap `methodi-copiae` `runtime_import_unresolved` owner `cpu-abi` (host import table, Stage 2 note); record carrier `Record→AggregateHandle` (`operand.rs`) | **probe** + host-note | U2 |
| S3 | Option/error carriers | `∪ null`; `fac { } cape err { }`; `⇥`/`*_or` recovery (`FetchResponse ⇥ textus`); P10 `ReturnError` | U5 R3/R4/R8 grounding (`_or` family `lib.rs:199-210`; status-first `lib.rs:505-514`); `operatores/optional-chain`, `functio/sponte-vel` gaps are `output_mismatch` (host parity) | **probe** | U3 |
| S4 | Matrices and vectors | `Vector3`/`Matrix4`/`Euler`/`Quaternion`/`TransformPayload` as **record carriers** over f32 (triga `math.fab`); `int<u32>` casts (`conversio` family) | `vector/*` 5 gap = `@ nucleum` **register lanes** → **P12** (required tier `validated`, owner GPU session); `conversio/*` 7 `wasm-encoding` cast-emission rows → **R2**; record-carrier math = S2 carrier (live) | **probe** (record subset) + **R2** + **P12** rejection | U4 |
| S5 | Callable identity | `functio` composition; cross-module accessor identity (`float32_values` enum-scope — known gap) | `functio/*` at parity (1 `output_mismatch` host row); `float32_values` = known library gap (`triga/corpus/README.md` §Library gaps; U6 §5.2 row 1) | **probe** + **R3** if reject | U5 |
| S6 | Callback exports | Exported callable with aggregate-carrier param (`FrameState`-shaped record); callback identity as first-class export; `TransformPayload` 32-f32 carrier | U6 §5.2 rows 3–4; export ABI surface = record-param export via aggregate handle (verification item) | **probe** + **R4** if reject | U6 |

---

## 5. Recorded radix-owned blocking deliveries (R1–R4)

These are **recorded, not implemented here**. Mind routes them to the owning
repo's own delivery process. Faber units complete by recording the exact
reject (contract-rejection with one earliest owner) per the CAMPAIGN gate —
the faber graph does not stall on radix.

| # | Shape | Ledger rows / fixture | Earliest owner | Radix home | Faber dependency |
| --- | --- | --- | --- | --- | --- |
| R1 | `@ nucleum` GPU-register lanes (`Vector<T,[N]>`, `Matrix<T,[R,C]>`) — cross/dot/elementwise/swizzle/builtins/matrix-register | `vector/cross.fab`, `vector/dot.fab`, `vector/elementwise.fab`, `vector/swizzle.fab`, `vector/builtins.fab`, `gpu-core-types/matrix-register.fab` (all `declaration-only`, tier `mir→validated`, `wasm_emission_unsupported`) | GPU session (device-lane) | wasm-host-parity **promotion-packet-p12** (already open; recheck trigger = device-lane disposition or a host-side lane ABI row) | U4 records the contract-rejection with owner GPU session; probes must not use register types |
| R2 | `int<u32>` / numeric cast emission (`conversio` family) | `ad/sermo-conversio`, `conversio/fallibilis`, `conversio/instans`, `conversio/instans-valor-carrier`, `conversio/octeti`, `conversio/valor-genus`, `conversio/valor-scalaria` (all `mir→outcome-checked`, `wasm_emission_unsupported`) | `wasm-encoding` (radix-mir-wasm) | radix delivery via wasm-host-parity Stage 3/3b reachability or a new promotion packet (MIR Blocker Promotion law) | U4 records exact reject; done only with landing or contract-rejection |
| R3 | Cross-module enum-scope access (`discerne` over `AttributeData` from another module; `float32_values` accessor identity) | U6 §5.2 row 1; `triga/corpus/README.md` §Library gaps ("not workarounds in demos") | radix frontend / shared-MIR fact (or `wasm-encoding` identity naming) | radix delivery via wasm-host-parity MIR Blocker Promotion proposal | U5 probes; R3 filed only if the probe rejects |
| R4 | Aggregate-carrier export ABI (record-param exported callables) | verification item — filed only if U6's probe rejects | `wasm-encoding` (radix-mir-wasm) | radix delivery (wasm-host-parity home) | U6 records the export surface; R4 pending probe result |

Native-host parity residuals (recorded as Stage 2 notes, not blocking):
`lista/methodi-copiae` (`runtime_import_unresolved`, owner `cpu-abi`) and the
`output_mismatch` rows (`tabula/tabula`, `intrinseca/*`, `functio/sponte-vel`,
`operatores/*` subset) are host import-table / output-matching concerns the
browser host must resolve at Stage 2 admission (stage0-closeout §5 module-set
linking contract); they do not block Stage 1 emit+validate.

---

## 6. P3 Ordered unit graph

### 6.1 Topology

```text
U0 (probe scaffold + harness)
 ├── Lane G (geometry probe — shared package surface, SERIAL):
 │     U1 (S1 package calls) → U2 (S2 records/lists) → U4 (S4 matrices/vectors/casts)
 └── Lane C (controller probe — shared package surface, SERIAL):
       U3 (S3 option/error) → U5 (S5 callable identity) → U6 (S6 callback exports)
          └── all ──► U7 (Stage 1 closeout + gate review)
```

- **Serialization boundary (named, per Rule 5):** each lane's units share one
  probe package's `src/` + `main.fab` write surface, so a lane is serial.
  The two lanes are disjoint (separate probe packages, separate evidence
  docs) and run in parallel.
- **Parallelism:** U1‖U3 after U0; U2‖U5; U4‖U6; U7 strictly last.
- **Cargo rule:** one cargo command at a time; `-p` scoped; no workspace
  flags. Prebuilt-binary probes are the default.
- **Read-only everywhere:** radix, hosts, triga, faber-web, examples, and
  `faber/src/` are read-only. `evidence/*` + `stage0-closeout.md` are
  read-only (auditor-2 in flight).

### 6.2 Unit tables

#### U0 — Stage 1 probe scaffold + probe harness

| Field | Value |
|---|---|
| `id` | `bwp-s1-u0-probe-scaffold` |
| `outcome` | `faber/corpus/browser-wasm/` created: two probe package skeletons (`probe-controller/`, `probe-geometry/`), each with a `faber.toml` (`[package]` + `[paths]`, **no** `[build] target` row) and a minimal `main.fab` that emits + validates today through the CLI `--target wasm` route; `run-probes.sh` harness (build each probe → assert EXIT=0 → capture transcript → second build → shasum `.wat` set → export/import-surface greps); `README.md` recording the §1.3 gate reading, the shape→probe→unit mapping, and the probe manifest decision (verified live; fallback = U2 manifestless default-entry precedent). `faber/corpus/README.md` gets probe rows. Exempla harness module `wasm_shapes.rs` wired into the e2e_harness test binary (pattern: `wasm_ledger`/`wasm_package`) running both probes in-process. Evidence doc `stage1-evidence/u0-scaffold.md`. |
| `write_scope` | `faber/corpus/browser-wasm/` (new: `probe-controller/`, `probe-geometry/`, `run-probes.sh`, `README.md`), `faber/corpus/README.md` (append probe rows), `faber/crates/exempla/src/exempla_e2e/wasm_shapes.rs` (new) + e2e_harness wiring, `faber/docs/factory/browser-wasm-product/stage1-evidence/u0-scaffold.md` (new) |
| `read_scope` | `faber/corpus/importa-wasm/` (manifest + entry shape), `faber/src/package/wasm.rs` + `wasm_test.rs` (build path, determinism naming, round trip), `evidence/faber-wasm-package-baseline.md` §(a)/(c), `stage0-closeout.md` §4, `faber/corpus/README.md` |
| `done_when` | Both skeleton probes build + validate through the prebuilt binary with EXIT=0; run-twice shasum of `target/faber/wasm/*.wat` identical (deterministic module set); `run-probes.sh` exits 0 and captures transcripts; `wasm_shapes.rs` compiles and runs both probe builds in-process (or documents the exact unavailable surface honestly with a skip — never a silent stub); probe README records the gate reading + mapping + manifest decision; `check-factory-goal-status` reports no drift. |
| `validation` | `cd faber && ./scripta/check-factory-goal-status` (no cargo); `FABER_BIN=$PWD/target/debug/faber ./corpus/browser-wasm/run-probes.sh` (prebuilt binary, transcript); `cargo nextest run -p exempla --test e2e_harness wasm_shapes` (single crate, single test binary, run once) |
| `est_work_tokens` | 6 000 – 9 000 |
| `tool_latency` | probe builds 30 s–3 min each (prebuilt binary); exempla harness compile+run 1–10 min (single `-p exempla` command); `check-factory-goal-status` < 10 s |
| `depends_on` | None (reads the U7 closeout `1ece368` as base; no product code) |
| `non_goals` | No shape modules yet (U1–U6 own them); no `[build] target` manifest rows (Stage 3); no faber `src/` product edits; no radix/hosts/triga/faber-web/examples writes; no `evidence/` or `stage0-closeout.md` edits; no CAMPAIGN.md/goal.md status edits |

#### U1 — S1 Package calls (geometry lane)

| Field | Value |
|---|---|
| `id` | `bwp-s1-u1-package-calls` |
| `outcome` | Geometry probe extended to a ≥3-unit package graph (entry + ≥2 sibling modules) mirroring the fixture's cross-module package-call surface (sibling `importa ex`, carrier-typed cross-module calls, canonical `__faber_external_*` sibling exports, `incipit` entry). Module set emits + validates + is deterministic. Evidence doc `s1-package-calls.md` records the live package-call surface (U2 authority), the exact import surface (grep of `import "faber_rt_v1"` / `import "faber_external"` rows — feeds the Stage 2 module-set contract), and the textus-handle cross-module deviation (separate linear memories) as a recorded boundary, not a Stage 1 gap. |
| `write_scope` | `faber/corpus/browser-wasm/probe-geometry/src/*.fab` (S1 modules) + `main.fab` wiring, `faber/docs/factory/browser-wasm-product/stage1-evidence/s1-package-calls.md` (new), `faber/corpus/browser-wasm/README.md` (shape row) |
| `read_scope` | `faber/corpus/importa-wasm/` (U2 probe shape), `faber/src/package/wasm.rs`, `hosts/wasm/tests/package_run_test.rs`, `evidence/faber-wasm-package-baseline.md` §(a)/(c), `stage0-closeout.md` §4 S1 row |
| `done_when` | probe-geometry module set (entry + ≥2 siblings) emits + validates (EXIT=0); `.wat` shows `__faber_external_*` canonical exports on each sibling and the entry's `faber_external` imports resolving by field name; entry keeps `incipit`, siblings never export it; repeat-build shasum identical; evidence doc records the package-call surface + import surface + textus deviation (recorded boundary, no workaround). |
| `validation` | `cd faber/corpus/browser-wasm/probe-geometry && "$FABER_BIN" build --target wasm --package .` (transcript, twice); `shasum target/faber/wasm/*.wat` identical; grep `.wat` for `__faber_external_`, `"incipit"`, sibling export names; no cargo (optional: extend `wasm_shapes.rs` assertion and run `cargo nextest run -p exempla --test e2e_harness wasm_shapes` once) |
| `est_work_tokens` | 4 000 – 7 000 |
| `tool_latency` | probe build 30 s–2 min (prebuilt); greps < 5 s; no cargo required |
| `depends_on` | U0 |
| `non_goals` | No new compiler primitives; no textus cross-module carrier workaround; no faber product code; no edits to `evidence/*`/`stage0-closeout.md` |

#### U2 — S2 Records and lists (geometry lane)

| Field | Value |
|---|---|
| `id` | `bwp-s1-u2-records-lists` |
| `outcome` | Geometry probe carries `CorpusMesh`/`CorpusGeometryManifest`-shaped records, `list<f32>` / `list<int<u32>>` append + iteration at scale (interleaved 9-f32 vertex payload projection; nested-loop build). Module set emits + validates + is deterministic. Evidence doc `s2-records-lists.md` records per-carrier dispositions against the ledger: record carrier live (`Record→AggregateHandle`, `operand.rs`); list constructs emit through v1 rows (`array_new`/`array_push`, `collection.rs`); `tabula/tabula` `output_mismatch` = native host parity note (not emission); `lista/methodi-copiae` `runtime_import_unresolved` owner `cpu-abi` = browser-host import-table note for Stage 2 (not a Stage 1 blocker). |
| `write_scope` | `faber/corpus/browser-wasm/probe-geometry/src/*.fab` (S2 modules) + `main.fab` wiring, `stage1-evidence/s2-records-lists.md` (new), probe README shape row |
| `read_scope` | `triga/corpus/webgl-geometries/src/shapes.fab` + `scene.fab` (shape reference — read only), `radix/docs/factory/wasm-host-parity/baseline-gap-ledger.toml` (`tabula/*`, `lista/*` rows), `radix/crates/radix-mir-wasm/src/collection.rs` + `operand.rs`, `stage0-closeout.md` §4 S2 |
| `done_when` | S2 module emits + validates with record carriers and list append/iteration at ≥100-element scale; `.wat` shows the v1 list-construct imports and aggregate-handle carriers; evidence doc records each ledger-row disposition (live / host-note / contract-rejection with owner); repeat-build determinism; `check-factory-goal-status` no drift. |
| `validation` | probe build (twice) + shasum; grep `.wat` for `__faber_rt_v1_` collection/array imports; ledger cross-check: `cd radix/docs/factory/wasm-host-parity && grep -n 'path = "tabula/\|path = "lista/' baseline-gap-ledger.toml` (no cargo) |
| `est_work_tokens` | 6 000 – 10 000 |
| `tool_latency` | probe build 30 s–3 min (scale loops); grep/ledger < 15 s; no cargo required |
| `depends_on` | U1 (same lane — probe-geometry shared write surface) |
| `non_goals` | No `@ nucleum` register types (P12/§5 R1); no host-parity or ABI-catalog fixes; no triga edits; no runtime-contract edits |

#### U3 — S3 Option/error carriers (controller lane)

| Field | Value |
|---|---|
| `id` | `bwp-s1-u3-option-error` |
| `outcome` | Controller probe carries `∪ null` option carriers, `fac { } cape err { }`, `⇥`/`*_or` recovery, P10 `ReturnError` carrier. Module set emits + validates + is deterministic. Evidence doc `s3-option-error.md` records the `_or` recovery surface (U5 R3/R4/R8 grounding: `_or` family `radix-host-abi/lib.rs:199-210`, status-first `lib.rs:505-514`) and dispositions the ledger families (`operatores/optional-chain`, `functio/sponte-vel` = `output_mismatch` host-parity notes; `conversio/fallibilis` = R2 routing). |
| `write_scope` | `faber/corpus/browser-wasm/probe-controller/src/*.fab` (S3 modules) + `main.fab` wiring, `stage1-evidence/s3-option-error.md` (new), probe README shape row |
| `read_scope` | `radix/crates/radix-host-abi/src/lib.rs` (P10 `ReturnError` rows; `_or` family), `evidence/async-abi-ledger.md` (R3/R4/R8), `faber-web/src/dom.fab` (controller shape reference — read only), `baseline-gap-ledger.toml` (`operatores/*`, `functio/*`, `conversio/*` rows), `stage0-closeout.md` §4 S3 |
| `done_when` | S3 module emits + validates; probe source contains `∪ null`, `fac`/`cape`, and an `*_or`/`⇥` recovery shape; evidence doc maps the carriers to U5 rows and records the disposition of the three ledger families; repeat-build determinism. |
| `validation` | probe build (twice) + shasum; `rg` cross-check of the U5 citations (`fetch_text`, `_or` rows) in the evidence doc; no cargo |
| `est_work_tokens` | 5 000 – 8 000 |
| `tool_latency` | probe build 30 s–2 min; rg < 10 s; no cargo required |
| `depends_on` | U0 |
| `non_goals` | No async-dispatcher surface (Stage 2/U5 Q2); no faber-web edits; no host work; no ABI catalog edits |

#### U4 — S4 Matrices/vectors/casts (geometry lane)

| Field | Value |
|---|---|
| `id` | `bwp-s1-u4-matrices-vectors` |
| `outcome` | Geometry probe carries the **record-carrier** math subset: `Vector3`/`Matrix4`/`Euler`/`Quaternion` as struct records over f32, `TransformPayload` as a 32-f32 record, and `int<u32>` cast arithmetic. Emits + validates for the record-carrier subset. Evidence doc `s4-matrices-vectors.md` (a) routes the `@ nucleum` register lanes to **P12** (owner GPU session, required tier `validated`) — contract rejection, not a Stage 1 gap; (b) records each `conversio/*` cast-emission gap as **R2** (owner `wasm-encoding`) with the exact rejecting construct (fixture line/expression), or proves casts live; (c) records the `TransformPayload` 128-byte aggregate carrier for the Stage 2/6 consumption note. |
| `write_scope` | `faber/corpus/browser-wasm/probe-geometry/src/*.fab` (S4 modules) + `main.fab` wiring, `stage1-evidence/s4-matrices-vectors.md` (new), probe README shape row |
| `read_scope` | `triga/src/math.fab` (`Vector3`/`Matrix4`/`Euler`/`Quaternion`/`TransformPayload` shapes — read only), `radix/docs/factory/wasm-host-parity/promotion-packet-p12-device-vector-matrix-lanes.md`, `baseline-gap-ledger.toml` (`vector/*`, `gpu-core-types/*`, `conversio/*` rows), `stage0-closeout.md` §4 S4 |
| `done_when` | Record-carrier math subset emits + validates (EXIT=0); P12 register lanes recorded as contract-rejection with owner GPU session (verbatim fixture list cited); each `conversio/*` cast row is proven live or recorded as an exact R2 reject with the owning construct named; `TransformPayload` carrier recorded; repeat-build determinism. |
| `validation` | probe build (twice) + shasum; P12 fixture list cross-check (`grep` promotion packet + ledger rows); `awk`/`grep` extraction of `conversio/*` gap rows into the evidence doc; no cargo |
| `est_work_tokens` | 5 000 – 9 000 |
| `tool_latency` | probe build 30 s–2 min; ledger extraction < 15 s; no cargo required |
| `depends_on` | U2 (same lane) |
| `non_goals` | No `@ nucleum` register usage in the probe (routes to P12); no math-library edits; no force-lowering of the register lanes; no faber glue for casts; no host ABI edits |

#### U5 — S5 Callable identity (controller lane)

| Field | Value |
|---|---|
| `id` | `bwp-s1-u5-callable-identity` |
| `outcome` | Controller probe carries `functio` composition + cross-module callable identity (a module calls a sibling function through the canonical external symbol; a callable identity is forwarded). The `float32_values` enum-scope cross-module accessor shape (U6 §5.2 row 1; `triga/corpus/README.md` §Library gaps) is probed: if cross-module enum-scope access rejects, evidence doc `s5-callable-identity.md` records the exact reject and files **R3** (radix frontend/shared-MIR fact, MIR Blocker Promotion law) — no demo-side workaround, per the corpus README's own rule. |
| `write_scope` | `faber/corpus/browser-wasm/probe-controller/src/*.fab` (S5 modules) + `main.fab` wiring, `stage1-evidence/s5-callable-identity.md` (new), probe README shape row |
| `read_scope` | `triga/corpus/webgl-geometries/src/shapes.fab` (`float32_values` call site — read only), `triga/src/geometry/attribute.fab` (accessor + enum co-location note), `triga/corpus/README.md` §Library gaps, `baseline-gap-ledger.toml` (`functio/*`, `intrinseca/*` rows), `stage0-closeout.md` §4 S5 |
| `done_when` | functio composition + cross-module callable identity emit + validate; cross-module accessor probe result recorded (live, or exact reject + R3 filed with the triga README citation); `.wat` shows the canonical external symbol for the cross-module accessor; repeat-build determinism. |
| `validation` | probe build (twice) + shasum; grep `.wat` for the canonical external symbol of the cross-module accessor; `rg` the triga README §Library gaps citation into the evidence doc; no cargo |
| `est_work_tokens` | 4 000 – 7 000 |
| `tool_latency` | probe build 30 s–2 min; no cargo required |
| `depends_on` | U3 (same lane) |
| `non_goals` | No demo-side workaround for the enum-scope gap; no triga edits; no compiler edits; no faber-web edits |

#### U6 — S6 Callback exports (controller lane)

| Field | Value |
|---|---|
| `id` | `bwp-s1-u6-callback-exports` |
| `outcome` | Controller probe exports a callable taking an aggregate-carrier parameter (`FrameState`-shaped record) and carries a `TransformPayload`-shaped 32-f32 record, each under a stable export name (callback identity as first-class export). Module set validates; `.wat` shows the export + i64 aggregate-handle param. Evidence doc `s6-callback-exports.md` records the **export ABI surface** for Stage 2 (export name, aggregate-handle param carrier, host call-in contract — how the browser host invokes the export with an arena payload record), per stage0-closeout §4 S6 + U6 §5.2 rows 3–4. If record-param export emission rejects → record the exact reject and file **R4** (owner `wasm-encoding`, export ABI for aggregate carriers). |
| `write_scope` | `faber/corpus/browser-wasm/probe-controller/src/*.fab` (S6 modules) + `main.fab` wiring, `stage1-evidence/s6-callback-exports.md` (new), probe README shape row |
| `read_scope` | `faber-web/src/dom.fab` (`on_frame`/`FrameState` contract — read only), `triga/src/math.fab` (`TransformPayload` — read only), `radix/crates/radix-mir-wasm/src/operand.rs` (`scalar_ty` record→aggregate-handle), `stage0-closeout.md` §4 S6 + §5 (Stage 2 packet), `evidence/async-abi-ledger.md` |
| `done_when` | ≥1 exported function with an aggregate-carrier param under a stable name emits + validates; `.wat` shows the export name + i64 param; evidence doc records the export ABI surface for the Stage 2 host (or the exact R4 reject); repeat-build determinism. |
| `validation` | probe build (twice) + shasum; grep `.wat` for the export name + param carrier; no cargo |
| `est_work_tokens` | 4 000 – 7 000 |
| `tool_latency` | probe build 30 s–2 min; no cargo required |
| `depends_on` | U5 (same lane) |
| `non_goals` | No browser host (Stage 2); no dispatcher naming (Q2, Stage 2); no faber-web rework (Stage 4); no physical browser run (auditor gate) |

#### U7 — Stage 1 closeout + gate review

| Field | Value |
|---|---|
| `id` | `bwp-s1-u7-stage1-closeout` |
| `outcome` | `stage1-closeout.md` re-checks every CAMPAIGN §Stage 1 gate element against the U0–U6 evidence: (a) both probe module sets emit + validate deterministically; (b) every S1–S6 shape disposition (probe-proven / contract-rejection + one earliest owner) with a named evidence file + row; (c) no source replay / TS fallback in any transcript. Emits the **Stage 1→2 handoff** (module-set contract notes: export surface, aggregate-handle carriers, exact `faber_rt_v1`/`faber_external` import surface the browser host must provide — consuming stage0-closeout §5) and the **Stage 1→3 handoff** (what the product recipe consumes: the probe-proven shape set + determinism evidence). Records residuals: emit-vs-check PARSE030 divergence (radix-emit surface item), `lista/methodi-copiae` runtime import (Stage 2 browser-host import table), R1–R4 status. |
| `write_scope` | `faber/docs/factory/browser-wasm-product/stage1-closeout.md` (new) |
| `read_scope` | all `stage1-evidence/*` docs (U0–U6), final probe packages, CAMPAIGN.md §Stage 1, stage0-closeout.md §4/§5, this spec §2/§4/§5 |
| `done_when` | Gate-checklist maps each CAMPAIGN §Stage 1 gate element → evidence file + row; shape disposition table (S1–S6) complete with owners; R1–R4 status table (open / landed / rejection); Stage 2 + Stage 3 handoff sections explicit; residuals named with owners. |
| `validation` | `cd faber && ./scripta/check-factory-goal-status` (no drift; no cargo); `git diff --check`; one full `./corpus/browser-wasm/run-probes.sh` run (prebuilt binary) as the aggregate probe — run exactly once |
| `est_work_tokens` | 3 000 – 5 000 |
| `tool_latency` | `check-factory-goal-status` < 10 s; `run-probes.sh` 1–5 min; no cargo |
| `depends_on` | U1, U2, U3, U4, U5, U6 |
| `non_goals` | No CAMPAIGN.md/goal.md status edits (Stage 1 stays `planned`; the close lives in `stage1-closeout.md`); no README regen; no product code; no `evidence/` or `stage0-closeout.md` edits |

---

## 7. Checkpoints and gates

- **Batching / split decision:** split-on-boundary per CAMPAIGN §Stage 1. Two
  parallel lanes (geometry, controller) over one scaffold; homogeneous carrier
  families batched within a shape unit (prove one pattern per shape family).
- **Stage 1 gate (CAMPAIGN):** re-checked at U7 (§2.1 done-when).
- **Delivery audit:** this spec is the P3 artifact for the auditor (delivery
  audit `admitted`/`revise`) before Mind admits units.
- **Implementation audit:** at the stage/phase boundary, Mind freezes the
  aggregate faber range (`faber/corpus/browser-wasm/`, `stage1-evidence/`,
  `stage1-closeout.md`, optional `wasm_shapes.rs`) for evidence-honesty
  review; radix deliveries R2–R4 are audited in their own repo's range.
- **Release posture:** `not-applicable` / `defer-release` (goal.md §Release
  Posture; no tags, pushes, or publication).
- **Auditor/operator gates:** physical browser runs, pixel readback, frame
  progress, and input evidence remain Stage 5/6 auditor/operator gates — not
  Hand claims in Stage 1.

## 8. Validation summary

- Per-unit: probe build (twice) + shasum determinism + export/import-surface
  grep (prebuilt binary; no cargo), plus the unit's ledger cross-check.
- Exempla regression (optional per unit, required at U0):
  `cargo nextest run -p exempla --test e2e_harness wasm_shapes` — single
  crate, single test binary, run once.
- Stage closeout (U7): `./scripta/check-factory-goal-status` (no drift),
  `git diff --check`, one aggregate `run-probes.sh`.
- Forbidden here: bare `cargo`, `--workspace`/`--all`/`--all-targets`,
  `./scripta/release-gate`, `radix/scripta/test --stage 5-6`/`--e2e`,
  full-profile nextest.

## 9. Open questions for Mind

1. **Radix routing for R2/R3/R4.** Mind routes each recorded blocking
   delivery to radix's own goal-forge/delivery (wasm-host-parity campaign is
   the natural home; P12 already exists for R1). Default: route R2/R3 via
   wasm-host-parity MIR Blocker Promotion; R4 only if U6's probe rejects.
2. **Probe package manifest shape** (`[paths]`-only faber.toml vs
   manifestless default-entry). U0 verifies live; default fallback = U2
   manifestless precedent. No `[build] target` row either way (Stage 3).
3. **Whether "both selected packages" in the CAMPAIGN gate should also name
   the real demo packages** — reading recorded §1.3 (probe packages) because
   real-package closure is Stage 3. Flag for Mind/operator only if the gate
   reading is disputed.
4. **Exempla harness cost** — `-p exempla` compile is heavy; if the harness
   blocks lane velocity, Mind may defer `wasm_shapes.rs` wiring to a
   post-stage regression unit. Default: keep the optional harness at U0.

## 10. Stage handoff notes

- **Stage 1 is compiler-surface only.** Stage 2 consumes the module-set
  contract notes (export surface, aggregate-handle carriers, exact import
  surface) from `stage1-evidence/s6-callback-exports.md` + `u0-scaffold.md`
  + the U1 import-surface records; the browser host remains inside the Stage 0
  allowlist (host-js-allowlist.md) and reuses the module-set linking contract
  (`hosts/wasm`, stage0-closeout §5) — no duplicated native lifecycle work.
- **Stage 3 consumes** the probe-proven shape set + determinism evidence and
  adds the product recipe, `[build] target = "wasm"` manifest arm, persisted
  deterministic `.wasm` module set, and the locked source-library closure.
- **Stage 4/5/6 consume** the S4 record-carrier math, S6 callback-export ABI,
  and `TransformPayload` carrier records as the compiler-surface basis for
  the faber-web contracts and Triga proofs.
- **Closeout rule (tugboat):** one `./scripta/check-factory-goal-status` +
  `git diff --check` + one aggregate `run-probes.sh` at U7 — run exactly
  once; no post-done verification.
