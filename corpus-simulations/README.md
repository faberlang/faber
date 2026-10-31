# Corpus simulations

Complete Faber workloads with small, self-contained smokes and the compact
reference oracles used to check their outputs. Benchmark harnesses, receipts and
larger reference data stay under `acceptance/` and are not part of the public
export.

| Simulation | Distinctive work | Routes shown |
| --- | --- | --- |
| [fusion-containment](fusion-containment/) | One Latin Faber source for particle orbits, compared on the reference route, multicore CPU and Metal, with a partition audit | reference, cpu, metal |
| [drone-mission](drone-mission/) | Branchy aircraft mission evaluation: each GPU lane flies a whole mission | reference, cpu, metal |
| [swarm-decimal](swarm-decimal/) | Deterministic many-actor simulation with fixed-point decimal arithmetic | see its README |
| [h264-encoder](h264-encoder/) | A constrained-baseline H.264 encoder on native multicore CPU work; no GPU acceleration is claimed | cpu |

Each directory has an English README and a `PROVENANCE.md`. Device execution and
the native Rust emitter are in the released Faber binary, not the open-source
build; each README gives its exact route requirements.
