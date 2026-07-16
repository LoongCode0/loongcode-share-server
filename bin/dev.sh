#!/usr/bin/env bash
# 开发：终端 1 跑本脚本（API），终端 2 跑 `pnpm --dir web dev`（前端热更，/api 已代理 8787）。
set -euo pipefail
cd "$(dirname "$0")/.."
export SHARE_HMAC_SECRET="${SHARE_HMAC_SECRET:-dev-secret-0123456789}"
export SHARE_BASE_URL="${SHARE_BASE_URL:-http://127.0.0.1:8787}"
export SHARE_LISTEN="${SHARE_LISTEN:-127.0.0.1:8787}"
cargo run
