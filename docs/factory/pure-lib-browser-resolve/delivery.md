# Delivery: Pure-Lib Triga Browser Resolve

**Parent want**: `bedc26ac` — pure-lib triga: browser product resolve without link-triga-ts.mjs
**Priority**: P3
**Primary repo**: `faber`
**Planner**: planner-2
**Lowered**: 2026-07-26
**Status**: READY for implementation assessment; NOT READY for implement until
phase 1 completes.

## Interpreted Unit

Remove `scripta/link-triga-ts.mjs` by making the faber browser product build
automatically emit and link pure-library (`kind = "lib"`) TypeScript
dependencies, so `import { triga } from "triga:triga"` resolves natively.

## What Exists Now

### The gap

`faber build` on `examples/hello-voxel` (a `kind = "bin"`, `target = "ts"`,
`product.kind = "browser-app"` package) calls `build_browser_product` in
`faber/src/package/product.rs`. That function:

1. Analyzes the package (resolves library imports via `FABER_LIBRARY_HOME`)
2. Calls `emit_typescript_modules` — emits TS **only for the package's own
   units** (main.fab, voxel.fab, meshing.fab, application.fab)
3. Writes `faber-web.d.ts` with hardcoded `declare module "triga:triga" { … }`
   ambient type stubs via `web_ambient_declarations()`
4. Writes `faber-browser.ts` entry, tsconfig, invokes tsc

