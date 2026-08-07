# Release authority model

**Status:** accepted — Stage 1 decision record (component-release-streamline)
**Date-stamped:** 2026-08-07
**Resolves:** campaign Open Question 9 (production authority: which human role
may tag, publish, promote, withdraw, revoke, supersede)

> Separate roles are named even when one operator fills several roles
> (CAMPAIGN "Authority And Durable Homes"). The role table is the contract;
> seat-filling is an operator decision.

---

## 1. Roles

| Role | May do | Authority | Default agents may? |
| --- | --- | --- | --- |
| **proposer** | Draft a release intent: version bump, manifest change, release notes | any release participant (operator or agent) | **yes** (prepare only) |
| **builder** | Build, gate, archive, checksum on controlled builders (burgus / pharos / GHA hosted) | operator-controlled machines + controlled builders | **yes** (build and package in dry-run; not for production publish) |
| **verifier** | Check version alignment, pinned SHAs, gate outcomes, hashes, signatures, leakage scan | operator or trusted reviewer | **yes** (inspect and verify) |
| **tagger/signer** | Create the annotated/signed source tag; sign the checksum manifest with the release signing key | **operator only** | **no** |
| **publisher** | Upload candidate/draft and final assets to `faberlang/releases` | **operator only** | **no** |
| **promoter** | Advance candidate → stable, update installer/site metadata and global `Latest` | **operator only** | **no** |
| **withdraw / revocation** | Withdraw, supersede, revoke credentials, authorize the exceptional incident path (`failure-recovery-matrix.md` §3) | **operator only** (revocation also requires the credential owner) | **no** |

## 2. Production authority is operator-owned

Production tags, public uploads, promotion, overwrite, deletion, and revocation
remain **operator-authorized external effects**. No agent defaults into them —
agents may **prepare, inspect, and dry-run** by default (CAMPAIGN "Authority
And Durable Homes"; `delivery-stage1.md` write_scope). The dry-run recipe
(`worktree-dry-run-recipe.md`) rehearses every role's actions with zero public
effect.

A role may be delegated to a specific trusted agent **only** by explicit
operator authorization for that release, and even then `tagger/signer` and
`promoter` authority require the operator to hold the corresponding secret
and to execute or explicitly witness the final external effect.

## 3. Cross-repo authority mapping

| Scope | Authority owner | Notes |
| --- | --- | --- |
| Coordinated Faber product process | this contract + the operator | `faber/docs/release/` is the durable home (CAMPAIGN "Authority And Durable Homes") |
| Faber product scripts/receipts | `faber/scripta/` + `faber/docs/release/` | product surface |
| Radix component protocol | `radix/docs/release/` | radix-local scripts; release workflow owner |
| Cista component protocol | `cista/docs/release/` | workflow-centric today; cista owner |
| Container layout | linked context only | never the sole committed authority |

## 4. Escalation and exceptions

- **Stop-if (unresolved authority):** if production or recovery authority is
  not assignable for a release, that release is paused and routed — it is never
  executed around a silent default (CAMPAIGN "Stop Conditions"; `delivery-stage1.md`
  risks "Stop-if").
- **Exceptional immutability incidents** (security/legal takedown) require the
  withdraw/revocation role (§1) and an incident record — see
  `failure-recovery-matrix.md` §3.
- **Credential compromise** routes immediately to the operator + credential
  owner (revocation authority) — `failure-recovery-matrix.md` §4.

## 5. Decision ledger

| # | Decision | Marking | Evidence |
| --- | --- | --- | --- |
| OQ9 | Role table §1; production authority operator-owned; agents prepare/inspect/dry-run by default | **accepted** | B11 OQ9 (`needs-stage-1-decision`); CAMPAIGN "Authority And Durable Homes"; `delivery-stage1.md` write_scope |
| OQ9 | Agent delegation of tagger/publisher/promoter roles | **explicitly-deferred-with-owner** (operator; per-release explicit authorization only) | CAMPAIGN "Authority And Durable Homes" |

## 6. References

- `release-contract.md` — contract context; roles cited by the ledger.
- `failure-recovery-matrix.md` — withdrawal/revocation/supersede semantics.
- `process-local-first.md` — where each role acts in the flow.
- CAMPAIGN "Authority And Durable Homes" — source of the role list.
