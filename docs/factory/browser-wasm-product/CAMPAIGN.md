# Campaign: Browser Wasm Product

**Status**: active — Stage 0 CLOSED (U0–U7 landed, audit clean_pass, phase-close council complete, closeout 1ece368); Stage 1 spec lowered (3b36fb8) pending admission (audit revise 2 P2s + council corrections queued)
**Mode**: routing artifact — does not implement code directly
**Date**: 2026-08-09
**Control-plane repo**: `/Users/ianzepp/work/faberlang/faber`
**Goal**: [goal.md](goal.md)
**Consumes**: [Wasm Host Parity](../../../../radix/docs/factory/wasm-host-parity/CAMPAIGN.md) and [Direct Device Product Pipeline](../direct-device-product-pipeline/CAMPAIGN.md)

## Summary

This campaign replaces the TypeScript browser-application product route with a
normal Faber package-to-core-Wasm route:

```text
Faber package + Triga + faber-web
  → shared frontend
  → validated MIR
  → core Wasm module set
  → thin browser capability host
  → DOM and WebGPU behavior
```

One cross-repo campaign lives in Faber because Faber owns the browser-app
product recipe, package graph, artifact layout, target selection, and build
command. Owner-repo delivery specs will be created only when a stage is
selected. This avoids duplicate campaign state in Radix, `faber-web`, `hosts`,
and Triga.

## Desired End State

1. Faber's browser-app product recipe selects the ordinary `wasm` compiler
   target and builds packages without `tsc`.
2. Application and library behavior runs in Wasm, including controllers,
   scene state, geometry, transforms, animation, and renderer policy.
3. A shared browser host contains only module loading, object handles,
   lifecycle callbacks, raw DOM/WebGPU operations, and declared asset fetch.
4. `faber-web` is expressed as target-neutral Faber contracts plus a
   browser-Wasm binding map, with no required TypeScript runtime.
5. Triga provides static, animated, and interactive browser WebGPU acceptance
   proofs with observed pixels and frame progress.
6. Stable metadata has one owner. Redundant generated TypeScript and JSON are
   removed.
7. The TypeScript browser-app recipe is removed after the accepted release
   checkpoint. The explicit `ts` code-generation target is not part of that
   removal.

## Development Posture

- **Normal target path.** Extend validated MIR and the existing Wasm target;
  do not create a browser compiler shortcut.
- **Thin host.** Browser capabilities stay in JavaScript only when the browser
  API demands JavaScript objects or callbacks.
- **Policy in Wasm.** If code decides what the application or renderer should
  do, it belongs in Faber/Wasm.
- **One contract.** Consume the existing runtime ABI and package module graph.
- **Fail closed.** Unsupported imports, value shapes, or lifecycle operations
  reject clearly. No TypeScript fallback.
- **Submission boundary.** DOM and setup calls may be direct. WebGPU draw and
  dispatch work crosses at compiled submission-region boundaries from the
  first graphics proof; coarser batching requires measured need.
- **Clean break.** Once the release gate is accepted, remove the old browser-
  app recipe rather than maintain two permanent product paths.

## Campaign Workflow

For each stage:

1. Mind selects the stage and names its owner repo.
2. A planner runs goal-forge against live source and predecessor evidence.
3. Goal-check returns `READY` before delivery planning.
4. Delivery names exact files, fixtures, commands, overlap, and done oracle.
5. Factory implements the accepted slice.
6. The owner repo records focused evidence. Cross-repo status changes only
   when the campaign gate is observed.
7. A blocked stage routes its missing fact to the earliest owner and advances
   another independent unit where possible.

This campaign never serves as an implementation task by itself.

## Scope Routing

