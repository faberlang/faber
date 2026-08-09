# TypeScript Browser Behavior — Reference Evidence (U1)

**Unit**: `bwp-s0-u1-ts-browser-reference` (delivery-stage0.md §U1, lines 137–151)
**Campaign**: BROWSER-WASM-PRODUCT Stage 0 — baseline, ownership, boundary lock
**Status**: delivered (evidence captured 2026-08-09)
**Hand**: hand-4 (tugboat, task `b3443c2b`)

## Capture environment

| Item | Value |
| --- | --- |
| faber binary | `/Users/ianzepp/work/faberlang/faber/target/debug/faber` — `faber 1.6.0-rc.1`, built 2026-08-09 16:19 |
| faber tree HEAD at capture | `917c5ff` (`test(device): faber consumer migration …`) — clean at capture |
| tsc | `tsc` 6.0.3 on PATH (`/Users/ianzepp/.nvm/versions/node/v24.15.0/bin/tsc`, node v24.15.0) |
| workspace | `/Users/ianzepp/work/faberlang` (faber, faber-web, triga, examples sibling repos) |
| capture date | 2026-08-09 |

All runs were transcript-only against the prebuilt binary (no cargo). The
sibling `evidence/` inventory (U0, parallel lane) was not present at capture
time, so artifact paths/counts below were measured directly from the fixtures
and build outputs rather than cited from the inventory.

Physical browser WebGPU observation is **not** part of this unit (per
delivery-stage0.md §U1 `non_goals`; that is the U6/auditor surface).

---

## (a) `faber build --package .` on `examples/browser-app` — succeeds, produces `dist/` with `faber-esm/` + `controllers.json`

Fixture: `examples/browser-app/` — the WEB5 application fixture
(`faber.toml`):

```toml
[package]
name = "browser-app"
version = "0.1.0"
edition = "2026"

[paths]
entry = "main.fab"

[build]
target = "ts"
kind = "bin"

[product]
kind = "browser-app"
emit = "typescript"
out = "dist"
templates = "pages"
styles = "styles"
public = "public"
controllers_json = "controllers.json"

[dependencies]
web = "0.1.0"
```

Ran via the fixture's own `tests/run.sh` (`FABER_BIN` env; the script also
rewrites `faber.lock` to the workspace path layout):

```sh
cd /Users/ianzepp/work/faberlang/examples/browser-app
FABER=/Users/ianzepp/work/faberlang/faber/target/debug/faber ./tests/run.sh
```

Transcript (`RUN_EXIT=0`; abridged in the middle):

```text
Building browser product...
/Users/ianzepp/work/faberlang/examples/browser-app/dist/faber-esm/faber-browser.js
Running DOM harness...
loader-hook resolve: "./main.js" parent: dist/faber-esm/faber-browser.js
loader-hook resolve: "./web-dom.js" parent: dist/faber-esm/main.js
loader-hook: intercept compiled ./web-dom.js → bridge
...
54 passed, 0 failed
```

The `dist/` layout after the build (current binary; note the `web-canvas2d*`
modules are also emitted from the faber-web library surface):

```text
dist/
  assets.json
  controllers.json
  faber-esm/
    faber-browser.js
    main.js
    web-canvas2d.js
    web-dom.js
    web-shim-canvas2d.js
    web-shim-dom.js
    web-web.js
  faber-ts/
    faber-browser.ts
    faber-web.d.ts
    main.ts
    web-canvas2d.ts
    web-dom.ts
    web-shim-canvas2d.ts
    web-shim-dom.ts
    web-web.ts
  pages/index.html
  product.json
  public/
  styles/main.css
  tsconfig.faber-browser.json
```

`dist/faber-esm/faber-browser.js` (the ESM entry) is written and
`dist/controllers.json` exists. The fixture's node DOM harness
(`tests/browser-fixture-test.mjs` + `register-hooks.mjs` + `fake-dom.mjs`)
imports the built ESM, mounts the controllers against a fake DOM, simulates
events, and reports **54 passed, 0 failed**.

---

## (b) `tsc --project` invocation (from `ts_render.rs` behavior)

The build drives `tsc` fail-closed. Source authority —
`faber/src/package/product/ts_render.rs`:

