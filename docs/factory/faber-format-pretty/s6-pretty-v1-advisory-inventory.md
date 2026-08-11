# FORMAT-PRETTY S6 — pretty-v1 advisory diff inventory

**Date**: 2026-08-11
**Unit**: `faber-format-pretty-s6-admission-advisory` (task 14b98a06)
**Goal**: [faber-format-pretty](goal.md) §Rollout safety step 4 — advisory
`faber format --policy pretty-v1 --check` against the format corpus and
representative packages; record which files drift, how many lines, and a
per-file classification **formatting-only** vs **behavior-flag**.
**Deliverable**: this inventory is the evidence input for the operator's
migration-window / default-flip decision (S8) and the S7 rebaseline scope.

## Method

- Binary: `faber 1.6.0` (built 2026-08-10 from local main; verified byte-
  identical to the packet engine: zero commits touching
  `crates/radix/src/forma/` after the build, and the faber format pipeline
  functions untouched by the one post-build faber commit `5df5e8c`, which only
  refactored policy *resolution*).
- Advisory command: `faber format --policy pretty-v1 --check <surface>` (never
  writes; exit 1 = drift found). Drift list and diagnostics on stderr.
- **Marginal pretty-v1 delta**: per file, `diff(pretty-v1 --stdout, normalise-v1
  --stdout)`. This isolates the pretty-v1 layout change from the pre-existing
  `normalise-v1` formatter behavior (keyword dialect normalization, trailing-
  newline handling) that is already today's `faber format` contract.
- Changed-lines counts: `diff <source> <pretty-v1 --stdout> | grep -c '^[<>]'`.

### Important context for reading the numbers

The drift reported by `--policy pretty-v1 --check` on English-keyword sources
includes the **pre-existing normalise keyword conversion** (e.g.
`import from … private` → `importa ex … privata`). Both policies convert
identically (marginal delta 0 for those files), so flipping the default to
pretty-v1 does **not** newly introduce keyword conversion — it is today's
`faber format` behavior. The *marginal* pretty-v1 delta is pure layout
(wrapping, expansion, blank-line collapse, whitespace normalization).

## Surface 1 — format corpus (`faber/corpus/format/`, 9 fixtures)

