#!/usr/bin/env bash
# Install the Faber lane-grid nightly unit on pharos (EL-5 host config).
# Run from pharos as ianzepp; needs passwordless sudo for linger only.
#
# Usage: ./install-grid-unit.sh   (from a checkout that contains this file,
#                                  or after scp'ing the units to ~/.config/systemd/user)

set -euo pipefail

UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
mkdir -p "$UNIT_DIR"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cp "$SCRIPT_DIR/faber-lane-grid.service" "$UNIT_DIR/"
cp "$SCRIPT_DIR/faber-lane-grid.timer" "$UNIT_DIR/"

systemctl --user daemon-reload
systemctl --user enable --now faber-lane-grid.timer

# User timers stop when the user logs out unless lingering is enabled.
sudo loginctl enable-linger "$USER"

echo "installed:"
systemctl --user list-timers faber-lane-grid.timer --no-pager
systemctl --user status faber-lane-grid.service --no-pager || true
