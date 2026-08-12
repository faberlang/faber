# Install-page reference (faber side) — for faberlang.dev

The faber-side reference the `faberlang.dev` start/install pages must name
(faber-onboarding Stage 3, unit D1). This file pins the **exact
artifact/version/digest under test** and lists **every command the install
pages will cite**, each verified to exist at the tested release
(zombie-docs check, 2026-08-12). The site page content itself is owned by
`faberlang.dev` — see Routing.

## Tested release

- **Version:** faber **1.6.0** (`faber/Cargo.toml`; the installer's
  `DEFAULT_VERSION`; the release manifest's `releaseIntent.version`).
- **CLI surface verified at:** faber main tip `6acb753` (2026-08-12) — the
  integration surface carrying Batch A (`scripta/install-faber`, `faber
  self update`, `faber self uninstall`).
- **Supported triples** (`platform-matrix.md` OQ9): `aarch64-apple-darwin`
  (macOS arm64) and `x86_64-unknown-linux-gnu` (Linux x86_64). All other
  platforms are named residuals and fail closed.

## Exact artifact, version, and digest under test

### Archive (published per platform triple)

| Field | Value |
| --- | --- |
| Artifact name | `faber-v1.6.0-<triple>.tar.gz` (basename-only `.sha256` asset beside it) |
| Version | `1.6.0` |
| Archive SHA-256 | **published by the release process (R1) — not yet published at this date.** `component-release-streamline` Stages 2–3 (wrap + publish) have not landed; the archive digest cannot be truthfully cited before publish. Do not fabricate one on the pages. |

Archive naming and the basename-only checksum rule follow
`component-release-streamline` `release-contract.md` §5.1; the wrapper that
produces the exact published shape is `faber/scripta/package-archive` (the
shape the installer tests against).

### Canonical Stage 2 payload digests (verified, committed)

`faber/release-manifest.yaml` — freeze prepared 2026-08-08 by
`scripta/assemble-dev-kit` (the manifest instance; regenerated at each release
prepare, schema §7):

| Pack | Component | Version | Digest (sha256) |
| --- | --- | --- | --- |
| launcher | faber | 1.6.0 | `8b45f0f7d193b62f59538cc7e870c4538946090cd620fcb669815df05c9a0eb4` |
| core-support | faber | 1.6.0 | `88ae432d4be7df98e85dba7d6debc50ab84b8171c7bbd5498f6583b18c02ca5b` |
| reference-pack | reference | 1.6.0 | `8ffdd60ba4e00f062c806383883b7b869fe9235a44e3f61d30d1850693c6445f` |
| locale-packs | locale | 1.6.0 | `cea681b54b3724c600fd28662bde758312e9504eaffa1cdb4e41d507d5da283d` |
| library-pack | norma | 0.1.0 | `cb1bcfa8a5d880d354f77ef8d43444b27aa31e66eb95f4e1770876b920dec544` |

Pinned source commits (same freeze): faber `6dbda59b…`, radix `a85d802d…`,
cista `93552882…`, faber-runtime `6d42d8d2…`, hosts `c7f1ff97…`.

These are the checksums the installer verifies **before** any payload is
unpacked or executed (checksum policy, `install-channel-matrix.md`); the
archive-level digest joins the table when R1 publishes.

## Commands the install pages may cite (zombie-docs verified)

Every command below exists at the tested release; each was verified against
its live `--help` output on 2026-08-12. Pages must not cite other commands as
supported install-surface without re-verifying.

| Command | Purpose | Verified |
| --- | --- | --- |
| `python3 scripta/install-faber` | Verified bootstrap installer (user-local prefix default `~/.faber`) | `scripta/install-faber --help` |
| `curl -fsSL https://github.com/faberlang/releases/releases/download/faber-v1.6.0/install-faber \| python3 - --version 1.6.0` | Convenience path (no checkout; needs only python3) | same engine; script published at the release tag per R1 — label as convenience until the tag exists |
| `faber --version` | Post-install proof | binary `--version` |
| `scripta/verify-dev-kit --payload <prefix> --version 1.6.0` | Clean-room verify of the installed kit | `scripta/verify-dev-kit --help` |
| `faber self update --version <target>` | Upgrade / pinned downgrade | `faber self --help` + `faber self update --help` |
| `faber self uninstall [--purge]` | Uninstall (product-owned only; `--purge` also removes user data explicitly) | `faber self --help` + `faber self uninstall --help` |
| `faber explain SEM001` | Non-build verification (reference pack lookup) | `faber explain` in the shipped CLI help |

**curl|sh marketing gate — HELD.** CAMPAIGN §Dependency rules: *"one curl |
sh" is only presented once Stage 3 installs and verifies the canonical Stage 2
payload.* State at this date: **not presented.** The tested path is the fixed
release payload download (the `curl | python3` bootstrap above); the marketing
one-liner is not authorized until the canonical payload has been installed and
verified through the published channel (R1) at the Stage 3 gate. The install
pages must not present `curl | sh` as a supported one-liner before that.

**Formula-lag (G7).** Homebrew remains a secondary, non-authoritative channel;
pages label it non-authoritative and state the true served version. Site
wording is routed to faberlang.dev (see Routing).

**Prerequisites (gate decision 3).** Pages state prerequisites explicitly: the
released execution path requires what the released faber requires (the kit
plus, where the default execution target needs it, the system Rust toolchain)
— the portable no-Rust path is not claimed as released. No unreleased no-rust
claim is made as current proof.

## Routing

- **Site pages:** `faberlang.dev` start/install pages (install, lifecycle,
  agent) are **routed needs** to the faberlang.dev side via faber-onboarding
  decision-ledger G3/G6/G7 and `delivery-stage3.md` D1–D3 routed-needs rows;
  faberlang.dev owns the page content. **faber does not write faberlang.dev.**
  The pages must name the artifact/version/digest pinned in this reference and
  re-run the zombie-docs check against their cited release.
- **Release publish (R1):** `component-release-streamline` owns wrap + publish
  per platform triple and the published checksum/provenance assets; the archive
  digest row above fills when that lands. Unsigned/unproven remains a labeled
  residual (G8) until signing lands — never a silent "supported signed
  channel" claim.
- **Stage 9:** the JSON diagnostics and agent-skill surface stays staged
  (`docs/install/agent-installer.md` — labeled, not implemented).

## Residuals

- Archive-level SHA-256 unavailable until R1 publish (hold, not fabricated).
- No `.pkg`/`.dmg`; native macOS packaging explicitly deferred (OQ8).
- Homebrew formula lag labeled per G7; site wording routed.
- curl|sh marketing one-liner held per the CAMPAIGN dependency gate.
