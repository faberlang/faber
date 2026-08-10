#!/usr/bin/env python3
"""Fixture tests for faber/scripta/smoke-test-release-archive.

Proves U3 (S3) of the faber 1.5.1 pack-release delivery spec: the consumer
smoke-test gate. Covers argument parsing, fixture writing, clean-prefix env
construction, assertion-order enforcement (pack-exercising class BEFORE the
version assertion), and the deterministic pack-error classification — on a
synthetic correct-version bare archive (`--expect-fail-class pack-error` ->
exit 0), on a wrong-version bare archive under the same mode (-> exit 1, wrong
class), and on a v1.5.0-shape fixture (pack-error class, not
version-mismatch). The real positive proof runs against the real 1.5.1
archive at the release boundary (operator/verifier + CI).

Synthetic archives are built with scripta/package-archive from fixture staging
dirs; stub `faber` binaries are shell scripts that either fail closed when the
`en` reader pack is missing (mirroring the real v1.5.0+ fail-closed behavior)
or pass through every command while reporting a fixed version.
"""

from __future__ import annotations

import importlib.machinery
import importlib.util
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("smoke-test-release-archive")
PACKAGE_ARCHIVE = Path(__file__).with_name("package-archive")
COMPONENT = "faber"
TRIPLE = "aarch64-apple-darwin"
VERSION = "1.5.1"
WRONG_VERSION = "1.4.0"
LEGACY_VERSION = "1.5.0"
ARCHIVE = f"{COMPONENT}-v{VERSION}-{TRIPLE}.tar.gz"


