#!/usr/bin/env python3
"""Build and run the Faber drone-mission port on each host target and diff it,
byte for byte, against the Python reference.

usage: compare.py [--faber PATH] [--targets runner,rust,ts,...] [--variants package,flat]
                  [--timeout SECONDS] [--keep] [--json PATH]

The program prints three sections, each opened by a marker line (see
src/main.fab). Each section is compared with no tolerance:

  == trace    reference/golden/trace.csv
  == summary  reference/golden/summary.csv
  == rules    `python3 reference/drone.py rules`

Two variants per target:
  package  the package as written (src/data.fab, src/model.fab, src/main.fab)
  flat     the same three files concatenated into one module (imports and module
           qualifiers removed), built mechanically here. It exists because several
           targets cannot yet compile cross-module failable calls or constants
           (GAPS.md); it is not a second source of truth.

Everything runs in temporary copies, so no build product lands in the tree.
Run it where `faber` is built (the studio, through studio-exec). Standard library
only; needs the toolchain of each target on PATH (node and tsc, go, cargo, python3).
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
REF = os.path.join(HERE, "reference")
NAME = "drone-mission"
MARKERS = ["trace", "summary", "rules"]

# target -> (how to run, package build supported)
TARGETS = ["runner", "rust", "ts", "go", "python", "llvm-host", "clif", "wasm"]


def sh(cmd, cwd, timeout):
    try:
        p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout)
        return p.returncode, p.stdout, p.stderr
    except subprocess.TimeoutExpired:
        return 124, "", "timeout after %ds" % timeout


def first_error(text):
    """The first meaningful diagnostic lines of a tool's stderr (no source echo,
    no warnings, not the failed command line)."""
    keep = []
    for line in text.splitlines():
        t = line.strip()
        if not t or t.startswith(("|", "warning", "Compiling", "Finished")) or "failed for '" in t:
            continue
        keep.append(t)
        if len(keep) == 2:
            break
    return " / ".join(keep)[:300] if keep else "(no message)"


def flatten(src_dir, dst_dir, toml_text):
    out = ""
    for f in ("data", "model", "main"):
        t = open(os.path.join(src_dir, f + ".fab"), encoding="utf-8").read()
        t = re.sub(r"^import from .*\n", "", t, flags=re.M)
        t = re.sub(r"^@ public\n", "", t, flags=re.M)
        t = re.sub(r"\b(data|model)\.", "", t)
        out += t + "\n"
    os.makedirs(os.path.join(dst_dir, "src"))
    with open(os.path.join(dst_dir, "src", "main.fab"), "w", encoding="utf-8") as h:
        h.write(out)
    with open(os.path.join(dst_dir, "faber.toml"), "w", encoding="utf-8") as h:
        h.write(toml_text)


def make_variant(variant, tmp):
    d = os.path.join(tmp, variant)
    toml_text = open(os.path.join(HERE, "faber.toml"), encoding="utf-8").read()
    if variant == "package":
        os.makedirs(d)
        shutil.copytree(os.path.join(HERE, "src"), os.path.join(d, "src"))
        shutil.copy(os.path.join(HERE, "faber.toml"), d)
    else:
        flatten(os.path.join(HERE, "src"), d, toml_text)
    return d


def execute(faber, target, variant, d, tmp, timeout):
    """Build and run one target. Returns (stage, stdout, message): stage is 'ok',
    'unsupported', 'build' or 'run'."""
    out = os.path.join(tmp, "out-%s-%s" % (target, variant))
    single = os.path.join(d, "src", "main.fab")
    if target == "runner":
        rc, so, se = sh([faber, "run", d], tmp, timeout)
        return ("ok", so, "") if rc == 0 else ("run", so, first_error(se))
    if target in ("ts", "go", "llvm-host"):
        rc, so, se = sh([faber, "run", "--target", target, d], tmp, timeout)
        return ("ok", so, "") if rc == 0 else ("build", so, first_error(se) if se.strip() else "exit %d, no message" % rc)
    if target in ("rust", "clif"):
        rc, so, se = sh([faber, "build", "-t", target, "-o", out, d], tmp, timeout)
        if rc != 0:
            return ("build", "", first_error(se))
        exe = os.path.join(d, "target", "debug", NAME) if target == "rust" else os.path.join(out, os.path.basename(d))
        if not os.path.exists(exe):
            return ("build", "", "build reported success but %s is missing" % exe)
        rc, so, se = sh([exe], tmp, timeout)
        return ("ok", so, "") if rc == 0 else ("run", so, first_error(se))
    if target == "python":
        if variant == "package":
            return ("unsupported", "", "package build unsupported for this target (PKG001 package_target_assembly_pending)")
        rc, so, se = sh([faber, "build", "-t", "python", single, "-o", out], tmp, timeout)
        if rc != 0:
            return ("build", "", first_error(se))
        rc, so, se = sh([sys.executable, os.path.join(out, "main.py")], tmp, timeout)
        return ("ok", so, "") if rc == 0 else ("run", so, first_error(se))
    if target == "wasm":
        if variant == "package":
            rc, so, se = sh([faber, "build", "-t", "wasm", "-o", out, d], tmp, timeout)
        else:
            rc, so, se = sh([faber, "build", "-t", "wasm", single, "-o", out], tmp, timeout)
        if rc != 0:
            return ("build", "", first_error(se))
        return ("build", "", "built; no host here runs the module (not yet wired)")
    return ("unsupported", "", "unknown target")


def sections(text):
    got, cur = {}, None
    for line in text.splitlines():
        m = re.match(r"^== (\w+)$", line)
        if m:
            cur = m.group(1)
            got[cur] = []
        elif cur is not None:
            got[cur].append(line)
    return {k: "\n".join(v) + "\n" for k, v in got.items()}


def expected_sections():
    exp = {}
    exp["trace"] = open(os.path.join(REF, "golden", "trace.csv"), encoding="utf-8").read()
    exp["summary"] = open(os.path.join(REF, "golden", "summary.csv"), encoding="utf-8").read()
    exp["rules"] = subprocess.run([sys.executable, "drone.py", "rules"], cwd=REF, capture_output=True, text=True, check=True).stdout
    return exp


def diff_row(want, got):
    wl, gl = want.splitlines(), got.splitlines()
    for i in range(max(len(wl), len(gl))):
        w = wl[i] if i < len(wl) else "<missing>"
        g = gl[i] if i < len(gl) else "<missing>"
        if w != g:
            return i + 1, w, g
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--faber", default=os.path.join(HERE, "..", "..", "..", "target", "debug", "faber"))
    ap.add_argument("--targets", default=",".join(TARGETS))
    ap.add_argument("--variants", default="package,flat")
    ap.add_argument("--timeout", type=int, default=900)
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--json")
    a = ap.parse_args()
    faber = os.path.abspath(a.faber)
    exp = expected_sections()
    tmp = tempfile.mkdtemp(prefix="drone-compare-")
    rows = []
    try:
        for variant in a.variants.split(","):
            for target in a.targets.split(","):
                vt = os.path.join(tmp, "%s-%s" % (variant, target))
                os.makedirs(vt)
                d = make_variant(variant, vt)
                t0 = time.perf_counter()
                stage, so, msg = execute(faber, target, variant, d, vt, a.timeout)
                row = {"target": target, "variant": variant, "stage": stage, "message": msg,
                       "seconds": round(time.perf_counter() - t0, 1)}
                if stage == "ok":
                    got = sections(so)
                    for k in MARKERS:
                        if k not in got:
                            row[k] = "missing"
                            continue
                        df = diff_row(exp[k], got[k])
                        row[k] = "match" if df is None else {"line": df[0], "want": df[1], "got": df[2]}
                rows.append(row)
                state = ", ".join("%s=%s" % (k, ("match" if row.get(k) == "match" else "DIFF" if isinstance(row.get(k), dict) else row.get(k)))
                                  for k in MARKERS) if stage == "ok" else "%s: %s" % (stage, msg)
                print("%-9s %-8s %5.1fs  %s" % (target, variant, row["seconds"], state), flush=True)
    finally:
        if a.keep:
            print("kept", tmp)
        else:
            shutil.rmtree(tmp, ignore_errors=True)
    if a.json:
        with open(a.json, "w", encoding="utf-8") as h:
            json.dump(rows, h, indent=1)
    bad = [r for r in rows if r["stage"] != "ok" or any(r.get(k) != "match" for k in MARKERS)]
    print("%d of %d runs match byte for byte" % (len(rows) - len(bad), len(rows)))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
