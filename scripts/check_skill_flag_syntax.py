#!/usr/bin/env python3
"""Audit builtin skill docs against the real CLI signatures in commands.rs.

Catches the write-metric class of bug: doc shows positional args where the
CLI requires --flags (or vice versa), unknown flags, unknown commands,
missing required flags, too many positionals.
"""
import re
import sys
from pathlib import Path

ROOT = Path("/Users/shenmingming/CamThink Project/NeoMind")
CMDS = ROOT / "crates/neomind-cli-ops/src/dispatch/commands.rs"
SKILLS = ROOT / "crates/neomind-agent/src/skills/builtins"

# ---------------------------------------------------------------- CLI parse

def kebab(name: str) -> str:
    s = name.replace('_', '-')  # clap kebab-cases snake_case fields too
    s = re.sub(r'([a-z0-9])([A-Z])', r'\1-\2', s)
    s = re.sub(r'([A-Z]+)([A-Z][a-z])', r'\1-\2', s)
    return s.lower()

def split_enum_blocks(src: str):
    """Return {enum_name: body} for every `pub enum X { ... }` at top level."""
    out = {}
    for m in re.finditer(r'^pub enum (\w+) \{\n(.*?)^\}\n', src, re.S | re.M):
        out[m.group(1)] = m.group(2)
    return out

def split_variants(body: str):
    """Split an enum body into variants: [(name, inner)]."""
    variants = []
    cur_name, cur = None, []
    for line in body.splitlines():
        vunit = re.match(r'^    ([A-Z][A-Za-z0-9]*)(?: \{\})?,?\s*$', line)
        vm = re.match(r'^    ([A-Z][A-Za-z0-9]*) \{', line)
        if vunit and cur_name is None:
            variants.append((vunit.group(1), []))
        elif vm:
            cur_name, cur = vm.group(1), []
        elif cur_name is not None:
            if re.match(r'^    \},?$', line):
                variants.append((cur_name, cur))
                cur_name, cur = None, []
            else:
                cur.append(line)
    return variants

ENUM_SEG = re.compile(r'^\s*(?:[A-Z][\w \-]{1,30}: )?"?([a-z0-9_-]{2,})"?')

def extract_enum(doc_lines):
    """Conservative value-enum extraction from a field's doc comments.

    A line like `Schedule type: interval | cron | event | manual. Required.`
    yields {interval, cron, event, manual}. Lines are REJECTED unless every
    `|`-segment starts with a clean lowercase token and no segment carries
    ` or ` (prose that hides a second value, e.g. `"free" (...), or
    "structured" (...)`). False negatives are fine; false positives are not.
    """
    # doc comments can wrap mid-enum ("... greater_equal |\n/// less_equal | ...")
    merged, k = [], 0
    while k < len(doc_lines):
        buf = doc_lines[k].lstrip('/ ').strip()
        while buf.endswith('|') and k + 1 < len(doc_lines):
            k += 1
            buf = buf[:-1].rstrip() + ' | ' + doc_lines[k].lstrip('/ ').strip()
        merged.append(buf)
        k += 1
    for line in merged:
        if '|' not in line:
            continue
        segs = line.split('|')
        if not 2 <= len(segs) <= 5:
            continue
        vals = set()
        ok = True
        for seg in segs:
            if ' or ' in seg:
                ok = False
                break
            m = ENUM_SEG.match(seg)
            if not m:
                ok = False
                break
            vals.add(m.group(1))
        if ok and len(vals) == len(segs):
            return vals
    return None


