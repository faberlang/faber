# Faber Onboarding — Stage 0: Golden Path Inventory And Lie List

**Status**: done — Stage 0 inventory complete (evidence date 2026-08-07)
**Campaign**: [faber-onboarding](CAMPAIGN.md) — Stage 0 of 10
**Delivery spec**: [delivery-stage0.md](delivery-stage0.md) (commit `3b5fcfc`)
**Artifact**: single-file inventory + lie list + open-decisions handoff (D1–D9)
**Control plane**: `faber`; all other repos read-only (cista, faberlang.dev, norma,
triga, examples, radix, GitHub releases)
**Scope guard**: this file is the only write. No product code, no release
execution, no site rebuild, no touches to `component-release-streamline/`.

---

## 0. Method, evidence discipline, and caveats

Method: read-only observation plus live commands executed **against the
published `faber-v1.4.0` archive in hermetic scratch directories** (fresh
`HOME`, no sibling repos, no env overrides, scratch stores). No builds of the
product, no store mutation in real locations, no release execution, no git
pushes. Every row cites a file (`file:line` where stable) or an observed
command result; anything not directly verified is labeled `unknown`.

Evidence index (all observed 2026-08-07 unless noted):

| # | Evidence | Source (read-only) | Result |
|---|---|---|---|
| E1 | Site start track, English | `faberlang.dev/src/en-US/start/{install,index,hello,commands,projects,examples}.md` | Install page documents **Faber 1.4.0**; verify step = `faber --version` + `faber explain SEM001`; first-package check = clone `examples` + `faber check examples/ai-workbench/packages/faber-ai` |
| E2 | Site start track, 6 other locales | `faberlang.dev/src/{zh-Hans,zh-Hant,ar,hi,vi,th-TH}/start/install.md` | All six document **Faber 1.2.0** (en-US: 1.4.0) — stale by two minors |
| E3 | Published archive contents | `github.com/faberlang/releases` release `faber-v1.4.0` (published 2026-07-31), 4 assets | `tar -tzf` of `faber-v1.4.0-aarch64-apple-darwin.tar.gz` lists exactly `faber-v1.4.0-aarch64-apple-darwin/{faber,README.txt}`; SHA-256 verified (`14413108…b04e2d`); `README.txt` = "faber v1.4.0 / target: aarch64-apple-darwin / source: …/tree/v1.4.0" |
| E4 | Published binary live, clean room | `./faber` from the archive, hermetic cwd + fresh `HOME`, `/var/tmp` scratch | `faber --version` → `faber 1.4.0`; `faber explain SEM001` → **error: failed to load reader locale 'la' pack '/Users/runner/work/faber/faber/faber/../radix/stdlib/reader/la/pack.toml'** (baked CI build path); `faber explain functio` → **error: Faber reference pack not found**; `faber init` scaffolds `[build] target = "rust"`; `faber check` on scaffold → `ok:`; `faber run` → Cargo (73 crates from crates.io) → `Salve, munde!` |
| E5 | Reference-pack walk-up leak | `faber` binary run from `/tmp`, `/var/tmp`, `/` | `faber explain functio` **appeared green** until the binary was placed in a hermetic dir; a stray sibling checkout at `/tmp/radix/corpus` (present on this machine) satisfies `dev_repo_root` walk-up — the classic "works on my monorepo" false green |
| E6 | First-package check, clean room | clone of public `faberlang/examples` (depth 1) + published binary | `faber check examples/ai-workbench/packages/faber-ai` → **error: failed to load reader locale 'la' pack '/Users/runner/work/faber/faber/radix/stdlib/reader/la/pack.toml'** — the package declares `[reader] locale = "la"` |
| E7 | CLI install fail-closed | published binary, scratch `--store` | `faber install triga` → error "registry install requires an exact name@version pin"; `faber install` (no args) → clap usage error |
| E8 | CLI source: explain path | `faber/src/diagnostic_explain.rs:169-199` | `installed_locale_pack_path` hardcodes `env!("CARGO_MANIFEST_DIR")/../radix/stdlib/locale/<locale>/pack.toml` — compile-time absolute path, no installed-pack fallback; released v1.4.0 variant used `radix/stdlib/reader/…` (layout since renamed to `locale/` on main) |
| E9 | CLI source: reference path | `faber/src/reference.rs:380-399, 489-495, 546-586` | `resolve_reference_root`: env `FABER_REFERENCE_ROOT` → install sibling (`share/faber/reference`, `lib/faber/reference`) → dev-repo walk-up. No embedded pack |
| E10 | CLI source: run default | `faber/src/commands/run.rs:225-238` (main), `faber` tag `v1.4.0` `src/commands/init.rs` | main: untargeted `run` → portable `FHIR → FMIR`, "never probes Cargo"; **released v1.4.0 `init` scaffolds `[build] target = "rust"`** and run compiles via Cargo (observed E4). The portable default exists on main but is **not released** |
| E11 | CLI surface gaps | `faber --help` (local 1.3.0 binary), `faber/src/cli/mod.rs` grep, `faber/src/commands/*` | No `doctor` command anywhere (neither released nor current source); no `self update` / `self uninstall` |
| E12 | Core-support materialization | `faber/src/core_support.rs`, `faber/src/core_support/materialize.rs`, `faber/core-support-manifest.txt` | Core-support (`faber-runtime`, `radix-runtime-contract`, hosts crates) is **embedded in the binary** (`include_bytes!(OUT_DIR/core-support.tar.zst)`) and materialized to the platform cache (`~/Library/Caches` macOS, `~/.cache` Linux). The materialized tree contains **no reference pack** (`find … -name index.toml` → empty) |
| E13 | Cista store + lock | `cista/src/store.rs:20-37`, `cista/src/commands/install.rs` (`PLATFORM_DEFAULT_PACKAGES = ["norma"]`), `cista/src/faber_lock.rs:4, 294-343` | Store root: `--store` → `CISTAE_HOME` → `~/.faber/cistae`. Lock rewrite writes **canonicalized absolute** `package_root` / `interface_root` / `target_manifest`; "faber consumes absolute paths from it without knowing about the package store" |
| E14 | Norma/Triga manifests | `norma/cista.toml`, `triga/cista.toml` | Norma: package `norma` `0.1.0`, `faber_min = "0.38.0"`, target rust `mode = "compile"` (install compiles via rustc). Triga: package `triga` `0.2.0`, target rust `mode = "compile"` |
| E15 | Example locks (drift) | `examples/triga-budapest/faber.lock`, `examples/hello-voxel/faber.lock`, `examples/vivilite/faber.lock` | Triga examples pin `triga 0.1.0` with `source = "path"`, absolute `package_root = /Users/ianzepp/work/faberlang/triga` (live manifest 0.2.0); vivilite lock uses relative `../sqlite` |
| E16 | Example manifests | `examples/ai-workbench/packages/faber-ai/{faber.toml,src/commands/*.fab}`, `examples/vivilite/faber.toml` | `faber-ai` has `[reader] locale = "la"`, imports `norma:solum`/`norma:processus`/`norma:model`, and **no `faber.lock`**; vivilite declares `[dependencies] sqlite = "0.1.0"` + `[reader] locale = "la"` |
| E17 | Release CI + payload | `faber/.github/workflows/release.yml`, `faber/core-support-manifest.txt` | Matrix `ubuntu-latest` + `macos-14` (macos-13 Intel removed: "GitHub-hosted queue is often unbounded"); checks out siblings at **default branches**; payload = `faber-runtime`, `radix/crates/radix-runtime-contract`, `hosts/crates/*` |
| E18 | Site honesty self-assessment | `faberlang.dev/CONTENT-PLAN.md` | Install page container-verified quickstart row unticked; "the fastest to go stale — it names versions, install commands, and paths"; translation `source_commit` tracking exists but a recorded `source_hash` is already stale |
| E19 | Installed-binary drift | `faber --version` on PATH (this machine) | Local binary reports **1.3.0**; site documents 1.4.0. Homebrew cache shows `faber--0.38.0.tar.gz` (formula-era) |
| E20 | Locale packs in radix tree | `radix/stdlib/locale/{ar,en,hi,la,th-TH,vi,zh-Hans,zh-Hant}/pack.toml` | All eight locale packs exist **in the monorepo only**; none shipped in the release archive |

