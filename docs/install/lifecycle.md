# Faber lifecycle: install, update, downgrade, reinstall, uninstall

User-facing lifecycle documentation for the primary Faber install channel.
Every command and flag documented here exists on the CLI at the **tested
release: faber 1.6.0** (zombie-docs verified 2026-08-12 — `faber self --help`,
`scripta/install-faber --help`, `scripta/verify-dev-kit --help`). Contract
sources: `docs/factory/faber-onboarding/` decision records
(`install-channel-matrix.md` prefix rules, `dev-kit-contract.md` layers,
`package-and-lock-contract.md` store semantics) and `docs/release/policy.md`
version lanes. The site-facing wording for the same lifecycle is owned by
`faberlang.dev` (routed via faber-onboarding Stage 8); this document is the
CLI-facing text the site mirrors.

## Supported platforms

The supported install slice is **macOS arm64** (`aarch64-apple-darwin`) and
**Linux x86_64** (`x86_64-unknown-linux-gnu`). macOS Intel, Windows, and other
Linux variants are named residuals: the installer fails closed with a message
naming the residual rather than attempting an install
(`docs/factory/faber-onboarding/platform-matrix.md`).

## What an install touches

A faber install writes **only product-owned files**, all under the install
prefix (default `~/.faber`, user-local, no sudo):

```
<prefix>/bin/faber                                    launcher
<prefix>/share/faber/reference/                       reference pack
<prefix>/share/faber/locale/<locale>/pack.toml        locale packs
<prefix>/share/faber/install-receipt.json             install receipt
```

The core-support archive is embedded in the launcher and materialized on first
use to the platform cache (`~/Library/Caches/faber/core-support/…` on macOS,
`$XDG_CACHE_HOME/faber/…` or `~/.cache/faber/…` on Linux). The install receipt
records version, platform triple, payload files, source, and checksum — the
launcher metadata that `faber self update` / `faber self uninstall` and the
future doctor consume.

**PATH/shell changes are explicit and reversible.** The installer never edits
your shell config by itself; only the explicit `--add-to-path` flag appends
`export PATH="<prefix>/bin:$PATH"` to the detected shell rc, and `faber self
uninstall` removes exactly that line again.

**User projects and the package store are never touched by install, update,
downgrade, or reinstall.** The store (`<prefix>/cistae`, selectable via
`$CISTAE_HOME`) and any projects under `<prefix>/projects/` survive every
operation below; only uninstall with the explicit `--purge` flag removes them.

## Install

The verified bootstrap installer is `scripta/install-faber`. The `curl`
convenience path pipes the same self-contained script to `python3` (no repo
checkout); the script is designed to be published at the release tag by the
release process (R1 — pending; see `docs/install/install-page-reference.md`):

```sh
# from a repo checkout (developers):
python3 scripta/install-faber

# published convenience path (no checkout; uses the default release version):
curl -fsSL https://github.com/faberlang/releases/releases/download/\
    faber-v1.6.0/install-faber | python3 - --version 1.6.0
```

Key behavior:

- **Checksum before install.** The installer downloads the exact release
  payload for the platform triple plus the published basename-only `.sha256`
  asset, verifies SHA-256 **before any payload is unpacked or executed**, and
  aborts with **no partial install** on a mismatch. Nothing under the prefix is
  touched before verification passes.
- **User-local default.** The default prefix is `~/.faber` — no sudo, no hidden
  writes outside the prefix + platform cache + store.
- **Explicit prefix for system/agent installs:** `--prefix /opt/faber` (or any
  admin-writable root). Explicit triples: `--triple aarch64-apple-darwin` or
  `--triple x86_64-unknown-linux-gnu`.
- **Non-interactive:** the installer never prompts. Agent-facing behavior is
  documented in `docs/install/agent-installer.md`.
- **Change report.** On success the installer prints a report: result, prefix,
  source, `archiveSha256`, files added/updated/unchanged, receipt path, and
  PATH state.

After install, verify the kit:

```sh
faber --version
scripta/verify-dev-kit --payload ~/.faber --version 1.6.0   # full clean-room verify
```

### Flags (`scripta/install-faber`)

```
--version VERSION        release version to install (default: 1.6.0)
--prefix PREFIX          install prefix (default: ~/.faber, user-local)
--triple TRIPLE          platform triple (default: auto-detect)
--base-url URL|DIR       asset base URL or directory (mirrors, offline staging)
--add-to-path            append the PATH export to the detected shell rc
                         (explicit opt-in; reversible)
--update                 versioned install on the existing install (the
                         `faber self update` engine; upgrade OR pinned downgrade)
--allow-lane-change      explicitly allow crossing the odd dev / even LTS lane
--uninstall              remove ONLY product-owned files; reverse PATH changes
--purge                  with --uninstall, ALSO remove user projects and the
                         package store (explicit ask, never implicit)
```

