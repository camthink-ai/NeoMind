"""Tests for run_eval._resume_cases — run: `python3 eval/lib/test_resume.py`.

`run --resume <dir>` must skip cases already scored so a killed long eval can
restart without redoing completed cases. The zh/en sets are mirrors (identical
case ids), so the skip must key on (lang, id) — a done en case must NOT skip its
zh twin.
"""
from __future__ import annotations

import json
import os
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.dirname(__file__)))  # eval/
import run_eval  # noqa: E402


def _make(tmp: str, fname: str, cid: str, lang: str = "zh"):
    p = Path(tmp) / f"{fname}.json"
    p.write_text(json.dumps({"id": cid, "lang": lang}))
    return p


def test_resume_filters_done_by_lang_id():
    with tempfile.TemporaryDirectory() as tmp:
        cases = [_make(tmp, "a", "a", "zh"), _make(tmp, "b", "b", "zh"),
                 _make(tmp, "c", "c", "zh")]
        out = run_eval._resume_cases(cases, {("zh", "b")})
    assert [Path(x).stem for x in out] == ["a", "c"], out


def test_resume_mirror_zh_done_does_not_skip_en():
    # zh and en share case ids (mirrored sets). A done zh case must NOT skip
    # its en twin — the resume bug that silently dropped the whole en set.
    with tempfile.TemporaryDirectory() as tmp:
        zh = _make(tmp, "zh-a", "a", "zh")
        en = _make(tmp, "en-a", "a", "en")
        out = run_eval._resume_cases([zh, en], {("zh", "a")})
    assert [Path(x).stem for x in out] == ["en-a"], out


def test_resume_empty_done_keeps_all():
    with tempfile.TemporaryDirectory() as tmp:
        cases = [_make(tmp, "a", "a"), _make(tmp, "b", "b")]
        assert len(run_eval._resume_cases(cases, set())) == 2


def test_resume_all_done_returns_none():
    with tempfile.TemporaryDirectory() as tmp:
        cases = [_make(tmp, "a", "a", "zh")]
        assert run_eval._resume_cases(cases, {("zh", "a")}) == []


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
