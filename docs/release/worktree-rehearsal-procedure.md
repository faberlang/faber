# Worktree release-rehearsal procedure (operationalized dry run)

**Status:** active — Stage 2 delivery (component-release-streamline)
**Date-stamped:** 2026-08-07
**Purpose:** the **runnable form** of the Stage-1 recipe
[`worktree-dry-run-recipe.md`](worktree-dry-run-recipe.md) — exact commands,
the operator checklist, the pin-matrix generation steps, and the receipt
schema, with **zero public effect**. This procedure operationalizes the recipe;
it does not rewrite it. Scripts that automate it are Stage 3.
**Layout authority:** [`radix/docs/factory/worktree-convention.md`](../../../radix/docs/factory/worktree-convention.md)
(factory worktree packet discipline) — packet lifecycle is **operator-managed**.
**Invariants (from the recipe §1):** no tag push, no `gh release`, no remote
ref change, no public metadata change; no ambient credentials (any step needing
one emits a `would-*` placeholder); everything lands in the packet's `out/`;
foreign worktrees are never auto-pruned; the rehearsal never merges to main.

> **Foreign-worktree rule:** `exact-output-transfer` and `test-lifecycle-split`
> are foreign worktrees today, as is any pre-existing
> `release-dry-run-<version>` packet. This procedure never prunes, moves,
> repairs, or reuses them — the operator owns that decision
> (worktree-convention.md "Housekeeping").

---

## 1. Packet naming and layout

A rehearsal is a packet named **`release-dry-run-<version>`** at
`/Users/ianzepp/work/faberlang/worktrees/release-dry-run-<version>/`, following
the factory convention (worktree-convention.md "Standard packet layout"):

```text
/Users/ianzepp/work/faberlang/worktrees/release-dry-run-<version>/
  faber/            # writable member on factory/release-dry-run-<version>
  radix/            # pinned member, detached at the exact source commit
  faber-runtime/    # pinned member, detached at the exact source commit
  hosts/            # pinned member, detached at the exact source commit
  cista/            # pinned member, detached at the exact source commit
  out/              # explicit output directory (plans, archives, receipts)
```

The nested layout is load-bearing: `faber/Cargo.toml` resolves
`radix = { path = "../radix/crates/radix" }` to the packet-local sibling
(worktree-convention.md). Writable members use the packet branch; read-only
dependencies are **detached and pinned to the exact commits** resolved by the
pin-matrix procedure (§4).

---

## 2. Member lifecycle (operator-managed)

### 2.1 Create

```bash
SLUG="release-dry-run-v1.5.0"                              # exact packet name
PACKET="/Users/ianzepp/work/faberlang/worktrees/${SLUG}"

# Writable faber member on the packet branch (branch off a green main tip):
git -C /Users/ianzepp/work/faberlang/faber worktree add \
  -b "factory/${SLUG}" "${PACKET}/faber" main

# Pinned members, detached at the exact commits from the pin matrix (§4).
# <pin-sha> is resolved live per §4 step 3 — never guessed:
git -C /Users/ianzepp/work/faberlang/radix worktree add --detach \
  "${PACKET}/radix" <radix-pin-sha>
git -C /Users/ianzepp/work/faberlang/cista worktree add --detach \
  "${PACKET}/cista" <cista-pin-sha>
git -C /Users/ianzepp/work/faberlang/faber-runtime worktree add --detach \
  "${PACKET}/faber-runtime" <faber-runtime-pin-sha>
git -C /Users/ianzepp/work/faberlang/hosts worktree add --detach \
  "${PACKET}/hosts" <hosts-pin-sha>

# Explicit output directory:
mkdir -p "${PACKET}/out"
```

**Verify detachment and pin identity before doing anything else:**

```bash
git -C "${PACKET}/radix" rev-parse HEAD          # must equal <radix-pin-sha>
git -C "${PACKET}/cista" rev-parse HEAD          # must equal <cista-pin-sha>
git -C "${PACKET}/faber-runtime" rev-parse HEAD  # must equal <faber-runtime-pin-sha>
git -C "${PACKET}/hosts" rev-parse HEAD          # must equal <hosts-pin-sha>
```

### 2.2 Reuse

A previous `release-dry-run-<version>` packet may be reused **only** when:

1. the operator confirms no other session is active in it (`git worktree list`
   + the Vivi mailspace for the packet slug), and
2. the pinned members still sit at the same SHAs (re-verify with the command
   block above), and
3. `faber/` is on `factory/release-dry-run-<version>` with no uncommitted work
   that matters.

Do not reuse an integrated, dirty, orphaned, or historically named worktree
(worktree-convention.md).

