#!/usr/bin/env python3
"""CLI -> API field drift: keys the CLI writes that no request struct declares.

The CLI is a pure HTTP client — it builds a JSON body and posts it. The API
deserializes into a `*Request` struct, and none of those use
`deny_unknown_fields`, so an undeclared key is **accepted and discarded**. The
CLI reports success and the field never lands.

Two shipped parameters went that way: `--enable-tool-chaining` (its field had
been removed from the API DTOs) and `--message-type` (the CLI wrote `type`,
the struct declared `message_type`). Neither could be caught by a test,
because a dropped field leaves no trace — not in the response, not in a log,
not in a stored row.

Output: a JSON manifest { "orphans": {key: "file that writes it"} }.
Exit code is 0 even with orphans; the Rust test decides what to do with them.
"""
import argparse
import json
import pathlib
import re
import sys

FIELD = re.compile(r"^\s*(?:pub\s+)?([a-z_][a-z0-9_]*)\s*:", re.M)
BODY_KEY = re.compile(r'body\["([a-z_][a-z0-9_]*)"\]')
STRUCT_HEAD = re.compile(r"^\s*(?:pub\s+)?struct\s+\w*(?:Request|Config)\w*\s*\{", re.M)


def production_only(src: str) -> str:
    """Drop everything from `#[cfg(test)]` on — fixtures write bodies too."""
    i = src.find("#[cfg(test)]")
    return src if i < 0 else src[:i]


def declared_fields(crates: pathlib.Path) -> set:
    fields = set()
    for f in crates.rglob("*.rs"):
        try:
            src = f.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for head in STRUCT_HEAD.finditer(src):
            # Walk to the closing brace at the same nesting depth.
            depth, i = 1, head.end()
            while i < len(src) and depth:
                if src[i] == "{":
                    depth += 1
                elif src[i] == "}":
                    depth -= 1
                i += 1
            fields |= set(FIELD.findall(src[head.end():i]))
    return fields


def written_keys(cli_src: pathlib.Path) -> dict:
    keys = {}
    for f in cli_src.rglob("*.rs"):
        try:
            src = production_only(f.read_text(encoding="utf-8", errors="replace"))
        except OSError:
            continue
        for k in BODY_KEY.findall(src):
            keys.setdefault(k, f.name)
    return keys


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", help="write the manifest here")
    ap.add_argument("--root", default=".", help="workspace root")
    args = ap.parse_args()

    root = pathlib.Path(args.root).resolve()
    crates = root / "crates"
    declared = declared_fields(crates)
    written = written_keys(crates / "neomind-cli-ops" / "src")

    manifest = {
        "declared_count": len(declared),
        "written_count": len(written),
        "orphans": {k: v for k, v in sorted(written.items()) if k not in declared},
    }
    text = json.dumps(manifest, indent=2, ensure_ascii=False)
    if args.out:
        pathlib.Path(args.out).write_text(text, encoding="utf-8")
    else:
        print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
