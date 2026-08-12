# Faber installer for agents: non-interactive use

Non-interactive installer documentation for agent installers and CI. The faber
install channel is fully non-interactive by design: **no prompts, ever** —
with stdin closed, with a minimal PATH, in a scratch HOME. This is the basis
for the `agent-noninteractive` clean-room profile
(`docs/factory/faber-onboarding/platform-matrix.md`).

Tested release: **faber 1.6.0** (zombie-docs verified 2026-08-12). The
machine-readable JSON diagnostics and skills surfaces are **faber Stage 9 —
staged, not implemented**. They are named in this document only as staged; an
agent that needs JSON output today must parse the human-readable change report
below (its lines are stable and prefixed).

## The agent contract

1. **Explicit prefix.** Automation always passes `--prefix` (and `--triple` for
   determinism). Never rely on the ambient default `~/.faber` in a script; an
   agent run that depends on a specific location names it.
2. **No prompts.** The installer reads nothing from stdin. Closing stdin
   (`</dev/null`) is safe. There is no interactive confirmation step, no
   license prompt, no `y/N` question.
3. **Stable exit codes.** The exit code alone tells the caller the outcome
   (see the table in `docs/install/lifecycle.md`):
   - `0` — success: installed, updated, downgraded, already current,
     uninstalled, purged, or nothing to uninstall.
   - `1` — fail-closed operation: checksum mismatch, version conflict
     (cross-version without `--update`, cross-lane without
     `--allow-lane-change`), incompatible locked pack set on a downgrade,
     install/update failure (no partial install), or uninstall failure.
   - `2` — usage error: unsupported platform/triple, malformed arguments.
4. **Machine-readable report of what changed.** The report on stdout is
   line-prefixed and stable:

   ```text
   faber install report — faber 1.6.0 (aarch64-apple-darwin)
     result:     installed
     prefix:     /opt/faber
     source:     <base>/faber-v1.6.0-aarch64-apple-darwin.tar.gz
     checksum:   sha256 verified before install (basename-only asset)
     archiveSha256: <sha256>
     files:      N added, N updated, N unchanged, 0 removed
       added     bin/faber
       added     share/faber/reference/…
       …
     receipt:    <prefix>/share/faber/install-receipt.json
     PATH:       <prefix>/bin is not on PATH
                 add: export PATH="<prefix>/bin:$PATH"
     proxy:      downloads honor HTTPS_PROXY / HTTP_PROXY env vars
     verify:     scripta/verify-dev-kit --payload <prefix> --version 1.6.0
   ```

   `result` is one of `installed`, `already current`, `repaired`,
   `updated (from <v>)`, `downgraded (from <v>)`, `uninstalled`, `purged`,
   `nothing to uninstall`. The JSON diagnostics surface is **faber Stage 9 —
   staged** (see below).

## Minimum agent run

```sh
python3 scripta/install-faber \
  --version 1.6.0 \
  --prefix /opt/faber \
  --triple aarch64-apple-darwin \
  --base-url https://github.com/faberlang/releases/releases/download/faber-v1.6.0
```

The `--base-url` may be a local directory for offline/mirror staging; the
checksum is still verified against the `.sha256` fetched from that same
source. Offline or missing assets fail closed with exit 1 and a message naming
the URL.

An agent should not set `--add-to-path` (a shell rc edit is a human-facing
change); to invoke faber from automation, use the full path
`<prefix>/bin/faber` or export PATH itself.

## Agent-style walkthrough (clean-room, verified)

One clean-room walkthrough was executed at the tested release against a
synthetic layout-faithful payload wrapped by the real `scripta/package-archive`
(scratch HOME, `PATH=/usr/bin:/bin`, stdin closed). Result: the documented
exit codes reproduced exactly.

```sh
# W1: explicit-prefix install, stdin closed, minimal PATH
python3 scripta/install-faber --version 1.6.0 --triple aarch64-apple-darwin \
  --prefix <prefix> --base-url <host> </dev/null
#   result: installed; files: 5 added …; exit=0

# W2: idempotent re-run
#   result: already current; files: 0 added, 0 updated, 5 unchanged; exit=0

# W3: tampered checksum (one byte flipped in the archive)
#   error: SHA-256 mismatch for …; computed ≠ published
#   checksum verified before install; aborting with NO partial install
#   exit=1; nothing under the target prefix exists (no partial install)

# W4: uninstall
python3 scripta/install-faber --version 1.6.0 --triple aarch64-apple-darwin \
  --prefix <prefix> --base-url <host> --uninstall </dev/null
#   result: uninstalled; files: 5 removed; receipt removed; exit=0

# W5: usage error (unsupported triple)
#   error: triple 'x86_64-apple-darwin' is not supported: macOS Intel (named
#   residual, platform-matrix.md); exit=2
```

Every run above completed with stdin closed and no prompt of any kind. The
payload content in this walkthrough was synthetic and honestly labeled; it
proves the installer's non-interactive contract (prefix, checksum-before-
install, no prompts, exit codes, change report), not payload functionality —
payload functionality is proven by `scripta/verify-dev-kit` against the
canonical Stage 2 payload and by the recurring clean-room CI (faber Stage 10).

## Update and uninstall from automation

Same engine, same exit-code contract:

```sh
# upgrade or pinned downgrade (side-by-side lanes; user projects + store untouched)
<prefix>/bin/faber self update --version 1.6.1 --prefix /opt/faber
#   exit=0 on success; exit=1 fail-closed (typed cause + next action on stdout)

# uninstall (product-owned only; user projects + store survive)
<prefix>/bin/faber self uninstall --prefix /opt/faber
#   exit=0; report lists what was removed and what was left

# explicit data removal, never implicit
<prefix>/bin/faber self uninstall --prefix /opt/faber --purge
```

In automation, prefer `--prefix` on `faber self update` / `faber self
uninstall` too — without it the wrapper discovers the prefix from the running
binary's install receipt (correct for interactive use, but explicit is the
agent contract).

## Verification step for agents

```sh
scripta/verify-dev-kit --payload /opt/faber --version 1.6.0
```

The Stage 2 clean-room verifier asserts the installed kit at the prefix:
`--version` match, reference lookup, locale pack, manifest digest match, and
fail-closed behavior on tampered/skewed content. Its exit code is the agent's
install-success proof.

## Staged surface (faber Stage 9) — labeled, not implemented

The full agent-surface consolidation stays **faber Stage 9** (delivery-stage3
D3; CAMPAIGN Stage 9): structured JSON diagnostics for install/update/
uninstall, and agent skill packaging. At the tested release:

- The change report is **human-readable** (line-prefixed, stable — parseable,
  but not JSON).
- There is **no `--json` flag** on `scripta/install-faber` and no JSON mode on
  `faber self update` / `faber self uninstall`.
- There is **no agent skill** shipped with the kit.

Agents must not depend on a JSON surface at this release. When Stage 9 lands,
this document is updated to name the exact JSON schema and any skill bundle;
until then the surface is staged and this paragraph is the honest label.
