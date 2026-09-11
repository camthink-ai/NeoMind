#!/bin/bash
# Build BOTH binaries needed for the smoke environment. The target directory
# gets periodically cleaned (by cargo clean, disk cleanup, or Docker) — this
# script ensures both neomind and neomind-extension-runner exist before serve.
set -e
cd "$(dirname "$0")/.."
NEED_BUILD=0
[ ! -f target/release/neomind ] && NEED_BUILD=1
[ ! -f target/release/neomind-extension-runner ] && NEED_BUILD=1
if [ $NEED_BUILD -eq 1 ]; then
    echo "==> Building neomind + runner (one or both missing)..."
    cargo build --release --bin neomind --bin neomind-extension-runner --features static
fi
exec ./scripts/smoke-serve.sh
