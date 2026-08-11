# EL-5 bisect proof — lane-scoped localization (2026-08-11)

Run on pharos in the runner checkout `/home/ianzepp/work/lane-grid-hand6`
(faber at origin/main `4410b44`), scratch branch `grid-bisect-proof`.

Injected break (commit `15e787f`, c3 of the proof branch): added a stale
expected-failure row — `salve-munde.fab` — to
`crates/exempla/src/exempla_e2e/expectations/roundtrip.rs`. `salve-munde.fab`
is asserted to pass by the roundtrip lane (`salve_ok`), so the stale row trips
the `unexpected_passes` assertion and the roundtrip lane turns red.

Branch layout (oldest first):

```
50babff c1 neutral scratch note
7a7bb3e c2 neutral scratch note
15e787f c3 INJECTED stale expected-failure row for roundtrip  ← the break
e30b58f c4 neutral scratch note
9a8a9f8 c5 neutral scratch note (tip)
```

Command:

```
lane-grid-bisect --lane roundtrip --good 4410b44 --bad 9a8a9f8 \
  --root /home/ianzepp/work/lane-grid-hand6
```

Result (4 lane-only runs, budget 10 — required ≤7):

```
  run 1: 4410b441986c -> green   (test result: ok ...)
  run 2: 9a8a9f8929ca -> red     (test result: FAILED ...)
  run 3: 15e787f17671 -> red     (test result: FAILED ...)
  run 4: 7a7bb3ea83d9 -> green   (test result: ok ...)
first red commit: 15e787f1767177cd81492243e7983546ae4d6336
  ([bisect-proof] c3: INJECTED stale expected-failure row for roundtrip)
lane-only runs used: 4 (budget 10)
```

The bisect localized the injected break to the exact commit in 4 lane-only
runs (each run is the single roundtrip lane command — never the whole grid).
Proof branch and receipts remain on pharos (`lane-grid-hand6`).
