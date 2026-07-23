#!/usr/bin/env bash
# Builds the wasm module and serves the demo locally.
# Usage: sse-web/scripts/serve_demo.sh [port]
set -euo pipefail

port="${1:-8321}"
repo="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"

# Stage the design system where the page expects it (CI does the same
# via the assemble step). Fetched at the pinned tag if not present.
if [ ! -d "$repo/design" ]; then
    echo "fetching design system v0.1.0 ..."
    curl -sSfL https://raw.githubusercontent.com/lere01/design/v0.1.0/ci/fetch-design.sh \
        | sh -s -- v0.1.0 "$repo/design"
fi
rm -rf "$repo/sse-web/web/design"
cp -R "$repo/design" "$repo/sse-web/web/design"

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
