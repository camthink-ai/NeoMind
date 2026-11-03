import copy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from compare_refinement import compare
from lib.answer_refinement import digest
from lib.semantic_evidence import compile_evidence
from lib.test_answer_refinement import source


def review(record, verdict):
    return {"verdict": verdict, "answer_sha256": digest(record["turn_records"][-1]["assistant_message"]),
            "evidence_sha256": digest(compile_evidence(record)), "case_id": record.get("case_id"),
            "target_turn": len(record["turn_records"]) - 1, "rubric_sha256": "same"}


def attempt():
    before = source(); after = copy.deepcopy(before)
    after["turn_records"][-1]["assistant_message"] = "现在离线，历史连接未核实。"
    return {"baseline": before, "candidate": after, "status": "produced", "reason": "needs_review"}


class ComparisonTests(unittest.TestCase):
    def test_semantic_pass_cannot_override_delivery_failure(self):
        a = attempt(); a["candidate"]["turn_records"][-1]["assistant_message"] = "The device name is"
        r = compare([a], [review(a["baseline"], "FAIL")], [review(a["candidate"], "PASS")])
        self.assertFalse(r["screening_promising"])
        self.assertEqual(r["delivery_failures"], 1)

    def test_gain_is_screening_only_and_skips_are_not_successes(self):
        a = attempt(); b = attempt(); b.update(status="skipped", reason="out_of_scope", candidate=b["baseline"])
        r = compare([a, b], [review(a["baseline"], "FAIL"), review(b["baseline"], "ABSTAIN")],
                    [review(a["candidate"], "PASS"), review(b["candidate"], "ABSTAIN")])
        self.assertEqual(r["gained_passes"], 1)
        self.assertEqual(r["eligible_pairs"], 1)
        self.assertTrue(r["screening_promising"])
        self.assertFalse(r["production_acceptance"])

    def test_regression_abstention_and_refinement_failure_block_acceptance(self):
        for before, after, status in [("PASS", "FAIL", "produced"), ("FAIL", "ABSTAIN", "produced"),
                                       ("FAIL", "PASS", "failed")]:
            a = attempt(); a["status"] = status
            r = compare([a], [review(a["baseline"], before)], [review(a["candidate"], after)])
            self.assertFalse(r["screening_promising"])

    def test_swapped_answer_or_execution_changes_are_rejected(self):
        a = attempt(); old = review(a["baseline"], "FAIL"); new = review(a["candidate"], "PASS")
        broken = copy.deepcopy(new); broken["answer_sha256"] = "other"
        with self.assertRaises(ValueError): compare([a], [old], [broken])
        broken = copy.deepcopy(new); broken["evidence_sha256"] = "other"
        with self.assertRaises(ValueError): compare([a], [old], [broken])
        a["candidate"]["turn_records"][-1]["tool_calls"].clear()
        with self.assertRaises(ValueError): compare([a], [old], [new])


if __name__ == "__main__": unittest.main()
