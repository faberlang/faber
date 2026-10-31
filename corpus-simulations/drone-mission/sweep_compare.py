#!/usr/bin/env python3
"""Step 3 comparator: the Faber device sweep (src/sweep.fab) against the references.

  python3 sweep_compare.py grid.txt [bisect.txt] [--torch-device cpu|mps|cuda]
  python3 sweep_compare.py --identical a.txt b.txt        # scenario + summary sections byte-equal

`grid.txt` is the output of `faber run --device <d> src/sweep.fab -- 0 <reps>`, `bisect.txt`
of `... -- 1 1`. Against the scalar float64 reference (reference/drone.py) it checks:
  * the per-scenario pass flag, the mission error codes, and the 12 search answers (exact);
  * the four statistics (takeoff margin, peak current, landing SoC, motor-out margin) against the
    scalar float64 reference: 1e-5 relative for all but SoC, which is 2e-6 absolute. Nonfinite
    values on completed missions fail verification.
With --torch-device it also runs reference/sweep_torch.py in the dtype that device uses
(float32 on cpu/mps here, to compare the like precision; float64 only on request via
--torch-dtype float64) and reports the same numbers against it. The fixed 972-row scenario grid,
unique ids, input tuples, flags, error codes, statistics and 12 search answers are required; exit
status 1 on any mismatch.
"""
import argparse
import csv
import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "reference"))

import drone  # noqa: E402
import scenarios as S  # noqa: E402

CODES = {"pack_collapse": 1, "pack_empty": 2, "vortex_ring": 3, "step_limit": 4}
STATS = ["takeoff_margin", "max_current", "final_soc", "min_margin_fail"]
STAT_LIMITS = {
    "takeoff_margin": ("relative", 1e-5),
    "max_current": ("relative", 1e-5),
    "final_soc": ("absolute", 2e-6),
    "min_margin_fail": ("relative", 1e-5),
}
SCENARIO_HEADER = ["scenario", "aircraft", "payload_kg", "distance_m", "ok", "err_nom", "err_fail",
                   "takeoff_margin", "max_current", "final_soc", "min_margin_fail"]


def expected_scenario_inputs():
    """Return the fixed 972 (aircraft, payload, distance) tuples in scenario-id order."""
    payloads = [i * S.PAYLOAD_RES_KG for i in range(int(S.PAYLOAD_CAP_KG / S.PAYLOAD_RES_KG) + 1)]
    distances = [i * S.RANGE_RES_M for i in range(int(S.RANGE_CAP_M / S.RANGE_RES_M) + 1)]
    expected = []
    for aircraft in S.AIRCRAFT_ORDER:
        expected.extend((aircraft, payload, distance) for distance in S.SUMMARY_DISTANCES_M for payload in payloads)
        expected.extend((aircraft, payload, distance) for payload in S.SUMMARY_PAYLOADS_KG for distance in distances)
    if len(expected) != 972:
        raise ValueError("internal scenario definition has %d rows, expected 972" % len(expected))
    return expected


def validate_scenarios(rows, expected=None):
    """Reject incomplete, duplicated, or mis-keyed scenario grids."""
    expected = expected_scenario_inputs() if expected is None else expected
    if len(rows) != len(expected):
        raise ValueError("expected %d scenarios, got %d" % (len(expected), len(rows)))
    ids = [r["i"] for r in rows]
    if len(set(ids)) != len(ids):
        raise ValueError("duplicate scenario id")
    if set(ids) != set(range(len(expected))):
        raise ValueError("scenario ids must be exactly 0 through %d" % (len(expected) - 1))
    by_id = {r["i"]: r for r in rows}
    for i, (aircraft, payload, distance) in enumerate(expected):
        row = by_id[i]
        actual = (row["aircraft"], row["payload"], row["dist"])
        wanted = (aircraft, payload, distance)
        if actual != wanted:
            raise ValueError("scenario %d input mismatch: expected %r, got %r" % (i, wanted, actual))
        if row["ok"] not in (0, 1):
            raise ValueError("scenario %d has invalid pass flag %r" % (i, row["ok"]))


