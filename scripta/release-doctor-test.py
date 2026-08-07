#!/usr/bin/env python3
"""Fixture tests for faber/scripta/release-doctor.

Proves D7 of component-release-streamline Stage 3: the doctor passes on a
clean prepared candidate and fails with NAMED reasons on a dirty tree, wrong
remote, version/tag mismatch, stale lock, missing notes, missing dev-kit
packs, pin mismatches, and ambient release credentials. It never proceeds
past the would-tag / would-upload plan placeholders.
"""

from __future__ import annotations

import hashlib
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("release-doctor")
GENERATOR = Path(__file__).with_name("generate-release-manifest")
VERSION = "1.5.0"

PACK_BYTES = {
    "launcher": b"#!/bin/sh\necho faber launcher\n",
    "core-support": b"core support payload\n",
}


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
        self.packs_dir = self.container / "packs"
        self.build_candidate()
        self.generate_manifest()

    def tearDown(self) -> None:
        self.tmp.cleanup()

    # -- fixture ----------------------------------------------------------

    def build_candidate(self) -> None:
        for repo in (self.faber, self.radix, self.cista, self.runtime, self.hosts):
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

    def generate_manifest(self) -> None:
        packs: list[str] = []
        self.packs_dir.mkdir(parents=True, exist_ok=True)
        for name, content in PACK_BYTES.items():
            artifact = self.packs_dir / name
            artifact.write_bytes(content)
            packs.append(
                '{{"name": "{name}", "component": "faber", "version": "1.5.0", '
                '"digest": "sha256:{digest}", "compatibility": "1.x", '
                '"license": "MIT", "destination": "{name}"}}'.format(
                    name=name,
                    digest=hashlib.sha256(content).hexdigest()))
        packs_json = "[" + ",".join(packs) + "]"
        args = [
            sys.executable, str(GENERATOR), "--root", str(self.faber),
            "--version", VERSION, "--channel", "stable", "--line", "1.x",
            "--faber-sha", head_of(self.faber),
            "--radix-sha", head_of(self.radix),
            "--cista-sha", head_of(self.cista),
            "--faber-runtime-sha", head_of(self.runtime),
            "--hosts-sha", head_of(self.hosts),
            "--packs", packs_json,
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
        (self.packs_dir / "launcher").unlink()
        res = self.run_doctor()
        self.assertEqual(res.returncode, 1)
        self.assertIn("launcher", res.stderr)

    def test_fails_on_pack_digest_mismatch(self) -> None:
        (self.packs_dir / "launcher").write_bytes(b"tampered\n")
        res = self.run_doctor()
        self.assertEqual(res.returncode, 1)
        self.assertIn("digest mismatch", res.stderr)

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