| File | Drift | Changed lines | Marginal pretty delta | Class |
| --- | --- | --- | --- | --- |
| `blocks.fab` | yes | 6 | 0 (normalise already non-cuddles) | formatting-only |
| `frontmatter.fab` | yes | 1 | 0 (blank after `+++` removed; same as normalise) | formatting-only |
| `long-lines.fab` | yes | 11 | 11 (signature wrap, one param per line) | formatting-only |
| `compact-ergo.fab` | no | — | — | formatting-only (drifts under normalise's double-space quirk; pretty-v1 **fixes** it, source already matches) |
| `calls.fab`, `comments.fab`, `fac-cape.fab`, `locale.fab`, `records.fab` | no | — | — | clean |

Summary: **3 drift, 0 errors**. All three drifting sources are the S7
rebaseline scope for the corpus (faber-owned `.fab` fixtures pinned by the
harness). The admitted `*.pretty-v1.expected` snapshots (9 files) are reviewed;
see the admission commit.

## Surface 2 — stdlib exempla (`norma/exempla/`, 24 files)

- **19 drift** (62 changed lines vs source), **4 clean**, **1 behavior-flag**.

Drifting files (changed lines vs source):

| File | Changed lines | Marginal pretty delta | Class |
| --- | --- | --- | --- |
| `chorda/angustat.fab` | 2 | 0 | formatting-only |
| `chorda/discidit.fab` | 2 | 0 | formatting-only |
| `chorda/mechanica.fab` | 1 | 0 | formatting-only |
| `chorda/retine.fab` | 10 | 10 (arg wrap) | formatting-only |
| `stdlib-nativum/aleator.fab` | 3 | 0 | formatting-only |
| `stdlib-nativum/caelum-terminus.fab` | 2 | 5 (record expansion) | formatting-only |
| `stdlib-nativum/chorda.fab` | 2 | 0 (blank-line collapse) | formatting-only |
| `stdlib-nativum/codex.fab` | 1 | 0 | formatting-only |
| `stdlib-nativum/consolum.fab` | 1 | 0 | formatting-only |
| `stdlib-nativum/csv-chorda.fab` | 2 | 0 | formatting-only |
| `stdlib-nativum/mathesis-operators.fab` | 2 | 0 | formatting-only |
| `stdlib-nativum/mathesis.fab` | 2 | 0 | formatting-only |
| `stdlib-nativum/retorta.fab` | 2 | 0 | formatting-only |
| `stdlib-nativum/solum-explora-contract.fab` | 1 | 0 | formatting-only |
| `stdlib-nativum/solum.fab` | 1 | 0 | formatting-only |
| `stdlib-nativum/tempus-civil.fab` | 14 | 0 | formatting-only |
| `stdlib-nativum/tensor-applicata.fab` | 2 | 0 | formatting-only |
| `stdlib-nativum/tensor-bridge.fab` | 10 | 9 (list wrap) | formatting-only |
| `stdlib-nativum/vector-pending-placeholder.fab` | 2 | 0 | formatting-only |

Clean: `ad-multiplica-backward/src/main.fab`, `chorda/diducta.fab`,
`stdlib-nativum/toml-exige-claves.fab`, `stdlib-nativum/toml-navigatio.fab`.

**Behavior-flag (1):**
- `crypta-sha2/src/main.fab` — the line-sensitive grammar guard fired:
  `error: … pretty-v1 cannot preserve this construct confidently; file left
  unchanged`. The file uses `incipit` blocks, byte literals (`|61 62 63|`),
  and underscore-prefixed function names. **Excluded from any rebaseline;
  routed for investigation** (radix S3 guard coverage or an explicit pretty
  rule for the construct).

## Surface 3 — triga library (`triga/src/`, 26 files)

- **26 drift** (6636 changed lines vs source), **0 errors**.

Marginal pretty delta nonzero in **11 files** (pure layout — record/call
expansion, wrapped signatures, wrapped lists):

`face` (49), `attribute` (27), `batch` (19), `data` (333), `camera` (23),
`base` (29), `math` (287), `material/basic` (736), `resource` (112),
`scene` (650), `shader_contract` (40).

The remaining **15 files** drift with marginal delta 0: the drift is the
pre-existing normalise keyword conversion (`import from … private` →
`importa ex … privata`) — e.g. `lighting.fab`, `geometry.fab`, `bounds.fab`,
`layout.fab`, `graph.fab`, `object.fab`, `light.fab`, `material.fab`,
`primitives/basic.fab`, `lit.fab`, `standard.fab`, `primitives.fab`,
`renderable.fab`, `mesh.fab`, `triga.fab`. All formatting-only.

Note: triga's package manifest declares `[reader] locale = "en"`, but the
author emit path still converts keywords to the Latin default (verified:
default `faber format` output converts; only explicit `--locale en` preserves
English keywords). This is existing normalise behavior, not a pretty-v1 change.

## Overall classification

| Class | Count | Scope |
| --- | --- | --- |
| **Formatting-only** | 48 drifting files (3 corpus + 19 exempla + 26 triga) | corpus sources → S7 rebaseline; exempla/triga sources → per-package adoption during the migration window |
| **Behavior-flag** | 1 (`norma/exempla/crypta-sha2/src/main.fab`) | routed for investigation; never silently rebaselined |

Idempotence: full-corpus `format(format(x)) == format(x)` verified under
pretty-v1 for all 9 corpus fixtures (and pinned durably by the
`pretty_v1_corpus_is_byte_exact_and_idempotent` harness test).

## S7 implications

- S7 rebaseline scope: the corpus `*.pretty-v1.expected` snapshots (admitted,
  reviewed) plus the 3 corpus fixture sources that drift (`blocks.fab`,
  `frontmatter.fab`, `long-lines.fab`).
- norma/exempla and triga/src sources are **not** S7 scope — their adoption is
  a per-package decision during the migration window (delivery §S7, goal
  §Rollout safety step 5). The numbers above are the migration-window evidence.
- The behavior-flag file is excluded from any mechanical commit.

## Residuals

1. **`crypta-sha2` guard coverage** (radix S3 follow-up): the guard is doing
   its job, but the exact construct that cannot be preserved confidently is
   not yet identified; the file should be investigated so the guard either
   gains coverage or the report names the construct.
2. **Keyword normalization in the advisory drift** is today's normalise
   behavior (both policies identical). If the migration decision wants
   English-keyword packages to stay English under pretty-v1, that is a
   separate product decision (`--locale en` / reader-locale emit), not part of
   this inventory.
3. **`long-lines` body parenthesization** (the `+` chain becomes fully
   parenthesized) is pre-existing normalise behavior, identical under both
   policies — not a pretty-v1 change.
