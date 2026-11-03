import copy
import json
import io
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from lib.answer_refinement import delivery_issues, refine_once
from lib.test_semantic_review import record
from refine_answers import LocalSparkGenerator


def source():
    item = record("设备从未连接。")
    item["turn_records"][0]["processing_time_ms"] = 1000
    return item


def good(payload, timeout):
    ref = next(c["ref"] for c in payload["citation_catalog"] if c["pointer"] == "/observations/0/fields/status")
    return {"finish_reason": "stop", "content": json.dumps({"claims": [
        {"text": "当前记录为离线", "refs": [ref]}], "answer": "当前记录为离线；此前连接情况及根因尚不确定。"})}


class RefinementTests(unittest.TestCase):
    def test_delivery_checks_do_not_accept_control_tags_wrong_language_or_fragments(self):
        self.assertIn("leaked_control_markup", delivery_issues("设备离线。</tool_call>", "设备状态？"))
        self.assertIn("requested_language_not_preserved", delivery_issues("The device is offline.", "请检查设备是否离线。"))
        self.assertIn("possible_unfinished_sentence", delivery_issues("The device name is", "Check this device."))
        self.assertEqual(delivery_issues("设备当前离线，根因未知。", "请检查设备是否离线。"), [])
        self.assertEqual(delivery_issues("The device is offline.", "Check whether the device named 客厅温湿度传感器 is online."), [])

    def test_failed_delivery_preserves_draft_even_with_valid_json(self):
        def bad(payload, timeout):
            response = good(payload, timeout)
            value = json.loads(response["content"]); value["answer"] = "The device name is"
            response["content"] = json.dumps(value)
            return response
        item = source(); result = refine_once(item, bad)
        self.assertEqual(result["reason"], "delivery_checks_failed")
        self.assertEqual(result["candidate"], item)

    def test_one_call_preserves_original_tools_and_grades(self):
        item = source(); before = copy.deepcopy(item); calls = []
        def generate(p, timeout):
            calls.append(timeout)
            return good(p, timeout)
        result = refine_once(item, generate)
        self.assertEqual(item, before)
        self.assertEqual(result["baseline"], before)
        self.assertEqual(result["status"], "produced")
        self.assertEqual(len(calls), 1)
        old = before["turn_records"][0]; new = result["candidate"]["turn_records"][0]
        for key in old.keys() - {"assistant_message", "processing_time_ms"}:
            self.assertEqual(new[key], old[key])
        self.assertEqual(result["reason"], "candidate_requires_independent_review")

    def test_failures_never_retry_or_change_draft(self):
        for response in [{"finish_reason": "length", "content": "partial"},
                         {"finish_reason": "stop", "content": "not JSON"},
                         {"finish_reason": "stop", "content": '{"claims":[],"answer":"all okay"}'},
                         {"finish_reason": "stop", "content": '{"claims":[{"text":"x","refs":["made-up"]}],"answer":"x"}'}]:
            calls = []
            def generate(p, timeout):
                calls.append(1); return response
            item = source(); result = refine_once(item, generate)
            self.assertEqual(result["status"], "failed")
            self.assertEqual(result["candidate"], item)
            self.assertEqual(len(calls), 1)
        calls = []
        def unavailable(p, timeout):
            calls.append(1); raise TimeoutError()
        self.assertEqual(refine_once(source(), unavailable)["reason"], "generation_unavailable")
        self.assertEqual(len(calls), 1)

    def test_budget_caps_actual_call_and_discards_late_result(self):
        calls = []
        def generate(p, timeout):
            calls.append(timeout); return good(p, timeout)
        with patch("lib.answer_refinement.time.monotonic", side_effect=[10, 12]):
            result = refine_once(source(), generate, total_budget_ms=2000)
        self.assertEqual(calls, [1])
        self.assertEqual(result["reason"], "refinement_budget_exceeded")
        self.assertEqual(result["candidate"], result["baseline"])

    def test_unsupported_or_incomplete_inputs_do_not_generate(self):
        def forbidden(*args): self.fail("must skip without a generation")
        variants = []
        item = source(); item["turn_records"][0]["generation_complete"] = False; variants.append(item)
        item = source(); item["turn_records"][0]["finish_reason"] = "length"; variants.append(item)
        item = source(); item["turn_records"][0].pop("processing_time_ms"); variants.append(item)
        item = source(); item["turn_records"][0]["processing_time_ms"] = 240000; variants.append(item)
        item = source(); item["turn_records"][0]["tool_calls"][0]["arguments"]["command"] = "neomind device update d1 --name new"; variants.append(item)
        for item in variants:
            result = refine_once(item, forbidden)
            self.assertEqual(result["status"], "skipped")
            self.assertEqual(result["calls"], 0)
            self.assertEqual(result["candidate"], item)

    def test_generator_cannot_mutate_record_or_catalog(self):
        def mutate(payload, timeout):
            result = good(payload, timeout)
            payload["evidence_packet"]["observations"][0]["entity_id"] = "other"
            payload["citation_catalog"].clear()
            return result
        item = source(); before = copy.deepcopy(item)
        self.assertEqual(refine_once(item, mutate)["status"], "produced")
        self.assertEqual(item, before)

    def test_missing_complete_or_duplicate_json_is_rejected(self):
        def bad(p, timeout):
            result = good(p, timeout)
            result["content"] = result["content"][:-1] + ',"answer":"invented"}'
            return result
        self.assertEqual(refine_once(source(), bad)["reason"], "invalid_revision")

    def test_model_transport_is_explicit_loopback_without_credentials(self):
        for url in ["http://user:secret@localhost:18080", "http://192.168.93.3:3000",
                    "http://localhost:18080?key=secret", "http://localhost:18080/other"]:
            with self.assertRaises(ValueError): LocalSparkGenerator(url, "spark")
        generator = LocalSparkGenerator("http://127.0.0.1:18080", "spark")
        self.assertEqual(generator.configuration["temperature"], .3)

    def test_schema_and_non_thinking_are_sent_and_reasoning_is_not_persisted(self):
        raw = {"choices": [{"finish_reason": "stop", "message": {
            "content": '{"claims":[],"answer":"reply"}', "reasoning_content": "private-analysis"}}]}
        generator = LocalSparkGenerator("http://127.0.0.1:18080", "spark", thinking="off")
        with patch("refine_answers.urllib.request.build_opener") as build:
            build.return_value.open.return_value.__enter__.return_value = io.BytesIO(json.dumps(raw).encode())
            response = generator({"citation_catalog": [{"ref": "e0"}, {"ref": "e1"}]}, 10)
            request = build.return_value.open.call_args.args[0]
        body = json.loads(request.data)
        self.assertEqual(body["chat_template_kwargs"], {"enable_thinking": False})
        schema = body["response_format"]["json_schema"]["schema"]
        self.assertEqual(schema["properties"]["claims"]["items"]["properties"]["refs"]["items"]["enum"], ["e0", "e1"])
        self.assertEqual(schema["properties"]["claims"]["maxItems"], 3)
        self.assertEqual(response["reasoning_chars"], len("private-analysis"))
        self.assertNotIn("private-analysis", json.dumps(response))


if __name__ == "__main__": unittest.main()
