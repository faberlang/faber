#!/usr/bin/env python3
"""Fixture tests for faber/scripta/generate-release-manifest.

Proves D5 of component-release-streamline Stage 3: the generator produces an
instance from live evidence that validates against the machine schema, and
refuses ad-hoc edits outside a release intent (schema §7). The fixture is a
faber-shaped tree with sibling radix/cista/faber-runtime/hosts trees, the
layout the release worktree rehearsal uses.
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

SCRIPT = Path(__file__).with_name("generate-release-manifest")

SHAS = {
    "faber": "3f1a2b4c5d6e7f8091a2b3c4d5e6f708192a3b4c",
    "radix": "5bbdbbd49c0d1e2f3a4b5c6d7e8f9001a2b3c4d5",
    "cista": "99acb1e2f3a4b5c6d7e8f90a1b2c3d4e5f607182",
    "faber-runtime": "57493dc1e2f3a4b5c6d7e8f90a1b2c3d4e5f60718",
    "hosts": "ced40f81a2b3c4d5e6f708192a3b4c5d6e7f8091",
}

PACKS = [
    {"name": "launcher", "component": "faber", "version": "1.5.0",
     "digest": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
     "compatibility": "1.x", "license": "MIT", "destination": "bin/faber"},
    {"name": "core-support", "component": "faber", "version": "1.5.0",
     "digest": "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210",
     "compatibility": "1.x", "license": "MIT",
     "destination": "share/faber/core-support/"},
]


def load_module():
    loader = importlib.machinery.SourceFileLoader(
        "faber_generate_release_manifest", str(SCRIPT)
    )
    spec = importlib.util.spec_from_loader("faber_generate_release_manifest", loader)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    loader.exec_module(module)
    return module


def write_package(manifest: Path, name: str, version: str) -> None:
    manifest.parent.mkdir(parents=True, exist_ok=True)
    manifest.write_text(
        f'[package]\nname = "{name}"\nversion = "{version}"\nedition = "2021"\n',
        encoding="utf-8",
    )


def build_fixture(container: Path, *, faber_version: str = "1.5.0") -> Path:
    faber = container / "faber"
    write_package(faber / "Cargo.toml", "faber", faber_version)
    (faber / "crates/hygiene-ratchet/Cargo.toml").parent.mkdir(parents=True)
    write_package(faber / "crates/hygiene-ratchet/Cargo.toml",
                  "hygiene-ratchet", "0.1.0")
    write_package(container / "radix/crates/radix/Cargo.toml", "radix", "0.80.0")
    write_package(container / "radix/crates/hygiene-ratchet/Cargo.toml",
                  "hygiene-ratchet", "0.1.0")
    write_package(container / "cista/Cargo.toml", "cista", "0.2.0")
    write_package(container / "faber-runtime/Cargo.toml",
                  "faber-runtime", "0.1.0")
    write_package(container / "hosts/Cargo.toml", "hosts", "0.1.0")
    return faber


def sha_args() -> list[str]:
    args: list[str] = []
    for name, sha in SHAS.items():
        args += [f"--{name}-sha", sha]
    return args


def packs_args() -> list[str]:
    return ["--packs", json.dumps(PACKS)]


class GenerateReleaseManifestTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.container = Path(self.tmp.name)
        self.module = load_module()

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def run_script(self, faber: Path, *extra: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(faber), *extra],
            check=False, capture_output=True, text=True,
        )

    def test_generates_validating_instance(self) -> None:
        faber = build_fixture(self.container)
        res = self.run_script(
            faber, "--version", "1.5.0", "--channel", "stable", "--line", "1.x",
            *sha_args(), *packs_args(),
        )
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        out = faber / "release-manifest.yaml"
        self.assertTrue(out.exists())
        text = out.read_text(encoding="utf-8")
        self.assertIn('releaseTag: faber-v1.5.0', text)
        self.assertIn('advancesGlobalLatest: true', text)
        self.assertIn('component: radix', text)
        self.assertIn('version: "0.80.0"', text)

    def test_version_mismatch_with_manifest_is_hard_stop(self) -> None:
        faber = build_fixture(self.container)
        res = self.run_script(
            faber, "--version", "9.9.9", *sha_args(), *packs_args(),
        )
        self.assertEqual(res.returncode, 2)
        self.assertIn("version mismatch", res.stderr)
        self.assertFalse((faber / "release-manifest.yaml").exists())

    def test_refuses_to_overwrite_without_force(self) -> None:
        faber = build_fixture(self.container)
        args = ["--version", "1.5.0", *sha_args(), *packs_args()]
        self.assertEqual(self.run_script(faber, *args).returncode, 0)
        res = self.run_script(faber, *args)
        self.assertEqual(res.returncode, 1)
        self.assertIn("refusing to overwrite", res.stderr)

    def test_force_overwrites_as_release_intent(self) -> None:
        faber = build_fixture(self.container)
        args = ["--version", "1.5.0", *sha_args(), *packs_args()]
        self.assertEqual(self.run_script(faber, *args).returncode, 0)
        res = self.run_script(faber, *args, "--force")
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)

    def test_check_mode_validates_existing_instance(self) -> None:
        faber = build_fixture(self.container)
        args = ["--version", "1.5.0", *sha_args(), *packs_args()]
        self.assertEqual(self.run_script(faber, *args).returncode, 0)
        res = self.run_script(faber, "--check")
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)

    def test_missing_packs_evidence_is_usage_error(self) -> None:
        faber = build_fixture(self.container)
        res = self.run_script(faber, "--version", "1.5.0", *sha_args())
        self.assertEqual(res.returncode, 2)
        self.assertIn("packs", res.stderr)

    def test_missing_sibling_tree_is_evidence_error(self) -> None:
        faber = build_fixture(self.container)
        (self.container / "cista").rename(self.container / "cista-gone")
        res = self.run_script(
            faber, "--version", "1.5.0", *sha_args(), *packs_args(),
        )
        self.assertEqual(res.returncode, 2)
        self.assertIn("cista", res.stderr)

    def test_bad_semver_rejected(self) -> None:
        faber = build_fixture(self.container)
        res = self.run_script(
            faber, "--version", "one.5", *sha_args(), *packs_args(),
        )
        self.assertEqual(res.returncode, 2)

    def test_packs_receipt_file_accepted(self) -> None:
        faber = build_fixture(self.container)
        receipt = self.container / "payload-receipt.json"
        receipt.write_text(json.dumps({"packs": PACKS}), encoding="utf-8")
        res = self.run_script(
            faber, "--version", "1.5.0", "--packs-receipt", str(receipt),
            *sha_args(),
        )
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)

    def test_channel_defaults_and_latest_rule(self) -> None:
        faber = build_fixture(self.container)
        res = self.run_script(
            faber, "--version", "1.5.0", "--channel", "candidate",
            *sha_args(), *packs_args(),
        )
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        text = (faber / "release-manifest.yaml").read_text(encoding="utf-8")
        self.assertIn("channel: candidate", text)
        self.assertIn("advancesGlobalLatest: false", text)


if __name__ == "__main__":
    unittest.main()
