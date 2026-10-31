# Provenance

- `faber.toml` and `src/` are the locally authored Faber package and device sweep, moved from `acceptance/drone-mission/` without source edits. Aircraft, mission, rules, and input values are invented; no real aircraft or mission data is used.
- `reference/drone.py`, `reference/scenarios.py`, and `reference/test_drone.py` are the locally authored standard-library scalar model and its tests. `reference/golden/` contains the small trace and summary oracles generated from that model.
- `compare.py` and `sweep_compare.py` are local verification drivers for the package and sweep.
- `smoke/` is a new, tiny exact-output whole-tensor kernel fixture for exercising the reference and CPU device routes.
- `README.md` describes the sources and routes. PyTorch/CUDA benchmark harnesses, receipts, gap notes, and diagnostic probes remain in `acceptance/drone-mission/` and are not shipped here.