### 2.3 Dispose

Disposal is the **operator's** action after the rehearsal outcome is recorded
and, if the candidate is accepted, after the real release path has consumed it:

```bash
# Per-member removal (never git worktree prune on foreign trees):
git -C "${PACKET}/faber" worktree remove "${PACKET}/faber" --force  # operator decision only
git -C /Users/ianzepp/work/faberlang/radix worktree remove "${PACKET}/radix"
git -C /Users/ianzepp/work/faberlang/cista worktree remove "${PACKET}/cista"
git -C /Users/ianzepp/work/faberlang/faber-runtime worktree remove "${PACKET}/faber-runtime"
git -C /Users/ianzepp/work/faberlang/hosts worktree remove "${PACKET}/hosts"
git -C /Users/ianzepp/work/faberlang/faber branch -D "factory/${SLUG}"  # after integration decision
```

`./scripta/audit-worktrees` (radix) is for **visibility only**; it never
authorizes creation, removal, or pruning (worktree-convention.md
"Housekeeping").

---

## 3. Rehearsal steps (exact commands)

The rehearsal runs the runbook flow up to and including package
(`release-runbook.md` §1 steps 1–5), then **stops** at the `would-tag` /
`would-upload` plan. Nothing beyond the plan executes.

### Step 1 — Pin matrix + plan header

Follow §4 to produce the candidate pin matrix. Record the plan header:

```bash
cd "${PACKET}"
cat > out/release-plan-v1.5.0.md <<'EOF'
# Dry-run release plan — faber v1.5.0
rehearsal: release-dry-run-v1.5.0
date: <today ISO-8601>
operator: <name>
channel: stable            # development|candidate|stable|lts|hotfix
line: "1.x"
# pin matrix → §4 table; would-tag / would-upload → §6
EOF
```

### Step 2 — Prepare in the worktrees (never on main)

```bash
cd "${PACKET}/faber"
# version bump + lockfile regen (local; cargo update index fetch = network)
#   edit Cargo.toml version = "1.5.0"; then:
cargo update
#   update faber/release-manifest.yaml source pins (§4) + packs rows
#   (consume the faber-onboarding instance; record pending rows per its shape)
#   draft notes: docs/release/v1.5.0.md (release record)
git add -A && git commit -m "release(wip): faber v1.5.0 bump + lockfile + manifest (dry-run)"
```

The commit is **local to the packet branch** — the rehearsal never merges it
to main (recipe §1).

### Step 3 — Local proof

```bash
cd "${PACKET}/faber"
cargo build --locked --release --bin faber
./scripta/release-gate --locked-release-build
# optional compiler/corpus claims: ../radix/scripta/test --full
# leakgate scan (§7 of this procedure) over the staged dist + receipts
```

### Step 4 — Package into `out/`

```bash
cd "${PACKET}/faber"
TRIPLE="aarch64-apple-darwin"          # one leg at a time; matrix per platform-builder-matrix.md §2
staging="dist/faber-v1.5.0-${TRIPLE}"
mkdir -p "$staging"
cp "target/${TRIPLE}/release/faber" "$staging/faber"
chmod +x "$staging/faber"
cat > "$staging/README.txt" <<EOF
faber v1.5.0
target: ${TRIPLE}
source: https://github.com/faberlang/faber/tree/v1.5.0
EOF
archive="faber-v1.5.0-${TRIPLE}.tar.gz"
tar -C dist -czf "dist/${archive}" "faber-v1.5.0-${TRIPLE}"
(cd dist && shasum -a 256 "${archive}" > "${archive}.sha256")   # basename-only content
grep -Eq "^[0-9a-f]{64}  ${archive}$" "dist/${archive}.sha256"   # the release.yml:150-157 check
cp dist/* "${PACKET}/out/"
```

### Step 5 — Plan phase: stop at `would-tag` / `would-upload`

Append the plan to `out/release-plan-v1.5.0.md` (template in §6), listing:

- `would-tag v1.5.0` — the annotated tag that *would* be created on the bump
  commit and pushed to `origin` (no tag is created);
- `would-upload <out/release-plan-v1.5.0.md> → faberlang/releases` as
  `faber-v1.5.0` — the release objects and assets that *would* be published
  (nothing is uploaded);