def load_script():
    # The script has no .py suffix, so spec_from_file_location needs an
    # explicit SourceFileLoader.
    loader = importlib.machinery.SourceFileLoader(
        "smoke_test_mod", str(SCRIPT)
    )
    spec = importlib.util.spec_from_loader("smoke_test_mod", loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


def realistic_stub(version: str) -> str:
    """Stub that fails closed when the en reader pack is missing (mirrors the
    real release-shaped binary) and reports `version` for --version."""
    return (
        "#!/bin/sh\n"
        "# realistic faber stub: fail closed without the en reader pack\n"
        'EN_PACK="$(dirname "$0")/../share/faber/locale/en/pack.toml"\n'
        'if [ ! -f "$EN_PACK" ]; then\n'
        '  echo "error: failed to load reader locale \'en\' pack \'$EN_PACK\': No such file or directory (os error 2)" >&2\n'
        '  echo "next action: install the matching reader pack for locale \'en\' (share/faber/locale/en/pack.toml beside the faber binary) or fix the package pack path" >&2\n'
        "  exit 1\n"
        "fi\n"
        'if [ "$1" = "--version" ]; then\n'
        f'  echo "faber {version}"\n'
        "  exit 0\n"
        "fi\n"
        "exit 0\n"
    )


def passing_stub(version: str) -> str:
    """Stub that passes every command (no pack check) and reports `version`."""
    return (
        "#!/bin/sh\n"
        'if [ "$1" = "--version" ]; then\n'
        f'  echo "faber {version}"\n'
        "  exit 0\n"
        "fi\n"
        "exit 0\n"
    )


def write_binary(path: Path, script: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(script, encoding="utf-8")
    path.chmod(0o755)


def package_staging(root: Path, staging: Path, version: str, out: Path) -> Path:
    out.mkdir(parents=True, exist_ok=True)
    res = subprocess.run(
        [sys.executable, str(PACKAGE_ARCHIVE),
         "--staging", str(staging),
         "--component", COMPONENT,
         "--version", version,
         "--triple", TRIPLE,
         "--out", str(out)],
        check=False, capture_output=True, text=True,
    )
    assert res.returncode == 0, res.stdout + res.stderr
    return out / f"{COMPONENT}-v{version}-{TRIPLE}.tar.gz"


def build_bare_archive(root: Path, *, stub: str, version: str) -> Path:
    """Bare archive: bin/faber only (the P0 shape), stub-scripted binary."""
    staging = root / "staging-bare"
    write_binary(staging / "bin" / "faber", stub)
    return package_staging(root, staging, version, root / "out")


def build_legacy_wrapped_archive(root: Path, *, stub: str) -> Path:
    """v1.5.0-shaped archive: top-level wrapper dir + bare binary (matches the
    real v1.5.0 archive layout)."""
    staging = root / "staging-legacy"
    wrap = staging / f"faber-v1.5.0-{TRIPLE}"
    write_binary(wrap / "faber", stub)
    (wrap / "README.txt").write_text("Faber v1.5.0\n", encoding="utf-8")
    return package_staging(root, staging, LEGACY_VERSION, root / "out")


def build_devkit_archive(
    root: Path, *, stub: str, version: str, locales: list[str] | None = None
) -> Path:
    """Dev-kit-shaped archive: bin/faber + reference + N locale packs."""
    staging = root / "staging-devkit"
    write_binary(staging / "bin" / "faber", stub)
    reference = staging / "share" / "faber" / "reference"
    reference.mkdir(parents=True)
    (reference / "index.toml").write_text('generated_on = "2026-08-09"\n', encoding="utf-8")
    (reference / "PACK.toml").write_text(
        f'faber_version = "{version}"\n', encoding="utf-8"
    )
    (reference / "legacy-redirects.toml").write_text("redirects = []\n", encoding="utf-8")
    for loc in locales or ["ar", "en", "hi", "la", "th-TH", "vi", "zh-Hans", "zh-Hant"]:
        pack = staging / "share" / "faber" / "locale" / loc
        pack.mkdir(parents=True)
        (pack / "pack.toml").write_text(
            f'[metadata]\nid = "{loc}"\n', encoding="utf-8"
        )
    return package_staging(root, staging, version, root / "out")


class SmokeTestReleaseArchiveTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.mod = load_script()

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def run_script(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args],
            check=False, capture_output=True, text=True,
        )

    # --- argument parsing ---

    def test_requires_archive_or_download_into(self) -> None:
        res = self.run_script("--version", VERSION, "--triple", TRIPLE)
        self.assertEqual(res.returncode, 2, res.stdout + res.stderr)
        self.assertIn("exactly one of --archive / --download-into", res.stderr)

    def test_rejects_both_archive_and_download_into(self) -> None:
        res = self.run_script(
            "--archive", str(self.root), "--download-into", str(self.root),
            "--version", VERSION, "--triple", TRIPLE,
        )
        self.assertEqual(res.returncode, 2, res.stdout + res.stderr)
        self.assertIn("exactly one of --archive / --download-into", res.stderr)

    def test_requires_version_and_triple(self) -> None:
        archive = self.root / "faber-v1.5.1-aarch64-apple-darwin.tar.gz"
        res = self.run_script("--archive", str(archive))
        self.assertEqual(res.returncode, 2, res.stdout + res.stderr)
        res = self.run_script("--archive", str(archive), "--version", VERSION)
        self.assertEqual(res.returncode, 2, res.stdout + res.stderr)

    def test_rejects_unknown_expect_fail_class(self) -> None:
        archive = self.root / "x.tar.gz"
        res = self.run_script(
            "--archive", str(archive), "--version", VERSION, "--triple", TRIPLE,
            "--expect-fail-class", "nonsense",
        )
        self.assertEqual(res.returncode, 2, res.stdout + res.stderr)

    def test_rejects_missing_archive_file(self) -> None:
        missing = self.root / "does-not-exist.tar.gz"
        res = self.run_script("--archive", str(missing), "--version", VERSION,
                              "--triple", TRIPLE)
        self.assertEqual(res.returncode, 1)
        self.assertIn("archive not found", res.stderr)

    # --- fixture writing ---

    def test_write_hello_world_fixture(self) -> None:
        root = self.root / "hello"
        self.mod.write_hello_world(root)
        self.assertTrue((root / "faber.toml").is_file())
        self.assertTrue((root / "src" / "main.fab").is_file())
        faber_toml = (root / "faber.toml").read_text(encoding="utf-8")
        self.assertIn('name = "smoke-hello"', faber_toml)
        self.assertNotIn("[reader]", faber_toml)
        main_fab = (root / "src" / "main.fab").read_text(encoding="utf-8")
        self.assertIn('incipit { nota "Salve, munde!" }', main_fab)

    # --- clean-prefix env construction ---

    def test_clean_env_is_minimal(self) -> None:
        work = self.root / "work"
        env = self.mod.clean_env(work)
        self.assertEqual(env["HOME"], str(work / "home"))
        self.assertEqual(env["PATH"], "/usr/bin:/bin")
        self.assertEqual(env["TMPDIR"], str(work / "tmp"))
        self.assertEqual(env["LC_ALL"], "C")
        self.assertEqual(
            sorted(env.keys()),
            ["HOME", "LC_ALL", "PATH", "TMPDIR"],
            "no ambient env keys may leak into the clean prefix",
        )

    def test_locate_binary_finds_flat_and_wrapped_layouts(self) -> None:
        prefix = self.root / "prefix"
        write_binary(prefix / "bin" / "faber", passing_stub(VERSION))
        self.assertEqual(self.mod.locate_binary(prefix), prefix / "bin" / "faber")

        wrapped = self.root / "wrapped"
        write_binary(wrapped / f"faber-v1.5.0-{TRIPLE}" / "faber", passing_stub(VERSION))
        self.assertIsNotNone(self.mod.locate_binary(wrapped))

    # --- assertion-order enforcement: pack class BEFORE version ---

    def test_v150_shape_classified_pack_error_not_version_mismatch(self) -> None:
        # The v1.5.0-shaped fixture is BOTH bare (fails closed on the pack
        # error) AND wrong-version (reports 1.5.0). Assertion order must
        # classify it as pack-error, never version-mismatch.
        archive = build_legacy_wrapped_archive(
            self.root, stub=realistic_stub(LEGACY_VERSION)
        )
        res = self.run_script(
            "--archive", str(archive), "--version", VERSION, "--triple", TRIPLE,
            "--expect-fail-class", "pack-error",
        )
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("negative proof GREEN", res.stdout)
        self.assertIn("pack-error", res.stdout)

        # Same fixture under --expect-fail-class version-mismatch must exit 1:
        # it was rejected for the pack class, NOT the version class.
        res = self.run_script(
            "--archive", str(archive), "--version", VERSION, "--triple", TRIPLE,
            "--expect-fail-class", "version-mismatch",
        )
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("rejected with pack-error", res.stdout)

    # --- deterministic pack-error classification ---

    def test_synthetic_correct_version_bare_archive_pack_error(self) -> None:
        # Correct-version bare archive: realistic stub reports 1.5.1 but fails
        # closed (no packs). Must be rejected with the pack-error class.
        archive = build_bare_archive(
            self.root, stub=realistic_stub(VERSION), version=VERSION
        )
        res = self.run_script(
            "--archive", str(archive), "--version", VERSION, "--triple", TRIPLE,
            "--expect-fail-class", "pack-error",
        )
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("negative proof GREEN", res.stdout)
        self.assertIn("pack-error", res.stdout)

        # Plain (non-negative) mode must name the failing pack assertion.
        res = self.run_script(
            "--archive", str(archive), "--version", VERSION, "--triple", TRIPLE,
        )
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("UNRELEASABLE", res.stdout)
        self.assertIn("class pack-error", res.stdout)

    def test_wrong_version_bare_archive_wrong_class(self) -> None:
        # A bare archive whose stub passes every command but reports the wrong
        # version is rejected for version-mismatch (the pack-exercising class
        # cannot fire). Under --expect-fail-class pack-error that is the wrong
        # class -> exit 1 (negative proof NOT established).
        archive = build_bare_archive(
            self.root, stub=passing_stub(WRONG_VERSION), version=VERSION
        )
        res = self.run_script(
            "--archive", str(archive), "--version", VERSION, "--triple", TRIPLE,
            "--expect-fail-class", "pack-error",
        )
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("rejected with version-mismatch", res.stdout)

        # Under --expect-fail-class version-mismatch the SAME archive is the
        # expected rejection -> exit 0.
        res = self.run_script(
            "--archive", str(archive), "--version", VERSION, "--triple", TRIPLE,
            "--expect-fail-class", "version-mismatch",
        )
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("negative proof GREEN", res.stdout)

    # --- positive path ---

    def test_devkit_archive_passes(self) -> None:
        archive = build_devkit_archive(
            self.root, stub=realistic_stub(VERSION), version=VERSION
        )
        res = self.run_script(
            "--archive", str(archive), "--version", VERSION, "--triple", TRIPLE,
        )
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("RELEASABLE", res.stdout)

    def test_layout_guard_rejects_non_devkit_shape(self) -> None:
        # Correct version, full pack-exercising pass, but a locale pack is
        # missing: the archive-layout structural proof rejects it.
        archive = build_devkit_archive(
            self.root,
            stub=passing_stub(VERSION),
            version=VERSION,
            locales=["ar", "en", "hi", "la", "th-TH", "vi", "zh-Hans"],  # no zh-Hant
        )
        res = self.run_script(
            "--archive", str(archive), "--version", VERSION, "--triple", TRIPLE,
        )
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("UNRELEASABLE", res.stdout)
        self.assertIn("archive layout", res.stdout)
        self.assertIn("zh-Hant", res.stdout)


if __name__ == "__main__":
    unittest.main()
