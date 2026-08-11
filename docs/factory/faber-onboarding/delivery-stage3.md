# Delivery — Faber Onboarding Stage 3: Primary Install Channel And Lifecycle

**Status**: ready — planning artifact (lowered); implementation units dispatch after this lands
**Campaign**: [faber-onboarding](CAMPAIGN.md) — Stage 3 of 10
**Unit title**: `faber-onboarding-stage3-channel-lifecycle`
**Control plane**: `/Users/ianzepp/work/faberlang/faber`
**Owners**: faber (installer + lifecycle + release-labeling policy); release sibling interlock (`component-release-streamline`, `release-and-portable-default` — read-only for this planning artifact); `faberlang.dev` (install pages — routed need, sibling)
**Created**: 2026-08-10
**Base**: faber main `39a5052` (clean except untracked `dist/` tarballs — Class-B foreign dirt, untouched)
**Source of truth**: CAMPAIGN Stage 3 section (outcome, gate, overlap rule) + §Install bundle contract + §Dependency rules (curl|sh row); Stage 1 decision records — `install-channel-matrix.md` (channel dispositions, checksum/provenance/signing policy, prefix rules, lifecycle owners, formula-lag policy, OQ8), `dev-kit-contract.md` (four layers, deterministic locations, discovery rules, diagnostic classes), `package-and-lock-contract.md` (store/cache semantics — uninstall survival), `platform-matrix.md` (supported slice, clean-room profiles, residual policy), `decision-ledger.md` (gate decision 2, OQ8, G7/G8/G12 routing); Stage 2 delivery (`delivery-stage2.md` — payload assembly + manifest + verify-dev-kit, the artifact this stage installs); sibling `component-release-streamline` and `release-and-portable-default` (release process). This spec pins the delivery contract; it does not reopen campaign decisions.

---

## Entry gate — Stage 2 delivered

Stage 2 is **delivered** (campaign status line; [delivery-stage2.md](delivery-stage2.md) D1–D11 closeout). The
entry gate for this stage's implementation units is therefore:

- `assemble-dev-kit` produces the four-layer payload staging directory (launcher, reference pack, all eight
  locale packs, core-support recorded) and `faber/release-manifest.yaml` is committed with
  `pinnedInputs.packs` rows (component / version / digest / compatibility / license / destination).
- `verify-dev-kit` positive **and** negative runs recorded (scratch HOME, arbitrary prefix, stray-checkout
  negative, tamper/skew negatives) — the verification basis this stage's installer reuses.
- No-rust / portable-execution claim explicitly **not** made by the payload (E4/E10 discipline; that proof
  stays with `release-and-portable-default` + faber Stage 5).

If the entry gate regresses before a unit starts, the unit holds and the gap routes to faber Stage 2
follow-up or the release sibling — never a silent workaround.

---

## Outcome

The primary channel installs the canonical Stage 2 payload. It supports a user-local prefix and
non-interactive use, reports what changed, and has **tested** reinstall, upgrade, repair, downgrade policy,
and uninstall behavior. The installer verifies checksums/provenance before install, makes PATH/shell changes
explicit and reversible, and uninstalls only product-owned files while user projects and dependency caches
survive by default. Install pages name the exact artifact/version under test; formula lag is labeled or fixed.

**Gate (CAMPAIGN Stage 3):** checksums/provenance are verified before install; PATH/shell changes are
explicit and reversible; uninstall removes only product-owned files; user projects and dependency caches
survive by default; install pages name the exact artifact/version under test; formula lag is labeled or fixed.

**Overlap rule (recorded, not negotiable):** a shell bootstrap downloads a **fixed release payload** — it is
not a second release system. Native macOS packaging must install the **same logical layout** and pass
Gatekeeper/notarization policy before being called supported (OQ8 conditions, `install-channel-matrix.md`).

**Contract sourcing (referenced, not duplicated):** every unit below pulls its policy from the Stage 1
decision records — channel dispositions and lifecycle owners from `install-channel-matrix.md`, layer layout
and discovery from `dev-kit-contract.md`, store/cache semantics from `package-and-lock-contract.md`, platform
slice/profiles from `platform-matrix.md`, routing from `decision-ledger.md` (G7, G8, G12, OQ8).

---

## Batching and unit table