| Surface | Owner | Campaign responsibility |
| --- | --- | --- |
| Shared semantic, layout, callable, callback, and ABI facts | Radix shared MIR | Supply target-neutral facts needed by more than one target. |
| Core-Wasm encoding, imports, exports, memory carriers, validation | Radix Wasm leaves | Consume or reject MIR facts without reconstructing source semantics. |
| Portable v1 CPU ABI and package module linking | Wasm Host Parity / `hosts/wasm` | Predecessor authority; this campaign consumes it. |
| Browser-app product recipe, artifact layout, serve/build UX, target truth | Faber | Make ordinary package-to-Wasm the browser product route. |
| Browser capabilities, handles, callbacks, WebGPU and DOM thunks | `hosts/webgpu-browser` | Keep the authored JavaScript host generic and bounded. |
| Target-neutral web contracts and binding declarations | `faber-web` | Remove the Wasm product's dependency on TS runtime shims. |
| Geometry and graphics behavior | Triga | Run library behavior in Wasm and supply product acceptance scenes. |
| Reusable app corpus and launch fixtures | `examples` when shared beyond Triga | Hold public package fixtures, not compiler-specific harness logic. |
| Shader program | WGSL producer and shader owner | Preserve WGSL; expose reflection as owned build facts. |

## JavaScript Boundary Budget

Allowed in the shared host:

- fetch and instantiate the declared Wasm module set;
- map opaque integer handles to browser DOM and WebGPU objects;
- translate browser events and animation-frame callbacks into versioned Wasm
  calls;
- issue raw DOM, canvas, WebGPU adapter/device, buffer, texture, pipeline,
  encoder, queue, and presentation operations selected by Wasm. Draw and
  dispatch selection arrives as compiled submission-region descriptors or
  command batches, not per-object JavaScript policy;
- copy declared byte ranges between Wasm memory and browser buffers where the
  platform requires it;
- report traps, promise completion, device loss, and browser errors through
  the declared contract.

Async host law:

- async starts return opaque `i32` operation identifiers;
- Promise completion is queued and delivered through one versioned typed Wasm
  dispatcher only after the initiating import returns;
- the dispatcher is serialized and non-reentrant;
- every operation reaches exactly one completed, failed, cancelled, or
  device-lost terminal state;
- cancellation is best-effort, and late Promise results never re-enter Wasm;
- frame and subscription order follow browser delivery order. Independent
  Promise operations have no fabricated total order;
- `fetch_text` and other future-valued routes use this contract or fail closed.

Not allowed in JavaScript:

- application or controller state machines;
- Triga math, geometry generation, scene construction, transforms, camera or
  animation policy;
- shader selection, material policy, render-pass selection, draw ordering, or
  resource lifetime policy;
- source-language semantics, hidden coercions, fallback execution, or parsing
  Faber output to recover compiler facts.

Stage 0 must turn this semantic budget into an exact file allowlist and a
measured baseline. Later stages may reduce the allowlist. Expanding it requires
an explicit campaign decision.

The product budget is also explicit:

- zero generated TypeScript bytes and zero `.ts`/`.tsx` files in promoted
  distributions;
- zero generated Faber/Triga application JavaScript modules;
- zero inline application JavaScript in promoted pages;
- zero private per-demo renderer/runtime copies and zero Three.js runtime
  dependency;
- at most 16 KiB unminified for any per-application loader or generated ABI
  adapter;
- zero runtime `controllers.json`, `draw.json`, or reflection JSON at closeout;
- zero served package/product/assets JSON unless a named runtime-dynamic
  consumer proves that the fact cannot be stable build metadata.

The shared generic browser host is measured separately from the per-application
adapter. Stage 0 records its current bytes and modules. The semantic allowlist,
not an arbitrary total-size promise, is its first hard gate.

## Batching And Split Policy

| Stage family | Posture | Split only on |
| --- | --- | --- |
| Baseline and boundary | discovery-first | ownership conflict or public product decision |
| MIR/Wasm reachability | split-on-boundary | shared MIR fact versus target encoding |
| Browser loader and host contract | one coherent admission slice | ABI change or browser ownership conflict |
| Browser operation families | batch-by-default after first pattern | DOM, lifecycle, WebGPU resource, or command family |
| Faber product recipe | split-on-boundary | compiler artifact versus product packaging |
| `faber-web` bindings | batch-by-default after first controller | distinct callback or capability family |
| Triga migration | controller, static, then animated/interactive | new missing compiler or host fact |
| Metadata cleanup | batch-by-owner | runtime consumer proves a distinct dynamic requirement |

Do not create one factory phase per browser method, ABI symbol, geometry, or
fixture. Prove one pattern, then complete its homogeneous family.

## Ground Truth Hierarchy

