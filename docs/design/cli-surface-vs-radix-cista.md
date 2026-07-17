# Faber CLI surface vs Radix + Cista — product usability coverage

**Status:** head-cto analysis (task 7cc973c7)
**Scope:** product-level command surface for a developer who installs Faber and
does real work. Not 100% internal parity.
**Evidence source:** live `--help` output from release binaries
(`faber` v1.1.0, `radix` v0.38.0, `cista` v0.1.0), clap struct definitions, and
`docs/help/*` after-help text. No stale docs cited without live confirmation.

---

## 1. Inventory

### Faber (v1.1.0) — user-facing commands

| Command | Purpose | Key flags |
| --- | --- | --- |
| `-c` / `--command` | One-liner eval via MIR stepper | trailing args after `--` |
| `build` | Compile file/package → disk | `-t`, `-o`, `--release`, `--format`, `--linter`, `--reader-locale`, `--package` |
| `targets` | Show supported targets + capability notes | — |
| `check` | Semantic analysis (file or package) | `--diagnostics`, `--permissive`, `--reader-locale`, `--package` |
| `verify` | HIR aspect verification (single file) | inherits `radix::tool::VerifyArgs` |
| `verify-library` | Verify library package target binding manifest | `--target`, `<input>` |
| `init` | Create new Faber package | `<path>` |
| `install` | Install/update public source library via `FABER_LIBRARY_HOME` | `<library>` |
| `explain` | Language reference (glyphs, keywords, grammar) | `--json`, `--search`, `--list`, `--category`, `--reader-locale` |
| `run` | Build (if needed) + run compiled package | `-t`, `--release`, `--interpret`, `--compile`, `--reader-locale` |
| `script` | Interpret source via MIR stepper (file/pkg/archive) | trailing args after `--` |
| `repl` | Interactive MIR stepper REPL | trailing args after `--` |
| `test` | Run package tests via generated Rust harness | `--name`, `--suite`, `--tag`, `--exact`, `--nocapture`, `--ignored` |
| `lex` | Tokenize → JSON (radix alias) | `<input>` |
| `parse` | Parse → AST JSON (radix alias) | `<input>` |
| `hir` | Lower to HIR → JSON (radix alias) | `<input>` |
| `cli-ir` | CLI IR → JSON (radix alias) | `<input>` |
| `emit` | Compile to target → stdout (radix alias, pkg-aware) | `-t`, `--format`, `--linter`, `--reflection`, `--output`, `--reader-locale`, `--package` |
| `format` | Format Faber source (author mode default) | `--canonical`, `--check`, `--stdout`, `--reader-locale`, `--config` |
| `host` | Script host introspection (kernel manifest) | — |
| `__fmir-run` | *(hidden)* internal FMIR image runner | — |

### Radix (v0.38.0) — developer tooling commands

| Command | Purpose | Key flags |
| --- | --- | --- |
| `lex` | Tokenize → JSON | `<input>` |
| `parse` | Parse → AST JSON | `<input>` |
| `hir` | Lower to HIR → JSON | `<input>` |
| **`mir`** | Lower to MIR → deterministic text dump | `<input>` |
| `cli-ir` | CLI IR → JSON | `<input>` |
| `check` | Semantic analysis | `--diagnostics`, `--reader-pack`, `--permissive`, `--package` |
| `verify` | HIR aspect verification | `--package` |
| `emit` | Compile to target → stdout | `-t`, `--format`, `--linter`, `--reflection`, `--output`, `--reader-pack`, `--diagnostics`, `--package` |
| `targets` | Show supported targets + capability notes | — |
| **`abi`** | Print host ABI contract | `--format json\|rust`, `--output` |

### Cista (v0.1.0) — package-store commands

