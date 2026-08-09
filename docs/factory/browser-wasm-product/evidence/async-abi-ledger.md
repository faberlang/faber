# Async ABI Ledger — Stage 0 U5

**Unit**: `bwp-s0-u5-async-abi-ledger` (delivery-stage0.md §U5, lines 212–227)
**Campaign**: BROWSER-WASM-PRODUCT Stage 0 — baseline, ownership, boundary lock
**Status**: delivered (evidence captured 2026-08-09)
**Hand**: hand-7 (tugboat, task `f5f196bb`)
**Predecessor**: `042f26c` (auditor-5 P2 residual fold into Stage 0 docs)

## Capture environment

| Item | Value |
| --- | --- |
| workspace | `/Users/ianzepp/work/faberlang` (faber, faber-web, radix, hosts sibling repos) |
| faber tree HEAD at capture | `e5764f6` (`docs(factory): browser-wasm Stage 0 U4 — host JS allowlist + byte baseline evidence`) |
| capture date | 2026-08-09 |
| toolchain | `rg`/`sed` only — **no cargo** (delivery §4 cargo discipline; nothing to compile) |

This ledger names the **async ABI contract the browser host will expose** for the
browser-Wasm product route, per CAMPAIGN §Stage 0 gate bullet ("the async ABI
ledger names operation identifiers, dispatcher export, status and payload
records, cancellation race behavior, non-reentry rule, ordering, device-loss
delivery, and which future-valued routes are admitted or deferred").

**Scope discipline (U5 non_goals):** this document records contract *names* for
Stage 2 (`hosts/webgpu-browser`) contract admission. **No ABI code or constant
edits** — `radix-host-abi` stays untouched. There is no new ABI dialect: every
row below is expressed in the idioms of the existing `__faber_rt_v1_*` closed
surface (versioned import module, `i32` status codes, status-first multi-value
payloads, opaque integer handles). Spellings marked **[provisional]** are open
questions owned by the Stage 2 host contract (delivery-stage0.md §9 item 2),
not admitted constants.

## 1. Authority grounding — the `__faber_rt_v1_*` closed surface

Measured facts that anchor every ledger row. Ground-truth hierarchy: live code
first (CAMPAIGN §Ground Truth Hierarchy).

| Fact | Live authority |
| --- | --- |
| host-v1 import **module name** `faber_rt_v1` | `radix/crates/radix-mir-wasm/src/import_names.rs:40` — `pub(crate) const WASM_IMPORT_HOST_V1: &str = "faber_rt_v1";`; import emission at `radix/crates/radix-mir-wasm/src/program.rs:16-23` |
| **`__faber_rt_v1_*` symbol family** (imports the host must provide) | `radix/crates/radix-host-abi/src/lib.rs` `SYMBOL_ROWS` (the closed table) |
| ABI **version** / surface name | `lib.rs:24-27` — `ABI_VERSION: u32 = 1`, `ABI_SURFACE_NAME = "faber-rt-v1"` |
| **status idiom** `FaberRtStatusV1 { code: i32 }` | `lib.rs:438-443` (`LLVM_STATUS_TYPE` rows) + `lib.rs:499-514` `STATUS_CODES` (`STATUS_OK=0 … STATUS_FALLIBLE=5`) |
| **status-first multi-value** `(i32 status, payload…)` | `lib.rs:505-514` P10 comment — the shared failable status model ("status-first multi-value `(i32 status, payload…)` of the W10 profile") |
| **`VALUE_KIND_*` payload tagging** | `lib.rs:448-467` `VALUE_KIND_ROWS` (i1…ascii, text, valor, instans) |
| genus **field-layout extraction table** (`__faber_rt_v1_valor_get_genus`) | `lib.rs:252-266` P7 comment — parallel arrays `(context, valor, count, names, kinds, policies, outputs) → status` |
| **`⇥` recovery family** (`*_or` rows) | `lib.rs:199-210` P6 comment — closed-set recovery, "substitutes the fallback on a missing or wrong-typed payload instead of aborting" |
| **existing async-flavored host row** `__faber_rt_v1_tempus_wait` | `lib.rs:231-237` P3 comment (`dormiet`/`expectet` wait → closed-set host row taking i64 ms); wasm leaf wiring `radix/crates/radix-mir-wasm/src/calls.rs:826-838`; import signature `(import "faber_rt_v1" "__faber_rt_v1_tempus_wait" (func $__faber_rt_v1_tempus_wait (param i64)))` at `radix/crates/radix-mir-wasm/tests/import_signatures_test.rs:2570` |
| **promissum carrier** exists in shared MIR (async boundary is real, not invented) | `radix/docs/factory/wasm-host-parity/promotion-packet-p3-frontend-async-promissum.md` (resolved); `radix/stdlib/locale/la/pack.toml:2169-2201` (promissum ↔ async/sync boundary diagnostics) |
| **module exports today** (the async dispatcher would join these) | `incipit` entry + `__faber_external_*` cross-module exports — U2 baseline `evidence/faber-wasm-package-baseline.md`; closed `faber_rt_v1` + `faber_external` import sets |

Key consequence of the closed surface: the browser host is **not** free to
invent a second async dialect. The `faber_rt_v1` module and `__faber_rt_v1_*`
names are the only crossing points; the async contract below is a *shape* over
those existing idioms (new names recorded provisionally for Stage 2), exactly
as `tempus_wait` already demonstrates a host-implemented async-shaped row.

## 2. Async ABI contract ledger

Rows R1–R11. Each row: the contract the browser host will expose, the CAMPAIGN
§Async host law bullet it implements, and the closed-surface grounding.

### R1 — Operation identifiers: allocation rule

- **Contract.** Every async start (an `@ futura`-marked import call on the
  `faber_rt_v1` surface) returns an opaque `i32` operation identifier. The
  host allocates ids from a **monotonic per-instance counter** over the
  positive `i32` range. **`0` and negative ids are reserved**: `0` is the
  reserved id for unsolicited/system events (R10); negative values are invalid
  (the host never emits them and rejects a module that returns one). Ids are
  never reused within one module-instance lifetime (2³¹−1 usable ids; no
  practical wrap). The id is opaque to the module — its only valid uses are
  matching a later completion record (R4/R7) and cancellation (R8).
- **Law.** CAMPAIGN §Async host law bullet 1: "async starts return opaque
  `i32` operation identifiers".
- **Grounding.** `i32` matches the existing integer-idiom surface
  (`FaberRtStatusV1 { code: i32 }`, `lib.rs:438-443`); the opaque-handle
  precedent is the reserved discriminator idiom (`GradientWgslHandle(u32)`,
  `lib.rs:108-110`, `GRADIENT_WGSL_HANDLE_TYPE`). Host-side allocation, like
  `tempus_wait`'s host-implemented wait, keeps all async machinery out of the
  module.

### R2 — Dispatcher export (provisional name)

- **Contract.** Promise completion is delivered **host → module** through
  **exactly one** versioned, typed Wasm dispatcher. The dispatcher is a module
  **export** the host invokes to hand one completion record to the instance
  that started the operation. Provisional name: **`__faber_rt_v1_async_dispatch`
  [provisional]** — follows the closed-surface spelling convention
  (`__faber_rt_v1_*`) and is marked provisional per delivery-stage0.md §9 item 2
  ("Async dispatcher export spelling (`faber_rt_v1_async_dispatch` provisional)
  — owned by Stage 2 host contract, recorded in U5 with an explicit provisional
  marker"). Versioning rides the existing surface: `ABI_VERSION = 1` /
  `ABI_SURFACE_NAME = "faber-rt-v1"` (`lib.rs:24-27`) — "versioned" is not a
  second versioning scheme. Exact signature is Stage 2 admission work; the
  record it carries is the status-first `(i32 status, payload…)` of R4.
- **Law.** CAMPAIGN §Async host law bullet 2: "Promise completion is queued and
  delivered through one versioned typed Wasm dispatcher only after the
  initiating import returns".
- **Grounding.** Export direction (host calls into the module) is the only
  interruption-safe direction: the host cannot interrupt a running module
  between imports, so delivery must be a *call the host makes when the module
  is at a delivery point* (R5). The `__faber_rt_v1_*` naming and `faber_rt_v1`
  module convention are the measured spelling rules (§1).

### R3 — Status codes: completed / failed / cancelled / device-lost

- **Contract.** Every operation reaches exactly one of four terminal statuses,
  expressed through the existing `i32` status-code idiom:
  `completed`, `failed`, `cancelled`, `device-lost`. **`completed` reuses
  `STATUS_OK` (0)** — a zero status already means success on this surface.
  The three non-ok discriminators are **[provisional] Stage 2 admission
  constants in the reserved range beyond the existing 0–5** (`STATUS_FALLIBLE
  = 5` is the last admitted code; the async discriminators must not collide,
  e.g. suggested `6`/`7`/`8` for failed/cancelled/device-lost — recorded here
  as a naming/layout contract, **not** added to `radix-host-abi`). `failed`
  means the operation's promise rejected or its result was undeliverable;
  `cancelled` is the terminal record for a best-effort-cancelled operation
  (R8); `device-lost` is the terminal record applied to every in-flight
  operation when the device is lost (R10).
- **Law.** CAMPAIGN §Async host law bullet 4: "every operation reaches exactly
  one completed, failed, cancelled, or device-lost terminal state". Supporting
  boundary-budget bullet: "report traps, promise completion, device loss, and
  browser errors through the declared contract" (CAMPAIGN §JavaScript Boundary
  Budget).
- **Grounding.** `STATUS_CODES` family (`lib.rs:499-514`, codes 0–5, i32);
  the reserved-high-range proposal avoids collision with all admitted codes and
  keeps the failable status model's status-first placement intact (P10,
  `lib.rs:505-514`).

