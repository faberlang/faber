#!/usr/bin/env python3
"""Build and run the Faber swarm-decimal port on each host target and diff it,
byte for byte, against the Python reference's golden files.

usage: compare.py [--faber PATH] [--targets runner,rust,go,device-cpu-1,device-metal,...] [--ticks N]
                  [--ticks-for runner=60,python=100] [--timeout SECONDS] [--keep] [--json PATH]

CPU worker-count legs use the label `device-cpu-N` (for example, `device-cpu-1`).
The worker count is recorded in JSON; `--threads N` is passed only to CPU device runs.

The program (src/main.fab) prints marked sections from one run:

  == hashes         reference/golden/hashes.txt        ("tick sha256", ticks 0..TICKS)
  == summary        reference/golden/summary.csv
  == snapshot_0     reference/golden/snapshot_0.csv    (likewise _100, _500)

Every section is compared with no tolerance. By default each target runs the full
500 ticks. `--ticks N` (all targets) or `--ticks-for target=N,...` rewrites the one
`const int TICKS` line in a temporary copy, for a target too slow to run 500 ticks;
the golden files are then cut to the same prefix (hash lines 0..N, summary rows with
tick <= N, snapshots with tick <= N) and the result is reported as "prefix N", never
as a full match.

Everything runs in temporary copies, so no build product lands in the tree. Run it
where `faber` is built (the studio, through studio-exec). Standard library only;
needs the toolchain of each target on PATH (node and tsc, go, cargo, python3, clang).
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
GOLD = os.path.join(HERE, "reference", "golden")
NAME = "swarm-decimal"
FULL_TICKS = 500
TARGETS = ["runner", "rust", "go", "ts", "python", "llvm-host", "clif", "wasm"]
# `device-reference|cpu|metal` run the device package (`device/`) with `faber run --device`;
# `device-cpu-N` selects an explicit CPU worker count. These are not in the default set.
SNAPS = (0, 100, 500)


def sh(cmd, cwd, timeout):
    t0 = time.perf_counter()
    try:
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr, time.perf_counter() - t0
    except subprocess.TimeoutExpired:
        return 124, b"", b"timeout after %ds" % timeout, time.perf_counter() - t0


def first_error(text):
    """The first meaningful diagnostic lines of a tool's stderr."""
    if isinstance(text, bytes):
        text = text.decode("utf-8", errors="replace")
    keep = []
    for line in text.splitlines():
        t = line.strip()
        if not t or t.startswith(("|", "warning", "Compiling", "Finished")) or "failed for '" in t:
            continue
        keep.append(t)
        if len(keep) == 2:
            break
    return " / ".join(keep)[:300] if keep else "(no message)"


def read(name):
    with open(os.path.join(GOLD, name), "rb") as f:
        return f.read()


def raw_lines(data):
    """Split only on LF, retaining every byte in each line."""
    if not data:
        return []
    lines = data.split(b"\n")
    out = [line + b"\n" for line in lines[:-1]]
    if lines[-1]:
        out.append(lines[-1])
    return out


def expected_sections(ticks):
    """The golden files cut to ticks 0..ticks (all of them when ticks is 500)."""
    exp = {}
    exp["hashes"] = b"".join(raw_lines(read("hashes.txt"))[: ticks + 1])
    rows = raw_lines(read("summary.csv"))
    exp["summary"] = rows[0] + b"".join(r for r in rows[1:] if int(r.split(b",", 1)[0]) <= ticks)
    for t in SNAPS:
        exp["snapshot_%d" % t] = read("snapshot_%d.csv" % t) if t <= ticks else b""
    return exp


def make_copy(tmp, ticks, device=False):
    d = os.path.join(tmp, "pkg")
    root = os.path.join(HERE, "device") if device else HERE
    shutil.copytree(os.path.join(root, "src"), os.path.join(d, "src"))
    shutil.copy(os.path.join(root, "faber.toml"), d)
    p = os.path.join(d, "src", "main.fab")
    text = open(p, encoding="utf-8").read()
    text, n = re.subn(r"^const int TICKS = \d+$", "const int TICKS = %d" % ticks, text, flags=re.M)
    assert n == 1, "const int TICKS line not found"
    with open(p, "w", encoding="utf-8") as h:
        h.write(text)
    return d