- `would-sign` — the detached Ed25519 signature over the checksum manifest,
  naming the secret *holder* ("signing key held by the tagger/signer role on
  burgus") and never a value;
- the receipt (§6).

### Step 6 — Operator review

Inspect the plan + receipt. **Dry-run success = a clean plan with no public
writes and no credentials.** Either authorize a real release (outside this
dry-run, per `release-runbook.md`), or abort. A rehearsal branch is never
merged to main by the rehearsal.

---

## 4. Pin-matrix generation (hand-followable procedure)

Stage 3's `generate-release-manifest` automates exactly this procedure; until
then the operator produces the candidate pin matrix by hand from live
evidence. Output: the `pinnedInputs.source` + `pinnedInputs.packs` rows of the
candidate plan, matching `release-manifest-schema.md` §3/§4/§6.

1. **State the release intent.** Read the manifest's `releaseIntent` (or the
   draft): component, version, channel, line (`release-manifest-schema.md`
   §3). Example: `faber`, `1.5.0`, `stable`, `1.x`.
2. **Verify the component source tag SHA.** Resolve the tag and confirm the
   `Cargo.toml` version equals the tag version (`release-contract.md` §3.2):
   ```bash
   git ls-remote --tags origin v1.5.0          # network, read-only — or locally:
   git rev-parse "v1.5.0^{commit}"             # the tag's commit SHA
   grep -m1 '^version' Cargo.toml              # must equal 1.5.0
   ```
3. **Resolve sibling commits.** For every other pinned input, fix the exact
   commit SHA from live evidence:
   - **radix / cista**: the tagged commit of the sibling version being used,
     e.g. `git ls-remote --tags origin v0.79.0` then the tag commit SHA
     (`release-manifest-schema.md` §4);
   - **faber-runtime / hosts**: the explicit commit (these have no release
     tags; `process-versioning-and-deps.md` §3.2);
   - if carrying forward the last release record's companion pins, verify each
     SHA still resolves: `git cat-file -e <sha>` in that checkout.
   The pin is a **commit SHA** — never a branch tip, never a moving ref
   (this is the F4/B7 gap the manifest closes).
4. **List the dev-kit payload packs + digests from the onboarding manifest
   instance.** Read `faber/release-manifest.yaml` `pinnedInputs.packs` rows —
   component, version, digest, compatibility, license, destination per
   `release-manifest-schema.md` §6 (launcher, core-support, reference-pack,
   locale-packs, library-pack shape from `faber-onboarding/dev-kit-contract.md`).
   - If the instance is not yet committed (faber-onboarding Stage 2 in flight),
     record each pack row as **pending** against the §6 shape with its source
     (`assemble-dev-kit` produces it) — never invent a digest, never write a
     parallel manifest.
5. **Record the candidate plan.** Write the verified pin rows + the
   `would-tag` / `would-upload` plan to `out/` (receipt schema §6). A pin that
   fails verification (mismatched version, unresolvable SHA) marks the
   candidate **incomplete** — no `would-upload` plan is produced.

---

## 5. Scrubbed credentials

- The rehearsal environment carries **no real tokens, keys, or secrets**
  (recipe §4). Publish/sign steps render `would-upload` / `would-sign`
  placeholders; the plan names the secret *holder*, never a value.
- The private radix checkout is a **network** step with a token today
  (`faber/.github/workflows/release.yml:69-76`). It is rehearsed by
  documenting it and, where available, using an operator-provided local clone
  — never by placing a private token in the rehearsal env (recipe §4;
  threat-model T1/T3).
- Before running, confirm the environment holds no ambient credentials:
  ```bash
  env | grep -iE 'token|secret|key|gh_|git_credential' || echo "no ambient credentials"
  ```
  (If any render, stop — see §8 stop points.)

---

## 6. Receipt schema

Receipts go to `out/` and record (recipe §6):

