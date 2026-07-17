#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKSPACE_ROOT="$(cd "$ROOT/.." && pwd)"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/faber-store-only-resolve.XXXXXX")"
cleanup() {
  rm -rf "$TMP"
}
trap cleanup EXIT

STORE="$TMP/cistae"
CONSUMER="$TMP/consumer"
mkdir -p "$CONSUMER/src"

cat > "$CONSUMER/faber.toml" <<'TOML'
[package]
name = "store-only-consumer"
version = "0.1.0"
edition = "2026"

[dependencies]
norma = "0.1.0"
triga = "0.1.0"

[paths]
source = "src"
entry = "main.fab"
TOML

cat > "$CONSUMER/src/main.fab" <<'FAB'
importa ex "norma:chorda" privata chorda
importa ex "triga:triga" privata triga

incipit {
    fixum textus reversed ← chorda.retorta("abc")
    fixum triga.Vector3 origin ← triga.Vector3 {
        x = 0.0,
        y = 0.0,
        z = 0.0
    }
    nota reversed
    nota origin.x
}
FAB

cargo run --quiet -- install --path "$WORKSPACE_ROOT/norma" --store "$STORE" --project "$CONSUMER"
cargo run --quiet -- install --path "$WORKSPACE_ROOT/triga" --store "$STORE" --project "$CONSUMER"

env -u FABER_LIBRARY_HOME \
  -u FABER_ENABLE_WORKSPACE_LIBRARY_PROBE \
  CISTAE_HOME="$STORE" \
  cargo run --quiet -- check --package "$CONSUMER"

echo "store-only resolve proof passed: $CONSUMER"
