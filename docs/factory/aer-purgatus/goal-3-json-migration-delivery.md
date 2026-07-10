# Delivery: First-Party JSON Migration And FVI Adoption

**Status**: in progress — M1 inventory complete, M2 selected
**Date**: 2026-07-09
**Campaign**: [`CAMPAIGN.md`](CAMPAIGN.md)
**Primary repo**: `/Users/ianzepp/work/faberlang/examples`
**Supporting repos**: `norma` for direct first-party callers; `radix` for the
corpus harness only
**Factory checkpoint**: every selected first-party Faber JSON boundary parses
or constructs a formal `json` value once, then performs typed schema work.

## Interpreted Unit

Migrate first-party Faber application code from hand-built JSON strings and
partial text scanners to the formal object-rooted `json` type delivered by Goal
2. Use the Faber Vector Interchange (FVI) pipeline as the strongest integration
proof because it crosses command stdout, persisted files, subprocess
boundaries, vector arrays, diagnostics, and query results.

This delivery includes all live first-party manual JSON paths discovered during
planning, not only the five files named in the original campaign intake:

- `ai-workbench/.../commands/chat.fab`
- `ai-workbench/.../commands/embed.fab`
- `ai-workbench/.../commands/generate.fab`
- `ai-workbench/.../commands/index.fab`
- `ai-workbench/.../commands/model.fab`
- `ai-workbench/.../commands/query.fab`
- `vivilite/src/main.fab`

The factory starts with a fresh inventory, so new paths created after this plan
was written cannot escape the migration merely by being absent from this list.

## Normalized Spec

### Invariant

At a first-party application boundary, JSON text is decoded exactly once into
`json`; all structural access after that point is typed conversion or explicit
object access, and all output is produced from `json` or an `@ json` genus.

### Canonical migration pattern

Input paths follow one pipeline:

```text
text/file/stdin
      │
      v
norma:json.solve ─> json document ─> @json wire genus ─> domain validation
```

Output paths follow the reverse:

```text
domain result ─> @json response genus ─> json document ─> json.pange ─> stdout/file
```

The JSON genus represents the wire schema, not the whole domain model. Checks
such as supported FVI version, non-empty identifier, dimension agreement, and
vector-count agreement remain named domain validators after schema extraction.
Do not hide semantic validation inside another string parser or a giant
conversion expression.

### Shared FVI schema seam

Create one package-local schema module, preferably
`examples/ai-workbench/packages/faber-ai/src/fvi.fab`, rather than repeating
wire genera in each command. Exact genus names may follow local naming, but the
module must model these live records:

| Wire record | Required contract |
| --- | --- |
| stage-2 document | `format`, `model`, `source`, `status`, `input_count`, `dimensions`, `normalization`, `vectors`, `diagnostics` plus current documented summaries |
| vector record | stable identifier/text metadata and numeric `values` |
| stage-3 index | `format`, `source_format`, model/source metadata, input count, dimensions, metric, normalization, vectors, diagnostics |
| query vector | query identity/text and numeric values |
| scored result | `rank`, identifier, text, finite score |
| command response | command-specific success/error payload already promised on stdout |

Use `@ json { nomen = "..." }` only where Faber and wire names intentionally
differ. Do not rename a public FVI key merely to make a Faber identifier more
convenient.

Each versioned document gets a small public decode/validate function:

1. accept `json`, not raw `textus`;
2. convert to its `@ json` wire genus;
3. verify the exact format marker (`fvi-stage2`, stage-3 equivalent, or the
   current committed spelling);
4. verify counts and dimensions against the actual vector collection;
5. reject non-finite vector/score values even if an upstream runtime could
   construct them outside JSON text;
6. return a structured text error that names the document field or record.

Parsing and version validation stay separate so tests can identify whether a
failure is malformed JSON, wrong schema, or inconsistent FVI data.

### Parse-once rule

`index.fab` and `query.fab` currently scan the same input text repeatedly with
helpers such as `field_textus`, `field_numerus`, `array_end`, `object_end`,
`json_string_end`, and `json_unescape`. Delete those scanners after their last
caller migrates. A function may retain the original text only for diagnostics
or passthrough storage; it may not rescan it for a second field.

Subprocess stdout is a boundary too. `embed`, `generate`, `chat`, and `model`
must parse a provider response through one declared response genus where the
command needs structure. If a command deliberately forwards an opaque provider
payload, it must still validate the payload as `json` and document the opaque
boundary rather than splice it into another string.

### Output and determinism contract

- Build output from `@ json` genera or explicit JSON object literals.
- Use compact `json.pange` for stdout and persisted FVI artifacts.
- Object ordering is the formal runtime's deterministic ordering. JSON member
  order is not a schema contract, so old hand-assembly order is not preserved.
