# Delivery — Faber Format Pretty Policy (`faber-format-pretty`)

**Status**: done — S2–S7 delivered; S8 global default flip **cancelled** (operator 2026-08-11); product default stays `normalise-v1`; `pretty-v1` opt-in only
**Goal**: [faber-format-pretty](goal.md) (committed as faber `69f2181`; status line is in `goal.md`, owned by Mind)
**Planner**: planner-2
**Created**: 2026-08-09
**Control plane**: `/Users/ianzepp/work/faberlang/faber` (goal inventory lives here)
**Owners**: `faber` (product surface: CLI, manifest, corpus, docs) + `radix` (`radix::forma` engine: policy selection, pretty-v1 rules, rule-slug registry)
**Source of truth**: `goal.md` §Rule set, §Flags and options surface, §Rollout safety, §Complexity guardrails, §Scope boundaries. This spec pins the delivery contract; it does not reopen goal decisions.

---

## 1. Planning stages

| Stage | Artifact | State |
| --- | --- | --- |
| P1 — goal-forge | `goal.md` (heads synthesis, rule set, rollout, guardrails) | done (2026-08-09) |
| P2 — goal-check | §2 below (READY verdict) | done (this doc) |
| P3 — delivery lowering | this doc: stage graph, per-stage contracts, checkpoints, evidence gates, open-decision resolutions | done (this doc) |

Hands implement from this delivery; they do not invent factory/delivery for an
unlowered stage (fleet lowering law). No product code exists yet.

---

## 2. Goal-check — READY verdict

**Verdict: READY.** The goal is clear-scoped, has a defined acceptance surface
(draft in `goal.md`, refined into per-stage evidence gates here), names owners
(faber vs radix), and has no blockers on the critical path.

| Check | Status | Note |
| --- | --- | --- |
| Scope is unambiguous | ✅ | §Scope boundaries splits faber vs radix work; §Complexity guardrails delimit the reject list |
| Acceptance is defined | ✅ | `goal.md` draft acceptance refined into per-stage done_when + evidence gates below |
| Open decisions resolvable at lowering | ✅ | All three resolved here (§3) with rationale |
| Engine-side foundation is known | ✅ | `radix::forma` `FormatOptions { canonical }`, `forma/author/{emit,layout,trivia}.rs`, `forma/compile.rs` read and grounded |
| CLI-side foundation is known | ✅ | `faber/src/commands/format.rs` (`resolve_format_paths`, `format_session`, locale chain, `--check`/`--stdout`/`--locale`, `--config` stub) read and grounded |
| No lane collision on the first unit | ✅ | First unit (S1) is radix-owned; faber `src/` has hand-6 WIP, so faber-side commits start at S2 (see §5 S2 risk) |
| Byte-identity contract is enforceable | ✅ | HARD CONTRACT (normalise-v1 byte-identical) has a concrete gate: live before/after diff at S1 + durable `.expected` pinning at S2 |

Non-blocking caveats (resolved by this doc, not by new decisions):
- The `--stdout` surface tightens from "many files with `=== path ===` separators"
  to "exactly one file" (§5 S4). This is the goal's stated flag surface, flagged
  as a deliberate behavior change, not a blocker.
- Where exactly formatted output is pinned byte-exactly across the corpus today
  is not fully inventoried (no format-specific `.expected` files exist yet — the
  corpus `.expected` files are runnable-output fixtures). S2 therefore opens with
  a short format-expectation survey; the rebaseline scope (S7) is derived from
  it. Recorded as S2/S7 risk, not a blocker.

---

## 3. Open-decision resolutions

### (a) `--policy <slug>` vs `--pretty` alias — RECOMMEND `--policy <slug>` (one switch, no alias)

**Resolution**: `faber format --policy <slug>`; no `--pretty` alias in v1.

Rationale:
- **One vocabulary.** `--policy` mirrors the manifest key `[format] policy = "pretty-v1"`
  exactly — CLI and manifest share the slug vocabulary with no translation layer.
  A `--pretty` alias would be a second spelling of the same concept and would
  need help-text, tests, and precedence rules of its own.
