import copy
import json
import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from lib.semantic_review import OpenAICompatibleJudge, citation_catalog, resolve_pointer, review_record


def record(answer="设备当前记录为离线。"):
    current = {"success": True, "data": {"device": {
        "id": "d1", "status": "offline", "online": False,
        "transport_connected": False, "last_seen": 2000, "transport_changed_at": None},
        "metrics": {"values/temp~x": {"value": 0}}}}
    return {"case_id": "test", "turn_records": [{"user": "设备现在什么状态？",
        "assistant_message": answer, "generation_complete": True, "tool_calls": [{
            "name": "shell", "arguments": {"command": "neomind device get d1"},
            "result": json.dumps({"exit_code": 0, "stdout": json.dumps(current)})}]}]}


def cite(payload, pointer):
    return {"ref": next(c["ref"] for c in payload["citation_catalog"] if c["pointer"] == pointer)}


def judgment(payload, verdict="supported"):
    return {"schema_version": 1, "complete": True, "notes": "reviewed every unit",
        "segments": [{"id": unit["id"], "kind": "assertion", "verdict": verdict,
            "quote": unit["text"], "reason": "Current platform state is scoped to this snapshot.",
            "evidence": [cite(payload, "/observations/0/fields/status")]}
            for unit in payload["units"]]}


def judge(payload):
    return {"content": json.dumps(judgment(payload)), "finish_reason": "stop"}