| Command | Purpose |
| --- | --- |
| `init` | Create low-level package skeleton |
| `check` | Validate manifest, interfaces, bindings, resolver metadata |
| `inspect` | Inspect package path or identifier |
| `metadata` | Emit machine-readable package metadata |
| `graph` | Print resolved package/provider graph |
| `resolve` | Resolve dependencies + runtime bindings (no compile) |
| `fetch` | Fetch package metadata/artifacts into cache |
| `install` | Install local/registry package into shared store |
| `run` | Run executable from installed binary package |
| `remove` | Remove package from store/cache |
| `update` | Refresh metadata + cached artifacts |
| `cache` | `list \| path \| prune \| clean` |
| `package` | `list \| show \| files \| interfaces \| runtimes` |
| `runtime` | `list \| show \| verify \| bindings` |
| `target` | `list \| show \| verify` |
| `publish` | Publish package to registry |
| `yank` | Yank published package version |
| `login` | Authenticate to registry (token from env var) |
| `logout` | Remove registry credentials |
| `doctor` | Run package-store health checks |

---

## 2. Capability map — product capability → Faber command (or gap)

| Product capability | Faber command | Status |
| --- | --- | --- |
| Create new project | `faber init <path>` | ✅ covered |
| Semantic check (file) | `faber check <file>` | ✅ covered |
| Semantic check (package) | `faber check --package <dir>` | ✅ covered |
| Compile to Rust | `faber build -t rust <input>` | ✅ covered |
| Compile to other targets | `faber build -t <target> <input>` | ✅ covered (14 targets) |
| Compile to stdout (inspection) | `faber emit -t <target> <input>` | ✅ covered |
| Build MIR package artifacts | `faber build -t fmir-text\|fmir\|fmir-bin` | ✅ covered |
| Run compiled package | `faber run <path> -- args` | ✅ covered |
| Interpret source (no Cargo) | `faber script <path> -- args` | ✅ covered |
| Interactive REPL | `faber repl` | ✅ covered |
| Run tests | `faber test <path>` | ✅ covered |
| Format source | `faber format [paths]` | ✅ covered |
| Language reference | `faber explain <term>` | ✅ covered |
| Tokenize → JSON | `faber lex <file>` | ✅ covered (alias) |
| Parse → AST JSON | `faber parse <file>` | ✅ covered (alias) |
| Lower to HIR → JSON | `faber hir <file>` | ✅ covered (alias) |
| CLI IR → JSON | `faber cli-ir <file>` | ✅ covered (alias) |
| Aspect verification | `faber verify <file>` | ✅ covered |
| One-liner eval | `faber -c 'source'` | ✅ covered |
| Host introspection | `faber host` | ✅ covered |
| **Lower to MIR → text dump** | — | ⚠️ **gap** (radix-only: `radix mir`) |
| **Host ABI contract** | — | ⚠️ **gap** (radix-only: `radix abi`) |
| **Install package from store** | — | ⚠️ **gap** (cista-only: `cista install`) |
| **Publish package** | — | ⚠️ **gap** (cista-only: `cista publish`) |
| **Package store inspection** | — | ⚠️ **gap** (cista-only: `cista inspect/package`) |
| **Run installed binary** | — | ⚠️ **gap** (cista-only: `cista run`) |
| **Remove/update package** | — | ⚠️ **gap** (cista-only: `cista remove/update`) |
| **Registry auth** | — | ⚠️ **gap** (cista-only: `cista login/logout`) |

---

## 3. Radix-only surface — useful compiler capabilities not reachable via Faber

| Capability | Radix command | Severity | Assessment |
| --- | --- | --- | --- |
| **MIR text dump** | `radix mir <file>` | **Power-user ok** | Single-file MIR inspection for compiler debugging. Faber aliases `lex`/`parse`/`hir`/`cli-ir`/`emit` but omits `mir`. A user debugging codegen who is already in `faber` workflow must context-switch to `radix mir`. Low-impact: most users never need MIR output. Fix is a trivial alias if desired. |
| **Host ABI contract** | `radix abi --format json\|rust` | **Power-user ok** | Emits the compiler's host ABI contract (function signatures the generated code expects from the host runtime). Used by CI, release packaging, and `faber-runtime` maintainers. Not needed by application developers. Correctly stays in `radix`. |
| **`--reader-pack` (raw TOML path)** | `radix check/emit --reader-pack <path>` | **Intentional divergence** | Faber uses `--reader-locale` (locale name → resolves to pack path); radix uses `--reader-pack` (raw path). This is by design — Faber provides the higher-level UX. Not a gap. |

