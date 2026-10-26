#!/usr/bin/env python3
"""Perception-slot eval: does the vision model actually SEE?

The 40+ case agent gate asserts tool selection and platform state; it never
grades what a vision model says about an image. Choosing a perception model
(LFM2.5-VL-3B vs MiniCPM-V 4.7-1B/35B-A3B …) therefore needs its own harness:

    image + question -> VLM -> checks against a manifest

Scoring is deliberately mechanical (manifest `checks`, case-insensitive
substring): a hallucination that misses "red" or "circle" fails, and the same
image set gives comparable numbers across models. Use `--judge` for a
qualitative second opinion against an optional `reference` description.

Usage:
    # build the request matrix without calling anything
    python3 eval/perception_eval.py --dry-run

    # score a local llama-server (vision slot at :8082)
    python3 eval/perception_eval.py \
        --endpoint http://127.0.0.1:8082 --model LFM2.5-VL-3B

    # score a cloud model with a judge pass
    ANTHROPIC_API_KEY=... python3 eval/perception_eval.py \
        --endpoint https://api.deepseek.com --model deepseek-flash \
        --api-key $KEY --judge

Exit code 0 when every check passes.
"""
from __future__ import annotations

import argparse
import re
import base64
import json
import os
import sys
import time
import urllib.parse
from pathlib import Path

import requests

HERE = Path(__file__).resolve().parent
DEFAULT_MANIFEST = HERE / "perception" / "manifest.json"


def data_url(path: Path) -> str:
    mime = "image/png" if path.suffix.lower() == ".png" else "image/jpeg"
    return f"data:{mime};base64," + base64.b64encode(path.read_bytes()).decode()


def ask(endpoint: str, model: str, api_key: str, image: str, prompt: str,
        max_tokens: int, timeout: float, effort: str | None = None) -> tuple[str, dict, float]:
    started = time.monotonic()
    body: dict = {
        "model": model,
        "temperature": 0.2,
        "max_tokens": max_tokens,
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {"type": "image_url", "image_url": {"url": image}},
                ],
            }
        ],
    }
    # Reasoning-first endpoints (deepseek-flash &c.) can spend the whole budget
    # in thinking and return an EMPTY content — pass a low effort for
    # perception asks unless the caller wants the model's default.
    if effort:
        body["effort"] = effort
    r = requests.post(
        endpoint.rstrip("/") + "/v1/chat/completions",
        headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
        json=body,
        timeout=timeout,
    )
    latency = time.monotonic() - started
    if not r.ok:
        raise RuntimeError(f"{r.status_code}: {r.text[:400]}")
    body_j = r.json()
    msg = body_j["choices"][0]["message"]
    text = (msg.get("content") or "").strip()
    if not text:
        # thinking models sometimes put it all in reasoning_content — surface
        # that instead of silently scoring an empty answer as a miss.
        text = ((msg.get("reasoning_content") or "").strip())[:400]
        if text:
            text = "[reasoning-only] " + text
    return text, body_j.get("usage") or {}, latency