**Batching: split-on-boundary** (CAMPAIGN Stage 3): **installer** (faber CLI + scripta, control plane),
**release** (release sibling + faber release interlock), **docs** (faberlang.dev + faber docs). Units are
dispatchable independently within their batch; batch order installer → release → docs for the stage gate,
with release and docs units allowed to run once their installer-dependency units land.

**est_basis**: pilot — person-days on a pilot basis; implementers record actuals against these.

### Batch A — Installer (faber)

| Unit | done_when | validation | est |
| --- | --- | --- | --- |
| **A1. Verified bootstrap installer** (`scripta/install-faber`; also the `curl` convenience path) | Downloads the **exact** Stage 2 release payload for the platform triple; verifies SHA-256 against the published checksum asset **before** any payload unpack or script execution; mismatch aborts with **no partial install** (`install-channel-matrix.md` checksum policy). Installs to a user-local prefix (`~/.local`/`~/.faber` default; explicit `--prefix` for system/agent). Non-interactive mode with stable exit codes. Reports what changed: files written, PATH/shell edits, receipt path. Proxy/offline behavior stated. **Overlap rule:** fixed release payload only — never a second release system. | Clean-room run on a scratch HOME + minimal PATH in the `linux-x64-minimal` / `macos-arm64-fresh` profile shape; tampered checksum fails **before** execution (no payload touched); report lists exact filesystem changes; agent-style non-interactive run with explicit prefix. Reuses `verify-dev-kit` positive cases at the installed prefix. | 2d |
| **A2. Reinstall idempotency + upgrade** (`faber self update`) | Same-prefix reinstall is a no-op or clean upgrade; user projects untouched. `faber self update` upgrades the launcher; versioned side-by-side installs allowed; version-lane policy per `faber/docs/release/policy.md` (odd dev / even LTS, cited by `install-channel-matrix.md`). Reports what changed. User projects + dependency caches (`$CISTAE_HOME` / `~/.faber/cistae`, `package-and-lock-contract.md`) survive by default. | Reinstall over an existing prefix; upgrade from a prior version; assert a user project and the populated store are byte-identical after; version-lane guard honored (no cross-lane silent jump). | 1.5d |
| **A3. Downgrade policy** | Pinned reinstall of an older version is supported and reversible via the side-by-side layout; a downgrade that would strand a version-incompatible pack fails closed with one actionable message — no silent mixing (`dev-kit-contract.md` layer lifecycle; `faber/docs/release/policy.md` lanes). | Downgrade one version and re-verify `--version` + `explain`; incompatible-pack negative case exits nonzero naming the layer and one next action. | 1d |
| **A4. Repair policy + failure classification (install-side)** | A broken or tampered install (missing/digest-mismatched pack, missing launcher metadata, corrupt core-support materialization) is detected by the installer and repairable by re-running install/update; the install-side state machine and failure classes match `dev-kit-contract.md` diagnostic classes and feed faber Stage 4 doctor (**full doctor surface is Stage 4 — staged, not implemented here**). | Tamper a pack → installer detects and restores via re-install; unwritable-prefix negative fails closed with one next action. | 1d |
| **A5. Uninstall** (`faber self uninstall`) | Removes **only product-owned files** (prefix install, installer receipt, kit-owned platform-cache entries) and reverses the explicit PATH/shell changes it made (`install-channel-matrix.md` prefix rules). User projects, locks, and dependency caches survive by default; removing them requires an explicit ask (`--purge`-style flag), never implicit. Reports what was removed and what was left. | Fresh HOME: install → use → uninstall; assert user project + store survive; assert no product-owned files remain outside the prefix; PATH restored. | 1.5d |
| **A6. macOS-native packaging decision (OQ8 revisit)** | **landed** (2026-08-11) — OQ8 re-reviewed after A1–A5: disposition **explicitly-deferred-with-owner**. No real OS integration beyond the archive named; drag-a-CLI `.dmg` still has no value; no signing identity (do not invent). No `.pkg`/`.dmg` artifact; native channel not labeled supported; Gatekeeper/notarization rule restated for any future signed path. Owners: `component-release-streamline` (signer prerequisite); faber-onboarding (product reopen when signer + concrete need both exist). Rows in `decision-ledger.md` + `install-channel-matrix.md`. | Ledger + matrix rows updated with disposition and owner; no `.pkg`/`.dmg` artifact; no "supported" label on an unsigned channel. | 0.5d |

### Batch R — Release (release sibling + faber interlock)

