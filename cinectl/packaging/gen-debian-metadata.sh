#!/usr/bin/env bash
# Regenerates debian/control (from control.in) and debian/changelog from
# cinectl/Cargo.toml, so a dpkg-buildpackage run never ships a stale
# version or description. Both generated files are gitignored; run this
# right before dpkg-buildpackage. Self-locating (cds to its own directory
# first) so it works regardless of the caller's working directory.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"
source cargo-metadata.sh

sed "s/@DESCRIPTION@/$DESCRIPTION/" debian/control.in > debian/control

cat > debian/changelog <<EOF
cinectl ($VERSION-1) unstable; urgency=medium

  * Packaging build for cinectl $VERSION.

 -- Edwin Amsler <EdwinGuy@GMail.com>  $(date -R)
EOF