class SemanticReviewTests(unittest.TestCase):
    def test_supported_and_unsupported_are_separate_from_hard_grades(self):
        source = record()
        original = copy.deepcopy(source)
        self.assertEqual(review_record(source, judge)["verdict"], "PASS")
        self.assertEqual(source, original)
        result = review_record(record("设备从未连接。"), lambda p: {
            "content": json.dumps(judgment(p, "unsupported")), "finish_reason": "stop"})
        self.assertEqual(result["verdict"], "FAIL")
        self.assertIs(result["hard_grade_modified"], False)

    def test_bad_or_incomplete_assessments_never_pass(self):
        def missing_unit(j): j["segments"].pop()
        def extra_key(j): j["unexpected"] = True
        def wrong_quote(j): j["segments"][0]["quote"] = "invented answer words"
        def wrong_value(j): j["segments"][0]["evidence"][0]["value"] = "online"
        def unknown_pointer(j): j["segments"][0]["evidence"][0]["ref"] = "e999999"
        def missing_citation(j): j["segments"][0]["evidence"] = []
        def duplicate_unit(j): j["segments"][1]["id"] = "u0"
        def empty_claims(j):
            for s in j["segments"]: s.update(kind="non_factual", verdict="not_applicable", evidence=[])
        for mutate in [missing_unit, extra_key, wrong_quote, wrong_value, unknown_pointer,
                       missing_citation, duplicate_unit, empty_claims]:
            with self.subTest(mutate=mutate.__name__):
                def bad(p):
                    data = judgment(p); mutate(data)
                    return {"content": json.dumps(data), "finish_reason": "stop"}
                self.assertEqual(review_record(record("设备离线。\n原因尚未确定。"), bad)["verdict"], "ABSTAIN")
        for content in ['not JSON', '```json\n{}\n```', '{"complete":true,"complete":false}', 'NaN']:
            self.assertEqual(review_record(record(), lambda _: {"content": content, "finish_reason": "stop"})["verdict"], "ABSTAIN")

    def test_target_or_judge_truncation_and_missing_completion_abstain(self):
        for value in [None, False, 1]:
            item = record(); item["turn_records"][0]["generation_complete"] = value
            self.assertEqual(review_record(item, judge)["verdict"], "ABSTAIN")
        item = record(); item["turn_records"][0]["finish_reason"] = "length"
        self.assertEqual(review_record(item, judge)["verdict"], "ABSTAIN")
        self.assertEqual(review_record(record(), lambda p: {"content": json.dumps(judgment(p)), "finish_reason": "length"})["verdict"], "ABSTAIN")

    def test_scope_missing_evidence_and_input_budget_do_not_call_judge(self):
        def forbidden(_): self.fail("judge must not be called")
        item = record(); item["turn_records"][0]["tool_calls"].append({
            "name": "shell", "arguments": {"command": "neomind connector test c1"}, "result": "{}"})
        self.assertEqual(review_record(item, forbidden)["reason"], "evidence_scope_incomplete")
        self.assertEqual(review_record(record(), forbidden, max_input_chars=10)["reason"], "review_input_exceeds_limit")
        self.assertEqual(review_record(record(), None)["reason"], "judge_unconfigured")

    def test_uncertain_supported_reference_produces_abstention(self):
        result = review_record(record(), lambda p: {
            "content": json.dumps(judgment(p, "uncertain")), "finish_reason": "stop"})
        self.assertEqual(result["verdict"], "ABSTAIN")

    def test_judge_cannot_mutate_trusted_evidence(self):
        def corrupt(payload):
            payload["evidence_packet"]["observations"][0]["fields"]["status"] = "online"
            for entry in payload["citation_catalog"]:
                if entry["pointer"] == "/observations/0/fields/status":
                    entry["value"] = "online"
            return {"content": json.dumps(judgment(payload)), "finish_reason": "stop"}
        result = review_record(record(), corrupt)
        self.assertEqual(result["verdict"], "PASS")
        self.assertEqual(result["evidence_packet"]["observations"][0]["fields"]["status"], "offline")
        ref = result["assessment"]["segments"][0]["evidence"][0]
        self.assertEqual(ref["pointer"], "/observations/0/fields/status")
        self.assertEqual(ref["value"], "offline")

    def test_exact_zero_false_types_and_json_pointer_escaping(self):
        result = review_record(record(), judge)
        packet = result["evidence_packet"]
        self.assertEqual(resolve_pointer(packet, "/observations/0/current_metrics/values~1temp~0x/value"), 0)
        catalog = citation_catalog(packet)
        flag = next(c for c in catalog if c["pointer"] == "/observations/0/fields/online")
        self.assertIs(flag["value"], False)
        scalar = next(c for c in catalog if c["pointer"] == "/observations/0/current_metrics/values~1temp~0x/value")
        self.assertIs(type(scalar["value"]), int)
        self.assertEqual(scalar["value"], 0)
        def wrong_type(payload):
            data = judgment(payload)
            data["segments"][0]["evidence"] = [{"pointer": "/observations/0/fields/online", "value": 0}]
            return {"content": json.dumps(data), "finish_reason": "stop"}
        self.assertEqual(review_record(record(), wrong_type)["verdict"], "ABSTAIN")

    def test_timeout_and_credentials_in_endpoint_are_not_accepted(self):
        def timeout(_): raise TimeoutError("not persisted")
        self.assertEqual(review_record(record(), timeout)["reason"], "judge_unavailable")
        for url in ['http://user:secret@host/v1', 'http://host/v1?key=secret']:
            with self.assertRaises(ValueError): OpenAICompatibleJudge(url, "model")
        config = OpenAICompatibleJudge('http://localhost:3000', 'model', 'runtime-secret').configuration
        self.assertNotIn('runtime-secret', json.dumps(config))

    def test_known_measurement_failure_is_evidence_not_silent_zero(self):
        item = record("历史请求失败，尚不能确认窗口内是否有记录。")
        item["turn_records"][0]["tool_calls"].append({"name": "shell",
            "arguments": {"command": "neomind device history d1"},
            "result": json.dumps({"exit_code": 1, "stderr": "request failed", "stdout": ""})})
        def failure_review(payload):
            data = judgment(payload)
            data["segments"][0]["evidence"] = [cite(payload, "/source_errors/0/reason")]
            return {"content": json.dumps(data), "finish_reason": "stop"}
        self.assertEqual(review_record(item, failure_review)["verdict"], "PASS")
        packet = review_record(item, None)["evidence_packet"]
        self.assertEqual(packet["source_errors"][0]["kind"], "measurement_failed")
        self.assertEqual(packet["source_errors"][0]["outcome"]["exit_code"], 1)


if __name__ == "__main__": unittest.main()