1. Live code and focused observed commands.
2. Current runtime contract tables and package tests.
3. Current target and product help.
4. Active factory goals and campaign evidence.
5. Historical prose and old capability matrices.

Stage 0 must name any conflict instead of selecting the more convenient claim.

## Current Tracks

| Track | Current state | Campaign action |
| --- | --- | --- |
| Radix core Wasm | valid for a bounded MIR subset | admit the shapes needed by browser controller and Triga proofs |
| Faber Wasm packages | package-aware builder persists WAT and retains binary modules in memory; locked source-library closure is incomplete | persist deterministic binary modules, close target-native libraries, and expose a browser-app recipe |
| Portable Wasm host | native module-set linker and v1 host exist | reuse contract; do not duplicate native lifecycle work |
| Browser product recipe | TypeScript plus `tsc` | replace with Wasm artifacts and host assets |
| `faber-web` | TypeScript-oriented bindings and shims | make contracts target-neutral and add Wasm host mapping |
| Browser WebGPU host | useful JavaScript/WebGPU proof exists | shrink to generic raw capability thunks |
| Triga browser proof | TypeScript/JavaScript product route | migrate behavior and acceptance to Wasm |
| Metadata | several generated formats and sidecars | assign owners, remove duplicates, pack stable facts |

## Campaign Path

### Stage 0 — Baseline, ownership, and boundary lock

**Status**: selected; ready for delivery planning.

**Purpose**: inventory every generated and authored TypeScript, JavaScript,
JSON, WGSL, and Wasm artifact in the current browser route; measure current
behavior; reconcile stale Wasm product claims; select the first controller and
  Triga static fixture; freeze the initial JavaScript allowlist.

**Depends on**: live main across Radix, Faber, `faber-web`, hosts, and Triga.

**Owner**: Faber control-plane planner, with read-only evidence from each repo.

**Overlap**: no product implementation. Do not edit current foreign compiler
or device work.

**Gate**:

- one checked-in inventory names artifact, producer, consumer, owner, whether
  it is runtime-required, and intended keep/move/remove disposition;
- current TypeScript browser behavior is recorded as reference evidence;
- the Faber baseline records that the current package builder persists `.wat`
  while retaining `.wasm` bytes in memory, and Stage 3 names the durable binary
  module-set and manifest output that replaces this limitation;
- every target capability conflict is named with live authority;
- the exact host JavaScript file allowlist and line/byte baseline are recorded;
- the async ABI ledger names operation identifiers, dispatcher export, status
  and payload records, cancellation race behavior, non-reentry rule, ordering,
  device-loss delivery, and which future-valued routes are admitted or
  deferred;
- `triga/corpus/webgl-geometries` is confirmed as the first vertical slice,
  followed by `webgl-geometry-terrain`, `webgl-animation-orbit`, and
  `webgl-animation-terrain`, with exact browser proof commands;
- no unresolved question changes the Stage 1 compiler or Stage 2 host boundary.

**Batch posture**: discovery-first.

**Lowers to**: goal-forge → goal-check → delivery. Delivery may place the
inventory in the repo that owns the browser product fixture, while this
campaign remains the status authority.

### Stage 1 — Browser-minimum MIR-to-Wasm reachability

**Status**: planned after Stage 0.

**Purpose**: make the selected controller and static Triga source shapes lower
through validated MIR to valid core Wasm. Expected families include package
calls, records and lists, option/error carriers, matrices or vectors, callable
identity, and callback exports; Stage 0 evidence decides the exact set.

**Depends on**: Stage 0 inventory and Wasm Host Parity's live contract.

**Owner**: Radix shared MIR for missing target-neutral facts; Radix Wasm leaves
for encoding, imports, exports, memory, and validation.

**Overlap**: shared MIR is single-writer. Wasm Host Parity continues owning
general CPU parity and the v1 catalog.

**Gate**: both selected packages emit and validate as a deterministic module
set, or each remaining failure is an explicit target-contract rejection with
one earliest owner. No source replay or TS fallback participates.

**Batch posture**: split-on-boundary; batch homogeneous Wasm carrier families.

**Lowers to**: owner-specific goal-forge → goal-check → delivery → factory.

