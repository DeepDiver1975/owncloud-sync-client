#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

echo "Installing oc-dbus-service..."
mkdir -p ~/.local/bin
cp "$REPO_ROOT/target/release/oc-dbus-service" ~/.local/bin/

echo "Installing Nautilus extension..."
mkdir -p ~/.local/share/nautilus/extensions
cp "$SCRIPT_DIR/nautilus/owncloud-nautilus.py" ~/.local/share/nautilus/extensions/

echo "Installing Dolphin service menu..."
mkdir -p ~/.local/share/kservices5/ServiceMenus
cp "$SCRIPT_DIR/dolphin/owncloud.desktop" ~/.local/share/kservices5/ServiceMenus/

echo "Installation complete."
echo ""
echo "Next steps:"
echo "  1. Start the daemon:   ./target/release/ocsyncd &"
echo "  2. Start the service:  ~/.local/bin/oc-dbus-service &"
echo "  3. Restart Nautilus:   nautilus -q && nautilus"
echo "  4. Verify D-Bus:       busctl --user list | grep owncloud"