def judge(reference: str, answer: str, image_name: str) -> dict:
    """Optional qualitative pass via Claude. Returns {score, reason}."""
    key = os.environ.get("ANTHROPIC_API_KEY")
    if not key:
        raise RuntimeError("--judge needs ANTHROPIC_API_KEY")
    import anthropic

    client = anthropic.Anthropic(api_key=key)
    msg = client.messages.create(
        model=os.environ.get("EVAL_JUDGE_MODEL", "claude-opus-4-6"),
        max_tokens=400,
        messages=[{
            "role": "user",
            "content": (
                "Grade a vision model's description of an image against a reference description.\n"
                f"Image: {image_name}\nReference: {reference}\nModel answer: {answer}\n\n"
                "Return JSON only: {\"score\": 1-5, \"reason\": \"<one sentence>\"}. "
                "5 = fully matches the reference's salient content; 3 = partly; 1 = wrong or empty."
            ),
        }],
    )
    text = "".join(b.text for b in msg.content if getattr(b, "type", "") == "text")
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return {"score": None, "reason": text[:200]}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", default=str(DEFAULT_MANIFEST))
    ap.add_argument("--endpoint", default=os.environ.get("PERCEPTION_ENDPOINT", "http://127.0.0.1:8082"))
    ap.add_argument("--model", default=os.environ.get("PERCEPTION_MODEL", "LFM2.5-VL-3B"))
    ap.add_argument("--api-key", default=os.environ.get("PERCEPTION_API_KEY", "none"))
    ap.add_argument("--max-tokens", type=int, default=900,
                   help="generous default: thinking-first models spend budget on reasoning before content")
    ap.add_argument("--timeout", type=float, default=180.0)
    ap.add_argument("--effort", default=None,
                   help="pass e.g. low/high/max to endpoints that support it (deepseek-flash)")
    ap.add_argument("--judge", action="store_true", help="extra Claude pass vs `reference`")
    ap.add_argument("--out", default="", help="write a markdown report here")
    ap.add_argument("--dry-run", action="store_true", help="print the matrix, call nothing")
    args = ap.parse_args()

    manifest_path = Path(args.manifest)
    manifest = json.loads(manifest_path.read_text())
    entries = manifest.get("images") or []
    if not entries:
        print("manifest has no images", file=sys.stderr)
        return 2
    base = manifest_path.parent

    if args.dry_run:
        for e in entries:
            checks = ", ".join(c["name"] for c in e.get("checks") or [])
            print(f"{e['file']:40s} checks=[{checks}]")
        return 0

    rows = []
    total_checks = passed_checks = 0
    for e in entries:
        path = (base / e["file"]).resolve()
        if not path.exists():
            print(f"missing image: {path}", file=sys.stderr)
            return 2
        prompt = e.get("prompt") or "Describe this image in detail."
        try:
            answer, usage, latency = ask(
                args.endpoint, args.model, args.api_key, data_url(path), prompt,
                args.max_tokens, args.timeout, args.effort,
            )
        except Exception as exc:  # noqa: BLE001 - one bad image must not hide the rest
            rows.append({"file": e["file"], "error": str(exc), "checks": [], "answer": ""})
            print(f"  ERROR {e['file']}: {exc}", file=sys.stderr)
            continue

        low = answer.lower()
        checks = []
        for c in e.get("checks") or []:
            if c.get("regex"):
                # word-boundary regex — for counts, where substring matching
                # lets "75" satisfy a check for "7"
                hit = re.search(c["regex"], answer, re.I) is not None
            else:
                hit = any(tok.lower() in low for tok in c.get("any_of", []))
            checks.append({"name": c["name"], "pass": hit})
            total_checks += 1
            passed_checks += 1 if hit else 0

        row = {
            "file": e["file"], "answer": answer, "checks": checks,
            "latency_s": round(latency, 2),
            "tokens": usage.get("total_tokens"),
        }
        if args.judge and e.get("reference"):
            try:
                row["judge"] = judge(e["reference"], answer, e["file"])
            except Exception as exc:  # noqa: BLE001
                row["judge"] = {"score": None, "reason": f"judge failed: {exc}"}
        rows.append(row)
        mark = "PASS" if all(c["pass"] for c in checks) else "FAIL"
        print(f"  {mark}  {e['file']:36s} {latency:5.1f}s  {answer[:70]!r}")

    rate = (passed_checks / total_checks) if total_checks else 0.0
    judged = [r["judge"]["score"] for r in rows if isinstance(r.get("judge"), dict) and r["judge"].get("score")]
    print(f"\n=== perception: {passed_checks}/{total_checks} checks ({rate:.0%}), "
          f"{len(rows)} images, model={args.model}"
          + (f", judge avg {sum(judged)/len(judged):.1f}/5" if judged else "") + " ===")

    if args.out:
        lines = [f"# Perception eval — {args.model}", "",
                 f"- endpoint: `{args.endpoint}`",
                 f"- checks: {passed_checks}/{total_checks} ({rate:.0%})",
                 f"- images: {len(rows)}", ""]
        for r in rows:
            lines.append(f"## {r['file']}")
            if r.get("error"):
                lines.append(f"- ERROR: {r['error']}")
                continue
            lines.append(f"- {r['latency_s']}s, {r.get('tokens')} tokens")
            for c in r["checks"]:
                lines.append(f"- [{'x' if c['pass'] else ' '}] {c['name']}")
            if r.get("judge"):
                lines.append(f"- judge: {r['judge'].get('score')}/5 — {r['judge'].get('reason')}")
            lines += ["", "```", r["answer"][:1500], "```", ""]
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text("\n".join(lines))
        print(f"report: {args.out}")

    return 0 if total_checks and passed_checks == total_checks else 1


if __name__ == "__main__":
    sys.exit(main())
