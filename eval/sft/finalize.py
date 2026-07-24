"""Finalize the golden dataset: merge zh+en, validate every example renders
through the MiniCPM5 template, emit two trainable files + stats + README.

Outputs (under data/sft/):
  golden.jsonl          — ShareGPT/OpenAI format {messages, tools, ...}.
                          Primary training file: a framework with the MiniCPM5
                          tokenizer applies the chat template + masks
                          non-assistant tokens. (recommended for SFT)
  golden.minicpm.jsonl  — pre-rendered {"text": <ChatML>} via the vendored
                          template, for raw-text training / inspection.
  README.md             — dataset card + how to train.
"""
from __future__ import annotations
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SFT = ROOT / "data" / "sft"
sys_path = ROOT
import sys; sys.path.insert(0, str(sys_path))
from eval.sft.render_minicpm import render_minicpm_text  # noqa: E402

records = []
for lang in ("zh", "en"):
    for line in (SFT / f"golden-{lang}.jsonl").read_text().splitlines():
        if not line.strip():
            continue
        r = json.loads(line)
        if not r.get("messages"):
            continue
        r["lang"] = lang
        records.append(r)

# Render-validate every example through the real MiniCPM5 template.
errors = []
total_chars = 0
rendered = []
for r in records:
    try:
        text = render_minicpm_text(r["messages"], r.get("tools"))
    except Exception as e:  # noqa: BLE001
        errors.append((r.get("case_id"), str(e)))
        continue
    total_chars += len(text)
    rendered.append({"text": text, "case_id": r.get("case_id"), "lang": r["lang"]})

if errors:
    print(f"!! {len(errors)} examples failed to render:")
    for cid, e in errors[:10]:
        print(f"   {cid}: {e}")
    raise SystemExit(1)

# Primary trainable file (framework-applied template).
(SFT / "golden.jsonl").write_text(
    "\n".join(json.dumps(r, ensure_ascii=False) for r in records) + "\n")
# Pre-rendered MiniCPM ChatML text.
(SFT / "golden.minicpm.jsonl").write_text(
    "\n".join(json.dumps(r, ensure_ascii=False) for r in rendered) + "\n")

# Stats
def tcount(recs, key):
    from collections import Counter
    c = Counter()
    for r in recs:
        for m in r["messages"]:
            for tc in (m.get("tool_calls") or []):
                c[tc.get("function", tc).get("name")] += 1
    return dict(c)

n_zh = sum(1 for r in records if r["lang"] == "zh")
n_en = sum(1 for r in records if r["lang"] == "en")
avg_chars = total_chars // len(records)
est_tokens = total_chars / 3.5  # rough mixed zh/en estimate
domains = sorted({r["case_id"].split("-")[0] for r in records})
print(f"merged: {len(records)} golden (zh {n_zh} / en {n_en})")
print(f"render-valid: {len(rendered)}/{len(records)} OK (0 errors)")
print(f"size: {total_chars:,} chars  ~{est_tokens:,.0f} tokens (est)  avg {avg_chars:,} chars/example")
print(f"tool call distribution: {tcount(records, None)}")
print(f"wrote: data/sft/golden.jsonl + data/sft/golden.minicpm.jsonl")
