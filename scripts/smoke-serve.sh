#!/bin/bash
# Smoke-test serve on :9375 with a PERSISTENT data dir (the old
# /tmp/neomind-smoke was wiped by macOS cleanup once, costing an hour
# of env rebuild). Survives reboots; kill + rerun to bounce.
set -e
cd "$(dirname "$0")/.."
export NEOMIND_DATA_DIR="$PWD/.smoke/data"
exec ./target/release/neomind serve --port 9375