## Reinstall (idempotency)

Running the installer again for the **same version** over the same prefix is a
no-op or a repair:

- Everything present and intact → `result: already current`, nothing changes
  (all files reported unchanged).
- Missing or tampered payload files → they are restored and reported
  (`result: repaired`); the same-version re-run is the install-side repair
  path for a broken install.
- A missing or corrupt install receipt → the re-run rewrites it
  (`missing-launcher-metadata` class, one next action).
- Installing a **different version** over the same prefix without `--update`
  fails closed and names the upgrade path (`faber self update`).

## Update

```sh
faber self update --version 1.6.1
```

`faber self update` upgrades the installed faber through the same verified
engine as the installer. The current install is preserved as a side-by-side
lane first, the target version is installed as its own lane
(`<prefix>/versions/<version>/`), and the active launcher, packs, and receipt
flip to the target with rollback. Failure fails closed: a checksum mismatch
aborts before any write, and a failed write rolls back to the previously active
version.

- **What changes:** the active launcher/reference/locale packs + receipt flip
  to the target version; the previous version stays available as a lane.
- **What survives:** user projects and the package store are byte-identical.
- **Version lanes:** odd majors are development lines, even majors are
  language-locked LTS lines (`docs/release/policy.md`). A cross-lane update
  fails closed unless you pass `--allow-lane-change` explicitly.
- **Flags:** `--version <VERSION>` (required), `--prefix <PATH>` (default:
  discovered from this binary's install receipt), `--base-url <URL>`,
  `--allow-lane-change`.

## Downgrade

```sh
faber self update --version 1.6.0    # from 1.6.1: pinned downgrade
```

A pinned downgrade to an older released version runs through the same side-
by-side lane machinery and is **reversible** — the active version flips back,
the old lane stays. Downgrade **fails closed** when it would strand a
version-incompatible locked pack: if a user project under `<prefix>/projects/`
has a `faber.lock` whose locked package declares a `faber_min` compatibility
bound the target cannot satisfy, the installer rejects the downgrade with the
typed cause `incompatible-pack-downgrade`, the affected layer, and ONE next
action — never a partial or silent mix of versions. Cross-lane downgrades
require `--allow-lane-change`.

## Uninstall

```sh
faber self uninstall
```

`faber self uninstall` removes **only product-owned files**:

- the installed payload and install receipt at the prefix,
- every side-by-side version lane (`<prefix>/versions/`),
- install rollback leftovers,
- the kit-owned platform-cache entries the launcher materialized,

and **reverses the explicit PATH/shell change** the installer made (the exact
`export PATH="<prefix>/bin:$PATH"` line it appended, with its marker comment).

**User projects, locks, and dependency caches survive by default.** Removing
them requires the explicit `--purge` flag, never implicit:

```sh
faber self uninstall --purge    # ALSO removes user projects + package store
```

The uninstall report lists what was removed and what was left (including
`user projects — pass --purge to remove` and `package store (dependency cache)
— pass --purge to remove` entries). A receipt-less prefix at an explicit
`--prefix` still gets the canonical kit layout removed (the broken-install
path); `faber self uninstall` itself needs the receipt to locate the published
engine and names the direct `scripta/install-faber --uninstall --prefix <p>`
invocation as the next action when the receipt is lost.

Flags: `--prefix <PATH>`, `--base-url <URL>`, `--purge`.

## Exit codes

`scripta/install-faber` (and therefore `faber self update` / `faber self
uninstall`, which run the same engine) exits with stable codes:

| Code | Meaning |
| --- | --- |
| `0` | installed, updated, downgraded, already current (idempotent re-run), uninstalled, purged, or nothing to uninstall (idempotent) |
| `1` | checksum mismatch, version conflict (cross-version without `--update`, cross-lane without `--allow-lane-change`), incompatible locked pack set on a downgrade, install/update failure (fail-closed; no partial install), or uninstall failure |
| `2` | usage error (unsupported platform/triple, malformed arguments) |

## Proxy and offline

Downloads honor the standard `HTTP_PROXY` / `HTTPS_PROXY` / `http_proxy` /
`https_proxy` environment variables. Offline or missing assets fail closed
with exit 1 and a message naming the URL. `--base-url` may point at a mirror
or a local directory; the checksum is still verified against the `.sha256`
fetched from that same source.

## Reference

- CLI help (zombie-docs cross-check): `faber self --help`, `faber self update
  --help`, `faber self uninstall --help`, `python3 scripta/install-faber
  --help`, `python3 scripta/verify-dev-kit --help`.
- Install-page artifact/version reference for `faberlang.dev`:
  `docs/install/install-page-reference.md`.
- Agent (non-interactive) use: `docs/install/agent-installer.md`.