def parse_variant(inner):
    """Parse variant fields -> dict(positionals, flags, sub_enum)."""
    positionals, flags = [], {}
    sub_enum = None
    i = 0
    raw = [l.strip() for l in inner if l.strip()]
    doc_buf = []
    # join multi-line `#[arg(` ... `)]` attribute blocks into one line
    lines, j = [], 0
    while j < len(raw):
        if raw[j].startswith('#[arg(') and not raw[j].endswith(')]'):
            buf = raw[j]
            while j < len(raw) and not raw[j].endswith(')]'):
                j += 1
                buf += ' ' + raw[j] if j < len(raw) else ''
            lines.append(buf)
            j += 1
        else:
            lines.append(raw[j])
            j += 1
    while i < len(lines):
        l = lines[i]
        if '#[command(subcommand)]' in l:
            m = re.match(r'\w+:\s*(\w+),?$', lines[i + 1]) if i + 1 < len(lines) else None
            if m:
                sub_enum = m.group(1)
            i += 2
            continue
        if l.startswith('///'):
            doc_buf.append(l)
            i += 1
            continue
        m = re.match(r'#\[arg\((.*)\)\]$', l)
        if m:
            attr = m.group(1)
            # field line may follow immediately
            fm = re.match(r'(?:r#)?(\w+):\s*([^,]+),?$', lines[i + 1]) if i + 1 < len(lines) else None
            if fm:
                fname, ftype = fm.group(1), fm.group(2).strip()
                enum_vals = extract_enum(doc_buf)
                is_opt = ftype.startswith('Option<')
                is_bool = ftype == 'bool'
                has_long = re.search(r'\blong\b', attr) and 'long_about' not in attr
                long_eq = re.search(r'long\s*=\s*"([^"]+)"', attr)
                alias = re.search(r'alias\s*=\s*"([^"]+)"', attr)
                required = 'required = true' in attr or 'required =' in attr and 'true' in attr
                has_default = 'default_value' in attr
                takes_value = not is_bool
                if has_long or long_eq:
                    lname = long_eq.group(1) if long_eq else kebab(fname)
                    entry = {'required': required and not is_opt and not has_default,
                             'takes_value': takes_value,
                             'optional': is_opt or has_default,
                             'enum': enum_vals}
                    flags[lname] = entry
                    if alias:
                        flags[alias.group(1)] = entry
                else:
                    positionals.append(fname)
                i += 2
                doc_buf = []
                continue
        else:
            # field without #[arg]: bare positional (e.g. `id: String,`)
            fm = re.match(r'(?:r#)?(\w+):\s*(.+?),?$', l)
            if fm and not fm.group(2).startswith('Command') and not l.startswith('///'):
                ftype = fm.group(2).strip()
                if not ftype.endswith('Command'):
                    positionals.append(fm.group(1))
        i += 1
    return {'positionals': positionals, 'flags': flags, 'sub': sub_enum}

def load_cli():
    src = CMDS.read_text()
    enums = split_enum_blocks(src)
    cli = {}       # tuple path -> spec
    trees = {}     # enum name -> subtree

    def build(enum_name):
        if enum_name in trees:
            return trees[enum_name]
        tree = {}
        trees[enum_name] = tree
        for vname, inner in split_variants(enums[enum_name]):
            vs = parse_variant(inner)
            path_name = kebab(vname)
            if vs['sub']:
                sub = build(vs['sub'])
                tree[path_name] = sub
                # NOTE: cli keys are only meaningful for the TOP-level call;
                # nested build() calls also write bare keys (e.g. ('list',)
                # from LlmCommand). Coverage mode walks `trees` instead.
            else:
                tree[path_name] = {'__cmd__': vs}
                cli[(path_name,)] = vs
        return tree

    build('Command')
    return cli, trees

# ---------------------------------------------------------------- doc parse

def tokenize(s):
    toks, cur, q = [], '', None
    for ch in s:
        if q:
            if ch == q:
                q = None
                cur += ch  # keep quotes so flags=value strings survive? drop them
                cur = cur  # noqa
            else:
                cur += ch
        elif ch in '"\'':
            q = ch
            cur += ch
        elif ch.isspace():
            if cur:
                toks.append(cur); cur = ''
        else:
            cur += ch
    if cur:
        toks.append(cur)
    return toks

def strip_comment(line):
    idx, inq = None, None
    for i, ch in enumerate(line):
        if inq:
            if ch == inq:
                inq = None
        elif ch in '"\'':
            inq = ch
        elif ch == '#' and i > 0 and line[i-1] == ' ':
            idx = i
            break
    return line[:idx] if idx else line

def extract_commands(text):
    """Yield (lineno, command_string) for every neomind command in a doc."""
    lines = text.splitlines()
    in_fence = False
    i = 0
    while i < len(lines):
        line = lines[i]
        if line.strip().startswith('```'):
            in_fence = not in_fence
            i += 1
            continue
        if in_fence:
            buf, start = '', i
            while i < len(lines) and lines[i].strip().startswith('```') is False:
                piece = lines[i].strip()
                if piece.startswith('$ '):
                    piece = piece[2:]
                if piece.endswith('\\'):
                    buf += piece[:-1].strip() + ' '
                    i += 1
                    continue
                buf += piece
                break
            else:
                i += 1
            joined = strip_comment(buf.strip())
            if joined.startswith('neomind '):
                yield start + 1, joined
            i += 1
            continue
        # inline spans (tables, prose)
        if 'NOT exist' in line or 'do NOT' in line:
            i += 1
            continue
        for m in re.finditer(r'`([^`]+)`', line):
            span = m.group(1).strip()
            if span.startswith('neomind ') and not span.startswith('neomind x'):
                yield i + 1, span
        i += 1

