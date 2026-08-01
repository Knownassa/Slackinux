#!/usr/bin/env bash
# Bumps the Slackinux version in all three source-of-truth files, then keeps
# the lockfiles and the compiler in sync.
#
# Usage: scripts/set-version.sh 0.3.0
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
new="${1:-}"

if [[ -z "$new" || ! "$new" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: $0 X.Y.Z" >&2
  exit 1
fi

node_manifest="$root/apps/desktop/package.json"
cargo_manifest="$root/apps/desktop/src-tauri/Cargo.toml"
tauri_config="$root/apps/desktop/src-tauri/tauri.conf.json"

for f in "$node_manifest" "$cargo_manifest" "$tauri_config"; do
  if [[ ! -f "$f" ]]; then
    echo "error: missing $f" >&2
    exit 1
  fi
done

# package.json / tauri.conf.json use "version": "x.y.z"; Cargo.toml uses version = "x.y.z".
sed -i -E 's/("version"[[:space:]]*:[[:space:]]*)"[0-9]+\.[0-9]+\.[0-9]+"/\1"'"$new"'"/' "$node_manifest"
sed -i -E 's/("version"[[:space:]]*:[[:space:]]*)"[0-9]+\.[0-9]+\.[0-9]+"/\1"'"$new"'"/' "$tauri_config"
sed -i -E 's/(^version[[:space:]]*=[[:space:]]*)"[0-9]+\.[0-9]+\.[0-9]+"/\1"'"$new"'"/' "$cargo_manifest"

"$root/scripts/check-version-consistency.sh"

cargo check --workspace --manifest-path "$cargo_manifest"
(cd "$root/apps/desktop" && npm install --package-lock-only >/dev/null)

echo "Version bumped to $new across package.json, Cargo.toml and tauri.conf.json."
