#!/usr/bin/env python3
"""Detect drift between case expectations and the real CLI surface.

The 2026-10 audit found expectations only ever pin `neomind <domain> <verb>`
— flags were invisible, so `--enabled true` (rejected by clap) still passed.
Two drift classes matter, checked in order:

  1. domain/verb drift — a case expects a subcommand that no longer exists.
     Checked against a COMMITTED manifest (eval/cli_manifest.json) so CI needs
     no Rust toolchain; regenerate the manifest deliberately when the CLI
     changes:  python3 eval/check_cli_drift.py --regen
  2. flag drift — an `expect.substrings` flag that the command doesn't take.
     Checked against per-command flags recorded in the same manifest.

Exit 1 on any drift. With NEOMIND_TEST_BIN unset and no binary present the
manifest is still checked (that is the point of committing it).
"""
from __future__ import annotations

import argparse
import glob
import json
import os
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
MANIFEST = HERE / "cli_manifest.json"
FLAG_RE = re.compile(r"--[a-z0-9][a-z0-9-]*")


def _bin() -> str | None:
    cand = os.environ.get("NEOMIND_TEST_BIN")
    if cand and Path(cand).exists():
        return cand
    repo = HERE.parent / "target" / "release" / "neomind"
    return str(repo) if repo.exists() else None


CORE_WIDGETS = HERE.parent / "crates" / "neomind-core" / "src" / "dashboard.rs"
RENDERERS = HERE.parent / "web" / "src" / "pages" / "dashboard-components" / "Renderers.tsx"


def extract_core_widget_types() -> list[str]:
    """type_id literals from BUILTIN_WIDGET_TYPES — the single source of truth."""
    src = CORE_WIDGETS.read_text()
    return sorted(set(__import__("re").findall(r'type_id:\s*"([a-z-]+)"', src)))


def extract_frontend_widget_types() -> list[str]:
    """builtInTypes from Renderers.tsx — the frontend's copy of the same list."""
    src = RENDERERS.read_text()
    start = src.find("builtInTypes")
    # exact array span: builtInTypes ... ] — a wide window once matched the
    # bare word 'undefined' and false-alarmed the gate
    end = src.find("]", start)
    seg = src[start:end if end > 0 else start + 2000]
    return sorted(set(__import__("re").findall(r"['\"]([a-z][a-z-]+)['\"]", seg)))


def check_widget_type_sources(manifest: dict) -> list[str]:
    """Core source, frontend copy, and frozen manifest must all agree.

    Three drift incidents (gauge in API hints, gauge in agent intent
    examples, counter/metric-card missing from RESERVED_IDS) all traced to
    hand-maintained copies of this list. Any side changing without the
    others now fails CI here.
    """
    problems: list[str] = []
    core = extract_core_widget_types()
    front = extract_frontend_widget_types()
    frozen = manifest.get("widget_types")
    if frozen is None:
        problems.append("manifest missing widget_types — regenerate with --regen")
        return problems
    if front != core:
        only_front = sorted(set(front) - set(core))
        only_core = sorted(set(core) - set(front))
        problems.append(
            f"widget types: Renderers.tsx != core BUILTIN_WIDGET_TYPES "
            f"(only-frontend={only_front}, only-core={only_core})")
    if sorted(frozen) != core:
        problems.append(
            f"widget types: manifest (frozen) != core source — regenerate "
            f"with --regen after an INTENTIONAL list change "
            f"(manifest-only={sorted(set(frozen) - set(core))}, "
            f"core-only={sorted(set(core) - set(frozen))})")
    return problems


def generate() -> dict:
    binary = _bin()
    if not binary:
        print("no neomind binary found — cannot --regen", file=sys.stderr)
        sys.exit(2)
    top = subprocess.run([binary, "--help"], capture_output=True, text=True).stdout
    domains = re.findall(r"^  ([a-z][a-z0-9-]+)\s{2,}", top, re.M)
    manifest: dict[str, dict[str, list[str]]] = {}
    for dom in domains:
        out = subprocess.run([binary, dom, "--help"], capture_output=True, text=True).stdout
        verbs = re.findall(r"^  ([a-z][a-z0-9-]+)\s{2,}", out, re.M)
        manifest[dom] = {}
        for verb in verbs:
            vout = subprocess.run([binary, dom, verb, "--help"],
                                  capture_output=True, text=True).stdout
            flags = sorted(set(FLAG_RE.findall(vout)))
            manifest[dom][verb] = flags
    return {"_meta": {"note": "regenerate with check_cli_drift.py --regen after CLI changes"},
            "domains": manifest,
            "widget_types": extract_core_widget_types()}


