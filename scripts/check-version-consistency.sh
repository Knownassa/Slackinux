#!/usr/bin/env bash
# Fails when package.json, Cargo.toml and tauri.conf.json disagree on the
# application version. Used by CI and by scripts/set-version.sh.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

node_ver="$(grep -m1 '"version"' "$root/apps/desktop/package.json" | sed -E 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/')"
cargo_ver="$(grep -m1 '^version[[:space:]]*=' "$root/apps/desktop/src-tauri/Cargo.toml" | sed -E 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/')"
tauri_ver="$(grep -m1 '"version"' "$root/apps/desktop/src-tauri/tauri.conf.json" | sed -E 's/.*"version"[[:space:]]*:[[:space:]]*"([^"]+)".*/\1/')"

if [[ -z "$node_ver" || -z "$cargo_ver" || -z "$tauri_ver" ]]; then
  echo "error: could not extract a version from one of the files" >&2
  exit 1
fi

if [[ "$node_ver" != "$cargo_ver" || "$node_ver" != "$tauri_ver" ]]; then
  echo "Version mismatch:" >&2
  echo "  apps/desktop/package.json            = $node_ver" >&2
  echo "  apps/desktop/src-tauri/Cargo.toml    = $cargo_ver" >&2
  echo "  apps/desktop/src-tauri/tauri.conf.json = $tauri_ver" >&2
  exit 1
fi

echo "Versions are consistent: $node_ver"
