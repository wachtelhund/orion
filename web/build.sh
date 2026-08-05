#!/bin/sh
# Build the browser bundle into web/dist (needs wasm-bindgen-cli matching
# the wasm-bindgen version in Cargo.lock).
set -e
cd "$(dirname "$0")/.."
cargo build --release -p orion-client --target wasm32-unknown-unknown
wasm-bindgen --target web --out-dir web/dist --no-typescript \
    target/wasm32-unknown-unknown/release/orion-client.wasm
echo "serve with: cd web && python3 serve.py"
