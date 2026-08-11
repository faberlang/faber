#!/usr/bin/env python3
"""Fixture tests for faber/scripta/install-faber.

Proves unit A1 of faber-onboarding Stage 3 (delivery-stage3.md): the verified
bootstrap installer

  - installs the canonical dev-kit payload to a user-local prefix
    non-interactively, verifying SHA-256 against the published basename-only
    checksum asset BEFORE any payload is unpacked or executed;
  - reports what changed (files written, receipt path, PATH state);
  - fails closed on checksum mismatch with NO partial install (nothing under
    the prefix touched);
  - behaves sanely on idempotent re-run (already current, or restores missing
    files); a different version on the same prefix fails closed (that matrix
    is unit A2);
  - rejects residuals and unsafe archive members;
  - honors --add-to-path idempotently.

The "release host" is a local directory produced by the real
`scripta/package-archive` wrapper (the exact published artifact shape:
basename-only .sha256), so the installer is exercised against the same
bytes a real release would publish. Clean-room runs use a scratch HOME,
minimal PATH, and stdin closed (no prompts possible).
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("install-faber")
PACKAGE_ARCHIVE = Path(__file__).with_name("package-archive")

VERSION = "1.5.0"
TRIPLE = "aarch64-apple-darwin"
ARCHIVE = f"faber-v{VERSION}-{TRIPLE}.tar.gz"

PAYLOAD_FILES = [
    "bin/faber",
    "share/faber/reference/index.toml",
    "share/faber/reference/PACK.toml",
    "share/faber/locale/en/pack.toml",
    "share/faber/locale/la/pack.toml",
]


def build_staging(root: Path) -> Path:
    """A minimal but layout-faithful dev-kit payload staging dir."""
    staging = root / f"dev-kit-{VERSION}-{TRIPLE}"
    for rel in PAYLOAD_FILES:
        path = staging / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        if rel == "bin/faber":
            path.write_bytes(b"#!/bin/sh\necho faber 1.5.0\n")
            path.chmod(0o755)
        elif rel.endswith("index.toml"):
            path.write_text('generated_on = "2026-08-11"\nfab_count = 0\n',
                            encoding="utf-8")
        else:
            path.write_text('[pack]\nname = "fixture"\n', encoding="utf-8")
    return staging


def build_release_host(root: Path, *, version: str = VERSION,
                       triple: str = TRIPLE) -> Path:
    """Wrap a payload with the real package-archive into a local release host."""
    staging_dir = f"dev-kit-{version}-{triple}"
    staging = root / "payload" / staging_dir
    for rel in PAYLOAD_FILES:
        path = staging / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        if rel == "bin/faber":
            path.write_bytes(b"#!/bin/sh\necho faber 1.5.0\n")
            path.chmod(0o755)
        else:
            path.write_text('[pack]\nname = "fixture"\n', encoding="utf-8")
    host = root / "host"
    res = subprocess.run(
        [sys.executable, str(PACKAGE_ARCHIVE),
         "--staging", str(staging),
         "--component", "faber",
         "--version", version,
         "--triple", triple,
         "--out", str(host)],
        check=False, capture_output=True, text=True,
    )
    if res.returncode != 0:
        raise RuntimeError(f"package-archive fixture failed: {res.stderr}")
    return host


def clean_env(root: Path) -> dict[str, str]:
    """Clean-room environment: scratch HOME, minimal PATH, no proxies/overrides."""
    env = {
        "HOME": str(root / "home"),
        "PATH": "/usr/bin:/bin",
        "TMPDIR": str(root / "tmp"),
        "LC_ALL": "C",
    }
    (root / "home").mkdir(parents=True, exist_ok=True)
    (root / "tmp").mkdir(parents=True, exist_ok=True)
    return env


class InstallFaberTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.host = build_release_host(self.root)
        # Resolve like the installer does (macOS /var -> /private/var), so
        # report/receipt/PATH assertions use identical paths.
        self.prefix = (self.root / "prefix").resolve()
        self.env = clean_env(self.root)

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def run_installer(self, *args: str, env: dict[str, str] | None = None,
                      version: str = VERSION, base: Path | None = None,
                      prefix: Path | None = None) -> subprocess.CompletedProcess[str]:
        cmd = [
            sys.executable, str(SCRIPT),
            "--version", version,
            "--triple", TRIPLE,
            "--base-url", str(base or self.host),
            "--prefix", str(prefix or self.prefix),
            *args,
        ]
        return subprocess.run(
            cmd, env=env or self.env, stdin=subprocess.DEVNULL,
            check=False, capture_output=True, text=True,
        )

    def installed_files(self, prefix: Path) -> dict[str, bytes]:
        return {
            rel: (prefix / rel).read_bytes()
            for rel in PAYLOAD_FILES
            if (prefix / rel).is_file()
        }

    # -- positive -----------------------------------------------------------

    def test_installs_payload_and_reports_changes(self) -> None:
        res = self.run_installer()
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        installed = self.installed_files(self.prefix)
        self.assertEqual(set(installed), set(PAYLOAD_FILES))
        self.assertTrue(os.access(self.prefix / "bin/faber", os.X_OK))
        # report lists the exact filesystem changes + receipt + PATH
        self.assertIn("added     bin/faber", res.stdout)
        self.assertIn("added     share/faber/reference/index.toml", res.stdout)
        self.assertIn("added     share/faber/locale/la/pack.toml", res.stdout)
        self.assertIn("files:      5 added, 0 updated, 0 unchanged, 0 removed", res.stdout)
        receipt = self.prefix / "share/faber/install-receipt.json"
        self.assertIn(f"receipt:    {receipt}", res.stdout)
        self.assertIn("PATH:", res.stdout)
        # receipt carries version/prefix/digest
        data = json.loads(receipt.read_text(encoding="utf-8"))
        self.assertEqual(data["version"], VERSION)
        self.assertEqual(data["triple"], TRIPLE)
        self.assertEqual(data["prefix"], str(self.prefix))
        self.assertEqual(data["archive"], ARCHIVE)
        real_digest = hashlib.sha256((self.host / ARCHIVE).read_bytes()).hexdigest()
        self.assertEqual(data["archiveSha256"], real_digest)
        self.assertIn("bin/faber", data["files"])

    def test_noninteractive_agent_style(self) -> None:
        # stdin closed; no prompts; clean stderr; report on stdout.
        res = self.run_installer()
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertEqual(res.stderr, "")
        self.assertIn("faber install report", res.stdout)
        self.assertIn("result:     installed", res.stdout)

    def test_report_lists_exact_filesystem_changes(self) -> None:
        res = self.run_installer()
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        for rel in PAYLOAD_FILES:
            self.assertIn(f"added     {rel}", res.stdout)

    # -- checksum verification before execution ----------------------------

    def test_tampered_checksum_fails_closed_before_any_payload_write(self) -> None:
        checksum = self.host / (ARCHIVE + ".sha256")
        checksum.write_text(
            f"{'0' * 64}  {ARCHIVE}\n", encoding="utf-8")
        res = self.run_installer()
        self.assertEqual(res.returncode, 1)
        self.assertIn("SHA-256 mismatch", res.stderr)
        self.assertIn("NO partial install", res.stderr)
        # nothing under the prefix may exist: verification precedes all writes
        self.assertFalse(self.prefix.exists()
                         or any(self.prefix.parent.rglob("prefix/*")))

    def test_path_qualified_checksum_name_rejected(self) -> None:
        # F7: the asset must name the basename only.
        digest = hashlib.sha256((self.host / ARCHIVE).read_bytes()).hexdigest()
        checksum = self.host / (ARCHIVE + ".sha256")
        checksum.write_text(f"{digest}  dist/{ARCHIVE}\n", encoding="utf-8")
        res = self.run_installer()
        self.assertEqual(res.returncode, 1)
        self.assertIn("basename", res.stderr)
        self.assertFalse(self.prefix.exists())

    def test_missing_archive_asset_fails_closed(self) -> None:
        empty = self.root / "empty-host"
        empty.mkdir()
        res = self.run_installer(base=empty)
        self.assertEqual(res.returncode, 1)
        self.assertIn("not found", res.stderr)

    def test_unsafe_archive_member_fails_closed(self) -> None:
        # A payload containing a `../` traversal member must be rejected.
        evil = self.root / "evil"
        evil.mkdir()
        bad_tar = evil / ARCHIVE
        import io
        with tarfile.open(bad_tar, "w:gz") as tar:
            info = tarfile.TarInfo("../escape.txt")
            info.size = 4
            tar.addfile(info, io.BytesIO(b"evil"))
        checksum = hashlib.sha256(bad_tar.read_bytes()).hexdigest()
        (evil / (ARCHIVE + ".sha256")).write_text(
            f"{checksum}  {ARCHIVE}\n", encoding="utf-8")
        res = self.run_installer(base=evil)
        self.assertEqual(res.returncode, 1)
        self.assertIn("unsafe archive member", res.stderr)
        self.assertFalse((self.root / "escape.txt").exists())
        self.assertFalse(self.prefix.exists())

    # -- idempotency ---------------------------------------------------------

    def test_idempotent_rerun_is_already_current(self) -> None:
        first = self.run_installer()
        self.assertEqual(first.returncode, 0, first.stdout + first.stderr)
        before = self.installed_files(self.prefix)
        res = self.run_installer()
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("result:     already current", res.stdout)
        self.assertIn("0 added, 0 updated, 5 unchanged, 0 removed", res.stdout)
        self.assertEqual(self.installed_files(self.prefix), before)

    def test_rerun_restores_missing_payload_file(self) -> None:
        self.assertEqual(self.run_installer().returncode, 0)
        missing = self.prefix / "share/faber/locale/en/pack.toml"
        missing.unlink()
        res = self.run_installer()
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertTrue(missing.is_file())
        self.assertIn("added     share/faber/locale/en/pack.toml", res.stdout)

    def test_rerun_restores_tampered_payload_file(self) -> None:
        self.assertEqual(self.run_installer().returncode, 0)
        tampered = self.prefix / "share/faber/reference/PACK.toml"
        tampered.write_text("# tampered\n", encoding="utf-8")
        res = self.run_installer()
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("updated   share/faber/reference/PACK.toml", res.stdout)
        self.assertEqual(tampered.read_bytes(),
                         (self.root / "payload" / f"dev-kit-{VERSION}-{TRIPLE}"
                          / "share/faber/reference/PACK.toml").read_bytes())

    def test_different_version_on_existing_prefix_fails_closed(self) -> None:
        # Versioned reinstall (upgrade/downgrade) is unit A2; A1 fails closed.
        self.assertEqual(self.run_installer().returncode, 0)
        binary_before = (self.prefix / "bin/faber").read_bytes()
        host2 = build_release_host(self.root, version="1.6.0")
        res = self.run_installer(version="1.6.0", base=host2)
        self.assertEqual(res.returncode, 1)
        self.assertIn("faber self update", res.stderr)
        self.assertEqual((self.prefix / "bin/faber").read_bytes(), binary_before)

    # -- platform / shell ----------------------------------------------------

    def test_residual_triple_fails_closed(self) -> None:
        res = self.run_installer("--triple", "x86_64-apple-darwin")
        self.assertEqual(res.returncode, 2)
        self.assertIn("residual", res.stderr)

    def test_add_to_path_is_explicit_and_idempotent(self) -> None:
        res = self.run_installer("--add-to-path")
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        home = self.root / "home"
        rc = home / (".zshrc" if sys.platform == "darwin" else ".bashrc")
        self.assertTrue(rc.is_file(), f"expected {rc} to be created")
        text = rc.read_text(encoding="utf-8")
        expected = f'export PATH="{self.prefix}/bin:$PATH"'
        self.assertEqual(text.count(expected), 1)
        self.assertIn("appended to", res.stdout)
        # idempotent: a second --add-to-path run does not duplicate the line
        res2 = self.run_installer("--add-to-path")
        self.assertEqual(res2.returncode, 0, res2.stdout + res2.stderr)
        self.assertEqual(rc.read_text(encoding="utf-8").count(expected), 1)

    def test_default_version_matches_committed_release_manifest(self) -> None:
        # R1/D1 interlock: the installer's default version must name the same
        # artifact as the committed freeze manifest.
        repo = Path(__file__).resolve().parents[1]
        manifest = repo / "release-manifest.yaml"
        if not manifest.is_file():
            self.skipTest("release-manifest.yaml not committed")
        import importlib.machinery
        import importlib.util
        loader = importlib.machinery.SourceFileLoader(
            "faber_install_faber", str(SCRIPT))
        spec = importlib.util.spec_from_loader("faber_install_faber", loader)
        module = importlib.util.module_from_spec(spec)
        loader.exec_module(module)
        manifest_text = manifest.read_text(encoding="utf-8")
        match = None
        for line in manifest_text.splitlines():
            line = line.strip()
            if line.startswith("version:"):
                match = line.split(":", 1)[1].strip().strip('"')
                break
        self.assertEqual(module.DEFAULT_VERSION, match)


if __name__ == "__main__":
    unittest.main()