def execute(faber, target, d, tmp, timeout, threads=None):
    """Build and run one target. Returns (stage, stdout, message, build_s, run_s); stage is
    'ok', 'unsupported', 'build' or 'run'."""
    out = os.path.join(tmp, "out")
    single = os.path.join(d, "src", "main.fab")
    if target.startswith("device-"):
        # step 3a: the device package (`device/`), two `@ kernel` lane loops, on a device
        if target == "device-cpu" and threads is not None:
            cmd = [faber, "run", "--device", "cpu", "--threads", str(threads), d]
        else:
            cmd = [faber, "run", "--device", target[len("device-"):], d]
        rc, so, se, t = sh(cmd, tmp, timeout)
        return ("ok", so, "", 0.0, t) if rc == 0 else ("run", so, first_error(se), 0.0, t)
    if target == "runner":
        rc, so, se, t = sh([faber, "run", d], tmp, timeout)
        return ("ok", so, "", 0.0, t) if rc == 0 else ("run", so, first_error(se), 0.0, t)
    if target in ("ts", "go", "llvm-host"):
        rc, so, se, t = sh([faber, "run", "--target", target, d], tmp, timeout)
        return ("ok", so, "", 0.0, t) if rc == 0 else ("build", so, first_error(se) if se.strip() else "exit %d, no message" % rc, t, 0.0)
    if target in ("rust", "clif"):
        # clif has no package build (`faber targets`): it takes the single file
        src = d if target == "rust" else single
        rc, so, se, tb = sh([faber, "build", "-t", target, "-o", out, src], tmp, timeout)
        if rc != 0:
            return ("build", "", first_error(se), tb, 0.0)
        exe = os.path.join(d, "target", "debug", NAME) if target == "rust" else os.path.join(out, "main")
        if not os.path.exists(exe):
            return ("build", "", "build reported success but %s is missing" % exe, tb, 0.0)
        rc, so, se, tr = sh([exe], tmp, timeout)
        return ("ok", so, "", tb, tr) if rc == 0 else ("run", so, first_error(se), tb, tr)
    if target == "python":
        rc, so, se, tb = sh([faber, "build", "-t", "python", single, "-o", out], tmp, timeout)
        if rc != 0:
            return ("build", "", first_error(se), tb, 0.0)
        rc, so, se, tr = sh([sys.executable, os.path.join(out, "main.py")], tmp, timeout)
        return ("ok", so, "", tb, tr) if rc == 0 else ("run", so, first_error(se), tb, tr)
    if target == "wasm":
        rc, so, se, tb = sh([faber, "build", "-t", "wasm", "-o", out, d], tmp, timeout)
        if rc != 0:
            return ("build", "", first_error(se), tb, 0.0)
        return ("build", "", "built; no host here runs the module (not yet wired)", tb, 0.0)
    return ("unsupported", "", "unknown target", 0.0, 0.0)


def sections(data):
    """Extract marked sections while leaving their contents byte-for-byte intact."""
    markers = list(re.finditer(rb"(?m)^== ([A-Za-z0-9_]+)\r?(?=\n|\Z)", data))
    got = {}
    for i, marker in enumerate(markers):
        start = marker.end()
        if data[start:start + 1] == b"\n":
            start += 1
        end = markers[i + 1].start() if i + 1 < len(markers) else len(data)
        got[marker.group(1).decode("ascii")] = data[start:end]
    return got


def diff_row(want, got):
    wl, gl = raw_lines(want), raw_lines(got)
    for i in range(max(len(wl), len(gl))):
        w = wl[i] if i < len(wl) else None
        g = gl[i] if i < len(gl) else None
        if w != g:
            show = lambda line: "<missing>" if line is None else repr(line.decode("utf-8", errors="backslashreplace"))[1:-1]
            return i, show(w), show(g)
    return None