- **Future-proof.** After the default flip, `--policy normalise-v1` is the
  legacy override and any third policy needs no new flag. `--pretty` hardcodes
  one policy and becomes a lie after the flip.
- **Guardrail-consistent.** The goal's guardrail is "one switch, not a matrix";
  `--policy` is one switch parameterized by slug. An alias is ergonomics, not
  scope — if needed post-v1, a deprecated alias is a trivial additive change.
- **Migration served.** The flag's dual role (migration switch + steady-state
  override) is exactly what a parameterized policy selector provides.

Contract: `--policy <slug>` must validate the slug against the radix rule-slug
registry; unknown slugs fail clearly (nonzero exit, error message distinct from
formatting differences). Help text: `--policy <slug>  format with a named policy (normalise-v1 | pretty-v1)`.

### (b) line-width / indent-width as manifest keys — RECOMMEND v1 = implementation constants only

**Resolution**: v1 exposes **no** width/indent keys. Indent = 4 spaces,
soft width = 100 columns, as **implementation constants** in `radix::forma`
(pinned at corpus admission; see §7 corpus pinning).

Future minor (documented here so it is never "discovered" later):
- **Trigger**: real-world demand — a genuine package hitting the 100-col soft
  width badly, observed in the advisory `--check` inventory or package feedback.
  Not speculative "users may want it."
- **Shape**: a single bounded `[format.pretty]` block with `line_width` and
  `indent_width` keys, range-validated (e.g. width 40–200, indent 2–8), **stable
  per package** (never per-file — per-file widths are on the guardrail reject
  list), and introduced together with a **policy-identity bump** (`pretty-v2`
  slug or an explicit policy-version key) so the corpus pin stays honest — the
  corpus is pinned to a policy *identity*, and a width is part of that identity.
- **v1 reject**: no `--line-width` / `--indent-width` flags, no per-file width,
  no tabs.

Rationale: widths are the single highest-churn knob in formatter ecosystems
(the goal names config sprawl and environment-dependence as reject-list items);
exposing them before a policy-identity mechanism exists would silently drift the
byte-exact corpus. Constants keep v1 deterministic and corpus-pinable.

### (c) Placement of the corpus rebaseline commit — RECOMMEND one dedicated mechanical stage (S7), after corpus admission, before the default flip

**Resolution**: the rebaseline is **stage S7**, a dedicated mechanical commit
(or, where drift spans sibling repos, one mechanical commit **per repo** in a
coordinated wave — fleet one-lane-per-repo rule). Its position in the graph is
mandatory:

```
golden-corpus admission (S6, reviewed diff) → rebaseline (S7, mechanical) → default flip (S8)
```

- **After admission**: the S6 pretty-v1 corpus expectations (reviewed, committed)
  are the reference the rebaseline writes against — you cannot rebaseline before
  the new profile is admitted and reviewed.
- **Before the flip**: the flip (S8) must never land against an unpinned profile;
  the mechanical commit is what makes `--check` green under pretty-v1.
- **Never mixed with semantic changes**: S7's done_when forbids bundling any
  semantic fixture edit, rustfmt/clippy cleanup, or unrelated fix. If the S6
  advisory inventory surfaces a behavior-flag file, that file is **excluded**
  from the mechanical commit and routed to a separate investigation (never
  silently rebaselined).
- **Per-repo discipline**: the wave updates format expectations in each repo
  that has them (faber-owned format fixtures; radix corpus sources only if the
  S2 survey finds format expectations there), one commit per repo, each
  mechanical-only. See S7 for the full scope rule (what is *not* auto-reformatted).

---

## 4. Implementation stage graph

```
S1 radix: policy selection + normalise-v1 promotion (byte-identity contract)
   │
   ├──→ S2 faber: formatter golden corpus + normalise-v1 baseline expectations
   │         (parallelizable with S3 — different repos, two lanes)
   │
   └──→ S3 radix: pretty-v1 layout engine + rule-slug registry (normalise-v1
   │         byte-identity re-gated)
   │
   └──── S4 faber: CLI surface (--write, --stdin, --policy, combo constraints)
   │
   └──────── S5 faber: [format] manifest schema + precedence
   │
   └──────────── S6 faber: pretty-v1 corpus admission + advisory --check (diff inventory)
   │
   └──────────────── S7 faber (+ per-repo lanes): exempla rebaseline — one mechanical commit per repo
   │
   └──────────────────── S8 faber: default flip to pretty-v1 + product docs + skill currency
```

