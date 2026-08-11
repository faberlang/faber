#!/usr/bin/env python3
"""Fixture tests for faber/scripta/regen-lock.

Proves D4 of component-release-streamline Stage 3 for the faber lock: --check
verifies freshness without writing; regen runs the cargo step through a mock
(never a real workspace lock update, never a test suite). The stale-lock
negative covers F2.

EL-6 extensions: --pinned-siblings is the one-command pin-packet rehearsal.
Tests prove pin-match success, pin-mismatch hard-stop (no cargo write),
missing-sibling hard-stop, YAML + JSON pin loading, and that a --root
scoped run never mutates a caller's (outer) Cargo.lock.
"""

from __future__ import annotations

import importlib.machinery
import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("regen-lock")


def load_module():
    loader = importlib.machinery.SourceFileLoader("faber_regen_lock", str(SCRIPT))
    spec = importlib.util.spec_from_loader("faber_regen_lock", loader)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    loader.exec_module(module)
    return module


def write_crate(root: Path, rel: str, name: str, version: str) -> None:
    toml = root / rel / "Cargo.toml"
    toml.parent.mkdir(parents=True, exist_ok=True)
    toml.write_text(
        f'[package]\nname = "{name}"\nversion = "{version}"\nedition = "2021"\n',
        encoding="utf-8",
    )


def write_lock(root: Path, versions: dict[str, str]) -> None:
    parts = ["version = 4", ""]
    for name, version in sorted(versions.items()):
        parts += ["[[package]]", f'name = "{name}"', f'version = "{version}"', ""]
    (root / "Cargo.lock").write_text("\n".join(parts), encoding="utf-8")


def build_fixture(root: Path, *, stale: bool = False) -> None:
    # faber root: both a [package] (faber) and a [workspace] with members.
    root.mkdir(parents=True, exist_ok=True)
    (root / "Cargo.toml").write_text(
        '[package]\nname = "faber"\nversion = "1.4.0"\nedition = "2021"\n\n'
        '[workspace]\nmembers = [\n'
        '    ".",\n'
        '    "crates/exempla",\n'
        '    "crates/faber-hir-rust",\n'
        '    "crates/hygiene-ratchet",\n'
        ']\nresolver = "2"\n',
        encoding="utf-8",
    )
    write_crate(root, "crates/exempla", "exempla", "0.1.0")
    write_crate(root, "crates/faber-hir-rust", "faber-hir-rust", "1.4.0")
    write_crate(root, "crates/hygiene-ratchet", "hygiene-ratchet", "0.1.0")
    versions = {
        "faber": "1.3.0" if stale else "1.4.0",
        "exempla": "0.1.0",
        "faber-hir-rust": "1.4.0",
        "hygiene-ratchet": "0.1.0",
    }
    write_lock(root, versions)


def git(repo: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", "-C", str(repo), *args],
        check=False, capture_output=True, text=True,
    )


def git_init_commit(repo: Path, message: str = "init") -> str:
    repo.mkdir(parents=True, exist_ok=True)
    git(repo, "init", "-q", "-b", "main")
    git(repo, "config", "user.email", "regen-lock-test@example.com")
    git(repo, "config", "user.name", "regen-lock-test")
    (repo / "README").write_text(f"{repo.name}\n", encoding="utf-8")
    git(repo, "add", "-A")
    res = git(repo, "commit", "-q", "-m", message)
    if res.returncode != 0:
        raise RuntimeError(f"fixture commit failed: {res.stderr}")
    return git(repo, "rev-parse", "HEAD").stdout.strip()


def write_manifest_yaml(path: Path, pins: dict[str, str]) -> None:
    lines = [
        'schemaVersion: "1"',
        "manifestName: faber-release",
        "preparedAt: 2026-08-11",
        "releaseIntent:",
        "  component: faber",
        '  version: "1.4.0"',
        "  channel: candidate",
        '  line: "1.x"',
        "pinnedInputs:",
        "  source:",
    ]
    for name, commit in pins.items():
        lines.append(f"    - name: {name}")
        lines.append(f"      commit: {commit}")
    lines += [
        "  packs: []",
        "versionSources: []",
        "exclusions: []",
        "",
    ]
    path.write_text("\n".join(lines), encoding="utf-8")