**Sibling-WIP interlock (not chased):** `component-release-streamline` and
`release-and-portable-default` both own facts that touch releases. Facts above
are recorded at the 2026-08-07 state. In particular `release-and-portable-default/
delivery.md` still states "current product defaults still point at Rust" — true
for the **released** 1.4.0, while **main** has since removed the init Rust
default (E10). Both statements are recorded; the release state governs the
published golden path.

**Foreign WIP note:** the faber working tree carries unrelated uncommitted
changes (`src/package/*`, `crates/exempla/*`, `component-release-streamline/
CAMPAIGN.md`) and a stray radix checkout exists at `/tmp/radix` on this machine.
Neither was touched. E5/E6 were captured with the **published archive binary**,
not the working tree.

---

## 1. Golden path — step-numbered (D2)

Cold-reader path from "opened install page" through "ran a package that imports
Norma", plus the optional Triga branch. Every step: command(s), expected
outcome, live evidence (E# or file), and tag
`must-work-without-monorepo` vs `developer-only`.

Step numbering matches the site funnel
(`start/install` → `start/hello` → `start/commands` → `start/projects`).

| Step | Command(s) | Expected outcome (per site) | Live evidence (2026-08-07) | Verdict | Tag |
|---|---|---|---|---|---|
| 0 | Open `faberlang.dev/start/` → `/start/install.html` | Start track entry; en-US + 6 other locales have full start pages | E1, E2; all 7 locales have `install.md`+`hello.md`+`commands.md`+`projects.md`+`examples.md`+`index.md` | green (but see G6) | must-work-without-monorepo |
| 1 | `curl -fsSL -o faber.tgz …faber-v1.4.0-<triple>.tar.gz` | Download current release archive | E1; archive fetch OK, assets exist (E3) | green | must-work-without-monorepo |
| 2 | checksum: compare first field of `.sha256` with local `shasum -a 256` | Checksum matches | E3: `14413108…` verified | green | must-work-without-monorepo |
| 3 | `tar -xzf faber.tgz`; place `faber-v1.4.0-<triple>/faber` on `PATH` | Binary present; archive extracts to a single `faber` + `README.txt` | E3 | green | must-work-without-monorepo |
| 4 | `faber --version` | `faber 1.4.0` | E4: `faber 1.4.0` | green | must-work-without-monorepo |
| 5 | `faber explain SEM001` (site "Verify" step) | "a version line … and a diagnostic explanation" | **Fails** clean-room: reader locale `la` pack not found at baked CI path (E4, E8) | **broken — lie L1/G1** | must-work-without-monorepo |
| 6 | `mkdir salve-munde; …` manual `faber.toml`+`src/main.fab` (site hello) **or** `faber init salve-munde` | Package scaffold; init writes `faber.toml` + `src/main.fab` with `Salve, munde!` | E4: init wrote `[package] name=… version=0.1.0 edition=2026` + `[paths]` + `[build] target="rust", kind="bin"`; entry `incipit { nota "Salve, munde!" }` | green (scaffold) | must-work-without-monorepo |
| 7 | `faber check .` | `ok: <name>` — front-end lex/parse/type-check, no native build | E4: `ok: salve` (scaffold has no `[reader] locale`, so no reader pack needed) | green | must-work-without-monorepo |
| 8 | `faber run .` | `Salve, munde!` | E4: **works but via Cargo** — `Updating crates.io index`, 73 crates compiled, `faber-runtime` compiled from materialized core-support, output `Salve, munde!`. Requires Rust toolchain + network; not documented on install page (G4) | **conditional lie L3/G4** | must-work-without-monorepo |
| 9 | `git clone https://github.com/faberlang/examples.git; faber check examples/ai-workbench/packages/faber-ai` (site "First package check") | "type-check" the example | **Fails** clean-room: `[reader] locale = "la"` → reader pack at baked CI path missing (E6). Also `faber-ai` imports `norma:*` with **no `faber.lock`** — store-based resolution is unexercised/unverifiable here | **broken — lie L2/G3** | must-work-without-monorepo |
| 10 | Norma acquisition: `faber install <norma-source>` | One-command stdlib install | No product path. `faber install norma` (bare) → fail-closed pin error (E7); registry `name@version` needs a `--registry`/`CISTA_REGISTRY` that no public registry satisfies; git URL install exists but unpinned + compiles rust target (E13, E14). Norma is a cista **platform default** (not a `[dependencies]` entry) with no seeding path in the release (G5) | **broken — lie L4/G5** | must-work-without-monorepo |
| 11 | `importa ex "norma:solum" privata solum` in a package + `check`/`run` | Norma import resolves | Works only in dev layouts (monorepo store/lock or `FABER_LIBRARY_HOME`); on a binary-only install there is no store/lock to resolve from (E13, E16) | **broken — G5** | developer-only today |
| 12 | Triga branch (optional): `faber install <triga-source>` + `importa ex "triga:math" privata math` | Declared dep, lock, check | Same acquisition gap as step 10; live manifest `0.2.0` vs example locks `0.1.0` + absolute machine paths (E14, E15); example lock content is machine-specific | **broken — G5/G9** | developer-only today |
| 13 | Multi-locale: follow start track in a non-English locale; run with a non-default diagnostic/code locale | Same golden path in English + one other locale | 6 of 7 site locales are stale at 1.2.0 (G6); CLI `--reader-locale` / `--diagnostic-locale` (present, E10/help) depend on reader packs that are monorepo-only → fail clean-room for any non-default locale (G10) | **broken — G6/G10** | must-work-without-monorepo |
| 14 | Diagnostics when something is missing | Fail closed, one actionable next step | Partial: bare-name install fails closed with an actionable pin error (E7); but the two most common failures (explain, run prerequisite) either die on a baked build path (E4) or silently require Cargo (E4/G4). No `faber doctor` surface exists (E11) | **partial — G5/G11** | must-work-without-monorepo |

**Explicit split (campaign gate):** steps 0–10 are `must-work-without-monorepo`;
they are the site's own promises. Steps 11–12 are today `developer-only` and
**must not** be presented as a public install story until G5 lands. `faber build
… -t rust`, `FABER_LIBRARY_HOME`, dev-workspace `explain`, and "build from
source" are `developer-only` by design and are not part of the cold-reader path.

**Stop-if evidence (D8):** the primary install channel *can* be named without
inventing a release process — **GitHub prebuilt archive is the only channel with
a current artifact (E1/E3); Homebrew is explicitly non-authoritative on the site
and the observed formula is 0.38.0-era (E19)**. So no need is routed; the
channel question is recorded for Stage 1 formalization (see §8).

---

## 2. Desired-end-state coverage matrix (D3)

One row per Desired End State outcome from `CAMPAIGN.md` §"Desired End State".
Each row: current-state summary, gap, owner (repo + stage, never blank).

| # | Outcome (campaign) | Current state (2026-08-07) | Gap | Owner |
|---|---|---|---|---|
| O1 | Install a current binary from the documented primary path and prove it with `--version` + a tiny non-build check | Download + checksum + `--version` work (E3/E4). The documented non-build check **`faber explain SEM001` fails** clean-room (E4, E8); `faber explain` reference lookups also fail clean-room (E4, E9) | Diagnostic + reference packs not shipped; resolver bakes a build-machine path; no `doctor` surface | faber Stage 2 (payload: reference/locale packs) + Stage 4 (doctor/self-check) |
| O2 | Hello — create/init, `check`, and **`run` on the documented default** execution path | `init` + `check` green; `run` works **only via Rust/Cargo + network** on the released binary (E4). Portable FHIR/FMIR default exists on main but is unreleased (E10) | Released default execution target ≠ portable; install page documents no Rust/network prerequisite | faber Stage 5 (default execution) + release-and-portable-default (clean-room `no-rust` profile); faberlang.dev Stage 8 |
| O3 | Project — understand layout (`faber.toml`, entry, lock) | Manifest + entry + lock shape documented (E1 hello page); locks work, but written with absolute paths and example locks embed machine paths (E13/E15) | Lock portability contract open; example lock drift (0.1.0 vs 0.2.0) | cista (lock writer) + faber Stage 1 (package-and-lock contract) |
| O4 | Libraries — obtain Norma + Triga via a product command, import them, fail closed | No newcomer-usable command (E7/E14); imports resolve only in dev layouts (E16); fail-closed message exists for bare-name installs | Registry/pin/bootstrap source + rust compile-at-install prerequisite + Norma platform-default seeding | cista (routed need: pin/registry/bootstrap) + faber Stage 6 (Norma), Stage 7 (Triga) |
| O5 | Locales — English + one other site locale; non-default diagnostic/code locale without dead ends | Site locales present but **6/7 stale at 1.2.0** (E2); CLI locale flags exist (E10) but reader packs are monorepo-only (E20) | Version drift; reader/diagnostic pack distribution; package-owned locale transport | faberlang.dev Stage 8 + faber Stage 2 (packs) + faber Stage 8 (CLI locale) |
| O6 | Honesty — every golden-path command container-verified or labeled residual | Site labels Homebrew residual and container verification residual (E1/E18); but the broken steps above are **not labeled residual** — the site ships them as current instructions | Clean-room gate absent (CONTENT-PLAN unticked, E18); per-command residual labels missing | faberlang.dev Stage 8 + faber Stage 10 (continuous honesty CI) |

---

## 3. Surface survey (D5)

One row per Problem-table surface with current-state + live evidence. All
**seven site locale dirs** were surveyed for the locales row.

| Surface | Current state (2026-08-07) | Evidence |
|---|---|---|
| Website start track | en-US install page documents 1.4.0 with archive+checksum+verify+first-package steps; Homebrew and container-verification labeled residual; en-US is the only locale at 1.4.0 | E1, E18 |
| CLI | `init`/`check`/`build`/`run`/`script`/`install`/`explain`/`test`/`targets` + locale flags; no `doctor`, no `self update`/`self uninstall`; released run = Rust/Cargo path, main = portable FHIR/FMIR default | E4, E8–E11 |
| Cista store | Store root `--store` → `CISTAE_HOME` → `~/.faber/cistae`; install rewrites `faber.lock` with absolute canonicalized paths; Norma is a hardcoded platform-default package; bare names fail closed; git install unpinned | E7, E13 |
| Norma / Triga distribution | Source packages with cista manifests (norma 0.1.0 `faber_min 0.38.0`; triga 0.2.0), both rust `mode = "compile"`; no release-path seeding; example locks pin triga 0.1.0 with absolute machine paths | E14, E15 |
| Locales | Site: 7 locale dirs, all with the same 6 start files; **6/7 document 1.2.0**; en-US documents 1.4.0. CLI: 8 reader packs exist in `radix/stdlib/locale/` (monorepo-only, not shipped); `--reader-locale` / `--diagnostic-locale` flags present | E1, E2, E20 |
| Release honesty | Archive = `faber` + `README.txt` only; SHA-256 files published; no provenance manifest, no license file, no installer receipt, unsigned; CI checks out siblings at default branches; site labels formula/container checks residual | E3, E17, E18 |

---

## 4. Release honesty (D6)

**Observed archive contents (verified, 2026-08-07):** `faber-v1.4.0-<triple>.tar.gz`
contains exactly the `faber` executable and `README.txt` (E3). README.txt names
version, target, and source URL only (E3). No reference pack, no locale packs,
no Norma/Triga, no installer receipt, no license file, no sibling-source
provenance manifest. Core-support (runtime crates) is embedded in the binary
and materialized to the platform cache (E12).

**Dev-kit product definition (CAMPAIGN.md §Product Definition) requires four
layers:** launcher; core support; reference/locale packs; Faber libraries — with
deterministic locations, no hidden env vars, and a diagnostic naming missing
layers. The released archive supplies layer 1 and (embedded) layer 2 only.
Layer 3 is **absent** (explain fails clean-room, E4/E9); layer 4 is **absent**
and unacquirable via product command (E7/E14).

**Labels:** archive contents = **verified** by direct observation. "Dev kit is
installable as a coherent product" = **residual** (no proof; the clean-room gate
is a `release-and-portable-default` deliverable). "Works on my monorepo" is
**explicitly not** release-proof — the false green in E5 demonstrates the trap.
All "✓ on this workspace" claims are labeled `developer-only` in §1.

