#!/usr/bin/env python3
"""Fixture tests for faber/scripta/release-doctor.

Proves D7 of component-release-streamline Stage 3: the doctor passes on a
clean prepared candidate and fails with NAMED reasons on a dirty tree, wrong
remote, version/tag mismatch, stale lock, missing notes, missing dev-kit
packs, pin mismatches, and ambient release credentials. It never proceeds
past the would-tag / would-upload plan placeholders.

The dev-kit pack fixture is the REAL assemble-dev-kit payload layout
(`bin/faber`, `share/faber/reference/`, `share/faber/locale/<locale>/` plus
the embedded core-support and library-pack source sets) — never flat files
named after the pack rows.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("release-doctor")
GENERATOR = Path(__file__).with_name("generate-release-manifest")
VERSION = "1.5.0"
LOCALES = ["en", "la"]


# -- digest helpers — mirror the doctor's recomputations --------------------

def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_digest(root: Path) -> str:
    entries: list[tuple[str, str]] = []
    for path in sorted(root.rglob("*")):
        if path.is_file():
            rel = path.relative_to(root).as_posix()
            entries.append((rel, sha256_file(path)))
    canonical = "\n".join(f"{rel}\t{digest}" for rel, digest in sorted(entries))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def tree_digest_with_prefix(prefix: str, root: Path) -> str:
    entries: list[tuple[str, str]] = []
    for path in sorted(root.rglob("*")):
        if path.is_file():
            rel = f"{prefix}/{path.relative_to(root).as_posix()}"
            entries.append((rel, sha256_file(path)))
    canonical = "\n".join(f"{rel}\t{digest}" for rel, digest in sorted(entries))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def locale_packs_digest(locale_root: Path) -> str:
    per_locale = {
        d.name: tree_digest(d)
        for d in sorted(locale_root.iterdir())
        if d.is_dir()
    }
    return hashlib.sha256(
        "\n".join(f"{loc}\t{dg}" for loc, dg in sorted(per_locale.items())).encode("utf-8")
    ).hexdigest()


def core_support_digest(container: Path) -> str:
    # mirror the doctor: the faber root is resolved, so the container must be
    # resolved before computing container-relative paths (macOS /var symlink)
    container = container.resolve()
    manifest = container / "faber" / "core-support-manifest.txt"
    roots = []
    for line in manifest.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        roots.append((container / line).resolve())
    entries = [
        (root.relative_to(container).as_posix(), tree_digest(root))
        for root in roots
    ]
    canonical = "\n".join(f"{rel}\t{dg}" for rel, dg in sorted(entries))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


# -- fixture plumbing --------------------------------------------------------

def git(repo: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", "-C", str(repo), *args],
        check=False, capture_output=True, text=True,
    )


def git_init(repo: Path) -> None:
    repo.mkdir(parents=True, exist_ok=True)
    git(repo, "init", "-q", "-b", "main")
    git(repo, "config", "user.email", "doctor-test@example.com")
    git(repo, "config", "user.name", "doctor-test")


def write(repo: Path, name: str, content: str | bytes) -> None:
    path = repo / name
    path.parent.mkdir(parents=True, exist_ok=True)
    mode = "wb" if isinstance(content, bytes) else "w"
    kwargs = {} if isinstance(content, bytes) else {"encoding": "utf-8"}
    with open(path, mode, **kwargs) as fh:
        fh.write(content)


def git_commit(repo: Path, message: str) -> str:
    git(repo, "add", "-A")
    res = git(repo, "commit", "-q", "-m", message)
    if res.returncode != 0:
        raise RuntimeError(f"fixture commit failed: {res.stderr}")
    head = git(repo, "rev-parse", "HEAD")
    return head.stdout.strip()


def head_of(repo: Path) -> str:
    res = git(repo, "rev-parse", "HEAD")
    return res.stdout.strip()


class ReleaseDoctorTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.container = Path(self.tmp.name)
        self.faber = self.container / "faber"
        self.radix = self.container / "radix"
        self.cista = self.container / "cista"
        self.runtime = self.container / "faber-runtime"
        self.hosts = self.container / "hosts"
        self.norma = self.container / "norma"
        self.packs_dir = self.container / "dev-kit-1.5.0-aarch64-apple-darwin"
        self.build_candidate()
        self.generate_manifest()

    def tearDown(self) -> None:
        self.tmp.cleanup()

    # -- fixture ----------------------------------------------------------

    def build_candidate(self) -> None:
        for repo in (self.faber, self.radix, self.cista, self.runtime,
                     self.hosts, self.norma):
            git_init(repo)
        write(self.faber, "Cargo.toml",
              '[package]\nname = "faber"\nversion = "1.5.0"\nedition = "2021"\n\n'
              '[workspace]\nmembers = [\n'
              '    ".",\n    "crates/exempla",\n    "crates/faber-hir-rust",\n'
              '    "crates/hygiene-ratchet",\n]\nresolver = "2"\n')
        write(self.faber, "crates/exempla/Cargo.toml",
              '[package]\nname = "exempla"\nversion = "0.1.0"\nedition = "2021"\n')
        write(self.faber, "crates/faber-hir-rust/Cargo.toml",
              '[package]\nname = "faber-hir-rust"\nversion = "1.5.0"\n'
              'edition = "2021"\n')
        write(self.faber, "crates/hygiene-ratchet/Cargo.toml",
              '[package]\nname = "hygiene-ratchet"\nversion = "0.1.0"\n'
              'edition = "2021"\n')
        write(self.faber, "Cargo.lock",
              'version = 4\n\n'
              '[[package]]\nname = "faber"\nversion = "1.5.0"\n\n'
              '[[package]]\nname = "exempla"\nversion = "0.1.0"\n\n'
              '[[package]]\nname = "faber-hir-rust"\nversion = "1.5.0"\n\n'
              '[[package]]\nname = "hygiene-ratchet"\nversion = "0.1.0"\n')
        write(self.faber, "src/main.rs", "fn main() {}\n")
        write(self.faber, "docs/release/v1.5.0.md",
              "# faber v1.5.0\n\nDraft release notes.\n")
        # core-support source set: the embedded pack's digest recomputes from
        # these container-relative roots (same as scripta/assemble-dev-kit)
        write(self.faber, "core-support-manifest.txt", "faber-runtime\n")
        git(self.faber, "remote", "add", "origin",
            "https://github.com/faberlang/faber.git")
        git_commit(self.faber, "faber candidate")

        write(self.radix, "crates/radix/Cargo.toml",
              '[package]\nname = "radix"\nversion = "0.80.0"\nedition = "2021"\n')
        git_commit(self.radix, "radix")
        write(self.cista, "Cargo.toml",
              '[package]\nname = "cista"\nversion = "0.2.0"\nedition = "2021"\n')
        git_commit(self.cista, "cista")
        write(self.runtime, "Cargo.toml",
              '[package]\nname = "faber-runtime"\nversion = "0.1.0"\n'
              'edition = "2021"\n')
        git_commit(self.runtime, "faber-runtime")
        write(self.hosts, "Cargo.toml",
              '[package]\nname = "hosts"\nversion = "0.1.0"\nedition = "2021"\n')
        git_commit(self.hosts, "hosts")
        write(self.norma, "src/plato.toml", 'name = "plato"\n')
        git_commit(self.norma, "norma")

    def build_payload(self) -> None:
        """The real assemble-dev-kit output layout (a directory tree, not
        flat files named after pack rows)."""
        bin_dir = self.packs_dir / "bin"
        reference = self.packs_dir / "share" / "faber" / "reference"
        locale = self.packs_dir / "share" / "faber" / "locale"
        bin_dir.mkdir(parents=True)
        reference.mkdir(parents=True)
        for name in LOCALES:
            (locale / name).mkdir(parents=True)

        (bin_dir / "faber").write_bytes(b"#!/bin/sh\necho faber 1.5.0\n")
        (bin_dir / "faber").chmod(0o755)
        (reference / "index.toml").write_text('terms = []\n', encoding="utf-8")
        (reference / "PACK.toml").write_text(
            'faber_version = "1.5.0"\n', encoding="utf-8")
        (reference / "legacy-redirects.toml").write_text(
            'redirects = []\n', encoding="utf-8")
        for name in LOCALES:
            (locale / name / "pack.toml").write_text(
                f'[locale]\nname = "{name}"\n', encoding="utf-8")

    def generate_manifest(self) -> None:
        self.build_payload()
        packs = [
            {"name": "launcher", "component": "faber", "version": VERSION,
             "digest": f"sha256:{sha256_file(self.packs_dir / 'bin' / 'faber')}",
             "compatibility": "1.x", "license": "MIT",
             "destination": "bin/faber"},
            {"name": "core-support", "component": "faber", "version": VERSION,
             "digest": f"sha256:{core_support_digest(self.container)}",
             "compatibility": "1.x", "license": "MIT",
             "destination": "embedded in launcher"},
            {"name": "reference-pack", "component": "reference",
             "version": VERSION,
             "digest": f"sha256:{tree_digest(self.packs_dir / 'share/faber/reference')}",
             "compatibility": "1.x", "license": "MIT",
             "destination": "share/faber/reference"},
            {"name": "locale-packs", "component": "locale", "version": VERSION,
             "digest": f"sha256:{locale_packs_digest(self.packs_dir / 'share/faber/locale')}",
             "compatibility": "1.x", "license": "MIT",
             "destination": "share/faber/locale/<locale>/pack.toml"},
            {"name": "library-pack", "component": "norma", "version": "0.1.0",
             "digest": f"sha256:{tree_digest_with_prefix('norma', (self.norma / 'src').resolve())}",
             "compatibility": "1.x", "license": "MIT",
             "destination": "store seeding"},
        ]
        args = [
            sys.executable, str(GENERATOR), "--root", str(self.faber),
            "--version", VERSION, "--channel", "stable", "--line", "1.x",
            "--faber-sha", head_of(self.faber),
            "--radix-sha", head_of(self.radix),
            "--cista-sha", head_of(self.cista),
            "--faber-runtime-sha", head_of(self.runtime),
            "--hosts-sha", head_of(self.hosts),
            "--packs", json.dumps(packs),
        ]
        res = subprocess.run(args, check=False, capture_output=True, text=True)
        if res.returncode != 0:
            raise RuntimeError(f"manifest generation failed: {res.stdout}{res.stderr}")
        # commit the freeze artifact (the bump+lock single commit of a release)
        git_commit(self.faber, "manifest freeze artifact")

    # -- runner -----------------------------------------------------------

    def run_doctor(self, *extra: str, env: dict | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(self.faber),
             "--version", VERSION, "--packs-dir", str(self.packs_dir), *extra],
            check=False, capture_output=True, text=True,
            env=env,
        )

    # -- tests ------------------------------------------------------------

    def test_passes_on_clean_prepared_candidate(self) -> None:
        res = self.run_doctor()
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("would-tag", res.stdout)
        plan = self.container / "out" / f"release-plan-{VERSION}.yaml"
        self.assertTrue(plan.exists())
        text = plan.read_text(encoding="utf-8")
        self.assertIn("would-tag", text)
        self.assertIn("would-upload", text)
        self.assertIn("git tag -a v1.5.0", text)
        self.assertIn("gh release create faber-v1.5.0", text)

    def test_fails_on_dirty_tree(self) -> None:
        write(self.faber, "src/main.rs", "fn main() { println!(\"x\"); }\n")
        res = self.run_doctor()
        self.assertEqual(res.returncode, 1)
        self.assertIn("dirty tree", res.stderr)

    def test_fails_on_wrong_remote(self) -> None:
        git(self.faber, "remote", "set-url", "origin",
            "https://github.com/someone-else/faber.git")
        res = self.run_doctor()
        self.assertEqual(res.returncode, 1)
        self.assertIn("wrong remote", res.stderr)

    def test_fails_on_version_mismatch(self) -> None:
        res = self.run_doctor("--version", "9.9.9")
        self.assertEqual(res.returncode, 1)
        self.assertIn("version mismatch", res.stderr)

    def test_fails_when_tag_already_exists(self) -> None:
        git(self.faber, "tag", f"v{VERSION}")
        res = self.run_doctor()
        self.assertEqual(res.returncode, 1)
        self.assertIn("already exists", res.stderr)

    def test_fails_on_stale_lock(self) -> None:
        lock = self.faber / "Cargo.lock"
        lock.write_text(lock.read_text(encoding="utf-8").replace(
            'name = "faber"\nversion = "1.5.0"',
            'name = "faber"\nversion = "1.4.0"'), encoding="utf-8")
        res = self.run_doctor()
        self.assertEqual(res.returncode, 1)
        self.assertIn("stale", res.stderr)

    def test_fails_on_missing_release_notes(self) -> None:
        (self.faber / "docs/release/v1.5.0.md").unlink()
        res = self.run_doctor()
        self.assertEqual(res.returncode, 1)
        self.assertIn("missing release notes", res.stderr)

    def test_fails_without_packs_dir(self) -> None:
        res = self.run_doctor("--packs-dir", str(self.container / "nope"))
        self.assertEqual(res.returncode, 1)
        self.assertIn("dev-kit packs", res.stderr)

    def test_fails_on_missing_pack_artifact(self) -> None:
        (self.packs_dir / "bin" / "faber").unlink()
        res = self.run_doctor()
        self.assertEqual(res.returncode, 1)
        self.assertIn("launcher", res.stderr)

    def test_fails_on_pack_digest_mismatch(self) -> None:
        (self.packs_dir / "bin" / "faber").write_bytes(b"tampered\n")
        res = self.run_doctor()
        self.assertEqual(res.returncode, 1)
        self.assertIn("digest mismatch", res.stderr)

    def test_fails_on_tampered_locale_pack_tree(self) -> None:
        la = self.packs_dir / "share" / "faber" / "locale" / "la" / "pack.toml"
        la.write_text(la.read_text(encoding="utf-8") + "# tampered\n",
                      encoding="utf-8")
        res = self.run_doctor()
        self.assertEqual(res.returncode, 1)
        self.assertIn("digest mismatch", res.stderr)
        self.assertIn("locale-packs", res.stderr)

    def test_fails_on_ambient_release_credentials(self) -> None:
        env = {**os.environ, "FABERLANG_RELEASES_TOKEN": "not-a-real-token"}
        res = self.run_doctor(env=env)
        self.assertEqual(res.returncode, 1)
        self.assertIn("ambient release credentials", res.stderr)

    def test_fails_on_pin_mismatch_for_sibling(self) -> None:
        # move the radix checkout HEAD after the manifest was pinned
        write(self.radix, "crates/radix/Cargo.toml",
              '[package]\nname = "radix"\nversion = "0.80.0"\nedition = "2021"\n'
              '# drift\n')
        git_commit(self.radix, "drift")
        res = self.run_doctor()
        self.assertEqual(res.returncode, 1)
        self.assertIn("pin mismatch", res.stderr)
        self.assertIn("radix", res.stderr)

    def test_fails_on_stale_faber_pin(self) -> None:
        # two commits past the pinned source commit → pin is neither HEAD
        # nor its parent
        write(self.faber, "extra1.txt", "1\n")
        git_commit(self.faber, "extra 1")
        write(self.faber, "extra2.txt", "2\n")
        git_commit(self.faber, "extra 2")
        res = self.run_doctor()
        self.assertEqual(res.returncode, 1)
        self.assertIn("pin mismatch", res.stderr)
        self.assertIn("faber", res.stderr)

    def test_fails_on_invalid_manifest(self) -> None:
        manifest = self.faber / "release-manifest.yaml"
        manifest.write_text(manifest.read_text(encoding="utf-8").replace(
            "channel: stable", "channel: nonsense"), encoding="utf-8")
        res = self.run_doctor()
        self.assertEqual(res.returncode, 1)
        self.assertIn("pin manifest invalid", res.stderr)


if __name__ == "__main__":
    unittest.main()