# ---------------------------------------------------------------- checking

def check_cmd(cli, trees, words):
    """Return list of problems for one tokenized command."""
    problems = []
    i = 1  # skip 'neomind'
    node = trees.get('Command')
    spec = None
    while i < len(words):
        if '__cmd__' in node:
            spec = node['__cmd__']
            break
        w = words[i]
        if w.startswith('-'):
            problems.append(f'flag before/during command path: {w}')
            return problems
        nxt = node.get(w) if isinstance(node, dict) else None
        if nxt is not None:
            node = nxt
            i += 1
            continue
        problems.append(f'unknown command path token: {w!r} (under "neomind '
                        f'{words[1:i] and " ".join(words[1:i]) or ""}")')
        return problems
    if spec is None:
        if isinstance(node, dict) and '__cmd__' in node:
            spec = node['__cmd__']
        else:
            # bare group name used as a doc section label (e.g. `neomind push`)
            # — clap answers with the subcommand help; acceptable in tables
            return problems
    pos, seen_flags = list(spec['positionals']), []
    shorts = {'-' + f[0] for f in spec['flags'] if f}
    def flag_ok(name):
        return name in spec['flags'] or name in ('verbose', 'help', 'model')
    def takes_value(name):
        e = spec['flags'].get(name)
        return e['takes_value'] if e else True
    def allowed_vals(name):
        e = spec['flags'].get(name)
        return (e or {}).get('enum')
    while i < len(words):
        w = words[i].strip('[]')
        if w.startswith('--'):
            name = w[2:].split('=', 1)[0]
            if not flag_ok(name):
                problems.append(f'unknown flag --{name}')
                i += 1
                continue
            val = None
            if '=' in w:
                val = w.split('=', 1)[1]
                i += 1
            elif takes_value(name):
                val = words[i + 1] if i + 1 < len(words) else None
                i += 2
            else:
                i += 1
            seen_flags.append(name)
            allowed = allowed_vals(name)
            v = (val or '').strip('\'"')
            if (allowed and v and re.fullmatch(r'[a-z0-9_-]{2,}', v)
                    and v not in allowed and not v.startswith('<')):
                problems.append(f'--{name} {v}: value not in CLI doc enum '
                                f'{sorted(allowed)}')
        elif w.startswith('-') and len(w) == 2:
            if w not in shorts and w not in ('-v', '-h'):
                problems.append(f'unknown short flag {w}')
            i += 2 if i + 1 < len(words) and not words[i+1].startswith('-') and w not in shorts else (2 if w in shorts else 1)
        elif w in ('options', '...', '…') or w == '':
            i += 1
        else:
            if pos:
                pos.pop(0)
                i += 1
            else:
                problems.append(f'extra positional token {w!r} — CLI takes flags here '
                                f'(positionals: {list(spec["positionals"])})')
                i += 1
    for pname, fe in spec['flags'].items():
        if fe['required'] and pname not in seen_flags:
            problems.append(f'missing required flag --{pname}')
    return problems

# CLI commands deliberately NOT documented in any skill: server lifecycle,
# auth/admin, one-off CLI-side LLM invocation. The chat agent runs inside the
# already-authenticated API server and must not drive these. Adding a NEW
# undocumented command requires an entry here (with a reason) or a skill doc.
INTENTIONALLY_UNDOCUMENTED = {
    ("serve",), ("chat",), ("prompt",), ("list-models",),
    ("login",), ("logout",), ("whoami",), ("health",), ("logs",),
    ("check-update",), ("upgrade",), ("uninstall",),
    ("user", "list"), ("user", "reset-password"), ("user", "set-role"),
    ("api-key", "create"), ("api-key", "delete"), ("api-key", "list"),
    ("config", "export"), ("config", "import"), ("config", "validate"),
    ("data", "list"), ("device", "latest"), ("device", "types", "delete"),
    ("message", "delete"),
}