---

## 5. Lie list (D4)

Every lie / monorepo assumption / missing binary / locale dead end, with owner
and severity. **Severity: `blocking`** = a documented golden-path step fails on a
clean published-archive install; **`residual`** = drift, unshipped surface, or
honesty gap that does not stop the step it rides on.

| ID | Lie / assumption / gap | Evidence | Severity | Owner |
|---|---|---|---|---|
| G1 | `faber explain SEM001` (site verify step) works after install | Fails clean-room (E4); pack resolver bakes build path (E8); packs not shipped (E20) | blocking | faber Stage 2 + Stage 4 |
| G2 | `faber explain <term>` / `--list` works after install | Fails clean-room "reference pack not found" (E4/E9); false green from stray monorepo walk-up (E5) | blocking | faber Stage 2 + Stage 4 |
| G3 | Site "First package check" (`faber check examples/ai-workbench/packages/faber-ai`) works after install | Fails clean-room on `[reader] locale = "la"` (E6); example also imports `norma:*` with no lock (E16) | blocking | faber Stage 5 + Stage 2; faberlang.dev Stage 8 |
| G4 | Hello `faber run` needs nothing beyond the install page | Runs only via Cargo + crates.io on released binary; 73 crates fetched (E4); init scaffolds `target = "rust"` (E10); portable default unreleased (E10) | blocking | faber Stage 5 + release-and-portable-default; faberlang.dev Stage 8 |
| G5 | Norma/Triga are obtainable through a product command | No registry/pin source; bare name fails closed (E7); git install unpinned + rust compile-at-install (E13/E14); no store seeding path in release | blocking | cista (routed need) + faber Stage 6/7 |
| G6 | Multi-locale start track matches current release | 6 of 7 site locales at 1.2.0 vs en-US 1.4.0 (E2) | blocking (for the locale outcome) | faberlang.dev Stage 8 |
| G7 | Installed binary on this machine matches the site | PATH binary = 1.3.0, site = 1.4.0; Homebrew formula 0.38.0-era (E19) | residual | faberlang.dev Stage 8 + faber Stage 3 (formula-lag policy) |
| G8 | Release archive is a self-consistent product artifact | Binary + README.txt only; no provenance, no license, unsigned, sibling checkouts unpinned (E3/E17) | residual | faber Stage 2 + component-release-streamline |
| G9 | Locks are portable / reproducible | Absolute canonicalized lock paths (E13); triga example locks 0.1.0 + `/Users/ianzepp/…` paths vs live 0.2.0 (E15) | residual | cista + faber Stage 1 (package-and-lock contract) |
| G10 | CLI locale flags work for newcomers | Flags exist (E10) but reader packs are monorepo-only (E20); format/emit `--locale` docs depend on them | residual | faber Stage 2 + Stage 8 |
| G11 | Install path is container-verified | CONTENT-PLAN row unticked; site labels residual (E1/E18); no `doctor` surface (E11) | residual | faberlang.dev + faber Stage 10 |
| G12 | Platform slice is explicit | Released assets only macOS arm64 + Linux x86_64; macOS Intel dropped from CI; no Windows; no native packaging/signing (E3/E17) | residual | faber Stage 1 (platform matrix) + Stage 3 |

