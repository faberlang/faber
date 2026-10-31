#!/bin/sh
# Faber installer.
#
# Installs one released binary as <prefix>/bin/faber-<version> and points
# <prefix>/bin/faber at it. Versions sit side by side; `faber update` moves the
# symlink, and `faber update --version <VERSION>` switches back.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/faberlang/faber/main/install.sh | sh
#   curl -fsSL .../install.sh | sh -s -- --version 1.18.1 --prefix "$HOME/.faber"
#
# The default version is the release this script was exported from. Pass
# --version to install another published release.

set -eu

PREFIX="${FABER_PREFIX:-${HOME:?HOME is not set}/.faber}"
VERSION="${FABER_VERSION:-1.18.1}"
BASE_URL="${FABER_BASE_URL:-https://github.com/faberlang/releases/releases/download}"

usage() {
    cat <<'EOF'
usage: install.sh [--version VERSION] [--prefix DIR] [--base-url URL]

  --version VERSION  release to install (default: the release this script came from)
  --prefix DIR       install prefix (default: ~/.faber)
  --base-url URL     release asset base (default: the public release host)

Installs <prefix>/bin/faber-VERSION and points <prefix>/bin/faber at it.
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version) VERSION="${2:?--version needs a value}"; shift 2 ;;
        --version=*) VERSION="${1#--version=}"; shift ;;
        --prefix) PREFIX="${2:?--prefix needs a value}"; shift 2 ;;
        --prefix=*) PREFIX="${1#--prefix=}"; shift ;;
        --base-url) BASE_URL="${2:?--base-url needs a value}"; shift 2 ;;
        --base-url=*) BASE_URL="${1#--base-url=}"; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "error: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

VERSION="${VERSION#v}"
BASE_URL="${BASE_URL%/}"
case "$VERSION" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "error: --version must look like 1.18.1 (got ${VERSION})" >&2; exit 2 ;;
esac

case "$(uname -s):$(uname -m)" in
    Darwin:arm64) TRIPLE="aarch64-apple-darwin" ;;
    Linux:x86_64) TRIPLE="x86_64-unknown-linux-gnu" ;;
    *)
        echo "error: no published faber binary for $(uname -s) $(uname -m); releases support aarch64-apple-darwin and x86_64-unknown-linux-gnu" >&2
        exit 1
        ;;
esac

command -v curl >/dev/null 2>&1 || { echo "error: curl is required" >&2; exit 1; }
command -v tar >/dev/null 2>&1 || { echo "error: tar is required" >&2; exit 1; }

archive="faber-v${VERSION}-${TRIPLE}.tar.gz"
url="${BASE_URL}/faber-v${VERSION}/${archive}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

echo "faber install — ${VERSION} (${TRIPLE})"
echo "  prefix:   ${PREFIX}/bin"
echo "  download: ${url}"

curl -fsSL "$url" -o "${tmp}/${archive}"
curl -fsSL "${url}.sha256" -o "${tmp}/${archive}.sha256"

if command -v shasum >/dev/null 2>&1; then
    (cd "$tmp" && shasum -a 256 -c "${archive}.sha256")
elif command -v sha256sum >/dev/null 2>&1; then
    (cd "$tmp" && sha256sum -c "${archive}.sha256")
else
    echo "error: shasum or sha256sum is required to verify the download" >&2
    exit 1
fi

mkdir -p "${PREFIX}/bin"
tar -xzf "${tmp}/${archive}" -C "$tmp" bin/faber
mv "${tmp}/bin/faber" "${PREFIX}/bin/faber-${VERSION}"
chmod 755 "${PREFIX}/bin/faber-${VERSION}"

rm -f "${PREFIX}/bin/.faber.link.tmp"
ln -s "faber-${VERSION}" "${PREFIX}/bin/.faber.link.tmp"
mv -f "${PREFIX}/bin/.faber.link.tmp" "${PREFIX}/bin/faber"

echo "installed ${PREFIX}/bin/faber-${VERSION}"
echo "active:    ${PREFIX}/bin/faber -> faber-${VERSION}"
case ":${PATH}:" in
    *":${PREFIX}/bin:"*) ;;
    *) echo "add to PATH: export PATH=\"${PREFIX}/bin:\$PATH\"" ;;
esac
"${PREFIX}/bin/faber" --version
