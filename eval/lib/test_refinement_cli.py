import contextlib
import copy
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import evaluate_refinement as cli
from lib.test_answer_refinement import source


class RefinementCliTests(unittest.TestCase):
    def test_delivery_rejection_does_not_read_key_or_call_paid_judge(self):
        item = source(); candidate = copy.deepcopy(item)
        candidate["turn_records"][-1]["assistant_message"] = "The device name is"
        attempt = {"status": "produced", "reason": "needs_review", "baseline": item, "candidate": candidate}
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); path = root / "attempts.jsonl"; path.write_text(json.dumps(attempt) + "\n")
            with patch.object(cli.getpass, "getpass", side_effect=AssertionError("must not request a key")), patch.object(cli, "OpenAICompatibleJudge", side_effect=AssertionError("must not call a judge")), contextlib.redirect_stdout(io.StringIO()):
                code = cli.main(["--attempts", str(path), "--calibration-dir", str(root / "unused"),
                                 "--run-dir", str(root / "review"), "--judge-url", "http://unused",
                                 "--judge-model", "unused", "--ask-key"])
            self.assertEqual(code, 2)
            result = json.loads((root / "review/summary.json").read_text())
            self.assertEqual(result["model_judge_calls"], 0)
            self.assertFalse(result["screening_promising"])


if __name__ == "__main__": unittest.main()