### R4 — Payload record shape

- **Contract.** The dispatch record is the existing **status-first
  multi-value `(i32 status, payload…)`** of the P10 failable model. On
  `completed`, the payload is the operation's typed carrier, read through the
  existing closed-set extraction rows — genus-shaped results via the
  `__faber_rt_v1_valor_get_genus` field-layout table (parallel `names` /
  `kinds` / `policies` / pre-seeded `outputs` arrays; missing mandatory key →
  non-ok status latched for `⇥` recovery), scalar/valor results via the
  `VALUE_KIND_*`-tagged valor rows. The `⇥` fallback of a future-valued call
  site (e.g. `FetchResponse ⇥ textus`) binds to the P6 `*_or` recovery family
  (`__faber_rt_v1_valor_get_text_or` and siblings): on a missing or
  wrong-typed payload the host substitutes the fallback instead of aborting.
  On `failed`, the payload is the P10 `ReturnError` carrier (the `cape err`
  recovery read). On `cancelled` / `device-lost` the payload is the vacuum
  carrier (the P3 `tempus_wait` return shape — no result value exists).
- **Law.** CAMPAIGN §Async host law bullet 2 ("typed Wasm dispatcher") with
  bullet 4 (payload is per-terminal-state); the failure channel is the P10
  failable model the closed surface already owns.
