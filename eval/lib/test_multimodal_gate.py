"""Tests for run_eval._filter_multimodal — run: `python3 eval/lib/test_multimodal_gate.py`.

The 4 tools/vision cases carry `requires_multimodal: true` but the runner never
gated on it — a non-multimodal config (e.g. GLM-5.2 "假多模态") ran them, failed
at image-input validation, and inflated the fail count by 4. `--skip-multimodal`
excludes them for non-multimodal configs.
"""
from __future__ import annotations

import json
import os
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.dirname(__file__)))  # eval/
import run_eval  # noqa: E402


def _make_case(tmp: str, name: str, req_mm):
    p = os.path.join(tmp, f"{name}.json")
    with open(p, "w") as f:
        json.dump({"id": name, "requires_multimodal": req_mm}, f)
    return Path(p)  # _load_case does p.read_text() → needs a Path


def test_skip_filters_requires_multimodal():
    with tempfile.TemporaryDirectory() as tmp:
        cases = [_make_case(tmp, "a", False), _make_case(tmp, "b", True),
                 _make_case(tmp, "c", None)]  # absent field → eligible
        out = run_eval._filter_multimodal(cases, True)
    assert [os.path.basename(x) for x in out] == ["a.json", "c.json"], out


def test_keep_all_when_not_skip():
    with tempfile.TemporaryDirectory() as tmp:
        cases = [_make_case(tmp, "b", True), _make_case(tmp, "a", False)]
        out = run_eval._filter_multimodal(cases, False)
    assert len(out) == 2, out


def _run_all():
    fns = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    failed = 0
    for fn in fns:
        try:
            fn()
            print(f"  PASS  {fn.__name__}")
        except AssertionError as e:
            failed += 1
            print(f"  FAIL  {fn.__name__}: {e!r}")
        except Exception as e:  # noqa: BLE001
            failed += 1
            print(f"  ERROR {fn.__name__}: {e!r}")
    print(f"\n{len(fns) - failed}/{len(fns)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(_run_all())