Dependencies (edges): S1 → {S2, S3}; S2 ∧ S3 → S4; S4 → S5; S3 ∧ S5 → S6;
S6 → S7; S7 → S8. Parallel lanes: S2 (faber) ∥ S3 (radix). All other stages are
serial, one lane per repo, small units.

| Stage | Owner | Repo | Unit(s) | Gate |
| --- | --- | --- | --- | --- |
| S1 policy selection + normalise-v1 promotion | radix | radix | 1 | byte-identity empty diff |
| S2 formatter golden corpus + normalise baseline | faber | faber | 1–2 | corpus harness green in default profile |
| S3 pretty-v1 layout engine | radix | radix | 4 (serial) | engine idempotent; normalise-v1 re-gated |
| S4 CLI surface (`--write`/`--stdin`/`--policy`/combos) | faber | faber | 1–2 | flag + combo tests green |
| S5 `[format]` manifest schema + precedence | faber | faber | 1 | precedence matrix tests green |
| S6 corpus admission + advisory `--check` | faber | faber | 1 + review | admission diff reviewed; diff inventory recorded |
| S7 exempla rebaseline (mechanical) | faber + per-repo lanes | faber (+ radix/norma as survey shows) | 1 | mechanical-only; `--check` green |
| S8 default flip + docs | faber | faber | 1 | default pretty-v1; docs + skill current |

Checkpoints (review points, not stages): **C1** after S1 (byte-identity proven),
**C2** after S3 (engine stable, contract intact), **C3** after S5 (full policy
resolution reachable from the CLI), **C4** after S7 (rebaseline clean, zero
semantic drift), **C5** after S8 (flip live, docs current). Each checkpoint is
the evidence gate of its stage plus a Mind review of the recorded evidence.

---

## 5. Stages

### S1 — radix::forma policy selection + normalise-v1 promotion (radix-owned)

**Entry**: no conflicting WIP in `radix/crates/radix/src/forma/`; faber CLI
surface frozen as-is for this stage.

**Outcome**: `radix::forma` gains a policy concept and a rule-slug registry
whose default is `normalise-v1`, byte-identical to today's author-mode output.

- `FormatPolicy` (or equivalent) in `forma/options.rs` alongside the existing
  `FormatOptions { canonical }`. The `canonical` knob stays an internal
  implementation detail — not product vocabulary.
- Rule-slug registry: `normalise-v1` (default), `pretty-v1` (reserved, not yet
  implemented). Unknown slugs fail clearly (engine-level error, never silent
  fallback). Registry lives in radix so faber's `--policy` and the manifest
  schema share the same source of truth.
- `normalise-v1` selects the exact current author emit path (`forma/author/`).
  Any refactor that makes the policy explicit must not touch output behavior.

**HARD CONTRACT**: `normalise-v1` output is byte-identical to today's
`compile_author` output.

**Units**: `faber-format-pretty-s1-policy-selection` (one unit: policy type,
registry, default wiring, byte-identity harness).

**done_when**:
- `FormatPolicy`/registry present; `normalise-v1` is the default.
- Unknown-slug failure is a clear error.
- Byte-identity: author-format every fixture under `radix/corpus/` and
  `faber/corpus/` with the pre-change and post-change binaries; the diff is
  **empty** (live before/after at closeout — the durable pin is S2's
  `.normalise-v1.expected` files).
- Existing `forma` tests and `faber/src/commands/format_test.rs`-style identity
  tests remain green.

**Evidence gate** (narrow, once at closeout): `cargo check -p radix` + the
touched-crate forma test target from `radix/`; the recorded byte-identity diff
(empty). No wider suites.

**Forbids**: no output behavior change; no `pretty-v1` engine work yet; no
faber-side changes.

---

### S2 — formatter golden corpus + normalise-v1 baseline (faber-owned)

**Entry**: S1 done (normalise-v1 pinned as the explicit baseline policy).

