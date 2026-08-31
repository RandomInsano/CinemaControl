#!/usr/bin/env bash
# Registers cinectl-menubar as a per-user LaunchAgent so it starts
# automatically at login. cinectl-menubar ships as a bare binary rather
# than an .app bundle, so SMAppService (the modern Login Items API, which
# expects an app bundle) isn't an option — a LaunchAgent plist plus
# launchctl is the standard mechanism for a plain executable.
#
# Usage: packaging/install.sh [path to cinectl-menubar binary]
# Defaults to whatever's on PATH. Self-locating (cds to its own directory
# first) so it works regardless of the caller's working directory.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

label=com.cinemacontrol.cinectl-menubar
plist_path="$HOME/Library/LaunchAgents/$label.plist"

exe_path="${1:-$(command -v cinectl-menubar || true)}"
if [[ -z "$exe_path" ]]; then
    echo "error: cinectl-menubar isn't on PATH; pass its location explicitly:" >&2
    echo "  $0 /path/to/cinectl-menubar" >&2
    exit 1
fi
# Resolve to an absolute path — launchd doesn't run this relative to
# anything in particular.
exe_dir=$(cd "$(dirname "$exe_path")" && pwd)
exe_path="$exe_dir/$(basename "$exe_path")"

mkdir -p "$HOME/Library/LaunchAgents"
sed "s|@CINECTL_MENUBAR_PATH@|$exe_path|" "$label.plist" > "$plist_path"

# Tolerate failure: a no-op if it wasn't already loaded, and lets a re-run
# of this script pick up a moved binary.
launchctl bootout "gui/$(id -u)/$label" >/dev/null 2>&1 || true
launchctl bootstrap "gui/$(id -u)" "$plist_path"

echo "Installed — cinectl-menubar will start automatically at login."
echo "Wrote $plist_path (pointing at $exe_path)"
