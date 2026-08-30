#!/usr/bin/env bash
# Single source of truth for pulling packaging-relevant fields out of
# cinectl/Cargo.toml, so debian/ and packaging/cinectl.spec never hardcode
# a version/description that can drift from the crate. Run once, by the
# cinectl-linux build job (the only packaging-adjacent job with cargo
# already installed) — its output is threaded to the deb/rpm packaging
# jobs as job outputs, so neither needs cargo/jq installed just to
# re-derive the same values. Meant to be sourced (not executed) from the
# repo root: `source cinectl/packaging/cargo-metadata.sh`.
set -euo pipefail

meta=$(cargo metadata --no-deps --format-version=1)
pkg='.packages[] | select(.name == "cinectl")'
VERSION=$(jq -r "$pkg | .version" <<<"$meta")
DESCRIPTION=$(jq -r "$pkg | .description" <<<"$meta")
LICENSE=$(jq -r "$pkg | .license" <<<"$meta")