- Existing key names, required/nullable status, numeric meaning, and FVI format
  markers remain contracts unless a fixture proves they were only internal.
- One logical result produces one JSON document and one trailing newline on
  stdout. Diagnostics intended for humans go to stderr.
- Tests compare parsed documents for schema semantics and separately prove
  byte-for-byte deterministic re-rendering of the same document.

### Failure contract

Malformed JSON, a non-object root, missing required fields, wrong field types,
unknown/unsupported FVI versions, inconsistent dimensions/counts, and
non-finite numbers all fail at the owning boundary. Do not return partial
vectors or default missing required numeric fields to zero.

Command failures retain the CLI's current exit-status policy, but any
machine-readable error document must itself be built through an `@ json` genus.
A command must not emit a partial success document before validation finishes.

### Migration audit contract

Add a focused first-party source audit under `examples/scripta/` (exact name
chosen in M1) that detects known manual JSON construction and scanning idioms:

- helpers named `json_*`, `field_textus`, `field_numerus`, `array_end`, or
  `object_end` outside an explicit allowlist;
- long string concatenations containing JSON punctuation/quoted keys;
- direct assembly of FVI format markers into text;
- repeated substring/index scanning of JSON input.

The audit is a ratchet, not a blanket ban on braces or string interpolation.
Its initial allowlist must be empty for the selected application paths; any
unrelated unavoidable case is recorded with owner and reason, not broadly
excluded. Norma's formal JSON facade and tests are outside this application
audit.

### Clean-break posture

- Delete migrated string builders and scanners in the same stage as their last
  caller.
- Do not keep old helper wrappers around `json.solve`/`json.pange` merely to
  preserve names.
- Do not support both old `valor` and new `json` Norma signatures.
- Update checked-in fixtures and harnesses to the formal deterministic output;
  do not add dual-golden compatibility modes.

### Non-goals

- No redesign of the FVI domain or new FVI version.
- No provider API redesign unrelated to JSON correctness.
- No migration of arbitrary third-party packages or archived examples.
- No new general schema-description language beyond `@ json` genera.
- No hand-written streaming parser for large vectors in this goal. If measured
  documents make full-tree decoding untenable, stop and route a separate
  streaming design rather than weakening the formal type.

## Repo-Aware Baseline

### Facts

- Six AI workbench command files currently contain manual JSON behavior; the
  campaign intake named five and omitted `model.fab`.
- `vivilite/src/main.fab` is another active first-party manual JSON producer.
- `index.fab` and `query.fab` contain partial JSON string scanners and decode
  fields by rescanning source text.
- FVI stage-2, stage-3 index, query-vector, and query-result shapes are already
  exercised by shell/Python harnesses that parse emitted JSON.
- Existing harnesses use explicit timeouts and Python JSON readback, so they
  can compare schema semantics without preserving object insertion order.
- Goal 2 changes Norma's public JSON functions from broad `valor` to formal
  `json`; implementation before that closeout would target the wrong API.

### Architectural decision

Keep shared wire types with the owning Faber application package. Norma owns
the JSON document/codec contract; Radix owns the language machinery; neither
should acquire AI-workbench-specific FVI genera.

## Stage Graph

```text
M1 live inventory + schema ledger
             │
             v
M2 shared FVI genera/validators
             │
        ┌────┴──────────┐
        v               v
M3 readers/index/query  M4 emitters/chat/embed/generate/model
        └────┬──────────┘
             v
M5 vivilite + residual first-party paths
             │
             v
M6 audit, harnesses, docs, closeout
```

| Stage | Entry condition | Work and output | Exit gate |
| --- | --- | --- | --- |
| M1 — Inventory | Goal 2 J6 closed | Search active Faber sources; classify each hit as construction, parse, provider boundary, FVI boundary, or false positive. Record exact fixtures and commands in a migration ledger. | Every live hit has an owner/stage; no unexplained campaign-file mismatch. |
| M2 — Shared schema | M1 + formal `json` available | Add `@ json` FVI wire genera, version/domain validators, and focused round-trip/negative fixtures. | All committed FVI fixture families decode and re-encode deterministically; wrong versions/counts/dimensions fail. |
| M3 — Readers | M2 | Migrate `index.fab` and `query.fab` to parse once, convert to typed documents, then validate; delete scanners. | Existing stage-2/index/query harnesses pass and scanner symbols are absent. |
| M4 — Emitters/providers | M2 | Migrate chat/embed/generate/model command output and any structured provider response access to typed JSON. | No selected AI command constructs JSON punctuation by text concatenation; output schemas remain stable. |
| M5 — Residual apps | M3 + M4 | Migrate `vivilite` and every additional active hit from M1; update byte fixtures to canonical compact output. | Inventory ledger has no open hit and all owning packages run. |
| M6 — Ratchet | M5 | Land focused manual-JSON audit, docs, regenerated corpus metadata if needed, and release evidence. | Audit fails on a seeded forbidden helper/builder and passes on the clean tree. |

