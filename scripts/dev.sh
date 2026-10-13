#!/usr/bin/env bash
# Runs the UI dev server and the Tauri app side by side.
# The Tauri app automatically spawns the core server on startup.
set -euo pipefail
trap 'kill 0' EXIT
( cd ui && npm run dev ) &
( cd crates/tauri && cargo tauri dev ) &
wait
