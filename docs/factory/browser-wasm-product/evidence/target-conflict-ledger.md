# Target Capability Conflict Ledger (U3)

**Unit**: `bwp-s0-u3-target-conflict-ledger` (delivery-stage0.md §U3, lines 182–197)
**Campaign**: BROWSER-WASM-PRODUCT Stage 0 — baseline, ownership, boundary lock
**Status**: delivered (evidence captured 2026-08-09)
**Hand**: hand-3 (tugboat, task `76d2e9cd`)
**Consumes**: U0 `evidence/artifact-inventory.md` (Class `.wasm`/`.wat` rows), U2 `evidence/faber-wasm-package-baseline.md` (probe evidence; soft edge re-run by this unit)

## Capture environment

| Item | Value |
| --- | --- |
| faber binary | `/Users/ianzepp/work/faberlang/faber/target/debug/faber` — `faber 1.6.0-rc.1` (prebuilt, no cargo) |
| faber tree HEAD at capture | `e5764f6` — `docs(factory): browser-wasm Stage 0 U4 — host JS allowlist + byte baseline evidence` — **not clean**: foreign WIP in `src/cli/*`, `src/commands/format*`, `src/package/mir/lane_test.rs`, `src/package/mir/link.rs` (other hands' in-flight work, Class B; never touched, never staged) |
| radix tree HEAD at capture | `c238d2cb8` |
| capture date | 2026-08-09 |

## Method

Per delivery-stage0.md §U3 `validation`: `./target/debug/faber targets` (prebuilt binary, transcript captured in §2 below) plus `rg` line authorities — no cargo. U2's CLI probe was re-run fresh by this unit (allowed by the delivery §4 soft-edge note), so this ledger carries its own observed transcript.

## CAMPAIGN gate

CAMPAIGN.md §Stage 0 gate (`faber/docs/factory/browser-wasm-product/CAMPAIGN.md:222`):

> every target capability conflict is named with live authority;

CAMPAIGN.md:181:

> Stage 0 must name any conflict instead of selecting the more convenient claim.

This ledger names four conflicts. Each row: conflict, live authority (file:line + observed command), and proposed reconciliation owner. Per delivery U3 `non_goals`, **no capability table or docs are edited here** — Stage 0 names owners only; repair is Stage 3/8 product work.

---

## Row 1 — `faber targets` `wasm` row says `run=no package=no` / "not faber run/package" vs a live package-wasm build

**Conflict.** The capability surface (both `faber targets` and radix `tool/commands/targets.rs`) reports `wasm` as `run=no package=no` with note "not faber run/package", while faber ships a live, package-aware wasm build path (`faber build --target wasm --package .`), and `artifact_plan`/`plan_package` mark `MirWasmBinary` **supported**. The `package=no` cell and the "not faber run/package" note are stale relative to the live package builder; only `run=no` is currently true (no `faber run`/`--backend` wasm execution).

**Live authority.**

- Capability row (radix): `radix/crates/radix/src/tool/commands/targets.rs:195-201` — `Target::MirWasmBinary => TargetCapabilities { check: true, build: true, run: false, package: false, note: "supported MIR-backed Wasm binary emit (fail-closed subset); external host + faber_* imports; not faber run/package" }`. (Name mapping at `targets.rs:79` and `:104`; import at `:18`.)
- Surface propagation (faber): `faber/src/commands/targets.rs:35` — `FABER_TARGET_ROWS` maps `Target::MirWasmBinary` → name `"wasm"`, feature `"mir-wasm"`; the wasm row inherits radix's row unchanged because `faber_surface_capabilities` overrides only the FMIR family (`faber/src/commands/targets.rs:143-171`, `_ => capabilities` arm).
- Package-wasm build (live): `faber/src/package/compile.rs:449-452` — "U6-D: the package-aware Wasm path accepts Target::MirWasmBinary and emits one module per unit through the package-to-Wasm builder"; `compile.rs:463` — `plan_package(&package, Target::MirWasmBinary)`.
- Support flag (live): `faber/src/package/artifact_plan.rs:170-175` — `Target::MirWasmBinary => { plan_wasm_artifacts(...); (true, None, entry) }` (supported); target spelling `"wasm"` at `artifact_plan.rs:400`.
- Test proof: `faber/src/package/artifact_plan_test.rs:82-91` — `plan_package_wasm_is_supported_and_no_longer_rejected` (`assert!(plan.supported)`, `assert_eq!(plan.target, "wasm")`, `plan_or_reject(...).is_ok()`).

**Observed command** (prebuilt binary, transcript in §2):

```text
$ ./target/debug/faber build --target wasm --package .
/private/tmp/u3-wasm-probe/target/faber/wasm
EXIT=0
$ find target -type f
target/faber/wasm/000-u3-wasm-probe-auxilium.wat
target/faber/wasm/001-u3-wasm-probe-root.wat
```

Two units → two `.wat` modules (one module per package unit; dependency-first order). See U2 `evidence/faber-wasm-package-baseline.md` for the full probe analysis (in-memory `.wasm` bytes, `faber_external` symbol linking, `incipit` entry).

**Proposed reconciliation owner.** **Stage 3 (product) — faber capability-surface truth repair** (`faber/src/commands/targets.rs` faber-surface override to reflect the package-aware wasm builder, plus radix `tool/commands/targets.rs` row sync to `package=yes` while `run=no` stays). Repair is out of scope for Stage 0 per delivery U3 `non_goals`; delivery §8 `zombie-docs` names this as the stale-`wasm`-row repair. The cell that needs the revision is `package` (`no` → `yes`), and the note ("not faber run/package") needs rewording to distinguish "no `faber run`/host execution" from "package build exists".

---

## Row 2 — `manifest_build_target` rejects `[build] target = "wasm"` while CLI `--target wasm` works

**Conflict.** The manifest surface has **no** `"wasm"` spelling: `manifest_build_target` fails closed on `[build] target = "wasm"`, so a package whose manifest declares the wasm target cannot be built. The CLI surface (`faber build --target wasm --package .`) accepts the same target and builds it. Two entry points into the same compiler disagree about the same target string.

**Live authority.**

- Allowed-target list: `faber/src/package/manifest.rs:346-368` — `manifest_build_target` match arms: `None`→`HirFhir`, `"rust"`, `"fhir"`, `"ts"`/`"typescript"`, `"scena"`, `"fmir-text"`, `"fmir"`, `"fmir-bin"`, `"llvm-host"`; **no `"wasm"` arm**; `Some(unsupported)` → error `"faber.toml build.target '{unsupported}' is not supported for package builds"` (`manifest.rs:361-368`).
- Call sites: `faber/src/package/manifest.rs:769` (package-build route) and `:811` (CLI-target route) — both route through the same `manifest_build_target`.
- U2 recorded the same asymmetry: `evidence/faber-wasm-package-baseline.md:162` — "the CLI `--target wasm` package route is live (a) while the manifest surface has no `"wasm"` spelling … named for U3's target-conflict-ledger".

**Observed command** (prebuilt binary; fresh two-unit probe package with a manifest declaring the wasm target):

```text
$ ./target/debug/faber build --package .        # [build] target = "wasm" in faber.toml
error: faber.toml build.target 'wasm' is not supported for package builds
EXIT=1

$ ./target/debug/faber build --target wasm --package .   # same package, CLI flag
/private/tmp/u3-wasm-probe/target/faber/wasm
EXIT=0
```

**Proposed reconciliation owner.** **Stage 3 (product) — faber package manifest**: accept `"wasm"` in `manifest_build_target` (`faber/src/package/manifest.rs`) when the durable binary module-set + browser-app recipe lands (CAMPAIGN §Current Tracks "Faber Wasm packages": "persist deterministic binary modules, close target-native libraries, and expose a browser-app recipe"). Until the recipe exists the fail-closed rejection is intentional; the campaign must land both together. Manifest is the authority for the *declared* target and is never silently ignored (`manifest.rs:372-376`).

---

## Row 3 — target-capability-matrix wasm deferral (reopen event = this campaign) + `wasm` CLI-surface row

**Conflict.** `radix/docs/design/target-capability-matrix.md` §Browser Application Product Packaging records wasm as **deferred** ("Wasm remains out of the default target; reopen when browser-Wasm execution is a specified deliverable"), and the §CLI capability surface lists `wasm` as `run=no package=no`. This campaign **is** the reopen event: BROWSER-WASM-PRODUCT specifies browser-Wasm execution as a deliverable (CAMPAIGN §Current Tracks: "Browser product recipe | TypeScript plus `tsc` | replace with Wasm artifacts and host assets"). The deferral is therefore superseded by campaign admission; it is a documented-claim conflict, not a code conflict.

**Live authority.**

- §CLI capability surface (wasm row): `radix/docs/design/target-capability-matrix.md:101` — `| wasm | yes | yes | no | no | MIR |` (run/package cells `no`; the surface is sourced from `tool/commands/targets.rs` per `matrix:88-89`, i.e. Row 1's row).
- §Browser Application Product Packaging contract: `matrix:199-208` — "the **`web` package product** is a `faber` product workflow over Radix's existing **HIR → TypeScript** emit"; "`[build] target = "web"` is **not** an accepted package target"; "no `faber build --target web` codegen peer".
- `ts` unchanged note: `matrix:218-220` — "the `ts` row in the CLI capability surface above stays `run=no`, `package=no` — the browser product is a `faber` packaging workflow over the existing TS lane".
- Deferral row: `matrix:229-233` — "| Wasm | Wasm remains out of the default target; reopen when browser-Wasm execution is a specified deliverable. |"
- Reopen event: `faber/docs/factory/browser-wasm-product/delivery-stage0.md:98-100` — "Known stale claims (must be recorded in D3)": includes "`target-capability-matrix.md` §Browser Application Product Packaging wasm deferral".

**Proposed reconciliation owner.** **Stage 3/8 (product) — radix docs matrix**: update the wasm deferral row to a reopen record citing this campaign, and reconcile the §CLI surface `wasm` package cell with Row 1's repair. Radix docs are edited at the owner-appropriate stage (Stage 3 product work per CAMPAIGN; matrix repair is the same `zombie-docs` item as Row 1). The §Browser Application Product Packaging §"`ts` target unchanged" prose will need a sibling update when the wasm product recipe replaces the TS route (Stage 8 clean break).

---

## Row 4 — `faber-web/README.md` architecture law ("HIR → TypeScript emit") vs this campaign's Wasm product goal

**Conflict.** `faber-web/README.md` states an **architecture law** — browser apps use Radix's HIR → TypeScript emit plus faber product packaging, and faber-web is "not a Radix `Target::Web`". The campaign replaces the browser product route's TS/`tsc` recipe with Wasm artifacts + host assets. This is a **supersession recorded, not a code conflict**: faber-web's binding contracts (`web:web`/`web:dom`) survive target-neutralized (CAMPAIGN track "`faber-web` | TypeScript-oriented bindings and shims | make contracts target-neutral and add Wasm host mapping"); what changes is the emit route the law names.

**Live authority.**

- Architecture law: `faber-web/README.md:5-8` — "Architecture law: browser apps use Radix's existing HIR → TypeScript emit plus `faber` product packaging. This package provides imported framework meaning such as `WebController` and `web:dom`; it is not a Radix `Target::Web` and does not make `faber build --target web` a codegen peer…".
- Evidence/deferral pointers: `faber-web/README.md:38-40` — links to matrix §Browser Application Product Packaging and the WEB6 delivery spec (§ Product Claims And Reciprocity).
- Campaign tracks (supersession): `CAMPAIGN.md:191` — "Browser product recipe | TypeScript plus `tsc` | replace with Wasm artifacts and host assets"; `CAMPAIGN.md:192` — "`faber-web` | TypeScript-oriented bindings and shims | make contracts target-neutral and add Wasm host mapping".
- Delivery baseline: `delivery-stage0.md:70-74` — faber-web TS bindings are the live browser product contract today (`src/dom.fab`, `src/canvas2d.fab`, `src/web.fab`, `bindings/ts.toml`).

**Proposed reconciliation owner.** **Stage 3 (product) — `faber-web` contracts + README**: when the wasm browser-app recipe lands, amend the README architecture law to name the Wasm route (HIR → MIR → Wasm via the package-aware builder, per Row 1) while keeping the "not a Radix `Target::Web`" boundary, and neutralize the bindings contracts (`CAMPAIGN.md:192`). The law itself is a boundary claim this campaign intentionally supersedes — recorded here so the Stage 8 clean break does not silently rewrite an architecture law.

---

## Validation transcript (captured)

### (a) `./target/debug/faber targets` — prebuilt binary

```text
rust available=yes check=yes build=yes run=yes package=yes note=primary backend; full package build + run via `faber`
fhir available=yes check=yes build=yes run=yes package=yes note=portable FHIR package envelope via `faber build --target fhir`; load and lower to FMIR for run (Rust stays explicit)
fmir-text available=yes check=yes build=yes run=yes package=yes note=faber package MIR image target: `faber build --target fmir-text`
fmir available=yes check=yes build=yes run=yes package=yes note=faber package MIR image target: `faber build --target fmir`
fmir-bin available=yes check=yes build=yes run=yes package=yes note=faber package MIR image target: `faber build --target fmir-bin`
faber available=yes check=yes build=yes run=no package=no note=canonical Faber re-emission for compiler inspection; package compilation not supported
go available=yes check=yes build=yes run=no package=no note=file emission supported; package compilation not yet supported
wasm available=yes check=yes build=yes run=no package=no note=supported MIR-backed Wasm binary emit (fail-closed subset); external host + faber_* imports; not faber run/package
wasm-text available=yes check=yes build=yes run=no package=no note=supported MIR-backed WAT emit (fail-closed subset); external host + faber_* imports; not faber run/package
llvm-text available=yes check=yes build=yes run=yes package=yes note=CUDA device execution at RC level on NVIDIA RTX 5070 / pharos (faber 1.6.0-rc.1, pinned revisions; E6/E7 receipts): `faber run --backend cuda` + archive compiled FMIR images; `-t llvm-text` remains emit-only
llvm-host available=yes check=yes build=yes run=yes package=yes note=native host executable via `faber build/run --target llvm-host` (MIR-to-LLVM emitter + llvm-as/clang + faber-host-llvm runtime archive on the local host triple)
metal-text available=yes check=yes build=yes run=yes package=yes note=Metal device execution at RC level on Apple M5 Max / burgus (faber 1.6.0-rc.1, pinned revisions; E6/E7 receipts): `faber run --backend metal` + archive compiled FMIR images; `-t metal-text` remains emit-only
wgsl-text available=yes check=yes build=yes run=no package=no note=supported MIR-backed WGSL compute shader source (fail-closed device-safe subset); external naga validate; not GPU launch or package runtime
sexp available=yes check=yes build=yes run=no package=no note=supported MIR-backed Racket validation target (fail-closed subset); run via external racket; not a package runtime
ts available=yes check=yes build=yes run=no package=no note=file emission supported; package compilation not yet supported
device-runtime available=yes check=no build=no run=no package=no note=device runtime support surface (faber-runtime + package/device modules); compiled only under the device-runtime feature
host-macos-arm64 available=yes check=no build=no run=no package=no note=native host leaf (Metal/CUDA host session support); compiled only under the host-macos-arm64 feature
host-wasm available=yes check=no build=no run=no package=no note=wasm host leaf (browser host session support); compiled only under the host-wasm feature
```

### (b) rg line authorities

```text
$ rg -n "MirWasmBinary" radix/crates/radix/src/tool/commands/targets.rs faber/src/package/manifest.rs
radix/crates/radix/src/tool/commands/targets.rs:18:    crate::codegen::Target::MirWasmBinary,
radix/crates/radix/src/tool/commands/targets.rs:79:        crate::codegen::Target::MirWasmBinary => "wasm",
radix/crates/radix/src/tool/commands/targets.rs:104:        crate::codegen::Target::MirWasmBinary => "wasm",
radix/crates/radix/src/tool/commands/targets.rs:195:        crate::codegen::Target::MirWasmBinary => TargetCapabilities {
```

(`manifest.rs` has no `MirWasmBinary` occurrence — that absence is Row 2's authority: the manifest surface never names the wasm target.)

### (c) Live wasm package probe (this unit; re-run of U2's probe, delivery §4 soft edge)

```text
$ ./target/debug/faber build --target wasm --package .    # fresh two-unit package
/private/tmp/u3-wasm-probe/target/faber/wasm
EXIT=0
$ find target -type f
target/faber/wasm/000-u3-wasm-probe-auxilium.wat
target/faber/wasm/001-u3-wasm-probe-root.wat
```

### (d) Live manifest rejection probe (this unit)

```text
$ ./target/debug/faber build --package .                  # [build] target = "wasm" in faber.toml
error: faber.toml build.target 'wasm' is not supported for package builds
EXIT=1
```

## Validation status

Delivery U3 `validation`: `./target/debug/faber targets` captured above (prebuilt binary); `rg -n "MirWasmBinary"` on the two named files captured above. **No cargo** — none run. All four done-when rows carry live authority (file:line + observed command) and a proposed reconciliation owner. Non-goals honored: no capability table/doc edits; `src/cli/*`, `src/commands/format*`, and `src/package/mir/*` foreign WIP untouched.