```rust
pub(super) fn invoke_tsc(tsconfig: &Path) -> Result<(), Box<Diagnostic>> {
    let output = std::process::Command::new("tsc")
        .arg("--project")
        .arg(tsconfig)
        .output();
    ...
    if !output.status.success() {
        ...
        return Err(Box::new(
            product_diag(format!(
                "browser product TypeScript check failed: {stdout}{stderr}"
            ))
            .with_file(tsconfig.display().to_string())
            .with_arg("issue", "product_tsc_failed"),
        ));
    }
    Ok(())
}
```

(`ts_render.rs` lines 217–245; `tsc`-missing → `product_tsc_missing`,
"browser product requires `tsc` on PATH".)

The rendered tsconfig (same file, `render_tsconfig`, lines 194–215) for the
browser-app build — staged copy:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ES2022",
    "moduleResolution": "bundler",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],
    "strict": true,
    "noEmitOnError": true,
    "rootDir": ".../dist/faber-ts",
    "outDir": ".../dist/faber-esm",
    "skipLibCheck": true
  },
  "include": [".../dist/faber-ts/*.ts"]
}
```

Product directory constants — `faber/src/package/product/mod.rs` lines 20–25:
`FABER_TS_DIR = "faber-ts"`, `FABER_ESM_DIR = "faber-esm"`,
`TSCONFIG_FILE = "tsconfig.faber-browser.json"`,
`BROWSER_ENTRY_TS = "faber-browser.ts"`, `WEB_AMBIENT_DTS = "faber-web.d.ts"`,
`BROWSER_ENTRY_JS = "faber-browser.js"`.

Notes on the observed invocation:

- The product build stages into a temporary sibling of the final output
  (`.dist.faber.tmp-<pid>-<nanos>-0`) and atomically publishes only on full
  success (FBR-P2-005, `build.rs` lines 70–160). `tsc` therefore runs against
  the **staged** `faber-ts/` directory, and its error paths in the transcript
  name the temp directory.
- `faber build` does not echo the `tsc` command line. The invocation was
  confirmed by shadowing `tsc` on PATH with a capture wrapper (the wrapper
  copies the staged product tree before exec'ing the real tsc; wrapper
  stdout: `bwp-u1-capture: staged product TS preserved at /tmp/bwp-u1/staged`).
  The effective invocation is:

  ```text
  tsc --project <staged>/tsconfig.faber-browser.json
  ```

- tsc version used: 6.0.3. `noEmitOnError: true` + `strict: true` means any
  diagnostic fails the build and leaves the previous product untouched.

---

## (c) `controllers.json` fields (selector/mount facts)

Producer — `faber/src/package/product/ts_render.rs`, `render_controllers_json`
(lines 247–266):

```rust
serde_json::to_string_pretty(&serde_json::json!({
    "version": 1,
    "controllers": controllers,
}))
```

Record shape — `faber/src/package/product/build.rs` lines 26–32:

```rust
pub(crate) struct BrowserController {
    pub name: String,
    pub selector: String,
    pub module: String,
    pub export: String,
}
```

Default file name — `faber/src/package/manifest.rs` lines 340–342:

```rust
fn default_product_controllers_json() -> String {
    "controllers.json".to_owned()
}
```

Captured from `examples/browser-app/dist/controllers.json` (11 controllers;
every controller row carries `export`, `module` (`./main.js`), `name`, and
`selector`; top level carries `version: 1`):

```json
{
  "controllers": [
    {
      "export": "filter_controller",
      "module": "./main.js",
      "name": "filter_controller",
      "selector": "#filter-demo"
    },
    {
      "export": "focus_controller",
      "module": "./main.js",
      "name": "focus_controller",
      "selector": "#focus-demo"
    },
    {
      "export": "toggle_controller",
      "module": "./main.js",
      "name": "toggle_controller",
      "selector": "#toggle-demo"
    }
  ],
  "version": 1
}
```

Selector/mount facts: `selector` is the CSS mount selector declared on each
`@ WebController` (e.g. `#filter-demo`), `module` is the browser-ESM module
path relative to `dist/faber-esm/`, and `export` is the controller export
name inside that module.

---

## (d) `triga/corpus/webgl-geometries/tests/run.sh` outcome — check passes, **build fails at the tsc gate**

Fixture: `triga/corpus/webgl-geometries/` (`faber.toml`):

