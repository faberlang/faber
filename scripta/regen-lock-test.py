#!/usr/bin/env python3
"""Fixture tests for faber/scripta/regen-lock.

Proves D4 of component-release-streamline Stage 3 for the faber lock: --check
verifies freshness without writing; regen runs the cargo step through a mock
(never a real workspace lock update, never a test suite). The stale-lock
negative covers F2.
"""

from __future__ import annotations

import importlib.machinery
import importlib.util
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


if __name__ == "__main__":
    unittest.main()
