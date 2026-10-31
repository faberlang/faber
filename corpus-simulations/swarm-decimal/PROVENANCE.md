# Provenance

- `faber.toml` and `src/` are the locally authored Faber host simulation; they were moved from `acceptance/swarm-decimal/` without source edits. The model, rules, constants, and seed are synthetic.
- `device/` is the locally authored lane-kernel variant of that simulation, moved from the same acceptance directory. It computes the decision phase on the device and retains the host phases.
- `reference/` contains the locally authored Python fixed-point reference, unit tests, and expected hash/summary/snapshot files generated from that reference. No donor code or data is included.
- `repros/` contains small, locally authored compiler regression examples associated with the implementation work; these are not copied from an external project.
- `smoke/` is a new, tiny exact-output kernel fixture for exercising the reference and CPU device routes.
- `README.md` describes the sources and routes. Benchmark harnesses, measurements, gap notes, and other exploratory material remain in `acceptance/swarm-decimal/` and are not shipped here.