```toml
[package]
name = "triga-corpus-geometries"
version = "0.1.0"
edition = "2026"

[paths]
entry = "main.fab"

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

[dependencies]
web = "0.1.0"
triga = "0.1.0"
```

Ran via the fixture's own `tests/run.sh` (`FABER_BIN` env):

```sh
cd /Users/ianzepp/work/faberlang/triga/corpus/webgl-geometries
FABER=/Users/ianzepp/work/faberlang/faber/target/debug/faber ./tests/run.sh
```

Run phases and outcome (**exit 1**):

1. **Asset sync** — host engine JS (`public/src/{product,contract,engine,backend,presentation}`)
   is copied from `hosts/webgpu-browser/public/src/`; `triga-lit.wgsl` +
   `triga-lit-reflection.json` from `hosts/webgpu-browser/public/generated/`
   land in `public/` and (renamed) in `src/shaders/test-data/` as
   `kernel.wgsl`/`reflection.json`.
2. **`faber.lock` rewrite** — the script regenerates the lockfile (quoted below).
3. **Check** — all four sources pass (`checking src/shapes.fab` … `ok:`, then
   `camera_controls.fab`, `scene.fab`, `main.fab`; the output carries the
   expected `warning[LOCALE002]` and unused-symbol warnings, exit continues):
   `ok: /Users/ianzepp/work/faberlang/triga/corpus/webgl-geometries/src/shapes.fab` … `ok: …/src/main.fab`.
4. **Build** — **fails at the `tsc` gate** (transcript):

   ```text
   building browser package
   error: browser product TypeScript check failed: .dist.faber.tmp-54618-1786308003388315000-0/faber-ts/camera_controls.ts(23,14): error TS2304: Cannot find name 'Vec3'.
   .dist.faber.tmp-54618-1786308003388315000-0/faber-ts/camera_controls.ts(110,16): error TS2304: Cannot find name 'Vec3'.
   .dist.faber.tmp-54618-1786308003388315000-0/faber-ts/camera_controls.ts(114,22): error TS2304: Cannot find name 'Vec3'.
   .dist.faber.tmp-54618-1786308003388315000-0/faber-ts/camera_controls.ts(115,16): error TS2304: Cannot find name 'Vec3'.
   .dist.faber.tmp-54618-1786308003388315000-0/faber-ts/camera_controls.ts(116,109): error TS2552: Cannot find name 'Object3D'. Did you mean 'Object'?
   ```

5. **Contract greps did not run** — because the build gate failed, none of the
   `test -f`/`grep`/node assertions in `tests/run.sh` (lines 82–122:
   `dist/faber-esm/faber-browser.js`, `dist/controllers.json`, the
   `"selector": "#triga-corpus-geometries"` grep,
   `dist/public/src/product/bootstrap.js`, engine/backend lanes,
   `triga-lit.wgsl`/`triga-lit-reflection.json`, stale-flat-host absence,
   node content checks) executed. The final `triga-corpus-geometries checks ok`
   line was not printed.

The failing generated module (`camera_controls.ts`, staged copy), with the
error positions quoted — line 23 declares `target!: Vec3;`, and lines
110–116 use `Vec3`/`Object3D` as bare names:

```ts
// Generated by radix - do not edit

import { Matrix4, TransformPayload, math } from "./triga-math.js";
import { object as graph_object } from "./triga-graph-object.js";
import { PerspectiveCamera, camera } from "./triga-graph-camera.js";

export class OrbitCamera {
    target!: Vec3;                                        // line 23
    ...
}
    const pan: Vec3 = math.camera_motus_planus_ex_yaw(...) // line 110
    const direction: Vec3 = math.camera_directio_ex_yaw_pitch(...) // line 114
    const eye: Vec3 = cam.target.subtracta(direction.multiplicata(...)) // line 115
    const perspective: PerspectiveCamera = Object.assign(new PerspectiveCamera(),
      { base: Object.assign(new Object3D(), { ... }), ... }); // line 116
```

Observed mismatch (recorded as behavior, not root-caused — Stage 1/2
boundary work): the emitted library module `triga-math.ts` exports `Vector3`
(not `Vec3`), and `camera_controls.ts` imports neither `Vec3` nor `Object3D`;
the app module references them as undeclared ambient names, so `tsc --strict`
rejects the build. This is the current reference state of the first static
Triga slice: it does **not** yet reach a typechecking browser build.