- **Grounding.** P10 status-first model (`lib.rs:505-514`); P7 genus table
  (`lib.rs:252-266`); P6 `*_or` recovery rows (`lib.rs:199-210`);
  `VALUE_KIND_ROWS` (`lib.rs:448-467`); the concrete future-valued carrier
  `FetchResponse ⇥ textus` at `faber-web/src/dom.fab:291` (§4).

### R5 — Queue-after-import-returns

- **Contract.** A Promise completion may arrive at the host at any time, but
  delivery through the dispatcher **begins only after the initiating import
  has returned** to the module. The host queues the `(op_id, status, payload)`
  record and never invokes the dispatcher while the module is inside an import
  call. Consequences: (a) the module cannot be interrupted mid-import; (b) the
  queue drains only when the module is at a delivery point — after an import
  returns and at the dispatcher entry; (c) queue order is the host-observed
  order (FIFO of settlement observation), which the serialized dispatcher
  preserves (R6).
- **Law.** CAMPAIGN §Async host law bullet 2: "Promise completion is queued and
  delivered through one versioned typed Wasm dispatcher **only after the
  initiating import returns**".
- **Grounding.** The initiating-import pattern is the existing call shape
  (`tempus_wait` is an import the module calls and the host services,
  `calls.rs:826-838`); the queue is a host-side data structure (no new ABI
  surface needed to queue — the ledger records the rule).

### R6 — Serialized, non-reentrant delivery

- **Contract.** The dispatcher is **serialized** — one completion record is
  delivered at a time; no second record is delivered until the first dispatch
  call returns. It is **non-reentrant** — while a dispatch is in progress, no
  nested or re-entrant dispatch can occur, including: the module must not call
  the dispatcher from inside an import handler, and an import the module makes
  while servicing a dispatch must not trigger a re-entrant delivery. The host
  enforces both; a violation is a host-contract error (fail closed, §4 policy).
- **Law.** CAMPAIGN §Async host law bullet 3: "the dispatcher is serialized and
  non-reentrant".
