# Goal: Browser Wasm Product

**Status**: active — Stage 0 closed (2026-08-09, closeout 1ece368); Stage 1 delivery lowering complete pending admission
**Created**: 2026-08-09
**Target repo**: `/Users/ianzepp/work/faberlang/faber`
**Campaign**: [CAMPAIGN.md](CAMPAIGN.md)
**Control-plane owner**: Faber browser product workflow

## Summary

Make core WebAssembly the normal browser application artifact for Faber
packages, including Triga applications. Faber and library behavior should run
in Wasm. A small, shared JavaScript host may remain only where the browser
requires JavaScript objects or callbacks.

The product path must use the normal Faber package workflow and Radix MIR-to-
Wasm target. It must not introduce a separate HIR-direct browser compiler or
recreate application behavior in JavaScript.

## Problem

The live Triga browser demonstrations currently prove useful behavior through
generated TypeScript, `tsc`, JavaScript modules, and direct browser WebGPU
calls. That path works, but it makes TypeScript part of the product recipe and
puts too much application and renderer policy on the host side.

The live Wasm path is stronger than older target notes imply. Faber can build a
package module set and its portable native host can link and run that set.
There is still no browser Wasm product recipe or browser host for the same
module contract. Several Faber and Triga value shapes also remain outside the
validated Wasm lowering boundary.

Without one campaign, the likely failure modes are a second browser-only
compiler route, duplicated runtime contracts, a JavaScript engine hiding
behind a Wasm entry point, or many repo-local plans with no product gate.

## Goals

- Make `faber build --target wasm --package .` the compiler path used by a
  browser application product recipe.
- Compile application, controller, scene, geometry, transform, and renderer
  policy from Faber source into core Wasm.
- Keep one thin, generic browser host for irreducible browser APIs.
- Use the existing versioned Faber runtime contract and Wasm package module
  linking work rather than create a second browser ABI.
- Make `faber-web` bindings usable from Wasm without a generated TypeScript
  runtime.
- Prove Triga static and animated WebGPU applications through Wasm, WGSL, and
  the thin browser host.
- Remove `tsc`, generated `.ts`, generated application `.js`, and generated
  TypeScript configuration from the Wasm browser recipe.
- Reduce runtime JSON to facts that are truly dynamic. Prefer build-time Wasm
  data, exports, custom sections, or compact generated assets for stable facts.
- Retire the TypeScript browser-app recipe after the Wasm product path meets
  the declared proof and release gates.

## Non-goals

- No HIR-direct Wasm backend and no new Radix `web` target.
- No Component Model, WIT, WASI, browser Worker, wasm64, thread, or shared-
  memory migration.
- No claim that browsers can call DOM or WebGPU APIs without any JavaScript.
- No rewrite of WebGPU in Wasm. Raw browser WebGPU calls remain host thunks.
- No replacement of WGSL. WGSL remains the GPU shader artifact.
- No private copy of Faber semantics, Triga math, scene policy, or renderer
  policy in JavaScript.
- No broad Wasm performance campaign before the functional product proof.
- No release, deployment, or public availability claim from this campaign.
- No removal of the explicit general-purpose TypeScript code-generation target
  merely because the browser-app product stops using it.

## Ground Truth Researched

- `radix/crates/radix-mir-wasm/` and the binary serializer own the
  normal validated-MIR-to-core-Wasm artifact route.
- [Wasm Host Parity](../../../../radix/docs/factory/wasm-host-parity/CAMPAIGN.md) owns the portable core-
  Wasm CPU ABI, package linking, native host parity, and the shared gap ledger.
  It explicitly excludes browser packaging.
- `faber/src/package/wasm.rs` and focused package tests prove a package-aware
  Wasm builder rather than a single loose module. It currently persists WAT
  while retaining binary Wasm in memory; it does not yet publish a browser
  module set or close locked source-library dependencies.