The regenerated `faber.lock` (quoted; `web` + `triga` both as
`target_language = "ts"` path deps with `target_triple = "browser"`):

```toml
[[package]]
name = "web"
version = "0.1.0"
source = "path"
package_root = "/Users/ianzepp/work/faberlang/faber-web"
kind = "lib"
target_language = "ts"
target_triple = "browser"
target_manifest = ""
interface_root = "/Users/ianzepp/work/faberlang/faber-web/src"
artifact = ""
crate = "web"
rustc = ""

[[package]]
name = "triga"
version = "0.1.0"
source = "path"
package_root = "/Users/ianzepp/work/faberlang/triga"
kind = "lib"
target_language = "ts"
target_triple = "browser"
target_manifest = ""
interface_root = "/Users/ianzepp/work/faberlang/triga/src"
artifact = ""
crate = "triga"
rustc = ""
```

(Identical content is regenerated by `tests/run.sh` from the workspace path
layout; both fixture `faber.lock` files are gitignored.)

---

## (e) `fetch_text` TS async gap + WEB5 fixture scope (with source paths)

**Async gap** — `faber-web/README.md` lines 42–44:

> Known gap: the Radix TS backend does not await `@ futura` calls inside
> `fac`/`cape` blocks, so `dom.fetch_text` is exercised at the runtime-bridge
> level in the WEB5 fixture until the async codegen gap closes.

The fixture-side echo — `examples/browser-app/README.md` §"Known codegen gap" (lines 50–56):

> The Radix TypeScript backend does not yet `await` `@ futura` calls inside
> `fac`/`cape` blocks. `dom.fetch_text` success and failure are therefore
> exercised at the runtime-bridge level in the harness, not from the Faber
> controller body. When the async codegen gap closes, the submit controller can
> be extended to call `dom.fetch_text` directly.

The `web:dom` ambient surface already declares the future-valued route —
`faber-ts/faber-web.d.ts` (staged, generated from `web_ambient_declarations`):
`export function fetch_text(request: FetchRequest): Promise<FetchResponse>;`

**WEB5 fixture scope** (what the browser-behavior claims are limited to):

- `faber-web/README.md` line 35 (delivery status table, WEB5 row):
  "Application fixture — three ESM controllers + DOM harness (faber
  `8aec665`, examples `49be895`)" (the live fixture in fact defines eleven
  `@ WebController` functions in `examples/browser-app/src/main.fab`).
- `radix/docs/factory/faber-hir-v1/browser-application-delivery.md` §WEB5
  (lines 68–73): "Build a static site with two controllers … **Gate**: browser
  or DOM harness observes mounts and mutations from built ESM."
- `radix/docs/design/target-capability-matrix.md` §"Browser Application Product
  Packaging", "Browser behavior claims" (lines 222–228): "limited to the
  **WEB5 fixture evidence** … No navigation, lifecycle, SSR, or Wasm-execution
  claim is made here" — with WEB6 deferral table (navigation/router, SSR, Wasm)
  and reopen rules.

---

## Validation commands run (this unit)

```sh
# browser-app fixture (success): build + DOM harness, exit 0
cd /Users/ianzepp/work/faberlang/examples/browser-app
FABER=/Users/ianzepp/work/faberlang/faber/target/debug/faber ./tests/run.sh
# -> "54 passed, 0 failed"

# triga fixture (current reference outcome: check ok, build fails at tsc gate), exit 1
cd /Users/ianzepp/work/faberlang/triga/corpus/webgl-geometries
FABER=/Users/ianzepp/work/faberlang/faber/target/debug/faber ./tests/run.sh
# -> "error: browser product TypeScript check failed: …TS2304…'Vec3'…TS2552…'Object3D'"

# node/grep assertions mirroring tests/run.sh (browser-app dist)
test -f dist/faber-esm/faber-browser.js
test -f dist/controllers.json
node -e "const c=require('./dist/controllers.json'); process.exit(c.controllers.length===11 && c.version===1?0:1)"
```

No cargo was invoked; no sibling-repo files were modified (both `examples` and
`triga` trees remained clean; generated outputs there are gitignored). The
staged generated TS for both fixtures was captured out-of-tree under
`/tmp/bwp-u1/staged` via the tsc shadow wrapper.