Sibling-owned rows are routed needs with a faber-side verification unit; faber never operates the release
process itself (CAMPAIGN "No monorepo assumption" + scope routing; `component-release-streamline` owns how
artifacts are cut and published).

| Unit | done_when | validation | est |
| --- | --- | --- | --- |
| **R1. Published per-triple artifacts (payload wrap + publish)** | The Stage 2 payload staging directory is wrapped and published per platform triple by the release process — archive naming + basename-only checksum per `component-release-streamline` `release-contract.md` §5.1, staged publish per its Stage 6 — so the primary channel installs a **published** artifact, not a local staging dir. The release names the exact artifact/version/digest that docs will test against (feeds D1). | `verify-dev-kit` against the **published** artifact (not a locally built binary) on the supported slice (macOS arm64 + Linux x86_64, `platform-matrix.md`); published artifact matches `release-manifest.yaml` instance pins. | 0.5d (faber verification; publish is sibling-owned) |
| **R2. Formula-lag labeled or fixed (G7)** | Homebrew channel either tracks the tested release or carries the label stating its **true served version** with the non-authoritative channel label applied (`install-channel-matrix.md` formula-lag policy; `decision-ledger.md` G7). No silent currency claim. Site wording need routed to `faberlang.dev` Stage 8. | Homebrew page/formula shows the true served version; site labels the channel non-authoritative; a stale formula never appears current. | 1d |
| **R3. Provenance/checksum publish interlock** | The release publishes the checksums + provenance manifest the installer verifies before install (`install-channel-matrix.md` checksum/provenance policy). Unsigned/unproven remains a **labeled residual** until signing lands (G8; `platform-matrix.md` signature cells) — never a silent "supported signed channel" claim. | A1's verify step consumes the published checksum asset; tamper fails before execution; residual label present in docs and matrix. | 0.5d |

### Batch D — Docs (faberlang.dev + faber docs)

Site rows are routed needs; faber writes the CLI-facing text, `faberlang.dev` owns the site pages
(decision-ledger G3/G6/G7 routing; sibling site-implementation campaign).

| Unit | done_when | validation | est |
| --- | --- | --- | --- |
| **D1. Install pages name the exact artifact/version under test** | `faberlang.dev` `start/install` pages name the exact artifact/version/digest under test (CAMPAIGN Stage 3 gate); no silent drift between page and tested release; prerequisites stated per gate decision 3 (released execution path with its prerequisites, no unreleased no-rust claim — `decision-ledger.md` gate 3). **curl|sh marketing gate (CAMPAIGN §Dependency rules):** "one curl \| sh" is only presented once Stage 3 installs and verifies the canonical Stage 2 payload. | Zombie-docs check — every command/version on the page exists at the tested release; clean-room install from the page alone in the `linux-x64-minimal` / `macos-arm64-fresh` profile shape. | 1.5d |
| **D2. Lifecycle user docs (install / update / downgrade / uninstall)** | User-facing docs for the primary-channel lifecycle match the tested CLI: install, `faber self update`, downgrade, `faber self uninstall`, reinstall; each states what changes and what survives (user projects, store). CLI help text matches (`faber self --help`, `faber install --help`). | Every documented command exists on the CLI at the tested release (zombie-docs rule); one clean-room doc walkthrough per supported platform row. | 1d |
| **D3. Agent-installer non-interactive docs** | Non-interactive use is documented for agent installers: explicit prefix, no prompts, stable exit codes, machine-readable report of what changed. Basis for the `agent-noninteractive` clean-room profile (`platform-matrix.md`); full agent-surface consolidation (JSON diagnostics, skills) stays faber Stage 9 — **staged, not implemented here**. | Agent-style doc walkthrough: explicit-prefix run with no prompts produces the documented exit codes; docs name the JSON surface only where Stage 9 has landed (else labeled staged). | 1d |

**Stage est total ≈ 13.5 person-days (pilot); faber-side ≈ 12.5d + 1d routed-doc verification.**

---

## write_scope (implementation units, for later dispatch — NOT this planning commit)

This delivery is a **planning artifact**: it writes only this spec. The units above will write (in later
dispatch, under their own delivery):

- **faber**: `scripta/install-faber` (A1); `faber/src/commands/` `self` update/uninstall surface + tests
  (A2–A5); installer receipt + prefix-record format consumed by Stage 4 doctor (A4); decision rows in
  `decision-ledger.md` / `install-channel-matrix.md` (A6, R2); CLI help text for the lifecycle commands (D2).