**Outcome**: a formatter-specific corpus whose `normalise-v1` expectations pin
today's behavior byte-exactly, plus a cheap harness that runs in the faber
default test profile.

- **Format-expectation survey (stage opens with this)**: inventory everywhere
  formatted source is currently pinned byte-exactly — `src/commands/format_test.rs`,
  `radix::forma::test_gate`, any corpus snapshot files. Today the corpus
  `.expected` files are runnable-output fixtures, not format expectations; the
  survey confirms that and records any stragglers. Output of the survey feeds
  S7's rebaseline scope.
- **Corpus home** (default): `faber/corpus/format/` — fixture sources `*.fab`
  plus `*.normalise-v1.expected` and (later) `*.pretty-v1.expected`. Harness as
  a new faber lib test (`src/commands/format_corpus_test.rs`), running under the
  cheap default nextest profile (`./scripta/test`), **no new dependencies**
  (avoids touching Cargo.lock, which has hand-6 WIP).
- **Coverage** (per goal): blocks; compact `ergo` arms; `si`/`sin`/`secus`
  chains in the **non-cuddled** form (operator style decision 2026-08-09);
  `fac { … } cape err { … }` handler blocks in the **non-cuddled** form; calls;
  records; comments; blank lines; frontmatter; locale output (en/la — cfg-gated
  on the `hir-faber` feature so narrow builds stay cheap); long unbreakable
  lines; idempotence (`format(format(x)) == format(x)`).
- Admission rule: fixtures enter with a **normalise-v1** `.expected`
  (byte-identical to current behavior). pretty-v1 expectations are admitted in
  S6 after review.

**Units**: `faber-format-pretty-s2-golden-corpus` (survey + corpus + harness;
split into two units if the survey surfaces surprises).

**done_when**: corpus committed; `normalise-v1` expectations byte-exact;
harness green in the default profile; idempotence tests present.

**Evidence gate**: `./scripta/test` (default stage 1) from `faber/`, once.

**Risks**: hand-6 WIP in `faber/src/` — the harness is a new file and the corpus
is a new directory, so collision risk is minimal, but the commit must be
path-limited and staged after hand-6's state is known (Mind coordinates).

---

### S3 — pretty-v1 layout engine (radix-owned)

**Entry**: S1 done (normalise-v1 pinned); S2 may be in flight (different repo).

**Outcome**: the `pretty-v1` layout rules implemented in a new `forma/pretty/`
module, selected via the policy registry, with `normalise-v1` unchanged.

Rule delivery (goal §Rule set; units below are serial on the radix lane):
1. **Blocks & indentation**: one statement per line in a block; 4-space indent
   (constant); braces attached; `si … sin … secus` chains render as one
   readable chain with **non-cuddled branch heads** (operator style decision
   2026-08-09): the closing brace of a branch sits on its own line and `sin` /
   `secus` begin on a fresh line — never `} sin … {`. The same non-cuddled rule
   applies to every brace-attached construct: `cape` (`catchClause := 'cape'
   IDENTIFIER blockStmt`, attaching to conditional arms, `dum`, `itera`,
   `elige`, `cura`, and `fac`) always starts on its own line after the closing
   brace — never `} cape … {`. Compact one-statement arms stay compact when
   they fit, expand to a block when the arm exceeds the soft width; empty
   blocks stay compact. (Grammar authority: radix/EBNF.md + compiler source;
   archived goldens are historical, not authoritative.)
2. **Width-aware wrapping**: 100-col soft limit (constant, a trigger not a
   promise); break only at syntactic boundaries (params, args, record fields,
   already-safe grouping); never split strings, URLs, identifiers, or operators
   in precedence-obscuring ways; unbreakable long lines stay long.
3. **Calls, parameters, records**: fit if possible, expand when needed; wrapped
   lists one per line; nested records one field per line; no trailing commas.
4. **Blank lines**: one blank line between top-level declarations; collapse
   runs; preserve author blanks between sections inside bodies; never
   manufacture blanks between statements.
5. **Comments & frontmatter**: re-indent comments with syntax; normalize
   comment-marker spacing only; preserve comment text/line breaks byte-for-byte;
   no prose reflow; `+++` frontmatter preserved as frontmatter, never formatted
   with Faber layout rules.