**Missing binaries/surfaces catalog:** no `faber doctor` (E11); no `faber self
update` / `faber self uninstall` (E11); no locale/reference packs shipped
(E20); no macOS `.pkg`/`.dmg`/signing (E3/E17); no container-verified install
CI (E18).

---

## 6. Monorepo-split notes (D2/D4)

What **must** work without a monorepo (site promises): steps 0–10 of §1 —
download, checksum, `--version`, `init`, `check`, `run`, example check,
library acquisition, multi-locale. What is **developer-only by design** and must
never be documented as user path: `FABER_LIBRARY_HOME` overrides, dev-workspace
`explain`/locale packs, monorepo store/lock resolution, `faber build … -t rust`
as a review surface, "build from source" (private radix tree).

Concrete monorepo assumptions leaking into user-facing behavior (each maps to a
gap): reader/locale packs resolved via `CARGO_MANIFEST_DIR`-relative paths (G1,
G3, G10); reference pack resolved by dev-repo walk-up with no shipped pack (G2);
example check depending on the `la` reader pack (G3); example locks embedding
machine paths (G9); core-support materialization relying on the platform cache
with no `doctor` to verify it (G12-side, G11).

---

## 7. Locale dead ends

| ID | Dead end | Evidence | Maps to |
|---|---|---|---|
| LDE1 | Non-default **diagnostic** locale on a clean install: `--diagnostic-locale` requires the reader pack at the baked build path | E4/E8/E20 | G1, G10 |
| LDE2 | Non-default **code/reader** locale on a clean install: `--reader-locale`, `faber format --locale=`, `faber emit --locale=` (documented on `commands.md`) require monorepo packs | E1/E20 | G10 |
| LDE3 | Any package declaring `[reader] locale = "la"` (ai-workbench, vivilite, coreutils) fails `check`/`run` on a clean install | E6/E16 | G3 |
| LDE4 | Non-English site start track installs a stale binary (1.2.0) | E2 | G6 |
| LDE5 | Package-owned API locale transport (Stage 8 gate) — no mechanism documented; locale metadata in cista snapshots is not specified | E14 (packages carry no locale metadata) | O5 (residual, Stage 8) |

