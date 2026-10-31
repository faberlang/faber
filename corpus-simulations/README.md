# Public simulation corpus

These examples pair complete Faber workloads with small, self-contained route
smokes and the compact reference oracles used to check their outputs. Their
benchmark harnesses, receipts, and issue-focused scratch material remain under
`acceptance/` and are not part of the public export.

| Simulation | Distinctive work | Runnable source |
| --- | --- | --- |
| [drone-mission](drone-mission/) | Branchy aircraft mission evaluation and batched, divergent GPU work | `src/sweep.fab` |
| [swarm-decimal](swarm-decimal/) | Deterministic many-actor simulation with fixed-point decimal arithmetic and a spatial index | `device/` |

Each directory has an English README and a `PROVENANCE.md`. Run the small
smokes with the released Faber binary as described in each README. Device
routes are not present in the open-source build.