### Stage 2 — Browser core-Wasm host admission

**Status**: planned after the needed Stage 1 slice.

**Purpose**: instantiate and link the Faber module set in a browser, bind the
existing v1 runtime imports, expose opaque DOM/WebGPU handle arenas, and prove
entry, trap, callback, and shutdown lifecycle.

**Depends on**: selected valid Wasm controller artifact; portable runtime
contract and module graph from Wasm Host Parity.

**Owner**: `hosts/webgpu-browser`.

**Overlap**: the browser host consumes the runtime contract. It does not edit
the logical ABI catalog without a separately routed contract delivery.

**Gate**: a hand-authored minimal loader instantiates the real generated module
set, invokes its entry, forwards one callback, rejects unknown imports, and
reports traps. One async fixture proves queued non-reentrant completion,
exactly one terminal result, cancellation race handling, typed device loss,
and callback-trap reporting. Host code stays inside the Stage 0 allowlist and
contains no app policy.

**Batch posture**: one coherent admission slice.

**Lowers to**: hosts goal-forge → goal-check → delivery → factory.

### Stage 3 — Faber browser-app Wasm product recipe

**Status**: planned after Stage 2 admission.

**Purpose**: make Faber package workflow assemble the Wasm module set, browser
host, WGSL, declared assets, and owned metadata into a deterministic browser
distribution without invoking the TypeScript toolchain.

This stage must dispatch a browser Wasm product before the generic Wasm package
fallback, persist real `.wasm` modules, and close locked source-library
dependencies through the Wasm target. It should reuse Faber's existing
preflight, collision, staging, quarantine, and atomic-publication machinery.

**Depends on**: Stage 2 loader contract and Stage 0 artifact ownership.

**Owner**: Faber product/package workflow.

**Overlap**: Radix emits artifacts only. Faber owns recipe, optional
postprocessing, distribution layout, build/serve UX, and target truth.

**Gate**:

- the normal command uses external target `wasm` for a browser-app product;
- output contains the declared Wasm, host, WGSL, and asset files;
- the module closure includes Triga, `faber-web`, and every selected locked
  source library in deterministic dependency order;
- output contains no generated `.ts`, generated app `.js`, or TypeScript
  configuration;
- the command transcript contains no `tsc` and no fallback;
- repeated builds produce the same manifest and artifact names;
- a failed rebuild preserves the previously published product and leaves no
  staging or quarantine residue.

**Batch posture**: split compiler artifacts from product packaging; keep the
recipe itself coherent.

**Lowers to**: Faber goal-forge → goal-check → delivery → factory.

### Stage 4 — `faber-web` target-neutral contracts

**Status**: planned after Stages 2–3.

**Purpose**: express DOM, canvas, events, animation frame, and browser error
operations as Faber-facing contracts with a Wasm binding map. Remove the Wasm
product's dependency on TypeScript runtime shims.

**Depends on**: admitted browser host contract and product recipe.

**Owner**: `faber-web` for public contracts; hosts for implementations; Radix
only when a genuinely target-neutral callable or effect fact is absent.

**Overlap**: no duplicate DOM model in Radix. No browser policy in the ABI.

**Gate**: the selected Faber controller mounts, writes observed DOM state,
receives an animation-frame or input callback, and surfaces a browser error
through Wasm with zero generated TypeScript.

**Batch posture**: prove one controller, then complete homogeneous DOM,
lifecycle, and event families.

**Lowers to**: `faber-web` and hosts delivery specs with serialized shared
contract edits.

### Stage 5 — Triga static WebGPU proof

**Status**: planned after Stage 4 and the required Stage 1 shapes.

**Purpose**: run `triga/corpus/webgl-geometries` and then
`webgl-geometry-terrain` geometry and scene setup in Wasm while the thin
host performs raw WebGPU operations selected by Wasm. Use compiler-owned
reflection facts and WGSL without duplicating policy in JavaScript.

**Depends on**: static fixture selection, valid Wasm package, browser WebGPU
capabilities, Faber product recipe.

**Owner**: Triga for geometry/scene and acceptance; hosts for raw WebGPU
thunks; Radix for separately routed lowering blockers.

**Overlap**: existing WebGPU JavaScript is reference evidence. Only generic
thunks may survive in the host allowlist.

