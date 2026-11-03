import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from run_eval import _build_turn_record


class SemanticCompletionTests(unittest.TestCase):
    def test_real_completion_propagates_in_stream_and_history_paths(self):
        for calls in [[], [{"name": "shell", "arguments": {}, "result": "{}"}]]:
            for complete in [True, False, None]:
                with self.subTest(calls=calls, complete=complete):
                    response = {"response": "answer", "tool_calls_stream": calls,
                                "generation_complete": complete}
                    self.assertIs(_build_turn_record("query", response, 1)["generation_complete"], complete)

    def test_missing_completion_is_not_promoted_to_true(self):
        for response in [{"response": "answer"}, {"response": "answer", "tool_calls_stream": []}]:
            self.assertIsNone(_build_turn_record("query", response, 1)["generation_complete"])


if __name__ == "__main__": unittest.main()
