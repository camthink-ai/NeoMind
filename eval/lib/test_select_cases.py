"""Tests for run_eval._select_cases — run: `.venv/bin/python eval/lib/test_select_cases.py`.

The zh/en case sets are mirrors (identical ids). Selection must apply EVERY
filter: `--lang zh --case-id X` must not run the en mirror of X (it did:
case_id was in an if/else, bypassing lang/workflow).
"""
from __future__ import annotations

import json
import os
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.dirname(__file__)))  # eval/
import run_eval  # noqa: E402


def _make(root: Path, lang: str, wf: str, cid: str):
    d = root / lang / wf
    d.mkdir(parents=True, exist_ok=True)
    (d / f"{cid}.json").write_text(json.dumps({"id": cid, "lang": lang}))


def _root(tmp: str) -> Path:
    root = Path(tmp) / "cases"
    _make(root, "zh", "wf-a", "shared")
    _make(root, "en", "wf-a", "shared")
    _make(root, "zh", "wf-b", "other")
    return root


def test_case_id_respects_lang():
    with tempfile.TemporaryDirectory() as tmp:
        root = _root(tmp)
        got = run_eval._select_cases(root, "zh", None, "shared")
        assert len(got) == 1, [str(p) for p in got]
        assert "zh" in got[0].parts, got


def test_both_langs_selects_both_mirrors():
    with tempfile.TemporaryDirectory() as tmp:
        root = _root(tmp)
        got = run_eval._select_cases(root, "both", None, "shared")
        assert len(got) == 2, [str(p) for p in got]


def test_workflow_filter_applies_with_case_id():
    with tempfile.TemporaryDirectory() as tmp:
        root = _root(tmp)
        assert run_eval._select_cases(root, "zh", ["wf-a"], "shared")
        assert not run_eval._select_cases(root, "zh", ["wf-b"], "shared")


def test_no_filters_returns_everything():
    with tempfile.TemporaryDirectory() as tmp:
        root = _root(tmp)
        assert len(run_eval._select_cases(root, "both", None, None)) == 3


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