- **routed (sibling, never written by faber)**: `faberlang.dev` install/lifecycle pages (D1–D3, via
  faberlang.dev Stage 8); release publish/provenance/checksums (R1, R3 — `component-release-streamline`);
  Homebrew formula sync (R2).

Read-only references (cite, never modify): `install-channel-matrix.md`, `dev-kit-contract.md`,
`package-and-lock-contract.md`, `platform-matrix.md`, `decision-ledger.md`, `delivery-stage2.md`,
`faber/docs/release/policy.md`, `faber/docs/release/release-manifest-schema.md` §3/§4/§6,
`faber/docs/factory/component-release-streamline/{CAMPAIGN.md,delivery-stage1.md,delivery-stage3.md}`,
`faber/docs/factory/release-and-portable-default/delivery.md`, `faberlang.dev` start track (site), `cista`
(store contract — read-only).

Forbidden roots for implementation units: `cista/`, `norma/`, `triga/`, `examples/`, `hosts/`,
`faber-runtime/`, `radix/`, `faberlang.dev/` — survey and cite only. No writes under `faber/docs/release/`
(streamline owns the schema + process docs). No edits to `faber/.github/workflows/` (Stage 10 / streamline
Stage 8). The untracked `dist/` tarballs in the faber working tree are Class-B foreign dirt — never touched.

---

## done_when

- **D1.** This delivery spec is committed as `delivery-stage3.md` under this campaign dir.
- **D2.** Entry gate recorded: Stage 2 delivered (campaign status line; `delivery-stage2.md` D1–D11 closeout
  evidence) — named above, not assumed.
- **D3.** Unit table complete: every unit has a non-blank done_when, validation, and pilot est; batches
  named **installer / release / docs** per the campaign's split-on-boundary batching.
- **D4.** Existing contracts referenced, not duplicated: `install-channel-matrix.md` (dispositions, prefix,
  checksum/provenance policy, lifecycle owners, formula-lag policy, OQ8), `dev-kit-contract.md` (layers,
  discovery), `package-and-lock-contract.md` (store/cache survival), `platform-matrix.md` (slice + profiles),
  `decision-ledger.md` (gate 2, OQ8, G7/G8/G12). No contract body is re-stated here.
- **D5.** Release-sibling dependency named: **faberlang.dev install pages** (routed, D-batch) and the
  **release process** (`component-release-streamline` publish/packaging + `release-and-portable-default`
  clean-room archive); overlap rule (bootstrap = fixed release payload; native macOS same logical layout +
  Gatekeeper before supported) recorded.
- **D6.** Stage 4's dependency on Stage 3 is recorded as **staged, not lowered** (see Staged later units):
  doctor's prefix/channel reporting, install-state machine, and repair paths consume Stage 3 outputs.
- **D7.** No product code; no writes outside this planning artifact; no `dist/` tarballs touched;
  `git diff --check` clean.

## validation

Planning artifact — docs-only, no cargo. At closeout, exactly **one** pass:

1. `git diff --check` and `git diff --cached --check` clean; staged set is exactly the single path
   `docs/factory/faber-onboarding/delivery-stage3.md` before and after commit.
2. Internal proof: every unit row cites the contract section it enforces (not prose re-derivation); the
   entry gate names Stage 2 delivered with its closeout evidence; Stage 4 is marked staged; the
   release-sibling rows mark sibling ownership; spec is date-stamped and cites base `39a5052`.
3. Note: the generated factory README's faber-onboarding doc-count will lag by one until the next docs
   commit regenerates it — recorded as known drift, not fixed in this path-limited commit.

No ladder runs, no `release-gate`, no cargo, no site build.

## forbids

- No product code in any repo; this is a planning artifact.
- No release execution: no tags, no pushes, no `gh release`, no asset mutation, no formula push, no cargo
  builds, no `release-gate`, no ladder runs.
- No writes outside `faber/docs/factory/faber-onboarding/` in this commit; no `faber/docs/release/` writes
  (streamline owns the schema + release process docs).
- No `cista/`, `norma/`, `triga/`, `examples/`, `hosts/`, `faber-runtime/`, `radix/`, `faberlang.dev/`
  writes — survey and cite only.
- No `faber/.github/workflows/` edits (Stage 10 / streamline Stage 8), no `build.rs` /
  `core-support-manifest.txt` edits.
- No Stage 4 doctor implementation (staged), no Stage 9 agent-surface consolidation (staged), no
  installer/lifecycle implementation in this planning commit.