def leaf_paths(trees):
    out = set()
    def walk(node, prefix):
        if not isinstance(node, dict):
            return
        if '__cmd__' in node and prefix:
            out.add(tuple(prefix))
        for k, v in node.items():
            if k != '__cmd__' and isinstance(v, dict):
                walk(v, prefix + (k,))
    walk(trees.get('Command', {}), ())
    return out

def documented_paths(trees):
    import re as _re
    doc = set()
    for md in sorted(SKILLS.glob('*.md')):
        for _lineno, cmd in extract_commands(md.read_text()):
            words = tokenize(cmd)
            node = trees.get('Command')
            path = []
            for w in words[1:]:
                if not _re.match(r'^[a-z][a-z0-9_-]*$', w):
                    break
                nxt = node.get(w) if isinstance(node, dict) else None
                if nxt is not None:
                    node = nxt
                    path.append(w)
                else:
                    break
            if path:
                doc.add(tuple(path))
    return doc

def coverage_report(trees):
    """Reverse coverage: CLI commands that no skill doc mentions.

    Exit 1 if a command is neither documented nor whitelisted — a NEW CLI
    command added without a skill doc (or a whitelist reason) fails CI.
    """
    all_leaves = leaf_paths(trees)
    doc = documented_paths(trees)
    undoc = sorted(p for p in all_leaves if p not in doc)
    unexplained = [p for p in undoc if p not in INTENTIONALLY_UNDOCUMENTED]
    stale = [p for p in INTENTIONALLY_UNDOCUMENTED if p in doc or p not in all_leaves]

    print(f"CLI leaf commands: {len(all_leaves)}")
    print(f"documented in skills: {len(all_leaves & doc)}")
    print(f"undocumented: {len(undoc)} (whitelisted: {len(undoc) - len(unexplained)}, "
          f"UNEXPLAINED: {len(unexplained)})")
    for p in unexplained:
        print(f"  UNDOCUMENTED & UNWHITELISTED: neomind {' '.join(p)}")
    for p in stale:
        print(f"  whitelist entry stale (documented or gone): neomind {' '.join(p)}")
    return 1 if (unexplained or stale) else 0


def main():
    cli, trees = load_cli()
    print(f'CLI leaf commands: {len(leaf_paths(trees))}')
    total_cmds, issues = 0, 0
    for md in sorted(SKILLS.glob('*.md')):
        text = md.read_text()
        for lineno, cmd in extract_commands(text):
            if '<' in cmd.split('neomind ', 1)[1].split(' ')[0]:
                continue
            total_cmds += 1
            problems = check_cmd(cli, trees, tokenize(cmd))
            for p in problems:
                issues += 1
                print(f'{md.name}:{lineno}  {cmd}\n    -> {p}')
    print(f'\nchecked {total_cmds} concrete commands, {issues} problems')

def smoke_help(trees):
    """Runtime smoke: `<cmd> --help` for every documented command path.

    Compiles-time reflection (the Rust tests) and the shipped binary can
    drift (stale binary, feature flags). --help exits 0 and never starts
    the server, so this is safe to run anywhere the binary exists.
    """
    import shutil
    import subprocess
    # repo-fresh binary first; the PATH one may be an older install
    binary = next(
        (str(b) for b in (
            ROOT / 'target/release/neomind', ROOT / 'target/debug/neomind')
         if b.exists()), None) or shutil.which('neomind')
    if not binary:
        print('no neomind binary found (build first, or add to PATH)')
        return 1
    doc = documented_paths(trees)
    failures = 0
    for path in sorted(doc):
        argv = [binary, *path, '--help']
        r = subprocess.run(argv, capture_output=True, text=True, timeout=10)
        if r.returncode != 0:
            failures += 1
            print(f"FAIL ({r.returncode}) neomind {' '.join(path)} --help")
            print('   ', (r.stderr or r.stdout).strip().splitlines()[:1])
    print(f"smoke: {len(doc)} documented commands, {failures} failures ({binary})")
    return 1 if failures else 0


if __name__ == '__main__':
    import argparse
    ap = argparse.ArgumentParser()
    ap.add_argument('--coverage', action='store_true',
                    help='reverse coverage: undocumented CLI commands must be whitelisted')
    ap.add_argument('--smoke-help', action='store_true',
                    help='run `<cmd> --help` via the built binary for every documented command')
    args = ap.parse_args()
    if args.coverage:
        raise SystemExit(coverage_report(load_cli()[1]))
    if args.smoke_help:
        raise SystemExit(smoke_help(load_cli()[1]))
    main()
