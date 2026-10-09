#!/usr/bin/env bash
# Refresh the compiled-in model capability registry (multimodal / context
# window / reasoning detection) from LiteLLM's community-maintained
# model_prices_and_context_window.json.
#
# What it does:
#   1. Downloads the latest registry from LiteLLM's main branch.
#   2. Sanity-checks it (valid JSON, plausible entry count, sample keys).
#   3. Replaces crates/neomind-core/src/assets/model_registry.json.
#   4. Prints the before/after stats to paste into the registry.rs doc
#      comment (entry / vision / reasoning counts + today's date).
#   5. Runs the registry unit tests — upstream PRUNES keys (the 2026-10
#      snapshot dropped the dated bare Claude-3 keys), and aliases_for()
#      targets can disappear. If tests fail here, update the alias table
#      in crates/neomind-core/src/llm/registry.rs before committing.
#
# The registry is embedded via include_str! — a rebuild is required for
# the new data to reach the binary.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ASSET="$REPO_ROOT/crates/neomind-core/src/assets/model_registry.json"
URL="${LITELLM_REGISTRY_URL:-https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json}"
# Temp file in the ASSET's own directory: the final replace is a same-fs
# `mv` (atomic rename). A /tmp temp forced `cp`, which an interrupted run
# could leave as a truncated asset — include_str! would then embed garbage.
TMP="$(mktemp "${ASSET}.XXXXXX")"
trap 'rm -f "$TMP"' EXIT

echo "→ downloading $URL"
curl -sSL --max-time 60 "$URL" -o "$TMP"

echo "→ sanity check"
python3 - "$TMP" "$ASSET" <<'PY'
import json, sys

new = json.load(open(sys.argv[1]))
old = json.load(open(sys.argv[2]))

def stats(d):
    e = {k: v for k, v in d.items() if isinstance(v, dict) and k != "sample_spec"}
    return (
        len(e),
        sum(1 for v in e.values() if v.get("supports_vision")),
        sum(1 for v in e.values() if v.get("supports_reasoning")),
    )

n, o = stats(new), stats(old)
# sample_spec in old builds had 2988 total keys; guard against a truncated
# or schema-broken download: at least 80% of the current entry count, and
# the two models every downstream test relies on must still resolve.
assert n[0] > o[0] * 0.8, f"suspicious shrink: {o[0]} -> {n[0]} entries"
for probe in ("gpt-4o", "vertex_ai/claude-3-5-sonnet"):
    assert probe in new, f"probe key missing: {probe}"
print(f"old: {o[0]} entries, {o[1]} vision, {o[2]} reasoning")
print(f"new: {n[0]} entries, {n[1]} vision, {n[2]} reasoning")
print(f"added {len(set(new)-set(old))}, removed {len(set(old)-set(new))}")
PY

chmod 644 "$TMP"   # mktemp creates 0600; the asset is a normal repo file
mv "$TMP" "$ASSET" # same-directory rename — atomic replace
echo "→ replaced $ASSET"
echo

# The registry tests are the step that catches what a refresh breaks (a
# stale-text test pinned to a model whose flags changed, pruned alias
# targets). Running them HERE instead of leaving them as a "next step"
# printed at exit is what caught the deepseek-v4-flash pin during the
# 2026-10-08 refresh — make it automatic.
echo "→ running registry tests (cargo test -p neomind-core --lib llm::registry)"
if cargo test -p neomind-core --lib llm::registry; then
    echo "✓ registry tests pass"
else
    echo "✗ registry tests FAILED — likely a stale test pin or pruned alias target;" >&2
    echo "  fix crates/neomind-core/src/llm/registry.rs + tests before committing." >&2
    exit 1
fi
echo
echo "Next steps:"
echo "  1. Update the counts + date in the doc comment of"
echo "     crates/neomind-core/src/llm/registry.rs"
echo "  2. Rebuild: the registry is include_str!-embedded."
