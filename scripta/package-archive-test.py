#!/usr/bin/env python3
"""Fixture tests for faber/scripta/package-archive.

Proves D6 of component-release-streamline Stage 3: the archive + basename-only
.sha256 verify with shasum -c on a downloaded set; identical-hash retry is
idempotent; a different-hash collision fails closed (release-contract.md
§5.1/§5.3, F7).
"""

from __future__ import annotations

import hashlib
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("package-archive")
COMPONENT = "faber"
VERSION = "1.5.0"
TRIPLE = "aarch64-apple-darwin"
ARCHIVE = f"{COMPONENT}-v{VERSION}-{TRIPLE}.tar.gz"


def build_staging(root: Path) -> Path:
    staging = root / "dev-kit-1.5.0-aarch64-apple-darwin"
    (staging / "bin").mkdir(parents=True)
    (staging / "share/faber/reference").mkdir(parents=True)
    (staging / "bin/faber").write_bytes(b"#!/bin/sh\necho faber 1.5.0\n")
    (staging / "share/faber/reference/PACK.toml").write_text(
        "[pack]\nname = \"reference\"\n", encoding="utf-8"
    )
    (staging / "bin/faber").chmod(0o755)
    return staging


class PackageArchiveTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.staging = build_staging(self.root)
        self.out = self.root / "out"

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def run_script(self, *extra: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT),
             "--staging", str(self.staging),
             "--component", COMPONENT,
             "--version", VERSION,
             "--triple", TRIPLE,
             "--out", str(self.out),
             *extra],
            check=False, capture_output=True, text=True,
        )

    def test_packages_archive_and_checksum(self) -> None:
        res = self.run_script()
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        archive = self.out / ARCHIVE
        checksum = self.out / (ARCHIVE + ".sha256")
        self.assertTrue(archive.exists())
        self.assertTrue(checksum.exists())

    def test_checksum_content_is_basename_only(self) -> None:
        self.assertEqual(self.run_script().returncode, 0)
        line = (self.out / (ARCHIVE + ".sha256")).read_text(encoding="utf-8").strip()
        digest, sep, name = line.partition("  ")
        self.assertEqual(sep, "  ")
        self.assertRegex(digest, r"^[0-9a-f]{64}$")
        self.assertEqual(name, ARCHIVE)
        self.assertNotIn("dist/", line)
        self.assertNotIn(str(self.out), line)

    def test_shasum_c_passes_on_downloaded_set(self) -> None:
        self.assertEqual(self.run_script().returncode, 0)
        shasum = shutil.which("shasum")
        self.assertIsNotNone(shasum, "shasum required on macOS for this test")
        # simulate a downloaded set: copy archive + .sha256 into a fresh dir
        download_dir = self.root / "downloaded"
        download_dir.mkdir()
        for name in (ARCHIVE, ARCHIVE + ".sha256"):
            shutil.copy2(self.out / name, download_dir / name)
        res = subprocess.run(
            [shasum, "-c", ARCHIVE + ".sha256"],
            cwd=str(download_dir), check=False, capture_output=True, text=True,
        )
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)

    def test_retry_is_idempotent_and_does_not_rewrite(self) -> None:
        self.assertEqual(self.run_script().returncode, 0)
        archive = self.out / ARCHIVE
        first_bytes = archive.read_bytes()
        res = self.run_script()
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("idempotent", res.stdout)
        self.assertEqual(archive.read_bytes(), first_bytes)

    def test_collision_fails_closed_without_overwrite(self) -> None:
        self.assertEqual(self.run_script().returncode, 0)
        archive = self.out / ARCHIVE
        first_bytes = archive.read_bytes()
        (self.staging / "bin/faber").write_bytes(b"#!/bin/sh\necho different\n")
        res = self.run_script()
        self.assertEqual(res.returncode, 1)
        self.assertIn("refusing to overwrite", res.stderr)
        self.assertIn("fail closed", res.stderr)
        self.assertEqual(archive.read_bytes(), first_bytes)

    def test_check_verifies_valid_pair(self) -> None:
        self.assertEqual(self.run_script().returncode, 0)
        res = self.run_script("--check")
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("verified", res.stdout)

    def test_check_fails_on_tampered_archive(self) -> None:
        self.assertEqual(self.run_script().returncode, 0)
        archive = self.out / ARCHIVE
        archive.write_bytes(archive.read_bytes() + b"tamper")
        res = self.run_script("--check")
        self.assertEqual(res.returncode, 1)
        self.assertIn("hash mismatch", res.stderr)

    def test_check_rejects_path_qualified_checksum_name(self) -> None:
        # F7 regression: checksum content must name the basename only.
        self.assertEqual(self.run_script().returncode, 0)
        checksum = self.out / (ARCHIVE + ".sha256")
        digest = hashlib.sha256((self.out / ARCHIVE).read_bytes()).hexdigest()
        checksum.write_text(f"{digest}  dist/{ARCHIVE}\n", encoding="utf-8")
        res = self.run_script("--check")
        self.assertEqual(res.returncode, 1)
        self.assertIn("basename", res.stderr)

    def test_identical_inputs_produce_identical_archives(self) -> None:
        self.assertEqual(self.run_script().returncode, 0)
        first = (self.out / ARCHIVE).read_bytes()
        out2 = self.root / "out2"
        res = subprocess.run(
            [sys.executable, str(SCRIPT),
             "--staging", str(self.staging),
             "--component", COMPONENT,
             "--version", VERSION,
             "--triple", TRIPLE,
             "--out", str(out2)],
            check=False, capture_output=True, text=True,
        )
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertEqual(first, (out2 / ARCHIVE).read_bytes())


if __name__ == "__main__":
    unittest.main()
