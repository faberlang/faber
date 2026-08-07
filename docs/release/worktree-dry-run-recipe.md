# Worktree dry-run recipe (release rehearsal)

**Status:** accepted — Stage 1 decision record (component-release-streamline)
**Date-stamped:** 2026-08-07
**Purpose:** the decided rehearsal procedure for a release candidate with
**zero public effect**. **Documentation only** — this stage authors no script;
Stage 3 scripts this recipe (bump, pin, package, checksum, plan generation).
**Layout authority:** [`radix/docs/factory/worktree-convention.md`](../../../radix/docs/factory/worktree-convention.md)
(factory worktree packet discipline).

---

## 1. Invariants

- No public mutation: no tag push, no `gh release`, no remote ref change, no
  public metadata change. The rehearsal is **local + network-read only**.
- No ambient credentials: the run holds **no signing keys and no publish
  tokens**; any step that would need one emits a `would-upload` placeholder
  instead (§4).
- Explicit output directory: every artifact, plan, and receipt lands in a
  named `out/` directory; nothing is written outside the worktree branches.
- Never auto-prune foreign worktrees: worktree lifecycle is operator-managed
  (worktree-convention.md "Housekeeping").
- Rehearsal never merges to main. A dry-run release bump is not a release
  accept/merge decision (CAMPAIGN "Stop Conditions").

## 2. Layout

A rehearsal is a **packet** under `faberlang/worktrees/` following the factory
convention:

```text
faberlang/worktrees/release-dry-run-<version>/
  faber/            # writable member on factory/release-dry-run-<version>
  radix/            # pinned member, detached at the exact source commit
  faber-runtime/    # pinned member, detached at the exact source commit
  hosts/            # pinned member, detached at the exact source commit
  cista/            # pinned member, detached at the exact source commit
  out/              # explicit output directory (plans, archives, receipts)
```

Writable members use the packet branch; read-only dependencies are **detached
and pinned to the exact commits being rehearsed** (the manifest's pins,
`release-manifest-schema.md` §4). The nested layout is load-bearing — Cargo
path dependencies resolve siblings relative to each checkout
(worktree-convention.md "Standard packet layout").

Worktree creation/removal is **operator-managed**; the recipe documents the
layout, and Stage 3 scripts the create/reuse/dispose flow.

## 3. Rehearsal steps

1. **Name the packet + output dir** (`release-dry-run-<version>`, `out/`).
2. **Prepare in the worktrees**: version bump, lockfile regen, pin manifest
   generation, draft release notes — never on main.
3. **Local proof** (`process-local-first.md` §2): locked build, gates, archive,
   basename-only checksum, private-radix leakage scan.
4. **Package** into `out/`: archives + `.sha256` + signed-manifest placeholder.
5. **Plan phase — stop at `would-tag` / `would-upload`**: emit a plan file
   naming the exact commands that *would* run and the exact remote objects
   that *would* be created (`would-tag vX.Y.Z`, `would-upload <manifest>
   → faberlang/releases`), plus the receipt (source pins, toolchain, hashes,
   gate outcomes). No step beyond the plan executes.
6. **Operator review**: inspect the plan + receipt; either authorize a real
   release (outside this dry-run) or abort. Dry-run success = a clean plan
   with no public writes and no credentials.

## 4. Scrubbed credentials

- The dry-run environment carries **no real tokens, keys, or secrets**. Any
  publish/sign step renders a placeholder (`would-upload`, `would-sign`) in
  the plan; the plan names the secret *holder* (e.g. "signing key held by the
  tagger/signer role on burgus") and never a value.
- The private radix checkout (a **network** step with a token today) is
  rehearsed by documenting it and, where available, using an operator-provided
  local clone — never by placing a private token in the rehearsal env.
- Credentials never enter rehearsed logs or receipts (leakage gate,
  `process-local-first.md` §4).

## 5. Stop points

| Stop point | What happens |
| --- | --- |
| `would-tag` | The plan lists the annotated tag that would be created and pushed; no tag is created. |
| `would-upload` | The plan lists the release objects and assets that would be published; nothing is uploaded. |
| Any step requiring a real credential | Becomes a `would-*` placeholder; the run does not proceed past it. |
| Missing supported matrix leg | Rehearsal records the candidate as incomplete (`platform-builder-matrix.md` §1); no plan to publish. |

## 6. Receipt

The dry-run receipt records: exact source pins, dirty-state checks, toolchains,
gates run, artifacts + hashes, timing, skipped claims, and the `would-tag` /
`would-upload` plan. Receipts go to `out/` and match the schema local and CI
receipts will share (CAMPAIGN Stage 8).

## 7. Decision ledger

| # | Decision | Marking | Evidence |
| --- | --- | --- | --- |
| — | Rehearsal procedure §1–§6 with `would-tag`/`would-upload` stops, scrubbed credentials, no public mutation, explicit output dir | **accepted** (documentation only; Stage 3 scripts it) | B11 OQ2/§9; CAMPAIGN "Desired End State" 3 + Stage 3; `delivery-stage1.md` write_scope |
| — | Worktree layout per factory convention; operator-managed lifecycle | **accepted** | `radix/docs/factory/worktree-convention.md` |

## 8. References

- `process-local-first.md` — the flow being rehearsed.
- `release-manifest-schema.md` — pins the rehearsal resolves.
- `platform-builder-matrix.md` — legs and gates in scope.
- `radix/docs/factory/worktree-convention.md` — packet layout + lifecycle.
