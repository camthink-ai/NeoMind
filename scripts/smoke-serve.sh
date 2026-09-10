#!/bin/bash
# Smoke-test serve on :9375 with a PERSISTENT data dir. The env's DB
# records ABSOLUTE extension paths under /tmp/neomind-smoke — keep that
# path alive as a symlink to the persistent copy so macOS /tmp cleanup
# can only break a symlink (recreated here), never the data.
set -e
cd "$(dirname "$0")/.."
export NEOMIND_DATA_DIR="$PWD/.smoke/data"
if [ ! -L /tmp/neomind-smoke ]; then
  rm -rf /tmp/neomind-smoke
  ln -s "$PWD/.smoke" /tmp/neomind-smoke
fi
exec ./target/release/neomind serve --port 9375