| Field | Content |
| --- | --- |
| `rehearsal` | packet slug (`release-dry-run-<version>`) |
| `date` / `operator` | ISO date + the acting operator |
| `sourcePins` | the verified pin matrix (§4): component tag + sibling commit SHAs + pack rows |
| `dirtyChecks` | `git status --porcelain` empty for every writable member at start and end |
| `toolchain` | Rust toolchain/SDK versions + target triples used |
| `gates` | every gate run + outcome (`release-gate`, ladder claims, smoke) |
| `artifacts` | archive list + basename-only SHA-256 hashes (== the manifest's pack digests where they exist) |
| `leakageScan` | scan scope + result (clean or blocker) |
| `timing` | start/end timestamps |
| `skippedClaims` | claims intentionally not run (e.g. CI legs, second-builder comparison) |
| `wouldPlan` | the `would-tag` / `would-upload` / `would-sign` plan, verbatim |

Plan template (appended in step 5):

```text
would-tag:  v1.5.0 → faberlang/faber (annotated, on commit <bump-commit>)
would-upload: faber-v1.5.0 → faberlang/releases
  assets: faber-v1.5.0-<triple>.tar.gz + .sha256 (basename-only)
  latest: NO (component) | YES (product, after readback)
would-sign: detached Ed25519 over the checksum manifest
  secret holder: <role + machine>, never a value
stop: no tag created, no bytes uploaded, no credentials touched.
```

---

## 7. Leakage scan (T1 control, run on `out/`)

```bash
cd "${PACKET}/out"
# reject private source paths, private-repo markers, token shapes, secret URLs:
grep -rniE '/Users/ianzepp|\.\./radix|faberlang/radix|FABERLANG_RELEASES_TOKEN|ghp_[A-Za-z0-9]{20,}' . \
  && echo "LEAKAGE HIT — stop" || echo "leakage scan clean"
```

A hit blocks the `would-upload` plan (threat-model §2; `process-local-first.md`
§4).

---

## 8. Stop points

| Stop point | What happens |
| --- | --- |
| `would-tag` | The plan lists the annotated tag that would be created and pushed; no tag is created (recipe §5). |
| `would-upload` | The plan lists the release objects/assets that would be published; nothing is uploaded. |
| Any step requiring a real credential | Becomes a `would-*` placeholder; the run does not proceed past it. |
| Missing supported matrix leg | Candidate recorded incomplete (`platform-builder-matrix.md` §1); no plan to publish. |
| Leakage-scan hit or ambient-credential hit | Stop; fix/record; re-run the scan before any plan is accepted. |
| Pinned member not at its pin SHA | Stop; the operator re-syncs the member before any build. |
| Foreign worktree in the packet path | Stop; never prune/move — the operator owns it. |

---

## 9. Worked walkthrough (cold-operator proof, 2026-08-07)

A Faber **product** dry-run for `v1.5.0` (dev line, per
`v1.5.0-dev-notes.md`; baseline facts from `process-versioning-and-deps.md`
§2.2):

1. **Pin matrix.** Intent: faber `1.5.0`, stable, `1.x`. Source tag verified:
   `git rev-parse v1.5.0^{commit}` (example values below are **illustrative
   companion pins from the v1.4.0 record**, `v1.4.0.md` "Companion pins" —
   resolved live in a real run):

   | Input | Pin (example) |
   | --- | --- |
   | faber | own tag commit `v1.5.0` |
   | radix | `v0.79.0` → `5bbdbbd49` |
   | cista | `99acb1e` |
   | faber-runtime | `57493dc` |
   | hosts | `ced40f8` |
   | packs | rows from `faber/release-manifest.yaml` `pinnedInputs.packs` (launcher, core-support, reference-pack, locale-packs, library-pack); `pending` until the onboarding instance lands |

2. **Packet.** `release-dry-run-v1.5.0` created per §2.1 with `faber` writable
   on `factory/release-dry-run-v1.5.0` and the four siblings detached at the
   table's SHAs; every `rev-parse HEAD` matched.
3. **Prepare.** `Cargo.toml` → `1.5.0`, `cargo update`, manifest pins +
   pack rows recorded, `docs/release/v1.5.0.md` drafted; single local commit on
   the packet branch.
4. **Local proof.** `cargo build --locked --release --bin faber` +
   `./scripta/release-gate --locked-release-build`; leakage scan clean.
5. **Package.** `faber-v1.5.0-aarch64-apple-darwin.tar.gz` + basename-only
   `.sha256` into `out/` (one leg; `x86_64-unknown-linux-gnu` second).
6. **Plan.** `out/release-plan-v1.5.0.md` records
   `would-tag v1.5.0`, `would-upload faber-v1.5.0 → faberlang/releases`
   (`--latest=false` for the surface; the product form would add the
   promotion rule), `would-sign` (secret holder named), the receipt, and the
   stop line. **No tag, no upload, no credentials.**
7. **Review.** Operator either authorizes a real release (outside the dry-run)
   or aborts; the packet is disposed per §2.3 after the decision.

---

## 10. References

- `worktree-dry-run-recipe.md` — the Stage-1 recipe this procedure runs.
- `release-runbook.md` — the flow being rehearsed (§1 steps 1–5).
- `release-manifest-schema.md` §3/§4/§6 — pins + packs the matrix resolves.
- `platform-builder-matrix.md` §1–§3 — legs, missing-leg block, builders.
- `radix/docs/factory/worktree-convention.md` — packet layout + lifecycle.
- `threat-model.md` — T1/T3 controls this procedure enforces.
- `faber/docs/factory/faber-onboarding/delivery-stage2.md` +
  `dev-kit-contract.md` — the pack rows' producer (read-only here).
