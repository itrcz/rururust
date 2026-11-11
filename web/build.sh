#!/bin/sh
# Builds an example for the browser into web/pkg.
#   ./web/build.sh [example]     then:  python3 -m http.server -d web 8080
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version <same as in Cargo.lock>
set -e
cd "$(dirname "$0")/.."
EXAMPLE="${1:-showcase}"
cargo build --profile dist --example "$EXAMPLE" --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg \
  "target/wasm32-unknown-unknown/dist/examples/$EXAMPLE.wasm"
ls -lh web/pkg/*.wasm