Fixing all of the above is **Stage 8** (site locales) / **Stage 2+4** (pack
distribution) / **Stage 1** (package-owned locale transport). Stage 0 records
them; it fixes nothing.

---

## 8. Open-decisions handoff to Stage 1 (D7)

The campaign's 9 Open Questions, each marked
`answered-by-evidence` | `carried-to-stage-1` | `needs-stage-1-decision`.
This list is the Stage 1 decision-input handoff; **no Stage 1 artifact is
pre-written (D9)**.

| OQ | Question | Mark | Stage 0 evidence handed to Stage 1 |
|---|---|---|---|
| OQ1 | Canonical payload representation | `needs-stage-1-decision` | Today: single binary + embedded core-support materialized to platform cache (E12); archive ships launcher only (E3). Dev-kit needs 4 layers (CAMPAIGN §Product Definition) |
| OQ2 | Norma model | `needs-stage-1-decision` | Norma is cista hardcoded platform-default `["norma"]` (E13), source manifest 0.1.0 / `faber_min 0.38.0` (E14), no seeding path in release (G5) |
| OQ3 | Portable lock and restore | `needs-stage-1-decision` | Locks written absolute (E13); example locks machine-specific + version-drifted (E15); vivilite lock relative (`../sqlite`) shows a relocatable shape exists (E15) |
| OQ4 | Git/registry bootstrap | `needs-stage-1-decision` | Git install unpinned, no registry, no remote HTTPS fetch in faber install (E7/E13); bare names fail closed — a pin/registry source must be selected |
| OQ5 | Default newcomer execution | `answered-by-evidence` (state) + `needs-stage-1-decision` (choice) | Released 1.4.0: init scaffolds `target = "rust"`, run via Cargo (E4/E10). Main: portable FHIR/FMIR default, no Cargo probe (E10) — **not released**. The `no-rust` clean-room profile (CAMPAIGN §Named profiles) cannot pass on the release |
| OQ6 | Dependency graph | `carried-to-stage-1` | Manifests carry no transitive graph; rust `mode = "compile"` exists (E14); third-party ecosystem unexpressed |
| OQ7 | Init language surface | `answered-by-evidence` | Released init scaffolds Latin `Salve, munde!` (E4); site hello is Latin (E1). Whether init becomes locale-parameterized is Stage 1 |
| OQ8 | macOS-native value | `carried-to-stage-1` | No `.pkg`/`.dmg`, no signing/notarization anywhere (E3/E17); archive is the only macOS path today |
| OQ9 | Deferred platforms | `answered-by-evidence` + `needs-stage-1-decision` (slice) | Released assets: macOS arm64 + Linux x86_64 only; macOS Intel removed from CI; Windows absent (E3/E17) |