def parse_scenarios(lines):
    if not lines:
        raise ValueError("missing scenarios section")
    reader = csv.reader(lines)
    head = next(reader)
    if head != SCENARIO_HEADER:
        raise ValueError("unexpected scenarios header: %r" % (head,))
    rows = []
    for line_number, f in enumerate(reader, 2):
        if len(f) != len(SCENARIO_HEADER):
            raise ValueError("scenario CSV line %d has %d columns, expected %d" %
                             (line_number, len(f), len(SCENARIO_HEADER)))
        try:
            rows.append({"i": int(f[0]), "aircraft": f[1], "payload": float(f[2]), "dist": float(f[3]),
                         "ok": int(f[4]), "err_nom": int(f[5]), "err_fail": int(f[6]),
                         "takeoff_margin": float(f[7]), "max_current": float(f[8]),
                         "final_soc": float(f[9]), "min_margin_fail": float(f[10])})
        except ValueError as exc:
            raise ValueError("invalid scenario CSV line %d: %s" % (line_number, exc)) from exc
    return rows


def sections(path):
    out, cur = {}, None
    for line in open(path, encoding="utf-8").read().splitlines():
        if line.startswith("== "):
            cur = line[3:].strip()
            out[cur] = []
        elif cur is not None and line.strip():
            out[cur].append(line)
    return out


def scalar_rows(rows):
    """The float64 scalar reference for every scenario of the Faber grid."""
    out = []
    for r in rows:
        ac = S.AIRCRAFT[r["aircraft"]]
        ref = {"i": r["i"], "ok": 1 if drone.passes(ac, S.MISSION, r["payload"], r["dist"]) else 0,
               "err_nom": 0, "err_fail": 0}
        try:
            n = drone.fly(ac, S.MISSION, r["payload"], r["dist"], None)
            ref.update(takeoff_margin=n["takeoff_margin"], max_current=n["max_current"], final_soc=n["final_soc"])
        except drone.MissionError as e:
            ref["err_nom"] = CODES[str(e)]
        try:
            f = drone.fly(ac, S.MISSION, r["payload"], r["dist"], S.FAIL_T_S)
            ref["min_margin_fail"] = f["min_margin_fail"]
        except drone.MissionError as e:
            ref["err_fail"] = CODES[str(e)]
        out.append(ref)
    return out


def torch_rows(rows, device, dtype_name):
    import torch
    import sweep_torch as T
    dt = getattr(torch, dtype_name)
    prm = T.make_params(device, dt)
    ac = torch.tensor([0 if r["aircraft"] == "quad" else 1 for r in rows], device=device)
    pay = torch.tensor([r["payload"] for r in rows], dtype=dt, device=device)
    dist = torch.tensor([r["dist"] for r in rows], dtype=dt, device=device)
    ok, st = T.evaluate_scenarios(ac, pay, dist, device, dt, prm)
    ok = ok.cpu().tolist()
    st = {k: v.cpu().double().tolist() for k, v in st.items()}
    out = []
    for i in range(len(rows)):
        out.append({"i": rows[i]["i"], "ok": 1 if ok[i] else 0, "err_nom": int(st["err_nom"][i]), "err_fail": int(st["err_fail"][i]),
                    "takeoff_margin": st["takeoff_margin"][i], "max_current": st["max_current"][i],
                    "final_soc": st["final_soc"][i], "min_margin_fail": st["min_margin_fail"][i]})
    return out


def compare(name, rows, ref):
    if len(rows) != len(ref):
        print("vs %s: row count differs (%d vs %d)" % (name, len(rows), len(ref)))
        return 1
    try:
        row_ids = [r["i"] for r in rows]
        ref_ids = [r["i"] for r in ref]
        if len(set(row_ids)) != len(row_ids) or len(set(ref_ids)) != len(ref_ids):
            raise ValueError("duplicate scenario id")
        if set(row_ids) != set(ref_ids):
            raise ValueError("scenario ids differ")
        rows_by_id = {r["i"]: r for r in rows}
        ref_by_id = {r["i"]: r for r in ref}
    except (KeyError, TypeError, ValueError) as exc:
        print("vs %s: invalid scenario ids: %s" % (name, exc))
        return 1
    scenario_ids = sorted(row_ids)
    rows = [rows_by_id[i] for i in scenario_ids]
    ref = [ref_by_id[i] for i in scenario_ids]
    flips = [r["i"] for r, q in zip(rows, ref) if r["ok"] != q["ok"]]
    ecode = [r["i"] for r, q in zip(rows, ref) if (r["err_nom"], r["err_fail"]) != (q["err_nom"], q["err_fail"])]
    print("vs %s: %d scenarios; pass/fail flips %d; error-code differences %d" % (name, len(rows), len(flips), len(ecode)))
    if flips:
        print("  flipped scenarios:", flips[:20], "..." if len(flips) > 20 else "")
    if ecode:
        print("  error-code differing scenarios:", ecode[:20])
    bad_stats = 0
    for k in STATS:
        mode, limit = STAT_LIMITS[k]
        worst_rel, worst_abs, n, exact, nonfinite = 0.0, 0.0, 0, 0, 0
        for r, q in zip(rows, ref):
            if r["err_nom"] or r["err_fail"] or q["err_nom"] or q["err_fail"]:
                continue
            x, y = r[k], q[k]
            if not (math.isfinite(x) and math.isfinite(y)):
                nonfinite += 1
                continue
            n += 1
            delta = abs(x - y)
            rel = delta / max(abs(y), 1e-300)
            exact += delta == 0.0
            worst_abs = max(worst_abs, delta)
            worst_rel = max(worst_rel, rel)
        measured = worst_abs if mode == "absolute" else worst_rel
        failed = nonfinite > 0 or n == 0 or measured > limit
        bad_stats += int(failed)
        print("  %-16s max rel diff %.3e (max abs %.3e) over %d completed scenarios (%d bit-equal); "
              "nonfinite=%d %s_limit=%.1e%s" %
              (k, worst_rel, worst_abs, n, exact, nonfinite, mode, limit, " FAIL" if failed else ""))
    return len(flips) + len(ecode) + bad_stats


