#!/usr/bin/env bash
# Regenerates debian/control (from control.in) and debian/changelog using
# VERSION/DESCRIPTION, which the caller must already have exported (CI sets
# them from the cinectl-linux job's cargo-metadata.sh output, so this
# packaging job never needs cargo/jq installed just to re-derive them).
# Both generated files are gitignored; run this right before
# dpkg-buildpackage. Self-locating (cds to its own directory first) so it
# works regardless of the caller's working directory.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

: "${VERSION:?VERSION must be set}"
: "${DESCRIPTION:?DESCRIPTION must be set}"

sed "s/@DESCRIPTION@/$DESCRIPTION/" debian/control.in > debian/control

cat > debian/changelog <<EOF
cinectl ($VERSION-1) unstable; urgency=medium

  * Packaging build for cinectl $VERSION.

 -- Edwin Amsler <EdwinGuy@GMail.com>  $(date -R)
EOF