def write_manifest_json(path: Path, pins: dict[str, str]) -> None:
    data = {
        "schemaVersion": "1",
        "manifestName": "faber-release",
        "preparedAt": "2026-08-11",
        "releaseIntent": {
            "component": "faber",
            "version": "1.4.0",
            "channel": "candidate",
            "line": "1.x",
        },
        "pinnedInputs": {
            "source": [
                {"name": name, "commit": commit} for name, commit in pins.items()
            ],
            "packs": [],
        },
        "versionSources": [],
        "exclusions": [],
    }
    path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def build_pin_packet(container: Path) -> tuple[Path, dict[str, str]]:
    """Build a scratch pin packet: container/{faber,radix,cista,...}."""
    pins: dict[str, str] = {}
    for name in ("radix", "cista", "faber-runtime", "hosts"):
        pins[name] = git_init_commit(container / name)
    # faber pin is recorded but not enforced by --pinned-siblings
    faber = container / "faber"
    build_fixture(faber)
    pins["faber"] = "0" * 40  # placeholder; not checked
    write_manifest_yaml(faber / "release-manifest.yaml", pins)
    return faber, pins


class FaberRegenLockTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.module = load_module()

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def run_script(self, *argv: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(self.root), *argv],
            check=False,
            capture_output=True,
            text=True,
        )

    def test_check_passes_on_fresh_lock(self) -> None:
        build_fixture(self.root)
        res = self.run_script("--check")
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)

    def test_check_fails_on_stale_root_member(self) -> None:
        build_fixture(self.root, stale=True)
        res = self.run_script("--check")
        self.assertNotEqual(res.returncode, 0)
        self.assertIn("faber", res.stderr)
        self.assertIn("1.3.0", res.stderr)

    def test_check_fails_on_stale_crate_member(self) -> None:
        build_fixture(self.root)
        write_lock(self.root, {
            "faber": "1.4.0",
            "exempla": "0.1.0",
            "faber-hir-rust": "1.3.0",
            "hygiene-ratchet": "0.1.0",
        })
        res = self.run_script("--check")
        self.assertNotEqual(res.returncode, 0)
        self.assertIn("faber-hir-rust", res.stderr)

    def test_check_ignores_non_member_crate(self) -> None:
        # A crate under crates/ that is NOT a workspace member must not fail
        # the freshness check (mirrors faber's crates/http-transport today).
        build_fixture(self.root)
        write_crate(root=self.root, rel="crates/http-transport",
                    name="faber-http-transport", version="0.1.0")
        res = self.run_script("--check")
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)

    def test_check_never_writes(self) -> None:
        build_fixture(self.root, stale=True)
        before = (self.root / "Cargo.lock").read_text(encoding="utf-8")
        self.run_script("--check")
        after = (self.root / "Cargo.lock").read_text(encoding="utf-8")
        self.assertEqual(before, after)

    def test_regen_runs_cargo_offline_and_verifies(self) -> None:
        build_fixture(self.root, stale=True)
        calls: list[list[str]] = []

        def fake_cargo(argv: list[str], **kwargs) -> object:
            calls.append(argv)
            write_lock(self.root, {
                "faber": "1.4.0",
                "exempla": "0.1.0",
                "faber-hir-rust": "1.4.0",
                "hygiene-ratchet": "0.1.0",
            })
            return subprocess.CompletedProcess(argv, 0, "", "")

        self.module.subprocess_run = fake_cargo
        code, message = self.module.regen(self.root, "cargo", offline=True)
        self.assertEqual(code, 0, message)
        self.assertEqual(calls, [["cargo", "update", "--offline"]])

    def test_regen_reports_cargo_failure(self) -> None:
        build_fixture(self.root)

        def fake_cargo(argv: list[str], **kwargs) -> object:
            return subprocess.CompletedProcess(argv, 1, "", "index error")

        self.module.subprocess_run = fake_cargo
        code, message = self.module.regen(self.root, "cargo", offline=True)
        self.assertEqual(code, 1)
        self.assertIn("cargo update failed", message)

    # -- EL-6: --pinned-siblings --------------------------------------------

    def test_pinned_siblings_check_passes_on_matching_packet(self) -> None:
        container = self.root / "packet"
        faber, _pins = build_pin_packet(container)
        res = subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(faber),
             "--pinned-siblings", "--check"],
            check=False, capture_output=True, text=True,
        )
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("pinned siblings match", res.stdout)
        self.assertIn("fresh", res.stdout)

    def test_pinned_siblings_fails_on_pin_mismatch(self) -> None:
        container = self.root / "packet"
        faber, pins = build_pin_packet(container)
        # Advance radix past the pin so the lock would resolve against a
        # newer sibling — the classic L2 trap.
        (container / "radix" / "extra").write_text("drift\n", encoding="utf-8")
        git(container / "radix", "add", "-A")
        git(container / "radix", "commit", "-q", "-m", "drift")
        before = (faber / "Cargo.lock").read_text(encoding="utf-8")
        res = subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(faber),
             "--pinned-siblings"],
            check=False, capture_output=True, text=True,
        )
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("pin mismatch", res.stderr)
        self.assertIn("radix", res.stderr)
        after = (faber / "Cargo.lock").read_text(encoding="utf-8")
        self.assertEqual(before, after, "pin mismatch must not rewrite the lock")
        # pinned SHA still in the manifest
        self.assertIn(pins["radix"][:12], res.stderr)

    def test_pinned_siblings_fails_on_missing_sibling(self) -> None:
        container = self.root / "packet"
        faber, _pins = build_pin_packet(container)
        # Remove hosts — incomplete pin packet.
        import shutil
        shutil.rmtree(container / "hosts")
        res = subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(faber),
             "--pinned-siblings", "--check"],
            check=False, capture_output=True, text=True,
        )
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("sibling missing", res.stderr)
        self.assertIn("hosts", res.stderr)

    def test_pinned_siblings_accepts_json_manifest(self) -> None:
        container = self.root / "packet"
        faber, pins = build_pin_packet(container)
        write_manifest_json(faber / "release-manifest.yaml", pins)
        res = subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(faber),
             "--pinned-siblings", "--check"],
            check=False, capture_output=True, text=True,
        )
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)

    def test_pinned_siblings_short_sha_match(self) -> None:
        container = self.root / "packet"
        faber, pins = build_pin_packet(container)
        short = {n: c[:12] for n, c in pins.items()}
        write_manifest_yaml(faber / "release-manifest.yaml", short)
        res = subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(faber),
             "--pinned-siblings", "--check"],
            check=False, capture_output=True, text=True,
        )
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)

    def test_pinned_siblings_regen_scopes_to_root(self) -> None:
        """A --root scoped regen never mutates an outer caller's Cargo.lock."""
        outer = self.root / "outer-caller"
        build_fixture(outer, stale=True)
        outer_before = (outer / "Cargo.lock").read_text(encoding="utf-8")

        container = self.root / "packet"
        faber, _pins = build_pin_packet(container)
        # Make the pin-packet lock stale so regen would rewrite it.
        write_lock(faber, {
            "faber": "1.3.0",
            "exempla": "0.1.0",
            "faber-hir-rust": "1.4.0",
            "hygiene-ratchet": "0.1.0",
        })
        packet_before = (faber / "Cargo.lock").read_text(encoding="utf-8")

        # Intercept cargo only when cwd is the pin-packet faber root.
        real_run = subprocess.run

        def selective_run(argv, **kwargs):
            cwd = Path(kwargs.get("cwd") or ".").resolve()
            if argv and argv[0] == "cargo":
                self.assertEqual(cwd, faber.resolve())
                write_lock(faber, {
                    "faber": "1.4.0",
                    "exempla": "0.1.0",
                    "faber-hir-rust": "1.4.0",
                    "hygiene-ratchet": "0.1.0",
                })
                return subprocess.CompletedProcess(argv, 0, "", "")
            return real_run(argv, **kwargs)

        # Drive via the loaded module so we can mock subprocess_run.
        self.module.subprocess_run = selective_run
        code = self.module.main([
            "--root", str(faber),
            "--pinned-siblings",
        ])
        self.assertEqual(code, 0)
        outer_after = (outer / "Cargo.lock").read_text(encoding="utf-8")
        self.assertEqual(outer_before, outer_after,
                         "caller lock must be untouched by pin-packet regen")
        packet_after = (faber / "Cargo.lock").read_text(encoding="utf-8")
        self.assertNotEqual(packet_before, packet_after)
        self.assertIn('version = "1.4.0"', packet_after)

    def test_load_source_pins_yaml_subset(self) -> None:
        pins = {
            "faber": "a" * 40,
            "radix": "b" * 40,
            "cista": "c" * 40,
            "faber-runtime": "d" * 40,
            "hosts": "e" * 40,
        }
        path = self.root / "release-manifest.yaml"
        write_manifest_yaml(path, pins)
        loaded = self.module.load_source_pins(path)
        self.assertEqual(loaded, pins)


if __name__ == "__main__":
    unittest.main()