def compare_sections(expected, output):
    """Return per-section matches, byte diffs, or missing-section diagnostics."""
    got = sections(output)
    result = {}
    for name, want in expected.items():
        if name not in got:
            result[name] = "missing"
            continue
        diff = diff_row(want, got[name])
        result[name] = "match" if diff is None else {"line": diff[0], "want": diff[1], "got": diff[2]}
    return result


def parse_leg(label):
    """Return (execution target, CPU worker count) for a displayed leg label."""
    m = re.fullmatch(r"device-cpu-([0-9]+)", label)
    if m:
        count = int(m.group(1))
        if count < 1:
            raise ValueError("CPU worker count must be positive: %s" % label)
        return "device-cpu", count
    return label, None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--faber", default=os.path.join(HERE, "..", "..", "target", "debug", "faber"))
    ap.add_argument("--targets", default=",".join(TARGETS))
    ap.add_argument("--ticks", type=int, default=FULL_TICKS)
    ap.add_argument("--ticks-for", default="")
    ap.add_argument("--timeout", type=int, default=1500)
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--json")
    a = ap.parse_args()
    faber = os.path.abspath(a.faber)
    labels = a.targets.split(",")
    if not all(labels):
        ap.error("target labels cannot be empty")
    if len(labels) != len(set(labels)):
        ap.error("each target leg must have a unique label")
    try:
        configs = [parse_leg(label) for label in labels]
    except ValueError as exc:
        ap.error(str(exc))
    per = {k: int(v) for k, v in (kv.split("=") for kv in a.ticks_for.split(",") if kv)}
    tmp = tempfile.mkdtemp(prefix="swarm-compare-")
    rows = []
    try:
        for label, (target, threads) in zip(labels, configs):
            ticks = per.get(label, a.ticks)
            exp = expected_sections(ticks)
            tt = os.path.join(tmp, label)
            os.makedirs(tt)
            d = make_copy(tt, ticks, device=target.startswith("device-"))
            stage, so, msg, tb, tr = execute(faber, target, d, tt, a.timeout, threads)
            row = {"label": label, "target": target, "worker_count": threads,
                   "ticks": ticks, "stage": stage, "message": msg,
                   "build_s": round(tb, 1), "run_s": round(tr, 1)}
            if stage == "ok":
                row.update(compare_sections(exp, so))
            rows.append(row)
            if stage == "ok":
                parts = []
                for k in exp:
                    v = row.get(k)
                    parts.append("%s=%s" % (k, "match" if v == "match" else "DIFF@%d" % v["line"] if isinstance(v, dict) else v))
                state = ", ".join(parts)
            else:
                state = "%s: %s" % (stage, msg)
            tag = "" if ticks == FULL_TICKS else " [prefix %d ticks]" % ticks
            print("%-16s build %6.1fs run %6.1fs  %s%s" % (label, tb, tr, state, tag), flush=True)
            for k in exp:
                v = row.get(k)
                if isinstance(v, dict):
                    print("    %s line %d\n      want %s\n      got  %s" % (k, v["line"], v["want"][:200], v["got"][:200]))
    finally:
        if a.keep:
            print("kept", tmp)
        else:
            shutil.rmtree(tmp, ignore_errors=True)
    if a.json:
        with open(a.json, "w", encoding="utf-8") as h:
            json.dump(rows, h, indent=1)
    keys = ["hashes", "summary"] + ["snapshot_%d" % t for t in SNAPS]
    bad = [r for r in rows if r["stage"] != "ok" or any(r.get(k) != "match" for k in keys)]
    full = [r for r in rows if r["stage"] == "ok" and r["ticks"] == FULL_TICKS and all(r.get(k) == "match" for k in keys)]
    print("%d of %d targets match byte for byte over all %d ticks" % (len(full), len(rows), FULL_TICKS))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
