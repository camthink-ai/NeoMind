#!/usr/bin/env python3
"""Extract a version's release notes from CHANGELOG.md.

Single source of truth for NeoMind's three release-note channels:
  --mode full   -> GitHub Release body (long-form; no length concern)
  --mode short  -> OTA updater `notes` field + Discord embed (glanceable)

The short mode renders a scannable summary from structures you already write:
  * the lead paragraph under the `## [x.y.z]` header, and
  * the first bullet of each `### Subsection` (the `**bold lead-in**` style).
If a section contains an explicit `### Highlights` block, its bullets are used
verbatim (override) — handy on big releases where you want tight control over
the one-screen summary.

Fails safe: a miss (version not found / file unreadable) prints a minimal
fallback so the release pipeline never breaks. Pass --check to exit non-zero
instead (for CI gating / version-consistency tests).

Usage:
  extract_changelog.py --version 0.9.13 --mode short
  extract_changelog.py --version 0.9.13            # default: full
  extract_changelog.py --version 0.9.13 --check    # exit 1 if not found
"""

from __future__ import annotations

import argparse
import os
import re
import sys

REPO_URL = os.environ.get("NEOMIND_REPO_URL", "https://github.com/camthink-ai/NeoMind")

SHORT_BUDGET = 1500  # keep OTA dialog / Discord embed compact
LEAD_BUDGET = 320
BULLET_BUDGET = 140


def find_section(text: str, version: str):
    """Return (header_line, body, date) or (None, None, None) if not found."""
    ver = version.lstrip("v")
    header_re = re.compile(
        r"^##\s*\[" + re.escape(ver) + r"\]\s*(?:-\s*(.+?))?\s*$",
        re.MULTILINE,
    )
    m = header_re.search(text)
    if not m:
        return None, None, None
    date = (m.group(1) or "").strip()
    header_line = m.group(0).rstrip()
    rest = text[m.end():]
    nxt = re.search(r"\n##\s*\[", rest)
    body = rest if not nxt else rest[: nxt.start()]
    return header_line, body, date


def parse_section(body: str):
    """Split body into (lead_paragraph, [(subsection_name, [lines]), ...])."""
    lines = body.split("\n")
    lead, i = [], 0
    while i < len(lines) and not lines[i].startswith("###"):
        lead.append(lines[i])
        i += 1
    lead_text = "\n".join(lead).strip()

    subs = []
    cur_name, cur_lines = None, []
    for line in lines[i:]:
        if line.startswith("### "):
            if cur_name is not None:
                subs.append((cur_name, cur_lines))
            cur_name, cur_lines = line[4:].strip(), []
        elif cur_name is not None:
            cur_lines.append(line)
    if cur_name is not None:
        subs.append((cur_name, cur_lines))
    return lead_text, subs


def _cap(s: str, n: int) -> str:
    """Collapse whitespace and cap to n chars with an ellipsis."""
    s = re.sub(r"\s+", " ", s).strip()
    return s if len(s) <= n else s[: n - 1].rstrip() + "…"


def _first_bullet(lines) -> str | None:
    """First bullet, joining indented continuation lines into one string.

    No sentence splitting — a naive splitter breaks on abbreviations like
    "e.g." / "i.e.". BULLET_BUDGET caps with an ellipsis instead.
    """
    for idx, bl in enumerate(lines):
        s = bl.strip()
        if not s.startswith("- "):
            continue
        parts = [s[2:]]
        for cont in lines[idx + 1:]:
            if not cont or cont[0] not in " \t":  # only indented continuations
                break
            cs = cont.strip()
            if not cs or cs.startswith("- ") or cont.startswith("###"):
                break
            parts.append(cs)
        return " ".join(parts).strip()
    return None


def _debold(s: str) -> str:
    # "**Bold lead-in**: rest" -> "Bold lead-in: rest" (keep whatever follows verbatim)
    return re.sub(r"^\*\*(.+?)\*\*", r"\1", s).strip()


def build_short(version: str, date: str, lead: str, subs) -> str:
    ver = version.lstrip("v")
    # Bold (not "#") so it renders in both react-markdown and Discord embeds
    # (Discord does not render markdown headings).
    title = f"**NeoMind {ver}**" + (f" — {date}" if date else "")
    link = f"[Full release notes]({REPO_URL}/releases/tag/v{ver})"

    highlights = next((bl for n, bl in subs if n.lower() == "highlights"), None)
    if highlights:
        cands = [b.rstrip() for b in highlights if b.strip().startswith("-")]
    else:
        cands = [
            f"- **{n}**: {_cap(_debold(_first_bullet(bl)), BULLET_BUDGET)}"
            for n, bl in subs
            if _first_bullet(bl)
        ]

    # Fit as many bullets as the budget allows (reserve title + lead + link).
    lead_line = _cap(lead, LEAD_BUDGET) if lead else ""
    overhead = len(title) + 2 + len(link) + 2 + (len(lead_line) + 2 if lead_line else 0)

    chosen, used, truncated = [], overhead, False
    for c in cands:
        if used + len(c) + 1 > SHORT_BUDGET:
            truncated = True
            break
        chosen.append(c)
        used += len(c) + 1

    out = [title, ""]
    if lead_line:
        out += [lead_line, ""]
    out += chosen
    if truncated:
        out.append("- …")
    out += ["", link]
    return "\n".join(out).strip()


def build_full(header_line: str, body: str) -> str:
    return (header_line + "\n" + body).strip()


def main() -> None:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--version", required=True, help="version, e.g. 0.9.13 (v prefix ok)")
    ap.add_argument("--mode", choices=["short", "full"], default="full")
    ap.add_argument("--changelog", default="CHANGELOG.md")
    ap.add_argument(
        "--check",
        action="store_true",
        help="exit non-zero if the section is missing (CI gating)",
    )
    args = ap.parse_args()

    try:
        with open(args.changelog, encoding="utf-8") as f:
            text = f.read()
    except OSError as e:
        if args.check:
            sys.exit(f"cannot read {args.changelog}: {e}")
        print(f"NeoMind {args.version.lstrip('v')}")
        return

    header_line, body, date = find_section(text, args.version)
    if header_line is None:
        if args.check:
            sys.exit(f"version {args.version} not found in {args.changelog}")
        print(f"NeoMind {args.version.lstrip('v')}")  # graceful fallback
        return

    if args.mode == "short":
        lead, subs = parse_section(body)
        print(build_short(args.version, date, lead, subs))
    else:
        print(build_full(header_line, body))


if __name__ == "__main__":
    main()