- `hosts/wasm` owns the portable native module linker and v1 host contract.
- Faber's current browser-app recipe is TypeScript-specific and invokes `tsc`.
- `faber-web` currently exposes TypeScript-target bindings and TypeScript
  runtime shims rather than a target-neutral Faber contract plus browser-Wasm
  host map.
- Triga browser proofs currently emit TypeScript/JavaScript and call WebGPU
  through that host route. They remain the behavioral reference for the first
  Wasm browser proofs, not the desired final architecture.
- Existing shader reflection facts are build inputs. The browser host must not
  recover them by parsing WGSL.

Live source and focused tests outrank old target matrices and historical
campaign prose. Stage 0 must reconcile any product claim that still describes
Wasm as single-module, emit-only, or unable to build packages.

## Reference Packet

Before lowering any stage, inspect:

- [Wasm Host Parity](../../../../radix/docs/factory/wasm-host-parity/CAMPAIGN.md) and its live gap ledger.
- `radix/crates/radix-mir-wasm/` for supported carriers, imports,
  exports, calls, and fail-closed boundaries.
- `faber/src/package/wasm.rs`, Faber product recipes, and `faber targets` for
  the current package and product surface.
- `hosts/wasm` for the current module-set and runtime ABI contract.
- `hosts/webgpu-browser` and `faber-web` for browser-owned APIs and existing
  bindings.
- Triga's current browser exempla, build scripts, emitted metadata, and browser
  verification fixtures.
- `radix/docs/design/target-capability-matrix.md` for claims that Stage 0 must
  verify or repair.

## Constraints And Invariants

> Faber source lowers through validated MIR to core Wasm. The browser host may
> expose browser capabilities, but it may not become a second application
> runtime or language implementation.

- The ordinary external target remains `wasm`. Browser application packaging
  is a Faber product recipe over that artifact, not a new compiler target.
- The host imports are closed and versioned. Unknown imports fail before entry.
- The browser host owns WebAssembly instantiation, module linking, browser
  object handles, callback trampolines, browser lifecycle, raw DOM calls, raw
  WebGPU calls, and asset fetch.
- Faber/Wasm owns controller state, scene construction, transforms, geometry,
  animation, renderer policy, command selection, and application errors.
- Stable facts have one owner. WGSL reflection, bind-group layout, vertex
  layout, and controller identity must not be duplicated across TypeScript,
  JSON, and Wasm.
- Missing target-neutral facts route to shared MIR. Wasm encoding remains
  Radix-owned. Browser capability policy remains host-owned.
- No hidden TypeScript fallback is allowed in build, serve, test, or demo
  scripts.
- Crossing the Wasm/JavaScript boundary is explicit and measurable. Batch
  operations or command buffers may replace chatty calls after correctness is
  proven, without moving policy into JavaScript.
- Existing foreign work and unrelated repository dirt stay untouched.

## Architecture Direction

```text
Faber package + Triga + faber-web contracts
                   │
                   ▼
       Radix frontend → validated MIR → core Wasm module set
                   │                         │
                   │                         ├── application/controller policy
                   │                         ├── scene, geometry, transforms
                   │                         └── renderer command policy
                   │
          WGSL + owned reflection facts
                   │
                   ▼
       thin generic browser host JavaScript
         ├── instantiate and link module set
         ├── map opaque handles to DOM/WebGPU objects
         ├── forward events and animation-frame callbacks
         ├── issue raw browser WebGPU calls
         └── fetch declared assets
                   │
                   ▼
          observed DOM, pixels, frames, and input
```

The preferred browser boundary is a small versioned capability surface. DOM
and resource-lifecycle operations may be individual calls. WebGPU draw and
dispatch work crosses the Wasm/JavaScript boundary as submission-region
descriptors or command batches from the first graphics proof, consistent with
the Direct Device Product Pipeline. Later measurement may coarsen the boundary,
but it may not move renderer policy into JavaScript.

Browser asynchrony uses explicit completion, not hidden Wasm stack suspension:

- an async start returns an opaque `i32` operation identifier;
- the JavaScript host resolves Promises and later invokes one versioned Wasm
  completion dispatcher with operation id, typed status, and payload record;