**The three gate decisions, named explicitly:**

1. **Is a public registry required?** Evidence: **no** for the local-store
   mechanism (store is `CISTAE_HOME`/`~/.faber/cistae`, E13). Norma/Triga
   acquisition needs *an* immutable verified bootstrap source (OQ4); a public
   `cista.dev` registry is not required to start Stage 6/7. Mark:
   `carried-to-stage-1` — do not block on `cista-dev-registry`.
2. **Which install channel is primary?** Evidence: **GitHub prebuilt archive is
   the only channel with a current artifact**; Homebrew is explicitly
   non-authoritative and 0.38.0-era in practice (E1/E19). Mark:
   `answered-by-evidence` — archive primary; Homebrew secondary/residual; Stage
   1 formalizes (OQ8 native packaging separately).
3. **Default execution target for newcomers?** Evidence: released 1.4.0 = Rust
   (Cargo); main = portable FHIR/FMIR, unreleased (E4/E10). Mark:
   `needs-stage-1-decision` — must align with `release-and-portable-default`
   clean-room `no-rust` profile and the site's prerequisites page.

**Council-4 interlock — recorded for the Stage 1 planner (not resolved):**
`faber/docs/release/` is the named landing spot for both `component-release-
streamline` Stage 1 (coordinated product release process + release manifest
schema) and this campaign's Stage 2 dev-kit payload manifest (machine-readable
manifest of every component, version, digest, compatibility bound, license,
destination). **Both campaigns' Stage 1 decision outputs converge on the same
directory; a single routing authority for `faber/docs/release/` must be decided
at the campaigns' Stage 1 planning before the outputs overlap** (council-4).
Stage 0 records the overlap only; it writes nothing there and pre-writes no
schema (D9).