- **Grounding.** No new dialect: serialization is a host-side invariant over
  the one dispatcher export (R2); the pattern matches the single-writer
  discipline the closed surface already uses for status-first multi-value
  returns.

### R7 — Exactly-one-terminal-result

- **Contract.** Per `op_id`, the host delivers **exactly one** terminal record
  (`completed` | `failed` | `cancelled` | `device-lost`). After a terminal
  record is delivered, any further settlement for that id is **dropped at the
  host boundary** — the dispatcher is never invoked a second time for the same
  operation (overlaps with R8's late-result discard; R7 is the per-op guarantee
  on the dispatcher, R8 is the race rule on the Promise side). The module can
  rely on: one dispatch per op, no duplicates, no missing terminal for any
  started op (unless the whole instance dies, which is a host failure outside
  the per-op contract).
- **Law.** CAMPAIGN §Async host law bullet 4: "every operation reaches exactly
  one completed, failed, cancelled, or device-lost terminal state".
- **Grounding.** The status-first shape already returns exactly one
  `(status, payload)` pair per call (P10); the "one terminal per op" rule is
  the per-operation instance of that idiom.

### R8 — Best-effort cancellation with late-result discard

- **Contract.** Cancellation is **best-effort**: the host forwards a cancel
  request to the underlying browser promise when a cancel mechanism exists,
  but the promise may still settle (the browser provides no cancel for some
  operations — e.g. `fetch` in-flight). The cancel is acknowledged as a
  `cancelled` terminal record when the host can commit to the operation being
  dead to the module. **Late results never re-enter Wasm**: a Promise result
  that settles after the op's terminal record (cancelled, or any terminal) is
  discarded at the host boundary and never delivered to the module — no
  post-terminal dispatch, no race window where a stale result reaches Faber.
- **Law.** CAMPAIGN §Async host law bullet 5: "cancellation is best-effort, and
  late Promise results never re-enter Wasm".
- **Grounding.** "Never re-enter Wasm" is a host-boundary discard rule; the
  `⇥`/`_or` recovery family (`lib.rs:199-210`) shows the closed surface already
  routes fallback-shaped outcomes through closed-set rows rather than
  aborting — the late-result discard is the async analogue (drop, never
  deliver).

### R9 — Browser delivery order preserved; no fabricated total order

- **Contract.** (a) **Frame and subscription events** (animation frame,
  input, resize, subscription callbacks) are delivered through the contract in
  the order the browser delivered them — the host is a transport, not a
  scheduler, and must not reorder browser-order events. (b) **Independent
  Promise operations have no fabricated total order**: the host must not impose
  an arbitrary or deterministic-but-fabricated ordering on independent
  operations (no sorting by id, no artificial sequencing); their completions
  are delivered in host-observed order, and any observation order is valid.
  The serialized dispatcher (R6) constrains the *mechanism* (one call at a
  time), never the *logical order* of independent results. (c) The R5 queue
  preserves, within its drain discipline, the order in which records were
  observed.
- **Law.** CAMPAIGN §Async host law bullet 6: "frame and subscription order
  follow browser delivery order. Independent Promise operations have no
  fabricated total order".
- **Grounding.** The current JS route already treats browser order as
  authoritative (rAF loop, `onDeviceLost` — §5 reference evidence); the Wasm
  contract makes that an explicit non-reordering rule instead of JS policy.

### R10 — Device-loss and unsolicited events through the same typed path

- **Contract.** Browser **unsolicited events** — device loss (WebGPU
  `device.lost`), context loss, trap/browser-error reports — are delivered
  through the **same typed dispatcher** (R2), not a second channel. An
  unsolicited event carries the **reserved `op_id = 0`** (R1) plus a typed
  status/payload record (`device-lost` → `ASYNC_STATUS_DEVICE_LOST` with the
  reason carrier; trap reports → `failed`-shaped with the diagnostic carrier
  via the closed diagnostic surface `DIAGNOSTIC_SYMBOLS_V1`). Device loss also
  terminates every in-flight operation: each open op receives `device-lost`
  as its exactly-one terminal record (R3/R7). Ops not yet started are not
  promised — the host fails them closed or defers them per the Stage 2
  lifecycle contract; no op is left without a terminal.
- **Law.** CAMPAIGN §Async host law bullet 4 (device-lost is one of the four
  terminal states — and it is, alone among them, *unsolicited* for most ops),
  plus the boundary-budget bullet "report traps, promise completion, device
  loss, and browser errors through the declared contract" (CAMPAIGN §JavaScript
  Boundary Budget).