M3 and M4 may run in parallel after M2 because they own disjoint command files.
M5 remains a separate checkpoint so AI/FVI success cannot hide residual
first-party debt.

## Implementation Work

### Workstream A — Inventory and schema ledger

Search active `.fab` files for JSON punctuation assembly, field scanners,
provider payload handling, FVI markers, and old `norma:json` signatures. Store
the factory-time ledger beside this delivery plan or in the examples package
docs, with columns for path, boundary, schema, stage, fixture, and disposition.

Acceptance:

- generated/build/archive directories are excluded explicitly;
- each campaign-named file appears;
- `model.fab`, `vivilite`, and any newly discovered path are not silently
  discarded;
- false positives have a concrete explanation.

### Workstream B — FVI wire module

Primary surface:

- `examples/ai-workbench/packages/faber-ai/src/fvi.fab`
- adjacent package tests/fixtures and imports

Acceptance:

- wire genera use the live key spelling and symmetric `nomen` behavior;
- nested vector/diagnostic records are typed, not left as broad `valor`;
- format markers, dimensions, input/vector counts, metric, and normalization
  receive named validation;
- validation completes before any file/stdout write.

### Workstream C — AI command migration

Primary surfaces are the seven command files found by M1, initially the six
listed above. `index` and `query` are the risk-first batch; output-oriented
commands are the second batch.

Acceptance:

- each external JSON payload has one decode point;
- no deleted parser helper is recreated under a generic name;
- stable CLI keys and FVI markers are asserted by harnesses;
- negative fixtures cover malformed syntax, root mismatch, missing field,
  wrong type/version, inconsistent vector shape, and provider error payload.

### Workstream D — Residual application migration and audit

Migrate `vivilite` and inventory residuals. Add the audit script and run it in
the examples validation entrypoint or closest existing hygiene harness.

Acceptance:

- the audit is scoped to active first-party application source;
- a test fixture proves it detects at least one scanner and one manual builder;
- no broad directory exemption can conceal new Faber application debt.

### Workstream E — Harness and documentation proof

Update FVI shell/Python harnesses to assert parsed semantics, deterministic
repeat rendering, stderr/stdout separation, and failure status. Update package
docs with the formal JSON/FVI boundary and one canonical source example.

## Checkpoints And Gates

### Batching / split decision

Use three commits/checkpoints when practical:

1. shared FVI schema plus tests;
2. AI/FVI migration and deletion of scanners/builders;
3. residual applications, audit, and docs.

Do not split a helper deletion away from its last caller migration. Do not land
an audit that passes only because all current debt was broadly allowlisted.

### Correctness gates

| Gate | Required proof |
| --- | --- |
| Parse once | instrumentation or focused source audit shows one `solve` per input boundary and no field rescans |
| Typed schema | FVI documents pass through declared `@ json` genera before domain use |
| Atomic failure | invalid child/count/dimension yields no partial output artifact |
| Stable contract | parsed golden documents retain keys, types, markers, and nullability |
| Determinism | rendering an identical document twice is byte-identical |
| Residual closure | factory ledger and source audit have zero unowned hits |

### Release decision

`release-prep`, coordinated with Goal 2. This goal consumes the breaking Norma
signature and updates first-party products. Prepare application/FVI migration
notes, but do not create a second release boundary if Goal 2 and Goal 3 ship in
the same campaign train.

## Validation

Run the live command harnesses with explicit outer timeouts. The source audit is
created at the named path during M6:

```bash
cd ../examples
timeout 600 python3 ai-workbench/harness/check-chat.py
timeout 600 python3 ai-workbench/harness/check-embed.py
timeout 600 python3 ai-workbench/harness/check-generate.py
timeout 600 python3 ai-workbench/harness/check-index.py
timeout 600 python3 ai-workbench/harness/check-query.py
timeout 600 python3 ai-workbench/harness/check-model-inspect.py
timeout 180 python3 scripta/check-no-manual-json.py --check
git diff --check
```

Run each migrated package/command's existing focused test separately. Then run
the language/corpus integration lane:

```bash
cd ../radix
timeout 300 cargo test -p exempla json -- --format terse
timeout 300 ./scripta/test
git diff --check
```

If Norma direct callers change:

```bash
cd ../norma
timeout 180 ./scripta/check-source
git diff --check
```

M1 may add newly discovered owning-package harnesses, but must not replace the
six named integration lanes with a narrower synthetic test.

## Companion Skill Plan

- `correctness`: FVI count/dimension/version invariants, provider error paths,
  and atomic output.