**Gate**: selected Triga static geometries render from a fresh build; evidence
records module provenance, device setup, upload, non-background pixel readback,
expected geometry cases, and clean console diagnostics.

**Batch posture**: first one honest scene, then batch the declared static
geometry set.

**Lowers to**: Triga acceptance delivery plus any separately owned host or
compiler blockers.

### Stage 6 — Wasm-owned animation, input, and renderer policy

**Status**: planned after Stage 5.

**Purpose**: move the remaining application, animation, camera, scene, and
renderer decisions into Faber/Wasm. Keep JavaScript at raw browser operations.
Use the Direct Device Product Pipeline's submission-region boundary for draw
and dispatch. Add a coarser command surface only when measured boundary traffic
requires it.

**Depends on**: static proof and boundary measurements.

**Owner**: Triga and Faber application source for policy; hosts for generic
capability calls; Radix for explicit compiler blockers.

**Overlap**: any render command representation must be target-neutral enough
to avoid a JavaScript-only scene protocol.

**Gate**: `webgl-animation-orbit` and then `webgl-animation-terrain` advance
across measured frames, respond to declared input, preserve non-background
pixels, and have no app or renderer policy in JavaScript. Boundary call volume
and time are recorded before and after any batching.

**Batch posture**: scene state first, then input/lifecycle, then measured
batching if needed.

**Lowers to**: Triga, application fixture, and hosts deliveries by owner.

### Stage 7 — Metadata collapse and artifact cleanup

**Status**: planned after the first animated proof.

**Purpose**: remove redundant controller JSON, reflection JSON, generated
configuration, and compatibility artifacts. Keep only runtime-dynamic data
with named consumers.

**Depends on**: proven consumers from Stages 3–6.

**Owner**: the producer of each canonical fact; Faber owns product assembly.

**Overlap**: do not move compiler facts into handwritten host constants.

**Gate**: every remaining JSON file has a named dynamic consumer and rationale;
stable facts have one canonical representation; no TypeScript artifact or
compatibility manifest is needed to run the browser product.

**Batch posture**: batch by canonical owner.

**Lowers to**: owner-specific cleanup deliveries.

### Stage 8 — Product acceptance and clean break

**Status**: planned after Stages 5–7.

**Purpose**: run the complete browser proof, make product claims truthful, set
the release checkpoint, and retire the TypeScript browser-app recipe after
operator acceptance.

**Depends on**: controller, static, animated, input, metadata, and artifact
gates.

**Owner**: Faber product for recipe and help; Triga/examples for public proof;
Radix for target truth; operator for version and release decision.

**Overlap**: the explicit general `ts` codegen target is not removed unless a
separate goal authorizes it.

**Gate**:

- fresh normal build and serve path uses Wasm with no TypeScript fallback;
- controller DOM behavior, static geometry, animated frames, input, pixel
  readback, device state, and console state are recorded;
- host JavaScript stays within the accepted allowlist and budget;
- capability matrices, target help, package docs, and Triga instructions match
  observed behavior;
- operator accepts the version/migration decision;
- the old TypeScript browser-app recipe and its generated configuration are
  removed by clean break.

**Batch posture**: one product acceptance theme; release actions remain a
separate confirmation gate.

**Lowers to**: final acceptance delivery, then operator-owned release protocol
if explicitly authorized.

## Dependency Spine

```text
Stage 0 baseline
  ├── Stage 1 Wasm reachability ──┐
  └── host boundary facts ────────┤
                                  ▼
                         Stage 2 browser host
                                  │
                                  ▼
                         Stage 3 Faber recipe
                                  │
                                  ▼
                       Stage 4 faber-web contract
                                  │
                                  ▼
                       Stage 5 Triga static proof
                                  │
                                  ▼
                    Stage 6 animation/input/policy
                                  │
                                  ├── Stage 7 metadata collapse
                                  │
                                  ▼
                       Stage 8 clean-break acceptance
```

Stage 1 may continue in bounded shape families alongside Stages 2–4, but a
consumer stage cannot claim readiness until its exact artifact validates.

## Useful Milestones

1. **Controller milestone**: a Faber package builds to Wasm, loads in a
   browser, updates the DOM, and receives a callback with no `tsc`.
