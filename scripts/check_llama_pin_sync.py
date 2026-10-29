#!/usr/bin/env python3
"""llama.cpp version-pin sync checker.

The llama.cpp runtime tag is pinned in four places that must move together
(see the bump-policy comment atop crates/neomind-core/src/builtin_llm/
runtime.rs):

  1. LLAMA_CPP_VERSION      — crates/neomind-core/src/builtin_llm/runtime.rs
                              (the pin the running server downloads/caches)
  2. LLAMA_CPP_TAG default  — scripts/build-llama-server.sh
                              (what the bundled desktop/Tauri binary builds)
  3. LLAMA_VERSION default  — scripts/install.sh
                              (what a fresh server install downloads)
  4. self-hosted asset names — llama-server-b<tag>-… constants/constructors in
                              the same core file (embedded tag must match)

A drift here ships mixed runtimes: e.g. install.sh pulls an older llama.cpp
than the code's cache key expects. CI fails on any mismatch.

Usage:
    python3 scripts/check_llama_pin_sync.py            # exit 1 on drift
    python3 scripts/check_llama_pin_sync.py --json     # machine-readable
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

CORE_RUNTIME = REPO_ROOT / "crates" / "neomind-core" / "src" / "builtin_llm" / "runtime.rs"
BUILD_SCRIPT = REPO_ROOT / "scripts" / "build-llama-server.sh"
INSTALL_SCRIPT = REPO_ROOT / "scripts" / "install.sh"

TAG_RE = re.compile(r"^b\d{4,7}$")


def core_pin() -> str:
    src = CORE_RUNTIME.read_text(encoding="utf-8")
    m = re.search(r'LLAMA_CPP_VERSION:\s*&str\s*=\s*"([^"]+)"', src)
    if not m:
        sys.exit("FATAL: LLAMA_CPP_VERSION not found in core runtime.rs")
    return m.group(1)


def script_default(path: Path, var: str) -> str:
    src = path.read_text(encoding="utf-8")
    # LLAMA_CPP_TAG="${LLAMA_CPP_TAG:-b11609}"
    m = re.search(re.escape(var) + r'="\$\{' + re.escape(var) + r':-([^"}]+)\}"', src)
    if not m:
        sys.exit(f"FATAL: {var} default not found in {path.name}")
    return m.group(1)


def asset_name_tags() -> list[str]:
    """Tags embedded in literal `llama-server-b<tag>-…` asset strings.

    After the tag-parameterization refactor the names are built from the pin,
    so no literal tags remain — then this simply returns [] and correctness
    is guaranteed by construction (plus the core unit tests).
    """
    src = CORE_RUNTIME.read_text(encoding="utf-8")
    return re.findall(r"llama-server-(b\d{4,7})-", src)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="JSON output")
    args = parser.parse_args()

    pin = core_pin()
    build_tag = script_default(BUILD_SCRIPT, "LLAMA_CPP_TAG")
    install_tag = script_default(INSTALL_SCRIPT, "LLAMA_VERSION")
    asset_tags = asset_name_tags()

    sites = {
        "core LLAMA_CPP_VERSION": pin,
        "build-llama-server.sh LLAMA_CPP_TAG": build_tag,
        "install.sh LLAMA_VERSION": install_tag,
        **{f"asset name #{i + 1}": t for i, t in enumerate(asset_tags)},
    }

    drift = {name: tag for name, tag in sites.items() if tag != pin}
    ok = not drift

    if args.json:
        print(json.dumps({"pin": pin, "sites": sites, "ok": ok}, indent=2))
    elif ok:
        print(f"OK: all llama.cpp pins in sync at {pin} ({len(sites)} site(s))")
    else:
        print(f"LLAMA PIN DRIFT — core pin is {pin}:")
        for name, tag in drift.items():
            print(f"  {name}: {tag}")
        print(
            "Bump all sites together (see the bump-policy comment atop\n"
            "crates/neomind-core/src/builtin_llm/runtime.rs): core pin, both\n"
            "scripts' defaults, and the self-hosted asset names/SHAs."
        )

    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