---

## 9. Stop-if check (D8)

Primary channel nameable without inventing a release process: **yes** — GitHub
prebuilt archive (current artifact, verified checksums, site-documented) is
primary; Homebrew explicitly non-authoritative (site labels residual; observed
formula is stale). Therefore **no need is routed** to
`release-and-portable-default` as a precondition of this inventory. The
clean-room gate itself remains a sibling delivery; this stage's rows already
separate verified vs residual per §4.

---

## 10. Residuals / observations for the Mind (out of Stage 0 scope)

- `faber check`/`run`/`build` on a package with `[reader] locale` also breaks
  `faber build`/`faber run` flows beyond the site's example — the la-pack
  dependency is package-wide, not just the documented check (G3/G10).
- `faber install`'s git path is **unpinned by design today** (no required
  revision/checksum); any Stage 1 "verified bootstrap" choice must decide
  whether to harden the faber path or route to cista (OQ4).
- The materialized core-support cache is keyed by content hash and immutable —
  a good base for the Stage 2 payload, but nothing verifies it end-to-end today
  (`doctor` absent, G11).
- The site's `source_commit`/`source_hash` translation-tracking mechanism exists
  but has already drifted (E18); Stage 8 should treat parity as an enforced
  gate, not a hash note.
- This inventory's live runs used the **published** archive; a locally built
  binary would bake this machine's paths and mask G1/G3. That is itself the
  clean-room argument the campaign makes.

## 11. D-reference summary

- **D1** — file committed under `faber/docs/factory/faber-onboarding/`. ✓
- **D2** — step-numbered path §1, every step has commands, expected outcome,
  live evidence (E#) or `unknown`, and a `must-work-without-monorepo` /
  `developer-only` tag. ✓
- **D3** — §2: 6/6 desired-end-state outcomes with current-state row, gap,
  owner (repo + stage). ✓
- **D4** — §5: 12-item lie list (G1–G12) each with owner + severity; missing
  binaries/surfaces cataloged; locale dead ends in §7. ✓
- **D5** — §3: all 6 Problem-table surfaces surveyed; all 7 site locale dirs
  surveyed (E1/E2). ✓
- **D6** — §4: observed archive contents (verified) vs Dev-Kit product
  definition, labeled verified vs residual; no monorepo claim as release-proof. ✓
- **D7** — §8: 9/9 Open Questions marked; 3 gate decisions named; Stage 1
  handoff list; council-4 interlock recorded. ✓
- **D8** — §9: primary channel named without inventing a release process; no
  need routed. ✓
- **D9** — no Stage 1 artifact pre-written; no decision records authored. ✓