- `red-green`: characterize existing JSON schemas before replacing builders;
  add malformed/schema-negative fixtures first.
- `cleanliness`: keep wire genera, domain validation, command orchestration,
  and transport separate; remove partial parser residue.
- `consequences`: compare every public stdout/file schema and downstream
  harness before final fixture updates.
- `polish`: inspect every migrated `.fab` file and the new audit one at a time.

## Open Questions

No blocking product question remains. M1 must resolve from live evidence:

1. the exact checked-in FVI format-marker spellings and optional fields for
   each fixture family;
2. which provider payloads are intentionally opaque versus structurally read;
3. whether newly discovered first-party sites have additional owning-package
   harnesses beyond the six named lanes.

## M1 Inventory Ledger

Generated/build/archive directories were excluded from the live scan. Active
first-party `.fab` source was searched for manual JSON emitters, partial JSON
scanners, FVI markers, provider payload boundaries, and direct
`norma:json` use.

| Path | Boundary | Schema / behavior | Stage | Fixture / gate | Disposition |
| --- | --- | --- | --- | --- | --- |
| `examples/ai-workbench/packages/faber-ai/src/commands/embed.fab` | JSON stdout and Stage 2 FVI artifact output | command summary, blocked/oracle Stage 2 vector artifact (`fvi-stage2`) | M2/M4 | `check-embed.py`; index fixtures consume Stage 2 artifacts | migrate emitters to shared `@ json` FVI/response genera; delete `json_quote`, `json_escape`, `json_string` after last caller |
| `examples/ai-workbench/packages/faber-ai/src/commands/index.fab` | Stage 2 FVI input parse, Stage 3 FVI output, JSON stdout | `fvi-stage2` input; `fvi-stage3-index` output; repeated substring scanners (`field_textus`, `field_numerus`, `array_end`, `json_unescape`) | M2/M3 | `check-index.py`; query fixtures consume Stage 3 indexes | parse once through `norma:json.solve`, convert to shared FVI genera, validate counts/dimensions, delete scanners |
| `examples/ai-workbench/packages/faber-ai/src/commands/query.fab` | Stage 3 index input parse, query-vector input parse, JSON stdout | `fvi-stage3-index`, `fvi-stage3-query-vector`, scored result output; repeated substring/object/array scanners | M2/M3 | `check-query.py` | parse both documents once, convert to shared FVI genera, validate dimensions/counts, delete scanners |
| `examples/ai-workbench/packages/faber-ai/src/commands/generate.fab` | JSON stdout and JSONL event artifact output | metadata/diagnostic events and command summary | M2/M4 | `check-generate.py` | migrate output to shared response/event genera; preserve JSONL artifact shape; delete manual string builders |
| `examples/ai-workbench/packages/faber-ai/src/commands/chat.fab` | JSON stdout and JSONL event artifact output | metadata/diagnostic events and command summary | M2/M4 | `check-chat.py` | migrate output to shared response/event genera; preserve JSONL artifact shape; delete manual string builders |
| `examples/ai-workbench/packages/faber-ai/src/commands/model.fab` | JSON stdout for alias/local model inspection | alias/model summary, tensors, diagnostics | M4 | `check-model-inspect.py` | migrate command summary output to typed JSON; retain binary metadata parsing in `norma:model` as non-JSON format parsing |
| `examples/vivilite/src/main.fab` | JSON stdout for status and board output | status object and board object with task/need/want item arrays | M5 | package tests tagged `vivilite`; CLI route smoke if available | migrate output helpers to `@ json` genera and `json.pange`; delete `json_escape`, `jq`, `lb`, `rb`, `q`, `qq`, `pair*` |
| `norma/src/model.fab` | safetensors/GGUF binary metadata parsing | scans binary-derived safetensors header text and GGUF fields, not first-party application JSON | retained | `check-model-inspect.py`; `norma` source checks | out of Goal 3 migration unless a command starts treating its output as JSON text; keep as binary-format parser |
| `norma/src/json*.fab` | formal JSON facade/parser/serializer | owns `json` document codec from Goal 2 | retained | `../norma/scripta/check-source` | out of application audit allowlist; this is the canonical JSON implementation |
| `examples/script-kernel/glob-import.fab` | corpus demonstration of `json.solve`/`json.pange` | already uses formal `norma:json` facade | retained | corpus/exempla gates | no migration needed |

No additional active first-party Faber JSON grammar owner was found outside
AI workbench, `vivilite`, the canonical Norma JSON implementation, and retained
binary-format parsing in `norma:model`.

Stop and return to campaign routing if a selected consumer requires arbitrary
root JSON, if FVI documents cannot fit the formal object-root contract, or if a
measured document size requires a streaming representation. Do not restore
`valor` parsing or a manual scanner as an expedient fallback.
