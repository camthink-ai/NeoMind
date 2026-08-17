#!/usr/bin/env python3
"""Parallel sharded full-eval runner.

Splits the en case set into N balanced workflow shards, runs N
`run_eval.py run` workers in parallel (each with a private MQTT broker
port via NEOMIND_EVAL_MQTT_PORT and its own run dir), then merges the
shards into one scores.jsonl and prints a summary vs a baseline.

Prerequisite: llama-server already running with -np N (shared slots).
Usage:
  python eval/shard_run.py --workers 4 --out-dir eval/runs/<name> \
      [--baseline eval/runs/lfm26b-full-final/scores.jsonl]
"""
import argparse, json, os, subprocess, sys, time
from pathlib import Path

ROOT = Path(__file__).resolve().parent

def workflow_counts(lang="en"):
    cases = ROOT / "cases" / lang
    return {d.name: len(list(d.glob("*.json"))) for d in sorted(cases.iterdir()) if d.is_dir()}

def balanced_shards(counts, n):
    """Greedy largest-first bin packing by case count."""
    shards = [[] for _ in range(n)]
    loads = [0] * n
    for wf, c in sorted(counts.items(), key=lambda kv: -kv[1]):
        i = loads.index(min(loads))
        shards[i].append(wf)
        loads[i] += c
    return shards, loads

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument("--out-dir", required=True)
    ap.add_argument("--baseline", default=None)
    ap.add_argument("--lang", default="en")
    ap.add_argument("--case-timeout", type=int, default=600)
    args = ap.parse_args()

    counts = workflow_counts(args.lang)
    total = sum(counts.values())
    shards, loads = balanced_shards(counts, args.workers)
    print(f"[shard] {total} cases / {len(counts)} workflows -> {args.workers} shards: {loads}")
    for i, s in enumerate(shards):
        print(f"  shard {i}: {len(s)} workflows, {loads[i]} cases :: {','.join(s[:6])}{'…' if len(s) > 6 else ''}")

    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)

    env_common = dict(os.environ)
    env_common.update({
        "AGENT_LLM_API_KEY": os.environ.get("AGENT_LLM_API_KEY", "none"),
        "AGENT_LLM_ENDPOINT": os.environ["AGENT_LLM_ENDPOINT"],
        "AGENT_LLM_MODEL": os.environ.get("AGENT_LLM_MODEL", "lfm2.5-2.6b"),
        "AGENT_LLM_BACKEND_TYPE": "llamacpp",
        "AGENT_LLM_THINKING": "false",
    })

    procs = []
    t0 = time.time()
    for i, shard in enumerate(shards):
        if not shard:
            continue
        env = dict(env_common)
        env["NEOMIND_EVAL_MQTT_PORT"] = str(1883 + 10 + i)  # 1893, 1894, …
        logf = open(out / f"shard{i}.log", "w")
        p = subprocess.Popen(
            [sys.executable, str(ROOT / "run_eval.py"), "run",
             "--lang", args.lang,
             "--workflow", ",".join(shard),
             "--run-dir", str(out / f"shard{i}"),
             "--case-timeout", str(args.case_timeout)],
            env=env, stdout=logf, stderr=subprocess.STDOUT, cwd=str(ROOT.parent),
        )
        procs.append((i, p, logf))
        print(f"[shard] worker {i} launched (pid {p.pid}, mqtt :{env['NEOMIND_EVAL_MQTT_PORT']})")

    rc_all = 0
    for i, p, logf in procs:
        rc = p.wait()
        logf.close()
        print(f"[shard] worker {i} exited rc={rc} ({time.time()-t0:.0f}s elapsed)")
        rc_all |= rc
    if rc_all:
        print(f"[shard] WARNING: at least one worker exited non-zero")

    # merge
    merged = out / "scores.jsonl"
    n = 0
    with merged.open("w") as mf:
        for i in range(args.workers):
            sp = out / f"shard{i}" / "scores.jsonl"
            if sp.exists():
                for line in sp.read_text().splitlines():
                    if line.strip():
                        mf.write(line + "\n")
                        n += 1
    print(f"[shard] merged {n} cases -> {merged}")

    # summary
    rows = [json.loads(l) for l in merged.read_text().splitlines() if l.strip()]
    cmdn = [r for r in rows if r["hard"].get("cmd_ok") is not None]
    cm = sum(1 for r in cmdn if r["hard"]["cmd_ok"])
    to = sum(1 for r in rows if r.get("status") == "agent_error")
    took = sum(1 for r in rows if r["hard"].get("tool_ok"))
    print(f"[shard] RESULT: {len(rows)} cases | cmd_ok {cm}/{len(cmdn)} ({100*cm/max(1,len(cmdn)):.1f}%) | tool_ok {took}/{len(rows)} | timeouts {to}")

    if args.baseline:
        base = {r["case_id"]: r for r in map(json.loads, open(args.baseline)) if True}
        def verdict(r):
            if r is None: return None
            if r.get("status") == "agent_error": return False
            h = r["hard"]
            if h.get("cmd_ok") is not None: return h["cmd_ok"]
            return h.get("tool_ok")
        up = [c for c in {r["case_id"] for r in rows} if c in base
              and verdict(rows_by := {r["case_id"]: r for r in rows}[c]) and not verdict(base[c])]
        dn = [c for c in {r["case_id"] for r in rows} if c in base
              and not verdict({r["case_id"]: r for r in rows}[c]) and verdict(base[c])]
        print(f"[shard] vs baseline: FAIL->PASS {len(up)} :: {sorted(up)[:12]}")
        print(f"[shard] vs baseline: PASS->FAIL {len(dn)} :: {sorted(dn)[:12]}")

if __name__ == "__main__":
    main()