### Targets in `faber build` but not `faber emit`

Faber `emit` intentionally excludes package-only MIR artifact targets
(`fmir-text`, `fmir`, `fmir-bin`, `scena`) from its target enum — those belong
to `build`/`run`. This is correct layering, not a gap.

---

## 4. Cista-only / planned — package flows users need that Faber does not front

This is the most significant usability area. Cista is a full package manager
(20 commands) that is completely disconnected from Faber at the binary level.

### 4.1 Dual-entry confusion: `faber install` vs `cista install`

| Mechanism | `faber install <library>` | `cista install --path <dir>` |
| --- | --- | --- |
| **What it does** | Git-clones a public source library into `FABER_LIBRARY_HOME` | Snapshots a local package into `$CISTAE_HOME` store with interface/target/binding metadata |
| **Store** | `FABER_LIBRARY_HOME` (flat `.fab` files) | `$CISTAE_HOME` / `~/.faber/cistae` (structured `interfaces/` + `targets/`) |
| **Lockfile** | none | rewrites project `faber.lock` |
| **Binary deps** | no | yes (bin role, `cista run`) |
| **Registry** | git remote | local/dev filesystem registry + remote HTTP (`cista.dev`) |

A user who reads "install a package" faces two incompatible commands with
different stores, different semantics, and no documentation in `faber --help`
that mentions `cista` at all.

**Architecture invariant (by design):** The Cista goal doc
(`cista/docs/factory/cista-package-store/goal.md`) explicitly mandates that
`faber` must not know `cista` exists — no crate dependency, no process
dependency, no store discovery. Cross-repo integration uses file contracts
(`faber.toml`, `faber.lock`, documented store layout). This is a deliberate
clean-architecture boundary, not an oversight.

The goal doc also states the intended direction: *"faber is a thin install
facade later"* — meaning `faber install` would eventually delegate to cista
via process spawn. This has not been implemented.

### 4.2 Capability gaps for "install Faber and do real work"

| User intent | Today | Gap |
| --- | --- | --- |
| "Install a third-party library" | `cista install --path <local>` or `cista install <name>@<ver>` | Not fronted by `faber`; two stores |
| "See what I have installed" | `cista package list` or check `FABER_LIBRARY_HOME` | Not fronted by `faber` |
| "Run an installed tool" | `cista run <name> -- args` | Not fronted by `faber` |
| "Remove a package" | `cista remove <name>` | Not fronted by `faber` |
| "Publish my package" | `cista publish --path <dir>` | Not fronted by `faber` |
| "Add a dependency to my project" | Edit `faber.toml [dependencies]` then `cista install` + lock | Two-step, two tools |
| "Update dependencies" | `cista update` | Not fronted by `faber` |

---

## 5. Product usability bar — minimum command surface

For a developer who installs Faber and does real work, the current surface is
strong for the **compile/check/run/test** loop and weak for the **package
management** loop. The recommended minimum bar is **not** full cista parity
under `faber`, but rather:

### Already met (no action needed)

1. **Project lifecycle:** `init → check → build → run → test`
2. **Source inspection:** `lex`, `parse`, `hir`, `cli-ir`, `emit`
3. **Language reference:** `explain`
4. **Format:** `format` (author + canonical)
5. **Interpretation:** `script`, `repl`, `-c`
6. **Target breadth:** 14 backends via `targets`

### Recommended minimum additions (if pursued)

