# EL-5 bisect proof — rust lane (2026-08-11)

Companion to `bisect-proof-20260811.md` (roundtrip lane). Runs on pharos in
the runner checkout `/home/ianzepp/work/lane-grid` (faber at origin/main
`4410b44`), scratch branch `grid-bisect-demo`.

Injected break (commit `57025c3`, one commit on top of `4410b44`): added a
stale expected-failure row — `incipit/incipit.fab` — to
`crates/exempla/src/exempla_e2e/expectations/rust.rs`. `incipit/incipit.fab`
is the language exemplum and compiles cleanly, so the row trips the
`stale_known_failures` assertion (`rust.rs:523`) and the rust lane turns red.

Range: good = `1220158` (10 first-parent commits below the injected commit),
bad = `57025c3` — 11 candidates in `good..bad`.

Command:

```
lane-grid-bisect --lane rust --good 122015852cca63f983f763e2bdaac191a38e687b \
  --bad 57025c3437ae5928663a215a6d50a21203796c76 \
  --root /home/ianzepp/work/lane-grid
```

Result (5 lane-only runs, budget 10 — required ≤7):

```
  run 1: 122015852cca -> green  (test result: ok ...)
  run 2: 57025c3437ae -> red    (test result: FAILED ...)
  run 3: 309fb3dc915e -> green  (test result: ok ...)
  run 4: b3cc928be422 -> green  (test result: ok ...)
  run 5: 4410b441986c -> green  (test result: ok ...)
first red commit: 57025c3437ae5928663a215a6d50a21203796c76
  (chore(grid-bisect-demo): inject stale rust known-failure row (incipit/incipit.fab))
lane-only runs used: 5 (budget 10)
```

The bisect localized the injected break to the exact commit in 5 lane-only
runs (each run is the single rust lane command — never the whole grid).

This run also exercised the bisect's adjacency-termination fix: the first
attempt (same range, pre-fix script) re-classified the collapse candidate
repeatedly and burned the full budget; `lane-grid-bisect` now terminates at
`len(candidates) <= 1` (lo/hi adjacent) and probes strict interior commits,
so a single-candidate collapse is recognized as the first red immediately.
