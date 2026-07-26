# Delivery: Pure-Lib Triga Browser Resolve — CLOSED

**Parent want**: `bedc26ac` — pure-lib triga: browser product resolve without link-triga-ts.mjs
**Priority**: P3
**Primary repo**: `faber` (Phases 2-4), `examples` (Phase 5)
**Planner**: planner-2
**Lowered**: 2026-07-26
**Accepted**: 2026-07-26 (P5 31f271a)
**Status**: **ACCEPTED** — all 5 phases complete; theme closed out.

## Interpreted Unit

Remove `scripta/link-triga-ts.mjs` by making the faber browser product build
automatically emit and link pure-library (`kind = "lib"`) TypeScript
dependencies, so `import { triga } from "triga:triga"` resolves natively.

## What Exists Now (post-implementation)

All 5 phases are complete. The faber-native build path works end-to-end.

### The gap (was)

The delivery originally addressed this gap:
- `faber build` emitted TS only for the package's own units, never library
  dependency source (triga/src/*.fab)
- Ambient declarations in `faber-web.d.ts` declared types but provided no
  implementations; ESM files imported triga namespaces that did not exist at
  runtime.

### Resolved

- `build_browser_product` now emits library TS modules (Phase 2, ed039aa)
- Emit defects from radix codegen are patched by
  `apply_library_emit_fixes` (Phase 3, c952507)
- Import rewriting (`triga:triga` → `./triga-triga.js`) is wired into both
  app and library emit paths; triga ambient declarations removed from
  `web_ambient_declarations()` (Phase 4, cb1bb56)
- `link-triga-ts.mjs` is deleted; proof scripts use `faber build` only
  (Phase 5, 31f271a on examples)

## Assessment

### Where this belonged

**faber** (compiler product surface). Not cista.

| Option | Rationale |
|--------|-----------|
| **faber product build** (implemented) | The browser product build already resolved library imports during package analysis. It owned the TS emit pipeline and TypeScript compilation. Extending it to emit library dependencies was a natural extension of its existing responsibility. |
| cista package store | cista installs/distributes packages. It could pre-build TS artifacts, but that adds a separate build step the user must remember before `faber build`. The faber product build is now self-contained. |

### What faber changed

The `build_browser_product` function in `faber/src/package/product.rs` was
extended to:

1. **Detect library dependencies that target `ts`**: Iterate the lockfile to
   find `kind = "lib"` packages with `target_language = "ts"` (triga is the
   canonical first case; the mechanism is generic, not triga-specific).

2. **Emit TypeScript for each library module**: Call
   `radix::codegen::generate_from_analyzed(Target::TypeScript, …)` on
   each source module via the `faber emit -t ts` subprocess.

3. **Namespace-wrap the emitted code**: Emit namespace objects matching the
   existing ambient declaration pattern.

4. **Rewrite cross-module library imports**: `triga:triga` →
   `./triga-triga.js` (relative ESM path within dist/faber-ts).

5. **Rewrite app module imports**: Same rewriting during app TS emit.

6. **Remove the hardcoded ambient declarations** for triga modules
   from `web_ambient_declarations()`. Ambient declarations become the
   actual emitted module files. (`web:dom` and `web:web` ambient
   declarations remain — those are platform contracts, not library code.)

7. **Include library TS files in tsconfig**: The generated `tsconfig.json`
   includes the new triga-*.ts files.

### Minimum scope achieved

| Scope item | Detail |
|------------|--------|
| First library | triga (3 modules: geometry.fab, triga.fab, scene.fab) |
| Mechanism | Generic — any `kind = "lib"` dep with TS target triggers library emit |
| Emit defects | IIFE-as-LHS and `unresolved_def` handled by `apply_library_emit_fixes` post-processing in Phase 3 (c952507). Radix issues filed in `docs/factory/ts-codegen/`. |
| Non-claims | No cista pre-build cache, no multi-library orchestration beyond triga, no ambient declaration auto-generation from source introspection |

## Stage Graph (all phases complete)

### Phase 1 — Assessment confirmation (this delivery)

**Status**: complete
**Gate**: assessment answers the three questions (where, what change, minimum
scope).
**Evidence**: this file committed (755e971).

### Phase 2 — Library TS emit in faber product build

**Status**: complete (ed039aa)
**Output**: `faber build` on hello-voxel produces triga-*.ts files in
`dist/faber-ts/` alongside the app's own modules.
**Write scope**: `faber/src/package/product.rs` (emit path only).
**Gate passed**: `faber build` on hello-voxel produces `dist/faber-ts/triga-triga.ts`,
`dist/faber-ts/triga-geometry.ts`, `dist/faber-ts/triga-scene.ts` with valid
TypeScript.

### Phase 3 — Emit defect handling

**Status**: complete (c952507)
**Output**: Radix issues filed; `apply_library_emit_fixes` post-processing
handles IIFE-as-LHS and `unresolved_def` for blocking tsc.
**Write scope**: `faber/src/package/product.rs` (post-processing), + radix issues.
**Gate passed**: `npx tsc -p dist/tsconfig.faber-browser.json` passes (library
files only).

### Phase 4 — Import rewriting and integration

**Status**: complete (cb1bb56)
**Output**: `faber build` on hello-voxel produces a complete, tsc-clean browser
product with triga implementations linked.
**Write scope**: `faber/src/package/product.rs` (import rewriting, tsconfig
update, ambient declaration removal).
**Gate passed**:
- `npx tsc -p dist/tsconfig.faber-browser.json` exits 0
- `dist/faber-esm/faber-browser.js` imports triga modules and runs without
  runtime import errors

### Phase 5 — Remove link-triga-ts.mjs

**Status**: complete (31f271a)
**Output**: `link-triga-ts.mjs` removed; hello-voxel build and proof scripts
use `faber build` only.
**Write scope**: `examples/hello-voxel/scripta/link-triga-ts.mjs` (deleted),
`examples/hello-voxel/tests/` (updated build steps).
**Gate passed**: hello-voxel browser proof passes through the faber-native build
path only.

## Implementation Work (all phases complete)

| Phase | Done (SHA) | Non-goals |
|-------|------------|-----------|
| 2 | ed039aa | No tsc, no import rewriting |
| 3 | c952507 | No full product build |
| 4 | cb1bb56 | link-triga-ts.mjs removal deferred to Phase 5 |
| 5 | 31f271a | No triga beyond current 3 modules |

## Repo-Aware Baseline

- **faber product build**: `faber/src/package/product.rs` — `build_browser_product`,
  `emit_typescript_modules`, `emit_library_typescript_modules`,
  `apply_library_emit_fixes`, `build_library_ts_module_map`,
  `rewrite_library_imports`, `web_ambient_declarations`, `render_tsconfig`
- **faber package analysis**: `faber/src/package/compile.rs` — `analyze_package`,
  library resolver
- **faber lockfile**: `faber/src/package/lockfile.rs` — records `kind = "lib"`,
  `target_language = "ts"` for triga
- **radix TS codegen**: `radix/crates/radix/src/codegen/` — `Target::TypeScript`,
  `generate_from_analyzed`

## Key Design Decisions

| Decision | Reasoning |
|----------|-----------|
| Namespace wrapping in faber, not radix | Radix codegen emits free functions; namespace wrapping is a faber product concern (how the browser product imports and uses library code). Radix should not encode a faber packaging convention. |
| Generic mechanism, triga-first | The detection loop iterates lockfile deps. triga is the only current case, but the code must not hardcode "triga". |
| Emit into dist/faber-ts/ alongside app modules | Simpler than a separate directory. The tsconfig `include` already covers `faber-ts/**/*.ts`. |
| Keep `web_ambient_declarations()` for web:* only | `web:dom` and `web:web` are platform contracts, not library code. They stay as ambient declarations. triga ambient declarations become actual emitted files. |

## Open Questions (all resolved)

1. **tsconfig `rootDir` constraint**: The current tsconfig sets `rootDir` to
   `dist/faber-ts`. Library TS files are emitted there. No tsconfig structural
   change was needed. **Resolved**.

2. **Emit defect severity**: IIFE-as-LHS and `unresolved_def` were the blocking
   defects for tsc. Both are handled by `apply_library_emit_fixes` in
   Phase 3 (c952507). Radix issues filed. **Resolved**.

3. **Multi-library orchestration**: triga is the only TS-targeting lib
   dependency. Mechanism is generic; no multi-library case has appeared.
   **Deferred**.

## Validation (all gates passed)

```bash
# Phase 2: library TS emit
cd examples/hello-voxel && faber build
ls dist/faber-ts/triga-*.ts  # exists (ed039aa)

# Phase 4: full tsc
npx tsc -p dist/tsconfig.faber-browser.json  # exits 0 (cb1bb56)

# Phase 5: remove workaround, verify proof
test ! -f scripta/link-triga-ts.mjs  # deleted (31f271a)
./tests/run.sh  # passes through faber-native path
```

## Theme Acceptance

- Mind ACCEPT theme bedc26ac.
- All non-claims respected: no cista pre-build, no triga-specific hardcoding
  in the library emit loop, radix codegen defects handled in faber
  post-processing (not radix), link-triga-ts.mjs removed only after
  faber-native path proven, web:dom/web:web ambient declarations preserved.

## Companion Skill Plan (delivered)

- `correctness`: import rewriting correctness, namespace object shape matches
  ambient declaration shape — delivered in Phase 4 (cb1bb56)
- `cleanliness`: remove hardcoded triga declarations from
  `web_ambient_declarations()` — delivered in Phase 4 (cb1bb56)
- `polish`: faber/src/package/product.rs changes — delivered across Phases 2-4
  (ed039aa, c952507, cb1bb56)

## Stop Conditions (all respected)

- triga-specific hardcoding in the library emit loop — **avoided**: generic over
  lockfile deps
- radix codegen defect fixes in faber — **avoided**: post-processing only;
  radix issues filed
- removal of `link-triga-ts.mjs` before faber-native path proven — **avoided**:
  removed only in Phase 5 after Phase 4 proven
- breaking the existing `web:dom`/`web:web` ambient declarations — **avoided**:
  only triga declarations removed
- adding cista pre-build as a dependency — **avoided**