Library dependency source (triga/src/*.fab) is never emitted. The ambient
declarations in `faber-web.d.ts` declare types but provide **no
implementations**. After tsc, the resulting ESM files import triga namespaces
that do not exist at runtime.

### The workaround

`examples/hello-voxel/scripta/link-triga-ts.mjs` (~260 lines) bridges the gap:

1. Runs `faber emit -t ts` directly on `triga/src/geometry.fab`,
   `triga/src/triga.fab`, and `triga/src/scene.fab`
2. Wraps each emitted module's free functions/classes into a namespace
   object: `export const triga = { vector3, … }`
3. Rewrites cross-module imports (`triga:geometry` → `./triga-geometry.js`)
4. Rewrites the app's main.ts imports (`"triga:triga"` → `"./triga-triga.js"`)
5. Patches known emit defects (IIFE-as-LHS, `unresolved_def`, truncating
   division)
6. Runs tsc

### Ambient declarations (already exist)

`web_ambient_declarations()` in `faber/src/package/product.rs:1043-1116` already
contains hardcoded `declare module "triga:triga"`, `declare module
"triga:geometry"`, and `declare module "triga:scene"` blocks. These are
manually maintained type stubs for every triga function hello-voxel uses.

## Assessment

### Where does this belong?

**Answer: faber** (compiler product surface). Not cista.

| Option | Rationale |
|--------|-----------|
| **faber product build** (this plan) | The browser product build already resolves library imports during package analysis. It owns the TS emit pipeline and TypeScript compilation. Extending it to emit library dependencies is a natural extension of its existing responsibility. |
| cista package store | cista installs/distributes packages. It could pre-build TS artifacts, but that adds a separate build step the user must remember before `faber build`. The faber product build should be self-contained. cista pre-build is a future optimization, not the initial resolution. |

### What faber change is needed?

The `build_browser_product` function in `faber/src/package/product.rs` must:

1. **Detect library dependencies that target `ts`**: After package analysis,
   iterate the lockfile or dependency graph to find `kind = "lib"` packages
   with `target_language = "ts"` (triga is the canonical first case; the
   mechanism must be generic, not triga-specific).

2. **Emit TypeScript for each library module**: For each library dependency,
   call `radix::codegen::generate_from_analyzed(Target::TypeScript, …)` on
   each source module. This is the same code path `faber emit -t ts` uses.

3. **Namespace-wrap the emitted code**: Library modules export free
   functions; the browser product imports them as namespaces
   (`import { triga } from "triga:triga"`). The emitted code must produce
   namespace objects matching the existing ambient declaration pattern:
   ```typescript
   export const triga = { vector3, vector3_subtracta, … };
   ```

4. **Rewrite cross-module library imports**: When `triga:triga` imports
   `triga:geometry`, rewrite to `./triga-geometry.js` (relative ESM path within
   the dist/faber-ts directory).

5. **Rewrite app module imports**: When `main.ts` imports `"triga:triga"`,
   rewrite to `"./triga-triga.js"`. This must happen during or after app TS
   emit.

6. **Remove the hardcoded ambient declarations** for triga modules
   from `web_ambient_declarations()`. The ambient declarations become the
   actual emitted module files. (Keep `web:dom` and `web:web` ambient
   declarations — those are platform contracts, not library code.)

7. **Include library TS files in tsconfig**: The generated `tsconfig.json`
   must include the new triga-*.ts files in its `include` array (or a glob
   that covers them).

### Minimum scope

| Scope item | Detail |
|------------|--------|
| First library | triga only (3 modules: geometry.fab, triga.fab, scene.fab) |
| Mechanism | Generic — any `kind = "lib"` dep with TS target triggers library emit |
| Emit defects | The emit defects that `link-triga-ts.mjs` patches (IIFE-as-LHS, `unresolved_def`, truncating division) are radix codegen bugs. The delivery must **file radix issues** for those defects rather than reproducing the patches. If any defect blocks hello-voxel tsc, add a minimal emit post-processing step in faber (not in the library emit path). |
| Non-claims | No cista pre-build cache, no multi-library orchestration beyond triga, no ambient declaration auto-generation from source introspection |

### Non-claims

- **Do not** add a cista pre-build step. That is a follow-up optimization.
- **Do not** auto-generate ambient declarations from triga source. The emitted
  `.ts` files are the declarations.
- **Do not** handle library dependencies that target `rust` but not `ts` (irrelevant for browser products).
- **Do not** fix radix TS codegen defects in this delivery. File issues; add
  minimal post-processing only if tsc would fail.
- **Do not** remove `link-triga-ts.mjs` until the faber build path is proven.

## Stage Graph

### Phase 1 — Assessment confirmation (this delivery)

**Status**: complete (this artifact)
**Gate**: assessment answers the three questions (where, what change, minimum
scope).
**Evidence**: this file committed.

### Phase 2 — Library TS emit in faber product build

**Depends on**: Phase 1
**Output**: `faber build` on hello-voxel produces triga-*.ts files in
`dist/faber-ts/` alongside the app's own modules.
**Write scope**: `faber/src/package/product.rs` (emit path only).
**Gate**: `faber build` on hello-voxel produces `dist/faber-ts/triga-triga.ts`,
`dist/faber-ts/triga-geometry.ts`, `dist/faber-ts/triga-scene.ts` with valid
TypeScript.
**Non-claims**: tsc may still fail on emit defects; that is Phase 3's concern.

### Phase 3 — Emit defect handling

**Depends on**: Phase 2
**Output**: File radix issues for TS codegen defects found in triga emit.
Add minimal post-processing for any defect that blocks hello-voxel tsc.
**Write scope**: `faber/src/package/product.rs` (post-processing), + radix issues.
**Gate**: `npx tsc -p dist/tsconfig.faber-browser.json` passes (library files
only; full product build may need Phase 4).

### Phase 4 — Import rewriting and integration

**Depends on**: Phase 3
**Output**: `faber build` on hello-voxel produces a complete, tsc-clean browser
product with triga implementations linked.
**Write scope**: `faber/src/package/product.rs` (import rewriting, tsconfig
update, ambient declaration removal).
**Gate**:
- `npx tsc -p dist/tsconfig.faber-browser.json` exits 0
- `dist/faber-esm/faber-browser.js` imports triga modules and runs without
  runtime import errors
- Existing `link-triga-ts.mjs` path still works (coexistence during transition)

### Phase 5 — Remove link-triga-ts.mjs

**Depends on**: Phase 4, HV-04B/C proof green
**Output**: `link-triga-ts.mjs` removed; hello-voxel build and proof scripts
use `faber build` only.
**Write scope**: `examples/hello-voxel/scripta/link-triga-ts.mjs` (delete),
`examples/hello-voxel/tests/` (update build steps).
**Gate**: hello-voxel browser proof passes through the faber-native build path
only.

## Implementation Work

| Phase | Done when | Non-goals |
|-------|-----------|-----------|
| 2 | triga modules emit alongside app modules in dist/faber-ts/ | No tsc, no import rewriting |
| 3 | Critical emit defects have radix issues; tsc passes on library files | No full product build |
| 4 | Full `faber build` produces working browser product with triga linked | No link-triga-ts.mjs removal yet |
| 5 | link-triga-ts.mjs deleted; proof passes through faber-native path | No triga beyond current 3 modules |

## Repo-Aware Baseline

- **faber product build**: `faber/src/package/product.rs` — `build_browser_product`,
  `emit_typescript_modules`, `web_ambient_declarations`, `render_tsconfig`
- **faber package analysis**: `faber/src/package/compile.rs` — `analyze_package`,
  library resolver
- **faber lockfile**: `faber/src/package/lockfile.rs` — records `kind = "lib"`,
  `target_language = "ts"` for triga
- **radix TS codegen**: `radix/crates/radix/src/codegen/` — `Target::TypeScript`,
  `generate_from_analyzed`
- **workaround reference**: `examples/hello-voxel/scripta/link-triga-ts.mjs`
- **ambient declarations**: `faber/src/package/product.rs:1043-1116`

## Key Design Decisions

| Decision | Reasoning |
|----------|-----------|
| Namespace wrapping in faber, not radix | Radix codegen emits free functions; namespace wrapping is a faber product concern (how the browser product imports and uses library code). Radix should not encode a faber packaging convention. |
| Generic mechanism, triga-first | The detection loop iterates lockfile deps. triga is the only current case, but the code must not hardcode "triga". |
| Emit into dist/faber-ts/ alongside app modules | Simpler than a separate directory. The tsconfig `include` already covers `faber-ts/**/*.ts`. |
| Keep `web_ambient_declarations()` for web:* only | `web:dom` and `web:web` are platform contracts, not library code. They stay as ambient declarations. triga ambient declarations become actual emitted files. |

## Open Questions

1. **tsconfig `rootDir` constraint**: The current tsconfig sets `rootDir` to
   `dist/faber-ts`. If library TS files are emitted there, this holds. No change
   needed. **Resolved**: no tsconfig structural change needed.

2. **Emit defect severity**: How many of the `link-triga-ts.mjs` patches are
   actually needed for tsc to pass? **Action**: Phase 2 measures which defects
   cause tsc failures. File radix issues for all; only patch the blocking ones.

3. **Multi-library orchestration**: What happens when two browser-app
   dependencies both target TS and import each other? **Deferred**: no current
   case exists. triga is the only TS-targeting lib dependency.

## Validation

```bash
# Phase 2: library TS emit
cd examples/hello-voxel && faber build
ls dist/faber-ts/triga-*.ts  # must exist

# Phase 4: full tsc
npx tsc -p dist/tsconfig.faber-browser.json  # must exit 0

# Phase 5: remove workaround, verify proof
test ! -f scripta/link-triga-ts.mjs
./tests/run.sh  # must pass
```

## Companion Skill Plan

- `correctness`: import rewriting correctness, namespace object shape matches
  ambient declaration shape
- `cleanliness`: remove hardcoded triga declarations from
  `web_ambient_declarations()`
- `polish`: faber/src/package/product.rs changes

## Stop Conditions

- triga-specific hardcoding in the library emit loop (must be generic over
  lockfile deps)
- radix codegen defect fixes in faber (file radix issues; minimal
  post-processing only)
- removal of `link-triga-ts.mjs` before faber-native path is proven
- breaking the existing `web:dom`/`web:web` ambient declarations
- adding cista pre-build as a dependency
