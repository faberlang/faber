#!/usr/bin/env python3
"""Fixture tests for faber/scripta/install-faber.

Proves units A1 + A2 of faber-onboarding Stage 3 (delivery-stage3.md): the
verified bootstrap installer

  - installs the canonical dev-kit payload to a user-local prefix
    non-interactively, verifying SHA-256 against the published basename-only
    checksum asset BEFORE any payload is unpacked or executed;
  - reports what changed (files written, receipt path, PATH state);
  - fails closed on checksum mismatch with NO partial install (nothing under
    the prefix touched);
  - behaves sanely on idempotent re-run (already current, or restores missing
    files); a different version on the same prefix fails closed and names the
    upgrade path;
  - the reinstall idempotency matrix (A2): same-version reinstall is "already
    current" with no churn; missing/tampered files are restored and reported;
    cross-version installs fail closed;
  - the `faber self update` engine (A2/A3): `--update` installs a versioned
    target into side-by-side lanes (`<prefix>/versions/<version>/`),
    preserves the current install as a lane, flips the active launcher/receipt,
    and leaves user projects + the package store byte-identically untouched;
    failure paths fail closed (no partial install, version unchanged);
    cross-lane changes (odd dev <-> even LTS) fail closed without
    --allow-lane-change;
  - the downgrade policy (A3): a pinned downgrade to an older version
    succeeds through the same lane machinery (active-flip + rollback) with
    user projects/store byte-identical, and re-verifies `--version` +
    `explain`; a downgrade that would strand a locked pack of a user project
    (faber_min bound the target cannot satisfy — lock entry or the locked
    package's cista.toml) fails closed with a typed cause + one next action
    and NO partial install; a blocked target lane fails closed; mid-flip
    write failures roll back;
  - rejects residuals and unsafe archive members;
  - destination-side containment (CTO audit FINDING 1): a pre-existing
    `prefix/share -> /outside` symlink is rejected before any write and both
    sides stay byte-identical; the same rule covers the receipt write and
    PATH-owned shell rc writes;
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
    """Wrap a payload with the real package-archive into a local release host.

    The fixture launcher answers `--version` (re-verify after a downgrade)
    and `explain` by printing the ACTIVE reference pack's index.toml (the
    pack layer flips with the launcher — re-verified after a downgrade).
    """
    staging_dir = f"dev-kit-{version}-{triple}"
    staging = root / "payload" / staging_dir
    for rel in PAYLOAD_FILES:
        path = staging / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        if rel == "bin/faber":
            path.write_bytes(
                ("#!/bin/sh\n"
                 'if [ "$1" = "explain" ]; then\n'
                 '  d=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)\n'
                 '  exec cat "$d/../share/faber/reference/index.toml"\n'
                 "fi\n"
                 f"echo faber {version}\n").encode()
            )
            path.chmod(0o755)
        else:
            path.write_text(f'[pack]\nname = "fixture {version}"\n', encoding="utf-8")
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

    # -- destination-side containment (CTO audit FINDING 1) ------------------

    def test_symlinked_prefix_child_fails_closed_no_escape(self) -> None:
        # A pre-existing `prefix/share -> outside` symlink must never redirect
        # a validated payload write outside the prefix. The installer rejects
        # the destination BEFORE any write; BOTH sides stay byte-identical.
        outside = self.root / "outside"
        victim = outside / "faber" / "reference" / "PACK.toml"
        victim.parent.mkdir(parents=True, exist_ok=True)
        victim.write_bytes(b"outside: byte-identical\n")
        prefix = self.prefix
        prefix.mkdir(parents=True, exist_ok=True)
        (prefix / "share").symlink_to(outside, target_is_directory=True)
        marker = prefix / ".fixture-marker"
        marker.write_text("prefix: byte-identical\n", encoding="utf-8")
        prefix_before = {
            p.relative_to(prefix).as_posix(): p.read_bytes()
            for p in prefix.rglob("*") if p.is_file() and not p.is_symlink()
        }
        outside_before = {
            p.relative_to(outside).as_posix(): p.read_bytes()
            for p in outside.rglob("*") if p.is_file()
        }
        res = self.run_installer()
        self.assertEqual(res.returncode, 1)
        self.assertIn("symlink", res.stderr)
        # outside byte-identical: no payload member landed outside the prefix
        outside_after = {
            p.relative_to(outside).as_posix(): p.read_bytes()
            for p in outside.rglob("*") if p.is_file()
        }
        self.assertEqual(outside_after, outside_before)
        self.assertEqual(victim.read_bytes(), b"outside: byte-identical\n")
        # prefix byte-identical: no payload file, no rollback dir, symlink intact
        prefix_after = {
            p.relative_to(prefix).as_posix(): p.read_bytes()
            for p in prefix.rglob("*") if p.is_file() and not p.is_symlink()
        }
        self.assertEqual(prefix_after, prefix_before)
        # the prefix root holds exactly the fixture marker + the share symlink:
        # nothing was created inside the prefix (no bin/, no rollback dir)
        self.assertEqual(sorted(p.name for p in prefix.iterdir()),
                         [".fixture-marker", "share"])
        self.assertTrue((prefix / "share").is_symlink())

    def test_symlinked_receipt_fails_closed_no_escape(self) -> None:
        # The receipt write applies the same containment rule: a pre-existing
        # symlink at the receipt path must not redirect the receipt write
        # outside the prefix.
        prefix = self.prefix
        (prefix / "share" / "faber").mkdir(parents=True, exist_ok=True)
        outside_receipt = self.root / "outside-receipt.json"
        (prefix / "share" / "faber" / "install-receipt.json").symlink_to(
            outside_receipt)
        res = self.run_installer()
        self.assertEqual(res.returncode, 1)
        self.assertIn("symlink", res.stderr)
        self.assertFalse(outside_receipt.exists())
        # no payload was written into the prefix either
        for rel in PAYLOAD_FILES:
            self.assertFalse((prefix / rel).exists(),
                             f"payload file leaked into prefix: {rel}")
        self.assertTrue(
            (prefix / "share" / "faber" / "install-receipt.json").is_symlink())

    def test_add_to_path_rejects_rc_symlink_escaping_home(self) -> None:
        # PATH-owned writes apply the same containment rule: an rc symlink
        # that resolves OUTSIDE home must not receive the PATH append.
        home = self.root / "home"
        rc = home / (".zshrc" if sys.platform == "darwin" else ".bashrc")
        outside_rc = self.root / "outside-rc"
        rc.symlink_to(outside_rc)
        res = self.run_installer("--add-to-path")
        self.assertEqual(res.returncode, 1)
        self.assertIn("outside", res.stderr)
        self.assertFalse(outside_rc.exists())

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
        # Cross-version installs over the same prefix fail closed and name the
        # upgrade path (A2 completes the A1 stub: `faber self update` exists).
        self.assertEqual(self.run_installer().returncode, 0)
        binary_before = (self.prefix / "bin/faber").read_bytes()
        host2 = build_release_host(self.root, version="1.6.0")
        res = self.run_installer(version="1.6.0", base=host2)
        self.assertEqual(res.returncode, 1)
        self.assertIn("faber self update", res.stderr)
        self.assertIn("upgrade", res.stderr)
        self.assertEqual((self.prefix / "bin/faber").read_bytes(), binary_before)

    # -- A2: reinstall matrix + `faber self update` engine -------------------

    def seed_user_data(self, prefix: Path) -> dict[str, bytes]:
        """User project + package store inside the prefix (byte-identity proof)."""
        store = prefix / "cistae" / "pkgs" / "demo" / "p.toml"
        store.parent.mkdir(parents=True, exist_ok=True)
        store.write_text("store-data v1\n", encoding="utf-8")
        proj = prefix / "projects" / "demo" / "faber.toml"
        proj.parent.mkdir(parents=True, exist_ok=True)
        proj.write_text("project-data v1\n", encoding="utf-8")
        lock = prefix / "projects" / "demo" / "faber.lock"
        lock.write_text("lock-data\n", encoding="utf-8")
        return {str(store): store.read_bytes(), str(proj): proj.read_bytes(),
                str(lock): lock.read_bytes()}

    def assert_user_data_unchanged(self, before: dict[str, bytes]) -> None:
        for path, content in before.items():
            self.assertEqual(Path(path).read_bytes(), content, f"{path} changed")

    def test_update_upgrades_side_by_side_and_preserves_user_data(self) -> None:
        self.assertEqual(self.run_installer().returncode, 0)  # 1.5.0
        before = self.seed_user_data(self.prefix)
        host16 = build_release_host(self.root, version="1.6.0")
        res = self.run_installer("--update", version="1.6.0", base=host16)
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("result:     updated (from 1.5.0)", res.stdout)
        self.assertIn("lanes:      1.5.0, 1.6.0 (active: 1.6.0)", res.stdout)
        # active launcher is 1.6.0; both versions present side by side
        self.assertIn("echo faber 1.6.0", (self.prefix / "bin/faber").read_text())
        self.assertIn("echo faber 1.5.0",
                      (self.prefix / "versions/1.5.0/bin/faber").read_text())
        self.assertIn("echo faber 1.6.0",
                      (self.prefix / "versions/1.6.0/bin/faber").read_text())
        # per-lane receipts exist for both versions
        self.assertTrue((self.prefix / "versions/1.5.0/share/faber/install-receipt.json").is_file())
        self.assertTrue((self.prefix / "versions/1.6.0/share/faber/install-receipt.json").is_file())
        # top-level receipt records the active version + side-by-side lanes
        receipt = json.loads((self.prefix / "share/faber/install-receipt.json").read_text())
        self.assertEqual(receipt["version"], "1.6.0")
        self.assertEqual(receipt["laneVersions"], ["1.5.0", "1.6.0"])
        self.assertEqual(receipt["archive"], f"faber-v1.6.0-{TRIPLE}.tar.gz")
        # user project + package store survive byte-identically
        self.assert_user_data_unchanged(before)
        # idempotent re-run of the update: already current, no churn
        res2 = self.run_installer("--update", version="1.6.0", base=host16)
        self.assertEqual(res2.returncode, 0, res2.stdout + res2.stderr)
        self.assertIn("result:     already current", res2.stdout)
        self.assert_user_data_unchanged(before)

    def test_update_same_version_is_already_current(self) -> None:
        self.assertEqual(self.run_installer().returncode, 0)
        res = self.run_installer("--update")
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("result:     already current", res.stdout)
        self.assertIn("0 added, 0 updated, 5 unchanged, 0 removed", res.stdout)

    def test_update_restores_tampered_active_file(self) -> None:
        self.assertEqual(self.run_installer().returncode, 0)
        tampered = self.prefix / "share/faber/reference/PACK.toml"
        tampered.write_text("# tampered\n", encoding="utf-8")
        res = self.run_installer("--update")
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("updated   share/faber/reference/PACK.toml", res.stdout)
        self.assertIn("echo faber 1.5.0", (self.prefix / "bin/faber").read_text())

    def test_cross_lane_update_fails_closed_without_allow_lane_change(self) -> None:
        # 1.5.0 is the odd (development) lane; 2.0.0 is the even (LTS) lane.
        self.assertEqual(self.run_installer().returncode, 0)
        host20 = build_release_host(self.root, version="2.0.0")
        res = self.run_installer("--update", version="2.0.0", base=host20)
        self.assertEqual(res.returncode, 1)
        self.assertIn("cross-lane", res.stderr)
        self.assertIn("--allow-lane-change", res.stderr)
        # fail closed: active version unchanged, no partial lane installed
        self.assertIn("echo faber 1.5.0", (self.prefix / "bin/faber").read_text())
        self.assertFalse((self.prefix / "versions/2.0.0").exists())
        receipt = json.loads((self.prefix / "share/faber/install-receipt.json").read_text())
        self.assertEqual(receipt["version"], "1.5.0")
        # explicit opt-in succeeds (no silent lane jump)
        res2 = self.run_installer("--update", "--allow-lane-change",
                                  version="2.0.0", base=host20)
        self.assertEqual(res2.returncode, 0, res2.stdout + res2.stderr)
        receipt = json.loads((self.prefix / "share/faber/install-receipt.json").read_text())
        self.assertEqual(receipt["version"], "2.0.0")

    def test_downgrade_older_version_succeeds_with_lanes_and_survival(self) -> None:
        # A3: pinned downgrade to an older version succeeds through the A2
        # lane machinery — preserve current lane, install target lane, flip
        # active with rollback; user projects + store survive byte-identically.
        host16 = build_release_host(self.root, version="1.6.0")
        res = self.run_installer(version="1.6.0", base=host16)
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        before = self.seed_user_data(self.prefix)
        res2 = self.run_installer("--update", version="1.5.0", base=self.host)
        self.assertEqual(res2.returncode, 0, res2.stdout + res2.stderr)
        self.assertIn("result:     downgraded (from 1.6.0)", res2.stdout)
        self.assertIn("lanes:      1.5.0, 1.6.0 (active: 1.5.0)", res2.stdout)
        # active launcher is the older version; both versions side by side
        self.assertIn("echo faber 1.5.0", (self.prefix / "bin/faber").read_text())
        self.assertIn("echo faber 1.6.0",
                      (self.prefix / "versions/1.6.0/bin/faber").read_text())
        self.assertIn("echo faber 1.5.0",
                      (self.prefix / "versions/1.5.0/bin/faber").read_text())
        # re-verify --version + explain against the flipped active install
        ver = subprocess.run([str(self.prefix / "bin/faber"), "--version"],
                             capture_output=True, text=True)
        self.assertEqual(ver.stdout.strip(), "faber 1.5.0")
        expl = subprocess.run([str(self.prefix / "bin/faber"), "explain", "loop"],
                              capture_output=True, text=True)
        self.assertIn("fixture 1.5.0", expl.stdout)
        # top-level receipt records the active version + both lanes
        receipt = json.loads((self.prefix / "share/faber/install-receipt.json").read_text())
        self.assertEqual(receipt["version"], "1.5.0")
        self.assertEqual(receipt["laneVersions"], ["1.5.0", "1.6.0"])
        # user project + package store survive byte-identically
        self.assert_user_data_unchanged(before)

    def test_downgrade_then_upgrade_round_trips_through_lanes(self) -> None:
        host16 = build_release_host(self.root, version="1.6.0")
        self.assertEqual(self.run_installer(version="1.6.0", base=host16).returncode, 0)
        res = self.run_installer("--update", version="1.5.0", base=self.host)
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("result:     downgraded (from 1.6.0)", res.stdout)
        # a second downgrade to the same version is already current
        res2 = self.run_installer("--update", version="1.5.0", base=self.host)
        self.assertEqual(res2.returncode, 0, res2.stdout + res2.stderr)
        self.assertIn("result:     already current", res2.stdout)
        # upgrade back reuses the preserved 1.6.0 lane (reversible)
        res3 = self.run_installer("--update", version="1.6.0", base=host16)
        self.assertEqual(res3.returncode, 0, res3.stdout + res3.stderr)
        self.assertIn("result:     updated (from 1.5.0)", res3.stdout)
        self.assertIn("echo faber 1.6.0", (self.prefix / "bin/faber").read_text())
        receipt = json.loads((self.prefix / "share/faber/install-receipt.json").read_text())
        self.assertEqual(receipt["version"], "1.6.0")
        self.assertEqual(receipt["laneVersions"], ["1.5.0", "1.6.0"])

    def write_locked_project(self, lock_text: str) -> Path:
        """Seed a user project under the prefix with the given faber.lock."""
        lock = self.prefix / "projects" / "demo" / "faber.lock"
        lock.parent.mkdir(parents=True, exist_ok=True)
        lock.write_text(lock_text, encoding="utf-8")
        return lock

    def test_downgrade_with_incompatible_lock_entry_fails_closed(self) -> None:
        # A locked pack whose faber_min the target cannot satisfy is stranded
        # by the downgrade (no silent mixing) — the downgrade rejects BEFORE
        # any write with a typed cause + one next action.
        host16 = build_release_host(self.root, version="1.6.0")
        self.assertEqual(self.run_installer(version="1.6.0", base=host16).returncode, 0)
        lock_text = (
            '[[package]]\n'
            'name = "demo"\n'
            'version = "0.1.0"\n'
            'source = "fixture"\n'
            'faber_min = "1.6.0"\n'
        )
        lock = self.write_locked_project(lock_text)
        res = self.run_installer("--update", version="1.5.0", base=self.host)
        self.assertEqual(res.returncode, 1)
        self.assertIn("incompatible-pack-downgrade", res.stderr)
        self.assertIn("faber libraries", res.stderr)
        self.assertIn("`demo`@0.1.0 requires faber >= 1.6.0", res.stderr)
        self.assertIn("next:", res.stderr)
        # fail closed: active version unchanged, no partial downgrade, lock
        # itself untouched
        self.assertIn("echo faber 1.6.0", (self.prefix / "bin/faber").read_text())
        self.assertFalse((self.prefix / "versions").exists())
        receipt = json.loads((self.prefix / "share/faber/install-receipt.json").read_text())
        self.assertEqual(receipt["version"], "1.6.0")
        self.assertEqual(lock.read_text(encoding="utf-8"), lock_text)

    def test_downgrade_incompatible_bound_via_store_manifest_fails_closed(self) -> None:
        # The lock records the package_root; the compatibility bound lives in
        # the locked package's cista.toml (package-and-lock-contract.md
        # OQ3/OQ6) — the same conflict must fail closed.
        host16 = build_release_host(self.root, version="1.6.0")
        self.assertEqual(self.run_installer(version="1.6.0", base=host16).returncode, 0)
        lock_text = (
            '[[package]]\n'
            'name = "demo"\n'
            'version = "0.1.0"\n'
            'source = "fixture"\n'
            'package_root = "../../cistae/pkgs/demo"\n'
        )
        self.write_locked_project(lock_text)
        store_pack = self.prefix / "cistae" / "pkgs" / "demo"
        store_pack.mkdir(parents=True, exist_ok=True)
        (store_pack / "cista.toml").write_text(
            '[source]\npackage = "demo"\nversion = "0.1.0"\nfaber_min = "1.6.0"\n',
            encoding="utf-8",
        )
        res = self.run_installer("--update", version="1.5.0", base=self.host)
        self.assertEqual(res.returncode, 1)
        self.assertIn("incompatible-pack-downgrade", res.stderr)
        self.assertIn("cista.toml", res.stderr)
        self.assertIn("`demo`@0.1.0 requires faber >= 1.6.0", res.stderr)
        self.assertIn("next:", res.stderr)
        # nothing was written: no lanes, active + receipt unchanged
        self.assertIn("echo faber 1.6.0", (self.prefix / "bin/faber").read_text())
        self.assertFalse((self.prefix / "versions").exists())
        receipt = json.loads((self.prefix / "share/faber/install-receipt.json").read_text())
        self.assertEqual(receipt["version"], "1.6.0")

    def test_downgrade_ignores_locked_pack_with_satisfiable_bound(self) -> None:
        # A locked pack whose faber_min the target SATISFIES is not a conflict:
        # the downgrade proceeds and the lock survives byte-identically.
        host16 = build_release_host(self.root, version="1.6.0")
        self.assertEqual(self.run_installer(version="1.6.0", base=host16).returncode, 0)
        lock_text = (
            '[[package]]\n'
            'name = "demo"\n'
            'version = "0.1.0"\n'
            'source = "fixture"\n'
            'faber_min = "1.4.0"\n'
        )
        lock = self.write_locked_project(lock_text)
        res = self.run_installer("--update", version="1.5.0", base=self.host)
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("result:     downgraded (from 1.6.0)", res.stdout)
        self.assertIn("echo faber 1.5.0", (self.prefix / "bin/faber").read_text())
        self.assertEqual(lock.read_text(encoding="utf-8"), lock_text)

    def test_downgrade_blocked_target_lane_fails_closed_no_partial_flip(self) -> None:
        # A pre-existing symlink in the target lane's destination is rejected
        # before the flip (destination-side containment, CTO FINDING 1): the
        # downgrade fails closed and the active version stays unchanged.
        host16 = build_release_host(self.root, version="1.6.0")
        self.assertEqual(self.run_installer(version="1.6.0", base=host16).returncode, 0)
        before = self.seed_user_data(self.prefix)
        outside = self.root / "outside-lane"
        outside.mkdir()
        victim = outside / "victim.txt"
        victim.write_bytes(b"outside: byte-identical\n")
        lane_bin = self.prefix / "versions" / "1.5.0" / "bin"
        lane_bin.mkdir(parents=True, exist_ok=True)
        (lane_bin / "faber").symlink_to(victim)
        res = self.run_installer("--update", version="1.5.0", base=self.host)
        self.assertEqual(res.returncode, 1)
        self.assertIn("symlink", res.stderr)
        # no escape: the outside file was never written through
        self.assertEqual(victim.read_bytes(), b"outside: byte-identical\n")
        self.assertTrue((lane_bin / "faber").is_symlink())
        # fail closed: active version + receipt unchanged, user data untouched
        self.assertIn("echo faber 1.6.0", (self.prefix / "bin/faber").read_text())
        receipt = json.loads((self.prefix / "share/faber/install-receipt.json").read_text())
        self.assertEqual(receipt["version"], "1.6.0")
        self.assert_user_data_unchanged(before)

    def test_cross_lane_downgrade_fails_closed_without_allow_lane_change(self) -> None:
        # 2.0.0 is the even LTS lane; 1.5.0 is the odd development lane
        # (faber/docs/release/policy.md): a downgrade across lanes needs the
        # explicit opt-in — never a silent lane jump.
        host20 = build_release_host(self.root, version="2.0.0")
        self.assertEqual(self.run_installer(version="2.0.0", base=host20).returncode, 0)
        res = self.run_installer("--update", version="1.5.0", base=self.host)
        self.assertEqual(res.returncode, 1)
        self.assertIn("cross-lane", res.stderr)
        self.assertIn("--allow-lane-change", res.stderr)
        self.assertIn("echo faber 2.0.0", (self.prefix / "bin/faber").read_text())
        self.assertFalse((self.prefix / "versions/1.5.0").exists())
        # explicit opt-in succeeds
        res2 = self.run_installer("--update", "--allow-lane-change",
                                  version="1.5.0", base=self.host)
        self.assertEqual(res2.returncode, 0, res2.stdout + res2.stderr)
        self.assertIn("result:     downgraded (from 2.0.0)", res2.stdout)
        receipt = json.loads((self.prefix / "share/faber/install-receipt.json").read_text())
        self.assertEqual(receipt["version"], "1.5.0")

    def test_downgrade_flip_write_failure_rolls_back_active_unchanged(self) -> None:
        # A mid-flip write failure (read-only reference dir) must roll back:
        # the active payload is byte-identical, the receipt still records the
        # newer version, and no rollback dir is left behind — never a partial
        # downgrade.
        if hasattr(os, "geteuid") and os.geteuid() == 0:
            self.skipTest("read-only directories do not block root")
        host16 = build_release_host(self.root, version="1.6.0")
        self.assertEqual(self.run_installer(version="1.6.0", base=host16).returncode, 0)
        before = self.installed_files(self.prefix)
        ref = self.prefix / "share/faber/reference"
        os.chmod(ref, 0o555)
        try:
            res = self.run_installer("--update", version="1.5.0", base=self.host)
        finally:
            os.chmod(ref, 0o755)
        self.assertEqual(res.returncode, 1)
        self.assertIn("failed to write payload", res.stderr)
        # rollback: active payload byte-identical, receipt unchanged
        self.assertEqual(self.installed_files(self.prefix), before)
        receipt = json.loads((self.prefix / "share/faber/install-receipt.json").read_text())
        self.assertEqual(receipt["version"], "1.6.0")
        leftovers = list(self.prefix.glob(".faber-install-rollback-*"))
        self.assertEqual(leftovers, [])

    def test_update_failure_paths_fail_closed_no_partial_install(self) -> None:
        self.assertEqual(self.run_installer().returncode, 0)  # 1.5.0
        before = self.seed_user_data(self.prefix)
        host16 = build_release_host(self.root, version="1.6.0")
        checksum = host16 / (f"faber-v1.6.0-{TRIPLE}.tar.gz" + ".sha256")
        checksum.write_text(f"{'0' * 64}  faber-v1.6.0-{TRIPLE}.tar.gz\n", encoding="utf-8")
        res = self.run_installer("--update", version="1.6.0", base=host16)
        self.assertEqual(res.returncode, 1)
        self.assertIn("SHA-256 mismatch", res.stderr)
        self.assertIn("NO partial install", res.stderr)
        # version unchanged, no lane installed, user data untouched
        self.assertIn("echo faber 1.5.0", (self.prefix / "bin/faber").read_text())
        self.assertFalse((self.prefix / "versions").exists())
        receipt = json.loads((self.prefix / "share/faber/install-receipt.json").read_text())
        self.assertEqual(receipt["version"], "1.5.0")
        self.assert_user_data_unchanged(before)

    def test_update_requires_existing_install(self) -> None:
        res = self.run_installer("--update")
        self.assertEqual(res.returncode, 1)
        self.assertIn("receipt", res.stderr)

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