1. **`faber mir`** alias — trivial parity with existing `lex`/`parse`/`hir`
   aliases. One struct delegation. Unblocks MIR debugging without leaving the
   `faber` tool. *(effort: S)*

2. **Package management discoverability** — at minimum, `faber install` should
   document or detect the cista store path and guide users to `cista install`
   when they are trying to install a store package rather than a git library.
   Today `faber install` silently uses a git-clone mechanism that is
   disconnected from the package store. *(effort: M — design decision required;
   see §4.1 invariant)*

3. **`faber add <dep>`** (or `faber deps add`) — a thin facade that writes
   `faber.toml [dependencies]` and shells out to `cista install --project`.
   This closes the "add a dependency" loop into one command. Violates no
   invariant: faber already reads `faber.toml` and `faber.lock`; it would
   spawn `cista` as an external tool. *(effort: M)*

---

## 6. Gap list — ordered recommendations for Mind → Hand tasks

| # | Gap | Severity | Effort | Recommendation |
| --- | --- | --- | --- | --- |
| 1 | **`faber mir` alias missing** | Low (power-user) | S | Add `mir` as a compatibility alias matching `radix mir`, same as `lex`/`parse`/`hir`. One clap variant + delegation. Quick win. |
| 2 | **Package management dual-entry** | Medium (usability) | M (design) | File a **need** for operator decision: should `faber install` detect cista store packages and delegate, or should docs clarify the two-store model? The repo-separation invariant means delegation = process spawn, not crate dependency. |
| 3 | **No `faber add`/dependency management front** | Medium (usability) | M | After #2 is decided, implement a thin `faber add <name>@<ver>` that writes `faber.toml` and spawns `cista install --project`. Closes the most common package loop. |
| 4 | **`faber install` silently uses git-clone, not store** | Low-medium | S (docs) | At minimum, `faber install --help` should note that this installs source libraries via git into `FABER_LIBRARY_HOME`, and that `cista install` is the package-store mechanism. Prevents user confusion. |
| 5 | **`radix abi` not fronted** | None (correctly radix-only) | — | No action. ABI contract is compiler-runtime integration, not application developer surface. |
| 6 | **No `faber package list/show`** | Low | M | Optional: thin facade over `cista package list/show`. Lower priority than #2/#3. Can defer until package management UX is decided. |

---

## 7. Release 1.1 narrative assessment

**Does any gap block the "release 1.1" narrative?** **No.**

- The **compile/check/run/test** loop — the core developer experience — is
  fully covered by Faber with broad target support.
- The **`mir` alias** gap is a convenience issue for power users who already
  have `radix` installed alongside `faber`.
- The **package management** dual-entry is a known architectural decision
  (repo-separation invariant) with a documented roadmap (Cista Phase A shipped,
  Phase B next). It is not a regression or an oversight — it is staged
  delivery.
- **Cista** is at v0.1.0 with local/dev registry working; remote `cista.dev` is
  environment-gated. Package management is explicitly early-stage in the
  release docs.

**Risk:** The main risk to the 1.1 narrative is not missing commands but
**undocumented dual-entry confusion** — a new user who tries `faber install
<package>` expecting package-store behavior gets a git clone instead. This is
addressable with documentation (#4 above) without any code change to the
release binary.

---

## Evidence notes

- All command inventories confirmed against live `--help` from release binaries
  built at the repos' current tips (faber 62d86f2, radix 69767af9, cista 0d8e819).
- Clap struct definitions cross-checked: `faber/src/cli/mod.rs`,
  `faber/src/cli/emit.rs`, `radix/crates/radix/src/tool/cli.rs`,
  `cista/src/cli.rs`.
- Package architecture from `cista/docs/factory/cista-package-store/goal.md`
  (1145 lines, comprehensive — read in full).
- `faber` has zero code dependency on `cista` (confirmed: `grep -rn cista
  faber/src/` returns only a test fixture path).
- Release interdependency analysis from
  `faber/docs/release/process-versioning-and-deps.md`.