def check_summary(path_lines, label):
    want = open(os.path.join(HERE, "reference", "golden", "summary.csv"), encoding="utf-8").read().splitlines()
    got = path_lines
    bad = [(w, g) for w, g in zip(want, got) if w != g]
    if len(want) != len(got):
        bad.append(("rows %d" % len(want), "rows %d" % len(got)))
    print("%s: 12 search answers vs golden summary.csv (scalar float64): %d differences" % (label, len(bad)))
    for w, g in bad:
        print("  want", w, "\n  got ", g)
    return len(bad)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("files", nargs="*")
    ap.add_argument("--identical", nargs=2, metavar=("A", "B"))
    ap.add_argument("--versus", nargs=2, metavar=("A", "B"), help="two Faber grid outputs: flips, error codes, bit-equal counts, max rel diff")
    ap.add_argument("--torch-device", default=None)
    ap.add_argument("--torch-dtype", default="float32")
    a = ap.parse_args()
    if a.identical:
        sa, sb = sections(a.identical[0]), sections(a.identical[1])
        try:
            required = {"scenarios", "summary"}
            for path, found in zip(a.identical, (sa, sb)):
                missing = required - found.keys()
                if missing:
                    raise ValueError("%s missing required section(s): %s" %
                                     (path, ", ".join(sorted(missing))))
            for section in (sa["scenarios"], sb["scenarios"]):
                validate_scenarios(parse_scenarios(section))
        except (ValueError, TypeError, IndexError, AssertionError) as exc:
            print("invalid scenario grid: %s" % exc, file=sys.stderr)
            return 1
        diff = [k for k in ("scenarios", "summary") if sa[k] != sb[k]]
        print("identical" if not diff else "DIFFERENT sections: %s" % diff)
        return 1 if diff else 0
    if a.versus:
        try:
            ra = parse_scenarios(sections(a.versus[0])["scenarios"])
            rb = parse_scenarios(sections(a.versus[1])["scenarios"])
            validate_scenarios(ra)
            validate_scenarios(rb)
        except (KeyError, ValueError, IndexError, AssertionError) as exc:
            print("invalid scenario grid: %s" % exc, file=sys.stderr)
            return 1
        return 1 if compare("%s" % a.versus[1], ra, rb) else 0
    bad = 0
    try:
        grid = sections(a.files[0])
        rows = parse_scenarios(grid["scenarios"])
        validate_scenarios(rows)
    except (IndexError, KeyError, ValueError) as exc:
        print("invalid scenario grid: %s" % exc, file=sys.stderr)
        return 1
    ref = scalar_rows(rows)
    bad += compare("scalar float64 (drone.py)", rows, ref)
    bad += check_summary(grid["summary"], "grid maxima")
    if a.torch_device:
        tr = torch_rows(rows, a.torch_device, a.torch_dtype)
        compare("torch %s %s" % (a.torch_device, a.torch_dtype), rows, tr)
        print("context: the torch run itself")
        compare("scalar float64", tr, ref)
    if len(a.files) > 1:
        bad += check_summary(sections(a.files[1])["summary"], "batched bisection")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
