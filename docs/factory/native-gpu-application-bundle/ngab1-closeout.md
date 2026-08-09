# NGAB1 — Phase Closeout (gate): stage status, README regen, audit 0 findings

**Campaign**: native-gpu-application-bundle (NGAB1 — Radix host/device
partition and callable device boundary)
**Unit**: NGAB1 phase closeout (delivery spec
[`ngab1-delivery.md`](ngab1-delivery.md) §Phase gate, GATE)
**Authority**: [`CAMPAIGN.md`](CAMPAIGN.md) §NGAB1 (gate, overlap rule, batch
posture) + NGAB0-R1 amendment (`ngab0-major-revision-ddcp0.md`), the DDPP0
contract §ChildRouting (`faber/docs/factory/direct-device-product-pipeline/ddpp0-contract.md`),
and the paired DDCP contract (DDCP0 closeout, `radix/docs/factory/direct-device-compilation-pipeline/ddcp0-closeout.md`).
**Predecessors**: NGAB1-U1 `0bb5ebbd6`/`754a8e6`, NGAB1-U2 `3f8541bcb`/`93393cd`,
NGAB1-U3 `3001fdd90`, NGAB1-U4 `cb92b4f11`/`4435068` (all LANDED).
**Status**: NGAB1 accepted (2026-08-09)

## 1. Phase gate checklist

| # | Gate item | Evidence | Verdict |
| --- | --- | --- | --- |
| 1 | **U1 — one analyzed package produces validated host MIR/LLVM AND a typed device program** (minimal one-prepared-region vertical slice) | NGAB0-U11 fixture proven by U1 (`0bb5ebbd6` radix: typed `HostPartition`/`DeviceProgram` derivation + host-lane emission; `754a8e6` faber: host-partition wire + llvm-host lane + fixture test); the prepared region carries the kernel and its typed device facts — typed, not text-parsed | **PASSED** |
| 2 | **U2 — versioned call ABI + compile-time rejection** | Versioned host→device call ABI landed (`3f8541bcb` radix boundary rejection; `93393cd` faber diagnostics + negative fixtures); invalid cross-boundary values fail at compile time with typed diagnostics; a negative fixture proves rejection without a launch | **PASSED** |
| 3 | **U3 — resource/lifetime/mutation/observation facts survive lowering** | `3001fdd90` — facts from the semantic program survive to the device program and the ABI; no fact dropped or re-derived from text | **PASSED** |
| 4 | **U4 — batch compatible call shapes, one region per call** | `cb92b4f11` radix red-green proofs + `4435068` faber seam test — two-kernel composition batches as multiple kernels per prepared submission region; one host call → one region, never per-kernel ABI rows (NGAB0-R1 granularity); no per-shape special-casing in the ABI | **PASSED** |
| 5 | **NGAB1 stage status line machine-parseable** | `CAMPAIGN.md` §NGAB1 `**Status**: accepted — NGAB1-U4 phase closeout (2026-08-09); NGAB2 next after DDCP1/DDCP2 per the DDPP3 absorption + paired DDCP contract` | **PASSED** |
| 6 | **Faber + radix factory READMEs regenerated** | faber: `python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check` fresh. radix: `python3 scripta/generate-factory-readme.py --check` fresh | **PASSED** |
| 7 | **Goal-status audit 0 findings** | `./scripta/check-factory-goal-status` exit 0 (faber and radix) | **PASSED** |

## 2. Validation run (this closeout, 2026-08-09, no cargo)

```bash
grep -n '^\*\*Status\*\*' docs/factory/native-gpu-application-bundle/CAMPAIGN.md   # NGAB1 accepted line
cd faber && python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check   # exit 0
cd faber && ./scripta/check-factory-goal-status                                        # exit 0 (16 goals, no drift)
cd radix && python3 scripta/generate-factory-readme.py --check                        # exit 0
cd radix && ./scripta/check-factory-goal-status                                       # exit 0 (146 goals, no drift)
git diff --check                                                                       # clean (both repos)
```

## 3. Residuals (owned by later stages, not NGAB1)

- **NGAB2 is next after DDCP1/DDCP2** per the paired DDCP contract and the
  DDPP3 absorption: NGAB1–NGAB4 lower and implement as **DDPP3 child
  packets** (DDPP0 contract §ChildRouting, council H1); DDCP1/DDCP2 have READY
  delivery-sized slices (`ddcp0-closeout.md`).
- NGAB0 operator gates (llvm-host identity, MSL source-first, PTX arch set)
  remain open as recorded in NGAB0 §Admission — defaults hold until the named
  operator decision closes.
- No new unresolved ABI or wire questions were raised by U1–U4; any future
  missing wire fact routes per `ngab1-delivery.md` §Open Questions (extend the
  typed schema, never text).
