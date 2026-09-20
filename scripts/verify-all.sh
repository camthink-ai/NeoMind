#!/usr/bin/env bash
# One-shot full verification — the exact gates CI runs, locally.
#
# Run this before committing anything nontrivial. It exists because a final
# review once claimed "all green" after re-running only some gates; this
# script makes "all green" a single reproducible command.
#
# Usage:
#   scripts/verify-all.sh              # everything
#   scripts/verify-all.sh --skip-rust  # frontend gates only
#   scripts/verify-all.sh --skip-web   # rust gates only
#   scripts/verify-all.sh --fast       # skip the long test suites
#                                     # (cargo test + vitest), keep the rest
set -uo pipefail
cd "$(dirname "$0")/.."

SKIP_RUST=0; SKIP_WEB=0; FAST=0
for arg in "$@"; do
  case "$arg" in
    --skip-rust) SKIP_RUST=1 ;;
    --skip-web)  SKIP_WEB=1 ;;
    --fast)      FAST=1 ;;
    *) echo "unknown flag: $arg (use --skip-rust | --skip-web | --fast)"; exit 2 ;;
  esac
done

GATE() { printf '\n\033[1m== %s ==\033[0m\n' "$1"; }
FAIL() { printf '\n\033[31m** FAILED: %s **\033[0m\n\n' "$1"; exit 1; }
START=$SECONDS

if [ "$SKIP_RUST" -eq 0 ]; then
  GATE "rust: cargo fmt --check"
  cargo fmt --all -- --check || FAIL "cargo fmt"

  GATE "rust: clippy (workspace, all targets, -D warnings)"
  cargo clippy --workspace --all-targets --locked -- -D warnings || FAIL "clippy"

  if [ "$FAST" -eq 0 ]; then
    GATE "rust: cargo test (workspace)"
    cargo test --workspace --locked --features neomind-agent/test-utils || FAIL "cargo test"
  else
    echo "(skipped by --fast)"
  fi
fi

if [ "$SKIP_WEB" -eq 0 ]; then
  cd web || FAIL "cd web"

  GATE "web: eslint errors"
  npm run lint:ci || FAIL "eslint errors"

  GATE "web: eslint warning ratchet (baseline .eslint-baseline)"
  BASELINE=$(cat .eslint-baseline)
  COUNT=$(npx eslint . --ext ts,tsx --report-unused-disable-directives --format json \
    | node -e "let d='';process.stdin.on('data',c=>d+=c);process.stdin.on('end',()=>{const r=JSON.parse(d);console.log(r.reduce((s,f)=>s+f.warningCount,0))})")
  echo "ESLint warnings: $COUNT (baseline: $BASELINE)"
  [ "$COUNT" -le "$BASELINE" ] || FAIL "eslint ratchet ($COUNT > $BASELINE)"

  GATE "web: tsc --noEmit"
  npx tsc --noEmit || FAIL "tsc"

  if [ "$FAST" -eq 0 ]; then
    GATE "web: vitest run"
    npx vitest run || FAIL "vitest"
  else
    echo "(skipped by --fast)"
  fi
  cd ..
fi

printf '\n\033[32mALL GATES GREEN in %ss\033[0m\n' "$((SECONDS - START))"
