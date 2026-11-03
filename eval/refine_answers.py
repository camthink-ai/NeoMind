#!/usr/bin/env python3
"""Offline paired answer refinement. Does not edit source records or production."""
from __future__ import annotations

import argparse
import collections
import hashlib
import ipaddress
import json
from pathlib import Path
import urllib.parse
import urllib.request

from lib.answer_refinement import REFINEMENT_PROMPT, canonical, refine_once


class LocalSparkGenerator:
    def __init__(self, endpoint, model, max_tokens=3072, thinking="default", structured_output="schema"):
        parsed = urllib.parse.urlsplit(endpoint)
        try:
            local = parsed.hostname == "localhost" or ipaddress.ip_address(parsed.hostname).is_loopback
        except (ValueError, TypeError):
            local = False
        if (not local or parsed.scheme != "http" or parsed.username or parsed.password
            or parsed.query or parsed.fragment or parsed.path not in ("", "/", "/v1", "/v1/")):
            raise ValueError("explicit_loopback_model_endpoint_required")
        if not isinstance(model, str) or not model.strip() or type(max_tokens) is not int or max_tokens <= 0:
            raise ValueError("invalid_model_configuration")
        self.endpoint, self.model, self.max_tokens = endpoint.rstrip("/"), model, max_tokens
        if thinking not in {"default", "off"}:
            raise ValueError("invalid_thinking_mode")
        self.thinking = thinking
        if structured_output not in {"prompt", "schema"}:
            raise ValueError("invalid_structured_output_mode")
        self.structured_output = structured_output
        self.url = self.endpoint + ("/chat/completions" if self.endpoint.endswith("/v1") else "/v1/chat/completions")

    @property
    def configuration(self):
        return {"endpoint": self.endpoint, "model": self.model, "max_tokens": self.max_tokens,
                "temperature": .3, "top_p": .75, "top_k": 20, "stream": False,
                "thinking_mode": self.thinking, "structured_output": self.structured_output}

    def __call__(self, payload, timeout):
        body = {k: v for k, v in self.configuration.items() if k not in {"endpoint", "thinking_mode", "structured_output"}}
        if self.thinking == "off":
            body["chat_template_kwargs"] = {"enable_thinking": False}
        if self.structured_output == "schema":
            claim = {"type": "object", "additionalProperties": False, "required": ["text", "refs"],
                     "properties": {"text": {"type": "string", "minLength": 1},
                                    "refs": {"type": "array", "minItems": 1, "maxItems": 3,
                                             "items": {"type": "string", "enum": [c["ref"] for c in payload["citation_catalog"]]}}}}
            schema = {"type": "object", "additionalProperties": False, "required": ["claims", "answer"],
                      "properties": {"claims": {"type": "array", "minItems": 1, "maxItems": 3, "items": claim},
                                     "answer": {"type": "string", "minLength": 1}}}
            body["response_format"] = {"type": "json_schema", "json_schema": {
                "name": "device_grounded_revision", "strict": True, "schema": schema}}
        body["messages"] = [{"role": "system", "content": REFINEMENT_PROMPT},
                            {"role": "user", "content": canonical(payload)}]
        request = urllib.request.Request(self.url, data=json.dumps(body).encode(),
                                         headers={"Content-Type": "application/json"})
        class NoRedirect(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, req, fp, code, msg, headers, newurl):
                return None
        with urllib.request.build_opener(NoRedirect).open(request, timeout=timeout) as response:
            raw = json.load(response)
        choice = raw["choices"][0]
        return {"content": choice["message"].get("content"),
                "finish_reason": choice.get("finish_reason"), "usage": raw.get("usage"),
                "reasoning_chars": len(choice["message"].get("reasoning_content") or ""),
                "model": raw.get("model"), "id": raw.get("id")}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--records", type=Path, required=True)
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--max-tokens", type=int, default=3072)
    parser.add_argument("--thinking", choices=["default", "off"], default="default")
    parser.add_argument("--structured-output", choices=["prompt", "schema"], default="schema")
    parser.add_argument("--timeout", type=float, default=60)
    parser.add_argument("--budget-ms", type=int, default=240000)
    args = parser.parse_args(argv)
    if not 0 < args.timeout <= 300 or args.budget_ms <= 0:
        parser.error("positive budget and timeout <=300 seconds required")
    generator = LocalSparkGenerator(args.endpoint, args.model, args.max_tokens, args.thinking, args.structured_output)
    records = [json.loads(line) for line in args.records.read_text().splitlines() if line.strip()]
    args.run_dir.mkdir(parents=True, exist_ok=False)
    source = Path(__file__).parent
    paths = [Path(__file__), source / "lib/answer_refinement.py",
             source / "lib/semantic_evidence.py", source / "lib/semantic_review.py"]
    manifest = {"schema_version": 1, "input_sha256": hashlib.sha256(args.records.read_bytes()).hexdigest(),
                "input_path": str(args.records.resolve()), "generator": generator.configuration,
                "total_processing_budget_ms": args.budget_ms, "refinement_timeout_seconds": args.timeout,
                "source_sha256": {str(p.relative_to(source)): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths},
                "method": "one attempted rewrite per eligible complete record; no judge feedback or best-of selection",
                "limitation": "Offline final-answer test; tool/state results inherited unchanged. Processing budget excludes unrecorded overhead.",
                "production_changed": False, "hard_grade_changed": False}
    (args.run_dir / "manifest.json").write_text(json.dumps(manifest, indent=2))
    for p in paths:
        dest = args.run_dir / "implementation" / p.relative_to(source)
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(p.read_bytes())
    statuses, reasons, calls = collections.Counter(), collections.Counter(), 0
    with (args.run_dir / "attempts.jsonl").open("x") as attempts, (args.run_dir / "baseline.jsonl").open("x") as baseline, (args.run_dir / "candidate.jsonl").open("x") as candidate:
        for index, record in enumerate(records, 1):
            result = refine_once(record, generator, total_budget_ms=args.budget_ms,
                                 max_refinement_ms=args.timeout * 1000)
            for file, value in [(baseline, result["baseline"]), (candidate, result["candidate"]), (attempts, result)]:
                file.write(json.dumps(value, ensure_ascii=False) + "\n"); file.flush()
            statuses[result["status"]] += 1; reasons[result["reason"]] += 1; calls += result["calls"]
            print(index, result["status"], result["reason"], flush=True)
    summary = {"total": len(records), "statuses": dict(statuses), "reasons": dict(reasons), "model_calls": calls,
               "semantic_acceptance": "not_evaluated; independent paired review required"}
    (args.run_dir / "summary.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary), flush=True)
    return 2 if statuses["failed"] or not statuses["produced"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
