"""Schema validation for eval case JSON files (spec §13 step 7).

Ported from crates/eval-runner/src/validate.rs. Run before any case to catch
shape errors early.
"""
from __future__ import annotations

ALLOWED_DIMS = {
    "tool_accuracy",
    "task_completion",
    "response_quality",
    "context_retention",
    "error_recovery",
    "language_adherence",
}

KNOWN_QUERY_TYPES = {
    "device_exists",
    "device_count",
    "rule_exists",
    "rule_enabled",
    "agent_exists",
    "agent_status",
    "agent_execution_count",
    "transform_exists",
    "transform_count",
    "dashboard_exists",
    "dashboard_component_count",
    "channel_exists",
    "message_count",
    "push_enabled",
    "extension_installed",
    "widget_exists",
    "llm_backend_exists",
    "settings_value",
    "device_type_has_metric",
}


def validate_case(case: dict) -> list[str]:
    """Return list of error strings; empty list = valid."""
    errors: list[str] = []

    # Rule 1: ID must not have zh-/en- prefix (IDs are language-agnostic).
    cid = case.get("id", "")
    if isinstance(cid, str) and (cid.startswith("zh-") or cid.startswith("en-")):
        errors.append(f"id '{cid}' has lang prefix — IDs are language-agnostic")

    # Rule 2: applies[] dims must all be known.
    applies = case.get("applies", []) or []
    if not isinstance(applies, list):
        errors.append("applies must be a list")
    else:
        for d in applies:
            if d not in ALLOWED_DIMS:
                errors.append(f"applies '{d}' is not a known dimension")

    # Rule 3: single_turn cases must NOT include context_retention.
    scenario = case.get("scenario_type", "")
    if scenario == "single_turn" and "context_retention" in applies:
        errors.append("context_retention not applicable to single_turn")

    # Rule 4: per_turn length must equal turns length.
    turns = case.get("turns", []) or []
    per_turn = (case.get("expectations") or {}).get("per_turn", []) or []
    if len(turns) != len(per_turn):
        errors.append(
            f"per_turn length {len(per_turn)} != turns length {len(turns)}"
        )

    # Rule 5: state_queries types must be known.
    sqs = case.get("state_queries") or []
    if isinstance(sqs, list):
        for q in sqs:
            t = q.get("type", "") if isinstance(q, dict) else ""
            if t not in KNOWN_QUERY_TYPES:
                errors.append(f"state_query type '{t}' not supported")

    return errors
