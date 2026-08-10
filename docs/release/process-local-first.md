# Local-first release process (contract level)

**Status:** accepted — Stage 1 decision record (component-release-streamline)
**Date-stamped:** 2026-08-07
**Resolves:** the decided local-first flow, per-step classification, the local
proof of releasability, gate mapping, and the private-radix leakage gate.
**Scope note:** this is the **contract-level process** — step order,
classification, proof, and gates. The full operator runbook + threat model is
Stage 2; the rehearsal procedure is `worktree-dry-run-recipe.md`; scripts are
Stage 3. No script is authored in this stage.

---

## 1. Decided flow

The release process completes its **proof of releasability locally**; GitHub
Actions becomes corroboration or publish, not the only truth (CAMPAIGN
"Development Posture"). Step classification uses the four-row legend from
`stage0-baseline.md` ("Classification legend"):

- **local** — operator-controlled machine, no required public effect
- **controlled-builder** — managed build platform (GHA hosted runner today)
- **network** — requires network/private clone/token/remote fetch; no public write
- **public mutation** — changes externally visible public state

| # | Step | Classification |
| --- | --- | --- |
| 1 | **Prepare**: version bump (faber `Cargo.toml`; radix bulk `crates/*`; cista `Cargo.toml`), lockfile regen (`cargo update`; `--offline` caveat), pin manifest update (`release-manifest-schema.md` §7), draft release notes | **local** (lockfile index fetch = **network** unless `--offline`) |
| 2 | **Local proof**: locked release build + local gates + archive/checksum + leakage scan (§4) on burgus/pharos | **local** |
| 3 | **Tag**: annotated/signed source tag `vX.Y.Z` (bookkeeping); push main + tag | **local**; push = **network** |
| 4 | **Controlled-builder build**: CI rebuilds from the pinned inputs (`release-manifest-schema.md` §4) on the matrix legs; stable/LTS product releases add a second clean-builder comparison (`platform-builder-matrix.md` §3) | **controlled-builder** (private radix clone in faber GHA = **network**) |
| 5 | **Package + checksum + sign**: archive per the matrix, basename-only `.sha256`, sign the checksum manifest, assemble provenance/SBOM | **local** (mirror in CI) |
| 6 | **Publish candidate**: upload to the candidate/draft namespace | **public mutation** (operator only) |
| 7 | **Readback**: re-download public bytes, verify hashes/signature/`--version` | **network** (read-only) |
| 8 | **Promote**: advance candidate → stable; update installer/site metadata + global `Latest` **last** | **public mutation** (operator only) |

Steps 6–8 are operator-authorized external effects (`authority.md` §2). The
dry-run recipe stops after step 5 at the `would-tag` / `would-upload` plan.

## 2. Local proof of releasability

**Decision (accepted):** a release is **ready** when, on an operator-controlled
machine (burgus/pharos), all of the following pass:

1. locked release build (`cargo build --locked --release --bin <component>`);
2. the component's release gate (§3);
3. the pin manifest validates and its pins match live evidence (§1 step 1);
4. archive + basename-only checksum manifest produced; checksum verifies;
5. **consumer smoke-test gate** on the packaged archive
   (`scripta/smoke-test-release-archive --archive dist/<component>-v<version>-<triple>.tar.gz
   --version <version> --triple <triple>`) — the P0 guard: a bare-binary
   archive must be rejected with the `pack-error` class
   (`--expect-fail-class pack-error`);
6. private-radix leakage scan clean (§4).

CI is **corroboration**, and the publish leg still needs network — but the
**proof** that the release is releasable never depends on GitHub Actions
queues, flaky runners, or publish timeouts (CAMPAIGN Problem table / Desired
End State 2).

## 3. Gate mapping

| Surface | Gate | When | Evidence |
| --- | --- | --- | --- |
| faber product | `./scripta/release-gate --locked-release-build` (only full-workspace gate required) | every faber release | `faber/AGENTS.md:131-133` |
| faber product archive | `./scripta/smoke-test-release-archive` (consumer smoke-test gate — pack-exercising first, version last, archive-layout proof; negative-mode `--expect-fail-class`) | every faber release, at packaging (CI, before upload) AND readback (step 7, on the downloaded bytes) | `docs/factory/faber-1.5.1-pack-release/delivery.md` §5 U3 / §6 G5; unit test `scripta/smoke-test-release-archive-test.py` |
| faber compiler/corpus claims | `../radix/scripta/test --full` / `--e2e` | optional, when the release includes compiler/corpus claims | `faber/AGENTS.md:133-134` |
| radix component | radix ladder `./scripta/test --full` at tag; `--stage 1-4` on main | every radix release (tag) | `radix/AGENTS.md:389`; `radix/.github/workflows/ci.yml` |
| cista component | smoke: build + `--version` today; test/lint/hygiene/install/package smoke surfaces are a recorded gap (F6) | every cista release | `stage0-baseline.md` §1.2 / F6 — **routed** to cista owner / Stage 8 |
| clean-install / portable | sibling `release-and-portable-default` portable gates | called by faber product dry-run **when they exist** | CAMPAIGN "Ground Truth"; sibling delivery.md |

No gate is run more than once per release boundary; the matrix's
second-builder comparison is an additional cross-check, not a gate re-run
(`platform-builder-matrix.md` §3).

## 4. Private-radix leakage gate

**Decision (accepted):** before any public upload, the staged archive and
receipts must prove the absence of private Radix material:

- only bytes in the **staged dist directory** + generated receipts are ever
  uploaded — never build trees;
- the scan rejects private source paths (`/Users/ianzepp/…`, `../radix/…`
  absolute build paths), private-repo markers, embedded tokens/credentials,
  and secret URLs;
- CI logs are never published as release evidence;
- the signed checksum manifest covers exactly the staged artifacts.

The full threat model (untrusted build inputs, credential exposure, asset
replacement) is Stage 2; this gate is the decided minimum enforced from Stage
1 onward.

## 5. Decision ledger

| # | Decision | Marking | Evidence |
| --- | --- | --- | --- |
| — | Local-first flow §1 with per-step classification | **accepted** | B11 OQ2/§9 flow; CAMPAIGN "Desired End State" 1–2; `stage0-baseline.md` §4 |
| — | Local proof of releasability §2 | **accepted** | CAMPAIGN "Development Posture" (local proof is authority) |
| — | Gate mapping §3 (faber release-gate, radix ladder, cista smoke) | **accepted**; cista smoke surfaces **routed** (cista owner / Stage 8) | `faber/AGENTS.md:131-134`; `radix/AGENTS.md:389`; F6 |
| — | Private-radix leakage gate §4 | **accepted** | CAMPAIGN "Supply Chain And Secret Boundary" |

## 6. References

- `release-contract.md` — contract, channels, identity.
- `authority.md` — role boundaries in this flow.
- `failure-recovery-matrix.md` — recovery in this flow.
- `worktree-dry-run-recipe.md` — the rehearsal procedure.
- `stage0-baseline.md` §4 — network vs offline classification.
