"""Regression gate keeps evidence for every execution, including failures.

Run: python3 eval/lib/test_regression_traces.py
No server or LLM is started.
"""
from __future__ import annotations

import argparse
import contextlib
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import run_eval  # noqa: E402


class RegressionTraceTests(unittest.TestCase):
    def fixture(self, directory: str, rounds: int):
        root = Path(directory)
        case = {
            "id": "list-devices", "lang": "zh",
            "expect": {"tools": ["shell"], "commands": ["neomind device list"]},
        }
        path = root / "case.json"
        path.write_text(json.dumps(case))
        case_set = root / "set.txt"
        case_set.write_text("list-devices\n")
        run_dir = root / "run"
        run_dir.mkdir()
        args = argparse.Namespace(
            skip_preflight=True, root=str(root), regression_set=str(case_set),
            rounds=rounds, baseline=str(root / "baseline.jsonl"),
            run_dir=str(run_dir), update_baseline=True, case_timeout=30,
            lang="zh", skip_multimodal=False, allow_model_mismatch=False,
        )
        record = {
            "case_id": case["id"], "lang": "zh", "status": None,
            "turn_records": [{
                "user": "列出设备", "assistant_message": "共有三台设备。",
                "processing_time_ms": 123,
                "tool_calls": [{
                    "name": "shell", "arguments": {"command": "neomind device list"},
                    "result": {"exit_code": 0, "stdout": "three devices"},
                }],
            }],
            "state_queries": [{"type": "device_count", "passed": True}],
            "guards": {"events": []},
        }
        return args, case, path, run_dir, record

    @staticmethod
    def rows(path: Path):
        return [json.loads(line) for line in path.read_text().splitlines()]

    def test_success_timeout_and_error_keep_distinct_rounds_without_score_changes(self):
        with tempfile.TemporaryDirectory() as tmp:
            args, case, path, run_dir, success = self.fixture(tmp, rounds=3)
            error = run_eval._error_record(case, "llm_config_error", "endpoint unavailable")
            # Regression's existing contract is a fresh output, not resume.
            for name in ("cases.jsonl", "scores.jsonl"):
                (run_dir / name).write_text('{"stale":true}\n')
            compute = run_eval.hard_signal.compute
            observed = []

            def score_after_trace(case_input, record):
                traces = self.rows(run_dir / "cases.jsonl")
                trace = traces[-1]
                self.assertEqual(trace["round"], len(observed))
                self.assertEqual(trace["turn_records"], record["turn_records"])
                observed.append(record)
                return compute(case_input, record)

            with (
                patch.object(run_eval, "_check_model_identity", return_value=0),
                patch.object(run_eval, "_select_cases", return_value=[path]),
                patch.object(run_eval, "_filter_multimodal", side_effect=lambda paths, _: paths),
                patch.object(run_eval, "_filter_network", side_effect=lambda paths: paths),
                patch.object(run_eval, "run_case", side_effect=[success, run_eval._CaseTimeout(), error]),
                patch.object(run_eval.hard_signal, "compute", side_effect=score_after_trace),
                contextlib.redirect_stdout(io.StringIO()),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                self.assertEqual(run_eval.cmd_regression(args), 0)

            traces = self.rows(run_dir / "cases.jsonl")
            scores = self.rows(run_dir / "scores.jsonl")
            self.assertEqual([r["round"] for r in traces], [0, 1, 2])
            self.assertEqual([r["case_id"] for r in traces], ["list-devices"] * 3)
            self.assertEqual([r["lang"] for r in traces], ["zh"] * 3)
            self.assertEqual([r["status"] for r in traces], [None, "timeout", "llm_config_error"])
            self.assertEqual(traces[0]["turn_records"][0]["assistant_message"], "共有三台设备。")
            self.assertNotIn("round", success, "trace metadata must not mutate scoring input")
            for score, record in zip(scores, observed):
                self.assertEqual(score["hard"], compute(case, record))
            self.assertEqual(
                [run_eval.hard_signal.pass_for_case_product(r["hard"]) for r in scores],
                [True, False, False],
            )
            self.assertEqual((Path(tmp) / "baseline.jsonl").read_text(),
                             (run_dir / "scores.jsonl").read_text())

    def test_trace_survives_a_scoring_exception(self):
        with tempfile.TemporaryDirectory() as tmp:
            args, _, path, run_dir, success = self.fixture(tmp, rounds=1)
            with (
                patch.object(run_eval, "_check_model_identity", return_value=0),
                patch.object(run_eval, "_select_cases", return_value=[path]),
                patch.object(run_eval, "_filter_multimodal", side_effect=lambda paths, _: paths),
                patch.object(run_eval, "_filter_network", side_effect=lambda paths: paths),
                patch.object(run_eval, "run_case", return_value=success),
                patch.object(run_eval.hard_signal, "compute", side_effect=RuntimeError("scoring failed")),
            ):
                with self.assertRaisesRegex(RuntimeError, "scoring failed"):
                    run_eval.cmd_regression(args)
            traces = self.rows(run_dir / "cases.jsonl")
            self.assertEqual(len(traces), 1)
            self.assertEqual(traces[0]["turn_records"], success["turn_records"])
            self.assertEqual((run_dir / "scores.jsonl").read_text(), "")


if __name__ == "__main__":
    unittest.main()