- **Grounding.** The current JS route already models device-lost as a typed
  lifecycle state (`startup → ready → suspended → device-lost → recovering →
  failed`, `hosts/webgpu-browser/public/src/engine/engine.js:14,411-424`) and
  surfaces it via `onDeviceLost` (`engine.js:783-790`); the Wasm contract keeps
  the typed path but moves it into the dispatcher. The diagnostic carrier
  surface (`lib.rs:519-543`) is the closed channel for browser-error reports.

### R11 — Future-valued routes: admitted / deferred table

- **Contract.** Every future-valued route (a call whose result is not
  available synchronously) uses this async contract **or fails closed**.
  "Admitted" = the route has a binding to the op-id start + typed dispatch
  contract and is exported to Faber modules. "Deferred / not admitted" = no
  binding exists; the browser Wasm host provides **no** import for the route,
  and any request for it is **rejected clearly** (fail closed) — no
  TypeScript fallback, no partial synchronous shim, per CAMPAIGN §Development
  Posture ("Unsupported imports … reject clearly. No TypeScript fallback").
- **Law.** CAMPAIGN §Async host law bullet 7: "`fetch_text` and other
  future-valued routes use this contract or fail closed".
- **Grounding.** The concrete future-valued surface today is `web:dom.fetch_text`
  (measured, §4); the fail-closed posture is the campaign's locked Development
  Posture row. Table:

| Route | Source declaration | Current status | Disposition |
| --- | --- | --- | --- |
| `web:dom.fetch_text` | `faber-web/src/dom.fab:291` — `@ futura functio fetch_text(FetchRequest request) → FetchResponse ⇥ textus` (carrier genera `FetchRequest`/`FetchResponse`, `dom.fab:84-94`); TS binding `faber-web/bindings/ts.toml:153-157` (`webDomFetchText`); runtime shim `faber-web/runtime/dom.ts` | **TS route only.** Known async codegen gap: "the Radix TS backend does not await `@ futura` calls inside `fac`/`cape` blocks, so `dom.fetch_text` is exercised at the runtime-bridge level in the WEB5 fixture until the async codegen gap closes" (`faber-web/README.md:42-44`) | **Deferred — not admitted.** Admitted only if a contract routes it through this async ABI (op-id start + typed dispatch, R1/R2/R4). Default: not admitted → the browser-Wasm host provides no async fetch import and the request fails closed |
| (any other `@ futura` route) | none present in the scan (measured, §4) | n/a | **Policy row:** any new future-valued route must be admitted through this contract or fail closed — same rule, no per-route exceptions |

## 3. Traceability table — ledger rows → CAMPAIGN §Async host law bullets

CAMPAIGN §Async host law appears at CAMPAIGN.md lines 112–124 (the seven
bullets), inside §JavaScript Boundary Budget.

| Ledger row | CAMPAIGN §Async host law bullet (verbatim) | Boundary-budget support |
| --- | --- | --- |
| R1 operation ids | bullet 1 — "async starts return opaque `i32` operation identifiers" | — |
| R2 dispatcher export | bullet 2 — "Promise completion is queued and delivered through one versioned typed Wasm dispatcher only after the initiating import returns" | — |
| R3 status codes | bullet 4 — "every operation reaches exactly one completed, failed, cancelled, or device-lost terminal state" | "report traps, promise completion, device loss, and browser errors through the declared contract" |
| R4 payload record | bullet 2 (typed dispatcher) + bullet 4 (payload per terminal state) | — |
| R5 queue-after-import-returns | bullet 2 — "…only after the initiating import returns" | — |
| R6 serialized non-reentrant | bullet 3 — "the dispatcher is serialized and non-reentrant" | — |
| R7 exactly-one-terminal | bullet 4 — "every operation reaches exactly one … terminal state" | — |
| R8 cancellation + late discard | bullet 5 — "cancellation is best-effort, and late Promise results never re-enter Wasm" | — |
| R9 delivery order | bullet 6 — "frame and subscription order follow browser delivery order. Independent Promise operations have no fabricated total order" | — |
| R10 device-loss + unsolicited | bullet 4 (device-lost terminal state) | "report traps, promise completion, device loss, and browser errors through the declared contract" |
| R11 future-valued routes | bullet 7 — "`fetch_text` and other future-valued routes use this contract or fail closed" | "Fail closed. Unsupported imports … reject clearly. No TypeScript fallback" (CAMPAIGN §Development Posture) |

