#!/usr/bin/env python3
"""Fixture tests for faber/scripta/validate-release-manifest.

Proves D5 of component-release-streamline Stage 3: the validator accepts the
faber-onboarding Stage-2 instance shape (pinnedInputs.packs rows per
release-manifest-schema.md §6) and rejects malformed input with named
reasons; the schema JSON mirrors the documented schema section-by-section
via the shared JSON-Schema engine.
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

SCRIPT = Path(__file__).with_name("validate-release-manifest")

VALID_INSTANCE = """\
schemaVersion: "1"
manifestName: faber-release
preparedAt: 2026-08-07
releaseIntent:
  component: faber
  version: "1.5.0"
  channel: stable
  line: "1.x"
pinnedInputs:
  source:
    - name: faber
      commit: 3f1a2b4c5d6e7f8091a2b3c4d5e6f708192a3b4c
      tag: v1.5.0
    - name: radix
      commit: 5bbdbbd49c0d1e2f3a4b5c6d7e8f9001a2b3c4d5
    - name: cista
      commit: 99acb1e2f3a4b5c6d7e8f90a1b2c3d4e5f607182
    - name: faber-runtime
      commit: 57493dc1e2f3a4b5c6d7e8f90a1b2c3d4e5f60718
    - name: hosts
      commit: ced40f81a2b3c4d5e6f708192a3b4c5d6e7f8091
  packs:
    - name: launcher
      component: faber
      version: "1.5.0"
      digest: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
      compatibility: "1.x"
      license: MIT
      destination: bin/faber
    - name: core-support
      component: faber
      version: "1.5.0"
      digest: sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210
      compatibility: "1.x"
      license: MIT
      destination: share/faber/core-support/
    - name: reference-pack
      component: reference
      version: "1.5.0"
      digest: sha256:0000111122223333444455556666777788889999aaaabbbbccccddddeeeeffff
      compatibility: "1.x"
      license: MIT
      destination: share/faber/reference/
versionSources:
  - component: faber
    manifest: Cargo.toml
    version: "1.5.0"
  - component: radix
    manifest: crates/radix/Cargo.toml
    version: "0.80.0"
  - component: cista
    manifest: Cargo.toml
    version: "0.2.0"
exclusions:
  - component: faber
    path: crates/hygiene-ratchet
    reason: not release-aligned; stays 0.1.0
  - component: radix
    path: crates/hygiene-ratchet
    reason: not release-aligned; stays 0.1.0
  - component: cista
    path: crates/hygiene-ratchet
    reason: not release-aligned; stays 0.1.0
publication:
  releaseTag: faber-v1.5.0
  host: faberlang/releases
  advancesGlobalLatest: true
