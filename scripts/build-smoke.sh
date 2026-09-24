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
    # `--features static` embeds crates/neomind-api/static/ into the binary at
    # compile time. Nothing else populates it, so refresh it from the real
    # frontend build first — otherwise the binary serves whatever an earlier
    # session left behind. neomind-api/build.rs fails the build if this is
    # skipped and the embed turns out to be older than web/dist.
    if [ -f web/dist/index.html ] && [ web/dist/index.html -nt crates/neomind-api/static/index.html ]; then
        echo "==> Syncing web/dist → crates/neomind-api/static/ ..."
        mkdir -p crates/neomind-api/static
        rsync -a --delete web/dist/ crates/neomind-api/static/
    fi
    echo "==> Building neomind + runner (one or both missing)..."
    cargo build --release --bin neomind --bin neomind-extension-runner --features static
fi
exec ./scripts/smoke-serve.sh
