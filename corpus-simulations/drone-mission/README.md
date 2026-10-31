# Drone mission

`src/` is a Faber package for evaluating delivery missions of two invented
multirotor aircraft. It models battery voltage and charge, rotor power and
thrust margin, a ten-phase delivery-and-return mission, rotor failure, and
payload/range searches. `src/sweep.fab` adds the device-facing experiment: one
GPU lane evaluates one scenario, including its nominal and motor-out missions.
The kernel's per-lane mission loop is branchy and divergent, unlike a uniform
matrix operation.

The deterministic scalar model and its small fixed trace/summary oracles are in
`reference/`. The one-line route smoke is in `smoke/`; it exercises a whole-
tensor kernel without running the large mission sweep.

## Same sweep source on each route

All commands below use **the same** `src/sweep.fab` and arguments. Mode `0`
runs a small launch of the 972-scenario grid; mode `1` runs the batched
bisection. The small smoke can instead be used to check expected output quickly:

```sh
faber check corpus-simulations/drone-mission
faber run --device reference corpus-simulations/drone-mission/smoke
faber run --device cpu --threads 2 corpus-simulations/drone-mission/smoke
```

For the measured sweep, repeat the same source and arguments for each route:

| Route | Command |
| --- | --- |
| Reference | `faber run --device reference corpus-simulations/drone-mission/src/sweep.fab -- 0 1` |
| CPU (2 workers) | `faber run --device cpu --threads 2 corpus-simulations/drone-mission/src/sweep.fab -- 0 1` |
| Metal | `faber run --device metal corpus-simulations/drone-mission/src/sweep.fab -- 0 1` |
| CUDA | `faber run --device cuda corpus-simulations/drone-mission/src/sweep.fab -- 0 1` |

The sweep uses whole-tensor kernels, so Metal and CUDA are supported by the
released device implementation. These runs have been compared against the
float64 scalar model; the CPU run is the exact f32 kernel oracle and GPU
statistics use the stated tolerance in the retained acceptance receipts. No
partitioned run is claimed here.

**Device routes require the released `faber` binary.** The open-source build
excludes the device route implementation. The reference interpreter is not a
device route and does not need the released binary.

## Recorded device timings

Median of five warm repeats for the same sweep workload (grid and 12-search
bisection); first-launch costs are excluded. These are receipt values, not
benchmarks from this checkout. The receipts remain in `acceptance/` and are not
part of the public export.

| Route | Grid | Bisection | Date and hardware | Receipt |
| --- | ---: | ---: | --- | --- |
| CUDA, Faber | 5.361 ms | 36.657 ms | 2026-10-28, RunPod NVIDIA RTX 4090 | `acceptance/drone-mission/receipts/bench-2026-10-28/cuda-a/receipt.md` |
| Metal, Faber | 7.364 ms | 49.645 ms | 2026-10-30, Apple M5 Max (32 GPU cores) | `acceptance/drone-mission/receipts/bench-2026-10-30/metal/receipt.md` |

The receipts do not preserve comparable reference or CPU timer samples, so no
timing is stated for those routes.

## Checks and expected output

```sh
python3 -m unittest discover -s corpus-simulations/drone-mission/reference
python3 corpus-simulations/drone-mission/compare.py --faber /path/to/faber --targets runner --variants package
```

The package's `main.fab` produces the full trace, payload/range summary, and
rule table. `compare.py` compares those sections byte for byte with the moved
oracles. The GPU sweep driver is `sweep_compare.py`; its timing harnesses and
receipts remain in `acceptance/drone-mission/`.