- completion is queued after the initiating import returns, so the host never
  synchronously re-enters an active Wasm export;
- one serialized dispatcher delivers each operation exactly one terminal
  result: completed, failed, cancelled, or device-lost;
- cancellation is explicit and best-effort. A late Promise result after a
  terminal state is discarded by the host and never re-enters Wasm;
- animation frames and each subscription preserve browser delivery order.
  Independent Promise operations have no invented global order beyond their
  operation identifiers;
- device loss and other unsolicited browser lifecycle events use the same
  queued typed-event path;
- `fetch_text` must consume this contract or remain unsupported. It may not
  pretend that a Promise completed synchronously.

Wasm owns operation state, continuation policy, and application response. The
host owns Promise objects, browser scheduling, and typed transport only.

## Supporting Skills

- `faber`: verify language, compiler, product, host, and exempla claims against
  live source.
- `campaign`: keep cross-repo work ordered and lower only the selected stage.
- `delivery`: turn a selected stage into exact files, owners, fixtures, and
  commands in the repo that will implement it.
- `factory`: implement one accepted delivery slice at a time.
- `zombie-docs`: repair stale target and product claims when live behavior has
  advanced.

## Implementation Shape

1. Record the current generated TypeScript, JavaScript, JSON, WGSL, and browser
   host surface. Lock the allowed JavaScript boundary and reconcile Wasm
   capability claims.
2. Close the smallest Radix Wasm shape gaps needed by one `faber-web` browser
   controller and one Triga static scene.
3. Add a generic browser core-Wasm loader and versioned host capability map.
4. Add a Faber browser-app Wasm product recipe with deterministic artifacts and
   no TypeScript toolchain step.
5. Move `faber-web` to target-neutral Faber contracts with browser-Wasm
   bindings.
6. Prove a controller, then static Triga graphics, then animated and interactive
   Triga graphics.
7. Remove duplicated metadata and retire the TypeScript browser-app recipe at
   the release checkpoint.

The ordered stages, dependencies, owners, and gates are in
[CAMPAIGN.md](CAMPAIGN.md). Each implementation stage must first lower through
goal-forge, goal-check, delivery, and factory.

## Release Posture

Decision: defer release. The first complete browser proof creates a release
checkpoint. At that checkpoint, the operator chooses the Faber version and
migration window. After the checkpoint is accepted, the browser-app recipe
moves by clean break to Wasm and the old TypeScript browser-app recipe is
removed. This goal does not authorize tags, pushes, deployment, or publication.

## Exit Strategy

Decision: included. If a required browser operation cannot fit the shared v1
runtime contract honestly, stop at the capability boundary and route a
contract proposal. Keep the existing TypeScript proof available only as
reference evidence until the Wasm proof passes. Do not hide the gap with a
JavaScript implementation of Faber or Triga behavior.

If browser overhead is too high, measure the crossing and add a target-neutral
batch or command representation. Do not respond by moving renderer policy into
the host.

## Acceptance Criteria

- A normal Faber package command builds the browser application through the
  external `wasm` target and emits a deterministic core-Wasm module set.
- The module set includes the target-native locked source-library closure for
  Triga, `faber-web`, and other selected Faber libraries. No library is dropped
  or replaced by TypeScript.
- The Wasm browser recipe contains no generated `.ts`, no generated
  application `.js`, no `tsconfig.faber-browser.json`, and no `tsc` invocation.
- Authored JavaScript is confined to an explicit allowlist in the shared
  browser-Wasm host. No app or library directory contains JavaScript policy.
- A `faber-web` controller mounts, changes observed DOM state, receives a
  browser callback, and reports errors through Wasm.
- The promoted distribution contains zero inline application JavaScript and no
  private per-demo renderer/runtime copy. One shared loader entry is used.
- Any per-application loader or generated ABI adapter is at most 16 KiB
  unminified and contains no application, Triga, shader, or renderer policy.