- No curl|sh marketing claim unless Stage 3 installs and verifies the canonical Stage 2 payload
  (CAMPAIGN §Dependency rules).
- No touching the untracked `dist/` tarballs (Class-B foreign dirt).
- No verification loops: one closeout pass, then done.

## risks

- **Release-sibling readiness (primary):** the primary channel installs a **published** artifact; if
  `component-release-streamline`'s wrap+publish has not landed when installer units start, units must hold
  or install from the published archive-equivalent (Stage 2 staging dir) **with the residual labeled** —
  never a fake "published channel" claim. Interlock order: R1/D1 depend on the release sibling, not on faber.
- **curl|sh marketing pressure:** the gate is fixed (verify the canonical payload first); a docs/CLI split
  is not an acceptable workaround.
- **Uninstall safety regression:** user projects and dependency caches survive by default — proven by
  negative-then-positive clean-room proof (A5), never asserted from a dev-tree run.
- **Repair scope creep:** repair *policy* + failure classification is Stage 3; the full doctor surface is
  Stage 4. If a unit finds itself building diagnostics beyond the install state machine, it stops and files
  the Stage 4 dependency.
- **macOS-native scope creep (OQ8):** A6 is a **decision**, not an implementation; native packaging must not
  fork the payload layout or library model (`dev-kit-contract.md` layers; `install-channel-matrix.md` OQ8).
- **Formula drift (G7):** a stale formula must be labeled, never silently presented as current; site wording
  sync is routed to faberlang.dev Stage 8.
- **Stage 2 entry-gate regression:** if `verify-dev-kit` or the manifest instance is invalidated mid-stage,
  units hold and the gap routes to faber Stage 2 follow-up — no silent workaround.
- **Foreign WIP:** untracked `dist/` tarballs in the faber tree — untouched; base `39a5052` cited for the
  diff.
- **Site/CLI drift since 2026-08-07 evidence** — all contract citations date-stamped; re-verify claims that
  would change an install decision.

## Staged later units (dependency recorded, NOT lowered)

| Future stage | Unit title (future) | Dependency on Stage 3 (recorded) |
| --- | --- | --- |
| **Stage 4** | `faber-onboarding-stage4-doctor` | **Depends on Stage 3**: doctor reports prefix/channel from the Stage 3 install receipt; consumes the Stage 3 install-state machine + failure classes (A4); repair steps name the Stage 3 re-install/update path. **Staged — not lowered here.** |
| Stage 5 | `faber-onboarding-stage5-first-hour` | Consumes the installed kit from Stage 3; execution-default decision interlocked with `release-and-portable-default`. |
| Stage 6 | `faber-onboarding-stage6-norma` | Store/cache survival semantics from `package-and-lock-contract.md` (installed by Stage 3 channel). |
| Stage 7 | `faber-onboarding-stage7-triga` | Same store/cache + verified-bootstrap basis. |
| Stage 8 | `faber-onboarding-stage8-locales` | Depends on Stages 3–7 as their text stabilizes (CAMPAIGN Stage 8); faberlang.dev needs routed separately. |
| Stage 9 | `faber-onboarding-stage9-agent-surfaces` | Agent non-interactive basis lands in Stage 3 (D3); consolidation (JSON, skills) is Stage 9. |
| Stage 10 | `faber-onboarding-stage10-honesty` | Wires recurring clean-room CI over the Stage 3 channel + Stage 2 `verify-dev-kit`. |

Routed needs recorded (not implemented): **faberlang.dev** — install pages name exact artifact/version under
test (D1), lifecycle pages (D2), agent-installer docs (D3), formula-lag site wording (R2), all via
faberlang.dev Stage 8 (decision-ledger G3/G6/G7). **`component-release-streamline`** — per-triple
wrap+publish, archive naming + basename-only checksum (`release-contract.md` §5.1), provenance/checksum
assets, signing identity leg (R1/R3/A6). **`release-and-portable-default`** — clean-room release archive +
no-rust proof; this stage installs the archive, it does not prove portability.

---

## Suggested closeout evidence

Planning artifact committed (`delivery-stage3.md` only, path-limited); unit table + batches named in the
unit reply; entry gate (Stage 2 delivered) cited; Stage 4 dependency recorded as staged; `git diff --check`
clean; README count drift noted for the next docs commit; unit reply cites D1–D7 by letter.
