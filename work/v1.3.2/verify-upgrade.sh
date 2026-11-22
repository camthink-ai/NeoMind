#!/usr/bin/env bash
# Compatibility shim — Gate G6 script moved to tracked scripts/verify-upgrade.sh
# (work/ is gitignored; keeping a force-tracked copy here caused false greens).
exec "$(cd "$(dirname "$0")/../.." && pwd)/scripts/verify-upgrade.sh" "$@"
