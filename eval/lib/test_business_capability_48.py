"""Offline freeze checks for Plan F business-capability 48 catalog."""
from __future__ import annotations

import json
import unittest
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CATALOG = ROOT / "eval" / "acceptance" / "business-capability-48.json"

EXPECTED_IDS = (
    [f"DEV{i:02d}" for i in range(1, 9)]
    + [f"TR{i:02d}" for i in range(1, 9)]
    + [f"RULE{i:02d}" for i in range(1, 9)]
    + [f"AG{i:02d}" for i in range(1, 7)]
    + [f"AUX{i:02d}" for i in range(1, 7)]
    + [f"E2E{i:02d}" for i in range(1, 13)]
)


class BusinessCapability48Freeze(unittest.TestCase):
    def setUp(self) -> None:
        self.doc = json.loads(CATALOG.read_text(encoding="utf-8"))
        self.scenarios = self.doc["scenarios"]

    def test_exact_48_unique_plan_ids(self) -> None:
        ids = [s["id"] for s in self.scenarios]
        self.assertEqual(len(ids), 48)
        self.assertEqual(len(set(ids)), 48)
        self.assertEqual(set(ids), set(EXPECTED_IDS))
        # Catalog order follows plan families (DEV→TR→RULE→AG→AUX→E2E).
        self.assertEqual(ids, EXPECTED_IDS)

    def test_split_counts_24_12_12(self) -> None:
        splits = Counter(s["split"] for s in self.scenarios)
        self.assertEqual(splits["development"], 24)
        self.assertEqual(splits["daily_regression"], 12)
        self.assertEqual(splits["independent_holdout"], 12)
        lists = self.doc["split_lists"]
        self.assertEqual(len(lists["development"]), 24)
        self.assertEqual(len(lists["daily_regression"]), 12)
        self.assertEqual(len(lists["independent_holdout"]), 12)
        # lists must partition the catalog
        all_listed = (
            lists["development"]
            + lists["daily_regression"]
            + lists["independent_holdout"]
        )
        self.assertEqual(set(all_listed), set(EXPECTED_IDS))
        for s in self.scenarios:
            self.assertIn(s["id"], lists[s["split"]])

    def test_family_counts(self) -> None:
        fam = Counter(s["family"] for s in self.scenarios)
        self.assertEqual(fam["device"], 8)
        self.assertEqual(fam["transform"], 8)
        self.assertEqual(fam["rule"], 8)
        self.assertEqual(fam["agent"], 6)
        self.assertEqual(fam["aux"], 6)
        self.assertEqual(fam["e2e"], 12)

    def test_holdout_has_no_prompt_leak_and_is_flagged(self) -> None:
        for s in self.scenarios:
            if s["split"] != "independent_holdout":
                continue
            self.assertEqual(
                s.get("prompt_status"),
                "maintainer_only",
                f"{s['id']} holdout missing maintainer_only",
            )
            self.assertNotIn("prompt", s)
            self.assertNotIn("turns", s)

    def test_e2e01_links_plan_e_case(self) -> None:
        e2e01 = next(s for s in self.scenarios if s["id"] == "E2E01")
        self.assertEqual(e2e01["case_status"], "case_ready")
        self.assertIn("main-objective-e2e-detection-chain", e2e01["provisional_case_ids"])
        ref = ROOT / e2e01["acceptance_ref"]
        self.assertTrue(ref.is_file())

    def test_provisional_case_ids_exist_when_mapped(self) -> None:
        case_root = ROOT / "eval" / "cases"
        known = {p.stem for p in case_root.rglob("*.json")}
        # Also allow ids that differ from filename stem — resolve via id field.
        by_id: set[str] = set()
        for p in case_root.rglob("*.json"):
            try:
                by_id.add(json.loads(p.read_text(encoding="utf-8")).get("id") or "")
            except Exception:
                continue
        known |= {x for x in by_id if x}
        missing: list[str] = []
        for s in self.scenarios:
            if s["case_status"] == "stub":
                continue
            for cid in s.get("provisional_case_ids") or []:
                if cid not in known:
                    missing.append(f"{s['id']}→{cid}")
        self.assertEqual(missing, [], f"unknown provisional case ids: {missing}")

    def test_pairing_protocol_present_and_138_untouched(self) -> None:
        p = self.doc["pairing_protocol"]
        self.assertEqual(p["arms"], ["baseline", "candidate"])
        self.assertTrue(any("Interleave" in c or "交错" in c or "Identical" in c for c in p["constraints"]))
        self.assertFalse(self.doc["contention"]["eval_138"]["touched"])
        self.assertIn("Live paired runs on tip", p["deferred"])


if __name__ == "__main__":
    unittest.main()
