#!/usr/bin/env bash
# Assembles cinectl-menubar into a proper .app bundle
# (Contents/{MacOS,Info.plist}) and ad-hoc code-signs it.
#
# The code signature isn't optional cosmetics: SMAppService (see
# ../src/login_item.rs) refuses to register an app that isn't signed, even
# for purely local use. Ad-hoc signing (no certificate, no Apple Developer
# account) satisfies that, but doesn't make the bundle notarized or
# Gatekeeper-trusted for anyone else it's handed to.
#
# Usage: packaging/bundle.sh [path to cinectl-menubar binary]
# Defaults to building a fresh release binary. Self-locating (cds to its
# own directory first) so it works regardless of the caller's working
# directory.
set -euo pipefail

# Resolve a relative `$1` against the caller's cwd before the `cd` below
# changes it out from under us.
exe_path="${1:-}"
if [[ -n "$exe_path" ]]; then
    exe_path="$(cd "$(dirname "$exe_path")" && pwd)/$(basename "$exe_path")"
fi

cd "$(dirname "${BASH_SOURCE[0]}")"

bundle=CinectlMenubar.app

if [[ -z "$exe_path" ]]; then
    (cd ../.. && cargo build -p cinectl-menubar --release)
    exe_path=../../target/release/cinectl-menubar
fi

rm -rf "$bundle"
mkdir -p "$bundle/Contents/MacOS"
cp Info.plist "$bundle/Contents/Info.plist"
cp "$exe_path" "$bundle/Contents/MacOS/cinectl-menubar"
chmod +x "$bundle/Contents/MacOS/cinectl-menubar"

codesign --force --deep --sign - "$bundle"

echo "Built $(cd "$(dirname "$bundle")" && pwd)/$bundle"