2. **Static Triga milestone**: real Triga geometry reaches WebGPU and produces
   verified non-background pixels.
3. **Application milestone**: a terrain, water, or orbit proof advances frames
   and handles input with application and renderer policy in Wasm.
4. **Product milestone**: the normal browser-app recipe is Wasm-only and the
   TypeScript browser-app recipe is retired after operator acceptance.

## Campaign Acceptance

- Every stage has a named owner, dependency, overlap rule, gate, batching
  posture, and lowering route.
- The campaign consumes Wasm Host Parity rather than duplicating its ABI,
  package linker, native host, or corpus-parity work.
- The thin-host boundary is testable and excludes application/library policy.
- The path uses normal Faber package workflow and the ordinary Radix `wasm`
  target.
- Browser proof requires output/readback, animation progress, interaction, and
  console diagnostics rather than a successful process exit alone.
- Release and clean-break decisions remain explicit operator gates.

## Validation Pointers

- Stage 0 inventory: repository-scoped `rg`/`find`, current build transcripts,
  emitted manifests, and fresh browser reference proof.
- Radix stages: focused Wasm leaf checks and validation fixtures; one cheap
  owner-repo closeout after final edits.
- Faber stages: focused product-recipe tests, deterministic manifest checks,
  and negative assertions for `tsc`, `.ts`, and TS configuration.
- Host stages: import-table, handle-arena, callback, promise/trap, and unknown-
  import tests.
- Negative browser cases cover malformed Wasm, missing sibling modules,
  signature mismatch, malformed metadata, missing selectors, unavailable
  WebGPU, device loss, and callback traps as typed non-success outcomes.
- Triga stages: fresh browser run with WebGPU state, pixel readback, frames,
  input, and console evidence.
- Factory inventory: regenerate `radix/docs/factory/README.md` and run the
  factory status audit after campaign status changes.

## Decisions Locked

- One Faber-owned cross-repo campaign; later delivery specs live with the
  implementation owner.
- Core Wasm through validated MIR is the compiler path.
- Browser packaging is a Faber product recipe, not a new target.
- `hosts/webgpu-browser` is the browser loader and capability-host owner.
- A thin JavaScript host is allowed and strictly bounded.
- Browser asynchrony uses queued operation identifiers and one typed,
  serialized, non-reentrant Wasm completion dispatcher.
- WebGPU draw and dispatch cross at compiled submission-region boundaries.
- WGSL remains the shader artifact.
- Wasm Host Parity remains the ABI and native package-host predecessor.
- The TypeScript browser-app recipe is retired after the accepted product gate;
  the explicit `ts` codegen target remains outside this clean break.

## Open Decisions

- Exact encoding for stable binding/reflection tables.
- Whether Stage 6 measurements justify a batched render command surface.
- Faber version and migration window at Stage 8.

Stage 0 resolves the metadata encoding within the locked architecture. Stage 6
owns the coarser-batching measurement decision. Stage 8 stops for the release
decision.

## Stop Conditions

- Stop if a stage requires a new HIR-direct Wasm or browser compiler target.
- Stop if a host change moves application, Triga, or renderer policy into
  JavaScript.
- Stop if JavaScript expands beyond the accepted generic host allowlist without
  a reviewed campaign amendment.
- Stop on any silent `ts`/`tsc` fallback.
- Stop if WGSL is parsed to recover reflection or one stable fact gains several
  generated owners.
- Stop and create a separate campaign for Components, WIT, WASI, Workers,
  threads, shared memory, or wasm64.
- Stop if a stage duplicates Wasm Host Parity's runtime contract, package
  module linking, or general CPU parity work.
- Stop before incompatible public contract or release action until the operator
  accepts the version and migration plan.

## Planning Handoff

**Selected next stage**: Stage 0 — Baseline, ownership, and boundary lock.

**Readiness**: ready for delivery planning after the independent goal-check in
[goal.md](goal.md) records `READY`.

Stage 0 is evidence-only and does not authorize product edits. Its delivery
must name exact repository roots, inventory outputs, selected fixtures, fresh
browser proof commands, target-claim conflicts, and the done oracle before a
Hand begins.