def _iter_expected():
    """Yield (case_path, case_id, command, substrings) for every case.

    substrings is normalized to [(scope_cmd_or_None, substring)].
    """
    sys.path.insert(0, str(HERE / "lib"))
    import hard_signal as hs  # noqa: E402
    for f in sorted(glob.glob(str(HERE / "cases" / "**" / "*.json"), recursive=True)):
        d = json.load(open(f))
        e = hs.derive_expected(d)
        subs = e.get("substrings") or []
        if isinstance(subs, dict):
            pairs = [(scope, s) for scope, lst in subs.items() for s in (lst or [])]
        else:
            pairs = [(None, s) for s in subs]
        for c in e["commands"]:
            yield f, d.get("id"), c, pairs
        # any-of groups: validate EVERY member against the CLI tree so a
        # renamed verb can't hide inside a group
        for g in e.get("groups") or []:
            if len(g) > 1:
                for c in g:
                    yield f, d.get("id"), c, pairs


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--regen", action="store_true",
                    help="regenerate cli_manifest.json from the built binary")
    args = ap.parse_args()

    if args.regen:
        MANIFEST.write_text(json.dumps(generate(), indent=1, sort_keys=True) + "\n")
        print(f"manifest regenerated: {MANIFEST}")
        return 0

    if not MANIFEST.exists():
        print(f"{MANIFEST} missing — run with --regen once", file=sys.stderr)
        return 2
    domains = json.loads(MANIFEST.read_text())["domains"]

    problems: list[str] = []
    n_cmds = n_flags = 0
    # substring flags are scoped (dict) or case-wide (list). A case-wide flag
    # is valid if ANY expected command takes it; a scoped flag must be taken
    # by its own scope command — that precision is why the dict shape exists.
    case_flags: dict[str, list[str]] = {}
    scoped_flags: list[tuple[str, str, str]] = []  # (cid, scope, flag)
    for path, cid, cmd, subs in _iter_expected():
        toks = cmd.split()
        if len(toks) < 2 or toks[0] != "neomind":
            continue
        dom = toks[1]
        verb = toks[2] if len(toks) > 2 else None
        if dom not in domains:
            problems.append(f"{cid}: unknown domain '{dom}' in `{cmd}`")
            continue
        if verb and not verb.startswith("-"):
            if verb not in domains[dom]:
                problems.append(f"{cid}: unknown verb '{dom} {verb}' in `{cmd}`")
                continue
            n_cmds += 1
            known = set(domains[dom][verb])
            for scope, sub in subs:
                for flag in FLAG_RE.findall(sub):
                    n_flags += 1
                    if scope is not None:
                        if scope == cmd:
                            if flag not in known:
                                problems.append(
                                    f"{cid}: substring flag {flag} not taken by `{cmd}`")
                    else:
                        case_flags.setdefault(cid, []).append(flag)
    # case-wide flags: valid if any of that case's expected commands takes them
    sys.path.insert(0, str(HERE / "lib"))
    import hard_signal as hs  # noqa: E402
    for f in sorted(glob.glob(str(HERE / "cases" / "**" / "*.json"), recursive=True)):
        d = json.load(open(f))
        cid = d.get("id")
        if cid not in case_flags:
            continue
        union: set[str] = set()
        for c in hs.derive_expected(d)["commands"]:
            toks = c.split()
            if len(toks) >= 3 and toks[1] in domains and toks[2] in domains[toks[1]]:
                union |= set(domains[toks[1]][toks[2]])
        for flag in case_flags[cid]:
            if flag not in union:
                problems.append(f"{cid}: case-wide substring flag {flag} "
                                f"not taken by any expected command")
    problems.extend(check_widget_type_sources(json.loads(MANIFEST.read_text())))

    print(f"checked {n_cmds} domain/verb expectations, {n_flags} substring flags "
          f"against {len(domains)} CLI domains; "
          f"{len(json.loads(MANIFEST.read_text()).get('widget_types', []))} widget types "
          f"(core==frontend==manifest)")
    if problems:
        print("\nCLI drift:")
        for p in problems:
            print(f"  {p}")
        return 1
    print("no drift between case expectations and CLI manifest")
    return 0


if __name__ == "__main__":
    sys.exit(main())
