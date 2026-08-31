#!/usr/bin/env bash
# Unregisters the LaunchAgent installed by install.sh.
set -euo pipefail

label=com.cinemacontrol.cinectl-menubar
plist_path="$HOME/Library/LaunchAgents/$label.plist"

launchctl bootout "gui/$(id -u)/$label" >/dev/null 2>&1 || true
rm -f "$plist_path"

echo "Uninstalled — cinectl-menubar will no longer start at login."
