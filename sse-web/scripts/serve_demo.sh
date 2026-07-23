#!/usr/bin/env bash
# Builds the wasm module and serves the demo locally.
# Usage: sse-web/scripts/serve_demo.sh [port]
set -euo pipefail

port="${1:-8321}"
repo="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"

echo "building wasm module ..."
cargo build -p sse-web --target wasm32-unknown-unknown --release \
    --manifest-path "$repo/Cargo.toml"
wasm-bindgen --target web --no-typescript \
    --out-dir "$repo/sse-web/web/pkg" \
    "$repo/target/wasm32-unknown-unknown/release/sse_web.wasm"

echo
echo "demo: http://localhost:$port/"
echo "stop with Ctrl-C"
exec python3 -m http.server "$port" --directory "$repo/sse-web/web"
