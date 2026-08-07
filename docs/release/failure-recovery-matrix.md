# Failure and recovery matrix

**Status:** accepted — Stage 1 decision record (component-release-streamline)
**Date-stamped:** 2026-08-07
**Resolves:** campaign Open Question 6 exceptions (stable immutability
exceptions) and the CAMPAIGN "Failure And Recovery Matrix" table
**Authority for every operator-authorized exception:** the withdraw/revocation
role in [`authority.md`](authority.md) §1.

---

## 1. Resolved failure table

Each row of the campaign's "Failure And Recovery Matrix" is resolved to
concrete semantics. Default responses are **accepted**; no row silently
replaces stable bytes or leaves discovery metadata pointing at an incomplete
release.

| Failure point | Default response | Semantics (decided) |
| --- | --- | --- |
| Before tag | **abort candidate** | Delete/never-create local candidate state; no public state existed; safe to restart from a new prepare (`release-contract.md` §5 idempotent retry). |
| One of several source tags created | **stop; record partial state** | Do not publish anything until source identity is reconciled; record which tags exist and which are authoritative; reconciliation is a verifier action. |
| Required target missing or fails | **candidate incomplete** | Stable promotion blocked for the whole component (`platform-builder-matrix.md` §1 — supported leg missing blocks); candidate may be re-marked or aborted; never promoted. |
| Upload interrupted / token expired | **retry only against identical manifest and hashes** | Same-hash retry is idempotent; a different-hash collision **fails closed** (`release-contract.md` §5.3). |
| Existing asset has different bytes | **fail closed** | Never automatic `--clobber`; the differing asset is inspected; a changed artifact requires a new patch release unless the exceptional incident path (§3) applies. |
| Bad checksum, notes, signature, or metadata before promotion | **correct candidate/draft, re-verify** | Candidate/draft namespace is mutable (non-production); fix, re-verify hashes/signature, re-run affected gates. |
| Defect found after promotion | **withdraw / deprecate + supersede** | Publish an incident note; deprecate or withdraw the release record; ship a new patch release. Stable assets stay in place; discovery metadata moves to the superseding release (§2). |
| Signing or upload credential compromised | **revoke, freeze, inventory, recover** | §4 — named compromised-credential path. |

## 2. Withdrawal, revocation, supersede — semantics

| Term | Meaning |
| --- | --- |
| **withdrawal** | A released candidate/asset is removed from **discovery** (draft/candidate state, or a stable release flagged withdrawn/deprecated with an incident note). Stable bytes are not silently deleted; the release record stays as evidence with a visible withdrawn marker. |
| **revocation** | A credential, signature key, or signing identity is declared invalid — applies to the **trust**, not to a rollback of bytes. Requires the operator + credential owner. |
| **supersede** | A newer release replaces the prior one in **discovery metadata** (installer/site/`Latest`) only after the new release passes all gates + remote readback. Supersede is the normal repair for post-promotion defects. |

The state machine is unchanged: `withdrawn → defect recorded; superseded, never
silently replaced` (CAMPAIGN "Candidate, Publication, And Promotion State
Machine").

## 3. Immutability exceptions (OQ6)

**Decision (accepted):** stable tags and assets are immutable; deletion, tag
movement, and stable-asset replacement are **never** ordinary rollback.
The single exceptional path:

1. **Trigger:** security or legal necessity only (e.g. a takedown, a
   credential-baked artifact that must not persist). Not convenience, not
   "oops", not flaky CI.
2. **Authority:** the withdraw/revocation operator role (`authority.md` §1).
3. **Record:** a written **incident record** naming the release, the trigger,
   the exact bytes/objects removed or replaced, the operator who authorized it,
   and the timestamp. The record is committed to the release docs.
4. **Aftermath:** the incident record is published alongside the affected
   release; discovery metadata is updated only in the same operation.

There is **no silent `--clobber`, no tag move, no stable-asset replacement**
as an ordinary retry/rollback mechanism (CAMPAIGN "Stop Conditions";
`release-contract.md` §5.3).

## 4. Compromised-credential path (named)

If a signing or upload credential is suspected or confirmed compromised:

1. **Revoke** the credential immediately (revocation authority = operator +
   credential owner).
2. **Freeze promotion** — no candidate → stable, no `Latest` update, no
   uploads under the affected identity.
3. **Inventory affected releases** — every release signed/published with the
   affected credential; re-verify their artifacts against independent sources.
4. **Publish a verified recovery record** — what was affected, what was
   re-signed/re-verified, and the new credential identity.
5. **Route** any release whose authenticity can no longer be established to
   withdrawal/supersede under §2.

## 5. Decision ledger

| # | Decision | Marking | Evidence |
| --- | --- | --- | --- |
| OQ6 | Stable immutability absolute; exceptional incident path §3; no silent clobber/tag move/replacement | **accepted** | B11 OQ6; F4 (`--clobber` gap); CAMPAIGN "Failure And Recovery Matrix" + Stop Conditions |
| OQ6 | Abort/partial-failure/withdrawal/revocation/supersede semantics §1–§2 | **accepted** | CAMPAIGN failure table; `release-contract.md` §5.3 |
| OQ6 | Compromised-credential path §4 | **accepted** | CAMPAIGN "Supply Chain And Secret Boundary" |

## 6. References

- `release-contract.md` §5.3 — immutability + idempotent retry.
- `authority.md` §1 — withdraw/revocation authority.
- `process-local-first.md` — where recovery decisions occur in the flow.
- CAMPAIGN "Failure And Recovery Matrix", "Stop Conditions".