Every ledger row maps to at least one §Async host law bullet; no bullet is left
unimplemented by a row.

## 4. `fetch_text` status confirmation (validation evidence)

Exact validation command, run 2026-08-09 from `/Users/ianzepp/work/faberlang`:

```sh
rg -n "fetch_text|fetchText" faber-web radix --glob '!**/target/**'
```

Result — 3 hits, all in `faber-web`, none in `radix`, no camelCase `fetchText`:

```
faber-web/src/dom.fab:291:functio fetch_text(FetchRequest request) → FetchResponse ⇥ textus {
faber-web/bindings/ts.toml:153:[functions."web:dom.fetch_text"]
faber-web/README.md:43:`fac`/`cape` blocks, so `dom.fetch_text` is exercised at the runtime-bridge
```

Confirmed current status: `fetch_text` is declared on the **faber-web contract
surface only** (source declaration + TS binding + recorded TS-async gap). There
is **no radix-side async dispatch surface** and no camelCase spelling anywhere.
Supplementary scan `rg -n "futura" faber-web …` confirms `@ futura` appears in
the faber-web source surface only at `dom.fab:290` — `fetch_text` is the **only**
future-valued route declared today. The radix grammar still marks `@ futura`
as the async annotation (`radix/EBNF.md:161,204,1073`) and the promissum
carrier/async-boundary diagnostics exist in the shared surface
(`radix/stdlib/locale/la/pack.toml:2169-2201`), so the async *language* concept
is real — only the browser-Wasm *routing* is not admitted yet (R11).

## 5. Current JS async behavior — reference evidence (read-only)

The current browser route owns asynchrony in JavaScript policy (reference
evidence for the contract above; disposition of these lanes is U4's
`host-js-allowlist.md`):

- Session/device-lost state machine `startup → ready → suspended → device-lost →
  recovering → failed` — `hosts/webgpu-browser/public/src/engine/engine.js:14`
  (header) and `engine.js:411-424` (transition table); device loss surfaced via
  `onDeviceLost` callback at `engine.js:783-790` → typed `device-lost` state.
- Frame loop via `requestAnimationFrame` — `engine.js` frame-scheduler lane
  `frame-scheduler.js:168,193`; GPU readback via `await
  device.queue.onSubmittedWorkDone()` + `mapAsync(GPUMapMode.READ)`
  (`frame-scheduler.js:92-93`).
- Direct `await` / `Promise.all` usage across the proof page and engine
  (`public/src/app.js:41,51-61,96-157`; `engine.js:108-129`), including
  `onDeviceLost(device, …)` registration (`app.js:119`).

Relevance: the current route delivers promise completion, frame order, and
device loss as **JS policy** (direct awaits, callbacks, a JS state machine).
The R1–R11 contract is the Wasm-side replacement — the same facts moved behind
the `__faber_rt_v1_*` closed surface, with the JS host reduced to transport
(queue + typed dispatch + raw capability calls). Per the CAMPAIGN JS boundary
budget, scheduling/ordering policy is **not** allowed in JavaScript in the
target route.

## 6. Validation commands run (this unit)

```sh
cd /Users/ianzepp/work/faberlang
rg -n "fetch_text|fetchText" faber-web radix --glob '!**/target/**'   # -> 3 hits (§4); current status confirmed
rg -n "futura" faber-web radix faber --glob '!**/target/**' -i        # -> fetch_text is the only faber-web @ futura route
rg -n "tempus_wait|TEMPUS_WAIT" radix/crates/radix-mir-wasm/src radix/crates/radix-host-abi/src   # -> closed-surface async row + wasm wiring
rg -n "WASM_IMPORT_HOST_V1" radix/crates/radix-mir-wasm/src/import_names.rs                        # -> "faber_rt_v1" module name
sed -n '1,40p' hosts/webgpu-browser/public/src/engine/engine.js        # -> JS reference async/device-lost evidence
```

Cross-check result: every ledger row maps to one CAMPAIGN §Async host law
bullet (traceability table §3); the `fetch_text`/`fetchText` scan confirms the
route is faber-web-declared only and **deferred/not admitted** in the
browser-Wasm contract (§4). **No cargo** was invoked; **no ABI code or
constant was edited** (`radix-host-abi` untouched); no sibling-repo file was
modified.
