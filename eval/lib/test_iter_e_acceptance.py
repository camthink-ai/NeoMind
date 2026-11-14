"""Offline checks for Plan E preregistered acceptance (no LLM / no llama)."""
from __future__ import annotations

import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ACCEPTANCE = ROOT / "eval" / "acceptance" / "iter-e-detection-chain.json"


class IterEAcceptancePreregister(unittest.TestCase):
    def setUp(self) -> None:
        self.doc = json.loads(ACCEPTANCE.read_text(encoding="utf-8"))

    def test_ten_trials_five_zh_five_en(self) -> None:
        trials = self.doc["trials"]
        self.assertEqual(len(trials), 10)
        langs = [t["lang"] for t in trials]
        self.assertEqual(langs.count("zh"), 5)
        self.assertEqual(langs.count("en"), 5)
        self.assertEqual(self.doc["pass_rule"]["min_full_success"], 9)
        seeds = [t["seed"] for t in trials]
        self.assertEqual(len(seeds), len(set(seeds)), "trial seeds must be unique")

    def test_case_files_exist_and_share_id(self) -> None:
        paths = self.doc["scenario"]["case_paths"]
        ids = set()
        for lang, rel in paths.items():
            p = ROOT / rel
            self.assertTrue(p.is_file(), f"missing {rel}")
            case = json.loads(p.read_text(encoding="utf-8"))
            self.assertEqual(case["lang"], lang)
            self.assertEqual(case["id"], self.doc["scenario"]["id"])
            ids.add(case["id"])
            # Hardened contract: ambient rule must stay
            types = {q["type"] for q in case["state_queries"]}
            self.assertIn("rule_exists", types)
            self.assertIn("transform_count", types)
            self.assertIn("rule_count", types)
            battery = next(
                q for q in case["state_queries"] if q["type"] == "rule_exists"
            )
            self.assertEqual(battery["params"]["name"], "rule-battery-low")
            self.assertEqual(battery["expected"], True)
            rule_min = next(
                q for q in case["state_queries"] if q["type"] == "rule_count"
            )
            self.assertGreaterEqual(rule_min["expected_min"], 2)
        self.assertEqual(ids, {"main-objective-e2e-detection-chain"})

    def test_sampling_frozen_and_138_not_shared(self) -> None:
        s = self.doc["frozen_sampling"]
        self.assertEqual(s["temperature"], 0.3)
        self.assertEqual(s["top_p"], 0.75)
        self.assertEqual(s["top_k"], 20)
        self.assertTrue(s["thinking"])
        c = self.doc["contention"]["eval_138"]
        self.assertFalse(c["touched"])
        self.assertEqual(c["llama_port"], 29431)


if __name__ == "__main__":
    unittest.main()
