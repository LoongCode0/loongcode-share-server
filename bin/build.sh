#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
pnpm --dir web install
pnpm --dir web build
cargo build --release
echo "OK: target/release/share-server + web/dist"
