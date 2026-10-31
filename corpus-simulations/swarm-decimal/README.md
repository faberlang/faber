# Swarm decimal

This is a deterministic, invented ecosystem simulation: 800 prey, 150 predators,
and 50 regrowing food patches interact for 500 ticks. Each tick builds a spatial
index, makes independent per-entity decisions, resolves predator/food conflicts
in stable id order, integrates movement and energy, and performs births, deaths,
and order-preserving compaction. `d64` decimal arithmetic makes every stored
value and every state hash reproducible. The interesting device work is the
divergent neighbor search and decision step, not a regular matrix operation.

`src/` is the host Faber port. `device/` is its kernel variant: the same
simulation phases and outputs, with the prey and predator decisions expressed
as lane kernels. The Python reference, tests, and compact golden outputs are in
`reference/`; focused compiler reproductions are in `repros/`. The small route
smoke is a separate package under `smoke/`, so it checks the device toolchain
without running the full 500-tick simulation.

## Same device source on each route

Every route below runs the same `device/` package and the same 500-tick
simulation. Run the small route smoke first for a fast output check:

```sh
faber check corpus-simulations/swarm-decimal
faber check corpus-simulations/swarm-decimal/device
faber run --device reference corpus-simulations/swarm-decimal/smoke
faber run --device cpu --threads 2 corpus-simulations/swarm-decimal/smoke
```

| Route | Command |
| --- | --- |
| Reference | `faber run --device reference corpus-simulations/swarm-decimal/device` |
| CPU (8 workers) | `faber run --device cpu --threads 8 corpus-simulations/swarm-decimal/device` |
| Metal | `faber run --device metal corpus-simulations/swarm-decimal/device` |

`compare.py` can compare each of those routes against the five checked-in
output sections, byte for byte:

```sh
python3 corpus-simulations/swarm-decimal/compare.py \
  --faber /path/to/faber --targets device-reference,device-cpu-8,device-metal
```

**Device routes require the released `faber` binary.** The open-source build
excludes the device route implementation. The `reference` route is an oracle,
not a device route, and does not need the released binary. CUDA is not listed:
the simulation's device package uses lane-form kernels, which the released CUDA
route does not lower. No partitioned result is claimed.

## Timing record

No separate timing receipt is retained for this simulation in the repository,
so this README makes no timing claim. The table records that evidence status
rather than inferring timing values from prose notes.

| Route | Timing | Date / hardware | Receipt |
| --- | --- | --- | --- |
| Reference | not reported | — | no retained timing receipt |
| CPU device | not reported | — | no retained timing receipt |
| Metal | not reported | — | no retained timing receipt |

## Reference tests and output

```sh
python3 -m unittest discover -s corpus-simulations/swarm-decimal/reference
python3 corpus-simulations/swarm-decimal/compare.py \
  --faber /path/to/faber --targets device-reference
```

The output includes the state hash at every tick, summary rows, and snapshots at
ticks 0, 100, and 500. The hash covers every entity's id, state, position,
velocity, and energy. The comparison driver verifies every section without a
tolerance; it also accepts CPU and Metal device labels as shown above. Long-run
benchmark harnesses and measurement notes remain under `acceptance/`.
