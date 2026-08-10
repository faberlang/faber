# Release checklist (operator, cold)

**Status:** active — Stage 2 delivery (component-release-streamline)
**Date-stamped:** 2026-08-07
**Purpose:** one line per flow step, usable cold. Authority role, classification,
exact command, and stop point. Full prose + all paths: `release-runbook.md`;
decisions: the seven Stage-1 docs. Dry-run form: `worktree-rehearsal-procedure.md`.
Live-state caveat (2026-08-07): re-verify any version/tag before executing.

Legend — classification: **L** local · **CB** controlled-builder · **N** network · **P** public mutation
Authority — `authority.md` §1: **proposer** / **builder** / **verifier** /
**tagger/signer** / **publisher** / **promoter** / **withdraw-revocation**
(grey = agent may act by default; **bold** = operator only).

| # | Step | Class | Authority | Command (faber; radix/cista variants in runbook §2.1) | Stop point |
| --- | --- | --- | --- | --- | --- |
| 1 | **Prepare** | L (N if `cargo update` index fetch) | **proposer** → **verifier** | bump `Cargo.toml` (radix: bulk `perl -pi -e` over `crates/*/Cargo.toml`); `cargo update`; update `release-manifest.yaml` pins + packs (`release-manifest-schema.md` §7); draft notes | manifest validation + pin-vs-live check; mismatch = hard stop |
| 2 | **Local proof** | L | **builder** → **verifier** | `cargo build --locked --release --bin faber`; `./scripta/release-gate --locked-release-build` (`faber/AGENTS.md:126-133`); optional `../radix/scripta/test --full` for compiler/corpus claims; **consumer smoke-test gate** per triple: `scripta/smoke-test-release-archive --archive dist/faber-vX.Y.Z-<triple>.tar.gz --version X.Y.Z --triple <triple>` (P0 guard: bare-binary archive rejected via `--expect-fail-class pack-error`); leakage scan (`process-local-first.md` §4) | gate fail, leak hit, smoke fail, or missing supported matrix leg = stop (candidate incomplete) |
| 3 | **Tag** | L; push N | **tagger/signer** | single bump+lock commit; `git tag -a vX.Y.Z -m "<Component> vX.Y.Z"`; `git push origin main && git push origin vX.Y.Z` | **never tag a stale lockfile** (`faber/AGENTS.md:141-144`); tag must equal manifest version |
| 4 | **Controlled-builder** | CB (radix clone = N) | **builder** | tag push triggers `release.yml`; `gh run list -R faberlang/faber --limit 1` | missing **supported** leg blocks the whole release (`platform-builder-matrix.md` §1) |
| 5 | **Package/checksum/sign** | L (mirror in CI) | **tagger/signer** | archive `<component>-v<v>-<triple>.tar.gz`; `(cd dist && shasum -a 256 "<archive>" > "<archive>.sha256")` **basename-only**; detached Ed25519 over the manifest; provenance/SBOM for stable/LTS product | checksum basename check; signed manifest covers exactly the staged artifacts |
| 6 | **Publish candidate** | P | **publisher** | draft/candidate namespace first (`gh release create --draft` today; workflow regex rejects `-rc.N`); product: `gh release create faber-vX.Y.Z dist/* --repo faberlang/releases`; components: `--latest=false` | explicit external-write approval required; no `--clobber`; same hashes as readback target |
| 7 | **Readback** | N (read-only) | **verifier** | `gh release download <tag> --repo faberlang/releases --dir out/readback`; `shasum -c`; **consumer smoke-test gate on the downloaded bytes** per triple: `scripta/smoke-test-release-archive --archive out/readback/faber-vX.Y.Z-<triple>.tar.gz --version X.Y.Z --triple <triple>` (or `--download-into out/readback`); verify detached signature | promotion blocked until all required artifacts pass remote readback + smoke (`release-contract.md` §4.3) |
| 8 | **Promote** | P | **promoter** | candidate → stable; installer/site/package-index/Homebrew/`Latest` **last** (`release-contract.md` §4.2–4.3) | global `Latest` reserved for the latest **promoted product** release; never partial |
| 9 | **Withdraw/supersede** | P | **withdraw-revocation** | incident note; withdraw/deprecate the record; supersede with a new patch release after gates + readback | stable bytes stay in place; no tag move, no deletion as rollback |

**Abort (pre-tag):** drop the never-pushed local candidate commit; no public
state exists. **Partial-tag abort:** stop, record partial state, reconcile
source identity before anything publishes (`failure-recovery-matrix.md` §1).
**Never-pushed local tag cleanup:** `git tag -d vX.Y.Z` is safe; a pushed tag
is public — never delete or move it.

**Cheat line for the common path (faber product, stable):**
`cargo update` → build+gate (`./scripta/release-gate --locked-release-build`)
→ single commit → `git tag -a vX.Y.Z` → push main+tag → monitor CI → package +
basename `.sha256` + sign → draft publish → readback `shasum -c` → promote
`Latest` last.

**References:** `release-runbook.md` (all paths, prose); `process-local-first.md`
(flow + gates); `authority.md` (roles); `release-contract.md` (channels,
immutability, Latest); `platform-builder-matrix.md` (matrix); `threat-model.md`
(four classes); `worktree-rehearsal-procedure.md` (dry-run form).
