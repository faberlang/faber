#!/usr/bin/env python3
"""Shared lane table + helpers for the per-lane grid scripts.

Delivery: `docs/factory/per-lane-e2e-validation/delivery.md` EL-5. The lane
table mirrors the exempla feature pass-through (EL-1, decision 1) and the
diff-derived lane selection (`crates/exempla/src/exempla_e2e/lane_selection.rs`):
one lane per radix target feature, plus the no-backend minimal lanes
(`mir`, `roundtrip`) that run on the bare default-features build (decision 2).
"""

import os
import shutil
import subprocess
import sys

# Lane rows: (label, feature, run_target, filter_token, tools, kind)
#   feature     exempla feature selecting the lane; None = bare default build
#   run_target  "integration" -> `--test e2e_harness`; "lib" -> `--lib`
#   filter_token  libtest substring filter restricting the run to the lane's
#                 e2e test (required for the default-build lanes where every
#                 lane's modules compile; optional for feature-scoped builds)
#   tools       host toolchain binaries the lane e2e invokes at runtime; a
#               missing tool makes the lane SKIPPED (never a false green —
#               the harness itself would otherwise pass trivially)
#   kind        "e2e" or "compile-gate" (metal has no exempla harness e2e)
LANES = [
    ("go",        "hir-go",    "integration", None,                          ["go"],              "e2e"),
    ("ts",        "hir-ts",    "integration", None,                          ["node", "tsc", "deno"], "e2e"),
    ("wasm",      "mir-wasm",  "integration", None,                          ["wasmtime", "wasm-tools"], "e2e"),
    ("rust",      "hir-rust",  "integration", None,                          ["cargo", "rustc", "rustfmt"], "e2e"),
    ("swift",     "hir-swift", "integration", None,                          ["swiftc"],        "e2e"),
    ("sexp",      "mir-sexp",  "integration", None,                          ["racket"],        "e2e"),
    ("llvm",      "mir-llvm",  "integration", None,                          ["llvm-as", "opt"], "e2e"),
    ("metal",     "mir-metal", "integration", None,                          [],                "compile-gate"),
    ("mir",       None,        "lib",         "exempla_mir_e2e",             [],                "e2e"),
    ("roundtrip", None,        "integration", "exempla_faber_roundtrip_e2e", [],                "e2e"),
]

CANONICAL_ORDER = [row[0] for row in LANES]


def lane_row(label):
    for row in LANES:
        if row[0] == label:
            return row
    raise KeyError(f"unknown lane: {label}")


def extra_tool_dirs():
    """Candidate dirs for toolchains installed for the grid host.

    Grid infra (pharos) keeps non-apt toolchains under ~/.local/bin
    (deno/wasmtime/wasm-tools) and the Swift.org tarball under ~/swift.
    Rustup lives at ~/.cargo. Everything is probed with shutil.which, so
    stale entries are harmless.
    """
    home = os.path.expanduser("~")
    return [
        os.path.join(home, ".cargo", "bin"),
        os.path.join(home, ".local", "bin"),
        os.path.join(home, "swift", "usr", "bin"),
        "/usr/local/bin",
    ]


def path_with_tool_dirs():
    existing = os.environ.get("PATH", "")
    return ":".join([d for d in extra_tool_dirs()] + ([existing] if existing else []))


def _which(cmd):
    return shutil.which(cmd, path=path_with_tool_dirs())


def ts_toolchain_ok():
    """ts.rs detects deno OR (tsc AND node); mirror that probe.

    eslint/biome are deliberately NOT required: the burgus reference has
    neither, so the ts lane's lint tier is skipped there. An unconfigured
    eslint on the grid host (e.g. apt eslint 6.x) errors on every file and
    turns the whole lane red — keep it off the grid PATH (see
    docs/factory/per-lane-e2e-validation/pharos/README.md).
    """
    if _which("deno"):
        return True
    return bool(_which("tsc") and _which("node"))


def probe_tools(label):
    """Return (ok, missing_list). `ts` has an OR toolchain rule."""
    _, _, _, _, tools, _ = lane_row(label)
    if label == "ts":
        return ts_toolchain_ok(), []
    missing = [t for t in tools if not _which(t)]
    return (not missing), missing


def lane_command(label):
    """The exact-crate EL-1 lane command for one lane.

    Feature-scoped builds are the lane (they compile only that lane's
    modules), so no filter token is needed; default-build lanes pass their
    e2e filter so one lane's receipt never swallows another's tests.
    """
    _, feature, target, filter_token, _, _ = lane_row(label)
    base = ["cargo", "test", "-p", "exempla"]
    if feature is None:
        cmd = base + ["--lib"] if target == "lib" else base + ["--test", "e2e_harness"]
        cmd += ["--", "--ignored"]
        if filter_token:
            cmd.append(filter_token)
        return cmd
    return base + [
        "--no-default-features",
        "--features", feature,
        "--test", "e2e_harness",
        "--", "--ignored",
    ]


def run_captured(cmd, cwd, timeout_s, env_extra=None):
    """Run a lane command, capturing output. Returns (exit_code, stdout+stderr)."""
    env = dict(os.environ)
    env["PATH"] = path_with_tool_dirs()
    if env_extra:
        env.update(env_extra)
    try:
        proc = subprocess.run(
            cmd,
            cwd=cwd,
            env=env,
            capture_output=True,
            text=True,
            timeout=timeout_s,
        )
        return proc.returncode, (proc.stdout or "") + (proc.stderr or "")
    except subprocess.TimeoutExpired as err:
        out = (err.stdout or b"") if isinstance(err.stdout, bytes) else (err.stdout or "")
        errout = (err.stderr or b"") if isinstance(err.stderr, bytes) else (err.stderr or "")
        return -1, str(out) + "\n" + str(errout) + "\n[TIMEOUT after %ds]" % timeout_s
    except FileNotFoundError as err:
        return -2, f"command not found: {err}"


def extract_test_summary(output):
    """Last `test result: ...` line, if any."""
    for line in reversed(output.splitlines()):
        if "test result:" in line:
            return line.strip()
    return None


def is_dev_host():
    """Burgus is the dev machine holding the shared Cargo lock — the grid
    must never run there (done_when EL-5(d))."""
    try:
        import socket
        return socket.gethostname().lower().startswith("burgus")
    except Exception:
        return False