6. **Binding/operator spacing (P1)**: normalize spacing around `←`, `→`, `=`,
   commas, delimiters; **no vertical alignment anywhere**.
7. **Line-sensitive grammar guard (record risk)**: if a construct cannot be
   preserved confidently (compact `ergo` bodies, annotation sugar), leave the
   file unchanged, report the path/location, and fail in `--check` mode. Engine
   design note: implement conservatively — constructs not explicitly handled by
   pretty rules are emitted in preserved form, never flattened "because it fits".
   A partial rewrite is worse than an unformatted file.

Constants: indent = 4, soft width = 100 — implementation constants only
(decision (b)). Width/indent are never configurable in v1.

**HARD CONTRACT re-gate**: after the engine lands, `normalise-v1` output is
still byte-identical (S1 harness re-run; must stay empty).

Locale/HIR path: see §7 design note — the layout policy must apply to both the
author (AST+trivia) path and the `--locale` HIR re-emit path, sharing the same
layout module; locale changes keyword spelling, never wrapping/indentation.

**Units** (serial, radix lane): `…-s3-engine-blocks-indent` (items 1),
`…-s3-engine-wrapping` (items 2–3), `…-s3-engine-trivia-blanks` (items 4–5),
`…-s3-engine-spacing-guard` (items 6–7 + registry completion). Each unit lands
its own leaf-owned tests (radix test ownership law: leaf proofs live with the
leaf).

**done_when**: each rule class has leaf-owned tests; engine output is stable and
idempotent on the S2 fixture shapes (engine-local fixtures); normalise-v1
byte-identity gate green; unknown slugs fail clearly.

**Evidence gate**: `cargo check -p radix` + touched-crate forma tests from
`radix/`, once per unit; the byte-identity re-gate at unit 4 closeout.

---

### S4 — faber CLI surface: `--write`, `--stdin`, `--policy`, combos (faber-owned)

**Entry**: S3 done (engine selectable); S2 done (corpus available for
verification).

**Outcome**: the steady-state flag surface, with combo constraints enforced.

- `--write`: explicit spelling of the in-place default (scripts/editors).
  Functionally the default behavior; added for clarity, not a mode change.
- `--stdin`: read exactly one source document from stdin → stdout; implies
  stdout; incompatible with path arguments. Source name for diagnostics:
  `<stdin>`.
