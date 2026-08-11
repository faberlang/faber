# Per-Lane E2E Grid — Standing Status (Tier 1 report-only)

- **Updated**: 2026-08-11T16:26:26.029118+00:00
- **Run**: `grid-20260811T161941`
- **Target**: faber `4410b441` (origin/main)
- **Sibling pins**: radix `9a508551` cista `93552882` faber-runtime `5a525e32` hosts `6ff374b5` norma `f48e1013` examples `e824cf26`
- **Host**: pharos (Linux pharos 6.8.0-117-generic, 8 cores)
- **Grid root**: `/home/ianzepp/work/lane-grid`
- **Receipts**: `/home/ianzepp/work/lane-grid/receipts/grid-20260811T161941`

## Lane receipts

| Lane | Status | Exit | Duration | Summary |
| --- | --- | --- | --- | --- |
| go | red | 101 | 20.2s | test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 19 filtered out; finished in 19.75s |
| ts | green | 0 | 89.5s | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 23 filtered out; finished in 89.31s |
| wasm | red | 101 | 17.2s | test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 42 filtered out; finished in 17.04s |
| rust | green | 0 | 67.2s | test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 21 filtered out; finished in 66.99s |
| swift | red | 101 | 56.3s | test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 20 filtered out; finished in 56.14s |
| sexp | red | 101 | 144.4s | test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 16 filtered out; finished in 144.25s |
| llvm | red | 101 | 0.7s |  |
| metal | green | 0 | 0.2s | compile-gate lane: no dedicated exempla harness e2e for mir-metal; build+harness compile is the gate (0 tests ran) |
| mir | green | 0 | 3.1s | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 190 filtered out; finished in 2.88s |
| roundtrip | green | 0 | 6.1s | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 164 filtered out; finished in 5.94s |

## Legend

- `green` = lane command exit 0 (all lane e2e assertions held).
- `red` = lane command non-zero (unexpected failure or assertion break).
- `skipped` = host toolchain probe failed (missing tool on the grid host); **not** a pass.
- `error` = the runner could not execute the lane (infra/timeout).
- Tier 1 report-only: this file does not gate merges (goal stop condition 3).
