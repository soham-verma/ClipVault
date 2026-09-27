#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
npm ci
case "$(uname -s)" in
  Darwin) npm run tauri -- build --bundles app,dmg "$@" ;;
  Linux) npm run tauri -- build --bundles deb,rpm,appimage "$@" ;;
  *) echo "ClipVault supports macOS and Linux." >&2; exit 1 ;;
esac