- `--stdout`: tightened to **exactly one file** (today's code prints
  `=== path ===` separators for multiple files — a deliberate behavior change
  per the goal's flag surface; update `cli_test`/`format_test` accordingly).
- `--policy <slug>`: CLI override (decision (a)); validates against the radix
  registry; unknown slug → clear error, exit nonzero, message distinct from
  formatting-difference output.
- Combo constraints: `--check` ⊥ `--stdout` / `--stdin`; `--write` ⊥
  `--stdout`; `--stdin` implies stdout and takes no paths. Parse/config
  failures are distinct from formatting differences.
- Policy resolution so far: built-in default (`normalise-v1`) < CLI `--policy`.
  Manifest precedence lands in S5.
- `--config` stays the deferred warning stub — unchanged.

**Units**: `faber-format-pretty-s4-cli-flags` (flag definitions, combo
constraints, cli_test coverage), optionally `…-s4-policy-wiring` (thread slug →
`FormatPolicy` through `cmd_format`).

**done_when**: all new flags and constraints covered in `cli_test.rs` +
`format_test.rs`; unknown slug fails clearly; `--stdin` → stdout roundtrip
works; `--stdout` single-file tightening covered; existing behavior otherwise
unchanged.

**Evidence gate**: `./scripta/test` (default stage 1) from `faber/`, once.

---

### S5 — `[format]` manifest schema + precedence (faber-owned)

**Entry**: S4 done.

**Outcome**: `faber.toml` `[format] policy` selection with the full precedence
chain; no second config language.

- Schema: `[format] policy = "pretty-v1" | "normalise-v1"`; unknown slug → clear
  error at parse/validation time.
- Precedence: built-in defaults < package `[format]` < CLI `--policy`.
- Discovery: `faber format` (no path) reads the package manifest at cwd; explicit
  paths resolve the containing package of each root, falling back to built-in
  defaults when no manifest is discoverable (e.g. `--stdin`, or a file outside
  any package). Default for multi-root invocations spanning packages: per-root
  resolution. Record the choice in the CLI docs.
- Explicitly **not** in v1: `[forma]`, `forma.toml`, global dotfile config,
  config inheritance, per-file rule lists. A frontmatter policy selector is only
  added if migration proves a bounded escape hatch is necessary (goal §Flags).
- Locale precedence unchanged (CLI → frontmatter → package `[reader] locale` →
  en) and layout-independent.

**Units**: `faber-format-pretty-s5-manifest-policy`.

**done_when**: precedence matrix tested (defaults < package < CLI); unknown slug
tested; locale precedence regression-covered; `--config` still deferred.

**Evidence gate**: `./scripta/test` (default stage 1) from `faber/`, once.

---

### S6 — pretty-v1 corpus admission + advisory `--check` (faber-owned, radix assist)

**Entry**: S3 + S5 done (engine selectable, full policy resolution reachable).

**Outcome**: pretty-v1 expectations admitted for the S2 corpus, and an advisory
diff inventory recorded — the evidence base for the S7 rebaseline and the
migration-window decision.

- **Corpus admission**: generate `*.pretty-v1.expected` for the S2 fixtures via
  `faber format --policy pretty-v1`; the diff is **reviewed** (Mind/head review,
  not blind write) and committed as the admission commit. Never admit a snapshot
  that was not reviewed.
- **Idempotence**: full-corpus `format(format(x)) == format(x)` re-run under
  pretty-v1.
- **Advisory CI**: `faber format --policy pretty-v1 --check` against
  representative packages and the corpus (goal §Rollout safety step 4). Record a
  diff inventory: which files drift, how many lines, and a per-file
  classification **formatting-only** vs **behavior-flag**. Behavior-flag files
  are routed for investigation; formatting-only files are the S7 rebaseline
  scope. This inventory is the evidence input for the operator's
  migration-window and default-flip decision (S8).
- Note: the drift scope here is the *corpus / language fixtures*. Triga and
  stdlib exempla *source* reformatting is a per-package adoption decision during
  the migration window (opt-in via `--policy`), **not** forced by the rebaseline
  (see S7).

**Units**: `faber-format-pretty-s6-admission-advisory` (+ a review step owned by
Mind/head).

**done_when**: admission diff committed after review; idempotence green; diff
inventory recorded with classification; no open behavior-flag blocking.

**Evidence gate**: `./scripta/test` (default stage 1) once + the recorded,
reviewed admission diff.

---

### S7 — exempla rebaseline: one dedicated mechanical commit (faber-owned, per-repo lanes)

**Entry**: S6 admission committed; advisory inventory classified; no open
behavior-flags in the rebaseline scope.

**Outcome**: corpus format expectations (and faber-owned `.fab` fixtures that
the harness pins) rebaselined to pretty-v1 — **one dedicated mechanical commit
per repo**, nothing else in them (decision (c)).

**Scope rule**: only files that the S2 survey identified as format expectations,
and faber-owned `.fab` fixtures that must be pretty-formatted for `--check` to
stay green. Reformatting *third-party/package* sources (norma/exempla,
triga/src) is **not** part of this commit — it is per-package adoption during
the migration window, chosen by package owners/operator, applied via
`--policy pretty-v1` on that package, and lands as its own (non-mechanical)
change if adopted. The one-commit rule protects the corpus, not other repos'
working sources.

**done_when** (hard):
- The mechanical commit(s) change only format-expectation files and pinned
  `.fab` fixtures; **zero semantic diff** (verified by diff review).
- No rustfmt/clippy cleanup, no unrelated fixes, no docs bundled.
- Behavior-flag files found during the change are excluded and routed
  separately — never silently rebaselined.
- `faber format --policy pretty-v1 --check` (and the default-profile harness)
  is green on the touched roots.
- Per-repo: one commit per repo that has rebaseline scope (faber + whatever the
  S2 survey found in radix/norma), each mechanical-only.

**Evidence gate**: the committed diff classified mechanical-only (reviewed);
`--check` green; no build suites run (this stage is a fixture change, validated
by the cheap harness only).

---

### S8 — default flip to pretty-v1 + product docs + skill currency (faber-owned)

**Status (operator 2026-08-11): CANCELLED as a global product flip.**

Product built-in default remains **`normalise-v1`**. That avoids a mass
reformat of every package that relies on the default. `pretty-v1` remains a
fully landed **named** policy for **on-demand** use when the operator
explicitly requests pretty layout for a specific area or package:

- CLI: `faber format --policy pretty-v1 …`
- Manifest: `[format] policy = "pretty-v1"`

**Do not merge** hand tip `c32b69b` (`factory/hand-3`, S8 default-flip
implementation) onto main unless a future operator request re-opens a scoped
flip. That commit is historical WIP only.

**Original planned entry** (superseded): S7 rebaselined; migration window
elapsed and operator approved global flip.

**Original planned outcome** (superseded): product default becomes
`pretty-v1`; docs/skill updated for the flip. S7 still owns the only
mechanical rebaseline boundary; no second broad rebaseline was planned in S8.

**Units**: `faber-format-pretty-s8-default-flip-docs` — **not** for dispatch
as a global flip. Future work is scoped opt-in adoption only, filed as new
units under operator request.

**done_when (revised)**: product default remains `normalise-v1` on main; S8
global flip not merged; disposition recorded here and in `goal.md` Status.

**Evidence gate**: main tip still resolves built-in default to
`normalise-v1` (no S8 flip on main).

---

## 6. Ownership summary

| Work item | Owner | Repo | Stages |
| --- | --- | --- | --- |
| Policy selection, `normalise-v1` promotion, rule-slug registry | radix | radix | S1, S3 |
| `pretty-v1` layout rules + line-sensitive guard | radix | radix | S3 |
| Golden corpus + harness + admission | faber | faber | S2, S6 |
| CLI (`--write`, `--stdin`, `--policy`, combo constraints) | faber | faber | S4 |
| `[format]` manifest schema + precedence | faber | faber | S5 |
| Advisory `--check`, diff inventory, migration-window evidence | faber | faber | S6 |
| Rebaseline (mechanical, per-repo) | faber (+ per-repo lanes) | faber + survey findings | S7 |
| Default flip, product docs, skill currency | faber | faber | S8 |

---

## 7. Cross-cutting contracts

- **HARD CONTRACT (radix)**: `normalise-v1` output is byte-identical to today's
  behavior — gated live at S1, re-gated at S3 closeout, and pinned durably by
  the S2 `.normalise-v1.expected` files.
- **Determinism**: output depends on source bytes + locale + policy only. No
  environment, filesystem-ordering, or host-tool behavior (goal §Flags).
- **Idempotence**: `format(format(x)) == format(x)` — does *not* require all
  semantically-equal sources to collapse to one byte sequence (goal §Idempotence).
- **Unknown slugs fail clearly** — registry-enforced in radix, surfaced by faber
  CLI and manifest validation as distinct errors.
- **Corpus pinning**: corpus expectations are keyed by policy slug; width/indent
  are part of the policy identity if ever exposed (decision (b) — see
  effective-policy identity below). `--check` means "matches the current
  profile".
- **Effective-policy identity (bounded follow-up, head-cto strategy fire-16)**:
  before any output-affecting `[format.pretty]` values are exposed, the corpus
  pin must key on more than the slug — a corpus keyed only by `pretty-v2`
  cannot honestly pin output when packages can choose different widths under
  the same slug. The effective identity must be either a versioned policy
  whose output-affecting options are fixed, or a tuple `(policy slug, canonical
  options fingerprint)`. Engine revision stays rebaseline provenance, never
  policy identity. `normalise-v1`/`pretty-v1` are immutable semantic
  identities, not names for whichever defaults are current.
- **Locale interplay (head-cto strategy fire-16)**: policy selection and locale
  conversion must not silently override one another — `--policy normalise-v1
  --locale en` must not quietly switch from the author normalizer to canonical
  HIR re-emission; if a combination cannot honor both contracts, reject it
  explicitly. The formatter's `--locale` flag is a dialect/keyword selection,
  not a lossless source transcode (see the Tela U0 lossless-transcode
  follow-up).
- **Locale/layout separation**: locale changes keyword spelling, never
  wrapping/indentation. Design note (residual to confirm at S3): the pretty
  layout rules must be shared by the author (AST+trivia) path and the `--locale`
  HIR re-emit path via one layout module; default direction is a shared layout
  module; if the HIR path cannot share it in v1, the gap is recorded and the
  author path remains the primary product surface.
- **Line-sensitive grammar guard**: unconfident constructs are left unchanged +
  reported + `--check` failure. Partial rewrites are worse than unformatted
  files (goal §Record risk).
- **One committing lane per repo; mechanical commits are pure** (S7/S8): path
 -limited staging, staged-path check before commit (fleet git law), no bundled
  cleanup.
- **Cargo discipline**: narrow checks only — `cargo check -p <crate>` and
  single touched-crate tests during work; `./scripta/test` (default stage 1,
  faber) or the radix equivalent once at each stage closeout. Never
  whole-workspace nextest, `--profile full`, stages 4–6, or e2e.

---

## 8. Checkpoints

| Checkpoint | After | Verifies |
| --- | --- | --- |
| C1 | S1 | normalise-v1 byte-identity proven (empty diff, recorded) |
| C2 | S3 | engine stable + idempotent; byte-identity contract intact; registry complete |
| C3 | S5 | full policy resolution reachable (defaults < manifest < CLI), unknown slugs fail |
| C4 | S7 | rebaseline mechanical-only, zero semantic drift, `--check` green |
| C5 | S8 | default flipped; docs + skill current; corpus pinned |

Mind owns checkpoint reviews; the Auditor owns full-verification runs at named
boundaries (never the implementing Hand).

---

## 9. Handoff

**Recommended first Hand unit**: `faber-format-pretty-s1-policy-selection`
(S1, radix lane).

**done_when** (for that unit): `FormatPolicy` + rule-slug registry land in
`radix::forma` with `normalise-v1` default; unknown slugs error; author-format
over `radix/corpus/` + `faber/corpus/` before-vs-after the change diffs empty;
`cargo check -p radix` + touched-crate forma tests green (narrow, once).

Subsequent units follow the graph: S2 (faber) may run in parallel with S3 (radix)
once S1 lands. Mind files each unit citing this delivery's stage contract.

---

## 10. Residuals / risks (out-of-scope observations — not work)

1. **README staleness on commit**: adding this `delivery.md` makes the generated
   `faber/docs/factory/README.md` stale. Mind regenerates
   (`radix/scripta/generate-factory-readme.py --factory-root …/faber/docs/factory`)
   and runs the goal-status audit when committing — planner-2 must not touch the
   README (foreign dirt).
2. **goal.md status**: currently `planned`. Recommend Mind sets it to `active`
   when the first Hand unit (S1) is filed.
3. **`--stdout` tightening**: today multiple files print `=== path ===`
   separators; v1 restricts to one file (goal's flag surface). Flagged so
   existing scripts/tests expecting separators are updated deliberately.
4. **Locale/HIR layout sharing** (§7): the pretty rules must compose with the
   `--locale` HIR re-emit path; shared-layout is the default, recorded at S3 if
   the HIR path diverges.
5. **Package-source reformatting is not the rebaseline**: norma/exempla and
   triga/src become pretty only by per-package adoption during the migration
   window — a separate, non-mechanical decision, not part of S7's commit.
6. **hand-6 WIP**: faber `src/` and `Cargo.lock` are hand-6's; S2 avoids new
   dependencies (no Cargo.lock touch) and all faber commits are path-limited.
   The docs/factory/README.md and browser-wasm-product/ dirt is untouched.
7. **faber-format-lossless interplay** (goal §Relationship): normaliser-based
   pretty-v1 satisfies most trivia-preservation concerns; lossless remains a
   follow-on only if string-delimiter/`forma`-literal fidelity proves necessary.
8. **Deterministic fixture drift**: if the corpus `.fab` sources drift during
   the migration window, the advisory inventory (S6) may need a re-run before
   S7; the C4 checkpoint catches it.