- The promoted product has no Three.js runtime dependency.
- Triga renders the declared static geometry set through Wasm and WebGPU.
- Triga advances an animated terrain or water scene across multiple frames and
  responds to declared input through Wasm-owned state.
- Browser evidence includes module provenance, successful instantiation,
  controller mount, WebGPU device and upload success, non-background pixel
  readback, advancing frames, interaction, and no unexpected console errors.
- WGSL reflection and controller metadata each have one canonical owner. Any
  remaining JSON has a named runtime consumer and cannot be replaced by a
  stable build fact.
- `faber targets`, target capability docs, and browser build help describe the
  observed product truth with no TypeScript fallback claim.
- The old TypeScript browser-app recipe is retired after the accepted release
  checkpoint. The explicit `ts` compiler target may remain for users who ask
  for TypeScript output.

## Validation

Every delivery must declare the narrowest owner-repo check. The campaign close
uses these product proofs:

- Radix focused Wasm lowering and validation tests for each newly admitted
  shape, plus explicit reject tests for shapes still outside the contract.
- Faber focused product-recipe tests proving package command, artifact layout,
  and absence of `tsc` or TypeScript artifacts.
- Browser-host contract tests for imports, handles, callback lifecycle, traps,
  and unknown-import rejection.
- Async contract tests cover queued non-reentrant completion, exactly one
  terminal result, cancellation races, browser delivery order, callback traps,
  and device loss.
- `find <browser-dist> -type f \( -name '*.ts' -o -name 'tsconfig*.json' \)`
  returns no files for the Wasm browser recipe.
- The build transcript contains no `tsc` invocation and no fallback target.
- Fresh browser verification records DOM output, pixel readback, frame
  progression, input response, and console diagnostics.
- The appropriate repository's cheap closeout ladder runs once after the last
  implementation edit. Expensive E2E or release suites remain auditor-owned.

## Open Questions

- Should stable browser binding and shader reflection tables be encoded as
  ordinary Wasm data exports, a custom section, or a compact sidecar? Stage 0
  must measure consumers before Stage 3 delivery chooses.
- Does measured host-call volume require coarser batches than the required
  submission-region boundary? The default is no; Stage 6 adds one only from
  evidence.

The first fixture is locked to `triga/corpus/webgl-geometries`, followed by
`webgl-geometry-terrain`, `webgl-animation-orbit`, and
`webgl-animation-terrain`. These questions do not change the compiler route or
ownership boundary.

## Stop Conditions

- Stop if a proposal creates HIR-direct Wasm or a second browser compiler
  target.
- Stop if JavaScript starts owning Faber semantics, controller state, scene
  construction, geometry, transforms, animation, or renderer policy.
- Stop if the host grows beyond the generic platform capability allowlist
  without an explicit architecture review.
- Stop if a build silently falls back to `ts` or invokes `tsc`.
- Stop if reflection is reconstructed by parsing WGSL or copied into multiple
  generated formats without one declared owner.
- Stop and split a new campaign if Component Model, WIT, WASI, Workers,
  threads, shared memory, or wasm64 becomes required.
- Stop if the work duplicates the CPU ABI or package-linking scope already
  owned by Wasm Host Parity.
- Stop at the release checkpoint for any incompatible public product contract;
  do not infer version or migration policy.

## Goal Check

**Evaluator mode**: independent cold pass against live source and factory machinery

**Intended next consumer**: Stage 0 delivery planning

**Verdict**: READY

Stage 0 is sufficiently grounded for delivery planning. The campaign has one
Faber control plane, names `hosts/webgpu-browser` as the browser-host owner,
records the current WAT/in-memory-Wasm artifact baseline, fixes WebGPU crossings
at submission-region boundaries, and defines queued non-reentrant async
completion with operation IDs, terminal states, cancellation races, ordering,
and device loss. Factory README generation and status auditing are clean. Stage
0 remains evidence-only; its delivery must name exact inventory files,
fixtures, commands, and done oracle.