"""


def load_module():
    loader = importlib.machinery.SourceFileLoader(
        "faber_validate_release_manifest", str(SCRIPT)
    )
    spec = importlib.util.spec_from_loader("faber_validate_release_manifest", loader)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    loader.exec_module(module)
    return module


def swap(text: str, old: str, new: str) -> str:
    assert old in text, f"fixture missing {old!r}"
    return text.replace(old, new)


class ValidateReleaseManifestTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)
        self.module = load_module()

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def write_instance(self, text: str, name: str = "release-manifest.yaml") -> Path:
        path = self.dir / name
        path.write_text(text, encoding="utf-8")
        return path

    def run_script(self, path: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), str(path)],
            check=False,
            capture_output=True,
            text=True,
        )

    def errors(self, text: str) -> list[str]:
        path = self.write_instance(text)
        return self.module.validate_instance_file(path)

    def test_valid_instance_has_no_errors(self) -> None:
        self.assertEqual(self.errors(VALID_INSTANCE), [])

    def test_committed_example_validates(self) -> None:
        example = Path(__file__).resolve().parents[1] / "docs/release" \
            / "release-manifest.example.yaml"
        self.assertTrue(example.exists(), str(example))
        res = self.run_script(example)
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)

    def test_valid_instance_passes_cli(self) -> None:
        path = self.write_instance(VALID_INSTANCE)
        res = self.run_script(path)
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("validates", res.stdout)

    def test_json_instance_is_accepted(self) -> None:
        data = self.module.parse_manifest(VALID_INSTANCE)
        path = self.write_instance(json.dumps(data, indent=2), "instance.json")
        res = self.run_script(path)
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)

    def test_missing_required_top_level_field(self) -> None:
        errors = self.errors(swap(VALID_INSTANCE,
                                  "  releaseTag: faber-v1.5.0\n", ""))
        self.assertTrue(any("publication" in e and "releaseTag" in e for e in errors),
                        errors)

    def test_bad_channel_rejected(self) -> None:
        errors = self.errors(swap(VALID_INSTANCE, "  channel: stable",
                                  "  channel: nightly"))
        self.assertTrue(any("channel" in e for e in errors), errors)

    def test_bad_digest_shape_rejected(self) -> None:
        errors = self.errors(swap(
            VALID_INSTANCE,
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "sha256:tooshort"))
        self.assertTrue(any("digest" in e for e in errors), errors)

    def test_wrong_schema_version_rejected(self) -> None:
        errors = self.errors(swap(VALID_INSTANCE, 'schemaVersion: "1"',
                                  'schemaVersion: "2"'))
        self.assertTrue(any("schemaVersion" in e for e in errors), errors)

    def test_boolean_as_string_rejected(self) -> None:
        errors = self.errors(swap(VALID_INSTANCE,
                                  "  advancesGlobalLatest: true",
                                  "  advancesGlobalLatest: \"true\""))
        self.assertTrue(any("boolean" in e for e in errors), errors)

    def test_additional_property_rejected(self) -> None:
        errors = self.errors(swap(VALID_INSTANCE,
                                  "  channel: stable",
                                  "  channel: stable\n  sneaky: yes"))
        self.assertTrue(any("additional property" in e for e in errors), errors)

    def test_malformed_yaml_rejected(self) -> None:
        bad = swap(VALID_INSTANCE, "pinnedInputs:", "\tpinnedInputs:")
        errors = self.errors(bad)
        self.assertTrue(any("malformed" in e for e in errors), errors)

    def test_commit_sha_short_form_accepted(self) -> None:
        # Short-form SHAs (7+ hex) are valid pins.
        text = swap(VALID_INSTANCE, "5bbdbbd49c0d1e2f3a4b5c6d7e8f9001a2b3c4d5",
                    "5bbdbbd")
        self.assertEqual(self.errors(text), [])

    def test_missing_file_returns_exit_2(self) -> None:
        res = self.run_script(self.dir / "does-not-exist.yaml")
        self.assertEqual(res.returncode, 2)

    def test_invalid_instance_returns_exit_1_with_reasons(self) -> None:
        path = self.write_instance(swap(VALID_INSTANCE, "  channel: stable",
                                        "  channel: nightly"))
        res = self.run_script(path)
        self.assertEqual(res.returncode, 1)
        self.assertIn("channel", res.stderr)

    def test_pack_row_missing_field_rejected(self) -> None:
        text = swap(VALID_INSTANCE, "      license: MIT\n", "")
        errors = self.errors(text)
        self.assertTrue(any("license" in e for e in errors), errors)

    def test_unsupported_schema_keyword_hard_fails(self) -> None:
        # head-cxo RR-2: silent ignore is a hard error. A $ref anywhere in
        # the schema fails with a named keyword, even for a valid instance.
        schema = self.module.load_schema()
        schema["$ref"] = "#/$defs/anything"
        schema_path = self.dir / "ref.schema.json"
        schema_path.write_text(json.dumps(schema), encoding="utf-8")
        instance_path = self.write_instance(VALID_INSTANCE)
        errors = self.module.validate_instance_file(instance_path, schema_path)
        self.assertTrue(
            any("unsupported schema keyword" in e and "'$ref'" in e
                for e in errors),
            errors,
        )

    def test_unsupported_schema_keyword_nested_names_path(self) -> None:
        # The gate is schema-driven and names the offending keyword + path.
        schema = self.module.load_schema()
        schema["properties"]["preparedAt"]["minimum"] = "2026-01-01"
        schema_path = self.dir / "minimum.schema.json"
        schema_path.write_text(json.dumps(schema), encoding="utf-8")
        instance_path = self.write_instance(VALID_INSTANCE)
        errors = self.module.validate_instance_file(instance_path, schema_path)
        self.assertTrue(
            any("unsupported schema keyword" in e and "'minimum'" in e
                and "preparedAt" in e for e in errors),
            errors,
        )

    def test_unsupported_schema_keyword_cli_exit_1(self) -> None:
        schema = self.module.load_schema()
        schema["properties"]["preparedAt"]["minLength"] = 10
        schema_path = self.dir / "minlength.schema.json"
        schema_path.write_text(json.dumps(schema), encoding="utf-8")
        path = self.write_instance(VALID_INSTANCE)
        res = subprocess.run(
            [sys.executable, str(SCRIPT), str(path), "--schema", str(schema_path)],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("unsupported schema keyword", res.stderr)
        self.assertIn("minLength", res.stderr)

    def test_schema_section_anchors(self) -> None:
        # Section-by-section mapping guard: the schema JSON carries the §2–§7
        # decisions. Assert the decisions the docs pin.
        schema = self.module.load_schema()
        self.assertEqual(schema["properties"]["schemaVersion"]["const"], "1")
        self.assertEqual(schema["properties"]["manifestName"]["const"], "faber-release")
        self.assertEqual(schema["properties"]["pinnedInputs"]["required"],
                         ["source", "packs"])
        pack_req = schema["properties"]["pinnedInputs"]["properties"]["packs"] \
                       ["items"]["required"]
        self.assertEqual(pack_req, ["name", "component", "version", "digest",
                                    "compatibility", "license", "destination"])
        self.assertEqual(schema["properties"]["publication"]["properties"]["host"]["const"],
                         "faberlang/releases")
        channel = schema["properties"]["releaseIntent"]["properties"]["channel"]["enum"]
        self.assertEqual(channel, ["development", "candidate", "stable", "lts",
                                   "hotfix"])


if __name__ == "__main__":
    unittest.main()
