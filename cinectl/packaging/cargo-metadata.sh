#!/usr/bin/env bash
# Single source of truth for pulling packaging-relevant fields out of
# cinectl/Cargo.toml, so debian/ and packaging/cinectl.spec never hardcode
# a version/description that can drift from the crate. Meant to be sourced
# (not executed) from the repo root: `source packaging/cargo-metadata.sh`.
set -euo pipefail

meta=$(cargo metadata --no-deps --format-version=1)
pkg='.packages[] | select(.name == "cinectl")'
VERSION=$(jq -r "$pkg | .version" <<<"$meta")
DESCRIPTION=$(jq -r "$pkg | .description" <<<"$meta")
LICENSE=$(jq -r "$pkg | .license" <<<"$meta")
